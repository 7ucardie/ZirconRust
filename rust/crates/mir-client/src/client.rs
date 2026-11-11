//! Client shell: owns the server connection and routes input and messages to
//! the active scene (login, character select, game).

use mir_proto::{ClientMessage, ServerMessage, PROTOCOL_VERSION};

use crate::assets::Assets;
use crate::game::Game;
use crate::gfx::{Gpu, SpriteRenderer};
use crate::net::{Connection, NetEvent};
use crate::scenes::{LoginScene, SceneAction, SelectScene};
use crate::text::TextLayer;
use crate::ui::Input;

pub enum Scene {
    Login(Box<LoginScene>),
    Select(Box<SelectScene>),
    Game,
}

pub struct Client {
    pub game: Game,
    pub scene: Scene,
    pub input: Input,
    conn: Option<Connection>,
    server_addr: String,
    connected: bool,
    reconnect_at: u64,
    disconnect_reason: Option<String>,
    pub debug: bool,
    /// Developer automation: `ZIRCON_AUTOLOGIN=email:password` logs in (creating
    /// the account when missing); `ZIRCON_AUTOSTART=name` enters the world with
    /// that character, creating a warrior if none exists.
    auto_login: Option<(String, String)>,
    auto_start: Option<String>,
    auto_tried_create: bool,
    auto_tried_account: bool,
}

