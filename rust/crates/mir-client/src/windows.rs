//! In-game windows: inventory, character (equipment), NPC dialog and shop.
//! Layouts follow Zircon's `InventoryDialog`, `CharacterDialog`,
//! `NPCDialog` and `NPCGoodsDialog`; the first two use the standard window
//! chrome because this asset set lacks their dedicated panel images.

use mir_proto::{
    item_type, parse_dialog, BeltLink, ClientMessage, DialogPart, Gender, Good, Grid, ItemInstance,
    EQUIPMENT_SIZE, INVENTORY_SIZE, MAX_BELT,
};
use std::collections::HashMap;

use crate::assets::lib;
use crate::gfx::Blend;
use crate::items::{stat_name, type_name, ItemCatalog, ItemDef};
use crate::menu::{self, MenuAction, MenuState};
use crate::ui::{Button, Ctx, Rect, TextBox, GOLD};
use mir_proto::ObjectId;

const CELL: f32 = 36.0;
const PITCH: f32 = 37.0;

#[derive(Default)]
pub(crate) struct WindowLayout {
    positions: HashMap<&'static str, (f32, f32)>,
    dragging: Option<(&'static str, (f32, f32))>,
}

impl WindowLayout {
    pub(crate) fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// The window's placed rectangle holds the mouse, or it is being dragged.
    pub(crate) fn contains_mouse(
        &self,
        id: &'static str,
        rect: Rect,
        mouse: (f32, f32),
        screen: (i32, i32),
    ) -> bool {
        self.rect(id, rect, screen).contains(mouse.0, mouse.1) || self.is_dragging()
    }

    pub(crate) fn end_frame(&mut self, input: &crate::ui::Input) {
        if !input.lmb_down {
            self.dragging = None;
        }
    }

    pub(crate) fn rect(&self, id: &'static str, mut rect: Rect, screen: (i32, i32)) -> Rect {
        (rect.x, rect.y) = self.positions.get(id).copied().unwrap_or((rect.x, rect.y));
        rect.x = rect.x.clamp(0.0, (screen.0 as f32 - rect.w).max(0.0));
        rect.y = rect.y.clamp(0.0, (screen.1 as f32 - rect.h).max(0.0));
        rect
    }

    pub(crate) fn place(
        &mut self,
        id: &'static str,
        mut rect: Rect,
        input: &crate::ui::Input,
        screen: (i32, i32),
    ) -> Rect {
        rect = self.rect(id, rect, screen);
        let bounds = (
            (screen.0 as f32 - rect.w).max(0.0),
            (screen.1 as f32 - rect.h).max(0.0),
        );

        if input.lmb_pressed {
            let title = Rect::new(rect.x, rect.y, rect.w, 31.0);
            let close = Rect::new(rect.x + rect.w - 24.0, rect.y + 3.0, 21.0, 21.0);
            if title.contains(input.mouse.0, input.mouse.1)
                && !close.contains(input.mouse.0, input.mouse.1)
            {
                self.dragging = Some((id, (input.mouse.0 - rect.x, input.mouse.1 - rect.y)));
            }
        }
        if let Some((active, grab)) = self.dragging {
            if active == id && input.lmb_down {
                rect.x = (input.mouse.0 - grab.0).clamp(0.0, bounds.0);
                rect.y = (input.mouse.1 - grab.1).clamp(0.0, bounds.1);
                self.positions.insert(id, (rect.x, rect.y));
            }
        }
        rect
    }
}

fn sale_count(requested: &str, available: u32) -> Option<u32> {
    if available == 0 {
        return None;
    }
    if requested.is_empty() {
        return Some(available);
    }
    requested
        .parse::<u32>()
        .ok()
        .filter(|count| *count > 0)
        .map(|count| count.min(available))
}

fn purchase_count(requested: &str, stack_size: i32) -> Option<u32> {
    let maximum = stack_size.max(1) as u32;
    if requested.is_empty() {
        return Some(1);
    }
    requested
        .parse::<u32>()
        .ok()
        .filter(|count| *count > 0)
        .map(|count| count.min(maximum))
}

#[cfg(test)]
mod shop_count_tests {
    use super::{purchase_count, sale_count};

    #[test]
    fn selected_sale_count_defaults_to_stack_and_caps_partial_sale() {
        assert_eq!(sale_count("", 8), Some(8));
        assert_eq!(sale_count("3", 8), Some(3));
        assert_eq!(sale_count("99", 8), Some(8));
        assert_eq!(sale_count("0", 8), None);
        assert_eq!(sale_count("3", 0), None);
    }

    #[test]
    fn purchase_count_defaults_to_one_and_stays_within_item_stack() {
        assert_eq!(purchase_count("", 999), Some(1));
        assert_eq!(purchase_count("25", 999), Some(25));
        assert_eq!(purchase_count("999", 20), Some(20));
        assert_eq!(purchase_count("0", 999), None);
        assert_eq!(purchase_count("2", 1), Some(1));
    }
}

#[allow(dead_code)]
pub struct NpcDialog {
    pub npc: mir_proto::ObjectId,
    pub page: i32,
    pub parts: Vec<DialogPart>,
    pub dialog_type: i32,
    pub currency: String,
    pub goods: Vec<Good>,
    pub sell_types: Vec<u8>,
    pub quests: Vec<mir_proto::NpcQuest>,
    pub selected_good: Option<usize>,
    /// Bag slots and item IDs selected for one sale. IDs prevent selling a replacement item.
    pub selected_sell: Vec<(u8, u32)>,
    buy_count: TextBox,
    buy_prompt: bool,
    sell_count: TextBox,
    pub selected_repair: Vec<(Grid, u8, u32)>,
    pub special_repair: bool,
    repair_button: Button,
    special_button: Button,
    pub last_button: u64,
    /// First visible goods row (Zircon scrolls by pixels; we scroll by rows).
    pub goods_scroll: usize,
}

impl NpcDialog {
    #[allow(clippy::too_many_arguments)] // Mirrors one NpcResponse packet.
    pub fn new(
        npc: mir_proto::ObjectId,
        page: i32,
        say: &str,
        dialog_type: i32,
        currency: String,
        goods: Vec<Good>,
        sell_types: Vec<u8>,
        quests: Vec<mir_proto::NpcQuest>,
    ) -> NpcDialog {
        NpcDialog {
            npc,
            page,
            parts: parse_dialog(say),
            dialog_type,
            currency,
            goods,
            sell_types,
            quests,
            selected_good: None,
            selected_sell: Vec::new(),
            buy_count: TextBox::new(Rect::new(0.0, 0.0, 70.0, 22.0), 6),
            buy_prompt: false,
            sell_count: TextBox::new(Rect::new(0.0, 0.0, 70.0, 22.0), 6),
            selected_repair: Vec::new(),
            special_repair: true,
            repair_button: Button::default_style(0.0, 0.0, 80.0, "Repair"),
            special_button: Button::default_style(0.0, 0.0, 140.0, "Special repair"),
            last_button: 0,
            goods_scroll: 0,
        }
    }

    fn toggle_repair(&mut self, grid: Grid, slot: u8, id: u32) {
        let cell = (grid, slot, id);
        if let Some(i) = self.selected_repair.iter().position(|&entry| entry == cell) {
            self.selected_repair.remove(i);
        } else if self.selected_repair.len() < 55 {
            self.selected_repair.push(cell);
        }
    }

    fn toggle_sell(&mut self, slot: u8, id: u32) {
        let key = (slot, id);
        if let Some(index) = self.selected_sell.iter().position(|entry| *entry == key) {
            self.selected_sell.remove(index);
        } else {
            self.selected_sell.push(key);
        }
    }
}

fn repairable(item: &ItemInstance, def: &ItemDef, types: &[u8]) -> bool {
    def.can_repair
        && def.durability > 0
        && item.durability < item.max_durability
        && types.contains(&def.item_type)
        && matches!(
            def.item_type,
            item_type::WEAPON
                | item_type::ARMOUR
                | item_type::HELMET
                | item_type::NECKLACE
                | item_type::BRACELET
                | item_type::RING
                | item_type::SHOES
                | item_type::SHIELD
        )
}

fn repair_cost(item: &ItemInstance, def: &ItemDef, special: bool) -> u64 {
    let half = def.price as f64 / 2.0;
    let full = (item.max_durability as f64 * half / def.durability as f64 + half).floor();
    let ratio = item.durability.max(0) as f64 / item.max_durability.max(1) as f64;
    let worn = (item.max_durability as f64 * half / def.durability as f64 * (1.0 + ratio) / 2.0
        + half)
        .floor();
    let stats = 1.0
        + item
            .added
            .iter()
            .map(|(_, amount)| amount.unsigned_abs() as u64)
            .sum::<u64>() as f64
            * 0.1;
    let sale_bonus = [(59, 1.20), (58, 1.15), (57, 1.10), (56, 1.05)]
        .into_iter()
        .find_map(|(stat, rate)| {
            let needed = def
                .stats
                .iter()
                .find(|(id, _)| *id == stat)
                .map(|(_, n)| *n)
                .unwrap_or(0);
            (needed > 0 && needed as u32 <= item.count).then_some(rate)
        })
        .unwrap_or(1.0);
    let current = if item.worthless {
        0.0
    } else {
        (worn * stats * item.count as f64 * def.sell_rate * sale_bonus)
            .max(0.0)
            .trunc()
    };
    let cost = (full * stats * item.count as f64 - current).max(0.0) as u64;
    cost.saturating_mul(if special { 2 } else { 1 })
}

/// Zircon `ItemInfo.ShouldLinkInfo`: stackables, consumables and scrolls
/// link by item type; everything else links the specific item.
pub fn link_for(
    catalog: &ItemCatalog,
    inventory: &[Option<ItemInstance>],
    inv_slot: u8,
    belt_slot: u8,
) -> BeltLink {
    let Some(item) = inventory.get(inv_slot as usize).and_then(|i| i.as_ref()) else {
        return BeltLink::empty(belt_slot);
    };
    let by_info = catalog
        .get(item.info)
        .map(|d| {
            d.stack_size > 1
                || d.item_type == item_type::CONSUMABLE
                || d.item_type == item_type::SCROLL
        })
        .unwrap_or(false);
    BeltLink {
        slot: belt_slot,
        info: if by_info { Some(item.info) } else { None },
        item: if by_info { None } else { Some(item.id) },
    }
}

/// The bag slot a belt link resolves to (first item of the type, or the
/// specific item), as Zircon's belt `UseItem` does.
pub fn belt_inventory_slot(inventory: &[Option<ItemInstance>], link: &BeltLink) -> Option<u8> {
    if let Some(info) = link.info {
        return inventory
            .iter()
            .position(|i| i.as_ref().map(|i| i.info == info).unwrap_or(false))
            .map(|p| p as u8);
    }
    let id = link.item?;
    inventory
        .iter()
        .position(|i| i.as_ref().map(|i| i.id == id).unwrap_or(false))
        .map(|p| p as u8)
}

/// What a belt cell shows: an item link shows that item, an info link shows
/// the type with the total count in the bag.
fn belt_cell_item(inventory: &[Option<ItemInstance>], link: &BeltLink) -> Option<ItemInstance> {
    if let Some(info) = link.info {
        let count: u32 = inventory
            .iter()
            .flatten()
            .filter(|i| i.info == info)
            .map(|i| i.count)
            .sum();
        return Some(ItemInstance {
            id: 0,
            info,
            count,
            durability: 0,
            max_durability: 0,
            added: Vec::new(),
            enhancements: Vec::new(),
            refinable: false,
            level: 0,
            accessory_experience: 0,
            non_refinable: false,
            bound: false,
            worthless: false,
            locked: false,
            expires_at: None,
            colour: 0,
        });
    }
    let id = link.item?;
    inventory.iter().flatten().find(|i| i.id == id).cloned()
}

pub struct WindowState {
    layout: WindowLayout,
    pub inventory_open: bool,
    inventory_page: usize,
    storage_page: usize,
    /// Server-configured Game Gold prices; zero means no more tabs.
    pub tab_prices: (u32, u32),
    pub character_open: bool,
    pub toggle_wardrobe: bool,
    pub discipline_next: mir_proto::DisciplineNext,
    pub skills_open: bool,
    pub quests_open: bool,
    pub quest_scroll: f32,
    /// Storage window (S): account storage, usable in safe zones.
    pub storage_open: bool,
    /// Set when the menu asks to toggle a window `Game` owns; `Game` reads
    /// the flag and clears it on the next frame.
    pub toggle_fortune: bool,
    pub toggle_map: bool,
    pub open_character_services: bool,
    pub toggle_chat_options: bool,
    trade_gold: Option<TextBox>,
    trade_buttons: Vec<Button>,
    request_buttons: Vec<Button>,
    marriage_buttons: Vec<Button>,
    /// Mail window (M): mailbox on the left, the selected mail or the
    /// compose form on the right.
    pub mail_open: bool,
    /// Zircon `MarketPlaceDialog`, opened with C.
    pub market: crate::market::MarketState,
    /// Refine page draft: type, quality and the chosen cells.
    craft_cells: [Option<(u8, u32)>; 7],
    craft_class: usize,
    craft_buttons: Vec<Button>,
    refine_type: u8,
    refine_quality: u8,
    refine_ores: Vec<(Grid, u8, u32)>,
    refine_items: Vec<(Grid, u8, u32)>,
    refine_specials: Vec<(Grid, u8, u32)>,
    refine_buttons: Vec<Button>,
    retrieve_buttons: Vec<Button>,
    accessory_target: Option<(Grid, u8, u32)>,
    accessory_ore: Option<(Grid, u8, u32)>,
    accessory_materials: Vec<(Grid, u8, u32)>,
    accessory_type: u8,
    accessory_buttons: Vec<Button>,
    /// Companion page: name box, one Adopt/Unlock per offer, three per
    /// owned companion; companion window (U) with Take buttons.
    pub companion_open: bool,
    companion_name: Option<TextBox>,
    companion_offer_buttons: Vec<Button>,
    companion_own_buttons: Vec<Button>,
    companion_skill_buttons: Vec<Button>,
    pub mail_selected: Option<u32>,
    mail_compose: bool,
    mail_boxes: Vec<TextBox>,
    mail_buttons: Vec<Button>,
    mail_take: Vec<Button>,
    /// Attachments for the mail being composed: (grid, slot, count).
    pub mail_attach: Vec<(Grid, u8, u32)>,
    /// Guild window (G): create, members, notice, invite, kick, leave.
    pub guild_open: bool,
    guild_name: Option<TextBox>,
    guild_notice: Option<TextBox>,
    guild_buttons: Vec<Button>,
    guild_kick: Vec<Button>,
    guild_invite_buttons: Vec<Button>,
    /// Group window (P): members, invite box, allow toggle.
    pub group_open: bool,
    group_name: Option<TextBox>,
    group_buttons: Vec<Button>,
    group_kick: Vec<Button>,
    invite_buttons: Vec<Button>,
    /// The belt is shown by default (Zircon `BeltDialog`, toggled with Z).
    pub belt_open: bool,
    /// Bag slot under the mouse this frame (for belt binding with digit keys).
    pub hover_inventory: Option<u8>,
    tooltip_item: Option<ItemInstance>,
    tooltip_slot: Option<(String, f32, f32)>,
    revive_button: Option<Button>,
    pub skill_scroll: f32,
    /// Magic whose icon is under the mouse in the skill window (for key binding).
    pub hover_magic: Option<u16>,
    pub npc: Option<NpcDialog>,
    /// Slot picked up with the mouse, waiting for a destination.
    pub carrying: Option<(Grid, u8)>,
    last_inventory_click: Option<(u8, u32, u64)>,
    pub tooltip: Option<(i32, f32, f32)>,
    buy_button: Option<Button>,
    sell_button: Option<Button>,
    /// HUD menu buttons, help, exit, currency, auto potion, drop filter.
    pub menu: MenuState,
    pub settings: crate::settings::Settings,
    /// Ranking board (R).
    pub ranking: crate::ranking::RankingState,
    /// The menu asked to toggle the friends window; `Game` drains it.
    pub toggle_friends: bool,
    close_all_hint: bool,
    auto_button_done: bool,
    magic_tip: Option<(u16, f32, f32)>,
}

impl WindowState {
    pub fn inventory_rect(&self, width: i32, height: i32) -> Rect {
        self.layout.rect(
            "inventory",
            Rect::new(width as f32 - 273.0, 40.0, 263.0, 444.0),
            (width, height),
        )
    }

    /// True while a window text box has keyboard focus.
    pub fn typing(&self) -> bool {
        (self.group_open && self.group_name.as_ref().is_some_and(|b| b.focused))
            || (self.guild_open
                && (self.guild_name.as_ref().is_some_and(|b| b.focused)
                    || self.guild_notice.as_ref().is_some_and(|b| b.focused)))
            || self.trade_gold.as_ref().is_some_and(|b| b.focused)
            || (self.mail_open && self.mail_boxes.iter().any(|b| b.focused))
            || (self.npc.is_some() && self.companion_name.as_ref().is_some_and(|b| b.focused))
            || self
                .npc
                .as_ref()
                .is_some_and(|d| d.sell_count.focused || d.buy_count.focused)
            || self.menu.typing()
            || self.market.typing()
            || self.ranking.typing()
    }
}

impl Default for WindowState {
    fn default() -> WindowState {
        WindowState {
            layout: WindowLayout::default(),
            settings: Default::default(),
            toggle_fortune: false,
            toggle_map: false,
            open_character_services: false,
            toggle_chat_options: false,
            inventory_open: false,
            inventory_page: 0,
            storage_page: 0,
            tab_prices: (100, 200),
            character_open: false,
            toggle_wardrobe: false,
            discipline_next: Default::default(),
            skills_open: false,
            quests_open: false,
            storage_open: false,
            trade_gold: None,
            trade_buttons: Vec::new(),
            request_buttons: Vec::new(),
            marriage_buttons: Vec::new(),
            mail_open: false,
            market: crate::market::MarketState::default(),
            craft_cells: [None; 7],
            craft_class: 0,
            craft_buttons: Vec::new(),
            refine_type: 2,
            refine_quality: 2,
            refine_ores: Vec::new(),
            refine_items: Vec::new(),
            refine_specials: Vec::new(),
            refine_buttons: Vec::new(),
            retrieve_buttons: Vec::new(),
            accessory_target: None,
            accessory_ore: None,
            accessory_materials: Vec::new(),
            accessory_type: mir_proto::refine_type::DC,
            accessory_buttons: Vec::new(),
            companion_open: false,
            companion_name: None,
            companion_offer_buttons: Vec::new(),
            companion_own_buttons: Vec::new(),
            companion_skill_buttons: Vec::new(),
            mail_selected: None,
            mail_compose: false,
            mail_boxes: Vec::new(),
            mail_buttons: Vec::new(),
            mail_take: Vec::new(),
            mail_attach: Vec::new(),
            guild_open: false,
            guild_name: None,
            guild_notice: None,
            guild_buttons: Vec::new(),
            guild_kick: Vec::new(),
            guild_invite_buttons: Vec::new(),
            group_open: false,
            group_name: None,
            group_buttons: Vec::new(),
            group_kick: Vec::new(),
            invite_buttons: Vec::new(),
            quest_scroll: 0.0,
            belt_open: true,
            hover_inventory: None,
            tooltip_item: None,
            tooltip_slot: None,
            revive_button: None,
            skill_scroll: 0.0,
            hover_magic: None,
            npc: None,
            carrying: None,
            last_inventory_click: None,
            tooltip: None,
            buy_button: None,
            sell_button: None,
            menu: MenuState::default(),
            ranking: crate::ranking::RankingState::default(),
            toggle_friends: false,
            close_all_hint: false,
            auto_button_done: false,
            magic_tip: None,
        }
    }
}

fn companion_elixir_inventory(
    inventory: &[Option<ItemInstance>],
    catalog: &ItemCatalog,
    level: i32,
) -> (Option<u8>, u32) {
    let mut slot = None;
    let mut owned = 0u32;
    for (i, item) in inventory.iter().enumerate() {
        let Some(item) = item else { continue };
        if item.count > 0
            && catalog.get(item.info).is_some_and(|def| {
                def.item_type == item_type::CONSUMABLE
                    && def.shape == 16
                    && def.required_amount == level
            })
        {
            slot.get_or_insert(i as u8);
            owned = owned.saturating_add(item.count);
        }
    }
    (slot, owned)
}

fn companion_reroll_action(level: i32, armed: bool, slot: u8, out: &mut Vec<ClientMessage>) {
    if !armed {
        out.push(ClientMessage::CompanionSkillToggle { level });
    }
    out.push(ClientMessage::ItemUse { slot });
}

pub struct Bag<'a> {
    pub gender: Gender,
    pub hair: u8,
    pub wardrobe_item: Option<i32>,
    pub wardrobe_helmet: Option<i32>,
    pub wardrobe_weapon: Option<i32>,
    pub wardrobe_shoes: Option<i32>,
    pub inventory: &'a [Option<ItemInstance>],
    pub equipment: &'a [Option<ItemInstance>],
    pub gold: u64,
    pub weights: &'a mir_proto::Weights,
    pub stats: &'a PlayerView,
    pub catalog: &'a ItemCatalog,
    pub magics: &'a [mir_proto::MagicSummary],
    pub toggles: &'a std::collections::HashSet<u16>,
    pub cooldowns: &'a std::collections::HashMap<u16, u64>,
    pub belt: &'a [BeltLink],
    pub use_item_time: u64,
    pub dead: bool,
    pub quests: &'a [mir_proto::UserQuestSummary],
    pub player_name: &'a str,
    pub group: &'a [(ObjectId, String)],
    pub allow_group: bool,
    pub group_invite: Option<&'a str>,
    pub user: Option<ObjectId>,
    pub storage: &'a [Option<ItemInstance>],
    pub trade: Option<&'a TradeState>,
    pub trade_request: Option<&'a str>,
    pub guild: Option<&'a mir_proto::GuildSummary>,
    pub guild_invite: Option<(&'a str, &'a str)>,
    pub mail: &'a [mir_proto::MailSummary],
    pub partner: Option<&'a str>,
    pub wedding_ring: Option<u32>,
    pub marriage_invite: Option<&'a str>,
    pub refines: &'a [mir_proto::RefineSummary],
    pub companions: &'a [mir_proto::CompanionSummary],
    pub companion_shop: &'a [mir_proto::CompanionOffer],
    pub currencies: &'a [mir_proto::CurrencySummary],
    /// Castles as (index, name, owner) and the one under conquest.
    pub castles: &'a [(i32, String, String)],
    pub conquest: Option<i32>,
    /// The last ranking page the server sent.
    pub ranking: &'a crate::ranking::RankingView,
}

