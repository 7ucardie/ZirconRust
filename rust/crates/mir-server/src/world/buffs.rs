use super::*;

impl World {
    // ---- buffs, charges, dashes, spell objects -----------------------------

    /// Zircon `BuffAdd`: replaces a buff of the same kind.
    pub(super) fn buff_add(&mut self, id: ObjectId, kind: u16, duration_ms: u64, stats: BuffStats) {
        let now = self.now;
        let Some(o) = self.objects.get_mut(&id) else {
            return;
        };
        let Some(p) = o.player_mut() else {
            return;
        };
        p.buffs.retain(|b| b.kind != kind);
        let expires = if duration_ms == u64::MAX {
            u64::MAX
        } else {
            now + duration_ms
        };
        p.buffs.push(Buff {
            kind,
            expires,
            stats,
            tick_at: now + 2000,
        });
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.send_to(
            id,
            ServerMessage::BuffAdd(BuffSummary {
                kind,
                remaining_ms: duration_ms,
            }),
        );
        if buff_type::is_visible(kind) {
            self.events
                .push((id, ServerMessage::ObjectBuff { id, kind, on: true }));
        }
        if kind == buff_type::STRENGTH_OF_FAITH {
            self.refresh_pets(id);
        }
    }

    pub(super) fn buff_remove(&mut self, id: ObjectId, kind: u16) {
        if kind == buff_type::SOUL_RESONANCE
            && self
                .objects
                .get(&id)
                .and_then(|o| o.player())
                .is_some_and(|p| p.soul_link.is_some())
        {
            self.soul_resonance_remove(id);
            return;
        }
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let before = p.buffs.len();
        p.buffs.retain(|b| b.kind != kind);
        if p.buffs.len() == before {
            return;
        }
        if kind == buff_type::CLOAK {
            // Ghost Walk cannot outlive the cloak.
            p.buffs.retain(|b| b.kind != buff_type::GHOST_WALK);
            self.send_to(
                id,
                ServerMessage::BuffRemove {
                    kind: buff_type::GHOST_WALK,
                },
            );
        }
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.send_to(id, ServerMessage::BuffRemove { kind });
        if buff_type::is_visible(kind) {
            self.events.push((
                id,
                ServerMessage::ObjectBuff {
                    id,
                    kind,
                    on: false,
                },
            ));
        }
        if kind == buff_type::STRENGTH_OF_FAITH {
            self.refresh_pets(id);
        }
    }

