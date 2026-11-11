//! Skills wave six: the last player magics from
//! `docs/research/skills-remaining.md` - Elemental Hurricane (channel),
//! Mirror Image, Frost Bite, Tornado, Cursed Doll, Soul Resonance, Corpse
//! Exploder, Summon Dead, Dragon Blood and Chain, plus the passives and
//! augments Shuriken, Burning, Shocked, Augment Poison Dust, Infection,
//! Magic Combustion and Chain Of Fire that ride on other paths.

use super::monster_ai::has_poison;
use super::*;
use mir_proto::magic_type as m;

/// Zircon `Globals.ShurikenLibraryWeaponShape`.
pub const SHURIKEN_SHAPE: u16 = 33;

/// A channelled spell in progress (Elemental Hurricane).
#[derive(Debug, Clone, Copy)]
pub struct Channel {
    pub magic: u16,
    pub direction: Direction,
    pub next_tick: u64,
    pub until: u64,
    pub cost: i32,
}

pub(super) fn handled(magic: u16) -> bool {
    matches!(
        magic,
        m::ELEMENTAL_HURRICANE
            | m::MIRROR_IMAGE
            | m::FROST_BITE
            | m::TORNADO
            | m::CURSED_DOLL
            | m::SOUL_RESONANCE
            | m::CORPSE_EXPLODER
            | m::SUMMON_DEAD
            | m::CHAIN
    )
}

impl World {
    /// Does the player know a magic (any level)?
    pub(super) fn knows(&self, id: ObjectId, magic: u16) -> bool {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .is_some_and(|p| p.magics.iter().any(|x| x.magic == magic))
    }

    /// Dark Stone affinity comes from the equipped item's base stats, not its bonuses.
    fn dark_stone_affinity(&self, id: ObjectId, mirror: bool) -> u8 {
        let Some(def) = self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .and_then(|p| p.bag.equipment.get(slot::AMULET)?.as_ref())
            .and_then(|it| self.data.items.get(&it.info))
            .filter(|d| d.item_type == item_type::DARK_STONE)
        else {
            return element::NONE;
        };
        let mut best = (element::NONE, 0);
        for element in 1..=if mirror { 4 } else { 7 } {
            let value = def.stat(43 + i32::from(element));
            if value > best.1 {
                best = (element, value);
                if mirror {
                    break;
                }
            }
        }
        best.0
    }

