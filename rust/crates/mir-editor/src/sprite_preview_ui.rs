//! Animation review uses the same frame contract as the running client.
use crate::sprite_project::{self, Manifest};
use eframe::egui;
use mir_formats::zl::{SurfaceKind, ZlLibrary};
use std::path::{Path, PathBuf};

pub struct Preview {
    project: PathBuf,
    manifest: Option<Manifest>,
    original: Option<ZlLibrary>,
    action: usize,
    actions: Vec<String>,
    frames: Vec<crate::sprite_contract::RequiredFrame>,
    direction: u8,
    playing: bool,
    frame: u32,
    source: bool,
    onion: bool,
    texture: Option<egui::TextureHandle>,
    key: Option<(usize, bool)>,
    ghost: Option<egui::TextureHandle>,
    ghost_key: Option<(usize, bool)>,
    ghost_dimensions: [u16; 2],
    ghost_offsets: [i16; 2],
    visible_slot: Option<usize>,
    dimensions: [u16; 2],
    offsets: [i16; 2],
    status: String,
}
impl Default for Preview {
    fn default() -> Self {
        Self {
            project: PathBuf::new(),
            manifest: None,
            original: None,
            action: 0,
            actions: Vec::new(),
            frames: Vec::new(),
            direction: 0,
            playing: true,
            frame: 0,
            source: false,
            onion: false,
            texture: None,
            key: None,
            ghost: None,
            ghost_key: None,
            ghost_dimensions: [0, 0],
            ghost_offsets: [0, 0],
            visible_slot: None,
            dimensions: [0, 0],
            offsets: [0, 0],
            status: String::new(),
        }
    }
}
impl Preview {
    pub fn load(&mut self, project: &Path) -> anyhow::Result<()> {
        sprite_project::validate(project)?;
        let project = project.canonicalize()?;
        let manifest = sprite_project::load_manifest(&project)?;
        let original = ZlLibrary::open(project.join("source.zl"))?;
        let path = project.join("contract.json").canonicalize()?;
        anyhow::ensure!(
            path.starts_with(&project) && std::fs::metadata(&path)?.len() <= 16 * 1024 * 1024,
            "Invalid animation contract path or size"
        );
        let contract: crate::sprite_contract::Contract =
            serde_json::from_slice(&std::fs::read(path)?)?;
        anyhow::ensure!(
            !contract.frames.is_empty()
                && contract.frames.len() <= 2048
                && contract.frames.iter().all(|f| f.direction < 8
                    && f.index >= manifest.source_start
                    && f.index - manifest.source_start < manifest.slots.len()),
            "Invalid preview frame mapping"
        );
        self.project = project;
        self.manifest = Some(manifest);
        self.original = Some(original);
        self.actions.clear();
        for frame in &contract.frames {
            if !self.actions.contains(&frame.action) {
                self.actions.push(frame.action.clone());
            }
        }
        self.frames = contract.frames;
        self.action = 0;
        self.direction = 0;
        self.frame = 0;

        self.key = None;
        self.texture = None;
        self.ghost_key = None;
        self.ghost = None;
        self.visible_slot = None;
        self.status.clear();
        Ok(())
    }
    pub fn selected_slot(&self) -> Option<usize> {
        self.visible_slot
    }

    pub fn loaded_project(&self) -> Option<&Path> {
        self.manifest.as_ref().map(|_| self.project.as_path())
    }

    pub fn current_run_slots(&self) -> Vec<usize> {
        let (Some(manifest), Some(action)) =
            (self.manifest.as_ref(), self.actions.get(self.action))
        else {
            return vec![];
        };
        self.frames
            .iter()
            .filter(|frame| frame.action == *action && frame.direction == self.direction)
            .map(|frame| frame.index - manifest.source_start)
            .collect()
    }

