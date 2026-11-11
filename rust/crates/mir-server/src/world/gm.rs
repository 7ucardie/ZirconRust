//! Server-authoritative GM chat commands. The persisted character grant is
//! checked here; clients cannot grant themselves command access.

use super::*;
use crate::items::StatSource;
use mir_proto::ChatKind;

impl World {
    pub(super) fn gm_chat(&mut self, id: ObjectId, command: &str) {
        if !self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .is_some_and(|p| p.gm)
        {
            self.gm_line(id, "GM access required.");
            return;
        }
        let words: Vec<&str> = command.split_whitespace().collect();
        let name = words.first().copied().unwrap_or("").to_ascii_lowercase();
        let args = &words[usize::from(!words.is_empty())..];
        if name.is_empty() || name == "help" {
            for line in [
                "GM world: @move @goto @recall @monster @moblocation @make @makespecial @level @addstat @refineweapon",
                "GM character: @giveskills @levelskill @resetdiscipline @sethermitstat @changegender @setcompanionlevel @setcompanionstat @givehorse @promotefame @removepkpoints",
                "GM account: @ban @chatban @globalban @kick @givegamegold @takegamegold @removecaption",
                "GM guild/server: @createguild @startconquest @endconquest @takecastle @gamemaster @superman @observer @reboot @clearipblocks @gcollect",
            ] { self.gm_line(id, line); }
            return;
        }
        if name == "moblocation" {
            match self.gm_mob_location(args) {
                Ok(lines) => {
                    for line in lines {
                        self.gm_line(id, &line);
                    }
                }
                Err(error) => self.gm_line(id, &error),
            }
            return;
        }
        let result = match name.as_str() {
            "move" => self.gm_move(id, args),
            "goto" => self.gm_goto(id, args),
            "recall" => self.gm_recall(id, args),
            "monster" | "spawn" => self.gm_monster(id, args),
            "make" => self.gm_make(id, args, false),
            "makespecial" => self.gm_make(id, args, true),
            "level" => self.gm_level(id, args),
            "giveskills" => self.gm_give_skills(id, args),
            "levelskill" => self.gm_level_skill(id, args),
            "resetdiscipline" => self.gm_reset_discipline(id, args),
            "setcompanionlevel" => self.gm_set_companion_level(id, args),
            "givehorse" => self.gm_give_horse(id, args),
            "addstat" => self.gm_add_stat(id, args, StatSource::Added),
            "refineweapon" => self.gm_refine_weapon(id, args),
            "sethermitstat" | "sethermistat" => self.gm_set_hermit_stat(id, args),
            "changegender" => self.gm_change_gender(id, args),
            "setcompanionstat" => self.gm_set_companion_stat(id, args),
            "startconquest" => self.gm_start_conquest(args),
            "endconquest" => self.gm_end_conquest(args),
            "takecastle" => self.gm_take_castle(id, args),
            "createguild" => self.gm_create_guild_command(id, args),
            "gamemaster" => self.gm_toggle(id, args, "gamemaster"),
            "superman" => self.gm_toggle(id, args, "superman"),
            "observer" => self.gm_toggle(id, args, "observer"),
            "reboot" if args.is_empty() => Ok(format!(
                "Cancelled {} affected listings.",
                self.gm_cancel_changed_listings()
            )),
            "removepkpoints" => self.gm_remove_pk_points(id, args),
            "promotefame" => self.gm_promote_fame(id, args),
            _ => Err(format!("Unknown GM command: {name}. Type @help.")),
        };
        self.gm_line(id, &result.unwrap_or_else(|error| error));
    }

