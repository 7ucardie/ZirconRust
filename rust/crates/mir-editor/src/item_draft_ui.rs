//! Independent item preview state; the editor owns application and grouped undo.
use crate::item_draft::{self, Draft};
use mir_formats::mirdb::MirDb;

#[derive(Default)]
pub struct Panel {
    source: Option<i32>,
    level: i32,
    name: String,
    draft: Option<Draft>,
    error: Option<String>,
}
impl Panel {
    /// Returns a reviewed draft to apply through the editor's undo wrapper.
    pub fn show(&mut self, ui: &mut eframe::egui::Ui, db: &MirDb, source: i32) -> Option<Draft> {
        if self.source != Some(source) {
            *self = Self::default();
            self.source = Some(source);
            if let Some(c) = db.collection("ItemInfo") {
                if let Some(row) = c.records.iter().find(|r| c.index(r) == source) {
                    self.level = c.int_or(row, "RequiredAmount", 1).clamp(1, 255) as i32;
                    self.name = format!("{} Variant", c.str_or(row, "ItemName", "Item"));
                }
            }
            if std::env::var_os("ZIRCON_EDITOR_ITEM_DRAFT").is_some() {
                self.level = std::env::var("ZIRCON_EDITOR_ITEM_LEVEL")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30);
                match item_draft::propose(db, source, self.level, &self.name) {
                    Ok(draft) => self.draft = Some(draft),
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        }

        let mut apply = None;
        eframe::egui::CollapsingHeader::new("Draft equipment variant").default_open(true).show(ui, |ui| {
            ui.label("Copies this item's slot, appearance and restrictions. Automatic starting-item grants are disabled for the variant. Review estimated stats before creating.");
            ui.horizontal_wrapped(|ui| {
                ui.label("Name");
                let name_changed = ui.text_edit_singleline(&mut self.name).changed();
                ui.label("Level");
                let level_changed = ui.add(eframe::egui::DragValue::new(&mut self.level).range(1..=255)).changed();
                if name_changed || level_changed { self.draft = None; }
            });
            if ui.button("Preview item draft").clicked() {
                match item_draft::propose(db, source, self.level, &self.name) {
                    Ok(draft) => { self.draft = Some(draft); self.error = None; }
                    Err(error) => { self.draft = None; self.error = Some(error.to_string()); }
                }
            }
            if let Some(draft) = &self.draft {
                ui.label(format!("{} · level {} to {} · {} comparable items", draft.slot, draft.source_level, draft.target_level, draft.reference_count));
                ui.label(format!("Estimated core budget: {:.1} to {:.1}", draft.budget_before, draft.budget_after));
                ui.small(&draft.method);
                eframe::egui::Grid::new("item_draft_stats").striped(true).show(ui, |ui| {
                    ui.strong("Stat"); ui.strong("Source"); ui.strong("Draft"); ui.end_row();
                    for change in &draft.changes {
                        ui.label(format!("{} ({})", change.label, change.stat));
                        ui.label(change.before.to_string()); ui.label(change.after.to_string()); ui.end_row();
                    }
                });
                if ui.button("Create item variant (one Undo)").clicked() { apply = self.draft.take(); }
            }
            if let Some(error) = &self.error { ui.colored_label(eframe::egui::Color32::RED, error); }
        });
        apply
    }
}
