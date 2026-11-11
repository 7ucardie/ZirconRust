//! Explicit walkable edge connections for authored map pieces.
use mir_formats::map::MapFile;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    North,
    East,
    South,
    West,
}
impl Side {
    pub fn label(self) -> &'static str {
        match self {
            Self::North => "North",
            Self::East => "East",
            Self::South => "South",
            Self::West => "West",
        }
    }
    fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::East => Self::West,
            Self::South => Self::North,
            Self::West => Self::East,
        }
    }
    fn outward(self) -> (i32, i32) {
        match self {
            Self::North => (0, -1),
            Self::East => (1, 0),
            Self::South => (0, 1),
            Self::West => (-1, 0),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Port {
    pub side: Side,
    #[serde(default)]
    pub inset: u16,
    pub offset: u16,
    pub width: u16,
    pub profile: String,
}
impl Default for Port {
    fn default() -> Self {
        Self {
            side: Side::East,
            inset: 0,
            offset: 0,
            width: 2,
            profile: "default".into(),
        }
    }
}
impl Port {
    pub fn cells(&self, map: &MapFile) -> Vec<(i32, i32)> {
        (i32::from(self.offset)..i32::from(self.offset) + i32::from(self.width))
            .map(|n| match self.side {
                Side::North => (n, i32::from(self.inset)),
                Side::East => (i32::from(map.width) - 1 - i32::from(self.inset), n),
                Side::South => (n, i32::from(map.height) - 1 - i32::from(self.inset)),
                Side::West => (i32::from(self.inset), n),
            })
            .collect()
    }
}
/// Every opening has two walkable cells of depth, and all declared openings
/// connect inside the piece. This checks collision, not visual art compatibility.
pub fn validate(map: &MapFile, ports: &[Port]) -> anyhow::Result<()> {
    anyhow::ensure!(
        ports.len() <= 16,
        "A piece supports at most 16 connections."
    );
    let mut openings = BTreeSet::new();
    for (i, p) in ports.iter().enumerate() {
        anyhow::ensure!(
            !p.profile.trim().is_empty() && p.profile.chars().count() <= 40,
            "Connection {} needs a profile of 1–40 characters.",
            i + 1
        );
        let edge = match p.side {
            Side::North | Side::South => map.width,
            _ => map.height,
        };
        anyhow::ensure!(
            p.width > 0 && u32::from(p.offset) + u32::from(p.width) <= u32::from(edge),
            "Connection {} extends past its edge.",
            i + 1
        );
        let depth = match p.side {
            Side::North | Side::South => map.height,
            _ => map.width,
        };
        anyhow::ensure!(
            p.inset < depth,
            "Connection inset exceeds piece dimensions."
        );
        let (dx, dy) = p.side.outward();
        for (x, y) in p.cells(map) {
            anyhow::ensure!(openings.insert((x, y)), "Connection openings overlap.");
            anyhow::ensure!(
                map.is_walkable(x, y) && map.is_walkable(x - dx, y - dy),
                "Connection {} needs walkable edge and interior cells.",
                i + 1
            );
        }
    }
    if let Some(&start) = openings.first() {
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some((x, y)) = queue.pop_front() {
            for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let q = (x + dx, y + dy);
                if map.is_walkable(q.0, q.1) && seen.insert(q) {
                    queue.push_back(q);
                }
            }
        }
        anyhow::ensure!(
            openings.is_subset(&seen),
            "Connections are separated by blocked cells inside the piece."
        );
    }
    Ok(())
}
/// Translate B beside A without rotating sprites or shifting the back-tile grid.
pub fn alignment(a: &MapFile, pa: &Port, b: &MapFile, pb: &Port) -> anyhow::Result<(i32, i32)> {
    validate(a, std::slice::from_ref(pa))?;
    validate(b, std::slice::from_ref(pb))?;
    anyhow::ensure!(
        pa.side.opposite() == pb.side,
        "Connections must face opposite directions."
    );
    anyhow::ensure!(pa.width == pb.width, "Connection widths must match.");
    anyhow::ensure!(
        pa.profile == pb.profile,
        "Connection profiles must match exactly."
    );
    let (ax, ay) = pa.cells(a)[0];
    let (bx, by) = pb.cells(b)[0];
    let (dx, dy) = pa.side.outward();
    let shift = (ax + dx - bx, ay + dy - by);
    anyhow::ensure!(
        shift.0 % 2 == 0 && shift.1 % 2 == 0,
        "These offsets would misalign the back tiles. Use offsets with matching parity."
    );
    Ok(shift)
}
/// Validate that openings lie on owned-cell boundaries, including recesses.
pub fn validate_boundary(
    map: &MapFile,
    ports: &[Port],
    coverage: Option<&[bool]>,
) -> anyhow::Result<()> {
    if let Some(mask) = coverage {
        anyhow::ensure!(
            mask.len() == usize::from(map.width) * usize::from(map.height),
            "Invalid connection coverage."
        );
    }
    let mut covered_map;
    if let Some(mask) = coverage {
        covered_map = map.clone();
        for x in 0..i32::from(map.width) {
            for y in 0..i32::from(map.height) {
                if !mask[(y * i32::from(map.width) + x) as usize] {
                    covered_map.cell_mut(x, y).unwrap().flag = 0;
                }
            }
        }
        validate(&covered_map, ports)?;
    } else {
        validate(map, ports)?;
    }
    let owned = |x: i32, y: i32| {
        map.cell(x, y).is_some()
            && coverage.is_none_or(|m| m[(y * i32::from(map.width) + x) as usize])
    };
    for p in ports {
        let (dx, dy) = p.side.outward();
        for (x, y) in p.cells(map) {
            anyhow::ensure!(
                owned(x, y) && owned(x - dx, y - dy) && !owned(x + dx, y + dy),
                "Connection must face uncovered space with covered edge and inward cells."
            );
        }
    }
    Ok(())
}
/// Keep the exact opening position even when it becomes a recessed boundary.
pub fn translate(
    port: &Port,
    source: &MapFile,
    shift: (i32, i32),
    target: &MapFile,
) -> anyhow::Result<Port> {
    let first = port.cells(source)[0];
    let (x, y) = (first.0 + shift.0, first.1 + shift.1);
    let (inset, offset) = match port.side {
        Side::North => (y, x),
        Side::East => (i32::from(target.width) - 1 - x, y),
        Side::South => (i32::from(target.height) - 1 - y, x),
        Side::West => (x, y),
    };
    Ok(Port {
        inset: u16::try_from(inset)?,
        offset: u16::try_from(offset)?,
        ..port.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn room() -> MapFile {
        let mut m = MapFile::blank(8, 8);
        for x in 0..8 {
            for y in 0..8 {
                m.cell_mut(x, y).unwrap().flag = 3;
            }
        }
        m
    }
    fn port(side: Side) -> Port {
        Port {
            side,
            offset: 2,
            ..Port::default()
        }
    }
    #[test]
    fn opening_depth_and_internal_route_are_required() {
        let mut m = room();
        let ports = [port(Side::West), port(Side::East)];
        validate(&m, &ports).unwrap();
        m.cell_mut(1, 2).unwrap().flag = 0;
        assert!(validate(&m, &ports).is_err());
        m.cell_mut(1, 2).unwrap().flag = 3;
        for y in 0..8 {
            m.cell_mut(4, y).unwrap().flag = 0;
        }
        assert!(validate(&m, &ports).is_err());
        m.cell_mut(4, 6).unwrap().flag = 3;
        validate(&m, &ports).unwrap();
        assert!(validate(&m, &[ports[0].clone(), ports[0].clone()]).is_err());
        assert!(validate(
            &m,
            &[Port {
                width: 0,
                ..ports[0].clone()
            }]
        )
        .is_err());
        assert!(validate(
            &m,
            &[Port {
                offset: u16::MAX,
                ..ports[0].clone()
            }]
        )
        .is_err());
    }
    #[test]
    fn recessed_openings_require_uncovered_outside_and_covered_depth() {
        let map = room();
        let p = Port {
            side: Side::North,
            inset: 4,
            offset: 2,
            ..Port::default()
        };
        assert!(validate_boundary(&map, std::slice::from_ref(&p), None).is_err());
        let mut coverage = vec![true; 64];
        for y in 0..4 {
            for x in 0..8 {
                coverage[y * 8 + x] = false;
            }
        }
        validate_boundary(&map, std::slice::from_ref(&p), Some(&coverage)).unwrap();
        coverage[5 * 8 + 2] = false;
        assert!(validate_boundary(&map, std::slice::from_ref(&p), Some(&coverage)).is_err());
        assert!(validate_boundary(
            &map,
            &[Port {
                inset: u16::MAX,
                ..p
            }],
            Some(&coverage)
        )
        .is_err());
    }
    #[test]
    fn matching_openings_align_in_all_directions_without_rotation() {
        let m = room();
        for (side, expected) in [
            (Side::North, (0, -8)),
            (Side::East, (8, 0)),
            (Side::South, (0, 8)),
            (Side::West, (-8, 0)),
        ] {
            let a = port(side);
            let b = port(side.opposite());
            assert_eq!(alignment(&m, &a, &m, &b).unwrap(), expected);
            assert!(alignment(&m, &a, &m, &a).is_err());
            assert!(alignment(
                &m,
                &a,
                &m,
                &Port {
                    offset: 3,
                    ..b.clone()
                }
            )
            .is_err());
            assert!(alignment(
                &m,
                &a,
                &m,
                &Port {
                    width: 1,
                    ..b.clone()
                }
            )
            .is_err());
            assert!(alignment(
                &m,
                &a,
                &m,
                &Port {
                    profile: "other".into(),
                    ..b
                }
            )
            .is_err());
        }
    }
}
