//! Real server movement through physical passages of generated Wood branches.
use mir_formats::{
    map::MapFile,
    mirdb::{MirDb, Value},
};
use mir_proto::{
    decode_frame, encode, Class, ClientMessage, Direction, Gender, NewCharacterResult,
    ServerMessage, PROTOCOL_VERSION,
};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    path: PathBuf,
    server: Child,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
struct Bot {
    stream: TcpStream,
    buffer: Vec<u8>,
}
impl Bot {
    fn connect(port: u16) -> Self {
        let deadline = Instant::now() + Duration::from_secs(30);
        let stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(_) => {
                    assert!(Instant::now() < deadline, "Branch server did not start");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        stream.set_nodelay(true).unwrap();
        Self {
            stream,
            buffer: vec![],
        }
    }
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
                "Timed out waiting for branch movement"
            );
            let mut bytes = [0; 8192];
            match self.stream.read(&mut bytes) {
                Ok(0) => panic!("Branch server disconnected"),
                Ok(count) => self.buffer.extend_from_slice(&bytes[..count]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => panic!("Branch socket read: {error}"),
            }
        }
    }
    fn enter(&mut self, label: &str) -> (mir_proto::ObjectId, (i32, i32)) {
        self.send(ClientMessage::Hello {
            version: PROTOCOL_VERSION,
        });
        self.wait(|m| matches!(m, ServerMessage::Connected));
        let email = format!("branch{label}@qa.test");
        self.send(ClientMessage::NewAccount {
            email: email.clone(),
            password: "branchtest123".into(),
        });
        self.wait(|m| matches!(m, ServerMessage::NewAccountResult(_)));
        self.send(ClientMessage::Login {
            email,
            password: "branchtest123".into(),
        });
        self.wait(|m| matches!(m, ServerMessage::LoginResult(_)));
        self.send(ClientMessage::NewCharacter {
            name: format!("B{}", label.chars().take(10).collect::<String>()),
            class: Class::Warrior,
            gender: Gender::Male,
            hair: 1,
        });
        let id = match self.wait(|m| matches!(m, ServerMessage::NewCharacterResult(_))) {
            ServerMessage::NewCharacterResult(NewCharacterResult::Success { character }) => {
                character.id
            }
            other => panic!("Branch character creation failed: {other:?}"),
        };
        self.send(ClientMessage::StartGame { id });
        match self.wait(|m| matches!(m, ServerMessage::Welcome { .. })) {
            ServerMessage::Welcome { id, location, .. } => (id, (location.x, location.y)),
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy)]
struct Route {
    label: &'static str,
    start: (i32, i32),
    direction: Direction,
    steps: i32,
    blocked: Option<Direction>,
}

