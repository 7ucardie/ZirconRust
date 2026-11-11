//! Castle guards, gates and flags (Zircon `CastleGuard`, `CastleGate`,
//! `CastleFlag`), placed from the `CastleGuardInfo` / `CastleGateInfo` /
//! `CastleFlagInfo` rows when the castle map loads. Guards shoot enemy
//! guild members during a war, gates block until opened (owners walk up
//! and they open on their own; the owner guild can toggle and repair
//! them), and a flag changes hands after a contesting guild stands beside
//! it for thirty seconds unopposed.

use super::guilds::CastleOwner;
use super::*;
use mir_proto::ChatKind;

/// Zircon `CastleFlag._takeDuration`.
const FLAG_TAKE_MS: u64 = 30_000;
const GUARD_RANGE: i32 = 15;

#[derive(Debug, Clone)]
pub enum CastleRole {
    Guard {
        castle: i32,
        repair_cost: i64,
    },
    Gate {
        castle: i32,
        closed: bool,
        /// Cells blocked besides the gate's own while closed.
        blocks: Vec<Point>,
        repair_cost: i64,
        /// Auto-opened gates close again after this time.
        close_at: u64,
    },
    Flag {
        castle: i32,
        contester: Option<u32>,
        contest_until: u64,
    },
}

impl CastleRole {
    pub fn castle(&self) -> i32 {
        match self {
            CastleRole::Guard { castle, .. }
            | CastleRole::Gate { castle, .. }
            | CastleRole::Flag { castle, .. } => *castle,
        }
    }
}

/// Zircon `CastleGate.OnSpawned` block layout by the gate's `FaceImage`.
fn gate_blocks(face_image: i32) -> Vec<(i32, i32)> {
    match face_image {
        1 => vec![(1, 0), (0, 1), (-1, -1), (-1, 0), (0, -1)],
        2 => vec![(1, -1), (-1, 0), (0, -1), (0, 1), (1, 0)],
        3 => vec![(-1, -1), (-1, 0), (0, 1), (1, 0), (0, -1)],
        _ => Vec::new(),
    }
}

impl World {
    fn castle_role(&self, id: ObjectId) -> Option<&CastleRole> {
        match &self.objects.get(&id)?.kind {
            Kind::Monster(m) => m.castle.as_ref(),
            _ => None,
        }
    }

    /// Place a castle's guards, gates and flags on a freshly loaded map.
    pub(super) fn spawn_castle_objects(&mut self, map: i32) {
        let castles: Vec<crate::data::CastleDef> = self
            .data
            .castles
            .iter()
            .filter(|c| c.map == map)
            .cloned()
            .collect();
        for castle in castles {
            for part in castle
                .guards
                .iter()
                .chain(&castle.gates)
                .chain(&castle.flags)
            {
                if !self.data.monsters.contains_key(&part.monster) {
                    continue;
                }
                let at = Point::new(part.x, part.y);
                let id = self.create_monster(part.monster, map, at, None, None, 0);
                let face = self.data.monsters[&part.monster].face_image;
                let role = match part.kind {
                    0 => CastleRole::Guard {
                        castle: castle.index,
                        repair_cost: part.repair_cost,
                    },
                    1 => CastleRole::Gate {
                        castle: castle.index,
                        closed: true,
                        blocks: gate_blocks(face)
                            .into_iter()
                            .map(|(dx, dy)| Point::new(at.x + dx, at.y + dy))
                            .collect(),
                        repair_cost: part.repair_cost,
                        close_at: 0,
                    },
                    _ => CastleRole::Flag {
                        castle: castle.index,
                        contester: None,
                        contest_until: u64::MAX,
                    },
                };
                if let Some(o) = self.objects.get_mut(&id) {
                    o.direction = Direction::from_index(part.direction.clamp(0, 7) as u8);
                    if let Some(m) = o.monster_mut() {
                        m.castle = Some(role);
                    }
                }
            }
        }
    }

    /// Is the cell shut by a closed gate (its own cell or its blocks)?
    pub(super) fn gate_blocks_cell(&self, map: i32, p: Point) -> bool {
        self.on_map(map).any(|o| {
            !o.dead
                && matches!(&o.kind, Kind::Monster(m)
                    if matches!(&m.castle, Some(CastleRole::Gate { closed: true, blocks, .. })
                        if o.location == p || blocks.contains(&p)))
        })
    }

