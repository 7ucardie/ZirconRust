//! Bounded branching Wood layouts with new corner art beside original source art.
use crate::{generate, tile_doc::Document};
use anyhow::{ensure, Context, Result};
use mir_formats::{
    map::MapFile,
    zl::{Sprite, SurfaceKind, ZlBuilder, ZlLibrary},
};
use std::{collections::BTreeSet, io::Cursor, path::Path};

pub const LIBRARY: &str = "Data/Map Data/Wood/Dungeonsc.Zl";
pub const PHASES: usize = 16;
pub const NEW_IMAGES: usize = 16 * PHASES;
const SIZE: u16 = 112;
type Point = (i32, i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// West, north, east.
    T,
    /// Northwest, northeast, south.
    Y,
    /// West, north, east, south.
    Cross,
}

/// `centers` starts with the junction, then follows `arms` in shape order.
pub struct Built {
    pub doc: Document,
    pub layout: generate::Layout,
    pub library: Vec<u8>,
    pub first_new_image: u16,
    pub arms: Vec<u8>,
    pub centers: Vec<Point>,
}

fn add_rect(floor: &mut BTreeSet<Point>, (left, top, right, bottom): (i32, i32, i32, i32)) {
    floor.extend((left..=right).flat_map(|x| (top..=bottom).map(move |y| (x, y))));
}

fn add_room(
    floor: &mut BTreeSet<Point>,
    room: (i32, i32, i32, i32),
    protected: u8,
    organic: bool,
    seed: u64,
) {
    add_rect(floor, room);
    if organic {
        let (left, top, right, bottom) = room;
        // Indent only edges without passage openings. Preserve the solid 9×9 interior.
        for x in left..=right {
            for y in top..=bottom {
                let edge = (top + 2..=bottom - 2).contains(&y)
                    && ((x == left && protected & 8 == 0) || (x == right && protected & 2 == 0))
                    || (left + 2..=right - 2).contains(&x)
                        && ((y == top && protected & 1 == 0)
                            || (y == bottom && protected & 4 == 0));
                if edge
                    && generate::Random(seed ^ ((x as u64) << 32) ^ y as u64)
                        .next()
                        .is_multiple_of(3)
                {
                    floor.remove(&(x, y));
                }
            }
        }
    }
}

fn block_floor(
    shape: Shape,
    arms: &[u8],
    organic: bool,
    seed: u64,
) -> (BTreeSet<Point>, Vec<Point>) {
    let mut floor = BTreeSet::new();
    add_rect(&mut floor, (23, 23, 33, 33));
    let mut centers = vec![(57, 57)];
    let cardinal = match shape {
        Shape::T => &['W', 'N', 'E'][..],
        Shape::Y => &['A', 'B', 'S'][..],
        Shape::Cross => &['W', 'N', 'E', 'S'][..],
    };
    let mut passages = BTreeSet::new();
    for (index, &direction) in cardinal.iter().enumerate() {
        let a = i32::from(arms[index]);
        let (room, center, protected) = match direction {
            'W' => {
                add_rect(&mut passages, (23 - a, 26, 22, 30));
                ((12 - a, 23, 22 - a, 33), (35 - 2 * a, 57), 2)
            }
            'N' => {
                add_rect(&mut passages, (26, 23 - a, 30, 22));
                ((23, 12 - a, 33, 22 - a), (57, 35 - 2 * a), 4)
            }
            'E' => {
                add_rect(&mut passages, (34, 26, 33 + a, 30));
                ((34 + a, 23, 44 + a, 33), (79 + 2 * a, 57), 8)
            }
            'S' => {
                add_rect(&mut passages, (26, 34, 30, 33 + a));
                ((23, 34 + a, 33, 44 + a), (57, 79 + 2 * a), 1)
            }
            'A' | 'B' => {
                for t in 0..=a + 4 {
                    let (x, y) = if direction == 'A' {
                        (25 - t, 23 - t)
                    } else {
                        (31 + t, 23 - t)
                    };
                    add_rect(&mut passages, (x - 2, y - 2, x + 2, y + 2));
                }
                if direction == 'A' {
                    (
                        (12 - a, 12 - a, 22 - a, 22 - a),
                        (35 - 2 * a, 35 - 2 * a),
                        2 | 4,
                    )
                } else {
                    (
                        (34 + a, 12 - a, 44 + a, 22 - a),
                        (79 + 2 * a, 35 - 2 * a),
                        4 | 8,
                    )
                }
            }
            _ => unreachable!(),
        };
        add_room(&mut floor, room, protected, organic, seed);
        centers.push(center);
    }
    floor.extend(passages);
    (floor, centers)
}

