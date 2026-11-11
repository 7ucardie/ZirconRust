//! The drop-fortune readout (Zircon `FortuneCheckerDialog`).
//!
//! Every roll a monster makes for an item banks that roll's expected yield
//! against the account. Spending a Fortune Checker snapshots two numbers
//! for one item: how many have actually dropped, and how many *should*
//! have by now. Zircon shows the gap between them as a percentage, so a
//! player can see how close the next forced drop is.
//!
//! The top half searches the item catalogue by name; the bottom half lists
//! the items already checked, newest check first.

use mir_proto::{ClientMessage, FortuneSummary};

use crate::assets::lib;
use crate::items::ItemCatalog;
use crate::ui::{Button, Ctx, Rect, TextBox, GOLD};
use crate::windows::WindowLayout;

const ROW_H: f32 = 52.0;
const SEARCH_ROWS: usize = 3;
const CHECKED_ROWS: usize = 3;

/// The window's own state.
pub struct FortuneWindow {
    pub open: bool,
    search: TextBox,
    /// Items matching the search, as (index, name).
    results: Vec<(i32, String)>,
    /// The search text the results were built from.
    searched: String,
    search_scroll: usize,
    checked_scroll: usize,
    check_buttons: Vec<Button>,
    layout: WindowLayout,
}

impl Default for FortuneWindow {
    fn default() -> FortuneWindow {
        // ZIRCON_DEV_SEARCH prefills the box so a screenshot has rows.
        let mut search = TextBox::new(Rect::new(0.0, 0.0, 240.0, 22.0), 32);
        search.text = std::env::var("ZIRCON_DEV_SEARCH").unwrap_or_default();
        FortuneWindow {
            open: false,
            search,
            results: Vec::new(),
            searched: String::new(),
            search_scroll: 0,
            checked_scroll: 0,
            check_buttons: Vec::new(),
            layout: WindowLayout::default(),
        }
    }
}

/// Zircon's "to go": `1 + DropCount - Progress`, as a percentage. It falls
/// from 100 % just after a drop towards 0 % as the expectation catches up,
/// and at 0 the next roll is forced through.
pub fn to_go(drop_count: i64, progress: f64) -> f64 {
    (1.0 + drop_count as f64 - progress) * 100.0
}

/// Zircon formats the gap finely while it is small and coarsely once the
/// expectation has run far ahead.
pub fn format_to_go(drop_count: i64, progress: f64) -> String {
    let pct = to_go(drop_count, progress);
    if progress < 10_000.0 {
        format!("{pct:.5}%")
    } else {
        format!("{pct:.2}%")
    }
}

