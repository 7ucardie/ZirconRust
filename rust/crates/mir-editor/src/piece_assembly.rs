//! Conservative whole-kit assembly: consume every source once without guessing.
use crate::{piece_candidates, wall_piece::Piece};
use anyhow::Context;
use std::path::Path;

/// Start with the first source and require exactly one fully valid join at each
/// step. Ambiguity and disconnected sources fail without changing the catalogue.
/// At most 256 discovery joins across all rounds, plus 15 chosen-join revalidations.
/// Open connections may remain; successful assembly does not imply a perimeter.
pub fn assemble(pieces: &[Piece], assets: &Path) -> anyhow::Result<Piece> {
    anyhow::ensure!(
        !pieces.is_empty(),
        "Load at least one source piece to assemble."
    );
    anyhow::ensure!(
        pieces.len() <= piece_candidates::MAX_SOURCES,
        "Assemble at most {} source pieces at once.",
        piece_candidates::MAX_SOURCES
    );
    for (index, piece) in pieces.iter().enumerate() {
        piece
            .serialized(&piece.name, piece.kind)
            .with_context(|| format!("Source {} ({}) is invalid", index + 1, piece.name))?;
    }
    let mut current = pieces[0].clone();
    let mut remaining = pieces[1..].to_vec();
    let mut budget = piece_candidates::MAX_ATTEMPTS;
    while !remaining.is_empty() {
        let found =
            piece_candidates::enumerate_with_budget(&current, &remaining, assets, &mut budget)?;
        anyhow::ensure!(
            !found.is_empty(),
            "Assembly stalled with {} unused source pieces: no valid connection to the assembled piece.",
            remaining.len()
        );
        anyhow::ensure!(
            found.len() == 1,
            "Assembly is ambiguous: {} valid joins with {} unused source pieces. Build a chosen join manually or make connection profiles more specific.",
            found.len(),
            remaining.len()
        );
        let chosen = &found[0];
        current = current.join(
            &remaining[chosen.source_index],
            chosen.current_port,
            chosen.candidate_port,
            assets,
        )?;
        remaining.remove(chosen.source_index);
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        piece_ports::{Port, Side},
        tile_doc::{Document, Stamp},
        wall_piece::{Kind, Run},
    };
    use mir_formats::map::MapFile;

    fn port(side: Side, profile: &str) -> Port {
        Port {
            side,
            offset: 0,
            width: 2,
            inset: 0,
            profile: profile.into(),
        }
    }
    fn piece(name: &str, ports: Vec<Port>) -> Piece {
        let mut map = MapFile::blank(4, 4);
        for x in 0..4 {
            for y in 0..4 {
                let cell = map.cell_mut(x, y).unwrap();
                cell.back_file = 255;
                cell.flag = 3;
            }
        }
        let mut result = Piece::capture(
            &Stamp::capture(&map, (0, 0), (3, 3)),
            name.into(),
            Kind::Wall,
            Path::new("."),
        )
        .unwrap();
        result.ports = ports;
        result
    }

    #[test]
    fn unique_chain_consumes_each_source_once_without_mutating_inputs() {
        let pieces = vec![
            piece("Start", vec![port(Side::East, "first")]),
            piece("End", vec![port(Side::West, "second")]),
            piece(
                "Middle",
                vec![port(Side::West, "first"), port(Side::East, "second")],
            ),
        ];
        let before: Vec<_> = pieces
            .iter()
            .map(|p| p.serialized(&p.name, p.kind).unwrap())
            .collect();
        let joined = assemble(&pieces, Path::new(".")).unwrap();
        assert_eq!(joined.size(), (12, 4));
        assert!(joined.ports.is_empty());
        assert_eq!(joined.serialized(&joined.name, joined.kind).unwrap(), {
            let repeated = assemble(&pieces, Path::new(".")).unwrap();
            repeated.serialized(&repeated.name, repeated.kind).unwrap()
        });
        for (piece, original) in pieces.iter().zip(before) {
            assert_eq!(piece.serialized(&piece.name, piece.kind).unwrap(), original);
        }
    }

    #[test]
    fn ambiguous_and_disconnected_catalogues_fail_instead_of_returning_partial_pieces() {
        let start = piece("Start", vec![port(Side::East, "wall")]);
        let end = piece("End", vec![port(Side::West, "wall")]);
        let error = assemble(&[start.clone(), end.clone(), end.clone()], Path::new("."))
            .err()
            .unwrap();
        assert!(error.to_string().contains("ambiguous: 2 valid joins"));
        let error = assemble(&[start, end, piece("Detached", vec![])], Path::new("."))
            .err()
            .unwrap();
        assert!(error.to_string().contains("stalled with 1 unused"));
    }

    #[test]
    fn single_piece_is_validated_and_catalogue_size_is_bounded() {
        let valid = piece("Single", vec![]);
        assert_eq!(
            assemble(std::slice::from_ref(&valid), Path::new("."))
                .unwrap()
                .name,
            "Single"
        );
        let mut invalid = valid.clone();
        invalid.name.clear();
        assert!(assemble(&[invalid], Path::new(".")).is_err());
        let mut invalid = valid.clone();
        invalid.ports = vec![port(Side::East, "wall")];
        invalid.ports[0].offset = 4;
        assert!(assemble(&[invalid], Path::new(".")).is_err());
        assert!(assemble(&[], Path::new(".")).is_err());
        assert!(assemble(&vec![valid; 17], Path::new(".")).is_err());
    }

    #[test]
    fn maximum_length_unique_chain_stays_within_size_and_name_limits() {
        let pieces: Vec<_> = (0..16)
            .map(|index| {
                let mut ports = vec![];
                if index > 0 {
                    ports.push(port(Side::West, &format!("seam-{index}")));
                }
                if index < 15 {
                    ports.push(port(Side::East, &format!("seam-{}", index + 1)));
                }
                piece(&"X".repeat(80), ports)
            })
            .collect();
        let full = assemble(&pieces, Path::new(".")).unwrap();
        assert_eq!(full.size(), (64, 4));
        assert!(full.ports.is_empty());
        full.serialized(&full.name, full.kind).unwrap();
    }

    #[test]
    fn built_in_kits_assemble_with_exact_source_art_and_collision() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        for (name, pieces, origin, size) in [
            (
                "north",
                crate::wall_kit::wood_corner(&assets).unwrap(),
                (14, 2),
                (14, 14),
            ),
            (
                "south",
                crate::wood_corners::south_corner(&assets).unwrap(),
                (20, 26),
                (14, 24),
            ),
        ] {
            let full = assemble(&pieces, &assets).unwrap();
            assert_eq!(full.size(), size);
            assert!(full.ports.is_empty());
            let mut doc = Document::open(assets.join("Map/D1006.map")).unwrap();
            let before = doc.map.to_bytes();
            full.place(&mut doc, origin, None, Run::default()).unwrap();
            assert_eq!(doc.map.to_bytes(), before);
            if let Some(out) = std::env::var_os("ZIRCON_ASSEMBLY_QA_DIR") {
                let out = std::path::PathBuf::from(out);
                std::fs::create_dir_all(&out).unwrap();
                full.save(
                    &out.join(format!("{name}.json")),
                    &format!("Assembled Wood {name} kit"),
                    Kind::Formation,
                )
                .unwrap();
                std::fs::write(out.join(format!("{name}.map")), before).unwrap();
                let source = doc.map;
                doc.map = MapFile::blank(source.width, source.height);
                full.place(&mut doc, origin, None, Run::default()).unwrap();
                std::fs::write(
                    out.join(format!("{name}-selection.map")),
                    doc.map.to_bytes(),
                )
                .unwrap();
            }
        }
    }
}
