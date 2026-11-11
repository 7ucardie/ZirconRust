//! Player body and armour mapping shared by the runtime and sprite authoring.
pub use crate::monster_animation::FrameSpec;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Standing,
    Walking,
    Running,
    Attack,
    Attack2,
    Cast1,
    Cast2,
    Attack5,
    Attack6,
    Dash,
    Stance,
    Struck,
    Die,
    Dead,
    FishingReel,
    FishingWait,
    Mining,
    Harvest,
    FishingCast,
}
pub const ACTIONS: [Action; 19] = [
    Action::Standing,
    Action::Walking,
    Action::Running,
    Action::Attack,
    Action::Attack2,
    Action::Cast1,
    Action::Cast2,
    Action::Attack5,
    Action::Attack6,
    Action::Dash,
    Action::Stance,
    Action::Struck,
    Action::Die,
    Action::Dead,
    Action::FishingReel,
    Action::FishingWait,
    Action::Mining,
    Action::Harvest,
    Action::FishingCast,
];
/// All Rust player actions have body mappings, including the Assassin Cast2 alias.
pub fn supported_actions(_assassin: bool) -> impl Iterator<Item = Action> {
    ACTIONS.into_iter()
}
pub fn frame(action: Action, fishing: bool) -> FrameSpec {
    let (start, count, interval) = match action {
        Action::Standing => (0, 4, 500),
        Action::Walking => (80, 6, 100),
        Action::Running => (160, 6, 100),
        Action::Attack => (720, 6, 100),
        Action::Attack2 => (800, 6, 100),
        Action::Cast1 => (560, 5, 120),
        Action::Cast2 => (640, 5, 120),
        Action::Attack5 => (880, 10, 60),
        Action::Attack6 => (960, 10, 60),
        Action::Dash => (1120, 6, 50),
        Action::Stance => (400, 3, 200),
        Action::Struck => (1840, 3, 100),
        Action::Die => (1920, 10, 100),
        Action::Dead => (1929, 1, 1000),
        // This asset pack's body libraries stop at 1999 before the horse
        // block, so without the fishing frames the swing and standing poses
        // stand in.
        Action::FishingCast if fishing => (2000, 8, 100),
        Action::FishingWait if fishing => (2080, 6, 120),
        Action::FishingReel if fishing => (2160, 8, 100),
        Action::FishingCast | Action::FishingReel => (160, 6, 100),
        Action::FishingWait => (0, 4, 500),
        Action::Mining => (720, 6, 100),
        // `FrameSet.DefaultPlayer[Harvest]`: two frames at 300 ms, so the
        // swing runs exactly the 600 ms the server charges for it.
        Action::Harvest => (480, 2, 300),
    };
    FrameSpec {
        start,
        count,
        interval,
    }
}

pub const fn shape_stride(assassin: bool) -> u32 {
    if assassin {
        3000
    } else {
        5000
    }
}
pub fn body_frame(action: Action, mounted: bool, fishing: bool) -> FrameSpec {
    if mounted {
        let mounted_frame = match action {
            Action::Standing => Some((2240, 4, 500)),
            Action::Walking => Some((2320, 6, 100)),
            Action::Running => Some((2400, 6, 100)),
            Action::Struck => Some((2480, 3, 100)),
            _ => None,
        };
        if let Some((start, count, interval)) = mounted_frame {
            return FrameSpec {
                start,
                count,
                interval,
            };
        }
    }
    frame(action, fishing)
}
/// `PlayerObject.SetFrame` adjusts only Assassin body/helmet/shield frames.
pub fn armour_shift(action: Action, assassin: bool, mounted: bool, fishing: bool) -> i32 {
    if !assassin {
        return 0;
    }
    if mounted
        && matches!(
            action,
            Action::Standing | Action::Walking | Action::Running | Action::Struck
        )
    {
        return 80;
    }
    match action {
        Action::Walking | Action::Running => 1600,
        Action::Cast1 => -400,
        // The Rust action set groups several Assassin spells into Cast2.
        // C# leaves Assassin Combat2 unimplemented; use its valid Cast1 body
        // until those spell-specific animations have dedicated actions.
        Action::Cast2 => -480,
        Action::Attack2 => 80,
        Action::Attack5 | Action::Attack6 => 400,
        Action::Dash => 720,
        Action::Harvest | Action::Stance => 160,
        Action::Struck => -640,
        Action::Die | Action::Dead => -400,
        Action::FishingCast | Action::FishingReel if !fishing => 1600,
        Action::FishingCast | Action::FishingWait | Action::FishingReel if fishing => 80,
        _ => 0,
    }
}
/// Exact body slot including shape, direction, mounted and fishing shifts.
pub fn slot(
    shape: u16,
    action: Action,
    direction: u8,
    frame_index: u32,
    assassin: bool,
    mounted: bool,
    fishing: bool,
) -> Option<u32> {
    let spec = body_frame(action, mounted, fishing);
    (direction < 8 && frame_index < spec.count).then_some(
        (u32::from(shape % 11) * shape_stride(assassin)
            + spec.start
            + u32::from(direction) * 10
            + frame_index)
            .saturating_add_signed(armour_shift(action, assassin, mounted, fishing)),
    )
}
/// Zircon `PlayerObject.ArmourList`: armour shape / 11 -> body library.
pub fn armour_library(shape: u16, female: bool, assassin: bool) -> Option<u16> {
    let key = shape / 11;
    let base = match (assassin, female) {
        (false, false) => [Some(31), Some(32), Some(33), Some(34), Some(35)],
        (false, true) => [Some(42), Some(43), Some(44), Some(45), Some(46)],
        (true, false) => [Some(53), Some(54), Some(55), Some(56), None],
        (true, true) => [Some(59), Some(60), Some(61), Some(62), None],
    };
    match key {
        0..=4 => base[key as usize],
        10..=13 if !assassin => Some(if female { 47 } else { 36 } + (key - 10)),
        20 => Some(match (assassin, female) {
            (false, false) => 40,
            (false, true) => 51,
            (true, false) => 57,
            (true, true) => 63,
        }),
        _ => None,
    }
}

