//! Cosmetic armour picker. The server checks ownership and stores the choice.

use mir_proto::{item_type, slot, ClientMessage, Gender, ItemInstance};

use crate::assets::lib;
use crate::gfx::Blend;
use crate::items::ItemCatalog;
use crate::ui::{Ctx, Rect, GOLD};
use crate::windows::WindowLayout;

pub struct WardrobeWindow {
    pub open: bool,
    /// Armour choice also appears in the character panel.
    pub chosen: Option<i32>,
    choices: [Option<i32>; 4],
    category: usize,
    preview: Option<i32>,
    scroll: usize,
    layout: WindowLayout,
}

impl Default for WardrobeWindow {
    fn default() -> Self {
        Self {
            open: std::env::var("ZIRCON_OPEN")
                .unwrap_or_default()
                .split(',')
                .any(|name| name.trim() == "wardrobe"),
            chosen: None,
            choices: [None; 4],
            category: 0,
            preview: None,
            scroll: 0,
            layout: WindowLayout::default(),
        }
    }
}

impl WardrobeWindow {
    pub fn contains_mouse(&self, mouse: (f32, f32), width: i32, height: i32) -> bool {
        self.open
            && self.layout.contains_mouse(
                "wardrobe",
                Rect::new((width as f32 - 474.0) / 2.0, 50.0, 474.0, 420.0),
                mouse,
                (width, height),
            )
    }

    pub fn set_chosen_slot(&mut self, slot: usize, item: Option<i32>) {
        let Some(category) = [slot::ARMOUR, slot::HELMET, slot::WEAPON, slot::SHOES]
            .iter()
            .position(|&candidate| candidate == slot)
        else {
            return;
        };
        self.choices[category] = item;
        if category == 0 {
            self.chosen = item;
        }
        if self.category == category {
            self.preview = item;
        }
    }

