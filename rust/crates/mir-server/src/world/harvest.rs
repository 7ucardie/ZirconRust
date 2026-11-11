//! Corpse harvesting, Zircon `PlayerObject.Harvest` and the `NeedHarvest`
//! half of `MonsterObject.Die`.
//!
//! A monster whose AI class registers `NeedHarvest` does not scatter its
//! drops on death. They are held on the corpse, keyed by the account that
//! earned them, and the killer has to butcher the body to get them out.
//! `HarvestCount` swings land on nothing first, so a chicken takes three
//! swings in all and a pig four.
//!
//! The corpse's bones are per viewer. Zircon decides this in
//! `MonsterObject.GetInfoPacket`: after a share has been harvested, players
//! still owed a share see flesh, while other viewers see a skeleton. Fresh,
//! untouched corpses initially show flesh to everyone.

use mir_proto::rules::HARVEST_TIME;
use mir_proto::{Direction, Point, ServerMessage};

use crate::items::UserItem;

use super::{Kind, ObjectId, World};

/// Zircon `Config.HarvestDuration`: a corpse with nothing left rots this
/// much sooner.
pub const HARVEST_DURATION: u64 = 5 * 60_000;

impl World {
    /// A deterministic corpse beside the player for headless client checks.
    pub(super) fn dev_harvest(&mut self, id: ObjectId) {
        if std::env::var_os("ZIRCON_DEV_HARVEST").is_none() {
            return;
        }
        let Some(def) = self
            .data
            .monsters
            .values()
            .filter(|d| d.ai == 1)
            .min_by_key(|d| d.index)
            .map(|d| d.index)
        else {
            return;
        };
        let o = &self.objects[&id];
        let (map, loc) = (o.map, o.location);
        let Some(cell) = Direction::ALL
            .iter()
            .map(|d| loc.step(*d, 1))
            .find(|p| !self.cell_blocked(map, *p, false))
        else {
            return;
        };
        let corpse = self.create_monster(def, map, cell, None, None, 0);
        self.objects
            .get_mut(&corpse)
            .unwrap()
            .monster_mut()
            .unwrap()
            .exp_owner = Some(id);
        self.monster_die(corpse, id);
    }

    /// Does this monster hold its drops until butchered?
    pub(super) fn needs_harvest(&self, monster: ObjectId) -> bool {
        self.objects
            .get(&monster)
            .and_then(|o| o.monster())
            .is_some_and(|m| {
                self.data
                    .monsters
                    .get(&m.def)
                    .is_some_and(|d| super::ai_profile::harvest_count(d.ai) > 0)
            })
    }

    /// Hold one account's drops on the corpse instead of dropping them
    /// (Zircon `Drops[owner.Character.Account] = drops`).
    pub(super) fn stash_harvest_drop(&mut self, monster: ObjectId, account: u32, item: UserItem) {
        if let Some(m) = self.objects.get_mut(&monster).and_then(|o| o.monster_mut()) {
            m.harvest_drops.entry(account).or_default().push(item);
        }
    }

    /// Whether this corpse should be drawn as bones for `account`: only
    /// harvestable bodies ever are, and only once they owe that account
    /// nothing (Zircon `GetInfoPacket`).
    pub(super) fn is_skeleton_for(&self, monster: ObjectId, account: u32) -> bool {
        self.objects
            .get(&monster)
            .filter(|o| o.dead)
            .and_then(|o| o.monster())
            .is_some_and(|m| {
                (self
                    .data
                    .monsters
                    .get(&m.def)
                    .is_some_and(|d| super::ai_profile::harvest_count(d.ai) > 0))
                    && m.harvested
                    && !m.harvest_drops.contains_key(&account)
            })
    }

    /// Zircon `PlayerObject.Harvest`: swing at the corpses in front.
    ///
    /// The swing itself always happens, and always costs the time; what it
    /// finds decides the rest. Corpses within the player's pickup radius of the cell
    /// ahead are considered, nearest first.
    pub fn harvest(&mut self, id: ObjectId, direction: Direction) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        // Zircon refuses while mounted, dead or mid-action.
        if o.dead || self.now < o.action_time || self.now < o.move_time || p.mounted {
            return;
        }
        let (map, loc, account, max_bag, pickup_radius) = (
            o.map,
            o.location,
            p.account,
            p.max_bag,
            o.stats.pickup_radius,
        );
        let o = self.objects.get_mut(&id).unwrap();
        o.direction = direction;
        o.action_time = self.now + HARVEST_TIME;
        self.events
            .push((id, ServerMessage::ObjectHarvest { id, direction }));

