//! Atomic fixed-placement composition, including cycles with several closing seams.
use crate::{
    cave_art, piece_ports,
    tile_doc::{Document, Stamp},
    wall_piece::Piece,
};
use anyhow::Context;
use mir_formats::map::MapFile;
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
};

pub struct Placement<'a> {
    pub piece: &'a Piece,
    pub at: (i32, i32),
}
#[derive(Clone, Copy)]
pub struct Seam {
    pub a: usize,
    pub a_port: usize,
    pub b: usize,
    pub b_port: usize,
}

/// Validate an explicit connected seam graph and publish all owned cells in one
/// undo transaction. Unused ports must remain exposed after every seam closes.
/// This does not fill an interior or establish that the result is enclosed.
pub fn compose(
    placements: &[Placement<'_>],
    seams: &[Seam],
    width: u16,
    height: u16,
    assets: &Path,
) -> anyhow::Result<Document> {
    anyhow::ensure!(
        !placements.is_empty() && placements.len() <= 64,
        "Compose between 1 and 64 placed pieces."
    );
    anyhow::ensure!(
        (2..=256).contains(&width)
            && (2..=256).contains(&height)
            && width.is_multiple_of(2)
            && height.is_multiple_of(2),
        "Composition canvas must have even dimensions between 2 and 256."
    );
    anyhow::ensure!(
        seams.len() <= 512,
        "Composition supports at most 512 explicit seams."
    );
    let mut candidate = Document::blank(width, height);
    for x in 0..i32::from(width) {
        for y in 0..i32::from(height) {
            let cell = candidate.map.cell_mut(x, y).unwrap();
            cell.flag = 0;
            cell.back_file = 255;
        }
    }
    let mut coverage = vec![false; usize::from(width) * usize::from(height)];
    let mut stamp = Stamp { cells: Vec::new() };
    let mut assembled = candidate.map.clone();
    for (index, placement) in placements.iter().enumerate() {
        let p = placement.piece;
        p.serialized(&p.name, p.kind)
            .with_context(|| format!("Invalid piece {}", index + 1))?;
        piece_ports::validate_boundary(p.map(), &p.ports, p.coverage())?;
        let (x, y) = placement.at;
        anyhow::ensure!(
            x >= 0
                && y >= 0
                && x <= i32::from(width)
                && y <= i32::from(height)
                && x % 2 == 0
                && y % 2 == 0,
            "Piece {} needs an even nonnegative origin inside the canvas.",
            index + 1
        );
        anyhow::ensure!(
            x + i32::from(p.map().width) <= i32::from(width)
                && y + i32::from(p.map().height) <= i32::from(height),
            "Piece {} extends past the canvas.",
            index + 1
        );
        let (left, top, right, bottom) = cave_art::source_extent(p.map(), assets)?;
        anyhow::ensure!(
            x + left >= 0
                && y + top >= 0
                && x + right <= i32::from(width)
                && y + bottom <= i32::from(height),
            "Piece {} artwork extends past the canvas.",
            index + 1
        );
        for &(cx, cy, cell) in p.cells() {
            let (tx, ty) = (x + cx, y + cy);
            let owned = &mut coverage[(ty * i32::from(width) + tx) as usize];
            anyhow::ensure!(!*owned, "Placed pieces overlap owned cells at ({tx},{ty}).");
            *owned = true;
            *assembled.cell_mut(tx, ty).unwrap() = cell;
            stamp.cells.push((tx, ty, cell));
        }
    }
    let mut consumed = BTreeSet::new();
    let mut graph = vec![Vec::new(); placements.len()];
    for seam in seams {
        anyhow::ensure!(
            seam.a < placements.len() && seam.b < placements.len() && seam.a != seam.b,
            "Seam must connect two distinct placed pieces."
        );
        let a = &placements[seam.a];
        let b = &placements[seam.b];
        let pa = a
            .piece
            .ports
            .get(seam.a_port)
            .context("Seam references a missing connection.")?;
        let pb = b
            .piece
            .ports
            .get(seam.b_port)
            .context("Seam references a missing connection.")?;
        anyhow::ensure!(
            consumed.insert((seam.a, seam.a_port)) && consumed.insert((seam.b, seam.b_port)),
            "A connection can belong to only one seam."
        );
        let expected = piece_ports::alignment(a.piece.map(), pa, b.piece.map(), pb)?;
        anyhow::ensure!(
            expected == (b.at.0 - a.at.0, b.at.1 - a.at.1),
            "Seam connections do not meet at the placed coordinates."
        );
        graph[seam.a].push(seam.b);
        graph[seam.b].push(seam.a);
    }
    let mut reached = BTreeSet::from([0]);
    let mut queue = VecDeque::from([0]);
    while let Some(index) = queue.pop_front() {
        for &other in &graph[index] {
            if reached.insert(other) {
                queue.push_back(other);
            }
        }
    }
    anyhow::ensure!(
        reached.len() == placements.len(),
        "Explicit seams must connect every placed piece."
    );
    let mut unused = Vec::new();
    for (index, placement) in placements.iter().enumerate() {
        for (port_index, port) in placement.piece.ports.iter().enumerate() {
            if !consumed.contains(&(index, port_index)) {
                let translated =
                    piece_ports::translate(port, placement.piece.map(), placement.at, &assembled)?;
                unused.push(translated);
            }
        }
    }
    // The reusable-piece 16-port limit does not cap the whole canvas.
    // Batching avoids cloning the full coverage map once per open endpoint.
    for ports in unused.chunks(16) {
        piece_ports::validate_boundary(&assembled, ports, Some(&coverage))?;
    }
    validate_connected_floor(&assembled)?;
    candidate.stamp((0, 0), &stamp);
    Ok(candidate)
}

fn validate_connected_floor(map: &MapFile) -> anyhow::Result<()> {
    let walkable: BTreeSet<_> = (0..i32::from(map.width))
        .flat_map(|x| (0..i32::from(map.height)).map(move |y| (x, y)))
        .filter(|&(x, y)| map.is_walkable(x, y))
        .collect();
    let Some(&start) = walkable.first() else {
        return Ok(());
    };
    let mut seen = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some((x, y)) = queue.pop_front() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = (x + dx, y + dy);
            if map.is_walkable(next.0, next.1) && seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    anyhow::ensure!(
        seen.len() == walkable.len(),
        "Composition contains disconnected walkable floor."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        piece_ports::{Port, Side},
        wall_piece::Kind,
    };
    fn piece(sides: &[(Side, u16)]) -> Piece {
        let mut map = MapFile::blank(4, 4);
        for x in 0..4 {
            for y in 0..4 {
                let c = map.cell_mut(x, y).unwrap();
                c.flag = 3;
                c.back_file = 255;
            }
        }
        let mut piece = Piece::capture(
            &Stamp::capture(&map, (0, 0), (3, 3)),
            "Cycle component".into(),
            Kind::Wall,
            Path::new("."),
        )
        .unwrap();
        piece.ports = sides
            .iter()
            .map(|&(side, offset)| Port {
                side,
                offset,
                inset: 0,
                width: 2,
                profile: "cycle".into(),
            })
            .collect();
        piece
    }
    fn cycle() -> Vec<Piece> {
        vec![
            piece(&[(Side::East, 0), (Side::South, 0)]),
            piece(&[(Side::West, 0), (Side::South, 2)]),
            piece(&[(Side::North, 2), (Side::West, 2)]),
            piece(&[(Side::North, 0), (Side::East, 2)]),
        ]
    }
    fn placements(pieces: &[Piece]) -> Vec<Placement<'_>> {
        pieces
            .iter()
            .zip([(2, 2), (6, 2), (6, 6), (2, 6)])
            .map(|(piece, at)| Placement { piece, at })
            .collect()
    }
    fn seams() -> Vec<Seam> {
        vec![
            Seam {
                a: 0,
                a_port: 0,
                b: 1,
                b_port: 0,
            },
            Seam {
                a: 1,
                a_port: 1,
                b: 2,
                b_port: 0,
            },
            Seam {
                a: 2,
                a_port: 1,
                b: 3,
                b_port: 1,
            },
            Seam {
                a: 3,
                a_port: 0,
                b: 0,
                b_port: 1,
            },
        ]
    }
    #[test]
    fn closing_cycle_is_atomic_and_preserves_the_inputs() {
        let pieces = cycle();
        let before: Vec<_> = pieces
            .iter()
            .map(|p| p.serialized(&p.name, p.kind).unwrap())
            .collect();
        let ab = pieces[0].join(&pieces[1], 0, 0, Path::new(".")).unwrap();
        let abc = ab.join(&pieces[2], 1, 0, Path::new(".")).unwrap();
        assert!(abc.join(&pieces[3], 0, 0, Path::new(".")).is_err());
        let mut result = compose(&placements(&pieces), &seams(), 12, 12, Path::new(".")).unwrap();
        for x in 0..12 {
            for y in 0..12 {
                assert_eq!(
                    result.map.is_walkable(x, y),
                    (2..10).contains(&x) && (2..10).contains(&y)
                );
                if !(2..10).contains(&x) || !(2..10).contains(&y) {
                    assert_eq!(result.map.cell(x, y).unwrap().back_file, 255);
                }
            }
        }
        result.undo();
        let undone = result.map.to_bytes();
        result.undo();
        assert_eq!(result.map.to_bytes(), undone);
        assert!((0..12).all(|x| (0..12).all(|y| !result.map.is_walkable(x, y))));
        for (piece, bytes) in pieces.iter().zip(before) {
            assert_eq!(piece.serialized(&piece.name, piece.kind).unwrap(), bytes);
        }
    }
    #[test]
    fn duplicate_missing_unpaired_and_disconnected_seams_are_rejected() {
        let pieces = cycle();
        let p = placements(&pieces);
        let mut duplicate = seams();
        duplicate.push(duplicate[0]);
        assert!(compose(&p, &duplicate, 12, 12, Path::new("."))
            .err()
            .unwrap()
            .to_string()
            .contains("only one seam"));
        let mut missing = seams();
        missing[0].a_port = 9;
        assert!(compose(&p, &missing, 12, 12, Path::new(".")).is_err());
        assert!(compose(&p, &seams()[..3], 12, 12, Path::new("."))
            .err()
            .unwrap()
            .to_string()
            .contains("uncovered space"));
        assert!(compose(&p, &seams()[..2], 12, 12, Path::new("."))
            .err()
            .unwrap()
            .to_string()
            .contains("connect every"));
    }
    #[test]
    fn invalid_placement_dimensions_overlap_and_seam_alignment_are_rejected() {
        let pieces = cycle();
        let mut p = placements(&pieces);
        p[1].at.0 = 7;
        assert!(compose(&p, &seams(), 16, 12, Path::new("."))
            .err()
            .unwrap()
            .to_string()
            .contains("even"));
        p[1].at.0 = 4;
        assert!(compose(&p, &seams(), 16, 12, Path::new("."))
            .err()
            .unwrap()
            .to_string()
            .contains("overlap"));
        p[1].at.0 = 10;
        assert!(compose(&p, &seams(), 16, 12, Path::new("."))
            .err()
            .unwrap()
            .to_string()
            .contains("do not meet"));
        assert!(compose(&placements(&pieces), &seams(), 10, 9, Path::new(".")).is_err());
        assert!(compose(&placements(&pieces), &seams(), 258, 12, Path::new(".")).is_err());
        assert!(compose(&[], &[], 12, 12, Path::new(".")).is_err());
    }
    #[test]
    fn unowned_padding_can_overlap_and_recessed_seams_align() {
        let source = piece(&[]);
        let mut mask = vec![true; 16];
        for y in 0..2 {
            for x in 2..4 {
                mask[y * 4 + x] = false;
            }
        }
        let mut a = Piece::capture_covered(
            &Stamp::capture(source.map(), (0, 0), (3, 3)),
            "Recess".into(),
            Kind::Wall,
            Path::new("."),
            mask,
        )
        .unwrap();
        a.ports = vec![Port {
            side: Side::East,
            inset: 2,
            offset: 0,
            width: 2,
            profile: "recess".into(),
        }];
        let mut b = Piece::capture(
            &Stamp::capture(source.map(), (0, 0), (1, 1)),
            "Filler".into(),
            Kind::Wall,
            Path::new("."),
        )
        .unwrap();
        b.ports = vec![Port {
            side: Side::West,
            inset: 0,
            offset: 0,
            width: 2,
            profile: "recess".into(),
        }];
        let result = compose(
            &[
                Placement {
                    piece: &a,
                    at: (2, 2),
                },
                Placement {
                    piece: &b,
                    at: (4, 2),
                },
            ],
            &[Seam {
                a: 0,
                a_port: 0,
                b: 1,
                b_port: 0,
            }],
            8,
            8,
            Path::new("."),
        )
        .unwrap();
        assert!((2..6).all(|x| (2..6).all(|y| result.map.is_walkable(x, y))));
    }
    #[test]
    fn incompatible_ports_and_disconnected_floor_are_rejected() {
        let mut pieces = cycle();
        pieces[1].ports[0].profile = "other".into();
        assert!(compose(&placements(&pieces), &seams(), 12, 12, Path::new(".")).is_err());
        let mut map = MapFile::blank(6, 6);
        for x in 0..6 {
            for y in 0..6 {
                map.cell_mut(x, y).unwrap().back_file = 255;
            }
        }
        map.cell_mut(0, 0).unwrap().flag = 3;
        map.cell_mut(5, 5).unwrap().flag = 3;
        let p = Piece::capture(
            &Stamp::capture(&map, (0, 0), (5, 5)),
            "Islands".into(),
            Kind::Formation,
            Path::new("."),
        )
        .unwrap();
        assert!(compose(
            &[Placement {
                piece: &p,
                at: (0, 0)
            }],
            &[],
            8,
            8,
            Path::new(".")
        )
        .err()
        .unwrap()
        .to_string()
        .contains("disconnected walkable"));
    }
}
