//! Groups (Zircon `PlayerObject.Social`): invite by name, accept, leave,
//! kick; the first member leads; groups of one dissolve. Experience and
//! drops are shared with members on the same map within view range.

use super::*;
use mir_proto::ChatKind;

/// Zircon `Globals.GroupLimit`.
pub const GROUP_LIMIT: usize = 15;

impl World {
    fn system(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }

    pub(super) fn player_name(&self, id: ObjectId) -> String {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.name.clone())
            .unwrap_or_default()
    }

    pub(super) fn find_player(&self, name: &str) -> Option<ObjectId> {
        self.players()
            .find(|o| {
                o.player()
                    .is_some_and(|p| p.name.eq_ignore_ascii_case(name))
            })
            .map(|o| o.id)
    }

    pub fn group_switch(&mut self, id: ObjectId, allow: bool) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        if p.allow_group == allow {
            return;
        }
        p.allow_group = allow;
        self.send_to(id, ServerMessage::GroupSwitch { allow });
        if !allow {
            self.group_leave(id);
        }
    }

    pub fn group_invite(&mut self, id: ObjectId, name: String) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if let Some(g) = p.group {
            if self.groups.get(&g).and_then(|m| m.first()) != Some(&id) {
                self.system(id, "You are not the leader of your group.".into());
                return;
            }
        }
        let Some(target) = self.find_player(&name) else {
            self.system(id, format!("Could not find {name}."));
            return;
        };
        if target == id {
            self.system(id, "You cannot group with yourself.".into());
            return;
        }
        let tp = self.objects[&target].player().unwrap();
        let tname = tp.name.clone();
        if tp.group.is_some() {
            self.system(id, format!("{tname} is already in a group."));
            return;
        }
        if tp.group_invite.is_some() {
            self.system(id, format!("{tname} has already been invited to a group."));
            return;
        }
        if !tp.allow_group {
            self.system(id, format!("{tname} is not allowing group invites."));
            return;
        }
        // Zircon refuses the invite outright when either side blocks the
        // other, with the same message as a closed group.
        let (my_account, their_account) = (
            self.objects[&id].player().unwrap().account,
            self.objects[&target].player().unwrap().account,
        );
        if self.is_blocking(my_account, their_account) {
            self.system(id, format!("{tname} is not allowing group invites."));
            return;
        }
        let from = self.player_name(id);
        self.objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap()
            .group_invite = Some(id);
        self.send_to(target, ServerMessage::GroupInvite { from });
    }

    pub fn group_response(&mut self, id: ObjectId, accept: bool) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let inviter = p.group_invite.take();
        if p.group.is_some() || !accept {
            return;
        }
        let Some(inviter) = inviter else {
            return;
        };
        let Some(ip) = self.objects.get(&inviter).and_then(|o| o.player()) else {
            return;
        };
        let gid = match ip.group {
            Some(g) => {
                let members = &self.groups[&g];
                if members.first() != Some(&inviter) {
                    return;
                }
                if members.len() >= GROUP_LIMIT {
                    self.system(id, "The group is full.".into());
                    return;
                }
                g
            }
            None => {
                let g = self.next_group;
                self.next_group += 1;
                self.groups.insert(g, vec![inviter]);
                let ip = self
                    .objects
                    .get_mut(&inviter)
                    .and_then(|o| o.player_mut())
                    .unwrap();
                ip.group = Some(g);
                if !ip.allow_group {
                    ip.allow_group = true;
                    self.send_to(inviter, ServerMessage::GroupSwitch { allow: true });
                }
                let name = self.player_name(inviter);
                self.send_to(inviter, ServerMessage::GroupMember { id: inviter, name });
                g
            }
        };
        let name = self.player_name(id);
        let members = self.groups[&gid].clone();
        for m in members {
            self.send_to(
                m,
                ServerMessage::GroupMember {
                    id,
                    name: name.clone(),
                },
            );
            let mname = self.player_name(m);
            self.send_to(id, ServerMessage::GroupMember { id: m, name: mname });
        }
        self.groups.get_mut(&gid).unwrap().push(id);
        self.objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap()
            .group = Some(gid);
        self.send_to(id, ServerMessage::GroupMember { id, name });
    }

    pub fn group_remove(&mut self, id: ObjectId, name: String) {
        let Some(g) = self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .and_then(|p| p.group)
        else {
            self.system(id, "You are not in a group.".into());
            return;
        };
        let members = self.groups[&g].clone();
        if members.first() != Some(&id) {
            self.system(id, "You are not the leader of your group.".into());
            return;
        }
        let member = members.into_iter().find(|m| {
            self.objects[m]
                .player()
                .is_some_and(|p| p.name.eq_ignore_ascii_case(&name))
        });
        match member {
            Some(m) => self.group_leave(m),
            None => self.system(id, format!("{name} is not in your group.")),
        }
    }

    /// Leave the current group; a group left with one member dissolves.
    pub fn group_leave(&mut self, id: ObjectId) {
        self.buff_remove(id, mir_proto::buff_type::SOUL_RESONANCE);
        let Some(g) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .and_then(|p| p.group.take())
        else {
            return;
        };
        let Some(members) = self.groups.get_mut(&g) else {
            return;
        };
        members.retain(|m| *m != id);
        let remaining = members.clone();
        for m in &remaining {
            self.send_to(*m, ServerMessage::GroupRemove { id });
        }
        self.send_to(id, ServerMessage::GroupRemove { id });
        if remaining.len() <= 1 {
            self.groups.remove(&g);
            if let Some(last) = remaining.first().copied() {
                if let Some(p) = self.objects.get_mut(&last).and_then(|o| o.player_mut()) {
                    p.group = None;
                }
                self.send_to(last, ServerMessage::GroupRemove { id: last });
            }
        }
    }

    /// Members of the speaker's group (including the speaker); empty when
    /// not grouped.
    pub(super) fn group_members(&self, id: ObjectId) -> Vec<ObjectId> {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .and_then(|p| p.group)
            .and_then(|g| self.groups.get(&g))
            .cloned()
            .unwrap_or_default()
    }

    /// Group members (including `owner`) on `map` within view range of
    /// `loc`: Zircon's `dPlayers`. Empty when not grouped.
    pub(super) fn group_sharers(&self, owner: ObjectId, map: i32, loc: Point) -> Vec<ObjectId> {
        self.group_members(owner)
            .into_iter()
            .filter(|m| {
                self.objects
                    .get(m)
                    .is_some_and(|o| o.map == map && o.location.distance(loc) <= MAX_VIEW_RANGE)
            })
            .collect()
    }

    /// Forget a leaving player: its group slot, invites it sent, its trade.
    pub(super) fn group_forget(&mut self, id: ObjectId) {
        self.trade_forget(id);
        self.guild_logout(id);
        self.group_leave(id);
        for o in self.objects.values_mut() {
            if let Some(p) = o.player_mut() {
                if p.group_invite == Some(id) {
                    p.group_invite = None;
                }
            }
        }
    }
}
