//! The HUD button row and the dialogs behind it: Zircon's `MainPanel`
//! buttons (GameInter 82..117 at the panel's right edge), `MenuDialog`,
//! `HelpDialog`, `ExitDialog`, `CurrencyDialog`, `AutoPotionDialog` and
//! `FilterDropDialog`.
//!
//! The windows here own their own state so `windows.rs` only has to draw
//! them and apply the [`MenuAction`]s they return.

use crate::assets::lib;
use crate::gfx::Blend;
use crate::ui::{Button, Ctx, Rect, TextBox, GOLD};
use crate::windows::Bag;

/// A window the menu can toggle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Window {
    Character,
    Inventory,
    Skills,
    Quests,
    Mail,
    Belt,
    Group,
    Guild,
    Storage,
    Companion,
    Ranking,
    Friends,
    Market,
    Map,
    /// Owns its open flag on `Game`, so the toggle is relayed as a request.
    Fortune,
    ChatOptions,
    Settings,
}

/// What the player asked for by clicking the HUD or the menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuAction {
    Toggle(Window),
    /// Back to the character list.
    Logout,
    /// Close the client.
    Quit,
}

/// Zircon `MainPanel`: icon, x offset in the panel, window, hint.
const PANEL_BUTTONS: [(u32, f32, Option<Window>, &str, &str); 8] = [
    (82, 650.0, Some(Window::Character), "Character", "Q"),
    (87, 689.0, Some(Window::Inventory), "Inventory", "W"),
    (92, 728.0, Some(Window::Skills), "Skills", "E"),
    (112, 767.0, Some(Window::Quests), "Quest log", "J"),
    (97, 806.0, Some(Window::Mail), "Mail", ","),
    (107, 845.0, Some(Window::Belt), "Belt", "Z"),
    (102, 884.0, Some(Window::Group), "Group", "P"),
    (117, 923.0, None, "Menu", "N"),
];

/// The key list shown by the help window. Kept beside the real bindings in
/// `game/input.rs`; both follow Zircon's `KeyBindAction` defaults.
const KEYS: [(&str, &str); 35] = [
    ("Q", "Character"),
    ("D", "Appearance and discipline"),
    ("W", "Inventory"),
    ("E", "Skills"),
    ("J", "Quest log"),
    (",", "Mail"),
    ("Z", "Belt"),
    ("P", "Group"),
    ("C", "Market place"),
    ("G", "Guild"),
    ("S", "Storage"),
    ("U", "Companion"),
    ("R", "Rankings"),
    ("F", "Friends and blocked"),
    ("V", "Cycle the minimap"),
    ("X", "Big map"),
    ("K", "Fortune checker"),
    ("I", "Dungeon codex"),
    ("O", "Chat options"),
    ("N", "Menu"),
    ("H", "Help"),
    ("A", "Auto potion"),
    ("M", "Mount or dismount"),
    ("T", "Ask the player in front to trade"),
    ("Ctrl+H", "Attack mode"),
    ("Ctrl+A", "Pet mode"),
    ("Alt+Q", "Leave the game"),
    ("Tab", "Pick up"),
    ("B", "Harvest a nearby corpse"),
    ("1-9, 0 twice", "Use a belt slot"),
    ("F1-F12", "Cast a skill"),
    ("Cmd/Opt+1..0,-,=", "Mac skill slots"),
    ("Enter", "Chat, Escape closes it"),
    ("`", "Debug line"),
    ("Ctrl+Alt+S", "Screenshot"),
];

/// Auto potion (Zircon `AutoPotionDialog`): drink from a belt slot when a
/// pool drops below its percentage.
#[derive(Debug, Clone)]
pub struct AutoPotion {
    pub enabled: bool,
    /// Belt slot (0-based) and the percentage it fires below.
    pub hp_slot: u8,
    pub hp_percent: i32,
    pub mp_slot: u8,
    pub mp_percent: i32,
}

impl Default for AutoPotion {
    fn default() -> AutoPotion {
        AutoPotion {
            // ZIRCON_AUTO_POTION=1 turns it on for headless checks.
            enabled: std::env::var_os("ZIRCON_AUTO_POTION").is_some(),
            hp_slot: 0,
            hp_percent: 50,
            mp_slot: 1,
            mp_percent: 50,
        }
    }
}

