//! Companions (Zircon `Companion`, `UserCompanion`, `CompanionInfo`): a pet
//! that follows its owner, picks up the owner's drops into its own bag,
//! gains a level over time and gets hungry. Adopted at a CompanionManage
//! page for the companion's price, stored and retrieved there.

use serde::{Deserialize, Serialize};

use super::*;
use crate::items::UserItem;
use mir_proto::{ChatKind, CompanionOffer, CompanionSummary};

/// Zircon `Globals.CompanionInventorySize`, `Stat.CompanionHunger` = 86,
/// companion buff tick 1 minute, `Config.CompanionRate` 0.
pub const COMPANION_BAG_SIZE: usize = 30;
pub const COMPANION_HUNGER_STAT: i32 = 86;
pub const COMPANION_TICK: u64 = 60_000;
/// How far a companion looks for the owner's drops.
const PICKUP_RANGE: i32 = 8;
const SKILL_LEVELS: [i32; 7] = [3, 5, 7, 10, 11, 13, 15];

fn skill_bit(level: i32) -> Option<u8> {
    SKILL_LEVELS
        .iter()
        .position(|candidate| *candidate == level)
        .map(|index| 1 << index)
}

/// Zircon `Companion.CanUseItem`: required types 11/12 use pet level.
pub(super) fn companion_food_requirement(
    def: &crate::data::ItemDef,
    level: i32,
) -> Result<(), String> {
    match def.required_type {
        11 if level < def.required_amount => {
            Err(format!("Companion needs level {}.", def.required_amount))
        }
        12 if level > def.required_amount => Err(format!(
            "Companion needs level {} or below.",
            def.required_amount
        )),
        _ => Ok(()),
    }
}

/// Zircon `UserCompanion`, persisted per character.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCompanion {
    pub index: u32,
    /// `CompanionInfo` index.
    pub info: i32,
    pub name: String,
    pub level: i32,
    pub experience: i32,
    pub hunger: i32,
    pub items: Vec<UserItem>,
    /// Zircon `CompanionSlot.Food`; one equipped stack feeds automatically.
    #[serde(default)]
    pub food: Option<UserItem>,
    #[serde(default)]
    pub bag: Option<UserItem>,
    #[serde(default)]
    pub bonuses: Vec<(i32, i32, i32)>,
}

impl World {
    fn companion_line(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }

    fn level_def(&self, level: i32) -> Option<&crate::data::CompanionLevelDef> {
        self.data
            .companion_levels
            .iter()
            .filter(|l| l.level <= level)
            .max_by_key(|l| l.level)
    }

    /// Bag slots and weight allowed at a level (Zircon `CompanionLevelInfo`).
    fn bag_limits(&self, companion: &StoredCompanion) -> (usize, i32) {
        let (slots, weight) = self
            .level_def(companion.level)
            .map(|l| {
                (
                    l.inventory_space.clamp(0, COMPANION_BAG_SIZE as i32) as usize,
                    l.inventory_weight,
                )
            })
            .unwrap_or((5, 100));
        let bonus = |stat| {
            companion.bag.as_ref().map_or(0, |bag| {
                self.data
                    .items
                    .get(&bag.info)
                    .map_or(0, |def| def.stat(stat))
                    .saturating_add(bag.bonus(stat))
            })
        };
        (
            (slots as i32)
                .saturating_add(bonus(82))
                .clamp(0, COMPANION_BAG_SIZE as i32) as usize,
            weight.saturating_add(bonus(83)),
        )
    }

    fn roll_companion_skill(&mut self, level: i32) -> Option<(i32, i32, i32)> {
        let choices: Vec<_> = self
            .data
            .companion_skills
            .iter()
            .filter(|skill| skill.level == level && skill.weight > 0 && skill.max_amount > 0)
            .collect();
        let total: i32 = choices.iter().map(|skill| skill.weight).sum();
        if total <= 0 {
            return None;
        }
        let mut roll = self.rng.random_range(0..total);
        for skill in choices {
            if roll < skill.weight {
                return Some((
                    level,
                    skill.stat,
                    self.rng.random_range(1..=skill.max_amount),
                ));
            }
            roll -= skill.weight;
        }
        None
    }

