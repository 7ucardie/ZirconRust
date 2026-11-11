//! Game state, input and scene rendering.
//!
//! Screen geometry follows `MapControl` in Zircon: 48x32 cells, the player's
//! cell centred horizontally and 34 px above centre, back tiles drawn 2x2,
//! middle/front tiles bottom-anchored, objects y-sorted per row.

use std::collections::HashMap;

use mir_formats::zl::SurfaceKind;
use mir_formats::MapFile;
use mir_proto::{
    Action, Appearance, BeltLink, CharacterSummary, Class, ClientMessage, Direction, Gender,
    ItemInstance, ObjectId, ObjectState, PlayerStats, Point, ServerMessage, Weights,
    EQUIPMENT_SIZE, INVENTORY_SIZE, MAX_BELT, MAX_INVENTORY_SIZE,
};

use crate::anim::{ClientObject, Queued};
use crate::assets::{
    armour_library, helmet_library, kr_library, lib, shield_library, weapon_library, Assets,
};
use crate::chat_panel::ChatPanel;
use crate::effects::{self, Anchor, Effect, Projectile};
use crate::gfx::{Blend, Gpu, SpriteKey, SpriteRegion, SpriteRenderer, Surface};
use crate::items::{ItemCatalog, ItemDef};
use crate::net::Connection;
use crate::sound_table;
use crate::text::TextLayer;
use crate::ui::{Ctx, Input};
use crate::windows::{Bag, NpcDialog, WindowState};
use mir_formats::monster_table::monster_sprite;
use mir_proto::Grid;
use mir_proto::{buff_type, magic_type, MagicSummary};
use sound::{map_effect_sound, music_index, object_effect_sound};

/// Client-side fishing state: the float, whether a fish is on, the reel
/// click since the last recast, and the server's point tally.
pub struct FishingUi {
    pub direction: Direction,
    pub float: Point,
    pub found: bool,
    pub reeled: bool,
    pub next_cast: u64,
    pub points: i32,
    pub required: i32,
}

/// Zircon `AttackMode` descriptions.
pub fn attack_mode_name(mode: u8) -> &'static str {
    match mode {
        mir_proto::attack_mode::PEACE => "Peaceful",
        mir_proto::attack_mode::GROUP => "Group",
        mir_proto::attack_mode::GUILD => "Guild",
        mir_proto::attack_mode::WAR_RED_BROWN => "War, Red, Brown",
        _ => "All",
    }
}

pub fn pet_mode_name(mode: u8) -> &'static str {
    match mode {
        mir_proto::pet_mode::BOTH => "Move + Attack",
        mir_proto::pet_mode::MOVE => "Move",
        mir_proto::pet_mode::ATTACK => "Attack",
        mir_proto::pet_mode::PVP => "PvP",
        _ => "None",
    }
}
use std::collections::{HashMap as StdHashMap, HashSet};

pub const CELL_W: i32 = 48;
pub const CELL_H: i32 = 32;
const MANUAL_HEIGHT_OFFSET: i32 = 34;
use mir_proto::rules::{
    attack_delay, use_item_lock, ATTACK_TIME, HARVEST_TIME, MOVE_TIME, TURN_TIME,
};

/// A spell payload waiting for the cast animation: (time, magic, caster cell, targets, cells).
type PendingPayload = (u64, u16, Point, Vec<ObjectId>, Vec<Point>);

