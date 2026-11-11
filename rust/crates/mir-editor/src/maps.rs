//! Map-region placement edits System.db; map tiles are read-only here.
use eframe::egui::{self, Color32};
use mir_formats::{
    map::MapFile,
    mirdb::{region_points, MirDb, Value},
};
use std::{collections::BTreeSet, path::Path};

pub struct Edit {
    pub collection: &'static str,
    pub index: Option<i32>,
    pub values: Vec<(&'static str, Value)>,
}

struct Region {
    id: i32,
    name: String,
    points: Vec<(i32, i32)>,
    uses: Vec<(String, i32, String)>,
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Tool {
    #[default]
    Inspect,
    Add,
    Erase,
}

pub struct Maps {
    map: i32,
    region: Option<i32>,
    loaded: Option<(String, MapFile)>,
    regions: Vec<Region>,
    texture: Option<egui::TextureHandle>,
    dirty: bool,
    zoom: f32,
    fit: bool,
    tool: Tool,
    start: Option<(i32, i32)>,
    name: String,
    monster: i32,
    count: i32,
    delay: i32,
    filter: String,
    show_spawns: bool,
    show_safe: bool,
    show_other: bool,
    pub message: String,
    pub jump: Option<(&'static str, i32)>,
}
impl Default for Maps {
    fn default() -> Self {
        Self {
            map: std::env::var("ZIRCON_EDITOR_MAP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            region: std::env::var("ZIRCON_EDITOR_REGION")
                .ok()
                .and_then(|s| s.parse().ok()),
            loaded: None,
            regions: vec![],
            texture: None,
            dirty: true,
            zoom: 1.0,
            fit: true,
            tool: Tool::Inspect,
            start: None,
            name: "New region".into(),
            monster: 0,
            count: 1,
            delay: 60,
            filter: String::new(),
            show_spawns: true,
            show_safe: true,
            show_other: true,
            message: String::new(),
            jump: None,
        }
    }
}

fn choices(db: &MirDb, table: &str, prop: &str) -> Vec<(i32, String)> {
    let Some(c) = db.collection(table) else {
        return vec![];
    };
    let mut rows: Vec<_> = c
        .records
        .iter()
        .map(|r| (c.index(r), c.str_or(r, prop, "").to_owned()))
        .collect();
    rows.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    rows
}

fn read_regions(db: &MirDb, map: i32, width: i32) -> Vec<Region> {
    let Some(c) = db.collection("MapRegion") else {
        return vec![];
    };
    let mut result = Vec::new();
    for r in c
        .records
        .iter()
        .filter(|r| c.int_or(r, "Map", 0) == map as i64)
    {
        let bits = match c.get(r, "BitRegion") {
            Some(Value::BitArray(Some(b))) => Some(b.as_slice()),
            _ => None,
        };
        let points = match c.get(r, "PointRegion") {
            Some(Value::PointArray(Some(p))) => Some(p.as_slice()),
            _ => None,
        };
        let id = c.index(r);
        let mut uses = vec![];
        for (table, prop) in [
            ("RespawnInfo", "Region"),
            ("SafeZoneInfo", "Region"),
            ("NPCInfo", "Region"),
            ("MovementInfo", "SourceRegion"),
            ("MovementInfo", "DestinationRegion"),
        ] {
            if let Some(t) = db.collection(table) {
                for row in t
                    .records
                    .iter()
                    .filter(|row| t.int_or(row, prop, 0) == id as i64)
                {
                    let label = if table == "RespawnInfo" {
                        let monster = t.int_or(row, "Monster", 0);
                        let name = db
                            .collection("MonsterInfo")
                            .and_then(|m| {
                                m.records
                                    .iter()
                                    .find(|r| m.index(r) as i64 == monster)
                                    .map(|r| m.str_or(r, "MonsterName", "?").to_owned())
                            })
                            .unwrap_or_default();
                        format!(
                            "{} × {} ({} s)",
                            name,
                            t.int_or(row, "Count", 0),
                            t.int_or(row, "Delay", 0)
                        )
                    } else if table == "NPCInfo" {
                        t.str_or(row, "NPCName", "NPC").to_owned()
                    } else {
                        prop.into()
                    };
                    uses.push((table.into(), t.index(row), label));
                }
            }
        }
        result.push(Region {
            id,
            name: c.str_or(r, "Description", "").into(),
            points: region_points(bits, points, width),
            uses,
        });
    }
    result.sort_by_key(|r| r.id);
    result
}

/// One gesture is one edit, preserving cells outside the rectangle. Existing
/// bit regions are decoded with the same row order as the server before editing.
pub fn paint(
    db: &MirDb,
    map: &MapFile,
    map_id: i32,
    region: i32,
    from: (i32, i32),
    to: (i32, i32),
    erase: bool,
) -> anyhow::Result<Edit> {
    let regions = read_regions(db, map_id, map.width as i32);
    let r = regions
        .iter()
        .find(|r| r.id == region)
        .ok_or_else(|| anyhow::anyhow!("Select a region on this map."))?;
    let mut points: BTreeSet<_> = r.points.iter().copied().collect();
    let before = points.clone();
    for x in from.0.min(to.0).max(0)..=from.0.max(to.0).min(map.width as i32 - 1) {
        for y in from.1.min(to.1).max(0)..=from.1.max(to.1).min(map.height as i32 - 1) {
            if erase {
                points.remove(&(x, y));
            } else if map.is_walkable(x, y) {
                points.insert((x, y));
            }
        }
    }
    anyhow::ensure!(
        points != before,
        "No cells changed (adding skips blocked cells)."
    );
    Ok(Edit {
        collection: "MapRegion",
        index: Some(region),
        values: vec![
            ("BitRegion", Value::BitArray(None)),
            (
                "PointRegion",
                Value::PointArray(Some(points.into_iter().collect())),
            ),
        ],
    })
}

impl Maps {
    pub fn invalidate(&mut self) {
        self.dirty = true;
        self.texture = None;
        self.start = None;
    }
    pub fn reload(&mut self) {
        self.loaded = None;
        self.invalidate();
    }