impl Client {
    pub fn new(assets: Assets, server_addr: String) -> Client {
        let db = std::env::var_os("ZIRCON_DB")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| assets.root().join("../Database/System.db"));
        let catalog = match crate::items::ItemCatalog::load(&db) {
            Ok(c) => {
                tracing::info!(path = %db.display(), items = c.len(), "item catalogue loaded");
                c
            }
            Err(e) => {
                tracing::warn!("no item catalogue: {e}");
                crate::items::ItemCatalog::empty()
            }
        };
        let mut c = Client {
            game: Game::new(assets, catalog),
            scene: Scene::Login(Box::new(LoginScene::new())),
            input: Input::default(),
            conn: None,
            server_addr,
            connected: false,
            reconnect_at: 0,
            disconnect_reason: None,
            debug: true,
            auto_login: std::env::var("ZIRCON_AUTOLOGIN").ok().and_then(|v| {
                v.split_once(':')
                    .map(|(e, p)| (e.to_string(), p.to_string()))
            }),
            auto_start: std::env::var("ZIRCON_AUTOSTART").ok(),
            auto_tried_create: false,
            auto_tried_account: false,
        };
        // ZIRCON_MOUSE=x,y parks the cursor for headless screenshots, so
        // hover-only overlays (tooltips) can be captured without a mouse.
        if let Some((x, y)) = std::env::var("ZIRCON_MOUSE").ok().and_then(|v| {
            v.split_once(',')
                .map(|(a, b)| (a.to_string(), b.to_string()))
        }) {
            if let (Ok(x), Ok(y)) = (x.trim().parse::<f32>(), y.trim().parse::<f32>()) {
                c.input.mouse = (x, y);
            }
        }
        c.connect();
        c
    }

    fn connect(&mut self) {
        match Connection::connect(&self.server_addr) {
            Ok(c) => {
                c.send(ClientMessage::Hello {
                    version: PROTOCOL_VERSION,
                });
                self.conn = Some(c);
                let status = match &self.disconnect_reason {
                    Some(reason) => format!("Connection lost: {reason}. Reconnecting..."),
                    None => format!("Connecting to {}...", self.server_addr),
                };
                self.set_status(status);
            }
            Err(e) => {
                self.conn = None;
                let status = match &self.disconnect_reason {
                    Some(reason) => format!("Connection lost: {reason}. Reconnect failed: {e}"),
                    None => format!("Cannot reach {}: {e}", self.server_addr),
                };
                self.set_status(status);
            }
        }
    }

    fn set_status(&mut self, s: String) {
        tracing::info!("{s}");
        match &mut self.scene {
            Scene::Login(l) => l.status = s,
            Scene::Select(sel) => sel.status = s,
            Scene::Game => self.game.status = s,
        }
    }

    /// Darkness colour for the light pass (white outside the world).
    pub fn light_clear(&self) -> [f32; 3] {
        self.game.light_clear()
    }

    pub fn send(&self, msg: ClientMessage) {
        if let Some(c) = &self.conn {
            c.send(msg);
        }
    }

    fn poll_network(&mut self, now: u64) {
        let Some(conn) = &self.conn else {
            if now >= self.reconnect_at {
                self.reconnect_at = now + 3000;
                self.connect();
            }
            return;
        };
        let mut events = Vec::new();
        while let Some(ev) = conn.poll() {
            events.push(ev);
        }
        for ev in events {
            match ev {
                NetEvent::Message(m) => self.handle(m, now),
                NetEvent::Disconnected(why) => {
                    self.conn = None;
                    self.connected = false;
                    self.game.leave_world();
                    self.scene = Scene::Login(Box::new(LoginScene::new()));
                    self.disconnect_reason = Some(if why == "server closed" {
                        self.disconnect_reason
                            .take()
                            .unwrap_or_else(|| "server stopped or closed the connection".into())
                    } else {
                        why
                    });
                    self.set_status(format!(
                        "Connection lost: {}. Reconnecting...",
                        self.disconnect_reason.as_deref().unwrap()
                    ));
                    self.reconnect_at = now + 2000;
                    return;
                }
            }
        }
    }

    fn handle(&mut self, msg: ServerMessage, now: u64) {
        match msg {
            ServerMessage::Connected => {
                self.connected = true;
                let status = match &self.disconnect_reason {
                    Some(reason) => {
                        format!("Connection lost: {reason}. Reconnected; log in again.")
                    }
                    None => "Connected. Log in or create an account.".into(),
                };
                self.set_status(status);
                if let Some((email, password)) = self.auto_login.clone() {
                    self.send(ClientMessage::Login { email, password });
                }
            }
            ServerMessage::Rejected { reason } => {
                if reason.starts_with("Kicked") || reason.starts_with("Account banned") {
                    self.disconnect_reason = Some(reason.clone());
                }
                self.set_status(format!("Rejected: {reason}"));
            }
            ServerMessage::NewAccountResult(r) => {
                if let Some((email, password)) = self.auto_login.clone() {
                    if matches!(r, mir_proto::NewAccountResult::Success) {
                        self.send(ClientMessage::Login { email, password });
                    }
                }
                if let Scene::Login(l) = &mut self.scene {
                    l.on_new_account(r);
                }
            }
            ServerMessage::LoginResult(r) => match r {
                mir_proto::LoginResult::Success { characters } => {
                    self.disconnect_reason = None;
                    if let Some(name) = self.auto_start.clone() {
                        match characters
                            .iter()
                            .find(|c| c.name.eq_ignore_ascii_case(&name))
                        {
                            Some(c) => {
                                let summary = c.clone();
                                self.game.character = Some(summary.clone());
                                self.send(ClientMessage::StartGame { id: summary.id });
                            }
                            None if !self.auto_tried_create => {
                                self.auto_tried_create = true;
                                let class = match std::env::var("ZIRCON_AUTOCLASS").ok().as_deref()
                                {
                                    Some("wizard") => mir_proto::Class::Wizard,
                                    Some("taoist") => mir_proto::Class::Taoist,
                                    Some("assassin") => mir_proto::Class::Assassin,
                                    _ => mir_proto::Class::Warrior,
                                };
                                self.send(ClientMessage::NewCharacter {
                                    name,
                                    class,
                                    gender: mir_proto::Gender::Male,
                                    hair: 1,
                                });
                            }
                            None => {}
                        }
                    }
                    self.scene = Scene::Select(Box::new(SelectScene::new(characters)));
                }
                mir_proto::LoginResult::Failed { reason } => {
                    if let Some((email, password)) = self.auto_login.clone() {
                        if reason == "Account not found" && !self.auto_tried_account {
                            self.auto_tried_account = true;
                            self.send(ClientMessage::NewAccount { email, password });
                        }
                    }
                    if let Scene::Login(l) = &mut self.scene {
                        l.status = reason;
                    }
                }
            },
            ServerMessage::NewCharacterResult(r) => {
                if let (Some(_), mir_proto::NewCharacterResult::Success { character }) =
                    (&self.auto_start, &r)
                {
                    self.game.character = Some(character.clone());
                    self.send(ClientMessage::StartGame { id: character.id });
                }
                if let Scene::Select(s) = &mut self.scene {
                    s.on_new_character(r);
                }
            }
            ServerMessage::DeleteCharacterResult { id, ok, reason } => {
                if let Scene::Select(s) = &mut self.scene {
                    s.on_deleted(id, ok, reason);
                }
            }
            ServerMessage::LoggedOut { characters } => {
                self.game.leave_world();
                self.scene = Scene::Select(Box::new(SelectScene::new(characters)));
            }
            ServerMessage::Welcome { .. } => {
                self.scene = Scene::Game;
                self.game.handle(msg, now);
            }
            other => {
                if matches!(self.scene, Scene::Game) {
                    self.game.handle(other, now);
                }
            }
        }
    }

    /// Per-frame update and scene drawing.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        text: &mut TextLayer,
        width: i32,
        height: i32,
        now: u64,
        fps: f32,
    ) {
        self.poll_network(now);
        match self.scene {
            Scene::Login(_) => self
                .game
                .audio
                .play_music(crate::sound_table::idx::LOGIN_SCENE),
            Scene::Select(_) => self
                .game
                .audio
                .play_music(crate::sound_table::idx::SELECT_SCENE),
            Scene::Game => {}
        }
        let action = match &mut self.scene {
            Scene::Login(l) => l.frame(
                &self.input,
                &mut self.game.assets,
                renderer,
                gpu,
                text,
                width,
                height,
                now,
            ),
            Scene::Select(s) => s.frame(
                &self.input,
                &mut self.game.assets,
                renderer,
                gpu,
                text,
                width,
                height,
                now,
            ),
            Scene::Game => {
                self.game.mouse = self.input.mouse;
                self.game.lmb = self.input.lmb_down;
                self.game.rmb = self.input.rmb_down;
                self.game.debug = self.debug;
                self.game.input = self.input.clone();
                self.game.update(now, width, height, self.conn.as_ref());
                self.game
                    .render(gpu, renderer, text, width, height, now, fps);
                for m in self.game.take_messages() {
                    self.send(m);
                }
                // Escape with nothing open opens the exit dialog (see
                // `menu.rs`); leaving is its Character list button.
                SceneAction::None
            }
        };
        match action {
            SceneAction::None => {}
            SceneAction::Send(msg) => {
                if let ClientMessage::StartGame { id } = &msg {
                    if let Scene::Select(s) = &self.scene {
                        self.game.character = s.character(*id).cloned();
                    }
                }
                self.send(msg);
            }
            SceneAction::Quit => std::process::exit(0),
        }
        if self.debug && !matches!(self.scene, Scene::Game) {
            let line = format!(
                "{} | {:.0} fps | server {}",
                if self.connected { "online" } else { "offline" },
                fps,
                self.server_addr
            );
            text.draw(&line, 12, 12.0, height as f32 - 20.0, [200, 200, 200, 255]);
        }
        self.input.end_frame();
    }
}
