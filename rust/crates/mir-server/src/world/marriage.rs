//! Marriage (Zircon `PlayerObject.Social` marriage region): the NPC
//! `Marriage` action proposes to the player in front, both pay 500,000
//! gold on acceptance, a ring becomes the wedding ring at the NPC, the
//! ring teleports to the partner every two minutes, `Divorce` ends it.

use super::*;
use mir_proto::{slot, ChatKind};

pub const MARRIAGE_COST: u64 = 500_000;
pub const MARRIAGE_LEVEL: i32 = 22;
pub const TELEPORT_DELAY: u64 = 120_000;

impl World {
    fn marry_line(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }

    /// Online player object for a character id.
    fn online_by_character(&self, character: u32) -> Option<ObjectId> {
        self.players()
            .find(|o| o.player().is_some_and(|p| p.character == character))
            .map(|o| o.id)
    }

    pub(super) fn send_marriage_info(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let partner = p.partner.as_ref().map(|(_, n)| n.clone());
        let wedding_ring = p.wedding_ring;
        self.send_to(
            id,
            ServerMessage::MarriageInfo {
                partner,
                wedding_ring,
            },
        );
    }

    /// Is the wedding ring worn on the left ring finger?
    pub(super) fn wears_wedding_ring(&self, p: &PlayerData) -> bool {
        p.wedding_ring.is_some_and(|ring| {
            p.bag
                .equipment
                .get(slot::RING_L)
                .and_then(|c| c.as_ref())
                .is_some_and(|it| it.id == ring)
        })
    }

