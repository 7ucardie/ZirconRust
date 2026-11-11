//! Rust Zircon game server.
//!
//! ```text
//! mir-server [--assets DIR] [--db FILE] [--port N] [--map FILE] [--inspect]
//!   ZIRCON_ASSETS  client root containing Map/ (default ~/zircon-assets/Client)
//!   ZIRCON_DB      System.db path (default <assets>/../Database/System.db)
//!   ZIRCON_DROP_SPECIALS  1 enables random +1 stats on equipment drops (default 0)
//!   ZIRCON_EXPERIENCE_MULTIPLIER  positive XP rate (default 1)
//!   ZIRCON_INVENTORY_TAB_PRICE / ZIRCON_STORAGE_TAB_PRICE  Game Gold (100 / 200)
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mir_proto::{
    ClientMessage, LoginResult, NewAccountResult, NewCharacterResult, ServerMessage,
    PROTOCOL_VERSION,
};

use mir_server::accounts::{now_secs, test_character, Accounts};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use mir_server::data::GameData;
use mir_server::net::{self, Inbound};
use mir_server::world::{ConnId, Outgoing, World};

mod gm_admin;

const TICK: Duration = Duration::from_millis(50);

struct Args {
    assets: PathBuf,
    db: PathBuf,
    data: PathBuf,
    port: u16,
    map: Option<String>,
    inspect: bool,
}

fn parse_args() -> Args {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut assets = std::env::var_os("ZIRCON_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("zircon-assets/Client"));
    let mut db = std::env::var_os("ZIRCON_DB").map(PathBuf::from);
    let mut data = std::env::var_os("ZIRCON_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data"));
    let mut port = std::env::var("ZIRCON_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(mir_proto::DEFAULT_PORT);
    let mut map = None;
    let mut inspect = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--assets" => assets = PathBuf::from(it.next().expect("--assets DIR")),
            "--db" => db = Some(PathBuf::from(it.next().expect("--db FILE"))),
            "--data" => data = PathBuf::from(it.next().expect("--data DIR")),
            "--port" => port = it.next().expect("--port N").parse().expect("port"),
            "--map" => map = Some(it.next().expect("--map FILE")),
            "--inspect" => inspect = true,
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let db = db.unwrap_or_else(|| assets.join("../Database/System.db"));
    Args {
        assets,
        db,
        data,
        port,
        map,
        inspect,
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let args = parse_args();
    tracing::info!(assets = %args.assets.display(), db = %args.db.display(), "loading game data");
    let data = GameData::load(&args.db)?;
    tracing::info!(
        maps = data.maps.len(),
        regions = data.regions.len(),
        respawns = data.respawns.len(),
        monsters = data.monsters.len(),
        safe_zones = data.safe_zones.len(),
        base_stats = data.base_stats.len(),
        "game data loaded"
    );
    let map_dir = std::env::var_os("ZIRCON_MAP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| args.assets.join("Map"));
    let mut world = World::new(data, map_dir, args.map.clone());
    world.drop_specials = match std::env::var("ZIRCON_DROP_SPECIALS") {
        Ok(value) if value == "1" => true,
        Ok(value) if value == "0" => false,
        Ok(value) => anyhow::bail!("ZIRCON_DROP_SPECIALS must be 0 or 1, got {value}"),
        Err(std::env::VarError::NotPresent) => false,
        Err(error) => return Err(error.into()),
    };
    let xp_rate = match std::env::var("ZIRCON_EXPERIENCE_MULTIPLIER") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "1".into(),
        Err(error) => return Err(error.into()),
    };
    world.experience_multiplier = xp_rate
        .parse::<f64>()
        .ok()
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .ok_or_else(|| {
            anyhow::anyhow!("ZIRCON_EXPERIENCE_MULTIPLIER must be positive and finite")
        })?;
    for (name, price) in [
        ("ZIRCON_INVENTORY_TAB_PRICE", &mut world.inventory_tab_price),
        ("ZIRCON_STORAGE_TAB_PRICE", &mut world.storage_tab_price),
    ] {
        if let Ok(value) = std::env::var(name) {
            *price = value
                .parse()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| anyhow::anyhow!("{name} must be a positive integer"))?;
        }
    }
    tracing::info!(
        drop_specials = world.drop_specials,
        experience_multiplier = world.experience_multiplier,
        "server rates"
    );
    world.set_store_dir(&args.data);

    if args.inspect {
        return inspect(&mut world);
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<Inbound>();
    #[cfg(unix)]
    {
        let tx = in_tx.clone();
        runtime.spawn(async move {
            let Ok(mut hup) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
            else {
                return;
            };
            while hup.recv().await.is_some() {
                let _ = tx.send(Inbound::Reload);
            }
        });
        let tx = in_tx.clone();
        runtime.spawn(async move {
            let Ok(mut term) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            else {
                return;
            };
            term.recv().await;
            let _ = tx.send(Inbound::Shutdown);
        });
    }
    {
        let tx = in_tx.clone();
        runtime.spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                let _ = tx.send(Inbound::Shutdown);
            }
        });
    }
    let addr: SocketAddr = format!("0.0.0.0:{}", args.port).parse()?;
    runtime.spawn(async move {
        if let Err(e) = net::listen(addr, in_tx).await {
            tracing::error!("listener failed: {e}");
            std::process::exit(1);
        }
    });

    let mut accounts = Accounts::load(args.data.join("accounts.json"))?;
    let mut sessions = Sessions {
        conns: HashMap::new(),
        state: HashMap::new(),
        since: HashMap::new(),
        counts: HashMap::new(),
        dropped: Vec::new(),
        login_at: HashMap::new(),
        save_soon: false,
    };
    let start = Instant::now();
    let mut next_tick = Instant::now();
    let mut next_save = Instant::now() + SAVE_INTERVAL;
    let mut next_db_check = Instant::now() + DB_CHECK_INTERVAL;
    let mut db_stamp = db_modified(&args.db);
    for p in world.data.validate(&args.assets.join("Map")) {
        tracing::warn!("data: {p}");
    }
    let mut next_sweep = Instant::now();
    loop {
        // Drain input; each connection gets Zircon's `Config.MaxPacket`
        // (50) messages per tick before it is dropped as a flooder.
        sessions.counts.clear();
        while let Ok(ev) = in_rx.try_recv() {
            match ev {
                Inbound::Reload => {
                    reload(&mut world, &args);
                    continue;
                }
                Inbound::Shutdown => {
                    tracing::info!("shutting down: saving");
                    sessions.snapshot_all(&world, &mut accounts);
                    save_accounts(&mut accounts);
                    return Ok(());
                }
                ev => handle_inbound(&mut world, &mut accounts, &mut sessions, ev),
            }
        }
        for character in world.take_divorced() {
            accounts.clear_partner(character);
        }
        // Slow clients (full outbound queue) and connections that never
        // said hello are dropped.
        if Instant::now() >= next_sweep {
            next_sweep = Instant::now() + Duration::from_secs(1);
            let stale: Vec<ConnId> = sessions
                .since
                .iter()
                .filter(|(c, t)| match sessions.state.get(c) {
                    Some(Stage::New) => t.elapsed() > HELLO_TIMEOUT,
                    Some(Stage::Connected) => t.elapsed() > LOGIN_TIMEOUT,
                    _ => false,
                })
                .map(|(c, _)| *c)
                .collect();
            for c in stale {
                tracing::info!(conn = c, "no hello: dropping");
                disconnect(&mut world, &mut accounts, &mut sessions, c);
            }
        }
        for c in std::mem::take(&mut sessions.dropped) {
            tracing::warn!(conn = c, "client too slow: dropping");
            disconnect(&mut world, &mut accounts, &mut sessions, c);
        }
        // The editor saved a new System.db: reload it.
        if Instant::now() >= next_db_check {
            next_db_check = Instant::now() + DB_CHECK_INTERVAL;
            let stamp = db_modified(&args.db);
            if stamp != db_stamp {
                db_stamp = stamp;
                reload(&mut world, &args);
            }
        }
        let now = start.elapsed().as_millis() as u64;
        world.tick(now);
        for out in world.outgoing.drain(..) {
            match out {
                Outgoing::To(conn, msg) => sessions.send(conn, msg),
            }
        }
        // Items that just moved into the market or mail stores, and players
        // who just left, reach accounts.json this tick rather than in up to
        // SAVE_INTERVAL: a crash in between would duplicate the item.
        if std::mem::take(&mut world.account_dirty) || std::mem::take(&mut sessions.save_soon) {
            next_save = Instant::now();
        }
        if Instant::now() >= next_save {
            next_save = Instant::now() + SAVE_INTERVAL;
            sessions.snapshot_all(&world, &mut accounts);
            save_accounts(&mut accounts);
        }
        next_tick += TICK;
        let sleep = next_tick.saturating_duration_since(Instant::now());
        if sleep.is_zero() {
            next_tick = Instant::now();
        } else {
            std::thread::sleep(sleep);
        }
    }
}

