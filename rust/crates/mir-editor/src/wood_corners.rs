//! Additional authored Wood wall orientations, captured without rearranging their art.
use crate::{
    piece_ports::{self, Port, Side},
    tile_doc::Stamp,
    wall_piece::{Kind, Piece},
};
use mir_formats::map::MapFile;
use std::path::Path;

/// Capture the actual south convex bend and its adjoining diagonal wall sections.
/// The deep floor collar retains the complete decoration above the southern wall.
pub fn south_corner(assets: &Path) -> anyhow::Result<Vec<Piece>> {
    let source = MapFile::load(assets.join("Map/D1006.map"))?;
    for (x, y, image) in [
        (20, 43, 612),
        (21, 45, 611),
        (22, 45, 282),
        (23, 47, 281),
        (24, 48, 280),
        (25, 48, 279),
        (26, 47, 278),
        (27, 45, 277),
        (28, 45, 628),
        (29, 43, 627),
    ] {
        let cell = source
            .cell(x, y)
            .ok_or_else(|| anyhow::anyhow!("Wood south corner source is too small."))?;
        anyhow::ensure!(
            cell.middle_file == 21 && cell.middle_image == image,
            "D1006 does not match the checked Wood south corner artwork."
        );
    }
    let port = |side, offset, profile: &str| Port {
        side,
        offset,
        width: 2,
        inset: 0,
        profile: profile.into(),
    };
    let specs = [
        (
            "Wood SW approach",
            Kind::Wall,
            (20, 34, 22, 46),
            vec![port(Side::East, 2, "wood-sw")],
        ),
        (
            "Wood south corner",
            Kind::Corner,
            (22, 28, 28, 50),
            vec![
                port(Side::West, 8, "wood-sw"),
                port(Side::East, 8, "wood-se"),
            ],
        ),
        (
            "Wood SE decorated section",
            Kind::Formation,
            (28, 26, 34, 46),
            vec![port(Side::West, 10, "wood-se")],
        ),
    ];
    let mut pieces = Vec::new();
    for (name, kind, (left, top, right, bottom), ports) in specs {
        crate::cave_art::validate_selection(&source, assets, (left, top, right, bottom))?;
        let mut piece = Piece::capture(
            &Stamp::capture(&source, (left, top), (right - 1, bottom - 1)),
            name.into(),
            kind,
            assets,
        )?;
        let mut map = MapFile::blank((right - left) as u16, (bottom - top) as u16);
        for x in left..right {
            for y in top..bottom {
                *map.cell_mut(x - left, y - top).unwrap() = *source.cell(x, y).unwrap();
            }
        }
        piece_ports::validate_boundary(&map, &ports, None)?;
        piece.ports = ports;
        pieces.push(piece);
    }
    // The circular floor drawing includes (27,27). Its 2x2 cap preserves that
    // column without capturing the unrelated tall statue at (24,26).
    let cap_bounds = (26, 26, 28, 28);
    crate::cave_art::validate_selection(&source, assets, cap_bounds)?;
    let mut cap = Piece::capture(
        &Stamp::capture(&source, (26, 26), (27, 27)),
        "South decoration cap".into(),
        Kind::Formation,
        assets,
    )?;
    cap.ports = vec![port(Side::South, 0, "wood-south-decoration")];
    pieces[1]
        .ports
        .push(port(Side::North, 4, "wood-south-decoration"));
    let mut corner = pieces[1].join(&cap, 2, 0, assets)?;
    corner.name = "Wood south decorated corner".into();
    corner.kind = Kind::Corner;
    pieces[1] = corner;
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn south_bend_reassembles_complete_source_art_and_collision() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let pieces = south_corner(&assets).unwrap();
        let first = pieces[0].join(&pieces[1], 0, 0, &assets).unwrap();
        let full = first.join(&pieces[2], 0, 0, &assets).unwrap();
        assert_eq!(full.size(), (14, 24));
        assert!(full.ports.is_empty());
        let mut doc = crate::tile_doc::Document::open(assets.join("Map/D1006.map")).unwrap();
        let before = doc.map.to_bytes();
        full.place(&mut doc, (20, 26), None, crate::wall_piece::Run::default())
            .unwrap();
        assert_eq!(doc.map.to_bytes(), before);
        let source = doc.map.clone();
        doc.map = MapFile::blank(source.width, source.height);
        full.place(&mut doc, (20, 26), None, crate::wall_piece::Run::default())
            .unwrap();
        // The semantic drawing spans many individually bounded sprites. Every
        // southern circular-floor cell and every flame column must be retained.
        let mut floor_cells = 0;
        for x in 22..34 {
            for y in 26..39 {
                let cell = source.cell(x, y).unwrap();
                if cell.middle_file == 21 && (4615..=4686).contains(&cell.middle_image) {
                    assert_eq!(doc.map.cell(x, y), Some(cell));
                    floor_cells += 1;
                }
            }
        }
        assert_eq!(floor_cells, 72);
        for x in 24..33 {
            assert_eq!(doc.map.cell(x, 38), source.cell(x, 38));
        }
        if let Some(out) = std::env::var_os("ZIRCON_SOUTH_CORNER_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            for (i, piece) in pieces.iter().enumerate() {
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
                "Assembled Wood south corner",
                Kind::Corner,
            )
            .unwrap();
            std::fs::write(out.join("SouthCorner.map"), source.to_bytes()).unwrap();
            std::fs::write(out.join("SouthSelection.map"), doc.map.to_bytes()).unwrap();
        }
    }

    #[test]
    fn cropped_south_decoration_is_rejected() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = MapFile::load(assets.join("Map/D1006.map")).unwrap();
        assert!(crate::cave_art::validate_selection(&source, &assets, (22, 36, 28, 50)).is_err());
        // A later animation frame reaches above the static frame at row 28.
        assert!(crate::cave_art::validate_selection(&source, &assets, (28, 28, 30, 46)).is_err());
    }
}
