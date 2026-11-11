use super::*;

fn item_label_y(x: f32, mut y: f32, width: f32, occupied: &[(f32, f32, f32)]) -> f32 {
    while occupied.iter().any(|&(other_x, other_y, other_width)| {
        x < other_x + other_width && other_x < x + width && y < other_y + 15.0 && other_y < y + 15.0
    }) {
        y -= 16.0;
    }
    y
}

/// Horse armour libraries from `PlayerObject.UpdateLibraries`. The installed
/// collection omits Royal/BlueDragon art, so show a related mounted horse
/// instead of making the mount disappear.
fn horse_layers(assets: &mut Assets, shape: u8, horse: u8, frame: u32) -> Vec<(u16, u32, Blend)> {
    let horse_frame = frame + (u32::from(horse).saturating_sub(1)) * 5000;
    let plain = |library, frame| vec![(library, frame, Blend::Alpha)];
    match shape {
        1 => plain(73, horse_frame),
        2 => plain(74, horse_frame),
        3 => plain(75, horse_frame),
        4 => plain(76, frame),
        5 => vec![(77, frame, Blend::Alpha), (78, frame, Blend::Screen)],
        6 if assets.info(79, frame).is_some() => {
            let mut layers = plain(79, frame);
            if assets.info(80, frame).is_some() {
                layers.push((80, frame, Blend::Screen));
            }
            layers
        }
        6 if assets.info(75, horse_frame).is_some() => plain(75, horse_frame),
        6 => plain(71, horse_frame),
        7 if assets.info(81, frame).is_some() => plain(81, frame),
        7 => plain(76, frame),
        _ => plain(71, horse_frame),
    }
}

fn equipped_weapon_sprite(
    shape: u16,
    female: bool,
    assassin: bool,
    body_frame: u32,
) -> Option<(u16, u32)> {
    let library = if assassin && shape == 1263 {
        mir_formats::wearable_animation::weapon_right_library(shape, female)?
    } else {
        weapon_library(shape, female)?
    };
    let draw_shape = if shape >= 1000 { shape - 1000 } else { shape };
    Some((library, body_frame + (draw_shape as u32 % 10) * 5000))
}

impl Game {
    pub(super) fn body_sprite(&self, o: &ClientObject) -> Option<(u16, u32)> {
        match &o.appearance {
            Appearance::Player {
                gender,
                armour,
                class,
                ..
            } => {
                let female = *gender == Gender::Female;
                let assassin = *class == Class::Assassin;
                let library =
                    armour_library(*armour, female, assassin).unwrap_or(match (assassin, female) {
                        (false, false) => lib::M_HUM,
                        (false, true) => lib::WM_HUM,
                        (true, false) => lib::M_HUM_A,
                        (true, true) => lib::WM_HUM_A,
                    });
                Some((
                    library,
                    o.sprite_index(*armour)
                        .saturating_add_signed(o.armour_shift()),
                ))
            }
            Appearance::Monster { image, .. } => {
                let (library, shape) = monster_sprite(*image)?;
                Some((library, o.sprite_index(shape)))
            }
            Appearance::Spell { effect } => {
                let library = crate::anim::spell_library(*effect)?;
                Some((library, o.sprite_index(0)))
            }
            Appearance::Npc { .. } => Some((lib::NPC, o.sprite_index(0))),
            Appearance::Item { info, .. } => {
                let image = self.catalog.get(*info).map(|d| d.image).unwrap_or(0);
                Some(match *info {
                    1166 => (lib::STORE_ITEMS, 1800),
                    1167 => (lib::STORE_ITEMS, 1804),
                    _ => (lib::GROUND, image.max(0) as u32),
                })
            }
        }
    }

    /// Weapon library and index for a player, if one is equipped.
    pub(super) fn weapon_sprite(&self, o: &ClientObject) -> Option<(u16, u32)> {
        let Appearance::Player {
            gender,
            weapon,
            class,
            ..
        } = &o.appearance
        else {
            return None;
        };
        let shape = (*weapon)?;
        equipped_weapon_sprite(
            shape,
            *gender == Gender::Female,
            *class == Class::Assassin,
            o.draw_frame(),
        )
    }

    pub(super) fn weapon_right_sprite(&self, o: &ClientObject) -> Option<(u16, u32)> {
        let Appearance::Player {
            gender,
            weapon: Some(shape),
            class,
            ..
        } = &o.appearance
        else {
            return None;
        };
        if *class != Class::Assassin || *shape == 1263 {
            return None;
        }
        let library = mir_formats::wearable_animation::weapon_right_library(
            *shape,
            *gender == Gender::Female,
        )?;
        Some((
            library,
            o.draw_frame() + ((*shape as u32 - 1000) % 10) * 5000,
        ))
    }

    // ---- rendering ----------------------------------------------------------------