fn wall_texture() -> Result<(Vec<u8>, usize)> {
    let atlas = include_bytes!("../art/wood_wall_atlas_16.png");
    let mut reader = png::Decoder::new(Cursor::new(atlas)).read_info()?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .context("Wood atlas size overflow")?
    ];
    let frame = reader.next_frame(&mut pixels)?;
    ensure!(
        frame.width == frame.height
            && frame.width >= 512
            && frame.color_type == png::ColorType::Rgb
            && frame.bit_depth == png::BitDepth::Eight,
        "Wood wall atlas must be a square RGB 4×4 sheet."
    );
    pixels.truncate(frame.buffer_size());
    Ok((pixels, frame.width as usize))
}

fn wall_sprite(
    mask: u8,
    phase: usize,
    caps: &[Vec<u8>],
    faces: &[Vec<u8>],
    texture: &[u8],
    atlas_width: usize,
) -> Vec<u8> {
    let mut rgba = vec![0; 48 * 64 * 4];
    let painting = phase & 3;
    for y in 0usize..64 {
        for x in 0usize..48 {
            let (sources, sy) = if y < 16 {
                (caps, y + 72)
            } else {
                (faces, y + 64)
            };
            let source_pixel = (sy * 48 + x) * 4;
            let mirrored_pixel = (sy * 48 + 47 - x) * 4;
            let target_pixel = (y * 48 + x) * 4;
            // Shared four-pixel rim matches Zl's 4×4 compression blocks.
            // Mirror averaging makes left and right rims match on straight runs.
            let distance = x.min(47 - x).min(y.min(63 - y));
            let interior = distance.saturating_sub(3).min(8) as u16;
            for channel in 0..3 {
                let shared = sources
                    .iter()
                    .map(|source| {
                        u16::from(source[source_pixel + channel])
                            + u16::from(source[mirrored_pixel + channel])
                    })
                    .sum::<u16>()
                    / 8;
                let variant = u16::from(sources[painting][source_pixel + channel]);
                let mixed = ((shared * (8 - interior) + variant * interior + 4) / 8) as u8;
                let painted = if y >= 16 {
                    let panel = atlas_width / 4;
                    let atlas_x = (phase % 4) * panel + x * panel / 48;
                    let atlas_y = (phase / 4) * panel + (y - 16) * panel / 48;
                    texture[(atlas_y * atlas_width + atlas_x) * 3 + channel]
                } else {
                    mixed
                };
                let paint_weight = interior;
                let mixed = ((u16::from(mixed) * (8 - paint_weight)
                    + u16::from(painted) * paint_weight
                    + 4)
                    / 8) as u8;
                let value = &mut rgba[target_pixel + channel];
                *value = if y < 16 {
                    mixed.saturating_add(8)
                } else {
                    mixed.saturating_sub(6)
                };
            }
            if (14..17).contains(&y) || (y >= 16 && !(3..45).contains(&x)) {
                for channel in &mut rgba[target_pixel..target_pixel + 3] {
                    *channel = channel.saturating_sub(14);
                }
            }
            if (mask & 1 != 0 && y == 0)
                || (mask & 2 != 0 && x == 47)
                || (mask & 4 != 0 && y == 63)
                || (mask & 8 != 0 && x == 0)
            {
                for channel in &mut rgba[target_pixel..target_pixel + 3] {
                    *channel = channel.saturating_add(10);
                }
            }
            rgba[target_pixel + 3] = 255;
        }
    }
    rgba
}

