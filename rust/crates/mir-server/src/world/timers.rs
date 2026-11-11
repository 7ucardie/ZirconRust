//! Named countdowns shown on the client's HUD (Zircon `S.SetTimer` and
//! `TimerDialog`).
//!
//! Zircon's only caller is the instance expiry a player carries into a
//! dungeon; this pack has no instances (its `InstanceInfo` table is empty),
//! so the conquest window drives the timer instead. It is the same shape of
//! state: a deadline attached to the map you are standing on.

use mir_proto::{ObjectId, ServerMessage};

use super::World;

/// Zircon `SetTimer.Type`: 0 draws the digits alone, 1 and 2 add the
/// running egg timer beside them.
pub mod timer_kind {
    pub const PLAIN: u8 = 0;
    pub const RUNNING: u8 = 1;
}

/// The key the conquest countdown uses, so a refresh replaces it rather
/// than stacking a second timer.
pub const CONQUEST_KEY: &str = "Conquest";

impl World {
    /// Start, refresh or (with `seconds` <= 0) clear one player's timer.
    pub fn set_timer(&mut self, id: ObjectId, key: &str, kind: u8, seconds: i32) {
        self.send_to(
            id,
            ServerMessage::SetTimer {
                key: key.to_string(),
                kind,
                seconds,
            },
        );
    }

    /// Show every player on the conquest map how long the war has left, and
    /// clear it for anyone who has left the map. Called once a second while
    /// a conquest runs, and once more when it ends.
    pub(super) fn refresh_conquest_timers(&mut self) {
        let war = self
            .conquest
            .as_ref()
            .map(|c| (c.map, c.ends_at.saturating_sub(self.now)));
        let players: Vec<(ObjectId, i32)> = self
            .players()
            .map(|o| {
                let left = match war {
                    Some((map, ms)) if map == o.map => (ms / 1000) as i32,
                    _ => 0,
                };
                (o.id, left)
            })
            .collect();
        for (id, seconds) in players {
            // A zero clears a timer the client may not have; that is a
            // cheap no-op on the client, and keeps the two in step when a
            // player walks off the map mid-war.
            self.set_timer(id, CONQUEST_KEY, timer_kind::RUNNING, seconds);
        }
    }

    /// `ZIRCON_DEV_TIMER=<seconds>` starts a countdown as soon as a player
    /// enters the world, so the HUD element can be screenshotted without a
    /// conquest running.
    pub(super) fn dev_timer(&mut self, id: ObjectId) {
        let Some(seconds) = std::env::var("ZIRCON_DEV_TIMER")
            .ok()
            .and_then(|v| v.trim().parse::<i32>().ok())
        else {
            return;
        };
        self.set_timer(id, "Dev", timer_kind::RUNNING, seconds);
    }
}
