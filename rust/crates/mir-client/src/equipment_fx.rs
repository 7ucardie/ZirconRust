//! Original Zircon equipment-effect frame tables.

use mir_proto::{Direction, Gender};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectSprite {
    pub library: u16,
    pub frame: u32,
    pub alpha: f32,
    pub blend: bool,
}

/// Low-opacity colour for a copy of the equipped Odyn sprite. No borrowed
/// blade overlay can align to Odyn's mace/staff silhouette.
pub fn weapon_glow_colour(effect: u8) -> Option<[f32; 4]> {
    match effect {
        240 => Some([0.34, 0.65, 1.0, 0.32]), // Son: blue.
        241 => Some([1.0, 0.76, 0.28, 0.32]), // Mythical: gold.
        242 => Some([0.34, 1.0, 0.70, 0.32]), // Elemental: green.
        243 => Some([1.0, 0.30, 0.42, 0.32]), // Sin: red.
        _ => None,
    }
}

/// The C# client puts auras in front, emblem rings behind, and other effects
/// on the side of the player selected by the facing direction.
pub fn behind(effect: u8, direction: Direction) -> bool {
    match effect {
        1..=3 => false,
        200..=202 => true,
        110..=112 => matches!(
            direction,
            Direction::Up | Direction::UpLeft | Direction::Left | Direction::DownLeft
        ),
        50..=53 => matches!(
            direction,
            Direction::UpRight | Direction::Right | Direction::DownRight
        ),
        _ => matches!(
            direction,
            Direction::DownRight | Direction::Down | Direction::DownLeft
        ),
    }
}

/// `EquipEffect-Part/Full/FullEx*` and `MonMagicEx26` frame layout.
pub fn world_sprites(
    effect: u8,
    gender: Gender,
    direction: Direction,
    body_frame: u32,
    animation: u32,
) -> Vec<EffectSprite> {
    let dir = direction.index() as u32 * 20;
    let tick = animation / 2;
    let sprite = |library, frame, alpha, blend| EffectSprite {
        library,
        frame,
        alpha,
        blend,
    };
    match effect {
        1 => vec![sprite(8, 800 + tick % 13, 0.7, true)],
        2 => vec![sprite(8, 820 + tick % 13, 0.7, true)],
        3 => vec![sprite(8, 840 + tick % 13, 0.7, true)],
        10 => vec![sprite(8, 400 + dir + tick % 15, 1.0, true)],
        11 => vec![sprite(8, 200 + dir + tick % 15, 1.0, true)],
        12 => vec![sprite(8, dir + tick % 15, 1.0, true)],
        13 => vec![sprite(8, 600 + dir + tick % 13, 1.0, true)],
        14 => vec![sprite(
            8,
            4566 + direction.index() as u32 * 9 + tick % 4,
            1.0,
            true,
        )],
        15 => vec![sprite(
            8,
            4454 + direction.index() as u32 * 9 + tick % 4,
            1.0,
            true,
        )],
        16 => vec![sprite(8, 4062 + dir + tick % 8, 1.0, true)],
        17 => vec![sprite(8, 4258 + dir + tick % 8, 1.0, true)],
        18 => vec![sprite(8, 4678 + dir + tick % 8, 1.0, true)],
        50..=53 => {
            let base =
                900 + u32::from(effect - 50) * 200 + direction.index() as u32 * 10 + animation % 4;
            vec![
                sprite(8, base, 1.0, false),
                sprite(8, base + 100, 1.0, true),
            ]
        }
        100..=106 => {
            let base = match effect {
                100 => 0,
                101 => 10000,
                102 => 50000,
                103 => 60000,
                104 => 70000,
                105 => 80000,
                _ => 90000,
            };
            vec![sprite(9, base + body_frame, 1.0, true)]
        }
        110..=112 => {
            let base = if effect == 110 { 40000 } else { 20000 };
            vec![sprite(
                9,
                base + if gender == Gender::Female { 5000 } else { 0 } + body_frame,
                1.0,
                true,
            )]
        }
        120 | 121 => vec![sprite(
            10,
            if effect == 121 { 10000 } else { 0 } + body_frame,
            1.0,
            true,
        )],
        140 => vec![sprite(11, body_frame, 1.0, true)],
        160 => vec![sprite(12, body_frame, 1.0, true)],
        200 => vec![
            sprite(184, 90 + tick % 24, 1.0, true),
            sprite(184, 140 + tick % 28, 1.0, true),
        ],
        201 => vec![
            sprite(184, 220 + tick % 25, 1.0, true),
            sprite(184, 180 + tick % 28, 1.0, true),
        ],
        202 => vec![
            sprite(184, 330 + tick % 20, 1.0, true),
            sprite(184, 270 + tick % 28, 1.0, true),
        ],
        203 => vec![sprite(184, 360 + tick % 10, 1.0, true)],
        _ => Vec::new(),
    }
}

