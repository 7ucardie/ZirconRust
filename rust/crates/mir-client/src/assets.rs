//! Asset resolution: Zircon `LibraryFile` ids -> `.Zl` files on disk, plus a
//! cache of open libraries.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mir_formats::zl::{ImageInfo, SurfaceKind, ZlLibrary};

use mir_formats::monster_table::library_path;

pub mod lib {
    pub const EQUIP: u16 = 6;
    pub const EQUIP_EFFECT_UI: u16 = 7;
    pub const PROG_USE: u16 = 13;
    pub const INTERFACE: u16 = 3;
    pub const GAME_INTER: u16 = 4;
    pub const GAME_INTER2: u16 = 5;
    pub const MINI_MAP: u16 = 18;
    pub const MAGIC_ICON: u16 = 20;
    pub const STORE_ITEMS: u16 = 14;
    pub const GROUND: u16 = 16;
    pub const NPC: u16 = 17;
    pub const M_HUM: u16 = 31;
    pub const M_HAIR: u16 = 41;
    pub const WM_HUM: u16 = 42;
    pub const WM_HAIR: u16 = 52;
    pub const M_HUM_A: u16 = 53;
    pub const M_HAIR_A: u16 = 58;
    pub const WM_HUM_A: u16 = 59;
    pub const WM_HAIR_A: u16 = 64;
}

pub use mir_formats::wearable_animation::{
    armour_library, helmet_library, shield_library, weapon_library,
};

pub use mir_formats::monster_table::kr_library;

pub struct Assets {
    root: PathBuf,
    libs: HashMap<u16, Option<Arc<ZlLibrary>>>,
}

impl Assets {
    pub fn new(root: impl AsRef<Path>) -> Assets {
        Assets {
            root: root.as_ref().to_path_buf(),
            libs: HashMap::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn library(&mut self, id: u16) -> Option<Arc<ZlLibrary>> {
        if let Some(l) = self.libs.get(&id) {
            return l.clone();
        }
        let loaded = library_path(id).and_then(|rel| {
            let mut path = self.root.join(rel);
            if !path.exists() {
                // Asset packs differ in filename case (Storeitems.Zl) and upstream
                // renamed StoreItems.Zl to StoreItem.Zl.
                if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
                    let wanted = name.to_string_lossy().to_lowercase();
                    let alt = wanted.replace("storeitems.zl", "storeitem.zl");
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for e in entries.flatten() {
                            let have = e.file_name().to_string_lossy().to_lowercase();
                            if have == wanted || have == alt {
                                path = e.path();
                                break;
                            }
                        }
                    }
                }
            }
            match ZlLibrary::open(&path) {
                Ok(l) => {
                    tracing::info!(id, path = %path.display(), images = l.len(), version = l.version(), "library opened");
                    Some(Arc::new(l))
                }
                Err(e) => {
                    tracing::warn!(id, path = %path.display(), "cannot open library: {e}");
                    None
                }
            }
        });
        self.libs.insert(id, loaded.clone());
        loaded
    }

    pub fn info(&mut self, id: u16, index: u32) -> Option<ImageInfo> {
        self.library(id)?.info(index as usize).copied()
    }

    /// Decode a surface to (padded width, padded height, rgba).
    pub fn decode(
        &mut self,
        id: u16,
        index: u32,
        kind: SurfaceKind,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let lib = self.library(id)?;
        match lib.decode(index as usize, kind) {
            Ok(Some(s)) => Some((s.width, s.height, s.rgba)),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(id, index, "decode failed: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kr_library_values() {
        assert_eq!(kr_library(0), Some(266)); // Tilesc
        assert_eq!(kr_library(11), Some(254)); // Animationsc
        assert_eq!(kr_library(12), Some(260)); // Object1c
        assert_eq!(kr_library(60), Some(278)); // Forest_Tilesc
        assert_eq!(kr_library(15), Some(314)); // Wood_Tilesc
        assert_eq!(kr_library(14), None);
    }

    #[test]
    fn library_paths_match_generated_table() {
        assert_eq!(library_path(266), Some("Data/Map Data/Tilesc.Zl"));
        assert_eq!(library_path(278), Some("Data/Map Data/Forest/Tilesc.Zl"));
        assert_eq!(library_path(314), Some("Data/Map Data/Wood/Tilesc.Zl"));
        assert_eq!(library_path(lib::M_HUM), Some("Data/M-Hum.Zl"));
        assert_eq!(library_path(lib::GAME_INTER), Some("Data/GameInter.Zl"));
    }
}
