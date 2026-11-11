//! Character changes and discipline training, backed by server-validated requests.
use crate::ui::{Button, Ctx, Rect, TextBox, GOLD};
use crate::windows::WindowLayout;
use mir_proto::{Appearance, Class, ClientMessage, DisciplineNext, Gender, PlayerStats};

pub struct CharacterServices {
    pub open: bool,
    hair: TextBox,
    colour: TextBox,
    name: TextBox,
    buttons: Vec<Button>,
    layout: WindowLayout,
}
impl Default for CharacterServices {
    fn default() -> Self {
        let mut hair = TextBox::new(Rect::new(0.0, 0.0, 72.0, 22.0), 2);
        hair.text = "1".into();
        let mut colour = TextBox::new(Rect::new(0.0, 0.0, 110.0, 22.0), 6);
        colour.text = "FFFFFF".into();
        Self {
            open: std::env::var_os("ZIRCON_OPEN_CHARACTER_SERVICES").is_some(),
            hair,
            colour,
            name: TextBox::new(Rect::new(0.0, 0.0, 150.0, 22.0), 15),
            buttons: [
                "Change hair",
                "Change gender",
                "Dye armour",
                "Rename",
                "Train discipline",
            ]
            .iter()
            .map(|s| Button::default_style(0.0, 0.0, 150.0, s))
            .collect(),
            layout: WindowLayout::default(),
        }
    }
}
impl CharacterServices {
    pub fn typing(&self) -> bool {
        self.open && (self.hair.focused || self.colour.focused || self.name.focused)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        c: &mut Ctx,
        appearance: Option<&Appearance>,
        stats: &PlayerStats,
        gold: u64,
        next: &DisciplineNext,
        width: i32,
        height: i32,
        out: &mut Vec<ClientMessage>,
    ) -> bool {
        if !self.open {
            return false;
        }
        let win = self.layout.place(
            "character_services",
            Rect::new(
                (width as f32 - 470.0) / 2.0,
                ((height as f32 - 380.0) / 2.0).max(0.0),
                470.0,
                380.0,
            ),
            c.input,
            (width, height),
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1) || self.layout.is_dragging();
        let closed = c.window(win, "Character services", true) && !self.layout.is_dragging();
        let (class, gender) = match appearance {
            Some(Appearance::Player { class, gender, .. }) => (*class, *gender),
            _ => (Class::Warrior, Gender::Male),
        };
        let max = if class == Class::Assassin {
            5
        } else if gender == Gender::Male {
            10
        } else {
            11
        };
        c.text.draw(
            &format!("Hair style (0-{max}; 0 = bald)"),
            12,
            win.x + 20.0,
            win.y + 38.0,
            GOLD,
        );
        self.hair.rect.x = win.x + 220.0;
        self.hair.rect.y = win.y + 34.0;
        self.hair.update(c);
        c.text.draw(
            "Colour (RGB hex, e.g. FF8040)",
            12,
            win.x + 20.0,
            win.y + 66.0,
            GOLD,
        );
        self.colour.rect.x = win.x + 270.0;
        self.colour.rect.y = win.y + 62.0;
        self.colour.update(c);
        let style = self.hair.text.parse::<u8>().ok();
        let colour = (self.colour.text.len() == 6)
            .then(|| u32::from_str_radix(&self.colour.text, 16).ok())
            .flatten();
        self.hair.valid = Some(style.is_some_and(|h| h <= max));
        self.colour.valid = Some(colour.is_some());
        for i in 0..3 {
            let b = &mut self.buttons[i];
            b.pos = (win.x + 15.0 + i as f32 * 150.0, win.y + 96.0);
            let limit = if i == 1 && class != Class::Assassin {
                if gender == Gender::Male {
                    11
                } else {
                    10
                }
            } else {
                max
            };
            b.enabled = colour.is_some() && (i == 2 || style.is_some_and(|h| h <= limit));
            if b.update(c) {
                let hair = style.unwrap_or(0);
                let rgb = colour.unwrap_or(0) | 0xFF00_0000;
                let hair_colour = if hair == 0 { 0 } else { rgb };
                out.push(match i {
                    0 => ClientMessage::HairChange {
                        hair,
                        colour: hair_colour,
                    },
                    1 => ClientMessage::GenderChange {
                        gender: if gender == Gender::Male {
                            Gender::Female
                        } else {
                            Gender::Male
                        },
                        hair,
                        colour: hair_colour,
                    },
                    _ => ClientMessage::ArmourDye { colour: rgb },
                });
            }
        }
        c.text.draw(
            "Each change uses its matching item from your bag.",
            11,
            win.x + 20.0,
            win.y + 130.0,
            [200, 200, 200, 255],
        );
        c.text.draw(
            "Remove armour before changing gender.",
            11,
            win.x + 20.0,
            win.y + 147.0,
            [200, 200, 200, 255],
        );
        c.text
            .draw("New name", 12, win.x + 20.0, win.y + 178.0, GOLD);
        self.name.rect.x = win.x + 100.0;
        self.name.rect.y = win.y + 174.0;
        self.name.update(c);
        let b = &mut self.buttons[3];
        b.pos = (win.x + 285.0, win.y + 170.0);
        b.enabled = (3..=15).contains(&self.name.text.len())
            && self.name.text.bytes().all(|c| c.is_ascii_alphanumeric());
        if b.update(c) {
            out.push(ClientMessage::NameChange {
                name: self.name.text.clone(),
            });
        }
        c.text.draw(
            &format!(
                "Discipline {}    Focus {}/{}",
                stats.discipline, stats.fp, stats.max_fp
            ),
            13,
            win.x + 20.0,
            win.y + 220.0,
            GOLD,
        );
        if next.level > 0 {
            c.text.draw(
                &format!(
                    "Next: {} focus; requires level {}",
                    next.focus_points, next.required_level
                ),
                12,
                win.x + 20.0,
                win.y + 246.0,
                [255; 4],
            );
            c.text.draw(
                &format!(
                    "Gold: {}/{}; discipline XP: {}/{}",
                    gold, next.required_gold, stats.discipline_experience, next.required_experience
                ),
                11,
                win.x + 20.0,
                win.y + 265.0,
                [220, 220, 220, 255],
            );
            let b = &mut self.buttons[4];
            b.pos = (win.x + 160.0, win.y + 303.0);
            b.enabled = stats.level >= next.required_level
                && stats.discipline_experience >= next.required_experience
                && gold >= next.required_gold;
            if b.update(c) {
                out.push(ClientMessage::IncreaseDiscipline);
            }
        } else {
            c.text.draw(
                "No further discipline training is available.",
                12,
                win.x + 20.0,
                win.y + 250.0,
                [255; 4],
            );
        }
        if closed {
            self.open = false;
        }
        self.layout.end_frame(c.input);
        over
    }
}
