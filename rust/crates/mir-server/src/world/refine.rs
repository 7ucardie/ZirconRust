//! Weapon refining (Zircon `PlayerObject.NPC.NPCRefine` / `NPCRefineRetrieve`):
//! at a Refine page the equipped weapon goes into the furnace with black
//! iron ore, common jewellery and an optional special for 50,000 gold; the
//! chance depends on ore durability, jewellery level and quality; the
//! weapon comes back at a RefineRetrieve page after the quality's wait,
//! improved on a successful roll.

use serde::{Deserialize, Serialize};

use super::*;
use crate::items::{Bag, StatSource, UserItem, ACCESSORY_EXPERIENCE};
use mir_formats::mirdb::stat;
use mir_proto::{item_type, refine_quality, refine_type, slot, ChatKind, RefineSummary};
use std::collections::HashSet;

/// Zircon: `RefineCost` 50,000 gold; `ItemEffect.BlackIronOre` = 20;
/// `ItemType.RefineSpecial` = 17; `Stat.MaxRefineChance` = 81.
pub const REFINE_COST: u64 = 50_000;
pub const BLACK_IRON_ORE_EFFECT: u8 = 20;
pub const REFINE_SPECIAL_TYPE: u8 = 17;
pub const MAX_REFINE_CHANCE_STAT: i32 = 81;
/// Zircon `Stat.WeaponElement` and the first elemental attack stat
/// (`FireAttack` 20, then every second id up to `PhantomAttack` 32).
pub const WEAPON_ELEMENT_STAT: i32 = 52;
pub const FIRE_ATTACK_STAT: i32 = 20;
/// Zircon `Globals.RefineTimes` by quality, in milliseconds.
pub const REFINE_TIMES: [u64; 5] = [60_000, 1_800_000, 3_600_000, 21_600_000, 86_400_000];
const ACCESSORY_RESET_COST: u64 = 1_000_000;

fn accessory_kind(kind: u8) -> bool {
    matches!(
        kind,
        item_type::NECKLACE | item_type::BRACELET | item_type::RING
    )
}

fn accessory_cell(bag: &Bag, safe: bool, cell: (Grid, u8, u32)) -> Option<&UserItem> {
    if cell.0 == Grid::Storage && !safe {
        return None;
    }
    bag.grid(cell.0)
        .get(cell.1 as usize)?
        .as_ref()
        .filter(|item| item.id == cell.2)
}

pub(super) fn distinct_cells(cells: &[(Grid, u8, u32)]) -> bool {
    let mut seen = HashSet::new();
    cells
        .iter()
        .all(|(grid, slot, _)| seen.insert((*grid, *slot)))
}

fn accessory_stats(kind: u8, def: &crate::data::ItemDef, amount: i32) -> Option<Vec<(i32, i32)>> {
    let single = match kind {
        refine_type::DC => stat::MAX_DC,
        refine_type::HEALTH => stat::HEALTH,
        refine_type::MANA => stat::MANA,
        refine_type::DC_PERCENT => stat::DC_PERCENT,
        refine_type::HEALTH_PERCENT => 54,
        refine_type::MANA_PERCENT => 94,
        refine_type::ACCURACY => stat::ACCURACY,
        refine_type::AGILITY => stat::AGILITY,
        refine_type::FIRE..=refine_type::PHANTOM => {
            FIRE_ATTACK_STAT + (kind - refine_type::FIRE) as i32 * 2
        }
        _ => -1,
    };
    if single >= 0 {
        return Some(vec![(
            single,
            amount
                * if matches!(kind, refine_type::HEALTH | refine_type::MANA) {
                    10
                } else {
                    1
                },
        )]);
    }
    match kind {
        refine_type::AC => Some(vec![(stat::MIN_AC, amount), (stat::MAX_AC, amount)]),
        refine_type::MR => Some(vec![(stat::MIN_MR, amount), (stat::MAX_MR, amount)]),
        refine_type::SPELL_POWER | refine_type::SP_PERCENT => {
            let mc = def.stat(stat::MIN_MC) > 0 || def.stat(stat::MAX_MC) > 0;
            let sc = def.stat(stat::MIN_SC) > 0 || def.stat(stat::MAX_SC) > 0;
            let (mc_stat, sc_stat) = if kind == refine_type::SPELL_POWER {
                (stat::MAX_MC, stat::MAX_SC)
            } else {
                (stat::MC_PERCENT, stat::SC_PERCENT)
            };
            Some(
                [(mc_stat, mc), (sc_stat, sc)]
                    .into_iter()
                    .filter_map(|(stat, has)| (has || (!mc && !sc)).then_some((stat, amount)))
                    .collect(),
            )
        }
        _ => None,
    }
}

