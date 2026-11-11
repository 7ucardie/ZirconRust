//! Byte-preserving map edits with bounded gesture undo and observed adjacency.
use mir_formats::map::{Cell, MapFile};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Back,
    Middle,
    Front,
    Collision,
    Light,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tile {
    pub file: u8,
    pub image: u16,
    pub animation: u8,
}
impl Tile {
    pub fn read(c: &Cell, layer: Layer) -> Self {
        match layer {
            Layer::Back => Self {
                file: c.back_file,
                image: c.back_image,
                animation: 0,
            },
            Layer::Middle => Self {
                file: c.middle_file,
                image: c.middle_image,
                animation: c.middle_anim,
            },
            _ => Self {
                file: c.front_file,
                image: c.front_image,
                animation: c.front_anim_raw,
            },
        }
    }
    fn write(self, c: &mut Cell, layer: Layer) {
        match layer {
            Layer::Back => {
                c.back_file = self.file;
                c.back_image = self.image;
            }
            Layer::Middle => {
                c.middle_file = self.file;
                c.middle_image = self.image;
                c.middle_anim = self.animation;
            }
            Layer::Front => {
                c.front_file = self.file;
                c.front_image = self.image;
                c.set_front_anim(self.animation);
            }
            _ => {}
        }
    }
}
struct Undo {
    revision: u64,
    cells: Vec<(i32, i32, Cell)>,
}
pub struct Document {
    pub map: MapFile,
    pub path: Option<PathBuf>,
    revision: u64,
    saved: u64,
    next: u64,
    undo: Vec<Undo>,
    disk_bytes: Option<Vec<u8>>,
}
impl Document {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            disk_bytes: Some(std::fs::read(&path)?),
            map: MapFile::load(&path)?,
            path: Some(path.as_ref().to_owned()),
            revision: 0,
            saved: 0,
            next: 1,
            undo: vec![],
        })
    }
    pub fn blank(w: u16, h: u16) -> Self {
        let mut map = MapFile::blank(w, h);
        for x in 0..w as i32 {
            for y in 0..h as i32 {
                let c = map.cell_mut(x, y).unwrap();
                if x % 2 == 0 && y % 2 == 0 && x + 1 < w as i32 && y + 1 < h as i32 {
                    c.back_file = 255;
                }
                c.flag = 3;
            }
        }
        Self {
            map,
            disk_bytes: None,
            path: None,
            revision: 1,
            saved: 0,
            next: 2,
            undo: vec![],
        }
    }
    pub fn dirty(&self) -> bool {
        self.revision != self.saved
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn save(&mut self, path: &Path) -> anyhow::Result<Option<PathBuf>> {
        if self.path.as_deref() == Some(path) {
            if let Some(expected) = &self.disk_bytes {
                anyhow::ensure!(
                    std::fs::read(path)? == *expected,
                    "The map changed on disk. Save to a new path or reopen it before overwriting."
                );
            }
        }
        let backup = self.map.save(path)?;
        self.disk_bytes = Some(self.map.to_bytes());
        self.path = Some(path.to_owned());
        self.saved = self.revision;
        Ok(backup)
    }
    fn commit(&mut self, cells: Vec<(i32, i32, Cell)>) {
        if cells.is_empty() {
            return;
        }
        self.undo.push(Undo {
            revision: self.revision,
            cells,
        });
        self.revision = self.next;
        self.next += 1;
        while self.undo.len() > 50
            || (self.undo.len() > 1
                && self.undo.iter().map(|u| u.cells.len()).sum::<usize>() > 1_000_000)
        {
            self.undo.remove(0);
        }
    }
    pub fn undo(&mut self) {
        if let Some(u) = self.undo.pop() {
            for (x, y, c) in u.cells {
                *self.map.cell_mut(x, y).unwrap() = c;
            }
            self.revision = u.revision;
        }
    }
    pub fn paint(
        &mut self,
        points: &BTreeSet<(i32, i32)>,
        layer: Layer,
        tile: Tile,
        walkable: bool,
        light: u8,
        auto: Option<&Adjacency>,
    ) {
        let step = if layer == Layer::Back { 2 } else { 1 };
        let points: BTreeSet<_> = points
            .iter()
            .map(|&(x, y)| (x.div_euclid(step) * step, y.div_euclid(step) * step))
            .collect();
        let mut changed = vec![];
        for (x, y) in points {
            if layer == Layer::Back
                && (x + 1 >= self.map.width as i32 || y + 1 >= self.map.height as i32)
            {
                continue;
            }
            let Some(old) = self.map.cell(x, y).copied() else {
                continue;
            };
            let mut c = old;
            match layer {
                Layer::Collision => c.flag = (c.flag & !3) | if walkable { 3 } else { 0 },
                Layer::Light => c.set_light(light),
                _ => auto
                    .map(|a| a.choose(&self.map, x, y, layer, tile))
                    .unwrap_or(tile)
                    .write(&mut c, layer),
            }
            if c != old {
                *self.map.cell_mut(x, y).unwrap() = c;
                changed.push((x, y, old));
            }
        }
        self.commit(changed);
    }
    pub fn stamp(&mut self, at: (i32, i32), stamp: &Stamp) {
        let (x0, y0) = (at.0 & !1, at.1 & !1);
        let mut changed = vec![];
        for &(x, y, c) in &stamp.cells {
            if let Some(dest) = self.map.cell_mut(x0 + x, y0 + y) {
                if *dest != c {
                    changed.push((x0 + x, y0 + y, *dest));
                    *dest = c;
                }
            }
        }
        self.commit(changed);
    }
}

