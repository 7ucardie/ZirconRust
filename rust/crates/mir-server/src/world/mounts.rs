//! Horses (Zircon `PlayerObject.Mount` / `RemoveMount`): an owned horse
//! type (bought through the NPC `ChangeHorse` action) grants stats always
//! and can be ridden on maps that allow it; riding runs three cells a step
//! but forbids attacking, casting and item use; being pushed, dying or
//! entering a no-horse map dismounts.

use super::*;
use mir_proto::ChatKind;

/// Zircon `PlayerObject.Stats` horse bonuses: (bag weight, AC/MR/DC/MC/SC).
pub fn horse_bonus(horse: u8) -> (i32, i32) {
    match horse {
        mir_proto::horse_type::BROWN => (50, 0),
        mir_proto::horse_type::WHITE => (100, 5),
        mir_proto::horse_type::RED => (150, 12),
        mir_proto::horse_type::BLACK => (200, 25),
        mir_proto::horse_type::WHITE_UNICORN | mir_proto::horse_type::RED_UNICORN => (250, 30),
        _ => (0, 0),
    }
}

impl World {
    fn mount_line(&mut self, id: ObjectId, text: &str) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text: text.to_string(),
            },
        );
    }

    /// Zircon `Mount`: toggle riding.
    pub fn mount_toggle(&mut self, id: ObjectId) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if self.now < o.action_time {
            return;
        }
        if o.dead {
            self.mount_line(id, "You cannot ride while dead.");
            return;
        }
        if p.horse == mir_proto::horse_type::NONE {
            self.mount_line(id, "You do not own a horse.");
            return;
        }
        let can_horse = self.data.maps.get(&o.map).is_some_and(|m| m.can_horse);
        if !can_horse {
            self.mount_line(id, "You cannot ride here.");
            return;
        }
        let now = self.now;
        let o = self.objects.get_mut(&id).unwrap();
        o.action_time = now + mir_proto::rules::TURN_TIME;
        let p = o.player_mut().unwrap();
        p.mounted = !p.mounted;
        // Riding breaks stealth (Zircon removes Cloak and Transparency).
        self.buff_remove(id, buff_type::CLOAK);
        self.buff_remove(id, buff_type::TRANSPARENCY);
        self.refresh_appearance(id);
    }

    /// Zircon `RemoveMount`: dismount if riding.
    pub(super) fn remove_mount(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        if !p.mounted {
            return;
        }
        p.mounted = false;
        self.refresh_appearance(id);
    }

    pub(super) fn is_mounted(&self, id: ObjectId) -> bool {
        self.objects
            .get(&id)
            .and_then(|o| o.player())
            .is_some_and(|p| p.mounted)
    }
}
