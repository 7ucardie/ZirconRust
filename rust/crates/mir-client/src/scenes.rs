//! Login and character-select scenes, laid out like Zircon's `LoginScene`
//! and `SelectScene` (1024x768 design, centred in the window).

use mir_proto::{
    rules, CharacterSummary, Class, ClientMessage, Gender, NewAccountResult, NewCharacterResult,
};

use crate::assets::{lib, Assets};
use crate::gfx::{Blend, Gpu, SpriteRenderer};
use crate::text::TextLayer;
use crate::ui::{caption, cycle_focus, Button, Ctx, Input, Rect, TextBox, GOLD};

const INTERFACE1C: u16 = 1;
const PROG_USE: u16 = 13;
const EQUIP: u16 = 6;
const SCENE_W: f32 = 1024.0;
const SCENE_H: f32 = 768.0;

pub enum SceneAction {
    None,
    Send(ClientMessage),
    Quit,
}

fn ctx<'a>(
    input: &'a Input,
    assets: &'a mut Assets,
    renderer: &'a mut SpriteRenderer,
    gpu: &'a Gpu,
    text: &'a mut TextLayer,
    now: u64,
) -> Ctx<'a> {
    Ctx {
        input,
        assets,
        renderer,
        gpu,
        text,
        now,
    }
}

/// Top-left of the 1024x768 design area inside the window.
fn origin(width: i32, height: i32) -> (f32, f32) {
    (
        ((width as f32 - SCENE_W) / 2.0).floor(),
        ((height as f32 - SCENE_H) / 2.0).floor(),
    )
}

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

struct NewAccountDialog {
    email: TextBox,
    password: TextBox,
    confirm: TextBox,
    create: Button,
    cancel: Button,
    status: String,
    pending: bool,
}

pub struct LoginScene {
    pub status: String,
    email: TextBox,
    password: TextBox,
    login: Button,
    exit: Button,
    new_account: Button,
    dialog: Option<NewAccountDialog>,
    start: u64,
    pending: bool,
}

impl LoginScene {
    pub fn new() -> LoginScene {
        LoginScene {
            status: String::new(),
            email: TextBox::new(Rect::new(0.0, 0.0, 170.0, 18.0), rules::EMAIL_MAX),
            password: TextBox::new(Rect::new(0.0, 0.0, 170.0, 18.0), rules::PASSWORD_MAX).masked(),
            login: Button::default_style(0.0, 0.0, 100.0, "Log In"),
            exit: Button::default_style(0.0, 0.0, 100.0, "Exit Game"),
            new_account: Button::default_style(0.0, 0.0, 136.0, "New Account"),
            dialog: None,
            start: 0,
            pending: false,
        }
    }

