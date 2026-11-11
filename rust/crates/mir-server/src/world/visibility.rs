use super::*;

impl World {
    /// Zircon `CanBeSeenBy`: everyone sees everyone for now; invisibility
    /// and cloak plug in here.
    pub(super) fn can_see(&self, viewer: &Object, target: &Object) -> bool {
        if target.player().is_some_and(|p| p.observer) && viewer.id != target.id {
            return false;
        }
        if matches!(&target.kind, Kind::Monster(m) if m.hidden) {
            return false;
        }
        if let (Kind::Npc(n), Some(p)) = (&target.kind, viewer.player()) {
            if !self.npc_visible_to(n.info, p) {
                return false;
            }
        }
        // Zircon `CanBeSeenBy`: transparent or cloaked players vanish unless
        // the viewer is at least their level and within CloakRange (3).
        if target.has_buff(buff_type::TRANSPARENCY) || target.has_buff(buff_type::CLOAK) {
            let vlevel = self.level_of(viewer);
            let tlevel = self.level_of(target);
            if vlevel < tlevel || viewer.location.distance(target.location) > 3 {
                return false;
            }
        }
        true
    }

    /// Level of any fighting object.
    pub(super) fn level_of(&self, o: &Object) -> i32 {
        match &o.kind {
            Kind::Player(p) => p.level,
            Kind::Monster(m) => self.data.monsters[&m.def].level,
            _ => 0,
        }
    }

    /// Zircon `ShouldAttackTarget` extras: monsters ignore invisible and
    /// transparent players, and cloaked ones beyond two cells.
    pub(super) fn monster_may_target(&self, monster: &Object, target: &Object) -> bool {
        if target.player().is_some_and(|p| p.gm_mode || p.observer) {
            return false;
        }
        if target.has_buff(buff_type::INVISIBILITY) || target.has_buff(buff_type::TRANSPARENCY) {
            return false;
        }
        if target.has_buff(buff_type::CLOAK) {
            if monster.location.distance(target.location) > 2 {
                return false;
            }
            if self.level_of(target) >= self.level_of(monster) {
                return false;
            }
        }
        true
    }

    pub(super) fn insert_object(&mut self, obj: Object) {
        let map = self.maps.get_mut(&obj.map).expect("map loaded");
        map.objects.push(obj.id);
        map.add_to_cell(obj.id, obj.location);
        if obj.is_player() {
            self.players.insert(obj.id);
        }
        self.objects.insert(obj.id, obj);
    }

