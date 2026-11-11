//! Rotating boss hunts from the legacy System.db EventInfo/EventTarget/EventAction
//! rows. One boss is available per two-hour UTC slot. The kill closes that
//! slot, while the original boss drop table still supplies equipment loot.

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

use mir_proto::ChatKind;
use serde::{Deserialize, Serialize};

use super::*;

const SLOT_SECONDS: u64 = 2 * 60 * 60;
const REWARD_GOLD: u64 = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventGrant {
    slot: u64,
    character: u32,
    gold: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct WorldEventState {
    slot: u64,
    completed: bool,
    #[serde(default)]
    legacy_progress: BTreeMap<i32, i32>,
    #[serde(default)]
    pending: Vec<EventGrant>,
    #[serde(skip)]
    boss: Option<ObjectId>,
    #[serde(skip)]
    legacy_spawned: HashSet<ObjectId>,
    #[serde(skip)]
    path: Option<PathBuf>,
    #[serde(skip)]
    failed_slot: Option<u64>,
    #[serde(skip)]
    persisted: bool,
}

impl WorldEventState {
    pub(super) fn load(path: PathBuf) -> Self {
        let mut state: Self = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        state.path = Some(path);
        state.persisted = true;
        state
    }

    fn save(&self) -> bool {
        let Some(path) = &self.path else { return true };
        let result = (|| -> anyhow::Result<()> {
            let data = serde_json::to_vec(self)?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, data)?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        })();
        if let Err(error) = result {
            tracing::error!(path = %path.display(), "saving world event: {error:#}");
            return false;
        }
        true
    }
}

impl World {
    fn world_event_for_slot(&self, slot: u64) -> Option<&crate::data::WorldEventDef> {
        let events = &self.data.world_events;
        (!events.is_empty()).then(|| &events[slot as usize % events.len()])
    }

    pub(super) fn process_world_events_at(&mut self, unix_seconds: u64) {
        let slot = unix_seconds / SLOT_SECONDS;
        if self.rotating_event.slot != slot {
            if let Some(boss) = self.rotating_event.boss.take() {
                self.remove_object(boss);
            }
            self.rotating_event.slot = slot;
            self.rotating_event.completed = false;
            self.rotating_event.failed_slot = None;
            self.rotating_event.persisted = self.rotating_event.save();
            if self.rotating_event.persisted {
                self.apply_pending_world_event_rewards_to_online();
            }
        }
        if self.rotating_event.completed && !self.rotating_event.persisted {
            self.rotating_event.persisted = self.rotating_event.save();
            if self.rotating_event.persisted {
                self.apply_pending_world_event_rewards_to_online();
            }
        }
        if self.rotating_event.completed || self.rotating_event.failed_slot == Some(slot) {
            return;
        }
        if let Some(id) = self.rotating_event.boss {
            if self.objects.contains_key(&id) {
                return;
            }
            self.rotating_event.boss = None;
        }
        let Some(event) = self.world_event_for_slot(slot).cloned() else {
            return;
        };
        let Some(respawn) = self
            .data
            .respawns
            .iter()
            .find(|r| r.index == event.boss_respawn)
            .cloned()
        else {
            self.rotating_event.failed_slot = Some(slot);
            return;
        };
        let Some((boss, map, location)) = self.spawn_event_respawn(&respawn) else {
            tracing::warn!("world event {} could not spawn", event.description);
            self.rotating_event.failed_slot = Some(slot);
            return;
        };
        self.rotating_event.boss = Some(boss);
        let name = &self.data.monsters[&respawn.monster].name;
        let map_name = &self.data.maps[&map].description;
        self.announce(format!(
            "World event: {}. Defeat {} in {} near {},{} within two hours. Reward: {} gold per nearby group member, plus boss drops. Type @event for status.",
            event.description, name, map_name, location.x, location.y, REWARD_GOLD
        ));
    }

