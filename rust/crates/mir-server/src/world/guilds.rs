//! Guilds (Zircon `PlayerObject.Social` guild section): create for gold,
//! invite with the AddMember permission, ranks and permissions edited by
//! leaders, notice, tax on picked-up gold feeding the funds, member limit
//! bought from the funds. Persisted in `guilds.json` under the data dir.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::*;
use mir_proto::{guild_permission, ChatKind, GuildMemberSummary, GuildSummary};

/// Zircon `Globals`: creation 7,500,000 gold plus 1,000,000 per member
/// slot; names 2-15 alphanumerics; notice up to 4000 chars; a war costs
/// 200,000 from the funds and lasts two hours.
pub const GUILD_CREATION_COST: u64 = 7_500_000;
pub const GUILD_WAR_COST: u64 = 200_000;
pub const GUILD_WAR_SECS: u64 = 2 * 60 * 60;
pub const GUILD_MEMBER_COST: u64 = 1_000_000;
pub const MAX_NOTICE: usize = 4000;
pub const MAX_MEMBER_LIMIT: i32 = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildMember {
    pub index: u32,
    /// Character id (Zircon keys members by account).
    pub character: u32,
    pub name: String,
    pub rank: String,
    pub permission: i32,
    pub joined: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guild {
    pub id: u32,
    pub name: String,
    pub notice: String,
    pub member_limit: i32,
    pub funds: i64,
    /// Percent of picked-up gold paid to the guild.
    pub tax: i32,
    pub default_rank: String,
    pub default_permission: i32,
    pub next_index: u32,
    pub members: Vec<GuildMember>,
}

/// A guild war (Zircon `GuildWarInfo`): two hours from the declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildWar {
    pub guild1: u32,
    pub guild2: u32,
    /// Unix seconds when it ends.
    pub ends_at: u64,
}

/// Which guild holds a castle (Zircon `GuildInfo.Castle`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastleOwner {
    pub castle: i32,
    pub guild: u32,
}

/// A guild signed up for a castle's next war (Zircon `UserConquest`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConquestRequest {
    pub castle: i32,
    pub guild: u32,
    /// Unix seconds of the midnight before the war.
    pub war_day: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GuildStore {
    pub guilds: Vec<Guild>,
    pub next_id: u32,
    #[serde(default)]
    pub wars: Vec<GuildWar>,
    #[serde(default)]
    pub castles: Vec<CastleOwner>,
    #[serde(default)]
    pub conquests: Vec<ConquestRequest>,
    #[serde(skip)]
    path: Option<PathBuf>,
    /// Funds changed by a taxed pickup; flushed once per tick, not per pickup.
    #[serde(skip)]
    pub dirty: bool,
}

impl GuildStore {
    pub(super) fn load(path: PathBuf) -> GuildStore {
        let mut store: GuildStore = super::load_json(&path);
        store.path = Some(path);
        store
    }

    pub(super) fn save(&self) {
        if let Some(p) = &self.path {
            super::save_json(p, self);
        }
    }

    pub fn get(&self, id: u32) -> Option<&Guild> {
        self.guilds.iter().find(|g| g.id == id)
    }

    fn get_mut(&mut self, id: u32) -> Option<&mut Guild> {
        self.guilds.iter_mut().find(|g| g.id == id)
    }

    /// The guild a character belongs to.
    /// The guild that owns a castle.
    pub fn castle_owner(&self, castle: i32) -> Option<u32> {
        self.castles
            .iter()
            .find(|c| c.castle == castle)
            .map(|c| c.guild)
    }

    /// The castle a guild owns.
    pub fn castle_of(&self, guild: u32) -> Option<i32> {
        self.castles
            .iter()
            .find(|c| c.guild == guild)
            .map(|c| c.castle)
    }

