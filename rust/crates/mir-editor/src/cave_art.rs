//! Complete source-art templates. Never crop or rotate multi-cell structures.
use crate::preview::resolve_case_insensitive;
use mir_formats::{
    map::{Cell, MapFile},
    monster_table::{kr_library, library_path},
    zl::ZlLibrary,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Rect {
    x: i32,
    y: i32,
    right: i32,
    bottom: i32,
}
impl Rect {
    fn overlaps(self, b: Self) -> bool {
        self.x < b.right && b.x < self.right && self.y < b.bottom && b.y < self.bottom
    }
    fn union(self, b: Self) -> Self {
        Self {
            x: self.x.min(b.x),
            y: self.y.min(b.y),
            right: self.right.max(b.right),
            bottom: self.bottom.max(b.bottom),
        }
    }
    fn aligned(self) -> Self {
        Self {
            x: self.x & !1,
            y: self.y & !1,
            right: (self.right + 1) & !1,
            bottom: (self.bottom + 1) & !1,
        }
    }
    fn expanded(self) -> Self {
        Self {
            x: self.x - 1,
            y: self.y - 1,
            right: self.right + 1,
            bottom: self.bottom + 1,
        }
    }
    fn width(self) -> i32 {
        self.right - self.x
    }
    fn height(self) -> i32 {
        self.bottom - self.y
    }
}
struct Sprite {
    bounds: Rect,
}
pub struct Template {
    pub width: i32,
    pub height: i32,
    pub wall: bool,
    cells: Vec<Cell>,
}
#[derive(Default, Debug)]
pub struct Report {
    pub templates: usize,
    pub walls: usize,
    pub decorations: usize,
    pub rejected_large: usize,
    pub rejected_strips: usize,
}
fn root(parents: &mut [usize], mut n: usize) -> usize {
    while parents[n] != n {
        parents[n] = parents[parents[n]];
        n = parents[n];
    }
    n
}
/// Include every touching sprite and every sprite intersected by the rectangular
/// capture, then align back tiles. A bounded closure rejects whole large groups.
fn capture_bounds(seed: Rect, sprites: &[Sprite], w: i32, h: i32) -> Option<Rect> {
    let mut bounds = seed.expanded().aligned();
    loop {
        if bounds.x < 0
            || bounds.y < 0
            || bounds.right > w
            || bounds.bottom > h
            || bounds.width() > 32
            || bounds.height() > 32
        {
            return None;
        }
        let before = bounds;
        for sprite in sprites {
            if bounds.overlaps(sprite.bounds) {
                bounds = bounds.union(sprite.bounds).aligned();
            }
        }
        if bounds == before {
            return Some(bounds);
        }
    }
}

fn source_sprites(
    source: &MapFile,
    assets: &Path,
    include_back: bool,
) -> anyhow::Result<Vec<Sprite>> {
    let mut libraries: BTreeMap<u16, ZlLibrary> = BTreeMap::new();
    let mut sprites = vec![];
    for x in 0..source.width as i32 {
        for y in 0..source.height as i32 {
            let c = source.cell(x, y).unwrap();
            for (file, index, frames, animated, back) in [
                (c.back_file, c.back_image, 1, false, true),
                (
                    c.middle_file,
                    c.middle_image,
                    c.middle_anim_count(),
                    c.middle_animated(),
                    false,
                ),
                (
                    c.front_file,
                    c.front_image,
                    c.front_anim_count(),
                    c.front_animated(),
                    false,
                ),
            ] {
                if (back && (!include_back || x % 2 != 0 || y % 2 != 0)) || (!back && file == 0) {
                    continue;
                }
                let Some(lib) = kr_library(file) else {
                    continue;
                };
                if let std::collections::btree_map::Entry::Vacant(e) = libraries.entry(lib) {
                    let rel = library_path(lib)
                        .ok_or_else(|| anyhow::anyhow!("Unknown map library {lib}"))?;
                    let path = resolve_case_insensitive(assets, rel)
                        .ok_or_else(|| anyhow::anyhow!("Missing source-art library {rel}"))?;
                    e.insert(ZlLibrary::open(path)?);
                }
                let mut bounds = None;
                for frame in 0..if animated { frames.max(1) } else { 1 } {
                    let slot = index as usize + frame as usize;
                    let library = &libraries[&lib];
                    anyhow::ensure!(
                        slot < library.len(),
                        "Source image index out of range {file}:{slot}"
                    );
                    // Explicitly empty library slots draw nothing. They are different
                    // from an out-of-range reference or a missing asset library.
                    let Some(info) = library.info(slot) else {
                        continue;
                    };
                    if info.width == 0 || info.height == 0 {
                        continue;
                    }
                    let floor = (info.width == 48 && info.height == 32)
                        || (info.width == 96 && info.height == 64);
                    let top = if back || floor {
                        y * 32
                    } else {
                        (y + 1) * 32 - info.height as i32
                    };
                    let r = Rect {
                        x,
                        y: top.div_euclid(32),
                        right: x + (info.width as i32 + 47) / 48,
                        bottom: (top + info.height as i32 + 31).div_euclid(32),
                    };
                    bounds = Some(bounds.map_or(r, |b: Rect| b.union(r)));
                }
                if let Some(bounds) = bounds {
                    sprites.push(Sprite { bounds });
                }
            }
        }
    }
    Ok(sprites)
}

/// Union of complete frame extents, including artwork extending outside cells.
pub fn source_extent(source: &MapFile, assets: &Path) -> anyhow::Result<(i32, i32, i32, i32)> {
    let mut extent = Rect {
        x: 0,
        y: 0,
        right: source.width as i32,
        bottom: source.height as i32,
    };
    for sprite in source_sprites(source, assets, true)? {
        extent = extent.union(sprite.bounds);
    }
    let extent = extent.aligned();
    Ok((extent.x, extent.y, extent.right, extent.bottom))
}

/// Check a half-open, even-grid selection against every source sprite, including
/// back tiles, all animation frames and art anchored outside the selection.
/// This checks individual frames, not semantic groups of adjacent sprites.
/// A larger motif can still be cut between sprite columns; review it visually.
pub fn validate_selection(
    source: &MapFile,
    assets: &Path,
    bounds: (i32, i32, i32, i32),
) -> anyhow::Result<()> {
    let (x, y, right, bottom) = bounds;
    anyhow::ensure!(
        x >= 0
            && y >= 0
            && right > x
            && bottom > y
            && right <= source.width as i32
            && bottom <= source.height as i32
            && [x, y, right, bottom].iter().all(|n| n % 2 == 0),
        "Source selection must be a nonempty even-grid rectangle inside the map"
    );
    let selection = Rect {
        x,
        y,
        right,
        bottom,
    };
    for sprite in source_sprites(source, assets, true)? {
        anyhow::ensure!(
            !selection.overlaps(sprite.bounds) || selection.union(sprite.bounds) == selection,
            "Source selection cuts sprite bounds ({}, {}) through ({}, {})",
            sprite.bounds.x,
            sprite.bounds.y,
            sprite.bounds.right,
            sprite.bounds.bottom
        );
    }
    Ok(())
}

/// Validate a shaped selection against complete individual sprite frames.
/// Coverage is row-major and owns complete 2×2 blocks; semantic motifs still
/// need authored group checks and native visual review.
pub fn validate_covered_selection(
    source: &MapFile,
    assets: &Path,
    bounds: (i32, i32, i32, i32),
    coverage: &[bool],
) -> anyhow::Result<()> {
    let (l, t, r, b) = bounds;
    anyhow::ensure!(
        l >= 0
            && t >= 0
            && r > l
            && b > t
            && r <= i32::from(source.width)
            && b <= i32::from(source.height)
            && [l, t, r, b].iter().all(|n| n % 2 == 0),
        "Invalid covered selection bounds."
    );
    let w = (r - l) as usize;
    let h = (b - t) as usize;
    anyhow::ensure!(
        coverage.len() == w * h && coverage.iter().any(|v| *v),
        "Coverage must match the nonempty selection."
    );
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let a = coverage[y * w + x];
            anyhow::ensure!(
                a == coverage[y * w + x + 1]
                    && a == coverage[(y + 1) * w + x]
                    && a == coverage[(y + 1) * w + x + 1],
                "Coverage must own complete 2x2 blocks."
            );
        }
    }
    let owned = |x: i32, y: i32| {
        x >= l && y >= t && x < r && y < b && coverage[(y - t) as usize * w + (x - l) as usize]
    };
    for sprite in source_sprites(source, assets, true)? {
        let s = sprite.bounds;
        let mut intersects = false;
        for y in s.y.max(t)..s.bottom.min(b) {
            for x in s.x.max(l)..s.right.min(r) {
                intersects |= owned(x, y);
            }
        }
        if intersects {
            anyhow::ensure!(
                s.x >= l && s.y >= t && s.right <= r && s.bottom <= b,
                "Covered selection cuts sprite bounds."
            );
            for y in s.y..s.bottom {
                for x in s.x..s.right {
                    anyhow::ensure!(
                        owned(x, y),
                        "Covered selection cuts sprite ownership at ({x},{y})."
                    );
                }
            }
        }
    }
    Ok(())
}