    fn spawn_event_respawn(
        &mut self,
        respawn: &crate::data::RespawnDef,
    ) -> Option<(ObjectId, i32, Point)> {
        let region = self.data.regions.get(&respawn.region)?.clone();
        if !self.data.monsters.contains_key(&respawn.monster) {
            return None;
        }
        let map = region.map;
        if let Err(error) = self.ensure_map(map) {
            tracing::warn!("event spawn map {map}: {error}");
            return None;
        }
        let file = &self.maps[&map].file;
        let points: Vec<Point> = region
            .points(file.width as i32)
            .into_iter()
            .filter(|&(x, y)| file.is_walkable(x, y))
            .map(|(x, y)| Point::new(x, y))
            .collect();
        let mut location = None;
        for _ in 0..20 {
            if let Some(point) = self.random_point(&points) {
                if !self.cell_blocked(map, point, false) {
                    location = Some(point);
                    break;
                }
            }
        }
        let location = location.or_else(|| {
            points
                .iter()
                .copied()
                .find(|&point| !self.cell_blocked(map, point, false))
        })?;
        let id = self.create_monster(respawn.monster, map, location, None, None, 0);
        if let Some(monster) = self.objects.get_mut(&id).and_then(|o| o.monster_mut()) {
            monster.drop_set = respawn.drop_set;
        }
        Some((id, map, location))
    }

    /// Legacy System.db kill counter. Save the crossed threshold before its
    /// actions so a restart cannot duplicate announcements or monster spawns.
    pub(super) fn legacy_event_monster_died(&mut self, id: ObjectId, owner: Option<ObjectId>) {
        let (monster, drop_set, map, location, spawned) = {
            let object = &self.objects[&id];
            let monster = object.monster_ref();
            (
                monster.def,
                monster.drop_set,
                object.map,
                object.location,
                monster.spawn.is_some(),
            )
        };
        let event_spawned = self.rotating_event.legacy_spawned.remove(&id);
        if !spawned && self.rotating_event.boss != Some(id) && !event_spawned {
            return;
        }
        self.legacy_event_target_died(monster, drop_set, map, location, owner);
    }

    fn legacy_event_target_died(
        &mut self,
        monster: i32,
        drop_set: i32,
        map: i32,
        location: Point,
        owner: Option<ObjectId>,
    ) {
        let changes: Vec<_> = self
            .data
            .legacy_events
            .iter()
            .filter_map(|event| {
                let value: i32 = event
                    .targets
                    .iter()
                    .filter(|target| {
                        target.monster == monster && (drop_set & target.drop_set) == target.drop_set
                    })
                    .map(|target| target.value)
                    .sum();
                (value != 0).then_some((event.clone(), value))
            })
            .collect();
        for (event, value) in changes {
            let start = self
                .rotating_event
                .legacy_progress
                .get(&event.index)
                .copied()
                .unwrap_or(0);
            let end = start.saturating_add(value).clamp(0, event.max_value);
            if start == end {
                continue;
            }
            self.rotating_event.legacy_progress.insert(event.index, end);
            if !self.rotating_event.save() {
                self.rotating_event
                    .legacy_progress
                    .insert(event.index, start);
                continue;
            }
            for action in event
                .actions
                .iter()
                .filter(|action| start < action.trigger_value && action.trigger_value <= end)
            {
                match action.kind {
                    2 if !action.message.is_empty() => {
                        let players: Vec<_> = self
                            .players()
                            .filter(|player| player.map == action.map)
                            .map(|player| player.id)
                            .collect();
                        for player in players {
                            self.send_to(
                                player,
                                ServerMessage::Say {
                                    id: None,
                                    kind: ChatKind::System,
                                    text: action.message.clone(),
                                },
                            );
                        }
                    }
                    3 if !action.message.is_empty() => {
                        if let Some(owner) = owner {
                            self.send_to(
                                owner,
                                ServerMessage::Say {
                                    id: None,
                                    kind: ChatKind::System,
                                    text: action.message.clone(),
                                },
                            );
                        }
                    }
                    4 => {
                        let respawn = self
                            .data
                            .respawns
                            .iter()
                            .find(|respawn| respawn.index == action.respawn && respawn.event_spawn)
                            .cloned();
                        if let Some(respawn) = respawn {
                            if let Some((id, _, _)) = self.spawn_event_respawn(&respawn) {
                                self.rotating_event.legacy_spawned.insert(id);
                            }
                        }
                    }
                    5 if self.data.monsters.contains_key(&action.monster)
                        && self.maps.contains_key(&map) =>
                    {
                        let points: Vec<_> = (-10..=10)
                            .flat_map(|dy| {
                                (-10..=10)
                                    .map(move |dx| Point::new(location.x + dx, location.y + dy))
                            })
                            .filter(|&point| !self.cell_blocked(map, point, false))
                            .collect();
                        if let Some(point) = self.random_point(&points) {
                            self.create_monster(action.monster, map, point, None, None, 0);
                        }
                    }
                    // The installed EventAction rows use the pre-2024 C# enum:
                    // 7 recalls players on MapParameter1 into RegionParameter1;
                    // 8 sends them to their own bind region.
                    7 | 8 => self.legacy_event_move_players(action),
                    _ => {}
                }
            }
        }
    }

