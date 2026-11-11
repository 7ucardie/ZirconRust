//! Small, deterministic directories of reusable authored piece files.
use crate::{piece_candidates::MAX_SOURCES, wall_piece::Piece};
use anyhow::Context;
use std::{io::Write, path::Path};

const MAX_DIRECTORY_ENTRIES: usize = 128;

/// Load direct JSON children in filename order. Any invalid file rejects the
/// entire result, so callers can retain their previous catalogue on failure.
pub fn load(path: &Path, assets: &Path) -> anyhow::Result<Vec<Piece>> {
    let mut paths = Vec::new();
    for (index, entry) in std::fs::read_dir(path)?.enumerate() {
        anyhow::ensure!(
            index < MAX_DIRECTORY_ENTRIES,
            "Piece folders may contain at most {MAX_DIRECTORY_ENTRIES} entries."
        );
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|ext| ext == "json")
        {
            paths.push(entry.path());
            anyhow::ensure!(
                paths.len() <= MAX_SOURCES,
                "Load at most {MAX_SOURCES} source pieces at once."
            );
        }
    }
    anyhow::ensure!(
        !paths.is_empty(),
        "No piece JSON files found in this folder."
    );
    paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    paths
        .into_iter()
        .map(|file| {
            Piece::load(&file, assets)
                .with_context(|| format!("Could not load piece {}", file.display()))
        })
        .collect()
}

/// Export validated pieces into a newly created directory. Existing directories
/// and files are never overwritten. An I/O failure can leave a partial new
/// folder; report it explicitly rather than deleting potentially changed files.
pub fn export(pieces: &[Piece], path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        !pieces.is_empty() && pieces.len() <= MAX_SOURCES,
        "Export between 1 and {MAX_SOURCES} source pieces."
    );
    let serialized: Vec<_> = pieces
        .iter()
        .map(|piece| piece.serialized(&piece.name, piece.kind))
        .collect::<anyhow::Result<_>>()?;
    std::fs::create_dir(path)
        .with_context(|| format!("Choose a new piece folder: {}", path.display()))?;
    for (index, bytes) in serialized.iter().enumerate() {
        let file = path.join(format!("piece-{:02}.json", index + 1));
        let result = (|| -> anyhow::Result<()> {
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)?;
            output.write_all(bytes)?;
            output.sync_all()?;
            Ok(())
        })();
        result.with_context(|| {
            format!(
                "Export stopped at {}; the new folder may contain partial results",
                file.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        piece_ports::{Port, Side},
        tile_doc::Stamp,
        wall_piece::Kind,
    };
    use mir_formats::map::MapFile;
    use std::path::PathBuf;

    static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "zircon-piece-catalogue-{}-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn piece(name: &str) -> Piece {
        let mut map = MapFile::blank(4, 4);
        for x in 0..4 {
            for y in 0..4 {
                let cell = map.cell_mut(x, y).unwrap();
                cell.flag = 3;
                cell.back_file = 255;
                cell.middle_file = 255;
                cell.front_file = 255;
            }
        }
        Piece::capture(
            &Stamp::capture(&map, (0, 0), (3, 3)),
            name.into(),
            Kind::Corner,
            Path::new("unused-assets"),
        )
        .unwrap()
    }

    #[test]
    fn catalogue_roundtrip_uses_filename_order_and_never_overwrites() {
        let temp = Temp::new();
        let folder = temp.0.join("kit");
        export(&[piece("Z first"), piece("A second")], &folder).unwrap();
        let first = std::fs::read(folder.join("piece-01.json")).unwrap();
        // Directory traversal is deliberately nonrecursive; unrelated files
        // and nested JSON files cannot become candidate pieces.
        std::fs::create_dir(folder.join("nested")).unwrap();
        std::fs::write(folder.join("nested/broken.json"), "invalid").unwrap();
        std::fs::write(folder.join("notes.txt"), "notes").unwrap();
        let loaded = load(&folder, Path::new("unused-assets")).unwrap();
        assert_eq!(
            loaded.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["Z first", "A second"]
        );
        assert_eq!(loaded[0].kind, Kind::Corner);
        assert_eq!(loaded[0].size(), (4, 4));
        assert!(export(&[piece("Replacement")], &folder).is_err());
        assert_eq!(std::fs::read(folder.join("piece-01.json")).unwrap(), first);
        std::fs::write(folder.join("piece-03.json"), "invalid").unwrap();
        assert!(load(&folder, Path::new("unused-assets")).is_err());
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn invalid_export_is_rejected_before_creating_a_folder() {
        let temp = Temp::new();
        let destination = temp.0.join("invalid");
        let mut invalid = piece("Invalid connection");
        invalid.ports.push(Port {
            side: Side::North,
            offset: 3,
            width: 2,
            inset: 0,
            profile: "stone".into(),
        });
        assert!(export(&[piece("Valid"), invalid], &destination).is_err());
        assert!(!destination.exists());
        assert!(export(&[], &destination).is_err());
        let many: Vec<_> = (0..=MAX_SOURCES).map(|_| piece("Many")).collect();
        assert!(export(&many, &destination).is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn catalogue_limits_scan_and_piece_counts() {
        let temp = Temp::new();
        let files = temp.0.join("files");
        std::fs::create_dir(&files).unwrap();
        for index in 0..=MAX_DIRECTORY_ENTRIES {
            std::fs::write(files.join(format!("{index}.txt")), "").unwrap();
        }
        let error = load(&files, Path::new("unused-assets"))
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("128 entries"), "{error}");
        let pieces = temp.0.join("pieces");
        std::fs::create_dir(&pieces).unwrap();
        for index in 0..=MAX_SOURCES {
            std::fs::write(pieces.join(format!("{index}.json")), "invalid").unwrap();
        }
        let error = load(&pieces, Path::new("unused-assets"))
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("16 source pieces"), "{error}");
    }
}
