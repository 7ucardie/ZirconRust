//! Wire protocol shared by the Rust Zircon client and server.
//!
//! Every frame on the TCP stream is `u32 little-endian payload length` followed
//! by a [postcard](https://docs.rs/postcard) encoded [`ClientMessage`] or
//! [`ServerMessage`]. Both ends own this crate, so there is no need to match
//! the C# reflection-ordered packet ids.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 19;
pub const MAX_PLAYER_LEVEL: i32 = 1000;
pub const DEFAULT_PORT: u16 = 7000;
pub const MAX_FRAME_LEN: usize = 1 << 20;
/// Largest frame a client may send (a 4000-character guild notice fits).
pub const CLIENT_MAX_FRAME_LEN: usize = 64 << 10;

/// Eight-way direction, in the same order Zircon's sprite sheets use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Direction {
    Up = 0,
    UpRight = 1,
    Right = 2,
    DownRight = 3,
    Down = 4,
    DownLeft = 5,
    Left = 6,
    UpLeft = 7,
}

impl Direction {
    pub const ALL: [Direction; 8] = [
        Direction::Up,
        Direction::UpRight,
        Direction::Right,
        Direction::DownRight,
        Direction::Down,
        Direction::DownLeft,
        Direction::Left,
        Direction::UpLeft,
    ];

    pub fn from_index(i: u8) -> Direction {
        Direction::ALL[(i & 7) as usize]
    }

    pub fn index(self) -> u8 {
        self as u8
    }

    /// Cell delta for one step in this direction (x right, y down).
    pub fn delta(self) -> (i32, i32) {
        match self {
            Direction::Up => (0, -1),
            Direction::UpRight => (1, -1),
            Direction::Right => (1, 0),
            Direction::DownRight => (1, 1),
            Direction::Down => (0, 1),
            Direction::DownLeft => (-1, 1),
            Direction::Left => (-1, 0),
            Direction::UpLeft => (-1, -1),
        }
    }

    pub fn opposite(self) -> Direction {
        Direction::from_index(self.index().wrapping_add(4))
    }

    pub fn rotate(self, steps: i8) -> Direction {
        Direction::from_index((self.index() as i8).wrapping_add(steps) as u8 & 7)
    }

    /// Direction from `from` toward `to` (Zircon's `Functions.DirectionFromPoint`).
    pub fn from_points(from: Point, to: Point) -> Direction {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        if dx == 0 && dy == 0 {
            return Direction::Down;
        }
        // Angle in screen space; classify into 8 sectors.
        let angle = (dy as f64).atan2(dx as f64); // 0 = right, pi/2 = down
        let sector = ((angle / (std::f64::consts::PI / 4.0)).round() as i32).rem_euclid(8);
        match sector {
            0 => Direction::Right,
            1 => Direction::DownRight,
            2 => Direction::Down,
            3 => Direction::DownLeft,
            4 => Direction::Left,
            5 => Direction::UpLeft,
            6 => Direction::Up,
            _ => Direction::UpRight,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Point {
        Point { x, y }
    }
    pub fn step(self, dir: Direction, distance: i32) -> Point {
        let (dx, dy) = dir.delta();
        Point::new(self.x + dx * distance, self.y + dy * distance)
    }
    /// Chebyshev distance: the number of 8-way steps between two cells.
    pub fn distance(self, other: Point) -> i32 {
        (self.x - other.x).abs().max((self.y - other.y).abs())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Gender {
    Male,
    Female,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Class {
    Warrior,
    Wizard,
    Taoist,
    Assassin,
}

impl Class {
    pub const ALL: [Class; 4] = [
        Class::Warrior,
        Class::Wizard,
        Class::Taoist,
        Class::Assassin,
    ];
    /// Zircon `MirClass` value (BaseStat.Class).
    pub fn mir_class(self) -> u8 {
        match self {
            Class::Warrior => 0,
            Class::Wizard => 1,
            Class::Taoist => 2,
            Class::Assassin => 3,
        }
    }
    /// Zircon `RequiredClass` flag (SafeZoneInfo.StartClass).
    pub fn flag(self) -> u8 {
        1 << self.mir_class()
    }
    pub fn name(self) -> &'static str {
        match self {
            Class::Warrior => "Warrior",
            Class::Wizard => "Wizard",
            Class::Taoist => "Taoist",
            Class::Assassin => "Assassin",
        }
    }
}

/// What an object looks like; enough for the client to pick sprites.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Appearance {
    Player {
        name: String,
        gender: Gender,
        class: Class,
        /// Equipped armour `ItemInfo.Shape` (0 = basic clothes).
        armour: u16,
        /// Equipped weapon `ItemInfo.Shape`, if any.
        weapon: Option<u16>,
        hair: u8,
        /// Zircon `Character.HairColour`, packed ARGB. 0 leaves the sprite
        /// at its painted colour.
        hair_colour: u32,
        /// Dye on the equipped armour (Zircon `UserItem.Colour`), packed ARGB.
        armour_colour: u32,
        /// Equipped helmet `ItemInfo.Shape` (1-based; 0 = none, hair shows).
        helmet: u16,
        /// Equipped shield `ItemInfo.Shape`, if any.
        shield: Option<u16>,
        /// `ItemInfo.ExteriorEffect` from armour, emblem, weapon and shield.
        armour_effect: u8,
        emblem_effect: u8,
        weapon_effect: u8,
        shield_effect: u8,
        /// Name colour: 0 white, 1 yellow (PK points), 2 brown, 3 red.
        name_color: u8,
        /// Guild name and rank shown under the name ("" when none).
        guild: String,
        guild_rank: String,
        /// `horse_type::*` being ridden (0 = on foot) and the horse armour
        /// shape (0 none, 1 iron, 2 silver, 3 gold, 4 blue, 5 dark, 6 royal).
        horse: u8,
        horse_shape: u8,
    },
    Monster {
        name: String,
        /// Zircon `MonsterImage` value; the client maps it to a library + base index.
        image: u16,
        /// Name of the player this pet fights for.
        owner: Option<String>,
        /// The corpse has nothing left for this viewer, so it is drawn as
        /// bones. Zircon decides this per viewer in `GetInfoPacket`: the
        /// killer still sees flesh while the corpse owes them a drop.
        skeleton: bool,
    },
    Npc {
        name: String,
        /// `NPCInfo.Image`; sprite index = image * 100 + frame in NPC.Zl.
        image: u16,
    },
    /// A spell object on a cell (fire wall, cloud); drawn on the floor.
    Spell { effect: u8 },
    /// An item lying on the ground.
    Item {
        /// `ItemInfo.Index`.
        info: i32,
        count: u32,
        enhanced: bool,
    },
}

/// An item instance as the client sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemInstance {
    pub id: u32,
    /// `ItemInfo.Index`.
    pub info: i32,
    pub count: u32,
    pub durability: i32,
    pub max_durability: i32,
    /// Added stats (Zircon `UserItem.AddedStats`): (stat id, amount).
    #[serde(default)]
    pub added: Vec<(i32, i32)>,
    /// Enhancement portion of the aggregate added stats (preserved on reroll).
    pub enhancements: Vec<(i32, i32)>,
    pub refinable: bool,
    #[serde(default)]
    pub level: i32,
    #[serde(default)]
    pub accessory_experience: u64,
    pub non_refinable: bool,
    #[serde(default)]
    pub bound: bool,
    #[serde(default)]
    pub worthless: bool,
    #[serde(default)]
    pub locked: bool,

    /// Unix seconds this item expires at, for the timed goods the store
    /// sells (Zircon `UserItemFlags.Expirable`). `None` never expires.
    #[serde(default)]
    pub expires_at: Option<u64>,
    /// Zircon `UserItem.Colour`: the armour dye, packed ARGB. 0 is undyed.
    #[serde(default)]
    pub colour: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Grid {
    Inventory,
    Equipment,
    /// Account storage (safe zones only).
    Storage,
}

pub const INVENTORY_SIZE: usize = 48;
pub const MAX_INVENTORY_SIZE: usize = 96;
pub const EQUIPMENT_SIZE: usize = 22;
/// Zircon sends the marketplace nine rows at a time.
pub const MARKET_PAGE: usize = 9;

/// Zircon `EquipmentSlot`.
pub mod slot {
    pub const WEAPON: usize = 0;
    pub const ARMOUR: usize = 1;
    pub const HELMET: usize = 2;
    pub const TORCH: usize = 3;
    pub const NECKLACE: usize = 4;
    pub const BRACELET_L: usize = 5;
    pub const BRACELET_R: usize = 6;
    pub const RING_L: usize = 7;
    pub const RING_R: usize = 8;
    pub const SHOES: usize = 9;
    pub const POISON: usize = 10;
    pub const AMULET: usize = 11;
    pub const FLOWER: usize = 12;
    pub const HORSE_ARMOUR: usize = 13;
    pub const EMBLEM: usize = 14;
    pub const SHIELD: usize = 15;
    pub const HOOK: usize = 17;
    pub const FLOAT: usize = 18;
    pub const BAIT: usize = 19;
    pub const FINDER: usize = 20;
    pub const REEL: usize = 21;
}

/// Zircon `ItemType` values.
pub mod item_type {
    pub const NOTHING: u8 = 0;
    pub const CONSUMABLE: u8 = 1;
    pub const WEAPON: u8 = 2;
    pub const ARMOUR: u8 = 3;
    pub const TORCH: u8 = 4;
    pub const HELMET: u8 = 5;
    pub const NECKLACE: u8 = 6;
    pub const BRACELET: u8 = 7;
    pub const RING: u8 = 8;
    pub const SHOES: u8 = 9;
    pub const POISON: u8 = 10;
    pub const AMULET: u8 = 11;
    pub const MEAT: u8 = 12;
    pub const ORE: u8 = 13;
    pub const BOOK: u8 = 14;
    pub const SCROLL: u8 = 15;
    pub const DARK_STONE: u8 = 16;
    pub const HORSE_ARMOUR: u8 = 18;
    pub const FLOWER: u8 = 19;
    pub const COMPANION_FOOD: u8 = 20;
    pub const COMPANION_BAG: u8 = 21;
    pub const SHIELD: u8 = 27;
    pub const HOOK: u8 = 29;
    pub const FLOAT: u8 = 30;
    pub const BAIT: u8 = 31;
    pub const FINDER: u8 = 32;
    pub const REEL: u8 = 33;
    /// Zircon `ItemType.System`: the gender/hair/dye/name change items and
    /// the fortune checker.
    pub const SYSTEM: u8 = 24;
    pub const EMBLEM: u8 = 26;
    pub const CURRENCY: u8 = 34;

    /// Equipment slots an item type may occupy (Zircon `Functions.CorrectSlot`).
    pub fn slots(item_type: u8) -> &'static [usize] {
        match item_type {
            WEAPON => &[super::slot::WEAPON],
            ARMOUR => &[super::slot::ARMOUR],
            HELMET => &[super::slot::HELMET],
            TORCH => &[super::slot::TORCH],
            NECKLACE => &[super::slot::NECKLACE],
            BRACELET => &[super::slot::BRACELET_L, super::slot::BRACELET_R],
            RING => &[super::slot::RING_L, super::slot::RING_R],
            SHOES => &[super::slot::SHOES],
            POISON => &[super::slot::POISON],
            AMULET | DARK_STONE => &[super::slot::AMULET],
            SHIELD => &[super::slot::SHIELD],
            EMBLEM => &[super::slot::EMBLEM],
            HORSE_ARMOUR => &[super::slot::HORSE_ARMOUR],
            FLOWER => &[super::slot::FLOWER],
            HOOK => &[super::slot::HOOK],
            FLOAT => &[super::slot::FLOAT],
            BAIT => &[super::slot::BAIT],
            FINDER => &[super::slot::FINDER],
            REEL => &[super::slot::REEL],
            _ => &[],
        }
    }
}

/// A learned skill as the client sees it (`MagicInfo` comes from System.db).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MagicSummary {
    /// Zircon `MagicType` value.
    pub magic: u16,
    pub level: u8,
    pub experience: u64,
    /// Hotkey 1..=12 (F1..F12), 0 = none.
    pub key: u8,
}

/// Zircon `Globals.MaxBeltCount`: belt slots 0..=9 (keys 1..9, 0).
pub const MAX_BELT: usize = 10;

/// One belt slot (Zircon `ClientBeltLink`): links a stackable/consumable by
/// `ItemInfo` index, or a specific item by its id. Never both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeltLink {
    pub slot: u8,
    pub info: Option<i32>,
    pub item: Option<u32>,
}

