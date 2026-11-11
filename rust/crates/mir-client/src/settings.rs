//! Persistent sound and display preferences and their native settings window.
use crate::ui::{Button, Ctx, Rect, GOLD};
use crate::windows::WindowLayout;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub volumes: [u8; 5],
    pub muted: bool,
    #[serde(default)]
    pub resolution: Option<(u32, u32)>,
    #[serde(default)]
    pub show_fps: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            volumes: [50, 30, 50, 50, 50],
            muted: false,
            resolution: None,
            show_fps: false,
        }
    }
}
fn path() -> Option<PathBuf> {
    Some(
        std::env::current_exe()
            .ok()?
            .with_file_name("client-settings.json"),
    )
}
impl Preferences {
    pub fn load() -> Self {
        path().map_or_else(Self::default, |path| Self::load_from(&path))
    }
    fn load_from(path: &Path) -> Self {
        std::fs::metadata(path)
            .ok()
            .filter(|metadata| metadata.len() <= 4096)
            .and_then(|_| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|settings| {
                settings.volumes.iter().all(|value| *value <= 100)
                    && settings.resolution.is_none_or(valid_resolution)
            })
            .unwrap_or_default()
    }
    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if self.volumes.iter().any(|value| *value > 100)
            || self.resolution.is_some_and(|size| !valid_resolution(size))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid settings value",
            ));
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(temporary, path)
    }
}

pub fn valid_resolution((width, height): (u32, u32)) -> bool {
    (640..=8192).contains(&width) && (480..=4320).contains(&height)
}

const RESOLUTIONS: &[(u32, u32)] = &[
    (800, 600),
    (1024, 768),
    (1280, 720),
    (1366, 768),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
    (3440, 1440),
    (3840, 2160),
    (5120, 1440),
];

