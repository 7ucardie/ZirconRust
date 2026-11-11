//! Client-side objects and their animation state, following Zircon's
//! `MapObject.UpdateFrame` / `FrameSet` tables.

use std::collections::VecDeque;

use mir_proto::{Action, Appearance, Direction, ObjectId, Point};

#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub start: u32,
    pub count: u32,
    pub interval: u64,
}

impl Frame {
    pub const fn new(start: u32, count: u32, interval: u64) -> Frame {
        Frame {
            start,
            count,
            interval,
        }
    }
    pub fn sum(&self) -> u64 {
        self.count as u64 * self.interval
    }
}

/// `FrameSet.Players` (all have direction stride 10).
/// Whether the body libraries carry the fishing frame block (2000-2239).
static FISHING_FRAMES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_fishing_frames(available: bool) {
    FISHING_FRAMES.store(available, std::sync::atomic::Ordering::Relaxed);
}

fn fishing_frames() -> bool {
    FISHING_FRAMES.load(std::sync::atomic::Ordering::Relaxed)
}

fn wearable_action(action: Action) -> mir_formats::wearable_animation::Action {
    use mir_formats::wearable_animation::Action as WearableAction;
    match action {
        Action::Standing => WearableAction::Standing,
        Action::Walking => WearableAction::Walking,
        Action::Running => WearableAction::Running,
        Action::Attack => WearableAction::Attack,
        Action::Attack2 => WearableAction::Attack2,
        Action::Cast1 => WearableAction::Cast1,
        Action::Cast2 => WearableAction::Cast2,
        Action::Attack5 => WearableAction::Attack5,
        Action::Attack6 => WearableAction::Attack6,
        Action::Dash => WearableAction::Dash,
        Action::Stance => WearableAction::Stance,
        Action::Struck => WearableAction::Struck,
        Action::Die => WearableAction::Die,
        Action::Dead => WearableAction::Dead,
        Action::FishingReel => WearableAction::FishingReel,
        Action::FishingWait => WearableAction::FishingWait,
        Action::Mining => WearableAction::Mining,
        Action::Harvest => WearableAction::Harvest,
        Action::FishingCast => WearableAction::FishingCast,
    }
}
pub fn player_frame(action: Action) -> Frame {
    let spec = mir_formats::wearable_animation::frame(wearable_action(action), fishing_frames());
    Frame::new(spec.start, spec.count, spec.interval)
}

static FIREWALL_FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(5);

pub fn set_firewall_frames(count: u32) {
    FIREWALL_FRAMES.store(count.clamp(1, 5), std::sync::atomic::Ordering::Relaxed);
}

/// Spell objects (`Client/Models/SpellObject.cs`): looping floor animations.
pub fn spell_frame(effect: u8) -> Frame {
    match effect {
        mir_proto::spell_effect::FIRE_WALL => Frame::new(
            920,
            FIREWALL_FRAMES.load(std::sync::atomic::Ordering::Relaxed),
            150,
        ),
        mir_proto::spell_effect::TEMPEST => Frame::new(920, 10, 150),
        mir_proto::spell_effect::TRAP_OCTAGON => Frame::new(640, 10, 100),
        mir_proto::spell_effect::POISONOUS_CLOUD => Frame::new(400, 15, 100),
        mir_proto::spell_effect::DEATH_CLOUD => Frame::new(850, 10, 100),
        // Exact persistent-field ranges from SpellObject.UpdateLibraries.
        mir_proto::spell_effect::ICE_AURA => Frame::new(2600, 10, 150),
        mir_proto::spell_effect::BURNING_FIRE => Frame::new(1000, 8, 100),
        mir_proto::spell_effect::DARK_SOUL_PRISON => Frame::new(700, 10, 100),
        // Rubble (ProgUse 230..234 by pile size; drawn at the smallest).
        mir_proto::spell_effect::RUBBLE => Frame::new(230, 1, 3_600_000),
        _ => Frame::new(0, 1, 3_600_000),
    }
}

