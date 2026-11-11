use super::*;

fn shop_price(base: u64, exchange_rate: f64) -> u64 {
    (base as f64 * exchange_rate).max(1.0) as u64
}

impl World {
    /// Keep a dialog only while its NPC is still visible to this player.
    pub(super) fn npc_session(&mut self, id: ObjectId) -> Option<(ObjectId, i32)> {
        let player = self.objects.get(&id)?;
        let session = player.player()?.npc?;
        let valid = self.objects.get(&session.0).is_some_and(|npc| {
            !player.dead
                && npc.map == player.map
                && npc.location.distance(player.location) <= MAX_VIEW_RANGE
                && matches!(&npc.kind, Kind::Npc(data) if self.npc_visible_to(data.info, player.player().unwrap()))
        });
        if !valid {
            self.npc_close(id);
            self.send_to(id, ServerMessage::NpcClose);
            return None;
        }
        Some(session)
    }

    pub fn npc_call(&mut self, id: ObjectId, npc: ObjectId) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let (map, loc) = (o.map, o.location);
        let entry = match self.objects.get(&npc) {
            Some(n) if n.map == map && n.location.distance(loc) <= MAX_VIEW_RANGE => {
                match &n.kind {
                    Kind::Npc(d) => d.entry_page,
                    _ => return,
                }
            }
            _ => return,
        };
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.npc = None;
        }
        self.npc_run_page(id, npc, entry);
    }

    pub fn npc_button(&mut self, id: ObjectId, button: i32) {
        let Some((npc, page)) = self.npc_session(id) else {
            return;
        };
        let Some(def) = self.data.npc_pages.get(&page) else {
            return;
        };
        let Some((_, dest)) = def.buttons.iter().find(|(b, d)| *b == button && *d != 0) else {
            return;
        };
        let dest = *dest;
        self.npc_run_page(id, npc, dest);
    }

    pub fn npc_close(&mut self, id: ObjectId) {
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.npc = None;
        }
    }

    /// Zircon `NPCObject.NPCCall`: walk pages through checks and actions until
    /// one with text is reached.
    pub(super) fn npc_run_page(&mut self, id: ObjectId, npc: ObjectId, mut page: i32) {
        for _ in 0..20 {
            if page == 0 {
                self.npc_close(id);
                self.send_to(id, ServerMessage::NpcClose);
                return;
            }
            let Some(def) = self.data.npc_pages.get(&page).cloned() else {
                self.npc_close(id);
                self.send_to(id, ServerMessage::NpcClose);
                return;
            };
            // Scripted pages: `on_open` may redirect or cancel.
            self.npc_script = (!def.script_file.trim().is_empty()).then(|| def.script_file.clone());
            if let Some(nav) = self.script_on_open(id) {
                match nav {
                    None => {
                        self.npc_script = None;
                        self.npc_close(id);
                        self.send_to(id, ServerMessage::NpcClose);
                        return;
                    }
                    Some(target) => {
                        page = target;
                        continue;
                    }
                }
            }
            let mut failed = None;
            for c in &def.checks {
                if !self.npc_check(id, c) {
                    failed = Some(c.fail_page);
                    break;
                }
            }
            if let Some(fail) = failed {
                page = fail;
                continue;
            }
            for a in &def.actions {
                self.npc_action(id, a);
            }
            if def.say.trim().is_empty() {
                if def.success_page != 0 {
                    page = def.success_page;
                    continue;
                }
                self.npc_close(id);
                self.send_to(id, ServerMessage::NpcClose);
                return;
            }
            let Some(currency) = self.npc_page_currency(&def).cloned() else {
                self.npc_close(id);
                self.send_to(id, ServerMessage::NpcClose);
                return;
            };
            let goods = def
                .goods
                .iter()
                .filter_map(|(item, rate)| {
                    let d = self.data.items.get(item)?;
                    let base = ((d.price as f64 * rate).round() as u64).max(1);
                    Some(Good {
                        info: *item,
                        price: shop_price(base, currency.exchange_rate),
                    })
                })
                .collect();
            if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                p.npc = Some((npc, page));
            }
            let say = self.npc_page_say(id, &def);
            let _ = parse_dialog(&say);
            self.send_to(
                id,
                ServerMessage::NpcResponse {
                    npc,
                    page,
                    say,
                    dialog_type: def.dialog_type,
                    currency: currency.name,
                    goods,
                    sell_types: def.types.clone(),
                    quests: self.npc_quests(id, npc),
                },
            );
            match def.dialog_type {
                4 => self.send_refine_list(id),
                5 => self.send_companion_shop(id),
                _ => {}
            }
            return;
        }
    }

    fn npc_page_say(&self, id: ObjectId, page: &crate::data::NpcPageDef) -> String {
        let mut say = page.say.clone();
        let Some(player) = self.objects.get(&id).and_then(|o| o.player()) else {
            return say;
        };
        for value in &page.values {
            // This pack uses Field.Name and Field.FameCost on the Fame page.
            let field = match (value.value_type, value.field_type) {
                (3, 1) => Some(player.name.clone()),
                (3, 100) => self
                    .next_fame_title(player.fame)
                    .map(|f| f.cost.to_string()),
                _ => None,
            };
            let prefix = format!("<{}:", value.id);
            while let Some(start) = say.find(&prefix) {
                let fallback_start = start + prefix.len();
                let Some(end_offset) = say[fallback_start..].find('>') else {
                    break;
                };
                let end = fallback_start + end_offset;
                let replacement = field
                    .clone()
                    .unwrap_or_else(|| say[fallback_start..end].into());
                say.replace_range(start..=end, &replacement);
            }
        }
        say
    }

    pub(super) fn npc_check(&mut self, id: ObjectId, c: &crate::data::NpcCheckDef) -> bool {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return false;
        };
        let cmp = |op: i32, a: i64, b: i64| match op {
            0 => a == b,
            1 => a != b,
            2 => a < b,
            3 => a <= b,
            4 => a > b,
            5 => a >= b,
            _ => true,
        };
        // Zircon `Config.RedPoint`.
        const RED_POINT: i64 = 200;
        let weapon = p.bag.equipment.get(slot::WEAPON).and_then(|w| w.as_ref());
        let equal = c.operator == 0;
        match c.check_type {
            0 => cmp(c.operator, p.level as i64, c.int1 as i64),
            1 => cmp(c.operator, p.class.mir_class() as i64, c.int1 as i64),
            // Gender has no server case in Zircon: always passes.
            2 => true,
            3 => cmp(c.operator, p.bag.gold as i64, c.int1 as i64),
            4 => c.item1 == 0 || cmp(c.operator, p.bag.count_of(c.item1) as i64, c.int1 as i64),
            // PK points: nobody is red in the prototype (Redemption 0).
            5 => {
                let threshold = if c.int1 == 0 {
                    RED_POINT
                } else {
                    c.int1 as i64
                };
                cmp(c.operator, 0, threshold)
            }
            6 => weapon.is_some() == equal,
            // Weapon level: successful refines (Zircon `UserItem.Level`).
            7 => weapon.is_some_and(|w| cmp(c.operator, w.level as i64, c.int1 as i64)),
            // Weapon element: value of the weapon's element (added stats plus
            // the item's own), only counted when it matches `int1` (0 = any).
            8 => weapon.is_some_and(|w| {
                let def = self.data.items.get(&w.info);
                let added = |stat: i32| w.bonus(stat);
                let base = |stat: i32| def.map(|d| d.stat(stat)).unwrap_or(0);
                let elem_stat = |v: i32| -> Option<i32> {
                    (1..=7)
                        .contains(&v)
                        .then(|| super::refine::FIRE_ATTACK_STAT + (v - 1) * 2)
                };
                // The weapon's element: WeaponElement stat, else the first
                // elemental attack stat it carries.
                let element = elem_stat(added(super::refine::WEAPON_ELEMENT_STAT))
                    .or_else(|| elem_stat(base(super::refine::WEAPON_ELEMENT_STAT)))
                    .or_else(|| {
                        (0..7)
                            .map(|i| super::refine::FIRE_ATTACK_STAT + i * 2)
                            .find(|s| added(*s) > 0 || base(*s) > 0)
                    });
                let value = match (element, elem_stat(c.int1)) {
                    (Some(e), None) => added(e) + base(e),
                    (Some(e), Some(want)) if e == want => added(e) + base(e),
                    _ => 0,
                };
                cmp(c.operator, value as i64, c.int2 as i64)
            }),
            // Readiness is separate from the permanent NonRefinable flag.
            9 => weapon.is_some_and(|w| w.refinable && !w.non_refinable) == equal,
            // Weapon added stat `stat1` compared with `int1`.
            16 => weapon.is_some_and(|w| {
                let v = w.bonus(c.stat1);
                cmp(c.operator, v as i64, c.int1 as i64)
            }),
            // Horse type owned (NPC pages compare the HorseType value).
            10 => cmp(c.operator, p.horse as i64, c.int1 as i64),
            // Marriage: Equal means married; anything else means single.
            11 => p.partner.is_some() == equal,
            // Wedding ring worn on the left ring finger.
            12 => self.wears_wedding_ring(p) == equal,
            13 => {
                c.item1 == 0
                    || p.bag
                        .can_gain(&self.data, c.item1, c.int1.max(1) as u32, p.max_bag)
            }
            // Weapon reset cooldown: never on cooldown.
            14 => weapon.is_some() && equal,
            15 => {
                let roll = self.rng.random_range(0..c.int1.max(1)) as i64;
                cmp(c.operator, roll, c.int2 as i64)
            }
            // Currency by name (case-insensitive); unknown names pass like Zircon's `continue`.
            17 => match self.currency_by_name(&c.string1) {
                Some(cur) if cur.name.eq_ignore_ascii_case("gold") => {
                    cmp(c.operator, p.bag.gold as i64, c.int1 as i64)
                }
                Some(cur) => cmp(
                    c.operator,
                    p.currencies.get(&cur.index).copied().unwrap_or(0),
                    c.int1 as i64,
                ),
                None => true,
            },
            // Roll result: missing means failure.
            18 => match p.npc_roll {
                Some(r) => cmp(c.operator, r as i64, c.int1 as i64),
                None => false,
            },
            // Data list membership: `<String1>_NameList` keyed by the data type.
            19 => match self.npc_data_key(p, c.int1) {
                Some(key) => self
                    .npc_store
                    .lists
                    .get(&format!("{}_NameList", c.string1))
                    .map(|l| l.contains_key(&key))
                    .unwrap_or(false),
                None => true,
            },
            // Data value: missing rows count as 0, compared with IntParameter2.
            20 => match self.npc_data_key(p, c.int1) {
                Some(key) => {
                    let v = self
                        .npc_store
                        .lists
                        .get(&c.string1)
                        .and_then(|l| l.get(&key))
                        .copied()
                        .unwrap_or(0);
                    cmp(c.operator, v, c.int2 as i64)
                }
                None => true,
            },
            // Fame: the next title exists and its Fame Point cost is covered.
            21 => self.fame_check(id),
            // Lua: `StringParameter1` names a function in the page's script.
            22 => self.script_check(id, &c.string1),
            _ => true,
        }
    }

    pub(super) fn npc_action(&mut self, id: ObjectId, a: &crate::data::NpcActionDef) {
        match a.action_type {
            0 => {
                // Teleport to MapParameter1 at (int1, int2) or a random cell.
                let Some(map) = self.data.maps.get(&a.map1).map(|m| m.index) else {
                    return;
                };
                if self.ensure_map(map).is_err() {
                    return;
                }
                let target = if a.int1 == 0 && a.int2 == 0 {
                    self.random_walkable(map)
                } else {
                    Some(Point::new(a.int1, a.int2))
                };
                if let Some(t) = target {
                    self.change_map(id, map, t);
                }
            }
            1 | 2 => {
                if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                    if a.action_type == 1 {
                        p.bag.gold += a.int1.max(0) as u64;
                    } else {
                        p.bag.gold = p.bag.gold.saturating_sub(a.int1.max(0) as u64);
                    }
                }
                self.send_gold(id);
            }
            3 => {
                let count = a.int1.max(1) as u32;
                if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                    let mut next = p.next_item_id;
                    let changes = p.bag.gain(&self.data, a.item1, count, &mut next);
                    p.next_item_id = next;
                    self.send_changes(id, changes);
                }
            }
            4 => {
                let count = a.int1.max(1) as u32;
                if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                    let changes = p.bag.take_info(a.item1, count);
                    self.send_changes(id, changes);
                }
            }
            // Message: no server case in Zircon either.
            7 => {}
            // Rebirth: only at level 86 + rebirths; back to level 1 with 1/200 exp.
            14 => {
                let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
                    return;
                };
                if p.level < 86 + p.rebirth {
                    return;
                }
                p.level = 1;
                let kept = (p.experience as f64 + p.experience_fraction) / 200.0;
                p.experience = kept.floor() as u64;
                p.experience_fraction = kept.fract();
                p.rebirth += 1;
                self.refresh_stats(id, true);
                self.send_player_stats(id);
            }
            // Currencies by name.
            15 | 16 => {
                let Some(cur) = self.currency_by_name(&a.string1).cloned() else {
                    return;
                };
                let delta = if a.action_type == 15 {
                    a.int1 as i64
                } else {
                    -(a.int1 as i64)
                };
                if cur.name.eq_ignore_ascii_case("gold") {
                    if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                        p.bag.gold = (p.bag.gold as i64 + delta).max(0) as u64;
                    }
                    self.send_gold(id);
                } else {
                    if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                        let e = p.currencies.entry(cur.index).or_insert(0);
                        *e += delta;
                    }
                    self.send_currencies(id);
                }
            }
            // Data lists and values (Zircon `GameNPCList`).
            17..=21 => {
                let Some(key) = self
                    .objects
                    .get(&id)
                    .and_then(|o| o.player())
                    .and_then(|p| self.npc_data_key(p, a.int1))
                else {
                    return;
                };
                let list_cat = format!("{}_NameList", a.string1);
                match a.action_type {
                    17 => {
                        self.npc_store
                            .lists
                            .entry(list_cat)
                            .or_default()
                            .entry(key)
                            .or_insert(0);
                    }
                    18 => {
                        if let Some(l) = self.npc_store.lists.get_mut(&list_cat) {
                            l.remove(&key);
                        }
                    }
                    19 => {
                        self.npc_store.lists.remove(&list_cat);
                    }
                    20 => {
                        let e = self
                            .npc_store
                            .lists
                            .entry(a.string1.clone())
                            .or_default()
                            .entry(key)
                            .or_insert(0);
                        *e += a.int2 as i64;
                    }
                    _ => {
                        self.npc_store
                            .lists
                            .entry(a.string1.clone())
                            .or_default()
                            .insert(key, a.int2 as i64);
                    }
                }
                self.npc_store.save();
            }
            // ChangeHorse: the account now owns this horse type.
            6 => {
                if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                    p.horse = a.int1.clamp(0, 6) as u8;
                }
                self.remove_mount(id);
                self.refresh_stats(id, false);
                self.send_player_stats(id);
            }
            8 => self.marriage_request(id),
            9 => self.marriage_leave(id),
            10 => self.marriage_remove_ring(id),
            22 => self.promote_fame(id),
            23 => self.script_action(id, &a.string1),
            // Element/refine actions live on their own pages.
            _ => {}
        }
    }

    /// `CurrencyInfo` by name, case-insensitive.
    pub(super) fn currency_by_name(&self, name: &str) -> Option<&crate::data::CurrencyDef> {
        if name.is_empty() {
            return None;
        }
        self.data.currencies.iter().find(|c| {
            c.name.eq_ignore_ascii_case(name) || c.abbreviation.eq_ignore_ascii_case(name)
        })
    }

    fn npc_page_currency(
        &self,
        page: &crate::data::NpcPageDef,
    ) -> Option<&crate::data::CurrencyDef> {
        self.data
            .currencies
            .iter()
            .find(|currency| match page.currency {
                Some(index) => currency.index == index,
                None => currency.currency_type == 0,
            })
    }

    /// Zircon `GetDataTypeValue`: the key a data list/value is stored under.
    /// `NPCDataType`: None 0, User 1, Guild 2 (no guilds yet), Account 3.
    pub(super) fn npc_data_key(&self, p: &PlayerData, data_type: i32) -> Option<String> {
        match data_type {
            0 => Some("None".into()),
            1 => Some(format!("User_{}", p.name)),
            3 => Some(format!("Account_{}", p.account)),
            _ => None,
        }
    }

    /// Zircon `NPCObject.CanBeSeenBy`: `NPCRequirement` rows gate visibility.
    pub(super) fn npc_visible_to(&self, npc_info: i32, viewer: &PlayerData) -> bool {
        let Some(def) = self.data.npcs.iter().find(|n| n.index == npc_info) else {
            return true;
        };
        def.requirements.iter().all(|r| match r.requirement {
            0 => viewer.level >= r.int1,
            1 => viewer.level <= r.int1,
            // Quests: nothing accepted or completed yet.
            2 | 4 => false,
            3 | 5 => true,
            6 => r.class & (1 << viewer.class.mir_class()) != 0,
            7 => {
                let day = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
                    / 86_400
                    + 4)
                    % 7;
                r.days & (1 << day) != 0
            }
            _ => true,
        })
    }

    pub fn npc_buy(&mut self, id: ObjectId, info: i32, count: u32) {
        let Some((_, page)) = self.npc_session(id) else {
            return;
        };
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some(def) = self.data.npc_pages.get(&page) else {
            return;
        };
        let Some(currency) = self.npc_page_currency(def).cloned() else {
            return;
        };
        let Some((_, rate)) = def.goods.iter().find(|(i, _)| *i == info) else {
            return;
        };
        let Some(item) = self.data.items.get(&info) else {
            return;
        };
        if count == 0 || count > item.stack_size.max(1) as u32 {
            return;
        }
        let base = ((item.price as f64 * rate).round() as u64).max(1);
        let price = shop_price(base, currency.exchange_rate);
        let Some(total) = price.checked_mul(count as u64) else {
            return;
        };
        let has_funds = if currency.currency_type == 0 {
            p.bag.gold >= total
        } else {
            total <= i64::MAX as u64
                && p.currencies.get(&currency.index).copied().unwrap_or(0) >= total as i64
        };
        if !has_funds {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: format!("Not enough {}.", currency.name),
                },
            );
            return;
        }
        let mut bought = UserItem::fresh(&self.data, info, count).unwrap();
        bought.locked = true;
        bought.non_refinable = matches!(
            item.item_type,
            mir_proto::item_type::WEAPON
                | mir_proto::item_type::ARMOUR
                | mir_proto::item_type::HELMET
                | mir_proto::item_type::NECKLACE
                | mir_proto::item_type::BRACELET
                | mir_proto::item_type::RING
                | mir_proto::item_type::SHOES
                | mir_proto::item_type::BOOK
        );
        if !p.bag.can_gain_item(&self.data, &bought, p.max_bag) {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: "You cannot carry that.".into(),
                },
            );
            return;
        }
        let name = item.name.clone();
        let o = self.objects.get_mut(&id).unwrap();
        let p = o.player_mut().unwrap();
        if currency.currency_type == 0 {
            p.bag.gold -= total;
        } else {
            *p.currencies.entry(currency.index).or_insert(0) -= total as i64;
        }
        let changes = p.bag.gain_item(&self.data, bought, &mut p.next_item_id);
        if currency.currency_type == 0 {
            self.send_gold(id);
        } else {
            self.send_currencies(id);
        }
        self.send_changes(id, changes);
        self.send_to(
            id,
            ServerMessage::Chat {
                text: format!("Bought {name} x{count} for {total} {}.", currency.name),
            },
        );
    }

    pub fn npc_sell(&mut self, id: ObjectId, items: Vec<(u8, u32)>) {
        let Some((_, page)) = self.npc_session(id) else {
            return;
        };
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let Some(def) = self.data.npc_pages.get(&page).cloned() else {
            return;
        };
        if def.dialog_type != 1 || def.types.is_empty() {
            return;
        }
        let Some(currency) = self.npc_page_currency(&def).cloned() else {
            return;
        };
        let requested = !items.is_empty();
        let mut earned = 0u64;
        let mut sold = 0u32;
        let mut selected = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (slot, count) in items {
            if count == 0 || !seen.insert(slot) {
                continue;
            }
            let Some(item) = p.bag.inventory.get(slot as usize).and_then(Option::as_ref) else {
                continue;
            };
            let Some(idef) = self.data.items.get(&item.info) else {
                continue;
            };
            if count > item.count
                || item.locked
                || item.worthless
                || !idef.can_sell
                || !def.types.contains(&idef.item_type)
            {
                continue;
            }
            let mut part = item.clone();
            part.count = count;
            let price = (sell_price(idef, &part) as f64 * currency.exchange_rate).max(0.0) as u64;
            let Some(next) = earned.checked_add(price) else {
                return;
            };
            earned = next;
            sold += count;
            selected.push((slot, count));
        }
        let can_credit = if currency.currency_type == 0 {
            p.bag.gold.checked_add(earned).is_some()
        } else {
            earned <= i64::MAX as u64
                && p.currencies
                    .get(&currency.index)
                    .copied()
                    .unwrap_or(0)
                    .checked_add(earned as i64)
                    .is_some()
        };
        if !can_credit {
            return;
        }
        let mut changes = Changed::new();
        for (slot, count) in selected {
            let o = self.objects.get_mut(&id).unwrap();
            let p = o.player_mut().unwrap();
            if let Some(c) = p.bag.take(Grid::Inventory, slot, count) {
                changes.push(c);
            }
        }
        let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
        if currency.currency_type == 0 {
            p.bag.gold += earned;
            self.send_gold(id);
        } else {
            *p.currencies.entry(currency.index).or_insert(0) += earned as i64;
            self.send_currencies(id);
        }
        self.send_changes(id, changes);
        if sold > 0 {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: format!("Sold {sold} item(s) for {earned} {}.", currency.name),
                },
            );
        } else if requested {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: "This shop cannot buy that item, or the item is locked or worthless."
                        .into(),
                },
            );
        }
    }

    pub fn npc_repair(&mut self, id: ObjectId, items: Vec<(Grid, u8, u32)>, special: bool) {
        let Some((_, page)) = self.npc_session(id) else {
            return;
        };
        let Some(object) = self.objects.get(&id) else {
            return;
        };
        let Some(player) = object.player() else {
            return;
        };
        let Some(def) = self.data.npc_pages.get(&page) else {
            return;
        };
        if object.dead || def.dialog_type != 2 || items.is_empty() || items.len() > 55 {
            return;
        }
        if items.iter().any(|(g, ..)| *g == Grid::Storage)
            && !self.in_safe_zone(object.map, object.location)
        {
            return;
        }
        let now = super::store::now_secs();
        let mut seen = std::collections::HashSet::new();
        let mut cost = 0u64;
        for &(grid, slot, item_id) in &items {
            let valid = player
                .bag
                .grid(grid)
                .get(slot as usize)
                .and_then(Option::as_ref)
                .filter(|item| item.id == item_id)
                .and_then(|item| self.data.items.get(&item.info).map(|idef| (item, idef)));
            let Some((item, idef)) = valid else {
                self.send_to(
                    id,
                    ServerMessage::Chat {
                        text: "Item changed. Select it again.".into(),
                    },
                );
                return;
            };
            if !seen.insert((grid, slot))
                || !idef.can_repair
                || idef.durability <= 0
                || item.durability >= item.max_durability
                || !def.types.contains(&idef.item_type)
                || !matches!(
                    idef.item_type,
                    mir_proto::item_type::WEAPON
                        | mir_proto::item_type::ARMOUR
                        | mir_proto::item_type::HELMET
                        | mir_proto::item_type::NECKLACE
                        | mir_proto::item_type::BRACELET
                        | mir_proto::item_type::RING
                        | mir_proto::item_type::SHOES
                        | mir_proto::item_type::SHIELD
                )
            {
                self.send_to(
                    id,
                    ServerMessage::Chat {
                        text: "This NPC cannot repair that item.".into(),
                    },
                );
                return;
            }
            if special && now < item.special_repair_until {
                self.send_to(
                    id,
                    ServerMessage::Chat {
                        text: "Special repair is still cooling down.".into(),
                    },
                );
                return;
            }
            cost = cost.saturating_add(crate::items::repair_cost(idef, item, special));
        }
        if player.bag.gold < cost {
            self.send_to(
                id,
                ServerMessage::Chat {
                    text: format!("Repair costs {cost} gold; you do not have enough."),
                },
            );
            return;
        }
        let player = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
        player.bag.gold -= cost;
        let mut changes = Vec::with_capacity(items.len());
        let mut refresh = false;
        for (grid, slot, _) in items.iter().copied() {
            let item = player.bag.grid_mut(grid)[slot as usize].as_mut().unwrap();
            refresh |= grid == Grid::Equipment && item.durability == 0;
            if special {
                item.durability = item.max_durability;
                if self.data.items[&item.info].item_type != mir_proto::item_type::WEAPON {
                    item.special_repair_until = now + 8 * 60 * 60;
                }
            } else {
                item.max_durability =
                    (item.max_durability - (item.max_durability - item.durability) / 15).max(0);
                item.durability = item.max_durability;
            }
            changes.push((grid, slot, Some(item.instance())));
        }
        self.send_gold(id);
        self.send_changes(id, changes);
        if refresh {
            self.refresh_stats(id, false);
        }
        self.send_to(
            id,
            ServerMessage::Chat {
                text: format!(
                    "{} repaired {} item(s) for {cost} gold.",
                    if special { "Special" } else { "Normal" },
                    items.len()
                ),
            },
        );
    }
}