    /// Guilds `guild` is at war with.
    pub fn enemies_of(&self, guild: u32) -> Vec<u32> {
        self.wars
            .iter()
            .filter_map(|w| {
                if w.guild1 == guild {
                    Some(w.guild2)
                } else if w.guild2 == guild {
                    Some(w.guild1)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn of_character(&self, character: u32) -> Option<u32> {
        self.guilds
            .iter()
            .find(|g| g.members.iter().any(|m| m.character == character))
            .map(|g| g.id)
    }
}

pub(super) fn has(permission: i32, bit: i32) -> bool {
    if bit == guild_permission::LEADER {
        return permission == guild_permission::LEADER;
    }
    permission == guild_permission::LEADER || permission & bit != 0
}

fn valid_name(name: &str) -> bool {
    (2..=15).contains(&name.chars().count()) && name.chars().all(|c| c.is_ascii_alphanumeric())
}

impl World {
    pub(super) fn guild_line(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }

    pub(super) fn guild_of(&self, id: ObjectId) -> Option<u32> {
        self.objects.get(&id)?.player()?.guild
    }

    fn character_of(&self, id: ObjectId) -> Option<u32> {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.character)
    }

    /// Online player object for a character id.
    fn online_character(&self, character: u32) -> Option<ObjectId> {
        self.players()
            .find(|o| o.player().is_some_and(|p| p.character == character))
            .map(|o| o.id)
    }

    /// Online members of a guild.
    pub(super) fn guild_online(&self, guild: u32) -> Vec<ObjectId> {
        self.players()
            .filter(|o| o.player().is_some_and(|p| p.guild == Some(guild)))
            .map(|o| o.id)
            .collect()
    }

    fn guild_summary(
        &self,
        guild: u32,
        viewer: ObjectId,
        online: &HashSet<u32>,
    ) -> Option<GuildSummary> {
        let g = self.guild_store.get(guild)?;
        let me = self.character_of(viewer)?;
        Some(GuildSummary {
            name: g.name.clone(),
            notice: g.notice.clone(),
            member_limit: g.member_limit,
            funds: g.funds,
            tax: g.tax,
            default_rank: g.default_rank.clone(),
            default_permission: g.default_permission,
            user_index: g
                .members
                .iter()
                .find(|m| m.character == me)
                .map(|m| m.index)
                .unwrap_or(0),
            members: g
                .members
                .iter()
                .map(|m| GuildMemberSummary {
                    index: m.index,
                    name: m.name.clone(),
                    rank: m.rank.clone(),
                    permission: m.permission,
                    online: online.contains(&m.character),
                })
                .collect(),
            castle: self
                .guild_store
                .castle_of(guild)
                .and_then(|c| self.data.castles.iter().find(|d| d.index == c))
                .map(|d| d.name.clone())
                .unwrap_or_default(),
            wars: {
                let now = crate::accounts::now_secs();
                self.guild_store
                    .wars
                    .iter()
                    .filter(|w| w.guild1 == guild || w.guild2 == guild)
                    .filter_map(|w| {
                        let other = if w.guild1 == guild {
                            w.guild2
                        } else {
                            w.guild1
                        };
                        let name = self.guild_store.get(other)?.name.clone();
                        Some((name, w.ends_at.saturating_sub(now)))
                    })
                    .collect()
            },
        })
    }

    /// Zircon `GuildWar`: StartWar permission, a real other guild, no war
    /// yet, 200,000 from the funds; two hours; both guilds told.
    pub fn guild_war(&mut self, id: ObjectId, name: String) {
        let Some((guild, permission, _)) = self.member_permission(id) else {
            self.guild_line(id, "You are not in a guild.".into());
            return;
        };
        if !has(permission, guild_permission::START_WAR) {
            self.guild_line(id, "You do not have permission to start a war.".into());
            return;
        }
        let Some(target) = self
            .guild_store
            .guilds
            .iter()
            .find(|g| g.name.eq_ignore_ascii_case(&name))
            .map(|g| g.id)
        else {
            self.guild_line(id, format!("Could not find the guild {name}."));
            return;
        };
        let tname = self.guild_store.get(target).unwrap().name.clone();
        if target == guild {
            self.guild_line(id, "You cannot declare war on your own guild.".into());
            return;
        }
        if self.guild_store.enemies_of(guild).contains(&target) {
            self.guild_line(id, format!("You are already at war with {tname}."));
            return;
        }
        {
            let g = self.guild_store.get_mut(guild).unwrap();
            if g.funds < GUILD_WAR_COST as i64 {
                self.guild_line(id, "The guild cannot afford a war.".into());
                return;
            }
            g.funds -= GUILD_WAR_COST as i64;
        }
        let ends_at = crate::accounts::now_secs() + GUILD_WAR_SECS;
        self.guild_store.wars.push(GuildWar {
            guild1: guild,
            guild2: target,
            ends_at,
        });
        self.guild_store.save();
        let gname = self.guild_store.get(guild).unwrap().name.clone();
        for (side, enemy) in [(guild, tname), (target, gname)] {
            for m in self.guild_online(side) {
                self.send_to(
                    m,
                    ServerMessage::GuildWarStarted {
                        guild: enemy.clone(),
                        duration_secs: GUILD_WAR_SECS,
                    },
                );
            }
            self.guild_broadcast_info(side);
        }
        self.refresh_war_flags();
    }

    /// Wars end on their clock; both sides hear about it.
    pub(super) fn process_guild_wars(&mut self) {
        if self.guild_store.wars.is_empty() {
            return;
        }
        let now = crate::accounts::now_secs();
        let over: Vec<GuildWar> = self
            .guild_store
            .wars
            .iter()
            .filter(|w| now >= w.ends_at)
            .cloned()
            .collect();
        if over.is_empty() {
            return;
        }
        self.guild_store.wars.retain(|w| now < w.ends_at);
        self.guild_store.save();
        for w in over {
            let names = (
                self.guild_store
                    .get(w.guild1)
                    .map(|g| g.name.clone())
                    .unwrap_or_default(),
                self.guild_store
                    .get(w.guild2)
                    .map(|g| g.name.clone())
                    .unwrap_or_default(),
            );
            for (side, enemy) in [(w.guild1, names.1), (w.guild2, names.0)] {
                for m in self.guild_online(side) {
                    self.send_to(
                        m,
                        ServerMessage::GuildWarFinished {
                            guild: enemy.clone(),
                        },
                    );
                }
                self.guild_broadcast_info(side);
            }
        }
        self.refresh_war_flags();
    }

    /// Recompute every online player's war flags: the guilds they are at
    /// war with and whether they stand on a map under conquest (Zircon
    /// `AtWar`, cached so `hostile_to` needs no world access).
    pub(super) fn refresh_war_flags(&mut self) {
        let conquest_map = self.conquest.as_ref().map(|c| c.map);
        let ids: Vec<ObjectId> = self.players().map(|o| o.id).collect();
        for id in ids {
            let o = self.objects.get_mut(&id).unwrap();
            let map = o.map;
            let Some(p) = o.player_mut() else {
                continue;
            };
            p.war_guilds = p
                .guild
                .map(|g| self.guild_store.enemies_of(g))
                .unwrap_or_default();
            p.conquest_map = conquest_map == Some(map);
        }
    }

    /// Zircon `GuildWarDeath`: both guilds hear who fell to whom.
    pub(super) fn guild_war_death(&mut self, victim: ObjectId, killer: ObjectId) {
        let (Some(vg), Some(kg)) = (self.guild_of(victim), self.guild_of(killer)) else {
            return;
        };
        let vname = self.player_name(victim);
        let kname = self.player_name(killer);
        let vg_name = self
            .guild_store
            .get(vg)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let kg_name = self
            .guild_store
            .get(kg)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let line = format!("{vname} of {vg_name} was killed by {kname} of {kg_name}.");
        let mut members = self.guild_online(vg);
        if vg != kg {
            members.extend(self.guild_online(kg));
        }
        for m in members {
            self.guild_line(m, line.clone());
        }
    }

    /// Send the full guild view to every online member (after any change).
    pub(super) fn guild_broadcast_info(&mut self, guild: u32) {
        let online: HashSet<u32> = self
            .players()
            .filter_map(|o| self.character_of(o.id))
            .collect();
        for m in self.guild_online(guild) {
            let info = self.guild_summary(guild, m, &online);
            self.send_to(m, ServerMessage::GuildInfo(info));
        }
    }

    /// Guild name and rank shown under the player's name.
    pub(super) fn guild_tag(&self, p: &PlayerData) -> (String, String) {
        p.guild
            .and_then(|g| self.guild_store.get(g))
            .map(|g| {
                let rank = g
                    .members
                    .iter()
                    .find(|m| m.character == p.character)
                    .map(|m| m.rank.clone())
                    .unwrap_or_default();
                (g.name.clone(), rank)
            })
            .unwrap_or_default()
    }

    /// On entry: resolve membership, send the guild view, tell the others.
    pub(super) fn guild_login(&mut self, id: ObjectId) {
        let Some(character) = self.character_of(id) else {
            return;
        };
        let guild = self.guild_store.of_character(character);
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.guild = guild;
        }
        self.refresh_war_flags();
        self.castle_login(id);
        let Some(guild) = guild else {
            return;
        };
        if let Some(g) = self.guild_store.get_mut(guild) {
            // Keep the stored name current (renames are not a thing yet).
            if let Some(m) = g.members.iter_mut().find(|m| m.character == character) {
                m.name = self.objects[&id].player().unwrap().name.clone();
            }
        }
        self.guild_broadcast_info(guild);
    }

    pub(super) fn guild_logout(&mut self, id: ObjectId) {
        let Some(guild) = self.guild_of(id) else {
            return;
        };
        let others: Vec<ObjectId> = self
            .guild_online(guild)
            .into_iter()
            .filter(|m| *m != id)
            .collect();
        let index = self
            .character_of(id)
            .and_then(|c| {
                self.guild_store
                    .get(guild)?
                    .members
                    .iter()
                    .find(|m| m.character == c)
                    .map(|m| m.index)
            })
            .unwrap_or(0);
        for m in others {
            self.send_to(m, ServerMessage::GuildMemberOffline { index });
        }
        for o in self.objects.values_mut() {
            if let Some(p) = o.player_mut() {
                if p.guild_invite == Some(id) {
                    p.guild_invite = None;
                }
            }
        }
    }

    pub fn guild_create(&mut self, id: ObjectId, name: String, members: i32) {
        self.guild_create_inner(id, name, members, false);
    }

    pub(super) fn gm_create_guild(&mut self, id: ObjectId, name: String) -> bool {
        if self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .is_some_and(|p| p.guild.is_some())
        {
            return false;
        }
        self.guild_create_inner(id, name, 1, true);
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .is_some_and(|p| p.guild.is_some())
    }

    fn guild_create_inner(&mut self, id: ObjectId, name: String, members: i32, free: bool) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if p.guild.is_some() {
            self.guild_line(id, "You are already in a guild.".into());
            return;
        }
        if !valid_name(&name) {
            self.guild_line(id, "Guild names are 2 to 15 letters or digits.".into());
            return;
        }
        if !(1..=MAX_MEMBER_LIMIT).contains(&members) {
            return;
        }
        if self
            .guild_store
            .guilds
            .iter()
            .any(|g| g.name.eq_ignore_ascii_case(&name))
        {
            self.guild_line(id, "That guild name is taken.".into());
            return;
        }
        let cost = GUILD_CREATION_COST + members as u64 * GUILD_MEMBER_COST;
        if !free && p.bag.gold < cost {
            self.guild_line(id, format!("Creating that guild costs {cost} gold."));
            return;
        }
        let (character, pname) = (p.character, p.name.clone());
        let now = self.now;
        let gid = {
            let store = &mut self.guild_store;
            store.next_id += 1;
            let gid = store.next_id;
            store.guilds.push(Guild {
                id: gid,
                name: name.clone(),
                notice: String::new(),
                member_limit: members,
                funds: 0,
                tax: 0,
                default_rank: "New Member".into(),
                default_permission: guild_permission::NONE,
                next_index: 2,
                members: vec![GuildMember {
                    index: 1,
                    character,
                    name: pname,
                    rank: "Guild Leader".into(),
                    permission: guild_permission::LEADER,
                    joined: now,
                }],
            });
            store.save();
            gid
        };
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        if !free {
            p.bag.gold -= cost;
        }
        p.guild = Some(gid);
        self.send_gold(id);
        self.guild_line(id, format!("Guild {name} created."));
        self.guild_broadcast_info(gid);
        self.refresh_appearance(id);
    }

    pub(super) fn member_permission(&self, id: ObjectId) -> Option<(u32, i32, u32)> {
        let guild = self.guild_of(id)?;
        let character = self.character_of(id)?;
        let m = self
            .guild_store
            .get(guild)?
            .members
            .iter()
            .find(|m| m.character == character)?;
        Some((guild, m.permission, m.index))
    }

    pub fn guild_edit_notice(&mut self, id: ObjectId, notice: String) {
        let Some((guild, permission, _)) = self.member_permission(id) else {
            return;
        };
        if !has(permission, guild_permission::EDIT_NOTICE) {
            self.guild_line(id, "You do not have permission to edit the notice.".into());
            return;
        }
        if notice.chars().count() > MAX_NOTICE {
            return;
        }
        if let Some(g) = self.guild_store.get_mut(guild) {
            g.notice = notice.clone();
        }
        self.guild_store.save();
        for m in self.guild_online(guild) {
            self.send_to(
                m,
                ServerMessage::GuildNoticeChanged {
                    notice: notice.clone(),
                },
            );
        }
    }

    /// Leader sets a member's rank and permission (index 0 edits the
    /// defaults for new members); nobody changes their own permission.
    pub fn guild_edit_member(&mut self, id: ObjectId, index: u32, rank: String, permission: i32) {
        let Some((guild, my_permission, my_index)) = self.member_permission(id) else {
            return;
        };
        if !has(my_permission, guild_permission::LEADER) {
            self.guild_line(id, "Only a guild leader can edit members.".into());
            return;
        }
        if rank.is_empty() || rank.chars().count() > 15 {
            self.guild_line(id, "Ranks are 1 to 15 characters.".into());
            return;
        }
        let mut changed_character = None;
        {
            let Some(g) = self.guild_store.get_mut(guild) else {
                return;
            };
            if index == 0 {
                g.default_rank = rank;
                g.default_permission = permission;
            } else {
                let Some(m) = g.members.iter_mut().find(|m| m.index == index) else {
                    self.guild_line(id, "No such member.".into());
                    return;
                };
                if m.index != my_index {
                    m.permission = permission;
                }
                m.rank = rank;
                changed_character = Some(m.character);
            }
        }
        self.guild_store.save();
        self.guild_broadcast_info(guild);
        if let Some(o) = changed_character.and_then(|c| self.online_character(c)) {
            self.refresh_appearance(o);
        }
    }

    pub fn guild_invite(&mut self, id: ObjectId, name: String) {
        let Some((guild, permission, _)) = self.member_permission(id) else {
            self.guild_line(id, "You are not in a guild.".into());
            return;
        };
        if !has(permission, guild_permission::ADD_MEMBER) {
            self.guild_line(id, "You do not have permission to invite members.".into());
            return;
        }
        let target = self
            .players()
            .find(|o| {
                o.player()
                    .is_some_and(|p| p.name.eq_ignore_ascii_case(&name))
            })
            .map(|o| o.id);
        let Some(target) = target else {
            self.guild_line(id, format!("Could not find {name}."));
            return;
        };
        let tp = self.objects[&target].player().unwrap();
        let tname = tp.name.clone();
        if tp.guild.is_some() {
            self.guild_line(id, format!("{tname} is already in a guild."));
            return;
        }
        if tp.guild_invite.is_some() {
            self.guild_line(id, format!("{tname} has already been invited to a guild."));
            return;
        }
        let g = self.guild_store.get(guild).unwrap();
        if g.members.len() as i32 >= g.member_limit {
            self.guild_line(id, "Your guild has no room for more members.".into());
            return;
        }
        let gname = g.name.clone();
        let from = self.objects[&id].player().unwrap().name.clone();
        self.objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap()
            .guild_invite = Some(id);
        self.send_to(target, ServerMessage::GuildInvite { from, guild: gname });
    }

    pub fn guild_response(&mut self, id: ObjectId, accept: bool) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let inviter = p.guild_invite.take();
        if !accept || p.guild.is_some() {
            return;
        }
        let (character, name) = (p.character, p.name.clone());
        let Some(inviter) = inviter else {
            return;
        };
        let Some((guild, permission, _)) = self.member_permission(inviter) else {
            return;
        };
        if !has(permission, guild_permission::ADD_MEMBER) {
            return;
        }
        let now = self.now;
        let (gname, ok) = {
            let g = self.guild_store.get_mut(guild).unwrap();
            if g.members.len() as i32 >= g.member_limit {
                (g.name.clone(), false)
            } else {
                let index = g.next_index;
                g.next_index += 1;
                g.members.push(GuildMember {
                    index,
                    character,
                    name: name.clone(),
                    rank: g.default_rank.clone(),
                    permission: g.default_permission,
                    joined: now,
                });
                (g.name.clone(), true)
            }
        };
        if !ok {
            self.guild_line(id, format!("{gname} has no room for you."));
            return;
        }
        self.guild_store.save();
        self.objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap()
            .guild = Some(guild);
        self.guild_line(id, format!("Welcome to {gname}."));
        for m in self.guild_online(guild) {
            if m != id {
                self.guild_line(m, format!("{name} has joined the guild."));
            }
        }
        self.guild_broadcast_info(guild);
        self.refresh_appearance(id);
        self.refresh_war_flags();
    }

