//! The game world: maps, objects, movement, combat, monster AI and spawning.
//!
//! Rules and timings follow Zircon's server (`ServerLibrary/Models/*`): action
//! cooldowns rather than a fixed tick, 600 ms per walk/run, 300 ms turns,
//! attack delay `max(800, 1500 - AttackSpeed * 47)`, monster AI with 3 s search,
//! 2 s roam, greedy chase, 1-cell melee. Time is milliseconds since server start.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use mir_formats::mirdb::stat;
use mir_formats::MapFile;
use mir_proto::{
    buff_type, effect, element, item_type, magic_type, parse_dialog, slot, spell_effect,
    Appearance, BeltLink, BuffSummary, Class, Direction, Gender, Good, Grid, MapDescriptor,
    ObjectId, ObjectState, PlayerStats, Point, ServerMessage, Weights, CAST_TIME, MAGIC_DELAY,
    MAGIC_RANGE, MAX_BELT,
};
use rand::Rng;

use crate::accounts::CharacterRecord;
use crate::data::{DropDef, GameData, MonsterDef, RespawnDef};
use crate::items::{can_use, default_slot, sell_price, Bag, Changed, UserItem};
use crate::magic::{UserMagic, SKILL_EXP};

pub use mir_proto::rules::{attack_delay, ATTACK_TIME, MOVE_TIME, TURN_TIME};
pub const MAX_VIEW_RANGE: i32 = 18;
pub const SEARCH_DELAY: u64 = 3000;
pub const ROAM_DELAY: u64 = 2000;
pub const DEAD_DURATION: u64 = 60_000;
pub const REGEN_DELAY: u64 = 10_000;
/// Zircon `Config.AutoReviveDelay`: forced town revive after 10 minutes;
/// the player can return to town at any time while dead.
pub const REVIVE_DELAY: u64 = 600_000;

/// Zircon `PoisonType` bits used by the prototype.
pub mod poison_kind {
    pub const GREEN: u16 = 1;
    pub const RED: u16 = 2;
    pub const SLOW: u16 = 4;
    pub const PARALYSIS: u16 = 8;
    /// Stops movement (Zircon `WraithGrip`).
    pub const WRAITH_GRIP: u16 = 16;
    /// Tick damage with no colour (Zircon `HellFire`).
    pub const HELL_FIRE: u16 = 32;
    /// Stops a monster completely.
    pub const SILENCED: u16 = 64;
    /// Shrinks a monster's view range to 2 and halves its hit chance.
    pub const ABYSS: u16 = 128;
    /// Monsters stop attacking and run.
    pub const FEAR: u16 = 1024;
    /// Roots (tick damage).
    pub const CONTAINMENT: u16 = 4096;
    /// Tick damage that stops regeneration.
    pub const HEMORRHAGE: u16 = 16384;
    pub const BINDING: u16 = 32768;
    /// Tick damage that spreads (Parasite).
    pub const PARASITE: u16 = 256;
    /// Doubles attack and move delays.
    pub const NEUTRALIZE: u16 = 512;
    /// Zircon `BuffType.MagicWeakness`: magic resistance counts as zero.
    pub const MAGIC_WEAKNESS: u16 = 2048;
}

/// Paralysed objects take no actions.
fn paralysed(o: &Object) -> bool {
    o.poisons.iter().any(|p| p.kind == poison_kind::PARALYSIS)
}

/// Zircon slow poison: `Value * 100` ms added to every action delay.
fn slow_ms(o: &Object) -> u64 {
    o.poisons
        .iter()
        .filter(|p| p.kind == 4)
        .map(|p| p.value.max(0) as u64 * 100)
        .max()
        .unwrap_or(0)
}

/// Belt links from the character record, one entry per slot, with stale
/// item links pruned (Zircon `GetStartInformation`).
fn load_belt(stored: &[BeltLink], bag: &Bag) -> Vec<BeltLink> {
    (0..MAX_BELT as u8)
        .map(|slot| {
            let mut l = stored
                .iter()
                .find(|l| l.slot == slot)
                .copied()
                .unwrap_or(BeltLink::empty(slot));
            l.slot = slot;
            if let Some(item) = l.item {
                let present = bag
                    .inventory
                    .iter()
                    .any(|s| s.as_ref().map(|it| it.id == item).unwrap_or(false));
                if !present {
                    l.item = None;
                }
            }
            l
        })
        .collect()
}
pub const CELL_GRACE: u64 = 300;
/// Ground items vanish after this (Zircon `Config.DropDuration`, 60 min).
pub const DROP_DURATION: u64 = 60 * 60_000;
/// Other players may take a drop after this (Zircon group rule: 2 min).
pub const DROP_SHARE_AFTER: u64 = 120_000;
pub const DROP_DISTANCE: i32 = 5;
pub const PICKUP_RADIUS: i32 = 1;
/// Element index 0 is physical; 1..=7 match `mir_proto::element`.
pub(super) const ELEMENT_ATTACK_STATS: [i32; 8] = [0, 20, 22, 24, 26, 28, 30, 32];
/// Zircon `Stat.FireAffinity`..`PhantomAffinity`, indexed by `element::*`.
pub(crate) const AFFINITY_STATS: [i32; 8] = [0, 43, 44, 45, 46, 47, 48, 49];

/// Zircon `Functions.GetAffinityElement`: the element whose affinity stat
/// is highest, earlier elements winning ties; none when all are zero.
pub(crate) fn affinity_element(stat: impl Fn(i32) -> i32) -> u8 {
    let mut best = (mir_proto::element::NONE, 0);
    for (element, id) in AFFINITY_STATS.iter().enumerate().skip(1) {
        let value = stat(*id);
        if value > best.1 {
            best = (element as u8, value);
        }
    }
    best.0
}
pub(super) const ELEMENT_RESIST_STATS: [i32; 8] = [118, 21, 23, 25, 27, 29, 31, 33];

pub type ConnId = u64;

