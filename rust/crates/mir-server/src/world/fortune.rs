//! Drop progress and the fortune checker (Zircon `UserDrop`,
//! `UserFortuneInfo` and `PlayerObject.FortuneCheck`).
//!
//! Every drop roll a player makes adds the roll's *expected* yield to that
//! account's progress for the item, whether or not the roll came in. The
//! roll itself is unchanged, but when the expectation has run a whole item
//! ahead of what actually dropped, the drop is forced -- so a long dry
//! streak on a 1-in-5000 item eventually pays out. Spending a Fortune
//! Checker item snapshots the numbers so the player can read them.

use mir_proto::{ChatKind, FortuneSummary, ObjectId, ServerMessage};

use super::World;

/// Zircon `ItemEffect.FortuneChecker`: the item a check is paid with.
pub const ITEM_EFFECT_FORTUNE_CHECKER: u8 = 54;

/// Running drop progress for one item on one account (Zircon `UserDrop`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DropProgress {
    pub item: i32,
    /// How many of this item have actually dropped for the account.
    pub drop_count: i64,
    /// Expected yield accumulated over every roll.
    pub progress: f64,
}

/// What a spent checker recorded (Zircon `UserFortuneInfo`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct FortuneRecord {
    pub item: i32,
    pub drop_count: i64,
    pub progress: f64,
    /// Unix seconds of the check.
    pub checked_at: u64,
}

impl World {
    /// Add one roll's expected yield to the killer's progress, and say
    /// whether the pity rule should force this drop through after a failed
    /// roll. `one_in` is the roll's denominator (Zircon `drop.Chance`
    /// times the number of players sharing it).
    ///
    /// Zircon adds `amount / one_in` per roll: the mean number of items the
    /// roll is worth. `part_only` rows are skipped, as they never drop the
    /// item itself.
    pub fn add_drop_progress(
        &mut self,
        killer: Option<ObjectId>,
        item: i32,
        amount: i32,
        one_in: i64,
        part_only: bool,
    ) -> bool {
        if one_in <= 0 {
            return false;
        }
        let Some(p) = killer
            .and_then(|k| self.objects.get_mut(&k))
            .and_then(|o| o.player_mut())
        else {
            return false;
        };
        let entry = match p.drops.iter().position(|d| d.item == item) {
            Some(i) => &mut p.drops[i],
            None => {
                p.drops.push(DropProgress {
                    item,
                    ..Default::default()
                });
                p.drops.last_mut().expect("just pushed")
            }
        };
        if !part_only {
            entry.progress += amount.max(1) as f64 / one_in as f64;
        }
        // Pity: the expectation has run a whole item ahead of reality.
        entry.progress.floor() as i64 > entry.drop_count
    }

    /// Record that `amount` of `item` actually dropped for the killer.
    pub fn add_drop_count(&mut self, killer: Option<ObjectId>, item: i32, amount: i64) {
        let Some(p) = killer
            .and_then(|k| self.objects.get_mut(&k))
            .and_then(|o| o.player_mut())
        else {
            return;
        };
        if let Some(d) = p.drops.iter_mut().find(|d| d.item == item) {
            d.drop_count += amount;
        }
    }

