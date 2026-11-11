//! The editor window: collection list, filterable table, detail form with
//! sprite preview and related records, save with backup, undo.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use mir_formats::mirdb::{MirDb, Record, Value};

use crate::preview::{self, Previews};
use crate::value;

/// Snapshot of one collection's rows for undo.
struct CollectionSnapshot {
    collection: usize,
    records: Vec<Record>,
    next_index: i32,
}

/// One user action can update several linked collections.
struct Undo {
    selected: usize,
    collections: Vec<CollectionSnapshot>,
}

pub struct Editor {
    assets: PathBuf,
    sprite_variant: crate::sprite_variant::VariantEditor,
    sprite_project: crate::sprite_ui::Panel,
    castle_gate: crate::castle_gate_ui::Panel,
    item_draft_panel: crate::item_draft_ui::Panel,
    tiles: crate::tiles::Tiles,
    tile_mode: bool,
    maps: crate::maps::Maps,
    map_mode: bool,
    db_path: PathBuf,
    db: MirDb,
    previews: Previews,
    dirty: bool,
    status: String,
    /// Selected collection index.
    selected: usize,
    filter: String,
    /// (property index, ascending).
    sort: Option<(usize, bool)>,
    /// Selected record index within the collection.
    row: Option<usize>,
    /// Pending jump to (collection, record) requested from a link.
    jump: Option<(usize, usize)>,
    undo: Vec<Undo>,
    /// Text being edited in the form for complex values: property -> text.
    form_text: HashMap<usize, String>,
    form_error: Option<String>,
    monster_direction: u8,
    draft_source: Option<i32>,
    draft_level: i32,
    draft_name: String,
    monster_draft: Option<crate::monster_draft::Draft>,
    draft_error: Option<String>,
    drop_source: Option<i32>,
    drop_donor: i32,
    material_donor: i32,
    drop_templates: Vec<crate::drop_draft::Template>,
    drop_draft: Option<crate::drop_draft::Draft>,
    drop_error: Option<String>,
    drop_selection: Vec<bool>,
    loot_recipe: Option<crate::drop_draft::Recipe>,
    /// Client `Map/` folder for the walkability checks.
    map_dir: PathBuf,
    /// Last validation run: load error or the list of problems.
    validation: Option<Validation>,
}

enum Validation {
    Failed(String),
    Problems(Vec<mir_server::data::Problem>),
}

const NAME_PROPS: [&str; 7] = [
    "ItemName",
    "MonsterName",
    "NPCName",
    "Name",
    "FileName",
    "Description",
    "RegionName",
];

