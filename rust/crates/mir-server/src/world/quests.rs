//! Quests (Zircon `PlayerObject.Quests` / `UserQuest`): accept at the start
//! NPC, progress on kills, item gathering and region visits, complete at the
//! finish NPC for rewards. See `docs/research/npc-quests-events.md`.
//!
//! Deviation: gather tasks credit on the kill roll instead of spawning a
//! quest item to pick up.

use super::*;
use crate::accounts::{now_secs, StoredQuest};
use crate::data::QuestDef;
use mir_proto::{NpcQuest, UserQuestSummary};

/// Zircon `QuestType`.
mod quest_type {
    pub const DAILY: i32 = 1;
    pub const WEEKLY: i32 = 2;
    pub const REPEATABLE: i32 = 3;
}

/// Zircon `QuestTaskType`.
mod task_type {
    pub const KILL_MONSTER: i32 = 0;
    pub const GAIN_ITEM: i32 = 1;
    pub const REGION: i32 = 2;
}

fn summary(q: &StoredQuest) -> UserQuestSummary {
    UserQuestSummary {
        quest: q.quest,
        track: q.track,
        completed: q.completed,
        selected_reward: q.selected_reward,
        tasks: q.tasks.clone(),
    }
}

/// All tasks present and at their required amount (Zircon `IsComplete`).
fn is_complete(q: &StoredQuest, def: &QuestDef) -> bool {
    def.tasks.iter().all(|t| {
        q.tasks
            .iter()
            .any(|(ti, amount)| *ti == t.index && *amount >= t.amount)
    })
}

impl World {
    /// Send the whole quest log (on entering the world), after dropping
    /// expired daily/weekly/repeatable entries.
    pub(super) fn send_quest_list(&mut self, id: ObjectId) {
        self.expire_quests(id);
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let list: Vec<UserQuestSummary> = p.quests.iter().map(summary).collect();
        self.send_to(id, ServerMessage::QuestList(list));
    }

    fn send_quest(&mut self, id: ObjectId, quest: i32) {
        let Some(q) = self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .and_then(|p| p.quests.iter().find(|q| q.quest == quest))
        else {
            return;
        };
        let s = summary(q);
        self.send_to(id, ServerMessage::QuestChanged(s));
        self.send_quest_markers(id);
    }

    /// Quest icons depend on the viewer's level and quest progress.
    pub(super) fn npc_quest_marker(&self, player: ObjectId, npc: ObjectId) -> u8 {
        let quests = self.npc_quests(player, npc);
        if quests.iter().any(|quest| quest.state == 2) {
            3
        } else if quests.iter().any(|quest| quest.state == 0) {
            1
        } else if quests.iter().any(|quest| quest.state == 1) {
            2
        } else {
            0
        }
    }

    pub(super) fn send_quest_markers(&mut self, player: ObjectId) {
        let Some(visible) = self
            .objects
            .get(&player)
            .map(|object| object.visible.clone())
        else {
            return;
        };
        for npc in visible {
            if matches!(
                self.objects.get(&npc).map(|object| &object.kind),
                Some(Kind::Npc(_))
            ) {
                let marker = self.npc_quest_marker(player, npc);
                self.send_to(player, ServerMessage::NpcQuestMarker { id: npc, marker });
            }
        }
    }

    /// Zircon `QuestCanAccept`.
    pub(super) fn quest_can_accept(&self, p: &PlayerData, def: &QuestDef) -> bool {
        if p.quests.iter().any(|q| q.quest == def.index) {
            return false;
        }
        def.requirements.iter().all(|r| match r.requirement {
            0 => p.level >= r.int1,
            1 => p.level <= r.int1,
            2 => !p.quests.iter().any(|q| q.quest == r.quest),
            3 => p.quests.iter().any(|q| q.quest == r.quest && q.completed),
            4 => !p.quests.iter().any(|q| q.quest == r.quest && q.completed),
            5 => r.class & (1 << p.class.mir_class()) != 0,
            _ => true,
        })
    }

