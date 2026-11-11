use super::*;

impl World {
    pub(super) fn spawn_monster(&mut self, map: i32, group: usize) -> bool {
        let (def_index, points) = {
            let g = &self.maps[&map].spawns[group];
            (g.def.monster, g.points.clone())
        };
        let mut location = None;
        for _ in 0..20 {
            if let Some(p) = self.random_point(&points) {
                if !self.cell_blocked(map, p, false) {
                    location = Some(p);
                    break;
                }
            }
        }
        let Some(location) = location else {
            return false;
        };
        self.create_monster(def_index, map, location, Some((map, group)), None, 0);
        self.maps.get_mut(&map).unwrap().spawns[group].alive += 1;
        true
    }

    /// Put a monster of `def` on the map; pets carry an owner and a summon level.
    pub(super) fn create_monster(
        &mut self,
        def_index: i32,
        map: i32,
        location: Point,
        spawn: Option<(i32, usize)>,
        owner: Option<ObjectId>,
        summon_level: i32,
    ) -> ObjectId {
        let def: MonsterDef = self.data.monsters[&def_index].clone();
        let (drop_set, event_chance) = spawn
            .and_then(|(map, group)| self.maps.get(&map)?.spawns.get(group))
            .map(|g| (g.def.drop_set, g.def.easter_event_chance))
            .unwrap_or((0, 0));
        let easter_event = event_chance > 0
            && self.now < self.easter_event_until
            && self.rng.random_range(0..event_chance) == 0;
        let id = self.alloc_id();
        let dir = Direction::from_index(self.rng.random_range(0..8));
        let jitter_search = self.rng.random_range(0..SEARCH_DELAY);
        let jitter_roam = self.rng.random_range(0..ROAM_DELAY);
        let owner_name = owner
            .and_then(|o| self.objects.get(&o))
            .and_then(|o| o.player())
            .map(|p| p.name.clone());
        let profile = super::ai_profile::profile(def.ai);
        let obj = Object {
            id,
            kind: Kind::Monster(MonsterData {
                def: def.index,
                drop_set,
                easter_event,
                spawn,
                target: None,
                search_time: self.now + jitter_search,
                roam_time: self.now + jitter_roam,
                dead_time: 0,
                struck_time: 0,
                regen_time: self.now + REGEN_DELAY,
                attack_delay: def.attack_delay.max(0) as u64,
                move_delay: def.move_delay.max(0) as u64,
                experience: def.experience,
                exp_owner: None,
                owner,
                shock_until: 0,
                summon_level,
                tame_until: if owner.is_some() { u64::MAX } else { 0 },
                explode_at: None,
                hidden: profile.hidden.is_some() || profile.phase.is_some() || profile.mode_windows,
                // C# Shinsu.Appear runs one second after spawn; it idles in
                // small form even before a target opens its fighting window.
                hide_check: if profile.mode_windows {
                    self.now + 1000
                } else {
                    0
                },
                range_time: 0,
                fear_time: 0,
                blink_time: 0,
                bonus: false,
                panic_used: false,
                spell_time: 0,
                stage: -1,
                despawn_at: None,
                mirror_element: None,
                link: None,
                chained: None,
                revives_left: 0,
                death_count: 0,
                revive_at: 0,
                cadence_time: 0,
                minion_time: 0,
                numa_action: None,
                mode: false,
                mode_time: 0,
                phase_time: 0,
                behaviour_heal_time: 0,
                behaviour_teleported: false,
                behaviour_enraged: false,
                rage_until: 0,
                minions: Vec::new(),
                master: None,
                guard: def.ai == -1,
                companion: None,
                castle: None,
                room_door: None,
                harvest_count: super::ai_profile::harvest_count(def.ai),
                harvested: false,
                harvest_drops: std::collections::BTreeMap::new(),
            }),
            map,
            location,
            direction: dir,
            hp: 1,
            max_hp: 1,
            dead: false,
            stats: CombatStats::ZERO,
            action_time: if profile.mode_windows {
                self.now + 2000
            } else {
                0
            },
            move_time: 0,
            attack_time: 0,
            cell_time: 0,
            in_safe_zone: false,
            light: def.stat(stat::LIGHT).clamp(0, 255) as u8,
            appearance: Appearance::Monster {
                name: def.name.clone(),
                image: if profile.mode_windows { 100 } else { def.image },
                owner: owner_name,
                // Per-viewer; `update_visibility` sets it on the way out.
                skeleton: false,
            },
            visible: HashSet::new(),
            poisons: Vec::new(),
            heal: None,
        };
        self.insert_object(obj);
        self.refresh_monster_stats(id, true);
        if let Some(o) = owner {
            if let Some(p) = self.objects.get_mut(&o).and_then(|o| o.player_mut()) {
                p.pets.push(id);
            }
        }
        // Class quirks set at spawn: ghosts get 0-3 revives, gates last 20 min.
        let prof = self.ai_profile(def.ai);
        if prof.revives || prof.gate.is_some() {
            let revives = if prof.revives {
                self.rng.random_range(0..4)
            } else {
                0
            };
            let now = self.now;
            if let Some(mm) = self.objects.get_mut(&id).and_then(|o| o.monster_mut()) {
                mm.revives_left = revives;
                if prof.gate.is_some() {
                    mm.despawn_at = Some(now + 20 * 60_000);
                }
            }
        }
        if def.ai == 115 {
            self.spawn_prophet_sorcerer(id);
        }
        id
    }