    fn refresh(&mut self, db: &MirDb, dir: &Path) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(c) = db.collection("MapInfo") else {
            return;
        };
        if !c.records.iter().any(|r| c.index(r) == self.map) {
            self.map = c.records.first().map(|r| c.index(r)).unwrap_or(0);
        }
        let Some(r) = c.records.iter().find(|r| c.index(r) == self.map) else {
            return;
        };
        let file = c.str_or(r, "FileName", "").to_owned();
        if self.loaded.as_ref().is_none_or(|(name, _)| *name != file) {
            self.loaded = None;
            match MapFile::load(dir.join(format!("{file}.map"))) {
                Ok(map) => {
                    self.loaded = Some((file, map));
                    self.fit = true;
                    self.message.clear();
                }
                Err(e) => self.message = format!("Cannot open map: {e}"),
            }
        }
        if let Some((_, map)) = &self.loaded {
            self.regions = read_regions(db, self.map, map.width as i32);
            if !self.regions.iter().any(|r| Some(r.id) == self.region) {
                self.region = None;
            }
        } else {
            self.regions.clear();
        }
    }

    fn make_texture(&mut self, ctx: &egui::Context) {
        if self.texture.is_some() {
            return;
        }
        let Some((_, map)) = &self.loaded else { return };
        let (w, h) = (map.width as usize, map.height as usize);
        let mut image = egui::ColorImage::filled([w, h], Color32::from_rgb(24, 28, 32));
        for y in 0..h {
            for x in 0..w {
                if map.is_walkable(x as i32, y as i32) {
                    image.pixels[y * w + x] = Color32::from_rgb(83, 93, 99);
                }
            }
        }
        for selected in [false, true] {
            for r in &self.regions {
                if (Some(r.id) == self.region) != selected {
                    continue;
                }
                let spawn = r.uses.iter().any(|u| u.0 == "RespawnInfo");
                let safe = r.uses.iter().any(|u| u.0 == "SafeZoneInfo");
                let colour = if selected {
                    Color32::from_rgb(255, 216, 85)
                } else if safe && self.show_safe {
                    Color32::from_rgb(67, 190, 126)
                } else if spawn && self.show_spawns {
                    Color32::from_rgb(216, 96, 100)
                } else if !spawn && !safe && self.show_other {
                    Color32::from_rgb(84, 144, 222)
                } else {
                    continue;
                };
                for &(x, y) in &r.points {
                    if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                        image.pixels[y as usize * w + x as usize] = colour;
                    }
                }
            }
        }
        self.texture = Some(ctx.load_texture("map_regions", image, egui::TextureOptions::NEAREST));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, db: &MirDb, dir: &Path) -> Option<Edit> {
        self.refresh(db, dir);
        let mut edit = None;
        egui::Panel::left("map_controls")
            .default_size(320.0)
            .show(ui, |ui| {
                ui.heading("Maps & regions");
                ui.label("Map name / file / index");
                ui.text_edit_singleline(&mut self.filter);
                let maps = choices(db, "MapInfo", "Description");
                let title = maps
                    .iter()
                    .find(|m| m.0 == self.map)
                    .map(|m| format!("{} · {}", m.0, m.1))
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("map_choice")
                    .selected_text(title)
                    .width(295.0)
                    .show_ui(ui, |ui| {
                        let needle = self.filter.to_lowercase();
                        for (id, name) in &maps {
                            let filename = db
                                .collection("MapInfo")
                                .and_then(|c| {
                                    c.records
                                        .iter()
                                        .find(|r| c.index(r) == *id)
                                        .map(|r| c.str_or(r, "FileName", "").to_owned())
                                })
                                .unwrap_or_default();
                            if format!("{id} {name} {filename}")
                                .to_lowercase()
                                .contains(&needle)
                                && ui
                                    .selectable_value(
                                        &mut self.map,
                                        *id,
                                        format!("{id} {name} ({filename})"),
                                    )
                                    .changed()
                            {
                                self.region = None;
                                self.tool = Tool::Inspect;
                                self.invalidate();
                            }
                        }
                    });
                ui.separator();
                ui.label("Regions (selected = yellow)");
                egui::ScrollArea::vertical()
                    .id_salt("regions")
                    .max_height(190.0)
                    .show(ui, |ui| {
                        for r in &self.regions {
                            if ui
                                .selectable_value(
                                    &mut self.region,
                                    Some(r.id),
                                    format!("{} {} · {} cells", r.id, r.name, r.points.len()),
                                )
                                .changed()
                            {
                                self.texture = None;
                                self.start = None;
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.name);
                });
                if ui.button("New region").clicked()
                    && self.loaded.is_some()
                    && !self.name.trim().is_empty()
                {
                    edit = Some(Edit {
                        collection: "MapRegion",
                        index: None,
                        values: vec![
                            ("Map", Value::Int(self.map as i64)),
                            ("Description", Value::Str(self.name.trim().into())),
                            ("PointRegion", Value::PointArray(Some(vec![]))),
                        ],
                    });
                }
                let selected = self.regions.iter().find(|r| Some(r.id) == self.region);
                if let Some(r) = selected {
                    ui.strong(format!("Selected: #{} {}", r.id, r.name));
                    if ui.button("Edit region fields in Tables").clicked() {
                        self.jump = Some(("MapRegion", r.id));
                    }
                    ui.separator();
                    ui.label("Uses of this region (shape changes affect all):");
                    egui::ScrollArea::vertical()
                        .id_salt("region_uses")
                        .max_height(105.0)
                        .show(ui, |ui| {
                            for (table, id, label) in &r.uses {
                                if ui.link(format!("{table} #{id}: {label}")).clicked() {
                                    let table = match table.as_str() {
                                        "RespawnInfo" => "RespawnInfo",
                                        "SafeZoneInfo" => "SafeZoneInfo",
                                        "NPCInfo" => "NPCInfo",
                                        _ => "MovementInfo",
                                    };
                                    self.jump = Some((table, *id));
                                }
                            }
                        });
                    let eligible = self
                        .loaded
                        .as_ref()
                        .is_some_and(|(_, m)| r.points.iter().any(|&(x, y)| m.is_walkable(x, y)));
                    ui.separator();
                    let monsters = choices(db, "MonsterInfo", "MonsterName");
                    egui::ComboBox::from_id_salt("spawn_monster")
                        .selected_text(
                            monsters
                                .iter()
                                .find(|m| m.0 == self.monster)
                                .map(|m| m.1.as_str())
                                .unwrap_or("Choose monster"),
                        )
                        .show_ui(ui, |ui| {
                            for (id, name) in monsters {
                                ui.selectable_value(&mut self.monster, id, name);
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("Count");
                        ui.add(egui::DragValue::new(&mut self.count).range(1..=10000));
                        ui.label("Delay (s)");
                        ui.add(egui::DragValue::new(&mut self.delay).range(1..=86400));
                    });
                    if ui
                        .add_enabled(
                            eligible && self.monster != 0,
                            egui::Button::new("Add monster spawn"),
                        )
                        .clicked()
                    {
                        edit = Some(Edit {
                            collection: "RespawnInfo",
                            index: None,
                            values: vec![
                                ("Region", Value::Int(r.id as i64)),
                                ("Monster", Value::Int(self.monster as i64)),
                                ("Count", Value::Int(self.count as i64)),
                                ("Delay", Value::Int(self.delay as i64)),
                            ],
                        });
                    }
                    if ui
                        .add_enabled(
                            eligible && !r.uses.iter().any(|u| u.0 == "SafeZoneInfo"),
                            egui::Button::new("Make safe zone"),
                        )
                        .clicked()
                    {
                        edit = Some(Edit {
                            collection: "SafeZoneInfo",
                            index: None,
                            values: vec![
                                ("Region", Value::Int(r.id as i64)),
                                ("BindRegion", Value::Int(r.id as i64)),
                            ],
                        });
                    }
                    if !eligible {
                        ui.label("Add walkable cells before placing a spawn or safe zone.");
                    }
                }
            });
        // Map choice above may have changed; refresh before interacting.
        self.refresh(db, dir);
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tool, Tool::Inspect, "Inspect");
                ui.add_enabled_ui(self.region.is_some(), |ui| {
                    ui.selectable_value(&mut self.tool, Tool::Add, "Add rectangle");
                    ui.selectable_value(&mut self.tool, Tool::Erase, "Erase rectangle");
                });
                ui.add(egui::Slider::new(&mut self.zoom, 0.05..=12.0).text("Zoom"));
                if ui.button("Fit").clicked() { self.fit = true; }
            });
            ui.horizontal(|ui| {
                let changed = ui.checkbox(&mut self.show_spawns, "Spawns · red").changed()
                    | ui.checkbox(&mut self.show_safe, "Safe zones · green").changed()
                    | ui.checkbox(&mut self.show_other, "Other regions · blue").changed();
                if changed { self.texture = None; }
                ui.label("Walkable · grey / blocked · dark");
            });
            ui.label("Click to inspect; drag to add or erase cells. Add skips blocked cells. Scroll to pan.");
            if !self.message.is_empty() { ui.label(&self.message); }
            self.make_texture(ui.ctx());
            let Some((name, map)) = &self.loaded else { return };
            ui.label(format!("{name} · {} × {} cells", map.width, map.height));
            if self.fit {
                self.zoom = ((ui.available_width() - 12.0) / map.width as f32)
                    .min((ui.available_height() - 12.0) / map.height as f32).clamp(0.05, 12.0);
                self.fit = false;
            }
            let Some(texture) = self.texture.as_ref().map(|t| t.id()) else { return };
            egui::ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
                let size = egui::vec2(map.width as f32 * self.zoom, map.height as f32 * self.zoom);
                let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().image(texture, rect, uv, Color32::WHITE);
                // Single-cell NPC and portal regions stay visible when zoomed out.
                if let Some(r) = self.regions.iter().find(|r| Some(r.id) == self.region) {
                    if r.points.len() <= 8 {
                        for &(x, y) in &r.points {
                            let center = rect.min + egui::vec2((x as f32 + 0.5) * self.zoom, (y as f32 + 0.5) * self.zoom);
                            ui.painter().circle_stroke(center, 5.0, (1.5, Color32::YELLOW));
                        }
                    }
                }
                let to_cell = |p: egui::Pos2| {
                    (((p.x - rect.min.x) / self.zoom).floor() as i32,
                     ((p.y - rect.min.y) / self.zoom).floor() as i32)
                };
                let cell = response.interact_pointer_pos().or_else(|| response.hover_pos()).map(to_cell);
                if response.drag_started() {
                    self.start = response.interact_pointer_pos().map(|p| to_cell(p - response.drag_delta()));
                }
                if let Some((x, y)) = cell {
                    let state = if map.is_walkable(x, y) { "walkable" } else { "blocked" };
                    response.clone().on_hover_text(format!("({x}, {y}) · {state}"));
                    if let Some(start) = self.start {
                        let a = rect.min + egui::vec2(start.0.min(x) as f32 * self.zoom, start.1.min(y) as f32 * self.zoom);
                        let b = rect.min + egui::vec2((start.0.max(x) + 1) as f32 * self.zoom, (start.1.max(y) + 1) as f32 * self.zoom);
                        ui.painter().rect_stroke(egui::Rect::from_min_max(a, b), 0.0, (1.0, Color32::WHITE), egui::StrokeKind::Inside);
                    }
                    if response.clicked() || response.drag_stopped() {
                        if self.tool == Tool::Inspect {
                            // Prefer small overlapping regions such as an NPC point.
                            self.region = self.regions.iter().filter(|r| r.points.contains(&(x, y)))
                                .min_by_key(|r| r.points.len()).map(|r| r.id);
                            self.texture = None;
                        } else if let Some(region) = self.region {
                            match paint(db, map, self.map, region, self.start.unwrap_or((x, y)), (x, y), self.tool == Tool::Erase) {
                                Ok(e) => edit = Some(e),
                                Err(e) => self.message = e.to_string(),
                            }
                        }
                        self.start = None;
                    }
                }
            });
        });
        edit
    }

    pub fn edited(&mut self, collection: &str, index: i32) {
        if collection == "MapRegion" {
            self.region = Some(index);
        }
        self.message =
            "Changed in memory. Save to write System.db; Undo restores the previous edit.".into();
        self.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_edit_decodes_bit_regions_clips_bounds_and_preserves_other_cells() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let mut db =
            MirDb::load(std::path::PathBuf::from(assets).join("../Database/System.db")).unwrap();
        let c = db.collection_mut("MapRegion").unwrap();
        let id = c.push_record(vec![
            ("Map", Value::Int(123456)),
            ("BitRegion", Value::BitArray(Some(vec![0b0010_0001, 0]))),
        ]);
        let mut map = MapFile::blank(4, 4);
        for x in 0..4 {
            for y in 0..4 {
                map.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        map.cell_mut(2, 2).unwrap().flag = 0;
        let edit = paint(&db, &map, 123456, id, (1, 1), (8, 8), false).unwrap();
        let Some((_, Value::PointArray(Some(points)))) =
            edit.values.iter().find(|(n, _)| *n == "PointRegion")
        else {
            panic!("points")
        };
        assert!(points.contains(&(0, 0))); // retained bit outside the rectangle
        assert!(points.contains(&(1, 1))); // row-major bit 5
        assert!(points.contains(&(3, 3))); // clipped edge
        assert!(!points.contains(&(2, 2))); // blocked
        assert_eq!(points.len(), 9);
        let erased = paint(&db, &map, 123456, id, (0, 0), (0, 0), true).unwrap();
        assert!(matches!(&erased.values[1].1,Value::PointArray(Some(p)) if p==&vec![(1,1)]));
        assert!(paint(&db, &map, 999, id, (1, 1), (2, 2), false).is_err());
        assert!(paint(&db, &map, 123456, id, (2, 2), (2, 2), false).is_err());
    }
}
