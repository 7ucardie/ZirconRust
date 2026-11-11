//! The marketplace window (Zircon `MarketPlaceDialog`): a Search tab that
//! browses what everyone has listed and buys, and a Consign tab that lists
//! items out of the bag and takes them back.
//!
//! Zircon's third tab is the Store, the game-gold cash shop
//! (`MarketPlaceStoreBuy`). It is a different system from the auction house
//! (fixed stock out of `StoreInfo`, paid for in game gold or hunt gold
//! rather than the gold in the bag) but it shares this window, as it does
//! in Zircon.

use mir_proto::{
    item_type, ClientMessage, ItemInstance, MarketListing, MarketSort, StoreEntry, StoreSort,
    MARKET_PAGE,
};

use crate::items::ItemCatalog;
use crate::ui::{Button, Ctx, Rect, TextBox};
use crate::windows::WindowLayout;

/// The item-type filters Zircon offers, in its own order. `None` is "any".
pub const TYPE_FILTERS: [(Option<u8>, &str); 9] = [
    (None, "Any"),
    (Some(item_type::WEAPON), "Weapon"),
    (Some(item_type::ARMOUR), "Armour"),
    (Some(item_type::HELMET), "Helmet"),
    (Some(item_type::NECKLACE), "Necklace"),
    (Some(item_type::RING), "Ring"),
    (Some(item_type::BRACELET), "Bracelet"),
    (Some(item_type::CONSUMABLE), "Potion"),
    (Some(item_type::BOOK), "Book"),
];

/// How many store rows fit on a page. Zircon shows nine.
pub const STORE_PAGE: usize = 9;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Search,
    Consign,
    Store,
}

#[derive(Default)]
pub struct MarketState {
    pub open: bool,
    pub tab: Tab,
    /// Results of the last search, and how many it matched in total.
    pub results: Vec<MarketListing>,
    pub total: u32,
    pub page: u32,
    pub sort: usize,
    pub filter: usize,
    /// What this account has listed.
    pub consignments: Vec<MarketListing>,
    /// Row picked on the current tab.
    pub selected: Option<u32>,
    /// Bag slot picked to list.
    pub consign_slot: Option<u8>,
    /// The whole store, as the server sent it on entry.
    pub stock: Vec<StoreEntry>,
    /// Every tag in the stock, sorted, with "All" as index 0. Zircon builds
    /// its drop-down the same way, out of the data rather than a fixed list.
    pub store_tags: Vec<String>,
    pub store_tag: usize,
    pub store_sort: usize,
    pub store_page: u32,
    pub store_selected: Option<u32>,
    /// Pay with hunt gold instead of game gold.
    pub hunt_gold: bool,
    /// A tag to select as soon as the stock arrives (`ZIRCON_STORE_TAG`).
    pub wanted_tag: Option<String>,
    /// Search name, buy count, consign price, consign count, consign note.
    boxes: Vec<TextBox>,
    buttons: Vec<Button>,
    /// A search is owed to the server (tab opened, filter changed, paged).
    dirty: bool,
    layout: WindowLayout,
}

/// Indices into `boxes`.
const B_NAME: usize = 0;
const B_BUY_COUNT: usize = 1;
const B_PRICE: usize = 2;
const B_COUNT: usize = 3;
const B_NOTE: usize = 4;
const B_STORE_NAME: usize = 5;
const B_STORE_COUNT: usize = 6;

impl MarketState {
    pub fn typing(&self) -> bool {
        self.open && self.boxes.iter().any(|b| b.focused)
    }

    /// Ask for a fresh page next frame (after a filter or tab change).
    pub fn refresh(&mut self) {
        self.dirty = true;
    }

    pub fn sort(&self) -> MarketSort {
        MarketSort::ALL[self.sort.min(MarketSort::ALL.len() - 1)].0
    }

    fn filter_type(&self) -> Option<u8> {
        TYPE_FILTERS[self.filter.min(TYPE_FILTERS.len() - 1)].0
    }

    fn search_msg(&self) -> ClientMessage {
        ClientMessage::MarketSearch {
            name: self
                .boxes
                .get(B_NAME)
                .map(|b| b.text.clone())
                .unwrap_or_default(),
            item_type: self.filter_type(),
            sort: self.sort(),
            page: self.page,
        }
    }