/// Equipment library mappings used by both renderer and authoring.
pub fn helmet_library(shape: u16, female: bool, assassin: bool) -> Option<u16> {
    if shape == 0 {
        return None;
    }
    let key = (shape - 1) / 10;
    match (assassin, female) {
        (false, false) => match key {
            0..=4 => Some(139 + key),
            10..=13 => Some(144 + key - 10),
            20 => Some(148),
            _ => None,
        },
        (false, true) => match key {
            0..=4 => Some(149 + key),
            10..=13 => Some(154 + key - 10),
            20 => Some(158),
            _ => None,
        },
        (true, false) => match key {
            0..=3 => Some(159 + key),
            20 => Some(163),
            _ => None,
        },
        (true, true) => match key {
            0..=3 => Some(164 + key),
            20 => Some(168),
            _ => None,
        },
    }
}
pub fn shield_library(shape: u16, female: bool) -> Option<u16> {
    match (female, shape / 10) {
        (false, 0) => Some(135),
        (false, 1) => Some(136),
        (true, 0) => Some(137),
        (true, 1) => Some(138),
        _ => None,
    }
}
pub fn weapon_library(shape: u16, female: bool) -> Option<u16> {
    let key = shape / 10;
    let male = match key {
        0..=6 => Some(83 + key),
        10..=16 => Some(90 + key - 10),
        110..=112 => Some(117 + key - 110),
        113 => Some(119),
        114..=116 => Some(120 + key - 114),
        120 => Some(111),
        122 => Some(112),
        126 => Some(113),
        _ => None,
    }?;
    Some(if female {
        match male {
            83..=96 => male + 14,
            111..=122 => male + 12,
            other => other,
        }
    } else {
        male
    })
}

/// Assassin dual blades have a separate right-hand library in Zircon.
/// Chaotic Heaven Blade (shape 1263) replaces the left layer with it.
pub fn weapon_right_library(shape: u16, female: bool) -> Option<u16> {
    let male = match shape / 10 {
        120 => 114,
        122 => 115,
        126 => 116,
        _ => return None,
    };
    Some(if female { male + 12 } else { male })
}

/// Zircon `PlayerObject.UpdateLibraries` horse-armour shape -> model library.
pub fn horse_armour_library(shape: u16) -> Option<u16> {
    match shape {
        1 => Some(73),
        2 => Some(74),
        3 => Some(75),
        4 => Some(76),
        5 => Some(77),
        6 => Some(79),
        7 => Some(81),
        _ => None,
    }
}

