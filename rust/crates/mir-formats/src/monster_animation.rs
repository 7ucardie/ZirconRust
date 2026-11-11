//! Default monster animation contract shared by runtime and sprite authoring.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Standing,
    Walking,
    Attack,
    Struck,
    Die,
    Dead,
    Skeleton,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameSpec {
    pub start: u32,
    pub count: u32,
    pub interval: u64,
}
pub const ACTIONS: [Action; 7] = [
    Action::Standing,
    Action::Walking,
    Action::Attack,
    Action::Struck,
    Action::Die,
    Action::Dead,
    Action::Skeleton,
];
pub const fn frame(action: Action) -> FrameSpec {
    let (start, count, interval) = match action {
        Action::Standing => (0, 4, 500),
        Action::Walking => (80, 6, 100),
        Action::Attack => (160, 6, 100),
        Action::Struck => (240, 2, 100),
        Action::Die => (320, 10, 100),
        Action::Dead => (329, 1, 1000),
        Action::Skeleton => (880, 1, 1000),
    };
    FrameSpec {
        start,
        count,
        interval,
    }
}
pub fn slot(shape: u16, action: Action, direction: u8, frame_index: u32) -> Option<u32> {
    let spec = frame(action);
    (direction < 8 && frame_index < spec.count).then_some(
        u32::from(shape % 10) * 1000 + spec.start + u32::from(direction) * 10 + frame_index,
    )
}
/// Castle gate images use FrameSet.SabukGate, not DefaultMonster.
pub const fn is_gate(image: u16) -> bool {
    image >= 540 && image <= 543
}
pub const fn frame_for_image(image: u16, action: Action) -> FrameSpec {
    if image == 137 {
        let (start, count, interval) = match action {
            Action::Die => (320, 9, 100),
            Action::Dead | Action::Skeleton => (328, 1, 1000),
            _ => (0, 4, 500),
        };
        return FrameSpec {
            start,
            count,
            interval,
        };
    }
    if !is_gate(image) {
        return frame(action);
    }
    let (start, count, interval) = match action {
        Action::Standing | Action::Walking => (0, 1, 1000),
        Action::Struck => (240, 2, 100),
        Action::Attack => (640, 7, 100),
        Action::Die => (320, 8, 100),
        Action::Dead | Action::Skeleton => (327, 1, 1000),
    };
    FrameSpec {
        start,
        count,
        interval,
    }
}
/// Gates use direction 7 while open and 0..2 for closed damage levels.
/// Closing plays Combat2 (the opening frames in reverse).
pub fn slot_for_image(
    image: u16,
    shape: u16,
    action: Action,
    direction: u8,
    frame_index: u32,
) -> Option<u32> {
    if image == 137 {
        let spec = frame_for_image(image, action);
        return (direction < 8 && frame_index < spec.count)
            .then_some(u32::from(shape % 10) * 1000 + spec.start + frame_index);
    }
    if !is_gate(image) {
        return slot(shape, action, direction, frame_index);
    }
    let spec = frame_for_image(image, action);
    if direction >= 8 || frame_index >= spec.count {
        return None;
    }
    let stride = match action {
        Action::Standing | Action::Walking | Action::Struck => 10,
        _ => 0,
    };
    let index = if action == Action::Attack && direction != 7 {
        spec.count - 1 - frame_index
    } else {
        frame_index
    };
    Some(u32::from(shape % 10) * 1000 + spec.start + u32::from(direction) * stride + index)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chestnut_tree_uses_directionless_source_frames() {
        assert_eq!(slot_for_image(137, 7, Action::Standing, 4, 0), Some(7000));
        assert_eq!(slot_for_image(137, 7, Action::Die, 2, 8), Some(7328));
        assert_eq!(slot_for_image(137, 7, Action::Dead, 6, 0), Some(7328));
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let library =
            crate::zl::ZlLibrary::open(std::path::PathBuf::from(assets).join("Data/Mon-13.Zl"))
                .unwrap();
        for index in [7000, 7328] {
            assert!(library.info(index).is_some(), "missing tree frame {index}");
        }
    }
    #[test]
    fn installed_gate_animation_slots_decode_visible_art() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let library =
            crate::zl::ZlLibrary::open(std::path::PathBuf::from(assets).join("Data/Mon-54.Zl"))
                .unwrap();
        for image in 540..=543 {
            for (action, directions) in [
                (Action::Standing, &[0, 1, 2, 7][..]),
                (Action::Struck, &[0, 1, 2][..]),
                (Action::Attack, &[0, 7][..]),
                (Action::Die, &[0, 7][..]),
                (Action::Dead, &[0, 7][..]),
            ] {
                for &direction in directions {
                    for frame in 0..frame_for_image(image, action).count {
                        let index =
                            slot_for_image(image, image - 540, action, direction, frame).unwrap();
                        let decoded = library
                            .decode(index as usize, crate::zl::SurfaceKind::Image)
                            .unwrap()
                            .unwrap();
                        assert!(decoded.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0), "Missing gate image {image} {action:?} direction {direction} slot {index}");
                    }
                }
            }
        }
    }
    #[test]
    fn gate_slots_match_sabuk_frames_and_reverse_closing() {
        assert_eq!(frame_for_image(543, Action::Standing).count, 1);
        assert_eq!(slot_for_image(543, 3, Action::Standing, 0, 0), Some(3000));
        assert_eq!(slot_for_image(543, 3, Action::Standing, 7, 0), Some(3070));
        for index in 0..7 {
            assert_eq!(
                slot_for_image(543, 3, Action::Attack, 7, index),
                Some(3640 + index)
            );
            assert_eq!(
                slot_for_image(543, 3, Action::Attack, 0, index),
                Some(3646 - index)
            );
        }
        assert_eq!(slot_for_image(543, 3, Action::Die, 7, 7), Some(3327));
        assert_eq!(slot_for_image(543, 3, Action::Dead, 7, 0), Some(3327));
        assert_eq!(slot_for_image(543, 3, Action::Standing, 0, 1), None);
    }
    #[test]
    fn default_runtime_slots_and_timing_are_unchanged() {
        let expected = [
            (0, 4, 500),
            (80, 6, 100),
            (160, 6, 100),
            (240, 2, 100),
            (320, 10, 100),
            (329, 1, 1000),
            (880, 1, 1000),
        ];
        for (action, (start, count, interval)) in ACTIONS.into_iter().zip(expected) {
            assert_eq!(
                frame(action),
                FrameSpec {
                    start,
                    count,
                    interval
                }
            );
            assert_eq!(
                slot(12, action, 7, count - 1),
                Some(2000 + start + 70 + count - 1)
            );
            assert_eq!(slot(0, action, 8, 0), None);
            assert_eq!(slot(0, action, 0, count), None);
        }
    }
}
