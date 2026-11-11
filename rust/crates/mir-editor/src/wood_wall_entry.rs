//! Complete authored four-seam Wood junction around a physical floor passage.
use crate::{
    piece_ports::{Port, Side},
    tile_doc::Stamp,
    wall_piece::{Kind, Piece},
};
use mir_formats::map::MapFile;
use std::path::Path;

const BOUNDS: (i32, i32, i32, i32) = (92, 182, 112, 214);
const COLUMNS: [(i32, i32, i32); 10] = [
    (92, 182, 196),
    (94, 186, 198),
    (96, 188, 200),
    (98, 188, 202),
    (100, 188, 204),
    (102, 186, 206),
    (104, 184, 208),
    (106, 182, 210),
    (108, 196, 214),
    (110, 192, 214),
];
const WALLS: &[(i32, i32, u16)] = &[
    (92, 187, 399),
    (93, 188, 394),
    (94, 189, 458),
    (95, 190, 452),
    (96, 191, 360),
    (97, 192, 355),
    (98, 193, 350),
    (99, 193, 345),
    (100, 192, 340),
    (101, 191, 335),
    (102, 190, 538),
    (103, 189, 533),
    (104, 188, 550),
    (105, 187, 545),
    (106, 186, 538),
    (107, 185, 533),
    (105, 201, 387),
    (106, 201, 383),
    (107, 202, 378),
    (108, 202, 373),
    (109, 204, 367),
    (110, 201, 608),
    (111, 200, 607),
    (110, 205, 433),
    (111, 206, 428),
];

fn coverage() -> Vec<bool> {
    let mut mask = vec![false; 20 * 32];
    for (x, top, bottom) in COLUMNS {
        for xx in x..x + 2 {
            for y in top..bottom {
                mask[((y - 182) * 20 + xx - 92) as usize] = true;
            }
        }
    }
    mask
}
/// Ports, in order: west NE wall, recessed east NW wall, east SE wall,
/// east NE wall. All four walls must be continued; this is an authored junction,
/// not a self-contained opening that can replace one straight wall segment.
pub fn entry(assets: &Path) -> anyhow::Result<Piece> {
    let source = MapFile::load(assets.join("Map/D1008.map"))?;
    for &(x, y, image) in WALLS {
        let c = source
            .cell(x, y)
            .ok_or_else(|| anyhow::anyhow!("Wood wall junction source is too small."))?;
        anyhow::ensure!(
            c.middle_file == 21 && c.middle_image == image,
            "D1008 does not match the checked Wood wall junction."
        );
    }
    let mask = coverage();
    crate::cave_art::validate_covered_selection(&source, assets, BOUNDS, &mask)?;
    let mut piece = Piece::capture_covered(
        &Stamp::capture(&source, (92, 182), (111, 213)),
        "Wood physical wall junction".into(),
        Kind::Doorway,
        assets,
        mask,
    )?;
    piece.ports = [
        (Side::West, 0, 10, "wood-wall-ne"),
        (Side::East, 4, 8, "wood-wall-nw"),
        (Side::East, 0, 10, "wood-wall-se"),
        (Side::East, 0, 30, "wood-wall-ne"),
    ]
    .into_iter()
    .map(|(side, inset, offset, profile)| Port {
        side,
        inset,
        offset,
        width: 2,
        profile: profile.into(),
    })
    .collect();
    piece.serialized(&piece.name, piece.kind)?;
    Ok(piece)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        piece_composition::{self, Placement, Seam},
        tile_doc::Document,
        wall_piece::Run,
    };
    #[test]
    fn authored_passage_and_all_four_wall_seams_preserve_source_collision() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let junction = entry(&assets).unwrap();
        let bodies = crate::wood_wall_bodies::bodies(&assets).unwrap();
        let source = MapFile::load(assets.join("Map/D1008.map")).unwrap();
        let mut exact = Document::open(assets.join("Map/D1008.map")).unwrap();
        let before = exact.map.to_bytes();
        junction
            .place(&mut exact, (92, 182), None, Run::default())
            .unwrap();
        assert_eq!(exact.map.to_bytes(), before);
        let mut placements = vec![Placement {
            piece: &junction,
            at: (92, 182),
        }];
        let mut seams = vec![];
        for (port, body, body_port, at) in [
            (0, 1, 1, (90, 182)),
            (1, 0, 0, (108, 180)),
            (2, 3, 0, (112, 190)),
            (3, 1, 0, (112, 204)),
        ] {
            placements.push(Placement {
                piece: &bodies[body],
                at,
            });
            seams.push(Seam {
                a: 0,
                a_port: port,
                b: placements.len() - 1,
                b_port: body_port,
            });
        }
        let composed = piece_composition::compose(&placements, &seams, 128, 224, &assets).unwrap();
        for p in &placements {
            for &(x, y, _) in p.piece.cells() {
                let (x, y) = (p.at.0 + x, p.at.1 + y);
                assert_eq!(
                    source.is_walkable(x, y),
                    composed.map.is_walkable(x, y),
                    "junction seam collision at {x},{y}"
                );
            }
        }
        for p in placements.iter().skip(1) {
            p.piece
                .place(&mut exact, p.at, None, Run::default())
                .unwrap();
        }
        if let Some(out) = std::env::var_os("ZIRCON_WALL_ENTRY_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            junction
                .save(&out.join("junction.json"), &junction.name, junction.kind)
                .unwrap();
            std::fs::write(out.join("Junction.map"), exact.map.to_bytes()).unwrap();
            std::fs::write(out.join("Isolated.map"), composed.map.to_bytes()).unwrap();
        }
    }
}