pub struct Settings {
    pub open: bool,
    pub preferences: Preferences,
    pub pending_resolution: Option<(u32, u32)>,
    buttons: Vec<Button>,
    notice: String,
    environment: String,
    layout: WindowLayout,
}
impl Default for Settings {
    fn default() -> Self {
        let environment = if std::env::var_os("ZIRCON_MUTE").is_some() {
            "Sound disabled by ZIRCON_MUTE.".into()
        } else if let Some(gain) = std::env::var("ZIRCON_VOLUME")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite())
        {
            format!("Environment volume multiplier: {}", gain.clamp(0.0, 2.0))
        } else {
            String::new()
        };
        Self {
            open: false,
            preferences: Preferences::load(),
            pending_resolution: None,
            buttons: vec![],
            notice: String::new(),
            environment,
            layout: WindowLayout::default(),
        }
    }
}
impl Settings {
    pub fn draw(&mut self, c: &mut Ctx, width: i32, height: i32) -> bool {
        if !self.open {
            return false;
        }
        if c.input.escape {
            self.open = false;
            return false;
        }
        let win = self.layout.place(
            "settings",
            Rect::new(
                (width as f32 - 380.0) / 2.0,
                (height as f32 - 365.0) / 2.0,
                380.0,
                365.0,
            ),
            c.input,
            (width, height),
        );
        if c.window(win, "Settings", false) && !self.layout.is_dragging() {
            self.open = false;
        }
        if self.buttons.is_empty() {
            for _ in 0..5 {
                self.buttons
                    .push(Button::default_style(0.0, 0.0, 32.0, "-"));
                self.buttons
                    .push(Button::default_style(0.0, 0.0, 32.0, "+"));
            }
            for label in ["Mute", "Save", "Reset"] {
                self.buttons
                    .push(Button::default_style(0.0, 0.0, 100.0, label));
            }
            self.buttons
                .push(Button::default_style(0.0, 0.0, 32.0, "-"));
            self.buttons
                .push(Button::default_style(0.0, 0.0, 32.0, "+"));
            self.buttons
                .push(Button::default_style(0.0, 0.0, 100.0, "Show FPS"));
        }
        for (i, name) in ["System", "Music", "Magic", "Monsters", "Players"]
            .into_iter()
            .enumerate()
        {
            let y = win.y + 38.0 + i as f32 * 31.0;
            c.text.draw(name, 13, win.x + 20.0, y, GOLD);
            c.text.draw(
                &format!("{}%", self.preferences.volumes[i]),
                13,
                win.x + 223.0,
                y,
                [230, 230, 230, 255],
            );
            for (offset, delta) in [(0, -5i16), (1, 5)] {
                let button = &mut self.buttons[i * 2 + offset];
                button.pos = (win.x + 180.0 + offset as f32 * 105.0, y - 3.0);
                if button.update(c) {
                    self.preferences.volumes[i] =
                        (i16::from(self.preferences.volumes[i]) + delta).clamp(0, 100) as u8;
                    self.notice = "Changed. Save to keep after restart.".into();
                }
            }
        }
        let resolution = self
            .preferences
            .resolution
            .unwrap_or((width as u32, height as u32));
        let current_index = RESOLUTIONS.iter().position(|size| *size == resolution);
        c.text
            .draw("Resolution", 13, win.x + 20.0, win.y + 204.0, GOLD);
        c.text.draw(
            &format!("{}x{}", resolution.0, resolution.1),
            13,
            win.x + 192.0,
            win.y + 204.0,
            [230, 230, 230, 255],
        );
        for (offset, step) in [(0, -1isize), (1, 1)] {
            let button = &mut self.buttons[13 + offset];
            button.pos = (win.x + 145.0 + offset as f32 * 155.0, win.y + 201.0);
            if button.update(c) {
                let index = current_index.unwrap_or(1) as isize;
                let next = (index + step).clamp(0, RESOLUTIONS.len() as isize - 1) as usize;
                let size = RESOLUTIONS[next];
                self.preferences.resolution = Some(size);
                self.pending_resolution = Some(size);
                self.notice = "Resolution changed. Save to keep after restart.".into();
            }
        }
        for i in 0..3 {
            let button = &mut self.buttons[10 + i];
            button.pos = (win.x + 20.0 + i as f32 * 112.0, win.y + 244.0);
            button.selected = i == 0 && self.preferences.muted;
            if i == 0 {
                button.label = Some(
                    if self.preferences.muted {
                        "Unmute"
                    } else {
                        "Mute"
                    }
                    .into(),
                );
            }
            if button.update(c) {
                match i {
                    0 => {
                        self.preferences.muted = !self.preferences.muted;
                        self.notice = "Changed. Save to keep after restart.".into();
                    }
                    1 => {
                        self.notice =
                            match path().and_then(|path| self.preferences.save_to(&path).ok()) {
                                Some(()) => "Settings saved.",
                                None => "Could not save settings.",
                            }
                            .into();
                    }
                    _ => {
                        self.preferences = Preferences::default();
                        self.pending_resolution = Some((1024, 768));
                        self.notice = "Defaults restored. Save to keep.".into();
                    }
                }
            }
        }
        let button = &mut self.buttons[15];
        button.pos = (win.x + 20.0, win.y + 278.0);
        button.label = Some(if self.preferences.show_fps {
            "Hide FPS".into()
        } else {
            "Show FPS".into()
        });
        button.selected = self.preferences.show_fps;
        if button.update(c) {
            self.preferences.show_fps = !self.preferences.show_fps;
            self.notice = "Changed. Save to keep after restart.".into();
        }
        c.text.draw(
            &self.notice,
            11,
            win.x + 20.0,
            win.y + 309.0,
            [230, 230, 230, 255],
        );
        c.text
            .draw(&self.environment, 11, win.x + 20.0, win.y + 334.0, GOLD);
        let over = win.contains(c.input.mouse.0, c.input.mouse.1) || self.layout.is_dragging();
        self.layout.end_frame(c.input);
        over
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_round_trip_and_reject_invalid_saved_volume() {
        let path =
            std::env::temp_dir().join(format!("zircon-settings-{}.json", std::process::id()));
        let settings = Preferences {
            volumes: [0, 10, 50, 90, 100],
            muted: true,
            resolution: Some((1920, 1080)),
            show_fps: true,
        };
        settings.save_to(&path).unwrap();
        assert_eq!(Preferences::load_from(&path), settings);
        std::fs::write(&path, br#"{"volumes":[50,30,50,50,50],"muted":false}"#).unwrap();
        assert_eq!(Preferences::load_from(&path), Preferences::default());
        std::fs::write(&path, br#"{"volumes":[101,10,50,90,100],"muted":false}"#).unwrap();
        assert_eq!(Preferences::load_from(&path), Preferences::default());
        std::fs::write(
            &path,
            br#"{"volumes":[50,30,50,50,50],"muted":false,"resolution":[99999,1080]}"#,
        )
        .unwrap();
        assert_eq!(Preferences::load_from(&path), Preferences::default());
        std::fs::write(&path, b"invalid").unwrap();
        assert_eq!(Preferences::load_from(&path), Preferences::default());
        std::fs::remove_file(&path).unwrap();
    }
}
