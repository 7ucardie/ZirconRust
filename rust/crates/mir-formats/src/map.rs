//! `.map` reader and writer (Zircon `MapControl.LoadMap` / server `Map.Load`).
//!
//! ```text
//! offset 22: i16 width, i16 height
//! offset 28: back layer, (width/2)*(height/2) records of 3 bytes, x-major:
//!            u8 back_file, u16 back_image           -> stored at cell (x*2, y*2)
//! then:      width*height records of 14 bytes, x-major:
//!            u8 flag, u8 middle_anim, u8 front_anim, u8 front_file, u8 middle_file,
//!            u16 middle_image, u16 front_image, 3 skipped, u8 light, 1 skipped
//! ```
//! A cell is walkable only when both bits 0 and 1 of `flag` are set.
//!
//! The parser keeps every byte it does not interpret (header, raw animation
//! and light bytes, padding, trailer) so [`MapFile::to_bytes`] reproduces the
//! input exactly.

use std::path::Path;

use crate::{Cursor, FormatError, Result, Writer};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cell {
    pub flag: u8,
    /// Zircon `KROrder` key of the back tile library (only meaningful on even x/y cells).
    pub back_file: u8,
    /// Raw back image index (used verbatim).
    pub back_image: u16,
    pub middle_file: u8,
    /// Raw middle image index (Zircon stores +1 and subtracts at draw time).
    pub middle_image: u16,
    pub front_file: u8,
    pub front_image: u16,
    /// Raw animation byte for the middle tile.
    pub middle_anim: u8,
    /// Front animation byte masked with 0x8F (255 -> 0).
    pub front_anim: u8,
    /// Light level, 0..=30.
    pub light: u8,
    /// Raw front animation byte before masking (kept for saving).
    pub front_anim_raw: u8,
    /// Raw light byte before scaling (kept for saving).
    pub light_raw: u8,
    /// The four bytes Zircon skips (offsets 9..12 and 13 of the record).
    pub padding: [u8; 4],
}

impl Cell {
    /// Set the front animation byte, keeping the decoded view in sync.
    pub fn set_front_anim(&mut self, raw: u8) {
        self.front_anim_raw = raw;
        self.front_anim = (if raw == 255 { 0 } else { raw }) & 0x8F;
    }
    /// Set the light level (0..=30, even values), keeping the raw byte in sync.
    pub fn set_light(&mut self, light: u8) {
        self.light_raw = (self.light_raw & 0xF0) | ((light / 2) & 0x0F);
        self.light = (self.light_raw & 0x0F) * 2;
    }
    pub fn is_walkable(&self) -> bool {
        (self.flag & 0x01) == 1 && (self.flag & 0x02) == 2
    }
    pub fn middle_anim_count(&self) -> u8 {
        self.middle_anim & 0x0F
    }
    pub fn middle_anim_blend(&self) -> bool {
        self.middle_anim & 0x80 != 0
    }
    /// Zircon animates a middle tile only when `1 < anim < 255`.
    pub fn middle_animated(&self) -> bool {
        self.middle_anim > 1 && self.middle_anim < 255
    }
    pub fn front_anim_count(&self) -> u8 {
        self.front_anim & 0x0F
    }
    pub fn front_anim_blend(&self) -> bool {
        self.front_anim & 0x80 != 0
    }
    pub fn front_animated(&self) -> bool {
        self.front_anim > 1 && self.front_anim < 255
    }
}

#[derive(Debug, Clone)]
pub struct MapFile {
    pub width: u16,
    pub height: u16,
    cells: Vec<Cell>,
    /// The 28-byte header verbatim (width/height are rewritten on save).
    header: Vec<u8>,
    /// Bytes after the last cell record, if any.
    trailer: Vec<u8>,
}

impl MapFile {
    pub fn load(path: impl AsRef<Path>) -> Result<MapFile> {
        let data = std::fs::read(path)?;
        MapFile::parse(&data)
    }

