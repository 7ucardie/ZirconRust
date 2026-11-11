//! Closed authored Wood wall rings with source-floor fill inside owned boundaries.
use crate::{
    generate::{self, Layout},
    piece_composition::{self, Placement, Seam},
    piece_ports,
    tile_doc::{Document, Stamp},
    wall_piece::Piece,
    wood_perimeter_kit, wood_wall_bodies,
};
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
};

/// Repeat counts select the two independent diamond sides. Opposite counts are
/// derived from the measured asymmetric corner approaches, never guessed equal.
pub fn generate(assets: &Path, nw: u16, ne: u16, seed: u64) -> anyhow::Result<(Document, Layout)> {
    anyhow::ensure!(
        (3..=10).contains(&nw) && (3..=10).contains(&ne),
        "Wood perimeter repeats must be 3–10 on each side."
    );
    let corners = wood_perimeter_kit::corners(assets)?;
    let bodies = wood_wall_bodies::bodies(assets)?;
    // Source roles: N,E,S,W then NW,NE,SW,SE bodies.
    let mut chain = vec![(&corners[3], 1usize, 0usize)];
    for _ in 0..nw {
        chain.push((&bodies[0], 0, 1));
    }
    chain.push((&corners[0], 0, 1));
    for _ in 0..ne {
        chain.push((&bodies[1], 0, 1));
    }
    chain.push((&corners[1], 0, 1));
    for _ in 0..nw + 2 {
        chain.push((&bodies[3], 1, 0));
    }
    chain.push((&corners[2], 1, 0));
    for _ in 0..ne {
        chain.push((&bodies[2], 1, 0));
    }
    let (doc, mut layout, _) = compose_ring(&chain, assets, seed)?;
    layout.wall_repeats = Some([nw, ne]);
    layout.wall_rooms = 1;
    layout.validate(&doc.map)?;
    Ok((doc, layout))
}

