use super::*;

impl World {
    pub fn item_lock(&mut self, id: ObjectId, grid: Grid, slot: u8, locked: bool) {
        let Some(o) = self.objects.get_mut(&id) else {
            return;
        };
        let safe = o.in_safe_zone;
        let Some(p) = o.player_mut() else { return };
        if grid == Grid::Storage && (!safe || slot as usize >= p.storage_size as usize) {
            return;
        }
        let Some(item) = p
            .bag
            .grid_mut(grid)
            .get_mut(slot as usize)
            .and_then(Option::as_mut)
        else {
            return;
        };
        item.locked = locked;
        let change = (grid, slot, Some(item.instance()));
        self.send_changes(id, vec![change]);
    }

    pub fn item_move(&mut self, id: ObjectId, from: Grid, from_slot: u8, to: Grid, to_slot: u8) {
        let result = self.item_move_inner(id, from, from_slot, to, to_slot);
        match result {
            Ok(changes) => {
                self.send_changes(id, changes);
                self.refresh_stats(id, false);
                self.refresh_appearance(id);
                self.send_player_stats(id);
            }
            Err(e) => self.send_to(id, ServerMessage::Chat { text: e }),
        }
    }

    pub(super) fn item_move_inner(
        &mut self,
        id: ObjectId,
        from: Grid,
        from_slot: u8,
        to: Grid,
        to_slot: u8,
    ) -> Result<Changed, String> {
        let o = self.objects.get(&id).ok_or("no player")?;
        let p = o.player().ok_or("no player")?;
        let (class, gender, level) = (p.class, p.gender, p.level);
        let (max_wear, max_hand) = (p.max_wear, p.max_hand);
        if from == Grid::Equipment && to == Grid::Equipment {
            return Err("Cannot move between equipment slots".into());
        }
        if from == Grid::Storage || to == Grid::Storage {
            if p.trade.is_some() {
                return Err("Cannot use storage while trading".into());
            }
            if !self.in_safe_zone(o.map, o.location) {
                return Err(self.storage_safe_zone_error(o.map, o.location));
            }
            let size = p.storage_size as usize;
            if (from == Grid::Storage && from_slot as usize >= size)
                || (to == Grid::Storage && to_slot as usize >= size)
            {
                return Err("No such storage slot".into());
            }
        }
        if to_slot as usize >= p.bag.grid(to).len() {
            return Err("No such slot".into());
        }
        let src = p
            .bag
            .grid(from)
            .get(from_slot as usize)
            .cloned()
            .flatten()
            .ok_or("Nothing there")?;
        let dst = p.bag.grid(to).get(to_slot as usize).cloned().flatten();
        let poison_merge = ((from == Grid::Inventory
            && to == Grid::Equipment
            && to_slot as usize == slot::POISON)
            || (from == Grid::Equipment
                && from_slot as usize == slot::POISON
                && to == Grid::Inventory))
            && self
                .data
                .items
                .get(&src.info)
                .is_some_and(|def| def.item_type == item_type::POISON);
        let stack = self
            .data
            .items
            .get(&src.info)
            .map(|def| def.stack_size.max(1) as u32)
            .unwrap_or(1);
        let stack = if poison_merge { stack.min(999) } else { stack };
        let merge = (from == Grid::Inventory && to == Grid::Inventory || poison_merge)
            && dst
                .as_ref()
                .is_some_and(|d| d.stacks_with(&src) && d.id != src.id && d.count < stack);
        if to == Grid::Storage {
            let def = self.data.items.get(&src.info).ok_or("Unknown item")?;
            if !def.can_store {
                return Err("That item cannot be stored".into());
            }
        }
        if (to == Grid::Equipment || from == Grid::Equipment) && !(poison_merge && merge) {
            // The item moving INTO equipment must fit; the item moving out
            // (if any) needs no check.
            let (moving_in, target_slot) = if to == Grid::Equipment {
                (Some(&src), to_slot as usize)
            } else {
                (dst.as_ref(), from_slot as usize)
            };
            if let Some(item) = moving_in {
                let def = self.data.items.get(&item.info).ok_or("Unknown item")?;
                if !item_type::slots(def.item_type).contains(&target_slot) {
                    return Err("That does not go there".into());
                }
                can_use(def, class, gender, level)?;
                let replaced_weight = p
                    .bag
                    .equipment
                    .get(target_slot)
                    .and_then(|c| c.as_ref())
                    .map(|c| crate::items::item_weight(&self.data, c))
                    .unwrap_or(0);
                let hand = matches!(
                    def.item_type,
                    item_type::WEAPON | item_type::TORCH | item_type::SHIELD
                );
                let (current, max) = if hand {
                    (p.bag.hand_weight(&self.data), max_hand)
                } else {
                    (p.bag.wear_weight(&self.data), max_wear)
                };
                if current - replaced_weight + def.weight > max {
                    return Err("Too heavy to wear".into());
                }
            }
        }
        let o = self.objects.get_mut(&id).unwrap();
        let p = o.player_mut().unwrap();
        let mut changes = Changed::new();
        // Poison stacks can merge between the bag and poison equipment slot.
        if merge {
            let add = (stack - dst.as_ref().unwrap().count).min(src.count);
            let d = p.bag.grid_mut(to)[to_slot as usize].as_mut().unwrap();
            d.count += add;
            changes.push((to, to_slot, Some(d.instance())));
            let s = p.bag.grid_mut(from)[from_slot as usize].as_mut().unwrap();
            s.count -= add;
            if s.count == 0 {
                p.bag.grid_mut(from)[from_slot as usize] = None;
                changes.push((from, from_slot, None));
            } else {
                changes.push((from, from_slot, Some(s.instance())));
            }
            return Ok(changes);
        }
        if poison_merge && from == Grid::Equipment && dst.is_some() {
            return Err("Cannot move equipped poison onto an occupied slot".into());
        }
        p.bag.grid_mut(from)[from_slot as usize] = dst.clone();
        p.bag.grid_mut(to)[to_slot as usize] = Some(src.clone());
        changes.push((from, from_slot, dst.map(|d| d.instance())));
        changes.push((to, to_slot, Some(src.instance())));
        Ok(changes)
    }

