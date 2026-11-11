//! `mir-editor`: desktop editor for Zircon's `System.db`.
//!
//! ```text
//! mir-editor [--db FILE] [--assets DIR]
//! ```
//! Defaults come from `ZIRCON_ASSETS` (`<assets>/../Database/System.db` and
//! `<assets>/Data` for sprite previews). Every save writes a timestamped
//! backup next to the file (see `mir_formats::save_with_backup`).
//!
//! Automation for visual checks: `ZIRCON_SCREENSHOT=path.png:secs` saves a
//! frame and exits; `ZIRCON_EDITOR_COLLECTION=ItemInfo`,
//! `ZIRCON_EDITOR_FILTER=Potion` and `ZIRCON_EDITOR_ROW=0` preselect state;
//! `ZIRCON_EDITOR_MAP=605 ZIRCON_EDITOR_REGION=1711` opens map placement;
//! `ZIRCON_EDITOR_TILE_MAP=/path/to/copy.map` opens the tile workspace;
//! `ZIRCON_EDITOR_TEST_WALK=1` launches an isolated client/server session.
//! `ZIRCON_EDITOR_VALIDATE=1` runs the validation on open (`break` first
//! corrupts a spawn so the panel shows a finding).

mod app;
mod castle_gate_authoring;
mod castle_gate_ui;
mod cave_art;
mod drop_draft;
mod dungeon;
mod freeform_art;
mod generate;
mod item_draft;
mod item_draft_ui;
mod maps;
mod mcp_authoring;
mod mcp_protocol;
mod monster_draft;
mod palace_kit;
mod piece_assembly;
mod piece_browser;
mod piece_candidates;
mod piece_catalogue;
mod piece_composition;
mod piece_ports;
mod preview;
mod sprite_authoring;
mod sprite_contract;
#[cfg(test)]
mod sprite_mcp_tests;
mod sprite_preview_ui;
mod sprite_project;
mod sprite_publish;
mod sprite_ui;
mod sprite_variant;
mod test_walk;
mod tile_doc;
mod tiles;
mod value;
mod wall_kit;
mod wall_piece;
mod wearable_publish;
mod wood_branch;
mod wood_concave;
mod wood_connected;
mod wood_corners;
mod wood_door_art;
mod wood_doorway;
mod wood_entry;
mod wood_ne;
mod wood_perimeter;
mod wood_perimeter_kit;
mod wood_wall_bodies;
mod wood_wall_entry;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

struct Args {
    db: PathBuf,
    assets: PathBuf,
    mcp: bool,
    project: Option<PathBuf>,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut db = None;
    let mut mcp = false;
    let mut project = None;
    let mut assets = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--mcp" => mcp = true,
            "--project" => project = it.next().map(PathBuf::from),
            "--db" => db = it.next().map(PathBuf::from),
            "--assets" => assets = it.next().map(PathBuf::from),
            "--help" | "-h" => {
                println!("mir-editor [--db FILE] [--assets DIR] [--mcp --project DIR]");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    let assets = assets.ok_or_else(|| {
        anyhow::anyhow!("set ZIRCON_ASSETS or pass --assets DIR (the client root with Data/)")
    })?;
    let db = db.unwrap_or_else(|| assets.join("../Database/System.db"));
    Ok(Args {
        db,
        assets,
        mcp,
        project,
    })
}

/// Screenshot automation: request a frame after the delay, save it, quit.
struct Shot {
    path: PathBuf,
    at: Instant,
    requested: bool,
}

impl Shot {
    fn from_env() -> Option<Shot> {
        let v = std::env::var("ZIRCON_SCREENSHOT").ok()?;
        let (path, secs) = v.rsplit_once(':').unwrap_or((&v, "3"));
        let secs: f64 = secs.parse().unwrap_or(3.0);
        Some(Shot {
            path: PathBuf::from(path),
            at: Instant::now() + Duration::from_secs_f64(secs),
            requested: false,
        })
    }

    fn tick(&mut self, ctx: &egui::Context) {
        // Screenshot readback completes asynchronously. Keep rendering while
        // automation waits, including when the native window is unfocused.
        ctx.request_repaint();
        if !self.requested {
            if Instant::now() >= self.at {
                self.requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            match save_png(&self.path, w as u32, h as u32, image.as_raw()) {
                Ok(()) => eprintln!("screenshot saved to {}", self.path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn save_png(path: &PathBuf, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(rgba)?;
    Ok(())
}

struct Root {
    app: app::Editor,
    shot: Option<Shot>,
    confirm_close: bool,
    allow_close: bool,
}

impl eframe::App for Root {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.ctx().input(|i| i.viewport().close_requested())
            && self.app.unsaved()
            && !self.allow_close
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        self.app.ui(ui);
        if self.confirm_close {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("You have unsaved map, database or queued loot changes.");
                    ui.horizontal(|ui| {
                        if ui.button("Keep editing").clicked() {
                            self.confirm_close = false;
                        }
                        if ui.button("Discard and close").clicked() {
                            self.allow_close = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
        }
        if let Some(shot) = &mut self.shot {
            shot.tick(ui.ctx());
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    if args.mcp {
        let project = args
            .project
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--mcp requires --project DIR"))?;
        let workspace = mcp_authoring::Workspace::open(project, &args.assets, &args.db)?;
        return mcp_protocol::run(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            mcp_authoring::tools(),
            |name, arguments| workspace.call(name, arguments),
        );
    }
    anyhow::ensure!(args.project.is_none(), "--project is only used with --mcp");
    let app = app::Editor::open(args.db, args.assets)?;
    let shot = Shot::from_env();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ZirconRust Editor")
            .with_inner_size([1400.0, 860.0]),
        ..Default::default()
    };
    eframe::run_native(
        "mir-editor",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(Root {
                app,
                shot,
                confirm_close: false,
                allow_close: false,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