/// Persistent field sprite libraries from `SpellObject.UpdateLibraries`.
pub fn spell_library(effect: u8) -> Option<u16> {
    use crate::effects;
    use mir_proto::spell_effect::*;
    Some(match effect {
        FIRE_WALL | TRAP_OCTAGON => effects::MAGIC,
        TEMPEST => effects::MAGIC_EX2,
        POISONOUS_CLOUD => effects::MAGIC_EX4,
        DEATH_CLOUD => 171, // LibraryFile.MonMagicEx2
        ICE_AURA => effects::MAGIC_EX5,
        BURNING_FIRE | DARK_SOUL_PRISON => effects::MAGIC_EX6,
        RUBBLE => effects::PROG_USE,
        _ => return None,
    })
}

/// NPC standing loop (`Client/Models/NPCObject.cs`): a few images have
/// longer or static animations, the rest use 4 frames at 1 s.
pub fn npc_frame(image: u16) -> Frame {
    match image {
        64 | 65 | 91 | 92 | 93 | 157 | 158 | 160 | 165 | 166 | 168 | 208 | 209 | 210 | 211
        | 212 | 213 | 214 | 231 | 234 => Frame::new(0, 1, 3_600_000),
        56 | 57 => Frame::new(0, 12, 200),
        156 => Frame::new(0, 16, 200),
        _ => Frame::new(0, 4, 1000),
    }
}

/// `FrameSet.DefaultMonster`.
pub fn monster_frame(action: Action) -> Frame {
    let spec = mir_formats::monster_animation::frame(monster_action(action));
    Frame::new(spec.start, spec.count, spec.interval)
}

fn monster_action(action: Action) -> mir_formats::monster_animation::Action {
    use mir_formats::monster_animation::Action as MonsterAction;
    match action {
        Action::Standing
        | Action::FishingCast
        | Action::FishingWait
        | Action::FishingReel
        | Action::Mining
        | Action::Harvest => MonsterAction::Standing,
        Action::Walking | Action::Running | Action::Dash => MonsterAction::Walking,
        Action::Attack
        | Action::Attack2
        | Action::Attack5
        | Action::Attack6
        | Action::Cast1
        | Action::Cast2
        | Action::Stance => MonsterAction::Attack,
        Action::Struck => MonsterAction::Struck,
        Action::Die => MonsterAction::Die,
        Action::Dead => MonsterAction::Dead,
    }
}

fn monster_frame_for_image(image: u16, action: Action) -> Frame {
    if image == 273 {
        // Sama Prophet has no walking art; hold its standing pose while moving.
        return match action {
            Action::Standing
            | Action::FishingCast
            | Action::FishingWait
            | Action::FishingReel
            | Action::Mining
            | Action::Harvest => Frame::new(50, 4, 500),
            Action::Walking | Action::Running | Action::Dash => Frame::new(50, 1, 600),
            Action::Attack2 => Frame::new(210, 9, 100),
            Action::Cast1 | Action::Cast2 => Frame::new(290, 10, 100),
            Action::Struck => Frame::new(370, 3, 100),
            Action::Die => Frame::new(450, 10, 100),
            Action::Dead => Frame::new(459, 1, 500),
            Action::Attack | Action::Attack5 | Action::Attack6 | Action::Stance => {
                Frame::new(130, 9, 100)
            }
        };
    }
    if !mir_formats::monster_animation::is_gate(image) && image != 137 {
        return monster_frame(action);
    }
    let spec = mir_formats::monster_animation::frame_for_image(image, monster_action(action));
    Frame::new(spec.start, spec.count, spec.interval)
}

/// The harvested corpse uses the same shared contract as the authoring tools.
pub fn monster_skeleton_frame() -> Frame {
    let spec =
        mir_formats::monster_animation::frame(mir_formats::monster_animation::Action::Skeleton);
    Frame::new(spec.start, spec.count, spec.interval)
}