impl BeltLink {
    pub fn empty(slot: u8) -> BeltLink {
        BeltLink {
            slot,
            info: None,
            item: None,
        }
    }
}

pub const MAGIC_RANGE: i32 = 10;
pub const MAGIC_DELAY: u64 = 2000;
pub const CAST_TIME: u64 = 600;

/// A shop entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Good {
    pub info: i32,
    pub price: u64,
}

/// A quest an NPC can start or finish for the player (Zircon quest lines
/// in the dialog). `state`: 0 available, 1 in progress, 2 ready to complete.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpcQuest {
    pub quest: i32,
    pub name: String,
    pub state: u8,
}

/// Zircon `ClientUserQuest`: one entry of the quest log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserQuestSummary {
    pub quest: i32,
    pub track: bool,
    pub completed: bool,
    pub selected_reward: i32,
    /// `(QuestTask index, amount)`.
    pub tasks: Vec<(i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weights {
    pub bag: i32,
    pub max_bag: i32,
    pub wear: i32,
    pub max_wear: i32,
    pub hand: i32,
    pub max_hand: i32,
}

/// Zircon `AttackMode`.
pub mod attack_mode {
    pub const PEACE: u8 = 0;
    pub const GROUP: u8 = 1;
    pub const GUILD: u8 = 2;
    pub const WAR_RED_BROWN: u8 = 3;
    pub const ALL: u8 = 4;
}

/// Zircon `PetMode`.
pub mod pet_mode {
    pub const BOTH: u8 = 0;
    pub const MOVE: u8 = 1;
    pub const ATTACK: u8 = 2;
    pub const PVP: u8 = 3;
    pub const NONE: u8 = 4;
}

/// Zircon `HorseType`.
pub mod horse_type {
    pub const NONE: u8 = 0;
    pub const BROWN: u8 = 1;
    pub const WHITE: u8 = 2;
    pub const RED: u8 = 3;
    pub const BLACK: u8 = 4;
    pub const WHITE_UNICORN: u8 = 5;
    pub const RED_UNICORN: u8 = 6;
}

/// Zircon `OnlineState`: what a player advertises to the friends who
/// have them on their list.
pub mod online_state {
    pub const ONLINE: u8 = 0;
    pub const BUSY: u8 = 1;
    pub const AWAY: u8 = 2;
    pub const OFFLINE: u8 = 3;
    /// Zircon lists them in declaration order, and the friend list sorts by it.
    pub const ALL: [u8; 4] = [ONLINE, BUSY, AWAY, OFFLINE];

    pub fn name(state: u8) -> &'static str {
        match state {
            ONLINE => "Online",
            BUSY => "Busy",
            AWAY => "Away",
            _ => "Offline",
        }
    }
}

/// One friend as the owner sees them (Zircon `ClientFriendInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendSummary {
    pub index: u32,
    pub name: String,
    /// `online_state::*`; a character who is not logged in is `OFFLINE`.
    pub state: u8,
}

/// One blocked name (Zircon `ClientBlockInfo`). Zircon blocks the whole
/// account behind the name, so every character on it is blocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockSummary {
    pub index: u32,
    pub name: String,
}

/// Zircon `GuildPermission` bits (`LEADER` has every permission).
pub mod guild_permission {
    pub const NONE: i32 = 0;
    pub const LEADER: i32 = -1;
    pub const EDIT_NOTICE: i32 = 1;
    pub const ADD_MEMBER: i32 = 2;
    pub const REMOVE_MEMBER: i32 = 4;
    pub const STORAGE: i32 = 8;
    pub const START_WAR: i32 = 128;
}

/// A guild as its members see it (Zircon `ClientGuildInfo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuildSummary {
    pub name: String,
    pub notice: String,
    pub member_limit: i32,
    pub funds: i64,
    pub tax: i32,
    pub default_rank: String,
    pub default_permission: i32,
    pub user_index: u32,
    pub members: Vec<GuildMemberSummary>,
    /// Name of the castle the guild owns ("" when none).
    pub castle: String,
    /// Guilds at war with this one and the seconds left in each war.
    pub wars: Vec<(String, u64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuildMemberSummary {
    pub index: u32,
    pub name: String,
    pub rank: String,
    pub permission: i32,
    pub online: bool,
}

/// Zircon `RefineType`.
pub mod refine_type {
    pub const NONE: u8 = 0;
    pub const DURABILITY: u8 = 1;
    pub const DC: u8 = 2;
    pub const SPELL_POWER: u8 = 3;
    pub const FIRE: u8 = 4;
    pub const ICE: u8 = 5;
    pub const LIGHTNING: u8 = 6;
    pub const WIND: u8 = 7;
    pub const HOLY: u8 = 8;
    pub const DARK: u8 = 9;
    pub const PHANTOM: u8 = 10;
    pub const HEALTH: u8 = 12;
    pub const MANA: u8 = 13;
    pub const AC: u8 = 14;
    pub const MR: u8 = 15;
    pub const ACCURACY: u8 = 16;
    pub const AGILITY: u8 = 17;
    pub const DC_PERCENT: u8 = 18;
    pub const SP_PERCENT: u8 = 19;
    pub const HEALTH_PERCENT: u8 = 20;
    pub const MANA_PERCENT: u8 = 21;
}

/// Zircon `RefineQuality` (wait: 1 min, 30 min, 1 h, 6 h, 1 day).
pub mod refine_quality {
    pub const RUSH: u8 = 0;
    pub const QUICK: u8 = 1;
    pub const STANDARD: u8 = 2;
    pub const CAREFUL: u8 = 3;
    pub const PRECISE: u8 = 4;
}

/// A weapon in the furnace (Zircon `ClientRefineInfo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefineSummary {
    pub index: u32,
    pub weapon: ItemInstance,
    pub refine_type: u8,
    pub quality: u8,
    pub chance: i32,
    pub max_chance: i32,
    /// Milliseconds until it can be collected (0 = ready).
    pub ready_in_ms: u64,
}

/// A companion for sale at a CompanionManage page (Zircon `CompanionInfo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompanionOffer {
    pub index: i32,
    pub name: String,
    pub description: String,
    pub price: i32,
    pub currency: String,
    /// Available to this player (always, or unlocked with the item).
    pub unlocked: bool,
    pub unlock_item: i32,
}

/// One of the player's companions (Zircon `ClientUserCompanion`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompanionSummary {
    pub index: u32,
    /// Monster name of the companion's look.
    pub kind: String,
    pub name: String,
    pub level: i32,
    pub experience: i32,
    pub max_experience: i32,
    pub hunger: i32,
    pub active: bool,
    pub items: Vec<ItemInstance>,
    pub bag_size: u32,
    pub bag_weight: i32,
    pub max_weight: i32,
    /// Food stack equipped in Zircon `CompanionSlot.Food`.
    pub food: Option<ItemInstance>,
    /// Equipped bag increases companion capacity.
    pub bag: Option<ItemInstance>,
    /// Earned (level, stat type, amount) rolls from `CompanionSkillInfo`.
    pub bonuses: Vec<(i32, i32, i32)>,
    /// Session-only reroll confirmation flags for levels 3/5/7/10/11/13/15.
    pub reroll_enabled: u8,
}

/// One of the player's currencies (Zircon `ClientUserCurrency`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrencySummary {
    pub name: String,
    pub abbreviation: String,
    pub amount: i64,
}

/// One row of the ranking board (Zircon `RankInfo`).
///
/// `rank` counts every character passing the class filter, so it does not
/// change when "online only" hides rows -- that is Zircon's own rule.
/// `change` is how many places the character has gained since the last
/// reset: positive is a climb, negative a fall, zero a hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankEntry {
    pub rank: u32,
    /// Character id, so the client can mark the player's own row.
    pub character: u32,
    pub name: String,
    pub class: Class,
    pub level: i32,
    pub experience: u64,
    /// Experience needed for the next level (0 at the cap).
    pub max_experience: u64,
    pub online: bool,
    pub rebirth: i32,
    pub change: i32,
}

/// Public character details shown by Zircon's ranking inspection panel.
/// Only equipped items leave the server; inventory and storage stay private.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankInspection {
    pub character: u32,
    pub name: String,
    pub class: Class,
    pub gender: Gender,
    pub level: i32,
    pub hair: u8,
    pub hair_colour: u32,
    pub partner: String,
    pub fame: i32,
    pub guild_name: String,
    pub guild_rank: String,
    pub equipment: Vec<(u8, ItemInstance)>,
}

/// A mail as the client sees it (Zircon `ClientMailInfo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailSummary {
    pub index: u32,
    pub opened: bool,
    /// Unix seconds.
    pub date: u64,
    pub sender: String,
    pub subject: String,
    pub message: String,
    pub gold: u64,
    pub items: Vec<ItemInstance>,
}

/// A marketplace listing as the client sees it (Zircon
/// `ClientMarketPlaceInfo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketListing {
    pub index: u32,
    pub item: ItemInstance,
    /// Price per unit.
    pub price: u64,
    pub seller: String,
    pub message: String,
    /// True when this account listed it, so the client offers Cancel.
    pub is_owner: bool,
}

/// One row of the in-game store (Zircon `StoreInfo` as the Store tab shows
/// it). The client is told both prices so it can label the row the way
/// Zircon does, and the tag list so it can build the filter drop-down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreEntry {
    pub index: u32,
    /// `ItemInfo.Index` of what is on sale.
    pub item: i32,
    /// How many of it one purchase yields at most (its stack size).
    pub stack_size: u32,
    /// Game-gold price per unit.
    pub price: u64,
    /// Hunt-gold price per unit. 0 means hunt gold costs `price` too,
    /// which is Zircon's rule, not an absence of a hunt-gold option.
    pub hunt_gold_price: u64,
    pub filters: Vec<String>,
    pub available: bool,
    /// Lifetime in seconds; 0 never expires.
    pub duration: u32,
}

/// Zircon `MarketPlaceStoreSort`. Zircon's fourth entry, Favourite, is a
/// `TODO` in its own client and sorts nothing, so it is left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoreSort {
    Alphabetical,
    HighestPrice,
    LowestPrice,
}

impl StoreSort {
    pub const ALL: [(StoreSort, &'static str); 3] = [
        (StoreSort::Alphabetical, "A-Z"),
        (StoreSort::HighestPrice, "Highest Price"),
        (StoreSort::LowestPrice, "Lowest Price"),
    ];
}

/// Zircon `MarketPlaceSort`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketSort {
    Newest,
    Oldest,
    HighestPrice,
    LowestPrice,
}

impl MarketSort {
    /// The sorts in the order Zircon lists them, with their labels.
    pub const ALL: [(MarketSort, &'static str); 4] = [
        (MarketSort::Newest, "Newest"),
        (MarketSort::Oldest, "Oldest"),
        (MarketSort::HighestPrice, "Highest Price"),
        (MarketSort::LowestPrice, "Lowest Price"),
    ];
}

/// What the fortune checker knows about one item (Zircon
/// `ClientFortuneInfo` built from `UserFortuneInfo`).
///
/// `progress` is the expected number of this item the account should have
/// received by now, accumulated a fraction at a time on every drop roll;
/// `drop_count` is how many actually dropped. Zircon shows the gap between
/// them as "to go", and forces a drop once the expectation runs a whole
/// item ahead of reality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FortuneSummary {
    pub item: i32,
    pub drop_count: i64,
    pub progress: f64,
    /// Unix seconds when the account last spent a checker on this item.
    pub checked_at: u64,
}

