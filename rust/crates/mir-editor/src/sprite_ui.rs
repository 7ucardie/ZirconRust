//! Sprite project operations share the same authoring interface as MCP clients.
use eframe::egui;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct Source<'a> {
    pub kind: &'a str,
    pub id: i32,
    pub image: u16,
    pub armour_shape: Option<u16>,
    pub equipment_shape: Option<u16>,
    pub equipment_type: u8,
}

pub struct Panel {
    root: String,
    project: String,
    output: String,
    image: String,
    slot: usize,
    layer: String,
    fit: bool,
    binary_alpha: bool,
    tint: [u8; 3],
    trim_transparent: bool,
    new_canvas: bool,
    canvas: [u16; 2],
    name: String,
    wearable_mode: bool,
    equipment_mode: bool,
    wearable_family: String,
    wearable_projects: String,
    pose_donor: Option<u16>,
    status: String,
    preview: crate::sprite_preview_ui::Preview,
    drawing: Drawing,
    draw_padding: [u8; 4],
    motion_keys: Vec<crate::sprite_project::MotionKeyframe>,
    motion_project: String,
    motion_root: PathBuf,
    auto_strength: u8,
    open_project_on_start: bool,
}
impl Default for Panel {
    fn default() -> Self {
        let project = std::env::var("ZIRCON_EDITOR_SPRITE_PROJECT")
            .ok()
            .filter(|value| value.starts_with("project:"));
        Self {
            root: std::env::var("ZIRCON_SPRITE_PROJECT_ROOT")
                .unwrap_or_else(|_| "sprite-projects".into()),
            project: project
                .clone()
                .unwrap_or_else(|| "project:sprite-source/project.json".into()),
            output: "sprite-source".into(),
            image: String::new(),
            slot: 0,
            layer: "image".into(),
            fit: false,
            binary_alpha: false,
            tint: [255; 3],
            trim_transparent: false,
            new_canvas: false,
            canvas: [32, 32],
            name: "New sprite".into(),
            wearable_mode: false,
            equipment_mode: false,
            wearable_family: "armour-male".into(),
            wearable_projects: "project:sprite-source/project.json".into(),
            pose_donor: None,
            status: String::new(),
            preview: Default::default(),
            drawing: Default::default(),
            draw_padding: [0; 4],
            motion_keys: Vec::new(),
            motion_project: String::new(),
            motion_root: PathBuf::new(),
            auto_strength: 3,
            open_project_on_start: project.is_some(),
        }
    }
}
impl Panel {
    pub fn show(&mut self, ui: &mut egui::Ui, assets: &Path, database: &Path, source: Source<'_>) {
        let Source {
            kind,
            id: source_id,
            image,
            armour_shape,
            equipment_shape,
            equipment_type,
        } = source;
        egui::CollapsingHeader::new("Original sprite authoring")
            .default_open(std::env::var_os("ZIRCON_EDITOR_SPRITE_PROJECT").is_some())
            .show(ui, |ui| {
                ui.label("Export editable PNG frames, import your artwork, and build a private game pack.");
                ui.horizontal(|ui| { ui.label("Project folder"); ui.text_edit_singleline(&mut self.root); });
                ui.horizontal(|ui| { ui.label("New revision name"); ui.text_edit_singleline(&mut self.output); });
                let mut request: Option<(&str, Value)> = None;
                if armour_shape.is_some() {
                    if ui.checkbox(&mut self.wearable_mode, "Author worn clothing").changed() && self.wearable_mode { self.equipment_mode = false; }
                } else { self.wearable_mode = false; }
                if equipment_shape.is_some() {
                    if ui.checkbox(&mut self.equipment_mode, "Author equipped weapon/helmet/shield art").changed() && self.equipment_mode { self.wearable_mode = false; }
                } else { self.equipment_mode = false; }
                if self.wearable_mode {
                    if !["armour-male", "armour-female", "armour-assassin-male", "armour-assassin-female"].contains(&self.wearable_family.as_str()) { self.wearable_family = "armour-male".into(); }
                    egui::ComboBox::from_label("Body family").selected_text(&self.wearable_family).show_ui(ui, |ui| {
                        for family in ["armour-male", "armour-female", "armour-assassin-male", "armour-assassin-female"] { ui.selectable_value(&mut self.wearable_family, family.into(), family); }
                    });
                    ui.small("Export one project for each gender/class allowed by this item. Keep body anchors and equipment alignment.");
                }
                let equipment_families: &[&str] = match equipment_type {
                    2 => &["weapon-male", "weapon-female"],
                    5 => &["helmet-male", "helmet-female", "helmet-assassin-male", "helmet-assassin-female"],
                    27 => &["shield-male", "shield-female", "shield-assassin-male", "shield-assassin-female"],
                    _ => &[],
                };
                if self.equipment_mode {
                    if !equipment_families.contains(&self.wearable_family.as_str()) { self.wearable_family = equipment_families[0].into(); }
                    egui::ComboBox::from_label("Equipment family").selected_text(&self.wearable_family).show_ui(ui, |ui| {
                        for family in equipment_families { ui.selectable_value(&mut self.wearable_family, (*family).into(), *family); }
                    });
                    ui.small("Edit complete equipped frames for each allowed gender/class, then publish all required projects together.");
                }
                let kind = if self.wearable_mode || self.equipment_mode { self.wearable_family.as_str() } else {kind};
                let image = if self.wearable_mode {armour_shape.unwrap()} else if self.equipment_mode {equipment_shape.unwrap()} else {image};

                if self.open_project_on_start {
                    self.open_project_on_start = false;
                    let loaded = (|| -> anyhow::Result<()> {
                        let relative = self.project.strip_prefix("project:").unwrap();
                        let root = Path::new(self.root.trim()).canonicalize()?;
                        let manifest = root.join(relative).canonicalize()?;
                        anyhow::ensure!(manifest.starts_with(&root), "Project escapes the selected folder");
                        self.preview.load(manifest.parent().unwrap())?;
                        self.drawing.load(ui, &self.root, &self.project, self.slot, self.draw_padding)?;
                        Ok(())
                    })();
                    self.status = loaded.map(|_| "Sprite project loaded".to_string())
                        .unwrap_or_else(|error| error.to_string());
                }

                if ui.button("Export selected sprite and reference sheet").clicked() {
                    request = Some(("sprite_export_project", json!({"kind":kind,"source_image":image,"output":self.output})));
                }
                if self.wearable_mode {
                    ui.horizontal(|ui| {
                        ui.label("Reference armour shape");
                        let donor = self.pose_donor.get_or_insert(image.saturating_add(1));
                        ui.add(egui::DragValue::new(donor).range(0..=230));
                        if ui.button("Create full-body poses from reference").clicked() {
                            request = Some(("sprite_pose_from_reference", json!({"kind":kind,"source_image":image,"donor_image":donor,"output":self.output})));
                        }
                    });
                    ui.small("Copies drawn body poses and shadows into a new editable project; paint new clothing before publishing.");
                }
                ui.horizontal(|ui| { ui.label("Editable manifest"); ui.text_edit_singleline(&mut self.project); });
                ui.horizontal(|ui| {
                    if ui.button("Validate project").clicked() { request = Some(("sprite_validate_project",json!({"project":self.project}))); }
                    if ui.button("Build sprite pack").clicked() { request = Some(("sprite_build_pack",json!({"project":self.project,"output":self.output}))); }
                });
                if ui.button("Load animation preview").clicked() {
                    self.status = (|| -> anyhow::Result<String> {
                        let relative=self.project.strip_prefix("project:").ok_or_else(||anyhow::anyhow!("Use a project: manifest path"))?;
                        let root=Path::new(self.root.trim()).canonicalize()?;
                        let manifest=root.join(relative).canonicalize()?;
                        anyhow::ensure!(manifest.starts_with(&root),"Project escapes the selected folder");
                        self.preview.load(manifest.parent().unwrap())?;
                        Ok("Animation preview loaded.".into())
                    })().unwrap_or_else(|error|error.to_string());
                }
                self.preview.show(ui);
                egui::CollapsingHeader::new("Draw on a frame")
                    .default_open(std::env::var("ZIRCON_EDITOR_SPRITE_PROJECT").is_ok_and(|value| value.starts_with("project:")))
                    .show(ui, |ui| {
                    ui.small("Paint opaque pixels, erase, or add colour behind existing art. Save as a new sprite revision.");
                    ui.horizontal(|ui| {
                        ui.label("Frame slot");
                        ui.add(egui::DragValue::new(&mut self.slot).range(0..=4999));
                        if let Some(slot) = self.preview.selected_slot() {
                            if ui.button("Use preview frame").clicked() {
                                self.slot = slot;
                            }
                        }
                        if ui.button("Load pixel canvas").clicked() {
                            self.drawing.status = self
                                .drawing
                                .load(ui, &self.root, &self.project, self.slot, self.draw_padding)
                                .map(|_| "Pixel canvas loaded".to_string())
                                .unwrap_or_else(|error| error.to_string());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Extra pixels: left / top / right / bottom");
                        for value in &mut self.draw_padding {
                            ui.add(egui::DragValue::new(value).range(0..=64));
                        }
                    });
                    let (save_pixels, capture_pose) = self.drawing.show(ui);
                    if save_pixels {
                        let same_root = Path::new(self.root.trim())
                            .canonicalize()
                            .is_ok_and(|root| root == self.drawing.root);
                        if same_root {
                            request = Some(("sprite_paint", json!({
                                "project":self.drawing.project_ref,
                                "output":self.output,
                                "padding":self.drawing.padding,
                                "polygons":self.drawing.edits,
                            })));
                        } else {
                            self.drawing.status = "Reload pixel canvas after changing project folder".into();
                        }
                    }
                    if capture_pose {
                        if let Some(pose) = self.drawing.pose() {
                            if self.motion_project != self.drawing.project_ref
                                || self.motion_root != self.drawing.root {
                                self.motion_keys.clear();
                                self.motion_project = self.drawing.project_ref.clone();
                                self.motion_root = self.drawing.root.clone();
                            }
                            self.motion_keys.retain(|key| key.slot != pose.slot);
                            self.motion_keys.push(pose);
                        }
                    }
                    let run = self.preview.current_run_slots();
                    let keys: Vec<_> = run.iter().filter_map(|slot| self.motion_keys.iter().find(|key| key.slot == *slot).cloned()).collect();
                    if !run.is_empty() {
                        ui.horizontal(|ui| {
                            ui.small(format!("Current animation: {} frames · {} keyframes kept", run.len(), keys.len()));
                            if ui.button("Clear motion keyframes").clicked() {
                                self.motion_keys.clear();
                                self.motion_project.clear();
                                self.motion_root.clear();
                            }
                        });
                        let ready = keys.len() >= 2
                            && keys.first().is_some_and(|key| key.slot == run[0])
                            && keys.last().is_some_and(|key| key.slot == *run.last().unwrap());
                        if ui.add_enabled(ready, egui::Button::new("Animate polygon into new revision")).clicked() {
                            let same_root = Path::new(self.root.trim())
                                .canonicalize()
                                .is_ok_and(|root| root == self.drawing.root);
                            let same_preview = self.preview.loaded_project()
                                == Some(self.drawing.project_dir.as_path());
                            if same_root
                                && self.motion_root == self.drawing.root
                                && self.motion_project == self.project
                                && same_preview {
                                request = Some(("sprite_paint_motion", json!({
                                    "project":self.motion_project,
                                    "output":self.output,
                                    "padding":self.draw_padding,
                                    "slots":run,
                                    "keyframes":keys,
                                })));
                            } else {
                                self.drawing.status = "Reload this project before animating keyframes".into();
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut self.auto_strength, 1..=12).text("Cloth motion"));
                            if ui.add_enabled(run.len() >= 2 && self.drawing.pose().is_some_and(|pose| run.contains(&pose.slot)),
                                egui::Button::new("Design motion from drawn polygon")).clicked() {
                                let same_root = Path::new(self.root.trim())
                                    .canonicalize()
                                    .is_ok_and(|root| root == self.drawing.root);
                                let same_preview = self.preview.loaded_project()
                                    == Some(self.drawing.project_dir.as_path());
                                if same_root && same_preview && self.drawing.project_ref == self.project {
                                    request = Some(("sprite_auto_motion", json!({
                                        "project":self.project,
                                        "output":self.output,
                                        "padding":self.draw_padding,
                                        "slots":run,
                                        "seed":self.drawing.pose().unwrap(),
                                        "strength":self.auto_strength,
                                    })));
                                } else {
                                    self.drawing.status = "Reload this project before designing motion".into();
                                }
                            }
                        });
                        ui.small("One polygon seeds attached cloth motion; preview action chooses sway, lunge, or recoil.");
                    }
                });
                ui.collapsing("Colour variant across all frames", |ui| {
                    ui.color_edit_button_srgb(&mut self.tint);
                    ui.small("RGB multipliers; white keeps original colours. Frame size, anchors and alpha stay unchanged.");
                    if ui.button("Tint image layer into new revision").clicked() {
                        request = Some(("sprite_tint", json!({"project":self.project,"layer":"image","rgb":self.tint,"output":self.output})));
                    }
                });
                ui.collapsing("Import a PNG frame", |ui| {
                    ui.label("PNG must be inside the project folder. Use project:folder/image.png.");
                    ui.text_edit_singleline(&mut self.image);
                    ui.horizontal(|ui| {
                        ui.label("Frame slot"); ui.add(egui::DragValue::new(&mut self.slot).range(0..=4999));
                        for layer in ["image", "shadow", "overlay"] { ui.selectable_value(&mut self.layer,layer.into(),layer); }
                    });
                    ui.checkbox(&mut self.fit,"Fit artwork inside the existing frame canvas");
                    ui.add_enabled(self.fit && kind=="item",egui::Checkbox::new(&mut self.trim_transparent,"Fit visible artwork, ignoring transparent margins"));
                    if !self.fit || kind!="item" {self.trim_transparent=false;}
                    if kind=="item" && self.layer=="image" {
                        ui.checkbox(&mut self.new_canvas,"Choose a new icon canvas");
                        if self.new_canvas {ui.horizontal(|ui|{ui.label("Pixels");ui.add(egui::DragValue::new(&mut self.canvas[0]).range(8..=128));ui.add(egui::DragValue::new(&mut self.canvas[1]).range(8..=128));});}
                    }
                    ui.checkbox(&mut self.binary_alpha,"Convert alpha to transparent/opaque for DXT1");
                    if ui.button("Import into a new revision").clicked() {
                        let mut arguments=json!({"project":self.project,"image":self.image,"slot":self.slot,"layer":self.layer,"fit":self.fit,"binary_alpha":self.binary_alpha,"trim_transparent":self.trim_transparent,"output":self.output});
                        if self.new_canvas && kind=="item" && self.layer=="image" { arguments["canvas"]=json!(self.canvas); arguments["offsets"]=json!([-(self.canvas[0] as i16)/2,-(self.canvas[1] as i16)/2]); }
                        request=Some(("sprite_import_image",arguments));
                    }
                });
                ui.collapsing("Create game content", |ui| {
                    ui.label("Uses the saved database and allocates a separate appearance for the new record.");
                    if kind=="item" {ui.small("Changes inventory and ground icons. Use equipped art mode for a new worn appearance.");}
                    ui.horizontal(|ui| { ui.label("New name"); ui.text_edit_singleline(&mut self.name); });
                    if self.wearable_mode {
                        ui.label("Required body projects (one project: manifest path per line)");
                        ui.text_edit_multiline(&mut self.wearable_projects);
                        if ui.button("Publish complete wearable outfit").clicked() {
                            let projects: Vec<_> = self.wearable_projects.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
                            request = Some(("wearable_publish",json!({"projects":projects,"source_id":source_id,"name":self.name,"output":self.output})));
                        }
                    } else if self.equipment_mode {
                        ui.label("Required equipment projects (one project: manifest path per line)");
                        ui.text_edit_multiline(&mut self.wearable_projects);
                        if ui.button("Publish equipped artwork").clicked() {
                            let projects: Vec<_> = self.wearable_projects.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
                            request = Some(("equipment_publish",json!({"projects":projects,"source_id":source_id,"name":self.name,"output":self.output})));
                        }
                    } else if ui.button("Build private assets and new database record").clicked() {
                        request = Some(("sprite_publish",json!({"project":self.project,"kind":kind,"source_id":source_id,"name":self.name,"output":self.output})));
                    }
                });
                if let Some((tool, arguments)) = request {
                    self.status = match crate::mcp_authoring::Workspace::open(Path::new(self.root.trim()), assets,database)
                        .and_then(|workspace|workspace.call(tool,arguments)) {
                        Ok(result) => format!("Completed. {}{}", result.get("output").or_else(||result.get("directory")).and_then(Value::as_str).unwrap_or("Project validated."), result.get("changed_slots").map(|n|format!(" — {n} changed frames")).unwrap_or_default()),
                        Err(error) => format!("{error:#}"),
                    };
                }
                if !self.status.is_empty() { ui.label(&self.status); }
            });
    }
}

