//! Authored northeast continuation; the bend and terminal body are not repeatable.
use crate::{
    cave_art,
    piece_ports::{self, Port, Side},
    tile_doc::Stamp,
    wall_piece::{Kind, Piece},
};
use mir_formats::map::MapFile;
use std::path::Path;

const WALLS: [(i32, i32, u16); 8] = [
    (20, 7, 360),
    (21, 8, 355),
    (22, 9, 350),
    (23, 9, 345),
    (24, 8, 340),
    (25, 7, 335),
    (26, 7, 445),
    (27, 8, 440),
];

fn check_source(source: &MapFile) -> anyhow::Result<()> {
    for (x, y, image) in WALLS {
        let cell = source
            .cell(x, y)
            .ok_or_else(|| anyhow::anyhow!("Wood NE source is too small."))?;
        anyhow::ensure!(
            cell.middle_file == 21 && cell.middle_image == image,
            "D1006 does not match the checked Wood NE artwork."
        );
    }
    Ok(())
}

fn capture(
    source: &MapFile,
    assets: &Path,
    name: &str,
    kind: Kind,
    bounds: (i32, i32, i32, i32),
    ports: &[(Side, u16, &str)],
) -> anyhow::Result<Piece> {
    cave_art::validate_selection(source, assets, bounds)?;
    let (l, t, r, b) = bounds;
    let stamp = Stamp::capture(source, (l, t), (r - 1, b - 1));
    let mut local = MapFile::blank((r - l) as u16, (b - t) as u16);
    for &(x, y, cell) in &stamp.cells {
        *local.cell_mut(x, y).unwrap() = cell;
    }
    let ports = ports
        .iter()
        .map(|&(side, offset, profile)| Port {
            side,
            offset,
            width: 2,
            inset: 0,
            profile: profile.into(),
        })
        .collect::<Vec<_>>();
    piece_ports::validate_boundary(&local, &ports, None)?;
    let mut piece = Piece::capture(&stamp, name.into(), kind, assets)?;
    piece.ports = ports;
    Ok(piece)
}

/// The source-adjacent approach needs two additional floor rows to reach the bend.
pub fn approach(assets: &Path) -> anyhow::Result<Piece> {
    let source = MapFile::load(assets.join("Map/D1006.map"))?;
    check_source(&source)?;
    capture(
        &source,
        assets,
        "Wood NE approach",
        Kind::Wall,
        (20, 4, 22, 16),
        &[(Side::West, 8, "wood-ne"), (Side::East, 10, "wood-ne-bend")],
    )
}

/// Preserve the irregular source bend and stop before neighboring shrine art.
pub fn extension(assets: &Path) -> anyhow::Result<Vec<Piece>> {
    let source = MapFile::load(assets.join("Map/D1006.map"))?;
    check_source(&source)?;
    Ok(vec![
        capture(
            &source,
            assets,
            "Wood NE bend",
            Kind::Corner,
            (22, 4, 26, 16),
            &[
                (Side::West, 10, "wood-ne-bend"),
                (Side::East, 8, "wood-ne-body"),
            ],
        )?,
        capture(
            &source,
            assets,
            "Wood NE terminal body",
            Kind::Wall,
            (26, 4, 28, 14),
            &[(Side::West, 8, "wood-ne-body")],
        )?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tile_doc::Document, wall_piece::Run};

    #[test]
    fn changed_ne_art_fingerprints_are_rejected() {
        let mut source = MapFile::blank(28, 20);
        for (x, y, image) in WALLS {
            let cell = source.cell_mut(x, y).unwrap();
            cell.middle_file = 21;
            cell.middle_image = image;
        }
        check_source(&source).unwrap();
        source.cell_mut(23, 9).unwrap().middle_image += 1;
        assert!(check_source(&source).is_err());
        assert!(check_source(&MapFile::blank(2, 2)).is_err());
    }

    #[test]
    fn checked_ne_continuation_preserves_the_original_source() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let start = approach(&assets).unwrap();
        let extension = extension(&assets).unwrap();
        let first = start.join(&extension[0], 1, 0, &assets).unwrap();
        let full = first.join(&extension[1], 1, 0, &assets).unwrap();
        assert_eq!(full.size(), (8, 12));
        assert_eq!(full.ports.len(), 1);
        let mut doc = Document::open(assets.join("Map/D1006.map")).unwrap();
        let before = doc.map.to_bytes();
        full.place(&mut doc, (20, 4), None, Run::default()).unwrap();
        assert_eq!(doc.map.to_bytes(), before);
        // The next two-row floor collar cuts shrine artwork anchored outside it.
        assert!(cave_art::validate_selection(&doc.map, &assets, (26, 4, 28, 16)).is_err());
        // Distinct profiles prohibit treating the irregular bend as a repeatable wall.
        assert!(extension[0].join(&extension[0], 1, 0, &assets).is_err());

        let kit = crate::wall_kit::wood_corner(&assets).unwrap();
        let northwest = kit.iter().find(|p| p.name == "Wood NW approach").unwrap();
        let corner = kit.iter().find(|p| p.name == "Wood north corner").unwrap();
        let east_port = |piece: &Piece, profile: &str| {
            piece
                .ports
                .iter()
                .position(|p| p.side == Side::East && p.profile == profile)
                .unwrap()
        };
        let mut north = northwest
            .join(corner, east_port(northwest, "wood-nw"), 0, &assets)
            .unwrap();
        north = north
            .join(&start, east_port(&north, "wood-ne"), 0, &assets)
            .unwrap();
        for (next, profile) in extension.iter().zip(["wood-ne-bend", "wood-ne-body"]) {
            north = north
                .join(next, east_port(&north, profile), 0, &assets)
                .unwrap();
        }
        north
            .place(&mut doc, (14, 2), None, Run::default())
            .unwrap();
        assert_eq!(doc.map.to_bytes(), before);

        if let Some(out) = std::env::var_os("ZIRCON_NE_KIT_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            for (i, piece) in std::iter::once(&start).chain(extension.iter()).enumerate() {
                piece
                    .save(
                        &out.join(format!("piece-{i}.json")),
                        &piece.name,
                        piece.kind,
                    )
                    .unwrap();
            }
            full.save(
                &out.join("assembled.json"),
                "Wood NE continuation",
                Kind::Corner,
            )
            .unwrap();
            std::fs::write(out.join("Northeast.map"), doc.map.to_bytes()).unwrap();
        }
    }
}