/// Static character-panel art from C# `EquipEffectDecider`. Item image is a
/// fallback for original armours whose database `ExteriorEffect` is unset.
pub fn preview_frame(effect: u8, item_image: i32, gender: Gender, animation: u32) -> Option<u32> {
    let female = gender == Gender::Female;
    let tick = animation;
    let frame = match effect {
        1 => {
            if female {
                620
            } else {
                600
            }
        }
        2 => {
            if female {
                621
            } else {
                601
            }
        }
        3 => {
            if female {
                622
            } else {
                602
            }
        }
        9 => (if female { 1720 } else { 1700 }) + tick % 10,
        10 => (if female { 420 } else { 400 }) + tick % 15,
        11 => (if female { 320 } else { 300 }) + tick % 15,
        12 => (if female { 220 } else { 200 }) + tick % 15,
        13 => (if female { 520 } else { 500 }) + tick % 13,
        50..=53 => 2800 + u32::from(effect - 50) * 10 + tick % 10,
        100 => (if female { 120 } else { 100 }) + tick % 10,
        101 => {
            if female {
                1820
            } else {
                1800
            }
        }
        102..=105 => 2100 + u32::from(effect - 102),
        106 => 2200 + tick % 11,
        110 => 2000 + tick % 10,
        111 => 1900 + tick % 12,
        112 => 1920 + tick % 12,
        120 => 2300 + tick % 15,
        121 => 3000,
        140 => (if female { 2500 } else { 2400 }) + tick % 14,
        160 => (if female { 2700 } else { 2600 }) + tick % 15,
        _ => match item_image {
            942 => 700,
            952 => 720,
            961 => 1600,
            971 => 1620,
            982 => 800,
            992 => 820,
            983 => 1200,
            993 => 1220,
            984 => 1100,
            994 => 1120,
            1022 => 900,
            1032 => 920,
            1023 => 1300,
            1033 => 1320,
            1002 => 1000,
            1012 => 1020,
            1003 => 1400,
            1013 => 1420,
            _ => return None,
        },
    };
    Some(frame)
}

