//! Seeded cave topology, observed 2×2 art patches, and validated content bundles.
use crate::tile_doc::Document;
use mir_formats::{
    map::MapFile,
    mirdb::{MirDb, Value},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
};
type Point = (i32, i32);
const DIRS: [Point; 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    pub start: Point,
    pub exits: [Point; 2],
    pub landings: [Point; 2],
    pub safe: Vec<Point>,
    pub spawns: Vec<Point>,
    pub seed: u64,
    #[serde(default)]
    pub enclosed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_repeats: Option<[u16; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wall_room_repeats: Vec<[u16; 2]>,
    #[serde(default)]
    pub wall_rooms: u8,
    #[serde(default)]
    pub rooms: Vec<crate::dungeon::Room>,
    #[serde(default)]
    pub corridors: Vec<Point>,
    #[serde(default)]
    pub corridor_width: u16,
    #[serde(default)]
    pub drop_set: i32,
    #[serde(default)]
    pub easter_event_chance: i32,
}
pub(crate) struct Random(pub(crate) u64);
impl Random {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut n = self.0;
        n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
        n ^ (n >> 31)
    }
}
fn connected(start: Point, open: &BTreeSet<Point>) -> Vec<Point> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    if open.contains(&start) {
        seen.insert(start);
        queue.push_back(start);
    }
    let mut result = vec![];
    while let Some(p) = queue.pop_front() {
        result.push(p);
        for (dx, dy) in DIRS {
            let q = (p.0 + dx, p.1 + dy);
            if open.contains(&q) && seen.insert(q) {
                queue.push_back(q);
            }
        }
    }
    result
}
fn walkable(map: &MapFile) -> BTreeSet<Point> {
    (0..map.width as i32)
        .flat_map(|x| (0..map.height as i32).map(move |y| (x, y)))
        .filter(|&(x, y)| map.is_walkable(x, y))
        .collect()
}
impl Layout {
    pub fn navigation(&self, map: &MapFile) -> anyhow::Result<mir_formats::navigation::Navigation> {
        use mir_formats::navigation::{Navigation, Portal};
        self.validate(map)?;
        Ok(Navigation::new(
            map,
            vec![
                Portal {
                    position: self.exits[0],
                    destination: self.landings[1],
                },
                Portal {
                    position: self.exits[1],
                    destination: self.landings[0],
                },
            ],
        )?)
    }

    pub fn validate(&self, map: &MapFile) -> anyhow::Result<usize> {
        anyhow::ensure!(
            self.easter_event_chance >= 0,
            "Easter marking chance must be nonnegative."
        );
        if let Some(repeats) = self.wall_repeats {
            anyhow::ensure!(
                self.enclosed && repeats.iter().all(|n| (3..=10).contains(n)),
                "Wall repeats require an enclosed layout and counts between 3 and 10."
            );
        }
        anyhow::ensure!(
            self.wall_room_repeats.is_empty() || self.wall_repeats.is_none(),
            "Wall room metadata cannot mix uniform and varied repeats."
        );
        anyhow::ensure!(
            self.wall_rooms <= 3
                && (self.wall_rooms == 0
                    || self.wall_repeats.is_some()
                    || !self.wall_room_repeats.is_empty()),
            "Wall room metadata requires a wall recipe with one to three rooms."
        );
        if !self.wall_room_repeats.is_empty() {
            anyhow::ensure!(
                self.enclosed,
                "Varied wall rooms require an enclosed layout."
            );
            anyhow::ensure!(
                usize::from(self.wall_rooms) == self.wall_room_repeats.len(),
                "Wall room count does not match varied repeats."
            );
            crate::wood_connected::validate_varied_parameters(&self.wall_room_repeats)?;
        } else if self.wall_rooms >= 2 {
            let [nw, ne] = self
                .wall_repeats
                .ok_or_else(|| anyhow::anyhow!("Connected rooms require wall repeats"))?;
            crate::wood_connected::validate_parameters(nw, ne, self.wall_rooms)?;
        }
        let open = walkable(map);
        anyhow::ensure!(open.len() >= 64, "At least 64 walkable cells are required.");
        if self.enclosed {
            anyhow::ensure!(
                open.iter().all(|&(x, y)| x > 0
                    && y > 0
                    && x + 1 < i32::from(map.width)
                    && y + 1 < i32::from(map.height)),
                "Enclosed layouts must have blocked border cells."
            );
        }
        let reached: BTreeSet<_> = connected(self.start, &open).into_iter().collect();
        anyhow::ensure!(
            reached.len() == open.len(),
            "The cave contains unreachable floor cells or the start is blocked."
        );
        anyhow::ensure!(self.exits[0] != self.exits[1], "Portals must be distinct.");
        anyhow::ensure!(
            !self.safe.is_empty() && self.spawns.len() >= 8,
            "A safe zone and at least eight spawn cells are required."
        );
        for p in self
            .safe
            .iter()
            .chain(&self.spawns)
            .chain(&self.exits)
            .chain(&self.landings)
        {
            anyhow::ensure!(
                reached.contains(p),
                "Content at {},{} is blocked or unreachable.",
                p.0,
                p.1
            );
        }
        anyhow::ensure!(
            self.landings.iter().all(|p| !self.exits.contains(p)),
            "Portal landings must not trigger another portal."
        );
        anyhow::ensure!(
            self.rooms.is_empty()
                || (!self.corridors.is_empty() && [2, 4, 6].contains(&self.corridor_width)),
            "Dungeon corridor metadata is missing or invalid."
        );
        for room in &self.rooms {
            anyhow::ensure!(
                room.x >= 0
                    && room.y >= 0
                    && room.width > 0
                    && room.height > 0
                    && room.x <= map.width as i32 - room.width
                    && room.y <= map.height as i32 - room.height,
                "Room is outside the map."
            );
            anyhow::ensure!(
                reached.contains(&room.center()),
                "A room center is blocked or unreachable."
            );
        }
        anyhow::ensure!(
            self.corridors.iter().all(|p| reached.contains(p)),
            "A corridor is blocked or unreachable."
        );
        let safe: BTreeSet<_> = self.safe.iter().collect();
        anyhow::ensure!(
            !self
                .spawns
                .iter()
                .any(|p| safe.contains(p) || self.exits.contains(p) || self.landings.contains(p)),
            "Spawn cells overlap the safe zone or a portal."
        );
        anyhow::ensure!(
            self.safe.contains(&self.start),
            "Start must be inside the safe zone."
        );
        Ok(open.len())
    }
}

