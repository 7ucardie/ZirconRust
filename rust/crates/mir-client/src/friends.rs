//! The friends window (Zircon `CommunicationDialog`'s Friend and Block
//! tabs; its two mail tabs are our separate mail window).
//!
//! Zircon shows the friends sorted by online state, each with a coloured
//! `(State)` suffix, a combo box picking the state we advertise and another
//! filtering the list by state. Adding and removing take a typed name and
//! the selected row. Blocking is per account, so a blocked name silences
//! every character that person owns.

use mir_proto::{online_state, BlockSummary, ClientMessage, FriendSummary};

use crate::ui::{Button, Ctx, Rect, TextBox};
use crate::windows::WindowLayout;

/// Which tab of the window is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Friends,
    Blocked,
}

/// Zircon colours each state in `FriendRow.UpdateStateLabel`.
pub fn state_colour(state: u8) -> [u8; 4] {
    match state {
        online_state::ONLINE => [50, 205, 50, 255],
        online_state::AWAY => [255, 165, 0, 255],
        online_state::BUSY => [255, 0, 0, 255],
        _ => [128, 128, 128, 255],
    }
}

/// The rows the friend tab shows: everything, or one state, matching
/// Zircon's "View status" box (whose first entry is All).
pub fn visible_friends(friends: &[FriendSummary], filter: Option<u8>) -> Vec<&FriendSummary> {
    friends
        .iter()
        .filter(|f| filter.is_none_or(|want| f.state == want))
        .collect()
}

/// Everything the window keeps between frames.
pub struct FriendsWindow {
    pub open: bool,
    pub tab: Tab,
    /// Selected row of the active tab, by entry index.
    pub selected: Option<u32>,
    /// `None` is Zircon's "All".
    pub filter: Option<u8>,
    name: Option<TextBox>,
    buttons: Vec<Button>,
    pub scroll: f32,
    /// Whisper was pressed: the caller opens the chat bar on this name,
    /// the way Zircon pre-fills it rather than sending an empty line.
    pub whisper_to: Option<String>,
    layout: WindowLayout,
}

impl Default for FriendsWindow {
    fn default() -> FriendsWindow {
        FriendsWindow {
            // ZIRCON_OPEN=friends opens it on entry for screenshots;
            // `blocked` opens it on the second tab.
            open: opened_by_env().is_some(),
            tab: opened_by_env().unwrap_or(Tab::Friends),
            selected: None,
            filter: None,
            name: None,
            buttons: Vec::new(),
            scroll: 0.0,
            whisper_to: None,
            layout: WindowLayout::default(),
        }
    }
}

/// `ZIRCON_OPEN=friends` (or `blocked`) opens the window on that tab.
fn opened_by_env() -> Option<Tab> {
    std::env::var("ZIRCON_OPEN")
        .unwrap_or_default()
        .split(',')
        .find_map(|n| match n.trim() {
            "friends" => Some(Tab::Friends),
            "blocked" => Some(Tab::Blocked),
            _ => None,
        })
}

impl FriendsWindow {
    /// True while the name box has the keyboard, so movement keys do not
    /// leak into the world.
    pub fn typing(&self) -> bool {
        self.open && self.name.as_ref().is_some_and(|b| b.focused)
    }

