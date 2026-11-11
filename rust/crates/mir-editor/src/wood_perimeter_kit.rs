//! Four complete convex Wood corners with matching diagonal wall-body ports.
use crate::{
    piece_ports::{Port, Side},
    tile_doc::Stamp,
    wall_piece::{Kind, Piece},
};
use mir_formats::map::MapFile;
use std::path::Path;

struct Spec {
    name: &'static str,
    bounds: (i32, i32, i32, i32),
    fingerprints: &'static [(i32, i32, u16)],
    ports: [(Side, u16, &'static str); 2],
}
const SPECS: [Spec; 4] = [
    Spec {
        name: "Wood north perimeter corner",
        bounds: (168, 276, 178, 292),
        fingerprints: &[
            (168, 284, 550),
            (169, 283, 545),
            (170, 282, 274),
            (171, 281, 269),
            (172, 280, 264),
            (173, 280, 259),
            (174, 281, 254),
            (175, 282, 249),
            (176, 283, 458),
            (177, 284, 452),
        ],
        ports: [
            (Side::West, 14, "wood-wall-nw"),
            (Side::East, 14, "wood-wall-ne"),
        ],
    },
    Spec {
        name: "Wood east perimeter corner",
        bounds: (42, 90, 50, 110),
        fingerprints: &[
            (42, 93, 458),
            (43, 94, 452),
            (42, 109, 608),
            (43, 108, 607),
            (44, 95, 445),
            (45, 96, 440),
            (44, 107, 628),
            (45, 105, 627),
            (46, 105, 294),
            (47, 103, 293),
            (48, 102, 292),
            (49, 102, 291),
        ],
        ports: [
            (Side::West, 8, "wood-wall-ne"),
            (Side::West, 12, "wood-wall-se"),
        ],
    },
    Spec {
        name: "Wood south perimeter corner",
        bounds: (70, 316, 76, 328),
        fingerprints: &[
            (70, 323, 282),
            (71, 325, 281),
            (72, 326, 280),
            (73, 326, 279),
            (74, 325, 278),
            (75, 323, 277),
        ],
        ports: [
            (Side::West, 0, "wood-wall-sw"),
            (Side::East, 0, "wood-wall-se"),
        ],
    },
    Spec {
        name: "Wood west perimeter corner",
        bounds: (4, 276, 14, 300),
        fingerprints: &[
            (4, 290, 288),
            (5, 290, 287),
            (6, 291, 286),
            (7, 293, 285),
            (8, 284, 594),
            (9, 284, 589),
            (8, 293, 612),
            (9, 295, 611),
            (10, 282, 538),
            (11, 281, 533),
            (10, 295, 624),
            (11, 297, 623),
            (12, 280, 538),
            (13, 279, 533),
            (12, 297, 612),
            (13, 299, 611),
        ],
        ports: [
            (Side::East, 8, "wood-wall-nw"),
            (Side::East, 16, "wood-wall-sw"),
        ],
    },
];

/// North, east, south and west corners, retaining their complete authored caps.
pub fn corners(assets: &Path) -> anyhow::Result<Vec<Piece>> {
    let source = MapFile::load(assets.join("Map/D1008.map"))?;
    SPECS
        .iter()
        .map(|spec| {
            for &(x, y, image) in spec.fingerprints {
                let cell = source
                    .cell(x, y)
                    .ok_or_else(|| anyhow::anyhow!("Wood perimeter source is too small."))?;
                anyhow::ensure!(
                    cell.middle_file == 21 && cell.middle_image == image,
                    "D1008 does not match the checked Wood perimeter corner artwork."
                );
            }
            crate::cave_art::validate_selection(&source, assets, spec.bounds)?;
            let (l, t, r, b) = spec.bounds;
            let mut piece = Piece::capture(
                &Stamp::capture(&source, (l, t), (r - 1, b - 1)),
                spec.name.into(),
                Kind::Corner,
                assets,
            )?;
            piece.ports = spec
                .ports
                .iter()
                .map(|(side, offset, profile)| Port {
                    side: *side,
                    offset: *offset,
                    width: 2,
                    inset: 0,
                    profile: (*profile).into(),
                })
                .collect();
            piece.serialized(&piece.name, piece.kind)?;
            Ok(piece)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tile_doc::Document, wall_piece::Run};
    #[test]
    fn complete_corners_accept_both_bodies_at_authored_source_positions() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let corners = corners(&assets).unwrap();
        let bodies = crate::wood_wall_bodies::bodies(&assets).unwrap();
        for (spec, corner) in SPECS.iter().zip(&corners) {
            let mut original = Document::open(assets.join("Map/D1008.map")).unwrap();
            let before = original.map.to_bytes();
            corner
                .place(
                    &mut original,
                    (spec.bounds.0, spec.bounds.1),
                    None,
                    Run::default(),
                )
                .unwrap();
            assert_eq!(
                original.map.to_bytes(),
                before,
                "{} complete source capture",
                spec.name
            );
            let mut joined = corner.clone();
            let mut at = (spec.bounds.0, spec.bounds.1);
            for (_, _, profile) in spec.ports {
                let pi = joined
                    .ports
                    .iter()
                    .position(|p| p.profile == profile)
                    .unwrap();
                let body = bodies
                    .iter()
                    .find(|p| p.ports[0].profile == profile)
                    .unwrap();
                let bi = if joined.ports[pi].side == Side::West {
                    1
                } else {
                    0
                };
                let shift = crate::piece_ports::alignment(
                    joined.map(),
                    &joined.ports[pi],
                    body.map(),
                    &body.ports[bi],
                )
                .unwrap();
                at.0 += shift.0.min(0);
                at.1 += shift.1.min(0);
                joined = joined.join(body, pi, bi, &assets).unwrap();
            }
            assert_eq!(joined.ports.len(), 2);
            joined
                .place(&mut original, at, None, Run::default())
                .unwrap();
            let source = MapFile::load(assets.join("Map/D1008.map")).unwrap();
            for x in 0..i32::from(source.width) {
                for y in 0..i32::from(source.height) {
                    assert_eq!(
                        source.is_walkable(x, y),
                        original.map.is_walkable(x, y),
                        "{} body seam collision at {x},{y}",
                        spec.name
                    );
                }
            }
            if let Some(out) = std::env::var_os("ZIRCON_PERIMETER_KIT_QA_DIR") {
                let out = std::path::PathBuf::from(out);
                std::fs::create_dir_all(&out).unwrap();
                let index = SPECS.iter().position(|s| s.name == spec.name).unwrap();
                corner
                    .save(
                        &out.join(format!("corner-{index}.json")),
                        &corner.name,
                        corner.kind,
                    )
                    .unwrap();
                joined
                    .save(
                        &out.join(format!("joined-{index}.json")),
                        &joined.name,
                        joined.kind,
                    )
                    .unwrap();
                std::fs::write(
                    out.join(format!("corner-{index}.map")),
                    original.map.to_bytes(),
                )
                .unwrap();
            }
        }
    }
}