impl Editor {
    pub fn open(db_path: PathBuf, assets: PathBuf) -> anyhow::Result<Editor> {
        let db = MirDb::load(&db_path)
            .map_err(|e| anyhow::anyhow!("loading {}: {e}", db_path.display()))?;
        let mut ed = Editor {
            assets: assets.clone(),
            sprite_variant: Default::default(),
            sprite_project: Default::default(),
            castle_gate: Default::default(),
            item_draft_panel: Default::default(),
            tiles: crate::tiles::Tiles::new(&assets),
            tile_mode: std::env::var_os("ZIRCON_EDITOR_TILE_MAP").is_some(),
            maps: crate::maps::Maps::default(),
            map_mode: std::env::var_os("ZIRCON_EDITOR_MAP").is_some(),
            status: format!(
                "{} collections loaded from {}",
                db.collections.len(),
                db_path.display()
            ),
            db_path,
            db,
            previews: Previews::new(&assets),
            dirty: false,
            selected: 0,
            filter: String::new(),
            sort: None,
            row: None,
            jump: None,
            undo: Vec::new(),
            form_text: HashMap::new(),
            form_error: None,
            monster_direction: 4,
            draft_source: None,
            draft_level: 1,
            draft_name: String::new(),
            monster_draft: None,
            draft_error: None,
            drop_source: None,
            drop_donor: 0,
            material_donor: 0,
            drop_templates: Vec::new(),
            drop_draft: None,
            drop_error: None,
            drop_selection: Vec::new(),
            loot_recipe: None,
            map_dir: std::env::var_os("ZIRCON_MAP_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| assets.join("Map")),
            validation: None,
        };
        if let Ok(name) = std::env::var("ZIRCON_EDITOR_COLLECTION") {
            if let Some(i) = ed
                .db
                .collections
                .iter()
                .position(|c| c.short_name() == name)
            {
                ed.selected = i;
            }
        }
        if let Ok(f) = std::env::var("ZIRCON_EDITOR_FILTER") {
            ed.filter = f;
        }
        if let Ok(r) = std::env::var("ZIRCON_EDITOR_ROW") {
            let rows = ed.visible_rows();
            ed.row = r.parse::<usize>().ok().and_then(|i| rows.get(i).copied());
        }
        if std::env::var_os("ZIRCON_EDITOR_VALIDATE").is_some() {
            // Visual check: break one spawn so the panel has something to show.
            if std::env::var("ZIRCON_EDITOR_VALIDATE").as_deref() == Ok("break") {
                if let Some(c) = ed.db.collection_mut("RespawnInfo") {
                    if let Some(first) = c.records.first().cloned() {
                        let mut r = first;
                        c.set(&mut r, "Monster", Value::Int(999_999));
                        c.records[0] = r;
                        ed.dirty = true;
                    }
                }
            }
            ed.validate();
        }
        Ok(ed)
    }

    // ---- data helpers -------------------------------------------------------

    /// The collection an integer property points at, by naming convention
    /// (`Item` -> `ItemInfo`, `Region` -> `MapRegion`, `Page` -> `NPCPage`).
    fn fk_target(&self, prop: &str) -> Option<usize> {
        if prop == "Index" {
            return None;
        }
        let candidates = [
            format!("{prop}Info"),
            format!("Map{prop}"),
            format!("NPC{prop}"),
            prop.to_string(),
        ];
        self.db
            .collections
            .iter()
            .position(|c| candidates.iter().any(|n| c.short_name() == *n))
    }

    fn record_name(&self, collection: usize, record: &Record) -> String {
        let c = &self.db.collections[collection];
        for n in NAME_PROPS {
            if let Some(v) = c.get(record, n).and_then(Value::as_str) {
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
        format!("#{}", c.index(record))
    }

    fn position_of_index(&self, collection: usize, index: i32) -> Option<usize> {
        let c = &self.db.collections[collection];
        c.records.iter().position(|r| c.index(r) == index)
    }

    /// Record indices matching the filter, in sort order.
    fn visible_rows(&self) -> Vec<usize> {
        let c = &self.db.collections[self.selected];
        let needle = self.filter.trim().to_lowercase();
        let index_needle: Option<i32> = needle.parse().ok();
        let mut rows: Vec<usize> = c
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                if needle.is_empty() {
                    return true;
                }
                if index_needle == Some(c.index(r)) {
                    return true;
                }
                r.values.iter().any(|v| match v {
                    Value::Str(s) => s.to_lowercase().contains(&needle),
                    _ => false,
                })
            })
            .map(|(i, _)| i)
            .collect();
        if let Some((prop, asc)) = self.sort {
            rows.sort_by(|a, b| {
                let va = &c.records[*a].values[prop];
                let vb = &c.records[*b].values[prop];
                let ord = match (va.as_f64(), vb.as_f64()) {
                    (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                    _ => value::summary(va).cmp(&value::summary(vb)),
                };
                if asc {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }
        rows
    }

    fn snapshot(&mut self) {
        self.drop_source = None;
        self.drop_draft = None;
        self.maps.invalidate();
        self.validation = None;
        let c = &self.db.collections[self.selected];
        self.undo.push(Undo {
            selected: self.selected,
            collections: vec![CollectionSnapshot {
                collection: self.selected,
                records: c.records.clone(),
                next_index: c.next_index,
            }],
        });
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
        self.dirty = true;
    }

    fn undo(&mut self) {
        if self.tile_mode {
            self.tiles.undo();
            return;
        }
        if let Some(u) = self.undo.pop() {
            self.drop_source = None;
            self.drop_draft = None;
            for snapshot in u.collections {
                let c = &mut self.db.collections[snapshot.collection];
                c.records = snapshot.records;
                c.next_index = snapshot.next_index;
            }
            self.selected = u.selected;
            self.row = self
                .row
                .filter(|r| *r < self.db.collections[self.selected].records.len());
            self.monster_draft = None;
            self.form_text.clear();
            self.status = "Undone".into();
            self.dirty = true;
            self.validation = None;
            self.maps.invalidate();
        }
    }

    fn apply_map_edit(&mut self, edit: crate::maps::Edit) -> anyhow::Result<i32> {
        let collection = self
            .db
            .collections
            .iter()
            .position(|c| c.short_name() == edit.collection)
            .ok_or_else(|| anyhow::anyhow!("Collection {} is unavailable", edit.collection))?;
        let c = &self.db.collections[collection];
        for (name, _) in &edit.values {
            anyhow::ensure!(c.property_index(name).is_some(), "Missing property {name}");
        }
        let row = if let Some(index) = edit.index {
            Some(
                c.records
                    .iter()
                    .position(|r| c.index(r) == index)
                    .ok_or_else(|| anyhow::anyhow!("Record no longer exists"))?,
            )
        } else {
            None
        };
        self.selected = collection;
        self.snapshot();
        let c = &mut self.db.collections[collection];
        let index = if let Some(row) = row {
            let mut record = c.records[row].clone();
            for (name, value) in edit.values {
                c.set(&mut record, name, value);
            }
            let index = c.index(&record);
            c.records[row] = record;
            index
        } else {
            c.push_record(edit.values)
        };
        self.row = c.records.iter().position(|r| c.index(r) == index);
        self.form_text.clear();
        self.maps.edited(edit.collection, index);
        self.status = format!("Updated {} #{index}", edit.collection);
        Ok(index)
    }

    fn save(&mut self) {
        if self.tile_mode {
            self.tiles.save();
            self.maps.reload();
            return;
        }
        match self.db.save(&self.db_path) {
            Ok(Some(bak)) => {
                self.dirty = false;
                self.status = format!("Saved; previous file kept as {}", bak.display());
            }
            Ok(None) => {
                self.dirty = false;
                self.status = "Saved".into();
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    /// Run the server's loader and consistency checks on the in-memory data.
    fn validate(&mut self) {
        self.validation = Some(match mir_server::data::GameData::from_db(&self.db) {
            Ok(data) => Validation::Problems(data.validate(&self.map_dir)),
            Err(e) => Validation::Failed(format!("{e:#}")),
        });
        self.status = match &self.validation {
            Some(Validation::Problems(p)) if p.is_empty() => "Validation passed".into(),
            Some(Validation::Problems(p)) => format!("{} problems found", p.len()),
            _ => "Validation failed: the server cannot load this data".into(),
        };
    }

    fn reload(&mut self) {
        if self
            .loot_recipe
            .as_ref()
            .is_some_and(|r| !r.entries().is_empty())
        {
            self.status =
                "Apply or discard the queued loot in MonsterInfo before reloading.".into();
            return;
        }
        if self.tile_mode {
            self.tiles.status =
                "Use Open map to reload, or Discard current map changes if needed.".into();
            return;
        }
        match MirDb::load(&self.db_path) {
            Ok(db) => {
                self.db = db;
                self.dirty = false;
                self.undo.clear();
                self.monster_draft = None;
                self.draft_source = None;
                self.drop_source = None;
                self.drop_draft = None;
                self.form_text.clear();
                self.row = None;
                self.status = "Reloaded from disk".into();
                self.validation = None;
                self.maps.reload();
            }
            Err(e) => self.status = format!("Reload failed: {e}"),
        }
    }

    // ---- ui -----------------------------------------------------------------

    pub fn unsaved(&self) -> bool {
        self.dirty
            || self.tiles.dirty()
            || self
                .loot_recipe
                .as_ref()
                .is_some_and(|r| !r.entries().is_empty())
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Some((c, r)) = self.jump.take() {
            self.map_mode = false;
            self.tile_mode = false;
            self.selected = c;
            self.row = Some(r);
            self.filter.clear();
            self.form_text.clear();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.undo();
        }
        self.top_bar(ui);
        if self.tile_mode {
            self.tiles.ui(ui, &self.db);
            if let Some(edit) = self.tiles.registration.take() {
                if let Err(e) = self.apply_map_edit(edit) {
                    self.tiles.status = e.to_string();
                }
            }
            return;
        }
        if self.map_mode {
            self.validation_panel(ui);
            if let Some(edit) = self.maps.show(ui, &self.db, &self.map_dir) {
                if let Err(e) = self.apply_map_edit(edit) {
                    self.status = e.to_string();
                }
            }
            if let Some((table, index)) = self.maps.jump.take() {
                if let Some(c) = self
                    .db
                    .collections
                    .iter()
                    .position(|c| c.short_name() == table)
                {
                    if let Some(r) = self.position_of_index(c, index) {
                        self.jump = Some((c, r));
                    }
                }
            }
            return;
        }
        self.collections_panel(ui);
        self.detail_panel(ui);
        self.validation_panel(ui);
        egui::CentralPanel::default().show(ui, |ui| self.table(ui));
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                let save = ui.add_enabled(
                    if self.tile_mode {
                        self.tiles.dirty()
                    } else {
                        self.dirty
                    },
                    egui::Button::new(if self.tile_mode {
                        "Save map (Cmd+S)"
                    } else {
                        "Save DB (Cmd+S)"
                    }),
                );
                if save.clicked() {
                    self.save();
                }
                if ui.button("Reload").clicked() {
                    self.reload();
                }
                if ui
                    .add_enabled(
                        if self.tile_mode {
                            self.tiles.can_undo()
                        } else {
                            !self.undo.is_empty()
                        },
                        egui::Button::new("Undo (Cmd+Z)"),
                    )
                    .clicked()
                {
                    self.undo();
                }
                ui.separator();
                if ui
                    .selectable_label(!self.map_mode && !self.tile_mode, "Tables")
                    .clicked()
                {
                    self.map_mode = false;
                    self.tile_mode = false;
                }
                if ui
                    .selectable_label(self.map_mode && !self.tile_mode, "Regions")
                    .clicked()
                {
                    self.map_mode = true;
                    self.tile_mode = false;
                    self.maps.reload();
                }
                if ui.selectable_label(self.tile_mode, "Tiles").clicked() {
                    self.tile_mode = true;
                }
                if self.tile_mode {
                    ui.label(if self.dirty {
                        "Database also has unsaved changes"
                    } else {
                        "Map file workspace"
                    });
                    return;
                }
                if self.map_mode {
                    if ui.button("Validate").clicked() {
                        self.validate();
                    }
                    ui.label(format!(
                        "{}{}",
                        self.status,
                        if self.dirty { " (unsaved changes)" } else { "" }
                    ));
                    return;
                }
                ui.label("Filter:");
                ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(220.0));
                if ui.button("Clear").clicked() {
                    self.filter.clear();
                }
                ui.separator();
                if ui.button("Add").clicked() {
                    self.snapshot();
                    let c = &mut self.db.collections[self.selected];
                    c.push_record(Vec::new());
                    self.row = Some(c.records.len() - 1);
                    self.filter.clear();
                }
                let has_row = self.row.is_some();
                if ui
                    .add_enabled(has_row, egui::Button::new("Duplicate"))
                    .clicked()
                {
                    let r = self.row.unwrap();
                    self.snapshot();
                    let c = &mut self.db.collections[self.selected];
                    let mut copy = c.records[r].clone();
                    let index = c.allocate_index();
                    c.set(&mut copy, "Index", Value::Int(index as i64));
                    c.records.push(copy);
                    self.row = Some(c.records.len() - 1);
                }
                if ui
                    .add_enabled(has_row, egui::Button::new("Delete"))
                    .clicked()
                {
                    let r = self.row.unwrap();
                    self.snapshot();
                    self.db.collections[self.selected].records.remove(r);
                    self.row = None;
                    self.form_text.clear();
                }
                if ui.button("Validate").clicked() {
                    self.validate();
                }
                ui.separator();
                let dirty = if self.dirty { " (unsaved changes)" } else { "" };
                ui.label(format!("{}{dirty}", self.status));
            });
        });
    }

    /// Bottom panel with the last validation result; problems link to rows.
    fn validation_panel(&mut self, ui: &mut egui::Ui) {
        let Some(v) = &self.validation else {
            return;
        };
        let mut jump = None;
        let mut close = false;
        egui::Panel::bottom("validation")
            .default_size(160.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Validation");
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                egui::ScrollArea::vertical().show(ui, |ui| match v {
                    Validation::Failed(e) => {
                        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), e);
                    }
                    Validation::Problems(p) if p.is_empty() => {
                        ui.label("No problems: the server loads this data cleanly.");
                    }
                    Validation::Problems(p) => {
                        for problem in p {
                            if ui.link(problem.to_string()).clicked() {
                                jump = Some((problem.collection.clone(), problem.index));
                            }
                        }
                    }
                });
            });
        if close {
            self.validation = None;
        }
        if let Some((collection, index)) = jump {
            if let Some(ci) = self
                .db
                .collections
                .iter()
                .position(|c| c.short_name() == collection)
            {
                // DropInfo has no Index: the problem carries the row position.
                let pos = if collection == "DropInfo" {
                    Some(index as usize)
                } else {
                    self.position_of_index(ci, index)
                };
                if let Some(pos) = pos {
                    self.jump = Some((ci, pos));
                }
            }
        }
    }