pub fn cave(
    source: &MapFile,
    width: u16,
    height: u16,
    seed: u64,
) -> anyhow::Result<(Document, Layout)> {
    anyhow::ensure!(
        (32..=256).contains(&width)
            && (32..=256).contains(&height)
            && width.is_multiple_of(2)
            && height.is_multiple_of(2),
        "Cave dimensions must be even, between 32 and 256."
    );
    let (w, h) = (width as i32 / 2, height as i32 / 2);
    let mut rng = Random(seed);
    let mut floor = BTreeSet::new();
    for x in 1..w - 1 {
        for y in 1..h - 1 {
            if rng.next() % 100 < 58 {
                floor.insert((x, y));
            }
        }
    }
    for _ in 0..4 {
        let mut next = BTreeSet::new();
        for x in 1..w - 1 {
            for y in 1..h - 1 {
                let count = (-1..=1)
                    .flat_map(|dx| (-1..=1).map(move |dy| (x + dx, y + dy)))
                    .filter(|p| floor.contains(p))
                    .count();
                if count >= 5 {
                    next.insert((x, y));
                }
            }
        }
        floor = next;
    }
    let mut remaining = floor.clone();
    let mut largest = vec![];
    while let Some(&p) = remaining.first() {
        let component = connected(p, &floor);
        for q in &component {
            remaining.remove(q);
        }
        if component.len() > largest.len() {
            largest = component;
        }
    }
    anyhow::ensure!(
        largest.len() >= 32,
        "Seed produced too little connected floor; try another seed."
    );
    floor = largest.into_iter().collect();
    let doc = paint_floor(source, width, height, &floor, &mut rng)?;
    let layout = layout(&doc.map, seed)?;
    Ok((doc, layout))
}

pub(crate) fn paint_floor(
    source: &MapFile,
    width: u16,
    height: u16,
    floor: &BTreeSet<Point>,
    rng: &mut Random,
) -> anyhow::Result<Document> {
    let pool = floor_patches(source)?;
    let mut doc = Document::blank(width, height);
    for x in 0..i32::from(width) / 2 {
        for y in 0..i32::from(height) / 2 {
            if !floor.contains(&(x, y)) {
                for dx in 0..2 {
                    for dy in 0..2 {
                        doc.map.cell_mut(x * 2 + dx, y * 2 + dy).unwrap().flag = 0;
                    }
                }
                continue;
            }
            let mask = DIRS.iter().enumerate().fold(0u8, |bits, (i, &(dx, dy))| {
                bits | (u8::from(floor.contains(&(x + dx, y + dy))) << i)
            });
            let best = pool
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.is_empty())
                .map(|(i, _)| (i as u8 ^ mask).count_ones())
                .min()
                .unwrap();
            let candidates: Vec<_> = pool
                .iter()
                .enumerate()
                .filter(|(i, b)| !b.is_empty() && (*i as u8 ^ mask).count_ones() == best)
                .map(|(_, b)| b)
                .collect();
            let count: usize = candidates.iter().map(|b| b.len()).sum();
            let mut pick = (rng.next() % count as u64) as usize;
            let &(sx, sy) = candidates
                .iter()
                .find_map(|b| {
                    if pick < b.len() {
                        Some(&b[pick])
                    } else {
                        pick -= b.len();
                        None
                    }
                })
                .unwrap();
            for dx in 0..2 {
                for dy in 0..2 {
                    *doc.map.cell_mut(x * 2 + dx, y * 2 + dy).unwrap() =
                        *source.cell(sx + dx, sy + dy).unwrap();
                }
            }
        }
    }
    Ok(doc)
}

fn floor_patches(source: &MapFile) -> anyhow::Result<[Vec<Point>; 16]> {
    // Use complete undecorated floor patches. Large wall/object composites span
    // multiple cells and cannot safely be shuffled as independent 2×2 blocks.
    let mut pool: [Vec<Point>; 16] = std::array::from_fn(|_| vec![]);
    for x in (0..source.width as i32 - 1).step_by(2) {
        for y in (0..source.height as i32 - 1).step_by(2) {
            let count = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .iter()
                .filter(|&&(dx, dy)| source.is_walkable(x + dx, y + dy))
                .count();
            if count != 4
                || [(0, 0), (1, 0), (0, 1), (1, 1)].iter().any(|&(dx, dy)| {
                    let c = source.cell(x + dx, y + dy).unwrap();
                    c.middle_file != 0 || c.front_file != 0
                })
            {
                continue;
            }
            let c = source.cell(x, y).unwrap();
            if mir_formats::map::kr_order(c.back_file).is_none() {
                continue;
            }
            let mask = DIRS.iter().enumerate().fold(0u8, |bits, (i, &(dx, dy))| {
                bits | (u8::from(source.is_walkable(x + dx * 2, y + dy * 2)) << i)
            });
            pool[mask as usize].push((x, y));
        }
    }
    anyhow::ensure!(
        pool.iter().any(|b| !b.is_empty()),
        "Source needs complete 2×2 walkable floor patches without middle/front objects."
    );
    Ok(pool)
}

