use super::*;

fn confirm_belt_key(pending: &mut Option<(u8, u64)>, slot: u8, now: u64) -> bool {
    let confirmed =
        pending.is_some_and(|(previous, at)| previous == slot && now.saturating_sub(at) <= 450);
    *pending = (!confirmed).then_some((slot, now));
    confirmed
}

fn spell_bar_slot((x, y): (f32, f32), height: i32) -> Option<u8> {
    let top = height as f32 - 140.0;
    let x = x - 10.0;
    if !(top..top + 36.0).contains(&y) || !(0.0..12.0 * 37.0).contains(&x) {
        return None;
    }
    (x % 37.0 < 36.0).then_some((x / 37.0) as u8 + 1)
}

impl Game {
    /// True while any text box anywhere owns the keyboard, so world
    /// shortcuts stay out of the way of typing.
    fn text_entry_open(&self) -> bool {
        self.windows.typing()
            || self.friends_window.typing()
            || self.fortune.typing()
            || self.dungeon_codex.typing()
            || self.chat_options.typing()
            || self.character_services.typing()
    }

    pub(super) fn hit_test(&mut self, width: i32, height: i32) -> Option<ObjectId> {
        // Labels move to avoid overlap; use their displayed bounds, not the floor sprite.
        if let Some(&(id, _, _, _)) = self.item_label_rects.iter().rev().find(|&&(id, x, y, w)| {
            self.objects.get(&id).is_some_and(|o| o.is_item())
                && self.mouse.0 >= x
                && self.mouse.0 < x + w
                && self.mouse.1 >= y
                && self.mouse.1 < y + 15.0
        }) {
            return Some(id);
        }
        let view = View::new(width, height, self.user());
        let (mx, my) = (self.mouse.0 as i32, self.mouse.1 as i32);
        let mut best: Option<(i32, ObjectId)> = None;
        let ids: Vec<ObjectId> = self.objects.keys().copied().collect();
        for id in ids {
            let o = &self.objects[&id];
            // Corpses stay clickable for Corpse Exploder and Summon Dead.
            if Some(id) == self.user || o.is_spell() || (o.dead && !o.is_monster()) {
                continue;
            }
            let Some((library, index)) = self.body_sprite(o) else {
                continue;
            };
            let (dx, dy) = view.object_px(o);
            let ry = o.render_y();
            let Some(info) = self.assets.info(library, index) else {
                continue;
            };
            let (x0, y0) = if o.is_item() {
                (
                    dx + (CELL_W - info.width as i32) / 2,
                    dy + (CELL_H - info.height as i32) / 2,
                )
            } else {
                (dx + info.offset_x as i32, dy + info.offset_y as i32)
            };
            let inside =
                mx >= x0 && mx < x0 + info.width as i32 && my >= y0 && my < y0 + info.height as i32;
            if inside && best.map(|(y, _)| ry >= y).unwrap_or(true) {
                best = Some((ry, id));
            }
        }
        best.map(|(_, id)| id)
    }

    pub(super) fn blocked(&self, p: Point) -> bool {
        let Some(map) = &self.map else {
            return true;
        };
        if !map.is_walkable(p.x, p.y) {
            return true;
        }
        self.objects.values().any(|o| {
            !o.dead && !o.is_item() && !o.is_spell() && Some(o.id) != self.user && o.location == p
        })
    }