    fn ice_attack(&self, id: ObjectId) -> i32 {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return 0;
        };
        let equipment = p.bag.equipment_stats(&self.data, p.level, p.class);
        equipment.get(&22).copied().unwrap_or(0)
            + self
                .fame_stats(p.fame)
                .into_iter()
                .filter(|(stat, _)| *stat == 22)
                .map(|(_, value)| value)
                .sum::<i32>()
    }

    fn summon_spot(&mut self, map: i32, near: Point) -> Option<Point> {
        for _ in 0..25 {
            let p = Point::new(
                near.x + self.rng.random_range(-1..=1),
                near.y + self.rng.random_range(-1..=1),
            );
            if self.maps[&map].file.is_walkable(p.x, p.y) && !self.cell_blocked(map, p, false) {
                return Some(p);
            }
        }
        // The random search can miss the only free neighbouring cell.
        for dir in Direction::ALL {
            let p = near.step(dir, 1);
            if !self.cell_blocked(map, p, false) {
                return Some(p);
            }
        }
        if !self.cell_blocked(map, near, false) {
            return Some(near);
        }
        None
    }

    fn monster_with_flag(&self, flag: i32) -> Option<i32> {
        self.data
            .monsters
            .values()
            .find(|d| d.flag == flag)
            .map(|d| d.index)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn wave6_cast(
        &mut self,
        id: ObjectId,
        magic: u16,
        target: Option<ObjectId>,
        location: Point,
        direction: Direction,
        loc: Point,
        now: u64,
        pending: &mut Vec<PendingMagic>,
        targets: &mut Vec<ObjectId>,
        locations: &mut Vec<Point>,
    ) -> bool {
        let level = self.magic_level(id, magic);
        let mk = |time: u64, target: Option<ObjectId>, cell: Point| PendingMagic {
            time,
            caster: id,
            magic,
            target,
            location: cell,
            direction: Some(direction),
            primary: true,
            chain: None,
        };
        let dist_ms = |w: &World, t: ObjectId| w.objects[&t].location.distance(loc) as u64 * 48;
        let map = self.objects[&id].map;
        match magic {
            m::ELEMENTAL_HURRICANE => {
                // C# starts without payment; stopping costs Mana * Cost / 1000.
                let (cost, max_mp) = {
                    let p = self.objects[&id].player().unwrap();
                    let def = &self.data.magics[&magic];
                    let um = p.magics.iter().find(|x| x.magic == magic).unwrap();
                    (um.cost(def), p.max_mp)
                };
                let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
                if p.channel.take().is_some() {
                    p.mp = (p.mp - max_mp * cost / 1000).max(0);
                    return true;
                }
                p.channel = Some(Channel {
                    magic,
                    direction,
                    next_tick: now + 500,
                    until: u64::MAX,
                    cost,
                });
                locations.push(loc.step(direction, 1));
            }
            m::MIRROR_IMAGE => {
                if location.distance(loc) > MAGIC_RANGE {
                    return false;
                }
                locations.push(location);
                pending.push(mk(now + 500, None, location));
            }
            m::FROST_BITE => pending.push(mk(now + 500, None, loc)),
            m::TORNADO => {
                if location.distance(loc) > MAGIC_RANGE {
                    return false;
                }
                locations.push(location);
                pending.push(mk(now + 500, None, location));
            }
            m::CURSED_DOLL => {
                let clevel = self.level_of(&self.objects[&id]);
                let Some(t) = target.filter(|t| {
                    let o = &self.objects[t];
                    self.objects[&id].hostile_to(o)
                        && self.level_of(o) <= clevel + 2
                        && !o
                            .monster()
                            .is_some_and(|m| self.data.monsters[&m.def].is_boss)
                        && !self
                            .objects
                            .values()
                            .any(|o| o.monster().is_some_and(|m| m.link == Some(*t)))
                }) else {
                    locations.push(location);
                    return false;
                };
                if self.use_amulet_shape(id, 1, 1).is_none() {
                    return false;
                }
                targets.push(t);
                pending.push(mk(now + 500 + dist_ms(self, t), Some(t), location));
            }
            m::SOUL_RESONANCE => {
                let members = self.group_members(id);
                let Some(t) = target.filter(|t| {
                    *t != id
                        && members.contains(t)
                        && self
                            .objects
                            .get(t)
                            .is_some_and(|o| !o.dead && !o.has_buff(buff_type::SOUL_RESONANCE))
                }) else {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: "Soul Resonance links you with a group member.".into(),
                        },
                    );
                    return false;
                };
                targets.push(t);
                pending.push(mk(now + 500, Some(t), location));
            }
            m::CORPSE_EXPLODER | m::SUMMON_DEAD => {
                let Some(t) =
                    target.filter(|t| self.objects[t].is_monster() && self.objects[t].dead)
                else {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: "That needs a corpse.".into(),
                        },
                    );
                    return false;
                };
                let amulets = if magic == m::SUMMON_DEAD { 10 } else { 2 };
                if self
                    .use_amulet_shape(id, amulets, i32::from(magic == m::SUMMON_DEAD))
                    .is_none()
                {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: format!("You need {amulets} amulets."),
                        },
                    );
                    return false;
                }
                targets.push(t);
                let delay = if magic == m::SUMMON_DEAD {
                    800
                } else {
                    1000 + dist_ms(self, t)
                };
                pending.push(mk(now + delay, Some(t), self.objects[&t].location));
            }
            m::CHAIN => {
                let Some(t) =
                    target.filter(|t| self.objects[t].is_monster() && !self.objects[t].dead)
                else {
                    locations.push(location);
                    return false;
                };
                targets.push(t);
                pending.push(mk(now + 500 + dist_ms(self, t), Some(t), location));
            }
            _ => return false,
        }
        let _ = (level, map);
        true
    }

    pub(super) fn wave6_land(&mut self, pm: &PendingMagic, cmap: i32, cloc: Point) {
        let caster = pm.caster;
        let level = self.magic_level(caster, pm.magic);
        let (pmin, pmax, _) = self.caster_power(caster, pm.magic);
        let cs = self.objects[&caster].stats;
        let power = self.roll_range(pmin, pmax);
        let sc = self.roll_lucky(cs.min_sc, cs.max_sc, cs.luck);
        let now = self.now;
        match pm.magic {
            m::MIRROR_IMAGE => {
                let affinity = self.dark_stone_affinity(caster, true);
                if affinity == element::NONE
                    || self.objects.values().any(|o| {
                        o.monster().is_some_and(|m| {
                            m.owner == Some(caster) && self.data.monsters[&m.def].flag == 7
                        })
                    })
                {
                    return;
                }
                let Some(def) = self.monster_with_flag(7) else {
                    return;
                };
                let spot = self.summon_spot(cmap, pm.location).unwrap_or(cloc);
                let decoy = self.create_monster(def, cmap, spot, None, Some(caster), 0);
                if let Some(mm) = self.objects.get_mut(&decoy).and_then(|o| o.monster_mut()) {
                    mm.despawn_at = Some(now + (level.max(1) * 5) as u64 * 1000);
                    mm.mirror_element = Some(affinity);
                }
                self.level_magic(caster, pm.magic);
            }
            m::FROST_BITE => {
                if self.objects[&caster].has_buff(buff_type::FROST_BITE) {
                    return;
                }
                let secs = 3 + 3 * level;
                let mc = self.roll_lucky(cs.min_mc, cs.max_mc, cs.luck);
                let ice = self.ice_attack(caster);
                self.buff_add(
                    caster,
                    buff_type::FROST_BITE,
                    secs as u64 * 1000,
                    BuffStats {
                        frost: mc + power + ice * 2,
                        frost_chance: 5 + 5 * level,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::TORNADO => {
                let Some(def) = self.monster_with_flag(8) else {
                    return;
                };
                let spot = self.summon_spot(cmap, pm.location).unwrap_or(cloc);
                let tornado = self.create_monster(def, cmap, spot, None, Some(caster), level * 2);
                let (hp, ac, mr, mc) = {
                    let c = &self.objects[&caster];
                    let l = level.max(1);
                    (
                        (c.max_hp / 10 * l).max(1),
                        c.stats.max_ac / 10 * l,
                        c.stats.max_mr / 10 * l,
                        c.stats.max_mc / 10 * l,
                    )
                };
                if let Some(o) = self.objects.get_mut(&tornado) {
                    o.max_hp = hp;
                    o.hp = hp;
                    o.stats.max_ac = o.stats.max_ac.max(ac);
                    o.stats.max_mr = o.stats.max_mr.max(mr);
                    o.stats.max_dc = o.stats.max_dc.max(mc);
                    if let Some(mm) = o.monster_mut() {
                        mm.despawn_at = Some(now + 10_000);
                    }
                }
                self.level_magic(caster, pm.magic);
            }
            m::CURSED_DOLL => {
                let Some(t) = pm.target.filter(|t| !self.objects[t].dead) else {
                    return;
                };
                let Some(def) = self.monster_with_flag(5) else {
                    return;
                };
                let spot = self.summon_spot(cmap, cloc).unwrap_or(cloc);
                let doll = self.create_monster(def, cmap, spot, None, Some(caster), 0);
                if let Some(mm) = self.objects.get_mut(&doll).and_then(|o| o.monster_mut()) {
                    mm.despawn_at = Some(now + (10 + 5 * level) as u64 * 1000);
                    mm.link = Some(t);
                }
                self.level_magic(caster, pm.magic);
            }
            m::SOUL_RESONANCE => {
                let Some(t) = pm.target.filter(|t| {
                    *t != caster
                        && self.group_members(caster).contains(t)
                        && self
                            .objects
                            .get(t)
                            .is_some_and(|o| !o.dead && !o.has_buff(buff_type::SOUL_RESONANCE))
                }) else {
                    return;
                };
                self.buff_remove(caster, buff_type::SOUL_RESONANCE);
                for (a, b) in [(caster, t), (t, caster)] {
                    if let Some(p) = self.objects.get_mut(&a).and_then(|o| o.player_mut()) {
                        p.soul_link = Some(b);
                    }
                    self.buff_add(
                        a,
                        buff_type::SOUL_RESONANCE,
                        u64::MAX,
                        BuffStats {
                            hp_pct: if a == t { power } else { 0 },
                            ..BuffStats::default()
                        },
                    );
                }
                self.level_magic(caster, pm.magic);
            }
            m::CORPSE_EXPLODER => {
                let Some(t) = pm
                    .target
                    .filter(|t| self.objects.get(t).is_some_and(|o| o.dead))
                else {
                    return;
                };
                let at = self.objects[&t].location;
                let victims: Vec<ObjectId> = self
                    .on_map(cmap)
                    .filter(|o| {
                        !o.dead
                            && o.location.distance(at) <= 1
                            && self.objects[&caster].hostile_to(o)
                    })
                    .map(|o| o.id)
                    .collect();
                self.remove_object(t);
                for v in victims {
                    let damage = power + sc;
                    let ts = self.objects[&v].stats;
                    let dealt = damage - self.roll_range(ts.min_mr, ts.max_mr);
                    if dealt > 0 {
                        self.damage(v, caster, dealt, element::DARK, true);
                    }
                }
                self.level_magic(caster, pm.magic);
            }
            m::SUMMON_DEAD => {
                let Some(t) = pm
                    .target
                    .filter(|t| self.objects.get(t).is_some_and(|o| o.dead))
                else {
                    return;
                };
                let at = self.objects[&t].location;
                self.remove_object(t);
                // Success: Random(MagicMaxLevel + 1) <= Level.
                if self
                    .rng
                    .random_range(0..=i32::from(crate::magic::MAGIC_MAX_LEVEL))
                    > level
                {
                    self.send_to(
                        caster,
                        ServerMessage::Chat {
                            text: "The corpse does not rise.".into(),
                        },
                    );
                    return;
                }
                let Some(def) = self
                    .data
                    .monsters
                    .values()
                    .find(|d| d.name.to_ascii_lowercase().replace(' ', "") == "undeadsoul")
                    .map(|d| d.index)
                else {
                    return;
                };
                let pets = self.objects[&caster]
                    .player()
                    .map(|p| p.pets.len())
                    .unwrap_or(0);
                if pets >= 2 {
                    self.send_to(
                        caster,
                        ServerMessage::Chat {
                            text: "You cannot control more summons.".into(),
                        },
                    );
                    return;
                }
                let spot = self.summon_spot(cmap, at).unwrap_or(at);
                self.create_monster(def, cmap, spot, None, Some(caster), level * 2);
                self.level_magic(caster, pm.magic);
            }
            m::CHAIN => {
                let Some(t) = pm.target.filter(|t| !self.objects[t].dead) else {
                    return;
                };
                let at = self.objects[&t].location;
                let max = (5 + 2 * level) as usize;
                let followers: Vec<ObjectId> = self
                    .on_map(cmap)
                    .filter(|o| {
                        o.id != t
                            && o.is_monster()
                            && !o.dead
                            && o.location.distance(at) <= 2
                            && self.objects[&caster].hostile_to(o)
                    })
                    .map(|o| o.id)
                    .take(max)
                    .collect();
                let until = now + power.max(1) as u64 * 1000;
                for f in followers {
                    if let Some(mm) = self.objects.get_mut(&f).and_then(|o| o.monster_mut()) {
                        mm.chained = Some((t, until));
                    }
                }
                // Chain Of Fire: the tether explodes around the leader.
                if self.knows(caster, m::CHAIN_OF_FIRE) {
                    let (amin, amax, _) = self.caster_power(caster, m::CHAIN_OF_FIRE);
                    let aug = self.roll_range(amin, amax);
                    let mc = self.roll_lucky(cs.min_mc, cs.max_mc, cs.luck);
                    let victims: Vec<ObjectId> = self
                        .on_map(cmap)
                        .filter(|o| {
                            !o.dead
                                && o.location.distance(at) <= 2
                                && self.objects[&caster].hostile_to(o)
                        })
                        .map(|o| o.id)
                        .collect();
                    for v in victims {
                        let ts = self.objects[&v].stats;
                        let dealt = mc * aug / 100 - self.roll_range(ts.min_mr, ts.max_mr);
                        if dealt > 0 {
                            self.damage(v, caster, dealt, element::FIRE, true);
                        }
                    }
                }
                self.level_magic(caster, pm.magic);
            }
            _ => {}
        }
    }

    // ---- hooks ---------------------------------------------------------

    /// Stop a channelled spell (move, turn, struck, or out of mana).
    pub(super) fn channel_cancel(&mut self, id: ObjectId) {
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.channel = None;
        }
    }

    /// Elemental Hurricane ticks: an 8-cell beam with weaker flanks every
    /// 500 ms, each tick costing the spell's mana.
    pub(super) fn process_channels(&mut self) {
        let now = self.now;
        let ids: Vec<ObjectId> = self
            .players()
            .filter(|o| o.player().is_some_and(|p| p.channel.is_some()))
            .map(|o| o.id)
            .collect();
        for id in ids {
            let (ch, map, loc, dead) = {
                let o = &self.objects[&id];
                (
                    o.player().unwrap().channel.unwrap(),
                    o.map,
                    o.location,
                    o.dead,
                )
            };
            if dead || now >= ch.until {
                self.channel_cancel(id);
                continue;
            }
            if now < ch.next_tick {
                continue;
            }
            {
                let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
                if p.mp < ch.cost {
                    p.channel = None;
                    continue;
                }
                p.mp -= ch.cost;
                if let Some(c) = p.channel.as_mut() {
                    c.next_tick = now + 500;
                }
            }
            self.send_player_stats(id);
            let (l, r) = if ch.direction.index().is_multiple_of(2) {
                (2i8, -2i8)
            } else {
                (1, -1)
            };
            let mut cells: Vec<(Point, i32)> = Vec::new();
            for i in 1..=8 {
                let c = loc.step(ch.direction, i);
                if !self.maps[&map].file.is_walkable(c.x, c.y) {
                    break;
                }
                cells.push((c, 100));
                cells.push((c.step(ch.direction.rotate(l), 1), 30));
                cells.push((c.step(ch.direction.rotate(r), 1), 30));
            }
            let mut hit: Vec<ObjectId> = Vec::new();
            for (cell, scale) in cells {
                let victims: Vec<ObjectId> = self.maps[&map]
                    .objects_at(cell)
                    .iter()
                    .copied()
                    .filter(|v| !hit.contains(v) && self.objects[&id].hostile_to(&self.objects[v]))
                    .collect();
                for v in victims {
                    hit.push(v);
                    let element = self.dark_stone_affinity(id, false);
                    self.magic_attack(id, v, ch.magic, element, scale);
                }
            }
            if !hit.is_empty() {
                self.level_magic(id, ch.magic);
            }
        }
    }

    /// Mirror Image uses the owner's MC, with C#'s Ice damage for every affinity.
    pub(super) fn process_mirror_image(&mut self, id: ObjectId) {
        let object = &self.objects[&id];
        let monster = object.monster_ref();
        let (map, loc, affinity, owner, ready, current, view) = (
            object.map,
            object.location,
            monster.mirror_element.unwrap(),
            monster.owner,
            monster.range_time,
            monster.target,
            self.data.monsters[&monster.def].view_range,
        );
        let Some(owner) = owner.filter(|owner| self.objects.get(owner).is_some_and(|o| !o.dead))
        else {
            self.despawn_summon(id);
            return;
        };
        if self.now < ready {
            return;
        }
        let valid = |o: &&Object| {
            o.map == map
                && o.id != id
                && o.location != loc
                && o.location.distance(loc) <= view
                && self.objects[&id].hostile_to(o)
        };
        let target = current
            .and_then(|target| self.objects.get(&target))
            .filter(valid)
            .or_else(|| self.on_map(map).find(valid))
            .map(|o| (o.id, o.location));
        let Some((target, at)) = target else {
            return;
        };
        let magic = match affinity {
            element::FIRE => m::FIRE_STORM,
            element::ICE => m::ICE_STORM,
            element::LIGHTNING => m::CHAIN_LIGHTNING,
            _ => m::DRAGON_TORNADO,
        };
        let direction = Direction::from_points(loc, at);
        let stats = self.objects[&owner].stats;
        let power = self.roll_lucky(stats.min_mc, stats.max_mc, stats.luck);
        let victims = self
            .on_map(map)
            .filter(|o| o.location.distance(at) <= 2 && self.objects[&id].hostile_to(o))
            .map(|o| o.id)
            .collect();
        let object = self.objects.get_mut(&id).unwrap();
        object.direction = direction;
        let monster = object.monster_mut().unwrap();
        monster.target = Some(target);
        monster.range_time = self.now + 1500;
        self.events.push((
            id,
            ServerMessage::ObjectMagic {
                id,
                direction,
                location: loc,
                magic,
                targets: Vec::new(),
                locations: vec![at],
                cast: true,
            },
        ));
        self.pending_monster_spells.push(PendingMonsterSpell {
            time: self.now + 500,
            caster: id,
            targets: victims,
            cells: Vec::new(),
            power,
            element: element::ICE,
            map,
        });
    }

    /// Frost Bite banks the damage its bearer takes.
    pub(super) fn frost_store(&mut self, target: ObjectId, attacker: ObjectId, amount: i32) {
        if let Some(b) = self
            .objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.buffs.iter_mut().find(|b| b.kind == buff_type::FROST_BITE))
        {
            b.stats.frost += amount;
            let chance = b.stats.frost_chance;
            let rate = if self.level_of(&self.objects[&attacker]) >= 250 {
                2000
            } else {
                200
            };
            if self.rng.random_range(0..rate) < chance {
                self.apply_poison(
                    attacker,
                    Poison {
                        kind: poison_kind::SLOW,
                        value: 0,
                        ticks_left: 5,
                        next_tick: self.now + 2000,
                        owner: Some(target),
                    },
                );
            }
        }
    }

    /// When Frost Bite ends the banked damage bursts on monsters within 3.
    pub(super) fn frost_bite_burst(&mut self, id: ObjectId, stored: i32) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let (map, loc, cap) = (
            o.map,
            o.location,
            o.stats.max_mc * 50 + self.ice_attack(id) * 70,
        );
        let amount = stored.min(cap.max(1));
        if amount <= 0 {
            return;
        }
        let victims: Vec<ObjectId> = self
            .on_map(map)
            .filter(|v| {
                v.monster()
                    .is_some_and(|m| !self.data.monsters[&m.def].is_boss)
                    && o.hostile_to(v)
                    && v.location.distance(loc) <= 3
            })
            .map(|v| v.id)
            .collect();
        for v in victims {
            self.damage(v, id, amount, element::ICE, true);
        }
    }

    /// Consume the selected charge, then roll a new charge using equipped green poison.
    pub(super) fn dragon_blood_attack(&mut self, id: ObjectId, selected: Option<u16>) -> bool {
        if !self.knows(id, m::DRAGON_BLOOD)
            || self.objects[&id].player().unwrap().level
                < self.data.magics[&m::DRAGON_BLOOD].need_level[0]
        {
            return false;
        }
        let used =
            selected == Some(m::DRAGON_BLOOD) && self.objects[&id].player().unwrap().dragon_blood;
        if used {
            self.objects
                .get_mut(&id)
                .unwrap()
                .player_mut()
                .unwrap()
                .dragon_blood = false;
            self.send_to(
                id,
                ServerMessage::MagicToggle {
                    magic: m::DRAGON_BLOOD,
                    on: false,
                },
            );
        }
        if !self.objects[&id].player().unwrap().dragon_blood && self.rng.random_range(0..5) == 0 {
            let poison = self.objects[&id].player().unwrap().bag.equipment[slot::POISON].as_ref();
            let valid = poison.is_some_and(|it| {
                it.count > 0
                    && self
                        .data
                        .items
                        .get(&it.info)
                        .is_some_and(|d| d.item_type == item_type::POISON && d.shape == 2)
            });
            if valid {
                if self.rng.random_range(0..10) > 0 {
                    let change = self
                        .objects
                        .get_mut(&id)
                        .unwrap()
                        .player_mut()
                        .unwrap()
                        .bag
                        .take(Grid::Equipment, slot::POISON as u8, 1);
                    self.send_changes(id, change.into_iter().collect());
                    self.refresh_stats(id, false);
                }
                self.objects
                    .get_mut(&id)
                    .unwrap()
                    .player_mut()
                    .unwrap()
                    .dragon_blood = true;
                self.send_to(
                    id,
                    ServerMessage::MagicToggle {
                        magic: m::DRAGON_BLOOD,
                        on: true,
                    },
                );
            }
        }
        used
    }

    /// Melee hits by players: Dragon Blood poisons, Magic Combustion drains
    /// a player's mana.
    pub(super) fn melee_landed(
        &mut self,
        attacker: ObjectId,
        target: ObjectId,
        dealt: i32,
        dragon_blood: bool,
    ) {
        let _ = dealt;
        let stats = self.objects[&attacker].stats;
        let poison_rate = if self.objects[&attacker]
            .player()
            .is_some_and(|p| p.level >= 250)
        {
            2000
        } else {
            200
        };
        for (chance, kind, duration) in [
            (stats.paralysis_chance, poison_kind::PARALYSIS, 3000),
            (stats.slow_chance, poison_kind::SLOW, 5000),
            (stats.silence_chance, poison_kind::SILENCED, 5000),
        ] {
            if chance > 0
                && (kind != poison_kind::SLOW || !self.objects[&target].is_player())
                && self.rng.random_range(0..poison_rate) < chance
            {
                self.apply_poison(
                    target,
                    Poison {
                        kind,
                        value: if kind == poison_kind::SLOW { 20 } else { 0 },
                        ticks_left: 0,
                        next_tick: self.now + duration,
                        owner: Some(attacker),
                    },
                );
            }
        }
        if dragon_blood {
            let (pmin, pmax, _) = self.caster_power(attacker, m::DRAGON_BLOOD);
            let power = self.roll_range(pmin, pmax);
            let cs = self.objects[&attacker].stats;
            let sp = self.roll_lucky(cs.min_mc.min(cs.min_sc), cs.max_mc.min(cs.max_sc), cs.luck);
            let value = (sp * power / 100).max(1);
            let existing = self.objects[&target]
                .poisons
                .iter()
                .find(|p| p.kind == poison_kind::GREEN)
                .map(|p| p.value)
                .unwrap_or(0);
            self.apply_poison(
                target,
                Poison {
                    kind: poison_kind::GREEN,
                    value: (existing + value).min(value * 4),
                    ticks_left: 10,
                    next_tick: self.now + 2000,
                    owner: Some(attacker),
                },
            );
            self.level_magic(attacker, m::DRAGON_BLOOD);
        }
        if self.objects[&target].is_player() && self.knows(attacker, m::MAGIC_COMBUSTION) {
            let (pmin, pmax, _) = self.caster_power(attacker, m::MAGIC_COMBUSTION);
            let power = self.roll_range(pmin, pmax);
            let cs = self.objects[&attacker].stats;
            let dc = self.roll_dc(cs);
            let drain = dc * power / 100;
            if drain > 0 {
                if let Some(p) = self.objects.get_mut(&target).and_then(|o| o.player_mut()) {
                    p.mp = (p.mp - drain).max(0);
                }
                self.send_player_stats(target);
            }
        }
    }

    /// Shuriken: with a shuriken weapon the swing becomes a thrown hit on
    /// the first enemy along the facing line (up to 8 cells).
    pub(super) fn shuriken_attack(
        &mut self,
        id: ObjectId,
        direction: Direction,
        map: i32,
        loc: Point,
        stats: CombatStats,
    ) -> bool {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return false;
        };
        if !p.magics.iter().any(|x| x.magic == m::SHURIKEN) {
            return false;
        }
        let shape = p.bag.equipped_shape(&self.data, slot::WEAPON).unwrap_or(-1);
        if shape != SHURIKEN_SHAPE as i32 {
            return false;
        }
        let mut target = None;
        for i in 1..=8 {
            let cell = loc.step(direction, i);
            if let Some(t) = self.maps[&map]
                .objects_at(cell)
                .iter()
                .copied()
                .find(|t| self.objects[&id].hostile_to(&self.objects[t]))
            {
                target = Some((t, i as u64));
                break;
            }
            if !self.maps[&map].file.is_walkable(cell.x, cell.y) {
                break;
            }
        }
        let Some((t, dist)) = target else {
            return false;
        };
        let power = self.roll_dc(stats);
        let tloc = self.objects[&t].location;
        self.events.push((
            id,
            ServerMessage::ObjectRangeAttack {
                id,
                direction,
                target: Some(t),
                location: tloc,
                magic: m::SHURIKEN,
            },
        ));
        self.pending_hits.push(PendingHit {
            time: self.now + (dist * 50).clamp(100, 750),
            attacker: id,
            target_cell: (map, tloc),
            power,
            target: Some(t),
            magics: vec![m::SHURIKEN],
            primary: true,
            raw: false,
            element: element::NONE,
            ranged: true,
        });
        true
    }

    /// Burning / Shocked augments: a fire or lightning spell hit sometimes
    /// carries the augment's power on top.
    pub(super) fn burn_shock_bonus(&mut self, attacker: ObjectId, elem: u8) -> i32 {
        let aug = match elem {
            element::FIRE => m::BURNING,
            element::LIGHTNING => m::SHOCKED,
            _ => return 0,
        };
        if !self.knows(attacker, aug) {
            return 0;
        }
        let level = self.magic_level(attacker, aug);
        if self
            .rng
            .random_range(0..i32::from(crate::magic::MAGIC_MAX_LEVEL))
            > level
        {
            return 0;
        }
        let (pmin, pmax, _) = self.caster_power(attacker, aug);
        self.roll_range(pmin, pmax)
    }

    /// Clear both halves before removing buffs, so removal cannot leave a hidden death link.
    pub(super) fn soul_resonance_remove(&mut self, id: ObjectId) {
        let partner = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.soul_link.take());
        let partner = partner.filter(|partner| {
            self.objects
                .get(partner)
                .and_then(|o| o.player())
                .is_some_and(|p| p.soul_link == Some(id))
        });
        if let Some(partner) = partner {
            self.objects
                .get_mut(&partner)
                .unwrap()
                .player_mut()
                .unwrap()
                .soul_link = None;
        }
        self.buff_remove(id, buff_type::SOUL_RESONANCE);
        if let Some(partner) = partner {
            self.buff_remove(partner, buff_type::SOUL_RESONANCE);
        }
    }

    /// Soul Resonance: when one linked player dies, so does the other.
    pub(super) fn soul_resonance_death(&mut self, id: ObjectId) {
        let Some(partner) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.soul_link.take())
        else {
            return;
        };
        let linked_back = self
            .objects
            .get(&partner)
            .and_then(|o| o.player())
            .is_some_and(|p| p.soul_link == Some(id));
        if !linked_back || self.objects[&partner].dead {
            return;
        }
        if let Some(p) = self.objects.get_mut(&partner).and_then(|o| o.player_mut()) {
            p.soul_link = None;
        }
        self.buff_remove(id, buff_type::SOUL_RESONANCE);
        self.buff_remove(partner, buff_type::SOUL_RESONANCE);
        self.send_to(
            partner,
            ServerMessage::Chat {
                text: "Your resonating soul follows your partner into death.".into(),
            },
        );
        self.objects.get_mut(&partner).unwrap().hp = 0;
        self.player_die(partner);
    }

    /// A cursed doll forwards what it suffers to its victim.
    pub(super) fn doll_link(&self, id: ObjectId) -> Option<ObjectId> {
        let o = self.objects.get(&id)?;
        let mm = match &o.kind {
            Kind::Monster(mm) => mm,
            _ => return None,
        };
        let victim = mm.link?;
        let v = self.objects.get(&victim)?;
        (!v.dead && !matches!(&v.kind, Kind::Monster(x) if x.link.is_some())).then_some(victim)
    }

    /// Timed summons vanish; they leave their owner's pet list too.
    pub(super) fn despawn_summon(&mut self, id: ObjectId) {
        if let Some(owner) = self.objects.get(&id).and_then(|o| o.monster_ref().owner) {
            if let Some(p) = self.objects.get_mut(&owner).and_then(|o| o.player_mut()) {
                p.pets.retain(|x| *x != id);
            }
        }
        self.remove_object(id);
    }

    /// Infection: a Parasite tick spreads to one monster next to the victim.
    pub(super) fn infection_spread(&mut self, victim: ObjectId, owner: ObjectId, value: i32) {
        if !self.knows(owner, m::INFECTION) {
            return;
        }
        let level = self.magic_level(owner, m::INFECTION);
        let (pmin, pmax, _) = self.caster_power(owner, m::INFECTION);
        let ticks = self.roll_range(pmin, pmax).max(1);
        let Some(vo) = self.objects.get(&victim) else {
            return;
        };
        let (map, loc) = (vo.map, vo.location);
        let next = self
            .on_map(map)
            .find(|o| {
                o.id != victim
                    && o.is_monster()
                    && !o.dead
                    && o.location.distance(loc) <= 1
                    && !has_poison(o, poison_kind::PARASITE)
            })
            .map(|o| o.id);
        if let Some(n) = next {
            self.apply_poison(
                n,
                Poison {
                    kind: poison_kind::PARASITE,
                    value: (value * level + 1) / 10,
                    ticks_left: ticks,
                    next_tick: self.now + 2000,
                    owner: Some(owner),
                },
            );
        }
    }
}

