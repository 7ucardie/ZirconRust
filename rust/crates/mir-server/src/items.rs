//! Inventory, equipment and item rules (Zircon `PlayerObject.Inventory.cs`).

use std::collections::HashMap;

use mir_proto::{
    item_type, Class, Gender, Grid, ItemInstance, Weights, EQUIPMENT_SIZE, INVENTORY_SIZE,
    MAX_INVENTORY_SIZE,
};
use serde::{Deserialize, Serialize};

use crate::data::{GameData, ItemDef};

/// Bonus provenance, retained independently even when two sources modify one stat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatSource {
    Legacy,
    Added,
    Refine,
    Enhancement,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StoredStat")]
pub struct ItemStat {
    pub stat: i32,
    pub amount: i32,
    pub source: StatSource,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredStat {
    Sourced {
        stat: i32,
        amount: i32,
        source: StatSource,
    },
    Legacy((i32, i32)),
}
impl From<StoredStat> for ItemStat {
    fn from(value: StoredStat) -> Self {
        match value {
            StoredStat::Sourced {
                stat,
                amount,
                source,
            } => Self {
                stat,
                amount,
                source,
            },
            StoredStat::Legacy((stat, amount)) => Self {
                stat,
                amount,
                source: StatSource::Legacy,
            },
        }
    }
}
fn legacy_refinable() -> bool {
    true
}

/// C# Globals.WeaponExperienceList. Stored experience uses tenths to avoid
/// losing fractional experience from small kills.
pub const WEAPON_EXPERIENCE: [u64; 17] = [
    0, 300000, 350000, 400000, 450000, 500000, 550000, 600000, 650000, 700000, 750000, 800000,
    850000, 900000, 1000000, 1300000, 2000000,
];

/// Zircon `Globals.AccessoryExperienceList`, indexed by accessory level.
pub const ACCESSORY_EXPERIENCE: [u64; 11] = [
    0, 5, 20, 80, 350, 1_500, 6_200, 26_500, 114_000, 490_000, 2_090_000,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserItem {
    pub id: u32,
    pub info: i32,
    pub count: u32,
    pub durability: i32,
    pub max_durability: i32,
    /// Added stats with provenance; old JSON pairs load as Legacy.
    #[serde(default)]
    pub added: Vec<ItemStat>,
    /// Refine level (Zircon `UserItem.Level`).
    #[serde(default)]
    pub level: i32,
    /// Existing saves previously allowed refining unconditionally.
    #[serde(default = "legacy_refinable")]
    pub refinable: bool,
    #[serde(default)]
    pub non_refinable: bool,
    #[serde(default)]
    pub bound: bool,
    #[serde(default)]
    pub worthless: bool,
    #[serde(default)]
    pub locked: bool,
    /// Unix seconds when special repair becomes available again.
    #[serde(default)]
    pub special_repair_until: u64,

    #[serde(default)]
    pub weapon_experience_tenths: u64,
    #[serde(default)]
    pub accessory_experience: u64,
    /// Unix seconds this item expires at, for store goods with a `Duration`
    /// (Zircon `UserItemFlags.Expirable`). `None` never expires.
    #[serde(default)]
    pub expires_at: Option<u64>,
    /// Zircon `UserItem.Colour`: the armour dye, packed ARGB (0 = undyed).
    #[serde(default)]
    pub colour: u32,
}

impl UserItem {
    pub fn add_stat(&mut self, stat: i32, amount: i32, source: StatSource) {
        if let Some(s) = self
            .added
            .iter_mut()
            .find(|s| s.stat == stat && s.source == source)
        {
            s.amount = s.amount.saturating_add(amount);
        } else {
            self.added.push(ItemStat {
                stat,
                amount,
                source,
            });
        }
    }

    pub fn bonus(&self, stat: i32) -> i32 {
        self.added
            .iter()
            .filter(|s| s.stat == stat)
            .fold(0i32, |n, s| n.saturating_add(s.amount))
    }

    pub fn bonus_totals(&self) -> Vec<(i32, i32)> {
        let mut stats = std::collections::BTreeMap::<i32, i32>::new();
        for s in &self.added {
            let v = stats.entry(s.stat).or_default();
            *v = v.saturating_add(s.amount);
        }
        stats.into_iter().collect()
    }

    pub fn gain_weapon_experience(&mut self, player_experience: u64) -> bool {
        if player_experience == 0 || self.refinable || self.non_refinable {
            return false;
        }
        let Some(required) = usize::try_from(self.level)
            .ok()
            .and_then(|i| WEAPON_EXPERIENCE.get(i))
        else {
            return false;
        };
        self.weapon_experience_tenths = self
            .weapon_experience_tenths
            .saturating_add(player_experience);
        if self.weapon_experience_tenths < required * 10 {
            return false;
        }
        self.weapon_experience_tenths = 0;
        self.level += 1;
        self.refinable = (self.level as usize) < WEAPON_EXPERIENCE.len();
        true
    }

    pub fn fresh(data: &GameData, info: i32, count: u32) -> Option<Self> {
        let def = data.items.get(&info)?;
        Some(Self {
            id: 0,
            info,
            count,
            durability: def.durability,
            max_durability: def.durability,
            added: Vec::new(),
            level: 0,
            refinable: false,
            non_refinable: false,
            bound: false,
            worthless: false,
            locked: false,
            special_repair_until: 0,
            weapon_experience_tenths: 0,
            accessory_experience: 0,
            expires_at: None,
            colour: 0,
        })
    }

    pub fn stacks_with(&self, other: &Self) -> bool {
        self.bound == other.bound
            && self.worthless == other.worthless
            && self.locked == other.locked
            && self.special_repair_until == other.special_repair_until
            && self.info == other.info
            && self.expires_at == other.expires_at
            && self.colour == other.colour
            && self.added == other.added
            && self.level == other.level
            && self.refinable == other.refinable
            && self.non_refinable == other.non_refinable
            && self.weapon_experience_tenths == other.weapon_experience_tenths
            && self.accessory_experience == other.accessory_experience
            && self.durability == other.durability
            && self.max_durability == other.max_durability
    }

    pub fn instance(&self) -> ItemInstance {
        ItemInstance {
            id: self.id,
            info: self.info,
            count: self.count,
            durability: self.durability,
            max_durability: self.max_durability,
            added: self.bonus_totals(),
            enhancements: self
                .added
                .iter()
                .filter(|s| s.source == StatSource::Enhancement)
                .map(|s| (s.stat, s.amount))
                .collect(),
            refinable: self.refinable,
            level: self.level,
            accessory_experience: self.accessory_experience,
            non_refinable: self.non_refinable,
            bound: self.bound,
            worthless: self.worthless,
            locked: self.locked,
            expires_at: self.expires_at,
            colour: self.colour,
        }
    }
}

/// Persisted form: which grid and slot an item sits in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredItem {
    pub grid: Grid,
    pub slot: u8,
    pub item: UserItem,
}

#[derive(Debug, Default)]
pub struct Bag {
    pub inventory: Vec<Option<UserItem>>,
    pub equipment: Vec<Option<UserItem>>,
    /// Account storage (filled by `World::set_storage`).
    pub storage: Vec<Option<UserItem>>,
    pub gold: u64,
}

/// Zircon `Globals.StorageSize`: base account storage slots.
pub const STORAGE_SIZE: u32 = 100;

/// Result of putting an item somewhere: the slots that changed.
pub type Changed = Vec<(Grid, u8, Option<ItemInstance>)>;

impl Bag {
    pub fn new() -> Bag {
        Bag {
            inventory: (0..INVENTORY_SIZE).map(|_| None).collect(),
            equipment: (0..EQUIPMENT_SIZE).map(|_| None).collect(),
            storage: (0..STORAGE_SIZE).map(|_| None).collect(),
            gold: 0,
        }
    }

