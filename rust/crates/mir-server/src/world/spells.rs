use super::*;

impl World {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn spawn_spell(
        &mut self,
        map: i32,
        location: Point,
        effect: u8,
        ticks: i32,
        frequency: u64,
        owner: ObjectId,
        magic: u16,
    ) -> ObjectId {
        // Zircon: a new fire wall replaces any fire wall on the cell.
        let old: Vec<ObjectId> = self.maps[&map]
            .objects_at(location)
            .iter()
            .copied()
            .filter(|v| {
                matches!(&self.objects[v].kind, Kind::Spell(s)
                    if s.effect == effect
                        || matches!((s.effect, effect), (spell_effect::FIRE_WALL, spell_effect::TEMPEST) | (spell_effect::TEMPEST, spell_effect::FIRE_WALL)))
            })
            .collect();
        for v in old {
            self.remove_object(v);
        }
        let id = self.alloc_id();
        let obj = Object {
            id,
            kind: Kind::Spell(SpellData {
                effect,
                tick_count: ticks,
                tick_frequency: frequency,
                tick_time: 0,
                owner,
                magic,
                targets: Vec::new(),
            }),
            map,
            location,
            direction: Direction::Up,
            hp: 0,
            max_hp: 0,
            dead: false,
            stats: CombatStats::ZERO,
            action_time: 0,
            move_time: 0,
            attack_time: 0,
            cell_time: 0,
            in_safe_zone: false,
            light: match effect {
                spell_effect::FIRE_WALL
                | spell_effect::TEMPEST
                | spell_effect::ICE_AURA
                | spell_effect::BURNING_FIRE => 15,
                _ => 0,
            },
            appearance: Appearance::Spell { effect },
            visible: HashSet::new(),
            poisons: Vec::new(),
            heal: None,
        };
        self.insert_object(obj);
        id
    }

