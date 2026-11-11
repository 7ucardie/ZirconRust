//! The chat log and chat bar (Zircon `ChatTab`, `ChatTextBox`).
//!
//! Zircon keeps a floating, scrollable tab panel above the chat bar: every
//! line carries a `MessageType` that decides its colour and which tabs show
//! it, the backlog holds 250 lines, and the default tab fades out when the
//! bar is closed. This module holds the same state and draws it with the
//! client's own chrome.

use mir_proto::ChatKind;

use crate::ui::{Ctx, Input, Rect, TextBox, GOLD};

/// Zircon `ChatTab.History`: lines kept per tab.
pub const MAX_LINES: usize = 250;
/// A line stays on the faded log this long after arriving.
const FADE_MS: u64 = 15_000;
const LINE_H: f32 = 16.0;
const TAB_H: f32 = 18.0;
const PANEL_H: f32 = 150.0;
const PANEL_W: f32 = 480.0;

/// What a line is, which decides its colour and the tabs that show it
/// (Zircon `MessageType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Category {
    Local,
    Shout,
    Global,
    Group,
    Guild,
    Whisper,
    System,
    /// Client-side notices (Zircon `MessageType.Hint`).
    Hint,
}

impl Category {
    pub fn from_kind(kind: ChatKind) -> Category {
        match kind {
            ChatKind::Normal => Category::Local,
            ChatKind::Shout => Category::Shout,
            ChatKind::Global => Category::Global,
            ChatKind::Group => Category::Group,
            ChatKind::Guild => Category::Guild,
            ChatKind::WhisperIn | ChatKind::WhisperOut => Category::Whisper,
            ChatKind::System => Category::System,
        }
    }

    /// Zircon `Config` defaults (`LocalTextForeColour` and friends).
    fn colour(self) -> [u8; 4] {
        match self {
            Category::Local => [255, 255, 255, 255],
            Category::Shout => [255, 255, 0, 255],
            Category::Global => [0, 255, 0, 255],
            Category::Group => [221, 160, 221, 255],
            Category::Guild => [255, 182, 193, 255],
            Category::Whisper => [0, 255, 255, 255],
            Category::System => [255, 0, 0, 255],
            Category::Hint => [250, 235, 215, 255],
        }
    }

    /// Every category, in the order the options window lists them.
    pub const ALL: [Category; 8] = [
        Category::Local,
        Category::Shout,
        Category::Global,
        Category::Group,
        Category::Guild,
        Category::Whisper,
        Category::System,
        Category::Hint,
    ];

    /// The name the options window puts beside a checkbox.
    pub fn label(self) -> &'static str {
        match self {
            Category::Local => "Local",
            Category::Shout => "Shout",
            Category::Global => "Global",
            Category::Group => "Group",
            Category::Guild => "Guild",
            Category::Whisper => "Whisper",
            Category::System => "System",
            Category::Hint => "Hint",
        }
    }

    /// Zircon gives system lines a light plate behind the text.
    fn back(self) -> Option<[f32; 4]> {
        match self {
            Category::System => Some([1.0, 1.0, 1.0, 200.0 / 255.0]),
            _ => None,
        }
    }
}

/// A tab and the categories it lets through (Zircon's per-tab checkboxes).
///
/// Zircon lets the player build these in `ChatOptionsDialog`, so they are
/// owned state rather than a constant table: the options window edits this
/// list and the panel redraws from it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Tab {
    pub name: String,
    pub cats: Vec<Category>,
    /// Zircon `FadeOutCheckBox`: old lines drop off the unfocused log.
    pub fade: bool,
    /// Zircon `HideTabCheckBox`: the tab button is not drawn.
    pub hidden: bool,
}

/// The six tabs a fresh install starts with.
pub fn default_tabs() -> Vec<Tab> {
    let tab = |name: &str, cats: Vec<Category>| Tab {
        name: name.to_string(),
        cats,
        fade: true,
        hidden: false,
    };
    vec![
        tab("All", Category::ALL.to_vec()),
        tab("Local", vec![Category::Local, Category::Shout]),
        tab("Group", vec![Category::Group]),
        tab("Guild", vec![Category::Guild]),
        tab("Whisper", vec![Category::Whisper]),
        tab(
            "System",
            vec![Category::System, Category::Hint, Category::Global],
        ),
    ]
}

