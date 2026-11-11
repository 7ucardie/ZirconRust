use super::ai_profile::{AiProfile, Hidden, Spell};
use super::*;
use mir_proto::{attack_mode, pet_mode};

/// Zircon `Globals.ProjectileSpeed`: ms per cell of travel.
const PROJECTILE_MS: u64 = 48;

/// Delayed actions from Zircon-Godot's NumaWarlord (AI 137).
#[derive(Debug)]
pub enum NumaAction {
    Teleport(ObjectId),
    Pull(Vec<ObjectId>),
}

impl World {
    // ---- monster AI --------------------------------------------------------

    /// Cached AI profile for a `MonsterInfo.AI` value.
    pub(super) fn ai_profile(&mut self, ai: i32) -> AiProfile {
        self.ai_profiles
            .entry(ai)
            .or_insert_with(|| super::ai_profile::profile(ai))
            .clone()
    }

    fn monster_ai(&self, id: ObjectId) -> i32 {
        self.data.monsters[&self.objects[&id].monster_ref().def].ai
    }

    pub(super) fn process_monster(&mut self, id: ObjectId) {
        let Some(object) = self.objects.get(&id) else {
            return;
        };
        if object.monster_ref().room_door.is_some() {
            return;
        }
        let now = self.now;
        let (dead, dead_time) = {
            let o = &self.objects[&id];
            let m = match &o.kind {
                Kind::Monster(m) => m,
                _ => return,
            };
            (o.dead, m.dead_time)
        };
        if dead {
            let (revives, revive_at) = {
                let m = self.objects[&id].monster_ref();
                (m.revives_left, m.revive_at)
            };
            if revives > 0 && now >= revive_at {
                self.monster_revive(id);
                return;
            }
            if now > dead_time {
                self.remove_object(id);
            }
            return;
        }
        self.process_numa_action(id);
        let object = &self.objects[&id];
        if object.dead {
            return;
        }
        let (map, loc) = (object.map, object.location);
        // Timed summons vanish; gates only sweep players through.
        if let Some(t) = self.objects[&id].monster_ref().despawn_at {
            if now >= t {
                self.despawn_summon(id);
                return;
            }
        }
        if self.objects[&id].monster_ref().mirror_element.is_some() {
            self.process_mirror_image(id);
            return;
        }
        if let Some(gate) = self.ai_profile(self.monster_ai(id)).gate {
            self.process_gate(id, gate, map, loc);
            return;
        }
        // Paralysis and Silenced stop a monster completely.
        if paralysed(&self.objects[&id]) || has_poison(&self.objects[&id], poison_kind::SILENCED) {
            return;
        }
        // Puppets stand still and blow up after 5 s, or once something
        // targets or strikes them.
        if let Some(explode_at) = self.objects[&id].monster_ref().explode_at {
            let targeted = self.objects[&id].monster_ref().target.is_some();
            if now >= explode_at || targeted {
                self.puppet_explode(id);
            }
            return;
        }
        // Chained followers stay within two cells of their leader.
        if let Some((leader, until)) = self.objects[&id].monster_ref().chained {
            let leader_loc = self
                .objects
                .get(&leader)
                .filter(|l| !l.dead && l.map == self.objects[&id].map)
                .map(|l| l.location);
            match leader_loc {
                Some(ll) if now < until => {
                    if self.objects[&id].location.distance(ll) > 2 && self.monster_can_move(id) {
                        self.move_toward(id, ll, false);
                        return;
                    }
                }
                _ => {
                    if let Some(mm) = self.objects.get_mut(&id).and_then(|o| o.monster_mut()) {
                        mm.chained = None;
                    }
                }
            }
        }
        // Companions only follow their owner and gather drops.
        if self.objects[&id].monster_ref().companion.is_some() {
            if let Some(owner) = self.objects[&id].monster_ref().owner {
                self.process_companion(id, owner);
            } else {
                self.remove_object(id);
            }
            return;
        }
        // Pets: follow the owner, come back when out of sight, untame on expiry.
        let owner = self.objects[&id].monster_ref().owner;
        if let Some(owner_id) = owner {
            let owner_state = self
                .objects
                .get(&owner_id)
                .filter(|o| o.is_player())
                .map(|o| {
                    (
                        o.dead,
                        o.map,
                        o.location,
                        o.direction,
                        o.visible.contains(&id),
                        o.player().unwrap().pet_mode,
                    )
                });
            let Some((odead, omap, oloc, odir, seen, mode)) = owner_state else {
                self.remove_object(id);
                return;
            };
            if odead {
                self.monster_die(id, owner_id);
                return;
            }
            // Shinsu is invisible for its first second. Do not treat that as
            // a lost pet and return before its reveal timer can run.
            let appearing_shinsu = self.objects[&id].monster_ref().hidden
                && self.ai_profile(self.monster_ai(id)).mode_windows
                && omap == map;
            if now >= self.objects[&id].monster_ref().tame_until {
                self.untame(id);
            } else if matches!(mode, pet_mode::BOTH | pet_mode::MOVE | pet_mode::PVP)
                && (!seen || omap != map)
                && !appearing_shinsu
            {
                let behind = oloc.step(odir.rotate(4), 1);
                let to = if omap == map
                    && self.maps[&omap].file.is_walkable(behind.x, behind.y)
                    && !self.cell_blocked(omap, behind, false)
                {
                    behind
                } else {
                    oloc
                };
                if omap != map {
                    self.change_map(id, omap, to);
                } else {
                    self.teleport_object(id, to);
                }
                if let Some(m) = self.objects.get_mut(&id).and_then(|o| o.monster_mut()) {
                    m.target = None;
                }
                return;
            }
        }

        let ai = self.monster_ai(id);
        let prof = self.ai_profile(ai);
        self.process_boss_minions(id, ai);

        // Drop invalid targets (Zircon `Process`): dead, elsewhere, too far,
        // or no longer attackable. Abyss shrinks the view range to 2.
        let view_range = self.monster_view_range(id);
        {
            let target = self.objects[&id].monster_ref().target;
            if let Some(t) = target {
                let me = &self.objects[&id];
                let valid = self
                    .objects
                    .get(&t)
                    .map(|to| {
                        !to.dead
                            && to.map == map
                            && to.location.distance(loc) <= MAX_VIEW_RANGE
                            && (to.location.distance(loc) <= view_range
                                || !has_poison(me, poison_kind::ABYSS))
                            && self.monster_hostile_to(id, to)
                            && self.monster_may_target(me, to)
                    })
                    .unwrap_or(false);
                if !valid {
                    self.objects
                        .get_mut(&id)
                        .unwrap()
                        .monster_mut()
                        .unwrap()
                        .target = None;
                }
            }
        }

        // Hidden monsters (burrowers, ambushers, statues) only watch for prey.
        if let Some(h) = prof.hidden {
            if self.process_hidden(id, &h, &prof) {
                return;
            }
        }
        // Castle guards, gates and flags run their own routine.
        if self.objects[&id].monster_ref().castle.is_some() {
            self.process_castle_object(id);
            return;
        }
        // Shinsu's fighting window opens and closes on its own clock.
        if prof.mode_windows {
            self.process_mode_window(id, now);
        }
        // Doom Claw never moves or searches like the others: every attack
        // delay one of its parts strikes whoever stands in its reach.
        if prof.doom_claw {
            self.process_doom_claw(id, map, loc);
            return;
        }

        // MonsterInfo.Behaviours: periodic self-heal, one-shot escape and
        // enrage at a quarter HP.
        self.process_behaviours(id);
        // Long timers: Hwa's scatter curse, the Queen's map-wide scatter,
        // Guzak's mass purification.
        if prof.scatter_curse || prof.map_scatter || prof.purify_every_ms > 0 {
            self.process_long_cadence(id, &prof, map, loc);
        }
        // Healer ants tend injured allies before anything else.
        if prof.heals_allies && self.heal_ally(id, map, loc) {
            return;
        }

        // Regen: 2 % every 10 s, never while bleeding (the castle lord
        // never heals).
        if !has_poison(&self.objects[&id], poison_kind::HEMORRHAGE) && !self.is_castle_lord(id) {
            let o = self.objects.get_mut(&id).unwrap();
            let m = match &mut o.kind {
                Kind::Monster(m) => m,
                _ => unreachable!(),
            };
            if now >= m.regen_time {
                m.regen_time = now + REGEN_DELAY;
                if o.hp < o.max_hp {
                    o.hp = (o.hp + (o.max_hp as f32 * 0.02).max(1.0) as i32).min(o.max_hp);
                    let (hp, max_hp) = (o.hp, o.max_hp);
                    self.events
                        .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
                }
            }
        }

        // Larva: nothing to chase means nothing to live for.
        if prof.dies_without_target && self.objects[&id].monster_ref().target.is_none() {
            self.monster_die(id, id);
            return;
        }

        // Search (Zircon `ProcessSearch`): every 3 s when idle; with a target
        // Zircon re-searches every tick and switches to the closest, which we
        // sample every 500 ms. Passive monsters only retaliate.
        {
            let (search_due, has_target) = {
                let m = self.objects[&id].monster_ref();
                (now >= m.search_time, m.target.is_some())
            };
            let is_pet = owner.is_some();
            // Taoist summons defend their owner or join an attack; they do not
            // pick fights merely because a monster walks into view.
            if search_due
                && !self.is_reactive_summon(id)
                && (!prof.passive || is_pet)
                && self.monster_can_attack_mode(id)
            {
                let delay = if has_target { 500 } else { SEARCH_DELAY };
                self.objects
                    .get_mut(&id)
                    .unwrap()
                    .monster_mut()
                    .unwrap()
                    .search_time = now + delay;
                if let Some(pick) =
                    self.monster_search(id, view_range.saturating_sub(1).max(1), prof.guard)
                {
                    self.objects
                        .get_mut(&id)
                        .unwrap()
                        .monster_mut()
                        .unwrap()
                        .target = Some(pick);
                }
            }
        }
        let target = self.objects[&id].monster_ref().target;

        // Boss summon phases: at every 1/n of max HP lost, raise minions.
        if let Some(stages) = &prof.stages {
            self.process_stages(id, stages, target);
        }

        // Pets with nothing to fight walk to the cell behind their owner.
        if let Some(owner_id) = owner {
            if target.is_none() && self.monster_can_move(id) {
                let (oloc, odir) = {
                    let o = &self.objects[&owner_id];
                    (o.location, o.direction)
                };
                let goal = oloc.step(odir.rotate(4), 1);
                if goal != loc {
                    self.move_toward(id, goal, false);
                }
            }
            if let Some(t) = target {
                self.process_target(id, t, &prof);
            }
            return;
        }

        // Roam (Zircon `ProcessRoam`): step off a shared cell; otherwise every
        // 2 s a 10 % chance to walk (2/3) or turn (1/3) while someone watches.
        if !prof.immobile && self.monster_can_move(id) {
            let roam_due = now >= self.objects[&id].monster_ref().roam_time;
            if roam_due {
                self.objects
                    .get_mut(&id)
                    .unwrap()
                    .monster_mut()
                    .unwrap()
                    .roam_time = now + ROAM_DELAY;
                let seen = self.players().any(|p| p.visible.contains(&id));
                let shared = self.maps[&map]
                    .objects_at(loc)
                    .iter()
                    .any(|o| *o != id && self.objects[o].blocking());
                if seen && shared {
                    let dir = Direction::from_index(self.rng.random_range(0..8));
                    for i in 0..8 {
                        if self.monster_walk(id, dir.rotate(i)) {
                            break;
                        }
                    }
                } else if seen && target.is_none() && self.rng.random_range(0..10) == 0 {
                    if self.rng.random_range(0..3) > 0 {
                        let dir = self.objects[&id].direction;
                        self.monster_walk(id, dir);
                    } else {
                        let dir = Direction::from_index(self.rng.random_range(0..8));
                        self.monster_turn(id, dir);
                    }
                }
            }
        }

        // Target (Zircon `ProcessTarget`).
        let Some(t) = target else {
            return;
        };
        self.process_target(id, t, &prof);
    }

    /// Zircon `ViewRange`: 2 under Abyss, else the definition's.
    fn monster_view_range(&self, id: ObjectId) -> i32 {
        let o = &self.objects[&id];
        if has_poison(o, poison_kind::ABYSS) {
            2
        } else {
            self.data.monsters[&o.monster_ref().def].view_range
        }
    }