    fn collections_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("collections")
            .default_size(220.0)
            .show(ui, |ui| {
                ui.heading("Collections");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for i in 0..self.db.collections.len() {
                        let c = &self.db.collections[i];
                        let label = format!("{} ({})", c.short_name(), c.records.len());
                        if ui.selectable_label(self.selected == i, label).clicked()
                            && self.selected != i
                        {
                            self.selected = i;
                            self.row = None;
                            self.sort = None;
                            self.form_text.clear();
                        }
                    }
                });
            });
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        let rows = self.visible_rows();
        let props: Vec<(String, String)> = self.db.collections[self.selected]
            .mapping
            .properties
            .iter()
            .map(|p| (p.name.clone(), p.type_name.clone()))
            .collect();
        ui.heading(format!(
            "{}  —  {} of {} rows",
            self.db.collections[self.selected].short_name(),
            rows.len(),
            self.db.collections[self.selected].records.len()
        ));
        let mut changed = false;
        let mut clicked_row = None;
        let mut jump = None;
        let mut sort_click = None;
        let selected = self.selected;
        let current = self.row;
        let sort = self.sort;
        let fk: Vec<Option<usize>> = props.iter().map(|(n, _)| self.fk_target(n)).collect();
        let text_height = 20.0;
        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(26.0));
        for (name, ty) in &props {
            let w = match ty.as_str() {
                "System.String" if name.contains("Description") => 260.0,
                "System.String" => 150.0,
                "System.Boolean" => 60.0,
                _ => 80.0,
            };
            builder = builder.column(Column::initial(w).at_least(40.0).clip(true));
        }
        builder
            .header(text_height, |mut header| {
                header.col(|_| {});
                for (pi, (name, _)) in props.iter().enumerate() {
                    header.col(|ui| {
                        let marker = match sort {
                            Some((p, true)) if p == pi => " ▲",
                            Some((p, false)) if p == pi => " ▼",
                            _ => "",
                        };
                        if ui.button(format!("{name}{marker}")).clicked() {
                            sort_click = Some(pi);
                        }
                    });
                }
            })
            .body(|body| {
                let db = &mut self.db;
                body.rows(text_height, rows.len(), |mut row| {
                    let ri = rows[row.index()];
                    let is_selected = current == Some(ri);
                    row.set_selected(is_selected);
                    row.col(|ui| {
                        if ui.selectable_label(is_selected, "▶").clicked() {
                            clicked_row = Some(ri);
                        }
                    });
                    for (pi, target) in fk.iter().enumerate() {
                        row.col(|ui| {
                            let c = &mut db.collections[selected];
                            let v = &mut c.records[ri].values[pi];
                            if let (Some(target), Some(idx)) = (*target, v.as_i32()) {
                                // Foreign key: value plus link to the named record.
                                if value::cell_widget(ui, v, true) {
                                    changed = true;
                                }
                                if idx != 0 {
                                    let name = db
                                        .collections
                                        .get(target)
                                        .and_then(|t| {
                                            t.records.iter().find(|r| t.index(r) == idx).map(|r| {
                                                NAME_PROPS
                                                    .iter()
                                                    .find_map(|n| {
                                                        t.get(r, n).and_then(Value::as_str)
                                                    })
                                                    .unwrap_or("")
                                                    .to_string()
                                            })
                                        })
                                        .unwrap_or_default();
                                    if ui.link(name).on_hover_text("open").clicked() {
                                        jump = Some((target, idx));
                                    }
                                }
                            } else if value::cell_widget(ui, v, true) {
                                changed = true;
                            }
                        });
                    }
                });
            });
        if let Some(pi) = sort_click {
            self.sort = match self.sort {
                Some((p, true)) if p == pi => Some((pi, false)),
                Some((p, false)) if p == pi => None,
                _ => Some((pi, true)),
            };
        }
        if let Some(r) = clicked_row {
            self.row = Some(r);
            self.form_text.clear();
        }
        if changed {
            self.drop_source = None;
            self.drop_draft = None;
            // Widgets already mutated the value; keep one undo point per frame.
            if !self.dirty {
                self.dirty = true;
            }
            self.status = "Edited".into();
        }
        if let Some((target, idx)) = jump {
            if let Some(pos) = self.position_of_index(target, idx) {
                self.jump = Some((target, pos));
            }
        }
    }

    fn detail_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("detail")
            .default_size(420.0)
            .show(ui, |ui| {
                let Some(ri) = self.row else {
                    ui.heading("Details");
                    ui.label("Select a row with ▶ to edit it here.");
                    return;
                };
                if ri >= self.db.collections[self.selected].records.len() {
                    self.row = None;
                    return;
                }
                let short = self.db.collections[self.selected].short_name().to_string();
                let name = self.record_name(
                    self.selected,
                    &self.db.collections[self.selected].records[ri],
                );
                ui.heading(format!("{short}: {name}"));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.preview(ui, &short, ri);
                    if short == "ItemInfo" {
                        let c = &self.db.collections[self.selected];
                        let source = c.index(&c.records[ri]);
                        if let Some(draft) = self.item_draft_panel.show(ui, &self.db, source) {
                            match self.create_item_draft(&draft) {
                                Ok(_) => return,
                                Err(e) => self.status = format!("Item draft failed: {e}"),
                            }
                        }
                    }
                    if short == "MonsterInfo" && self.monster_draft_controls(ui, ri) {
                        return;
                    }
                    ui.separator();
                    if short == "MonsterInfo" {
                        self.drop_draft_controls(ui, ri);
                        ui.separator();
                    }
                    self.form(ui, ri);
                    ui.separator();
                    self.relations(ui, ri);
                });
            });
    }

    fn create_item_draft(&mut self, draft: &crate::item_draft::Draft) -> anyhow::Result<i32> {
        let collections = self
            .db
            .collections
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.short_name(), "ItemInfo" | "ItemInfoStat"))
            .map(|(collection, c)| CollectionSnapshot {
                collection,
                records: c.records.clone(),
                next_index: c.next_index,
            })
            .collect();
        let id = draft.apply(&mut self.db)?;
        self.undo.push(Undo {
            selected: self.selected,
            collections,
        });
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
        self.selected = self
            .db
            .collections
            .iter()
            .position(|c| c.short_name() == "ItemInfo")
            .unwrap();
        self.row = self.position_of_index(self.selected, id);
        self.filter.clear();
        self.form_text.clear();
        self.dirty = true;
        self.validation = None;
        self.maps.invalidate();
        self.item_draft_panel = Default::default();
        self.status =
            format!("Created item #{id}. Save the database, then add it to loot or shop goods.");
        Ok(id)
    }

    fn create_monster_draft(&mut self, draft: &crate::monster_draft::Draft) -> anyhow::Result<i32> {
        let collections = self
            .db
            .collections
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.short_name(), "MonsterInfo" | "MonsterInfoStat"))
            .map(|(collection, c)| CollectionSnapshot {
                collection,
                records: c.records.clone(),
                next_index: c.next_index,
            })
            .collect();
        let id = draft.apply(&mut self.db)?;
        self.undo.push(Undo {
            selected: self.selected,
            collections,
        });
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
        self.selected = self
            .db
            .collections
            .iter()
            .position(|c| c.short_name() == "MonsterInfo")
            .unwrap();
        self.row = self.position_of_index(self.selected, id);
        self.filter.clear();
        self.form_text.clear();
        self.dirty = true;
        self.validation = None;
        self.maps.invalidate();
        self.monster_draft = None;
        self.status = format!(
            "Created monster #{id}. Save the database, then choose it in a spawn or generated map."
        );
        Ok(id)
    }

    fn monster_draft_controls(&mut self, ui: &mut egui::Ui, ri: usize) -> bool {
        let c = &self.db.collections[self.selected];
        let source = c.index(&c.records[ri]);
        if self.draft_source != Some(source) {
            self.draft_source = Some(source);
            self.draft_name = format!(
                "{} Draft",
                c.str_or(&c.records[ri], "MonsterName", "Monster")
            );
            self.draft_level = std::env::var("ZIRCON_EDITOR_MONSTER_LEVEL")
                .ok()
                .and_then(|v| v.parse::<i32>().ok())
                .filter(|v| (1..=255).contains(v))
                .unwrap_or_else(|| {
                    c.int_or(&c.records[ri], "Level", 1)
                        .saturating_add(5)
                        .clamp(1, 255) as i32
                });
            self.monster_draft = None;
            self.draft_error = None;
        }
        let mut created = false;
        egui::CollapsingHeader::new("Generate monster stat draft")
            .default_open(std::env::var_os("ZIRCON_EDITOR_MONSTER_DRAFT").is_some())
            .show(ui, |ui| {
            ui.label("Derive combat stats for a new level. Appearance, AI and movement speeds come from this monster. Drops are configured separately.");
            if ui.text_edit_singleline(&mut self.draft_name).changed() { self.monster_draft = None; }
            ui.horizontal(|ui| {
                ui.label("Target level");
                if ui.add(egui::DragValue::new(&mut self.draft_level).range(1..=255)).changed() {
                    self.monster_draft = None;
                }
            });
            let auto_preview = std::env::var_os("ZIRCON_EDITOR_MONSTER_DRAFT").is_some()
                && self.monster_draft.is_none() && self.draft_error.is_none();
            if ui.button("Preview stats").clicked() || auto_preview {
                match crate::monster_draft::propose(&self.db, source, self.draft_level, &self.draft_name) {
                    Ok(draft) => { self.monster_draft = Some(draft); self.draft_error = None; }
                    Err(e) => self.draft_error = Some(e.to_string()),
                }
            }
            if let Some(draft) = &self.monster_draft {
                ui.label(format!("Level {} to {}", draft.source_level, draft.target_level));
                ui.label(&draft.method);
                ui.label(format!("{} reference monsters", draft.reference_count));
                egui::Grid::new("monster_draft_stats").show(ui, |ui| {
                    ui.label("Stat"); ui.label("Source"); ui.label("Draft"); ui.end_row();
                    for change in &draft.changes {
                        ui.label(&change.label);
                        ui.label(change.before.to_string()); ui.label(change.after.to_string()); ui.end_row();
                    }
                    ui.label("Experience");
                    ui.label(format!("{:.2}", draft.experience_before));
                    ui.label(format!("{:.2}", draft.experience_after)); ui.end_row();
                });
                ui.label("Review these estimates in play before using them in progression.");
                if ui.button("Create new monster").clicked() {
                    let draft = self.monster_draft.take().unwrap();
                    match self.create_monster_draft(&draft) {
                        Ok(_) => created = true,
                        Err(e) => self.draft_error = Some(e.to_string()),
                    }
                }
            }
            if let Some(error) = &self.draft_error { ui.colored_label(egui::Color32::RED, error); }
        });
        created
    }

    fn apply_drop_draft(
        &mut self,
        draft: &crate::drop_draft::Draft,
        selection: Option<&[usize]>,
    ) -> anyhow::Result<()> {
        let collection = self
            .db
            .collections
            .iter()
            .position(|c| c.short_name() == "DropInfo")
            .ok_or_else(|| anyhow::anyhow!("DropInfo is unavailable"))?;
        let c = &self.db.collections[collection];
        let snapshot = CollectionSnapshot {
            collection,
            records: c.records.clone(),
            next_index: c.next_index,
        };
        match selection {
            Some(indices) => draft.apply_selected(&mut self.db, indices)?,
            None => draft.apply(&mut self.db)?,
        }
        self.undo.push(Undo {
            selected: self.selected,
            collections: vec![snapshot],
        });
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
        self.dirty = true;
        self.validation = None;
        self.maps.invalidate();
        self.drop_draft = None;
        self.monster_draft = None;
        self.status = "Drop template applied. Save the database to use it on the server.".into();
        Ok(())
    }

    fn drop_draft_controls(&mut self, ui: &mut egui::Ui, ri: usize) {
        let c = &self.db.collections[self.selected];
        let target = c.index(&c.records[ri]);
        if self.drop_source != Some(target) {
            self.drop_source = Some(target);
            self.drop_templates = crate::drop_draft::candidates(&self.db, target);
            self.drop_donor = self
                .drop_templates
                .first()
                .map(|t| t.monster_id)
                .unwrap_or(0);
            if let Ok(name) = std::env::var("ZIRCON_EDITOR_DROP_DONOR") {
                if let Some(t) = self.drop_templates.iter().find(|t| t.name == name) {
                    self.drop_donor = t.monster_id;
                }
            }
            self.material_donor = self
                .drop_templates
                .iter()
                .find(|t| t.material_count > 0)
                .map(|t| t.monster_id)
                .unwrap_or(0);
            self.drop_draft = None;
            self.drop_error = None;
        }
        egui::CollapsingHeader::new("Apply drop template")
            .default_open(std::env::var_os("ZIRCON_EDITOR_DROP_DRAFT").is_some())
            .show(ui, |ui| {
                ui.label("Select authored drops for a monster with no drops. Apply one donor directly, or queue rows from several donors and review them together.");
                let selected = self.drop_templates.iter().find(|t| t.monster_id == self.drop_donor)
                    .map(|t| format!("{} (level {})", t.name, t.level)).unwrap_or_else(|| "No template".into());
                egui::ComboBox::from_id_salt("drop_template_source").selected_text(selected).show_ui(ui, |ui| {
                    for template in &self.drop_templates {
                        if ui.selectable_value(&mut self.drop_donor, template.monster_id,
                            format!("{} · level {} · {} drops", template.name, template.level, template.row_count)).changed() {
                            self.drop_draft = None; self.drop_error = None;
                        }
                    }
                });
                egui::CollapsingHeader::new("Tier + biome materials")
                    .default_open(true).show(ui, |ui| {
                        ui.label("Use the donor above for non-material drops. Choose a monster from your intended biome for materials. Donors are ranked by boss status and nearby level; biome membership is your choice.");
                        let selected = self.drop_templates.iter().find(|t| t.monster_id == self.material_donor)
                            .map(|t| format!("{} · {} materials", t.name, t.material_count)).unwrap_or_else(|| "No material donor".into());
                        egui::ComboBox::from_id_salt("loot_material_source").selected_text(selected).show_ui(ui, |ui| {
                            for t in self.drop_templates.iter().filter(|t| t.material_count > 0) {
                                ui.selectable_value(&mut self.material_donor, t.monster_id, format!("{} · level {} · {} materials", t.name, t.level, t.material_count));
                            }
                        });
                        let pending = self.loot_recipe.as_ref().is_some_and(|r| !r.entries().is_empty());
                        if ui.add_enabled(!pending && self.material_donor > 0, egui::Button::new("Draft tier + materials")).on_hover_text("Keeps authored odds and conditions. Review the resulting queue before applying; it is not a balanced reward estimate.").clicked() {
                            match crate::drop_draft::tier_material_recipe(&self.db, target, self.drop_donor, self.material_donor) {
                                Ok(recipe) => { self.loot_recipe = Some(recipe); self.drop_error = None; self.drop_selection.fill(false); }
                                Err(e) => self.drop_error = Some(e.to_string()),
                            }
                        }
                        if pending { ui.label("Apply or discard the queued draft before building another."); }
                    });
                let auto = std::env::var_os("ZIRCON_EDITOR_DROP_DRAFT").is_some()
                    && self.drop_draft.is_none() && self.drop_error.is_none();
                if ui.button("Preview drop table").clicked() || auto {
                    match crate::drop_draft::propose(&self.db, target, self.drop_donor) {
                        Ok(draft) => { self.drop_selection = vec![true; draft.rows.len()]; self.drop_draft = Some(draft); self.drop_error = None; }
                        Err(e) => self.drop_error = Some(e.to_string()),
                    }
                }
                if let Some(draft) = &self.drop_draft {
                    ui.label(format!("{}: {} rows", draft.donor_name, draft.rows.len()));
                    ui.label(&draft.notes);
                    ui.horizontal_wrapped(|ui| {
                        for category in ["All", "None", "Consumables", "Equipment", "Materials", "Other"] {
                            if ui.button(category).clicked() {
                                for (chosen, row) in self.drop_selection.iter_mut().zip(&draft.rows) {
                                    *chosen = category == "All" || row.category == category;
                                }
                            }
                        }
                    });
                    ui.label(format!("{} of {} drops selected", self.drop_selection.iter().filter(|v| **v).count(), draft.rows.len()));
                    egui::ScrollArea::vertical().id_salt("drop_preview_scroll").max_height(260.0).show(ui, |ui| {
                        egui::Grid::new("drop_preview_grid").striped(true).show(ui, |ui| {
                            ui.label("Use"); ui.label("Item"); ui.label("Base odds"); ui.label("Amount"); ui.label("Restrictions"); ui.end_row();
                            for (i, row) in draft.rows.iter().enumerate() {
                                ui.checkbox(&mut self.drop_selection[i], "").on_hover_text(&row.category);
                                ui.label(&row.item_name).on_hover_text(format!("Item #{}", row.item_id));
                                ui.label(if row.chance > 0 { format!("1 in {}", row.chance) } else { "Disabled".into() });
                                let low = (row.amount / 2).max(1);
                                let high = (i64::from(row.amount / 2) + i64::from(row.amount) - 1).max(1);
                                ui.label(if i64::from(low) == high { low.to_string() } else { format!("{low}–{high}") });
                                ui.label(&row.restrictions); ui.end_row();
                            }
                        });
                    });
                    Self::loot_yield_controls(ui, "selected_loot_yield", crate::drop_draft::yield_summary(draft.rows.iter().zip(&self.drop_selection).filter_map(|(row, chosen)| chosen.then_some(row))));
                    if ui.add_enabled(self.drop_selection.iter().any(|v| *v), egui::Button::new("Add selected to loot draft")).clicked() {
                        let indices: Vec<_> = self.drop_selection.iter().enumerate().filter_map(|(i, selected)| selected.then_some(i)).collect();
                        let recipe = self.loot_recipe.get_or_insert_with(|| crate::drop_draft::Recipe::new(draft));
                        match recipe.add(draft, &indices) {
                            Ok(()) => { self.drop_error = None; self.drop_selection.fill(false); }
                            Err(e) => self.drop_error = Some(e.to_string()),
                        }
                    }
                    let has_queue = self.loot_recipe.as_ref().is_some_and(|r| !r.entries().is_empty());
                    if ui.add_enabled(!has_queue && self.drop_selection.iter().any(|v| *v), egui::Button::new("Apply selected drops")).clicked() {
                        let indices: Vec<_> = self.drop_selection.iter().enumerate().filter_map(|(i, selected)| selected.then_some(i)).collect();
                        let draft = self.drop_draft.take().unwrap();
                        if let Err(e) = self.apply_drop_draft(&draft, Some(&indices)) { self.drop_error = Some(e.to_string()); }
                        else { self.drop_error = None; }
                    }
                }
                self.loot_recipe_controls(ui);
                if let Some(error) = &self.drop_error { ui.colored_label(egui::Color32::RED, error); }
            });
    }

    fn apply_loot_recipe(&mut self, recipe: &crate::drop_draft::Recipe) -> anyhow::Result<()> {
        let collection = self
            .db
            .collections
            .iter()
            .position(|c| c.short_name() == "DropInfo")
            .ok_or_else(|| anyhow::anyhow!("DropInfo is unavailable"))?;
        let c = &self.db.collections[collection];
        let snapshot = CollectionSnapshot {
            collection,
            records: c.records.clone(),
            next_index: c.next_index,
        };
        recipe.apply(&mut self.db)?;
        self.undo.push(Undo {
            selected: self.selected,
            collections: vec![snapshot],
        });
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
        self.dirty = true;
        self.validation = None;
        self.maps.invalidate();
        self.drop_source = None;
        self.drop_draft = None;
        self.monster_draft = None;
        self.status = format!(
            "Applied {} queued drops to monster #{}. Save the database to use them.",
            recipe.entries().len(),
            recipe.target_id()
        );
        Ok(())
    }

    fn loot_yield_controls(ui: &mut egui::Ui, id: &str, summary: crate::drop_draft::YieldSummary) {
        egui::CollapsingHeader::new("Base loot yield comparison").id_salt(id).default_open(true).show(ui, |ui| {
            ui.label("Average item units per 100 independent base rolls. Extra gated yield assumes every required mask/event condition is met. Account drop progress, group sharing and harvest timing change actual results.");
            if summary.disabled > 0 || summary.part_only > 0 {
                ui.label(format!("Excluded rows: {} disabled, {} PartOnly (counts may overlap).", summary.disabled, summary.part_only));
            }
            egui::ScrollArea::vertical().id_salt((id, "scroll")).max_height(150.0).show(ui, |ui| {
                egui::Grid::new((id, "grid")).striped(true).show(ui, |ui| {
                    ui.label("Item"); ui.label("Ungated units / 100"); ui.label("Extra if gated / 100"); ui.end_row();
                    for (item, value) in &summary.items {
                        ui.label(&value.name).on_hover_text(format!("Item #{item}"));
                        ui.label(if value.ordinary > 0.0 && value.ordinary < 0.0001 { format!("{:.4e}", value.ordinary) } else { format!("{:.4}", value.ordinary) });
                        ui.label(if value.conditional > 0.0 && value.conditional < 0.0001 { format!("{:.4e}", value.conditional) } else { format!("{:.4}", value.conditional) }); ui.end_row();
                    }
                });
            });
            if summary.items.is_empty() { ui.label("No active ordinary loot rows selected."); }
        });
    }

    fn loot_recipe_controls(&mut self, ui: &mut egui::Ui) {
        let Some(recipe) = &mut self.loot_recipe else {
            return;
        };
        ui.separator();
        let name = self
            .db
            .collection("MonsterInfo")
            .and_then(|c| {
                c.records
                    .iter()
                    .find(|r| c.index(r) == recipe.target_id())
                    .map(|r| c.str_or(r, "MonsterName", "Monster"))
            })
            .unwrap_or("Missing monster");
        ui.heading(format!("Queued loot for {name}"));
        ui.label("Queue rows from more donors before applying. Changing the database invalidates this draft.");
        let mut items = std::collections::BTreeSet::new();
        if recipe
            .entries()
            .iter()
            .any(|e| !items.insert(e.row.item_id))
        {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Repeated items are separate rolls; their chances are not merged.",
            );
        }
        Self::loot_yield_controls(
            ui,
            "queued_loot_yield",
            crate::drop_draft::yield_summary(recipe.entries().iter().map(|entry| &entry.row)),
        );
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("loot_recipe_review")
            .max_height(240.0)
            .show(ui, |ui| {
                for (index, entry) in recipe.entries().iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                        ui.label(format!("{}: {}", entry.donor_name, entry.row.item_name));
                        let row = &entry.row;
                        let odds = if row.chance > 0 {
                            format!("1 in {}", row.chance)
                        } else {
                            "Disabled".into()
                        };
                        let low = (row.amount / 2).max(1);
                        let high = (i64::from(row.amount / 2) + i64::from(row.amount) - 1).max(1);
                        let amount = if i64::from(low) == high {
                            low.to_string()
                        } else {
                            format!("{low}–{high}")
                        };
                        ui.label(format!("{odds}, amount {amount}, {}", row.restrictions));
                    });
                }
            });
        if let Some(index) = remove {
            recipe.remove(index);
        }
        let mut apply = false;
        let mut discard = false;
        ui.horizontal(|ui| {
            apply = ui
                .add_enabled(
                    !recipe.entries().is_empty(),
                    egui::Button::new("Apply queued loot"),
                )
                .clicked();
            discard = ui.button("Discard queued loot").clicked();
        });
        if discard || recipe.entries().is_empty() {
            self.loot_recipe = None;
            self.drop_error = None;
        } else if apply {
            let recipe = self.loot_recipe.take().unwrap();
            if let Err(e) = self.apply_loot_recipe(&recipe) {
                self.drop_error = Some(e.to_string());
                self.loot_recipe = Some(recipe);
            } else {
                self.drop_error = None;
            }
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui, short: &str, ri: usize) {
        let c = &self.db.collections[self.selected];
        let r = &c.records[ri];
        if matches!(short, "CastleInfo" | "CastleGateInfo") {
            let selection = if short == "CastleInfo" {
                crate::castle_gate_ui::Selection::Castle(c.index(r))
            } else {
                crate::castle_gate_ui::Selection::Gate(c.index(r))
            };
            self.castle_gate.show(ui, &self.assets, &self.db, selection);
            return;
        }
        let shown = match short {
            "ItemInfo" => {
                let image = c.int_or(r, "Image", 0) as u32;
                self.sprite_project.show(
                    ui,
                    &self.assets,
                    &self.db_path,
                    crate::sprite_ui::Source {
                        kind: "item",
                        id: c.index(r),
                        image: image as u16,
                        armour_shape: (c.int_or(r, "ItemType", 0) == 3)
                            .then(|| c.int_or(r, "Shape", 0) as u16),
                        equipment_shape: matches!(c.int_or(r, "ItemType", 0), 2 | 5 | 27)
                            .then(|| c.int_or(r, "Shape", 0) as u16),
                        equipment_type: c.int_or(r, "ItemType", 0) as u8,
                    },
                );
                self.previews.show(ui, preview::STORE_ITEMS, image, 2.0)
            }
            "MagicInfo" => {
                let icon = c.int_or(r, "Icon", 0) as u32;
                self.previews.show(ui, preview::MAGIC_ICON, icon, 2.0)
            }
            "NPCInfo" => {
                let image = c.int_or(r, "Image", 0) as u32;
                self.previews.show(ui, preview::NPC, image * 100, 1.0)
            }
            "MonsterInfo" => {
                let image = c.int_or(r, "Image", 0) as u16;
                let dir = self.monster_direction;
                let shown = match Previews::monster_frame(image, dir) {
                    Some((lib, frame)) => self.previews.show(ui, lib, frame, 1.0),
                    None => false,
                };
                ui.horizontal(|ui| {
                    ui.label("Facing:");
                    ui.add(egui::Slider::new(&mut self.monster_direction, 0..=7));
                });
                self.sprite_variant.ui(ui, &self.assets, image);
                self.sprite_project.show(
                    ui,
                    &self.assets,
                    &self.db_path,
                    crate::sprite_ui::Source {
                        kind: "monster",
                        id: c.index(r),
                        image,
                        armour_shape: None,
                        equipment_shape: None,
                        equipment_type: 0,
                    },
                );
                shown
            }
            _ => return,
        };
        if !shown {
            ui.label("(no sprite for this image)");
        }
    }

    fn form(&mut self, ui: &mut egui::Ui, ri: usize) {
        let props: Vec<(String, String)> = self.db.collections[self.selected]
            .mapping
            .properties
            .iter()
            .map(|p| (p.name.clone(), p.type_name.clone()))
            .collect();
        let mut commit: Option<(usize, String)> = None;
        let mut changed = false;
        egui::Grid::new("form")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                for (pi, (name, ty)) in props.iter().enumerate() {
                    ui.label(name).on_hover_text(ty);
                    let v = &mut self.db.collections[self.selected].records[ri].values[pi];
                    match v {
                        Value::Bool(_) | Value::Int(_) | Value::UInt(_) | Value::Float(_) => {
                            if value::cell_widget(ui, v, true) {
                                changed = true;
                            }
                        }
                        Value::Str(_) => {
                            if value::cell_widget(ui, v, false) {
                                changed = true;
                            }
                        }
                        other => {
                            let text = self
                                .form_text
                                .entry(pi)
                                .or_insert_with(|| value::to_text(other));
                            let resp = ui
                                .add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                            if resp.lost_focus() {
                                commit = Some((pi, text.clone()));
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        if changed {
            self.drop_source = None;
            self.drop_draft = None;
            self.dirty = true;
            self.status = "Edited".into();
        }
        if let Some((pi, text)) = commit {
            let c = &mut self.db.collections[self.selected];
            let current = &c.records[ri].values[pi];
            match value::from_text(current, &text) {
                Some(v) if v != *current => {
                    c.records[ri].values[pi] = v;
                    self.dirty = true;
                    self.form_error = None;
                    self.status = "Edited".into();
                }
                Some(_) => {}
                None => {
                    self.form_error = Some(format!("Cannot parse '{text}' for {}", props[pi].0));
                }
            }
        }
        if let Some(e) = &self.form_error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), e);
        }
    }

    /// Records in other collections that point at this one.
    fn relations(&mut self, ui: &mut egui::Ui, ri: usize) {
        let short = self.db.collections[self.selected].short_name().to_string();
        let index = self.db.collections[self.selected]
            .index(&self.db.collections[self.selected].records[ri]);
        let mut jump = None;
        for ci in 0..self.db.collections.len() {
            if ci == self.selected {
                continue;
            }
            let fk_props: Vec<usize> = self.db.collections[ci]
                .mapping
                .properties
                .iter()
                .enumerate()
                .filter(|(_, p)| p.type_name == "System.Int32")
                .filter(|(_, p)| {
                    self.fk_target(&p.name)
                        .map(|t| self.db.collections[t].short_name() == short)
                        .unwrap_or(false)
                })
                .map(|(i, _)| i)
                .collect();
            if fk_props.is_empty() {
                continue;
            }
            let c = &self.db.collections[ci];
            let hits: Vec<usize> = c
                .records
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    fk_props
                        .iter()
                        .any(|p| r.values[*p].as_i32() == Some(index))
                })
                .map(|(i, _)| i)
                .collect();
            if hits.is_empty() {
                continue;
            }
            ui.collapsing(format!("{} ({})", c.short_name(), hits.len()), |ui| {
                for h in hits.iter().take(200) {
                    let r = &c.records[*h];
                    let summary: Vec<String> = c
                        .mapping
                        .properties
                        .iter()
                        .zip(&r.values)
                        .filter(|(p, _)| {
                            p.name != "Index"
                                && !fk_props
                                    .contains(&c.property_index(&p.name).unwrap_or(usize::MAX))
                        })
                        .take(4)
                        .map(|(p, v)| format!("{}={}", p.name, value::summary(v)))
                        .collect();
                    if ui.link(summary.join("  ")).clicked() {
                        jump = Some((ci, *h));
                    }
                }
                if hits.len() > 200 {
                    ui.label(format!("… {} more", hits.len() - 200));
                }
            });
        }
        if let Some(j) = jump {
            self.jump = Some(j);
        }
    }
}

