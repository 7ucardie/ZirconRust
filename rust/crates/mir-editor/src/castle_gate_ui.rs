//! Native gate placement previews; every save creates a private database snapshot.
use crate::castle_gate_authoring::{self, Preview, Request};
use anyhow::{ensure, Context, Result};
use eframe::egui;
use mir_formats::mirdb::MirDb;
use std::{io::Write, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Castle(i32),
    Gate(i32),
}

pub struct Panel {
    selection: Option<Selection>,
    x: i32,
    y: i32,
    monster: i32,
    gate_image: i32,
    repair_cost: i32,
    output: String,
    preview: Option<Preview>,
    status: String,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            selection: None,
            x: 0,
            y: 0,
            monster: 1,
            gate_image: 540,
            repair_cost: 0,
            output: "castle-gate-draft.db".into(),
            preview: None,
            status: String::new(),
        }
    }
}
impl Panel {
    fn select(&mut self, db: &MirDb, selection: Selection) {
        if self.selection == Some(selection) {
            return;
        }
        self.selection = Some(selection);
        self.preview = None;
        self.status.clear();
        self.x = 0;
        self.y = 0;
        self.repair_cost = 0;
        if let Some(monsters) = db.collection("MonsterInfo") {
            if let Some(row) = monsters
                .records
                .iter()
                .find(|row| {
                    monsters
                        .int(row, "Image")
                        .is_some_and(|image| (540..=543).contains(&image))
                })
                .or_else(|| monsters.records.first())
            {
                self.monster = monsters.index(row);
            }
        }
        if let Selection::Gate(id) = selection {
            if let Some(gates) = db.collection("CastleGateInfo") {
                if let Some(row) = gates.records.iter().find(|row| gates.index(row) == id) {
                    self.x = gates.int_or(row, "X", 0) as i32;
                    self.y = gates.int_or(row, "Y", 0) as i32;
                    self.monster = gates.int_or(row, "Monster", 0) as i32;
                    self.repair_cost = gates.int_or(row, "RepairCost", 0) as i32;
                    if let Some(monsters) = db.collection("MonsterInfo") {
                        if let Some(monster) = monsters
                            .records
                            .iter()
                            .find(|row| monsters.index(row) == self.monster)
                        {
                            self.gate_image = monsters.int_or(monster, "Image", 540) as i32;
                        }
                    }
                }
            }
        }
    }
    fn request(&self, selection: Selection) -> Request {
        let (gate, castle, monster, gate_image) = match selection {
            Selection::Castle(id) => (None, Some(id), Some(self.monster), Some(self.gate_image)),
            Selection::Gate(id) => (Some(id), None, None, None),
        };
        Request {
            gate,
            castle,
            monster,
            gate_image,
            x: self.x,
            y: self.y,
            repair_cost: self.repair_cost,
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, assets: &Path, db: &MirDb, selection: Selection) {
        let changed_selection = self.selection != Some(selection);
        self.select(db, selection);
        if changed_selection && matches!(selection, Selection::Gate(_)) {
            match castle_gate_authoring::preview(db, assets, &self.request(selection)) {
                Ok(preview) => self.preview = Some(preview),
                Err(error) => self.status = format!("{error:#}"),
            }
        }
        egui::CollapsingHeader::new("Castle gate placement").default_open(true).show(ui,|ui| {
            let create=matches!(selection,Selection::Castle(_));
            ui.label(if create {"Place a gate using an existing stock gate appearance."} else {"Relocate this gate; preserve its castle, monster and repair cost."});
            let before=(self.x,self.y,self.monster,self.gate_image,self.repair_cost);
            ui.horizontal(|ui| { ui.label("Map X");ui.add(egui::DragValue::new(&mut self.x).range(0..=i32::MAX));ui.label("Map Y");ui.add(egui::DragValue::new(&mut self.y).range(0..=i32::MAX)); });
            ui.add_enabled_ui(create,|ui| {
                ui.horizontal(|ui| {ui.label("Source monster ID");ui.add(egui::DragValue::new(&mut self.monster).range(1..=i32::MAX));});
                egui::ComboBox::from_label("Gate orientation").selected_text(format!("Image {}",self.gate_image)).show_ui(ui,|ui| {
                    for (image,label) in [(540,"540 — six cells, face 2"),(541,"541 — one cell, face 0"),(542,"542 — six cells, face 3"),(543,"543 — six cells, face 1")] {ui.selectable_value(&mut self.gate_image,image,label);}
                });
                ui.horizontal(|ui| {ui.label("Repair cost");ui.add(egui::DragValue::new(&mut self.repair_cost).range(0..=i32::MAX));});
            });
            if before!=(self.x,self.y,self.monster,self.gate_image,self.repair_cost) {self.preview=None;self.status.clear();}
            if ui.button("Preview closed gate footprint").clicked() {
                match castle_gate_authoring::preview(db,assets,&self.request(selection)) {
                    Ok(preview)=>{self.preview=Some(preview);self.status.clear();},
                    Err(error)=>{self.preview=None;self.status=format!("{error:#}");},
                }
            }
            if let Some(preview)=&self.preview {
                ui.label(format!("Map {} ({}) · Image {} · {} blocked cells",preview.file,preview.map,preview.image,preview.closed_cells.len()));
                footprint(ui,preview);
                if preview.clones_monster {ui.small("Creates a separate gate monster definition; existing monsters stay unchanged.");}
            }
            ui.horizontal(|ui| {ui.label("New private database");ui.text_edit_singleline(&mut self.output);});
            ui.small("Choose a new .db filename in an existing folder. Open that snapshot to continue editing.");
            if ui.add_enabled(self.preview.is_some(),egui::Button::new("Save new gate database")).clicked() {
                let result=(||->Result<Preview>{
                    let mut snapshot=MirDb { collections: db.collections.clone() };
                    let preview=castle_gate_authoring::apply(&mut snapshot,assets,&self.request(selection))?;
                    save_new(&snapshot,assets,Path::new(self.output.trim()))?;
                    Ok(preview)
                })();
                match result {
                    Ok(preview)=> {self.status=format!("Saved gate {} to {}",preview.gate.unwrap_or_default(),self.output);self.preview=Some(preview);},
                    Err(error)=>self.status=format!("{error:#}"),
                }
            }
            if !self.status.is_empty() {ui.label(&self.status);}
        });
    }
}
fn footprint(ui: &mut egui::Ui, preview: &Preview) {
    let size = egui::vec2(175.0, 175.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    for row in 0..7 {
        for column in 0..7 {
            let x = i64::from(preview.x) + column - 3;
            let y = i64::from(preview.y) + row - 3;
            let closed = preview
                .closed_cells
                .iter()
                .any(|&(px, py)| i64::from(px) == x && i64::from(py) == y);
            let cell = egui::Rect::from_min_size(
                rect.min + egui::vec2(column as f32 * 25.0, row as f32 * 25.0),
                egui::vec2(23.0, 23.0),
            );
            ui.painter().rect_filled(
                cell,
                1.0,
                if closed {
                    egui::Color32::from_rgb(180, 65, 45)
                } else {
                    egui::Color32::from_gray(55)
                },
            );
            if column == 3 && row == 3 {
                ui.painter().text(
                    cell.center(),
                    egui::Align2::CENTER_CENTER,
                    "G",
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                );
            }
        }
    }
    ui.small("Red: blocked while closed. G: gate origin. Columns increase X; rows increase Y.");
}
fn save_new(db: &MirDb, assets: &Path, output: &Path) -> Result<()> {
    ensure!(
        output
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("db")),
        "Choose a .db filename"
    );
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let assets = assets.canonicalize()?;
    ensure!(
        !parent.starts_with(&assets),
        "Save outside original client assets"
    );
    if let Some(database) = assets
        .parent()
        .map(|parent| parent.join("Database"))
        .and_then(|path| path.canonicalize().ok())
    {
        ensure!(
            !parent.starts_with(database),
            "Save outside original database directory"
        );
    }
    let output = parent.join(output.file_name().context("Missing database filename")?);
    let bytes = db.to_bytes()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .context("Output must be a new writable filename")?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&output);
        return Err(error.into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_snapshot_save_is_exclusive_and_refuses_source_directories() {
        let root = std::env::temp_dir().join(format!("zircon-gate-ui-{}", std::process::id()));
        let assets = root.join("Client");
        let database = root.join("Database");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::create_dir_all(&database).unwrap();
        let db = MirDb {
            collections: vec![],
        };
        let output = root.join("draft.db");
        save_new(&db, &assets, &output).unwrap();
        let original = std::fs::read(&output).unwrap();
        assert!(save_new(&db, &assets, &output).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), original);
        assert!(save_new(&db, &assets, &assets.join("bad.db")).is_err());
        assert!(save_new(&db, &assets, &database.join("bad.db")).is_err());
        assert!(MirDb::load(&output).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}