/// Compose a measured clockwise wall recipe and fill only its enclosed interior.
/// Returns translated piece origins for callers to verify authored neck geometry.
pub(crate) type ComposedRing = (Document, Layout, Vec<(i32, i32)>);
pub(crate) fn compose_ring(
    chain: &[(&Piece, usize, usize)],
    assets: &Path,
    seed: u64,
) -> anyhow::Result<ComposedRing> {
    anyhow::ensure!(
        !chain.is_empty() && chain.len() <= 64,
        "A wall ring needs 1–64 placements."
    );
    let mut origins = vec![(0i32, 0i32)];
    let mut seams = vec![];
    for i in 1..chain.len() {
        let (prev, _, out) = chain[i - 1];
        let (next, input, _) = chain[i];
        let delta =
            piece_ports::alignment(prev.map(), &prev.ports[out], next.map(), &next.ports[input])?;
        origins.push((origins[i - 1].0 + delta.0, origins[i - 1].1 + delta.1));
        seams.push(Seam {
            a: i - 1,
            a_port: out,
            b: i,
            b_port: input,
        });
    }
    let last = chain.len() - 1;
    let delta = piece_ports::alignment(
        chain[last].0.map(),
        &chain[last].0.ports[chain[last].2],
        chain[0].0.map(),
        &chain[0].0.ports[chain[0].1],
    )?;
    anyhow::ensure!(
        origins[last].0 + delta.0 == 0 && origins[last].1 + delta.1 == 0,
        "Authored wall recipe does not close."
    );
    seams.push(Seam {
        a: last,
        a_port: chain[last].2,
        b: 0,
        b_port: chain[0].1,
    });
    let left = origins.iter().map(|p| p.0).min().unwrap();
    let top = origins.iter().map(|p| p.1).min().unwrap();
    let right = origins
        .iter()
        .zip(chain)
        .map(|(p, c)| p.0 + i32::from(c.0.size().0))
        .max()
        .unwrap();
    let bottom = origins
        .iter()
        .zip(chain)
        .map(|(p, c)| p.1 + i32::from(c.0.size().1))
        .max()
        .unwrap();
    let width = (right - left + 8) as u16;
    let height = (bottom - top + 8) as u16;
    let placements: Vec<_> = chain
        .iter()
        .zip(origins.iter())
        .map(|(c, p)| Placement {
            piece: c.0,
            at: (p.0 - left + 4, p.1 - top + 4),
        })
        .collect();
    let mut doc = piece_composition::compose(&placements, &seams, width, height, assets)?;
    let mut owned = BTreeSet::new();
    for p in &placements {
        for &(x, y, _) in p.piece.cells() {
            owned.insert(((p.at.0 + x) / 2, (p.at.1 + y) / 2));
        }
    }
    let bw = i32::from(width) / 2;
    let bh = i32::from(height) / 2;
    let mut outside = BTreeSet::new();
    let mut queue = VecDeque::from([(0, 0)]);
    outside.insert((0, 0));
    while let Some((x, y)) = queue.pop_front() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let q = (x + dx, y + dy);
            if q.0 >= 0
                && q.1 >= 0
                && q.0 < bw
                && q.1 < bh
                && !owned.contains(&q)
                && outside.insert(q)
            {
                queue.push_back(q);
            }
        }
    }
    let interior: BTreeSet<_> = (0..bw)
        .flat_map(|x| (0..bh).map(move |y| (x, y)))
        .filter(|p| !owned.contains(p) && !outside.contains(p))
        .collect();
    anyhow::ensure!(
        !interior.is_empty(),
        "Wall ownership does not surround an interior."
    );
    let source = mir_formats::map::MapFile::load(assets.join("Map/D1008.map"))?;
    let floor = generate::paint_floor_compatible(
        &source,
        width,
        height,
        &interior,
        &mut generate::Random(seed),
    )?;
    let mut stamp = Stamp { cells: vec![] };
    for &(x, y) in &interior {
        for dx in 0..2 {
            for dy in 0..2 {
                let p = (x * 2 + dx, y * 2 + dy);
                stamp
                    .cells
                    .push((p.0, p.1, *floor.map.cell(p.0, p.1).unwrap()));
            }
        }
    }
    doc.stamp((0, 0), &stamp);
    // The full source wall collar is retained. Any walkable leak into an outside
    // ownership block or to the canvas border invalidates the generated room.
    for &(x, y) in &outside {
        for dx in 0..2 {
            for dy in 0..2 {
                anyhow::ensure!(
                    !doc.map.is_walkable(x * 2 + dx, y * 2 + dy),
                    "Perimeter has exterior walkable floor."
                );
            }
        }
    }
    let mut layout = generate::layout(&doc.map, seed)?;
    layout.enclosed = true;
    layout.validate(&doc.map)?;
    let origins = placements.iter().map(|placement| placement.at).collect();
    Ok((doc, layout, origins))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_advertised_sizes_close_with_connected_floor_and_preserve_sources() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = std::fs::read(assets.join("Map/D1008.map")).unwrap();
        for (nw, ne) in (3..=10).flat_map(|nw| (3..=10).map(move |ne| (nw, ne))) {
            let (doc, layout) = generate(&assets, nw, ne, 42).unwrap();
            assert!(layout.enclosed);
            layout.validate(&doc.map).unwrap();
            for x in 0..i32::from(doc.map.width) {
                assert!(!doc.map.is_walkable(x, 0));
                assert!(!doc.map.is_walkable(x, i32::from(doc.map.height) - 1));
            }
            for y in 0..i32::from(doc.map.height) {
                assert!(!doc.map.is_walkable(0, y));
                assert!(!doc.map.is_walkable(i32::from(doc.map.width) - 1, y));
            }
            if let Some(out) = std::env::var_os("ZIRCON_PERIMETER_QA_DIR")
                .filter(|_| [(3, 3), (3, 4), (5, 6), (10, 10)].contains(&(nw, ne)))
            {
                let out = std::path::PathBuf::from(out);
                std::fs::create_dir_all(&out).unwrap();
                std::fs::write(
                    out.join(format!("perimeter-{nw}-{ne}.map")),
                    doc.map.to_bytes(),
                )
                .unwrap();
                std::fs::write(
                    out.join(format!("perimeter-{nw}-{ne}.cave.json")),
                    serde_json::to_vec_pretty(&(layout, 1)).unwrap(),
                )
                .unwrap();
            }
        }
        assert_eq!(std::fs::read(assets.join("Map/D1008.map")).unwrap(), source);
    }
}
