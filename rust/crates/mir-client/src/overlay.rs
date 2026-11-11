//! Screen overlays that sit outside the windows: the target panel (Zircon
//! `MonsterDialog`), the tracked-quest list (`QuestTrackerDialog`) and the
//! tooltip for the buff icons (`BuffDialog`).

use mir_proto::{buff_type, UserQuestSummary};

use crate::assets::lib;
use crate::items::{ItemCatalog, QuestDef};
use crate::ui::{Ctx, Rect};

/// Zircon `MonsterDialog` sits at (250, 50) and is 186 wide.
const TARGET_X: f32 = 250.0;
const TARGET_Y: f32 = 50.0;
const TARGET_W: f32 = 186.0;
/// The gold Zircon borders its little panels with.
const BORDER: [u8; 4] = [198, 166, 99, 255];
const PANEL_BG: [f32; 4] = [0.0, 0.0, 0.0, 0.6];

/// Where the buff icons live (mirrors `draw_hud`): six to a row, 27 px
/// apart, growing leftwards from the right edge.
pub fn buff_icon_rect(index: usize, width: i32) -> Rect {
    Rect::new(
        width as f32 - 30.0 - (index % 6) as f32 * 27.0,
        6.0 + (index / 6) as f32 * 27.0,
        24.0,
        24.0,
    )
}

/// What the target panel needs to know about the thing under the cursor.
pub struct TargetInfo {
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub poisoned: bool,
    /// Visible buffs (Zircon `VisibleBuffs`).
    pub buffs: Vec<u16>,
}

/// Zircon `MonsterDialog`: a level-less name box with a health bar under
/// it (the client never learns a monster's level, so that box is dropped).
pub fn draw_target_panel(c: &mut Ctx, t: &TargetInfo) {
    let name_box = Rect::new(TARGET_X, TARGET_Y, TARGET_W, 20.0);
    c.fill(name_box, PANEL_BG);
    c.border(name_box, BORDER);
    c.text.draw_centered(
        &t.name,
        12,
        name_box.x + name_box.w / 2.0,
        name_box.y + 2.0,
        [255, 255, 255, 255],
    );

    let bar = Rect::new(TARGET_X, TARGET_Y + 27.0, 124.0, 16.0);
    c.fill(bar, PANEL_BG);
    c.border(bar, BORDER);
    let pct = (t.hp.max(0) as f32 / t.max_hp.max(1) as f32).clamp(0.0, 1.0);
    if pct > 0.0 {
        if let Some(r) = c.sprite(lib::GAME_INTER, 5430) {
            c.renderer
                .draw_cropped(r, bar.x + 1.0, bar.y + 2.0, pct, [1.0, 1.0, 1.0, 1.0]);
        }
    }
    c.text.draw_centered(
        &format!("{}/{}", t.hp.max(0), t.max_hp.max(0)),
        11,
        bar.x + bar.w / 2.0,
        bar.y + 1.0,
        [255, 255, 255, 255],
    );

    // Poison and buffs on the target, as small icons beside the bar.
    let mut x = bar.x + bar.w + 5.0;
    if t.poisoned {
        let dot = Rect::new(x, bar.y + 3.0, 10.0, 10.0);
        c.fill(dot, [0.2, 0.8, 0.2, 0.9]);
        c.border(dot, BORDER);
        x += 14.0;
    }
    for kind in t.buffs.iter().take(3) {
        let icon = Rect::new(x, bar.y + 3.0, 10.0, 10.0);
        c.fill(icon, [0.4, 0.5, 0.9, 0.9]);
        c.border(icon, BORDER);
        let _ = kind;
        x += 14.0;
    }
}

/// Zircon `QuestTrackerDialog`: the tracked quests over the world, faint
/// until the mouse is on it. Ours also lists each task's progress.
pub fn draw_quest_tracker(c: &mut Ctx, lines: &[(String, [u8; 4])], width: i32, mouse: (f32, f32)) {
    if lines.is_empty() {
        return;
    }
    let w = 250.0;
    let x = width as f32 - w - 10.0;
    let y = 66.0;
    let h = lines.len() as f32 * 15.0 + 8.0;
    let panel = Rect::new(x, y, w, h);
    // A faint plate always, so the lines stay readable over bright ground;
    // Zircon fades its own in further on hover.
    let hovered = panel.contains(mouse.0, mouse.1);
    c.fill(panel, [0.0, 0.0, 0.0, if hovered { 0.5 } else { 0.35 }]);
    if hovered {
        c.border(panel, BORDER);
    }
    for (i, (line, color)) in lines.iter().enumerate() {
        c.text
            .draw(line, 11, x + 6.0, y + 4.0 + i as f32 * 15.0, *color);
    }
}

/// Name and remaining time of the buff icon under the cursor.
pub fn draw_buff_tooltip(
    c: &mut Ctx,
    buffs: &[(u16, u64)],
    width: i32,
    mouse: (f32, f32),
    now: u64,
) {
    let Some((i, (kind, until))) = buffs
        .iter()
        .enumerate()
        .find(|(i, _)| buff_icon_rect(*i, width).contains(mouse.0, mouse.1))
    else {
        return;
    };
    let label = if *until == u64::MAX {
        buff_name(*kind).to_string()
    } else {
        let secs = until.saturating_sub(now).div_ceil(1000);
        format!("{} ({}s)", buff_name(*kind), secs)
    };
    let tw = c.text.width(&label, 11) + 10.0;
    let r = buff_icon_rect(i, width);
    let box_rect = Rect::new((r.x + r.w - tw).max(4.0), r.y + r.h + 2.0, tw, 18.0);
    c.fill(box_rect, [0.0, 0.0, 0.0, 0.85]);
    c.border(box_rect, BORDER);
    c.text.draw(
        &label,
        11,
        box_rect.x + 5.0,
        box_rect.y + 1.0,
        [255, 255, 200, 255],
    );
}

