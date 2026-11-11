//! Multiplayer smoke test against the real binary: a dozen bots log in,
//! walk and chat at once, a flooder and an oversized frame are dropped
//! without disturbing the others, and SIGTERM saves everything.
//! Needs `ZIRCON_ASSETS`; skipped otherwise.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mir_proto::{
    decode_frame, encode, Class, ClientMessage, Direction, Gender, NewCharacterResult,
    ServerMessage, PROTOCOL_VERSION,
};

const BOTS: usize = 12;

struct Bot {
    stream: TcpStream,
    buf: Vec<u8>,
    welcomed: bool,
    moves: usize,
    pongs: usize,
    closed: bool,
    received: Vec<ServerMessage>,
}

impl Bot {
    fn connect(port: u16) -> Bot {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_millis(30)))
            .unwrap();
        stream.set_nodelay(true).unwrap();
        Bot {
            stream,
            buf: Vec::new(),
            welcomed: false,
            moves: 0,
            pongs: 0,
            closed: false,
            received: Vec::new(),
        }
    }

    fn send(&mut self, m: &ClientMessage) {
        if self.stream.write_all(&encode(m).unwrap()).is_err() {
            self.closed = true;
        }
    }

    /// Read what has arrived; returns the decoded messages.
    fn pump(&mut self) -> Vec<ServerMessage> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    self.closed = true;
                    break;
                }
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => break,
                Err(_) => {
                    self.closed = true;
                    break;
                }
            }
        }
        while let Ok(Some(m)) = decode_frame::<ServerMessage>(&mut self.buf) {
            match &m {
                ServerMessage::Welcome { .. } => self.welcomed = true,
                ServerMessage::ObjectMove { .. } | ServerMessage::MoveDenied { .. } => {
                    self.moves += 1
                }
                ServerMessage::Pong { .. } => self.pongs += 1,
                _ => {}
            }
            self.received.push(m.clone());
            out.push(m);
        }
        out
    }

    /// Pump until `pred` matches a message or the deadline passes.
    fn wait_for(
        &mut self,
        secs: u64,
        pred: impl Fn(&ServerMessage) -> bool,
    ) -> Option<ServerMessage> {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            for m in self.pump() {
                if pred(&m) {
                    return Some(m);
                }
            }
            if self.closed {
                return None;
            }
        }
        None
    }

    fn enter_world(&mut self, i: usize) {
        self.send(&ClientMessage::Hello {
            version: PROTOCOL_VERSION,
        });
        self.wait_for(10, |m| matches!(m, ServerMessage::Connected))
            .expect("connected");
        let email = format!("bot{i}@load.test");
        self.send(&ClientMessage::NewAccount {
            email: email.clone(),
            password: "secret123".into(),
        });
        self.wait_for(10, |m| matches!(m, ServerMessage::NewAccountResult(_)))
            .expect("account result");
        self.send(&ClientMessage::Login {
            email,
            password: "secret123".into(),
        });
        self.wait_for(10, |m| matches!(m, ServerMessage::LoginResult(_)))
            .expect("login result");
        self.send(&ClientMessage::NewCharacter {
            name: format!("Bot{i}"),
            class: Class::Warrior,
            gender: Gender::Male,
            hair: 1,
        });
        let created = self
            .wait_for(10, |m| matches!(m, ServerMessage::NewCharacterResult(_)))
            .expect("character result");
        let id = match created {
            ServerMessage::NewCharacterResult(NewCharacterResult::Success { character }) => {
                character.id
            }
            other => panic!("character creation failed: {other:?}"),
        };
        self.send(&ClientMessage::StartGame { id });
        self.wait_for(20, |m| matches!(m, ServerMessage::Welcome { .. }))
            .expect("welcome");
    }
}

