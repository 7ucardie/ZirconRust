//! Read-only default dungeon atlas, bundled so it works without a browser.

use std::io::Cursor;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::gfx::{Blend, SpriteKey, Surface};
use crate::ui::{Ctx, Rect, TextBox, GOLD};
use crate::windows::WindowLayout;

#[path = "dungeon_codex_maps.rs"]
mod dungeon_codex_maps;
use dungeon_codex_maps::MAP_IMAGES;

#[derive(Deserialize)]
struct Dungeon {
    map_id: u32,
    map_name: String,
    map_file: String,
    width: u32,
    height: u32,
    monsters: Vec<Monster>,
}

#[derive(Deserialize)]
struct Monster {
    id: i32,
    name: String,
    boss: bool,
    count: i32,
    share: f32,
    drops: Vec<Drop>,
}

#[derive(Deserialize)]
struct Drop {
    item_id: i32,
    item: String,
    one_in: i32,
    amount: i32,
}

fn dungeons() -> &'static [Dungeon] {
    static DUNGEONS: OnceLock<Vec<Dungeon>> = OnceLock::new();
    DUNGEONS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../docs/atlas/details.json"))
            .expect("default dungeon atlas must be valid")
    })
}

fn map_image(id: u32) -> Option<&'static [u8]> {
    MAP_IMAGES
        .binary_search_by_key(&id, |(map_id, _)| *map_id)
        .ok()
        .map(|index| MAP_IMAGES[index].1)
}

fn decode_map(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().ok()?;
    let mut rgb = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut rgb).ok()?;
    if frame.color_type != png::ColorType::Rgb {
        return None;
    }
    let mut rgba = Vec::with_capacity(frame.width as usize * frame.height as usize * 4);
    for pixel in rgb[..frame.buffer_size()].as_chunks::<3>().0 {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
    Some((frame.width, frame.height, rgba))
}