struct Line {
    text: String,
    time: u64,
    cat: Category,
    /// Set when a caller picked its own colour (client notices).
    colour: Option<[u8; 4]>,
}

impl Line {
    fn colour(&self) -> [u8; 4] {
        self.colour.unwrap_or_else(|| self.cat.colour())
    }
}

/// The chat log, its tabs and the chat bar.
pub struct ChatPanel {
    lines: Vec<Line>,
    /// The bar is open and owns the keyboard.
    pub open: bool,
    pub bar: TextBox,
    /// The tabs, as the options window left them.
    pub tabs: Vec<Tab>,
    tab: usize,
    /// Lines scrolled up from the newest.
    scroll: usize,
    unread: Vec<bool>,
    /// Lines sent this session, walked with the arrow keys.
    history: Vec<String>,
    history_pos: Option<usize>,
    /// Zircon `ChatTextBox.LastPM`: the last `/name` typed.
    last_pm: Option<String>,
    /// When the player entered the world; Enter is ignored for a moment
    /// after it so a stray key cannot open the bar.
    pub entered_at: u64,
    /// Where the panel was drawn last frame, for hit tests.
    rect: Rect,
    hovered: bool,
    /// Zircon `ChatTab.HideChat`: faded away with nothing recent to show.
    hidden: bool,
    dev_seeded: bool,
}

impl Default for ChatPanel {
    fn default() -> ChatPanel {
        ChatPanel {
            lines: Vec::new(),
            open: false,
            bar: TextBox::new(Rect::new(0.0, 0.0, 10.0, 22.0), 200),
            tabs: default_tabs(),
            tab: 0,
            scroll: 0,
            unread: vec![false; default_tabs().len()],
            history: Vec::new(),
            history_pos: None,
            last_pm: None,
            entered_at: 0,
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            hovered: false,
            hidden: false,
            dev_seeded: false,
        }
    }
}

impl ChatPanel {
    pub fn contains_mouse(&self, mouse: (f32, f32)) -> bool {
        (!self.hidden && self.rect.contains(mouse.0, mouse.1))
            || (self.open && self.bar.rect.contains(mouse.0, mouse.1))
    }

    /// Zircon `ChatTab.HideChat` (upstream #284): with the bar closed and no
    /// line newer than `FADE_MS` on a fading tab, the panel is not a control.
    /// Clicks there walk and hovering does not bring it back.
    fn update_hidden(&mut self, now: u64) {
        self.hidden = !self.open
            && self.tabs.get(self.tab).is_none_or(|t| t.fade)
            && self
                .lines
                .last()
                .is_none_or(|l| now.saturating_sub(l.time) >= FADE_MS);
    }

    /// Keep the unread flags the same length as the tabs after the options
    /// window adds or removes one.
    pub fn sync_tabs(&mut self) {
        self.unread.resize(self.tabs.len(), false);
        self.tab = self.tab.min(self.tabs.len().saturating_sub(1));
        self.scroll = 0;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll = 0;
        self.unread = vec![false; self.tabs.len()];
        self.open = false;
        self.bar.set_text(String::new());
        self.bar.focused = false;
    }

    /// Add a line of a known kind (chat from the server).
    pub fn push(&mut self, text: String, time: u64, cat: Category) {
        self.add(Line {
            text,
            time,
            cat,
            colour: None,
        });
    }

    /// Add a client notice that brings its own colour.
    pub fn push_colored(&mut self, text: String, time: u64, colour: [u8; 4]) {
        self.add(Line {
            text,
            time,
            cat: Category::Hint,
            colour: Some(colour),
        });
    }

    fn add(&mut self, line: Line) {
        for (i, tab) in self.tabs.iter().enumerate() {
            if i != self.tab && tab.cats.contains(&line.cat) {
                self.unread[i] = true;
            }
        }
        self.lines.push(line);
        if self.lines.len() > MAX_LINES {
            self.lines.remove(0);
        }
        // New lines pull the view back to the bottom.
        self.scroll = 0;
    }