    pub(super) fn sprite(
        assets: &mut Assets,
        renderer: &mut SpriteRenderer,
        gpu: &Gpu,
        library: u16,
        index: u32,
        surface: Surface,
    ) -> Option<SpriteRegion> {
        let key = SpriteKey {
            library,
            index,
            surface,
        };
        let kind = match surface {
            Surface::Image => SurfaceKind::Image,
            Surface::Shadow => SurfaceKind::Shadow,
            Surface::Overlay => SurfaceKind::Overlay,
        };
        renderer.sprite(gpu, key, || assets.decode(library, index, kind))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        text: &mut TextLayer,
        width: i32,
        height: i32,
        now: u64,
        fps: f32,
    ) {
        if std::mem::take(&mut self.atlas_reset) {
            renderer.reset(gpu);
        }
        self.ui_hover_last_frame = false;
        self.item_label_rects.clear();
        let white = [1.0, 1.0, 1.0, 1.0];
        let view = View::new(width, height, self.user());
        let map_taken = self.map.take();
        if let (Some(map), Some(_)) = (map_taken.as_ref(), self.user) {
            let (ux, uy) = (view.ux, view.uy);
            let x_range =
                (ux - view.off_x - 4).max(0)..=(ux + view.off_x + 4).min(map.width as i32 - 1);
            let y_range =
                (uy - view.off_y - 4).max(0)..=(uy + view.off_y + 4).min(map.height as i32 - 1);

            // Back layer: 96x64 tiles on even cells.
            for y in y_range.clone() {
                if y % 2 != 0 {
                    continue;
                }
                for x in x_range.clone() {
                    if x % 2 != 0 {
                        continue;
                    }
                    let Some(cell) = map.cell(x, y) else { continue };
                    let Some(library) = kr_library(cell.back_file) else {
                        continue;
                    };
                    let (dx, dy) = view.cell_px(x, y);
                    if let Some(r) = Self::sprite(
                        &mut self.assets,
                        renderer,
                        gpu,
                        library,
                        cell.back_image as u32,
                        Surface::Image,
                    ) {
                        renderer.draw(r, dx as f32, dy as f32, white, Blend::Alpha);
                    }
                }
            }

            // Floor pass (upstream MapControl fix): cell-sized static middle and
            // front tiles belong to the floor and never occlude objects.
            for y in y_range.clone() {
                let draw_y = (y - uy + view.off_y + 1) * CELL_H + view.poy - view.umy;
                for x in x_range.clone() {
                    let Some(cell) = map.cell(x, y) else { continue };
                    let draw_x = (x - ux + view.off_x) * CELL_W + view.pox - view.umx;
                    for (file, index, animated) in [
                        (cell.middle_file, cell.middle_image, cell.middle_animated()),
                        (cell.front_file, cell.front_image, cell.front_animated()),
                    ] {
                        if file == 0 || animated {
                            continue;
                        }
                        let Some(library) = kr_library(file) else {
                            continue;
                        };
                        let Some(info) = self.assets.info(library, index as u32) else {
                            continue;
                        };
                        let (w, h) = (info.width as i32, info.height as i32);
                        if !((w == 48 && h == 32) || (w == 96 && h == 64)) {
                            continue;
                        }
                        if let Some(r) = Self::sprite(
                            &mut self.assets,
                            renderer,
                            gpu,
                            library,
                            index as u32,
                            Surface::Image,
                        ) {
                            renderer.draw(
                                r,
                                draw_x as f32,
                                (draw_y - CELL_H) as f32,
                                white,
                                Blend::Alpha,
                            );
                        }
                    }
                }
            }

            // Loot labels belong to the ground layer, below running actors.
            let mut ids: Vec<ObjectId> = self.objects.keys().copied().collect();
            ids.sort_unstable();
            for &id in &ids {
                if self.objects[&id].is_item() {
                    self.draw_object(id, &view, gpu, renderer);
                }
            }
            // Reserve actor names before placing loot, independent of object creation order.
            let mut item_labels: Vec<_> = ids
                .iter()
                .filter_map(|id| {
                    let o = &self.objects[id];
                    if o.is_item() || o.is_spell() || !o.name_visible() {
                        return None;
                    }
                    let (dx, dy) = view.object_px(o);
                    let width = text.width(&o.display_name(), 12) + 6.0;
                    Some((
                        dx as f32 + 24.0 - width / 2.0,
                        (if o.dead { dy + 21 } else { dy - 6 }) as f32,
                        width,
                    ))
                })
                .collect();
            for id in ids {
                let o = &self.objects[&id];
                let (dx, dy) = view.object_px(o);
                if dx < -100 || dx > width + 100 || dy < -150 || dy > height + 100 {
                    continue;
                }
                if o.is_item() {
                    if let Appearance::Item {
                        info,
                        count,
                        enhanced,
                    } = &o.appearance
                    {
                        let mut label = self.catalog.name(*info);
                        if *count > 1 {
                            label = format!("{label} ({count})");
                        }
                        // Drop filter (Zircon `FilterDropDialog`): a wanted
                        // item gets a gold plate so it stands out on the floor.
                        let wanted = crate::menu::highlighted(
                            &self.windows.menu.drop_filter,
                            &self.catalog.name(*info),
                        );
                        let w = text.width(&label, 11) + 6.0;
                        let lx = dx as f32 + 24.0 - w / 2.0;
                        let ly = item_label_y(lx, dy as f32 - 4.0, w, &item_labels);
                        item_labels.push((lx, ly, w));
                        self.item_label_rects.push((id, lx, ly, w));
                        let plate = if wanted {
                            [64.0 / 255.0, 48.0 / 255.0, 0.0, 0.85]
                        } else {
                            [0.0, 24.0 / 255.0, 48.0 / 255.0, 0.75]
                        };
                        renderer.fill_rect(lx, ly, w, 15.0, plate);
                        let ink = if *enhanced {
                            [255, 255, 0, 255]
                        } else if wanted {
                            [255, 220, 120, 255]
                        } else {
                            [255, 255, 255, 255]
                        };
                        text.draw_centered(&label, 11, dx as f32 + 24.0, ly - 1.0, ink);
                    }
                    continue;
                }
            }
            renderer.push_layer();
            text.push_layer();

            // Rows: middle tiles, front tiles, then objects.
            let mut rows: HashMap<i32, Vec<ObjectId>> = HashMap::new();
            for o in self.objects.values().filter(|o| !o.is_item()) {
                rows.entry(o.render_y()).or_default().push(o.id);
            }
            let row_range =
                (uy - view.off_y - 4).max(0)..=(uy + view.off_y + 25).min(map.height as i32 - 1);
            // Zircon (#283): corpses form a ground layer, so a large dead
            // sprite in a later row cannot cover living actors.
            for y in row_range.clone() {
                if let Some(ids) = rows.get(&y) {
                    for id in ids {
                        if self.objects.get(id).is_some_and(|o| o.dead) {
                            self.draw_object(*id, &view, gpu, renderer);
                        }
                    }
                }
            }
            for y in row_range {
                let draw_y = (y - uy + view.off_y + 1) * CELL_H + view.poy - view.umy;
                for layer in 0..2 {
                    for x in x_range.clone() {
                        let Some(cell) = map.cell(x, y) else { continue };
                        let (file, raw_index, animated, blend_flag, count) = if layer == 0 {
                            (
                                cell.middle_file,
                                cell.middle_image,
                                cell.middle_animated(),
                                cell.middle_anim_blend(),
                                cell.middle_anim_count(),
                            )
                        } else {
                            (
                                cell.front_file,
                                cell.front_image,
                                cell.front_animated(),
                                cell.front_anim_blend(),
                                cell.front_anim_count(),
                            )
                        };
                        if file == 0 {
                            continue;
                        }
                        let Some(library) = kr_library(file) else {
                            continue;
                        };
                        let mut index = raw_index as u32;
                        let mut blend = false;
                        if animated {
                            blend = blend_flag;
                            if count > 0 {
                                index += self.animation % count as u32;
                            }
                        }
                        let Some(info) = self.assets.info(library, index) else {
                            continue;
                        };
                        let (w, h) = (info.width as i32, info.height as i32);
                        let cell_sized = (w == 48 && h == 32) || (w == 96 && h == 64);
                        // Static cell-sized tiles were drawn by the floor pass.
                        if cell_sized && !animated {
                            continue;
                        }
                        let draw_x = (x - ux + view.off_x) * CELL_W + view.pox - view.umx;
                        let py = if cell_sized {
                            draw_y - CELL_H
                        } else {
                            draw_y - h
                        };
                        let use_blend = blend;
                        let _ = layer;
                        if let Some(r) = Self::sprite(
                            &mut self.assets,
                            renderer,
                            gpu,
                            library,
                            index,
                            Surface::Image,
                        ) {
                            renderer.draw(
                                r,
                                draw_x as f32,
                                py as f32,
                                white,
                                if use_blend {
                                    Blend::Screen
                                } else {
                                    Blend::Alpha
                                },
                            );
                        }
                    }
                }
                if let Some(ids) = rows.get(&y) {
                    for id in ids {
                        if self.objects.get(id).is_some_and(|o| !o.dead) {
                            self.draw_object(*id, &view, gpu, renderer);
                        }
                    }
                }
            }
            self.draw_effects(&view, gpu, renderer, now);

            // Light layer (Zircon LLayer): blobs into the light target,
            // then the target multiplied over everything drawn so far.
            let light_sprite = renderer.light_sprite(gpu);
            renderer.begin_light();
            self.draw_lights(&view, renderer, light_sprite, map, width, height);
            renderer.end_light();
            if let Some(light) = renderer.light_region() {
                renderer.draw_scaled(
                    light,
                    0.0,
                    0.0,
                    width as f32,
                    height as f32,
                    white,
                    Blend::Multiply,
                );
            }

            // Overlay: actor names, health bars, damage numbers.
            let mut ids: Vec<_> = self.objects.keys().copied().collect();
            ids.sort_unstable();
            for id in ids {
                let o = &self.objects[&id];
                let (dx, dy) = view.object_px(o);
                if o.is_item()
                    || o.is_spell()
                    || dx < -100
                    || dx > width + 100
                    || dy < -150
                    || dy > height + 100
                {
                    continue;
                }
                let name_color = if o.is_npc() {
                    [0, 255, 0, 255]
                } else if let Appearance::Player {
                    name_color, guild, ..
                } = &o.appearance
                {
                    // Members of a guild we are at war with show in orange.
                    let enemy = !guild.is_empty()
                        && self
                            .guild
                            .as_ref()
                            .is_some_and(|g| g.wars.iter().any(|(n, _)| n == guild));
                    // Zircon ProcessNameColour: yellow at 50 PK points,
                    // brown after hitting an innocent, red at 200.
                    match name_color {
                        _ if enemy => [255, 140, 0, 255],
                        1 => [255, 255, 0, 255],
                        2 => [165, 42, 42, 255],
                        3 => [255, 0, 0, 255],
                        _ => [255, 255, 255, 255],
                    }
                } else {
                    [255, 255, 255, 220]
                };
                let name_y = if o.dead { dy + 21 } else { dy - 6 };
                if o.is_npc() && o.quest_marker != 0 {
                    let (symbol, colour) = match o.quest_marker {
                        1 => ("!", [255, 224, 62, 255]),
                        2 => ("?", [160, 160, 160, 255]),
                        _ => ("?", [255, 222, 62, 255]),
                    };
                    text.draw_centered(symbol, 24, dx as f32 + 24.0, name_y as f32 - 68.0, colour);
                }
                if let Some((line, t)) = &o.bubble {
                    if now < t + 5000 {
                        // Zircon `DrawChat`: grey while the speaker is dead.
                        let color = if o.dead {
                            [160, 160, 160, 255]
                        } else {
                            [255, 255, 255, 255]
                        };
                        text.draw_centered(line, 12, dx as f32 + 24.0, name_y as f32 - 16.0, color);
                    }
                }
                let my_pet = o.pet_owner().is_some() && o.pet_owner() == self.user_name();
                if o.name_visible() {
                    text.draw_centered(
                        &o.display_name(),
                        12,
                        dx as f32 + 24.0,
                        name_y as f32,
                        name_color,
                    );
                }
                if let Appearance::Player {
                    guild, guild_rank, ..
                } = &o.appearance
                {
                    if !guild.is_empty() {
                        text.draw_centered(
                            &format!("{guild} [{guild_rank}]"),
                            10,
                            dx as f32 + 24.0,
                            name_y as f32 + 13.0,
                            [200, 200, 255, 255],
                        );
                    }
                }
                let show_bar = !o.is_player()
                    && !o.dead
                    && (my_pet || now < o.health_time || self.hovered == Some(id));
                if show_bar {
                    if let Some(bg) = Self::sprite(
                        &mut self.assets,
                        renderer,
                        gpu,
                        lib::INTERFACE,
                        80,
                        Surface::Image,
                    ) {
                        renderer.draw(bg, dx as f32, (dy - 55) as f32, white, Blend::Alpha);
                    }
                    if let Some(fill) = Self::sprite(
                        &mut self.assets,
                        renderer,
                        gpu,
                        lib::INTERFACE,
                        79,
                        Surface::Image,
                    ) {
                        let pct = o.hp.max(0) as f32 / o.max_hp.max(1) as f32;
                        renderer.draw_cropped(
                            fill,
                            (dx + 1) as f32,
                            (dy - 54) as f32,
                            pct,
                            [0.0, 200.0 / 255.0, 74.0 / 255.0, 1.0],
                        );
                    }
                }
                for (dmg, t) in &o.damage {
                    let age = now.saturating_sub(*t) as f32 / 1500.0;
                    let rise = age * 40.0;
                    let alpha = ((1.0 - age) * 255.0) as u8;
                    // Zircon `DamageInfo`: red, green, orange then white as
                    // the hit grows (its values are negative for damage).
                    let color = crate::overlay::damage_color(-dmg, alpha);
                    text.draw_centered(
                        &format!("-{dmg}"),
                        16,
                        dx as f32 + 24.0,
                        (dy - 70) as f32 - rise,
                        color,
                    );
                }
            }
        } else {
            text.draw_centered(
                &self.status,
                18,
                width as f32 / 2.0,
                height as f32 / 2.0,
                [255, 255, 255, 255],
            );
        }
        self.map = map_taken;

        self.draw_overlays(gpu, renderer, text, width, now);
        // The codex is topmost. Keep a click inside it from activating a
        // covered HUD button, map, or inventory slot beneath it.
        let codex_input = self
            .dungeon_codex
            .contains_mouse(self.input.mouse, width, height)
            .then(|| self.input.clone());
        if codex_input.is_some() {
            self.input.lmb_down = false;
            self.input.lmb_pressed = false;
            self.input.lmb_released = false;
            self.input.rmb_down = false;
            self.input.rmb_pressed = false;
            self.input.wheel = 0.0;
        }
        renderer.push_layer();
        text.push_layer();
        self.draw_hud(gpu, renderer, text, width, height, now, fps);
        self.draw_map_windows(gpu, renderer, text, width, height, now, false);
        self.draw_windows(gpu, renderer, text, width, height, now);
        if std::mem::take(&mut self.windows.toggle_map) {
            self.minimap.big_open = !self.minimap.big_open;
        }
        self.draw_map_windows(gpu, renderer, text, width, height, now, true);
        self.draw_extra_windows(gpu, renderer, text, width, height, now);
        if let Some(input) = codex_input {
            self.input = input;
        }
        let codex_over = {
            let Game {
                assets,
                input,
                dungeon_codex,
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
            dungeon_codex.draw(&mut c, width, height)
        };
        self.windows_open_last_frame |= self.dungeon_codex.open;
        self.ui_hover_last_frame |= codex_over;
        if codex_over {
            self.lmb = false;
            self.rmb = false;
            self.input.lmb_pressed = false;
            self.input.rmb_pressed = false;
            self.hovered = None;
        }
        let mouse = self.input.mouse;
        let over_ui = self.chat.contains_mouse(mouse)
            || (self.map.is_some() && self.minimap.contains_mouse(mouse, width, height));
        if let Some(drop) = self
            .windows
            .drop_carried_item(&self.inventory, self.input.lmb_pressed && !over_ui)
        {
            self.pending_messages.push(drop);
            self.lmb = false;
        }
        if self.windows.settings.preferences.show_fps {
            renderer.push_layer();
            text.push_layer();
            renderer.fill_rect(8.0, 8.0, 78.0, 24.0, [0.0, 0.0, 0.0, 0.8]);
            text.draw(
                &format!("{fps:.0} FPS"),
                12,
                14.0,
                12.0,
                [255, 255, 255, 255],
            );
        }
    }

    /// The windows that keep their own state rather than living in
    /// `WindowState`: the fortune checker, the chat-tab editor and the HUD
    /// countdown. Each opens its own layer through `Ctx::window`.
    pub(super) fn draw_extra_windows(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        text: &mut TextLayer,
        width: i32,
        height: i32,
        now: u64,
    ) {
        // The menu relays its clicks through these one-shot flags.
        if std::mem::take(&mut self.windows.toggle_fortune) {
            self.fortune.open = !self.fortune.open;
        }
        if std::mem::take(&mut self.windows.toggle_chat_options) {
            self.chat_options.open = !self.chat_options.open;
        }
        if std::mem::take(&mut self.windows.open_character_services) {
            self.character_services.open = true;
        }
        let appearance = self.user().map(|u| u.appearance.clone());
        let mut out = Vec::new();
        let over = {
            let Game {
                assets,
                input,
                catalog,
                fortune,
                fortunes,
                chat_options,
                character_services,
                gold,
                stats,
                windows,
                chat,
                timers,
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
            timers.draw(&mut c, width, height, now);
            let now_secs = crate::fortune::wall_clock();
            let a = fortune.draw(&mut c, catalog, fortunes, now_secs, width, height, &mut out);
            let b = chat_options.draw(&mut c, chat, width, height);
            let d = character_services.draw(
                &mut c,
                appearance.as_ref(),
                stats,
                *gold,
                &windows.discipline_next,
                width,
                height,
                &mut out,
            );
            a || b || d
        };
        self.pending_messages.extend(out);
        self.windows_open_last_frame |=
            self.character_services.open || self.chat_options.open || self.fortune.open;
        self.ui_hover_last_frame |= over;
        if over {
            self.lmb = false;
            self.rmb = false;
            self.input.lmb_pressed = false;
            self.input.rmb_pressed = false;
            self.hovered = None;
        }
    }

    pub(super) fn draw_object(
        &mut self,
        id: ObjectId,
        view: &View,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
    ) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some((library, index)) = self.body_sprite(o) else {
            return;
        };
        let (dx, mut dy) = view.object_px(o);
        if matches!(&o.appearance, Appearance::Monster { image: 137, .. }) {
            dy -= CELL_H;
        }
        if o.is_spell() {
            let color = match &o.appearance {
                Appearance::Spell { effect }
                    if matches!(
                        *effect,
                        mir_proto::spell_effect::FIRE_WALL
                            | mir_proto::spell_effect::TEMPEST
                            | mir_proto::spell_effect::ICE_AURA
                    ) =>
                {
                    [1.0, 1.0, 1.0, 0.55]
                }
                Appearance::Spell { effect }
                    if *effect == mir_proto::spell_effect::TRAP_OCTAGON =>
                {
                    [1.0, 1.0, 1.0, 0.8]
                }
                Appearance::Spell { effect }
                    if matches!(
                        *effect,
                        mir_proto::spell_effect::BURNING_FIRE
                            | mir_proto::spell_effect::DARK_SOUL_PRISON
                            | mir_proto::spell_effect::DEATH_CLOUD
                    ) =>
                {
                    [1.0, 1.0, 1.0, 1.0]
                }
                _ => [139.0 / 255.0, 69.0 / 255.0, 19.0 / 255.0, 1.0],
            };
            if let Some(info) = self.assets.info(library, index) {
                if let Some(r) = Self::sprite(
                    &mut self.assets,
                    renderer,
                    gpu,
                    library,
                    index,
                    Surface::Image,
                ) {
                    renderer.draw(
                        r,
                        (dx + info.offset_x as i32) as f32,
                        (dy + info.offset_y as i32) as f32,
                        color,
                        Blend::Screen,
                    );
                }
            }
            return;
        }
        if o.is_item() {
            if let Some(info) = self.assets.info(library, index) {
                let x = dx + (CELL_W - info.width as i32) / 2;
                let y = dy + (CELL_H - info.height as i32) / 2;
                if let Some(r) = Self::sprite(
                    &mut self.assets,
                    renderer,
                    gpu,
                    library,
                    index,
                    Surface::Image,
                ) {
                    renderer.draw(r, x as f32, y as f32, [1.0, 1.0, 1.0, 1.0], Blend::Alpha);
                }
            }
            return;
        }
        let weapon = self.weapon_sprite(o);
        let weapon_right = self.weapon_right_sprite(o);
        let weapon_glow = match &o.appearance {
            Appearance::Player { weapon_effect, .. } => {
                crate::equipment_fx::weapon_glow_colour(*weapon_effect)
            }
            _ => None,
        };
        let direction = o.direction;
        let player_effects = match &o.appearance {
            Appearance::Player {
                gender,
                armour_effect,
                emblem_effect,
                weapon_effect,
                shield_effect,
                horse,
                ..
            } => Some((
                [
                    *armour_effect,
                    *emblem_effect,
                    *weapon_effect,
                    *shield_effect,
                ],
                *gender,
                *horse > 0,
                o.draw_frame(),
            )),
            _ => None,
        };
        // Zircon: WeaponLibrary1 is behind the body for Up/DownLeft/Left/UpLeft.
        let weapon_behind = matches!(
            direction,
            Direction::Up | Direction::DownLeft | Direction::Left | Direction::UpLeft
        );
        // Helmet replaces hair; shields sit behind the body when facing
        // right (Zircon `DrawBody`).
        let (helmet, shield) = match &o.appearance {
            Appearance::Player {
                helmet,
                shield,
                gender,
                class,
                ..
            } => {
                let female = *gender == Gender::Female;
                let assassin = *class == Class::Assassin;
                let stride = if assassin { 3000 } else { 5000 };
                let h = if *helmet > 0 {
                    helmet_library(*helmet, female, assassin).map(|l| {
                        (
                            l,
                            (o.draw_frame() + ((*helmet as u32 - 1) % 10) * stride)
                                .saturating_add_signed(o.armour_shift()),
                        )
                    })
                } else {
                    None
                };
                let s = shield.and_then(|sh| {
                    shield_library(sh, female).map(|l| {
                        (
                            l,
                            (o.draw_frame() + (sh as u32 % 10) * stride)
                                .saturating_add_signed(o.armour_shift()),
                        )
                    })
                });
                (h, s)
            }
            _ => (None, None),
        };
        let has_helmet = matches!(&o.appearance, Appearance::Player { helmet, .. } if *helmet > 0);
        let shield_behind = matches!(
            direction,
            Direction::UpRight | Direction::Right | Direction::DownRight
        );
        let (hair_tint, armour_tint) = match &o.appearance {
            Appearance::Player {
                hair_colour,
                armour_colour,
                ..
            } => (
                Self::dye_tint(*hair_colour),
                (*armour_colour != 0).then(|| Self::dye_tint(*armour_colour)),
            ),
            _ => ([1.0; 4], None),
        };
        let hair = match &o.appearance {
            Appearance::Player {
                hair,
                gender,
                class,
                ..
            } if *hair > 0 && !has_helmet => {
                let hair_lib = match (*class == Class::Assassin, *gender == Gender::Female) {
                    (false, false) => lib::M_HAIR,
                    (false, true) => lib::WM_HAIR,
                    (true, false) => lib::M_HAIR_A,
                    (true, true) => lib::WM_HAIR_A,
                };
                Some((hair_lib, o.draw_frame() + (*hair as u32 - 1) * 5000))
            }
            _ => None,
        };
        // The horse goes under everything (Zircon draws it before the body).
        if let Appearance::Player {
            horse, horse_shape, ..
        } = &o.appearance
        {
            if *horse > 0
                && matches!(
                    o.action,
                    Action::Standing | Action::Walking | Action::Running | Action::Struck
                )
            {
                let frame = o.draw_frame();
                for (hl, hi, blend) in horse_layers(&mut self.assets, *horse_shape, *horse, frame) {
                    self.draw_layer_blended(hl, hi, dx, dy, gpu, renderer, [1.0; 4], blend);
                }
            }
        }
        if let Some((effects, gender, mounted, body_frame)) = player_effects {
            Self::draw_exterior_effects(
                &mut self.assets,
                renderer,
                gpu,
                effects,
                gender,
                direction,
                body_frame,
                self.animation,
                mounted,
                dx,
                dy,
                true,
            );
        }
        if let (Some((wl, wi)), true) = (weapon, weapon_behind) {
            self.draw_weapon_layer(wl, wi, dx, dy, gpu, renderer, weapon_glow);
        }
        if let (Some((wl, wi)), false) = (weapon_right, weapon_behind) {
            self.draw_weapon_layer(wl, wi, dx, dy, gpu, renderer, weapon_glow);
        }
        if let (Some((sl, si)), true) = (shield, shield_behind) {
            self.draw_layer(sl, si, dx, dy, gpu, renderer);
        }
        let Some(info) = self.assets.info(library, index) else {
            return;
        };
        let white = [1.0, 1.0, 1.0, 1.0];

        // Shadow: baked surface if present, else Zircon's sheared fallback.
        if info.has_surface(SurfaceKind::Shadow) {
            if let Some(r) = Self::sprite(
                &mut self.assets,
                renderer,
                gpu,
                library,
                index,
                Surface::Shadow,
            ) {
                renderer.draw(
                    r,
                    (dx + info.shadow_offset_x as i32) as f32,
                    (dy + info.shadow_offset_y as i32) as f32,
                    [1.0, 1.0, 1.0, 0.5],
                    Blend::Alpha,
                );
            }
        } else if matches!(info.shadow_type, 177 | 176 | 49) {
            if let Some(r) = Self::sprite(
                &mut self.assets,
                renderer,
                gpu,
                library,
                index,
                Surface::Image,
            ) {
                let (w, h) = (r.width as f32, r.height as f32);
                let tx = (dx + info.shadow_offset_x as i32) as f32 + h / 2.0;
                let ty = (dy + info.shadow_offset_y as i32) as f32;
                // (px, py) -> (px - 0.5*py + tx, 0.5*py + ty)
                let corners = [
                    [tx, ty],
                    [tx + w, ty],
                    [tx + w - 0.5 * h, ty + 0.5 * h],
                    [tx - 0.5 * h, ty + 0.5 * h],
                ];
                renderer.draw_quad(r, corners, [0.0, 0.0, 0.0, 0.5], Blend::Alpha);
            }
        }

        if let Some(r) = Self::sprite(
            &mut self.assets,
            renderer,
            gpu,
            library,
            index,
            Surface::Image,
        ) {
            let hidden = self
                .objects
                .get(&id)
                .map(|o| {
                    o.visible_buffs.iter().any(|b| {
                        matches!(
                            *b,
                            buff_type::INVISIBILITY | buff_type::TRANSPARENCY | buff_type::CLOAK
                        )
                    })
                })
                .unwrap_or(false);
            let tint = if let Some(tint) = self.objects.get(&id).and_then(|o| o.poison_tint()) {
                tint
            } else if hidden {
                [1.0, 1.0, 1.0, 0.5]
            } else if self.hovered == Some(id) {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                white
            };
            renderer.draw(
                r,
                (dx + info.offset_x as i32) as f32,
                (dy + info.offset_y as i32) as f32,
                tint,
                Blend::Alpha,
            );
        }
        // Zircon dyes the armour overlay, leaving the painted body untouched.
        if let Some(armour_tint) = armour_tint {
            if let Some(r) = Self::sprite(
                &mut self.assets,
                renderer,
                gpu,
                library,
                index,
                Surface::Overlay,
            ) {
                renderer.draw(
                    r,
                    (dx + info.offset_x as i32) as f32,
                    (dy + info.offset_y as i32) as f32,
                    armour_tint,
                    Blend::Alpha,
                );
            }
        }
        if let Some((hl, hi)) = helmet {
            self.draw_layer(hl, hi, dx, dy, gpu, renderer);
        }
        if let Some((hair_lib, hair_index)) = hair {
            self.draw_layer_tinted(hair_lib, hair_index, dx, dy, gpu, renderer, hair_tint);
        }
        if let (Some((wl, wi)), false) = (weapon, weapon_behind) {
            self.draw_weapon_layer(wl, wi, dx, dy, gpu, renderer, weapon_glow);
        }
        if let (Some((wl, wi)), true) = (weapon_right, weapon_behind) {
            self.draw_weapon_layer(wl, wi, dx, dy, gpu, renderer, weapon_glow);
        }
        if let (Some((sl, si)), false) = (shield, shield_behind) {
            self.draw_layer(sl, si, dx, dy, gpu, renderer);
        }
        if let Some((effects, gender, mounted, body_frame)) = player_effects {
            Self::draw_exterior_effects(
                &mut self.assets,
                renderer,
                gpu,
                effects,
                gender,
                direction,
                body_frame,
                self.animation,
                mounted,
                dx,
                dy,
                false,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_exterior_effects(
        assets: &mut Assets,
        renderer: &mut SpriteRenderer,
        gpu: &Gpu,
        effects: [u8; 4],
        gender: Gender,
        direction: Direction,
        body_frame: u32,
        animation: u32,
        mounted: bool,
        dx: i32,
        dy: i32,
        behind: bool,
    ) {
        for (slot, effect) in effects.into_iter().enumerate() {
            if effect == 0
                || (slot == 1 && mounted)
                || crate::equipment_fx::behind(effect, direction) != behind
            {
                continue;
            }
            let (offset_x, offset_y) = if mounted {
                if (1..=3).contains(&effect) {
                    let x = match direction {
                        Direction::UpRight | Direction::Right | Direction::DownRight => 7,
                        Direction::DownLeft | Direction::Left | Direction::UpLeft => -8,
                        _ => 0,
                    };
                    (x, -25)
                } else if (10..=18).contains(&effect) {
                    let x = match direction {
                        Direction::UpRight | Direction::Right | Direction::DownRight => 7,
                        Direction::DownLeft | Direction::Left | Direction::UpLeft => -8,
                        _ => 0,
                    };
                    let y = if matches!(
                        direction,
                        Direction::Down | Direction::DownLeft | Direction::DownRight
                    ) {
                        -16
                    } else {
                        -30
                    };
                    (x, y)
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };
            for sprite in
                crate::equipment_fx::world_sprites(effect, gender, direction, body_frame, animation)
            {
                let Some(info) = assets.info(sprite.library, sprite.frame) else {
                    continue;
                };
                if let Some(region) = Self::sprite(
                    assets,
                    renderer,
                    gpu,
                    sprite.library,
                    sprite.frame,
                    Surface::Image,
                ) {
                    renderer.draw(
                        region,
                        (dx + offset_x + info.offset_x as i32) as f32,
                        (dy + offset_y + info.offset_y as i32) as f32,
                        [1.0, 1.0, 1.0, sprite.alpha],
                        if sprite.blend {
                            Blend::Screen
                        } else {
                            Blend::Alpha
                        },
                    );
                }
            }
        }
    }

    /// A packed ARGB dye as a multiply tint. 0 means undyed, which leaves
    /// the sprite's own colours alone.
    pub(super) fn dye_tint(colour: u32) -> [f32; 4] {
        if colour >> 24 == 0 {
            return [1.0, 1.0, 1.0, 1.0];
        }
        [
            ((colour >> 16) & 0xFF) as f32 / 255.0,
            ((colour >> 8) & 0xFF) as f32 / 255.0,
            (colour & 0xFF) as f32 / 255.0,
            1.0,
        ]
    }

    /// Draw one equipment/hair layer with the image's own offsets.
    pub(super) fn draw_layer(
        &mut self,
        library: u16,
        index: u32,
        dx: i32,
        dy: i32,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
    ) {
        self.draw_layer_tinted(library, index, dx, dy, gpu, renderer, [1.0, 1.0, 1.0, 1.0]);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_weapon_layer(
        &mut self,
        library: u16,
        index: u32,
        dx: i32,
        dy: i32,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        glow: Option<[f32; 4]>,
    ) {
        if let Some(colour) = glow {
            for (x, y) in [(-2, 0), (2, 0), (0, -2), (0, 2)] {
                self.draw_layer_blended(
                    library,
                    index,
                    dx + x,
                    dy + y,
                    gpu,
                    renderer,
                    colour,
                    Blend::Screen,
                );
            }
        }
        self.draw_layer(library, index, dx, dy, gpu, renderer);
    }

    /// The same, multiplied by a tint: hair takes its dye this way, as does
    /// dyed armour.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_layer_tinted(
        &mut self,
        library: u16,
        index: u32,
        dx: i32,
        dy: i32,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        tint: [f32; 4],
    ) {
        self.draw_layer_blended(library, index, dx, dy, gpu, renderer, tint, Blend::Alpha);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_layer_blended(
        &mut self,
        library: u16,
        index: u32,
        dx: i32,
        dy: i32,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        tint: [f32; 4],
        blend: Blend,
    ) {
        if let Some(info) = self.assets.info(library, index) {
            if let Some(r) = Self::sprite(
                &mut self.assets,
                renderer,
                gpu,
                library,
                index,
                Surface::Image,
            ) {
                renderer.draw(
                    r,
                    (dx + info.offset_x as i32) as f32,
                    (dy + info.offset_y as i32) as f32,
                    tint,
                    blend,
                );
            }
        }
    }
}

impl Game {
    /// Zircon `MapControl.Light.OnClearTexture`: object lights (`0.1 +
    /// light * 0.04` of the 1024x768 sprite, anchored at the cell's top
    /// centre), then cell lights (`0.1 + light * 0.6`, cell centre).
    fn draw_lights(
        &self,
        view: &View,
        renderer: &mut SpriteRenderer,
        sprite: SpriteRegion,
        map: &MapFile,
        width: i32,
        height: i32,
    ) {
        // The sprite is half of Zircon's bitmap: scale twice as much.
        let (sw, sh) = (1024.0f32, 768.0f32);
        let blob =
            |renderer: &mut SpriteRenderer, cx: f32, cy: f32, scale: f32, color: [f32; 4]| {
                let (w, h) = (sw * scale, sh * scale);
                renderer.draw_scaled(sprite, cx - w / 2.0, cy - h / 2.0, w, h, color, Blend::Add);
            };
        let user = self.user;
        for o in self.objects.values() {
            let is_user = Some(o.id) == user;
            let mut light = o.light as f32;
            let mut color = [1.0, 1.0, 1.0, 1.0];
            if is_user {
                light = light.max(3.0);
                if o.light == 0 {
                    color[3] = 120.0 / 255.0;
                }
            }
            if light <= 0.0 || (o.dead && !is_user && !o.is_spell()) {
                continue;
            }
            if let Appearance::Spell { effect } = &o.appearance {
                color = match *effect {
                    mir_proto::spell_effect::FIRE_WALL | mir_proto::spell_effect::BURNING_FIRE => {
                        effects::FIRE
                    }
                    mir_proto::spell_effect::TEMPEST => effects::WIND,
                    mir_proto::spell_effect::ICE_AURA => effects::ICE,
                    _ => color,
                };
            }
            let (dx, dy) = view.object_px(o);
            if dx < -600 || dx > width + 600 || dy < -600 || dy > height + 600 {
                continue;
            }
            let scale = 0.1 + light * 0.04;
            blob(
                renderer,
                dx as f32 + CELL_W as f32 / 2.0,
                dy as f32,
                scale,
                color,
            );
        }
        let x_range = (view.ux - view.off_x - 15).max(0)
            ..=(view.ux + view.off_x + 15).min(map.width as i32 - 1);
        let y_range = (view.uy - view.off_y - 15).max(0)
            ..=(view.uy + view.off_y + 15).min(map.height as i32 - 1);
        for y in y_range {
            for x in x_range.clone() {
                let Some(cell) = map.cell(x, y) else { continue };
                if cell.light == 0 {
                    continue;
                }
                let (dx, dy) = view.cell_px(x, y);
                let scale = 0.1 + cell.light as f32 * 0.6;
                blob(
                    renderer,
                    dx as f32 + CELL_W as f32 / 2.0,
                    dy as f32 + CELL_H as f32 / 2.0,
                    scale,
                    [1.0, 1.0, 1.0, 1.0],
                );
            }
        }
    }
}

#[cfg(test)]
mod item_label_tests {
    use super::item_label_y;

    #[test]
    fn moves_colliding_labels_until_they_fit() {
        let occupied = [(10.0, 100.0, 70.0), (12.0, 84.0, 70.0)];
        assert_eq!(item_label_y(20.0, 100.0, 50.0, &occupied), 68.0);
        assert_eq!(item_label_y(80.0, 100.0, 50.0, &occupied), 100.0);
    }
}

#[cfg(test)]
mod horse_art_tests {
    use super::{horse_layers, Assets, Blend};

    #[test]
    fn royal_and_blue_dragon_use_source_art_or_related_visible_fallbacks() {
        let Some(root) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let mut assets = Assets::new(root);
        let royal = horse_layers(&mut assets, 6, 1, 2240);
        let dragon = horse_layers(&mut assets, 7, 1, 2240);
        assert!(matches!(royal[0], (79 | 75 | 71, 2240, Blend::Alpha)));
        assert!(matches!(dragon[0], (81 | 76, 2240, Blend::Alpha)));
        for (library, frame, _) in royal.into_iter().chain(dragon) {
            assert!(
                assets.info(library, frame).is_some(),
                "missing horse {library}:{frame}"
            );
        }
        for frame in [1800, 1804] {
            assert!(assets
                .info(crate::assets::lib::STORE_ITEMS, frame)
                .is_some());
        }
    }
}

#[cfg(test)]
mod late_weapon_tests {
    use super::{equipped_weapon_sprite, Assets};

    #[test]
    fn janitor_and_chaotic_world_frames_have_matching_source_art() {
        let Some(root) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let mut assets = Assets::new(root);
        for (shape, female, assassin, library, frame, icon) in [
            (44, false, false, 87, 20000, 1076),
            (67, false, false, 89, 35000, 2530),
            (67, true, false, 103, 35000, 2530),
            (1114, false, true, 118, 20000, 2550),
            (1114, true, true, 130, 20000, 2550),
            (1263, false, true, 116, 15000, 1076),
            (1263, true, true, 128, 15000, 1076),
        ] {
            assert_eq!(
                equipped_weapon_sprite(shape, female, assassin, 0),
                Some((library, frame))
            );
            assert!(
                assets.info(library, frame).is_some(),
                "missing weapon {library}:{frame}"
            );
            assert!(assets.info(crate::assets::lib::STORE_ITEMS, icon).is_some());
            assert!(assets.info(crate::assets::lib::EQUIP, icon).is_some());
        }
        for effect in [110, 111, 112] {
            let sprite = crate::equipment_fx::world_sprites(
                effect,
                mir_proto::Gender::Male,
                mir_proto::Direction::Down,
                0,
                0,
            )[0];
            assert!(assets.info(sprite.library, sprite.frame).is_some());
        }
    }
}
