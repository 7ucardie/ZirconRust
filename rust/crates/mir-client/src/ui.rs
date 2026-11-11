//! A very small UI toolkit for the pre-game screens, following Zircon's
//! `DXButton` / `DXTextBox` / `DXWindow` looks: buttons keep one image and
//! shift a pixel down on hover, label-less buttons are a 3-slice from
//! `Interface.Zl` 16/18/17, windows are 9-sliced from `Interface.Zl`.

use crate::assets::{lib, Assets};
use crate::gfx::{Blend, Gpu, SpriteKey, SpriteRegion, SpriteRenderer, Surface};
use crate::text::TextLayer;

pub const GOLD: [u8; 4] = [198, 166, 99, 255];
pub const WINDOW_BG: [f32; 4] = [16.0 / 255.0, 8.0 / 255.0, 8.0 / 255.0, 1.0];
pub const DEFAULT_BUTTON_HEIGHT: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// Input gathered per frame by the window layer.
#[derive(Debug, Default, Clone)]
pub struct Input {
    pub mouse: (f32, f32),
    pub lmb_down: bool,
    pub lmb_pressed: bool,
    pub lmb_released: bool,
    pub rmb_down: bool,
    pub rmb_pressed: bool,
    pub text: String,
    pub backspace: bool,
    pub enter: bool,
    pub tab: bool,
    pub escape: bool,
    /// Held modifiers (Zircon binds Ctrl+H and Alt+Q).
    pub ctrl: bool,
    pub alt: bool,
    pub command: bool,
    pub shift: bool,
    /// Letter pressed while a modifier is held (text input carries control
    /// characters then, so the physical key is used instead).
    pub chord: Option<char>,
    pub delete: bool,
    /// Arrow keys (the chat bar walks its history with them).
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Spell slot 1..=12, from F keys or the Mac Option-key fallback.
    pub fkey: Option<u8>,
    /// Belt key pressed this frame: 1..9 -> slot 0..8, 0 -> slot 9.
    pub digit: Option<u8>,
    pub wheel: f32,
}

impl Input {
    /// Clear one-shot events after a frame.
    pub fn end_frame(&mut self) {
        self.lmb_pressed = false;
        self.lmb_released = false;
        self.rmb_pressed = false;
        self.text.clear();
        self.backspace = false;
        self.enter = false;
        self.tab = false;
        self.escape = false;
        self.chord = None;
        self.delete = false;
        self.up = false;
        self.down = false;
        self.left = false;
        self.right = false;
        self.fkey = None;
        self.digit = None;
        self.wheel = 0.0;
    }
}

/// Everything a widget needs to draw itself.
pub struct Ctx<'a> {
    pub input: &'a Input,
    pub assets: &'a mut Assets,
    pub renderer: &'a mut SpriteRenderer,
    pub gpu: &'a Gpu,
    pub text: &'a mut TextLayer,
    pub now: u64,
}