pub(crate) fn wall_builder(assets: &Path) -> Result<(u16, ZlBuilder)> {
    let source = ZlLibrary::open(assets.join(LIBRARY)).context("Open original Wood dungeon art")?;
    let first = u16::try_from(source.len()).context("Wood library has too many image slots")?;
    anyhow::ensure!(
        usize::from(first) + NEW_IMAGES <= usize::from(u16::MAX),
        "No image slots remain for Wood branch art."
    );
    // Sixteen distinct painted panels replace the repeated source wall face.
    // Opaque bands keep every generated edge solid, including concave corners.
    let mut caps = Vec::new();
    let mut faces = Vec::new();
    for index in [340, 345, 394, 399] {
        let art = source
            .decode(index, SurfaceKind::Image)?
            .with_context(|| format!("Wood wall texture {index} is missing"))?;
        anyhow::ensure!(
            art.width == 48
                && art.height == 160
                && art.rgba[64 * 48 * 4..96 * 48 * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[3] == 255),
            "Wood source cap texture changed."
        );
        caps.push(art.rgba);
    }
    for index in [360, 383, 409, 416] {
        let art = source
            .decode(index, SurfaceKind::Image)?
            .with_context(|| format!("Wood wall texture {index} is missing"))?;
        anyhow::ensure!(
            art.width == 48
                && art.height == 128
                && art.rgba[80 * 48 * 4..128 * 48 * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[3] == 255),
            "Wood source face texture changed."
        );
        faces.push(art.rgba);
    }
    let (texture, atlas_width) = wall_texture()?;
    let mut library = source.to_builder()?;
    for mask in 0..16 {
        for phase in 0..PHASES {
            let pixels = wall_sprite(mask, phase, &caps, &faces, &texture, atlas_width);
            library.push_rgba(&Sprite {
                width: 48,
                height: 64,
                offset_x: 0,
                offset_y: 0,
                rgba: &pixels,
                shadow_type: 0,
                shadow_offset_x: 0,
                shadow_offset_y: 0,
                shadow: None,
            });
        }
    }
    Ok((first, library))
}

pub(crate) fn wall_library(assets: &Path) -> Result<(u16, Vec<u8>)> {
    let (first, library) = wall_builder(assets)?;
    Ok((first, library.to_bytes()))
}

/// Append 16 corner profiles in four source-texture phases to a private Wood library. Every
/// installed image slot and original map remains untouched.
pub fn generate(assets: &Path, seed: u64) -> Result<Built> {
    generate_with_arms(assets, seed, [5, 5, 5])
}

/// Three corridor lengths, west/north/east, each two to eight floor blocks.
pub fn generate_with_arms(assets: &Path, seed: u64, arms: [u8; 3]) -> Result<Built> {
    generate_shape(assets, seed, Shape::T, &arms, false)
}

/// Generate a bounded star graph. Arm order follows the shape's direction order above.
/// Organic mode gives terminal rooms irregular edges without changing passages.
pub fn generate_shape(
    assets: &Path,
    seed: u64,
    shape: Shape,
    arms: &[u8],
    organic: bool,
) -> Result<Built> {
    let expected = if shape == Shape::Cross { 4 } else { 3 };
    anyhow::ensure!(
        arms.len() == expected && arms.iter().all(|n| (2..=8).contains(n)),
        "Wood branch shape requires {expected} arms, each 2–8 floor blocks long."
    );
    let (floor, centers) = block_floor(shape, arms, organic, seed);
    build(assets, seed, floor, centers, arms.to_vec(), SIZE)
}