const SAVE_INTERVAL: Duration = Duration::from_secs(30);
/// Zircon `Config.MaxPacket`: client messages per tick before disconnect.
const MAX_MESSAGES_PER_TICK: u32 = 50;
/// A connection must send `Hello` within this long.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Said hello but never logged in: the slot goes back to the gate so idle
/// unauthenticated sockets cannot hold all `MAX_CONNECTIONS`.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const LOGIN_WINDOW: Duration = Duration::from_secs(3);
const LOGIN_BURST: u8 = 3;
const DB_CHECK_INTERVAL: Duration = Duration::from_secs(2);

fn db_modified(path: &PathBuf) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Re-read `System.db`; on failure the old data stays in use.
fn reload(world: &mut World, args: &Args) {
    match GameData::load(&args.db) {
        Ok(data) => {
            let problems = data.validate(&args.assets.join("Map"));
            for p in &problems {
                tracing::warn!("data: {p}");
            }
            world.reload_data(data);
            tracing::info!(problems = problems.len(), "game data reloaded");
        }
        Err(e) => tracing::error!("reload failed, keeping the old data: {e:#}"),
    }
}

/// Where a connection is in the login flow.
#[derive(Debug, Clone, Copy)]
enum Stage {
    /// TCP connected, no `Hello` yet.
    New,
    Connected,
    LoggedIn {
        account: u32,
    },
    InGame {
        account: u32,
        character: u32,
        object: mir_proto::ObjectId,
    },
}