    pub fn parse(data: &[u8]) -> Result<MapFile> {
        if data.len() < 28 {
            return Err(FormatError::Eof(data.len()));
        }
        let mut c = Cursor::at(data, 22);
        let width = c.i16()?;
        let height = c.i16()?;
        if width <= 0 || height <= 0 {
            return Err(FormatError::Invalid("map dimensions"));
        }
        let (w, h) = (width as usize, height as usize);
        let end = 28 + (w / 2) * (h / 2) * 3 + w * h * 14;
        if data.len() < end {
            return Err(FormatError::Eof(data.len()));
        }
        let mut cells = vec![Cell::default(); w * h];
        c.seek(28);
        for x in 0..w / 2 {
            for y in 0..h / 2 {
                let back_file = c.u8()?;
                let back_image = c.u16()?;
                let cell = &mut cells[(y * 2) * w + x * 2];
                cell.back_file = back_file;
                cell.back_image = back_image;
            }
        }
        for x in 0..w {
            for y in 0..h {
                let cell = &mut cells[y * w + x];
                cell.flag = c.u8()?;
                cell.middle_anim = c.u8()?;
                let v = c.u8()?;
                cell.front_anim_raw = v;
                cell.front_anim = (if v == 255 { 0 } else { v }) & 0x8F;
                cell.front_file = c.u8()?;
                cell.middle_file = c.u8()?;
                cell.middle_image = c.u16()?;
                cell.front_image = c.u16()?;
                let pad = c.bytes(3)?;
                cell.padding[..3].copy_from_slice(pad);
                let l = c.u8()?;
                cell.light_raw = l;
                cell.light = (l & 0x0F) * 2;
                cell.padding[3] = c.u8()?;
            }
        }
        Ok(MapFile {
            width: width as u16,
            height: height as u16,
            cells,
            header: data[..28].to_vec(),
            trailer: data[end.min(data.len())..].to_vec(),
        })
    }

    /// A blank map: every cell unwalkable with no tiles.
    pub fn blank(width: u16, height: u16) -> MapFile {
        MapFile {
            width,
            height,
            cells: vec![Cell::default(); width as usize * height as usize],
            header: vec![0; 28],
            trailer: Vec::new(),
        }
    }

    /// Re-encode the map; `parse(to_bytes())` is byte-identical for shipped maps.
    pub fn to_bytes(&self) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut out = Writer::default();
        out.bytes(&self.header[..22]);
        out.i16(self.width as i16);
        out.i16(self.height as i16);
        out.bytes(&self.header[26..28]);
        for x in 0..w / 2 {
            for y in 0..h / 2 {
                let cell = &self.cells[(y * 2) * w + x * 2];
                out.u8(cell.back_file);
                out.u16(cell.back_image);
            }
        }
        for x in 0..w {
            for y in 0..h {
                let cell = &self.cells[y * w + x];
                out.u8(cell.flag);
                out.u8(cell.middle_anim);
                out.u8(cell.front_anim_raw);
                out.u8(cell.front_file);
                out.u8(cell.middle_file);
                out.u16(cell.middle_image);
                out.u16(cell.front_image);
                out.bytes(&cell.padding[..3]);
                out.u8(cell.light_raw);
                out.u8(cell.padding[3]);
            }
        }
        out.bytes(&self.trailer);
        out.data
    }

    /// Save with a timestamped backup of the previous file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<Option<std::path::PathBuf>> {
        crate::save_with_backup(path, &self.to_bytes())
    }

    pub fn cell_mut(&mut self, x: i32, y: i32) -> Option<&mut Cell> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some(&mut self.cells[y as usize * self.width as usize + x as usize])
    }

    pub fn cell(&self, x: i32, y: i32) -> Option<&Cell> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some(&self.cells[y as usize * self.width as usize + x as usize])
    }

    pub fn is_walkable(&self, x: i32, y: i32) -> bool {
        self.cell(x, y).map(|c| c.is_walkable()).unwrap_or(false)
    }

    pub fn walkable_count(&self) -> usize {
        self.cells.iter().filter(|c| c.is_walkable()).count()
    }
}

