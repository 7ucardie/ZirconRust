//! The marketplace (Zircon `MarketPlaceDialog` and the `AuctionInfo` half of
//! `PlayerObject.Quests`): players consign items from a safe zone, search
//! what everyone has listed, and buy at the asking price.
//!
//! Zircon's rules, which this follows:
//! - Consigning costs nothing. `Globals.MarketPlaceFee` is 0 and the C#
//!   leaves the fee arithmetic commented out, so nothing is charged up front.
//! - The seller is paid `price * count` minus `Globals.MarketPlaceTax` (7%),
//!   and the money arrives as mail, not as gold in hand.
//! - A listing never expires. It stands until it is bought or cancelled.
//! - You cannot buy your own listing.
//! - A part-bought listing shrinks; it is deleted when it reaches zero.
//!
//! Listings live in `market.json` under the data dir, like guilds and mail.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::*;
use crate::items::UserItem;
use mir_proto::{ChatKind, MarketListing, MarketSort, MARKET_PAGE};

/// Zircon `Globals.MarketPlaceTax`: the cut taken off a sale, in percent.
/// Held as an integer so the arithmetic is exact.
pub const MARKET_TAX_PERCENT: u64 = 7;
/// Zircon caps the seller's note at 150 characters.
pub const MAX_MESSAGE: usize = 150;
/// Zircon: `HighestLevel() * 3 + StorageSize - Globals.StorageSize`. We have
/// no per-account storage upgrades, so the level term is the whole of it,
/// with a floor so a level 1 character can still list something.
pub const CONSIGN_PER_LEVEL: usize = 3;
pub const MIN_CONSIGN_LIMIT: usize = 5;

/// The seller's cut of a sale after tax.
pub fn after_tax(total: u64) -> u64 {
    total - tax_on(total)
}

/// The tax taken off a sale total.
pub fn tax_on(total: u64) -> u64 {
    total * MARKET_TAX_PERCENT / 100
}

