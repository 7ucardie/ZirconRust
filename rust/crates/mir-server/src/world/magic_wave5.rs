//! Skills wave five: lines, bounces, placed fields, summons, kicks and
//! dashes, ticking channels and augments. Rules from
//! `docs/research/skills-remaining.md`.
//!
//! `PendingMagic.chain` carries per-magic state: Crushing Wave `(primary,
//! [origin])`, bounces `(remaining, visited targets as points)`.

use super::monster_ai::has_poison;
use super::*;
use mir_proto::magic_type as m;

pub(super) fn handled(magic: u16) -> bool {
    matches!(
        magic,
        m::CRUSHING_WAVE
            | m::FIRE_BOUNCE
            | m::LIGHTNING_STRIKE
            | m::ICE_AURA
            | m::BURNING_FIRE
            | m::DARK_SOUL_PRISON
            | m::SUMMON_DEMONIC_CREATURE
            | m::DEMON_EXPLOSION
            | m::THUNDER_KICK
            | m::DANCE_OF_SWALLOW
            | m::HUNDRED_FIST
            | m::DRAGON_REPULSE
            | m::ELEMENTAL_SWORDS
            | m::BINDING_TALISMAN
            | m::BRAIN_STORM
            | m::IMPROVED_EXPLOSIVE_TALISMAN
    )
}

/// Straight line or exact diagonal (Zircon `IsStraightEightDirection`).
fn straight(a: Point, b: Point) -> bool {
    let dx = (a.x - b.x).abs();
    let dy = (a.y - b.y).abs();
    dx == 0 || dy == 0 || dx == dy
}

impl World {
    /// Push `target` up to `distance` cells along `dir`; returns cells moved.
    pub(super) fn push_back(&mut self, target: ObjectId, dir: Direction, distance: i32) -> i32 {
        // Zircon `Pushed` throws the rider off the horse.
        self.remove_mount(target);
        let (map, mut loc) = {
            let o = &self.objects[&target];
            (o.map, o.location)
        };
        let mut moved = 0;
        for _ in 0..distance {
            let next = loc.step(dir, 1);
            if !self.maps[&map].file.is_walkable(next.x, next.y)
                || self.cell_blocked(map, next, false)
            {
                break;
            }
            loc = next;
            moved += 1;
        }
        if moved > 0 {
            self.teleport_object(target, loc);
            if let Some(o) = self.objects.get_mut(&target) {
                o.direction = dir.rotate(4);
                o.action_time = self.now + 500;
            }
        }
        moved
    }

