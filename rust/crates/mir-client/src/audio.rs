//! Sound playback (rodio) keyed by Zircon `SoundIndex`.
//!
//! The index -> file table comes from `sound_table` (generated from the C#
//! client). Files are resolved case-insensitively under `Sound/`, missing
//! files are silent. Each index may overlap up to five times; looping
//! clips (music, fire wall hum) are singletons that `stop` ends. Channels
//! (System, Music, Magic, Monster, Player) have their own volume.
//!
//! Without an output device (CI, headless) or with `ZIRCON_MUTE=1` every
//! call is a no-op, so the client never depends on audio working.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

use crate::sound_table::{sound_file, CHANNELS};

/// Most simultaneous copies of one index (Zircon `DXSound` overlap cap).
const OVERLAP: usize = 5;
/// Global cap on one-shot voices.
const MAX_VOICES: usize = 32;

pub struct Audio {
    sink: Option<MixerDeviceSink>,
    /// Lower-cased file name -> real path under `Sound/`.
    files: HashMap<String, PathBuf>,
    cache: HashMap<u16, Option<Arc<[u8]>>>,
    /// One-shot voices: (index, player).
    voices: Vec<(u16, Player)>,
    /// Looping singletons by index.
    loops: HashMap<u16, Player>,
    music: Option<u16>,
    /// Volume per channel, 0..1 (Zircon: 0..100 per `SoundType`).
    volumes: [f32; CHANNELS],
    preferences: crate::settings::Preferences,
    gain: f32,
}

impl Audio {
    /// `root` is the client root holding `Sound/`.
    pub fn new(root: impl AsRef<Path>) -> Audio {
        let muted = std::env::var_os("ZIRCON_MUTE").is_some();
        let sink = if muted {
            None
        } else {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut s) => {
                    s.log_on_drop(false);
                    Some(s)
                }
                Err(e) => {
                    tracing::warn!("no audio output: {e}");
                    None
                }
            }
        };
        let dir = root.as_ref().join("Sound");
        let mut files = HashMap::new();
        if sink.is_some() {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    files.insert(e.file_name().to_string_lossy().to_lowercase(), e.path());
                }
            }
            tracing::info!(count = files.len(), dir = %dir.display(), "sound files");
        }
        let preferences = crate::settings::Preferences::load();
        let gain = std::env::var("ZIRCON_VOLUME")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(1.0)
            .clamp(0.0, 2.0);
        let volumes = channel_volumes(&preferences, gain);
        Audio {
            sink,
            files,
            cache: HashMap::new(),
            voices: Vec::new(),
            loops: HashMap::new(),
            music: None,
            volumes,
            preferences,
            gain,
        }
    }

    /// Apply preferences to current voices as well as future playback.
    pub fn set_preferences(&mut self, preferences: &crate::settings::Preferences) {
        if self.preferences == *preferences {
            return;
        }
        self.preferences = preferences.clone();
        self.volumes = channel_volumes(preferences, self.gain);
        for (index, player) in self
            .voices
            .iter()
            .map(|(i, p)| (*i, p))
            .chain(self.loops.iter().map(|(i, p)| (*i, p)))
        {
            if let Some((_, channel, _)) = sound_file(index) {
                player.set_volume(self.volumes[(channel as usize).min(CHANNELS - 1)]);
            }
        }
    }

    fn bytes(&mut self, index: u16, file: &str) -> Option<Arc<[u8]>> {
        if let Some(b) = self.cache.get(&index) {
            return b.clone();
        }
        let loaded = self
            .files
            .get(&file.to_lowercase())
            .and_then(|p| std::fs::read(p).ok())
            .map(Arc::<[u8]>::from);
        if loaded.is_none() {
            tracing::debug!(index, file, "missing sound file");
        }
        self.cache.insert(index, loaded.clone());
        loaded
    }

    /// Play a sound by `SoundIndex`; looping indices become singletons.
    pub fn play(&mut self, index: u16) {
        if index == 0 || self.sink.is_none() {
            return;
        }
        let Some((file, channel, looping)) = sound_file(index) else {
            return;
        };
        self.voices.retain(|(_, p)| !p.empty());
        self.loops.retain(|_, p| !p.empty());
        if looping && self.loops.contains_key(&index) {
            return;
        }
        if !looping && self.voices.iter().filter(|(i, _)| *i == index).count() >= OVERLAP {
            return;
        }
        let Some(bytes) = self.bytes(index, file) else {
            return;
        };
        let Ok(source) = Decoder::new_wav(Cursor::new(bytes)) else {
            return;
        };
        let sink = self.sink.as_ref().unwrap();
        let player = Player::connect_new(sink.mixer());
        player.set_volume(self.volumes[(channel as usize).min(CHANNELS - 1)]);
        if looping {
            player.append(source.repeat_infinite());
            self.loops.insert(index, player);
        } else {
            player.append(source);
            if self.voices.len() >= MAX_VOICES {
                let (_, old) = self.voices.remove(0);
                old.stop();
            }
            self.voices.push((index, player));
        }
    }

    /// Stop a looping sound (no-op for one-shots).
    pub fn stop(&mut self, index: u16) {
        if let Some(p) = self.loops.remove(&index) {
            p.stop();
        }
    }

    /// True while a looping index is playing.
    pub fn is_looping(&self, index: u16) -> bool {
        self.loops.get(&index).is_some_and(|p| !p.empty())
    }

    /// Switch the music track (`MapInfo.Music`, login/select scenes);
    /// re-requesting the playing track is a no-op, 0 stops it.
    pub fn play_music(&mut self, index: u16) {
        if self.music == Some(index) {
            return;
        }
        if let Some(old) = self.music.take() {
            self.stop(old);
        }
        if index != 0 {
            self.music = Some(index);
            self.play(index);
        }
    }

    /// Stop every loop (leaving the world).
    pub fn stop_all(&mut self) {
        for (_, p) in self.loops.drain() {
            p.stop();
        }
        self.music = None;
    }
}