    /// Rebuild the tag list from freshly arrived stock. Zircon splits every
    /// `Filter`, collects the distinct tags and sorts them, with "All" first.
    pub fn set_stock(&mut self, stock: Vec<StoreEntry>) {
        let mut tags: Vec<String> = Vec::new();
        for e in &stock {
            for f in &e.filters {
                if !tags.iter().any(|t| t.eq_ignore_ascii_case(f)) {
                    tags.push(f.clone());
                }
            }
        }
        tags.sort();
        tags.insert(0, "All".into());
        self.stock = stock;
        self.store_tag = self
            .wanted_tag
            .as_deref()
            .and_then(|w| tags.iter().position(|t| t.eq_ignore_ascii_case(w)))
            .unwrap_or(0);
        self.store_tags = tags;
        self.store_page = 0;
    }

    /// The tag the filter is on, or `None` while it is on "All".
    pub fn store_tag(&self) -> Option<&str> {
        match self.store_tags.get(self.store_tag) {
            Some(t) if self.store_tag > 0 => Some(t.as_str()),
            _ => None,
        }
    }

    pub fn store_sort(&self) -> StoreSort {
        StoreSort::ALL[self.store_sort.min(StoreSort::ALL.len() - 1)].0
    }

    /// The stock the current filter, name box and sort select, in order.
    pub fn store_results(&self, catalog: &ItemCatalog) -> Vec<StoreEntry> {
        let name = self
            .boxes
            .get(B_STORE_NAME)
            .map(|b| b.text.clone())
            .unwrap_or_default();
        let tag = self.store_tag();
        let mut rows: Vec<StoreEntry> = self
            .stock
            .iter()
            .filter(|e| store_matches(e, &catalog.name(e.item), tag, &name))
            .cloned()
            .collect();
        match self.store_sort() {
            StoreSort::Alphabetical => {
                rows.sort_by_key(|e| catalog.name(e.item));
            }
            StoreSort::HighestPrice => rows.sort_by_key(|a| std::cmp::Reverse(a.price)),
            StoreSort::LowestPrice => rows.sort_by_key(|a| a.price),
        }
        rows
    }

    /// How many pages the store spans at the current filter (at least one).
    pub fn store_pages(&self, matched: usize) -> u32 {
        (matched as u32).div_ceil(STORE_PAGE as u32).max(1)
    }

    /// How many pages the last search spans (at least one).
    pub fn pages(&self) -> u32 {
        self.total.div_ceil(MARKET_PAGE as u32).max(1)
    }

