//! Complete exposed cave and room boundaries with private Wood wall frames.
use crate::{
    generate::{Layout, Random},
    wood_branch,
};
use mir_formats::{
    map::MapFile,
    monster_table::kr_library,
    zl::{Sprite, SurfaceKind, ZlLibrary},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

type Point = (i32, i32);

fn has_sprite(file: u8) -> bool {
    file != 0 && kr_library(file).is_some()
}

fn threshold(texture: &[u8], vertical: bool) -> Vec<u8> {
    let mut rgba = vec![0; 48 * 32 * 4];
    for y in 0usize..32 {
        for x in 0usize..48 {
            let band = if vertical { x } else { y };
            if !(13..19).contains(&band) {
                continue;
            }
            let source_x = if vertical { y } else { x };
            let source_y = 72 + band - 13;
            let src = (source_y * 48 + source_x) * 4;
            let dst = (y * 48 + x) * 4;
            rgba[dst..dst + 3].copy_from_slice(&texture[src..src + 3]);
            rgba[dst + 3] = if band == 13 || band == 18 { 180 } else { 232 };
        }
    }
    rgba
}

fn doorway_cells(map: &MapFile, layout: &Layout) -> BTreeMap<Point, bool> {
    let corridor: BTreeSet<_> = layout.corridors.iter().copied().collect();
    let mut cells = BTreeMap::new();
    for room in &layout.rooms {
        for x in room.x..room.x + room.width {
            for (inside, outside) in [
                (room.y, room.y - 1),
                (room.y + room.height - 1, room.y + room.height),
            ] {
                if corridor.contains(&(x, outside))
                    && map.is_walkable(x, inside)
                    && map.is_walkable(x, outside)
                {
                    cells.insert((x, inside), false);
                }
            }
        }
        for y in room.y..room.y + room.height {
            for (inside, outside) in [
                (room.x, room.x - 1),
                (room.x + room.width - 1, room.x + room.width),
            ] {
                if corridor.contains(&(outside, y))
                    && map.is_walkable(inside, y)
                    && map.is_walkable(outside, y)
                {
                    cells.insert((inside, y), true);
                }
            }
        }
    }
    cells
}

/// Existing source formations keep their own sprites. Every other blocked cell
/// touching floor gets one matching, deterministic wall frame.
pub fn frame(
    map: &mut MapFile,
    assets: &Path,
    seed: u64,
    layout: Option<&Layout>,
) -> anyhow::Result<(Vec<u8>, usize)> {
    let (first, mut library) = wood_branch::wall_builder(assets)?;
    anyhow::ensure!(
        usize::from(first) + wood_branch::NEW_IMAGES + 2 <= usize::from(u16::MAX),
        "No image slots remain for Wood doorway art."
    );
    anyhow::ensure!(
        library.len() == usize::from(first) + wood_branch::NEW_IMAGES,
        "Wood wall slot contract changed."
    );
    anyhow::ensure!(
        library.len() + 2 <= usize::from(u16::MAX) + 1,
        "No image slots remain for Wood doorway thresholds."
    );
    let source = ZlLibrary::open(assets.join(wood_branch::LIBRARY))?;
    let cap = source
        .decode(340, SurfaceKind::Image)?
        .ok_or_else(|| anyhow::anyhow!("Wood threshold texture 340 is missing"))?;
    anyhow::ensure!(
        cap.width == 48 && cap.height >= 78,
        "Wood threshold texture changed."
    );
    for vertical in [false, true] {
        let rgba = threshold(&cap.rgba, vertical);
        library.push_rgba(&Sprite {
            width: 48,
            height: 32,
            offset_x: 0,
            offset_y: 0,
            rgba: &rgba,
            shadow_type: 0,
            shadow_offset_x: 0,
            shadow_offset_y: 0,
            shadow: None,
        });
    }
    let mut added = 0;
    for y in 1..i32::from(map.height) - 1 {
        for x in 1..i32::from(map.width) - 1 {
            if map.is_walkable(x, y)
                || !(-1..=1).any(|dx| (-1..=1).any(|dy| map.is_walkable(x + dx, y + dy)))
            {
                continue;
            }
            let cell = map.cell(x, y).unwrap();
            if has_sprite(cell.middle_file) || has_sprite(cell.front_file) {
                continue;
            }
            let mask = [(0, -1), (1, 0), (0, 1), (-1, 0)]
                .into_iter()
                .enumerate()
                .fold(0u8, |bits, (i, (dx, dy))| {
                    bits | (u8::from(map.is_walkable(x + dx, y + dy)) << i)
                });
            let phase = (Random(
                seed ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                    ^ (y as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9),
            )
            .next()
                & (wood_branch::PHASES as u64 - 1)) as u16;
            let cell = map.cell_mut(x, y).unwrap();
            cell.middle_file = 21;
            cell.middle_image = first + u16::from(mask) * wood_branch::PHASES as u16 + phase;
            added += 1;
        }
    }
    if let Some(layout) = layout {
        for ((x, y), vertical) in doorway_cells(map, layout) {
            let cell = map.cell_mut(x, y).unwrap();
            if !has_sprite(cell.middle_file) && !has_sprite(cell.front_file) {
                cell.middle_file = 21;
                cell.middle_image = first + wood_branch::NEW_IMAGES as u16 + u16::from(vertical);
            }
        }
    }
    Ok((library.to_bytes(), added))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_complete(map: &MapFile) {
        for y in 1..i32::from(map.height) - 1 {
            for x in 1..i32::from(map.width) - 1 {
                if map.is_walkable(x, y)
                    || !(-1..=1).any(|dx| (-1..=1).any(|dy| map.is_walkable(x + dx, y + dy)))
                {
                    continue;
                }
                let c = map.cell(x, y).unwrap();
                assert!(
                    has_sprite(c.middle_file) || has_sprite(c.front_file),
                    "bare edge {x},{y}"
                );
            }
        }
    }

    #[test]
    fn cave_and_dungeon_have_framed_edges_without_changing_source() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1008.map")).unwrap();
        let before = source.to_bytes();
        let (cave, cave_layout, _, cave_library, cave_walls) =
            crate::generate::cave_with_full_art(&source, 64, 64, 42, &assets).unwrap();
        let (rooms, room_layout, _, room_library, room_walls) =
            crate::dungeon::generate_full_art(&source, 80, 80, 42, 6, 4, &assets).unwrap();
        assert!(cave_walls > 0 && room_walls > 0);
        assert!(!cave_library.is_empty() && !room_library.is_empty());
        assert_complete(&cave.map);
        assert_complete(&rooms.map);
        let first = ZlLibrary::open(assets.join(wood_branch::LIBRARY))
            .unwrap()
            .len() as u16;
        let mouths = doorway_cells(&rooms.map, &room_layout);
        assert!(!mouths.is_empty());
        assert!(mouths.keys().any(|&(x, y)| {
            let cell = rooms.map.cell(x, y).unwrap();
            cell.is_walkable()
                && cell.middle_file == 21
                && (first + wood_branch::NEW_IMAGES as u16
                    ..=first + wood_branch::NEW_IMAGES as u16 + 1)
                    .contains(&cell.middle_image)
        }));
        cave_layout.validate(&cave.map).unwrap();
        room_layout.validate(&rooms.map).unwrap();
        assert_eq!(source.to_bytes(), before);
    }
}