/// C# item-image preset plus ZirconRust's authored armour glow, when present.
pub fn armour_preview_frames(
    item: i32,
    database_effect: u8,
    image: i32,
    gender: Gender,
    animation: u32,
) -> [Option<u32>; 2] {
    let preset = preview_frame(database_effect, image, gender, animation);
    let authored = (database_effect == 0)
        .then(|| mir_proto::equipment_visual::armour_effect(item, 0))
        .filter(|effect| *effect != 0)
        .and_then(|effect| preview_frame(effect, 0, gender, animation));
    [preset, authored]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_effects_select_original_libraries() {
        for effect in [50, 51, 52, 53, 110, 111, 112] {
            assert!(!world_sprites(effect, Gender::Male, Direction::Down, 40, 0).is_empty());
            assert!(preview_frame(effect, 0, Gender::Male, 0).is_some());
        }
        assert_eq!(
            world_sprites(1, Gender::Male, Direction::Down, 40, 0)[0].frame,
            800
        );
        assert_eq!(
            world_sprites(110, Gender::Female, Direction::Down, 40, 0)[0].frame,
            45040
        );
        assert!(behind(110, Direction::Up));
        assert!(!behind(110, Direction::Down));
    }

    #[test]
    fn authored_wyvern_glow_keeps_original_panel_overlay_and_commoner_plain() {
        assert_eq!(
            armour_preview_frames(351, 0, 971, Gender::Female, 0),
            [Some(1620), Some(622)]
        );
        assert_eq!(
            armour_preview_frames(128, 0, 951, Gender::Female, 0),
            [None, None]
        );
        assert_eq!(
            world_sprites(3, Gender::Female, Direction::Down, 0, 0)[0].frame,
            840
        );
        if let Ok(root) = std::env::var("ZIRCON_ASSETS") {
            let mut assets = crate::assets::Assets::new(root);
            for (library, frame) in [(8, 840), (7, 1620), (7, 622)] {
                assert!(
                    assets.info(library, frame).is_some(),
                    "missing authored armour frame {library}:{frame}"
                );
            }
            for effect in [1, 2, 3, 11, 12, 13] {
                let world = world_sprites(effect, Gender::Female, Direction::Down, 0, 0);
                let preview = preview_frame(effect, 0, Gender::Female, 0).unwrap();
                assert!(!world.is_empty());
                for sprite in world {
                    assert!(
                        assets.info(sprite.library, sprite.frame).is_some(),
                        "missing world armour effect {effect}"
                    );
                }
                assert!(
                    assets.info(7, preview).is_some(),
                    "missing preview armour effect {effect}"
                );
            }
        }
    }

    #[test]
    fn odyn_glow_uses_its_own_weapon_silhouette() {
        for (item, effect) in [(1069, 240), (1070, 241), (1072, 242), (1079, 243)] {
            assert_eq!(mir_proto::equipment_visual::weapon_effect(item, 0), effect);
            assert!(weapon_glow_colour(effect).is_some());
        }
        assert_eq!(mir_proto::equipment_visual::weapon_effect(1069, 111), 111);
        assert!(weapon_glow_colour(111).is_none());
        if let Ok(root) = std::env::var("ZIRCON_ASSETS") {
            let mut assets = crate::assets::Assets::new(root);
            for library in [93, 107] {
                assert!(assets.info(library, 30_000).is_some());
            }
            assert!(assets.info(crate::assets::lib::EQUIP, 3422).is_some());
        }
    }

    #[test]
    fn live_effect_frames_exist_in_assets() {
        let Ok(root) = std::env::var("ZIRCON_ASSETS") else {
            return;
        };
        let mut assets = crate::assets::Assets::new(root);
        for effect in [50, 51, 52, 53, 110, 111, 112] {
            for direction in Direction::ALL {
                for frame in 0..4 {
                    for sprite in world_sprites(
                        effect,
                        Gender::Male,
                        direction,
                        10 * direction.index() as u32 + frame,
                        frame,
                    ) {
                        assert!(
                            assets.info(sprite.library, sprite.frame).is_some(),
                            "missing world sprite {effect}:{}:{}",
                            sprite.library,
                            sprite.frame
                        );
                    }
                }
            }
            let frame = preview_frame(effect, 0, Gender::Male, 0).unwrap();
            assert!(
                assets.info(7, frame).is_some(),
                "missing preview {effect}:{frame}"
            );
        }
        for effect in [
            1, 2, 3, 10, 11, 12, 13, 100, 101, 102, 103, 104, 105, 106, 120, 121, 140, 160,
        ] {
            for sprite in world_sprites(effect, Gender::Male, Direction::Down, 40, 0) {
                assert!(
                    assets.info(sprite.library, sprite.frame).is_some(),
                    "missing optional world sprite {effect}:{}:{}",
                    sprite.library,
                    sprite.frame
                );
            }
        }
    }
}