fn short(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!(
            "{}…",
            text.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

fn matches(map: &Dungeon, query: &str) -> bool {
    query.is_empty()
        || map.map_name.to_ascii_lowercase().contains(query)
        || map.map_file.to_ascii_lowercase().contains(query)
        || map.map_id.to_string() == query
        || map
            .monsters
            .iter()
            .any(|monster| monster.name.to_ascii_lowercase().contains(query))
}

pub struct DungeonCodex {
    pub open: bool,
    search: TextBox,
    selected_map: u32,
    selected_monster: Option<i32>,
    map_scroll: usize,
    monster_scroll: usize,
    drop_scroll: usize,
    layout: WindowLayout,
}

impl Default for DungeonCodex {
    fn default() -> Self {
        Self {
            open: std::env::var("ZIRCON_OPEN")
                .unwrap_or_default()
                .split(',')
                .any(|name| name.trim() == "codex"),
            search: TextBox::new(Rect::new(0.0, 0.0, 200.0, 22.0), 40),
            selected_map: 0,
            selected_monster: None,
            map_scroll: 0,
            monster_scroll: 0,
            drop_scroll: 0,
            layout: WindowLayout::default(),
        }
    }
}

impl DungeonCodex {
    pub fn select_floor(&mut self, file: &str) {
        let Some(map) = dungeons()
            .iter()
            .find(|map| map.map_file.eq_ignore_ascii_case(file))
        else {
            return;
        };
        self.search.text.clear();
        self.selected_map = map.map_id;
        self.selected_monster = None;
        let mut maps: Vec<_> = dungeons().iter().collect();
        maps.sort_by(|a, b| a.map_file.cmp(&b.map_file));
        self.map_scroll = maps
            .iter()
            .position(|entry| entry.map_id == map.map_id)
            .unwrap_or(0);
        self.monster_scroll = 0;
        self.drop_scroll = 0;
    }

    pub fn typing(&self) -> bool {
        self.open && self.search.focused
    }

    fn rect(width: i32, height: i32) -> Rect {
        let win_w = (width as f32 - 28.0).min(980.0);
        let win_h = (height as f32 - 40.0).min(700.0);
        Rect::new(
            (width as f32 - win_w) / 2.0,
            (height as f32 - win_h) / 2.0,
            win_w,
            win_h,
        )
    }

    pub fn contains_mouse(&self, mouse: (f32, f32), width: i32, height: i32) -> bool {
        self.open
            && self.layout.contains_mouse(
                "dungeon_codex",
                Self::rect(width, height),
                mouse,
                (width, height),
            )
    }

    pub fn draw(&mut self, c: &mut Ctx, width: i32, height: i32) -> bool {
        if !self.open {
            return false;
        }
        let win = self.layout.place(
            "dungeon_codex",
            Self::rect(width, height),
            c.input,
            (width, height),
        );
        let over = win.contains(c.input.mouse.0, c.input.mouse.1) || self.layout.is_dragging();
        let closed = c.window(win, "Dungeon Codex", false) && !self.layout.is_dragging();
        let left_w = (win.w * 0.25).min(240.0);
        let right_w = (win.w * 0.31).min(300.0);
        let left = Rect::new(win.x + 14.0, win.y + 69.0, left_w, win.h - 85.0);
        let right = Rect::new(
            win.x + win.w - right_w - 14.0,
            win.y + 42.0,
            right_w,
            win.h - 58.0,
        );
        let centre = Rect::new(
            left.x + left.w + 10.0,
            win.y + 42.0,
            right.x - left.x - left.w - 20.0,
            win.h - 58.0,
        );

        c.text.draw("Find", 12, left.x, win.y + 38.0, GOLD);
        self.search.rect = Rect::new(left.x + 34.0, win.y + 36.0, left.w - 34.0, 24.0);
        self.search.update(c);
        let query = self.search.text.trim().to_ascii_lowercase();
        let mut filtered: Vec<&Dungeon> = dungeons()
            .iter()
            .filter(|map| matches(map, &query))
            .collect();
        filtered.sort_by(|a, b| a.map_file.cmp(&b.map_file));
        if !filtered.iter().any(|map| map.map_id == self.selected_map) {
            self.selected_map = filtered.first().map_or(0, |map| map.map_id);
            self.selected_monster = None;
            self.map_scroll = 0;
        }

        c.panel(left, [0, 0, 0, 160]);
        let map_rows = (left.h / 31.0).floor().max(1.0) as usize;
        self.map_scroll = self.map_scroll.min(filtered.len().saturating_sub(map_rows));
        if left.contains(c.input.mouse.0, c.input.mouse.1) {
            self.map_scroll = scroll(self.map_scroll, filtered.len(), map_rows, c.input.wheel);
        }
        for (row, map) in filtered
            .iter()
            .skip(self.map_scroll)
            .take(map_rows)
            .enumerate()
        {
            let y = left.y + row as f32 * 31.0;
            let hit = Rect::new(left.x + 2.0, y, left.w - 4.0, 30.0);
            if map.map_id == self.selected_map {
                c.fill(hit, [0.35, 0.27, 0.14, 0.75]);
            }
            let color = if map.map_id == self.selected_map {
                [246, 211, 141, 255]
            } else {
                [224, 218, 197, 255]
            };
            c.text.draw(
                &short(
                    &format!("{}  {}", map.map_file, map.map_name),
                    ((left.w - 12.0) / 7.1) as usize,
                ),
                12,
                hit.x + 5.0,
                hit.y + 7.0,
                color,
            );
            if hit.contains(c.input.mouse.0, c.input.mouse.1) && c.input.lmb_pressed {
                self.selected_map = map.map_id;
                self.selected_monster = None;
                self.monster_scroll = 0;
                self.drop_scroll = 0;
            }
        }
        if filtered.is_empty() {
            c.text
                .draw("No matching dungeon.", 12, left.x + 7.0, left.y + 9.0, GOLD);
        }

        if let Some(map) = filtered.iter().find(|map| map.map_id == self.selected_map) {
            self.draw_selected(c, map, centre, right);
        }
        self.layout.end_frame(c.input);
        if closed {
            self.open = false;
            self.search.focused = false;
        }
        over
    }

    fn draw_selected(&mut self, c: &mut Ctx, map: &Dungeon, centre: Rect, right: Rect) {
        c.text.draw(
            &short(
                &format!("{} · {}", map.map_file, map.map_name),
                ((centre.w - 5.0) / 7.5) as usize,
            ),
            14,
            centre.x,
            centre.y,
            GOLD,
        );
        let map_panel = Rect::new(
            centre.x,
            centre.y + 25.0,
            centre.w,
            (centre.h * 0.48).min(300.0),
        );
        c.panel(map_panel, [15, 20, 18, 255]);
        if let Some(bytes) = map_image(map.map_id) {
            let key = SpriteKey {
                library: u16::MAX,
                index: map.map_id,
                surface: Surface::Image,
            };
            if let Some(image) = c.renderer.sprite(c.gpu, key, || decode_map(bytes)) {
                let scale = ((map_panel.w - 8.0) / image.width as f32)
                    .min((map_panel.h - 8.0) / image.height as f32);
                let w = image.width as f32 * scale;
                let h = image.height as f32 * scale;
                c.renderer.draw_scaled(
                    image,
                    map_panel.x + (map_panel.w - w) / 2.0,
                    map_panel.y + (map_panel.h - h) / 2.0,
                    w,
                    h,
                    [1.0; 4],
                    Blend::Alpha,
                );
            }
        }
        c.text.draw(
            &format!(
                "{} × {}  ·  {} monsters",
                map.width,
                map.height,
                map.monsters.len()
            ),
            12,
            centre.x,
            map_panel.y + map_panel.h + 6.0,
            [180, 174, 156, 255],
        );
        let monster_list = Rect::new(
            centre.x,
            map_panel.y + map_panel.h + 33.0,
            centre.w,
            (centre.y + centre.h - map_panel.y - map_panel.h - 33.0).max(0.0),
        );
        c.panel(monster_list, [0, 0, 0, 160]);
        if !map
            .monsters
            .iter()
            .any(|m| Some(m.id) == self.selected_monster)
        {
            self.selected_monster = map.monsters.first().map(|m| m.id);
            self.drop_scroll = 0;
        }
        let monster_rows = (monster_list.h / 30.0).floor().max(1.0) as usize;
        self.monster_scroll = self
            .monster_scroll
            .min(map.monsters.len().saturating_sub(monster_rows));
        if monster_list.contains(c.input.mouse.0, c.input.mouse.1) {
            self.monster_scroll = scroll(
                self.monster_scroll,
                map.monsters.len(),
                monster_rows,
                c.input.wheel,
            );
        }
        for (row, monster) in map
            .monsters
            .iter()
            .skip(self.monster_scroll)
            .take(monster_rows)
            .enumerate()
        {
            let hit = Rect::new(
                monster_list.x + 2.0,
                monster_list.y + row as f32 * 30.0,
                monster_list.w - 4.0,
                29.0,
            );
            if Some(monster.id) == self.selected_monster {
                c.fill(hit, [0.35, 0.27, 0.14, 0.75]);
            }
            let color = if monster.boss {
                [230, 139, 114, 255]
            } else {
                [224, 218, 197, 255]
            };
            c.text.draw(
                &short(
                    &format!("{} ({})", monster.name, monster.id),
                    ((hit.w - 70.0) / 7.0) as usize,
                ),
                12,
                hit.x + 5.0,
                hit.y + 6.0,
                color,
            );
            c.text.draw(
                &format!("{:.1}%", monster.share),
                11,
                hit.x + hit.w - 49.0,
                hit.y + 7.0,
                GOLD,
            );
            if hit.contains(c.input.mouse.0, c.input.mouse.1) && c.input.lmb_pressed {
                self.selected_monster = Some(monster.id);
                self.drop_scroll = 0;
            }
        }
        if map.monsters.is_empty() {
            c.text.draw(
                "No configured monsters.",
                12,
                monster_list.x + 6.0,
                monster_list.y + 7.0,
                GOLD,
            );
        }

        let monster = map
            .monsters
            .iter()
            .find(|m| Some(m.id) == self.selected_monster);
        c.text.draw(
            &short(
                &monster.map_or_else(|| "Drops".to_owned(), |m| format!("{} drops", m.name)),
                ((right.w - 5.0) / 7.5) as usize,
            ),
            14,
            right.x,
            right.y,
            GOLD,
        );
        if let Some(monster) = monster {
            c.text.draw(
                &format!("{} spawns · {:.1}% of map", monster.count, monster.share),
                11,
                right.x,
                right.y + 22.0,
                [180, 174, 156, 255],
            );
        }
        let drops = Rect::new(right.x, right.y + 47.0, right.w, right.h - 47.0);
        c.panel(drops, [0, 0, 0, 160]);
        let rows = (drops.h / 29.0).floor().max(1.0) as usize;
        let list = monster.map(|m| m.drops.as_slice()).unwrap_or(&[]);
        self.drop_scroll = self.drop_scroll.min(list.len().saturating_sub(rows));
        if drops.contains(c.input.mouse.0, c.input.mouse.1) {
            self.drop_scroll = scroll(self.drop_scroll, list.len(), rows, c.input.wheel);
        }
        for (row, drop) in list.iter().skip(self.drop_scroll).take(rows).enumerate() {
            let y = drops.y + row as f32 * 29.0;
            if row.is_multiple_of(2) {
                c.fill(
                    Rect::new(drops.x + 2.0, y, drops.w - 4.0, 28.0),
                    [0.14, 0.13, 0.1, 0.55],
                );
            }
            c.text.draw(
                &short(
                    &format!("{} ({})", drop.item, drop.item_id),
                    ((drops.w - 15.0) / 7.0) as usize,
                ),
                12,
                drops.x + 6.0,
                y + 3.0,
                [224, 218, 197, 255],
            );
            c.text.draw(
                &format!(
                    "1 in {}{}",
                    drop.one_in,
                    if drop.amount > 1 {
                        format!(" · ×{}", drop.amount)
                    } else {
                        String::new()
                    }
                ),
                11,
                drops.x + 6.0,
                y + 16.0,
                GOLD,
            );
        }
        if list.is_empty() {
            c.text
                .draw("No default drops.", 12, drops.x + 7.0, drops.y + 9.0, GOLD);
        }
    }
}

fn scroll(current: usize, total: usize, visible: usize, wheel: f32) -> usize {
    let max = total.saturating_sub(visible);
    if wheel > 0.0 {
        (current + 1).min(max)
    } else if wheel < 0.0 {
        current.saturating_sub(1)
    } else {
        current.min(max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_current_floor_clears_previous_search_and_selection() {
        let map = dungeons()
            .iter()
            .find(|map| map.map_file == "D001")
            .unwrap();
        let mut codex = DungeonCodex::default();
        codex.search.text = "unrelated search".into();
        codex.selected_monster = Some(-1);
        codex.monster_scroll = 9;
        codex.select_floor("d001");
        assert_eq!(codex.selected_map, map.map_id);
        assert!(codex.search.text.is_empty());
        assert!(codex.selected_monster.is_none());
        assert_eq!(codex.monster_scroll, 0);
        codex.select_floor("unknown map");
        assert_eq!(codex.selected_map, map.map_id);
    }

    #[test]
    fn default_dungeons_have_images_and_drops() {
        let maps = dungeons();
        assert_eq!(maps.len(), MAP_IMAGES.len());
        for map in maps {
            let (width, height, _) = decode_map(map_image(map.map_id).unwrap()).unwrap();
            assert_eq!((width, height), (map.width, map.height), "{}", map.map_file);
        }
        assert!(maps
            .iter()
            .any(|map| map.monsters.iter().any(|m| !m.drops.is_empty())));
        let phantom = maps.iter().find(|map| map.map_file == "D001").unwrap();
        assert!(!matches(phantom, "totally missing"));
        assert!(matches(phantom, "broodmother"));
        assert_eq!(scroll(4, 10, 4, 1.0), 5);
        assert_eq!(scroll(0, 10, 4, -1.0), 0);
    }
}
