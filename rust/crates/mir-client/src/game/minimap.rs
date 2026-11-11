//! Minimap and big map (Zircon `MiniMapDialog` / `BigMapDialog`).
//!
//! Both draw the same image: `MapInfo.MiniMap` in the `MiniMap` library,
//! which covers the whole map, so a cell is `image_size / map_size` pixels.
//! The image is drawn at 1:1 scrolled so the player sits in the middle of
//! the panel and clipped to its edges (Zircon `ClipMap`), with coloured
//! dots for everything the player can see. Maps without an authored image
//! use a cached walkability overview at two pixels per cell.

use super::*;
use crate::assets::lib;
use crate::gfx::Blend;
use crate::ui::{Ctx, Rect};
use crate::windows::WindowLayout;

/// Panel size of the minimap (Zircon's default window is 200x200).
const MINI_SIZE: f32 = 200.0;
/// Margin from the screen edges.
const MARGIN: f32 = 10.0;
/// Room for the buff icons in the top-right corner.
const MINI_TOP: f32 = 70.0;
/// Room above the big map, and below it for the bottom HUD: the window sits
/// in the band between them so the health bars, spell bar and belt stay
/// readable while it is open.
const BIG_TOP: f32 = 40.0;
const BIG_BOTTOM: f32 = 190.0;

/// Minimap visibility, cycled by its key: Zircon goes shown -> dimmed ->
/// hidden (`GameScene.Input` `MapMiniWindow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniMode {
    Shown,
    Dimmed,
    Hidden,
}

pub struct MiniMap {
    pub mode: MiniMode,
    pub big_open: bool,
    pub(super) navigation: Option<NavigationMap>,
    pub(super) safe_cells: Option<std::collections::HashSet<(i32, i32)>>,
    layout: WindowLayout,
}

impl Default for MiniMap {
    fn default() -> MiniMap {
        // ZIRCON_OPEN=bigmap / minimap for screenshots.
        let open: Vec<String> = std::env::var("ZIRCON_OPEN")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        MiniMap {
            navigation: None,
            safe_cells: None,
            layout: WindowLayout::default(),
            mode: if open.iter().any(|s| s == "nominimap") {
                MiniMode::Hidden
            } else {
                MiniMode::Shown
            },
            big_open: open.iter().any(|s| s == "bigmap"),
        }
    }
}

impl MiniMap {
    fn mini_rect(width: i32) -> Rect {
        Rect::new(
            width as f32 - MINI_SIZE - MARGIN,
            MINI_TOP,
            MINI_SIZE,
            MINI_SIZE,
        )
    }

    fn big_rect(width: i32, height: i32) -> Rect {
        let band = (height as f32 - BIG_TOP - BIG_BOTTOM).max(160.0);
        let win_w = (width as f32 - 80.0).min(900.0);
        let win_h = band.min(700.0);
        Rect::new(
            (width as f32 - win_w) / 2.0,
            BIG_TOP + (band - win_h) / 2.0,
            win_w,
            win_h,
        )
    }

    pub(super) fn contains_mouse(&self, mouse: (f32, f32), width: i32, height: i32) -> bool {
        if self.big_open {
            self.layout.contains_mouse(
                "big_map",
                Self::big_rect(width, height),
                mouse,
                (width, height),
            )
        } else if self.mode != MiniMode::Hidden {
            Self::mini_rect(width).contains(mouse.0, mouse.1)
        } else {
            false
        }
    }

    /// Zircon cycles the minimap with one key: shown, dimmed, hidden.
    pub fn cycle(&mut self) {
        self.mode = match self.mode {
            MiniMode::Shown => MiniMode::Dimmed,
            MiniMode::Dimmed => MiniMode::Hidden,
            MiniMode::Hidden => MiniMode::Shown,
        };
    }

    fn opacity(&self) -> f32 {
        match self.mode {
            MiniMode::Shown => 1.0,
            MiniMode::Dimmed => 0.5,
            MiniMode::Hidden => 0.0,
        }
    }
}

/// Horizontal floor runs, computed once when a map loads. Artwork remains in
/// the existing sprite pipeline; this overview only describes walkable terrain.
#[derive(Debug, Clone)]
pub(super) struct NavigationMap {
    rows: Vec<Vec<(i32, i32)>>,
}