#[cfg(test)]
mod targeting_tests {
    use super::*;

    fn fixture() -> Option<(World, ObjectId, i32, Point, i32)> {
        let assets = std::path::PathBuf::from(std::env::var_os("ZIRCON_ASSETS")?);
        let data = GameData::load(assets.join("../Database/System.db")).unwrap();
        let mut world = World::new(data, assets.join("Map"), None).with_seed(42);
        let mut record = crate::accounts::test_character("SpellTargets");
        record.level = 90;
        let player = world.add_player(1, 1, &record).unwrap();
        let map = world.objects[&player].map;
        let loc = world.objects[&player].location;
        let def = world
            .data
            .monsters
            .values()
            .find(|m| m.ai == 0 && !m.is_boss && m.level <= 92)
            .unwrap()
            .index;
        Some((world, player, map, loc, def))
    }

    #[test]
    fn soul_resonance_lifecycle_clears_both_halves_and_revalidates_delayed_casts() {
        let Some((mut world, player, map, loc, _)) = fixture() else {
            return;
        };
        let mut record = crate::accounts::test_character("SoulPartner");
        record.id = 2;
        let partner = world.add_player(2, 2, &record).unwrap();
        world
            .objects
            .get_mut(&player)
            .unwrap()
            .player_mut()
            .unwrap()
            .magics
            .push(UserMagic {
                magic: m::SOUL_RESONANCE,
                level: 0,
                experience: 0,
                key: 0,
                cooldown_until: 0,
            });
        let def = world.data.magics.get_mut(&m::SOUL_RESONANCE).unwrap();
        def.min_base_power = 20;
        def.max_base_power = 20;
        let group = |w: &mut World| {
            w.groups.insert(99, vec![player, partner]);
            for id in [player, partner] {
                w.objects.get_mut(&id).unwrap().player_mut().unwrap().group = Some(99);
            }
        };
        let pm = PendingMagic {
            time: 0,
            caster: player,
            magic: m::SOUL_RESONANCE,
            target: Some(partner),
            location: loc,
            direction: None,
            primary: true,
            chain: None,
        };
        group(&mut world);
        world.wave6_land(&pm, map, loc);
        for (id, bonus) in [(player, 0), (partner, 20)] {
            let b = world.objects[&id]
                .player()
                .unwrap()
                .buffs
                .iter()
                .find(|b| b.kind == buff_type::SOUL_RESONANCE)
                .unwrap();
            assert_eq!(b.expires, u64::MAX);
            assert_eq!(b.stats.hp_pct, bonus);
        }
        world.now = 60_001;
        world.process_buffs();
        assert!(world.objects[&partner].has_buff(buff_type::SOUL_RESONANCE));
        // Removal through the shared buff API clears both buffs and both links.
        world.buff_remove(player, buff_type::SOUL_RESONANCE);
        let cleared = |w: &World| {
            for id in [player, partner] {
                assert!(!w.objects[&id].has_buff(buff_type::SOUL_RESONANCE));
                assert_eq!(w.objects[&id].player().unwrap().soul_link, None);
                assert!(!w.objects[&id].dead);
            }
        };
        cleared(&world);
        world.wave6_land(&pm, map, loc);
        world.group_leave(partner);
        cleared(&world);
        world.wave6_land(&pm, map, loc); // Group changed during travel.
        cleared(&world);
        group(&mut world);
        world.objects.get_mut(&partner).unwrap().dead = true;
        world.wave6_land(&pm, map, loc);
        assert_eq!(world.objects[&player].player().unwrap().soul_link, None);
        world.objects.get_mut(&partner).unwrap().dead = false;
        world.wave6_land(&pm, map, loc);
        world.remove_object(partner); // Disconnect follows group leave.
        assert_eq!(world.objects[&player].player().unwrap().soul_link, None);
        assert!(!world.objects[&player].has_buff(buff_type::SOUL_RESONANCE));
    }

