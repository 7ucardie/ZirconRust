//! Sound triggers (Zircon `MapObject`/`DXSoundManager` rules): swings by
//! weapon shape, monster attack/struck/die by `MonsterImage`, magic
//! cast/travel/end, effects, footsteps and spell-object loops.

use super::*;
use crate::sound_table::{self, idx};
use mir_proto::{effect, item_type, spell_effect};

/// `MapInfo.Music` is a `SoundIndex`; negative / out of range means none.
pub(super) fn music_index(music: i32) -> u16 {
    music.clamp(0, u16::MAX as i32) as u16
}

/// Weapon swing by `LibraryWeaponShape` (`PlayerObject` attack switch).
fn swing_sound(class: Class, weapon: Option<u16>) -> u16 {
    let shape = weapon.unwrap_or(0);
    if class == Class::Assassin {
        if shape >= 1200 {
            return idx::CLAW_ATTACK;
        }
        if shape >= 1100 {
            return idx::GLAIVE_ATTACK;
        }
    }
    match shape {
        100 => idx::WAND_SWING,
        9 | 101 => idx::WOOD_SWING,
        102 => idx::AXE_SWING,
        103 => idx::DAGGER_SWING,
        104 => idx::SHORT_SWORD_SWING,
        26 | 105 => idx::IRON_SWORD_SWING,
        _ => idx::FIST_SWING,
    }
}

pub(super) fn object_effect_sound(kind: u8) -> u16 {
    match kind {
        effect::TELEPORT_OUT => idx::TELEPORT_OUT,
        effect::TELEPORT_IN => idx::TELEPORT_IN,
        effect::FULL_BLOOM => idx::FULL_BLOOM,
        effect::WHITE_LOTUS => idx::WHITE_LOTUS,
        effect::RED_LOTUS => idx::RED_LOTUS,
        effect::SWEET_BRIER => idx::SWEET_BRIER,
        effect::KARMA => idx::KARMA,
        effect::FLASH_OF_LIGHT => idx::FLASH_OF_LIGHT_END,
        effect::HUNDRED_FIST => idx::HUNDRED_FIST,
        effect::ELEMENTAL_SWORD => idx::ELEMENTAL_SWORDS_END,
        effect::BURNING_FIRE | effect::DEMON_EXPLOSION => idx::FIRE_STORM_END,
        effect::PUPPET => idx::SUMMON_SKELETON_END,
        _ => 0,
    }
}

pub(super) fn map_effect_sound(kind: u8) -> u16 {
    match kind {
        effect::BURNING_FIRE => idx::FIRE_STORM_END,
        effect::HUNDRED_FIST => idx::HUNDRED_FIST,
        _ => 0,
    }
}

/// Item cell pick/put/use sound by `ItemType` (`DXItemCell`).
pub(super) fn item_sound(def: &ItemDef) -> u16 {
    match def.item_type {
        item_type::WEAPON => idx::ITEM_WEAPON,
        item_type::ARMOUR => idx::ITEM_ARMOUR,
        item_type::HELMET => idx::ITEM_HELMET,
        item_type::NECKLACE => idx::ITEM_NECKLACE,
        item_type::BRACELET => idx::ITEM_BRACELET,
        item_type::RING => idx::ITEM_RING,
        item_type::SHOES => idx::ITEM_SHOES,
        item_type::CONSUMABLE if def.shape == 0 => idx::ITEM_POTION,
        _ => idx::ITEM_DEFAULT,
    }
}

impl Game {
    /// Swing / monster attack sound plus the attack-skill sound.
    pub(super) fn sfx_attack(&mut self, id: ObjectId, attack_magic: Option<u16>) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let mut sounds: Vec<u16> = Vec::new();
        match &o.appearance {
            Appearance::Player {
                class,
                weapon,
                gender,
                ..
            } => {
                sounds.push(swing_sound(*class, *weapon));
                if let Some(m) = attack_magic {
                    let table = sound_table::attack_magic(m);
                    // Slaying lists the male then the female grunt.
                    let pick = if table.len() >= 2 && *gender == Gender::Female {
                        table[1]
                    } else {
                        table.first().copied().unwrap_or(0)
                    };
                    sounds.push(pick);
                }
            }
            Appearance::Monster { image, .. } => sounds.push(sound_table::monster_sounds(*image).0),
            _ => {}
        }
        for s in sounds {
            self.audio.play(s);
        }
    }

    pub(super) fn sfx_struck(&mut self, id: ObjectId) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let sounds = match &o.appearance {
            Appearance::Player { gender, .. } => [
                if *gender == Gender::Female {
                    idx::FEMALE_STRUCK
                } else {
                    idx::MALE_STRUCK
                },
                idx::GENERIC_STRUCK_PLAYER,
            ],
            Appearance::Monster { image, .. } => [
                sound_table::monster_sounds(*image).1,
                idx::GENERIC_STRUCK_MONSTER,
            ],
            _ => return,
        };
        for s in sounds {
            self.audio.play(s);
        }
    }

    pub(super) fn sfx_die(&mut self, id: ObjectId) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let sound = match &o.appearance {
            Appearance::Player { gender, .. } => {
                if *gender == Gender::Female {
                    idx::FEMALE_DIE
                } else {
                    idx::MALE_DIE
                }
            }
            Appearance::Monster { image, .. } => sound_table::monster_sounds(*image).2,
            _ => return,
        };
        self.audio.play(sound);
    }

    /// Spell objects announce themselves when they appear.
    pub(super) fn sfx_appear(&mut self, state: &ObjectState) {
        if let Appearance::Spell { effect } = state.appearance {
            let sound = match effect {
                spell_effect::POISONOUS_CLOUD => idx::POISONOUS_CLOUD_START,
                spell_effect::DARK_SOUL_PRISON => idx::DARK_SOUL_PRISON,
                _ => 0,
            };
            self.audio.play(sound);
        }
    }

    /// Sound for an inventory / equipment cell interaction.
    pub(super) fn sfx_item_slot(&mut self, grid: Grid, slot: u8) {
        let cells = match grid {
            Grid::Inventory => &self.inventory,
            Grid::Equipment => &self.equipment,
            Grid::Storage => &self.storage,
        };
        let sound = cells
            .get(slot as usize)
            .and_then(|c| c.as_ref())
            .and_then(|i| self.catalog.get(i.info))
            .map(item_sound)
            .unwrap_or(0);
        self.audio.play(sound);
    }

    /// Footsteps on walk/run frames 1 and 4 of the local player, and the
    /// fire wall / tempest hums while one is within 20 cells.
    pub(super) fn sfx_frame(&mut self, before: Option<(Action, u32)>, now: u64) {
        let Some(u) = self.user() else {
            return;
        };
        let step = (u.action, u.frame_index);
        let user_loc = u.location;
        if matches!(u.action, Action::Walking | Action::Running)
            && (u.frame_index == 1 || u.frame_index == 4)
            && before != Some(step)
        {
            self.audio.play(idx::FOOT1 + 1 + (now / 97 % 3) as u16);
        }
        for (effect, sound) in [
            (spell_effect::FIRE_WALL, idx::FIRE_WALL_DURATION),
            (spell_effect::TEMPEST, idx::TEMPEST_DURATION),
        ] {
            let near = self.objects.values().any(|o| {
                matches!(o.appearance, Appearance::Spell { effect: e } if e == effect)
                    && o.location.distance(user_loc) <= 20
            });
            if near {
                self.audio.play(sound);
            } else if self.audio.is_looping(sound) {
                self.audio.stop(sound);
            }
        }
    }
}
