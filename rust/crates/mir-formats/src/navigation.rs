//! Optional navigation markers for generated maps, separate from the game protocol.
use crate::{FormatError, MapFile, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Portal {
    pub position: (i32, i32),
    pub destination: (i32, i32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Navigation {
    pub version: u16,
    pub width: u16,
    pub height: u16,
    /// Exact map-byte checksum for stale-file detection, not authentication.
    pub map_checksum: String,
    pub portals: Vec<Portal>,
}

fn checksum(map: &MapFile) -> String {
    let hash = map
        .to_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, b| {
            (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
    format!("{hash:016x}")
}

impl Navigation {
    pub fn new(map: &MapFile, portals: Vec<Portal>) -> Result<Self> {
        let navigation = Self {
            version: 1,
            width: map.width,
            height: map.height,
            map_checksum: checksum(map),
            portals,
        };
        navigation.validate(map)?;
        Ok(navigation)
    }

    pub fn validate(&self, map: &MapFile) -> Result<()> {
        if self.version != 1 {
            return Err(FormatError::Invalid("navigation version"));
        }
        if (self.width, self.height) != (map.width, map.height)
            || self.map_checksum != checksum(map)
        {
            return Err(FormatError::Invalid(
                "navigation map identity (stale sidecar)",
            ));
        }
        let mut positions = BTreeSet::new();
        for portal in &self.portals {
            if !map.is_walkable(portal.position.0, portal.position.1)
                || !map.is_walkable(portal.destination.0, portal.destination.1)
            {
                return Err(FormatError::Invalid(
                    "navigation portal outside walkable map",
                ));
            }
            if !positions.insert(portal.position) {
                return Err(FormatError::Invalid("duplicate navigation portal"));
            }
        }
        if self
            .portals
            .iter()
            .any(|p| positions.contains(&p.destination))
        {
            return Err(FormatError::Invalid("navigation landing triggers a portal"));
        }
        Ok(())
    }

    /// Missing metadata is normal for original maps; invalid metadata is an error.
    pub fn load(map_path: &Path, map: &MapFile) -> Result<Option<Self>> {
        let bytes = match std::fs::read(map_path.with_extension("navigation.json")) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let navigation: Self = serde_json::from_slice(&bytes)
            .map_err(|error| FormatError::Other(format!("Invalid navigation metadata: {error}")))?;
        navigation.validate(map)?;
        Ok(Some(navigation))
    }

    pub fn save(&self, map_path: &Path, map: &MapFile) -> Result<()> {
        self.validate(map)?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| FormatError::Other(error.to_string()))?;
        crate::save_with_backup(map_path.with_extension("navigation.json"), &bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> MapFile {
        let mut map = MapFile::blank(8, 8);
        for x in 0..8 {
            for y in 0..8 {
                map.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        map
    }

    fn portals() -> Vec<Portal> {
        vec![
            Portal {
                position: (1, 1),
                destination: (6, 5),
            },
            Portal {
                position: (6, 6),
                destination: (1, 2),
            },
        ]
    }

    #[test]
    fn sidecar_round_trip_and_missing_or_malformed_metadata() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder =
            std::env::temp_dir().join(format!("zircon-navigation-{}-{unique}", std::process::id()));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("draft.map");
        let map = map();
        assert!(Navigation::load(&path, &map).unwrap().is_none());
        let navigation = Navigation::new(&map, portals()).unwrap();
        navigation.save(&path, &map).unwrap();
        assert_eq!(Navigation::load(&path, &map).unwrap(), Some(navigation));
        std::fs::write(path.with_extension("navigation.json"), b"{broken").unwrap();
        assert!(Navigation::load(&path, &map).is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn rejects_stale_maps_versions_and_invalid_portals() {
        let map = map();
        let navigation = Navigation::new(&map, portals()).unwrap();
        let mut changed = map.clone();
        changed.cell_mut(0, 0).unwrap().light = 2;
        changed.cell_mut(0, 0).unwrap().light_raw = 1;
        assert!(navigation.validate(&changed).is_err());
        assert!(navigation.validate(&MapFile::blank(10, 8)).is_err());
        let mut future = navigation.clone();
        future.version = 2;
        assert!(future.validate(&map).is_err());
        for position in [(-1, 1), (8, 1), (1, 8), (i32::MAX, i32::MIN), (6, 6)] {
            let mut bad = portals();
            bad[0].position = position;
            assert!(Navigation::new(&map, bad).is_err());
        }
        for destination in [(-1, 1), (8, 1), (1, 1), (6, 6)] {
            let mut bad = portals();
            bad[0].destination = destination;
            assert!(Navigation::new(&map, bad).is_err());
        }
        for (x, y) in [(1, 1), (6, 5)] {
            let mut blocked = map.clone();
            blocked.cell_mut(x, y).unwrap().flag = 0;
            assert!(Navigation::new(&blocked, portals()).is_err());
        }
    }
}
