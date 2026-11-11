//! Linear Wood rooms sharing physical necks inside one closed authored outline.
use crate::{
    generate::Layout, tile_doc::Document, wood_concave, wood_perimeter, wood_perimeter_kit,
    wood_wall_bodies,
};
use std::path::Path;

/// Generated map, layout and checked passage centers.
pub type Built = (Document, Layout, Vec<(i32, i32)>);

/// Validate before reading assets or constructing a potentially oversized chain.
pub fn validate_parameters(nw: u16, ne: u16, rooms: u8) -> anyhow::Result<()> {
    validate_varied_parameters(&vec![[nw, ne]; usize::from(rooms)])
}

/// Nonuniform room runs retain the measured source corner and wall orientations.
pub fn validate_varied_parameters(repeats: &[[u16; 2]]) -> anyhow::Result<()> {
    anyhow::ensure!(
        (2..=3).contains(&repeats.len()),
        "Connected Wood layouts need two or three rooms."
    );
    anyhow::ensure!(
        repeats
            .iter()
            .all(|&[nw, ne]| (3..=10).contains(&nw) && (3..=10).contains(&ne)),
        "Connected Wood room repeats must be 3–10 per side."
    );
    let placements: u32 = repeats
        .iter()
        .map(|&[nw, ne]| 2 * (u32::from(nw) + u32::from(ne)) + 6)
        .sum();
    anyhow::ensure!(placements <= 64, "Connected Wood layout needs {placements} placements; maximum is 64. Reduce rooms or side repeats.");
    Ok(())
}

/// Necks are ordered left to right. Each point is the walkable center of a
/// vertical eight-cell cut that separates the adjoining rooms.
pub fn generate_rooms_with_necks(
    assets: &Path,
    nw: u16,
    ne: u16,
    rooms: u8,
    seed: u64,
) -> anyhow::Result<Built> {
    build(assets, &vec![[nw, ne]; usize::from(rooms)], seed)
}

/// Also return the center of each checked walkable neck, left to right.
pub fn generate_varied_with_necks(
    assets: &Path,
    repeats: &[[u16; 2]],
    seed: u64,
) -> anyhow::Result<Built> {
    build(assets, repeats, seed)
}