pub struct Game {
    pub assets: Assets,
    /// A map was loaded: the renderer drops its sprite cache before the next frame.
    pub atlas_reset: bool,
    pub audio: crate::audio::Audio,
    /// The character we entered the world with (name, look).
    pub character: Option<CharacterSummary>,
    pub status: String,
    map: Option<MapFile>,
    map_name: String,
    map_file: String,
    /// `MapInfo.Light` of the current map and the server's daylight.
    map_light: u8,
    /// `MapInfo.MiniMap` of the current map (0 = none).
    mini_map_index: i32,
    minimap: minimap::MiniMap,
    portal_markers: Vec<mir_formats::navigation::Portal>,
    /// Server-driven countdowns on the HUD (Zircon `TimerDialog`).
    timers: timers::Timers,
    /// Drop-fortune window and the snapshots the server last sent.
    fortune: crate::fortune::FortuneWindow,
    dungeon_codex: crate::dungeon_codex::DungeonCodex,
    wardrobe: crate::wardrobe::WardrobeWindow,
    fortunes: Vec<mir_proto::FortuneSummary>,
    /// The chat-tab editor (Zircon `ChatOptionsDialog`).
    chat_options: crate::chat_options::ChatOptions,
    character_services: crate::character_services::CharacterServices,
    day_time: f32,
    objects: HashMap<ObjectId, ClientObject>,
    user: Option<ObjectId>,
    stats: PlayerStats,
    pub mouse: (f32, f32),
    pub lmb: bool,
    pub rmb: bool,
    action_time: u64,
    move_time: u64,
    attack_time: u64,
    animation: u32,
    animation_time: u64,
    /// Chat lines: text, time, colour.
    /// The chat log, its tabs and the chat bar (Zircon `ChatTab`).
    chat: ChatPanel,
    auto_chat_done: bool,
    /// When the local fishing float effect must be replayed.
    float_time: u64,
    auto_mount_done: bool,
    hovered: Option<ObjectId>,
    item_label_rects: Vec<(ObjectId, f32, f32, f32)>,
    /// Zircon `MonsterDialog.Monster`: what the target panel describes.
    /// Latched from the last hovered monster or player.
    target: Option<ObjectId>,
    pub debug: bool,
    pub input: Input,
    pub catalog: ItemCatalog,
    inventory: Vec<Option<ItemInstance>>,
    equipment: Vec<Option<ItemInstance>>,
    /// Account storage (Zircon `Storage`), sized by the server.
    storage: Vec<Option<ItemInstance>>,
    /// Open trade, if any.
    trade: Option<crate::windows::TradeState>,
    trade_request: Option<String>,
    gold: u64,
    weights: Weights,
    windows: WindowState,
    /// Object we are walking toward to interact with (NPC or ground item).
    goal: Option<ObjectId>,
    windows_open_last_frame: bool,
    ui_hover_last_frame: bool,
    pending_messages: Vec<ClientMessage>,
    magics: Vec<MagicSummary>,
    /// Quest log (Zircon `ClientUserQuest`).
    quests: Vec<mir_proto::UserQuestSummary>,
    /// Group members in join order (the first leads).
    group: Vec<(ObjectId, String)>,
    allow_group: bool,
    /// Zircon `AttackMode` (`attack_mode::*`), cycled with H.
    attack_mode: u8,
    /// Zircon `PetMode` (`pet_mode::*`), cycled with Ctrl+A.
    pet_mode: u8,
    guild: Option<mir_proto::GuildSummary>,
    /// Our own cast in progress (Zircon `MapControl.FishingState`).
    fishing: Option<FishingUi>,
    /// Mailbox (Zircon `MailList`), newest last.
    mail: Vec<mir_proto::MailSummary>,
    /// Castles: (index, name, owner guild), and the one under conquest.
    castles: Vec<(i32, String, String)>,
    conquest: Option<i32>,
    /// The last ranking page the server sent (Zircon `S.Rankings`).
    ranking: crate::ranking::RankingView,
    /// Marriage partner, wedding ring item id, and a pending proposal.
    partner: Option<String>,
    wedding_ring: Option<u32>,
    marriage_invite: Option<String>,
    /// Weapons in the furnace (Zircon `RefineList`).
    refines: Vec<mir_proto::RefineSummary>,
    companions: Vec<mir_proto::CompanionSummary>,
    /// Offers of the open CompanionManage page.
    companion_shop: Vec<mir_proto::CompanionOffer>,
    /// Every currency the player holds (Zircon `CurrencyDialog`).
    currencies: Vec<mir_proto::CurrencySummary>,
    /// Friends, blocked names and the state we advertise (Zircon
    /// `CommunicationDialog`'s friend and block tabs).
    friends: Vec<mir_proto::FriendSummary>,
    blocks: Vec<mir_proto::BlockSummary>,
    online_state: u8,
    friends_window: crate::friends::FriendsWindow,
    /// Pending guild invite: (who, guild).
    guild_invite: Option<(String, String)>,
    /// Pending invite: who asked.
    group_invite: Option<String>,
    auto_group_done: bool,
    /// Stance skills currently on (Thrusting, Half Moon).
    toggles: HashSet<u16>,
    /// Slaying's power attack is charged.
    slaying_ready: bool,
    cooldowns: StdHashMap<u16, u64>,
    magic_time: u64,
    effects: Vec<Effect>,
    projectiles: Vec<Projectile>,
    /// Spell payloads waiting for the cast animation: (time, magic, caster cell, targets, cells).
    pending_payloads: Vec<PendingPayload>,
    /// Belt links, one per slot (Zircon `BeltDialog.Links`).
    belt: Vec<BeltLink>,
    belt_key_pending: Option<(u8, u64)>,
    /// Zircon `GameScene.UseItemTime`: no consumable before this.
    use_item_time: u64,
    /// Own buffs: (kind, client time it ends; MAX = never).
    buffs: Vec<(u16, u64)>,
    /// A charged warrior power attack waiting for the next swing.
    charged: Option<u16>,
    /// Lotus skill armed for the next swing (Zircon `User.AttackMagic`).
    armed_lotus: Option<u16>,
    /// Moon charge the server armed (Calamity Of Full Moon / Waning Moon).
    auto_charged: Option<u16>,
}