    /// Zircon push gate: `Random(MagicMaxLevel + 12) >= 6 + 3L + level - obLevel` fails.
    pub(super) fn push_allowed(
        &mut self,
        target: ObjectId,
        caster_level: i32,
        magic_level: i32,
    ) -> bool {
        let (tlevel, can_push, boss) = match &self.objects[&target].kind {
            Kind::Monster(mo) => {
                let d = &self.data.monsters[&mo.def];
                (d.level, d.can_push, d.is_boss)
            }
            Kind::Player(p) => (p.level, true, false),
            _ => return false,
        };
        if !can_push || boss || tlevel >= caster_level {
            return false;
        }
        if self.objects[&target].has_buff(buff_type::ENDURANCE) {
            return false;
        }
        let max = crate::magic::MAGIC_MAX_LEVEL as i32 + 12;
        self.rng.random_range(0..max) < 6 + magic_level * 3 + caster_level - tlevel
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn wave5_cast(
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
        let mk = |time: u64,
                  target: Option<ObjectId>,
                  cell: Point,
                  chain: Option<(i32, Vec<Point>)>| PendingMagic {
            time,
            caster: id,
            magic,
            target,
            location: cell,
            direction: Some(direction),
            primary: true,
            chain,
        };
        let dist_ms = |w: &World, t: ObjectId| w.objects[&t].location.distance(loc) as u64 * 48;
        let map = self.objects[&id].map;
        match magic {
            m::CRUSHING_WAVE => {
                let (l, r) = if direction.index().is_multiple_of(2) {
                    (2i8, -2i8)
                } else {
                    (1, -1)
                };
                for i in 1..=12 {
                    let cell = loc.step(direction, i);
                    if !self.maps[&map].file.is_walkable(cell.x, cell.y) {
                        break;
                    }
                    pending.push(mk(
                        now + 400 + i as u64 * 60,
                        None,
                        cell,
                        Some((1, vec![loc])),
                    ));
                    for side in [l, r] {
                        let flank = loc.step(direction.rotate(side), 1).step(direction, i);
                        pending.push(mk(
                            now + 200 + i as u64 * 60,
                            None,
                            flank,
                            Some((0, vec![loc])),
                        ));
                    }
                    if i <= 4 {
                        locations.push(cell);
                    }
                }
            }
            m::FIRE_BOUNCE | m::LIGHTNING_STRIKE => {
                let Some(t) = target.filter(|t| self.objects[t].is_monster()) else {
                    locations.push(location);
                    return false;
                };
                targets.push(t);
                let tloc = self.objects[&t].location;
                pending.push(mk(
                    now + 500 + dist_ms(self, t),
                    Some(t),
                    tloc,
                    Some((level + 2, vec![tloc])),
                ));
            }
            m::ICE_AURA => {
                locations.push(loc.step(direction, 5));
                for i in 1..=8 {
                    let cell = loc.step(direction, i);
                    if !self.maps[&map].file.is_walkable(cell.x, cell.y) {
                        break;
                    }
                    pending.push(mk(now + 500 + i as u64 * 48, None, cell, None));
                }
            }
            m::BURNING_FIRE => {
                if location.distance(loc) > MAGIC_RANGE {
                    return false;
                }
                let max = level.clamp(1, 3) as usize;
                let mine_count = self
                    .on_map(map)
                    .filter(|o| matches!(&o.kind, Kind::Spell(s) if s.owner == id && s.effect == spell_effect::BURNING_FIRE))
                    .count();
                if mine_count >= max {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: "You cannot place more fires.".into(),
                        },
                    );
                    return false;
                }
                locations.push(location);
                pending.push(mk(now + 1600, None, location, None));
            }
            m::DARK_SOUL_PRISON => {
                if location.distance(loc) > MAGIC_RANGE {
                    return false;
                }
                locations.push(location);
                pending.push(mk(now + 500, None, location, None));
            }
            m::SUMMON_DEMONIC_CREATURE => {
                if self.use_amulet(id, 25).is_none() {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: "Summon Demonic Creature needs 25 talismans.".into(),
                        },
                    );
                    return false;
                }
                pending.push(mk(now + 500, None, loc.step(direction.rotate(4), 1), None));
            }
            m::DEMON_EXPLOSION => {
                let has_demon = self.objects[&id]
                    .player()
                    .map(|p| p.pets.iter().any(|pet| self.monster_flag(*pet) == 4))
                    .unwrap_or(false);
                if !has_demon || self.use_amulet(id, 20).is_none() {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: "Demon Explosion needs your demon and 20 talismans.".into(),
                        },
                    );
                    return false;
                }
                pending.push(mk(now + 500, None, loc, None));
            }
            m::THUNDER_KICK => {
                let front = loc.step(direction, 1);
                if !self.maps[&map].objects_at(front).is_empty() {
                    locations.push(front);
                }
                pending.push(mk(now + 500, None, front, None));
            }
            m::DANCE_OF_SWALLOW => {
                let Some(t) = target.filter(|t| self.objects[t].is_monster()) else {
                    return false;
                };
                let tloc = self.objects[&t].location;
                if has_poison(&self.objects[&id], poison_kind::WRAITH_GRIP) {
                    return false;
                }
                // First free cell around the target, starting from our side.
                let base = Direction::from_points(tloc, loc);
                let mut spot = None;
                for i in 0..8 {
                    let cell = tloc.step(base.rotate(i), 1);
                    if self.maps[&map].file.is_walkable(cell.x, cell.y)
                        && !self.cell_blocked(map, cell, false)
                    {
                        spot = Some(cell);
                        break;
                    }
                }
                let Some(spot) = spot else { return false };
                self.buff_remove(id, buff_type::TRANSPARENCY);
                self.buff_remove(id, buff_type::CLOAK);
                self.teleport_object(id, spot);
                let face = Direction::from_points(spot, tloc);
                if let Some(o) = self.objects.get_mut(&id) {
                    o.direction = face;
                }
                self.events.push((
                    id,
                    ServerMessage::ObjectAttack {
                        id,
                        direction: face,
                        attack_magic: Some(m::DANCE_OF_SWALLOW),
                        element: mir_proto::element::NONE,
                    },
                ));
                targets.push(t);
                pending.push(mk(now + 400, Some(t), tloc, None));
            }
            m::HUNDRED_FIST => {
                let Some(t) = target.filter(|t| self.objects[t].is_monster()) else {
                    return false;
                };
                let tloc = self.objects[&t].location;
                if !straight(loc, tloc) || has_poison(&self.objects[&id], poison_kind::WRAITH_GRIP)
                {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: "Hundred Fist needs a straight run at the target.".into(),
                        },
                    );
                    return false;
                }
                let dir = Direction::from_points(loc, tloc);
                let stop = tloc.step(dir.rotate(4), 1);
                if stop != loc
                    && (!self.maps[&map].file.is_walkable(stop.x, stop.y)
                        || self.cell_blocked(map, stop, false))
                {
                    return false;
                }
                let travelled = loc.distance(stop);
                if stop != loc {
                    self.teleport_object(id, stop);
                }
                if let Some(o) = self.objects.get_mut(&id) {
                    o.direction = dir;
                    o.action_time = now + 300;
                }
                targets.push(t);
                pending.push(mk(now + 300, Some(t), tloc, Some((travelled, vec![stop]))));
            }
            m::DRAGON_REPULSE => {
                let (hp, max_hp, mp, max_mp, cost) = {
                    let o = &self.objects[&id];
                    let p = o.player().unwrap();
                    let um = p.magics.iter().find(|x| x.magic == magic).unwrap();
                    (
                        o.hp,
                        o.max_hp,
                        p.mp,
                        p.max_mp,
                        um.cost(&self.data.magics[&magic]),
                    )
                };
                let hp_cost = max_hp * cost / 1000;
                let mp_cost = max_mp * cost / 1000;
                if hp_cost >= hp || hp < max_hp / 10 || mp_cost >= mp || mp < max_mp / 10 {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: "Not enough health or mana for Dragon Repulse.".into(),
                        },
                    );
                    return false;
                }
                {
                    let o = self.objects.get_mut(&id).unwrap();
                    o.hp -= hp_cost;
                    o.player_mut().unwrap().mp -= mp_cost;
                    let (hp, max_hp) = (o.hp, o.max_hp);
                    self.events
                        .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
                }
                pending.push(mk(now + 200, None, loc, None));
            }
            m::ELEMENTAL_SWORDS => {
                if self.objects[&id].has_buff(buff_type::ELEMENTAL_SWORDS) {
                    return false;
                }
                pending.push(mk(now + 500, None, loc, None));
            }
            m::BINDING_TALISMAN | m::BRAIN_STORM | m::IMPROVED_EXPLOSIVE_TALISMAN => {
                let Some(t) = target.filter(|t| self.objects[t].is_monster()) else {
                    locations.push(location);
                    return false;
                };
                if magic == m::BINDING_TALISMAN
                    && has_poison(&self.objects[&t], poison_kind::BINDING)
                {
                    return false;
                }
                if magic == m::BRAIN_STORM && !has_poison(&self.objects[&t], poison_kind::BINDING) {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: "Brain Storm needs a bound target.".into(),
                        },
                    );
                    return false;
                }
                if self.use_amulet(id, 1).is_none() {
                    return false;
                }
                targets.push(t);
                let base = if magic == m::BRAIN_STORM { 1000 } else { 500 };
                pending.push(mk(now + base + dist_ms(self, t), Some(t), location, None));
            }
            _ => return false,
        }
        true
    }

    /// `MonsterInfo.Flag` of a monster object (0 for anything else).
    pub(super) fn monster_flag(&self, id: ObjectId) -> i32 {
        match self.objects.get(&id).map(|o| &o.kind) {
            Some(Kind::Monster(mo)) => self.data.monsters[&mo.def].flag,
            _ => 0,
        }
    }

    pub(super) fn wave5_land(&mut self, pm: &PendingMagic, cmap: i32, cloc: Point) {
        let caster = pm.caster;
        let level = self.magic_level(caster, pm.magic);
        let (pmin, pmax, _) = self.caster_power(caster, pm.magic);
        let cs = self.objects[&caster].stats;
        let power = self.roll_range(pmin, pmax);
        let dc = self.roll_dc(cs);
        let sp = self.roll_lucky(cs.min_mc.min(cs.min_sc), cs.max_mc.min(cs.max_sc), cs.luck);
        let clevel = self.level_of(&self.objects[&caster]);
        match pm.magic {
            m::CRUSHING_WAVE => {
                let primary = pm.chain.as_ref().map(|c| c.0 == 1).unwrap_or(true);
                let victims: Vec<ObjectId> = self.maps[&cmap]
                    .objects_at(pm.location)
                    .iter()
                    .copied()
                    .filter(|v| self.objects[&caster].hostile_to(&self.objects[v]))
                    .collect();
                for v in victims {
                    let mut p = if primary { dc } else { dc * power / 100 };
                    if self.objects[&v].is_player() {
                        p /= 2;
                    }
                    let ts = self.objects[&v].stats;
                    let dealt = p - self.roll_ac(ts);
                    if dealt > 0 {
                        self.damage(v, caster, dealt, element::NONE, true);
                    }
                }
                if primary {
                    self.level_magic(caster, pm.magic);
                }
            }
            m::FIRE_BOUNCE | m::LIGHTNING_STRIKE => {
                let Some(t) = pm.target else { return };
                let (elem,) = if pm.magic == m::FIRE_BOUNCE {
                    (element::FIRE,)
                } else {
                    (element::LIGHTNING,)
                };
                if self.objects.get(&t).map(|o| o.dead).unwrap_or(true) {
                    return;
                }
                if self.objects[&t].location.distance(pm.location) > MAGIC_RANGE {
                    return;
                }
                self.magic_attack(caster, t, pm.magic, elem, 100);
                self.level_magic(caster, pm.magic);
                let Some((remaining, visited)) = pm.chain.clone() else {
                    return;
                };
                if remaining <= 1 {
                    return;
                }
                let tloc = self.objects[&t].location;
                let candidates: Vec<ObjectId> = self
                    .on_map(cmap)
                    .filter(|o| o.is_monster() && !o.dead && o.id != t)
                    .filter(|o| o.location.distance(tloc) <= 3 && !visited.contains(&o.location))
                    .filter(|o| self.objects[&caster].hostile_to(o))
                    .map(|o| o.id)
                    .collect();
                if candidates.is_empty() {
                    return;
                }
                let next = candidates[self.rng.random_range(0..candidates.len())];
                let nloc = self.objects[&next].location;
                let mut visited = visited;
                visited.push(nloc);
                self.events.push((
                    next,
                    ServerMessage::ObjectEffect {
                        id: next,
                        effect: effect::BOUNCE,
                        location: nloc,
                    },
                ));
                self.pending_magics.push(PendingMagic {
                    time: self.now + tloc.distance(nloc) as u64 * 48,
                    caster,
                    magic: pm.magic,
                    target: Some(next),
                    location: nloc,
                    direction: pm.direction,
                    primary: true,
                    chain: Some((remaining - 1, visited)),
                });
            }
            m::ICE_AURA => {
                // Only the first cell along the line that holds a monster gets the field.
                let already = self
                    .on_map(cmap)
                    .any(|o| matches!(&o.kind, Kind::Spell(s) if s.owner == caster && s.effect == spell_effect::ICE_AURA && s.tick_time >= self.now.saturating_sub(48 * 9)));
                if already {
                    return;
                }
                let has_target = self.maps[&cmap]
                    .objects_at(pm.location)
                    .iter()
                    .any(|v| self.objects[&caster].hostile_to(&self.objects[v]));
                if !has_target {
                    return;
                }
                self.spawn_spell(
                    cmap,
                    pm.location,
                    spell_effect::ICE_AURA,
                    2,
                    (5 + level as u64 * 3) * 1000,
                    caster,
                    pm.magic,
                );
                self.level_magic(caster, pm.magic);
            }
            m::BURNING_FIRE => {
                self.spawn_spell(
                    cmap,
                    pm.location,
                    spell_effect::BURNING_FIRE,
                    15,
                    1000,
                    caster,
                    pm.magic,
                );
                self.level_magic(caster, pm.magic);
            }
            m::DARK_SOUL_PRISON => {
                let old: Vec<ObjectId> = self
                    .on_map(cmap)
                    .filter(|o| matches!(&o.kind, Kind::Spell(s) if s.owner == caster && s.effect == spell_effect::DARK_SOUL_PRISON))
                    .filter(|o| o.location.distance(pm.location) <= 6)
                    .map(|o| o.id)
                    .collect();
                for o in old {
                    self.remove_object(o);
                }
                self.spawn_spell(
                    cmap,
                    pm.location,
                    spell_effect::DARK_SOUL_PRISON,
                    level + 5,
                    2000,
                    caster,
                    pm.magic,
                );
                self.level_magic(caster, pm.magic);
            }
            m::SUMMON_DEMONIC_CREATURE => {
                let Some(def_index) = self
                    .data
                    .monsters
                    .values()
                    .find(|d| d.flag == 4)
                    .map(|d| d.index)
                else {
                    return;
                };
                let pets: Vec<ObjectId> = self.objects[&caster]
                    .player()
                    .map(|p| p.pets.clone())
                    .unwrap_or_default();
                if let Some(existing) = pets.iter().copied().find(|p| self.monster_flag(*p) == 4) {
                    let to = self.summon_placement(cmap, pm.location, cloc);
                    self.teleport_object(existing, to);
                    return;
                }
                if pets.len() >= 2 {
                    return;
                }
                let spot = self.summon_placement(cmap, pm.location, cloc);
                self.create_monster(def_index, cmap, spot, None, Some(caster), level * 2);
                self.level_magic(caster, pm.magic);
            }
            m::DEMON_EXPLOSION => {
                let Some(pet) = self.objects[&caster].player().and_then(|p| {
                    p.pets
                        .iter()
                        .copied()
                        .find(|pet| self.monster_flag(*pet) == 4)
                }) else {
                    return;
                };
                let (pet_max, pet_loc, pet_map) = {
                    let o = &self.objects[&pet];
                    (o.max_hp, o.location, o.map)
                };
                {
                    let o = self.objects.get_mut(&pet).unwrap();
                    o.hp = (o.hp - pet_max * 75 / 100).max(1);
                    let (hp, max_hp) = (o.hp, o.max_hp);
                    self.events.push((
                        pet,
                        ServerMessage::HealthChanged {
                            id: pet,
                            hp,
                            max_hp,
                        },
                    ));
                }
                let sc = self.roll_lucky(cs.min_sc, cs.max_sc, cs.luck);
                let boom = pet_max * power / 100 + sc * 3;
                let victims: Vec<ObjectId> = self
                    .on_map(pet_map)
                    .filter(|o| o.is_monster() && !o.dead && o.location.distance(pet_loc) <= 2)
                    .filter(|o| self.objects[&caster].hostile_to(o))
                    .map(|o| o.id)
                    .collect();
                for v in victims {
                    let ts = self.objects[&v].stats;
                    let dealt = boom - self.roll_range(ts.min_mr, ts.max_mr);
                    if dealt > 0 {
                        self.damage(v, caster, dealt, element::PHANTOM, true);
                    }
                }
                self.events.push((
                    pet,
                    ServerMessage::ObjectEffect {
                        id: pet,
                        effect: effect::DEMON_EXPLOSION,
                        location: pet_loc,
                    },
                ));
                for dx in -3..=3 {
                    for dy in -3..=3 {
                        let cell = Point::new(pet_loc.x + dx, pet_loc.y + dy);
                        if self.maps[&pet_map].file.is_walkable(cell.x, cell.y) {
                            self.spawn_spell(
                                pet_map,
                                cell,
                                spell_effect::FIRE_WALL,
                                level + 2,
                                2000,
                                caster,
                                m::FIRE_WALL,
                            );
                        }
                    }
                }
                self.level_magic(caster, pm.magic);
            }
            m::THUNDER_KICK => {
                let front = pm.location;
                let dir = pm.direction.unwrap_or(Direction::Down);
                let victims: Vec<ObjectId> = self
                    .on_map(cmap)
                    .filter(|o| !o.dead && o.location.distance(front) <= 1)
                    .filter(|o| self.objects[&caster].hostile_to(o))
                    .map(|o| o.id)
                    .collect();
                for v in victims {
                    if !self.push_allowed(v, clevel, level) {
                        continue;
                    }
                    let vloc = self.objects[&v].location;
                    let push_dir = if vloc == front {
                        dir
                    } else {
                        Direction::from_points(front, vloc)
                    };
                    if vloc != front && self.push_back(v, push_dir, power) <= 0 {
                        continue;
                    } else if vloc == front {
                        self.push_back(v, push_dir, power);
                    }
                    let ts = self.objects[&v].stats;
                    let dealt = dc - self.roll_ac(ts);
                    if dealt > 0 {
                        self.damage(v, caster, dealt, element::NONE, false);
                    }
                    self.level_magic(caster, pm.magic);
                    break;
                }
            }
            m::DANCE_OF_SWALLOW => {
                let Some(t) = pm.target else { return };
                if self.objects.get(&t).map(|o| o.dead).unwrap_or(true) {
                    return;
                }
                let ts = self.objects[&t].stats;
                let dealt = dc * 2 - self.roll_ac(ts);
                let tloc = self.objects[&t].location;
                self.events.push((
                    t,
                    ServerMessage::ObjectEffect {
                        id: t,
                        effect: effect::DANCE_OF_SWALLOW,
                        location: tloc,
                    },
                ));
                if dealt > 0 && self.damage(t, caster, dealt, element::NONE, false) > 0 {
                    let tlevel = self.level_of(&self.objects[&t]);
                    if tlevel < clevel {
                        self.apply_poison(
                            t,
                            Poison {
                                kind: poison_kind::SILENCED,
                                value: 0,
                                ticks_left: 0,
                                next_tick: self.now + (power as u64 + 1) * 1000,
                                owner: Some(caster),
                            },
                        );
                        self.apply_poison(
                            t,
                            Poison {
                                kind: poison_kind::PARALYSIS,
                                value: 0,
                                ticks_left: 0,
                                next_tick: self.now + 1000,
                                owner: Some(caster),
                            },
                        );
                    }
                    self.level_magic(caster, pm.magic);
                }
            }
            m::HUNDRED_FIST => {
                let Some(t) = pm.target else { return };
                if self.objects.get(&t).map(|o| o.dead).unwrap_or(true) {
                    return;
                }
                let travelled = pm.chain.as_ref().map(|c| c.0).unwrap_or(0);
                let wanted = travelled * 2;
                let dir = pm.direction.unwrap_or(Direction::Down);
                let tloc = self.objects[&t].location;
                self.events.push((
                    t,
                    ServerMessage::ObjectEffect {
                        id: t,
                        effect: effect::HUNDRED_FIST,
                        location: tloc,
                    },
                ));
                let pushed = if wanted > 0 && self.push_allowed(t, clevel, level) {
                    self.push_back(t, dir, wanted)
                } else {
                    0
                };
                // Damage only when the push was cut short.
                if wanted > 0 && pushed < wanted {
                    let ts = self.objects[&t].stats;
                    let dealt = power + dc * pushed.max(1) - self.roll_ac(ts);
                    if dealt > 0 {
                        self.damage(t, caster, dealt, element::NONE, false);
                    }
                }
                self.level_magic(caster, pm.magic);
            }
            m::DRAGON_REPULSE => {
                self.buff_add(
                    caster,
                    buff_type::DRAGON_REPULSE,
                    6000,
                    BuffStats {
                        pool: dc * power / 100 + clevel,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::ELEMENTAL_SWORDS => {
                self.buff_add(
                    caster,
                    buff_type::ELEMENTAL_SWORDS,
                    u64::MAX,
                    BuffStats {
                        stacks: 5,
                        pool: power,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::BINDING_TALISMAN => {
                let Some(t) = pm.target else { return };
                let dealt = self.magic_attack(caster, t, pm.magic, element::HOLY, 100);
                if dealt > 0 {
                    self.apply_poison(
                        t,
                        Poison {
                            kind: poison_kind::BINDING,
                            value: dealt,
                            ticks_left: (power / 2).max(1),
                            next_tick: self.now + 2000,
                            owner: Some(caster),
                        },
                    );
                    self.level_magic(caster, pm.magic);
                }
            }
            m::BRAIN_STORM => {
                let Some(t) = pm.target else { return };
                if let Some(o) = self.objects.get_mut(&t) {
                    o.poisons.retain(|p| p.kind != poison_kind::BINDING);
                }
                self.send_poison_status(t);
                let sc = self.roll_lucky(cs.min_sc, cs.max_sc, cs.luck);
                let ts = self.objects[&t].stats;
                let dealt = sc * (level + 1) - self.roll_range(ts.min_mr, ts.max_mr);
                if dealt > 0 && self.damage(t, caster, dealt, element::HOLY, true) > 0 {
                    if let Some(mo) = self.objects.get_mut(&t).and_then(|o| o.monster_mut()) {
                        mo.shock_until = self.now + power.max(0) as u64;
                    }
                    self.level_magic(caster, pm.magic);
                }
            }
            m::IMPROVED_EXPLOSIVE_TALISMAN => {
                let Some(t) = pm.target else { return };
                if self.magic_attack(caster, t, pm.magic, element::DARK, 100) > 0 {
                    self.level_magic(caster, pm.magic);
                }
                let _ = sp;
            }
            _ => {}
        }
    }

    /// Dragon Repulse tick (every 500 ms): hit and shove everything within
    /// five cells (four-direction distance).
    pub(super) fn dragon_repulse_tick(&mut self, id: ObjectId, power: i32) {
        let (map, loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        let victims: Vec<ObjectId> = self
            .on_map(map)
            .filter(|o| !o.dead && self.objects[&id].hostile_to(o))
            .filter(|o| (o.location.x - loc.x).abs() + (o.location.y - loc.y).abs() <= 5)
            .map(|o| o.id)
            .collect();
        for v in victims {
            let ts = self.objects[&v].stats;
            let dealt = power - self.roll_range(ts.min_mr, ts.max_mr);
            if dealt > 0 {
                self.damage(v, id, dealt, element::LIGHTNING, true);
            }
            let vloc = self.objects[&v].location;
            let dir = Direction::from_points(loc, vloc);
            if self.push_back(v, dir, 1) == 0 && self.push_back(v, dir.rotate(1), 1) == 0 {
                self.push_back(v, dir.rotate(-1), 1);
            }
        }
    }

    /// Elemental Swords tick (every 5 s): one sword flies at a random
    /// monster within five cells that is hunting the player.
    pub(super) fn elemental_swords_tick(&mut self, id: ObjectId, power: i32) -> bool {
        let (map, loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        let candidates: Vec<ObjectId> = self
            .on_map(map)
            .filter(|o| !o.dead && o.location.distance(loc) <= 5)
            .filter(|o| matches!(&o.kind, Kind::Monster(mo) if mo.target == Some(id)))
            .map(|o| o.id)
            .collect();
        if candidates.is_empty() {
            return false;
        }
        let t = candidates[self.rng.random_range(0..candidates.len())];
        let tloc = self.objects[&t].location;
        self.events.push((
            t,
            ServerMessage::ObjectEffect {
                id: t,
                effect: effect::ELEMENTAL_SWORD,
                location: tloc,
            },
        ));
        let ts = self.objects[&t].stats;
        let dealt = power - self.roll_range(ts.min_mr, ts.max_mr);
        if dealt > 0 && self.damage(t, id, dealt, element::NONE, true) > 0 {
            if self.objects[&t].dead && self.rng.random_range(0..4) == 0 {
                let level = self.magic_level(id, m::ELEMENTAL_SWORDS);
                if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                    let missing = p.max_mp - p.mp;
                    p.mp += missing * (10 + level * 10) / 100;
                }
                self.send_player_stats(id);
            }
            self.level_magic(id, m::ELEMENTAL_SWORDS);
        }
        true
    }

    /// Demonic Recovery: heal the demon pet by its health percent.
    pub(super) fn demonic_recovery(&mut self, id: ObjectId) {
        let Some(pet) = self.objects[&id].player().and_then(|p| {
            p.pets
                .iter()
                .copied()
                .find(|pet| self.monster_flag(*pet) == 4)
        }) else {
            return;
        };
        let (pmin, pmax, _) = self.caster_power(id, m::DEMONIC_RECOVERY);
        let power = self.roll_range(pmin, pmax);
        let o = self.objects.get_mut(&pet).unwrap();
        o.hp = (o.hp + o.max_hp * power / 100).min(o.max_hp);
        let (hp, max_hp) = (o.hp, o.max_hp);
        self.events.push((
            pet,
            ServerMessage::HealthChanged {
                id: pet,
                hp,
                max_hp,
            },
        ));
        self.level_magic(id, m::DEMONIC_RECOVERY);
    }
}
