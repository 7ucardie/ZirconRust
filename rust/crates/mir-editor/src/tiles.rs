//! Tile workspace: palette, visible-cell rendering, gesture tools and test play.
use crate::{
    preview::Previews,
    test_walk::TestWalk,
    tile_doc::{self, Adjacency, Document, Layer, Stamp, Tile},
};
use eframe::egui::{self, Color32};
use mir_formats::{
    map::{kr_order, MapFile},
    mirdb::MirDb,
    monster_table::kr_library,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
};

struct PrivateArt {
    folder: PathBuf,
    client: PathBuf,
}

impl PrivateArt {
    fn create(source: &Path, library: &[u8]) -> anyhow::Result<Self> {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let folder =
            std::env::temp_dir().join(format!("zircon-editor-art-{}-{unique}", std::process::id()));
        std::fs::create_dir(&folder)?;
        match write_private_art(source, &folder, library) {
            Ok(client) => Ok(Self { folder, client }),
            Err(error) => {
                let _ = std::fs::remove_dir_all(&folder);
                Err(error)
            }
        }
    }
}

impl Drop for PrivateArt {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn highest_wood_image(map: &MapFile) -> Option<usize> {
    (0..i32::from(map.height))
        .flat_map(|y| (0..i32::from(map.width)).map(move |x| map.cell(x, y).unwrap()))
        .flat_map(|cell| {
            [
                (cell.middle_file == 21).then_some(cell.middle_image as usize),
                (cell.front_file == 21).then_some(cell.front_image as usize),
            ]
        })
        .flatten()
        .max()
}

fn write_private_art(source: &Path, folder: &Path, library: &[u8]) -> anyhow::Result<PathBuf> {
    let relative = PathBuf::from(crate::wood_branch::LIBRARY);
    let client = folder.join("Client");
    let overrides = BTreeMap::from([(relative.clone(), library.to_vec())]);
    crate::sprite_publish::asset_view(source, source, &client, &overrides)?;
    let path = client.join(relative);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, library)?;
    Ok(client)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pen,
    Rectangle,
    Pick,
    Copy,
    Stamp,
    Piece,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum GenerationMode {
    Organic,
    Enclosed,
    Rooms,
    WoodPerimeter,
    WoodConnected,
}
pub struct Tiles {
    pub doc: Option<Document>,
    path: String,
    assets: PathBuf,
    previews: Previews,
    layer: Layer,
    tool: Tool,
    tile: Tile,
    erase: bool,
    walkable: bool,
    light: u8,
    page: u32,
    zoom: f32,
    overlays: bool,
    grid: bool,
    start: Option<(i32, i32)>,
    last: Option<(i32, i32)>,
    stroke: BTreeSet<(i32, i32)>,
    stamp: Option<Stamp>,
    wall_piece: crate::wall_piece::Panel,
    adjacency: Option<Adjacency>,
    auto: bool,
    scroll: Option<egui::Vec2>,
    test_at: (i32, i32),
    run: Option<TestWalk>,
    auto_test: bool,
    width: u16,
    height: u16,
    seed: u64,
    generation_mode: GenerationMode,
    room_count: usize,
    wall_nw: u16,
    wall_ne: u16,
    wall_rooms: u8,
    wall_varied: bool,
    wall_room_repeats: [[u16; 2]; 3],
    corridor_width: u16,
    generated: Option<crate::generate::Layout>,
    private_art: Option<Vec<u8>>,
    private_assets: Option<PrivateArt>,
    sidecars_pending: bool,
    monster: i32,
    export_path: String,
    pub status: String,
    pub registration: Option<crate::maps::Edit>,
}
impl Tiles {
    pub fn new(assets: &Path) -> Self {
        let path = std::env::var("ZIRCON_EDITOR_TILE_MAP").unwrap_or_default();
        let mut s = Self {
            doc: None,
            path,
            assets: assets.into(),
            previews: Previews::new(assets),
            layer: Layer::Back,
            tool: Tool::Pen,
            tile: Tile {
                file: 0,
                image: 0,
                animation: 0,
            },
            erase: false,
            walkable: true,
            light: 0,
            page: 0,
            zoom: std::env::var("ZIRCON_TILE_ZOOM")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite() && (0.1..=2.0).contains(v))
                .unwrap_or(0.75),
            overlays: false,
            grid: false,
            start: None,
            last: None,
            stroke: BTreeSet::new(),
            stamp: None,
            wall_piece: crate::wall_piece::Panel::default(),
            adjacency: None,
            auto: false,
            scroll: None,
            test_at: (0, 0),
            run: None,
            auto_test: std::env::var_os("ZIRCON_EDITOR_TEST_WALK").is_some(),
            width: 64,
            height: 64,
            seed: 42,
            generation_mode: match std::env::var("ZIRCON_CAVE_MODE").as_deref() {
                Ok("enclosed") => GenerationMode::Enclosed,
                Ok("rooms") => GenerationMode::Rooms,
                Ok("wood-perimeter") => GenerationMode::WoodPerimeter,
                Ok("wood-connected") => GenerationMode::WoodConnected,
                _ => GenerationMode::Organic,
            },
            room_count: 8,
            wall_nw: 3,
            wall_ne: 4,
            wall_rooms: 2,
            wall_varied: false,
            wall_room_repeats: [[3, 4]; 3],
            corridor_width: 4,
            generated: None,
            private_art: None,
            private_assets: None,
            sidecars_pending: false,
            monster: 0,
            export_path: String::new(),
            status: String::new(),
            registration: None,
        };
        if !s.path.is_empty() {
            s.open();
        }
        match s.wall_piece.autoload(assets) {
            Ok(true) => s.tool = Tool::Piece,
            Ok(false) => {}
            Err(e) => s.status = format!("Piece load failed: {e}"),
        }
        s
    }
    pub fn dirty(&self) -> bool {
        self.doc
            .as_ref()
            .is_some_and(|d| d.dirty() || self.sidecars_pending)
    }
    pub fn can_undo(&self) -> bool {
        self.doc.as_ref().is_some_and(Document::can_undo)
    }
    pub fn undo(&mut self) {
        if let Some(d) = &mut self.doc {
            d.undo();
            self.status = "Map gesture undone.".into();
        }
    }
    pub fn save(&mut self) {
        if let Some(d) = &mut self.doc {
            if self.path.trim().is_empty() {
                self.status = "Enter a .map path before saving.".into();
                return;
            }
            if let Some(layout) = &self.generated {
                if let Err(e) = layout.validate(&d.map) {
                    self.status = format!("Save refused: {e}");
                    return;
                }
            }
            match d.save(Path::new(&self.path)) {
                Ok(b) => {
                    if let Some(art) = &self.private_art {
                        if let Err(e) = mir_formats::save_with_backup(
                            Path::new(&self.path).with_extension("wood.zl"),
                            art,
                        ) {
                            self.status =
                                format!("Map saved, but private wall art could not be saved: {e}");
                            return;
                        }
                    }
                    self.sidecars_pending = self.generated.is_some();
                    if let Some(layout) = &self.generated {
                        let result = serde_json::to_vec_pretty(&(layout, self.monster))
                            .map_err(anyhow::Error::from)
                            .and_then(|bytes| {
                                mir_formats::save_with_backup(
                                    Path::new(&self.path).with_extension("cave.json"),
                                    &bytes,
                                )
                                .map_err(anyhow::Error::from)
                            });
                        if let Err(e) = result {
                            self.status =
                                format!("Map saved, but cave placements could not be saved: {e}");
                            return;
                        }
                    }
                    if let Some(layout) = &self.generated {
                        if let Err(e) = layout.navigation(&d.map).and_then(|navigation| {
                            navigation
                                .save(Path::new(&self.path), &d.map)
                                .map_err(anyhow::Error::from)
                        }) {
                            self.status =
                                format!("Map saved, but portal markers could not be saved: {e}");
                            return;
                        }
                    }
                    self.sidecars_pending = false;
                    self.status = b
                        .map(|p| format!("Map saved; backup: {}", p.display()))
                        .unwrap_or_else(|| "Map saved.".into())
                }
                Err(e) => self.status = format!("Save failed: {e}"),
            }
        }
    }
    fn open(&mut self) {
        if self.dirty() {
            self.status =
                "Save or discard the current map changes before opening another map.".into();
            return;
        }
        let loaded = (|| -> anyhow::Result<(Document, Option<Vec<u8>>, Option<PrivateArt>)> {
            let d = Document::open(&self.path)?;
            let path = Path::new(&self.path);
            let sidecar = path.with_extension("wood.zl");
            let sibling = path
                .parent()
                .and_then(Path::parent)
                .map(|p| p.join("Client"));
            let source = if sidecar.is_file() {
                self.assets.as_path()
            } else if sibling
                .as_ref()
                .is_some_and(|p| p.join(crate::wood_branch::LIBRARY).is_file())
            {
                sibling.as_deref().unwrap()
            } else {
                self.assets.as_path()
            };
            let art = if sidecar.is_file() {
                Some(std::fs::read(sidecar)?)
            } else if source != self.assets {
                Some(std::fs::read(source.join(crate::wood_branch::LIBRARY))?)
            } else {
                None
            };
            let private = art
                .as_ref()
                .map(|bytes| PrivateArt::create(source, bytes))
                .transpose()?;
            if let Some(index) = highest_wood_image(&d.map) {
                let active = private
                    .as_ref()
                    .map_or(self.assets.as_path(), |p| p.client.as_path());
                let count =
                    mir_formats::zl::ZlLibrary::open(active.join(crate::wood_branch::LIBRARY))?
                        .len();
                anyhow::ensure!(index < count, "Map references Wood art slot {index}, but its private wall library is missing or incomplete.");
            }
            Ok((d, art, private))
        })();
        match loaded {
            Ok((d, art, private)) => {
                self.doc = Some(d);
                self.opened();
                if let Some(private) = private {
                    self.previews = Previews::new(&private.client);
                    self.private_assets = Some(private);
                    self.private_art = art;
                }
                match crate::generate::read_layout(Path::new(&self.path)) {
                    Ok(Some((layout, monster))) => {
                        if std::env::var_os("ZIRCON_TILE_CENTER").is_none() {
                            self.test_at = layout.start;
                        }
                        self.generation_mode = if !layout.wall_room_repeats.is_empty() {
                            self.wall_varied = true;
                            self.wall_rooms = layout.wall_rooms;
                            for (target, repeats) in self
                                .wall_room_repeats
                                .iter_mut()
                                .zip(&layout.wall_room_repeats)
                            {
                                *target = *repeats;
                            }
                            GenerationMode::WoodConnected
                        } else if let Some([nw, ne]) = layout.wall_repeats {
                            self.wall_varied = false;
                            self.wall_nw = nw;
                            self.wall_ne = ne;
                            if layout.wall_rooms >= 2 {
                                self.wall_rooms = layout.wall_rooms;
                                GenerationMode::WoodConnected
                            } else {
                                GenerationMode::WoodPerimeter
                            }
                        } else if layout.enclosed {
                            GenerationMode::Enclosed
                        } else if !layout.rooms.is_empty() {
                            GenerationMode::Rooms
                        } else {
                            GenerationMode::Organic
                        };
                        self.seed = layout.seed;
                        if !layout.rooms.is_empty() {
                            self.room_count = layout.rooms.len();
                            self.corridor_width = layout.corridor_width;
                        }
                        self.generated = Some(layout);
                        self.monster = monster;
                        self.center_test();
                        self.status="Generated cave loaded, including placements. Use Export to save a complete bundle.".into();
                    }
                    Ok(None) => {}
                    Err(e) => {
                        self.status =
                            format!("Map loaded, but cave placements could not be read: {e}")
                    }
                }
            }
            Err(e) => self.status = format!("Could not open map: {e}"),
        }
    }
    fn opened(&mut self) {
        self.run = None;
        self.previews = Previews::new(&self.assets);
        self.private_assets = None;
        self.private_art = None;
        self.sidecars_pending = false;
        self.generated = None;
        self.adjacency = None;
        self.auto = false;
        self.stamp = None;
        self.start = None;
        self.last = None;
        self.stroke.clear();
        if let Some(d) = &self.doc {
            self.test_at = std::env::var("ZIRCON_TILE_CENTER")
                .ok()
                .and_then(|v| {
                    v.split_once(',')
                        .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))
                })
                .unwrap_or_else(|| {
                    (0..d.map.height as i32)
                        .find_map(|y| {
                            (0..d.map.width as i32)
                                .find(|&x| d.map.is_walkable(x, y))
                                .map(|x| (x, y))
                        })
                        .unwrap_or((0, 0))
                });
            self.scroll = Some(egui::vec2(
                (self.test_at.0 as f32 * 48.0 * self.zoom - 400.0).max(0.0),
                (self.test_at.1 as f32 * 32.0 * self.zoom - 250.0).max(0.0),
            ));
            if let Some(c) = d.map.cell(self.test_at.0 & !1, self.test_at.1 & !1) {
                self.tile = Tile::read(c, self.layer);
                self.page = self.tile.image as u32 / 32;
            }
        }
        self.status = "Map loaded. Right-click a walkable cell to choose the test position.".into();
    }
    fn start_test(&mut self, db: &MirDb) {
        if let Some(d) = &self.doc {
            match TestWalk::start(
                &d.map,
                db,
                self.private_assets
                    .as_ref()
                    .map_or(self.assets.as_path(), |p| p.client.as_path()),
                self.test_at,
                self.generated.as_ref().map(|l| (l, self.monster)),
            ) {
                Ok(r) => {
                    self.run = Some(r);
                    self.status = "Testing an isolated snapshot of the current map.".into();
                }
                Err(e) => self.status = e.to_string(),
            }
        }
    }
    fn register(&mut self, db: &MirDb) {
        let Some(doc) = self.doc.as_ref().filter(|d| !d.dirty()) else {
            self.status = "Save the map file before registering it.".into();
            return;
        };
        let Some(file) = doc
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
        else {
            return;
        };
        let exists = db.collection("MapInfo").is_some_and(|c| {
            c.records
                .iter()
                .any(|r| c.str_or(r, "FileName", "").eq_ignore_ascii_case(file))
        });
        if exists {
            self.status = "This map already has a database record.".into();
            return;
        }
        self.registration = Some(crate::maps::Edit {
            collection: "MapInfo",
            index: None,
            values: vec![
                ("FileName", mir_formats::mirdb::Value::Str(file.into())),
                ("Description", mir_formats::mirdb::Value::Str(file.into())),
            ],
        });
        self.status = "Map registered. Save the database in Tables; keep the map in the server's map directory.".into();
    }

    fn center_test(&mut self) {
        self.scroll = Some(egui::vec2(
            (self.test_at.0 as f32 * 48.0 * self.zoom - 400.0).max(0.0),
            (self.test_at.1 as f32 * 32.0 * self.zoom - 250.0).max(0.0),
        ));
    }
    fn export_generated(&self, db: &MirDb, folder: &Path) -> anyhow::Result<()> {
        let doc = self
            .doc
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No map is open."))?;
        let layout = self
            .generated
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No generated layout is open."))?;
        crate::generate::export(folder, &doc.map, db, layout, self.monster)?;
        let result = (|| -> anyhow::Result<()> {
            if let Some(art) = &self.private_art {
                let source = self
                    .private_assets
                    .as_ref()
                    .map_or(self.assets.as_path(), |p| p.client.as_path());
                let client = write_private_art(source, folder, art)?;
                writeln!(
                    std::fs::OpenOptions::new()
                        .append(true)
                        .open(folder.join("README.txt"))?,
                    "Set ZIRCON_ASSETS={} for private Wood art.",
                    client.display()
                )?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(folder);
        }
        result
    }
    fn generation_ui(&mut self, ui: &mut egui::Ui, db: &MirDb) {
        egui::CollapsingHeader::new("Generate cave or room map")
            .default_open(self.generated.is_some())
            .show(ui, |ui| self.generation_controls(ui, db));
    }
    fn generation_controls(&mut self, ui: &mut egui::Ui, db: &MirDb) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.generation_mode,
                GenerationMode::Organic,
                "Organic cave",
            );
            ui.selectable_value(
                &mut self.generation_mode,
                GenerationMode::Enclosed,
                "Enclosed chamber",
            );
            ui.selectable_value(
                &mut self.generation_mode,
                GenerationMode::Rooms,
                "Rooms + corridors",
            );
        });
        ui.selectable_value(
            &mut self.generation_mode,
            GenerationMode::WoodPerimeter,
            "Wood wall perimeter",
        );
        ui.selectable_value(
            &mut self.generation_mode,
            GenerationMode::WoodConnected,
            "Wood connected rooms",
        );
        let wood = matches!(
            self.generation_mode,
            GenerationMode::WoodPerimeter | GenerationMode::WoodConnected
        );
        let connected = self.generation_mode == GenerationMode::WoodConnected;
        if connected {
            ui.horizontal(|ui| {
                ui.label("Connected rooms");
                ui.add(egui::DragValue::new(&mut self.wall_rooms).range(2..=3));
            });
            ui.checkbox(&mut self.wall_varied, "Different room sizes");
        }
        if wood {
            ui.label(if connected { "Enclosed rooms joined by a walkable passage, with complete source wall artwork." } else { "Assembles source wall bodies and four corner groups into a closed room. Side lengths vary; artwork is never rotated." });
            let maximum = if connected && self.wall_rooms == 3 && !self.wall_varied {
                4
            } else {
                10
            };
            self.wall_nw = self.wall_nw.clamp(3, maximum);
            self.wall_ne = self.wall_ne.clamp(3, maximum);
            if connected && self.wall_varied {
                for (i, room) in self
                    .wall_room_repeats
                    .iter_mut()
                    .take(usize::from(self.wall_rooms))
                    .enumerate()
                {
                    ui.horizontal(|ui| {
                        ui.label(format!("Room {} NW / NE", i + 1));
                        ui.add(egui::DragValue::new(&mut room[0]).range(3..=10));
                        ui.add(egui::DragValue::new(&mut room[1]).range(3..=10));
                    });
                }
            } else {
                ui.horizontal(|ui| {
                    ui.label("NW / NE repeats");
                    ui.add(egui::DragValue::new(&mut self.wall_nw).range(3..=maximum));
                    ui.add(egui::DragValue::new(&mut self.wall_ne).range(3..=maximum));
                });
            }
            if connected {
                ui.small(
                    "Wood wall art frames static, passable thresholds. No open/close interaction.",
                );
            }
        } else if self.generation_mode == GenerationMode::Enclosed {
            ui.label("Preserves the complete source chamber and its authored perimeter. The seed varies its placement, not its outline.");
            ui.label(
                "Use a compact closed source such as D1006. The canvas must fit every wall frame.",
            );
            if ui
                .add_enabled(self.doc.is_some(), egui::Button::new("Fit source artwork"))
                .clicked()
            {
                match crate::generate::chamber_size(&self.doc.as_ref().unwrap().map, &self.assets) {
                    Ok((w, h)) => {
                        self.width = w;
                        self.height = h;
                        self.status =
                            format!("Canvas set to {w}×{h} to fit the complete source artwork.");
                    }
                    Err(e) => self.status = e.to_string(),
                }
            }
        } else if self.generation_mode == GenerationMode::Rooms {
            ui.label("Connected rooms with protected passages, complete Wood edge frames and passable thresholds.");
            ui.horizontal(|ui| {
                ui.label("Rooms");
                ui.add(egui::DragValue::new(&mut self.room_count).range(2..=12));
                ui.label("Corridor width");
                ui.add(
                    egui::DragValue::new(&mut self.corridor_width)
                        .range(2..=6)
                        .speed(2),
                );
            });
        } else {
            ui.label("Reuses complete formations and frames all exposed organic boundaries with private Wood art.");
        }
        ui.label(if wood {
            "Dimensions follow the repeat counts. Save changes before generating."
        } else {
            "Size: even, 32–256. Save changes before generating."
        });
        ui.horizontal(|ui| {
            ui.label("Seed");
            ui.add(egui::DragValue::new(&mut self.seed));
        });
        if ui
            .add_enabled(
                (self.doc.is_some() || wood) && !self.dirty(),
                egui::Button::new(if connected {
                    "Generate connected Wood rooms"
                } else if wood {
                    "Generate Wood perimeter"
                } else {
                    "Generate from open map"
                }),
            )
            .clicked()
        {
            let source = self.doc.as_ref().map(|d| &d.map);
            let active_assets = self
                .private_assets
                .as_ref()
                .map_or(self.assets.as_path(), |p| p.client.as_path());
            let result = if connected {
                let built = if self.wall_varied {
                    crate::wood_connected::generate_varied_with_necks(
                        &self.assets,
                        &self.wall_room_repeats[..usize::from(self.wall_rooms)],
                        self.seed,
                    )
                } else {
                    crate::wood_connected::generate_rooms_with_necks(
                        &self.assets,
                        self.wall_nw,
                        self.wall_ne,
                        self.wall_rooms,
                        self.seed,
                    )
                };
                built.and_then(|(doc, layout, necks)| {
                    let centers = necks.iter().map(|&neck| crate::wood_doorway::inspect(&doc.map, neck).map(|door| door.center)).collect::<anyhow::Result<Vec<_>>>()?;
                    Ok((doc, layout, format!("Connected Wood rooms assembled. Static passable thresholds: {centers:?}."), None))
                })
            } else if wood {
                crate::wood_perimeter::generate(&self.assets,self.wall_nw,self.wall_ne,self.seed).map(|(d,l)|(d,l,"Closed Wood perimeter assembled; interior floor and gameplay routes validated.".into(),None))
            } else if self.generation_mode == GenerationMode::Rooms {
                crate::dungeon::generate_full_art(
                    source.unwrap(),
                    self.width,
                    self.height,
                    self.seed,
                    self.room_count,
                    self.corridor_width,
                    active_assets,
                )
                .map(|(doc, layout, report, art, walls)| {
                    (
                        doc,
                        layout,
                        format!(
                            "{} rooms joined by {}-cell corridors; {} formations and decorations; {walls} framed edge cells.",
                            self.room_count,
                            self.corridor_width,
                            report.walls + report.decorations
                        ),
                        Some(art),
                    )
                })
            } else if self.generation_mode == GenerationMode::Enclosed {
                crate::generate::enclosed_chamber(
                    source.unwrap(),
                    self.width,
                    self.height,
                    self.seed,
                    &self.assets,
                )
                .map(|(doc, layout)| {
                    (
                        doc,
                        layout,
                        "Complete source chamber and perimeter preserved.".to_owned(),
                        None,
                    )
                })
            } else {
                crate::generate::cave_with_full_art(
                    source.unwrap(),
                    self.width,
                    self.height,
                    self.seed,
                    active_assets,
                )
                .map(|(doc, layout, report, art, walls)| {
                    (
                        doc,
                        layout,
                        format!(
                            "{} formations, {} decorations from {} templates; {walls} framed edge cells.",
                            report.walls, report.decorations, report.templates
                        ),
                        Some(art),
                    )
                })
            };
            match result {
                Ok((doc, layout, description, art)) => {
                    let private = match art
                        .as_ref()
                        .map(|bytes| PrivateArt::create(active_assets, bytes))
                        .transpose()
                    {
                        Ok(private) => private,
                        Err(e) => {
                            self.status = format!("Private art setup failed: {e}");
                            return;
                        }
                    };
                    self.doc = Some(doc);
                    self.path.clear();
                    self.opened();
                    if let Some(private) = private {
                        self.previews = Previews::new(&private.client);
                        self.private_assets = Some(private);
                        self.private_art = art;
                    }
                    self.test_at = layout.start;
                    self.center_test();
                    self.status = format!(
                        "Seed {}. {description} Select a monster for the eight spawns.",
                        layout.seed
                    );
                    self.generated = Some(layout);
                }
                Err(e) => self.status = e.to_string(),
            }
        }
        let Some(layout) = &mut self.generated else {
            return;
        };
        let mut loot_changed = false;
        egui::CollapsingHeader::new("Spawn loot conditions")
            .default_open(layout.drop_set != 0 || layout.easter_event_chance != 0)
            .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Drop-set mask");
                loot_changed |= ui.add(egui::DragValue::new(&mut layout.drop_set)).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Easter marking: 1 in");
                loot_changed |= ui.add(egui::DragValue::new(&mut layout.easter_event_chance).range(0..=i32::MAX)).changed();
            });
            ui.label("0 disables Easter marking. The server event end time must also be active. Conditions are fixed when monsters spawn.");
        });
        if loot_changed {
            self.sidecars_pending = true;
        }
        if let Some(c) = db.collection("MonsterInfo") {
            let name = c
                .records
                .iter()
                .find(|r| c.index(r) == self.monster)
                .map(|r| c.str_or(r, "MonsterName", "Monster"))
                .unwrap_or("Choose monster…");
            egui::ComboBox::from_id_salt("cave_monster")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for r in &c.records {
                        ui.selectable_value(
                            &mut self.monster,
                            c.index(r),
                            c.str_or(r, "MonsterName", "Monster"),
                        );
                    }
                });
        }
        if ui.button("Validate cave and placements").clicked() {
            self.status = match layout.validate(&self.doc.as_ref().unwrap().map) {
                Ok(n) => format!("Valid: {n} connected floor cells; all placements reachable."),
                Err(e) => e.to_string(),
            };
        }
        ui.label("Export to a new folder (parent must exist)");
        ui.text_edit_singleline(&mut self.export_path);
        if ui.button("Export map + database snapshot").clicked() {
            let folder = Path::new(&self.export_path);
            self.status = match self.export_generated(db, folder) {
                Ok(()) => format!("Bundle exported to {}", self.export_path),
                Err(e) => format!("Export failed: {e}"),
            };
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, db: &MirDb) {
        self.previews.begin_frame();
        if let Some(run) = &mut self.run {
            run.tick();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.auto_test && self.doc.is_some() {
            self.auto_test = false;
            self.start_test(db);
        }
        egui::Panel::left("tile_tools")
            .default_size(325.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Tile painting");
                    ui.label("Open / save map path");
                    ui.text_edit_singleline(&mut self.path);
                    ui.horizontal(|ui| {
                        if ui.button("Open map").clicked() {
                            self.open();
                        }
                        if ui.button("Save map").clicked() {
                            self.save();
                        }
                    });
                    if ui.button("Discard current map changes").clicked() {
                        self.doc = None;
                        self.open();
                    }
                    if let Some(c) = db.collection("MapInfo") {
                        egui::ComboBox::from_id_salt("tile_map_choice")
                            .selected_text("Choose existing map…")
                            .show_ui(ui, |ui| {
                                for r in &c.records {
                                    let file = c.str_or(r, "FileName", "");
                                    if ui
                                        .selectable_label(
                                            false,
                                            format!("{} ({file})", c.str_or(r, "Description", "")),
                                        )
                                        .clicked()
                                    {
                                        if self.dirty() {
                                            self.status =
                                                "Save or discard changes before opening a map."
                                                    .into();
                                        } else {
                                            self.path = std::env::var_os("ZIRCON_MAP_DIR")
                                                .map(PathBuf::from)
                                                .unwrap_or_else(|| self.assets.join("Map"))
                                                .join(format!("{file}.map"))
                                                .display()
                                                .to_string();
                                            self.open();
                                        }
                                    }
                                }
                            });
                    }
                    ui.horizontal(|ui| {
                        ui.label("New size");
                        ui.add(egui::DragValue::new(&mut self.width).range(2..=1024));
                        ui.add(egui::DragValue::new(&mut self.height).range(2..=1024));
                    });
                    if ui
                        .add_enabled(!self.dirty(), egui::Button::new("New blank map"))
                        .clicked()
                    {
                        self.doc = Some(Document::blank(self.width, self.height));
                        self.path.clear();
                        self.opened();
                    }
                    self.generation_ui(ui, db);
                    if ui
                        .add_enabled(
                            self.generated.is_none(),
                            egui::Button::new("Register saved map in database"),
                        )
                        .on_hover_text("For generated caves use Export to include placements.")
                        .clicked()
                    {
                        self.register(db);
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        for (layer, name) in [
                            (Layer::Back, "Back"),
                            (Layer::Middle, "Middle"),
                            (Layer::Front, "Front"),
                            (Layer::Collision, "Collision"),
                            (Layer::Light, "Light"),
                        ] {
                            if ui.selectable_value(&mut self.layer, layer, name).changed() {
                                self.adjacency = None;
                                self.auto = false;
                                self.start = None;
                                self.stroke.clear();
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (tool, name) in [
                            (Tool::Pen, "Pencil"),
                            (Tool::Rectangle, "Rectangle"),
                            (Tool::Pick, "Pick tile"),
                            (Tool::Copy, "Copy area"),
                            (Tool::Stamp, "Stamp"),
                            (Tool::Piece, "Piece"),
                        ] {
                            ui.selectable_value(&mut self.tool, tool, name);
                        }
                    });
                    let (activate, status) =
                        self.wall_piece.show(ui, self.stamp.as_ref(), &self.assets);
                    if activate {
                        self.tool = Tool::Piece;
                    }
                    if let Some(status) = status {
                        self.status = status;
                    }
                    if self.layer == Layer::Collision {
                        ui.checkbox(&mut self.walkable, "Allow movement");
                        ui.label("Changes the two movement bits; other flag bits are retained.");
                    } else if self.layer == Layer::Light {
                        ui.add(
                            egui::Slider::new(&mut self.light, 0..=30)
                                .step_by(2.0)
                                .text("Light"),
                        );
                    } else {
                        ui.checkbox(&mut self.erase, "Erase selected layer");
                        let title = kr_order(self.tile.file)
                            .map(|(t, n)| format!("{t} {n}"))
                            .unwrap_or_else(|| "Empty / unknown library".into());
                        egui::ComboBox::from_id_salt("tile_library")
                            .selected_text(title)
                            .show_ui(ui, |ui| {
                                for file in 0..=71u8 {
                                    if let Some((theme, name)) = kr_order(file) {
                                        if ui
                                            .selectable_value(
                                                &mut self.tile.file,
                                                file,
                                                format!("{file}: {theme} {name}"),
                                            )
                                            .changed()
                                        {
                                            self.tile.image = 0;
                                            self.page = 0;
                                            self.adjacency = None;
                                            self.auto = false;
                                        }
                                    }
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.label("Image");
                            ui.add(egui::DragValue::new(&mut self.tile.image));
                        });
                        if self.layer != Layer::Back {
                            let mut frames = self.tile.animation & 15;
                            let mut blend = self.tile.animation & 128 != 0;
                            ui.horizontal(|ui| {
                                ui.label("Frames");
                                ui.add(egui::DragValue::new(&mut frames).range(0..=15));
                                ui.checkbox(&mut blend, "Blend");
                            });
                            self.tile.animation = frames | if blend { 128 } else { 0 };
                            if self.tile.file == 0 && !self.erase {
                                ui.colored_label(
                                    Color32::YELLOW,
                                    "Library 0 is the empty marker on middle/front layers.",
                                );
                            }
                        }
                        if ui
                            .button("Learn neighbour patterns from this map")
                            .clicked()
                        {
                            if let Some(d) = &self.doc {
                                self.adjacency =
                                    Some(Adjacency::learn(&d.map, self.layer, self.tile.file));
                                self.auto = true;
                            }
                        }
                        if let Some(a) = &self.adjacency {
                            ui.checkbox(
                                &mut self.auto,
                                format!("Match neighbours ({} observed tiles)", a.len()),
                            );
                        }
                        self.palette(ui);
                    }
                    ui.separator();
                    ui.checkbox(&mut self.overlays, "Show blocked cells");
                    ui.checkbox(&mut self.grid, "Cell grid");
                    ui.horizontal(|ui| {
                        ui.label("Test position");
                        ui.add(egui::DragValue::new(&mut self.test_at.0));
                        ui.add(egui::DragValue::new(&mut self.test_at.1));
                    });
                    if ui.button("Test walk in game").clicked() {
                        self.start_test(db);
                    }
                    if self.run.is_some() && ui.button("Stop test game").clicked() {
                        self.run = None;
                    }
                    if let Some(run) = &self.run {
                        ui.label(&run.status);
                    }
                    ui.label("Test walk uses temporary map/database copies and temporary saves.");
                });
            });
        egui::CentralPanel::default().show(ui,|ui|{
            ui.horizontal(|ui|{ui.add(egui::Slider::new(&mut self.zoom,0.1..=2.0).text("Zoom"));if ui.button("Center test position").clicked(){self.scroll=Some(egui::vec2((self.test_at.0 as f32*48.0*self.zoom-350.0).max(0.0),(self.test_at.1 as f32*32.0*self.zoom-250.0).max(0.0)));}ui.label(if self.dirty(){"Map: unsaved changes"}else{"Map: saved"});});
            ui.label(&self.status);
            let Some(d)=&mut self.doc else{ui.label("Open an existing map or create a blank map to start painting.");return};
            let mut scroll=egui::ScrollArea::both().auto_shrink([false;2]);if let Some(offset)=self.scroll.take(){scroll=scroll.scroll_offset(offset);}
            scroll.show(ui,|ui|{
                let (rect,response)=ui.allocate_exact_size(egui::vec2(d.map.width as f32*48.0*self.zoom,d.map.height as f32*32.0*self.zoom),egui::Sense::click_and_drag());
                draw_map(ui,&d.map,&mut self.previews,rect,self.zoom,self.overlays,self.grid);
                if let Some(layout)=&self.generated {
                    for (i,room) in layout.rooms.iter().enumerate() {
                        let r=egui::Rect::from_min_size(rect.min+egui::vec2(room.x as f32*48.0*self.zoom,room.y as f32*32.0*self.zoom),egui::vec2(room.width as f32*48.0*self.zoom,room.height as f32*32.0*self.zoom));
                        ui.painter().rect_stroke(r,0.0,(1.0,Color32::LIGHT_BLUE),egui::StrokeKind::Inside);
                        ui.painter().text(r.left_top(),egui::Align2::LEFT_TOP,format!("Room {}",i+1),egui::FontId::proportional(12.0),Color32::LIGHT_BLUE);
                    }

                    for (points,color) in [(&layout.safe,Color32::from_rgba_unmultiplied(40,220,120,45)),(&layout.spawns,Color32::from_rgba_unmultiplied(230,160,40,20))] {
                        for &(x,y) in points {let r=egui::Rect::from_min_size(rect.min+egui::vec2(x as f32*48.0*self.zoom,y as f32*32.0*self.zoom),egui::vec2(48.0*self.zoom,32.0*self.zoom));if ui.is_rect_visible(r){ui.painter().rect_filled(r,0.0,color);} }
                    }
                    for (i,&(x,y)) in layout.exits.iter().enumerate(){let p=rect.min+egui::vec2((x as f32+0.5)*48.0*self.zoom,(y as f32+0.5)*32.0*self.zoom); ui.painter().circle_stroke(p,10.0,(2.0,Color32::LIGHT_BLUE));ui.painter().text(p,egui::Align2::CENTER_BOTTOM,if i==0{"Portal A"}else{"Portal B"},egui::FontId::proportional(13.0),Color32::LIGHT_BLUE);}
                }
                let pos=response.interact_pointer_pos().or_else(||response.hover_pos());
                let cell=pos.map(|p|(((p.x-rect.left())/(48.0*self.zoom)).floor() as i32,((p.y-rect.top())/(32.0*self.zoom)).floor() as i32));
                if let Some(cell)=cell {
                    response.clone().on_hover_text(format!("{}, {}",cell.0,cell.1));
                    if response.secondary_clicked(){self.test_at=cell;}
                    if response.drag_started_by(egui::PointerButton::Primary){let p=pos.unwrap()-response.drag_delta();self.start=Some((((p.x-rect.left())/(48.0*self.zoom)).floor() as i32,((p.y-rect.top())/(32.0*self.zoom)).floor() as i32));self.last=self.start;self.stroke.clear();}
                    if response.dragged_by(egui::PointerButton::Primary){if let Some(last)=self.last{self.stroke.extend(tile_doc::line(last,cell));}self.last=Some(cell);}
                    let a=self.start.unwrap_or(cell);let r=egui::Rect::from_min_max(rect.min+egui::vec2(a.0.min(cell.0) as f32*48.0*self.zoom,a.1.min(cell.1) as f32*32.0*self.zoom),rect.min+egui::vec2((a.0.max(cell.0)+1) as f32*48.0*self.zoom,(a.1.max(cell.1)+1) as f32*32.0*self.zoom));
                    ui.painter().rect_stroke(r,0.0,(1.5,Color32::YELLOW),egui::StrokeKind::Inside);
                    if self.tool==Tool::Piece {
                        if let Some(piece)=&self.wall_piece.piece {
                            let (x,y)=(cell.0&!1,cell.1&!1);
                            let (l,t,r,b)=piece.extent;
                            for (dx,dy) in self.wall_piece.run.offsets().unwrap_or_default() {
                                let (x,y)=(x+dx,y+dy);
                                let fits=x>=-l && y>=-t && x<=i32::from(d.map.width)-r && y<=i32::from(d.map.height)-b;
                                let bounds=egui::Rect::from_min_max(rect.min+egui::vec2((x+l) as f32*48.0*self.zoom,(y+t) as f32*32.0*self.zoom),rect.min+egui::vec2((x+r) as f32*48.0*self.zoom,(y+b) as f32*32.0*self.zoom));
                                ui.painter().rect_stroke(bounds,0.0,(2.0,if fits{Color32::LIGHT_GREEN}else{Color32::RED}),egui::StrokeKind::Inside);
                            }
                        }
                    }
                    if response.clicked() || response.drag_stopped_by(egui::PointerButton::Primary){
                        match self.tool {
                            Tool::Pick=>{let at=if self.layer==Layer::Back{(cell.0&!1,cell.1&!1)}else{cell};if let Some(c)=d.map.cell(at.0,at.1){self.tile=Tile::read(c,self.layer);self.walkable=c.is_walkable();self.light=c.light;self.page=self.tile.image as u32/32;self.erase=false;}},
                            Tool::Copy=>{self.stamp=Some(Stamp::capture(&d.map,a,cell));self.tool=Tool::Stamp;self.status="Area copied, including every layer and cell property. Click to stamp.".into();},
                            Tool::Stamp=>{if let Some(stamp)=&self.stamp{d.stamp(cell,stamp);}else{self.status="Copy an area before stamping.".into();}},
                            Tool::Piece=>{if let Some(piece)=&self.wall_piece.piece { self.status=match piece.place(d,cell,self.generated.as_ref(),self.wall_piece.run) { Ok(())=>"Piece placed. Undo restores the entire placement.".into(),Err(e)=>format!("Placement refused: {e}") }; }},
                            Tool::Pen | Tool::Rectangle=>{
                                let points=if self.tool==Tool::Rectangle{tile_doc::rectangle(&d.map,a,cell)}else{self.stroke.extend(tile_doc::line(self.last.unwrap_or(cell),cell));self.stroke.clone()};
                                let tile=if self.erase{Tile{file:if self.layer==Layer::Back{255}else{0},image:0,animation:0}}else{self.tile};
                                if !self.erase && matches!(self.layer,Layer::Back|Layer::Middle|Layer::Front) && kr_library(tile.file).and_then(|lib|self.previews.texture(ui.ctx(),lib,tile.image as u32)).is_none(){self.status="Select an image present in this library before painting.".into();}
                                else{d.paint(&points,self.layer,tile,self.walkable,self.light,if self.auto && !self.erase{self.adjacency.as_ref()}else{None});}
                            }
                        }
                        self.start=None;self.last=None;self.stroke.clear();
                    }
                }
                let p=rect.min+egui::vec2((self.test_at.0 as f32+0.5)*48.0*self.zoom,(self.test_at.1 as f32+0.5)*32.0*self.zoom);
                ui.painter().circle_stroke(p,7.0,(2.0,Color32::GREEN));
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        });
    }
    fn palette(&mut self, ui: &mut egui::Ui) {
        let Some(lib) = kr_library(self.tile.file) else {
            return;
        };
        let count = self.previews.count(lib).min(65536) as u32;
        ui.horizontal(|ui| {
            if ui.button("◀").clicked() {
                self.page = self.page.saturating_sub(1);
            }
            ui.label(format!(
                "Page {} / {}",
                self.page + 1,
                count.div_ceil(32).max(1)
            ));
            if ui.button("▶").clicked() && (self.page + 1) * 32 < count {
                self.page += 1;
            }
        });
        egui::Grid::new("tile_palette")
            .num_columns(4)
            .show(ui, |ui| {
                for i in self.page * 32..((self.page + 1) * 32).min(count) {
                    let (r, res) =
                        ui.allocate_exact_size(egui::vec2(66.0, 58.0), egui::Sense::click());
                    ui.painter().rect_filled(
                        r,
                        0.0,
                        if i == self.tile.image as u32 {
                            Color32::from_rgb(75, 80, 35)
                        } else {
                            Color32::from_gray(35)
                        },
                    );
                    if let Some((tex, [w, h])) = self.previews.texture(ui.ctx(), lib, i) {
                        let scale = (62.0 / w.max(1) as f32).min(42.0 / h.max(1) as f32);
                        let dest = egui::Rect::from_center_size(
                            r.center() - egui::vec2(0.0, 6.0),
                            egui::vec2(w as f32 * scale, h as f32 * scale),
                        );
                        let uv = egui::Rect::from_min_max(
                            egui::Pos2::ZERO,
                            egui::pos2(w as f32 / tex.size_vec2().x, h as f32 / tex.size_vec2().y),
                        );
                        ui.painter().image(tex.id(), dest, uv, Color32::WHITE);
                    }
                    ui.painter().text(
                        r.left_bottom() - egui::vec2(-2.0, 2.0),
                        egui::Align2::LEFT_BOTTOM,
                        i.to_string(),
                        egui::FontId::monospace(10.0),
                        Color32::WHITE,
                    );
                    if res.clicked() {
                        self.tile.image = i as u16;
                        self.erase = false;
                    }
                    if (i - self.page * 32) % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
    }
}

fn draw_map(
    ui: &mut egui::Ui,
    map: &MapFile,
    previews: &mut Previews,
    rect: egui::Rect,
    zoom: f32,
    blocked: bool,
    grid: bool,
) {
    let clip = ui.clip_rect().intersect(rect);
    ui.painter().rect_filled(clip, 0.0, Color32::BLACK);
    let x0 = (((clip.left() - rect.left()) / (48.0 * zoom)).floor() as i32 - 6).max(0);
    let x1 = (((clip.right() - rect.left()) / (48.0 * zoom)).ceil() as i32 + 6)
        .min(map.width as i32 - 1);
    let y0 = (((clip.top() - rect.top()) / (32.0 * zoom)).floor() as i32 - 6).max(0);
    let y1 = (((clip.bottom() - rect.top()) / (32.0 * zoom)).ceil() as i32 + 25)
        .min(map.height as i32 - 1);
    let frame = (ui.input(|i| i.time) * 10.0) as u32;
    // Same floor/row passes and cell anchoring as the game renderer.
    for pass in 0..3 {
        for y in y0..=y1 {
            let layers: &[Layer] = if pass == 0 {
                &[Layer::Back]
            } else {
                &[Layer::Middle, Layer::Front]
            };
            let draws: Vec<_> = if pass == 2 {
                layers
                    .iter()
                    .flat_map(|&layer| (x0..=x1).map(move |x| (x, layer)))
                    .collect()
            } else {
                (x0..=x1)
                    .flat_map(|x| layers.iter().map(move |&layer| (x, layer)))
                    .collect()
            };
            for (x, layer) in draws {
                let c = map.cell(x, y).unwrap();
                if layer == Layer::Back && (x % 2 != 0 || y % 2 != 0) {
                    continue;
                }
                let tile = Tile::read(c, layer);
                if layer != Layer::Back && tile.file == 0 {
                    continue;
                }
                let Some(lib) = kr_library(tile.file) else {
                    continue;
                };
                let animated = layer != Layer::Back
                    && if layer == Layer::Middle {
                        c.middle_animated()
                    } else {
                        c.front_animated()
                    };
                let frames = tile.animation & 15;
                let index = tile.image as u32
                    + if animated && frames > 0 {
                        frame % frames as u32
                    } else {
                        0
                    };
                let Some((tex, [w, h])) = previews.texture(ui.ctx(), lib, index) else {
                    continue;
                };
                let floor = (w == 48 && h == 32) || (w == 96 && h == 64);
                if pass == 1 && (!floor || animated) || pass == 2 && floor && !animated {
                    continue;
                }
                let ypx = if pass == 2 && !floor {
                    (y + 1) as f32 * 32.0 - h as f32
                } else {
                    y as f32 * 32.0
                };
                let dest = egui::Rect::from_min_size(
                    rect.min + egui::vec2(x as f32 * 48.0 * zoom, ypx * zoom),
                    egui::vec2(w as f32 * zoom, h as f32 * zoom),
                );
                if dest.intersects(clip) {
                    let uv = egui::Rect::from_min_max(
                        egui::Pos2::ZERO,
                        egui::pos2(w as f32 / tex.size_vec2().x, h as f32 / tex.size_vec2().y),
                    );
                    let tint = if animated && tile.animation & 128 != 0 {
                        Color32::from_rgb_additive(255, 255, 255)
                    } else {
                        Color32::WHITE
                    };
                    ui.painter().image(tex.id(), dest, uv, tint);
                }
            }
        }
    }
    if blocked || grid {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let r = egui::Rect::from_min_size(
                    rect.min + egui::vec2(x as f32 * 48.0 * zoom, y as f32 * 32.0 * zoom),
                    egui::vec2(48.0 * zoom, 32.0 * zoom),
                );
                if blocked && !map.is_walkable(x, y) {
                    ui.painter().rect_filled(
                        r,
                        0.0,
                        Color32::from_rgba_unmultiplied(255, 40, 40, 75),
                    );
                }
                if grid && zoom >= 0.3 {
                    ui.painter().rect_stroke(
                        r,
                        0.0,
                        (0.5, Color32::from_rgba_unmultiplied(255, 255, 255, 45)),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_room_art_survives_desktop_save_and_reopen() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1008.map")).unwrap();
        let (doc, layout, _, art, _) =
            crate::dungeon::generate_full_art(&source, 80, 80, 42, 6, 4, &assets).unwrap();
        let folder = std::env::temp_dir().join(format!(
            "zircon-editor-art-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("rooms.map");
        let mut tiles = Tiles::new(&assets);
        tiles.doc = Some(doc);
        tiles.generated = Some(layout);
        tiles.private_art = Some(art.clone());
        tiles.path = path.to_string_lossy().into_owned();
        tiles.save();
        assert!(tiles.status.starts_with("Map saved"), "{}", tiles.status);
        assert_eq!(std::fs::read(path.with_extension("wood.zl")).unwrap(), art);
        let mut reopened = Tiles::new(&assets);
        reopened.path = tiles.path.clone();
        reopened.open();
        assert!(reopened.generated.is_some(), "{}", reopened.status);
        assert_eq!(reopened.private_art.as_ref(), Some(&art));
        let original = mir_formats::zl::ZlLibrary::open(assets.join(crate::wood_branch::LIBRARY))
            .unwrap()
            .len();
        let generated = original + crate::wood_branch::NEW_IMAGES + 2;
        assert_eq!(reopened.previews.count(306), generated);
        assert!(highest_wood_image(&reopened.doc.as_ref().unwrap().map).unwrap() < generated);
        let db = MirDb::load(assets.parent().unwrap().join("Database/System.db")).unwrap();
        reopened.monster = 8;
        let bundle = folder.join("bundle");
        reopened.export_generated(&db, &bundle).unwrap();
        assert_eq!(
            std::fs::read(bundle.join("Client").join(crate::wood_branch::LIBRARY)).unwrap(),
            art
        );
        assert!(bundle.join("README.txt").is_file());
        drop(reopened);
        drop(tiles);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn failed_portal_sidecar_save_stays_dirty_until_retry() {
        let folder = std::env::temp_dir().join(format!(
            "zircon-portal-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("draft.map");
        // A directory at the sidecar path reliably forces a write failure.
        std::fs::create_dir(path.with_extension("navigation.json")).unwrap();
        let mut tiles = Tiles::new(&folder);
        tiles.path = path.to_string_lossy().into_owned();
        tiles.doc = Some(Document::blank(32, 32));
        tiles.generated =
            Some(crate::generate::layout(&tiles.doc.as_ref().unwrap().map, 42).unwrap());
        tiles.save();
        assert!(path.is_file());
        assert!(tiles.status.contains("portal markers could not be saved"));
        assert!(tiles.dirty());
        tiles.open();
        assert!(tiles.status.contains("Save or discard"));
        std::fs::remove_dir(path.with_extension("navigation.json")).unwrap();
        tiles.save();
        assert!(!tiles.dirty());
        assert!(
            mir_formats::navigation::Navigation::load(&path, &tiles.doc.as_ref().unwrap().map)
                .unwrap()
                .is_some()
        );
        std::fs::remove_dir_all(folder).unwrap();
    }
}
