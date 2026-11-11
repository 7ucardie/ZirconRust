//! Skills wave four: self-centred bursts, single-target spells, cell areas
//! and self buffs for all four classes. Rules from
//! `docs/research/skills-remaining.md` (formulas quoted there).
//!
//! `PendingMagic.chain` carries `(extra, [centre])`: `extra` is the
//! distance from the burst centre (Taecheon Sword) or a pass index.

use super::monster_ai::has_poison;
use super::*;
use mir_proto::magic_type as m;

/// Magics whose cast and landing live in this module.
pub(super) fn handled(magic: u16) -> bool {
    matches!(
        magic,
        m::SEISMIC_SLAM
            | m::TAECHEON_SWORD
            | m::FIRE_SWORD
            | m::THUNDER_STRIKE
            | m::ICE_BREAKER
            | m::FROZEN_DRAGON
            | m::HEAVENLY_SKY
            | m::POISON_CLOUD
            | m::FOUR_WHEELS
            | m::CRESCENT_MOON
            | m::FLASH_OF_LIGHT
            | m::ICE_DRAGON
            | m::SEARING_LIGHT
            | m::HEMORRHAGE
            | m::ABYSS
            | m::CONTAINMENT
            | m::NEUTRALIZE
            | m::PARASITE
            | m::ICE_RAIN
            | m::ASTEROID
            | m::INVINCIBILITY
            | m::EVASION
            | m::RAGING_WIND
            | m::CONCENTRATION
            | m::THE_NEW_BEGINNING
            | m::JUDGEMENT_OF_HEAVEN
            | m::SUPERIOR_MAGIC_SHIELD
            | m::DARK_CONVERSION
            | m::LIFE_STEAL
            | m::SPIRITUALISM
    )
}

/// Cells within `radius` (Chebyshev) of `centre`, nearest first.
fn disc(centre: Point, radius: i32) -> Vec<Point> {
    let mut cells: Vec<Point> = (-radius..=radius)
        .flat_map(|dx| (-radius..=radius).map(move |dy| Point::new(centre.x + dx, centre.y + dy)))
        .collect();
    cells.sort_by_key(|c| (c.distance(centre), c.y, c.x));
    cells
}

