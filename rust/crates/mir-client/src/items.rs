//! Client-side item catalogue read from `System.db` (names, icons, stats).

use std::collections::HashMap;
use std::path::Path;

use mir_formats::mirdb::MirDb;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ItemDef {
    pub index: i32,
    pub name: String,
    pub item_type: u8,
    /// Zircon `ItemEffect` (5 pickaxe, 82 fishing rod, 83 fishing robe).
    /// Zircon `ItemEffect` (20 = black iron ore).
    pub effect: u8,
    pub exterior_effect: u8,
    pub rarity: u8,
    pub required_class: u8,
    pub required_type: u8,
    pub required_amount: i32,
    pub shape: i32,
    pub image: i32,
    pub durability: i32,
    pub can_repair: bool,
    pub can_sell: bool,
    pub price: i32,
    pub sell_rate: f64,
    pub weight: i32,
    pub stack_size: i32,
    pub description: String,
    pub stats: Vec<(i32, i32)>,
    /// `ItemInfo.Set`, 0 when the item belongs to no set.
    pub set: i32,
}

/// An equipment set as the client needs it: its name and the stats it
/// pays out once every piece is worn (Zircon `SetInfo`).
#[derive(Debug, Clone)]
pub struct SetDef {
    pub name: String,
    /// `ItemInfo` indices that make up the set, so the tooltip can list them.
    pub items: Vec<i32>,
    /// (stat, amount, required class flags, required level).
    pub stats: Vec<(i32, i32, u8, i32)>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MagicDef {
    pub index: i32,
    pub name: String,
    pub magic: u16,
    pub class: u8,
    pub school: i32,
    pub icon: i32,
    pub base_cost: i32,
    pub level_cost: i32,
    pub need_level: [i32; 3],
    pub experience: [i32; 3],
    pub delay: i32,
    pub description: String,
}

impl MagicDef {
    pub fn cost(&self, level: u8) -> i32 {
        mir_proto::rules::magic_cost(self.base_cost, self.level_cost, level)
    }
}

/// `QuestInfo` as the client shows it.
#[derive(Debug, Clone)]
pub struct QuestDef {
    pub index: i32,
    pub name: String,
    pub progress_text: String,
    pub completed_text: String,
    pub start_npc: i32,
    pub finish_npc: i32,
    pub tasks: Vec<QuestTaskDef>,
    pub rewards: Vec<QuestRewardDef>,
}

#[derive(Debug, Clone)]
pub struct QuestTaskDef {
    pub index: i32,
    /// KillMonster 0, GainItem 1, Region 2.
    pub task: i32,
    pub item: i32,
    pub description: String,
    pub amount: i32,
}

#[derive(Debug, Clone)]
pub struct QuestRewardDef {
    pub index: i32,
    pub item: i32,
    pub amount: i32,
    pub choice: bool,
    pub class: u8,
}

pub struct ItemCatalog {
    items: HashMap<i32, ItemDef>,
    magics: HashMap<u16, MagicDef>,
    quests: HashMap<i32, QuestDef>,
    npc_names: HashMap<i32, String>,
    sets: HashMap<i32, SetDef>,
}

pub fn stat_name(id: i32) -> Option<&'static str> {
    Some(match id {
        2 => "HP",
        3 => "MP",
        4 => "Min AC",
        5 => "Max AC",
        6 => "Min MR",
        7 => "Max MR",
        8 => "Min DC",
        9 => "Max DC",
        10 => "Min MC",
        11 => "Max MC",
        12 => "Min SC",
        13 => "Max SC",
        14 => "Accuracy",
        15 => "Agility",
        16 => "Attack Speed",
        18 => "Strength",
        19 => "Luck",
        20 => "Fire Attack",
        21 => "Fire Resist",
        22 => "Ice Attack",
        23 => "Ice Resist",
        24 => "Lightning Attack",
        25 => "Lightning Resist",
        26 => "Wind Attack",
        27 => "Wind Resist",
        28 => "Holy Attack",
        29 => "Holy Resist",
        30 => "Dark Attack",
        31 => "Dark Resist",
        32 => "Phantom Attack",
        33 => "Phantom Resist",
        34 => "Comfort",
        35 => "Life Steal",
        36 => "Experience Rate",
        37 => "Drop Rate",
        40 => "Pick Up Range",
        54 => "Health %",
        55 => "Critical Chance",
        67 => "MC %",
        73 => "Bag Weight",
        74 => "Wear Weight",
        75 => "Hand Weight",
        76 => "Companion Collection",
        84 => "DC %",
        85 => "SC %",
        87 => "Pet DC %",
        95 => "Recall Set",
        106 => "Base Experience Rate",
        115 => "Critical Damage",
        118 => "Physical Resist",
        122 => "Paralysis Chance",
        123 => "Slow Chance",
        124 => "Silence Chance",
        125 => "Block Chance",
        126 => "Evasion Chance",
        129 => "Poison Resist",
        _ => return None,
    })
}