    pub(super) fn handle_input(
        &mut self,
        now: u64,
        width: i32,
        height: i32,
        conn: Option<&Connection>,
    ) {
        if !self.dungeon_codex.typing() {
            self.chat_keys(now, conn);
        }
        if self.windows.inventory_open
            && self
                .windows
                .inventory_rect(width, height)
                .contains(self.input.mouse.0, self.input.mouse.1)
        {
            self.lmb = false;
            self.rmb = false;
        }
        // A focused window text box owns the keyboard. The fortune search
        // and the chat-tab rename box live outside `WindowState`, so they
        // are asked separately.
        if self.text_entry_open() {
            self.input.digit = None;
            self.input.fkey = None;
            self.input.tab = false;
        }
        if self.input.lmb_pressed && !self.chat.open {
            if let Some(slot) = spell_bar_slot(self.input.mouse, height) {
                self.windows.hover_magic = None;
                self.function_key(slot, now, width, height, conn);
                return;
            }
        }
        // Keyboard shortcuts, Zircon's `KeyBindAction` defaults: Q character,
        // W inventory, E magic, J quest log, `,` mail, Z belt, P group,
        // G guild, S storage, U companion, N menu, H help, A auto potion,
        // M mount, T trade, Tab pick up, Ctrl+H attack mode, Ctrl+A pet mode,
        // Alt+Q leave.
        let typing = self.text_entry_open();
        // A modifier is held, so the text event carries a control character
        // and `chord` names the physical letter instead.
        if !typing && (self.input.ctrl || self.input.alt) {
            match (self.input.chord, self.input.ctrl) {
                (Some('h'), true) => {
                    if let Some(c) = conn {
                        c.send(ClientMessage::AttackMode {
                            mode: (self.attack_mode + 1) % (mir_proto::attack_mode::ALL + 1),
                        });
                    }
                }
                (Some('a'), true) => {
                    if let Some(c) = conn {
                        c.send(ClientMessage::PetMode {
                            mode: (self.pet_mode + 1) % (mir_proto::pet_mode::NONE + 1),
                        });
                    }
                }
                (Some('q'), false) => self.windows.menu.exit_open = true,
                _ => {}
            }
        }
        let shortcuts: String = if typing || self.input.ctrl || self.input.alt {
            String::new()
        } else {
            self.input.text.clone()
        };
        for ch in shortcuts.chars() {
            match ch.to_ascii_lowercase() {
                'b' => {
                    if now >= self.action_time
                        && self.user().is_some_and(|u| !u.dead)
                        && self.user().is_some_and(|u| !u.mounted())
                    {
                        if let Some(cell) = self.nearest_corpse(1) {
                            self.swing_harvest(cell, now, conn);
                        }
                    }
                }
                'w' => self.windows.inventory_open = !self.windows.inventory_open,
                'q' => self.windows.character_open = !self.windows.character_open,
                'e' => self.windows.skills_open = !self.windows.skills_open,
                'z' => self.windows.belt_open = !self.windows.belt_open,
                'j' => self.windows.quests_open = !self.windows.quests_open,
                'p' => self.windows.group_open = !self.windows.group_open,
                's' => self.windows.storage_open = !self.windows.storage_open,
                'g' => self.windows.guild_open = !self.windows.guild_open,
                ',' => self.windows.mail_open = !self.windows.mail_open,
                'm' => {
                    if let Some(c) = conn {
                        c.send(ClientMessage::Mount);
                    }
                }
                'u' => self.windows.companion_open = !self.windows.companion_open,
                // Zircon's MarketPlaceWindow default.
                'c' => {
                    self.windows.market.open = !self.windows.market.open;
                    if self.windows.market.open {
                        self.windows.market.refresh();
                    }
                }
                // Zircon opens these two from menus and an NPC, with no
                // key of their own; K and O were free here.
                'k' => self.fortune.open = !self.fortune.open,
                'i' => {
                    self.dungeon_codex.open = !self.dungeon_codex.open;
                    if self.dungeon_codex.open {
                        self.dungeon_codex.select_floor(&self.map_file);
                    }
                }
                'y' => self.wardrobe.open = !self.wardrobe.open,
                'o' => self.chat_options.open = !self.chat_options.open,
                'd' => self.character_services.open = !self.character_services.open,
                // Zircon cycles the minimap with V (shown, dimmed, hidden)
                // and opens the big map with B; B is storage here, so the
                // big map takes X.
                'v' => self.minimap.cycle(),
                'x' => self.minimap.big_open = !self.minimap.big_open,
                'r' => self.windows.ranking.open = !self.windows.ranking.open,
                // Zircon's BlockListWindow default; it opens the same
                // dialog our friends window stands in for.
                'f' => self.friends_window.open = !self.friends_window.open,
                'n' => self.windows.menu.menu_open = !self.windows.menu.menu_open,
                'h' => self.windows.menu.help_open = !self.windows.menu.help_open,
                'a' => self.windows.menu.auto_potion_open = !self.windows.menu.auto_potion_open,
                't' => {
                    if let Some(c) = conn {
                        c.send(ClientMessage::TradeRequest);
                    }
                }
                _ => {}
            }
        }
        if let Some(slot) = self.input.digit {
            self.belt_key(slot, now, conn, true);
        }
        if self.input.tab {
            if let Some(c) = conn {
                c.send(ClientMessage::PickUp);
            }
        }
        if let Some(f) = self.input.fkey {
            self.function_key(f, now, width, height, conn);
        }
        // Escape closes what is open; with a clear screen it asks to leave,
        // as Zircon's `ExitDialog` does.
        if self.input.escape && !self.windows_open_last_frame {
            self.windows.menu.exit_open = true;
        }
        if self.input.escape && self.windows_open_last_frame {
            self.windows.menu.close_all();
            self.windows.inventory_open = false;
            self.windows.character_open = false;
            self.windows.skills_open = false;
            self.windows.group_open = false;
            self.windows.storage_open = false;
            self.minimap.big_open = false;
            self.windows.guild_open = false;
            self.windows.mail_open = false;
            self.windows.companion_open = false;
            self.windows.ranking.open = false;
            self.friends_window.open = false;
            self.windows.market.open = false;
            self.fortune.open = false;
            self.dungeon_codex.open = false;
            self.wardrobe.open = false;
            self.chat_options.open = false;
            self.character_services.open = false;
            if self.trade.is_some() {
                if let Some(c) = conn {
                    c.send(ClientMessage::TradeClose);
                }
            }
            if self.windows.npc.take().is_some() {
                if let Some(c) = conn {
                    c.send(ClientMessage::NpcClose);
                }
            }
        }
        if self.input.lmb_pressed || self.input.rmb_pressed {
            self.goal = None;
        }
        // Keep clicks over UI for the UI, while clicks outside open panels
        // still reach the world.
        if self.ui_hover_last_frame {
            return;
        }
        let Some(user) = self.user() else {
            return;
        };
        if user.dead || user.poison_kinds & 8 != 0 {
            return;
        }
        let user_loc = user.location;
        let user_dir = user.direction;
        let view = View::new(width, height, Some(user));

        // Interacting with a goal object once close enough.
        if let Some(gid) = self.goal {
            match self.objects.get(&gid) {
                Some(g) if user_loc.distance(g.location) <= if g.room_door { 2 } else { 1 } => {
                    if let Some(c) = conn {
                        if g.is_item() {
                            c.send(ClientMessage::PickUpItem { id: gid });
                        } else if g.is_npc() {
                            c.send(ClientMessage::NpcCall { id: gid });
                        } else if g.room_door {
                            c.send(ClientMessage::RoomDoorToggle { id: gid });
                        }
                    }
                    self.goal = None;
                    return;
                }
                Some(g) if now >= self.action_time && now >= self.move_time => {
                    let target = g.location;
                    self.step_toward(now, user_loc, target, false, conn);
                    return;
                }
                Some(_) => return,
                None => self.goal = None,
            }
        }

        if self.input.lmb_pressed {
            if let Some(target) = self.hovered.and_then(|id| self.objects.get(&id)) {
                if target.is_npc() || target.is_item() || target.room_door {
                    self.goal = Some(target.id);
                    return;
                }
            }
        }
        if !(self.lmb || self.rmb) {
            return;
        }

        // Fishing: a click reels; Escape or a move (below) cancels.
        if let Some(f) = &mut self.fishing {
            if self.input.lmb_pressed {
                f.reeled = true;
            }
            if self.input.escape {
                if let Some(c) = conn {
                    c.send(ClientMessage::FishingCast {
                        state: mir_proto::fishing_state::CANCEL,
                        direction: f.direction,
                        float: f.float,
                        caught: false,
                    });
                }
                self.fishing = None;
            }
            return;
        }
        // Gathering: a click on an unwalkable cell swings a pickaxe at an
        // adjacent wall or casts a rod (or `ZIRCON_DEV_FISHING`) at water.
        if self.input.lmb_pressed && self.hovered.is_none() {
            let cell = view.cell_at(self.mouse.0, self.mouse.1);
            let walkable = self
                .map
                .as_ref()
                .is_some_and(|m| m.is_walkable(cell.x, cell.y));
            let weapon_effect = self
                .equipment
                .get(mir_proto::slot::WEAPON)
                .and_then(|c| c.as_ref())
                .and_then(|i| self.catalog.get(i.info))
                .map(|d| d.effect)
                .unwrap_or(0);
            let distance = user_loc.distance(cell);
            let can_fish = weapon_effect == 82 || std::env::var_os("ZIRCON_DEV_FISHING").is_some();
            if !walkable && distance >= 1 && now >= self.action_time {
                if weapon_effect == 5 && distance == 1 {
                    self.swing_pickaxe(cell, now, conn);
                    return;
                }
                if can_fish && distance <= 4 {
                    self.cast_rod(cell, now, conn);
                    return;
                }
            }
        }
        if self.lmb && self.user().is_some_and(|u| !u.mounted()) {
            if let Some(cell) = self
                .hovered
                .and_then(|id| self.objects.get(&id))
                .filter(|o| {
                    o.dead
                        && matches!(
                            o.appearance,
                            Appearance::Monster {
                                skeleton: false,
                                ..
                            }
                        )
                })
                .map(|o| o.location)
                .filter(|p| user_loc.distance(*p) <= 1)
            {
                if now >= self.action_time {
                    self.swing_harvest(cell, now, conn);
                }
                return;
            }
        }
        // Attack a hovered monster in melee range.
        if self.lmb {
            if let Some(target) = self.hovered.and_then(|id| self.objects.get(&id)) {
                if self.attackable(target) && user_loc.distance(target.location) <= 1 {
                    if now >= self.action_time && now >= self.attack_time {
                        let direction = Direction::from_points(user_loc, target.location);
                        self.action_time = now + ATTACK_TIME;
                        // Zircon: the swing delay shortens with attack speed.
                        self.attack_time = now + attack_delay(self.stats.attack_speed as i64);
                        // Zircon `UserObject` priority: lotus arm, Slaying,
                        // stances, Destructive Surge, then charged power attacks.
                        let mut attack_magic = self.armed_lotus.or(self.auto_charged);
                        if self.slaying_ready {
                            attack_magic = Some(magic_type::SLAYING);
                        }
                        if self.toggles.contains(&magic_type::THRUSTING) {
                            attack_magic = Some(magic_type::THRUSTING);
                        }
                        if self.toggles.contains(&magic_type::HALF_MOON) {
                            attack_magic = Some(magic_type::HALF_MOON);
                        }
                        if self.toggles.contains(&magic_type::DESTRUCTIVE_SURGE) {
                            attack_magic = Some(magic_type::DESTRUCTIVE_SURGE);
                        }
                        if self.toggles.contains(&magic_type::FLAME_SPLASH)
                            && attack_magic.is_none()
                        {
                            attack_magic = Some(magic_type::FLAME_SPLASH);
                        }
                        if let Some(c) = self.charged {
                            attack_magic = Some(c);
                        }
                        let action = attack_action(attack_magic);
                        if let Some(u) = self.user_mut() {
                            u.queue.clear();
                            u.enqueue(Queued {
                                action,
                                direction,
                                location: user_loc,
                                distance: 0,
                            });
                        }
                        if let Some(m) = attack_magic {
                            if let Some(uid) = self.user {
                                if let Some(e) = effects::attack_effect(
                                    m,
                                    uid,
                                    direction.index(),
                                    now,
                                    self.stats.attack_element,
                                ) {
                                    self.effects.push(e);
                                }
                            }
                        }
                        if let Some(c) = conn {
                            c.send(ClientMessage::Attack {
                                direction,
                                attack_magic,
                            });
                        }
                    }
                    return;
                }
            }
        }

        let target = view.cell_at(self.mouse.0, self.mouse.1);
        if target == user_loc || now < self.action_time || now < self.move_time {
            return;
        }
        let run = self.rmb && user_loc.distance(target) >= 2;
        let _ = user_dir;
        self.step_toward(now, user_loc, target, run, conn);
    }