pub fn extract(source: &MapFile, assets: &Path) -> anyhow::Result<(Vec<Template>, Report)> {
    let sprites = source_sprites(source, assets, false)?;
    // Union sprites through the cell grid rather than comparing every pair.
    let mut parents: Vec<_> = (0..sprites.len()).collect();
    let mut occupied = BTreeMap::new();
    for (i, sprite) in sprites.iter().enumerate() {
        let r = sprite.bounds;
        for x in r.x..=r.right {
            for y in r.y..=r.bottom {
                if let Some(&other) = occupied.get(&(x, y)) {
                    let a = root(&mut parents, i);
                    let b = root(&mut parents, other);
                    parents[a] = b;
                }
                occupied.insert((x, y), i);
            }
        }
    }
    let mut groups = BTreeMap::new();
    for (i, sprite) in sprites.iter().enumerate() {
        groups
            .entry(root(&mut parents, i))
            .and_modify(|b: &mut Rect| *b = b.union(sprite.bounds))
            .or_insert(sprite.bounds);
    }
    let mut report = Report::default();
    let mut seen = BTreeSet::new();
    let mut templates = vec![];
    for bounds in groups.into_values() {
        // An isolated tall strip can be one slice of a wall completed by baked
        // background art. Its rectangle alone is not evidence of a whole prop.
        if bounds.width() <= 2 && bounds.height() >= 4 {
            report.rejected_strips += 1;
            continue;
        }
        let Some(r) = capture_bounds(bounds, &sprites, source.width as i32, source.height as i32)
        else {
            report.rejected_large += 1;
            continue;
        };
        if !seen.insert(r) {
            continue;
        }
        let cells: Vec<_> = (r.y..r.bottom)
            .flat_map(|y| (r.x..r.right).map(move |x| *source.cell(x, y).unwrap()))
            .collect();
        let open = cells.iter().filter(|c| c.is_walkable()).count();
        if open == 0 {
            continue;
        }
        templates.push(Template {
            width: r.width(),
            height: r.height(),
            wall: open < cells.len(),
            cells,
        });
    }
    report.templates = templates.len();
    Ok((templates, report))
}