struct Sessions {
    conns: HashMap<ConnId, mpsc::Sender<ServerMessage>>,
    state: HashMap<ConnId, Stage>,
    /// When each connection arrived (hello deadline).
    since: HashMap<ConnId, Instant>,
    /// Messages received this tick per connection.
    counts: HashMap<ConnId, u32>,
    /// Connections whose outbound queue overflowed; dropped by the loop.
    dropped: Vec<ConnId>,
    /// Login / new-account attempts per connection: window start and count.
    login_at: HashMap<ConnId, (Instant, u8)>,
    /// A player left: save accounts on the next tick, once for all of them.
    save_soon: bool,
}

impl Sessions {
    /// At most `LOGIN_BURST` login / new-account attempts per connection
    /// per `LOGIN_WINDOW`: an Argon2 hash costs tens of milliseconds and
    /// fifty logins in one tick used to stall the world for everyone. The
    /// burst covers the login screen's failed login, create, login sequence.
    fn login_allowed(&mut self, conn: ConnId) -> bool {
        let now = Instant::now();
        let entry = self.login_at.entry(conn).or_insert((now, 0));
        if now.duration_since(entry.0) >= LOGIN_WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= LOGIN_BURST
    }

    fn send(&mut self, conn: ConnId, msg: ServerMessage) {
        if let Some(tx) = self.conns.get(&conn) {
            if let Err(mpsc::error::TrySendError::Full(_)) = tx.try_send(msg) {
                if !self.dropped.contains(&conn) {
                    self.dropped.push(conn);
                }
            }
        }
    }

    fn snapshot_all(&self, world: &World, accounts: &mut Accounts) {
        for stage in self.state.values() {
            if let Stage::InGame {
                account,
                character,
                object,
            } = stage
            {
                if let Some(rec) = accounts.character_mut(*account, *character) {
                    world.snapshot(*object, rec);
                    accounts.mark_dirty();
                }
                save_storage(world, accounts, *account, *object);
            }
        }
    }

    /// Is this account already logged in on another connection?
    fn account_in_use(&self, account: u32, except: ConnId) -> bool {
        self.state.iter().any(|(c, s)| {
            *c != except
                && matches!(s, Stage::LoggedIn { account: a } | Stage::InGame { account: a, .. } if *a == account)
        })
    }
}

/// Take a player out of the world and persist it.
fn leave_world(world: &mut World, accounts: &mut Accounts, stage: Stage) {
    if let Stage::InGame {
        account,
        character,
        object,
    } = stage
    {
        if let Some(rec) = accounts.character_mut(account, character) {
            world.snapshot(object, rec);
            accounts.mark_dirty();
        }
        save_storage(world, accounts, account, object);
        // Friends see Offline before the object goes away.
        world.go_offline(object);
        save_fortunes(world, accounts, account, object);
        world.remove_object(object);
    }
}

/// Write the player's account storage back to the account record.
fn save_storage(world: &World, accounts: &mut Accounts, account: u32, object: mir_proto::ObjectId) {
    if let Some((items, size)) = world.storage_of(object) {
        if let Some(acc) = accounts.account_mut(account) {
            acc.storage = items;
            acc.storage_size = size;
            accounts.mark_dirty();
        }
    }
}

/// Write the player's drop progress back to the account record.
fn save_fortunes(
    world: &World,
    accounts: &mut Accounts,
    account: u32,
    object: mir_proto::ObjectId,
) {
    if let Some((drops, fortunes)) = world.fortunes_of(object) {
        if let Some(acc) = accounts.account_mut(account) {
            acc.drops = drops;
            acc.fortunes = fortunes;
            accounts.mark_dirty();
        }
    }
}

/// Drop a connection from the server side: the writer ends, the reader
/// follows, and the world is left now (a later `Disconnected` is a no-op).
fn disconnect(world: &mut World, accounts: &mut Accounts, sessions: &mut Sessions, conn: ConnId) {
    sessions.conns.remove(&conn);
    sessions.since.remove(&conn);
    sessions.counts.remove(&conn);
    sessions.login_at.remove(&conn);
    if let Some(stage) = sessions.state.remove(&conn) {
        leave_world(world, accounts, stage);
        // One save next tick covers a mass disconnect; a full rewrite per
        // connection here stalled the tick.
        sessions.save_soon = true;
    }
}

fn save_accounts(accounts: &mut Accounts) {
    if let Err(e) = accounts.save_if_dirty() {
        tracing::error!("saving accounts failed: {e:#}");
    }
}