    fn gm_line(&mut self, id: ObjectId, text: &str) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text: text.to_string(),
            },
        );
    }

    fn gm_move(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let (map, point) = match args {
            [x, y] if x.parse::<i32>().is_ok() && y.parse::<i32>().is_ok() => (
                self.objects[&id].map,
                Some(Point::new(x.parse().unwrap(), y.parse().unwrap())),
            ),
            [file] | [file, _, _] => {
                let map = self
                    .data
                    .maps
                    .values()
                    .find(|map| map.file_name.eq_ignore_ascii_case(file))
                    .map(|map| map.index)
                    .ok_or_else(|| format!("Unknown map: {file}"))?;
                let point = if args.len() == 3 {
                    Some(Point::new(
                        args[1].parse().map_err(|_| "Invalid X coordinate")?,
                        args[2].parse().map_err(|_| "Invalid Y coordinate")?,
                    ))
                } else {
                    None
                };
                (map, point)
            }
            _ => return Err("Usage: @move <map file> [x y] or @move <x> <y>".into()),
        };
        self.ensure_map(map)
            .map_err(|e| format!("Cannot load map: {e}"))?;
        let point = match point {
            Some(point) => point,
            None => {
                let file = &self.maps[&map].file;
                let (width, height) = (file.width as i32, file.height as i32);
                if width <= 0 || height <= 0 {
                    return Err("Map has no walkable cells".into());
                }
                let mut found = None;
                for _ in 0..1000 {
                    let candidate = Point::new(
                        self.rng.random_range(0..width),
                        self.rng.random_range(0..height),
                    );
                    if !self.cell_blocked(map, candidate, false)
                        && !self.maps[&map]
                            .movements
                            .contains_key(&(candidate.x, candidate.y))
                    {
                        found = Some(candidate);
                        break;
                    }
                }
                found.ok_or("No free cell on that map")?
            }
        };
        let point = if self.maps[&map].movements.contains_key(&(point.x, point.y)) {
            self.gm_safe_point(map, point)
                .ok_or("No free cell near that portal")?
        } else {
            point
        };
        if self.cell_blocked(map, point, false) {
            return Err(format!("Blocked cell: {}, {}", point.x, point.y));
        }
        if self.objects[&id].map == map {
            self.teleport_object(id, point);
        } else {
            self.change_map(id, map, point);
        }
        Ok(format!(
            "Moved to map {} at {}, {}.",
            self.data.maps[&map].file_name, point.x, point.y
        ))
    }

    /// Pick a free cell near a monster or portal without triggering travel.
    fn gm_safe_point(&self, map: i32, center: Point) -> Option<Point> {
        let state = self.maps.get(&map)?;
        for radius in 0i32..=10 {
            for y in -radius..=radius {
                for x in -radius..=radius {
                    if x.abs().max(y.abs()) != radius {
                        continue;
                    }
                    let point = Point::new(center.x + x, center.y + y);
                    if !state.movements.contains_key(&(point.x, point.y))
                        && !self.cell_blocked(map, point, false)
                    {
                        return Some(point);
                    }
                }
            }
        }
        None
    }

    fn gm_goto(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let [name] = args else {
            return Err("Usage: @goto <player>".into());
        };
        let target = self
            .find_player(name)
            .ok_or_else(|| format!("Player {name} is not online."))?;
        let (map, point) = (self.objects[&target].map, self.objects[&target].location);
        // A player occupies their cell; choose a free neighbour.
        let point = Direction::ALL
            .iter()
            .map(|dir| point.step(*dir, 1))
            .find(|point| !self.cell_blocked(map, *point, false))
            .ok_or("No free cell beside that player")?;
        if self.objects[&id].map == map {
            self.teleport_object(id, point);
        } else {
            self.change_map(id, map, point);
        }
        Ok(format!("Moved to {name}."))
    }

    fn gm_recall(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let [name] = args else {
            return Err("Usage: @recall <player>".into());
        };
        let target = self
            .find_player(name)
            .ok_or_else(|| format!("Player {name} is not online."))?;
        if target == id {
            return Err("You cannot recall yourself.".into());
        }
        let (map, point) = (self.objects[&id].map, self.objects[&id].location);
        let point = Direction::ALL
            .iter()
            .map(|dir| point.step(*dir, 1))
            .find(|point| !self.cell_blocked(map, *point, false))
            .ok_or("No free cell beside you")?;
        if self.objects[&target].map == map {
            self.teleport_object(target, point);
        } else {
            self.change_map(target, map, point);
        }
        Ok(format!("Recalled {name}."))
    }

    fn gm_monster(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let ([name] | [name, _]) = args else {
            return Err("Usage: @monster <id or name> [count]".into());
        };
        let count = if args.len() == 2 {
            args[1]
                .parse::<u32>()
                .map_err(|_| "Invalid monster count")?
        } else {
            1
        };
        if !(1..=50).contains(&count) {
            return Err("Monster count must be 1..50.".into());
        }
        let monster = name
            .parse::<i32>()
            .ok()
            .filter(|id| self.data.monsters.contains_key(id))
            .or_else(|| {
                self.data
                    .monsters
                    .values()
                    .find(|m| m.name.replace(' ', "").eq_ignore_ascii_case(name))
                    .map(|m| m.index)
            })
            .ok_or_else(|| format!("Unknown monster: {name}"))?;
        let (map, loc) = (self.objects[&id].map, self.objects[&id].location);
        let point = Direction::ALL
            .iter()
            .map(|dir| loc.step(*dir, 1))
            .find(|point| !self.cell_blocked(map, *point, false))
            .ok_or("No free adjacent cell")?;
        for _ in 0..count {
            self.create_monster(monster, map, point, None, None, 0);
        }
        Ok(format!("Spawned {count} monster(s): {name}."))
    }

    fn gm_mob_location(&self, args: &[&str]) -> Result<Vec<String>, String> {
        let query = args.join(" ");
        if query.is_empty() {
            return Err("Usage: @moblocation <monster id or name>".into());
        }
        let monsters: Vec<_> = if let Ok(index) = query.parse::<i32>() {
            self.data.monsters.get(&index).into_iter().collect()
        } else {
            let name = query.replace(' ', "");
            self.data
                .monsters
                .values()
                .filter(|monster| monster.name.replace(' ', "").eq_ignore_ascii_case(&name))
                .collect()
        };
        let [monster] = monsters.as_slice() else {
            if monsters.is_empty() {
                return Err(format!("Unknown monster: {query}"));
            }
            let mut ids: Vec<_> = monsters.iter().map(|monster| monster.index).collect();
            ids.sort_unstable();
            return Err(format!(
                "Several monsters named {query}: IDs {ids:?}. Use @moblocation <id>."
            ));
        };
        let mut locations: Vec<_> = self
            .data
            .respawns
            .iter()
            .filter(|spawn| spawn.monster == monster.index && spawn.count > 0)
            .filter_map(|spawn| {
                let region = self.data.regions.get(&spawn.region)?;
                let map = self.data.maps.get(&region.map)?;
                Some((spawn.event_spawn, map, region))
            })
            .collect();
        locations.sort_by_key(|(event, map, region)| (*event, map.index, region.index));
        locations.dedup_by_key(|(event, map, region)| (*event, map.index, region.index));
        let mut lines = vec![format!(
            "{} (ID {}) locations:",
            monster.name, monster.index
        )];
        let mut living: Vec<_> = self
            .objects
            .values()
            .filter(|object| {
                !object.dead && object.monster().is_some_and(|m| m.def == monster.index)
            })
            .collect();
        living.sort_by_key(|object| (object.map, object.location.x, object.location.y));
        lines.push(format!("{} alive on loaded maps.", living.len()));
        for object in living.iter().take(8) {
            let map = &self.data.maps[&object.map];
            let near = self
                .gm_safe_point(object.map, object.location)
                .map(|point| format!("; @move {} {} {}", map.file_name, point.x, point.y))
                .unwrap_or_default();
            lines.push(format!(
                "{} ({}): {}, {}{near}",
                map.description, map.file_name, object.location.x, object.location.y
            ));
        }
        if living.len() > 8 {
            lines.push(format!("... and {} more alive.", living.len() - 8));
        }
        if locations.is_empty() {
            lines
                .push("No saved respawn location; this monster may be summoned or GM-only.".into());
            return Ok(lines);
        }
        let mut loaded = HashMap::new();
        for (event, map, region) in locations.iter().take(12) {
            let file = loaded.entry(map.index).or_insert_with(|| {
                MapFile::load(self.map_dir.join(format!("{}.map", map.file_name))).ok()
            });
            let cells = file
                .as_ref()
                .map(|file| {
                    region
                        .points(file.width as i32)
                        .into_iter()
                        .filter(|&(x, y)| file.is_walkable(x, y))
                        .count()
                })
                .map(|count| format!("; {count} spawn cells"))
                .unwrap_or_default();
            let event = if *event { " [event only]" } else { "" };
            lines.push(format!(
                "{} ({}){event}{cells}; @move {} (rerun for live position)",
                map.description, map.file_name, map.file_name
            ));
        }
        if locations.len() > 12 {
            lines.push(format!("... and {} more regions.", locations.len() - 12));
        }
        Ok(lines)
    }

    fn gm_level(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let (target, value) = match args {
            [value] => (id, *value),
            [name, value] => (
                self.find_player(name)
                    .ok_or_else(|| format!("Player {name} is not online."))?,
                *value,
            ),
            _ => return Err("Usage: @level [player] <level 1..1000>".into()),
        };
        let value = value.parse::<i32>().map_err(|_| "Invalid level")?;
        if !(1..=mir_proto::MAX_PLAYER_LEVEL).contains(&value) {
            return Err(format!(
                "Level must be between 1 and {}.",
                mir_proto::MAX_PLAYER_LEVEL
            ));
        }
        let p = self
            .objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap();
        p.level = value;
        p.experience = 0;
        self.refresh_stats(target, true);
        self.send_player_stats(target);
        Ok(format!(
            "{} is now level {value}.",
            self.player_name(target)
        ))
    }

    fn gm_remove_pk_points(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let target = match args {
            [] => id,
            [name] => self
                .find_player(name)
                .ok_or_else(|| format!("Player {name} is not online."))?,
            _ => return Err("Usage: @removepkpoints [player]".into()),
        };
        let p = self
            .objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap();
        p.pk_points = 0;
        p.pk_tick = 0;
        p.brown = false;
        self.refresh_appearance(target);
        Ok(format!(
            "Cleared PK points for {}.",
            self.player_name(target)
        ))
    }

    fn gm_promote_fame(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let target = match args {
            [] => id,
            [name] => self
                .find_player(name)
                .ok_or_else(|| format!("Player {name} is not online."))?,
            _ => return Err("Usage: @promotefame [player]".into()),
        };
        let old = self.objects[&target].player().unwrap().fame;
        self.promote_fame(target);
        if self.objects[&target].player().unwrap().fame == old {
            return Err("Fame promotion requirements were not met.".into());
        }
        Ok(format!("Promoted {}'s fame.", self.player_name(target)))
    }

    fn gm_make(&mut self, id: ObjectId, args: &[&str], special: bool) -> Result<String, String> {
        let ([name] | [name, _] | [name, _, _]) = args else {
            return Err(if special {
                "Usage: @makespecial <equipment id or name> [count] [player]"
            } else {
                "Usage: @make <item id or name> [count] [player]"
            }
            .into());
        };
        let info = name
            .parse::<i32>()
            .ok()
            .filter(|n| self.data.items.contains_key(n))
            .or_else(|| {
                self.data
                    .items
                    .values()
                    .find(|item| item.name.replace(' ', "").eq_ignore_ascii_case(name))
                    .map(|item| item.index)
            })
            .ok_or_else(|| format!("Unknown item: {name}"))?;
        let count = if args.len() > 1 {
            args[1].parse::<u32>().map_err(|_| "Invalid item count")?
        } else {
            1
        };
        let currency = self.data.currencies.iter().find(|c| c.drop_item == info);
        if special && !Self::can_roll_special(self.data.items[&info].item_type) {
            return Err("Only equipment can have random specials.".into());
        }
        if count == 0 || (currency.is_none() && count > 1000) {
            return Err(
                "Item count must be positive; non-currency items are limited to 1000.".into(),
            );
        }
        let target = if args.len() == 3 {
            self.find_player(args[2])
                .ok_or_else(|| format!("Player {} is not online.", args[2]))?
        } else {
            id
        };
        if let Some(currency) = currency {
            let kind = currency.currency_type;
            let index = currency.index;
            let p = self
                .objects
                .get_mut(&target)
                .and_then(|o| o.player_mut())
                .unwrap();
            if kind == 0 {
                p.bag.gold = p.bag.gold.saturating_add(u64::from(count));
                self.send_gold(target);
            } else {
                let balance = p.currencies.entry(index).or_insert(0);
                *balance = balance.saturating_add(i64::from(count));
                self.send_currencies(target);
            }
        } else {
            let p = self.objects[&target].player().unwrap();
            if !p.bag.can_gain(&self.data, info, count, p.max_bag)
                || (special
                    && p.bag.inventory.iter().filter(|cell| cell.is_none()).count()
                        < count as usize)
            {
                return Err("Target has no room for that item.".into());
            }
            let made = if special {
                let def = self.data.items[&info].clone();
                (0..count)
                    .map(|_| {
                        let mut item = crate::items::UserItem::fresh(&self.data, info, 1).unwrap();
                        while item.added.is_empty() {
                            self.roll_special_bonus(&mut item, &def);
                        }
                        item
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let p = self
                .objects
                .get_mut(&target)
                .and_then(|o| o.player_mut())
                .unwrap();
            let changes = if special {
                made.into_iter()
                    .flat_map(|item| p.bag.gain_item(&self.data, item, &mut p.next_item_id))
                    .collect()
            } else {
                p.bag.gain(&self.data, info, count, &mut p.next_item_id)
            };
            self.send_changes(target, changes);
        }
        Ok(format!(
            "Made {count} {}{} for {}.",
            if special { "special " } else { "" },
            self.data.items[&info].name,
            self.player_name(target)
        ))
    }

    fn gm_give_skills(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let target = match args {
            [] => id,
            [name] => self
                .find_player(name)
                .ok_or_else(|| format!("Player {name} is not online."))?,
            _ => return Err("Usage: @giveskills [player]".into()),
        };
        let (class, level) = {
            let p = self.objects[&target].player().unwrap();
            (p.class, p.level)
        };
        let skills: Vec<(u16, u8)> = self
            .data
            .magics
            .values()
            .filter(|m| {
                m.school != 0
                    && m.school != 20
                    && m.class & class.flag() != 0
                    && m.need_level[0] <= level
            })
            .map(|m| {
                (
                    m.magic,
                    if level >= m.need_level[2] {
                        3
                    } else if level >= m.need_level[1] {
                        2
                    } else {
                        1
                    },
                )
            })
            .collect();
        let p = self
            .objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap();
        for (magic, level) in &skills {
            if let Some(existing) = p.magics.iter_mut().find(|m| m.magic == *magic) {
                existing.level = *level;
                existing.experience = 0;
            } else {
                p.magics.push(UserMagic {
                    magic: *magic,
                    level: *level,
                    experience: 0,
                    key: 0,
                    cooldown_until: 0,
                });
            }
        }
        self.send_magics(target);
        self.refresh_stats(target, false);
        self.send_player_stats(target);
        Ok(format!(
            "Granted {} class skills to {}.",
            skills.len(),
            self.player_name(target)
        ))
    }

    fn gm_level_skill(&mut self, _id: ObjectId, args: &[&str]) -> Result<String, String> {
        let [name, spell, level] = args else {
            return Err("Usage: @levelskill <player> <skill> <0..3>".into());
        };
        let target = self
            .find_player(name)
            .ok_or_else(|| format!("Player {name} is not online."))?;
        let level = level.parse::<u8>().map_err(|_| "Invalid skill level")?;
        if level > 3 {
            return Err("Skill level must be 0..3.".into());
        }
        let magic = self
            .data
            .magics
            .values()
            .find(|m| m.name.replace(' ', "").eq_ignore_ascii_case(spell))
            .map(|m| m.magic)
            .ok_or_else(|| format!("Unknown skill: {spell}"))?;
        let p = self
            .objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap();
        let skill = p
            .magics
            .iter_mut()
            .find(|m| m.magic == magic)
            .ok_or("Target has not learned that skill")?;
        skill.level = level;
        skill.experience = 0;
        self.send_to(
            target,
            ServerMessage::MagicLeveled {
                magic,
                level,
                experience: 0,
            },
        );
        self.refresh_stats(target, false);
        self.send_player_stats(target);
        Ok(format!("Set {name}'s {spell} to level {level}."))
    }

    fn gm_reset_discipline(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let target = match args {
            [] => id,
            [name] => self
                .find_player(name)
                .ok_or_else(|| format!("Player {name} is not online."))?,
            _ => return Err("Usage: @resetdiscipline [player]".into()),
        };
        let p = self
            .objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap();
        p.discipline = 0;
        p.discipline_experience = 0;
        p.magics.retain(|magic| {
            self.data
                .magics
                .get(&magic.magic)
                .is_none_or(|m| m.school != 20)
        });
        self.refresh_stats(target, false);
        self.send_player_stats(target);
        self.send_magics(target);
        self.send_discipline(target);
        Ok(format!(
            "Reset discipline for {}.",
            self.player_name(target)
        ))
    }

    fn gm_set_companion_level(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let [level] = args else {
            return Err("Usage: @setcompanionlevel <1..15>".into());
        };
        let level = level
            .parse::<i32>()
            .map_err(|_| "Invalid companion level")?;
        if !(1..=15).contains(&level) {
            return Err("Companion level must be 1..15.".into());
        }
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let active = p.active_companion.ok_or("No active companion")?;
        let companion = p
            .companions
            .iter_mut()
            .find(|c| c.index == active)
            .ok_or("No active companion")?;
        companion.level = level;
        companion.experience = 0;
        companion
            .bonuses
            .retain(|(milestone, _, _)| *milestone <= level);
        self.ensure_companion_skills(id);
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.send_companions(id);
        Ok(format!("Companion is now level {level}."))
    }

    fn gm_give_horse(&mut self, _id: ObjectId, args: &[&str]) -> Result<String, String> {
        let [name, kind] = args else {
            return Err(
                "Usage: @givehorse <player> <None|Brown|White|Red|Black|WhiteUnicorn|RedUnicorn>"
                    .into(),
            );
        };
        let target = self
            .find_player(name)
            .ok_or_else(|| format!("Player {name} is not online."))?;
        let horse = match kind.to_ascii_lowercase().as_str() {
            "none" | "0" => 0,
            "brown" | "1" => 1,
            "white" | "2" => 2,
            "red" | "3" => 3,
            "black" | "4" => 4,
            "whiteunicorn" | "5" => 5,
            "redunicorn" | "6" => 6,
            _ => return Err(format!("Unknown horse: {kind}")),
        };
        self.objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap()
            .horse = horse;
        if horse == 0 {
            self.remove_mount(target);
        }
        self.refresh_stats(target, false);
        self.send_player_stats(target);
        Ok(format!("Gave {name} horse type {horse}."))
    }

    fn gm_add_stat(
        &mut self,
        id: ObjectId,
        args: &[&str],
        source: StatSource,
    ) -> Result<String, String> {
        let [slot_name, stat_name, amount] = args else {
            return Err("Usage: @addstat <equipment slot> <stat> <amount>".into());
        };
        let slot = gm_slot(slot_name).ok_or("Unknown equipment slot")?;
        let stat = gm_stat(stat_name).ok_or("Unknown stat")?;
        let amount = gm_stat_amount(amount)?;
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let item = p
            .bag
            .equipment
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or("No item in that equipment slot")?;
        item.add_stat(stat, amount, source);
        let changed = item.instance();
        self.send_to(
            id,
            ServerMessage::ItemChanged {
                grid: Grid::Equipment,
                slot: slot as u8,
                item: Some(changed),
            },
        );
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        Ok(format!("Added {stat_name} {amount:+} to {slot_name}."))
    }

    fn gm_refine_weapon(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        match args {
            [stat, amount] => self.gm_add_stat(id, &["weapon", stat, amount], StatSource::Refine),
            _ => Err("Usage: @refineweapon <stat> <amount>. Stats: DC MC SC AC MR, MinDC/MaxDC, MinMC/MaxMC, MinSC/MaxSC.".into()),
        }
    }

    fn gm_set_hermit_stat(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let [stat_name, amount] = args else {
            return Err("Usage: @sethermitstat <stat> <amount>".into());
        };
        let stat = gm_stat(stat_name).ok_or("Unknown stat")?;
        let amount = gm_stat_amount(amount)?;
        self.objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap()
            .hermit_stats
            .insert(stat, amount);
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        Ok(format!("Set Hermit {stat_name} to {amount}."))
    }

    fn gm_change_gender(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let gender = match args {
            [] => match p.gender {
                Gender::Male => Gender::Female,
                Gender::Female => Gender::Male,
            },
            [gender] if gender.eq_ignore_ascii_case("male") => Gender::Male,
            [gender] if gender.eq_ignore_ascii_case("female") => Gender::Female,
            _ => return Err("Usage: @changegender [male|female]".into()),
        };
        if p.bag.equipment[slot::ARMOUR].is_some() {
            return Err("Take your armour off first.".into());
        }
        p.gender = gender;
        p.hair = p.hair.min(appearance::max_hair(p.class, gender));
        self.refresh_appearance(id);
        Ok(format!("Gender changed to {gender:?}."))
    }

    fn gm_set_companion_stat(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let [milestone, stat_name, amount] = args else {
            return Err("Usage: @setcompanionstat <milestone> <stat> <amount>".into());
        };
        let milestone = milestone
            .parse::<i32>()
            .map_err(|_| "Invalid companion milestone")?;
        if ![3, 5, 7, 10, 11, 13, 15].contains(&milestone) {
            return Err("Companion milestone must be 3, 5, 7, 10, 11, 13 or 15.".into());
        }
        let stat = gm_stat(stat_name).ok_or("Unknown stat")?;
        let amount = gm_stat_amount(amount)?;
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let active = p.active_companion.ok_or("No active companion")?;
        let companion = p
            .companions
            .iter_mut()
            .find(|c| c.index == active)
            .ok_or("No active companion")?;
        if companion.level < milestone {
            return Err("Companion is not high enough level.".into());
        }
        companion
            .bonuses
            .retain(|(level, _, _)| *level != milestone);
        companion.bonuses.push((milestone, stat, amount));
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.send_companions(id);
        Ok(format!(
            "Set companion level {milestone} {stat_name} to {amount}."
        ))
    }

    fn gm_castle(&self, name: &str) -> Result<(i32, String), String> {
        self.data
            .castles
            .iter()
            .find(|castle| {
                castle.index.to_string() == name
                    || castle.name.replace(' ', "").eq_ignore_ascii_case(name)
            })
            .map(|castle| (castle.index, castle.name.clone()))
            .ok_or_else(|| format!("Unknown castle: {name}"))
    }

    fn gm_start_conquest(&mut self, args: &[&str]) -> Result<String, String> {
        let [name] = args else {
            return Err("Usage: @startconquest <castle>".into());
        };
        let (castle, proper) = self.gm_castle(name)?;
        if !self.start_conquest(castle, true) {
            return Err("A conquest is already running or its map cannot load.".into());
        }
        Ok(format!("Started the conquest of {proper}."))
    }

    fn gm_end_conquest(&mut self, args: &[&str]) -> Result<String, String> {
        let [name] = args else {
            return Err("Usage: @endconquest <castle>".into());
        };
        let (castle, proper) = self.gm_castle(name)?;
        if !self
            .conquest
            .as_ref()
            .is_some_and(|war| war.castle == castle)
        {
            return Err(format!("{proper} has no active conquest."));
        }
        self.end_conquest();
        Ok(format!("Ended the conquest of {proper}."))
    }

    fn gm_take_castle(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let [name] = args else {
            return Err("Usage: @takecastle <castle>".into());
        };
        let (castle, proper) = self.gm_castle(name)?;
        let guild = self.objects[&id].player().and_then(|p| p.guild);
        if let Some(guild) = guild {
            if self
                .guild_store
                .castle_of(guild)
                .is_some_and(|held| held != castle)
            {
                return Err("Your guild already owns another castle.".into());
            }
            if self.guild_store.castle_owner(castle) == Some(guild) {
                return Ok(format!("Your guild already owns {proper}."));
            }
            self.castle_capture(castle, guild);
            Ok(format!("Your guild took {proper}."))
        } else {
            self.guild_store
                .castles
                .retain(|owner| owner.castle != castle);
            self.guild_store.save();
            self.castle_broadcast_all();
            Ok(format!("Cleared the owner of {proper}."))
        }
    }

    fn gm_toggle(&mut self, id: ObjectId, args: &[&str], mode: &str) -> Result<String, String> {
        if !args.is_empty() {
            return Err(format!("Usage: @{mode}"));
        }
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let enabled = match mode {
            "gamemaster" => {
                p.gm_mode = !p.gm_mode;
                p.gm_mode
            }
            "superman" => {
                p.superman = !p.superman;
                p.superman
            }
            _ => {
                p.observer = !p.observer;
                p.observer
            }
        };
        if mode == "observer" {
            self.update_visibility();
        }
        if mode == "superman" && enabled {
            let o = self.objects.get_mut(&id).unwrap();
            o.hp = o.max_hp;
            let p = o.player_mut().unwrap();
            p.mp = p.max_mp;
            self.send_player_stats(id);
        }
        Ok(format!(
            "{mode} {}.",
            if enabled { "enabled" } else { "disabled" }
        ))
    }

    fn gm_create_guild_command(&mut self, id: ObjectId, args: &[&str]) -> Result<String, String> {
        let (target, name) = match args {
            [name] => (id, *name),
            [player, name] => (
                self.find_player(player)
                    .ok_or_else(|| format!("Player {player} is not online."))?,
                *name,
            ),
            _ => return Err("Usage: @createguild [player] <guild name>".into()),
        };
        if !self.gm_create_guild(target, name.to_string()) {
            return Err("Could not create guild; check its name and target membership.".into());
        }
        Ok(format!(
            "Created guild {name} for {}.",
            self.player_name(target)
        ))
    }
}

fn gm_stat(name: &str) -> Option<i32> {
    use mir_formats::mirdb::stat;
    name.parse::<i32>().ok().or_else(|| {
        Some(
            match name.to_ascii_lowercase().replace(['_', '-'], "").as_str() {
                "basehealth" => stat::BASE_HEALTH,
                "basemana" => stat::BASE_MANA,
                "health" | "hp" => stat::HEALTH,
                "mana" | "mp" => stat::MANA,
                "minac" => stat::MIN_AC,
                "maxac" | "ac" => stat::MAX_AC,
                "minmr" => stat::MIN_MR,
                "maxmr" | "mr" => stat::MAX_MR,
                "mindc" => stat::MIN_DC,
                "maxdc" | "dc" => stat::MAX_DC,
                "minmc" => stat::MIN_MC,
                "maxmc" | "mc" => stat::MAX_MC,
                "minsc" => stat::MIN_SC,
                "maxsc" | "sc" => stat::MAX_SC,
                "accuracy" => stat::ACCURACY,
                "agility" => stat::AGILITY,
                "attackspeed" => stat::ATTACK_SPEED,
                "light" => stat::LIGHT,
                "focus" => stat::FOCUS,
                _ => return None,
            },
        )
    })
}

fn gm_stat_amount(text: &str) -> Result<i32, String> {
    let amount = text.parse::<i32>().map_err(|_| "Invalid amount")?;
    if !(-1_000_000..=1_000_000).contains(&amount) {
        return Err("Stat amount must be within -1000000..1000000.".into());
    }
    Ok(amount)
}

fn gm_slot(name: &str) -> Option<usize> {
    use mir_proto::slot;
    name.parse::<usize>().ok().or_else(|| {
        Some(match name.to_ascii_lowercase().as_str() {
            "weapon" => slot::WEAPON,
            "armour" | "armor" => slot::ARMOUR,
            "helmet" => slot::HELMET,
            "torch" => slot::TORCH,
            "necklace" => slot::NECKLACE,
            "braceletl" | "leftbracelet" => slot::BRACELET_L,
            "braceletr" | "rightbracelet" => slot::BRACELET_R,
            "ringl" | "leftring" => slot::RING_L,
            "ringr" | "rightring" => slot::RING_R,
            "shoes" | "boots" => slot::SHOES,
            "poison" => slot::POISON,
            "amulet" => slot::AMULET,
            "shield" => slot::SHIELD,
            _ => return None,
        })
    })
}
