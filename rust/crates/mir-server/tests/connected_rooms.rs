//! Real login and authoritative walking through a generated physical room neck.
//! Run after exporting an editor fixture with ZIRCON_CONNECTED_QA_DIR:
//! ZIRCON_CONNECTED_MAP=/tmp/zircon-connected-final/connected-3-4.map
//! ZIRCON_ASSETS=... cargo test -p mir-server --test connected_rooms -- --nocapture
use mir_formats::{
    map::MapFile,
    mirdb::{MirDb, Value},
};
use mir_proto::{
    decode_frame, encode, Class, ClientMessage, Direction, Gender, NewCharacterResult,
    ServerMessage, PROTOCOL_VERSION,
};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    path: PathBuf,
    server: Option<Child>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(server) = &mut self.server {
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
struct Bot {
    stream: TcpStream,
    buffer: Vec<u8>,
}
impl Bot {
    fn send(&mut self, message: ClientMessage) {
        self.stream.write_all(&encode(&message).unwrap()).unwrap();
    }
    fn wait(&mut self, predicate: impl Fn(&ServerMessage) -> bool) -> ServerMessage {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            while let Some(message) = decode_frame::<ServerMessage>(&mut self.buffer).unwrap() {
                if predicate(&message) {
                    return message;
                }
            }
            assert!(
                Instant::now() < deadline,
                "Timed out waiting for server response"
            );
            let mut bytes = [0; 8192];
            match self.stream.read(&mut bytes) {
                Ok(0) => panic!("Server disconnected"),
                Ok(count) => self.buffer.extend_from_slice(&bytes[..count]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => panic!("Socket read: {error}"),
            }
        }
    }
    fn enter(&mut self, index: usize) -> (mir_proto::ObjectId, (i32, i32)) {
        self.send(ClientMessage::Hello {
            version: PROTOCOL_VERSION,
        });
        self.wait(|m| matches!(m, ServerMessage::Connected));
        self.send(ClientMessage::NewAccount {
            email: format!("neck{index}@qa.test"),
            password: "necktest123".into(),
        });
        self.wait(|m| matches!(m, ServerMessage::NewAccountResult(_)));
        self.send(ClientMessage::Login {
            email: format!("neck{index}@qa.test"),
            password: "necktest123".into(),
        });
        self.wait(|m| matches!(m, ServerMessage::LoginResult(_)));
        self.send(ClientMessage::NewCharacter {
            name: format!("NeckTest{index}"),
            class: Class::Warrior,
            gender: Gender::Male,
            hair: 1,
        });
        let id = match self.wait(|m| matches!(m, ServerMessage::NewCharacterResult(_))) {
            ServerMessage::NewCharacterResult(NewCharacterResult::Success { character }) => {
                character.id
            }
            other => panic!("Character creation failed: {other:?}"),
        };
        self.send(ClientMessage::StartGame { id });
        match self.wait(|m| matches!(m, ServerMessage::Welcome { .. })) {
            ServerMessage::Welcome { id, location, .. } => (id, (location.x, location.y)),
            _ => unreachable!(),
        }
    }
}

#[test]
fn player_walks_from_one_generated_room_through_the_neck_into_the_other() {
    let (Some(assets), Some(input)) = (
        std::env::var_os("ZIRCON_ASSETS"),
        std::env::var_os("ZIRCON_CONNECTED_MAP"),
    ) else {
        return;
    };
    let assets = PathBuf::from(assets);
    let input = PathBuf::from(input);
    let source = std::fs::read(&input).unwrap();
    let map = MapFile::load(&input).unwrap();
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(input.with_extension("cave.json")).unwrap()).unwrap();
    let rooms = metadata[0]["wall_rooms"].as_u64().unwrap();
    assert!(
        (2..=3).contains(&rooms),
        "Supply a generated connected-room fixture"
    );
    // The middle-third's narrowest open column identifies the physical neck.
    let width = i32::from(map.width);
    let height = i32::from(map.height);
    let neck_index = std::env::var("ZIRCON_CONNECTED_NECK")
        .ok()
        .and_then(|n| n.parse::<u8>().ok())
        .unwrap_or(0);
    let columns = if rooms == 3 {
        assert!(neck_index < 2);
        if neck_index == 0 {
            width / 6..width / 2
        } else {
            width / 2..width * 5 / 6
        }
    } else {
        width / 3..width * 2 / 3
    };
    let neck = columns
        .filter_map(|x| {
            let ys: Vec<_> = (0..height).filter(|&y| map.is_walkable(x, y)).collect();
            (!ys.is_empty()).then_some((ys.len(), (x - width / 2).abs(), x, ys))
        })
        .min_by_key(|(count, distance, _, _)| (*count, *distance))
        .unwrap();
    let (neck_x, neck_y) = (neck.2, neck.3[neck.3.len() / 2]);
    let start = (neck_x - 8, neck_y);
    let destination = (neck_x + 8, neck_y);
    assert!(map.is_walkable(start.0, start.1) && map.is_walkable(destination.0, destination.1));
    let mut previous = BTreeMap::from([(start, start)]);
    let mut queue = VecDeque::from([start]);
    while let Some((x, y)) = queue.pop_front() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let point = (x + dx, y + dy);
            if map.is_walkable(point.0, point.1) && !previous.contains_key(&point) {
                previous.insert(point, (x, y));
                queue.push_back(point);
            }
        }
    }
    let mut route = vec![destination];
    while *route.last().unwrap() != start {
        route.push(previous[route.last().unwrap()]);
    }
    route.reverse();
    assert!(route.len() <= 64);
    assert!(route.iter().any(|&(x, _)| x == neck_x));
    let path = std::env::temp_dir().join(format!(
        "zircon-neck-server-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    let mut fixture = Fixture { path, server: None };
    let map_dir = fixture.path.join("Map");
    std::fs::create_dir(&map_dir).unwrap();
    std::fs::write(map_dir.join("ConnectedRoomsQa.map"), &source).unwrap();
    let database_path = assets.join("../Database/System.db");
    let original_db = std::fs::read(&database_path).unwrap();
    let mut db = MirDb::parse(&original_db).unwrap();
    db.collection_mut("MapInfo").unwrap().push_record(vec![
        ("FileName", Value::Str("ConnectedRoomsQa".into())),
        ("Description", Value::Str("Physical room neck QA".into())),
        ("Light", Value::Int(1)),
    ]);
    // Isolate movement from monster collision and teleport triggers in this private snapshot.
    db.collection_mut("RespawnInfo").unwrap().records.clear();
    db.collection_mut("MovementInfo").unwrap().records.clear();
    let database = fixture.path.join("System.db");
    db.save(&database).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    fixture.server = Some(
        Command::new(env!("CARGO_BIN_EXE_mir-server"))
            .args(["--port", &port.to_string(), "--db"])
            .arg(&database)
            .arg("--data")
            .arg(fixture.path.join("Save"))
            .env("ZIRCON_ASSETS", &assets)
            .env("ZIRCON_MAP_DIR", &map_dir)
            .env(
                "ZIRCON_DEV_START",
                format!("ConnectedRoomsQa:{},{}", start.0, start.1),
            )
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    stream.set_nodelay(true).unwrap();
    let mut bot = Bot {
        stream,
        buffer: vec![],
    };
    let (object, location) = bot.enter(0);
    assert_eq!(location, start);
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    stream.set_nodelay(true).unwrap();
    let mut observer = Bot {
        stream,
        buffer: vec![],
    };
    let (_, observer_location) = observer.enter(1);
    assert!(route.iter().all(|&(x, y)| (x - observer_location.0)
        .abs()
        .max((y - observer_location.1).abs())
        <= 18));
    for cells in route.windows(2) {
        let direction = match (cells[1].0 - cells[0].0, cells[1].1 - cells[0].1) {
            (1, 0) => Direction::Right,
            (-1, 0) => Direction::Left,
            (0, 1) => Direction::Down,
            (0, -1) => Direction::Up,
            _ => unreachable!(),
        };
        std::thread::sleep(Duration::from_millis(mir_proto::rules::MOVE_TIME + 50));
        bot.send(ClientMessage::Move {
            direction,
            run: false,
        });
        match observer.wait(|m| {
            matches!(m,ServerMessage::ObjectMove{id,..}if *id==object)
                || matches!(
                    m,
                    ServerMessage::MoveDenied { .. } | ServerMessage::Welcome { .. }
                )
        }) {
            ServerMessage::ObjectMove { from, to, .. } => {
                assert_eq!((from.x, from.y), cells[0]);
                assert_eq!((to.x, to.y), cells[1]);
            }
            other => panic!("Physical movement failed or teleported: {other:?}"),
        }
    }
    assert_eq!(std::fs::read(database_path).unwrap(), original_db);
    assert_eq!(std::fs::read(input).unwrap(), source);
    println!("Authoritative physical traversal: {} acknowledged steps from {start:?} through column {neck_x} to {destination:?}; no portal records",route.len()-1);
}

#[test]
fn ordinary_room_door_toggle_is_authoritative_and_reloads_closed() {
    let (Some(assets), Some(input), Some(database_path)) = (
        std::env::var_os("ZIRCON_ASSETS"),
        std::env::var_os("ZIRCON_ROOM_DOOR_MAP"),
        std::env::var_os("ZIRCON_ROOM_DOOR_DB"),
    ) else {
        return;
    };
    let assets = PathBuf::from(assets);
    let input = PathBuf::from(input);
    let database_path = PathBuf::from(database_path);
    let source_map = std::fs::read(&input).unwrap();
    let source_db = std::fs::read(&database_path).unwrap();
    let map = MapFile::load(&input).unwrap();
    let doors = mir_formats::room_door::RoomDoors::load(&input, &map)
        .unwrap()
        .unwrap();
    let [x, y] = doors.doors[0].center;
    let start = (x - 3, y);
    assert!(map.is_walkable(start.0, start.1) && map.is_walkable(x + 1, y));
    let folder =
        std::env::temp_dir().join(format!("zircon-room-door-server-{}", std::process::id()));
    std::fs::create_dir(&folder).unwrap();
    let mut fixture = Fixture {
        path: folder,
        server: None,
    };
    let map_dir = fixture.path.join("Map");
    std::fs::create_dir(&map_dir).unwrap();
    let private_map = map_dir.join("RoomDoorQa.map");
    std::fs::write(&private_map, &source_map).unwrap();
    std::fs::copy(
        input.with_extension("doors.json"),
        private_map.with_extension("doors.json"),
    )
    .unwrap();
    let mut db = MirDb::parse(&source_db).unwrap();
    db.collection_mut("MapInfo").unwrap().push_record(vec![
        ("FileName", Value::Str("RoomDoorQa".into())),
        ("Description", Value::Str("Room door QA".into())),
        ("Light", Value::Int(1)),
    ]);
    db.collection_mut("RespawnInfo").unwrap().records.clear();
    db.collection_mut("MovementInfo").unwrap().records.clear();
    let private_db = fixture.path.join("System.db");
    db.save(&private_db).unwrap();
    for round in 0..2 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        fixture.server = Some(
            Command::new(env!("CARGO_BIN_EXE_mir-server"))
                .args(["--port", &port.to_string(), "--db"])
                .arg(&private_db)
                .arg("--data")
                .arg(fixture.path.join("Save"))
                .env("ZIRCON_ASSETS", &assets)
                .env("ZIRCON_MAP_DIR", &map_dir)
                .env(
                    "ZIRCON_DEV_START",
                    format!("RoomDoorQa:{},{}", start.0, start.1),
                )
                .env("RUST_LOG", "warn")
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        let stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(_) => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        stream.set_nodelay(true).unwrap();
        let mut bot = Bot {
            stream,
            buffer: vec![],
        };
        let (actor, location) = bot.enter(round + 20);
        assert_eq!(location, start);
        let door = match bot
            .wait(|message| matches!(message, ServerMessage::ObjectShow(state) if state.room_door))
        {
            ServerMessage::ObjectShow(state) => {
                assert_eq!((state.location.x, state.location.y), (x, y));
                assert_eq!(
                    state.direction.index(),
                    0,
                    "door must start closed after reload"
                );
                state.id
            }
            _ => unreachable!(),
        };
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        stream.set_nodelay(true).unwrap();
        let mut observer = Bot {
            stream,
            buffer: vec![],
        };
        observer.enter(round + 30);
        if round == 0 {
            bot.send(ClientMessage::RoomDoorToggle { id: door });
            assert!(matches!(bot.wait(|m| matches!(m, ServerMessage::Chat {text} if text == "Move closer to the door.")), ServerMessage::Chat {..}));
            std::thread::sleep(Duration::from_millis(mir_proto::rules::MOVE_TIME + 50));
            bot.send(ClientMessage::Move {
                direction: Direction::Right,
                run: false,
            });
            observer.wait(|m| matches!(m, ServerMessage::ObjectMove {id, ..} if *id == actor));
            bot.send(ClientMessage::RoomDoorToggle { id: door });
            observer.wait(|m| matches!(m, ServerMessage::ObjectAttack {id, direction, ..} if *id == door && direction.index() == 7));
            for _ in 0..2 {
                std::thread::sleep(Duration::from_millis(mir_proto::rules::MOVE_TIME + 50));
                bot.send(ClientMessage::Move {
                    direction: Direction::Right,
                    run: false,
                });
                observer.wait(|m| matches!(m, ServerMessage::ObjectMove {id, ..} if *id == actor));
            }
            bot.send(ClientMessage::RoomDoorToggle { id: door });
            bot.wait(
                |m| matches!(m, ServerMessage::Chat {text} if text == "The doorway is occupied."),
            );
            std::thread::sleep(Duration::from_millis(mir_proto::rules::MOVE_TIME + 50));
            bot.send(ClientMessage::Move {
                direction: Direction::Right,
                run: false,
            });
            observer.wait(|m| matches!(m, ServerMessage::ObjectMove {id, ..} if *id == actor));
            bot.send(ClientMessage::RoomDoorToggle { id: door });
            observer.wait(|m| matches!(m, ServerMessage::ObjectAttack {id, direction, ..} if *id == door && direction.index() == 0));
            std::thread::sleep(Duration::from_millis(mir_proto::rules::MOVE_TIME + 50));
            bot.send(ClientMessage::Move {
                direction: Direction::Left,
                run: false,
            });
            bot.wait(|m| matches!(m, ServerMessage::MoveDenied { .. }));
        }
        drop(bot);
        drop(observer);
        let mut server = fixture.server.take().unwrap();
        server.kill().unwrap();
        server.wait().unwrap();
    }
    assert_eq!(std::fs::read(&input).unwrap(), source_map);
    assert_eq!(std::fs::read(database_path).unwrap(), source_db);
    println!("Ordinary room door: remote toggle rejected; open crossed; occupied close rejected; closed blocked; reload closed");
}