    /// Nearest attackable object within `view_range` (ties broken at random).
    /// Guards look for wild monsters instead of players.
    fn monster_search(&mut self, id: ObjectId, view_range: i32, guard: bool) -> Option<ObjectId> {
        let (map, loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        let mut best: Vec<ObjectId> = Vec::new();
        let mut best_d = i32::MAX;
        let me = &self.objects[&id];
        for pid in &self.maps[&map].objects {
            let p = &self.objects[pid];
            if *pid == id || p.dead {
                continue;
            }
            let ok = if guard && me.monster_ref().owner.is_none() {
                match &p.kind {
                    Kind::Monster(m) => {
                        m.owner.is_none()
                            && !m.guard
                            && !self.data.monsters[&p.monster_ref().def].is_passive()
                            && !self.ai_profiles_passive(p.monster_ref().def)
                    }
                    Kind::Player(pl) => pl.pk_points >= super::pvp::RED_POINT,
                    _ => false,
                }
            } else {
                self.monster_hostile_to(id, p)
                    && self.can_see(me, p)
                    && self.monster_may_target(me, p)
            };
            if !ok {
                continue;
            }
            let d = p.location.distance(loc);
            if d > view_range {
                continue;
            }
            if d < best_d {
                best_d = d;
                best.clear();
            }
            if d == best_d {
                best.push(*pid);
            }
        }
        if best.is_empty() {
            None
        } else {
            Some(best[self.rng.random_range(0..best.len())])
        }
    }

    /// Whether a definition's AI profile is passive (without mutating the cache).
    fn ai_profiles_passive(&self, def: i32) -> bool {
        let ai = self.data.monsters[&def].ai;
        self.ai_profiles
            .get(&ai)
            .map(|p| p.passive)
            .unwrap_or_else(|| super::ai_profile::profile(ai).passive)
    }

    /// Hidden monsters: reveal when a target comes within `find_range`,
    /// hide again (healing) when it leaves `hide_range`. Returns true while
    /// the monster stays hidden this tick.
    fn process_hidden(&mut self, id: ObjectId, h: &Hidden, prof: &AiProfile) -> bool {
        let now = self.now;
        let (hidden, check_due, loc, map) = {
            let o = &self.objects[&id];
            let m = o.monster_ref();
            (m.hidden, now >= m.hide_check, o.location, o.map)
        };
        if !check_due {
            return hidden;
        }
        self.objects
            .get_mut(&id)
            .unwrap()
            .monster_mut()
            .unwrap()
            .hide_check = now + 3000;
        // Anyone attackable close enough?
        let range = if hidden { h.find_range } else { h.hide_range };
        let near = {
            let me = &self.objects[&id];
            self.maps[&map]
                .objects
                .iter()
                .map(|o| &self.objects[o])
                .filter(|p| self.monster_hostile_to(id, p) && self.monster_may_target(me, p))
                .filter(|p| p.location.distance(loc) <= range.max(prof.attack_range))
                .map(|p| p.id)
                .min_by_key(|p| self.objects[p].location.distance(loc))
        };
        if hidden {
            if let Some(t) = near {
                self.reveal_monster(id, Some(t));
                if h.wake_range > 0 {
                    // Zuma guardians and kings share the same wake family.
                    let others: Vec<ObjectId> = self
                        .on_map(map)
                        .filter(|o| {
                            o.id != id
                                && matches!(&o.kind, Kind::Monster(m) if m.hidden && matches!(self.data.monsters[&m.def].ai, 21 | 22))
                                && o.location.distance(loc) <= h.wake_range
                        })
                        .map(|o| o.id)
                        .collect();
                    for o in others {
                        self.reveal_monster(o, Some(t));
                    }
                }
                return false;
            }
            return true;
        }
        if h.hide_range > 0 && near.is_none() {
            // Submerge; burrowers heal fully and shake off poisons.
            let o = self.objects.get_mut(&id).unwrap();
            if h.heal_on_hide {
                o.hp = o.max_hp;
                o.poisons.clear();
            }
            o.action_time = now + 1000;
            if let Kind::Monster(m) = &mut o.kind {
                m.hidden = true;
                m.target = None;
            }
            let (hp, max_hp) = (o.hp, o.max_hp);
            self.events
                .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
            self.send_poison_status(id);
            return true;
        }
        false
    }

    fn reveal_monster(&mut self, id: ObjectId, target: Option<ObjectId>) {
        let delay = if self.monster_ai(id) == 22 {
            2000
        } else {
            1000
        };
        let o = self.objects.get_mut(&id).unwrap();
        o.action_time = self.now + delay;
        o.cell_time = self.now + 500;
        if let Kind::Monster(m) = &mut o.kind {
            m.hidden = false;
            if target.is_some() {
                m.target = target;
            }
        }
    }

    fn teleport_near_cell(&mut self, id: ObjectId, centre: Point, radius: i32) {
        let map = self.objects[&id].map;
        for _ in 0..100 {
            let cell = Point::new(
                centre.x + self.rng.random_range(-radius..=radius),
                centre.y + self.rng.random_range(-radius..=radius),
            );
            if self.maps[&map].file.is_walkable(cell.x, cell.y)
                && !self.cell_blocked(map, cell, false)
            {
                self.teleport_object(id, cell);
                return;
            }
        }
    }

    pub(super) fn spawn_prophet_sorcerer(&mut self, id: ObjectId) {
        self.spawn_minions(id, 1, &[(190, 1)], 1, None, 9);
        let centre = self.objects[&id].location.step(Direction::DownLeft, 5);
        let minion = self.objects[&id].monster_ref().minions.first().copied();
        if let Some(minion) = minion {
            self.teleport_near_cell(minion, centre, 4);
        }
    }

    fn process_boss_minions(&mut self, id: ObjectId, ai: i32) {
        if !matches!(ai, 74 | 77 | 101 | 102 | 115 | 122)
            || self.now < self.objects[&id].monster_ref().minion_time
        {
            return;
        }
        let (map, loc) = (self.objects[&id].map, self.objects[&id].location);
        let interval = match ai {
            74 => 45_000,
            77 => 30_000,
            122 => 20_000,
            101 | 102 => 15_000,
            _ => 10_000,
        };
        self.objects
            .get_mut(&id)
            .unwrap()
            .monster_mut()
            .unwrap()
            .minion_time = self.now + interval;
        if ai == 115 {
            let view = self.data.monsters[&self.objects[&id].monster_ref().def].view_range;
            let far = self
                .on_map(map)
                .find(|o| {
                    o.monster()
                        .is_some_and(|m| self.data.monsters[&m.def].flag == 190)
                        && o.location.distance(loc) > view - 3
                })
                .map(|o| o.id);
            if let Some(minion) = far {
                self.teleport_near_cell(minion, loc.step(Direction::DownLeft, 2), 1);
            }
            return;
        }
        let leash = if matches!(ai, 74 | 77) { 10 } else { 15 };
        let stray: Vec<_> = self.objects[&id]
            .monster_ref()
            .minions
            .iter()
            .copied()
            .filter(|m| {
                self.objects.get(m).is_some_and(|o| {
                    ai != 122
                        && (o.map != map || o.location.distance(loc) > leash)
                        && !(ai == 102
                            && o.monster()
                                .is_some_and(|m| self.data.monsters[&m.def].is_boss))
                })
            })
            .collect();
        for minion in stray {
            self.remove_object(minion);
        }
        let target = self.objects[&id].monster_ref().target;
        if ai == 74 {
            self.spawn_minions(id, 5, &[(180, 2)], 5, target, 10);
        } else if ai == 122 {
            self.spawn_minions(id, 10, &[(204, 2)], 10, target, 10);
        } else if ai == 77 {
            self.spawn_minions(id, 5, &[(142, 1)], 5, target, 10);
        } else if ai == 102 {
            self.spawn_minions(
                id,
                10,
                &[
                    (171, 10000),
                    (172, 10000),
                    (173, 10000),
                    (174, 10000),
                    (175, 10000),
                    (176, 10000),
                    (177, 10000),
                    (178, 10000),
                    (170, 1),
                ],
                10,
                target,
                10,
            );
        } else {
            self.spawn_minions(
                id,
                10,
                &[
                    (171, 2),
                    (172, 2),
                    (173, 2),
                    (174, 2),
                    (175, 2),
                    (176, 2),
                    (177, 2),
                    (178, 2),
                ],
                10,
                target,
                10,
            );
        }
    }

    /// Zircon `Stage` phases: each time HP crosses another 1/n, spawn
    /// `fixed + Random(random + 1)` minions from the weighted list.
    fn process_stages(
        &mut self,
        id: ObjectId,
        stages: &super::ai_profile::Stages,
        target: Option<ObjectId>,
    ) {
        let (hp, max_hp, stage) = {
            let o = &self.objects[&id];
            (o.hp, o.max_hp, o.monster_ref().stage)
        };
        let stage = if stage < 0 { stages.count } else { stage };
        if stage <= 0 || (hp as i64 * stages.count as i64 / max_hp.max(1) as i64) >= stage as i64 {
            if self.objects[&id].monster_ref().stage < 0 {
                self.objects
                    .get_mut(&id)
                    .unwrap()
                    .monster_mut()
                    .unwrap()
                    .stage = stage;
            }
            return;
        }
        self.objects
            .get_mut(&id)
            .unwrap()
            .monster_mut()
            .unwrap()
            .stage = stage - 1;
        let count = stages.fixed + self.rng.random_range(0..stages.random + 1);
        self.spawn_minions(id, count, &stages.list, stages.max_minions, target, 6);
    }

    /// Zircon `SpawnMinions`: up to `count` monsters picked by weight from
    /// `list` (MonsterFlag values), within `radius` of the master.
    pub(super) fn spawn_minions(
        &mut self,
        id: ObjectId,
        count: i32,
        list: &[(i32, i32)],
        max_minions: i32,
        target: Option<ObjectId>,
        radius: i32,
    ) {
        // Forget dead minions.
        let alive: Vec<ObjectId> = self.objects[&id]
            .monster_ref()
            .minions
            .iter()
            .copied()
            .filter(|m| self.objects.get(m).map(|o| !o.dead).unwrap_or(false))
            .collect();
        self.objects
            .get_mut(&id)
            .unwrap()
            .monster_mut()
            .unwrap()
            .minions = alive.clone();
        let room = (max_minions - alive.len() as i32).max(0);
        let count = count.min(room);
        let total: i32 = list.iter().map(|(_, w)| *w).sum();
        if count <= 0 || total <= 0 {
            return;
        }
        let (map, loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        for _ in 0..count {
            let mut roll = self.rng.random_range(0..total);
            let mut flag = list[0].0;
            for (f, w) in list {
                if roll < *w {
                    flag = *f;
                    break;
                }
                roll -= w;
            }
            let Some(def) = self
                .data
                .monsters
                .values()
                .find(|d| d.flag == flag)
                .map(|d| d.index)
            else {
                continue;
            };
            let mut spot = None;
            for _ in 0..25 {
                let p = Point::new(
                    loc.x + self.rng.random_range(-radius..=radius),
                    loc.y + self.rng.random_range(-radius..=radius),
                );
                if self.maps[&map].file.is_walkable(p.x, p.y) && !self.cell_blocked(map, p, false) {
                    spot = Some(p);
                    break;
                }
            }
            let Some(spot) = spot else { continue };
            let minion = self.create_monster(def, map, spot, None, None, 0);
            if let Some(m) = self.objects.get_mut(&minion).and_then(|o| o.monster_mut()) {
                m.master = Some(id);
                m.target = target;
            }
            self.objects
                .get_mut(&id)
                .unwrap()
                .monster_mut()
                .unwrap()
                .minions
                .push(minion);
        }
    }

    /// Zircon `ProcessTarget` with the class-specific attack shapes.
    fn process_target(&mut self, id: ObjectId, t: ObjectId, prof: &AiProfile) {
        let now = self.now;
        let (loc, map) = {
            let o = &self.objects[&id];
            (o.location, o.map)
        };
        if matches!(self.monster_ai(id), 115 | 116)
            && self.monster_can_attack(id)
            && loc.distance(self.objects[&t].location) > 5
            && self.rng.random_range(0..5) == 0
        {
            let centre = if self.monster_ai(id) == 115 {
                loc.step(Direction::DownLeft, 2)
            } else {
                loc
            };
            let radius = if self.monster_ai(id) == 115 { 1 } else { 2 };
            self.teleport_near_cell(t, centre, radius);
        }
        let tloc = self.objects[&t].location;
        let dist = tloc.distance(loc);
        // Small idle Shinsu remains visible but cannot attack.
        if prof.mode_windows && !self.objects[&id].monster_ref().mode {
            return;
        }
        // Terracotta: unseen it rushes in and shows itself at 2 cells;
        // seen and far it may fade out again.
        if let Some(can_phase_out) = prof.phase {
            if self.process_phase(id, t, can_phase_out, dist, now) {
                return;
            }
        }
        // Fear: run straight away and never swing.
        if has_poison(&self.objects[&id], poison_kind::FEAR) {
            if self.monster_can_move(id) && !prof.immobile {
                let away = Direction::from_points(tloc, loc);
                self.move_toward(id, loc.step(away, 2), true);
            }
            return;
        }
        if matches!(self.monster_ai(id), 98 | 100 | 101) {
            if dist > 0
                && dist <= prof.attack_range
                && self.monster_can_attack(id)
                && self.rng.random_range(0..2) == 0
            {
                self.monster_attack_shape(id, t, prof);
            }
            if self.monster_can_move(id) {
                if tloc == loc {
                    let dir = Direction::from_index(self.rng.random_range(0..8));
                    for i in 0..8 {
                        if self.monster_walk(id, dir.rotate(i)) {
                            break;
                        }
                    }
                } else {
                    self.move_toward(id, tloc, false);
                }
            }
            let distance = self.objects[&id].location.distance(tloc);
            if distance > 0 && distance <= prof.attack_range && self.monster_can_attack(id) {
                self.monster_attack_shape(id, t, prof);
            }
            return;
        }
        if self.monster_ai(id) == 137 {
            self.process_numa_target(id, t);
            return;
        }
        // Banyo Captain: every 3 s a 1-in-n chance to purify the target.
        if prof.purify_chance > 0
            && now > self.objects[&id].monster_ref().cadence_time
            && self.monster_can_attack(id)
        {
            self.objects
                .get_mut(&id)
                .unwrap()
                .monster_mut()
                .unwrap()
                .cadence_time = now + 3000;
            if self.rng.random_range(0..prof.purify_chance) == 0 {
                let dir = Direction::from_points(loc, tloc);
                self.face_and_swing(id, dir, None);
                self.events.push((
                    id,
                    ServerMessage::ObjectMagic {
                        id,
                        direction: dir,
                        location: loc,
                        magic: magic_type::PURIFICATION,
                        targets: vec![t],
                        locations: Vec::new(),
                        cast: true,
                    },
                ));
                self.purify(id, t);
                return;
            }
        }
        // Timed curses: weakness on everything within 3, death clouds on
        // every target in view.
        if (prof.weakness_every_ms > 0 || prof.death_cloud_every_ms > 0)
            && now > self.objects[&id].monster_ref().cadence_time
            && self.monster_can_attack(id)
        {
            let every = prof.weakness_every_ms.max(prof.death_cloud_every_ms);
            self.objects
                .get_mut(&id)
                .unwrap()
                .monster_mut()
                .unwrap()
                .cadence_time = now + every;
            if prof.weakness_every_ms > 0 {
                let victims = self.hostiles_within(id, loc, 3);
                if let Some(&target) = victims.first() {
                    self.face_and_swing(
                        id,
                        Direction::from_points(loc, self.objects[&target].location),
                        Some(target),
                    );
                }
                for v in victims {
                    self.apply_poison(
                        v,
                        Poison {
                            kind: poison_kind::MAGIC_WEAKNESS,
                            value: 0,
                            ticks_left: 0,
                            next_tick: now + 10_000,
                            owner: Some(id),
                        },
                    );
                }
            }
            if prof.death_cloud_every_ms > 0 {
                let view = self.data.monsters[&self.objects[&id].monster_ref().def].view_range;
                let (hp, max_hp) = (self.objects[&id].hp, self.objects[&id].max_hp);
                let victims = self.hostiles_within(id, loc, view.max(1));
                if !victims.is_empty() {
                    let dir = self.objects[&id].direction;
                    self.begin_monster_attack(id, dir);
                    self.events.push((
                        id,
                        ServerMessage::ObjectMagic {
                            id,
                            direction: dir,
                            location: loc,
                            magic: 0,
                            targets: vec![t],
                            locations: Vec::new(),
                            cast: true,
                        },
                    ));
                }
                for v in victims {
                    if hp > max_hp / 2 && self.rng.random_range(0..2) > 0 {
                        continue;
                    }
                    let at = self.objects[&v].location;
                    self.death_cloud(id, map, at);
                }
            }
        }
        // Blink packages.
        if let Some((far, cooldown)) = prof.blink_when_far {
            let blink_due = now > self.objects[&id].monster_ref().blink_time;
            if dist > far && blink_due && self.monster_can_move(id) {
                self.objects
                    .get_mut(&id)
                    .unwrap()
                    .monster_mut()
                    .unwrap()
                    .blink_time = now + cooldown;
                self.blink_beside(id, t);
                return;
            }
        }
        if prof.blink_chance > 0
            && dist > 1
            && !prof.immobile
            && self.monster_can_move(id)
            && self.monster_can_attack(id)
        {
            let blink_due = now > self.objects[&id].monster_ref().blink_time;
            if blink_due {
                self.objects
                    .get_mut(&id)
                    .unwrap()
                    .monster_mut()
                    .unwrap()
                    .blink_time = now + 1000;
                if self.rng.random_range(0..prof.blink_chance) == 0 && self.blink_beside(id, t) {
                    self.objects
                        .get_mut(&id)
                        .unwrap()
                        .monster_mut()
                        .unwrap()
                        .bonus = prof.double_on_blink;
                    return;
                }
            }
        }
        let on_ray = {
            let dx = (tloc.x - loc.x).abs();
            let dy = (tloc.y - loc.y).abs();
            dx == 0 || dy == 0 || dx == dy
        };
        let ranged_allowed = prof.attack_range > 1
            && (prof.range_cooldown_ms == 0 || now > self.objects[&id].monster_ref().range_time);
        let reach = if ranged_allowed { prof.attack_range } else { 1 };
        let in_range = self.monster_ai(id) == 19
            || (tloc != loc && dist <= reach && (!prof.rays_only || on_ray || dist <= 1));
        // Timed or chance-based spells.
        if let Some(spell) = &prof.spell {
            let cooled = now >= self.objects[&id].monster_ref().spell_time;
            let chance = if in_range {
                prof.spell_chance_near
            } else {
                prof.spell_chance_far
            };
            let wants = chance > 0
                && cooled
                && self.monster_can_attack(id)
                && dist <= MAGIC_RANGE
                && (chance == 1 || self.rng.random_range(0..chance) == 0);
            if wants {
                if prof.spell_cooldown_ms > 0 {
                    self.objects
                        .get_mut(&id)
                        .unwrap()
                        .monster_mut()
                        .unwrap()
                        .spell_time = now + prof.spell_cooldown_ms;
                }
                self.monster_cast(id, t, spell.clone());
                return;
            }
        }
        // Archers keep their distance.
        if prof.kite && in_range && dist < prof.attack_range - 1 && self.monster_can_move(id) {
            let away = Direction::from_points(tloc, loc);
            self.move_toward(id, loc.step(away, 2), true);
        } else if !in_range {
            if prof.immobile {
                return;
            }
            if self.monster_can_move(id) {
                if tloc == loc && self.monster_can_move(id) {
                    let dir = Direction::from_index(self.rng.random_range(0..8));
                    for i in 0..8 {
                        if self.monster_walk(id, dir.rotate(i)) {
                            break;
                        }
                    }
                } else {
                    self.move_toward(id, tloc, false);
                }
            }
            // Zircon attacks again in the same tick after moving into range.
            let nloc = self.objects[&id].location;
            let ndist = tloc.distance(nloc);
            if !(ndist <= reach && ndist > 0) {
                return;
            }
        }
        if !self.monster_can_attack(id) || now < self.objects[&id].monster_ref().fear_time {
            return;
        }
        let _ = map;
        self.monster_attack_shape(id, t, prof);
    }

    fn numa_teleport_cell(&mut self, id: ObjectId, target: ObjectId) -> Option<Point> {
        let me = &self.objects[&id];
        let victim = self.objects.get(&target)?;
        if victim.dead || me.map != victim.map || !self.monster_hostile_to(id, victim) {
            return None;
        }
        let (map, loc, direction) = (
            me.map,
            victim.location,
            Direction::from_points(me.location, victim.location),
        );
        let start: i8 = self.rng.random_range(0..8);
        let rotation: i8 = if self.rng.random_range(0..2) == 0 {
            1
        } else {
            -1
        };
        (0..8)
            .map(|i| loc.step(direction.rotate((start + i * rotation).rem_euclid(8)), 1))
            .find(|cell| {
                self.maps[&map].file.is_walkable(cell.x, cell.y)
                    && !self.cell_blocked(map, *cell, false)
            })
    }

    fn process_numa_target(&mut self, id: ObjectId, target: ObjectId) {
        let loc = self.objects[&id].location;
        let target_loc = self.objects[&target].location;
        let distance = loc.distance(target_loc);
        let direction = Direction::from_points(loc, target_loc);
        let ready = self.monster_can_attack(id);
        if ready
            && distance > 4
            && self.now >= self.objects[&id].monster_ref().blink_time
            && self.numa_teleport_cell(id, target).is_some()
        {
            self.face_and_swing(id, direction, None);
            let m = self.objects.get_mut(&id).unwrap().monster_mut().unwrap();
            m.blink_time = self.now + 6000;
            m.numa_action = Some((self.now + 500, NumaAction::Teleport(target)));
            return;
        }
        if ready
            && distance > 2
            && distance <= 5
            && self.now >= self.objects[&id].monster_ref().cadence_time
        {
            let targets = self.hostiles_within(id, loc, 5);
            if !targets.is_empty() {
                self.begin_monster_attack(id, direction);
                self.events.push((
                    id,
                    ServerMessage::ObjectMagic {
                        id,
                        direction,
                        location: loc,
                        magic: 0,
                        targets: targets.clone(),
                        locations: Vec::new(),
                        cast: true,
                    },
                ));
                let m = self.objects.get_mut(&id).unwrap().monster_mut().unwrap();
                m.cadence_time = self.now + 10000;
                m.numa_action = Some((self.now + 400, NumaAction::Pull(targets)));
                return;
            }
        }
        if distance == 0 || distance > 2 {
            if self.monster_can_move(id) {
                if distance == 0 {
                    let direction = Direction::from_index(self.rng.random_range(0..8));
                    for i in 0..8 {
                        if self.monster_walk(id, direction.rotate(i)) {
                            break;
                        }
                    }
                } else {
                    self.move_toward(id, target_loc, false);
                }
            }
            return;
        }
        if ready {
            self.monster_range_swing(id, direction);
            let element = self.monster_attack_element(id);
            for victim in self.hostiles_within(id, loc.step(direction, 1), 1) {
                self.push_monster_hit(id, victim, 400, true, element);
            }
        }
    }

    fn process_numa_action(&mut self, id: ObjectId) {
        if self.objects[&id].dead
            || self.objects[&id]
                .monster_ref()
                .numa_action
                .as_ref()
                .is_none_or(|(at, _)| self.now < *at)
        {
            return;
        }
        let (_, action) = self
            .objects
            .get_mut(&id)
            .unwrap()
            .monster_mut()
            .unwrap()
            .numa_action
            .take()
            .unwrap();
        match action {
            NumaAction::Teleport(target) => {
                if let Some(cell) = self.numa_teleport_cell(id, target) {
                    self.objects.get_mut(&id).unwrap().direction =
                        Direction::from_points(cell, self.objects[&target].location);
                    self.teleport_object(id, cell);
                }
            }
            NumaAction::Pull(targets) => {
                let (map, loc, stats) = (
                    self.objects[&id].map,
                    self.objects[&id].location,
                    self.objects[&id].stats,
                );
                let element = self.monster_attack_element(id);
                for victim in targets {
                    let Some(location) = self
                        .objects
                        .get(&victim)
                        .filter(|o| {
                            o.map == map
                                && loc.distance(o.location) <= 5
                                && self.monster_hostile_to(id, o)
                        })
                        .map(|o| o.location)
                    else {
                        continue;
                    };
                    let power = (self.roll_dc(stats) / 2).max(1);
                    self.resolve_hit_batch(vec![PendingHit {
                        time: self.now,
                        attacker: id,
                        target_cell: (map, location),
                        power,
                        target: Some(victim),
                        magics: Vec::new(),
                        primary: true,
                        raw: false,
                        element,
                        ranged: true,
                    }]);
                    let Some(o) = self
                        .objects
                        .get(&victim)
                        .filter(|o| !o.dead && o.map == map)
                    else {
                        continue;
                    };
                    if o.has_buff(buff_type::ENDURANCE)
                        || o.has_buff(buff_type::DRAGON_REPULSE)
                        || o.player().is_some_and(|p| p.channel.is_some())
                        || o.monster()
                            .is_some_and(|m| !self.data.monsters[&m.def].can_push)
                    {
                        continue;
                    }
                    let distance = (o.location.distance(loc) - 1).min(3);
                    if distance > 0 {
                        self.push_back(victim, Direction::from_points(o.location, loc), distance);
                    }
                }
            }
        }
    }

    /// Straight line toward `goal`, then rotate outward through all eight
    /// directions (Zircon `MoveTo`); `away` picks the opposite rotation order.
    pub(super) fn move_toward(&mut self, id: ObjectId, goal: Point, away: bool) {
        let loc = self.objects[&id].location;
        if goal == loc {
            return;
        }
        let dir = Direction::from_points(loc, goal);
        for i in 0..8 {
            let r: i8 = if i % 2 == 0 {
                (i / 2) as i8
            } else {
                -((i + 1) / 2) as i8
            };
            let d = dir.rotate(if away { -r } else { r });
            if self.monster_walk(id, d) {
                return;
            }
        }
    }

    /// Teleport next to the target (first free cell around it). Returns
    /// whether a cell was found.
    fn blink_beside(&mut self, id: ObjectId, t: ObjectId) -> bool {
        let (map, tloc) = {
            let o = &self.objects[&t];
            (o.map, o.location)
        };
        let base = Direction::from_points(tloc, self.objects[&id].location);
        for i in 0..8 {
            let cell = tloc.step(base.rotate(i), 1);
            if self.maps[&map].file.is_walkable(cell.x, cell.y)
                && !self.cell_blocked(map, cell, false)
            {
                self.teleport_object(id, cell);
                let face = Direction::from_points(cell, tloc);
                if let Some(o) = self.objects.get_mut(&id) {
                    o.direction = face;
                    o.action_time = self.now + 300;
                }
                self.events.push((
                    id,
                    ServerMessage::ObjectTurn {
                        id,
                        direction: face,
                    },
                ));
                return true;
            }
        }
        false
    }

    /// Zircon `TeleportNearby(min, max)`: a random walkable cell at that distance.
    pub(super) fn teleport_nearby(&mut self, id: ObjectId, min: i32, max: i32) {
        let (map, loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        for _ in 0..40 {
            let d = self.rng.random_range(min..=max);
            let dir = Direction::from_index(self.rng.random_range(0..8));
            let cell = loc.step(dir, d);
            if self.maps[&map].file.is_walkable(cell.x, cell.y)
                && !self.cell_blocked(map, cell, false)
            {
                self.teleport_object(id, cell);
                return;
            }
        }
    }

    /// Pick the attack shape for this swing: spawner, line, ranged, splash,
    /// self area or plain melee.
    fn monster_attack_shape(&mut self, id: ObjectId, t: ObjectId, prof: &AiProfile) {
        let now = self.now;
        let loc = self.objects[&id].location;
        let tloc = self.objects[&t].location;
        let dist = tloc.distance(loc);
        let dir = Direction::from_points(loc, tloc);
        if let Some((flag, count)) = prof.spawn_on_attack {
            self.face_and_swing(id, dir, None);
            self.spawn_minions(id, count, &[(flag, 1)], 20, Some(t), 1);
            return;
        }
        if prof.suicide_range > 0 {
            self.monster_die(id, id);
            return;
        }
        if prof.guard {
            self.face_and_swing(id, dir, None);
            // Guards end monsters outright (300 ms).
            let power = self.objects[&t].hp.max(1);
            self.pending_hits.push(PendingHit {
                time: now + 300,
                attacker: id,
                target_cell: (self.objects[&id].map, tloc),
                power,
                target: Some(t),
                magics: Vec::new(),
                primary: true,
                raw: true,
                element: element::NONE,
                ranged: true,
            });
            return;
        }
        // These bosses strike everyone around themselves, even with an adjacent target.
        let ai = self.monster_ai(id);
        if matches!(ai, 13 | 19 | 77) {
            let direction = if ai == 19 { Direction::Up } else { dir };
            self.face_and_swing(id, direction, (ai == 19).then_some(t));
            let radius = if ai == 19 { 40 } else { MAGIC_RANGE };
            for victim in self.hostiles_within(id, loc, radius) {
                let delay = 500
                    + if ai == 19 {
                        0
                    } else {
                        loc.distance(self.objects[&victim].location) as u64 * PROJECTILE_MS
                    };
                self.push_monster_hit(id, victim, delay, true, element::NONE);
            }
            return;
        }
        if ai == 102 {
            self.begin_monster_attack(id, dir);
            let affinity = self.monster_attack_element(id);
            for centre in self.hostiles_within(id, loc, 12) {
                if self.rng.random_range(0..10) > 3 {
                    continue;
                }
                self.events.push((
                    id,
                    ServerMessage::ObjectRangeAttack {
                        id,
                        direction: dir,
                        target: Some(centre),
                        location: self.objects[&centre].location,
                        magic: 0,
                    },
                ));
                for victim in self.hostiles_within(id, self.objects[&centre].location, 2) {
                    self.push_monster_hit(id, victim, 1000, true, affinity);
                }
            }
            return;
        }
        if matches!(ai, 98 | 100 | 101) {
            let affinity = self.monster_attack_element(id);
            if self.rng.random_range(0..5) == 0 && dist <= 3 {
                self.monster_range_swing(id, dir);
                let element = match ai {
                    98 => element::LIGHTNING,
                    101 => affinity,
                    _ => element::NONE,
                };
                for victim in self.hostiles_within(id, loc, 3) {
                    self.push_monster_hit(id, victim, 400, true, element);
                }
            } else {
                self.face_and_swing(id, dir, None);
                self.push_monster_hit(id, t, 400, false, affinity);
            }
            return;
        }
        if matches!(ai, 78 | 127) {
            if self.rng.random_range(0..3) == 0 || dist > 2 {
                self.monster_line_attack(id, dir, 3);
            } else {
                self.face_and_swing(id, dir, None);
                if let Some(victim) = self
                    .hostiles_within(id, loc.step(dir, 1), 1)
                    .first()
                    .copied()
                {
                    let affinity = self.monster_attack_element(id);
                    self.push_monster_hit(id, victim, 400, false, affinity);
                }
            }
            return;
        }
        if prof.line_attack > 0 && dist > 1 {
            self.monster_line_attack(id, dir, prof.line_attack);
            return;
        }
        if prof.self_aoe > 0
            && (prof.self_aoe_chance == 0 || self.rng.random_range(0..prof.self_aoe_chance) == 0)
        {
            self.face_and_swing(id, dir, None);
            let victims = self.hostiles_within(id, loc, prof.self_aoe);
            for v in victims {
                self.push_monster_hit(id, v, 400, false, element::NONE);
            }
            return;
        }
        if dist > 1 {
            // Ranged: projectile, optionally splashing around the target.
            self.objects
                .get_mut(&id)
                .unwrap()
                .monster_mut()
                .unwrap()
                .range_time = now + prof.range_cooldown_ms;
            self.face_and_swing(id, dir, Some(t));
            let delay = if prof.kite {
                400 + dist as u64 * PROJECTILE_MS
            } else {
                400
            };
            let splash = prof.splash > 0
                && (prof.splash_chance == 0 || self.rng.random_range(0..prof.splash_chance) == 0);
            if splash {
                let victims = self.hostiles_within(id, tloc, prof.splash);
                for v in victims {
                    self.push_monster_hit(id, v, delay, true, element::NONE);
                }
            } else {
                self.push_monster_hit(id, t, delay, true, element::NONE);
            }
            if prof.fear_rate > 0 && self.rng.random_range(0..prof.fear_rate) == 0 {
                let secs = prof.fear_duration_s + self.rng.random_range(0..4);
                self.objects
                    .get_mut(&id)
                    .unwrap()
                    .monster_mut()
                    .unwrap()
                    .fear_time = now + secs as u64 * 1000;
            }
            return;
        }
        self.monster_attack(id, t);
    }

    fn begin_monster_attack(&mut self, id: ObjectId, dir: Direction) {
        {
            let o = self.objects.get_mut(&id).unwrap();
            o.direction = dir;
            let (md, ad) = {
                let m = o.monster_ref();
                (m.move_delay, m.attack_delay)
            };
            let slow = slow_ms(o);
            o.attack_time = self.now + ad + slow;
            o.action_time = self.now + ad.saturating_sub(100).min(md.max(1)) + slow;
        }
    }

    fn monster_range_swing(&mut self, id: ObjectId, dir: Direction) {
        self.begin_monster_attack(id, dir);
        self.events.push((
            id,
            ServerMessage::ObjectRangeAttack {
                id,
                direction: dir,
                target: None,
                location: self.objects[&id].location,
                magic: 0,
            },
        ));
    }

    fn monster_attack_element(&self, id: ObjectId) -> u8 {
        let def = &self.data.monsters[&self.objects[&id].monster_ref().def];
        let mut best = (element::NONE, 0);
        for element in 1..=7 {
            let value = def.stat(43 + i32::from(element));
            if value > best.1 {
                best = (element, value);
            }
        }
        best.0
    }

    /// Face the target, broadcast the swing and start the attack timers.
    pub(super) fn face_and_swing(
        &mut self,
        id: ObjectId,
        dir: Direction,
        ranged_at: Option<ObjectId>,
    ) {
        self.begin_monster_attack(id, dir);
        match ranged_at {
            Some(t) => self.events.push((
                id,
                ServerMessage::ObjectRangeAttack {
                    id,
                    direction: dir,
                    target: Some(t),
                    location: self.objects[&t].location,
                    magic: 0,
                },
            )),
            None => self.events.push((
                id,
                ServerMessage::ObjectAttack {
                    id,
                    direction: dir,
                    attack_magic: None,
                    element: mir_proto::element::NONE,
                },
            )),
        }
    }

    /// Hostile, living objects within `radius` of `centre` on the monster's map.
    fn hostiles_within(&self, id: ObjectId, centre: Point, radius: i32) -> Vec<ObjectId> {
        let me = &self.objects[&id];
        self.maps[&me.map]
            .objects
            .iter()
            .map(|o| &self.objects[o])
            .filter(|o| o.location.distance(centre) <= radius && self.monster_hostile_to(id, o))
            .map(|o| o.id)
            .collect()
    }

    pub(super) fn push_monster_hit(
        &mut self,
        id: ObjectId,
        target: ObjectId,
        delay: u64,
        ranged: bool,
        elem: u8,
    ) {
        let stats = self.objects[&id].stats;
        let mut power = self.roll_dc(stats);
        if self.objects[&id].monster_ref().bonus {
            power *= 2;
            self.objects
                .get_mut(&id)
                .unwrap()
                .monster_mut()
                .unwrap()
                .bonus = false;
        }
        let map = self.objects[&id].map;
        let tloc = self.objects[&target].location;
        self.pending_hits.push(PendingHit {
            time: self.now + delay,
            attacker: id,
            target_cell: (map, tloc),
            power,
            target: Some(target),
            magics: Vec::new(),
            primary: true,
            raw: false,
            element: elem,
            ranged,
        });
    }

    /// Zircon `LineAttack(n)`: the first object in each of the `n` cells
    /// along the facing direction is struck after 400 ms.
    fn monster_line_attack(&mut self, id: ObjectId, dir: Direction, n: i32) {
        let jinchon = matches!(self.monster_ai(id), 78 | 127);
        let element = if jinchon {
            self.monster_range_swing(id, dir);
            self.monster_attack_element(id)
        } else {
            self.face_and_swing(id, dir, None);
            element::NONE
        };
        let (map, loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        for i in 1..=n {
            let cell = loc.step(dir, i);
            let victim = self.maps[&map]
                .objects_at(cell)
                .iter()
                .copied()
                .find(|v| self.monster_hostile_to(id, &self.objects[v]));
            if let Some(v) = victim {
                self.push_monster_hit(id, v, 400, true, element);
            }
        }
    }

    /// Zircon monster spells: queue the landing(s) and tell clients to play
    /// the cast with the payload on the affected targets/cells.
    pub(super) fn monster_cast(&mut self, id: ObjectId, t: ObjectId, spell: Spell) {
        let spell = match spell {
            Spell::Random(list) if !list.is_empty() => {
                let i = self.rng.random_range(0..list.len());
                list[i].clone()
            }
            s => s,
        };
        let (map, loc) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        let tloc = self.objects[&t].location;
        let dir = Direction::from_points(loc, tloc);
        let now = self.now;
        {
            let o = self.objects.get_mut(&id).unwrap();
            o.direction = dir;
            let (md, ad) = {
                let m = o.monster_ref();
                (m.move_delay, m.attack_delay)
            };
            let slow = slow_ms(o);
            o.attack_time = now + ad + slow;
            o.action_time = now + ad.saturating_sub(100).min(md.max(1)) + slow;
        }
        if matches!(self.monster_ai(id), 41 | 85)
            && matches!(
                spell,
                Spell::Aoe {
                    magic: magic_type::MONSTER_SPLASH,
                    ..
                }
            )
        {
            self.face_and_swing(id, dir, Some(t));
            let affinity = self.monster_attack_element(id);
            for victim in self.hostiles_within(id, tloc, 2) {
                self.push_monster_hit(id, victim, 400, true, affinity);
            }
            return;
        }
        let stats = self.objects[&id].stats;
        let dc = self.roll_dc(stats);
        let mut targets = Vec::new();
        let mut locations = Vec::new();
        let magic;
        match spell {
            Spell::Bolt {
                magic: m,
                element,
                travel,
                scale,
            } => {
                magic = m;
                targets.push(t);
                let delay = 500
                    + if travel {
                        tloc.distance(loc) as u64 * PROJECTILE_MS
                    } else {
                        0
                    };
                self.pending_monster_spells.push(PendingMonsterSpell {
                    time: now + delay,
                    caster: id,
                    targets: vec![t],
                    cells: Vec::new(),
                    power: dc * scale / 100,
                    element,
                    map,
                });
            }
            Spell::Aoe {
                radius,
                magic: m,
                element,
                at_self,
                scale,
            } => {
                magic = m;
                let centre = if at_self { loc } else { tloc };
                locations.push(centre);
                let cells: Vec<(Point, i32)> = (-radius..=radius)
                    .flat_map(|dx| {
                        (-radius..=radius).map(move |dy| Point::new(centre.x + dx, centre.y + dy))
                    })
                    .map(|p| (p, 100))
                    .collect();
                self.pending_monster_spells.push(PendingMonsterSpell {
                    time: now + 500,
                    caster: id,
                    targets: Vec::new(),
                    cells,
                    power: dc * scale / 100,
                    element,
                    map,
                });
            }
            Spell::Line {
                len,
                min,
                max,
                magic: m,
                element,
            } => {
                magic = m;
                for r in min..=max {
                    let d = dir.rotate(r);
                    let mut cells = Vec::new();
                    for i in 1..=len {
                        let p = loc.step(d, i);
                        if !self.maps[&map].file.is_walkable(p.x, p.y) {
                            break;
                        }
                        cells.push((p, 100));
                        // Flanks at half power (Zircon LineAoE).
                        cells.push((p.step(d.rotate(2), 1), 50));
                        cells.push((p.step(d.rotate(-2), 1), 50));
                        if i <= 3 {
                            locations.push(p);
                        }
                        self.pending_monster_spells.push(PendingMonsterSpell {
                            time: now + 500 + i as u64 * 75,
                            caster: id,
                            targets: Vec::new(),
                            cells: std::mem::take(&mut cells),
                            power: dc,
                            element,
                            map,
                        });
                    }
                }
            }
            Spell::Mass {
                magic: m,
                element,
                pct,
            } => {
                magic = m;
                let victims = self.hostiles_within(id, loc, MAX_VIEW_RANGE);
                for v in victims {
                    if pct < 100 && self.rng.random_range(0..100) >= pct {
                        continue;
                    }
                    let d = self.objects[&v].location.distance(loc) as u64;
                    targets.push(v);
                    self.pending_monster_spells.push(PendingMonsterSpell {
                        time: now + 500 + d * PROJECTILE_MS,
                        caster: id,
                        targets: vec![v],
                        cells: Vec::new(),
                        power: dc,
                        element,
                        map,
                    });
                }
            }
            Spell::ThunderStorm => {
                magic = magic_type::MONSTER_THUNDER_STORM;
                locations.push(loc);
                let cells: Vec<(Point, i32)> = (-2..=2)
                    .flat_map(|dx| (-2..=2).map(move |dy| Point::new(loc.x + dx, loc.y + dy)))
                    .map(|p| (p, 100))
                    .collect();
                self.pending_monster_spells.push(PendingMonsterSpell {
                    time: now + 500,
                    caster: id,
                    targets: Vec::new(),
                    cells,
                    power: dc,
                    element: element::LIGHTNING,
                    map,
                });
            }
            Spell::FireWall => {
                // Zircon `FireWall`: a cross of five walls on a random
                // target's cell, 15 ticks every 2 s, fire from the caster's
                // DC on whoever stands in them.
                magic = magic_type::FIRE_WALL;
                let victims = self.hostiles_within(id, loc, 20);
                if victims.is_empty() {
                    return;
                }
                let centre =
                    self.objects[&victims[self.rng.random_range(0..victims.len())]].location;
                locations.push(centre);
                let cells = [
                    centre,
                    centre.step(Direction::Up, 1),
                    centre.step(Direction::Down, 1),
                    centre.step(Direction::Left, 1),
                    centre.step(Direction::Right, 1),
                ];
                for cell in cells {
                    if !self.maps[&map].file.is_walkable(cell.x, cell.y) {
                        continue;
                    }
                    let sid = self.spawn_spell(
                        map,
                        cell,
                        spell_effect::FIRE_WALL,
                        15,
                        2000,
                        id,
                        magic_type::FIRE_WALL,
                    );
                    if let Some(Kind::Spell(s)) = self.objects.get_mut(&sid).map(|o| &mut o.kind) {
                        s.tick_time = now + 500;
                    }
                }
            }
            Spell::PoisonousCloud => {
                // Zircon `PoisonousCloud`: a 5x5 cloud around the caster for
                // 20 s; its +20 agility goes to the caster's allies, which
                // monsters do not model, so the field is visible only.
                magic = magic_type::POISONOUS_CLOUD;
                locations.push(loc);
                for dx in -2..=2 {
                    for dy in -2..=2 {
                        let cell = Point::new(loc.x + dx, loc.y + dy);
                        if !self.maps[&map].file.is_walkable(cell.x, cell.y) {
                            continue;
                        }
                        let sid = self.spawn_spell(
                            map,
                            cell,
                            spell_effect::POISONOUS_CLOUD,
                            1,
                            20_000,
                            id,
                            magic_type::POISONOUS_CLOUD,
                        );
                        if let Some(Kind::Spell(s)) =
                            self.objects.get_mut(&sid).map(|o| &mut o.kind)
                        {
                            s.tick_time = now + 20_000;
                        }
                    }
                }
                let _ = dc;
            }
            Spell::Random(_) => return,
        }
        self.events.push((
            id,
            ServerMessage::ObjectMagic {
                id,
                direction: dir,
                location: loc,
                magic,
                targets,
                locations,
                cast: true,
            },
        ));
    }

    /// Land due monster spells: elemental damage through MR on every hostile
    /// target or cell occupant.
    pub(super) fn process_monster_spells(&mut self) {
        let due: Vec<PendingMonsterSpell> = {
            let (due, later): (Vec<_>, Vec<_>) = self
                .pending_monster_spells
                .drain(..)
                .partition(|s| s.time <= self.now);
            self.pending_monster_spells = later;
            due
        };
        for s in due {
            if self.objects.get(&s.caster).map(|o| o.dead).unwrap_or(true) {
                continue;
            }
            let mut victims: Vec<(ObjectId, i32)> = s.targets.iter().map(|t| (*t, 100)).collect();
            for (cell, scale) in &s.cells {
                if let Some(m) = self.maps.get(&s.map) {
                    for v in m.objects_at(*cell) {
                        victims.push((*v, *scale));
                    }
                }
            }
            for (v, scale) in victims {
                let Some(target) = self.objects.get(&v) else {
                    continue;
                };
                if target.dead || v == s.caster || !self.monster_hostile_to(s.caster, target) {
                    continue;
                }
                let tstats = target.stats;
                let mr = if has_poison(target, poison_kind::MAGIC_WEAKNESS) {
                    0
                } else {
                    self.roll_range(tstats.min_mr, tstats.max_mr)
                };
                let power = s.power * scale / 100 - mr;
                if power > 0 {
                    self.damage(v, s.caster, power, s.element, true);
                }
            }
        }
    }

    /// Zircon `ProcessTarget`: plain melee when adjacent (kept for pets and
    /// the base loop).
    pub(super) fn monster_attack(&mut self, id: ObjectId, target: ObjectId) {
        let tloc = self.objects[&target].location;
        let loc = self.objects[&id].location;
        let dir = Direction::from_points(loc, tloc);
        self.face_and_swing(id, dir, None);
        self.push_monster_hit(id, target, 400, false, element::NONE);
    }

    pub(super) fn monster_can_move(&self, id: ObjectId) -> bool {
        let o = &self.objects[&id];
        let m = o.monster_ref();
        !o.dead
            && (m.companion.is_some()
                || m.owner.is_none_or(|owner| {
                    self.objects.get(&owner).is_some_and(|o| {
                        o.player().is_some_and(|p| {
                            matches!(p.pet_mode, pet_mode::BOTH | pet_mode::MOVE | pet_mode::PVP)
                        })
                    })
                }))
            && m.move_delay > 0
            && self.now >= o.action_time
            && self.now >= o.move_time
            && self.now >= m.shock_until
            && !o.poisons.iter().any(|p| {
                matches!(
                    p.kind,
                    poison_kind::WRAITH_GRIP
                        | poison_kind::SILENCED
                        | poison_kind::CONTAINMENT
                        | poison_kind::BINDING
                )
            })
    }

    /// Skeleton, Jin Skeleton, Shinsu, Infernal Soldier and Undead Soul are
    /// Taoist summons. Tamed wild monsters retain their original search AI.
    pub(super) fn is_reactive_summon(&self, id: ObjectId) -> bool {
        self.objects
            .get(&id)
            .and_then(|o| o.monster())
            .is_some_and(|m| {
                m.owner.is_some() && matches!(self.monster_flag(id), 1 | 2 | 3 | 4 | 9)
            })
    }

    pub(super) fn monster_can_attack(&self, id: ObjectId) -> bool {
        let o = &self.objects[&id];
        let m = o.monster_ref();
        !o.dead
            && self.monster_can_attack_mode(id)
            && m.attack_delay > 0
            && self.now >= o.action_time
            && self.now >= o.attack_time
            && !has_poison(o, poison_kind::FEAR)
            && !has_poison(o, poison_kind::SILENCED)
    }

    fn monster_can_attack_mode(&self, id: ObjectId) -> bool {
        self.objects[&id].monster_ref().owner.is_none_or(|owner| {
            self.objects.get(&owner).is_some_and(|o| {
                o.player().is_some_and(|p| {
                    matches!(
                        p.pet_mode,
                        pet_mode::BOTH | pet_mode::ATTACK | pet_mode::PVP
                    )
                })
            })
        })
    }

    /// Owner settings and both safe zones govern a pet's player and pet targets.
    pub(super) fn monster_hostile_to(&self, id: ObjectId, target: &Object) -> bool {
        let me = &self.objects[&id];
        let Some(owner) = me.monster().and_then(|m| m.owner) else {
            return me.hostile_to(target);
        };
        let Some(player) = self.objects.get(&owner).and_then(|o| o.player()) else {
            return false;
        };
        if matches!(player.pet_mode, pet_mode::MOVE | pet_mode::NONE) || target.dead {
            return false;
        }
        let (other_id, other, target_safe) = if let Some(m) = target.monster() {
            if player.pet_mode == pet_mode::PVP {
                return false;
            }
            let Some(other_id) = m.owner else {
                return me.hostile_to(target);
            };
            if m.guard || m.castle.is_some() || m.companion.is_some() || m.room_door.is_some() {
                return false;
            }
            let Some(other) = self.objects.get(&other_id).and_then(|o| o.player()) else {
                return false;
            };
            (
                other_id,
                other,
                self.in_safe_zone(target.map, target.location),
            )
        } else if let Some(other) = target.player() {
            (target.id, other, target.in_safe_zone)
        } else {
            return false;
        };
        if other_id == owner || self.in_safe_zone(me.map, me.location) || target_safe {
            return false;
        }
        match player.attack_mode {
            attack_mode::PEACE => false,
            attack_mode::GROUP => player.group.is_none() || player.group != other.group,
            attack_mode::GUILD => player.guild.is_none() || player.guild != other.guild,
            attack_mode::WAR_RED_BROWN => {
                other.brown
                    || other.pk_points >= super::pvp::RED_POINT
                    || super::pvp::at_war(player, other)
            }
            _ => true,
        }
    }

    pub(super) fn monster_walk(&mut self, id: ObjectId, dir: Direction) -> bool {
        let (map, from) = {
            let o = &self.objects[&id];
            (o.map, o.location)
        };
        let to = from.step(dir, 1);
        if self.cell_blocked(map, to, false) {
            return false;
        }
        {
            let o = self.objects.get_mut(&id).unwrap();
            o.direction = dir;
            let (md, ad) = {
                let m = o.monster_ref();
                (m.move_delay, m.attack_delay)
            };
            let slow = slow_ms(o);
            o.move_time = self.now + md + slow;
            o.action_time = self.now + md.saturating_sub(100).min(ad) + slow;
        }
        self.move_object(id, to);
        self.events.push((
            id,
            ServerMessage::ObjectMove {
                id,
                from,
                to,
                direction: dir,
                run: false,
            },
        ));
        true
    }

    pub(super) fn monster_turn(&mut self, id: ObjectId, dir: Direction) {
        let o = self.objects.get_mut(&id).unwrap();
        o.direction = dir;
        let (md, ad) = {
            let m = o.monster_ref();
            (m.move_delay, m.attack_delay)
        };
        let slow = slow_ms(o);
        o.move_time = self.now + md + slow;
        o.action_time = self.now + md.saturating_sub(100).min(ad) + slow;
        self.events
            .push((id, ServerMessage::ObjectTurn { id, direction: dir }));
    }

    /// Profile hooks on death: suicide bursts and corpse spawns.
    pub(super) fn monster_death_effects(&mut self, id: ObjectId) {
        let ai = self.monster_ai(id);
        let prof = self.ai_profile(ai);
        let loc = self.objects[&id].location;
        if ai == 101 {
            if let Some(def) = self
                .data
                .monsters
                .values()
                .find(|d| d.flag == 170)
                .map(|d| d.index)
            {
                self.create_monster(def, self.objects[&id].map, loc, None, None, 0);
            }
        }
        if prof.suicide_range > 0 {
            let victims = self.hostiles_within(id, loc, prof.suicide_range);
            for v in victims {
                self.push_monster_hit(id, v, 800, true, element::NONE);
            }
        }
        if let Some((radius, flag, per_player)) = prof.die_splash {
            let victims = self.hostiles_within(id, loc, radius);
            let players: Vec<ObjectId> = victims
                .iter()
                .copied()
                .filter(|v| self.objects[v].is_player())
                .collect();
            for v in victims {
                self.push_monster_hit(id, v, 400, true, element::NONE);
            }
            for p in players {
                self.spawn_minions(id, per_player, &[(flag, 1)], 20, Some(p), 1);
            }
        }
    }

    /// Zircon `UnTame`: back to the wild at a tenth of its health.
    /// Zircon `Shinsu.Process`: after one second the idle small form appears.
    /// A target opens a 10 s fighting window in big form; without a target it
    /// returns to the visible small form. Only the big form may attack.
    fn process_mode_window(&mut self, id: ObjectId, now: u64) {
        let o = self.objects.get_mut(&id).unwrap();
        let m = o.monster_mut().unwrap();
        if m.hidden && now >= m.hide_check {
            m.hidden = false;
        }
        if now < o.action_time {
            return;
        }
        let m = o.monster_mut().unwrap();
        if m.target.is_some() {
            m.mode_time = now + 10_000;
        }
        let mut mode = None;
        if !m.mode && now < m.mode_time {
            m.mode = true;
            mode = Some(true);
            o.action_time = now + 2000;
        } else if m.mode && now > m.mode_time {
            m.mode = false;
            mode = Some(false);
            o.action_time = now + 2000;
        }
        if let Some(big) = mode {
            if let Appearance::Monster { image, .. } = &mut o.appearance {
                *image = if big { 99 } else { 100 };
            }
            let appearance = o.appearance.clone();
            self.events
                .push((id, ServerMessage::ObjectAppearance { id, appearance }));
        }
    }

    /// Zircon `Terracotta.ProcessTarget`: returns true when the tick is
    /// consumed (walking unseen or phasing).
    fn process_phase(
        &mut self,
        id: ObjectId,
        t: ObjectId,
        can_phase_out: bool,
        dist: i32,
        now: u64,
    ) -> bool {
        let hidden = self.objects[&id].monster_ref().hidden;
        if hidden {
            if dist <= 2 {
                self.reveal_monster(id, None);
                return true;
            }
            if self.monster_can_move(id) {
                let goal = self.objects[&t].location;
                self.move_toward(id, goal, false);
                // Unseen it moves every 20 ms.
                let o = self.objects.get_mut(&id).unwrap();
                o.move_time = now + 20;
                o.action_time = now + 20;
            }
            return true;
        }
        let phase_due = now >= self.objects[&id].monster_ref().phase_time;
        if can_phase_out && dist > 4 && phase_due && self.rng.random_range(0..45) == 0 {
            let o = self.objects.get_mut(&id).unwrap();
            o.move_time = now + 1200;
            o.action_time = now + 1200;
            let m = o.monster_mut().unwrap();
            m.hidden = true;
            m.phase_time = now + 5000;
            return true;
        }
        false
    }

    /// Zircon `DoomClaw.ProcessAI`: right claw covers 5 cells right (radius
    /// 5), left claw 5 cells down (radius 5), the maw 5 cells down-right
    /// (view range); 1 in 20 attacks is a wave on everything in view.
    fn process_doom_claw(&mut self, id: ObjectId, map: i32, loc: Point) {
        use mir_proto::magic_type as m;
        if !self.monster_can_attack(id) {
            return;
        }
        let right = self.hostiles_within(id, loc.step(Direction::Right, 5), 5);
        let left = self.hostiles_within(id, loc.step(Direction::Down, 5), 5);
        let middle = self.hostiles_within(id, loc.step(Direction::DownRight, 5), MAX_VIEW_RANGE);
        let dir = self.objects[&id].direction;
        self.face_and_swing(id, dir, None);
        let (magic, victims, push): (u16, Vec<ObjectId>, Option<(Direction, i32)>) =
            if self.rng.random_range(0..20) == 0 {
                let all = self.hostiles_within(id, loc, MAX_VIEW_RANGE);
                if all.is_empty() {
                    return;
                }
                (m::DOOM_CLAW_WAVE, all, Some((Direction::DownRight, 15)))
            } else {
                let total = right.len() + left.len() + middle.len();
                if total == 0 {
                    return;
                }
                let mut value = self.rng.random_range(0..total) as i64;
                value -= right.len() as i64;
                if value < 0 {
                    if self.rng.random_range(0..10) > 0 {
                        (m::DOOM_CLAW_RIGHT_PINCH, middle, None)
                    } else {
                        (
                            m::DOOM_CLAW_RIGHT_SWIPE,
                            middle,
                            Some((Direction::DownLeft, 5)),
                        )
                    }
                } else {
                    value -= left.len() as i64;
                    if value < 0 {
                        if self.rng.random_range(0..10) > 0 {
                            (m::DOOM_CLAW_LEFT_PINCH, middle, None)
                        } else {
                            (
                                m::DOOM_CLAW_LEFT_SWIPE,
                                middle,
                                Some((Direction::UpRight, 5)),
                            )
                        }
                    } else {
                        (m::DOOM_CLAW_SPIT, middle, None)
                    }
                }
            };
        self.events.push((
            id,
            ServerMessage::ObjectMagic {
                id,
                direction: dir,
                location: loc,
                magic,
                targets: victims.clone(),
                locations: Vec::new(),
                cast: true,
            },
        ));
        let stats = self.objects[&id].stats;
        for v in victims {
            let mut power = self.roll_dc(stats);
            // Warriors take 60 %, wizards 70 %, taoists 40 %, assassins 80 %.
            if let Some(class) = self.objects[&v].player().map(|p| p.class.mir_class()) {
                power = power * [60, 70, 40, 80][(class as usize).min(3)] / 100;
            }
            let tloc = self.objects[&v].location;
            self.pending_hits.push(PendingHit {
                time: self.now + 400,
                attacker: id,
                target_cell: (map, tloc),
                power,
                target: Some(v),
                magics: Vec::new(),
                primary: true,
                raw: false,
                element: element::NONE,
                ranged: true,
            });
            if let Some((pdir, cells)) = push {
                self.push_back(v, pdir, cells);
            }
        }
    }

    /// Zircon `ProcessBehaviours` (`MonsterInfo.Behaviours`): Heals (4) 10 %
    /// of max HP every 30 s while hurt, Teleports (8) once 5-15 cells away
    /// at a quarter HP, Enrages (64) once at a quarter HP (attack delay
    /// x0.6 for 10 minutes).
    fn process_behaviours(&mut self, id: ObjectId) {
        let now = self.now;
        let (flags, hp, max_hp) = {
            let o = &self.objects[&id];
            (
                self.data.monsters[&o.monster_ref().def].behaviours,
                o.hp,
                o.max_hp,
            )
        };
        if flags == 0 {
            return;
        }
        if flags & 4 != 0
            && hp < max_hp
            && now >= self.objects[&id].monster_ref().behaviour_heal_time
        {
            let o = self.objects.get_mut(&id).unwrap();
            o.monster_mut().unwrap().behaviour_heal_time = now + 30_000;
            o.hp = (o.hp + (o.max_hp as f32 * 0.10).max(1.0) as i32).min(o.max_hp);
            let (hp, max_hp) = (o.hp, o.max_hp);
            self.events
                .push((id, ServerMessage::HealthChanged { id, hp, max_hp }));
        }
        if flags & 8 != 0
            && hp <= max_hp / 4
            && !self.objects[&id].monster_ref().behaviour_teleported
        {
            self.objects
                .get_mut(&id)
                .unwrap()
                .monster_mut()
                .unwrap()
                .behaviour_teleported = true;
            self.teleport_nearby(id, 5, 15);
        }
        if flags & 64 != 0 && hp <= max_hp / 4 && !self.objects[&id].monster_ref().behaviour_enraged
        {
            let m = self.objects.get_mut(&id).unwrap().monster_mut().unwrap();
            m.behaviour_enraged = true;
            m.attack_delay = (m.attack_delay as f32 * 0.6) as u64;
            m.rage_until = now + 10 * 60_000;
        }
    }

    /// Frost Lord Hwa, Queen Of Dawn and Banyo Lord Guzak's minute timers.
    fn process_long_cadence(&mut self, id: ObjectId, prof: &AiProfile, map: i32, loc: Point) {
        let now = self.now;
        if now < self.objects[&id].monster_ref().cadence_time {
            return;
        }
        let every = if prof.purify_every_ms > 0 {
            prof.purify_every_ms
        } else {
            60_000
        };
        // The first cadence starts a full period after spawn.
        let first = self.objects[&id].monster_ref().cadence_time == 0;
        self.objects
            .get_mut(&id)
            .unwrap()
            .monster_mut()
            .unwrap()
            .cadence_time = now + every;
        if first {
            return;
        }
        if prof.purify_every_ms > 0 {
            let victims = self.hostiles_within(id, loc, MAX_VIEW_RANGE);
            if !victims.is_empty() {
                let dir = self.objects[&id].direction;
                self.events.push((
                    id,
                    ServerMessage::ObjectMagic {
                        id,
                        direction: dir,
                        location: loc,
                        magic: magic_type::PURIFICATION,
                        targets: victims.clone(),
                        locations: Vec::new(),
                        cast: true,
                    },
                ));
                for v in victims {
                    self.purify(id, v);
                }
            }
        }
        if prof.scatter_curse {
            // Zircon `FrostLordHwa`: scatter everything in view within view
            // range, then Abyss, Silenced and Red for 10 s and Wraith Grip
            // for 5 s.
            let victims = self.hostiles_within(id, loc, MAX_VIEW_RANGE);
            for v in victims {
                for _ in 0..20 {
                    let d = self.rng.random_range(1..=MAX_VIEW_RANGE);
                    let dir = Direction::from_index(self.rng.random_range(0..8));
                    let cell = loc.step(dir, d);
                    if self.maps[&map].file.is_walkable(cell.x, cell.y)
                        && !self.cell_blocked(map, cell, false)
                    {
                        self.teleport_with_effects(v, cell);
                        break;
                    }
                }
                for (kind, secs) in [
                    (poison_kind::ABYSS, 10u64),
                    (poison_kind::SILENCED, 10),
                    (poison_kind::RED, 10),
                    (poison_kind::WRAITH_GRIP, 5),
                ] {
                    let (value, ticks) = if kind == poison_kind::RED {
                        (1, 5)
                    } else {
                        (0, 0)
                    };
                    self.apply_poison(
                        v,
                        Poison {
                            kind,
                            value,
                            ticks_left: ticks,
                            next_tick: now + secs * 1000,
                            owner: Some(id),
                        },
                    );
                }
            }
        }
        if prof.map_scatter {
            // Zircon `QueenOfDawn`: every attackable object on the map lands
            // on a random cell.
            let victims: Vec<ObjectId> = self
                .on_map(map)
                .filter(|o| self.monster_hostile_to(id, o))
                .map(|o| o.id)
                .collect();
            let (w, h) = {
                let f = &self.maps[&map].file;
                (f.width as i32, f.height as i32)
            };
            for v in victims {
                for _ in 0..200 {
                    let cell = Point::new(self.rng.random_range(0..w), self.rng.random_range(0..h));
                    if self.maps[&map].file.is_walkable(cell.x, cell.y)
                        && !self.cell_blocked(map, cell, false)
                    {
                        self.teleport_with_effects(v, cell);
                        break;
                    }
                }
            }
        }
    }

    /// Zircon `Purify`: on an enemy strip the helpful buffs (always when
    /// the target is no higher level, else 42 %); on an ally clear poisons
    /// except Parasite.
    pub(super) fn purify(&mut self, caster: ObjectId, target: ObjectId) {
        let Some(t) = self.objects.get(&target) else {
            return;
        };
        if t.dead {
            return;
        }
        let hostile = self.monster_hostile_to(caster, t);
        if hostile {
            let (clevel, tlevel) = (
                self.level_of(&self.objects[&caster]),
                self.level_of(&self.objects[&target]),
            );
            if tlevel > clevel && self.rng.random_range(0..100) >= 42 {
                return;
            }
            const STRIPPED: [u16; 24] = [
                buff_type::HEAL,
                buff_type::INVISIBILITY,
                buff_type::MAGIC_RESISTANCE,
                buff_type::RESILIENCE,
                buff_type::MAGIC_SHIELD,
                buff_type::SUPERIOR_MAGIC_SHIELD,
                buff_type::ELEMENTAL_SUPERIORITY,
                buff_type::BLOOD_LUST,
                buff_type::DEFIANCE,
                buff_type::MIGHT,
                buff_type::REFLECT_DAMAGE,
                buff_type::JUDGEMENT_OF_HEAVEN,
                buff_type::STRENGTH_OF_FAITH,
                buff_type::CELESTIAL_LIGHT,
                buff_type::TRANSPARENCY,
                buff_type::RENOUNCE,
                buff_type::CLOAK,
                buff_type::GHOST_WALK,
                buff_type::LIFE_STEAL,
                buff_type::DARK_CONVERSION,
                buff_type::EVASION,
                buff_type::RAGING_WIND,
                buff_type::INVINCIBILITY,
                buff_type::ENDURANCE,
            ];
            let held: Vec<u16> = self.objects[&target]
                .player()
                .map(|p| p.buffs.iter().map(|b| b.kind).collect())
                .unwrap_or_default();
            for kind in held {
                if STRIPPED.contains(&kind) {
                    self.buff_remove(target, kind);
                }
            }
            if self.objects[&target].heal.is_some() {
                self.objects.get_mut(&target).unwrap().heal = None;
            }
        } else {
            let o = self.objects.get_mut(&target).unwrap();
            o.poisons.retain(|p| p.kind == poison_kind::PARASITE);
            self.send_poison_status(target);
        }
    }

    /// Zircon `HealerAnt`: shoot a ticking heal at the nearest injured ally
    /// in view that is not already being healed. Returns true when a heal
    /// was sent this tick.
    fn heal_ally(&mut self, id: ObjectId, map: i32, loc: Point) -> bool {
        if !self.monster_can_attack(id) {
            return false;
        }
        let view = self.monster_view_range(id).max(1);
        let ally = self
            .on_map(map)
            .filter(|o| {
                o.id != id
                    && !o.dead
                    && o.heal.is_none()
                    && o.hp < o.max_hp
                    && o.location.distance(loc) <= view
                    && matches!(&o.kind, Kind::Monster(m) if m.owner.is_none() && !m.guard)
            })
            .min_by_key(|o| o.location.distance(loc))
            .map(|o| (o.id, o.location, o.max_hp));
        let Some((ally, aloc, amax)) = ally else {
            return false;
        };
        let dir = Direction::from_points(loc, aloc);
        self.face_and_swing(id, dir, Some(ally));
        // Zircon ticks Stat.Healing per second up to HealingCap; the ant's
        // stats carry none in this pack, so heal a tenth of the ally's
        // health over ten seconds.
        let delay = 400 + aloc.distance(loc) as u64 * PROJECTILE_MS;
        if let Some(o) = self.objects.get_mut(&ally) {
            o.heal = Some(HealBuff {
                pool: (amax / 10).max(10),
                cap: (amax / 100).max(1),
                next_tick: self.now + delay,
            });
        }
        true
    }

    /// Voracious ghost: back on its feet with half the HP per death so far.
    fn monster_revive(&mut self, id: ObjectId) {
        let now = self.now;
        let (location, direction, hp) = {
            let o = self.objects.get_mut(&id).unwrap();
            o.dead = false;
            let m = o.monster_mut().unwrap();
            m.revives_left -= 1;
            m.revive_at = 0;
            let deaths = m.death_count.clamp(0, 30);
            let hp = (o.max_hp >> deaths).max(1);
            o.hp = hp;
            o.action_time = now + 1500;
            (o.location, o.direction, hp)
        };
        self.events.push((
            id,
            ServerMessage::ObjectRevive {
                id,
                location,
                direction,
                hp,
            },
        ));
    }

    /// Zircon `DeathCloud`: every cell within 2 of `at` gets a cloud that
    /// bursts once after 4-6 s on whoever stands there.
    fn death_cloud(&mut self, caster: ObjectId, map: i32, at: Point) {
        for dx in -2..=2 {
            for dy in -2..=2 {
                let cell = Point::new(at.x + dx, at.y + dy);
                if !self.maps[&map].file.is_walkable(cell.x, cell.y) {
                    continue;
                }
                let delay = if self.monster_ai(caster) == 127 {
                    2500 + self.rng.random_range(0..5000)
                } else {
                    4500
                };
                let sid = self.spawn_spell(
                    map,
                    cell,
                    spell_effect::DEATH_CLOUD,
                    1,
                    delay,
                    caster,
                    magic_type::MONSTER_DEATH_CLOUD,
                );
                // The cloud waits its delay before its single burst.
                if let Some(Kind::Spell(s)) = self.objects.get_mut(&sid).map(|o| &mut o.kind) {
                    s.tick_time = self.now + delay;
                }
            }
        }
    }

    /// Gates (Netherworld Gate, Jinam Stone Gate): every 3 s, players in
    /// view outside safe zones are sent to the configured region.
    fn process_gate(&mut self, id: ObjectId, gate: u8, map: i32, loc: Point) {
        let now = self.now;
        if now < self.objects[&id].monster_ref().cadence_time {
            return;
        }
        self.objects
            .get_mut(&id)
            .unwrap()
            .monster_mut()
            .unwrap()
            .cadence_time = now + 3000;
        let Some(region) =
            self.gate_regions[gate as usize].and_then(|r| self.data.regions.get(&r).cloned())
        else {
            return;
        };
        if self.ensure_map(region.map).is_err() {
            return;
        }
        let points = region.points(self.maps[&region.map].file.width as i32);
        if points.is_empty() {
            return;
        }
        let view = self.data.monsters[&self.objects[&id].monster_ref().def]
            .view_range
            .max(1);
        let riders: Vec<ObjectId> = self
            .on_map(map)
            .filter(|o| {
                o.player().is_some()
                    && !o.dead
                    && !o.in_safe_zone
                    && o.location.distance(loc) <= view
            })
            .map(|o| o.id)
            .collect();
        for r in riders {
            let (x, y) = points[self.rng.random_range(0..points.len())];
            self.change_map(r, region.map, Point::new(x, y));
        }
    }

    pub(super) fn untame(&mut self, id: ObjectId) {
        let owner = self.objects[&id].monster_ref().owner;
        if let Some(p) = owner
            .and_then(|o| self.objects.get_mut(&o))
            .and_then(|o| o.player_mut())
        {
            p.pets.retain(|x| *x != id);
        }
        if let Some(o) = self.objects.get_mut(&id) {
            if let Kind::Monster(m) = &mut o.kind {
                m.owner = None;
                m.target = None;
                m.summon_level = 0;
                m.tame_until = 0;
                m.search_time = 0;
            }
            if let Appearance::Monster { owner, .. } = &mut o.appearance {
                *owner = None;
            }
        }
        self.refresh_monster_stats(id, false);
        let (max_hp, appearance) = {
            let o = &self.objects[&id];
            (o.max_hp, o.appearance.clone())
        };
        if let Some(o) = self.objects.get_mut(&id) {
            o.hp = o.hp.min((max_hp / 10).max(1));
        }
        self.events
            .push((id, ServerMessage::ObjectAppearance { id, appearance }));
    }

    /// Make `id` the pet of `owner` (Zircon tame/summon bookkeeping).
    pub(super) fn tame(&mut self, id: ObjectId, owner: ObjectId, summon_level: i32, tame_ms: u64) {
        let owner_name = self.objects[&owner].player().map(|p| p.name.clone());
        let old_spawn = self.objects[&id].monster_ref().spawn;
        if let Some((map, gi)) = old_spawn {
            if let Some(g) = self.maps.get_mut(&map).and_then(|m| m.spawns.get_mut(gi)) {
                g.alive -= 1;
            }
        }
        if let Some(o) = self.objects.get_mut(&id) {
            if let Kind::Monster(m) = &mut o.kind {
                m.owner = Some(owner);
                m.spawn = None;
                m.target = None;
                m.summon_level = summon_level;
                m.tame_until = self.now.saturating_add(tame_ms);
                m.shock_until = 0;
            }
            if let Appearance::Monster { owner: on, .. } = &mut o.appearance {
                *on = owner_name;
            }
        }
        if let Some(p) = self.objects.get_mut(&owner).and_then(|o| o.player_mut()) {
            if !p.pets.contains(&id) {
                p.pets.push(id);
            }
        }
        self.refresh_monster_stats(id, false);
        let appearance = self.objects[&id].appearance.clone();
        self.events
            .push((id, ServerMessage::ObjectAppearance { id, appearance }));
    }

    /// Re-scale every pet of a player (after Strength Of Faith changes).
    pub(super) fn refresh_pets(&mut self, owner: ObjectId) {
        let pets: Vec<ObjectId> = self
            .objects
            .get(&owner)
            .and_then(|o| o.player())
            .map(|p| p.pets.clone())
            .unwrap_or_default();
        for pet in pets {
            self.refresh_monster_stats(pet, false);
        }
    }

    /// Zircon `Puppet.Die`: the owner's Summon Puppet hits everything within
    /// two cells 800 ms later.
    pub(super) fn puppet_explode(&mut self, id: ObjectId) {
        let (owner, map, loc) = {
            let o = &self.objects[&id];
            (o.monster_ref().owner, o.map, o.location)
        };
        self.events.push((
            id,
            ServerMessage::ObjectEffect {
                id,
                effect: effect::PUPPET,
                location: loc,
            },
        ));
        if let Some(owner) = owner {
            let victims: Vec<ObjectId> = self
                .on_map(map)
                .filter(|o| !o.dead && o.location.distance(loc) <= 2)
                .filter(|o| matches!(&o.kind, Kind::Monster(m) if m.owner.is_none()))
                .map(|o| o.id)
                .collect();
            for v in victims {
                self.pending_magics.push(PendingMagic {
                    time: self.now + 800,
                    caster: owner,
                    magic: magic_type::SUMMON_PUPPET,
                    target: Some(v),
                    location: loc,
                    direction: None,
                    primary: true,
                    chain: None,
                });
            }
        }
        self.monster_die(id, owner.unwrap_or(id));
    }
}

/// Whether the object carries a poison of `kind`.
pub(super) fn has_poison(o: &Object, kind: u16) -> bool {
    o.poisons.iter().any(|p| p.kind == kind)
}

#[cfg(test)]
mod report_tests {
    use super::*;

    fn setup() -> Option<(World, ObjectId, i32, Point)> {
        let assets = std::path::PathBuf::from(std::env::var_os("ZIRCON_ASSETS")?);
        let data = crate::data::GameData::load(assets.join("../Database/System.db")).unwrap();
        let mut world = World::new(data, assets.join("Map"), None).with_seed(0x5A1C0);
        let player = world
            .add_player(1, 1, &crate::accounts::test_character("BossTester"))
            .unwrap();
        let map = world.objects[&player].map;
        world.test_clear_monsters(map);
        let loc = world
            .test_quiet_cell(map, world.objects[&player].location)
            .unwrap();
        world.teleport(player, loc);
        Some((world, player, map, loc))
    }

    #[test]
    fn monster_cast_keeps_caster_location_and_separate_targets() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let caster_loc = loc.step(Direction::Up, 3);
        for ai in [16, 36, 38] {
            let caster = world.test_spawn_ai(ai, map, caster_loc).unwrap();
            let spell = world.ai_profile(ai).spell.unwrap();
            world.monster_cast(caster, player, spell);
            assert!(world.events.iter().any(|(_, m)| matches!(m,
                ServerMessage::ObjectMagic { id, location, targets, .. }
                if *id == caster && *location == caster_loc && targets.contains(&player))));
            assert_eq!(world.objects[&caster].location, caster_loc);
            world.remove_object(caster);
        }
    }

    #[test]
    fn redmoon_hits_everyone_around_caster_even_with_adjacent_target() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let second = world
            .add_player(2, 2, &crate::accounts::test_character("SecondTarget"))
            .unwrap();
        let caster_loc = loc.step(Direction::Up, 1);
        world.teleport(second, caster_loc.step(Direction::Up, 40));
        let caster = world.test_spawn_ai(19, map, caster_loc).unwrap();
        let profile = world.ai_profile(19);
        world.monster_attack_shape(caster, player, &profile);
        for target in [player, second] {
            assert!(world.pending_hits.iter().any(|h| h.attacker == caster
                && h.target == Some(target)
                && h.time == world.now + 500));
        }
    }

    #[test]
    fn banyo_cannot_blink_during_attack_cooldown() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let caster = world
            .test_spawn_ai(71, map, loc.step(Direction::Up, 3))
            .unwrap();
        world.objects.get_mut(&caster).unwrap().attack_time = 100_000;
        let profile = world.ai_profile(71);
        world.now = 2_000;
        world.process_target(caster, player, &profile);
        assert_eq!(world.objects[&caster].monster_ref().blink_time, 0);
    }

    #[test]
    fn bosses_replenish_minions_and_dragon_queen_spawns_lord_on_death() {
        let Some((mut world, _, map, loc)) = setup() else {
            return;
        };
        for (ai, count, interval) in [
            (74, 5, 45_000),
            (77, 5, 30_000),
            (101, 10, 15_000),
            (102, 10, 15_000),
            (122, 10, 20_000),
        ] {
            let boss = world
                .test_spawn_ai(ai, map, loc.step(Direction::Up, 5))
                .unwrap();
            world.process_boss_minions(boss, ai);
            assert_eq!(world.objects[&boss].monster_ref().minions.len(), count);
            let first = world.objects[&boss].monster_ref().minions[0];
            world.remove_object(first);
            world.now += interval;
            world.process_boss_minions(boss, ai);
            assert_eq!(world.objects[&boss].monster_ref().minions.len(), count);
            if ai == 101 {
                world.monster_death_effects(boss);
                assert!(world.on_map(map).any(|o| o.monster().is_some_and(|m| world
                    .data
                    .monsters[&m.def]
                    .flag
                    == 170)
                    && o.location == world.objects[&boss].location));
            }
            let minions = world.objects[&boss].monster_ref().minions.clone();
            for minion in minions {
                world.remove_object(minion);
            }
            world.remove_object(boss);
        }
    }

    #[test]
    fn necromancer_weakness_consumes_attack_and_removes_spell_resistance() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let necromancer = world
            .test_spawn_ai(59, map, loc.step(Direction::Up, 2))
            .unwrap();
        world.now = 1;
        let profile = world.ai_profile(59);
        world.process_target(necromancer, player, &profile);
        assert!(world.objects[&necromancer].attack_time > world.now);
        assert!(has_poison(
            &world.objects[&player],
            poison_kind::MAGIC_WEAKNESS
        ));
        let hp = world.objects[&player].hp;
        let player_stats = &mut world.objects.get_mut(&player).unwrap().stats;
        player_stats.min_mr = 10_000;
        player_stats.max_mr = 10_000;
        world.pending_monster_spells.push(PendingMonsterSpell {
            time: world.now,
            caster: necromancer,
            targets: vec![player],
            cells: Vec::new(),
            power: 10,
            element: element::LIGHTNING,
            map,
        });
        world.process_monster_spells();
        assert!(world.objects[&player].hp < hp);
    }