    /// Draw the window. Returns true while the mouse is over it.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        c: &mut Ctx,
        catalog: &ItemCatalog,
        inventory: &[Option<ItemInstance>],
        gold: u64,
        currencies: &[mir_proto::CurrencySummary],
        width: i32,
        height: i32,
        out: &mut Vec<ClientMessage>,
    ) -> bool {
        if !self.open {
            return false;
        }
        if self.boxes.is_empty() {
            self.boxes = vec![
                TextBox::new(Rect::new(0.0, 0.0, 150.0, 22.0), 40),
                TextBox::new(Rect::new(0.0, 0.0, 60.0, 22.0), 9),
                TextBox::new(Rect::new(0.0, 0.0, 100.0, 22.0), 12),
                TextBox::new(Rect::new(0.0, 0.0, 60.0, 22.0), 9),
                TextBox::new(Rect::new(0.0, 0.0, 300.0, 22.0), 150),
                TextBox::new(Rect::new(0.0, 0.0, 150.0, 22.0), 40),
                TextBox::new(Rect::new(0.0, 0.0, 60.0, 22.0), 9),
            ];
        }
        if self.buttons.is_empty() {
            self.buttons = vec![
                Button::default_style(0.0, 0.0, 80.0, "Search"),
                Button::default_style(0.0, 0.0, 70.0, "Buy"),
                Button::default_style(0.0, 0.0, 60.0, "Prev"),
                Button::default_style(0.0, 0.0, 60.0, "Next"),
                Button::default_style(0.0, 0.0, 70.0, "List"),
                Button::default_style(0.0, 0.0, 80.0, "Cancel"),
                Button::default_style(0.0, 0.0, 70.0, "Buy"),
                Button::default_style(0.0, 0.0, 60.0, "Prev"),
                Button::default_style(0.0, 0.0, 60.0, "Next"),
            ];
        }
        let win = self.layout.place(
            "market",
            Rect::new(width as f32 / 2.0 - 320.0, 40.0, 640.0, 440.0),
            c.input,
            (width, height),
        );
        let mouse = c.input.mouse;
        let over = win.contains(mouse.0, mouse.1) || self.layout.is_dragging();
        if c.window(win, "Market Place", true) && !self.layout.is_dragging() {
            self.open = false;
            self.layout.end_frame(c.input);
            return over;
        }

        // ---- Tabs ----
        let mut tx = win.x + 12.0;
        for (tab, label) in [
            (Tab::Search, "Search"),
            (Tab::Consign, "Consign"),
            (Tab::Store, "Store"),
        ] {
            let r = Rect::new(tx, win.y + 30.0, 84.0, 22.0);
            let hot = r.contains(mouse.0, mouse.1);
            let on = self.tab == tab;
            c.fill(
                r,
                if on {
                    [0.35, 0.3, 0.18, 0.95]
                } else if hot {
                    [0.2, 0.2, 0.2, 0.8]
                } else {
                    [0.1, 0.1, 0.1, 0.7]
                },
            );
            c.border(r, [198, 166, 99, 255]);
            c.text.draw_centered(
                label,
                12,
                r.x + r.w / 2.0,
                r.y + 4.0,
                if on {
                    [255, 255, 200, 255]
                } else {
                    [190, 190, 190, 255]
                },
            );
            if hot && c.input.lmb_pressed && !on {
                self.tab = tab;
                self.selected = None;
                self.dirty = true;
            }
            tx += 90.0;
        }
        c.text.draw(
            &format!("Gold: {gold}"),
            12,
            win.x + win.w - 150.0,
            win.y + 34.0,
            [255, 220, 120, 255],
        );

        let fy = win.y + win.h - 3.0 - 42.0 + 8.0;
        match self.tab {
            Tab::Search => self.draw_search(c, catalog, win, fy, out),
            Tab::Consign => self.draw_consign(c, catalog, inventory, win, fy, out),
            Tab::Store => self.draw_store(c, catalog, currencies, win, fy, out),
        }
        // The store is served whole on entry, so only the auction tabs owe
        // the server a search.
        if self.dirty && self.tab != Tab::Store {
            self.dirty = false;
            out.push(self.search_msg());
        }
        self.layout.end_frame(c.input);
        over
    }

    fn draw_search(
        &mut self,
        c: &mut Ctx,
        catalog: &ItemCatalog,
        win: Rect,
        fy: f32,
        out: &mut Vec<ClientMessage>,
    ) {
        let mouse = c.input.mouse;
        let top = win.y + 60.0;
        c.text
            .draw("Name", 11, win.x + 12.0, top + 4.0, [220, 220, 220, 255]);
        self.boxes[B_NAME].rect = Rect::new(win.x + 52.0, top, 150.0, 22.0);
        self.boxes[B_NAME].update(c);

        // Item type and sort cycle on click, the way Zircon's combo boxes do
        // without the drop-down.
        let tr = Rect::new(win.x + 212.0, top, 96.0, 22.0);
        if cycle(c, tr, TYPE_FILTERS[self.filter].1) {
            self.filter = (self.filter + 1) % TYPE_FILTERS.len();
            self.page = 0;
            self.dirty = true;
        }
        let sr = Rect::new(win.x + 316.0, top, 120.0, 22.0);
        if cycle(c, sr, MarketSort::ALL[self.sort].1) {
            self.sort = (self.sort + 1) % MarketSort::ALL.len();
            self.page = 0;
            self.dirty = true;
        }
        let b = &mut self.buttons[0];
        b.pos = (win.x + 444.0, top);
        b.enabled = true;
        if b.update(c) {
            self.page = 0;
            self.dirty = true;
        }

        // ---- Results ----
        let rows = self.results.clone();
        let mut y = top + 34.0;
        for l in &rows {
            let r = Rect::new(win.x + 12.0, y, win.w - 24.0, 32.0);
            let hot = r.contains(mouse.0, mouse.1);
            let on = self.selected == Some(l.index);
            c.fill(
                r,
                if on {
                    [0.3, 0.3, 0.5, 0.85]
                } else if hot {
                    [0.22, 0.22, 0.3, 0.6]
                } else {
                    [0.0, 0.0, 0.0, 0.35]
                },
            );
            crate::windows::draw_item_cell(
                c,
                catalog,
                Rect::new(r.x + 2.0, r.y + 2.0, 28.0, 28.0),
                Some(&l.item),
                false,
                false,
            );
            let name = catalog.name(l.item.info);
            let label = if l.item.count > 1 {
                format!("{name} x{}", l.item.count)
            } else {
                name
            };
            c.text
                .draw(&label, 12, r.x + 38.0, r.y + 2.0, [255, 255, 255, 255]);
            c.text.draw(
                &format!("{} each", l.price),
                11,
                r.x + 38.0,
                r.y + 17.0,
                [255, 220, 120, 255],
            );
            c.text.draw(
                &l.seller,
                11,
                r.x + r.w - 220.0,
                r.y + 2.0,
                if l.is_owner {
                    [150, 255, 150, 255]
                } else {
                    [200, 200, 255, 255]
                },
            );
            c.text.draw(
                &l.message,
                10,
                r.x + r.w - 220.0,
                r.y + 17.0,
                [160, 160, 160, 255],
            );
            c.text.draw(
                &format!("{}", l.price * l.item.count as u64),
                12,
                r.x + r.w - 70.0,
                r.y + 9.0,
                [255, 220, 120, 255],
            );
            if hot && c.input.lmb_pressed {
                self.selected = Some(l.index);
                self.boxes[B_BUY_COUNT].text = "1".into();
            }
            y += 34.0;
        }
        if rows.is_empty() {
            c.text.draw(
                "Nothing matched. Try a wider filter.",
                12,
                win.x + 16.0,
                top + 44.0,
                [180, 180, 180, 255],
            );
        }

        // ---- Footer: paging and buy ----
        c.text.draw(
            &format!(
                "{} listings, page {}/{}",
                self.total,
                self.page + 1,
                self.pages()
            ),
            11,
            win.x + 12.0,
            fy + 5.0,
            [200, 200, 200, 255],
        );
        let pages = self.pages();
        let b = &mut self.buttons[2];
        b.pos = (win.x + 180.0, fy);
        b.enabled = self.page > 0;
        if b.update(c) && b.enabled {
            self.page -= 1;
            self.dirty = true;
        }
        let b = &mut self.buttons[3];
        b.pos = (win.x + 246.0, fy);
        b.enabled = self.page + 1 < pages;
        if b.update(c) && b.enabled {
            self.page += 1;
            self.dirty = true;
        }
        let picked = self
            .selected
            .and_then(|i| rows.iter().find(|l| l.index == i))
            .cloned();
        c.text
            .draw("Count", 11, win.x + 330.0, fy + 5.0, [220, 220, 220, 255]);
        self.boxes[B_BUY_COUNT].rect = Rect::new(win.x + 376.0, fy, 60.0, 22.0);
        self.boxes[B_BUY_COUNT].update(c);
        self.boxes[B_BUY_COUNT]
            .text
            .retain(|ch| ch.is_ascii_digit());
        let count: u32 = self.boxes[B_BUY_COUNT].text.parse().unwrap_or(0);
        let b = &mut self.buttons[1];
        b.pos = (win.x + 446.0, fy);
        // Zircon refuses your own listing, so the button is simply off.
        b.enabled = picked
            .as_ref()
            .is_some_and(|l| !l.is_owner && count > 0 && count <= l.item.count);
        if b.update(c) && b.enabled {
            if let Some(l) = &picked {
                out.push(ClientMessage::MarketBuy {
                    index: l.index,
                    count,
                });
                self.dirty = true;
            }
        }
        if let Some(l) = &picked {
            c.text.draw(
                &format!("Total {}", l.price * u64::from(count)),
                11,
                win.x + 530.0,
                fy + 5.0,
                [255, 220, 120, 255],
            );
        }
    }

    /// The Store tab: fixed stock out of `StoreInfo`, filtered by tag and
    /// name, paid for in game gold or hunt gold.
    fn draw_store(
        &mut self,
        c: &mut Ctx,
        catalog: &ItemCatalog,
        currencies: &[mir_proto::CurrencySummary],
        win: Rect,
        fy: f32,
        out: &mut Vec<ClientMessage>,
    ) {
        let mouse = c.input.mouse;
        let top = win.y + 60.0;

        // ---- Filter row: name, tag, sort ----
        c.text
            .draw("Name", 11, win.x + 12.0, top + 4.0, [220, 220, 220, 255]);
        self.boxes[B_STORE_NAME].rect = Rect::new(win.x + 52.0, top, 150.0, 22.0);
        self.boxes[B_STORE_NAME].update(c);

        let tag_label = self
            .store_tags
            .get(self.store_tag)
            .cloned()
            .unwrap_or_else(|| "All".into());
        let tr = Rect::new(win.x + 212.0, top, 110.0, 22.0);
        if cycle(c, tr, &tag_label) && !self.store_tags.is_empty() {
            self.store_tag = (self.store_tag + 1) % self.store_tags.len();
            self.store_page = 0;
            self.store_selected = None;
        }
        let sr = Rect::new(win.x + 330.0, top, 110.0, 22.0);
        if cycle(c, sr, StoreSort::ALL[self.store_sort].1) {
            self.store_sort = (self.store_sort + 1) % StoreSort::ALL.len();
            self.store_page = 0;
        }
        // Hunt gold is a toggle, as it is a check box in Zircon.
        let hr = Rect::new(win.x + 448.0, top, 96.0, 22.0);
        let hot = hr.contains(mouse.0, mouse.1);
        c.fill(
            hr,
            if self.hunt_gold {
                [0.35, 0.3, 0.18, 0.95]
            } else if hot {
                [0.25, 0.25, 0.25, 0.9]
            } else {
                [0.12, 0.12, 0.12, 0.85]
            },
        );
        c.border(hr, [198, 166, 99, 255]);
        c.text.draw(
            if self.hunt_gold {
                "Hunt gold"
            } else {
                "Game gold"
            },
            11,
            hr.x + 6.0,
            hr.y + 4.0,
            if self.hunt_gold {
                [255, 255, 200, 255]
            } else {
                [200, 200, 200, 255]
            },
        );
        if hot && c.input.lmb_pressed {
            self.hunt_gold = !self.hunt_gold;
        }

        // ---- Rows ----
        let all = self.store_results(catalog);
        let pages = self.store_pages(all.len());
        if self.store_page >= pages {
            self.store_page = pages - 1;
        }
        let rows: Vec<StoreEntry> = all
            .iter()
            .skip(self.store_page as usize * STORE_PAGE)
            .take(STORE_PAGE)
            .cloned()
            .collect();
        let mut y = top + 34.0;
        for e in &rows {
            let r = Rect::new(win.x + 12.0, y, win.w - 24.0, 32.0);
            let hot = r.contains(mouse.0, mouse.1);
            let on = self.store_selected == Some(e.index);
            c.fill(
                r,
                if on {
                    [0.3, 0.3, 0.5, 0.85]
                } else if hot {
                    [0.22, 0.22, 0.3, 0.6]
                } else {
                    [0.0, 0.0, 0.0, 0.35]
                },
            );
            let inst = ItemInstance {
                id: 0,
                info: e.item,
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
            crate::windows::draw_item_cell(
                c,
                catalog,
                Rect::new(r.x + 2.0, r.y + 2.0, 28.0, 28.0),
                Some(&inst),
                false,
                false,
            );
            c.text.draw(
                &catalog.name(e.item),
                12,
                r.x + 38.0,
                r.y + 2.0,
                [255, 255, 255, 255],
            );
            // Zircon labels the tag list under the name, which is the only
            // place a row says what kind of thing it is.
            c.text.draw(
                &e.filters.join(", "),
                10,
                r.x + 38.0,
                r.y + 18.0,
                [160, 160, 160, 255],
            );
            if e.duration > 0 {
                c.text.draw(
                    &format!("{} days", e.duration / 86_400),
                    10,
                    r.x + r.w - 250.0,
                    r.y + 18.0,
                    [200, 180, 140, 255],
                );
            }
            let price = self.unit_price(e);
            c.text.draw(
                &format!("{price} {}", if self.hunt_gold { "HG" } else { "GG" }),
                12,
                r.x + r.w - 130.0,
                r.y + 9.0,
                if e.available {
                    [255, 220, 120, 255]
                } else {
                    [160, 160, 160, 255]
                },
            );
            if !e.available {
                c.text.draw(
                    "(Not Available)",
                    10,
                    r.x + r.w - 250.0,
                    r.y + 2.0,
                    [220, 140, 140, 255],
                );
            }
            if hot && c.input.lmb_pressed {
                self.store_selected = Some(e.index);
                self.boxes[B_STORE_COUNT].text = "1".into();
            }
            y += 34.0;
        }
        if rows.is_empty() {
            c.text.draw(
                "Nothing matched. Try another tag.",
                12,
                win.x + 16.0,
                top + 44.0,
                [180, 180, 180, 255],
            );
        }

        // ---- Footer: paging, count, buy ----
        c.text.draw(
            &format!(
                "{} items, page {}/{}",
                all.len(),
                self.store_page + 1,
                pages
            ),
            11,
            win.x + 12.0,
            fy + 5.0,
            [200, 200, 200, 255],
        );
        let b = &mut self.buttons[7];
        b.pos = (win.x + 180.0, fy);
        b.enabled = self.store_page > 0;
        if b.update(c) && b.enabled {
            self.store_page -= 1;
        }
        let b = &mut self.buttons[8];
        b.pos = (win.x + 246.0, fy);
        b.enabled = self.store_page + 1 < pages;
        if b.update(c) && b.enabled {
            self.store_page += 1;
        }
        c.text
            .draw("Count", 11, win.x + 330.0, fy + 5.0, [220, 220, 220, 255]);
        self.boxes[B_STORE_COUNT].rect = Rect::new(win.x + 376.0, fy, 60.0, 22.0);
        self.boxes[B_STORE_COUNT].update(c);
        self.boxes[B_STORE_COUNT]
            .text
            .retain(|ch| ch.is_ascii_digit());
        let count: u32 = self.boxes[B_STORE_COUNT].text.parse().unwrap_or(0);
        let picked = self
            .store_selected
            .and_then(|i| rows.iter().find(|e| e.index == i))
            .cloned();
        let b = &mut self.buttons[6];
        b.pos = (win.x + 446.0, fy);
        b.enabled = picked
            .as_ref()
            .is_some_and(|e| e.available && count > 0 && count <= e.stack_size);
        if b.update(c) && b.enabled {
            if let Some(e) = &picked {
                out.push(ClientMessage::MarketStoreBuy {
                    index: e.index,
                    count,
                    hunt_gold: self.hunt_gold,
                });
            }
        }
        // What the purchase costs, and what is in the purse for it.
        let want = if self.hunt_gold { "HG" } else { "GG" };
        let held = currencies
            .iter()
            .find(|c| c.abbreviation == want)
            .map(|c| c.amount)
            .unwrap_or(0);
        let total = picked
            .as_ref()
            .map(|e| self.unit_price(e) * u64::from(count))
            .unwrap_or(0);
        c.text.draw(
            &format!("Total {total} / {held} {want}"),
            11,
            win.x + 530.0,
            fy + 5.0,
            if total as i64 > held {
                [255, 140, 140, 255]
            } else {
                [255, 220, 120, 255]
            },
        );
    }

    /// The price one unit costs right now. Zircon's rule: a zero
    /// `HuntGoldPrice` means hunt gold costs the game-gold price, not that
    /// the row cannot be bought with it.
    fn unit_price(&self, e: &StoreEntry) -> u64 {
        if self.hunt_gold && e.hunt_gold_price != 0 {
            e.hunt_gold_price
        } else {
            e.price
        }
    }

    fn draw_consign(
        &mut self,
        c: &mut Ctx,
        catalog: &ItemCatalog,
        inventory: &[Option<ItemInstance>],
        win: Rect,
        fy: f32,
        out: &mut Vec<ClientMessage>,
    ) {
        let mouse = c.input.mouse;
        let top = win.y + 60.0;
        // ---- Left: the bag, to pick what to sell ----
        c.text
            .draw("Your bag", 12, win.x + 12.0, top, [255, 255, 200, 255]);
        let cols = 6;
        for (i, cell) in inventory.iter().enumerate().take(36) {
            let r = Rect::new(
                win.x + 12.0 + (i % cols) as f32 * 32.0,
                top + 18.0 + (i / cols) as f32 * 32.0,
                30.0,
                30.0,
            );
            let hot = r.contains(mouse.0, mouse.1);
            let on = self.consign_slot == Some(i as u8);
            crate::windows::draw_item_cell(c, catalog, r, cell.as_ref(), hot, on);
            if hot && c.input.lmb_pressed && cell.is_some() {
                self.consign_slot = Some(i as u8);
                self.boxes[B_COUNT].text =
                    cell.as_ref().map(|it| it.count).unwrap_or(1).to_string();
            }
        }

        // ---- Right: the listing form ----
        let rx = win.x + 12.0 + cols as f32 * 32.0 + 16.0;
        let chosen = self
            .consign_slot
            .and_then(|s| inventory.get(s as usize))
            .and_then(|c| c.as_ref())
            .cloned();
        let label = chosen
            .as_ref()
            .map(|it| format!("{} x{}", catalog.name(it.info), it.count))
            .unwrap_or_else(|| "Pick a bag item".into());
        c.text.draw(&label, 12, rx, top, [255, 255, 255, 255]);
        let fields = [
            ("Price each", B_PRICE),
            ("Count", B_COUNT),
            ("Note", B_NOTE),
        ];
        let mut by = top + 22.0;
        for (name, i) in fields {
            c.text.draw(name, 11, rx, by + 4.0, [220, 220, 220, 255]);
            let w = if i == B_NOTE { 230.0 } else { 100.0 };
            self.boxes[i].rect = Rect::new(rx + 74.0, by, w, 22.0);
            self.boxes[i].update(c);
            if i != B_NOTE {
                self.boxes[i].text.retain(|ch| ch.is_ascii_digit());
            }
            by += 28.0;
        }
        let price: u64 = self.boxes[B_PRICE].text.parse().unwrap_or(0);
        let count: u32 = self.boxes[B_COUNT].text.parse().unwrap_or(0);
        // Zircon takes its cut when the item sells, not when it is listed, so
        // show the seller what they would actually receive.
        let gross = price * u64::from(count);
        c.text.draw(
            &format!(
                "Sells for {gross}, you receive {} after {}% tax",
                gross - gross * 7 / 100,
                7
            ),
            11,
            rx,
            by + 2.0,
            [200, 200, 160, 255],
        );
        let b = &mut self.buttons[4];
        b.pos = (rx, by + 22.0);
        b.enabled = chosen
            .as_ref()
            .is_some_and(|it| price > 0 && count > 0 && count <= it.count);
        if b.update(c) && b.enabled {
            if let Some(slot) = self.consign_slot {
                out.push(ClientMessage::MarketConsign {
                    slot,
                    count,
                    price,
                    message: self.boxes[B_NOTE].text.clone(),
                });
                self.consign_slot = None;
                self.boxes[B_PRICE].text.clear();
                self.boxes[B_COUNT].text.clear();
            }
        }

        // ---- Below: what is already listed ----
        let listed = self.consignments.clone();
        c.text.draw(
            &format!("Listed by you ({})", listed.len()),
            12,
            win.x + 12.0,
            top + 210.0,
            [255, 255, 200, 255],
        );
        let mut y = top + 228.0;
        for l in listed.iter().take(4) {
            let r = Rect::new(win.x + 12.0, y, win.w - 24.0, 24.0);
            let hot = r.contains(mouse.0, mouse.1);
            let on = self.selected == Some(l.index);
            c.fill(
                r,
                if on {
                    [0.3, 0.3, 0.5, 0.85]
                } else if hot {
                    [0.22, 0.22, 0.3, 0.6]
                } else {
                    [0.0, 0.0, 0.0, 0.35]
                },
            );
            c.text.draw(
                &format!(
                    "{} x{}  at {} each",
                    catalog.name(l.item.info),
                    l.item.count,
                    l.price
                ),
                11,
                r.x + 6.0,
                r.y + 5.0,
                [255, 255, 255, 255],
            );
            if hot && c.input.lmb_pressed {
                self.selected = Some(l.index);
            }
            y += 26.0;
        }
        if listed.is_empty() {
            c.text.draw(
                "You have nothing listed.",
                11,
                win.x + 16.0,
                y + 2.0,
                [180, 180, 180, 255],
            );
        }
        let picked = self
            .selected
            .and_then(|i| listed.iter().find(|l| l.index == i))
            .cloned();
        let b = &mut self.buttons[5];
        b.pos = (win.x + 12.0, fy);
        b.enabled = picked.is_some();
        if b.update(c) && b.enabled {
            if let Some(l) = &picked {
                out.push(ClientMessage::MarketCancelConsign {
                    index: l.index,
                    count: l.item.count,
                });
                self.selected = None;
            }
        }
        c.text.draw(
            "Listing is free. Tax is taken from the sale. Safe zones only.",
            11,
            win.x + 104.0,
            fy + 5.0,
            [170, 170, 170, 255],
        );
    }
}

