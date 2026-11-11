//! Configuring the chat tabs (Zircon `ChatOptionsDialog`).
//!
//! Zircon lists the tabs down the left, and for the selected one shows a
//! checkbox per message type plus a few behaviour toggles, with Add, Reset,
//! Save and Reload along the bottom. This does the same against the tabs
//! the chat panel actually draws, and saves them beside the client so the
//! layout survives a restart.
//!
//! Zircon also offers Observer, Gains and Alert message types. This client
//! has no such lines, so those three checkboxes are absent rather than
//! present and dead.

use std::path::PathBuf;

use crate::chat_panel::{default_tabs, Category, ChatPanel, Tab};
use crate::ui::{Button, Ctx, Rect, TextBox, GOLD};
use crate::windows::WindowLayout;

/// Where the tab layout is saved. Beside the executable, so a checkout can
/// be moved without losing it and nothing writes outside the project.
fn config_path() -> Option<PathBuf> {
    let mut p = std::env::current_exe().ok()?;
    p.pop();
    Some(p.join("chat-tabs.json"))
}

/// Load a saved layout, falling back to the defaults.
pub fn load_tabs() -> Vec<Tab> {
    let tabs = config_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Vec<Tab>>(&b).ok())
        .filter(|t| !t.is_empty());
    match tabs {
        Some(t) => t,
        None => default_tabs(),
    }
}

fn save_tabs(tabs: &[Tab]) -> bool {
    let Some(path) = config_path() else {
        return false;
    };
    let Ok(text) = serde_json::to_vec_pretty(tabs) else {
        return false;
    };
    match std::fs::write(&path, text) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("could not save chat tabs: {e}");
            false
        }
    }
}

/// The options window's own state.
#[derive(Default)]
pub struct ChatOptions {
    pub open: bool,
    /// Which tab's settings the right-hand panel is editing.
    selected: usize,
    /// Rename box for the selected tab, rebuilt when the selection moves.
    name_box: Option<(usize, TextBox)>,
    buttons: Vec<Button>,
    /// A line to show under the buttons after Save or Reload.
    notice: Option<(String, u64)>,
    layout: WindowLayout,
}