    /// The guild of the player behind an object, if any.
    fn guild_side(&self, id: ObjectId) -> Option<u32> {
        let side = self.objects.get(&id)?.side()?;
        self.objects.get(&side)?.player()?.guild
    }

    /// Zircon `CastleGuard`/`CastleFlag` target rule: a player of a guild
    /// that owns no castle and, when the war names participants, is one.
    fn castle_enemy(&self, castle: i32, id: ObjectId) -> bool {
        let Some(c) = self.conquest.as_ref().filter(|c| c.castle == castle) else {
            return false;
        };
        let Some(o) = self.objects.get(&id) else {
            return false;
        };
        if o.dead || o.player().is_none() {
            return false;
        }
        let Some(guild) = o.player().and_then(|p| p.guild) else {
            return false;
        };
        if self.guild_store.castle_of(guild).is_some() {
            return false;
        }
        c.participants.is_empty() || c.participants.contains(&guild)
    }

    /// Damage rule for castle objects: `Some(false)` blocks the hit,
    /// `Some(true)` lets it through, `None` for ordinary targets.
    pub(super) fn castle_object_accepts_hit(
        &self,
        attacker: ObjectId,
        target: ObjectId,
    ) -> Option<bool> {
        let role = self.castle_role(target)?;
        Some(match role {
            CastleRole::Guard { castle, .. } => {
                let castle = *castle;
                let side = self.objects.get(&attacker).and_then(|o| o.side());
                match side {
                    Some(s) => {
                        self.castle_enemy(castle, s)
                            && self.guild_side(attacker) != self.guild_store.castle_owner(castle)
                    }
                    None => false,
                }
            }
            CastleRole::Gate { closed, .. } => *closed,
            CastleRole::Flag { .. } => false,
        })
    }