#[derive(Default)]
pub struct MenuState {
    pub menu_open: bool,
    pub help_open: bool,
    pub exit_open: bool,
    pub currency_open: bool,
    pub auto_potion_open: bool,
    pub drop_filter_open: bool,
    pub auto_potion: AutoPotion,
    /// Names (lower case) whose ground items are highlighted.
    pub drop_filter: Vec<String>,
    panel_buttons: Vec<Button>,
    menu_buttons: Vec<Button>,
    exit_buttons: Vec<Button>,
    potion_buttons: Vec<Button>,
    potion_boxes: Vec<TextBox>,
    filter_boxes: Vec<TextBox>,
    help_scroll: f32,
    /// Hover hint drawn last: text and where.
    hint: Option<(String, f32, f32)>,
}

/// Zircon `MenuDialog`: what a menu row does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuEntry {
    Toggle(Window),
    Help,
    Currency,
    AutoPotion,
    DropFilter,
    Leave,
}

/// Menu rows: action, label, key hint.
const MENU_ENTRIES: [(MenuEntry, &str, &str); 15] = [
    (MenuEntry::Help, "Help", "H"),
    (MenuEntry::Toggle(Window::Map), "Map", "X"),
    (MenuEntry::Toggle(Window::Guild), "Guild", "G"),
    (MenuEntry::Toggle(Window::Storage), "Storage", "S"),
    (MenuEntry::Toggle(Window::Companion), "Companion", "U"),
    (MenuEntry::Toggle(Window::Ranking), "Rankings", "R"),
    (MenuEntry::Toggle(Window::Friends), "Friends", "F"),
    (MenuEntry::Toggle(Window::Market), "Market place", "C"),
    (MenuEntry::Currency, "Currency", ""),
    (MenuEntry::AutoPotion, "Auto potion", "A"),
    (MenuEntry::DropFilter, "Drop filter", ""),
    (MenuEntry::Toggle(Window::Fortune), "Fortune checker", "K"),
    (MenuEntry::Toggle(Window::ChatOptions), "Chat options", "O"),
    (MenuEntry::Toggle(Window::Settings), "Settings", ""),
    (MenuEntry::Leave, "Leave game", "Alt+Q"),
];

impl MenuState {
    /// True while one of these windows owns the keyboard.
    pub fn typing(&self) -> bool {
        self.potion_boxes
            .iter()
            .chain(&self.filter_boxes)
            .any(|b| b.focused)
    }

    /// True if any of these windows is open (Escape closes them first).
    pub fn any_open(&self) -> bool {
        self.menu_open
            || self.help_open
            || self.exit_open
            || self.currency_open
            || self.auto_potion_open
            || self.drop_filter_open
    }

    pub fn close_all(&mut self) {
        self.menu_open = false;
        self.help_open = false;
        self.exit_open = false;
        self.currency_open = false;
        self.auto_potion_open = false;
        self.drop_filter_open = false;
    }

    /// Draw the HUD buttons and every open dialog. Returns true while the
    /// mouse is over one of them, so the world ignores the click.
    pub fn draw(
        &mut self,
        c: &mut Ctx,
        bag: &Bag,
        width: i32,
        height: i32,
        actions: &mut Vec<MenuAction>,
    ) -> bool {
        self.hint = None;
        let mut over = self.draw_panel_buttons(c, width, height, actions);
        if self.menu_open {
            over |= self.draw_menu(c, width, height, actions);
        }
        if self.help_open {
            over |= self.draw_help(c, width, height);
        }
        if self.currency_open {
            over |= self.draw_currency(c, bag, width, height);
        }
        if self.auto_potion_open {
            over |= self.draw_auto_potion(c, width, height);
        }
        if self.drop_filter_open {
            over |= self.draw_drop_filter(c, width, height);
        }
        if self.exit_open {
            over |= self.draw_exit(c, width, height, actions);
        }
        if let Some((text, x, y)) = self.hint.clone() {
            self.draw_hint(c, &text, x, y, width, height);
        }
        over
    }