/// Opt-in floor tiling with observed directional adjacency. Returns an error
/// for unsatisfiable sources or the bounded search limit; never weakens rules.
pub(crate) fn paint_floor_compatible(
    source: &MapFile,
    width: u16,
    height: u16,
    floor: &BTreeSet<Point>,
    rng: &mut Random,
) -> anyhow::Result<Document> {
    let (w, h) = (width as i32 / 2, height as i32 / 2);
    let pool = floor_patches(source)?;
    // Learn only observed directional pairs between eligible complete patches.
    // Identity is the back tile; the chosen origin still supplies all four raw cells.
    let mut groups = BTreeMap::<(u8, u16), Vec<(u8, Point)>>::new();
    for (mask, origins) in pool.iter().enumerate() {
        for &origin in origins {
            let cell = source.cell(origin.0, origin.1).unwrap();
            groups
                .entry((cell.back_file, cell.back_image))
                .or_default()
                .push((mask as u8, origin));
        }
    }
    let patches: Vec<_> = groups.into_values().collect();
    let origins: BTreeMap<_, _> = patches
        .iter()
        .enumerate()
        .flat_map(|(id, origins)| origins.iter().map(move |&(_, (x, y))| ((x / 2, y / 2), id)))
        .collect();
    let mut pairs = BTreeSet::new();
    for (&(x, y), &id) in &origins {
        for (direction, &(dx, dy)) in DIRS.iter().enumerate() {
            if let Some(&neighbor) = origins.get(&(x + dx, y + dy)) {
                pairs.insert((id, neighbor, direction));
            }
        }
    }
    let positions: Vec<_> = floor.iter().copied().collect();
    let salts: Vec<_> = positions.iter().map(|_| rng.next()).collect();
    let masks: Vec<_> = positions
        .iter()
        .map(|&(x, y)| {
            DIRS.iter().enumerate().fold(0u8, |bits, (i, &(dx, dy))| {
                bits | (u8::from(floor.contains(&(x + dx, y + dy))) << i)
            })
        })
        .collect();
    let score = |id: usize, mask: u8| {
        patches[id]
            .iter()
            .map(|(candidate, _)| (candidate ^ mask).count_ones())
            .min()
            .unwrap()
    };
    let mut selected = BTreeMap::new();
    let mut choices = Vec::<(Vec<usize>, usize)>::new();
    let mut cursor = 0;
    let mut attempts = 0;
    // ponytail: bounded depth-first search, no general constraint solver. Fail
    // explicitly at the budget; introduce propagation only if real sources need it.
    while cursor < positions.len() {
        if choices.len() == cursor {
            let mut candidates: Vec<_> = (0..patches.len()).collect();
            candidates.sort_by_cached_key(|&id| {
                (
                    score(id, masks[cursor]),
                    Random(salts[cursor] ^ id as u64).next(),
                )
            });
            choices.push((candidates, 0));
        }
        let (candidates, next) = &mut choices[cursor];
        if *next == candidates.len() {
            choices.pop();
            anyhow::ensure!(cursor > 0, "Source floor tile adjacency cannot satisfy this layout; choose another floor source.");
            cursor -= 1;
            selected.remove(&positions[cursor]);
            continue;
        }
        let id = candidates[*next];
        *next += 1;
        attempts += 1;
        anyhow::ensure!(attempts <= 1_000_000, "Floor adjacency search exceeded 1000000 candidate checks; use a smaller layout or another floor source.");
        let (x, y) = positions[cursor];
        if DIRS.iter().enumerate().all(|(direction, &(dx, dy))| {
            selected
                .get(&(x + dx, y + dy))
                .is_none_or(|&neighbor| pairs.contains(&(id, neighbor, direction)))
        }) {
            selected.insert((x, y), id);
            cursor += 1;
        }
    }
    let mut doc = Document::blank(width, height);
    for x in 0..w {
        for y in 0..h {
            let Some(&id) = selected.get(&(x, y)) else {
                for dx in 0..2 {
                    for dy in 0..2 {
                        doc.map.cell_mut(x * 2 + dx, y * 2 + dy).unwrap().flag = 0;
                    }
                }
                continue;
            };
            let mask = DIRS.iter().enumerate().fold(0u8, |bits, (i, &(dx, dy))| {
                bits | (u8::from(floor.contains(&(x + dx, y + dy))) << i)
            });
            let best = score(id, mask);
            let matching: Vec<_> = patches[id]
                .iter()
                .filter(|(candidate, _)| (candidate ^ mask).count_ones() == best)
                .collect();
            let (_, (sx, sy)) = matching[(rng.next() % matching.len() as u64) as usize];
            for dx in 0..2 {
                for dy in 0..2 {
                    *doc.map.cell_mut(x * 2 + dx, y * 2 + dy).unwrap() =
                        *source.cell(sx + dx, sy + dy).unwrap();
                }
            }
        }
    }
    Ok(doc)
}

pub(crate) fn layout(map: &MapFile, seed: u64) -> anyhow::Result<Layout> {
    let open = walkable(map);
    anyhow::ensure!(open.len() >= 64, "Too little walkable floor.");
    let first = *open.first().unwrap();
    let reached = connected(first, &open);
    anyhow::ensure!(reached.len() == open.len(), "Disconnected cave floor.");
    let a = *reached.last().unwrap();
    let b = *connected(a, &open).last().unwrap();
    let safe: Vec<_> = connected(a, &open).into_iter().take(16).collect();
    let start = *safe.iter().find(|&&p| p != a && p != b).unwrap();
    let landings = [a, b].map(|p| {
        DIRS.iter()
            .map(|&(dx, dy)| (p.0 + dx, p.1 + dy))
            .find(|q| open.contains(q) && *q != a && *q != b)
            .unwrap()
    });
    let spawns = open
        .iter()
        .copied()
        .filter(|p| {
            !safe.contains(p)
                && *p != a
                && *p != b
                && !landings.contains(p)
                && (p.0 - start.0).abs().max((p.1 - start.1).abs()) >= 8
        })
        .collect();
    let layout = Layout {
        start,
        exits: [a, b],
        landings,
        safe,
        spawns,
        seed,
        enclosed: false,
        wall_repeats: None,
        wall_room_repeats: vec![],
        wall_rooms: 0,
        rooms: vec![],
        corridors: vec![],
        corridor_width: 0,
        drop_set: 0,
        easter_event_chance: 0,
    };
    layout.validate(map)?;
    Ok(layout)
}

pub fn cave_with_art(
    source: &MapFile,
    width: u16,
    height: u16,
    seed: u64,
    assets: &Path,
) -> anyhow::Result<(Document, Layout, crate::cave_art::Report)> {
    let (mut doc, _) = cave(source, width, height, seed)?;
    let (templates, mut report) = crate::cave_art::extract(source, assets)?;
    crate::cave_art::apply(&mut doc.map, &templates, seed, &mut report);
    let layout = layout(&doc.map, seed)?;
    Ok((doc, layout, report))
}

