//! Seeded BSP rooms connected by a minimum spanning tree of protected corridors.
use crate::{
    cave_art,
    generate::{self, Layout, Random},
    tile_doc::Document,
};
use mir_formats::map::MapFile;
use std::{collections::BTreeSet, path::Path};
type Point = (i32, i32);

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Room {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
impl Room {
    pub fn center(&self) -> Point {
        (self.x + self.width / 2, self.y + self.height / 2)
    }
}
struct Topology {
    rooms: Vec<Room>,
    floor: BTreeSet<Point>,
    corridors: BTreeSet<Point>,
}
fn between(rng: &mut Random, min: i32, max: i32) -> i32 {
    min + (rng.next() % (max - min + 1) as u64) as i32
}
fn topology(
    width: u16,
    height: u16,
    seed: u64,
    count: usize,
    corridor_width: u16,
) -> anyhow::Result<Topology> {
    anyhow::ensure!(
        (32..=256).contains(&width)
            && (32..=256).contains(&height)
            && width.is_multiple_of(2)
            && height.is_multiple_of(2),
        "Dungeon size must be even, between 32 and 256."
    );
    anyhow::ensure!((2..=12).contains(&count), "Choose between 2 and 12 rooms.");
    anyhow::ensure!(
        (2..=6).contains(&corridor_width) && corridor_width.is_multiple_of(2),
        "Corridor width must be 2, 4 or 6 cells."
    );
    let mut rng = Random(seed);
    let mut leaves = vec![Room {
        x: 1,
        y: 1,
        width: width as i32 / 2 - 2,
        height: height as i32 / 2 - 2,
    }];
    while leaves.len() < count {
        let pick=leaves.iter().enumerate().filter(|(_,r)|r.width>=10 || r.height>=10)
            .max_by_key(|(i,r)|(r.width*r.height,std::cmp::Reverse(*i))).map(|(i,_)|i)
            .ok_or_else(||anyhow::anyhow!("The canvas cannot fit {count} rooms. Increase New size or reduce the room count."))?;
        let r = leaves.remove(pick);
        let vertical = r.width >= 10
            && (r.height < 10
                || r.width > r.height
                || (r.width == r.height && rng.next().is_multiple_of(2)));
        let split = between(
            &mut rng,
            5,
            if vertical { r.width - 5 } else { r.height - 5 },
        );
        if vertical {
            leaves.push(Room {
                x: r.x,
                y: r.y,
                width: split,
                height: r.height,
            });
            leaves.push(Room {
                x: r.x + split,
                y: r.y,
                width: r.width - split,
                height: r.height,
            });
        } else {
            leaves.push(Room {
                x: r.x,
                y: r.y,
                width: r.width,
                height: split,
            });
            leaves.push(Room {
                x: r.x,
                y: r.y + split,
                width: r.width,
                height: r.height - split,
            });
        }
    }
    let mut rooms = vec![];
    let mut floor = BTreeSet::new();
    for leaf in leaves {
        let w = between(&mut rng, 3, (leaf.width - 2).min(9));
        let h = between(&mut rng, 3, (leaf.height - 2).min(9));
        let x = between(&mut rng, leaf.x + 1, leaf.x + leaf.width - w - 1);
        let y = between(&mut rng, leaf.y + 1, leaf.y + leaf.height - h - 1);
        for x in x..x + w {
            for y in y..y + h {
                floor.insert((x, y));
            }
        }
        rooms.push(Room {
            x: x * 2,
            y: y * 2,
            width: w * 2,
            height: h * 2,
        });
    }
    let centers: Vec<_> = rooms
        .iter()
        .map(|r| {
            let (x, y) = r.center();
            (x / 2, y / 2)
        })
        .collect();
    // Prim's algorithm makes every room reachable with count-1 links.
    let mut reached = BTreeSet::from([0]);
    let mut corridors = BTreeSet::new();
    while reached.len() < count {
        let (_, from, to) = reached
            .iter()
            .flat_map(|&i| {
                let centers = &centers;
                (0..count).filter(|j| !reached.contains(j)).map(move |j| {
                    let a = centers[i];
                    let b = centers[j];
                    ((a.0 - b.0).abs() + (a.1 - b.1).abs(), i, j)
                })
            })
            .min()
            .unwrap();
        let a = centers[from];
        let b = centers[to];
        let corner = if rng.next().is_multiple_of(2) {
            (b.0, a.1)
        } else {
            (a.0, b.1)
        };
        let thickness = corridor_width as i32 / 2;
        for (p, q) in [(a, corner), (corner, b)] {
            for x in p.0.min(q.0)..=p.0.max(q.0) {
                for y in p.1.min(q.1)..=p.1.max(q.1) {
                    for dx in -(thickness / 2)..thickness - thickness / 2 {
                        for dy in -(thickness / 2)..thickness - thickness / 2 {
                            corridors.insert((x + dx, y + dy));
                        }
                    }
                }
            }
        }
        reached.insert(to);
    }
    floor.extend(corridors.iter().copied());
    let corridors = corridors
        .into_iter()
        .flat_map(|(x, y)| {
            [
                (x * 2, y * 2),
                (x * 2 + 1, y * 2),
                (x * 2, y * 2 + 1),
                (x * 2 + 1, y * 2 + 1),
            ]
        })
        .collect();
    Ok(Topology {
        rooms,
        floor,
        corridors,
    })
}

pub fn generate(
    source: &MapFile,
    width: u16,
    height: u16,
    seed: u64,
    count: usize,
    corridor_width: u16,
    assets: &Path,
) -> anyhow::Result<(Document, Layout, cave_art::Report)> {
    let t = topology(width, height, seed, count, corridor_width)?;
    let mut rng = Random(seed ^ 0xe7037ed1a0b428db);
    let mut doc = generate::paint_floor(source, width, height, &t.floor, &mut rng)?;
    let (templates, mut report) = cave_art::extract(source, assets)?;
    let mut protected = t.corridors.clone();
    protected.extend(t.rooms.iter().map(Room::center));
    cave_art::apply_protected(&mut doc.map, &templates, seed, &mut report, &protected);
    let mut layout = generate::layout(&doc.map, seed)?;
    layout.rooms = t.rooms;
    layout.corridors = t.corridors.into_iter().collect();
    layout.corridor_width = corridor_width;
    layout.validate(&doc.map)?;
    Ok((doc, layout, report))
}

/// Room-dungeon with a complete private wall library around rooms and passages.
pub fn generate_full_art(
    source: &MapFile,
    width: u16,
    height: u16,
    seed: u64,
    count: usize,
    corridor_width: u16,
    assets: &Path,
) -> anyhow::Result<(Document, Layout, cave_art::Report, Vec<u8>, usize)> {
    let (mut doc, layout, report) =
        generate(source, width, height, seed, count, corridor_width, assets)?;
    let (library, walls) = crate::freeform_art::frame(&mut doc.map, assets, seed, Some(&layout))?;
    Ok((doc, layout, report, library, walls))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rooms_and_corridors_are_connected_and_seeded_at_all_widths() {
        for width in [2, 4, 6] {
            for seed in 0..12 {
                let t = topology(64, 64, seed, 8, width).unwrap();
                assert_eq!(t.rooms.len(), 8);
                assert_eq!(t.rooms, topology(64, 64, seed, 8, width).unwrap().rooms);
                let mut map = MapFile::blank(64, 64);
                for &(x, y) in &t.floor {
                    for dx in 0..2 {
                        for dy in 0..2 {
                            map.cell_mut(x * 2 + dx, y * 2 + dy).unwrap().flag = 3;
                        }
                    }
                }
                let l = generate::layout(&map, seed).unwrap();
                l.validate(&map).unwrap();
                for r in &t.rooms {
                    let p = r.center();
                    assert!(map.is_walkable(p.0, p.1));
                }
                for (i, a) in t.rooms.iter().enumerate() {
                    for b in &t.rooms[i + 1..] {
                        assert!(
                            a.x + a.width <= b.x
                                || b.x + b.width <= a.x
                                || a.y + a.height <= b.y
                                || b.y + b.height <= a.y
                        );
                    }
                }
                assert!(t.corridors.iter().all(|&(x, y)| map.is_walkable(x, y)));
            }
        }
        assert_ne!(
            topology(64, 64, 1, 8, 4).unwrap().rooms,
            topology(64, 64, 2, 8, 4).unwrap().rooms
        );
    }
    #[test]
    fn wider_passages_preserve_the_route_and_add_clearance() {
        let narrow = topology(64, 64, 42, 8, 2).unwrap();
        let medium = topology(64, 64, 42, 8, 4).unwrap();
        let wide = topology(64, 64, 42, 8, 6).unwrap();
        assert_eq!(narrow.rooms, wide.rooms);
        assert!(narrow.corridors.is_subset(&medium.corridors));
        assert!(medium.corridors.is_subset(&wide.corridors));
        assert!(
            narrow.corridors.len() < medium.corridors.len()
                && medium.corridors.len() < wide.corridors.len()
        );
        for (w, h, count) in [(32, 32, 2), (256, 32, 12), (32, 256, 12), (256, 256, 12)] {
            let t = topology(w, h, 42, count, 6).unwrap();
            assert!(t
                .floor
                .iter()
                .all(|&(x, y)| x > 0 && y > 0 && x < w as i32 / 2 - 1 && y < h as i32 / 2 - 1));
        }
    }
    #[test]
    fn invalid_or_overcrowded_requests_are_rejected() {
        assert!(topology(32, 32, 42, 12, 2).is_err());
        assert!(topology(64, 64, 42, 8, 3).is_err());
        assert!(topology(63, 64, 42, 8, 2).is_err());
        assert!(topology(64, 64, 42, 1, 2).is_err());
    }
}