    /// Zircon `MainPanel`: the icon row on the panel's right half.
    fn draw_panel_buttons(
        &mut self,
        c: &mut Ctx,
        width: i32,
        height: i32,
        actions: &mut Vec<MenuAction>,
    ) -> bool {
        let Some(panel) = c
            .assets
            .info(lib::GAME_INTER, 50)
            .map(|i| (i.width, i.height))
        else {
            return false;
        };
        let (pw, ph) = (panel.0 as f32, panel.1 as f32);
        let px = (width as f32 - pw) / 2.0;
        let py = height as f32 - ph;
        if self.panel_buttons.is_empty() {
            self.panel_buttons = PANEL_BUTTONS
                .iter()
                .map(|(icon, _, _, _, _)| Button::image(lib::GAME_INTER, *icon, 0.0, 0.0))
                .collect();
        }
        let mouse = c.input.mouse;
        for (i, (_, dx, window, label, key)) in PANEL_BUTTONS.iter().enumerate() {
            let b = &mut self.panel_buttons[i];
            b.pos = (px + dx, py + 23.0);
            let clicked = b.update(c);
            let r = b.rect();
            if r.contains(mouse.0, mouse.1) {
                // Zircon leaves HoverIndex unset on these, so the highlight is
                // the same icon blended over itself rather than a second sprite.
                c.draw_tinted(
                    lib::GAME_INTER,
                    PANEL_BUTTONS[i].0,
                    r.x,
                    r.y + 1.0,
                    [0.45, 0.45, 0.3, 1.0],
                    Blend::Screen,
                );
                self.hint = Some((format!("{label} ({key})"), mouse.0, mouse.1));
            }
            if clicked {
                match window {
                    Some(w) => actions.push(MenuAction::Toggle(*w)),
                    None => self.menu_open = !self.menu_open,
                }
            }
        }
        // Clicking the HUD never walks the character.
        Rect::new(px, py, pw, ph).contains(mouse.0, mouse.1)
    }

    fn draw_menu(
        &mut self,
        c: &mut Ctx,
        width: i32,
        height: i32,
        actions: &mut Vec<MenuAction>,
    ) -> bool {
        // Above the belt and skill bars, with its right edge under the menu
        // button, the way Zircon's `MenuDialog` sits over the main panel.
        let win = Rect::new(
            width as f32 - 222.0,
            height as f32 - 202.0 - MENU_ENTRIES.len() as f32 * 26.0,
            210.0,
            MENU_ENTRIES.len() as f32 * 26.0 + 44.0,
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1);
        let closed = c.window(win, "Menu", false);
        if self.menu_buttons.is_empty() {
            self.menu_buttons = MENU_ENTRIES
                .iter()
                .map(|(_, label, _)| Button::default_style(0.0, 0.0, 136.0, label))
                .collect();
        }
        let mut hit = None;
        for (i, (_, _, key)) in MENU_ENTRIES.iter().enumerate() {
            let b = &mut self.menu_buttons[i];
            b.pos = (win.x + 16.0, win.y + 34.0 + i as f32 * 26.0);
            if b.update(c) {
                hit = Some(i);
            }
            if !key.is_empty() {
                let y = b.pos.1 + 5.0;
                let kw = c.text.width(key, 10);
                c.text
                    .draw(key, 10, win.x + 198.0 - kw, y, [150, 150, 150, 255]);
            }
        }
        if let Some(i) = hit {
            match MENU_ENTRIES[i].0 {
                MenuEntry::Toggle(w) => actions.push(MenuAction::Toggle(w)),
                MenuEntry::Help => self.help_open = !self.help_open,
                MenuEntry::Currency => self.currency_open = !self.currency_open,
                MenuEntry::AutoPotion => self.auto_potion_open = !self.auto_potion_open,
                MenuEntry::DropFilter => self.drop_filter_open = !self.drop_filter_open,
                MenuEntry::Leave => self.exit_open = true,
            }
            self.menu_open = false;
        }
        if closed {
            self.menu_open = false;
        }
        over
    }