/// Cave with a complete private wall library for every exposed floor edge.
pub fn cave_with_full_art(
    source: &MapFile,
    width: u16,
    height: u16,
    seed: u64,
    assets: &Path,
) -> anyhow::Result<(Document, Layout, crate::cave_art::Report, Vec<u8>, usize)> {
    let (mut doc, layout, report) = cave_with_art(source, width, height, seed, assets)?;
    let (library, walls) = crate::freeform_art::frame(&mut doc.map, assets, seed, None)?;
    Ok((doc, layout, report, library, walls))
}

/// Preserve a complete authored chamber, including the perimeter and full
/// frame extents. Only its even-grid placement and gameplay content vary.
pub fn enclosed_chamber(
    source: &MapFile,
    width: u16,
    height: u16,
    seed: u64,
    assets: &Path,
) -> anyhow::Result<(Document, Layout)> {
    let extent = crate::cave_art::source_extent(source, assets)?;
    chamber_with_extent(source, width, height, seed, extent)
}

pub fn chamber_size(source: &MapFile, assets: &Path) -> anyhow::Result<(u16, u16)> {
    let (left, top, right, bottom) = crate::cave_art::source_extent(source, assets)?;
    let w = (right - left + 4).max(32);
    let h = (bottom - top + 4).max(32);
    anyhow::ensure!(w<=256 && h<=256,"Whole source artwork needs {w}×{h}; the generator supports up to 256×256. Choose a smaller chamber.");
    Ok((w as u16, h as u16))
}

fn chamber_with_extent(
    source: &MapFile,
    width: u16,
    height: u16,
    seed: u64,
    extent: (i32, i32, i32, i32),
) -> anyhow::Result<(Document, Layout)> {
    anyhow::ensure!(
        (32..=256).contains(&width)
            && (32..=256).contains(&height)
            && width.is_multiple_of(2)
            && height.is_multiple_of(2),
        "Chamber canvas dimensions must be even, between 32 and 256."
    );
    anyhow::ensure!(
        source.width.is_multiple_of(2) && source.height.is_multiple_of(2),
        "Use an even-sized source map to preserve its back-tile alignment."
    );
    let w = source.width as i32;
    let h = source.height as i32;
    anyhow::ensure!(
        !(0..w).any(|x| source.is_walkable(x, 0) || source.is_walkable(x, h - 1))
            && !(0..h).any(|y| source.is_walkable(0, y) || source.is_walkable(w - 1, y)),
        "The source has walkable cells at its edge; choose a closed source chamber."
    );
    // Validate before allocation and translation: disconnected source rooms are
    // not silently removed, because doing so would cut their surrounding artwork.
    layout(source, seed)?;
    let (left, top, right, bottom) = extent;
    let needed_w = right - left + 4;
    let needed_h = bottom - top + 4;
    anyhow::ensure!(width as i32>=needed_w && height as i32>=needed_h, "Whole source artwork needs a canvas of at least {needed_w}×{needed_h}; increase New size or choose a smaller chamber (for example D1006).");
    let mut rng = Random(seed);
    let dx = 2 - left + 2 * (rng.next() % ((width as i32 - needed_w) / 2 + 1) as u64) as i32;
    let dy = 2 - top + 2 * (rng.next() % ((height as i32 - needed_h) / 2 + 1) as u64) as i32;
    let mut doc = Document::blank(width, height);
    for x in 0..width as i32 {
        for y in 0..height as i32 {
            doc.map.cell_mut(x, y).unwrap().flag = 0;
        }
    }
    for x in 0..w {
        for y in 0..h {
            *doc.map.cell_mut(x + dx, y + dy).unwrap() = *source.cell(x, y).unwrap();
        }
    }
    let mut layout = layout(&doc.map, seed)?;
    layout.enclosed = true;
    Ok((doc, layout))
}

/// Apply only to a private database snapshot, after validating the current map.
pub fn populate(
    db: &mut MirDb,
    map: &MapFile,
    layout: &Layout,
    file: &str,
    monster: i32,
) -> anyhow::Result<i32> {
    layout.validate(map)?;
    anyhow::ensure!(
        db.collection("MonsterInfo")
            .is_some_and(|c| c.records.iter().any(|r| c.index(r) == monster)),
        "Select an existing monster."
    );
    for table in [
        "MapInfo",
        "MapRegion",
        "SafeZoneInfo",
        "RespawnInfo",
        "MovementInfo",
    ] {
        anyhow::ensure!(
            db.collection(table).is_some(),
            "Missing {table} collection."
        );
    }
    anyhow::ensure!(
        !db.collection("MapInfo").unwrap().records.iter().any(|r| db
            .collection("MapInfo")
            .unwrap()
            .str_or(r, "FileName", "")
            .eq_ignore_ascii_case(file)),
        "Map filename already registered: {file}"
    );
    let id = db.collection_mut("MapInfo").unwrap().push_record(vec![
        ("FileName", Value::Str(file.into())),
        (
            "Description",
            Value::Str(format!(
                "Generated {} {}",
                if layout.wall_rooms >= 2 {
                    "Wood connected rooms"
                } else if layout.wall_repeats.is_some() {
                    "Wood room"
                } else if !layout.rooms.is_empty() {
                    "dungeon"
                } else if layout.enclosed {
                    "chamber"
                } else {
                    "cave"
                },
                layout.seed
            )),
        ),
        ("Light", Value::Int(1)),
    ]);
    let mut region = |name: &str, points: Vec<Point>| {
        db.collection_mut("MapRegion").unwrap().push_record(vec![
            ("Map", Value::Int(id as i64)),
            ("Description", Value::Str(name.into())),
            ("PointRegion", Value::PointArray(Some(points))),
        ])
    };
    let safe = region("Cave arrival", layout.safe.clone());
    let spawn = region("Cave inhabitants", layout.spawns.clone());
    let a = region("Cave portal A", vec![layout.exits[0]]);
    let b = region("Cave portal B", vec![layout.exits[1]]);
    let landing_a = region("Cave landing A", vec![layout.landings[0]]);
    let landing_b = region("Cave landing B", vec![layout.landings[1]]);
    db.collection_mut("SafeZoneInfo").unwrap().push_record(vec![
        ("Region", Value::Int(safe as i64)),
        ("BindRegion", Value::Int(safe as i64)),
        ("StartClass", Value::Int(15)),
    ]);
    db.collection_mut("RespawnInfo").unwrap().push_record(vec![
        ("Region", Value::Int(spawn as i64)),
        ("Monster", Value::Int(monster as i64)),
        ("Count", Value::Int(8)),
        ("Delay", Value::Int(30)),
        ("DropSet", Value::Int(layout.drop_set.into())),
        (
            "EasterEventChance",
            Value::Int(layout.easter_event_chance.into()),
        ),
    ]);
    for (from, to) in [(a, landing_b), (b, landing_a)] {
        db.collection_mut("MovementInfo").unwrap().push_record(vec![
            ("SourceRegion", Value::Int(from as i64)),
            ("DestinationRegion", Value::Int(to as i64)),
        ]);
    }
    Ok(id)
}