/// Zircon: `max(800, AttackDelay - AttackSpeed * ASpeedRate)` milliseconds.
#[derive(Debug, Clone, Copy)]
pub struct CombatStats {
    pub accuracy: i32,
    pub agility: i32,
    pub min_ac: i32,
    pub max_ac: i32,
    pub min_dc: i32,
    pub max_dc: i32,
    pub min_mr: i32,
    pub max_mr: i32,
    pub min_mc: i32,
    pub max_mc: i32,
    pub min_sc: i32,
    pub max_sc: i32,
    pub luck: i32,
    pub critical_chance: i32,
    pub critical_damage: i32,
    pub paralysis_chance: i32,
    pub slow_chance: i32,
    pub silence_chance: i32,
    pub block_chance: i32,
    pub evasion_chance: i32,
    pub poison_resistance: i32,
    pub pickup_radius: i32,
    pub element_attack: [i32; 8],
    pub element_resist: [i32; 8],
    /// Zircon `GetAffinityElement`: the element with the highest affinity.
    pub attack_element: u8,
}

impl CombatStats {
    pub const ZERO: CombatStats = CombatStats {
        accuracy: 0,
        agility: 0,
        min_ac: 0,
        max_ac: 0,
        min_dc: 0,
        max_dc: 0,
        min_mr: 0,
        max_mr: 0,
        min_mc: 0,
        max_mc: 0,
        min_sc: 0,
        max_sc: 0,
        luck: 0,
        critical_chance: 0,
        critical_damage: 0,
        paralysis_chance: 0,
        slow_chance: 0,
        silence_chance: 0,
        block_chance: 0,
        evasion_chance: 0,
        poison_resistance: 0,
        pickup_radius: 1,
        element_attack: [0; 8],
        element_resist: [0; 8],
        attack_element: 0,
    };
}

/// A poison instance (Zircon `Poison`).
#[derive(Debug, Clone)]
pub struct Poison {
    /// Zircon `PoisonType` bit: 1 Green, 2 Red.
    pub kind: u16,
    pub value: i32,
    pub ticks_left: i32,
    pub next_tick: u64,
    pub owner: Option<ObjectId>,
}

/// Stat changes a buff carries (Zircon `BuffInfo.Stats`, the subset used).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuffStats {
    pub dc_pct: i32,
    pub phys_def_pct: i32,
    pub mag_def_pct: i32,
    pub max_ac: i32,
    pub max_mr: i32,
    pub agility: i32,
    /// Percent of damage absorbed (Zircon `Stat.MagicShield`).
    pub magic_shield: i32,
    /// Percent of melee damage returned to monsters (Zircon `Stat.ReflectDamage`).
    pub reflect: i32,
    pub hp_pct: i32,
    pub mc_pct: i32,
    /// Zircon `Stat.PetDCPercent`.
    pub pet_dc_pct: i32,
    pub max_dc: i32,
    pub max_mc: i32,
    pub max_sc: i32,
    /// Percent of max HP restored instead of dying (Zircon `Stat.CelestialLight`).
    pub celestial: i32,
    /// HP drained every 2 s while cloaked (Zircon `Stat.CloakDamage`).
    pub cloak_damage: i32,
    /// Percent chance to dodge elemental damage (Evasion).
    pub evasion: i32,
    /// Extra critical chance percent (Concentration).
    pub crit: i32,
    /// Charge stacks (The New Beginning).
    pub stacks: i32,
    /// Absorb pool (Superior Magic Shield) or tick amount (Dark Conversion).
    pub pool: i32,
    /// Percent chance to strike back when hit (Judgement Of Heaven).
    pub judgement: i32,
    pub invincible: bool,
    /// Raging Wind level + 1 (re-splits AC/MR pools).
    pub raging_wind: i32,
    /// Life steal percent from a buff.
    pub life_steal: i32,
    /// Damage banked by Frost Bite.
    pub frost: i32,
    pub frost_chance: i32,
}

/// A timed buff on a player (Zircon `BuffInfo`).
#[derive(Debug, Clone)]
pub struct Buff {
    pub kind: u16,
    /// Server time when it ends; `u64::MAX` never.
    pub expires: u64,
    pub stats: BuffStats,
    /// Next tick for ticking buffs (Cloak drain).
    pub tick_at: u64,
}

/// Shoulder Dash in progress.
#[derive(Debug, Clone, Copy)]
pub struct Dash {
    pub remaining: i32,
    pub travelled: i32,
    pub direction: Direction,
    pub next_step: u64,
}

/// A spell object on a cell (Zircon `SpellObject`).
#[derive(Debug)]
pub struct SpellData {
    pub effect: u8,
    pub tick_count: i32,
    pub tick_frequency: u64,
    pub tick_time: u64,
    pub owner: ObjectId,
    pub magic: u16,
    /// Trap Octagon: the monsters held; the ring vanishes when none is left.
    pub targets: Vec<ObjectId>,
}

