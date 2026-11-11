use super::*;

impl World {
    /// Enter the world with a saved character. Spawns at the saved map/cell
    /// when it is walkable, otherwise at a start zone for the class.
    pub fn add_player(
        &mut self,
        conn: ConnId,
        account: u32,
        rec: &CharacterRecord,
    ) -> anyhow::Result<ObjectId> {
        let class = rec.class;
        let mut placed = None;
        if !rec.map.is_empty() && self.force_map.is_none() {
            if let Some(m) = self.data.map_by_file(&rec.map).map(|m| m.index) {
                if self.ensure_map(m).is_ok()
                    && self.maps[&m]
                        .file
                        .is_walkable(rec.location.x, rec.location.y)
                    && !self.cell_blocked(m, rec.location, true)
                {
                    placed = Some((m, rec.location));
                }
            }
        }
        let (start_map, start_loc, bind_region) = self.start_location(class)?;
        let (map, location) = placed.unwrap_or((start_map, start_loc));
        // Developer aid: ZIRCON_DEV_LEVEL starts fresh characters at that level.
        let dev_level = std::env::var("ZIRCON_DEV_LEVEL")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .filter(|_| rec.last_login == 0);
        let level = dev_level
            .unwrap_or(rec.level)
            .clamp(1, mir_proto::MAX_PLAYER_LEVEL);
        let base = self
            .data
            .player_base_stat(class.mir_class(), level)
            .ok_or_else(|| anyhow::anyhow!("no BaseStat rows for {class:?}"))?;
        let hp = if rec.hp > 0 {
            rec.hp.min(base.health)
        } else {
            base.health
        };
        let mp = if rec.mp > 0 {
            rec.mp.min(base.mana)
        } else {
            base.mana
        };
        let id = self.alloc_id();
        let bag = Bag::from_stored_with_size(&rec.items, rec.gold, rec.inventory_size as usize);
        let belt = load_belt(&rec.belt, &bag);
        let next_item_id = rec.next_item_id.max(bag.max_id());
        let obj = Object {
            id,
            kind: Kind::Player(PlayerData {
                conn,
                account,
                name: rec.name.clone(),
                class,
                gender: rec.gender,
                hair: rec.hair.max(1),
                hair_colour: rec.hair_colour,
                wardrobe_item: rec.wardrobe_item.filter(|item| {
                    self.data
                        .items
                        .get(item)
                        .is_some_and(|def| def.item_type == item_type::ARMOUR)
                }),
                wardrobe_helmet: rec.wardrobe_helmet.filter(|item| {
                    self.data
                        .items
                        .get(item)
                        .is_some_and(|def| def.item_type == item_type::HELMET)
                }),
                wardrobe_weapon: rec.wardrobe_weapon.filter(|item| {
                    self.data
                        .items
                        .get(item)
                        .is_some_and(|def| def.item_type == item_type::WEAPON)
                }),
                wardrobe_shoes: rec.wardrobe_shoes.filter(|item| {
                    self.data
                        .items
                        .get(item)
                        .is_some_and(|def| def.item_type == item_type::SHOES)
                }),
                character: rec.id,
                gm: rec.gm,
                gm_mode: false,
                superman: false,
                observer: false,
                chat_ban_until: 0,
                global_ban_until: 0,
                level,
                experience: rec.experience,
                experience_fraction: if (0.0..1.0).contains(&rec.experience_fraction) {
                    rec.experience_fraction
                } else {
                    0.0
                },
                max_mp: base.mana,
                mp,
                fp: 0,
                max_fp: 0,
                discipline: rec.discipline,
                discipline_experience: rec.discipline_experience,
                revive_time: 0,
                bind_region,
                regen_time: self.now + 15_000,
                bag,
                event_rewards: rec.event_rewards.clone(),
                next_item_id,
                attack_speed: 0,
                max_bag: base.bag_weight,
                max_wear: base.wear_weight,
                max_hand: base.hand_weight,
                comfort: 0,
                drop_rate: 0,
                companion_collection: false,
                companion_reroll_enabled: 0,
                npc: None,
                magics: rec.magics.iter().map(UserMagic::from_stored).collect(),
                magic_time: 0,
                slaying_charged: false,
                thrusting_on: false,
                half_moon_on: false,
                belt,
                use_item_time: 0,
                buffs: Vec::new(),
                charge: None,
                surge_on: false,
                dash: None,
                life_steal: 0,
                pets: Vec::new(),
                full_moon_ready: false,
                waning_moon_ready: false,
                flame_splash_on: false,
                currencies: rec.currencies.iter().copied().collect(),
                rebirth: rec.rebirth,
                npc_roll: None,
                shout_time: 0,
                global_shout_time: 0,
                allow_group: rec.allow_group,
                // Zircon starts everyone Online; the friend tab changes it.
                online_state: mir_proto::online_state::ONLINE,
                group: None,
                attack_mode: rec.attack_mode,
                pet_mode: rec.pet_mode.min(mir_proto::pet_mode::NONE),
                pk_points: rec.pk_points,
                fame: rec.fame,
                hermit_stats: rec.hermit_stats.iter().copied().collect(),
                pk_tick: if rec.pk_points > 0 { 1 } else { 0 },
                brown_until: 0,
                brown: false,
                guild: None,
                guild_invite: None,
                war_guilds: Vec::new(),
                conquest_map: false,
                mail_time: 0,
                channel: None,
                dragon_blood: false,
                soul_link: None,
                // ZIRCON_DEV_HORSE=<HorseType> hands new characters a horse.
                horse: if rec.last_login == 0 {
                    std::env::var("ZIRCON_DEV_HORSE")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(rec.horse)
                } else {
                    rec.horse
                },
                mounted: false,
                partner: (rec.partner != 0).then(|| (rec.partner, rec.partner_name.clone())),
                wedding_ring: (rec.wedding_ring != 0).then_some(rec.wedding_ring),
                marriage_invite: None,
                marriage_teleport_time: 0,
                fishing: None,
                refines: rec.refines.clone(),
                companions: rec.companions.clone(),
                active_companion: rec.active_companion,
                companion_unlocks: rec.companion_unlocks.clone(),
                companion: None,
                companion_tick: 0,
                storage_size: crate::items::STORAGE_SIZE,
                trade: None,
                trade_request: None,
                group_invite: None,
                quests: rec.quests.clone(),
                drops: Vec::new(),
                fortunes: Vec::new(),
                def_mastery: 0,
                phys_immunity: 0,
                magic_immunity: 0,
                vitality: 0,
                last_stand: 0,
            }),
            map,
            location,
            direction: rec.direction,
            hp,
            max_hp: base.health,
            dead: false,
            stats: CombatStats::ZERO,
            action_time: 0,
            move_time: 0,
            attack_time: 0,
            cell_time: 0,
            in_safe_zone: false,
            light: 0,
            appearance: Appearance::Player {
                name: rec.name.clone(),
                gender: rec.gender,
                class,
                armour: 0,
                weapon: None,
                hair: rec.hair.max(1),
                hair_colour: rec.hair_colour,
                armour_colour: 0,
                helmet: 0,
                shield: None,
                armour_effect: 0,
                emblem_effect: 0,
                weapon_effect: 0,
                shield_effect: 0,
                name_color: if rec.pk_points >= pvp::RED_POINT {
                    3
                } else if rec.pk_points >= 50 {
                    1
                } else {
                    0
                },
                guild: String::new(),
                guild_rank: String::new(),
                horse: 0,
                horse_shape: 0,
            },
            visible: HashSet::new(),
            poisons: Vec::new(),
            heal: None,
        };
        self.insert_object(obj);
        self.apply_pending_world_event_rewards(id);
        // Never had items yet (new character, or one from before items existed).
        if rec.next_item_id == 0 && rec.items.is_empty() {
            self.give_start_items(id);
        }
        // Developer aid: ZIRCON_DEV_SKILLS grants every class skill the level allows.
        if std::env::var_os("ZIRCON_DEV_SKILLS").is_some() && rec.magics.is_empty() {
            let (class, level) = (rec.class.mir_class(), level);
            let mut grant: Vec<u16> = self
                .data
                .magics
                .values()
                .filter(|m| {
                    m.class & (1 << class) != 0
                        && m.school != 0
                        && m.school != 20
                        && m.need_level[0] <= level
                })
                .map(|m| m.magic)
                .collect();
            grant.sort();
            if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                for (i, m) in grant.into_iter().enumerate() {
                    p.magics.push(UserMagic {
                        magic: m,
                        level: 0,
                        experience: 0,
                        key: if i < 11 { i as u8 + 1 } else { 0 },
                        cooldown_until: 0,
                    });
                }
            }
        }
        // Developer aid: ZIRCON_DEV_GAMEGOLD=<n> / ZIRCON_DEV_HUNTGOLD=<n>
        // stock the two store currencies, which nothing else grants yet, so
        // the store's Buy button can be exercised.
        for (var, kind) in [
            (
                "ZIRCON_DEV_GAMEGOLD",
                crate::world::store::CURRENCY_GAME_GOLD,
            ),
            (
                "ZIRCON_DEV_HUNTGOLD",
                crate::world::store::CURRENCY_HUNT_GOLD,
            ),
        ] {
            let Some(amount) = std::env::var(var).ok().and_then(|v| v.parse::<i64>().ok()) else {
                continue;
            };
            let Some(idx) = self
                .data
                .currencies
                .iter()
                .find(|c| c.currency_type == kind)
                .map(|c| c.index)
            else {
                continue;
            };
            if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                p.currencies.insert(idx, amount);
            }
        }
        // Developer aid: ZIRCON_DEV_ITEMS="Bronze Helmet;Healing Potion*5" gives
        // the named items on entry (once per name already in the bag).
        if let Ok(list) = std::env::var("ZIRCON_DEV_ITEMS") {
            for entry in list.split(';').map(str::trim).filter(|s| !s.is_empty()) {
                let (name, count) = match entry.split_once('*') {
                    Some((n, c)) => (n.trim(), c.trim().parse().unwrap_or(1)),
                    None => (entry, 1),
                };
                let Some(info) = self
                    .data
                    .items
                    .values()
                    .find(|d| d.name.eq_ignore_ascii_case(name))
                    .map(|d| d.index)
                else {
                    tracing::warn!("ZIRCON_DEV_ITEMS: no item named {name:?}");
                    continue;
                };
                let has = self.objects[&id]
                    .player()
                    .map(|p| p.bag.count_of(info) > 0)
                    .unwrap_or(true);
                if has {
                    continue;
                }
                if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
                    let mut next = p.next_item_id;
                    p.bag.gain(&self.data, info, count, &mut next);
                    p.next_item_id = next;
                }
                // Wearables go straight on.
                let wearable = !item_type::slots(self.data.items[&info].item_type).is_empty();
                let slot = self.objects[&id].player().and_then(|p| {
                    p.bag
                        .inventory
                        .iter()
                        .position(|s| s.as_ref().map(|i| i.info == info).unwrap_or(false))
                });
                if let (true, Some(slot)) = (wearable, slot) {
                    self.item_use(id, slot as u8);
                }
            }
        }
        self.refresh_stats(id, false);
        self.refresh_appearance(id);
        let o = &self.objects[&id];
        let stats = self.player_stats(o);
        let desc = self.maps[&map].descriptor.clone();
        self.outgoing.push(Outgoing::To(
            conn,
            ServerMessage::Welcome {
                id,
                map: desc,
                location,
                direction: rec.direction,
                stats,
                light: o.light,
            },
        ));
        self.send_inventory(id);
        let p = self.objects[&id].player().unwrap();
        let wardrobe = [
            (slot::ARMOUR, p.wardrobe_item),
            (slot::HELMET, p.wardrobe_helmet),
            (slot::WEAPON, p.wardrobe_weapon),
            (slot::SHOES, p.wardrobe_shoes),
        ];
        for (slot, item) in wardrobe {
            self.send_to(
                id,
                ServerMessage::Wardrobe {
                    slot: slot as u8,
                    item,
                },
            );
        }
        self.send_belt(id);
        self.send_quest_list(id);
        let mode = self.objects[&id].player().unwrap().attack_mode;
        self.send_to(id, ServerMessage::AttackMode { mode });
        let mode = self.objects[&id].player().unwrap().pet_mode;
        self.send_to(id, ServerMessage::PetMode { mode });
        self.refresh_safe_zone(id);
        self.send_safe_zones(id, map);
        self.guild_login(id);
        self.mail_login(id);
        self.send_social(id);
        // Zircon announces the arrival to everyone who friended us.
        self.update_online_state(id, true);
        self.market_login(id);
        // Timed store goods die while the owner is away, so sweep before the
        // store's own stock (and the bag) reach the client.
        self.expire_items(id);
        self.store_login(id);
        self.send_marriage_info(id);
        self.send_refine_list(id);
        self.send_companions(id);
        self.send_currencies(id);
        // ZIRCON_DEV_DISCIPLINE grants a level before the pool is reported.
        self.dev_discipline(id);
        self.send_discipline(id);
        self.companion_spawn(id);
        self.refresh_appearance(id);
        let day_time = self.day_time;
        self.send_to(id, ServerMessage::DayChanged { day_time });
        self.world_event_status(id);
        self.send_magics(id);
        self.dev_timer(id);
        self.dev_harvest(id);
        Ok(id)
    }

    /// Zircon `ClientUserCurrency`: the player's currencies, sent on entry
    /// and again whenever one changes.
    pub fn send_currencies(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let mut list: Vec<mir_proto::CurrencySummary> = self
            .data
            .currencies
            .iter()
            .filter(|c| c.currency_type != 0)
            .map(|c| mir_proto::CurrencySummary {
                name: c.name.clone(),
                abbreviation: c.abbreviation.clone(),
                amount: p.currencies.get(&c.index).copied().unwrap_or(0),
            })
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        self.send_to(id, ServerMessage::Currencies(list));
    }

    pub(super) fn send_belt(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let belt = p.belt.clone();
        self.send_to(id, ServerMessage::BeltLinks(belt));
    }

    /// Zircon `BeltLinkChanged`: link a belt slot to an item type or a
    /// specific inventory item, or clear it.
    pub fn belt_link(&mut self, id: ObjectId, slot: u8, info: Option<i32>, item: Option<u32>) {
        if slot as usize >= MAX_BELT || (info.is_some() && item.is_some()) {
            return;
        }
        let info = info.filter(|i| self.data.items.contains_key(i));
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let item = item.filter(|i| {
            p.bag
                .inventory
                .iter()
                .any(|s| s.as_ref().map(|it| it.id == *i).unwrap_or(false))
        });
        p.belt[slot as usize] = BeltLink { slot, info, item };
    }

    /// Copy the live state of a player back into its character record.
    pub fn snapshot(&self, id: ObjectId, rec: &mut CharacterRecord) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        rec.level = p.level;
        rec.experience = p.experience;
        rec.experience_fraction = p.experience_fraction;
        rec.hp = if o.dead { 0 } else { o.hp };
        rec.mp = p.mp;
        rec.direction = o.direction;
        rec.items = p.bag.to_stored();
        rec.inventory_size = p.bag.inventory.len() as u8;
        rec.gold = p.bag.gold;
        rec.event_rewards = p.event_rewards.clone();
        rec.next_item_id = p.next_item_id;
        rec.magics = p.magics.iter().map(|m| m.stored()).collect();
        rec.belt = p.belt.clone();
        rec.currencies = p.currencies.iter().map(|(k, v)| (*k, *v)).collect();
        rec.rebirth = p.rebirth;
        rec.quests = p.quests.clone();
        rec.allow_group = p.allow_group;
        rec.attack_mode = p.attack_mode;
        rec.pet_mode = p.pet_mode;
        rec.pk_points = p.pk_points;
        rec.fame = p.fame;
        rec.hermit_stats = p.hermit_stats.iter().map(|(k, v)| (*k, *v)).collect();
        rec.horse = p.horse;
        rec.partner = p.partner.as_ref().map(|(c, _)| *c).unwrap_or(0);
        rec.partner_name = p
            .partner
            .as_ref()
            .map(|(_, n)| n.clone())
            .unwrap_or_default();
        rec.wedding_ring = p.wedding_ring.unwrap_or(0);
        rec.refines = p.refines.clone();
        // A rename, restyle or gender swap has to survive the relog.
        rec.name = p.name.clone();
        rec.gender = p.gender;
        rec.hair = p.hair;
        rec.hair_colour = p.hair_colour;
        rec.wardrobe_item = p.wardrobe_item;
        rec.wardrobe_helmet = p.wardrobe_helmet;
        rec.wardrobe_weapon = p.wardrobe_weapon;
        rec.wardrobe_shoes = p.wardrobe_shoes;
        rec.discipline = p.discipline;
        rec.discipline_experience = p.discipline_experience;
        rec.companions = p.companions.clone();
        rec.active_companion = p.active_companion;
        rec.companion_unlocks = p.companion_unlocks.clone();
        if let Some(m) = self.maps.get(&o.map) {
            rec.map = m.descriptor.file.clone();
            rec.location = o.location;
        }
    }

    pub(super) fn player_stats(&self, o: &Object) -> PlayerStats {
        let p = o.player().expect("player");
        PlayerStats {
            level: p.level,
            hp: o.hp,
            max_hp: o.max_hp,
            mp: p.mp,
            max_mp: p.max_mp,
            experience: p.experience,
            max_experience: GameData::max_experience(p.level),
            min_dc: o.stats.min_dc,
            max_dc: o.stats.max_dc,
            min_ac: o.stats.min_ac,
            max_ac: o.stats.max_ac,
            min_mr: o.stats.min_mr,
            max_mr: o.stats.max_mr,
            min_mc: o.stats.min_mc,
            max_mc: o.stats.max_mc,
            min_sc: o.stats.min_sc,
            max_sc: o.stats.max_sc,
            accuracy: o.stats.accuracy,
            agility: o.stats.agility,
            attack_speed: o.player().map(|p| p.attack_speed as i32).unwrap_or(0),
            fame: p.fame,
            fame_title: self
                .fame_def(p.fame)
                .map(|f| f.name.clone())
                .unwrap_or_default(),
            fp: p.fp,
            max_fp: p.max_fp,
            attack_element: o.stats.attack_element,
            discipline: p.discipline,
            discipline_experience: p.discipline_experience,
        }
    }

    pub(super) fn gain_experience(&mut self, id: ObjectId, amount: u64) {
        self.gain_experience_inner(id, amount);
    }

    pub(super) fn gain_combat_experience(&mut self, id: ObjectId, amount: u64) {
        self.gain_experience_inner(id, amount);
    }

    fn gain_experience_inner(&mut self, id: ObjectId, amount: u64) {
        // Zircon GainExperience applies ExperienceRate, then the independent
        // BaseExperienceRate, then halves the result once per rebirth.
        let (rate, base_rate, rebirth) = {
            let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
                return;
            };
            let mut stats = p.bag.equipment_stats(&self.data, p.level, p.class);
            for (&stat, &value) in &p.hermit_stats {
                *stats.entry(stat).or_insert(0) += value;
            }
            if let Some(companion) = p
                .companions
                .iter()
                .find(|c| Some(c.index) == p.active_companion && c.hunger > 0)
            {
                for &(milestone, stat, value) in &companion.bonuses {
                    if companion.level >= milestone {
                        *stats.entry(stat).or_insert(0) += value;
                    }
                }
            }
            for (stat, value) in self.fame_stats(p.fame) {
                *stats.entry(stat).or_insert(0) += value;
            }
            let guild_rate = p
                .guild
                .and_then(|guild| self.guild_store.get(guild))
                .filter(|_| {
                    p.group.is_none_or(|group| {
                        self.groups.get(&group).is_none_or(|members| {
                            members.iter().all(|member| {
                                self.objects
                                    .get(member)
                                    .and_then(|o| o.player())
                                    .is_some_and(|other| other.guild == p.guild)
                            })
                        })
                    })
                })
                .map(|guild| match guild.members.len() {
                    0..=15 => 30,
                    16..=30 => 23,
                    31..=45 => 18,
                    _ => 13,
                })
                .unwrap_or(0);
            let rate = stats.get(&stat::EXPERIENCE_RATE).copied().unwrap_or(0)
                + guild_rate
                + if self.castle_experience_bonus(id) {
                    10
                } else {
                    0
                };
            (
                rate,
                stats.get(&stat::BASE_EXPERIENCE_RATE).copied().unwrap_or(0),
                p.rebirth.max(0),
            )
        };
        let earned = amount as f64
            * (1.0 + rate as f64 / 100.0).max(0.0)
            * (1.0 + base_rate as f64 / 100.0).max(0.0)
            * self.experience_multiplier
            * 0.5_f64.powi(rebirth);
        let Some(o) = self.objects.get_mut(&id) else {
            return;
        };
        let Some(p) = o.player_mut() else {
            return;
        };
        let earned = earned + p.experience_fraction;
        let amount = earned.floor() as u64;
        p.experience_fraction = if earned < u64::MAX as f64 {
            earned.fract()
        } else {
            0.0
        };
        if amount == 0 {
            return;
        }
        p.experience = p.experience.saturating_add(amount);
        let weapon_update = p
            .bag
            .equipment
            .first_mut()
            .and_then(Option::as_mut)
            .and_then(|w| {
                let def = self.data.items.get(&w.info)?;
                if def.item_type != mir_proto::item_type::WEAPON || def.effect == 5 {
                    return None;
                }
                w.gain_weapon_experience(amount).then(|| w.instance())
            });
        let mut leveled = false;
        while p.level < mir_proto::MAX_PLAYER_LEVEL
            && p.experience >= GameData::max_experience(p.level)
            && GameData::max_experience(p.level) > 0
        {
            p.experience -= GameData::max_experience(p.level);
            p.level += 1;
            leveled = true;
        }
        if p.level >= mir_proto::MAX_PLAYER_LEVEL {
            p.experience = 0;
        }
        let name = p.name.clone();
        let level = p.level;
        let class = p.class;
        let _ = class;
        if leveled {
            self.refresh_stats(id, true);
            self.send_quest_markers(id);
            let location = self.objects[&id].location;
            self.events.push((
                id,
                ServerMessage::ObjectEffect {
                    id,
                    effect: mir_proto::effect::LEVEL_UP, // MagicEx 2030..2045.
                    location,
                },
            ));
            self.events.push((
                id,
                ServerMessage::Chat {
                    text: format!("{name} has reached level {level}!"),
                },
            ));
            let (hp, max_hp) = {
                let o = &self.objects[&id];
                (o.hp, o.max_hp)
            };
            self.events
                .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
        }
        if let Some(item) = weapon_update {
            self.send_to(
                id,
                ServerMessage::ItemChanged {
                    grid: mir_proto::Grid::Equipment,
                    slot: 0,
                    item: Some(item),
                },
            );
        }
        let stats = self.player_stats(&self.objects[&id]);
        self.send_to(id, ServerMessage::StatsChanged(stats));
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: mir_proto::ChatKind::System,
                text: format!("Gained {amount} experience."),
            },
        );
    }

    pub(super) fn player_die(&mut self, id: ObjectId) {
        let o = self.objects.get_mut(&id).unwrap();
        o.dead = true;
        o.hp = 0;
        let p = o.player_mut().unwrap();
        p.revive_time = self.now + REVIVE_DELAY;
        let pets = std::mem::take(&mut p.pets);
        self.events.push((id, ServerMessage::ObjectDie { id }));
        self.trade_close(id);
        self.channel_cancel(id);
        self.soul_resonance_death(id);
        self.remove_mount(id);
        self.fishing_cancel(id);
        for pet in pets {
            if self.objects.get(&pet).map(|o| !o.dead).unwrap_or(false) {
                self.monster_die(pet, id);
            }
        }
        self.send_to(
            id,
            ServerMessage::Chat {
                text: "You have died. Use the Revive button to return to town.".into(),
            },
        );
    }

    /// Zircon `C.TownRevive`: return to the bind point immediately.
    pub fn town_revive(&mut self, id: ObjectId) {
        let dead = self.objects.get(&id).map(|o| o.dead).unwrap_or(false);
        if !dead {
            return;
        }
        if let Err(e) = self.revive_player(id) {
            tracing::warn!("revive failed: {e}");
        }
    }

    pub(super) fn revive_player(&mut self, id: ObjectId) -> anyhow::Result<()> {
        let bind_region = self.objects[&id].player().unwrap().bind_region;
        let location = if bind_region != 0 {
            self.go_to_bind_point(id)?
        } else {
            let location = self.objects[&id].location;
            self.move_object(id, location);
            location
        };
        let (hp, dir) = {
            let o = self.objects.get_mut(&id).unwrap();
            o.dead = false;
            o.hp = o.max_hp;
            o.direction = Direction::Down;
            let p = o.player_mut().unwrap();
            p.mp = p.max_mp;
            (o.hp, o.direction)
        };
        self.events.push((
            id,
            ServerMessage::ObjectRevive {
                id,
                location,
                direction: dir,
                hp,
            },
        ));
        let stats = self.player_stats(&self.objects[&id]);
        self.send_to(id, ServerMessage::StatsChanged(stats));
        Ok(())
    }

    // ---- stats, inventory, equipment --------------------------------------

    /// Zircon `RefreshStats`: base stats for class/level plus equipped items.
    pub(super) fn refresh_stats(&mut self, id: ObjectId, restore: bool) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        let Some(base) = self.data.player_base_stat(p.class.mir_class(), p.level) else {
            return;
        };
        let focus = self.focus_points(p.discipline);
        let mut eq = p.bag.equipment_stats(&self.data, p.level, p.class);
        for (&stat, &amount) in &p.hermit_stats {
            *eq.entry(stat).or_insert(0) += amount;
        }
        if let Some(companion) = p
            .companions
            .iter()
            .find(|c| Some(c.index) == p.active_companion && c.hunger > 0)
        {
            for &(milestone, stat, amount) in &companion.bonuses {
                if companion.level >= milestone {
                    *eq.entry(stat).or_insert(0) += amount;
                }
            }
        }
        // A held fame title adds its stats like a permanent buff (Zircon
        // ApplyFameBuff).
        for (stat, amount) in self.fame_stats(p.fame) {
            *eq.entry(stat).or_insert(0) += amount;
        }
        let g = |k: i32| eq.get(&k).copied().unwrap_or(0);
        let horse = p.horse;
        // Passive skills (Zircon `GetPassiveStats`).
        let mut pas_acc = 0;
        let mut pas_agi = 0;
        let mut pas_dc = 0;
        let mut pas_min_dc = 0;
        let mut life_steal = 0;
        let mut pas_mc_pct = 0;
        let mut pas_max_ac = 0;
        let mut pas_max_mr = 0;
        let (mut def_mastery, mut phys_immunity, mut magic_immunity, mut vitality, mut last_stand) =
            (0, 0, 0, 0, 0);
        for m in &p.magics {
            let Some(def) = self.data.magics.get(&m.magic) else {
                continue;
            };
            if p.level < def.need_level[0] {
                continue;
            }
            let (pmin, _) = m.power_range(def);
            match m.magic {
                magic_type::SWORDSMANSHIP | magic_type::SPIRIT_SWORD => pas_acc += pmin,
                magic_type::WILLOW_DANCE => pas_agi += pmin,
                magic_type::SLAYING => {
                    pas_acc += m.level as i32 * 2;
                    pas_dc += m.level as i32 * 2;
                }
                // Zircon rolls GetPower() on every refresh; we use the minimum.
                magic_type::VINE_TREE_DANCE => pas_acc += pmin,
                magic_type::DISCIPLINE => {
                    pas_acc += pmin / 3;
                    pas_min_dc += pmin;
                }
                magic_type::BLOODY_FLOWER => life_steal += pmin,
                // Zircon quirk: owning Renounce grants its MC percent passively.
                magic_type::RENOUNCE => pas_mc_pct += (m.level as i32 + 1) * 10,
                magic_type::ADVENT_OF_DEMON => pas_max_ac += pmin,
                magic_type::ADVENT_OF_DEVIL => pas_max_mr += pmin,
                magic_type::DEFENSIVE_MASTERY => def_mastery += pmin,
                magic_type::PHYSICAL_IMMUNITY => phys_immunity += pmin,
                magic_type::MAGIC_IMMUNITY => magic_immunity += pmin,
                magic_type::VITALITY => vitality += pmin,
                magic_type::LAST_STAND => last_stand += pmin,
                _ => {}
            }
        }
        // Buff stats: flat first, then the summed percents once.
        let mut bs = BuffStats::default();
        for b in &p.buffs {
            bs.dc_pct += b.stats.dc_pct;
            bs.phys_def_pct += b.stats.phys_def_pct;
            bs.mag_def_pct += b.stats.mag_def_pct;
            bs.max_ac += b.stats.max_ac;
            bs.max_mr += b.stats.max_mr;
            bs.agility += b.stats.agility;
            bs.hp_pct += b.stats.hp_pct;
            bs.mc_pct += b.stats.mc_pct;
            bs.max_dc += b.stats.max_dc;
            bs.max_mc += b.stats.max_mc;
            bs.max_sc += b.stats.max_sc;
            bs.raging_wind += b.stats.raging_wind;
            bs.life_steal += b.stats.life_steal;
        }
        bs.mc_pct += pas_mc_pct;
        let level_bonus = (p.level - 1).max(0);
        let class = p.class;
        let o = self.objects.get_mut(&id).unwrap();
        o.max_hp = base.health + g(stat::HEALTH);
        o.max_hp += o.max_hp * g(54) / 100;
        o.max_hp += o.max_hp * bs.hp_pct / 100;
        o.max_hp = o.max_hp.max(10);
        let mut s = CombatStats {
            accuracy: base.accuracy + g(stat::ACCURACY) + pas_acc,
            agility: base.agility + g(stat::AGILITY) + pas_agi + bs.agility,
            min_ac: base.min_ac + g(stat::MIN_AC),
            max_ac: base.max_ac + g(stat::MAX_AC) + bs.max_ac + pas_max_ac,
            min_dc: base.min_dc + g(stat::MIN_DC) + pas_dc + pas_min_dc,
            max_dc: base.max_dc + g(stat::MAX_DC) + pas_dc + bs.max_dc,
            min_mr: base.min_mr + g(stat::MIN_MR),
            max_mr: base.max_mr + g(stat::MAX_MR) + bs.max_mr + pas_max_mr,
            min_mc: base.min_mc + g(stat::MIN_MC),
            max_mc: base.max_mc + g(stat::MAX_MC) + bs.max_mc,
            min_sc: base.min_sc + g(stat::MIN_SC),
            max_sc: base.max_sc + g(stat::MAX_SC) + bs.max_sc,
            luck: g(stat::LUCK),
            critical_chance: 1 + g(55),
            critical_damage: g(115),
            paralysis_chance: g(122),
            slow_chance: g(123),
            silence_chance: g(124),
            block_chance: g(stat::BLOCK_CHANCE),
            evasion_chance: g(stat::EVASION_CHANCE),
            poison_resistance: g(stat::POISON_RESISTANCE),
            pickup_radius: (PICKUP_RADIUS + g(stat::PICK_UP_RADIUS)).clamp(0, 10),
            element_attack: super::ELEMENT_ATTACK_STATS
                .map(|stat| if stat == 0 { 0 } else { g(stat) }),
            element_resist: super::ELEMENT_RESIST_STATS.map(|stat| g(stat).min(5)),
            attack_element: super::affinity_element(g),
        };
        // BaseStat rows can keep a class's attack unchanged for several levels.
        // Grant one extra maximum attack point per level to its primary stat.
        match class {
            Class::Warrior | Class::Assassin => s.max_dc += level_bonus,
            Class::Wizard => s.max_mc += level_bonus,
            Class::Taoist => s.max_sc += level_bonus,
        }
        // An owned horse adds bag weight and defence/attack (Zircon
        // PlayerObject.Stats).
        let (horse_bag, horse_stat) = mounts::horse_bonus(horse);
        s.max_ac += horse_stat;
        s.max_mr += horse_stat;
        s.max_dc += horse_stat;
        s.max_mc += horse_stat;
        s.max_sc += horse_stat;
        let dc_pct = bs.dc_pct + g(stat::DC_PERCENT);
        let mc_pct = bs.mc_pct + g(stat::MC_PERCENT);
        let sc_pct = g(stat::SC_PERCENT);
        s.min_dc += s.min_dc * dc_pct / 100;
        s.max_dc += s.max_dc * dc_pct / 100;
        s.min_mc += s.min_mc * mc_pct / 100;
        s.max_mc += s.max_mc * mc_pct / 100;
        s.min_sc += s.min_sc * sc_pct / 100;
        s.max_sc += s.max_sc * sc_pct / 100;
        s.min_ac += s.min_ac * bs.phys_def_pct / 100;
        s.max_ac += s.max_ac * bs.phys_def_pct / 100;
        s.min_mr += s.min_mr * bs.mag_def_pct / 100;
        s.max_mr += s.max_mr * bs.mag_def_pct / 100;
        s.min_ac = s.min_ac.max(0);
        s.max_ac = s.max_ac.max(0);
        s.min_mr = s.min_mr.max(0);
        s.max_mr = s.max_mr.max(0);
        s.min_dc = s.min_dc.max(0).min(s.max_dc.max(0));
        s.max_dc = s.max_dc.max(0);
        // Raging Wind: pool AC and MR, add 4 + 6 per level, split 30/70.
        if bs.raging_wind > 0 {
            let bonus = 4 + (bs.raging_wind - 1) * 6;
            let pool = s.min_ac + s.max_ac + bonus;
            s.min_ac = pool * 3 / 10;
            s.max_ac = pool - s.min_ac;
            let pool = s.min_mr + s.max_mr + bonus;
            s.min_mr = pool * 3 / 10;
            s.max_mr = pool - s.min_mr;
        }
        o.stats = s;
        if restore {
            o.hp = o.max_hp;
        } else {
            o.hp = o.hp.min(o.max_hp);
        }
        let light = g(stat::LIGHT).clamp(0, 255) as u8;
        let light_changed = o.light != light;
        o.light = light;
        let p = o.player_mut().unwrap();
        p.max_mp = base.mana + g(stat::MANA);
        if restore {
            p.mp = p.max_mp;
        } else {
            p.mp = p.mp.min(p.max_mp);
        }
        // Zircon: Stats[Stat.Focus] is the discipline's FocusPoints, plus
        // anything equipment adds. No item in this pack carries Focus.
        p.max_fp = focus + g(stat::FOCUS);
        if restore {
            p.fp = p.max_fp;
        } else {
            p.fp = p.fp.min(p.max_fp);
        }
        p.attack_speed = g(stat::ATTACK_SPEED) as i64;
        p.life_steal = life_steal + g(35) + bs.life_steal;
        p.def_mastery = def_mastery;
        p.phys_immunity = phys_immunity;
        p.magic_immunity = magic_immunity;
        p.vitality = vitality;
        p.last_stand = last_stand;
        p.max_bag = base.bag_weight + g(73) + horse_bag;
        p.max_wear = base.wear_weight + g(74);
        p.max_hand = base.hand_weight + g(75);
        p.comfort = g(34).clamp(0, 20);
        p.drop_rate = g(37);
        p.companion_collection = g(76) > 0;
        if light_changed {
            self.events
                .push((id, ServerMessage::ObjectLight { id, light }));
        }
    }

    pub(super) fn weights_of(&self, id: ObjectId) -> Option<Weights> {
        let p = self.objects.get(&id)?.player()?;
        Some(p.bag.weights(&self.data, p.max_bag, p.max_wear, p.max_hand))
    }

    pub(super) fn send_inventory(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let inventory = p
            .bag
            .inventory
            .iter()
            .enumerate()
            .filter_map(|(i, it)| it.as_ref().map(|it| (i as u8, it.instance())))
            .collect();
        let equipment = p
            .bag
            .equipment
            .iter()
            .enumerate()
            .filter_map(|(i, it)| it.as_ref().map(|it| (i as u8, it.instance())))
            .collect();
        let gold = p.bag.gold;
        let size = p.bag.inventory.len() as u8;
        let weights = p.bag.weights(&self.data, p.max_bag, p.max_wear, p.max_hand);
        self.send_to(
            id,
            ServerMessage::Inventory {
                size,
                inventory,
                equipment,
                gold,
                weights,
            },
        );
    }

    pub(super) fn send_changes(&mut self, id: ObjectId, changes: Changed) {
        for (grid, slot, item) in changes {
            self.send_to(id, ServerMessage::ItemChanged { grid, slot, item });
        }
        if let Some(w) = self.weights_of(id) {
            self.send_to(id, ServerMessage::WeightsChanged(w));
        }
    }

    pub(super) fn send_gold(&mut self, id: ObjectId) {
        if let Some(gold) = self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .map(|p| p.bag.gold)
        {
            self.send_to(id, ServerMessage::GoldChanged { gold });
        }
    }

    pub(super) fn send_player_stats(&mut self, id: ObjectId) {
        if let Some(o) = self.objects.get(&id) {
            if o.is_player() {
                let stats = self.player_stats(o);
                let (hp, max_hp) = (o.hp, o.max_hp);
                self.send_to(id, ServerMessage::StatsChanged(stats));
                self.events
                    .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
            }
        }
    }

    /// Choose an owned item's appearance. Once selected, its look stays after
    /// selling the item; changing to another look requires ownership again.
    pub fn set_wardrobe(&mut self, id: ObjectId, item: Option<i32>) {
        self.set_wardrobe_slot(id, slot::ARMOUR, item);
    }

    pub fn set_wardrobe_slot(&mut self, id: ObjectId, selected_slot: usize, item: Option<i32>) {
        let kind = match selected_slot {
            slot::ARMOUR => item_type::ARMOUR,
            slot::HELMET => item_type::HELMET,
            slot::WEAPON => item_type::WEAPON,
            slot::SHOES => item_type::SHOES,
            _ => return,
        };
        if let Some(info) = item {
            let owned = self
                .objects
                .get(&id)
                .and_then(|o| o.player())
                .is_some_and(|p| {
                    p.bag
                        .inventory
                        .iter()
                        .chain(&p.bag.equipment)
                        .chain(&p.bag.storage)
                        .flatten()
                        .any(|owned| owned.info == info)
                });
            if !owned
                || !self
                    .data
                    .items
                    .get(&info)
                    .is_some_and(|def| def.item_type == kind)
            {
                self.send_to(
                    id,
                    ServerMessage::Chat {
                        text: "Own that item before choosing its look.".into(),
                    },
                );
                return;
            }
        }
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        match selected_slot {
            slot::ARMOUR => p.wardrobe_item = item,
            slot::HELMET => p.wardrobe_helmet = item,
            slot::WEAPON => p.wardrobe_weapon = item,
            slot::SHOES => p.wardrobe_shoes = item,
            _ => unreachable!(),
        }
        self.refresh_appearance(id);
        self.send_to(
            id,
            ServerMessage::Wardrobe {
                slot: selected_slot as u8,
                item,
            },
        );
    }

    /// Update the player's look from equipment and broadcast on change.
    pub(super) fn refresh_appearance(&mut self, id: ObjectId) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        let wardrobe = p
            .wardrobe_item
            .and_then(|info| self.data.items.get(&info))
            .filter(|def| def.item_type == item_type::ARMOUR);
        let wardrobe_helmet = p
            .wardrobe_helmet
            .and_then(|info| self.data.items.get(&info))
            .filter(|def| def.item_type == item_type::HELMET);
        let wardrobe_weapon = p
            .wardrobe_weapon
            .and_then(|info| self.data.items.get(&info))
            .filter(|def| def.item_type == item_type::WEAPON);
        let equipped_armour = p.bag.equipment[slot::ARMOUR]
            .as_ref()
            .and_then(|item| self.data.items.get(&item.info));
        let equipped_weapon = p.bag.equipment[slot::WEAPON]
            .as_ref()
            .and_then(|item| self.data.items.get(&item.info));
        let armour_effect = wardrobe.or(equipped_armour).map_or(0, |def| {
            mir_proto::equipment_visual::armour_effect(def.index, def.exterior_effect)
        });
        let weapon_effect = wardrobe_weapon.or(equipped_weapon).map_or(0, |def| {
            mir_proto::equipment_visual::weapon_effect(def.index, def.exterior_effect)
        });
        let armour = wardrobe
            .map(|def| def.shape)
            .or_else(|| p.bag.equipped_shape(&self.data, slot::ARMOUR))
            .unwrap_or(0)
            .max(0) as u16;
        let weapon = wardrobe_weapon
            .map(|def| def.shape)
            .or_else(|| p.bag.equipped_shape(&self.data, slot::WEAPON))
            .map(|s| s.max(0) as u16);
        // Zircon: `Helmet = Equipment[Helmet]?.Info.Shape ?? 0` (1-based),
        // `Shield = Equipment[Shield]?.Info.Shape ?? -1`.
        let helmet = wardrobe_helmet
            .map(|def| def.shape)
            .or_else(|| p.bag.equipped_shape(&self.data, slot::HELMET))
            .unwrap_or(0)
            .max(0) as u16;
        let shield = p
            .bag
            .equipped_shape(&self.data, slot::SHIELD)
            .map(|s| s.max(0) as u16);
        let appearance = Appearance::Player {
            name: p.name.clone(),
            gender: p.gender,
            class: p.class,
            armour,
            weapon,
            hair: p.hair,
            hair_colour: p.hair_colour,
            armour_colour: if wardrobe.is_some() {
                0
            } else {
                p.bag.equipped_colour(slot::ARMOUR)
            },
            helmet,
            shield,
            armour_effect,
            emblem_effect: p.bag.equipped_exterior_effect(&self.data, slot::EMBLEM),
            weapon_effect,
            shield_effect: p.bag.equipped_exterior_effect(&self.data, slot::SHIELD),
            name_color: pvp::name_color(p),
            guild: self.guild_tag(p).0,
            guild_rank: self.guild_tag(p).1,
            horse: if p.mounted { p.horse } else { 0 },
            horse_shape: p
                .bag
                .equipped_shape(&self.data, slot::HORSE_ARMOUR)
                .unwrap_or(0)
                .clamp(0, 7) as u8,
        };
        if o.appearance != appearance {
            let o = self.objects.get_mut(&id).unwrap();
            o.appearance = appearance.clone();
            self.events
                .push((id, ServerMessage::ObjectAppearance { id, appearance }));
        }
    }

    /// Zircon `NewCharacter`: every `StartItem` usable by the class/gender.
    pub(super) fn give_start_items(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let (class, gender) = (p.class, p.gender);
        let gflag = match gender {
            Gender::Male => 1,
            Gender::Female => 2,
        };
        let mut starts: Vec<i32> = self
            .data
            .items
            .values()
            .filter(|i| {
                i.start_item
                    && i.required_class & class.flag() != 0
                    && i.required_gender & gflag != 0
            })
            .map(|i| i.index)
            .collect();
        starts.sort();
        let o = self.objects.get_mut(&id).unwrap();
        let p = o.player_mut().unwrap();
        for info in starts {
            let mut next = p.next_item_id;
            p.bag.gain(&self.data, info, 1, &mut next);
            p.next_item_id = next;
        }
        // Auto-equip what fits so the character does not start naked.
        let mut auto = Vec::new();
        for (i, it) in p.bag.inventory.iter().enumerate() {
            if let Some(it) = it {
                if let Some(def) = self.data.items.get(&it.info) {
                    if !item_type::slots(def.item_type).is_empty() {
                        auto.push(i as u8);
                    }
                }
            }
        }
        for slot in auto {
            let _ = self.item_use_inner(id, slot);
        }
    }

    // ---- magic ----------------------------------------------------------------------

    pub(super) fn send_magics(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let list = p.magics.iter().map(|m| m.summary()).collect();
        self.send_to(id, ServerMessage::Magics(list));
    }

    /// Zircon `Resurrection`: back on the same cell at a percent of HP/MP.
    pub(super) fn revive_in_place(&mut self, id: ObjectId, pct: i32) {
        let (hp, dir, location) = {
            let Some(o) = self.objects.get_mut(&id) else {
                return;
            };
            o.dead = false;
            o.hp = (o.max_hp * pct / 100).max(1);
            if let Some(p) = o.player_mut() {
                p.mp = (p.max_mp * pct / 100).max(1);
                p.revive_time = 0;
            }
            (o.hp, o.direction, o.location)
        };
        self.events.push((
            id,
            ServerMessage::ObjectRevive {
                id,
                location,
                direction: dir,
                hp,
            },
        ));
        let stats = self.player_stats(&self.objects[&id]);
        self.send_to(id, ServerMessage::StatsChanged(stats));
    }
}