/// Kills the server when the test ends, even on a panic.
struct Server(std::process::Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn a_dozen_bots_play_while_abusers_are_dropped_and_sigterm_saves() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let data = std::env::temp_dir().join(format!("zircon-load-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let port = free_port();
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_mir-server"))
            .args(["--port", &port.to_string(), "--data"])
            .arg(&data)
            .env("ZIRCON_ASSETS", &assets)
            .env(
                "RUST_LOG",
                std::env::var("ZIRCON_LOAD_LOG").unwrap_or_else(|_| "warn".into()),
            )
            .stdout(if std::env::var_os("ZIRCON_LOAD_LOG").is_some() {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn server"),
    );
    // Wait for the listener.
    let start = Instant::now();
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(200));
    }

    let mut bots: Vec<Bot> = (0..BOTS).map(|_| Bot::connect(port)).collect();
    for (i, b) in bots.iter_mut().enumerate() {
        b.enter_world(i);
    }
    assert!(bots.iter().all(|b| b.welcomed));

    // Three seconds of everyone walking, chatting and pinging at once.
    let dirs = Direction::ALL;
    let end = Instant::now() + Duration::from_secs(3);
    let mut step = 0u32;
    while Instant::now() < end {
        for (i, b) in bots.iter_mut().enumerate() {
            b.send(&ClientMessage::Move {
                direction: dirs[(step as usize + i) % dirs.len()],
                run: false,
            });
            if step.is_multiple_of(6) {
                b.send(&ClientMessage::Chat {
                    text: format!("hello from bot {i}"),
                });
                b.send(&ClientMessage::Ping { nonce: step });
            }
            b.pump();
        }
        step += 1;
        std::thread::sleep(Duration::from_millis(150));
    }
    for b in bots.iter_mut() {
        b.pump();
    }
    assert!(bots.iter().all(|b| !b.closed), "a bot was disconnected");
    assert!(bots.iter().all(|b| b.moves > 0), "a bot never moved");
    assert!(bots.iter().all(|b| b.pongs > 0), "a bot got no pong");

    // A flooder (hundreds of chat lines in one burst) is dropped.
    let mut flooder = Bot::connect(port);
    flooder.enter_world(BOTS);
    for n in 0..600 {
        flooder.send(&ClientMessage::Chat {
            text: format!("spam {n}"),
        });
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while !flooder.closed && Instant::now() < deadline {
        flooder.pump();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(flooder.closed, "the flooder was not disconnected");

    // A frame larger than the client cap is dropped before it is read.
    let mut oversize = Bot::connect(port);
    let mut frame = (200u32 * 1024).to_le_bytes().to_vec();
    frame.extend(std::iter::repeat_n(0u8, 4096));
    let _ = oversize.stream.write_all(&frame);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !oversize.closed && Instant::now() < deadline {
        oversize.pump();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(oversize.closed, "the oversized frame was accepted");

    // The honest bots are still served.
    for b in bots.iter_mut() {
        let before = b.pongs;
        b.send(&ClientMessage::Ping { nonce: 99 });
        assert!(
            b.wait_for(5, |m| matches!(m, ServerMessage::Pong { nonce: 99 }))
                .is_some(),
            "bot lost service (pongs {before})"
        );
    }

    // SIGTERM saves every character before exit.
    #[cfg(unix)]
    let mut server = server;
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &server.0.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(Some(_)) = server.0.try_wait() {
                break;
            }
            if Instant::now() > deadline {
                panic!("server did not exit on SIGTERM");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let saved = std::fs::read_to_string(data.join("accounts.json")).expect("accounts.json");
        for i in 0..BOTS {
            assert!(saved.contains(&format!("\"Bot{i}\"")), "Bot{i} not saved");
        }
    }
    drop(server);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn store_appearance_and_discipline_work_over_the_wire_and_persist() {
    use mir_proto::{slot, Appearance, Grid, LoginResult};
    use mir_server::accounts::Accounts;
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
        return;
    };
    let data = std::env::temp_dir().join(format!("zircon-features-{}", std::process::id()));
    std::fs::create_dir_all(&data).unwrap();
    let path = data.join("accounts.json");
    let _ = std::fs::remove_file(&path);
    let mut accounts = Accounts::load(&path).unwrap();
    let account = accounts
        .create("features@local.test", "feature123")
        .unwrap();
    let rec = accounts
        .new_character(account, "FeatureBot", Class::Warrior, Gender::Male, 1)
        .unwrap();
    let character = rec.id;
    let rec = accounts.character_mut(account, character).unwrap();
    rec.level = 86;
    rec.gold = 100_000_000;
    accounts.mark_dirty();
    accounts.save_if_dirty().unwrap();
    let port = free_port();
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_mir-server"))
            .args(["--port", &port.to_string(), "--data"])
            .arg(&data)
            .env("ZIRCON_ASSETS", &assets)
            .env("ZIRCON_DEV_GAMEGOLD", "100000")
            .env(
                "ZIRCON_DEV_ITEMS",
                "Hair Change;Gender Change;Armour Dye;Name Change",
            )
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut b = Bot::connect(port);
    b.send(&ClientMessage::Hello {
        version: PROTOCOL_VERSION,
    });
    b.wait_for(5, |m| matches!(m, ServerMessage::Connected))
        .unwrap();
    b.send(&ClientMessage::Login {
        email: "features@local.test".into(),
        password: "feature123".into(),
    });
    b.wait_for(5, |m| {
        matches!(m, ServerMessage::LoginResult(LoginResult::Success { .. }))
    })
    .unwrap();
    b.send(&ClientMessage::StartGame { id: character });
    b.wait_for(10, |m| matches!(m, ServerMessage::Welcome { .. }))
        .unwrap();
    // A ping drains every entry packet even when the welcome arrived in a separate read.
    b.send(&ClientMessage::Ping { nonce: 10 });
    b.wait_for(5, |m| matches!(m, ServerMessage::Pong { nonce: 10 }))
        .unwrap();
    let stock = b
        .received
        .iter()
        .find_map(|m| {
            if let ServerMessage::StoreStock(s) = m {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(stock.len(), 92);
    let row = stock
        .iter()
        .find(|e| e.available && e.price > 0 && e.duration == 0)
        .unwrap()
        .clone();
    b.send(&ClientMessage::MarketStoreBuy {
        index: row.index,
        count: 1,
        hunt_gold: false,
    });
    b.wait_for(
        5,
        |m| matches!(m,ServerMessage::Say{text,..} if text.starts_with("You bought")),
    )
    .unwrap();
    let (purchased_slot, purchased) = b
        .received
        .iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::ItemChanged {
                grid: Grid::Inventory,
                slot,
                item: Some(i),
            } if i.info == row.item => Some((*slot, i.clone())),
            _ => None,
        })
        .expect("purchased instance arrived on the wire");
    assert!(purchased.locked && purchased.worthless && !purchased.bound);
    for locked in [false, true] {
        b.send(&ClientMessage::ItemLock {
            grid: Grid::Inventory,
            slot: purchased_slot,
            locked,
        });
        b.wait_for(5, |m| matches!(m, ServerMessage::ItemChanged { grid: Grid::Inventory, slot, item: Some(i) }
            if *slot == purchased_slot && i.id == purchased.id && i.locked == locked && i.worthless && !i.bound))
            .expect("lock change and unchanged purchase flags echoed over protocol v5");
    }
    b.send(&ClientMessage::HairChange {
        hair: 3,
        colour: 0xff123456,
    });
    b.wait_for(5, |m| {
        matches!(
            m,
            ServerMessage::ObjectAppearance {
                appearance: Appearance::Player {
                    hair: 3,
                    hair_colour: 0xff123456,
                    ..
                },
                ..
            }
        )
    })
    .unwrap();
    b.send(&ClientMessage::ArmourDye { colour: 0xff4080ff });
    b.wait_for(5, |m| {
        matches!(
            m,
            ServerMessage::ObjectAppearance {
                appearance: Appearance::Player {
                    armour_colour: 0xff4080ff,
                    ..
                },
                ..
            }
        )
    })
    .unwrap();
    b.send(&ClientMessage::ItemMove {
        from: Grid::Equipment,
        from_slot: slot::ARMOUR as u8,
        to: Grid::Inventory,
        to_slot: 30,
    });
    b.wait_for(5,|m|matches!(m,ServerMessage::ItemChanged{grid:Grid::Equipment,slot:s,item:None} if *s==slot::ARMOUR as u8)).unwrap();
    b.send(&ClientMessage::GenderChange {
        gender: Gender::Female,
        hair: 2,
        colour: 0xffabcdef,
    });
    b.wait_for(5, |m| {
        matches!(
            m,
            ServerMessage::ObjectAppearance {
                appearance: Appearance::Player {
                    gender: Gender::Female,
                    ..
                },
                ..
            }
        )
    })
    .unwrap();
    b.send(&ClientMessage::NameChange {
        name: "RenamedBot".into(),
    });
    b.wait_for(5,|m|matches!(m,ServerMessage::ObjectAppearance{appearance:Appearance::Player{name,..},..} if name=="RenamedBot")).unwrap();
    b.send(&ClientMessage::IncreaseDiscipline);
    b.wait_for(
        5,
        |m| matches!(m,ServerMessage::StatsChanged(s) if s.discipline==1 && s.max_fp>0),
    )
    .unwrap();
    // Logging out invokes the same snapshot and account save used on disconnect.
    drop(b);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let saved = Accounts::load(&path).unwrap();
        if saved
            .character(account, character)
            .is_some_and(|r| r.name == "RenamedBot" && r.discipline == 1)
        {
            break;
        }
        assert!(Instant::now() < deadline, "changes were not persisted");
        std::thread::sleep(Duration::from_millis(100));
    }
    let saved = Accounts::load(&path).unwrap();
    let r = saved.character(account, character).unwrap();
    assert_eq!(
        (r.gender, r.hair, r.hair_colour),
        (Gender::Female, 2, 0xffabcdef)
    );
    assert!(r.items.iter().any(|s| s.item.colour == 0xff4080ff));
    assert!(r
        .items
        .iter()
        .any(|s| s.item.id == purchased.id && s.item.locked && s.item.worthless && !s.item.bound));
    assert!(r.gold < 100_000_000);
    drop(server);
    // A fresh server reads the saved flags and sends them in the entry inventory.
    let port = free_port();
    let restarted = Server(
        Command::new(env!("CARGO_BIN_EXE_mir-server"))
            .args(["--port", &port.to_string(), "--data"])
            .arg(&data)
            .env("ZIRCON_ASSETS", &assets)
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut reconnected = Bot::connect(port);
    reconnected.send(&ClientMessage::Hello {
        version: PROTOCOL_VERSION,
    });
    reconnected
        .wait_for(5, |m| matches!(m, ServerMessage::Connected))
        .unwrap();
    reconnected.send(&ClientMessage::Login {
        email: "features@local.test".into(),
        password: "feature123".into(),
    });
    reconnected
        .wait_for(5, |m| {
            matches!(m, ServerMessage::LoginResult(LoginResult::Success { .. }))
        })
        .unwrap();
    reconnected.send(&ClientMessage::StartGame { id: character });
    reconnected.wait_for(10, |m| matches!(m, ServerMessage::Inventory { inventory, .. }
        if inventory.iter().any(|(slot, i)| *slot == purchased_slot && i.id == purchased.id && i.locked && i.worthless && !i.bound)))
        .expect("restarted server preserves purchase flags");
    drop(reconnected);
    drop(restarted);
    // Optional disposable account fixture for native tooltip QA, never a personal save.
    if let Some(qa) = std::env::var_os("ZIRCON_STORE_QA_DIR") {
        let qa = std::path::PathBuf::from(qa);
        std::fs::create_dir(&qa).expect("QA output directory must be new");
        std::fs::copy(&path, qa.join("accounts.json")).unwrap();
        println!("Store QA fixture: {} ; login features@local.test:feature123 ; character RenamedBot ; inventory slot {}", qa.display(), purchased_slot);
    }
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn weapon_crafting_at_the_smith_works_over_the_wire_and_persists() {
    use mir_proto::{Appearance, Grid, LoginResult, Point};
    use mir_server::{
        accounts::Accounts,
        data::GameData,
        items::{StatSource, StoredItem, UserItem},
    };
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
        return;
    };
    let defs =
        GameData::load(std::path::PathBuf::from(&assets).join("../Database/System.db")).unwrap();
    let data = std::env::temp_dir().join(format!("zircon-craft-wire-{}", std::process::id()));
    std::fs::create_dir_all(&data).unwrap();
    let path = data.join("accounts.json");
    let _ = std::fs::remove_file(&path);
    let mut accounts = Accounts::load(&path).unwrap();
    let account = accounts.create("craft@local.test", "craft123").unwrap();
    let character = accounts
        .new_character(account, "CraftBot", Class::Warrior, Gender::Male, 1)
        .unwrap()
        .id;
    let rec = accounts.character_mut(account, character).unwrap();
    rec.level = 70;
    rec.gold = 100_000_000;
    rec.map = "D3400".into();
    rec.location = Point { x: 123, y: 53 };
    for (slot, info) in [967, 978, 979, 980, 981, 982, 983].into_iter().enumerate() {
        let mut item = UserItem::fresh(&defs, info, 2).unwrap();
        item.id = slot as u32 + 100;
        rec.items.push(StoredItem {
            grid: Grid::Inventory,
            slot: slot as u8,
            item,
        });
    }
    let mut enhanced = UserItem::fresh(&defs, 968, 1).unwrap();
    enhanced.id = 200;
    enhanced.refinable = true;
    enhanced.add_stat(9, 7, StatSource::Enhancement);
    enhanced.add_stat(9, 11, StatSource::Added);
    enhanced.add_stat(9, 3, StatSource::Refine);
    rec.items.push(StoredItem {
        grid: Grid::Inventory,
        slot: 7,
        item: enhanced,
    });
    rec.next_item_id = 201;
    accounts.mark_dirty();
    accounts.save_if_dirty().unwrap();
    let port = free_port();
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_mir-server"))
            .args(["--port", &port.to_string(), "--data"])
            .arg(&data)
            .env("ZIRCON_ASSETS", &assets)
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut b = Bot::connect(port);
    b.send(&ClientMessage::Hello {
        version: PROTOCOL_VERSION,
    });
    b.wait_for(5, |m| matches!(m, ServerMessage::Connected))
        .unwrap();
    b.send(&ClientMessage::Login {
        email: "craft@local.test".into(),
        password: "craft123".into(),
    });
    b.wait_for(5, |m| {
        matches!(m, ServerMessage::LoginResult(LoginResult::Success { .. }))
    })
    .unwrap();
    b.send(&ClientMessage::StartGame { id: character });
    b.wait_for(10, |m| matches!(m, ServerMessage::Welcome { .. }))
        .unwrap();
    b.send(&ClientMessage::Ping { nonce: 77 });
    b.wait_for(5, |m| matches!(m, ServerMessage::Pong { nonce: 77 }))
        .unwrap();
    let npc=b.received.iter().find_map(|m|match m {ServerMessage::ObjectShow(s) if matches!(&s.appearance,Appearance::Npc {name,..} if name=="Weapon Smith")=>Some(s.id), _=>None}).expect("Weapon Smith visible");
    b.send(&ClientMessage::NpcCall { id: npc });
    b.wait_for(5, |m| matches!(m, ServerMessage::NpcResponse { .. }))
        .unwrap();
    b.send(&ClientMessage::NpcButton { button: 1 });
    b.wait_for(5, |m| {
        matches!(
            m,
            ServerMessage::NpcResponse {
                dialog_type: 14,
                ..
            }
        )
    })
    .unwrap();
    b.send(&ClientMessage::NpcWeaponCraft {
        base: (0, 100),
        materials: std::array::from_fn(|i| Some((i as u8 + 1, i as u32 + 101))),
        class: Class::Wizard,
    });
    b.wait_for(
        5,
        |m| matches!(m,ServerMessage::Say {text,..} if text=="Your weapon has been crafted."),
    )
    .unwrap();
    assert!(b
        .received
        .iter()
        .any(|m| matches!(m, ServerMessage::GoldChanged { gold: 99_000_000 })));
    assert!(b.received.iter().any(|m|matches!(m,ServerMessage::ItemChanged {item:Some(i),..} if i.info==969 && !i.added.is_empty())));
    b.send(&ClientMessage::NpcWeaponCraft {
        base: (7, 200),
        materials: [None; 6],
        class: Class::Warrior,
    });
    b.wait_for(
        5,
        |m| matches!(m,ServerMessage::Say {text,..} if text=="Your weapon has been crafted."),
    )
    .unwrap();
    assert!(b.received.iter().any(|m|matches!(m,ServerMessage::ItemChanged { item:Some(i),.. } if i.id==200 && i.added==vec![(9,7)] && i.enhancements==vec![(9,7)] && !i.refinable && !i.non_refinable)));
    let expected_gold = 99_000_000
        - match defs.items[&968].rarity {
            0 => 30_000_000,
            1 => 60_000_000,
            _ => 80_000_000,
        };
    drop(b);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let saved = Accounts::load(&path).unwrap();
        if let Some(rec) = saved
            .character(account, character)
            .filter(|r| r.gold == expected_gold)
        {
            assert!(rec
                .items
                .iter()
                .any(|s| s.item.info == 969 && !s.item.added.is_empty()));
            let rerolled = &rec.items.iter().find(|s| s.item.id == 200).unwrap().item;
            assert_eq!(rerolled.bonus_totals(), vec![(9, 7)]);
            assert!(rerolled
                .added
                .iter()
                .all(|s| s.source == StatSource::Enhancement));
            assert!(!rerolled.refinable);
            for info in [967, 978, 979, 980, 981, 982, 983] {
                assert_eq!(
                    rec.items
                        .iter()
                        .find(|s| s.item.info == info)
                        .unwrap()
                        .item
                        .count,
                    1
                );
            }
            break;
        }
        assert!(Instant::now() < deadline, "craft not saved");
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(server);
    std::fs::remove_dir_all(data).unwrap();
}