/// An open trade as the client sees it.
pub struct TradeState {
    pub partner: String,
    /// My offered cells: (grid, slot, count).
    pub my_items: Vec<(Grid, u8, u32)>,
    pub my_gold: u64,
    pub their_items: Vec<ItemInstance>,
    pub their_gold: u64,
    /// I pressed Confirm and the server has not unlocked it since.
    pub confirmed: bool,
}

/// The few player facts windows need.
pub struct PlayerView {
    pub level: i32,
    pub class: u8,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub min_dc: i32,
    pub max_dc: i32,
    pub min_ac: i32,
    pub max_ac: i32,
    pub min_mc: i32,
    pub max_mc: i32,
    pub min_sc: i32,
    pub max_sc: i32,
    pub accuracy: i32,
    pub agility: i32,
    /// Held fame title ("" when none).
    pub fame_title: String,
}

/// Draw the item icon (StoreItems) centred in a cell, plus the count.
pub(crate) fn draw_item_cell(
    c: &mut Ctx,
    catalog: &ItemCatalog,
    r: Rect,
    item: Option<&ItemInstance>,
    hover: bool,
    selected: bool,
) {
    let bg = if selected {
        [125.0 / 255.0, 1.0, 125.0 / 255.0, 0.5]
    } else {
        [0.0, 0.0, 0.0, 0.55]
    };
    c.fill(r, bg);
    c.border(
        r,
        if hover || selected {
            [0, 255, 0, 255]
        } else {
            [99, 83, 50, 255]
        },
    );
    let Some(item) = item else { return };
    let Some(def) = catalog.get(item.info) else {
        return;
    };
    // Royal and Blue Dragon horse-armour ItemInfo images are absent from the
    // installed StoreItems library. Related original icons identify them.
    let image = match item.info {
        1166 => 1800, // Gold horse armour for Royal.
        1167 => 1804, // Blue horse armour for Blue Dragon.
        _ => def.image.max(0) as u32,
    };
    if let Some(info) = c.assets.info(lib::STORE_ITEMS, image) {
        let (w, h) = (info.width as f32, info.height as f32);
        let scale = (r.w / w).min(r.h / h).min(1.0);
        let (dw, dh) = (w * scale, h * scale);
        if let Some(sprite) = c.sprite(lib::STORE_ITEMS, image) {
            c.renderer.draw_scaled(
                sprite,
                r.x + (r.w - dw) / 2.0,
                r.y + (r.h - dh) / 2.0,
                dw,
                dh,
                [1.0, 1.0, 1.0, 1.0],
                Blend::Alpha,
            );
        }
    }
    if item.count > 1 {
        let s = item.count.to_string();
        let w = c.text.width(&s, 11);
        c.text.draw(
            &s,
            11,
            r.x + r.w - w - 2.0,
            r.y + r.h - 14.0,
            [255, 255, 255, 255],
        );
    }
}

fn dark_stone_overlay_frame(item_type: u8, shape: i32, now: u64) -> Option<u32> {
    (item_type == 16 && (1..=4).contains(&shape))
        .then_some(2020 + (shape as u32 - 1) * 10 + (now / 100 % 10) as u32)
}

/// The heading for a set: its name, the pieces worn, and the word "set"
/// only when the name does not already carry it ("Gale Set" does).
pub fn set_heading(name: &str, worn: usize, needed: usize) -> String {
    if name.to_ascii_lowercase().ends_with("set") {
        format!("{name} ({worn}/{needed})")
    } else {
        format!("{name} set ({worn}/{needed})")
    }
}

/// How a set stands for the player: its name, which of its pieces are worn,
/// and the bonus lines with whether each one currently applies.
pub struct SetView {
    pub name: String,
    pub worn: usize,
    pub needed: usize,
    /// (label, active) per bonus line.
    pub lines: Vec<(String, bool)>,
}

/// Work out a set's state from what is equipped. Mirrors the server rule:
/// every piece must be worn, and each line still has to clear the level and
/// name the class.
pub fn set_view(bag: &Bag, set_index: i32) -> Option<SetView> {
    let def = bag.catalog.set(set_index)?;
    let mut worn: Vec<i32> = Vec::new();
    for it in bag.equipment.iter().flatten() {
        // A broken item gives no set credit, as on the server.
        if it.max_durability > 0 && it.durability <= 0 {
            continue;
        }
        if bag.catalog.get(it.info).map(|d| d.set) == Some(set_index) && !worn.contains(&it.info) {
            worn.push(it.info);
        }
    }
    let complete = !def.items.is_empty() && worn.len() == def.items.len();
    let class_flag = 1u8 << bag.stats.class;
    let lines = def
        .stats
        .iter()
        .filter_map(|(stat, amount, class, level)| {
            let name = stat_name(*stat)?;
            let ok = complete && bag.stats.level >= *level && class & class_flag != 0;
            let mut label = format!("{name} +{amount}");
            if *level > 0 {
                label.push_str(&format!(" (level {level})"));
            }
            Some((label, ok))
        })
        .collect();
    Some(SetView {
        name: def.name.clone(),
        worn: worn.len(),
        needed: def.items.len(),
        lines,
    })
}