/// `SetInfo` plus `SetInfoStat`. A set's membership is the reverse of
/// `ItemInfo.Set`, so it is counted from the items rather than stored.
fn load_sets(db: &MirDb, items: &HashMap<i32, ItemDef>) -> HashMap<i32, SetDef> {
    let mut stats: HashMap<i32, Vec<(i32, i32, u8, i32)>> = HashMap::new();
    if let Some(c) = db.collection("SetInfoStat") {
        for r in &c.records {
            stats
                .entry(c.int_or(r, "Set", 0) as i32)
                .or_default()
                .push((
                    c.int_or(r, "Stat", 0) as i32,
                    c.int_or(r, "Amount", 0) as i32,
                    c.int_or(r, "Class", 0) as u8,
                    c.int_or(r, "Level", 0) as i32,
                ));
        }
    }
    let mut members: HashMap<i32, Vec<i32>> = HashMap::new();
    for d in items.values() {
        if d.set != 0 {
            members.entry(d.set).or_default().push(d.index);
        }
    }
    for list in members.values_mut() {
        list.sort_unstable();
    }
    match db.collection("SetInfo") {
        Some(c) => c
            .records
            .iter()
            .map(|r| {
                let index = c.index(r);
                let d = SetDef {
                    name: c.str_or(r, "SetName", "").to_string(),
                    items: members.remove(&index).unwrap_or_default(),
                    stats: stats.remove(&index).unwrap_or_default(),
                };
                (index, d)
            })
            .collect(),
        None => HashMap::new(),
    }
}

pub fn type_name(t: u8) -> &'static str {
    match t {
        1 => "Consumable",
        2 => "Weapon",
        3 => "Armour",
        4 => "Torch",
        5 => "Helmet",
        6 => "Necklace",
        7 => "Bracelet",
        8 => "Ring",
        9 => "Shoes",
        10 => "Poison",
        11 => "Amulet",
        12 => "Meat",
        13 => "Ore",
        14 => "Book",
        15 => "Scroll",
        16 => "Dark Stone",
        27 => "Shield",
        34 => "Currency",
        _ => "Item",
    }
}

impl ItemCatalog {
    pub fn empty() -> ItemCatalog {
        ItemCatalog {
            items: HashMap::new(),
            magics: HashMap::new(),
            quests: HashMap::new(),
            npc_names: HashMap::new(),
            sets: HashMap::new(),
        }
    }

    /// Quest indices in order (the dev quest switch picks the first few).
    pub fn quest_indices(&self) -> Vec<i32> {
        let mut out: Vec<i32> = self.quests.keys().copied().collect();
        out.sort_unstable();
        out
    }

    pub fn quest(&self, index: i32) -> Option<&QuestDef> {
        self.quests.get(&index)
    }

    /// Display name of an NPC (the part after the last underscore).
    pub fn npc_name(&self, index: i32) -> String {
        self.npc_names
            .get(&index)
            .map(|n| n.rsplit('_').next().unwrap_or(n).to_string())
            .unwrap_or_default()
    }