    /// Leave; the last leader cannot leave a guild with other members.
    pub fn guild_leave(&mut self, id: ObjectId) {
        let Some((guild, permission, index)) = self.member_permission(id) else {
            return;
        };
        let g = self.guild_store.get(guild).unwrap();
        if has(permission, guild_permission::LEADER)
            && g.members.len() > 1
            && !g
                .members
                .iter()
                .any(|m| m.index != index && has(m.permission, guild_permission::LEADER))
        {
            self.guild_line(id, "Appoint another leader before leaving.".into());
            return;
        }
        let name = self.objects[&id].player().unwrap().name.clone();
        self.guild_remove_member(guild, index);
        self.guild_line(id, "You have left the guild.".into());
        for m in self.guild_online(guild) {
            self.guild_line(m, format!("{name} has left the guild."));
        }
    }

    pub fn guild_kick(&mut self, id: ObjectId, index: u32) {
        let Some((guild, permission, my_index)) = self.member_permission(id) else {
            return;
        };
        if !has(permission, guild_permission::LEADER) {
            self.guild_line(id, "Only a guild leader can kick members.".into());
            return;
        }
        if index == my_index {
            self.guild_line(id, "You cannot kick yourself.".into());
            return;
        }
        let Some(m) = self
            .guild_store
            .get(guild)
            .and_then(|g| g.members.iter().find(|m| m.index == index))
            .cloned()
        else {
            self.guild_line(id, "No such member.".into());
            return;
        };
        let kicker = self.objects[&id].player().unwrap().name.clone();
        self.guild_remove_member(guild, index);
        if let Some(o) = self.online_character(m.character) {
            self.guild_line(
                o,
                format!("You have been kicked from the guild by {kicker}."),
            );
        }
        for o in self.guild_online(guild) {
            self.guild_line(
                o,
                format!("{} was kicked from the guild by {kicker}.", m.name),
            );
        }
    }