    fn draw_help(&mut self, c: &mut Ctx, width: i32, height: i32) -> bool {
        let win = Rect::new(
            (width as f32 - 340.0) / 2.0,
            ((height as f32 - 470.0) / 2.0).max(8.0),
            340.0,
            470.0f32.min(height as f32 - 100.0),
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1);
        if over {
            self.help_scroll = (self.help_scroll - c.input.wheel * 18.0).max(0.0);
        }
        let closed = c.window(win, "Keys", false);
        let rows = KEYS.len() as f32;
        let visible = win.h - 56.0;
        self.help_scroll = self.help_scroll.min((rows * 18.0 - visible).max(0.0));
        for (i, (key, what)) in KEYS.iter().enumerate() {
            let y = win.y + 40.0 + i as f32 * 18.0 - self.help_scroll;
            if y < win.y + 34.0 || y + 18.0 > win.y + win.h - 10.0 {
                continue;
            }
            c.text.draw(key, 12, win.x + 20.0, y, GOLD);
            c.text
                .draw(what, 12, win.x + 110.0, y, [230, 230, 230, 255]);
        }
        if closed {
            self.help_open = false;
        }
        over
    }

    fn draw_currency(&mut self, c: &mut Ctx, bag: &Bag, width: i32, height: i32) -> bool {
        let win = Rect::new(
            (width as f32 - 280.0) / 2.0 - 40.0,
            (height as f32 - 300.0) / 2.0 - 60.0,
            280.0,
            300.0,
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1);
        let closed = c.window(win, "Currency", false);
        let mut y = win.y + 40.0;
        // Gold is carried in the bag, not in the currency table.
        c.text.draw("Gold", 12, win.x + 20.0, y, GOLD);
        let gold = bag.gold.to_string();
        let gw = c.text.width(&gold, 12);
        c.text.draw(
            &gold,
            12,
            win.x + win.w - 20.0 - gw,
            y,
            [255, 255, 255, 255],
        );
        y += 20.0;
        for cur in bag.currencies {
            if y + 18.0 > win.y + win.h - 10.0 {
                break;
            }
            let name = if cur.abbreviation.is_empty() {
                cur.name.clone()
            } else {
                format!("{} ({})", cur.name, cur.abbreviation)
            };
            c.text.draw(&name, 12, win.x + 20.0, y, GOLD);
            let amount = cur.amount.to_string();
            let aw = c.text.width(&amount, 12);
            c.text.draw(
                &amount,
                12,
                win.x + win.w - 20.0 - aw,
                y,
                [255, 255, 255, 255],
            );
            y += 20.0;
        }
        if bag.currencies.is_empty() {
            c.text.draw(
                "No other currencies yet.",
                11,
                win.x + 20.0,
                y + 4.0,
                [160, 160, 160, 255],
            );
        }
        if closed {
            self.currency_open = false;
        }
        over
    }