/// Reject messages whose strings or lists exceed what any legitimate
/// client sends (Zircon disconnects on malformed packets).
fn valid(msg: &ClientMessage) -> bool {
    const NAME: usize = 20;
    match msg {
        ClientMessage::NewAccount { email, password }
        | ClientMessage::Login { email, password } => email.len() <= 64 && password.len() <= 128,
        ClientMessage::NewCharacter { name, .. } => name.len() <= NAME,
        ClientMessage::Chat { text } => text.len() <= 512,
        ClientMessage::GroupInvite { name }
        | ClientMessage::GroupRemove { name }
        | ClientMessage::GuildInviteMember { name } => name.len() <= NAME,
        ClientMessage::GuildCreate { name, .. } => name.len() <= 32,
        ClientMessage::GuildEditNotice { notice } => notice.len() <= 4096 * 4,
        ClientMessage::GuildEditMember { rank, .. } => rank.len() <= 64,
        ClientMessage::MailSend {
            recipient,
            subject,
            message,
            items,
            ..
        } => {
            recipient.len() <= NAME
                && subject.len() <= 128
                && message.len() <= 1200
                && items.len() <= 5
        }
        ClientMessage::FriendAdd { name }
        | ClientMessage::BlockAdd { name }
        | ClientMessage::NameChange { name } => name.len() <= NAME,
        ClientMessage::NpcSell { items } => items.len() <= 64,
        ClientMessage::NpcRepair { items, .. } => items.len() <= 55,
        // Zircon caps the seller's note at 150 characters; the name box is
        // an item name, so the same limit as a character name is plenty.
        ClientMessage::MarketConsign { message, .. } => message.len() <= 600,
        ClientMessage::MarketSearch { name, .. } => name.len() <= 128,
        // One purchase can never exceed a stack, and the server clamps it
        // anyway; this only keeps a silly number off the wire.
        ClientMessage::MarketStoreBuy { count, .. } => *count <= 10_000,
        _ => true,
    }
}

fn handle_inbound(
    world: &mut World,
    accounts: &mut Accounts,
    sessions: &mut Sessions,
    ev: Inbound,
) {
    match ev {
        Inbound::Connected { conn, addr, tx } => {
            tracing::info!(conn, %addr, "connected");
            sessions.conns.insert(conn, tx);
            sessions.state.insert(conn, Stage::New);
            sessions.since.insert(conn, Instant::now());
        }
        // Handled by the main loop before dispatch.
        Inbound::Reload | Inbound::Shutdown => {}
        Inbound::Disconnected { conn } => {
            if sessions.conns.contains_key(&conn) {
                tracing::info!(conn, "disconnected");
            }
            disconnect(world, accounts, sessions, conn);
        }
        Inbound::Message { conn, msg } => {
            if !sessions.conns.contains_key(&conn) {
                return;
            }
            let count = sessions.counts.entry(conn).or_insert(0);
            *count += 1;
            if *count > MAX_MESSAGES_PER_TICK {
                tracing::warn!(conn, "flooding: dropping");
                disconnect(world, accounts, sessions, conn);
                return;
            }
            if !valid(&msg) {
                tracing::warn!(conn, kind = %message_kind(&msg), "oversized message: dropping");
                disconnect(world, accounts, sessions, conn);
                return;
            }
            let stage = sessions.state.get(&conn).copied().unwrap_or(Stage::New);
            tracing::debug!(conn, ?stage, kind = %message_kind(&msg), "message");
            handle_message(world, accounts, sessions, conn, stage, msg);
        }
    }
}

/// Variant name of a client message, for debug logging.
fn message_kind(msg: &ClientMessage) -> String {
    let text = format!("{msg:?}");
    text.split([' ', '{', '('])
        .next()
        .unwrap_or("?")
        .to_string()
}

/// Character summaries with the map name filled in from game data.
fn summaries(world: &World, accounts: &Accounts, account: u32) -> Vec<mir_proto::CharacterSummary> {
    let mut list = accounts.summaries(account);
    if let Some(acc) = accounts.account(account) {
        for s in &mut list {
            if let Some(rec) = acc.characters.iter().find(|c| c.id == s.id) {
                s.location = world
                    .data
                    .map_by_file(&rec.map)
                    .map(|m| m.description.clone())
                    .unwrap_or_default();
            }
        }
    }
    list
}