fn unique_file(db: &MirDb, base: &str) -> String {
    let mut file = base.to_owned();
    let mut suffix = 1;
    while db.collection("MapInfo").is_some_and(|c| {
        c.records
            .iter()
            .any(|r| c.str_or(r, "FileName", "").eq_ignore_ascii_case(&file))
    }) {
        file = format!("{base}_{suffix}");
        suffix += 1;
    }
    file
}

pub fn read_layout(path: &Path) -> anyhow::Result<Option<(Layout, i32)>> {
    match std::fs::read(path.with_extension("cave.json")) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn export(
    folder: &Path,
    map: &MapFile,
    db: &MirDb,
    layout: &Layout,
    monster: i32,
) -> anyhow::Result<()> {
    let mut copy = MirDb::parse(&db.to_bytes()?)?;
    let file = unique_file(db, "GeneratedCave");
    populate(&mut copy, map, layout, &file, monster)?;
    // create_dir refuses existing destinations, including previous bundles.
    std::fs::create_dir(folder)?;
    std::fs::create_dir(folder.join("Map"))?;
    map.save(folder.join(format!("Map/{file}.map")))?;
    layout
        .navigation(map)?
        .save(&folder.join(format!("Map/{file}.map")), map)?;
    std::fs::write(
        folder.join(format!("Map/{file}.cave.json")),
        serde_json::to_vec_pretty(&(layout, monster))?,
    )?;
    copy.save(folder.join("System.db"))?;
    std::fs::write(folder.join("README.txt"),format!("Generated map seed {}. Self-contained database snapshot; not a database overlay.\nUse --db System.db and ZIRCON_MAP_DIR=<this folder>/Map, --map {file}, ZIRCON_DEV_START={file}:{},{}. Keep original assets for tile libraries.\nPortals A and B connect two ends of this map. Eight selected monsters respawn every 30 seconds.\n",layout.seed,layout.start.0,layout.start.1))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn floor_uses_observed_directional_pairs_and_preserves_complete_patches() {
        let mut source = MapFile::blank(8, 8);
        for x in 0..8 {
            for y in 0..8 {
                let c = source.cell_mut(x, y).unwrap();
                c.flag = 3;
                // Alternate east/west, remain identical north/south. Reversing
                // the learned axes would therefore fail this fixture.
                c.back_image = ((x / 2) % 2) as u16;
                c.padding = [x as u8, y as u8, 17, 29];
                c.set_light(8);
            }
        }
        let before = source.to_bytes();
        let floor = (1..7).flat_map(|x| (1..7).map(move |y| (x, y))).collect();
        let generated = paint_floor_compatible(&source, 16, 16, &floor, &mut Random(42)).unwrap();
        assert_eq!(
            generated.map.to_bytes(),
            paint_floor_compatible(&source, 16, 16, &floor, &mut Random(42))
                .unwrap()
                .map
                .to_bytes()
        );
        for &(x, y) in &floor {
            let cell = generated.map.cell(x * 2, y * 2).unwrap();
            if floor.contains(&(x + 1, y)) {
                assert_ne!(
                    cell.back_image,
                    generated.map.cell((x + 1) * 2, y * 2).unwrap().back_image
                );
            }
            if floor.contains(&(x, y + 1)) {
                assert_eq!(
                    cell.back_image,
                    generated.map.cell(x * 2, (y + 1) * 2).unwrap().back_image
                );
            }
            assert!((0..8).step_by(2).any(|sx| (0..8).step_by(2).any(|sy| {
                (0..2).all(|dx| {
                    (0..2).all(|dy| {
                        generated.map.cell(x * 2 + dx, y * 2 + dy) == source.cell(sx + dx, sy + dy)
                    })
                })
            })));
        }
        assert_eq!(generated.map.walkable_count(), floor.len() * 4);
        assert_eq!(source.to_bytes(), before);
    }

    #[test]
    fn floor_rejects_unobserved_adjacency_without_inventing_a_fallback() {
        let mut source = MapFile::blank(2, 2);
        for x in 0..2 {
            for y in 0..2 {
                source.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        assert!(
            paint_floor_compatible(&source, 4, 2, &BTreeSet::from([(0, 0)]), &mut Random(0))
                .is_ok()
        );
        let error = paint_floor_compatible(
            &source,
            4,
            2,
            &BTreeSet::from([(0, 0), (1, 0)]),
            &mut Random(0),
        )
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("adjacency cannot satisfy"), "{error}");
    }

    #[test]
    fn floor_search_can_abandon_a_dead_end_patch_family() {
        let mut source = MapFile::blank(8, 6);
        // The top row offers A -> B, which cannot extend to six blocks. The
        // separate bottom row offers C -> C and can tile the full target.
        for (x, y, image) in [
            (0, 0, 1),
            (2, 0, 2),
            (0, 4, 3),
            (2, 4, 3),
            (4, 4, 3),
            (6, 4, 3),
        ] {
            for dx in 0..2 {
                for dy in 0..2 {
                    let cell = source.cell_mut(x + dx, y + dy).unwrap();
                    cell.flag = 3;
                    cell.back_image = image;
                }
            }
        }
        let floor = (0..6).map(|x| (x, 0)).collect();
        for seed in 0..16 {
            let result = paint_floor_compatible(&source, 12, 2, &floor, &mut Random(seed)).unwrap();
            for x in (0..12).step_by(2) {
                assert_eq!(result.map.cell(x, 0).unwrap().back_image, 3);
            }
        }
    }

    fn source() -> MapFile {
        let mut map = MapFile::blank(8, 8);
        for x in 0..8 {
            for y in 0..8 {
                let c = map.cell_mut(x, y).unwrap();
                c.flag = if x < 4 { 3 } else { 0 };
                c.back_image = if x < 4 { 12 } else { 20 };
            }
        }
        map
    }
    #[test]
    fn seeded_caves_are_connected_serializable_and_reuse_source_art() {
        for seed in 0..12 {
            let (d, l) = cave(&source(), 64, 64, seed).unwrap();
            assert!(l.validate(&d.map).unwrap() >= 64);
            assert_eq!(
                d.map.to_bytes(),
                cave(&source(), 64, 64, seed).unwrap().0.map.to_bytes()
            );
            let parsed = MapFile::parse(&d.map.to_bytes()).unwrap();
            l.validate(&parsed).unwrap();
            for x in (0..64).step_by(2) {
                for y in (0..64).step_by(2) {
                    let c = parsed.cell(x, y).unwrap();
                    assert!(c.back_file == 255 || c.back_image == 12);
                }
            }
        }
    }
    #[test]
    fn validation_rejects_blocked_content_and_isolated_floor() {
        let (mut d, l) = cave(&source(), 64, 64, 42).unwrap();
        d.map.cell_mut(l.exits[1].0, l.exits[1].1).unwrap().flag = 0;
        assert!(l.validate(&d.map).is_err());
        let (mut d, l) = cave(&source(), 64, 64, 42).unwrap();
        d.map.cell_mut(0, 0).unwrap().flag = 3;
        assert!(l.validate(&d.map).is_err());
        assert!(cave(&source(), 33, 64, 0).is_err());
        assert!(cave(&MapFile::blank(8, 8), 64, 64, 0).is_err());
    }
    #[test]
    fn asset_bundle_roundtrip_preserves_source_and_pairs_portals() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let db = MirDb::load(assets.join("../Database/System.db")).unwrap();
        let original = db.to_bytes().unwrap();
        let maps = db.collection("MapInfo").unwrap();
        let (file, source) = maps
            .records
            .iter()
            .filter(|r| {
                maps.str_or(r, "Description", "")
                    .to_lowercase()
                    .contains("cave")
            })
            .filter_map(|r| {
                let file = maps.str_or(r, "FileName", "");
                MapFile::load(assets.join(format!("Map/{file}.map")))
                    .ok()
                    .map(|map| (file, map))
            })
            .find(|(_, map)| cave(map, 64, 64, 42).is_ok())
            .expect("usable shipped cave");
        let (doc, layout, report) = cave_with_art(&source, 64, 64, 42, &assets).unwrap();
        println!("Art: {report:?}");
        assert!(report.templates > 0 && report.walls + report.decorations > 0);
        assert_eq!(
            doc.map.to_bytes(),
            cave_with_art(&source, 64, 64, 42, &assets)
                .unwrap()
                .0
                .map
                .to_bytes()
        );
        let monsters = db.collection("MonsterInfo").unwrap();
        let monster = monsters
            .records
            .iter()
            .find(|r| monsters.str_or(r, "MonsterName", "") == "Scarecrow")
            .map(|r| monsters.index(r))
            .unwrap_or_else(|| monsters.index(&monsters.records[0]));
        let keep = std::env::var_os("ZIRCON_CAVE_QA_DIR");
        let folder = keep
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "zircon-cave-test-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ))
            });
        export(&folder, &doc.map, &db, &layout, monster).unwrap();
        assert!(export(&folder, &doc.map, &db, &layout, monster).is_err());
        assert_eq!(db.to_bytes().unwrap(), original);
        let (restored, restored_monster) = read_layout(&folder.join("Map/GeneratedCave.map"))
            .unwrap()
            .unwrap();
        assert_eq!(restored.start, layout.start);
        assert_eq!(restored.landings, layout.landings);
        assert_eq!(restored_monster, monster);
        let navigation = mir_formats::navigation::Navigation::load(
            &folder.join("Map/GeneratedCave.map"),
            &doc.map,
        )
        .unwrap()
        .unwrap();
        assert_eq!(navigation.portals[0].position, layout.exits[0]);
        assert_eq!(navigation.portals[0].destination, layout.landings[1]);
        assert_eq!(navigation.portals[1].position, layout.exits[1]);
        assert_eq!(navigation.portals[1].destination, layout.landings[0]);
        let loaded = mir_server::data::GameData::load(folder.join("System.db")).unwrap();
        let map = loaded
            .maps
            .values()
            .find(|m| m.file_name == "GeneratedCave")
            .unwrap();
        let regions: BTreeSet<_> = loaded
            .regions
            .values()
            .filter(|r| r.map == map.index)
            .map(|r| r.index)
            .collect();
        assert_eq!(regions.len(), 6);
        let portals: Vec<_> = loaded
            .movements
            .iter()
            .filter(|m| regions.contains(&m.source_region))
            .collect();
        assert_eq!(portals.len(), 2);
        for (i, portal) in portals.iter().enumerate() {
            assert_eq!(
                loaded.regions[&portal.source_region].points(64),
                vec![layout.exits[i]]
            );
            assert_eq!(
                loaded.regions[&portal.destination_region].points(64),
                vec![layout.landings[1 - i]]
            );
            assert!(!portals
                .iter()
                .any(|p| p.source_region == portal.destination_region));
        }
        assert!(loaded
            .respawns
            .iter()
            .any(|r| regions.contains(&r.region) && r.monster == monster && r.count == 8));
        assert!(loaded
            .safe_zones
            .iter()
            .any(|s| regions.contains(&s.region) && s.region == s.bind_region));
        layout
            .validate(&MapFile::load(folder.join("Map/GeneratedCave.map")).unwrap())
            .unwrap();
        verify_playable_bundle(loaded, &layout, &folder);
        println!(
            "Cave source {file}; bundle {}; start {},{}; portals {:?}; monster {monster}",
            folder.display(),
            layout.start.0,
            layout.start.1,
            layout.exits
        );
        if keep.is_none() {
            std::fs::remove_dir_all(folder).unwrap();
        }
    }
    fn verify_playable_bundle(
        mut loaded: mir_server::data::GameData,
        layout: &Layout,
        folder: &Path,
    ) {
        let map_id = loaded
            .maps
            .values()
            .find(|m| m.file_name == "GeneratedCave")
            .unwrap()
            .index;
        let regions: BTreeSet<_> = loaded
            .regions
            .values()
            .filter(|r| r.map == map_id)
            .map(|r| r.index)
            .collect();
        loaded.safe_zones.retain(|z| regions.contains(&z.region));
        let mut world = mir_server::world::World::new(loaded, folder.join("Map"), None);
        let mut character = mir_server::accounts::test_character("CaveTester");
        character.map = "GeneratedCave".into();
        character.location.x = layout.landings[0].0;
        character.location.y = layout.landings[0].1;
        let player = world.add_player(1, 1, &character).unwrap();
        assert!(world.objects[&player].in_safe_zone);
        assert_eq!(world.on_map(map_id).filter(|o| o.is_monster()).count(), 8);
        for i in 0..2 {
            let from = world.objects[&player].location;
            assert_eq!((from.x, from.y), layout.landings[i]);
            let (dx, dy) = (layout.exits[i].0 - from.x, layout.exits[i].1 - from.y);
            let direction = match (dx, dy) {
                (0, -1) => "Up",
                (1, 0) => "Right",
                (0, 1) => "Down",
                (-1, 0) => "Left",
                _ => panic!("nonadjacent landing"),
            };
            world.now += 1000;
            world.player_move(
                player,
                serde_json::from_value(serde_json::json!(direction)).unwrap(),
                false,
            );
            let to = world.objects[&player].location;
            assert_eq!((to.x, to.y), layout.landings[1 - i]);
        }
    }

    #[test]
    fn enclosed_layout_revalidation_rejects_a_connected_border_breach() {
        let mut map = MapFile::blank(32, 32);
        for x in 2..30 {
            for y in 2..30 {
                map.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        let mut generated = layout(&map, 42).unwrap();
        generated.enclosed = true;
        generated.validate(&map).unwrap();
        // Extend existing floor to the border without disconnecting content.
        // Connectivity alone previously accepted this enclosure breach.
        map.cell_mut(2, 1).unwrap().flag = 3;
        map.cell_mut(2, 0).unwrap().flag = 3;
        assert!(generated
            .validate(&map)
            .unwrap_err()
            .to_string()
            .contains("blocked border"));
        generated.enclosed = false;
        generated.validate(&map).unwrap();
    }

    #[test]
    fn wall_repeat_metadata_roundtrips_and_legacy_layouts_default_to_none() {
        let mut map = MapFile::blank(32, 32);
        for x in 2..30 {
            for y in 2..30 {
                map.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        let mut generated = layout(&map, 42).unwrap();
        let mut legacy = serde_json::to_value(&generated).unwrap();
        legacy.as_object_mut().unwrap().remove("wall_rooms");
        assert!(legacy.get("wall_room_repeats").is_none());
        assert_eq!(
            serde_json::from_value::<Layout>(legacy.clone())
                .unwrap()
                .wall_rooms,
            0
        );
        assert!(legacy.get("wall_repeats").is_none());
        assert_eq!(
            serde_json::from_value::<Layout>(legacy)
                .unwrap()
                .wall_repeats,
            None
        );
        generated.enclosed = true;
        generated.wall_repeats = Some([3, 10]);
        generated.wall_rooms = 1;
        generated.validate(&map).unwrap();
        let restored: Layout =
            serde_json::from_slice(&serde_json::to_vec(&generated).unwrap()).unwrap();
        assert_eq!(restored.wall_repeats, Some([3, 10]));
        assert_eq!(restored.wall_rooms, 1);
        restored.validate(&map).unwrap();
        generated.wall_rooms = 3;
        assert!(generated.validate(&map).is_err());
        generated.wall_rooms = 2;
        generated.validate(&map).unwrap();
        generated.wall_repeats = Some([3, 6]);
        generated.validate(&map).unwrap();
        for repeats in [[2, 4], [3, 11], [0, 0]] {
            generated.wall_repeats = Some(repeats);
            assert!(generated.validate(&map).is_err());
        }
        generated.wall_repeats = Some([3, 4]);
        generated.enclosed = false;
        assert!(generated.validate(&map).is_err());
        generated.enclosed = true;
        generated.wall_repeats = None;
        generated.wall_rooms = 2;
        generated.wall_room_repeats = vec![[3, 3], [4, 3]];
        generated.validate(&map).unwrap();
        let restored: Layout =
            serde_json::from_slice(&serde_json::to_vec(&generated).unwrap()).unwrap();
        assert_eq!(restored.wall_room_repeats, vec![[3, 3], [4, 3]]);
        generated.wall_repeats = Some([3, 3]);
        assert!(generated.validate(&map).is_err());
    }

    #[test]
    fn enclosed_chambers_preserve_every_cell_and_full_frame_margins() {
        let mut source = MapFile::blank(32, 32);
        for x in 4..28 {
            for y in 4..28 {
                let c = source.cell_mut(x, y).unwrap();
                c.flag = 3;
                c.padding = [1, 2, 3, 4];
                c.middle_image = 123;
            }
        }
        // Simulate a tall frame extending eight cells above the source grid.
        let extent = (0, -8, 32, 32);
        let (doc, l) = chamber_with_extent(&source, 48, 48, 42, extent).unwrap();
        l.validate(&doc.map).unwrap();
        let first = walkable(&doc.map).first().copied().unwrap();
        let (dx, dy) = (first.0 - 4, first.1 - 4);
        assert!(dx >= 2 && dy >= 10 && dx % 2 == 0 && dy % 2 == 0);
        for x in 0..32 {
            for y in 0..32 {
                assert_eq!(doc.map.cell(x + dx, y + dy), source.cell(x, y));
            }
        }
        assert_eq!(doc.map.walkable_count(), source.walkable_count());
        assert_eq!(
            doc.map.to_bytes(),
            chamber_with_extent(&source, 48, 48, 42, extent)
                .unwrap()
                .0
                .map
                .to_bytes()
        );
        assert!(chamber_with_extent(&source, 32, 32, 42, extent).is_err());
        source.cell_mut(0, 8).unwrap().flag = 3;
        assert!(chamber_with_extent(&source, 48, 48, 42, extent).is_err());
    }

    #[test]
    fn shipped_closed_chamber_exports_all_of_its_perimeter() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1006.map")).unwrap();
        let before = source.to_bytes();
        let (doc, l) = enclosed_chamber(&source, 80, 80, 42, &assets).unwrap();
        assert!(l.enclosed);
        assert_eq!(doc.map.walkable_count(), source.walkable_count());
        assert_eq!(source.to_bytes(), before);
        let (w, h) = chamber_size(&source, &assets).unwrap();
        enclosed_chamber(&source, w, h, 42, &assets).unwrap();
        l.validate(&doc.map).unwrap();
        let db = MirDb::load(assets.join("../Database/System.db")).unwrap();
        let monsters = db.collection("MonsterInfo").unwrap();
        let monster = monsters
            .records
            .iter()
            .find(|r| monsters.str_or(r, "MonsterName", "") == "Scarecrow")
            .map(|r| monsters.index(r))
            .unwrap();
        let keep = std::env::var_os("ZIRCON_CHAMBER_QA_DIR");
        let folder = keep
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "zircon-chamber-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ))
            });
        export(&folder, &doc.map, &db, &l, monster).unwrap();
        let loaded = mir_server::data::GameData::load(folder.join("System.db")).unwrap();
        assert!(loaded.maps.values().any(|m| m.file_name == "GeneratedCave"));
        let roundtrip = MapFile::load(folder.join("Map/GeneratedCave.map")).unwrap();
        assert_eq!(roundtrip.to_bytes(), doc.map.to_bytes());
        assert!(
            read_layout(&folder.join("Map/GeneratedCave.map"))
                .unwrap()
                .unwrap()
                .0
                .enclosed
        );
        let mut legacy = serde_json::to_value(&l).unwrap();
        legacy.as_object_mut().unwrap().remove("enclosed");
        assert!(!serde_json::from_value::<Layout>(legacy).unwrap().enclosed);
        verify_playable_bundle(loaded, &l, &folder);
        println!(
            "Enclosed D1006: {} floor cells, start {:?}, exits {:?}, bundle {}",
            doc.map.walkable_count(),
            l.start,
            l.exits,
            folder.display()
        );
        if keep.is_none() {
            std::fs::remove_dir_all(folder).unwrap();
        }
    }
    #[test]
    fn dungeon_bundle_preserves_rooms_and_passages_and_plays_on_the_server() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D102.map")).unwrap();
        let before = source.to_bytes();
        let (mut doc, l, report) =
            crate::dungeon::generate(&source, 64, 64, 42, 8, 4, &assets).unwrap();
        assert_eq!(source.to_bytes(), before);
        assert_eq!(l.rooms.len(), 8);
        assert_eq!(l.corridor_width, 4);
        assert!(l.corridors.iter().all(|&(x, y)| doc.map.is_walkable(x, y)));
        assert_eq!(
            doc.map.to_bytes(),
            crate::dungeon::generate(&source, 64, 64, 42, 8, 4, &assets)
                .unwrap()
                .0
                .map
                .to_bytes()
        );
        let point = l.corridors[0];
        let old = *doc.map.cell(point.0, point.1).unwrap();
        doc.map.cell_mut(point.0, point.1).unwrap().flag = 0;
        assert!(l.validate(&doc.map).is_err());
        *doc.map.cell_mut(point.0, point.1).unwrap() = old;
        l.validate(&doc.map).unwrap();
        let db = MirDb::load(assets.join("../Database/System.db")).unwrap();
        let c = db.collection("MonsterInfo").unwrap();
        let monster = c
            .records
            .iter()
            .find(|r| c.str_or(r, "MonsterName", "") == "Scarecrow")
            .map(|r| c.index(r))
            .unwrap();
        let keep = std::env::var_os("ZIRCON_DUNGEON_QA_DIR");
        let folder = keep
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "zircon-dungeon-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ))
            });
        export(&folder, &doc.map, &db, &l, monster).unwrap();
        let (restored, _) = read_layout(&folder.join("Map/GeneratedCave.map"))
            .unwrap()
            .unwrap();
        assert_eq!(restored.rooms, l.rooms);
        assert_eq!(restored.corridors, l.corridors);
        assert_eq!(restored.corridor_width, 4);
        let navigation = mir_formats::navigation::Navigation::load(
            &folder.join("Map/GeneratedCave.map"),
            &doc.map,
        )
        .unwrap()
        .unwrap();
        assert_eq!(navigation, l.navigation(&doc.map).unwrap());
        assert_eq!(navigation.portals[0].position, l.exits[0]);
        assert_eq!(navigation.portals[0].destination, l.landings[1]);
        assert_eq!(navigation.portals[1].position, l.exits[1]);
        assert_eq!(navigation.portals[1].destination, l.landings[0]);
        let mut legacy = serde_json::to_value(&l).unwrap();
        for field in ["rooms", "corridors", "corridor_width"] {
            legacy.as_object_mut().unwrap().remove(field);
        }
        assert!(serde_json::from_value::<Layout>(legacy)
            .unwrap()
            .rooms
            .is_empty());
        let loaded = mir_server::data::GameData::load(folder.join("System.db")).unwrap();
        verify_playable_bundle(loaded, &l, &folder);
        println!(
            "Dungeon: {} rooms, {} protected corridor cells, art {report:?}, start {:?}, bundle {}",
            l.rooms.len(),
            l.corridors.len(),
            l.start,
            folder.display()
        );
        if keep.is_none() {
            std::fs::remove_dir_all(folder).unwrap();
        }
    }
}
