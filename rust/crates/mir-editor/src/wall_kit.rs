//! Source-checked starting pieces for the Wood temple wall kit.
use crate::{
    piece_ports::{self, Port, Side},
    tile_doc::Stamp,
    wall_piece::{Kind, Piece},
};
use mir_formats::map::MapFile;
use std::path::Path;

/// Capture the actual north convex wall bend, plus both source-adjacent approaches.
/// Image fingerprints and complete selection checks reject incompatible source packs.
pub fn wood_corner(assets: &Path) -> anyhow::Result<Vec<Piece>> {
    let source = MapFile::load(assets.join("Map/D1006.map"))?;
    for (x, y, index) in [(16, 6, 538), (17, 5, 533), (18, 5, 421), (19, 6, 416)] {
        let c = source
            .cell(x, y)
            .ok_or_else(|| anyhow::anyhow!("Wood corner source is too small."))?;
        anyhow::ensure!(
            c.middle_file == 21 && c.middle_image == index,
            "D1006 does not match the checked Wood corner artwork."
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
            "Wood NW approach",
            Kind::Wall,
            (14, 2, 16, 14),
            vec![port(Side::East, 10, "wood-nw")],
        ),
        (
            "Wood north corner",
            Kind::Corner,
            (16, 2, 20, 14),
            vec![
                port(Side::West, 10, "wood-nw"),
                port(Side::East, 10, "wood-ne"),
            ],
        ),
    ];
    let mut pieces = vec![];
    for (name, kind, (l, t, r, b), ports) in specs {
        crate::cave_art::validate_selection(&source, assets, (l, t, r, b))?;
        let mut p = Piece::capture(
            &Stamp::capture(&source, (l, t), (r - 1, b - 1)),
            name.into(),
            kind,
            assets,
        )?;
        piece_ports::validate_boundary(&source_piece_map(&source, (l, t, r, b)), &ports, None)?;
        p.ports = ports;
        pieces.push(p);
    }
    pieces.push(crate::wood_ne::approach(assets)?);
    pieces.extend(crate::wood_ne::extension(assets)?);
    Ok(pieces)
}
fn source_piece_map(source: &MapFile, (l, t, r, b): (i32, i32, i32, i32)) -> MapFile {
    let mut map = MapFile::blank((r - l) as u16, (b - t) as u16);
    for x in l..r {
        for y in t..b {
            *map.cell_mut(x - l, y - t).unwrap() = *source.cell(x, y).unwrap();
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_corner_kit_assembles_at_original_source_positions() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let pieces = wood_corner(&assets).unwrap();
        let first = pieces[0].join(&pieces[1], 0, 0, &assets).unwrap();
        let mut full = first.join(&pieces[2], 0, 0, &assets).unwrap();
        for piece in &pieces[3..] {
            full = full.join(piece, 0, 0, &assets).unwrap();
        }
        assert_eq!(full.size(), (14, 14));
        assert!(full.ports.is_empty());
        let mut doc = crate::tile_doc::Document::open(assets.join("Map/D1006.map")).unwrap();
        let before = doc.map.to_bytes();
        full.place(&mut doc, (14, 2), None, crate::wall_piece::Run::default())
            .unwrap();
        assert_eq!(doc.map.to_bytes(), before);
        if let Some(out) = std::env::var_os("ZIRCON_CORNER_KIT_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            for (i, p) in pieces.iter().enumerate() {
                p.save(&out.join(format!("piece-{i}.json")), &p.name, p.kind)
                    .unwrap();
            }
            full.save(
                &out.join("assembled.json"),
                "Assembled Wood corner",
                Kind::Corner,
            )
            .unwrap();
            std::fs::write(out.join("Corner.map"), doc.map.to_bytes()).unwrap();
        }
    }
}