/// Zircon `MessageType` (chat line colour and routing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatKind {
    Normal,
    Shout,
    WhisperIn,
    WhisperOut,
    Group,
    Global,
    System,
    Guild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Standing,
    Walking,
    Running,
    Attack,
    /// Zircon `Combat4` (Half Moon and other backhand swings).
    Attack2,
    /// Zircon `Combat1`: projectile spell cast.
    Cast1,
    /// Zircon `Combat2`: targeted spell cast.
    Cast2,
    /// Zircon `Combat5`: Dragon Rise / one-handed lotus swing.
    Attack5,
    /// Zircon `Combat6`: Blade Storm.
    Attack6,
    /// Zircon `Combat8`: Shoulder Dash step.
    Dash,
    /// Zircon `Combat15`: self-buff stance cast.
    Stance,
    Struck,
    Die,
    Dead,
    /// Zircon `MirAnimation.FishingCast/Wait/Reel`.
    FishingCast,
    FishingWait,
    FishingReel,
    /// Zircon `MirAction.Mining` (the weapon swing).
    Mining,
    /// Zircon `MirAnimation.Harvest`: the butchering swing over a corpse.
    Harvest,
}

/// Zircon `FishingState`.
pub mod fishing_state {
    pub const NONE: u8 = 0;
    pub const CAST: u8 = 1;
    pub const REEL: u8 = 2;
    pub const CANCEL: u8 = 3;
}

/// Zircon `Element`.
pub mod element {
    pub const NONE: u8 = 0;
    pub const FIRE: u8 = 1;
    pub const ICE: u8 = 2;
    pub const LIGHTNING: u8 = 3;
    pub const WIND: u8 = 4;
    pub const HOLY: u8 = 5;
    pub const DARK: u8 = 6;
    pub const PHANTOM: u8 = 7;
}

/// Zircon `MagicType` values used by the prototype.
pub mod magic_type {
    pub const SWORDSMANSHIP: u16 = 100;
    pub const POTION_MASTERY: u16 = 101;
    pub const SLAYING: u16 = 102;
    pub const THRUSTING: u16 = 103;
    pub const HALF_MOON: u16 = 104;
    pub const SHOULDER_DASH: u16 = 105;
    pub const FLAMING_SWORD: u16 = 106;
    pub const DRAGON_RISE: u16 = 107;
    pub const BLADE_STORM: u16 = 108;
    pub const DESTRUCTIVE_SURGE: u16 = 109;
    pub const INTERCHANGE: u16 = 110;
    pub const DEFIANCE: u16 = 111;
    pub const BECKON: u16 = 112;
    pub const MIGHT: u16 = 113;
    pub const SWIFT_BLADE: u16 = 114;
    pub const ASSAULT: u16 = 115;
    pub const ENDURANCE: u16 = 116;
    pub const REFLECT_DAMAGE: u16 = 117;
    pub const FETTER: u16 = 118;
    pub const MASS_BECKON: u16 = 123;
    pub const FIRE_BALL: u16 = 201;
    pub const LIGHTNING_BALL: u16 = 202;
    pub const ICE_BOLT: u16 = 203;
    pub const GUST_BLAST: u16 = 204;
    pub const REPULSION: u16 = 205;
    pub const ELECTRIC_SHOCK: u16 = 206;
    pub const TELEPORTATION: u16 = 207;
    pub const ADAMANTINE_FIRE_BALL: u16 = 208;
    pub const THUNDER_BOLT: u16 = 209;
    pub const ICE_BLADES: u16 = 210;
    pub const CYCLONE: u16 = 211;
    pub const SCORCHED_EARTH: u16 = 212;
    pub const LIGHTNING_BEAM: u16 = 213;
    pub const FROZEN_EARTH: u16 = 214;
    pub const BLOW_EARTH: u16 = 215;
    pub const FIRE_WALL: u16 = 216;
    pub const EXPEL_UNDEAD: u16 = 217;
    pub const GEO_MANIPULATION: u16 = 218;
    pub const MAGIC_SHIELD: u16 = 219;
    pub const FIRE_STORM: u16 = 220;
    pub const LIGHTNING_WAVE: u16 = 221;
    pub const ICE_STORM: u16 = 222;
    pub const DRAGON_TORNADO: u16 = 223;
    pub const GREATER_FROZEN_EARTH: u16 = 224;
    pub const CHAIN_LIGHTNING: u16 = 225;
    pub const METEOR_SHOWER: u16 = 226;
    pub const RENOUNCE: u16 = 227;
    pub const TEMPEST: u16 = 228;
    pub const HEAL: u16 = 300;
    pub const SPIRIT_SWORD: u16 = 301;
    pub const POISON_DUST: u16 = 302;
    pub const EXPLOSIVE_TALISMAN: u16 = 303;
    pub const EVIL_SLAYER: u16 = 304;
    pub const INVISIBILITY: u16 = 305;
    pub const MAGIC_RESISTANCE: u16 = 306;
    pub const MASS_INVISIBILITY: u16 = 307;
    pub const GREATER_EVIL_SLAYER: u16 = 308;
    pub const RESILIENCE: u16 = 309;
    pub const TRAP_OCTAGON: u16 = 310;
    pub const COMBAT_KICK: u16 = 311;
    pub const ELEMENTAL_SUPERIORITY: u16 = 312;
    pub const MASS_HEAL: u16 = 313;
    pub const BLOOD_LUST: u16 = 314;
    pub const RESURRECTION: u16 = 315;
    pub const PURIFICATION: u16 = 316;
    pub const TRANSPARENCY: u16 = 317;
    pub const CELESTIAL_LIGHT: u16 = 318;
    pub const SUMMON_SKELETON: u16 = 332;
    pub const SUMMON_SHINSU: u16 = 333;
    pub const SUMMON_JIN_SKELETON: u16 = 334;
    pub const STRENGTH_OF_FAITH: u16 = 335;
    pub const WILLOW_DANCE: u16 = 401;
    pub const VINE_TREE_DANCE: u16 = 402;
    pub const DISCIPLINE: u16 = 403;
    pub const POISONOUS_CLOUD: u16 = 404;
    pub const FULL_BLOOM: u16 = 405;
    pub const CLOAK: u16 = 406;
    pub const WHITE_LOTUS: u16 = 407;
    pub const CALAMITY_OF_FULL_MOON: u16 = 408;
    pub const WRAITH_GRIP: u16 = 409;
    pub const RED_LOTUS: u16 = 410;
    pub const HELL_FIRE: u16 = 411;
    pub const PLEDGE_OF_BLOOD: u16 = 412;
    pub const RAKE: u16 = 413;
    pub const SWEET_BRIER: u16 = 414;
    pub const SUMMON_PUPPET: u16 = 415;
    pub const KARMA: u16 = 416;
    pub const TOUCH_OF_THE_DEPARTED: u16 = 417;
    pub const WANING_MOON: u16 = 418;
    pub const GHOST_WALK: u16 = 419;
    pub const ELEMENTAL_PUPPET: u16 = 420;
    pub const REJUVENATION: u16 = 421;
    pub const RESOLUTION: u16 = 422;
    pub const RELEASE: u16 = 424;
    pub const FLAME_SPLASH: u16 = 425;
    pub const BLOODY_FLOWER: u16 = 426;
    // Wave four.
    pub const AUGMENT_DESTRUCTIVE_SURGE: u16 = 119;
    pub const AUGMENT_DEFIANCE: u16 = 120;
    pub const AUGMENT_REFLECT_DAMAGE: u16 = 121;
    pub const ADVANCED_POTION_MASTERY: u16 = 122;
    pub const SEISMIC_SLAM: u16 = 124;
    pub const INVINCIBILITY: u16 = 125;
    pub const CRUSHING_WAVE: u16 = 126;
    pub const DEFENSIVE_MASTERY: u16 = 127;
    pub const PHYSICAL_IMMUNITY: u16 = 128;
    pub const MAGIC_IMMUNITY: u16 = 129;
    pub const DEFENSIVE_BLOW: u16 = 130;
    pub const ELEMENTAL_SWORDS: u16 = 131;
    pub const SHURIKEN: u16 = 132;
    pub const HUNDRED_FIST: u16 = 133;
    pub const OFFENSIVE_BLOW: u16 = 134;
    pub const TAECHEON_SWORD: u16 = 135;
    pub const FIRE_SWORD: u16 = 136;
    pub const JUDGEMENT_OF_HEAVEN: u16 = 229;
    pub const THUNDER_STRIKE: u16 = 230;
    pub const FIRE_BOUNCE: u16 = 231;
    pub const ELEMENTAL_HURRICANE: u16 = 232;
    pub const SUPERIOR_MAGIC_SHIELD: u16 = 233;
    pub const BURNING: u16 = 234;
    pub const SHOCKED: u16 = 235;
    pub const LIGHTNING_STRIKE: u16 = 236;
    pub const MIRROR_IMAGE: u16 = 237;
    pub const ICE_RAIN: u16 = 238;
    pub const FROST_BITE: u16 = 239;
    pub const ASTEROID: u16 = 240;
    pub const TORNADO: u16 = 242;
    pub const ICE_AURA: u16 = 243;
    pub const ICE_DRAGON: u16 = 244;
    pub const ICE_BREAKER: u16 = 245;
    pub const FROZEN_DRAGON: u16 = 246;
    pub const EMPOWERED_HEALING: u16 = 319;
    pub const LIFE_STEAL: u16 = 320;
    pub const IMPROVED_EXPLOSIVE_TALISMAN: u16 = 321;
    pub const AUGMENT_POISON_DUST: u16 = 322;
    pub const CURSED_DOLL: u16 = 323;
    pub const THUNDER_KICK: u16 = 324;
    pub const SOUL_RESONANCE: u16 = 325;
    pub const PARASITE: u16 = 326;
    pub const SPIRITUALISM: u16 = 327;
    pub const AUGMENT_EXPLOSIVE_TALISMAN: u16 = 328;
    pub const AUGMENT_EVIL_SLAYER: u16 = 329;
    pub const AUGMENT_PURIFICATION: u16 = 330;
    pub const AUGMENT_RESURRECTION: u16 = 331;
    pub const SUMMON_DEMONIC_CREATURE: u16 = 336;
    pub const DEMON_EXPLOSION: u16 = 337;
    pub const INFECTION: u16 = 338;
    pub const DEMONIC_RECOVERY: u16 = 339;
    pub const NEUTRALIZE: u16 = 340;
    pub const AUGMENT_NEUTRALIZE: u16 = 341;
    pub const DARK_SOUL_PRISON: u16 = 342;
    pub const SEARING_LIGHT: u16 = 343;
    pub const AUGMENT_CELESTIAL_LIGHT: u16 = 344;
    pub const CORPSE_EXPLODER: u16 = 345;
    pub const SUMMON_DEAD: u16 = 346;
    pub const BINDING_TALISMAN: u16 = 347;
    pub const BRAIN_STORM: u16 = 348;
    pub const HEAVENLY_SKY: u16 = 349;
    pub const POISON_CLOUD: u16 = 350;
    pub const THE_NEW_BEGINNING: u16 = 427;
    pub const DANCE_OF_SWALLOW: u16 = 428;
    pub const DARK_CONVERSION: u16 = 429;
    pub const DRAGON_REPULSE: u16 = 430;
    pub const ADVENT_OF_DEMON: u16 = 431;
    pub const ADVENT_OF_DEVIL: u16 = 432;
    pub const ABYSS: u16 = 433;
    pub const FLASH_OF_LIGHT: u16 = 434;
    pub const STEALTH: u16 = 435;
    pub const EVASION: u16 = 436;
    pub const RAGING_WIND: u16 = 437;
    pub const MASSACRE: u16 = 439;
    pub const ART_OF_SHADOWS: u16 = 440;
    pub const DRAGON_BLOOD: u16 = 441;
    pub const FATAL_BLOW: u16 = 442;
    pub const LAST_STAND: u16 = 443;
    pub const MAGIC_COMBUSTION: u16 = 444;
    pub const VITALITY: u16 = 445;
    pub const CHAIN: u16 = 446;
    pub const CONCENTRATION: u16 = 447;
    pub const DUAL_WEAPON_SKILLS: u16 = 448;
    pub const CONTAINMENT: u16 = 449;
    pub const DRAGON_WAVE: u16 = 450;
    pub const HEMORRHAGE: u16 = 451;
    pub const BURNING_FIRE: u16 = 452;
    pub const CHAIN_OF_FIRE: u16 = 453;
    pub const FOUR_WHEELS: u16 = 456;
    pub const CRESCENT_MOON: u16 = 457;
    // Monster-only spells (Zircon 501+; 550+ are this port's own tags).
    pub const MONSTER_SCORCHED_EARTH: u16 = 501;
    pub const MONSTER_ICE_STORM: u16 = 502;
    pub const MONSTER_DEATH_CLOUD: u16 = 503;
    pub const MONSTER_THUNDER_STORM: u16 = 504;
    pub const SAMA_GUARDIAN_FIRE: u16 = 505;
    pub const SAMA_GUARDIAN_ICE: u16 = 506;
    pub const SAMA_GUARDIAN_LIGHTNING: u16 = 507;
    pub const SAMA_GUARDIAN_WIND: u16 = 508;
    pub const SAMA_PHOENIX_FIRE: u16 = 509;
    pub const SAMA_BLACK_ICE: u16 = 510;
    pub const SAMA_BLUE_LIGHTNING: u16 = 511;
    pub const SAMA_WHITE_WIND: u16 = 512;
    pub const SAMA_PROPHET_FIRE: u16 = 513;
    pub const SAMA_PROPHET_LIGHTNING: u16 = 514;
    pub const SAMA_PROPHET_WIND: u16 = 515;
    pub const PINK_FIRE_BALL: u16 = 530;
    pub const GREEN_SLUDGE_BALL: u16 = 540;
    pub const MONSTER_SPLASH: u16 = 550;
    pub const MONSTER_DARK_BEAM: u16 = 551;
    pub const DOOM_CLAW_LEFT_PINCH: u16 = 520;
    pub const DOOM_CLAW_LEFT_SWIPE: u16 = 521;
    pub const DOOM_CLAW_RIGHT_PINCH: u16 = 522;
    pub const DOOM_CLAW_RIGHT_SWIPE: u16 = 523;
    pub const DOOM_CLAW_WAVE: u16 = 524;
    pub const DOOM_CLAW_SPIT: u16 = 525;
    pub const FLAMING_DAGGERS: u16 = 454;
    pub const SHREDDING: u16 = 455;