fn start_fixture(
    assets: &Path,
    source: &[u8],
    source_db: &[u8],
    start: (i32, i32),
    label: &str,
) -> (Fixture, u16) {
    let path =
        std::env::temp_dir().join(format!("zircon-wood-branch-{}-{label}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    let map_dir = path.join("Map");
    std::fs::create_dir(&map_dir).unwrap();
    std::fs::write(map_dir.join("BranchQa.map"), source).unwrap();
    let mut db = MirDb::parse(source_db).unwrap();
    db.collection_mut("MapInfo").unwrap().push_record(vec![
        ("FileName", Value::Str("BranchQa".into())),
        ("Description", Value::Str("Wood branch traversal QA".into())),
        ("Light", Value::Int(1)),
    ]);
    db.collection_mut("RespawnInfo").unwrap().records.clear();
    db.collection_mut("MovementInfo").unwrap().records.clear();
    let database = path.join("System.db");
    db.save(&database).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let server = Command::new(env!("CARGO_BIN_EXE_mir-server"))
        .args(["--port", &port.to_string(), "--db"])
        .arg(&database)
        .arg("--data")
        .arg(path.join("Save"))
        .env("ZIRCON_ASSETS", assets)
        .env("ZIRCON_MAP_DIR", &map_dir)
        .env(
            "ZIRCON_DEV_START",
            format!("BranchQa:{},{}", start.0, start.1),
        )
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    (Fixture { path, server }, port)
}

fn verify_routes(assets: &Path, input: &Path, routes: &[Route]) {
    let source = std::fs::read(input).unwrap();
    let map = MapFile::parse(&source).unwrap();
    assert_eq!((map.width, map.height), (112, 112));
    let source_db = std::fs::read(assets.join("../Database/System.db")).unwrap();
    for &Route {
        label,
        start,
        direction,
        steps,
        blocked,
    } in routes
    {
        let (dx, dy) = direction.delta();
        for step in 0..=steps {
            assert!(
                map.is_walkable(start.0 + dx * step, start.1 + dy * step),
                "{label} passage is blocked at step {step}"
            );
        }
        if let Some(direction) = blocked {
            let (bx, by) = direction.delta();
            assert!(!map.is_walkable(start.0 + bx, start.1 + by));
        }
        let (_fixture, port) = start_fixture(assets, &source, &source_db, start, label);
        let mut actor = Bot::connect(port);
        let (actor_id, location) = actor.enter(&format!("A{label}"));
        assert_eq!(location, start);
        let mut observer = Bot::connect(port);
        let (observer_id, observer_location) = observer.enter(&format!("O{label}"));
        assert!(
            (observer_location.0 - start.0)
                .abs()
                .max((observer_location.1 - start.1).abs())
                <= 2
        );
        let on_route = |point: (i32, i32)| {
            (0..=steps).any(|step| point == (start.0 + dx * step, start.1 + dy * step))
        };
        if on_route(observer_location) {
            let escape = Direction::ALL
                .into_iter()
                .find(|direction| {
                    let (ox, oy) = direction.delta();
                    let next = (observer_location.0 + ox, observer_location.1 + oy);
                    map.is_walkable(next.0, next.1) && !on_route(next)
                })
                .expect("Observer cannot leave route");
            std::thread::sleep(Duration::from_millis(mir_proto::rules::MOVE_TIME + 50));
            observer.send(ClientMessage::Move {
                direction: escape,
                run: false,
            });
            match actor.wait(|message| matches!(message, ServerMessage::ObjectMove { id, .. } if *id == observer_id)) {
                ServerMessage::ObjectMove { to, .. } => assert!(!on_route((to.x, to.y))),
                _ => unreachable!(),
            }
        }
        if let Some(direction) = blocked {
            actor.send(ClientMessage::Move {
                direction,
                run: false,
            });
            match actor.wait(|message| matches!(message, ServerMessage::MoveDenied { .. })) {
                ServerMessage::MoveDenied { location, .. } => {
                    assert_eq!((location.x, location.y), start);
                }
                _ => unreachable!(),
            }
        }
        for step in 0..steps {
            std::thread::sleep(Duration::from_millis(mir_proto::rules::MOVE_TIME + 50));
            actor.send(ClientMessage::Move {
                direction,
                run: false,
            });
            let expected_from = (start.0 + dx * step, start.1 + dy * step);
            let expected_to = (expected_from.0 + dx, expected_from.1 + dy);
            match observer.wait(|message| {
                matches!(message, ServerMessage::ObjectMove { id, .. } if *id == actor_id)
                    || matches!(
                        message,
                        ServerMessage::MoveDenied { .. } | ServerMessage::Welcome { .. }
                    )
            }) {
                ServerMessage::ObjectMove { from, to, .. } => {
                    assert_eq!((from.x, from.y), expected_from);
                    assert_eq!((to.x, to.y), expected_to);
                }
                other => panic!("{label} movement failed: {other:?}"),
            }
        }
        println!("Wood branch {label}: {steps} acknowledged steps, no portal or collision bypass");
    }
    assert_eq!(std::fs::read(input).unwrap(), source);
    assert_eq!(
        std::fs::read(assets.join("../Database/System.db")).unwrap(),
        source_db
    );
}

#[test]
fn each_wood_branch_passage_is_traversed_authoritatively() {
    let (Some(assets), Some(input)) = (
        std::env::var_os("ZIRCON_ASSETS"),
        std::env::var_os("ZIRCON_BRANCH_MAP"),
    ) else {
        return;
    };
    verify_routes(
        &PathBuf::from(assets),
        &PathBuf::from(input),
        &[
            Route {
                label: "West",
                start: (46, 57),
                direction: Direction::Left,
                steps: 12,
                blocked: None,
            },
            Route {
                label: "North",
                start: (57, 46),
                direction: Direction::Up,
                steps: 12,
                blocked: None,
            },
            Route {
                label: "East",
                start: (66, 57),
                direction: Direction::Right,
                steps: 12,
                blocked: None,
            },
        ],
    );
}

#[test]
fn y_cross_and_cycle_passages_work_on_real_server() {
    let (Some(assets), Some(directory)) = (
        std::env::var_os("ZIRCON_ASSETS"),
        std::env::var_os("ZIRCON_BRANCH_QA_DIR"),
    ) else {
        return;
    };
    let assets = PathBuf::from(assets);
    let directory = PathBuf::from(directory);
    verify_routes(
        &assets,
        &directory.join("branch_y.map"),
        &[
            Route {
                label: "YNorthwest",
                start: (49, 49),
                direction: Direction::UpLeft,
                steps: 12,
                blocked: None,
            },
            Route {
                label: "YNortheast",
                start: (67, 45),
                direction: Direction::UpRight,
                steps: 16,
                blocked: None,
            },
            Route {
                label: "YSouth",
                start: (57, 66),
                direction: Direction::Down,
                steps: 12,
                blocked: None,
            },
        ],
    );
    verify_routes(
        &assets,
        &directory.join("branch_cross.map"),
        &[
            Route {
                label: "CrossWest",
                start: (40, 52),
                direction: Direction::Left,
                steps: 12,
                blocked: None,
            },
            Route {
                label: "CrossNorth",
                start: (57, 46),
                direction: Direction::Up,
                steps: 8,
                blocked: None,
            },
            Route {
                label: "CrossEast",
                start: (68, 52),
                direction: Direction::Right,
                steps: 12,
                blocked: Some(Direction::Up),
            },
            Route {
                label: "CrossSouth",
                start: (52, 68),
                direction: Direction::Down,
                steps: 6,
                blocked: None,
            },
        ],
    );
    verify_routes(
        &assets,
        &directory.join("branch_grid.map"),
        &[
            Route {
                label: "GridNorth",
                start: (52, 46),
                direction: Direction::Up,
                steps: 12,
                blocked: None,
            },
            Route {
                label: "GridWest",
                start: (46, 52),
                direction: Direction::Left,
                steps: 12,
                blocked: None,
            },
            Route {
                label: "GridEast",
                start: (66, 52),
                direction: Direction::Right,
                steps: 12,
                blocked: None,
            },
            Route {
                label: "GridSouth",
                start: (52, 66),
                direction: Direction::Down,
                steps: 12,
                blocked: None,
            },
            Route {
                label: "GridNorthToNW",
                start: (46, 27),
                direction: Direction::Left,
                steps: 12,
                blocked: None,
            },
            Route {
                label: "GridNWToWest",
                start: (27, 36),
                direction: Direction::Down,
                steps: 12,
                blocked: None,
            },
        ],
    );
}

#[test]
fn twelve_room_graph_supports_long_path_and_complete_cycle() {
    let (Some(assets), Some(directory)) = (
        std::env::var_os("ZIRCON_ASSETS"),
        std::env::var_os("ZIRCON_BRANCH_QA_DIR"),
    ) else {
        return;
    };
    let assets = PathBuf::from(assets);
    let input = PathBuf::from(directory).join("branch_grid12.map");
    let source = std::fs::read(&input).unwrap();
    let map = MapFile::parse(&source).unwrap();
    assert_eq!((map.width, map.height), (176, 176));
    let source_db = std::fs::read(assets.join("../Database/System.db")).unwrap();
    let start = (57, 57);
    for (label, segments) in [
        ("Grid12Long", &[(Direction::Right, 60)][..]),
        (
            "Grid12Cycle",
            &[
                (Direction::Right, 30),
                (Direction::Down, 30),
                (Direction::Left, 30),
                (Direction::Up, 30),
            ][..],
        ),
    ] {
        let (mut actor_at, mut observer_at) = (start, (start.0 - 1, start.1 - 1));
        for &(direction, steps) in segments {
            assert_eq!(steps % 2, 0);
            let (dx, dy) = direction.delta();
            for _ in 0..steps {
                actor_at = (actor_at.0 + dx, actor_at.1 + dy);
                observer_at = (observer_at.0 + dx, observer_at.1 + dy);
                assert!(map.is_walkable(actor_at.0, actor_at.1));
                assert!(map.is_walkable(observer_at.0, observer_at.1));
            }
        }
        let (_fixture, port) = start_fixture(&assets, &source, &source_db, start, label);
        let mut actor = Bot::connect(port);
        let (actor_id, location) = actor.enter(&format!("A{label}"));
        assert_eq!(location, start);
        let mut observer = Bot::connect(port);
        let (observer_id, location) = observer.enter(&format!("O{label}"));
        assert_eq!(location, (start.0 - 1, start.1 - 1));
        actor_at = start;
        observer_at = location;
        for &(direction, steps) in segments {
            let (dx, dy) = direction.delta();
            for _ in 0..steps / 2 {
                std::thread::sleep(Duration::from_millis(mir_proto::rules::MOVE_TIME + 50));
                actor.send(ClientMessage::Move {
                    direction,
                    run: true,
                });
                let next = (actor_at.0 + 2 * dx, actor_at.1 + 2 * dy);
                match observer.wait(|message| matches!(message, ServerMessage::ObjectMove { id, .. } if *id == actor_id)) {
                    ServerMessage::ObjectMove { from, to, .. } => {
                        assert_eq!((from.x, from.y), actor_at);
                        assert_eq!((to.x, to.y), next);
                    }
                    _ => unreachable!(),
                }
                actor_at = next;
                observer.send(ClientMessage::Move {
                    direction,
                    run: true,
                });
                let next = (observer_at.0 + 2 * dx, observer_at.1 + 2 * dy);
                match actor.wait(|message| matches!(message, ServerMessage::ObjectMove { id, .. } if *id == observer_id)) {
                    ServerMessage::ObjectMove { from, to, .. } => {
                        assert_eq!((from.x, from.y), observer_at);
                        assert_eq!((to.x, to.y), next);
                    }
                    _ => unreachable!(),
                }
                observer_at = next;
            }
        }
        assert_eq!(
            actor_at,
            if label == "Grid12Long" {
                (117, 57)
            } else {
                start
            }
        );
        println!(
            "Wood {label}: server acknowledged complete {}-cell route",
            segments.iter().map(|(_, n)| *n).sum::<i32>()
        );
    }
    assert_eq!(std::fs::read(input).unwrap(), source);
    assert_eq!(
        std::fs::read(assets.join("../Database/System.db")).unwrap(),
        source_db
    );
}
