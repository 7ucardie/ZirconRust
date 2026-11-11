//! Private map sidecar for ordinary interactive Wood room doors.
use crate::map::MapFile;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomDoors {
    pub version: u8,
    pub doors: Vec<RoomDoor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomDoor {
    pub center: [i32; 2],
    /// Existing MonsterInfo row used for base stats; appearance and behavior are overridden.
    pub source_monster: i32,
}

impl RoomDoors {
    pub fn empty() -> Self {
        Self {
            version: 1,
            doors: vec![],
        }
    }

    pub fn load(map_path: &Path, map: &MapFile) -> anyhow::Result<Option<Self>> {
        let path = map_path.with_extension("doors.json");
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_file(),
                "Room door sidecar must be a regular file, not a symlink."
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let file = std::fs::File::open(&path)?;
        let mut bytes = Vec::new();
        use std::io::Read;
        file.take(16 * 1024 + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() <= 16 * 1024,
            "Room door sidecar exceeds 16 KiB."
        );
        let layout: Self = serde_json::from_slice(&bytes)?;
        layout.validate(map)?;
        Ok(Some(layout))
    }

    pub fn validate(&self, map: &MapFile) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 1 && self.doors.len() <= 8,
            "Room doors require version 1 and at most eight entries."
        );
        let mut occupied = std::collections::BTreeSet::new();
        for door in &self.doors {
            anyhow::ensure!(
                door.source_monster > 0,
                "Room door needs an existing monster source ID."
            );
            let [x, y] = door.center;
            anyhow::ensure!(
                x >= 3 && y >= 3 && x <= i32::from(map.width) - 4 && y <= i32::from(map.height) - 6,
                "Room door footprint is outside the map."
            );
            anyhow::ensure!(
                (x - 3..x + 3).all(|px| (y - 2..y + 2).all(|py| map.is_walkable(px, py))),
                "Room door requires a six-by-four walkable threshold."
            );
            for py in y - 3..y + 5 {
                anyhow::ensure!(map.is_walkable(x, py), "Room door crossing is blocked.");
                anyhow::ensure!(
                    occupied.insert((x, py)),
                    "Room door collision cuts overlap."
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_rejects_invalid_and_oversized_placements() {
        let mut map = MapFile::blank(32, 32);
        for x in 0..32 {
            for y in 0..32 {
                map.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        let mut layout = RoomDoors {
            version: 1,
            doors: vec![RoomDoor {
                center: [12, 12],
                source_monster: 1,
            }],
        };
        layout.validate(&map).unwrap();
        layout.doors.push(layout.doors[0].clone());
        assert!(layout.validate(&map).is_err());
        layout.doors.truncate(1);
        layout.doors[0].center = [i32::MAX, 12];
        assert!(layout.validate(&map).is_err());
        layout.doors[0].center = [12, 12];
        map.cell_mut(12, 12).unwrap().flag = 0;
        assert!(layout.validate(&map).is_err());
        let path = std::env::temp_dir().join(format!(
            "zircon-room-door-sidecar-{}.map",
            std::process::id()
        ));
        let sidecar = path.with_extension("doors.json");
        std::fs::write(&sidecar, b"not json").unwrap();
        assert!(RoomDoors::load(&path, &map).is_err());
        std::fs::write(&sidecar, vec![b' '; 16 * 1024 + 1]).unwrap();
        assert!(RoomDoors::load(&path, &map).is_err());
        std::fs::remove_file(sidecar).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_sidecar_is_rejected() {
        use std::os::unix::fs::symlink;
        let path =
            std::env::temp_dir().join(format!("zircon-room-door-link-{}.map", std::process::id()));
        let sidecar = path.with_extension("doors.json");
        let target = path.with_extension("target.json");
        std::fs::write(&target, b"{}").unwrap();
        symlink(&target, &sidecar).unwrap();
        assert!(RoomDoors::load(&path, &MapFile::blank(32, 32)).is_err());
        std::fs::remove_file(sidecar).unwrap();
        std::fs::remove_file(target).unwrap();
    }
}