    /// Where the line being typed will go, shown beside the bar.
    fn target(&self) -> &'static str {
        let t = self.bar.text.as_str();
        if t.starts_with("!!") {
            "group"
        } else if t.starts_with("!~") {
            "guild"
        } else if t.starts_with("!@") {
            "global"
        } else if t.starts_with('!') {
            "shout"
        } else if t.starts_with('/') {
            "whisper"
        } else {
            "local"
        }
    }

    /// Keyboard and wheel for the bar and the log. Returns a line to send.
    ///
    /// While the bar is open it eats the keys so the world never sees them.
    pub fn keys(&mut self, input: &mut Input, now: u64, in_world: bool) -> Option<String> {
        self.seed_dev_lines(now);
        // The wheel scrolls the log while the pointer is over it.
        if self.hovered && input.wheel != 0.0 {
            let max = self.lines.len().saturating_sub(1);
            if input.wheel > 0.0 {
                self.scroll = (self.scroll + 3).min(max);
            } else {
                self.scroll = self.scroll.saturating_sub(3);
            }
            input.wheel = 0.0;
        }
        if !self.open {
            // Zircon opens the bar on Enter, and on a prefix key with that
            // prefix already typed (`/` reuses the last whisper target).
            let prefix = input.text.chars().find(|c| matches!(c, '/' | '!' | '@'));
            if let Some(p) = prefix {
                self.open = true;
                self.bar.set_text(match p {
                    '/' => match &self.last_pm {
                        Some(name) => format!("{name} "),
                        None => "/".into(),
                    },
                    other => other.to_string(),
                });
                input.text.clear();
            } else if input.enter {
                input.enter = false;
                if in_world && now > self.entered_at + 1000 {
                    self.open = true;
                }
            }
            if self.open {
                self.bar.focused = true;
                self.history_pos = None;
            }
            return None;
        }

        self.bar.focused = true;
        self.bar.edit(input);
        // Walk the lines sent this session.
        if input.up && !self.history.is_empty() {
            let pos = match self.history_pos {
                None => self.history.len() - 1,
                Some(p) => p.saturating_sub(1),
            };
            self.history_pos = Some(pos);
            self.bar.set_text(self.history[pos].clone());
        }
        if input.down {
            match self.history_pos {
                Some(p) if p + 1 < self.history.len() => {
                    self.history_pos = Some(p + 1);
                    self.bar.set_text(self.history[p + 1].clone());
                }
                Some(_) => {
                    self.history_pos = None;
                    self.bar.set_text(String::new());
                }
                None => {}
            }
        }
        let mut send = None;
        if input.enter {
            let text = std::mem::take(&mut self.bar.text);
            self.bar.set_text(String::new());
            self.open = false;
            self.bar.focused = false;
            self.history_pos = None;
            if !text.trim().is_empty() {
                // Zircon remembers the whisper target for the next `/`.
                if let Some(rest) = text.strip_prefix('/') {
                    if let Some(name) = rest.split_whitespace().next() {
                        self.last_pm = Some(format!("/{name}"));
                    }
                }
                if self.history.last().map(String::as_str) != Some(text.as_str()) {
                    self.history.push(text.clone());
                    if self.history.len() > 50 {
                        self.history.remove(0);
                    }
                }
                send = Some(text);
            }
        }
        if input.escape {
            self.open = false;
            self.bar.focused = false;
            self.bar.set_text(String::new());
            self.history_pos = None;
        }
        // The world must not see anything typed into the bar.
        input.text.clear();
        input.backspace = false;
        input.delete = false;
        input.enter = false;
        input.escape = false;
        input.tab = false;
        input.up = false;
        input.down = false;
        input.left = false;
        input.right = false;
        input.digit = None;
        input.fkey = None;
        send
    }

    /// `ZIRCON_DEV_CHAT_LINES=n` fills the log with n lines of each kind.
    fn seed_dev_lines(&mut self, now: u64) {
        if self.dev_seeded {
            return;
        }
        let Ok(count) = std::env::var("ZIRCON_DEV_CHAT_LINES") else {
            return;
        };
        self.dev_seeded = true;
        let Ok(count) = count.parse::<usize>() else {
            return;
        };
        let kinds = [
            (Category::Local, "Tarnhelm: anyone selling a bronze helmet?"),
            (
                Category::Shout,
                "(!)Bodil: recruiting for Zuma temple, whisper me",
            ),
            (
                Category::Group,
                "Rieka: pulling the next room, hold here a second",
            ),
            (
                Category::Guild,
                "Sanhu: guild war starts at eight, be online",
            ),
            (
                Category::Whisper,
                "Isolde=> trade at the smith in five minutes",
            ),
            (
                Category::System,
                "A long system notice that has to wrap across the panel to prove the word wrapping works.",
            ),
            (Category::Hint, "You picked up 120 gold."),
        ];
        for i in 0..count {
            let (cat, text) = kinds[i % kinds.len()];
            self.push(text.to_string(), now, cat);
        }
    }

    /// Draw the log and, when it is open, the bar. `width`/`height` are the
    /// logical screen size.
    pub fn draw(&mut self, c: &mut Ctx, width: i32, height: i32, now: u64) {
        let w = (width as f32 - 24.0).min(PANEL_W);
        let bar = Rect::new(12.0, height as f32 - 176.0, w, 22.0);
        let panel = Rect::new(12.0, bar.y - PANEL_H - 4.0, w, PANEL_H);
        self.rect = panel;
        self.update_hidden(now);
        self.hovered = !self.hidden && panel.contains(c.input.mouse.0, c.input.mouse.1);
        // Zircon's default tab fades out: with the bar closed and the mouse
        // away, only recent lines show and the chrome stays hidden.
        let faded = !self.open && !self.hovered;
        if !faded {
            c.fill(panel, [0.0, 0.0, 0.0, 100.0 / 255.0]);
            c.border(panel, GOLD);
            self.draw_tabs(c, panel);
        }

        let text_w = panel.w - 16.0;
        let rows = ((panel.h - TAB_H - 8.0) / LINE_H) as usize;
        // The open tab decides which categories show, and whether old
        // lines fade off the unfocused log.
        let cats = self
            .tabs
            .get(self.tab)
            .map(|t| t.cats.clone())
            .unwrap_or_default();
        let faded = faded && self.tabs.get(self.tab).is_none_or(|t| t.fade);
        // Wrap newest-first so scrolling counts wrapped rows, then flip.
        let mut wrapped: Vec<(String, [u8; 4], Option<[f32; 4]>)> = Vec::new();
        for line in self
            .lines
            .iter()
            .filter(|l| cats.contains(&l.cat))
            .filter(|l| !faded || now.saturating_sub(l.time) < FADE_MS)
            .rev()
        {
            for part in crate::windows::wrap_text(c, &line.text, text_w, 12)
                .into_iter()
                .rev()
            {
                wrapped.push((part, line.colour(), line.cat.back()));
            }
            if wrapped.len() > rows + self.scroll {
                break;
            }
        }
        let shown: Vec<_> = wrapped.into_iter().skip(self.scroll).take(rows).collect();
        let mut y = panel.y + panel.h - LINE_H - 4.0;
        for (part, colour, back) in shown {
            if let Some(bg) = back {
                c.fill(Rect::new(panel.x + 6.0, y - 1.0, text_w + 4.0, LINE_H), bg);
            }
            c.text.draw(&part, 12, panel.x + 8.0, y, colour);
            y -= LINE_H;
        }
        if !faded && self.lines.len() > rows {
            self.draw_scrollbar(c, panel, rows);
        }

        if self.open {
            self.bar.rect = bar;
            self.bar.focused = true;
            self.bar.update(c);
            // Where this line will go (Zircon's prefixes).
            let target = self.target();
            c.text.draw(
                &format!("[{target}]"),
                11,
                bar.x + bar.w + 6.0,
                bar.y + 4.0,
                [200, 200, 160, 255],
            );
        }
    }

    fn draw_tabs(&mut self, c: &mut Ctx, panel: Rect) {
        let mut x = panel.x + 4.0;
        for i in 0..self.tabs.len() {
            if self.tabs[i].hidden {
                continue;
            }
            let name = self.tabs[i].name.clone();
            let w = c.text.width(&name, 11) + 12.0;
            let r = Rect::new(x, panel.y + 2.0, w, TAB_H - 2.0);
            let active = i == self.tab;
            let hover = r.contains(c.input.mouse.0, c.input.mouse.1);
            if active {
                c.fill(r, [0.25, 0.25, 0.35, 0.9]);
            } else if hover {
                c.fill(r, [0.2, 0.2, 0.25, 0.7]);
            }
            let colour = if active {
                [255, 255, 200, 255]
            } else if self.unread[i] {
                [255, 220, 120, 255]
            } else {
                [170, 170, 170, 255]
            };
            c.text.draw(&name, 11, r.x + 6.0, r.y + 1.0, colour);
            if hover && c.input.lmb_pressed {
                self.tab = i;
                self.scroll = 0;
                self.unread[i] = false;
            }
            x += w + 2.0;
        }
        self.unread[self.tab] = false;
    }

    fn draw_scrollbar(&self, c: &mut Ctx, panel: Rect, rows: usize) {
        let track = Rect::new(
            panel.x + panel.w - 6.0,
            panel.y + TAB_H,
            4.0,
            panel.h - TAB_H - 4.0,
        );
        c.fill(track, [1.0, 1.0, 1.0, 0.08]);
        let total = self.lines.len().max(1);
        let frac = (rows as f32 / total as f32).clamp(0.08, 1.0);
        let h = track.h * frac;
        let max_scroll = total.saturating_sub(rows).max(1) as f32;
        let pos = 1.0 - (self.scroll as f32 / max_scroll).clamp(0.0, 1.0);
        let y = track.y + (track.h - h) * pos;
        c.fill(Rect::new(track.x, y, track.w, h), [1.0, 1.0, 1.0, 0.35]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> ChatPanel {
        // Seeded already, so the dev switch cannot add lines under a test.
        ChatPanel {
            dev_seeded: true,
            ..ChatPanel::default()
        }
    }

    fn typed(text: &str) -> Input {
        Input {
            text: text.to_string(),
            ..Input::default()
        }
    }

    #[test]
    fn panel_and_open_chat_bar_block_ground_item_drop() {
        let mut p = panel();
        p.rect = Rect::new(12.0, 500.0, 480.0, 150.0);
        p.bar.rect = Rect::new(12.0, 654.0, 480.0, 22.0);
        assert!(p.contains_mouse((30.0, 520.0)));
        assert!(!p.contains_mouse((30.0, 660.0)));
        p.open = true;
        assert!(p.contains_mouse((30.0, 660.0)));
        assert!(!p.contains_mouse((600.0, 520.0)));
    }

    #[test]
    fn a_hidden_panel_lets_clicks_through_to_the_map() {
        let mut p = panel();
        p.rect = Rect::new(12.0, 500.0, 480.0, 150.0);
        p.push("hello".into(), 1_000, Category::Local);
        p.update_hidden(2_000);
        assert!(p.contains_mouse((30.0, 520.0)));
        p.update_hidden(1_000 + FADE_MS);
        assert!(!p.contains_mouse((30.0, 520.0)));
        p.open = true;
        p.update_hidden(1_000 + FADE_MS);
        assert!(p.contains_mouse((30.0, 520.0)));
    }

    #[test]
    fn backlog_is_capped_like_zircon() {
        let mut p = panel();
        for i in 0..MAX_LINES + 20 {
            p.push(format!("line {i}"), 0, Category::Local);
        }
        assert_eq!(p.lines.len(), MAX_LINES);
        // The oldest lines fall off the front.
        assert_eq!(p.lines[0].text, "line 20");
    }

    #[test]
    fn tabs_filter_by_category() {
        let mut p = panel();
        p.push("talk".into(), 0, Category::Local);
        p.push("team".into(), 0, Category::Group);
        p.push("notice".into(), 0, Category::System);
        fn shown(p: &ChatPanel, tab: usize) -> Vec<&str> {
            p.lines
                .iter()
                .filter(|l| p.tabs[tab].cats.contains(&l.cat))
                .map(|l| l.text.as_str())
                .collect()
        }
        assert_eq!(shown(&p, 0), vec!["talk", "team", "notice"]);
        assert_eq!(shown(&p, 1), vec!["talk"]);
        assert_eq!(shown(&p, 2), vec!["team"]);
        assert_eq!(shown(&p, 5), vec!["notice"]);
    }

    #[test]
    fn other_tabs_mark_unread_until_opened() {
        let mut p = panel();
        p.push("team".into(), 0, Category::Group);
        assert!(p.unread[2], "the group tab should be marked");
        assert!(!p.unread[0], "the open tab is never marked");
        p.tab = 2;
        p.unread[2] = false;
        p.push("more".into(), 0, Category::Group);
        assert!(!p.unread[2]);
    }

    #[test]
    fn a_line_is_sent_and_kept_for_the_history() {
        let mut p = panel();
        p.open = true;
        let mut input = typed("hello");
        assert_eq!(p.keys(&mut input, 0, true), None);
        let mut enter = Input {
            enter: true,
            ..Input::default()
        };
        assert_eq!(p.keys(&mut enter, 0, true).as_deref(), Some("hello"));
        assert!(!p.open, "sending closes the bar");
        assert_eq!(p.history, vec!["hello".to_string()]);
        // Blank lines are not sent and not remembered.
        p.open = true;
        let mut blank = Input {
            enter: true,
            ..Input::default()
        };
        assert_eq!(p.keys(&mut blank, 0, true), None);
        assert_eq!(p.history.len(), 1);
    }

    #[test]
    fn arrows_walk_the_history() {
        let mut p = panel();
        p.history = vec!["first".into(), "second".into()];
        p.open = true;
        let up = || Input {
            up: true,
            ..Input::default()
        };
        p.keys(&mut up(), 0, true);
        assert_eq!(p.bar.text, "second");
        p.keys(&mut up(), 0, true);
        assert_eq!(p.bar.text, "first");
        // Up sticks at the oldest line.
        p.keys(&mut up(), 0, true);
        assert_eq!(p.bar.text, "first");
        let down = || Input {
            down: true,
            ..Input::default()
        };
        p.keys(&mut down(), 0, true);
        assert_eq!(p.bar.text, "second");
        p.keys(&mut down(), 0, true);
        assert_eq!(p.bar.text, "", "past the newest the bar clears");
    }

    #[test]
    fn left_right_and_delete_edit_chat_at_the_caret() {
        let mut p = panel();
        p.open = true;
        p.bar.focused = true;
        p.keys(&mut typed("ac"), 0, true);
        p.keys(
            &mut Input {
                left: true,
                ..Input::default()
            },
            0,
            true,
        );
        p.keys(&mut typed("b"), 0, true);
        assert_eq!(p.bar.text, "abc");
        p.keys(
            &mut Input {
                delete: true,
                ..Input::default()
            },
            0,
            true,
        );
        assert_eq!(p.bar.text, "ab");
    }

    #[test]
    fn a_prefix_key_opens_the_bar_with_that_prefix() {
        let mut p = panel();
        let mut input = typed("!");
        p.keys(&mut input, 5000, true);
        assert!(p.open);
        assert_eq!(p.bar.text, "!");
        assert!(input.text.is_empty(), "the world must not see the key");
    }

    #[test]
    fn slash_reuses_the_last_whisper_target() {
        let mut p = panel();
        p.open = true;
        p.bar.text = "/Isolde hello".into();
        let mut enter = Input {
            enter: true,
            ..Input::default()
        };
        p.keys(&mut enter, 0, true);
        assert_eq!(p.last_pm.as_deref(), Some("/Isolde"));
        let mut slash = typed("/");
        p.keys(&mut slash, 5000, true);
        assert_eq!(p.bar.text, "/Isolde ");
    }

    #[test]
    fn enter_is_ignored_for_a_moment_after_entering_the_world() {
        let mut p = panel();
        p.entered_at = 1000;
        let mut early = Input {
            enter: true,
            ..Input::default()
        };
        p.keys(&mut early, 1500, true);
        assert!(!p.open, "a stray Enter must not open the bar");
        let mut later = Input {
            enter: true,
            ..Input::default()
        };
        p.keys(&mut later, 2500, true);
        assert!(p.open);
    }

    #[test]
    fn the_prefix_hint_names_the_target() {
        let mut p = panel();
        for (text, want) in [
            ("", "local"),
            ("hello", "local"),
            ("!shout", "shout"),
            ("!!team", "group"),
            ("!~guild", "guild"),
            ("!@world", "global"),
            ("/Bob hi", "whisper"),
        ] {
            p.bar.text = text.into();
            assert_eq!(p.target(), want, "for {text:?}");
        }
    }
}