fn build(assets: &Path, repeats: &[[u16; 2]], seed: u64) -> anyhow::Result<Built> {
    validate_varied_parameters(repeats)?;
    let rooms = repeats.len();
    let corners = wood_perimeter_kit::corners(assets)?;
    let concave = wood_concave::corners(assets)?;
    let bodies = wood_wall_bodies::bodies(assets)?;
    let mut chain = vec![(&corners[3], 1usize, 0usize)];
    let mut valleys = vec![];
    for (room, &[nw, ne]) in repeats.iter().enumerate() {
        for _ in 0..nw {
            chain.push((&bodies[0], 0, 1));
        }
        chain.push((&corners[0], 0, 1));
        for _ in 0..ne {
            chain.push((&bodies[1], 0, 1));
        }
        if room + 1 < rooms {
            valleys.push(chain.len());
            chain.push((&concave[0], 0, 1));
        }
    }
    chain.push((&corners[1], 0, 1));
    let mut notches = vec![];
    for (room, &[nw, ne]) in repeats.iter().rev().enumerate() {
        // Complete source corner collars have asymmetric lengths. The east
        // end needs two extra SE bodies; each further room needs one.
        for _ in 0..nw + if room == 0 { 2 } else { 1 } {
            chain.push((&bodies[3], 1, 0));
        }
        chain.push((&corners[2], 1, 0));
        for _ in 0..ne + u16::from(room > 0) {
            chain.push((&bodies[2], 1, 0));
        }
        if room + 1 < rooms {
            notches.push(chain.len());
            chain.push((&concave[1], 1, 0));
        }
    }
    let (doc, mut layout, origins) = wood_perimeter::compose_ring(&chain, assets, seed)?;
    let mut necks = vec![];
    for (&valley, &notch) in valleys.iter().zip(notches.iter().rev()) {
        let valley = origins[valley];
        let notch = origins[notch];
        anyhow::ensure!(
            notch == (valley.0, valley.1 + 14),
            "Connected room neck geometry changed."
        );
        // Six by four floor cells form a physical crossing between full collars.
        for x in valley.0..valley.0 + 6 {
            for y in valley.1 + 10..valley.1 + 14 {
                anyhow::ensure!(doc.map.is_walkable(x, y), "Connected room neck is blocked.");
            }
        }
        necks.push((valley.0 + 3, valley.1 + 12));
    }
    if repeats.iter().all(|&side| side == repeats[0]) {
        layout.wall_repeats = Some(repeats[0]);
    } else {
        layout.wall_room_repeats = repeats.to_vec();
    }
    layout.wall_rooms = rooms as u8;
    layout.validate(&doc.map)?;
    Ok((doc, layout, necks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, VecDeque};
    fn connected(
        map: &mir_formats::map::MapFile,
        start: (i32, i32),
        blocked: &BTreeSet<(i32, i32)>,
    ) -> BTreeSet<(i32, i32)> {
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some((x, y)) = queue.pop_front() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let q = (x + dx, y + dy);
                if !blocked.contains(&q) && map.is_walkable(q.0, q.1) && seen.insert(q) {
                    queue.push_back(q);
                }
            }
        }
        seen
    }
    #[test]
    fn every_supported_layout_closes_and_each_physical_neck_separates_rooms() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = std::fs::read(assets.join("Map/D1008.map")).unwrap();
        let mut supported = 0;
        for rooms in 2..=3 {
            for nw in 3..=10 {
                for ne in 3..=10 {
                    if validate_parameters(nw, ne, rooms).is_err() {
                        continue;
                    }
                    let (doc, layout, necks) = build(&assets, &vec![[nw, ne]; rooms as usize], 42)
                        .unwrap_or_else(|error| panic!("rooms={rooms}, nw={nw}, ne={ne}: {error}"));
                    supported += 1;
                    assert_eq!(layout.wall_rooms, rooms);
                    assert_eq!(layout.wall_repeats, Some([nw, ne]));
                    assert_eq!(necks.len(), usize::from(rooms - 1));
                    assert!(layout.enclosed);
                    layout.validate(&doc.map).unwrap();
                    let reached = connected(&doc.map, necks[0], &BTreeSet::new());
                    assert_eq!(reached.len(), doc.map.walkable_count());
                    let mut all_cuts = BTreeSet::new();
                    for &(x, y) in &necks {
                        // Include the walkable source collars around the four-row
                        // interior gap. Do not cut unrelated map columns.
                        let cut: BTreeSet<_> = (y - 3..y + 5).map(|yy| (x, yy)).collect();
                        let left = connected(&doc.map, (x - 1, y), &cut);
                        let right = connected(&doc.map, (x + 1, y), &cut);
                        assert!(left.len() > 64 && right.len() > 64);
                        assert!(
                            left.is_disjoint(&right),
                            "neck bypass: {rooms}/{nw}/{ne}, neck={x},{y}, walk column={:?}",
                            (0..i32::from(doc.map.height))
                                .filter(|yy| doc.map.is_walkable(x, *yy))
                                .collect::<Vec<_>>()
                        );
                        assert_eq!(left.len() + right.len() + cut.len(), reached.len());
                        all_cuts.extend(cut);
                    }
                    let mut remaining: BTreeSet<_> =
                        reached.difference(&all_cuts).copied().collect();
                    let mut components = 0;
                    while let Some(&start) = remaining.first() {
                        let room = connected(&doc.map, start, &all_cuts);
                        assert!(room.len() > 64);
                        remaining.retain(|point| !room.contains(point));
                        components += 1;
                    }
                    assert_eq!(components, rooms);
                    assert!(doc.map.width <= 256 && doc.map.height <= 256);
                    if let Some(out) = std::env::var_os("ZIRCON_CONNECTED_QA_DIR") {
                        let out = std::path::PathBuf::from(out);
                        std::fs::create_dir_all(&out).unwrap();
                        let name = if rooms == 2 {
                            format!("connected-{nw}-{ne}")
                        } else {
                            format!("connected-{rooms}-{nw}-{ne}")
                        };
                        std::fs::write(out.join(format!("{name}.map")), doc.map.to_bytes())
                            .unwrap();
                        std::fs::write(
                            out.join(format!("{name}.cave.json")),
                            serde_json::to_vec_pretty(&(layout, 1)).unwrap(),
                        )
                        .unwrap();
                    }
                }
            }
        }
        assert_eq!(supported, 39);
        assert_eq!(std::fs::read(assets.join("Map/D1008.map")).unwrap(), source);
    }
    #[test]
    fn varied_room_sides_close_and_each_neck_remains_the_only_crossing() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = std::fs::read(assets.join("Map/D1008.map")).unwrap();
        for repeats in [
            vec![[3, 3], [4, 3]],
            vec![[3, 6], [5, 3]],
            vec![[10, 3], [3, 10]],
            vec![[3, 3], [4, 3], [3, 3]],
            vec![[3, 4], [3, 3], [4, 3]],
            vec![[3, 5], [4, 3], [3, 3]],
        ] {
            let (doc, layout, necks) = generate_varied_with_necks(&assets, &repeats, 42)
                .unwrap_or_else(|error| panic!("{repeats:?}: {error}"));
            assert_eq!(layout.wall_room_repeats, repeats);
            assert_eq!(layout.wall_repeats, None);
            assert_eq!(necks.len(), repeats.len() - 1);
            layout.validate(&doc.map).unwrap();
            let mut cuts = BTreeSet::new();
            for &(x, y) in &necks {
                let cut: BTreeSet<_> = (y - 3..y + 5).map(|yy| (x, yy)).collect();
                assert!(
                    connected(&doc.map, (x - 1, y), &cut).is_disjoint(&connected(
                        &doc.map,
                        (x + 1, y),
                        &cut
                    ))
                );
                cuts.extend(cut);
            }
            let mut remaining: BTreeSet<_> = connected(&doc.map, necks[0], &BTreeSet::new())
                .difference(&cuts)
                .copied()
                .collect();
            let mut components = 0;
            while let Some(&start) = remaining.first() {
                let component = connected(&doc.map, start, &cuts);
                assert!(component.len() > 64);
                remaining.retain(|point| !component.contains(point));
                components += 1;
            }
            assert_eq!(components, repeats.len(), "{repeats:?}");
            if repeats == [[3, 3], [4, 3], [3, 3]] {
                if let Some(out) = std::env::var_os("ZIRCON_CONNECTED_QA_DIR") {
                    let out = std::path::PathBuf::from(out);
                    std::fs::create_dir_all(&out).unwrap();
                    std::fs::write(out.join("connected-varied-3.map"), doc.map.to_bytes()).unwrap();
                    std::fs::write(
                        out.join("connected-varied-3.cave.json"),
                        serde_json::to_vec_pretty(&(layout, 1)).unwrap(),
                    )
                    .unwrap();
                }
            }
        }
        assert_eq!(std::fs::read(assets.join("Map/D1008.map")).unwrap(), source);
    }
    #[test]
    fn varied_room_budget_extremes_build_from_source_art() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        for nw in 3..=10 {
            for ne in 3..=10 {
                let repeats = [[3, 3], [nw, ne]];
                let (doc, layout, _) = generate_varied_with_necks(&assets, &repeats, 17)
                    .unwrap_or_else(|error| panic!("{repeats:?}: {error}"));
                layout.validate(&doc.map).unwrap();
            }
        }
        for index in 0..3 {
            for nw in 3..=8 {
                for ne in 3..=8 {
                    let mut repeats = [[3, 3]; 3];
                    repeats[index] = [nw, ne];
                    if validate_varied_parameters(&repeats).is_err() {
                        continue;
                    }
                    let (doc, layout, _) = generate_varied_with_necks(&assets, &repeats, 17)
                        .unwrap_or_else(|error| panic!("{repeats:?}: {error}"));
                    layout.validate(&doc.map).unwrap();
                }
            }
        }
    }
    #[test]
    fn unsupported_repeat_counts_fail_before_reading_assets() {
        for rooms in [0, 1, 4, 255] {
            assert!(validate_parameters(3, 3, rooms).is_err());
            assert!(
                generate_rooms_with_necks(Path::new("missing-assets"), 3, 3, rooms, 0).is_err()
            );
        }
        assert!(validate_parameters(4, 4, 3).is_err());
        assert!(validate_parameters(u16::MAX, u16::MAX, 255).is_err());
        for (nw, ne) in [(2, 3), (3, 11), (11, 3), (3, 2)] {
            assert!(generate_rooms_with_necks(Path::new("missing-assets"), nw, ne, 2, 0).is_err());
        }
    }
}