    pub(super) fn process_buffs(&mut self) {
        let now = self.now;
        let mut expired = Vec::new();
        let mut drains = Vec::new();
        let mut conversions = Vec::new();
        let mut repulses = Vec::new();
        let mut swords = Vec::new();
        let mut frosts = Vec::new();
        for o in self.players() {
            if let Some(p) = o.player() {
                for b in &p.buffs {
                    if b.expires <= now {
                        expired.push((o.id, b.kind));
                        if b.kind == buff_type::FROST_BITE {
                            frosts.push((o.id, b.stats.frost));
                        }
                    } else if b.kind == buff_type::CLOAK && now >= b.tick_at {
                        // Cloak drains HP every 2 s and drops instead of killing.
                        if b.stats.cloak_damage >= o.hp {
                            expired.push((o.id, b.kind));
                        } else {
                            drains.push((o.id, b.stats.cloak_damage));
                        }
                    } else if b.kind == buff_type::DRAGON_REPULSE && now >= b.tick_at {
                        repulses.push((o.id, b.stats.pool));
                    } else if b.kind == buff_type::ELEMENTAL_SWORDS && now >= b.tick_at {
                        swords.push((o.id, b.stats.pool));
                    } else if b.kind == buff_type::DARK_CONVERSION && now >= b.tick_at {
                        // Dark Conversion: MP into twice the HP every 2 s.
                        if b.stats.pool > p.mp {
                            expired.push((o.id, b.kind));
                        } else {
                            conversions.push((o.id, b.stats.pool));
                        }
                    }
                }
            }
        }
        for (id, stored) in frosts {
            self.frost_bite_burst(id, stored);
        }
        for (id, amount) in drains {
            let (hp, max_hp) = {
                let o = self.objects.get_mut(&id).unwrap();
                o.hp -= amount;
                if let Some(b) = o
                    .player_mut()
                    .and_then(|p| p.buffs.iter_mut().find(|b| b.kind == buff_type::CLOAK))
                {
                    b.tick_at = now + 2000;
                }
                (o.hp, o.max_hp)
            };
            self.events
                .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
            let stats = self.player_stats(&self.objects[&id]);
            self.send_to(id, ServerMessage::StatsChanged(stats));
        }
        for (id, power) in repulses {
            if let Some(b) = self
                .objects
                .get_mut(&id)
                .and_then(|o| o.player_mut())
                .and_then(|p| {
                    p.buffs
                        .iter_mut()
                        .find(|b| b.kind == buff_type::DRAGON_REPULSE)
                })
            {
                b.tick_at = now + 500;
            }
            self.dragon_repulse_tick(id, power);
        }
        for (id, power) in swords {
            if let Some(b) = self
                .objects
                .get_mut(&id)
                .and_then(|o| o.player_mut())
                .and_then(|p| {
                    p.buffs
                        .iter_mut()
                        .find(|b| b.kind == buff_type::ELEMENTAL_SWORDS)
                })
            {
                b.tick_at = now + 5000;
            }
            if self.elemental_swords_tick(id, power) {
                self.buff_consume_stack(id, buff_type::ELEMENTAL_SWORDS);
            }
        }
        for (id, amount) in conversions {
            let (hp, max_hp) = {
                let o = self.objects.get_mut(&id).unwrap();
                o.hp = (o.hp + amount * 2).min(o.max_hp);
                let p = o.player_mut().unwrap();
                p.mp -= amount;
                if let Some(b) = p
                    .buffs
                    .iter_mut()
                    .find(|b| b.kind == buff_type::DARK_CONVERSION)
                {
                    b.tick_at = now + 2000;
                }
                (o.hp, o.max_hp)
            };
            self.events
                .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
            let stats = self.player_stats(&self.objects[&id]);
            self.send_to(id, ServerMessage::StatsChanged(stats));
        }
        for (id, kind) in expired {
            self.buff_remove(id, kind);
        }
    }

    /// Zircon `ApplyPoison`: a stronger instance of the same type wins.
    pub(super) fn apply_poison(&mut self, target: ObjectId, poison: Poison) {
        // The castle lord shrugs off every poison.
        if self.is_castle_lord(target) {
            return;
        }
        let Some(o) = self.objects.get_mut(&target) else {
            return;
        };
        if o.dead || self.rng.random_range(0..100) < o.stats.poison_resistance {
            return;
        }
        // Zircon: Endurance makes players immune to every poison.
        if o.has_buff(buff_type::ENDURANCE) {
            return;
        }
        let newly_stunned = o.is_player()
            && poison.kind == poison_kind::PARALYSIS
            && !o.poisons.iter().any(|p| p.kind == poison_kind::PARALYSIS);
        let before = o.poisons.iter().fold(0, |flags, p| flags | p.kind);
        if let Some(existing) = o.poisons.iter().position(|p| p.kind == poison.kind) {
            if o.poisons[existing].value > poison.value {
                return;
            }
            o.poisons.remove(existing);
        }
        o.poisons.push(poison);
        let kinds = o.poisons.iter().fold(0, |flags, p| flags | p.kind);
        if kinds != before {
            self.events
                .push((target, ServerMessage::ObjectPoisoned { id: target, kinds }));
        }
        if newly_stunned {
            self.send_to(
                target,
                ServerMessage::Say {
                    id: None,
                    kind: mir_proto::ChatKind::System,
                    text: "You are stunned.".into(),
                },
            );
        }
    }