impl Ctx<'_> {
    /// Open a new draw layer: sprites and text queued from now on render
    /// after everything queued so far, text included. Without this, all UI
    /// text renders in one pass above all UI sprites, so the text of an
    /// earlier window shows through a later one.
    pub fn layer(&mut self) {
        self.renderer.push_layer();
        self.text.push_layer();
    }

    pub fn sprite(&mut self, library: u16, index: u32) -> Option<SpriteRegion> {
        let key = SpriteKey {
            library,
            index,
            surface: Surface::Image,
        };
        let assets = &mut *self.assets;
        self.renderer.sprite(self.gpu, key, || {
            assets.decode(library, index, mir_formats::zl::SurfaceKind::Image)
        })
    }

    pub fn draw(&mut self, library: u16, index: u32, x: f32, y: f32) {
        if let Some(r) = self.sprite(library, index) {
            self.renderer
                .draw(r, x, y, [1.0, 1.0, 1.0, 1.0], Blend::Alpha);
        }
    }

    pub fn draw_tinted(
        &mut self,
        library: u16,
        index: u32,
        x: f32,
        y: f32,
        color: [f32; 4],
        blend: Blend,
    ) {
        if let Some(r) = self.sprite(library, index) {
            self.renderer.draw(r, x, y, color, blend);
        }
    }

    /// Draw with the image's own offsets applied (Zircon `UseOffSet`).
    pub fn draw_offset(
        &mut self,
        library: u16,
        index: u32,
        x: f32,
        y: f32,
        color: [f32; 4],
        blend: Blend,
    ) {
        let Some(info) = self.assets.info(library, index) else {
            return;
        };
        if let Some(r) = self.sprite(library, index) {
            self.renderer.draw(
                r,
                x + info.offset_x as f32,
                y + info.offset_y as f32,
                color,
                blend,
            );
        }
    }

    /// Draw a horizontal strip of `index` cropped/stretched to `w`.
    pub fn draw_strip(&mut self, library: u16, index: u32, x: f32, y: f32, w: f32) {
        if let Some(r) = self.sprite(library, index) {
            let h = r.height as f32;
            let sw = (r.width as f32).min(w);
            self.renderer
                .draw_part(r, 0.0, 0.0, sw, h, x, y, w, h, [1.0, 1.0, 1.0, 1.0]);
        }
    }

    /// Draw a vertical strip of `index` cropped/stretched to `h`.
    pub fn draw_vstrip(&mut self, library: u16, index: u32, x: f32, y: f32, h: f32) {
        if let Some(r) = self.sprite(library, index) {
            let w = r.width as f32;
            let sh = (r.height as f32).min(h);
            self.renderer
                .draw_part(r, 0.0, 0.0, w, sh, x, y, w, h, [1.0, 1.0, 1.0, 1.0]);
        }
    }

    pub fn fill(&mut self, r: Rect, color: [f32; 4]) {
        self.renderer.fill_rect(r.x, r.y, r.w, r.h, color);
    }

    pub fn border(&mut self, r: Rect, color: [u8; 4]) {
        let c = [
            color[0] as f32 / 255.0,
            color[1] as f32 / 255.0,
            color[2] as f32 / 255.0,
            color[3] as f32 / 255.0,
        ];
        self.renderer.fill_rect(r.x, r.y, r.w, 1.0, c);
        self.renderer.fill_rect(r.x, r.y + r.h - 1.0, r.w, 1.0, c);
        self.renderer.fill_rect(r.x, r.y, 1.0, r.h, c);
        self.renderer.fill_rect(r.x + r.w - 1.0, r.y, 1.0, r.h, c);
    }

    /// Zircon `DXWindow` chrome: dark body, top bar, title band, side and
    /// bottom borders, corners, optional footer band, close button.
    /// Returns true when the close button was clicked.
    pub fn window(&mut self, r: Rect, title: &str, has_footer: bool) -> bool {
        // Every window opens its own layer, so its chrome covers the text of
        // whatever was drawn before it instead of letting that text bleed
        // through (text is flushed once per layer, after that layer's sprites).
        self.layer();
        self.fill(r, WINDOW_BG);
        // Top border (0, 10 px) and title band (3, 21 px).
        self.draw_strip(lib::INTERFACE, 0, r.x, r.y, r.w);
        self.draw_strip(lib::INTERFACE, 3, r.x, r.y + 10.0, r.w);
        self.draw(lib::INTERFACE, 4, r.x, r.y + 10.0 - 3.0);
        self.draw(lib::INTERFACE, 5, r.x + r.w - 8.0, r.y + 10.0 - 3.0);
        // Sides (1) and bottom (2).
        self.draw_vstrip(lib::INTERFACE, 1, r.x, r.y, r.h);
        self.draw_vstrip(lib::INTERFACE, 1, r.x + r.w - 3.0, r.y, r.h);
        self.draw_strip(lib::INTERFACE, 2, r.x, r.y + r.h - 3.0, r.w);
        if has_footer {
            self.draw_strip(lib::INTERFACE, 10, r.x, r.y + r.h - 3.0 - 42.0, r.w);
            self.draw(lib::INTERFACE, 6, r.x, r.y + r.h - 3.0 - 42.0 - 3.0);
            self.draw(
                lib::INTERFACE,
                7,
                r.x + r.w - 8.0,
                r.y + r.h - 3.0 - 42.0 - 3.0,
            );
        }
        // Corners.
        self.draw(lib::INTERFACE, 11, r.x, r.y);
        self.draw(lib::INTERFACE, 12, r.x + r.w - 24.0, r.y);
        self.draw(lib::INTERFACE, 8, r.x, r.y + r.h - 8.0);
        self.draw(lib::INTERFACE, 9, r.x + r.w - 8.0, r.y + r.h - 8.0);
        self.text
            .draw_centered(title, 14, r.x + r.w / 2.0, r.y + 8.0, GOLD);
        // Close button (15, 21x21).
        let close = Rect::new(r.x + r.w - 21.0 - 3.0, r.y + 3.0, 21.0, 21.0);
        let hover = close.contains(self.input.mouse.0, self.input.mouse.1);
        self.draw(
            lib::INTERFACE,
            15,
            close.x,
            close.y + if hover { 1.0 } else { 0.0 },
        );
        hover && self.input.lmb_released
    }

    /// A bordered panel like Zircon's `DXControl` with `Border = true`.
    pub fn panel(&mut self, r: Rect, bg: [u8; 4]) {
        self.fill(
            r,
            [
                bg[0] as f32 / 255.0,
                bg[1] as f32 / 255.0,
                bg[2] as f32 / 255.0,
                bg[3] as f32 / 255.0,
            ],
        );
        self.border(r, GOLD);
    }

    /// Animated control: `base + frame`, frames advance evenly over `period_ms`.
    #[allow(clippy::too_many_arguments)]
    pub fn anim(
        &mut self,
        library: u16,
        base: u32,
        frames: u32,
        period_ms: u64,
        x: f32,
        y: f32,
        use_offset: bool,
        blend: bool,
        start: u64,
    ) {
        if frames == 0 {
            return;
        }
        let per = (period_ms / frames as u64).max(1);
        let frame = ((self.now.saturating_sub(start)) / per) % frames as u64;
        let index = base + frame as u32;
        let b = if blend { Blend::Screen } else { Blend::Alpha };
        if use_offset {
            self.draw_offset(library, index, x, y, [1.0, 1.0, 1.0, 1.0], b);
        } else {
            self.draw_tinted(library, index, x, y, [1.0, 1.0, 1.0, 1.0], b);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ButtonStyle {
    /// A single image that never changes frame.
    Image { library: u16, index: u32 },
    /// Zircon's default 3-slice button of a given width.
    Default { width: f32 },
}

pub struct Button {
    pub style: ButtonStyle,
    pub pos: (f32, f32),
    pub label: Option<String>,
    pub enabled: bool,
    /// Drawn as pressed (used for toggle groups like class selection).
    pub selected: bool,
    pressed: bool,
    size: (f32, f32),
}

impl Button {
    pub fn image(library: u16, index: u32, x: f32, y: f32) -> Button {
        Button {
            style: ButtonStyle::Image { library, index },
            pos: (x, y),
            label: None,
            enabled: true,
            selected: false,
            pressed: false,
            size: (0.0, 0.0),
        }
    }

    pub fn default_style(x: f32, y: f32, width: f32, label: &str) -> Button {
        Button {
            style: ButtonStyle::Default { width },
            pos: (x, y),
            label: Some(label.to_string()),
            enabled: true,
            selected: false,
            pressed: false,
            size: (width, DEFAULT_BUTTON_HEIGHT),
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.pos.0, self.pos.1, self.size.0, self.size.1)
    }

    /// Draw and return true when clicked this frame.
    pub fn update(&mut self, ctx: &mut Ctx) -> bool {
        if let ButtonStyle::Image { library, index } = self.style {
            if let Some(info) = ctx.assets.info(library, index) {
                self.size = (info.width as f32, info.height as f32);
            }
        }
        let rect = self.rect();
        let hover = self.enabled && rect.contains(ctx.input.mouse.0, ctx.input.mouse.1);
        if hover && ctx.input.lmb_pressed {
            self.pressed = true;
        }
        let clicked = self.pressed && hover && ctx.input.lmb_released;
        if ctx.input.lmb_released {
            self.pressed = false;
        }
        let down = self.selected || (self.pressed && hover);
        let dy = if hover || down { 1.0 } else { 0.0 };
        let tint = if self.enabled {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            [0.6, 0.6, 0.6, 1.0]
        };
        match self.style {
            ButtonStyle::Image { library, index } => {
                ctx.draw_tinted(library, index, rect.x, rect.y + dy, tint, Blend::Alpha);
            }
            ButtonStyle::Default { width } => {
                let y = rect.y + dy;
                ctx.draw_tinted(lib::INTERFACE, 16, rect.x, y, tint, Blend::Alpha);
                if let Some(mid) = ctx.sprite(lib::INTERFACE, 18) {
                    let inner = (width - 14.0).max(0.0);
                    let h = mid.height as f32;
                    ctx.renderer.draw_part(
                        mid,
                        0.0,
                        0.0,
                        (mid.width as f32).min(inner),
                        h,
                        rect.x + 7.0,
                        y,
                        inner,
                        h,
                        tint,
                    );
                }
                ctx.draw_tinted(
                    lib::INTERFACE,
                    17,
                    rect.x + width - 7.0,
                    y,
                    tint,
                    Blend::Alpha,
                );
            }
        }
        if let Some(label) = &self.label {
            let color = if !self.enabled {
                [170, 170, 170, 255]
            } else if hover || down {
                [255, 255, 255, 255]
            } else {
                [217, 217, 217, 255]
            };
            ctx.text.draw_centered(
                label,
                13,
                rect.x + rect.w / 2.0,
                rect.y + dy + rect.h / 2.0 - 8.0,
                color,
            );
        }
        clicked
    }
}

pub struct TextBox {
    pub rect: Rect,
    pub text: String,
    pub masked: bool,
    pub max_len: usize,
    pub focused: bool,
    /// Character position; None follows the end when callers replace text.
    cursor: Option<usize>,
    /// Optional validity for the border colour: None = neutral, Some(true) green, Some(false) red.
    pub valid: Option<bool>,
}

impl TextBox {
    pub fn new(rect: Rect, max_len: usize) -> TextBox {
        TextBox {
            rect,
            text: String::new(),
            masked: false,
            max_len,
            focused: false,
            cursor: None,
            valid: None,
        }
    }

    pub fn masked(mut self) -> TextBox {
        self.masked = true;
        self
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = None;
    }

    fn byte_offset(&self, position: usize) -> usize {
        self.text
            .char_indices()
            .nth(position)
            .map_or(self.text.len(), |(index, _)| index)
    }

    pub(crate) fn edit(&mut self, input: &Input) {
        if !self.focused {
            return;
        }
        let mut position = self
            .cursor
            .unwrap_or(self.text.chars().count())
            .min(self.text.chars().count());
        if input.left {
            position = position.saturating_sub(1);
        }
        if input.right {
            position = (position + 1).min(self.text.chars().count());
        }
        for c in input.text.chars() {
            if !c.is_control() && self.text.chars().count() < self.max_len {
                self.text.insert(self.byte_offset(position), c);
                position += 1;
            }
        }
        if input.backspace && position > 0 {
            self.text.remove(self.byte_offset(position - 1));
            position -= 1;
        }
        if input.delete && position < self.text.chars().count() {
            self.text.remove(self.byte_offset(position));
        }
        self.cursor = (position < self.text.chars().count()).then_some(position);
    }

    /// Handle typing (only when focused) and draw. Returns true if Enter was
    /// pressed while focused.
    pub fn update(&mut self, ctx: &mut Ctx) -> bool {
        if ctx.input.lmb_pressed {
            self.focused = self.rect.contains(ctx.input.mouse.0, ctx.input.mouse.1);
        }
        self.edit(ctx.input);
        let submitted = self.focused && ctx.input.enter;
        ctx.fill(self.rect, WINDOW_BG);
        let border = match self.valid {
            None => GOLD,
            Some(true) => [0, 160, 0, 255],
            Some(false) => [200, 0, 0, 255],
        };
        ctx.border(self.rect, border);
        let shown: String = if self.masked {
            self.text.chars().map(|_| '*').collect()
        } else {
            self.text.clone()
        };
        let caret = if self.focused && (ctx.now / 500).is_multiple_of(2) {
            "|"
        } else {
            ""
        };
        let position = self
            .cursor
            .unwrap_or(shown.chars().count())
            .min(shown.chars().count());
        let before: String = shown.chars().take(position).collect();
        let after: String = shown.chars().skip(position).collect();
        ctx.text.draw(
            &format!("{before}{caret}{after}"),
            13,
            self.rect.x + 4.0,
            self.rect.y + (self.rect.h - 17.0) / 2.0,
            [255, 255, 255, 255],
        );
        submitted
    }
}

/// Move focus to the next box on Tab.
pub fn cycle_focus(boxes: &mut [&mut TextBox], input: &Input) {
    if !input.tab || boxes.is_empty() {
        return;
    }
    let current = boxes.iter().position(|b| b.focused);
    let next = current.map(|i| (i + 1) % boxes.len()).unwrap_or(0);
    for (i, b) in boxes.iter_mut().enumerate() {
        b.focused = i == next;
    }
}

/// Right-aligned caption next to a field, like Zircon's dialog labels.
pub fn caption(ctx: &mut Ctx, text: &str, field: Rect) {
    let w = ctx.text.width(text, 13);
    ctx.text.draw(
        text,
        13,
        field.x - w - 5.0,
        field.y + (field.h - 17.0) / 2.0,
        [255, 255, 255, 255],
    );
}

#[cfg(test)]
mod text_box_tests {
    use super::*;

    #[test]
    fn arrow_keys_move_caret_without_deleting_text() {
        let mut box_ = TextBox::new(Rect::new(0.0, 0.0, 170.0, 18.0), 50);
        box_.focused = true;
        box_.text = "abc".into();
        box_.edit(&Input {
            left: true,
            ..Input::default()
        });
        assert_eq!(box_.text, "abc");
        box_.edit(&Input {
            text: "X".into(),
            ..Input::default()
        });
        assert_eq!(box_.text, "abXc");
        box_.edit(&Input {
            backspace: true,
            ..Input::default()
        });
        assert_eq!(box_.text, "abc");
        box_.edit(&Input {
            right: true,
            ..Input::default()
        });
        box_.edit(&Input {
            text: "!".into(),
            ..Input::default()
        });
        assert_eq!(box_.text, "abc!");
    }
}