    pub fn from_stored(items: &[StoredItem], gold: u64) -> Bag {
        Self::from_stored_with_size(items, gold, INVENTORY_SIZE)
    }

    pub fn from_stored_with_size(items: &[StoredItem], gold: u64, size: usize) -> Bag {
        let mut b = Bag::new();
        b.inventory
            .resize(size.clamp(INVENTORY_SIZE, MAX_INVENTORY_SIZE), None);
        b.gold = gold;
        for s in items {
            let target = match s.grid {
                Grid::Inventory => b.inventory.get_mut(s.slot as usize),
                Grid::Equipment => b.equipment.get_mut(s.slot as usize),
                Grid::Storage => None,
            };
            if let Some(cell) = target {
                if cell.is_none() {
                    *cell = Some(s.item.clone());
                }
            }
        }
        b
    }

    pub fn to_stored(&self) -> Vec<StoredItem> {
        let mut out = Vec::new();
        for (i, it) in self.inventory.iter().enumerate() {
            if let Some(item) = it {
                out.push(StoredItem {
                    grid: Grid::Inventory,
                    slot: i as u8,
                    item: item.clone(),
                });
            }
        }
        for (i, it) in self.equipment.iter().enumerate() {
            if let Some(item) = it {
                out.push(StoredItem {
                    grid: Grid::Equipment,
                    slot: i as u8,
                    item: item.clone(),
                });
            }
        }
        out
    }

