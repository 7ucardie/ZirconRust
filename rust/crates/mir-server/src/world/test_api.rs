#[cfg(test)]
use super::*;

#[cfg(test)]
impl World {
    pub fn test_nearest_town_region(&self, map: i32, at: Point) -> Option<i32> {
        self.nearest_town_region(map, at, Class::Warrior)
    }

    pub fn test_roll_drop_bonus(&mut self, item: &mut UserItem) {
        let def = self.data.items[&item.info].clone();
        self.roll_drop_bonus(item, &def);
    }
    /// Test helper: move an object to a cell without validation.
    #[cfg(test)]
    pub fn teleport(&mut self, id: ObjectId, to: Point) {
        self.move_object(id, to);
    }

    #[cfg(test)]
    pub fn test_hp(&self, id: ObjectId) -> (i32, i32, bool) {
        let o = &self.objects[&id];
        (o.hp, o.max_hp, o.dead)
    }

    #[cfg(test)]
    pub fn test_set_hp(&mut self, id: ObjectId, hp: i32) {
        if let Some(o) = self.objects.get_mut(&id) {
            o.hp = hp;
        }
    }

    #[cfg(test)]
    pub fn test_kill(&mut self, id: ObjectId) {
        self.player_die(id);
    }

    #[cfg(test)]
    pub fn test_belt(&self, id: ObjectId) -> Vec<BeltLink> {
        self.objects[&id].player().unwrap().belt.clone()
    }