        let front = loc.step(direction, 1);
        let mut corpses: Vec<ObjectId> = Vec::new();
        for d in 0..=pickup_radius {
            for dy in -d..=d {
                for dx in -d..=d {
                    if dx.abs().max(dy.abs()) != d {
                        continue;
                    }
                    let cell = Point::new(front.x + dx, front.y + dy);
                    if let Some(m) = self.maps.get(&map) {
                        corpses.extend(
                            m.objects_at(cell)
                                .iter()
                                .copied()
                                .filter(|c| self.objects.get(c).is_some_and(|o| o.dead))
                                .filter(|c| {
                                    matches!(&self.objects[c].kind, Kind::Monster(m)
                                        if !m.harvest_drops.is_empty())
                                }),
                        );
                    }
                }
            }
        }

        // Someone else's kill: Zircon says so once, however many corpses.
        let mut owned_by_other = false;
        for corpse in corpses {
            let Some(m) = self
                .objects
                .get(&corpse)
                .and_then(|o| o.monster())
                .filter(|m| !m.harvest_drops.is_empty())
            else {
                continue;
            };
            if !m.harvest_drops.contains_key(&account) {
                owned_by_other = true;
                continue;
            }
            // The first `HarvestCount` swings only tenderise it.
            if m.harvest_count > 0 {
                let m = self
                    .objects
                    .get_mut(&corpse)
                    .and_then(|o| o.monster_mut())
                    .unwrap();
                m.harvest_count -= 1;
                continue;
            }
            self.take_harvest(id, corpse, account, max_bag);
        }
        if owned_by_other {
            self.say_system(id, "That is not yours to harvest.".into());
        }
    }

    /// Move what this corpse owes the account into the bag, as far as the
    /// bag allows. What does not fit stays on the body for the next swing.
    fn take_harvest(&mut self, id: ObjectId, corpse: ObjectId, account: u32, max_bag: i32) {
        let items = self
            .objects
            .get(&corpse)
            .and_then(|o| o.monster())
            .and_then(|m| m.harvest_drops.get(&account))
            .cloned()
            .unwrap_or_default();
        let mut left: Vec<UserItem> = Vec::new();
        let mut carried = false;
        for item in items {
            if item.info == self.data.gold_item {
                let gold = self.guild_tax_gold(id, item.count as u64);
                if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                    p.bag.gold += gold;
                }
                self.send_gold(id);
                self.say_system(id, format!("You harvested {} gold.", item.count));
                continue;
            }
            let p = self.objects[&id].player().unwrap();
            if !p.bag.can_gain_item(&self.data, &item, max_bag) {
                left.push(item);
                carried = true;
                continue;
            }
            let name = self
                .data
                .items
                .get(&item.info)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            let count = item.count;
            let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
            let mut next = p.next_item_id;
            let changes = p.bag.gain_item(&self.data, item, &mut next);
            p.next_item_id = next;
            self.send_changes(id, changes);
            self.say_system(
                id,
                if count > 1 {
                    format!("You harvested {name} ({count}).")
                } else {
                    format!("You harvested {name}.")
                },
            );
        }
        let empty = left.is_empty();
        if let Some(m) = self.objects.get_mut(&corpse).and_then(|o| o.monster_mut()) {
            if empty {
                m.harvest_drops.remove(&account);
            } else {
                m.harvest_drops.insert(account, left);
            }
        }
        if carried {
            self.say_system(id, "You cannot carry any more.".into());
        }
        if empty {
            self.harvest_changed(corpse);
        }
    }

    /// Zircon `MonsterObject.HarvestChanged`: the body is bones to everyone
    /// it no longer owes, and once it owes nobody it rots sooner.
    pub(super) fn harvest_changed(&mut self, corpse: ObjectId) {
        let Some(m) = self.objects.get_mut(&corpse).and_then(|o| o.monster_mut()) else {
            return;
        };
        m.harvested = true;
        if m.harvest_drops.is_empty() {
            m.dead_time = m.dead_time.saturating_sub(HARVEST_DURATION);
        }
        self.events
            .push((corpse, ServerMessage::ObjectHarvested { id: corpse }));
    }
}