    fn draw_auto_potion(&mut self, c: &mut Ctx, width: i32, height: i32) -> bool {
        // Staggered so opening several of these at once still leaves every
        // title bar readable; none of them can be dragged.
        let win = Rect::new(
            (width as f32 - 300.0) / 2.0 + 60.0,
            (height as f32 - 190.0) / 2.0 + 70.0,
            300.0,
            190.0,
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1);
        let closed = c.window(win, "Auto potion", true);
        c.text.draw(
            "Drink from a belt slot when a pool runs low.",
            11,
            win.x + 16.0,
            win.y + 36.0,
            [200, 200, 200, 255],
        );
        if self.potion_boxes.is_empty() {
            let mut boxes: Vec<TextBox> = (0..4)
                .map(|_| TextBox::new(Rect::new(0.0, 0.0, 44.0, 22.0), 3))
                .collect();
            let p = &self.auto_potion;
            boxes[0].text = (p.hp_slot + 1).to_string();
            boxes[1].text = p.hp_percent.to_string();
            boxes[2].text = (p.mp_slot + 1).to_string();
            boxes[3].text = p.mp_percent.to_string();
            self.potion_boxes = boxes;
        }
        for (row, (label, pool)) in [("HP", 0usize), ("MP", 2usize)].iter().enumerate() {
            let y = win.y + 58.0 + row as f32 * 30.0;
            c.text.draw(label, 12, win.x + 16.0, y + 4.0, GOLD);
            c.text
                .draw("belt", 11, win.x + 48.0, y + 5.0, [200, 200, 200, 255]);
            self.potion_boxes[*pool].rect = Rect::new(win.x + 80.0, y, 44.0, 22.0);
            self.potion_boxes[*pool].update(c);
            self.potion_boxes[*pool]
                .text
                .retain(|ch| ch.is_ascii_digit());
            c.text
                .draw("below", 11, win.x + 132.0, y + 5.0, [200, 200, 200, 255]);
            self.potion_boxes[*pool + 1].rect = Rect::new(win.x + 176.0, y, 44.0, 22.0);
            self.potion_boxes[*pool + 1].update(c);
            self.potion_boxes[*pool + 1]
                .text
                .retain(|ch| ch.is_ascii_digit());
            c.text
                .draw("%", 11, win.x + 226.0, y + 5.0, [200, 200, 200, 255]);
        }
        if self.potion_buttons.is_empty() {
            self.potion_buttons = vec![Button::default_style(0.0, 0.0, 120.0, "Turn on")];
        }
        let b = &mut self.potion_buttons[0];
        b.pos = (win.x + 16.0, win.y + win.h - 3.0 - 42.0 + 8.0);
        b.label = Some(if self.auto_potion.enabled {
            "Turn off".into()
        } else {
            "Turn on".into()
        });
        let toggled = b.update(c);
        // Read the boxes back every frame so edits take effect at once.
        let read = |t: &str, default: i32| t.parse::<i32>().unwrap_or(default);
        self.auto_potion.hp_slot = read(&self.potion_boxes[0].text, 1).clamp(1, 10) as u8 - 1;
        self.auto_potion.hp_percent = read(&self.potion_boxes[1].text, 50).clamp(0, 99);
        self.auto_potion.mp_slot = read(&self.potion_boxes[2].text, 2).clamp(1, 10) as u8 - 1;
        self.auto_potion.mp_percent = read(&self.potion_boxes[3].text, 50).clamp(0, 99);
        if toggled {
            self.auto_potion.enabled = !self.auto_potion.enabled;
        }
        c.text.draw(
            if self.auto_potion.enabled {
                "On"
            } else {
                "Off"
            },
            12,
            win.x + 150.0,
            win.y + win.h - 3.0 - 42.0 + 13.0,
            if self.auto_potion.enabled {
                [120, 255, 120, 255]
            } else {
                [180, 180, 180, 255]
            },
        );
        if closed {
            self.auto_potion_open = false;
        }
        over
    }