impl World {
    fn script_view(&self, id: ObjectId) -> Option<lua::PlayerView> {
        let p = self.objects.get(&id)?.player()?;
        let mut items: HashMap<String, u32> = HashMap::new();
        for it in p.bag.inventory.iter().flatten() {
            if let Some(d) = self.data.items.get(&it.info) {
                *items.entry(d.name.to_ascii_lowercase()).or_insert(0) += it.count;
            }
        }
        Some(lua::PlayerView {
            name: p.name.clone(),
            level: p.level,
            gold: p.bag.gold,
            items,
        })
    }

    /// Apply what a script asked for; returns the navigation request.
    fn apply_script_commands(
        &mut self,
        id: ObjectId,
        commands: Vec<lua::Command>,
    ) -> Option<String> {
        let mut navigate = None;
        for cmd in commands {
            match cmd {
                lua::Command::GiveGold(n) => {
                    if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                        p.bag.gold = p.bag.gold.saturating_add(n);
                    }
                    self.send_gold(id);
                }
                lua::Command::TakeGold(n) => {
                    if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                        if p.bag.gold >= n {
                            p.bag.gold -= n;
                        }
                    }
                    self.send_gold(id);
                }
                lua::Command::GiveItem(name, count) => {
                    let Some(info) = self
                        .data
                        .items
                        .values()
                        .find(|d| d.name.eq_ignore_ascii_case(&name))
                        .map(|d| d.index)
                    else {
                        tracing::warn!("[NpcScript] give_item: no item named {name:?}");
                        continue;
                    };
                    let changes = {
                        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
                            continue;
                        };
                        if !p.bag.can_gain(&self.data, info, count, p.max_bag) {
                            continue;
                        }
                        let mut next = p.next_item_id;
                        let c = p.bag.gain(&self.data, info, count, &mut next);
                        p.next_item_id = next;
                        c
                    };
                    self.send_changes(id, changes);
                }
                lua::Command::TakeItem(name, count) => {
                    let Some(info) = self
                        .data
                        .items
                        .values()
                        .find(|d| d.name.eq_ignore_ascii_case(&name))
                        .map(|d| d.index)
                    else {
                        continue;
                    };
                    let changes = self
                        .objects
                        .get_mut(&id)
                        .and_then(|o| o.player_mut())
                        .map(|p| p.bag.take_info(info, count))
                        .unwrap_or_default();
                    self.send_changes(id, changes);
                }
                lua::Command::Message(text) => {
                    if !text.is_empty() {
                        self.send_to(id, ServerMessage::Chat { text });
                    }
                }
                lua::Command::Navigate(page) => navigate = Some(page),
            }
        }
        navigate
    }

    /// Run `func(player, npc)` from the current page's script. `None` when
    /// there is no script or function; otherwise the return value and any
    /// navigation the script asked for.
    #[allow(clippy::type_complexity)]
    fn run_script(
        &mut self,
        id: ObjectId,
        func: &str,
    ) -> Option<Result<(mlua::Value, Option<String>), String>> {
        let file = self.npc_script.clone()?;
        if func.trim().is_empty() {
            return None;
        }
        let view = self.script_view(id)?;
        let result = self.scripts.call(&file, func, &view)?;
        Some(match result {
            Ok(r) => {
                let nav = self.apply_script_commands(id, r.commands);
                Ok((r.value, nav))
            }
            Err(e) => {
                tracing::error!("[NpcScript] {e}");
                Err(e)
            }
        })
    }

    /// `NPCCheckType.Script`: fails only when the function returns `false`
    /// or errors (Zircon's code; `nil` passes); a missing script or
    /// function passes too.
    pub(super) fn script_check(&mut self, id: ObjectId, func: &str) -> bool {
        match self.run_script(id, func) {
            None => true,
            Some(Err(_)) => false,
            Some(Ok((value, _))) => !matches!(value, mlua::Value::Boolean(false)),
        }
    }

    /// `NPCActionType.Script`.
    pub(super) fn script_action(&mut self, id: ObjectId, func: &str) {
        let _ = self.run_script(id, func);
    }

    /// `on_open` hook: `Some(Some(page))` redirects to the page with that
    /// description, `Some(None)` cancels, `None` carries on.
    fn script_on_open(&mut self, id: ObjectId) -> Option<Option<i32>> {
        let (_, nav) = self.run_script(id, "on_open")?.ok()?;
        let nav = nav?;
        if nav.trim().is_empty() {
            return Some(None);
        }
        let target = self
            .data
            .npc_pages
            .values()
            .find(|p| p.description.eq_ignore_ascii_case(nav.trim()))
            .map(|p| p.index);
        match target {
            Some(t) => Some(Some(t)),
            None => {
                tracing::warn!("[NpcScript] navigate: no page described {nav:?}");
                None
            }
        }
    }
}

#[cfg(test)]
mod value_tests {
    use super::*;

    #[test]
    fn fame_page_uses_installed_npc_values() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let data = crate::data::GameData::load(assets.join("../Database/System.db")).unwrap();
        let mut world = World::new(data, assets.join("Map"), None);
        let id = world
            .add_player(1, 1, &crate::accounts::test_character("FameTester"))
            .unwrap();
        let page = &world.data.npc_pages[&334];
        assert_eq!(page.values.len(), 2);
        let say = world.npc_page_say(id, page);
        assert!(say.contains("{1000:LawnGreen} points are required"));
        assert!(!say.contains("<1:Unlimited>"));
    }
}

// ---- Lua page scripts (Zircon NpcScriptEngine) ---------------------------