fn handle_message(
    world: &mut World,
    accounts: &mut Accounts,
    sessions: &mut Sessions,
    conn: ConnId,
    stage: Stage,
    msg: ClientMessage,
) {
    match (stage, msg) {
        (_, ClientMessage::Ping { nonce }) => sessions.send(conn, ServerMessage::Pong { nonce }),
        (Stage::New, ClientMessage::Hello { version }) => {
            if version != PROTOCOL_VERSION {
                sessions.send(
                    conn,
                    ServerMessage::Rejected {
                        reason: format!("protocol {version} != {PROTOCOL_VERSION}"),
                    },
                );
                return;
            }
            sessions.state.insert(conn, Stage::Connected);
            sessions.send(conn, ServerMessage::Connected);
        }
        (Stage::Connected, ClientMessage::NewAccount { email, password }) => {
            if !sessions.login_allowed(conn) {
                let reason = "Too many attempts; wait a second.".into();
                sessions.send(
                    conn,
                    ServerMessage::NewAccountResult(NewAccountResult::Failed { reason }),
                );
                return;
            }
            let result = match accounts.create(&email, &password) {
                Ok(id) => {
                    tracing::info!(conn, id, email, "account created");
                    NewAccountResult::Success
                }
                Err(reason) => NewAccountResult::Failed { reason },
            };
            save_accounts(accounts);
            sessions.send(conn, ServerMessage::NewAccountResult(result));
        }
        (Stage::Connected, ClientMessage::Login { email, password }) => {
            if !sessions.login_allowed(conn) {
                let reason = "Too many attempts; wait a second.".into();
                sessions.send(
                    conn,
                    ServerMessage::LoginResult(LoginResult::Failed { reason }),
                );
                return;
            }
            let result = match accounts.login(&email, &password) {
                Ok(account) if sessions.account_in_use(account, conn) => LoginResult::Failed {
                    reason: "Account already logged in".into(),
                },
                Ok(account) => {
                    tracing::info!(conn, account, email, "logged in");
                    sessions.state.insert(conn, Stage::LoggedIn { account });
                    LoginResult::Success {
                        characters: summaries(world, accounts, account),
                    }
                }
                Err(reason) => LoginResult::Failed { reason },
            };
            sessions.send(conn, ServerMessage::LoginResult(result));
        }
        (
            Stage::LoggedIn { account },
            ClientMessage::NewCharacter {
                name,
                class,
                gender,
                hair,
            },
        ) => {
            let result = match accounts.new_character(account, &name, class, gender, hair) {
                Ok(rec) => {
                    tracing::info!(conn, account, name = rec.name, ?class, "character created");
                    NewCharacterResult::Success {
                        character: rec.summary(),
                    }
                }
                Err(reason) => NewCharacterResult::Failed { reason },
            };
            save_accounts(accounts);
            sessions.send(conn, ServerMessage::NewCharacterResult(result));
        }
        (Stage::LoggedIn { account }, ClientMessage::DeleteCharacter { id }) => {
            let (ok, reason) = match accounts.delete_character(account, id) {
                Ok(()) => (true, String::new()),
                Err(r) => (false, r),
            };
            save_accounts(accounts);
            sessions.send(
                conn,
                ServerMessage::DeleteCharacterResult { id, ok, reason },
            );
        }
        (Stage::LoggedIn { account }, ClientMessage::StartGame { id }) => {
            let Some(rec) = accounts.character(account, id).cloned() else {
                sessions.send(
                    conn,
                    ServerMessage::Rejected {
                        reason: "No such character".into(),
                    },
                );
                return;
            };
            match world.add_player(conn, account, &rec) {
                Ok(object) => {
                    tracing::info!(conn, account, name = rec.name, ?object, "entered world");
                    if let Some(acc) = accounts.account(account) {
                        world.set_chat_bans(object, acc.chat_ban_until, acc.global_ban_until);
                    }
                    let (storage, size) = accounts
                        .account(account)
                        .map(|a| (a.storage.clone(), a.storage_size))
                        .unwrap_or_default();
                    world.set_storage(object, &storage, size);
                    world.send_tab_prices(object);
                    let (drops, fortunes) = accounts
                        .account(account)
                        .map(|a| (a.drops.clone(), a.fortunes.clone()))
                        .unwrap_or_default();
                    world.set_fortunes(object, &drops, &fortunes);
                    if let Some(r) = accounts.character_mut(account, id) {
                        r.last_login = now_secs();
                        accounts.mark_dirty();
                    }
                    sessions.state.insert(
                        conn,
                        Stage::InGame {
                            account,
                            character: id,
                            object,
                        },
                    );
                }
                Err(e) => {
                    tracing::warn!(conn, "start game failed: {e:#}");
                    sessions.send(
                        conn,
                        ServerMessage::Rejected {
                            reason: format!("{e:#}"),
                        },
                    );
                }
            }
        }
        (Stage::InGame { account, .. }, ClientMessage::Logout) => {
            leave_world(world, accounts, stage);
            save_accounts(accounts);
            sessions.state.insert(conn, Stage::LoggedIn { account });
            sessions.send(
                conn,
                ServerMessage::LoggedOut {
                    characters: summaries(world, accounts, account),
                },
            );
        }
        (Stage::InGame { object, .. }, ClientMessage::Turn { direction }) => {
            world.player_turn(object, direction)
        }
        (Stage::InGame { object, .. }, ClientMessage::Move { direction, run }) => {
            world.player_move(object, direction, run)
        }
        (Stage::InGame { object, .. }, ClientMessage::RoomDoorToggle { id }) => {
            world.player_toggle_room_door(object, id)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::Attack {
                direction,
                attack_magic,
            },
        ) => world.player_attack(object, direction, attack_magic),
        (
            Stage::InGame { object, .. },
            ClientMessage::Magic {
                magic,
                direction,
                target,
                location,
            },
        ) => world.cast(object, magic, direction, target, location),
        (Stage::InGame { object, .. }, ClientMessage::MagicKey { magic, key }) => {
            world.magic_key(object, magic, key)
        }
        (Stage::InGame { object, .. }, ClientMessage::MagicToggle { magic, on }) => {
            world.magic_toggle(object, magic, on)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::ItemMove {
                from,
                from_slot,
                to,
                to_slot,
            },
        ) => world.item_move(object, from, from_slot, to, to_slot),
        (Stage::InGame { object, .. }, ClientMessage::ItemUse { slot }) => {
            world.item_use(object, slot)
        }
        (Stage::InGame { object, .. }, ClientMessage::BeltLink { slot, info, item }) => {
            world.belt_link(object, slot, info, item)
        }
        (Stage::InGame { object, .. }, ClientMessage::TownRevive) => world.town_revive(object),
        (Stage::InGame { object, .. }, ClientMessage::ItemLock { grid, slot, locked }) => {
            world.item_lock(object, grid, slot, locked)
        }
        (Stage::InGame { object, .. }, ClientMessage::ItemDrop { slot, count }) => {
            world.item_drop(object, slot, count)
        }
        (
            Stage::InGame {
                account,
                character,
                object,
            },
            ClientMessage::BuyTab { storage },
        ) => {
            if world.buy_tab(object, storage) {
                if let Some(rec) = accounts.character_mut(account, character) {
                    world.snapshot(object, rec);
                    accounts.mark_dirty();
                }
                save_storage(world, accounts, account, object);
                save_accounts(accounts);
            }
        }
        (Stage::InGame { object, .. }, ClientMessage::PickUp) => world.pick_up(object),
        (Stage::InGame { object, .. }, ClientMessage::Chat { text }) => {
            if !gm_admin::handle(world, accounts, sessions, conn, object, &text) {
                world.chat(object, text);
            }
        }
        (Stage::InGame { object, .. }, ClientMessage::GroupSwitch { allow }) => {
            world.group_switch(object, allow)
        }
        (Stage::InGame { object, .. }, ClientMessage::GroupInvite { name }) => {
            world.group_invite(object, name)
        }
        (Stage::InGame { object, .. }, ClientMessage::GroupResponse { accept }) => {
            world.group_response(object, accept)
        }
        (Stage::InGame { object, .. }, ClientMessage::GroupRemove { name }) => {
            world.group_remove(object, name)
        }
        (Stage::InGame { object, .. }, ClientMessage::AttackMode { mode }) => {
            world.set_attack_mode(object, mode)
        }
        (Stage::InGame { object, .. }, ClientMessage::PetMode { mode }) => {
            world.set_pet_mode(object, mode)
        }
        (Stage::InGame { object, .. }, ClientMessage::Wardrobe { slot, item }) => {
            world.set_wardrobe_slot(object, slot as usize, item)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::FishingCast {
                state,
                direction,
                float,
                caught,
            },
        ) => world.fishing_cast(object, state, direction, float, caught),
        (Stage::InGame { object, .. }, ClientMessage::Mining { direction }) => {
            world.mining(object, direction)
        }
        (Stage::InGame { object, .. }, ClientMessage::Harvest { direction }) => {
            world.harvest(object, direction)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildCreate { name, members }) => {
            world.guild_create(object, name, members)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildEditNotice { notice }) => {
            world.guild_edit_notice(object, notice)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::GuildEditMember {
                index,
                rank,
                permission,
            },
        ) => world.guild_edit_member(object, index, rank, permission),
        (Stage::InGame { object, .. }, ClientMessage::GuildInviteMember { name }) => {
            world.guild_invite(object, name)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildKickMember { index }) => {
            world.guild_kick(object, index)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildResponse { accept }) => {
            world.guild_response(object, accept)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildLeave) => world.guild_leave(object),
        (Stage::InGame { object, .. }, ClientMessage::GuildTax { tax }) => {
            world.guild_tax(object, tax)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildIncreaseMember) => {
            world.guild_increase_member(object)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildWar { name }) => {
            world.guild_war(object, name)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildToggleCastleGates) => {
            world.guild_toggle_castle_gates(object)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildRepairCastleGates) => {
            world.guild_repair_castle_gates(object)
        }
        (Stage::InGame { object, .. }, ClientMessage::GuildRequestConquest { index }) => {
            world.guild_request_conquest(object, index)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::MailSend {
                recipient,
                subject,
                message,
                gold,
                items,
            },
        ) => {
            let to = accounts.find_character(&recipient);
            world.mail_send(object, to, subject, message, gold, items)
        }
        (Stage::InGame { object, .. }, ClientMessage::MailOpened { index }) => {
            world.mail_opened(object, index)
        }
        (Stage::InGame { object, .. }, ClientMessage::HairChange { hair, colour }) => {
            world.hair_change(object, hair, colour)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::GenderChange {
                gender,
                hair,
                colour,
            },
        ) => world.gender_change(object, gender, hair, colour),
        (Stage::InGame { object, .. }, ClientMessage::ArmourDye { colour }) => {
            world.armour_dye(object, colour)
        }
        (
            Stage::InGame {
                object,
                account,
                character,
            },
            ClientMessage::NameChange { name },
        ) => {
            let holder = accounts.find_character_full(&name);
            world.name_change(object, name, holder);
            if let Some(rec) = accounts.character_mut(account, character) {
                world.snapshot(object, rec);
                accounts.mark_dirty();
            }
        }
        (Stage::InGame { object, .. }, ClientMessage::IncreaseDiscipline) => {
            world.increase_discipline(object)
        }
        (Stage::InGame { object, .. }, ClientMessage::FriendAdd { name }) => {
            let who = accounts.find_character_full(&name);
            world.friend_add(object, who, name)
        }
        (Stage::InGame { object, .. }, ClientMessage::FriendRemove { index }) => {
            world.friend_remove(object, index)
        }
        (Stage::InGame { object, .. }, ClientMessage::BlockAdd { name }) => {
            let who = accounts.find_character_full(&name);
            world.block_add(object, who, name)
        }
        (Stage::InGame { object, .. }, ClientMessage::BlockRemove { index }) => {
            world.block_remove(object, index)
        }
        (Stage::InGame { object, .. }, ClientMessage::ChangeOnlineState { state }) => {
            world.change_online_state(object, state)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::RankRequest {
                class,
                online_only,
                start,
            },
        ) => {
            // The roster lives in the account store, not the world, because
            // offline characters rank too.
            world.rank_request(object, ranking_rows(accounts), class, online_only, start)
        }
        (Stage::InGame { object, .. }, ClientMessage::RankSearch { name }) => {
            world.rank_search(object, ranking_rows(accounts), &name)
        }
        (Stage::InGame { object, .. }, ClientMessage::RankInspect { character }) => {
            let record = accounts
                .all_characters()
                .find(|row| row.id == character)
                .cloned();
            world.rank_inspect(object, record)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::MarketSearch {
                name,
                item_type,
                sort,
                page,
            },
        ) => world.market_search(object, name, item_type, sort, page),
        (
            Stage::InGame { object, .. },
            ClientMessage::MarketConsign {
                slot,
                count,
                price,
                message,
            },
        ) => world.market_consign(object, slot, count, price, message),
        (Stage::InGame { object, .. }, ClientMessage::MarketCancelConsign { index, count }) => {
            world.market_cancel_consign(object, index, count)
        }
        (Stage::InGame { object, .. }, ClientMessage::MarketBuy { index, count }) => {
            world.market_buy(object, index, count)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::MarketStoreBuy {
                index,
                count,
                hunt_gold,
            },
        ) => {
            let highest = world
                .objects
                .get(&object)
                .and_then(|o| o.player())
                .and_then(|p| accounts.account(p.account))
                .map(|a| a.characters.iter().map(|c| c.level).max().unwrap_or(0))
                .unwrap_or(0);
            world.store_buy_for_account(object, index, count, hunt_gold, highest);
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::NpcRefine {
                refine_type,
                quality,
                ores,
                items,
                specials,
            },
        ) => world.npc_refine(object, refine_type, quality, ores, items, specials),
        (
            Stage::InGame { object, .. },
            ClientMessage::NpcWeaponCraft {
                base,
                materials,
                class,
            },
        ) => {
            world.npc_weapon_craft(object, base, materials, class);
        }
        (Stage::InGame { object, .. }, ClientMessage::NpcRefineRetrieve { index }) => {
            world.npc_refine_retrieve(object, index)
        }
        (Stage::InGame { object, .. }, ClientMessage::NpcAccessoryLevel { target, materials }) => {
            world.npc_accessory_level(object, target, materials)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::NpcAccessoryUpgrade {
                target,
                refine_type,
            },
        ) => world.npc_accessory_upgrade(object, target, refine_type),
        (Stage::InGame { object, .. }, ClientMessage::NpcAccessoryReset { target }) => {
            world.npc_accessory_reset(object, target)
        }
        (
            Stage::InGame { object, .. },
            ClientMessage::NpcAccessoryRefine {
                target,
                ore,
                materials,
                refine_type,
            },
        ) => world.npc_accessory_refine(object, target, ore, materials, refine_type),
        (Stage::InGame { object, .. }, ClientMessage::CompanionUnlock { index }) => {
            world.companion_unlock(object, index)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionAdopt { index, name }) => {
            world.companion_adopt(object, index, name)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionRetrieve { index }) => {
            world.companion_retrieve(object, index)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionStore) => {
            world.companion_store(object)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionRelease { index }) => {
            world.companion_release(object, index)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionBagTake { index, slot }) => {
            world.companion_bag_take(object, index, slot)
        }
        (Stage::InGame { object, .. }, ClientMessage::PickUpItem { id }) => {
            world.pick_up_item(object, id)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionBagEquip { slot }) => {
            world.companion_bag_equip(object, slot)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionBagUnequip) => {
            world.companion_bag_unequip(object)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionFoodEquip { slot }) => {
            world.companion_food_equip(object, slot)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionFoodTake) => {
            world.companion_food_take(object)
        }
        (Stage::InGame { object, .. }, ClientMessage::CompanionSkillToggle { level }) => {
            world.companion_skill_toggle(object, level)
        }
        (Stage::InGame { object, .. }, ClientMessage::MailGetItem { index, slot }) => {
            world.mail_get_item(object, index, slot)
        }
        (Stage::InGame { object, .. }, ClientMessage::MailDelete { index }) => {
            world.mail_delete(object, index)
        }
        (Stage::InGame { object, .. }, ClientMessage::Mount) => world.mount_toggle(object),
        (Stage::InGame { object, .. }, ClientMessage::MarriageResponse { accept }) => {
            world.marriage_response(object, accept)
        }
        (Stage::InGame { object, .. }, ClientMessage::MarriageMakeRing { slot }) => {
            world.marriage_make_ring(object, slot)
        }
        (Stage::InGame { object, .. }, ClientMessage::MarriageTeleport) => {
            world.marriage_teleport(object)
        }
        (Stage::InGame { object, .. }, ClientMessage::TradeRequest) => world.trade_request(object),
        (Stage::InGame { object, .. }, ClientMessage::TradeResponse { accept }) => {
            world.trade_response(object, accept)
        }
        (Stage::InGame { object, .. }, ClientMessage::TradeClose) => world.trade_close(object),
        (Stage::InGame { object, .. }, ClientMessage::TradeAddItem { grid, slot, count }) => {
            world.trade_add_item(object, grid, slot, count)
        }
        (Stage::InGame { object, .. }, ClientMessage::TradeAddGold { gold }) => {
            world.trade_add_gold(object, gold)
        }
        (Stage::InGame { object, .. }, ClientMessage::TradeConfirm) => world.trade_confirm(object),
        (Stage::InGame { object, .. }, ClientMessage::NpcCall { id }) => world.npc_call(object, id),
        (Stage::InGame { object, .. }, ClientMessage::NpcButton { button }) => {
            world.npc_button(object, button)
        }
        (Stage::InGame { object, .. }, ClientMessage::NpcBuy { info, count }) => {
            world.npc_buy(object, info, count)
        }
        (Stage::InGame { object, .. }, ClientMessage::NpcSell { items }) => {
            world.npc_sell(object, items)
        }
        (Stage::InGame { object, .. }, ClientMessage::NpcRepair { items, special }) => {
            world.npc_repair(object, items, special)
        }
        (Stage::InGame { object, .. }, ClientMessage::NpcClose) => world.npc_close(object),
        (Stage::InGame { object, .. }, ClientMessage::FortuneCheck { item }) => {
            world.fortune_check(object, item)
        }
        (Stage::InGame { object, .. }, ClientMessage::QuestAccept { quest }) => {
            world.quest_accept(object, quest)
        }
        (Stage::InGame { object, .. }, ClientMessage::QuestComplete { quest, choice }) => {
            world.quest_complete(object, quest, choice)
        }
        (Stage::InGame { object, .. }, ClientMessage::QuestTrack { quest, track }) => {
            world.quest_track(object, quest, track)
        }
        (Stage::InGame { object, .. }, ClientMessage::QuestAbandon { quest }) => {
            world.quest_abandon(object, quest)
        }
        (stage, msg) => {
            tracing::debug!(conn, ?stage, ?msg, "message ignored in this stage");
        }
    }
}