    /// Quest lines for an NPC dialog: what this NPC can start or finish for
    /// the player (`NPCInfo.StartQuests` / `FinishQuests`).
    pub(super) fn npc_quests(&self, id: ObjectId, npc: ObjectId) -> Vec<NpcQuest> {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return Vec::new();
        };
        let Some(info) = self.objects.get(&npc).and_then(|o| match &o.kind {
            Kind::Npc(n) => Some(n.info),
            _ => None,
        }) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for def in self.data.quests.values() {
            if def.start_npc == info && self.quest_can_accept(p, def) {
                out.push(NpcQuest {
                    quest: def.index,
                    name: def.name.clone(),
                    state: 0,
                });
            } else if def.finish_npc == info {
                if let Some(q) = p
                    .quests
                    .iter()
                    .find(|q| q.quest == def.index && !q.completed)
                {
                    out.push(NpcQuest {
                        quest: def.index,
                        name: def.name.clone(),
                        state: if is_complete(q, def) { 2 } else { 1 },
                    });
                }
            }
        }
        out.sort_by_key(|q| (std::cmp::Reverse(q.state), q.quest));
        out
    }

    /// Zircon `QuestAccept`: only from the open dialog of the start NPC.
    pub fn quest_accept(&mut self, id: ObjectId, quest: i32) {
        let Some(def) = self.data.quests.get(&quest).cloned() else {
            return;
        };
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some((npc, _)) = p.npc else {
            return;
        };
        let at_start = matches!(self.objects.get(&npc).map(|o| &o.kind), Some(Kind::Npc(n)) if n.info == def.start_npc);
        if self.objects[&id].dead || !at_start || !self.quest_can_accept(p, &def) {
            return;
        }
        let entry = StoredQuest {
            quest,
            completed: false,
            track: true,
            selected_reward: 0,
            taken: now_secs(),
            completed_at: 0,
            tasks: Vec::new(),
        };
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.quests.push(entry);
        }
        self.send_quest(id, quest);
    }

    /// Zircon `QuestComplete`: rewards filtered by class; a choice reward
    /// needs `choice` to name it; everything must fit in the bag.
    pub fn quest_complete(&mut self, id: ObjectId, quest: i32, choice: i32) {
        let Some(def) = self.data.quests.get(&quest).cloned() else {
            return;
        };
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some((npc, _)) = p.npc else {
            return;
        };
        let at_finish = matches!(self.objects.get(&npc).map(|o| &o.kind), Some(Kind::Npc(n)) if n.info == def.finish_npc);
        let Some(q) = p.quests.iter().find(|q| q.quest == quest) else {
            return;
        };
        if self.objects[&id].dead || !at_finish || q.completed || !is_complete(q, &def) {
            return;
        }
        let class_bit = 1u8 << p.class.mir_class();
        let mut has_choice = false;
        let mut chosen = false;
        let mut grants: Vec<(i32, u32)> = Vec::new();
        for r in &def.rewards {
            if r.class & class_bit == 0 {
                continue;
            }
            if r.choice {
                has_choice = true;
                if r.index != choice {
                    continue;
                }
                chosen = true;
            }
            grants.push((r.item, r.amount.max(1) as u32));
        }
        if has_choice && !chosen {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: "Choose a reward first.".into(),
                },
            );
            return;
        }
        let max_bag = p.max_bag;
        let gold_item = self.data.gold_item;
        // Currency items (ItemType 34) go to the purse or the matching
        // CurrencyInfo (by DropItem), never into the bag.
        let currency_of = |item: i32| -> Option<i32> {
            if item == gold_item {
                return Some(0);
            }
            if self.data.items.get(&item).map(|i| i.item_type) != Some(34) {
                return None;
            }
            self.data
                .currencies
                .iter()
                .find(|c| c.drop_item == item)
                .map(|c| c.index)
        };
        // Experience "items" (ItemEffect 2) are granted as experience.
        let is_exp = |item: i32| self.data.items.get(&item).map(|i| i.effect) == Some(2);
        let room = grants
            .iter()
            .filter(|(item, _)| currency_of(*item).is_none() && !is_exp(*item))
            .all(|(item, count)| p.bag.can_gain(&self.data, *item, *count, max_bag));
        if !room {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: "You need more bag space for the reward.".into(),
                },
            );
            return;
        }
        let mut all_changes = Vec::new();
        let mut exp_gain: u64 = 0;
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            let mut next = p.next_item_id;
            for (item, count) in grants {
                if is_exp(item) {
                    exp_gain += count as u64;
                    continue;
                }
                match currency_of(item) {
                    Some(0) => {
                        p.bag.gold += count as u64;
                        continue;
                    }
                    Some(cur) => {
                        *p.currencies.entry(cur).or_insert(0) += count as i64;
                        continue;
                    }
                    None => {}
                }
                all_changes.extend(p.bag.gain(&self.data, item, count, &mut next));
            }
            p.next_item_id = next;
            if let Some(q) = p.quests.iter_mut().find(|q| q.quest == quest) {
                q.completed = true;
                q.track = false;
                q.completed_at = now_secs();
                q.selected_reward = if chosen { choice } else { 0 };
            }
        }
        self.send_changes(id, all_changes);
        self.send_gold(id);
        self.send_currencies(id);
        if exp_gain > 0 {
            self.gain_experience(id, exp_gain);
        }
        self.send_quest(id, quest);
    }

    pub fn quest_track(&mut self, id: ObjectId, quest: i32, track: bool) {
        if let Some(q) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| {
                p.quests
                    .iter_mut()
                    .find(|q| q.quest == quest && !q.completed)
            })
        {
            q.track = track;
        }
    }

    pub fn quest_abandon(&mut self, id: ObjectId, quest: i32) {
        let removed = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .map(|p| {
                let before = p.quests.len();
                p.quests.retain(|q| !(q.quest == quest && !q.completed));
                p.quests.len() != before
            })
            .unwrap_or(false);
        if removed {
            self.send_to(id, ServerMessage::QuestCancelled { quest });
            self.send_quest_markers(id);
        }
    }

    /// Kill and gather credit (Zircon `MonsterObject.Drop` quest loop):
    /// the first matching `QuestTaskMonsterDetails` row that passes its
    /// chance and map filter adds its amount.
    pub(super) fn quest_kill_progress(&mut self, killer: ObjectId, monster_def: i32, map: i32) {
        let Some(p) = self.objects.get(&killer).and_then(|o| o.player()) else {
            return;
        };
        let open: Vec<i32> = p
            .quests
            .iter()
            .filter(|q| !q.completed)
            .map(|q| q.quest)
            .collect();
        let mut changed = Vec::new();
        for quest in open {
            let Some(def) = self.data.quests.get(&quest).cloned() else {
                continue;
            };
            for task in &def.tasks {
                if task.task != task_type::KILL_MONSTER && task.task != task_type::GAIN_ITEM {
                    continue;
                }
                let mut count = 0;
                for d in &task.monsters {
                    if d.monster != monster_def || (d.map != 0 && d.map != map) {
                        continue;
                    }
                    if d.chance > 1 && self.rng.random_range(0..d.chance) != 0 {
                        continue;
                    }
                    count = d.amount;
                    break;
                }
                if count <= 0 {
                    continue;
                }
                let Some(p) = self.objects.get_mut(&killer).and_then(|o| o.player_mut()) else {
                    return;
                };
                let Some(q) = p.quests.iter_mut().find(|q| q.quest == quest) else {
                    continue;
                };
                let entry = match q.tasks.iter_mut().find(|(ti, _)| *ti == task.index) {
                    Some(e) => e,
                    None => {
                        q.tasks.push((task.index, 0));
                        q.tasks.last_mut().unwrap()
                    }
                };
                if entry.1 >= task.amount {
                    continue;
                }
                entry.1 = (entry.1 + count).min(task.amount);
                changed.push(quest);
            }
        }
        changed.dedup();
        for quest in changed {
            self.send_quest(killer, quest);
        }
    }

    /// Region tasks complete the moment the player steps into the region
    /// (Zircon `Cell.GetMovement`).
    pub(super) fn quest_region_progress(&mut self, id: ObjectId) {
        let Some((map, loc, open)) = self.objects.get(&id).and_then(|o| {
            o.player().map(|p| {
                (
                    o.map,
                    o.location,
                    p.quests
                        .iter()
                        .filter(|q| !q.completed)
                        .map(|q| q.quest)
                        .collect::<Vec<i32>>(),
                )
            })
        }) else {
            return;
        };
        if open.is_empty() {
            return;
        }
        let Some(width) = self.maps.get(&map).map(|m| m.file.width as i32) else {
            return;
        };
        let mut changed = Vec::new();
        for quest in open {
            let Some(def) = self.data.quests.get(&quest) else {
                continue;
            };
            for task in &def.tasks {
                if task.task != task_type::REGION || task.region == 0 {
                    continue;
                }
                let Some(region) = self.data.regions.get(&task.region) else {
                    continue;
                };
                if region.map != map || !region.points(width).contains(&(loc.x, loc.y)) {
                    continue;
                }
                if let Some(q) = self
                    .objects
                    .get_mut(&id)
                    .and_then(|o| o.player_mut())
                    .and_then(|p| p.quests.iter_mut().find(|q| q.quest == quest))
                {
                    let done = q.tasks.iter().any(|(ti, a)| *ti == task.index && *a >= 1);
                    if !done {
                        q.tasks.retain(|(ti, _)| *ti != task.index);
                        q.tasks.push((task.index, 1));
                        changed.push(quest);
                    }
                }
            }
        }
        for quest in changed {
            self.send_quest(id, quest);
        }
    }

    /// Zircon `ProcessQuests`: completed daily/weekly quests fall off on the
    /// next UTC day/week, repeatable ones as soon as they are done.
    fn expire_quests(&mut self, id: ObjectId) {
        let now = now_secs();
        let day = now / 86_400;
        let week = (day + 3) / 7;
        let mut removed = Vec::new();
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.quests.retain(|q| {
                if !q.completed {
                    return true;
                }
                let kind = self
                    .data
                    .quests
                    .get(&q.quest)
                    .map(|d| d.quest_type)
                    .unwrap_or(0);
                let done_day = q.completed_at / 86_400;
                let expired = match kind {
                    quest_type::DAILY => done_day != day,
                    quest_type::WEEKLY => (done_day + 3) / 7 != week,
                    quest_type::REPEATABLE => true,
                    _ => false,
                };
                if expired {
                    removed.push(q.quest);
                }
                !expired
            });
        }
        for quest in removed {
            self.send_to(id, ServerMessage::QuestCancelled { quest });
        }
        self.send_quest_markers(id);
    }
}