    /// Spell slots 1..12: bind in the skill window, toggle a stance, or cast.
    /// Digit key: link the carried/hovered bag item to the belt slot, else
    /// use what the slot links to (Zircon `UseBelt01..10`).
    pub(super) fn belt_key(
        &mut self,
        slot: u8,
        now: u64,
        conn: Option<&Connection>,
        confirm_use: bool,
    ) {
        let source = match self.windows.carrying {
            Some((mir_proto::Grid::Inventory, s)) => Some(s),
            _ => self.windows.hover_inventory,
        };
        if let Some(s) = source {
            if self.inventory.get(s as usize).map(|i| i.is_some()) == Some(true) {
                self.belt_key_pending = None;
                let link = crate::windows::link_for(&self.catalog, &self.inventory, s, slot);
                self.windows.carrying = None;
                self.apply_belt_link(link);
                if let Some(c) = conn {
                    c.send(ClientMessage::BeltLink {
                        slot: link.slot,
                        info: link.info,
                        item: link.item,
                    });
                }
                return;
            }
        }
        let Some(link) = self.belt.get(slot as usize).copied() else {
            return;
        };
        if let Some(inv) = crate::windows::belt_inventory_slot(&self.inventory, &link) {
            if !confirm_use || confirm_belt_key(&mut self.belt_key_pending, slot, now) {
                self.try_use_item(inv, now, conn);
            } else {
                self.say(
                    format!(
                        "Press {} again to use belt item.",
                        if slot == 9 { 0 } else { slot + 1 }
                    ),
                    now,
                );
            }
        }
    }