/// Copy whole templates; a failed topology check restores every original byte.
pub fn apply(map: &mut MapFile, templates: &[Template], seed: u64, report: &mut Report) {
    apply_protected(map, templates, seed, report, &BTreeSet::new());
}

pub fn apply_protected(
    map: &mut MapFile,
    templates: &[Template],
    seed: u64,
    report: &mut Report,
    protected: &BTreeSet<(i32, i32)>,
) {
    if templates.is_empty() {
        return;
    }
    let mut rng = crate::generate::Random(seed ^ 0xa0761d6478bd642f);
    let mut placed: Vec<Rect> = vec![];
    let budget = (map.width as usize * map.height as usize / 384).clamp(2, 32);
    let minimum = map.walkable_count() * 3 / 4;
    let mut wall_templates: Vec<_> = templates.iter().filter(|t| t.wall).collect();
    wall_templates.sort_by_key(|t| std::cmp::Reverse(t.width * t.height));
    let decorations: Vec<_> = templates.iter().filter(|t| !t.wall).collect();
    let wall_budget = (budget / 2).max(1);
    for attempt in 0..768 {
        if placed.len() >= budget {
            break;
        }
        let pool = if report.walls < wall_budget && !wall_templates.is_empty() && attempt < 512 {
            &wall_templates
        } else if !decorations.is_empty() {
            &decorations
        } else {
            &wall_templates
        };
        let choices = if report.walls == 0 && attempt < 128 {
            pool.len().min(8)
        } else {
            pool.len()
        };
        let template = pool[(rng.next() % choices as u64) as usize];
        let max_x = map.width as i32 - template.width - 2;
        let max_y = map.height as i32 - template.height - 2;
        if max_x < 2 || max_y < 2 {
            continue;
        }
        let x = 2 + 2 * (rng.next() % (max_x as u64 / 2)) as i32;
        let y = 2 + 2 * (rng.next() % (max_y as u64 / 2)) as i32;
        let r = Rect {
            x,
            y,
            right: x + template.width,
            bottom: y + template.height,
        };
        if placed.iter().any(|b| b.expanded().overlaps(r)) {
            continue;
        }
        let mut agree = 0;
        let mut current_open = 0;
        for (i, c) in template.cells.iter().enumerate() {
            let open =
                map.is_walkable(x + i as i32 % template.width, y + i as i32 / template.width);
            current_open += usize::from(open);
            agree += usize::from(open == c.is_walkable());
        }
        // Wall formations may meet a boundary or become an interior obstacle.
        // Decorations sit on floor. Reject substantially different collision.
        if agree * 100 < template.cells.len() * 80
            || current_open == 0
            || (!template.wall && current_open != template.cells.len())
        {
            continue;
        }
        if template.cells.iter().enumerate().any(|(i, c)| {
            !c.is_walkable()
                && protected
                    .contains(&(x + i as i32 % template.width, y + i as i32 / template.width))
        }) {
            continue;
        }
        if !try_place(map, template, x, y, minimum, seed) {
            continue;
        }
        placed.push(r);
        if template.wall {
            report.walls += 1;
        } else {
            report.decorations += 1;
        }
    }
}