fn durability_label(item_type: u8, current: i32, max: i32) -> Option<String> {
    if max <= 0 {
        return None;
    }
    let display = |value: i32| (value as f64 / 1000.0).round_ties_even() as i32;
    match item_type {
        item_type::BOOK => Some(format!("Pages {current}/{max}")),
        item_type::MEAT => Some(format!("Quality {}/{}", display(current), display(max))),
        item_type::ORE => Some(format!("Purity {}", display(current))),
        item_type::WEAPON
        | item_type::SHIELD
        | item_type::ARMOUR
        | item_type::HELMET
        | item_type::NECKLACE
        | item_type::BRACELET
        | item_type::RING
        | item_type::SHOES => Some(format!("Durability {}/{}", display(current), display(max))),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn tooltip(
    c: &mut Ctx,
    def: &ItemDef,
    item: Option<&ItemInstance>,
    slot_name: Option<&str>,
    position: (f32, f32),
    viewport: (f32, f32),
    price: Option<u64>,
    set: Option<&SetView>,
) {
    let mut lines: Vec<(String, [u8; 4])> = vec![(def.name.clone(), [255, 255, 255, 255])];
    if let Some(name) = slot_name {
        lines.push((name.to_string(), [255, 230, 120, 255]));
    }
    lines.push((type_name(def.item_type).to_string(), [200, 200, 200, 255]));
    for (k, v) in &def.stats {
        if let Some(n) = stat_name(*k) {
            lines.push((format!("{n} +{v}"), [180, 220, 255, 255]));
        }
    }
    if def.required_amount > 0 && def.required_type == 0 {
        lines.push((
            format!("Requires level {}", def.required_amount),
            [255, 200, 120, 255],
        ));
    }
    if matches!(def.required_type, 11 | 12) && def.required_amount > 0 {
        let limit = if def.required_type == 11 {
            "at least"
        } else {
            "at most"
        };
        lines.push((
            format!("Companion level {limit} {}", def.required_amount),
            [255, 200, 120, 255],
        ));
    }
    if let Some(i) = item {
        for (k, v) in &i.added {
            if *k == 52 {
                let element = [
                    "",
                    "Fire",
                    "Ice",
                    "Lightning",
                    "Wind",
                    "Holy",
                    "Dark",
                    "Phantom",
                ];
                lines.push((
                    format!("Element: {}", element.get(*v as usize).unwrap_or(&"?")),
                    [255, 180, 80, 255],
                ));
            } else if let Some(n) = stat_name(*k) {
                let label = if (20..=33).contains(k) {
                    let element = n.split_once(' ').map_or(n, |(element, _)| element);
                    if k % 2 == 0 {
                        format!("E(Att) {element} {v:+}")
                    } else if *v >= 0 {
                        format!("E(Adv) {element} x{v}")
                    } else {
                        format!("E(Dis) {element} x{}", -v)
                    }
                } else {
                    format!("{n} {v:+} (bonus)")
                };
                lines.push((label, [255, 180, 80, 255]));
            }
        }
        for (stat, amount) in &i.enhancements {
            if let Some(name) = stat_name(*stat) {
                lines.push((
                    format!("  Includes {name} {amount:+} enhancement (kept on reroll)"),
                    [140, 230, 170, 255],
                ));
            }
        }
        if def.item_type == item_type::WEAPON {
            let status = if i.non_refinable {
                "Cannot be refined"
            } else if i.refinable {
                "Ready to refine"
            } else {
                "Gain combat experience to unlock refining"
            };
            lines.push((status.into(), [200, 200, 160, 255]));
        } else if matches!(
            def.item_type,
            item_type::NECKLACE | item_type::BRACELET | item_type::RING
        ) {
            let status = if i.non_refinable {
                "Cannot be refined"
            } else if i.refinable {
                "Ready for accessory upgrade"
            } else {
                "Combine matching items for accessory experience"
            };
            lines.push((
                format!(
                    "Accessory level {} - {} XP. {status}",
                    i.level, i.accessory_experience
                ),
                [200, 200, 160, 255],
            ));
        }
        if let Some(label) = durability_label(def.item_type, i.durability, i.max_durability) {
            lines.push((label, [200, 200, 200, 255]));
        }
        for (active, label) in [
            (i.bound, "Bound to account"),
            (i.worthless, "Worthless: cannot sell to NPCs"),
            (i.locked, "Locked: cannot drop or sell"),
        ] {
            if active {
                lines.push((label.into(), [255, 180, 120, 255]));
            }
        }
        // Timed store goods say how long they have left.
        if let Some(t) = i.expires_at {
            let left = t.saturating_sub(crate::fortune::wall_clock());
            lines.push((
                if left == 0 {
                    "Expired".to_string()
                } else if left >= 86_400 {
                    format!("Expires in {} days", left / 86_400)
                } else if left >= 3_600 {
                    format!("Expires in {} hours", left / 3_600)
                } else {
                    format!("Expires in {} minutes", left / 60)
                },
                [255, 180, 120, 255],
            ));
        }
    }
    lines.push((format!("Weight {}", def.weight), [200, 200, 200, 255]));
    if let Some(p) = price {
        lines.push((format!("Price {p} gold"), [255, 230, 120, 255]));
    } else if def.price > 0 {
        lines.push((format!("Value {} gold", def.price), [200, 200, 200, 255]));
    }
    if let Some(set) = set {
        let done = set.needed > 0 && set.worn == set.needed;
        lines.push((
            set_heading(&set.name, set.worn, set.needed),
            if done {
                [120, 255, 120, 255]
            } else {
                [200, 200, 200, 255]
            },
        ));
        for (label, active) in &set.lines {
            lines.push((
                format!("  {label}"),
                if *active {
                    [120, 255, 120, 255]
                } else {
                    [140, 140, 140, 255]
                },
            ));
        }
    }
    if !def.description.is_empty() {
        lines.push((def.description.clone(), [170, 170, 170, 255]));
    }
    let w = lines
        .iter()
        .map(|(l, _)| c.text.width(l, 12))
        .fold(0.0, f32::max)
        + 16.0;
    let h = lines.len() as f32 * 16.0 + 8.0;
    let x = (position.0 + 16.0).min((viewport.0 - w - 4.0).max(4.0));
    let y = (position.1 + 8.0).min((viewport.1 - h - 4.0).max(4.0));
    let r = Rect::new(x.max(4.0), y.max(4.0), w, h);
    c.fill(r, [0.0, 0.0, 0.0, 0.85]);
    c.border(r, GOLD);
    for (i, (l, col)) in lines.iter().enumerate() {
        c.text
            .draw(l, 12, r.x + 8.0, r.y + 4.0 + i as f32 * 16.0, *col);
    }
}

/// Zircon `CharacterDialog` equipment slot rectangles (relative to the window).
fn equipment_rects() -> [(usize, Rect, Option<u32>); 16] {
    use mir_proto::slot::*;
    [
        (WEAPON, Rect::new(58.0, 122.0, 65.0, 90.0), None),
        (ARMOUR, Rect::new(120.0, 123.0, 70.0, 150.0), None),
        (HELMET, Rect::new(140.0, 90.0, 35.0, 35.0), None),
        (TORCH, Rect::new(10.0, 196.0, 36.0, 36.0), Some(38)),
        (NECKLACE, Rect::new(10.0, 157.0, 36.0, 36.0), Some(33)),
        (BRACELET_L, Rect::new(244.0, 157.0, 36.0, 36.0), Some(32)),
        (BRACELET_R, Rect::new(283.0, 157.0, 36.0, 36.0), Some(32)),
        (RING_L, Rect::new(244.0, 196.0, 36.0, 36.0), Some(31)),
        (RING_R, Rect::new(283.0, 196.0, 36.0, 36.0), Some(31)),
        (SHOES, Rect::new(10.0, 235.0, 36.0, 36.0), Some(36)),
        (POISON, Rect::new(244.0, 274.0, 36.0, 36.0), Some(40)),
        (AMULET, Rect::new(283.0, 235.0, 36.0, 36.0), Some(39)),
        (FLOWER, Rect::new(244.0, 235.0, 36.0, 36.0), Some(81)),
        (13, Rect::new(283.0, 118.0, 36.0, 36.0), Some(82)),
        (14, Rect::new(244.0, 118.0, 36.0, 36.0), Some(104)),
        (SHIELD, Rect::new(170.0, 170.0, 36.0, 36.0), None),
    ]
}

fn equipment_slot_name(slot: usize) -> &'static str {
    [
        "Weapon",
        "Armour",
        "Helmet",
        "Torch",
        "Necklace",
        "Left bracelet",
        "Right bracelet",
        "Left ring",
        "Right ring",
        "Shoes",
        "Poison",
        "Amulet",
        "Flower",
        "Horse armour",
        "Emblem",
        "Shield",
    ][slot]
}

fn skill_key_label(key: u8) -> String {
    if cfg!(target_os = "macos") {
        let key = b"1234567890-="
            .get(key.saturating_sub(1) as usize)
            .copied()
            .unwrap_or(b'?') as char;
        format!("⌥{key}")
    } else {
        format!("F{key}")
    }
}

impl WindowState {
    /// Draw all open windows. Returns true when the mouse is over a window
    /// (so the world should ignore clicks) and pushes messages to send.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        c: &mut Ctx,
        bag: &Bag,
        width: i32,
        height: i32,
        out: &mut Vec<ClientMessage>,
    ) -> bool {
        let mouse = c.input.mouse;
        let mut over = self.layout.is_dragging();
        self.tooltip = None;
        self.tooltip_slot = None;
        self.hover_inventory = None;
        self.tooltip_item = None;
        // ---- Quest log (L) ----
        if self.quests_open {
            let win = self.layout.place(
                "quests",
                Rect::new(width as f32 - 380.0 - 10.0, 30.0, 380.0, 460.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
                self.quest_scroll = (self.quest_scroll - c.input.wheel * 18.0).max(0.0);
            }
            let closed = c.window(win, "Quests", false) && !self.layout.is_dragging();
            // (line, colour, quest this line belongs to when it is a title)
            let mut rows: Vec<(String, [u8; 4], Option<i32>)> = Vec::new();
            if bag.quests.is_empty() {
                rows.push((
                    "No quests. Talk to NPCs to find some.".into(),
                    [200, 200, 200, 255],
                    None,
                ));
            }
            let mut sorted: Vec<&mir_proto::UserQuestSummary> = bag.quests.iter().collect();
            sorted.sort_by_key(|q| (q.completed, q.quest));
            for q in sorted {
                let Some(def) = bag.catalog.quest(q.quest) else {
                    continue;
                };
                let title_col = if q.completed {
                    [120, 120, 120, 255]
                } else {
                    [255, 255, 0, 255]
                };
                rows.push((
                    format!(
                        "{} {}{}",
                        if q.track { "[x]" } else { "[ ]" },
                        def.name,
                        if q.completed { " (done)" } else { "" }
                    ),
                    title_col,
                    Some(q.quest),
                ));
                let text = if q.completed {
                    bag.catalog
                        .quest_text(def, &def.completed_text, bag.player_name)
                } else {
                    bag.catalog
                        .quest_text(def, &def.progress_text, bag.player_name)
                };
                for line in wrap_text(c, &text, 340.0, 12) {
                    rows.push((line, [220, 220, 220, 255], None));
                }
                if !q.completed {
                    for (line, col) in crate::overlay::quest_task_lines(def, q, bag.catalog) {
                        rows.push((line, col, None));
                    }
                    let rewards: Vec<String> = def
                        .rewards
                        .iter()
                        .filter(|r| r.class & (1 << bag.stats.class) != 0)
                        .map(|r| {
                            format!(
                                "{}{} x{}",
                                if r.choice { "(choice) " } else { "" },
                                bag.catalog.name(r.item),
                                r.amount
                            )
                        })
                        .collect();
                    if !rewards.is_empty() {
                        rows.push((
                            format!("  Reward: {}", rewards.join(", ")),
                            [160, 200, 255, 255],
                            None,
                        ));
                    }
                }
                rows.push((String::new(), [0, 0, 0, 0], None));
            }
            let max_scroll = (rows.len() as f32 * 18.0 - 400.0).max(0.0);
            self.quest_scroll = self.quest_scroll.min(max_scroll);
            let top = win.y + 40.0;
            c.text.draw(
                "Click a quest to track it.",
                10,
                win.x + 20.0,
                win.y + win.h - 24.0,
                [160, 160, 160, 255],
            );
            for (i, (line, col, quest)) in rows.iter().enumerate() {
                let y = top + i as f32 * 18.0 - self.quest_scroll;
                if y < top - 1.0 || y + 18.0 > win.y + win.h - 10.0 {
                    continue;
                }
                if line.is_empty() {
                    continue;
                }
                // Titles toggle tracking (Zircon `ClientUserQuest.Track`).
                if let Some(id) = quest {
                    let row = Rect::new(win.x + 16.0, y - 1.0, win.w - 32.0, 18.0);
                    if row.contains(mouse.0, mouse.1) {
                        c.fill(row, [0.25, 0.25, 0.35, 0.6]);
                        if c.input.lmb_pressed {
                            let track = bag
                                .quests
                                .iter()
                                .find(|q| q.quest == *id)
                                .map(|q| !q.track)
                                .unwrap_or(true);
                            out.push(ClientMessage::QuestTrack { quest: *id, track });
                        }
                    }
                }
                c.text.draw(line, 12, win.x + 20.0, y, *col);
            }
            if closed {
                self.quests_open = false;
            }
        }

        // ---- Storage (B): 10 columns, account storage ----
        if self.storage_open {
            let cols = 10;
            let rows = 10;
            let unlocked = bag.storage.len() > 100;
            if !unlocked {
                self.storage_page = 0;
            }
            let win = self.layout.place(
                "storage",
                Rect::new(
                    10.0,
                    40.0,
                    20.0 + cols as f32 * PITCH + 20.0,
                    53.0 + rows as f32 * PITCH + 50.0,
                ),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            let closed = c.window(win, "Storage", true) && !self.layout.is_dragging();
            for (page, label) in [(0, "Tab 1"), (1, "Tab 2")] {
                let tab = Rect::new(win.x + 20.0 + page as f32 * 92.0, win.y + 33.0, 88.0, 18.0);
                let title = if page == 1 && !unlocked {
                    format!("Buy {} GG", self.tab_prices.1)
                } else {
                    label.into()
                };
                c.text.draw(
                    &title,
                    11,
                    tab.x,
                    tab.y,
                    if self.storage_page == page {
                        [255, 221, 103, 255]
                    } else {
                        [200, 200, 200, 255]
                    },
                );
                if c.input.lmb_pressed && tab.contains(mouse.0, mouse.1) {
                    if page == 0 || unlocked {
                        self.storage_page = page;
                    } else if self.tab_prices.1 > 0 {
                        out.push(ClientMessage::BuyTab { storage: true });
                    }
                }
            }
            for i in 0..100.min(bag.storage.len().saturating_sub(self.storage_page * 100)) {
                let (col, row) = (i % cols, i / cols);
                let slot = self.storage_page * 100 + i;
                let r = Rect::new(
                    win.x + 20.0 + col as f32 * PITCH,
                    win.y + 53.0 + row as f32 * PITCH,
                    CELL,
                    CELL,
                );
                let hover = r.contains(mouse.0, mouse.1);
                let item = bag.storage[slot].as_ref();
                let selected = self.carrying == Some((Grid::Storage, slot as u8));
                draw_item_cell(c, bag.catalog, r, item, hover, selected);
                if hover {
                    if let Some(it) = item {
                        self.tooltip = Some((it.info, mouse.0, mouse.1));
                        self.tooltip_item = Some(it.clone());
                    }
                    if c.input.lmb_pressed {
                        self.click_slot(Grid::Storage, slot as u8, item.is_some(), out);
                    }
                    if c.input.rmb_pressed && item.is_some() && self.inventory_open {
                        if let Some(message) = self.retrieve_storage_item(slot as u8, bag.inventory)
                        {
                            out.push(message);
                        }
                    }
                }
            }
            c.text.draw(
                "Safe zones only. Right-click item to transfer; click slot to move.",
                10,
                win.x + 20.0,
                win.y + win.h - 3.0 - 42.0 + 12.0,
                [160, 160, 160, 255],
            );
            if closed {
                self.storage_open = false;
                self.carrying = None;
            }
        }

        // ---- Trade window ----
        if let Some(t) = bag.trade {
            let win = self.layout.place(
                "trade",
                Rect::new(width as f32 / 2.0 - 200.0, 60.0, 400.0, 300.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            c.window(win, &format!("Trade with {}", t.partner), true);
            let col_w = 180.0;
            c.text.draw(
                "You offer (right-click bag items):",
                11,
                win.x + 16.0,
                win.y + 36.0,
                [255, 255, 0, 255],
            );
            for (i, (grid, slot, count)) in t.my_items.iter().enumerate() {
                let cells = match grid {
                    Grid::Inventory => bag.inventory,
                    Grid::Equipment => bag.equipment,
                    Grid::Storage => bag.storage,
                };
                let name = cells
                    .get(*slot as usize)
                    .and_then(|c| c.as_ref())
                    .map(|it| bag.catalog.name(it.info))
                    .unwrap_or_else(|| "?".into());
                c.text.draw(
                    &format!("{name} x{count}"),
                    11,
                    win.x + 16.0,
                    win.y + 54.0 + i as f32 * 15.0,
                    [255, 255, 255, 255],
                );
            }
            c.text.draw(
                &format!("{} offers:", t.partner),
                11,
                win.x + 16.0 + col_w + 20.0,
                win.y + 36.0,
                [255, 255, 0, 255],
            );
            for (i, it) in t.their_items.iter().enumerate() {
                c.text.draw(
                    &format!("{} x{}", bag.catalog.name(it.info), it.count),
                    11,
                    win.x + 16.0 + col_w + 20.0,
                    win.y + 54.0 + i as f32 * 15.0,
                    [255, 255, 255, 255],
                );
            }
            let gy = win.y + win.h - 3.0 - 42.0 - 56.0;
            c.text.draw(
                &format!("Gold: {}", t.my_gold),
                12,
                win.x + 16.0,
                gy,
                [218, 165, 32, 255],
            );
            c.text.draw(
                &format!("Gold: {}", t.their_gold),
                12,
                win.x + 16.0 + col_w + 20.0,
                gy,
                [218, 165, 32, 255],
            );
            let gold_box = self
                .trade_gold
                .get_or_insert_with(|| TextBox::new(Rect::new(0.0, 0.0, 100.0, 22.0), 12));
            gold_box.rect = Rect::new(win.x + 16.0, gy + 22.0, 100.0, 22.0);
            let submitted = gold_box.update(c);
            gold_box.text.retain(|ch| ch.is_ascii_digit());
            if self.trade_buttons.is_empty() {
                self.trade_buttons = vec![
                    Button::default_style(0.0, 0.0, 70.0, "Set gold"),
                    Button::default_style(0.0, 0.0, 80.0, "Confirm"),
                    Button::default_style(0.0, 0.0, 80.0, "Cancel"),
                ];
            }
            let gold: u64 = gold_box.text.parse().unwrap_or(0);
            let b = &mut self.trade_buttons[0];
            b.pos = (win.x + 122.0, gy + 20.0);
            b.enabled = gold > t.my_gold;
            if (b.update(c) || submitted) && b.enabled {
                out.push(ClientMessage::TradeAddGold { gold });
            }
            let fy = win.y + win.h - 3.0 - 42.0 + 8.0;
            let b = &mut self.trade_buttons[1];
            b.pos = (win.x + 110.0, fy);
            b.enabled = !t.confirmed;
            b.label = Some(if t.confirmed {
                "Waiting...".into()
            } else {
                "Confirm".into()
            });
            if b.update(c) {
                out.push(ClientMessage::TradeConfirm);
            }
            let b = &mut self.trade_buttons[2];
            b.pos = (win.x + 210.0, fy);
            if b.update(c) {
                out.push(ClientMessage::TradeClose);
            }
        } else if let Some(b) = &mut self.trade_gold {
            b.text.clear();
            b.focused = false;
        }

        // ---- Trade request prompt ----
        if let Some(from) = bag.trade_request {
            let win = self.layout.place(
                "trade_request",
                Rect::new(width as f32 / 2.0 - 150.0, 240.0, 300.0, 100.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            c.window(win, "Trade request", false);
            c.text.draw(
                &format!("{from} wants to trade with you."),
                12,
                win.x + 16.0,
                win.y + 40.0,
                [255, 255, 255, 255],
            );
            if self.request_buttons.is_empty() {
                self.request_buttons = vec![
                    Button::default_style(0.0, 0.0, 80.0, "Accept"),
                    Button::default_style(0.0, 0.0, 80.0, "Decline"),
                ];
            }
            let b = &mut self.request_buttons[0];
            b.pos = (win.x + 60.0, win.y + 66.0);
            if b.update(c) {
                out.push(ClientMessage::TradeResponse { accept: true });
            }
            let b = &mut self.request_buttons[1];
            b.pos = (win.x + 160.0, win.y + 66.0);
            if b.update(c) {
                out.push(ClientMessage::TradeResponse { accept: false });
            }
        }

        // ---- Marriage proposal prompt ----
        if let Some(from) = bag.marriage_invite {
            let win = self.layout.place(
                "proposal",
                Rect::new(width as f32 / 2.0 - 160.0, 480.0, 320.0, 100.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            c.window(win, "Proposal", false);
            c.text.draw(
                &format!("{from} asks you to marry them (500,000 gold each)."),
                11,
                win.x + 16.0,
                win.y + 40.0,
                [255, 255, 255, 255],
            );
            if self.marriage_buttons.is_empty() {
                self.marriage_buttons = vec![
                    Button::default_style(0.0, 0.0, 80.0, "Accept"),
                    Button::default_style(0.0, 0.0, 80.0, "Decline"),
                    Button::default_style(0.0, 0.0, 120.0, "To partner"),
                ];
            }
            let b = &mut self.marriage_buttons[0];
            b.pos = (win.x + 70.0, win.y + 66.0);
            if b.update(c) {
                out.push(ClientMessage::MarriageResponse { accept: true });
            }
            let b = &mut self.marriage_buttons[1];
            b.pos = (win.x + 170.0, win.y + 66.0);
            if b.update(c) {
                out.push(ClientMessage::MarriageResponse { accept: false });
            }
        }

        // ---- Mail (M) ----
        if self.mail_open {
            let win = self.layout.place(
                "mail",
                Rect::new(width as f32 / 2.0 - 260.0, 40.0, 520.0, 360.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            let closed = c.window(win, "Mail", true) && !self.layout.is_dragging();
            if self.mail_buttons.is_empty() {
                self.mail_buttons = vec![
                    Button::default_style(0.0, 0.0, 80.0, "Compose"),
                    Button::default_style(0.0, 0.0, 70.0, "Delete"),
                    Button::default_style(0.0, 0.0, 70.0, "Send"),
                    Button::default_style(0.0, 0.0, 90.0, "Take gold"),
                ];
            }
            if self.mail_boxes.is_empty() {
                self.mail_boxes = vec![
                    TextBox::new(Rect::new(0.0, 0.0, 200.0, 22.0), 20),
                    TextBox::new(Rect::new(0.0, 0.0, 200.0, 22.0), 30),
                    TextBox::new(Rect::new(0.0, 0.0, 300.0, 22.0), 300),
                    TextBox::new(Rect::new(0.0, 0.0, 100.0, 22.0), 12),
                ];
            }
            // Left: the mailbox.
            let list_w = 190.0;
            c.text.draw(
                &format!("Mailbox ({})", bag.mail.len()),
                12,
                win.x + 14.0,
                win.y + 36.0,
                [255, 255, 200, 255],
            );
            let mut y = win.y + 56.0;
            for m in bag.mail.iter().rev().take(14) {
                let r = Rect::new(win.x + 12.0, y - 2.0, list_w, 18.0);
                let hover = r.contains(mouse.0, mouse.1);
                let selected = self.mail_selected == Some(m.index);
                if selected {
                    c.fill(r, [0.3, 0.3, 0.5, 0.8]);
                } else if hover {
                    c.fill(r, [0.25, 0.25, 0.35, 0.6]);
                }
                let col = if m.opened {
                    [200, 200, 200, 255]
                } else {
                    [255, 255, 0, 255]
                };
                let mut label = format!("{}: {}", m.sender, m.subject);
                if !m.items.is_empty() || m.gold > 0 {
                    label.push_str(" *");
                }
                c.text.draw(&label, 11, win.x + 16.0, y, col);
                if hover && c.input.lmb_pressed {
                    self.mail_selected = Some(m.index);
                    self.mail_compose = false;
                    if !m.opened {
                        out.push(ClientMessage::MailOpened { index: m.index });
                    }
                }
                y += 18.0;
            }
            let fy = win.y + win.h - 3.0 - 42.0 + 8.0;
            let b = &mut self.mail_buttons[0];
            b.pos = (win.x + 12.0, fy);
            b.label = Some(if self.mail_compose {
                "Back".into()
            } else {
                "Compose".into()
            });
            if b.update(c) {
                self.mail_compose = !self.mail_compose;
                if self.mail_compose {
                    self.mail_selected = None;
                }
            }
            // Right: the selected mail, or the compose form.
            let rx = win.x + list_w + 30.0;
            let rw = win.w - list_w - 44.0;
            if self.mail_compose {
                let labels = ["To:", "Subject:", "Message:", "Gold:"];
                let mut by = win.y + 40.0;
                for (i, label) in labels.iter().enumerate() {
                    c.text.draw(label, 11, rx, by + 3.0, [220, 220, 220, 255]);
                    let bw = if i == 2 { rw - 70.0 } else { 160.0 };
                    let bx = self.mail_boxes.get_mut(i).unwrap();
                    bx.rect = Rect::new(rx + 64.0, by, bw, 22.0);
                    bx.update(c);
                    if i == 3 {
                        bx.text.retain(|ch| ch.is_ascii_digit());
                    }
                    by += 28.0;
                }
                c.text.draw(
                    "Attachments (right-click bag items, 5 max):",
                    10,
                    rx,
                    by + 4.0,
                    [160, 160, 160, 255],
                );
                by += 18.0;
                let mut remove = None;
                for (i, (grid, slot, count)) in self.mail_attach.iter().enumerate() {
                    let cells = match grid {
                        Grid::Inventory => bag.inventory,
                        Grid::Equipment => bag.equipment,
                        Grid::Storage => bag.storage,
                    };
                    let name = cells
                        .get(*slot as usize)
                        .and_then(|c| c.as_ref())
                        .map(|it| bag.catalog.name(it.info))
                        .unwrap_or_else(|| "?".into());
                    let r = Rect::new(rx, by, rw, 16.0);
                    let hover = r.contains(mouse.0, mouse.1);
                    c.text.draw(
                        &format!("{name} x{count}  (click to remove)"),
                        11,
                        rx,
                        by,
                        if hover {
                            [255, 150, 150, 255]
                        } else {
                            [255, 255, 255, 255]
                        },
                    );
                    if hover && c.input.lmb_pressed {
                        remove = Some(i);
                    }
                    by += 16.0;
                }
                if let Some(i) = remove {
                    self.mail_attach.remove(i);
                }
                let recipient = self.mail_boxes[0].text.trim().to_string();
                let b = &mut self.mail_buttons[2];
                b.pos = (rx, fy);
                b.enabled = !recipient.is_empty();
                if b.update(c) && b.enabled {
                    out.push(ClientMessage::MailSend {
                        recipient,
                        subject: self.mail_boxes[1].text.clone(),
                        message: self.mail_boxes[2].text.clone(),
                        gold: self.mail_boxes[3].text.parse().unwrap_or(0),
                        items: self.mail_attach.clone(),
                    });
                    for bx in &mut self.mail_boxes {
                        bx.text.clear();
                    }
                    self.mail_attach.clear();
                    self.mail_compose = false;
                }
            } else if let Some(m) = self
                .mail_selected
                .and_then(|i| bag.mail.iter().find(|m| m.index == i))
            {
                c.text.draw(
                    &format!("From {}: {}", m.sender, m.subject),
                    12,
                    rx,
                    win.y + 40.0,
                    [255, 255, 0, 255],
                );
                let mut by = win.y + 60.0;
                for line in wrap_text(c, &m.message, rw, 11).into_iter().take(8) {
                    c.text.draw(&line, 11, rx, by, [220, 220, 220, 255]);
                    by += 14.0;
                }
                by += 8.0;
                if self.mail_take.len() != m.items.len() {
                    self.mail_take = m
                        .items
                        .iter()
                        .map(|_| Button::default_style(0.0, 0.0, 50.0, "Take"))
                        .collect();
                }
                for (i, it) in m.items.iter().enumerate() {
                    c.text.draw(
                        &format!("{} x{}", bag.catalog.name(it.info), it.count),
                        11,
                        rx,
                        by + 2.0,
                        [255, 255, 255, 255],
                    );
                    let b = &mut self.mail_take[i];
                    b.pos = (rx + rw - 56.0, by - 2.0);
                    if b.update(c) {
                        out.push(ClientMessage::MailGetItem {
                            index: m.index,
                            slot: i as u8,
                        });
                    }
                    by += 22.0;
                }
                if m.gold > 0 {
                    c.text.draw(
                        &format!("{} gold", m.gold),
                        11,
                        rx,
                        by + 2.0,
                        [218, 165, 32, 255],
                    );
                    let b = &mut self.mail_buttons[3];
                    b.pos = (rx + rw - 96.0, by - 2.0);
                    if b.update(c) {
                        out.push(ClientMessage::MailGetItem {
                            index: m.index,
                            slot: 255,
                        });
                    }
                }
                let b = &mut self.mail_buttons[1];
                b.pos = (rx, fy);
                b.enabled = m.items.is_empty() && m.gold == 0;
                if b.update(c) && b.enabled {
                    out.push(ClientMessage::MailDelete { index: m.index });
                }
            } else {
                c.text.draw(
                    "Select a mail, or Compose a new one.",
                    11,
                    rx,
                    win.y + 40.0,
                    [160, 160, 160, 255],
                );
            }
            if closed {
                self.mail_open = false;
            }
        }

        // ---- Guild (G) ----
        if self.guild_open {
            let win = self.layout.place(
                "guild",
                Rect::new(width as f32 - 340.0 - 10.0, 30.0, 340.0, 400.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            let title = bag
                .guild
                .map(|g| format!("Guild: {}", g.name))
                .unwrap_or_else(|| "Guild".into());
            let closed = c.window(win, &title, true) && !self.layout.is_dragging();
            let name_box = self
                .guild_name
                .get_or_insert_with(|| TextBox::new(Rect::new(0.0, 0.0, 150.0, 22.0), 15));
            if self.guild_buttons.is_empty() {
                self.guild_buttons = vec![
                    Button::default_style(0.0, 0.0, 70.0, "Create"),
                    Button::default_style(0.0, 0.0, 60.0, "Invite"),
                    Button::default_style(0.0, 0.0, 60.0, "Leave"),
                    Button::default_style(0.0, 0.0, 80.0, "Set notice"),
                ];
            }
            let fy = win.y + win.h - 3.0 - 42.0 + 8.0;
            match bag.guild {
                None => {
                    c.text.draw(
                        "You are not in a guild. Founding one costs",
                        12,
                        win.x + 16.0,
                        win.y + 40.0,
                        [220, 220, 220, 255],
                    );
                    c.text.draw(
                        "7,500,000 gold plus 1,000,000 per member slot (10 here).",
                        12,
                        win.x + 16.0,
                        win.y + 58.0,
                        [220, 220, 220, 255],
                    );
                    name_box.rect = Rect::new(win.x + 16.0, fy + 2.0, 150.0, 22.0);
                    let submitted = name_box.update(c);
                    let name = name_box.text.trim().to_string();
                    let b = &mut self.guild_buttons[0];
                    b.pos = (win.x + 176.0, fy);
                    b.enabled = !name.is_empty();
                    if (b.update(c) || submitted) && b.enabled {
                        out.push(ClientMessage::GuildCreate { name, members: 10 });
                    }
                }
                Some(g) => {
                    let me = g.members.iter().find(|m| m.index == g.user_index);
                    let leader = me.is_some_and(|m| m.permission == -1);
                    let can_invite =
                        me.is_some_and(|m| m.permission == -1 || m.permission & 2 != 0);
                    let can_notice =
                        me.is_some_and(|m| m.permission == -1 || m.permission & 1 != 0);
                    c.text.draw(
                        &format!(
                            "Members {}/{}   Funds {}   Tax {}%",
                            g.members.len(),
                            g.member_limit,
                            g.funds,
                            g.tax
                        ),
                        11,
                        win.x + 16.0,
                        win.y + 38.0,
                        [255, 255, 200, 255],
                    );
                    let mut y = win.y + 56.0;
                    for line in wrap_text(c, &g.notice, 300.0, 11).into_iter().take(2) {
                        c.text
                            .draw(&line, 11, win.x + 16.0, y, [200, 200, 255, 255]);
                        y += 14.0;
                    }
                    // Castle and wars (Zircon GuildDialog war tab).
                    let mut status = String::new();
                    if !g.castle.is_empty() {
                        status.push_str(&format!("Holds {}. ", g.castle));
                    }
                    for (enemy, secs) in &g.wars {
                        status.push_str(&format!("War with {enemy} ({}m). ", secs / 60));
                    }
                    for (index, name, owner) in bag.castles {
                        let state = if bag.conquest == Some(*index) {
                            "under siege"
                        } else if owner.is_empty() {
                            "unclaimed"
                        } else {
                            owner.as_str()
                        };
                        status.push_str(&format!("{name}: {state}. "));
                    }
                    if !status.is_empty() {
                        c.text
                            .draw(&status, 11, win.x + 16.0, y, [255, 200, 120, 255]);
                    }
                    y = win.y + 104.0;
                    if self.guild_kick.len() != g.members.len() {
                        self.guild_kick = g
                            .members
                            .iter()
                            .map(|_| Button::default_style(0.0, 0.0, 50.0, "Kick"))
                            .collect();
                    }
                    for (i, m) in g.members.iter().enumerate().take(9) {
                        let col = if m.index == g.user_index {
                            [255, 255, 0, 255]
                        } else if m.online {
                            [255, 255, 255, 255]
                        } else {
                            [140, 140, 140, 255]
                        };
                        c.text.draw(
                            &format!("{} - {}", m.name, m.rank),
                            11,
                            win.x + 16.0,
                            y,
                            col,
                        );
                        if leader && m.index != g.user_index {
                            let b = &mut self.guild_kick[i];
                            b.pos = (win.x + win.w - 66.0, y - 2.0);
                            if b.update(c) {
                                out.push(ClientMessage::GuildKickMember { index: m.index });
                            }
                        }
                        y += 18.0;
                    }
                    // War on the named guild, and a conquest request for
                    // the first castle (StartWar / leader permissions).
                    let can_war = me.is_some_and(|m| m.permission == -1 || m.permission & 128 != 0);
                    let wy = win.y + win.h - 3.0 - 42.0 - 60.0;
                    if self.guild_buttons.len() < 8 {
                        self.guild_buttons.push(Button::default_style(
                            0.0,
                            0.0,
                            90.0,
                            "Declare war",
                        ));
                        self.guild_buttons.push(Button::default_style(
                            0.0,
                            0.0,
                            120.0,
                            "Request conquest",
                        ));
                        self.guild_buttons
                            .push(Button::default_style(0.0, 0.0, 60.0, "Gates"));
                        self.guild_buttons
                            .push(Button::default_style(0.0, 0.0, 70.0, "Repair"));
                    }
                    if !g.castle.is_empty() {
                        let b = &mut self.guild_buttons[6];
                        b.pos = (win.x + 16.0, wy - 26.0);
                        if b.update(c) {
                            out.push(ClientMessage::GuildToggleCastleGates);
                        }
                        let b = &mut self.guild_buttons[7];
                        b.pos = (win.x + 82.0, wy - 26.0);
                        b.enabled = leader;
                        if b.update(c) && b.enabled {
                            out.push(ClientMessage::GuildRepairCastleGates);
                        }
                    }
                    let name_now = name_box.text.trim().to_string();
                    let b = &mut self.guild_buttons[4];
                    b.pos = (win.x + 16.0, wy);
                    b.enabled = can_war && !name_now.is_empty();
                    if b.update(c) && b.enabled {
                        out.push(ClientMessage::GuildWar {
                            name: name_now.clone(),
                        });
                    }
                    let b = &mut self.guild_buttons[5];
                    b.pos = (win.x + 112.0, wy);
                    b.enabled = leader
                        && g.castle.is_empty()
                        && !bag.castles.is_empty()
                        && bag.conquest.is_none();
                    if b.update(c) && b.enabled {
                        if let Some((index, _, _)) = bag.castles.first() {
                            out.push(ClientMessage::GuildRequestConquest { index: *index });
                        }
                    }
                    // Footer: invite box, Leave, and the notice box above.
                    let notice_box = self
                        .guild_notice
                        .get_or_insert_with(|| TextBox::new(Rect::new(0.0, 0.0, 220.0, 22.0), 200));
                    let ny = win.y + win.h - 3.0 - 42.0 - 30.0;
                    notice_box.rect = Rect::new(win.x + 16.0, ny, 220.0, 22.0);
                    let notice_submitted = notice_box.update(c);
                    let b = &mut self.guild_buttons[3];
                    b.pos = (win.x + 244.0, ny - 2.0);
                    b.enabled = can_notice;
                    if (b.update(c) || notice_submitted) && b.enabled {
                        out.push(ClientMessage::GuildEditNotice {
                            notice: notice_box.text.clone(),
                        });
                    }
                    name_box.rect = Rect::new(win.x + 16.0, fy + 2.0, 150.0, 22.0);
                    let submitted = name_box.update(c);
                    let name = name_box.text.trim().to_string();
                    let b = &mut self.guild_buttons[1];
                    b.pos = (win.x + 176.0, fy);
                    b.enabled = can_invite && !name.is_empty();
                    if (b.update(c) || submitted) && b.enabled {
                        out.push(ClientMessage::GuildInviteMember { name });
                        if let Some(nb) = &mut self.guild_name {
                            nb.text.clear();
                        }
                    }
                    let b = &mut self.guild_buttons[2];
                    b.pos = (win.x + 244.0, fy);
                    b.enabled = true;
                    if b.update(c) {
                        out.push(ClientMessage::GuildLeave);
                    }
                }
            }
            if closed {
                self.guild_open = false;
            }
        }

        // ---- Guild invite prompt ----
        if let Some((from, guild)) = bag.guild_invite {
            let win = self.layout.place(
                "guild_invite",
                Rect::new(width as f32 / 2.0 - 160.0, 360.0, 320.0, 100.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            c.window(win, "Guild invite", false);
            c.text.draw(
                &format!("{from} invites you to join {guild}."),
                12,
                win.x + 16.0,
                win.y + 40.0,
                [255, 255, 255, 255],
            );
            if self.guild_invite_buttons.is_empty() {
                self.guild_invite_buttons = vec![
                    Button::default_style(0.0, 0.0, 80.0, "Accept"),
                    Button::default_style(0.0, 0.0, 80.0, "Decline"),
                ];
            }
            let b = &mut self.guild_invite_buttons[0];
            b.pos = (win.x + 70.0, win.y + 66.0);
            if b.update(c) {
                out.push(ClientMessage::GuildResponse { accept: true });
            }
            let b = &mut self.guild_invite_buttons[1];
            b.pos = (win.x + 170.0, win.y + 66.0);
            if b.update(c) {
                out.push(ClientMessage::GuildResponse { accept: false });
            }
        }

        // ---- Group (P) ----
        if self.group_open {
            // Left of the guild window so both can be open at once.
            let win = self.layout.place(
                "group",
                Rect::new(width as f32 - 300.0 - 10.0 - 360.0, 30.0, 300.0, 330.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            let closed = c.window(win, "Group", true) && !self.layout.is_dragging();
            let leader = bag.group.first().map(|(id, _)| *id);
            let i_lead = leader.is_some() && leader == bag.user;
            if bag.group.is_empty() {
                c.text.draw(
                    "Not in a group.",
                    12,
                    win.x + 16.0,
                    win.y + 40.0,
                    [200, 200, 200, 255],
                );
            }
            if self.group_kick.len() != bag.group.len() {
                self.group_kick = bag
                    .group
                    .iter()
                    .map(|_| Button::default_style(0.0, 0.0, 60.0, "Kick"))
                    .collect();
            }
            for (i, (id, name)) in bag.group.iter().enumerate() {
                let y = win.y + 40.0 + i as f32 * 22.0;
                let label = if Some(*id) == leader {
                    format!("{name} (leader)")
                } else {
                    name.clone()
                };
                let col = if Some(*id) == bag.user {
                    [255, 255, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
                c.text.draw(&label, 12, win.x + 16.0, y, col);
                if i_lead && Some(*id) != bag.user {
                    let b = &mut self.group_kick[i];
                    b.pos = (win.x + win.w - 76.0, y - 2.0);
                    if b.update(c) {
                        out.push(ClientMessage::GroupRemove { name: name.clone() });
                    }
                }
            }
            // Invite box + buttons in the footer.
            let fy = win.y + win.h - 3.0 - 42.0 + 8.0;
            let name_box = self
                .group_name
                .get_or_insert_with(|| TextBox::new(Rect::new(0.0, 0.0, 130.0, 22.0), 20));
            name_box.rect = Rect::new(win.x + 12.0, fy + 2.0, 130.0, 22.0);
            let submitted = name_box.update(c);
            if self.group_buttons.is_empty() {
                self.group_buttons = vec![
                    Button::default_style(0.0, 0.0, 60.0, "Invite"),
                    Button::default_style(0.0, 0.0, 60.0, "Leave"),
                    Button::default_style(0.0, 0.0, 120.0, "Allow: off"),
                ];
            }
            let name = name_box.text.trim().to_string();
            let b = &mut self.group_buttons[0];
            b.pos = (win.x + 148.0, fy);
            b.enabled = !name.is_empty() && (bag.group.is_empty() || i_lead);
            if (b.update(c) || submitted) && b.enabled {
                out.push(ClientMessage::GroupInvite { name: name.clone() });
                if let Some(nb) = &mut self.group_name {
                    nb.text.clear();
                }
            }
            let b = &mut self.group_buttons[1];
            b.pos = (win.x + 214.0, fy);
            b.enabled = !bag.group.is_empty();
            if b.update(c) {
                out.push(ClientMessage::GroupRemove {
                    name: bag.player_name.to_string(),
                });
            }
            let b = &mut self.group_buttons[2];
            b.pos = (win.x + 12.0, win.y + win.h - 3.0 - 42.0 - 30.0);
            b.label = Some(if bag.allow_group {
                "Allow group: on".into()
            } else {
                "Allow group: off".into()
            });
            if b.update(c) {
                out.push(ClientMessage::GroupSwitch {
                    allow: !bag.allow_group,
                });
            }
            if closed {
                self.group_open = false;
            }
        }

        // ---- Group invite prompt ----
        if let Some(from) = bag.group_invite {
            let win = self.layout.place(
                "group_invite",
                Rect::new(width as f32 / 2.0 - 150.0, 120.0, 300.0, 100.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            c.window(win, "Group invite", false);
            c.text.draw(
                &format!("{from} wants to group with you."),
                12,
                win.x + 16.0,
                win.y + 40.0,
                [255, 255, 255, 255],
            );
            if self.invite_buttons.is_empty() {
                self.invite_buttons = vec![
                    Button::default_style(0.0, 0.0, 80.0, "Accept"),
                    Button::default_style(0.0, 0.0, 80.0, "Decline"),
                ];
            }
            let b = &mut self.invite_buttons[0];
            b.pos = (win.x + 60.0, win.y + 66.0);
            if b.update(c) {
                out.push(ClientMessage::GroupResponse { accept: true });
            }
            let b = &mut self.invite_buttons[1];
            b.pos = (win.x + 160.0, win.y + 66.0);
            if b.update(c) {
                out.push(ClientMessage::GroupResponse { accept: false });
            }
        }

        // ---- NPC dialog (top-left, Zircon chrome 380/381/382) ----
        let mut npc_height = 0.0;
        let mut npc_origin = (0.0, 0.0);
        if let Some(d) = &mut self.npc {
            // Lay out text to find the needed height.
            let text_w = 350.0;
            let (lines, _) = layout_parts(c, &d.parts, text_w);
            let text_h = (lines.len() + d.quests.len()) as f32 * 18.0;
            let overflow = text_h - 140.0 - 64.0 + 35.0 + 45.0;
            let rows = if overflow > 0.0 {
                ((overflow / 20.0) as usize).min(6)
            } else {
                0
            };
            let h = 140.0 + 64.0 + rows as f32 * 20.0;
            npc_height = h;
            let r = self.layout.place(
                "npc_dialog",
                Rect::new(0.0, 0.0, 380.0, h),
                c.input,
                (width, height),
            );
            npc_origin = (r.x, r.y);
            c.draw(lib::GAME_INTER, 380, r.x, r.y);
            for i in 0..rows {
                c.draw(lib::GAME_INTER, 381, r.x, r.y + 140.0 + i as f32 * 20.0);
            }
            c.draw(lib::GAME_INTER, 382, r.x, r.y + 140.0 + rows as f32 * 20.0);
            if r.contains(mouse.0, mouse.1) {
                over = true;
            }
            let close = Rect::new(r.x + 380.0 - 21.0 - 3.0, r.y + 3.0, 21.0, 21.0);
            let close_hover = close.contains(mouse.0, mouse.1);
            c.draw(
                lib::INTERFACE,
                15,
                close.x,
                close.y + if close_hover { 1.0 } else { 0.0 },
            );
            let mut clicked_button = None;
            let mut y = r.y + 45.0;
            for line in &lines {
                let mut x = r.x + 15.0;
                for (word, button) in line {
                    let w = c.text.width(word, 13);
                    match button {
                        Some(id) => {
                            let hit = Rect::new(x, y, w, 18.0).contains(mouse.0, mouse.1);
                            let col = if hit {
                                [255, 60, 60, 255]
                            } else {
                                [255, 255, 0, 255]
                            };
                            c.text.draw(word, 13, x, y, col);
                            if hit && c.input.lmb_released && c.now >= d.last_button + 300 {
                                clicked_button = Some(*id);
                            }
                        }
                        None => c.text.draw(word, 13, x, y, [255, 255, 255, 255]),
                    }
                    x += w;
                }
                y += 18.0;
            }
            // Quest lines (Zircon shows them as links under the text).
            for q in &d.quests {
                let (label, col) = match q.state {
                    0 => (format!("[Accept] {}", q.name), [255, 255, 0, 255]),
                    2 => (format!("[Complete] {}", q.name), [80, 255, 80, 255]),
                    _ => (format!("(In progress) {}", q.name), [180, 180, 180, 255]),
                };
                let w = c.text.width(&label, 13);
                let hit = Rect::new(r.x + 15.0, y, w, 18.0).contains(mouse.0, mouse.1);
                let col = if hit && q.state != 1 {
                    [255, 60, 60, 255]
                } else {
                    col
                };
                c.text.draw(&label, 13, r.x + 15.0, y, col);
                if hit && c.input.lmb_released && c.now >= d.last_button + 300 {
                    d.last_button = c.now;
                    match q.state {
                        0 => out.push(ClientMessage::QuestAccept { quest: q.quest }),
                        2 => {
                            // First reward that offers a choice for this class, if any.
                            let choice = bag
                                .catalog
                                .quest(q.quest)
                                .and_then(|def| {
                                    def.rewards
                                        .iter()
                                        .find(|r| r.choice && r.class & (1 << bag.stats.class) != 0)
                                        .map(|r| r.index)
                                })
                                .unwrap_or(0);
                            out.push(ClientMessage::QuestComplete {
                                quest: q.quest,
                                choice,
                            });
                        }
                        _ => {}
                    }
                }
                y += 18.0;
            }
            let mut close_now = (close_hover && c.input.lmb_released && !self.layout.is_dragging())
                || c.input.escape;
            // Developer automation: press a dialog button once.
            if clicked_button.is_none() && !self.auto_button_done {
                if let Some(b) = std::env::var("ZIRCON_AUTO_NPC_BUTTON")
                    .ok()
                    .and_then(|v| v.parse().ok())
                {
                    self.auto_button_done = true;
                    clicked_button = Some(b);
                }
            }
            if let Some(id) = clicked_button {
                d.last_button = c.now;
                if id == 0 {
                    close_now = true;
                } else {
                    out.push(ClientMessage::NpcButton { button: id });
                }
            }
            if close_now {
                self.npc = None;
                out.push(ClientMessage::NpcClose);
            }
        }

        // ---- Refine / retrieve / companion pages under the dialog ----
        let page_type = self.npc.as_ref().map(|d| d.dialog_type).unwrap_or(0);
        if page_type != 14 {
            self.craft_cells = [None; 7];
        }
        if page_type == 14 {
            // Screenshot setup only: populate links, never submit a purchase.
            if self.craft_buttons.is_empty() && std::env::var_os("ZIRCON_CRAFT_PREVIEW").is_some() {
                for (slot, item) in bag.inventory.iter().enumerate() {
                    let Some(item) = item else { continue };
                    let Some(def) = bag.catalog.get(item.info) else {
                        continue;
                    };
                    let want_weapon =
                        std::env::var("ZIRCON_CRAFT_PREVIEW").is_ok_and(|v| v == "weapon");
                    let target = match def.effect {
                        _ if want_weapon
                            && def.item_type == item_type::WEAPON
                            && def.effect != 91 =>
                        {
                            Some(0)
                        }
                        60 if !want_weapon => Some(0),
                        70..=75 => Some((def.effect - 69) as usize),
                        _ => None,
                    };
                    if let Some(target) = target {
                        self.craft_cells[target] = Some((slot as u8, item.id));
                    }
                }
            }
            let win = self.layout.place(
                "weapon_crafting",
                Rect::new(npc_origin.0, npc_origin.1 + npc_height, 410.0, 242.0),
                c.input,
                (width, height),
            );
            over |= win.contains(mouse.0, mouse.1);
            let close_craft = c.window(win, "Weapon crafting", true) && !self.layout.is_dragging();
            let names = ["Base", "Yellow", "Blue", "Red", "Purple", "Green", "Grey"];
            let mut rolls = 0;
            let mut cost = 0u64;
            for (i, name) in names.iter().enumerate() {
                let item = self.craft_cells[i].and_then(|(slot, id)| {
                    bag.inventory
                        .get(slot as usize)
                        .and_then(Option::as_ref)
                        .filter(|it| it.id == id)
                });
                if item.is_none() {
                    self.craft_cells[i] = None;
                }
                if let Some(def) = item.and_then(|it| bag.catalog.get(it.info)) {
                    if i == 0 {
                        cost = if def.effect == 60 {
                            1_000_000
                        } else {
                            match def.rarity {
                                0 => 30_000_000,
                                1 => 60_000_000,
                                _ => 80_000_000,
                            }
                        };
                    } else {
                        rolls += def.shape;
                    }
                }
                let x = win.x + 12.0 + i as f32 * 56.0;
                c.text.draw(name, 10, x, win.y + 37.0, [230, 215, 170, 255]);
                let cell = Rect::new(x, win.y + 53.0, CELL, CELL);
                let hover = cell.contains(mouse.0, mouse.1);
                let selected_item = item.cloned().map(|mut it| {
                    it.count = 1;
                    it
                });
                draw_item_cell(c, bag.catalog, cell, selected_item.as_ref(), hover, false);
                if hover && c.input.lmb_released {
                    self.craft_cells[i] = None;
                }
            }
            if self.craft_buttons.is_empty() {
                for name in ["Warrior", "Wizard", "Taoist", "Assassin", "Craft"] {
                    self.craft_buttons
                        .push(Button::default_style(0.0, 0.0, 90.0, name));
                }
            }
            for i in 0..4 {
                let b = &mut self.craft_buttons[i];
                b.pos = (win.x + 12.0 + i as f32 * 96.0, win.y + 98.0);
                b.selected = self.craft_class == i;
                if b.update(c) {
                    self.craft_class = i;
                }
            }
            c.text.draw(
                &format!("Cost: {cost} gold    Stat rolls: {rolls}"),
                11,
                win.x + 12.0,
                win.y + 134.0,
                GOLD,
            );
            c.text.draw(
                "Right-click bag items to add; click a slot to remove.",
                10,
                win.x + 12.0,
                win.y + 155.0,
                [220, 220, 220, 255],
            );
            c.text.draw(
                "Reroll keeps enhancements; replaces other bonuses.",
                10,
                win.x + 12.0,
                win.y + 173.0,
                [240, 180, 130, 255],
            );
            let b = &mut self.craft_buttons[4];
            b.pos = (win.x + 12.0, win.y + 198.0);
            b.enabled = self.craft_cells[0].is_some() && bag.gold >= cost;
            if b.update(c) {
                if let Some(base) = self.craft_cells[0] {
                    out.push(ClientMessage::NpcWeaponCraft {
                        base,
                        materials: self.craft_cells[1..].try_into().unwrap(),
                        class: [
                            mir_proto::Class::Warrior,
                            mir_proto::Class::Wizard,
                            mir_proto::Class::Taoist,
                            mir_proto::Class::Assassin,
                        ][self.craft_class],
                    });
                    self.craft_cells = [None; 7];
                }
            }
            if close_craft {
                self.npc = None;
                self.craft_cells = [None; 7];
                out.push(ClientMessage::NpcClose);
            }
        }
        if !matches!(page_type, 11..=13 | 15) {
            self.accessory_target = None;
            self.accessory_ore = None;
            self.accessory_materials.clear();
        }
        if matches!(page_type, 11..=13 | 15) {
            let lookup = |cell: (Grid, u8, u32)| {
                let grid = match cell.0 {
                    Grid::Inventory => bag.inventory,
                    Grid::Equipment => bag.equipment,
                    Grid::Storage => bag.storage,
                };
                grid.get(cell.1 as usize)
                    .and_then(Option::as_ref)
                    .filter(|item| item.id == cell.2)
            };
            self.accessory_target = self.accessory_target.filter(|cell| lookup(*cell).is_some());
            self.accessory_ore = self.accessory_ore.filter(|cell| lookup(*cell).is_some());
            self.accessory_materials
                .retain(|cell| lookup(*cell).is_some());
            let win = self.layout.place(
                "accessory_refining",
                Rect::new(
                    npc_origin.0,
                    npc_origin.1 + npc_height,
                    420.0,
                    if page_type == 11 || page_type == 15 {
                        450.0
                    } else {
                        190.0
                    },
                ),
                c.input,
                (width, height),
            );
            over |= win.contains(mouse.0, mouse.1);
            let title = match page_type {
                11 => "Accessory upgrade",
                12 => "Accessory level",
                13 => "Accessory reset",
                _ => "Accessory refine",
            };
            let close = c.window(win, title, true) && !self.layout.is_dragging();
            let target = self.accessory_target.and_then(lookup);
            let target_name = target
                .map(|it| bag.catalog.name(it.info))
                .unwrap_or_else(|| "None".into());
            c.text.draw(
                &format!("Target: {target_name}"),
                12,
                win.x + 12.0,
                win.y + 38.0,
                GOLD,
            );
            if let Some(item) = target {
                if page_type == 12 || page_type == 11 {
                    let threshold = [
                        0, 5, 20, 80, 350, 1500, 6200, 26500, 114000, 490000, 2090000,
                    ]
                    .get(item.level.max(0) as usize)
                    .copied()
                    .unwrap_or(0);
                    c.text.draw(
                        &format!(
                            "Level {}   XP {}/{}   {}",
                            item.level,
                            item.accessory_experience,
                            threshold,
                            if item.refinable {
                                "Ready"
                            } else {
                                "Needs matching items"
                            }
                        ),
                        10,
                        win.x + 12.0,
                        win.y + 59.0,
                        [220, 220, 220, 255],
                    );
                }
            }
            if matches!(page_type, 11 | 12 | 15) {
                c.text.draw(
                    &format!(
                        "Matching items: {} (right-click to add)",
                        self.accessory_materials.len()
                    ),
                    11,
                    win.x + 12.0,
                    win.y + 80.0,
                    [220, 220, 220, 255],
                );
            }
            if page_type == 11 || page_type == 15 {
                let ore_name = self
                    .accessory_ore
                    .and_then(lookup)
                    .map(|it| bag.catalog.name(it.info))
                    .unwrap_or_else(|| "None".into());
                c.text.draw(
                    &format!("Corundum: {ore_name}"),
                    11,
                    win.x + 12.0,
                    win.y + 101.0,
                    [220, 220, 220, 255],
                );
                c.text.draw(
                    "Corundum refine can destroy target. Cost: 50,000 gold.",
                    10,
                    win.x + 12.0,
                    win.y + 122.0,
                    [240, 170, 140, 255],
                );
            }
            if page_type == 13 {
                c.text.draw(
                    "Reset removes refine stats. Cost: 1,000,000 gold.",
                    11,
                    win.x + 12.0,
                    win.y + 72.0,
                    [240, 170, 140, 255],
                );
            }
            const TYPES: &[(u8, &str)] = &[
                (2, "DC"),
                (3, "Spell"),
                (4, "Fire"),
                (5, "Ice"),
                (6, "Lightning"),
                (7, "Wind"),
                (8, "Holy"),
                (9, "Dark"),
                (10, "Phantom"),
                (12, "HP"),
                (13, "MP"),
                (14, "AC"),
                (15, "MR"),
                (16, "Accuracy"),
                (17, "Agility"),
                (18, "DC%"),
                (19, "Spell%"),
                (20, "HP%"),
                (21, "MP%"),
            ];
            if page_type == 11 || page_type == 15 {
                if self.accessory_buttons.len() != TYPES.len() + 2 {
                    self.accessory_buttons = TYPES
                        .iter()
                        .map(|(_, name)| Button::default_style(0.0, 0.0, 124.0, name))
                        .collect();
                    self.accessory_buttons
                        .push(Button::default_style(0.0, 0.0, 140.0, "Upgrade"));
                    self.accessory_buttons.push(Button::default_style(
                        0.0,
                        0.0,
                        220.0,
                        "Refine with Corundum",
                    ));
                }
                let top = 148.0;
                for (i, (kind, _)) in TYPES.iter().enumerate() {
                    let b = &mut self.accessory_buttons[i];
                    b.pos = (
                        win.x + 12.0 + (i % 3) as f32 * 132.0,
                        win.y + top + (i / 3) as f32 * 29.0,
                    );
                    b.selected = self.accessory_type == *kind;
                    if b.update(c) {
                        self.accessory_type = *kind
                    }
                }
            }
            if self.accessory_buttons.is_empty() {
                self.accessory_buttons
                    .push(Button::default_style(0.0, 0.0, 140.0, "Apply"));
            }
            let apply_index = if self.accessory_buttons.len() > 2 {
                self.accessory_buttons.len() - 2
            } else {
                0
            };
            let apply = &mut self.accessory_buttons[apply_index];
            apply.pos = (win.x + 12.0, win.y + win.h - 48.0);
            apply.label = Some(
                match page_type {
                    11 => "Upgrade",
                    12 => "Combine",
                    13 => "Reset",
                    _ => "Refine",
                }
                .into(),
            );
            apply.enabled = self.accessory_target.is_some()
                && match page_type {
                    11 => target.is_some_and(|it| it.refinable && !it.non_refinable),
                    12 => !self.accessory_materials.is_empty(),
                    13 => bag.gold >= 1_000_000,
                    _ => self.accessory_ore.is_some() && bag.gold >= 50_000,
                };
            if apply.update(c) && apply.enabled {
                let target = self.accessory_target.unwrap();
                out.push(match page_type {
                    11 => ClientMessage::NpcAccessoryUpgrade {
                        target,
                        refine_type: self.accessory_type,
                    },
                    12 => ClientMessage::NpcAccessoryLevel {
                        target,
                        materials: self.accessory_materials.clone(),
                    },
                    13 => ClientMessage::NpcAccessoryReset { target },
                    _ => ClientMessage::NpcAccessoryRefine {
                        target,
                        ore: self.accessory_ore.unwrap(),
                        materials: self.accessory_materials.clone(),
                        refine_type: self.accessory_type,
                    },
                });
                self.accessory_target = None;
                self.accessory_ore = None;
                self.accessory_materials.clear();
            }
            if page_type == 11 {
                let risky = self.accessory_buttons.last_mut().unwrap();
                risky.pos = (win.x + 166.0, win.y + win.h - 48.0);
                risky.enabled = target
                    .is_some_and(|it| !it.refinable && !it.non_refinable && it.level <= 1)
                    && self.accessory_ore.is_some()
                    && bag.gold >= 50_000;
                if risky.update(c) && risky.enabled {
                    out.push(ClientMessage::NpcAccessoryRefine {
                        target: self.accessory_target.unwrap(),
                        ore: self.accessory_ore.unwrap(),
                        materials: self.accessory_materials.clone(),
                        refine_type: self.accessory_type,
                    });
                    self.accessory_target = None;
                    self.accessory_ore = None;
                    self.accessory_materials.clear();
                }
            }
            if close {
                self.npc = None;
                self.accessory_target = None;
                self.accessory_ore = None;
                self.accessory_materials.clear();
                out.push(ClientMessage::NpcClose);
            }
        }
        if page_type != 3 {
            self.refine_ores.clear();
            self.refine_items.clear();
            self.refine_specials.clear();
        }
        if page_type == 3 {
            let win = self.layout.place(
                "refine",
                Rect::new(npc_origin.0, npc_origin.1 + npc_height, 380.0, 300.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            c.window(win, "Refine", true);
            let weapon = bag
                .equipment
                .first()
                .and_then(|w| w.as_ref())
                .map(|w| bag.catalog.name(w.info))
                .unwrap_or_else(|| "no weapon equipped".into());
            c.text.draw(
                &format!("Weapon: {weapon}   Cost: 50,000 gold"),
                11,
                win.x + 12.0,
                win.y + 36.0,
                [255, 255, 200, 255],
            );
            if self.refine_buttons.is_empty() {
                let types = [
                    "Dura", "DC", "SP", "Fire", "Ice", "Light", "Wind", "Holy", "Dark", "Phan",
                ];
                let qualities = ["Rush", "Quick", "Std", "Careful", "Precise"];
                for t in types {
                    self.refine_buttons
                        .push(Button::default_style(0.0, 0.0, 34.0, t));
                }
                for q in qualities {
                    self.refine_buttons
                        .push(Button::default_style(0.0, 0.0, 60.0, q));
                }
                self.refine_buttons
                    .push(Button::default_style(0.0, 0.0, 110.0, "Refine"));
            }
            c.text.draw(
                "Type:",
                11,
                win.x + 12.0,
                win.y + 58.0,
                [220, 220, 220, 255],
            );
            for i in 0..10 {
                let b = &mut self.refine_buttons[i];
                b.pos = (win.x + 12.0 + i as f32 * 36.0, win.y + 72.0);
                b.selected = self.refine_type == (i + 1) as u8;
                if b.update(c) {
                    self.refine_type = (i + 1) as u8;
                }
            }
            c.text.draw(
                "Quality (1 min, 30 min, 1 h, 6 h, 1 day):",
                11,
                win.x + 12.0,
                win.y + 104.0,
                [220, 220, 220, 255],
            );
            for i in 0..5 {
                let b = &mut self.refine_buttons[10 + i];
                b.pos = (win.x + 12.0 + i as f32 * 64.0, win.y + 118.0);
                b.selected = self.refine_quality == i as u8;
                if b.update(c) {
                    self.refine_quality = i as u8;
                }
            }
            c.text.draw(
                "Right-click bag items: black iron ore (5), common jewellery (3), special (1).",
                10,
                win.x + 12.0,
                win.y + 150.0,
                [160, 160, 160, 255],
            );
            let mut y = win.y + 166.0;
            let mut remove: Option<(usize, usize)> = None;
            for (li, (label, list)) in [
                ("Ore", &self.refine_ores),
                ("Items", &self.refine_items),
                ("Special", &self.refine_specials),
            ]
            .iter()
            .enumerate()
            {
                let names: Vec<String> = list
                    .iter()
                    .map(|(_, s, n)| {
                        bag.inventory
                            .get(*s as usize)
                            .and_then(|c| c.as_ref())
                            .map(|it| format!("{} x{n}", bag.catalog.name(it.info)))
                            .unwrap_or_else(|| "?".into())
                    })
                    .collect();
                let r = Rect::new(win.x + 12.0, y, win.w - 24.0, 16.0);
                let hover = r.contains(mouse.0, mouse.1);
                c.text.draw(
                    &format!("{label}: {}", names.join(", ")),
                    11,
                    win.x + 12.0,
                    y,
                    if hover && !list.is_empty() {
                        [255, 150, 150, 255]
                    } else {
                        [255, 255, 255, 255]
                    },
                );
                if hover && c.input.lmb_pressed && !list.is_empty() {
                    remove = Some((li, list.len() - 1));
                }
                y += 18.0;
            }
            if let Some((li, i)) = remove {
                match li {
                    0 => {
                        self.refine_ores.remove(i);
                    }
                    1 => {
                        self.refine_items.remove(i);
                    }
                    _ => {
                        self.refine_specials.remove(i);
                    }
                }
            }
            let fy = win.y + win.h - 3.0 - 42.0 + 8.0;
            let ready = bag
                .equipment
                .first()
                .and_then(Option::as_ref)
                .is_some_and(|w| w.refinable && !w.non_refinable);
            if !ready {
                c.text.draw(
                    if bag
                        .equipment
                        .first()
                        .and_then(Option::as_ref)
                        .is_some_and(|w| w.non_refinable)
                    {
                        "This weapon cannot be refined."
                    } else {
                        "Weapon needs combat experience before refining."
                    },
                    10,
                    win.x + 12.0,
                    win.y + 240.0,
                    [240, 180, 130, 255],
                );
            }
            let b = &mut self.refine_buttons[15];
            b.pos = (win.x + 12.0, fy);
            b.enabled = ready && bag.gold >= 50_000;
            if b.update(c) && b.enabled {
                out.push(ClientMessage::NpcRefine {
                    refine_type: self.refine_type,
                    quality: self.refine_quality,
                    ores: self.refine_ores.clone(),
                    items: self.refine_items.clone(),
                    specials: self.refine_specials.clone(),
                });
                self.refine_ores.clear();
                self.refine_items.clear();
                self.refine_specials.clear();
            }
        }
        if page_type == 4 {
            let rows = bag.refines.len().max(1);
            let win = self.layout.place(
                "refined_weapons",
                Rect::new(
                    npc_origin.0,
                    npc_origin.1 + npc_height,
                    380.0,
                    60.0 + rows as f32 * 22.0 + 46.0,
                ),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            c.window(win, "Refined weapons", true);
            if bag.refines.is_empty() {
                c.text.draw(
                    "Nothing in the furnace.",
                    11,
                    win.x + 12.0,
                    win.y + 40.0,
                    [200, 200, 200, 255],
                );
            }
            if self.retrieve_buttons.len() != bag.refines.len() {
                self.retrieve_buttons = bag
                    .refines
                    .iter()
                    .map(|_| Button::default_style(0.0, 0.0, 70.0, "Retrieve"))
                    .collect();
            }
            let types = [
                "",
                "Durability",
                "DC",
                "Spell power",
                "Fire",
                "Ice",
                "Lightning",
                "Wind",
                "Holy",
                "Dark",
                "Phantom",
            ];
            for (i, r) in bag.refines.iter().enumerate() {
                let y = win.y + 40.0 + i as f32 * 22.0;
                let ready = r.ready_in_ms == 0;
                let when = if ready {
                    "ready".to_string()
                } else {
                    let s = r.ready_in_ms / 1000;
                    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
                };
                c.text.draw(
                    &format!(
                        "{} ({}) {}% - {when}",
                        bag.catalog.name(r.weapon.info),
                        types.get(r.refine_type as usize).unwrap_or(&""),
                        r.chance
                    ),
                    11,
                    win.x + 12.0,
                    y,
                    if ready {
                        [255, 255, 255, 255]
                    } else {
                        [180, 180, 180, 255]
                    },
                );
                let b = &mut self.retrieve_buttons[i];
                b.pos = (win.x + win.w - 84.0, y - 2.0);
                b.enabled = ready;
                if b.update(c) && b.enabled {
                    out.push(ClientMessage::NpcRefineRetrieve { index: r.index });
                }
            }
        }
        if page_type == 5 {
            let rows = bag.companion_shop.len() + bag.companions.len();
            let win = self.layout.place(
                "companions",
                Rect::new(
                    npc_origin.0,
                    npc_origin.1 + npc_height,
                    400.0,
                    108.0 + rows as f32 * 22.0 + 46.0,
                ),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            c.window(win, "Companions", true);
            let name_box = self
                .companion_name
                .get_or_insert_with(|| TextBox::new(Rect::new(0.0, 0.0, 140.0, 22.0), 15));
            c.text.draw(
                "Name for a new companion:",
                11,
                win.x + 12.0,
                win.y + 38.0,
                [220, 220, 220, 255],
            );
            name_box.rect = Rect::new(win.x + 180.0, win.y + 34.0, 140.0, 22.0);
            name_box.update(c);
            let name = name_box.text.trim().to_string();
            let valid_name = (2..=15).contains(&name.chars().count())
                && name.chars().all(|ch| ch.is_ascii_alphanumeric());
            c.text.draw(
                "Enter 2-15 letters or digits to enable Adopt.",
                10,
                win.x + 12.0,
                win.y + 61.0,
                [230, 200, 130, 255],
            );
            if self.companion_offer_buttons.len() != bag.companion_shop.len() {
                self.companion_offer_buttons = bag
                    .companion_shop
                    .iter()
                    .map(|_| Button::default_style(0.0, 0.0, 64.0, "Adopt"))
                    .collect();
            }
            let mut y = win.y + 84.0;
            for (i, o) in bag.companion_shop.iter().enumerate() {
                let required = if !o.unlocked && o.unlock_item != 0 {
                    format!(" (unlock: {})", bag.catalog.name(o.unlock_item))
                } else {
                    String::new()
                };
                c.text.draw(
                    &format!("{} - {} {}{required}", o.name, o.price, o.currency),
                    11,
                    win.x + 12.0,
                    y,
                    if o.unlocked {
                        [255, 255, 255, 255]
                    } else {
                        [205, 205, 205, 255]
                    },
                );
                let b = &mut self.companion_offer_buttons[i];
                b.pos = (win.x + win.w - 78.0, y - 2.0);
                b.label = Some(if o.unlocked {
                    "Adopt".into()
                } else {
                    "Unlock".into()
                });
                b.enabled = !o.unlocked || valid_name;
                if b.update(c) && b.enabled {
                    if o.unlocked {
                        out.push(ClientMessage::CompanionAdopt {
                            index: o.index,
                            name: name.clone(),
                        });
                        if let Some(nb) = &mut self.companion_name {
                            nb.text.clear();
                        }
                    } else {
                        out.push(ClientMessage::CompanionUnlock { index: o.index });
                    }
                }
                y += 22.0;
            }
            if self.companion_own_buttons.len() != bag.companions.len() * 3 {
                self.companion_own_buttons = bag
                    .companions
                    .iter()
                    .flat_map(|_| {
                        [
                            Button::default_style(0.0, 0.0, 60.0, "Out"),
                            Button::default_style(0.0, 0.0, 60.0, "Store"),
                            Button::default_style(0.0, 0.0, 64.0, "Release"),
                        ]
                    })
                    .collect();
            }
            for (i, cmp) in bag.companions.iter().enumerate() {
                c.text.draw(
                    &format!(
                        "{} the {} Lv{}{}",
                        cmp.name,
                        cmp.kind,
                        cmp.level,
                        if cmp.active { " (out)" } else { "" }
                    ),
                    11,
                    win.x + 12.0,
                    y,
                    [255, 255, 0, 255],
                );
                let b = &mut self.companion_own_buttons[i * 3];
                b.pos = (win.x + win.w - 204.0, y - 2.0);
                b.enabled = !cmp.active;
                if b.update(c) && b.enabled {
                    out.push(ClientMessage::CompanionRetrieve { index: cmp.index });
                }
                let b = &mut self.companion_own_buttons[i * 3 + 1];
                b.pos = (win.x + win.w - 142.0, y - 2.0);
                b.enabled = cmp.active;
                if b.update(c) && b.enabled {
                    out.push(ClientMessage::CompanionStore);
                }
                let b = &mut self.companion_own_buttons[i * 3 + 2];
                b.pos = (win.x + win.w - 78.0, y - 2.0);
                b.enabled = cmp.items.is_empty() && cmp.food.is_none();
                if b.update(c) && b.enabled {
                    out.push(ClientMessage::CompanionRelease { index: cmp.index });
                }
                y += 22.0;
            }
        } else if let Some(b) = &mut self.companion_name {
            b.focused = false;
        }

        // ---- Companion window: the active companion and its bag ----
        if self.companion_open {
            let win = self.layout.place(
                "companion",
                Rect::new(width as f32 / 2.0 - 315.0, 80.0, 630.0, 450.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            let closed = c.window(win, "Companion", true) && !self.layout.is_dragging();
            match bag.companions.iter().find(|cmp| cmp.active) {
                None => c.text.draw(
                    "No companion is out. Visit the companion keeper.",
                    11,
                    win.x + 12.0,
                    win.y + 40.0,
                    [200, 200, 200, 255],
                ),
                Some(cmp) => {
                    if self.companion_skill_buttons.len() != 7 {
                        self.companion_skill_buttons = (0..7)
                            .map(|_| Button::default_style(0.0, 0.0, 80.0, "Reroll"))
                            .collect();
                    }
                    c.text.draw(
                        &format!("{} the {}  Lv {}", cmp.name, cmp.kind, cmp.level),
                        12,
                        win.x + 12.0,
                        win.y + 38.0,
                        [255, 255, 0, 255],
                    );
                    c.text.draw(
                        &format!(
                            "Exp {}/{}   Hunger {}   Bag {}/{} ({}/{} weight)",
                            cmp.experience,
                            cmp.max_experience,
                            cmp.hunger,
                            cmp.items.len(),
                            cmp.bag_size,
                            cmp.bag_weight,
                            cmp.max_weight
                        ),
                        10,
                        win.x + 12.0,
                        win.y + 56.0,
                        [220, 220, 220, 255],
                    );
                    c.text
                        .draw("Level bonuses", 11, win.x + 12.0, win.y + 87.0, GOLD);
                    c.text.draw(
                        "Cost: 1 matching Elixir of Regret; no gold",
                        9,
                        win.x + 130.0,
                        win.y + 89.0,
                        GOLD,
                    );
                    for (row, level) in [3, 5, 7, 10, 11, 13, 15].into_iter().enumerate() {
                        let bonus = cmp.bonuses.iter().find(|(earned, _, _)| *earned == level);
                        let (label, color) = match bonus {
                            Some((_, stat, amount)) => (
                                format!(
                                    "Lv {level}: {} +{amount}",
                                    stat_name(*stat)
                                        .map(str::to_string)
                                        .unwrap_or_else(|| format!("Stat {stat}"))
                                ),
                                [125, 235, 125, 255],
                            ),
                            None => (format!("Lv {level}: Not gained"), [160, 160, 160, 255]),
                        };
                        c.text.draw(
                            &label,
                            10,
                            win.x + 12.0,
                            win.y + 109.0 + row as f32 * 30.0,
                            color,
                        );
                        let (slot, owned) =
                            companion_elixir_inventory(bag.inventory, bag.catalog, level);
                        c.text.draw(
                            &format!("Cost: 1 Elixir Of Regret Lv {level} (owned {owned})"),
                            9,
                            win.x + 12.0,
                            win.y + 121.0 + row as f32 * 30.0,
                            if owned > 0 {
                                [190, 190, 190, 255]
                            } else {
                                [235, 155, 100, 255]
                            },
                        );
                        let button = &mut self.companion_skill_buttons[row];
                        button.pos = (win.x + 325.0, win.y + 103.0 + row as f32 * 30.0);
                        button.label = Some("Reroll".into());
                        button.selected = false;
                        button.enabled = bonus.is_some()
                            && cmp.level >= level
                            && slot.is_some()
                            && !bag.dead
                            && bag.use_item_time <= c.now;
                        if button.update(c) {
                            companion_reroll_action(
                                level,
                                cmp.reroll_enabled & (1 << row) != 0,
                                slot.unwrap(),
                                out,
                            );
                        }
                    }
                    c.text.draw("Food", 10, win.x + 12.0, win.y + 331.0, GOLD);
                    let food_cell = Rect::new(win.x + 74.0, win.y + 320.0, CELL, CELL);
                    let food_hover = food_cell.contains(mouse.0, mouse.1);
                    draw_item_cell(
                        c,
                        bag.catalog,
                        food_cell,
                        cmp.food.as_ref(),
                        food_hover,
                        false,
                    );
                    if food_hover {
                        if let Some(item) = &cmp.food {
                            self.tooltip = Some((item.info, mouse.0, mouse.1));
                            self.tooltip_item = Some(item.clone());
                            if c.input.rmb_pressed {
                                out.push(ClientMessage::CompanionFoodTake);
                            }
                        }
                    }
                    c.text.draw("Bag", 10, win.x + 130.0, win.y + 331.0, GOLD);
                    let bag_cell = Rect::new(win.x + 185.0, win.y + 320.0, CELL, CELL);
                    let bag_hover = bag_cell.contains(mouse.0, mouse.1);
                    draw_item_cell(c, bag.catalog, bag_cell, cmp.bag.as_ref(), bag_hover, false);
                    if bag_hover {
                        if let Some(item) = &cmp.bag {
                            self.tooltip = Some((item.info, mouse.0, mouse.1));
                            self.tooltip_item = Some(item.clone());
                            if c.input.rmb_pressed {
                                out.push(ClientMessage::CompanionBagUnequip);
                            }
                        }
                    }
                    c.text.draw("Bag", 11, win.x + 420.0, win.y + 38.0, GOLD);
                    for i in 0..30 {
                        let cell = Rect::new(
                            win.x + 420.0 + (i % 5) as f32 * PITCH,
                            win.y + 56.0 + (i / 5) as f32 * PITCH,
                            CELL,
                            CELL,
                        );
                        let item = cmp.items.get(i);
                        let hover = cell.contains(mouse.0, mouse.1);
                        draw_item_cell(c, bag.catalog, cell, item, hover, false);
                        if i >= cmp.bag_size as usize {
                            c.fill(cell, [0.0, 0.0, 0.0, 0.5]);
                        }
                        if hover {
                            if let Some(item) = item {
                                self.tooltip = Some((item.info, mouse.0, mouse.1));
                                self.tooltip_item = Some(item.clone());
                                if c.input.rmb_pressed {
                                    out.push(ClientMessage::CompanionBagTake {
                                        index: cmp.index,
                                        slot: i as u8,
                                    });
                                }
                            }
                        }
                    }
                    c.text.draw(
                        &format!("Weight {}/{}", cmp.bag_weight, cmp.max_weight),
                        10,
                        win.x + 420.0,
                        win.y + 335.0,
                        [220, 220, 220, 255],
                    );
                    c.text.draw(
                        "Right-click food/bag in inventory to equip; slot to return.",
                        9,
                        win.x + 12.0,
                        win.y + 365.0,
                        [195, 195, 195, 255],
                    );
                    let pickup_status = if cmp.hunger <= 0 {
                        "Pickup paused: hungry. Use food or equip automatic food."
                    } else if cmp.bag_size == 0 {
                        "No item slots: equip a companion bag or reach level 2."
                    } else if cmp.items.len() >= cmp.bag_size as usize {
                        "Bag slots full: only gold or matching stacks that fit."
                    } else {
                        "Collects your monster drops nearby; right-click bag items to take."
                    };
                    c.text.draw(
                        pickup_status,
                        10,
                        win.x + 12.0,
                        win.y + 388.0,
                        [235, 205, 140, 255],
                    );
                    c.text.draw(
                        "Food: Green Apple up to Lv 4; Chestnut Rice Ball up to Lv 8.",
                        9,
                        win.x + 12.0,
                        win.y + 408.0,
                        [195, 195, 195, 255],
                    );
                    c.text.draw("Food slot eats at zero hunger. Bag and food level limits appear in tooltips.", 9, win.x + 12.0, win.y + 423.0, [195, 195, 195, 255]);
                }
            }
            if closed {
                self.companion_open = false;
            }
        }

        // ---- Goods (shop) window under the dialog ----
        if let Some(d) = &mut self.npc {
            if d.dialog_type == 1 {
                let rows = d.goods.len().min(7);
                let win = self.layout.place(
                    "goods",
                    Rect::new(
                        npc_origin.0,
                        npc_origin.1 + npc_height,
                        245.0,
                        37.0 + rows as f32 * 43.0
                            + if d.sell_types.is_empty() { 50.0 } else { 76.0 }
                            + if d.buy_prompt { 45.0 } else { 0.0 },
                    ),
                    c.input,
                    (width, height),
                );
                let max_scroll = d.goods.len().saturating_sub(7);
                if win.contains(mouse.0, mouse.1) {
                    over = true;
                    if c.input.wheel != 0.0 && max_scroll > 0 {
                        let next = d.goods_scroll as f32 - c.input.wheel;
                        d.goods_scroll = next.round().clamp(0.0, max_scroll as f32) as usize;
                    }
                }
                let closed = c.window(win, &format!("Goods ({})", d.currency), true)
                    && !self.layout.is_dragging();
                let balance = if d.currency.eq_ignore_ascii_case("Gold") {
                    bag.gold
                } else {
                    bag.currencies
                        .iter()
                        .find(|currency| currency.name.eq_ignore_ascii_case(&d.currency))
                        .map_or(0, |currency| currency.amount.max(0) as u64)
                };
                if max_scroll > 0 {
                    // Scrollbar on the right of the list (Zircon `DXVScrollBar`).
                    let track =
                        Rect::new(win.x + 231.0, win.y + 37.0, 8.0, rows as f32 * 43.0 - 3.0);
                    c.fill(track, [0.0, 0.0, 0.0, 0.5]);
                    let thumb_h = (track.h * rows as f32 / d.goods.len() as f32).max(12.0);
                    let thumb_y =
                        track.y + (track.h - thumb_h) * d.goods_scroll as f32 / max_scroll as f32;
                    c.fill(
                        Rect::new(track.x, thumb_y, track.w, thumb_h),
                        [198.0 / 255.0, 166.0 / 255.0, 99.0 / 255.0, 1.0],
                    );
                }
                let first = d.goods_scroll.min(max_scroll);
                let mut focus_buy_count = false;
                for (i, g) in d.goods.iter().enumerate().skip(first).take(7) {
                    let row = Rect::new(
                        win.x + 10.0,
                        win.y + 37.0 + (i - first) as f32 * 43.0,
                        219.0,
                        40.0,
                    );
                    let hover = row.contains(mouse.0, mouse.1);
                    let selected = d.selected_good == Some(i);
                    c.fill(
                        row,
                        if selected {
                            [80.0 / 255.0, 80.0 / 255.0, 125.0 / 255.0, 1.0]
                        } else {
                            [25.0 / 255.0, 20.0 / 255.0, 0.0, 1.0]
                        },
                    );
                    c.border(row, if hover { GOLD } else { [99, 83, 50, 255] });
                    let cell = Rect::new(row.x + 2.0, row.y + 2.0, CELL, CELL);
                    let inst = ItemInstance {
                        id: 0,
                        info: g.info,
                        count: 1,
                        durability: 0,
                        max_durability: 0,
                        added: Vec::new(),
                        enhancements: Vec::new(),
                        refinable: false,
                        level: 0,
                        accessory_experience: 0,
                        non_refinable: false,
                        bound: false,
                        worthless: false,
                        locked: false,
                        expires_at: None,
                        colour: 0,
                    };
                    draw_item_cell(c, bag.catalog, cell, Some(&inst), false, false);
                    let name = bag.catalog.name(g.info);
                    c.text
                        .draw(&name, 12, row.x + 40.0, row.y + 2.0, [255, 255, 255, 255]);
                    let usable = bag
                        .catalog
                        .get(g.info)
                        .map(|d| d.required_type != 0 || d.required_amount <= bag.stats.level)
                        .unwrap_or(true);
                    let (req, rcol) = if usable {
                        ("Can use item", [127, 255, 212, 255])
                    } else {
                        ("Cannot use item", [255, 60, 60, 255])
                    };
                    c.text.draw(req, 11, row.x + 40.0, row.y + 22.0, rcol);
                    let price = format!("{}", g.price);
                    let pw = c.text.width(&price, 12);
                    let pcol = if g.price <= balance {
                        [255, 255, 0, 255]
                    } else {
                        [255, 60, 60, 255]
                    };
                    c.text
                        .draw(&price, 12, row.x + row.w - pw - 30.0, row.y + 22.0, pcol);
                    if d.currency.eq_ignore_ascii_case("Gold") {
                        c.draw(lib::STORE_ITEMS, 121, row.x + row.w - 24.0, row.y + 22.0);
                    }
                    if hover {
                        if let Some(def) = bag.catalog.get(g.info) {
                            self.tooltip = Some((def.index, mouse.0, mouse.1));
                        }
                        if c.input.lmb_pressed {
                            d.selected_good = Some(i);
                            d.buy_prompt = c.input.shift;
                            d.buy_count.text.clear();
                            if d.buy_prompt {
                                if let Some(item) = bag.catalog.get(g.info).filter(|item| {
                                    matches!(item.item_type, item_type::POISON | item_type::AMULET)
                                }) {
                                    let affordable = balance / g.price.max(1);
                                    let suggested = affordable
                                        .min(item.stack_size.max(1) as u64)
                                        .clamp(1, 5_000);
                                    d.buy_count.text = suggested.to_string();
                                }
                            }
                            focus_buy_count = d.buy_prompt;
                        }
                    }
                }
                let buy_row = win.y + 40.0 + rows as f32 * 43.0;
                let mut buy_submitted = false;
                if d.buy_prompt {
                    let selected_item = d
                        .selected_good
                        .and_then(|i| d.goods.get(i))
                        .and_then(|g| bag.catalog.get(g.info));
                    let max = selected_item.map(|item| item.stack_size.max(1));
                    let unit = if selected_item.is_some_and(|item| {
                        matches!(item.item_type, item_type::POISON | item_type::AMULET)
                    }) {
                        "charges"
                    } else {
                        "quantity"
                    };
                    c.text.draw(
                        &format!("Buy {unit} (max {})", max.unwrap_or(1)),
                        10,
                        win.x + 10.0,
                        buy_row + 2.0,
                        [220, 220, 220, 255],
                    );
                    d.buy_count.rect = Rect::new(win.x + 166.0, buy_row, 62.0, 22.0);
                    buy_submitted = d.buy_count.update(c);
                    d.buy_count.text.retain(|ch| ch.is_ascii_digit());
                    if focus_buy_count {
                        d.buy_count.focused = true;
                    }
                    if c.input.escape {
                        d.buy_prompt = false;
                        d.buy_count.focused = false;
                    }
                } else {
                    d.buy_count.focused = false;
                }
                if !d.sell_types.is_empty() {
                    let y = buy_row + if d.buy_prompt { 46.0 } else { 0.0 };
                    c.text.draw(
                        "Sell count/stack (blank: all)",
                        10,
                        win.x + 10.0,
                        y + 2.0,
                        [220, 220, 220, 255],
                    );
                    d.sell_count.rect = Rect::new(win.x + 166.0, y, 62.0, 22.0);
                    d.sell_count.update(c);
                    d.sell_count.text.retain(|ch| ch.is_ascii_digit());
                }
                let sell_count = d.sell_count.text.clone();
                let buy = self
                    .buy_button
                    .get_or_insert_with(|| Button::default_style(0.0, 0.0, 80.0, "Buy"));
                buy.pos = (win.x + 30.0, win.y + win.h - 43.0);
                let requested_buy = d.selected_good.and_then(|index| {
                    d.goods.get(index).and_then(|good| {
                        bag.catalog.get(good.info).and_then(|item| {
                            purchase_count(
                                if d.buy_prompt { &d.buy_count.text } else { "" },
                                item.stack_size,
                            )
                        })
                    })
                });
                let total = d
                    .selected_good
                    .and_then(|index| d.goods.get(index))
                    .zip(requested_buy)
                    .and_then(|(good, count)| good.price.checked_mul(count as u64));
                buy.enabled = total.is_some_and(|price| price <= balance);
                if d.buy_prompt {
                    c.text.draw(
                        &format!("Total: {} {}", total.unwrap_or(0), d.currency),
                        10,
                        win.x + 10.0,
                        buy_row + 27.0,
                        if buy.enabled {
                            GOLD
                        } else {
                            [255, 80, 80, 255]
                        },
                    );
                }
                buy.label = Some(match requested_buy {
                    Some(count) if count > 1 => format!("Buy x{count}"),
                    _ => "Buy".into(),
                });
                if buy.update(c) || (buy_submitted && buy.enabled) {
                    if let (Some(g), Some(count)) =
                        (d.selected_good.and_then(|i| d.goods.get(i)), requested_buy)
                    {
                        out.push(ClientMessage::NpcBuy {
                            info: g.info,
                            count,
                        });
                        d.buy_prompt = false;
                        d.buy_count.focused = false;
                        d.buy_count.text.clear();
                    }
                }
                if !d.sell_types.is_empty() {
                    let sellable = |item: &ItemInstance| {
                        !item.locked
                            && !item.worthless
                            && bag.catalog.get(item.info).is_some_and(|def| {
                                def.can_sell && d.sell_types.contains(&def.item_type)
                            })
                    };
                    let selected: Vec<(u8, u32)> = if d.selected_sell.is_empty() {
                        bag.inventory
                            .iter()
                            .enumerate()
                            .filter_map(|(slot, item)| {
                                item.as_ref()
                                    .filter(|item| sellable(item))
                                    .and_then(|item| {
                                        sale_count(&sell_count, item.count)
                                            .map(|count| (slot as u8, count))
                                    })
                            })
                            .collect()
                    } else {
                        d.selected_sell
                            .iter()
                            .filter_map(|&(slot, id)| {
                                bag.inventory
                                    .get(slot as usize)
                                    .and_then(Option::as_ref)
                                    .filter(|item| item.id == id && sellable(item))
                                    .and_then(|item| {
                                        sale_count(&sell_count, item.count)
                                            .map(|count| (slot, count))
                                    })
                            })
                            .collect()
                    };
                    let sell = self.sell_button.get_or_insert_with(|| {
                        Button::default_style(0.0, 0.0, 110.0, "Sell selected")
                    });
                    sell.pos = (win.x + 125.0, win.y + win.h - 43.0);
                    sell.label = Some(
                        if !sell_count.is_empty() {
                            "Sell count"
                        } else if d.selected_sell.is_empty() {
                            "Sell all"
                        } else {
                            "Sell selected"
                        }
                        .into(),
                    );
                    sell.enabled = !selected.is_empty();
                    if sell.update(c) {
                        for items in selected.chunks(64) {
                            out.push(ClientMessage::NpcSell {
                                items: items.to_vec(),
                            });
                        }
                        d.selected_sell.clear();
                    }
                    self.inventory_open = true;
                }
                c.text.draw(
                    "Shift-click item: choose stack/charges",
                    9,
                    win.x + 10.0,
                    win.y + win.h - 15.0,
                    [190, 190, 190, 255],
                );
                if closed {
                    self.npc = None;
                    out.push(ClientMessage::NpcClose);
                }
            }
        }
        // Zircon NPCRepairDialog: links to bag/equipment items, then one
        // normal or special repair request for the selected cells.
        if let Some(d) = self.npc.as_mut().filter(|d| d.dialog_type == 2) {
            self.inventory_open = true;
            d.selected_repair.retain(|&(grid, slot, id)| {
                let cells = match grid {
                    Grid::Inventory => bag.inventory,
                    Grid::Equipment => bag.equipment,
                    Grid::Storage => bag.storage,
                };
                cells
                    .get(slot as usize)
                    .and_then(Option::as_ref)
                    .is_some_and(|item| item.id == id)
            });
            let win = self.layout.place(
                "repair",
                Rect::new(npc_origin.0, npc_origin.1 + npc_height, 430.0, 312.0),
                c.input,
                (width, height),
            );
            over |= win.contains(mouse.0, mouse.1);
            let closed = c.window(win, "Repair Items", true) && !self.layout.is_dragging();
            for i in 0..55 {
                let cell = Rect::new(
                    win.x + 12.0 + (i % 11) as f32 * PITCH,
                    win.y + 37.0 + (i / 11) as f32 * PITCH,
                    CELL,
                    CELL,
                );
                let item = d.selected_repair.get(i).and_then(|&(grid, slot, id)| {
                    let cells = match grid {
                        Grid::Inventory => bag.inventory,
                        Grid::Equipment => bag.equipment,
                        Grid::Storage => bag.storage,
                    };
                    cells
                        .get(slot as usize)
                        .and_then(Option::as_ref)
                        .filter(|item| item.id == id)
                });
                let hover = cell.contains(mouse.0, mouse.1);
                draw_item_cell(c, bag.catalog, cell, item, hover, false);
                if hover {
                    if let Some(item) = item {
                        self.tooltip = Some((item.info, mouse.0, mouse.1));
                        self.tooltip_item = Some(item.clone());
                    }
                    if c.input.lmb_pressed && i < d.selected_repair.len() {
                        d.selected_repair.remove(i);
                    }
                }
            }
            let cost: u64 = d
                .selected_repair
                .iter()
                .filter_map(|&(grid, slot, id)| {
                    let cells = match grid {
                        Grid::Inventory => bag.inventory,
                        Grid::Equipment => bag.equipment,
                        Grid::Storage => bag.storage,
                    };
                    let item = cells
                        .get(slot as usize)
                        .and_then(Option::as_ref)
                        .filter(|item| item.id == id)?;
                    let def = bag.catalog.get(item.info)?;
                    Some(repair_cost(item, def, d.special_repair))
                })
                .sum();
            c.text.draw(
                &format!("Repair cost: {cost} gold"),
                12,
                win.x + 12.0,
                win.y + 232.0,
                if cost <= bag.gold {
                    GOLD
                } else {
                    [255, 60, 60, 255]
                },
            );
            c.text.draw(
                "Click bag items; open Character (C) for worn gear.",
                10,
                win.x + 12.0,
                win.y + 255.0,
                [200, 200, 200, 255],
            );
            d.special_button.pos = (win.x + 12.0, win.y + 278.0);
            d.special_button.selected = d.special_repair;
            if d.special_button.update(c) {
                d.special_repair = !d.special_repair;
            }
            d.repair_button.pos = (win.x + 330.0, win.y + 278.0);
            d.repair_button.enabled = !d.selected_repair.is_empty() && cost <= bag.gold;
            if d.repair_button.update(c) {
                out.push(ClientMessage::NpcRepair {
                    items: std::mem::take(&mut d.selected_repair),
                    special: d.special_repair,
                });
            }
            if closed {
                self.npc = None;
                out.push(ClientMessage::NpcClose);
            }
        }
        // ---- Inventory window (right side) ----
        if self.inventory_open {
            let win = self.layout.place(
                "inventory",
                Rect::new(width as f32 - 263.0 - 10.0, 40.0, 263.0, 444.0),
                c.input,
                (width, height),
            );
            let unlocked = bag.inventory.len() > INVENTORY_SIZE;
            if !unlocked {
                self.inventory_page = 0;
            }
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            let closed = c.window(win, "Inventory", true) && !self.layout.is_dragging();
            for (page, label) in [(0, "Tab 1"), (1, "Tab 2")] {
                let tab = Rect::new(
                    win.x + 20.0 + page as f32 * 104.0,
                    win.y + 33.0,
                    100.0,
                    18.0,
                );
                let title = if page == 1 && !unlocked {
                    format!("Buy {} GG", self.tab_prices.0)
                } else {
                    label.into()
                };
                c.text.draw(
                    &title,
                    11,
                    tab.x,
                    tab.y,
                    if self.inventory_page == page {
                        [255, 221, 103, 255]
                    } else {
                        [200, 200, 200, 255]
                    },
                );
                if c.input.lmb_pressed && tab.contains(mouse.0, mouse.1) {
                    if page == 0 || unlocked {
                        self.inventory_page = page;
                    } else if self.tab_prices.0 > 0 {
                        out.push(ClientMessage::BuyTab { storage: false });
                    }
                }
            }
            for i in 0..INVENTORY_SIZE {
                let (col, row) = (i % 6, i / 6);
                let slot = self.inventory_page * INVENTORY_SIZE + i;
                let r = Rect::new(
                    win.x + 20.0 + col as f32 * PITCH,
                    win.y + 53.0 + row as f32 * PITCH,
                    CELL,
                    CELL,
                );
                let hover = r.contains(mouse.0, mouse.1);
                let item = bag.inventory.get(slot).and_then(|cell| cell.as_ref());
                let selected = self.carrying == Some((Grid::Inventory, slot as u8))
                    || item.is_some_and(|it| {
                        self.npc.as_ref().is_some_and(|d| {
                            d.selected_sell.contains(&(slot as u8, it.id))
                                || d.selected_repair
                                    .contains(&(Grid::Inventory, slot as u8, it.id))
                        })
                    });
                draw_item_cell(c, bag.catalog, r, item, hover, selected);
                if hover {
                    if let Some(it) = item {
                        self.tooltip = Some((it.info, mouse.0, mouse.1));
                        self.tooltip_item = Some(it.clone());
                        self.hover_inventory = Some(slot as u8);
                    }
                    if c.input.lmb_pressed {
                        if let (Some(it), Some(d)) =
                            (item, self.npc.as_mut().filter(|d| d.dialog_type == 2))
                        {
                            self.last_inventory_click = None;
                            if bag
                                .catalog
                                .get(it.info)
                                .is_some_and(|def| repairable(it, def, &d.sell_types))
                            {
                                d.toggle_repair(Grid::Inventory, slot as u8, it.id);
                            }
                        } else if let Some(d) = self
                            .npc
                            .as_mut()
                            .filter(|d| d.dialog_type == 1 && !d.sell_types.is_empty())
                        {
                            self.last_inventory_click = None;
                            if let Some(it) = item.filter(|it| {
                                !it.locked
                                    && !it.worthless
                                    && bag.catalog.get(it.info).is_some_and(|def| {
                                        def.can_sell && d.sell_types.contains(&def.item_type)
                                    })
                            }) {
                                d.toggle_sell(slot as u8, it.id);
                            }
                        } else {
                            self.click_inventory_slot(slot as u8, item, c.now, out);
                        }
                    }
                    if c.input.rmb_pressed {
                        if let Some(it) = item {
                            if c.input.ctrl {
                                out.push(ClientMessage::ItemLock {
                                    grid: Grid::Inventory,
                                    slot: slot as u8,
                                    locked: !it.locked,
                                });
                                continue;
                            }
                            if let Some(t) = bag.trade {
                                if !t
                                    .my_items
                                    .iter()
                                    .any(|(g, s, _)| *g == Grid::Inventory && *s == slot as u8)
                                {
                                    out.push(ClientMessage::TradeAddItem {
                                        grid: Grid::Inventory,
                                        slot: slot as u8,
                                        count: it.count,
                                    });
                                }
                                continue;
                            }
                            if self.npc.as_ref().is_some_and(|d| d.dialog_type == 14) {
                                if let Some(def) = bag.catalog.get(it.info) {
                                    let target = if (70..=75).contains(&def.effect) {
                                        Some((def.effect - 69) as usize)
                                    } else if def.effect == 60
                                        || (def.item_type == item_type::WEAPON && def.effect != 91)
                                    {
                                        Some(0)
                                    } else {
                                        None
                                    };
                                    if let Some(target) = target {
                                        self.craft_cells[target] = Some((slot as u8, it.id));
                                    }
                                }
                                continue;
                            }
                            if let Some(page) = self
                                .npc
                                .as_ref()
                                .map(|d| d.dialog_type)
                                .filter(|d| matches!(d, 11..=13 | 15))
                            {
                                let cell = (Grid::Inventory, slot as u8, it.id);
                                if bag.catalog.get(it.info).is_some_and(|d| {
                                    matches!(
                                        d.item_type,
                                        item_type::NECKLACE | item_type::BRACELET | item_type::RING
                                    )
                                }) {
                                    if self.accessory_target == Some(cell) {
                                        self.accessory_target = None;
                                        self.accessory_materials.clear();
                                    } else if let Some(target) = self.accessory_target {
                                        let target_item = match target.0 {
                                            Grid::Inventory => bag.inventory,
                                            Grid::Equipment => bag.equipment,
                                            Grid::Storage => bag.storage,
                                        }
                                        .get(target.1 as usize)
                                        .and_then(Option::as_ref);
                                        if matches!(page, 11 | 12 | 15)
                                            && target_item.is_some_and(|t| t.info == it.info)
                                        {
                                            if !self.accessory_materials.contains(&cell) {
                                                self.accessory_materials.push(cell)
                                            }
                                        } else {
                                            self.accessory_target = Some(cell);
                                            self.accessory_materials.clear();
                                        }
                                    } else {
                                        self.accessory_target = Some(cell);
                                    }
                                } else if (page == 11 || page == 15)
                                    && bag.catalog.get(it.info).is_some_and(|d| d.effect == 25)
                                {
                                    self.accessory_ore = Some(cell);
                                }
                                continue;
                            }
                            // Wedding-ring NPC page: a ring becomes the ring.
                            if self.npc.as_ref().is_some_and(|d| d.dialog_type == 6) {
                                out.push(ClientMessage::MarriageMakeRing { slot: slot as u8 });
                                continue;
                            }
                            if self.npc.as_ref().is_some_and(|d| d.dialog_type == 3) {
                                // Refine page: sort the item into the furnace lists.
                                let cell = (Grid::Inventory, slot as u8, it.count);
                                let def = bag.catalog.get(it.info);
                                let already = self
                                    .refine_ores
                                    .iter()
                                    .chain(&self.refine_items)
                                    .chain(&self.refine_specials)
                                    .any(|(g, s, _)| *g == Grid::Inventory && *s == slot as u8);
                                if !already {
                                    if def.is_some_and(|d| d.effect == 20) {
                                        if self.refine_ores.len() < 5 {
                                            self.refine_ores.push(cell);
                                        }
                                    } else if def.is_some_and(|d| matches!(d.item_type, 6..=8)) {
                                        if self.refine_items.len() < 3 {
                                            self.refine_items.push(cell);
                                        }
                                    } else if def.is_some_and(|d| d.item_type == 17)
                                        && self.refine_specials.is_empty()
                                    {
                                        self.refine_specials.push((Grid::Inventory, slot as u8, 1));
                                    }
                                }
                                continue;
                            }
                            if self.mail_open && self.mail_compose {
                                if self.mail_attach.len() < 5
                                    && !self
                                        .mail_attach
                                        .iter()
                                        .any(|(g, s, _)| *g == Grid::Inventory && *s == slot as u8)
                                {
                                    self.mail_attach
                                        .push((Grid::Inventory, slot as u8, it.count));
                                }
                                continue;
                            }
                            if self.storage_open && self.npc.is_none() {
                                if let Some(message) = self.store_bag_item(slot as u8, bag.storage)
                                {
                                    out.push(message);
                                }
                                continue;
                            }
                            if let Some(d) = self
                                .npc
                                .as_mut()
                                .filter(|d| d.dialog_type == 1 && !d.sell_types.is_empty())
                            {
                                if !it.locked
                                    && !it.worthless
                                    && bag.catalog.get(it.info).is_some_and(|def| {
                                        def.can_sell && d.sell_types.contains(&def.item_type)
                                    })
                                {
                                    d.toggle_sell(slot as u8, it.id);
                                }
                                continue;
                            }
                            if self.companion_open
                                && bag
                                    .catalog
                                    .get(it.info)
                                    .is_some_and(|d| d.item_type == item_type::COMPANION_BAG)
                            {
                                out.push(ClientMessage::CompanionBagEquip { slot: slot as u8 });
                                continue;
                            }
                            if self.companion_open
                                && bag
                                    .catalog
                                    .get(it.info)
                                    .is_some_and(|d| d.item_type == item_type::COMPANION_FOOD)
                            {
                                out.push(ClientMessage::CompanionFoodEquip { slot: slot as u8 });
                                continue;
                            }
                            if bag
                                .catalog
                                .get(it.info)
                                .is_some_and(|d| matches!(d.effect, 50..=53))
                            {
                                self.open_character_services = true;
                            } else {
                                out.push(ClientMessage::ItemUse { slot: slot as u8 });
                            }
                        }
                    }
                }
            }
            // Weight bar (GameInter 360) and gold.
            let bar_y = win.y + 369.0;
            if let Some(bar) = c.sprite(lib::GAME_INTER, 360) {
                let pct = (bag.weights.bag as f32 / bag.weights.max_bag.max(1) as f32).min(1.0);
                c.renderer
                    .draw_cropped(bar, win.x + 33.0, bar_y, pct, [1.0, 1.0, 1.0, 1.0]);
            }
            c.text.draw_centered(
                &format!("{}/{}", bag.weights.bag, bag.weights.max_bag),
                11,
                win.x + 131.0,
                bar_y - 2.0,
                [255, 255, 255, 255],
            );
            c.draw(lib::STORE_ITEMS, 121, win.x + 30.0, win.y + 397.0);
            c.text
                .draw("Gold", 12, win.x + 55.0, win.y + 394.0, [218, 165, 32, 255]);
            let gold = format!("{}", bag.gold);
            let gw = c.text.width(&gold, 12);
            c.text.draw(
                &gold,
                12,
                win.x + 240.0 - gw,
                win.y + 394.0,
                [255, 255, 255, 255],
            );
            let game_gold = bag
                .currencies
                .iter()
                .find(|currency| {
                    currency
                        .name
                        .replace(' ', "")
                        .eq_ignore_ascii_case("GameGold")
                        || currency.abbreviation.eq_ignore_ascii_case("GG")
                })
                .map(|currency| currency.amount)
                .unwrap_or(0);
            c.text.draw(
                &format!("Game Gold: {game_gold}"),
                10,
                win.x + 30.0,
                win.y + 414.0,
                [218, 165, 32, 255],
            );
            if closed {
                self.inventory_open = false;
                self.carrying = None;
            }
        }

        // ---- Character window (left side) ----
        if self.character_open {
            // Sets the player is wearing any piece of, in set order. The
            // window grows to fit them rather than overrunning its footer.
            let mut set_views: Vec<SetView> = Vec::new();
            {
                let mut seen: Vec<i32> = Vec::new();
                for it in bag.equipment.iter().flatten() {
                    let Some(set_index) = bag.catalog.get(it.info).map(|d| d.set) else {
                        continue;
                    };
                    if set_index == 0 || seen.contains(&set_index) {
                        continue;
                    }
                    seen.push(set_index);
                    if let Some(v) = set_view(bag, set_index) {
                        set_views.push(v);
                    }
                }
            }
            let set_rows: usize = set_views.iter().map(|v| 1 + v.lines.len()).sum();
            // The block sits at y+430, under the stat rows; the footer band
            // begins at h-45, so the window has to grow past both.
            let extra = if set_rows == 0 {
                0.0
            } else {
                set_rows as f32 * 15.0 + 36.0
            };
            let win = self.layout.place(
                "character",
                Rect::new(10.0, 40.0, 320.0, 440.0 + extra),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            let closed = c.window(win, "Character", true) && !self.layout.is_dragging();
            // Zircon's character view uses full-size ProgUse/Equip artwork.
            let anchor = (win.x + 130.0, win.y + 270.0);
            let body = u32::from(bag.gender == Gender::Female);
            c.draw_offset(
                lib::PROG_USE,
                body,
                anchor.0,
                anchor.1,
                [1.0; 4],
                Blend::Alpha,
            );
            for slot in [
                mir_proto::slot::ARMOUR,
                mir_proto::slot::WEAPON,
                mir_proto::slot::SHIELD,
                mir_proto::slot::HELMET,
            ] {
                let equipped = bag
                    .equipment
                    .get(slot)
                    .and_then(|s| s.as_ref())
                    .and_then(|item| bag.catalog.get(item.info));
                let cosmetic = match slot {
                    mir_proto::slot::ARMOUR => bag.wardrobe_item,
                    mir_proto::slot::HELMET => bag.wardrobe_helmet,
                    mir_proto::slot::WEAPON => bag.wardrobe_weapon,
                    _ => None,
                };
                let def = cosmetic.and_then(|item| bag.catalog.get(item)).or(equipped);
                if let Some(def) = def {
                    let image = def.image as u32;
                    if c.assets.info(lib::EQUIP, image).is_some() {
                        if slot == mir_proto::slot::WEAPON {
                            let effect = mir_proto::equipment_visual::weapon_effect(
                                def.index,
                                def.exterior_effect,
                            );
                            if let Some(colour) = crate::equipment_fx::weapon_glow_colour(effect) {
                                for (x, y) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
                                    c.draw_offset(
                                        lib::EQUIP,
                                        image,
                                        anchor.0 + x,
                                        anchor.1 + y,
                                        colour,
                                        Blend::Screen,
                                    );
                                }
                            }
                        }
                        c.draw_offset(
                            lib::EQUIP,
                            image,
                            anchor.0,
                            anchor.1,
                            [1.0; 4],
                            Blend::Alpha,
                        );
                    }
                    let frames = if slot == mir_proto::slot::ARMOUR {
                        crate::equipment_fx::armour_preview_frames(
                            def.index,
                            def.exterior_effect,
                            def.image,
                            bag.gender,
                            (c.now / 100) as u32,
                        )
                    } else {
                        [
                            crate::equipment_fx::preview_frame(
                                def.exterior_effect,
                                def.image,
                                bag.gender,
                                (c.now / 100) as u32,
                            ),
                            None,
                        ]
                    };
                    for effect in frames.into_iter().flatten() {
                        c.draw_offset(
                            lib::EQUIP_EFFECT_UI,
                            effect,
                            anchor.0,
                            anchor.1,
                            [1.0; 4],
                            Blend::Screen,
                        );
                    }
                }
            }
            if bag
                .equipment
                .get(mir_proto::slot::HELMET)
                .is_none_or(|s| s.is_none())
                && bag.wardrobe_helmet.is_none()
                && bag.hair > 0
            {
                let base = match (bag.stats.class, bag.gender) {
                    (3, Gender::Male) => 1100,
                    (3, Gender::Female) => 1120,
                    (_, Gender::Male) => 60,
                    (_, Gender::Female) => 80,
                };
                c.draw_offset(
                    lib::PROG_USE,
                    base + u32::from(bag.hair - 1),
                    anchor.0,
                    anchor.1,
                    [1.0; 4],
                    Blend::Alpha,
                );
            }
            if let Some(boots) = bag.wardrobe_shoes.and_then(|id| bag.catalog.get(id)) {
                c.text.draw(
                    &format!(
                        "Boot look: {}",
                        boots.name.chars().take(25).collect::<String>()
                    ),
                    10,
                    win.x + 74.0,
                    win.y + 302.0,
                    GOLD,
                );
            }
            for (slot, rr, glyph) in equipment_rects() {
                let r = Rect::new(win.x + rr.x, win.y + rr.y, rr.w, rr.h);
                let hover = r.contains(mouse.0, mouse.1);
                let item = bag.equipment.get(slot).and_then(|s| s.as_ref());
                let selected = self.carrying == Some((Grid::Equipment, slot as u8))
                    || item.is_some_and(|it| {
                        self.npc.as_ref().is_some_and(|d| {
                            d.selected_repair
                                .contains(&(Grid::Equipment, slot as u8, it.id))
                        })
                    });
                let worn_art = slot != mir_proto::slot::SHOES
                    && item
                        .and_then(|item| bag.catalog.get(item.info))
                        .is_some_and(|def| c.assets.info(lib::EQUIP, def.image as u32).is_some());
                if (item.is_none() || worn_art)
                    && matches!(
                        slot,
                        mir_proto::slot::ARMOUR
                            | mir_proto::slot::WEAPON
                            | mir_proto::slot::HELMET
                            | mir_proto::slot::SHIELD
                            | mir_proto::slot::SHOES
                    )
                {
                    c.border(
                        r,
                        if hover || selected {
                            [0, 255, 0, 255]
                        } else {
                            [99, 83, 50, 255]
                        },
                    );
                } else {
                    draw_item_cell(c, bag.catalog, r, item, hover, selected);
                }
                if slot == mir_proto::slot::AMULET {
                    if let Some(frame) = item
                        .and_then(|it| bag.catalog.get(it.info))
                        .and_then(|def| dark_stone_overlay_frame(def.item_type, def.shape, c.now))
                    {
                        c.draw_offset(
                            lib::GAME_INTER,
                            frame,
                            r.x - 5.0,
                            r.y + 20.0,
                            [1.0, 1.0, 1.0, 0.8],
                            Blend::Screen,
                        );
                    }
                }
                if item.is_none() {
                    if let Some(g) = glyph {
                        if let Some(info) = c.assets.info(lib::INTERFACE, g) {
                            c.draw_tinted(
                                lib::INTERFACE,
                                g,
                                r.x + (r.w - info.width as f32) / 2.0,
                                r.y + (r.h - info.height as f32) / 2.0,
                                [1.0, 1.0, 1.0, 0.25],
                                Blend::Alpha,
                            );
                        }
                    }
                }
                if hover {
                    let cosmetic = match slot {
                        mir_proto::slot::ARMOUR => bag.wardrobe_item,
                        mir_proto::slot::HELMET => bag.wardrobe_helmet,
                        mir_proto::slot::WEAPON => bag.wardrobe_weapon,
                        mir_proto::slot::SHOES => bag.wardrobe_shoes,
                        _ => None,
                    };
                    let slot_name = cosmetic
                        .and_then(|id| bag.catalog.get(id))
                        .map(|def| {
                            format!("{} slot (look: {})", equipment_slot_name(slot), def.name)
                        })
                        .unwrap_or_else(|| format!("{} slot", equipment_slot_name(slot)));
                    self.tooltip_slot = Some((slot_name, mouse.0, mouse.1));
                    if let Some(it) = item {
                        self.tooltip = Some((it.info, mouse.0, mouse.1));
                        self.tooltip_item = Some(it.clone());
                    }
                    if c.input.lmb_pressed {
                        if let (Some(it), Some(d)) =
                            (item, self.npc.as_mut().filter(|d| d.dialog_type == 2))
                        {
                            if bag
                                .catalog
                                .get(it.info)
                                .is_some_and(|def| repairable(it, def, &d.sell_types))
                            {
                                d.toggle_repair(Grid::Equipment, slot as u8, it.id);
                            }
                        } else {
                            self.click_slot(Grid::Equipment, slot as u8, item.is_some(), out);
                        }
                    }
                    if c.input.rmb_pressed && item.is_some() {
                        if let Some(it) = item.filter(|it| {
                            bag.catalog.get(it.info).is_some_and(|d| {
                                matches!(
                                    d.item_type,
                                    item_type::NECKLACE | item_type::BRACELET | item_type::RING
                                )
                            })
                        }) {
                            if self
                                .npc
                                .as_ref()
                                .is_some_and(|d| matches!(d.dialog_type, 11..=13 | 15))
                            {
                                self.accessory_target = Some((Grid::Equipment, slot as u8, it.id));
                                self.accessory_materials.clear();
                                continue;
                            }
                        }
                        // Unequip into the first free bag slot.
                        if let Some(free) = bag.inventory.iter().position(|s| s.is_none()) {
                            out.push(ClientMessage::ItemMove {
                                from: Grid::Equipment,
                                from_slot: slot as u8,
                                to: Grid::Inventory,
                                to_slot: free as u8,
                            });
                        }
                    }
                }
            }
            let s = bag.stats;
            let rows = [
                ("Level", s.level.to_string()),
                ("HP", format!("{}/{}", s.hp, s.max_hp)),
                ("MP", format!("{}/{}", s.mp, s.max_mp)),
                ("DC", format!("{}-{}", s.min_dc, s.max_dc)),
                ("AC", format!("{}-{}", s.min_ac, s.max_ac)),
                ("MC", format!("{}-{}", s.min_mc, s.max_mc)),
                ("SC", format!("{}-{}", s.min_sc, s.max_sc)),
                ("Accuracy", s.accuracy.to_string()),
                ("Agility", s.agility.to_string()),
                (
                    "Wear",
                    format!("{}/{}", bag.weights.wear, bag.weights.max_wear),
                ),
                (
                    "Hand",
                    format!("{}/{}", bag.weights.hand, bag.weights.max_hand),
                ),
                (
                    "Fame",
                    if s.fame_title.is_empty() {
                        "-".to_string()
                    } else {
                        s.fame_title.clone()
                    },
                ),
            ];
            for (i, (k, v)) in rows.iter().enumerate() {
                let (col, row) = (i % 2, i / 2);
                let x = win.x + 20.0 + col as f32 * 150.0;
                let y = win.y + 319.0 + row as f32 * 18.0;
                c.text.draw(k, 12, x, y, GOLD);
                c.text.draw(v, 12, x + 60.0, y, [255, 255, 255, 255]);
            }
            let mut sy = win.y + 430.0;
            for v in &set_views {
                let done = v.needed > 0 && v.worn == v.needed;
                c.text.draw(
                    &set_heading(&v.name, v.worn, v.needed),
                    11,
                    win.x + 20.0,
                    sy,
                    if done {
                        [120, 255, 120, 255]
                    } else {
                        [200, 200, 200, 255]
                    },
                );
                sy += 15.0;
                for (label, active) in &v.lines {
                    c.text.draw(
                        label,
                        11,
                        win.x + 32.0,
                        sy,
                        if *active {
                            [120, 255, 120, 255]
                        } else {
                            [140, 140, 140, 255]
                        },
                    );
                    sy += 15.0;
                }
            }
            c.text.draw(
                "Right-click: unequip",
                10,
                win.x + 20.0,
                win.y + win.h - 20.0,
                [160, 160, 160, 255],
            );
            if let Some(partner) = bag.partner {
                let ring_on = bag.wedding_ring.is_some_and(|r| {
                    bag.equipment
                        .get(7)
                        .and_then(|c| c.as_ref())
                        .is_some_and(|it| it.id == r)
                });
                c.text.draw(
                    &format!(
                        "Married to {partner}{}",
                        if ring_on { " (wedding ring on)" } else { "" }
                    ),
                    11,
                    win.x + 160.0,
                    win.y + 420.0,
                    [255, 150, 200, 255],
                );
                if self.marriage_buttons.is_empty() {
                    self.marriage_buttons = vec![
                        Button::default_style(0.0, 0.0, 80.0, "Accept"),
                        Button::default_style(0.0, 0.0, 80.0, "Decline"),
                        Button::default_style(0.0, 0.0, 120.0, "To partner"),
                    ];
                }
                let b = &mut self.marriage_buttons[2];
                b.pos = (win.x + 180.0, win.y + win.h - 3.0 - 42.0 + 8.0);
                b.enabled = ring_on;
                if b.update(c) {
                    out.push(ClientMessage::MarriageTeleport);
                }
            }
            let wardrobe_button = Rect::new(win.x + 208.0, win.y + 46.0, 94.0, 25.0);
            c.panel(wardrobe_button, [35, 30, 20, 240]);
            c.border(wardrobe_button, GOLD);
            c.text.draw_centered(
                "Wardrobe (Y)",
                11,
                wardrobe_button.x + wardrobe_button.w / 2.0,
                wardrobe_button.y + 5.0,
                GOLD,
            );
            if wardrobe_button.contains(mouse.0, mouse.1) && c.input.lmb_pressed {
                self.toggle_wardrobe = true;
            }
            if closed {
                self.character_open = false;
                self.carrying = None;
            }
        }

        // ---- Skill window (Zircon MagicDialog 419x511, standard chrome here) ----
        self.hover_magic = None;
        if self.skills_open {
            let win = self.layout.place(
                "skills",
                Rect::new((width as f32 - 419.0) / 2.0, 30.0, 419.0, 511.0),
                c.input,
                (width, height),
            );
            if win.contains(mouse.0, mouse.1) {
                over = true;
                self.skill_scroll = (self.skill_scroll - c.input.wheel * 59.0).max(0.0);
            }
            let closed = c.window(win, "Skills", false) && !self.layout.is_dragging();
            let class = bag.stats.class;
            let list = bag.catalog.class_magics(class);
            let max_scroll = ((list.len() as f32) * 59.0 - 460.0).max(0.0);
            self.skill_scroll = self.skill_scroll.min(max_scroll);
            let top = win.y + 40.0;
            let first = (self.skill_scroll / 59.0) as usize;
            for (i, def) in list.iter().enumerate().skip(first).take(8) {
                let y = top + (i as f32 * 59.0 - self.skill_scroll);
                if y + 54.0 > win.y + win.h - 6.0 {
                    break;
                }
                let cell = Rect::new(win.x + 25.0, y, 369.0, 54.0);
                c.panel(cell, [16, 8, 8, 255]);
                let known = bag.magics.iter().find(|m| m.magic == def.magic);
                let usable = bag.stats.level >= def.need_level[0];
                if known.is_some() {
                    let border = match def.school {
                        1 => 860,
                        2 => 861,
                        3 => 862,
                        4 => 870,
                        5 => 871,
                        6 => 872,
                        7 => 873,
                        10 => 874,
                        8 => 880,
                        9 => 881,
                        11 => 883,
                        12 => 890,
                        13 => 891,
                        14 => 892,
                        _ => 0,
                    };
                    if border > 0 {
                        c.draw(lib::GAME_INTER2, border, cell.x + 4.0, cell.y + 4.0);
                    }
                }
                let icon = Rect::new(cell.x + 9.0, cell.y + 9.0, 36.0, 36.0);
                let alpha = if known.is_some() {
                    1.0
                } else if usable {
                    0.6
                } else {
                    0.3
                };
                c.draw_tinted(
                    lib::MAGIC_ICON,
                    def.icon as u32,
                    icon.x,
                    icon.y,
                    [1.0, 1.0, 1.0, alpha],
                    Blend::Alpha,
                );
                if let Some(m) = known {
                    if m.key > 0 {
                        c.text.draw(
                            &skill_key_label(m.key),
                            11,
                            icon.x + 18.0,
                            icon.y + 22.0,
                            [127, 255, 212, 255],
                        );
                    }
                    if bag.toggles.contains(&def.magic) {
                        c.border(icon, [0, 255, 0, 255]);
                    }
                }
                c.text.draw(
                    &def.name,
                    13,
                    cell.x + 55.0,
                    cell.y + 1.0,
                    [255, 255, 255, 255],
                );
                match known {
                    Some(m) => {
                        c.text.draw(
                            &format!("Level: {}", m.level),
                            12,
                            cell.x + 57.0,
                            cell.y + 30.0,
                            GOLD,
                        );
                        let max = def.experience.get(m.level as usize).copied().unwrap_or(0);
                        let (label, pct) = if m.level >= 3 {
                            ("Experience: Max".to_string(), 1.0)
                        } else if max > 0 {
                            (
                                format!("Experience: {}/{}", m.experience, max),
                                (m.experience as f32 / max as f32).min(1.0),
                            )
                        } else {
                            (String::new(), 0.0)
                        };
                        let lw = c.text.width(&label, 11);
                        c.text.draw(
                            &label,
                            11,
                            cell.x + cell.w - lw - 6.0,
                            cell.y + 17.0,
                            [200, 200, 200, 255],
                        );
                        if let Some(bar) = c.sprite(lib::GAME_INTER2, 812) {
                            c.renderer.draw_cropped(
                                bar,
                                cell.x + 110.0,
                                cell.y + 36.0,
                                pct,
                                [1.0, 1.0, 1.0, 1.0],
                            );
                        }
                        let need = def.need_level.get(m.level as usize).copied().unwrap_or(0);
                        if m.level < 3 && bag.stats.level < need {
                            c.text.draw(
                                &format!("Required Level: {need}"),
                                11,
                                cell.x + 57.0,
                                cell.y + 17.0,
                                [255, 80, 80, 255],
                            );
                        }
                    }
                    None => {
                        c.text.draw(
                            "Not Learned",
                            12,
                            cell.x + 57.0,
                            cell.y + 17.0,
                            [255, 80, 80, 255],
                        );
                        let col = if usable {
                            [120, 255, 120, 255]
                        } else {
                            [255, 80, 80, 255]
                        };
                        c.text.draw(
                            &format!("Required Level: {}", def.need_level[0]),
                            11,
                            cell.x + 57.0,
                            cell.y + 33.0,
                            col,
                        );
                    }
                }
                if icon.contains(mouse.0, mouse.1) {
                    self.hover_magic = Some(def.magic);
                    self.magic_tip = Some((def.magic, mouse.0, mouse.1));
                    if known.is_some() && c.input.lmb_pressed {
                        out.push(ClientMessage::MagicKey {
                            magic: def.magic,
                            key: 0,
                        });
                    }
                }
            }
            c.text.draw(
                "Hover skill: F1-F12 (Mac: Cmd/Option+1..0, -, =); click to unbind",
                11,
                win.x + 25.0,
                win.y + win.h - 22.0,
                [160, 160, 160, 255],
            );
            if closed {
                self.skills_open = false;
            }
        }
        if let Some((magic, x, y)) = self.magic_tip.take() {
            if let Some(def) = bag.catalog.magic(magic) {
                let known = bag.magics.iter().find(|m| m.magic == magic);
                let mut lines: Vec<(String, [u8; 4])> =
                    vec![(def.name.clone(), [255, 255, 0, 255])];
                match known {
                    Some(m) => lines.push((
                        format!("Current Level: {}  Cost: {} MP", m.level, def.cost(m.level)),
                        [120, 255, 120, 255],
                    )),
                    None => lines.push(("Not learned".into(), [255, 80, 80, 255])),
                }
                for (i, (need, exp)) in def.need_level.iter().zip(def.experience.iter()).enumerate()
                {
                    let col = if bag.stats.level < *need {
                        [255, 80, 80, 255]
                    } else {
                        [200, 200, 200, 255]
                    };
                    lines.push((
                        format!("Rank {}: Level {need}, Experience {exp}", i + 1),
                        col,
                    ));
                }
                if !def.description.is_empty() {
                    lines.push((def.description.clone(), [245, 222, 179, 255]));
                }
                let w = lines
                    .iter()
                    .map(|(l, _)| c.text.width(l, 12))
                    .fold(0.0, f32::max)
                    + 16.0;
                let h = lines.len() as f32 * 16.0 + 8.0;
                let r = Rect::new((x + 16.0).min(width as f32 - w - 4.0), y + 8.0, w, h);
                c.fill(r, [0.0, 24.0 / 255.0, 48.0 / 255.0, 0.8]);
                c.border(r, [255, 255, 0, 255]);
                for (i, (l, col)) in lines.iter().enumerate() {
                    c.text
                        .draw(l, 12, r.x + 8.0, r.y + 4.0 + i as f32 * 16.0, *col);
                }
            }
        }

        // ---- Spell bar (bottom-left, above the HUD) ----
        {
            let base_y = height as f32 - 96.0 - 44.0;
            for key in 1..=12u8 {
                let r = Rect::new(10.0 + (key as f32 - 1.0) * 37.0, base_y, 36.0, 36.0);
                c.fill(r, [20.0 / 255.0, 20.0 / 255.0, 20.0 / 255.0, 0.6]);
                c.border(r, GOLD);
                if let Some(m) = bag.magics.iter().find(|m| m.key == key) {
                    if let Some(def) = bag.catalog.magic(m.magic) {
                        c.draw_tinted(
                            lib::MAGIC_ICON,
                            def.icon as u32,
                            r.x,
                            r.y,
                            [1.0, 1.0, 1.0, 0.8],
                            Blend::Alpha,
                        );
                        if bag.toggles.contains(&m.magic) {
                            c.border(r, [0, 255, 0, 255]);
                        }
                        if let Some(until) = bag.cooldowns.get(&m.magic) {
                            if *until > c.now {
                                c.fill(r, [50.0 / 255.0, 50.0 / 255.0, 50.0 / 255.0, 0.5]);
                                let secs = (until - c.now).div_ceil(1000);
                                c.text.draw_centered(
                                    &secs.to_string(),
                                    13,
                                    r.x + 18.0,
                                    r.y + 10.0,
                                    GOLD,
                                );
                            }
                        }
                        if r.contains(mouse.0, mouse.1) {
                            self.magic_tip = Some((m.magic, mouse.0, mouse.1));
                        }
                    }
                }
                c.text.draw(
                    &skill_key_label(key),
                    9,
                    r.x + 2.0,
                    r.y + 24.0,
                    [255, 255, 255, 200],
                );
            }
        }

        // ---- Belt (bottom-right, above the HUD): 10 cells keyed 1..9, 0 ----
        if self.belt_open {
            let cells = MAX_BELT as f32;
            let inner = Rect::new(
                width as f32 - 10.0 - (cells * (CELL - 1.0) + 1.0),
                height as f32 - 96.0 - 44.0,
                cells * (CELL - 1.0) + 1.0,
                CELL,
            );
            let win = Rect::new(inner.x - 6.0, inner.y - 6.0, inner.w + 12.0, inner.h + 12.0);
            if win.contains(mouse.0, mouse.1) {
                over = true;
            }
            c.panel(win, [20, 20, 20, 200]);
            for (i, link) in bag.belt.iter().enumerate().take(MAX_BELT) {
                let r = Rect::new(inner.x + i as f32 * (CELL - 1.0), inner.y, CELL, CELL);
                let hover = r.contains(mouse.0, mouse.1);
                let item = belt_cell_item(bag.inventory, link);
                draw_item_cell(c, bag.catalog, r, item.as_ref(), hover, false);
                if item.is_some() && bag.use_item_time > c.now {
                    c.fill(r, [120.0 / 255.0, 120.0 / 255.0, 120.0 / 255.0, 0.6]);
                    let secs = (bag.use_item_time - c.now).div_ceil(1000);
                    c.text
                        .draw_centered(&secs.to_string(), 13, r.x + 18.0, r.y + 10.0, GOLD);
                }
                c.text.draw(
                    &((i + 1) % 10).to_string(),
                    9,
                    r.x + 2.0,
                    r.y + 1.0,
                    [255, 255, 255, 220],
                );
                if !hover {
                    continue;
                }
                if let Some(it) = &item {
                    self.tooltip = Some((it.info, mouse.0, mouse.1));
                    self.tooltip_item = Some(it.clone());
                }
                if c.input.lmb_pressed {
                    match self.carrying.take() {
                        Some((Grid::Inventory, s)) => {
                            let l = link_for(bag.catalog, bag.inventory, s, i as u8);
                            out.push(ClientMessage::BeltLink {
                                slot: l.slot,
                                info: l.info,
                                item: l.item,
                            });
                        }
                        Some(other) => self.carrying = Some(other),
                        None => {
                            if let Some(slot) = belt_inventory_slot(bag.inventory, link) {
                                out.push(ClientMessage::ItemUse { slot });
                            }
                        }
                    }
                }
                if c.input.rmb_pressed {
                    out.push(ClientMessage::BeltLink {
                        slot: i as u8,
                        info: None,
                        item: None,
                    });
                }
            }
        }

        // ---- Death: revive button in the middle of the screen ----
        if bag.dead {
            let msg = "You have died.";
            let w = c.text.width(msg, 16);
            let cx = width as f32 / 2.0;
            let cy = height as f32 / 2.0;
            c.text
                .draw(msg, 16, cx - w / 2.0, cy - 60.0, [255, 80, 80, 255]);
            let b = self
                .revive_button
                .get_or_insert_with(|| Button::default_style(0.0, 0.0, 110.0, "Revive"));
            b.pos = (cx - 55.0, cy - 30.0);
            b.enabled = true;
            if b.update(c) {
                out.push(ClientMessage::TownRevive);
            }
        }

        over |= self.market.draw(
            c,
            bag.catalog,
            bag.inventory,
            bag.gold,
            bag.currencies,
            width,
            height,
            out,
        );

        // HUD buttons and the dialogs behind them, on top of everything.
        let mut actions: Vec<MenuAction> = Vec::new();
        over |= self.menu.draw(c, bag, width, height, &mut actions);
        for a in actions {
            match a {
                MenuAction::Toggle(w) => match w {
                    menu::Window::Character => self.character_open = !self.character_open,
                    menu::Window::Inventory => self.inventory_open = !self.inventory_open,
                    menu::Window::Skills => self.skills_open = !self.skills_open,
                    menu::Window::Quests => self.quests_open = !self.quests_open,
                    menu::Window::Mail => self.mail_open = !self.mail_open,
                    menu::Window::Belt => self.belt_open = !self.belt_open,
                    menu::Window::Group => self.group_open = !self.group_open,
                    menu::Window::Guild => self.guild_open = !self.guild_open,
                    menu::Window::Storage => self.storage_open = !self.storage_open,
                    menu::Window::Companion => self.companion_open = !self.companion_open,
                    menu::Window::Ranking => self.ranking.open = !self.ranking.open,
                    // The friends window lives on Game, so pulse a flag it drains.
                    menu::Window::Friends => self.toggle_friends = true,
                    menu::Window::Market => {
                        self.market.open = !self.market.open;
                        if self.market.open {
                            self.market.refresh();
                        }
                    }
                    menu::Window::Map => self.toggle_map = true,
                    // These two live on `Game`; relay the click to it.
                    menu::Window::Fortune => self.toggle_fortune = true,
                    menu::Window::ChatOptions => self.toggle_chat_options = true,
                    menu::Window::Settings => self.settings.open = !self.settings.open,
                },
                MenuAction::Logout => out.push(ClientMessage::Logout),
                MenuAction::Quit => std::process::exit(0),
            }
        }
        over |= self.settings.draw(c, width, height);
        // The board is its own window, drawn after the rest so it lands on
        // top when several are open.
        over |= self.ranking.draw(
            c,
            bag.ranking,
            bag.catalog,
            bag.player_name,
            width,
            height,
            out,
        );

        if let Some((info, x, y)) = self.tooltip {
            // Its own layer, above every window: a tooltip always wins.
            c.layer();
            let price = self
                .npc
                .as_ref()
                .and_then(|d| d.goods.iter().find(|g| g.info == info))
                .map(|g| g.price);
            if let Some(def) = bag.catalog.get(info) {
                let set = if def.set != 0 {
                    set_view(bag, def.set)
                } else {
                    None
                };
                tooltip(
                    c,
                    def,
                    self.tooltip_item.as_ref().filter(|i| i.info == info),
                    self.tooltip_slot.as_ref().map(|(name, _, _)| name.as_str()),
                    (x, y),
                    (width as f32, height as f32),
                    price,
                    set.as_ref(),
                );
            }
        }
        if let Some((name, x, y)) = self
            .tooltip_slot
            .as_ref()
            .filter(|_| self.tooltip.is_none())
        {
            c.layer();
            let label = name.to_string();
            let w = c.text.width(&label, 12) + 16.0;
            let x = (*x + 16.0).min((width as f32 - w - 4.0).max(4.0));
            let y = (*y + 8.0).min((height as f32 - 24.0 - 4.0).max(4.0));
            let r = Rect::new(x, y, w, 24.0);
            c.fill(r, [0.0, 0.0, 0.0, 0.85]);
            c.border(r, GOLD);
            c.text.draw(&label, 12, x + 8.0, y + 4.0, [255; 4]);
        }
        let _ = EQUIPMENT_SIZE;
        let _ = item_type::NOTHING;
        self.close_all_hint = false;
        self.layout.end_frame(c.input);
        over
    }

    fn click_slot(&mut self, grid: Grid, slot: u8, has_item: bool, out: &mut Vec<ClientMessage>) {
        match self.carrying {
            None => {
                if has_item {
                    self.carrying = Some((grid, slot));
                }
            }
            Some((from, from_slot)) => {
                if (from, from_slot) != (grid, slot) {
                    out.push(ClientMessage::ItemMove {
                        from,
                        from_slot,
                        to: grid,
                        to_slot: slot,
                    });
                }
                self.carrying = None;
            }
        }
    }

    fn click_inventory_slot(
        &mut self,
        slot: u8,
        item: Option<&ItemInstance>,
        now: u64,
        out: &mut Vec<ClientMessage>,
    ) {
        if let Some(item) = item {
            if self.carrying == Some((Grid::Inventory, slot))
                && self
                    .last_inventory_click
                    .is_some_and(|(last_slot, id, time)| {
                        last_slot == slot && id == item.id && now.saturating_sub(time) <= 500
                    })
            {
                self.carrying = None;
                self.last_inventory_click = None;
                out.push(ClientMessage::ItemUse { slot });
                return;
            }
            self.last_inventory_click = Some((slot, item.id, now));
        } else {
            self.last_inventory_click = None;
        }
        self.click_slot(Grid::Inventory, slot, item.is_some(), out);
    }

    fn store_bag_item(&self, slot: u8, storage: &[Option<ItemInstance>]) -> Option<ClientMessage> {
        if self.carrying.is_some() {
            return None;
        }
        let to_slot = u8::try_from(storage.iter().position(Option::is_none)?).ok()?;
        Some(ClientMessage::ItemMove {
            from: Grid::Inventory,
            from_slot: slot,
            to: Grid::Storage,
            to_slot,
        })
    }

    fn retrieve_storage_item(
        &self,
        slot: u8,
        inventory: &[Option<ItemInstance>],
    ) -> Option<ClientMessage> {
        if self.carrying.is_some() {
            return None;
        }
        let to_slot = u8::try_from(inventory.iter().position(Option::is_none)?).ok()?;
        Some(ClientMessage::ItemMove {
            from: Grid::Storage,
            from_slot: slot,
            to: Grid::Inventory,
            to_slot,
        })
    }

    pub(crate) fn drop_carried_item(
        &mut self,
        inventory: &[Option<ItemInstance>],
        clicked_world: bool,
    ) -> Option<ClientMessage> {
        if !clicked_world {
            return None;
        }
        let Some((Grid::Inventory, slot)) = self.carrying.take() else {
            return None;
        };
        let count = inventory.get(slot as usize)?.as_ref()?.count;
        Some(ClientMessage::ItemDrop { slot, count })
    }
}

/// Word-wrap dialog parts into lines of (word, button id).
type Line = Vec<(String, Option<i32>)>;

fn layout_parts(c: &mut Ctx, parts: &[DialogPart], max_w: f32) -> (Vec<Line>, f32) {
    let mut lines: Vec<Line> = vec![Vec::new()];
    let mut x = 0.0;
    let push = |c: &mut Ctx, lines: &mut Vec<Line>, x: &mut f32, word: String, id: Option<i32>| {
        let w = c.text.width(&word, 13);
        if *x + w > max_w && *x > 0.0 {
            lines.push(Vec::new());
            *x = 0.0;
        }
        *x += w;
        lines.last_mut().unwrap().push((word, id));
    };
    for p in parts {
        match p {
            DialogPart::NewLine => {
                lines.push(Vec::new());
                x = 0.0;
            }
            DialogPart::Text(t) => {
                for word in t.split_inclusive(' ') {
                    push(c, &mut lines, &mut x, word.to_string(), None);
                }
            }
            DialogPart::Button { label, id } => {
                push(c, &mut lines, &mut x, label.clone(), Some(*id));
            }
        }
    }
    (lines, max_w)
}

/// Greedy word wrap at `max_w` pixels for `size` text.
pub(crate) fn wrap_text(c: &mut Ctx, text: &str, max_w: f32, size: u32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if c.text.width(&candidate, size) > max_w && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod item_drop_tests {
    use super::*;

    #[test]
    fn dark_stone_character_overlay_uses_original_frames() {
        assert_eq!(dark_stone_overlay_frame(16, 1, 0), Some(2020));
        assert_eq!(dark_stone_overlay_frame(16, 4, 900), Some(2059));
        assert_eq!(dark_stone_overlay_frame(16, 5, 0), None);
        assert_eq!(dark_stone_overlay_frame(2, 1, 0), None);
        let Some(root) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let library = mir_formats::zl::ZlLibrary::open(
            std::path::PathBuf::from(root).join("Data/GameInter.Zl"),
        )
        .unwrap();
        for shape in 1..=4 {
            for frame in 0..10 {
                let index = dark_stone_overlay_frame(16, shape, frame * 100).unwrap();
                assert!(library.info(index as usize).is_some(), "missing {index}");
            }
        }
    }

    #[test]
    fn companion_reroll_button_uses_one_matching_elixir_after_arming() {
        let mut out = Vec::new();
        companion_reroll_action(3, false, 7, &mut out);
        assert_eq!(
            out,
            [
                ClientMessage::CompanionSkillToggle { level: 3 },
                ClientMessage::ItemUse { slot: 7 }
            ]
        );
        out.clear();
        companion_reroll_action(5, true, 8, &mut out);
        assert_eq!(out, [ClientMessage::ItemUse { slot: 8 }]);
        assert_eq!(
            companion_elixir_inventory(&[Some(item())], &ItemCatalog::empty(), 3),
            (None, 0)
        );
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let catalog =
            ItemCatalog::load(std::path::PathBuf::from(assets).join("../Database/System.db"))
                .unwrap();
        // Build inventory using the catalogue's actual level-specific elixirs.
        let db = mir_formats::mirdb::MirDb::load(
            std::path::PathBuf::from(std::env::var_os("ZIRCON_ASSETS").unwrap())
                .join("../Database/System.db"),
        )
        .unwrap();
        let collection = db.collection("ItemInfo").unwrap();
        let elixir = collection
            .records
            .iter()
            .find(|row| {
                collection.int_or(row, "Shape", 0) == 16
                    && collection.int_or(row, "RequiredAmount", 0) == 3
            })
            .unwrap();
        let mut elixir_item = item();
        elixir_item.info = collection.index(elixir);
        elixir_item.count = 2;
        assert_eq!(
            companion_elixir_inventory(&[Some(elixir_item.clone())], &catalog, 3),
            (Some(0), 2)
        );
        assert_eq!(
            companion_elixir_inventory(&[Some(elixir_item)], &catalog, 5),
            (None, 0)
        );
    }

    #[test]
    fn durability_tooltip_uses_original_item_units() {
        assert_eq!(
            durability_label(item_type::WEAPON, 4_500, 5_000).as_deref(),
            Some("Durability 4/5")
        );
        assert_eq!(
            durability_label(item_type::WEAPON, 5_500, 6_000).as_deref(),
            Some("Durability 6/6")
        );
        assert_eq!(
            durability_label(item_type::MEAT, 2_750, 5_000).as_deref(),
            Some("Quality 3/5")
        );
        assert_eq!(
            durability_label(item_type::ORE, 3_500, 5_000).as_deref(),
            Some("Purity 4")
        );
        assert_eq!(
            durability_label(item_type::BOOK, 72, 100).as_deref(),
            Some("Pages 72/100")
        );
        assert_eq!(durability_label(item_type::CONSUMABLE, 2_000, 2_000), None);
    }

    fn item() -> ItemInstance {
        ItemInstance {
            id: 1,
            info: 2,
            count: 7,
            durability: 0,
            max_durability: 0,
            added: Vec::new(),
            enhancements: Vec::new(),
            refinable: false,
            level: 0,
            accessory_experience: 0,
            non_refinable: false,
            bound: false,
            worthless: false,
            locked: false,
            expires_at: None,
            colour: 0,
        }
    }

    #[test]
    fn double_click_inventory_item_uses_it_once() {
        let mut windows = WindowState::default();
        let item = item();
        let mut out = Vec::new();
        windows.click_inventory_slot(7, Some(&item), 1_000, &mut out);
        assert!(out.is_empty());
        assert_eq!(windows.carrying, Some((Grid::Inventory, 7)));
        windows.click_inventory_slot(7, Some(&item), 1_250, &mut out);
        assert_eq!(out, [ClientMessage::ItemUse { slot: 7 }]);
        assert_eq!(windows.carrying, None);

        out.clear();
        windows.click_inventory_slot(7, Some(&item), 2_000, &mut out);
        windows.click_inventory_slot(7, Some(&item), 2_600, &mut out);
        assert!(out.is_empty(), "slow clicks only toggle selection");
    }

    #[test]
    fn clicking_world_with_selected_bag_item_drops_its_stack() {
        let mut windows = WindowState {
            carrying: Some((Grid::Inventory, 0)),
            ..WindowState::default()
        };
        let inventory = [Some(item())];
        assert_eq!(windows.drop_carried_item(&inventory, false), None);
        assert_eq!(windows.carrying, Some((Grid::Inventory, 0)));
        assert_eq!(
            windows.drop_carried_item(&inventory, true),
            Some(ClientMessage::ItemDrop { slot: 0, count: 7 })
        );
        assert_eq!(windows.carrying, None);
    }

    #[test]
    fn right_click_stores_in_first_empty_slot_unless_carrying() {
        let mut windows = WindowState::default();
        let storage = [Some(item()), None, None];
        assert_eq!(
            windows.store_bag_item(7, &storage),
            Some(ClientMessage::ItemMove {
                from: Grid::Inventory,
                from_slot: 7,
                to: Grid::Storage,
                to_slot: 1,
            })
        );
        assert_eq!(windows.store_bag_item(7, &[Some(item())]), None);
        windows.carrying = Some((Grid::Inventory, 7));
        assert_eq!(windows.store_bag_item(7, &storage), None);
    }

    #[test]
    fn right_click_retrieves_into_first_empty_bag_slot_unless_carrying() {
        let mut windows = WindowState::default();
        let inventory = [Some(item()), None, None];
        assert_eq!(
            windows.retrieve_storage_item(7, &inventory),
            Some(ClientMessage::ItemMove {
                from: Grid::Storage,
                from_slot: 7,
                to: Grid::Inventory,
                to_slot: 1,
            })
        );
        assert_eq!(windows.retrieve_storage_item(7, &[Some(item())]), None);
        windows.carrying = Some((Grid::Storage, 7));
        assert_eq!(windows.retrieve_storage_item(7, &inventory), None);
    }

    #[test]
    fn title_drag_moves_window_and_ignores_close_button() {
        let mut layout = WindowLayout::default();
        let base = Rect::new(10.0, 40.0, 320.0, 440.0);
        let screen = (800, 600);
        let mut input = crate::ui::Input {
            mouse: (30.0, 50.0),
            lmb_pressed: true,
            lmb_down: true,
            ..Default::default()
        };
        assert_eq!(layout.place("character", base, &input, screen), base);
        input.lmb_pressed = false;
        input.mouse = (130.0, 90.0);
        assert_eq!(
            layout.place("character", base, &input, screen),
            Rect::new(110.0, 80.0, 320.0, 440.0)
        );
        input.lmb_down = false;
        layout.end_frame(&input);
        input.lmb_pressed = true;
        input.mouse = (110.0 + 320.0 - 10.0, 80.0 + 10.0);
        layout.place("character", base, &input, screen);
        assert!(layout.dragging.is_none());
    }
}