    /// Replace Zircon's `[PLAYERNAME]`, `[STARTNAME]`, `[FINISHNAME]` tags.
    pub fn quest_text(&self, q: &QuestDef, text: &str, player: &str) -> String {
        text.replace("[PLAYERNAME]", player)
            .replace("[STARTNAME]", &self.npc_name(q.start_npc))
            .replace("[FINISHNAME]", &self.npc_name(q.finish_npc))
    }

    pub fn magic(&self, magic: u16) -> Option<&MagicDef> {
        self.magics.get(&magic)
    }

    /// All magics of a class, sorted by the level of their first rank.
    pub fn class_magics(&self, class: u8) -> Vec<&MagicDef> {
        let mut v: Vec<&MagicDef> = self
            .magics
            .values()
            .filter(|m| m.class & (1 << class) != 0 && m.school != 0 && m.school != 20)
            .collect();
        v.sort_by_key(|m| (m.need_level[0], m.magic));
        v
    }

    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<ItemCatalog> {
        let db = MirDb::load(path.as_ref())
            .map_err(|e| anyhow::anyhow!("loading {}: {e}", path.as_ref().display()))?;
        let mut stats: HashMap<i32, Vec<(i32, i32)>> = HashMap::new();
        if let Some(c) = db.collection("ItemInfoStat") {
            for r in &c.records {
                stats
                    .entry(c.int_or(r, "Item", 0) as i32)
                    .or_default()
                    .push((
                        c.int_or(r, "Stat", 0) as i32,
                        c.int_or(r, "Amount", 0) as i32,
                    ));
            }
        }
        let c = db
            .collection("ItemInfo")
            .ok_or_else(|| anyhow::anyhow!("no ItemInfo collection"))?;
        let items = c
            .records
            .iter()
            .map(|r| {
                let index = c.index(r);
                let d = ItemDef {
                    index,
                    name: c.str_or(r, "ItemName", "").to_string(),
                    item_type: c.int_or(r, "ItemType", 0) as u8,
                    effect: c.int_or(r, "ItemEffect", 0) as u8,
                    exterior_effect: c.int_or(r, "ExteriorEffect", 0) as u8,
                    rarity: c.int_or(r, "Rarity", 0) as u8,
                    required_class: c.int_or(r, "RequiredClass", 15) as u8,
                    required_type: c.int_or(r, "RequiredType", 0) as u8,
                    required_amount: c.int_or(r, "RequiredAmount", 0) as i32,
                    shape: c.int_or(r, "Shape", 0) as i32,
                    image: c.int_or(r, "Image", 0) as i32,
                    durability: c.int_or(r, "Durability", 0) as i32,
                    can_repair: c.bool_or(r, "CanRepair", true),
                    can_sell: c.bool_or(r, "CanSell", true),
                    price: c.int_or(r, "Price", 0) as i32,
                    sell_rate: c.float_or(r, "SellRate", 0.5),
                    weight: c.int_or(r, "Weight", 0) as i32,
                    stack_size: if c.int_or(r, "ItemType", 0) as u8
                        == mir_proto::item_type::CONSUMABLE
                    {
                        999
                    } else {
                        c.int_or(r, "StackSize", 1) as i32
                    },
                    description: c.str_or(r, "Description", "").to_string(),
                    stats: stats.remove(&index).unwrap_or_default(),
                    set: c.int_or(r, "Set", 0) as i32,
                };
                (index, d)
            })
            .collect();
        let sets = load_sets(&db, &items);
        let magics = match db.collection("MagicInfo") {
            Some(c) => c
                .records
                .iter()
                .map(|r| {
                    let m = MagicDef {
                        index: c.index(r),
                        name: c.str_or(r, "Name", "").to_string(),
                        magic: c.int_or(r, "Magic", 0) as u16,
                        class: if c.has_property("RequiredClass") {
                            c.int_or(r, "RequiredClass", 15) as u8
                        } else {
                            match c.int_or(r, "Class", 4) {
                                0 => 1,
                                1 => 2,
                                2 => 4,
                                3 => 8,
                                _ => 15,
                            }
                        },
                        school: c.int_or(r, "School", 0) as i32,
                        icon: c.int_or(r, "Icon", 0) as i32,
                        base_cost: c.int_or(r, "BaseCost", 0) as i32,
                        level_cost: c.int_or(r, "LevelCost", 0) as i32,
                        need_level: [
                            c.int_or(r, "NeedLevel1", 0) as i32,
                            c.int_or(r, "NeedLevel2", 0) as i32,
                            c.int_or(r, "NeedLevel3", 0) as i32,
                        ],
                        experience: [
                            c.int_or(r, "Experience1", 0) as i32,
                            c.int_or(r, "Experience2", 0) as i32,
                            c.int_or(r, "Experience3", 0) as i32,
                        ],
                        delay: c.int_or(r, "Delay", 0) as i32,
                        description: c.str_or(r, "Description", "").to_string(),
                    };
                    (m.magic, m)
                })
                .collect(),
            None => HashMap::new(),
        };
        let mut quests: HashMap<i32, QuestDef> = match db.collection("QuestInfo") {
            Some(c) => c
                .records
                .iter()
                .map(|r| {
                    let q = QuestDef {
                        index: c.index(r),
                        name: c.str_or(r, "QuestName", "").to_string(),
                        progress_text: c.str_or(r, "ProgressText", "").to_string(),
                        completed_text: c.str_or(r, "CompletedText", "").to_string(),
                        start_npc: c.int_or(r, "StartNPC", 0) as i32,
                        finish_npc: c.int_or(r, "FinishNPC", 0) as i32,
                        tasks: Vec::new(),
                        rewards: Vec::new(),
                    };
                    (q.index, q)
                })
                .collect(),
            None => HashMap::new(),
        };
        if let Some(c) = db.collection("QuestTask") {
            for r in &c.records {
                if let Some(q) = quests.get_mut(&(c.int_or(r, "Quest", 0) as i32)) {
                    q.tasks.push(QuestTaskDef {
                        index: c.index(r),
                        task: c.int_or(r, "Task", 0) as i32,
                        item: c.int_or(r, "ItemParameter", 0) as i32,
                        description: c.str_or(r, "MobDescription", "").to_string(),
                        amount: c.int_or(r, "Amount", 0) as i32,
                    });
                }
            }
        }
        if let Some(c) = db.collection("QuestReward") {
            for r in &c.records {
                if let Some(q) = quests.get_mut(&(c.int_or(r, "Quest", 0) as i32)) {
                    q.rewards.push(QuestRewardDef {
                        index: c.index(r),
                        item: c.int_or(r, "Item", 0) as i32,
                        amount: c.int_or(r, "Amount", 0) as i32,
                        choice: c.bool_or(r, "Choice", false),
                        class: c.int_or(r, "Class", 15) as u8,
                    });
                }
            }
        }
        let npc_names = match db.collection("NPCInfo") {
            Some(c) => c
                .records
                .iter()
                .map(|r| (c.index(r), c.str_or(r, "NPCName", "").to_string()))
                .collect(),
            None => HashMap::new(),
        };
        Ok(ItemCatalog {
            items,
            magics,
            quests,
            npc_names,
            sets,
        })
    }

    pub fn get(&self, index: i32) -> Option<&ItemDef> {
        self.items.get(&index)
    }

    pub fn set(&self, index: i32) -> Option<&SetDef> {
        self.sets.get(&index)
    }

    pub fn name(&self, index: i32) -> String {
        self.items
            .get(&index)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("Item {index}"))
    }

    /// Every item's index and name, for the fortune window's search.
    pub fn names(&self) -> impl Iterator<Item = (i32, &str)> {
        self.items.iter().map(|(i, d)| (*i, d.name.as_str()))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}