fn try_place(
    map: &mut MapFile,
    template: &Template,
    x: i32,
    y: i32,
    minimum: usize,
    seed: u64,
) -> bool {
    let mut old = Vec::with_capacity(template.cells.len());
    for (i, c) in template.cells.iter().enumerate() {
        let cell = map
            .cell_mut(x + i as i32 % template.width, y + i as i32 / template.width)
            .unwrap();
        old.push(*cell);
        *cell = *c;
    }
    if map.walkable_count() >= minimum && crate::generate::layout(map, seed).is_ok() {
        return true;
    }
    for (i, c) in old.into_iter().enumerate() {
        *map.cell_mut(x + i as i32 % template.width, y + i as i32 / template.width)
            .unwrap() = c;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_selection_rejects_invalid_rectangles_before_reading_assets() {
        let source = MapFile::blank(8, 8);
        for bounds in [
            (-2, 0, 4, 4),
            (0, -2, 4, 4),
            (0, 0, 10, 4),
            (0, 0, 4, 10),
            (2, 2, 2, 4),
            (4, 2, 2, 4),
            (1, 0, 4, 4),
            (0, 0, 4, 3),
        ] {
            let error = validate_selection(&source, Path::new("missing-assets"), bounds)
                .unwrap_err()
                .to_string();
            assert!(error.contains("even-grid rectangle"), "{bounds:?}: {error}");
        }
    }

    #[test]
    fn explicit_empty_library_slots_have_no_bounds_but_invalid_indices_fail() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let mut source = MapFile::blank(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                let c = source.cell_mut(x, y).unwrap();
                c.back_file = 255;
                c.middle_file = 255;
                c.front_file = 255;
            }
        }
        source.cell_mut(0, 0).unwrap().middle_file = 21;
        source.cell_mut(0, 0).unwrap().middle_image = 0;
        validate_selection(&source, &assets, (0, 0, 4, 4)).unwrap();
        source.cell_mut(0, 0).unwrap().middle_image = u16::MAX;
        assert!(validate_selection(&source, &assets, (0, 0, 4, 4))
            .unwrap_err()
            .to_string()
            .contains("out of range"));
    }

    #[test]
    fn source_selection_accepts_complete_corner_and_rejects_cut_and_intruding_walls() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1006.map")).unwrap();
        for bounds in [(14, 2, 16, 14), (16, 2, 20, 14), (20, 4, 22, 14)] {
            validate_selection(&source, &assets, bounds).unwrap();
        }
        // Image 521 is anchored inside this crop but extends up to row 3.
        // Image 889 is anchored outside the second crop at (27,18), yet its
        // drawing reaches row 15 inside it. Both would produce incomplete art.
        for bounds in [(14, 4, 16, 14), (22, 4, 28, 16)] {
            let error = validate_selection(&source, &assets, bounds)
                .unwrap_err()
                .to_string();
            assert!(error.contains("cuts sprite bounds"), "{bounds:?}: {error}");
        }
    }

    #[test]
    fn capture_expands_for_complete_art_and_rejects_oversized_composites() {
        let sprites = vec![
            Sprite {
                bounds: Rect {
                    x: 8,
                    y: 6,
                    right: 10,
                    bottom: 14,
                },
            },
            Sprite {
                bounds: Rect {
                    x: 10,
                    y: 4,
                    right: 12,
                    bottom: 14,
                },
            },
        ];
        let r = capture_bounds(sprites[0].bounds, &sprites, 64, 64).unwrap();
        assert!(r.y <= 4 && r.right >= 12 && r.bottom >= 14);
        assert_eq!(r.x % 2, 0);
        assert_eq!(r.y % 2, 0);
        let huge = vec![Sprite {
            bounds: Rect {
                x: 0,
                y: 0,
                right: 64,
                bottom: 64,
            },
        }];
        assert!(capture_bounds(sprites[0].bounds, &huge, 64, 64).is_none());
    }
    #[test]
    fn rejected_templates_leave_the_map_byte_identical() {
        let mut map = MapFile::blank(32, 32);
        for x in 2..30 {
            for y in 2..30 {
                map.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        let before = map.to_bytes();
        // Whole decorations may be placed on open floor.
        let t = Template {
            width: 4,
            height: 4,
            wall: false,
            cells: vec![
                Cell {
                    flag: 3,
                    ..Cell::default()
                };
                16
            ],
        };
        // Decoration on existing floor is valid; an oversized template never fits.
        let huge = Template {
            width: 64,
            height: 64,
            wall: true,
            cells: vec![Cell::default(); 4096],
        };
        apply(&mut map, &[huge], 42, &mut Report::default());
        assert_eq!(map.to_bytes(), before);
        let mut report = Report::default();
        apply(&mut map, &[t], 42, &mut report);
        assert!(report.decorations > 0);
        assert_eq!(map.walkable_count(), 28 * 28);
    }
    #[test]
    fn closing_a_corridor_rolls_back_all_layers_and_disconnected_islands_are_rejected() {
        let mut map = MapFile::blank(32, 32);
        for x in 2..30 {
            for y in 2..30 {
                if !(12..20).contains(&x) || y == 16 {
                    let c = map.cell_mut(x, y).unwrap();
                    c.flag = 3;
                    c.back_image = 17;
                    c.padding = [1, 2, 3, 4];
                }
            }
        }
        let before = map.to_bytes();
        let cells = (14..18)
            .flat_map(|y| (14..18).map(move |x| (x, y)))
            .map(|(x, y)| *map.cell(x, y).unwrap())
            .collect();
        let mut template = Template {
            width: 4,
            height: 4,
            wall: true,
            cells,
        };
        template.cells[8].flag = 0; // block the only joining corridor
        template.cells[8].back_image = 99;
        assert!(!try_place(&mut map, &template, 14, 14, 64, 42));
        assert_eq!(map.to_bytes(), before);
        map.cell_mut(0, 0).unwrap().flag = 3;
        assert!(crate::generate::layout(&map, 42).is_err());
    }
}