#[cfg(test)]
mod map_tests {
    use super::*;
    use crate::maps::{paint, Edit};
    use mir_formats::map::MapFile;

    #[test]
    fn map_placements_round_trip_through_server_loader_and_undo_restores_saved_rows() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("zircon-editor-maps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("System.db");
        std::fs::copy(assets.join("../Database/System.db"), &path).unwrap();
        let mut ed = Editor::open(path.clone(), assets).unwrap();
        let c = ed.db.collection("MapInfo").unwrap();
        let map_id = c.index(&c.records[0]);
        let monster = ed
            .db
            .collection("MonsterInfo")
            .unwrap()
            .index(&ed.db.collection("MonsterInfo").unwrap().records[0]);
        let original = ed.db.collection("MapRegion").unwrap().records.len();
        let region = ed
            .apply_map_edit(Edit {
                collection: "MapRegion",
                index: None,
                values: vec![
                    ("Map", Value::Int(map_id as i64)),
                    ("Description", Value::Str("Placement test".into())),
                    ("PointRegion", Value::PointArray(Some(vec![]))),
                ],
            })
            .unwrap();
        let mut map = MapFile::blank(4, 4);
        for x in 0..4 {
            for y in 0..4 {
                map.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        let edit = paint(&ed.db, &map, map_id, region, (1, 1), (2, 2), false).unwrap();
        ed.apply_map_edit(edit).unwrap();
        let spawn = ed
            .apply_map_edit(Edit {
                collection: "RespawnInfo",
                index: None,
                values: vec![
                    ("Region", Value::Int(region as i64)),
                    ("Monster", Value::Int(monster as i64)),
                    ("Count", Value::Int(3)),
                    ("Delay", Value::Int(60)),
                ],
            })
            .unwrap();
        let safe = ed
            .apply_map_edit(Edit {
                collection: "SafeZoneInfo",
                index: None,
                values: vec![
                    ("Region", Value::Int(region as i64)),
                    ("BindRegion", Value::Int(region as i64)),
                ],
            })
            .unwrap();
        ed.save();
        assert!(!ed.dirty, "{}", ed.status);
        let loaded = mir_server::data::GameData::load(&path).unwrap();
        assert_eq!(
            loaded.regions[&region].points(4),
            vec![(1, 1), (1, 2), (2, 1), (2, 2)]
        );
        assert!(loaded.respawns.iter().any(|r| r.index == spawn
            && r.region == region
            && r.monster == monster
            && r.count == 3
            && r.delay == 60));
        assert!(loaded
            .safe_zones
            .iter()
            .any(|s| s.index == safe && s.region == region && s.bind_region == region));
        assert!(std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(".bak-")));
        ed.undo();
        assert!(ed.dirty, "Undo after save must enable saving");
        assert!(!mir_server::data::GameData::from_db(&ed.db)
            .unwrap()
            .safe_zones
            .iter()
            .any(|s| s.index == safe));
        ed.undo();
        ed.undo();
        ed.undo();
        assert_eq!(
            ed.db.collection("MapRegion").unwrap().records.len(),
            original
        );
        ed.reload();
        assert!(mir_server::data::GameData::from_db(&ed.db)
            .unwrap()
            .safe_zones
            .iter()
            .any(|s| s.index == safe));
        assert!(!ed.dirty);
        drop(ed);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod monster_tests {
    use super::*;

    #[test]
    fn monster_creation_saves_and_undo_restores_both_collections() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let folder = std::env::temp_dir().join(format!(
            "zircon-monster-editor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("System.db");
        std::fs::copy(assets.join("../Database/System.db"), &path).unwrap();
        let mut ed = Editor::open(path.clone(), assets.clone()).unwrap();
        ed.tile_mode = false;
        ed.selected = ed
            .db
            .collections
            .iter()
            .position(|c| c.short_name() == "MonsterInfo")
            .unwrap();
        let c = &ed.db.collections[ed.selected];
        let source = c.index(
            c.records
                .iter()
                .find(|r| c.str_or(r, "MonsterName", "") == "Oma")
                .unwrap(),
        );
        let before = ed.db.to_bytes().unwrap();
        let draft = crate::monster_draft::propose(&ed.db, source, 20, "Draft Oma").unwrap();
        let id = ed.create_monster_draft(&draft).unwrap();
        assert!(ed.dirty);
        ed.db.save(&path).unwrap();
        ed.dirty = false;
        let loaded = mir_server::data::GameData::load(&path).unwrap();
        assert_eq!(loaded.monsters[&id].name, "Draft Oma");
        assert_eq!(loaded.monsters[&id].level, 20);
        assert_eq!(loaded.monsters[&id].image, loaded.monsters[&source].image);
        if let Some(bundle) = std::env::var_os("ZIRCON_MONSTER_QA_DIR").map(PathBuf::from) {
            let source_map = mir_formats::map::MapFile::load(assets.join("Map/D102.map")).unwrap();
            let (doc, layout, _) =
                crate::dungeon::generate(&source_map, 64, 64, 42, 8, 4, &assets).unwrap();
            crate::generate::export(&bundle, &doc.map, &ed.db, &layout, id).unwrap();
            let loaded = mir_server::data::GameData::load(bundle.join("System.db")).unwrap();
            let map_id = loaded
                .maps
                .values()
                .find(|m| m.file_name == "GeneratedCave")
                .unwrap()
                .index;
            let mut loaded = loaded;
            let regions: std::collections::BTreeSet<_> = loaded
                .regions
                .values()
                .filter(|r| r.map == map_id)
                .map(|r| r.index)
                .collect();
            loaded.safe_zones.retain(|z| regions.contains(&z.region));
            let mut world = mir_server::world::World::new(loaded, bundle.join("Map"), None);
            let mut character = mir_server::accounts::test_character("DraftTester");
            character.map = "GeneratedCave".into();
            character.location.x = layout.start.0;
            character.location.y = layout.start.1;
            world.add_player(1, 1, &character).unwrap();
            assert_eq!(world.on_map(map_id).filter(|o| o.is_monster()).count(), 8);
            println!(
                "Monster QA id {id}; test position {:?}; bundle {}",
                layout.spawns[layout.spawns.len() / 2],
                bundle.display()
            );
        }
        ed.undo();
        assert!(ed.dirty, "Undo after save must remain unsaved");
        assert_eq!(
            ed.db.to_bytes().unwrap(),
            before,
            "Both rows and ID counters must be restored"
        );
        ed.db.save(&path).unwrap();
        assert!(!mir_server::data::GameData::load(&path)
            .unwrap()
            .monsters
            .contains_key(&id));
        std::fs::remove_dir_all(folder).unwrap();
    }
}

#[cfg(test)]
mod drop_tests {
    use super::*;

    #[test]
    fn queued_loot_is_unsaved_and_applies_with_one_undo_after_save() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let root = std::env::temp_dir().join(format!("zircon-recipe-undo-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("System.db");
        std::fs::copy(assets.join("../Database/System.db"), &path).unwrap();
        let mut ed = Editor::open(path, assets).unwrap();
        let c = ed.db.collection("MonsterInfo").unwrap();
        let donor = c.index(
            c.records
                .iter()
                .find(|r| c.str_or(r, "MonsterName", "") == "Chicken")
                .unwrap(),
        );
        let target = crate::monster_draft::propose(&ed.db, donor, 20, "Recipe Undo")
            .unwrap()
            .apply(&mut ed.db)
            .unwrap();
        let drafts: Vec<_> = crate::drop_draft::candidates(&ed.db, target)
            .iter()
            .filter_map(|t| crate::drop_draft::propose(&ed.db, target, t.monster_id).ok())
            .take(2)
            .collect();
        assert_eq!(drafts.len(), 2);
        let mut recipe = crate::drop_draft::Recipe::new(&drafts[0]);
        for draft in &drafts {
            recipe.add(draft, &[0]).unwrap();
        }
        let before = ed.db.to_bytes().unwrap();
        ed.db.save(&ed.db_path).unwrap();
        ed.dirty = false;
        ed.loot_recipe = Some(recipe);
        assert!(ed.unsaved());
        ed.reload();
        assert!(ed.status.contains("Apply or discard"));
        assert_eq!(ed.db.to_bytes().unwrap(), before);
        let recipe = ed.loot_recipe.take().unwrap();
        let undo_count = ed.undo.len();
        ed.apply_loot_recipe(&recipe).unwrap();
        assert_eq!(ed.undo.len(), undo_count + 1);
        ed.db.save(&ed.db_path).unwrap();
        ed.dirty = false;
        ed.undo();
        assert!(ed.dirty);
        assert_eq!(ed.db.to_bytes().unwrap(), before);
        ed.dirty = false;
        assert!(!ed.unsaved());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn drop_template_undo_roundtrip_and_monster_kill_produces_loot() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let keep = std::env::var_os("ZIRCON_DROP_QA_DIR").map(PathBuf::from);
        let root = std::env::temp_dir().join(format!(
            "zircon-drop-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("System.db");
        std::fs::copy(assets.join("../Database/System.db"), &path).unwrap();
        let mut ed = Editor::open(path, assets.clone()).unwrap();
        ed.tile_mode = false;
        ed.selected = ed
            .db
            .collections
            .iter()
            .position(|c| c.short_name() == "MonsterInfo")
            .unwrap();
        let c = &ed.db.collections[ed.selected];
        let donor = c.index(
            c.records
                .iter()
                .find(|r| c.str_or(r, "MonsterName", "") == "Oma")
                .unwrap(),
        );
        let monster = crate::monster_draft::propose(&ed.db, donor, 20, "Loot Draft Oma").unwrap();
        let target = ed.create_monster_draft(&monster).unwrap();
        let data = mir_server::data::GameData::from_db(&ed.db).unwrap();
        let item = data
            .items
            .values()
            .filter(|i| i.item_type == 1 && i.shape == 0 && i.stat(2) > 0)
            .min_by_key(|i| i.index)
            .unwrap()
            .index;
        // A guaranteed single potion makes the death/drop integration deterministic.
        // Only the disposable fixture is adjusted; the template preserves it exactly.
        let drops = ed.db.collection_mut("DropInfo").unwrap();
        let mut row = drops
            .records
            .iter()
            .find(|r| drops.int_or(r, "Monster", 0) == i64::from(donor))
            .unwrap()
            .clone();
        let monster_col = drops.property_index("Monster").unwrap();
        drops
            .records
            .retain(|r| r.values[monster_col].as_i64() != Some(i64::from(donor)));
        for (field, value) in [
            ("Monster", Value::Int(donor.into())),
            ("Item", Value::Int(item.into())),
            ("Chance", Value::Int(1)),
            ("Amount", Value::Int(1)),
            ("DropSet", Value::Int(0)),
            ("PartOnly", Value::Bool(false)),
            ("EasterEvent", Value::Bool(false)),
        ] {
            drops.set(&mut row, field, value);
        }
        drops.records.push(row.clone());
        let index = drops.allocate_index();
        drops.set(&mut row, "Index", Value::Int(index.into()));
        drops.set(&mut row, "DropSet", Value::Int(3));
        drops.set(&mut row, "EasterEvent", Value::Bool(true));
        drops.records.push(row);
        let before = ed.db.to_bytes().unwrap();
        let draft = crate::drop_draft::propose(&ed.db, target, donor).unwrap();
        ed.apply_drop_draft(&draft, Some(&[1])).unwrap();
        assert_eq!(
            mir_server::data::GameData::from_db(&ed.db)
                .unwrap()
                .drops
                .iter()
                .filter(|d| d.monster == target)
                .count(),
            1
        );
        ed.db.save(&ed.db_path).unwrap();
        ed.dirty = false;
        ed.undo();
        assert!(ed.dirty);
        assert_eq!(ed.db.to_bytes().unwrap(), before);
        let draft = crate::drop_draft::propose(&ed.db, target, donor).unwrap();
        ed.apply_drop_draft(&draft, Some(&[1])).unwrap();
        let bundle = keep.clone().unwrap_or_else(|| root.join("bundle"));
        let source_map = mir_formats::map::MapFile::load(assets.join("Map/D102.map")).unwrap();
        let (doc, mut layout, _) =
            crate::dungeon::generate(&source_map, 64, 64, 42, 8, 4, &assets).unwrap();
        layout.drop_set = 7;
        layout.easter_event_chance = 1;
        crate::generate::export(&bundle, &doc.map, &ed.db, &layout, target).unwrap();
        let (restored, _) = crate::generate::read_layout(&bundle.join("Map/GeneratedCave.map"))
            .unwrap()
            .unwrap();
        assert_eq!((restored.drop_set, restored.easter_event_chance), (7, 1));
        let mut legacy = serde_json::to_value(&layout).unwrap();
        legacy.as_object_mut().unwrap().remove("drop_set");
        legacy
            .as_object_mut()
            .unwrap()
            .remove("easter_event_chance");
        let old: crate::generate::Layout = serde_json::from_value(legacy).unwrap();
        assert_eq!((old.drop_set, old.easter_event_chance), (0, 0));

        MirDb::parse(&before)
            .unwrap()
            .save(bundle.join("Preview.db"))
            .unwrap();
        let mut loaded = mir_server::data::GameData::load(bundle.join("System.db")).unwrap();
        let map_id = loaded
            .maps
            .values()
            .find(|m| m.file_name == "GeneratedCave")
            .unwrap()
            .index;
        let regions: std::collections::BTreeSet<_> = loaded
            .regions
            .values()
            .filter(|r| r.map == map_id)
            .map(|r| r.index)
            .collect();
        loaded.safe_zones.retain(|z| regions.contains(&z.region));
        let copied = loaded.drops.iter().find(|d| d.monster == target).unwrap();
        assert!(copied.easter_event);
        assert_eq!(copied.drop_set, 3);
        let mut world = mir_server::world::World::new(loaded, bundle.join("Map"), None);
        world.easter_event_until = 60_000;
        let mut character = mir_server::accounts::test_character("LootTester");
        character.map = "GeneratedCave".into();
        character.location.x = layout.start.0;
        character.location.y = layout.start.1;
        world.add_player(1, 1, &character).unwrap();
        let (victim, at) = world
            .on_map(map_id)
            .filter(|o| o.is_monster())
            .find_map(|o| {
                let at = (o.location.x - 1, o.location.y);
                (doc.map.is_walkable(at.0, at.1)
                    && !world.on_map(map_id).any(|other| {
                        !other.dead && other.location.x == at.0 && other.location.y == at.1
                    }))
                .then_some((o.id, at))
            })
            .unwrap();
        character.name = "LootKiller".into();
        character.location.x = at.0;
        character.location.y = at.1;
        let player = world.add_player(2, 2, &character).unwrap();
        assert_eq!(
            (
                world.objects[&player].location.x,
                world.objects[&player].location.y
            ),
            at
        );
        let attacker = world.objects.get_mut(&player).unwrap();
        attacker.stats.min_dc = 10000;
        attacker.stats.max_dc = 10000;
        attacker.stats.accuracy = 10000;
        let monster = world.objects.get_mut(&victim).unwrap();
        monster.hp = 1;
        monster.stats.agility = 0;
        for _ in 0..5 {
            world.now += 2000;
            world.player_attack(
                player,
                serde_json::from_value(serde_json::json!("Right")).unwrap(),
                None,
            );
            world.tick(world.now + 600);
            if world.objects[&victim].dead {
                break;
            }
        }
        assert!(world.objects[&victim].dead);
        assert!(
            world.on_map(map_id).any(|o| match &o.kind {
                mir_server::world::Kind::Item(drop) =>
                    drop.item.info == item && drop.item.count == 1 && drop.owner == Some(2),
                _ => false,
            }),
            "Killing the drafted monster must produce its guaranteed potion"
        );
        println!(
            "Drop QA bundle {}; monster {target}; item {item}; position {},{}",
            bundle.display(),
            at.0,
            at.1
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod item_draft_tests {
    use super::*;
    #[test]
    fn item_creation_save_and_single_undo_restore_rows_and_allocators() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let folder = std::env::temp_dir().join(format!(
            "zircon-item-undo-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("System.db");
        std::fs::copy(assets.join("../Database/System.db"), &path).unwrap();
        let mut editor = Editor::open(path.clone(), assets).unwrap();
        editor.tile_mode = false;
        editor.selected = editor
            .db
            .collections
            .iter()
            .position(|c| c.short_name() == "ItemInfo")
            .unwrap();
        let items = editor.db.collection("ItemInfo").unwrap();
        let source = items.index(
            items
                .records
                .iter()
                .find(|r| items.str_or(r, "ItemName", "") == "Wood Sword")
                .unwrap(),
        );
        let before = editor.db.to_bytes().unwrap();
        let draft = crate::item_draft::propose(&editor.db, source, 30, "Undo Draft Sword").unwrap();
        let id = editor.create_item_draft(&draft).unwrap();
        assert!(editor.dirty);
        assert_eq!(editor.undo.len(), 1);
        assert_eq!(editor.undo[0].collections.len(), 2);
        editor.save();
        assert!(!editor.dirty, "{}", editor.status);
        let loaded = mir_server::data::GameData::load(&path).unwrap();
        assert_eq!(loaded.items[&id].name, "Undo Draft Sword");
        assert_eq!(loaded.items[&id].required_amount, 30);
        editor.undo();
        assert!(editor.dirty);
        assert!(editor.undo.is_empty());
        // Entire serialized DB also checks both allocator counters and unrelated records.
        assert_eq!(editor.db.to_bytes().unwrap(), before);
        editor.save();
        assert!(!editor.dirty, "{}", editor.status);
        assert_eq!(MirDb::load(&path).unwrap().to_bytes().unwrap(), before);
        std::fs::remove_dir_all(folder).unwrap();
    }
}