    fn legacy_event_move_players(&mut self, action: &crate::data::LegacyEventAction) {
        let players: Vec<_> = self
            .players()
            .filter(|player| player.map == action.map)
            .map(|player| player.id)
            .collect();
        for player in players {
            let region = if action.kind == 7 {
                action.region
            } else {
                self.objects[&player].player().unwrap().bind_region
            };
            if region == 0 {
                tracing::warn!("event action {} has no destination region", action.kind);
                continue;
            }
            if let Err(error) = self.go_to_region(player, region) {
                tracing::warn!(
                    "event action {} could not move player: {error:#}",
                    action.kind
                );
            }
        }
    }

    /// Called after a boss's ordinary experience and drop handling.
    pub(super) fn world_event_boss_died(&mut self, boss: ObjectId, owner: Option<ObjectId>) {
        if self.rotating_event.completed || self.rotating_event.boss != Some(boss) {
            return;
        }
        let event_name = self
            .world_event_for_slot(self.rotating_event.slot)
            .map(|event| event.description.clone())
            .unwrap_or_else(|| "World event".into());
        let boss_map = self.objects[&boss].map;
        let boss_location = self.objects[&boss].location;
        let recipients = owner
            .filter(|id| self.objects.get(id).is_some_and(|o| o.player().is_some()))
            .map(|id| {
                let group = self.group_sharers(id, boss_map, boss_location);
                if group.is_empty() {
                    vec![id]
                } else {
                    group
                }
            })
            .unwrap_or_default();
        let grants: Vec<EventGrant> = recipients
            .into_iter()
            .filter_map(|id| {
                let object = self.objects.get(&id)?;
                if object.dead
                    || object.map != boss_map
                    || object.location.distance(boss_location) > MAX_VIEW_RANGE
                {
                    return None;
                }
                Some(EventGrant {
                    slot: self.rotating_event.slot,
                    character: object.player()?.character,
                    gold: REWARD_GOLD,
                })
            })
            .collect();
        let rewarded = grants.len();
        self.rotating_event.completed = true;
        self.rotating_event.boss = None;
        self.rotating_event.pending.extend(grants);
        self.rotating_event.persisted = self.rotating_event.save();
        if self.rotating_event.persisted {
            self.apply_pending_world_event_rewards_to_online();
        }
        self.announce(format!(
            "World event complete: {event_name}. {rewarded} nearby player(s) earned {REWARD_GOLD} gold each."
        ));
    }

    fn apply_pending_world_event_rewards_to_online(&mut self) {
        let players: Vec<ObjectId> = self.players().map(|o| o.id).collect();
        for id in players {
            self.apply_pending_world_event_rewards(id);
        }
    }