    fn frame_pixels(
        &self,
        manifest: &Manifest,
        slot: usize,
    ) -> anyhow::Result<(u16, u16, Vec<u8>, [i16; 2])> {
        if self.source {
            let lib = self.original.as_ref().unwrap();
            let info = lib
                .info(slot)
                .ok_or_else(|| anyhow::anyhow!("Empty reference slot"))?;
            let decoded = lib
                .decode(slot, SurfaceKind::Image)?
                .ok_or_else(|| anyhow::anyhow!("No reference image"))?;
            let mut pixels = Vec::new();
            for y in 0..usize::from(info.height) {
                let start = y * decoded.width as usize * 4;
                pixels.extend_from_slice(&decoded.rgba[start..start + usize::from(info.width) * 4]);
            }
            Ok((
                info.width,
                info.height,
                pixels,
                [info.offset_x, info.offset_y],
            ))
        } else {
            let frame = manifest
                .slots
                .get(slot)
                .ok_or_else(|| anyhow::anyhow!("Slot outside project"))?;
            let surface = frame
                .image
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No authored image in this slot"))?;
            let path = self.project.join(&surface.path).canonicalize()?;
            anyhow::ensure!(path.starts_with(&self.project), "Image escapes project");
            let (w, h, pixels) = sprite_project::read_png(&path)?;
            Ok((w, h, pixels, [frame.offset_x, frame.offset_y]))
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let Some(manifest) = self.manifest.as_ref() else {
            return;
        };
        let animated = self.frames.len() > 1;
        if animated {
            egui::ComboBox::from_label("Animation")
                .selected_text(&self.actions[self.action])
                .show_ui(ui, |ui| {
                    for (index, action) in self.actions.iter().enumerate() {
                        ui.selectable_value(&mut self.action, index, action);
                    }
                });
            ui.add(egui::Slider::new(&mut self.direction, 0..=7).text("Direction"));
            ui.checkbox(&mut self.playing, "Play animation");
            ui.checkbox(&mut self.onion, "Show previous frame guide");
        }
        ui.checkbox(&mut self.source, "Show original reference");
        let frames: Vec<_> = self
            .frames
            .iter()
            .filter(|f| {
                f.action == self.actions[self.action]
                    && f.direction == if animated { self.direction } else { 0 }
            })
            .collect();
        if frames.is_empty() {
            self.visible_slot = None;
            ui.label("No frames for this action/direction");
            return;
        }
        let count = frames.len() as u32;
        let index = if self.playing {
            ((ui.input(|i| i.time) * 1000.0 / frames[0].interval_ms.max(1) as f64) as u32) % count
        } else {
            self.frame.min(count - 1)
        };
        if animated && !self.playing {
            ui.add(egui::Slider::new(&mut self.frame, 0..=count - 1).text("Frame"));
        }
        let slot = frames[index as usize].index - manifest.source_start;
        self.visible_slot = Some(slot);
        let previous = (self.onion && index > 0)
            .then(|| frames[index as usize - 1].index - manifest.source_start);
        if let Some(previous) = previous {
            if self.ghost_key != Some((previous, self.source)) {
                self.ghost_key = Some((previous, self.source));
                self.ghost = None;
                if let Ok((w, h, pixels, offsets)) = self.frame_pixels(manifest, previous) {
                    self.ghost_dimensions = [w, h];
                    self.ghost_offsets = offsets;
                    self.ghost = Some(ui.ctx().load_texture(
                        "sprite-previous-frame",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [usize::from(w), usize::from(h)],
                            &pixels,
                        ),
                        egui::TextureOptions::NEAREST,
                    ));
                }
            }
        } else {
            self.ghost_key = None;
            self.ghost = None;
        }
        if self.key != Some((slot, self.source)) {
            self.key = Some((slot, self.source));
            self.texture = None;
            let loaded = self.frame_pixels(manifest, slot);
            match loaded {
                Ok((w, h, pixels, offsets)) => {
                    self.dimensions = [w, h];
                    self.offsets = offsets;
                    self.texture = Some(ui.ctx().load_texture(
                        "sprite-authoring-frame",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [usize::from(w), usize::from(h)],
                            &pixels,
                        ),
                        egui::TextureOptions::NEAREST,
                    ));
                    self.status.clear();
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(280.0, 230.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_gray(28));
        let anchor = egui::pos2(rect.center().x, rect.bottom() - 45.0);
        let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(65));
        for dx in -4..=4 {
            let x = anchor.x + dx as f32 * 24.0;
            ui.painter().line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                stroke,
            );
        }
        for dy in -8..=1 {
            let y = anchor.y + dy as f32 * 24.0;
            if y >= rect.top() {
                ui.painter().line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    stroke,
                );
            }
        }
        if let Some(ghost) = &self.ghost {
            let pos = anchor
                + egui::vec2(
                    f32::from(self.ghost_offsets[0]),
                    f32::from(self.ghost_offsets[1]),
                );
            let image_rect = egui::Rect::from_min_size(
                pos,
                egui::vec2(
                    f32::from(self.ghost_dimensions[0]),
                    f32::from(self.ghost_dimensions[1]),
                ),
            );
            ui.painter().with_clip_rect(rect).image(
                ghost.id(),
                image_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::from_white_alpha(85),
            );
        }
        if let Some(texture) = &self.texture {
            let pos = anchor + egui::vec2(f32::from(self.offsets[0]), f32::from(self.offsets[1]));
            let image_rect = egui::Rect::from_min_size(
                pos,
                egui::vec2(f32::from(self.dimensions[0]), f32::from(self.dimensions[1])),
            );
            ui.painter().with_clip_rect(rect).image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        ui.painter().circle_stroke(
            anchor,
            4.0,
            egui::Stroke::new(1.0, egui::Color32::LIGHT_GREEN),
        );
        ui.small(format!(
            "Slot {slot} · {} × {} · ground anchor {}, {}",
            self.dimensions[0], self.dimensions[1], self.offsets[0], self.offsets[1]
        ));
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
        if self.playing && animated {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(
                    frames[0].interval_ms.max(1),
                ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_contract_does_not_leave_a_partially_loaded_preview() {
        let root =
            std::env::temp_dir().join(format!("zircon-preview-invalid-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let mut builder = mir_formats::zl::ZlBuilder::new(1);
        builder.push_raw(None);
        let source = root.join("source.zl");
        std::fs::write(&source, builder.to_bytes()).unwrap();
        let project = root.join("project");
        sprite_project::export_range(&source, 0, 1, &project).unwrap();
        std::fs::write(project.join("contract.json"), "{}").unwrap();
        let mut preview = Preview::default();
        assert!(preview.load(&project).is_err());
        assert!(preview.manifest.is_none());
        assert!(preview.original.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
