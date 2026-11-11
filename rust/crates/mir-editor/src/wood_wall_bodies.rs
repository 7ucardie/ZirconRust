//! Complete source wall columns with clear floor collars for diagonal repetition.
use crate::{
    piece_ports::{Port, Side},
    tile_doc::Stamp,
    wall_piece::{Kind, Piece},
};
use mir_formats::map::MapFile;
use std::path::Path;

struct Spec {
    name: &'static str,
    origin: (i32, i32),
    height: i32,
    west: u16,
    east: u16,
    motif: (i32, i32, u16),
    profile: &'static str,
}
const SPECS: [Spec; 4] = [
    Spec {
        name: "Wood NW wall body",
        origin: (10, 278),
        height: 12,
        west: 10,
        east: 8,
        motif: (10, 282, 538),
        profile: "wood-wall-nw",
    },
    Spec {
        name: "Wood NE wall body",
        origin: (40, 248),
        height: 12,
        west: 8,
        east: 10,
        motif: (40, 251, 445),
        profile: "wood-wall-ne",
    },
    Spec {
        name: "Wood SW wall body",
        origin: (14, 180),
        height: 10,
        west: 0,
        east: 2,
        motif: (14, 187, 612),
        profile: "wood-wall-sw",
    },
    Spec {
        name: "Wood SE wall body",
        origin: (26, 276),
        height: 10,
        west: 2,
        east: 0,
        motif: (26, 285, 628),
        profile: "wood-wall-se",
    },
];

/// Capture checked D1008 bodies. Profiles describe orientation; adjacent corners
/// still require an independently reviewed art seam.
pub fn bodies(assets: &Path) -> anyhow::Result<Vec<Piece>> {
    let source = MapFile::load(assets.join("Map/D1008.map"))?;
    SPECS
        .iter()
        .map(|spec| capture(&source, assets, spec, (0, 0)))
        .collect()
}

fn capture(
    source: &MapFile,
    assets: &Path,
    spec: &Spec,
    shift: (i32, i32),
) -> anyhow::Result<Piece> {
    check_motif(source, spec, shift)?;
    let (mx, my, expected) = spec.motif;
    let cell = source
        .cell(mx + shift.0, my + shift.1)
        .ok_or_else(|| anyhow::anyhow!("Wood wall body source is too small."))?;
    anyhow::ensure!(
        cell.middle_file == 21 && cell.middle_image == expected,
        "D1008 does not match the checked Wood wall body artwork."
    );
    let (x, y) = (spec.origin.0 + shift.0, spec.origin.1 + shift.1);
    crate::cave_art::validate_selection(source, assets, (x, y, x + 2, y + spec.height))?;
    let mut piece = Piece::capture(
        &Stamp::capture(source, (x, y), (x + 1, y + spec.height - 1)),
        spec.name.into(),
        Kind::Wall,
        assets,
    )?;
    piece.ports = [(Side::West, spec.west), (Side::East, spec.east)]
        .into_iter()
        .map(|(side, offset)| Port {
            side,
            offset,
            width: 2,
            inset: 0,
            profile: spec.profile.into(),
        })
        .collect();
    piece.serialized(&piece.name, piece.kind)?;
    Ok(piece)
}

fn check_motif(source: &MapFile, spec: &Spec, shift: (i32, i32)) -> anyhow::Result<()> {
    let (mx, my, image) = spec.motif;
    let (other_y, other_image, tails) = match image {
        538 => (-1, 533, 5),
        445 => (1, 440, 5),
        612 => (2, 611, 1),
        628 => (-2, 627, 1),
        _ => unreachable!("Static checked wall body specification"),
    };
    for (dx, dy, first) in [(0, 0, image), (1, other_y, other_image)] {
        for tail in 0..tails {
            let cell = source
                .cell(mx + shift.0 + dx, my + shift.1 + dy + tail)
                .ok_or_else(|| anyhow::anyhow!("Wood wall body source is too small."))?;
            anyhow::ensure!(
                cell.middle_file == 21 && cell.middle_image == first - tail as u16,
                "D1008 does not match the complete checked Wood wall body motif."
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tile_doc::Document, wall_piece::Run};

    #[test]
    fn every_wall_column_and_floor_tail_is_fingerprinted() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1008.map")).unwrap();
        for spec in &SPECS {
            check_motif(&source, spec, (0, 0)).unwrap();
            let mut changed = source.clone();
            let (x, y, image) = spec.motif;
            let dy = match image {
                538 => 3,
                445 => 5,
                612 => 2,
                628 => -2,
                _ => unreachable!(),
            };
            changed.cell_mut(x + 1, y + dy).unwrap().middle_image = 0;
            assert!(check_motif(&changed, spec, (0, 0)).is_err());
        }
    }

    #[test]
    fn four_diagonal_bodies_preserve_complete_source_art_and_repeated_collision() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1008.map")).unwrap();
        let pieces = bodies(&assets).unwrap();
        for (spec, piece) in SPECS.iter().zip(&pieces) {
            let dy = i32::from(spec.east) - i32::from(spec.west);
            let neighbor = capture(&source, &assets, spec, (2, dy)).unwrap();
            let original_pair = piece.join(&neighbor, 1, 0, &assets).unwrap();
            let at = (spec.origin.0, spec.origin.1 + dy.min(0));
            let mut exact = Document::open(assets.join("Map/D1008.map")).unwrap();
            let before = exact.map.to_bytes();
            original_pair
                .place(&mut exact, at, None, Run::default())
                .unwrap();
            assert_eq!(
                exact.map.to_bytes(),
                before,
                "{} source reassembly",
                spec.name
            );
            let repeated = piece.join(piece, 1, 0, &assets).unwrap();
            assert_eq!(repeated.ports.len(), 2);
            assert_eq!(repeated.size(), (4, (spec.height + 2) as u16));
            let mut preview = Document::open(assets.join("Map/D1008.map")).unwrap();
            repeated
                .place(&mut preview, at, None, Run::default())
                .unwrap();
            for x in 0..i32::from(source.width) {
                for y in 0..i32::from(source.height) {
                    assert_eq!(
                        source.is_walkable(x, y),
                        preview.map.is_walkable(x, y),
                        "{} repeated collision at {x},{y}",
                        spec.name
                    );
                }
            }
            let triple = repeated.join(piece, 1, 0, &assets).unwrap();
            assert_eq!(triple.size(), (6, (spec.height + 4) as u16));
            assert_eq!(triple.ports.len(), 2);
            if let Some(dir) = std::env::var_os("ZIRCON_WALL_BODIES_QA_DIR") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&dir).unwrap();
                piece
                    .save(
                        &dir.join(format!("{}.json", spec.profile)),
                        &piece.name,
                        piece.kind,
                    )
                    .unwrap();
                repeated
                    .save(
                        &dir.join(format!("{}-pair.json", spec.profile)),
                        &repeated.name,
                        repeated.kind,
                    )
                    .unwrap();
                std::fs::write(
                    dir.join(format!("{}.map", spec.profile)),
                    preview.map.to_bytes(),
                )
                .unwrap();
            }
        }
    }
}