#[derive(Debug, Clone)]
pub struct Queued {
    pub action: Action,
    pub direction: Direction,
    /// Destination cell (for moves) or current cell.
    pub location: Point,
    pub distance: i32,
}

#[derive(Debug)]
pub struct ClientObject {
    pub id: ObjectId,
    pub appearance: Appearance,
    pub location: Point,
    pub direction: Direction,
    pub hp: i32,
    pub max_hp: i32,
    pub dead: bool,
    pub room_door: bool,
    /// Light radius (Zircon `MapObject.Light`).
    pub light: u8,
    pub action: Action,
    pub frame: Frame,
    pub frame_index: u32,
    pub frame_start: u64,
    pub move_distance: i32,
    pub moving_offset: (i32, i32),
    pub queue: VecDeque<Queued>,
    /// Show the health bar until this time.
    pub health_time: u64,
    /// Floating damage numbers: (value, time shown).
    pub damage: Vec<(i32, u64)>,
    pub poison_kinds: u16,
    stunned_at: Option<u64>,
    /// Visible buffs (Zircon `VisibleBuffs`): Magic Shield etc.
    pub visible_buffs: Vec<u16>,
    /// Last local chat line and when it was said (overhead bubble).
    pub bubble: Option<(String, u64)>,
    pub quest_marker: u8,
}

impl ClientObject {
    pub fn new(state: &mir_proto::ObjectState, now: u64) -> ClientObject {
        let action = if state.dead {
            Action::Dead
        } else {
            Action::Standing
        };
        let frame = match &state.appearance {
            Appearance::Player { .. } => player_frame(action),
            // Zircon `MonsterObject.UpdateFrame`: a dead body the server has
            // marked picked clean draws its bones instead of its corpse.
            Appearance::Monster {
                skeleton, image, ..
            } if *skeleton
                && action == Action::Dead
                && !mir_formats::monster_animation::is_gate(*image) =>
            {
                monster_skeleton_frame()
            }
            // The plant rises from a buried dirt patch before its standing loop.
            Appearance::Monster { image: 41, .. } if !state.dead => Frame::new(640, 8, 100),
            Appearance::Monster { image, .. } => monster_frame_for_image(*image, action),
            Appearance::Npc { image, .. } => npc_frame(*image),
            Appearance::Item { .. } => Frame::new(0, 1, 3_600_000),
            Appearance::Spell { effect } => spell_frame(*effect),
        };
        ClientObject {
            id: state.id,
            appearance: state.appearance.clone(),
            location: state.location,
            direction: state.direction,
            hp: state.hp,
            max_hp: state.max_hp,
            dead: state.dead,
            room_door: state.room_door,
            light: state.light,
            action,
            frame,
            frame_index: 0,
            frame_start: if matches!(&state.appearance, Appearance::Monster { image: 41, .. })
                && !state.dead
            {
                now
            } else {
                now.saturating_sub((now % 5) * 100)
            },
            move_distance: 0,
            moving_offset: (0, 0),
            queue: VecDeque::new(),
            health_time: 0,
            damage: Vec::new(),
            poison_kinds: 0,
            stunned_at: None,
            visible_buffs: Vec::new(),
            bubble: None,
            quest_marker: 0,
        }
    }

    pub fn is_spell(&self) -> bool {
        matches!(self.appearance, Appearance::Spell { .. })
    }

    /// Owner of a pet monster.
    pub fn pet_owner(&self) -> Option<&str> {
        match &self.appearance {
            Appearance::Monster { owner, .. } => owner.as_deref(),
            _ => None,
        }
    }

    /// Name as drawn: pets show their owner (Zircon `Name (Owner)`).
    pub fn display_name(&self) -> String {
        match self.pet_owner() {
            Some(o) => format!("{} ({o})", self.name()),
            None => self.name().to_string(),
        }
    }