    #[test]
    fn summon_dead_roll_includes_the_maximum_skill_level() {
        use rand::SeedableRng;
        let Some((mut world, player, map, loc, def)) = fixture() else {
            return;
        };
        let seed = (0..1000)
            .find(|seed| {
                rand::rngs::StdRng::seed_from_u64(*seed)
                    .random_range(0..=i32::from(crate::magic::MAGIC_MAX_LEVEL))
                    == i32::from(crate::magic::MAGIC_MAX_LEVEL)
            })
            .unwrap();
        let magic = world.data.magics.get_mut(&m::SUMMON_DEAD).unwrap();
        magic.min_base_power = 0;
        magic.max_base_power = 0;
        magic.min_level_power = 0;
        magic.max_level_power = 0;
        world.objects.get_mut(&player).unwrap().stats.min_sc = 0;
        world.objects.get_mut(&player).unwrap().stats.max_sc = 0;
        world
            .objects
            .get_mut(&player)
            .unwrap()
            .player_mut()
            .unwrap()
            .magics
            .push(UserMagic {
                magic: m::SUMMON_DEAD,
                level: 3,
                experience: 0,
                key: 0,
                cooldown_until: 0,
            });
        let corpse = world.create_monster(def, map, loc, None, None, 0);
        world.objects.get_mut(&corpse).unwrap().dead = true;
        world.outgoing.clear();
        world.rng = rand::rngs::StdRng::seed_from_u64(seed);
        world.wave6_land(
            &PendingMagic {
                time: 0,
                caster: player,
                magic: m::SUMMON_DEAD,
                target: Some(corpse),
                location: loc,
                direction: None,
                primary: true,
                chain: None,
            },
            map,
            loc,
        );
        assert!(world.outgoing.iter().any(|out| matches!(out,
            Outgoing::To(_, ServerMessage::Chat {text}) if text == "The corpse does not rise.")));
        assert!(!world.objects.contains_key(&corpse));
    }