/// Print what a new warrior would get, then exit. Useful without a client.
fn ranking_rows(accounts: &Accounts) -> Vec<mir_server::world::ranking::RankRow> {
    accounts
        .all_characters()
        .map(|c| mir_server::world::ranking::RankRow {
            character: c.id,
            name: c.name.clone(),
            class: c.class,
            level: c.level,
            experience: c.experience,
            max_experience: GameData::max_experience(c.level),
            rebirth: c.rebirth,
        })
        .collect()
}

fn inspect(world: &mut World) -> anyhow::Result<()> {
    let id = world.add_player(0, 0, &test_character("Inspector"))?;
    let o = &world.objects[&id];
    let map = &world.maps[&o.map];
    println!(
        "start: map {} \"{}\" ({}) at ({}, {}), {}x{} cells, {} walkable",
        map.index,
        map.descriptor.name,
        map.descriptor.file,
        o.location.x,
        o.location.y,
        map.file.width,
        map.file.height,
        map.file.walkable_count()
    );
    println!(
        "player: hp {} dc {}-{} ac {}-{} acc {} agi {}",
        o.hp,
        o.stats.min_dc,
        o.stats.max_dc,
        o.stats.min_ac,
        o.stats.max_ac,
        o.stats.accuracy,
        o.stats.agility
    );
    println!("spawn groups on map: {}", map.spawns.len());
    let mut counts: HashMap<String, (i32, i32)> = HashMap::new();
    for g in &map.spawns {
        let m = &world.data.monsters[&g.def.monster];
        let e = counts.entry(m.name.clone()).or_insert((0, 0));
        e.0 += g.def.count;
        e.1 += g.alive;
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by_key(|(_, (want, _))| std::cmp::Reverse(*want));
    for (name, (want, alive)) in v.iter().take(25) {
        let def = world
            .data
            .monsters
            .values()
            .find(|m| &m.name == name)
            .unwrap();
        println!(
            "  {name:24} want {want:4} alive {alive:4}  image {:3} lvl {:3} hp {:6} dc {}-{} ac {}-{} acc {} agi {} view {} atk {}ms move {}ms exp {}",
            def.image, def.level, def.health(), def.stat(8), def.stat(9), def.stat(4), def.stat(5), def.stat(14), def.stat(15), def.view_range, def.attack_delay, def.move_delay, def.experience
        );
    }
    let near: Vec<_> = world
        .objects
        .values()
        .filter(|m| !m.is_player() && m.location.distance(o.location) <= 18)
        .map(|m| match &m.appearance {
            mir_proto::Appearance::Monster { name, .. } => {
                format!("{name}@({},{})", m.location.x, m.location.y)
            }
            _ => String::new(),
        })
        .collect();
    println!(
        "monsters within view of start ({}): {}",
        near.len(),
        near.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_rejects_oversized_client_messages() {
        assert!(valid(&ClientMessage::Chat {
            text: "x".repeat(512)
        }));
        assert!(!valid(&ClientMessage::Chat {
            text: "x".repeat(513)
        }));
        let mail = |n| ClientMessage::MailSend {
            recipient: "Bob".into(),
            subject: String::new(),
            message: String::new(),
            gold: 0,
            items: vec![(mir_proto::Grid::Inventory, 0, 1); n],
        };
        assert!(valid(&mail(5)));
        assert!(!valid(&mail(6)));
        assert!(!valid(&ClientMessage::Login {
            email: "e".repeat(65),
            password: "p".into()
        }));
    }
}