    /// Skills that are never cast: they act on every melee swing or as stats.
    pub fn is_passive(m: u16) -> bool {
        matches!(
            m,
            SWORDSMANSHIP
                | DRAGON_BLOOD
                | POTION_MASTERY
                | SLAYING
                | SPIRIT_SWORD
                | WILLOW_DANCE
                | VINE_TREE_DANCE
                | DISCIPLINE
                | BLOODY_FLOWER
                | ASSAULT
                | PLEDGE_OF_BLOOD
                | TOUCH_OF_THE_DEPARTED
                | GHOST_WALK
                | ELEMENTAL_PUPPET
                | REJUVENATION
                | RESOLUTION
                | RELEASE
                | CALAMITY_OF_FULL_MOON
                | WANING_MOON
                | DEFENSIVE_MASTERY
                | PHYSICAL_IMMUNITY
                | MAGIC_IMMUNITY
                | ADVENT_OF_DEMON
                | ADVENT_OF_DEVIL
                | VITALITY
                | LAST_STAND
                | FATAL_BLOW
                | DUAL_WEAPON_SKILLS
                | MASSACRE
                | AUGMENT_DESTRUCTIVE_SURGE
                | AUGMENT_DEFIANCE
                | AUGMENT_REFLECT_DAMAGE
                | ADVANCED_POTION_MASTERY
                | STEALTH
                | ART_OF_SHADOWS
                | DRAGON_WAVE
                | EMPOWERED_HEALING
                | AUGMENT_POISON_DUST
                | AUGMENT_EXPLOSIVE_TALISMAN
                | AUGMENT_EVIL_SLAYER
                | AUGMENT_PURIFICATION
                | AUGMENT_RESURRECTION
                | AUGMENT_CELESTIAL_LIGHT
                | AUGMENT_NEUTRALIZE
                | INFECTION
                | BURNING
                | SHOCKED
                | MAGIC_COMBUSTION
                | CHAIN_OF_FIRE
        )
    }
    /// Wave-four directional swings: the mouse picks the facing.
    pub fn is_directional(m: u16) -> bool {
        matches!(
            m,
            SEISMIC_SLAM
                | FLASH_OF_LIGHT
                | CRUSHING_WAVE
                | ICE_AURA
                | THUNDER_KICK
                | ELEMENTAL_HURRICANE
        )
    }
    /// Stance skills switched with a hotkey and applied on melee swings.
    pub fn is_toggle(m: u16) -> bool {
        matches!(
            m,
            THRUSTING | HALF_MOON | DESTRUCTIVE_SURGE | FLAME_SPLASH | DEMONIC_RECOVERY
        )
    }
    /// Warrior power attacks charged with a hotkey for 12 s (Zircon `Toggle`).
    pub fn is_charge(m: u16) -> bool {
        matches!(
            m,
            FLAMING_SWORD | DRAGON_RISE | BLADE_STORM | DEFENSIVE_BLOW | OFFENSIVE_BLOW
        )
    }
    /// Assassin lotus combo: the hotkey arms the next swing.
    pub fn is_lotus(m: u16) -> bool {
        matches!(m, FULL_BLOOM | WHITE_LOTUS | RED_LOTUS)
    }
    /// Skills armed with a hotkey for the next swing (lotus chain, Sweetbrier, Karma).
    pub fn is_armed(m: u16) -> bool {
        is_lotus(m) || matches!(m, SWEET_BRIER | KARMA)
    }
    /// Moon charges that arm themselves while swinging.
    pub fn is_auto_charge(m: u16) -> bool {
        matches!(m, CALAMITY_OF_FULL_MOON | WANING_MOON | DRAGON_BLOOD)
    }
    /// Spells with the projectile cast animation (Zircon `Combat1`).
    pub fn is_projectile_cast(m: u16) -> bool {
        matches!(
            m,
            FIRE_BALL
                | ICE_DRAGON
                | SEARING_LIGHT
                | HEMORRHAGE
                | PARASITE
                | NEUTRALIZE
                | FIRE_BOUNCE
                | LIGHTNING_STRIKE
                | BINDING_TALISMAN
                | BRAIN_STORM
                | IMPROVED_EXPLOSIVE_TALISMAN
                | ICE_BOLT
                | FLAMING_DAGGERS
                | SHREDDING
                | LIGHTNING_BALL
                | GUST_BLAST
                | ADAMANTINE_FIRE_BALL
                | ICE_BLADES
                | EXPLOSIVE_TALISMAN
                | EVIL_SLAYER
                | GREATER_EVIL_SLAYER
                | MAGIC_RESISTANCE
                | RESILIENCE
                | BECKON
                | MASS_BECKON
                | METEOR_SHOWER
                | GREATER_FROZEN_EARTH
        )
    }
    /// Spells that fly at or strike a chosen monster.
    pub fn needs_target(m: u16) -> bool {
        matches!(
            m,
            FIRE_BALL
                | CURSED_DOLL
                | SOUL_RESONANCE
                | CHAIN
                | CORPSE_EXPLODER
                | SUMMON_DEAD
                | ICE_BOLT
                | THUNDER_BOLT
                | POISON_DUST
                | FLAMING_DAGGERS
                | SHREDDING
                | LIGHTNING_BALL
                | GUST_BLAST
                | ADAMANTINE_FIRE_BALL
                | ICE_BLADES
                | CYCLONE
                | EXPLOSIVE_TALISMAN
                | EVIL_SLAYER
                | GREATER_EVIL_SLAYER
                | INTERCHANGE
                | BECKON
                | EXPEL_UNDEAD
                | CHAIN_LIGHTNING
                | ELECTRIC_SHOCK
                | PURIFICATION
                | WRAITH_GRIP
                | HELL_FIRE
                | ICE_DRAGON
                | SEARING_LIGHT
                | HEMORRHAGE
                | ABYSS
                | NEUTRALIZE
                | PARASITE
                | FIRE_BOUNCE
                | LIGHTNING_STRIKE
                | DANCE_OF_SWALLOW
                | HUNDRED_FIST
                | BINDING_TALISMAN
                | BRAIN_STORM
                | IMPROVED_EXPLOSIVE_TALISMAN
        )
    }
    /// Spells cast on oneself (no target, own cell).
    pub fn is_self_cast(m: u16) -> bool {
        matches!(
            m,
            TELEPORTATION
                | FROST_BITE
                | TAECHEON_SWORD
                | FIRE_SWORD
                | THUNDER_STRIKE
                | ICE_BREAKER
                | FROZEN_DRAGON
                | HEAVENLY_SKY
                | POISON_CLOUD
                | FOUR_WHEELS
                | CRESCENT_MOON
                | CONTAINMENT
                | INVINCIBILITY
                | EVASION
                | RAGING_WIND
                | CONCENTRATION
                | THE_NEW_BEGINNING
                | JUDGEMENT_OF_HEAVEN
                | SUPERIOR_MAGIC_SHIELD
                | DARK_CONVERSION
                | SPIRITUALISM
                | SUMMON_DEMONIC_CREATURE
                | DEMON_EXPLOSION
                | DRAGON_REPULSE
                | ELEMENTAL_SWORDS
                | MAGIC_SHIELD
                | DEFIANCE
                | MIGHT
                | POISONOUS_CLOUD
                | SHOULDER_DASH
                | MASS_BECKON
                | ENDURANCE
                | REFLECT_DAMAGE
                | FETTER
                | RENOUNCE
                | SUMMON_SKELETON
                | SUMMON_SHINSU
                | SUMMON_JIN_SKELETON
                | STRENGTH_OF_FAITH
                | INVISIBILITY
                | TRANSPARENCY
                | CELESTIAL_LIGHT
                | COMBAT_KICK
                | CLOAK
                | RAKE
                | SUMMON_PUPPET
        )
    }
    /// Beneficial spells that can be cast during a walk or run.
    pub fn is_mobile_support_cast(m: u16) -> bool {
        matches!(
            m,
            HEAL | EMPOWERED_HEALING
                | MASS_HEAL
                | RESURRECTION
                | PURIFICATION
                | DEMONIC_RECOVERY
                | MAGIC_SHIELD
                | SUPERIOR_MAGIC_SHIELD
                | MAGIC_RESISTANCE
                | RESILIENCE
                | DEFIANCE
                | MIGHT
                | ENDURANCE
                | REFLECT_DAMAGE
                | INVINCIBILITY
                | EVASION
                | RAGING_WIND
                | CONCENTRATION
                | THE_NEW_BEGINNING
                | DARK_CONVERSION
                | INVISIBILITY
                | MASS_INVISIBILITY
                | TRANSPARENCY
                | CELESTIAL_LIGHT
                | STRENGTH_OF_FAITH
                | CLOAK
                | RENOUNCE
                | SOUL_RESONANCE
                | SPIRITUALISM
                | DRAGON_REPULSE
                | ELEMENTAL_SWORDS
                | ELEMENTAL_SUPERIORITY
                | BLOOD_LUST
                | SUMMON_SKELETON
                | SUMMON_SHINSU
                | SUMMON_JIN_SKELETON
                | SUMMON_DEMONIC_CREATURE
                | SUMMON_DEAD
                | SUMMON_PUPPET
                | ELEMENTAL_PUPPET
                | MIRROR_IMAGE
        )
    }
    /// Spells that only use the facing direction.
    pub fn is_line(m: u16) -> bool {
        matches!(
            m,
            SCORCHED_EARTH | LIGHTNING_BEAM | FROZEN_EARTH | BLOW_EARTH | GREATER_FROZEN_EARTH
        )
    }
    /// Self buffs cast facing down (Zircon `Combat15`).
    pub fn is_stance_cast(m: u16) -> bool {
        matches!(
            m,
            DEFIANCE | MIGHT | ENDURANCE | REFLECT_DAMAGE | MASS_BECKON
        )
    }
    /// Casts played with the stance frames although not facing down.
    pub fn is_stance_anim(m: u16) -> bool {
        matches!(m, DEFIANCE | MIGHT | ENDURANCE | REFLECT_DAMAGE | FETTER)
    }
    /// Spells the prototype can cast.
    pub fn is_castable(m: u16) -> bool {
        matches!(
            m,
            FIRE_BALL
                | ELEMENTAL_HURRICANE
                | MIRROR_IMAGE
                | FROST_BITE
                | TORNADO
                | CURSED_DOLL
                | SOUL_RESONANCE
                | CORPSE_EXPLODER
                | SUMMON_DEAD
                | CHAIN
                | ICE_BOLT
                | REPULSION
                | THUNDER_BOLT
                | HEAL
                | POISON_DUST
                | FLAMING_DAGGERS
                | SHREDDING
                | SHOULDER_DASH
                | DEFIANCE
                | MIGHT
                | LIGHTNING_BALL
                | GUST_BLAST
                | TELEPORTATION
                | ADAMANTINE_FIRE_BALL
                | ICE_BLADES
                | CYCLONE
                | SCORCHED_EARTH
                | LIGHTNING_BEAM
                | FROZEN_EARTH
                | BLOW_EARTH
                | FIRE_WALL
                | MAGIC_SHIELD
                | EXPLOSIVE_TALISMAN
                | EVIL_SLAYER
                | MAGIC_RESISTANCE
                | GREATER_EVIL_SLAYER
                | RESILIENCE
                | MASS_HEAL
                | POISONOUS_CLOUD
                | INTERCHANGE
                | BECKON
                | MASS_BECKON
                | SWIFT_BLADE
                | ENDURANCE
                | REFLECT_DAMAGE
                | FETTER
                | EXPEL_UNDEAD
                | GEO_MANIPULATION
                | FIRE_STORM
                | LIGHTNING_WAVE
                | ICE_STORM
                | DRAGON_TORNADO
                | GREATER_FROZEN_EARTH
                | CHAIN_LIGHTNING
                | METEOR_SHOWER
                | RENOUNCE
                | TEMPEST
                | ELECTRIC_SHOCK
                | SUMMON_SKELETON
                | SUMMON_SHINSU
                | SUMMON_JIN_SKELETON
                | STRENGTH_OF_FAITH
                | INVISIBILITY
                | MASS_INVISIBILITY
                | TRAP_OCTAGON
                | COMBAT_KICK
                | ELEMENTAL_SUPERIORITY
                | BLOOD_LUST
                | RESURRECTION
                | PURIFICATION
                | TRANSPARENCY
                | CELESTIAL_LIGHT
                | CLOAK
                | WRAITH_GRIP
                | HELL_FIRE
                | RAKE
                | SUMMON_PUPPET
                | SEISMIC_SLAM
                | TAECHEON_SWORD
                | FIRE_SWORD
                | THUNDER_STRIKE
                | ICE_BREAKER
                | FROZEN_DRAGON
                | HEAVENLY_SKY
                | POISON_CLOUD
                | FOUR_WHEELS
                | CRESCENT_MOON
                | FLASH_OF_LIGHT
                | ICE_DRAGON
                | SEARING_LIGHT
                | HEMORRHAGE
                | ABYSS
                | CONTAINMENT
                | NEUTRALIZE
                | PARASITE
                | ICE_RAIN
                | ASTEROID
                | INVINCIBILITY
                | EVASION
                | RAGING_WIND
                | CONCENTRATION
                | THE_NEW_BEGINNING
                | JUDGEMENT_OF_HEAVEN
                | SUPERIOR_MAGIC_SHIELD
                | DARK_CONVERSION
                | LIFE_STEAL
                | SPIRITUALISM
                | CRUSHING_WAVE
                | FIRE_BOUNCE
                | LIGHTNING_STRIKE
                | ICE_AURA
                | BURNING_FIRE
                | DARK_SOUL_PRISON
                | SUMMON_DEMONIC_CREATURE
                | DEMON_EXPLOSION
                | THUNDER_KICK
                | DANCE_OF_SWALLOW
                | HUNDRED_FIST
                | DRAGON_REPULSE
                | ELEMENTAL_SWORDS
                | BINDING_TALISMAN
                | BRAIN_STORM
                | IMPROVED_EXPLOSIVE_TALISMAN
        )
    }
}