    /// Zircon `MonsterObject.RefreshStats`: base stats, +10 % per summon
    /// level, and the owner's pet DC percent.
    pub(super) fn refresh_monster_stats(&mut self, id: ObjectId, restore: bool) {
        let Some((def_index, summon_level, owner)) =
            self.objects.get(&id).and_then(|o| match &o.kind {
                Kind::Monster(m) => Some((m.def, m.summon_level, m.owner)),
                _ => None,
            })
        else {
            return;
        };
        let def = &self.data.monsters[&def_index];
        let boost = |v: i32| v + v * summon_level / 10;
        let pet_dc = owner
            .and_then(|o| self.objects.get(&o))
            .and_then(|o| o.player())
            .map(|p| p.buffs.iter().map(|b| b.stats.pet_dc_pct).sum::<i32>())
            .unwrap_or(0);
        let mut stats = CombatStats {
            accuracy: boost(def.stat(stat::ACCURACY)),
            agility: boost(def.stat(stat::AGILITY)),
            min_ac: boost(def.stat(stat::MIN_AC)),
            max_ac: boost(def.stat(stat::MAX_AC)),
            min_dc: boost(def.stat(stat::MIN_DC)),
            max_dc: boost(def.stat(stat::MAX_DC)),
            min_mr: boost(def.stat(stat::MIN_MR)),
            max_mr: boost(def.stat(stat::MAX_MR)),
            min_mc: 0,
            max_mc: 0,
            min_sc: 0,
            max_sc: 0,
            luck: def.stat(19),
            critical_chance: def.stat(55),
            critical_damage: def.stat(115),
            paralysis_chance: def.stat(122),
            slow_chance: def.stat(123),
            silence_chance: def.stat(124),
            block_chance: def.stat(stat::BLOCK_CHANCE),
            evasion_chance: def.stat(stat::EVASION_CHANCE),
            poison_resistance: def.stat(stat::POISON_RESISTANCE),
            pickup_radius: 1,
            element_attack: super::ELEMENT_ATTACK_STATS.map(|stat| {
                if stat == 0 {
                    0
                } else {
                    boost(def.stat(stat))
                }
            }),
            element_resist: super::ELEMENT_RESIST_STATS.map(|stat| def.stat(stat)),
            attack_element: 0,
        };
        stats.min_dc += stats.min_dc * pet_dc / 100;
        stats.max_dc += stats.max_dc * pet_dc / 100;
        let max_hp = boost(def.health());
        let o = self.objects.get_mut(&id).unwrap();
        o.stats = stats;
        o.max_hp = max_hp;
        o.hp = if restore { max_hp } else { o.hp.min(max_hp) };
    }

    /// Zircon `SpawnInfo.DoSpawn`, run once per second per map.
    pub(super) fn do_spawns(&mut self, map: i32) {
        let count = self.maps[&map].spawns.len();
        for gi in 0..count {
            let (due, alive, want, delay, announce) = {
                let g = &self.maps[&map].spawns[gi];
                (
                    self.now >= g.next_spawn,
                    g.alive,
                    g.def.count,
                    g.def.delay,
                    g.def.announce,
                )
            };
            if !due {
                continue;
            }
            let delay_s = (delay.max(0) as u64).min(1_000_000) * 60;
            let next = if announce {
                delay_s * 1000
            } else {
                let d = if delay_s > 0 {
                    self.rng.random_range(0..delay_s)
                } else {
                    0
                };
                (d + delay_s / 2) * 1000
            };
            self.maps.get_mut(&map).unwrap().spawns[gi].next_spawn = self.now + next.max(1000);
            for _ in alive..want {
                if !self.spawn_monster(map, gi) {
                    break;
                }
            }
        }
    }