    /// NPC `Marriage` action: propose to the player in front who faces you.
    pub fn marriage_request(&mut self, id: ObjectId) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if p.partner.is_some() {
            self.marry_line(id, "You are already married.".into());
            return;
        }
        if p.level < MARRIAGE_LEVEL {
            self.marry_line(id, "You need to be level 22 to marry.".into());
            return;
        }
        if p.bag.gold < MARRIAGE_COST {
            self.marry_line(id, "You need 500,000 gold to marry.".into());
            return;
        }
        let front = o.location.step(o.direction, 1);
        let target = self
            .maps
            .get(&o.map)
            .and_then(|m| {
                m.objects_at(front)
                    .iter()
                    .copied()
                    .find(|t| self.objects.get(t).is_some_and(|t| t.player().is_some()))
            })
            .filter(|t| self.objects[t].direction == o.direction.opposite());
        let Some(target) = target else {
            self.marry_line(id, "You need to face the one you want to marry.".into());
            return;
        };
        let t = &self.objects[&target];
        let tp = t.player().unwrap();
        let tname = tp.name.clone();
        if tp.partner.is_some() {
            self.marry_line(id, format!("{tname} is already married."));
            return;
        }
        if tp.marriage_invite.is_some() {
            self.marry_line(id, format!("{tname} already has a proposal."));
            return;
        }
        if tp.level < MARRIAGE_LEVEL {
            self.marry_line(id, format!("{tname} needs to be level 22 to marry."));
            return;
        }
        if tp.bag.gold < MARRIAGE_COST {
            self.marry_line(id, format!("{tname} needs 500,000 gold to marry."));
            return;
        }
        if t.dead || o.dead {
            self.marry_line(id, "The dead cannot marry.".into());
            return;
        }
        let from = p.name.clone();
        self.objects
            .get_mut(&target)
            .and_then(|o| o.player_mut())
            .unwrap()
            .marriage_invite = Some(id);
        self.send_to(target, ServerMessage::MarriageInvite { from });
    }

    pub fn marriage_response(&mut self, id: ObjectId, accept: bool) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let Some(proposer) = p.marriage_invite.take() else {
            return;
        };
        if !accept || p.partner.is_some() {
            return;
        }
        let (my_char, my_name, my_gold) = (p.character, p.name.clone(), p.bag.gold);
        let Some(pp) = self.objects.get(&proposer).and_then(|o| o.player()) else {
            return;
        };
        if pp.partner.is_some() {
            return;
        }
        let (their_char, their_name, their_gold) = (pp.character, pp.name.clone(), pp.bag.gold);
        if my_gold < MARRIAGE_COST {
            self.marry_line(id, "You need 500,000 gold to marry.".into());
            self.marry_line(proposer, format!("{my_name} needs 500,000 gold to marry."));
            return;
        }
        if their_gold < MARRIAGE_COST {
            self.marry_line(id, format!("{their_name} needs 500,000 gold to marry."));
            self.marry_line(proposer, "You need 500,000 gold to marry.".into());
            return;
        }
        for (a, partner) in [
            (id, (their_char, their_name.clone())),
            (proposer, (my_char, my_name.clone())),
        ] {
            let p = self
                .objects
                .get_mut(&a)
                .and_then(|o| o.player_mut())
                .unwrap();
            p.partner = Some(partner);
            p.bag.gold -= MARRIAGE_COST;
            self.send_gold(a);
            self.send_marriage_info(a);
        }
        self.marry_line(id, format!("You are now married to {their_name}."));
        self.marry_line(proposer, format!("You are now married to {my_name}."));
    }

    /// NPC `Divorce` action: end the marriage (the partner may be offline;
    /// the caller clears their record through `take_divorced`).
    pub fn marriage_leave(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let Some((partner_char, partner_name)) = p.partner.take() else {
            return;
        };
        let my_name = p.name.clone();
        self.marriage_remove_ring(id);
        self.marry_line(id, format!("You have divorced {partner_name}."));
        self.send_marriage_info(id);
        match self.online_by_character(partner_char) {
            Some(o) => {
                if let Some(pp) = self.objects.get_mut(&o).and_then(|x| x.player_mut()) {
                    pp.partner = None;
                }
                self.marriage_remove_ring(o);
                self.marry_line(o, format!("{my_name} has divorced you."));
                self.send_marriage_info(o);
            }
            None => self.divorced.push(partner_char),
        }
    }

    /// Characters divorced while offline; the account registry clears them.
    pub fn take_divorced(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.divorced)
    }

    /// Wedding-ring NPC page: a wearable ring from the bag goes on the left
    /// ring finger and becomes the wedding ring.
    pub fn marriage_make_ring(&mut self, id: ObjectId, slot: u8) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if p.partner.is_none() {
            self.marry_line(id, "You are not married.".into());
            return;
        }
        if self.wears_wedding_ring(p) {
            self.marry_line(id, "You already wear your wedding ring.".into());
            return;
        }
        let Some(ring) = p.bag.inventory.get(slot as usize).cloned().flatten() else {
            return;
        };
        let is_ring = self
            .data
            .items
            .get(&ring.info)
            .is_some_and(|d| d.item_type == mir_proto::item_type::RING);
        if !is_ring {
            self.marry_line(id, "That is not a ring.".into());
            return;
        }
        match self.item_move_inner(
            id,
            Grid::Inventory,
            slot,
            Grid::Equipment,
            slot::RING_L as u8,
        ) {
            Ok(changes) => {
                let p = self
                    .objects
                    .get_mut(&id)
                    .and_then(|o| o.player_mut())
                    .unwrap();
                p.wedding_ring = Some(ring.id);
                self.send_changes(id, changes);
                self.refresh_stats(id, false);
                self.send_player_stats(id);
                self.send_marriage_info(id);
                self.marry_line(id, "The ring is now your wedding ring.".into());
            }
            Err(e) => self.marry_line(id, e),
        }
    }

    /// NPC `RemoveWeddingRing` action (and divorce): the ring is a plain
    /// ring again.
    pub fn marriage_remove_ring(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        if p.wedding_ring.take().is_some() {
            self.send_marriage_info(id);
        }
    }

    /// Wedding ring teleport: to a random cell within 10 of the partner.
    pub fn marriage_teleport(&mut self, id: ObjectId) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        let Some((partner_char, _)) = p.partner.clone() else {
            self.marry_line(id, "You are not married.".into());
            return;
        };
        if !self.wears_wedding_ring(p) {
            self.marry_line(id, "You need to wear your wedding ring.".into());
            return;
        }
        if o.dead {
            self.marry_line(id, "You cannot teleport while dead.".into());
            return;
        }
        if p.pk_points >= pvp::RED_POINT {
            self.marry_line(id, "Red names cannot teleport to their partner.".into());
            return;
        }
        if self.now < p.marriage_teleport_time {
            let wait = p
                .marriage_teleport_time
                .saturating_sub(self.now)
                .div_ceil(1000);
            self.marry_line(
                id,
                format!("You cannot teleport to your partner for another {wait} seconds."),
            );
            return;
        }
        let Some(target) = self.online_by_character(partner_char) else {
            self.marry_line(id, "Your partner is not online.".into());
            return;
        };
        let t = &self.objects[&target];
        if t.dead {
            self.marry_line(id, "Your partner is dead.".into());
            return;
        }
        let (map, loc) = (t.map, t.location);
        if !self
            .data
            .maps
            .get(&map)
            .is_some_and(|m| m.can_marriage_recall)
        {
            self.marry_line(id, "You cannot teleport to that map.".into());
            return;
        }
        // Zircon Map.GetRandomLocation(partner, 10).
        let mut cell = None;
        for _ in 0..40 {
            let dx = self.rng.random_range(-10..=10);
            let dy = self.rng.random_range(-10..=10);
            let c = Point::new(loc.x + dx, loc.y + dy);
            if self.maps[&map].file.is_walkable(c.x, c.y) && !self.cell_blocked(map, c, false) {
                cell = Some(c);
                break;
            }
        }
        let Some(cell) = cell else {
            self.marry_line(id, "There is no room next to your partner.".into());
            return;
        };
        let now = self.now;
        self.objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap()
            .marriage_teleport_time = now + TELEPORT_DELAY;
        if self.objects[&id].map == map {
            self.teleport_object(id, cell);
        } else {
            self.change_map(id, map, cell);
        }
    }
}
