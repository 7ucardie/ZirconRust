//! Castles (Zircon `ConquestWar`, `CastleLord`, `PlayerObject.GuildConquest`):
//! a guild asks to fight for a castle, the war opens at the castle's daily
//! start time for its duration, the castle lord stands in the objective
//! region and the guild that kills it takes the castle. On the castle map
//! during a war, everyone not in the same guild is an enemy (`AtWar`).
//! Ownership and requests persist in `guilds.json`.

use super::guilds::{has, ConquestRequest};
use super::*;
use mir_proto::{guild_permission, ChatKind};

const DAY: u64 = 86_400;

/// A castle war in progress (Zircon `ConquestWar`).
#[derive(Debug, Clone)]
pub struct Conquest {
    pub castle: i32,
    pub map: i32,
    /// Guilds allowed to hurt the lord (empty = any guild without a castle).
    pub participants: Vec<u32>,
    /// Server time when the war ends.
    pub ends_at: u64,
    pub lord: Option<ObjectId>,
}

impl World {
    fn castle_def(&self, index: i32) -> Option<&crate::data::CastleDef> {
        self.data.castles.iter().find(|c| c.index == index)
    }

    fn castle_owner_name(&self, castle: i32) -> String {
        self.guild_store
            .castle_owner(castle)
            .and_then(|g| self.guild_store.get(g))
            .map(|g| g.name.clone())
            .unwrap_or_default()
    }

    /// A system line to everyone online (Zircon broadcasts to every
    /// connection).
    pub(super) fn announce(&mut self, text: String) {
        let ids: Vec<ObjectId> = self.players().map(|o| o.id).collect();
        for id in ids {
            self.send_to(
                id,
                ServerMessage::Say {
                    id: None,
                    kind: ChatKind::System,
                    text: text.clone(),
                },
            );
        }
    }

    fn send_all(&mut self, msg: ServerMessage) {
        let ids: Vec<ObjectId> = self.players().map(|o| o.id).collect();
        for id in ids {
            self.send_to(id, msg.clone());
        }
    }

    /// Every castle and its owner, plus any war in progress (on entry).
    pub(super) fn castle_login(&mut self, id: ObjectId) {
        let castles: Vec<(i32, String, String)> = self
            .data
            .castles
            .iter()
            .map(|c| (c.index, c.name.clone(), self.castle_owner_name(c.index)))
            .collect();
        for (index, name, owner) in castles {
            self.send_to(id, ServerMessage::CastleInfo { index, name, owner });
        }
        if let Some(index) = self.conquest.as_ref().map(|c| c.castle) {
            self.send_to(id, ServerMessage::GuildConquestStarted { index });
        }
    }

    /// Tell everyone who owns what (after ownership changes).
    pub(super) fn castle_broadcast_all(&mut self) {
        let castles: Vec<(i32, String, String)> = self
            .data
            .castles
            .iter()
            .map(|c| (c.index, c.name.clone(), self.castle_owner_name(c.index)))
            .collect();
        for (index, name, owner) in castles {
            self.send_all(ServerMessage::CastleInfo { index, name, owner });
        }
    }

