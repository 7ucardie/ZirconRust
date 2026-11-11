//! Fishing and mining (Zircon `PlayerObject.Movement` `FishingCast` and
//! `Mining`).
//!
//! Fishing: with a rod and robe on, cast at a fishing zone within throw
//! range; every recast (the client sends one per attack delay) rolls for a
//! nibble, then each reel attempt adds or removes points until 50 lands a
//! drop from the zone's table or 0 loses the fish. This asset pack has no
//! fishing zones, rods, robes or bait, so `ZIRCON_DEV_FISHING=<item>`
//! turns every unwalkable cell into a zone dropping that item and waives
//! the tackle.
//!
//! Mining: swing a pickaxe at an unwalkable cell of a `CanMine` map; each
//! `MineInfo` row rolls 1 in Chance for one ore; the pickaxe loses 4
//! durability and rubble piles up under the miner.

use super::*;
use mir_proto::rules::{attack_delay, ATTACK_TIME};
use mir_proto::{fishing_state, slot, spell_effect, ChatKind};

/// Zircon `Config` fishing values and `ItemEffect` ids.
pub const FISH_POINTS_REQUIRED: i32 = 50;
pub const FISH_NIBBLE_CHANCE_BASE: i32 = 10;
pub const EFFECT_PICK_AXE: u8 = 5;
pub const EFFECT_FISHING_ROD: u8 = 82;
pub const EFFECT_FISHING_ROBE: u8 = 83;
/// `Stat` ids of the fishing tackle bonuses.
pub const STAT_THROW_DISTANCE: i32 = 200;
pub const STAT_FLEXIBILITY: i32 = 202;
pub const STAT_FLOAT_STRENGTH: i32 = 203;
pub const STAT_REEL_BONUS: i32 = 204;
pub const STAT_NIBBLE_CHANCE: i32 = 205;
pub const STAT_FINDER_CHANCE: i32 = 206;
/// Zircon `Globals.AttackDelay` + 100: the client recasts within this.
pub const FISHING_RECAST: u64 = 1600;

#[derive(Debug, Clone)]
pub struct FishingData {
    pub direction: Direction,
    pub float: Point,
    pub found: bool,
    pub points: i32,
    pub throw_quality: i32,
    pub attempts: i32,
    pub fails: i32,
    /// The line goes slack if no recast arrives by then.
    pub cast_time: u64,
}

/// Zircon `Functions.ValidFishingDistance`.
fn valid_throw(distance: i32, level: i32) -> bool {
    match level {
        2 => distance <= 6,
        3 => distance <= 8,
        4 => distance <= 9,
        _ => distance <= 4,
    }
}

/// Zircon `Functions.FishingThrowQuality`.
fn throw_quality(distance: i32) -> i32 {
    match distance {
        5 | 6 => 2,
        7 | 8 => 3,
        9 => 4,
        _ => 1,
    }
}

impl World {
    fn line(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }

    /// Lower an equipped item's durability (Zircon `DamageItem` without the
    /// Strength save); tells the client and refreshes stats when it breaks.
    pub(super) fn damage_equipment(&mut self, id: ObjectId, slot: usize, rate: i32) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let Some(item) = p.bag.equipment.get_mut(slot).and_then(|c| c.as_mut()) else {
            return;
        };
        if item.max_durability <= 0 || item.durability <= 0 {
            return;
        }
        item.durability = (item.durability - rate).max(0);
        let broke = item.durability == 0;
        let instance = item.instance();
        self.send_to(
            id,
            ServerMessage::ItemChanged {
                grid: Grid::Equipment,
                slot: slot as u8,
                item: Some(instance),
            },
        );
        if broke {
            self.refresh_stats(id, false);
            self.refresh_appearance(id);
        }
    }

    fn equipped_effect(&self, id: ObjectId, slot: usize) -> Option<(u8, i32, i32)> {
        let p = self.objects.get(&id)?.player()?;
        let item = p.bag.equipment.get(slot)?.as_ref()?;
        let def = self.data.items.get(&item.info)?;
        Some((def.effect, item.durability, item.max_durability))
    }

    /// The fishing zone covering `float` on `map`: drops as (item, chance,
    /// throw quality, perfect only), sorted by chance descending.
    fn fishing_drops(&self, map: i32, float: Point) -> Option<Vec<(i32, i32, i32, bool)>> {
        let width = self.maps.get(&map)?.file.width as i32;
        for zone in &self.data.fishing_zones {
            let Some(region) = self.data.regions.get(&zone.region) else {
                continue;
            };
            if region.map != map || !region.points(width).contains(&(float.x, float.y)) {
                continue;
            }
            let mut drops: Vec<(i32, i32, i32, bool)> = self
                .data
                .fishing_drops
                .iter()
                .filter(|d| d.fishing == zone.index)
                .map(|d| (d.item, d.chance.max(1), d.throw_quality, d.perfect_catch))
                .collect();
            drops.sort_by_key(|d| std::cmp::Reverse(d.1));
            return Some(drops);
        }
        if let Some(item) = self.dev_fishing_item {
            let m = self.maps.get(&map)?;
            if !m.file.is_walkable(float.x, float.y) {
                return Some(vec![(item, 1, 0, false)]);
            }
        }
        None
    }

    pub fn fishing_cast(
        &mut self,
        id: ObjectId,
        mut state: u8,
        direction: Direction,
        float: Point,
        caught: bool,
    ) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if o.dead || self.now < o.action_time {
            return;
        }
        let (map, loc) = (o.map, o.location);
        let stats = p.bag.equipment_stats(&self.data, p.level, p.class);
        let stat = |k: i32| stats.get(&k).copied().unwrap_or(0);
        let distance = loc.distance(float);
        let Some(drops) = self.fishing_drops(map, float) else {
            self.line(id, "There are no fish there.".into());
            return;
        };
        if distance == 0 || !valid_throw(distance, stat(STAT_THROW_DISTANCE)) {
            self.line(id, "That is too far to cast.".into());
            return;
        }
        let dev = self.dev_fishing_item.is_some();
        if !dev {
            let rod = self.equipped_effect(id, slot::WEAPON);
            let robe = self.equipped_effect(id, slot::ARMOUR);
            if rod.map(|r| r.0) != Some(EFFECT_FISHING_ROD)
                || robe.map(|r| r.0) != Some(EFFECT_FISHING_ROBE)
            {
                self.line(id, "You need a fishing rod and robe.".into());
                return;
            }
            let (_, dur, max) = rod.unwrap();
            if dur > 0 || max == 0 {
                self.damage_equipment(id, slot::WEAPON, 1);
            } else {
                state = fishing_state::REEL;
            }
        }
        {
            let o = self.objects.get_mut(&id).unwrap();
            o.direction = direction;
            o.action_time = self.now + ATTACK_TIME;
            o.attack_time = self.now + attack_delay(0);
        }
        let mut stats_msg = None;
        if state == fishing_state::CAST {
            let now = self.now;
            let starting = self.objects[&id].player().unwrap().fishing.is_none();
            let mut accuracy = -1;
            if starting {
                let mut points = self.rng.random_range(0..FISH_POINTS_REQUIRED - 10);
                points += FISH_POINTS_REQUIRED.min(points * stat(STAT_FINDER_CHANCE) / 100);
                accuracy = 10 + stat(STAT_FLEXIBILITY).clamp(0, 15);
                let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
                p.fishing = Some(FishingData {
                    direction,
                    float,
                    found: false,
                    points,
                    throw_quality: throw_quality(distance),
                    attempts: 0,
                    fails: 0,
                    cast_time: now + FISHING_RECAST,
                });
                for s in [slot::HOOK, slot::FLOAT, slot::FINDER, slot::REEL] {
                    self.damage_equipment(id, s, 4);
                }
                if !dev && !self.use_bait(id) {
                    state = fishing_state::REEL;
                    self.line(id, "Not enough bait.".into());
                }
            } else {
                let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
                p.fishing.as_mut().unwrap().cast_time = now + FISHING_RECAST;
            }
            let nibble = FISH_NIBBLE_CHANCE_BASE + stat(STAT_NIBBLE_CHANCE);
            let roll = self.rng.random_range(0..100);
            let reel_gain = (2 + stat(STAT_REEL_BONUS)).clamp(2, 5);
            let fail_loss = (5 - stat(STAT_FLOAT_STRENGTH)).clamp(0, 5);
            let mut outcome = None;
            {
                let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
                let f = p.fishing.as_mut().unwrap();
                if !f.found && state == fishing_state::CAST {
                    f.found = roll < nibble.max(0);
                }
                if f.found && state == fishing_state::CAST {
                    f.attempts += 1;
                    if caught {
                        f.points += reel_gain;
                    } else {
                        f.points -= fail_loss;
                        f.fails += 1;
                    }
                    if f.points >= FISH_POINTS_REQUIRED {
                        state = fishing_state::REEL;
                        outcome = Some(Some(f.attempts > 1 && f.fails == 0));
                    } else if f.points <= 0 {
                        state = fishing_state::REEL;
                        outcome = Some(None);
                    }
                }
                stats_msg = Some(ServerMessage::FishingStats {
                    points: f.points,
                    required: FISH_POINTS_REQUIRED,
                    throw_quality: f.throw_quality,
                    accuracy,
                });
            }
            match outcome {
                Some(Some(perfect)) => {
                    if perfect {
                        self.line(id, "Perfect catch!".into());
                    }
                    let quality = self.objects[&id]
                        .player()
                        .unwrap()
                        .fishing
                        .as_ref()
                        .unwrap()
                        .throw_quality;
                    let mut caught_item = None;
                    for (item, chance, tq, perfect_only) in drops {
                        if (perfect_only && !perfect) || (tq != 0 && tq != quality) {
                            continue;
                        }
                        if self.rng.random_range(0..chance) != 0 {
                            continue;
                        }
                        let p = self.objects[&id].player().unwrap();
                        if !p.bag.can_gain(&self.data, item, 1, p.max_bag) {
                            continue;
                        }
                        caught_item = Some(item);
                        break;
                    }
                    match caught_item {
                        Some(item) => {
                            let name = self
                                .data
                                .items
                                .get(&item)
                                .map(|d| d.name.clone())
                                .unwrap_or_default();
                            let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
                            let mut next = p.next_item_id;
                            let changes = p.bag.gain(&self.data, item, 1, &mut next);
                            p.next_item_id = next;
                            self.send_changes(id, changes);
                            self.line(id, format!("You caught a {name}."));
                        }
                        None => self.line(id, "The fish got away.".into()),
                    }
                }
                Some(None) => self.line(id, "The fish got away.".into()),
                None => {}
            }
        }
        let (fdir, ffloat, found) = {
            let p = self.objects[&id].player().unwrap();
            p.fishing
                .as_ref()
                .map(|f| (f.direction, f.float, f.found))
                .unwrap_or((direction, float, false))
        };
        if let Some(m) = stats_msg {
            self.send_to(id, m);
        }
        if state != fishing_state::CAST {
            self.objects
                .get_mut(&id)
                .unwrap()
                .player_mut()
                .unwrap()
                .fishing = None;
        }
        self.events.push((
            id,
            ServerMessage::ObjectFishing {
                id,
                state,
                direction: fdir,
                float: ffloat,
                found,
            },
        ));
    }

    /// Zircon `UseBait`: one bait from the bait slot per cast.
    fn use_bait(&mut self, id: ObjectId) -> bool {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return false;
        };
        let Some(ch) = p.bag.take(Grid::Equipment, slot::BAIT as u8, 1) else {
            return false;
        };
        self.send_changes(id, vec![ch]);
        true
    }

    /// Stop fishing (a move, death or a missed recast); tells everyone.
    pub(super) fn fishing_cancel(&mut self, id: ObjectId) {
        let Some(f) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.fishing.take())
        else {
            return;
        };
        self.events.push((
            id,
            ServerMessage::ObjectFishing {
                id,
                state: fishing_state::CANCEL,
                direction: f.direction,
                float: f.float,
                found: false,
            },
        ));
    }

    /// Lines go slack when the client stops recasting.
    pub(super) fn process_fishing(&mut self) {
        let now = self.now;
        let stale: Vec<ObjectId> = self
            .players()
            .filter(|o| {
                o.player()
                    .and_then(|p| p.fishing.as_ref())
                    .is_some_and(|f| now > f.cast_time + FISHING_RECAST)
            })
            .map(|o| o.id)
            .collect();
        for id in stale {
            self.fishing_cancel(id);
        }
    }

    pub fn mining(&mut self, id: ObjectId, direction: Direction) {
        let Some(o) = self.objects.get_mut(&id) else {
            return;
        };
        if o.dead || self.now < o.action_time || self.now < o.attack_time {
            return;
        }
        o.direction = direction;
        o.action_time = self.now + ATTACK_TIME;
        let aspeed = o.player().map(|p| p.attack_speed).unwrap_or(0);
        o.attack_time = self.now + attack_delay(aspeed).max(800);
        let (map, loc) = (o.map, o.location);
        let front = loc.step(direction, 1);
        let can_mine = self.data.maps.get(&map).is_some_and(|m| m.can_mine)
            && self
                .maps
                .get(&map)
                .is_some_and(|m| !m.file.is_walkable(front.x, front.y));
        let mut result = false;
        if can_mine {
            if let Some((EFFECT_PICK_AXE, dur, max)) = self.equipped_effect(id, slot::WEAPON) {
                if dur > 0 || max == 0 {
                    self.damage_equipment(id, slot::WEAPON, 4);
                    let mines: Vec<(i32, i32)> = self
                        .data
                        .mines
                        .iter()
                        .filter(|m| m.map == map)
                        .map(|m| (m.item, m.chance.max(1)))
                        .collect();
                    for (item, chance) in mines {
                        if self.rng.random_range(0..chance) != 0 {
                            continue;
                        }
                        let p = self.objects[&id].player().unwrap();
                        if !p.bag.can_gain(&self.data, item, 1, p.max_bag) {
                            continue;
                        }
                        let name = self
                            .data
                            .items
                            .get(&item)
                            .map(|d| d.name.clone())
                            .unwrap_or_default();
                        let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
                        let mut next = p.next_item_id;
                        let changes = p.bag.gain(&self.data, item, 1, &mut next);
                        p.next_item_id = next;
                        self.send_changes(id, changes);
                        self.line(id, format!("You mined {name}."));
                    }
                    // Rubble piles up under the miner for a minute.
                    let rubble = self.maps[&map].objects_at(loc).iter().copied().find(|v| {
                        matches!(&self.objects[v].kind, Kind::Spell(s) if s.effect == spell_effect::RUBBLE)
                    });
                    match rubble {
                        Some(r) => {
                            if let Some(Kind::Spell(s)) =
                                self.objects.get_mut(&r).map(|o| &mut o.kind)
                            {
                                s.tick_time = self.now + 60_000;
                            }
                        }
                        None => {
                            self.spawn_spell(map, loc, spell_effect::RUBBLE, 1, 60_000, id, 0);
                        }
                    }
                    result = true;
                }
            }
        }
        self.events.push((
            id,
            ServerMessage::ObjectMining {
                id,
                direction,
                effect: result,
            },
        ));
    }
}
