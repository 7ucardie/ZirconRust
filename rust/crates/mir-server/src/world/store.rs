//! The in-game store: Zircon's `MarketPlaceDialog` Store tab, bought with
//! `C.MarketPlaceStoreBuy` and served by `PlayerObject.MarketPlaceStoreBuy`
//! in `ServerLibrary/Models/PlayerObject.Quests.cs`.
//!
//! This is the cash shop rather than the player auction house: the stock is
//! the `StoreInfo` table, it never runs out, and it is paid for in game gold
//! or hunt gold rather than in the gold carried in the bag.
//!
//! The C# rules, in the order it applies them:
//!
//! 1. a count of zero or less does nothing;
//! 2. an unknown row, or one whose `Item` is missing, does nothing;
//! 3. a row that is not `Available` answers "not available";
//! 4. the count is clamped to the item's stack size;
//! 5. the unit price is `HuntGoldPrice` when paying in hunt gold *and that
//!    price is not zero*, otherwise `Price` either way -- so a zero hunt-gold
//!    price means "costs the same in hunt gold", not "cannot be bought";
//! 6. no room in the bag answers "need space";
//! 7. not enough of the chosen currency answers "cost", and nothing moves;
//! 8. only then is the currency taken and the item created.

use mir_proto::{ChatKind, ServerMessage, StoreEntry};

use super::*;
use crate::items::{Changed, UserItem};

/// Zircon `CurrencyType`.
pub const CURRENCY_GAME_GOLD: i32 = 1;
pub const CURRENCY_HUNT_GOLD: i32 = 2;

/// Unix seconds, for stamping a timed item's expiry.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Zircon's `StoreSearch`: a row matches when the filter tag appears in its
/// tag list and the typed text appears in the item's name. Both tests are
/// case-insensitive here; Zircon's name test is too (`OrdinalIgnoreCase`).
pub fn store_matches(entry: &StoreEntry, item_name: &str, tag: Option<&str>, name: &str) -> bool {
    if let Some(tag) = tag {
        if !entry.filters.iter().any(|f| f.eq_ignore_ascii_case(tag)) {
            return false;
        }
    }
    name.is_empty() || item_name.to_lowercase().contains(&name.to_lowercase())
}

impl World {
    pub fn send_tab_prices(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        self.send_to(
            id,
            ServerMessage::TabPrices {
                inventory: if p.bag.inventory.len() < mir_proto::MAX_INVENTORY_SIZE {
                    self.inventory_tab_price
                } else {
                    0
                },
                storage: if p.storage_size < crate::items::STORAGE_SIZE * 2 {
                    self.storage_tab_price
                } else {
                    0
                },
            },
        );
    }