    pub(super) fn apply_belt_link(&mut self, link: BeltLink) {
        if let Some(l) = self.belt.get_mut(link.slot as usize) {
            *l = link;
        }
    }

    /// Zircon `DXItemCell.UseItem` for consumables: the client-side lock is
    /// `max(250, Durability)` ms; the server enforces its own.
    pub(super) fn try_use_item(&mut self, slot: u8, now: u64, conn: Option<&Connection>) {
        let Some(item) = self.inventory.get(slot as usize).cloned().flatten() else {
            return;
        };
        let Some(def) = self.catalog.get(item.info) else {
            return;
        };
        if def.item_type == mir_proto::item_type::CONSUMABLE {
            if now < self.use_item_time {
                return;
            }
            self.use_item_time = now + use_item_lock(def.durability);
        }
        if let Some(c) = conn {
            self.sfx_item_slot(Grid::Inventory, slot);
            c.send(ClientMessage::ItemUse { slot });
        }
    }

    pub(super) fn function_key(
        &mut self,
        f: u8,
        now: u64,
        width: i32,
        height: i32,
        conn: Option<&Connection>,
    ) {
        // Binding: hovering a learned skill's icon in the skill window.
        if let Some(magic) = self.windows.hover_magic {
            if self.magics.iter().any(|m| m.magic == magic) {
                self.apply_magic_key(magic, f);
                if let Some(c) = conn {
                    c.send(ClientMessage::MagicKey { magic, key: f });
                }
            }
            return;
        }
        let Some(magic) = self.magics.iter().find(|m| m.key == f).map(|m| m.magic) else {
            self.say(format!("No spell bound to F{f}."), now);
            return;
        };
        self.use_skill(magic, now, width, height, conn);
    }

