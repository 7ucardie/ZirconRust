//! NPC weapon templates and colour-ingredient stat rolls.
use super::*;
use crate::items::{weapon_craft_pick, weapon_craft_total, StatSource, UserItem};

impl World {
    pub fn npc_weapon_craft(
        &mut self,
        id: ObjectId,
        base: (u8, u32),
        materials: [Option<(u8, u32)>; 6],
        class: Class,
    ) {
        if let Err(reason) = self.craft_weapon(id, base, materials, class) {
            self.say_system(id, reason.into());
        }
    }

    fn craft_weapon(
        &mut self,
        id: ObjectId,
        base: (u8, u32),
        materials: [Option<(u8, u32)>; 6],
        class: Class,
    ) -> Result<(), &'static str> {
        if self.npc_dialog_type(id) != Some(14) {
            return Err("Visit a weapon-crafting NPC first.");
        }
        let o = self.objects.get(&id).ok_or("Player unavailable.")?;
        if o.dead {
            return Err("You cannot craft while dead.");
        }
        let p = o.player().ok_or("Player unavailable.")?;
        let npc = p
            .npc
            .and_then(|(npc, _)| self.objects.get(&npc))
            .ok_or("The crafting NPC is unavailable.")?;
        if !matches!(npc.kind, Kind::Npc(_))
            || npc.map != o.map
            || npc.location.distance(o.location) > MAX_VIEW_RANGE
        {
            return Err("Move closer to the weapon-crafting NPC.");
        }
        let cell = |link: (u8, u32)| {
            p.bag
                .inventory
                .get(link.0 as usize)
                .and_then(Option::as_ref)
                .filter(|i| i.id == link.1 && i.count > 0)
        };
        let base_item = cell(base).ok_or("Select the weapon or template again.")?;
        let def = self
            .data
            .items
            .get(&base_item.info)
            .ok_or("Unknown base item.")?;
        let template = def.effect == 60;
        if !template
            && (def.item_type != item_type::WEAPON || def.effect == 91 || base_item.count != 1)
        {
            return Err("Use a weapon template or a single ordinary weapon.");
        }
        let cost = if template {
            1_000_000
        } else {
            match def.rarity {
                0 => 30_000_000,
                1 => 60_000_000,
                _ => 80_000_000,
            }
        };
        if p.bag.gold < cost {
            return Err("Not enough gold to craft this weapon.");
        }
        let mut seen = vec![base.0];
        let mut rolls = 0i32;
        for (colour, link) in materials.iter().enumerate() {
            let Some(link) = link else { continue };
            if seen.contains(&link.0) {
                return Err("Each ingredient must use a different inventory slot.");
            }
            seen.push(link.0);
            let item = cell(*link).ok_or("Select the ingredients again.")?;
            let material = self
                .data
                .items
                .get(&item.info)
                .ok_or("Unknown ingredient.")?;
            if material.effect != 70 + colour as u8 || material.shape < 0 {
                return Err("Incorrect colour ingredient.");
            }
            rolls = rolls
                .checked_add(material.shape)
                .filter(|n| *n <= 1000)
                .ok_or("Too many stat rolls.")?;
        }
        let total = weapon_craft_total(&self.data, class);
        if total <= 0
            || self
                .data
                .weapon_craft_stats
                .iter()
                .any(|s| s.class & class.flag() != 0 && (s.weight < 0 || s.min_value > s.max_value))
        {
            return Err("Crafting stats are unavailable.");
        }
        let mut weapon = if template {
            let effect = match class {
                Class::Warrior => 61,
                Class::Wizard => 63,
                Class::Taoist => 64,
                Class::Assassin => 65,
            };
            let info = self
                .data
                .items
                .values()
                .filter(|d| d.effect == effect && d.item_type == item_type::WEAPON)
                .map(|d| d.index)
                .min()
                .ok_or("No weapon template output for that class.")?;
            let item = UserItem::fresh(&self.data, info, 1).ok_or("Unknown weapon.")?;
            if !p.bag.can_gain_item(&self.data, &item, p.max_bag) {
                return Err("Make room in your bag for the crafted weapon.");
            }
            item
        } else {
            let mut item = base_item.clone();
            item.level = 1;
            item.refinable = false;
            item.added.retain(|s| s.source == StatSource::Enhancement);
            item
        };
        for _ in 0..rolls {
            let (stat, min, max) =
                weapon_craft_pick(&self.data, class, self.rng.random_range(0..total))
                    .ok_or("Invalid crafting table.")?;
            let value = self.rng.random_range(min..=max);
            weapon.add_stat(stat, value, StatSource::Added);
        }
        let p = self.objects.get_mut(&id).unwrap().player_mut().unwrap();
        let mut changes = Vec::new();
        for link in materials.into_iter().flatten() {
            changes.extend(p.bag.take(Grid::Inventory, link.0, 1));
        }
        if template {
            changes.extend(p.bag.take(Grid::Inventory, base.0, 1));
            changes.extend(p.bag.gain_item(&self.data, weapon, &mut p.next_item_id));
        } else {
            changes.push((Grid::Inventory, base.0, Some(weapon.instance())));
            p.bag.inventory[base.0 as usize] = Some(weapon);
        }
        p.bag.gold -= cost;
        self.send_changes(id, changes);
        self.send_gold(id);
        self.say_system(id, "Your weapon has been crafted.".into());
        Ok(())
    }
}