fn channel_volumes(preferences: &crate::settings::Preferences, gain: f32) -> [f32; CHANNELS] {
    let mut volumes = [0.0; CHANNELS];
    if !preferences.muted {
        for (i, percent) in preferences.volumes.iter().enumerate() {
            volumes[i + 1] = f32::from((*percent).min(100)) / 100.0 * gain;
        }
    }
    volumes
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_settings_updates_active_loops_and_voices_without_audio_device() {
        let (mixer, _source) =
            rodio::mixer::mixer(2.try_into().unwrap(), 44100.try_into().unwrap());
        let music = Player::connect_new(&mixer);
        let system = Player::connect_new(&mixer);
        let system_index = (1..1000)
            .find(|index| sound_file(*index).is_some_and(|(_, channel, _)| channel == 1))
            .unwrap();
        let preferences = crate::settings::Preferences::default();
        let mut audio = Audio {
            sink: None,
            files: HashMap::new(),
            cache: HashMap::new(),
            voices: vec![(system_index, system)],
            loops: HashMap::from([(1, music)]),
            music: Some(1),
            volumes: channel_volumes(&preferences, 0.5),
            preferences,
            gain: 0.5,
        };
        let mut changed = crate::settings::Preferences {
            volumes: [80, 20, 50, 50, 50],
            muted: false,
            resolution: None,
            show_fps: false,
        };
        audio.set_preferences(&changed);
        assert!((audio.loops[&1].volume() - 0.1).abs() < 0.0001);
        assert!((audio.voices[0].1.volume() - 0.4).abs() < 0.0001);
        changed.muted = true;
        audio.set_preferences(&changed);
        assert_eq!(audio.loops[&1].volume(), 0.0);
        assert_eq!(audio.voices[0].1.volume(), 0.0);
    }
}