    fn guild_remove_member(&mut self, guild: u32, index: u32) {
        let removed = {
            let Some(g) = self.guild_store.get_mut(guild) else {
                return;
            };
            let Some(pos) = g.members.iter().position(|m| m.index == index) else {
                return;
            };
            let m = g.members.remove(pos);
            if g.members.is_empty() {
                self.guild_store.guilds.retain(|x| x.id != guild);
            }
            m
        };
        self.guild_store.save();
        if let Some(o) = self.online_character(removed.character) {
            if let Some(p) = self.objects.get_mut(&o).and_then(|x| x.player_mut()) {
                p.guild = None;
            }
            self.send_to(o, ServerMessage::GuildInfo(None));
            self.refresh_appearance(o);
        }
        for m in self.guild_online(guild) {
            self.send_to(m, ServerMessage::GuildKick { index });
        }
        // A dissolved guild loses its wars, castle and conquest requests.
        if self.guild_store.get(guild).is_none() {
            self.guild_store
                .wars
                .retain(|w| w.guild1 != guild && w.guild2 != guild);
            self.guild_store.castles.retain(|c| c.guild != guild);
            self.guild_store.conquests.retain(|c| c.guild != guild);
            self.guild_store.save();
            self.castle_broadcast_all();
        }
        self.refresh_war_flags();
    }