/// Zircon `BuffType.Heal`: heals `cap` per second until the pool is empty.
#[derive(Debug, Clone)]
pub struct HealBuff {
    pub pool: i32,
    pub cap: i32,
    pub next_tick: u64,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct PlayerData {
    pub conn: ConnId,
    pub account: u32,
    pub name: String,
    pub class: Class,
    pub gender: Gender,
    pub hair: u8,
    /// Zircon `Character.HairColour`, packed ARGB.
    pub hair_colour: u32,
    /// Cosmetic armour look, chosen while the matching item was owned.
    pub wardrobe_item: Option<i32>,
    pub wardrobe_helmet: Option<i32>,
    pub wardrobe_weapon: Option<i32>,
    pub wardrobe_shoes: Option<i32>,
    /// Account character id, for persistence.
    pub character: u32,
    pub gm: bool,
    pub gm_mode: bool,
    pub superman: bool,
    pub observer: bool,
    pub chat_ban_until: u64,
    pub global_ban_until: u64,
    pub level: i32,
    pub experience: u64,
    pub experience_fraction: f64,
    pub max_mp: i32,
    pub mp: i32,
    /// Zircon `CurrentFP` and `Stat.Focus`: the focus pool a discipline
    /// grants. Both stay 0 until the player takes discipline level 1.
    pub fp: i32,
    pub max_fp: i32,
    /// Zircon `UserDiscipline`: the level taken (0 = none) and the
    /// experience banked towards the next.
    pub discipline: i32,
    pub discipline_experience: i64,
    pub revive_time: u64,
    pub bind_region: i32,
    pub regen_time: u64,
    pub bag: Bag,
    pub event_rewards: Vec<u64>,
    pub next_item_id: u32,
    pub attack_speed: i64,
    pub max_bag: i32,
    pub max_wear: i32,
    pub max_hand: i32,
    pub comfort: i32,
    pub drop_rate: i32,
    pub companion_collection: bool,
    /// C# `CompanionLevelLockN`: session-only confirmation gates.
    pub companion_reroll_enabled: u8,
    /// Open NPC dialog: (npc object, page index).
    pub npc: Option<(ObjectId, i32)>,
    pub magics: Vec<UserMagic>,
    pub magic_time: u64,
    /// Slaying's charged power attack (Zircon `CanPowerAttack`).
    pub slaying_charged: bool,
    pub thrusting_on: bool,
    pub half_moon_on: bool,
    /// Belt links (Zircon `CharacterBeltLink`), one per slot.
    pub belt: Vec<BeltLink>,
    /// Zircon `UseItemTime`: no consumable can be used before this.
    pub use_item_time: u64,
    pub buffs: Vec<Buff>,
    /// A charged warrior power attack and when the charge expires.
    pub charge: Option<(u16, u64)>,
    /// Destructive Surge stance (Zircon `CanDestructiveSurge`).
    pub surge_on: bool,
    pub dash: Option<Dash>,
    /// Zircon `Stat.LifeSteal` percent from passives.
    pub life_steal: i32,
    pub pets: Vec<ObjectId>,
    /// Moon charges (Zircon `CanAttack` on Calamity / Waning Moon).
    pub full_moon_ready: bool,
    pub waning_moon_ready: bool,
    pub flame_splash_on: bool,
    /// Zircon `UserCurrency` amounts by `CurrencyInfo` index (gold is in the bag).
    pub currencies: HashMap<i32, i64>,
    pub rebirth: i32,
    /// Zircon `NPCVals["ROLLRESULT"]`.
    pub npc_roll: Option<i32>,
    /// Zircon `ShoutExpiry` / `Account.GlobalShoutExpiry`.
    pub shout_time: u64,
    pub global_shout_time: u64,
    pub allow_group: bool,
    /// Zircon `OnlineState`: what the friend lists show for this player.
    pub online_state: u8,
    pub group: Option<u32>,
    /// Zircon `AttackMode` (`attack_mode::*`).
    pub attack_mode: u8,
    /// Zircon `PetMode` (`pet_mode::*`).
    pub pet_mode: u8,
    /// Zircon `Stat.PKPoint`; `pk_tick` is the next decay time.
    pub pk_points: i32,
    /// Zircon `Character.Fame`: held `FameInfo` index (0 = none).
    pub fame: i32,
    pub hermit_stats: BTreeMap<i32, i32>,
    pub pk_tick: u64,
    /// Zircon brown buff: end time and current state (kept in sync each tick).
    pub brown_until: u64,
    pub brown: bool,
    /// Guild id (resolved from `guilds.json` on entry).
    pub guild: Option<u32>,
    pub guild_invite: Option<ObjectId>,
    /// Guilds this player's guild is at war with, and whether the player
    /// stands on a map under conquest (Zircon `AtWar`, cached).
    pub war_guilds: Vec<u32>,
    pub conquest_map: bool,
    /// Zircon `MailTime`: no mail before this.
    pub mail_time: u64,
    /// Channelled spell in progress (Elemental Hurricane).
    pub channel: Option<magic_wave6::Channel>,
    /// Dragon Blood armed: melee hits poison.
    pub dragon_blood: bool,
    /// Soul Resonance partner.
    pub soul_link: Option<ObjectId>,
    /// Owned horse (`horse_type::*`) and whether it is being ridden.
    pub horse: u8,
    pub mounted: bool,
    /// Partner character id and name.
    pub partner: Option<(u32, String)>,
    /// Item id of the wedding ring.
    pub wedding_ring: Option<u32>,
    pub marriage_invite: Option<ObjectId>,
    pub marriage_teleport_time: u64,
    /// Zircon `Fishing`: the cast in progress.
    pub fishing: Option<gathering::FishingData>,
    /// Weapons in the furnace (Zircon `RefineInfo`).
    pub refines: Vec<StoredRefine>,
    /// Zircon `UserCompanion`s; `active_companion` is the one out.
    pub companions: Vec<StoredCompanion>,
    pub active_companion: Option<u32>,
    pub companion_unlocks: Vec<i32>,
    /// The spawned companion object and its next upkeep tick.
    pub companion: Option<ObjectId>,
    pub companion_tick: u64,
    pub storage_size: u32,
    pub trade: Option<trade::Trade>,
    /// Who asked us to trade (Zircon `TradePartnerRequest`).
    pub trade_request: Option<ObjectId>,
    /// Who invited us (Zircon `GroupInvitation`).
    pub group_invite: Option<ObjectId>,
    pub quests: Vec<crate::accounts::StoredQuest>,
    /// Account drop progress and the fortune snapshots taken from it.
    pub drops: Vec<fortune::DropProgress>,
    pub fortunes: Vec<fortune::FortuneRecord>,
    /// Passive skill stats (Zircon `GetPassiveStats` and combat checks).
    pub def_mastery: i32,
    pub phys_immunity: i32,
    pub magic_immunity: i32,
    pub vitality: i32,
    pub last_stand: i32,
}

/// Zircon `GameNPCList` rows: named lists/values NPC scripts read and write,
/// persisted as JSON next to the accounts.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct NpcStore {
    /// category -> type value -> IntValue1.
    pub lists: HashMap<String, HashMap<String, i64>>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl NpcStore {
    fn load(path: PathBuf) -> NpcStore {
        let mut store: NpcStore = load_json(&path);
        store.path = Some(path);
        store
    }

    fn save(&self) {
        if let Some(p) = &self.path {
            save_json(p, self);
        }
    }
}

/// Shared persistence for the JSON side stores (guilds, mail, market,
/// social, NPC). A missing file is an empty store; a corrupt one refuses
/// startup instead of silently becoming empty and being saved over.
pub(super) fn load_json<T: Default + serde::de::DeserializeOwned>(path: &std::path::Path) -> T {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            panic!(
                "{}: cannot parse: {e}; fix or move the file",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => T::default(),
        Err(e) => panic!("{}: {e}", path.display()),
    }
}

/// Temp file + rename, so a crash mid-write never leaves a truncated store.
pub(super) fn save_json<T: serde::Serialize>(path: &std::path::Path, value: &T) {
    let result = (|| -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(value)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        tracing::error!(path = %path.display(), "save failed: {e}");
    }
}

#[derive(Debug)]
pub struct MonsterData {
    /// Spawn-time loot eligibility, retained through database reloads.
    pub drop_set: i32,
    pub easter_event: bool,
    pub def: i32,
    pub spawn: Option<(i32, usize)>,
    pub target: Option<ObjectId>,
    pub search_time: u64,
    pub roam_time: u64,
    pub dead_time: u64,
    pub struck_time: u64,
    pub regen_time: u64,
    pub attack_delay: u64,
    pub move_delay: u64,
    pub experience: f64,
    pub exp_owner: Option<ObjectId>,
    /// Player this monster fights for (tamed or summoned pets).
    pub owner: Option<ObjectId>,
    /// Zircon `ShockTime`: cannot move until then; any damage clears it.
    pub shock_until: u64,
    /// Zircon `SummonLevel`: +10 % stats per point.
    pub summon_level: i32,
    /// Pets return to the wild after this (`TameTime`).
    pub tame_until: u64,
    /// Puppets explode at this time (or sooner when targeted or struck).
    pub explode_at: Option<u64>,
    /// Burrowed / dormant: invisible and untouchable until something comes close.
    pub hidden: bool,
    pub hide_check: u64,
    /// Ranged attackers: earliest time for the next ranged shot.
    pub range_time: u64,
    /// Archers stop shooting until then after a scare.
    pub fear_time: u64,
    pub blink_time: u64,
    /// Next hit deals double (blink-in strike).
    pub bonus: bool,
    pub panic_used: bool,
    pub spell_time: u64,
    /// Boss summon phase (-1 = not started).
    pub stage: i32,
    /// Timed summons (mirror images, tornadoes, cursed dolls) vanish then.
    pub despawn_at: Option<u64>,
    /// Equipped focus affinity captured when a Mirror Image is created.
    pub mirror_element: Option<u8>,
    /// Cursed doll: the victim that suffers what the doll suffers.
    pub link: Option<ObjectId>,
    /// Chain: tethered to a leader until the time.
    pub chained: Option<(ObjectId, u64)>,
    /// Voracious ghosts: revives left, deaths so far, next revive time.
    pub revives_left: i32,
    pub death_count: i32,
    pub revive_at: u64,
    /// Timed class behaviours (weakness curse, death clouds, gate sweeps).
    pub cadence_time: u64,
    /// Next periodic boss minion replenishment, independent of combat/curses.
    pub minion_time: u64,
    pub numa_action: Option<(u64, monster_ai::NumaAction)>,
    /// Shinsu: fighting window on/off and when it ends.
    pub mode: bool,
    pub mode_time: u64,
    /// Terracotta: earliest next phase-out.
    pub phase_time: u64,
    /// `MonsterBehaviour` state: next self-heal, one-shot teleport and
    /// enrage used, enrage end.
    pub behaviour_heal_time: u64,
    pub behaviour_teleported: bool,
    pub behaviour_enraged: bool,
    pub rage_until: u64,
    pub minions: Vec<ObjectId>,
    pub master: Option<ObjectId>,
    /// Town guard (AI -1): fights wild monsters, cannot be hurt.
    pub guard: bool,
    /// Companion (owner's `StoredCompanion.index`): follows and picks up,
    /// never fights and is never a target.
    pub companion: Option<u32>,
    /// Castle guard, gate or flag.
    pub castle: Option<castle_parts::CastleRole>,
    pub room_door: Option<room_doors::RoomDoorRole>,
    /// Zircon `HarvestCount`: swings still to land before this corpse gives
    /// up its drops. 0 on a corpse that holds drops means the next swing
    /// pays out.
    pub harvest_count: i32,
    pub harvested: bool,
    /// Zircon `MonsterObject.Drops`: what each account is owed, held on the
    /// corpse instead of dropped on the floor. Empty on a monster that does
    /// not need harvesting.
    pub harvest_drops: BTreeMap<u32, Vec<UserItem>>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct NpcData {
    pub info: i32,
    pub entry_page: i32,
}

#[derive(Debug)]
pub struct ItemData {
    pub item: UserItem,
    /// Owning account while the drop is protected.
    pub owner: Option<u32>,
    pub spawn_time: u64,
    pub expire: u64,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Kind {
    Player(PlayerData),
    Monster(MonsterData),
    Npc(NpcData),
    Item(ItemData),
    Spell(SpellData),
}

#[derive(Debug)]
pub struct Object {
    pub id: ObjectId,
    pub kind: Kind,
    pub map: i32,
    pub location: Point,
    pub direction: Direction,
    pub hp: i32,
    pub max_hp: i32,
    pub dead: bool,
    pub stats: CombatStats,
    pub action_time: u64,
    pub move_time: u64,
    pub attack_time: u64,
    pub cell_time: u64,
    /// Zircon `InSafeZone`, refreshed on every move.
    pub in_safe_zone: bool,
    /// Light radius shown by the client (Zircon `MapObject.Light`).
    pub light: u8,
    pub appearance: Appearance,
    /// Objects this one currently sees (players only).
    pub visible: HashSet<ObjectId>,
    pub poisons: Vec<Poison>,
    pub heal: Option<HealBuff>,
}

impl Object {
    pub fn is_player(&self) -> bool {
        matches!(self.kind, Kind::Player(_))
    }
    pub fn player(&self) -> Option<&PlayerData> {
        match &self.kind {
            Kind::Player(p) => Some(p),
            _ => None,
        }
    }
    pub fn player_mut(&mut self) -> Option<&mut PlayerData> {
        match &mut self.kind {
            Kind::Player(p) => Some(p),
            _ => None,
        }
    }
    /// The monster half, or None for anything else. `monster_ref` panics;
    /// this is for code that holds an id of unknown kind.
    pub fn monster(&self) -> Option<&MonsterData> {
        match &self.kind {
            Kind::Monster(m) => Some(m),
            _ => None,
        }
    }
    pub fn monster_mut(&mut self) -> Option<&mut MonsterData> {
        match &mut self.kind {
            Kind::Monster(m) => Some(m),
            _ => None,
        }
    }
    pub fn is_monster(&self) -> bool {
        matches!(self.kind, Kind::Monster(_))
    }
    pub fn is_item(&self) -> bool {
        matches!(self.kind, Kind::Item(_))
    }
    pub fn is_spell(&self) -> bool {
        matches!(self.kind, Kind::Spell(_))
    }
    pub fn blocking(&self) -> bool {
        if let Kind::Monster(m) = &self.kind {
            if let Some(door) = &m.room_door {
                return door.closed;
            }
        }
        // An open castle gate is a doorway, not a body.
        if let Kind::Monster(m) = &self.kind {
            if let Some(castle_parts::CastleRole::Gate { closed, .. }) = &m.castle {
                return *closed && !self.dead;
            }
        }
        !self.dead && !self.is_item() && !self.is_spell()
    }
    /// The player behind an object: itself for players, the owner for pets.
    pub fn side(&self) -> Option<ObjectId> {
        match &self.kind {
            Kind::Player(_) => Some(self.id),
            Kind::Monster(m) => m.owner,
            _ => None,
        }
    }
    /// Zircon `CanAttackTarget`: players and their pets fight wild
    /// monsters, wild monsters fight players and pets, guards fight wild
    /// monsters and red players, and players fight players by attack mode
    /// outside safe zones. Pet modes use `World::monster_hostile_to`.
    pub fn hostile_to(&self, other: &Object) -> bool {
        if matches!(&self.kind, Kind::Monster(m) if m.room_door.is_some())
            || matches!(&other.kind, Kind::Monster(m) if m.room_door.is_some())
        {
            return false;
        }
        if other.dead || other.is_item() || other.is_spell() || matches!(other.kind, Kind::Npc(_)) {
            return false;
        }
        // Companions neither fight nor get fought.
        if matches!(&self.kind, Kind::Monster(m) if m.companion.is_some())
            || matches!(&other.kind, Kind::Monster(m) if m.companion.is_some())
        {
            return false;
        }
        // Guards police wild monsters and red names; nobody else fights
        // their own kind.
        if let Kind::Monster(me) = &self.kind {
            if me.guard {
                return match &other.kind {
                    Kind::Monster(them) => {
                        them.owner.is_none() && !them.guard && them.castle.is_none()
                    }
                    Kind::Player(p) => p.pk_points >= pvp::RED_POINT,
                    _ => false,
                };
            }
        }
        if matches!(&other.kind, Kind::Monster(them) if them.guard) {
            return false;
        }
        if let (Kind::Player(me), Kind::Player(them)) = (&self.kind, &other.kind) {
            if self.id == other.id || self.in_safe_zone || other.in_safe_zone {
                return false;
            }
            return match me.attack_mode {
                mir_proto::attack_mode::PEACE => false,
                mir_proto::attack_mode::GROUP => me.group.is_none() || me.group != them.group,
                mir_proto::attack_mode::GUILD => me.guild.is_none() || me.guild != them.guild,
                mir_proto::attack_mode::WAR_RED_BROWN => {
                    them.brown || them.pk_points >= pvp::RED_POINT || pvp::at_war(me, them)
                }
                _ => true,
            };
        }
        match (self.side(), other.side()) {
            (Some(_), None) => other.is_monster(),
            (None, Some(_)) => true,
            _ => false,
        }
    }
    pub fn has_buff(&self, kind: u16) -> bool {
        self.player()
            .map(|p| p.buffs.iter().any(|b| b.kind == kind))
            .unwrap_or(false)
    }
    pub fn state(&self) -> ObjectState {
        ObjectState {
            id: self.id,
            light: self.light,
            appearance: self.appearance.clone(),
            location: self.location,
            direction: self.direction,
            hp: self.hp,
            max_hp: self.max_hp,
            dead: self.dead,
            room_door: matches!(&self.kind, Kind::Monster(m) if m.room_door.is_some()),
        }
    }
}

#[derive(Debug)]
pub struct SpawnGroup {
    pub def: RespawnDef,
    pub points: Vec<Point>,
    pub alive: i32,
    pub next_spawn: u64,
}

#[derive(Debug)]
pub struct MapState {
    pub index: i32,
    pub file: MapFile,
    pub descriptor: MapDescriptor,
    pub spawns: Vec<SpawnGroup>,
    pub objects: Vec<ObjectId>,
    cells: HashMap<(i32, i32), Vec<ObjectId>>,
    /// Cells that trigger travel: indices into `GameData::movements`.
    movements: HashMap<(i32, i32), Vec<usize>>,
}

impl MapState {
    pub fn objects_at(&self, p: Point) -> &[ObjectId] {
        self.cells
            .get(&(p.x, p.y))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
    fn remove_from_cell(&mut self, id: ObjectId, p: Point) {
        if let Some(v) = self.cells.get_mut(&(p.x, p.y)) {
            v.retain(|o| *o != id);
        }
    }
    fn add_to_cell(&mut self, id: ObjectId, p: Point) {
        self.cells.entry((p.x, p.y)).or_default().push(id);
    }
}

/// A delayed melee hit (Zircon `DelayedAction`).
#[derive(Debug)]
struct PendingHit {
    time: u64,
    attacker: ObjectId,
    target_cell: (i32, Point),
    power: i32,
    target: Option<ObjectId>,
    /// Attack skills riding on this swing (Zircon `magics` list).
    magics: Vec<u16>,
    primary: bool,
    /// Blade Storm's delayed half: damage already computed.
    raw: bool,
    /// Elemental hits skip the dodge roll and go through MR.
    element: u8,
    /// Ranged hits do not require adjacency when they land.
    ranged: bool,
}

/// A monster spell landing after its delay: elemental damage on targets
/// and/or on whoever stands in `cells` (with a percent scale each).
#[derive(Debug)]
struct PendingMonsterSpell {
    time: u64,
    caster: ObjectId,
    targets: Vec<ObjectId>,
    cells: Vec<(Point, i32)>,
    power: i32,
    element: u8,
    map: i32,
}

/// A spell whose effect lands after its travel/cast delay (Zircon `DelayMagic`).
#[derive(Debug)]
struct PendingMagic {
    time: u64,
    caster: ObjectId,
    magic: u16,
    target: Option<ObjectId>,
    location: Point,
    /// Repulsion: direction to push.
    direction: Option<Direction>,
    /// Line spells: centre cells hit for full power, flanks for 30 %.
    primary: bool,
    /// Chain Lightning: (power divisor, cells already struck).
    chain: Option<(i32, Vec<Point>)>,
}

/// Outgoing message with routing info.
#[derive(Debug)]
pub enum Outgoing {
    To(ConnId, ServerMessage),
}

pub struct World {
    pub data: GameData,
    /// A bag changed in a way that is already persisted elsewhere (market,
    /// mail); main saves accounts this tick instead of waiting.
    pub account_dirty: bool,
    map_dir: PathBuf,
    /// Ordered maps so ticks iterate deterministically (seeded tests).
    pub maps: BTreeMap<i32, MapState>,
    pub objects: BTreeMap<ObjectId, Object>,
    /// Ids of every player object, kept in sync by `insert_object` /
    /// `remove_object`; ordered like `objects` so scans stay deterministic.
    players: BTreeSet<ObjectId>,
    next_id: u32,
    pub now: u64,
    /// Startup options; defaults preserve existing drop and XP behavior.
    pub drop_specials: bool,
    pub experience_multiplier: f64,
    pub inventory_tab_price: u32,
    pub storage_tab_price: u32,
    /// Monotonic event deadline; configured with ZIRCON_EASTER_EVENT_END (UTC Unix seconds).
    pub easter_event_until: u64,
    rotating_event: world_events::WorldEventState,
    /// Seeded from `ZIRCON_SEED` when set (tests), otherwise from the OS.
    rng: rand::rngs::StdRng,
    pending_hits: Vec<PendingHit>,
    /// Zircon `DayTime`: 0 night .. 1 day, broadcast on change.
    pub day_time: f32,
    pub npc_store: NpcStore,
    /// NPC page scripts (Lua).
    pub scripts: lua::ScriptEngine,
    /// Script file of the page whose checks/actions are running.
    npc_script: Option<String>,
    pending_monster_spells: Vec<PendingMonsterSpell>,
    ai_profiles: HashMap<i32, ai_profile::AiProfile>,
    pending_magics: Vec<PendingMagic>,
    /// Events raised this tick: (subject object, message).
    events: Vec<(ObjectId, ServerMessage)>,
    next_item_expiry: u64,
    pub outgoing: Vec<Outgoing>,
    /// `ZIRCON_DEV_FISHING`: item every unwalkable cell yields when the
    /// pack has no fishing zones (tackle checks waived).
    pub dev_fishing_item: Option<i32>,
    /// Gate destinations (Zircon `Config.MysteryShipRegionIndex` /
    /// `LairRegionIndex`): `ZIRCON_MYSTERY_SHIP_REGION`, `ZIRCON_LAIR_REGION`.
    pub gate_regions: [Option<i32>; 2],
    /// Groups by id; the first member leads.
    groups: BTreeMap<u32, Vec<ObjectId>>,
    pub guild_store: guilds::GuildStore,
    /// The castle war in progress, if any.
    pub conquest: Option<castles::Conquest>,
    conquest_check: u64,
    dev_conquest_done: bool,
    /// (castle, day) wars already opened, so a start window fires once.
    conquests_started: HashSet<(i32, u64)>,
    pub mail_store: mail::MailStore,
    /// Ranking baselines, so the board can show how far a name has moved.
    ranking: ranking::Ranking,
    /// Friends and blocks (`social.json`).
    pub social_store: social::SocialStore,
    /// `ZIRCON_DEV_FRIENDS` only: states for the seeded friends, which have
    /// no real character behind them.
    dev_states: HashMap<u32, u8>,
    pub market_store: market::MarketStore,
    /// Characters divorced while offline (drained by the account registry).
    divorced: Vec<u32>,
    next_group: u32,
    last_spawn_check: u64,
    force_map: Option<String>,
    drops_by_monster: HashMap<i32, Vec<DropDef>>,
}

pub mod ai_profile;
pub mod appearance;
mod buffs;
mod castle_parts;
mod castles;
mod chat;
mod combat;
mod companion;
pub mod discipline;
pub mod fame;
pub mod fortune;
mod gathering;
mod gm;
mod groups;
pub mod guilds;
pub mod harvest;
mod inventory;
mod lua;
mod magic;
mod magic_wave4;
mod magic_wave5;
mod magic_wave6;
mod mail;
mod maps;
pub mod market;
mod marriage;
mod monster_ai;
mod mounts;
mod movement;
mod npc;
mod player;
mod pvp;
mod quests;
pub mod ranking;
pub mod refine;
mod room_doors;
mod skills;
pub mod social;
mod spawn;
mod spells;
mod storage;
pub mod store;
mod test_api;
pub mod timers;
mod trade;
mod visibility;
pub mod weapon_craft;
mod world_events;

pub use companion::StoredCompanion;
pub use refine::StoredRefine;

impl World {
    /// Replace the RNG with a seeded one so a test run is reproducible.
    #[cfg(test)]
    pub fn with_seed(mut self, seed: u64) -> World {
        self.rng = rand::SeedableRng::seed_from_u64(seed);
        self
    }