    pub fn name_visible(&self) -> bool {
        !matches!(&self.appearance, Appearance::Monster { image: 41, .. } if self.frame.start == 640)
    }

    pub fn is_player(&self) -> bool {
        matches!(self.appearance, Appearance::Player { .. })
    }

    pub fn is_monster(&self) -> bool {
        matches!(self.appearance, Appearance::Monster { .. })
    }

    pub fn is_npc(&self) -> bool {
        matches!(self.appearance, Appearance::Npc { .. })
    }

    pub fn is_item(&self) -> bool {
        matches!(self.appearance, Appearance::Item { .. })
    }

    pub fn name(&self) -> &str {
        match &self.appearance {
            Appearance::Player { name, .. } => name,
            Appearance::Monster { name, .. } => name,
            Appearance::Npc { name, .. } => name,
            Appearance::Item { .. } | Appearance::Spell { .. } => "",
        }
    }

    /// True while riding (Zircon `Horse != HorseType.None`).
    pub fn mounted(&self) -> bool {
        matches!(&self.appearance, Appearance::Player { horse, .. } if *horse > 0)
    }

    /// Signed Assassin body/helmet/shield offset from `PlayerObject.SetFrame`.
    /// Normal classes use the unshifted player frames, including mounted poses.
    pub fn armour_shift(&self) -> i32 {
        let assassin = matches!(&self.appearance, Appearance::Player { class, .. } if *class == mir_proto::Class::Assassin);
        mir_formats::wearable_animation::armour_shift(
            wearable_action(self.action),
            assassin,
            self.mounted(),
            fishing_frames(),
        )
    }

    /// Recompute the frame set for the current action (after the
    /// appearance changed, e.g. mounting).
    pub fn refresh_frame(&mut self) {
        self.frame = self.frame_for(self.action);
        self.frame_index = self.frame_index.min(self.frame.count.saturating_sub(1));
    }

    fn frame_for(&self, action: Action) -> Frame {
        match &self.appearance {
            // Zircon FrameSet: HorseStanding 2240x4 500 ms, HorseWalking
            // 2320x6, HorseRunning 2400x6, HorseStruck 2480x3 (100 ms).
            Appearance::Player { horse, .. } => {
                let spec = mir_formats::wearable_animation::body_frame(
                    wearable_action(action),
                    *horse > 0,
                    fishing_frames(),
                );
                Frame::new(spec.start, spec.count, spec.interval)
            }
            // Zircon `MonsterObject.UpdateFrame`: a dead body the server has
            // marked picked clean draws its bones instead of its corpse.
            Appearance::Monster {
                skeleton, image, ..
            } if *skeleton
                && action == Action::Dead
                && !mir_formats::monster_animation::is_gate(*image) =>
            {
                monster_skeleton_frame()
            }
            Appearance::Monster { image, .. } => monster_frame_for_image(*image, action),
            Appearance::Npc { image, .. } => npc_frame(*image),
            Appearance::Item { .. } => Frame::new(0, 1, 3_600_000),
            Appearance::Spell { effect } => spell_frame(*effect),
        }
    }

    pub fn enqueue(&mut self, q: Queued) {
        self.queue.push_back(q);
    }

    /// Snap to a server-reported state, dropping any pending animation.
    pub fn snap(&mut self, location: Point, direction: Direction, now: u64) {
        self.queue.clear();
        self.location = location;
        self.direction = direction;
        self.moving_offset = (0, 0);
        self.move_distance = 0;
        self.set_action(
            Queued {
                action: if self.dead {
                    Action::Dead
                } else {
                    Action::Standing
                },
                direction,
                location,
                distance: 0,
            },
            now,
        );
    }

    fn set_action(&mut self, q: Queued, now: u64) {
        self.action = q.action;
        self.direction = q.direction;
        self.location = q.location;
        self.move_distance = q.distance;
        self.frame = self.frame_for(q.action);
        self.frame_index = 0;
        self.frame_start = now;
    }

