//! Reusable authored map pieces. Assets remain references, never cropped pixels.
use crate::{
    cave_art,
    generate::Layout,
    piece_ports::{self, Port, Side},
    tile_doc::{Document, Stamp},
};
use eframe::egui;
use mir_formats::map::MapFile;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

const MAX_BYTES: u64 = 512 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Wall,
    Corner,
    Doorway,
    Formation,
}
impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Wall => "Wall",
            Self::Corner => "Corner",
            Self::Doorway => "Doorway",
            Self::Formation => "Formation",
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    name: String,
    kind: Kind,
    map: Vec<u8>,
    #[serde(default)]
    ports: Vec<Port>,
    #[serde(default)]
    coverage: Option<Vec<bool>>,
}
#[derive(Clone)]
pub struct Piece {
    pub name: String,
    pub kind: Kind,
    map: MapFile,
    pub extent: (i32, i32, i32, i32),
    stamp: Stamp,
    pub ports: Vec<Port>,
    coverage: Option<Vec<bool>>,
}
impl Piece {
    pub(crate) fn map(&self) -> &MapFile {
        &self.map
    }
    pub(crate) fn coverage(&self) -> Option<&[bool]> {
        self.coverage.as_deref()
    }
    pub(crate) fn cells(&self) -> &[(i32, i32, mir_formats::map::Cell)] {
        &self.stamp.cells
    }
    fn from_map(name: String, kind: Kind, map: MapFile, assets: &Path) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !name.trim().is_empty() && name.chars().count() <= 80,
            "Give the piece a name of 1–80 characters."
        );
        anyhow::ensure!(
            (2..=64).contains(&map.width)
                && (2..=64).contains(&map.height)
                && map.width.is_multiple_of(2)
                && map.height.is_multiple_of(2),
            "Pieces must be even-sized, between 2 and 64 cells per side."
        );
        let extent = cave_art::source_extent(&map, assets)?;
        let stamp = Stamp::capture(
            &map,
            (0, 0),
            (i32::from(map.width) - 1, i32::from(map.height) - 1),
        );
        Ok(Self {
            name: name.trim().into(),
            kind,
            map,
            extent,
            stamp,
            ports: vec![],
            coverage: None,
        })
    }
    pub fn capture(stamp: &Stamp, name: String, kind: Kind, assets: &Path) -> anyhow::Result<Self> {
        Self::capture_masked(stamp, name, kind, assets, None)
    }
    pub fn capture_covered(
        stamp: &Stamp,
        name: String,
        kind: Kind,
        assets: &Path,
        coverage: Vec<bool>,
    ) -> anyhow::Result<Self> {
        Self::capture_masked(stamp, name, kind, assets, Some(coverage))
    }
    fn capture_masked(
        stamp: &Stamp,
        name: String,
        kind: Kind,
        assets: &Path,
        coverage: Option<Vec<bool>>,
    ) -> anyhow::Result<Self> {
        let w = stamp.cells.iter().map(|c| c.0 + 1).max().unwrap_or(0);
        let h = stamp.cells.iter().map(|c| c.1 + 1).max().unwrap_or(0);
        anyhow::ensure!(
            (2..=64).contains(&w) && (2..=64).contains(&h) && w % 2 == 0 && h % 2 == 0,
            "Copy an even-sized area of 2–64 cells per side first."
        );
        let mut map = MapFile::blank(w as u16, h as u16);
        let mut seen = std::collections::BTreeSet::new();
        for &(x, y, c) in &stamp.cells {
            anyhow::ensure!(seen.insert((x, y)), "Copied area contains duplicate cells.");
            *map.cell_mut(x, y)
                .ok_or_else(|| anyhow::anyhow!("Copied cell is outside its area."))? = c;
        }
        anyhow::ensure!(
            seen.len() == w as usize * h as usize,
            "Copy a complete rectangular area."
        );
        if let Some(mask) = &coverage {
            validate_coverage(&map, mask)?;
            for y in 0..h {
                for x in 0..w {
                    if !mask[(y * w + x) as usize] {
                        *map.cell_mut(x, y).unwrap() = mir_formats::map::Cell {
                            back_file: 255,
                            ..Default::default()
                        };
                    }
                }
            }
        }
        let mut piece = Self::from_map(name, kind, map, assets)?;
        if let Some(mask) = coverage {
            piece.set_coverage(mask)?;
        }
        Ok(piece)
    }
    pub fn load(path: &Path, assets: &Path) -> anyhow::Result<Self> {
        use std::io::Read;
        let mut bytes = vec![];
        std::fs::File::open(path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "Piece file exceeds 512 KiB."
        );
        let file: File = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            matches!(file.version, 1..=3),
            "Unsupported piece version {}.",
            file.version
        );
        // Bound dimensions before parsing, which allocates the cell grid.
        anyhow::ensure!(file.map.len() >= 28, "Piece map is truncated.");
        let w = i16::from_le_bytes([file.map[22], file.map[23]]);
        let h = i16::from_le_bytes([file.map[24], file.map[25]]);
        anyhow::ensure!(
            (2..=64).contains(&w) && (2..=64).contains(&h),
            "Piece dimensions exceed 2–64 cells."
        );
        anyhow::ensure!(
            file.version >= 2 || (file.ports.is_empty() && file.coverage.is_none()),
            "Version 1 pieces cannot declare connections or coverage."
        );
        anyhow::ensure!(
            file.version >= 3 || file.ports.iter().all(|p| p.inset == 0),
            "Recessed connections require piece version 3."
        );
        let mut map = MapFile::parse(&file.map)?;
        if let Some(mask) = &file.coverage {
            validate_coverage(&map, mask)?;
            // Unowned padding never contributes artwork, collision or ports.
            let blank = mir_formats::map::Cell {
                back_file: 255,
                ..Default::default()
            };
            for x in 0..i32::from(map.width) {
                for y in 0..i32::from(map.height) {
                    if !mask[(y * i32::from(map.width) + x) as usize] {
                        *map.cell_mut(x, y).unwrap() = blank;
                    }
                }
            }
        }
        let mut piece = Self::from_map(file.name, file.kind, map, assets)?;
        if let Some(mask) = file.coverage {
            piece.set_coverage(mask)?;
        }
        piece_ports::validate_boundary(&piece.map, &file.ports, piece.coverage.as_deref())?;
        piece.ports = file.ports;
        Ok(piece)
    }
    pub fn serialized(&self, name: &str, kind: Kind) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            !name.trim().is_empty() && name.chars().count() <= 80,
            "Give the piece a name of 1–80 characters."
        );
        piece_ports::validate_boundary(&self.map, &self.ports, self.coverage.as_deref())?;
        let bytes = serde_json::to_vec(&File {
            version: 3,
            name: name.trim().into(),
            kind,
            map: self.map.to_bytes(),
            ports: self.ports.clone(),
            coverage: self.coverage.clone(),
        })?;
        anyhow::ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "Piece file exceeds 512 KiB."
        );
        Ok(bytes)
    }
    pub fn save(&self, path: &Path, name: &str, kind: Kind) -> anyhow::Result<()> {
        let bytes = self.serialized(name, kind)?;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        Ok(())
    }
    pub fn place(
        &self,
        doc: &mut Document,
        at: (i32, i32),
        layout: Option<&Layout>,
        run: Run,
    ) -> anyhow::Result<()> {
        piece_ports::validate_boundary(&self.map, &self.ports, self.coverage.as_deref())?;
        let (x, y) = (at.0 & !1, at.1 & !1);
        let (l, t, r, b) = self.extent;
        let mut cells = std::collections::BTreeMap::new();
        for (dx, dy) in run.offsets()? {
            let x = x
                .checked_add(dx)
                .ok_or_else(|| anyhow::anyhow!("Placement is outside the map."))?;
            let y = y
                .checked_add(dy)
                .ok_or_else(|| anyhow::anyhow!("Placement is outside the map."))?;
            anyhow::ensure!(x>=-l && y>=-t && x<=i32::from(doc.map.width)-r && y<=i32::from(doc.map.height)-b,
                "Every piece and its complete artwork must fit inside the map. Move the run away from the edge.");
            for &(cx, cy, c) in &self.stamp.cells {
                if let Some(old) = cells.insert((x + cx, y + cy), c) {
                    anyhow::ensure!(
                        old == c,
                        "Repeated pieces overlap with different cells. Adjust the step."
                    );
                }
            }
        }
        if let Some(layout) = layout {
            let mut candidate = doc.map.clone();
            for (&(x, y), &c) in &cells {
                *candidate.cell_mut(x, y).unwrap() = c;
            }
            layout.validate(&candidate)?;
        }
        let stamp = Stamp {
            cells: cells.into_iter().map(|((x, y), c)| (x, y, c)).collect(),
        };
        doc.stamp((0, 0), &stamp);
        Ok(())
    }
    pub fn join(&self, other: &Piece, a: usize, b: usize, assets: &Path) -> anyhow::Result<Piece> {
        piece_ports::validate_boundary(&self.map, &self.ports, self.coverage.as_deref())?;
        piece_ports::validate_boundary(&other.map, &other.ports, other.coverage.as_deref())?;
        let pa = self
            .ports
            .get(a)
            .ok_or_else(|| anyhow::anyhow!("Select a connection on this piece."))?;
        let pb = other
            .ports
            .get(b)
            .ok_or_else(|| anyhow::anyhow!("Select a connection on the other piece."))?;
        let (dx, dy) = piece_ports::alignment(&self.map, pa, &other.map, pb)?;
        let (left, top) = (dx.min(0), dy.min(0));
        let width = (dx + i32::from(other.map.width)).max(i32::from(self.map.width)) - left;
        let height = (dy + i32::from(other.map.height)).max(i32::from(self.map.height)) - top;
        anyhow::ensure!(
            width <= 64 && height <= 64,
            "Combined piece exceeds 64 cells per side."
        );
        let mut map = MapFile::blank(width as u16, height as u16);
        for x in 0..width {
            for y in 0..height {
                map.cell_mut(x, y).unwrap().back_file = 255;
            }
        }
        let mut ports = vec![];
        let mut coverage = vec![false; width as usize * height as usize];
        for (piece, offset, used) in [(self, (-left, -top), a), (other, (dx - left, dy - top), b)] {
            for &(x, y, c) in &piece.stamp.cells {
                anyhow::ensure!(
                    !coverage[((y + offset.1) * width + x + offset.0) as usize],
                    "Joined pieces overlap covered cells. Choose a different piece or opening."
                );
                *map.cell_mut(x + offset.0, y + offset.1).unwrap() = c;
                coverage[((y + offset.1) * width + x + offset.0) as usize] = true;
            }
            for (i, p) in piece.ports.iter().enumerate() {
                if i != used {
                    ports.push(piece_ports::translate(p, &piece.map, offset, &map)?);
                }
            }
        }
        // The openings are adjacent and open, and each piece's declared ports
        // were connected before composing. Revalidate all remaining routes.
        piece_ports::validate_boundary(&map, &ports, Some(&coverage))?;
        let mut joined = Piece::from_map("Joined pieces".into(), Kind::Formation, map, assets)?;
        joined.ports = ports;
        joined.set_coverage(coverage)?;
        Ok(joined)
    }
    fn set_coverage(&mut self, coverage: Vec<bool>) -> anyhow::Result<()> {
        validate_coverage(&self.map, &coverage)?;
        self.stamp
            .cells
            .retain(|&(x, y, _)| coverage[(y * i32::from(self.map.width) + x) as usize]);
        self.coverage = Some(coverage);
        Ok(())
    }
    pub fn size(&self) -> (u16, u16) {
        (self.map.width, self.map.height)
    }
}