    pub fn item_use(&mut self, id: ObjectId, slot: u8) {
        match self.item_use_inner(id, slot) {
            Ok(changes) => {
                self.send_changes(id, changes);
                self.refresh_stats(id, false);
                self.refresh_appearance(id);
                self.send_player_stats(id);
            }
            Err(e) => self.send_to(id, ServerMessage::Chat { text: e }),
        }
    }

    pub(super) fn item_use_inner(&mut self, id: ObjectId, slot: u8) -> Result<Changed, String> {
        let o = self.objects.get(&id).ok_or("no player")?;
        let p = o.player().ok_or("no player")?;
        let item = p
            .bag
            .inventory
            .get(slot as usize)
            .cloned()
            .flatten()
            .ok_or("Nothing there")?;
        let def = self
            .data
            .items
            .get(&item.info)
            .ok_or("Unknown item")?
            .clone();
        if !item_type::slots(def.item_type).is_empty() {
            let target = default_slot(def.item_type, &p.bag.equipment).ok_or("Cannot equip")?;
            return self.item_move_inner(id, Grid::Inventory, slot, Grid::Equipment, target as u8);
        }
        if p.mounted {
            return Err("You cannot use items while riding".into());
        }
        match def.item_type {
            item_type::CONSUMABLE => {
                can_use(&def, p.class, p.gender, p.level)?;
                if o.dead {
                    return Err("You cannot use that while dead".into());
                }
                // Zircon: `if (SEnvir.Now < UseItemTime) return;` (silent).
                if self.now < p.use_item_time {
                    return Ok(Vec::new());
                }
                self.use_consumable(id, slot, &def)
            }
            item_type::COMPANION_BAG => {
                self.companion_bag_equip(id, slot);
                Ok(Vec::new())
            }
            item_type::COMPANION_FOOD => {
                can_use(&def, p.class, p.gender, p.level)?;
                let Some(level) = p
                    .active_companion
                    .and_then(|active| p.companions.iter().find(|c| c.index == active))
                    .map(|c| c.level)
                else {
                    return Err("No companion is out".into());
                };
                super::companion::companion_food_requirement(&def, level)?;
                if self.now < p.use_item_time {
                    return Ok(Vec::new());
                }
                if !self.companion_feed(id, def.stat(super::companion::COMPANION_HUNGER_STAT)) {
                    return Err("Your companion is not hungry".into());
                }
                let p = self
                    .objects
                    .get_mut(&id)
                    .and_then(|o| o.player_mut())
                    .unwrap();
                Ok(p.bag.take(Grid::Inventory, slot, 1).into_iter().collect())
            }
            item_type::BOOK => self.learn_book(id, slot, &def),
            _ => Err(format!("{} cannot be used", def.name)),
        }
    }