/// Connected graph on a 5×5 room lattice. Nodes are unique `[column,row]` coordinates (0–4);
/// links are pairs of node indices connecting cardinal neighbors. Two to twelve rooms are allowed.
pub fn generate_grid_graph(
    assets: &Path,
    seed: u64,
    nodes: &[[u8; 2]],
    links: &[[usize; 2]],
    organic: bool,
) -> Result<Built> {
    anyhow::ensure!(
        (2..=12).contains(&nodes.len())
            && nodes.iter().all(|p| p[0] <= 4 && p[1] <= 4)
            && nodes.iter().copied().collect::<BTreeSet<_>>().len() == nodes.len(),
        "Wood graph requires 2–12 unique rooms on a 5×5 grid."
    );
    anyhow::ensure!(
        (1..=20).contains(&links.len()),
        "Wood graph requires 1–20 links."
    );
    let mut edges = BTreeSet::new();
    let mut seen = BTreeSet::from([0]);
    for &[a, b] in links {
        anyhow::ensure!(
            a < nodes.len() && b < nodes.len() && a != b,
            "Wood graph link has an invalid room index."
        );
        let pair = (a.min(b), a.max(b));
        anyhow::ensure!(edges.insert(pair), "Wood graph has a duplicate link.");
        let distance = nodes[a][0].abs_diff(nodes[b][0]) + nodes[a][1].abs_diff(nodes[b][1]);
        anyhow::ensure!(
            distance == 1,
            "Wood graph links must join cardinal neighbors."
        );
    }
    loop {
        let before = seen.len();
        for &(a, b) in &edges {
            if seen.contains(&a) {
                seen.insert(b);
            }
            if seen.contains(&b) {
                seen.insert(a);
            }
        }
        if seen.len() == before {
            break;
        }
    }
    anyhow::ensure!(seen.len() == nodes.len(), "Wood graph must be connected.");

    let mut floor = BTreeSet::new();
    let mut protected = vec![0u8; nodes.len()];
    let point = |[x, y]: [u8; 2]| (13 + 15 * i32::from(x), 13 + 15 * i32::from(y));
    for &(a, b) in &edges {
        let (x1, y1) = point(nodes[a]);
        let (x2, y2) = point(nodes[b]);
        if x1 != x2 {
            add_rect(&mut floor, (x1.min(x2) + 6, y1 - 2, x1.max(x2) - 6, y1 + 2));
            protected[a] |= if x1 < x2 { 2 } else { 8 };
            protected[b] |= if x1 < x2 { 8 } else { 2 };
        } else {
            add_rect(&mut floor, (x1 - 2, y1.min(y2) + 6, x1 + 2, y1.max(y2) - 6));
            protected[a] |= if y1 < y2 { 4 } else { 1 };
            protected[b] |= if y1 < y2 { 1 } else { 4 };
        }
    }
    let centers = nodes
        .iter()
        .copied()
        .enumerate()
        .map(|(index, node)| {
            let (x, y) = point(node);
            add_room(
                &mut floor,
                (x - 5, y - 5, x + 5, y + 5),
                protected[index],
                organic,
                seed,
            );
            (2 * x + 1, 2 * y + 1)
        })
        .collect();
    let size = if nodes.iter().any(|p| p[0] > 2 || p[1] > 2) {
        176
    } else {
        SIZE
    };
    build(assets, seed, floor, centers, Vec::new(), size)
}