    #[test]
    fn soul_resonance_death_kills_only_the_current_linked_partner() {
        let Some((mut world, player, _, _, _)) = fixture() else {
            return;
        };
        let mut record = crate::accounts::test_character("SoulPartner");
        record.id = 2;
        let partner = world.add_player(2, 2, &record).unwrap();
        for (id, other) in [(player, partner), (partner, player)] {
            world
                .objects
                .get_mut(&id)
                .unwrap()
                .player_mut()
                .unwrap()
                .soul_link = Some(other);
            world.buff_add(
                id,
                buff_type::SOUL_RESONANCE,
                u64::MAX,
                BuffStats::default(),
            );
        }
        world.test_kill(player);
        for id in [player, partner] {
            assert!(world.objects[&id].dead);
            assert_eq!(world.objects[&id].player().unwrap().soul_link, None);
            assert!(!world.objects[&id].has_buff(buff_type::SOUL_RESONANCE));
        }
    }

    fn learn(world: &mut World, player: ObjectId, magic: u16) {
        world
            .objects
            .get_mut(&player)
            .unwrap()
            .player_mut()
            .unwrap()
            .magics
            .push(UserMagic {
                magic,
                level: 0,
                experience: 0,
                key: 0,
                cooldown_until: 0,
            });
    }

