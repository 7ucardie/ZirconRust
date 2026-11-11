use super::*;

impl World {
    /// Prefer the requested summon cell, then nearby free cells; C# falls
    /// back to the caster's occupied cell if no free cell exists.
    pub(super) fn summon_placement(&self, map: i32, preferred: Point, caster: Point) -> Point {
        for center in [preferred, caster] {
            if !self.cell_blocked(map, center, false) {
                return center;
            }
            for dir in Direction::ALL {
                let p = center.step(dir, 1);
                if !self.cell_blocked(map, p, false) {
                    return p;
                }
            }
        }
        caster
    }

    /// Zircon `PlayerObject.CanAttackTarget`, including a summoned monster's owner.
    pub(super) fn player_can_attack_target(&self, caster: ObjectId, target: &Object) -> bool {
        let Some(me) = self.objects.get(&caster) else {
            return false;
        };
        let Some(player) = me.player() else {
            return false;
        };
        let Some(monster) = target.monster() else {
            return me.hostile_to(target);
        };
        let Some(owner) = monster.owner else {
            return me.hostile_to(target);
        };
        if target.dead
            || monster.guard
            || monster.castle.is_some()
            || monster.companion.is_some()
            || monster.room_door.is_some()
            || monster.hidden
        {
            return false;
        }
        let puppet = self
            .data
            .monsters
            .get(&monster.def)
            .is_some_and(|def| def.flag == 6);
        if owner == caster {
            return player.attack_mode == mir_proto::attack_mode::ALL || puppet;
        }
        if puppet || me.in_safe_zone || target.in_safe_zone {
            return false;
        }
        let Some(other) = self.objects.get(&owner).and_then(Object::player) else {
            return false;
        };
        match player.attack_mode {
            mir_proto::attack_mode::PEACE => false,
            mir_proto::attack_mode::GROUP => player.group.is_none() || player.group != other.group,
            mir_proto::attack_mode::GUILD => player.guild.is_none() || player.guild != other.guild,
            mir_proto::attack_mode::WAR_RED_BROWN => {
                other.brown
                    || other.pk_points >= super::pvp::RED_POINT
                    || super::pvp::at_war(player, other)
            }
            _ => true,
        }
    }