    pub(super) fn process_poisons(&mut self) {
        let now = self.now;
        let ids: Vec<ObjectId> = self
            .objects
            .values()
            .filter(|o| !o.poisons.is_empty())
            .map(|o| o.id)
            .collect();
        let mut spreads: Vec<(ObjectId, ObjectId, i32)> = Vec::new();
        for id in ids {
            let before = self.objects[&id]
                .poisons
                .iter()
                .fold(0, |flags, p| flags | p.kind);
            let (boss, lethal) = {
                let o = &self.objects[&id];
                let ordinary = o.monster().is_some_and(|m| {
                    m.owner.is_none()
                        && !m.guard
                        && m.castle.is_none()
                        && m.room_door.is_none()
                        && !self.data.monsters[&m.def].is_boss
                });
                let boss = o
                    .monster()
                    .is_some_and(|m| self.data.monsters[&m.def].is_boss);
                let lethal = o
                    .poisons
                    .iter()
                    .map(|p| {
                        ordinary
                            && p.kind == poison_kind::GREEN
                            && p.value > 0
                            && p.owner
                                .and_then(|owner| self.objects.get(&owner))
                                .is_some_and(Object::is_player)
                    })
                    .collect::<Vec<_>>();
                (boss, lethal)
            };
            let (damage, owner, lethal_owner) = {
                let o = self.objects.get_mut(&id).unwrap();
                let mut damage = 0;
                let mut owner = None;
                let mut lethal_owner = None;
                for (p, can_kill) in o.poisons.iter_mut().zip(lethal) {
                    if now < p.next_tick {
                        continue;
                    }
                    p.next_tick = now + 2000;
                    p.ticks_left -= 1;
                    if p.kind == poison_kind::PARASITE {
                        if let Some(owner) = p.owner {
                            spreads.push((id, owner, p.value));
                        }
                    }
                    if matches!(
                        p.kind,
                        poison_kind::GREEN
                            | poison_kind::HELL_FIRE
                            | poison_kind::HEMORRHAGE
                            | poison_kind::CONTAINMENT
                            | poison_kind::PARASITE
                            | poison_kind::BINDING
                    ) {
                        damage += p.value;
                        owner = p.owner;
                        if can_kill {
                            lethal_owner = p.owner;
                        }
                    }
                }
                o.poisons.retain(|p| p.ticks_left >= 0);
                if o.dead || boss {
                    damage = 0;
                }
                // ZirconRust: a player's green poison can finish ordinary
                // monsters. C# defaults every Poison.CanKill to false.
                damage = damage.min(o.hp - i32::from(lethal_owner.is_none())).max(0);
                (damage, owner, lethal_owner)
            };
            if damage > 0 {
                let attacker = owner.unwrap_or(id);
                let o = self.objects.get_mut(&id).unwrap();
                o.hp -= damage;
                let (hp, max_hp) = (o.hp, o.max_hp);
                self.events
                    .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
                if let Some(m) = self.objects.get_mut(&id).and_then(|o| o.monster_mut()) {
                    if m.target.is_none() && attacker != id {
                        m.target = Some(attacker);
                    }
                }
                if hp <= 0 {
                    let killer = lethal_owner.expect("only player green poison is lethal");
                    self.objects
                        .get_mut(&id)
                        .unwrap()
                        .monster_mut()
                        .unwrap()
                        .exp_owner
                        .get_or_insert(killer);
                    self.monster_die(id, killer);
                }
            }
            let kinds = self.objects[&id]
                .poisons
                .iter()
                .fold(0, |flags, p| flags | p.kind);
            if kinds != before {
                self.events
                    .push((id, ServerMessage::ObjectPoisoned { id, kinds }));
            }
        }
        // Infection: Parasite ticks spread to a neighbour of the victim.
        for (victim, owner, value) in spreads {
            self.infection_spread(victim, owner, value);
        }
    }

    pub(super) fn send_poison_status(&mut self, id: ObjectId) {
        if let Some(o) = self.objects.get(&id) {
            let kinds = o.poisons.iter().fold(0, |flags, p| flags | p.kind);
            self.events
                .push((id, ServerMessage::ObjectPoisoned { id, kinds }));
        }
    }

    pub(super) fn process_heals(&mut self) {
        let now = self.now;
        let ids: Vec<ObjectId> = self
            .objects
            .values()
            .filter(|o| o.heal.is_some())
            .map(|o| o.id)
            .collect();
        for id in ids {
            let o = self.objects.get_mut(&id).unwrap();
            let Some(h) = o.heal.as_mut() else { continue };
            if now < h.next_tick {
                continue;
            }
            h.next_tick = now + 1000;
            let amount = h.pool.min(h.cap);
            h.pool -= amount;
            o.hp = (o.hp + amount).min(o.max_hp);
            if o.hp >= o.max_hp || h.pool <= 0 || o.dead {
                o.heal = None;
            }
            let (hp, max_hp) = (o.hp, o.max_hp);
            self.events
                .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
            if self.objects[&id].is_player() {
                self.send_player_stats(id);
            }
        }
    }
}