    /// Per-tick behaviour of a castle object; always consumes the tick.
    pub(super) fn process_castle_object(&mut self, id: ObjectId) {
        let now = self.now;
        let Some(role) = self.castle_role(id).cloned() else {
            return;
        };
        let (map, loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        match role {
            CastleRole::Guard { castle, .. } => {
                if !self.monster_can_attack(id)
                    || self.conquest.as_ref().map(|c| c.castle) != Some(castle)
                {
                    return;
                }
                let owner = self.guild_store.castle_owner(castle);
                let target = self
                    .on_map(map)
                    .filter(|o| o.location.distance(loc) <= GUARD_RANGE)
                    .filter(|o| self.castle_enemy(castle, o.id))
                    .filter(|o| o.player().and_then(|p| p.guild) != owner)
                    .min_by_key(|o| o.location.distance(loc))
                    .map(|o| (o.id, o.location));
                let Some((t, tloc)) = target else {
                    return;
                };
                let dir = Direction::from_points(loc, tloc);
                self.face_and_swing(id, dir, Some(t));
                let delay = 400 + tloc.distance(loc) as u64 * 48;
                self.push_monster_hit(id, t, delay, true, element::NONE);
            }
            CastleRole::Gate {
                castle,
                closed,
                close_at,
                ..
            } => {
                if now < self.objects[&id].monster_ref().cadence_time {
                    return;
                }
                self.objects
                    .get_mut(&id)
                    .unwrap()
                    .monster_mut()
                    .unwrap()
                    .cadence_time = now + 1000;
                let at_war = self.conquest.as_ref().map(|c| c.castle) == Some(castle);
                let owner = self.guild_store.castle_owner(castle);
                // Zircon opens for owner members in peacetime, and holds the
                // door while anyone at all stands within four cells.
                let (owner_near, anyone_near) =
                    self.on_map(map).fold((false, false), |(own, any), o| {
                        if o.dead || o.player().is_none() || o.location.distance(loc) > 4 {
                            return (own, any);
                        }
                        let mine = owner.is_some() && o.player().and_then(|p| p.guild) == owner;
                        (own || mine, true)
                    });
                if closed && !at_war && owner_near {
                    self.set_gate(id, false, now + 10_000);
                } else if !closed && close_at > 0 && now >= close_at && !anyone_near {
                    self.set_gate(id, true, 0);
                }
            }
            CastleRole::Flag {
                castle,
                contester,
                contest_until,
            } => {
                if now < self.objects[&id].monster_ref().cadence_time {
                    return;
                }
                self.objects
                    .get_mut(&id)
                    .unwrap()
                    .monster_mut()
                    .unwrap()
                    .cadence_time = now + 1000;
                if self.conquest.as_ref().map(|c| c.castle) != Some(castle) {
                    return;
                }
                let view = self.data.monsters[&self.objects[&id].monster_ref().def]
                    .view_range
                    .max(1);
                let name = self.castle_name(castle);
                // Guilds of the enemies standing by the flag.
                let near: Vec<u32> = self
                    .on_map(map)
                    .filter(|o| o.location.distance(loc) <= view && self.castle_enemy(castle, o.id))
                    .filter_map(|o| o.player().and_then(|p| p.guild))
                    .collect();
                match contester {
                    None => {
                        if let Some(&g) = near.first() {
                            let gname = self.guild_name(g);
                            self.set_flag(id, Some(g), now + FLAG_TAKE_MS);
                            self.announce(format!(
                                "{gname} is taking the flag of {name}: {} seconds to capture.",
                                FLAG_TAKE_MS / 1000
                            ));
                        }
                    }
                    Some(g) => {
                        if let Some(&other) = near.iter().find(|x| **x != g) {
                            let oname = self.guild_name(other);
                            self.set_flag(id, Some(g), now + FLAG_TAKE_MS);
                            self.announce(format!("{oname} is preventing the capture of {name}."));
                            return;
                        }
                        if !near.contains(&g) {
                            let gname = self.guild_name(g);
                            self.set_flag(id, None, u64::MAX);
                            self.announce(format!(
                                "{gname} is no longer taking the flag of {name}."
                            ));
                            return;
                        }
                        if now >= contest_until {
                            self.set_flag(id, None, u64::MAX);
                            self.castle_capture(castle, g);
                        }
                    }
                }
            }
        }
    }

    fn castle_name(&self, castle: i32) -> String {
        self.data
            .castles
            .iter()
            .find(|c| c.index == castle)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }

    fn guild_name(&self, guild: u32) -> String {
        self.guild_store
            .get(guild)
            .map(|g| g.name.clone())
            .unwrap_or_default()
    }

    fn set_gate(&mut self, id: ObjectId, close: bool, close_at_time: u64) {
        let dir = if close {
            let object = &self.objects[&id];
            // Source CastleGate: closed direction reflects three health stages.
            let level = (3.0 * f64::from(object.hp) / f64::from(object.max_hp.max(1)))
                .round_ties_even()
                .clamp(1.0, 3.0) as u8;
            Direction::from_index(3 - level)
        } else {
            Direction::UpLeft
        };
        if let Some(CastleRole::Gate {
            closed, close_at, ..
        }) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.monster_mut())
            .and_then(|m| m.castle.as_mut())
        {
            *closed = close;
            *close_at = close_at_time;
        }
        self.objects.get_mut(&id).unwrap().direction = dir;
        // Zircon shows the swing as the door moving.
        self.events.push((
            id,
            ServerMessage::ObjectAttack {
                id,
                direction: dir,
                attack_magic: None,
                element: mir_proto::element::NONE,
            },
        ));
    }