    pub fn remove_object(&mut self, id: ObjectId) {
        if self.objects.get(&id).is_some_and(|o| o.player().is_some()) {
            self.group_forget(id);
            self.companion_despawn(id);
        }
        // A leaving player takes its pets along.
        let pets: Vec<ObjectId> = self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.pets.clone())
            .unwrap_or_default();
        for pet in pets {
            self.remove_object(pet);
        }
        if let Some(obj) = self.objects.remove(&id) {
            self.players.remove(&id);
            if let Some(map) = self.maps.get_mut(&obj.map) {
                map.objects.retain(|o| *o != id);
                map.remove_from_cell(id, obj.location);
            }
            if let Kind::Monster(m) = &obj.kind {
                if !obj.dead {
                    if let Some((map, gi)) = m.spawn {
                        if let Some(g) = self.maps.get_mut(&map).and_then(|m| m.spawns.get_mut(gi))
                        {
                            g.alive -= 1;
                        }
                    }
                }
            }
            // Everyone who saw it gets a remove.
            for pid in &self.players {
                let other = self.objects.get_mut(pid).expect("player indexed");
                if other.visible.remove(&id) {
                    if let Some(p) = other.player() {
                        self.outgoing
                            .push(Outgoing::To(p.conn, ServerMessage::ObjectRemove { id }));
                    }
                }
            }
        }
    }

    pub(super) fn send_to(&mut self, id: ObjectId, msg: ServerMessage) {
        if let Some(p) = self.objects.get(&id).and_then(|o| o.player()) {
            self.outgoing.push(Outgoing::To(p.conn, msg));
        }
    }

    /// Recompute each player's visible set and emit show/remove.
    pub(super) fn update_visibility(&mut self) {
        let players: Vec<ObjectId> = self.players().map(|o| o.id).collect();
        for pid in players {
            let (map, loc, conn, account) = {
                let p = &self.objects[&pid];
                let pd = p.player().unwrap();
                (p.map, p.location, pd.conn, pd.account)
            };
            let now = self.now;
            let now_visible: HashSet<ObjectId> = self.maps[&map]
                .objects
                .iter()
                .copied()
                .filter(|id| *id != pid)
                .filter(|id| {
                    let o = &self.objects[id];
                    if o.location.distance(loc) > MAX_VIEW_RANGE {
                        return false;
                    }
                    match &o.kind {
                        Kind::Item(i) => {
                            i.owner.is_none_or(|a| a == account)
                                || (!i.item.bound && now >= i.spawn_time + DROP_SHARE_AFTER)
                        }
                        _ => self.can_see(&self.objects[&pid], o),
                    }
                })
                .collect();
            let old = std::mem::take(&mut self.objects.get_mut(&pid).unwrap().visible);
            for id in old.difference(&now_visible) {
                self.outgoing
                    .push(Outgoing::To(conn, ServerMessage::ObjectRemove { id: *id }));
            }
            for id in now_visible.difference(&old) {
                let mut state = self.objects[id].state();
                // Zircon `GetInfoPacket`: a harvestable corpse is bones to
                // everyone it owes nothing, so only the killer sees a body
                // still worth swinging at.
                if let mir_proto::Appearance::Monster { skeleton, .. } = &mut state.appearance {
                    *skeleton = self.is_skeleton_for(*id, account);
                }
                self.outgoing
                    .push(Outgoing::To(conn, ServerMessage::ObjectShow(state)));
                let kinds = self.objects[id]
                    .poisons
                    .iter()
                    .fold(0, |flags, p| flags | p.kind);
                if kinds != 0 {
                    self.outgoing.push(Outgoing::To(
                        conn,
                        ServerMessage::ObjectPoisoned { id: *id, kinds },
                    ));
                }
                if matches!(&self.objects[id].kind, Kind::Npc(_)) {
                    let marker = self.npc_quest_marker(pid, *id);
                    self.outgoing.push(Outgoing::To(
                        conn,
                        ServerMessage::NpcQuestMarker { id: *id, marker },
                    ));
                }
            }
            self.objects.get_mut(&pid).unwrap().visible = now_visible;
        }
    }

    /// Deliver this tick's events to every player that can see the subject
    /// (Zircon `Broadcast` to `SeenByPlayers`). Self-originated movement and
    /// attacks are not echoed; the client predicts those.
    pub(super) fn flush_events(&mut self) {
        let events = std::mem::take(&mut self.events);
        let players: Vec<(ObjectId, ConnId)> = self
            .players()
            .filter_map(|o| o.player().map(|p| (o.id, p.conn)))
            .collect();
        for (subject, msg) in events {
            let echo_self = !matches!(
                msg,
                ServerMessage::ObjectMove { .. }
                    | ServerMessage::ObjectTurn { .. }
                    | ServerMessage::ObjectAttack { .. }
                    | ServerMessage::ObjectHarvest { .. }
            );
            for (pid, conn) in &players {
                let sees =
                    *pid == subject && echo_self || self.objects[pid].visible.contains(&subject);
                if matches!(msg, ServerMessage::ObjectHarvested { .. }) {
                    let account = self.objects[pid].player().unwrap().account;
                    if !self.is_skeleton_for(subject, account) {
                        continue;
                    }
                }
                if sees {
                    self.outgoing.push(Outgoing::To(*conn, msg.clone()));
                }
            }
        }
    }
}