struct View {
    off_x: i32,
    off_y: i32,
    pox: i32,
    poy: i32,
    ux: i32,
    uy: i32,
    umx: i32,
    umy: i32,
}

impl View {
    fn new(w: i32, h: i32, user: Option<&ClientObject>) -> View {
        let off_x = w / 2 / CELL_W;
        let off_y = h / 2 / CELL_H;
        let (ux, uy, umx, umy) = user
            .map(|u| {
                (
                    u.location.x,
                    u.location.y,
                    u.moving_offset.0,
                    u.moving_offset.1,
                )
            })
            .unwrap_or((0, 0, 0, 0));
        View {
            off_x,
            off_y,
            pox: (w - CELL_W) / 2 - off_x * CELL_W,
            poy: (h - CELL_H) / 2 - off_y * CELL_H - MANUAL_HEIGHT_OFFSET,
            ux,
            uy,
            umx,
            umy,
        }
    }
    /// Top-aligned pixel position of a cell.
    fn cell_px(&self, x: i32, y: i32) -> (i32, i32) {
        (
            (x - self.ux + self.off_x) * CELL_W + self.pox - self.umx,
            (y - self.uy + self.off_y) * CELL_H + self.poy - self.umy,
        )
    }
    fn object_px(&self, o: &ClientObject) -> (i32, i32) {
        let (x, y) = self.cell_px(o.location.x, o.location.y);
        (x + o.moving_offset.0, y + o.moving_offset.1)
    }
    fn cell_at(&self, mx: f32, my: f32) -> Point {
        let cx = ((mx as i32 - self.pox + self.umx).div_euclid(CELL_W)) + self.ux - self.off_x;
        let cy = ((my as i32 - self.poy + self.umy).div_euclid(CELL_H)) + self.uy - self.off_y;
        Point::new(cx, cy)
    }
}

#[allow(clippy::too_many_arguments)]
mod fx;
mod hud;
mod input;
mod minimap;
mod net;
mod render;
mod sound;
mod timers;

impl Game {
    pub fn take_pending_resolution(&mut self) -> Option<(u32, u32)> {
        self.windows.settings.pending_resolution.take()
    }