    fn set_flag(&mut self, id: ObjectId, guild: Option<u32>, until: u64) {
        if let Some(CastleRole::Flag {
            contester,
            contest_until,
            ..
        }) = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.monster_mut())
            .and_then(|m| m.castle.as_mut())
        {
            *contester = guild;
            *contest_until = until;
        }
    }

    /// Hand a castle to a guild (lord killed or flag held).
    pub(super) fn castle_capture(&mut self, castle: i32, guild: u32) {
        if self.guild_store.castle_of(guild).is_some() {
            return;
        }
        self.guild_store.castles.retain(|c| c.castle != castle);
        self.guild_store.castles.push(CastleOwner { castle, guild });
        self.guild_store.save();
        let gname = self.guild_name(guild);
        let name = self.castle_name(castle);
        self.announce(format!("{gname} has captured {name}!"));
        self.castle_broadcast_all();
        let guilds: Vec<u32> = self.guild_store.guilds.iter().map(|g| g.id).collect();
        for g in guilds {
            self.guild_broadcast_info(g);
        }
    }

    /// The castle objects of the castle a player's guild owns.
    fn owned_castle_objects(&self, id: ObjectId) -> Option<(i32, Vec<ObjectId>)> {
        let guild = self.objects.get(&id)?.player()?.guild?;
        let castle = self.guild_store.castle_of(guild)?;
        let map = self.data.castles.iter().find(|c| c.index == castle)?.map;
        let ids = self
            .on_map(map)
            .filter(|o| matches!(&o.kind, Kind::Monster(m) if m.castle.as_ref().is_some_and(|r| r.castle() == castle)))
            .map(|o| o.id)
            .collect();
        Some((castle, ids))
    }

    /// Zircon `GuildToggleCastleGates`: any owner member; opens every gate
    /// if one is closed, else closes them all.
    pub fn guild_toggle_castle_gates(&mut self, id: ObjectId) {
        let Some((_, ids)) = self.owned_castle_objects(id) else {
            self.send_to(
                id,
                ServerMessage::Say {
                    id: None,
                    kind: ChatKind::System,
                    text: "Your guild does not own a castle.".into(),
                },
            );
            return;
        };
        let gates: Vec<(ObjectId, bool)> = ids
            .iter()
            .filter_map(|g| match self.castle_role(*g) {
                Some(CastleRole::Gate { closed, .. }) if !self.objects[g].dead => {
                    Some((*g, *closed))
                }
                _ => None,
            })
            .collect();
        let open = gates.iter().any(|(_, closed)| *closed);
        for (g, _) in gates {
            self.set_gate(g, !open, 0);
        }
    }

    /// Zircon `GuildRepairCastleGates`: the leader pays each part's repair
    /// cost scaled by the damage from the guild funds; dead parts rise.
    pub fn guild_repair_castle_gates(&mut self, id: ObjectId) {
        let Some((_, ids)) = self.owned_castle_objects(id) else {
            return;
        };
        let Some(guild) = self.objects[&id].player().and_then(|p| p.guild) else {
            return;
        };
        let leader = self
            .guild_store
            .get(guild)
            .and_then(|g| {
                let c = self.objects[&id].player()?.character;
                g.members
                    .iter()
                    .find(|m| m.character == c)
                    .map(|m| m.permission)
            })
            .is_some_and(|p| p == mir_proto::guild_permission::LEADER);
        if !leader {
            self.send_to(
                id,
                ServerMessage::Say {
                    id: None,
                    kind: ChatKind::System,
                    text: "Only a guild leader can repair the castle.".into(),
                },
            );
            return;
        }
        let mut cost = 0i64;
        let mut hurt = Vec::new();
        for o in &ids {
            let obj = &self.objects[o];
            let repair = match self.castle_role(*o) {
                Some(CastleRole::Guard { repair_cost, .. })
                | Some(CastleRole::Gate { repair_cost, .. }) => *repair_cost,
                _ => continue,
            };
            if repair <= 0 || (!obj.dead && obj.hp >= obj.max_hp) {
                continue;
            }
            let missing = if obj.dead {
                100
            } else {
                (obj.max_hp - obj.hp).max(0) as i64 * 100 / obj.max_hp.max(1) as i64
            };
            cost += repair * missing / 100;
            hurt.push(*o);
        }
        if cost == 0 {
            return;
        }
        let funds = self.guild_store.get(guild).map(|g| g.funds).unwrap_or(0);
        if funds < cost {
            self.send_to(
                id,
                ServerMessage::Say {
                    id: None,
                    kind: ChatKind::System,
                    text: format!("Repairs cost {cost} gold from the guild funds."),
                },
            );
            return;
        }
        if let Some(g) = self.guild_store.guilds.iter_mut().find(|g| g.id == guild) {
            g.funds -= cost;
        }
        self.guild_store.save();
        for o in hurt {
            let (location, direction, hp) = {
                let obj = self.objects.get_mut(&o).unwrap();
                obj.dead = false;
                obj.hp = obj.max_hp;
                if let Some(m) = obj.monster_mut() {
                    m.dead_time = 0;
                }
                (obj.location, obj.direction, obj.hp)
            };
            self.events.push((
                o,
                ServerMessage::ObjectRevive {
                    id: o,
                    location,
                    direction,
                    hp,
                },
            ));
        }
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text: format!("The castle was repaired for {cost} gold."),
            },
        );
    }
}