    fn equip(world: &mut World, player: ObjectId, item: i32, count: u32, slot: usize) {
        world.test_give_item(player, item, count);
        let inventory = world.test_slot_of(player, item).unwrap() as usize;
        let p = world
            .objects
            .get_mut(&player)
            .unwrap()
            .player_mut()
            .unwrap();
        p.bag.equipment[slot] = p.bag.inventory[inventory].take();
    }

    #[test]
    fn equipped_stone_controls_channels_and_mirror_without_consumption() {
        let Some((mut world, player, map, loc, def)) = fixture() else {
            return;
        };
        learn(&mut world, player, m::ELEMENTAL_HURRICANE);
        learn(&mut world, player, m::MIRROR_IMAGE);
        let item = world
            .data
            .items
            .values()
            .find(|d| d.item_type == item_type::DARK_STONE)
            .unwrap()
            .index;
        for stat in 44..=50 {
            world
                .data
                .items
                .get_mut(&item)
                .unwrap()
                .stats
                .insert(stat, 0);
        }
        world.data.items.get_mut(&item).unwrap().stats.insert(44, 5);
        world.data.items.get_mut(&item).unwrap().stats.insert(45, 9);
        world.test_give_item(player, item, 1);
        assert_eq!(
            world.dark_stone_affinity(player, true),
            element::NONE,
            "a bag stone is not equipped"
        );
        let inventory = world.test_slot_of(player, item).unwrap() as usize;
        let p = world
            .objects
            .get_mut(&player)
            .unwrap()
            .player_mut()
            .unwrap();
        p.bag.equipment[slot::AMULET] = p.bag.inventory[inventory].take();
        p.mp = 1000;
        p.max_mp = 1000;
        world
            .data
            .magics
            .get_mut(&m::ELEMENTAL_HURRICANE)
            .unwrap()
            .base_cost = 10;
        assert_eq!(
            world.dark_stone_affinity(player, false),
            element::ICE,
            "hurricane uses greatest affinity"
        );
        assert_eq!(
            world.dark_stone_affinity(player, true),
            element::FIRE,
            "mirror uses first positive affinity"
        );
        assert!(world.wave6_cast(
            player,
            m::ELEMENTAL_HURRICANE,
            None,
            loc,
            Direction::Down,
            loc,
            0,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new()
        ));
        assert_eq!(world.objects[&player].player().unwrap().mp, 1000);
        world.now = 11_000;
        world.process_channels();
        assert!(world.objects[&player].player().unwrap().channel.is_some());
        assert_eq!(world.objects[&player].player().unwrap().mp, 990);
        assert!(world.wave6_cast(
            player,
            m::ELEMENTAL_HURRICANE,
            None,
            loc,
            Direction::Down,
            loc,
            11_000,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new()
        ));
        assert!(world.objects[&player].player().unwrap().channel.is_none());
        assert_eq!(world.objects[&player].player().unwrap().mp, 980);
        let mut mirror = world.data.monsters[&def].clone();
        mirror.index = i32::MAX;
        mirror.flag = 7;
        world.data.monsters.retain(|_, d| d.flag != 7);
        world.data.monsters.insert(mirror.index, mirror);
        let pm = PendingMagic {
            time: 0,
            caster: player,
            magic: m::MIRROR_IMAGE,
            target: None,
            location: loc,
            direction: None,
            primary: true,
            chain: None,
        };
        world.wave6_land(&pm, map, loc);
        world.wave6_land(&pm, map, loc);
        let decoy = *world.objects[&player]
            .player()
            .unwrap()
            .pets
            .last()
            .unwrap();
        assert_eq!(
            world.objects[&decoy].monster_ref().mirror_element,
            Some(element::FIRE)
        );
        let decoy_loc = world.objects[&decoy].location;
        let before_hp = world.objects[&decoy].hp;
        assert_eq!(world.damage(decoy, player, 100, element::FIRE, true), 0);
        assert_eq!(world.objects[&decoy].hp, before_hp);
        for o in world
            .objects
            .values_mut()
            .filter(|o| o.is_monster() && o.id != decoy)
        {
            o.dead = true;
        }
        let enemy =
            world.create_monster(def, map, decoy_loc.step(Direction::Down, 2), None, None, 0);
        world.objects.get_mut(&player).unwrap().stats.min_mc = 100;
        world.objects.get_mut(&player).unwrap().stats.max_mc = 100;
        world.process_monster(decoy);
        assert_eq!(world.objects[&decoy].location, decoy_loc);
        let spell = world.pending_monster_spells.last().unwrap();
        assert_eq!(
            spell.element,
            element::ICE,
            "C# intentionally uses Ice damage for all four mirror affinities"
        );
        assert_eq!(spell.power, 100);
        assert!(spell.targets.contains(&enemy));
        assert_eq!(spell.time, world.now + 500);
        assert!(world.events.iter().any(|(_, event)| matches!(event, ServerMessage::ObjectMagic { id, magic, .. } if *id == decoy && *magic == m::FIRE_STORM)));
        assert_eq!(
            world
                .objects
                .values()
                .filter(|o| o
                    .monster()
                    .is_some_and(|m| m.owner == Some(player) && m.def == i32::MAX))
                .count(),
            1
        );
        assert_eq!(
            world.objects[&player].player().unwrap().bag.equipment[slot::AMULET]
                .as_ref()
                .unwrap()
                .count,
            1
        );
    }

