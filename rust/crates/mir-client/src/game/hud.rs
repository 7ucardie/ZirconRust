use super::*;

fn xp_bar_fill(progress: f32, width: f32) -> f32 {
    if progress > 0.0 {
        (progress.clamp(0.0, 1.0) * width).max(3.0).min(width)
    } else {
        0.0
    }
}

fn short_exp(value: u64) -> String {
    if value >= 1_000_000_000 {
        format!("{:.2}B", value as f64 / 1_000_000_000.0)
    } else if value >= 1_000_000 {
        format!("{:.2}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

impl Game {
    /// Inventory, character and NPC windows on top of everything; consumes
    /// clicks over them.
    pub(super) fn draw_windows(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        text: &mut TextLayer,
        width: i32,
        height: i32,
        now: u64,
    ) {
        let mut out = Vec::new();
        let over = {
            let Game {
                assets,
                windows,
                inventory,
                equipment,
                gold,
                weights,
                stats,
                catalog,
                input,
                character,
                magics,
                quests,
                toggles,
                cooldowns,
                belt,
                use_item_time,
                objects,
                user,
                group,
                allow_group,
                group_invite,
                storage,
                trade,
                trade_request,
                guild,
                guild_invite,
                mail,
                partner,
                wedding_ring,
                marriage_invite,
                refines,
                companions,
                companion_shop,
                currencies,
                castles,
                conquest,
                ranking,
                friends,
                blocks,
                online_state,
                friends_window,
                wardrobe,
                ..
            } = self;
            let player_name = character
                .as_ref()
                .map(|c| c.name.clone())
                .unwrap_or_default();
            let dead = user
                .and_then(|u| objects.get(&u))
                .map(|o| o.dead)
                .unwrap_or(false);
            let mut lower_input = input.clone();
            if wardrobe.contains_mouse(input.mouse, width, height) {
                lower_input.lmb_pressed = false;
                lower_input.lmb_released = false;
                lower_input.rmb_pressed = false;
                lower_input.wheel = 0.0;
            }
            let mut c = Ctx {
                input: &lower_input,
                assets,
                renderer,
                gpu,
                text,
                now,
            };
            let view = crate::windows::PlayerView {
                level: stats.level,
                class: character.as_ref().map(|c| c.class.mir_class()).unwrap_or(0),
                hp: stats.hp,
                max_hp: stats.max_hp,
                mp: stats.mp,
                max_mp: stats.max_mp,
                min_dc: stats.min_dc,
                max_dc: stats.max_dc,
                min_ac: stats.min_ac,
                max_ac: stats.max_ac,
                min_mc: stats.min_mc,
                max_mc: stats.max_mc,
                min_sc: stats.min_sc,
                max_sc: stats.max_sc,
                accuracy: stats.accuracy,
                agility: stats.agility,
                fame_title: stats.fame_title.clone(),
            };
            let bag = Bag {
                gender: character
                    .as_ref()
                    .map(|c| c.gender)
                    .unwrap_or(mir_proto::Gender::Male),
                hair: character.as_ref().map(|c| c.hair).unwrap_or(0),
                wardrobe_item: wardrobe.chosen,
                wardrobe_helmet: wardrobe.chosen_slot(mir_proto::slot::HELMET),
                wardrobe_weapon: wardrobe.chosen_slot(mir_proto::slot::WEAPON),
                wardrobe_shoes: wardrobe.chosen_slot(mir_proto::slot::SHOES),
                inventory,
                equipment,
                gold: *gold,
                weights,
                stats: &view,
                catalog,
                magics,
                toggles,
                cooldowns,
                belt,
                use_item_time: *use_item_time,
                dead,
                quests,
                player_name: &player_name,
                group,
                allow_group: *allow_group,
                group_invite: group_invite.as_deref(),
                user: *user,
                storage,
                trade: trade.as_ref(),
                trade_request: trade_request.as_deref(),
                guild: guild.as_ref(),
                guild_invite: guild_invite.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
                mail,
                partner: partner.as_deref(),
                wedding_ring: *wedding_ring,
                marriage_invite: marriage_invite.as_deref(),
                refines,
                companions,
                companion_shop,
                currencies,
                castles,
                conquest: *conquest,
                ranking,
            };
            let over = windows.draw(&mut c, &bag, width, height, &mut out);
            // Its own window, drawn after the rest so it sits on top.
            let over_friends = friends_window.draw(
                &mut c,
                friends,
                blocks,
                *online_state,
                width,
                height,
                &mut out,
            );
            c.input = input;
            if std::mem::take(&mut windows.toggle_wardrobe) {
                wardrobe.open = !wardrobe.open;
            }
            let over_wardrobe = wardrobe.draw(
                &mut c,
                catalog,
                inventory,
                equipment,
                bag.storage,
                bag.gender,
                width,
                height,
                &mut out,
            );
            over || over_friends || over_wardrobe
        };
        self.audio
            .set_preferences(&self.windows.settings.preferences);
        if std::mem::take(&mut self.windows.toggle_friends) {
            self.friends_window.open = !self.friends_window.open;
        }
        // Whisper pre-fills the chat bar instead of sending an empty line.
        if let Some(name) = self.friends_window.whisper_to.take() {
            self.chat.open = true;
            self.chat.bar.text = format!("/{name} ");
            self.chat.bar.focused = true;
        }
        // Belt links and item uses decided inside the windows go through the
        // same local bookkeeping as the keyboard paths.
        let mut kept = Vec::with_capacity(out.len());
        for m in out {
            match m {
                ClientMessage::BeltLink { slot, info, item } => {
                    self.apply_belt_link(BeltLink { slot, info, item });
                    kept.push(ClientMessage::BeltLink { slot, info, item });
                }
                ClientMessage::MagicKey { magic, key } => {
                    self.apply_magic_key(magic, key);
                    kept.push(ClientMessage::MagicKey { magic, key });
                }
                ClientMessage::ItemUse { slot } => {
                    let consumable = self
                        .inventory
                        .get(slot as usize)
                        .cloned()
                        .flatten()
                        .and_then(|i| self.catalog.get(i.info).cloned())
                        .filter(|d| d.item_type == mir_proto::item_type::CONSUMABLE);
                    if let Some(def) = consumable {
                        if now < self.use_item_time {
                            continue;
                        }
                        self.use_item_time = now + use_item_lock(def.durability);
                    }
                    self.sfx_item_slot(Grid::Inventory, slot);
                    kept.push(ClientMessage::ItemUse { slot });
                }
                ClientMessage::ItemMove {
                    from, from_slot, ..
                } => {
                    self.sfx_item_slot(from, from_slot);
                    kept.push(m);
                }
                ClientMessage::NpcButton { .. } => {
                    self.audio.play(sound_table::idx::BUTTON_C);
                    kept.push(m);
                }
                ClientMessage::GroupResponse { accept } => {
                    self.group_invite = None;
                    kept.push(ClientMessage::GroupResponse { accept });
                }
                ClientMessage::TradeResponse { accept } => {
                    self.trade_request = None;
                    kept.push(ClientMessage::TradeResponse { accept });
                }
                // The server stores the flag silently, so show it at once.
                ClientMessage::QuestTrack { quest, track } => {
                    if let Some(q) = self.quests.iter_mut().find(|q| q.quest == quest) {
                        q.track = track;
                    }
                    kept.push(ClientMessage::QuestTrack { quest, track });
                }
                ClientMessage::GuildResponse { accept } => {
                    self.guild_invite = None;
                    kept.push(ClientMessage::GuildResponse { accept });
                }
                ClientMessage::MarriageResponse { accept } => {
                    self.marriage_invite = None;
                    kept.push(ClientMessage::MarriageResponse { accept });
                }
                ClientMessage::MailOpened { index } => {
                    if let Some(m) = self.mail.iter_mut().find(|m| m.index == index) {
                        m.opened = true;
                    }
                    kept.push(ClientMessage::MailOpened { index });
                }
                ClientMessage::TradeConfirm => {
                    if let Some(t) = &mut self.trade {
                        t.confirmed = true;
                    }
                    kept.push(ClientMessage::TradeConfirm);
                }
                other => kept.push(other),
            }
        }
        let out = kept;
        self.windows_open_last_frame = self.windows.settings.open
            || self.friends_window.open
            || self.windows.inventory_open
            || self.windows.character_open
            || self.wardrobe.open
            || self.windows.skills_open
            || self.windows.group_open
            || self.windows.storage_open
            || self.windows.guild_open
            || self.windows.mail_open
            || self.windows.companion_open
            || self.windows.ranking.open
            || self.windows.market.open
            || self.windows.menu.any_open()
            || self.minimap.big_open
            || self.trade.is_some()
            || self.windows.npc.is_some();
        self.ui_hover_last_frame = over
            || self.chat.contains_mouse(self.input.mouse)
            || (self.map.is_some() && self.minimap.contains_mouse(self.input.mouse, width, height));
        self.pending_messages.extend(out);
        if over {
            // Swallow world input while the mouse is over a window.
            self.lmb = false;
            self.rmb = false;
            self.input.lmb_pressed = false;
            self.input.rmb_pressed = false;
            self.hovered = None;
        }
    }

    /// Overlays that windows may cover (Zircon draws them as their own
    /// dialogs): the target panel, the tracked quests and the tooltip of the
    /// buff icon under the cursor. Drawn in the world layer, before the HUD
    /// and the windows, so an open window hides them.
    pub(super) fn draw_overlays(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        text: &mut TextLayer,
        width: i32,
        now: u64,
    ) {
        let buffs = self.buffs.clone();
        let target = self
            .target
            .and_then(|id| self.objects.get(&id).filter(|o| o.name_visible()))
            .map(|o| crate::overlay::TargetInfo {
                name: o.display_name(),
                hp: o.hp,
                max_hp: o.max_hp,
                poisoned: o.poison_kinds & 3 != 0,
                buffs: o.visible_buffs.clone(),
            });
        let tracked = crate::overlay::tracker_lines(&self.quests, &self.catalog);
        let mouse = self.mouse;
        {
            let mut c = Ctx {
                input: &self.input,
                assets: &mut self.assets,
                renderer,
                gpu,
                text,
                now,
            };
            if let Some(t) = &target {
                crate::overlay::draw_target_panel(&mut c, t);
            }
            crate::overlay::draw_quest_tracker(&mut c, &tracked, width, mouse);
            crate::overlay::draw_buff_tooltip(&mut c, &buffs, width, mouse, now);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_hud(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        text: &mut TextLayer,
        width: i32,
        height: i32,
        now: u64,
        fps: f32,
    ) {
        let white = [1.0, 1.0, 1.0, 1.0];
        if let Some(panel) = Self::sprite(
            &mut self.assets,
            renderer,
            gpu,
            lib::GAME_INTER,
            50,
            Surface::Image,
        ) {
            let px = (width - panel.width as i32) / 2;
            let py = height - panel.height as i32;
            renderer.draw(panel, px as f32, py as f32, white, Blend::Alpha);
            let hp_pct = self.stats.hp.max(0) as f32 / self.stats.max_hp.max(1) as f32;
            let mp_pct = self.stats.mp.max(0) as f32 / self.stats.max_mp.max(1) as f32;
            if let Some(hp) = Self::sprite(
                &mut self.assets,
                renderer,
                gpu,
                lib::GAME_INTER,
                52,
                Surface::Image,
            ) {
                renderer.draw_cropped(hp, (px + 35) as f32, (py + 22) as f32, hp_pct, white);
            }
            if let Some(mp) = Self::sprite(
                &mut self.assets,
                renderer,
                gpu,
                lib::GAME_INTER,
                54,
                Surface::Image,
            ) {
                renderer.draw_cropped(mp, (px + 35) as f32, (py + 36) as f32, mp_pct, white);
            }
            if let Some(frame) = Self::sprite(
                &mut self.assets,
                renderer,
                gpu,
                lib::GAME_INTER,
                51,
                Surface::Image,
            ) {
                let fx = px + (panel.width as i32 - frame.width as i32) / 2 + 1;
                renderer.draw(frame, fx as f32, (py + 3) as f32, white, Blend::Alpha);
            }
            let xp_progress = (self.stats.experience as f64
                / self.stats.max_experience.max(1) as f64)
                .clamp(0.0, 1.0) as f32;
            let xp_width = (panel.width as f32 - 24.0).max(0.0);
            renderer.fill_rect(
                (px + 12) as f32,
                (py + 13) as f32,
                xp_width,
                3.0,
                [0.08, 0.04, 0.01, 0.95],
            );
            let xp_fill = xp_bar_fill(xp_progress, xp_width);
            if xp_fill > 0.0 {
                renderer.fill_rect(
                    (px + 12) as f32,
                    (py + 13) as f32,
                    xp_fill,
                    3.0,
                    [0.95, 0.69, 0.14, 1.0],
                );
            }
            let stats = [
                ("AC", format!("{}-{}", self.stats.min_ac, self.stats.max_ac)),
                ("MR", format!("{}-{}", self.stats.min_mr, self.stats.max_mr)),
                ("DC", format!("{}-{}", self.stats.min_dc, self.stats.max_dc)),
                ("MC", format!("{}-{}", self.stats.min_mc, self.stats.max_mc)),
                ("SC", format!("{}-{}", self.stats.min_sc, self.stats.max_sc)),
                ("Acc", self.stats.accuracy.to_string()),
                ("Agi", self.stats.agility.to_string()),
                ("AS", self.stats.attack_speed.to_string()),
            ];
            for (index, (label, value)) in stats.iter().enumerate() {
                let x = (px + 302 + (index % 4) as i32 * 85) as f32;
                let y = (py + 23 + (index / 4) as i32 * 20) as f32;
                let value_x = x + text.width(label, 10) + 3.0;
                text.draw(label, 10, x, y, [198, 166, 99, 255]);
                let available = x + 80.0 - value_x;
                let mut size = 10;
                while size > 1 && text.width(value, size) > available {
                    size -= 1;
                }
                text.draw(value, size, value_x, y, [245, 238, 216, 255]);
            }
            text.draw(
                &format!("{}/{}", self.stats.hp, self.stats.max_hp),
                11,
                (px + 40) as f32,
                (py + 22) as f32,
                [255, 255, 255, 255],
            );
            text.draw(
                &format!("{}/{}", self.stats.mp, self.stats.max_mp),
                11,
                (px + 40) as f32,
                (py + 36) as f32,
                [255, 255, 255, 255],
            );
            text.draw(
                &format!(
                    "Lv {}  EXP {}/{} ({:.3}%)",
                    self.stats.level,
                    short_exp(self.stats.experience),
                    short_exp(self.stats.max_experience),
                    100.0 * xp_progress as f64
                ),
                11,
                (px + 40) as f32,
                (py + 50) as f32,
                [255, 230, 160, 255],
            );
            let modes = format!(
                "Attack: {}   Pet: {}  [Ctrl+A]",
                super::attack_mode_name(self.attack_mode),
                super::pet_mode_name(self.pet_mode)
            );
            let w = text.width(&modes, 11) + 10.0;
            let x = px as f32 + panel.width as f32 - w - 8.0;
            let y = py as f32 - 18.0;
            renderer.fill_rect(x, y, w, 17.0, [0.08, 0.04, 0.01, 0.9]);
            text.draw(&modes, 11, x + 5.0, y, [255, 218, 145, 255]);
        }
        {
            let Game {
                chat,
                input,
                assets,
                ..
            } = self;
            let mut c = Ctx {
                input,
                assets,
                renderer,
                gpu,
                text,
                now,
            };
            chat.draw(&mut c, width, height, now);
        }
        // Own buffs: Zircon `BuffDialog` icons (CBIcon), top-right, 27 px pitch.
        let buffs = self.buffs.clone();
        for (i, (kind, until)) in buffs.iter().enumerate() {
            let icon = buff_icon(*kind);
            let slot = crate::overlay::buff_icon_rect(i, width);
            let (x, y) = (slot.x, slot.y);
            if let Some(r) = Self::sprite(&mut self.assets, renderer, gpu, 21, icon, Surface::Image)
            {
                renderer.draw(r, x, y, white, Blend::Alpha);
            }
            if *until != u64::MAX {
                let secs = until.saturating_sub(now).div_ceil(1000);
                text.draw(
                    &secs.to_string(),
                    10,
                    x + 2.0,
                    y + 14.0,
                    [255, 255, 255, 230],
                );
            }
        }
        if self.debug {
            let (pages, sprites) = renderer.stats();
            // The key hints give way when the status line carries state
            // (fishing), so the line never wraps into the skill bar.
            let hints = if self.status.len() > 48 {
                ""
            } else {
                " | LMB walk, RMB run, Tab pick up, W bag, Q character, Y wardrobe, J quests, Z belt, N menu, H keys, ` hide"
            };
            let dbg = format!(
                "{} | {:.0} fps | {} objects | {} sprites / {} pages{}",
                self.status,
                fps,
                self.objects.len(),
                sprites,
                pages,
                hints
            );
            text.draw(&dbg, 12, 12.0, height as f32 - 152.0, [200, 200, 200, 255]);
            if let Some(h) = self
                .hovered
                .and_then(|id| self.objects.get(&id).filter(|o| o.name_visible()))
            {
                text.draw(
                    &format!("{} {}/{}", h.name(), h.hp, h.max_hp),
                    12,
                    self.mouse.0 + 14.0,
                    self.mouse.1 + 14.0,
                    [255, 255, 255, 255],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::xp_bar_fill;

    #[test]
    fn xp_bar_shows_tiny_progress_and_reaches_full_width() {
        assert_eq!(xp_bar_fill(0.0, 1000.0), 0.0);
        assert_eq!(xp_bar_fill(0.00028, 1000.0), 3.0);
        assert_eq!(xp_bar_fill(0.5, 1000.0), 500.0);
        assert_eq!(xp_bar_fill(1.0, 1000.0), 1000.0);
    }
}