    /// Zircon `SpellObject.Process`: tick every `frequency` until the count
    /// runs out; despawn when the owner is gone.
    pub(super) fn process_spells(&mut self) {
        let now = self.now;
        let spells: Vec<ObjectId> = self
            .objects
            .values()
            .filter(|o| o.is_spell())
            .map(|o| o.id)
            .collect();
        for sid in spells {
            let (effect, owner, map, loc, magic, due, done) = {
                let o = &self.objects[&sid];
                let Kind::Spell(s) = &o.kind else { continue };
                (
                    s.effect,
                    s.owner,
                    o.map,
                    o.location,
                    s.magic,
                    now >= s.tick_time,
                    s.tick_count <= 0,
                )
            };
            let owner_gone = self.objects.get(&owner).map(|o| o.dead).unwrap_or(true);
            if owner_gone {
                self.remove_object(sid);
                continue;
            }
            if !due {
                continue;
            }
            if done {
                if effect == spell_effect::FIRE_WALL {
                    self.events.push((
                        sid,
                        ServerMessage::MapEffect {
                            location: loc,
                            effect: effect::FIRE_WALL_SMOKE,
                        },
                    ));
                }
                self.remove_object(sid);
                continue;
            }
            {
                let o = self.objects.get_mut(&sid).unwrap();
                let Kind::Spell(s) = &mut o.kind else {
                    continue;
                };
                s.tick_count -= 1;
                s.tick_time = now + s.tick_frequency;
            }
            match effect {
                spell_effect::FIRE_WALL => {
                    let victims: Vec<ObjectId> = self.maps[&map]
                        .objects_at(loc)
                        .iter()
                        .copied()
                        .filter(|v| {
                            !self.objects[v].dead
                                && self.objects[&owner].hostile_to(&self.objects[v])
                        })
                        .collect();
                    for v in victims {
                        self.firewall_hit(owner, v, magic);
                    }
                }
                spell_effect::DEATH_CLOUD => {
                    // The cloud bursts once on whoever stands in it.
                    let victims: Vec<ObjectId> = self.maps[&map]
                        .objects_at(loc)
                        .iter()
                        .copied()
                        .filter(|v| {
                            self.objects
                                .get(&owner)
                                .is_some_and(|c| c.hostile_to(&self.objects[v]))
                        })
                        .collect();
                    for v in victims {
                        // Zircon's death cloud delivers two fixed physical hits, not caster DC.
                        for _ in 0..2 {
                            self.pending_hits.push(PendingHit {
                                time: now,
                                attacker: owner,
                                target_cell: (map, loc),
                                power: 4000,
                                target: Some(v),
                                magics: Vec::new(),
                                primary: true,
                                raw: false,
                                element: element::NONE,
                                ranged: true,
                            });
                        }
                    }
                }
                spell_effect::ICE_AURA => {
                    // Everyone standing in the aura is paralysed for the tick.
                    let level = self.magic_level(owner, magic);
                    let victims: Vec<ObjectId> = self.maps[&map]
                        .objects_at(loc)
                        .iter()
                        .copied()
                        .filter(|v| self.objects[&owner].hostile_to(&self.objects[v]))
                        .collect();
                    let freq = match &self.objects[&sid].kind {
                        Kind::Spell(s) => s.tick_frequency,
                        _ => 2000,
                    };
                    for v in victims {
                        self.apply_poison(
                            v,
                            Poison {
                                kind: poison_kind::PARALYSIS,
                                value: (3 + level) * 2,
                                ticks_left: 0,
                                next_tick: now + freq,
                                owner: Some(owner),
                            },
                        );
                    }
                }
                spell_effect::BURNING_FIRE => {
                    // A mine: the first hostile to step in sets it off.
                    let stepped = self.maps[&map]
                        .objects_at(loc)
                        .iter()
                        .any(|v| self.objects[&owner].hostile_to(&self.objects[v]));
                    if stepped {
                        let victims: Vec<ObjectId> = self
                            .on_map(map)
                            .filter(|o| !o.dead && o.location.distance(loc) <= 1)
                            .filter(|o| self.objects[&owner].hostile_to(o))
                            .map(|o| o.id)
                            .collect();
                        for v in victims {
                            self.magic_attack(owner, v, magic, element::FIRE, 100);
                        }
                        self.events.push((
                            sid,
                            ServerMessage::MapEffect {
                                effect: effect::BURNING_FIRE,
                                location: loc,
                            },
                        ));
                        self.remove_object(sid);
                        continue;
                    }
                }
                spell_effect::DARK_SOUL_PRISON => {
                    // Radius-3 dark field around the prison's centre.
                    let victims: Vec<ObjectId> = self
                        .on_map(map)
                        .filter(|o| !o.dead && o.location.distance(loc) <= 3)
                        .filter(|o| self.objects[&owner].hostile_to(o))
                        .map(|o| o.id)
                        .collect();
                    for v in victims {
                        self.magic_attack(owner, v, magic, element::DARK, 40);
                    }
                }
                spell_effect::TEMPEST => {
                    let victims: Vec<ObjectId> = self.maps[&map]
                        .objects_at(loc)
                        .iter()
                        .copied()
                        .filter(|v| self.objects[v].is_monster() && !self.objects[v].dead)
                        .collect();
                    for v in victims {
                        if self.magic_attack(owner, v, magic, element::WIND, 80) > 0 {
                            self.try_repel(v, owner, 5);
                        }
                    }
                }
                spell_effect::TRAP_OCTAGON => {
                    let held = {
                        let Kind::Spell(s) = &self.objects[&sid].kind else {
                            continue;
                        };
                        s.targets.iter().any(|t| {
                            self.objects
                                .get(t)
                                .and_then(|o| match &o.kind {
                                    Kind::Monster(m) => Some(!o.dead && m.shock_until > now),
                                    _ => None,
                                })
                                .unwrap_or(false)
                        })
                    };
                    if !held {
                        self.remove_object(sid);
                    }
                }
                spell_effect::POISONOUS_CLOUD if self.objects[&owner].is_monster() => {
                    // A monster's cloud would buff its allies; monsters carry
                    // no buffs here, so it only lingers.
                }
                spell_effect::POISONOUS_CLOUD => {
                    // Everyone within 2 cells gains +5 agility while it lasts.
                    let until = {
                        let Kind::Spell(s) = &self.objects[&sid].kind else {
                            continue;
                        };
                        s.tick_time
                    };
                    let players: Vec<ObjectId> = self
                        .on_map(map)
                        .filter(|o| o.is_player() && o.location.distance(loc) <= 2)
                        .map(|o| o.id)
                        .collect();
                    for pid in players {
                        let stats = BuffStats {
                            agility: 5,
                            ..BuffStats::default()
                        };
                        self.buff_add(
                            pid,
                            buff_type::POISONOUS_CLOUD,
                            until.saturating_sub(now),
                            stats,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn firewall_hit(&mut self, owner: ObjectId, target: ObjectId, magic: u16) {
        let Some(caster) = self.objects.get(&owner) else {
            return;
        };
        let Some(victim) = self.objects.get(&target) else {
            return;
        };
        if caster.dead || !caster.hostile_to(victim) {
            return;
        }
        if caster.is_monster() {
            let stats = caster.stats;
            let target_stats = victim.stats;
            let dc = self.roll_dc(stats);
            let dealt = dc - self.roll_range(target_stats.min_mr, target_stats.max_mr);
            if dealt > 0 {
                self.damage(target, owner, dealt, element::FIRE, true);
            }
        } else {
            self.magic_attack(owner, target, magic, element::FIRE, 60);
        }
    }

    /// Cells of Zircon's line spells: 8 steps ahead, each with two flanks.
    pub(super) fn line_cells(loc: Point, dir: Direction) -> Vec<(Point, bool)> {
        let mut out = Vec::with_capacity(24);
        let cardinal = matches!(
            dir,
            Direction::Up | Direction::Right | Direction::Down | Direction::Left
        );
        for i in 1..=8 {
            let c = loc.step(dir, i);
            out.push((c, true));
            let (a, b) = if cardinal { (-2, 2) } else { (1, -1) };
            out.push((c.step(dir.rotate(a), 1), false));
            out.push((c.step(dir.rotate(b), 1), false));
        }
        out
    }
}