/// Unix seconds now, for measuring how stale a check is.
pub fn wall_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// "3h 20m ago" style, for the time since the check.
pub fn ago(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s ago"),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86_399 => format!("{}h {}m ago", seconds / 3600, seconds % 3600 / 60),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

impl FortuneWindow {
    /// True while the search box owns the keyboard.
    pub fn typing(&self) -> bool {
        self.open && self.search.focused
    }

    /// Draw the window. `fortunes` is what the server last sent, `now_secs`
    /// is the wall clock in Unix seconds. Any check the player asks for is
    /// pushed onto `out`. Returns true while the pointer is over it.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        c: &mut Ctx,
        catalog: &ItemCatalog,
        fortunes: &[FortuneSummary],
        now_secs: u64,
        width: i32,
        height: i32,
        out: &mut Vec<ClientMessage>,
    ) -> bool {
        if !self.open {
            return false;
        }
        let win = self.layout.place(
            "fortune",
            Rect::new(
                (width as f32 - 500.0) / 2.0,
                (height as f32 - 420.0) / 2.0,
                500.0,
                420.0,
            ),
            c.input,
            (width, height),
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1) || self.layout.is_dragging();
        let closed = c.window(win, "Fortune Checker", false) && !self.layout.is_dragging();

        // ---- search ----
        self.search.rect = Rect::new(win.x + 74.0, win.y + 36.0, 240.0, 22.0);
        c.text.draw("Search", 12, win.x + 16.0, win.y + 39.0, GOLD);
        self.search.update(c);
        if self.search.text != self.searched {
            self.searched = self.search.text.clone();
            self.search_scroll = 0;
            self.rebuild_results(catalog);
        }

        let list = Rect::new(
            win.x + 14.0,
            win.y + 66.0,
            win.w - 28.0,
            SEARCH_ROWS as f32 * ROW_H + 6.0,
        );
        c.panel(list, [0, 0, 0, 140]);
        if list.contains(c.input.mouse.0, c.input.mouse.1) && c.input.wheel != 0.0 {
            let max = self.results.len().saturating_sub(SEARCH_ROWS);
            self.search_scroll = if c.input.wheel > 0.0 {
                (self.search_scroll + 1).min(max)
            } else {
                self.search_scroll.saturating_sub(1)
            };
        }
        if self.results.is_empty() {
            let hint = if self.searched.trim().is_empty() {
                "Type part of an item's name to find it."
            } else {
                "No item by that name."
            };
            c.text
                .draw(hint, 12, list.x + 10.0, list.y + 10.0, [170, 170, 170, 255]);
        }

        // One Check button per visible row, rebuilt each frame so the
        // button always belongs to the row under it.
        let visible: Vec<(i32, String)> = self
            .results
            .iter()
            .skip(self.search_scroll)
            .take(SEARCH_ROWS)
            .cloned()
            .collect();
        if self.check_buttons.len() != visible.len() {
            self.check_buttons = (0..visible.len())
                .map(|_| Button::default_style(0.0, 0.0, 78.0, "Check"))
                .collect();
        }
        for (i, (index, name)) in visible.iter().enumerate() {
            let row = Rect::new(
                list.x + 3.0,
                list.y + 3.0 + i as f32 * ROW_H,
                list.w - 6.0,
                ROW_H,
            );
            let found = fortunes.iter().find(|f| f.item == *index);
            self.draw_row(c, catalog, row, *index, name, found, now_secs, true);
            let b = &mut self.check_buttons[i];
            b.pos = (row.x + row.w - 86.0, row.y + 15.0);
            if b.update(c) {
                out.push(ClientMessage::FortuneCheck { item: *index });
            }
        }

        // ---- already checked ----
        c.text
            .draw("Checked", 12, win.x + 16.0, win.y + 240.0, GOLD);
        let checked = Rect::new(
            win.x + 14.0,
            win.y + 260.0,
            win.w - 28.0,
            CHECKED_ROWS as f32 * ROW_H + 6.0,
        );
        c.panel(checked, [0, 0, 0, 140]);
        let mut sorted: Vec<&FortuneSummary> = fortunes.iter().collect();
        sorted.sort_by_key(|f| std::cmp::Reverse(f.checked_at));
        if checked.contains(c.input.mouse.0, c.input.mouse.1) && c.input.wheel != 0.0 {
            let max = sorted.len().saturating_sub(CHECKED_ROWS);
            self.checked_scroll = if c.input.wheel > 0.0 {
                (self.checked_scroll + 1).min(max)
            } else {
                self.checked_scroll.saturating_sub(1)
            };
        }
        if sorted.is_empty() {
            c.text.draw(
                "Nothing checked yet. Find an item above and spend a Fortune Checker on it.",
                11,
                checked.x + 10.0,
                checked.y + 10.0,
                [170, 170, 170, 255],
            );
        }
        for (i, f) in sorted
            .iter()
            .skip(self.checked_scroll)
            .take(CHECKED_ROWS)
            .enumerate()
        {
            let row = Rect::new(
                checked.x + 3.0,
                checked.y + 3.0 + i as f32 * ROW_H,
                checked.w - 6.0,
                ROW_H,
            );
            let name = catalog.name(f.item);
            self.draw_row(c, catalog, row, f.item, &name, Some(f), now_secs, false);
        }

        if closed {
            self.open = false;
        }
        self.layout.end_frame(c.input);
        over
    }

    /// One row: the item's icon and name, then its numbers (or "Not
    /// checked" when no checker has been spent on it).
    #[allow(clippy::too_many_arguments)]
    fn draw_row(
        &self,
        c: &mut Ctx,
        catalog: &ItemCatalog,
        row: Rect,
        item: i32,
        name: &str,
        fortune: Option<&FortuneSummary>,
        now_secs: u64,
        // Search rows carry a Check button where the third column would
        // sit, so they show two columns rather than three.
        room_for_button: bool,
    ) {
        c.fill(row, [0.1, 0.08, 0.0, 0.6]);
        let cell = Rect::new(row.x + 4.0, row.y + 6.0, 40.0, 40.0);
        c.fill(cell, [0.0, 0.0, 0.0, 0.55]);
        c.border(cell, [99, 83, 50, 255]);
        if let Some(def) = catalog.get(item) {
            if let Some(info) = c.assets.info(lib::STORE_ITEMS, def.image as u32) {
                let (w, h) = (info.width as f32, info.height as f32);
                let scale = (cell.w / w).min(cell.h / h).min(1.0);
                let (dw, dh) = (w * scale, h * scale);
                if let Some(sprite) = c.sprite(lib::STORE_ITEMS, def.image as u32) {
                    c.renderer.draw_scaled(
                        sprite,
                        cell.x + (cell.w - dw) / 2.0,
                        cell.y + (cell.h - dh) / 2.0,
                        dw,
                        dh,
                        [1.0, 1.0, 1.0, 1.0],
                        crate::gfx::Blend::Alpha,
                    );
                }
            }
        }
        c.text.draw(name, 12, row.x + 50.0, row.y + 18.0, GOLD);
        let (dropped, togo, when) = match fortune {
            Some(f) => (
                f.drop_count.to_string(),
                format_to_go(f.drop_count, f.progress),
                ago(now_secs.saturating_sub(f.checked_at)),
            ),
            None => (
                "Not checked".into(),
                "Not checked".into(),
                "Not checked".into(),
            ),
        };
        let mut columns: Vec<(&str, String)> = vec![("Dropped", dropped), ("To go", togo)];
        if !room_for_button {
            columns.push(("Checked", when));
        }
        let mut x = row.x + 190.0;
        for (label, value) in columns {
            c.text.draw(label, 10, x, row.y + 8.0, [180, 180, 180, 255]);
            c.text
                .draw(&value, 11, x, row.y + 24.0, [255, 255, 255, 255]);
            x += 96.0;
        }
    }

    /// Items whose name contains the search text, capped so a one-letter
    /// search does not build a thousand-row list.
    fn rebuild_results(&mut self, catalog: &ItemCatalog) {
        self.results.clear();
        let needle = self.searched.trim().to_lowercase();
        if needle.is_empty() {
            return;
        }
        let mut hits: Vec<(i32, String)> = catalog
            .names()
            .filter(|(_, name)| name.to_lowercase().contains(&needle))
            .map(|(i, name)| (i, name.to_string()))
            .collect();
        hits.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        hits.truncate(50);
        self.results = hits;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_go_falls_from_full_to_nothing() {
        // Nothing dropped and nothing expected: a whole item to go.
        assert!((to_go(0, 0.0) - 100.0).abs() < 1e-9);
        // Half the expectation banked: half an item to go.
        assert!((to_go(0, 0.5) - 50.0).abs() < 1e-9);
        // The expectation has caught up with reality: the next roll is owed.
        assert!((to_go(0, 1.0)).abs() < 1e-9);
        // Counts and expectation move together, so the gap resets.
        assert!((to_go(3, 3.0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn small_progress_is_shown_finely() {
        // Under 10000 Zircon uses five decimals, above it two.
        assert_eq!(format_to_go(0, 0.5), "50.00000%");
        assert_eq!(format_to_go(10_000, 10_000.5), "50.00%");
    }

    #[test]
    fn ago_reads_in_the_largest_useful_unit() {
        assert_eq!(ago(0), "0s ago");
        assert_eq!(ago(59), "59s ago");
        assert_eq!(ago(60), "1m ago");
        assert_eq!(ago(3600), "1h 0m ago");
        assert_eq!(ago(3600 + 20 * 60), "1h 20m ago");
        assert_eq!(ago(86_400 * 3), "3d ago");
    }
}