impl World {
    /// Cast-time work for a wave-four magic. Returns `cast_ok`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn wave4_cast(
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
        let mk = |time: u64, target: Option<ObjectId>, cell: Point, extra: i32, centre: Point| {
            PendingMagic {
                time,
                caster: id,
                magic,
                target,
                location: cell,
                direction: Some(direction),
                primary: true,
                chain: Some((extra, vec![centre])),
            }
        };
        let dist_ms = |w: &World, t: ObjectId| w.objects[&t].location.distance(loc) as u64 * 48;
        match magic {
            // ---- self-centred bursts ----
            m::SEISMIC_SLAM => {
                let centre = loc.step(direction, 3);
                locations.push(centre);
                for cell in disc(centre, 3) {
                    pending.push(mk(now + 600, None, cell, cell.distance(centre), centre));
                }
            }
            m::TAECHEON_SWORD
            | m::HEAVENLY_SKY
            | m::FOUR_WHEELS
            | m::POISON_CLOUD
            | m::CRESCENT_MOON => {
                let (radius, delay) = match magic {
                    m::CRESCENT_MOON => (3, 1500),
                    m::HEAVENLY_SKY => (2, 1000),
                    m::POISON_CLOUD => (2, 2500),
                    _ => (2, 1500),
                };
                locations.push(loc);
                for cell in disc(loc, radius) {
                    pending.push(mk(now + delay, None, cell, cell.distance(loc), loc));
                }
            }
            m::FIRE_SWORD => {
                locations.push(loc);
                for (i, cell) in disc(loc, 2).into_iter().enumerate() {
                    pending.push(mk(now + 1300 + 100 * i as u64, None, cell, 0, loc));
                }
            }
            m::THUNDER_STRIKE => {
                let radius = if level > 3 { 6 } else { 3 };
                locations.push(loc);
                for cell in disc(loc, radius) {
                    pending.push(mk(now + 500, None, cell, 0, loc));
                }
            }
            m::ICE_BREAKER => {
                locations.push(loc);
                for cell in disc(loc, 2) {
                    let d = cell.distance(loc) as u64;
                    pending.push(mk(now + 500 + 500 * d, None, cell, 0, loc));
                }
            }
            m::FROZEN_DRAGON => {
                locations.push(loc);
                for wave in 1..=2u64 {
                    for cell in disc(loc, 2) {
                        let d = cell.distance(loc) as u64;
                        pending.push(mk(now + 500 + 500 * d * wave, None, cell, wave as i32, loc));
                    }
                }
            }
            m::FLASH_OF_LIGHT => {
                for i in 1..=2 {
                    let cell = loc.step(direction, i);
                    pending.push(mk(now + 400, None, cell, 0, loc));
                }
                locations.push(loc.step(direction, 1));
            }
            // ---- single targets ----
            m::ICE_DRAGON | m::SEARING_LIGHT | m::HEMORRHAGE | m::PARASITE => {
                let Some(t) = target.filter(|t| self.objects[t].is_monster()) else {
                    locations.push(location);
                    return false;
                };
                targets.push(t);
                let delay = 500 + dist_ms(self, t);
                pending.push(mk(now + delay, Some(t), location, 0, loc));
            }
            m::NEUTRALIZE => {
                let Some(t) = target.filter(|t| self.objects[t].is_monster()) else {
                    locations.push(location);
                    return false;
                };
                if self.use_amulet(id, 1).is_none() {
                    return false;
                }
                targets.push(t);
                let delay = 1400 + dist_ms(self, t);
                pending.push(mk(now + delay, Some(t), location, 0, loc));
            }
            m::ABYSS => {
                let Some(t) = target.filter(|t| self.objects[t].is_monster()) else {
                    locations.push(location);
                    return false;
                };
                let boss = matches!(&self.objects[&t].kind, Kind::Monster(mo) if self.data.monsters[&mo.def].is_boss);
                if boss {
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: "That creature cannot be cast into the abyss.".into(),
                        },
                    );
                    return false;
                }
                targets.push(t);
                pending.push(mk(now + 500, Some(t), location, 0, loc));
            }
            m::CONTAINMENT => {
                let count = (1 + level) as usize;
                let mut pool: Vec<ObjectId> = {
                    let me = &self.objects[&id];
                    self.on_map(me.map)
                        .filter(|o| o.is_monster() && !o.dead && me.hostile_to(o))
                        .filter(|o| o.location.distance(loc) <= 3)
                        .map(|o| o.id)
                        .collect()
                };
                while !pool.is_empty() && targets.len() < count {
                    let i = self.rng.random_range(0..pool.len());
                    let t = pool.swap_remove(i);
                    targets.push(t);
                    pending.push(mk(now + 500, Some(t), location, 0, loc));
                }
                if targets.is_empty() {
                    return false;
                }
            }
            // ---- cell areas ----
            m::ICE_RAIN => {
                if location.distance(loc) > MAGIC_RANGE {
                    return false;
                }
                locations.push(location);
                let mut idx = 0u64;
                for _ in 0..level.max(1) {
                    for cell in disc(location, 3) {
                        if self.rng.random_range(0..10) > 2 {
                            continue;
                        }
                        pending.push(mk(now + 980 + idx * 200, None, cell, 0, location));
                        idx += 1;
                    }
                }
            }
            m::ASTEROID => {
                if location.distance(loc) > MAGIC_RANGE {
                    return false;
                }
                locations.push(location);
                for cell in disc(location, 3) {
                    pending.push(mk(now + 1200, None, cell, 0, location));
                }
            }
            m::LIFE_STEAL => {
                if location.distance(loc) > MAGIC_RANGE || self.use_amulet(id, 2).is_none() {
                    return false;
                }
                locations.push(location);
                let allies: Vec<ObjectId> = self
                    .on_map(self.objects[&id].map)
                    .filter(|o| o.is_player() && !o.dead && o.location.distance(location) <= 3)
                    .map(|o| o.id)
                    .collect();
                for a in allies {
                    let d = self.objects[&a].location.distance(location) as u64 * 48;
                    targets.push(a);
                    pending.push(mk(now + 500 + d, Some(a), location, 0, loc));
                }
            }
            // ---- self buffs ----
            m::SPIRITUALISM => {
                if self.use_amulet(id, 1).is_none() {
                    return false;
                }
                pending.push(mk(now + 500, None, loc, 0, loc));
            }
            m::DARK_CONVERSION => {
                if self.objects[&id].has_buff(buff_type::DARK_CONVERSION) {
                    self.buff_remove(id, buff_type::DARK_CONVERSION);
                    return false;
                }
                pending.push(mk(now + 500, None, loc, 0, loc));
            }
            m::SUPERIOR_MAGIC_SHIELD => pending.push(mk(now + 1100, None, loc, 0, loc)),
            m::JUDGEMENT_OF_HEAVEN => pending.push(mk(now + 600, None, loc, 0, loc)),
            _ => pending.push(mk(now + 500, None, loc, 0, loc)),
        }
        true
    }

    /// Landing of a wave-four magic.
    pub(super) fn wave4_land(&mut self, pm: &PendingMagic, cmap: i32, cloc: Point) {
        let (extra, centre) = match &pm.chain {
            Some((e, c)) => (*e, c.first().copied().unwrap_or(cloc)),
            None => (0, cloc),
        };
        let caster = pm.caster;
        let level = self.magic_level(caster, pm.magic);
        let (pmin, pmax, _) = self.caster_power(caster, pm.magic);
        let cs = self.objects[&caster].stats;
        let power = self.roll_range(pmin, pmax);
        let dc = self.roll_dc(cs);
        let sp = self.roll_lucky(cs.min_mc.min(cs.min_sc), cs.max_mc.min(cs.max_sc), cs.luck);
        // Victims: the explicit target, or hostiles standing on the cell.
        let victims: Vec<ObjectId> = match pm.target {
            Some(t) if pm.magic == m::POISON_CLOUD => self
                .objects
                .get(&t)
                .filter(|o| self.player_can_attack_target(caster, o))
                .map(|_| vec![t])
                .unwrap_or_default(),
            Some(t) => vec![t],
            None => self
                .maps
                .get(&cmap)
                .map(|mp| mp.objects_at(pm.location).to_vec())
                .unwrap_or_default()
                .into_iter()
                .filter(|v| {
                    if pm.magic == m::POISON_CLOUD {
                        self.player_can_attack_target(caster, &self.objects[v])
                    } else {
                        self.objects[&caster].hostile_to(&self.objects[v])
                    }
                })
                .collect(),
        };
        match pm.magic {
            // Physical bursts: `Player.Attack` with IgnoreAccuracy, minus AC.
            m::SEISMIC_SLAM
            | m::TAECHEON_SWORD
            | m::FIRE_SWORD
            | m::FLASH_OF_LIGHT
            | m::HEAVENLY_SKY
            | m::FOUR_WHEELS
            | m::CRESCENT_MOON => {
                for v in victims {
                    let is_player = self.objects[&v].is_player();
                    let mut p = match pm.magic {
                        m::SEISMIC_SLAM => dc * power / 100,
                        m::TAECHEON_SWORD => power + dc * (4 - extra).max(0),
                        m::FIRE_SWORD => power + dc,
                        m::FLASH_OF_LIGHT => {
                            let stacks = self.buff_stacks(caster, buff_type::THE_NEW_BEGINNING);
                            let mut p = dc * power / 100;
                            if stacks > 0 {
                                p += 80 * (stacks + 1);
                                self.buff_consume_stack(caster, buff_type::THE_NEW_BEGINNING);
                            }
                            p
                        }
                        m::HEAVENLY_SKY => power * dc,
                        _ => power * sp,
                    };
                    if is_player && matches!(pm.magic, m::SEISMIC_SLAM) {
                        p /= 2;
                    }
                    let elem = match pm.magic {
                        m::TAECHEON_SWORD | m::FIRE_SWORD => element::FIRE,
                        m::HEAVENLY_SKY => element::LIGHTNING,
                        _ => element::NONE,
                    };
                    let ts = self.objects[&v].stats;
                    let dealt = if elem == element::NONE {
                        let ac = self.roll_ac(ts);
                        p - ac
                    } else {
                        p - self.roll_range(ts.min_mr, ts.max_mr)
                    };
                    if dealt <= 0 {
                        continue;
                    }
                    let done = self.damage(v, caster, dealt, elem, true);
                    if done > 0 && pm.magic == m::SEISMIC_SLAM {
                        for (kind, ms) in [
                            (poison_kind::PARALYSIS, 3000u64),
                            (poison_kind::WRAITH_GRIP, 1500),
                            (poison_kind::SILENCED, 5000),
                        ] {
                            self.apply_poison(
                                v,
                                Poison {
                                    kind,
                                    value: 0,
                                    ticks_left: 0,
                                    next_tick: self.now + ms,
                                    owner: Some(caster),
                                },
                            );
                        }
                    }
                    if pm.magic == m::FLASH_OF_LIGHT {
                        let vloc = self.objects[&v].location;
                        self.events.push((
                            v,
                            ServerMessage::ObjectEffect {
                                id: v,
                                effect: effect::FLASH_OF_LIGHT,
                                location: vloc,
                            },
                        ));
                    }
                }
                self.level_magic(caster, pm.magic);
            }
            // Magic bursts and cell areas through `magic_attack`.
            m::THUNDER_STRIKE | m::ICE_BREAKER | m::FROZEN_DRAGON | m::ICE_RAIN | m::ASTEROID => {
                let (elem, scale) = match pm.magic {
                    m::THUNDER_STRIKE => (element::LIGHTNING, 150),
                    m::ASTEROID => (element::FIRE, 100),
                    _ => (element::ICE, 100),
                };
                for v in victims {
                    if pm.magic == m::THUNDER_STRIKE && self.rng.random_range(0..2) > 0 {
                        continue;
                    }
                    let dealt = self.magic_attack(caster, v, pm.magic, elem, scale);
                    if dealt > 0 {
                        match pm.magic {
                            m::ICE_BREAKER => self.try_slow(v, caster, 5, 5),
                            m::FROZEN_DRAGON => self.try_slow(v, caster, 2, 5),
                            _ => {}
                        }
                    }
                }
                let _ = centre;
                self.level_magic(caster, pm.magic);
            }
            m::POISON_CLOUD => {
                let duration = power + self.roll_lucky(cs.min_sc, cs.max_sc, cs.luck);
                let value = level + 1 + self.level_of(&self.objects[&caster]) / 14;
                for v in victims {
                    self.apply_poison(
                        v,
                        Poison {
                            kind: poison_kind::GREEN,
                            value,
                            ticks_left: duration / 2,
                            next_tick: self.now + 2000,
                            owner: Some(caster),
                        },
                    );
                }
                self.level_magic(caster, pm.magic);
            }
            // ---- single targets ----
            m::ICE_DRAGON => {
                if let Some(t) = pm.target {
                    if self.magic_attack(caster, t, pm.magic, element::ICE, 100) > 0 {
                        self.try_slow(t, caster, 2, 3);
                        self.level_magic(caster, pm.magic);
                    }
                }
            }
            m::SEARING_LIGHT => {
                if let Some(t) = pm.target {
                    let dealt = self.magic_attack(caster, t, pm.magic, element::HOLY, 100);
                    let tlevel = self.level_of(&self.objects[&t]);
                    let clevel = self.level_of(&self.objects[&caster]);
                    if dealt > 0 && tlevel <= clevel + 2 && self.rng.random_range(0..3) == 0 {
                        self.apply_poison(
                            t,
                            Poison {
                                kind: poison_kind::FEAR,
                                value: 0,
                                ticks_left: 0,
                                next_tick: self.now + (level as u64 + 2) * 1000,
                                owner: Some(caster),
                            },
                        );
                    }
                    if dealt > 0 {
                        self.level_magic(caster, pm.magic);
                    }
                }
            }
            m::HEMORRHAGE => {
                if let Some(t) = pm.target {
                    let boss = matches!(&self.objects[&t].kind, Kind::Monster(mo) if self.data.monsters[&mo.def].is_boss);
                    let dealt = self.magic_attack(caster, t, pm.magic, element::NONE, 100);
                    if dealt > 0 && !boss {
                        self.apply_poison(
                            t,
                            Poison {
                                kind: poison_kind::HEMORRHAGE,
                                value: sp,
                                ticks_left: (power / 2).max(1),
                                next_tick: self.now + 2000,
                                owner: Some(caster),
                            },
                        );
                        self.level_magic(caster, pm.magic);
                    }
                }
            }
            m::PARASITE => {
                if let Some(t) = pm.target {
                    if has_poison(&self.objects[&t], poison_kind::PARASITE) {
                        return;
                    }
                    self.apply_poison(
                        t,
                        Poison {
                            kind: poison_kind::PARASITE,
                            value: power,
                            ticks_left: 10 + level * 5,
                            next_tick: self.now + 2000,
                            owner: Some(caster),
                        },
                    );
                    self.magic_attack(caster, t, pm.magic, element::NONE, 50);
                    self.level_magic(caster, pm.magic);
                }
            }
            m::ABYSS => {
                if let Some(t) = pm.target {
                    if has_poison(&self.objects[&t], poison_kind::ABYSS) {
                        return;
                    }
                    let mut secs = (level + 3) * 2;
                    if self.objects[&t].is_monster() {
                        secs *= 2;
                    }
                    self.apply_poison(
                        t,
                        Poison {
                            kind: poison_kind::ABYSS,
                            value: sp,
                            ticks_left: 0,
                            next_tick: self.now + secs as u64 * 1000,
                            owner: Some(caster),
                        },
                    );
                    if let Some(mo) = self.objects.get_mut(&t).and_then(|o| o.monster_mut()) {
                        mo.target = None;
                    }
                    self.level_magic(caster, pm.magic);
                }
            }
            m::CONTAINMENT => {
                if let Some(t) = pm.target {
                    let tlevel = self.level_of(&self.objects[&t]);
                    let clevel = self.level_of(&self.objects[&caster]);
                    if tlevel > clevel + 2 {
                        return;
                    }
                    if self
                        .rng
                        .random_range(0..(crate::magic::MAGIC_MAX_LEVEL as i32 + 1))
                        > level
                    {
                        return;
                    }
                    self.apply_poison(
                        t,
                        Poison {
                            kind: poison_kind::CONTAINMENT,
                            value: sp,
                            ticks_left: 2,
                            next_tick: self.now + 2000,
                            owner: Some(caster),
                        },
                    );
                    self.level_magic(caster, pm.magic);
                }
            }
            m::NEUTRALIZE => {
                if let Some(t) = pm.target {
                    let tlevel = self.level_of(&self.objects[&t]);
                    let clevel = self.level_of(&self.objects[&caster]);
                    if tlevel >= clevel || has_poison(&self.objects[&t], poison_kind::NEUTRALIZE) {
                        return;
                    }
                    let secs = 5 + level * 2;
                    self.apply_poison(
                        t,
                        Poison {
                            kind: poison_kind::NEUTRALIZE,
                            value: 0,
                            ticks_left: 0,
                            next_tick: self.now + secs as u64 * 1000,
                            owner: Some(caster),
                        },
                    );
                    self.level_magic(caster, pm.magic);
                }
            }
            // ---- buffs ----
            m::LIFE_STEAL => {
                if let Some(t) = pm.target {
                    let secs = power + self.roll_lucky(cs.min_sc, cs.max_sc, cs.luck);
                    self.buff_add(
                        t,
                        buff_type::LIFE_STEAL,
                        secs.max(1) as u64 * 1000,
                        BuffStats {
                            life_steal: 4 + level * 2,
                            ..BuffStats::default()
                        },
                    );
                }
                self.level_magic(caster, pm.magic);
            }
            m::SPIRITUALISM => {
                let secs = power + self.roll_lucky(cs.min_sc, cs.max_sc, cs.luck) * 2;
                self.buff_add(
                    caster,
                    buff_type::SPIRITUALISM,
                    secs.max(1) as u64 * 1000,
                    BuffStats {
                        max_ac: 5 + level,
                        max_mr: 5 + level,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::INVINCIBILITY => {
                self.buff_add(
                    caster,
                    buff_type::INVINCIBILITY,
                    (5 + level as u64) * 1000,
                    BuffStats {
                        invincible: true,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::EVASION => {
                self.buff_add(
                    caster,
                    buff_type::EVASION,
                    power.max(1) as u64 * 1000,
                    BuffStats {
                        evasion: 4 + level * 2,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::RAGING_WIND => {
                self.buff_add(
                    caster,
                    buff_type::RAGING_WIND,
                    power.max(1) as u64 * 1000,
                    BuffStats {
                        raging_wind: level + 1,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::CONCENTRATION => {
                self.buff_add(
                    caster,
                    buff_type::CONCENTRATION,
                    power.max(1) as u64 * 1000,
                    BuffStats {
                        crit: 5 + level,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::THE_NEW_BEGINNING => {
                self.buff_remove(caster, buff_type::CLOAK);
                let stacks =
                    (self.buff_stacks(caster, buff_type::THE_NEW_BEGINNING) + 1).min(level + 2);
                self.buff_add(
                    caster,
                    buff_type::THE_NEW_BEGINNING,
                    60_000,
                    BuffStats {
                        stacks,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::JUDGEMENT_OF_HEAVEN => {
                self.buff_add(
                    caster,
                    buff_type::JUDGEMENT_OF_HEAVEN,
                    (30 + 30 * level as u64) * 1000,
                    BuffStats {
                        judgement: (2 + level) * 20,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::SUPERIOR_MAGIC_SHIELD => {
                if self.objects[&caster].has_buff(buff_type::SUPERIOR_MAGIC_SHIELD) {
                    return;
                }
                self.buff_remove(caster, buff_type::MAGIC_SHIELD);
                let max_mp = self.objects[&caster]
                    .player()
                    .map(|p| p.max_mp)
                    .unwrap_or(0);
                let pool = (max_mp as f32 * (0.25 + level as f32 * 0.05)) as i32;
                self.buff_add(
                    caster,
                    buff_type::SUPERIOR_MAGIC_SHIELD,
                    u64::MAX,
                    BuffStats {
                        pool,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            m::DARK_CONVERSION => {
                self.buff_add(
                    caster,
                    buff_type::DARK_CONVERSION,
                    u64::MAX,
                    BuffStats {
                        pool: power,
                        ..BuffStats::default()
                    },
                );
                self.level_magic(caster, pm.magic);
            }
            _ => {}
        }
    }

    /// Stack count carried by a buff (`BuffStats.stacks`).
    pub(super) fn buff_stacks(&self, id: ObjectId, kind: u16) -> i32 {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .and_then(|p| p.buffs.iter().find(|b| b.kind == kind))
            .map(|b| b.stats.stacks)
            .unwrap_or(0)
    }

    /// Zircon `DecreaseBuffCharge`: one stack less, gone at zero.
    pub(super) fn buff_consume_stack(&mut self, id: ObjectId, kind: u16) {
        let remaining = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.buffs.iter_mut().find(|b| b.kind == kind))
            .map(|b| {
                b.stats.stacks -= 1;
                b.stats.stacks
            });
        if remaining.map(|r| r <= 0).unwrap_or(false) {
            self.buff_remove(id, kind);
        }
    }
}