/// A click-to-cycle stand-in for Zircon's `DXComboBox`.
/// Zircon `StoreSearch`: a row matches when the chosen tag is in its tag
/// list and the typed text is in the item's name, both case-insensitively.
pub fn store_matches(e: &StoreEntry, item_name: &str, tag: Option<&str>, name: &str) -> bool {
    if let Some(tag) = tag {
        if !e.filters.iter().any(|f| f.eq_ignore_ascii_case(tag)) {
            return false;
        }
    }
    name.is_empty() || item_name.to_lowercase().contains(&name.to_lowercase())
}

fn cycle(c: &mut Ctx, r: Rect, label: &str) -> bool {
    let hot = r.contains(c.input.mouse.0, c.input.mouse.1);
    c.fill(
        r,
        if hot {
            [0.25, 0.25, 0.25, 0.9]
        } else {
            [0.12, 0.12, 0.12, 0.85]
        },
    );
    c.border(r, [198, 166, 99, 255]);
    c.text
        .draw(label, 11, r.x + 6.0, r.y + 4.0, [235, 235, 235, 255]);
    hot && c.input.lmb_pressed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> MarketState {
        MarketState {
            total: 20,
            ..Default::default()
        }
    }

    #[test]
    fn pages_round_up_and_never_reach_zero() {
        let mut s = state();
        // 20 listings at 9 a page is three pages.
        assert_eq!(s.pages(), 3);
        s.total = 9;
        assert_eq!(s.pages(), 1);
        s.total = 0;
        assert_eq!(s.pages(), 1);
    }

    fn entry(index: u32, item: i32, price: u64, tags: &[&str]) -> StoreEntry {
        StoreEntry {
            index,
            item,
            stack_size: 10,
            price,
            hunt_gold_price: 0,
            filters: tags.iter().map(|t| t.to_string()).collect(),
            available: true,
            duration: 0,
        }
    }

    #[test]
    fn store_tags_are_built_from_the_stock_sorted_with_all_first() {
        let mut s = state();
        s.set_stock(vec![
            entry(1, 10, 200, &["Torch", "Temporary"]),
            entry(2, 11, 500, &["Mount", "Temporary"]),
            entry(3, 12, 100, &["Torch"]),
        ]);
        // Distinct, sorted, "All" first -- Zircon builds its drop-down the
        // same way rather than from a fixed list.
        assert_eq!(s.store_tags, vec!["All", "Mount", "Temporary", "Torch"]);
        // Index 0 is "All", which filters nothing.
        assert_eq!(s.store_tag(), None);
        s.store_tag = 3;
        assert_eq!(s.store_tag(), Some("Torch"));
    }

    #[test]
    fn store_sort_and_paging_follow_zircon() {
        let mut s = state();
        s.set_stock(
            (0..14)
                .map(|i| entry(i, i as i32, 100 + i as u64, &["X"]))
                .collect(),
        );
        // Nine rows a page, as Zircon shows.
        assert_eq!(s.store_pages(14), 2);
        assert_eq!(s.store_pages(9), 1);
        assert_eq!(s.store_pages(0), 1);
        // Zircon's fourth sort, Favourite, is a TODO in its own client and
        // is left out, so three remain.
        assert_eq!(StoreSort::ALL.len(), 3);
        assert_eq!(s.store_sort(), StoreSort::Alphabetical);
        s.store_sort = 1;
        assert_eq!(s.store_sort(), StoreSort::HighestPrice);
        s.store_sort = 99;
        assert_eq!(s.store_sort(), StoreSort::LowestPrice);
    }

    #[test]
    fn store_matching_is_tag_and_name_case_insensitive() {
        let e = entry(1, 10, 200, &["Torch", "Warrior"]);
        assert!(store_matches(&e, "Iron Torch", None, ""));
        assert!(store_matches(&e, "Iron Torch", Some("torch"), "IRON"));
        assert!(!store_matches(&e, "Iron Torch", Some("Mount"), ""));
        assert!(!store_matches(&e, "Iron Torch", None, "sword"));
    }

    #[test]
    fn sort_and_filter_cycle_through_zircon_order() {
        let mut s = state();
        assert_eq!(s.sort(), MarketSort::Newest);
        s.sort = 2;
        assert_eq!(s.sort(), MarketSort::HighestPrice);
        // Out of range clamps rather than panicking.
        s.sort = 99;
        assert_eq!(s.sort(), MarketSort::LowestPrice);
        assert_eq!(TYPE_FILTERS[0].0, None);
        assert_eq!(TYPE_FILTERS[1].0, Some(item_type::WEAPON));
    }
}