    fn next_action(&mut self, now: u64) {
        let q = match self.queue.pop_front() {
            Some(q) => q,
            None => Queued {
                action: if matches!(self.action, Action::Die | Action::Dead) || self.dead {
                    Action::Dead
                } else {
                    Action::Standing
                },
                direction: self.direction,
                location: self.location,
                distance: 0,
            },
        };
        self.set_action(q, now);
    }

    pub fn set_poison_kinds(&mut self, kinds: u16, now: u64) {
        let stunned = kinds & 8 != 0;
        if stunned && self.stunned_at.is_none() {
            self.stunned_at = Some(now);
        } else if !stunned {
            if let Some(start) = self.stunned_at.take() {
                if !self.dead {
                    self.frame_start += now.saturating_sub(start.max(self.frame_start));
                }
            }
        }
        self.poison_kinds = kinds;
    }

    pub fn poison_tint(&self) -> Option<[f32; 4]> {
        if self.poison_kinds & 8 != 0 {
            Some([0.6, 0.6, 0.6, 1.0])
        } else if self.poison_kinds & 1 != 0 {
            Some([0.4, 1.0, 0.4, 1.0])
        } else if self.poison_kinds & 2 != 0 {
            Some([1.0, 0.4, 0.4, 1.0])
        } else {
            None
        }
    }

    /// Advance animation; call every frame.
    pub fn process(&mut self, now: u64) {
        if self.stunned_at.is_some() && !self.dead {
            return;
        }
        let interruptible = matches!(self.action, Action::Standing | Action::Dead);
        let elapsed = now.saturating_sub(self.frame_start);
        let mut frame = (elapsed / self.frame.interval.max(1)) as u32;
        if frame >= self.frame.count || (interruptible && !self.queue.is_empty()) {
            self.next_action(now);
            let elapsed = now.saturating_sub(self.frame_start);
            frame = ((elapsed / self.frame.interval.max(1)) as u32).min(self.frame.count - 1);
        }
        self.frame_index = frame;

        // Smooth movement (Config.SmoothMove = true): remaining offset back toward origin.
        self.moving_offset = (0, 0);
        if matches!(
            self.action,
            Action::Walking | Action::Running | Action::Dash
        ) && self.move_distance > 0
        {
            let sum = self.frame.sum() as f32;
            let t = (now.saturating_sub(self.frame_start) as f32).min(sum);
            let r = (sum - t) / sum;
            let dx = (48.0 * self.move_distance as f32 * r) as i32;
            let dy = (32.0 * self.move_distance as f32 * r) as i32;
            let (x, y) = match self.direction {
                Direction::Up => (0, dy),
                Direction::UpRight => (-dx, dy),
                Direction::Right => (-dx, 0),
                Direction::DownRight => (-dx, -dy),
                Direction::Down => (0, -dy),
                Direction::DownLeft => (dx, -dy),
                Direction::Left => (dx, 0),
                Direction::UpLeft => (dx, dy),
            };
            // Upstream dropped the even-pixel snap with the render/sim decoupling.
            self.moving_offset = (x, y);
        }
        self.damage.retain(|(_, t)| now < t + 1500);
    }

    /// Row used for y-sorting (`MapObject.RenderY`).
    pub fn render_y(&self) -> i32 {
        if self.moving_offset != (0, 0)
            && matches!(
                self.direction,
                Direction::Up | Direction::UpRight | Direction::UpLeft
            )
        {
            self.location.y + self.move_distance
        } else {
            self.location.y
        }
    }

