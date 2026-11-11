//! Trading (Zircon `PlayerObject.Social` trade): request the player in the
//! cell in front who faces you, offer items and gold, both confirm; any
//! step, turn, death or disconnect closes it.

use super::*;
use crate::items::UserItem;
use mir_proto::{ChatKind, ItemInstance};

/// Zircon: at most 15 items per side.
pub const TRADE_ITEMS: usize = 15;

#[derive(Debug, Clone)]
pub struct Trade {
    pub partner: ObjectId,
    /// Offered cells: (grid, slot, count, item id). The id pins the item
    /// shown to the partner; a swap into the slot after they confirmed
    /// cancels the trade instead of delivering the swapped item.
    pub items: Vec<(Grid, u8, u32, u32)>,
    pub gold: u64,
    pub confirmed: bool,
}

impl World {
    fn trade_system(&mut self, id: ObjectId, text: &str) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text: text.to_string(),
            },
        );
    }

    fn player_at(&self, map: i32, loc: Point) -> Option<ObjectId> {
        self.maps
            .get(&map)?
            .objects_at(loc)
            .iter()
            .copied()
            .find(|o| self.objects.get(o).is_some_and(|o| o.player().is_some()))
    }

    /// Is `other` in front of `id`, facing back?
    fn facing_each_other(&self, id: ObjectId, other: ObjectId) -> bool {
        let (Some(a), Some(b)) = (self.objects.get(&id), self.objects.get(&other)) else {
            return false;
        };
        a.map == b.map
            && a.location.step(a.direction, 1) == b.location
            && b.direction == a.direction.opposite()
    }

    pub fn trade_request(&mut self, id: ObjectId) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if p.trade.is_some() {
            self.trade_system(id, "You are already trading.");
            return;
        }
        if p.trade_request.is_some() {
            self.trade_system(id, "You already have a trade request.");
            return;
        }
        let (map, front) = (o.map, o.location.step(o.direction, 1));
        let Some(target) = self.player_at(map, front) else {
            self.trade_system(id, "There is nobody in front of you to trade with.");
            return;
        };
        if !self.facing_each_other(id, target) {
            self.trade_system(id, "You need to face each other to trade.");
            return;
        }
        let t = &self.objects[&target];
        let tp = t.player().unwrap();
        let tname = tp.name.clone();
        if tp.trade.is_some() {
            self.trade_system(id, &format!("{tname} is already trading."));
            return;
        }
        if tp.trade_request.is_some() {
            self.trade_system(id, &format!("{tname} already has a trade request."));
            return;
        }
        if t.dead || o.dead {
            self.trade_system(id, "You cannot trade with the dead.");
            return;
        }
        // Zircon refuses trades between blocked accounts.
        if self.is_blocking(p.account, tp.account) {
            self.trade_system(id, &format!("{tname} is not trading with you."));
            return;
        }
        let from = p.name.clone();
        self.objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap()
            .trade_request = Some(id);
        self.send_to(target, ServerMessage::TradeRequest { from });
        self.trade_system(id, &format!("Trade request sent to {tname}."));
    }

    pub fn trade_response(&mut self, id: ObjectId, accept: bool) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let Some(requester) = p.trade_request.take() else {
            return;
        };
        if !accept || p.trade.is_some() {
            return;
        }
        let Some(r) = self.objects.get(&requester) else {
            return;
        };
        let Some(rp) = r.player() else {
            return;
        };
        if rp.trade.is_some() || r.dead || !self.facing_each_other(requester, id) {
            return;
        }
        let rname = rp.name.clone();
        let name = self.objects[&id].player().unwrap().name.clone();
        for (a, b, bname) in [(id, requester, rname), (requester, id, name)] {
            let p = self
                .objects
                .get_mut(&a)
                .and_then(|o| o.player_mut())
                .unwrap();
            p.trade = Some(Trade {
                partner: b,
                items: Vec::new(),
                gold: 0,
                confirmed: false,
            });
            self.send_to(a, ServerMessage::TradeOpen { name: bname });
        }
    }

    pub fn trade_add_item(&mut self, id: ObjectId, grid: Grid, slot: u8, count: u32) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        let Some(trade) = &p.trade else {
            return;
        };
        let partner = trade.partner;
        let refuse = |world: &mut World, why: &str| {
            world.trade_system(id, why);
        };
        if trade.items.len() >= TRADE_ITEMS {
            refuse(self, "You cannot offer more than 15 items.");
            return;
        }
        if grid == Grid::Storage && !self.in_safe_zone(o.map, o.location) {
            refuse(self, "You cannot access storage outside of a safe zone.");
            return;
        }
        let Some(item) = p.bag.grid(grid).get(slot as usize).cloned().flatten() else {
            refuse(self, "Nothing there.");
            return;
        };
        if count == 0 || count > item.count {
            refuse(self, "Not that many.");
            return;
        }
        let Some(def) = self.data.items.get(&item.info) else {
            return;
        };
        if item.bound || !def.can_trade {
            refuse(self, "That item cannot be traded.");
            return;
        }
        if trade
            .items
            .iter()
            .any(|(g, s, ..)| *g == grid && *s == slot)
        {
            refuse(self, "That item is already offered.");
            return;
        }
        let instance = ItemInstance {
            count,
            ..item.instance()
        };
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        p.trade
            .as_mut()
            .unwrap()
            .items
            .push((grid, slot, count, item.id));
        self.trade_unlock_both(id, partner);
        self.send_to(id, ServerMessage::TradeAddItem { grid, slot, count });
        self.send_to(partner, ServerMessage::TradeItemAdded { item: instance });
    }

    pub fn trade_add_gold(&mut self, id: ObjectId, gold: u64) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let Some(trade) = &mut p.trade else {
            return;
        };
        let partner = trade.partner;
        // Zircon: gold can only be raised, up to what you carry.
        if gold == 0 || gold <= trade.gold || gold > p.bag.gold {
            let current = trade.gold;
            self.send_to(id, ServerMessage::TradeAddGold { gold: current });
            return;
        }
        trade.gold = gold;
        self.trade_unlock_both(id, partner);
        self.send_to(id, ServerMessage::TradeAddGold { gold });
        self.send_to(partner, ServerMessage::TradeGoldAdded { gold });
    }

    /// A changed offer clears both confirmations (both must confirm the
    /// final offer).
    fn trade_unlock_both(&mut self, id: ObjectId, partner: ObjectId) {
        for x in [id, partner] {
            if let Some(t) = self
                .objects
                .get_mut(&x)
                .and_then(|o| o.player_mut())
                .and_then(|p| p.trade.as_mut())
            {
                if t.confirmed {
                    t.confirmed = false;
                    self.send_to(x, ServerMessage::TradeUnlock);
                }
            }
        }
    }

    pub fn trade_confirm(&mut self, id: ObjectId) {
        let Some(t) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.trade.as_mut())
        else {
            return;
        };
        t.confirmed = true;
        let partner = t.partner;
        let Some(pt) = self
            .objects
            .get(&partner)
            .and_then(|o| o.player())
            .and_then(|p| p.trade.as_ref())
        else {
            self.trade_close(id);
            return;
        };
        if !pt.confirmed {
            self.trade_system(id, "Waiting for your partner to confirm.");
            self.trade_system(partner, "Your partner is waiting for you to confirm.");
            return;
        }
        // Both confirmed: verify gold, items and space, then swap.
        let sides = [(id, partner), (partner, id)];
        let mut offers: Vec<(ObjectId, ObjectId, Vec<UserItem>, u64)> = Vec::new();
        for (a, b) in sides {
            let pa = self.objects[&a].player().unwrap();
            let ta = pa.trade.as_ref().unwrap();
            if ta.gold > pa.bag.gold {
                self.trade_system(a, "You do not have enough gold.");
                self.trade_system(b, "Your partner does not have enough gold.");
                self.trade_close(id);
                return;
            }
            let mut items = Vec::new();
            for (g, s, c, item_id) in &ta.items {
                match pa.bag.grid(*g).get(*s as usize).cloned().flatten() {
                    Some(it)
                        if it.id == *item_id
                            && it.count >= *c
                            && !it.bound
                            && self.data.items.get(&it.info).is_some_and(|d| d.can_trade) =>
                    {
                        items.push(UserItem { count: *c, ..it })
                    }
                    _ => {
                        self.trade_system(a, "Your offered items changed; trade cancelled.");
                        self.trade_system(b, "Your partner's items changed; trade cancelled.");
                        self.trade_close(id);
                        return;
                    }
                }
            }
            offers.push((a, b, items, ta.gold));
        }
        // Space (Zircon: CanGainItems without weight): free bag slots for
        // the incoming stacks.
        for (a, b, items, _) in &offers {
            let free = self.objects[b]
                .player()
                .unwrap()
                .bag
                .inventory
                .iter()
                .filter(|s| s.is_none())
                .count();
            if items.len() > free {
                self.trade_system(*b, "You do not have enough room for the trade.");
                self.trade_system(*a, "Your partner does not have enough room.");
                let p = self
                    .objects
                    .get_mut(b)
                    .and_then(|o| o.player_mut())
                    .unwrap();
                if let Some(t) = p.trade.as_mut() {
                    t.confirmed = false;
                }
                self.send_to(*b, ServerMessage::TradeUnlock);
                return;
            }
        }
        // Transfer.
        for (a, b, items, gold) in offers {
            let mut changes_a = Changed::new();
            {
                let pa = self
                    .objects
                    .get_mut(&a)
                    .and_then(|o| o.player_mut())
                    .unwrap();
                let cells = pa.trade.as_ref().unwrap().items.clone();
                for (g, s, c, _) in cells {
                    if let Some(ch) = pa.bag.take(g, s, c) {
                        changes_a.push(ch);
                    }
                }
                pa.bag.gold -= gold;
            }
            let mut changes_b = Changed::new();
            {
                let pb = self
                    .objects
                    .get_mut(&b)
                    .and_then(|o| o.player_mut())
                    .unwrap();
                for it in items {
                    let slot = pb.bag.inventory.iter().position(|s| s.is_none()).unwrap();
                    pb.next_item_id += 1;
                    let item = UserItem {
                        id: pb.next_item_id,
                        ..it
                    };
                    changes_b.push((Grid::Inventory, slot as u8, Some(item.instance())));
                    pb.bag.inventory[slot] = Some(item);
                }
                pb.bag.gold += gold;
            }
            self.send_changes(a, changes_a);
            self.send_changes(b, changes_b);
        }
        for x in [id, partner] {
            self.send_gold(x);
            self.refresh_stats(x, false);
            self.refresh_appearance(x);
            self.send_player_stats(x);
            self.trade_system(x, "Trade complete.");
        }
        self.trade_close(id);
    }

    /// Close a trade from either side (no-op when not trading).
    pub fn trade_close(&mut self, id: ObjectId) {
        let Some(t) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.trade.take())
        else {
            return;
        };
        self.send_to(id, ServerMessage::TradeClose);
        if let Some(p) = self
            .objects
            .get_mut(&t.partner)
            .and_then(|o| o.player_mut())
        {
            if p.trade.take().is_some() {
                self.send_to(t.partner, ServerMessage::TradeClose);
            }
        }
    }

    /// A leaving player: close its trade and drop requests it sent.
    pub(super) fn trade_forget(&mut self, id: ObjectId) {
        self.trade_close(id);
        for o in self.objects.values_mut() {
            if let Some(p) = o.player_mut() {
                if p.trade_request == Some(id) {
                    p.trade_request = None;
                }
            }
        }
    }
}