    pub fn grid(&self, g: Grid) -> &Vec<Option<UserItem>> {
        match g {
            Grid::Inventory => &self.inventory,
            Grid::Equipment => &self.equipment,
            Grid::Storage => &self.storage,
        }
    }

    pub fn grid_mut(&mut self, g: Grid) -> &mut Vec<Option<UserItem>> {
        match g {
            Grid::Inventory => &mut self.inventory,
            Grid::Equipment => &mut self.equipment,
            Grid::Storage => &mut self.storage,
        }
    }

    pub fn max_id(&self) -> u32 {
        self.inventory
            .iter()
            .chain(self.equipment.iter())
            .flatten()
            .map(|i| i.id)
            .max()
            .unwrap_or(0)
    }

    pub fn count_of(&self, info: i32) -> u32 {
        self.inventory
            .iter()
            .flatten()
            .filter(|i| i.info == info)
            .map(|i| i.count)
            .sum()
    }

    /// Weight of everything in the bag (Zircon `RefreshWeight`).
    pub fn bag_weight(&self, data: &GameData) -> i32 {
        self.inventory
            .iter()
            .flatten()
            .map(|i| item_weight(data, i))
            .sum()
    }

    pub fn hand_weight(&self, data: &GameData) -> i32 {
        self.equipment
            .iter()
            .flatten()
            .filter(|i| {
                matches!(
                    data.items.get(&i.info).map(|d| d.item_type),
                    Some(item_type::WEAPON) | Some(item_type::TORCH) | Some(item_type::SHIELD)
                )
            })
            .map(|i| item_weight(data, i))
            .sum()
    }

    pub fn wear_weight(&self, data: &GameData) -> i32 {
        self.equipment
            .iter()
            .flatten()
            .filter(|i| {
                !matches!(
                    data.items.get(&i.info).map(|d| d.item_type),
                    Some(item_type::WEAPON) | Some(item_type::TORCH)
                )
            })
            .map(|i| item_weight(data, i))
            .sum()
    }

    /// Stack only items whose persistent properties agree.
    pub fn can_gain_item(&self, data: &GameData, item: &UserItem, max_bag: i32) -> bool {
        let Some(def) = data.items.get(&item.info) else {
            return false;
        };
        if i64::from(self.bag_weight(data)) + i64::from(def.weight) * i64::from(item.count)
            > i64::from(max_bag)
        {
            return false;
        }
        let stack = def.stack_size.max(1) as u32;
        let mut remaining = item.count;
        for existing in self.inventory.iter().flatten() {
            if existing.stacks_with(item) {
                remaining = remaining.saturating_sub(stack.saturating_sub(existing.count));
            }
        }
        remaining.div_ceil(stack) <= self.inventory.iter().filter(|s| s.is_none()).count() as u32
    }

