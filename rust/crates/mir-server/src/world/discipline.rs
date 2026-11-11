//! The discipline track (Zircon `PlayerObject.Instances.cs`, `#region
//! Discipline`): a second, late-game progression ladder bought with
//! character level and gold, whose only reward in this data is a pool of
//! focus points.
//!
//! Two halves of Zircon's design are inert in this asset pack, and are
//! implemented but never fire:
//!
//! * **Discipline experience.** All four `DisciplineInfo` rows have
//!   `RequiredExperience = 0`, and no monster carries the `GrowthLevel`
//!   stat that `GainDisciplineExperience` is paid from, so nothing ever
//!   banks or spends it.
//! * **Discipline magics.** `IncreaseDiscipline` grants the first unlearnt
//!   magic whose school is `MagicSchool.Discipline` (20). This pack has no
//!   such magic, so no skill is ever handed out. The "Discipline" magic at
//!   index 89 is school 12, `Atrocity`, an ordinary warrior passive that
//!   `refresh_stats` already applies; it is a different thing sharing a name.

use mir_proto::{ChatKind, DisciplineNext, ObjectId, ServerMessage};

use super::World;

impl World {
    /// Zircon `IncreaseDiscipline`: buy the next rung, if the player has the
    /// level, the experience and the gold for it.
    pub fn increase_discipline(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let (current, level, gold, exp) =
            (p.discipline, p.level, p.bag.gold, p.discipline_experience);
        let Some(next) = self
            .data
            .disciplines
            .iter()
            .find(|d| d.level == current + 1)
            .cloned()
        else {
            self.say_discipline(id, "You have mastered every discipline.".into());
            return;
        };
        if level < next.required_level {
            let msg = format!(
                "You need to be level {} to train that.",
                next.required_level
            );
            self.say_discipline(id, msg);
            return;
        }
        if gold < next.required_gold {
            let msg = format!("You need {} gold to train that.", next.required_gold);
            self.say_discipline(id, msg);
            return;
        }
        if exp < next.required_experience {
            let msg = format!(
                "You need {} discipline experience to train that.",
                next.required_experience
            );
            self.say_discipline(id, msg);
            return;
        }
        let gold_left = {
            let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
                return;
            };
            p.bag.gold -= next.required_gold;
            p.discipline = next.level;
            p.bag.gold
        };
        self.send_to(id, ServerMessage::GoldChanged { gold: gold_left });
        let p = self.objects[&id].player().unwrap();
        let mut schools: Vec<_> = self
            .data
            .magics
            .values()
            .filter(|m| m.school == 20 && m.class & (1 << p.class.mir_class()) != 0)
            .collect();
        schools.sort_by_key(|m| (m.need_level[0], m.index));
        let magic = schools
            .into_iter()
            .take(4)
            .find(|m| {
                m.need_level[0] <= next.required_level
                    && !p.magics.iter().any(|known| known.magic == m.magic)
            })
            .map(|m| m.magic);
        if let Some(magic) = magic {
            let learned = super::UserMagic {
                magic,
                level: 0,
                experience: 0,
                key: 0,
                cooldown_until: 0,
            };
            let summary = learned.summary();
            self.objects
                .get_mut(&id)
                .unwrap()
                .player_mut()
                .unwrap()
                .magics
                .push(learned);
            self.send_to(id, ServerMessage::NewMagic(summary));
        }
        // Zircon refreshes stats here, which is what turns the new level
        // into a focus pool.
        self.refresh_stats(id, false);
        self.send_discipline(id);
        let msg = format!("You have reached discipline level {}.", next.level);
        self.say_discipline(id, msg);
    }

    /// Zircon `GainDisciplineExperience`: banked only once a discipline has
    /// been taken. Nothing in this pack calls it with a non-zero amount --
    /// see the module note.
    pub fn gain_discipline_experience(&mut self, id: ObjectId, amount: i32) {
        if amount <= 0 {
            return;
        }
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        if p.discipline == 0 {
            return;
        }
        p.discipline_experience += amount as i64;
        let stats = self.player_stats(&self.objects[&id]);
        self.send_to(id, ServerMessage::StatsChanged(stats));
    }

    /// The focus pool a discipline level grants (Zircon
    /// `Stats[Stat.Focus] = Character.Discipline?.Info.FocusPoints ?? 0`).
    pub fn focus_points(&self, level: i32) -> i32 {
        if level <= 0 {
            return 0;
        }
        self.data
            .disciplines
            .iter()
            .find(|d| d.level == level)
            .map(|d| d.focus_points)
            .unwrap_or(0)
    }

    /// What the character window shows about the next rung.
    pub fn next_discipline(&self, id: ObjectId) -> DisciplineNext {
        let current = self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.discipline)
            .unwrap_or(0);
        self.data
            .disciplines
            .iter()
            .find(|d| d.level == current + 1)
            .map(|d| DisciplineNext {
                level: d.level,
                required_level: d.required_level,
                required_experience: d.required_experience,
                required_gold: d.required_gold,
                focus_points: d.focus_points,
            })
            .unwrap_or_default()
    }

    pub(super) fn send_discipline(&mut self, id: ObjectId) {
        let next = self.next_discipline(id);
        self.send_player_stats(id);
        self.send_to(id, ServerMessage::DisciplineInfo(next));
    }

    /// `ZIRCON_DEV_DISCIPLINE=<n>` grants discipline level `n` outright on
    /// entry. The first rung needs character level 70 and a million gold, so
    /// without this there is no way to see the focus pool in a screenshot.
    pub(super) fn dev_discipline(&mut self, id: ObjectId) {
        let Some(n) = std::env::var("ZIRCON_DEV_DISCIPLINE")
            .ok()
            .and_then(|v| v.trim().parse::<i32>().ok())
            .filter(|n| *n > 0)
        else {
            return;
        };
        let top = self
            .data
            .disciplines
            .iter()
            .map(|d| d.level)
            .max()
            .unwrap_or(0);
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.discipline = n.min(top);
        }
        self.refresh_stats(id, true);
    }

    fn say_discipline(&mut self, id: ObjectId, text: String) {
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