    pub(super) fn apply_magic_key(&mut self, magic: u16, key: u8) {
        if key != 0 {
            for known in &mut self.magics {
                if known.key == key {
                    known.key = 0;
                }
            }
        }
        if let Some(known) = self.magics.iter_mut().find(|known| known.magic == magic) {
            known.key = key;
        }
    }

    /// Use a learned skill: toggle, charge, arm or cast it.
    pub(super) fn use_skill(
        &mut self,
        magic: u16,
        now: u64,
        width: i32,
        height: i32,
        conn: Option<&Connection>,
    ) {
        let Some(m) = self.magics.iter().find(|m| m.magic == magic).cloned() else {
            return;
        };
        let Some(def) = self.catalog.magic(magic).cloned() else {
            return;
        };
        if magic_type::is_passive(magic) {
            self.say(format!("{} works on its own.", def.name), now);
            return;
        }
        if magic_type::is_toggle(magic) {
            let on = !self.toggles.contains(&magic);
            if let Some(c) = conn {
                c.send(ClientMessage::MagicToggle { magic, on });
            }
            return;
        }
        if magic_type::is_charge(magic) {
            if def.cost(m.level) > self.stats.mp {
                self.say("Not enough mana.".into(), now);
                return;
            }
            if let Some(c) = conn {
                c.send(ClientMessage::MagicToggle { magic, on: true });
            }
            return;
        }
        if magic_type::is_armed(magic) {
            if magic == magic_type::KARMA && !self.buffs.iter().any(|(k, _)| *k == buff_type::CLOAK)
            {
                self.say("Karma needs the cloak.".into(), now);
                return;
            }
            if self.armed_lotus != Some(magic) {
                self.armed_lotus = Some(magic);
                self.say(format!("{} is ready.", def.name), now);
            }
            return;
        }
        if !magic_type::is_castable(magic) {
            self.say(format!("{} is not implemented yet.", def.name), now);
            return;
        }
        let Some(user) = self.user() else {
            return;
        };
        if user.dead || user.poison_kinds & 8 != 0 {
            return;
        }
        if user.mounted() {
            self.say("You cannot cast while riding.".into(), now);
            return;
        }
        let user_loc = user.location;
        if self.stats.level < def.need_level[0] {
            self.say(
                format!("{} needs level {}.", def.name, def.need_level[0]),
                now,
            );
            return;
        }
        let mobile_support = magic_type::is_mobile_support_cast(magic)
            && (matches!(user.action, Action::Walking | Action::Running) || self.lmb || self.rmb);
        if now < self.magic_time {
            return;
        }
        if now < self.action_time && !mobile_support {
            self.say(
                format!("Finish your action before casting {}.", def.name),
                now,
            );
            return;
        }
        if self
            .cooldowns
            .get(&magic)
            .map(|t| now < *t)
            .unwrap_or(false)
        {
            self.say(format!("{} is cooling down.", def.name), now);
            return;
        }
        if def.cost(m.level) > self.stats.mp {
            self.say("Not enough mana.".into(), now);
            return;
        }
        let view = View::new(width, height, Some(user));
        let mouse_cell = view.cell_at(self.mouse.0, self.mouse.1);
        let hovered = self.hovered.and_then(|id| self.objects.get(&id));
        if magic == magic_type::MAGIC_SHIELD
            && self
                .buffs
                .iter()
                .any(|(k, _)| *k == buff_type::MAGIC_SHIELD)
        {
            self.say("You are already shielded.".into(), now);
            return;
        }
        let self_cast = magic_type::is_self_cast(magic);
        let target = match magic {
            magic_type::CORPSE_EXPLODER | magic_type::SUMMON_DEAD => {
                hovered.filter(|o| o.is_monster() && o.dead).map(|o| o.id)
            }
            magic_type::HEAL => hovered
                .filter(|o| o.is_player())
                .map(|o| o.id)
                .or(self.user),
            m if magic_type::needs_target(m) => {
                hovered.filter(|o| self.attackable(o)).map(|o| o.id)
            }
            _ => None,
        };
        // Ground casts and lines use the mouse cell; self casts the own cell.
        let target_loc = if self_cast {
            user_loc
        } else {
            target
                .and_then(|t| self.objects.get(&t))
                .map(|o| o.location)
                .unwrap_or(mouse_cell)
        };
        if target_loc.distance(user_loc) > mir_proto::MAGIC_RANGE {
            self.say("Too far away.".into(), now);
            return;
        }
        let direction = if magic_type::is_stance_cast(magic) {
            Direction::Down
        } else if magic == magic_type::SHOULDER_DASH
            || magic == magic_type::COMBAT_KICK
            || magic == magic_type::RAKE
            || magic_type::is_directional(magic)
            || magic_type::is_line(magic)
        {
            if mouse_cell == user_loc {
                user.direction
            } else {
                Direction::from_points(user_loc, mouse_cell)
            }
        } else if target_loc == user_loc {
            user.direction
        } else {
            Direction::from_points(user_loc, target_loc)
        };
        self.magic_time = now + mir_proto::MAGIC_DELAY;
        if !mobile_support {
            self.action_time = now + 600;
        }
        if magic == magic_type::SHOULDER_DASH {
            // No cast animation: the server streams dash steps.
            if let Some(c) = conn {
                c.send(ClientMessage::Magic {
                    magic,
                    direction,
                    target: None,
                    location: user_loc,
                });
            }
            return;
        }
        if !mobile_support {
            let action = cast_action(magic);
            if let Some(u) = self.user_mut() {
                u.queue.clear();
                u.enqueue(Queued {
                    action,
                    direction,
                    location: user_loc,
                    distance: 0,
                });
            }
        }
        if let Some(c) = conn {
            c.send(ClientMessage::Magic {
                magic,
                direction,
                target,
                location: target_loc,
            });
        }
    }