    pub fn on_new_account(&mut self, r: NewAccountResult) {
        if let Some(d) = &mut self.dialog {
            d.pending = false;
            match r {
                NewAccountResult::Success => {
                    self.email.set_text(d.email.text.clone());
                    self.password.set_text(String::new());
                    self.status = "Account created. Please log in.".into();
                    self.dialog = None;
                }
                NewAccountResult::Failed { reason } => d.status = reason,
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        input: &Input,
        assets: &mut Assets,
        renderer: &mut SpriteRenderer,
        gpu: &Gpu,
        text: &mut TextLayer,
        width: i32,
        height: i32,
        now: u64,
    ) -> SceneAction {
        if self.start == 0 {
            self.start = now;
            self.email.focused = true;
        }
        let (ox, oy) = origin(width, height);
        let mut c = ctx(input, assets, renderer, gpu, text, now);

        // Background and its animated layers (Zircon LoginScene).
        c.draw(INTERFACE1C, 20, ox, oy);
        let start = self.start;
        c.anim(INTERFACE1C, 2200, 100, 10_000, ox, oy, true, false, start);
        c.anim(INTERFACE1C, 2400, 30, 5_000, ox, oy, true, false, start);
        c.anim(INTERFACE1C, 2300, 30, 10_000, ox, oy, true, true, start);
        c.anim(INTERFACE1C, 2500, 30, 8_000, ox, oy, true, false, start);
        // The original logo sprite says Legend of Mir; keep the background,
        // but use the game's own name on the login screen.
        let title = Rect::new(ox + (SCENE_W - 540.0) / 2.0, oy + 36.0, 540.0, 94.0);
        c.fill(title, [0.08, 0.03, 0.01, 0.78]);
        c.border(title, [151, 109, 46, 255]);
        c.text.draw_centered(
            "ZirconRust",
            48,
            ox + SCENE_W / 2.0 + 2.0,
            oy + 53.0 + 3.0,
            [37, 16, 6, 255],
        );
        c.text.draw_centered(
            "ZirconRust",
            48,
            ox + SCENE_W / 2.0,
            oy + 53.0,
            [247, 207, 112, 255],
        );

        // Login window at the bottom of the scene.
        let win = Rect::new(
            ox + (SCENE_W - 800.0) / 2.0,
            oy + SCENE_H - 125.0 - 20.0,
            800.0,
            125.0,
        );
        let dialog_open = self.dialog.is_some();
        let closed = c.window(win, "", false);
        if closed {
            return SceneAction::Quit;
        }
        c.text.draw(
            "Enter your Email and Password",
            13,
            win.x + 70.0,
            win.y + 36.0,
            [214, 190, 148, 255],
        );
        self.email.rect = Rect::new(win.x + 70.0, win.y + 65.0, 170.0, 18.0);
        self.password.rect = Rect::new(win.x + 357.0, win.y + 65.0, 170.0, 18.0);
        caption(&mut c, "Email", self.email.rect);
        caption(&mut c, "Password", self.password.rect);
        self.login.pos = (win.x + 550.0, win.y + 62.0);
        self.exit.pos = (win.x + 660.0, win.y + 62.0);
        self.new_account.pos = (win.x + 485.0, win.y + 32.0);

        let mut action = SceneAction::None;
        if !dialog_open {
            let inactive = Input::default();
            let _ = inactive;
            cycle_focus(&mut [&mut self.email, &mut self.password], c.input);
            let e = self.email.update(&mut c);
            let p = self.password.update(&mut c);
            self.email.valid = Some(rules::valid_email(&self.email.text));
            self.password.valid = Some(rules::valid_password(&self.password.text));
            self.login.enabled = self.email.valid == Some(true)
                && self.password.valid == Some(true)
                && !self.pending;
            let submit = (e || p) && self.login.enabled;
            if self.login.update(&mut c) || submit {
                self.pending = true;
                self.status = "Logging in...".into();
                action = SceneAction::Send(ClientMessage::Login {
                    email: self.email.text.clone(),
                    password: self.password.text.clone(),
                });
            }
            if self.exit.update(&mut c) {
                return SceneAction::Quit;
            }
            if self.new_account.update(&mut c) {
                self.dialog = Some(NewAccountDialog {
                    email: TextBox::new(Rect::default_zero(), rules::EMAIL_MAX),
                    password: TextBox::new(Rect::default_zero(), rules::PASSWORD_MAX).masked(),
                    confirm: TextBox::new(Rect::default_zero(), rules::PASSWORD_MAX).masked(),
                    create: Button::default_style(0.0, 0.0, 80.0, "Create"),
                    cancel: Button::default_style(0.0, 0.0, 80.0, "Cancel"),
                    status: String::new(),
                    pending: false,
                });
                if let Some(d) = &mut self.dialog {
                    d.email.focused = true;
                }
            }
        } else {
            // Draw the login widgets inert behind the dialog.
            let inactive = Input::default();
            let mut ic = ctx(&inactive, c.assets, c.renderer, c.gpu, c.text, now);
            self.email.update(&mut ic);
            self.password.update(&mut ic);
            self.login.update(&mut ic);
            self.exit.update(&mut ic);
            self.new_account.update(&mut ic);
        }
        if self.pending && self.status != "Logging in..." {
            self.pending = false;
        }
        c.text.draw_centered(
            &self.status,
            13,
            win.x + win.w / 2.0,
            win.y + 96.0,
            [214, 190, 148, 255],
        );

        // Account creation dialog (Zircon NewAccountDialog, 300x255 centred).
        if let Some(d) = &mut self.dialog {
            let r = Rect::new(
                ox + (SCENE_W - 300.0) / 2.0,
                oy + (SCENE_H - 255.0) / 2.0,
                300.0,
                255.0,
            );
            let closed = c.window(r, "Account Creation", true);
            d.email.rect = Rect::new(r.x + 85.0, r.y + 45.0, 180.0, 20.0);
            d.password.rect = Rect::new(r.x + 85.0, r.y + 70.0, 136.0, 20.0);
            d.confirm.rect = Rect::new(r.x + 85.0, r.y + 95.0, 136.0, 20.0);
            caption(&mut c, "Email", d.email.rect);
            caption(&mut c, "Password", d.password.rect);
            caption(&mut c, "Confirm", d.confirm.rect);
            cycle_focus(
                &mut [&mut d.email, &mut d.password, &mut d.confirm],
                c.input,
            );
            let e1 = d.email.update(&mut c);
            let e2 = d.password.update(&mut c);
            let e3 = d.confirm.update(&mut c);
            d.email.valid = Some(rules::valid_email(&d.email.text));
            d.password.valid = Some(rules::valid_password(&d.password.text));
            d.confirm.valid =
                Some(d.password.valid == Some(true) && d.confirm.text == d.password.text);
            c.text.draw(
                &format!(
                    "Password: {}-{} characters",
                    rules::PASSWORD_MIN,
                    rules::PASSWORD_MAX
                ),
                12,
                r.x + 20.0,
                r.y + 125.0,
                [160, 160, 160, 255],
            );
            c.text.draw_centered(
                &d.status,
                12,
                r.x + 150.0,
                r.y + 160.0,
                [255, 120, 120, 255],
            );
            d.create.pos = (r.x + 60.0, r.y + 212.0);
            d.cancel.pos = (r.x + 160.0, r.y + 212.0);
            d.create.enabled =
                d.email.valid == Some(true) && d.confirm.valid == Some(true) && !d.pending;
            let submit = (e1 || e2 || e3) && d.create.enabled;
            if d.create.update(&mut c) || submit {
                d.pending = true;
                d.status = "Creating...".into();
                action = SceneAction::Send(ClientMessage::NewAccount {
                    email: d.email.text.clone(),
                    password: d.password.text.clone(),
                });
            }
            if d.cancel.update(&mut c) || closed || c.input.escape {
                self.dialog = None;
            }
        }
        action
    }
}

impl Rect {
    fn default_zero() -> Rect {
        Rect::new(0.0, 0.0, 0.0, 0.0)
    }
}

// ---------------------------------------------------------------------------
// Character select
// ---------------------------------------------------------------------------

/// Preview animation per class/gender (Interface1c): intro then loop.
fn preview_frames(class: Class, gender: Gender) -> ((u32, u32, u64), (u32, u32, u64)) {
    match (class, gender) {
        (Class::Warrior, Gender::Male) => ((240, 22, 2200), (300, 13, 1900)),
        (Class::Warrior, Gender::Female) => ((440, 28, 2800), (500, 13, 1900)),
        (Class::Wizard, Gender::Male) => ((740, 20, 2000), (800, 10, 1500)),
        (Class::Wizard, Gender::Female) => ((940, 26, 2600), (1000, 15, 2250)),
        (Class::Taoist, Gender::Male) => ((1240, 27, 2700), (1300, 15, 2250)),
        (Class::Taoist, Gender::Female) => ((1440, 20, 2000), (1500, 10, 1500)),
        (Class::Assassin, Gender::Male) => ((1740, 25, 2500), (1800, 16, 2400)),
        (Class::Assassin, Gender::Female) => ((1940, 20, 2000), (2000, 10, 1500)),
    }
}

struct NewCharacterDialog {
    name: TextBox,
    class: Class,
    gender: Gender,
    hair: u8,
    class_buttons: [Button; 4],
    gender_buttons: [Button; 2],
    hair_down: Button,
    hair_up: Button,
    create: Button,
    status: String,
    pending: bool,
}

impl NewCharacterDialog {
    fn new() -> NewCharacterDialog {
        let class_buttons = [
            Button::image(INTERFACE1C, 121, 0.0, 0.0),
            Button::image(INTERFACE1C, 126, 0.0, 0.0),
            Button::image(INTERFACE1C, 131, 0.0, 0.0),
            Button::image(INTERFACE1C, 136, 0.0, 0.0),
        ];
        let gender_buttons = [
            Button::image(INTERFACE1C, 116, 0.0, 0.0),
            Button::image(INTERFACE1C, 111, 0.0, 0.0),
        ];
        let mut name = TextBox::new(Rect::default_zero(), rules::NAME_MAX);
        name.focused = true;
        NewCharacterDialog {
            name,
            class: Class::Warrior,
            gender: Gender::Male,
            hair: 1,
            class_buttons,
            gender_buttons,
            hair_down: Button::image(lib::GAME_INTER, 1011, 0.0, 0.0),
            hair_up: Button::image(lib::GAME_INTER, 1010, 0.0, 0.0),
            create: Button::default_style(0.0, 0.0, 80.0, "Create"),
            status: String::new(),
            pending: false,
        }
    }

    fn max_hair(&self) -> u8 {
        match (self.class, self.gender) {
            (Class::Assassin, _) => 5,
            (_, Gender::Male) => 10,
            (_, Gender::Female) => 11,
        }
    }
}

pub struct SelectScene {
    pub status: String,
    characters: Vec<CharacterSummary>,
    selected: Option<usize>,
    start: Button,
    create: Button,
    delete: Button,
    dialog: Option<NewCharacterDialog>,
    /// Pending delete confirmation: (character id, time armed).
    confirm_delete: Option<(u32, u64)>,
    entered: u64,
    preview_start: u64,
    pending_start: bool,
    auto_opened: bool,
}

impl SelectScene {
    pub fn new(mut characters: Vec<CharacterSummary>) -> SelectScene {
        characters.sort_by_key(|c| std::cmp::Reverse(c.last_login));
        SelectScene {
            status: String::new(),
            selected: if characters.is_empty() { None } else { Some(0) },
            characters,
            start: Button::default_style(0.0, 0.0, 80.0, "Start Game"),
            create: Button::default_style(0.0, 0.0, 80.0, "Create"),
            delete: Button::default_style(0.0, 0.0, 80.0, "Delete"),
            dialog: None,
            confirm_delete: None,
            entered: 0,
            preview_start: 0,
            pending_start: false,
            auto_opened: false,
        }
    }

    pub fn character(&self, id: u32) -> Option<&CharacterSummary> {
        self.characters.iter().find(|c| c.id == id)
    }

    pub fn on_new_character(&mut self, r: NewCharacterResult) {
        match r {
            NewCharacterResult::Success { character } => {
                self.characters.insert(0, character);
                self.selected = Some(0);
                self.preview_start = 0;
                self.dialog = None;
                self.status = "Character created.".into();
            }
            NewCharacterResult::Failed { reason } => {
                if let Some(d) = &mut self.dialog {
                    d.pending = false;
                    d.status = reason;
                }
            }
        }
    }

    pub fn on_deleted(&mut self, id: u32, ok: bool, reason: String) {
        if ok {
            self.characters.retain(|c| c.id != id);
            self.selected = if self.characters.is_empty() {
                None
            } else {
                Some(0)
            };
            self.preview_start = 0;
            self.status = "Character deleted.".into();
        } else {
            self.status = reason;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        input: &Input,
        assets: &mut Assets,
        renderer: &mut SpriteRenderer,
        gpu: &Gpu,
        text: &mut TextLayer,
        width: i32,
        height: i32,
        now: u64,
    ) -> SceneAction {
        if self.entered == 0 {
            self.entered = now;
        }
        let (ox, oy) = origin(width, height);
        let mut c = ctx(input, assets, renderer, gpu, text, now);
        let mut action = SceneAction::None;

        // Background (Interface1c 50) and its two blended animations.
        c.draw(INTERFACE1C, 50, ox, oy);
        let entered = self.entered;
        c.anim(INTERFACE1C, 2800, 17, 3_000, ox, oy, false, true, entered);
        c.anim(
            INTERFACE1C,
            2900,
            17,
            3_000,
            ox + 20.0,
            oy + 25.0,
            true,
            true,
            entered,
        );

        // Selected character preview at (450, 200) with offsets.
        if let Some(sel) = self.selected.and_then(|i| self.characters.get(i)) {
            if self.preview_start == 0 {
                self.preview_start = now;
            }
            let ((ib, ic, id), (lb, lc, ld)) = preview_frames(sel.class, sel.gender);
            let elapsed = now.saturating_sub(self.preview_start);
            let (base, frame, overlay) = if elapsed < id {
                (ib, (elapsed * ic as u64 / id.max(1)) as u32, 30)
            } else {
                let t = (elapsed - id) % ld.max(1);
                (lb, (t * lc as u64 / ld.max(1)) as u32, 20)
            };
            let index = base + frame.min(ic.max(lc) - 1);
            c.draw_offset(
                INTERFACE1C,
                index + overlay,
                ox + 450.0,
                oy + 200.0,
                [1.0, 1.0, 1.0, 180.0 / 255.0],
                Blend::Alpha,
            );
            c.draw_offset(
                INTERFACE1C,
                index,
                ox + 450.0,
                oy + 200.0,
                [1.0, 1.0, 1.0, 1.0],
                Blend::Alpha,
            );
        }

        // Select window 320x425 at ((1024/2 - 320)/2, (768-425)/2).
        let win = Rect::new(
            ox + (SCENE_W / 2.0 - 320.0) / 2.0,
            oy + (SCENE_H - 425.0) / 2.0,
            320.0,
            425.0,
        );
        let dialog_open = self.dialog.is_some() || self.confirm_delete.is_some();
        let closed = c.window(win, "Select Character", true);
        if closed && !dialog_open {
            return SceneAction::Send(ClientMessage::Logout);
        }
        // Slot cards.
        for i in 0..rules::MAX_CHARACTERS {
            let card = Rect::new(win.x + 20.0, win.y + 45.0 + i as f32 * 78.0, 280.0, 75.0);
            let selected = self.selected == Some(i);
            let hover = card.contains(c.input.mouse.0, c.input.mouse.1);
            if selected {
                c.fill(card, [72.0 / 255.0, 36.0 / 255.0, 36.0 / 255.0, 1.0]);
                c.draw_strip(lib::INTERFACE, 2, card.x, card.y, card.w);
                c.draw_strip(lib::INTERFACE, 2, card.x, card.y + card.h - 3.0, card.w);
                c.draw_vstrip(lib::INTERFACE, 1, card.x, card.y, card.h);
                c.draw_vstrip(lib::INTERFACE, 1, card.x + card.w - 3.0, card.y, card.h);
                c.draw(lib::INTERFACE, 25, card.x, card.y);
                c.draw(lib::INTERFACE, 26, card.x + card.w - 8.0, card.y);
                c.draw(lib::INTERFACE, 8, card.x, card.y + card.h - 8.0);
                c.draw(
                    lib::INTERFACE,
                    9,
                    card.x + card.w - 8.0,
                    card.y + card.h - 8.0,
                );
            } else {
                c.panel(card, [24, 12, 12, 255]);
            }
            if let Some(ch) = self.characters.get(i) {
                let icon = Rect::new(card.x + 6.0, card.y + 6.0, 64.0, 64.0);
                c.draw(
                    lib::INTERFACE,
                    27 + ch.class.mir_class() as u32,
                    icon.x,
                    icon.y,
                );
                c.border(icon, GOLD);
                let fields = [
                    (
                        "Name",
                        Rect::new(card.x + 135.0, card.y + 8.0, 130.0, 15.0),
                        ch.name.clone(),
                    ),
                    (
                        "Class",
                        Rect::new(card.x + 135.0, card.y + 28.0, 53.0, 15.0),
                        ch.class.name().to_string(),
                    ),
                    (
                        "Level",
                        Rect::new(card.x + 235.0, card.y + 28.0, 30.0, 15.0),
                        ch.level.to_string(),
                    ),
                    (
                        "Location",
                        Rect::new(card.x + 135.0, card.y + 48.0, 130.0, 15.0),
                        if ch.location.is_empty() {
                            "New Character".to_string()
                        } else {
                            ch.location.clone()
                        },
                    ),
                ];
                for (label, r, value) in fields {
                    c.panel(r, [16, 8, 8, 255]);
                    c.text
                        .draw(&value, 11, r.x + 3.0, r.y, [255, 255, 255, 255]);
                    let w = c.text.width(label, 11);
                    c.text
                        .draw(label, 11, r.x - w - 5.0, r.y, [255, 255, 255, 255]);
                }
                if hover && c.input.lmb_pressed && !dialog_open {
                    if self.selected != Some(i) {
                        self.preview_start = 0;
                    }
                    self.selected = Some(i);
                }
            }
        }
        // Buttons at y = 382.
        self.start.pos = (win.x + 25.0, win.y + 382.0);
        self.create.pos = (win.x + 120.0, win.y + 382.0);
        self.delete.pos = (win.x + 215.0, win.y + 382.0);
        let has_sel = self.selected.and_then(|i| self.characters.get(i)).is_some();
        self.start.enabled = has_sel && !dialog_open && !self.pending_start;
        self.delete.enabled = has_sel && !dialog_open;
        self.create.enabled = self.characters.len() < rules::MAX_CHARACTERS && !dialog_open;
        let start_clicked =
            self.start.update(&mut c) || (c.input.enter && !dialog_open && self.start.enabled);
        if start_clicked {
            if let Some(ch) = self.selected.and_then(|i| self.characters.get(i)) {
                self.pending_start = true;
                self.status = format!("Entering the world as {}...", ch.name);
                action = SceneAction::Send(ClientMessage::StartGame { id: ch.id });
            }
        }
        if self.create.update(&mut c)
            || (self.dialog.is_none()
                && !self.auto_opened
                && std::env::var_os("ZIRCON_OPEN_CREATE").is_some())
        {
            self.auto_opened = true;
            self.dialog = Some(NewCharacterDialog::new());
        }
        if self.delete.update(&mut c) {
            if let Some(ch) = self.selected.and_then(|i| self.characters.get(i)) {
                self.confirm_delete = Some((ch.id, now));
            }
        }
        c.text.draw_centered(
            &self.status,
            12,
            win.x + 160.0,
            win.y + 360.0,
            [214, 190, 148, 255],
        );

        // Delete confirmation (5 s arm time like Zircon).
        if let Some((id, armed)) = self.confirm_delete {
            let r = Rect::new(
                ox + (SCENE_W - 360.0) / 2.0,
                oy + (SCENE_H - 140.0) / 2.0,
                360.0,
                140.0,
            );
            let closed = c.window(r, "Delete Character", true);
            let name = self
                .character(id)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            c.text.draw_centered(
                &format!("Permanently delete {name}?"),
                13,
                r.x + 180.0,
                r.y + 45.0,
                [255, 255, 255, 255],
            );
            let wait = 5000u64.saturating_sub(now.saturating_sub(armed));
            let mut yes = Button::default_style(r.x + 90.0, r.y + 97.0, 80.0, "Yes");
            yes.enabled = wait == 0;
            let mut no = Button::default_style(r.x + 190.0, r.y + 97.0, 80.0, "No");
            if wait > 0 {
                c.text.draw_centered(
                    &format!("({}s)", wait.div_ceil(1000)),
                    12,
                    r.x + 180.0,
                    r.y + 70.0,
                    [180, 180, 180, 255],
                );
            }
            if yes.update(&mut c) {
                self.confirm_delete = None;
                action = SceneAction::Send(ClientMessage::DeleteCharacter { id });
            }
            if no.update(&mut c) || closed || c.input.escape {
                self.confirm_delete = None;
            }
        }

        // New character dialog (260x650 centred).
        if let Some(d) = &mut self.dialog {
            let r = Rect::new(
                ox + (SCENE_W - 260.0) / 2.0,
                oy + (SCENE_H - 650.0) / 2.0,
                260.0,
                650.0,
            );
            let closed = c.window(r, "Create Character", true);
            // Class panel.
            let cp = Rect::new(r.x + 30.0, r.y + 40.0, 200.0, 85.0);
            c.panel(cp, [72, 36, 36, 255]);
            c.text
                .draw_centered("Select Class", 13, cp.x + 100.0, cp.y, [255, 255, 255, 255]);
            let bw = 36.0;
            let off = (200.0 - bw * 4.0) / 5.0;
            for (i, b) in d.class_buttons.iter_mut().enumerate() {
                let n = i as f32 + 1.0;
                b.pos = (cp.x + off * n + bw * (n - 1.0), cp.y + 22.0);
                let selected = Class::ALL[i] == d.class;
                b.selected = selected;
                let (sel_idx, un_idx) = [(120, 121), (125, 126), (130, 131), (135, 136)][i];
                b.style = crate::ui::ButtonStyle::Image {
                    library: INTERFACE1C,
                    index: if selected { sel_idx } else { un_idx },
                };
                if b.update(&mut c) {
                    d.class = Class::ALL[i];
                }
            }
            let cl = Rect::new(cp.x + 60.0, cp.y + 65.0, 80.0, 15.0);
            c.panel(cl, [16, 8, 8, 255]);
            c.text
                .draw_centered(d.class.name(), 11, cl.x + 40.0, cl.y, [255, 255, 255, 255]);
            // Gender panel.
            let gp = Rect::new(r.x + 30.0, r.y + 135.0, 200.0, 85.0);
            c.panel(gp, [72, 36, 36, 255]);
            c.text.draw_centered(
                "Select Gender",
                13,
                gp.x + 100.0,
                gp.y,
                [255, 255, 255, 255],
            );
            for (i, b) in d.gender_buttons.iter_mut().enumerate() {
                let n = (i + 2) as f32; // Male at the Wizard slot, Female at the Taoist slot
                b.pos = (gp.x + off * n + bw * (n - 1.0), gp.y + 22.0);
                let g = [Gender::Male, Gender::Female][i];
                let selected = g == d.gender;
                b.selected = selected;
                let (sel_idx, un_idx) = [(115, 116), (110, 111)][i];
                b.style = crate::ui::ButtonStyle::Image {
                    library: INTERFACE1C,
                    index: if selected { sel_idx } else { un_idx },
                };
                if b.update(&mut c) {
                    d.gender = g;
                }
            }
            let gl = Rect::new(gp.x + 60.0, gp.y + 65.0, 80.0, 15.0);
            c.panel(gl, [16, 8, 8, 255]);
            let gname = match d.gender {
                Gender::Male => "Male",
                Gender::Female => "Female",
            };
            c.text
                .draw_centered(gname, 11, gl.x + 40.0, gl.y, [255, 255, 255, 255]);
            // Customization panel.
            let up = Rect::new(r.x + 30.0, r.y + 230.0, 200.0, 330.0);
            c.panel(up, [72, 36, 36, 255]);
            c.text.draw_centered(
                "Customization",
                13,
                up.x + 100.0,
                up.y,
                [255, 255, 255, 255],
            );
            let hair_box = Rect::new(up.x + 90.0 + 19.0, up.y + 25.0 + 1.0, 50.0, 20.0);
            c.panel(hair_box, [16, 8, 8, 255]);
            c.text.draw_centered(
                &d.hair.to_string(),
                11,
                hair_box.x + 25.0,
                hair_box.y + 2.0,
                [255, 255, 255, 255],
            );
            let w = c.text.width("Hair", 12);
            c.text.draw(
                "Hair",
                12,
                up.x + 90.0 - w - 5.0,
                up.y + 27.0,
                [255, 255, 255, 255],
            );
            d.hair_down.pos = (up.x + 90.0, up.y + 25.0 + 1.0 + 3.0);
            d.hair_up.pos = (up.x + 90.0 + 73.0, up.y + 25.0 + 1.0 + 3.0);
            let max_hair = d.max_hair();
            if d.hair > max_hair {
                d.hair = max_hair;
            }
            if d.hair_down.update(&mut c) && d.hair > 1 {
                d.hair -= 1;
            }
            if d.hair_up.update(&mut c) && d.hair < max_hair {
                d.hair += 1;
            }
            // Preview panel with Zircon's ProgUse/Equip composite.
            let pp = Rect::new(up.x + 5.0, up.y + 100.0, 190.0, 225.0);
            c.panel(pp, [49, 40, 24, 255]);
            c.text
                .draw_centered("Preview", 13, pp.x + 95.0, pp.y, [255, 255, 255, 255]);
            let (px, py) = (pp.x + 70.0, pp.y + 160.0);
            let female = d.gender == Gender::Female;
            let assassin = d.class == Class::Assassin;
            let body = if female { 1 } else { 0 };
            let (armour, weapon, hair_base) = match (assassin, female) {
                (false, false) => (941, 1042, 60),
                (false, true) => (951, 1042, 80),
                (true, false) => (2000, 2200, 1100),
                (true, true) => (2010, 2200, 1120),
            };
            let white = [1.0, 1.0, 1.0, 1.0];
            c.draw_offset(PROG_USE, body, px, py, white, Blend::Alpha);
            c.draw_offset(EQUIP, armour, px, py, white, Blend::Alpha);
            c.draw_offset(EQUIP, weapon, px, py, white, Blend::Alpha);
            if d.hair > 0 {
                c.draw_offset(
                    PROG_USE,
                    hair_base + d.hair as u32 - 1,
                    px,
                    py,
                    [0.55, 0.35, 0.15, 1.0],
                    Blend::Alpha,
                );
            }
            // Name and create.
            d.name.rect = Rect::new(r.x + 75.0, r.y + 570.0, 155.0, 20.0);
            caption(&mut c, "Name:", d.name.rect);
            let submit = d.name.update(&mut c);
            d.name.valid = Some(rules::valid_name(&d.name.text));
            d.create.pos = (r.x + 90.0, r.y + 607.0);
            d.create.enabled = d.name.valid == Some(true) && !d.pending;
            c.text.draw_centered(
                &d.status,
                11,
                r.x + 130.0,
                r.y + 595.0,
                [255, 120, 120, 255],
            );
            if d.create.update(&mut c) || (submit && d.create.enabled) {
                d.pending = true;
                d.status = "Creating...".into();
                action = SceneAction::Send(ClientMessage::NewCharacter {
                    name: d.name.text.clone(),
                    class: d.class,
                    gender: d.gender,
                    hair: d.hair,
                });
            }
            if closed || c.input.escape {
                self.dialog = None;
            }
        }
        if self.pending_start && !self.status.starts_with("Entering") {
            self.pending_start = false;
        }
        action
    }
}
