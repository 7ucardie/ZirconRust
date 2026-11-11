//! The ranking board (Zircon `RankingDialog`, opened with R).
//!
//! Zircon shows eleven lines at a time with a rank number, an online lamp,
//! the name, the level and how far the name has moved since the last reset.
//! The class filter is a drop-down there; here it is a row of tabs, because
//! the five choices fit and a tab row needs no second click to read.

use mir_proto::{Class, ClientMessage, Gender, RankEntry, RankInspection, EQUIPMENT_SIZE};

use crate::assets::lib;
use crate::items::ItemCatalog;
use crate::ui::{Button, Ctx, Rect, TextBox, GOLD};
use crate::windows::{draw_item_cell, WindowLayout};

/// Visible lines, as Zircon's `Lines` array is sized.
pub const ROWS: usize = 11;
/// Row pitch, from `Location = new Point(12, 16 + (23 * i))`.
const ROW_H: f32 = 23.0;
const BOARD_W: f32 = 430.0;
const WIN_W: f32 = 700.0;
/// The gold Zircon borders its panels with.
const BORDER: [u8; 4] = [198, 166, 99, 255];
/// Zircon highlights the selected line with `Color.FromArgb(50, 255, 16, 16)`.
const OWN_ROW: [f32; 4] = [1.0, 0.06, 0.06, 0.2];
/// How often an open board asks the server again, so levels gained by other
/// players show up without the player touching anything.
const REFRESH_MS: u64 = 5_000;

/// The class tabs, in Zircon's drop-down order. `None` is `RequiredClass.All`.
const TABS: [(Option<Class>, &str); 5] = [
    (None, "All"),
    (Some(Class::Warrior), "Warrior"),
    (Some(Class::Wizard), "Wizard"),
    (Some(Class::Taoist), "Taoist"),
    (Some(Class::Assassin), "Assassin"),
];

/// The board as the client holds it: the last page the server sent plus the
/// filters that page was asked for.
#[derive(Debug, Default)]
pub struct RankingView {
    pub entries: Vec<RankEntry>,
    pub total: u32,
    /// Index of the first row of `entries` within the filtered board.
    pub start: u32,
    pub class: Option<Class>,
    pub online_only: bool,
}

/// What the window is asking for, which may be ahead of what it has.
#[derive(Default)]
pub struct RankingState {
    pub open: bool,
    pub class: Option<Class>,
    pub online_only: bool,
    /// First row the player wants to see.
    pub start: u32,
    tabs: Vec<Button>,
    online_button: Option<Button>,
    search: Option<TextBox>,
    search_button: Option<Button>,
    selected: Option<u32>,
    inspection: Option<RankInspection>,
    search_status: String,
    /// When the next automatic refresh is due (0 = ask at once).
    next_request: u64,
    /// The filters the last request carried, so a change re-asks immediately.
    asked: Option<(Option<Class>, bool, u32)>,
    layout: WindowLayout,
}

/// Zircon's class colours, so a name reads as its class at a glance.
fn class_color(class: Class) -> [u8; 4] {
    match class {
        Class::Warrior => [255, 190, 150, 255],
        Class::Wizard => [150, 200, 255, 255],
        Class::Taoist => [150, 255, 190, 255],
        Class::Assassin => [230, 180, 255, 255],
    }
}

fn class_name(class: Class) -> &'static str {
    match class {
        Class::Warrior => "Warrior",
        Class::Wizard => "Wizard",
        Class::Taoist => "Taoist",
        Class::Assassin => "Assassin",
    }
}

/// The movement column: Zircon writes a climb in orange-red, a fall in
/// dodger blue and a hold as a white dash.
pub fn change_label(change: i32) -> (String, [u8; 4]) {
    match change {
        0 => (" - ".to_string(), [255, 255, 255, 255]),
        n if n > 0 => (format!("+{n}"), [255, 69, 0, 255]),
        n => (format!("{n}"), [30, 144, 255, 255]),
    }
}

/// The name as the board writes it, with Zircon's rebirth suffix.
pub fn row_name(entry: &RankEntry) -> String {
    if entry.rebirth > 0 {
        format!("{} [Rebirth: {}]", entry.name, entry.rebirth)
    } else {
        entry.name.clone()
    }
}

/// The class named by `ZIRCON_RANK_CLASS`, for headless checks of the tabs.
pub fn class_from_name(name: &str) -> Option<Class> {
    match name.trim().to_ascii_lowercase().as_str() {
        "warrior" => Some(Class::Warrior),
        "wizard" => Some(Class::Wizard),
        "taoist" => Some(Class::Taoist),
        "assassin" => Some(Class::Assassin),
        _ => None,
    }
}