    pub fn can_gain(&self, data: &GameData, info: i32, count: u32, max_bag: i32) -> bool {
        let Some(item) = UserItem::fresh(data, info, count) else {
            return false;
        };
        self.can_gain_item(data, &item, max_bag)
    }

    /// Add fresh rewards, keeping timed and modified stacks separate.
    pub fn gain(&mut self, data: &GameData, info: i32, count: u32, next_id: &mut u32) -> Changed {
        let Some(item) = UserItem::fresh(data, info, count) else {
            return Changed::new();
        };
        self.gain_item(data, item, next_id)
    }

    /// Transfer an item without losing expiry, dye, durability or added stats.
    /// The caller must first check capacity with `can_gain_item`.
    pub fn gain_item(&mut self, data: &GameData, item: UserItem, next_id: &mut u32) -> Changed {
        let Some(def) = data.items.get(&item.info) else {
            return Changed::new();
        };
        let stack = def.stack_size.max(1) as u32;
        let mut count = item.count;
        let mut changed = Changed::new();
        for (i, cell) in self.inventory.iter_mut().enumerate() {
            if count == 0 {
                break;
            }
            if let Some(existing) = cell {
                if existing.stacks_with(&item) && existing.count < stack {
                    let add = (stack - existing.count).min(count);
                    existing.count += add;
                    count -= add;
                    changed.push((Grid::Inventory, i as u8, Some(existing.instance())));
                }
            }
        }
        for (i, cell) in self.inventory.iter_mut().enumerate() {
            if count == 0 {
                break;
            }
            if cell.is_none() {
                let add = stack.min(count);
                *next_id += 1;
                let placed = UserItem {
                    id: *next_id,
                    count: add,
                    ..item.clone()
                };
                changed.push((Grid::Inventory, i as u8, Some(placed.instance())));
                *cell = Some(placed);
                count -= add;
            }
        }
        changed
    }

    /// Remove `count` from a slot; returns the change (None when emptied).
    pub fn take(
        &mut self,
        grid: Grid,
        slot: u8,
        count: u32,
    ) -> Option<(Grid, u8, Option<ItemInstance>)> {
        let cell = self.grid_mut(grid).get_mut(slot as usize)?;
        let item = cell.as_mut()?;
        if item.count <= count {
            *cell = None;
            Some((grid, slot, None))
        } else {
            item.count -= count;
            Some((grid, slot, Some(item.instance())))
        }
    }

    /// Remove `count` of `info` from anywhere in the inventory.
    pub fn take_info(&mut self, info: i32, mut count: u32) -> Changed {
        let mut changed = Changed::new();
        for i in 0..self.inventory.len() {
            if count == 0 {
                break;
            }
            let Some(item) = &self.inventory[i] else {
                continue;
            };
            if item.info != info {
                continue;
            }
            let take = item.count.min(count);
            count -= take;
            if let Some(c) = self.take(Grid::Inventory, i as u8, take) {
                changed.push(c);
            }
        }
        changed
    }

    pub fn weights(&self, data: &GameData, max_bag: i32, max_wear: i32, max_hand: i32) -> Weights {
        Weights {
            bag: self.bag_weight(data),
            max_bag,
            wear: self.wear_weight(data),
            max_wear,
            hand: self.hand_weight(data),
            max_hand,
        }
    }