/// The progress line of every task of a quest, shared by the quest log and
/// the tracker so both count the same way.
pub fn quest_task_lines(
    def: &QuestDef,
    q: &UserQuestSummary,
    catalog: &ItemCatalog,
) -> Vec<(String, [u8; 4])> {
    def.tasks
        .iter()
        .map(|t| {
            let have = q
                .tasks
                .iter()
                .find(|(ti, _)| *ti == t.index)
                .map(|(_, a)| *a)
                .unwrap_or(0);
            let what = if !t.description.is_empty() {
                t.description.clone()
            } else if t.item != 0 {
                catalog.name(t.item)
            } else {
                match t.task {
                    2 => "Visit the area".to_string(),
                    _ => "Kills".to_string(),
                }
            };
            let color = if have >= t.amount {
                [80, 255, 80, 255]
            } else {
                [255, 200, 120, 255]
            };
            (format!("  {what}: {have}/{}", t.amount), color)
        })
        .collect()
}

/// The lines the tracker shows: every tracked quest and its tasks.
pub fn tracker_lines(quests: &[UserQuestSummary], catalog: &ItemCatalog) -> Vec<(String, [u8; 4])> {
    let mut out = Vec::new();
    for q in quests.iter().filter(|q| q.track) {
        let Some(def) = catalog.quest(q.quest) else {
            continue;
        };
        if q.completed {
            out.push((format!("{} (done)", def.name), [120, 255, 120, 255]));
            continue;
        }
        out.push((def.name.clone(), [255, 255, 0, 255]));
        out.extend(quest_task_lines(def, q, catalog));
    }
    out
}

/// Zircon `DamageInfo`: the colour of a floating number by its size, and
/// blue when it heals instead of hurts.
pub fn damage_color(value: i32, alpha: u8) -> [u8; 4] {
    let [r, g, b] = if value > 0 {
        [80, 160, 255]
    } else if value <= -1000 {
        [255, 255, 255]
    } else if value <= -500 {
        [255, 165, 0]
    } else if value <= -100 {
        [80, 255, 80]
    } else {
        [255, 80, 80]
    };
    [r, g, b, alpha]
}

/// Buff names for the tooltip (Zircon `BuffType`).
pub fn buff_name(kind: u16) -> &'static str {
    match kind {
        buff_type::DEFIANCE => "Defiance",
        buff_type::MIGHT => "Might",
        buff_type::ENDURANCE => "Endurance",
        buff_type::REFLECT_DAMAGE => "Reflect Damage",
        buff_type::RENOUNCE => "Renounce",
        buff_type::STRENGTH_OF_FAITH => "Strength Of Faith",
        buff_type::INVISIBILITY => "Invisibility",
        buff_type::ELEMENTAL_SUPERIORITY => "Elemental Superiority",
        buff_type::BLOOD_LUST => "Blood Lust",
        buff_type::CELESTIAL_LIGHT => "Celestial Light",
        buff_type::TRANSPARENCY => "Transparency",
        buff_type::CLOAK => "Cloak",
        buff_type::GHOST_WALK => "Ghost Walk",
        buff_type::INVINCIBILITY => "Invincibility",
        buff_type::EVASION => "Evasion",
        buff_type::RAGING_WIND => "Raging Wind",
        buff_type::CONCENTRATION => "Concentration",
        buff_type::THE_NEW_BEGINNING => "The New Beginning",
        buff_type::JUDGEMENT_OF_HEAVEN => "Judgement Of Heaven",
        buff_type::SUPERIOR_MAGIC_SHIELD => "Superior Magic Shield",
        buff_type::DARK_CONVERSION => "Dark Conversion",
        buff_type::LIFE_STEAL => "Life Steal",
        buff_type::SPIRITUALISM => "Spiritualism",
        buff_type::DRAGON_REPULSE => "Dragon Repulse",
        buff_type::ELEMENTAL_SWORDS => "Elemental Swords",
        buff_type::MAGIC_SHIELD => "Magic Shield",
        buff_type::HEAL => "Heal",
        buff_type::MAGIC_RESISTANCE => "Magic Resistance",
        buff_type::RESILIENCE => "Resilience",
        buff_type::POISONOUS_CLOUD => "Poisonous Cloud",
        buff_type::FULL_BLOOM => "Full Bloom",
        buff_type::WHITE_LOTUS => "White Lotus",
        buff_type::RED_LOTUS => "Red Lotus",
        buff_type::FROST_BITE => "Frost Bite",
        buff_type::TORNADO => "Tornado",
        buff_type::SOUL_RESONANCE => "Soul Resonance",
        _ => "Buff",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_tiers_match_zircon() {
        // Heals are blue, then red, green, orange and white as the hit grows.
        assert_eq!(damage_color(50, 255), [80, 160, 255, 255]);
        assert_eq!(damage_color(-50, 255), [255, 80, 80, 255]);
        assert_eq!(damage_color(-100, 255), [80, 255, 80, 255]);
        assert_eq!(damage_color(-500, 255), [255, 165, 0, 255]);
        assert_eq!(damage_color(-1000, 200), [255, 255, 255, 200]);
    }

    #[test]
    fn buff_icons_wrap_every_six() {
        let first = buff_icon_rect(0, 1024);
        assert_eq!((first.x, first.y), (994.0, 6.0));
        // Sixth icon is on the same row, the seventh starts the next one.
        assert_eq!(buff_icon_rect(5, 1024).y, first.y);
        assert_eq!(buff_icon_rect(6, 1024).x, first.x);
        assert_eq!(buff_icon_rect(6, 1024).y, first.y + 27.0);
    }
}