    /// One walk/run step toward `target`, turning if blocked.
    /// Swing the pickaxe at an adjacent wall cell (Zircon `Mining`).
    pub(super) fn swing_pickaxe(&mut self, cell: Point, now: u64, conn: Option<&Connection>) {
        let Some(user_loc) = self.user().map(|u| u.location) else {
            return;
        };
        let direction = Direction::from_points(user_loc, cell);
        self.action_time = now + ATTACK_TIME;
        self.attack_time = now + attack_delay(self.stats.attack_speed as i64).max(800);
        if let Some(u) = self.user_mut() {
            u.queue.clear();
            u.enqueue(Queued {
                action: Action::Mining,
                direction,
                location: user_loc,
                distance: 0,
            });
        }
        if let Some(c) = conn {
            c.send(ClientMessage::Mining { direction });
        }
    }

    /// Swing at a corpse in front (Zircon `Harvest`). The server works out
    /// which bodies are in reach and what they owe; the client only picks a
    /// facing and plays the swing.
    pub(super) fn swing_harvest(&mut self, cell: Point, now: u64, conn: Option<&Connection>) {
        let Some(user_loc) = self.user().map(|u| u.location) else {
            return;
        };
        let direction = Direction::from_points(user_loc, cell);
        self.action_time = now + HARVEST_TIME;
        if let Some(u) = self.user_mut() {
            u.queue.clear();
            u.enqueue(Queued {
                action: Action::Harvest,
                direction,
                location: user_loc,
                distance: 0,
            });
        }
        if let Some(c) = conn {
            c.send(ClientMessage::Harvest { direction });
        }
    }