    #[test]
    fn dragon_blood_requires_green_poison_and_consumes_selected_charge() {
        use rand::SeedableRng;
        let Some((mut world, player, _, _, _)) = fixture() else {
            return;
        };
        learn(&mut world, player, m::DRAGON_BLOOD);
        let seed = (0..1000)
            .find(|seed| {
                let mut rng = rand::rngs::StdRng::seed_from_u64(*seed);
                rng.random_range(0..5) == 0 && rng.random_range(0..10) > 0
            })
            .unwrap();
        world.rng = rand::rngs::StdRng::seed_from_u64(seed);
        assert!(!world.dragon_blood_attack(player, None));
        assert!(!world.objects[&player].player().unwrap().dragon_blood);
        let item = world
            .data
            .items
            .values()
            .find(|d| d.item_type == item_type::POISON)
            .unwrap()
            .index;
        world.data.items.get_mut(&item).unwrap().shape = 2;
        equip(&mut world, player, item, 5, slot::POISON);
        let count = |w: &World| {
            w.objects[&player].player().unwrap().bag.equipment[slot::POISON]
                .as_ref()
                .unwrap()
                .count
        };
        let before = count(&world);
        world.rng = rand::rngs::StdRng::seed_from_u64(seed);
        assert!(!world.dragon_blood_attack(player, None));
        assert!(world.objects[&player].player().unwrap().dragon_blood);
        assert_eq!(count(&world), before - 1);
        assert!(!world.dragon_blood_attack(player, None));
        assert_eq!(count(&world), before - 1);
        let fail_seed = (0..1000)
            .find(|seed| rand::rngs::StdRng::seed_from_u64(*seed).random_range(0..5) != 0)
            .unwrap();
        world.rng = rand::rngs::StdRng::seed_from_u64(fail_seed);
        assert!(world.dragon_blood_attack(player, Some(m::DRAGON_BLOOD)));
        assert!(!world.objects[&player].player().unwrap().dragon_blood);
        assert_eq!(count(&world), before - 1);
        assert!(m::is_auto_charge(m::DRAGON_BLOOD));
        assert!(!m::is_castable(m::DRAGON_BLOOD));
    }