fn validate_coverage(map: &MapFile, mask: &[bool]) -> anyhow::Result<()> {
    anyhow::ensure!(
        mask.len() == usize::from(map.width) * usize::from(map.height) && mask.iter().any(|&c| c),
        "Piece coverage must match its dimensions and include cells."
    );
    anyhow::ensure!(
        map.width.is_multiple_of(2) && map.height.is_multiple_of(2),
        "Coverage requires even map dimensions."
    );
    for x in (0..usize::from(map.width)).step_by(2) {
        for y in (0..usize::from(map.height)).step_by(2) {
            let at = |dx, dy| mask[(y + dy) * usize::from(map.width) + x + dx];
            anyhow::ensure!(
                at(0, 0) == at(1, 0) && at(0, 0) == at(0, 1) && at(0, 0) == at(1, 1),
                "Coverage must retain complete 2x2 back tiles."
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub struct Run {
    pub count: u16,
    pub step: (i32, i32),
}
impl Default for Run {
    fn default() -> Self {
        Self {
            count: 1,
            step: (2, -2),
        }
    }
}
impl Run {
    pub fn offsets(self) -> anyhow::Result<Vec<(i32, i32)>> {
        anyhow::ensure!((1..=16).contains(&self.count), "Repeat count must be 1–16.");
        if self.count == 1 {
            return Ok(vec![(0, 0)]);
        }
        anyhow::ensure!(
            self.step != (0, 0)
                && [-64..=64, -64..=64]
                    .into_iter()
                    .zip([self.step.0, self.step.1])
                    .all(|(range, n)| range.contains(&n) && n % 2 == 0),
            "Repeat steps must be even, between -64 and 64, and must move the piece."
        );
        Ok((0..i32::from(self.count))
            .map(|i| (i * self.step.0, i * self.step.1))
            .collect())
    }
}

pub struct Panel {
    pub piece: Option<Piece>,
    pub run: Run,
    name: String,
    kind: Kind,
    path: String,
    join_path: String,
    other: Option<Piece>,
    joins: [usize; 2],
    browser: crate::piece_browser::Browser,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            piece: None,
            run: Run::default(),
            name: "Wall piece".into(),
            kind: Kind::Wall,
            path: std::env::var("ZIRCON_WALL_PIECE").unwrap_or_default(),
            join_path: std::env::var("ZIRCON_JOIN_PIECE").unwrap_or_default(),
            other: None,
            joins: [0, 0],
            browser: Default::default(),
        }
    }
}
impl Panel {
    pub fn autoload(&mut self, assets: &Path) -> anyhow::Result<bool> {
        anyhow::ensure!(
            self.path.is_empty() || std::env::var_os("ZIRCON_ASSEMBLE_KIT").is_none(),
            "Choose either ZIRCON_ASSEMBLE_KIT or ZIRCON_WALL_PIECE, not both."
        );
        let starter = self.browser.autoload(assets)?;
        if !self.path.is_empty() {
            self.load(assets)?;
        } else if let Some(p) = starter {
            self.name = p.name.clone();
            self.kind = p.kind;
            self.piece = Some(p);
        } else {
            return Ok(false);
        }
        if std::env::var_os("ZIRCON_FIND_JOINS").is_some() {
            self.browser.search(self.piece.as_ref().unwrap(), assets)?;
        }
        if !self.join_path.is_empty() {
            self.other = Some(Piece::load(Path::new(&self.join_path), assets)?);
        }
        Ok(true)
    }
    fn load(&mut self, assets: &Path) -> anyhow::Result<()> {
        let piece = Piece::load(Path::new(&self.path), assets)?;
        self.name = piece.name.clone();
        self.kind = piece.kind;
        self.piece = Some(piece);
        self.browser.invalidate();
        self.run = Run::default();
        if let Ok(count) = std::env::var("ZIRCON_WALL_REPEATS") {
            self.run.count = count
                .parse()
                .ok()
                .filter(|n| (1..=16).contains(n))
                .unwrap_or(1);
        }
        Ok(())
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        stamp: Option<&Stamp>,
        assets: &Path,
    ) -> (bool, Option<String>) {
        let mut activate = false;
        let mut status = None;
        egui::CollapsingHeader::new("Reusable wall pieces")
            .default_open(self.piece.is_some())
            .show(ui, |ui| {
                let (picked, message) = self.browser.show(ui, self.piece.as_ref(), &self.name, self.kind, assets);
                if let Some(message) = message { status=Some(message); }
                if let Some(p) = picked { self.name=p.name.clone(); self.kind=p.kind; self.piece=Some(p); self.run=Run::default(); self.joins=[0,0]; activate=true; }
                ui.label("Copy an area, name it, then save it for other maps. Include the ground and collision around walls.");
                ui.text_edit_singleline(&mut self.name);
                ui.horizontal_wrapped(|ui| {
                    for kind in [Kind::Wall, Kind::Corner, Kind::Doorway, Kind::Formation] {
                        ui.selectable_value(&mut self.kind, kind, kind.label());
                    }
                });
                if ui.add_enabled(stamp.is_some(), egui::Button::new("Use copied area")).clicked() {
                    match Piece::capture(stamp.unwrap(), self.name.clone(), self.kind, assets) {
                        Ok(p) => {
                            self.browser.invalidate();
                            self.piece = Some(p);
                            self.run = Run::default();
                            activate = true;
                            status = Some("Piece ready. Click the map to place it.".into());
                        }
                        Err(e) => status = Some(e.to_string()),
                    }
                }
                if let Some(p) = &self.piece {
                    egui::CollapsingHeader::new("Placement").default_open(p.ports.is_empty()).show(ui,|ui| {
                    let (w, h) = p.size();
                    let (l, t, r, b) = p.extent;
                    ui.label(format!("{w} x {h} cells; artwork spans {} x {} cells.", r-l, b-t));
                    ui.label("Placement copies every layer and collision cell. Selection is manual: check neighboring wall joins and doorway clearance in Test Walk.");
                    ui.horizontal(|ui| {
                        ui.label("Repeats");
                        ui.add(egui::DragValue::new(&mut self.run.count).range(1..=16));
                    });
                    if self.run.count > 1 {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Step X / Y");
                            ui.add(egui::DragValue::new(&mut self.run.step.0).range(-64..=64).speed(2));
                            ui.add(egui::DragValue::new(&mut self.run.step.1).range(-64..=64).speed(2));
                        });
                        ui.label("Use even steps matching the authored wall direction. One Undo restores the whole run.");
                    }
                    if ui.button("Place piece / run").clicked() {
                        activate = true;
                    }
                    });
                }
                if let Some(piece)=&mut self.piece {
                    let old_ports = piece.ports.clone();
                    egui::CollapsingHeader::new("Connections").default_open(!piece.ports.is_empty()).show(ui,|ui| {
                        ui.label("Offset runs along the edge; inset moves it inward to a recess. Openings must face uncovered space.");
                        let mut remove=None;
                        for (i,port) in piece.ports.iter_mut().enumerate() {
                            ui.push_id(i,|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(format!("Connection {}",i+1));
                                    if ui.small_button("Remove").clicked() {remove=Some(i);}
                                });
                                egui::ComboBox::from_id_salt("side").selected_text(port.side.label()).show_ui(ui,|ui| {
                                    for side in [Side::North,Side::East,Side::South,Side::West] {ui.selectable_value(&mut port.side,side,side.label());}
                                });
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Inset");ui.add(egui::DragValue::new(&mut port.inset).range(0..=63));
                                    ui.label("Offset");ui.add(egui::DragValue::new(&mut port.offset).range(0..=63));
                                    ui.label("Width");ui.add(egui::DragValue::new(&mut port.width).range(1..=64));
                                });
                                ui.label("Compatibility profile");ui.text_edit_singleline(&mut port.profile);
                            });
                        }
                        if let Some(i)=remove {piece.ports.remove(i);}
                        if ui.add_enabled(piece.ports.len()<16,egui::Button::new("Add connection")).clicked() {piece.ports.push(Port::default());}
                        match piece_ports::validate_boundary(&piece.map,&piece.ports,piece.coverage.as_deref()) {
                            Ok(()) if !piece.ports.is_empty()=>{ui.label("Openings and internal routes are clear.");},
                            Ok(())=>{ui.label("No connections declared.");},
                            Err(e)=>{ui.colored_label(egui::Color32::LIGHT_RED,e.to_string());},
                        }
                        let scale=(190.0/f32::from(piece.map.width)).min(8.0);
                        let (rect,_)=ui.allocate_exact_size(egui::vec2(f32::from(piece.map.width)*scale,f32::from(piece.map.height)*scale),egui::Sense::hover());
                        ui.painter().rect_filled(rect,0.0,egui::Color32::BLACK);
                        for x in 0..i32::from(piece.map.width) {for y in 0..i32::from(piece.map.height) {
                            if piece.map.is_walkable(x,y) {ui.painter().rect_filled(egui::Rect::from_min_size(rect.min+egui::vec2(x as f32*scale,y as f32*scale),egui::vec2(scale,scale)),0.0,egui::Color32::DARK_GREEN);}
                        }}
                        for p in &piece.ports {for (x,y) in p.cells(&piece.map) {if piece.map.cell(x,y).is_some() {ui.painter().rect_filled(egui::Rect::from_min_size(rect.min+egui::vec2(x as f32*scale,y as f32*scale),egui::vec2(scale,scale)),0.0,egui::Color32::GOLD);}}}
                        ui.label("Green: floor. Gold: opening. Save a new file to keep connection edits.");
                    });
                    if old_ports != piece.ports { self.browser.invalidate(); }
                }
                egui::CollapsingHeader::new("Join another piece").default_open(self.other.is_some()).show(ui,|ui| {
                    ui.text_edit_singleline(&mut self.join_path);
                    if ui.button("Load adjoining piece").clicked() {
                        match Piece::load(Path::new(&self.join_path),assets) {Ok(p)=>{self.other=Some(p);self.joins=[0,0];},Err(e)=>status=Some(e.to_string())}
                    }
                    if let (Some(a),Some(b))=(&self.piece,&self.other) {
                        ui.label(format!("Other: {}",b.name));
                        for (i,piece) in [a,b].into_iter().enumerate() {
                            egui::ComboBox::from_id_salt(("join",i)).selected_text(format!("{} connection {}",if i==0 {"This"} else {"Other"},self.joins[i]+1)).show_ui(ui,|ui| {
                                for (j,p) in piece.ports.iter().enumerate() {ui.selectable_value(&mut self.joins[i],j,format!("{}: {} / {}",j+1,p.side.label(),p.profile));}
                            });
                        }
                        ui.label("Joining creates a new piece for review. It does not rotate artwork or alter the open map.");
                        if ui.add_enabled(!a.ports.is_empty() && !b.ports.is_empty(),egui::Button::new("Build joined piece")).clicked() {
                            match a.join(b,self.joins[0],self.joins[1],assets) {
                                Ok(p)=>{self.browser.invalidate();self.name=p.name.clone();self.kind=p.kind;self.piece=Some(p);self.run=Run::default();self.joins=[0,0];activate=true;status=Some("Joined piece ready. Review, place and save it to a new file.".into());},
                                Err(e)=>status=Some(format!("Join refused: {e}")),
                            }
                        }
                    }
                });
                ui.label("Piece file (.json)");
                ui.text_edit_singleline(&mut self.path);
                ui.horizontal(|ui| {
                    if ui.button("Load piece").clicked() {
                        match self.load(assets) {
                            Ok(()) => {
                                activate = true;
                                status = Some("Piece loaded. Click the map to place it.".into());
                            }
                            Err(e) => status = Some(e.to_string()),
                        }
                    }
                    if ui.add_enabled(self.piece.is_some(), egui::Button::new("Save new file")).clicked() {
                        status = Some(match self.piece.as_ref().unwrap().save(Path::new(&self.path), &self.name, self.kind) {
                            Ok(()) => "Piece saved; original assets are referenced by the file.".into(),
                            Err(e) => e.to_string(),
                        });
                    }
                });
            });
        (activate, status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn folder() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "zircon-wall-piece-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        p
    }
    fn source() -> MapFile {
        let mut m = MapFile::blank(4, 4);
        for x in 0..4 {
            for y in 0..4 {
                let c = m.cell_mut(x, y).unwrap();
                c.back_file = if x % 2 == 0 && y % 2 == 0 { 255 } else { 0 };
                c.middle_file = 255;
                c.front_file = 255;
                c.flag = 0x80;
                c.middle_image = 65535;
                c.front_image = 65535;
                c.middle_anim = 255;
                c.set_front_anim(255);
                c.padding = [x as u8 + 1, 2, 3, 4];
                c.set_light(18);
            }
        }
        m
    }
    #[test]
    fn saved_piece_roundtrips_all_cells_places_once_and_undo_restores_saved_map() {
        let folder = folder();
        let map = source();
        let p =
            Piece::from_map("Corner fixture".into(), Kind::Corner, map.clone(), &folder).unwrap();
        let path = folder.join("piece.json");
        p.save(&path, &p.name, p.kind).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(p.save(&path, "Overwrite", Kind::Wall).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let p = Piece::load(&path, &folder).unwrap();
        assert_eq!(p.map.to_bytes(), map.to_bytes());
        let mut doc = Document::blank(32, 32);
        doc.save(&folder.join("target.map")).unwrap();
        let before = doc.map.to_bytes();
        p.place(
            &mut doc,
            (9, 11),
            None,
            Run {
                count: 3,
                step: (4, 0),
            },
        )
        .unwrap();
        for x in 0..4 {
            for y in 0..4 {
                for i in 0..3 {
                    assert_eq!(doc.map.cell(8 + i * 4 + x, 10 + y), map.cell(x, y));
                }
            }
        }
        doc.save(&folder.join("target.map")).unwrap();
        doc.undo();
        assert_eq!(doc.map.to_bytes(), before);
        assert!(!doc.can_undo());
        assert!(doc.dirty());
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn invalid_placement_and_generated_content_conflict_are_atomic() {
        let folder = folder();
        let p = Piece::from_map("Wall".into(), Kind::Wall, source(), &folder).unwrap();
        let mut doc = Document::blank(32, 32);
        let before = doc.map.to_bytes();
        for at in [(-1, 0), (31, 30), (i32::MAX, i32::MAX), (i32::MIN, 0)] {
            assert!(p.place(&mut doc, at, None, Run::default()).is_err());
            assert_eq!(doc.map.to_bytes(), before);
            assert!(!doc.can_undo());
        }
        for (at, run) in [
            (
                (26, 0),
                Run {
                    count: 2,
                    step: (4, 0),
                },
            ),
            (
                (0, 0),
                Run {
                    count: 2,
                    step: (2, 0),
                },
            ),
            (
                (0, 0),
                Run {
                    count: 2,
                    step: (1, 0),
                },
            ),
            (
                (0, 0),
                Run {
                    count: 2,
                    step: (0, 0),
                },
            ),
            (
                (0, 0),
                Run {
                    count: 17,
                    step: (4, 0),
                },
            ),
        ] {
            assert!(p.place(&mut doc, at, None, run).is_err());
            assert_eq!(doc.map.to_bytes(), before);
            assert!(!doc.can_undo());
        }
        let layout = crate::generate::layout(&doc.map, 42).unwrap();
        assert!(p
            .place(&mut doc, layout.start, Some(&layout), Run::default())
            .is_err());
        assert_eq!(doc.map.to_bytes(), before);
        assert!(!doc.can_undo());
        let mut invalid = Stamp::capture(&source(), (0, 0), (3, 3));
        invalid.cells.pop();
        assert!(Piece::capture(&invalid, "Incomplete".into(), Kind::Wall, &folder).is_err());
        let mut f = File {
            version: 4,
            name: "Future".into(),
            kind: Kind::Wall,
            map: source().to_bytes(),
            ports: vec![],
            coverage: None,
        };
        let path = folder.join("bad.json");
        std::fs::write(&path, serde_json::to_vec(&f).unwrap()).unwrap();
        assert!(Piece::load(&path, &folder).is_err());
        f.version = 1;
        f.map[22..24].copy_from_slice(&32767i16.to_le_bytes());
        std::fs::write(&path, serde_json::to_vec(&f).unwrap()).unwrap();
        assert!(Piece::load(&path, &folder).is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }
    fn connected_piece(assets: &Path, sides: &[Side]) -> Piece {
        let mut map = source();
        for x in 0..4 {
            for y in 0..4 {
                map.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        let mut piece =
            Piece::from_map("Connected fixture".into(), Kind::Corner, map, assets).unwrap();
        piece.ports = sides
            .iter()
            .map(|&side| Port {
                side,
                offset: 0,
                width: 1,
                profile: "wood".into(),
                inset: 0,
            })
            .collect();
        piece
    }
    #[test]
    fn corner_join_preserves_cells_and_roundtrips_remaining_connections() {
        let dir = folder();
        // Disjoint edge cells, with a turn through the first piece.
        let mut a = connected_piece(&dir, &[Side::North, Side::East]);
        a.ports[0].offset = 2;
        let b = connected_piece(&dir, &[Side::West, Side::East]);
        let original = a.map.to_bytes();
        let joined = a.join(&b, 1, 0, &dir).unwrap();
        assert_eq!(joined.size(), (8, 4));
        assert_eq!(joined.ports.len(), 2);
        assert_eq!(joined.ports[0], a.ports[0]);
        assert_eq!(joined.ports[1], b.ports[1]);
        for x in 0..4 {
            for y in 0..4 {
                assert_eq!(joined.map.cell(x, y), a.map.cell(x, y));
                assert_eq!(joined.map.cell(x + 4, y), b.map.cell(x, y));
            }
        }
        let path = dir.join("joined.json");
        joined.save(&path, "Joined corner", Kind::Corner).unwrap();
        let loaded = Piece::load(&path, &dir).unwrap();
        assert_eq!(loaded.ports, joined.ports);
        assert_eq!(loaded.map.to_bytes(), joined.map.to_bytes());
        let mut doc = Document::blank(32, 32);
        let before = doc.map.to_bytes();
        loaded
            .place(&mut doc, (8, 8), None, Run::default())
            .unwrap();
        doc.undo();
        assert_eq!(doc.map.to_bytes(), before);
        assert_eq!(a.map.to_bytes(), original);
        let mut v2: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        v2["version"] = 2.into();
        for port in v2["ports"].as_array_mut().unwrap() {
            port.as_object_mut().unwrap().remove("inset");
        }
        std::fs::write(&path, serde_json::to_vec(&v2).unwrap()).unwrap();
        assert_eq!(Piece::load(&path, &dir).unwrap().ports, joined.ports);
        // Old files remain readable, but cannot smuggle new declarations into v1.
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        legacy["version"] = 1.into();
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(Piece::load(&path, &dir).is_err());
        legacy.as_object_mut().unwrap().remove("ports");
        legacy.as_object_mut().unwrap().remove("coverage");
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(Piece::load(&path, &dir).unwrap().ports.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn invalid_joins_and_blocked_saved_openings_leave_sources_untouched() {
        let dir = folder();
        let a = connected_piece(&dir, &[Side::East]);
        let mut b = connected_piece(&dir, &[Side::West]);
        let before = a.map.to_bytes();
        b.ports[0].profile = "other".into();
        assert!(a.join(&b, 0, 0, &dir).is_err());
        b.ports[0].profile = "wood".into();
        b.ports[0].offset = 1;
        assert!(a.join(&b, 0, 0, &dir).is_err());
        b.ports[0].offset = 0;
        b.map.cell_mut(1, 0).unwrap().flag = 0;
        assert!(a.join(&b, 0, 0, &dir).is_err());
        let path = dir.join("blocked.json");
        assert!(b.save(&path, "Blocked", Kind::Doorway).is_err());
        assert!(!path.exists());
        std::fs::write(
            &path,
            serde_json::to_vec(&File {
                version: 2,
                name: b.name.clone(),
                kind: b.kind,
                map: b.map.to_bytes(),
                ports: b.ports.clone(),
                coverage: b.coverage.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(Piece::load(&path, &dir).is_err());
        let mut corner = connected_piece(&dir, &[Side::North, Side::East]);
        corner.ports[0].offset = 2;
        let mut shifted = connected_piece(&dir, &[Side::West]);
        shifted.ports[0].offset = 2;
        let recessed = corner.join(&shifted, 1, 0, &dir).unwrap();
        assert_eq!(recessed.ports[0].inset, 2);
        assert_eq!(a.map.to_bytes(), before);
        assert!(a.join(&a, 10, 0, &dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn offset_join_padding_survives_reload_without_erasing_destination() {
        let dir = folder();
        let a = connected_piece(&dir, &[Side::East]);
        let mut b = connected_piece(&dir, &[Side::West]);
        b.ports[0].offset = 2;
        let joined = a.join(&b, 0, 0, &dir).unwrap();
        assert_eq!(joined.size(), (8, 6));
        let path = dir.join("offset.json");
        joined.save(&path, "Offset", Kind::Formation).unwrap();
        let loaded = Piece::load(&path, &dir).unwrap();
        let mut doc = Document::blank(32, 32);
        doc.map.cell_mut(8, 8).unwrap().padding = [90, 91, 92, 93];
        let before = doc.map.to_bytes();
        let padding = *doc.map.cell(8, 8).unwrap();
        loaded
            .place(&mut doc, (8, 8), None, Run::default())
            .unwrap();
        assert_eq!(*doc.map.cell(8, 8).unwrap(), padding);
        assert_eq!(doc.map.cell(8, 10), a.map.cell(0, 0));
        assert_eq!(doc.map.cell(12, 8), b.map.cell(0, 0));
        doc.undo();
        assert_eq!(doc.map.to_bytes(), before);
        let bytes = std::fs::read(&path).unwrap();
        let mut file: File = serde_json::from_slice(&bytes).unwrap();
        let mask = file.coverage.as_mut().unwrap();
        let i = mask.iter().position(|&v| v).unwrap();
        mask[i] = false;
        std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        assert!(Piece::load(&path, &dir).is_err());
        file = serde_json::from_slice(&bytes).unwrap();
        // Hidden padding cannot fabricate a walkable connection on load.
        let mut map = MapFile::parse(&file.map).unwrap();
        map.cell_mut(0, 0).unwrap().flag = 3;
        map.cell_mut(1, 0).unwrap().flag = 3;
        file.map = map.to_bytes();
        file.ports = vec![Port {
            side: Side::West,
            width: 1,
            ..Port::default()
        }];
        std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        assert!(Piece::load(&path, &dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recessed_join_survives_reload_and_accepts_a_third_piece() {
        let dir = folder();
        let mut a = connected_piece(&dir, &[Side::North, Side::East]);
        a.ports[0].offset = 2;
        let mut b = connected_piece(&dir, &[Side::West]);
        b.ports[0].offset = 2;
        let recess = a.join(&b, 1, 0, &dir).unwrap();
        assert_eq!(recess.ports[0].inset, 2);
        let path = dir.join("recess.json");
        recess.save(&path, "Recess", Kind::Corner).unwrap();
        let loaded = Piece::load(&path, &dir).unwrap();
        assert_eq!(loaded.ports, recess.ports);
        let mut cap_map = MapFile::blank(4, 2);
        for x in 0..4 {
            for y in 0..2 {
                let c = cap_map.cell_mut(x, y).unwrap();
                c.flag = 3;
                c.back_file = 255;
            }
        }
        let mut cap = Piece::from_map("Cap".into(), Kind::Corner, cap_map, &dir).unwrap();
        cap.ports = vec![Port {
            side: Side::South,
            offset: 2,
            width: 1,
            profile: "wood".into(),
            inset: 0,
        }];
        let mut extra = connected_piece(&dir, &[Side::West, Side::West]);
        extra.ports[0].offset = 2;
        let exposed = a.join(&extra, 1, 0, &dir).unwrap();
        assert!(exposed
            .join(&cap, 0, 0, &dir)
            .err()
            .unwrap()
            .to_string()
            .contains("uncovered"));
        let final_piece = loaded.join(&cap, 0, 0, &dir).unwrap();
        assert_eq!(final_piece.size(), (8, 6));
        assert!(final_piece.ports.is_empty());
        for x in 0..4 {
            for y in 0..2 {
                assert_eq!(final_piece.map.cell(x, y), cap.map.cell(x, y));
            }
        }
        let mut doc = Document::blank(32, 32);
        let before = doc.map.to_bytes();
        final_piece
            .place(&mut doc, (8, 8), None, Run::default())
            .unwrap();
        doc.undo();
        assert_eq!(doc.map.to_bytes(), before);
        let mut wide_map = MapFile::blank(8, 4);
        for x in 0..8 {
            for y in 0..4 {
                let c = wide_map.cell_mut(x, y).unwrap();
                c.flag = 3;
                c.back_file = 255;
            }
        }
        let mut wide = Piece::from_map("Overlap".into(), Kind::Corner, wide_map, &dir).unwrap();
        wide.ports = cap.ports.clone();
        let original = loaded.map.to_bytes();
        assert!(loaded
            .join(&wide, 0, 0, &dir)
            .err()
            .unwrap()
            .to_string()
            .contains("overlap"));
        assert_eq!(loaded.map.to_bytes(), original);
        let mut old: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        old["version"] = 2.into();
        std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(Piece::load(&path, &dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn shipped_floor_assembly_fills_recess_and_exports_for_test_walk() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1006.map")).unwrap();
        let make = |w: u16, h: u16, name: &str| {
            let mut map = MapFile::blank(w, h);
            for x in 0..i32::from(w) {
                for y in 0..i32::from(h) {
                    *map.cell_mut(x, y).unwrap() = *source.cell(20 + x % 4, 24 + y % 4).unwrap();
                }
            }
            Piece::from_map(name.into(), Kind::Corner, map, &assets).unwrap()
        };
        let port = |side, offset| Port {
            side,
            offset,
            width: 2,
            profile: "wood-floor".into(),
            inset: 0,
        };
        let mut a = make(8, 8, "Corner A");
        a.ports = vec![port(Side::North, 2), port(Side::East, 2)];
        let mut b = make(8, 8, "Corner B");
        b.ports = vec![port(Side::West, 6), port(Side::East, 6)];
        let recess = a.join(&b, 1, 0, &assets).unwrap();
        assert_eq!(recess.ports[0].inset, 4);
        let mut cap = make(8, 4, "Recess filler");
        cap.ports = vec![port(Side::South, 2), port(Side::West, 0)];
        let filled = recess.join(&cap, 0, 0, &assets).unwrap();
        assert_eq!(filled.size(), (16, 12));
        assert_eq!(filled.map.walkable_count(), 160);
        let mut doc = Document::blank(64, 64);
        for x in 0..64 {
            for y in 0..64 {
                doc.map.cell_mut(x, y).unwrap().flag = 0;
            }
        }
        filled
            .place(&mut doc, (24, 24), None, Run::default())
            .unwrap();
        crate::generate::layout(&doc.map, 42).unwrap();
        if let Some(out) = std::env::var_os("ZIRCON_RECESS_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            recess
                .save(&out.join("recess.json"), "Recessed corner", Kind::Corner)
                .unwrap();
            cap.save(&out.join("filler.json"), &cap.name, cap.kind)
                .unwrap();
            filled
                .save(&out.join("filled.json"), "Three-piece corner", Kind::Corner)
                .unwrap();
            std::fs::write(out.join("Recess.map"), doc.map.to_bytes()).unwrap();
        }
    }

    #[test]
    fn shipped_wall_connections_compose_without_changing_collision() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1006.map")).unwrap();
        let before = source.to_bytes();
        let mut a = Piece::capture(
            &Stamp::capture(&source, (10, 8), (11, 17)),
            "NW wall A".into(),
            Kind::Wall,
            &assets,
        )
        .unwrap();
        let mut b = Piece::capture(
            &Stamp::capture(&source, (12, 6), (13, 17)),
            "NW wall B".into(),
            Kind::Wall,
            &assets,
        )
        .unwrap();
        a.ports = vec![
            Port {
                side: Side::West,
                offset: 8,
                width: 2,
                profile: "wood-nw-ground".into(),
                inset: 0,
            },
            Port {
                side: Side::East,
                offset: 8,
                width: 2,
                profile: "wood-nw-ground".into(),
                inset: 0,
            },
        ];
        b.ports = vec![
            Port {
                side: Side::West,
                offset: 10,
                width: 2,
                profile: "wood-nw-ground".into(),
                inset: 0,
            },
            Port {
                side: Side::East,
                offset: 10,
                width: 2,
                profile: "wood-nw-ground".into(),
                inset: 0,
            },
        ];
        let joined = a.join(&b, 1, 0, &assets).unwrap();
        assert_eq!(joined.size(), (4, 12));
        assert_eq!(
            joined.ports.iter().map(|p| p.offset).collect::<Vec<_>>(),
            vec![10, 10]
        );
        let mut doc = Document::blank(50, 50);
        doc.map = source.clone();
        let layout = crate::generate::layout(&source, 42).unwrap();
        joined
            .place(&mut doc, (10, 6), Some(&layout), Run::default())
            .unwrap();
        for x in 0..50 {
            for y in 0..50 {
                assert_eq!(doc.map.is_walkable(x, y), source.is_walkable(x, y));
            }
        }
        if let Some(out) = std::env::var_os("ZIRCON_PORTS_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            a.save(&out.join("wall-a.json"), &a.name, a.kind).unwrap();
            b.save(&out.join("wall-b.json"), &b.name, b.kind).unwrap();
            joined
                .save(
                    &out.join("joined-wall.json"),
                    "Joined northwest wall",
                    Kind::Wall,
                )
                .unwrap();
            std::fs::write(out.join("JoinedWall.map"), doc.map.to_bytes()).unwrap();
        }
        doc.undo();
        assert_eq!(doc.map.to_bytes(), before);
        assert_eq!(source.to_bytes(), before);
    }

    #[test]
    fn shipped_wall_strip_roundtrips_and_joins_at_authored_neighbor() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1006.map")).unwrap();
        let before = source.to_bytes();
        let stamp = Stamp::capture(&source, (10, 8), (11, 17));
        let piece =
            Piece::capture(&stamp, "Wood northwest wall".into(), Kind::Wall, &assets).unwrap();
        assert_eq!(piece.size(), (2, 10));
        assert_eq!(piece.extent, (0, 0, 2, 10));
        let narrow = Piece::capture(
            &Stamp::capture(&source, (10, 12), (11, 13)),
            "Tall anchor".into(),
            Kind::Wall,
            &assets,
        )
        .unwrap();
        assert!(narrow.extent.1 < 0);
        let mut blank = Document::blank(32, 32);
        let blank_before = blank.map.to_bytes();
        assert!(narrow
            .place(&mut blank, (0, 0), None, Run::default())
            .is_err());
        assert_eq!(blank.map.to_bytes(), blank_before);
        let dir = folder();
        let path = dir.join("northwest-wall.json");
        piece.save(&path, &piece.name, piece.kind).unwrap();
        let piece = Piece::load(&path, &assets).unwrap();
        let mut doc = Document::blank(50, 50);
        doc.map = source.clone();
        let layout = crate::generate::layout(&doc.map, 42).unwrap();
        piece
            .place(
                &mut doc,
                (10, 8),
                Some(&layout),
                Run {
                    count: 2,
                    step: (2, -2),
                },
            )
            .unwrap();
        for (dx, dy, c) in &stamp.cells {
            assert_eq!(doc.map.cell(12 + dx, 6 + dy), Some(c));
        }
        // Collision matches the authored adjoining repeat exactly.
        for x in 0..50 {
            for y in 0..50 {
                assert_eq!(doc.map.is_walkable(x, y), source.is_walkable(x, y));
            }
        }
        if let Some(out) = std::env::var_os("ZIRCON_WALL_PIECE_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            piece
                .save(&out.join("northwest-wall.json"), &piece.name, piece.kind)
                .unwrap();
            std::fs::write(out.join("WallKit.map"), doc.map.to_bytes()).unwrap();
        }
        doc.undo();
        assert_eq!(doc.map.to_bytes(), before);
        assert_eq!(source.to_bytes(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