/// A weapon in the furnace (Zircon `RefineInfo`), persisted per character.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRefine {
    pub index: u32,
    pub weapon: UserItem,
    pub refine_type: u8,
    pub quality: u8,
    pub chance: i32,
    pub max_chance: i32,
    /// Unix seconds when it can be collected.
    pub ready_at: u64,
}

fn now_secs() -> u64 {
    crate::accounts::now_secs()
}

impl World {
    fn refine_line(&mut self, id: ObjectId, text: &str) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text: text.to_string(),
            },
        );
    }

    /// The open NPC page's dialog type, if any.
    pub(super) fn npc_dialog_type(&self, id: ObjectId) -> Option<i32> {
        let (_, page) = self.objects.get(&id)?.player()?.npc?;
        Some(self.data.npc_pages.get(&page)?.dialog_type)
    }

    pub(super) fn send_refine_list(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let now = now_secs();
        let list: Vec<RefineSummary> = p
            .refines
            .iter()
            .map(|r| RefineSummary {
                index: r.index,
                weapon: r.weapon.instance(),
                refine_type: r.refine_type,
                quality: r.quality,
                chance: r.chance,
                max_chance: r.max_chance,
                ready_in_ms: r.ready_at.saturating_sub(now) * 1000,
            })
            .collect();
        self.send_to(id, ServerMessage::RefineList(list));
    }

    /// Start a refine (Zircon `NPCRefine`): cells are (grid, slot, count).
    pub fn npc_refine(
        &mut self,
        id: ObjectId,
        refine_type: u8,
        quality: u8,
        ores: Vec<(Grid, u8, u32)>,
        items: Vec<(Grid, u8, u32)>,
        specials: Vec<(Grid, u8, u32)>,
    ) {
        if !(refine_type::DURABILITY..=refine_type::PHANTOM).contains(&refine_type)
            || quality > refine_quality::PRECISE
        {
            return;
        }
        if self.npc_dialog_type(id) != Some(3) {
            return;
        }
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if o.dead || ores.len() > 5 || items.len() > 3 || specials.len() > 1 {
            return;
        }
        // One cell named twice would add its ore or quality twice but be
        // consumed once.
        if !distinct_cells(&[ores.as_slice(), items.as_slice(), specials.as_slice()].concat()) {
            return;
        }
        let Some(weapon) = p.bag.equipment.get(slot::WEAPON).cloned().flatten() else {
            self.refine_line(id, "You need to be holding the weapon to refine.");
            return;
        };
        if !weapon.refinable || weapon.non_refinable {
            self.refine_line(id, "This weapon is not ready to refine. Eligible weapons gain readiness through combat experience.");
            return;
        }
        if p.bag.gold < REFINE_COST {
            self.refine_line(id, "Refining costs 50,000 gold.");
            return;
        }
        let in_safe_zone = o.in_safe_zone;
        let cell = |grid: Grid, s: u8| -> Option<UserItem> {
            if grid == Grid::Storage && !in_safe_zone {
                return None;
            }
            p.bag.grid(grid).get(s as usize).cloned().flatten()
        };
        let (mut ore, mut level_sum, mut special) = (0i64, 0i64, 0i32);
        for (g, s, c) in &ores {
            let Some(it) = cell(*g, *s) else { return };
            if *c == 0 || *c > it.count {
                return;
            }
            if self.data.items.get(&it.info).map(|d| d.effect) != Some(BLACK_IRON_ORE_EFFECT) {
                self.refine_line(id, "Only black iron ore feeds the furnace.");
                return;
            }
            ore += it.durability as i64;
        }
        let mut quality_items = 0;
        for (g, s, c) in &items {
            let Some(it) = cell(*g, *s) else { return };
            if *c == 0 || *c > it.count {
                return;
            }
            let Some(def) = self.data.items.get(&it.info) else {
                return;
            };
            if !matches!(
                def.item_type,
                item_type::NECKLACE | item_type::BRACELET | item_type::RING
            ) {
                self.refine_line(id, "Only necklaces, bracelets and rings go in.");
                return;
            }
            if def.rarity != 0 {
                quality_items += 1;
            }
            level_sum += def.required_amount as i64;
        }
        for (g, s, _) in &specials {
            let Some(it) = cell(*g, *s) else { return };
            let Some(def) = self.data.items.get(&it.info) else {
                return;
            };
            if def.item_type != REFINE_SPECIAL_TYPE || def.shape != 1 {
                return;
            }
            special += def.stat(MAX_REFINE_CHANCE_STAT);
        }
        // Zircon: max 90% - level (+special), base 60% - 5% per level, ore
        // 1% per 2000 durability, jewellery 1% per 6 levels, 25% per superior
        // item; quality shifts the ceiling.
        let mut max_chance = 90 - weapon.level + special;
        let mut chance = 60 - weapon.level * 5;
        max_chance += match quality {
            refine_quality::RUSH => -5,
            refine_quality::QUICK => 0,
            refine_quality::STANDARD => 5,
            refine_quality::CAREFUL => 10,
            _ => 20,
        };
        chance += (ore / 2000) as i32;
        chance += (level_sum / 6) as i32;
        chance += quality_items * 25;
        max_chance = max_chance.min(100);
        chance = chance.min(max_chance);

        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let mut changes = Changed::new();
        for (g, s, c) in ores.iter().chain(&items) {
            if let Some(ch) = p.bag.take(*g, *s, *c) {
                changes.push(ch);
            }
        }
        for (g, s, _) in &specials {
            if let Some(ch) = p.bag.take(*g, *s, 1) {
                changes.push(ch);
            }
        }
        p.bag.equipment[slot::WEAPON] = None;
        changes.push((Grid::Equipment, slot::WEAPON as u8, None));
        p.bag.gold -= REFINE_COST;
        let index = p.refines.iter().map(|r| r.index).max().unwrap_or(0) + 1;
        p.refines.push(StoredRefine {
            index,
            weapon,
            refine_type,
            quality,
            chance,
            max_chance,
            ready_at: now_secs() + REFINE_TIMES[quality as usize] / 1000,
        });
        self.send_changes(id, changes);
        self.send_gold(id);
        self.refresh_stats(id, false);
        self.refresh_appearance(id);
        self.send_player_stats(id);
        self.send_refine_list(id);
        self.refine_line(id, "Your weapon is in the furnace.");
    }

    /// Collect a refined weapon (Zircon `NPCRefineRetrieve`).
    pub fn npc_refine_retrieve(&mut self, id: ObjectId, index: u32) {
        if self.npc_dialog_type(id) != Some(4) {
            return;
        }
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some(r) = p.refines.iter().find(|r| r.index == index).cloned() else {
            return;
        };
        if now_secs() < r.ready_at {
            self.refine_line(id, "Your weapon is not ready yet.");
            return;
        }
        if !p.bag.inventory.iter().any(|s| s.is_none()) {
            self.refine_line(id, "You need a free bag slot for the weapon.");
            return;
        }
        let mut weapon = r.weapon.clone();
        let def = self.data.items.get(&weapon.info).cloned();
        let success = self.rng.random_range(0..100) < r.chance;
        if success {
            let mut add =
                |stat_id: i32, amount: i32| weapon.add_stat(stat_id, amount, StatSource::Refine);
            match r.refine_type {
                refine_type::DURABILITY => weapon.max_durability += 2000,
                refine_type::DC => add(stat::MAX_DC, 1),
                refine_type::SPELL_POWER => {
                    let has = |s: i32| def.as_ref().map(|d| d.stat(s) > 0).unwrap_or(false);
                    let mc = has(stat::MIN_MC) || has(stat::MAX_MC);
                    let sc = has(stat::MIN_SC) || has(stat::MAX_SC);
                    if !mc && !sc {
                        add(stat::MAX_MC, 1);
                        add(stat::MAX_SC, 1);
                    } else {
                        if mc {
                            add(stat::MAX_MC, 1);
                        }
                        if sc {
                            add(stat::MAX_SC, 1);
                        }
                    }
                }
                t @ refine_type::FIRE..=refine_type::PHANTOM => {
                    let element = (t - refine_type::FIRE + 1) as i32;
                    add(FIRE_ATTACK_STAT + (element - 1) * 2, 1);
                    let current = weapon.bonus(WEAPON_ELEMENT_STAT)
                        + def
                            .as_ref()
                            .map(|d| d.stat(WEAPON_ELEMENT_STAT))
                            .unwrap_or(0);
                    weapon.add_stat(WEAPON_ELEMENT_STAT, element - current, StatSource::Refine);
                }
                _ => {}
            }

            self.refine_line(id, "The refine succeeded.");
        } else {
            self.refine_line(id, "The refine failed.");
        }
        weapon.refinable = false;
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        p.refines.retain(|x| x.index != index);
        let free = p.bag.inventory.iter().position(|s| s.is_none()).unwrap();
        p.bag.inventory[free] = Some(weapon.clone());
        self.send_changes(
            id,
            vec![(Grid::Inventory, free as u8, Some(weapon.instance()))],
        );
        self.send_to(id, ServerMessage::RefineRetrieved { index });
    }

    pub fn npc_accessory_level(
        &mut self,
        id: ObjectId,
        target: (Grid, u8, u32),
        materials: Vec<(Grid, u8, u32)>,
    ) {
        if self.npc_dialog_type(id) != Some(12) || materials.is_empty() || materials.len() > 100 {
            return;
        }
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else { return };
        if o.dead || !distinct_cells(&[&[target][..], &materials].concat()) {
            return;
        }
        let safe = o.in_safe_zone;
        let Some(item) = accessory_cell(&p.bag, safe, target) else {
            return;
        };
        let Some(def) = self.data.items.get(&item.info) else {
            return;
        };
        if !accessory_kind(def.item_type)
            || item.non_refinable
            || item.refinable
            || item.level < 0
            || item.level as usize >= ACCESSORY_EXPERIENCE.len()
        {
            return;
        }
        let info = item.info;
        for &cell in &materials {
            let Some(material) = accessory_cell(&p.bag, safe, cell) else {
                return;
            };
            if material.info != info
                || material.locked
                || material.non_refinable
                || (material.bound && !item.bound)
            {
                return;
            }
        }
        let mut changes = Changed::new();
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        for (grid, slot, _) in materials {
            let target_level = p.bag.grid(target.0)[target.1 as usize]
                .as_ref()
                .unwrap()
                .level;
            if p.bag.grid(target.0)[target.1 as usize]
                .as_ref()
                .unwrap()
                .refinable
            {
                break;
            }
            let material = p.bag.grid(grid)[slot as usize].as_ref().unwrap().clone();
            let mut gain = if def.rarity != 0 || target_level == 1 {
                5u64
            } else {
                1
            };
            if material.level > 1 && def.rarity == 0 {
                gain = gain.saturating_sub(4)
            }
            for level in 1..material.level {
                gain = gain.saturating_add(
                    ACCESSORY_EXPERIENCE
                        .get(level as usize)
                        .copied()
                        .unwrap_or(0),
                );
            }
            gain = gain.saturating_add(material.accessory_experience);
            let target_item = p.bag.grid_mut(target.0)[target.1 as usize]
                .as_mut()
                .unwrap();
            target_item.accessory_experience =
                target_item.accessory_experience.saturating_add(gain);
            let threshold = ACCESSORY_EXPERIENCE[target_item.level as usize];
            if target_item.accessory_experience >= threshold {
                target_item.accessory_experience -= threshold;
                target_item.level += 1;
                target_item.refinable = true;
            }
            if let Some(change) = p.bag.take(grid, slot, 1) {
                changes.push(change)
            }
        }
        let updated = p.bag.grid(target.0)[target.1 as usize]
            .as_ref()
            .unwrap()
            .instance();
        changes.push((target.0, target.1, Some(updated)));
        self.send_changes(id, changes);
        self.refine_line(id, "Accessory experience updated.");
    }

    pub fn npc_accessory_upgrade(&mut self, id: ObjectId, target: (Grid, u8, u32), kind: u8) {
        if self.npc_dialog_type(id) != Some(11) {
            return;
        }
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else { return };
        if o.dead {
            return;
        }
        let Some(item) = accessory_cell(&p.bag, o.in_safe_zone, target) else {
            return;
        };
        let Some(def) = self.data.items.get(&item.info) else {
            return;
        };
        if !accessory_kind(def.item_type) || !item.refinable || item.non_refinable {
            return;
        }
        let Some(stats) = accessory_stats(kind, def, 1) else {
            return;
        };
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let item = p.bag.grid_mut(target.0)[target.1 as usize]
            .as_mut()
            .unwrap();
        for (stat, amount) in stats {
            item.add_stat(stat, amount, StatSource::Refine)
        }
        item.refinable = false;
        if let Some(&threshold) = ACCESSORY_EXPERIENCE.get(item.level as usize) {
            if item.accessory_experience >= threshold {
                item.accessory_experience -= threshold;
                item.level += 1;
                item.refinable = true;
            }
        }
        let updated = item.instance();
        self.send_changes(id, vec![(target.0, target.1, Some(updated))]);
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.refine_line(id, "Accessory upgraded.");
    }

    pub fn npc_accessory_reset(&mut self, id: ObjectId, target: (Grid, u8, u32)) {
        if self.npc_dialog_type(id) != Some(13) {
            return;
        }
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else { return };
        if o.dead || p.bag.gold < ACCESSORY_RESET_COST {
            return;
        }
        let Some(item) = accessory_cell(&p.bag, o.in_safe_zone, target) else {
            return;
        };
        let Some(def) = self.data.items.get(&item.info) else {
            return;
        };
        if !accessory_kind(def.item_type)
            || item.non_refinable
            || item.level < 1
            || (item.level == 1 && item.refinable)
        {
            return;
        }
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let item = p.bag.grid_mut(target.0)[target.1 as usize]
            .as_mut()
            .unwrap();
        item.added.retain(|s| s.source != StatSource::Refine);
        while item.level > 1 {
            item.accessory_experience = item
                .accessory_experience
                .saturating_add(ACCESSORY_EXPERIENCE[(item.level - 1) as usize]);
            item.level -= 1;
        }
        if item.accessory_experience >= ACCESSORY_EXPERIENCE[1] {
            item.accessory_experience -= ACCESSORY_EXPERIENCE[1];
            item.level = 2;
            item.refinable = true;
        }
        let updated = item.instance();
        p.bag.gold -= ACCESSORY_RESET_COST;
        self.send_changes(id, vec![(target.0, target.1, Some(updated))]);
        self.send_gold(id);
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.refine_line(id, "Accessory refinement reset.");
    }

    pub fn npc_accessory_refine(
        &mut self,
        id: ObjectId,
        target: (Grid, u8, u32),
        ore: (Grid, u8, u32),
        materials: Vec<(Grid, u8, u32)>,
        kind: u8,
    ) {
        if !matches!(self.npc_dialog_type(id), Some(11 | 15)) || materials.len() > 100 {
            return;
        }
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else { return };
        if o.dead
            || p.bag.gold < REFINE_COST
            || !distinct_cells(&[&[target, ore][..], &materials].concat())
        {
            return;
        }
        let safe = o.in_safe_zone;
        let Some(item) = accessory_cell(&p.bag, safe, target) else {
            return;
        };
        let Some(def) = self.data.items.get(&item.info) else {
            return;
        };
        if !accessory_kind(def.item_type) || item.non_refinable || item.refinable || item.level > 1
        {
            return;
        }
        let Some(ore_item) = accessory_cell(&p.bag, safe, ore) else {
            return;
        };
        if self.data.items.get(&ore_item.info).map(|d| d.effect) != Some(25)
            || ore_item.non_refinable
        {
            return;
        }
        for &cell in &materials {
            let Some(material) = accessory_cell(&p.bag, safe, cell) else {
                return;
            };
            if material.info != item.info
                || material.level > 1
                || material.locked
                || material.non_refinable
                || (material.bound && !item.bound)
                || material.added != item.added
            {
                return;
            }
        }
        let chance = (100 - ore_item.durability / 1000).max(1);
        let success = self.rng.random_range(0..chance) < if def.rarity == 0 { 30 } else { 40 };
        let amount = if success && self.rng.random_range(0..chance) == 0 {
            2
        } else {
            1
        };
        let Some(mut stats) = accessory_stats(kind, def, amount) else {
            return;
        };
        // Combination refines add only maximum defence, unlike level upgrades.
        if kind == refine_type::AC {
            stats.retain(|(stat, _)| *stat == stat::MAX_AC)
        }
        if kind == refine_type::MR {
            stats.retain(|(stat, _)| *stat == stat::MAX_MR)
        }
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let mut changes = Changed::new();
        for (grid, slot, _) in materials {
            if let Some(change) = p.bag.take(grid, slot, 1) {
                changes.push(change)
            }
        }
        if let Some(change) = p.bag.take(ore.0, ore.1, 1) {
            changes.push(change)
        }
        if success {
            let item = p.bag.grid_mut(target.0)[target.1 as usize]
                .as_mut()
                .unwrap();
            for (stat, value) in stats {
                item.add_stat(stat, value, StatSource::Added)
            }
            changes.push((target.0, target.1, Some(item.instance())));
        } else if let Some(change) = p.bag.take(target.0, target.1, 1) {
            changes.push(change);
        }
        p.bag.gold -= REFINE_COST;
        self.send_changes(id, changes);
        self.send_gold(id);
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.refine_line(
            id,
            if success {
                "Accessory refine succeeded."
            } else {
                "Accessory refine failed; accessory was destroyed."
            },
        );
    }
}