    /// Sprite index within the body library for the current frame.
    pub fn sprite_index(&self, shape: u16) -> u32 {
        let base = self.frame_index + self.frame.start + 10 * self.direction.index() as u32;
        match &self.appearance {
            Appearance::Player { class, .. } => {
                let stride = mir_formats::wearable_animation::shape_stride(
                    *class == mir_proto::Class::Assassin,
                );
                base + (shape as u32 % 11) * stride
            }
            Appearance::Monster { image, .. }
                if mir_formats::monster_animation::is_gate(*image) || *image == 137 =>
            {
                mir_formats::monster_animation::slot_for_image(
                    *image,
                    shape,
                    monster_action(self.action),
                    self.direction.index(),
                    self.frame_index,
                )
                .expect("valid special monster frame")
            }
            Appearance::Monster { image: 273, .. } => {
                (shape as u32 % 10) * 1000 + self.frame.start + self.frame_index
            }
            Appearance::Monster { image: 41, .. } if self.frame.start == 640 => {
                (shape as u32 % 10) * 1000 + 640 + 7 - self.frame_index
            }
            Appearance::Monster { .. } => base + (shape as u32 % 10) * 1000,
            // NPCs never face a direction: index = image * 100 + frame.
            Appearance::Npc { image, .. } => self.frame_index + *image as u32 * 100,
            Appearance::Item { .. } => 0,
            Appearance::Spell { .. } => self.frame.start + self.frame_index,
        }
    }

    /// Frame index within the current animation, without shape or direction.
    pub fn draw_frame(&self) -> u32 {
        self.frame_index + self.frame.start + 10 * self.direction.index() as u32
    }
}

#[cfg(test)]
mod monster_contract_tests {
    use super::*;

    #[test]
    fn paralysis_freezes_frames_is_grey_and_resumes_after_release() {
        let state = mir_proto::ObjectState {
            id: ObjectId(1),
            appearance: Appearance::Monster {
                name: "StunTarget".into(),
                image: 1,
                owner: None,
                skeleton: false,
            },
            location: Point::new(0, 0),
            direction: Direction::Down,
            hp: 10,
            max_hp: 10,
            dead: false,
            light: 0,
            room_door: false,
        };
        let mut object = ClientObject::new(&state, 0);
        object.process(100);
        let frame = object.frame_index;
        object.set_poison_kinds(9, 100);
        object.process(1500);
        assert_eq!(object.frame_index, frame);
        assert_eq!(object.poison_tint(), Some([0.6, 0.6, 0.6, 1.0]));
        object.set_poison_kinds(1, 1500);
        object.process(1500);
        assert_eq!(object.frame_index, frame);
        assert_eq!(object.poison_tint(), Some([0.4, 1.0, 0.4, 1.0]));
        object.process(2000);
        assert_ne!(object.frame_index, frame);
        object.set_poison_kinds(0, 2000);
        assert_eq!(object.poison_tint(), None);
        object.set_poison_kinds(8, 2100);
        object.snap(Point::new(1, 1), Direction::Up, 2500);
        object.set_poison_kinds(0, 3000);
        assert_eq!(object.frame_start, 3000);
        object.set_poison_kinds(8, 3100);
        object.dead = true;
        object.snap(Point::new(1, 1), Direction::Up, 3500);
        object.process(3900);
        object.set_poison_kinds(0, 4000);
        assert_eq!(object.frame_start, 3500);
    }