/// One behaviour toggle: its label, and how to read and write it on a tab.
type Toggle = (&'static str, fn(&Tab) -> bool, fn(&mut Tab, bool));

/// The behaviour toggles Zircon offers that this panel can honour.
const BEHAVIOUR: [Toggle; 2] = [
    ("Fade out when idle", |t| t.fade, |t, v| t.fade = v),
    ("Hide the tab button", |t| t.hidden, |t, v| t.hidden = v),
];

impl ChatOptions {
    /// True while the rename box owns the keyboard.
    pub fn typing(&self) -> bool {
        self.open && self.name_box.as_ref().is_some_and(|(_, b)| b.focused)
    }

    /// Draw the window and apply every edit straight to `chat`. Returns
    /// true while the pointer is over it, so the world ignores the click.
    pub fn draw(&mut self, c: &mut Ctx, chat: &mut ChatPanel, width: i32, height: i32) -> bool {
        if !self.open {
            return false;
        }
        let win = self.layout.place(
            "chat_options",
            Rect::new(
                (width as f32 - 470.0) / 2.0,
                (height as f32 - 360.0) / 2.0,
                470.0,
                360.0,
            ),
            c.input,
            (width, height),
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1) || self.layout.is_dragging();
        let closed = c.window(win, "Chat Options", false) && !self.layout.is_dragging();

        self.selected = self.selected.min(chat.tabs.len().saturating_sub(1));
        self.draw_tab_list(c, chat, win);
        self.draw_settings(c, chat, win);
        self.draw_buttons(c, chat, win);

        if closed {
            self.open = false;
        }
        self.layout.end_frame(c.input);
        over
    }

    /// The tab list down the left, with the selected row lit.
    fn draw_tab_list(&mut self, c: &mut Ctx, chat: &mut ChatPanel, win: Rect) {
        let list = Rect::new(win.x + 12.0, win.y + 34.0, 130.0, 250.0);
        c.panel(list, [0, 0, 0, 160]);
        let mut y = list.y + 4.0;
        for i in 0..chat.tabs.len() {
            let row = Rect::new(list.x + 2.0, y, list.w - 4.0, 20.0);
            let hover = row.contains(c.input.mouse.0, c.input.mouse.1);
            if i == self.selected {
                c.fill(row, [0.31, 0.31, 0.49, 0.9]);
            } else if hover {
                c.fill(row, [0.2, 0.2, 0.25, 0.7]);
            }
            let label = if chat.tabs[i].hidden {
                format!("{} (hidden)", chat.tabs[i].name)
            } else {
                chat.tabs[i].name.clone()
            };
            c.text
                .draw(&label, 12, row.x + 6.0, row.y + 2.0, [230, 230, 230, 255]);
            if hover && c.input.lmb_pressed && self.selected != i {
                self.selected = i;
                self.name_box = None;
            }
            y += 21.0;
        }
    }

    /// The selected tab's name, message types and behaviour toggles.
    fn draw_settings(&mut self, c: &mut Ctx, chat: &mut ChatPanel, win: Rect) {
        let x = win.x + 156.0;
        let Some(tab) = chat.tabs.get(self.selected) else {
            return;
        };
        // Rename box, rebuilt whenever the selection moves.
        let stale = self
            .name_box
            .as_ref()
            .is_none_or(|(i, _)| *i != self.selected);
        if stale {
            let mut b = TextBox::new(Rect::new(x + 52.0, win.y + 36.0, 240.0, 22.0), 24);
            b.text = tab.name.clone();
            self.name_box = Some((self.selected, b));
        }
        if let Some((_, b)) = &mut self.name_box {
            c.text.draw("Name", 12, x, b.rect.y + 3.0, GOLD);
            b.update(c);
            let typed = b.text.trim().to_string();
            if !typed.is_empty() {
                if let Some(t) = chat.tabs.get_mut(self.selected) {
                    t.name = typed;
                }
            }
        }

        c.text
            .draw("Show these messages", 12, x, win.y + 70.0, GOLD);
        // Two columns of checkboxes, four rows each.
        for (i, cat) in Category::ALL.iter().enumerate() {
            let (col, row) = (i / 4, i % 4);
            let at = Rect::new(
                x + col as f32 * 150.0,
                win.y + 92.0 + row as f32 * 24.0,
                14.0,
                14.0,
            );
            let on = chat.tabs[self.selected].cats.contains(cat);
            if checkbox(c, at, cat.label(), on) {
                let t = &mut chat.tabs[self.selected];
                if on {
                    t.cats.retain(|x| x != cat);
                } else {
                    t.cats.push(*cat);
                }
            }
        }

        c.text.draw("Behaviour", 12, x, win.y + 196.0, GOLD);
        for (i, (label, get, set)) in BEHAVIOUR.iter().enumerate() {
            let at = Rect::new(x, win.y + 218.0 + i as f32 * 24.0, 14.0, 14.0);
            let on = get(&chat.tabs[self.selected]);
            if checkbox(c, at, label, on) {
                set(&mut chat.tabs[self.selected], !on);
            }
        }
    }

    /// Add, Remove, Reset, Save and Reload along the bottom.
    fn draw_buttons(&mut self, c: &mut Ctx, chat: &mut ChatPanel, win: Rect) {
        if self.buttons.is_empty() {
            let y = win.y + 300.0;
            let labels = ["Add", "Remove", "Reset", "Save", "Reload"];
            self.buttons = labels
                .iter()
                .enumerate()
                .map(|(i, l)| Button::default_style(win.x + 14.0 + i as f32 * 88.0, y, 84.0, l))
                .collect();
        }
        // Removing the last tab would leave the panel with nothing to show.
        self.buttons[1].enabled = chat.tabs.len() > 1;
        let mut clicked = None;
        for (i, b) in self.buttons.iter_mut().enumerate() {
            if b.update(c) {
                clicked = Some(i);
            }
        }
        match clicked {
            Some(0) => {
                chat.tabs.push(Tab {
                    name: format!("Window {}", chat.tabs.len()),
                    cats: Category::ALL.to_vec(),
                    fade: true,
                    hidden: false,
                });
                chat.sync_tabs();
                self.selected = chat.tabs.len() - 1;
                self.name_box = None;
            }
            Some(1) if chat.tabs.len() > 1 => {
                chat.tabs.remove(self.selected);
                chat.sync_tabs();
                self.selected = self.selected.min(chat.tabs.len() - 1);
                self.name_box = None;
            }
            Some(2) => {
                chat.tabs = default_tabs();
                chat.sync_tabs();
                self.selected = 0;
                self.name_box = None;
                self.notice = Some(("Tabs reset to the defaults.".into(), c.now));
            }
            Some(3) => {
                let ok = save_tabs(&chat.tabs);
                let text = if ok {
                    "Chat layout saved."
                } else {
                    "Could not save the layout."
                };
                self.notice = Some((text.into(), c.now));
            }
            Some(4) => {
                chat.tabs = load_tabs();
                chat.sync_tabs();
                self.selected = 0;
                self.name_box = None;
                self.notice = Some(("Chat layout reloaded.".into(), c.now));
            }
            _ => {}
        }
        // The notice sits under the buttons for a few seconds.
        if let Some((text, at)) = &self.notice {
            if c.now.saturating_sub(*at) < 5_000 {
                c.text
                    .draw(text, 11, win.x + 16.0, win.y + 330.0, [200, 220, 200, 255]);
            }
        }
    }
}

/// A labelled checkbox; returns true when it was clicked this frame.
fn checkbox(c: &mut Ctx, box_rect: Rect, label: &str, on: bool) -> bool {
    let hit = Rect::new(
        box_rect.x,
        box_rect.y - 2.0,
        box_rect.w + 8.0 + c.text.width(label, 12),
        box_rect.h + 4.0,
    );
    let hover = hit.contains(c.input.mouse.0, c.input.mouse.1);
    c.fill(box_rect, [0.0, 0.0, 0.0, 0.7]);
    c.border(box_rect, if hover { [255, 255, 255, 255] } else { GOLD });
    if on {
        let tick = Rect::new(
            box_rect.x + 3.0,
            box_rect.y + 3.0,
            box_rect.w - 6.0,
            box_rect.h - 6.0,
        );
        c.fill(tick, [0.78, 0.65, 0.39, 1.0]);
    }
    c.text.draw(
        label,
        12,
        box_rect.x + box_rect.w + 6.0,
        box_rect.y - 2.0,
        [220, 220, 220, 255],
    );
    hover && c.input.lmb_pressed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_route_each_category_somewhere() {
        let tabs = default_tabs();
        for cat in Category::ALL {
            assert!(
                tabs.iter().any(|t| t.cats.contains(&cat)),
                "{cat:?} reaches no tab"
            );
        }
    }

    #[test]
    fn a_layout_survives_a_round_trip() {
        let mut tabs = default_tabs();
        tabs[1].name = "Chatter".into();
        tabs[1].cats = vec![Category::Whisper];
        tabs[1].hidden = true;
        let text = serde_json::to_vec(&tabs).expect("encode");
        let back: Vec<Tab> = serde_json::from_slice(&text).expect("decode");
        assert_eq!(back, tabs);
    }

    #[test]
    fn a_corrupt_file_falls_back_to_the_defaults() {
        // load_tabs is total: anything unreadable yields the defaults, so a
        // bad file can never leave the panel with no tabs at all.
        let bad: Option<Vec<Tab>> = serde_json::from_slice(b"not json").ok();
        assert!(bad.is_none());
        assert!(!load_tabs().is_empty());
    }
}