    /// Zircon `GuildConquest`: the leader of a castle-less guild without a
    /// pending request signs up for the castle's war the day after
    /// tomorrow (a day later once today's start time has passed), paying
    /// the castle's item if it asks for one.
    pub fn guild_request_conquest(&mut self, id: ObjectId, index: i32) {
        let Some((guild, permission, _)) = self.member_permission(id) else {
            self.guild_line(id, "You are not in a guild.".into());
            return;
        };
        if !has(permission, guild_permission::LEADER) {
            self.guild_line(id, "Only a guild leader can request a conquest.".into());
            return;
        }
        if self.guild_store.castle_of(guild).is_some() {
            self.guild_line(id, "Your guild already holds a castle.".into());
            return;
        }
        if self.guild_store.conquests.iter().any(|c| c.guild == guild) {
            self.guild_line(id, "Your guild has already requested a conquest.".into());
            return;
        }
        let Some(castle) = self.castle_def(index).cloned() else {
            self.guild_line(id, "There is no such castle.".into());
            return;
        };
        if self.conquest.is_some() {
            self.guild_line(id, "A conquest is already in progress.".into());
            return;
        }
        if castle.item != 0 {
            let has_item = self.objects[&id]
                .player()
                .is_some_and(|p| p.bag.count_of(castle.item) > 0);
            if !has_item {
                let item = self
                    .data
                    .items
                    .get(&castle.item)
                    .map(|i| i.name.clone())
                    .unwrap_or_else(|| format!("item {}", castle.item));
                self.guild_line(
                    id,
                    format!(
                        "You need {item} to request the conquest of {}.",
                        castle.name
                    ),
                );
                return;
            }
            let changes = self
                .objects
                .get_mut(&id)
                .and_then(|o| o.player_mut())
                .map(|p| p.bag.take_info(castle.item, 1))
                .unwrap_or_default();
            self.send_changes(id, changes);
        }
        let now = crate::accounts::now_secs();
        let mut war_day = now - now % DAY + 2 * DAY;
        if (now % DAY) as i64 >= castle.start_secs {
            war_day += DAY;
        }
        self.guild_store.conquests.push(ConquestRequest {
            castle: index,
            guild,
            war_day,
        });
        self.guild_store.save();
        let war_in_secs = (war_day as i64 + castle.start_secs) - now as i64;
        let mut told = self.guild_online(guild);
        if let Some(owner) = self.guild_store.castle_owner(index) {
            for m in self.guild_online(owner) {
                self.guild_line(
                    m,
                    format!("{} will be attacked at its next war.", castle.name),
                );
                told.push(m);
            }
        }
        for m in self.guild_online(guild) {
            self.guild_line(
                m,
                format!("Your guild will fight for {} at its next war.", castle.name),
            );
        }
        for m in told {
            self.send_to(m, ServerMessage::GuildConquestDate { index, war_in_secs });
        }
    }

    /// Castle wars open at their start time when someone signed up (the
    /// owner always defends); `ZIRCON_DEV_CONQUEST=1` opens the first
    /// castle's war for everyone as soon as the server is up.
    pub(super) fn process_conquests(&mut self) {
        if self.now < self.conquest_check {
            return;
        }
        self.conquest_check = self.now + 1000;
        if let Some(c) = &self.conquest {
            if self.now >= c.ends_at {
                self.end_conquest();
            } else {
                self.refresh_conquest_timers();
            }
            return;
        }
        if !self.dev_conquest_done && std::env::var_os("ZIRCON_DEV_CONQUEST").is_some() {
            self.dev_conquest_done = true;
            if let Some(index) = self.data.castles.first().map(|c| c.index) {
                self.start_conquest(index, true);
                return;
            }
        }
        let now = crate::accounts::now_secs();
        let today = now - now % DAY;
        let due: Vec<i32> = self
            .data
            .castles
            .iter()
            .filter(|c| {
                let start = today as i64 + c.start_secs;
                (now as i64) >= start
                    && (now as i64) < start + 60
                    && !self.conquests_started.contains(&(c.index, today))
            })
            .map(|c| c.index)
            .collect();
        for index in due {
            self.conquests_started.insert((index, today));
            self.start_conquest(index, false);
        }
    }

    /// Zircon `SEnvir.StartConquest` + `ConquestWar.StartWar`.
    pub fn start_conquest(&mut self, index: i32, forced: bool) -> bool {
        let Some(castle) = self.castle_def(index).cloned() else {
            return false;
        };
        if self.conquest.is_some() {
            return false;
        }
        let mut participants: Vec<u32> = Vec::new();
        if !forced {
            let now = crate::accounts::now_secs();
            let today = now - now % DAY;
            let (due, rest): (Vec<ConquestRequest>, Vec<ConquestRequest>) = self
                .guild_store
                .conquests
                .drain(..)
                .partition(|c| c.castle == index && c.war_day <= today);
            self.guild_store.conquests = rest;
            for c in &due {
                if self.guild_store.get(c.guild).is_some() {
                    participants.push(c.guild);
                }
            }
            self.guild_store.save();
            if participants.is_empty() {
                return false;
            }
            if let Some(owner) = self.guild_store.castle_owner(index) {
                participants.push(owner);
            }
        }
        if self.ensure_map(castle.map).is_err() {
            return false;
        }
        self.announce(format!("The conquest of {} has begun!", castle.name));
        self.send_all(ServerMessage::GuildConquestStarted { index });
        self.conquest = Some(Conquest {
            castle: index,
            map: castle.map,
            participants,
            ends_at: self.now + castle.duration_secs.max(1) as u64 * 1000,
            lord: None,
        });
        self.ping_castle_players(index);
        self.refresh_conquest_timers();
        // The lord waits in the objective region.
        let width = self.maps[&castle.map].file.width as i32;
        let spot = self
            .data
            .regions
            .get(&castle.objective_region)
            .map(|r| r.points(width))
            .and_then(|pts| {
                let pts: Vec<Point> = pts.into_iter().map(|(x, y)| Point::new(x, y)).collect();
                self.random_point(&pts)
            });
        if let (Some(spot), true) = (spot, self.data.monsters.contains_key(&castle.monster)) {
            let lord = self.create_monster(castle.monster, castle.map, spot, None, None, 0);
            if let Some(c) = self.conquest.as_mut() {
                c.lord = Some(lord);
            }
        }
        self.refresh_war_flags();
        true
    }