    fn drops_by_monster(data: &GameData) -> HashMap<i32, Vec<DropDef>> {
        let mut drops_by_monster: HashMap<i32, Vec<DropDef>> = HashMap::new();
        for d in &data.drops {
            drops_by_monster
                .entry(d.monster)
                .or_default()
                .push(d.clone());
        }
        drops_by_monster
    }

    /// Hot reload: swap in freshly loaded game data. Live objects keep their
    /// state; new definitions apply to the next spawn, stat refresh, drop or
    /// purchase. Spawn groups of loaded maps are refreshed in place.
    pub fn reload_data(&mut self, data: GameData) {
        self.drops_by_monster = World::drops_by_monster(&data);
        self.data = data;
        for map in self.maps.values_mut() {
            for g in &mut map.spawns {
                if let Some(def) = self.data.respawns.iter().find(|r| r.index == g.def.index) {
                    g.def = def.clone();
                }
            }
        }
        let players: Vec<ObjectId> = self.players().map(|o| o.id).collect();
        for id in players {
            self.refresh_stats(id, false);
            self.send_discipline(id);
            self.store_login(id);
        }
    }

    pub fn new(data: GameData, map_dir: impl AsRef<Path>, force_map: Option<String>) -> World {
        let drops_by_monster = World::drops_by_monster(&data);
        World {
            drops_by_monster,
            data,
            map_dir: map_dir.as_ref().to_path_buf(),
            maps: BTreeMap::new(),
            objects: BTreeMap::new(),
            players: BTreeSet::new(),
            next_id: 1,
            now: 0,
            drop_specials: false,
            experience_multiplier: 1.0,
            inventory_tab_price: 100,
            storage_tab_price: 200,
            easter_event_until: easter_event_deadline(),
            rotating_event: world_events::WorldEventState::default(),
            rng: match std::env::var("ZIRCON_SEED")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
            {
                Some(seed) => rand::SeedableRng::seed_from_u64(seed),
                None => rand::SeedableRng::from_os_rng(),
            },
            pending_hits: Vec::new(),
            day_time: 1.0,
            npc_store: NpcStore::default(),
            scripts: lua::ScriptEngine::new(),
            npc_script: None,
            pending_monster_spells: Vec::new(),
            ai_profiles: HashMap::new(),
            pending_magics: Vec::new(),
            events: Vec::new(),
            next_item_expiry: 0,
            outgoing: Vec::new(),
            account_dirty: false,
            groups: BTreeMap::new(),
            guild_store: guilds::GuildStore::default(),
            ranking: ranking::Ranking::default(),
            mail_store: mail::MailStore::default(),
            social_store: social::SocialStore::default(),
            dev_states: HashMap::new(),
            market_store: market::MarketStore::default(),
            conquest: None,
            conquest_check: 0,
            dev_conquest_done: false,
            conquests_started: HashSet::new(),
            divorced: Vec::new(),
            next_group: 1,
            last_spawn_check: 0,
            force_map,
            dev_fishing_item: std::env::var("ZIRCON_DEV_FISHING")
                .ok()
                .and_then(|v| v.parse().ok()),
            gate_regions: [
                std::env::var("ZIRCON_MYSTERY_SHIP_REGION")
                    .ok()
                    .and_then(|v| v.parse().ok()),
                std::env::var("ZIRCON_LAIR_REGION")
                    .ok()
                    .and_then(|v| v.parse().ok()),
            ],
        }
    }