    fn draw_drop_filter(&mut self, c: &mut Ctx, width: i32, height: i32) -> bool {
        let win = Rect::new(
            (width as f32 - 300.0) / 2.0 - 60.0,
            (height as f32 - 230.0) / 2.0 + 70.0,
            300.0,
            230.0,
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1);
        let closed = c.window(win, "Drop filter", false);
        c.text.draw(
            "Ground items matching these names are marked.",
            11,
            win.x + 16.0,
            win.y + 36.0,
            [200, 200, 200, 255],
        );
        if self.filter_boxes.is_empty() {
            self.filter_boxes = (0..5)
                .map(|_| TextBox::new(Rect::new(0.0, 0.0, 250.0, 22.0), 30))
                .collect();
            // ZIRCON_DROP_FILTER=meat,gold fills the boxes for headless checks.
            for (b, word) in self.filter_boxes.iter_mut().zip(
                std::env::var("ZIRCON_DROP_FILTER")
                    .unwrap_or_default()
                    .split(','),
            ) {
                b.text = word.trim().to_string();
            }
        }
        for (i, b) in self.filter_boxes.iter_mut().enumerate() {
            b.rect = Rect::new(win.x + 24.0, win.y + 58.0 + i as f32 * 28.0, 250.0, 22.0);
            b.update(c);
        }
        self.drop_filter = self
            .filter_boxes
            .iter()
            .map(|b| b.text.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if closed {
            self.drop_filter_open = false;
        }
        over
    }

    fn draw_exit(
        &mut self,
        c: &mut Ctx,
        width: i32,
        height: i32,
        actions: &mut Vec<MenuAction>,
    ) -> bool {
        let win = Rect::new(
            (width as f32 - 300.0) / 2.0,
            (height as f32 - 130.0) / 2.0,
            300.0,
            130.0,
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1);
        // Opaque backing: this one asks a question, so nothing shows through.
        c.fill(win, [0.06, 0.03, 0.03, 1.0]);
        let closed = c.window(win, "Leave the game", false);
        c.text.draw(
            "Back to the character list, or close the game?",
            11,
            win.x + 20.0,
            win.y + 40.0,
            [220, 220, 220, 255],
        );
        if self.exit_buttons.is_empty() {
            self.exit_buttons = vec![
                Button::default_style(0.0, 0.0, 120.0, "Character list"),
                Button::default_style(0.0, 0.0, 100.0, "Exit game"),
            ];
        }
        let b = &mut self.exit_buttons[0];
        b.pos = (win.x + 20.0, win.y + 78.0);
        if b.update(c) {
            actions.push(MenuAction::Logout);
            self.exit_open = false;
        }
        let b = &mut self.exit_buttons[1];
        b.pos = (win.x + 160.0, win.y + 78.0);
        if b.update(c) {
            actions.push(MenuAction::Quit);
        }
        if closed {
            self.exit_open = false;
        }
        over
    }

    /// A one-line tooltip, kept on screen and clear of the main panel.
    fn draw_hint(&self, c: &mut Ctx, text: &str, x: f32, y: f32, width: i32, height: i32) {
        let w = c.text.width(text, 11) + 12.0;
        let hx = (x + 14.0).min(width as f32 - w - 6.0).max(6.0);
        let hy = (y - 26.0).min(height as f32 - 96.0).max(6.0);
        let r = Rect::new(hx, hy, w, 20.0);
        c.panel(r, [24, 16, 16, 240]);
        c.text
            .draw(text, 11, r.x + 6.0, r.y + 3.0, [255, 255, 200, 255]);
    }
}

/// Belt slot to drink from, if a pool is below its threshold.
pub fn auto_potion_slot(potion: &AutoPotion, hp_pct: i32, mp_pct: i32) -> Option<u8> {
    if !potion.enabled {
        return None;
    }
    if hp_pct < potion.hp_percent {
        return Some(potion.hp_slot);
    }
    if mp_pct < potion.mp_percent {
        return Some(potion.mp_slot);
    }
    None
}

/// Does a ground item's name match one of the drop filters?
pub fn highlighted(filters: &[String], name: &str) -> bool {
    let name = name.to_lowercase();
    filters.iter().any(|f| name.contains(f.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_potion_picks_the_pool_that_ran_low() {
        let mut p = AutoPotion::default();
        assert_eq!(auto_potion_slot(&p, 10, 10), None, "off means never");
        p.enabled = true;
        assert_eq!(auto_potion_slot(&p, 90, 90), None);
        assert_eq!(auto_potion_slot(&p, 20, 90), Some(p.hp_slot));
        assert_eq!(auto_potion_slot(&p, 90, 20), Some(p.mp_slot));
        // Health comes first when both pools are low.
        assert_eq!(auto_potion_slot(&p, 5, 5), Some(p.hp_slot));
    }

    #[test]
    fn drop_filter_matches_on_a_substring() {
        let f = vec!["gold".to_string(), "sword".to_string()];
        assert!(highlighted(&f, "Gold Bar"));
        assert!(highlighted(&f, "Wooden Sword"));
        assert!(!highlighted(&f, "Herbal Medicine"));
        assert!(!highlighted(&[], "Gold Bar"), "no filter marks nothing");
    }

    #[test]
    fn the_help_window_names_every_advertised_key() {
        for (_, _, _, label, key) in PANEL_BUTTONS {
            assert!(!label.is_empty() && !key.is_empty());
            assert!(KEYS.iter().any(|(k, _)| *k == key), "help misses {key}");
        }
        for (_, _, key) in MENU_ENTRIES {
            assert!(
                key.is_empty() || KEYS.iter().any(|(k, _)| *k == key),
                "help misses {key}"
            );
        }
    }
}