    pub(super) fn spawn_ground_item(
        &mut self,
        map: i32,
        near: Point,
        item: UserItem,
        owner: Option<u32>,
        spread: i32,
    ) {
        if !self.maps.contains_key(&map) {
            return;
        }
        let location = if spread > 0 {
            'search: {
                let mut best = None;
                for distance in 0..=spread {
                    for dy in -distance..=distance {
                        for dx in -distance..=distance {
                            if dx.abs().max(dy.abs()) != distance {
                                continue;
                            }
                            let p = Point::new(near.x + dx, near.y + dy);
                            if self.cell_blocked(map, p, false) {
                                continue;
                            }
                            let count = self.maps[&map]
                                .objects_at(p)
                                .iter()
                                .filter(|id| self.objects[id].is_item())
                                .count();
                            if count == 0 {
                                break 'search p;
                            }
                            if best.is_none_or(|(fewest, _)| count < fewest) {
                                best = Some((count, p));
                            }
                        }
                    }
                }
                break 'search best.map_or(near, |(_, p)| p);
            }
        } else {
            near
        };
        let id = self.alloc_id();
        let appearance = Appearance::Item {
            info: item.info,
            count: item.count,
            enhanced: !item.added.is_empty(),
        };
        let obj = Object {
            id,
            kind: Kind::Item(ItemData {
                item,
                owner,
                spawn_time: self.now,
                expire: self.now + DROP_DURATION,
            }),
            map,
            location,
            direction: Direction::Down,
            hp: 0,
            max_hp: 0,
            dead: false,
            stats: CombatStats::ZERO,
            action_time: 0,
            move_time: 0,
            attack_time: 0,
            cell_time: 0,
            in_safe_zone: false,
            light: 0,
            appearance,
            visible: HashSet::new(),
            poisons: Vec::new(),
            heal: None,
        };
        self.insert_object(obj);
    }

    /// Zircon `MonsterObject.Drop`: every DropInfo row rolls `1 in Chance`.
    /// Roll the monster's drops for `killer`; `players` is the number of
    /// group members rolling (chance and gold are divided by it).
    pub(super) fn drop_loot(
        &mut self,
        monster: ObjectId,
        killer: Option<ObjectId>,
        players: usize,
    ) {
        let (map, loc, def_index, drop_set, easter_event) = {
            let o = &self.objects[&monster];
            (
                o.map,
                o.location,
                o.monster_ref().def,
                o.monster_ref().drop_set,
                o.monster_ref().easter_event,
            )
        };
        let owner = killer
            .and_then(|k| self.objects.get(&k))
            .and_then(|o| o.player())
            .map(|p| p.account);
        // Zircon `NeedHarvest`: the body keeps its drops until butchered.
        // With no owning account there is nobody to owe, so it drops as
        // usual and the corpse is bones to everyone.
        let harvest = self.needs_harvest(monster) && owner.is_some();
        if harvest {
            self.objects
                .get_mut(&monster)
                .unwrap()
                .monster_mut()
                .unwrap()
                .harvest_drops
                .entry(owner.unwrap())
                .or_default();
        }
        let drops = self
            .drops_by_monster
            .get(&def_index)
            .cloned()
            .unwrap_or_default();
        for d in drops {
            if d.chance <= 0
                || d.part_only
                || (drop_set & d.drop_set) != d.drop_set
                || (d.easter_event && !easter_event)
            {
                continue;
            }
            let Some(item) = self.data.items.get(&d.item).cloned() else {
                continue;
            };
            let mut amount = (d.amount / 2 + self.rng.random_range(0..d.amount.max(1))).max(1);
            // Zircon banks the roll's expected yield before rolling, and
            // lets a failed roll through once the expectation has run a
            // whole item ahead of what the account has actually seen.
            let players = i64::try_from(players.max(1)).unwrap_or(i64::MAX);
            let one_in = i64::from(d.chance).saturating_mul(players);
            let owed = self.add_drop_progress(killer, d.item, amount, one_in, d.part_only);
            let rate = killer
                .and_then(|id| self.objects.get(&id))
                .and_then(|o| o.player())
                .map(|p| p.drop_rate)
                .unwrap_or(0);
            let roll_span = one_in.saturating_mul(100);
            let chance = (100_i64 + i64::from(rate)).clamp(0, roll_span);
            if self.rng.random_range(0..roll_span) >= chance && !owed {
                continue;
            }
            if item.index == self.data.gold_item {
                amount = (i64::from(amount) / players).max(1) as i32;
            }
            self.add_drop_count(killer, d.item, amount as i64);
            let mut remaining = amount as u32;
            let stack = item.stack_size.max(1) as u32;
            while remaining > 0 {
                let count = remaining.min(stack);
                remaining -= count;
                let mut ui = UserItem {
                    id: 0,
                    info: item.index,
                    count,
                    durability: item.durability,
                    max_durability: item.durability,
                    added: Vec::new(),
                    level: 0,
                    refinable: false,
                    non_refinable: false,
                    bound: false,
                    worthless: false,
                    locked: false,
                    weapon_experience_tenths: 0,
                    accessory_experience: 0,
                    special_repair_until: 0,
                    expires_at: None,
                    colour: 0,
                };
                self.roll_drop_bonus(&mut ui, &item);
                ui.durability = match item.item_type {
                    item_type::WEAPON
                    | item_type::SHIELD
                    | item_type::ARMOUR
                    | item_type::HELMET
                    | item_type::NECKLACE
                    | item_type::BRACELET
                    | item_type::RING
                    | item_type::SHOES => (self.rng.random_range(0..item.durability.max(1)) + 1000)
                        .min(ui.max_durability),
                    item_type::MEAT => {
                        self.rng
                            .random_range(0..item.durability.saturating_mul(2).max(1))
                            + 2000
                    }
                    item_type::ORE => {
                        self.rng
                            .random_range(0..item.durability.saturating_mul(3).max(1))
                            + 3000
                    }
                    item_type::BOOK => self.rng.random_range(5..101),
                    _ => item.durability,
                };
                if harvest {
                    self.stash_harvest_drop(monster, owner.unwrap(), ui);
                } else if !killer.is_some_and(|id| self.companion_auto_collect(id, &ui)) {
                    self.spawn_ground_item(map, loc, ui, owner, DROP_DISTANCE);
                }
                if item.index == self.data.gold_item {
                    break;
                }
            }
        }
    }

    pub(super) fn can_roll_special(item_type: u8) -> bool {
        matches!(
            item_type,
            item_type::WEAPON
                | item_type::SHIELD
                | item_type::ARMOUR
                | item_type::HELMET
                | item_type::NECKLACE
                | item_type::BRACELET
                | item_type::RING
                | item_type::SHOES
        )
    }

    pub(super) fn roll_drop_bonus(&mut self, item: &mut UserItem, def: &crate::data::ItemDef) {
        if !Self::can_roll_special(def.item_type) {
            return;
        }
        let chance = if def.rarity == 0 { 15 } else { 30 };
        if !self.drop_specials || self.rng.random_range(0..chance) != 0 {
            return;
        }
        self.roll_special_bonus(item, def);
    }

    pub(super) fn roll_special_bonus(&mut self, item: &mut UserItem, def: &crate::data::ItemDef) {
        use crate::items::StatSource::Added;
        match def.item_type {
            item_type::WEAPON => {
                if let Some(v) = self.roll_bonus_amount(5, 50, 250) {
                    item.add_stat(stat::MAX_DC, v, Added);
                }
                if let Some(v) = self.roll_bonus_amount(5, 50, 250) {
                    Self::add_spell_bonus(item, def, v);
                }
                if let Some(v) = self.roll_bonus_amount(5, 250, 1250) {
                    item.add_stat(stat::ACCURACY, v, Added);
                }
                if let Some(v) = self.roll_bonus_amount(3, 5, 25) {
                    self.add_attack_element(item, v);
                }
            }
            item_type::SHIELD => {
                if let Some(v) = self.roll_bonus_amount(10, 50, 250) {
                    item.add_stat(stat::DC_PERCENT, v, Added);
                }
                if let Some(v) = self.roll_bonus_amount(10, 50, 250) {
                    item.add_stat(stat::MC_PERCENT, v, Added);
                    item.add_stat(stat::SC_PERCENT, v, Added);
                }
                for stat in [
                    stat::BLOCK_CHANCE,
                    stat::EVASION_CHANCE,
                    stat::POISON_RESISTANCE,
                ] {
                    if let Some(v) = self.roll_bonus_amount(10, 50, 250) {
                        item.add_stat(stat, v, Added);
                    }
                }
                self.roll_resistances(item, 2, 45, 60);
            }
            item_type::ARMOUR => {
                if let Some(v) = self.roll_bonus_amount(2, 15, 150) {
                    item.add_stat(stat::MAX_AC, v, Added);
                }
                if let Some(v) = self.roll_bonus_amount(2, 15, 150) {
                    item.add_stat(stat::MAX_MR, v, Added);
                }
                self.roll_resistances(item, 2, 45, 60);
            }
            item_type::HELMET | item_type::SHOES => {
                let (one, two) = if def.item_type == item_type::SHOES {
                    (15, 150)
                } else {
                    (25, 250)
                };
                if let Some(v) = self.roll_bonus_amount(5, one, two) {
                    item.add_stat(stat::MAX_AC, v, Added);
                }
                if let Some(v) = self.roll_bonus_amount(5, one, two) {
                    item.add_stat(stat::MAX_MR, v, Added);
                }
                self.roll_resistances(item, 1, 45, 60);
            }
            item_type::NECKLACE | item_type::BRACELET | item_type::RING => {
                if def.item_type == item_type::BRACELET {
                    if let Some(v) = self.roll_bonus_amount(5, 15, 150) {
                        item.add_stat(stat::MAX_AC, v, Added);
                    }
                    if let Some(v) = self.roll_bonus_amount(5, 15, 150) {
                        item.add_stat(stat::MAX_MR, v, Added);
                    }
                }
                if let Some(v) = self.roll_bonus_amount(5, 25, 250) {
                    item.add_stat(stat::MAX_DC, v, Added);
                }
                if let Some(v) = self.roll_bonus_amount(5, 25, 250) {
                    Self::add_spell_bonus(item, def, v);
                }
                if def.item_type == item_type::RING {
                    if let Some(v) = self.roll_bonus_amount(3, 15, 150) {
                        item.add_stat(stat::PICK_UP_RADIUS, v, Added);
                    }
                }
                if def.item_type != item_type::RING {
                    if let Some(v) = self.roll_bonus_amount(5, 25, 250) {
                        item.add_stat(stat::ACCURACY, v, Added);
                    }
                    if let Some(v) = self.roll_bonus_amount(5, 25, 250) {
                        item.add_stat(stat::AGILITY, v, Added);
                    }
                }
                if def.item_type == item_type::BRACELET {
                    self.roll_resistances(item, 1, 30, 40);
                } else if self.rng.random_range(0..3) == 0 {
                    self.add_attack_element(item, 1);
                    if self.rng.random_range(0..5) == 0 {
                        self.add_attack_element(item, 1);
                    }
                    if self.rng.random_range(0..25) == 0 {
                        self.add_attack_element(item, 1);
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn roll_bonus_amount(&mut self, chance: u32, rare_one: u32, rare_two: u32) -> Option<i32> {
        (self.rng.random_range(0..chance) == 0).then(|| {
            1 + i32::from(self.rng.random_range(0..rare_one) == 0)
                + i32::from(self.rng.random_range(0..rare_two) == 0)
        })
    }

    fn add_spell_bonus(item: &mut UserItem, def: &crate::data::ItemDef, amount: i32) {
        use crate::items::StatSource::Added;
        let mc = def.stat(stat::MIN_MC) > 0 || def.stat(stat::MAX_MC) > 0;
        let sc = def.stat(stat::MIN_SC) > 0 || def.stat(stat::MAX_SC) > 0;
        if mc || !sc {
            item.add_stat(stat::MAX_MC, amount, Added);
        }
        if sc || !mc {
            item.add_stat(stat::MAX_SC, amount, Added);
        }
    }

    fn add_attack_element(&mut self, item: &mut UserItem, amount: i32) {
        use crate::items::StatSource::Added;
        let index = self.rng.random_range(1..super::ELEMENT_ATTACK_STATS.len());
        item.add_stat(super::ELEMENT_ATTACK_STATS[index], amount, Added);
    }

    fn add_resistance(&mut self, item: &mut UserItem, available: &mut Vec<i32>, amount: i32) {
        use crate::items::StatSource::Added;
        let index = self.rng.random_range(0..available.len());
        item.add_stat(available.remove(index), amount, Added);
    }

    fn roll_resistances(&mut self, item: &mut UserItem, amount: i32, second: u32, third: u32) {
        let mut available = super::ELEMENT_RESIST_STATS.to_vec();
        if self.rng.random_range(0..10) == 0 {
            self.add_resistance(item, &mut available, amount);
            if self.rng.random_range(0..2) == 0 {
                self.add_resistance(item, &mut available, -amount);
            }
            if self.rng.random_range(0..second) == 0 {
                self.add_resistance(item, &mut available, amount);
                if self.rng.random_range(0..2) == 0 {
                    self.add_resistance(item, &mut available, -amount);
                }
                if self.rng.random_range(0..third) == 0 {
                    self.add_resistance(item, &mut available, amount);
                    if self.rng.random_range(0..2) == 0 {
                        self.add_resistance(item, &mut available, -amount);
                    }
                } else if self.rng.random_range(0..third) == 0 {
                    self.add_resistance(item, &mut available, -amount);
                }
            } else if self.rng.random_range(0..second) == 0 {
                self.add_resistance(item, &mut available, -amount);
            }
        } else if self.rng.random_range(0..10) == 0 {
            self.add_resistance(item, &mut available, -amount);
        }
    }

    /// Zircon `Map.CreateGuards`: fixed guard placements from `GuardInfo`.
    pub(super) fn spawn_guards(&mut self, map: i32) {
        let guards: Vec<(i32, Point, u8)> = self
            .data
            .guards
            .iter()
            .filter(|g| g.map == map)
            .map(|g| (g.monster, Point::new(g.x, g.y), g.direction))
            .collect();
        for (monster, at, dir) in guards {
            if !self.data.monsters.contains_key(&monster)
                || !self.maps[&map].file.is_walkable(at.x, at.y)
            {
                continue;
            }
            let id = self.create_monster(monster, map, at, None, None, 0);
            if let Some(o) = self.objects.get_mut(&id) {
                o.direction = Direction::from_index(dir);
            }
        }
    }

    // ---- NPCs ----------------------------------------------------------------------

    pub(super) fn spawn_npcs(&mut self, map: i32) {
        let width = self.maps[&map].file.width as i32;
        let npcs: Vec<(i32, i32, String, i32, i32)> = self
            .data
            .npcs
            .iter()
            .filter_map(|n| {
                let r = self.data.regions.get(&n.region)?;
                if r.map != map {
                    return None;
                }
                Some((n.index, n.region, n.name.clone(), n.image, n.entry_page))
            })
            .collect();
        for (info, region, name, image, entry_page) in npcs {
            let points: Vec<Point> = self.data.regions[&region]
                .points(width)
                .into_iter()
                .map(|(x, y)| Point::new(x, y))
                .collect();
            let Some(location) = self.random_point(&points) else {
                continue;
            };
            let id = self.alloc_id();
            let display = name.rsplit('_').next().unwrap_or(&name).to_string();
            let obj = Object {
                id,
                kind: Kind::Npc(NpcData { info, entry_page }),
                light: 10,
                map,
                location,
                direction: Direction::Up,
                hp: 1,
                max_hp: 1,
                dead: false,
                stats: CombatStats::ZERO,
                action_time: 0,
                move_time: 0,
                attack_time: 0,
                cell_time: 0,
                in_safe_zone: false,
                appearance: Appearance::Npc {
                    name: display,
                    image: image.max(0) as u16,
                },
                visible: HashSet::new(),
                poisons: Vec::new(),
                heal: None,
            };
            self.insert_object(obj);
        }
    }
}

#[cfg(test)]
mod conditional_drop_tests {
    use super::*;

    fn fixture() -> Option<(World, ObjectId, ObjectId, i32)> {
        let assets = std::path::PathBuf::from(std::env::var_os("ZIRCON_ASSETS")?);
        let data = GameData::load(assets.join("../Database/System.db")).unwrap();
        let mut world = World::new(data, assets.join("Map"), None).with_seed(42);
        let player = world
            .add_player(1, 1, &crate::accounts::test_character("ConditionalLoot"))
            .unwrap();
        world.tick(0);
        let monster = world
            .objects
            .values()
            .find(|o| o.monster().is_some_and(|m| m.spawn.is_some()))
            .unwrap()
            .id;
        let item = world.data.gold_item;
        Some((world, player, monster, item))
    }

    fn drops(world: &mut World, monster: ObjectId, item: i32, mask: i32, event: bool) {
        world.drops_by_monster.insert(
            world.objects[&monster].monster_ref().def,
            vec![DropDef {
                monster: world.objects[&monster].monster_ref().def,
                item,
                chance: 1,
                amount: 1,
                drop_set: mask,
                easter_event: event,
                part_only: false,
            }],
        );
    }

    #[test]
    fn equipment_drops_roll_start_durability_below_maximum() {
        let Some((mut world, _, monster, _)) = fixture() else {
            return;
        };
        let info = world
            .data
            .items
            .values()
            .find(|item| item.item_type == item_type::WEAPON)
            .unwrap()
            .index;
        world.data.items.get_mut(&info).unwrap().durability = 5_000;
        world.drop_specials = false;
        drops(&mut world, monster, info, 0, false);
        let map = world.objects[&monster].map;
        let mut saw_used = false;
        for _ in 0..50 {
            world.drop_loot(monster, None, 1);
            let id = world.maps[&map].objects.last().unwrap();
            let Kind::Item(dropped) = &world.objects[id].kind else {
                panic!("expected item drop");
            };
            assert_eq!(dropped.item.max_durability, 5_000);
            assert!((1_000..=5_000).contains(&dropped.item.durability));
            saw_used |= dropped.item.durability < dropped.item.max_durability;
        }
        assert!(saw_used);
    }

    #[test]
    fn conditional_drop_columns_load_from_database() {
        use mir_formats::mirdb::{MirDb, Value};
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let mut db =
            MirDb::load(std::path::PathBuf::from(assets).join("../Database/System.db")).unwrap();
        let c = db.collection_mut("RespawnInfo").unwrap();
        let mut row = c.records[0].clone();
        let index = c.index(&row);
        assert!(c.set(&mut row, "DropSet", Value::Int(i64::from(i32::MIN | 3))));
        assert!(c.set(&mut row, "EasterEventChance", Value::Int(7)));
        c.records[0] = row;
        let c = db.collection_mut("DropInfo").unwrap();
        let mut row = c.records[0].clone();
        assert!(c.set(&mut row, "DropSet", Value::Int(i64::from(i32::MIN))));
        assert!(c.set(&mut row, "EasterEvent", Value::Bool(true)));
        c.records[0] = row;
        let data = GameData::from_db(&db).unwrap();
        let spawn = data.respawns.iter().find(|r| r.index == index).unwrap();
        assert_eq!(spawn.drop_set, i32::MIN | 3);
        assert_eq!(spawn.easter_event_chance, 7);
        assert_eq!(data.drops[0].drop_set, i32::MIN);
        assert!(data.drops[0].easter_event);
    }

    #[test]
    fn conditional_rows_gate_before_random_roll_and_account_progress() {
        let Some((mut world, player, monster, item)) = fixture() else {
            return;
        };
        // All required bits must be present, including the sign bit.
        for (available, required, event_mob, event_row, eligible) in [
            (0, 0, false, false, true),
            (1, 3, false, false, false),
            (3, 1, false, false, true),
            (i32::MIN, i32::MIN, false, false, true),
            (0, i32::MIN, false, false, false),
            (-1, -1, true, true, true),
            (-1, 1, false, true, false),
            (-1, 1, true, true, true),
        ] {
            let m = world
                .objects
                .get_mut(&monster)
                .unwrap()
                .monster_mut()
                .unwrap();
            m.drop_set = available;
            m.easter_event = event_mob;
            world
                .objects
                .get_mut(&player)
                .unwrap()
                .player_mut()
                .unwrap()
                .drops
                .clear();
            drops(&mut world, monster, item, required, event_row);
            let mut expected_rng = world.rng.clone();
            world.drop_loot(monster, Some(player), 1);
            assert_eq!(
                !world.objects[&player].player().unwrap().drops.is_empty(),
                eligible
            );
            if !eligible {
                assert_eq!(
                    world.rng.random::<u64>(),
                    expected_rng.random::<u64>(),
                    "excluded rows consume no RNG"
                );
            }
        }
    }

    #[test]
    fn event_eligibility_is_rolled_at_spawn_and_survives_reload() {
        let Some((mut world, player, monster, item)) = fixture() else {
            return;
        };
        let original = &world.objects[&monster];
        let spawn = original.monster_ref().spawn.unwrap();
        let def = original.monster_ref().def;
        let location = original.location;
        world.easter_event_until = 100;
        for (now, chance, expected) in [
            (0, 1, true),
            (99, 1, true),
            (100, 1, false),
            (0, 0, false),
            (0, -1, false),
        ] {
            world.now = now;
            let g = &mut world.maps.get_mut(&spawn.0).unwrap().spawns[spawn.1];
            g.def.drop_set = i32::MIN | 3;
            g.def.easter_event_chance = chance;
            let id = world.create_monster(def, spawn.0, location, Some(spawn), None, 0);
            assert_eq!(world.objects[&id].monster_ref().easter_event, expected);
            assert_eq!(world.objects[&id].monster_ref().drop_set, i32::MIN | 3);
            world.now = 200;
            world.maps.get_mut(&spawn.0).unwrap().spawns[spawn.1]
                .def
                .drop_set = 0;
            let assets = std::path::PathBuf::from(std::env::var_os("ZIRCON_ASSETS").unwrap());
            world.reload_data(GameData::load(assets.join("../Database/System.db")).unwrap());
            assert_eq!(world.objects[&id].monster_ref().easter_event, expected);
            assert_eq!(world.objects[&id].monster_ref().drop_set, i32::MIN | 3);
            // A flagged monster still earns event loot after expiry/reload;
            // an unflagged monster never gains it retroactively.
            world
                .objects
                .get_mut(&player)
                .unwrap()
                .player_mut()
                .unwrap()
                .drops
                .clear();
            drops(&mut world, id, item, i32::MIN, true);
            world.drop_loot(id, Some(player), 1);
            assert_eq!(
                !world.objects[&player].player().unwrap().drops.is_empty(),
                expected
            );
        }
        world.now = 0;
        let summoned = world.create_monster(def, spawn.0, location, None, None, 0);
        assert!(!world.objects[&summoned].monster_ref().easter_event);
        assert_eq!(world.objects[&summoned].monster_ref().drop_set, 0);
    }

    #[test]
    fn group_harvest_rolls_share_gates_and_large_chances_do_not_overflow() {
        let Some((mut world, player, monster, item)) = fixture() else {
            return;
        };
        let def = world.objects[&monster].monster_ref().def;
        world.data.monsters.get_mut(&def).unwrap().ai = 1;
        drops(&mut world, monster, item, 1, true);
        world.drop_loot(monster, Some(player), 2);
        assert!(world.objects[&player].player().unwrap().drops.is_empty());
        assert!(world.objects[&monster]
            .monster_ref()
            .harvest_drops
            .values()
            .all(Vec::is_empty));
        let m = world
            .objects
            .get_mut(&monster)
            .unwrap()
            .monster_mut()
            .unwrap();
        m.drop_set = 1;
        m.easter_event = true;
        // An eligible group roll banks only its share of expected yield.
        world.drop_loot(monster, Some(player), 2);
        assert_eq!(
            world.objects[&player].player().unwrap().drops[0].progress,
            0.5
        );
        world.drop_loot(monster, Some(player), 2);
        assert!(!world.objects[&monster].monster_ref().harvest_drops[&1].is_empty());
        world.drops_by_monster.get_mut(&def).unwrap()[0].chance = i32::MAX;
        world.drop_loot(monster, Some(player), 3);
        let progress = world.objects[&player].player().unwrap().drops[0].progress;
        assert!(progress > 1.0 && progress < 1.000001);
    }
}

#[cfg(test)]
mod ground_item_tests {
    use super::*;

    #[test]
    fn fills_nearby_cells_before_stacking_items() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let assets = std::path::PathBuf::from(assets);
        let data = GameData::load(assets.join("../Database/System.db")).unwrap();
        let mut world = World::new(data, assets.join("Map"), None).with_seed(42);
        let player = world
            .add_player(1, 1, &crate::accounts::test_character("GroundDrop"))
            .unwrap();
        let (map, near) = (world.objects[&player].map, world.objects[&player].location);
        let free = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| Point::new(near.x + dx, near.y + dy)))
            .filter(|&p| !world.cell_blocked(map, p, false))
            .count();
        assert!(free >= 2);
        let info = world.data.gold_item;
        let item = UserItem {
            id: 0,
            info,
            count: 1,
            durability: 0,
            max_durability: 0,
            added: Vec::new(),
            level: 0,
            refinable: false,
            non_refinable: false,
            bound: false,
            worthless: false,
            locked: false,
            weapon_experience_tenths: 0,
            accessory_experience: 0,
            special_repair_until: 0,
            expires_at: None,
            colour: 0,
        };
        let mut occupied = HashSet::new();
        for _ in 0..free {
            world.spawn_ground_item(map, near, item.clone(), None, 1);
            let location = world.objects[world.maps[&map].objects.last().unwrap()].location;
            assert_ne!(location, near);
            assert!(
                occupied.insert(location),
                "items stacked before nearby cells filled"
            );
        }
        world.spawn_ground_item(map, near, item, None, 1);
        let location = world.objects[world.maps[&map].objects.last().unwrap()].location;
        assert!(
            occupied.contains(&location),
            "expected stack after nearby cells filled"
        );
    }
}