    /// Zircon `ItemUse` for `ItemType.Consumable`: potions heal instantly
    /// (boosted by Potion Mastery), town/random teleport scrolls move the
    /// player, and every use starts a `Durability` ms cooldown.
    pub(super) fn use_consumable(
        &mut self,
        id: ObjectId,
        slot: u8,
        def: &crate::data::ItemDef,
    ) -> Result<Changed, String> {
        let mut changes = Vec::new();
        match def.shape {
            0 => {
                let mut health = def.stat(stat::HEALTH);
                let mut mana = def.stat(stat::MANA);
                // Potion Mastery: `health += health * GetPower() / 100`, rolled
                // separately per stat; levels while something was missing.
                let mastery = self.objects[&id]
                    .player()
                    .and_then(|p| {
                        p.magics
                            .iter()
                            .find(|m| m.magic == magic_type::POTION_MASTERY)
                    })
                    .and_then(|m| self.data.magics.get(&m.magic).map(|d| m.power_range(d)));
                if let Some((pmin, pmax)) = mastery {
                    let mut roll = || {
                        if pmin >= pmax {
                            pmin
                        } else {
                            self.rng.random_range(pmin..=pmax)
                        }
                    };
                    let (hb, mb) = (roll(), roll());
                    health += health * hb / 100;
                    mana += mana * mb / 100;
                    // Advanced Potion Mastery stacks another percent on top.
                    let advanced = self.objects[&id]
                        .player()
                        .and_then(|p| {
                            p.magics
                                .iter()
                                .find(|m| m.magic == magic_type::ADVANCED_POTION_MASTERY)
                        })
                        .map(|m| m.power_range(&self.data.magics[&m.magic]).0)
                        .unwrap_or(0);
                    health += health * advanced / 100;
                    mana += mana * advanced / 100;
                    // Vitality: bigger potions below 30 % HP.
                    let (vitality, low) = {
                        let o = &self.objects[&id];
                        let p = o.player().unwrap();
                        (p.vitality, o.hp * 100 / o.max_hp.max(1) < 30)
                    };
                    if low && vitality > 0 {
                        health += health * vitality / 100;
                    }
                    let missing = {
                        let o = &self.objects[&id];
                        let p = o.player().unwrap();
                        o.hp < o.max_hp || p.mp < p.max_mp
                    };
                    if missing {
                        self.level_magic(id, magic_type::POTION_MASTERY);
                    }
                }
                let o = self.objects.get_mut(&id).unwrap();
                o.hp = (o.hp + health).min(o.max_hp);
                let p = o.player_mut().unwrap();
                p.mp = (p.mp + mana).min(p.max_mp);
                let exp = def.stat(stat::EXPERIENCE);
                if exp > 0 {
                    self.gain_experience(id, exp as u64);
                }
            }
            2 => {
                let o = &self.objects[&id];
                let fallback = o.player().unwrap().bind_region;
                let town = self.nearest_town_region(o.map, o.location, o.player().unwrap().class);
                let region = town.unwrap_or(fallback);
                if region == 0 {
                    return Err("You have no town to return to".into());
                }
                let from = o.location;
                let to = self.go_to_region(id, region).map_err(|e| e.to_string())?;
                self.events.push((
                    id,
                    ServerMessage::ObjectEffect {
                        id,
                        effect: effect::TELEPORT_OUT,
                        location: from,
                    },
                ));
                self.events.push((
                    id,
                    ServerMessage::ObjectEffect {
                        id,
                        effect: effect::TELEPORT_IN,
                        location: to,
                    },
                ));
            }
            3 => {
                let map = self.objects[&id].map;
                let to = self.random_walkable(map).ok_or("Nowhere to teleport to")?;
                self.teleport_with_effects(id, to);
            }
            4 => {
                let weapon = self.objects[&id]
                    .player()
                    .unwrap()
                    .bag
                    .equipment
                    .get(slot::WEAPON)
                    .and_then(Option::as_ref)
                    .ok_or("Equip a weapon first")?;
                let luck: i32 = weapon
                    .added
                    .iter()
                    .filter(|s| {
                        s.stat == stat::LUCK && s.source == crate::items::StatSource::Enhancement
                    })
                    .map(|s| s.amount)
                    .sum();
                if luck >= 7 {
                    return Err("Weapon Luck is already at maximum".into());
                }
                // Zircon defaults: MaxCurse -10, CurseRate 20, LuckRate 10.
                let change = if luck > -10 && self.rng.random_range(0..20) == 0 {
                    -1
                } else if luck <= 0 || self.rng.random_range(0..luck * 10) == 0 {
                    1
                } else {
                    0
                };
                let weapon = self
                    .objects
                    .get_mut(&id)
                    .unwrap()
                    .player_mut()
                    .unwrap()
                    .bag
                    .equipment[slot::WEAPON]
                    .as_mut()
                    .unwrap();
                if change != 0 {
                    weapon.add_stat(stat::LUCK, change, crate::items::StatSource::Enhancement);
                }
                changes.push((Grid::Equipment, slot::WEAPON as u8, Some(weapon.instance())));
            }
            16 => self.companion_skill_reroll(id, def.required_amount)?,
            _ => return Err(format!("{} cannot be used", def.name)),
        }
        let o = self.objects.get_mut(&id).unwrap();
        let p = o.player_mut().unwrap();
        p.use_item_time = self.now + def.durability.max(0) as u64;
        changes.extend(p.bag.take(Grid::Inventory, slot, 1));
        Ok(changes)
    }