    /// Grant any reached milestones once, including older saved companions.
    pub(super) fn ensure_companion_skills(&mut self, id: ObjectId) -> bool {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return false;
        };
        let Some(active) = p.active_companion else {
            return false;
        };
        let Some(companion) = p.companions.iter().find(|c| c.index == active) else {
            return false;
        };
        let missing: Vec<i32> = SKILL_LEVELS
            .into_iter()
            .filter(|level| {
                companion.level >= *level
                    && !companion
                        .bonuses
                        .iter()
                        .any(|(earned, _, _)| earned == level)
            })
            .collect();
        let rolled: Vec<_> = missing
            .into_iter()
            .filter_map(|level| self.roll_companion_skill(level))
            .collect();
        if rolled.is_empty() {
            return false;
        }
        if let Some(companion) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.companions.iter_mut().find(|c| c.index == active))
        {
            companion.bonuses.extend(rolled);
        }
        true
    }

    pub(super) fn send_companions(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let list: Vec<CompanionSummary> = p
            .companions
            .iter()
            .map(|c| {
                let (bag_size, max_weight) = self.bag_limits(c);
                CompanionSummary {
                    index: c.index,
                    kind: self
                        .data
                        .companions
                        .iter()
                        .find(|d| d.index == c.info)
                        .and_then(|d| self.data.monsters.get(&d.monster))
                        .map(|m| m.name.clone())
                        .unwrap_or_default(),
                    name: c.name.clone(),
                    level: c.level,
                    experience: c.experience,
                    max_experience: self
                        .level_def(c.level)
                        .map(|l| l.max_experience)
                        .unwrap_or(0),
                    hunger: c.hunger,
                    active: p.active_companion == Some(c.index),
                    items: c.items.iter().map(|i| i.instance()).collect(),
                    bag_size: bag_size as u32,
                    bag_weight: c
                        .items
                        .iter()
                        .map(|i| crate::items::item_weight(&self.data, i))
                        .sum(),
                    max_weight,
                    food: c.food.as_ref().map(|item| item.instance()),
                    bag: c.bag.as_ref().map(|item| item.instance()),
                    bonuses: c.bonuses.clone(),
                    reroll_enabled: p.companion_reroll_enabled,
                }
            })
            .collect();
        self.send_to(id, ServerMessage::Companions(list));
    }

    /// C# `@EnableLevelN`: arm or lock an Elixir of Regret reroll for this login.
    pub fn companion_skill_toggle(&mut self, id: ObjectId, level: i32) {
        let Some(bit) = skill_bit(level) else {
            self.companion_line(id, "Choose level 3, 5, 7, 10, 11, 13 or 15.".into());
            return;
        };
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        p.companion_reroll_enabled ^= bit;
        let armed = p.companion_reroll_enabled & bit != 0;
        self.send_companions(id);
        self.companion_line(
            id,
            format!(
                "Companion skill level {level} reroll {}.",
                if armed { "armed" } else { "locked" }
            ),
        );
    }

    /// Reroll only a learned milestone. Failed checks leave the elixir untouched.
    pub(super) fn companion_skill_reroll(
        &mut self,
        id: ObjectId,
        level: i32,
    ) -> Result<(), String> {
        let bit = skill_bit(level).ok_or("That elixir has no companion skill level")?;
        let p = self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .ok_or("No player")?;
        let active = p
            .active_companion
            .filter(|_| p.companion.is_some())
            .ok_or("No companion is out")?;
        let old = p
            .companions
            .iter()
            .find(|c| c.index == active && c.level >= level)
            .and_then(|c| c.bonuses.iter().find(|(earned, _, _)| *earned == level))
            .copied()
            .ok_or("Your companion has not learned that level's skill")?;
        if p.companion_reroll_enabled & bit == 0 {
            return Err(format!(
                "Use Reroll in the Companion window, or type @EnableLevel{level} before using this elixir."
            ));
        }
        let alternative = self
            .data
            .companion_skills
            .iter()
            .filter(|skill| skill.level == level && skill.weight > 0 && skill.max_amount > 0)
            .find_map(|skill| {
                let amount = if skill.stat == old.1 && old.2 == 1 {
                    2
                } else {
                    1
                };
                (amount <= skill.max_amount && (skill.stat, amount) != (old.1, old.2))
                    .then_some((level, skill.stat, amount))
            })
            .ok_or("No different skill roll is available for this level")?;
        let mut next = alternative;
        for _ in 0..64 {
            let Some(rolled) = self.roll_companion_skill(level) else {
                break;
            };
            if rolled != old {
                next = rolled;
                break;
            }
        }
        let companion = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap()
            .companions
            .iter_mut()
            .find(|c| c.index == active)
            .unwrap();
        *companion
            .bonuses
            .iter_mut()
            .find(|(earned, _, _)| *earned == level)
            .unwrap() = next;
        self.send_companions(id);
        self.companion_line(id, format!("Companion level {level} skill rerolled."));
        Ok(())
    }

    /// The companion shop for a CompanionManage page.
    pub(super) fn send_companion_shop(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let offers: Vec<CompanionOffer> = self
            .data
            .companions
            .iter()
            .map(|d| CompanionOffer {
                index: d.index,
                name: self
                    .data
                    .monsters
                    .get(&d.monster)
                    .map(|m| m.name.clone())
                    .unwrap_or_default(),
                description: d.description.clone(),
                price: d.price,
                currency: self
                    .data
                    .currencies
                    .iter()
                    .find(|c| c.index == d.currency)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "Gold".into()),
                unlocked: d.available || p.companion_unlocks.contains(&d.index),
                unlock_item: d.unlock_item,
            })
            .collect();
        self.send_to(id, ServerMessage::CompanionShop(offers));
    }

    /// Spawn the active companion beside its owner (on entry / retrieve).
    pub(super) fn companion_spawn(&mut self, id: ObjectId) {
        self.ensure_companion_skills(id);
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if p.companion.is_some() {
            return;
        }
        let Some(active) = p.active_companion else {
            return;
        };
        let Some(c) = p.companions.iter().find(|c| c.index == active) else {
            return;
        };
        let Some(def) = self.data.companions.iter().find(|d| d.index == c.info) else {
            return;
        };
        if !self.data.monsters.contains_key(&def.monster) {
            return;
        }
        let (map, loc, monster, name) = (o.map, o.location, def.monster, c.name.clone());
        let cid = self.create_monster(monster, map, loc, None, Some(id), 0);
        if let Some(m) = self.objects.get_mut(&cid).and_then(|o| o.monster_mut()) {
            m.companion = Some(active);
            m.tame_until = u64::MAX;
        }
        if let Some(o) = self.objects.get_mut(&cid) {
            if let Appearance::Monster { name: n, .. } = &mut o.appearance {
                *n = name;
            }
        }
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.companion = Some(cid);
            p.companion_tick = self.now + COMPANION_TICK;
        }
        self.refresh_stats(id, false);
        self.send_player_stats(id);
    }

    pub(super) fn companion_despawn(&mut self, id: ObjectId) {
        let cid = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.companion.take());
        if let Some(cid) = cid {
            self.remove_object(cid);
        }
    }

    /// Unlock a locked companion look with its unlock item (Zircon
    /// `CompanionUnlock`).
    pub fn companion_unlock(&mut self, id: ObjectId, index: i32) {
        let Some(def) = self
            .data
            .companions
            .iter()
            .find(|d| d.index == index)
            .cloned()
        else {
            return;
        };
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if def.available || p.companion_unlocks.contains(&index) {
            self.companion_line(id, "That companion is already available to you.".into());
            return;
        }
        let Some(slot) = p
            .bag
            .inventory
            .iter()
            .position(|s| s.as_ref().is_some_and(|i| i.info == def.unlock_item))
        else {
            self.companion_line(id, "You do not have the item that unlocks it.".into());
            return;
        };
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let change = p.bag.take(Grid::Inventory, slot as u8, 1);
        p.companion_unlocks.push(index);
        if let Some(ch) = change {
            self.send_changes(id, vec![ch]);
        }
        self.send_companion_shop(id);
    }

    /// Adopt a companion for its price (Zircon `CompanionAdopt`).
    pub fn companion_adopt(&mut self, id: ObjectId, index: i32, name: String) {
        if self.npc_dialog_type(id) != Some(5) {
            return;
        }
        let Some(def) = self
            .data
            .companions
            .iter()
            .find(|d| d.index == index)
            .cloned()
        else {
            return;
        };
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if !def.available && !p.companion_unlocks.contains(&index) {
            self.companion_line(id, "That companion is not available to you.".into());
            return;
        }
        let valid_name = (2..=15).contains(&name.chars().count())
            && name.chars().all(|c| c.is_ascii_alphanumeric());
        if !valid_name {
            self.companion_line(id, "Companion names are 2 to 15 letters or digits.".into());
            return;
        }
        let gold =
            def.currency == 0 || !self.data.currencies.iter().any(|c| c.index == def.currency);
        let price = def.price.max(0) as u64;
        if gold {
            if p.bag.gold < price {
                self.companion_line(id, format!("Adopting costs {price} gold."));
                return;
            }
        } else if p.currencies.get(&def.currency).copied().unwrap_or(0) < price as i64 {
            self.companion_line(id, "You cannot afford that companion.".into());
            return;
        }
        let max_hunger = self.level_def(1).map(|l| l.max_hunger).unwrap_or(100);
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        if gold {
            p.bag.gold -= price;
        } else {
            *p.currencies.entry(def.currency).or_insert(0) -= price as i64;
        }
        let cindex = p.companions.iter().map(|c| c.index).max().unwrap_or(0) + 1;
        p.companions.push(StoredCompanion {
            index: cindex,
            info: index,
            name: name.clone(),
            level: 1,
            experience: 0,
            hunger: max_hunger,
            items: Vec::new(),
            food: None,
            bag: None,
            bonuses: Vec::new(),
        });
        self.send_gold(id);
        self.send_currencies(id);
        self.send_companions(id);
        self.companion_line(id, format!("{name} is yours."));
    }

    /// Bring a stored companion out (Zircon `CompanionRetrieve`).
    pub fn companion_retrieve(&mut self, id: ObjectId, index: u32) {
        if self.npc_dialog_type(id) != Some(5) {
            return;
        }
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if !p.companions.iter().any(|c| c.index == index) {
            return;
        }
        self.companion_despawn(id);
        self.objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap()
            .active_companion = Some(index);
        self.companion_spawn(id);
        self.send_companions(id);
    }

    /// Put the active companion away (Zircon `CompanionStore`).
    pub fn companion_store(&mut self, id: ObjectId) {
        if self.npc_dialog_type(id) != Some(5) {
            return;
        }
        self.companion_despawn(id);
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.active_companion = None;
        }
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.send_companions(id);
    }

    /// Let a companion go for good (its bag must be empty).
    pub fn companion_release(&mut self, id: ObjectId, index: u32) {
        if self.npc_dialog_type(id) != Some(5) {
            return;
        }
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some(c) = p.companions.iter().find(|c| c.index == index) else {
            return;
        };
        if !c.items.is_empty() || c.food.is_some() || c.bag.is_some() {
            self.companion_line(
                id,
                "Remove the companion's items and equipment first.".into(),
            );
            return;
        }
        if p.active_companion == Some(index) {
            self.companion_despawn(id);
        }
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        if p.active_companion == Some(index) {
            p.active_companion = None;
        }
        p.companions.retain(|c| c.index != index);
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.send_companions(id);
    }

    /// Move one item from a companion's bag into the player's bag.
    pub fn companion_bag_take(&mut self, id: ObjectId, index: u32, slot: u8) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some(item) = p
            .companions
            .iter()
            .find(|c| c.index == index)
            .and_then(|c| c.items.get(slot as usize))
            .cloned()
        else {
            return;
        };
        if !p.bag.can_gain_item(&self.data, &item, p.max_bag) {
            self.companion_line(id, "You cannot carry that.".into());
            return;
        }
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let mut next = p.next_item_id;
        let changes = p.bag.gain_item(&self.data, item, &mut next);
        p.next_item_id = next;
        if let Some(c) = p.companions.iter_mut().find(|c| c.index == index) {
            c.items.remove(slot as usize);
        }
        self.send_changes(id, changes);
        self.send_companions(id);
    }

    pub fn companion_bag_equip(&mut self, id: ObjectId, slot: u8) {
        self.companion_bag_change(id, Some(slot));
    }

    pub fn companion_bag_unequip(&mut self, id: ObjectId) {
        self.companion_bag_change(id, None);
    }

    fn companion_bag_change(&mut self, id: ObjectId, slot: Option<u8>) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some(active) = p.active_companion.filter(|_| p.companion.is_some()) else {
            self.companion_line(id, "No companion is out.".into());
            return;
        };
        let Some(companion) = p.companions.iter().find(|c| c.index == active) else {
            return;
        };
        let new_bag = if let Some(slot) = slot {
            let Some(item) = p.bag.inventory.get(slot as usize).and_then(Option::as_ref) else {
                return;
            };
            let Some(def) = self.data.items.get(&item.info) else {
                return;
            };
            if def.item_type != mir_proto::item_type::COMPANION_BAG {
                return;
            }
            if let Err(message) = companion_food_requirement(def, companion.level) {
                self.companion_line(id, message);
                return;
            }
            Some(item.clone())
        } else {
            let Some(bag) = companion.bag.as_ref() else {
                return;
            };
            if !p.bag.can_gain_item(&self.data, bag, p.max_bag) {
                self.companion_line(id, "You cannot carry that.".into());
                return;
            }
            None
        };
        let mut next_companion = companion.clone();
        next_companion.bag = new_bag.clone();
        let (slots, max_weight) = self.bag_limits(&next_companion);
        let weight: i32 = companion
            .items
            .iter()
            .map(|item| crate::items::item_weight(&self.data, item))
            .sum();
        if companion.items.len() > slots || weight > max_weight {
            self.companion_line(
                id,
                "Empty the companion's inventory before reducing its capacity.".into(),
            );
            return;
        }
        let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
        let companion = p.companions.iter_mut().find(|c| c.index == active).unwrap();
        let old = std::mem::replace(&mut companion.bag, new_bag);
        let changes = if let Some(slot) = slot {
            p.bag.inventory[slot as usize] = old;
            vec![(
                Grid::Inventory,
                slot,
                p.bag.inventory[slot as usize]
                    .as_ref()
                    .map(UserItem::instance),
            )]
        } else {
            p.bag
                .gain_item(&self.data, old.unwrap(), &mut p.next_item_id)
        };
        self.send_changes(id, changes);
        self.send_companions(id);
    }

    /// Move an inventory food stack into the companion's food slot.
    pub fn companion_food_equip(&mut self, id: ObjectId, slot: u8) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some(active) = p.active_companion.filter(|_| p.companion.is_some()) else {
            self.companion_line(id, "No companion is out.".into());
            return;
        };
        let Some(item) = p
            .bag
            .inventory
            .get(slot as usize)
            .and_then(|item| item.as_ref())
            .cloned()
        else {
            return;
        };
        let Some(def) = self.data.items.get(&item.info) else {
            return;
        };
        if def.item_type != mir_proto::item_type::COMPANION_FOOD {
            self.companion_line(id, "Only Companion Food goes in that slot.".into());
            return;
        }
        if let Some(level) = p
            .companions
            .iter()
            .find(|c| c.index == active)
            .map(|c| c.level)
        {
            if let Err(message) = companion_food_requirement(def, level) {
                self.companion_line(id, message);
                return;
            }
        }
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let companion = p.companions.iter_mut().find(|c| c.index == active).unwrap();
        let old = companion.food.replace(item);
        p.bag.inventory[slot as usize] = old;
        let updated = p.bag.inventory[slot as usize]
            .as_ref()
            .map(UserItem::instance);
        self.send_changes(id, vec![(Grid::Inventory, slot, updated)]);
        if !self.companion_auto_feed(id) {
            self.send_companions(id);
            self.companion_line(
                id,
                "Food equipped; your companion eats automatically when hunger reaches zero.".into(),
            );
        }
    }

    pub fn companion_food_take(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some(active) = p.active_companion else {
            return;
        };
        let Some(food) = p
            .companions
            .iter()
            .find(|c| c.index == active)
            .and_then(|c| c.food.as_ref())
        else {
            return;
        };
        if !p.bag.can_gain_item(&self.data, food, p.max_bag) {
            self.companion_line(id, "You cannot carry that.".into());
            return;
        }
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let food = p
            .companions
            .iter_mut()
            .find(|c| c.index == active)
            .unwrap()
            .food
            .take()
            .unwrap();
        let mut next = p.next_item_id;
        let changes = p.bag.gain_item(&self.data, food, &mut next);
        p.next_item_id = next;
        self.send_changes(id, changes);
        self.send_companions(id);
    }

    /// Zircon `Companion.AutoFeed`: at zero hunger, consume one equipped food.
    fn companion_auto_feed(&mut self, id: ObjectId) -> bool {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return false;
        };
        let Some(active) = p.active_companion else {
            return false;
        };
        let Some(c) = p.companions.iter().find(|c| c.index == active) else {
            return false;
        };
        if c.hunger > 0 {
            return false;
        }
        let Some(food) = c.food.as_ref() else {
            return false;
        };
        if food.count == 0 {
            return false;
        }
        let Some(def) = self.data.items.get(&food.info) else {
            return false;
        };
        if companion_food_requirement(def, c.level).is_err() {
            return false;
        }
        let hunger = def.stat(COMPANION_HUNGER_STAT);
        if hunger <= 0 {
            return false;
        }
        let max = self.level_def(c.level).map(|l| l.max_hunger).unwrap_or(100);
        let name = def.name.clone();
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let c = p.companions.iter_mut().find(|c| c.index == active).unwrap();
        c.hunger = hunger.min(max);
        let food = c.food.as_mut().unwrap();
        food.count -= 1;
        if food.count == 0 {
            c.food = None;
        }
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.send_companions(id);
        self.companion_line(id, format!("Companion ate {name}."));
        true
    }

    /// Per-minute companion upkeep (Zircon companion buff tick): hunger
    /// drops outside safe zones, experience grows, levels advance.
    pub(super) fn process_companions(&mut self) {
        let now = self.now;
        let ids: Vec<ObjectId> = self
            .players()
            .filter(|o| {
                o.player()
                    .is_some_and(|p| p.companion.is_some() && now >= p.companion_tick)
            })
            .map(|o| o.id)
            .collect();
        for id in ids {
            let (in_safe_zone, active, highest) = {
                let o = &self.objects[&id];
                let p = o.player().unwrap();
                (
                    o.in_safe_zone,
                    p.active_companion,
                    p.companions.iter().map(|c| c.level).max().unwrap_or(1),
                )
            };
            let Some(active) = active else { continue };
            let level = self.objects[&id]
                .player()
                .unwrap()
                .companions
                .iter()
                .find(|c| c.index == active)
                .map(|c| c.level)
                .unwrap_or(1);
            let max_exp = self.level_def(level).map(|l| l.max_experience).unwrap_or(0);
            let (leveled, at_zero, became_starved) = {
                let p = self
                    .objects
                    .get_mut(&id)
                    .and_then(|o| o.player_mut())
                    .unwrap();
                p.companion_tick = now + COMPANION_TICK;
                let Some(c) = p.companions.iter_mut().find(|c| c.index == active) else {
                    continue;
                };
                let was_fed = c.hunger > 0;
                if was_fed && (!in_safe_zone || c.level < 15) {
                    c.hunger = (c.hunger - 1).max(0);
                }
                let mut leveled = false;
                if was_fed && max_exp > 0 {
                    let gain = if highest <= c.level { 1 } else { highest };
                    c.experience += gain;
                    if c.experience >= max_exp {
                        c.experience = 0;
                        c.level += 1;
                        leveled = true;
                    }
                }
                (leveled, c.hunger == 0, was_fed && c.hunger == 0)
            };
            if leveled {
                self.ensure_companion_skills(id);
            }
            let ate = at_zero && self.companion_auto_feed(id);
            if (leveled || became_starved) && !ate {
                self.refresh_stats(id, false);
                self.send_player_stats(id);
            }
            if !ate {
                self.send_companions(id);
            }
        }
    }

    /// Feed the active companion (Zircon: consumables with `CompanionHunger`).
    pub(super) fn companion_feed(&mut self, id: ObjectId, hunger: i32) -> bool {
        if hunger <= 0 {
            return false;
        }
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return false;
        };
        let Some(active) = p.active_companion else {
            return false;
        };
        let Some(level) = p
            .companions
            .iter()
            .find(|c| c.index == active)
            .map(|c| c.level)
        else {
            return false;
        };
        let max = self.level_def(level).map(|l| l.max_hunger).unwrap_or(100);
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let c = p.companions.iter_mut().find(|c| c.index == active).unwrap();
        if c.hunger >= max {
            return false;
        }
        let was_starved = c.hunger == 0;
        c.hunger = (c.hunger + hunger).min(max);
        if was_starved && c.hunger > 0 {
            self.refresh_stats(id, false);
            self.send_player_stats(id);
        }
        self.send_companions(id);
        self.companion_line(id, "Your companion is fed.".into());
        true
    }

    fn companion_can_gain(&self, companion: &StoredCompanion, item: &UserItem) -> bool {
        let Some(def) = self.data.items.get(&item.info) else {
            return false;
        };
        let (slots, max_weight) = self.bag_limits(companion);
        let weight: i64 = companion
            .items
            .iter()
            .map(|i| i64::from(crate::items::item_weight(&self.data, i)))
            .sum();
        if weight + i64::from(crate::items::item_weight(&self.data, item)) > i64::from(max_weight) {
            return false;
        }
        let stack = def.stack_size.max(1) as u32;
        let mut remaining = item.count;
        for existing in &companion.items {
            if existing.stacks_with(item) {
                remaining = remaining.saturating_sub(stack.saturating_sub(existing.count));
            }
        }
        remaining.div_ceil(stack) as usize <= slots.saturating_sub(companion.items.len())
    }

    /// The caller checks capacity first, preserving stack metadata like the player's bag.
    fn companion_gain_item(&mut self, owner: ObjectId, index: u32, item: UserItem) {
        let stack = self.data.items[&item.info].stack_size.max(1) as u32;
        let p = self.objects.get_mut(&owner).unwrap().player_mut().unwrap();
        let companion = p.companions.iter_mut().find(|c| c.index == index).unwrap();
        let mut remaining = item.count;
        for existing in &mut companion.items {
            if existing.stacks_with(&item) {
                let add = stack.saturating_sub(existing.count).min(remaining);
                existing.count += add;
                remaining -= add;
            }
        }
        while remaining > 0 {
            let count = remaining.min(stack);
            p.next_item_id += 1;
            companion.items.push(UserItem {
                id: p.next_item_id,
                count,
                ..item.clone()
            });
            remaining -= count;
        }
        self.send_companions(owner);
    }

    /// `Stat.CompanionCollection`: collect a fresh monster drop at once.
    pub(crate) fn companion_auto_collect(&mut self, owner: ObjectId, item: &UserItem) -> bool {
        let Some(p) = self.objects.get(&owner).and_then(|o| o.player()) else {
            return false;
        };
        let Some(active) = p.active_companion.filter(|_| p.companion.is_some()) else {
            return false;
        };
        if !p.companion_collection {
            return false;
        }
        let Some(c) = p
            .companions
            .iter()
            .find(|c| c.index == active && c.hunger > 0)
        else {
            return false;
        };
        if item.info == self.data.gold_item {
            let gold = self.guild_tax_gold(owner, item.count as u64);
            self.objects
                .get_mut(&owner)
                .unwrap()
                .player_mut()
                .unwrap()
                .bag
                .gold += gold;
            self.send_gold(owner);
            return true;
        }
        if !self.companion_can_gain(c, item) {
            return false;
        }
        self.companion_gain_item(owner, active, item.clone());
        true
    }

    /// Companion AI: stay near the owner and collect the owner's drops.
    pub(super) fn process_companion(&mut self, id: ObjectId, owner: ObjectId) {
        let now = self.now;
        let Some(o) = self.objects.get(&owner).filter(|o| o.is_player()) else {
            self.remove_object(id);
            return;
        };
        let (omap, oloc, odir, account) =
            (o.map, o.location, o.direction, o.player().unwrap().account);
        let cindex = self.objects[&id].monster_ref().companion.unwrap_or(0);
        let (map, loc) = (self.objects[&id].map, self.objects[&id].location);
        // Out of reach: appear behind the owner.
        if omap != map || oloc.distance(loc) > MAX_VIEW_RANGE {
            let behind = oloc.step(odir.rotate(4), 1);
            let to = if self.maps[&omap].file.is_walkable(behind.x, behind.y)
                && !self.cell_blocked(omap, behind, false)
            {
                behind
            } else {
                oloc
            };
            if omap != map {
                self.change_map(id, omap, to);
            } else {
                self.teleport_object(id, to);
            }
            return;
        }
        if now < self.objects[&id].move_time {
            return;
        }
        // Hungry companions only follow.
        let companion = self.objects[&owner]
            .player()
            .unwrap()
            .companions
            .iter()
            .find(|c| c.index == cindex);
        let hungry = companion.is_none_or(|c| c.hunger <= 0);
        if !hungry {
            let gold_item = self.data.gold_item;
            let mut best: Option<(ObjectId, i32)> = None;
            for other in self.on_map(map) {
                let Kind::Item(it) = &other.kind else {
                    continue;
                };
                if it.owner != Some(account) {
                    continue;
                }
                let d = other.location.distance(loc);
                if d > PICKUP_RANGE {
                    continue;
                }
                let is_gold = it.item.info == gold_item;
                if !is_gold && companion.is_none_or(|c| !self.companion_can_gain(c, &it.item)) {
                    continue;
                }
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((other.id, d));
                }
            }
            if let Some((target, d)) = best {
                if d <= 1 {
                    let item = match &self.objects[&target].kind {
                        Kind::Item(it) => it.item.clone(),
                        _ => return,
                    };
                    self.remove_object(target);
                    if item.info == gold_item {
                        let gold = self.guild_tax_gold(owner, item.count as u64);
                        let p = self
                            .objects
                            .get_mut(&owner)
                            .and_then(|o| o.player_mut())
                            .unwrap();
                        p.bag.gold += gold;
                        self.send_gold(owner);
                    } else {
                        self.companion_gain_item(owner, cindex, item);
                    }
                } else {
                    let goal = self.objects[&target].location;
                    self.move_toward(id, goal, false);
                }
                return;
            }
        }
        // Idle: keep within two cells of the owner.
        if oloc.distance(loc) > 2 {
            self.move_toward(id, oloc, false);
        }
    }
}