/// How far a page may start without running off the end of the board.
pub fn clamp_start(start: u32, total: u32) -> u32 {
    let last = total.saturating_sub(ROWS as u32);
    start.min(last)
}

impl RankingState {
    pub fn typing(&self) -> bool {
        self.open && self.search.as_ref().is_some_and(|box_| box_.focused)
    }

    /// C# selects the search result and immediately requests its equipment.
    pub fn search_result(&mut self, entry: Option<RankEntry>, start: u32) -> Option<u32> {
        self.class = None;
        self.online_only = false;
        self.start = start.saturating_sub((ROWS - 1) as u32);
        self.asked = None;
        self.selected = entry.as_ref().map(|entry| entry.character);
        self.inspection = None;
        self.search_status = if entry.is_some() {
            String::new()
        } else {
            "Character not found.".into()
        };
        self.selected
    }

    pub fn set_inspection(&mut self, details: Option<RankInspection>) {
        self.inspection = details.filter(|details| self.selected == Some(details.character));
        if self.inspection.is_none() && self.selected.is_some() {
            self.search_status = "Character unavailable.".into();
        }
    }

    /// Draw the board. `own_name` marks the player's own row -- character
    /// names are unique, so the board needs no id from the server.
    /// Returns true while the mouse is over it (so a click does not also
    /// walk the character).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        c: &mut Ctx,
        view: &RankingView,
        catalog: &ItemCatalog,
        own_name: &str,
        width: i32,
        height: i32,
        out: &mut Vec<ClientMessage>,
    ) -> bool {
        if !self.open {
            return false;
        }
        let h = ROWS as f32 * ROW_H + 185.0;
        let win = self.layout.place(
            "ranking",
            Rect::new(
                (width as f32 - WIN_W) / 2.0,
                ((height as f32 - h) / 2.0).max(30.0),
                WIN_W,
                h,
            ),
            c.input,
            (width, height),
        );
        let mouse = c.input.mouse;
        let over = win.contains(mouse.0, mouse.1) || self.layout.is_dragging();
        if c.window(win, "Rankings", true) && !self.layout.is_dragging() {
            self.open = false;
            self.layout.end_frame(c.input);
            return over;
        }

        // ---- class tabs ----
        if self.tabs.is_empty() {
            self.tabs = TABS
                .iter()
                .map(|(_, label)| Button::default_style(0.0, 0.0, 78.0, label))
                .collect();
        }
        for (i, (class, _)) in TABS.iter().enumerate() {
            let b = &mut self.tabs[i];
            b.pos = (win.x + 10.0 + i as f32 * 82.0, win.y + 32.0);
            b.selected = self.class == *class;
            if b.update(c) && self.class != *class {
                self.class = *class;
                self.start = 0;
            }
        }

        // ---- online-only toggle ----
        let ob = self
            .online_button
            .get_or_insert_with(|| Button::default_style(0.0, 0.0, 110.0, "Online only"));
        ob.pos = (win.x + 10.0, win.y + 60.0);
        ob.selected = self.online_only;
        if ob.update(c) {
            self.online_only = !self.online_only;
            self.start = 0;
        }
        let search = self
            .search
            .get_or_insert_with(|| TextBox::new(Rect::new(0.0, 0.0, 145.0, 22.0), 40));
        search.rect.x = win.x + 177.0;
        search.rect.y = win.y + 61.0;
        let submit = search.update(c);
        let button = self
            .search_button
            .get_or_insert_with(|| Button::default_style(0.0, 0.0, 85.0, "Search"));
        button.pos = (win.x + 328.0, win.y + 60.0);
        let clicked = button.update(c);
        if (submit || clicked) && !search.text.trim().is_empty() {
            out.push(ClientMessage::RankSearch {
                name: search.text.trim().to_owned(),
            });
            self.search_status = "Searching...".into();
        }
        c.text.draw(
            &format!("{} ranked", view.total),
            12,
            win.x + 130.0,
            win.y + 64.0,
            [200, 200, 200, 255],
        );

        // ---- scrolling ----
        let list = Rect::new(
            win.x + 8.0,
            win.y + 110.0,
            BOARD_W - 16.0,
            ROWS as f32 * ROW_H,
        );
        if list.contains(mouse.0, mouse.1) && c.input.wheel != 0.0 {
            let step = if c.input.wheel > 0.0 { -3.0 } else { 3.0 };
            self.start = (self.start as f32 + step).max(0.0) as u32;
        }
        if view.class == self.class && view.online_only == self.online_only {
            self.start = clamp_start(self.start, view.total);
        }

        // ---- header ----
        let (rank_x, name_x, level_x, change_x) = (
            list.x + 8.0,
            list.x + 62.0,
            list.x + 250.0,
            list.x + list.w - 50.0,
        );
        let head_y = list.y - 18.0;
        for (x, label) in [
            (rank_x, "#"),
            (name_x, "Name"),
            (level_x, "Level"),
            (change_x, "Move"),
        ] {
            c.text.draw(label, 12, x, head_y, [255, 220, 150, 255]);
        }
        c.fill(
            Rect::new(list.x, list.y - 2.0, list.w, 1.0),
            [0.78, 0.65, 0.39, 0.7],
        );

        // ---- rows ----
        // The page in hand may have been asked for under other filters (the
        // player just switched tabs and the answer is still in flight).
        // Zircon greys its lines as "Updating..." in that gap; do the same
        // rather than show rows that belong to a different board.
        let fresh = view.class == self.class && view.online_only == self.online_only;
        if !fresh {
            c.text.draw(
                "Updating...",
                12,
                list.x + 8.0,
                list.y + 6.0,
                [255, 165, 0, 255],
            );
        }
        // The page the server sent may begin above the row the player has
        // scrolled to; skip into it rather than showing the wrong ranks.
        let skip = self.start.saturating_sub(view.start) as usize;
        for i in 0..ROWS {
            if !fresh {
                break;
            }
            let y = list.y + i as f32 * ROW_H;
            let Some(entry) = view.entries.get(skip + i) else {
                break;
            };
            let row = Rect::new(list.x, y, list.w, ROW_H);
            if self.selected == Some(entry.character) {
                c.fill(row, [0.1, 0.5, 0.1, 0.3]);
                c.border(row, [0, 220, 0, 255]);
            } else if entry.name.eq_ignore_ascii_case(own_name) {
                c.fill(row, OWN_ROW);
                c.border(row, BORDER);
            } else if row.contains(mouse.0, mouse.1) {
                c.fill(row, [1.0, 1.0, 1.0, 0.07]);
            }
            if row.contains(mouse.0, mouse.1)
                && c.input.lmb_released
                && !self.layout.is_dragging()
                && self.selected != Some(entry.character)
            {
                self.selected = Some(entry.character);
                self.inspection = None;
                self.search_status.clear();
                out.push(ClientMessage::RankInspect {
                    character: entry.character,
                });
            }
            let ty = y + 4.0;
            c.text.draw(
                &entry.rank.to_string(),
                12,
                rank_x,
                ty,
                [255, 255, 255, 255],
            );
            // Zircon's lamp: 3625 lit for online, 3624 dark for offline.
            c.draw(
                lib::GAME_INTER,
                if entry.online { 3625 } else { 3624 },
                list.x + 38.0,
                y + 5.0,
            );
            c.text
                .draw(&row_name(entry), 12, name_x, ty, class_color(entry.class));
            c.text.draw(
                &format!("Lv. {}", entry.level),
                12,
                level_x,
                ty,
                [255, 255, 255, 255],
            );
            let (label, color) = change_label(entry.change);
            c.text.draw(&label, 12, change_x, ty, color);
        }

        // ---- footer ----
        let shown = if fresh {
            view.entries.len().min(ROWS)
        } else {
            0
        };
        let first = if shown == 0 { 0 } else { self.start + 1 };
        let footer = if view.total == 0 {
            "No characters ranked yet.".to_string()
        } else {
            let filter = match self.class {
                None => "All classes".to_string(),
                Some(cl) => class_name(cl).to_string(),
            };
            format!(
                "{filter}{}  --  showing {}-{} of {}",
                if self.online_only { ", online" } else { "" },
                first,
                self.start + shown as u32,
                view.total
            )
        };
        c.text.draw(
            &footer,
            12,
            win.x + 14.0,
            win.y + win.h - 34.0,
            [220, 220, 220, 255],
        );

        self.draw_inspection(c, catalog, win);

        // ---- ask the server ----
        let want = (self.class, self.online_only, self.start);
        if self.asked != Some(want) || c.now >= self.next_request {
            self.asked = Some(want);
            self.next_request = c.now + REFRESH_MS;
            out.push(ClientMessage::RankRequest {
                class: self.class,
                online_only: self.online_only,
                start: self.start,
            });
        }
        self.layout.end_frame(c.input);
        over
    }

    fn draw_inspection(&self, c: &mut Ctx, catalog: &ItemCatalog, win: Rect) {
        let panel = Rect::new(win.x + BOARD_W + 2.0, win.y + 31.0, 258.0, win.h - 43.0);
        c.fill(panel, [0.02, 0.01, 0.01, 0.8]);
        c.border(panel, BORDER);
        c.text
            .draw("Character", 14, panel.x + 12.0, panel.y + 8.0, GOLD);
        let Some(details) = &self.inspection else {
            let text = if self.search_status.is_empty() {
                "Select a character."
            } else {
                &self.search_status
            };
            c.text.draw(
                text,
                12,
                panel.x + 12.0,
                panel.y + 34.0,
                [210, 210, 210, 255],
            );
            return;
        };
        c.text
            .draw(&details.name, 14, panel.x + 12.0, panel.y + 33.0, GOLD);
        let gender = match details.gender {
            Gender::Male => "Male",
            Gender::Female => "Female",
        };
        c.text.draw(
            &format!(
                "Lv. {} {} {gender}",
                details.level,
                class_name(details.class)
            ),
            12,
            panel.x + 12.0,
            panel.y + 54.0,
            [220, 220, 220, 255],
        );
        if !details.guild_name.is_empty() {
            c.text.draw(
                &format!("{} ({})", details.guild_name, details.guild_rank),
                12,
                panel.x + 12.0,
                panel.y + 75.0,
                [160, 190, 255, 255],
            );
        }
        c.text.draw(
            &format!("Fame: {}  Partner: {}", details.fame, details.partner),
            11,
            panel.x + 12.0,
            panel.y + 92.0,
            [180, 180, 180, 255],
        );
        let mut hover_name = None;
        for slot in 0..EQUIPMENT_SIZE {
            let r = Rect::new(
                panel.x + 12.0 + (slot % 4) as f32 * 58.0,
                panel.y + 112.0 + (slot / 4) as f32 * 40.0,
                38.0,
                38.0,
            );
            let hovered = r.contains(c.input.mouse.0, c.input.mouse.1);
            let item = details
                .equipment
                .iter()
                .find(|(item_slot, _)| *item_slot as usize == slot)
                .map(|(_, item)| item);
            draw_item_cell(c, catalog, r, item, hovered, false);
            if hovered {
                hover_name = item
                    .and_then(|item| catalog.get(item.info))
                    .map(|def| &def.name);
            }
        }
        if let Some(name) = hover_name {
            c.text
                .draw(name, 12, panel.x + 12.0, panel.y + panel.h - 27.0, GOLD);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(rank: u32, change: i32, rebirth: i32) -> RankEntry {
        RankEntry {
            rank,
            character: rank,
            name: format!("N{rank}"),
            class: Class::Warrior,
            level: 10,
            experience: 0,
            max_experience: 100,
            online: false,
            rebirth,
            change,
        }
    }

    #[test]
    fn the_class_switch_names_every_class() {
        assert_eq!(class_from_name("Warrior"), Some(Class::Warrior));
        assert_eq!(class_from_name("taoist"), Some(Class::Taoist));
        // Anything else, "all" included, is Zircon's unfiltered board.
        assert_eq!(class_from_name("all"), None);
        assert_eq!(class_from_name(""), None);
    }

    #[test]
    fn change_column_matches_zircon() {
        // A hold is a white dash, a climb orange-red, a fall dodger blue.
        assert_eq!(change_label(0), (" - ".to_string(), [255, 255, 255, 255]));
        assert_eq!(change_label(3), ("+3".to_string(), [255, 69, 0, 255]));
        assert_eq!(change_label(-2), ("-2".to_string(), [30, 144, 255, 255]));
    }

    #[test]
    fn rebirth_is_appended_to_the_name() {
        assert_eq!(row_name(&entry(1, 0, 0)), "N1");
        assert_eq!(row_name(&entry(1, 0, 2)), "N1 [Rebirth: 2]");
    }

    #[test]
    fn a_page_cannot_start_past_the_last_full_screen() {
        // 30 rows and 11 lines: the furthest first row is index 19.
        assert_eq!(clamp_start(99, 30), 19);
        assert_eq!(clamp_start(5, 30), 5);
        // Fewer rows than lines: always start at the top.
        assert_eq!(clamp_start(4, 6), 0);
    }

    #[test]
    fn search_selects_result_and_scrolls_it_into_view() {
        let mut state = RankingState {
            class: Some(Class::Wizard),
            online_only: true,
            ..Default::default()
        };
        assert_eq!(state.search_result(Some(entry(30, 0, 0)), 29), Some(30));
        assert_eq!(state.start, 19);
        assert_eq!(state.class, None);
        assert!(!state.online_only);
        assert_eq!(state.selected, Some(30));
        assert_eq!(state.search_result(None, 0), None);
        assert_eq!(state.search_status, "Character not found.");
    }
}