fn build(
    assets: &Path,
    seed: u64,
    floor: BTreeSet<Point>,
    centers: Vec<Point>,
    arms: Vec<u8>,
    size: u16,
) -> Result<Built> {
    let source = MapFile::load(assets.join("Map/D1008.map"))?;
    let (first_new_image, library) = wall_library(assets)?;
    let mut doc =
        generate::paint_floor_compatible(&source, size, size, &floor, &mut generate::Random(seed))?;
    for x in 1..i32::from(size) - 1 {
        for y in 1..i32::from(size) - 1 {
            if doc.map.is_walkable(x, y) {
                continue;
            }
            let mask = [(0, -1), (1, 0), (0, 1), (-1, 0)]
                .into_iter()
                .enumerate()
                .fold(0u8, |bits, (i, (dx, dy))| {
                    bits | (u8::from(doc.map.is_walkable(x + dx, y + dy)) << i)
                });
            let near = (-1..=1).any(|dx| (-1..=1).any(|dy| doc.map.is_walkable(x + dx, y + dy)));
            if near {
                let cell = doc.map.cell_mut(x, y).unwrap();
                cell.flag = 0;
                cell.middle_file = 21;
                let phase = (generate::Random(
                    seed ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                        ^ (y as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9),
                )
                .next()
                    & (PHASES as u64 - 1)) as u16;
                cell.middle_image = first_new_image + u16::from(mask) * PHASES as u16 + phase;
            }
        }
    }
    let mut layout = generate::layout(&doc.map, seed)?;
    layout.enclosed = true;
    layout.validate(&doc.map)?;
    for &(x, y) in &centers {
        anyhow::ensure!(
            doc.map.is_walkable(x, y),
            "Wood branch room center is blocked."
        );
    }
    Ok(Built {
        doc,
        layout,
        library,
        first_new_image,
        arms,
        centers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn reached(map: &MapFile, start: Point, removed: &BTreeSet<Point>) -> BTreeSet<Point> {
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some((x, y)) = queue.pop_front() {
            for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let next = (x + dx, y + dy);
                if map.is_walkable(next.0, next.1) && !removed.contains(&next) && seen.insert(next)
                {
                    queue.push_back(next);
                }
            }
        }
        seen
    }

    #[test]
    fn three_arms_have_complete_new_wall_art_and_each_passage_is_a_cut() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source_map = std::fs::read(assets.join("Map/D1008.map")).unwrap();
        let source_lib = ZlLibrary::open(assets.join(LIBRARY)).unwrap();
        let built = generate(&assets, 42).unwrap();
        built.layout.validate(&built.doc.map).unwrap();
        assert_eq!(built.first_new_image as usize, source_lib.len());
        assert_eq!(
            reached(&built.doc.map, built.centers[0], &BTreeSet::new()).len(),
            built.doc.map.walkable_count()
        );
        for (room, cut) in [
            (
                built.centers[1],
                (2 * (22 - i32::from(built.arms[0]) / 2), 52..=61),
            ),
            (
                built.centers[2],
                (2 * (22 - i32::from(built.arms[1]) / 2), 52..=61),
            ),
            (
                built.centers[3],
                (2 * (34 + i32::from(built.arms[2]) / 2), 52..=61),
            ),
        ] {
            let removed: BTreeSet<Point> = cut
                .1
                .clone()
                .map(|n| if room.1 < 57 { (n, cut.0) } else { (cut.0, n) })
                .collect();
            assert!(!reached(&built.doc.map, built.centers[0], &removed).contains(&room));
        }
        let path =
            std::env::temp_dir().join(format!("zircon-wood-branch-{}.Zl", std::process::id()));
        std::fs::write(&path, &built.library).unwrap();
        let packed = ZlLibrary::open(&path).unwrap();
        assert_eq!(packed.len(), source_lib.len() + NEW_IMAGES);
        for index in 0..source_lib.len() {
            assert_eq!(
                packed.raw(index).unwrap().map(|image| image.payload),
                source_lib.raw(index).unwrap().map(|image| image.payload)
            );
        }
        for index in usize::from(built.first_new_image)..packed.len() {
            assert!(packed.decode(index, SurfaceKind::Image).unwrap().is_some());
        }
        let first = usize::from(built.first_new_image);
        let first_pixels = packed
            .decode(first, SurfaceKind::Image)
            .unwrap()
            .unwrap()
            .rgba;
        assert!(first_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
        let mut unique = BTreeSet::from([first_pixels.clone()]);
        for phase in 1..PHASES {
            let pixels = packed
                .decode(first + phase, SurfaceKind::Image)
                .unwrap()
                .unwrap()
                .rgba;
            assert!(
                unique.insert(pixels.clone()),
                "Repeated Wood wall panel {phase}"
            );
            assert!(pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255));
            for y in 0..64 {
                for x in 0..48 {
                    if x <= 2 || x >= 45 || y <= 2 || y >= 61 {
                        let offset = (y * 48 + x) * 4;
                        assert_eq!(pixels[offset..offset + 4], first_pixels[offset..offset + 4]);
                    }
                }
            }
        }
        for y in 0..64 {
            let left = y * 48 * 4;
            let right = left + 47 * 4;
            assert_eq!(first_pixels[left..left + 4], first_pixels[right..right + 4]);
        }
        for x in 1..i32::from(built.doc.map.width) - 1 {
            for y in 1..i32::from(built.doc.map.height) - 1 {
                if built.doc.map.is_walkable(x, y) {
                    continue;
                }
                if [(0, -1), (1, 0), (0, 1), (-1, 0)]
                    .iter()
                    .any(|&(dx, dy)| built.doc.map.is_walkable(x + dx, y + dy))
                {
                    let cell = built.doc.map.cell(x, y).unwrap();
                    assert_eq!(cell.middle_file, 21);
                    assert!(
                        (built.first_new_image..built.first_new_image + NEW_IMAGES as u16)
                            .contains(&cell.middle_image)
                    );
                }
            }
        }
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            std::fs::read(assets.join("Map/D1008.map")).unwrap(),
            source_map
        );
        if let Some(output) = std::env::var_os("ZIRCON_BRANCH_QA_DIR") {
            let output = std::path::PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(output.join("branch.map"), built.doc.map.to_bytes()).unwrap();
            std::fs::write(output.join("Dungeonsc.Zl"), &built.library).unwrap();
            std::fs::write(
                output.join("branch.cave.json"),
                serde_json::to_vec_pretty(&built.layout).unwrap(),
            )
            .unwrap();
            let db = mir_formats::mirdb::MirDb::load(
                assets.parent().unwrap().join("Database/System.db"),
            )
            .unwrap();
            let monsters = db.collection("MonsterInfo").unwrap();
            let monster = monsters
                .records
                .iter()
                .find(|row| monsters.str_or(row, "MonsterName", "") == "Scarecrow")
                .map(|row| monsters.index(row))
                .unwrap();
            let name = format!("branch-{}", std::process::id());
            let world = output.join(format!("{name}-world"));
            generate::export(&world, &built.doc.map, &db, &built.layout, monster).unwrap();
            let client = output.join(format!("{name}-Client"));
            let overrides = std::collections::BTreeMap::from([(
                std::path::PathBuf::from(LIBRARY),
                built.library.clone(),
            )]);
            crate::sprite_publish::asset_view(&assets, &assets, &client, &overrides).unwrap();
            std::fs::write(client.join(LIBRARY), &built.library).unwrap();
            println!(
                "Private Wood branch: world={} Client={}",
                world.display(),
                client.display()
            );
        }
    }

    #[test]
    fn independent_arm_lengths_keep_three_reachable_rooms() {
        assert!(generate_with_arms(Path::new("/missing"), 42, [1, 5, 5])
            .err()
            .unwrap()
            .to_string()
            .contains("2–8"));
        assert!(generate_with_arms(Path::new("/missing"), 42, [5, 9, 5]).is_err());
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        for arms in [[2, 2, 2], [2, 8, 6], [8, 2, 8], [8, 8, 8]] {
            let built = generate_with_arms(&assets, 17, arms).unwrap();
            assert_eq!(built.arms, arms);
            assert_eq!((built.doc.map.width, built.doc.map.height), (SIZE, SIZE));
            built.layout.validate(&built.doc.map).unwrap();
            let all = reached(&built.doc.map, built.centers[0], &BTreeSet::new());
            assert_eq!(all.len(), built.doc.map.walkable_count());
            for &center in &built.centers {
                assert!(all.contains(&center));
            }
            for (index, cut) in [
                (2 * (22 - i32::from(arms[0]) / 2), 52..=61),
                (2 * (22 - i32::from(arms[1]) / 2), 52..=61),
                (2 * (34 + i32::from(arms[2]) / 2), 52..=61),
            ]
            .into_iter()
            .enumerate()
            {
                let removed: BTreeSet<Point> = cut
                    .1
                    .map(|n| if index == 1 { (n, cut.0) } else { (cut.0, n) })
                    .collect();
                assert!(!reached(&built.doc.map, built.centers[0], &removed)
                    .contains(&built.centers[index + 1]));
            }
        }
    }

    #[test]
    fn junction_shapes_and_organic_edges_keep_separate_reachable_arms() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        for (shape, arms) in [
            (Shape::T, vec![3, 6, 8]),
            (Shape::Y, vec![2, 7, 4]),
            (Shape::Cross, vec![8, 2, 6, 3]),
        ] {
            let plain = generate_shape(&assets, 42, shape, &arms, false).unwrap();
            let jagged = generate_shape(&assets, 42, shape, &arms, true).unwrap();
            if let Some(output) = std::env::var_os("ZIRCON_BRANCH_QA_DIR") {
                let output = std::path::PathBuf::from(output);
                std::fs::create_dir_all(&output).unwrap();
                let name = match shape {
                    Shape::T => "branch_t.map",
                    Shape::Y => "branch_y.map",
                    Shape::Cross => "branch_cross.map",
                };
                std::fs::write(output.join(name), plain.doc.map.to_bytes()).unwrap();
            }
            assert_eq!(plain.centers.len(), arms.len() + 1);
            assert_eq!(plain.arms, arms);
            assert!(jagged.doc.map.walkable_count() < plain.doc.map.walkable_count());
            for built in [&plain, &jagged] {
                built.layout.validate(&built.doc.map).unwrap();
                let all = reached(&built.doc.map, built.centers[0], &BTreeSet::new());
                assert_eq!(all.len(), built.doc.map.walkable_count());
                for &center in &built.centers {
                    assert!(all.contains(&center));
                }
                let junction: BTreeSet<_> = (46..=67)
                    .flat_map(|x| (46..=67).map(move |y| (x, y)))
                    .collect();
                for (index, &center) in built.centers.iter().enumerate().skip(1) {
                    let component = reached(&built.doc.map, center, &junction);
                    for (other, &other_center) in built.centers.iter().enumerate().skip(1) {
                        if other != index {
                            assert!(!component.contains(&other_center));
                        }
                    }
                }
            }
        }
        assert!(generate_shape(&assets, 0, Shape::Cross, &[5, 5, 5], false).is_err());
        assert!(generate_shape(&assets, 0, Shape::Y, &[5, 5, 9], false).is_err());
    }

    #[test]
    fn bounded_grid_graph_accepts_forks_and_loops_without_extra_connections() {
        let assets = Path::new("/missing");
        assert!(generate_grid_graph(assets, 0, &[[0, 0]], &[], false).is_err());
        assert!(generate_grid_graph(assets, 0, &[[0, 0], [0, 0]], &[[0, 1]], false).is_err());
        assert!(generate_grid_graph(assets, 0, &[[0, 0], [1, 1]], &[[0, 1]], false).is_err());
        assert!(generate_grid_graph(assets, 0, &[[0, 0], [1, 0]], &[[0, 2]], false).is_err());
        assert!(
            generate_grid_graph(assets, 0, &[[0, 0], [1, 0]], &[[0, 1], [1, 0]], false).is_err()
        );
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let nodes = [[1, 1], [1, 0], [0, 1], [2, 1], [1, 2], [0, 0]];
        let links = [[0, 1], [0, 2], [0, 3], [0, 4], [1, 5], [2, 5]];
        let plain = generate_grid_graph(&assets, 19, &nodes, &links, false).unwrap();
        let jagged = generate_grid_graph(&assets, 19, &nodes, &links, true).unwrap();
        if let Some(output) = std::env::var_os("ZIRCON_BRANCH_QA_DIR") {
            let output = std::path::PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(output.join("branch_grid.map"), plain.doc.map.to_bytes()).unwrap();
        }
        assert!(jagged.doc.map.walkable_count() < plain.doc.map.walkable_count());
        for built in [&plain, &jagged] {
            assert!(built.arms.is_empty());
            assert_eq!(built.centers.len(), nodes.len());
            built.layout.validate(&built.doc.map).unwrap();
            let all = reached(&built.doc.map, built.centers[0], &BTreeSet::new());
            assert_eq!(all.len(), built.doc.map.walkable_count());
            let junction: BTreeSet<_> = (46..=67)
                .flat_map(|x| (46..=67).map(move |y| (x, y)))
                .collect();
            let north = reached(&built.doc.map, built.centers[1], &junction);
            assert!(north.contains(&built.centers[2])); // alternate loop through northwest
            assert!(north.contains(&built.centers[5]));
            assert!(!north.contains(&built.centers[3]));
            assert!(!north.contains(&built.centers[4]));
        }
    }

    #[test]
    fn twelve_room_grid_graph_remains_connected_and_enclosed() {
        let nodes: Vec<[u8; 2]> = (1..=3)
            .flat_map(|row| (1..=4).map(move |column| [column, row]))
            .collect();
        let mut links = Vec::new();
        for row in 0..3 {
            for column in 0..4 {
                let index = row * 4 + column;
                if column < 3 {
                    links.push([index, index + 1]);
                }
                if row < 2 {
                    links.push([index, index + 4]);
                }
            }
        }
        assert_eq!((nodes.len(), links.len()), (12, 17));
        assert!(generate_grid_graph(
            Path::new("/missing"),
            9,
            &[[0, 0], [5, 0]],
            &[[0, 1]],
            false
        )
        .is_err());
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let built = generate_grid_graph(&assets, 19, &nodes, &links, true).unwrap();
        if let Some(output) = std::env::var_os("ZIRCON_BRANCH_QA_DIR") {
            let output = std::path::PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(output.join("branch_grid12.map"), built.doc.map.to_bytes()).unwrap();
        }
        assert_eq!((built.doc.map.width, built.doc.map.height), (176, 176));
        built.layout.validate(&built.doc.map).unwrap();
        let all = reached(&built.doc.map, built.centers[0], &BTreeSet::new());
        assert_eq!(all.len(), built.doc.map.walkable_count());
        assert!(built.centers.iter().all(|center| all.contains(center)));
    }
}