    /// The nearest corpse still showing flesh. The server checks ownership;
    /// bones can be skipped because this viewer has nothing left to collect.
    pub(super) fn nearest_corpse(&self, radius: i32) -> Option<Point> {
        let user_loc = self.user()?.location;
        let mut best: Option<(i32, Point)> = None;
        for o in self.objects.values() {
            if !o.dead {
                continue;
            }
            let fleshy = matches!(
                &o.appearance,
                mir_proto::Appearance::Monster { skeleton, .. } if !*skeleton
            );
            if !fleshy {
                continue;
            }
            let d = o.location.distance(user_loc);
            if d > radius {
                continue;
            }
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, o.location));
            }
        }
        best.map(|(_, p)| p)
    }

    /// Cast the rod at a water cell (Zircon `FishingCast`).
    pub(super) fn cast_rod(&mut self, cell: Point, now: u64, conn: Option<&Connection>) {
        let Some(user_loc) = self.user().map(|u| u.location) else {
            return;
        };
        let direction = Direction::from_points(user_loc, cell);
        self.action_time = now + ATTACK_TIME;
        self.fishing = Some(FishingUi {
            direction,
            float: cell,
            found: false,
            reeled: false,
            next_cast: now + 1600,
            points: 0,
            required: 50,
        });
        if let Some(c) = conn {
            c.send(ClientMessage::FishingCast {
                state: mir_proto::fishing_state::CAST,
                direction,
                float: cell,
                caught: false,
            });
        }
    }

    /// Nearest unwalkable cell within `radius` of the player, closest first.
    pub(super) fn nearest_wall(&self, radius: i32) -> Option<Point> {
        let user_loc = self.user()?.location;
        let map = self.map.as_ref()?;
        let mut best: Option<(i32, Point)> = None;
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                let p = Point::new(user_loc.x + dx, user_loc.y + dy);
                if p == user_loc
                    || p.x < 0
                    || p.y < 0
                    || p.x >= map.width as i32
                    || p.y >= map.height as i32
                    || map.is_walkable(p.x, p.y)
                {
                    continue;
                }
                // Only walls with a walkable neighbour can be worked.
                let reachable = Direction::ALL
                    .iter()
                    .any(|d| map.is_walkable(p.step(*d, 1).x, p.step(*d, 1).y));
                if !reachable {
                    continue;
                }
                let d = p.distance(user_loc);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, p));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    pub(super) fn step_toward(
        &mut self,
        now: u64,
        user_loc: Point,
        target: Point,
        run: bool,
        conn: Option<&Connection>,
    ) {
        let user_dir = self.user().map(|u| u.direction).unwrap_or(Direction::Down);
        let wanted = Direction::from_points(user_loc, target);
        // Riding adds a step to a run (Zircon MapControl).
        let mounted = self.user().is_some_and(|u| u.mounted());
        let steps = match (run, mounted) {
            (true, true) => 3,
            (true, false) => 2,
            _ => 1,
        };
        let candidates = [
            wanted,
            wanted.rotate(-1),
            wanted.rotate(1),
            wanted.rotate(-2),
            wanted.rotate(2),
        ];
        let chosen = candidates
            .iter()
            .copied()
            .find(|d| (1..=steps).all(|i| !self.blocked(user_loc.step(*d, i))));
        match chosen {
            Some(direction) => {
                let to = user_loc.step(direction, steps);
                self.action_time = now + MOVE_TIME;
                self.move_time = now + MOVE_TIME;
                if let Some(u) = self.user_mut() {
                    u.queue.clear();
                    u.enqueue(Queued {
                        action: if run {
                            Action::Running
                        } else {
                            Action::Walking
                        },
                        direction,
                        location: to,
                        distance: steps,
                    });
                }
                if let Some(c) = conn {
                    c.send(ClientMessage::Move { direction, run });
                }
            }
            None => {
                if user_dir != wanted {
                    self.action_time = now + TURN_TIME;
                    if let Some(u) = self.user_mut() {
                        u.queue.clear();
                        u.enqueue(Queued {
                            action: Action::Standing,
                            direction: wanted,
                            location: user_loc,
                            distance: 0,
                        });
                    }
                    if let Some(c) = conn {
                        c.send(ClientMessage::Turn { direction: wanted });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod magic_key_tests {
    use super::*;

    #[test]
    fn reassignment_and_unbinding_update_local_shortcuts() {
        let mut game = Game::new(
            Assets::new(std::env::temp_dir().join("zircon-shortcut-test-assets")),
            ItemCatalog::empty(),
        );
        game.magics = vec![
            MagicSummary {
                magic: 1,
                level: 0,
                experience: 0,
                key: 1,
            },
            MagicSummary {
                magic: 2,
                level: 0,
                experience: 0,
                key: 2,
            },
        ];

        game.apply_magic_key(2, 1);
        assert_eq!(
            game.magics.iter().map(|m| m.key).collect::<Vec<_>>(),
            vec![0, 1]
        );
        game.apply_magic_key(2, 0);
        assert_eq!(
            game.magics.iter().map(|m| m.key).collect::<Vec<_>>(),
            vec![0, 0]
        );
    }

    #[test]
    fn belt_number_needs_two_distinct_presses() {
        let mut pending = None;
        assert!(!confirm_belt_key(&mut pending, 1, 100));
        assert_eq!(pending, Some((1, 100)));
        assert!(confirm_belt_key(&mut pending, 1, 400));
        assert_eq!(pending, None);
        assert!(!confirm_belt_key(&mut pending, 1, 1000));
        assert!(!confirm_belt_key(&mut pending, 2, 1100));
        assert!(!confirm_belt_key(&mut pending, 2, 1600));
    }

    #[test]
    fn spell_bar_clicks_match_visible_slots() {
        assert_eq!(spell_bar_slot((10.0, 628.0), 768), Some(1));
        assert_eq!(spell_bar_slot((47.0, 628.0), 768), Some(2));
        assert_eq!(spell_bar_slot((452.0, 628.0), 768), Some(12));
        assert_eq!(spell_bar_slot((46.0, 628.0), 768), None);
    }
}