    pub fn chosen_slot(&self, slot: usize) -> Option<i32> {
        [slot::ARMOUR, slot::HELMET, slot::WEAPON, slot::SHOES]
            .iter()
            .position(|&candidate| candidate == slot)
            .and_then(|index| self.choices[index])
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        c: &mut Ctx,
        catalog: &ItemCatalog,
        inventory: &[Option<ItemInstance>],
        equipment: &[Option<ItemInstance>],
        storage: &[Option<ItemInstance>],
        gender: Gender,
        width: i32,
        height: i32,
        out: &mut Vec<ClientMessage>,
    ) -> bool {
        if !self.open {
            return false;
        }
        let win = self.layout.place(
            "wardrobe",
            Rect::new((width as f32 - 474.0) / 2.0, 50.0, 474.0, 420.0),
            c.input,
            (width, height),
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1) || self.layout.is_dragging();
        let closed = c.window(win, "Wardrobe", false) && !self.layout.is_dragging();
        const CATEGORIES: [(usize, u8, &str); 4] = [
            (slot::ARMOUR, item_type::ARMOUR, "Armour"),
            (slot::HELMET, item_type::HELMET, "Helmet"),
            (slot::WEAPON, item_type::WEAPON, "Weapon"),
            (slot::SHOES, item_type::SHOES, "Boots"),
        ];
        for (index, (_, _, label)) in CATEGORIES.iter().enumerate() {
            let tab = Rect::new(
                win.x + 17.0 + index as f32 * 110.0,
                win.y + 43.0,
                104.0,
                28.0,
            );
            c.panel(
                tab,
                if index == self.category {
                    [70, 51, 21, 235]
                } else {
                    [30, 25, 20, 220]
                },
            );
            c.border(tab, GOLD);
            c.text
                .draw_centered(label, 12, tab.x + tab.w / 2.0, tab.y + 6.0, GOLD);
            if tab.contains(c.input.mouse.0, c.input.mouse.1) && c.input.lmb_pressed {
                self.category = index;
                self.preview = self.choices[index];
                self.scroll = 0;
            }
        }
        let (selected_slot, kind, _) = CATEGORIES[self.category];
        let mut owned: Vec<i32> = inventory
            .iter()
            .chain(equipment)
            .chain(storage)
            .flatten()
            .filter_map(|item| {
                catalog
                    .get(item.info)
                    .filter(|def| def.item_type == kind)
                    .map(|_| item.info)
            })
            .collect();
        owned.sort_unstable();
        owned.dedup();
        owned.sort_by(|a, b| {
            catalog
                .get(*a)
                .unwrap()
                .name
                .cmp(&catalog.get(*b).unwrap().name)
        });
        let list = Rect::new(win.x + 17.0, win.y + 77.0, 225.0, 264.0);
        c.panel(list, [0, 0, 0, 175]);
        let rows = 9;
        self.scroll = self.scroll.min(owned.len().saturating_sub(rows));
        if list.contains(c.input.mouse.0, c.input.mouse.1) {
            self.scroll = (self.scroll as i32 - c.input.wheel.round() as i32)
                .clamp(0, owned.len().saturating_sub(rows) as i32)
                as usize;
        }
        for (row, info) in owned.iter().skip(self.scroll).take(rows).enumerate() {
            let hit = Rect::new(list.x + 3.0, list.y + row as f32 * 29.0 + 3.0, 219.0, 27.0);
            if self.preview == Some(*info) {
                c.fill(hit, [0.30, 0.23, 0.12, 0.8]);
            }
            let name = &catalog.get(*info).unwrap().name;
            c.text.draw(
                &name.chars().take(28).collect::<String>(),
                12,
                hit.x + 5.0,
                hit.y + 6.0,
                if self.choices[self.category] == Some(*info) {
                    GOLD
                } else {
                    [225, 215, 194, 255]
                },
            );
            if hit.contains(c.input.mouse.0, c.input.mouse.1) && c.input.lmb_pressed {
                self.preview = Some(*info);
            }
        }
        if owned.is_empty() {
            c.text.draw(
                "No owned items in this slot.",
                12,
                list.x + 6.0,
                list.y + 8.0,
                GOLD,
            );
        }
        let anchor = (win.x + 346.0, win.y + 275.0);
        c.draw_offset(
            lib::PROG_USE,
            u32::from(gender == Gender::Female),
            anchor.0,
            anchor.1,
            [1.0; 4],
            Blend::Alpha,
        );
        let preview = self.preview.or_else(|| {
            equipment
                .get(selected_slot)
                .and_then(|item| item.as_ref())
                .map(|item| item.info)
        });
        if let Some(def) = preview.and_then(|info| catalog.get(info)) {
            if c.assets.info(lib::EQUIP, def.image.max(0) as u32).is_some() {
                if kind == item_type::WEAPON {
                    let effect =
                        mir_proto::equipment_visual::weapon_effect(def.index, def.exterior_effect);
                    if let Some(colour) = crate::equipment_fx::weapon_glow_colour(effect) {
                        for (x, y) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
                            c.draw_offset(
                                lib::EQUIP,
                                def.image.max(0) as u32,
                                anchor.0 + x,
                                anchor.1 + y,
                                colour,
                                Blend::Screen,
                            );
                        }
                    }
                }
                c.draw_offset(
                    lib::EQUIP,
                    def.image.max(0) as u32,
                    anchor.0,
                    anchor.1,
                    [1.0; 4],
                    Blend::Alpha,
                );
            }
            if kind == item_type::ARMOUR {
                for frame in crate::equipment_fx::armour_preview_frames(
                    def.index,
                    def.exterior_effect,
                    def.image,
                    gender,
                    (c.now / 100) as u32,
                )
                .into_iter()
                .flatten()
                {
                    c.draw_offset(
                        lib::EQUIP_EFFECT_UI,
                        frame,
                        anchor.0,
                        anchor.1,
                        [1.0; 4],
                        Blend::Screen,
                    );
                }
            }
            c.text
                .draw(&def.name, 12, win.x + 254.0, win.y + 304.0, GOLD);
        }
        c.text.draw(
            if kind == item_type::SHOES {
                "Boots have no separate world sprite. Stats stay equipped."
            } else {
                "Only appearance changes; stats stay equipped."
            },
            11,
            win.x + 18.0,
            win.y + 345.0,
            [180, 180, 180, 255],
        );
        let apply = Rect::new(win.x + 273.0, win.y + 358.0, 82.0, 29.0);
        let reset = Rect::new(win.x + 365.0, win.y + 358.0, 82.0, 29.0);
        for (button, label) in [(apply, "Apply"), (reset, "Reset")] {
            c.panel(button, [35, 30, 20, 240]);
            c.border(button, GOLD);
            c.text
                .draw_centered(label, 12, button.x + button.w / 2.0, button.y + 6.0, GOLD);
        }
        if apply.contains(c.input.mouse.0, c.input.mouse.1)
            && c.input.lmb_pressed
            && self.preview.is_some_and(|info| owned.contains(&info))
        {
            out.push(ClientMessage::Wardrobe {
                slot: selected_slot as u8,
                item: self.preview,
            });
        }
        if reset.contains(c.input.mouse.0, c.input.mouse.1) && c.input.lmb_pressed {
            self.preview = None;
            out.push(ClientMessage::Wardrobe {
                slot: selected_slot as u8,
                item: None,
            });
        }
        self.layout.end_frame(c.input);
        if closed {
            self.open = false;
        }
        over
    }
}
