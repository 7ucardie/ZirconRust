use super::*;

impl World {
    pub fn magic_key(&mut self, id: ObjectId, magic: u16, key: u8) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        if key != 0 {
            for m in p.magics.iter_mut() {
                if m.key == key {
                    m.key = 0;
                }
            }
        }
        if let Some(m) = p.magics.iter_mut().find(|m| m.magic == magic) {
            m.key = key.min(12);
        }
    }

    pub fn magic_toggle(&mut self, id: ObjectId, magic: u16, on: bool) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        if p.mounted {
            return;
        }
        if !p.magics.iter().any(|m| m.magic == magic) {
            return;
        }
        match magic {
            magic_type::THRUSTING => p.thrusting_on = on,
            magic_type::HALF_MOON => p.half_moon_on = on,
            magic_type::DESTRUCTIVE_SURGE => p.surge_on = on,
            magic_type::FLAME_SPLASH => p.flame_splash_on = on,
            magic_type::DEMONIC_RECOVERY => {
                self.demonic_recovery(id);
                return;
            }
            m if magic_type::is_charge(m) => {
                self.charge_toggle(id, m);
                return;
            }
            _ => return,
        }
        self.send_to(id, ServerMessage::MagicToggle { magic, on });
    }

    /// Zircon `FlamingSword/DragonRise/BladeStorm.Toggle`: pay, cool down,
    /// arm a 12 s charge and lock the other two for 2 s.
    pub(super) fn charge_toggle(&mut self, id: ObjectId, magic: u16) {
        let Some(def) = self.data.magics.get(&magic).cloned() else {
            return;
        };
        let now = self.now;
        let Some(o) = self.objects.get_mut(&id) else {
            return;
        };
        if o.dead {
            return;
        }
        let p = o.player_mut().unwrap();
        let Some(um) = p.magics.iter().find(|m| m.magic == magic).cloned() else {
            return;
        };
        let cost = um.cost(&def);
        if cost > p.mp || now < um.cooldown_until {
            return;
        }
        p.mp -= cost;
        if let Some(m) = p.magics.iter_mut().find(|m| m.magic == magic) {
            m.cooldown_until = now + def.delay.max(0) as u64;
        }
        let already = p
            .charge
            .map(|(m, until)| m == magic && now < until)
            .unwrap_or(false);
        let mut msgs = Vec::new();
        if already {
            msgs.push(ServerMessage::Chat {
                text: format!("{} is already charged.", def.name),
            });
        } else {
            p.charge = Some((magic, now + 12_000));
            msgs.push(ServerMessage::MagicToggle { magic, on: true });
        }
        if def.delay > 0 {
            msgs.push(ServerMessage::MagicCooldown {
                magic,
                delay_ms: def.delay as u32,
            });
        }
        for other in [
            magic_type::FLAMING_SWORD,
            magic_type::DRAGON_RISE,
            magic_type::BLADE_STORM,
        ] {
            if other == magic {
                continue;
            }
            if let Some(m) = p.magics.iter_mut().find(|m| m.magic == other) {
                if now + 2000 > m.cooldown_until {
                    m.cooldown_until = now + 2000;
                    msgs.push(ServerMessage::MagicCooldown {
                        magic: other,
                        delay_ms: 2000,
                    });
                }
            }
        }
        for m in msgs {
            self.send_to(id, m);
        }
        self.send_player_stats(id);
    }

    /// Charged power attacks expire after 12 s (Zircon `ChargeExpire`).
    pub(super) fn process_charges(&mut self) {
        let now = self.now;
        let expired: Vec<(ObjectId, u16)> = self
            .players()
            .filter_map(|o| {
                let p = o.player()?;
                let (m, until) = p.charge?;
                (now >= until).then_some((o.id, m))
            })
            .collect();
        for (id, magic) in expired {
            if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                p.charge = None;
            }
            let name = self
                .data
                .magics
                .get(&magic)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            self.send_to(id, ServerMessage::MagicToggle { magic, on: false });
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: format!("{name}'s charge has expired."),
                },
            );
        }
    }

    /// Consume `count` talismans from the amulet slot (Zircon `UseAmulet`),
    /// returning the item's holy/dark affinity flags.
    pub(super) fn use_amulet(&mut self, id: ObjectId, count: u32) -> Option<(bool, bool)> {
        self.use_amulet_shape(id, count, 0)
    }

    pub(super) fn use_amulet_shape(
        &mut self,
        id: ObjectId,
        count: u32,
        shape: i32,
    ) -> Option<(bool, bool)> {
        let (info, have) = {
            let p = self.objects.get(&id)?.player()?;
            let it = p.bag.equipment.get(slot::AMULET)?.as_ref()?;
            (it.info, it.count)
        };
        let def = self.data.items.get(&info)?;
        if def.item_type != item_type::AMULET || def.shape != shape || have < count {
            return None;
        }
        let flags = (def.stat(48) > 0, def.stat(49) > 0);
        let change = {
            let p = self.objects.get_mut(&id)?.player_mut()?;
            p.bag.take(Grid::Equipment, slot::AMULET as u8, count)
        };
        self.send_changes(id, change.into_iter().collect());
        self.refresh_stats(id, false);
        Some(flags)
    }

    /// Does the amulet slot hold a talisman with holy affinity?
    pub(super) fn amulet_holy(&self, id: ObjectId) -> bool {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .and_then(|p| p.bag.equipment.get(slot::AMULET)?.as_ref())
            .and_then(|it| self.data.items.get(&it.info))
            .map(|d| d.stat(48) > 0)
            .unwrap_or(false)
    }

    /// Zircon `LevelMagic`: 1..=3 experience per success while the player
    /// level allows it; levels up at the thresholds.
    pub(super) fn level_magic(&mut self, id: ObjectId, magic: u16) {
        let Some(def) = self.data.magics.get(&magic).cloned() else {
            return;
        };
        let exp = self.rng.random_range(1..=SKILL_EXP) as u64;
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let level = p.level;
        let Some(m) = p.magics.iter_mut().find(|m| m.magic == magic) else {
            return;
        };
        let Some(need) = m.need_level(&def) else {
            return;
        };
        let Some(max) = m.next_experience(&def) else {
            return;
        };
        if level < need || m.level >= 3 {
            return;
        }
        m.experience += exp;
        let mut leveled = false;
        if m.experience as i64 >= max && max > 0 {
            m.experience -= max as u64;
            m.level += 1;
            leveled = true;
        }
        let (lvl, xp) = (m.level, m.experience);
        self.send_to(
            id,
            ServerMessage::MagicLeveled {
                magic,
                level: lvl,
                experience: xp,
            },
        );
        if leveled {
            self.refresh_stats(id, false);
            self.send_player_stats(id);
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: format!("{} is now level {}.", def.name, lvl),
                },
            );
        }
    }
}
