//! A source-checked palace threshold, retaining its complete neighboring art.
use crate::{
    cave_art,
    piece_ports::{self, Port, Side},
    tile_doc::Stamp,
    wall_piece::{Kind, Piece},
};
use mir_formats::map::MapFile;
use std::path::Path;

const BOUNDS: (i32, i32, i32, i32) = (24, 4, 36, 52);
const WALLS: [(i32, i32, u16); 4] = [
    (26, 41, 1802),
    (27, 42, 1801),
    (32, 47, 1832),
    (33, 48, 1831),
];

fn check_source(source: &MapFile) -> anyhow::Result<()> {
    anyhow::ensure!(
        (source.width, source.height) == (70, 70),
        "Palace doorway requires the checked 70x70 source map 0_000."
    );
    for (x, y, image) in WALLS {
        let cell = source.cell(x, y).unwrap();
        anyhow::ensure!(
            cell.front_file == 22 && cell.front_image == image,
            "0_000 does not match the checked palace doorway artwork."
        );
    }
    Ok(())
}

/// Capture both wall endcaps and the physical carpeted opening between them.
/// The tall selection preserves additional palace art above the opening; this
/// is a palace piece, not a compatible Wood temple wall kit component.
pub fn doorway(assets: &Path) -> anyhow::Result<Piece> {
    let source = MapFile::load(assets.join("Map/0_000.map"))?;
    check_source(&source)?;
    cave_art::validate_selection(&source, assets, BOUNDS)?;
    let (left, top, right, bottom) = BOUNDS;
    let stamp = Stamp::capture(&source, (left, top), (right - 1, bottom - 1));
    let mut local = MapFile::blank((right - left) as u16, (bottom - top) as u16);
    for &(x, y, cell) in &stamp.cells {
        *local.cell_mut(x, y).unwrap() = cell;
    }
    let ports = [(Side::West, 40), (Side::East, 34)]
        .into_iter()
        .map(|(side, offset)| Port {
            side,
            inset: 0,
            offset,
            width: 2,
            profile: "palace-doorway".into(),
        })
        .collect::<Vec<_>>();
    piece_ports::validate_boundary(&local, &ports, None)?;
    let mut piece = Piece::capture(&stamp, "Palace doorway".into(), Kind::Doorway, assets)?;
    piece.ports = ports;
    Ok(piece)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tile_doc::Document, wall_piece::Run};

    #[test]
    fn changed_source_dimensions_or_wall_fingerprints_are_rejected() {
        assert!(check_source(&MapFile::blank(68, 70)).is_err());
        let mut map = MapFile::blank(70, 70);
        for (x, y, image) in WALLS {
            let cell = map.cell_mut(x, y).unwrap();
            cell.front_file = 22;
            cell.front_image = image;
        }
        check_source(&map).unwrap();
        map.cell_mut(32, 47).unwrap().front_image += 1;
        assert!(check_source(&map).is_err());
    }

    #[test]
    fn checked_palace_doorway_preserves_source_and_roundtrips() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let piece = doorway(&assets).unwrap();
        assert_eq!(piece.size(), (12, 48));
        assert_eq!(piece.ports.len(), 2);
        let mut doc = Document::open(assets.join("Map/0_000.map")).unwrap();
        let before = doc.map.to_bytes();
        piece
            .place(&mut doc, (24, 4), None, Run::default())
            .unwrap();
        assert_eq!(doc.map.to_bytes(), before);

        let path = std::env::temp_dir().join(format!(
            "zircon-palace-doorway-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        piece.save(&path, &piece.name, piece.kind).unwrap();
        let loaded = Piece::load(&path, &assets).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(loaded.ports, piece.ports);
        loaded
            .place(&mut doc, (24, 4), None, Run::default())
            .unwrap();
        assert_eq!(doc.map.to_bytes(), before);

        if let Some(out) = std::env::var_os("ZIRCON_PALACE_KIT_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            loaded
                .save(&out.join("doorway.json"), &loaded.name, loaded.kind)
                .unwrap();
            std::fs::write(out.join("Palace.map"), doc.map.to_bytes()).unwrap();
        }
    }
}