impl NavigationMap {
    pub(super) fn from_map(map: &MapFile) -> Self {
        let rows = (0..i32::from(map.height))
            .map(|y| {
                let mut runs = Vec::new();
                let mut x = 0;
                while x < i32::from(map.width) {
                    if !map.is_walkable(x, y) {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < i32::from(map.width) && map.is_walkable(x, y) {
                        x += 1;
                    }
                    runs.push((start, x));
                }
                runs
            })
            .collect();
        Self { rows }
    }

    fn draw(
        &self,
        renderer: &mut SpriteRenderer,
        area: Rect,
        off_x: f32,
        off_y: f32,
        opacity: f32,
    ) {
        for (y, runs) in self.rows.iter().enumerate() {
            let top = area.y + off_y + y as f32 * 2.0;
            if top + 2.0 <= area.y || top >= area.y + area.h {
                continue;
            }
            for &(start, end) in runs {
                let left = area.x + off_x + start as f32 * 2.0;
                if let Some(rect) = clip_floor(area, left, top, (end - start) as f32 * 2.0) {
                    renderer.fill_rect(rect.x, rect.y, rect.w, rect.h, [0.46, 0.43, 0.34, opacity]);
                }
            }
        }
    }
}

fn clip_floor(area: Rect, x: f32, y: f32, width: f32) -> Option<Rect> {
    clip_map_rect(area, x, y, width, 2.0)
}

fn clip_map_rect(area: Rect, x: f32, y: f32, width: f32, height: f32) -> Option<Rect> {
    let left = x.max(area.x);
    let top = y.max(area.y);
    let right = (x + width).min(area.x + area.w);
    let bottom = (y + height).min(area.y + area.h);
    (right > left && bottom > top).then(|| Rect::new(left, top, right - left, bottom - top))
}

fn safe_zone_edges(cells: &std::collections::HashSet<(i32, i32)>, x: i32, y: i32) -> [bool; 4] {
    [
        !cells.contains(&(x, y - 1)),
        !cells.contains(&(x + 1, y)),
        !cells.contains(&(x, y + 1)),
        !cells.contains(&(x - 1, y)),
    ]
}

fn draw_clipped(
    renderer: &mut SpriteRenderer,
    area: Rect,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    if let Some(rect) = clip_map_rect(area, x, y, width, height) {
        renderer.fill_rect(rect.x, rect.y, rect.w, rect.h, color);
    }
}

/// One marker on the map, in map cells.
struct Dot {
    cell: Point,
    size: f32,
    color: [f32; 4],
}

const LIME: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const ORANGE: [f32; 4] = [1.0, 0.65, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const DEEP_PINK: [f32; 4] = [1.0, 0.08, 0.58, 1.0];
const DEEP_SKY_BLUE: [f32; 4] = [0.0, 0.75, 1.0, 1.0];
const NPC_YELLOW: [f32; 4] = [1.0, 0.9, 0.2, 1.0];

impl Game {
    /// Everything worth a dot: the player (lime), group mates (lime),
    /// the partner (pink), guild mates (sky blue), other players (white),
    /// monsters (red, own pets orange) and NPCs (yellow).
    fn map_dots(&self) -> Vec<Dot> {
        let me = self.user;
        let my_name = self.user_name();
        let guild_members: Vec<&str> = self
            .guild
            .as_ref()
            .map(|g| g.members.iter().map(|m| m.name.as_str()).collect())
            .unwrap_or_default();
        let mut dots: Vec<Dot> = self
            .portal_markers
            .iter()
            .map(|portal| Dot {
                cell: Point {
                    x: portal.position.0,
                    y: portal.position.1,
                },
                size: 5.0,
                color: [0.2, 0.85, 1.0, 1.0],
            })
            .collect();
        for (id, o) in &self.objects {
            if o.is_spell() || o.is_item() {
                continue;
            }
            let (size, color) = if o.is_monster() {
                if o.dead {
                    continue;
                }
                match o.pet_owner() {
                    Some(owner) if Some(owner) == my_name => (4.0, ORANGE),
                    _ => (3.0, RED),
                }
            } else if o.is_npc() {
                (3.0, NPC_YELLOW)
            } else if Some(*id) == me {
                (5.0, LIME)
            } else if self.group.iter().any(|(g, _)| g == id) {
                (4.0, LIME)
            } else if self.partner.as_deref() == Some(o.name()) {
                (3.0, DEEP_PINK)
            } else if guild_members.contains(&o.name()) {
                (3.0, DEEP_SKY_BLUE)
            } else {
                (3.0, WHITE)
            };
            dots.push(Dot {
                cell: o.location,
                size,
                color,
            });
            if Some(*id) == me {
                // A white pip inside the lime square, so the player stands
                // out from the NPC and group dots.
                dots.push(Dot {
                    cell: o.location,
                    size: 2.0,
                    color: WHITE,
                });
            }
        }
        dots
    }

    /// Draw one map panel: the image scrolled so `centre` sits in the
    /// middle, clipped to the panel, with the dots on top.
    #[allow(clippy::too_many_arguments)]
    fn draw_map_panel(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        area: Rect,
        index: i32,
        centre: Point,
        dots: &[Dot],
        opacity: f32,
    ) -> bool {
        let Some(map) = self.map.as_ref() else {
            return false;
        };
        let (map_w, map_h) = (map.width.max(1) as f32, map.height.max(1) as f32);
        let region = if index > 0 {
            Self::sprite(
                &mut self.assets,
                renderer,
                gpu,
                lib::MINI_MAP,
                index as u32,
                Surface::Image,
            )
        } else {
            None
        };
        if region.is_none() && self.minimap.navigation.is_none() {
            return false;
        }
        let (img_w, img_h) = region
            .as_ref()
            .map(|r| (r.width as f32, r.height as f32))
            .unwrap_or((map_w * 2.0, map_h * 2.0));
        let scale_x = img_w / map_w;
        let scale_y = img_h / map_h;

        // Zircon `ClipMap`: keep the image over the panel, or centre it
        // when it is smaller than the panel.
        let mut off_x = area.w / 2.0 - scale_x * centre.x as f32;
        let mut off_y = area.h / 2.0 - scale_y * centre.y as f32;
        off_x = if img_w < area.w {
            (area.w - img_w) / 2.0
        } else {
            off_x.clamp(area.w - img_w, 0.0)
        };
        off_y = if img_h < area.h {
            (area.h - img_h) / 2.0
        } else {
            off_y.clamp(area.h - img_h, 0.0)
        };

        let src_x = (-off_x).max(0.0);
        let src_y = (-off_y).max(0.0);
        let dst_x = area.x + off_x.max(0.0);
        let dst_y = area.y + off_y.max(0.0);
        let w = (area.w - off_x.max(0.0)).min(img_w - src_x).max(0.0);
        let h = (area.h - off_y.max(0.0)).min(img_h - src_y).max(0.0);
        if let Some(region) = region {
            if w >= 1.0 && h >= 1.0 {
                let sub = region.sub(src_x as u32, src_y as u32, w as u32, h as u32);
                renderer.draw_scaled(
                    sub,
                    dst_x,
                    dst_y,
                    sub.width as f32,
                    sub.height as f32,
                    [1.0, 1.0, 1.0, opacity],
                    Blend::Alpha,
                );
            }
        } else if let Some(navigation) = &self.minimap.navigation {
            navigation.draw(renderer, area, off_x, off_y, opacity);
        }
        if let Some(cells) = &self.minimap.safe_cells {
            for &(cx, cy) in cells {
                let x = area.x + off_x + scale_x * cx as f32;
                let y = area.y + off_y + scale_y * cy as f32;
                draw_clipped(
                    renderer,
                    area,
                    x,
                    y,
                    scale_x,
                    scale_y,
                    [0.85, 0.55, 0.08, 0.2 * opacity],
                );
            }
            let line = scale_x.min(scale_y).clamp(1.0, 1.5);
            let border = [1.0, 0.8, 0.1, opacity];
            for &(cx, cy) in cells {
                let x = area.x + off_x + scale_x * cx as f32;
                let y = area.y + off_y + scale_y * cy as f32;
                let [top, right, bottom, left] = safe_zone_edges(cells, cx, cy);
                if top {
                    draw_clipped(renderer, area, x, y, scale_x, line, border);
                }
                if right {
                    draw_clipped(renderer, area, x + scale_x - line, y, line, scale_y, border);
                }
                if bottom {
                    draw_clipped(renderer, area, x, y + scale_y - line, scale_x, line, border);
                }
                if left {
                    draw_clipped(renderer, area, x, y, line, scale_y, border);
                }
            }
        }
        for d in dots {
            let x = area.x + off_x + scale_x * d.cell.x as f32 - d.size / 2.0;
            let y = area.y + off_y + scale_y * d.cell.y as f32 - d.size / 2.0;
            if x < area.x
                || y < area.y
                || x + d.size > area.x + area.w
                || y + d.size > area.y + area.h
            {
                continue;
            }
            let color = [d.color[0], d.color[1], d.color[2], d.color[3] * opacity];
            renderer.fill_rect(x, y, d.size, d.size, color);
        }
        true
    }

    /// Both map windows, drawn after the HUD so they sit above the world.
    /// `want_big` picks which of the two this call draws. The minimap is a
    /// corner readout, so it goes down with the HUD and a window covers it;
    /// the big map is a dialog, so it goes over the windows.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_map_windows(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        text: &mut TextLayer,
        width: i32,
        height: i32,
        now: u64,
        want_big: bool,
    ) {
        if self.map.is_none() {
            return;
        }
        let Some(user) = self.user() else {
            return;
        };
        let centre = user.location;
        let index = self.mini_map_index;
        let show_mini =
            !want_big && self.minimap.mode != MiniMode::Hidden && !self.minimap.big_open;
        let show_big = want_big && self.minimap.big_open;
        if !show_mini && !show_big {
            return;
        }
        let dots = self.map_dots();
        let name = self.map_name.clone();
        let opacity = self.minimap.opacity();

        let mut closed_mini = false;
        let mut closed_big = false;
        if show_mini {
            let win = MiniMap::mini_rect(width);
            let area = Rect::new(win.x + 4.0, win.y + 34.0, win.w - 8.0, win.h - 38.0);
            {
                let mut c = Ctx {
                    input: &self.input,
                    assets: &mut self.assets,
                    renderer,
                    gpu,
                    text,
                    now,
                };
                if c.window(win, &name, false) {
                    closed_mini = true;
                }
                c.fill(area, [0.0, 0.0, 0.0, 0.6 * opacity]);
            }
            if !self.draw_map_panel(gpu, renderer, area, index, centre, &dots, opacity) {
                text.draw(
                    "No map image.",
                    11,
                    area.x + 6.0,
                    area.y + 6.0,
                    [160, 160, 160, (160.0 * opacity) as u8],
                );
            }
            if let Some(cells) = &self.minimap.safe_cells {
                text.draw(
                    if cells.is_empty() {
                        "No safe zone on this map"
                    } else {
                        "Gold border: safe zone"
                    },
                    10,
                    area.x + 4.0,
                    area.y + 4.0,
                    [255, 220, 90, (255.0 * opacity) as u8],
                );
            }
            text.draw(
                &format!("{}, {}", centre.x, centre.y),
                11,
                area.x + 4.0,
                area.y + area.h - 16.0,
                [255, 255, 200, (255.0 * opacity) as u8],
            );
        }

        if show_big {
            let win = self.minimap.layout.place(
                "big_map",
                MiniMap::big_rect(width, height),
                &self.input,
                (width, height),
            );
            let area = Rect::new(win.x + 6.0, win.y + 36.0, win.w - 12.0, win.h - 44.0);
            {
                let mut c = Ctx {
                    input: &self.input,
                    assets: &mut self.assets,
                    renderer,
                    gpu,
                    text,
                    now,
                };
                if c.window(win, &format!("Map: {name}"), false)
                    && !self.minimap.layout.is_dragging()
                {
                    closed_big = true;
                }
                c.fill(area, [0.0, 0.0, 0.0, 0.85]);
            }
            if !self.draw_map_panel(gpu, renderer, area, index, centre, &dots, 1.0) {
                text.draw_centered(
                    "This map has no map image.",
                    13,
                    area.x + area.w / 2.0,
                    area.y + area.h / 2.0,
                    [200, 200, 200, 255],
                );
            }
            if let Some(cells) = &self.minimap.safe_cells {
                text.draw(
                    if cells.is_empty() {
                        "No safe zone on this map; storage unavailable"
                    } else {
                        "Gold border: safe zone (storage available inside)"
                    },
                    12,
                    area.x + 6.0,
                    area.y + 6.0,
                    [255, 220, 90, 255],
                );
            }
            text.draw(
                &format!("{} ({}, {})  -  X closes", name, centre.x, centre.y),
                12,
                area.x + 6.0,
                area.y + area.h - 18.0,
                [255, 255, 200, 255],
            );
            self.minimap.layout.end_frame(&self.input);
        }
        if closed_mini {
            self.minimap.mode = MiniMode::Hidden;
        }
        if closed_big {
            self.minimap.big_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clip_floor, safe_zone_edges, MiniMap, MiniMode, NavigationMap, Rect, WindowLayout,
    };
    use mir_formats::map::MapFile;
    use std::collections::HashSet;

    #[test]
    fn overview_preserves_floor_gaps_and_map_edges() {
        let mut map = MapFile::blank(8, 4);
        for y in 0..4 {
            for x in 0..8 {
                map.cell_mut(x, y).unwrap().flag = 0;
            }
        }
        for (x, y) in [(0, 0), (1, 0), (3, 0), (7, 0), (3, 1), (3, 2)] {
            map.cell_mut(x, y).unwrap().flag = 3;
        }
        // A partially set collision flag must not become navigable terrain.
        map.cell_mut(2, 0).unwrap().flag = 1;
        let nav = NavigationMap::from_map(&map);
        assert_eq!(nav.rows[0], vec![(0, 2), (3, 4), (7, 8)]);
        assert_eq!(nav.rows[1], vec![(3, 4)]);
        assert!(nav.rows[3].is_empty());
        for y in 0..4 {
            for x in 0..8 {
                assert_eq!(
                    nav.rows[y as usize].iter().any(|&(a, b)| x >= a && x < b),
                    map.is_walkable(x, y)
                );
            }
        }
    }

    #[test]
    fn overview_clips_scrolled_floor_to_panel() {
        let area = Rect::new(10.0, 20.0, 100.0, 50.0);
        let rect = clip_floor(area, 5.0, 19.0, 20.0).unwrap();
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (10.0, 20.0, 15.0, 1.0));
        let rect = clip_floor(area, 105.0, 69.0, 20.0).unwrap();
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (105.0, 69.0, 5.0, 1.0));
        assert!(clip_floor(area, 110.0, 30.0, 2.0).is_none());
        assert!(clip_floor(area, 20.0, 70.0, 2.0).is_none());
        assert!(clip_floor(area, 0.0, 30.0, 10.0).is_none());
    }

