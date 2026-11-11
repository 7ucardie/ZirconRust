//! Player chat (Zircon `PlayerObject.Chat`): `/name text` whispers, `!!`
//! group, `!@` global (level 33, 30 s), `!` shout (level 2, 10 s, whole
//! map), otherwise local talk to players in view range with an overhead
//! bubble.

use super::*;
use mir_proto::ChatKind;

/// Zircon `Config.ShoutDelay`.
const SHOUT_DELAY: u64 = 10_000;
const GLOBAL_SHOUT_DELAY: u64 = 30_000;

impl World {
    pub fn set_chat_bans(&mut self, id: ObjectId, chat_until: u64, global_until: u64) {
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.chat_ban_until = chat_until;
            p.global_ban_until = global_until;
        }
    }

    pub fn chat(&mut self, id: ObjectId, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        let now = crate::accounts::now_secs();
        if p.chat_ban_until > now {
            self.send_to(id, say(None, ChatKind::System, "You are muted.".into()));
            return;
        }
        if text.starts_with("!@") && p.global_ban_until > now {
            self.send_to(
                id,
                say(
                    None,
                    ChatKind::System,
                    "Global chat is muted for this account.".into(),
                ),
            );
            return;
        }
        let (name, level, map, loc) = (p.name.clone(), p.level, o.map, o.location);
        tracing::info!(target: "chat", "{name}: {text}");

        if let Some(command) = text.strip_prefix('@') {
            if command.eq_ignore_ascii_case("event") {
                self.world_event_status(id);
            } else if let Some(level) = command
                .to_ascii_lowercase()
                .strip_prefix("enablelevel")
                .and_then(|level| level.parse::<i32>().ok())
            {
                self.companion_skill_toggle(id, level);
            } else {
                self.gm_chat(id, command);
            }
        } else if let Some(rest) = text.strip_prefix('/') {
            // Whisper: "/name message".
            let mut parts = rest.splitn(2, ' ');
            let target_name = parts.next().unwrap_or("").to_string();
            let message = parts.next().unwrap_or("").trim().to_string();
            if target_name.is_empty() {
                return;
            }
            // Zircon answers a blocked whisper with "cannot find player",
            // so the sender never learns they were blocked.
            let my_account = self.objects[&id].player().unwrap().account;
            let target = self
                .players()
                .find(|o| {
                    o.player()
                        .is_some_and(|p| p.name.eq_ignore_ascii_case(&target_name))
                })
                .filter(|o| {
                    o.player()
                        .is_some_and(|p| !self.social_store.is_blocking(my_account, p.account))
                })
                .map(|o| o.id);
            match target {
                Some(t) => {
                    self.send_to(id, say(None, ChatKind::WhisperOut, format!("/{rest}")));
                    self.send_to(
                        t,
                        say(None, ChatKind::WhisperIn, format!("{name}=> {message}")),
                    );
                }
                None => self.send_to(
                    id,
                    say(
                        None,
                        ChatKind::System,
                        format!("Could not find {target_name}."),
                    ),
                ),
            }
        } else if let Some(rest) = text.strip_prefix("!!") {
            let members = self.group_members(id);
            if members.is_empty() {
                return;
            }
            let line = format!("{name}: {}", rest.trim_start());
            for m in members {
                self.send_to(m, say(None, ChatKind::Group, line.clone()));
            }
        } else if let Some(rest) = text.strip_prefix("!~") {
            let members = self.guild_members_online(id);
            if members.is_empty() {
                return;
            }
            let line = format!("{name}: {}", rest.trim_start());
            for m in members {
                self.send_to(m, say(None, ChatKind::Guild, line.clone()));
            }
        } else if let Some(rest) = text.strip_prefix("!@") {
            let p = self.objects[&id].player().unwrap();
            if self.now < p.global_shout_time {
                let wait = p.global_shout_time.saturating_sub(self.now).div_ceil(1000);
                self.send_to(
                    id,
                    say(
                        None,
                        ChatKind::System,
                        format!("You cannot global shout for another {wait} seconds."),
                    ),
                );
                return;
            }
            if level < 33 {
                self.send_to(
                    id,
                    say(
                        None,
                        ChatKind::System,
                        "You need to be level 33 to global shout.".into(),
                    ),
                );
                return;
            }
            self.objects
                .get_mut(&id)
                .and_then(|o| o.player_mut())
                .unwrap()
                .global_shout_time = self.now + GLOBAL_SHOUT_DELAY;
            let line = format!("(!@){name}: {}", rest.trim_start());
            let ids: Vec<ObjectId> = self.players().map(|o| o.id).collect();
            for t in ids {
                self.send_to(t, say(None, ChatKind::Global, line.clone()));
            }
        } else if let Some(rest) = text.strip_prefix('!') {
            let p = self.objects[&id].player().unwrap();
            if self.now < p.shout_time {
                let wait = p.shout_time.saturating_sub(self.now).div_ceil(1000);
                self.send_to(
                    id,
                    say(
                        None,
                        ChatKind::System,
                        format!("You cannot shout for another {wait} seconds."),
                    ),
                );
                return;
            }
            if level < 2 {
                self.send_to(
                    id,
                    say(
                        None,
                        ChatKind::System,
                        "You need to be level 2 to shout.".into(),
                    ),
                );
                return;
            }
            self.objects
                .get_mut(&id)
                .and_then(|o| o.player_mut())
                .unwrap()
                .shout_time = self.now + SHOUT_DELAY;
            let line = format!("(!){name}: {}", rest.trim_start());
            let ids: Vec<ObjectId> = self
                .on_map(map)
                .filter(|o| o.player().is_some())
                .map(|o| o.id)
                .collect();
            for t in ids {
                self.send_to(t, say(None, ChatKind::Shout, line.clone()));
            }
        } else {
            let line = format!("{name}: {text}");
            let ids: Vec<ObjectId> = self
                .on_map(map)
                .filter(|o| o.player().is_some() && o.location.distance(loc) <= MAX_VIEW_RANGE)
                .map(|o| o.id)
                .collect();
            for t in ids {
                self.send_to(t, say(Some(id), ChatKind::Normal, line.clone()));
            }
        }
    }
}

fn say(id: Option<ObjectId>, kind: ChatKind, text: String) -> ServerMessage {
    ServerMessage::Say { id, kind, text }
}
