//! Sprite previews: lazily opened `.Zl` libraries and an egui texture cache.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui;
use mir_formats::monster_table::{library_path, monster_sprite};
use mir_formats::zl::{SurfaceKind, ZlLibrary};

/// Zircon `LibraryFile` ids used by the previews.
pub const STORE_ITEMS: u16 = 14;
pub const NPC: u16 = 17;
pub const MAGIC_ICON: u16 = 20;

/// A loaded sprite: texture plus the unpadded size.
pub type Sprite = (egui::TextureHandle, [u16; 2]);

pub struct Previews {
    root: PathBuf,
    libs: HashMap<u16, Option<ZlLibrary>>,
    textures: HashMap<(u16, u32), Option<Sprite>>,
}

impl Previews {
    pub fn new(root: impl AsRef<Path>) -> Previews {
        Previews {
            root: root.as_ref().to_path_buf(),
            libs: HashMap::new(),
            textures: HashMap::new(),
        }
    }

    fn library(&mut self, id: u16) -> Option<&ZlLibrary> {
        let root = self.root.clone();
        self.libs
            .entry(id)
            .or_insert_with(|| {
                let rel = library_path(id)?;
                let path = resolve_case_insensitive(&root, rel)?;
                ZlLibrary::open(path).ok()
            })
            .as_ref()
    }

    pub fn begin_frame(&mut self) {
        if self.textures.len() > 2048 {
            self.textures.clear();
        }
    }

    pub fn count(&mut self, id: u16) -> usize {
        self.library(id).map(|l| l.len()).unwrap_or(0)
    }

    /// Texture for one image, decoded once. Returns the handle and the
    /// unpadded sprite size.
    pub fn texture(&mut self, ctx: &egui::Context, lib: u16, index: u32) -> Option<Sprite> {
        if let Some(t) = self.textures.get(&(lib, index)) {
            return t.clone();
        }
        let decoded = self.library(lib).and_then(|l| {
            let info = *l.info(index as usize)?;
            let s = l.decode(index as usize, SurfaceKind::Image).ok()??;
            Some((s, info))
        });
        let entry = decoded.map(|(s, info)| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [s.width as usize, s.height as usize],
                &s.rgba,
            );
            let handle = ctx.load_texture(
                format!("zl-{lib}-{index}"),
                image,
                egui::TextureOptions::NEAREST,
            );
            (handle, [info.width, info.height])
        });
        self.textures.insert((lib, index), entry.clone());
        entry
    }

    /// Draw a sprite at `scale`, cropping the DXT padding.
    pub fn show(&mut self, ui: &mut egui::Ui, lib: u16, index: u32, scale: f32) -> bool {
        let Some((tex, [w, h])) = self.texture(ui.ctx(), lib, index) else {
            return false;
        };
        let full = tex.size_vec2();
        let uv = egui::Rect::from_min_max(
            egui::pos2(0.0, 0.0),
            egui::pos2(w as f32 / full.x, h as f32 / full.y),
        );
        ui.add(
            egui::Image::new((tex.id(), egui::vec2(w as f32 * scale, h as f32 * scale)))
                .uv(uv)
                .bg_fill(egui::Color32::from_gray(40)),
        );
        true
    }

    /// Standing frame (facing down) of a monster by its `Image` value.
    pub fn monster_frame(image: u16, direction: u8) -> Option<(u16, u32)> {
        let (file, shape) = monster_sprite(image)?;
        Some((file, shape as u32 * 1000 + 10 * direction as u32))
    }
}

/// Asset packs differ in file-name case (`Storeitems.Zl`); find the real path.
pub(crate) fn resolve_case_insensitive(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut cur = root.to_path_buf();
    for part in rel.split('/') {
        let direct = cur.join(part);
        if direct.exists() {
            cur = direct;
            continue;
        }
        let found = std::fs::read_dir(&cur)
            .ok()?
            .flatten()
            .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        cur = found.path();
    }
    Some(cur)
}