    #[test]
    fn sama_prophet_uses_visible_directionless_frames() {
        let state = mir_proto::ObjectState {
            id: ObjectId(273),
            appearance: Appearance::Monster {
                name: "Sama Prophet".into(),
                image: 273,
                owner: None,
                skeleton: false,
            },
            location: Point::new(0, 0),
            direction: Direction::Down,
            hp: 10,
            max_hp: 10,
            dead: false,
            light: 0,
            room_door: false,
        };
        let mut prophet = ClientObject::new(&state, 0);
        let library = std::env::var_os("ZIRCON_ASSETS").map(|root| {
            mir_formats::zl::ZlLibrary::open(std::path::PathBuf::from(root).join("Data/Mon-27.Zl"))
                .unwrap()
        });
        for (action, start, count, interval) in [
            (Action::Standing, 50, 4, 500),
            (Action::Walking, 50, 1, 600),
            (Action::Attack, 130, 9, 100),
            (Action::Attack2, 210, 9, 100),
            (Action::Cast1, 290, 10, 100),
            (Action::Struck, 370, 3, 100),
            (Action::Die, 450, 10, 100),
            (Action::Dead, 459, 1, 500),
        ] {
            prophet.set_action(
                Queued {
                    action,
                    direction: Direction::Down,
                    location: state.location,
                    distance: 0,
                },
                0,
            );
            assert_eq!(
                (
                    prophet.frame.start,
                    prophet.frame.count,
                    prophet.frame.interval
                ),
                (start, count, interval)
            );
            for frame in 0..count {
                prophet.frame_index = frame;
                let slot = 3000 + start + frame;
                for direction in [
                    Direction::Up,
                    Direction::UpRight,
                    Direction::Right,
                    Direction::DownRight,
                    Direction::Down,
                    Direction::DownLeft,
                    Direction::Left,
                    Direction::UpLeft,
                ] {
                    prophet.direction = direction;
                    assert_eq!(prophet.sprite_index(3), slot);
                }
                if let Some(library) = &library {
                    let image = library
                        .decode(slot as usize, mir_formats::zl::SurfaceKind::Image)
                        .unwrap()
                        .unwrap();
                    assert!(image
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| pixel[3] > 0));
                }
            }
        }
    }

    #[test]
    fn chestnut_tree_uses_its_directionless_standing_frames() {
        let state = mir_proto::ObjectState {
            id: ObjectId(137),
            appearance: Appearance::Monster {
                name: "Chestnut Tree".into(),
                image: 137,
                owner: None,
                skeleton: false,
            },
            location: Point::new(0, 0),
            direction: Direction::Down,
            hp: 10,
            max_hp: 10,
            dead: false,
            light: 0,
            room_door: false,
        };
        let mut tree = ClientObject::new(&state, 0);
        assert_eq!(tree.sprite_index(7), 7000);
        tree.process(1500);
        assert_eq!(tree.sprite_index(7), 7003);
    }
    #[test]
    fn buried_carnivorous_plant_has_no_name_until_it_rises() {
        let state = mir_proto::ObjectState {
            id: ObjectId(41),
            appearance: Appearance::Monster {
                name: "Carnivorous Plant".into(),
                image: 41,
                owner: None,
                skeleton: false,
            },
            location: Point::new(0, 0),
            direction: Direction::Up,
            hp: 10,
            max_hp: 10,
            dead: false,
            light: 0,
            room_door: false,
        };
        let mut plant = ClientObject::new(&state, 452);
        assert_eq!(plant.sprite_index(1), 1647);
        assert!(!plant.name_visible());
        plant.process(1251);
        assert_eq!(plant.sprite_index(1), 1640);
        assert!(!plant.name_visible());
        plant.process(1252);
        assert_eq!(plant.sprite_index(1), 1000);
        assert!(plant.name_visible());
    }
    #[test]
    fn gate_stays_visible_and_plays_open_close_frames() {
        let state = mir_proto::ObjectState {
            id: ObjectId(1),
            appearance: Appearance::Monster {
                name: "Gate".into(),
                image: 543,
                owner: None,
                skeleton: false,
            },
            location: Point::new(23, 108),
            direction: Direction::Up,
            hp: 100,
            max_hp: 100,
            dead: false,
            light: 0,
            room_door: false,
        };
        let mut gate = ClientObject::new(&state, 0);
        for now in [0, 500, 1000, 1500] {
            gate.process(now);
            assert_eq!(gate.sprite_index(3), 3000);
        }
        for (direction, first, last, standing) in [
            (Direction::UpLeft, 3640, 3646, 3070),
            (Direction::Up, 3646, 3640, 3000),
        ] {
            gate.set_action(
                Queued {
                    action: Action::Attack,
                    direction,
                    location: state.location,
                    distance: 0,
                },
                2000,
            );
            assert_eq!(gate.sprite_index(3), first);
            gate.process(2600);
            assert_eq!(gate.sprite_index(3), last);
            gate.process(2700);
            assert_eq!(gate.sprite_index(3), standing);
        }
        gate.set_action(
            Queued {
                action: Action::Die,
                direction: Direction::UpLeft,
                location: state.location,
                distance: 0,
            },
            3000,
        );
        gate.process(3700);
        assert_eq!(gate.sprite_index(3), 3327);
        gate.process(3800);
        assert_eq!(gate.sprite_index(3), 3327);
    }
    #[test]
    fn shared_monster_specs_preserve_all_runtime_action_aliases() {
        for (actions, expected) in [
            (
                vec![
                    Action::Standing,
                    Action::FishingCast,
                    Action::FishingWait,
                    Action::FishingReel,
                    Action::Mining,
                    Action::Harvest,
                ],
                (0, 4, 500),
            ),
            (
                vec![Action::Walking, Action::Running, Action::Dash],
                (80, 6, 100),
            ),
            (
                vec![
                    Action::Attack,
                    Action::Attack2,
                    Action::Attack5,
                    Action::Attack6,
                    Action::Cast1,
                    Action::Cast2,
                    Action::Stance,
                ],
                (160, 6, 100),
            ),
            (vec![Action::Struck], (240, 2, 100)),
            (vec![Action::Die], (320, 10, 100)),
            (vec![Action::Dead], (329, 1, 1000)),
        ] {
            for action in actions {
                let actual = monster_frame(action);
                assert_eq!((actual.start, actual.count, actual.interval), expected);
            }
        }
        let skeleton = monster_skeleton_frame();
        assert_eq!(
            (skeleton.start, skeleton.count, skeleton.interval),
            (880, 1, 1000)
        );
    }
}

