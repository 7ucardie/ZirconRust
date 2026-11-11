//! Learned skills (Zircon `UserMagic`): levels, experience, hotkeys, cost and
//! power formulas.

use mir_proto::MagicSummary;
use serde::{Deserialize, Serialize};

use crate::data::MagicDef;

/// Zircon `Globals.MagicMaxLevel`.
pub const MAGIC_MAX_LEVEL: u8 = 4;
/// Zircon `Config.SkillExp`: 1..=3 experience per successful use.
pub const SKILL_EXP: i32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMagic {
    pub magic: u16,
    pub level: u8,
    pub experience: u64,
    pub key: u8,
}

#[derive(Debug, Clone)]
pub struct UserMagic {
    pub magic: u16,
    pub level: u8,
    pub experience: u64,
    pub key: u8,
    /// Server time (ms) until which the skill is on cooldown.
    pub cooldown_until: u64,
}

impl UserMagic {
    pub fn from_stored(s: &StoredMagic) -> UserMagic {
        UserMagic {
            magic: s.magic,
            level: s.level.min(MAGIC_MAX_LEVEL),
            experience: s.experience,
            key: s.key,
            cooldown_until: 0,
        }
    }

    pub fn stored(&self) -> StoredMagic {
        StoredMagic {
            magic: self.magic,
            level: self.level,
            experience: self.experience,
            key: self.key,
        }
    }

    pub fn summary(&self) -> MagicSummary {
        MagicSummary {
            magic: self.magic,
            level: self.level,
            experience: self.experience,
            key: self.key,
        }
    }

    /// `BaseCost + Level * LevelCost / 3`.
    pub fn cost(&self, def: &MagicDef) -> i32 {
        mir_proto::rules::magic_cost(def.base_cost, def.level_cost, self.level)
    }

    /// `(min, max)` power for the current level (Zircon `UserMagic.GetPower`).
    pub fn power_range(&self, def: &MagicDef) -> (i32, i32) {
        mir_proto::rules::magic_power(
            def.min_base_power,
            def.max_base_power,
            def.min_level_power,
            def.max_level_power,
            self.level,
        )
    }

    /// Player level needed to gain experience at the current magic level.
    pub fn need_level(&self, def: &MagicDef) -> Option<i32> {
        def.need_level.get(self.level as usize).copied()
    }

    /// Experience needed to reach the next level, if it can be gained by use.
    pub fn next_experience(&self, def: &MagicDef) -> Option<i64> {
        def.experience.get(self.level as usize).map(|e| *e as i64)
    }
}