    #[test]
    fn frost_bite_uses_ice_attack_and_slow_chance_without_refreshing_buff() {
        let Some((mut world, player, map, loc, def)) = fixture() else {
            return;
        };
        learn(&mut world, player, m::FROST_BITE);
        let item = world
            .data
            .items
            .values()
            .find(|d| d.item_type == item_type::AMULET && d.shape == 0)
            .unwrap()
            .index;
        world
            .data
            .items
            .get_mut(&item)
            .unwrap()
            .stats
            .insert(22, 10);
        equip(&mut world, player, item, 5, slot::AMULET);
        assert_eq!(world.ice_attack(player), 10);
        world.objects.get_mut(&player).unwrap().stats.min_mc = 5;
        world.objects.get_mut(&player).unwrap().stats.max_mc = 5;
        let magic = world.data.magics.get_mut(&m::FROST_BITE).unwrap();
        magic.min_base_power = 7;
        magic.max_base_power = 7;
        let pm = PendingMagic {
            time: 0,
            caster: player,
            magic: m::FROST_BITE,
            target: None,
            location: loc,
            direction: None,
            primary: true,
            chain: None,
        };
        world.wave6_land(&pm, map, loc);
        let b = world.objects[&player]
            .player()
            .unwrap()
            .buffs
            .iter()
            .find(|b| b.kind == buff_type::FROST_BITE)
            .unwrap();
        assert_eq!(b.stats.frost, 32);
        assert_eq!(b.stats.frost_chance, 5);
        let expiry = b.expires;
        world.now = 1000;
        world.wave6_land(&pm, map, loc);
        assert_eq!(
            world.objects[&player]
                .player()
                .unwrap()
                .buffs
                .iter()
                .find(|b| b.kind == buff_type::FROST_BITE)
                .unwrap()
                .expires,
            expiry
        );
        let enemy = world.create_monster(def, map, loc, None, None, 0);
        world
            .objects
            .get_mut(&player)
            .unwrap()
            .player_mut()
            .unwrap()
            .buffs
            .iter_mut()
            .find(|b| b.kind == buff_type::FROST_BITE)
            .unwrap()
            .stats
            .frost_chance = 2000;
        world.frost_store(player, enemy, 9);
        assert!(world.objects[&enemy]
            .poisons
            .iter()
            .any(|p| p.kind == poison_kind::SLOW && p.ticks_left == 5));
    }

    #[test]
    fn frost_bite_burst_excludes_bosses_and_friendly_monsters() {
        let Some((mut world, player, map, loc, def)) = fixture() else {
            return;
        };
        let enemy = world.create_monster(def, map, loc, None, None, 0);
        let pet = world.create_monster(def, map, loc, None, Some(player), 0);
        let mut boss_def = world.data.monsters[&def].clone();
        boss_def.index = i32::MAX;
        boss_def.is_boss = true;
        world.data.monsters.insert(boss_def.index, boss_def);
        let boss = world.create_monster(i32::MAX, map, loc, None, None, 0);
        world.objects.get_mut(&player).unwrap().stats.max_mc = 100;
        let before = [enemy, pet, boss].map(|id| world.objects[&id].hp);
        world.frost_bite_burst(player, 10);
        assert!(world.objects[&enemy].hp < before[0]);
        assert_eq!(world.objects[&pet].hp, before[1]);
        assert_eq!(world.objects[&boss].hp, before[2]);
    }

    #[test]
    fn cursed_doll_rejects_bosses_friends_and_existing_links_before_amulet_use() {
        let Some((mut world, player, map, loc, def)) = fixture() else {
            return;
        };
        let amulet = world
            .data
            .items
            .values()
            .find(|d| d.item_type == item_type::AMULET && d.shape == 1)
            .unwrap()
            .index;
        world.test_give_item(player, amulet, 5);
        let bag_slot = world.test_slot_of(player, amulet).unwrap() as usize;
        let p = world
            .objects
            .get_mut(&player)
            .unwrap()
            .player_mut()
            .unwrap();
        p.bag.equipment[slot::AMULET] = p.bag.inventory[bag_slot].take();
        let enemy = world.create_monster(def, map, loc, None, None, 0);
        let pet = world.create_monster(def, map, loc, None, Some(player), 0);
        let mut boss_def = world.data.monsters[&def].clone();
        boss_def.index = i32::MAX;
        boss_def.is_boss = true;
        world.data.monsters.insert(boss_def.index, boss_def);
        let boss = world.create_monster(i32::MAX, map, loc, None, None, 0);
        let count = |w: &World| {
            w.objects[&player].player().unwrap().bag.equipment[slot::AMULET]
                .as_ref()
                .unwrap()
                .count
        };
        let cast = |w: &mut World, target| {
            w.wave6_cast(
                player,
                m::CURSED_DOLL,
                Some(target),
                loc,
                Direction::Down,
                loc,
                0,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut Vec::new(),
            )
        };
        let before = count(&world);
        assert!(!cast(&mut world, boss));
        assert!(!cast(&mut world, pet));
        assert_eq!(count(&world), before);
        // C# keeps CursedList membership until the doll despawns, including its corpse.
        world
            .objects
            .get_mut(&pet)
            .unwrap()
            .monster_mut()
            .unwrap()
            .link = Some(enemy);
        world.objects.get_mut(&pet).unwrap().dead = true;
        assert!(!cast(&mut world, enemy));
        assert_eq!(count(&world), before);
        world
            .objects
            .get_mut(&pet)
            .unwrap()
            .monster_mut()
            .unwrap()
            .link = None;
        world.data.items.get_mut(&amulet).unwrap().shape = 0;
        assert!(!cast(&mut world, enemy));
        assert_eq!(count(&world), before);
        world.data.items.get_mut(&amulet).unwrap().shape = 1;
        assert!(cast(&mut world, enemy));
        assert_eq!(count(&world), before - 1);
        // C# allows hostile players too; the old monster-only gate rejected them.
        let mut record = crate::accounts::test_character("DollOpponent");
        record.id = 2;
        let opponent = world.add_player(2, 2, &record).unwrap();
        world.objects.get_mut(&opponent).unwrap().in_safe_zone = false;
        let caster = world.objects.get_mut(&player).unwrap();
        caster.in_safe_zone = false;
        caster.player_mut().unwrap().attack_mode = mir_proto::attack_mode::ALL;
        assert!(cast(&mut world, opponent));
        assert_eq!(count(&world), before - 2);
    }
}
