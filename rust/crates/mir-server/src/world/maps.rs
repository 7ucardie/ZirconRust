use super::*;
use mir_formats::room_door::RoomDoors;

impl World {
    /// Load a map (and register its spawn groups) if not already loaded.
    pub fn ensure_map(&mut self, index: i32) -> anyhow::Result<()> {
        if self.maps.contains_key(&index) {
            return Ok(());
        }
        let def = self
            .data
            .maps
            .get(&index)
            .ok_or_else(|| anyhow::anyhow!("unknown map {index}"))?
            .clone();
        let path = self.map_dir.join(format!("{}.map", def.file_name));
        let file =
            MapFile::load(&path).map_err(|e| anyhow::anyhow!("loading {}: {e}", path.display()))?;
        let doors = RoomDoors::load(&path, &file)?;
        if let Some(doors) = &doors {
            anyhow::ensure!(
                doors
                    .doors
                    .iter()
                    .all(|door| self.data.monsters.contains_key(&door.source_monster)),
                "Room door source monster is absent from System.db."
            );
        }
        let width = file.width as i32;
        let mut spawns = Vec::new();
        for r in self.data.respawns.iter().filter(|r| !r.event_spawn) {
            let Some(region) = self.data.regions.get(&r.region) else {
                continue;
            };
            if region.map != index {
                continue;
            }
            if !self.data.monsters.contains_key(&r.monster) {
                continue;
            }
            let points: Vec<Point> = region
                .points(width)
                .into_iter()
                .filter(|&(x, y)| file.is_walkable(x, y))
                .map(|(x, y)| Point::new(x, y))
                .collect();
            if points.is_empty() {
                continue;
            }
            spawns.push(SpawnGroup {
                def: r.clone(),
                points,
                alive: 0,
                next_spawn: 0,
            });
        }
        tracing::info!(
            map = index,
            name = def.description,
            file = def.file_name,
            size = format!("{}x{}", file.width, file.height),
            walkable = file.walkable_count(),
            spawn_groups = spawns.len(),
            monsters = spawns.iter().map(|s| s.def.count).sum::<i32>(),
            "map loaded"
        );
        let mut movements: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, m) in self.data.movements.iter().enumerate() {
            let Some(src) = self.data.regions.get(&m.source_region) else {
                continue;
            };
            if src.map != index {
                continue;
            }
            for p in src.movement_points(width) {
                movements.entry(p).or_default().push(i);
            }
        }
        self.maps.insert(
            index,
            MapState {
                index,
                file,
                descriptor: MapDescriptor {
                    file: def.file_name.clone(),
                    name: def.description.clone(),
                    light: def.light,
                    music: def.music,
                    mini_map: def.mini_map,
                },
                spawns,
                objects: Vec::new(),
                cells: HashMap::new(),
                movements,
            },
        );
        self.do_spawns(index);
        self.spawn_npcs(index);
        self.spawn_guards(index);
        self.spawn_castle_objects(index);
        if let Some(doors) = &doors {
            self.spawn_room_doors(index, doors);
        }
        Ok(())
    }

    pub(super) fn random_point(&mut self, points: &[Point]) -> Option<Point> {
        if points.is_empty() {
            return None;
        }
        Some(points[self.rng.random_range(0..points.len())])
    }

    /// Pick the start map/cell for a new character (Zircon `SetBindPoint` + `Spawn(BindRegion)`).
    pub(super) fn start_location(&mut self, class: Class) -> anyhow::Result<(i32, Point, i32)> {
        // Developer aid: ZIRCON_DEV_START=<map file>:<x>,<y> places new
        // characters on that cell (nearest walkable one).
        if let Some((file, x, y)) = std::env::var("ZIRCON_DEV_START").ok().and_then(|v| {
            let (file, xy) = v.split_once(':')?;
            let (x, y) = xy.split_once(',')?;
            Some((
                file.to_string(),
                x.trim().parse::<i32>().ok()?,
                y.trim().parse::<i32>().ok()?,
            ))
        }) {
            let map = self
                .data
                .map_by_file(&file)
                .ok_or_else(|| anyhow::anyhow!("ZIRCON_DEV_START {file}: no such map"))?
                .index;
            self.ensure_map(map)?;
            let want = Point::new(x, y);
            for r in 0..20 {
                for dx in -r..=r {
                    for dy in -r..=r {
                        let p = Point::new(x + dx, y + dy);
                        if self.maps[&map].file.is_walkable(p.x, p.y)
                            && !self.cell_blocked(map, p, true)
                        {
                            return Ok((map, p, 0));
                        }
                    }
                }
            }
            anyhow::bail!("ZIRCON_DEV_START: no walkable cell near {want:?}");
        }
        if let Some(file) = self.force_map.clone() {
            let map = self
                .data
                .map_by_file(&file)
                .ok_or_else(|| anyhow::anyhow!("--map {file}: no such map in System.db"))?
                .index;
            self.ensure_map(map)?;
            let m = &self.maps[&map];
            let (w, h) = (m.file.width as i32, m.file.height as i32);
            for _ in 0..10_000 {
                let p = Point::new(self.rng.random_range(0..w), self.rng.random_range(0..h));
                if self.maps[&map].file.is_walkable(p.x, p.y) && !self.cell_blocked(map, p, true) {
                    return Ok((map, p, 0));
                }
            }
            anyhow::bail!("no walkable cell found on {file}");
        }
        let flag = class.flag();
        let zones: Vec<(i32, i32)> = self
            .data
            .start_zones(flag)
            .iter()
            .filter_map(|z| {
                self.data
                    .regions
                    .get(&z.bind_region)
                    .map(|r| (z.bind_region, r.map))
            })
            .collect();
        if zones.is_empty() {
            anyhow::bail!("no start zone for {class:?}");
        }
        let (bind_region, map) = zones[self.rng.random_range(0..zones.len())];
        self.ensure_map(map)?;
        let point = self.bind_point(map, bind_region)?;
        Ok((map, point, bind_region))
    }

    pub(super) fn bind_point(&mut self, map: i32, bind_region: i32) -> anyhow::Result<Point> {
        let width = self.maps[&map].file.width as i32;
        let points: Vec<Point> = self.data.regions[&bind_region]
            .points(width)
            .into_iter()
            .map(|(x, y)| Point::new(x, y))
            .filter(|p| self.maps[&map].file.is_walkable(p.x, p.y))
            .collect();
        for _ in 0..20 {
            if let Some(p) = self.random_point(&points) {
                if !self.cell_blocked(map, p, true) {
                    return Ok(p);
                }
            }
        }
        self.random_point(&points)
            .ok_or_else(|| anyhow::anyhow!("bind region {bind_region} has no walkable cells"))
    }

    /// Move a player to a random cell of its bind point, changing map if
    /// the bind region lies elsewhere.
    pub(super) fn go_to_bind_point(&mut self, id: ObjectId) -> anyhow::Result<Point> {
        let bind_region = self.objects[&id].player().unwrap().bind_region;
        self.go_to_region(id, bind_region)
    }

    pub(super) fn go_to_region(&mut self, id: ObjectId, bind_region: i32) -> anyhow::Result<Point> {
        let map = self.objects[&id].map;
        let target_map = self
            .data
            .regions
            .get(&bind_region)
            .map(|r| r.map)
            .ok_or_else(|| anyhow::anyhow!("unknown bind region {bind_region}"))?;
        self.ensure_map(target_map)?;
        let location = self.bind_point(target_map, bind_region)?;
        if target_map == map {
            self.teleport_object(id, location);
        } else {
            self.change_map(id, target_map, location);
        }
        Ok(location)
    }

    /// Fewest map exits first; for equal routes, use the closest exit on the
    /// current map. Town scrolls need this rather than the character's spawn bind.
    pub(super) fn nearest_town_region(&self, map: i32, loc: Point, class: Class) -> Option<i32> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        let width = self.maps.get(&map)?.file.width as i32;
        let mut routes = BinaryHeap::from([Reverse((0_u32, 0_i32, map))]);
        let mut visited = HashSet::new();
        while let Some(Reverse((hops, first_exit_distance, here))) = routes.pop() {
            if !visited.insert(here) {
                continue;
            }
            if let Some(zone) = self
                .data
                .safe_zones
                .iter()
                .filter(|zone| zone.bind_region != 0)
                .filter(|zone| {
                    self.data
                        .regions
                        .get(&zone.region)
                        .is_some_and(|r| r.map == here)
                })
                .min_by_key(|zone| {
                    if here != map {
                        return 0;
                    }
                    self.data.regions[&zone.region]
                        .points(width)
                        .into_iter()
                        .map(|(x, y)| (x - loc.x).abs() + (y - loc.y).abs())
                        .min()
                        .unwrap_or(i32::MAX)
                })
            {
                return Some(zone.bind_region);
            }
            for movement in &self.data.movements {
                if movement.need_item != 0
                    || movement.need_spawn != 0
                    || (movement.required_class != 0 && movement.required_class & class.flag() == 0)
                {
                    continue;
                }
                let (Some(source), Some(dest)) = (
                    self.data.regions.get(&movement.source_region),
                    self.data.regions.get(&movement.destination_region),
                ) else {
                    continue;
                };
                if source.map != here || visited.contains(&dest.map) {
                    continue;
                }
                let distance = if hops == 0 {
                    source
                        .movement_points(width)
                        .into_iter()
                        .map(|(x, y)| (x - loc.x).abs() + (y - loc.y).abs())
                        .min()
                        .unwrap_or(i32::MAX)
                } else {
                    first_exit_distance
                };
                if distance != i32::MAX {
                    routes.push(Reverse((hops + 1, distance, dest.map)));
                }
            }
        }
        None
    }

    // ---- map travel -------------------------------------------------------------

    pub(super) fn random_walkable(&mut self, map: i32) -> Option<Point> {
        let (w, h) = {
            let m = self.maps.get(&map)?;
            (m.file.width as i32, m.file.height as i32)
        };
        for _ in 0..10_000 {
            let p = Point::new(self.rng.random_range(0..w), self.rng.random_range(0..h));
            if self.maps[&map].file.is_walkable(p.x, p.y) && !self.cell_blocked(map, p, true) {
                return Some(p);
            }
        }
        None
    }

    /// Zircon `Cell.GetMovement`: pick a random movement on the cell, a random
    /// destination cell, check level, then relocate.
    pub(super) fn try_travel(&mut self, id: ObjectId) -> bool {
        let Some(o) = self.objects.get(&id) else {
            return false;
        };
        if !o.is_player() {
            return false;
        }
        let (map, loc, level) = (o.map, o.location, o.player().unwrap().level);
        let Some(indices) = self
            .maps
            .get(&map)
            .and_then(|m| m.movements.get(&(loc.x, loc.y)).cloned())
        else {
            return false;
        };
        for _ in 0..5 {
            let mi = indices[self.rng.random_range(0..indices.len())];
            let m = self.data.movements[mi].clone();
            let Some(dest) = self.data.regions.get(&m.destination_region).cloned() else {
                continue;
            };
            let Some(dest_map) = self.data.maps.get(&dest.map).cloned() else {
                continue;
            };
            if dest_map.minimum_level > level {
                self.send_to(
                    id,
                    ServerMessage::Chat {
                        text: format!(
                            "You need level {} to enter {}.",
                            dest_map.minimum_level, dest_map.description
                        ),
                    },
                );
                return false;
            }
            if self.ensure_map(dest_map.index).is_err() {
                continue;
            }
            let width = self.maps[&dest_map.index].file.width as i32;
            let points: Vec<Point> = dest
                .points(width)
                .into_iter()
                .map(|(x, y)| Point::new(x, y))
                .filter(|p| self.maps[&dest_map.index].file.is_walkable(p.x, p.y))
                .collect();
            let Some(target) = self.random_point(&points) else {
                continue;
            };
            self.change_map(id, dest_map.index, target);
            return true;
        }
        false
    }

    /// Move a player to another map (or cell) and resync the client.
    pub(super) fn change_map(&mut self, id: ObjectId, map: i32, to: Point) {
        if self.ensure_map(map).is_err() {
            return;
        }
        let (old_map, old_loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        if let Some(m) = self.maps.get_mut(&old_map) {
            m.objects.retain(|x| *x != id);
            m.remove_from_cell(id, old_loc);
        }
        {
            let o = self.objects.get_mut(&id).unwrap();
            o.map = map;
            o.location = to;
            o.cell_time = self.now + CELL_GRACE;
            if let Some(p) = o.player_mut() {
                p.npc = None;
            }
        }
        self.refresh_safe_zone(id);
        self.refresh_war_flags();
        if !self.data.maps.get(&map).is_some_and(|m| m.can_horse) {
            self.remove_mount(id);
        }
        let m = self.maps.get_mut(&map).unwrap();
        m.objects.push(id);
        m.add_to_cell(id, to);
        let desc = m.descriptor.clone();
        let dir = self.objects[&id].direction;
        // Forget everything seen; visibility will re-add what is around.
        let old: Vec<ObjectId> = self.objects[&id].visible.iter().copied().collect();
        for v in old {
            self.send_to(id, ServerMessage::ObjectRemove { id: v });
        }
        self.objects.get_mut(&id).unwrap().visible.clear();
        self.send_to(
            id,
            ServerMessage::MapChanged {
                map: desc,
                location: to,
                direction: dir,
            },
        );
        self.send_safe_zones(id, map);
        // Landing on another movement cell chains (Zircon recurses).
        if old_map != map || old_loc != to {
            let _ = self.try_travel(id);
        }
    }
}