pub fn rectangle(map: &MapFile, a: (i32, i32), b: (i32, i32)) -> BTreeSet<(i32, i32)> {
    let mut cells = BTreeSet::new();
    for x in a.0.min(b.0).max(0)..=a.0.max(b.0).min(map.width as i32 - 1) {
        for y in a.1.min(b.1).max(0)..=a.1.max(b.1).min(map.height as i32 - 1) {
            cells.insert((x, y));
        }
    }
    cells
}
/// Fill gaps between sampled pointer positions during a pencil gesture.
pub fn line(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let n = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
    (0..=n)
        .map(|i| {
            (
                a.0 + ((b.0 - a.0) as f64 * i as f64 / n as f64).round() as i32,
                a.1 + ((b.1 - a.1) as f64 * i as f64 / n as f64).round() as i32,
            )
        })
        .collect()
}
#[derive(Clone)]
pub struct Stamp {
    pub cells: Vec<(i32, i32, Cell)>,
}
impl Stamp {
    pub fn capture(map: &MapFile, a: (i32, i32), b: (i32, i32)) -> Self {
        let start = (a.0.min(b.0).max(0) & !1, a.1.min(b.1).max(0) & !1);
        let end = (a.0.max(b.0) | 1, a.1.max(b.1) | 1);
        Self {
            cells: rectangle(map, start, end)
                .into_iter()
                .map(|(x, y)| (x - start.0, y - start.1, *map.cell(x, y).unwrap()))
                .collect(),
        }
    }
}