    /// Zircon `PlayerObject.Magic`: validate, pay, schedule the effect,
    /// broadcast the cast.
    pub fn cast(
        &mut self,
        id: ObjectId,
        magic: u16,
        direction: Direction,
        target: Option<ObjectId>,
        location: Point,
    ) {
        let Some(def) = self.data.magics.get(&magic).cloned() else {
            return;
        };
        if self.is_mounted(id) {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: "You cannot cast while riding.".into(),
                },
            );
            return;
        }
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        let (map, loc) = (o.map, o.location);
        let Some(um) = p.magics.iter().find(|m| m.magic == magic).cloned() else {
            return;
        };
        let deny = |w: &mut World, why: &str| {
            w.send_to(id, ServerMessage::Chat { text: why.into() });
            w.send_to(
                id,
                ServerMessage::MoveDenied {
                    location: loc,
                    direction,
                },
            );
        };
        if !magic_type::is_castable(magic) {
            deny(self, "That skill cannot be cast.");
            return;
        }
        if paralysed(o) {
            deny(self, "You are stunned.");
            return;
        }
        if p.level < def.need_level[0] {
            deny(
                self,
                &format!("{} needs level {}.", def.name, def.need_level[0]),
            );
            return;
        }
        let mobile_support = magic_type::is_mobile_support_cast(magic);
        if o.dead
            || (self.now < o.action_time && !(mobile_support && self.now < o.move_time))
            || self.now < p.magic_time
        {
            deny(self, "");
            return;
        }
        if self.now < um.cooldown_until {
            deny(self, &format!("{} is still cooling down.", def.name));
            return;
        }
        // Cloak is paid in HP (see its arm); everything else in MP.
        let cost = if matches!(magic, magic_type::CLOAK | magic_type::ELEMENTAL_HURRICANE) {
            0
        } else {
            um.cost(&def)
        };
        let required_cost = if magic == magic_type::ELEMENTAL_HURRICANE && p.channel.is_none() {
            um.cost(&def)
        } else {
            cost
        };
        if required_cost > p.mp {
            deny(self, "Not enough mana.");
            return;
        }
        // Target must be visible and within magic range.
        let corpse_magic = matches!(magic, magic_type::CORPSE_EXPLODER | magic_type::SUMMON_DEAD);
        let target = target.filter(|t| {
            self.objects
                .get(t)
                .map(|to| {
                    to.map == map
                        && (!to.dead || corpse_magic)
                        && to.location.distance(loc) <= MAGIC_RANGE
                })
                .unwrap_or(false)
        });
        let mut targets: Vec<ObjectId> = Vec::new();
        let mut locations: Vec<Point> = Vec::new();
        let mut pending: Vec<PendingMagic> = Vec::new();
        let dist = |w: &World, t: ObjectId| w.objects[&t].location.distance(loc) as u64;
        let mut cast_ok = true;
        let pm =
            |time: u64, target: Option<ObjectId>, location: Point, primary: bool| PendingMagic {
                time,
                caster: id,
                magic,
                target,
                location,
                direction: None,
                primary,
                chain: None,
            };
        let now = self.now;
        match magic {
            // Projectiles: 500 ms + 48 ms per cell of distance (Cyclone: flat 600).
            magic_type::FIRE_BALL
            | magic_type::ICE_BOLT
            | magic_type::FLAMING_DAGGERS
            | magic_type::SHREDDING
            | magic_type::LIGHTNING_BALL
            | magic_type::GUST_BLAST
            | magic_type::ADAMANTINE_FIRE_BALL
            | magic_type::ICE_BLADES
            | magic_type::CYCLONE
            | magic_type::EVIL_SLAYER
            | magic_type::GREATER_EVIL_SLAYER => {
                let base = match magic {
                    magic_type::FLAMING_DAGGERS | magic_type::SHREDDING => 1000,
                    _ => 500,
                };
                match target.filter(|t| self.objects[t].is_monster()) {
                    Some(t) => {
                        targets.push(t);
                        let time = if magic == magic_type::CYCLONE {
                            now + 600
                        } else {
                            now + base + dist(self, t) * 48
                        };
                        pending.push(pm(time, Some(t), location, true));
                        // Evil Slayer burns a holy talisman when one is worn.
                        if matches!(
                            magic,
                            magic_type::EVIL_SLAYER | magic_type::GREATER_EVIL_SLAYER
                        ) && self.amulet_holy(id)
                        {
                            self.use_amulet(id, 1);
                        }
                    }
                    None => locations.push(location),
                }
            }
            magic_type::EXPLOSIVE_TALISMAN => match target.filter(|t| self.objects[t].is_monster())
            {
                Some(t) => {
                    if self.use_amulet(id, 1).is_some() {
                        targets.push(t);
                        pending.push(pm(now + 500 + dist(self, t) * 48, Some(t), location, true));
                    } else {
                        self.send_to(
                            id,
                            ServerMessage::Chat {
                                text: "You need a talisman.".into(),
                            },
                        );
                    }
                }
                None => locations.push(location),
            },
            magic_type::THUNDER_BOLT | magic_type::POISON_DUST => {
                match target.filter(|t| {
                    if magic == magic_type::POISON_DUST {
                        self.player_can_attack_target(id, &self.objects[t])
                    } else {
                        self.objects[t].is_monster()
                    }
                }) {
                    Some(t) => {
                        targets.push(t);
                        pending.push(pm(
                            now + if magic == magic_type::THUNDER_BOLT {
                                600
                            } else {
                                500
                            },
                            Some(t),
                            location,
                            true,
                        ));
                    }
                    None => locations.push(location),
                }
            }
            magic_type::REPULSION => {
                for d in Direction::ALL {
                    pending.push(PendingMagic {
                        time: now + 500,
                        caster: id,
                        magic,
                        target: None,
                        location: loc.step(d, 1),
                        direction: Some(d),
                        primary: true,
                        chain: None,
                    });
                }
            }
            magic_type::HEAL => {
                let t = target.filter(|t| self.objects[t].is_player()).unwrap_or(id);
                targets.push(t);
                pending.push(pm(now + 500, Some(t), location, true));
            }
            // Warrior pulls: 300 ms, need a monster.
            magic_type::INTERCHANGE | magic_type::BECKON => {
                if let Some(t) = target.filter(|t| self.objects[t].is_monster()) {
                    targets.push(t);
                    pending.push(pm(now + 300, Some(t), location, true));
                }
            }
            magic_type::MASS_BECKON
            | magic_type::ENDURANCE
            | magic_type::REFLECT_DAMAGE
            | magic_type::FETTER => pending.push(pm(now + 500, None, loc, true)),
            magic_type::SWIFT_BLADE => {
                if location.distance(loc) > MAGIC_RANGE {
                    cast_ok = false;
                } else {
                    locations.push(location);
                    pending.push(pm(now + 900, None, location, true));
                }
            }
            // Self casts.
            magic_type::TELEPORTATION => pending.push(pm(now + 500, None, loc, true)),
            magic_type::MAGIC_SHIELD => pending.push(pm(now + 1100, None, loc, true)),
            magic_type::DEFIANCE | magic_type::MIGHT | magic_type::POISONOUS_CLOUD => {
                pending.push(pm(now + 500, None, loc, true))
            }
            magic_type::RENOUNCE => pending.push(pm(now + 600, None, loc, true)),
            // ---- Wave six ----
            mm if magic_wave6::handled(mm) => {
                if !self.wave6_cast(
                    id,
                    magic,
                    target,
                    location,
                    direction,
                    loc,
                    now,
                    &mut pending,
                    &mut targets,
                    &mut locations,
                ) {
                    cast_ok = false;
                }
            }
            // ---- Wave five ----
            mm if magic_wave5::handled(mm) => {
                if !self.wave5_cast(
                    id,
                    magic,
                    target,
                    location,
                    direction,
                    loc,
                    now,
                    &mut pending,
                    &mut targets,
                    &mut locations,
                ) {
                    cast_ok = false;
                }
            }
            // ---- Wave four ----
            mm if magic_wave4::handled(mm) => {
                if !self.wave4_cast(
                    id,
                    magic,
                    target,
                    location,
                    direction,
                    loc,
                    now,
                    &mut pending,
                    &mut targets,
                    &mut locations,
                ) {
                    cast_ok = false;
                }
            }
            // ---- Assassin wave 3 ----
            magic_type::CLOAK => {
                if self.objects[&id].has_buff(buff_type::CLOAK) {
                    // Casting again drops the cloak for free.
                    self.buff_remove(id, buff_type::CLOAK);
                } else {
                    let (hp, max_hp) = (self.objects[&id].hp, self.objects[&id].max_hp);
                    let hp_cost = max_hp * um.cost(&def) / 1000;
                    if hp_cost >= hp || hp < max_hp / 10 {
                        deny(self, "Not enough health to cloak.");
                        return;
                    }
                    let o = self.objects.get_mut(&id).unwrap();
                    o.hp -= hp_cost;
                    let (hp, max_hp) = (o.hp, o.max_hp);
                    self.events
                        .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
                    pending.push(pm(now + 500, None, loc, true));
                }
            }
            magic_type::RAKE => {
                if self.objects[&id].has_buff(buff_type::CLOAK) {
                    self.buff_remove(id, buff_type::CLOAK);
                    pending.push(pm(now + 600, None, loc.step(direction, 1), true));
                }
            }
            magic_type::WRAITH_GRIP | magic_type::HELL_FIRE => {
                match target.filter(|t| self.objects[t].is_monster()) {
                    Some(t) => {
                        let mlevel = match &self.objects[&t].kind {
                            Kind::Monster(m) => self.data.monsters[&m.def].level,
                            _ => 0,
                        };
                        if magic == magic_type::WRAITH_GRIP && mlevel > p.level + 15 {
                            cast_ok = false;
                            self.send_to(
                                id,
                                ServerMessage::Chat {
                                    text: "That is too strong to grip.".into(),
                                },
                            );
                        } else {
                            targets.push(t);
                            let delay = if magic == magic_type::HELL_FIRE {
                                1200
                            } else {
                                500
                            };
                            pending.push(pm(now + delay, Some(t), location, true));
                        }
                    }
                    None => {
                        cast_ok = false;
                        locations.push(location);
                    }
                }
            }
            magic_type::SUMMON_PUPPET => pending.push(pm(now + 500, None, loc, true)),
            // ---- Taoist wave 3 ----
            magic_type::INVISIBILITY | magic_type::TRANSPARENCY | magic_type::CELESTIAL_LIGHT => {
                let (need, delay) = match magic {
                    magic_type::INVISIBILITY => (2, 500),
                    magic_type::TRANSPARENCY => (10, 500),
                    _ => (20, 1500),
                };
                let already = match magic {
                    magic_type::INVISIBILITY => self.objects[&id].has_buff(buff_type::INVISIBILITY),
                    magic_type::TRANSPARENCY => self.objects[&id].has_buff(buff_type::TRANSPARENCY),
                    _ => self.objects[&id].has_buff(buff_type::CELESTIAL_LIGHT),
                };
                if already {
                    // Zircon: cast goes through (cooldown, MP) but nothing happens.
                } else if self.use_amulet(id, need).is_none() {
                    cast_ok = false;
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: format!(
                                "{} needs {need} Talisman charges. Equip a standard or elemental Talisman in the Amulet slot.",
                                def.name
                            ),
                        },
                    );
                } else {
                    if magic == magic_type::CELESTIAL_LIGHT {
                        targets.push(id);
                    }
                    pending.push(pm(now + delay, None, loc, true));
                }
            }
            magic_type::MASS_INVISIBILITY
            | magic_type::TRAP_OCTAGON
            | magic_type::ELEMENTAL_SUPERIORITY
            | magic_type::BLOOD_LUST => {
                let need = if magic == magic_type::ELEMENTAL_SUPERIORITY {
                    1
                } else {
                    2
                };
                if location.distance(loc) > MAGIC_RANGE {
                    cast_ok = false;
                } else if self.use_amulet(id, need).is_none() {
                    cast_ok = false;
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: format!(
                                "{} needs {need} Talisman charges. Equip a standard or elemental Talisman in the Amulet slot.",
                                def.name
                            ),
                        },
                    );
                } else {
                    locations.push(location);
                    let d = location.distance(loc) as u64;
                    pending.push(pm(now + 500 + d * 48, None, location, true));
                }
            }
            magic_type::COMBAT_KICK => {
                pending.push(pm(now + 500, None, loc.step(direction, 1), true))
            }
            magic_type::RESURRECTION => {
                // Dead players near the cell, one talisman each.
                let dead: Vec<ObjectId> = self
                    .on_map(map)
                    .filter(|o| {
                        o.is_player()
                            && o.dead
                            && o.location.distance(location) <= 3
                            && o.location.distance(loc) <= MAGIC_RANGE
                    })
                    .map(|o| o.id)
                    .collect();
                for t in dead {
                    if self.use_amulet(id, 1).is_none() {
                        self.send_to(
                            id,
                            ServerMessage::Chat {
                                text: format!("{} needs a talisman.", def.name),
                            },
                        );
                        break;
                    }
                    targets.push(t);
                    pending.push(pm(now + 500 + dist(self, t) * 48, Some(t), location, true));
                }
            }
            magic_type::PURIFICATION => {
                let t = target.unwrap_or(id);
                if self.use_amulet(id, 2).is_none() {
                    cast_ok = false;
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: format!("{} needs 2 talismans.", def.name),
                        },
                    );
                } else {
                    targets.push(t);
                    pending.push(pm(now + 500 + dist(self, t) * 48, Some(t), location, true));
                }
            }
            magic_type::ELECTRIC_SHOCK => {
                if let Some(t) = target.filter(|t| self.objects[t].is_monster()) {
                    targets.push(t);
                    pending.push(pm(now + 500, Some(t), location, true));
                } else {
                    locations.push(location);
                }
            }
            magic_type::SUMMON_SKELETON
            | magic_type::SUMMON_JIN_SKELETON
            | magic_type::SUMMON_SHINSU => {
                let need = match magic {
                    magic_type::SUMMON_SKELETON => 1,
                    magic_type::SUMMON_JIN_SKELETON => 2,
                    _ => 5,
                };
                if self.use_amulet(id, need).is_none() {
                    cast_ok = false;
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: format!(
                                "{} needs {need} Talisman charges. Equip a standard or elemental Talisman in the Amulet slot.",
                                def.name
                            ),
                        },
                    );
                } else {
                    let behind = loc.step(direction.rotate(4), 1);
                    pending.push(pm(now + 500, None, behind, true));
                }
            }
            magic_type::STRENGTH_OF_FAITH => {
                if self.use_amulet(id, 5).is_none() {
                    cast_ok = false;
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: format!("{} needs 5 talismans.", def.name),
                        },
                    );
                } else {
                    targets.push(id);
                    pending.push(pm(now + 500, None, loc, true));
                }
            }
            magic_type::EXPEL_UNDEAD => {
                let undead = target.filter(|t| match &self.objects[t].kind {
                    Kind::Monster(m) => self.data.monsters[&m.def].undead,
                    _ => false,
                });
                if let Some(t) = undead {
                    targets.push(t);
                    pending.push(pm(now + 500, Some(t), location, true));
                }
            }
            magic_type::GEO_MANIPULATION => {
                if location.distance(loc) > MAGIC_RANGE {
                    cast_ok = false;
                } else {
                    pending.push(pm(now + 500, None, location, true));
                }
            }
            // 3x3 storms on a cell.
            magic_type::FIRE_STORM
            | magic_type::LIGHTNING_WAVE
            | magic_type::ICE_STORM
            | magic_type::DRAGON_TORNADO
            | magic_type::TEMPEST => {
                if location.distance(loc) > MAGIC_RANGE {
                    cast_ok = false;
                } else {
                    locations.push(location);
                    let delay = if magic == magic_type::DRAGON_TORNADO {
                        1200
                    } else {
                        500
                    };
                    pending.push(pm(now + delay, None, location, true));
                }
            }
            magic_type::METEOR_SHOWER => {
                let level = um.level as usize;
                let mut pool: Vec<ObjectId> = self
                    .on_map(map)
                    .filter(|o| {
                        self.objects[&id].hostile_to(o)
                            && !o.dead
                            && o.location.distance(location) <= 3
                            && o.location.distance(loc) <= MAGIC_RANGE
                    })
                    .map(|o| o.id)
                    .collect();
                while !pool.is_empty() && targets.len() < 6 + level {
                    let i = self.rng.random_range(0..pool.len());
                    let t = pool.swap_remove(i);
                    targets.push(t);
                    pending.push(pm(now + 500 + dist(self, t) * 48, Some(t), location, true));
                }
            }
            magic_type::CHAIN_LIGHTNING => {
                if let Some(t) = target.filter(|t| self.objects[t].is_monster()) {
                    let cell = self.objects[&t].location;
                    locations.push(cell);
                    let mut p = pm(now + 600, None, cell, true);
                    p.chain = Some((0, Vec::new()));
                    pending.push(p);
                }
            }
            // Lines: 8 cells ahead with flanks; delay 800 (Lightning Beam 500).
            m if magic_type::is_line(m) => {
                let delay = if m == magic_type::LIGHTNING_BEAM {
                    500
                } else {
                    800
                };
                // Greater Frozen Earth fans three rays (direction -1, 0, +1).
                let rays: Vec<Direction> = if m == magic_type::GREATER_FROZEN_EARTH {
                    vec![direction.rotate(-1), direction, direction.rotate(1)]
                } else {
                    vec![direction]
                };
                for ray in rays {
                    for (i, (cell, primary)) in Self::line_cells(loc, ray).into_iter().enumerate() {
                        if primary && (m != magic_type::LIGHTNING_BEAM || i == 0) {
                            locations.push(cell);
                        }
                        pending.push(pm(now + delay, None, cell, primary));
                    }
                }
            }
            // Ground casts.
            magic_type::FIRE_WALL => {
                if location.distance(loc) > MAGIC_RANGE {
                    cast_ok = false;
                } else {
                    for cell in [
                        location,
                        location.step(Direction::Up, 1),
                        location.step(Direction::Down, 1),
                        location.step(Direction::Left, 1),
                        location.step(Direction::Right, 1),
                    ] {
                        pending.push(pm(now + 500, None, cell, true));
                    }
                }
            }
            magic_type::MASS_HEAL => {
                if location.distance(loc) > MAGIC_RANGE {
                    cast_ok = false;
                } else {
                    locations.push(location);
                    let d = location.distance(loc) as u64;
                    pending.push(pm(now + 500 + d * 48, None, location, true));
                }
            }
            magic_type::MAGIC_RESISTANCE | magic_type::RESILIENCE => {
                let need = if magic == magic_type::RESILIENCE {
                    2
                } else {
                    1
                };
                if location.distance(loc) > MAGIC_RANGE {
                    cast_ok = false;
                } else if let Some((_, _)) = self.use_amulet(id, need) {
                    locations.push(location);
                    let d = location.distance(loc) as u64;
                    pending.push(pm(now + 500 + d * 48, None, location, true));
                } else {
                    cast_ok = false;
                    self.send_to(
                        id,
                        ServerMessage::Chat {
                            text: format!("{} needs {need} talisman(s).", def.name),
                        },
                    );
                }
            }
            magic_type::SHOULDER_DASH => {
                // Zircon: no ObjectMagic; the dash streams as ObjectDash steps.
                let already = self.objects[&id].player().unwrap().dash.is_some();
                if already {
                    return;
                }
                let (pmin, pmax) = um.power_range(&def);
                let distance = self.roll_range(pmin, pmax).max(1);
                {
                    let o = self.objects.get_mut(&id).unwrap();
                    o.direction = direction;
                    let p = o.player_mut().unwrap();
                    p.mp -= cost;
                    p.dash = Some(Dash {
                        remaining: distance,
                        travelled: 0,
                        direction,
                        next_step: now,
                    });
                    if let Some(m) = p.magics.iter_mut().find(|m| m.magic == magic) {
                        m.cooldown_until = now + def.delay.max(0) as u64;
                    }
                }
                if def.delay > 0 {
                    self.send_to(
                        id,
                        ServerMessage::MagicCooldown {
                            magic,
                            delay_ms: def.delay as u32,
                        },
                    );
                }
                self.send_player_stats(id);
                return;
            }
            _ => {}
        }
        // Zircon `MagicFinalise`: casting drops the cloak and transparency,
        // except for the skills that keep them.
        if !matches!(
            magic,
            magic_type::CLOAK
                | magic_type::POISONOUS_CLOUD
                | magic_type::TRANSPARENCY
                | magic_type::THE_NEW_BEGINNING
                | magic_type::EVASION
                | magic_type::RAGING_WIND
                | magic_type::DARK_CONVERSION
        ) {
            let stealth = self.objects[&id]
                .player()
                .and_then(|p| p.magics.iter().find(|m| m.magic == magic_type::STEALTH))
                .map(|m| m.power_range(&self.data.magics[&m.magic]).0);
            let stacks = self.buff_stacks(id, buff_type::THE_NEW_BEGINNING);
            let keep = match stealth {
                Some(power) if stacks > 0 && self.objects[&id].has_buff(buff_type::CLOAK) => {
                    if self.rng.random_range(0..100) > power {
                        self.buff_consume_stack(id, buff_type::THE_NEW_BEGINNING);
                    } else {
                        self.level_magic(id, magic_type::STEALTH);
                    }
                    self.level_magic(id, magic_type::THE_NEW_BEGINNING);
                    true
                }
                _ => false,
            };
            if !keep {
                self.buff_remove(id, buff_type::CLOAK);
            }
            self.buff_remove(id, buff_type::TRANSPARENCY);
        }
        // Pay, set timers (Zircon: consume even when the spell fizzles).
        let face = match target {
            _ if magic_type::is_stance_cast(magic) => Direction::Down,
            Some(t) if t != id => Direction::from_points(loc, self.objects[&t].location),
            _ => direction,
        };
        {
            let o = self.objects.get_mut(&id).unwrap();
            if !mobile_support {
                o.action_time = self.now + CAST_TIME;
                o.direction = face;
            }
            let p = o.player_mut().unwrap();
            p.mp -= cost;
            p.magic_time = self.now + MAGIC_DELAY;
            if cast_ok {
                if let Some(m) = p.magics.iter_mut().find(|m| m.magic == magic) {
                    m.cooldown_until = self.now + def.delay.max(0) as u64;
                }
            }
        }
        let dir = self.objects[&id].direction;
        if def.delay > 0 && cast_ok {
            self.send_to(
                id,
                ServerMessage::MagicCooldown {
                    magic,
                    delay_ms: def.delay as u32,
                },
            );
        }
        self.send_player_stats(id);
        self.events.push((
            id,
            ServerMessage::ObjectMagic {
                id,
                direction: dir,
                location: loc,
                magic,
                targets,
                locations,
                cast: cast_ok,
            },
        ));
        self.pending_magics.extend(pending);
    }

    /// Spell damage (Zircon `MagicAttack`): magic power plus the class stat,
    /// minus the target's MR and element resistance.
    pub(super) fn magic_attack(
        &mut self,
        caster: ObjectId,
        target: ObjectId,
        magic: u16,
        elem: u8,
        scale: i32,
    ) -> i32 {
        let Some(def) = self.data.magics.get(&magic).cloned() else {
            return 0;
        };
        let Some(c) = self.objects.get(&caster) else {
            return 0;
        };
        let Some(p) = c.player() else {
            return 0;
        };
        let Some(um) = p.magics.iter().find(|m| m.magic == magic).cloned() else {
            return 0;
        };
        let (pmin, pmax) = um.power_range(&def);
        let cs = c.stats;
        let class = p.class;
        let Some(t) = self.objects.get(&target) else {
            return 0;
        };
        if !c.hostile_to(t) {
            return 0;
        }
        let ts = t.stats;
        let element_index = usize::from(elem).min(7);
        let resist = ts.element_resist[element_index];
        let target_is_player = t.player().is_some();
        let mut power = if pmin >= pmax {
            pmin
        } else {
            self.rng.random_range(pmin..=pmax)
        };
        power = match magic {
            // Rake and puppets: a percent of DC; Hell Fire: power plus DC.
            magic_type::RAKE | magic_type::SUMMON_PUPPET => self.roll_dc(cs) * power / 100,
            magic_type::HELL_FIRE => power + self.roll_dc(cs),
            _ => {
                power
                    + match class {
                        Class::Wizard => self.roll_lucky(cs.min_mc, cs.max_mc, cs.luck),
                        Class::Taoist => self.roll_lucky(cs.min_sc, cs.max_sc, cs.luck),
                        Class::Assassin => self.roll_lucky(
                            cs.min_mc.min(cs.min_sc),
                            cs.max_mc.min(cs.max_sc),
                            cs.luck,
                        ),
                        Class::Warrior => 0,
                    }
            }
        };
        // Zircon `ModifyPowerMultiplier` (flank cells 30 %, fire wall 60 %...).
        power = power * scale / 100;
        power -= self.roll_range(ts.min_mr, ts.max_mr);
        if elem != element::NONE {
            let attack = cs.element_attack[element_index].max(0);
            power += if target_is_player {
                self.rng.random_range(0..=attack) * 2
            } else {
                attack * 2
            };
        }
        if resist != 0 {
            power -= power * resist / 10;
        }
        if power <= 0 {
            return 0;
        }
        if self.rng.random_range(0..100) < 1 {
            power = power * 12 / 10;
        }
        let dealt = self.damage(target, caster, power, elem, true);
        if dealt > 0 {
            self.level_magic(caster, magic);
        }
        dealt
    }

    pub(super) fn resolve_magics(&mut self) {
        let due: Vec<PendingMagic> = {
            let (due, later): (Vec<_>, Vec<_>) = self
                .pending_magics
                .drain(..)
                .partition(|m| m.time <= self.now);
            self.pending_magics = later;
            due
        };
        for pm in due {
            let Some(c) = self.objects.get(&pm.caster) else {
                continue;
            };
            if c.dead {
                continue;
            }
            let (cmap, cloc, clevel) =
                (c.map, c.location, c.player().map(|p| p.level).unwrap_or(1));
            match pm.magic {
                magic_type::FIRE_BALL | magic_type::FLAMING_DAGGERS | magic_type::SHREDDING => {
                    if let Some(t) = pm.target {
                        self.magic_attack(pm.caster, t, pm.magic, element::FIRE, 100);
                    }
                }
                magic_type::ICE_BOLT => {
                    if let Some(t) = pm.target {
                        self.magic_attack(pm.caster, t, pm.magic, element::ICE, 100);
                    }
                }
                magic_type::THUNDER_BOLT => {
                    if let Some(t) = pm.target {
                        self.magic_attack(pm.caster, t, pm.magic, element::LIGHTNING, 100);
                    }
                }
                magic_type::REPULSION => {
                    let Some(dir) = pm.direction else { continue };
                    let Some(m) = self.maps.get(&cmap) else {
                        continue;
                    };
                    let victims: Vec<ObjectId> = m.objects_at(pm.location).to_vec();
                    let (lvl, power) = {
                        let p = self.objects[&pm.caster].player().unwrap();
                        let um = p.magics.iter().find(|m| m.magic == pm.magic).unwrap();
                        let def = &self.data.magics[&pm.magic];
                        (um.level as i32, um.power_range(def))
                    };
                    for v in victims {
                        let (start_loc, mlevel, is_boss) = match self.objects.get(&v) {
                            Some(o) if o.is_monster() && !o.dead => match &o.kind {
                                Kind::Monster(m) => (
                                    o.location,
                                    self.data.monsters[&m.def].level,
                                    self.data.monsters[&m.def].is_boss,
                                ),
                                _ => continue,
                            },
                            _ => continue,
                        };
                        if is_boss || mlevel >= clevel {
                            continue;
                        }
                        if self.rng.random_range(0..16) >= 6 + lvl * 3 + clevel - mlevel {
                            continue;
                        }
                        let distance = self.roll_range(power.0, power.1);
                        let mut from = start_loc;
                        let mut moved = 0;
                        for _ in 0..distance {
                            let next = from.step(dir, 1);
                            if self.cell_blocked(cmap, next, false) {
                                break;
                            }
                            from = next;
                            moved += 1;
                        }
                        if moved > 0 {
                            let start = self.objects[&v].location;
                            self.move_object(v, from);
                            self.events.push((
                                v,
                                ServerMessage::ObjectMove {
                                    id: v,
                                    from: start,
                                    to: from,
                                    direction: dir,
                                    run: moved > 1,
                                },
                            ));
                            self.level_magic(pm.caster, pm.magic);
                        }
                    }
                }
                magic_type::HEAL => {
                    let Some(t) = pm.target else { continue };
                    let Some(to) = self.objects.get(&t) else {
                        continue;
                    };
                    if to.dead || to.hp >= to.max_hp || to.heal.is_some() {
                        continue;
                    }
                    let (pmin, pmax, sc) = {
                        let c = &self.objects[&pm.caster];
                        let p = c.player().unwrap();
                        let um = p.magics.iter().find(|m| m.magic == pm.magic).unwrap();
                        let def = &self.data.magics[&pm.magic];
                        let (pmin, pmax) = um.power_range(def);
                        (pmin, pmax, (c.stats.min_sc, c.stats.max_sc))
                    };
                    let empowered = self.objects[&pm.caster]
                        .player()
                        .and_then(|p| {
                            p.magics
                                .iter()
                                .find(|m| m.magic == magic_type::EMPOWERED_HEALING)
                        })
                        .map(|m| (m.level as i32, m.power_range(&self.data.magics[&m.magic]).0));
                    let (bonus, cap) = empowered
                        .map(|(l, p)| (p, 30 + (1 + l) * 30))
                        .unwrap_or((0, 30));
                    let healing = self.roll_range(pmin, pmax) + self.roll_range(sc.0, sc.1) + bonus;
                    if healing <= 0 {
                        continue;
                    }
                    self.objects.get_mut(&t).unwrap().heal = Some(HealBuff {
                        pool: healing,
                        cap,
                        next_tick: self.now,
                    });
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::POISON_DUST => {
                    let Some(t) = pm.target else { continue };
                    let Some(to) = self.objects.get(&t) else {
                        continue;
                    };
                    if !self.player_can_attack_target(pm.caster, to) {
                        continue;
                    }
                    let (lvl, duration, kind, poison_slot) = {
                        let c = &self.objects[&pm.caster];
                        let p = c.player().unwrap();
                        let um = p.magics.iter().find(|m| m.magic == pm.magic).unwrap();
                        let def = &self.data.magics[&pm.magic];
                        let (pmin, pmax) = um.power_range(def);
                        // Zircon consumes one equipped poison; its shape picks Green/Red.
                        let poison_item =
                            p.bag.equipment.get(slot::POISON).and_then(|s| s.as_ref());
                        let kind = match poison_item.and_then(|i| self.data.items.get(&i.info)) {
                            Some(d) if d.shape != 0 => 2,
                            _ => 1,
                        };
                        (
                            um.level as i32,
                            (pmin, pmax, c.stats.min_sc, c.stats.max_sc),
                            kind,
                            poison_item.map(|_| slot::POISON as u8),
                        )
                    };
                    if let Some(ps) = poison_slot {
                        let o = self.objects.get_mut(&pm.caster).unwrap();
                        let change = o.player_mut().unwrap().bag.take(Grid::Equipment, ps, 1);
                        self.send_changes(pm.caster, change.into_iter().collect());
                    }
                    let dur = self.roll_range(duration.0, duration.1)
                        + self.roll_range(duration.2, duration.3);
                    let value = lvl + 1 + clevel / 14;
                    let poison = Poison {
                        kind,
                        value,
                        ticks_left: (dur / 2).max(1),
                        next_tick: self.now + 2000,
                        owner: Some(pm.caster),
                    };
                    self.apply_poison(t, poison.clone());
                    // Augment Poison Dust: GetPower() + 1 more monsters within 3.
                    if self.knows(pm.caster, magic_type::AUGMENT_POISON_DUST) {
                        let (amin, amax, _) =
                            self.caster_power(pm.caster, magic_type::AUGMENT_POISON_DUST);
                        let extra = (self.roll_range(amin, amax) + 1) as usize;
                        let at = self.objects[&t].location;
                        let map = self.objects[&t].map;
                        let more: Vec<ObjectId> = self
                            .on_map(map)
                            .filter(|o| {
                                o.id != t
                                    && self.player_can_attack_target(pm.caster, o)
                                    && o.location.distance(at) <= 3
                                    && !o.poisons.iter().any(|p| p.kind == kind)
                            })
                            .map(|o| o.id)
                            .take(extra)
                            .collect();
                        for v in more {
                            self.apply_poison(v, poison.clone());
                        }
                    }
                    self.level_magic(pm.caster, pm.magic);
                }
                // Single-target elemental bolts with side effects.
                magic_type::LIGHTNING_BALL => {
                    if let Some(t) = pm.target {
                        self.magic_attack(pm.caster, t, pm.magic, element::LIGHTNING, 100);
                    }
                }
                magic_type::ADAMANTINE_FIRE_BALL => {
                    if let Some(t) = pm.target {
                        self.magic_attack(pm.caster, t, pm.magic, element::FIRE, 100);
                    }
                }
                magic_type::ICE_BLADES => {
                    if let Some(t) = pm.target {
                        let dealt = self.magic_attack(pm.caster, t, pm.magic, element::ICE, 100);
                        if dealt > 0 {
                            self.try_slow(t, pm.caster, 5, 5);
                        }
                    }
                }
                magic_type::GUST_BLAST | magic_type::CYCLONE => {
                    if let Some(t) = pm.target {
                        let dealt = self.magic_attack(pm.caster, t, pm.magic, element::WIND, 100);
                        let repel = if pm.magic == magic_type::CYCLONE {
                            5
                        } else {
                            10
                        };
                        if dealt > 0 {
                            self.try_repel(t, pm.caster, repel);
                        }
                    }
                }
                magic_type::EVIL_SLAYER | magic_type::GREATER_EVIL_SLAYER => {
                    if let Some(t) = pm.target {
                        // Holy talisman bonus: 30 % (60 % for the greater one).
                        let scale = if self.amulet_holy(pm.caster) {
                            if pm.magic == magic_type::GREATER_EVIL_SLAYER {
                                160
                            } else {
                                130
                            }
                        } else {
                            100
                        };
                        self.magic_attack(pm.caster, t, pm.magic, element::HOLY, scale);
                    }
                }
                magic_type::EXPLOSIVE_TALISMAN => {
                    if let Some(t) = pm.target {
                        self.magic_attack(pm.caster, t, pm.magic, element::DARK, 100);
                    }
                }
                // Lines: everything on the cell; flanks for 30 %.
                m if magic_type::is_line(m) => {
                    let Some(map) = self.maps.get(&cmap) else {
                        continue;
                    };
                    let victims: Vec<ObjectId> = map
                        .objects_at(pm.location)
                        .iter()
                        .copied()
                        .filter(|v| self.objects[v].is_monster() && !self.objects[v].dead)
                        .collect();
                    let elem = match m {
                        magic_type::SCORCHED_EARTH => element::FIRE,
                        magic_type::LIGHTNING_BEAM => element::LIGHTNING,
                        magic_type::FROZEN_EARTH | magic_type::GREATER_FROZEN_EARTH => element::ICE,
                        _ => element::WIND,
                    };
                    let scale = if pm.primary { 100 } else { 30 };
                    for v in victims {
                        let dealt = self.magic_attack(pm.caster, v, m, elem, scale);
                        if dealt > 0 {
                            match m {
                                magic_type::FROZEN_EARTH => self.try_slow(v, pm.caster, 10, 3),
                                magic_type::GREATER_FROZEN_EARTH => {
                                    self.try_slow(v, pm.caster, 5, 5)
                                }
                                magic_type::BLOW_EARTH => self.try_repel(v, pm.caster, 10),
                                _ => {}
                            }
                        }
                    }
                }
                magic_type::FIRE_WALL => {
                    let walkable = self
                        .maps
                        .get(&cmap)
                        .map(|m| m.file.is_walkable(pm.location.x, pm.location.y))
                        .unwrap_or(false);
                    if !walkable {
                        continue;
                    }
                    let level = self.objects[&pm.caster]
                        .player()
                        .and_then(|p| p.magics.iter().find(|m| m.magic == pm.magic))
                        .map(|m| m.level as i32)
                        .unwrap_or(0);
                    self.spawn_spell(
                        cmap,
                        pm.location,
                        spell_effect::FIRE_WALL,
                        (level + 2) * 5,
                        2000,
                        pm.caster,
                        pm.magic,
                    );
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::POISONOUS_CLOUD => {
                    let exists = self.maps[&cmap]
                        .objects_at(cloc)
                        .iter()
                        .any(|v| matches!(&self.objects[v].kind, Kind::Spell(s) if s.effect == spell_effect::POISONOUS_CLOUD));
                    if exists {
                        continue;
                    }
                    let (pmin, pmax) = {
                        let p = self.objects[&pm.caster].player().unwrap();
                        let um = p.magics.iter().find(|m| m.magic == pm.magic).unwrap();
                        um.power_range(&self.data.magics[&pm.magic])
                    };
                    let seconds = self.roll_range(pmin, pmax).max(1) as u64;
                    self.spawn_spell(
                        cmap,
                        cloc,
                        spell_effect::POISONOUS_CLOUD,
                        1,
                        seconds * 1000,
                        pm.caster,
                        pm.magic,
                    );
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::TELEPORTATION => {
                    let level = self.objects[&pm.caster]
                        .player()
                        .and_then(|p| p.magics.iter().find(|m| m.magic == pm.magic))
                        .map(|m| m.level as i32)
                        .unwrap_or(0);
                    // Random.Next(MagicMaxLevel + 5) > 2 + Level * 2 => fail.
                    if self.rng.random_range(0..9) > 2 + level * 2 {
                        self.level_magic(pm.caster, pm.magic);
                        continue;
                    }
                    let Some(to) = self.random_walkable(cmap) else {
                        continue;
                    };
                    self.teleport_with_effects(pm.caster, to);
                    self.level_magic(pm.caster, pm.magic);
                }
                // ---- Warrior wave 3 ----
                magic_type::INTERCHANGE | magic_type::BECKON => {
                    let Some(t) = pm.target else { continue };
                    let Some((tloc, mlevel, boss, dead, tmap)) =
                        self.objects.get(&t).and_then(|o| match &o.kind {
                            Kind::Monster(m) => Some((
                                o.location,
                                self.data.monsters[&m.def].level,
                                self.data.monsters[&m.def].is_boss,
                                o.dead,
                                o.map,
                            )),
                            _ => None,
                        })
                    else {
                        continue;
                    };
                    if dead || tmap != cmap {
                        continue;
                    }
                    let level = self.magic_level(pm.caster, pm.magic);
                    let ok = if pm.magic == magic_type::INTERCHANGE {
                        mlevel < clevel && !boss
                    } else {
                        !boss
                    };
                    // Random.Next(9) > 2 + Level * 2 => fail.
                    if !ok || self.rng.random_range(0..9) > 2 + level * 2 {
                        continue;
                    }
                    if pm.magic == magic_type::INTERCHANGE {
                        self.teleport_with_effects(pm.caster, tloc);
                        self.teleport_object(t, cloc);
                    } else {
                        let dir = self.objects[&pm.caster].direction;
                        let front = cloc.step(dir, 1);
                        let walkable = self.maps[&cmap].file.is_walkable(front.x, front.y);
                        if !walkable || self.cell_blocked(cmap, front, false) {
                            continue;
                        }
                        self.teleport_object(t, front);
                        self.apply_poison(
                            t,
                            Poison {
                                kind: poison_kind::PARALYSIS,
                                value: 0,
                                ticks_left: 0,
                                next_tick: self.now + (1 + level as u64) * 1000,
                                owner: Some(pm.caster),
                            },
                        );
                    }
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::MASS_BECKON => {
                    let level = self.magic_level(pm.caster, pm.magic);
                    let victims: Vec<(ObjectId, i32, bool)> = self
                        .on_map(cmap)
                        .filter(|o| !o.dead && o.location.distance(cloc) <= 9)
                        .filter_map(|o| match &o.kind {
                            Kind::Monster(m) if m.owner.is_none() => Some((
                                o.id,
                                self.data.monsters[&m.def].level,
                                self.data.monsters[&m.def].is_boss,
                            )),
                            _ => None,
                        })
                        .collect();
                    for (v, mlevel, boss) in victims {
                        if mlevel - 10 > clevel || boss {
                            continue;
                        }
                        if self.rng.random_range(0..9) > 2 + level * 2 {
                            continue;
                        }
                        let mut dest = None;
                        for _ in 0..25 {
                            let p = Point::new(
                                cloc.x + self.rng.random_range(-3..=3),
                                cloc.y + self.rng.random_range(-3..=3),
                            );
                            if self.maps[&cmap].file.is_walkable(p.x, p.y)
                                && !self.cell_blocked(cmap, p, false)
                            {
                                dest = Some(p);
                                break;
                            }
                        }
                        let Some(p) = dest else { continue };
                        self.teleport_object(v, p);
                        self.apply_poison(
                            v,
                            Poison {
                                kind: poison_kind::PARALYSIS,
                                value: 0,
                                ticks_left: 0,
                                next_tick: self.now + (1 + level as u64) * 1000,
                                owner: Some(pm.caster),
                            },
                        );
                        self.level_magic(pm.caster, pm.magic);
                    }
                }
                magic_type::SWIFT_BLADE => {
                    // 7x7 around the cell: a DC-percent melee hit on everything.
                    let victims: Vec<Point> = self
                        .on_map(cmap)
                        .filter(|o| {
                            o.is_monster() && !o.dead && o.location.distance(pm.location) <= 3
                        })
                        .map(|o| o.location)
                        .collect();
                    let power = {
                        let s = self.objects[&pm.caster].stats;
                        self.roll_dc(s)
                    };
                    for cell in victims {
                        self.pending_hits.push(PendingHit {
                            time: self.now,
                            attacker: pm.caster,
                            target_cell: (cmap, cell),
                            power,
                            target: None,
                            magics: vec![magic_type::SWIFT_BLADE],
                            primary: true,
                            raw: false,
                            element: element::NONE,
                            ranged: false,
                        });
                    }
                }
                magic_type::ENDURANCE => {
                    let level = self.magic_level(pm.caster, pm.magic) as u64;
                    self.buff_add(
                        pm.caster,
                        buff_type::ENDURANCE,
                        (10 + level * 5) * 1000,
                        BuffStats::default(),
                    );
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::REFLECT_DAMAGE => {
                    let level = self.magic_level(pm.caster, pm.magic);
                    let aug = self.objects[&pm.caster]
                        .player()
                        .and_then(|p| {
                            p.magics
                                .iter()
                                .find(|m| m.magic == magic_type::AUGMENT_REFLECT_DAMAGE)
                        })
                        .map(|m| (m.level as i32, m.power_range(&self.data.magics[&m.magic]).0));
                    let (extra_secs, extra_pct) =
                        aug.map(|(l, p)| (5 + l as u64 * 5, p)).unwrap_or((0, 0));
                    self.buff_add(
                        pm.caster,
                        buff_type::REFLECT_DAMAGE,
                        (15 + level as u64 * 10 + extra_secs) * 1000,
                        BuffStats {
                            reflect: 5 + level * 3 + extra_pct,
                            ..BuffStats::default()
                        },
                    );
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::FETTER => {
                    let level = self.magic_level(pm.caster, pm.magic);
                    let victims: Vec<(ObjectId, i32)> = self
                        .on_map(cmap)
                        .filter(|o| !o.dead && o.location.distance(cloc) <= 2)
                        .filter_map(|o| match &o.kind {
                            Kind::Monster(m) if m.owner.is_none() => {
                                Some((o.id, self.data.monsters[&m.def].level))
                            }
                            _ => None,
                        })
                        .collect();
                    for (v, mlevel) in victims {
                        if mlevel > clevel + 15 {
                            continue;
                        }
                        self.apply_poison(
                            v,
                            Poison {
                                kind: poison_kind::SLOW,
                                value: (3 + level) * 2,
                                ticks_left: 0,
                                next_tick: self.now + (5 + level as u64 * 3) * 1000,
                                owner: Some(pm.caster),
                            },
                        );
                        self.level_magic(pm.caster, pm.magic);
                    }
                }
                magic_type::MAGIC_SHIELD => {
                    let c = &self.objects[&pm.caster];
                    if c.has_buff(buff_type::MAGIC_SHIELD) {
                        continue;
                    }
                    let (level, mc) = {
                        let p = c.player().unwrap();
                        let um = p.magics.iter().find(|m| m.magic == pm.magic).unwrap();
                        (um.level as i64, (c.stats.min_mc, c.stats.max_mc))
                    };
                    let mc = self.roll_range(mc.0, mc.1) as i64;
                    let secs = 30 + level * 20 + mc / 2;
                    self.buff_add(
                        pm.caster,
                        buff_type::MAGIC_SHIELD,
                        secs as u64 * 1000,
                        BuffStats {
                            magic_shield: 50,
                            ..BuffStats::default()
                        },
                    );
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::DEFIANCE | magic_type::MIGHT => {
                    let level = self.objects[&pm.caster]
                        .player()
                        .and_then(|p| p.magics.iter().find(|m| m.magic == pm.magic))
                        .map(|m| m.level as i32)
                        .unwrap_or(0);
                    let mut secs = (60 + level * 30) as u64;
                    let amount = 5 + level * 5;
                    if pm.magic == magic_type::DEFIANCE {
                        // Augment Defiance: less DC loss and a longer hold.
                        let mut offence = 20;
                        if self.objects[&pm.caster]
                            .player()
                            .map(|p| {
                                p.magics
                                    .iter()
                                    .any(|m| m.magic == magic_type::AUGMENT_DEFIANCE)
                            })
                            .unwrap_or(false)
                        {
                            offence = (20 - level * 5).max(0);
                            secs += (10 + level * 10) as u64;
                        }
                        self.buff_remove(pm.caster, buff_type::MIGHT);
                        self.buff_add(
                            pm.caster,
                            buff_type::DEFIANCE,
                            secs * 1000,
                            BuffStats {
                                phys_def_pct: amount,
                                mag_def_pct: amount,
                                dc_pct: -offence,
                                ..BuffStats::default()
                            },
                        );
                    } else {
                        self.buff_remove(pm.caster, buff_type::DEFIANCE);
                        self.buff_add(
                            pm.caster,
                            buff_type::MIGHT,
                            secs * 1000,
                            BuffStats {
                                dc_pct: amount,
                                phys_def_pct: -amount,
                                mag_def_pct: -amount,
                                ..BuffStats::default()
                            },
                        );
                    }
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::MASS_HEAL => {
                    let (pmin, pmax, sc) = {
                        let c = &self.objects[&pm.caster];
                        let p = c.player().unwrap();
                        let um = p.magics.iter().find(|m| m.magic == pm.magic).unwrap();
                        let (pmin, pmax) = um.power_range(&self.data.magics[&pm.magic]);
                        (pmin, pmax, (c.stats.min_sc, c.stats.max_sc))
                    };
                    let players: Vec<ObjectId> = self
                        .on_map(cmap)
                        .filter(|o| {
                            o.is_player()
                                && o.location.distance(pm.location) <= 2
                                && !o.dead
                                && o.hp < o.max_hp
                                && o.heal.is_none()
                        })
                        .map(|o| o.id)
                        .collect();
                    for t in players {
                        let healing = self.roll_range(pmin, pmax) + self.roll_range(sc.0, sc.1);
                        if healing <= 0 {
                            continue;
                        }
                        self.objects.get_mut(&t).unwrap().heal = Some(HealBuff {
                            pool: healing,
                            cap: 30,
                            next_tick: self.now,
                        });
                        self.level_magic(pm.caster, pm.magic);
                    }
                }
                magic_type::MAGIC_RESISTANCE | magic_type::RESILIENCE => {
                    let (level, pmin, pmax, sc) = {
                        let c = &self.objects[&pm.caster];
                        let p = c.player().unwrap();
                        let um = p.magics.iter().find(|m| m.magic == pm.magic).unwrap();
                        let (pmin, pmax) = um.power_range(&self.data.magics[&pm.magic]);
                        (
                            um.level as i32,
                            pmin,
                            pmax,
                            (c.stats.min_sc, c.stats.max_sc),
                        )
                    };
                    let players: Vec<ObjectId> = self
                        .on_map(cmap)
                        .filter(|o| {
                            o.is_player() && !o.dead && o.location.distance(pm.location) <= 3
                        })
                        .map(|o| o.id)
                        .collect();
                    for t in players {
                        let secs = (self.roll_range(pmin, pmax) + self.roll_range(sc.0, sc.1) * 2)
                            .max(1) as u64;
                        let (kind, stats) = if pm.magic == magic_type::MAGIC_RESISTANCE {
                            (
                                buff_type::MAGIC_RESISTANCE,
                                BuffStats {
                                    max_mr: 5 + level,
                                    ..BuffStats::default()
                                },
                            )
                        } else {
                            (
                                buff_type::RESILIENCE,
                                BuffStats {
                                    max_ac: 5 + level,
                                    ..BuffStats::default()
                                },
                            )
                        };
                        self.buff_add(t, kind, secs * 1000, stats);
                        self.level_magic(pm.caster, pm.magic);
                    }
                }
                // ---- Wave six ----
                mm if magic_wave6::handled(mm) => self.wave6_land(&pm, cmap, cloc),
                // ---- Wave five ----
                mm if magic_wave5::handled(mm) => self.wave5_land(&pm, cmap, cloc),
                // ---- Wave four ----
                mm if magic_wave4::handled(mm) => self.wave4_land(&pm, cmap, cloc),
                // ---- Assassin wave 3 ----
                magic_type::CLOAK => {
                    self.apply_cloak(pm.caster, false);
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::RAKE => {
                    let victims: Vec<ObjectId> = self.maps[&cmap]
                        .objects_at(pm.location)
                        .iter()
                        .copied()
                        .filter(|v| self.objects[v].is_monster() && !self.objects[v].dead)
                        .collect();
                    for v in victims {
                        if self.magic_attack(pm.caster, v, pm.magic, element::NONE, 100) > 0 {
                            // Guaranteed slow, level 20, 6-10 s.
                            let secs = (3 + self.rng.random_range(0..3)) * 2;
                            self.apply_poison(
                                v,
                                Poison {
                                    kind: poison_kind::SLOW,
                                    value: 20,
                                    ticks_left: 0,
                                    next_tick: self.now + secs as u64 * 1000,
                                    owner: Some(pm.caster),
                                },
                            );
                            break;
                        }
                    }
                }
                magic_type::WRAITH_GRIP => {
                    let Some(t) = pm.target else { continue };
                    if self.objects.get(&t).map(|o| o.dead).unwrap_or(true) {
                        continue;
                    }
                    let (pmin, pmax, _) = self.caster_power(pm.caster, pm.magic);
                    let duration = self.roll_range(pmin, pmax).max(2);
                    let cs = self.objects[&pm.caster].stats;
                    let sp = self.roll_lucky(
                        cs.min_mc.min(cs.min_sc),
                        cs.max_mc.min(cs.max_sc),
                        cs.luck,
                    );
                    self.apply_poison(
                        t,
                        Poison {
                            kind: poison_kind::WRAITH_GRIP,
                            value: sp,
                            ticks_left: duration / 2,
                            next_tick: self.now + 2000,
                            owner: Some(pm.caster),
                        },
                    );
                    let touch = self.objects[&pm.caster]
                        .player()
                        .map(|p| {
                            p.magics
                                .iter()
                                .any(|m| m.magic == magic_type::TOUCH_OF_THE_DEPARTED)
                        })
                        .unwrap_or(false);
                    if touch {
                        self.apply_poison(
                            t,
                            Poison {
                                kind: poison_kind::PARALYSIS,
                                value: 0,
                                ticks_left: 0,
                                next_tick: self.now + duration as u64 * 1000,
                                owner: Some(pm.caster),
                            },
                        );
                        self.level_magic(pm.caster, magic_type::TOUCH_OF_THE_DEPARTED);
                    }
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::HELL_FIRE => {
                    let Some(t) = pm.target else { continue };
                    if self.magic_attack(pm.caster, t, pm.magic, element::FIRE, 100) <= 0 {
                        continue;
                    }
                    let (pmin, pmax, _) = self.caster_power(pm.caster, pm.magic);
                    let duration = self.roll_range(pmin, pmax).max(2);
                    let cs = self.objects[&pm.caster].stats;
                    let value = self
                        .roll_lucky(cs.min_sc, cs.max_sc, cs.luck)
                        .min(self.roll_lucky(cs.min_mc, cs.max_mc, cs.luck))
                        / 2;
                    self.apply_poison(
                        t,
                        Poison {
                            kind: poison_kind::HELL_FIRE,
                            value: value.max(1),
                            ticks_left: duration / 2,
                            next_tick: self.now + 2000,
                            owner: Some(pm.caster),
                        },
                    );
                }
                magic_type::SUMMON_PUPPET => {
                    if let Some(t) = pm.target {
                        // The explosion of a puppet (queued by `puppet_explode`).
                        self.magic_attack(pm.caster, t, pm.magic, element::NONE, 100);
                        continue;
                    }
                    let Some(def_index) = self
                        .data
                        .monsters
                        .values()
                        .find(|d| d.flag == 6)
                        .map(|d| d.index)
                    else {
                        continue;
                    };
                    let level = self.magic_level(pm.caster, pm.magic);
                    let extra = self.objects[&pm.caster]
                        .player()
                        .and_then(|p| {
                            p.magics
                                .iter()
                                .find(|m| m.magic == magic_type::ART_OF_SHADOWS)
                        })
                        .map(|m| m.power_range(&self.data.magics[&m.magic]).0)
                        .unwrap_or(0);
                    for _ in 0..(level + 1 + extra) {
                        let mut spot = None;
                        for _ in 0..25 {
                            let p = Point::new(
                                cloc.x + self.rng.random_range(-1..=1),
                                cloc.y + self.rng.random_range(-1..=1),
                            );
                            if self.maps[&cmap].file.is_walkable(p.x, p.y)
                                && !self.cell_blocked(cmap, p, false)
                            {
                                spot = Some(p);
                                break;
                            }
                        }
                        let Some(spot) = spot else { break };
                        let puppet =
                            self.create_monster(def_index, cmap, spot, None, Some(pm.caster), 0);
                        if let Some(m) = self.objects.get_mut(&puppet).and_then(|o| o.monster_mut())
                        {
                            m.explode_at = Some(self.now + 5000);
                        }
                    }
                    // The caster slips away up to four cells and cloaks.
                    for _ in 0..25 {
                        let p = Point::new(
                            cloc.x + self.rng.random_range(-4..=4),
                            cloc.y + self.rng.random_range(-4..=4),
                        );
                        if p != cloc
                            && self.maps[&cmap].file.is_walkable(p.x, p.y)
                            && !self.cell_blocked(cmap, p, false)
                        {
                            self.teleport_object(pm.caster, p);
                            break;
                        }
                    }
                    self.apply_cloak(pm.caster, true);
                    self.level_magic(pm.caster, pm.magic);
                }
                // ---- Taoist wave 3 ----
                magic_type::INVISIBILITY | magic_type::TRANSPARENCY => {
                    let (pmin, pmax, sc) = self.caster_power(pm.caster, pm.magic);
                    let power = self.roll_range(pmin, pmax);
                    let sc = self.roll_range(sc.0, sc.1);
                    let (kind, secs) = if pm.magic == magic_type::INVISIBILITY {
                        (buff_type::INVISIBILITY, (power + sc).max(1) as u64)
                    } else {
                        (
                            buff_type::TRANSPARENCY,
                            (power + sc / 2).clamp(1, 3600) as u64,
                        )
                    };
                    self.buff_add(pm.caster, kind, secs * 1000, BuffStats::default());
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::CELESTIAL_LIGHT => {
                    let level = self.magic_level(pm.caster, pm.magic);
                    let (pmin, pmax, _) = self.caster_power(pm.caster, pm.magic);
                    let secs = self.roll_range(pmin, pmax).max(1) as u64;
                    self.buff_add(
                        pm.caster,
                        buff_type::CELESTIAL_LIGHT,
                        secs * 1000,
                        BuffStats {
                            celestial: (level + 1) * 10,
                            ..BuffStats::default()
                        },
                    );
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::MASS_INVISIBILITY
                | magic_type::ELEMENTAL_SUPERIORITY
                | magic_type::BLOOD_LUST => {
                    let level = self.magic_level(pm.caster, pm.magic);
                    let (pmin, pmax, sc) = self.caster_power(pm.caster, pm.magic);
                    let radius = if pm.magic == magic_type::MASS_INVISIBILITY {
                        2
                    } else {
                        3
                    };
                    let players: Vec<ObjectId> = self
                        .on_map(cmap)
                        .filter(|o| {
                            o.is_player() && !o.dead && o.location.distance(pm.location) <= radius
                        })
                        .map(|o| o.id)
                        .collect();
                    for t in players {
                        let power = self.roll_range(pmin, pmax);
                        let scr = self.roll_range(sc.0, sc.1);
                        match pm.magic {
                            magic_type::MASS_INVISIBILITY => {
                                if self.objects[&t].has_buff(buff_type::INVISIBILITY) {
                                    continue;
                                }
                                self.buff_add(
                                    t,
                                    buff_type::INVISIBILITY,
                                    (power + scr).max(1) as u64 * 1000,
                                    BuffStats::default(),
                                );
                            }
                            magic_type::ELEMENTAL_SUPERIORITY => self.buff_add(
                                t,
                                buff_type::ELEMENTAL_SUPERIORITY,
                                (power + scr * 2).max(1) as u64 * 1000,
                                BuffStats {
                                    max_mc: 5 + level,
                                    max_sc: 5 + level,
                                    ..BuffStats::default()
                                },
                            ),
                            _ => self.buff_add(
                                t,
                                buff_type::BLOOD_LUST,
                                (power + scr * 2).max(1) as u64 * 1000,
                                BuffStats {
                                    max_dc: 5 + level,
                                    ..BuffStats::default()
                                },
                            ),
                        }
                        self.level_magic(pm.caster, pm.magic);
                    }
                }
                magic_type::TRAP_OCTAGON => {
                    let (pmin, pmax, sc) = self.caster_power(pm.caster, pm.magic);
                    let trapped: Vec<ObjectId> = self
                        .on_map(cmap)
                        .filter(|o| !o.dead && o.location.distance(pm.location) <= 1)
                        .filter_map(|o| match &o.kind {
                            Kind::Monster(m) if m.owner.is_none() => {
                                Some((o.id, self.data.monsters[&m.def].level))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .into_iter()
                        .filter(|(_, ml)| *ml < clevel + self.rng.random_range(0..3))
                        .map(|(id, _)| id)
                        .collect();
                    if trapped.is_empty() {
                        continue;
                    }
                    let secs =
                        (self.roll_range(sc.0, sc.1) + self.roll_range(pmin, pmax)).max(1) as u64;
                    for t in &trapped {
                        if let Some(m) = self.objects.get_mut(t).and_then(|o| o.monster_mut()) {
                            m.shock_until = m.shock_until.max(self.now + secs * 1000);
                            m.target = None;
                        }
                        self.level_magic(pm.caster, pm.magic);
                    }
                    let c = pm.location;
                    for (dx, dy) in [
                        (-1, -2),
                        (-1, 2),
                        (1, -2),
                        (1, 2),
                        (-2, -1),
                        (-2, 1),
                        (2, -1),
                        (2, 1),
                    ] {
                        let cell = Point::new(c.x + dx, c.y + dy);
                        if !self.maps[&cmap].file.is_walkable(cell.x, cell.y) {
                            continue;
                        }
                        self.spawn_spell(
                            cmap,
                            cell,
                            spell_effect::TRAP_OCTAGON,
                            secs as i32 * 4,
                            250,
                            pm.caster,
                            pm.magic,
                        );
                        let sid = *self.maps[&cmap]
                            .objects_at(cell)
                            .iter()
                            .rev()
                            .find(|v| self.objects[v].is_spell())
                            .unwrap();
                        if let Kind::Spell(sd) = &mut self.objects.get_mut(&sid).unwrap().kind {
                            sd.targets = trapped.clone();
                        }
                    }
                }
                magic_type::COMBAT_KICK => {
                    let level = self.magic_level(pm.caster, pm.magic);
                    let dir = self.objects[&pm.caster].direction;
                    let victims: Vec<ObjectId> = self.maps[&cmap]
                        .objects_at(pm.location)
                        .iter()
                        .copied()
                        .filter(|v| self.objects[v].is_monster() && !self.objects[v].dead)
                        .collect();
                    let (pmin, pmax, _) = self.caster_power(pm.caster, pm.magic);
                    for v in victims {
                        let (mlevel, boss) = match &self.objects[&v].kind {
                            Kind::Monster(m) => (
                                self.data.monsters[&m.def].level,
                                self.data.monsters[&m.def].is_boss,
                            ),
                            _ => continue,
                        };
                        if mlevel >= clevel || boss {
                            continue;
                        }
                        if self.rng.random_range(0..16) >= 6 + level * 3 + clevel - mlevel {
                            continue;
                        }
                        let distance = self.roll_range(pmin, pmax).max(1);
                        let mut pushed = 0;
                        let mut at = self.objects[&v].location;
                        for _ in 0..distance {
                            let next = at.step(dir, 1);
                            if self.cell_blocked(cmap, next, false) {
                                break;
                            }
                            at = next;
                            pushed += 1;
                        }
                        if pushed == 0 {
                            continue;
                        }
                        let from = self.objects[&v].location;
                        self.move_object(v, at);
                        if let Some(o) = self.objects.get_mut(&v) {
                            o.direction = dir.rotate(4);
                        }
                        self.events.push((
                            v,
                            ServerMessage::ObjectMove {
                                id: v,
                                from,
                                to: at,
                                direction: dir,
                                run: pushed > 1,
                            },
                        ));
                        let power = {
                            let s = self.objects[&pm.caster].stats;
                            self.roll_dc(s)
                        };
                        self.pending_hits.push(PendingHit {
                            time: self.now,
                            attacker: pm.caster,
                            target_cell: (cmap, at),
                            power,
                            target: None,
                            magics: vec![magic_type::COMBAT_KICK],
                            primary: true,
                            raw: false,
                            element: element::NONE,
                            ranged: false,
                        });
                        self.level_magic(pm.caster, pm.magic);
                        break;
                    }
                }
                magic_type::RESURRECTION => {
                    let Some(t) = pm.target else { continue };
                    if !self
                        .objects
                        .get(&t)
                        .map(|o| o.dead && o.is_player())
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    let level = self.magic_level(pm.caster, pm.magic);
                    if self.rng.random_range(0..100) > 25 + level * 25 {
                        continue;
                    }
                    let (pmin, pmax, _) = self.caster_power(pm.caster, pm.magic);
                    let pct = self.roll_range(pmin, pmax).clamp(1, 100);
                    self.revive_in_place(t, pct);
                    self.level_magic(pm.caster, pm.magic);
                    if let Some(um) = self
                        .objects
                        .get_mut(&pm.caster)
                        .and_then(|o| o.player_mut())
                        .and_then(|p| p.magics.iter_mut().find(|m| m.magic == pm.magic))
                    {
                        um.cooldown_until = self.now + 20_000;
                    }
                    self.send_to(
                        pm.caster,
                        ServerMessage::MagicCooldown {
                            magic: pm.magic,
                            delay_ms: 20_000,
                        },
                    );
                }
                magic_type::PURIFICATION => {
                    let Some(t) = pm.target else { continue };
                    let level = self.magic_level(pm.caster, pm.magic);
                    if self.rng.random_range(0..100) > 40 + level * 20 {
                        continue;
                    }
                    let Some(o) = self.objects.get_mut(&t) else {
                        continue;
                    };
                    let removed = o.poisons.len();
                    if o.is_player() {
                        o.poisons.clear();
                    }
                    if removed > 0 && o.is_player() {
                        self.send_poison_status(t);
                        for _ in 0..removed {
                            self.level_magic(pm.caster, pm.magic);
                        }
                    }
                }
                // ---- Pets ----
                magic_type::ELECTRIC_SHOCK => {
                    let Some(t) = pm.target else { continue };
                    let Some((mlevel, boss, can_tame, owner, dead)) =
                        self.objects.get(&t).and_then(|o| match &o.kind {
                            Kind::Monster(m) => {
                                let d = &self.data.monsters[&m.def];
                                Some((d.level, d.is_boss, d.can_tame, m.owner, o.dead))
                            }
                            _ => None,
                        })
                    else {
                        continue;
                    };
                    if dead || boss {
                        continue;
                    }
                    let level = self.magic_level(pm.caster, pm.magic);
                    // Random.Next(MagicMaxLevel + 1) > Level => fail (half chance of exp).
                    if self.rng.random_range(0..5) > level {
                        if self.rng.random_range(0..2) == 0 {
                            self.level_magic(pm.caster, pm.magic);
                        }
                        continue;
                    }
                    self.level_magic(pm.caster, pm.magic);
                    let shock_ms = (level as u64 * 5 + 10) * 1000;
                    if owner == Some(pm.caster) || self.rng.random_range(0..2) > 0 {
                        if let Some(m) = self.objects.get_mut(&t).and_then(|o| o.monster_mut()) {
                            m.shock_until = self.now + shock_ms;
                            m.target = None;
                        }
                        continue;
                    }
                    if mlevel > clevel + 2 || !can_tame {
                        continue;
                    }
                    if self.rng.random_range(0..(clevel + 20 + level * 5).max(1)) <= mlevel + 10 {
                        continue;
                    }
                    let pets = self.objects[&pm.caster]
                        .player()
                        .map(|p| p.pets.len())
                        .unwrap_or(0);
                    if pets >= 3 || self.rng.random_range(0..4) > 0 {
                        continue;
                    }
                    if self.rng.random_range(0..20) == 0 {
                        let hp = self.objects[&t].hp;
                        self.damage(t, pm.caster, hp.max(1), element::NONE, true);
                        continue;
                    }
                    if let Some(old) = owner {
                        if let Some(p) = self.objects.get_mut(&old).and_then(|o| o.player_mut()) {
                            p.pets.retain(|x| *x != t);
                        }
                        let o = self.objects.get_mut(&t).unwrap();
                        o.hp = o.hp.min((o.max_hp / 10).max(1));
                    }
                    self.tame(t, pm.caster, level, (level as u64 + 1) * 3_600_000);
                }
                magic_type::SUMMON_SKELETON
                | magic_type::SUMMON_JIN_SKELETON
                | magic_type::SUMMON_SHINSU => {
                    let flag = match pm.magic {
                        magic_type::SUMMON_SKELETON => 1,
                        magic_type::SUMMON_JIN_SKELETON => 2,
                        _ => 3,
                    };
                    let Some(def_index) = self
                        .data
                        .monsters
                        .values()
                        .find(|d| d.flag == flag)
                        .map(|d| d.index)
                    else {
                        continue;
                    };
                    let level = self.magic_level(pm.caster, pm.magic);
                    let pets: Vec<ObjectId> = self.objects[&pm.caster]
                        .player()
                        .map(|p| p.pets.clone())
                        .unwrap_or_default();
                    // Re-casting recalls an existing pet of that kind.
                    if let Some(existing) = pets.iter().copied().find(|p| {
                        matches!(self.objects.get(p).map(|o| &o.kind), Some(Kind::Monster(m)) if m.def == def_index)
                    }) {
                        let dir = self.objects[&pm.caster].direction;
                        let behind = cloc.step(dir.rotate(4), 1);
                        let to = self.summon_placement(cmap, behind, cloc);
                        self.teleport_object(existing, to);
                        continue;
                    }
                    if pets.len() >= 2 {
                        continue;
                    }
                    let spot = self.summon_placement(cmap, pm.location, cloc);
                    self.create_monster(def_index, cmap, spot, None, Some(pm.caster), level * 2);
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::STRENGTH_OF_FAITH => {
                    let level = self.magic_level(pm.caster, pm.magic);
                    let (pmin, pmax) = {
                        let p = self.objects[&pm.caster].player().unwrap();
                        let um = p.magics.iter().find(|m| m.magic == pm.magic).unwrap();
                        um.power_range(&self.data.magics[&pm.magic])
                    };
                    let secs = self.roll_range(pmin, pmax).max(1) as u64;
                    self.buff_add(
                        pm.caster,
                        buff_type::STRENGTH_OF_FAITH,
                        secs * 1000,
                        BuffStats {
                            dc_pct: (level + 1) * -20,
                            pet_dc_pct: (level + 1) * 30,
                            ..BuffStats::default()
                        },
                    );
                    self.level_magic(pm.caster, pm.magic);
                }
                // ---- Wizard wave 3 ----
                magic_type::RENOUNCE => {
                    let level = self.magic_level(pm.caster, pm.magic);
                    self.buff_add(
                        pm.caster,
                        buff_type::RENOUNCE,
                        (30 + level as u64 * 30) * 1000,
                        BuffStats {
                            hp_pct: -(1 + level) * 10,
                            mc_pct: (1 + level) * 10,
                            ..BuffStats::default()
                        },
                    );
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::EXPEL_UNDEAD => {
                    let Some(t) = pm.target else { continue };
                    let Some((mlevel, boss, undead, dead)) =
                        self.objects.get(&t).and_then(|o| match &o.kind {
                            Kind::Monster(m) => {
                                let d = &self.data.monsters[&m.def];
                                Some((d.level, d.is_boss, d.undead, o.dead))
                            }
                            _ => None,
                        })
                    else {
                        continue;
                    };
                    if dead || boss || !undead || mlevel >= 70 {
                        continue;
                    }
                    if let Some(m) = self.objects.get_mut(&t).and_then(|o| o.monster_mut()) {
                        if m.target.is_none() {
                            m.target = Some(pm.caster);
                        }
                    }
                    let level = self.magic_level(pm.caster, pm.magic);
                    if mlevel >= clevel - 1 + self.rng.random_range(0..4) {
                        continue;
                    }
                    let chance = 35 + level * 9 + (clevel - mlevel) * 5;
                    if self.rng.random_range(0..100) >= chance {
                        continue;
                    }
                    let hp = self.objects[&t].hp;
                    self.damage(t, pm.caster, hp.max(1), element::NONE, true);
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::GEO_MANIPULATION => {
                    if pm.location == cloc {
                        continue;
                    }
                    let level = self.magic_level(pm.caster, pm.magic);
                    if self.rng.random_range(0..100) > 25 + level * 25 {
                        continue;
                    }
                    let ok = self.maps[&cmap]
                        .file
                        .is_walkable(pm.location.x, pm.location.y)
                        && !self.cell_blocked(cmap, pm.location, false);
                    if !ok {
                        continue;
                    }
                    self.teleport_with_effects(pm.caster, pm.location);
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::FIRE_STORM
                | magic_type::LIGHTNING_WAVE
                | magic_type::ICE_STORM
                | magic_type::DRAGON_TORNADO => {
                    let victims: Vec<ObjectId> = self
                        .on_map(cmap)
                        .filter(|o| {
                            o.is_monster() && !o.dead && o.location.distance(pm.location) <= 1
                        })
                        .map(|o| o.id)
                        .collect();
                    let elem = match pm.magic {
                        magic_type::FIRE_STORM => element::FIRE,
                        magic_type::LIGHTNING_WAVE => element::LIGHTNING,
                        magic_type::ICE_STORM => element::ICE,
                        _ => element::WIND,
                    };
                    for v in victims {
                        let dealt = self.magic_attack(pm.caster, v, pm.magic, elem, 100);
                        if dealt > 0 {
                            match pm.magic {
                                magic_type::ICE_STORM => self.try_slow(v, pm.caster, 5, 5),
                                magic_type::DRAGON_TORNADO => self.try_repel(v, pm.caster, 5),
                                _ => {}
                            }
                        }
                    }
                }
                magic_type::TEMPEST => {
                    let level = self.magic_level(pm.caster, pm.magic);
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let cell = Point::new(pm.location.x + dx, pm.location.y + dy);
                            if !self.maps[&cmap].file.is_walkable(cell.x, cell.y) {
                                continue;
                            }
                            self.spawn_spell(
                                cmap,
                                cell,
                                spell_effect::TEMPEST,
                                (level + 2) * 5,
                                2000,
                                pm.caster,
                                pm.magic,
                            );
                        }
                    }
                    self.level_magic(pm.caster, pm.magic);
                }
                magic_type::METEOR_SHOWER => {
                    if let Some(t) = pm.target {
                        self.magic_attack(pm.caster, t, pm.magic, element::FIRE, 100);
                    }
                }
                magic_type::CHAIN_LIGHTNING => {
                    let Some((divisor, mut visited)) = pm.chain.clone() else {
                        continue;
                    };
                    let cell = pm.location;
                    let victims: Vec<ObjectId> = self.maps[&cmap]
                        .objects_at(cell)
                        .iter()
                        .copied()
                        .filter(|v| {
                            let o = &self.objects[v];
                            o.is_monster() && !o.dead && o.location.distance(cloc) <= MAGIC_RANGE
                        })
                        .collect();
                    // Multiplier 5 / (divisor + 5).
                    let scale = 500 / (divisor + 5);
                    let mut any = false;
                    for v in victims {
                        if self.magic_attack(pm.caster, v, pm.magic, element::LIGHTNING, scale) >= 1
                        {
                            any = true;
                        }
                    }
                    visited.push(cell);
                    if !any {
                        continue;
                    }
                    let next_div = divisor + 1;
                    let mut next_cells = Vec::new();
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let c = Point::new(cell.x + dx, cell.y + dy);
                            if visited.contains(&c) || next_cells.contains(&c) {
                                continue;
                            }
                            let has_monster = self.maps[&cmap].objects_at(c).iter().any(|v| {
                                let o = &self.objects[v];
                                o.is_monster()
                                    && !o.dead
                                    && o.location.distance(cloc) <= MAGIC_RANGE
                            });
                            if has_monster && self.rng.random_range(0..next_div) == 0 {
                                next_cells.push(c);
                            }
                        }
                    }
                    for c in next_cells {
                        self.pending_magics.push(PendingMagic {
                            time: self.now + 200,
                            caster: pm.caster,
                            magic: pm.magic,
                            target: None,
                            location: c,
                            direction: None,
                            primary: true,
                            chain: Some((next_div, visited.clone())),
                        });
                    }
                }
                _ => {}
            }
            let _ = cloc;
        }
    }

    /// Zircon `MagicAttack` slow proc: 1-in-`chance` on non-boss monsters,
    /// value doubled, 6..10 s.
    pub(super) fn try_slow(&mut self, target: ObjectId, owner: ObjectId, chance: i32, level: i32) {
        if self.rng.random_range(0..chance.max(1)) != 0 {
            return;
        }
        let boss = match &self.objects.get(&target).map(|o| &o.kind) {
            Some(Kind::Monster(m)) => self.data.monsters[&m.def].is_boss,
            _ => return,
        };
        if boss {
            return;
        }
        let secs = (3 + self.rng.random_range(0..3)) * 2;
        self.apply_poison(
            target,
            Poison {
                kind: poison_kind::SLOW,
                value: level * 2,
                ticks_left: 0,
                next_tick: self.now + secs as u64 * 1000,
                owner: Some(owner),
            },
        );
    }

    /// Zircon `MagicAttack` repel proc: 1-in-`chance`, one cell away from the
    /// caster, only on lower-level monsters.
    pub(super) fn try_repel(&mut self, target: ObjectId, caster: ObjectId, chance: i32) {
        if self.rng.random_range(0..chance.max(1)) != 0 {
            return;
        }
        let (map, tloc, cloc, clevel) = {
            let c = &self.objects[&caster];
            let t = &self.objects[&target];
            (
                t.map,
                t.location,
                c.location,
                c.player().map(|p| p.level).unwrap_or(1),
            )
        };
        let (mlevel, boss) = match &self.objects[&target].kind {
            Kind::Monster(m) => (
                self.data.monsters[&m.def].level,
                self.data.monsters[&m.def].is_boss,
            ),
            _ => return,
        };
        if boss || clevel <= mlevel || cloc == tloc {
            return;
        }
        let dir = Direction::from_points(cloc, tloc);
        let rotation = if self.rng.random_range(0..2) == 0 {
            1
        } else {
            -1
        };
        for d in [dir, dir.rotate(rotation), dir.rotate(-rotation)] {
            let to = tloc.step(d, 1);
            if self.cell_blocked(map, to, false) {
                continue;
            }
            self.move_object(target, to);
            if let Some(o) = self.objects.get_mut(&target) {
                o.direction = d.rotate(4);
            }
            self.events.push((
                target,
                ServerMessage::ObjectMove {
                    id: target,
                    from: tloc,
                    to,
                    direction: d,
                    run: false,
                },
            ));
            return;
        }
    }

    /// Level of a learned skill (0 when unknown).
    pub(super) fn magic_level(&self, id: ObjectId, magic: u16) -> i32 {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .and_then(|p| p.magics.iter().find(|m| m.magic == magic))
            .map(|m| m.level as i32)
            .unwrap_or(0)
    }

    /// Zircon `Teleport`: out effect on the old cell, jump, in effect.
    pub(super) fn teleport_with_effects(&mut self, id: ObjectId, to: Point) {
        let from = self.objects[&id].location;
        self.events.push((
            id,
            ServerMessage::ObjectEffect {
                id,
                effect: effect::TELEPORT_OUT,
                location: from,
            },
        ));
        self.teleport_object(id, to);
        self.events.push((
            id,
            ServerMessage::ObjectEffect {
                id,
                effect: effect::TELEPORT_IN,
                location: to,
            },
        ));
    }

    /// Power range of the caster's skill plus its SC range.
    pub(super) fn caster_power(&self, id: ObjectId, magic: u16) -> (i32, i32, (i32, i32)) {
        let o = &self.objects[&id];
        let p = o.player().unwrap();
        let um = p.magics.iter().find(|m| m.magic == magic).unwrap();
        let (pmin, pmax) = um.power_range(&self.data.magics[&magic]);
        (pmin, pmax, (o.stats.min_sc, o.stats.max_sc))
    }

    /// Zircon `Cloak.MagicComplete` / `SummonPuppet.CloakEnd`: cloak with an
    /// HP drain reduced by Pledge Of Blood; Ghost Walk by roll or forced.
    pub(super) fn apply_cloak(&mut self, id: ObjectId, force_ghost: bool) {
        if self.objects[&id].has_buff(buff_type::CLOAK) {
            return;
        }
        let level = self.magic_level(id, magic_type::CLOAK);
        let pledge = self.objects[&id]
            .player()
            .and_then(|p| {
                p.magics
                    .iter()
                    .find(|m| m.magic == magic_type::PLEDGE_OF_BLOOD)
            })
            .map(|m| m.power_range(&self.data.magics[&m.magic]));
        let pledge_value = match pledge {
            Some((lo, hi)) => {
                self.level_magic(id, magic_type::PLEDGE_OF_BLOOD);
                self.roll_range(lo, hi)
            }
            None => 0,
        };
        let max_hp = self.objects[&id].max_hp;
        let drain = (max_hp * (20 - level - pledge_value).max(0) / 1000).max(0);
        self.buff_add(
            id,
            buff_type::CLOAK,
            u64::MAX,
            BuffStats {
                cloak_damage: drain,
                ..BuffStats::default()
            },
        );
        let ghost_level = self.objects[&id]
            .player()
            .and_then(|p| p.magics.iter().find(|m| m.magic == magic_type::GHOST_WALK))
            .map(|m| m.level as i32);
        let ghost = if force_ghost {
            true
        } else if let Some(gl) = ghost_level {
            let rate = (gl + 1) * 3;
            let ok = self.rng.random_range(0..(2 + rate)) < rate;
            if ok {
                self.level_magic(id, magic_type::GHOST_WALK);
            }
            ok
        } else {
            false
        };
        if ghost {
            self.buff_add(id, buff_type::GHOST_WALK, u64::MAX, BuffStats::default());
        }
    }
}