    pub fn new(mut assets: Assets, catalog: ItemCatalog) -> Game {
        let audio = crate::audio::Audio::new(assets.root());
        crate::anim::set_fishing_frames(assets.info(lib::M_HUM, 2000).is_some());
        // Some packs contain only 920..922; never animate through absent fire frames.
        let firewall_frames = (920..925)
            .take_while(|&index| assets.info(effects::MAGIC, index).is_some())
            .count();
        crate::anim::set_firewall_frames(firewall_frames as u32);
        // Windows that own their open flag read ZIRCON_OPEN themselves.
        let opened = |name: &str| {
            std::env::var("ZIRCON_OPEN")
                .unwrap_or_default()
                .split(',')
                .any(|n| n.trim() == name)
        };
        Game {
            assets,
            atlas_reset: false,
            audio,
            catalog,
            input: Input::default(),
            inventory: (0..INVENTORY_SIZE).map(|_| None).collect(),
            storage: Vec::new(),
            trade: None,
            trade_request: None,
            equipment: (0..EQUIPMENT_SIZE).map(|_| None).collect(),
            gold: 0,
            weights: Weights {
                bag: 0,
                max_bag: 1,
                wear: 0,
                max_wear: 1,
                hand: 0,
                max_hand: 1,
            },
            windows: {
                let mut w = WindowState::default();
                w.inventory_open = std::env::var_os("ZIRCON_OPEN_WINDOWS").is_some();
                w.character_open = w.inventory_open;
                w.skills_open = std::env::var_os("ZIRCON_OPEN_SKILLS").is_some();
                w.storage_open = std::env::var_os("ZIRCON_OPEN_STORAGE").is_some();
                // ZIRCON_OPEN=storage,group,guild,mail,quests opens windows on entry.
                for name in std::env::var("ZIRCON_OPEN").unwrap_or_default().split(',') {
                    match name.trim() {
                        "storage" => w.storage_open = true,
                        "group" => w.group_open = true,
                        "guild" => w.guild_open = true,
                        "mail" => w.mail_open = true,
                        "quests" => w.quests_open = true,
                        "inventory" => w.inventory_open = true,
                        "character" => w.character_open = true,
                        "skills" => w.skills_open = true,
                        "companion" => w.companion_open = true,
                        "menu" => w.menu.menu_open = true,
                        "settings" => w.settings.open = true,
                        "help" => w.menu.help_open = true,
                        "exit" => w.menu.exit_open = true,
                        "currency" => w.menu.currency_open = true,
                        "autopotion" => w.menu.auto_potion_open = true,
                        "dropfilter" => w.menu.drop_filter_open = true,
                        "ranking" => {
                            // ZIRCON_RANK_CLASS / ZIRCON_RANK_ONLINE pick the
                            // board's filters for headless checks.
                            w.ranking.open = true;
                            w.ranking.class = crate::ranking::class_from_name(
                                &std::env::var("ZIRCON_RANK_CLASS").unwrap_or_default(),
                            );
                            w.ranking.online_only =
                                std::env::var_os("ZIRCON_RANK_ONLINE").is_some();
                        }
                        "market" => {
                            w.market.open = true;
                            w.market.refresh();
                        }
                        "marketconsign" => {
                            w.market.open = true;
                            w.market.tab = crate::market::Tab::Consign;
                            w.market.refresh();
                        }
                        "marketstore" => {
                            // ZIRCON_STORE_TAG / ZIRCON_STORE_HUNT preset the
                            // Store tab's filter and currency for headless
                            // checks; the tag is applied once the stock lands.
                            w.market.open = true;
                            w.market.tab = crate::market::Tab::Store;
                            w.market.wanted_tag = std::env::var("ZIRCON_STORE_TAG").ok();
                            w.market.hunt_gold = std::env::var_os("ZIRCON_STORE_HUNT").is_some();
                        }
                        _ => {}
                    }
                }
                w
            },
            wardrobe: Default::default(),
            goal: None,
            windows_open_last_frame: false,
            ui_hover_last_frame: false,
            pending_messages: Vec::new(),
            magics: Vec::new(),
            quests: Vec::new(),
            group: Vec::new(),
            allow_group: false,
            attack_mode: 0,
            pet_mode: 0,
            guild: None,
            fishing: None,
            mail: Vec::new(),
            castles: Vec::new(),
            ranking: crate::ranking::RankingView::default(),
            conquest: None,
            partner: None,
            wedding_ring: None,
            marriage_invite: None,
            refines: Vec::new(),
            companions: Vec::new(),
            companion_shop: Vec::new(),
            currencies: Vec::new(),
            friends: Vec::new(),
            blocks: Vec::new(),
            online_state: mir_proto::online_state::ONLINE,
            friends_window: crate::friends::FriendsWindow::default(),
            guild_invite: None,
            group_invite: None,
            auto_group_done: false,
            toggles: HashSet::new(),
            slaying_ready: false,
            cooldowns: StdHashMap::new(),
            magic_time: 0,
            effects: Vec::new(),
            projectiles: Vec::new(),
            pending_payloads: Vec::new(),
            belt: (0..MAX_BELT as u8).map(BeltLink::empty).collect(),
            belt_key_pending: None,
            use_item_time: 0,
            buffs: Vec::new(),
            charged: None,
            armed_lotus: None,
            auto_charged: None,
            character: None,
            status: String::new(),
            map: None,
            map_name: String::new(),
            map_file: String::new(),
            map_light: 0,
            mini_map_index: 0,
            minimap: minimap::MiniMap::default(),
            portal_markers: Vec::new(),
            timers: timers::Timers::default(),
            fortune: {
                let mut w = crate::fortune::FortuneWindow::default();
                w.open = opened("fortune");
                w
            },
            dungeon_codex: crate::dungeon_codex::DungeonCodex::default(),
            fortunes: Vec::new(),
            character_services: {
                let mut w = crate::character_services::CharacterServices::default();
                w.open |= opened("characterservices");
                w
            },
            chat_options: {
                let mut w = crate::chat_options::ChatOptions::default();
                w.open = opened("chatoptions");
                w
            },
            day_time: 1.0,
            objects: HashMap::new(),
            user: None,
            stats: PlayerStats {
                level: 1,
                hp: 0,
                max_hp: 1,
                mp: 0,
                max_mp: 1,
                experience: 0,
                max_experience: 100,
                min_dc: 0,
                max_dc: 0,
                min_ac: 0,
                max_ac: 0,
                min_mr: 0,
                max_mr: 0,
                min_mc: 0,
                max_mc: 0,
                min_sc: 0,
                max_sc: 0,
                accuracy: 0,
                agility: 0,
                attack_speed: 0,
                fame: 0,
                fame_title: String::new(),
                fp: 0,
                max_fp: 0,
                attack_element: 0,
                discipline: 0,
                discipline_experience: 0,
            },
            mouse: (0.0, 0.0),
            lmb: false,
            rmb: false,
            action_time: 0,
            move_time: 0,
            attack_time: 0,
            animation: 0,
            animation_time: 0,
            chat: {
                let mut c = ChatPanel::default();
                c.tabs = crate::chat_options::load_tabs();
                c.sync_tabs();
                c
            },
            auto_chat_done: false,
            float_time: 0,
            auto_mount_done: false,
            hovered: None,
            item_label_rects: Vec::new(),
            target: None,
            debug: true,
        }
    }