    #[test]
    fn cycle_matches_zircon_order() {
        let mut m = MiniMap {
            mode: MiniMode::Shown,
            big_open: false,
            navigation: None,
            safe_cells: None,
            layout: WindowLayout::default(),
        };
        m.cycle();
        assert_eq!(m.mode, MiniMode::Dimmed);
        m.cycle();
        assert_eq!(m.mode, MiniMode::Hidden);
        m.cycle();
        assert_eq!(m.mode, MiniMode::Shown);
    }

    #[test]
    fn visible_maps_block_ground_item_drop() {
        let mut map = MiniMap {
            mode: MiniMode::Shown,
            big_open: false,
            navigation: None,
            safe_cells: None,
            layout: WindowLayout::default(),
        };
        assert!(map.contains_mouse((900.0, 100.0), 1024, 768));
        assert!(!map.contains_mouse((400.0, 100.0), 1024, 768));
        map.mode = MiniMode::Hidden;
        assert!(!map.contains_mouse((900.0, 100.0), 1024, 768));
        map.big_open = true;
        assert!(map.contains_mouse((512.0, 350.0), 1024, 768));
        assert!(!map.contains_mouse((20.0, 350.0), 1024, 768));
        assert!(!map.contains_mouse((100.0, 600.0), 1024, 768));
        let input = crate::ui::Input {
            mouse: (150.0, 50.0),
            lmb_down: true,
            lmb_pressed: true,
            ..Default::default()
        };
        map.layout
            .place("big_map", MiniMap::big_rect(1024, 768), &input, (1024, 768));
        let input = crate::ui::Input {
            mouse: (180.0, 100.0),
            lmb_down: true,
            ..Default::default()
        };
        map.layout
            .place("big_map", MiniMap::big_rect(1024, 768), &input, (1024, 768));
        map.layout.end_frame(&crate::ui::Input::default());
        assert!(map.contains_mouse((100.0, 600.0), 1024, 768));
    }

    #[test]
    fn safe_zone_border_excludes_shared_cell_edges() {
        let cells: HashSet<_> = [(2, 3), (3, 3), (2, 4), (3, 4)].into_iter().collect();
        assert_eq!(safe_zone_edges(&cells, 2, 3), [true, false, false, true]);
        assert_eq!(safe_zone_edges(&cells, 3, 3), [true, true, false, false]);
        assert_eq!(safe_zone_edges(&cells, 2, 4), [false, false, true, true]);
        assert_eq!(safe_zone_edges(&cells, 3, 4), [false, true, true, false]);
    }
}