    /// Zircon `PlayerObject.FortuneCheck`: spend one Fortune Checker to
    /// snapshot this item's progress. Items nothing drops are refused, so
    /// the checker is not wasted.
    pub fn fortune_check(&mut self, id: ObjectId, item: i32) {
        let Some(checker) = self.fortune_checker_item() else {
            self.say_system(id, "Fortune checking is not available here.".into());
            return;
        };
        if !self.data.items.contains_key(&item) {
            return;
        }
        if !self.drops_any(item) {
            self.say_system(id, "Nothing drops that item.".into());
            return;
        }
        let has = self.objects[&id]
            .player()
            .is_some_and(|p| p.bag.count_of(checker) > 0);
        if !has {
            let name = self
                .data
                .items
                .get(&checker)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "Fortune Checker".into());
            self.say_system(id, format!("You need a {name}."));
            return;
        }
        let changes = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .map(|p| p.bag.take_info(checker, 1))
            .unwrap_or_default();
        self.send_changes(id, changes);
        let now = crate::accounts::now_secs();
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let (drop_count, progress) = p
            .drops
            .iter()
            .find(|d| d.item == item)
            .map(|d| (d.drop_count, d.progress))
            .unwrap_or((0, 0.0));
        let record = FortuneRecord {
            item,
            drop_count,
            progress,
            checked_at: now,
        };
        match p.fortunes.iter_mut().find(|f| f.item == item) {
            Some(f) => *f = record,
            None => p.fortunes.push(record),
        }
        self.send_fortunes(id);
    }

    /// Send every checked item's snapshot to the player.
    pub(super) fn send_fortunes(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let fortunes: Vec<FortuneSummary> = p
            .fortunes
            .iter()
            .map(|f| FortuneSummary {
                item: f.item,
                drop_count: f.drop_count,
                progress: f.progress,
                checked_at: f.checked_at,
            })
            .collect();
        self.send_to(id, ServerMessage::FortuneUpdate { fortunes });
    }

    /// The `ItemInfo` index of the Fortune Checker item, if the pack has one.
    pub fn fortune_checker_item(&self) -> Option<i32> {
        self.data
            .items
            .values()
            .find(|i| i.effect == ITEM_EFFECT_FORTUNE_CHECKER)
            .map(|i| i.index)
    }

    /// `ZIRCON_DEV_FORTUNES=<n>` pre-checks the first `n` droppable items
    /// with made-up progress, so the window has rows to show in a
    /// screenshot without grinding for them.
    pub(super) fn dev_fortunes(&mut self, id: ObjectId) {
        let Some(n) = std::env::var("ZIRCON_DEV_FORTUNES")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|n| *n > 0)
        else {
            return;
        };
        let mut items: Vec<i32> = Vec::new();
        for d in &self.data.drops {
            if d.chance > 0 && !items.contains(&d.item) {
                items.push(d.item);
            }
            if items.len() >= n {
                break;
            }
        }
        let now = crate::accounts::now_secs();
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        for (i, item) in items.into_iter().enumerate() {
            let drop_count = (i as i64 + 1) * 3;
            let progress = drop_count as f64 + (i as f64 + 1.0) * 0.17;
            p.drops.push(DropProgress {
                item,
                drop_count,
                progress,
            });
            p.fortunes.push(FortuneRecord {
                item,
                drop_count,
                progress,
                checked_at: now.saturating_sub(i as u64 * 3600),
            });
        }
    }

    /// A system line to one player (the fortune window's feedback, and the
    /// harvest messages).
    pub(super) fn say_system(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }

    /// Load an account's saved drop progress on entry.
    pub fn set_fortunes(
        &mut self,
        id: ObjectId,
        drops: &[DropProgress],
        fortunes: &[FortuneRecord],
    ) {
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.drops = drops.to_vec();
            p.fortunes = fortunes.to_vec();
        }
        // Seeding happens here, not on entry: the account load runs after
        // `add_player` and would otherwise wipe the seeded rows.
        self.dev_fortunes(id);
        self.send_fortunes(id);
    }

    /// Read a player's drop progress back out, to save on the account.
    pub fn fortunes_of(&self, id: ObjectId) -> Option<(Vec<DropProgress>, Vec<FortuneRecord>)> {
        let p = self.objects.get(&id)?.player()?;
        Some((p.drops.clone(), p.fortunes.clone()))
    }

    /// Whether any monster drops this item at all.
    fn drops_any(&self, item: i32) -> bool {
        self.data
            .drops
            .iter()
            .any(|d| d.item == item && d.chance > 0)
    }
}