    pub(super) fn apply_pending_world_event_rewards(&mut self, id: ObjectId) {
        if !self.rotating_event.persisted {
            return;
        }
        let Some(character) = self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.character)
        else {
            return;
        };
        let pending: Vec<EventGrant> = self
            .rotating_event
            .pending
            .iter()
            .filter(|grant| grant.character == character)
            .cloned()
            .collect();
        let mut changed = false;
        if let Some(player) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            for grant in pending {
                if !player.event_rewards.contains(&grant.slot) {
                    player.bag.gold = player.bag.gold.saturating_add(grant.gold);
                    player.event_rewards.push(grant.slot);
                    changed = true;
                }
            }
        }
        if changed {
            self.send_gold(id);
        }
    }

    pub(super) fn world_event_status(&mut self, id: ObjectId) {
        let now = crate::accounts::now_secs();
        let slot = now / SLOT_SECONDS;
        let Some(event) = self.world_event_for_slot(slot) else {
            return;
        };
        let text = if self.rotating_event.slot == slot && self.rotating_event.completed {
            format!(
                "World event: {} completed. Next event in {} min.",
                event.description,
                (SLOT_SECONDS - now % SLOT_SECONDS).div_ceil(60)
            )
        } else if let Some(boss) = self
            .rotating_event
            .boss
            .and_then(|id| self.objects.get(&id))
        {
            let map = &self.data.maps[&boss.map].description;
            format!("World event: {} in {map} near {},{}. {} min left; reward {REWARD_GOLD} gold plus boss drops.", event.description, boss.location.x, boss.location.y, (SLOT_SECONDS - now % SLOT_SECONDS).div_ceil(60))
        } else {
            format!("World event: {} is preparing.", event.description)
        };
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::test_character;

    fn assets() -> Option<PathBuf> {
        std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from)
    }

    #[test]
    fn event_save_creates_missing_parent_directories() {
        let dir = std::env::temp_dir().join(format!(
            "zircon-event-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("data").join("world_event.json");
        let mut state = WorldEventState::load(path.clone());
        state.slot = 42;
        state.completed = true;
        assert!(state.save());
        let saved = WorldEventState::load(path.clone());
        assert_eq!(saved.slot, 42);
        assert!(saved.completed);
        assert!(!path.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_event_catalog_has_five_matching_boss_spawns() {
        let Some(assets) = assets() else { return };
        let data = GameData::load(assets.join("../Database/System.db")).unwrap();
        assert_eq!(data.world_events.len(), 5);
        assert_eq!(data.legacy_events.len(), 7);
        assert_eq!(
            data.legacy_events
                .iter()
                .flat_map(|event| &event.actions)
                .filter(|action| action.kind == 7)
                .count(),
            5
        );
        assert_eq!(
            data.legacy_events
                .iter()
                .flat_map(|event| &event.actions)
                .filter(|action| action.kind == 8)
                .count(),
            5
        );
        assert!(data
            .legacy_events
            .iter()
            .any(|event| event.description == "Banyo"));
        assert!(data
            .legacy_events
            .iter()
            .any(|event| event.description == "Lost Lands 2nd"));
        for event in &data.world_events {
            let respawn = data
                .respawns
                .iter()
                .find(|r| r.index == event.boss_respawn)
                .unwrap();
            assert!(respawn.event_spawn);
            assert!(data.monsters.contains_key(&respawn.monster));
            assert!(data.regions.contains_key(&respawn.region));
        }
        let mut world = World::new(data, assets.join("Map"), None).with_seed(7);
        let mut previous = None;
        for slot in 1..=5 {
            world.process_world_events_at(slot * SLOT_SECONDS);
            if let Some(previous) = previous {
                assert!(!world.objects.contains_key(&previous));
            }
            let boss = world.rotating_event.boss.expect("scheduled boss spawned");
            assert!(!world.objects[&boss].dead);
            previous = Some(boss);
        }
    }

    #[test]
    fn legacy_recall_and_escape_move_players_through_installed_regions() {
        let Some(assets) = assets() else { return };
        let data = GameData::load(assets.join("../Database/System.db")).unwrap();
        let mut recall = data
            .legacy_events
            .iter()
            .flat_map(|event| &event.actions)
            .find(|action| action.kind == 7)
            .unwrap()
            .clone();
        let mut escape = data
            .legacy_events
            .iter()
            .flat_map(|event| &event.actions)
            .find(|action| action.kind == 8)
            .unwrap()
            .clone();
        assert!(data.regions.contains_key(&recall.region));
        let mut world = World::new(data, assets.join("Map"), None).with_seed(7);
        let player = world
            .add_player(1, 1, &test_character("EventTraveller"))
            .unwrap();
        let start_map = world.objects[&player].map;
        let bind_region = world.objects[&player].player().unwrap().bind_region;
        let bind_map = world.data.regions[&bind_region].map;
        recall.map = start_map;
        world.legacy_event_move_players(&recall);
        assert_eq!(
            world.objects[&player].map,
            world.data.regions[&recall.region].map
        );
        escape.map = world.objects[&player].map;
        world.legacy_event_move_players(&escape);
        assert_eq!(world.objects[&player].map, bind_map);
    }

    #[test]
    fn legacy_kills_fire_seal_spawn_once_and_resume_after_restart() {
        let Some(assets) = assets() else { return };
        let db = assets.join("../Database/System.db");
        let dir = std::env::temp_dir().join(format!(
            "zircon-legacy-events-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut world =
            World::new(GameData::load(&db).unwrap(), assets.join("Map"), None).with_seed(7);
        world.set_store_dir(&dir);
        world.rotating_event.legacy_progress.insert(1, 3749);
        assert!(world.rotating_event.save());
        let region = world.data.regions[&world
            .data
            .respawns
            .iter()
            .find(|r| r.monster == 79)
            .unwrap()
            .region]
            .clone();
        let map = region.map;
        world.ensure_map(map).unwrap();
        let width = world.maps[&map].file.width as i32;
        let location = region
            .points(width)
            .into_iter()
            .find(|&(x, y)| world.maps[&map].file.is_walkable(x, y))
            .map(|(x, y)| Point::new(x, y))
            .unwrap();
        world.legacy_event_target_died(79, 0, map, location, None);
        assert_eq!(world.rotating_event.legacy_progress[&1], 3750);
        let stage = world
            .objects
            .values()
            .find(|object| {
                object.map == map
                    && object.location.distance(location) <= 10
                    && object
                        .monster()
                        .is_some_and(|monster| monster.def == 80 && monster.spawn.is_none())
            })
            .unwrap()
            .id;
        world.legacy_event_monster_died(stage, None);
        assert_eq!(world.rotating_event.legacy_progress[&1], 3750);
        let mut restarted =
            World::new(GameData::load(&db).unwrap(), assets.join("Map"), None).with_seed(7);
        restarted.set_store_dir(&dir);
        assert_eq!(restarted.rotating_event.legacy_progress[&1], 3750);
        restarted.legacy_event_target_died(79, 0, map, location, None);
        assert_eq!(restarted.rotating_event.legacy_progress[&1], 3751);
        assert!(
            restarted.objects.is_empty(),
            "threshold action must not repeat"
        );

        // Shared monster IDs use DropSet masks to select the correct palace.
        restarted.legacy_event_target_died(133, 1, map, location, None);
        assert_eq!(restarted.rotating_event.legacy_progress[&2], 1);
        assert!(!restarted.rotating_event.legacy_progress.contains_key(&3));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_final_threshold_spawns_database_boss() {
        let Some(assets) = assets() else { return };
        let mut world = World::new(
            GameData::load(assets.join("../Database/System.db")).unwrap(),
            assets.join("Map"),
            None,
        )
        .with_seed(7);
        world.rotating_event.legacy_progress.insert(1, 14_999);
        world.legacy_event_target_died(79, 0, 0, Point::new(0, 0), None);
        assert_eq!(world.rotating_event.legacy_progress[&1], 15_000);
        let boss = world
            .objects
            .values()
            .find(|object| {
                object
                    .monster()
                    .is_some_and(|monster| monster.def == 81 && monster.spawn.is_none())
            })
            .unwrap()
            .id;
        world.legacy_event_monster_died(boss, None);
        assert_eq!(world.rotating_event.legacy_progress[&1], 0);
    }

    #[test]
    fn rotating_boss_restarts_without_duplicate_reward() {
        let Some(assets) = assets() else { return };
        let map_dir = assets.join("Map");
        let db = assets.join("../Database/System.db");
        let dir = std::env::temp_dir().join(format!(
            "zircon-world-event-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let slot = 123_456;
        let mut world = World::new(GameData::load(&db).unwrap(), &map_dir, None).with_seed(1);
        world.set_store_dir(&dir);
        world.process_world_events_at(slot * SLOT_SECONDS);
        let first = world.rotating_event.boss.expect("event boss spawned");
        world.process_world_events_at(slot * SLOT_SECONDS + 30);
        assert_eq!(world.rotating_event.boss, Some(first));

        world.process_world_events_at((slot + 1) * SLOT_SECONDS);
        assert!(!world.objects.contains_key(&first));
        let boss = world.rotating_event.boss.expect("next boss spawned");
        let (map, location) = {
            let monster = &world.objects[&boss];
            (monster.map, monster.location)
        };
        let mut character = test_character("EventTester");
        character.id = 1;
        character.map = world.data.maps[&map].file_name.clone();
        character.location = (-3..=3)
            .flat_map(|dy| (-3..=3).map(move |dx| Point::new(location.x + dx, location.y + dy)))
            .find(|&point| !world.cell_blocked(map, point, true))
            .expect("free cell near event boss");
        let before_save = character.clone();
        let player = world.add_player(1, 1, &character).unwrap();
        world.world_event_boss_died(boss, Some(player));
        assert_eq!(
            world.objects[&player].player().unwrap().bag.gold,
            REWARD_GOLD
        );
        world.world_event_boss_died(boss, Some(player));
        assert_eq!(
            world.objects[&player].player().unwrap().bag.gold,
            REWARD_GOLD
        );
        let mut after_save = character.clone();
        world.snapshot(player, &mut after_save);
        let after_save: crate::accounts::CharacterRecord =
            serde_json::from_slice(&serde_json::to_vec(&after_save).unwrap()).unwrap();
        assert_eq!(after_save.gold, REWARD_GOLD);
        assert_eq!(after_save.event_rewards, vec![slot + 1]);

        let mut restarted = World::new(GameData::load(&db).unwrap(), &map_dir, None).with_seed(1);
        restarted.set_store_dir(&dir);
        restarted.process_world_events_at((slot + 1) * SLOT_SECONDS + 60);
        assert!(restarted.rotating_event.completed);
        assert!(restarted.rotating_event.boss.is_none());
        // Crash before accounts.json: pending grant replays once.
        let replayed = restarted.add_player(2, 1, &before_save).unwrap();
        assert_eq!(
            restarted.objects[&replayed].player().unwrap().bag.gold,
            REWARD_GOLD
        );
        assert_eq!(
            restarted.objects[&replayed].player().unwrap().event_rewards,
            vec![slot + 1]
        );

        // Crash after accounts.json: Gold and claim marker reload together.
        let mut saved = World::new(GameData::load(&db).unwrap(), &map_dir, None).with_seed(1);
        saved.set_store_dir(&dir);
        let credited = saved.add_player(3, 1, &after_save).unwrap();
        assert_eq!(
            saved.objects[&credited].player().unwrap().bag.gold,
            REWARD_GOLD
        );
        assert_eq!(
            saved.objects[&credited].player().unwrap().event_rewards,
            vec![slot + 1]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nearby_group_members_get_reward_when_killer_is_dead() {
        let Some(assets) = assets() else { return };
        let data = GameData::load(assets.join("../Database/System.db")).unwrap();
        let mut world = World::new(data, assets.join("Map"), None).with_seed(3);
        world.process_world_events_at(SLOT_SECONDS);
        let boss = world.rotating_event.boss.unwrap();
        let (map, location) = {
            let monster = &world.objects[&boss];
            (monster.map, monster.location)
        };
        let make_character = |world: &World, id, name| {
            let mut character = test_character(name);
            character.id = id;
            character.map = world.data.maps[&map].file_name.clone();
            character.location = (-3..=3)
                .flat_map(|dy| (-3..=3).map(move |dx| Point::new(location.x + dx, location.y + dy)))
                .find(|&point| !world.cell_blocked(map, point, true))
                .unwrap();
            character
        };
        let killer = world
            .add_player(1, 1, &make_character(&world, 1, "Killer"))
            .unwrap();
        let mate = world
            .add_player(2, 2, &make_character(&world, 2, "Mate"))
            .unwrap();
        world.groups.insert(7, vec![killer, mate]);
        world
            .objects
            .get_mut(&killer)
            .unwrap()
            .player_mut()
            .unwrap()
            .group = Some(7);
        world
            .objects
            .get_mut(&mate)
            .unwrap()
            .player_mut()
            .unwrap()
            .group = Some(7);
        world.objects.get_mut(&killer).unwrap().dead = true;
        world.world_event_boss_died(boss, Some(killer));
        assert_eq!(world.objects[&killer].player().unwrap().bag.gold, 0);
        assert_eq!(world.objects[&mate].player().unwrap().bag.gold, REWARD_GOLD);
    }

    #[test]
    fn failed_event_save_defers_reward_until_grant_is_durable() {
        let Some(assets) = assets() else { return };
        let data = GameData::load(assets.join("../Database/System.db")).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "zircon-event-retry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut world = World::new(data, assets.join("Map"), None).with_seed(5);
        world.set_store_dir(&dir);
        world.process_world_events_at(SLOT_SECONDS);
        let boss = world.rotating_event.boss.unwrap();
        let (map, location) = {
            let monster = &world.objects[&boss];
            (monster.map, monster.location)
        };
        let mut character = test_character("RetryPlayer");
        character.id = 1;
        character.map = world.data.maps[&map].file_name.clone();
        character.location = (-3..=3)
            .flat_map(|dy| (-3..=3).map(move |dx| Point::new(location.x + dx, location.y + dy)))
            .find(|&point| !world.cell_blocked(map, point, true))
            .unwrap();
        let player = world.add_player(1, 1, &character).unwrap();
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        world.rotating_event.path = Some(blocked.join("world_event.json"));
        world.world_event_boss_died(boss, Some(player));
        assert_eq!(world.objects[&player].player().unwrap().bag.gold, 0);
        std::fs::remove_file(&blocked).unwrap();
        world.process_world_events_at(SLOT_SECONDS + 30);
        assert_eq!(
            world.objects[&player].player().unwrap().bag.gold,
            REWARD_GOLD
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