    #[test]
    fn jinchon_cloud_cast_consumes_attack_and_melee_has_alternate_shapes() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss = world
            .test_spawn_ai(78, map, loc.step(Direction::Up, 1))
            .unwrap();
        let profile = world.ai_profile(78);
        world.now = 1;
        world.events.clear();
        world.process_target(boss, player, &profile);
        assert!(world.events.iter().any(
            |(_, m)| matches!(m, ServerMessage::ObjectMagic { id, magic: 0, .. } if *id == boss)
        ));
        assert!(world.objects[&boss].attack_time > world.now);
        assert!(!world.pending_hits.iter().any(|h| h.attacker == boss));
        let mut saw_melee = false;
        let mut saw_line = false;
        for _ in 0..40 {
            world.events.clear();
            world.pending_hits.clear();
            world.monster_attack_shape(boss, player, &profile);
            if world
                .events
                .iter()
                .any(|(_, m)| matches!(m, ServerMessage::ObjectAttack { id, .. } if *id == boss))
            {
                saw_melee = true;
                assert_eq!(
                    world
                        .pending_hits
                        .iter()
                        .filter(|h| h.attacker == boss)
                        .count(),
                    1
                );
            }
            saw_line |= world.events.iter().any(|(_, m)| matches!(m, ServerMessage::ObjectRangeAttack { id, target: None, .. } if *id == boss));
        }
        assert!(saw_melee && saw_line);
    }

    #[test]
    fn dragon_queen_chases_in_range_and_only_splashes_near_targets() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss_loc = loc.step(Direction::Up, 5);
        let boss = world.test_spawn_ai(101, map, boss_loc).unwrap();
        let profile = world.ai_profile(101);
        world.objects.get_mut(&boss).unwrap().attack_time = 100_000;
        world.now = 1;
        world.process_target(boss, player, &profile);
        assert_ne!(world.objects[&boss].location, boss_loc);
        world.teleport_object(boss, boss_loc);
        for _ in 0..40 {
            world.events.clear();
            world.pending_hits.clear();
            world.monster_attack_shape(boss, player, &profile);
            assert!(world
                .events
                .iter()
                .any(|(_, m)| matches!(m, ServerMessage::ObjectAttack { id, .. } if *id == boss)));
            assert_eq!(world.pending_hits.len(), 1);
            assert_eq!(
                world.pending_hits[0].element,
                world.monster_attack_element(boss)
            );
        }
        world.teleport_object(boss, loc.step(Direction::Up, 2));
        let mut saw_splash = false;
        for _ in 0..40 {
            world.events.clear();
            world.pending_hits.clear();
            world.monster_attack_shape(boss, player, &profile);
            saw_splash |= world.events.iter().any(|(_, m)| matches!(m, ServerMessage::ObjectRangeAttack { id, target: None, .. } if *id == boss));
            assert!(world
                .pending_hits
                .iter()
                .all(|h| h.element == world.monster_attack_element(boss)));
        }
        assert!(saw_splash);
    }

    fn spawn_test_numa(world: &mut World, map: i32, loc: Point) -> ObjectId {
        let mut def = world
            .data
            .monsters
            .values()
            .filter(|d| d.ai == 0)
            .min_by_key(|d| d.index)
            .unwrap()
            .clone();
        def.index = 100_137;
        def.ai = 137;
        def.image = 196;
        def.attack_delay = 1000;
        def.move_delay = 700;
        world.data.monsters.insert(def.index, def);
        let boss = world.test_spawn_ai(137, map, loc).unwrap();
        world.objects.get_mut(&boss).unwrap().stats.min_dc = 100;
        world.objects.get_mut(&boss).unwrap().stats.max_dc = 100;
        boss
    }

    #[test]
    fn numa_teleport_waits_half_second_and_tracks_target() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss_loc = loc.step(Direction::Up, 6);
        let boss = spawn_test_numa(&mut world, map, boss_loc);
        world.process_numa_target(boss, player);
        assert_eq!(world.objects[&boss].location, boss_loc);
        assert_eq!(world.objects[&boss].monster_ref().blink_time, 6000);
        world.now = 499;
        world.process_numa_action(boss);
        assert_eq!(world.objects[&boss].location, boss_loc);
        let moved_target = loc.step(Direction::Right, 2);
        world.teleport(player, moved_target);
        world.now = 500;
        world.process_monster(boss);
        assert_eq!(world.objects[&boss].location.distance(moved_target), 1);
        assert!(world.objects[&boss].monster_ref().numa_action.is_none());
    }

    #[test]
    fn numa_pull_waits_then_deals_half_dc_and_pulls_three_cells() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let direction = Direction::ALL
            .into_iter()
            .find(|d| {
                (1..=4).all(|n| {
                    let p = loc.step(*d, n);
                    world.maps[&map].file.is_walkable(p.x, p.y)
                        && !world.cell_blocked(map, p, false)
                })
            })
            .unwrap();
        let boss = spawn_test_numa(&mut world, map, loc.step(direction, 4));
        let victim = world.objects.get_mut(&player).unwrap();
        victim.hp = 1000;
        victim.max_hp = 1000;
        victim.stats = CombatStats::ZERO;
        world.process_numa_target(boss, player);
        assert_eq!(world.objects[&boss].monster_ref().cadence_time, 10000);
        world.now = 399;
        world.process_numa_action(boss);
        assert_eq!(world.objects[&player].hp, 1000);
        assert_eq!(world.objects[&player].location, loc);
        world.now = 400;
        world.process_numa_action(boss);
        assert_eq!(world.objects[&player].hp, 950);
        assert_eq!(world.objects[&player].location, loc.step(direction, 3));
    }

    #[test]
    fn numa_pull_respects_endurance_without_skipping_damage() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss = spawn_test_numa(&mut world, map, loc.step(Direction::Up, 4));
        let victim = world.objects.get_mut(&player).unwrap();
        victim.hp = 1000;
        victim.max_hp = 1000;
        victim.stats = CombatStats::ZERO;
        world.buff_add(player, buff_type::ENDURANCE, 10000, BuffStats::default());
        world.process_numa_target(boss, player);
        world.now = 400;
        world.process_numa_action(boss);
        assert!(world.objects[&player].hp < 1000);
        assert_eq!(world.objects[&player].location, loc);
    }

    #[test]
    fn numa_sweep_hits_front_area_and_not_behind() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let second = world
            .add_player(2, 2, &crate::accounts::test_character("NumaFront"))
            .unwrap();
        let behind = world
            .add_player(3, 3, &crate::accounts::test_character("NumaBehind"))
            .unwrap();
        let boss = spawn_test_numa(&mut world, map, loc.step(Direction::Up, 1));
        world.teleport(second, loc.step(Direction::Right, 1));
        world.teleport(behind, loc.step(Direction::Up, 3));
        world.process_numa_target(boss, player);
        for target in [player, second] {
            assert!(world.pending_hits.iter().any(|h| h.attacker == boss
                && h.target == Some(target)
                && h.time == 400
                && h.power == 100));
        }
        assert!(!world
            .pending_hits
            .iter()
            .any(|h| h.attacker == boss && h.target == Some(behind)));
    }

    #[test]
    fn numa_delayed_actions_reject_missing_or_out_of_range_targets() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss = spawn_test_numa(&mut world, map, loc.step(Direction::Up, 4));
        world.process_numa_target(boss, player);
        world.teleport(player, loc.step(Direction::Down, 3));
        let (hp, at) = (world.objects[&player].hp, world.objects[&player].location);
        world.now = 400;
        world.process_numa_action(boss);
        assert_eq!(world.objects[&player].hp, hp);
        assert_eq!(world.objects[&player].location, at);
        world.now = 2000;
        world.process_numa_target(boss, player);
        let boss_loc = world.objects[&boss].location;
        world.remove_object(player);
        world.now = 2500;
        world.process_numa_action(boss);
        assert_eq!(world.objects[&boss].location, boss_loc);
    }

    #[test]
    fn zuma_guardian_wakes_king_and_king_waits_two_seconds() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let guardian = world
            .test_spawn_ai(21, map, loc.step(Direction::Up, 1))
            .unwrap();
        let king = world
            .test_spawn_ai(22, map, loc.step(Direction::Up, 7))
            .unwrap();
        let profile = world.ai_profile(21);
        assert!(!world.process_hidden(guardian, &profile.hidden.unwrap(), &profile));
        assert!(!world.objects[&king].monster_ref().hidden);
        assert_eq!(world.objects[&king].monster_ref().target, Some(player));
        assert_eq!(world.objects[&king].action_time, world.now + 2000);
    }

    #[test]
    fn emperor_splash_is_physical_ranged_attack() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss = world
            .test_spawn_ai(41, map, loc.step(Direction::Up, 5))
            .unwrap();
        world.monster_cast(
            boss,
            player,
            Spell::Aoe {
                radius: 2,
                magic: magic_type::MONSTER_SPLASH,
                element: element::NONE,
                at_self: false,
                scale: 100,
            },
        );
        assert!(world.events.iter().any(|(_, m)| matches!(m, ServerMessage::ObjectRangeAttack { id, target: Some(t), .. } if *id == boss && *t == player)));
        assert!(world.pending_monster_spells.is_empty());
        assert!(world.pending_hits.iter().any(|h| h.attacker == boss
            && h.target == Some(player)
            && h.time == world.now + 400
            && h.element == world.monster_attack_element(boss)));
    }

    #[test]
    fn redmoon_attacks_target_on_its_own_cell() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss = world.test_spawn_ai(19, map, loc).unwrap();
        let profile = world.ai_profile(19);
        world.process_target(boss, player, &profile);
        assert!(world
            .pending_hits
            .iter()
            .any(|h| h.attacker == boss && h.target == Some(player)));
        assert_eq!(world.objects[&boss].direction, Direction::Up);
    }

    #[test]
    fn sama_guardians_and_bosses_actually_cast_their_kits() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        for ai in 107..=114 {
            let boss = world
                .test_spawn_ai(ai, map, loc.step(Direction::Up, 5))
                .unwrap();
            let profile = world.ai_profile(ai);
            assert_eq!(profile.attack_range, 2);
            world.objects.get_mut(&boss).unwrap().move_time = u64::MAX;
            world.events.clear();
            for _ in 0..50 {
                world.now += 5000;
                world.process_target(boss, player, &profile);
            }
            assert!(
                world.events.iter().any(
                    |(_, m)| matches!(m, ServerMessage::ObjectMagic { id, .. } if *id == boss)
                ),
                "AI {ai} never cast"
            );
            world.remove_object(boss);
        }
    }

    #[test]
    fn dragon_lord_targets_multiple_separated_players() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let second = world
            .add_player(2, 2, &crate::accounts::test_character("LordSecondTarget"))
            .unwrap();
        world.teleport(second, loc.step(Direction::Right, 6));
        let boss = world
            .test_spawn_ai(102, map, loc.step(Direction::Up, 5))
            .unwrap();
        let profile = world.ai_profile(102);
        for _ in 0..30 {
            world.monster_attack_shape(boss, player, &profile);
        }
        for target in [player, second] {
            assert!(world.pending_hits.iter().any(|h| h.attacker == boss
                && h.target == Some(target)
                && h.time == world.now + 1000));
        }
    }

    #[test]
    fn jinchon_death_cloud_bursts_twice_for_fixed_physical_damage() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss = world
            .test_spawn_ai(78, map, loc.step(Direction::Up, 5))
            .unwrap();
        world.death_cloud(boss, map, loc);
        let at = world
            .objects
            .values()
            .find_map(|o| match &o.kind {
                Kind::Spell(s) if s.effect == spell_effect::DEATH_CLOUD && o.location == loc => {
                    Some(s.tick_time)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(at, world.now + 4500);
        world.now = at;
        world.process_spells();
        let hits: Vec<_> = world
            .pending_hits
            .iter()
            .filter(|h| h.target == Some(player))
            .collect();
        assert_eq!(hits.len(), 2);
        assert!(hits
            .iter()
            .all(|h| h.power == 4000 && h.element == element::NONE && !h.raw));
    }

    #[test]
    fn removed_stray_minion_can_remain_in_tick_snapshot() {
        let Some((mut world, _, map, loc)) = setup() else {
            return;
        };
        let boss = world
            .test_spawn_ai(101, map, loc.step(Direction::Up, 5))
            .unwrap();
        world.process_boss_minions(boss, 101);
        let minion = world.objects[&boss].monster_ref().minions[0];
        world.teleport_object(minion, loc.step(Direction::Down, 25));
        world.now = 15_000;
        world.process_boss_minions(boss, 101);
        assert!(!world.objects.contains_key(&minion));
        // Tick snapshots IDs before bosses remove strays.
        world.process_monster(minion);
        world.tick(15_001);
    }

    #[test]
    fn ni_jae_can_apply_both_green_poison_and_paralysis() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss = world
            .test_spawn_ai(77, map, loc.step(Direction::Up, 2))
            .unwrap();
        for _ in 0..100 {
            world.monster_hit_poison(boss, player);
        }
        assert!(has_poison(&world.objects[&player], poison_kind::GREEN));
        assert!(has_poison(&world.objects[&player], poison_kind::PARALYSIS));
    }

    #[test]
    fn sama_prophet_spawns_sorcerer_and_protection_ends_after_removal() {
        let Some((mut world, player, map, loc)) = setup() else {
            return;
        };
        let boss = world
            .test_spawn_ai(115, map, loc.step(Direction::Up, 5))
            .unwrap();
        let minions = world.objects[&boss].monster_ref().minions.clone();
        assert_eq!(minions.len(), 1);
        assert_eq!(world.monster_flag(minions[0]), 190);
        assert_eq!(world.test_damage(boss, player, 100), 0);
        world.teleport_object(minions[0], loc.step(Direction::Down, 25));
        world.process_boss_minions(boss, 115);
        assert!(
            world.objects[&minions[0]]
                .location
                .distance(world.objects[&boss].location.step(Direction::DownLeft, 2))
                <= 1
        );
        world.remove_object(minions[0]);
        assert!(world.test_damage(boss, player, 100) > 0);
    }
}