    #[cfg(test)]
    pub fn test_set_gold(&mut self, id: ObjectId, gold: u64) {
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.bag.gold = gold;
        }
    }

    #[cfg(test)]
    pub fn test_open_page(&mut self, id: ObjectId, page: i32) {
        let npc = self.test_open_npc(id, 0);
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.npc = Some((npc, page));
        }
    }

    #[cfg(test)]
    pub fn test_bag(&self, id: ObjectId) -> (u64, Vec<(i32, u32)>) {
        let p = self.objects[&id].player().unwrap();
        (
            p.bag.gold,
            p.bag
                .inventory
                .iter()
                .flatten()
                .map(|i| (i.info, i.count))
                .collect(),
        )
    }

    #[cfg(test)]
    pub fn test_give_item(&mut self, id: ObjectId, info: i32, count: u32) {
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            let mut next = p.next_item_id;
            p.bag.gain(&self.data, info, count, &mut next);
            p.next_item_id = next;
        }
    }

    #[cfg(test)]
    pub fn test_slot_of(&self, id: ObjectId, info: i32) -> Option<u8> {
        self.objects[&id]
            .player()?
            .bag
            .inventory
            .iter()
            .position(|s| s.as_ref().map(|i| i.info == info).unwrap_or(false))
            .map(|i| i as u8)
    }

    #[cfg(test)]
    pub fn test_magics(&self, id: ObjectId) -> Vec<u16> {
        self.objects[&id]
            .player()
            .map(|p| p.magics.iter().map(|m| m.magic).collect())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn test_magic_exp(&self, id: ObjectId, magic: u16) -> u64 {
        self.objects[&id]
            .player()
            .and_then(|p| p.magics.iter().find(|m| m.magic == magic))
            .map(|m| m.experience + m.level as u64 * 1000)
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn test_movement_cells(&self, map: i32) -> Vec<Point> {
        self.maps[&map]
            .movements
            .keys()
            .map(|(x, y)| Point::new(*x, *y))
            .collect()
    }

    /// Test helper: spawn a monster whose definition uses `ai` next to `at`.
    #[cfg(test)]
    pub fn test_spawn_ai(&mut self, ai: i32, map: i32, at: Point) -> Option<ObjectId> {
        // Lowest index so the pick does not follow HashMap order.
        let def = self
            .data
            .monsters
            .values()
            .filter(|d| d.ai == ai)
            .min_by_key(|d| d.index)?
            .index;
        Some(self.create_monster(def, map, at, None, None, 0))
    }

    /// Test helper: spawn a specific monster, optionally owned by a player.
    #[cfg(test)]
    pub fn test_spawn_monster(
        &mut self,
        def: i32,
        map: i32,
        at: Point,
        owner: Option<ObjectId>,
    ) -> ObjectId {
        self.create_monster(def, map, at, None, owner, 0)
    }

    /// Test helper: number of guards alive on a map.
    #[cfg(test)]
    pub fn test_guard_count(&self, map: i32) -> usize {
        self.on_map(map)
            .filter(|o| matches!(&o.kind, Kind::Monster(m) if m.guard) && !o.dead)
            .count()
    }

    /// Test helper: give a monster a target.
    #[cfg(test)]
    pub fn test_set_target(&mut self, id: ObjectId, target: ObjectId) {
        if let Some(m) = self.objects.get_mut(&id).and_then(|o| o.monster_mut()) {
            m.target = Some(target);
        }
    }

    /// Test helper: full mana.
    #[cfg(test)]
    pub fn test_refill_mp(&mut self, id: ObjectId) {
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.mp = p.max_mp;
        }
    }

    /// Test helpers: run NPC page actions and checks directly.
    #[cfg(test)]
    pub fn test_npc_action(&mut self, id: ObjectId, a: &crate::data::NpcActionDef) {
        self.npc_action(id, a);
    }

    #[cfg(test)]
    pub fn test_npc_check(&mut self, id: ObjectId, c: &crate::data::NpcCheckDef) -> bool {
        self.npc_check(id, c)
    }

    #[cfg(test)]
    pub fn test_gold(&self, id: ObjectId) -> u64 {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.bag.gold)
            .unwrap_or(0)
    }

    /// Test helper: put an NPC of `info` next to the player and open its
    /// dialog so quest accept/complete pass the "at the NPC" gate.
    #[cfg(test)]
    pub fn test_open_npc(&mut self, id: ObjectId, info: i32) -> ObjectId {
        let (map, loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        let npc = self.alloc_id();
        let obj = Object {
            id: npc,
            kind: Kind::Npc(NpcData {
                info,
                entry_page: 0,
            }),
            map,
            location: loc.step(Direction::Up, 1),
            direction: Direction::Down,
            hp: 1,
            max_hp: 1,
            dead: false,
            stats: CombatStats::ZERO,
            action_time: 0,
            move_time: 0,
            attack_time: 0,
            cell_time: 0,
            in_safe_zone: false,
            light: 10,
            appearance: Appearance::Npc {
                name: "Tester".into(),
                image: 0,
            },
            visible: HashSet::new(),
            poisons: Vec::new(),
            heal: None,
        };
        self.insert_object(obj);
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.npc = Some((npc, 0));
        }
        npc
    }

    #[cfg(test)]
    pub fn test_quest_kill(&mut self, id: ObjectId, monster_def: i32, map: i32) {
        self.quest_kill_progress(id, monster_def, map);
    }

    #[cfg(test)]
    pub fn test_quests(&self, id: ObjectId) -> Vec<crate::accounts::StoredQuest> {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.quests.clone())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn test_npc_quests(&self, id: ObjectId, npc: ObjectId) -> Vec<mir_proto::NpcQuest> {
        self.npc_quests(id, npc)
    }

    #[cfg(test)]
    pub fn test_currency_total(&self, id: ObjectId) -> i64 {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.currencies.values().sum())
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn test_experience(&self, id: ObjectId) -> u64 {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.experience)
            .unwrap_or(0)
    }

    /// Test helper: deal raw damage from `attacker` to `target`.
    #[cfg(test)]
    pub fn test_damage(&mut self, target: ObjectId, attacker: ObjectId, power: i32) -> i32 {
        self.damage(target, attacker, power, mir_proto::element::NONE, false)
    }

    #[cfg(test)]
    pub fn test_damage_element(
        &mut self,
        target: ObjectId,
        attacker: ObjectId,
        power: i32,
        element: u8,
    ) -> i32 {
        self.damage(target, attacker, power, element, true)
    }

    #[cfg(test)]
    pub fn test_apply_poison(&mut self, target: ObjectId, kind: u16) {
        self.apply_poison(
            target,
            Poison {
                kind,
                value: 1,
                ticks_left: 1,
                next_tick: self.now + 1_000,
                owner: None,
            },
        );
    }

    #[cfg(test)]
    pub fn test_apply_owned_poison(
        &mut self,
        target: ObjectId,
        owner: ObjectId,
        kind: u16,
        value: i32,
    ) {
        self.apply_poison(
            target,
            Poison {
                kind,
                value,
                ticks_left: 1,
                next_tick: self.now + 1_000,
                owner: Some(owner),
            },
        );
    }

    /// Test helper: face a direction without the turn delay.
    #[cfg(test)]
    pub fn test_face(&mut self, id: ObjectId, direction: mir_proto::Direction) {
        if let Some(o) = self.objects.get_mut(&id) {
            o.direction = direction;
        }
    }

    /// Test helper: is the player standing in a safe zone?
    #[cfg(test)]
    pub fn test_in_safe_zone(&self, id: ObjectId) -> bool {
        let o = &self.objects[&id];
        self.in_safe_zone(o.map, o.location)
    }

    /// Test helper: storage contents as (slot, info, count).
    #[cfg(test)]
    pub fn test_storage(&self, id: ObjectId) -> Vec<(u8, i32, u32)> {
        self.objects[&id]
            .player()
            .unwrap()
            .bag
            .storage
            .iter()
            .enumerate()
            .filter_map(|(i, it)| it.as_ref().map(|it| (i as u8, it.info, it.count)))
            .collect()
    }

    /// Test helper: nearest walkable cell to `near` outside every safe zone.
    #[cfg(test)]
    pub fn test_cell_outside_safe_zone(&self, map: i32, near: Point) -> Option<Point> {
        let file = &self.maps.get(&map)?.file;
        for r in 1..80 {
            for dx in -r..=r {
                for dy in [-r, r] {
                    for (x, y) in [(near.x + dx, near.y + dy), (near.x + dy, near.y + dx)] {
                        let p = Point::new(x, y);
                        if file.is_walkable(x, y) && !self.in_safe_zone(map, p) {
                            return Some(p);
                        }
                    }
                }
            }
        }
        None
    }

    /// Test helper: is the cell blocked for walking?
    #[cfg(test)]
    pub fn test_cell_blocked(&self, map: i32, p: Point) -> bool {
        self.cell_blocked(map, p, false)
    }

    /// Test helper: place a castle's guards, gates and flags now.
    #[cfg(test)]
    pub fn test_spawn_castle_objects(&mut self, map: i32) {
        let _ = self.ensure_map(map);
        self.spawn_castle_objects(map);
    }

    /// Test helper: castle objects on a map as (id, kind 0 guard / 1 gate /
    /// 2 flag).
    #[cfg(test)]
    pub fn test_castle_parts(&self, map: i32) -> Vec<(ObjectId, u8)> {
        use super::castle_parts::CastleRole;
        self.on_map(map)
            .filter_map(|o| match &o.kind {
                Kind::Monster(m) => m.castle.as_ref().map(|r| {
                    (
                        o.id,
                        match r {
                            CastleRole::Guard { .. } => 0,
                            CastleRole::Gate { .. } => 1,
                            CastleRole::Flag { .. } => 2,
                        },
                    )
                }),
                _ => None,
            })
            .collect()
    }

    /// Test helper: the first flag's (contester, contest_until) on a map.
    #[cfg(test)]
    pub fn test_castle_flag_state(&self, map: i32) -> Option<(Option<u32>, u64)> {
        use super::castle_parts::CastleRole;
        self.on_map(map).find_map(|o| match &o.kind {
            Kind::Monster(m) => match &m.castle {
                Some(CastleRole::Flag {
                    contester,
                    contest_until,
                    ..
                }) => Some((*contester, *contest_until)),
                _ => None,
            },
            _ => None,
        })
    }

    /// Test helper: is a cell in a safe zone?
    #[cfg(test)]
    pub fn test_in_safe_zone_at(&self, map: i32, p: Point) -> bool {
        self.in_safe_zone(map, p)
    }

    /// Test helper: apply the guild tax to a gold amount.
    #[cfg(test)]
    pub fn guild_tax_gold_test(&mut self, id: ObjectId, gold: u64) -> u64 {
        self.guild_tax_gold(id, gold)
    }

    /// Test helper: move a player to another map.
    #[cfg(test)]
    pub fn test_change_map(&mut self, id: ObjectId, map: i32, to: Point) {
        self.change_map(id, map, to);
    }

    /// Test helper: a walkable cell of `map` with an unwalkable neighbour,
    /// and the direction of that neighbour.
    #[cfg(test)]
    pub fn test_wall_spot(&mut self, map: i32) -> Option<(Point, Direction)> {
        self.ensure_map(map).ok()?;
        let file = &self.maps.get(&map)?.file;
        for y in 1..file.height as i32 - 1 {
            for x in 1..file.width as i32 - 1 {
                if !file.is_walkable(x, y) {
                    continue;
                }
                let p = Point::new(x, y);
                for d in Direction::ALL {
                    let n = p.step(d, 1);
                    if !file.is_walkable(n.x, n.y) {
                        return Some((p, d));
                    }
                }
            }
        }
        None
    }

    /// Test helper: the item info equipped in a slot.
    #[cfg(test)]
    pub fn test_equipped(&self, id: ObjectId, slot: usize) -> Option<i32> {
        self.objects[&id]
            .player()?
            .bag
            .equipment
            .get(slot)?
            .as_ref()
            .map(|i| i.info)
    }

    /// Test helper: remove every monster on a map.
    #[cfg(test)]
    pub fn test_clear_monsters(&mut self, map: i32) {
        let ids: Vec<ObjectId> = self
            .on_map(map)
            .filter(|o| o.is_monster() && !o.dead)
            .map(|o| o.id)
            .collect();
        for id in ids {
            self.remove_object(id);
        }
    }

    /// Test helper: the durability of an equipped item.
    #[cfg(test)]
    pub fn test_equipment_durability(&self, id: ObjectId, slot: usize) -> Option<i32> {
        self.objects[&id]
            .player()?
            .bag
            .equipment
            .get(slot)?
            .as_ref()
            .map(|i| i.durability)
    }

    /// Test helper: is a rubble pile (or any spell) at the cell?
    #[cfg(test)]
    pub fn test_spell_at(&self, map: i32, at: Point, effect: u8) -> bool {
        self.maps[&map]
            .objects_at(at)
            .iter()
            .any(|v| matches!(&self.objects[v].kind, Kind::Spell(s) if s.effect == effect))
    }

    /// Test helper: drop an item on the ground owned by an account.
    #[cfg(test)]
    pub fn test_drop_item(&mut self, map: i32, at: Point, info: i32, count: u32, owner: u32) {
        let def = self.data.items[&info].clone();
        let ui = crate::items::UserItem {
            id: 0,
            info,
            count,
            durability: def.durability,
            max_durability: def.durability,
            added: Vec::new(),
            level: 0,
            refinable: false,
            non_refinable: false,
            bound: false,
            worthless: false,
            locked: false,
            special_repair_until: 0,
            weapon_experience_tenths: 0,
            accessory_experience: 0,
            expires_at: None,
            colour: 0,
        };
        self.spawn_ground_item(map, at, ui, Some(owner), 0);
    }

    /// Test helper: a page's dialog type can be forced for a scenario.
    #[cfg(test)]
    pub fn test_page_with_dialog_type(&mut self, dialog_type: i32) -> i32 {
        // Lowest index so the pick does not follow HashMap order.
        if let Some(p) = self
            .data
            .npc_pages
            .values()
            .filter(|p| p.dialog_type == dialog_type)
            .min_by_key(|p| p.index)
        {
            return p.index;
        }
        let index = self.data.npc_pages.keys().max().copied().unwrap_or(0) + 1000;
        let mut page = self.data.npc_pages.values().next().unwrap().clone();
        page.index = index;
        page.dialog_type = dialog_type;
        self.data.npc_pages.insert(index, page);
        index
    }

    /// Test helper: the leader a monster is chained to.
    #[cfg(test)]
    pub fn test_monster_chained(&self, id: ObjectId) -> Option<ObjectId> {
        self.objects.get(&id)?.monster_ref().chained.map(|(l, _)| l)
    }

    /// Test helper: is the player channelling a spell?
    #[cfg(test)]
    pub fn test_channelling(&self, id: ObjectId) -> bool {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .is_some_and(|p| p.channel.is_some())
    }

    /// Test helper: a walkable cell near `near` outside safe zones with no
    /// guard within 12 cells and nothing standing on it (monster tests).
    #[cfg(test)]
    pub fn test_quiet_cell(&self, map: i32, near: Point) -> Option<Point> {
        let file = &self.maps.get(&map)?.file;
        let guards: Vec<Point> = self
            .on_map(map)
            .filter(|o| matches!(&o.kind, Kind::Monster(m) if m.guard))
            .map(|o| o.location)
            .collect();
        for r in 1..120 {
            for dx in -r..=r {
                for dy in [-r, r] {
                    for (x, y) in [(near.x + dx, near.y + dy), (near.x + dy, near.y + dx)] {
                        let p = Point::new(x, y);
                        if file.is_walkable(x, y)
                            && !self.in_safe_zone(map, p)
                            && self.maps[&map].objects_at(p).is_empty()
                            && guards.iter().all(|g| g.distance(p) > 12)
                        {
                            return Some(p);
                        }
                    }
                }
            }
        }
        None
    }

    /// Test helper: a monster's current target.
    #[cfg(test)]
    pub fn test_monster_target(&self, id: ObjectId) -> Option<ObjectId> {
        self.objects[&id].monster_ref().target
    }

    /// Test helper: make a monster cast a spell at a target now.
    #[cfg(test)]
    pub fn test_monster_cast(
        &mut self,
        id: ObjectId,
        target: ObjectId,
        spell: super::ai_profile::Spell,
    ) {
        self.monster_cast(id, target, spell);
    }

    /// Test helper: set a monster definition's behaviour flags.
    #[cfg(test)]
    pub fn test_set_behaviours(&mut self, id: ObjectId, flags: i32) {
        let def = self.objects[&id].monster_ref().def;
        if let Some(d) = self.data.monsters.get_mut(&def) {
            d.behaviours = flags;
        }
    }

    /// Test helper: a monster's current attack delay.
    #[cfg(test)]
    pub fn test_monster_attack_delay(&self, id: ObjectId) -> u64 {
        self.objects[&id].monster_ref().attack_delay
    }

    /// Test helper: is a ticking heal on the object?
    #[cfg(test)]
    pub fn test_has_heal(&self, id: ObjectId) -> bool {
        self.objects[&id].heal.is_some()
    }

    /// Test helper: purify a target as a monster would.
    #[cfg(test)]
    pub fn test_purify(&mut self, caster: ObjectId, target: ObjectId) {
        self.purify(caster, target);
    }

    /// Test helper: (hidden, mode) of a monster.
    #[cfg(test)]
    pub fn test_monster_hidden_mode(&self, id: ObjectId) -> (bool, bool) {
        let m = self.objects[&id].monster_ref();
        (m.hidden, m.mode)
    }

    /// Test helper: set a ghost's remaining revives.
    #[cfg(test)]
    pub fn test_set_revives(&mut self, id: ObjectId, n: i32) {
        if let Some(m) = self.objects.get_mut(&id).and_then(|o| o.monster_mut()) {
            m.revives_left = n;
        }
    }

    /// Test helper: (revives left, deaths) of a monster.
    #[cfg(test)]
    pub fn test_monster_revives(&self, id: ObjectId) -> (i32, i32) {
        let m = self.objects[&id].monster_ref();
        (m.revives_left, m.death_count)
    }

    /// Test helper: when a timed monster vanishes.
    #[cfg(test)]
    pub fn test_monster_despawn_at(&self, id: ObjectId) -> Option<u64> {
        self.objects[&id].monster_ref().despawn_at
    }

    /// Test helper: stamp a refine level and added stats on the equipped weapon.
    #[cfg(test)]
    pub fn test_set_weapon_refine(&mut self, id: ObjectId, level: i32, added: Vec<(i32, i32)>) {
        if let Some(w) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.bag.equipment.get_mut(mir_proto::slot::WEAPON))
            .and_then(|w| w.as_mut())
        {
            w.level = level;
            w.added = added
                .into_iter()
                .map(|(stat, amount)| crate::items::ItemStat {
                    stat,
                    amount,
                    source: crate::items::StatSource::Legacy,
                })
                .collect();
        }
    }

    /// Test helper: set an NPC currency amount by `CurrencyType`.
    #[cfg(test)]
    pub fn test_set_currency_by_type(&mut self, id: ObjectId, currency_type: i32, amount: i64) {
        let Some(idx) = self
            .data
            .currencies
            .iter()
            .find(|c| c.currency_type == currency_type)
            .map(|c| c.index)
        else {
            return;
        };
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.currencies.insert(idx, amount);
        }
        self.send_currencies(id);
    }

    /// Test helper: the held fame title index.
    #[cfg(test)]
    pub fn test_fame(&self, id: ObjectId) -> i32 {
        self.objects[&id].player().map(|p| p.fame).unwrap_or(0)
    }

    /// Test helper: pretend the running page names this script file.
    #[cfg(test)]
    pub fn test_set_script(&mut self, file: Option<&str>) {
        self.npc_script = file.map(|f| f.to_string());
    }

    /// Test helper: run a page through `npc_run_page` with a fake NPC.
    #[cfg(test)]
    pub fn test_run_page(&mut self, id: ObjectId, page: i32) {
        self.npc_run_page(id, ObjectId(0), page);
    }

    /// Test helper: give a page a script file.
    #[cfg(test)]
    pub fn test_set_page_script(&mut self, page: i32, file: &str) {
        if let Some(p) = self.data.npc_pages.get_mut(&page) {
            p.script_file = file.to_string();
        }
    }

    /// Test helper: revive a dead player where they stand.
    #[cfg(test)]
    pub fn revive_player_test(&mut self, id: ObjectId) {
        let _ = self.revive_player(id);
    }

    /// Test helper: grant experience through the normal path (castle bonus
    /// included).
    #[cfg(test)]
    pub fn gain_experience_test(&mut self, id: ObjectId, amount: u64) {
        self.gain_combat_experience(id, amount);
    }

    /// Test helper: quest/item experience follows the same rate-affected path.
    #[cfg(test)]
    pub fn gain_reward_experience_test(&mut self, id: ObjectId, amount: u64) {
        self.gain_experience(id, amount);
    }

    /// Test helper: give a guild funds.
    #[cfg(test)]
    pub fn test_set_guild_funds(&mut self, guild: u32, funds: i64) {
        if let Some(g) = self.guild_store.guilds.iter_mut().find(|g| g.id == guild) {
            g.funds = funds;
        }
    }

    /// Test helper: end every guild war at the next tick.
    #[cfg(test)]
    pub fn test_expire_wars(&mut self) {
        for w in self.guild_store.wars.iter_mut() {
            w.ends_at = 0;
        }
    }

    /// Test helper: the castle lord of the running conquest.
    #[cfg(test)]
    pub fn test_castle_lord(&self) -> Option<ObjectId> {
        self.conquest.as_ref().and_then(|c| c.lord)
    }

    /// Test helper: end the running conquest now.
    #[cfg(test)]
    pub fn test_end_conquest(&mut self) {
        self.end_conquest();
    }

    /// Test helper: set PK points directly.
    #[cfg(test)]
    pub fn test_set_pk(&mut self, id: ObjectId, points: i32) {
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.pk_points = points;
        }
        self.refresh_appearance(id);
    }

    /// Test helper: a monster's experience value.
    #[cfg(test)]
    pub fn test_monster_experience(&self, id: ObjectId) -> i64 {
        self.objects[&id].monster_ref().experience as i64
    }
}