    /// One extra bag or account-storage page, paid with the character's Game Gold.
    pub fn buy_tab(&mut self, id: ObjectId, storage: bool) -> bool {
        let Some(currency) = self.currency_index(CURRENCY_GAME_GOLD) else {
            self.store_line(id, "Game Gold is not configured on this server.");
            return false;
        };
        let price = if storage {
            self.storage_tab_price
        } else {
            self.inventory_tab_price
        } as i64;
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return false;
        };
        let full = if storage {
            p.storage_size >= crate::items::STORAGE_SIZE * 2
        } else {
            p.bag.inventory.len() >= mir_proto::MAX_INVENTORY_SIZE
        };
        if full {
            self.store_line(id, "All tabs are already unlocked.");
            return false;
        }
        let held = p.currencies.get(&currency).copied().unwrap_or(0);
        if held < price {
            self.store_line(id, "Not enough Game Gold for that tab.");
            return false;
        }
        let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
        p.currencies.insert(currency, held - price);
        if storage {
            p.storage_size = crate::items::STORAGE_SIZE * 2;
            p.bag.storage.resize(p.storage_size as usize, None);
            self.send_storage(id);
        } else {
            p.bag.inventory.resize(mir_proto::MAX_INVENTORY_SIZE, None);
            self.send_inventory(id);
        }
        self.send_currencies(id);
        self.send_tab_prices(id);
        self.store_line(id, "Tab unlocked.");
        true
    }

    /// The store as the client sees it. Sent once on entry: the table is
    /// static, so the client filters and sorts its own copy.
    pub fn store_stock(&self) -> Vec<StoreEntry> {
        self.data
            .stores
            .iter()
            .filter_map(|s| {
                let def = self.data.items.get(&s.item)?;
                Some(StoreEntry {
                    index: s.index.max(0) as u32,
                    item: s.item,
                    stack_size: def.stack_size.max(1) as u32,
                    price: s.price.max(0) as u64,
                    hunt_gold_price: s.hunt_gold_price.max(0) as u64,
                    filters: s.filters.clone(),
                    available: s.available,
                    duration: s.duration.max(0) as u32,
                })
            })
            .collect()
    }

    pub fn store_login(&mut self, id: ObjectId) {
        let stock = self.store_stock();
        self.send_to(id, ServerMessage::StoreStock(stock));
    }

    fn store_line(&mut self, id: ObjectId, text: &str) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text: text.into(),
            },
        );
    }

    /// The index of the `CurrencyInfo` row of a `CurrencyType`, if the pack
    /// defines one.
    fn currency_index(&self, currency_type: i32) -> Option<i32> {
        self.data
            .currencies
            .iter()
            .find(|c| c.currency_type == currency_type)
            .map(|c| c.index)
    }

    /// Zircon `MarketPlaceStoreBuy`.
    pub fn store_buy(&mut self, id: ObjectId, index: u32, count: u32, hunt_gold: bool) {
        self.store_buy_for_account(id, index, count, hunt_gold, 0);
    }

    /// Highest persisted account level is combined with the current online level.
    pub fn store_buy_for_account(
        &mut self,
        id: ObjectId,
        index: u32,
        count: u32,
        hunt_gold: bool,
        highest_level: i32,
    ) {
        if count == 0 {
            return;
        }
        let Some(row) = self
            .data
            .stores
            .iter()
            .find(|s| s.index.max(0) as u32 == index)
            .cloned()
        else {
            return;
        };
        let Some(def) = self.data.items.get(&row.item).cloned() else {
            return;
        };
        if !row.available {
            self.store_line(id, "That is not available.");
            return;
        }
        // Zircon clamps the count to one stack rather than refusing.
        let count = count.min(def.stack_size.max(1) as u32);
        // A zero hunt-gold price means it costs the same in hunt gold, which
        // is every row in this pack; it does not mean hunt gold is refused.
        let unit = if hunt_gold && row.hunt_gold_price != 0 {
            row.hunt_gold_price
        } else {
            row.price
        }
        .max(0) as i64;
        let Some(cost) = unit.checked_mul(i64::from(count)) else {
            return;
        };
        let currency_type = if hunt_gold {
            CURRENCY_HUNT_GOLD
        } else {
            CURRENCY_GAME_GOLD
        };
        let Some(currency) = self.currency_index(currency_type) else {
            self.store_line(id, "That currency does not exist on this server.");
            return;
        };

        let mut item = UserItem::fresh(&self.data, row.item, count).unwrap();
        item.expires_at = (row.duration > 0).then(|| now_secs() + row.duration as u64);
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        item.worthless = true;
        item.locked = true;
        item.bound = hunt_gold || highest_level.max(p.level) < 40;
        // Room first, then money: Zircon checks in this order, so a full bag
        // never costs the player anything.
        if !p.bag.can_gain_item(&self.data, &item, p.max_bag) {
            self.store_line(id, "You do not have enough space.");
            return;
        }
        let held = p.currencies.get(&currency).copied().unwrap_or(0);
        if held < cost {
            self.store_line(id, "You cannot afford that.");
            return;
        }

        // Charge, then hand over. Nothing between these two can fail.
        let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
        p.currencies.insert(currency, held - cost);
        let changes = p.bag.gain_item(&self.data, item, &mut p.next_item_id);
        self.send_changes(id, changes);
        self.send_currencies(id);
        let name = def.name.clone();
        self.store_line(id, &format!("You bought {name} x{count}."));
    }

    /// Remove expired goods on entry and once per second while online.
    pub fn expire_items(&mut self, id: ObjectId) {
        let now = now_secs();
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let mut changes = Changed::new();
        let mut gone: Vec<i32> = Vec::new();
        let dead = |it: &Option<UserItem>| {
            it.as_ref()
                .and_then(|it| it.expires_at)
                .is_some_and(|t| t <= now)
        };
        for grid in [Grid::Inventory, Grid::Equipment, Grid::Storage] {
            for (i, cell) in p.bag.grid_mut(grid).iter_mut().enumerate() {
                if dead(cell) {
                    if let Some(it) = cell.take() {
                        gone.push(it.info);
                    }
                    changes.push((grid, i as u8, None));
                }
            }
        }
        if gone.is_empty() {
            return;
        }
        let names: Vec<String> = gone
            .iter()
            .map(|i| {
                self.data
                    .items
                    .get(i)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "An item".into())
            })
            .collect();
        self.send_changes(id, changes);
        for name in names {
            self.store_line(id, &format!("{name} has expired."));
        }
        // Equipment may have vanished, so the stat block has to be rebuilt.
        self.refresh_stats(id, false);
        self.refresh_appearance(id);
        self.send_player_stats(id);
    }
}