    /// Name of the character in play.
    pub fn user_name(&self) -> Option<&str> {
        self.character.as_ref().map(|c| c.name.as_str())
    }

    /// Something the player may attack: a living monster that is not a pet
    /// of theirs.
    fn attackable(&self, o: &ClientObject) -> bool {
        o.is_monster() && !o.dead && !(o.pet_owner().is_some() && o.pet_owner() == self.user_name())
    }

    fn user(&self) -> Option<&ClientObject> {
        self.user.and_then(|id| self.objects.get(&id))
    }

    fn user_mut(&mut self) -> Option<&mut ClientObject> {
        let id = self.user?;
        self.objects.get_mut(&id)
    }

    fn say(&mut self, text: String, now: u64) {
        self.say_colored(text, now, [255, 255, 200, 255]);
    }

    /// A client-side notice in its own colour (Zircon `MessageType.Hint`).
    fn say_colored(&mut self, text: String, now: u64, color: [u8; 4]) {
        tracing::info!("{text}");
        self.chat.push_colored(text, now, color);
    }

    /// A chat line from the server, filed under its kind.
    fn say_kind(&mut self, text: String, now: u64, kind: mir_proto::ChatKind) {
        tracing::info!("{text}");
        self.chat
            .push(text, now, crate::chat_panel::Category::from_kind(kind));
    }

    /// Chat bar keys: Enter opens the box, Enter sends, Escape closes; while
    /// it is open the keyboard belongs to it.
    fn chat_keys(&mut self, now: u64, conn: Option<&Connection>) {
        let in_world = self.user.is_some();
        if let Some(text) = self.chat.keys(&mut self.input, now, in_world) {
            if let Some(c) = conn {
                c.send(ClientMessage::Chat { text });
            }
        }
    }