/// How many listings an account of this level may hold at once.
pub fn consign_limit(level: i32) -> usize {
    (level.max(0) as usize * CONSIGN_PER_LEVEL).max(MIN_CONSIGN_LIMIT)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub index: u32,
    /// The account that listed it, which is paid and may cancel.
    pub account: u32,
    /// The character name shown as the seller.
    pub seller: String,
    pub item: UserItem,
    /// Price per unit.
    pub price: u64,
    pub message: String,
    /// Unix seconds, for the Newest and Oldest sorts.
    pub created: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MarketStore {
    pub next_index: u32,
    pub listings: Vec<Listing>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl MarketStore {
    pub(super) fn load(path: PathBuf) -> MarketStore {
        let mut store: MarketStore = super::load_json(&path);
        store.path = Some(path);
        store
    }

    pub(super) fn save(&self) {
        if let Some(p) = &self.path {
            super::save_json(p, self);
        }
    }

    /// Listings held by one account.
    pub fn of_account(&self, account: u32) -> impl Iterator<Item = &Listing> {
        self.listings.iter().filter(move |l| l.account == account)
    }
}

fn summary(l: &Listing, account: u32) -> MarketListing {
    MarketListing {
        index: l.index,
        item: l.item.instance(),
        price: l.price,
        seller: l.seller.clone(),
        message: l.message.clone(),
        is_owner: l.account == account,
    }
}

impl World {
    /// Legacy `@reboot`: withdraw listings affected by item-part and
    /// level-40..56 rule changes, returning each item by system mail.
    pub(super) fn gm_cancel_changed_listings(&mut self) -> usize {
        let (cancelled, kept): (Vec<Listing>, Vec<Listing>) =
            std::mem::take(&mut self.market_store.listings)
                .into_iter()
                .partition(|listing| {
                    self.data.items.get(&listing.item.info).is_some_and(|def| {
                        def.item_type == 25
                            || (def.required_type == 0 && (40..=56).contains(&def.required_amount))
                    })
                });
        self.market_store.listings = kept;
        if cancelled.is_empty() {
            return 0;
        }
        self.market_store.save();
        let count = cancelled.len();
        let mut affected = std::collections::BTreeSet::new();
        for listing in cancelled {
            let account = listing.account;
            affected.insert(account);
            self.mail_store.next_index += 1;
            let mail = super::mail::Mail {
                index: self.mail_store.next_index,
                sender: "System".into(),
                date: crate::accounts::now_secs(),
                subject: "Listing Cancelled".into(),
                message: "Your listing was cancelled because of item changes.".into(),
                opened: false,
                gold: 0,
                items: vec![listing.item],
            };
            if let Some(online) = self.online_account(account) {
                self.send_to(online, ServerMessage::MailNew(super::mail::summary(&mail)));
            }
            self.mail_store.boxes.entry(account).or_default().push(mail);
        }
        self.mail_store.save();
        for account in affected {
            if let Some(online) = self.online_account(account) {
                self.market_login(online);
            }
        }
        count
    }
    fn market_line(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }

    /// Send this account's listings (on entry and after every change).
    pub fn market_login(&mut self, id: ObjectId) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        let mine: Vec<MarketListing> = self
            .market_store
            .of_account(account)
            .map(|l| summary(l, account))
            .collect();
        self.send_to(id, ServerMessage::MarketConsignments(mine));
    }

    /// Zircon `SConnection.Process(C.MarketPlaceSearch)`: filter by name and
    /// item type, sort, and return one page of nine.
    pub fn market_search(
        &mut self,
        id: ObjectId,
        name: String,
        item_type: Option<u8>,
        sort: MarketSort,
        page: u32,
    ) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        let needle = name.trim().to_lowercase();
        let mut hits: Vec<&Listing> = self
            .market_store
            .listings
            .iter()
            .filter(|l| {
                let Some(def) = self.data.items.get(&l.item.info) else {
                    return false;
                };
                if let Some(t) = item_type {
                    if def.item_type != t {
                        return false;
                    }
                }
                needle.is_empty() || def.name.to_lowercase().contains(&needle)
            })
            .collect();
        match sort {
            MarketSort::Newest => hits.sort_by_key(|l| std::cmp::Reverse(l.index)),
            MarketSort::Oldest => hits.sort_by_key(|l| l.index),
            // Ties break by index so a page is stable between requests.
            MarketSort::HighestPrice => {
                hits.sort_by(|a, b| b.price.cmp(&a.price).then(a.index.cmp(&b.index)))
            }
            MarketSort::LowestPrice => {
                hits.sort_by(|a, b| a.price.cmp(&b.price).then(a.index.cmp(&b.index)))
            }
        }
        let total = hits.len() as u32;
        let results: Vec<MarketListing> = hits
            .into_iter()
            .skip(page as usize * MARKET_PAGE)
            .take(MARKET_PAGE)
            .map(|l| summary(l, account))
            .collect();
        self.send_to(
            id,
            ServerMessage::MarketSearchResults {
                total,
                page,
                results,
            },
        );
    }

    /// Zircon `PlayerObject.MarketPlaceConsign`: move `count` of the bag item
    /// in `slot` into escrow at `price` each.
    pub fn market_consign(
        &mut self,
        id: ObjectId,
        slot: u8,
        count: u32,
        price: u64,
        message: String,
    ) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if !o.in_safe_zone {
            self.market_line(id, "You can only consign items in a safe zone.".into());
            return;
        }
        if price == 0 || count == 0 || message.chars().count() > MAX_MESSAGE {
            return;
        }
        let Some(item) = p.bag.inventory.get(slot as usize).cloned().flatten() else {
            self.market_line(id, "Nothing there.".into());
            return;
        };
        if count > item.count {
            return;
        }
        if item.bound
            || self
                .data
                .items
                .get(&item.info)
                .is_some_and(|d| !d.can_trade)
        {
            self.market_line(id, "That item cannot be sold.".into());
            return;
        }
        let limit = consign_limit(p.level);
        if self.market_store.of_account(account).count() >= limit {
            self.market_line(id, format!("You cannot list more than {limit} items."));
            return;
        }
        let seller = p.name.clone();
        // Take the item first, so the listing can only ever hold what left
        // the bag.
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        let mut changes = Changed::new();
        if let Some(ch) = p.bag.take(Grid::Inventory, slot, count) {
            changes.push(ch);
        }
        self.send_changes(id, changes);
        self.account_dirty = true;
        let listed = {
            let store = &mut self.market_store;
            store.next_index += 1;
            let listing = Listing {
                index: store.next_index,
                account,
                seller,
                item: UserItem { count, ..item },
                price,
                message,
                created: crate::accounts::now_secs(),
            };
            store.listings.push(listing.clone());
            store.save();
            listing
        };
        self.send_to(
            id,
            ServerMessage::MarketConsignments(vec![summary(&listed, account)]),
        );
        self.market_line(id, "Item listed.".into());
        self.market_login(id);
    }

    /// Zircon `MarketPlaceCancelConsign`: take `count` back off your listing.
    /// The item goes to the bag when there is room in a safe zone, otherwise
    /// by mail, exactly as the C# does.
    pub fn market_cancel_consign(&mut self, id: ObjectId, index: u32, count: u32) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        if count == 0 {
            return;
        }
        let Some(listing) = self
            .market_store
            .listings
            .iter()
            .find(|l| l.index == index && l.account == account)
            .cloned()
        else {
            self.market_line(id, "That listing is gone.".into());
            return;
        };
        if listing.item.count < count {
            self.market_line(id, "There are not that many left.".into());
            return;
        }
        let returned = UserItem {
            count,
            ..listing.item.clone()
        };
        self.market_take(index, count);
        self.deliver_item(
            id,
            returned,
            "Listing Cancelled",
            "You cancelled your listing",
        );
        let left = self.listing_count(index);
        self.send_to(
            id,
            ServerMessage::MarketConsignChanged { index, count: left },
        );
        self.market_login(id);
    }

    /// Zircon `MarketPlaceBuy`: pay `price * count`, take the goods, and mail
    /// the seller the proceeds less tax.
    pub fn market_buy(&mut self, id: ObjectId, index: u32, count: u32) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        if count == 0 {
            return;
        }
        let Some(listing) = self
            .market_store
            .listings
            .iter()
            .find(|l| l.index == index)
            .cloned()
        else {
            self.market_line(id, "That listing is gone.".into());
            return;
        };
        if listing.account == account {
            self.market_line(id, "You cannot buy your own listing.".into());
            return;
        }
        if listing.item.count < count {
            self.market_line(id, "There are not that many left.".into());
            return;
        }
        let Some(total) = listing.price.checked_mul(count as u64) else {
            return;
        };
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if p.bag.gold < total {
            self.market_line(id, "You do not have that much gold.".into());
            return;
        }
        let buyer = p.name.clone();
        // Charge, take the goods off the listing, then hand them over. Doing
        // it in this order means a failure after this point can only ever
        // leave the item in the mail, never duplicated.
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        p.bag.gold -= total;
        self.send_gold(id);
        self.account_dirty = true;
        self.market_take(index, count);
        let bought = UserItem {
            count,
            ..listing.item.clone()
        };
        let item_name = self
            .data
            .items
            .get(&bought.info)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "item".into());
        self.deliver_item(id, bought, "Item Purchase", "You bought");
        // Pay the seller by mail, less tax, as Zircon does.
        let tax = tax_on(total);
        let paid = after_tax(total);
        let message = format!(
            "You have sold an item\n\nBuyer: {buyer}\nItem: {item_name} x{count}\n\
             Price: {} each\nSub Total: {total}\n\nTax: {tax} ({MARKET_TAX_PERCENT}%)\n\n\
             Total: {paid}",
            listing.price
        );
        self.market_mail(
            listing.account,
            "Listing Sale".into(),
            message,
            paid,
            Vec::new(),
        );
        let left = self.listing_count(index);
        // Tell the seller their listing shrank, if they are online.
        if let Some(seller_id) = self.online_account(listing.account) {
            self.send_to(
                seller_id,
                ServerMessage::MarketConsignChanged { index, count: left },
            );
            self.market_login(seller_id);
        }
        self.market_line(id, format!("Bought {item_name} x{count} for {total} gold."));
    }

    /// Remove `count` from a listing, deleting it when empty.
    fn market_take(&mut self, index: u32, count: u32) {
        let store = &mut self.market_store;
        if let Some(pos) = store.listings.iter().position(|l| l.index == index) {
            if store.listings[pos].item.count <= count {
                store.listings.remove(pos);
            } else {
                store.listings[pos].item.count -= count;
            }
        }
        store.save();
    }

    /// How many units a listing still holds (0 when it is gone).
    fn listing_count(&self, index: u32) -> u32 {
        self.market_store
            .listings
            .iter()
            .find(|l| l.index == index)
            .map(|l| l.item.count)
            .unwrap_or(0)
    }

    /// Hand an item to a player: into the bag when they are in a safe zone
    /// with room, otherwise by mail. Zircon does exactly this so a full bag
    /// can never eat the goods.
    fn deliver_item(&mut self, id: ObjectId, item: UserItem, subject: &str, what: &str) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        let name = self
            .data
            .items
            .get(&item.info)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "item".into());
        let o = &self.objects[&id];
        let room = o.in_safe_zone
            && o.player()
                .is_some_and(|p| p.bag.can_gain_item(&self.data, &item, p.max_bag));
        if room {
            self.gain_exact(id, item);
            return;
        }
        let count = item.count;
        self.market_mail(
            account,
            subject.into(),
            format!("{what} '{name} x{count}' and could not collect it here."),
            0,
            vec![item],
        );
    }

    /// Put an item into the bag keeping its identity: added stats and refine
    /// level survive, which `Bag::gain` (which builds a fresh item from the
    /// definition) would drop.
    fn gain_exact(&mut self, id: ObjectId, item: UserItem) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let changes = p.bag.gain_item(&self.data, item, &mut p.next_item_id);
        self.send_changes(id, changes);
    }

    /// Post a mail from the market itself. Mirrors `mail_send` but skips the
    /// sender's checks: the market is not a player.
    fn market_mail(
        &mut self,
        account: u32,
        subject: String,
        message: String,
        gold: u64,
        items: Vec<UserItem>,
    ) {
        let mail = {
            let store = &mut self.mail_store;
            store.next_index += 1;
            let mail = super::mail::Mail {
                index: store.next_index,
                sender: "Market Place".into(),
                date: crate::accounts::now_secs(),
                subject,
                message,
                opened: false,
                gold,
                items,
            };
            store.boxes.entry(account).or_default().push(mail.clone());
            store.save();
            mail
        };
        if let Some(target) = self.online_account(account) {
            self.send_to(target, ServerMessage::MailNew(super::mail::summary(&mail)));
        }
    }

    /// `ZIRCON_DEV_MARKET=<n>`: list the first `n` sellable items in the pack
    /// under a fake seller, so the board is not empty in a screenshot.
    pub fn dev_seed_market(&mut self, n: usize) {
        let mut defs: Vec<(i32, u64)> = self
            .data
            .items
            .values()
            .filter(|d| d.can_trade && d.item_type != 0)
            .map(|d| (d.index, (d.index as u64 % 40 + 1) * 250))
            .collect();
        defs.sort_by_key(|(i, _)| *i);
        let now = crate::accounts::now_secs();
        for (i, (info, price)) in defs.into_iter().take(n).enumerate() {
            let store = &mut self.market_store;
            store.next_index += 1;
            let index = store.next_index;
            // Every fifth row belongs to account 1, the first account the
            // server hands out, so a dev login sees the owner colouring, the
            // disabled Buy button and its own Consign list too.
            let mine = i % 5 == 4;
            store.listings.push(Listing {
                index,
                account: if mine { 1 } else { 0 },
                seller: if mine {
                    "You".to_string()
                } else {
                    ["Mira", "Jack", "Sabuk Trader", "Lin"][i % 4].to_string()
                },
                item: UserItem {
                    id: 900_000 + index,
                    info,
                    count: if i % 3 == 0 { 5 } else { 1 },
                    durability: 100,
                    max_durability: 100,
                    added: Vec::new(),
                    level: 0,
                    refinable: false,
                    non_refinable: false,
                    bound: false,
                    worthless: false,
                    locked: false,
                    special_repair_until: 0,
                    weapon_experience_tenths: 0,
                    accessory_experience: 0,
                    expires_at: None,
                    colour: 0,
                },
                price,
                message: "Fair price, no haggling.".into(),
                created: now,
            });
        }
        self.market_store.save();
    }
}