#[derive(Default)]
pub struct Adjacency {
    pairs: BTreeMap<(Tile, Tile, u8), u32>,
    candidates: BTreeSet<Tile>,
}
impl Adjacency {
    pub fn learn(map: &MapFile, layer: Layer, file: u8) -> Self {
        let mut a = Self::default();
        let step = if layer == Layer::Back { 2 } else { 1 };
        for x in (0..map.width as i32).step_by(step as usize) {
            for y in (0..map.height as i32).step_by(step as usize) {
                let tile = Tile::read(map.cell(x, y).unwrap(), layer);
                if tile.file != file {
                    continue;
                }
                a.candidates.insert(tile);
                for (dir, (dx, dy)) in [(step, 0), (0, step), (-step, 0), (0, -step)]
                    .into_iter()
                    .enumerate()
                {
                    if let Some(c) = map.cell(x + dx, y + dy) {
                        *a.pairs
                            .entry((tile, Tile::read(c, layer), dir as u8))
                            .or_default() += 1;
                    }
                }
            }
        }
        a
    }
    pub fn len(&self) -> usize {
        self.candidates.len()
    }
    fn choose(&self, map: &MapFile, x: i32, y: i32, layer: Layer, fallback: Tile) -> Tile {
        let step = if layer == Layer::Back { 2 } else { 1 };
        self.candidates
            .iter()
            .copied()
            .map(|tile| {
                let score: u64 = [(step, 0), (0, step), (-step, 0), (0, -step)]
                    .into_iter()
                    .enumerate()
                    .map(|(dir, (dx, dy))| {
                        map.cell(x + dx, y + dy)
                            .map(|c| {
                                *self
                                    .pairs
                                    .get(&(tile, Tile::read(c, layer), dir as u8))
                                    .unwrap_or(&0) as u64
                            })
                            .unwrap_or(0)
                    })
                    .sum();
                (score, tile)
            })
            .filter(|(score, _)| *score > 0)
            .max_by_key(|(score, tile)| (*score, std::cmp::Reverse(*tile)))
            .map(|(_, tile)| tile)
            .unwrap_or(fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tile(image: u16) -> Tile {
        Tile {
            file: 1,
            image,
            animation: 0,
        }
    }
    #[test]
    fn back_tiles_snap_to_serialized_anchors_and_undo_is_exact() {
        let mut d = Document::blank(5, 5);
        let before = d.map.to_bytes();
        let points = rectangle(&d.map, (1, 1), (4, 4));
        d.paint(&points, Layer::Back, tile(77), true, 0, None);
        assert_eq!(d.map.cell(0, 0).unwrap().back_image, 77);
        assert_eq!(d.map.cell(2, 2).unwrap().back_image, 77);
        assert_eq!(d.map.cell(4, 4).unwrap().back_image, 0);
        let bytes = d.map.to_bytes();
        let parsed = MapFile::parse(&bytes).unwrap();
        assert_eq!(parsed.to_bytes(), bytes);
        assert_eq!(parsed.cell(2, 2).unwrap().back_image, 77);
        d.undo();
        assert_eq!(d.map.to_bytes(), before);
    }
    #[test]
    fn property_brushes_preserve_unrelated_bits_and_other_layers() {
        let mut d = Document::blank(4, 4);
        let c = d.map.cell_mut(1, 1).unwrap();
        c.flag = 0xAC;
        c.light_raw = 0xB0;
        c.padding = [11, 22, 33, 44];
        c.front_image = 123;
        let points = BTreeSet::from([(1, 1)]);
        d.paint(&points, Layer::Collision, tile(0), true, 0, None);
        d.paint(&points, Layer::Light, tile(0), true, 10, None);
        d.paint(
            &points,
            Layer::Middle,
            Tile {
                animation: 0x84,
                ..tile(45)
            },
            true,
            0,
            None,
        );
        let c = d.map.cell(1, 1).unwrap();
        assert_eq!((c.flag, c.light_raw, c.light), (0xAF, 0xB5, 10));
        assert_eq!(
            (c.middle_image, c.middle_anim, c.front_image),
            (45, 0x84, 123)
        );
        assert_eq!(c.padding, [11, 22, 33, 44]);
        let m = MapFile::parse(&d.map.to_bytes()).unwrap();
        assert_eq!(m.cell(1, 1).unwrap(), c);
    }
    #[test]
    fn stamp_clips_at_edges_preserves_cells_and_undo_restores_every_byte() {
        let mut d = Document::blank(6, 6);
        d.map.cell_mut(0, 0).unwrap().back_image = 57;
        d.map.cell_mut(1, 1).unwrap().set_front_anim(0x83);
        d.map.cell_mut(1, 1).unwrap().padding = [9, 8, 7, 6];
        let stamp = Stamp::capture(&d.map, (1, 1), (2, 2)); // aligns origin to even back grid
        let before = d.map.to_bytes();
        d.stamp((5, 5), &stamp);
        assert_eq!(d.map.cell(4, 4).unwrap().back_image, 57);
        assert_eq!(d.map.cell(5, 5).unwrap().padding, [9, 8, 7, 6]);
        assert_eq!(d.map.cell(5, 5).unwrap().front_anim_raw, 0x83);
        d.undo();
        assert_eq!(d.map.to_bytes(), before);
    }
    #[test]
    fn observed_neighbours_choose_a_tile_seen_in_the_source_pattern() {
        let mut source = Document::blank(4, 4);
        for x in 0..4 {
            for y in 0..4 {
                tile(if x % 2 == 0 { 1 } else { 2 })
                    .write(source.map.cell_mut(x, y).unwrap(), Layer::Middle);
            }
        }
        let learned = Adjacency::learn(&source.map, Layer::Middle, 1);
        assert_eq!(learned.len(), 2);
        source.map.cell_mut(1, 1).unwrap().middle_image = 99;
        source.paint(
            &BTreeSet::from([(1, 1)]),
            Layer::Middle,
            tile(99),
            true,
            0,
            Some(&learned),
        );
        assert_eq!(source.map.cell(1, 1).unwrap().middle_image, 2);
        assert_eq!(line((0, 0), (3, 0)), vec![(0, 0), (1, 0), (2, 0), (3, 0)]);
    }
    #[test]
    fn map_save_backup_dirty_state_and_external_conflicts() {
        let dir = std::env::temp_dir().join(format!("zircon-tile-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.map");
        let mut d = Document::blank(4, 4);
        d.save(&path).unwrap();
        assert!(!d.dirty());
        let original = std::fs::read(&path).unwrap();
        d.paint(
            &BTreeSet::from([(1, 1)]),
            Layer::Middle,
            tile(37),
            true,
            0,
            None,
        );
        assert!(d.dirty());
        let backup = d.save(&path).unwrap().unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), original);
        d.paint(
            &BTreeSet::from([(2, 2)]),
            Layer::Front,
            tile(99),
            true,
            0,
            None,
        );
        d.undo();
        assert!(!d.dirty());
        let mut loaded = Document::open(&path).unwrap();
        assert_eq!(loaded.map.cell(1, 1).unwrap().middle_image, 37);
        std::fs::write(&path, b"external change").unwrap();
        assert!(loaded.save(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"external change");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