    /// Draw the window. Returns true if the mouse is over it.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        c: &mut Ctx,
        friends: &[FriendSummary],
        blocks: &[BlockSummary],
        state: u8,
        width: i32,
        height: i32,
        out: &mut Vec<ClientMessage>,
    ) -> bool {
        if !self.open {
            return false;
        }
        let mouse = c.input.mouse;
        let win = self.layout.place(
            "friends",
            Rect::new(width as f32 - 320.0 - 10.0, 30.0, 320.0, 400.0),
            c.input,
            (width, height),
        );
        let over = win.contains(mouse.0, mouse.1) || self.layout.is_dragging();
        if over {
            self.scroll = (self.scroll - c.input.wheel * 18.0).max(0.0);
        }
        let closed = c.window(win, "Friends", true) && !self.layout.is_dragging();

        self.draw_tabs(c, win);
        // The state we advertise, as a row of small buttons rather than
        // Zircon's combo box: the same four choices, one click each.
        c.text.draw(
            "Status",
            11,
            win.x + 16.0,
            win.y + 56.0,
            [198, 166, 99, 255],
        );
        let mut x = win.x + 64.0;
        for want in online_state::ALL {
            let label = online_state::name(want);
            let w = c.text.width(label, 11) + 10.0;
            let r = Rect::new(x, win.y + 54.0, w, 16.0);
            let hover = r.contains(mouse.0, mouse.1);
            if want == state {
                c.fill(r, [0.3, 0.3, 0.4, 0.9]);
            } else if hover {
                c.fill(r, [0.2, 0.2, 0.25, 0.7]);
            }
            let colour = if want == state {
                state_colour(want)
            } else {
                [150, 150, 150, 255]
            };
            c.text.draw(label, 11, r.x + 5.0, r.y + 1.0, colour);
            if hover && c.input.lmb_pressed && want != state {
                out.push(ClientMessage::ChangeOnlineState { state: want });
            }
            x += w + 2.0;
        }

        match self.tab {
            Tab::Friends => self.draw_friends(c, win, friends, out),
            Tab::Blocked => self.draw_blocks(c, win, blocks),
        }
        self.draw_footer(c, win, out);

        if closed {
            self.open = false;
        }
        self.layout.end_frame(c.input);
        over
    }

    fn draw_tabs(&mut self, c: &mut Ctx, win: Rect) {
        let mouse = c.input.mouse;
        let mut x = win.x + 14.0;
        for (tab, label) in [(Tab::Friends, "Friends"), (Tab::Blocked, "Blocked")] {
            let w = c.text.width(label, 12) + 16.0;
            let r = Rect::new(x, win.y + 30.0, w, 18.0);
            let active = self.tab == tab;
            let hover = r.contains(mouse.0, mouse.1);
            if active {
                c.fill(r, [0.3, 0.3, 0.4, 0.9]);
            } else if hover {
                c.fill(r, [0.2, 0.2, 0.25, 0.7]);
            }
            c.text.draw(
                label,
                12,
                r.x + 8.0,
                r.y + 2.0,
                if active {
                    [255, 255, 200, 255]
                } else {
                    [170, 170, 170, 255]
                },
            );
            if hover && c.input.lmb_pressed && !active {
                self.tab = tab;
                self.selected = None;
                self.scroll = 0.0;
            }
            x += w + 4.0;
        }
    }

    fn draw_friends(
        &mut self,
        c: &mut Ctx,
        win: Rect,
        friends: &[FriendSummary],
        out: &mut Vec<ClientMessage>,
    ) {
        let mouse = c.input.mouse;
        // "View status": All plus each state, Zircon's second combo box.
        c.text
            .draw("Show", 11, win.x + 16.0, win.y + 76.0, [198, 166, 99, 255]);
        let mut x = win.x + 64.0;
        let choices: Vec<(Option<u8>, &str)> = std::iter::once((None, "All"))
            .chain(online_state::ALL.map(|s| (Some(s), online_state::name(s))))
            .collect();
        for (want, label) in choices {
            let w = c.text.width(label, 11) + 8.0;
            let r = Rect::new(x, win.y + 74.0, w, 16.0);
            let hover = r.contains(mouse.0, mouse.1);
            if self.filter == want {
                c.fill(r, [0.3, 0.3, 0.4, 0.9]);
            } else if hover {
                c.fill(r, [0.2, 0.2, 0.25, 0.7]);
            }
            c.text.draw(
                label,
                11,
                r.x + 4.0,
                r.y + 1.0,
                if self.filter == want {
                    [255, 255, 200, 255]
                } else {
                    [150, 150, 150, 255]
                },
            );
            if hover && c.input.lmb_pressed {
                self.filter = want;
                self.scroll = 0.0;
            }
            x += w + 2.0;
        }

        let rows = visible_friends(friends, self.filter);
        if rows.is_empty() {
            c.text.draw(
                "Nobody here. Add a friend by name below.",
                11,
                win.x + 16.0,
                win.y + 106.0,
                [180, 180, 180, 255],
            );
            return;
        }
        let mut y = win.y + 102.0;
        let bottom = win.y + win.h - 3.0 - 42.0 - 34.0;
        for f in rows.iter().skip((self.scroll / 18.0) as usize) {
            if y + 18.0 > bottom {
                break;
            }
            let row = Rect::new(win.x + 12.0, y - 2.0, win.w - 24.0, 18.0);
            let hover = row.contains(mouse.0, mouse.1);
            if self.selected == Some(f.index) {
                c.fill(row, [0.5, 0.25, 0.25, 0.9]);
            } else if hover {
                c.fill(row, [0.25, 0.12, 0.12, 0.8]);
            }
            let name_colour = if self.selected == Some(f.index) {
                [255, 255, 255, 255]
            } else {
                [198, 166, 99, 255]
            };
            c.text.draw(&f.name, 12, row.x + 6.0, y, name_colour);
            let state = format!("({})", online_state::name(f.state));
            let sw = c.text.width(&state, 11);
            c.text.draw(
                &state,
                11,
                row.x + row.w - sw - 6.0,
                y + 1.0,
                state_colour(f.state),
            );
            if hover && c.input.lmb_pressed {
                self.selected = Some(f.index);
            }
            y += 18.0;
        }
        // Whisper and Invite act on the selected friend, as Zircon's
        // right-click menu does.
        if let Some(sel) = self.selected {
            if let Some(f) = friends.iter().find(|f| f.index == sel) {
                self.ensure_buttons(c);
                let by = win.y + win.h - 3.0 - 42.0 - 30.0;
                let online = f.state != online_state::OFFLINE;
                let b = &mut self.buttons[2];
                b.pos = (win.x + 16.0, by);
                b.enabled = online;
                if b.update(c) && b.enabled {
                    self.whisper_to = Some(f.name.clone());
                }
                let b = &mut self.buttons[3];
                b.pos = (win.x + 92.0, by);
                b.enabled = online;
                if b.update(c) && b.enabled {
                    out.push(ClientMessage::GroupInvite {
                        name: f.name.clone(),
                    });
                }
            }
        }
    }

    fn draw_blocks(&mut self, c: &mut Ctx, win: Rect, blocks: &[BlockSummary]) {
        let mouse = c.input.mouse;
        c.text.draw(
            "Blocked people cannot whisper, invite or trade with you.",
            11,
            win.x + 16.0,
            win.y + 76.0,
            [180, 180, 180, 255],
        );
        if blocks.is_empty() {
            c.text.draw(
                "Nobody is blocked.",
                11,
                win.x + 16.0,
                win.y + 106.0,
                [180, 180, 180, 255],
            );
            return;
        }
        let mut y = win.y + 102.0;
        let bottom = win.y + win.h - 3.0 - 42.0 - 34.0;
        for b in blocks.iter().skip((self.scroll / 18.0) as usize) {
            if y + 18.0 > bottom {
                break;
            }
            let row = Rect::new(win.x + 12.0, y - 2.0, win.w - 24.0, 18.0);
            let hover = row.contains(mouse.0, mouse.1);
            if self.selected == Some(b.index) {
                c.fill(row, [0.5, 0.25, 0.25, 0.9]);
            } else if hover {
                c.fill(row, [0.25, 0.12, 0.12, 0.8]);
            }
            c.text.draw(
                &b.name,
                12,
                row.x + 6.0,
                y,
                if self.selected == Some(b.index) {
                    [255, 255, 255, 255]
                } else {
                    [198, 166, 99, 255]
                },
            );
            if hover && c.input.lmb_pressed {
                self.selected = Some(b.index);
            }
            y += 18.0;
        }
    }

    fn ensure_buttons(&mut self, _c: &mut Ctx) {
        if self.buttons.is_empty() {
            self.buttons = vec![
                Button::default_style(0.0, 0.0, 60.0, "Add"),
                Button::default_style(0.0, 0.0, 70.0, "Remove"),
                Button::default_style(0.0, 0.0, 70.0, "Whisper"),
                Button::default_style(0.0, 0.0, 60.0, "Invite"),
            ];
        }
    }

    fn draw_footer(&mut self, c: &mut Ctx, win: Rect, out: &mut Vec<ClientMessage>) {
        self.ensure_buttons(c);
        let fy = win.y + win.h - 3.0 - 42.0 + 8.0;
        let name_box = self
            .name
            .get_or_insert_with(|| TextBox::new(Rect::new(0.0, 0.0, 130.0, 22.0), 15));
        name_box.rect = Rect::new(win.x + 16.0, fy + 2.0, 130.0, 22.0);
        let submitted = name_box.update(c);
        let typed = name_box.text.trim().to_string();

        let blocked_tab = self.tab == Tab::Blocked;
        let b = &mut self.buttons[0];
        b.pos = (win.x + 154.0, fy);
        b.enabled = !typed.is_empty();
        if (b.update(c) || submitted) && b.enabled {
            out.push(if blocked_tab {
                ClientMessage::BlockAdd { name: typed }
            } else {
                ClientMessage::FriendAdd { name: typed }
            });
            if let Some(nb) = &mut self.name {
                nb.text.clear();
            }
        }
        let selected = self.selected;
        let b = &mut self.buttons[1];
        b.pos = (win.x + 220.0, fy);
        b.enabled = selected.is_some();
        if b.update(c) && b.enabled {
            if let Some(index) = selected {
                out.push(if blocked_tab {
                    ClientMessage::BlockRemove { index }
                } else {
                    ClientMessage::FriendRemove { index }
                });
            }
            self.selected = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn friend(index: u32, name: &str, state: u8) -> FriendSummary {
        FriendSummary {
            index,
            name: name.into(),
            state,
        }
    }

    #[test]
    fn states_use_zircon_colours() {
        assert_eq!(state_colour(online_state::ONLINE), [50, 205, 50, 255]);
        assert_eq!(state_colour(online_state::AWAY), [255, 165, 0, 255]);
        assert_eq!(state_colour(online_state::BUSY), [255, 0, 0, 255]);
        assert_eq!(state_colour(online_state::OFFLINE), [128, 128, 128, 255]);
    }

    #[test]
    fn the_view_filter_picks_one_state() {
        let list = vec![
            friend(1, "Ann", online_state::ONLINE),
            friend(2, "Bob", online_state::OFFLINE),
            friend(3, "Cid", online_state::ONLINE),
        ];
        // "All" keeps every row; a state keeps only its own.
        assert_eq!(visible_friends(&list, None).len(), 3);
        let online = visible_friends(&list, Some(online_state::ONLINE));
        assert_eq!(online.len(), 2);
        assert_eq!(online[0].name, "Ann");
        assert_eq!(visible_friends(&list, Some(online_state::BUSY)).len(), 0);
    }
}