    /// Sum of `ItemInfoStat`s over equipped, unbroken items.
    /// Every stat the equipment grants, set bonuses included. `level` and
    /// `class` gate the set lines, as Zircon `RefreshStats` does.
    pub fn equipment_stats(&self, data: &GameData, level: i32, class: Class) -> HashMap<i32, i32> {
        let mut out = HashMap::new();
        // Distinct `ItemInfo` per set, so two copies of one piece count once
        // (Zircon guards its list with `if (!items.Contains(item.Info))`).
        let mut worn: HashMap<i32, Vec<i32>> = HashMap::new();
        for item in self.equipment.iter().flatten() {
            // A broken item is dead weight: no stats, and no set credit.
            if item.max_durability > 0 && item.durability <= 0 {
                continue;
            }
            if let Some(def) = data.items.get(&item.info) {
                if def.set != 0 {
                    let pieces = worn.entry(def.set).or_default();
                    if !pieces.contains(&def.index) {
                        pieces.push(def.index);
                    }
                }
                for (k, v) in &def.stats {
                    *out.entry(*k).or_insert(0) += v;
                }
            }
            for s in &item.added {
                *out.entry(s.stat).or_insert(0) += s.amount;
            }
        }
        for (stat, amount) in set_bonuses(data, &worn, level, class) {
            *out.entry(stat).or_insert(0) += amount;
        }
        out
    }

    /// The sets that are fully worn, as (set index, pieces worn, pieces
    /// needed). Drives the client's "3/3" readout.
    pub fn set_progress(&self, data: &GameData) -> Vec<(i32, usize, usize)> {
        let mut worn: HashMap<i32, Vec<i32>> = HashMap::new();
        for item in self.equipment.iter().flatten() {
            if item.max_durability > 0 && item.durability <= 0 {
                continue;
            }
            if let Some(def) = data.items.get(&item.info) {
                if def.set != 0 {
                    let pieces = worn.entry(def.set).or_default();
                    if !pieces.contains(&def.index) {
                        pieces.push(def.index);
                    }
                }
            }
        }
        let mut out: Vec<(i32, usize, usize)> = worn
            .into_iter()
            .map(|(set, pieces)| {
                let needed = data.sets.get(&set).map(|s| s.pieces).unwrap_or(0);
                (set, pieces.len(), needed)
            })
            .collect();
        out.sort_by_key(|(set, _, _)| *set);
        out
    }

    pub fn equipped_shape(&self, data: &GameData, slot: usize) -> Option<i32> {
        self.equipment
            .get(slot)?
            .as_ref()
            .and_then(|i| data.items.get(&i.info))
            .map(|d| d.shape)
    }

    pub fn equipped_exterior_effect(&self, data: &GameData, slot: usize) -> u8 {
        self.equipment
            .get(slot)
            .and_then(|item| item.as_ref())
            .and_then(|item| data.items.get(&item.info))
            .map(|item| item.exterior_effect)
            .unwrap_or(0)
    }

    /// The dye on an equipped item (Zircon `UserItem.Colour`), 0 when the
    /// slot is empty or undyed.
    pub fn equipped_colour(&self, slot: usize) -> u32 {
        self.equipment
            .get(slot)
            .and_then(|i| i.as_ref())
            .map(|i| i.colour)
            .unwrap_or(0)
    }
}

/// Zircon `RefreshStats`: a set pays out only when every one of its pieces
/// is worn, and then each line still has to clear the wearer's level and
/// name their class.
pub fn set_bonuses(
    data: &GameData,
    worn: &HashMap<i32, Vec<i32>>,
    level: i32,
    class: Class,
) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for (set, pieces) in worn {
        let Some(def) = data.sets.get(set) else {
            continue;
        };
        // A set with no pieces recorded would otherwise pay out for free.
        if def.pieces == 0 || pieces.len() != def.pieces {
            continue;
        }
        for stat in &def.stats {
            if level < stat.level || stat.class & class.flag() == 0 {
                continue;
            }
            out.push((stat.stat, stat.amount));
        }
    }
    out
}

/// One weighted pick from the weapon-craft table, as Zircon
/// `NPCWeaponCraft` rolls it: sum the weights of the rows this class can
/// take, pick a number under that total, then walk the rows subtracting
/// weights until it goes negative. `roll` is the caller's random number in
/// `0..total`, and the returned range is inclusive of `max_value`.
pub fn weapon_craft_pick(data: &GameData, class: Class, roll: i32) -> Option<(i32, i32, i32)> {
    let flag = class.flag();
    let mut value = roll;
    for stat in data
        .weapon_craft_stats
        .iter()
        .filter(|s| s.class & flag != 0)
    {
        value -= stat.weight;
        if value < 0 {
            return Some((stat.stat, stat.min_value, stat.max_value));
        }
    }
    None
}