    pub fn update(&mut self, now: u64, width: i32, height: i32, conn: Option<&Connection>) {
        if now >= self.animation_time + 100 {
            self.animation_time = now;
            self.animation = self.animation.wrapping_add(1);
        }
        let before = self.user().map(|u| (u.action, u.frame_index));
        for o in self.objects.values_mut() {
            o.process(now);
        }
        self.sfx_frame(before, now);
        self.advance_effects(now, width, height);
        self.hovered = self.hit_test(width, height);
        // The target panel keeps the last monster or player pointed at
        // until it dies or leaves (Zircon clears the box the same way).
        if let Some(h) = self.hovered.filter(|h| {
            self.objects
                .get(h)
                .is_some_and(|o| o.is_monster() || o.is_player())
        }) {
            self.target = Some(h);
        }
        if self
            .target
            .and_then(|t| self.objects.get(&t))
            .is_none_or(|o| o.dead)
        {
            self.target = None;
        }
        // ZIRCON_AUTO_TARGET=1 latches the nearest live monster so the target
        // panel can be screenshotted without a mouse to hover with.
        if self.target.is_none() && std::env::var_os("ZIRCON_AUTO_TARGET").is_some() {
            self.target = self.user().and_then(|u| {
                self.objects
                    .values()
                    .filter(|o| o.is_monster() && !o.dead)
                    .min_by_key(|o| o.location.distance(u.location))
                    .map(|o| o.id)
            });
        }
        // Developer automation: ZIRCON_AUTO_CAST=<F key> (or m<magic id>) casts
        // at the nearest monster, with the mouse over it for cell casts.
        if let Ok(v) = std::env::var("ZIRCON_AUTO_CAST") {
            if now >= self.magic_time {
                let nearest = self.user().and_then(|u| {
                    self.objects
                        .values()
                        .filter(|o| {
                            o.is_monster() && !o.dead && o.location.distance(u.location) <= 6
                        })
                        .min_by_key(|o| o.location.distance(u.location))
                        .map(|o| o.id)
                });
                let by_id = v.strip_prefix('m').and_then(|s| s.parse::<u16>().ok());
                // Self casts do not need a monster in reach.
                let self_cast = by_id.map(magic_type::is_self_cast).unwrap_or(false);
                if nearest.is_some() || self_cast {
                    // A parked cursor (ZIRCON_MOUSE) wins: screenshots of
                    // hover-only overlays must keep the mouse where it was put.
                    if let Some(t) = nearest.filter(|_| std::env::var_os("ZIRCON_MOUSE").is_none())
                    {
                        let view = View::new(width, height, self.user());
                        if let Some(o) = self.objects.get(&t) {
                            let (px, py) = view.object_px(o);
                            self.mouse = (px as f32 + 24.0, py as f32 + 16.0);
                        }
                        self.hovered = Some(t);
                    }
                    if let Some(id) = by_id {
                        self.use_skill(id, now, width, height, conn);
                    } else if let Ok(f) = v.parse::<u8>() {
                        self.function_key(f, now, width, height, conn);
                    }
                }
            }
        }
        // Developer automation: ZIRCON_AUTO_BELT=1 links every consumable type
        // in the bag to the belt; ZIRCON_AUTO_USE=<slot> presses that belt key.
        if std::env::var_os("ZIRCON_AUTO_BELT").is_some() && !self.inventory.is_empty() {
            let mut free: Vec<u8> = self
                .belt
                .iter()
                .filter(|l| l.info.is_none() && l.item.is_none())
                .map(|l| l.slot)
                .collect();
            free.reverse();
            let mut linked: Vec<i32> = self.belt.iter().filter_map(|l| l.info).collect();
            for (i, it) in self.inventory.clone().iter().enumerate() {
                let Some(it) = it else { continue };
                let consumable = self
                    .catalog
                    .get(it.info)
                    .map(|d| d.item_type == mir_proto::item_type::CONSUMABLE)
                    .unwrap_or(false);
                if !consumable || linked.contains(&it.info) {
                    continue;
                }
                let Some(slot) = free.pop() else { break };
                let link = crate::windows::link_for(&self.catalog, &self.inventory, i as u8, slot);
                linked.push(it.info);
                self.apply_belt_link(link);
                if let Some(c) = conn {
                    c.send(ClientMessage::BeltLink {
                        slot: link.slot,
                        info: link.info,
                        item: link.item,
                    });
                }
            }
        }
        if let Some(slot) = std::env::var("ZIRCON_AUTO_USE")
            .ok()
            .and_then(|v| v.parse::<u8>().ok())
        {
            if now > 3000 && self.use_item_time == 0 {
                self.belt_key(slot, now, conn, false);
            }
        }
        // ZIRCON_AUTO_GROUP=name invites that player 3 s after entry (and
        // allows groups); ZIRCON_AUTO_GROUP_ACCEPT=1 accepts any invite.
        if let Ok(name) = std::env::var("ZIRCON_AUTO_GROUP") {
            if now > 3000 && !self.auto_group_done && self.user.is_some() {
                self.auto_group_done = true;
                if let Some(c) = conn {
                    c.send(ClientMessage::GroupSwitch { allow: true });
                    c.send(ClientMessage::GroupInvite { name });
                }
                self.windows.group_open = true;
            }
        }
        if std::env::var_os("ZIRCON_AUTO_GROUP_ACCEPT").is_some() {
            if !self.allow_group && now > 1000 && self.user.is_some() && !self.auto_group_done {
                self.auto_group_done = true;
                if let Some(c) = conn {
                    c.send(ClientMessage::GroupSwitch { allow: true });
                }
            }
            if self.group_invite.take().is_some() {
                if let Some(c) = conn {
                    c.send(ClientMessage::GroupResponse { accept: true });
                }
                self.windows.group_open = true;
            }
        }
        // Screenshot automation: walk up to a corpse, then harvest it.
        if std::env::var_os("ZIRCON_AUTO_HARVEST").is_some()
            && now > 2000
            && self.user().is_some_and(|u| !u.dead && !u.mounted())
        {
            if let Some(corpse) = self.nearest_corpse(12) {
                let loc = self.user().unwrap().location;
                if loc.distance(corpse) <= 1 {
                    if now >= self.action_time && now >= self.move_time {
                        self.swing_harvest(corpse, now, conn);
                    }
                } else if now >= self.action_time && now >= self.move_time {
                    self.step_toward(now, loc, corpse, false, conn);
                }
            }
        }
        // ZIRCON_AUTO_MINE=1 swings the pickaxe at the nearest wall (walking
        // up to it first); ZIRCON_AUTO_FISH=1 casts at the nearest water
        // within 4 cells and reels every bite.
        if std::env::var_os("ZIRCON_AUTO_MINE").is_some() && now > 2000 && self.user.is_some() {
            if let Some(wall) = self.nearest_wall(12) {
                let user_loc = self.user().map(|u| u.location).unwrap();
                if user_loc.distance(wall) <= 1 {
                    if now >= self.action_time && now >= self.attack_time {
                        self.swing_pickaxe(wall, now, conn);
                    }
                } else if now >= self.action_time && now >= self.move_time {
                    self.step_toward(now, user_loc, wall, false, conn);
                }
            }
        }
        if std::env::var_os("ZIRCON_AUTO_FISH").is_some() && now > 2000 && self.user.is_some() {
            match &mut self.fishing {
                Some(f) => {
                    if f.found {
                        f.reeled = true;
                    }
                }
                None => {
                    if now >= self.action_time {
                        if let Some(water) = self.nearest_wall(4) {
                            self.cast_rod(water, now, conn);
                        }
                    }
                }
            }
        }
        // ZIRCON_AUTO_MOUNT=1 mounts 2 s after entering the world.
        if std::env::var_os("ZIRCON_AUTO_MOUNT").is_some()
            && now > 2000
            && !self.auto_mount_done
            && self.user.is_some()
        {
            self.auto_mount_done = true;
            if let Some(c) = conn {
                c.send(ClientMessage::Mount);
            }
        }
        // ZIRCON_AUTO_CHAT=text says it once, 2 s after entering the world.
        if let Ok(text) = std::env::var("ZIRCON_AUTO_CHAT") {
            if now > 2000 && !self.auto_chat_done && self.user.is_some() {
                self.auto_chat_done = true;
                if let Some(c) = conn {
                    c.send(ClientMessage::Chat { text });
                }
                self.chat.open = true;
            }
        }
        self.handle_input(now, width, height, conn);
        // Auto potion (Zircon `AutoPotionDialog`): drink from a belt slot when
        // a pool drops under its percentage. Client-side, same path as a
        // belt key, so the server sees an ordinary item use.
        if self.windows.menu.auto_potion.enabled && self.stats.hp > 0 && now >= self.use_item_time {
            let hp = self.stats.hp * 100 / self.stats.max_hp.max(1);
            let mp = self.stats.mp * 100 / self.stats.max_mp.max(1);
            if let Some(slot) =
                crate::menu::auto_potion_slot(&self.windows.menu.auto_potion, hp, mp)
            {
                if let Some(link) = self.belt.get(slot as usize).copied() {
                    if let Some(inv) = crate::windows::belt_inventory_slot(&self.inventory, &link) {
                        // Only drinkables, so a belt slot holding something
                        // else is not used over and over.
                        let drinkable = self
                            .inventory
                            .get(inv as usize)
                            .cloned()
                            .flatten()
                            .and_then(|i| self.catalog.get(i.info).cloned())
                            .is_some_and(|d| d.item_type == mir_proto::item_type::CONSUMABLE);
                        if drinkable {
                            self.try_use_item(inv, now, conn);
                        }
                    }
                }
            }
        }
        // Fishing: recast every attack delay, reeling if the player clicked
        // while a fish was on (Zircon FishingDialog auto-cast).
        if let Some(f) = &mut self.fishing {
            if now >= f.next_cast {
                f.next_cast = now + 1600;
                let caught = f.reeled && f.found;
                f.reeled = false;
                if let Some(c) = conn {
                    c.send(ClientMessage::FishingCast {
                        state: mir_proto::fishing_state::CAST,
                        direction: f.direction,
                        float: f.float,
                        caught,
                    });
                }
            }
        }
        // The float bobs the whole time a line is out (the server only
        // replays it on each cast).
        if let Some(f) = &self.fishing {
            if now >= self.float_time {
                self.float_time = now + 700;
                self.effects
                    .extend(effects::fishing_float(f.float, f.found, now));
            }
        }
        if let Some(u) = self.user() {
            let fishing = match &self.fishing {
                Some(f) if f.found => {
                    format!(" | Fishing: bite, click! {}/{}", f.points, f.required)
                }
                Some(f) => format!(" | Fishing: waiting {}/{}", f.points, f.required),
                None => String::new(),
            };
            self.status = format!(
                "{} ({}, {}) | Attack: {}{}",
                self.map_name,
                u.location.x,
                u.location.y,
                attack_mode_name(self.attack_mode),
                fishing
            );
        }
    }

