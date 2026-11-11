//! Complete source concave turns for authored Wood room/corridor outlines.
use crate::{
    cave_art,
    piece_ports::{Port, Side},
    tile_doc::Stamp,
    wall_piece::{Kind, Piece},
};
use mir_formats::map::MapFile;
use std::path::Path;

type CornerSpec = ((i32, i32, i32, i32), u16, [&'static str; 2]);
const SPECS: [CornerSpec; 2] = [
    ((128, 84, 134, 94), 8, ["wood-wall-ne", "wood-wall-nw"]),
    ((36, 378, 42, 388), 2, ["wood-wall-se", "wood-wall-sw"]),
];
const FINGERPRINTS: [[(i32, i32, u16); 6]; 2] = [
    [
        (128, 87, 360),
        (129, 88, 355),
        (130, 89, 350),
        (131, 89, 345),
        (132, 88, 340),
        (133, 87, 335),
    ],
    [
        (36, 387, 302),
        (37, 385, 301),
        (38, 384, 300),
        (39, 384, 299),
        (40, 385, 298),
        (41, 387, 297),
    ],
];
fn check_source(source: &MapFile, index: usize) -> anyhow::Result<()> {
    for (x, y, image) in FINGERPRINTS[index] {
        let cell = source
            .cell(x, y)
            .ok_or_else(|| anyhow::anyhow!("Wood concave source is too small."))?;
        anyhow::ensure!(
            cell.middle_file == 21 && cell.middle_image == image,
            "D1008 does not match the checked concave Wood corner artwork."
        );
    }
    Ok(())
}
/// Returns [southward valley, northward notch]. Do not rotate these sprites.
pub fn corners(assets: &Path) -> anyhow::Result<Vec<Piece>> {
    let source = MapFile::load(assets.join("Map/D1008.map"))?;
    SPECS
        .iter()
        .enumerate()
        .map(|(index, &(bounds, offset, profiles))| {
            check_source(&source, index)?;
            cave_art::validate_selection(&source, assets, bounds)?;
            let (l, t, r, b) = bounds;
            let name = ["Wood concave valley", "Wood concave notch"][index];
            let mut piece = Piece::capture(
                &Stamp::capture(&source, (l, t), (r - 1, b - 1)),
                name.into(),
                Kind::Corner,
                assets,
            )?;
            piece.ports = [Side::West, Side::East]
                .into_iter()
                .zip(profiles)
                .map(|(side, profile)| Port {
                    side,
                    offset,
                    inset: 0,
                    width: 2,
                    profile: profile.into(),
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
    fn changed_concave_motifs_are_rejected() {
        let mut map = MapFile::blank(400, 400);
        for fingerprints in FINGERPRINTS {
            for (x, y, image) in fingerprints {
                let c = map.cell_mut(x, y).unwrap();
                c.middle_file = 21;
                c.middle_image = image;
            }
        }
        for index in 0..2 {
            check_source(&map, index).unwrap();
        }
        map.cell_mut(130, 89).unwrap().middle_image = 0;
        assert!(check_source(&map, 0).is_err());
    }
    #[test]
    fn source_corners_and_both_adjoining_bodies_preserve_collision_and_art() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let corners = corners(&assets).unwrap();
        let bodies = crate::wood_wall_bodies::bodies(&assets).unwrap();
        for (index, (left, right, origin)) in [(1, 0, (126, 82)), (3, 2, (34, 378))]
            .into_iter()
            .enumerate()
        {
            let mut doc = Document::open(assets.join("Map/D1008.map")).unwrap();
            let before = doc.map.to_bytes();
            let b = SPECS[index].0;
            corners[index]
                .place(&mut doc, (b.0, b.1), None, Run::default())
                .unwrap();
            assert_eq!(doc.map.to_bytes(), before);
            assert_eq!(corners[index].size(), (6, 10));
            let joined = bodies[left].join(&corners[index], 1, 0, &assets).unwrap();
            let joined = joined.join(&bodies[right], 1, 0, &assets).unwrap();
            assert_eq!(joined.ports.len(), 2);
            let source = doc.map.clone();
            joined
                .place(&mut doc, origin, None, Run::default())
                .unwrap();
            for x in 0..400 {
                for y in 0..400 {
                    assert_eq!(
                        doc.map.is_walkable(x, y),
                        source.is_walkable(x, y),
                        "corner {index} at {x},{y}"
                    );
                }
            }
            for &(x, y, c) in corners[index].cells() {
                assert_eq!(doc.map.cell(b.0 + x, b.1 + y), Some(&c));
            }
            if let Some(out) = std::env::var_os("ZIRCON_CONCAVE_QA_DIR") {
                let out = std::path::PathBuf::from(out);
                std::fs::create_dir_all(&out).unwrap();
                corners[index]
                    .save(
                        &out.join(format!("corner-{index}.json")),
                        &corners[index].name,
                        Kind::Corner,
                    )
                    .unwrap();
                std::fs::write(out.join(format!("corner-{index}.map")), doc.map.to_bytes())
                    .unwrap();
            }
        }
    }
}
