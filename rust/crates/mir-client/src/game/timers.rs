//! The HUD countdown (Zircon `TimerDialog`).
//!
//! The server names each countdown, so a refresh replaces the one already
//! running rather than stacking a second. Several can be live at once; the
//! one closest to expiring is the one drawn, exactly as Zircon picks it.
//!
//! Zircon draws the digits from `GameInter` 6580-6589 with the colon at
//! 6590, and animates an egg timer beside them. This asset pack has the
//! digits and the colon but not the egg frames (6600+ and 961-965 are all
//! absent), so the digits are drawn alone whatever kind the server sends.

use crate::assets::lib;
use crate::ui::Ctx;

/// First digit sprite; `DIGITS + n` is the glyph for `n`.
const DIGITS: u32 = 6580;
const COLON: u32 = 6590;
/// Zircon's sprites carry this offset and are drawn with `UseOffSet`.
const OFFSET_X: f32 = -24.0;
const OFFSET_Y: f32 = -16.0;
/// Column positions inside the 120x100 box, from Zircon's control layout.
const COLS: [f32; 4] = [0.0, 25.0, 75.0, 100.0];
const COLON_COL: f32 = 50.0;
const ROW_Y: f32 = 70.0;

/// One named countdown the server started.
#[derive(Debug, Clone)]
struct Timer {
    key: String,
    /// Client time the countdown reaches zero.
    ends_at: u64,
}

/// Every live countdown; the soonest is the one shown.
#[derive(Debug, Default)]
pub struct Timers {
    timers: Vec<Timer>,
}

impl Timers {
    pub fn clear(&mut self) {
        self.timers.clear();
    }

    /// Apply a `SetTimer`: start it, refresh it, or (at zero or less) drop
    /// it. Zircon treats a repeat of a live key as a refresh, not a second
    /// timer, which is why the key exists at all.
    pub fn set(&mut self, key: String, seconds: i32, now: u64) {
        if seconds <= 0 {
            self.timers.retain(|t| t.key != key);
            return;
        }
        let ends_at = now + seconds as u64 * 1000;
        match self.timers.iter_mut().find(|t| t.key == key) {
            Some(t) => t.ends_at = ends_at,
            None => self.timers.push(Timer { key, ends_at }),
        }
    }

    /// Seconds left on the timer that expires first, once expired ones are
    /// dropped. `None` when nothing is running.
    pub fn best(&mut self, now: u64) -> Option<u64> {
        self.timers.retain(|t| t.ends_at > now);
        self.timers
            .iter()
            .map(|t| t.ends_at)
            .min()
            .map(|e| e.saturating_sub(now).div_ceil(1000))
    }

    /// Zircon splits the remaining time into two pairs: hours and minutes
    /// once there is an hour left, otherwise minutes and seconds.
    pub fn digits(total: u64) -> [u32; 4] {
        let (hours, minutes, seconds) = (total / 3600, total % 3600 / 60, total % 60);
        let (a, b) = if hours > 0 {
            (hours, minutes)
        } else {
            (minutes, seconds)
        };
        [
            (a / 10) as u32,
            (a % 10) as u32,
            (b / 10) as u32,
            (b % 10) as u32,
        ]
    }

    /// Draw the countdown at Zircon's spot: above the right end of the main
    /// panel (`MainPanel.Right - 115`, `Height - 170`).
    pub fn draw(&mut self, c: &mut Ctx, width: i32, height: i32, now: u64) {
        let Some(total) = self.best(now) else {
            return;
        };
        let (x, y) = (width as f32 - 115.0, height as f32 - 170.0);
        let digits = Timers::digits(total);
        for (col, d) in COLS.iter().zip(digits) {
            c.draw(
                lib::GAME_INTER,
                DIGITS + d,
                x + col + OFFSET_X,
                y + ROW_Y + OFFSET_Y,
            );
        }
        c.draw(
            lib::GAME_INTER,
            COLON,
            x + COLON_COL + OFFSET_X,
            y + ROW_Y + OFFSET_Y,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_split_like_zircon() {
        // Under an hour: minutes then seconds.
        assert_eq!(Timers::digits(5 * 60 + 7), [0, 5, 0, 7]);
        assert_eq!(Timers::digits(59), [0, 0, 5, 9]);
        // An hour or more: hours then minutes, so the seconds drop off.
        assert_eq!(Timers::digits(3600 + 23 * 60 + 45), [0, 1, 2, 3]);
        assert_eq!(Timers::digits(12 * 3600 + 34 * 60), [1, 2, 3, 4]);
    }

    #[test]
    fn same_key_refreshes_rather_than_stacks() {
        let mut t = Timers::default();
        t.set("Map".into(), 60, 0);
        t.set("Map".into(), 30, 0);
        assert_eq!(t.timers.len(), 1);
        assert_eq!(t.best(0), Some(30));
    }

    #[test]
    fn soonest_wins_and_zero_clears() {
        let mut t = Timers::default();
        t.set("Long".into(), 600, 0);
        t.set("Short".into(), 90, 0);
        assert_eq!(t.best(0), Some(90));
        // Clearing the short one falls back to the long one.
        t.set("Short".into(), 0, 0);
        assert_eq!(t.best(0), Some(600));
    }

    #[test]
    fn expired_timers_drop_out() {
        let mut t = Timers::default();
        t.set("A".into(), 10, 0);
        assert_eq!(t.best(5_000), Some(5));
        assert_eq!(t.best(10_000), None);
        assert!(t.timers.is_empty());
    }
}