    /// Persist NPC lists/values under `dir` (`npc_lists.json`).
    pub fn set_store_dir(&mut self, dir: &Path) {
        self.npc_store = NpcStore::load(dir.join("npc_lists.json"));
        self.guild_store = guilds::GuildStore::load(dir.join("guilds.json"));
        self.mail_store = mail::MailStore::load(dir.join("mail.json"));
        self.social_store = social::SocialStore::load(dir.join("social.json"));
        self.market_store = market::MarketStore::load(dir.join("market.json"));
        self.rotating_event = world_events::WorldEventState::load(dir.join("world_event.json"));
        // ZIRCON_DEV_MARKET=<n> seeds a board of n listings for screenshots.
        if self.market_store.listings.is_empty() {
            if let Some(n) = std::env::var("ZIRCON_DEV_MARKET")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
            {
                self.dev_seed_market(n);
            }
        }
        self.scripts.set_root(dir.join("scripts").join("npc"));
    }

    /// Objects on a map, in insertion order (empty when the map is not loaded).
    pub fn on_map(&self, map: i32) -> impl Iterator<Item = &Object> + '_ {
        self.maps
            .get(&map)
            .into_iter()
            .flat_map(|m| m.objects.iter())
            .map(|id| &self.objects[id])
    }

    /// Every player object, in id order.
    pub fn players(&self) -> impl Iterator<Item = &Object> + '_ {
        self.players.iter().map(|id| &self.objects[id])
    }