    pub fn guild_tax(&mut self, id: ObjectId, tax: i32) {
        let Some((guild, permission, _)) = self.member_permission(id) else {
            return;
        };
        if !has(permission, guild_permission::LEADER) || !(0..=100).contains(&tax) {
            return;
        }
        if let Some(g) = self.guild_store.get_mut(guild) {
            g.tax = tax;
        }
        self.guild_store.save();
        self.guild_send_update(guild);
    }

    pub fn guild_increase_member(&mut self, id: ObjectId) {
        let Some((guild, permission, _)) = self.member_permission(id) else {
            return;
        };
        if !has(permission, guild_permission::LEADER) {
            return;
        }
        let Some(g) = self.guild_store.get_mut(guild) else {
            return;
        };
        if g.member_limit >= MAX_MEMBER_LIMIT {
            self.guild_line(id, "The guild is at the member limit.".into());
            return;
        }
        if g.funds < GUILD_MEMBER_COST as i64 {
            self.guild_line(id, "The guild cannot afford another member slot.".into());
            return;
        }
        g.funds -= GUILD_MEMBER_COST as i64;
        g.member_limit += 1;
        self.guild_store.save();
        self.guild_send_update(guild);
    }

    fn guild_send_update(&mut self, guild: u32) {
        let Some(g) = self.guild_store.get(guild) else {
            return;
        };
        let (member_limit, funds, tax) = (g.member_limit, g.funds, g.tax);
        for m in self.guild_online(guild) {
            self.send_to(
                m,
                ServerMessage::GuildUpdate {
                    member_limit,
                    funds,
                    tax,
                },
            );
        }
    }

    /// Zircon `CalculateGuildTax`: the guild's cut of picked-up gold.
    pub(super) fn guild_tax_gold(&mut self, id: ObjectId, gold: u64) -> u64 {
        let Some(guild) = self.guild_of(id) else {
            return gold;
        };
        let Some(g) = self.guild_store.get_mut(guild) else {
            return gold;
        };
        if g.tax <= 0 {
            return gold;
        }
        let cut = gold * g.tax as u64 / 100;
        g.funds += cut as i64;
        self.guild_store.dirty = true;
        gold - cut
    }

    /// Online members for guild chat (`!~`).
    pub(super) fn guild_members_online(&self, id: ObjectId) -> Vec<ObjectId> {
        self.guild_of(id)
            .map(|g| self.guild_online(g))
            .unwrap_or_default()
    }
}