/// The total weight `weapon_craft_pick` rolls against for a class.
pub fn weapon_craft_total(data: &GameData, class: Class) -> i32 {
    let flag = class.flag();
    data.weapon_craft_stats
        .iter()
        .filter(|s| s.class & flag != 0)
        .map(|s| s.weight)
        .sum()
}

pub fn item_weight(data: &GameData, item: &UserItem) -> i32 {
    match data.items.get(&item.info) {
        Some(d) if matches!(d.item_type, item_type::POISON | item_type::AMULET) => d.weight,
        Some(d) => d.weight * item.count as i32,
        None => 0,
    }
}

/// Zircon `CanUseItem`: gender, class and the required-stat check.
pub fn can_use(def: &ItemDef, class: Class, gender: Gender, level: i32) -> Result<(), String> {
    let gflag = match gender {
        Gender::Male => 1,
        Gender::Female => 2,
    };
    if def.required_gender & gflag == 0 {
        return Err("Your gender cannot use this item".into());
    }
    if def.required_class & class.flag() == 0 {
        return Err("Your class cannot use this item".into());
    }
    match def.required_type {
        0 if level < def.required_amount => Err(format!("Requires level {}", def.required_amount)),
        1 if level > def.required_amount => {
            Err(format!("Requires level {} or below", def.required_amount))
        }
        _ => Ok(()),
    }
}

/// Which equipment slot an item goes to by default (left before right).
pub fn default_slot(item_type: u8, equipment: &[Option<UserItem>]) -> Option<usize> {
    let slots = item_type::slots(item_type);
    slots
        .iter()
        .copied()
        .find(|s| equipment.get(*s).map(|c| c.is_none()).unwrap_or(false))
        .or_else(|| slots.first().copied())
}

/// Sell price (Zircon `UserItem.Price`).
pub fn sell_price(def: &ItemDef, item: &UserItem) -> u64 {
    if item.worthless {
        return 0;
    }
    let mut p = def.price as f64;
    if def.durability > 0 {
        let r = def.price as f64 / 2.0 / def.durability as f64;
        p = item.max_durability as f64 * r;
        let ratio = if item.max_durability > 0 {
            item.durability as f64 / item.max_durability as f64
        } else {
            0.0
        };
        p = (p / 2.0 + p / 2.0 * ratio + def.price as f64 / 2.0).floor();
    }
    let stats = item
        .bonus_totals()
        .iter()
        .map(|(_, amount)| amount.unsigned_abs() as u64)
        .sum::<u64>();
    p *= 1.0 + stats as f64 * 0.1;
    // Zircon Stat.SaleBonus5..20 are IDs 56..59.
    for (stat, rate) in [(59, 1.20), (58, 1.15), (57, 1.10), (56, 1.05)] {
        let needed = def.stat(stat);
        if needed > 0 && needed as u32 <= item.count {
            p *= rate;
            break;
        }
    }
    (p * item.count as f64 * def.sell_rate).max(0.0) as u64
}

/// Zircon `UserItem.RepairCost`: full value minus current sale value.
pub fn repair_cost(def: &ItemDef, item: &UserItem, special: bool) -> u64 {
    if def.durability <= 0 || item.durability >= item.max_durability {
        return 0;
    }
    let half = def.price as f64 / 2.0;
    let full = (item.max_durability as f64 * half / def.durability as f64 + half).floor();
    let stats = 1.0
        + item
            .bonus_totals()
            .iter()
            .map(|(_, amount)| amount.unsigned_abs() as u64)
            .sum::<u64>() as f64
            * 0.1;
    let cost = (full * stats * item.count as f64 - sell_price(def, item) as f64).max(0.0) as u64;
    cost.saturating_mul(if special { 2 } else { 1 })
}
