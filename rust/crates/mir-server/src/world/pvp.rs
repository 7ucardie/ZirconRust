//! Player versus player (Zircon `PlayerObject.Combat`): attack modes gate
//! who may be hit, hitting an innocent outside a safe zone turns you brown
//! for a minute, a murder adds PK points that decay one per minute; at 200
//! the name turns red and guards attack.

use super::*;
use mir_proto::{attack_mode, pet_mode, ChatKind};

/// Zircon `Config.RedPoint`, `BrownDuration`, `PKPointRate`, `PKPointTickRate`.
pub const RED_POINT: i32 = 200;
pub const BROWN_DURATION: u64 = 60_000;
pub const PK_POINT_RATE: i32 = 50;
pub const PK_TICK: u64 = 60_000;

/// Name colour code sent in `Appearance::Player`: 0 white, 1 yellow (50+
/// PK points), 2 brown, 3 red (Zircon `ProcessNameColour`).
/// Zircon `AtWar`: the guilds are at war, or both stand on a map under
/// conquest and are not guild mates.
pub fn at_war(me: &PlayerData, them: &PlayerData) -> bool {
    if them.guild.is_some_and(|g| me.war_guilds.contains(&g)) {
        return true;
    }
    me.conquest_map
        && them.conquest_map
        && (me.guild.is_none() || them.guild.is_none() || me.guild != them.guild)
}

pub fn name_color(p: &PlayerData) -> u8 {
    if p.pk_points >= RED_POINT {
        3
    } else if p.brown {
        2
    } else if p.pk_points >= 50 {
        1
    } else {
        0
    }
}

impl World {
    pub fn set_attack_mode(&mut self, id: ObjectId, mode: u8) {
        if mode > attack_mode::ALL {
            return;
        }
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        p.attack_mode = mode;
        self.send_to(id, ServerMessage::AttackMode { mode });
    }

    pub fn set_pet_mode(&mut self, id: ObjectId, mode: u8) {
        if mode > pet_mode::NONE {
            return;
        }
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        p.pet_mode = mode;
        self.send_to(id, ServerMessage::PetMode { mode });
    }

    /// Recompute `Object.in_safe_zone` after a move or map change.
    pub(super) fn refresh_safe_zone(&mut self, id: ObjectId) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let safe = self.in_safe_zone(o.map, o.location);
        self.objects.get_mut(&id).unwrap().in_safe_zone = safe;
    }

    /// The player behind an attacker (itself, or a pet's owner).
    fn player_side(&self, id: ObjectId) -> Option<ObjectId> {
        let o = self.objects.get(&id)?;
        let side = o.side()?;
        self.objects.get(&side)?.player().map(|_| side)
    }

    /// Zircon `CheckBrown`: damaging an innocent player outside safe zones
    /// makes the attacker brown.
    pub(super) fn check_brown(&mut self, attacker: ObjectId, target: ObjectId) {
        let Some(a) = self.player_side(attacker) else {
            return;
        };
        if a == target {
            return;
        }
        let (Some(ao), Some(to)) = (self.objects.get(&a), self.objects.get(&target)) else {
            return;
        };
        let Some(tp) = to.player() else {
            return;
        };
        if ao.in_safe_zone || to.in_safe_zone {
            return;
        }
        let now = self.now;
        if tp.brown || tp.pk_points >= RED_POINT {
            return;
        }
        // Guild wars and conquests are fair fights.
        if ao.player().is_some_and(|ap| at_war(ap, tp)) {
            return;
        }
        let ap = self
            .objects
            .get_mut(&a)
            .and_then(|o| o.player_mut())
            .unwrap();
        let was = name_color(ap);
        ap.brown_until = now + BROWN_DURATION;
        ap.brown = true;
        if name_color(ap) != was {
            self.refresh_appearance(a);
        }
    }

    /// Zircon `Die` with a player killer: murdering an innocent costs PK
    /// points; brown and red victims are fair game.
    pub(super) fn pvp_kill(&mut self, victim: ObjectId, killer: ObjectId) {
        let Some(k) = self.player_side(killer) else {
            return;
        };
        if k == victim {
            return;
        }
        let (vname, innocent) = {
            let Some(vp) = self.objects.get(&victim).and_then(|o| o.player()) else {
                return;
            };
            (vp.name.clone(), !vp.brown && vp.pk_points < RED_POINT)
        };
        let kname = self.objects[&k].player().unwrap().name.clone();
        let war = {
            let kp = self.objects[&k].player().unwrap();
            let vp = self.objects[&victim].player().unwrap();
            at_war(kp, vp)
        };
        if war {
            self.guild_war_death(victim, k);
            return;
        }
        if innocent {
            self.system_line(victim, format!("You have been murdered by {kname}."));
            self.system_line(k, format!("You have murdered {vname}."));
            self.increase_pk_points(k, PK_POINT_RATE);
        } else {
            self.system_line(victim, format!("You have been killed by {kname}."));
            self.system_line(k, "You were protected from PK points.".into());
        }
    }

    pub(super) fn increase_pk_points(&mut self, id: ObjectId, count: i32) {
        let now = self.now;
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let was = name_color(p);
        p.pk_points += count;
        if p.pk_tick == 0 {
            p.pk_tick = now + PK_TICK;
        }
        if name_color(p) != was {
            self.refresh_appearance(id);
        }
    }

    /// PK points decay one per minute; brown wears off.
    pub(super) fn process_pk(&mut self) {
        let now = self.now;
        let ids: Vec<ObjectId> = self.players().map(|o| o.id).collect();
        for id in ids {
            let p = self
                .objects
                .get_mut(&id)
                .and_then(|o| o.player_mut())
                .unwrap();
            let was = name_color(p);
            if p.pk_points > 0 && now >= p.pk_tick {
                p.pk_points -= 1;
                p.pk_tick = now + PK_TICK;
            }
            if p.pk_points == 0 {
                p.pk_tick = 0;
            }
            if p.brown && now >= p.brown_until {
                p.brown = false;
            }
            if name_color(p) != was {
                self.refresh_appearance(id);
            }
        }
    }

    fn system_line(&mut self, id: ObjectId, text: String) {
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