/// Zircon `BuffType` values used by the prototype.
pub mod buff_type {
    pub const FROST_BITE: u16 = 205;
    pub const TORNADO: u16 = 206;
    pub const SOUL_RESONANCE: u16 = 311;
    pub const DEFIANCE: u16 = 100;
    pub const MIGHT: u16 = 101;
    pub const ENDURANCE: u16 = 102;
    pub const REFLECT_DAMAGE: u16 = 103;
    pub const RENOUNCE: u16 = 200;
    pub const MAGIC_SHIELD: u16 = 201;
    pub const HEAL: u16 = 300;
    pub const INVISIBILITY: u16 = 301;
    pub const MAGIC_RESISTANCE: u16 = 302;
    pub const RESILIENCE: u16 = 303;
    pub const ELEMENTAL_SUPERIORITY: u16 = 304;
    pub const BLOOD_LUST: u16 = 305;
    pub const STRENGTH_OF_FAITH: u16 = 306;
    pub const CELESTIAL_LIGHT: u16 = 307;
    pub const TRANSPARENCY: u16 = 308;
    pub const POISONOUS_CLOUD: u16 = 400;
    pub const FULL_BLOOM: u16 = 401;
    pub const WHITE_LOTUS: u16 = 402;
    pub const RED_LOTUS: u16 = 403;
    pub const CLOAK: u16 = 404;
    pub const GHOST_WALK: u16 = 405;
    pub const INVINCIBILITY: u16 = 104;
    pub const JUDGEMENT_OF_HEAVEN: u16 = 202;
    pub const SUPERIOR_MAGIC_SHIELD: u16 = 203;
    pub const LIFE_STEAL: u16 = 309;
    pub const SPIRITUALISM: u16 = 310;
    pub const EVASION: u16 = 406;
    pub const RAGING_WIND: u16 = 407;
    pub const CONCENTRATION: u16 = 408;
    pub const THE_NEW_BEGINNING: u16 = 409;
    pub const DARK_CONVERSION: u16 = 410;
    pub const DRAGON_REPULSE: u16 = 106;
    pub const ELEMENTAL_SWORDS: u16 = 107;
    /// Buffs other players can see (Zircon `visible: true`).
    pub fn is_visible(b: u16) -> bool {
        matches!(
            b,
            MAGIC_SHIELD
                | SUPERIOR_MAGIC_SHIELD
                | JUDGEMENT_OF_HEAVEN
                | LIFE_STEAL
                | MAGIC_RESISTANCE
                | RESILIENCE
                | REFLECT_DAMAGE
                | STRENGTH_OF_FAITH
                | INVISIBILITY
                | ELEMENTAL_SUPERIORITY
                | BLOOD_LUST
                | CELESTIAL_LIGHT
                | TRANSPARENCY
                | CLOAK
                | GHOST_WALK
        )
    }
}

/// Zircon `SpellEffect` values.
pub mod spell_effect {
    /// Zircon `MonsterDeathCloud`: a monster's poison cloud cell.
    pub const DEATH_CLOUD: u8 = 14;
    pub const FIRE_WALL: u8 = 2;
    pub const TEMPEST: u8 = 3;
    pub const TRAP_OCTAGON: u8 = 5;
    pub const ICE_AURA: u8 = 11;
    pub const BURNING_FIRE: u8 = 12;
    pub const DARK_SOUL_PRISON: u8 = 13;
    pub const POISONOUS_CLOUD: u8 = 7;
    /// Mining rubble under the miner (Zircon `SpellEffect.Rubble`).
    pub const RUBBLE: u8 = 9;
}

/// Zircon `Effect` values sent with `ObjectEffect` / `MapEffect`.
pub mod effect {
    pub const TELEPORT_OUT: u8 = 1;
    pub const TELEPORT_IN: u8 = 2;
    pub const FULL_BLOOM: u8 = 3;
    pub const WHITE_LOTUS: u8 = 4;
    pub const RED_LOTUS: u8 = 5;
    pub const FIRE_WALL_SMOKE: u8 = 6;
    pub const SWEET_BRIER: u8 = 7;
    pub const KARMA: u8 = 8;
    pub const PUPPET: u8 = 9;
    pub const FLASH_OF_LIGHT: u8 = 10;
    pub const BOUNCE: u8 = 11;
    pub const DEMON_EXPLOSION: u8 = 12;
    pub const DANCE_OF_SWALLOW: u8 = 13;
    pub const HUNDRED_FIST: u8 = 14;
    pub const ELEMENTAL_SWORD: u8 = 15;
    pub const BURNING_FIRE: u8 = 16;
    pub const LEVEL_UP: u8 = 17;
}

/// Authored ZirconRust looks for high-tier armour whose installed ItemInfo has
/// no ExteriorEffect. Values are original C# ExteriorEffect IDs; database
/// assignments always take precedence.
pub mod equipment_visual {
    pub fn armour_effect(item: i32, database_effect: u8) -> u8 {
        if database_effect != 0 {
            return database_effect;
        }
        match item {
            907 | 908 | 1002 | 1003 => 1,            // Argent/Blessed: white aura.
            350 | 351 | 909 | 910 => 3,              // Wyvern/Sacred: blue aura.
            911 | 912 => 11,                         // Phoenix: flame wings.
            913 | 914 => 12,                         // Azure: blue wings.
            917 | 918 | 921 | 922 | 936 | 937 => 13, // Demonic/Abyss: red wings.
            919 | 920 | 1106 | 1107 => 2,            // Embers/Comet: flame aura.
            _ => 0,
        }
    }

    /// Odyn glows follow the weapon sprite silhouette instead of borrowing
    /// the original blade-shaped Chaotic/Janitor overlays.
    pub fn weapon_effect(item: i32, database_effect: u8) -> u8 {
        if database_effect != 0 {
            return database_effect;
        }
        match item {
            1069 => 240, // Son
            1070 => 241, // Mythical
            1072 => 242, // Elemental
            1079 => 243, // Sin
            _ => 0,
        }
    }
}

