//! Checked, static Wood doorway thresholds in connected room maps.
use mir_formats::map::MapFile;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Threshold {
    pub center: [i32; 2],
    /// Half-open bounds of the six-by-four walkable crossing.
    pub bounds: [i32; 4],
    pub passable: bool,
    pub interactive: bool,
}

/// A paired source-art valley and notch frame this walkable crossing.
/// It is a static map opening; the C# game has no ordinary room-door object.
pub fn inspect(map: &MapFile, center: (i32, i32)) -> anyhow::Result<Threshold> {
    let (x, y) = center;
    let bounds = [x - 3, y - 2, x + 3, y + 2];
    anyhow::ensure!(
        (bounds[0]..bounds[2]).all(|px| (bounds[1]..bounds[3]).all(|py| map.is_walkable(px, py))),
        "Wood doorway threshold must remain six by four walkable cells."
    );
    anyhow::ensure!(
        (y - 3..y + 5).all(|py| map.is_walkable(x, py)),
        "Wood doorway needs its eight-cell crossing."
    );
    for (dx, dy, image) in [(-3, -9, 360), (-3, 11, 302)] {
        let cell = map.cell(x + dx, y + dy);
        anyhow::ensure!(
            cell.is_some_and(|cell| cell.middle_file == 21 && cell.middle_image == image),
            "Wood doorway is missing its source-art wall frame."
        );
    }
    Ok(Threshold {
        center: [x, y],
        bounds,
        passable: true,
        interactive: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_threshold_keeps_source_art_and_collision() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        for repeats in [&[[3, 3], [3, 3]][..], &[[3, 3], [4, 3], [3, 3]][..]] {
            let (doc, _, necks) =
                crate::wood_connected::generate_varied_with_necks(&assets, repeats, 42).unwrap();
            assert_eq!(necks.len(), repeats.len() - 1);
            for center in necks {
                let threshold = inspect(&doc.map, center).unwrap();
                assert_eq!(
                    threshold.bounds,
                    [center.0 - 3, center.1 - 2, center.0 + 3, center.1 + 2]
                );
                let mut blocked = doc.map.clone();
                blocked.cell_mut(center.0, center.1).unwrap().flag = 0;
                assert!(inspect(&blocked, center).is_err());
                let mut missing_art = doc.map.clone();
                missing_art
                    .cell_mut(center.0 - 3, center.1 - 9)
                    .unwrap()
                    .middle_image = 0;
                assert!(inspect(&missing_art, center).is_err());
            }
        }
    }
}