    fn alloc_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_id);
        self.next_id += 1;
        id
    }

    // ---- tick ----------------------------------------------------------------

    pub fn tick(&mut self, now: u64) {
        if self.guild_store.dirty {
            self.guild_store.dirty = false;
            self.guild_store.save();
        }
        self.now = now;

        // Player timers: regen and revive.
        let players: Vec<ObjectId> = self.players().map(|o| o.id).collect();
        if now >= self.next_item_expiry {
            self.next_item_expiry = now.saturating_add(1000);
            for id in &players {
                self.expire_items(*id);
            }
        }
        for id in &players {
            if self.objects[id].player().is_some_and(|p| p.superman) {
                let o = self.objects.get_mut(id).unwrap();
                let changed = o.hp != o.max_hp || o.player().is_some_and(|p| p.mp != p.max_mp);
                o.hp = o.max_hp;
                let p = o.player_mut().unwrap();
                p.mp = p.max_mp;
                if changed {
                    self.send_player_stats(*id);
                }
            }
            let (dead, revive_time, regen_due) = {
                let o = &self.objects[id];
                let p = o.player().unwrap();
                (o.dead, p.revive_time, now >= p.regen_time)
            };
            if dead {
                if now >= revive_time {
                    if let Err(e) = self.revive_player(*id) {
                        tracing::warn!("revive failed: {e}");
                    }
                }
                continue;
            }
            if regen_due {
                let o = self.objects.get_mut(id).unwrap();
                let p = o.player_mut().unwrap();
                p.regen_time = now + (15_000_i32 - p.comfort.clamp(0, 20) * 650) as u64;
                let mut rate = if p.class == Class::Wizard { 0.03 } else { 0.02 };
                // Rejuvenation: +0.5 % per level above the base.
                if let Some(m) = p
                    .magics
                    .iter()
                    .find(|m| m.magic == magic_type::REJUVENATION)
                {
                    rate += (0.5 + m.level as f32 * 0.5) / 100.0;
                }
                // Mana and focus refill at the same rate (Zircon regen);
                // one stats message covers whichever of them moved.
                let mana_due = p.mp < p.max_mp;
                let focus_due = p.fp < p.max_fp;
                if mana_due {
                    p.mp = (p.mp + (p.max_mp as f32 * rate).max(1.0) as i32).min(p.max_mp);
                }
                if focus_due {
                    p.fp = (p.fp + (p.max_fp as f32 * rate).max(1.0) as i32).min(p.max_fp);
                }
                if mana_due || focus_due {
                    let stats = self.player_stats(&self.objects[id]);
                    self.send_to(*id, ServerMessage::StatsChanged(stats));
                }
                let o = self.objects.get_mut(id).unwrap();
                if o.hp < o.max_hp {
                    o.hp = (o.hp + (o.max_hp as f32 * 0.02).max(1.0) as i32).min(o.max_hp);
                    let (hp, max_hp) = (o.hp, o.max_hp);
                    self.events.push((
                        *id,
                        ServerMessage::HealthChanged {
                            id: *id,
                            hp,
                            max_hp,
                        },
                    ));
                    let stats = self.player_stats(&self.objects[id]);
                    self.send_to(*id, ServerMessage::StatsChanged(stats));
                }
            }
        }

        // Monsters near players are active (Zircon only processes objects with NearByPlayers).
        let player_positions: Vec<(i32, Point)> = players
            .iter()
            .map(|id| (self.objects[id].map, self.objects[id].location))
            .collect();
        let expired: Vec<ObjectId> = self
            .objects
            .values()
            .filter(|o| matches!(&o.kind, Kind::Item(i) if now >= i.expire))
            .map(|o| o.id)
            .collect();
        for id in expired {
            self.remove_object(id);
        }
        let active: Vec<ObjectId> = self
            .objects
            .values()
            .filter(|o| o.is_monster())
            .filter(|o| {
                o.dead
                    || o.monster().and_then(|m| m.owner).is_some_and(|owner| {
                        self.objects.get(&owner).is_some_and(Object::is_player)
                    })
                    || player_positions
                        .iter()
                        .any(|(m, p)| *m == o.map && p.distance(o.location) <= MAX_VIEW_RANGE + 2)
            })
            .map(|o| o.id)
            .collect();
        for id in active {
            self.process_monster(id);
        }

        self.resolve_hits();
        self.resolve_magics();
        self.process_poisons();
        self.process_heals();
        self.process_buffs();
        self.process_channels();
        self.process_charges();
        self.process_dashes();
        self.process_spells();
        self.process_monster_spells();
        self.process_day_time();
        self.process_pk();
        self.process_guild_wars();
        self.process_conquests();
        self.process_fishing();
        self.process_companions();

        if now >= self.last_spawn_check + 1000 {
            self.last_spawn_check = now;
            self.process_world_events_at(crate::accounts::now_secs());
            let maps: Vec<i32> = self.maps.keys().copied().collect();
            for m in maps {
                self.do_spawns(m);
            }
        }

        self.update_visibility();
        self.flush_events();
    }
}