/// Deduplicated runtime body slots for ground and mounted poses.
/// `fishing` must match the client's installed-pack capability setting.
pub fn required_slots(shape: u16, assassin: bool, fishing: bool) -> Vec<u32> {
    let mut slots = std::collections::BTreeSet::new();
    for mounted in [false, true] {
        for action in supported_actions(assassin) {
            let spec = body_frame(action, mounted, fishing);
            for direction in 0..8 {
                for index in 0..spec.count {
                    slots.insert(
                        slot(shape, action, direction, index, assassin, mounted, fishing).unwrap(),
                    );
                }
            }
        }
    }
    slots.into_iter().collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn body_slots_preserve_direction_shape_mount_and_fishing_contract() {
        let expected = [
            (0, 4, 500),
            (80, 6, 100),
            (160, 6, 100),
            (720, 6, 100),
            (800, 6, 100),
            (560, 5, 120),
            (640, 5, 120),
            (880, 10, 60),
            (960, 10, 60),
            (1120, 6, 50),
            (400, 3, 200),
            (1840, 3, 100),
            (1920, 10, 100),
            (1929, 1, 1000),
            (160, 6, 100),
            (0, 4, 500),
            (720, 6, 100),
            (480, 2, 300),
            (160, 6, 100),
        ];
        for (action, expected) in ACTIONS.into_iter().zip(expected) {
            let frame = frame(action, false);
            assert_eq!((frame.start, frame.count, frame.interval), expected);
        }

        assert_eq!(
            slot(12, Action::Walking, 7, 5, false, false, false),
            Some(5155)
        );
        assert_eq!(
            slot(12, Action::Walking, 7, 5, true, false, false),
            Some(4755)
        );
        assert_eq!(
            slot(0, Action::Standing, 7, 3, false, true, false),
            Some(2313)
        );
        assert_eq!(
            slot(0, Action::FishingCast, 7, 7, true, false, true),
            Some(2157)
        );
        assert_eq!(
            slot(0, Action::FishingCast, 7, 5, true, false, false),
            Some(1835)
        );
        assert_eq!(slot(0, Action::Dead, 7, 0, false, false, false), Some(1999));
        assert_eq!(slot(0, Action::Walking, 8, 0, false, false, false), None);
        assert_eq!(slot(0, Action::Walking, 0, 6, false, false, false), None);
        for assassin in [false, true] {
            for fishing in [false, true] {
                let slots = required_slots(10, assassin, fishing);
                assert!(slots.windows(2).all(|pair| pair[0] < pair[1]));
                assert!(slots.iter().all(|index| {
                    (10 * shape_stride(assassin)..11 * shape_stride(assassin)).contains(index)
                }));
            }
        }
        for (shape, male, female, assassin_male, assassin_female) in [
            (0, Some(31), Some(42), Some(53), Some(59)),
            (43, Some(34), Some(45), Some(56), Some(62)),
            (44, Some(35), Some(46), None, None),
            (110, Some(36), Some(47), None, None),
            (143, Some(39), Some(50), None, None),
            (220, Some(40), Some(51), Some(57), Some(63)),
            (231, None, None, None, None),
        ] {
            assert_eq!(armour_library(shape, false, false), male);
            assert_eq!(armour_library(shape, true, false), female);
            assert_eq!(armour_library(shape, false, true), assassin_male);
            assert_eq!(armour_library(shape, true, true), assassin_female);
        }
    }
    #[test]
    fn installed_body_contract_covers_both_genders_and_classes() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let assets = std::path::PathBuf::from(assets);
        for assassin in [false, true] {
            for female in [false, true] {
                let id = armour_library(0, female, assassin).unwrap();
                let path = assets.join(crate::monster_table::library_path(id).unwrap());
                let library = crate::zl::ZlLibrary::open(path).unwrap();
                for shape in [0, 1] {
                    let required = required_slots(shape, assassin, false);
                    let missing: Vec<_> = required
                        .iter()
                        .filter(|i| {
                            library.info(**i as usize).is_none_or(|info| {
                                info.width == 0 || info.height == 0 || info.position == 0
                            })
                        })
                        .copied()
                        .collect();
                    assert!(missing.len() < required.len());
                    assert!(
                        missing.is_empty(),
                        "Runtime body slots must exist: {missing:?}"
                    );
                    println!(
                        "wearable library={id} shape={shape} slots={} missing_count={} first_missing={:?}",
                        required.len(),
                        missing.len(),
                        &missing[..missing.len().min(8)]
                    );
                }
            }
        }
    }

    #[test]
    fn csharp_weapon_keys_and_odyn_source_frames() {
        // PlayerObject.WeaponList: 10..16 are Weapon10..16, not 9..15.
        for (shape, male, female) in [
            (100, 90, 104),
            (136, 93, 107), // all four installed Odyn weapons
            (160, 96, 110),
            (1100, 117, 129),
            (1120, 119, 131),
            (1130, 119, 131),
            (1160, 122, 134),
            (1200, 111, 123),
            (1220, 112, 124),
            (1263, 113, 125),
        ] {
            assert_eq!(weapon_library(shape, false), Some(male));
            assert_eq!(weapon_library(shape, true), Some(female));
        }
        assert_eq!(weapon_right_library(1200, false), Some(114));
        assert_eq!(weapon_right_library(1220, true), Some(127));
        assert_eq!(weapon_right_library(1263, false), Some(116));
        if let Ok(root) = std::env::var("ZIRCON_ASSETS") {
            for library in [93, 107] {
                let path = std::path::Path::new(&root)
                    .join(crate::monster_table::library_path(library).unwrap());
                let source = crate::zl::ZlLibrary::open(path).unwrap();
                assert!(source.info(30_000).is_some(), "Odyn source frame missing");
            }
        }
    }

    #[test]
    fn csharp_horse_armour_libraries_include_royal_and_blue_dragon() {
        for (shape, library) in [
            (1, 73),
            (2, 74),
            (3, 75),
            (4, 76),
            (5, 77),
            (6, 79),
            (7, 81),
        ] {
            assert_eq!(horse_armour_library(shape), Some(library));
        }
        assert_eq!(horse_armour_library(0), None);
    }
}
