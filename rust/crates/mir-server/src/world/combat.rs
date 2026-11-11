use super::monster_ai::has_poison;
use super::*;

impl World {
    /// Zircon `PlayerObject.Attack`: melee swing with optional attack skill.
    pub fn player_attack(&mut self, id: ObjectId, direction: Direction, attack_magic: Option<u16>) {
        if self.is_mounted(id) {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: "You cannot attack while riding.".into(),
                },
            );
            return;
        }
        let Some(o) = self.objects.get_mut(&id) else {
            return;
        };
        if o.dead || paralysed(o) || self.now < o.action_time || self.now < o.attack_time {
            let (loc, dir) = (o.location, o.direction);
            self.send_to(
                id,
                ServerMessage::MoveDenied {
                    location: loc,
                    direction: dir,
                },
            );
            return;
        }
        o.direction = direction;
        o.action_time = self.now + ATTACK_TIME;
        let aspeed = o.player().map(|p| p.attack_speed).unwrap_or(0);
        o.attack_time = self.now + attack_delay(aspeed);
        let stats = o.stats;
        let (map, loc) = (o.map, o.location);

        // Which attack skills ride on this swing (Zircon `AttackCast`), in
        // MagicType order; the last one that "casts" is the valid attack magic.
        let mut magics: Vec<u16> = Vec::new();
        let mut valid: Option<u16> = None;
        let mut toggles = Vec::new();
        let mut karma_cost = 0;
        {
            let level = o.player().map(|p| p.level).unwrap_or(1);
            let mut owned: Vec<(u16, i32, i32)> = o
                .player()
                .map(|p| {
                    p.magics
                        .iter()
                        .filter_map(|m| {
                            let def = self.data.magics.get(&m.magic)?;
                            Some((m.magic, def.need_level[0], m.cost(def)))
                        })
                        .collect()
                })
                .unwrap_or_default();
            owned.sort_by_key(|(m, _, _)| *m);
            let roll: bool = self.rng.random_range(0..5) == 0;
            let moon_roll = self.rng.random_range(0..5);
            let (o_hp, o_max_hp) = (o.hp, o.max_hp);
            let weapon_effect = o
                .player()
                .and_then(|p| p.bag.equipment.get(slot::WEAPON).and_then(|w| w.as_ref()))
                .and_then(|w| self.data.items.get(&w.info))
                .map(|i| i.effect)
                .unwrap_or(0);
            let release = o.player().and_then(|p| {
                let um = p.magics.iter().find(|m| m.magic == magic_type::RELEASE)?;
                let def = self.data.magics.get(&um.magic)?;
                (p.level >= def.need_level[0]).then(|| um.power_range(def))
            });
            let resolution = o
                .player()
                .map(|p| p.magics.iter().any(|m| m.magic == magic_type::RESOLUTION))
                .unwrap_or(false);
            let p = o.player_mut().unwrap();
            for (m, need, cost) in owned {
                if level < need {
                    continue;
                }
                match m {
                    magic_type::SWORDSMANSHIP
                    | magic_type::SPIRIT_SWORD
                    | magic_type::VINE_TREE_DANCE
                    | magic_type::DISCIPLINE
                    | magic_type::BLOODY_FLOWER => magics.push(m),
                    // Charged power attacks: consumed by the swing they were armed for.
                    magic_type::FLAMING_SWORD
                    | magic_type::DRAGON_RISE
                    | magic_type::BLADE_STORM
                    | magic_type::DEFENSIVE_BLOW
                    | magic_type::OFFENSIVE_BLOW => {
                        if attack_magic == Some(m)
                            && p.charge
                                .map(|(c, until)| c == m && self.now < until)
                                .unwrap_or(false)
                        {
                            p.charge = None;
                            toggles.push((m, false));
                            valid = Some(m);
                            magics.push(m);
                        }
                    }
                    // Augments ride along with the skill they modify.
                    magic_type::AUGMENT_DESTRUCTIVE_SURGE => {
                        if attack_magic == Some(magic_type::DESTRUCTIVE_SURGE) {
                            magics.push(m);
                        }
                    }
                    magic_type::DRAGON_WAVE => {
                        if attack_magic == Some(magic_type::FLAME_SPLASH) {
                            magics.push(m);
                        }
                    }
                    magic_type::DESTRUCTIVE_SURGE => {
                        if attack_magic == Some(m) && p.surge_on && cost <= p.mp {
                            p.mp -= cost;
                            valid = Some(m);
                            magics.push(m);
                        }
                    }
                    // Lotus combo: the armed swing pays and cools down on a hit.
                    magic_type::FULL_BLOOM
                    | magic_type::WHITE_LOTUS
                    | magic_type::RED_LOTUS
                    | magic_type::SWEET_BRIER => {
                        let cd = p
                            .magics
                            .iter()
                            .find(|x| x.magic == m)
                            .map(|x| x.cooldown_until)
                            .unwrap_or(0);
                        if attack_magic == Some(m) && self.now >= cd && cost <= p.mp {
                            p.mp -= cost;
                            valid = Some(m);
                            magics.push(m);
                        }
                    }
                    // Karma: an HP-priced strike that needs the cloak.
                    magic_type::KARMA => {
                        let cd = p
                            .magics
                            .iter()
                            .find(|x| x.magic == m)
                            .map(|x| x.cooldown_until)
                            .unwrap_or(0);
                        let cloaked = p.buffs.iter().any(|b| b.kind == buff_type::CLOAK);
                        if attack_magic == Some(m) && self.now >= cd && cloaked {
                            let mut hp_cost = o_max_hp * cost / 100;
                            if let Some((rmin, _)) = release {
                                hp_cost -= hp_cost * rmin / 100;
                                magics.push(magic_type::RELEASE);
                            }
                            if resolution {
                                magics.push(magic_type::RESOLUTION);
                            }
                            if hp_cost < o_hp {
                                karma_cost = hp_cost;
                                valid = Some(m);
                                magics.push(m);
                            }
                        }
                    }
                    // Moon charges: consumed when armed, re-armed by a roll.
                    magic_type::CALAMITY_OF_FULL_MOON | magic_type::WANING_MOON => {
                        let cloaked = p.buffs.iter().any(|b| b.kind == buff_type::CLOAK);
                        let ready = if m == magic_type::CALAMITY_OF_FULL_MOON {
                            &mut p.full_moon_ready
                        } else {
                            &mut p.waning_moon_ready
                        };
                        if *ready && attack_magic == Some(m) {
                            *ready = false;
                            toggles.push((m, false));
                            valid = Some(m);
                            magics.push(m);
                        }
                        let allowed = if m == magic_type::CALAMITY_OF_FULL_MOON {
                            !cloaked
                        } else {
                            cloaked
                        };
                        let level = p
                            .magics
                            .iter()
                            .find(|x| x.magic == m)
                            .map(|x| x.level as i32)
                            .unwrap_or(0);
                        if !*ready && allowed && moon_roll > level {
                            *ready = true;
                            toggles.push((m, true));
                        }
                    }
                    magic_type::FLAME_SPLASH => {
                        if attack_magic == Some(m) && p.flame_splash_on && cost <= p.mp {
                            p.mp -= cost;
                            valid = Some(m);
                            magics.push(m);
                        }
                    }
                    // Always-on attack skills.
                    magic_type::FATAL_BLOW | magic_type::MASSACRE => magics.push(m),
                    magic_type::DUAL_WEAPON_SKILLS => {
                        if weapon_effect == 100 {
                            magics.push(m);
                        }
                    }
                    magic_type::SLAYING => {
                        if p.slaying_charged && attack_magic == Some(m) {
                            p.slaying_charged = false;
                            toggles.push((m, false));
                            valid = Some(m);
                            magics.push(m);
                        }
                        if !p.slaying_charged && roll {
                            p.slaying_charged = true;
                            toggles.push((m, true));
                        }
                    }
                    magic_type::THRUSTING | magic_type::HALF_MOON => {
                        let on = if m == magic_type::THRUSTING {
                            p.thrusting_on
                        } else {
                            p.half_moon_on
                        };
                        if attack_magic == Some(m) && on && cost <= p.mp {
                            p.mp -= cost;
                            valid = Some(m);
                            magics.push(m);
                        }
                    }
                    _ => {}
                }
            }
        }
        if self.dragon_blood_attack(id, attack_magic) {
            valid = Some(magic_type::DRAGON_BLOOD);
            magics.push(magic_type::DRAGON_BLOOD);
        }
        for (m, on) in toggles {
            self.send_to(id, ServerMessage::MagicToggle { magic: m, on });
        }
        if karma_cost > 0 && attack_magic == valid {
            let o = self.objects.get_mut(&id).unwrap();
            o.hp -= karma_cost;
            let (hp, max_hp) = (o.hp, o.max_hp);
            self.events
                .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
        }
        if attack_magic != valid {
            // Zircon logs and resyncs; the swing does not happen.
            self.send_to(
                id,
                ServerMessage::MoveDenied {
                    location: loc,
                    direction,
                },
            );
            return;
        }
        // Shuriken users throw instead of swinging.
        if valid.is_none() && self.shuriken_attack(id, direction, map, loc, stats) {
            return;
        }
        let power = self.roll_dc(stats);
        self.events.push((
            id,
            ServerMessage::ObjectAttack {
                id,
                direction,
                attack_magic: valid,
                element: stats.attack_element,
            },
        ));
        // Dragon Rise lands every hit of the swing at 600 ms instead of 300.
        let delay = if magics.contains(&magic_type::DRAGON_RISE) {
            600
        } else {
            300
        };
        let front = loc.step(direction, 1);
        self.pending_hits.push(PendingHit {
            time: self.now + delay,
            attacker: id,
            target_cell: (map, front),
            power,
            target: None,
            magics: magics.clone(),
            primary: true,
            raw: false,
            element: element::NONE,
            ranged: false,
        });
        // Secondary cells (Zircon `SecondaryAttackLocation`).
        let extra: Vec<Point> = match valid {
            Some(magic_type::THRUSTING) => vec![loc.step(direction, 2)],
            Some(magic_type::HALF_MOON) | Some(magic_type::DRAGON_RISE) => vec![
                loc.step(direction.rotate(-1), 1),
                loc.step(direction.rotate(1), 1),
                loc.step(direction.rotate(2), 1),
            ],
            Some(magic_type::DESTRUCTIVE_SURGE) => {
                (1..8).map(|i| loc.step(direction.rotate(i), 1)).collect()
            }
            // Flame Splash: up to four of the other seven directions that hold a monster.
            Some(magic_type::FLAME_SPLASH) => {
                let mut dirs: Vec<Point> = (1..8)
                    .map(|i| loc.step(direction.rotate(i), 1))
                    .filter(|c| {
                        self.maps[&map]
                            .objects_at(*c)
                            .iter()
                            .any(|v| self.objects[v].is_monster() && !self.objects[v].dead)
                    })
                    .collect();
                let mut picked = Vec::new();
                // Dragon Wave at level 3 sweeps every direction.
                let max_dirs = if self
                    .objects
                    .get(&id)
                    .and_then(|o| o.player())
                    .and_then(|p| p.magics.iter().find(|m| m.magic == magic_type::DRAGON_WAVE))
                    .map(|m| m.level >= 3)
                    .unwrap_or(false)
                {
                    8
                } else {
                    4
                };
                while !dirs.is_empty() && picked.len() < max_dirs {
                    let i = self.rng.random_range(0..dirs.len());
                    picked.push(dirs.swap_remove(i));
                }
                picked
            }
            _ => Vec::new(),
        };
        for cell in extra {
            self.pending_hits.push(PendingHit {
                time: self.now + delay,
                attacker: id,
                target_cell: (map, cell),
                power,
                target: None,
                magics: magics.clone(),
                primary: false,
                raw: false,
                element: element::NONE,
                ranged: false,
            });
        }
        // Any swing breaks the cloak.
        if self.objects[&id].has_buff(buff_type::CLOAK) {
            self.buff_remove(id, buff_type::CLOAK);
        }
        // Lotus chain (Zircon `AttackLocationSuccess`): on a target in front,
        // the used lotus cools down and the next one in the chain opens.
        if let Some(m) = valid.filter(|m| magic_type::is_armed(*m)) {
            let hit_something = self.maps[&map]
                .objects_at(front)
                .iter()
                .any(|v| self.objects[v].is_monster() && !self.objects[v].dead);
            if hit_something {
                let swing = attack_delay(aspeed);
                let own = self
                    .data
                    .magics
                    .get(&m)
                    .map(|d| d.delay.max(0) as u64)
                    .unwrap_or(0);
                let locks: Vec<(u16, u64)> = match m {
                    magic_type::FULL_BLOOM => {
                        vec![(m, own), (magic_type::RED_LOTUS, swing * 3 / 2)]
                    }
                    magic_type::WHITE_LOTUS => {
                        vec![(magic_type::FULL_BLOOM, swing * 3 / 2), (m, own)]
                    }
                    magic_type::RED_LOTUS => vec![
                        (magic_type::FULL_BLOOM, swing * 3 / 2),
                        (magic_type::WHITE_LOTUS, swing * 3 / 2),
                        (m, own),
                    ],
                    magic_type::SWEET_BRIER => vec![
                        (magic_type::WHITE_LOTUS, swing * 3 / 2),
                        (magic_type::RED_LOTUS, swing * 3 / 2),
                        (m, own),
                    ],
                    // Karma: locks Summon Puppet too and stops item use for 10 s.
                    _ => {
                        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                            p.use_item_time = self.now + 10_000;
                        }
                        vec![(m, own), (magic_type::SUMMON_PUPPET, own)]
                    }
                };
                self.send_to(
                    id,
                    ServerMessage::MagicToggle {
                        magic: m,
                        on: false,
                    },
                );
                for (lm, ms) in locks {
                    if let Some(um) = self
                        .objects
                        .get_mut(&id)
                        .and_then(|o| o.player_mut())
                        .and_then(|p| p.magics.iter_mut().find(|x| x.magic == lm))
                    {
                        um.cooldown_until = self.now + ms;
                        self.send_to(
                            id,
                            ServerMessage::MagicCooldown {
                                magic: lm,
                                delay_ms: ms as u32,
                            },
                        );
                    }
                }
            }
        }
        if valid.is_some() {
            self.send_player_stats(id);
        }
    }

    pub(super) fn roll_dc(&mut self, s: CombatStats) -> i32 {
        self.roll_lucky(s.min_dc, s.max_dc, s.luck)
    }

    /// Zircon `MapObject.GetDC/GetMC/GetSC`: luck favours range endpoints.
    pub(super) fn roll_lucky(&mut self, min: i32, max: i32, luck: i32) -> i32 {
        if min >= max {
            return max;
        }
        if luck > 0 && self.rng.random_range(0..10) < luck {
            return max;
        }
        if luck < 0 && luck < -self.rng.random_range(0..10) {
            return min;
        }
        self.rng.random_range(min.max(0)..=max)
    }

    pub(super) fn roll_ac(&mut self, s: CombatStats) -> i32 {
        if s.min_ac >= s.max_ac {
            s.max_ac
        } else {
            self.rng.random_range(s.min_ac..=s.max_ac)
        }
    }

    // ---- combat resolution ------------------------------------------------

    pub(super) fn resolve_hits(&mut self) {
        let due: Vec<PendingHit> = {
            let (due, later): (Vec<_>, Vec<_>) = self
                .pending_hits
                .drain(..)
                .partition(|h| h.time <= self.now);
            self.pending_hits = later;
            due
        };
        self.resolve_hit_batch(due);
    }

    pub(super) fn resolve_hit_batch(&mut self, due: Vec<PendingHit>) {
        for hit in due {
            let Some(attacker) = self.objects.get(&hit.attacker) else {
                continue;
            };
            if attacker.dead {
                continue;
            }
            let attacker_stats = attacker.stats;
            let attacker_is_player = attacker.is_player();
            let attacker_is_monster = attacker.is_monster();
            let targets: Vec<ObjectId> = match hit.target {
                Some(t) => vec![t],
                None => self
                    .maps
                    .get(&hit.target_cell.0)
                    .map(|m| m.objects_at(hit.target_cell.1).to_vec())
                    .unwrap_or_default(),
            };
            for tid in targets {
                let Some(target) = self.objects.get(&tid) else {
                    continue;
                };
                if target.dead || tid == hit.attacker {
                    continue;
                }
                if attacker_is_monster {
                    if !self.monster_hostile_to(hit.attacker, target) {
                        continue;
                    }
                } else if !self.objects[&hit.attacker].hostile_to(target) {
                    continue;
                }
                if hit.target.is_some()
                    && !hit.ranged
                    && target
                        .location
                        .distance(self.objects[&hit.attacker].location)
                        > 1
                {
                    continue; // target walked away before the swing landed
                }
                let tstats = target.stats;
                let target_is_player = target.is_player();
                if hit.raw {
                    // Blade Storm's second half: damage was settled already.
                    self.damage(tid, hit.attacker, hit.power, element::NONE, false);
                    continue;
                }
                let lotus = hit
                    .magics
                    .iter()
                    .copied()
                    .find(|m| magic_type::is_lotus(*m) || *m == magic_type::SWEET_BRIER);
                let karma = hit.magics.contains(&magic_type::KARMA);
                // Resolution: Karma swings gain accuracy and pierce armour by its power.
                let resolution_pct = if karma {
                    self.objects[&hit.attacker]
                        .player()
                        .and_then(|p| p.magics.iter().find(|m| m.magic == magic_type::RESOLUTION))
                        .map(|m| m.power_range(&self.data.magics[&m.magic]).0)
                        .unwrap_or(0)
                } else {
                    0
                };
                // Hit chance: Random.Next(Agility) > Accuracy => dodge (lotus never misses).
                let roll = if tstats.agility > 0 {
                    self.rng.random_range(0..tstats.agility)
                } else {
                    0
                };
                // Abyss halves a monster's chance to act on its swing.
                if !attacker_is_player
                    && has_poison(&self.objects[&hit.attacker], poison_kind::ABYSS)
                    && self.rng.random_range(0..2) > 0
                {
                    continue;
                }
                let elemental = hit.element != element::NONE;
                let sure_hit = elemental
                    || lotus.is_some()
                    || hit.magics.contains(&magic_type::SWIFT_BLADE)
                    || hit.magics.contains(&magic_type::SHURIKEN);
                let accuracy =
                    attacker_stats.accuracy + attacker_stats.accuracy * resolution_pct / 100;
                if !sure_hit && roll > accuracy {
                    continue;
                }
                let mut power = hit.power;
                // Attack skill modifiers (Zircon `ModifyPowerAdditionner`).
                for m in &hit.magics {
                    let Some(def) = self.data.magics.get(m) else {
                        continue;
                    };
                    let Some(um) = self.objects[&hit.attacker]
                        .player()
                        .and_then(|p| p.magics.iter().find(|x| x.magic == *m))
                    else {
                        continue;
                    };
                    let (pmin, pmax) = um.power_range(def);
                    let mp = if pmin >= pmax {
                        pmin
                    } else {
                        self.rng.random_range(pmin..=pmax)
                    };
                    match *m {
                        magic_type::SLAYING => power += mp,
                        magic_type::THRUSTING | magic_type::HALF_MOON if !hit.primary => {
                            power = power * mp / 100;
                        }
                        magic_type::FLAMING_SWORD
                        | magic_type::DRAGON_RISE
                        | magic_type::BLADE_STORM
                        | magic_type::SWIFT_BLADE
                        | magic_type::OFFENSIVE_BLOW => {
                            power = power * mp / 100;
                        }
                        magic_type::AUGMENT_DESTRUCTIVE_SURGE => power += mp,
                        magic_type::DRAGON_WAVE => power += power * mp / 100,
                        magic_type::DESTRUCTIVE_SURGE | magic_type::FLAME_SPLASH
                            if !hit.primary =>
                        {
                            power = power * mp / 100;
                        }
                        magic_type::CALAMITY_OF_FULL_MOON | magic_type::WANING_MOON => power += mp,
                        // Fatal Blow: an execute chance under 30 % HP.
                        magic_type::FATAL_BLOW => {
                            let (thp, tmax) = {
                                let t = &self.objects[&tid];
                                (t.hp, t.max_hp)
                            };
                            let level = um.level as i32;
                            if thp < tmax * 30 / 100
                                && self
                                    .rng
                                    .random_range(0..(crate::magic::MAGIC_MAX_LEVEL as i32 + 1))
                                    <= level
                            {
                                power += power * mp / 100;
                                self.level_magic(hit.attacker, magic_type::FATAL_BLOW);
                            }
                        }
                        magic_type::DUAL_WEAPON_SKILLS => power += power * mp / 100,
                        magic_type::KARMA => {
                            power += self.roll_dc(attacker_stats);
                            // The percent-HP execute: quartered on monsters, flat on bosses.
                            let (thp, boss) = {
                                let t = &self.objects[&tid];
                                let boss = matches!(&t.kind, Kind::Monster(m) if self.data.monsters[&m.def].is_boss);
                                (t.hp, boss)
                            };
                            let execute = if boss { mp * 20 } else { thp * mp / 100 / 4 };
                            if execute > 0 {
                                self.damage(tid, hit.attacker, execute, element::NONE, false);
                            }
                            let tloc = self.objects[&tid].location;
                            self.events.push((
                                tid,
                                ServerMessage::ObjectEffect {
                                    id: tid,
                                    effect: effect::KARMA,
                                    location: tloc,
                                },
                            ));
                        }
                        m if magic_type::is_lotus(m) || m == magic_type::SWEET_BRIER => {
                            // Zircon lotus: 2x DC minus one AC roll, plus a mana-scaled
                            // bonus minus MR; the combo buff triples the bonus.
                            let (max_mp, max_dc, combo) = {
                                let a = &self.objects[&hit.attacker];
                                let p = a.player().unwrap();
                                let prev = match m {
                                    magic_type::WHITE_LOTUS => Some(buff_type::FULL_BLOOM),
                                    magic_type::RED_LOTUS => Some(buff_type::WHITE_LOTUS),
                                    magic_type::SWEET_BRIER => Some(buff_type::RED_LOTUS),
                                    _ => None,
                                };
                                (
                                    p.max_mp,
                                    a.stats.max_dc,
                                    prev.map(|b| a.has_buff(b)).unwrap_or(false),
                                )
                            };
                            let mut bonus = max_mp * mp / 1000;
                            let ac = self.roll_ac(tstats);
                            let dc = self.roll_dc(attacker_stats);
                            power = (power - ac + dc).max(0);
                            if combo {
                                bonus *= 3;
                                power += (max_dc - 100).max(0);
                            }
                            let mr = if has_poison(&self.objects[&tid], poison_kind::MAGIC_WEAKNESS)
                            {
                                0
                            } else {
                                self.roll_range(tstats.min_mr, tstats.max_mr)
                            };
                            power += (bonus - mr).max(0);
                            let (next, fx) = match m {
                                magic_type::FULL_BLOOM => {
                                    (Some(buff_type::FULL_BLOOM), effect::FULL_BLOOM)
                                }
                                magic_type::WHITE_LOTUS => {
                                    (Some(buff_type::WHITE_LOTUS), effect::WHITE_LOTUS)
                                }
                                magic_type::RED_LOTUS => {
                                    (Some(buff_type::RED_LOTUS), effect::RED_LOTUS)
                                }
                                _ => (None, effect::SWEET_BRIER),
                            };
                            self.buff_remove(hit.attacker, buff_type::FULL_BLOOM);
                            self.buff_remove(hit.attacker, buff_type::WHITE_LOTUS);
                            self.buff_remove(hit.attacker, buff_type::RED_LOTUS);
                            if let Some(next) = next {
                                self.buff_add(hit.attacker, next, 15_000, BuffStats::default());
                            }
                            let tloc = self.objects[&tid].location;
                            self.events.push((
                                tid,
                                ServerMessage::ObjectEffect {
                                    id: tid,
                                    effect: fx,
                                    location: tloc,
                                },
                            ));
                        }
                        _ => {}
                    }
                }
                if elemental {
                    if !has_poison(&self.objects[&tid], poison_kind::MAGIC_WEAKNESS) {
                        power -= self.roll_range(tstats.min_mr, tstats.max_mr);
                    }
                    let index = usize::from(hit.element).min(7);
                    let attack = attacker_stats.element_attack[index];
                    power += if target_is_player && attack > 0 {
                        self.rng.random_range(0..=attack) * 2
                    } else {
                        attack * 2
                    };
                    power -= power * tstats.element_resist[index] / 10;
                } else if lotus.is_none() && !hit.magics.contains(&magic_type::MASSACRE) {
                    let mut ac = self.roll_ac(tstats);
                    // Defensive Mastery: the defender rolls max AC by chance.
                    let dm = self.objects[&tid]
                        .player()
                        .map(|p| p.def_mastery)
                        .unwrap_or(0);
                    if dm >= 10 || (dm > 0 && self.rng.random_range(0..10) < dm) {
                        ac = tstats.max_ac;
                    }
                    ac -= ac * resolution_pct / 100;
                    power = (power - ac).max(0);
                    let physical = tstats.element_resist[0];
                    power -= power * physical / if physical > 0 { 10 } else { 5 };
                    for index in 1..=7 {
                        let attack = attacker_stats.element_attack[index];
                        let resist = tstats.element_resist[index];
                        power += attack;
                        power -= attack * resist * if resist > 0 { 2 } else { 3 } / 10;
                    }
                }
                if power <= 0 {
                    continue;
                }
                if attacker_is_player {
                    let crit: i32 = attacker_stats.critical_chance
                        + self.objects[&hit.attacker]
                            .player()
                            .map(|p| p.buffs.iter().map(|b| b.stats.crit).sum::<i32>())
                            .unwrap_or(0);
                    if self.rng.random_range(0..100) < crit {
                        if self.objects[&tid].is_player() {
                            power = power.saturating_mul(13) / 10;
                        } else {
                            power = power.saturating_mul(
                                200_i32.saturating_add(attacker_stats.critical_damage),
                            ) / 100;
                        }
                    }
                }
                // Blade Storm: half now, half 300 ms later.
                if hit.magics.contains(&magic_type::BLADE_STORM) {
                    power /= 2;
                    self.pending_hits.push(PendingHit {
                        time: self.now + 300,
                        attacker: hit.attacker,
                        target_cell: hit.target_cell,
                        power,
                        target: Some(tid),
                        magics: Vec::new(),
                        primary: true,
                        raw: true,
                        element: element::NONE,
                        ranged: false,
                    });
                }
                let dealt = self.damage(tid, hit.attacker, power, hit.element, false);
                if dealt > 0 && !attacker_is_player {
                    self.monster_hit_poison(hit.attacker, tid);
                }
                if dealt > 0 && attacker_is_player {
                    self.melee_landed(
                        hit.attacker,
                        tid,
                        dealt,
                        hit.magics.contains(&magic_type::DRAGON_BLOOD),
                    );
                }
                // Offensive Blow: shove the target Level + 3 cells; a
                // successful shove also paralyses and silences for 3 s.
                if dealt > 0 && hit.magics.contains(&magic_type::OFFENSIVE_BLOW) {
                    let level = self.magic_level(hit.attacker, magic_type::OFFENSIVE_BLOW);
                    let dir = self.objects[&hit.attacker].direction;
                    let clevel = self.level_of(&self.objects[&hit.attacker]);
                    if self.push_allowed(tid, clevel, level)
                        && self.push_back(tid, dir, level + 3) > 0
                    {
                        for kind in [poison_kind::PARALYSIS, poison_kind::SILENCED] {
                            self.apply_poison(
                                tid,
                                Poison {
                                    kind,
                                    value: 0,
                                    ticks_left: 0,
                                    next_tick: self.now + 3000,
                                    owner: Some(hit.attacker),
                                },
                            );
                        }
                    }
                }
                if dealt > 0 && attacker_is_player {
                    // Bloody Flower life steal on the primary hit (cap 750, 1500 with a lotus).
                    let (pct, hp, max_hp) = {
                        let a = &self.objects[&hit.attacker];
                        (
                            a.player().map(|p| p.life_steal).unwrap_or(0),
                            a.hp,
                            a.max_hp,
                        )
                    };
                    if pct > 0 && hit.primary && hp < max_hp {
                        let cap = if lotus.is_some() { 1500 } else { 750 };
                        let heal = (dealt * pct / 100).min(cap);
                        if heal > 0 {
                            let a = self.objects.get_mut(&hit.attacker).unwrap();
                            a.hp = (a.hp + heal).min(a.max_hp);
                            let (hp, max_hp) = (a.hp, a.max_hp);
                            self.events.push((
                                hit.attacker,
                                ServerMessage::HealthChanged {
                                    id: hit.attacker,
                                    hp,
                                    max_hp,
                                },
                            ));
                        }
                    }
                    for m in hit.magics.clone() {
                        self.level_magic(hit.attacker, m);
                    }
                }
            }
        }
    }

    /// Zircon monster `Attack(ob, power, element)`: 1-in-`PoisonRate` chance
    /// to poison with the monster's SC as value.
    pub(super) fn monster_hit_poison(&mut self, attacker: ObjectId, target: ObjectId) {
        let ai = self.data.monsters[&self.objects[&attacker].monster_ref().def].ai;
        // Lord Ni Jae rolls paralysis separately from its green poison.
        if matches!(ai, 13 | 77) && self.rng.random_range(0..10) == 0 {
            self.apply_poison(
                target,
                Poison {
                    kind: poison_kind::PARALYSIS,
                    value: 0,
                    ticks_left: 0,
                    next_tick: self.now + 5000,
                    owner: Some(attacker),
                },
            );
        }
        let Some(hp) = self.ai_profile(ai).hit_poison else {
            return;
        };
        if hp.rate > 1 && self.rng.random_range(0..hp.rate) != 0 {
            return;
        }
        let stats = self.objects[&attacker].stats;
        let value = self.roll_lucky(stats.min_sc, stats.max_sc, stats.luck);
        let timed = !matches!(
            hp.kind,
            poison_kind::GREEN | poison_kind::RED | poison_kind::HELL_FIRE
        );
        let poison = if timed {
            Poison {
                kind: hp.kind,
                value,
                ticks_left: 0,
                next_tick: self.now + hp.ticks as u64 * hp.frequency_s as u64 * 1000,
                owner: Some(attacker),
            }
        } else {
            Poison {
                kind: hp.kind,
                value,
                ticks_left: hp.ticks,
                next_tick: self.now + hp.frequency_s as u64 * 1000,
                owner: Some(attacker),
            }
        };
        self.apply_poison(target, poison);
        if hp.kind == poison_kind::ABYSS {
            // Abyss makes a monster lose its target.
            if let Some(m) = self.objects.get_mut(&target).and_then(|o| o.monster_mut()) {
                m.target = None;
            }
        }
    }

    pub(super) fn damage(
        &mut self,
        target: ObjectId,
        attacker: ObjectId,
        power: i32,
        elem: u8,
        magic: bool,
    ) -> i32 {
        if self.objects[&attacker]
            .monster()
            .is_some_and(|m| m.owner.is_some())
            && !self.monster_hostile_to(attacker, &self.objects[&target])
        {
            return 0;
        }
        if self.objects[&target]
            .player()
            .is_some_and(|p| p.gm_mode || p.superman || p.observer)
        {
            return 0;
        }
        if self.objects[&target]
            .monster()
            .is_some_and(|m| m.mirror_element.is_some() || m.room_door.is_some())
        {
            return 0;
        }
        let now = self.now;
        // Monster class rules: untouchable guards and statues, 1-damage
        // trees, class-based mitigation.
        let mut power = power;
        if magic {
            power += self.burn_shock_bonus(attacker, elem);
        }
        // Castle guards are hurt only by enemy guilds at war, gates only
        // while closed, flags never.
        if self.castle_object_accepts_hit(attacker, target) == Some(false) {
            return 0;
        }
        // The castle lord takes one point per hit from eligible guilds only.
        if self.is_castle_lord(target) {
            match self.castle_lord_damage(attacker) {
                Some(1) => power = 1,
                _ => return 0,
            }
            let (hp, max_hp) = {
                let o = self.objects.get_mut(&target).unwrap();
                o.hp -= power;
                (o.hp, o.max_hp)
            };
            self.events.push((
                target,
                ServerMessage::HealthChanged {
                    id: target,
                    hp,
                    max_hp,
                },
            ));
            if hp <= 0 {
                self.castle_lord_died(Some(attacker));
                self.monster_die(target, attacker);
            }
            return power;
        }
        if let Some((ai, hidden)) = self.objects.get(&target).and_then(|o| match &o.kind {
            Kind::Monster(m) => Some((self.data.monsters[&m.def].ai, m.hidden)),
            _ => None,
        }) {
            if matches!(ai, 115 | 116)
                && self.on_map(self.objects[&target].map).any(|o| {
                    o.monster().is_some_and(|m| {
                        let flag = self.data.monsters[&m.def].flag;
                        flag == 191 || (ai == 115 && flag == 190)
                    })
                })
            {
                return 0;
            }
            let prof = self.ai_profile(ai);
            if prof.invulnerable && (prof.guard || hidden || prof.hidden.is_none()) {
                return 0;
            }
            if prof.clamp_damage && power > 1 {
                power = 1;
            }
            // Doom Claw only feels attackers within 10 cells.
            if prof.doom_claw
                && self.objects[&attacker]
                    .location
                    .distance(self.objects[&target].location)
                    > 10
            {
                return 0;
            }
            if let Some(mit) = prof.class_mitigation {
                if let Some(class) = self.objects[&attacker]
                    .player()
                    .map(|p| p.class.mir_class())
                {
                    power = power * mit[(class as usize).min(3)] / 100;
                }
            }
        }
        // Player defences: Invincibility, Evasion (elemental), immunities,
        // Last Stand below 30 % HP, Superior Magic Shield pool.
        if let Some(p) = self.objects.get(&target).and_then(|o| o.player()) {
            if p.buffs.iter().any(|b| b.stats.invincible) {
                return 0;
            }
            let stats = self.objects[&target].stats;
            let chance = if elem == element::NONE {
                stats.block_chance
            } else {
                stats.evasion_chance + p.buffs.iter().map(|b| b.stats.evasion).sum::<i32>()
            };
            let sides = if self.objects[&attacker].player().is_some() {
                200
            } else {
                100
            };
            if self.rng.random_range(0..sides) <= chance {
                return 0;
            }
            let immunity = if elem == element::NONE {
                p.phys_immunity
            } else {
                p.magic_immunity
            };
            power -= power * immunity / 100;
            let (hp, max_hp) = (self.objects[&target].hp, self.objects[&target].max_hp);
            if p.last_stand > 0 && elem == element::NONE && hp * 100 / max_hp.max(1) < 30 {
                power -= power * p.last_stand / 100;
            }
            if power <= 0 {
                return 0;
            }
        }
        if self.objects[&target].has_buff(buff_type::SUPERIOR_MAGIC_SHIELD) {
            let remaining = {
                let p = self.objects.get_mut(&target).unwrap().player_mut().unwrap();
                let b = p
                    .buffs
                    .iter_mut()
                    .find(|b| b.kind == buff_type::SUPERIOR_MAGIC_SHIELD)
                    .unwrap();
                let absorbed = power.max(0).min(b.stats.pool);
                b.stats.pool -= absorbed;
                power -= absorbed;
                b.stats.pool
            };
            if remaining <= 0 {
                self.buff_remove(target, buff_type::SUPERIOR_MAGIC_SHIELD);
            }
            if power <= 0 {
                return 0;
            }
        }
        let mut power = if self.objects[&target]
            .poisons
            .iter()
            .any(|p| p.kind == poison_kind::RED)
        {
            power * 12 / 10 // Red poison: +20 % damage taken
        } else {
            power
        };
        // Magic Shield: 25 ms of duration per point, then 50 % absorbed.
        if self.objects[&target].has_buff(buff_type::MAGIC_SHIELD) {
            let drain = power.max(0) as u64 * 25;
            let remaining = {
                let p = self.objects.get_mut(&target).unwrap().player_mut().unwrap();
                let b = p
                    .buffs
                    .iter_mut()
                    .find(|b| b.kind == buff_type::MAGIC_SHIELD)
                    .unwrap();
                b.expires = b.expires.saturating_sub(drain);
                b.expires.saturating_sub(now)
            };
            self.send_to(
                target,
                ServerMessage::BuffTime {
                    kind: buff_type::MAGIC_SHIELD,
                    remaining_ms: remaining,
                },
            );
            power -= power * 50 / 100;
        }
        // Reflect Damage: a monster's melee hit comes back at `reflect` %.
        let reflected = if !magic && self.objects[&attacker].is_monster() {
            self.objects[&target]
                .player()
                .and_then(|p| p.buffs.iter().find(|b| b.kind == buff_type::REFLECT_DAMAGE))
                .map(|b| power.max(0) * b.stats.reflect / 100)
                .filter(|r| *r > 0)
        } else {
            None
        };
        if let Some(r) = reflected {
            self.damage(attacker, target, r, element::NONE, false);
            self.level_magic(target, magic_type::REFLECT_DAMAGE);
        }
        if let Some(m) = self.objects.get_mut(&target).and_then(|o| o.monster_mut()) {
            if m.explode_at.is_some() {
                m.explode_at = Some(now);
            }
        }
        // Blink-strikers teleport away once at half health.
        if let Some(ai) = self.objects.get(&target).and_then(|o| match &o.kind {
            Kind::Monster(m) if !m.panic_used => Some(self.data.monsters[&m.def].ai),
            _ => None,
        }) {
            let (hp, max_hp) = {
                let o = &self.objects[&target];
                (o.hp - power.max(0), o.max_hp)
            };
            if self.ai_profile(ai).panic_teleport && hp > 0 && hp <= max_hp / 2 {
                self.objects
                    .get_mut(&target)
                    .unwrap()
                    .monster_mut()
                    .unwrap()
                    .panic_used = true;
                self.teleport_nearby(target, 7, 12);
            }
        }
        // Judgement Of Heaven: a chance to answer a monster's hit with lightning.
        if self.objects[&attacker].is_monster() {
            let judgement: i32 = self.objects[&target]
                .player()
                .map(|p| p.buffs.iter().map(|b| b.stats.judgement).sum())
                .unwrap_or(0);
            if judgement > 0 && self.rng.random_range(0..100) < judgement {
                let ts = self.objects[&target].stats;
                let bolt = self.roll_lucky(ts.min_mc, ts.max_mc, ts.luck) / 5;
                if bolt > 0 {
                    let aloc = self.objects[&attacker].location;
                    self.events.push((
                        attacker,
                        ServerMessage::ObjectEffect {
                            id: attacker,
                            effect: effect::FLASH_OF_LIGHT,
                            location: aloc,
                        },
                    ));
                    self.pending_hits.push(PendingHit {
                        time: now + 300,
                        attacker: target,
                        target_cell: (self.objects[&attacker].map, aloc),
                        power: bolt,
                        target: Some(attacker),
                        magics: Vec::new(),
                        primary: false,
                        raw: true,
                        element: element::LIGHTNING,
                        ranged: true,
                    });
                }
            }
        }
        let credit = self.objects[&attacker].side().unwrap_or(attacker);
        // Idle pets of the attacker join fights allowed by their current mode.
        if self.objects[&target].is_monster() || self.objects[&target].is_player() {
            let pets: Vec<ObjectId> = self.objects[&attacker]
                .player()
                .map(|p| p.pets.clone())
                .unwrap_or_default();
            for pet in pets {
                if !self.objects.contains_key(&pet)
                    || !self.monster_hostile_to(pet, &self.objects[&target])
                {
                    continue;
                }
                if let Some(m) = self.objects.get_mut(&pet).and_then(|o| o.monster_mut()) {
                    if m.target.is_none() {
                        m.target = Some(target);
                    }
                }
            }
        }
        // Taoist summons also defend their owner after a hostile hit.
        if self.objects[&target].is_player()
            && (self.objects[&attacker].is_monster() || self.objects[&attacker].is_player())
        {
            let pets = self.objects[&target]
                .player()
                .map(|p| p.pets.clone())
                .unwrap_or_default();
            for pet in pets {
                if !self.is_reactive_summon(pet)
                    || !self.monster_hostile_to(pet, &self.objects[&attacker])
                {
                    continue;
                }
                if let Some(m) = self.objects.get_mut(&pet).and_then(|o| o.monster_mut()) {
                    if m.target.is_none() {
                        m.target = Some(attacker);
                    }
                }
            }
        }
        let (died, is_player, map, struck) = {
            let t = self.objects.get_mut(&target).unwrap();
            t.hp -= power;
            let mut struck = true;
            if let Kind::Monster(m) = &mut t.kind {
                if m.exp_owner.is_none() {
                    m.exp_owner = Some(credit);
                }
                if m.target.is_none() {
                    m.target = Some(attacker);
                }
                m.shock_until = 0;
                struck = now > m.struck_time + 300;
                if struck {
                    m.struck_time = now;
                }
            }
            (t.hp <= 0, t.is_player(), t.map, struck)
        };
        let _ = map;
        if struck {
            self.events.push((
                target,
                ServerMessage::ObjectStruck {
                    id: target,
                    attacker,
                    damage: power,
                    element: elem,
                    magic,
                },
            ));
        }
        let (hp, max_hp) = {
            let t = &self.objects[&target];
            (t.hp.max(0), t.max_hp)
        };
        self.events.push((
            target,
            ServerMessage::HealthChanged {
                id: target,
                hp,
                max_hp,
            },
        ));
        // Celestial Light: survive the killing blow once at a percent of max HP.
        let celestial = self.objects[&target]
            .player()
            .and_then(|p| {
                p.buffs
                    .iter()
                    .find(|b| b.kind == buff_type::CELESTIAL_LIGHT)
            })
            .map(|b| b.stats.celestial);
        if let (true, true, Some(pct)) = (died, is_player, celestial) {
            let o = self.objects.get_mut(&target).unwrap();
            o.hp = (o.max_hp * pct / 100).max(1);
            let (hp, max_hp) = (o.hp, o.max_hp);
            self.buff_remove(target, buff_type::CELESTIAL_LIGHT);
            if let Some(um) = self
                .objects
                .get_mut(&target)
                .and_then(|o| o.player_mut())
                .and_then(|p| {
                    p.magics
                        .iter_mut()
                        .find(|m| m.magic == magic_type::CELESTIAL_LIGHT)
                })
            {
                um.cooldown_until = now + 6000;
            }
            self.send_to(
                target,
                ServerMessage::MagicCooldown {
                    magic: magic_type::CELESTIAL_LIGHT,
                    delay_ms: 6000,
                },
            );
            self.events.push((
                target,
                ServerMessage::HealthChanged {
                    id: target,
                    hp,
                    max_hp,
                },
            ));
            return power;
        }
        if is_player {
            self.check_brown(attacker, target);
            if power > 0 {
                self.frost_store(target, attacker, power);
                self.channel_cancel(target);
            }
        }
        // A cursed doll passes its suffering on to its victim.
        if let Some(victim) = self.doll_link(target) {
            self.damage(victim, attacker, power, elem, magic);
        }
        if died {
            if is_player {
                self.pvp_kill(target, attacker);
                self.player_die(target);
            } else {
                self.monster_die(target, attacker);
            }
        }
        power
    }

    pub(super) fn monster_die(&mut self, id: ObjectId, _killer: ObjectId) {
        let (exp, owner, spawn, pet_of, revives) = {
            let o = self.objects.get_mut(&id).unwrap();
            o.dead = true;
            o.hp = 0;
            let m = o.monster_mut().unwrap();
            // Castle parts stay as wrecks until the owners repair them.
            m.dead_time = if m.castle.is_some() {
                u64::MAX
            } else {
                self.now + DEAD_DURATION
            };
            m.target = None;
            // Voracious ghosts yield half the experience per revive left.
            let exp = m.experience / 2f64.powi(m.revives_left);
            (exp, m.exp_owner, m.spawn, m.owner, m.revives_left)
        };
        // The reward owner may have disconnected before the monster died.
        let owner = owner.filter(|owner| self.objects.contains_key(owner));
        if revives > 0 {
            // The ghost gets up again in 3-7 s; no drops until its last death.
            let revive_at = self.now + 3000 + self.rng.random_range(0..5000);
            let m = self.objects.get_mut(&id).unwrap().monster_mut().unwrap();
            m.revive_at = revive_at;
            m.dead_time = revive_at + DEAD_DURATION;
            m.death_count += 1;
            self.events.push((id, ServerMessage::ObjectDie { id }));
            if let Some(owner) = owner {
                if self.objects.get(&owner).is_some_and(|o| !o.dead) {
                    self.gain_combat_experience(owner, exp as u64);
                }
            }
            return;
        }
        if let Some(pet_of) = pet_of {
            // Pets yield no experience or drops.
            if let Some(p) = self.objects.get_mut(&pet_of).and_then(|o| o.player_mut()) {
                p.pets.retain(|x| *x != id);
            }
            self.events.push((id, ServerMessage::ObjectDie { id }));
            return;
        }
        if let Some((map, gi)) = spawn {
            if let Some(g) = self.maps.get_mut(&map).and_then(|m| m.spawns.get_mut(gi)) {
                g.alive -= 1;
            }
        }
        self.events.push((id, ServerMessage::ObjectDie { id }));
        self.monster_death_effects(id);
        if let Some(owner) = owner {
            let (def, map, loc) = {
                let o = &self.objects[&id];
                (o.monster_ref().def, o.map, o.location)
            };
            // Zircon `YieldReward`: group members on the map within view
            // range share drops; the living ones split the experience
            // (+6% per member) by level.
            let growth = self.data.monsters[&def]
                .stat(stat::GROWTH_LEVEL)
                .clamp(0, 3);
            let sharers = self.group_sharers(owner, map, loc);
            let alive: Vec<ObjectId> = sharers
                .iter()
                .copied()
                .filter(|m| !self.objects[m].dead)
                .collect();
            if alive.is_empty() {
                let o = &self.objects[&owner];
                if !o.dead && o.map == map && o.location.distance(loc) <= MAX_VIEW_RANGE {
                    self.gain_combat_experience(owner, exp as u64);
                    self.gain_discipline_experience(owner, growth);
                }
            } else {
                let total: i64 = alive
                    .iter()
                    .map(|m| self.objects[m].player().unwrap().level as i64)
                    .sum();
                let mut shared = exp;
                if alive.len() > 1 {
                    shared += shared * 0.06 * alive.len() as f64;
                }
                for m in alive {
                    let level = self.objects[&m].player().unwrap().level as f64;
                    self.gain_combat_experience(m, (shared * level / total.max(1) as f64) as u64);
                    self.gain_discipline_experience(m, growth);
                }
            }
            self.quest_kill_progress(owner, def, map);
            if sharers.is_empty() {
                self.drop_loot(id, Some(owner), 1);
            } else {
                let n = sharers.len();
                for m in sharers {
                    self.drop_loot(id, Some(m), n);
                }
            }
        } else {
            self.drop_loot(id, None, 1);
        }
        self.legacy_event_monster_died(id, owner);
        self.world_event_boss_died(id, owner);
        if self.needs_harvest(id) {
            if let Some(m) = self.objects.get_mut(&id).and_then(|o| o.monster_mut()) {
                if !m.harvest_drops.is_empty() {
                    m.dead_time += super::harvest::HARVEST_DURATION;
                }
            }
        }
    }

    pub(super) fn roll_range(&mut self, min: i32, max: i32) -> i32 {
        if min >= max {
            max
        } else {
            self.rng.random_range(min..=max)
        }
    }
}