impl Object {
    #[cfg(test)]
    pub fn monster_def(&self) -> i32 {
        self.monster_ref().def
    }
    fn monster_ref(&self) -> &MonsterData {
        match &self.kind {
            Kind::Monster(m) => m,
            _ => panic!("not a monster"),
        }
    }
}

/// Zircon `SEnvir.CalculateLights`: game minutes run `ZIRCON_DAY_CYCLE`
/// (default 12) times faster than real ones; dawn 05:00-08:00 ramps up,
/// dusk 17:00-20:00 ramps down.
pub fn day_time_now() -> f32 {
    // `ZIRCON_DAY_TIME=0..1` pins the daylight (screenshots of night maps).
    if let Some(fixed) = std::env::var("ZIRCON_DAY_TIME")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        return fixed.clamp(0.0, 1.0);
    }
    let cycle: u64 = std::env::var("ZIRCON_DAY_CYCLE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let minutes = (secs / 60 * cycle) % 1440;
    let m = minutes as f32;
    match minutes {
        0..=299 => 0.0,
        300..=479 => (m - 300.0) / 180.0,
        480..=1019 => 1.0,
        1020..=1199 => 1.0 - (m - 1020.0) / 180.0,
        _ => 0.0,
    }
}

impl World {
    /// Recompute the daylight and tell everyone when it visibly changed.
    fn process_day_time(&mut self) {
        let next = day_time_now();
        if (next * 255.0).round() == (self.day_time * 255.0).round() {
            return;
        }
        self.day_time = next;
        let players: Vec<ObjectId> = self.players().map(|o| o.id).collect();
        for id in players {
            self.send_to(id, ServerMessage::DayChanged { day_time: next });
        }
    }
}

/// Convert an explicit wall-clock deadline once; gameplay uses the world's monotonic clock.
fn easter_event_deadline() -> u64 {
    let end = match std::env::var("ZIRCON_EASTER_EVENT_END") {
        Ok(value) => match value.parse::<u64>() {
            Ok(end) => end,
            Err(_) => {
                tracing::warn!("ZIRCON_EASTER_EVENT_END must be UTC Unix seconds; event disabled");
                return 0;
            }
        },
        Err(_) => return 0,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    (u128::from(end) * 1000)
        .saturating_sub(now)
        .min(u128::from(u64::MAX)) as u64
}