    /// Zircon `ConquestWar.PingPlayers`: everyone on the castle map who is
    /// not in the owner guild is moved to the attackers' spawn region.
    fn ping_castle_players(&mut self, index: i32) {
        let Some(castle) = self.castle_def(index).cloned() else {
            return;
        };
        let owner = self.guild_store.castle_owner(index);
        let width = self.maps[&castle.map].file.width as i32;
        let Some(points) = self
            .data
            .regions
            .get(&castle.attack_spawn_region)
            .map(|r| r.points(width))
        else {
            return;
        };
        if points.is_empty() {
            return;
        }
        let movers: Vec<ObjectId> = self
            .on_map(castle.map)
            .filter(|o| {
                o.player()
                    .is_some_and(|p| p.guild.is_none() || p.guild != owner)
            })
            .map(|o| o.id)
            .collect();
        for id in movers {
            let (x, y) = points[self.rng.random_range(0..points.len())];
            self.teleport_object(id, Point::new(x, y));
        }
    }

    /// Zircon `ConquestWar.EndWar`.
    pub(super) fn end_conquest(&mut self) {
        let Some(c) = self.conquest.take() else {
            return;
        };
        let name = self
            .castle_def(c.castle)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        self.announce(format!("The conquest of {name} has ended."));
        if let Some(lord) = c.lord.filter(|l| self.objects.contains_key(l)) {
            self.remove_object(lord);
        }
        self.send_all(ServerMessage::GuildConquestFinished { index: c.castle });
        let owner = self.castle_owner_name(c.castle);
        if !owner.is_empty() {
            self.announce(format!("{owner} holds {name}."));
        }
        self.ping_castle_players(c.castle);
        self.refresh_war_flags();
        // The war is over: clear the countdown everyone was watching.
        self.refresh_conquest_timers();
    }

    pub(super) fn is_castle_lord(&self, id: ObjectId) -> bool {
        self.conquest.as_ref().is_some_and(|c| c.lord == Some(id))
    }

    /// Zircon `CastleLord.Attacked`: only a guild member of a castle-less,
    /// participating guild hurts the lord, and every hit does 1 damage.
    pub(super) fn castle_lord_damage(&self, attacker: ObjectId) -> Option<i32> {
        let c = self.conquest.as_ref()?;
        let side = self.objects.get(&attacker)?.side()?;
        let guild = self.objects.get(&side)?.player()?.guild?;
        if self.guild_store.castle_of(guild).is_some() {
            return Some(0);
        }
        if !c.participants.is_empty() && !c.participants.contains(&guild) {
            return Some(0);
        }
        Some(1)
    }

    /// Zircon `CastleLord.Die`: the killer's guild takes the castle.
    pub(super) fn castle_lord_died(&mut self, killer: Option<ObjectId>) {
        let Some(c) = self.conquest.as_mut() else {
            return;
        };
        let index = c.castle;
        c.lord = None;
        let guild = killer
            .and_then(|k| self.objects.get(&k))
            .and_then(|o| o.side())
            .and_then(|s| self.objects.get(&s))
            .and_then(|o| o.player())
            .and_then(|p| p.guild);
        let Some(guild) = guild else {
            return;
        };
        self.castle_capture(index, guild);
        self.ping_castle_players(index);
    }

    /// Zircon `ApplyCastleBuff`: owners earn 10 % more experience.
    pub(super) fn castle_experience_bonus(&self, id: ObjectId) -> bool {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .and_then(|p| p.guild)
            .is_some_and(|g| self.guild_store.castle_of(g).is_some())
    }
}