    pub fn item_drop(&mut self, id: ObjectId, slot: u8, count: u32) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if o.dead {
            return;
        }
        let (map, loc, account) = (o.map, o.location, p.account);
        let Some(item) = p.bag.inventory.get(slot as usize).cloned().flatten() else {
            return;
        };
        let Some(def) = self.data.items.get(&item.info) else {
            return;
        };
        if item.locked || !def.can_drop {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: "That cannot be dropped".into(),
                },
            );
            return;
        }
        let count = count.max(1).min(item.count);
        let change = self
            .objects
            .get_mut(&id)
            .unwrap()
            .player_mut()
            .unwrap()
            .bag
            .take(Grid::Inventory, slot, count);
        let dropped = UserItem { count, ..item };
        let owner = dropped.bound.then_some(account);
        self.spawn_ground_item(map, loc, dropped, owner, 1);
        self.send_changes(id, change.into_iter().collect());
    }

    pub fn pick_up(&mut self, id: ObjectId) {
        self.pick_up_target(id, None);
    }

    pub fn pick_up_item(&mut self, id: ObjectId, item: ObjectId) {
        self.pick_up_target(id, Some(item));
    }

    fn pick_up_target(&mut self, id: ObjectId, requested: Option<ObjectId>) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if o.dead {
            return;
        }
        let (map, loc, account, max_bag, pickup_radius) = (
            o.map,
            o.location,
            p.account,
            p.max_bag,
            o.stats.pickup_radius,
        );
        let mut candidates: Vec<ObjectId> = Vec::new();
        for d in 0..=pickup_radius {
            for dy in -d..=d {
                for dx in -d..=d {
                    if dx.abs().max(dy.abs()) != d {
                        continue;
                    }
                    let cell = Point::new(loc.x + dx, loc.y + dy);
                    if let Some(m) = self.maps.get(&map) {
                        candidates.extend(m.objects_at(cell).iter().copied());
                    }
                }
            }
        }
        let mut bag_full = false;
        for cid in candidates {
            if requested.is_some_and(|target| target != cid) {
                continue;
            }
            let (item, allowed) = {
                let Some(obj) = self.objects.get(&cid) else {
                    continue;
                };
                let Kind::Item(i) = &obj.kind else { continue };
                let allowed = i.owner.is_none_or(|a| a == account)
                    || (!i.item.bound && self.now >= i.spawn_time + DROP_SHARE_AFTER);
                (i.item.clone(), allowed)
            };
            if !allowed {
                continue;
            }
            let Some(def) = self.data.items.get(&item.info).cloned() else {
                continue;
            };
            if item.info == self.data.gold_item {
                // The guild's tax comes off the top (Zircon CalculateGuildTax).
                let gold = self.guild_tax_gold(id, item.count as u64);
                let o = self.objects.get_mut(&id).unwrap();
                let p = o.player_mut().unwrap();
                p.bag.gold += gold;
                self.remove_object(cid);
                self.send_gold(id);
                self.send_to(
                    id,
                    ServerMessage::Chat {
                        text: format!("You picked up {} gold.", item.count),
                    },
                );
                return;
            }
            let p = self.objects[&id].player().unwrap();
            if !p.bag.can_gain_item(&self.data, &item, max_bag) {
                bag_full = true;
                continue;
            }
            let o = self.objects.get_mut(&id).unwrap();
            let p = o.player_mut().unwrap();
            let mut next = p.next_item_id;
            let changes = p.bag.gain_item(&self.data, item.clone(), &mut next);
            p.next_item_id = next;
            self.remove_object(cid);
            self.send_changes(id, changes);
            let text = if item.count > 1 {
                format!("You picked up {} ({}).", def.name, item.count)
            } else {
                format!("You picked up {}.", def.name)
            };
            self.send_to(id, ServerMessage::Chat { text });
            return;
        }
        if bag_full {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: "You cannot carry any more.".into(),
                },
            );
        }
    }

    /// Zircon book use: learn the magic the book's `Shape` points at.
    pub(super) fn learn_book(
        &mut self,
        id: ObjectId,
        slot: u8,
        def: &crate::data::ItemDef,
    ) -> Result<Changed, String> {
        let magic = self
            .data
            .magic_by_index(def.shape)
            .cloned()
            .ok_or("This book teaches nothing")?;
        if magic.school == 0 {
            return Err("This skill is disabled".into());
        }
        let o = self.objects.get_mut(&id).ok_or("no player")?;
        let p = o.player_mut().ok_or("no player")?;
        if magic.class & (1 << p.class.mir_class()) == 0 {
            return Err("Your class cannot learn this".into());
        }
        if let Some(known) = p.magics.iter().find(|m| m.magic == magic.magic) {
            if known.level < 3 {
                return Err(format!("You already know {}", magic.name));
            }
            return Err("Level 4 skills are not supported yet".into());
        }
        let book = p
            .bag
            .inventory
            .get(slot as usize)
            .cloned()
            .flatten()
            .ok_or("Nothing there")?;
        let change = p.bag.take(Grid::Inventory, slot, 1);
        // Success chance = the book's current durability (100 when new).
        if self.rng.random_range(0..100) >= book.durability.max(0) {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: format!("You failed to learn {}.", magic.name),
                },
            );
            return Ok(change.into_iter().collect());
        }
        let um = UserMagic {
            magic: magic.magic,
            level: 0,
            experience: 0,
            key: 0,
            cooldown_until: 0,
        };
        let summary = um.summary();
        self.objects
            .get_mut(&id)
            .unwrap()
            .player_mut()
            .unwrap()
            .magics
            .push(um);
        self.send_to(id, ServerMessage::NewMagic(summary));
        self.send_to(
            id,
            ServerMessage::Chat {
                text: format!("You learned {}.", magic.name),
            },
        );
        Ok(change.into_iter().collect())
    }
}
