//! A complete authored Wood room/corridor junction, not a compact door sprite.
use crate::{
    cave_art,
    piece_ports::{Port, Side},
    tile_doc::Stamp,
    wall_piece::{Kind, Piece},
};
use mir_formats::map::MapFile;
use std::path::Path;

const BOUNDS: (i32, i32, i32, i32) = (74, 24, 90, 62);
const WALLS: [(i32, i32, u16); 12] = [
    (74, 48, 612),
    (75, 50, 611),
    (76, 50, 624),
    (77, 52, 623),
    (78, 37, 373),
    (79, 39, 367),
    (80, 36, 294),
    (81, 34, 293),
    (78, 55, 328),
    (79, 54, 323),
    (88, 60, 628),
    (89, 58, 627),
];

fn coverage() -> Vec<bool> {
    // Complete 2x2 columns around the central corridor rectangle. The upper
    // and lower tails retain all wall frames without recursively capturing
    // unrelated sprites elsewhere in the rectangular envelope.
    let mut mask = vec![false; 16 * 38];
    for (column, first, last) in [
        (0, 5, 15),
        (1, 4, 17),
        (2, 3, 16),
        (3, 0, 15),
        (4, 5, 15),
        (5, 5, 15),
        (6, 5, 15),
        (7, 5, 19),
    ] {
        for row in first..last {
            for dy in 0..2 {
                for dx in 0..2 {
                    mask[(row * 2 + dy) * 16 + column * 2 + dx] = true;
                }
            }
        }
    }
    mask
}

fn check_source(source: &MapFile) -> anyhow::Result<()> {
    anyhow::ensure!(
        (source.width, source.height) == (100, 100),
        "Wood entry requires the checked 100x100 source D10032."
    );
    for (x, y, image) in WALLS {
        let cell = source.cell(x, y).unwrap();
        anyhow::ensure!(
            cell.middle_file == 21 && cell.middle_image == image,
            "D10032 does not match the checked Wood room/corridor entry artwork."
        );
    }
    Ok(())
}

/// Preserve the room exit, adjoining corridor and its real wall turns together.
/// Profiles deliberately do not claim a seam with the D1006 corner kits.
pub fn entry(assets: &Path) -> anyhow::Result<Piece> {
    let source = MapFile::load(assets.join("Map/D10032.map"))?;
    check_source(&source)?;
    let mask = coverage();
    cave_art::validate_covered_selection(&source, assets, BOUNDS, &mask)?;
    let (left, top, right, bottom) = BOUNDS;
    let stamp = Stamp::capture(&source, (left, top), (right - 1, bottom - 1));
    let mut piece = Piece::capture_covered(
        &stamp,
        "Wood room/corridor entry".into(),
        Kind::Doorway,
        assets,
        mask,
    )?;
    piece.ports = [
        (Side::West, 10, "wood-entry-room"),
        (Side::East, 29, "wood-entry-corridor"),
    ]
    .into_iter()
    .map(|(side, offset, profile)| Port {
        side,
        offset,
        inset: 0,
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
    use crate::{tile_doc::Document, wall_piece::Run};

    #[test]
    fn source_fingerprints_and_owned_shape_are_checked() {
        assert!(check_source(&MapFile::blank(98, 100)).is_err());
        let mut source = MapFile::blank(100, 100);
        for (x, y, image) in WALLS {
            let cell = source.cell_mut(x, y).unwrap();
            cell.middle_file = 21;
            cell.middle_image = image;
        }
        check_source(&source).unwrap();
        source.cell_mut(78, 37).unwrap().middle_image += 1;
        assert!(check_source(&source).is_err());
        let mask = coverage();
        assert_eq!(mask.iter().filter(|&&v| v).count(), 380);
        assert!(!mask[0]);
        assert!(mask[6]);
    }

    #[test]
    fn real_entry_keeps_full_source_group_and_roundtrips() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source_map = MapFile::load(assets.join("Map/D10032.map")).unwrap();
        let mut cropped = coverage();
        for y in 0..2 {
            for x in 6..8 {
                cropped[y * 16 + x] = false;
            }
        }
        assert!(
            cave_art::validate_covered_selection(&source_map, &assets, BOUNDS, &cropped).is_err()
        );
        let piece = entry(&assets).unwrap();
        assert_eq!(piece.size(), (16, 38));
        assert_eq!(piece.ports.len(), 2);
        let mut doc = Document::open(assets.join("Map/D10032.map")).unwrap();
        let before = doc.map.to_bytes();
        piece
            .place(&mut doc, (74, 24), None, Run::default())
            .unwrap();
        assert_eq!(doc.map.to_bytes(), before);
        let source = doc.map.clone();
        doc.map = MapFile::blank(100, 100);
        for x in 0..100 {
            for y in 0..100 {
                doc.map.cell_mut(x, y).unwrap().back_file = 255;
            }
        }
        piece
            .place(&mut doc, (74, 24), None, Run::default())
            .unwrap();
        let mask = coverage();
        assert_eq!(
            (0..100)
                .flat_map(|x| (0..100).map(move |y| (x, y)))
                .filter(|&(x, y)| doc.map.cell(x, y).unwrap().is_walkable())
                .count(),
            111
        );
        for y in 0..38 {
            for x in 0..16 {
                if mask[(y * 16 + x) as usize] {
                    assert_eq!(doc.map.cell(x + 74, y + 24), source.cell(x + 74, y + 24));
                }
            }
        }
        let out = std::env::temp_dir().join(format!(
            "zircon-wood-entry-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        piece.save(&out, &piece.name, piece.kind).unwrap();
        let loaded = Piece::load(&out, &assets).unwrap();
        std::fs::remove_file(out).unwrap();
        assert_eq!(
            loaded.serialized(&loaded.name, loaded.kind).unwrap(),
            piece.serialized(&piece.name, piece.kind).unwrap()
        );
        if let Some(out) = std::env::var_os("ZIRCON_WOOD_ENTRY_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            loaded
                .save(&out.join("entry.json"), &loaded.name, loaded.kind)
                .unwrap();
            std::fs::write(out.join("entry-selection.map"), doc.map.to_bytes()).unwrap();
            std::fs::write(out.join("entry-source.map"), before).unwrap();
        }
    }
}