struct Drawing {
    project_ref: String,
    root: PathBuf,
    project_dir: PathBuf,
    slot: usize,
    dimensions: [u16; 2],
    padding: [u8; 4],
    source: Vec<u8>,
    texture: Option<egui::TextureHandle>,
    edits: Vec<crate::sprite_project::PaintPolygon>,
    color: [u8; 3],
    under: bool,
    erase: bool,
    zoom: u8,
    last: Option<(i32, i32)>,
    pose_mode: bool,
    pose_points: Vec<[i16; 2]>,
    status: String,
}
impl Default for Drawing {
    fn default() -> Self {
        Self {
            project_ref: String::new(),
            root: PathBuf::new(),
            project_dir: PathBuf::new(),
            slot: 0,
            dimensions: [0, 0],
            padding: [0; 4],
            source: Vec::new(),
            texture: None,
            edits: Vec::new(),
            color: [255, 80, 80],
            under: false,
            erase: false,
            zoom: 6,
            last: None,
            pose_mode: false,
            pose_points: Vec::new(),
            status: String::new(),
        }
    }
}
impl Drawing {
    fn load(
        &mut self,
        ui: &egui::Ui,
        root: &str,
        reference: &str,
        slot: usize,
        padding: [u8; 4],
    ) -> anyhow::Result<()> {
        let relative = reference
            .strip_prefix("project:")
            .ok_or_else(|| anyhow::anyhow!("Use a project: manifest path"))?;
        let root = Path::new(root.trim()).canonicalize()?;
        let manifest_path = root.join(relative).canonicalize()?;
        anyhow::ensure!(
            manifest_path.starts_with(&root)
                && manifest_path
                    .file_name()
                    .is_some_and(|name| name == "project.json"),
            "Project escapes the selected folder or has no project.json"
        );
        let project = manifest_path.parent().unwrap();
        crate::sprite_project::validate(project)?;
        let manifest = crate::sprite_project::load_manifest(project)?;
        let image = manifest
            .slots
            .get(slot)
            .and_then(|frame| frame.image.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Frame slot has no image canvas"))?;
        let path = project.join(&image.path).canonicalize()?;
        anyhow::ensure!(path.starts_with(project), "Image escapes project");
        let (width, height, source) = crate::sprite_project::read_png(&path)?;
        let canvas_width = usize::from(width) + usize::from(padding[0]) + usize::from(padding[2]);
        let canvas_height = usize::from(height) + usize::from(padding[1]) + usize::from(padding[3]);
        anyhow::ensure!(
            canvas_width <= 1024 && canvas_height <= 1024,
            "Drawing canvas supports frames up to 1024 × 1024; use PNG import for larger art"
        );
        let mut padded = vec![0; canvas_width * canvas_height * 4];
        for y in 0..usize::from(height) {
            let from = y * usize::from(width) * 4;
            let to = ((y + usize::from(padding[1])) * canvas_width + usize::from(padding[0])) * 4;
            padded[to..to + usize::from(width) * 4]
                .copy_from_slice(&source[from..from + usize::from(width) * 4]);
        }
        let texture = ui.ctx().load_texture(
            "sprite-drawing-frame",
            egui::ColorImage::from_rgba_unmultiplied([canvas_width, canvas_height], &padded),
            egui::TextureOptions::NEAREST,
        );
        self.project_ref = reference.into();
        self.root = root;
        self.project_dir = project.to_path_buf();
        self.slot = slot;
        self.dimensions = [canvas_width as u16, canvas_height as u16];
        self.padding = padding;
        self.source = padded;
        self.texture = Some(texture);
        self.edits.clear();
        self.last = None;
        self.pose_points.clear();
        self.status.clear();
        Ok(())
    }

    fn refresh(&mut self) {
        let [width, height] = self.dimensions;
        let mut rgba = self.source.clone();
        for polygon in &self.edits {
            crate::sprite_project::fill_polygon(
                &mut rgba,
                usize::from(width),
                usize::from(height),
                polygon,
                i32::from(self.padding[0]),
                i32::from(self.padding[1]),
            );
        }
        if let Some(texture) = &mut self.texture {
            texture.set(
                egui::ColorImage::from_rgba_unmultiplied(
                    [usize::from(width), usize::from(height)],
                    &rgba,
                ),
                egui::TextureOptions::NEAREST,
            );
        }
    }

    fn dab(&mut self, x: i32, y: i32) -> bool {
        let [width, height] = self.dimensions;
        if x < 0 || y < 0 || x >= i32::from(width) || y >= i32::from(height) {
            return false;
        }
        if self.edits.len() == 4096 {
            self.status =
                "4096 pixel edits reached; apply this revision before drawing more".into();
            return false;
        }
        self.edits.push(crate::sprite_project::PaintPolygon {
            slot: self.slot,
            points: vec![
                [
                    (x - i32::from(self.padding[0])) as i16,
                    (y - i32::from(self.padding[1])) as i16,
                ],
                [
                    (x + 1 - i32::from(self.padding[0])) as i16,
                    (y - i32::from(self.padding[1])) as i16,
                ],
                [
                    (x + 1 - i32::from(self.padding[0])) as i16,
                    (y + 1 - i32::from(self.padding[1])) as i16,
                ],
                [
                    (x - i32::from(self.padding[0])) as i16,
                    (y + 1 - i32::from(self.padding[1])) as i16,
                ],
            ],
            color: if self.erase {
                [0; 4]
            } else {
                [self.color[0], self.color[1], self.color[2], 255]
            },
            under: !self.erase && self.under,
        });
        true
    }

    fn pose(&self) -> Option<crate::sprite_project::MotionKeyframe> {
        (self.pose_points.len() >= 3).then(|| crate::sprite_project::MotionKeyframe {
            slot: self.slot,
            points: self.pose_points.clone(),
            color: if self.erase {
                [0; 4]
            } else {
                [self.color[0], self.color[1], self.color[2], 255]
            },
            under: !self.erase && self.under,
        })
    }

    fn show(&mut self, ui: &mut egui::Ui) -> (bool, bool) {
        let Some(texture) = self.texture.as_ref().map(|handle| handle.id()) else {
            return (false, false);
        };
        ui.horizontal(|ui| {
            ui.color_edit_button_srgb(&mut self.color);
            ui.checkbox(&mut self.erase, "Erase");
            ui.add_enabled(
                !self.erase,
                egui::Checkbox::new(&mut self.under, "Behind existing art"),
            );
            ui.add(egui::Slider::new(&mut self.zoom, 2..=12).text("Zoom"));
        });
        ui.checkbox(
            &mut self.pose_mode,
            "Polygon keyframe: click vertices on canvas",
        );
        ui.small(format!(
            "Frame {} · {} × {} pixels · {} staged pixel edits. Drag to draw.",
            self.slot,
            self.dimensions[0],
            self.dimensions[1],
            self.edits.len()
        ));
        let mut changed = false;
        egui::ScrollArea::both().max_height(420.0).show(ui, |ui| {
            let zoom = f32::from(self.zoom);
            let size = egui::vec2(
                f32::from(self.dimensions[0]) * zoom,
                f32::from(self.dimensions[1]) * zoom,
            );
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_gray(55));
            ui.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            if self.pose_mode {
                let points: Vec<_> = self
                    .pose_points
                    .iter()
                    .map(|&[x, y]| {
                        rect.min
                            + egui::vec2(
                                (f32::from(x) + f32::from(self.padding[0]) + 0.5) * zoom,
                                (f32::from(y) + f32::from(self.padding[1]) + 0.5) * zoom,
                            )
                    })
                    .collect();
                for segment in points.windows(2) {
                    ui.painter()
                        .line_segment([segment[0], segment[1]], (2.0, egui::Color32::YELLOW));
                }
                for point in points {
                    ui.painter()
                        .circle_filled(point, 3.0, egui::Color32::YELLOW);
                }
            }
            if response.drag_started() {
                self.last = None;
            }
            if self.pose_mode && response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if self.pose_points.len() < 64 {
                        let x = (((pos.x - rect.min.x) / zoom).floor() as i32)
                            .clamp(0, i32::from(self.dimensions[0]) - 1);
                        let y = (((pos.y - rect.min.y) / zoom).floor() as i32)
                            .clamp(0, i32::from(self.dimensions[1]) - 1);
                        self.pose_points.push([
                            (x - i32::from(self.padding[0])) as i16,
                            (y - i32::from(self.padding[1])) as i16,
                        ]);
                    } else {
                        self.status = "Polygon supports at most 64 vertices".into();
                    }
                }
            } else if !self.pose_mode
                && (response.clicked()
                    || response.dragged_by(egui::PointerButton::Primary)
                    || response.drag_stopped_by(egui::PointerButton::Primary))
            {
                if let Some(pos) = response.interact_pointer_pos() {
                    let point = (
                        (((pos.x - rect.min.x) / zoom).floor() as i32)
                            .clamp(0, i32::from(self.dimensions[0]) - 1),
                        (((pos.y - rect.min.y) / zoom).floor() as i32)
                            .clamp(0, i32::from(self.dimensions[1]) - 1),
                    );
                    let from = self.last.unwrap_or(point);
                    for (index, (x, y)) in
                        crate::tile_doc::line(from, point).into_iter().enumerate()
                    {
                        if self.last.is_some() && index == 0 {
                            continue;
                        }
                        changed |= self.dab(x, y);
                    }
                    self.last = Some(point);
                }
            }
            if response.drag_stopped() || !ui.input(|input| input.pointer.primary_down()) {
                self.last = None;
            }
        });
        if changed {
            self.refresh();
        }
        let mut apply = false;
        ui.horizontal(|ui| {
            if ui.button("Discard staged pixels").clicked() {
                self.edits.clear();
                self.refresh();
            }
            apply = ui
                .add_enabled(
                    !self.edits.is_empty(),
                    egui::Button::new("Save drawing as new revision"),
                )
                .clicked();
        });
        let mut capture = false;
        if self.pose_mode {
            ui.horizontal(|ui| {
                if ui.button("Undo vertex").clicked() {
                    self.pose_points.pop();
                }
                if ui.button("Clear polygon").clicked() {
                    self.pose_points.clear();
                }
                capture = ui
                    .add_enabled(
                        self.pose_points.len() >= 3,
                        egui::Button::new("Keep motion keyframe"),
                    )
                    .clicked();
            });
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
        (apply, capture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_brush_maps_padding_and_preserves_layer_order() {
        let mut drawing = Drawing {
            dimensions: [5, 5],
            padding: [1; 4],
            slot: 7,
            color: [200, 40, 20],
            under: true,
            source: vec![0; 5 * 5 * 4],
            ..Drawing::default()
        };
        drawing.source[(2 * 5 + 2) * 4..(2 * 5 + 3) * 4].copy_from_slice(&[10, 20, 30, 255]);
        assert!(drawing.dab(0, 0));
        assert_eq!(drawing.edits[0].points[0], [-1, -1]);
        assert!(drawing.dab(2, 2));
        drawing.erase = true;
        assert!(drawing.dab(2, 2));
        assert!(!drawing.dab(5, 2));
        let mut result = drawing.source.clone();
        for edit in &drawing.edits {
            assert_eq!(edit.slot, 7);
            crate::sprite_project::fill_polygon(&mut result, 5, 5, edit, 1, 1);
        }
        assert_eq!(&result[..4], &[200, 40, 20, 255]);
        assert_eq!(&result[(2 * 5 + 2) * 4..(2 * 5 + 3) * 4], &[0; 4]);
        drawing.erase = false;
        drawing.pose_points = vec![[-1, -1], [2, -1], [1, 3]];
        let pose = drawing.pose().unwrap();
        assert_eq!(pose.slot, 7);
        assert_eq!(pose.points, drawing.pose_points);
        assert_eq!(pose.color, [200, 40, 20, 255]);
        assert!(pose.under);
    }
}