#[cfg(test)]
mod field_art_tests {
    use super::*;
    #[test]
    fn firewall_loop_uses_only_available_frames() {
        let Some(root) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let mut assets = crate::assets::Assets::new(root);
        let count = (920..925)
            .take_while(|&i| assets.info(crate::effects::MAGIC, i).is_some())
            .count() as u32;
        assert!(count > 0);
        set_firewall_frames(count);
        let frame = spell_frame(mir_proto::spell_effect::FIRE_WALL);
        assert_eq!(frame.count, count);
        for index in frame.start..frame.start + frame.count {
            assert!(assets.info(crate::effects::MAGIC, index).is_some());
        }
    }

    #[test]
    fn persistent_field_art_matches_source_ranges_and_decodes_installed_frames() {
        let root = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from);
        for (effect, library, start, count, interval) in [
            (mir_proto::spell_effect::ICE_AURA, 247, 2600, 10, 150),
            (mir_proto::spell_effect::BURNING_FIRE, 248, 1000, 8, 100),
            (mir_proto::spell_effect::DARK_SOUL_PRISON, 248, 700, 10, 100),
            (mir_proto::spell_effect::DEATH_CLOUD, 171, 850, 10, 100),
        ] {
            assert_eq!(spell_library(effect), Some(library));
            let frame = spell_frame(effect);
            assert_eq!(
                (frame.start, frame.count, frame.interval),
                (start, count, interval)
            );
            if let Some(root) = &root {
                let library = mir_formats::zl::ZlLibrary::open(
                    root.join(mir_formats::monster_table::library_path(library).unwrap()),
                )
                .unwrap();
                for index in start..start + count {
                    let image = library
                        .decode(index as usize, mir_formats::zl::SurfaceKind::Image)
                        .unwrap()
                        .unwrap();
                    assert!(
                        image
                            .rgba
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|pixel| pixel[3] > 0),
                        "Missing visible field frame {index}"
                    );
                }
            }
        }
    }
}