    /// Messages produced by the windows, sent by the client shell.
    pub fn take_messages(&mut self) -> Vec<ClientMessage> {
        std::mem::take(&mut self.pending_messages)
    }
}

/// Zircon `Functions.GetAttackAnimation` for the swings we know.
fn attack_action(magic: Option<u16>) -> Action {
    match magic {
        Some(magic_type::HALF_MOON) | Some(magic_type::DESTRUCTIVE_SURGE) => Action::Attack2,
        Some(magic_type::DRAGON_RISE)
        | Some(magic_type::FULL_BLOOM)
        | Some(magic_type::WHITE_LOTUS)
        | Some(magic_type::RED_LOTUS) => Action::Attack5,
        Some(magic_type::BLADE_STORM) => Action::Attack6,
        _ => Action::Attack,
    }
}

/// Zircon `Functions.GetMagicAnimation`.
fn cast_action(magic: u16) -> Action {
    if magic_type::is_stance_anim(magic) || magic == magic_type::MASS_BECKON {
        Action::Stance
    } else if magic == magic_type::SWIFT_BLADE || magic_type::is_directional(magic) {
        Action::Attack
    } else if magic == magic_type::RAKE {
        Action::Attack5
    } else if magic_type::is_projectile_cast(magic) {
        Action::Cast1
    } else {
        Action::Cast2
    }
}