/// Zircon `Libraries.KROrder`: map-file library byte -> library name (file stem
/// under `Data/Map Data/`, with an optional theme subfolder).
pub fn kr_order(file: u8) -> Option<(&'static str, &'static str)> {
    const BASE: [&str; 12] = [
        "Tilesc",
        "Tiles30c",
        "Tiles5c",
        "SmTilesc",
        "Housesc",
        "Cliffsc",
        "Dungeonsc",
        "Innersc",
        "Furnituresc",
        "Wallsc",
        "SmObjectsc",
        "Animationsc",
    ];
    match file {
        0..=11 => Some(("", BASE[file as usize])),
        12 => Some(("", "Object1c")),
        13 => Some(("", "Object2c")),
        15..=26 => Some(("Wood", BASE[(file - 15) as usize])),
        30..=41 => Some(("Sand", BASE[(file - 30) as usize])),
        45..=56 => Some(("Snow", BASE[(file - 45) as usize])),
        60..=71 => Some(("Forest", BASE[(file - 60) as usize])),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kr_order_table() {
        assert_eq!(kr_order(0), Some(("", "Tilesc")));
        assert_eq!(kr_order(13), Some(("", "Object2c")));
        assert_eq!(kr_order(14), None);
        assert_eq!(kr_order(15), Some(("Wood", "Tilesc")));
        assert_eq!(kr_order(41), Some(("Sand", "Animationsc")));
        assert_eq!(kr_order(60), Some(("Forest", "Tilesc")));
        assert_eq!(kr_order(71), Some(("Forest", "Animationsc")));
        assert_eq!(kr_order(72), None);
    }

    #[test]
    fn blank_map_round_trips_and_edits() {
        let mut m = MapFile::blank(6, 4);
        assert!(!m.is_walkable(1, 1));
        let c = m.cell_mut(1, 1).unwrap();
        c.flag = 0x03;
        c.set_light(10);
        c.set_front_anim(0x83);
        let back = m.to_bytes();
        assert_eq!(back.len(), 28 + 3 * 2 * 3 + 6 * 4 * 14);
        let again = MapFile::parse(&back).unwrap();
        assert!(again.is_walkable(1, 1));
        assert_eq!(again.cell(1, 1).unwrap().light, 10);
        assert_eq!(again.cell(1, 1).unwrap().front_anim, 0x83);
        assert_eq!(again.to_bytes(), back);
    }

    #[test]
    fn truncated_map_is_an_error_not_an_abort() {
        let bytes = MapFile::blank(6, 4).to_bytes();
        for cut in [0, 5, 27, 28, bytes.len() / 2, bytes.len() - 1] {
            assert!(MapFile::parse(&bytes[..cut]).is_err(), "cut {cut}");
        }
        // A header claiming a huge map with no cell data behind it.
        let mut huge = bytes[..28].to_vec();
        huge[22..24].copy_from_slice(&i16::MAX.to_le_bytes());
        huge[24..26].copy_from_slice(&i16::MAX.to_le_bytes());
        assert!(MapFile::parse(&huge).is_err());
    }

    #[test]
    fn every_shipped_map_round_trips_byte_identical() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            eprintln!("ZIRCON_ASSETS not set; skipping");
            return;
        };
        let dir = Path::new(&assets).join("Map");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("map") {
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            let map = MapFile::parse(&data).unwrap();
            assert!(
                map.to_bytes() == data,
                "{} changed on round trip",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 100, "maps checked: {checked}");
    }

    #[test]
    fn parses_map_zero() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            eprintln!("ZIRCON_ASSETS not set; skipping");
            return;
        };
        let map = MapFile::load(Path::new(&assets).join("Map/0.map")).unwrap();
        assert_eq!((map.width, map.height), (350, 350));
        assert!(map.walkable_count() > 1000);
    }
}