/// A buff as the client sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuffSummary {
    pub kind: u16,
    /// Milliseconds left; `u64::MAX` for permanent.
    pub remaining_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectState {
    pub id: ObjectId,
    pub appearance: Appearance,
    pub location: Point,
    pub direction: Direction,
    pub hp: i32,
    pub max_hp: i32,
    pub dead: bool,
    /// Ordinary room door with server-owned open/close state.
    pub room_door: bool,
    /// Light radius (Zircon `Stat.Light`; NPCs 10, fire fields 15).
    pub light: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapDescriptor {
    /// File stem under `Map/`, e.g. "0" for `Map/0.map`.
    pub file: String,
    pub name: String,
    /// Zircon `LightSetting`: Default 0 (day cycle), Light 1, Night 2, Twilight 3.
    pub light: u8,
    /// `MapInfo.Music` sound index (0 = none).
    pub music: i32,
    /// `MapInfo.MiniMap`: image in the `MiniMap` library (0 = none).
    pub mini_map: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerStats {
    pub level: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub experience: u64,
    pub max_experience: u64,
    pub min_dc: i32,
    pub max_dc: i32,
    pub min_ac: i32,
    pub max_ac: i32,
    #[serde(default)]
    pub min_mr: i32,
    #[serde(default)]
    pub max_mr: i32,
    #[serde(default)]
    pub min_mc: i32,
    #[serde(default)]
    pub max_mc: i32,
    #[serde(default)]
    pub min_sc: i32,
    #[serde(default)]
    pub max_sc: i32,
    pub accuracy: i32,
    pub agility: i32,
    pub attack_speed: i32,
    /// Held fame title (Zircon `Stat.Fame` = `FameInfo` index, 0 = none) and its name.
    pub fame: i32,
    pub fame_title: String,
    /// Focus points (Zircon `CurrentFP` and `Stat.Focus`), the third pool
    /// beside health and mana. `max_fp` is 0 until a discipline is taken.
    #[serde(default)]
    pub fp: i32,
    #[serde(default)]
    pub max_fp: i32,
    /// Strongest affinity (`element::*`), so the client can tint its own
    /// predicted swing like the server-echoed ones.
    #[serde(default)]
    pub attack_element: u8,
    /// Discipline level (Zircon `UserDiscipline.Level`, 0 = none) and the
    /// experience banked towards the next one.
    #[serde(default)]
    pub discipline: i32,
    #[serde(default)]
    pub discipline_experience: i64,
}

/// What the client needs to show the next discipline level on the character
/// window (Zircon `DisciplineInfo`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisciplineNext {
    /// 0 when the player is already at the highest level the data defines.
    pub level: i32,
    pub required_level: i32,
    pub required_experience: i64,
    pub required_gold: u64,
    pub focus_points: i32,
}

/// Summary of a character on the select screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterSummary {
    pub id: u32,
    pub name: String,
    pub class: Class,
    pub gender: Gender,
    pub hair: u8,
    pub level: i32,
    /// Unix seconds of the last login, 0 if never played.
    pub last_login: u64,
    /// Name of the map the character is on ("" for a new character).
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LoginResult {
    Success { characters: Vec<CharacterSummary> },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NewAccountResult {
    Success,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NewCharacterResult {
    Success { character: CharacterSummary },
    Failed { reason: String },
}

/// Account rules shared by client-side validation and the server.
pub mod rules {
    /// Zircon `Globals` timings (ms).
    pub const MOVE_TIME: u64 = 600;
    pub const TURN_TIME: u64 = 300;
    pub const ATTACK_TIME: u64 = 600;
    /// Zircon `Globals.HarvestTime`: one butchering swing.
    pub const HARVEST_TIME: u64 = 600;
    pub const ATTACK_DELAY: u64 = 1500;
    pub const ASPEED_RATE: u64 = 47;

    /// Time between swings: `max(800, 1500 - AttackSpeed * 47)` ms.
    pub fn attack_delay(attack_speed: i64) -> u64 {
        (ATTACK_DELAY as i64 - attack_speed * ASPEED_RATE as i64).max(800) as u64
    }

    /// Zircon `UserMagic.Cost`: `BaseCost + Level * LevelCost / 3`.
    pub fn magic_cost(base_cost: i32, level_cost: i32, level: u8) -> i32 {
        base_cost + level as i32 * level_cost / 3
    }

    /// Zircon `UserMagic.GetPower` bounds for a level.
    pub fn magic_power(
        min_base: i32,
        max_base: i32,
        min_level: i32,
        max_level: i32,
        level: u8,
    ) -> (i32, i32) {
        let min = (min_base + level as i32 * min_level / 3).max(0);
        let max = max_base + level as i32 * max_level / 3;
        (min, max)
    }

    /// Client-side consumable lock: `max(250, Durability)` ms.
    pub fn use_item_lock(durability: i32) -> u64 {
        durability.max(250) as u64
    }

    pub const EMAIL_MIN: usize = 3;
    pub const EMAIL_MAX: usize = 50;
    pub const PASSWORD_MIN: usize = 6;
    pub const PASSWORD_MAX: usize = 30;
    pub const NAME_MIN: usize = 3;
    pub const NAME_MAX: usize = 15;
    pub const MAX_CHARACTERS: usize = 4;
    pub const HAIR_TYPES: u8 = 10;

    pub fn valid_email(s: &str) -> bool {
        let n = s.chars().count();
        (EMAIL_MIN..=EMAIL_MAX).contains(&n)
            && s.split_once('@').is_some_and(|(name, host)| {
                !name.is_empty() && !host.is_empty() && !host.contains('@')
            })
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "@._-+".contains(c))
    }
    pub fn valid_password(s: &str) -> bool {
        let n = s.chars().count();
        (PASSWORD_MIN..=PASSWORD_MAX).contains(&n) && !s.chars().any(char::is_control)
    }
    /// Character names: letters and digits, must start with a letter.
    pub fn valid_name(s: &str) -> bool {
        let n = s.chars().count();
        (NAME_MIN..=NAME_MAX).contains(&n)
            && s.chars()
                .next()
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false)
            && s.chars().all(|c| c.is_ascii_alphanumeric())
    }
}

#[cfg(test)]
mod account_email_tests {
    use super::rules;

    #[test]
    fn shared_combat_and_name_rules_match_zircon() {
        assert_eq!(rules::attack_delay(0), 1500);
        assert_eq!(rules::attack_delay(20), 800);
        assert_eq!(rules::magic_cost(10, 2, 1), 10);
        assert_eq!(rules::magic_cost(10, 2, 3), 12);
        assert_eq!(rules::magic_power(-5, 10, 1, 3, 1), (0, 11));
        assert_eq!(rules::use_item_lock(100), 250);
        assert!(rules::valid_name("Alice1"));
        assert!(!rules::valid_name("1abc"));
        assert!(rules::valid_password("abc123"));
        assert!(!rules::valid_password("abc\t12"));
    }

    #[test]
    fn email_requires_one_at_with_text_on_both_sides() {
        assert!(rules::valid_email("user@example.com"));
        assert!(rules::valid_email("user+tag@example.com"));
        for invalid in ["username", "@example.com", "user@", "user@@example.com"] {
            assert!(!rules::valid_email(invalid), "accepted {invalid}");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    Hello {
        version: u16,
    },
    NewAccount {
        email: String,
        password: String,
    },
    Login {
        email: String,
        password: String,
    },
    NewCharacter {
        name: String,
        class: Class,
        gender: Gender,
        hair: u8,
    },
    DeleteCharacter {
        id: u32,
    },
    StartGame {
        id: u32,
    },
    QuestAccept {
        quest: i32,
    },
    QuestComplete {
        quest: i32,
        /// `QuestReward` index when the quest offers a choice.
        choice: i32,
    },
    QuestTrack {
        quest: i32,
        track: bool,
    },
    QuestAbandon {
        quest: i32,
    },
    /// Leave the map and return to the character list.
    Logout,
    Turn {
        direction: Direction,
    },
    Move {
        direction: Direction,
        run: bool,
    },
    RoomDoorToggle {
        id: ObjectId,
    },
    Attack {
        direction: Direction,
        /// Melee-augment skill in effect (Slaying, Thrusting, Half Moon...).
        attack_magic: Option<u16>,
    },
    /// Cast a spell (`MirAction.Spell`).
    Magic {
        magic: u16,
        direction: Direction,
        target: Option<ObjectId>,
        location: Point,
    },
    /// Assign a hotkey (0 clears).
    MagicKey {
        magic: u16,
        key: u8,
    },
    /// Toggle a stance skill (Thrusting, Half Moon, Slaying auto).
    MagicToggle {
        magic: u16,
        on: bool,
    },
    /// Move an item between grid slots (equip/unequip/reorder).
    ItemMove {
        from: Grid,
        from_slot: u8,
        to: Grid,
        to_slot: u8,
    },
    /// Buy one additional inventory or account-storage tab with Game Gold.
    BuyTab {
        storage: bool,
    },
    /// Use a consumable or equip an item from the inventory.
    ItemUse {
        slot: u8,
    },
    /// Wear the look of an owned item in the selected equipment slot.
    /// `None` returns to the equipped look; stats never change.
    Wardrobe {
        slot: u8,
        item: Option<i32>,
    },
    /// Link (or clear, with both `None`) a belt slot.
    BeltLink {
        slot: u8,
        info: Option<i32>,
        item: Option<u32>,
    },
    /// Drop an inventory item on the ground.
    ItemLock {
        grid: Grid,
        slot: u8,
        locked: bool,
    },
    ItemDrop {
        slot: u8,
        count: u32,
    },
    /// Pick up whatever lies on or next to the player.
    PickUp,
    /// Chat text; prefixes route it (`/name` whisper, `!!` group, `!` shout,
    /// `!@` global).
    Chat {
        text: String,
    },
    /// Allow group invites (turning it off leaves the group).
    GroupSwitch {
        allow: bool,
    },
    GroupInvite {
        name: String,
    },
    /// Answer the pending invite.
    GroupResponse {
        accept: bool,
    },
    /// Leader kicks a member by name (or leaves by naming themself).
    GroupRemove {
        name: String,
    },
    /// Zircon `ChangeAttackMode` (`attack_mode::*`).
    AttackMode {
        mode: u8,
    },
    /// Zircon `ChangePetMode` (`pet_mode::*`).
    PetMode {
        mode: u8,
    },
    /// Zircon `FishingCast`: `state` is `fishing_state::*`; the client
    /// recasts every attack delay while fishing, `caught` when it reeled on
    /// a nibble.
    FishingCast {
        state: u8,
        direction: Direction,
        float: Point,
        caught: bool,
    },
    /// Swing a pickaxe at the cell in front.
    Mining {
        direction: Direction,
    },
    /// Zircon `C.Harvest`: swing at the corpses in front, in pick-up range.
    Harvest {
        direction: Direction,
    },
    /// Found a guild: 7.5M gold plus 1M per member slot.
    GuildCreate {
        name: String,
        members: i32,
    },
    GuildEditNotice {
        notice: String,
    },
    /// Leader edits a member's rank/permission (index 0 = defaults).
    GuildEditMember {
        index: u32,
        rank: String,
        permission: i32,
    },
    GuildInviteMember {
        name: String,
    },
    GuildKickMember {
        index: u32,
    },
    GuildResponse {
        accept: bool,
    },
    GuildLeave,
    GuildTax {
        tax: i32,
    },
    GuildIncreaseMember,
    /// Declare a two-hour guild war on a guild by name (StartWar
    /// permission, 200,000 from the funds).
    GuildWar {
        name: String,
    },
    /// Leader asks to fight for a castle at its next war window.
    GuildRequestConquest {
        index: i32,
    },
    /// Owner guild: open every castle gate if one is shut, else shut them.
    GuildToggleCastleGates,
    /// Owner guild leader: repair gates and guards from the funds.
    GuildRepairCastleGates,
    /// Send mail to a character by name (offline is fine); items are
    /// (grid, slot, count) cells, at most 5, from a safe zone.
    MailSend {
        recipient: String,
        subject: String,
        message: String,
        gold: u64,
        items: Vec<(Grid, u8, u32)>,
    },
    MailOpened {
        index: u32,
    },
    /// Zircon `C.HairChange`: restyle, spending a Hair Change item.
    /// `colour` is packed ARGB and must be opaque unless `hair` is 0.
    HairChange {
        hair: u8,
        colour: u32,
    },
    /// Zircon `C.GenderChange`: swap gender and restyle in one go, spending
    /// a Gender Change item. Refused while armour is worn.
    GenderChange {
        gender: Gender,
        hair: u8,
        colour: u32,
    },
    /// Zircon `C.ArmourDye`: dye the worn armour, spending an Armour Dye item.
    ArmourDye {
        colour: u32,
    },
    /// Zircon `C.NameChange`: rename, spending a Name Change item.
    NameChange {
        name: String,
    },
    /// Zircon `C.IncreaseDiscipline`: buy the next discipline level.
    IncreaseDiscipline,
    /// Befriend a character by name (offline is fine).
    FriendAdd {
        name: String,
    },
    FriendRemove {
        index: u32,
    },
    /// Block the account behind a character name.
    BlockAdd {
        name: String,
    },
    BlockRemove {
        index: u32,
    },
    /// Advertise a different `online_state` to everyone who friended us.
    ChangeOnlineState {
        state: u8,
    },
    /// Search the marketplace. `name` matches the item name, `item_type`
    /// filters by kind when set, and `page` is a page of `MARKET_PAGE`.
    MarketSearch {
        name: String,
        item_type: Option<u8>,
        sort: MarketSort,
        page: u32,
    },
    /// List `count` of the bag item in `slot` at `price` each, from a safe
    /// zone. `message` is the seller's note (Zircon allows 150 characters).
    MarketConsign {
        slot: u8,
        count: u32,
        price: u64,
        message: String,
    },
    /// Take `count` back off one of your own listings.
    MarketCancelConsign {
        index: u32,
        count: u32,
    },
    /// Buy `count` off a listing at its asking price.
    MarketBuy {
        index: u32,
        count: u32,
    },
    /// Buy `count` of a store row (Zircon `C.MarketPlaceStoreBuy`).
    /// `hunt_gold` spends hunt gold instead of game gold.
    MarketStoreBuy {
        index: u32,
        count: u32,
        hunt_gold: bool,
    },
    /// Zircon `Mount`: toggle riding the owned horse.
    Mount,
    /// Answer a marriage proposal.
    MarriageResponse {
        accept: bool,
    },
    /// Wedding-ring page: make the bag ring in `slot` the wedding ring.
    MarriageMakeRing {
        slot: u8,
    },
    /// Teleport to the partner (needs the wedding ring on).
    MarriageTeleport,
    /// Inventory slots and item identities; six optional colour ingredients.
    NpcWeaponCraft {
        base: (u8, u32),
        materials: [Option<(u8, u32)>; 6],
        class: Class,
    },
    /// Refine the equipped weapon at a Refine page: ores (<=5 black iron
    /// ore cells), items (<=3 common jewellery), specials (<=1).
    NpcRefine {
        refine_type: u8,
        quality: u8,
        ores: Vec<(Grid, u8, u32)>,
        items: Vec<(Grid, u8, u32)>,
        specials: Vec<(Grid, u8, u32)>,
    },
    /// Collect a finished refine at a RefineRetrieve page.
    NpcRefineRetrieve {
        index: u32,
    },
    /// Zircon accessory pages 11, 12, 13, and 15. Item identities prevent
    /// stale clicks from consuming a different item in the same slot.
    NpcAccessoryLevel {
        target: (Grid, u8, u32),
        materials: Vec<(Grid, u8, u32)>,
    },
    NpcAccessoryUpgrade {
        target: (Grid, u8, u32),
        refine_type: u8,
    },
    NpcAccessoryReset {
        target: (Grid, u8, u32),
    },
    NpcAccessoryRefine {
        target: (Grid, u8, u32),
        ore: (Grid, u8, u32),
        materials: Vec<(Grid, u8, u32)>,
        refine_type: u8,
    },
    /// Unlock a companion look with its unlock item.
    CompanionUnlock {
        index: i32,
    },
    /// Adopt a companion (CompanionManage page) and name it.
    CompanionAdopt {
        index: i32,
        name: String,
    },
    CompanionRetrieve {
        index: u32,
    },
    CompanionStore,
    CompanionRelease {
        index: u32,
    },
    /// Take one item out of a companion's bag.
    CompanionBagTake {
        index: u32,
        slot: u8,
    },
    /// Equip a Companion Food stack from inventory; swaps previous food back.
    CompanionFoodEquip {
        slot: u8,
    },
    /// Return the equipped food stack to inventory.
    CompanionFoodTake,
    /// Toggle the safety gate for rerolling one companion skill milestone.
    CompanionSkillToggle {
        level: i32,
    },
    /// Take an attachment (slot 255 = the gold).
    MailGetItem {
        index: u32,
        slot: u8,
    },
    MailDelete {
        index: u32,
    },
    /// Ask the player in front (facing you) to trade.
    TradeRequest,
    TradeResponse {
        accept: bool,
    },
    TradeClose,
    /// Offer `count` of a cell (Zircon `TradeAddItem`).
    TradeAddItem {
        grid: Grid,
        slot: u8,
        count: u32,
    },
    /// Raise the offered gold to this total.
    TradeAddGold {
        gold: u64,
    },
    TradeConfirm,
    /// Return to town while dead (Zircon `C.TownRevive`).
    TownRevive,
    NpcCall {
        id: ObjectId,
    },
    NpcButton {
        button: i32,
    },
    NpcBuy {
        info: i32,
        count: u32,
    },
    /// Sell counts from inventory slots to the open shop.
    NpcSell {
        items: Vec<(u8, u32)>,
    },
    /// Repair selected inventory or equipment items at an open repair page.
    NpcRepair {
        items: Vec<(Grid, u8, u32)>,
        special: bool,
    },
    NpcClose,
    /// Ask for a page of the ranking board (Zircon `C.RankRequest`).
    /// `class` None is Zircon's `RequiredClass.All`.
    RankRequest {
        class: Option<Class>,
        online_only: bool,
        start: u32,
    },
    /// Find an exact character name on the all-class board.
    RankSearch {
        name: String,
    },
    /// View public appearance and equipped items for a ranked character.
    RankInspect {
        character: u32,
    },
    /// Spend one Fortune Checker on this item to read its drop progress
    /// (Zircon `C.FortuneCheck`).
    FortuneCheck {
        item: i32,
    },
    Ping {
        nonce: u32,
    },
    /// Equip a companion bag from player inventory.
    CompanionBagEquip {
        slot: u8,
    },
    /// Return the equipped companion bag to player inventory.
    CompanionBagUnequip,
    /// Pick up the clicked ground item when several share a cell.
    PickUpItem {
        id: ObjectId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Sent once after `Hello` is accepted.
    Connected,
    NewAccountResult(NewAccountResult),
    LoginResult(LoginResult),
    NewCharacterResult(NewCharacterResult),
    DeleteCharacterResult {
        id: u32,
        ok: bool,
        reason: String,
    },
    /// The player left the map; the client should show the character list.
    LoggedOut {
        characters: Vec<CharacterSummary>,
    },
    Welcome {
        id: ObjectId,
        map: MapDescriptor,
        location: Point,
        direction: Direction,
        stats: PlayerStats,
        light: u8,
    },
    Rejected {
        reason: String,
    },
    ObjectShow(ObjectState),
    ObjectRemove {
        id: ObjectId,
    },
    ObjectTurn {
        id: ObjectId,
        direction: Direction,
    },
    ObjectMove {
        id: ObjectId,
        from: Point,
        to: Point,
        direction: Direction,
        run: bool,
    },
    /// The server refused the requester's own move; resync to `location`.
    MoveDenied {
        location: Point,
        direction: Direction,
    },
    ObjectAttack {
        id: ObjectId,
        direction: Direction,
        attack_magic: Option<u16>,
        /// Zircon `AttackElement`: the attacker's strongest affinity, which
        /// tints the swing effects (`element::*`, NONE = white).
        element: u8,
    },
    /// A monster shot something at `target` (or `location`); `magic` picks
    /// the projectile look (0 = plain).
    ObjectRangeAttack {
        id: ObjectId,
        direction: Direction,
        target: Option<ObjectId>,
        location: Point,
        magic: u16,
    },
    /// Someone cast a spell: play the cast animation and, when `cast`, the
    /// payload effects on `targets` / `locations` afterwards.
    ObjectMagic {
        id: ObjectId,
        direction: Direction,
        location: Point,
        magic: u16,
        targets: Vec<ObjectId>,
        locations: Vec<Point>,
        cast: bool,
    },
    /// The player's full skill list (on entry).
    Magics(Vec<MagicSummary>),
    /// Belt links on entry (after `Inventory`).
    BeltLinks(Vec<BeltLink>),
    NewMagic(MagicSummary),
    MagicLeveled {
        magic: u16,
        level: u8,
        experience: u64,
    },
    MagicCooldown {
        magic: u16,
        delay_ms: u32,
    },
    MagicToggle {
        magic: u16,
        on: bool,
    },
    /// Active poison flags, including paralysis, on an object.
    ObjectPoisoned {
        id: ObjectId,
        kinds: u16,
    },
    ObjectStruck {
        id: ObjectId,
        attacker: ObjectId,
        damage: i32,
        /// `element::*` of the hit; `None` for plain melee.
        element: u8,
        /// True when a spell (not a swing) caused the hit.
        magic: bool,
    },
    HealthChanged {
        id: ObjectId,
        hp: i32,
        max_hp: i32,
    },
    ObjectDie {
        id: ObjectId,
    },
    /// An object jumped to a cell (teleport, swap, pull); sent to itself too.
    ObjectTeleport {
        id: ObjectId,
        location: Point,
        direction: Direction,
    },
    /// One Shoulder Dash step (Zircon `S.ObjectDash`); sent to the dasher too.
    ObjectDash {
        id: ObjectId,
        direction: Direction,
        /// Cell the dasher now stands on.
        location: Point,
        distance: u8,
        magic: u16,
    },
    /// A one-off effect on an object (teleport in/out, lotus hits).
    ObjectEffect {
        id: ObjectId,
        effect: u8,
        location: Point,
    },
    /// A one-off effect on a cell (fire wall smoke).
    MapEffect {
        location: Point,
        effect: u8,
    },
    BuffAdd(BuffSummary),
    BuffRemove {
        kind: u16,
    },
    BuffTime {
        kind: u16,
        remaining_ms: u64,
    },
    /// A visible buff started or ended on someone (Zircon `ObjectBuffAdd/Remove`).
    ObjectBuff {
        id: ObjectId,
        kind: u16,
        on: bool,
    },
    ObjectRevive {
        id: ObjectId,
        location: Point,
        direction: Direction,
        hp: i32,
    },
    StatsChanged(PlayerStats),
    /// Full inventory sync on entering the world.
    Inventory {
        size: u8,
        inventory: Vec<(u8, ItemInstance)>,
        equipment: Vec<(u8, ItemInstance)>,
        gold: u64,
        weights: Weights,
    },
    /// A slot's content changed (`None` = now empty).
    ItemChanged {
        grid: Grid,
        slot: u8,
        item: Option<ItemInstance>,
    },
    GoldChanged {
        gold: u64,
    },
    WeightsChanged(Weights),
    /// An object's look changed (equipment).
    ObjectAppearance {
        id: ObjectId,
        appearance: Appearance,
    },
    /// Current cosmetic choice for one equipment slot.
    Wardrobe {
        slot: u8,
        item: Option<i32>,
    },
    /// An object's light radius changed, usually when a torch is equipped.
    ObjectLight {
        id: ObjectId,
        light: u8,
    },
    /// The next discipline level the player may buy, and what it costs
    /// (Zircon reads `DisciplineInfo` on the client; we send it instead).
    /// Sent on entry and after every discipline change.
    DisciplineInfo(DisciplineNext),
    /// The player was moved to another map; forget every object.
    MapChanged {
        map: MapDescriptor,
        location: Point,
        direction: Direction,
    },
    /// Cells where storage and other safe-zone rules apply on the current map.
    SafeZones {
        cells: Vec<Point>,
    },
    NpcResponse {
        npc: ObjectId,
        page: i32,
        say: String,
        /// Zircon `NPCDialogType` (1 = BuySell).
        dialog_type: i32,
        /// Currency name for shop prices and payment.
        currency: String,
        goods: Vec<Good>,
        /// Item types this shop buys.
        sell_types: Vec<u8>,
        /// Quests this NPC starts or finishes for the player.
        quests: Vec<NpcQuest>,
    },
    NpcClose,
    /// Daylight 0 (night) .. 1 (day) for maps with the default light setting.
    DayChanged {
        day_time: f32,
    },
    /// The whole quest log on entering the world.
    QuestList(Vec<UserQuestSummary>),
    QuestChanged(UserQuestSummary),
    /// Per-viewer marker above an NPC: 0 none, 1 available, 2 in progress, 3 ready.
    NpcQuestMarker {
        id: ObjectId,
        marker: u8,
    },
    QuestCancelled {
        quest: i32,
    },
    /// System / status line (Zircon `MessageType.System`).
    Chat {
        text: String,
    },
    GroupSwitch {
        allow: bool,
    },
    GroupInvite {
        from: String,
    },
    /// A member joined (sent per existing member to a joiner, self last);
    /// the first member received is the leader.
    GroupMember {
        id: ObjectId,
        name: String,
    },
    /// A member left; your own id means the group is gone for you.
    GroupRemove {
        id: ObjectId,
    },
    AttackMode {
        mode: u8,
    },
    PetMode {
        mode: u8,
    },
    /// Someone is fishing (Zircon `ObjectFishing`): `found` means a nibble.
    ObjectFishing {
        id: ObjectId,
        state: u8,
        direction: Direction,
        float: Point,
        found: bool,
    },
    /// The caster's own fishing progress (Zircon `FishingStats`).
    FishingStats {
        points: i32,
        required: i32,
        throw_quality: i32,
        /// Reel window hint (-1 = unchanged).
        accuracy: i32,
    },
    /// Someone swung a pickaxe; `effect` when rock was actually hit.
    ObjectMining {
        id: ObjectId,
        direction: Direction,
        effect: bool,
    },
    /// Someone swung at a corpse (Zircon `S.ObjectHarvest`).
    ObjectHarvest {
        id: ObjectId,
        direction: Direction,
    },
    /// This corpse has nothing left for you, so draw it as bones
    /// (Zircon `S.ObjectHarvested`).
    ObjectHarvested {
        id: ObjectId,
    },
    /// The player's guild (None when not in one); resent after changes.
    GuildInfo(Option<GuildSummary>),
    GuildNoticeChanged {
        notice: String,
    },
    GuildUpdate {
        member_limit: i32,
        funds: i64,
        tax: i32,
    },
    /// A member (by index) left or was kicked.
    GuildKick {
        index: u32,
    },
    GuildInvite {
        from: String,
        guild: String,
    },
    GuildMemberOffline {
        index: u32,
    },
    /// A guild war with `guild` began (both sides get it).
    GuildWarStarted {
        guild: String,
        duration_secs: u64,
    },
    GuildWarFinished {
        guild: String,
    },
    /// When the requested (or defended) castle war starts, in seconds
    /// (negative: none scheduled).
    GuildConquestDate {
        index: i32,
        war_in_secs: i64,
    },
    GuildConquestStarted {
        index: i32,
    },
    GuildConquestFinished {
        index: i32,
    },
    /// A castle and its owner guild ("" when unowned); sent on entry and
    /// whenever ownership changes.
    CastleInfo {
        index: i32,
        name: String,
        owner: String,
    },
    MarriageInvite {
        from: String,
    },
    /// Partner name and the wedding ring's item id (None when unmarried).
    MarriageInfo {
        partner: Option<String>,
        wedding_ring: Option<u32>,
    },
    /// Weapons in the furnace (on entry and after each refine).
    RefineList(Vec<RefineSummary>),
    RefineRetrieved {
        index: u32,
    },
    /// Companions for sale, sent with a CompanionManage page.
    CompanionShop(Vec<CompanionOffer>),
    /// The player's companions (on entry and after changes).
    Companions(Vec<CompanionSummary>),
    /// Every currency the player holds; resent when one changes.
    Currencies(Vec<CurrencySummary>),
    /// The mailbox on entry.
    MailList(Vec<MailSummary>),
    MailNew(MailSummary),
    MailDelete {
        index: u32,
    },
    /// An attachment was taken (slot 255 = the gold).
    MailItemDelete {
        index: u32,
        slot: u8,
    },
    /// The friend list on entry and after every change.
    Friends(Vec<FriendSummary>),
    /// The block list on entry and after every change.
    Blocks(Vec<BlockSummary>),
    /// One friend's state changed (Zircon `S.FriendUpdate`).
    FriendUpdate(FriendSummary),
    /// The state this player is advertising, echoed after a change.
    OnlineState {
        state: u8,
    },
    /// One page of search results, with the total the search matched.
    MarketSearchResults {
        total: u32,
        page: u32,
        results: Vec<MarketListing>,
    },
    /// Everything this account has listed (on entry and after a change).
    MarketConsignments(Vec<MarketListing>),
    /// A listing of ours changed size; `count` 0 means it is gone.
    MarketConsignChanged {
        index: u32,
        count: u32,
    },
    /// The whole store, sent on entry. It is 92 rows in this pack and never
    /// changes at run time, so the client filters and sorts it locally
    /// rather than asking the server per search, as Zircon's own client does.
    StoreStock(Vec<StoreEntry>),
    /// Account storage on entry.
    Storage {
        size: u32,
        items: Vec<(u8, ItemInstance)>,
    },
    /// Prices for the next tab, in Game Gold (zero means sold out).
    TabPrices {
        inventory: u32,
        storage: u32,
    },
    TradeRequest {
        from: String,
    },
    /// A trade with `name` opened.
    TradeOpen {
        name: String,
    },
    TradeClose,
    /// Your own offer was accepted.
    TradeAddItem {
        grid: Grid,
        slot: u8,
        count: u32,
    },
    /// Your offered gold total.
    TradeAddGold {
        gold: u64,
    },
    /// The partner offered an item (`count` = offered amount).
    TradeItemAdded {
        item: ItemInstance,
    },
    /// The partner's offered gold total.
    TradeGoldAdded {
        gold: u64,
    },
    /// Your confirmation was cleared; confirm again.
    TradeUnlock,
    /// Someone spoke; `id` set for local talk shows a bubble over them.
    Say {
        id: Option<ObjectId>,
        kind: ChatKind,
        text: String,
    },
    /// A page of the ranking board (Zircon `S.Rankings`). `total` counts
    /// every row the filters admit, so the client can size its scrollbar.
    Rankings {
        class: Option<Class>,
        online_only: bool,
        start: u32,
        total: u32,
        entries: Vec<RankEntry>,
    },
    RankSearchResult {
        entry: Option<RankEntry>,
        start: u32,
    },
    RankInspection(Option<RankInspection>),
    /// Start, refresh or clear a named countdown (Zircon `S.SetTimer`).
    /// `seconds` of 0 or less expires the timer; `kind` 0 draws digits
    /// only, 1 and 2 add the running egg timer.
    SetTimer {
        key: String,
        kind: u8,
        seconds: i32,
    },
    /// Drop progress for the items the account has checked
    /// (Zircon `S.FortuneUpdate`).
    FortuneUpdate {
        fortunes: Vec<FortuneSummary>,
    },
    Pong {
        nonce: u32,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("frame too large: {0} bytes")]
    FrameTooLarge(usize),
    #[error("encode error: {0}")]
    Encode(postcard::Error),
    #[error("decode error: {0}")]
    Decode(postcard::Error),
}

/// Encode a message into a length-prefixed frame.
pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, ProtoError> {
    let body = postcard::to_stdvec(msg).map_err(ProtoError::Encode)?;
    if body.len() > MAX_FRAME_LEN {
        return Err(ProtoError::FrameTooLarge(body.len()));
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Try to take one complete frame from the front of `buf`. Returns the decoded
/// message and drains the consumed bytes. `Ok(None)` means more data is needed.
pub fn decode_frame<T: for<'de> Deserialize<'de>>(
    buf: &mut Vec<u8>,
) -> Result<Option<T>, ProtoError> {
    decode_frame_max(buf, MAX_FRAME_LEN)
}

/// Like [`decode_frame`] with a caller-chosen frame cap (the server uses
/// [`CLIENT_MAX_FRAME_LEN`] for what clients send).
pub fn decode_frame_max<T: for<'de> Deserialize<'de>>(
    buf: &mut Vec<u8>,
    max_len: usize,
) -> Result<Option<T>, ProtoError> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > max_len {
        return Err(ProtoError::FrameTooLarge(len));
    }
    if buf.len() < 4 + len {
        return Ok(None);
    }
    let msg = postcard::from_bytes(&buf[4..4 + len]).map_err(ProtoError::Decode)?;
    buf.drain(..4 + len);
    Ok(Some(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let msg = ServerMessage::ObjectMove {
            id: ObjectId(7),
            from: Point::new(1, 2),
            to: Point::new(2, 3),
            direction: Direction::DownRight,
            run: false,
        };
        let mut buf = encode(&msg).unwrap();
        buf.extend_from_slice(&encode(&ServerMessage::Pong { nonce: 9 }).unwrap());
        let a: ServerMessage = decode_frame(&mut buf).unwrap().unwrap();
        assert_eq!(a, msg);
        let b: ServerMessage = decode_frame(&mut buf).unwrap().unwrap();
        assert_eq!(b, ServerMessage::Pong { nonce: 9 });
        assert!(decode_frame::<ServerMessage>(&mut buf).unwrap().is_none());
    }

    #[test]
    fn character_level_1000_roundtrip() {
        let msg = ServerMessage::LoginResult(LoginResult::Success {
            characters: vec![CharacterSummary {
                id: 1,
                name: "Hero".into(),
                class: Class::Warrior,
                gender: Gender::Male,
                hair: 0,
                level: MAX_PLAYER_LEVEL,
                last_login: 0,
                location: String::new(),
            }],
        });
        let mut buf = encode(&msg).unwrap();
        assert_eq!(decode_frame::<ServerMessage>(&mut buf).unwrap(), Some(msg));
    }

    #[test]
    fn direction_from_points() {
        let o = Point::new(5, 5);
        assert_eq!(Direction::from_points(o, Point::new(5, 4)), Direction::Up);
        assert_eq!(
            Direction::from_points(o, Point::new(6, 4)),
            Direction::UpRight
        );
        assert_eq!(
            Direction::from_points(o, Point::new(6, 5)),
            Direction::Right
        );
        assert_eq!(
            Direction::from_points(o, Point::new(6, 6)),
            Direction::DownRight
        );
        assert_eq!(Direction::from_points(o, Point::new(5, 6)), Direction::Down);
        assert_eq!(
            Direction::from_points(o, Point::new(4, 6)),
            Direction::DownLeft
        );
        assert_eq!(Direction::from_points(o, Point::new(4, 5)), Direction::Left);
        assert_eq!(
            Direction::from_points(o, Point::new(4, 4)),
            Direction::UpLeft
        );
        assert_eq!(
            Direction::from_points(o, Point::new(15, 6)),
            Direction::Right
        );
    }
}

/// One piece of an NPC dialog page: plain text or a clickable button link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DialogPart {
    Text(String),
    Button { label: String, id: i32 },
    NewLine,
}

/// Parse Zircon NPC page text: `[Label:ID]` becomes a button, `\r\n` a line
/// break, everything else plain text.
pub fn parse_dialog(say: &str) -> Vec<DialogPart> {
    let mut parts = Vec::new();
    let mut text = String::new();
    // Normalise line endings so the loop only deals with `\n`.
    let rest = say.replace("\r\n", "\n").replace('\r', "\n");
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => {
                if !text.is_empty() {
                    parts.push(DialogPart::Text(std::mem::take(&mut text)));
                }
                parts.push(DialogPart::NewLine);
            }
            '[' => {
                let mut inner = String::new();
                let mut closed = false;
                for d in chars.by_ref() {
                    if d == ']' {
                        closed = true;
                        break;
                    }
                    inner.push(d);
                }
                match (closed, inner.rsplit_once(':')) {
                    (true, Some((label, id))) if id.trim().parse::<i32>().is_ok() => {
                        if !text.is_empty() {
                            parts.push(DialogPart::Text(std::mem::take(&mut text)));
                        }
                        parts.push(DialogPart::Button {
                            label: label.to_string(),
                            id: id.trim().parse().unwrap(),
                        });
                    }
                    _ => {
                        text.push('[');
                        text.push_str(&inner);
                        if closed {
                            text.push(']');
                        }
                    }
                }
            }
            '{' => {
                let mut inner = String::new();
                let mut closed = false;
                for d in chars.by_ref() {
                    if d == '}' {
                        closed = true;
                        break;
                    }
                    inner.push(d);
                }
                if let (true, Some((label, _color))) = (closed, inner.rsplit_once(':')) {
                    text.push_str(label);
                } else {
                    text.push('{');
                    text.push_str(&inner);
                    if closed {
                        text.push('}');
                    }
                }
            }
            other => text.push(other),
        }
    }
    if !text.is_empty() {
        parts.push(DialogPart::Text(text));
    }
    parts
}

#[cfg(test)]
mod dialog_tests {
    use super::*;

    #[test]
    fn parses_buttons_and_lines() {
        let p = parse_dialog("Hi there,\r\n[Browse:1]\r\n\r\n[Exit:0] bye [x]");
        assert_eq!(
            p,
            vec![
                DialogPart::Text("Hi there,".into()),
                DialogPart::NewLine,
                DialogPart::Button {
                    label: "Browse".into(),
                    id: 1
                },
                DialogPart::NewLine,
                DialogPart::NewLine,
                DialogPart::Button {
                    label: "Exit".into(),
                    id: 0
                },
                DialogPart::Text(" bye [x]".into()),
            ]
        );
    }

    #[test]
    fn colored_npc_value_is_readable_without_markup() {
        assert_eq!(
            parse_dialog("{100:LawnGreen} points required"),
            vec![DialogPart::Text("100 points required".into())]
        );
    }
}