/// Zircon `BuffDialog` icon index in `CBIcons.Zl`.
fn buff_icon(kind: u16) -> u32 {
    match kind {
        buff_type::DEFIANCE => 97,
        buff_type::MIGHT => 96,
        buff_type::ENDURANCE => 95,
        buff_type::REFLECT_DAMAGE => 98,
        buff_type::RENOUNCE => 94,
        buff_type::STRENGTH_OF_FAITH => 141,
        buff_type::INVISIBILITY => 74,
        buff_type::ELEMENTAL_SUPERIORITY => 93,
        buff_type::BLOOD_LUST => 90,
        buff_type::CELESTIAL_LIGHT => 142,
        buff_type::TRANSPARENCY => 160,
        buff_type::CLOAK | buff_type::GHOST_WALK => 160,
        buff_type::INVINCIBILITY => 97,
        buff_type::EVASION => 91,
        buff_type::RAGING_WIND => 95,
        buff_type::CONCENTRATION => 96,
        buff_type::THE_NEW_BEGINNING => 160,
        buff_type::JUDGEMENT_OF_HEAVEN => 94,
        buff_type::SUPERIOR_MAGIC_SHIELD => 100,
        buff_type::DARK_CONVERSION => 90,
        buff_type::LIFE_STEAL => 90,
        buff_type::SPIRITUALISM => 92,
        buff_type::DRAGON_REPULSE => 98,
        buff_type::ELEMENTAL_SWORDS => 96,
        buff_type::MAGIC_SHIELD => 100,
        buff_type::HEAL => 78,
        buff_type::MAGIC_RESISTANCE => 92,
        buff_type::RESILIENCE => 91,
        buff_type::POISONOUS_CLOUD => 98,
        buff_type::FULL_BLOOM => 162,
        buff_type::WHITE_LOTUS => 163,
        buff_type::RED_LOTUS => 164,
        _ => 73,
    }
}

impl Game {
    /// Zircon `MapControl.Light.UpdateLights`: the colour the light target
    /// is cleared to before the blobs are added.
    pub fn light_clear(&self) -> [f32; 3] {
        if self.map.is_none() || self.user.is_none() {
            return [1.0, 1.0, 1.0];
        }
        let dead = self
            .user
            .and_then(|u| self.objects.get(&u))
            .map(|o| o.dead)
            .unwrap_or(false);
        if dead {
            return [205.0 / 255.0, 92.0 / 255.0, 92.0 / 255.0];
        }
        match self.map_light {
            1 => [1.0, 1.0, 1.0],
            2 => [15.0 / 255.0; 3],
            3 => [100.0 / 255.0; 3],
            _ => {
                let s = (255.0 * self.day_time.clamp(0.0, 1.0)).round() / 255.0;
                [s, s, s]
            }
        }
    }
}
