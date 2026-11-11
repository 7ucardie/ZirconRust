//! Disposable local client/server session for the map editor.
use mir_formats::{
    map::MapFile,
    mirdb::{MirDb, Value},
};
use std::{
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

pub struct TestWalk {
    server: Child,
    client: Option<Child>,
    folder: PathBuf,
    assets: PathBuf,
    port: u16,
    started: Instant,
    launching: bool,
    pub status: String,
}
impl TestWalk {
    pub fn start(
        map: &MapFile,
        db: &MirDb,
        assets: &Path,
        at: (i32, i32),
        generated: Option<(&crate::generate::Layout, i32)>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            map.is_walkable(at.0, at.1),
            "Choose a walkable test position (right-click the map)."
        );
        let exe = std::env::current_exe()?;
        let bin = exe.parent().unwrap();
        let server_bin = bin.join(format!("mir-server{}", std::env::consts::EXE_SUFFIX));
        let client_bin = bin.join(format!("mir-client{}", std::env::consts::EXE_SUFFIX));
        anyhow::ensure!(
            server_bin.is_file() && client_bin.is_file(),
            "Build the workspace first so mir-server and mir-client are beside the editor."
        );
        let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let folder =
            std::env::temp_dir().join(format!("zircon-map-test-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(folder.join("Map"))?;
        map.save(folder.join("Map/EditorTest.map"))?;
        // Serialize a snapshot, then add an isolated map definition; no live DB writes.
        let mut copy = MirDb::parse(&db.to_bytes()?)?;
        if let Some((layout, monster)) = generated {
            crate::generate::populate(&mut copy, map, layout, "EditorTest", monster)?;
            layout
                .navigation(map)?
                .save(&folder.join("Map/EditorTest.map"), map)?;
        } else {
            copy.collection_mut("MapInfo")
                .ok_or_else(|| anyhow::anyhow!("MapInfo unavailable"))?
                .push_record(vec![
                    ("FileName", Value::Str("EditorTest".into())),
                    ("Description", Value::Str("Editor test map".into())),
                    ("Light", Value::Int(1)),
                ]);
        }
        copy.save(folder.join("System.db"))?;
        let log = std::fs::File::create(folder.join("server.log"))?;
        let server = Command::new(server_bin)
            .args(["--port", &port.to_string(), "--assets"])
            .arg(assets)
            .arg("--db")
            .arg(folder.join("System.db"))
            .arg("--data")
            .arg(folder.join("saves"))
            .args(["--map", "EditorTest"])
            .env("ZIRCON_MAP_DIR", folder.join("Map"))
            .env("ZIRCON_DEV_START", format!("EditorTest:{},{}", at.0, at.1))
            .env("ZIRCON_DEV_LEVEL", "70")
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        Ok(Self {
            server,
            client: None,
            folder,
            assets: assets.into(),
            port,
            started: Instant::now(),
            launching: true,
            status: "Starting a temporary server…".into(),
        })
    }
    pub fn tick(&mut self) {
        if self.launching {
            if self.server.try_wait().ok().flatten().is_some() {
                self.launching = false;
                self.status = format!(
                    "Test server stopped. See {}",
                    self.folder.join("server.log").display()
                );
                return;
            }
            if self.started.elapsed() > Duration::from_secs(20) {
                self.launching = false;
                let _ = self.server.kill();
                self.status = "Server startup timed out.".into();
                return;
            }
            if TcpStream::connect_timeout(
                &([127, 0, 0, 1], self.port).into(),
                Duration::from_millis(5),
            )
            .is_ok()
            {
                self.launching = false;
                let exe = std::env::current_exe().unwrap();
                let mut cmd = Command::new(
                    exe.parent()
                        .unwrap()
                        .join(format!("mir-client{}", std::env::consts::EXE_SUFFIX)),
                );
                cmd.arg("--assets")
                    .arg(&self.assets)
                    .args(["--server", &format!("127.0.0.1:{}", self.port)])
                    .env("ZIRCON_MAP_DIR", self.folder.join("Map"))
                    .env("ZIRCON_DB", self.folder.join("System.db"))
                    .env("ZIRCON_AUTOLOGIN", "maptest@local.test:maptest123")
                    .env("ZIRCON_AUTOSTART", "TileTester")
                    .env("ZIRCON_MUTE", "1")
                    .env_remove("ZIRCON_SCREENSHOT");
                if let Ok(shot) = std::env::var("ZIRCON_TILE_TEST_SCREENSHOT") {
                    cmd.env("ZIRCON_SCREENSHOT", shot)
                        .env("ZIRCON_HEADLESS", "1");
                }
                match std::fs::File::create(self.folder.join("client.log"))
                    .and_then(|f| cmd.stdout(f.try_clone()?).stderr(f).spawn())
                {
                    Ok(client) => {
                        self.client = Some(client);
                        self.status = format!("Test game running. Logs: {}", self.folder.display());
                    }
                    Err(e) => {
                        let _ = self.server.kill();
                        self.status = format!("Client could not start: {e}");
                    }
                }
            }
        } else if self
            .client
            .as_mut()
            .is_some_and(|c| c.try_wait().ok().flatten().is_some())
        {
            self.client = None;
            let _ = self.server.kill();
            let _ = self.server.wait();
            self.status = format!(
                "Test finished. Snapshot and logs: {}",
                self.folder.display()
            );
        }
    }
}
impl Drop for TestWalk {
    fn drop(&mut self) {
        if let Some(c) = &mut self.client {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}
