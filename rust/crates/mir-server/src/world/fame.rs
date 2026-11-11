//! Fame titles (Zircon `FameInfo`, `PlayerObject.PromoteFame`): NPC pages
//! sell the next title in `Order` for Fame Points (the `CurrencyType.FP`
//! currency); a held title grants its buff stats permanently and pays its
//! item rewards once, and shows in the character window.

use super::*;
use crate::data::FameDef;

/// Zircon `CurrencyType.FP`.
pub const CURRENCY_FP: i32 = 4;

impl World {
    pub(super) fn fame_def(&self, index: i32) -> Option<&FameDef> {
        self.data.fames.iter().find(|f| f.index == index)
    }

    /// Zircon `GetNextFameTitle`: the first title ordered after the held one.
    pub(super) fn next_fame_title(&self, held: i32) -> Option<&FameDef> {
        let order = self.fame_def(held).map(|f| f.order).unwrap_or(-1);
        self.data
            .fames
            .iter()
            .filter(|f| f.order > order)
            .min_by_key(|f| f.order)
    }

    /// The Fame Point currency of this data pack.
    pub(super) fn fp_currency(&self) -> Option<i32> {
        self.data
            .currencies
            .iter()
            .find(|c| c.currency_type == CURRENCY_FP)
            .map(|c| c.index)
    }

    /// NPC check `CheckFame`: a next title exists and the player can pay it.
    pub(super) fn fame_check(&self, id: ObjectId) -> bool {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return false;
        };
        let Some(next) = self.next_fame_title(p.fame) else {
            return false;
        };
        let Some(fp) = self.fp_currency() else {
            return false;
        };
        p.currencies.get(&fp).copied().unwrap_or(0) >= next.cost as i64
    }

    /// NPC action `PromoteFame`.
    pub fn promote_fame(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some(next) = self.next_fame_title(p.fame).cloned() else {
            return;
        };
        let Some(fp) = self.fp_currency() else {
            return;
        };
        if p.currencies.get(&fp).copied().unwrap_or(0) < next.cost as i64 {
            return;
        }
        // Every reward must fit before anything changes hands.
        let gold_item = self.data.gold_item;
        let fits = next.rewards.iter().all(|(item, amount)| {
            *item == gold_item
                || p.bag
                    .can_gain(&self.data, *item, (*amount).max(0) as u32, p.max_bag)
        });
        if !fits {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: "You need more bag space for the title's rewards.".into(),
                },
            );
            return;
        }
        let mut changes = Changed::new();
        {
            let p = self
                .objects
                .get_mut(&id)
                .and_then(|o| o.player_mut())
                .unwrap();
            *p.currencies.entry(fp).or_insert(0) -= next.cost as i64;
            p.fame = next.index;
            for (item, amount) in &next.rewards {
                if *item == gold_item {
                    p.bag.gold += (*amount).max(0) as u64;
                } else {
                    let mut n = p.next_item_id;
                    changes.extend(
                        p.bag
                            .gain(&self.data, *item, (*amount).max(0) as u32, &mut n),
                    );
                    p.next_item_id = n;
                }
            }
        }
        self.send_changes(id, changes);
        self.send_gold(id);
        self.send_currencies(id);
        self.refresh_stats(id, false);
        self.send_player_stats(id);
        self.send_to(
            id,
            ServerMessage::Chat {
                text: format!("You are now known as {}.", next.name),
            },
        );
    }

    /// The held title's buff stats (stat id, amount), for `refresh_stats`.
    pub(super) fn fame_stats(&self, fame: i32) -> Vec<(i32, i32)> {
        self.fame_def(fame)
            .map(|f| f.stats.clone())
            .unwrap_or_default()
    }
}
