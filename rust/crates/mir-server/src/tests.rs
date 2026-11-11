//! World simulation tests (need `ZIRCON_ASSETS`; skipped otherwise).

use std::path::PathBuf;

use mir_proto::{Appearance, Direction, Grid, ObjectId, Point, ServerMessage};

use crate::accounts::test_character;
use crate::data::GameData;
use crate::world::{self, Outgoing, World};

fn world() -> Option<World> {
    let assets = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from)?;
    let data = GameData::load(assets.join("../Database/System.db")).ok()?;
    // Seeded so a failure reproduces; change the seed to explore other rolls.
    Some(World::new(data, assets.join("Map"), None).with_seed(0x5A1C0))
}

#[test]
fn leveling_emits_original_magic_ex_effect() {
    let Some(mut world) = world() else { return };
    let mut character = test_character("LevelGlowTester");
    character.next_item_id = 1;
    let id = world.add_player(1, 1, &character).unwrap();
    world.outgoing.clear();
    world.gain_experience_test(id, GameData::max_experience(1));
    world.tick(1);
    assert_eq!(world.objects[&id].player().unwrap().level, 2);
    assert!(world.outgoing.iter().any(|message| matches!(
        message,
        Outgoing::To(1, ServerMessage::ObjectEffect { id: target, effect: mir_proto::effect::LEVEL_UP, .. }) if *target == id
    )));
}

#[test]
fn npc_repair_charges_gold_and_applies_normal_or_special_durability() {
    let Some(mut world) = world() else { return };
    let (page, info) = world
        .data
        .npc_pages
        .values()
        .filter(|page| page.dialog_type == 2)
        .find_map(|page| {
            world
                .data
                .items
                .values()
                .find(|item| {
                    page.types.contains(&item.item_type)
                        && item.item_type != mir_proto::item_type::WEAPON
                        && item.can_repair
                        && item.durability >= 1500
                        && item.price > 0
                        && item.sell_rate > 0.0
                        && item.sell_rate < 1.0
                })
                .map(|item| (page.index, item.index))
        })
        .expect("repair page with a repairable non-weapon");
    let me = world
        .add_player(1, 1, &test_character("RepairTester"))
        .unwrap();
    world.test_give_item(me, info, 1);
    let slot = world.test_slot_of(me, info).unwrap();
    let item_id = {
        let item = world
            .objects
            .get_mut(&me)
            .unwrap()
            .player_mut()
            .unwrap()
            .bag
            .inventory[slot as usize]
            .as_mut()
            .unwrap();
        item.durability -= 1500;
        item.id
    };
    world.test_open_page(me, page);
    let before = world.objects[&me].player().unwrap().bag.inventory[slot as usize]
        .as_ref()
        .unwrap()
        .clone();
    let price = crate::items::repair_cost(&world.data.items[&info], &before, false);
    assert!(price > 0);
    world.test_set_gold(me, price - 1);
    world.npc_repair(me, vec![(Grid::Inventory, slot, item_id)], false);
    assert_eq!(
        world.objects[&me].player().unwrap().bag.inventory[slot as usize]
            .as_ref()
            .unwrap()
            .durability,
        before.durability
    );
    world.test_set_gold(me, price + 100);
    world.npc_repair(me, vec![(Grid::Inventory, slot, item_id)], false);
    let normal = world.objects[&me].player().unwrap().bag.inventory[slot as usize]
        .as_ref()
        .unwrap();
    assert_eq!(normal.max_durability, before.max_durability - 1500 / 15);
    assert_eq!(normal.durability, normal.max_durability);
    assert_eq!(world.objects[&me].player().unwrap().bag.gold, 100);

    let normal_max = normal.max_durability;
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[slot as usize]
        .as_mut()
        .unwrap()
        .durability -= 1500;
    let worn = world.objects[&me].player().unwrap().bag.inventory[slot as usize]
        .as_ref()
        .unwrap()
        .clone();
    let special_price = crate::items::repair_cost(&world.data.items[&info], &worn, true);
    world.test_set_gold(me, special_price + 100);
    world.npc_repair(me, vec![(Grid::Inventory, slot, item_id)], true);
    let special = world.objects[&me].player().unwrap().bag.inventory[slot as usize]
        .as_ref()
        .unwrap();
    assert_eq!(special.max_durability, normal_max);
    assert_eq!(special.durability, normal_max);
    assert!(special.special_repair_until > 0);
    assert_eq!(world.objects[&me].player().unwrap().bag.gold, 100);
}

#[test]
fn primary_attack_grows_with_every_level_for_each_class() {
    let Some(mut world) = world() else { return };
    for (index, class) in mir_proto::Class::ALL.into_iter().enumerate() {
        let mut rec = test_character(class.name());
        rec.class = class;
        rec.next_item_id = 1; // No starter equipment bonuses.
        let id = world.add_player(index as u64 + 1, 1, &rec).unwrap();
        let primary = |world: &World| {
            let s = &world.objects[&id].stats;
            match class {
                mir_proto::Class::Warrior | mir_proto::Class::Assassin => s.max_dc,
                mir_proto::Class::Wizard => s.max_mc,
                mir_proto::Class::Taoist => s.max_sc,
            }
        };
        let mut previous = primary(&world);
        for level in 1..90 {
            world.gain_experience_test(id, GameData::max_experience(level));
            let next = primary(&world);
            assert_eq!(world.objects[&id].player().unwrap().level, level + 1);
            assert!(
                next > previous,
                "{class:?} level {level} to {}: {previous} to {next}",
                level + 1
            );
            previous = next;
        }
    }
}

#[test]
fn character_levels_through_255_and_stops_at_1000() {
    let Some(mut world) = world() else { return };
    let id = world
        .add_player(1, 1, &test_character("level_cap"))
        .unwrap();
    let class = world.objects[&id].player().unwrap().class.mir_class();
    let hp_at_90 = world.data.player_base_stat(class, 90).unwrap().health;
    let hp_at_1000 = world.data.player_base_stat(class, 1000).unwrap().health;
    assert!(hp_at_1000 > hp_at_90);
    assert!(GameData::max_experience(255) > GameData::max_experience(100));

    world
        .objects
        .get_mut(&id)
        .unwrap()
        .player_mut()
        .unwrap()
        .level = 255;
    world.gain_experience_test(id, GameData::max_experience(255));
    assert_eq!(world.objects[&id].player().unwrap().level, 256);

    world
        .objects
        .get_mut(&id)
        .unwrap()
        .player_mut()
        .unwrap()
        .level = 999;
    world
        .objects
        .get_mut(&id)
        .unwrap()
        .player_mut()
        .unwrap()
        .experience = 0;
    world.gain_experience_test(id, GameData::max_experience(999));
    assert_eq!(world.objects[&id].player().unwrap().level, 1000);
    assert_eq!(world.objects[&id].player().unwrap().experience, 0);
    assert_eq!(GameData::max_experience(1000), 0);
    world.gain_experience_test(id, 1_000_000);
    assert_eq!(world.objects[&id].player().unwrap().level, 1000);
    assert_eq!(world.objects[&id].player().unwrap().experience, 0);
}

fn drain(world: &mut World) -> Vec<ServerMessage> {
    world
        .outgoing
        .drain(..)
        .map(|o| match o {
            Outgoing::To(_, m) => m,
        })
        .collect()
}

mod accessory_refine;
mod gm_options;
mod market;
mod poison_merge;
mod poison_target;
mod poison_tick;
mod sets;
mod special_drops;
mod store;
mod unfinished_features;
mod weapon_craft;

#[test]
fn life_pills_bundle_to_999() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("PillTester"))
        .unwrap();
    let pill = world
        .data
        .items
        .values()
        .find(|item| item.name == "Life Pill")
        .unwrap()
        .index;
    world.test_give_item(me, pill, 1200);
    let mut counts: Vec<_> = world.objects[&me]
        .player()
        .unwrap()
        .bag
        .inventory
        .iter()
        .flatten()
        .filter(|item| item.info == pill)
        .map(|item| item.count)
        .collect();
    counts.sort_unstable();
    assert_eq!(counts, [201, 999]);
}

#[test]
fn equipped_weapon_exposes_database_exterior_effect() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("EffectTester"))
        .unwrap();
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .level = 255;
    world.test_give_item(me, 1162, 1); // Chaotic Heaven Blade
    let slot = world.test_slot_of(me, 1162).unwrap();
    world.item_use(me, slot);
    assert_eq!(world.test_equipped(me, mir_proto::slot::WEAPON), Some(1162));
    assert!(matches!(
        &world.objects[&me].appearance,
        Appearance::Player {
            weapon_effect: 110,
            ..
        }
    ));
}

#[test]
fn chaotic_and_janitor_weapons_equip_for_their_required_classes() {
    let Some(mut world) = world() else { return };
    for (conn, class, item, shape, effect) in [
        (10, mir_proto::Class::Wizard, 1162, 44, 110),
        (11, mir_proto::Class::Wizard, 1163, 67, 111),
        (12, mir_proto::Class::Assassin, 1164, 1114, 112),
        (13, mir_proto::Class::Assassin, 1165, 1263, 110),
    ] {
        let mut character = test_character(&format!("Weapon{item}"));
        character.class = class;
        character.level = 70;
        character.next_item_id = 1;
        let player = world.add_player(conn, conn as u32, &character).unwrap();
        world.test_give_item(player, item, 1);
        world.item_use(player, world.test_slot_of(player, item).unwrap());
        assert_eq!(
            world.test_equipped(player, mir_proto::slot::WEAPON),
            Some(item)
        );
        assert!(matches!(world.objects[&player].appearance,
            Appearance::Player { weapon: Some(actual), weapon_effect, .. }
                if actual == shape && weapon_effect == effect));
    }
    let mut wizard = test_character("WrongClass");
    wizard.class = mir_proto::Class::Wizard;
    wizard.level = 70;
    wizard.next_item_id = 1;
    let player = world.add_player(14, 14, &wizard).unwrap();
    world.test_give_item(player, 1164, 1);
    world.item_use(player, world.test_slot_of(player, 1164).unwrap());
    assert_eq!(world.test_equipped(player, mir_proto::slot::WEAPON), None);
    assert!(world.outgoing.iter().any(|message| matches!(message,
        Outgoing::To(14, ServerMessage::Chat { text }) if text == "Your class cannot use this item"
    )));
}

#[test]
fn royal_and_blue_dragon_horse_armour_equip_and_keep_their_shapes() {
    let Some(mut world) = world() else { return };
    let mut character = test_character("HorseArmour");
    character.level = 70;
    character.next_item_id = 1;
    let player = world.add_player(1, 1, &character).unwrap();
    {
        let rider = world
            .objects
            .get_mut(&player)
            .unwrap()
            .player_mut()
            .unwrap();
        rider.mounted = true;
        rider.horse = 1;
    }
    for (item, shape) in [(1166, 6), (1167, 7)] {
        world.test_give_item(player, item, 1);
        world.item_use(player, world.test_slot_of(player, item).unwrap());
        assert_eq!(
            world.test_equipped(player, mir_proto::slot::HORSE_ARMOUR),
            Some(item)
        );
        assert!(matches!(world.objects[&player].appearance,
            Appearance::Player { horse_shape, horse: 1, .. } if horse_shape == shape));
    }
}

#[test]
fn benediction_oil_changes_weapon_luck_and_preserves_oil_on_invalid_use() {
    let Some(mut world) = world() else { return };
    assert_eq!(world.data.items[&296].shape, 4);
    let mut character = test_character("BenedictionTester");
    character.next_item_id = 1; // No starter weapon.
    let me = world.add_player(1, 1, &character).unwrap();
    world.test_give_item(me, 296, 1);
    let oil_slot = world.test_slot_of(me, 296).unwrap();
    world.item_use(me, oil_slot);
    assert_eq!(
        world.objects[&me].player().unwrap().bag.inventory[oil_slot as usize]
            .as_ref()
            .unwrap()
            .count,
        1
    );

    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .level = 255;
    world.test_give_item(me, 1162, 1);
    let weapon_slot = world.test_slot_of(me, 1162).unwrap();
    world.item_use(me, weapon_slot);
    let before_luck = world.objects[&me].stats.luck;
    world.outgoing.clear();
    world.item_use(me, oil_slot);
    let p = world.objects[&me].player().unwrap();
    let weapon = p.bag.equipment[mir_proto::slot::WEAPON].as_ref().unwrap();
    let change = weapon
        .added
        .iter()
        .find(|s| {
            s.stat == mir_formats::mirdb::stat::LUCK
                && s.source == crate::items::StatSource::Enhancement
        })
        .unwrap()
        .amount;
    assert!(change == 1 || change == -1);
    assert_eq!(world.objects[&me].stats.luck, before_luck + change);
    assert!(p.bag.inventory[oil_slot as usize].is_none());
    assert!(world.outgoing.iter().any(|event| matches!(event,
        Outgoing::To(1, ServerMessage::ItemChanged {
            grid: Grid::Equipment,
            slot: 0,
            item: Some(item),
        }) if item.enhancements.contains(&(mir_formats::mirdb::stat::LUCK, change))
    )));

    let weapon = world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .equipment[mir_proto::slot::WEAPON]
        .as_mut()
        .unwrap();
    weapon.add_stat(
        mir_formats::mirdb::stat::LUCK,
        7 - change,
        crate::items::StatSource::Enhancement,
    );
    world.test_give_item(me, 296, 1);
    let capped_oil_slot = world.test_slot_of(me, 296).unwrap();
    world.item_use(me, capped_oil_slot);
    assert_eq!(
        world.objects[&me].player().unwrap().bag.inventory[capped_oil_slot as usize]
            .as_ref()
            .unwrap()
            .count,
        1
    );
}

#[test]
fn wardrobe_requires_owned_armour_changes_only_appearance_and_survives_snapshot() {
    let Some(mut world) = world() else { return };
    let def = world
        .data
        .items
        .values()
        .find(|def| def.item_type == mir_proto::item_type::ARMOUR && def.shape > 0 && def.can_drop)
        .expect("armour with a body sprite")
        .clone();
    // Installed defaults have no armour ExteriorEffect; authored armour may.
    world
        .data
        .items
        .get_mut(&def.index)
        .unwrap()
        .exterior_effect = 42;
    let mut saved = test_character("WardrobeTester");
    let me = world.add_player(1, 1, &saved).unwrap();
    let viewer = world
        .add_player(2, 2, &test_character("WardrobeViewer"))
        .unwrap();
    let base = (world.objects[&me].stats.max_dc, world.objects[&me].max_hp);
    world.set_wardrobe(me, Some(def.index));
    assert_eq!(world.objects[&me].player().unwrap().wardrobe_item, None);
    world.test_give_item(me, def.index, 1);
    let slot = world.test_slot_of(me, def.index).unwrap();
    world.tick(0);
    drain(&mut world);
    world.set_wardrobe(me, Some(def.index));
    world.tick(1);
    assert!(world.outgoing.iter().any(|out| matches!(
        out,
        Outgoing::To(2, ServerMessage::ObjectAppearance { id, appearance: Appearance::Player { armour, .. } })
            if *id == me && *armour == def.shape as u16
    )), "visible player did not receive the new look");
    assert!(drain(&mut world).iter().any(|msg| matches!(
        msg,
        ServerMessage::ObjectAppearance { id, appearance: Appearance::Player { armour, .. } }
            if *id == me && *armour == def.shape as u16
    )));
    assert!(matches!(
        &world.objects[&me].appearance,
        Appearance::Player { armour, armour_effect, .. }
            if *armour == def.shape as u16 && *armour_effect == 42
    ));
    assert_eq!(
        (world.objects[&me].stats.max_dc, world.objects[&me].max_hp),
        base
    );
    world.snapshot(me, &mut saved);
    assert_eq!(saved.wardrobe_item, Some(def.index));
    world.item_drop(me, slot, 1);
    assert_eq!(
        world.objects[&me].player().unwrap().wardrobe_item,
        Some(def.index)
    );
    world.snapshot(me, &mut saved);
    assert!(saved.items.iter().all(|item| item.item.info != def.index));
    let reloaded = world.add_player(3, 3, &saved).unwrap();
    assert_ne!(reloaded, viewer);
    assert_eq!(
        world.objects[&reloaded].player().unwrap().wardrobe_item,
        Some(def.index)
    );
    assert!(matches!(
        world.objects[&reloaded].appearance,
        Appearance::Player { armour, .. } if armour == def.shape as u16
    ));
    world.set_wardrobe(me, None);
    assert_eq!(world.objects[&me].player().unwrap().wardrobe_item, None);
}

#[test]
fn wardrobe_helmet_weapon_and_shoes_require_owned_items_and_survive_relog() {
    let Some(mut world) = world() else { return };
    let find = |kind| {
        world
            .data
            .items
            .values()
            .find(|def| def.item_type == kind && def.shape > 0)
            .unwrap()
            .index
    };
    let helmet = find(mir_proto::item_type::HELMET);
    let weapon = world
        .data
        .items
        .values()
        .find(|def| {
            def.item_type == mir_proto::item_type::WEAPON
                && def.shape > 0
                && def.exterior_effect > 0
        })
        .unwrap()
        .index;
    let shoes = world
        .data
        .items
        .values()
        .find(|def| def.item_type == mir_proto::item_type::SHOES)
        .unwrap()
        .index;
    let mut saved = test_character("WardrobeSlots");
    let me = world.add_player(1, 1, &saved).unwrap();
    let base = world.objects[&me].stats.max_dc;
    world.set_wardrobe_slot(me, mir_proto::slot::HELMET, Some(helmet));
    assert_eq!(world.objects[&me].player().unwrap().wardrobe_helmet, None);
    world.test_give_item(me, helmet, 1);
    world.test_give_item(me, weapon, 1);
    world.test_give_item(me, shoes, 1);
    let shoe_slot = world.test_slot_of(me, shoes).unwrap() as usize;
    {
        let bag = &mut world
            .objects
            .get_mut(&me)
            .unwrap()
            .player_mut()
            .unwrap()
            .bag;
        bag.storage[0] = bag.inventory[shoe_slot].take();
    }
    world.set_wardrobe_slot(me, mir_proto::slot::HELMET, Some(weapon));
    assert_eq!(world.objects[&me].player().unwrap().wardrobe_helmet, None);
    world.set_wardrobe_slot(me, mir_proto::slot::HELMET, Some(helmet));
    world.set_wardrobe_slot(me, mir_proto::slot::WEAPON, Some(weapon));
    world.set_wardrobe_slot(me, mir_proto::slot::SHOES, Some(shoes));
    let p = world.objects[&me].player().unwrap();
    assert_eq!(
        (p.wardrobe_helmet, p.wardrobe_weapon, p.wardrobe_shoes),
        (Some(helmet), Some(weapon), Some(shoes))
    );
    assert_eq!(world.objects[&me].stats.max_dc, base);
    assert!(matches!(&world.objects[&me].appearance,
        Appearance::Player { helmet: h, weapon: Some(w), weapon_effect, .. }
            if *h == world.data.items[&helmet].shape as u16
                && *w == world.data.items[&weapon].shape as u16
                && *weapon_effect == world.data.items[&weapon].exterior_effect));
    world.snapshot(me, &mut saved);
    let restored = world.add_player(2, 2, &saved).unwrap();
    let p = world.objects[&restored].player().unwrap();
    assert_eq!(
        (p.wardrobe_helmet, p.wardrobe_weapon, p.wardrobe_shoes),
        (Some(helmet), Some(weapon), Some(shoes))
    );
}

#[test]
fn wyvern_armour_has_authored_blue_glow_without_lighting_commoner_outfit() {
    let Some(mut world) = world() else { return };
    let mut character = test_character("ArmourGlow");
    character.gender = mir_proto::Gender::Female;
    character.level = 70;
    character.next_item_id = 1;
    let me = world.add_player(1, 1, &character).unwrap();
    assert_eq!(world.data.items[&351].exterior_effect, 0);
    assert_eq!(mir_proto::equipment_visual::armour_effect(128, 0), 0);
    world.test_give_item(me, 128, 1);
    let commoner = world.test_slot_of(me, 128).unwrap();
    world.item_use(me, commoner);
    assert!(matches!(
        world.objects[&me].appearance,
        Appearance::Player {
            armour_effect: 0,
            ..
        }
    ));
    world.test_give_item(me, 351, 1);
    let wyvern = world.test_slot_of(me, 351).unwrap();
    world.item_use(me, wyvern);
    assert_eq!(world.test_equipped(me, mir_proto::slot::ARMOUR), Some(351));
    assert!(matches!(
        world.objects[&me].appearance,
        Appearance::Player {
            armour_effect: 3,
            ..
        }
    ));
}

#[test]
fn owned_odyn_looks_get_weapon_shaped_glow_and_preserve_stats() {
    let Some(mut world) = world() else { return };
    let me = world.add_player(1, 1, &test_character("OdynGlow")).unwrap();
    let base = world.objects[&me].stats.max_dc;
    assert_eq!(mir_proto::equipment_visual::weapon_effect(1069, 111), 111);
    assert_eq!(mir_proto::equipment_visual::weapon_effect(100, 0), 0);
    for (item, effect) in [(1069, 240), (1070, 241), (1072, 242), (1079, 243)] {
        assert_eq!(world.data.items[&item].exterior_effect, 0);
        world.test_give_item(me, item, 1);
        world.set_wardrobe_slot(me, mir_proto::slot::WEAPON, Some(item));
        assert!(matches!(world.objects[&me].appearance,
            Appearance::Player { weapon: Some(136), weapon_effect, .. } if weapon_effect == effect));
    }
    assert_eq!(world.objects[&me].stats.max_dc, base);
}

#[test]
fn mounted_player_can_equip_mark_but_cannot_drink_potion() {
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Rider")).unwrap();
    let player = world.objects.get_mut(&me).unwrap().player_mut().unwrap();
    player.level = 15;
    player.mounted = true;
    world.test_give_item(me, 709, 1); // Mark Of Destruction [T]
    let slot = world.test_slot_of(me, 709).unwrap();
    world.item_use(me, slot);
    assert_eq!(world.test_equipped(me, mir_proto::slot::TORCH), Some(709));

    world.test_give_item(me, 793, 1); // Thorned Rose
    let slot = world.test_slot_of(me, 793).unwrap();
    world.item_use(me, slot);
    assert_eq!(world.test_equipped(me, mir_proto::slot::FLOWER), Some(793));

    let potion = world
        .data
        .items
        .values()
        .find(|item| item.name == "Healing Potion")
        .unwrap()
        .index;
    world.test_give_item(me, potion, 1);
    let slot = world.test_slot_of(me, potion).unwrap();
    world.item_use(me, slot);
    assert!(drain(&mut world).iter().any(|message| matches!(message,
        ServerMessage::Chat { text } if text == "You cannot use items while riding"
    )));
    assert!(world.test_slot_of(me, potion).is_some());
}

#[test]
fn town_scroll_chooses_the_nearest_exit_from_phantom_forest() {
    let Some(mut world) = world() else {
        return;
    };
    world.ensure_map(12).unwrap();
    for town in [1, 6] {
        let movement = world
            .data
            .movements
            .iter()
            .find(|movement| {
                world.data.regions[&movement.source_region].map == 12
                    && world.data.regions[&movement.destination_region].map == town
            })
            .unwrap();
        let width = world.maps[&12].file.width as i32;
        let (x, y) = world.data.regions[&movement.source_region].movement_points(width)[0];
        let zone = world
            .test_nearest_town_region(12, Point::new(x, y))
            .unwrap();
        assert_eq!(world.data.regions[&zone].map, town);
    }
}

#[test]
fn startup_rates_control_drop_specials_and_experience() {
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Rates")).unwrap();
    world.test_give_item(me, 126, 1); // starting weapon
    let slot = world.test_slot_of(me, 126).unwrap();
    let plain = world.objects[&me].player().unwrap().bag.inventory[slot as usize]
        .clone()
        .unwrap();
    let mut item = plain.clone();
    for _ in 0..100 {
        world.test_roll_drop_bonus(&mut item);
    }
    assert!(item.added.is_empty());

    world.drop_specials = true;
    let mut found_mc = false;
    for _ in 0..1000 {
        let mut item = plain.clone();
        world.test_roll_drop_bonus(&mut item);
        found_mc |= item.bonus(mir_formats::mirdb::stat::MAX_MC) == 1;
    }
    assert!(found_mc, "enabled drops can gain +1 MC");

    world.experience_multiplier = 2.0;
    world.gain_experience_test(me, 10);
    assert_eq!(world.test_experience(me), 20);
}

#[test]
fn xp_rates_rebirth_fraction_and_reward_weapon_experience() {
    let Some(mut world) = world() else { return };
    let me = world.add_player(1, 1, &test_character("XpRules")).unwrap();
    world.data.items.get_mut(&968).unwrap().stats.extend([
        (mir_formats::mirdb::stat::EXPERIENCE_RATE, 50),
        (mir_formats::mirdb::stat::BASE_EXPERIENCE_RATE, 100),
    ]);
    world.test_give_item(me, 968, 1);
    let slot = world.test_slot_of(me, 968).unwrap();
    {
        let p = world.objects.get_mut(&me).unwrap().player_mut().unwrap();
        p.bag.equipment[0] = p.bag.inventory[slot as usize].take();
        p.bag.equipment[0].as_mut().unwrap().level = 1;
        p.rebirth = 1;
    }
    world.experience_multiplier = 2.0;
    world.gain_reward_experience_test(me, 20);
    let p = world.objects[&me].player().unwrap();
    assert_eq!(p.experience, 60); // 20 * 1.5 * 2 * 2 * 0.5
    assert_eq!(
        p.bag.equipment[0]
            .as_ref()
            .unwrap()
            .weapon_experience_tenths,
        60
    );

    // Old saves and repeated small awards retain fractional C# decimal XP.
    let mut record = test_character("XpRules");
    let p = world.objects.get_mut(&me).unwrap().player_mut().unwrap();
    p.bag.equipment[0] = None;
    p.experience = 0;
    p.experience_fraction = 0.0;
    world.experience_multiplier = 1.0;
    world.gain_reward_experience_test(me, 1);
    assert_eq!(world.test_experience(me), 0);
    world.snapshot(me, &mut record);
    assert_eq!(record.experience_fraction, 0.5);
    world.gain_reward_experience_test(me, 1);
    assert_eq!(world.test_experience(me), 1);
    assert_eq!(
        world.objects[&me].player().unwrap().experience_fraction,
        0.0
    );

    let guildie = world.add_player(2, 2, &test_character("GuildXp")).unwrap();
    world
        .objects
        .get_mut(&guildie)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .gold = 10_000_000;
    world.guild_create(guildie, "XpGuild".into(), 1);
    world.gain_reward_experience_test(guildie, 10);
    assert_eq!(world.test_experience(guildie), 13); // C# guild buff: +30% at <=15 members.

    let mut pet_record = test_character("PetXp");
    pet_record.id = 3;
    let pet_owner = world.add_player(3, 3, &pet_record).unwrap();
    let p = world
        .objects
        .get_mut(&pet_owner)
        .unwrap()
        .player_mut()
        .unwrap();
    p.companions.push(world::StoredCompanion {
        index: 1,
        info: 1,
        name: "Pet".into(),
        level: 3,
        experience: 0,
        hunger: 10,
        items: Vec::new(),
        food: None,
        bag: None,
        bonuses: vec![(3, mir_formats::mirdb::stat::EXPERIENCE_RATE, 50)],
    });
    p.active_companion = Some(1);
    world.gain_reward_experience_test(pet_owner, 2);
    assert_eq!(world.test_experience(pet_owner), 3);
    world
        .objects
        .get_mut(&pet_owner)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions[0]
        .hunger = 0;
    world.gain_reward_experience_test(pet_owner, 2);
    assert_eq!(world.test_experience(pet_owner), 5);

    let mut legacy = serde_json::to_value(test_character("LegacyXp")).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("experience_fraction");
    let loaded: crate::accounts::CharacterRecord = serde_json::from_value(legacy).unwrap();
    assert_eq!(loaded.experience_fraction, 0.0);
}

#[test]
fn mana_potion_restores_mp() {
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Mana")).unwrap();
    for info in [143, 152] {
        let expected = if info == 143 { 40 } else { 320 };
        assert_eq!(
            world.data.items[&info].stat(mir_formats::mirdb::stat::MANA),
            expected
        );
        let player = world.objects.get_mut(&me).unwrap().player_mut().unwrap();
        player.mp = 0;
        player.use_item_time = 0;
        world.test_give_item(me, info, 1);
        let slot = world.test_slot_of(me, info).unwrap();
        world.item_use(me, slot);
        assert!(world.objects[&me].player().unwrap().mp > 0);
        assert!(drain(&mut world).iter().any(|message| matches!(message,
            ServerMessage::StatsChanged(stats) if stats.mp > 0
        )));
    }
}

#[test]
fn warrior_kills_adjacent_monster_and_gains_experience() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Tester")).unwrap();
    world.tick(0);
    let msgs = drain(&mut world);
    assert!(matches!(msgs[0], ServerMessage::Welcome { .. }));
    let map = world.objects[&me].map;
    let loc = world.objects[&me].location;
    // Find any live monster on the map and teleport it next to us.
    // A chicken: 7 HP, 0 AC, agility 5, so a level-1 warrior lands hits.
    let victim = world
        .objects
        .values()
        .find(|o| {
            !o.dead
                && matches!(&o.appearance, Appearance::Monster { name, .. } if name == "Chicken")
        })
        .map(|o| o.id)
        .expect("a chicken");
    let target_cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 1))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .expect("walkable neighbour");
    world.teleport(victim, target_cell);
    let victim_hp = world.objects[&victim].max_hp;
    assert!(victim_hp > 0);
    let dir = Direction::from_points(loc, target_cell);
    // Swing until it dies (accuracy/agility rolls can miss).
    let mut now = 100;
    let mut kills = 0;
    for _ in 0..400 {
        world.player_attack(me, dir, None);
        now += 100;
        world.tick(now);
        now += 100;
        world.tick(now);
        now += 1400;
        world.tick(now);
        if world.objects.get(&victim).map(|v| v.dead).unwrap_or(true) {
            kills += 1;
            break;
        }
    }
    assert_eq!(kills, 1, "monster should die from repeated hits");
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::ObjectStruck { id, .. } if *id == victim)));
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::ObjectDie { id } if *id == victim)));
    let exp = world.objects[&me].player().unwrap().experience;
    let level = world.objects[&me].player().unwrap().level;
    assert!(exp > 0 || level > 1, "experience should be granted");
}

#[test]
fn potions_heal_instantly_with_a_durability_cooldown() {
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Tester")).unwrap();
    world.tick(1000);
    drain(&mut world);
    // Healing Potion: Shape 0, Durability 2000 ms cooldown.
    let potion = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .map(|d| d.index)
        .expect("Healing Potion");
    world.test_give_item(me, potion, 3);
    let slot = world.test_slot_of(me, potion).unwrap();
    let (_, max_hp, _) = world.test_hp(me);
    world.test_set_hp(me, 1);
    world.item_use(me, slot);
    let (hp, _, _) = world.test_hp(me);
    assert!(
        hp > 1 && hp <= max_hp,
        "potion should heal at once: {hp}/{max_hp}"
    );
    let count = |w: &World| {
        w.test_bag(me)
            .1
            .iter()
            .find(|(i, _)| *i == potion)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    };
    assert_eq!(count(&world), 2);
    // Still on cooldown: nothing consumed.
    world.tick(2000);
    world.item_use(me, slot);
    assert_eq!(
        count(&world),
        2,
        "second use inside the cooldown must be ignored"
    );
    world.tick(3100);
    world.item_use(me, slot);
    assert_eq!(count(&world), 1, "use after the cooldown consumes a potion");

    // Belt: link the potion type to slot 0, persist it, and reject a link
    // to an item that is not in the bag.
    world.belt_link(me, 0, Some(potion), None);
    world.belt_link(me, 1, None, Some(999_999));
    let belt = world.test_belt(me);
    assert_eq!(belt[0].info, Some(potion));
    assert_eq!(belt[1].item, None);
    let mut rec = test_character("Tester");
    world.snapshot(me, &mut rec);
    assert_eq!(rec.belt[0].info, Some(potion));
}

#[test]
fn dead_player_returns_to_town_on_request() {
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Tester")).unwrap();
    world.tick(1000);
    drain(&mut world);
    world.test_kill(me);
    assert!(world.test_hp(me).2, "player should be dead");
    // Well before the 10 minute forced revive.
    world.tick(5000);
    assert!(world.test_hp(me).2, "no automatic revive after 4 s");
    world.town_revive(me);
    let (hp, max_hp, dead) = world.test_hp(me);
    assert!(!dead);
    assert_eq!(hp, max_hp);
    let (map, loc) = (world.objects[&me].map, world.objects[&me].location);
    assert!(world.maps[&map].file.is_walkable(loc.x, loc.y));
}

#[test]
fn chicken_must_be_harvested_before_its_meat_can_be_sold() {
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Tester")).unwrap();
    world.tick(0);
    drain(&mut world);
    let map = world.objects[&me].map;
    let loc = world.objects[&me].location;
    let victim = world
        .objects
        .values()
        .find(|o| {
            !o.dead
                && matches!(&o.appearance, Appearance::Monster { name, .. } if name == "Chicken")
        })
        .map(|o| o.id)
        .expect("a chicken");
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 1))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.teleport(victim, cell);
    let dir = Direction::from_points(loc, cell);
    let mut now = 100;
    for _ in 0..400 {
        world.player_attack(me, dir, None);
        now += 1600;
        world.tick(now);
        if world.objects.get(&victim).map(|v| v.dead).unwrap_or(true) {
            break;
        }
    }
    // Chicken Meat is guaranteed, but stays on the body until three swings.
    assert!(!world
        .objects
        .values()
        .any(|o| matches!(o.appearance, Appearance::Item { info: 179, .. })));
    for swing in 0..3 {
        now += 600;
        world.tick(now);
        world.harvest(me, dir);
        if swing < 2 {
            assert!(!world.test_bag(me).1.iter().any(|(info, _)| *info == 179));
        }
    }
    assert!(world.test_bag(me).1.iter().any(|(info, _)| *info == 179));
    assert!(world.objects[&victim]
        .monster()
        .unwrap()
        .harvest_drops
        .is_empty());

    // Sell it to the meat shop page (4) which buys ItemType 12.
    world.test_set_gold(me, 0);
    world.test_open_page(me, 4);
    let slots = world.objects[&me]
        .player()
        .unwrap()
        .bag
        .inventory
        .iter()
        .enumerate()
        .filter_map(|(i, cell)| cell.as_ref().filter(|it| it.info == 179).map(|_| i as u8))
        .map(|slot| (slot, 1))
        .collect();
    world.npc_sell(me, slots);
    let (gold, bag) = world.test_bag(me);
    assert!(gold > 0, "selling should pay gold");
    assert!(!bag.iter().any(|(info, _)| *info == 179));

    // Buy one back.
    world.test_set_gold(me, 100_000);
    world.npc_buy(me, 179, 1);
    let (gold, bag) = world.test_bag(me);
    assert!(gold < 100_000);
    assert!(bag.iter().any(|(info, c)| *info == 179 && *c == 1));
}

#[test]
fn walking_onto_an_exit_changes_map() {
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Tester")).unwrap();
    world.tick(0);
    drain(&mut world);
    let map = world.objects[&me].map;
    let exits = world.test_movement_cells(map);
    assert!(!exits.is_empty(), "Bichon should have exits");
    // Find an exit with a walkable neighbour to step from.
    let mut done = false;
    let mut now = 1000;
    for exit in exits.iter() {
        now += 1000;
        let Some(from) = Direction::ALL
            .iter()
            .map(|d| exit.step(*d, 1))
            .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        else {
            continue;
        };
        // Nothing may stand on the exit cell.
        let blockers: Vec<ObjectId> = world
            .objects
            .values()
            .filter(|o| o.id != me && o.map == map && o.location == *exit)
            .map(|o| o.id)
            .collect();
        for b in blockers {
            world.remove_object(b);
        }
        world.teleport(me, from);
        world.tick(now);
        drain(&mut world);
        let dir = Direction::from_points(from, *exit);
        world.player_move(me, dir, false);
        let msgs = drain(&mut world);
        if world.objects[&me].map != map {
            assert!(msgs
                .iter()
                .any(|m| matches!(m, ServerMessage::MapChanged { .. })));
            done = true;
            break;
        }
    }
    assert!(
        done,
        "no exit led anywhere (level requirement or unloaded map)"
    );
}

/// Give the player a book for `magic_name`'s MagicInfo and learn it.
fn learn(world: &mut World, me: ObjectId, magic_name: &str) -> u16 {
    let def = world
        .data
        .magics
        .values()
        .find(|m| m.name == magic_name)
        .cloned()
        .expect("magic");
    let book = world
        .data
        .items
        .values()
        .find(|i| i.item_type == 14 && i.shape == def.index)
        .map(|i| i.index)
        .expect("book item");
    world.test_give_item(me, book, 1);
    let slot = world.test_slot_of(me, book).expect("book in bag");
    world.item_use(me, slot);
    let known = world.test_magics(me);
    assert!(
        known.contains(&def.magic),
        "should know {magic_name}: {known:?}"
    );
    def.magic
}

fn nearest_chicken(world: &World, me: ObjectId) -> ObjectId {
    let loc = world.objects[&me].location;
    let map = world.objects[&me].map;
    world
        .objects
        .values()
        .filter(|o| o.map == map)
        .filter(|o| {
            !o.dead
                && matches!(&o.appearance, Appearance::Monster { name, .. } if name == "Chicken")
        })
        .min_by_key(|o| o.location.distance(loc))
        .map(|o| o.id)
        .expect("a chicken")
}

#[test]
fn wizard_learns_fire_ball_and_burns_a_chicken() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Wiz");
    rec.class = mir_proto::Class::Wizard;
    rec.level = 16;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let fire_ball = learn(&mut world, me, "Fire Ball");
    let victim = nearest_chicken(&world, me);
    let loc = world.objects[&me].location;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 2))
        .find(|p| {
            world.maps[&world.objects[&me].map]
                .file
                .is_walkable(p.x, p.y)
        })
        .unwrap();
    world.teleport(victim, cell);
    let hp_before = world.objects[&victim].hp;
    let mut now = 1000;
    let mut hit = false;
    for _ in 0..40 {
        world.cast(
            me,
            fire_ball,
            Direction::from_points(loc, cell),
            Some(victim),
            cell,
        );
        for _ in 0..30 {
            now += 100;
            world.tick(now);
        }
        let msgs = drain(&mut world);
        if msgs
            .iter()
            .any(|m| matches!(m, ServerMessage::ObjectMagic { magic, .. } if *magic == fire_ball))
        {
            hit = true;
        }
        if world
            .objects
            .get(&victim)
            .map(|v| v.dead || v.hp < hp_before)
            .unwrap_or(true)
        {
            break;
        }
    }
    assert!(hit, "ObjectMagic should be broadcast");
    let dead_or_hurt = world
        .objects
        .get(&victim)
        .map(|v| v.dead || v.hp < hp_before)
        .unwrap_or(true);
    assert!(dead_or_hurt, "fire ball should damage the chicken");
    let mp = world.objects[&me].player().unwrap().mp;
    assert!(
        mp < world.objects[&me].player().unwrap().max_mp,
        "casting costs mana"
    );
    let msgs = world.test_magic_exp(me, fire_ball);
    assert!(msgs > 0, "fire ball should gain experience");
}

#[test]
fn wizard_fire_wall_burns_a_chicken_standing_in_it() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Pyro");
    rec.class = mir_proto::Class::Wizard;
    rec.level = 40;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let fire_wall = learn(&mut world, me, "Fire Wall");
    let victim = nearest_chicken(&world, me);
    let loc = world.objects[&me].location;
    let map = world.objects[&me].map;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 3))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.teleport(victim, cell);
    let hp_before = world.objects[&victim].hp;
    world.cast(me, fire_wall, Direction::from_points(loc, cell), None, cell);
    let mut now = 1000;
    // Six seconds: three wall ticks, enough for small rolls to add up.
    for _ in 0..60 {
        now += 100;
        // Keep the chicken on the burning cell; it roams otherwise.
        if world.objects.get(&victim).map(|v| !v.dead).unwrap_or(false) {
            world.teleport(victim, cell);
        }
        world.tick(now);
    }
    let walls = world
        .objects
        .values()
        .filter(|o| matches!(o.appearance, Appearance::Spell { .. }) && o.map == map)
        .count();
    assert!(walls >= 1, "fire wall spell objects should exist");
    let hurt = world
        .objects
        .get(&victim)
        .map(|v| v.dead || v.hp < hp_before)
        .unwrap_or(true);
    let (hp_now, dead_now) = world
        .objects
        .get(&victim)
        .map(|v| (v.hp, v.dead))
        .unwrap_or((0, true));
    assert!(
        hurt,
        "a chicken standing in the fire wall burns: hp {hp_before} -> {hp_now}, dead {dead_now}, walls {walls}, chicken at {:?}, wall cell {:?}",
        world.objects.get(&victim).map(|v| v.location),
        cell
    );
    // Walls burn out: level 0 => 10 ticks of 2 s.
    for _ in 0..220 {
        now += 100;
        world.tick(now);
    }
    let walls = world
        .objects
        .values()
        .filter(|o| matches!(o.appearance, Appearance::Spell { .. }))
        .count();
    assert_eq!(walls, 0, "fire walls expire");
}

#[test]
fn warrior_might_raises_damage_then_expires() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Brute");
    rec.level = 48;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let might = learn(&mut world, me, "Might");
    let before = world.objects[&me].stats.max_dc;
    let loc = world.objects[&me].location;
    world.cast(me, might, Direction::Down, None, loc);
    let mut now = 1000;
    for _ in 0..12 {
        now += 100;
        world.tick(now);
    }
    let during = world.objects[&me].stats.max_dc;
    // +5 % of a small DC rounds to 0, so only check it never drops.
    assert!(
        during >= before,
        "Might must not lower max DC: {before} -> {during}"
    );
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::BuffAdd(b) if b.kind == mir_proto::buff_type::MIGHT)));
    // Level 0 lasts 60 s.
    now += 61_000;
    world.tick(now);
    assert_eq!(
        world.objects[&me].stats.max_dc, before,
        "buff should expire"
    );
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(
        |m| matches!(m, ServerMessage::BuffRemove { kind } if *kind == mir_proto::buff_type::MIGHT)
    ));
}

#[test]
fn warrior_shoulder_dash_moves_forward() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Dasher");
    rec.level = 27;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let dash = learn(&mut world, me, "Shoulder Dash");
    let loc = world.objects[&me].location;
    let map = world.objects[&me].map;
    let dir = Direction::ALL
        .iter()
        .copied()
        .find(|d| {
            (1..=3).all(|i| {
                let p = loc.step(*d, i);
                world.maps[&map].file.is_walkable(p.x, p.y)
                    && world.maps[&map].objects_at(p).is_empty()
            })
        })
        .expect("a clear direction");
    world.cast(me, dash, dir, None, loc);
    let mut now = 1000;
    let mut steps = 0;
    for _ in 0..30 {
        now += 100;
        world.tick(now);
        steps += drain(&mut world)
            .iter()
            .filter(|m| matches!(m, ServerMessage::ObjectDash { id, .. } if *id == me))
            .count();
    }
    let after = world.objects[&me].location;
    assert!(steps >= 1, "dash steps should be broadcast");
    assert!(
        after != loc && loc.distance(after) >= 1,
        "player should have moved"
    );
    assert_eq!(Direction::from_points(loc, after), dir);
}

#[test]
fn taoist_explosive_talisman_consumes_an_amulet() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Tao");
    rec.class = mir_proto::Class::Taoist;
    rec.level = 13;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let talisman = learn(&mut world, me, "Explosive Talisman");
    let victim = nearest_chicken(&world, me);
    let loc = world.objects[&me].location;
    let map = world.objects[&me].map;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 2))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.teleport(victim, cell);
    // Without a talisman equipped nothing flies.
    world.cast(
        me,
        talisman,
        Direction::from_points(loc, cell),
        Some(victim),
        cell,
    );
    let msgs = drain(&mut world);
    let flew = msgs.iter().any(|m| matches!(m, ServerMessage::ObjectMagic { magic, targets, .. } if *magic == talisman && !targets.is_empty()));
    assert!(!flew, "no talisman: no target");
    // Equip a stack of talismans (ItemType Amulet, Shape 0).
    let amulet = world
        .data
        .items
        .values()
        .filter(|d| {
            d.item_type == mir_proto::item_type::AMULET && d.shape == 0 && d.required_amount <= 13
        })
        .map(|d| d.index)
        .min()
        .expect("a talisman item");
    world.test_give_item(me, amulet, 5);
    let slot = world.test_slot_of(me, amulet).unwrap();
    world.item_use(me, slot);
    let mut now = 3000;
    world.tick(now);
    drain(&mut world);
    world.cast(
        me,
        talisman,
        Direction::from_points(loc, cell),
        Some(victim),
        cell,
    );
    now += 50;
    world.tick(now);
    let msgs = drain(&mut world);
    let flew = msgs.iter().any(|m| matches!(m, ServerMessage::ObjectMagic { magic, targets, .. } if *magic == talisman && !targets.is_empty()));
    assert!(flew, "with a talisman the spell targets the chicken");
    let count = world.objects[&me].player().unwrap().bag.equipment[mir_proto::slot::AMULET]
        .as_ref()
        .map(|i| i.count)
        .unwrap_or(0);
    assert_eq!(count, 4, "one talisman is consumed per cast");
    let hp_before = world.objects[&victim].hp;
    for _ in 0..20 {
        now += 100;
        world.tick(now);
    }
    let hurt = world
        .objects
        .get(&victim)
        .map(|v| v.dead || v.hp < hp_before)
        .unwrap_or(true);
    assert!(hurt, "the talisman should hit");
}

#[test]
fn warrior_beckon_pulls_a_chicken_and_fetter_slows_it() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Puller");
    rec.level = 55;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let beckon = learn(&mut world, me, "Beckon");
    let fetter = learn(&mut world, me, "Fetter");
    let victim = nearest_chicken(&world, me);
    let loc = world.objects[&me].location;
    let map = world.objects[&me].map;
    let dir = Direction::ALL
        .iter()
        .copied()
        .find(|d| {
            (1..=3).all(|i| {
                let p = loc.step(*d, i);
                world.maps[&map].file.is_walkable(p.x, p.y)
                    && world.maps[&map].objects_at(p).is_empty()
            })
        })
        .expect("a clear direction");
    let far = loc.step(dir, 3);
    let mut now = 1000;
    let mut pulled = false;
    // Level 0 succeeds 3 times in 9; keep trying.
    for _ in 0..40 {
        world.teleport(victim, far);
        world.cast(me, beckon, dir, Some(victim), far);
        for _ in 0..12 {
            now += 100;
            world.tick(now);
            if world.objects[&victim].location == loc.step(dir, 1) {
                pulled = true;
                break;
            }
        }
        drain(&mut world);
        if pulled {
            break;
        }
        now += 3000;
        world.tick(now);
    }
    assert!(
        pulled,
        "Beckon should pull the chicken in front of the warrior"
    );
    assert!(
        world.objects[&victim]
            .poisons
            .iter()
            .any(|p| p.kind == world::poison_kind::PARALYSIS),
        "pulled monsters are paralysed"
    );
    now += 3000;
    world.tick(now);
    // The chicken may have wandered off meanwhile; put it back in reach, and
    // refill the mana the Beckon retries used up.
    world.teleport(victim, loc.step(dir, 1));
    world.test_refill_mp(me);
    world.cast(me, fetter, dir, None, loc);
    for _ in 0..8 {
        now += 100;
        world.tick(now);
    }
    assert!(
        world.objects[&victim]
            .poisons
            .iter()
            .any(|p| p.kind == world::poison_kind::SLOW),
        "Fetter slows monsters within two cells"
    );
}

#[test]
fn wizard_storms_renounce_and_tempest() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Storm");
    rec.class = mir_proto::Class::Wizard;
    rec.level = 54;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let fire_storm = learn(&mut world, me, "Fire Storm");
    let renounce = learn(&mut world, me, "Renounce");
    let tempest = learn(&mut world, me, "Tempest");
    let victim = nearest_chicken(&world, me);
    let loc = world.objects[&me].location;
    let map = world.objects[&me].map;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 3))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.teleport(victim, cell);
    let hp_before = world.objects[&victim].hp;
    world.cast(
        me,
        fire_storm,
        Direction::from_points(loc, cell),
        None,
        cell,
    );
    let mut now = 1000;
    for _ in 0..10 {
        now += 100;
        world.teleport(victim, cell);
        world.tick(now);
    }
    let hurt = world
        .objects
        .get(&victim)
        .map(|v| v.dead || v.hp < hp_before)
        .unwrap_or(true);
    assert!(hurt, "Fire Storm hits the 3x3 around the cell");

    // Renounce trades max HP for MC.
    let (hp_max_before, mc_before) = {
        let o = &world.objects[&me];
        (o.max_hp, o.stats.max_mc)
    };
    now += 3000;
    world.tick(now);
    world.cast(me, renounce, Direction::Down, None, loc);
    for _ in 0..8 {
        now += 100;
        world.tick(now);
    }
    let o = &world.objects[&me];
    assert!(
        o.max_hp < hp_max_before,
        "Renounce lowers max HP: {hp_max_before} -> {}",
        o.max_hp
    );
    assert!(o.stats.max_mc >= mc_before, "Renounce raises MC");

    // Tempest lays a 3x3 field of spell objects.
    now += 3000;
    world.tick(now);
    world.cast(me, tempest, Direction::from_points(loc, cell), None, cell);
    for _ in 0..8 {
        now += 100;
        world.tick(now);
    }
    let fields = world
        .objects
        .values()
        .filter(|o| matches!(o.appearance, Appearance::Spell { effect } if effect == mir_proto::spell_effect::TEMPEST))
        .count();
    assert!(fields >= 5, "tempest cells: {fields}");
}

#[test]
fn taoist_summons_a_skeleton_that_follows_and_fights() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Necro");
    rec.class = mir_proto::Class::Taoist;
    rec.level = 17;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let summon = learn(&mut world, me, "Summon Skeleton");
    let amulet = world
        .data
        .items
        .values()
        .filter(|d| {
            d.item_type == mir_proto::item_type::AMULET && d.shape == 0 && d.required_amount <= 17
        })
        .map(|d| d.index)
        .min()
        .expect("a talisman item");
    world.test_give_item(me, amulet, 5);
    let slot = world.test_slot_of(me, amulet).unwrap();
    world.item_use(me, slot);
    let mut now = 3000;
    world.tick(now);
    drain(&mut world);
    let loc = world.objects[&me].location;
    world.cast(me, summon, Direction::Down, None, loc);
    for _ in 0..8 {
        now += 100;
        world.tick(now);
    }
    let pet = world
        .objects
        .values()
        .find(|o| matches!(&o.kind, world::Kind::Monster(m) if m.owner == Some(me)))
        .map(|o| o.id)
        .expect("a summoned skeleton");
    assert!(
        matches!(&world.objects[&pet].appearance, Appearance::Monster { owner: Some(n), .. } if n == "Necro")
    );
    // Attack a chicken: the idle pet takes it as its target.
    let victim = nearest_chicken(&world, me);
    let map = world.objects[&me].map;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 1))
        .find(|p| {
            world.maps[&map].file.is_walkable(p.x, p.y)
                && world.maps[&map].objects_at(*p).is_empty()
        })
        .unwrap();
    world.teleport(victim, cell);
    let dir = Direction::from_points(loc, cell);
    for _ in 0..30 {
        now += 500;
        world.player_attack(me, dir, None);
        world.tick(now);
        let pet_target = world.objects.get(&pet).and_then(|o| match &o.kind {
            world::Kind::Monster(m) => m.target,
            _ => None,
        });
        if pet_target == Some(victim) {
            break;
        }
    }
    let pet_target = world.objects.get(&pet).and_then(|o| match &o.kind {
        world::Kind::Monster(m) => m.target,
        _ => None,
    });
    assert_eq!(pet_target, Some(victim), "the pet should join the fight");
    // A second cast recalls instead of summoning another.
    now += 3000;
    world.tick(now);
    world.cast(me, summon, Direction::Down, None, loc);
    for _ in 0..8 {
        now += 100;
        world.tick(now);
    }
    let pets = world
        .objects
        .values()
        .filter(|o| matches!(&o.kind, world::Kind::Monster(m) if m.owner == Some(me)))
        .count();
    assert_eq!(pets, 1, "same summon is recalled, not doubled");
}

#[test]
fn taoist_celestial_light_and_shinsu_use_talisman_charges() {
    use mir_proto::{buff_type, slot};
    let Some(mut world) = world() else { return };
    let mut rec = test_character("Tao");
    rec.class = mir_proto::Class::Taoist;
    rec.level = 100;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let light = learn(&mut world, me, "Celestial Light");
    let summon = learn(&mut world, me, "Summon Shinsu");
    // Lazero's equipped Talisman Of Darkness is also shape 0 and valid.
    let amulet = 221;
    assert_eq!(world.data.items[&amulet].shape, 0);
    world.test_give_item(me, amulet, 25);
    let slot = world.test_slot_of(me, amulet).unwrap();
    world.item_use(me, slot);
    world.tick(3000);
    let loc = world.objects[&me].location;
    world.cast(me, light, Direction::Down, None, loc);
    world.tick(4600);
    assert!(world.objects[&me].has_buff(buff_type::CELESTIAL_LIGHT));
    let charges = world.objects[&me].player().unwrap().bag.equipment[slot::AMULET]
        .as_ref()
        .unwrap()
        .count;
    assert_eq!(charges, 5);

    world.tick(10_000);
    world.test_refill_mp(me);
    world.cast(me, summon, Direction::Down, None, loc);
    world.tick(10_600);
    let pet = world
        .objects
        .values()
        .find(|o| matches!(&o.kind, world::Kind::Monster(m) if m.owner == Some(me)));
    assert!(pet.is_some(), "Shinsu should spawn from five charges");
    let pet_id = pet.unwrap().id;
    drain(&mut world);
    world.tick(11_800);
    assert!(
        drain(&mut world)
            .iter()
            .any(|m| { matches!(m, ServerMessage::ObjectShow(state) if state.id == pet_id) }),
        "summoned Shinsu should appear to its owner"
    );
    assert!(world.objects[&me].player().unwrap().bag.equipment[slot::AMULET].is_none());
}

#[test]
fn taoist_summon_explains_missing_talisman_charges() {
    let Some(mut world) = world() else { return };
    let mut rec = test_character("Tao");
    rec.class = mir_proto::Class::Taoist;
    rec.level = 100;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let summon = learn(&mut world, me, "Summon Shinsu");
    world.tick(3000);
    drain(&mut world);
    let loc = world.objects[&me].location;
    world.cast(me, summon, Direction::Down, None, loc);
    assert!(drain(&mut world).iter().any(|m| {
        matches!(m, ServerMessage::Chat { text } if text.contains("needs 5 Talisman charges") && text.contains("Amulet slot"))
    }));
}

#[test]
fn taoist_summon_families_follow_and_engage_only_after_owner_fights() {
    let Some(mut world) = world() else { return };
    let mut rec = test_character("Tao");
    rec.class = mir_proto::Class::Taoist;
    rec.level = 100;
    let owner = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    let map = world.objects[&owner].map;
    let start = world
        .test_quiet_cell(map, world.objects[&owner].location)
        .unwrap();
    world.test_clear_monsters(map);
    world.teleport(owner, start);
    let dir = Direction::ALL
        .iter()
        .copied()
        .find(|d| {
            (1..=5).all(|n| {
                let p = start.step(*d, n);
                world.maps[&map].file.is_walkable(p.x, p.y)
                    && world.maps[&map].objects_at(p).is_empty()
            })
        })
        .expect("an open path for owner and pet");
    let chicken_def = world
        .data
        .monsters
        .values()
        .find(|d| d.name == "Chicken")
        .unwrap()
        .index;

    for flag in [1, 2, 3, 4, 9] {
        world.set_pet_mode(owner, mir_proto::pet_mode::BOTH);
        let def = world
            .data
            .monsters
            .values()
            .find(|d| d.flag == flag)
            .unwrap()
            .index;
        let pet = world.test_spawn_monster(def, map, start.step(dir, 1), Some(owner));
        let chicken = world.test_spawn_monster(chicken_def, map, start.step(dir, 2), None);
        world.test_set_hp(chicken, 10_000);
        let mut now = world.now;
        for _ in 0..12 {
            now += 500;
            world.tick(now);
        }
        assert_eq!(
            world.test_monster_target(pet),
            None,
            "flag {flag} picked a fight"
        );

        world.teleport(owner, start.step(dir, 5));
        let before = world.objects[&pet]
            .location
            .distance(world.objects[&owner].location);
        for _ in 0..12 {
            now += 500;
            world.tick(now);
        }
        let after = world.objects[&pet]
            .location
            .distance(world.objects[&owner].location);
        assert!(
            after < before,
            "flag {flag} did not follow: {before} -> {after}"
        );
        assert_eq!(
            world.test_monster_target(pet),
            None,
            "flag {flag} picked a fight while following"
        );

        world.test_damage(chicken, owner, 1);
        assert_eq!(
            world.test_monster_target(pet),
            Some(chicken),
            "flag {flag} ignored the owner's attack"
        );
        world
            .objects
            .get_mut(&pet)
            .unwrap()
            .monster_mut()
            .unwrap()
            .target = None;
        world.test_damage(owner, chicken, 1);
        assert_eq!(
            world.test_monster_target(pet),
            Some(chicken),
            "flag {flag} ignored an attack on its owner"
        );

        world
            .objects
            .get_mut(&pet)
            .unwrap()
            .monster_mut()
            .unwrap()
            .target = None;
        world.set_pet_mode(owner, mir_proto::pet_mode::ATTACK);
        let held = world.objects[&pet].location;
        world.teleport(owner, start);
        now += 2500;
        world.tick(now);
        assert_eq!(
            world.objects[&pet].location, held,
            "flag {flag} moved in Attack mode"
        );
        world.test_damage(chicken, owner, 1);
        assert_eq!(
            world.test_monster_target(pet),
            Some(chicken),
            "flag {flag} ignored Attack mode"
        );

        for mode in [
            mir_proto::pet_mode::MOVE,
            mir_proto::pet_mode::PVP,
            mir_proto::pet_mode::NONE,
        ] {
            world.set_pet_mode(owner, mode);
            world
                .objects
                .get_mut(&pet)
                .unwrap()
                .monster_mut()
                .unwrap()
                .target = None;
            world.test_damage(chicken, owner, 1);
            assert_eq!(
                world.test_monster_target(pet),
                None,
                "flag {flag} attacked a monster in mode {mode}"
            );
        }
        world.set_pet_mode(owner, mir_proto::pet_mode::MOVE);
        let before = world.objects[&pet].location.distance(start);
        for _ in 0..8 {
            now += 500;
            world.tick(now);
        }
        assert!(
            world.objects[&pet].location.distance(start) < before,
            "flag {flag} did not follow in Move mode"
        );

        world.set_pet_mode(owner, mir_proto::pet_mode::BOTH);
        world.test_damage(chicken, owner, 1);
        assert_eq!(world.test_monster_target(pet), Some(chicken));
        world.remove_object(chicken);
        now += 500;
        world.tick(now);
        assert_eq!(
            world.test_monster_target(pet),
            None,
            "flag {flag} kept a lost target"
        );
        if flag == 3 {
            let enemy = world.add_player(2, 2, &test_character("Enemy")).unwrap();
            world.teleport(enemy, start.step(dir, 2));
            world.set_attack_mode(owner, mir_proto::attack_mode::ALL);
            world.set_pet_mode(owner, mir_proto::pet_mode::PVP);
            world.test_damage(owner, enemy, 1);
            assert_eq!(
                world.test_monster_target(pet),
                Some(enemy),
                "PvP Shinsu ignored an attack on its owner"
            );
            world.remove_object(enemy);
            let next_map = world.data.map_by_file("0").unwrap().index;
            assert_ne!(next_map, map, "transition test needs a different map");
            let (next, _) = world.test_wall_spot(next_map).unwrap();
            world.test_clear_monsters(next_map);
            world.set_pet_mode(owner, mir_proto::pet_mode::MOVE);
            world.test_change_map(owner, next_map, next);
            now += 500;
            world.tick(now);
            assert!(
                world.objects.contains_key(&pet),
                "Shinsu was removed during map transition"
            );
            assert_eq!(
                world.objects[&pet].map, next_map,
                "Shinsu did not follow across maps"
            );
            world.test_change_map(owner, map, start);
        }
        world.remove_object(pet);
        world.teleport(owner, start);
    }
}

#[test]
fn taoist_shinsu_summons_on_d504_with_blocked_behind_cell() {
    use mir_proto::slot;
    let Some(mut world) = world() else { return };
    let mut rec = test_character("Tao");
    rec.class = mir_proto::Class::Taoist;
    rec.level = 100;
    let owner = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    let summon = learn(&mut world, owner, "Summon Shinsu");
    world.test_give_item(owner, 221, 5);
    let bag_slot = world.test_slot_of(owner, 221).unwrap();
    world.item_use(owner, bag_slot);
    let map = world.data.map_by_file("D504").unwrap().index;
    let (loc, blocked) = world
        .test_wall_spot(map)
        .expect("D504 has a walkable wall edge");
    world.test_clear_monsters(map);
    world.test_change_map(owner, map, loc);
    world.tick(3000);
    drain(&mut world);
    let behind = loc.step(blocked, 1);
    assert!(!world.maps[&map].file.is_walkable(behind.x, behind.y));
    world.cast(owner, summon, blocked.rotate(4), None, loc);
    world.tick(3600);
    let pet = world.objects.values().find(|o| {
        matches!(&o.kind, world::Kind::Monster(m) if m.owner == Some(owner) && world.data.monsters[&m.def].flag == 3)
    }).expect("Shinsu spawns at the caster when the behind cell is blocked").id;
    assert_eq!(world.objects[&pet].map, map);
    let pet_loc = world.objects[&pet].location;
    assert_ne!(pet_loc, behind);
    assert_ne!(pet_loc, loc, "an adjacent free cell is available on D504");
    assert!(world.maps[&map].file.is_walkable(pet_loc.x, pet_loc.y));
    assert!(pet_loc.distance(loc) <= 2);
    drain(&mut world);
    world.tick(4800);
    assert!(
        drain(&mut world)
            .iter()
            .any(|m| { matches!(m, ServerMessage::ObjectShow(state) if state.id == pet) }),
        "the D504 Shinsu becomes visible"
    );
    assert!(world.objects[&owner].player().unwrap().bag.equipment[slot::AMULET].is_none());
}

#[test]
fn pet_modes_gate_damage_and_pvp_safety() {
    use mir_proto::{attack_mode, pet_mode};

    let Some(mut world) = world() else {
        return;
    };
    let owner = world.add_player(1, 1, &test_character("PetOwner")).unwrap();
    let other = world
        .add_player(2, 2, &test_character("PetTarget"))
        .unwrap();
    world.tick(0);
    let map = world.objects[&owner].map;
    let chickens: Vec<_> = world
        .objects
        .values()
        .filter(|o| o.map == map && !o.dead)
        .filter(|o| matches!(&o.appearance, Appearance::Monster { name, .. } if name == "Chicken"))
        .map(|o| o.id)
        .take(2)
        .collect();
    assert_eq!(chickens.len(), 2);
    let (pet, wild) = (chickens[0], chickens[1]);
    world
        .objects
        .get_mut(&pet)
        .unwrap()
        .monster_mut()
        .unwrap()
        .owner = Some(owner);
    world
        .objects
        .get_mut(&pet)
        .unwrap()
        .monster_mut()
        .unwrap()
        .tame_until = u64::MAX;
    world
        .objects
        .get_mut(&owner)
        .unwrap()
        .player_mut()
        .unwrap()
        .pets
        .push(pet);
    world.test_set_hp(wild, 1000);

    for (mode, should_hurt) in [
        (pet_mode::BOTH, true),
        (pet_mode::MOVE, false),
        (pet_mode::ATTACK, true),
        (pet_mode::PVP, false),
        (pet_mode::NONE, false),
    ] {
        world
            .objects
            .get_mut(&owner)
            .unwrap()
            .player_mut()
            .unwrap()
            .pet_mode = mode;
        assert_eq!(
            world.test_damage(wild, pet, 5) > 0,
            should_hurt,
            "mode {mode}"
        );
    }

    let start = world.objects[&owner].location;
    world.teleport(pet, start);
    world.set_pet_mode(owner, pet_mode::PVP);
    let mut saved = test_character("PetOwner");
    world.snapshot(owner, &mut saved);
    assert_eq!(saved.pet_mode, pet_mode::PVP);
    world.set_attack_mode(owner, attack_mode::ALL);
    assert_eq!(world.test_damage(other, pet, 5), 0, "pet in safe zone");
    let outside = world.test_cell_outside_safe_zone(map, start).unwrap();
    world.teleport(pet, outside);
    world.teleport(other, outside);
    world.test_set_hp(other, 1000);
    world.set_attack_mode(owner, attack_mode::PEACE);
    assert_eq!(world.test_damage(other, pet, 5), 0, "owner at peace");
    world.set_attack_mode(owner, attack_mode::ALL);
    for (mode, should_hurt) in [
        (pet_mode::BOTH, true),
        (pet_mode::MOVE, false),
        (pet_mode::ATTACK, true),
        (pet_mode::PVP, true),
        (pet_mode::NONE, false),
    ] {
        world
            .objects
            .get_mut(&owner)
            .unwrap()
            .player_mut()
            .unwrap()
            .pet_mode = mode;
        assert_eq!(
            world.test_damage(other, pet, 5) > 0,
            should_hurt,
            "mode {mode} player damage"
        );
    }

    // Both and Attack can fight an enemy pet under the same owner rules.
    world.teleport(wild, outside);
    world
        .objects
        .get_mut(&wild)
        .unwrap()
        .monster_mut()
        .unwrap()
        .owner = Some(other);
    for mode in [pet_mode::BOTH, pet_mode::ATTACK] {
        world
            .objects
            .get_mut(&owner)
            .unwrap()
            .player_mut()
            .unwrap()
            .pet_mode = mode;
        assert!(world.test_damage(wild, pet, 5) > 0, "mode {mode} enemy pet");
    }
    world
        .objects
        .get_mut(&owner)
        .unwrap()
        .player_mut()
        .unwrap()
        .pet_mode = pet_mode::PVP;
    assert_eq!(world.test_damage(wild, pet, 5), 0, "PvP ignores pets");

    // Recall follows the same movement permission as ordinary pet steps.
    for mode in [pet_mode::ATTACK, pet_mode::NONE] {
        world
            .objects
            .get_mut(&owner)
            .unwrap()
            .player_mut()
            .unwrap()
            .pet_mode = mode;
        world.objects.get_mut(&owner).unwrap().visible.remove(&pet);
        world.tick(world.now + 1000);
        assert_eq!(world.objects[&pet].location, outside, "mode {mode} moved");
    }
    for mode in [pet_mode::MOVE, pet_mode::PVP, pet_mode::BOTH] {
        world.teleport(pet, outside);
        world
            .objects
            .get_mut(&owner)
            .unwrap()
            .player_mut()
            .unwrap()
            .pet_mode = mode;
        world.objects.get_mut(&owner).unwrap().visible.remove(&pet);
        world.tick(world.now + 1000);
        assert!(
            world.objects[&pet].location.distance(start) <= 1,
            "mode {mode} did not recall"
        );
    }
}

#[test]
fn taoist_traps_and_buffs() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Tao2");
    rec.class = mir_proto::Class::Taoist;
    rec.level = 34;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let trap = learn(&mut world, me, "Trap Octagon");
    let blood = learn(&mut world, me, "Blood Lust");
    let amulet = world
        .data
        .items
        .values()
        .filter(|d| {
            d.item_type == mir_proto::item_type::AMULET && d.shape == 0 && d.required_amount <= 34
        })
        .map(|d| d.index)
        .min()
        .expect("a talisman item");
    world.test_give_item(me, amulet, 20);
    let slot = world.test_slot_of(me, amulet).unwrap();
    world.item_use(me, slot);
    let mut now = 3000;
    world.tick(now);
    drain(&mut world);
    let victim = nearest_chicken(&world, me);
    let loc = world.objects[&me].location;
    let map = world.objects[&me].map;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 3))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.teleport(victim, cell);
    world.cast(me, trap, Direction::from_points(loc, cell), None, cell);
    for _ in 0..10 {
        now += 100;
        world.teleport(victim, cell);
        world.tick(now);
    }
    let shocked =
        matches!(&world.objects[&victim].kind, world::Kind::Monster(m) if m.shock_until > now);
    assert!(shocked, "trapped chicken cannot move");
    let ring = world
        .objects
        .values()
        .filter(|o| matches!(o.appearance, Appearance::Spell { effect } if effect == mir_proto::spell_effect::TRAP_OCTAGON))
        .count();
    assert!(ring >= 4, "octagon cells: {ring}");
    // Blood Lust raises max DC of players in the area.
    let before = world.objects[&me].stats.max_dc;
    now += 3000;
    world.tick(now);
    world.cast(me, blood, Direction::Down, None, loc);
    for _ in 0..12 {
        now += 100;
        world.tick(now);
    }
    assert!(
        world.objects[&me].stats.max_dc > before,
        "Blood Lust adds DC"
    );
}

#[test]
fn assassin_cloak_grip_hell_fire_and_puppets() {
    let Some(mut world) = world() else {
        return;
    };
    // Assassins start on their own map; put this one at the warrior start
    // where the chickens are.
    let scout = world.add_player(1, 1, &test_character("Scout")).unwrap();
    let (scout_map, scout_loc) = (world.objects[&scout].map, world.objects[&scout].location);
    world.remove_object(scout);
    let mut rec = test_character("Sin3");
    rec.class = mir_proto::Class::Assassin;
    rec.level = 50;
    rec.map = world.data.maps[&scout_map].file_name.clone();
    rec.location = scout_loc;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let cloak = learn(&mut world, me, "Cloak");
    let grip = learn(&mut world, me, "Wraith Grip");
    let hell = learn(&mut world, me, "Hell Fire");
    let puppet = learn(&mut world, me, "Summon Puppet");
    let mut now = 3000;
    world.tick(now);
    drain(&mut world);
    let victim = nearest_chicken(&world, me);
    let loc = world.objects[&me].location;
    let map = world.objects[&me].map;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 2))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.teleport(victim, cell);
    // Wraith Grip pins the chicken with its own poison kind.
    world.cast(
        me,
        grip,
        Direction::from_points(loc, cell),
        Some(victim),
        cell,
    );
    for _ in 0..8 {
        now += 100;
        world.tick(now);
    }
    let gripped = world.objects[&victim]
        .poisons
        .iter()
        .any(|p| p.kind == world::poison_kind::WRAITH_GRIP);
    assert!(gripped, "Wraith Grip poison applied");
    // Hell Fire burns and leaves a ticking poison.
    now += 2000;
    world.tick(now);
    let (hp_before, _, _) = world.test_hp(victim);
    world.cast(
        me,
        hell,
        Direction::from_points(loc, cell),
        Some(victim),
        cell,
    );
    for _ in 0..15 {
        now += 100;
        world.tick(now);
    }
    let (hp_after, _, dead) = world.test_hp(victim);
    let burning = world.objects[&victim]
        .poisons
        .iter()
        .any(|p| p.kind == world::poison_kind::HELL_FIRE);
    assert!(
        dead || hp_after < hp_before || burning,
        "Hell Fire hurts: {hp_before} -> {hp_after}, burning {burning}"
    );
    // Cloak costs HP and keeps draining it every 2 s.
    now += 2000;
    world.tick(now);
    let (hp0, _, _) = world.test_hp(me);
    world.cast(me, cloak, Direction::Down, None, loc);
    for _ in 0..8 {
        now += 100;
        world.tick(now);
    }
    assert!(
        world.objects[&me].has_buff(mir_proto::buff_type::CLOAK),
        "cloaked"
    );
    let (hp1, _, _) = world.test_hp(me);
    assert!(hp1 < hp0, "cloak costs HP: {hp0} -> {hp1}");
    now += 2500;
    world.tick(now);
    let (hp2, _, _) = world.test_hp(me);
    assert!(hp2 < hp1, "cloak drains HP: {hp1} -> {hp2}");
    // Summon Puppet drops the cloak, raises a puppet, blinks and re-cloaks.
    world.cast(me, puppet, Direction::Down, None, loc);
    for _ in 0..8 {
        now += 100;
        world.tick(now);
    }
    let puppets = world
        .objects
        .values()
        .filter(|o| matches!(&o.kind, world::Kind::Monster(m) if m.owner == Some(me) && m.explode_at.is_some()))
        .count();
    assert_eq!(puppets, 1, "one puppet at level 0");
    assert!(
        world.objects[&me].has_buff(mir_proto::buff_type::GHOST_WALK),
        "puppet cloak always ghost walks"
    );
    for _ in 0..60 {
        now += 100;
        world.tick(now);
    }
    let puppets = world
        .objects
        .values()
        .filter(|o| matches!(&o.kind, world::Kind::Monster(m) if m.owner == Some(me) && !o.dead))
        .count();
    assert_eq!(puppets, 0, "puppets explode after 5 s");
}

#[test]
fn warrior_thrusting_reaches_the_second_cell() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Thruster");
    rec.level = 19;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let thrusting = learn(&mut world, me, "Thrusting");
    world.magic_toggle(me, thrusting, true);
    let victim = nearest_chicken(&world, me);
    let map = world.objects[&me].map;
    let loc = world.objects[&me].location;
    let dir = Direction::ALL
        .iter()
        .copied()
        .find(|d| {
            let a = loc.step(*d, 1);
            let b = loc.step(*d, 2);
            world.maps[&map].file.is_walkable(a.x, a.y)
                && world.maps[&map].file.is_walkable(b.x, b.y)
        })
        .unwrap();
    world.teleport(victim, loc.step(dir, 2));
    let hp_before = world.objects[&victim].hp;
    let mut now = 1000;
    for _ in 0..200 {
        // Keep the chicken two cells ahead; it wanders otherwise.
        if world.objects.get(&victim).map(|v| !v.dead).unwrap_or(false) {
            world.teleport(victim, loc.step(dir, 2));
        }
        world.player_attack(me, dir, Some(thrusting));
        now += 1600;
        world.tick(now);
        if world
            .objects
            .get(&victim)
            .map(|v| v.dead || v.hp < hp_before)
            .unwrap_or(true)
        {
            break;
        }
    }
    assert!(
        world
            .objects
            .get(&victim)
            .map(|v| v.dead || v.hp < hp_before)
            .unwrap_or(true),
        "thrusting should hit two cells ahead"
    );
}

#[test]
fn passive_animals_do_not_aggro_but_hunters_do() {
    let Some(mut world) = world() else {
        return;
    };
    let _me = world.add_player(1, 1, &test_character("Tester")).unwrap();
    world.tick(0);
    let passive: Vec<ObjectId> = world
        .objects
        .values()
        .filter(|o| match &o.appearance {
            Appearance::Monster { name, .. } => name == "Chicken" || name == "Deer",
            _ => false,
        })
        .map(|o| o.id)
        .collect();
    assert!(!passive.is_empty());
    for id in &passive {
        assert!(world.data.monsters[&world.objects[id].monster_def()].is_passive());
    }
    assert!(!world
        .data
        .monsters
        .values()
        .find(|m| m.name == "Wolf")
        .unwrap()
        .is_passive());
}

#[test]
fn monsters_start_aggro_one_cell_inside_view_range() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("Hunter");
    record.gm = true;
    let me = world.add_player(1, 1, &record).unwrap();
    world.tick(0);
    let map = world.objects[&me].map;
    let loc = world.objects[&me].location;
    world.test_clear_monsters(map);
    let wolf = world
        .data
        .monsters
        .values()
        .find(|m| m.name == "Wolf")
        .unwrap();
    let (wolf_id, range) = (wolf.index, wolf.view_range);
    assert!(range > 1);
    let at_distance = |distance| {
        (0..world.maps[&map].file.height as i32)
            .flat_map(|y| (0..world.maps[&map].file.width as i32).map(move |x| Point::new(x, y)))
            .find(|p| p.distance(loc) == distance && world.maps[&map].file.is_walkable(p.x, p.y))
            .unwrap()
    };
    let edge = at_distance(range);
    let inside = at_distance(range - 1);
    world.chat(me, format!("@monster {wolf_id}"));
    let mob = world
        .objects
        .values()
        .find(|o| o.is_monster() && o.monster_def() == wolf_id)
        .unwrap()
        .id;
    world.teleport(mob, edge);
    world.tick(3000);
    assert_eq!(world.test_monster_target(mob), None);
    world.teleport(mob, inside);
    world.tick(6000);
    assert_eq!(world.test_monster_target(mob), Some(me));
}

#[test]
fn magic_shield_cast_during_run_keeps_movement_timer() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("Runner");
    record.class = mir_proto::Class::Wizard;
    record.level = 50;
    let me = world.add_player(1, 1, &record).unwrap();
    world.tick(0);
    let magic = learn(&mut world, me, "Magic Shield");
    let map = world.objects[&me].map;
    let loc = world.objects[&me].location;
    let direction = Direction::ALL
        .iter()
        .copied()
        .find(|d| {
            (1..=2).all(|step| {
                let p = loc.step(*d, step);
                world.maps[&map].file.is_walkable(p.x, p.y)
                    && world.maps[&map].objects_at(p).is_empty()
            })
        })
        .unwrap();
    world.player_move(me, direction, true);
    let move_deadline = world.objects[&me].action_time;
    world.tick(100);
    let location = world.objects[&me].location;
    world.cast(me, magic, direction, None, location);
    assert_eq!(world.objects[&me].action_time, move_deadline);
    world.tick(1300);
    assert!(world.objects[&me].has_buff(mir_proto::buff_type::MAGIC_SHIELD));
}

#[test]
fn move_is_validated_against_walls_and_objects() {
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Tester")).unwrap();
    world.tick(0);
    drain(&mut world);
    let map = world.objects[&me].map;
    let loc = world.objects[&me].location;
    let blocked_dir = Direction::ALL.iter().copied().find(|d| {
        !world.maps[&map]
            .file
            .is_walkable(loc.step(*d, 1).x, loc.step(*d, 1).y)
    });
    if let Some(d) = blocked_dir {
        world.player_move(me, d, false);
        let msgs = drain(&mut world);
        assert!(matches!(
            msgs.last(),
            Some(ServerMessage::MoveDenied { .. })
        ));
        assert_eq!(world.objects[&me].location, loc);
    }
    let open_dir = Direction::ALL
        .iter()
        .copied()
        .find(|d| {
            world.maps[&map]
                .file
                .is_walkable(loc.step(*d, 1).x, loc.step(*d, 1).y)
        })
        .expect("open direction");
    // Clear any monster from that cell.
    let cell = loc.step(open_dir, 1);
    let blockers: Vec<ObjectId> = world
        .objects
        .values()
        .filter(|o| o.location == cell)
        .map(|o| o.id)
        .collect();
    for b in blockers {
        world.remove_object(b);
    }
    world.tick(1000);
    drain(&mut world);
    world.player_move(me, open_dir, false);
    assert_eq!(world.objects[&me].location, Point::new(cell.x, cell.y));
}

#[test]
fn guards_stand_in_town_and_archers_shoot_from_range() {
    let Some(mut world) = world() else {
        return;
    };
    let mut rec = test_character("Watch");
    rec.level = 40;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let map = world.objects[&me].map;
    assert!(
        world.test_guard_count(map) > 0,
        "GuardInfo places guards on the start map"
    );
    // A skeleton axe thrower (AI 7) six cells away shoots without closing in
    // (closer than that it would back off first).
    let loc = world.objects[&me].location;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 6))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    let archer = world.test_spawn_ai(7, map, cell).expect("an AI 7 monster");
    world.test_set_target(archer, me);
    let (hp0, _, _) = world.test_hp(me);
    let mut now = 3000;
    let mut shot = false;
    for _ in 0..80 {
        now += 100;
        world.teleport(archer, cell);
        world.teleport(me, loc);
        world.tick(now);
        if drain(&mut world)
            .iter()
            .any(|m| matches!(m, ServerMessage::ObjectRangeAttack { id, .. } if *id == archer))
        {
            shot = true;
        }
    }
    assert!(shot, "archer used a ranged attack");
    let (hp1, _, _) = world.test_hp(me);
    assert!(
        hp1 <= hp0,
        "ranged hit landed or was dodged: {hp0} -> {hp1}"
    );
}

#[test]
fn npc_data_lists_and_currencies_round_trip() {
    use crate::data::{NpcActionDef, NpcCheckDef};
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Scribe")).unwrap();
    world.tick(0);
    drain(&mut world);
    let action = |t: i32, s: &str, i1: i32, i2: i32| NpcActionDef {
        action_type: t,
        string1: s.into(),
        int1: i1,
        int2: i2,
        item1: 0,
        map1: 0,
        stat1: 0,
    };
    let check = |t: i32, op: i32, s: &str, i1: i32, i2: i32| NpcCheckDef {
        check_type: t,
        operator: op,
        string1: s.into(),
        int1: i1,
        int2: i2,
        item1: 0,
        stat1: 0,
        fail_page: 0,
    };
    // Data list membership (Zircon CheckDataList) after AddDataList / RemoveDataList.
    assert!(!world.test_npc_check(me, &check(19, 0, "Quest1", 1, 0)));
    world.test_npc_action(me, &action(17, "Quest1", 1, 0));
    assert!(world.test_npc_check(me, &check(19, 0, "Quest1", 1, 0)));
    world.test_npc_action(me, &action(18, "Quest1", 1, 0));
    assert!(!world.test_npc_check(me, &check(19, 0, "Quest1", 1, 0)));
    // Data values: set, change, compare with IntParameter2.
    world.test_npc_action(me, &action(21, "Kills", 1, 5));
    world.test_npc_action(me, &action(20, "Kills", 1, 2));
    assert!(world.test_npc_check(me, &check(20, 0, "Kills", 1, 7)));
    assert!(world.test_npc_check(me, &check(20, 5, "Kills", 1, 3)));
    // Currencies by name; gold routes to the bag.
    world.test_npc_action(me, &action(15, "Fame Point", 12, 0));
    assert!(world.test_npc_check(me, &check(17, 0, "FP", 12, 0)));
    world.test_npc_action(me, &action(16, "Fame Point", 2, 0));
    assert!(world.test_npc_check(me, &check(17, 0, "Fame Point", 10, 0)));
    let gold_before = world.test_gold(me);
    world.test_npc_action(me, &action(15, "Gold", 30, 0));
    assert_eq!(world.test_gold(me), gold_before + 30);
    // Unknown currency names pass the check like Zircon's `continue`.
    assert!(world.test_npc_check(me, &check(17, 0, "Moonstones", 1, 0)));
}

#[test]
fn quest_accept_progress_and_complete() {
    let Some(mut world) = world() else {
        return;
    };
    // "Curing the Poison Pt. 1": quest 9 at NPC 13, level 20, ten venoms from
    // monster 20 at a 1-in-2 roll, rewards item 801 x352.
    let Some(def) = world.data.quests.get(&9).cloned() else {
        return;
    };
    let mut rec = test_character("Seeker");
    rec.level = 20;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    let msgs = drain(&mut world);
    assert!(
        msgs.iter()
            .any(|m| matches!(m, ServerMessage::QuestList(_))),
        "quest log sent on entry"
    );
    let npc = world.test_open_npc(me, def.start_npc);
    world.tick(100);
    assert!(drain(&mut world).iter().any(|message| matches!(message,
        ServerMessage::NpcQuestMarker { id, marker: 1 } if *id == npc
    )));
    let offered = world.test_npc_quests(me, npc);
    assert!(
        offered.iter().any(|q| q.quest == 9 && q.state == 0),
        "quest offered: {offered:?}"
    );
    world.quest_accept(me, 9);
    assert!(world
        .test_quests(me)
        .iter()
        .any(|q| q.quest == 9 && !q.completed));
    let messages = drain(&mut world);
    assert!(messages.iter().any(|message| matches!(message,
        ServerMessage::QuestChanged(q) if q.quest == 9
    )));
    assert!(messages.iter().any(|message| matches!(message,
        ServerMessage::NpcQuestMarker { id, .. } if *id == npc
    )));
    // Not offered twice.
    assert!(!world
        .test_npc_quests(me, npc)
        .iter()
        .any(|q| q.quest == 9 && q.state == 0));
    // Kills credit the gather task (chance rolls; 200 kills is plenty).
    let task = def.tasks[0].clone();
    let monster = task.monsters[0].monster;
    let map = world.objects[&me].map;
    for _ in 0..200 {
        world.test_quest_kill(me, monster, map);
    }
    let q = world
        .test_quests(me)
        .into_iter()
        .find(|q| q.quest == 9)
        .unwrap();
    let amount = q
        .tasks
        .iter()
        .find(|(t, _)| *t == task.index)
        .map(|(_, a)| *a)
        .unwrap_or(0);
    assert_eq!(amount, task.amount, "task capped at the required amount");
    drain(&mut world);
    // Completing at the finish NPC grants the reward and marks it done.
    let finish = if def.finish_npc == def.start_npc {
        npc
    } else {
        world.test_open_npc(me, def.finish_npc)
    };
    assert!(world
        .test_npc_quests(me, finish)
        .iter()
        .any(|q| q.quest == 9 && q.state == 2));
    let gold_before = world.test_gold(me);
    let reward = def.rewards[0].clone();
    let reward_def = world.data.items[&reward.item].clone();
    let exp_before = world.test_experience(me);
    world.quest_complete(me, 9, 0);
    let q = world
        .test_quests(me)
        .into_iter()
        .find(|q| q.quest == 9)
        .unwrap();
    assert!(q.completed, "quest completed");
    if reward_def.effect == 2 {
        assert_eq!(world.test_experience(me), exp_before + reward.amount as u64);
    } else if reward.item == world.data.gold_item {
        assert_eq!(world.test_gold(me), gold_before + reward.amount as u64);
    } else if reward_def.item_type == 34 {
        assert!(
            world.test_currency_total(me) >= reward.amount as i64,
            "currency reward credited"
        );
    } else {
        assert!(
            world.test_slot_of(me, reward.item).is_some(),
            "reward item in the bag"
        );
    }
    // Requirement HaveNotCompleted now blocks a second run.
    assert!(!world.test_npc_quests(me, npc).iter().any(|q| q.quest == 9));
}

#[test]
fn wave_four_bursts_and_buffs() {
    let Some(mut world) = world() else {
        return;
    };
    // A high-level warrior at the start: Taecheon Sword burns everything
    // within two cells; Invincibility nulls incoming damage.
    let mut rec = test_character("Blade");
    rec.level = 70;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let has_book = |world: &World, name: &str| {
        world
            .data
            .magics
            .values()
            .find(|m| m.name == name)
            .map(|def| {
                world
                    .data
                    .items
                    .values()
                    .any(|i| i.item_type == 14 && i.shape == def.index)
            })
            .unwrap_or(false)
    };
    if !has_book(&world, "Taecheon Sword") || !has_book(&world, "Invincibility") {
        eprintln!("books missing; skipping");
        return;
    }
    let burst = learn(&mut world, me, "Taecheon Sword");
    let shield = learn(&mut world, me, "Invincibility");
    let mut now = 3000;
    world.tick(now);
    drain(&mut world);
    let victim = nearest_chicken(&world, me);
    let loc = world.objects[&me].location;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 1))
        .find(|p| {
            world.maps[&world.objects[&me].map]
                .file
                .is_walkable(p.x, p.y)
        })
        .unwrap();
    world.teleport(victim, cell);
    let (hp0, _, _) = world.test_hp(victim);
    world.cast(me, burst, Direction::Down, None, loc);
    for _ in 0..20 {
        now += 100;
        world.teleport(victim, cell);
        world.tick(now);
    }
    let (hp1, _, dead) = world.test_hp(victim);
    assert!(
        dead || hp1 < hp0,
        "Taecheon Sword hurt the chicken: {hp0} -> {hp1}"
    );
    // Invincibility: a direct hit does nothing while the buff lasts.
    world.cast(me, shield, Direction::Down, None, loc);
    for _ in 0..8 {
        now += 100;
        world.tick(now);
    }
    assert!(world.objects[&me].has_buff(mir_proto::buff_type::INVINCIBILITY));
    let (php0, _, _) = world.test_hp(me);
    world.test_damage(me, victim, 50);
    let (php1, _, _) = world.test_hp(me);
    assert_eq!(php0, php1, "invincible");
}

#[test]
fn wave_five_dance_of_swallow_and_thunder_kick() {
    let Some(mut world) = world() else {
        return;
    };
    let has_book = |world: &World, name: &str| {
        world
            .data
            .magics
            .values()
            .find(|m| m.name == name)
            .map(|def| {
                world
                    .data
                    .items
                    .values()
                    .any(|i| i.item_type == 14 && i.shape == def.index)
            })
            .unwrap_or(false)
    };
    // Assassin at the warrior start (chickens): Dance Of Swallow blinks next
    // to the target and strikes it.
    let scout = world.add_player(1, 1, &test_character("Scout2")).unwrap();
    let (scout_map, scout_loc) = (world.objects[&scout].map, world.objects[&scout].location);
    world.remove_object(scout);
    let mut rec = test_character("Swallow");
    rec.class = mir_proto::Class::Assassin;
    rec.level = 60;
    rec.map = world.data.maps[&scout_map].file_name.clone();
    rec.location = scout_loc;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    if !has_book(&world, "Dance Of Swallow") {
        eprintln!("book missing; skipping");
        return;
    }
    let dance = learn(&mut world, me, "Dance Of Swallow");
    let mut now = 3000;
    world.tick(now);
    drain(&mut world);
    let victim = nearest_chicken(&world, me);
    let loc = world.objects[&me].location;
    let map = world.objects[&me].map;
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 5))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.teleport(victim, cell);
    let (hp0, _, _) = world.test_hp(victim);
    world.cast(
        me,
        dance,
        Direction::from_points(loc, cell),
        Some(victim),
        cell,
    );
    for _ in 0..8 {
        now += 100;
        world.teleport(victim, cell);
        world.tick(now);
    }
    assert_eq!(
        world.objects[&me].location.distance(cell),
        1,
        "blinked next to the chicken"
    );
    let (hp1, _, dead) = world.test_hp(victim);
    assert!(dead || hp1 < hp0, "Dance Of Swallow hit: {hp0} -> {hp1}");
    // Thunder Kick (taoist) shoves the chicken in front of the caster.
    let mut rec = test_character("Kicker");
    rec.class = mir_proto::Class::Taoist;
    rec.level = 60;
    rec.map = world.data.maps[&scout_map].file_name.clone();
    rec.location = scout_loc;
    let tao = world.add_player(2, 2, &rec).unwrap();
    world.tick(now);
    drain(&mut world);
    if !has_book(&world, "Thunder Kick") {
        return;
    }
    let kick = learn(&mut world, tao, "Thunder Kick");
    let victim = nearest_chicken(&world, tao);
    let tloc = world.objects[&tao].location;
    let dir = Direction::ALL
        .iter()
        .copied()
        .find(|d| {
            (1..=4).all(|i| {
                let p = tloc.step(*d, i);
                world.maps[&map].file.is_walkable(p.x, p.y)
            })
        })
        .unwrap();
    let front = tloc.step(dir, 1);
    world.teleport(victim, front);
    let mut pushed = false;
    for _ in 0..30 {
        now += 3000;
        world.tick(now);
        world.teleport(victim, front);
        world.cast(tao, kick, dir, None, front);
        for _ in 0..8 {
            now += 100;
            world.tick(now);
        }
        if world.objects[&victim].location != front {
            pushed = true;
            break;
        }
        world.test_refill_mp(tao);
    }
    assert!(pushed, "Thunder Kick pushed the chicken");
}

#[test]
fn chat_routes_local_shout_and_whisper() {
    use mir_proto::ChatKind;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    drain(&mut world);
    let said = |world: &mut World| -> Vec<(world::ConnId, ChatKind, String)> {
        world
            .outgoing
            .drain(..)
            .filter_map(|o| match o {
                Outgoing::To(c, ServerMessage::Say { kind, text, .. }) => Some((c, kind, text)),
                _ => None,
            })
            .collect()
    };
    let conn_of = |world: &World, id: ObjectId| world.objects[&id].player().unwrap().conn;
    let (ca, cb) = (conn_of(&world, alice), conn_of(&world, bob));

    // Local talk reaches both (same start cell area) with a bubble id.
    world.chat(alice, "hello".into());
    let lines = said(&mut world);
    assert!(lines
        .iter()
        .any(|(c, k, t)| *c == cb && *k == ChatKind::Normal && t == "Alice: hello"));
    assert!(lines.iter().any(|(c, _, _)| *c == ca));

    // Shout needs level 2: a fresh character is level 1.
    world.chat(alice, "!hey".into());
    let lines = said(&mut world);
    assert!(lines
        .iter()
        .all(|(c, k, _)| *c == ca && *k == ChatKind::System));

    // Whisper by name, case-insensitive; unknown names fail politely.
    world.chat(bob, "/alice psst".into());
    let lines = said(&mut world);
    assert!(lines
        .iter()
        .any(|(c, k, t)| *c == ca && *k == ChatKind::WhisperIn && t == "Bob=> psst"));
    assert!(lines
        .iter()
        .any(|(c, k, _)| *c == cb && *k == ChatKind::WhisperOut));
    world.chat(bob, "/nobody hi".into());
    let lines = said(&mut world);
    assert!(lines
        .iter()
        .any(|(c, k, _)| *c == cb && *k == ChatKind::System));

    // Group chat without a group is silently dropped.
    world.chat(bob, "!!team".into());
    assert!(said(&mut world).is_empty());
}

#[test]
fn gm_commands_require_character_grant() {
    let Some(mut world) = world() else { return };
    let normal = world.add_player(1, 1, &test_character("Normal")).unwrap();
    let mut record = test_character("Master");
    record.gm = true;
    let gm = world.add_player(2, 2, &record).unwrap();
    world.tick(0);
    drain(&mut world);
    let monster = *world.data.monsters.keys().next().unwrap();
    let before = world.objects.len();
    world.chat(normal, format!("@spawn {monster}"));
    assert_eq!(world.objects.len(), before);
    assert!(world.outgoing.iter().any(|event| matches!(event,
        Outgoing::To(1, ServerMessage::Say { text, .. }) if text == "GM access required."
    )));
    world.chat(gm, format!("@spawn {monster}"));
    assert_eq!(world.objects.len(), before + 1);
    world.chat(gm, "@level 7".into());
    assert_eq!(world.objects[&gm].player().unwrap().level, 7);
    world.chat(gm, "@level 70".into());
    assert_eq!(world.objects[&gm].player().unwrap().level, 70);
    world.chat(gm, "@level 40".into());
    world.gain_experience_test(gm, GameData::max_experience(40));
    assert_eq!(world.objects[&gm].player().unwrap().level, 41);
    world.chat(gm, "@make 126".into());
    assert!(world.objects[&gm]
        .player()
        .unwrap()
        .bag
        .inventory
        .iter()
        .flatten()
        .any(|item| item.info == 126));
    let gold = world.objects[&gm].player().unwrap().bag.gold;
    world.chat(gm, "@make gold 100000".into());
    assert_eq!(world.objects[&gm].player().unwrap().bag.gold, gold + 100000);
    world.chat(gm, "@giveskills".into());
    assert!(!world.objects[&gm].player().unwrap().magics.is_empty());
    let dc = world.objects[&gm].stats.max_dc;
    world.chat(gm, "@sethermitstat MaxDC 5".into());
    assert_eq!(world.objects[&gm].stats.max_dc, dc + 5);
    assert!(world.objects[&gm].player().unwrap().bag.equipment[mir_proto::slot::WEAPON].is_some());
    world.chat(gm, "@addstat Weapon MaxDC 4".into());
    assert_eq!(world.objects[&gm].stats.max_dc, dc + 9);
    world.chat(gm, "@gamemaster".into());
    world.chat(gm, "@superman".into());
    world.chat(gm, "@observer".into());
    let p = world.objects[&gm].player().unwrap();
    assert!(p.gm_mode && p.superman && p.observer);
    assert!(!world.objects[&normal].visible.contains(&gm));
    world.chat(gm, "@gamemaster".into());
    world.chat(gm, "@observer".into());
    assert_eq!(world.test_damage(gm, normal, 100), 0);
    let mut saved = record;
    world.snapshot(gm, &mut saved);
    assert_eq!(saved.hermit_stats, vec![(9, 5)]);
    let changed_item = world
        .data
        .items
        .values()
        .find(|item| {
            item.item_type == 25
                || (item.required_type == 0 && (40..=56).contains(&item.required_amount))
        })
        .unwrap()
        .index;
    world.market_store.listings.push(world::market::Listing {
        index: 1,
        account: 1,
        seller: "Normal".into(),
        item: crate::items::UserItem::fresh(&world.data, changed_item, 1).unwrap(),
        price: 1,
        message: String::new(),
        created: 0,
    });
    world.chat(gm, "@reboot".into());
    assert!(world.market_store.listings.is_empty());
    assert_eq!(world.mail_store.boxes[&1][0].items[0].info, changed_item);
}

#[test]
fn gm_makespecial_guarantees_equipment_bonuses_without_drop_toggle() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("SpecialMaker");
    record.gm = true;
    let gm = world.add_player(1, 1, &record).unwrap();
    assert!(!world.drop_specials);
    world.chat(gm, "@make 126".into());
    world.chat(gm, "@makespecial 126 2".into());
    let swords: Vec<_> = world.objects[&gm]
        .player()
        .unwrap()
        .bag
        .inventory
        .iter()
        .flatten()
        .filter(|item| item.info == 126)
        .collect();
    assert_eq!(swords.len(), 3);
    assert_eq!(
        swords.iter().filter(|item| item.added.is_empty()).count(),
        1
    );
    assert_eq!(
        swords.iter().filter(|item| !item.added.is_empty()).count(),
        2
    );
}

#[test]
fn special_item_sale_and_repair_prices_follow_original_stats() {
    let Some(world) = world() else { return };
    let mut def = world.data.items[&126].clone();
    def.price = 1000;
    def.durability = 5000;
    def.sell_rate = 1.0;
    def.stats.retain(|stat, _| !(56..=59).contains(stat));
    let mut item = crate::items::UserItem::fresh(&world.data, 126, 1).unwrap();
    item.durability = 5000;
    item.max_durability = 5000;
    assert_eq!(crate::items::sell_price(&def, &item), 1000);
    item.add_stat(
        mir_formats::mirdb::stat::MAX_MC,
        3,
        crate::items::StatSource::Added,
    );
    assert_eq!(crate::items::sell_price(&def, &item), 1300);
    item.durability = 2500;
    assert_eq!(crate::items::sell_price(&def, &item), 1137);
    assert_eq!(crate::items::repair_cost(&def, &item, false), 163);
    assert_eq!(crate::items::repair_cost(&def, &item, true), 326);
    item.durability = 5000;
    def.stats.insert(59, 1);
    assert_eq!(crate::items::sell_price(&def, &item), 1560);
}

#[test]
fn gm_moblocation_finds_saved_spawn_by_id_and_name() {
    let Some(mut world) = world() else { return };
    let normal = world.add_player(1, 1, &test_character("Normal")).unwrap();
    let mut record = test_character("Master");
    record.gm = true;
    let gm = world.add_player(2, 2, &record).unwrap();
    let map = world.objects[&gm].map;
    let spawn = world.maps[&map].spawns.first().unwrap();
    let monster_id = spawn.def.monster;
    let monster_name = world.data.monsters[&monster_id].name.clone();
    let map_name = world.data.maps[&map].description.clone();
    drain(&mut world);

    world.chat(normal, format!("@moblocation {monster_id}"));
    assert!(drain(&mut world).iter().any(|message| matches!(
        message, ServerMessage::Say { text, .. } if text == "GM access required."
    )));

    world.chat(gm, format!("@moblocation {monster_id}"));
    let messages = drain(&mut world);
    assert!(messages.iter().any(|message| matches!(
        message, ServerMessage::Say { text, .. } if text.contains(&map_name) && text.contains("@move")
    )));

    world.chat(gm, format!("@moblocation {monster_name}"));
    assert!(drain(&mut world).iter().any(|message| matches!(
        message, ServerMessage::Say { text, .. } if text.contains(&format!("ID {monster_id}"))
    )));
    world.chat(gm, "@moblocation missing monster".into());
    assert!(drain(&mut world).iter().any(|message| matches!(
        message, ServerMessage::Say { text, .. } if text == "Unknown monster: missing monster"
    )));
}

#[test]
fn oma_hero_location_reports_live_monsters_and_spawn_area() {
    let Some(mut world) = world() else { return };
    world.ensure_map(1).unwrap();
    let flea_cave = world.data.map_by_file("D303").unwrap().index;
    world.ensure_map(flea_cave).unwrap();
    assert!(
        world.objects.values().any(|object| object.map == flea_cave
            && !object.dead
            && object.monster().is_some_and(|m| m.def == 49)),
        "Terror Spike must spawn somewhere in Flea Cave Lv 3"
    );
    let live: Vec<_> = world
        .objects
        .values()
        .filter(|object| {
            object.map == 1 && !object.dead && object.monster().is_some_and(|m| m.def == 23)
        })
        .map(|object| object.location)
        .collect();
    assert_eq!(live.len(), 2);
    let mut record = test_character("Tracker");
    record.gm = true;
    let gm = world.add_player(2, 2, &record).unwrap();
    drain(&mut world);
    world.chat(gm, "@moblocation Oma Hero".into());
    let messages = drain(&mut world);
    assert!(messages.iter().any(|message| matches!(
        message, ServerMessage::Say { text, .. }
            if live.iter().any(|point| text.contains(&format!("{}, {}", point.x, point.y)))
    )));
    assert!(messages.iter().any(|message| matches!(
        message, ServerMessage::Say { text, .. } if text.contains("spawn cells")
    )));
}

#[test]
fn gm_move_avoids_portal_at_banya_spawn_hint() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("Traveler");
    record.gm = true;
    let gm = world.add_player(2, 2, &record).unwrap();
    drain(&mut world);
    world.chat(gm, "@move 2 225 13".into());
    world.tick(world.now + 1000);
    assert_eq!(
        world.objects[&gm].map, 6,
        "GM move must stay in Banya Village"
    );
    assert_ne!(world.objects[&gm].location, Point::new(225, 13));
}

#[test]
fn candles_change_player_light_by_brightness() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("LightTester"))
        .unwrap();
    if world.objects[&me].player().unwrap().bag.equipment[mir_proto::slot::TORCH].is_some() {
        let empty = world.objects[&me]
            .player()
            .unwrap()
            .bag
            .inventory
            .iter()
            .position(Option::is_none)
            .unwrap() as u8;
        world.item_move(
            me,
            Grid::Equipment,
            mir_proto::slot::TORCH as u8,
            Grid::Inventory,
            empty,
        );
    }
    world.tick(0);
    drain(&mut world);
    assert_eq!(world.objects[&me].light, 0);
    let mut previous = 0;
    for item in [132, 157] {
        world.test_give_item(me, item, 1);
        let slot = world.test_slot_of(me, item).unwrap();
        world.item_move(
            me,
            Grid::Inventory,
            slot,
            Grid::Equipment,
            mir_proto::slot::TORCH as u8,
        );
        assert!(
            world.objects[&me].light > previous,
            "{} should increase light; stats: {:?}",
            world.data.items[&item].name,
            world.data.items[&item].stats
        );
        previous = world.objects[&me].light;
        world.tick(world.now + 1);
        let messages = drain(&mut world);
        assert!(
            messages.iter().any(|message| matches!(
                message, ServerMessage::ObjectLight { id, light } if *id == me && *light == previous
            )),
            "light messages: {messages:?}"
        );
    }
}

#[test]
fn town_scroll_reports_same_map_teleport_to_client() {
    let Some(mut world) = world() else { return };
    let me = world.add_player(1, 1, &test_character("Traveler")).unwrap();
    let map = world.objects[&me].map;
    let start = world.objects[&me].location;
    let far = (0..world.maps[&map].file.height as i32)
        .flat_map(|y| (0..world.maps[&map].file.width as i32).map(move |x| Point::new(x, y)))
        .find(|p| p.distance(start) > 20 && world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.teleport(me, far);
    world.tick(0);
    drain(&mut world);
    world.test_give_item(me, 156, 1); // Scroll Of Town Portal
    let slot = world.test_slot_of(me, 156).unwrap();
    world.item_use(me, slot);
    assert_ne!(world.objects[&me].location, far);
    world.tick(1);
    let messages = drain(&mut world);
    assert!(messages.iter().any(|message| matches!(
        message, ServerMessage::ObjectTeleport { id, location, .. }
            if *id == me && *location == world.objects[&me].location
    )));
    assert!(messages.iter().any(|message| matches!(
        message, ServerMessage::ObjectEffect { id, effect: mir_proto::effect::TELEPORT_OUT, location }
            if *id == me && *location == far
    )));
    assert!(messages.iter().any(|message| matches!(
        message, ServerMessage::ObjectEffect { id, effect: mir_proto::effect::TELEPORT_IN, location }
            if *id == me && *location == world.objects[&me].location
    )));
}

#[test]
fn town_scroll_plays_arrival_effect_after_changing_maps() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("Cave Traveler"))
        .unwrap();
    let cave = world.data.map_by_file("D303").unwrap().index;
    world.test_change_map(me, cave, Point::new(134, 13));
    world.tick(0);
    drain(&mut world);

    world.test_give_item(me, 156, 1);
    let slot = world.test_slot_of(me, 156).unwrap();
    world.item_use(me, slot);
    assert_ne!(world.objects[&me].map, cave);
    world.tick(1);
    let messages = drain(&mut world);
    let map_change = messages
        .iter()
        .position(|message| matches!(message, ServerMessage::MapChanged { .. }));
    let arrival = messages.iter().position(|message| matches!(
        message, ServerMessage::ObjectEffect { id, effect: mir_proto::effect::TELEPORT_IN, location }
            if *id == me && *location == world.objects[&me].location
    ));
    assert!(
        matches!((map_change, arrival), (Some(map), Some(effect)) if map < effect),
        "town arrival effect should play on destination map: {messages:?}"
    );
}

#[test]
fn groups_invite_join_share_experience_and_dissolve() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    drain(&mut world);
    let conn_of = |world: &World, id: ObjectId| world.objects[&id].player().unwrap().conn;
    let (ca, cb) = (conn_of(&world, alice), conn_of(&world, bob));
    let to = |world: &mut World| -> Vec<(world::ConnId, ServerMessage)> {
        world
            .outgoing
            .drain(..)
            .map(|o| match o {
                Outgoing::To(c, m) => (c, m),
            })
            .collect()
    };

    // Bob has not allowed groups: the invite is refused with a system line.
    world.group_invite(alice, "bob".into());
    let out = to(&mut world);
    assert!(out.iter().any(|(c, m)| *c == ca
        && matches!(m, ServerMessage::Say { text, .. } if text.contains("not allowing"))));
    assert!(!out
        .iter()
        .any(|(_, m)| matches!(m, ServerMessage::GroupInvite { .. })));

    world.group_switch(bob, true);
    world.group_invite(alice, "BOB".into());
    let out = to(&mut world);
    assert!(out
        .iter()
        .any(|(c, m)| *c == cb
            && matches!(m, ServerMessage::GroupInvite { from } if from == "Alice")));

    world.group_response(bob, true);
    let out = to(&mut world);
    // Alice (inviter, now leader) learns of herself and Bob; Bob gets Alice
    // then himself.
    let members = |c: world::ConnId, out: &[(world::ConnId, ServerMessage)]| -> Vec<ObjectId> {
        out.iter()
            .filter_map(|(cc, m)| match m {
                ServerMessage::GroupMember { id, .. } if *cc == c => Some(*id),
                _ => None,
            })
            .collect()
    };
    assert_eq!(members(ca, &out), vec![alice, bob]);
    assert_eq!(members(cb, &out), vec![alice, bob]);
    assert!(out
        .iter()
        .any(|(c, m)| *c == ca && matches!(m, ServerMessage::GroupSwitch { allow: true })));

    // Group chat now reaches both.
    world.chat(bob, "!!hi".into());
    let out = to(&mut world);
    assert_eq!(
        out.iter()
            .filter(|(_, m)| matches!(m, ServerMessage::Say { text, .. } if text == "Bob: hi"))
            .count(),
        2
    );

    // A kill by Alice shares experience with Bob standing nearby.
    let before_a = world.test_experience(alice);
    let before_b = world.test_experience(bob);
    let chicken = nearest_chicken(&world, alice);
    let exp = world.test_monster_experience(chicken);
    world.test_set_target(chicken, alice);
    world.test_damage(chicken, alice, 1000);
    let gained_a = world.test_experience(alice) - before_a;
    let gained_b = world.test_experience(bob) - before_b;
    assert!(gained_a > 0 && gained_b > 0, "{gained_a} {gained_b}");
    // Two level-1 members: 1.12 * exp split in half each.
    let expect = ((exp as f64 * 1.12) * 0.5) as u64;
    assert!(gained_a.abs_diff(expect) <= 1, "{gained_a} vs {expect}");

    // Only the leader may kick; a group of one dissolves.
    world.group_remove(bob, "Alice".into());
    let out = to(&mut world);
    assert!(out.iter().any(|(c, m)| *c == cb
        && matches!(m, ServerMessage::Say { text, .. } if text.contains("not the leader"))));
    world.group_remove(alice, "bob".into());
    let out = to(&mut world);
    assert!(out
        .iter()
        .any(|(c, m)| *c == cb && matches!(m, ServerMessage::GroupRemove { id } if *id == bob)));
    assert!(out
        .iter()
        .any(|(c, m)| *c == ca && matches!(m, ServerMessage::GroupRemove { id } if *id == alice)));
    assert!(world.objects[&alice].player().unwrap().group.is_none());
    assert!(world.objects[&bob].player().unwrap().group.is_none());
}

#[test]
fn storage_in_safe_zone_and_face_to_face_trade() {
    use mir_proto::Grid;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    drain(&mut world);
    // Storage: the warrior start is a safe zone; a stored item survives a
    // round trip through the account record.
    assert!(world.test_in_safe_zone(alice));
    let info = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .unwrap()
        .index;
    world.test_give_item(alice, info, 5);
    let slot = world.test_slot_of(alice, info).unwrap();
    world.item_move(alice, Grid::Inventory, slot, Grid::Storage, 3);
    assert_eq!(world.test_storage(alice).len(), 1);
    assert_eq!(world.test_storage(alice)[0].0, 3);
    let (stored, size) = world.storage_of(alice).unwrap();
    assert_eq!((stored.len(), size), (1, 100));
    world.item_move(alice, Grid::Storage, 3, Grid::Inventory, slot);
    assert!(world.test_storage(alice).is_empty());
    assert_eq!(world.test_slot_of(alice, info), Some(slot));
    drain(&mut world);

    // Trade: Bob stands in front of Alice, both facing each other.
    let loc = world.objects[&alice].location;
    let map = world.objects[&alice].map;
    let (dir, cell) = Direction::ALL
        .iter()
        .map(|d| (*d, loc.step(*d, 1)))
        .find(|(_, p)| world.maps[&map].file.is_walkable(p.x, p.y))
        .expect("walkable neighbour");
    world.teleport(bob, cell);
    world.test_face(alice, dir);
    world.test_face(bob, dir.opposite());
    world.test_set_gold(alice, 500);
    world.test_set_gold(bob, 0);
    world.trade_request(alice);
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::TradeRequest { from } if from == "Alice")));
    world.trade_response(bob, true);
    let msgs = drain(&mut world);
    assert_eq!(
        msgs.iter()
            .filter(|m| matches!(m, ServerMessage::TradeOpen { .. }))
            .count(),
        2
    );
    // Alice offers her first item and 300 gold; gold can only be raised.
    world.trade_add_item(alice, Grid::Inventory, slot, 5);
    world.trade_add_gold(alice, 300);
    world.trade_add_gold(alice, 100);
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::TradeItemAdded { item } if item.info == info)));
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::TradeGoldAdded { gold: 300 })));
    world.trade_confirm(alice);
    world.trade_confirm(bob);
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(|m| matches!(m, ServerMessage::TradeClose)));
    assert_eq!(world.test_gold(alice), 200);
    assert_eq!(world.test_gold(bob), 300);
    assert_eq!(world.test_slot_of(alice, info), None);
    assert!(world.test_slot_of(bob, info).is_some());
    assert!(world.objects[&alice].player().unwrap().trade.is_none());

    // A step closes an open trade.
    world.trade_request(alice);
    world.trade_response(bob, true);
    drain(&mut world);
    world.player_turn(alice, dir.opposite());
    let msgs = drain(&mut world);
    assert_eq!(
        msgs.iter()
            .filter(|m| matches!(m, ServerMessage::TradeClose))
            .count(),
        2
    );
}

#[test]
fn pvp_attack_modes_brown_and_pk_points() {
    use mir_proto::attack_mode;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    drain(&mut world);
    // The start is a safe zone: nobody can be attacked there whatever the mode.
    world.set_attack_mode(alice, attack_mode::ALL);
    assert!(!world.objects[&alice].hostile_to(&world.objects[&bob]));
    let map = world.objects[&alice].map;
    let start = world.objects[&alice].location;
    let cell = world
        .test_cell_outside_safe_zone(map, start)
        .expect("a cell outside the safe zone");
    world.teleport(alice, cell);
    let next = Direction::ALL
        .iter()
        .map(|d| cell.step(*d, 1))
        .find(|p| {
            world.maps[&map].file.is_walkable(p.x, p.y) && !world.test_in_safe_zone_at(map, *p)
        })
        .expect("neighbour outside the safe zone");
    world.teleport(bob, next);
    assert!(!world.test_in_safe_zone(alice) && !world.test_in_safe_zone(bob));
    // Peaceful never, All always, Group spares group mates, War/Red/Brown
    // only hostile names.
    world.set_attack_mode(alice, attack_mode::PEACE);
    assert!(!world.objects[&alice].hostile_to(&world.objects[&bob]));
    world.set_attack_mode(alice, attack_mode::WAR_RED_BROWN);
    assert!(!world.objects[&alice].hostile_to(&world.objects[&bob]));
    world.set_attack_mode(alice, attack_mode::ALL);
    assert!(world.objects[&alice].hostile_to(&world.objects[&bob]));
    drain(&mut world);

    // Hitting an innocent turns Alice brown (name colour 2 broadcast).
    world.test_damage(bob, alice, 5);
    world.tick(1);
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(|m| matches!(m,
        ServerMessage::ObjectAppearance { id, appearance: Appearance::Player { name_color: 2, .. } } if *id == alice)));
    // Brown Alice is fair game for Bob in War/Red/Brown mode.
    world.set_attack_mode(bob, attack_mode::WAR_RED_BROWN);
    assert!(world.objects[&bob].hostile_to(&world.objects[&alice]));

    // Murdering Bob adds 50 PK points; brown still shows over yellow.
    world.test_damage(bob, alice, 100_000);
    world.tick(2);
    assert!(world.objects[&bob].dead);
    assert_eq!(world.objects[&alice].player().unwrap().pk_points, 50);
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(|m| matches!(m,
        ServerMessage::Say { text, .. } if text.contains("murdered by Alice"))));

    // At 200 points the name is red and guards turn hostile.
    world.test_set_pk(alice, 200);
    world.tick(3);
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(|m| matches!(
        m,
        ServerMessage::ObjectAppearance {
            appearance: Appearance::Player { name_color: 3, .. },
            ..
        }
    )));
    let guard = world
        .objects
        .values()
        .find(|o| matches!(&o.kind, world::Kind::Monster(m) if m.guard))
        .map(|o| o.id)
        .expect("a guard");
    assert!(world.objects[&guard].hostile_to(&world.objects[&alice]));
    assert!(!world.objects[&guard].hostile_to(&world.objects[&bob]));
}

#[test]
fn guild_create_invite_notice_kick_and_leave() {
    use mir_proto::{guild_permission, ChatKind};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    // Guild membership is keyed by character id: give Bob his own.
    let mut bob_rec = test_character("Bob");
    bob_rec.id = 2;
    let bob = world.add_player(2, 2, &bob_rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let conn_of = |world: &World, id: ObjectId| world.objects[&id].player().unwrap().conn;
    let (ca, cb) = (conn_of(&world, alice), conn_of(&world, bob));
    let to = |world: &mut World| -> Vec<(world::ConnId, ServerMessage)> {
        world
            .outgoing
            .drain(..)
            .map(|o| match o {
                Outgoing::To(c, m) => (c, m),
            })
            .collect()
    };
    // Creation needs 7.5M + 1M per member slot; bad names are refused.
    world.guild_create(alice, "Bad Name!".into(), 5);
    assert!(world.objects[&alice].player().unwrap().guild.is_none());
    world.test_set_gold(alice, 20_000_000);
    world.guild_create(alice, "Knights".into(), 5);
    assert_eq!(world.test_gold(alice), 20_000_000 - 7_500_000 - 5_000_000);
    let out = to(&mut world);
    let info = out.iter().find_map(|(c, m)| match m {
        ServerMessage::GuildInfo(Some(g)) if *c == ca => Some(g.clone()),
        _ => None,
    });
    let info = info.expect("guild info");
    assert_eq!(info.name, "Knights");
    assert_eq!(info.members[0].permission, guild_permission::LEADER);
    assert_eq!(info.user_index, 1);
    world.guild_create(bob, "Knights".into(), 1);
    assert!(world.objects[&bob].player().unwrap().guild.is_none());

    // Invite and accept: Bob joins with the default rank.
    world.guild_invite(alice, "bob".into());
    let out = to(&mut world);
    assert!(out.iter().any(|(c, m)| *c == cb
        && matches!(m, ServerMessage::GuildInvite { from, guild } if from == "Alice" && guild == "Knights")));
    world.guild_response(bob, true);
    let out = to(&mut world);
    let bob_info = out
        .iter()
        .find_map(|(c, m)| match m {
            ServerMessage::GuildInfo(Some(g)) if *c == cb => Some(g.clone()),
            _ => None,
        })
        .expect("bob's guild info");
    assert_eq!(bob_info.members.len(), 2);
    assert_eq!(bob_info.members[1].rank, "New Member");
    assert!(bob_info.members.iter().all(|m| m.online));
    world.tick(1);
    let out = to(&mut world);
    assert!(out.iter().any(|(_, m)| matches!(m,
        ServerMessage::ObjectAppearance { id, appearance: Appearance::Player { guild, guild_rank, .. } }
            if *id == bob && guild == "Knights" && guild_rank == "New Member")));

    // Guild chat reaches both; the notice needs the EditNotice permission.
    world.chat(bob, "!~hail".into());
    let out = to(&mut world);
    assert_eq!(
        out.iter()
            .filter(|(_, m)| matches!(
                m,
                ServerMessage::Say {
                    kind: ChatKind::Guild,
                    ..
                }
            ))
            .count(),
        2
    );
    world.guild_edit_notice(bob, "Bob was here".into());
    assert_eq!(world.guild_store.guilds[0].notice, "");
    world.guild_edit_member(alice, 2, "Officer".into(), guild_permission::EDIT_NOTICE);
    world.guild_edit_notice(bob, "Bob was here".into());
    assert_eq!(world.guild_store.guilds[0].notice, "Bob was here");
    drain(&mut world);

    // Tax on picked-up gold feeds the funds.
    world.guild_tax(alice, 10);
    assert_eq!(world.guild_tax_gold_test(bob, 1000), 900);
    assert_eq!(world.guild_store.guilds[0].funds, 100);

    // Only the leader kicks; the leader cannot leave while alone in charge.
    world.guild_kick(bob, 1);
    assert_eq!(world.guild_store.guilds[0].members.len(), 2);
    world.guild_leave(alice);
    assert_eq!(world.guild_store.guilds[0].members.len(), 2);
    world.guild_kick(alice, 2);
    let out = to(&mut world);
    assert!(out
        .iter()
        .any(|(c, m)| *c == cb && matches!(m, ServerMessage::GuildInfo(None))));
    assert!(world.objects[&bob].player().unwrap().guild.is_none());
    world.guild_leave(alice);
    assert!(world.guild_store.guilds.is_empty());
    assert!(world.objects[&alice].player().unwrap().guild.is_none());
}

#[test]
fn mail_send_take_items_and_delete() {
    use mir_proto::Grid;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::MailList(l) if l.is_empty())));
    let potion = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .unwrap()
        .index;
    world.test_give_item(alice, potion, 4);
    world.test_set_gold(alice, 1000);
    let slot = world.test_slot_of(alice, potion).unwrap();
    // Self mail and unknown recipients are refused; Bob gets the mail live.
    world.mail_send(
        alice,
        Some((1, "Alice".into())),
        "hi".into(),
        "me".into(),
        0,
        vec![],
    );
    world.mail_send(alice, None, "hi".into(), "nobody".into(), 0, vec![]);
    assert!(world.mail_store.boxes.values().all(|b| b.is_empty()));
    world.tick(20_000);
    drain(&mut world);
    world.mail_send(
        alice,
        Some((2, "Bob".into())),
        "Potions".into(),
        "Three for you".into(),
        250,
        vec![(Grid::Inventory, slot, 3)],
    );
    assert_eq!(world.test_gold(alice), 750);
    assert_eq!(world.test_bag(alice).1, vec![(potion, 1)]);
    let msgs = drain(&mut world);
    let mail = msgs
        .iter()
        .find_map(|m| match m {
            ServerMessage::MailNew(m) => Some(m.clone()),
            _ => None,
        })
        .expect("bob's new mail");
    assert_eq!(
        (mail.sender.as_str(), mail.gold, mail.items[0].count),
        ("Alice", 250, 3)
    );
    // Bob opens it, cannot delete it with items inside, takes gold and
    // items (the start is a safe zone), then deletes it.
    world.mail_opened(bob, mail.index);
    world.mail_delete(bob, mail.index);
    assert_eq!(world.mail_store.boxes[&2].len(), 1);
    world.mail_get_item(bob, mail.index, 255); // the gold pseudo slot
    assert_eq!(world.test_gold(bob), 250);
    world.mail_get_item(bob, mail.index, 0);
    assert_eq!(world.test_bag(bob).1, vec![(potion, 3)]);
    world.mail_delete(bob, mail.index);
    assert!(world.mail_store.boxes[&2].is_empty());
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::MailDelete { index } if *index == mail.index)));
}

#[test]
fn marriage_ring_teleport_and_horses() {
    use crate::data::{NpcActionDef, NpcCheckDef};
    use mir_proto::{horse_type, item_type, slot, ServerMessage as S};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let mut bob_rec = test_character("Bob");
    bob_rec.id = 2;
    let bob = world.add_player(2, 2, &bob_rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let action = |t: i32, i1: i32| NpcActionDef {
        action_type: t,
        string1: String::new(),
        int1: i1,
        int2: 0,
        item1: 0,
        map1: 0,
        stat1: 0,
    };
    let check = |t: i32, op: i32, i1: i32| NpcCheckDef {
        check_type: t,
        operator: op,
        string1: String::new(),
        int1: i1,
        int2: 0,
        item1: 0,
        stat1: 0,
        fail_page: 0,
    };
    let map = world.objects[&alice].map;
    let loc = world.objects[&alice].location;

    // ---- Marriage: face to face, level 22, 500,000 gold each ----
    let (dir, cell) = Direction::ALL
        .iter()
        .map(|d| (*d, loc.step(*d, 1)))
        .find(|(_, p)| world.maps[&map].file.is_walkable(p.x, p.y))
        .expect("walkable neighbour");
    world.teleport(bob, cell);
    world.test_face(alice, dir);
    world.test_face(bob, dir.opposite());
    for id in [alice, bob] {
        world
            .objects
            .get_mut(&id)
            .unwrap()
            .player_mut()
            .unwrap()
            .level = 22;
        world.test_set_gold(id, 600_000);
    }
    assert!(!world.test_npc_check(alice, &check(11, 0, 0)));
    world.test_npc_action(alice, &action(8, 0));
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, S::MarriageInvite { from } if from == "Alice")));
    world.marriage_response(bob, true);
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(
        |m| matches!(m, S::MarriageInfo { partner: Some(p), .. } if p == "Bob" || p == "Alice")
    ));
    assert_eq!(world.test_gold(alice), 100_000);
    assert_eq!(world.test_gold(bob), 100_000);
    assert_eq!(
        world.objects[&alice]
            .player()
            .unwrap()
            .partner
            .as_ref()
            .map(|(_, n)| n.as_str()),
        Some("Bob")
    );
    assert!(world.test_npc_check(alice, &check(11, 0, 0)));
    assert!(!world.test_npc_check(alice, &check(12, 0, 0)));

    // A ring from the bag becomes the wedding ring on the left finger.
    let ring = world
        .data
        .items
        .values()
        .filter(|d| d.item_type == item_type::RING && d.required_amount <= 1)
        .min_by_key(|d| d.index)
        .expect("a ring")
        .index;
    world.test_give_item(alice, ring, 1);
    let ring_slot = world.test_slot_of(alice, ring).unwrap();
    world.marriage_make_ring(alice, ring_slot);
    {
        let p = world.objects[&alice].player().unwrap();
        let worn = p.bag.equipment[slot::RING_L].as_ref().expect("ring worn");
        assert_eq!(p.wedding_ring, Some(worn.id));
    }
    assert!(world.test_npc_check(alice, &check(12, 0, 0)));

    // The ring teleports to the partner within 10 cells, then waits 2 min.
    world.data.maps.get_mut(&map).unwrap().can_marriage_recall = true;
    let far = (0..world.maps[&map].file.width as i32)
        .flat_map(|x| (0..world.maps[&map].file.height as i32).map(move |y| Point::new(x, y)))
        .find(|p| {
            p.distance(loc) >= 20
                && p.distance(loc) <= 40
                && world.maps[&map].file.is_walkable(p.x, p.y)
                && world.maps[&map].objects_at(*p).is_empty()
        })
        .expect("a far cell");
    world.teleport(bob, far);
    drain(&mut world);
    world.marriage_teleport(alice);
    assert!(world.objects[&alice].location.distance(far) <= 10);
    world.marriage_teleport(alice);
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, S::Say { text, .. } if text.contains("another"))));

    // Divorce clears both sides and the ring flag.
    world.test_npc_action(alice, &action(9, 0));
    assert!(world.objects[&alice].player().unwrap().partner.is_none());
    assert!(world.objects[&bob].player().unwrap().partner.is_none());
    assert!(world.objects[&alice]
        .player()
        .unwrap()
        .wedding_ring
        .is_none());
    assert!(!world.test_npc_check(alice, &check(11, 0, 0)));

    // ---- Horses: owned through the NPC, ridden where the map allows ----
    drain(&mut world);
    world.mount_toggle(alice);
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, S::Say { text, .. } if text.contains("do not own"))));
    let (bag_before, ac_before) = {
        let o = &world.objects[&alice];
        (o.player().unwrap().max_bag, o.stats.max_ac)
    };
    world.test_npc_action(alice, &action(6, horse_type::WHITE as i32));
    {
        let o = &world.objects[&alice];
        assert_eq!(o.player().unwrap().horse, horse_type::WHITE);
        assert_eq!(o.player().unwrap().max_bag, bag_before + 100);
        assert_eq!(o.stats.max_ac, ac_before + 5);
    }
    assert!(world.test_npc_check(alice, &check(10, 0, horse_type::WHITE as i32)));
    world.data.maps.get_mut(&map).unwrap().can_horse = false;
    world.mount_toggle(alice);
    assert!(!world.objects[&alice].player().unwrap().mounted);
    world.data.maps.get_mut(&map).unwrap().can_horse = true;
    world.tick(5_000);
    drain(&mut world);
    world.mount_toggle(alice);
    assert!(world.objects[&alice].player().unwrap().mounted);
    world.tick(6_000);
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(|m| matches!(m,
        S::ObjectAppearance { id, appearance: Appearance::Player { horse, .. } } if *id == alice && *horse == horse_type::WHITE)));
    // No attacking from the saddle; a run covers three cells.
    world.player_attack(alice, dir, None);
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, S::Chat { text } if text.contains("riding"))));
    let start = world.objects[&alice].location;
    let ride = Direction::ALL.iter().copied().find(|d| {
        (1..=3).all(|i| {
            let p = start.step(*d, i);
            world.maps[&map].file.is_walkable(p.x, p.y) && world.maps[&map].objects_at(p).is_empty()
        })
    });
    if let Some(d) = ride {
        world.tick(7_000);
        world.player_move(alice, d, true);
        assert_eq!(world.objects[&alice].location, start.step(d, 3));
    }
    // Dying throws the rider off.
    world.test_kill(alice);
    assert!(!world.objects[&alice].player().unwrap().mounted);
}

#[test]
fn mining_a_wall_with_a_pickaxe_yields_ore_and_rubble() {
    use mir_proto::{slot, spell_effect, Grid};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    // Pick Axes need level 20.
    let mut rec = test_character("Miner");
    rec.level = 20;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    // Deserted Mine Lv 1 (136) can be mined: copper 1/50, iron 1/140 ...
    let (spot, dir) = world.test_wall_spot(136).expect("a wall in the mine");
    world.test_change_map(me, 136, spot);
    let pickaxe = world
        .data
        .items
        .values()
        .find(|d| d.name == "Pick Axe")
        .unwrap()
        .index;
    world.test_give_item(me, pickaxe, 1);
    let slot_in_bag = world.test_slot_of(me, pickaxe).unwrap();
    world.item_move(
        me,
        Grid::Inventory,
        slot_in_bag,
        Grid::Equipment,
        slot::WEAPON as u8,
    );
    let msgs = drain(&mut world);
    assert_eq!(world.objects[&me].map, 136, "moved to the mine");
    let def = world.data.items[&pickaxe].clone();
    assert_eq!(
        world.test_equipped(me, slot::WEAPON),
        Some(pickaxe),
        "pickaxe equipped ({} {} {}): {msgs:?}",
        def.required_class,
        def.required_type,
        def.required_amount
    );
    let start_dur = world.test_equipment_durability(me, slot::WEAPON).unwrap();
    // The mine has monsters and a level-1 tester: clear them out of the way.
    world.test_clear_monsters(136);
    // Swing until some ore turns up (1 in 50 for copper alone).
    let mut now = 1000;
    let mut swings = 0;
    let ore_types = [537, 538, 539, 540, 541];
    while swings < 400 {
        now += 1600;
        world.tick(now);
        if swings % 20 == 0 {
            world.test_clear_monsters(136);
        }
        world.mining(me, dir);
        swings += 1;
        if world
            .test_bag(me)
            .1
            .iter()
            .any(|(info, _)| ore_types.contains(info))
        {
            break;
        }
    }
    if swings >= 400 {
        let msgs = drain(&mut world);
        let mining: Vec<_> = msgs
            .iter()
            .filter(|m| matches!(m, ServerMessage::ObjectMining { .. }))
            .take(3)
            .collect();
        panic!(
            "no ore after {swings} swings; dur {:?}; can_mine {:?}; front walkable {}; mining {mining:?}",
            world.test_equipment_durability(me, slot::WEAPON),
            world.data.maps.get(&136).map(|m| m.can_mine),
            world.maps[&136].file.is_walkable(spot.step(dir, 1).x, spot.step(dir, 1).y)
        );
    }
    assert_eq!(
        world.test_equipment_durability(me, slot::WEAPON),
        Some(start_dur - 4 * swings)
    );
    assert!(world.test_spell_at(136, spot, spell_effect::RUBBLE));
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::ObjectMining { effect: true, .. })));
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::Say { text, .. } if text.starts_with("You mined"))));
    // Swinging at open ground does nothing but the animation.
    if let Some(open) = Direction::ALL.iter().copied().find(|d| {
        let n = spot.step(*d, 1);
        world.maps[&136].file.is_walkable(n.x, n.y)
    }) {
        world.tick(now + 2000);
        world.mining(me, open);
        world.tick(now + 2100);
        let msgs = drain(&mut world);
        assert!(msgs
            .iter()
            .any(|m| matches!(m, ServerMessage::ObjectMining { effect: false, .. })));
    }
}

#[test]
fn fishing_reels_in_a_catch_with_the_dev_zone() {
    use mir_proto::fishing_state;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Angler")).unwrap();
    world.tick(0);
    drain(&mut world);
    // No fishing zones, rods or bait ship in this pack: without the dev
    // zone every cast is refused.
    let map = world.objects[&me].map;
    // The town centre is open ground: stand next to the nearest wall.
    let (spot, dir) = world.test_wall_spot(map).expect("a wall on the start map");
    world.teleport(me, spot);
    let loc = spot;
    let water = loc.step(dir, 1);
    world.fishing_cast(me, fishing_state::CAST, dir, water, false);
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::Say { text, .. } if text.contains("no fish"))));
    let potion = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .unwrap()
        .index;
    world.dev_fishing_item = Some(potion);
    // Recast every 1.6 s, always reeling: once a fish nibbles the points
    // only climb, so the catch is certain. The first nibble can be a miss.
    let mut now = 1000;
    world.tick(now);
    world.fishing_cast(me, fishing_state::CAST, dir, water, false);
    assert!(world.objects[&me].player().unwrap().fishing.is_some());
    let mut casts = 0;
    while world.objects[&me].player().unwrap().fishing.is_some() && casts < 500 {
        now += 1600;
        world.tick(now);
        world.fishing_cast(me, fishing_state::CAST, dir, water, true);
        casts += 1;
    }
    assert!(casts < 500, "still fishing after {casts} casts");
    world.tick(now + 100);
    assert_eq!(world.test_bag(me).1, vec![(potion, 1)]);
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(|m| {
        matches!(m, ServerMessage::Say { text, .. } if text == "You caught a Healing Potion.")
    }));
    assert!(msgs.iter().any(
        |m| matches!(m, ServerMessage::ObjectFishing { state, .. } if *state == fishing_state::REEL)
    ));
    // A step cancels a cast in progress.
    world.tick(now + 2000);
    world.fishing_cast(me, fishing_state::CAST, dir, water, false);
    assert!(world.objects[&me].player().unwrap().fishing.is_some());
    world.tick(now + 4000);
    world.player_turn(me, dir.opposite());
    world.tick(now + 8000);
    assert!(world.objects[&me].player().unwrap().fishing.is_none());
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::ObjectFishing { state, .. } if *state == fishing_state::CANCEL)));
}

#[test]
fn refine_weapon_and_retrieve_it() {
    use mir_proto::{refine_quality, refine_type, Grid};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Smith")).unwrap();
    world.tick(0);
    drain(&mut world);
    // A level-1 warrior weapon and black iron ore (ItemEffect 20).
    let weapon = world
        .data
        .items
        .values()
        .filter(|d| d.item_type == mir_proto::item_type::WEAPON && d.required_amount <= 1)
        .filter(|d| {
            d.effect != 5
                && crate::items::can_use(d, mir_proto::Class::Warrior, mir_proto::Gender::Male, 1)
                    .is_ok()
        })
        .min_by_key(|d| d.index)
        .map(|d| d.index)
        .expect("a starter weapon");
    let ore = world
        .data
        .items
        .values()
        .find(|d| d.effect == 20)
        .map(|d| d.index)
        .expect("black iron ore");
    world.test_give_item(me, weapon, 1);
    world.test_give_item(me, ore, 1);
    world.test_set_gold(me, 100_000);
    let wslot = world.test_slot_of(me, weapon).unwrap();
    world.item_move(me, Grid::Inventory, wslot, Grid::Equipment, 0);
    let oslot = world.test_slot_of(me, ore).unwrap();
    // Combat unlocks the first refine; the furnace consumes this readiness.
    world.gain_experience_test(me, 1);
    assert!(
        world.objects[&me].player().unwrap().bag.equipment[0]
            .as_ref()
            .unwrap()
            .refinable
    );
    // Refining needs a Refine page open; the retrieve page is another type.
    let refine_page = world.test_page_with_dialog_type(3);
    let retrieve_page = world.test_page_with_dialog_type(4);
    world.test_open_page(me, retrieve_page);
    world.npc_refine(
        me,
        refine_type::DC,
        refine_quality::RUSH,
        vec![(Grid::Inventory, oslot, 1)],
        vec![],
        vec![],
    );
    assert!(world.objects[&me].player().unwrap().refines.is_empty());
    world.test_open_page(me, refine_page);
    world.npc_refine(
        me,
        refine_type::DC,
        refine_quality::RUSH,
        vec![(Grid::Inventory, oslot, 1)],
        vec![],
        vec![],
    );
    let p = world.objects[&me].player().unwrap();
    assert_eq!(p.refines.len(), 1);
    assert!(p.bag.equipment[0].is_none());
    assert_eq!(p.bag.gold, 50_000);
    assert_eq!(world.test_slot_of(me, ore), None);
    let r = p.refines[0].clone();
    assert_eq!(
        (r.refine_type, r.quality),
        (refine_type::DC, refine_quality::RUSH)
    );
    assert!(r.chance <= r.max_chance && r.max_chance <= 85);
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::RefineList(l) if l.len() == 1)));
    // Not ready yet; then force it ready and collect on a retrieve page.
    world.test_open_page(me, retrieve_page);
    world.npc_refine_retrieve(me, r.index);
    assert_eq!(world.objects[&me].player().unwrap().refines.len(), 1);
    world
        .objects
        .get_mut(&me)
        .and_then(|o| o.player_mut())
        .unwrap()
        .refines[0]
        .ready_at = 0;
    world.npc_refine_retrieve(me, r.index);
    let p = world.objects[&me].player().unwrap();
    assert!(p.refines.is_empty());
    let back = p
        .bag
        .inventory
        .iter()
        .flatten()
        .find(|i| i.id == r.weapon.id)
        .expect("weapon back in the bag");
    // Success adds a sourced stat; both outcomes consume readiness.
    assert_eq!(back.level, 1);
    assert!(!back.refinable);
    assert!(
        back.added.is_empty()
            || back.added.iter().any(|s| s.stat == 9
                && s.amount == 1
                && s.source == crate::items::StatSource::Refine)
    );
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::RefineRetrieved { index } if *index == r.index)));
}

#[test]
fn companion_adopt_follow_pick_up_and_store() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    if world.data.companions.is_empty() {
        eprintln!("no CompanionInfo rows; skipping");
        return;
    }
    let me = world.add_player(1, 1, &test_character("Keeper")).unwrap();
    world.tick(0);
    drain(&mut world);
    let offer = world
        .data
        .companions
        .iter()
        .find(|d| d.available)
        .or_else(|| world.data.companions.first())
        .cloned()
        .unwrap();
    if !offer.available {
        world
            .objects
            .get_mut(&me)
            .and_then(|o| o.player_mut())
            .unwrap()
            .companion_unlocks
            .push(offer.index);
    }
    world.test_set_gold(me, offer.price.max(0) as u64 + 1000);
    let page = world.test_page_with_dialog_type(5);
    // Adoption needs the page and a valid name.
    world.companion_adopt(me, offer.index, "Rex".into());
    assert!(world.objects[&me].player().unwrap().companions.is_empty());
    world.test_open_page(me, page);
    world.companion_adopt(me, offer.index, "R!".into());
    assert!(world.objects[&me].player().unwrap().companions.is_empty());
    world.companion_adopt(me, offer.index, "Rex".into());
    let p = world.objects[&me].player().unwrap();
    assert_eq!(p.companions.len(), 1);
    assert_eq!(p.companions[0].name, "Rex");
    assert!(p.companion.is_none());
    // Out it comes: a companion object next to us that never fights.
    world.companion_retrieve(me, 1);
    let cid = world.objects[&me]
        .player()
        .unwrap()
        .companion
        .expect("spawned");
    assert!(matches!(&world.objects[&cid].kind, world::Kind::Monster(m) if m.companion == Some(1)));
    assert!(!world.objects[&cid].hostile_to(&world.objects[&me]));
    let chicken = nearest_chicken(&world, me);
    assert!(!world.objects[&chicken].hostile_to(&world.objects[&cid]));
    // Our drop nearby gets collected into its bag within a few ticks.
    let (map, loc, account) = {
        let o = &world.objects[&me];
        (o.map, o.location, o.player().unwrap().account)
    };
    let potion = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .unwrap()
        .index;
    let drop_at = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 2))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    // Level 1 companions carry nothing (CompanionLevelInfo); level it up.
    world
        .objects
        .get_mut(&me)
        .and_then(|o| o.player_mut())
        .unwrap()
        .companions[0]
        .level = 2;
    world.test_drop_item(map, drop_at, potion, 2, account);
    let mut t = 100;
    let mut picked = false;
    while t < 20_000 {
        world.tick(t);
        t += 100;
        if world.objects[&me].player().unwrap().companions[0]
            .items
            .len()
            == 1
        {
            picked = true;
            break;
        }
    }
    assert!(picked, "companion did not pick up the drop");
    {
        let picked_item = &mut world
            .objects
            .get_mut(&me)
            .unwrap()
            .player_mut()
            .unwrap()
            .companions[0]
            .items[0];
        picked_item.durability = 1_500;
        picked_item.max_durability = 2_000;
        picked_item.bound = true;
        picked_item.colour = 0xFF0088CC;
    }
    drain(&mut world);
    world.companion_bag_take(me, 1, 0);
    assert_eq!(world.test_bag(me).1, vec![(potion, 2)]);
    let taken = world.objects[&me]
        .player()
        .unwrap()
        .bag
        .inventory
        .iter()
        .flatten()
        .find(|i| i.info == potion)
        .unwrap();
    assert_eq!(
        (
            taken.durability,
            taken.max_durability,
            taken.bound,
            taken.colour
        ),
        (1_500, 2_000, true, 0xFF0088CC)
    );
    assert!(world.objects[&me].player().unwrap().companions[0]
        .items
        .is_empty());
    // Store it away: the object disappears; release needs an empty bag.
    world.companion_store(me);
    let p = world.objects[&me].player().unwrap();
    assert!(p.companion.is_none() && p.active_companion.is_none());
    assert!(!world.objects.contains_key(&cid));
    world.companion_release(me, 1);
    assert!(world.objects[&me].player().unwrap().companions.is_empty());
}

#[test]
fn companion_skills_roll_at_milestones_once_and_survive_storage() {
    use crate::world::StoredCompanion;

    let Some(mut world) = world() else { return };
    assert!(!world.data.companion_skills.is_empty());
    let offer = world
        .data
        .companions
        .iter()
        .find(|offer| world.data.monsters.contains_key(&offer.monster))
        .unwrap()
        .clone();
    let me = world
        .add_player(1, 1, &test_character("SkillKeeper"))
        .unwrap();
    let max_exp = world
        .data
        .companion_levels
        .iter()
        .find(|def| def.level == 2)
        .unwrap()
        .max_experience;
    let p = world.objects.get_mut(&me).unwrap().player_mut().unwrap();
    p.companions.push(StoredCompanion {
        index: 1,
        info: offer.index,
        name: "Rex".into(),
        level: 2,
        experience: max_exp - 1,
        hunger: 100,
        items: Vec::new(),
        food: None,
        bag: None,
        bonuses: Vec::new(),
    });
    let page = world.test_page_with_dialog_type(5);
    world.test_open_page(me, page);
    world.companion_retrieve(me, 1);
    assert!(world.objects[&me].player().unwrap().companions[0]
        .bonuses
        .is_empty());

    world.tick(60_000);
    let companion = &world.objects[&me].player().unwrap().companions[0];
    assert_eq!(companion.level, 3);
    assert_eq!(companion.bonuses.len(), 1);
    let bonus = companion.bonuses[0];
    assert_eq!(bonus.0, 3);
    assert!(world.data.companion_skills.iter().any(|skill| {
        skill.level == 3 && skill.stat == bonus.1 && (1..=skill.max_amount).contains(&bonus.2)
    }));
    let saved = serde_json::to_string(companion).unwrap();
    let restored: StoredCompanion = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.bonuses, vec![bonus]);

    world.companion_store(me);
    world.companion_retrieve(me, 1);
    assert_eq!(
        world.objects[&me].player().unwrap().companions[0].bonuses,
        vec![bonus]
    );
    let p = world.objects.get_mut(&me).unwrap().player_mut().unwrap();
    p.companions[0].level = 5;
    world.companion_store(me);
    world.companion_retrieve(me, 1);
    let bonuses = &world.objects[&me].player().unwrap().companions[0].bonuses;
    assert_eq!(bonuses.len(), 2);
    assert!(bonuses.iter().any(|(level, _, _)| *level == 5));
}

#[test]
fn companion_elixir_rerolls_only_an_armed_learned_skill() {
    use crate::world::StoredCompanion;

    let Some(mut world) = world() else { return };
    for (item, level) in [
        (767, 3),
        (768, 5),
        (769, 7),
        (770, 10),
        (1094, 11),
        (1095, 13),
        (1096, 15),
    ] {
        let def = &world.data.items[&item];
        assert_eq!(
            (def.item_type, def.shape, def.required_amount),
            (mir_proto::item_type::CONSUMABLE, 16, level)
        );
    }
    let offer = world
        .data
        .companions
        .iter()
        .find(|offer| world.data.monsters.contains_key(&offer.monster))
        .unwrap()
        .clone();
    let me = world
        .add_player(1, 1, &test_character("RegretPet"))
        .unwrap();
    let old = (3, mir_formats::mirdb::stat::HEALTH, 1);
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions
        .push(StoredCompanion {
            index: 1,
            info: offer.index,
            name: "Rex".into(),
            level: 3,
            experience: 0,
            hunger: 100,
            items: Vec::new(),
            food: None,
            bag: None,
            bonuses: vec![old],
        });
    world.test_give_item(me, 767, 1);
    let slot = world.test_slot_of(me, 767).unwrap();
    world.item_use(me, slot);
    assert!(world.test_slot_of(me, 767).is_some());
    assert!(world.outgoing.iter().any(|event| matches!(event,
        Outgoing::To(1, ServerMessage::Chat { text }) if text == "No companion is out"
    )));
    world.chat(me, "@EnableLevel3".into());
    assert_eq!(
        world.objects[&me]
            .player()
            .unwrap()
            .companion_reroll_enabled,
        1
    );
    let page = world.test_page_with_dialog_type(5);
    world.test_open_page(me, page);
    world.companion_retrieve(me, 1);
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions[0]
        .bonuses
        .clear();
    world.item_use(me, slot);
    assert!(world.test_slot_of(me, 767).is_some());
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions[0]
        .bonuses
        .push(old);
    world.chat(me, "@EnableLevel3".into());
    world.item_use(me, slot);
    assert!(world.outgoing.iter().any(|event| matches!(event,
        Outgoing::To(1, ServerMessage::Chat { text }) if text.contains("@EnableLevel3")
    )));
    assert!(world.test_slot_of(me, 767).is_some());
    world.chat(me, "@EnableLevel3".into());
    let before_hp = world.objects[&me].max_hp;

    world.data.items.get_mut(&767).unwrap().required_amount = 4;
    world.item_use(me, slot);
    assert!(world.test_slot_of(me, 767).is_some());
    world.data.items.get_mut(&767).unwrap().required_amount = 3;
    world.data.items.get_mut(&767).unwrap().shape = 99;
    world.item_use(me, slot);
    assert!(world.test_slot_of(me, 767).is_some());
    world.data.items.get_mut(&767).unwrap().shape = 16;
    let skills = world.data.companion_skills.clone();
    world.data.companion_skills.retain(|skill| skill.level != 3);
    let mut only = skills
        .iter()
        .find(|skill| skill.level == 3 && skill.weight > 0)
        .unwrap()
        .clone();
    only.stat = old.1;
    only.max_amount = 1;
    world.data.companion_skills.push(only.clone());
    world.item_use(me, slot);
    assert!(world.test_slot_of(me, 767).is_some());
    assert_eq!(
        world.objects[&me].player().unwrap().companions[0].bonuses[0],
        old
    );
    only.max_amount = 2;
    world.data.companion_skills.pop();
    world.data.companion_skills.push(only);

    world.item_use(me, slot);
    let p = world.objects[&me].player().unwrap();
    let new = p.companions[0].bonuses[0];
    assert_eq!(new, (3, mir_formats::mirdb::stat::HEALTH, 2));
    assert_eq!(world.objects[&me].max_hp, before_hp + 1);
    assert!(p.bag.inventory[slot as usize].is_none());
    let restored: StoredCompanion =
        serde_json::from_str(&serde_json::to_string(&p.companions[0]).unwrap()).unwrap();
    assert_eq!(restored.bonuses, vec![new]);
}

#[test]
fn companion_food_slot_auto_feeds_and_hunger_pauses_experience() {
    use crate::world::StoredCompanion;

    let Some(mut world) = world() else { return };
    let offer = world
        .data
        .companions
        .iter()
        .find(|offer| world.data.monsters.contains_key(&offer.monster))
        .unwrap()
        .clone();
    let food = world
        .data
        .items
        .values()
        .find(|def| def.item_type == mir_proto::item_type::COMPANION_FOOD && def.stat(86) > 0)
        .unwrap()
        .index;
    let me = world
        .add_player(1, 1, &test_character("HungryPet"))
        .unwrap();
    world.test_give_item(me, food, 3);
    let slot = world.test_slot_of(me, food).unwrap();
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions
        .push(StoredCompanion {
            index: 1,
            info: offer.index,
            name: "Rex".into(),
            level: 1,
            experience: 0,
            hunger: 1,
            items: Vec::new(),
            food: None,
            bag: None,
            bonuses: Vec::new(),
        });
    let page = world.test_page_with_dialog_type(5);
    world.test_open_page(me, page);
    world.companion_retrieve(me, 1);
    let requirement = {
        let def = world.data.items.get(&food).unwrap();
        (def.required_type, def.required_amount)
    };
    let def = world.data.items.get_mut(&food).unwrap();
    def.required_type = 11; // CompanionLevel
    def.required_amount = 2;
    world.companion_food_equip(me, slot);
    assert!(world.objects[&me].player().unwrap().companions[0]
        .food
        .is_none());
    let def = world.data.items.get_mut(&food).unwrap();
    def.required_type = 12; // MaxCompanionLevel
    def.required_amount = 0;
    world.companion_food_equip(me, slot);
    assert!(world.objects[&me].player().unwrap().companions[0]
        .food
        .is_none());
    let def = world.data.items.get_mut(&food).unwrap();
    (def.required_type, def.required_amount) = requirement;
    world.companion_food_equip(me, slot);
    assert!(world.objects[&me].player().unwrap().bag.inventory[slot as usize].is_none());
    assert_eq!(
        world.objects[&me].player().unwrap().companions[0]
            .food
            .as_ref()
            .unwrap()
            .count,
        3
    );
    let saved = serde_json::to_string(&world.objects[&me].player().unwrap().companions[0]).unwrap();
    let restored: StoredCompanion = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.food.unwrap().count, 3);
    world.companion_release(me, 1);
    assert_eq!(world.objects[&me].player().unwrap().companions.len(), 1);

    world.tick(60_000);
    let c = &world.objects[&me].player().unwrap().companions[0];
    assert!(c.hunger > 0);
    assert_eq!(c.experience, 1);
    assert_eq!(c.food.as_ref().unwrap().count, 2);

    world.companion_food_take(me);
    let p = world.objects.get_mut(&me).unwrap().player_mut().unwrap();
    assert!(p.companions[0].food.is_none());
    p.companions[0].hunger = 1;
    world.tick(120_000);
    let exp = world.objects[&me].player().unwrap().companions[0].experience;
    assert_eq!(world.objects[&me].player().unwrap().companions[0].hunger, 0);
    world.tick(180_000);
    assert_eq!(
        world.objects[&me].player().unwrap().companions[0].experience,
        exp
    );

    let slot = world.test_slot_of(me, food).unwrap();
    world.item_use(me, slot);
    assert!(world.objects[&me].player().unwrap().companions[0].hunger > 0);
    world.tick(240_000);
    assert!(world.objects[&me].player().unwrap().companions[0].experience > exp);
}

#[test]
fn companion_bonus_stats_change_owner_and_collect_drops() {
    use crate::world::StoredCompanion;

    let Some(mut world) = world() else { return };
    let offer = world
        .data
        .companions
        .iter()
        .find(|offer| world.data.monsters.contains_key(&offer.monster))
        .unwrap()
        .clone();
    let me = world.add_player(1, 1, &test_character("BonusPet")).unwrap();
    let base_hp = world.objects[&me].max_hp;
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions
        .push(StoredCompanion {
            index: 1,
            info: offer.index,
            name: "Rex".into(),
            level: 3,
            experience: 0,
            hunger: 100,
            items: Vec::new(),
            food: None,
            bag: None,
            bonuses: vec![(3, 54, 10)],
        });
    let page = world.test_page_with_dialog_type(5);
    world.test_open_page(me, page);
    world.companion_retrieve(me, 1);
    assert_eq!(world.objects[&me].max_hp, base_hp + base_hp / 10);

    for (stat, amount) in [
        (34, 4),
        (35, 5),
        (37, 25),
        (55, 9),
        (115, 50),
        (19, 1),
        (122, 200),
        (123, 200),
        (124, 200),
        (125, 7),
        (126, 7),
        (76, 1),
    ] {
        world.companion_store(me);
        world
            .objects
            .get_mut(&me)
            .unwrap()
            .player_mut()
            .unwrap()
            .companions[0]
            .bonuses = vec![(3, stat, amount)];
        world.companion_retrieve(me, 1);
        let o = &world.objects[&me];
        let p = o.player().unwrap();
        match stat {
            34 => assert_eq!(p.comfort, 4),
            35 => assert_eq!(p.life_steal, 5),
            37 => assert_eq!(p.drop_rate, 25),
            55 => assert_eq!(o.stats.critical_chance, 10),
            115 => assert_eq!(o.stats.critical_damage, 50),
            19 => assert_eq!(o.stats.luck, 1),
            122 => assert_eq!(o.stats.paralysis_chance, 200),
            123 => assert_eq!(o.stats.slow_chance, 200),
            124 => assert_eq!(o.stats.silence_chance, 200),
            125 => assert_eq!(o.stats.block_chance, 7),
            126 => assert_eq!(o.stats.evasion_chance, 7),
            76 => assert!(p.companion_collection),
            _ => unreachable!(),
        }
    }
    let potion = world
        .data
        .items
        .values()
        .find(|item| item.name == "Healing Potion")
        .unwrap()
        .index;
    let item = crate::items::UserItem::fresh(&world.data, potion, 1).unwrap();
    assert!(world.companion_auto_collect(me, &item));
    assert_eq!(
        world.objects[&me].player().unwrap().companions[0].items[0].info,
        potion
    );

    world.companion_store(me);
    assert!(!world.companion_auto_collect(me, &item));
}

#[test]
fn companion_food_requires_active_hungry_companion_then_consumes_one() {
    use crate::world::StoredCompanion;
    const COMPANION_HUNGER_STAT: i32 = 86;
    let Some(mut world) = world() else { return };
    let me = world.add_player(1, 1, &test_character("Feeder")).unwrap();
    let food = world
        .data
        .items
        .values()
        .find(|d| {
            d.item_type == mir_proto::item_type::COMPANION_FOOD && d.stat(COMPANION_HUNGER_STAT) > 0
        })
        .expect("companion food")
        .clone();
    world.test_give_item(me, food.index, 2);
    let slot = world.test_slot_of(me, food.index).unwrap();
    drain(&mut world);

    world.item_use(me, slot);
    assert!(drain(&mut world)
        .iter()
        .any(|m| matches!(m, ServerMessage::Chat { text } if text == "No companion is out")));
    assert_eq!(
        world.objects[&me].player().unwrap().bag.inventory[slot as usize]
            .as_ref()
            .unwrap()
            .count,
        2
    );

    let max_hunger = world
        .data
        .companion_levels
        .iter()
        .filter(|l| l.level <= 1)
        .max_by_key(|l| l.level)
        .map(|l| l.max_hunger)
        .unwrap_or(100);
    let p = world.objects.get_mut(&me).unwrap().player_mut().unwrap();
    p.active_companion = Some(1);
    p.companions.push(StoredCompanion {
        index: 1,
        info: 0,
        name: "Rex".into(),
        level: 1,
        experience: 0,
        hunger: 0,
        items: Vec::new(),
        food: None,
        bag: None,
        bonuses: Vec::new(),
    });
    world.item_use(me, slot);
    let p = world.objects[&me].player().unwrap();
    assert_eq!(
        p.companions[0].hunger,
        food.stat(COMPANION_HUNGER_STAT).min(max_hunger)
    );
    assert_eq!(p.bag.inventory[slot as usize].as_ref().unwrap().count, 1);
    assert!(drain(&mut world)
        .iter()
        .any(|m| matches!(m, ServerMessage::Companions(_))));

    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions[0]
        .hunger = max_hunger;
    world.item_use(me, slot);
    assert_eq!(
        world.objects[&me].player().unwrap().bag.inventory[slot as usize]
            .as_ref()
            .unwrap()
            .count,
        1
    );
}

#[test]
fn wave_six_frost_bite_chain_hurricane_and_corpses() {
    use mir_proto::buff_type;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let mut rec = test_character("Wiz");
    rec.class = mir_proto::Class::Wizard;
    rec.level = 90;
    let me = world.add_player(1, 1, &rec).unwrap();
    // An assassin for Chain and a taoist for Corpse Exploder, standing with
    // the wizard (assassins start elsewhere).
    let mut sin_rec = test_character("Sin");
    sin_rec.class = mir_proto::Class::Assassin;
    sin_rec.level = 90;
    sin_rec.id = 2;
    let sin = world.add_player(2, 2, &sin_rec).unwrap();
    let mut tao_rec = test_character("Tao");
    tao_rec.class = mir_proto::Class::Taoist;
    tao_rec.level = 90;
    tao_rec.id = 3;
    let tao = world.add_player(3, 3, &tao_rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let map = world.objects[&me].map;
    let loc = world.objects[&me].location;
    let mut spots = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 1))
        .filter(|p| world.maps[&map].file.is_walkable(p.x, p.y));
    let sin_loc = spots.next().unwrap();
    let tao_loc = spots.next().unwrap();
    world.test_change_map(sin, map, sin_loc);
    world.test_change_map(tao, map, tao_loc);
    let frost = learn(&mut world, me, "Frost Bite");
    let hurricane = learn(&mut world, me, "Elemental Hurricane");
    let chain = learn(&mut world, sin, "Chain");
    let exploder = learn(&mut world, tao, "Corpse Exploder");

    // Frost Bite banks damage taken and bursts it on monsters within 3.
    world.test_refill_mp(me);
    world.cast(me, frost, Direction::Down, None, loc);
    world.tick(600);
    assert!(world.objects[&me].has_buff(buff_type::FROST_BITE));
    let chicken = nearest_chicken(&world, me);
    let near = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 2))
        .find(|p| {
            world.maps[&map].file.is_walkable(p.x, p.y)
                && world.maps[&map].objects_at(*p).is_empty()
        })
        .unwrap();
    world.teleport(chicken, near);
    let before = world.objects[&chicken].hp;
    world.test_damage(me, chicken, 20);
    // Let the buff run out: 3 + 3L seconds at level 0 is 3 s.
    world.tick(600 + 3_100);
    world.tick(600 + 3_200);
    assert!(!world.objects[&me].has_buff(buff_type::FROST_BITE));
    assert!(world.objects[&chicken].hp < before || world.objects[&chicken].dead);

    // Chain tethers monsters near the target for the spell's power.
    let now = 10_000;
    world.tick(now);
    let leader = nearest_chicken(&world, me);
    world.teleport(leader, near);
    let second = world
        .objects
        .values()
        .find(|o| {
            !o.dead
                && o.id != leader
                && matches!(&o.appearance, Appearance::Monster { name, .. } if name == "Chicken")
        })
        .map(|o| o.id)
        .expect("a second chicken");
    let beside = Direction::ALL
        .iter()
        .map(|d| near.step(*d, 1))
        .find(|p| {
            world.maps[&map].file.is_walkable(p.x, p.y)
                && world.maps[&map].objects_at(*p).is_empty()
        })
        .unwrap();
    world.teleport(second, beside);
    world.test_refill_mp(sin);
    let sin_dir = Direction::from_points(sin_loc, near);
    world.cast(sin, chain, sin_dir, Some(leader), near);
    world.tick(now + 1500);
    assert_eq!(world.test_monster_chained(second), Some(leader));

    // Elemental Hurricane ticks along the facing line and stops on a turn.
    let dir = Direction::from_points(loc, near);
    world.test_refill_mp(me);
    let hp = world.objects[&leader].hp;
    world.cast(me, hurricane, dir, None, loc);
    assert!(world.test_channelling(me));
    world.tick(now + 2100);
    world.tick(now + 2700);
    assert!(world.objects[&leader].hp < hp || world.objects[&leader].dead);
    world.player_turn(me, dir.opposite());
    assert!(!world.test_channelling(me));

    // Corpse Exploder needs a corpse target and two amulets.
    let amulet = world
        .data
        .items
        .values()
        .find(|d| {
            d.item_type == mir_proto::item_type::AMULET && d.shape == 0 && d.required_amount <= 13
        })
        .map(|d| d.index)
        .expect("an amulet");
    world.test_give_item(tao, amulet, 20);
    let slot = world.test_slot_of(tao, amulet).unwrap();
    world.item_move(
        tao,
        mir_proto::Grid::Inventory,
        slot,
        mir_proto::Grid::Equipment,
        mir_proto::slot::AMULET as u8,
    );
    world.test_set_target(leader, me);
    world.test_damage(leader, me, 100_000);
    assert!(world.objects[&leader].dead);
    world.tick(now + 3000);
    world.test_refill_mp(tao);
    drain(&mut world);
    world.cast(
        tao,
        exploder,
        Direction::from_points(tao_loc, near),
        Some(leader),
        near,
    );
    world.tick(now + 4600);
    assert!(
        !world.objects.contains_key(&leader),
        "the corpse was consumed"
    );
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::ObjectMagic { magic, .. } if *magic == exploder)));
}

/// Developer report: monsters per AI id and whether the AI has its own
/// profile (`cargo test -p mir-server report_monster_ai -- --ignored --nocapture`).
#[test]
#[ignore]
fn report_monster_ai_coverage() {
    let Some(world) = world() else {
        return;
    };
    let base = format!("{:?}", world::ai_profile::profile(15));
    let mut per_ai: std::collections::BTreeMap<i32, (usize, usize, Vec<String>)> =
        std::collections::BTreeMap::new();
    for m in world.data.monsters.values() {
        let e = per_ai.entry(m.ai).or_default();
        e.0 += 1;
        if e.2.len() < 4 {
            e.2.push(m.name.clone());
        }
    }
    for s in &world.data.respawns {
        if let Some(m) = world.data.monsters.get(&s.monster) {
            per_ai.entry(m.ai).or_default().1 += 1;
        }
    }
    for (ai, (defs, spawns, names)) in per_ai {
        let own = format!("{:?}", world::ai_profile::profile(ai)) != base;
        eprintln!(
            "AI {ai:4}: {defs:3} monsters, {spawns:3} spawns, profile {} - {}",
            if own { "own    " } else { "DEFAULT" },
            names.join(", ")
        );
    }
}

#[test]
fn monster_ai_wave_two_ghosts_curses_clouds_and_gates() {
    use mir_proto::spell_effect;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let mut rec = test_character("Tester");
    rec.level = 40;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let map = world.objects[&me].map;
    // Away from the town guards, who kill wild monsters on sight.
    let start = world.objects[&me].location;
    let loc = world
        .test_quiet_cell(map, start)
        .expect("a quiet cell outside town");
    world.teleport(me, loc);
    let mut free = Direction::ALL.iter().map(|d| loc.step(*d, 1)).filter(|p| {
        world.maps[&map].file.is_walkable(p.x, p.y) && world.maps[&map].objects_at(*p).is_empty()
    });
    let a = free.next().unwrap();
    let b = free.next().unwrap();

    // Voracious ghost: dies, gets up 3-7 s later with half its HP, and only
    // stays dead once its revives are spent.
    let ghost = world.test_spawn_ai(11, map, a).expect("a voracious ghost");
    world.test_set_revives(ghost, 1);
    let max_hp = world.objects[&ghost].max_hp;
    world.test_damage(ghost, me, 1_000_000);
    assert!(world.objects[&ghost].dead);
    assert_eq!(world.test_monster_revives(ghost), (1, 1));
    for t in 1..=9 {
        world.tick(t * 1000);
    }
    assert!(!world.objects[&ghost].dead, "the ghost should have revived");
    assert!(world.objects[&ghost].hp <= max_hp / 2 + 1);
    assert_eq!(world.test_monster_revives(ghost), (0, 1));
    world.test_damage(ghost, me, 1_000_000);
    for t in 10..=18 {
        world.tick(t * 1000);
    }
    assert!(world.objects[&ghost].dead);
    world.remove_object(ghost);

    // Crimson Necromancer: every 10 s targets within 3 lose their magic
    // resistance for 10 s.
    let necro = world.test_spawn_ai(59, map, a).expect("a necromancer");
    world.test_set_target(necro, me);
    for t in 19..=22 {
        world.tick(t * 1000);
    }
    assert!(world.objects[&me]
        .poisons
        .iter()
        .any(|p| p.kind == world::poison_kind::MAGIC_WEAKNESS));
    world.remove_object(necro);

    // Jinchon Devil: death clouds around each target in view.
    let devil = world.test_spawn_ai(78, map, b).expect("a jinchon devil");
    // Above half HP it skips each target half the time; wound it first.
    let max_hp = world.objects[&devil].max_hp;
    world.test_set_hp(devil, max_hp / 3);
    world.test_set_target(devil, me);
    for t in 23..=26 {
        world.tick(t * 1000);
    }
    assert!(world.test_spell_at(map, loc, spell_effect::DEATH_CLOUD));
    world.remove_object(devil);

    // Gates cannot be hurt and vanish after twenty minutes.
    let gate = world.test_spawn_ai(30, map, a).expect("a gate");
    assert_eq!(world.test_damage(gate, me, 500), 0);
    assert!(world.test_monster_despawn_at(gate).is_some());
}

#[test]
fn npc_weapon_checks_read_refine_level_element_and_added_stats() {
    use crate::data::NpcCheckDef;
    use mir_proto::Grid;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Smith")).unwrap();
    world.tick(0);
    drain(&mut world);
    let check = |t: i32, op: i32, i1: i32, i2: i32, stat1: i32| NpcCheckDef {
        check_type: t,
        operator: op,
        string1: String::new(),
        int1: i1,
        int2: i2,
        item1: 0,
        stat1,
        fail_page: 0,
    };
    let weapon = world
        .data
        .items
        .values()
        .filter(|d| d.item_type == mir_proto::item_type::WEAPON && d.required_amount <= 1)
        .filter(|d| {
            crate::items::can_use(d, mir_proto::Class::Warrior, mir_proto::Gender::Male, 1).is_ok()
                && (0..7).all(|i| d.stat(world::refine::FIRE_ATTACK_STAT + i * 2) == 0)
        })
        .min_by_key(|d| d.index)
        .map(|d| d.index)
        .expect("a plain starter weapon");
    world.test_give_item(me, weapon, 1);
    let wslot = world.test_slot_of(me, weapon).unwrap();
    world.item_move(me, Grid::Inventory, wslot, Grid::Equipment, 0);
    // Fresh weapon: level 0, not ready until combat, no added stats.
    assert!(world.test_npc_check(me, &check(7, 0, 0, 0, 0)));
    assert!(!world.test_npc_check(me, &check(7, 5, 3, 0, 0)));
    assert!(!world.test_npc_check(me, &check(9, 0, 0, 0, 0)));
    world.gain_experience_test(me, 1);
    assert!(world.test_npc_check(me, &check(9, 0, 0, 0, 0)));
    assert!(world.test_npc_check(me, &check(8, 0, 0, 0, 0)));
    // Three refines, a fire element of 5 and +3 added DC (stat 2).
    let fire = world::refine::FIRE_ATTACK_STAT;
    world.test_set_weapon_refine(
        me,
        3,
        vec![(world::refine::WEAPON_ELEMENT_STAT, 1), (fire, 5), (2, 3)],
    );
    assert!(world.test_npc_check(me, &check(7, 5, 3, 0, 0)));
    assert!(
        world.test_npc_check(me, &check(8, 5, 0, 5, 0)),
        "any element >= 5"
    );
    assert!(world.test_npc_check(me, &check(8, 0, 1, 5, 0)), "fire == 5");
    assert!(world.test_npc_check(me, &check(8, 0, 2, 0, 0)), "ice == 0");
    assert!(
        world.test_npc_check(me, &check(16, 0, 3, 0, 2)),
        "added DC == 3"
    );
    assert!(!world.test_npc_check(me, &check(16, 4, 3, 0, 2)));
}

/// Developer report: which NPCs open refine / companion / marriage pages
/// (`cargo test -p mir-server report_npc_pages -- --ignored --nocapture`).
#[test]
#[ignore]
fn report_npc_dialog_pages() {
    let Some(world) = world() else {
        return;
    };
    for n in &world.data.npcs {
        let Some(p) = world.data.npc_pages.get(&n.entry_page) else {
            continue;
        };
        if p.dialog_type != 0 {
            eprintln!(
                "NPC {:?} entry page {} dialog type {}",
                n.name, n.entry_page, p.dialog_type
            );
        }
    }
    for p in world.data.npc_pages.values() {
        if p.dialog_type != 0 {
            eprintln!("page {} dialog type {}", p.index, p.dialog_type);
        }
    }
}

/// Developer report: which NPC reaches a refine (3) or companion (5) page
/// through its buttons.
#[test]
#[ignore]
fn report_npc_reaching_pages() {
    let Some(world) = world() else {
        return;
    };
    let wanted = [3, 4, 5, 6];
    for n in &world.data.npcs {
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![n.entry_page];
        while let Some(pg) = stack.pop() {
            if !seen.insert(pg) {
                continue;
            }
            let Some(p) = world.data.npc_pages.get(&pg) else {
                continue;
            };
            if wanted.contains(&p.dialog_type) {
                eprintln!(
                    "NPC {:?} reaches page {} dialog type {}",
                    n.name, pg, p.dialog_type
                );
            }
            for (_, target) in &p.buttons {
                stack.push(*target);
            }
            stack.push(p.success_page);
        }
    }
}

#[test]
#[ignore]
fn report_npc_positions() {
    let Some(mut world) = world() else {
        return;
    };
    let me = world.add_player(1, 1, &test_character("Scout")).unwrap();
    let (map, loc) = (world.objects[&me].map, world.objects[&me].location);
    for o in world.on_map(map) {
        if let Appearance::Npc { name, .. } = &o.appearance {
            eprintln!(
                "NPC {name} at {:?} distance {}",
                o.location,
                o.location.distance(loc)
            );
        }
    }
    eprintln!("start {:?} on map {map}", loc);
}

#[test]
#[ignore]
fn report_refine_npc_locations() {
    let Some(mut world) = world() else {
        return;
    };
    for n in world.data.npcs.clone() {
        if ["Jack", "Maddox", "Piersym"].contains(&n.name.as_str()) {
            let Some(r) = world.data.regions.get(&n.region).cloned() else {
                continue;
            };
            let _ = world.ensure_map(r.map);
            let w = world.maps[&r.map].file.width as i32;
            let file = world
                .data
                .maps
                .get(&r.map)
                .map(|m| m.file_name.clone())
                .unwrap_or_default();
            eprintln!(
                "NPC {} map {} ({file}) cells {:?}",
                n.name,
                r.map,
                r.points(w).first()
            );
        }
    }
}

#[test]
#[ignore]
fn report_buttons_to_refine_pages() {
    let Some(world) = world() else {
        return;
    };
    for p in world.data.npc_pages.values() {
        for (button, target) in &p.buttons {
            if [85, 97, 149].contains(target) {
                eprintln!("page {} button {button} -> page {target}", p.index);
            }
        }
    }
}

#[test]
fn monster_ai_wave_three_shinsu_terracotta_and_doom_claw() {
    use mir_proto::magic_type;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let mut rec = test_character("Tester");
    rec.level = 40;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let map = world.objects[&me].map;
    let start = world.objects[&me].location;
    let loc = world
        .test_quiet_cell(map, start)
        .expect("a quiet cell outside town");
    world.teleport(me, loc);
    let cell_at = |world: &World, d: Direction, n: i32| -> Option<Point> {
        let p = loc.step(d, n);
        (world.maps[&map].file.is_walkable(p.x, p.y) && world.maps[&map].objects_at(p).is_empty())
            .then_some(p)
    };

    // Terracotta: invisible at spawn, walks in unseen and shows itself
    // within two cells of its target.
    let far = Direction::ALL
        .iter()
        .find_map(|d| cell_at(&world, *d, 6))
        .expect("a cell six away");
    let terra = world.test_spawn_ai(133, map, far).expect("a terracotta");
    assert!(world.test_monster_hidden_mode(terra).0);
    world.test_set_target(terra, me);
    for t in 1..=60 {
        world.tick(1000 + t * 50);
    }
    assert!(
        !world.test_monster_hidden_mode(terra).0,
        "terracotta should have shown itself"
    );
    assert!(world.objects[&terra].location.distance(loc) <= 2);
    world.remove_object(terra);

    // Shinsu appears in small form one second after spawn. A target opens
    // the 10 s big-form fighting window; idle small form stays visible.
    let near = Direction::ALL
        .iter()
        .find_map(|d| cell_at(&world, *d, 1))
        .expect("an adjacent cell");
    let shinsu = world.test_spawn_ai(53, map, near).expect("a shinsu");
    assert!(world.test_monster_hidden_mode(shinsu).0);
    drain(&mut world);
    world.tick(5500);
    assert_eq!(world.test_monster_hidden_mode(shinsu), (false, false));
    assert!(
        drain(&mut world)
            .iter()
            .any(|m| { matches!(m, ServerMessage::ObjectShow(state) if state.id == shinsu) }),
        "the small Shinsu should become visible to the player"
    );
    assert!(matches!(
        world.objects[&shinsu].appearance,
        Appearance::Monster { image: 100, .. }
    ));
    world.test_set_target(shinsu, me);
    for t in 0..8 {
        world.tick(10_000 + t * 500);
    }
    assert_eq!(world.test_monster_hidden_mode(shinsu), (false, true));
    assert!(matches!(
        world.objects[&shinsu].appearance,
        Appearance::Monster { image: 99, .. }
    ));
    // Out of view range (18) but still within the 20-cell tick radius: the
    // target lapses and the window closes 10 s later.
    let sloc = world.objects[&shinsu].location;
    let away = (0..8)
        .map(Direction::from_index)
        .find_map(|d| {
            let p = sloc.step(d, 19);
            world.maps[&map].file.is_walkable(p.x, p.y).then_some(p)
        })
        .expect("a cell 19 away");
    world.teleport(me, away);
    for t in 0..60 {
        world.tick(15_000 + t * 500);
    }
    assert!(
        !world.test_monster_hidden_mode(shinsu).1,
        "window should have closed"
    );
    assert_eq!(world.test_monster_hidden_mode(shinsu), (false, false));
    assert!(matches!(
        world.objects[&shinsu].appearance,
        Appearance::Monster { image: 100, .. }
    ));
    world.remove_object(shinsu);
    world.teleport(me, loc);

    // Doom Claw: stands still, strikes the zone the player stands in, and
    // ignores attackers further than ten cells.
    let claw = world
        .test_spawn_ai(120, map, loc.step(Direction::UpLeft, 5))
        .expect("a doom claw");
    let hp = world.objects[&me].hp;
    drain(&mut world);
    for t in 0..20 {
        world.tick(40_000 + t * 300);
    }
    let msgs = drain(&mut world);
    // A pinch, swipe or spit lands within 400 ms; the rarer wave shoves the
    // player 15 cells down-right (out of the claw's view, so its cast
    // message may not reach them).
    let struck = msgs.iter().any(|m| matches!(m,
        ServerMessage::ObjectMagic { id, magic, .. }
            if *id == claw && (magic_type::DOOM_CLAW_LEFT_PINCH..=magic_type::DOOM_CLAW_SPIT).contains(magic)));
    let hurt = world.objects[&me].hp < hp || world.objects[&me].dead;
    let shoved = world.objects[&me].location != loc;
    assert!(struck || hurt || shoved, "the claw never struck");
    assert!(hurt || shoved, "the claw's strike had no effect");
    world.teleport(me, away);
    assert_eq!(world.test_damage(claw, me, 500), 0);
    let close = Direction::ALL
        .iter()
        .map(|d| world.objects[&claw].location.step(*d, 2))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .expect("a cell near the claw");
    world.teleport(me, close);
    assert!(world.test_damage(claw, me, 500) > 0);
}

#[test]
fn monster_ai_wave_four_fields_purify_healer_and_behaviours() {
    use mir_proto::{buff_type, spell_effect};
    use world::ai_profile::Spell;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let mut rec = test_character("Tester");
    rec.level = 60;
    let me = world.add_player(1, 1, &rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let map = world.objects[&me].map;
    let start = world.objects[&me].location;
    let loc = world
        .test_quiet_cell(map, start)
        .expect("a quiet cell outside town");
    world.teleport(me, loc);
    let cell_at = |world: &World, d: Direction, n: i32| -> Option<Point> {
        let p = loc.step(d, n);
        (world.maps[&map].file.is_walkable(p.x, p.y) && world.maps[&map].objects_at(p).is_empty())
            .then_some(p)
    };

    // A monster fire wall is a real field: five walls on the target that
    // burn the player standing in them every 2 s.
    let spot = Direction::ALL
        .iter()
        .find_map(|d| cell_at(&world, *d, 3))
        .expect("a cell three away");
    let caster = world.test_spawn_ai(0, map, spot).expect("a plain monster");
    world.test_set_target(caster, me);
    world.test_monster_cast(caster, me, Spell::FireWall);
    assert!(world.test_spell_at(map, loc, spell_effect::FIRE_WALL));
    let hp = world.objects[&me].hp;
    for t in 1..=6 {
        world.tick(t * 1000);
    }
    assert!(world.objects[&me].hp < hp, "the wall should burn");
    // A poisonous cloud lingers as a 5x5 field around the caster.
    world.test_monster_cast(caster, me, Spell::PoisonousCloud);
    assert!(world.test_spell_at(map, spot, spell_effect::POISONOUS_CLOUD));
    world.remove_object(caster);
    world.test_set_hp(me, world.objects[&me].max_hp);

    // Purification strips a player's helpful buffs.
    let might = learn(&mut world, me, "Might");
    world.test_refill_mp(me);
    drain(&mut world);
    world.cast(me, might, Direction::Down, None, loc);
    world.tick(7_000);
    assert!(world.objects[&me].has_buff(buff_type::MIGHT));
    let captain = world.test_spawn_ai(72, map, spot).expect("a banyo captain");
    world.test_purify(captain, me);
    assert!(!world.objects[&me].has_buff(buff_type::MIGHT));
    world.remove_object(captain);

    // A healer ant tends an injured ally with a ticking heal.
    let ally_spot = Direction::ALL
        .iter()
        .find_map(|d| cell_at(&world, *d, 4))
        .expect("a cell four away");
    let ally = world.test_spawn_ai(0, map, ally_spot).expect("an ally");
    let max_hp = world.objects[&ally].max_hp;
    world.test_set_hp(ally, max_hp / 2);
    let ant = world.test_spawn_ai(12, map, spot).expect("a healer ant");
    for t in 8..=12 {
        world.tick(t * 1000);
    }
    assert!(
        world.test_has_heal(ally) || world.objects[&ally].hp > max_hp / 2,
        "the ant should heal its ally"
    );
    world.remove_object(ant);
    world.remove_object(ally);

    // Behaviour flags: at a quarter HP the monster jumps away and enrages,
    // and it heals a tenth every 30 s while hurt.
    let brute = world.test_spawn_ai(0, map, spot).expect("a brute");
    world.test_set_behaviours(brute, 4 | 8 | 64);
    let delay = world.test_monster_attack_delay(brute);
    let max_hp = world.objects[&brute].max_hp;
    world.test_set_hp(brute, max_hp / 5);
    world.tick(13_000);
    assert!(
        world.objects[&brute].location.distance(spot) >= 5,
        "should have jumped away"
    );
    assert!(
        world.test_monster_attack_delay(brute) < delay,
        "should have enraged"
    );
    assert!(
        world.objects[&brute].hp > max_hp / 5,
        "should have healed a tenth"
    );
    world.test_set_behaviours(brute, 0);
}

#[test]
fn fame_titles_are_bought_with_fame_points_and_buff_stats() {
    use crate::data::{NpcActionDef, NpcCheckDef};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Famous")).unwrap();
    world.tick(0);
    drain(&mut world);
    let check = NpcCheckDef {
        check_type: 21,
        operator: 0,
        string1: String::new(),
        int1: 0,
        int2: 0,
        item1: 0,
        stat1: 0,
        fail_page: 0,
    };
    let promote = NpcActionDef {
        action_type: 22,
        string1: String::new(),
        int1: 0,
        int2: 0,
        item1: 0,
        map1: 0,
        stat1: 0,
    };
    let first = world
        .data
        .fames
        .first()
        .cloned()
        .expect("fame titles in the pack");
    // Without Fame Points the check fails and the promotion is refused.
    assert!(!world.test_npc_check(me, &check));
    world.test_npc_action(me, &promote);
    assert_eq!(world.test_fame(me), 0);
    // With enough points the title is bought, its stats apply and the
    // character sheet names it.
    let dc_before = world.objects[&me].stats.max_dc;
    world.test_set_currency_by_type(me, world::fame::CURRENCY_FP, first.cost as i64 + 5);
    assert!(world.test_npc_check(me, &check));
    drain(&mut world);
    world.test_npc_action(me, &promote);
    assert_eq!(world.test_fame(me), first.index);
    assert_eq!(world.test_currency_total(me), 5);
    let max_dc_bonus = first
        .stats
        .iter()
        .filter(|(s, _)| *s == mir_formats::mirdb::stat::MAX_DC)
        .map(|(_, a)| *a)
        .sum::<i32>();
    assert_eq!(world.objects[&me].stats.max_dc, dc_before + max_dc_bonus);
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(|m| matches!(m,
        ServerMessage::StatsChanged(s) if s.fame == first.index && s.fame_title == first.name)));
    // The next title costs more than what is left.
    assert!(!world.test_npc_check(me, &check));
}

#[test]
fn lua_page_scripts_run_checks_actions_and_on_open() {
    use crate::data::{NpcActionDef, NpcCheckDef};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Scripted")).unwrap();
    world.tick(0);
    drain(&mut world);
    let check = |func: &str| NpcCheckDef {
        check_type: 22,
        operator: 0,
        string1: func.into(),
        int1: 0,
        int2: 0,
        item1: 0,
        stat1: 0,
        fail_page: 0,
    };
    let action = |func: &str| NpcActionDef {
        action_type: 23,
        string1: func.into(),
        int1: 0,
        int2: 0,
        item1: 0,
        map1: 0,
        stat1: 0,
    };
    let potion = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .map(|d| d.index)
        .expect("healing potion");
    // No script on the page: checks pass (Zircon has no opinion).
    world.test_set_script(None);
    assert!(world.test_npc_check(me, &check("is_rich")));
    // The example script: is_rich needs 1000 gold, has_potion a potion.
    world.test_set_script(Some("example.lua"));
    world.test_set_gold(me, 10);
    assert!(!world.test_npc_check(me, &check("is_rich")));
    assert!(!world.test_npc_check(me, &check("has_potion")));
    // Unknown functions and missing files pass.
    assert!(world.test_npc_check(me, &check("no_such_function")));
    world.test_set_script(Some("missing.lua"));
    assert!(world.test_npc_check(me, &check("is_rich")));
    world.test_set_script(Some("example.lua"));
    world.test_set_gold(me, 1500);
    assert!(world.test_npc_check(me, &check("is_rich")));
    // reward: takes 1000 gold, gives two potions, says something.
    drain(&mut world);
    world.test_npc_action(me, &action("reward"));
    assert_eq!(world.test_gold(me), 500);
    assert!(world
        .test_bag(me)
        .1
        .iter()
        .any(|(info, count)| *info == potion && *count == 2));
    assert!(world.test_npc_check(me, &check("has_potion")));
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(|m| matches!(m,
        ServerMessage::Chat { text } if text == "Enjoy the potions, Scripted.")));
    // on_open cancels the page for level 99 characters.
    let page = world.test_page_with_dialog_type(0);
    world.test_set_page_script(page, "example.lua");
    world.test_run_page(me, page);
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::NpcResponse { .. })));
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .level = 99;
    world.test_run_page(me, page);
    let msgs = drain(&mut world);
    assert!(msgs.iter().any(|m| matches!(m, ServerMessage::NpcClose)));
    assert!(!msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::NpcResponse { .. })));
}

#[test]
#[ignore]
fn report_mine_maps() {
    let Some(mut world) = world() else {
        return;
    };
    let mines: Vec<(i32, String, String)> = world
        .data
        .maps
        .values()
        .filter(|m| m.can_mine)
        .map(|m| (m.index, m.file_name.clone(), m.description.clone()))
        .collect();
    for (index, file, desc) in mines {
        if world.ensure_map(index).is_err() {
            continue;
        }
        if let Some((cell, dir)) = world.test_wall_spot(index) {
            eprintln!(
                "MINE {desc:?} file {file} wall spot {:?} facing {:?}",
                cell, dir
            );
        }
    }
    let potion = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .map(|d| d.index);
    eprintln!("POTION {potion:?}");
}

/// Developer report: the castle collections of the asset pack.
#[test]
#[ignore]
fn report_castle_collections() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
        return;
    };
    let db = mir_formats::mirdb::MirDb::load(assets.join("../Database/System.db")).expect("db");
    for name in [
        "CastleInfo",
        "CastleGuardInfo",
        "CastleGateInfo",
        "CastleFlagInfo",
        "GuildWarInfo",
        "UserConquest",
    ] {
        match db.collection(name) {
            Some(c) => {
                let fields: Vec<&str> = c
                    .mapping
                    .properties
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect();
                eprintln!("{name}: {} records; fields {fields:?}", c.records.len());
                for r in c.records.iter().take(4) {
                    let row: Vec<String> = fields
                        .iter()
                        .map(|f| format!("{f}={:?}", c.get(r, f)))
                        .collect();
                    eprintln!("  {}", row.join(" "));
                }
            }
            None => eprintln!("{name}: absent"),
        }
    }
}

#[test]
fn guild_war_and_castle_conquest() {
    use mir_proto::{attack_mode, ChatKind};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let mut bob_rec = test_character("Bob");
    bob_rec.id = 2;
    let bob = world.add_player(2, 2, &bob_rec).unwrap();
    world.tick(0);
    drain(&mut world);
    let conn_of = |world: &World, id: ObjectId| world.objects[&id].player().unwrap().conn;
    let (ca, cb) = (conn_of(&world, alice), conn_of(&world, bob));
    let to = |world: &mut World| -> Vec<(world::ConnId, ServerMessage)> {
        world
            .outgoing
            .drain(..)
            .map(|o| match o {
                Outgoing::To(c, m) => (c, m),
            })
            .collect()
    };
    // Two guilds of one, standing outside the safe zone.
    world.test_set_gold(alice, 20_000_000);
    world.test_set_gold(bob, 20_000_000);
    world.guild_create(alice, "Knights".into(), 5);
    world.guild_create(bob, "Rogues".into(), 5);
    let knights = world.guild_store.guilds[0].id;
    let rogues = world.guild_store.guilds[1].id;
    let map = world.objects[&alice].map;
    let start = world.objects[&alice].location;
    let loc = world
        .test_quiet_cell(map, start)
        .expect("a quiet cell outside town");
    world.teleport(alice, loc);
    let next = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 1))
        .find(|p| {
            world.maps[&map].file.is_walkable(p.x, p.y) && !world.test_in_safe_zone_at(map, *p)
        })
        .expect("a neighbour cell");
    world.teleport(bob, next);
    world.set_attack_mode(alice, attack_mode::WAR_RED_BROWN);
    assert!(!world.objects[&alice].hostile_to(&world.objects[&bob]));
    drain(&mut world);

    // War needs funds; then both guilds hear it and become fair game in
    // War/Red/Brown mode without brown names or PK points.
    world.guild_war(alice, "Rogues".into());
    assert!(world.guild_store.wars.is_empty());
    world.test_set_guild_funds(knights, 500_000);
    world.guild_war(alice, "rogues".into());
    assert_eq!(world.guild_store.wars.len(), 1);
    assert_eq!(world.guild_store.guilds[0].funds, 300_000);
    let out = to(&mut world);
    assert!(out.iter().any(|(c, m)| *c == cb
        && matches!(m, ServerMessage::GuildWarStarted { guild, .. } if guild == "Knights")));
    assert!(out.iter().any(|(c, m)| *c == ca
        && matches!(m, ServerMessage::GuildInfo(Some(g)) if g.wars.len() == 1 && g.wars[0].0 == "Rogues")));
    assert!(world.objects[&alice].hostile_to(&world.objects[&bob]));
    world.test_damage(bob, alice, 5);
    assert!(!world.objects[&alice].player().unwrap().brown);
    world.test_damage(bob, alice, 1_000_000);
    assert!(world.objects[&bob].dead);
    assert_eq!(world.objects[&alice].player().unwrap().pk_points, 0);
    let out = to(&mut world);
    assert!(out.iter().any(|(_, m)| matches!(m,
        ServerMessage::Say { kind: ChatKind::System, text, .. }
            if text.contains("Bob of Rogues was killed by Alice of Knights"))));
    world.test_expire_wars();
    world.tick(2000);
    assert!(world.guild_store.wars.is_empty());
    let out = to(&mut world);
    assert!(out.iter().any(|(c, m)| *c == ca
        && matches!(m, ServerMessage::GuildWarFinished { guild } if guild == "Rogues")));
    assert!(!world.objects[&alice].hostile_to(&world.objects[&bob]));

    // Conquest: the pack has Sabuk Wall; Bob's guild signs up (paying the
    // castle's item), a forced war spawns the lord on the castle map, only
    // castle-less participants hurt it for one point per hit, and the
    // killer's guild takes the castle.
    let Some(castle) = world.data.castles.first().cloned() else {
        eprintln!("no CastleInfo in this pack; conquest part skipped");
        return;
    };
    world.revive_player_test(bob);
    world.guild_request_conquest(bob, castle.index);
    assert!(
        world.guild_store.conquests.is_empty(),
        "the castle item is required first"
    );
    if castle.item != 0 {
        world.test_give_item(bob, castle.item, 1);
    }
    world.guild_request_conquest(bob, castle.index);
    assert_eq!(world.guild_store.conquests.len(), 1);
    let out = to(&mut world);
    assert!(out.iter().any(|(c, m)| *c == cb
        && matches!(m, ServerMessage::GuildConquestDate { index, war_in_secs } if *index == castle.index && *war_in_secs > 0)));
    assert!(world.start_conquest(castle.index, true));
    let lord = world.test_castle_lord().expect("a castle lord");
    assert_eq!(world.objects[&lord].map, castle.map);
    let lord_loc = world.objects[&lord].location;
    let beside = Direction::ALL
        .iter()
        .map(|d| lord_loc.step(*d, 1))
        .find(|p| world.maps[&castle.map].file.is_walkable(p.x, p.y))
        .expect("a cell beside the lord");
    world.test_change_map(bob, castle.map, beside);
    world.test_change_map(alice, castle.map, beside);
    // On the castle map, guilds that are not each other's are at war.
    assert!(world.objects[&alice].hostile_to(&world.objects[&bob]));
    let hp = world.objects[&lord].hp;
    assert_eq!(world.test_damage(lord, bob, 5000), 1);
    assert_eq!(world.objects[&lord].hp, hp - 1);
    world.test_set_hp(lord, 1);
    drain(&mut world);
    world.test_damage(lord, bob, 5000);
    assert_eq!(world.guild_store.castle_owner(castle.index), Some(rogues));
    let out = to(&mut world);
    assert!(out.iter().any(|(_, m)| matches!(m,
        ServerMessage::CastleInfo { owner, .. } if owner == "Rogues")));
    // The 10 % castle buff adds to this one-member guild's 30 % XP buff.
    let before = world.test_experience(bob);
    world.gain_experience_test(bob, 20);
    assert_eq!(world.test_experience(bob) - before, 28);
    world.test_end_conquest();
    assert!(world.conquest.is_none());
    drain(&mut world);
    world.guild_request_conquest(bob, castle.index);
    let out = to(&mut world);
    assert!(out.iter().any(|(c, m)| *c == cb
        && matches!(m, ServerMessage::Say { text, .. } if text.contains("already holds"))));
}

#[test]
fn castle_guards_gates_and_flags() {
    use crate::data::CastlePartDef;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let Some(castle) = world.data.castles.first().cloned() else {
        eprintln!("no castle in this pack; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let mut bob_rec = test_character("Bob");
    bob_rec.id = 2;
    let bob = world.add_player(2, 2, &bob_rec).unwrap();
    world.tick(0);
    drain(&mut world);
    world.test_set_gold(alice, 20_000_000);
    world.test_set_gold(bob, 20_000_000);
    world.guild_create(alice, "Knights".into(), 5);
    world.guild_create(bob, "Rogues".into(), 5);
    let knights = world.guild_store.guilds[0].id;
    let rogues = world.guild_store.guilds[1].id;
    // Knights hold the castle already.
    world.guild_store.castles.push(world::guilds::CastleOwner {
        castle: castle.index,
        guild: knights,
    });

    // Lay out a guard, a gate and a flag in the castle region.
    let map = castle.map;
    world.test_spawn_castle_objects(map);
    let width = world.maps[&map].file.width as i32;
    let open_patch = |world: &World, p: Point| {
        (-2..=2).all(|dx| {
            (-2..=2).all(|dy| {
                let q = Point::new(p.x + dx, p.y + dy);
                world.maps[&map].file.is_walkable(q.x, q.y)
                    && world.maps[&map].objects_at(q).is_empty()
            })
        })
    };
    let origin = world.data.regions[&castle.castle_region]
        .points(width)
        .first()
        .map(|(x, y)| Point::new(*x, *y))
        .expect("a castle region cell");
    let base = (0..80)
        .flat_map(|r| {
            (-r..=r).flat_map(move |dx| [(dx, -r), (dx, r), (-r, dx), (r, dx)].into_iter())
        })
        .map(|(dx, dy)| Point::new(origin.x + dx, origin.y + dy))
        .find(|p| open_patch(&world, *p))
        .expect("an open 5x5 patch near the castle");
    let part = |kind: u8, x: i32, y: i32| CastlePartDef {
        kind,
        monster: castle.monster,
        x,
        y,
        direction: 0,
        repair_cost: 10_000,
    };
    let guard_at = Point::new(base.x - 2, base.y - 2);
    let gate_at = Point::new(base.x + 2, base.y);
    let flag_at = Point::new(base.x, base.y + 2);
    {
        let def = world
            .data
            .castles
            .iter_mut()
            .find(|c| c.index == castle.index)
            .unwrap();
        def.guards.push(part(0, guard_at.x, guard_at.y));
        def.gates.push(part(1, gate_at.x, gate_at.y));
        def.flags.push(part(2, flag_at.x, flag_at.y));
    }
    world.test_spawn_castle_objects(map);
    let parts = world.test_castle_parts(map);
    let guard = parts
        .iter()
        .find(|(_, k)| *k == 0)
        .map(|(i, _)| *i)
        .expect("guard");
    let gate = parts
        .iter()
        .find(|(_, k)| *k == 1)
        .map(|(i, _)| *i)
        .expect("gate");
    assert!(parts.iter().any(|(_, k)| *k == 2), "flag");

    // A shut gate blocks its cell; the owner walking up opens it in peace.
    assert!(world.test_cell_blocked(map, gate_at));
    world.test_change_map(alice, map, base);
    world.test_change_map(bob, map, Point::new(base.x, base.y - 1));
    for t in 1..=3 {
        world.tick(t * 1000);
    }
    assert!(
        !world.test_cell_blocked(map, gate_at),
        "owner should open the gate"
    );

    // War: the owner's toggle shuts and opens every gate; the guard shoots
    // the enemy and only the enemy can hurt it.
    assert!(world.start_conquest(castle.index, true));
    // The lord has its own test; here it would only kill Bob.
    if let Some(lord) = world.test_castle_lord() {
        world.remove_object(lord);
    }
    world.test_change_map(bob, map, Point::new(base.x, base.y - 1));
    world.test_change_map(alice, map, base);
    world.guild_toggle_castle_gates(alice);
    assert!(world.test_cell_blocked(map, gate_at));
    world.guild_toggle_castle_gates(bob);
    assert!(
        world.test_cell_blocked(map, gate_at),
        "only the owner toggles"
    );
    world.guild_toggle_castle_gates(alice);
    assert!(!world.test_cell_blocked(map, gate_at));
    assert_eq!(world.test_damage(guard, alice, 500), 0);
    assert!(world.test_damage(guard, bob, 500) > 0);
    let bob_hp = world.objects[&bob].hp;
    for t in 4..=12 {
        world.tick(t * 1000);
    }
    assert!(
        world.objects[&bob].hp < bob_hp || world.objects[&bob].dead,
        "the guard should shoot"
    );
    world.test_set_hp(bob, 1_000_000);
    if world.objects[&bob].dead {
        world.revive_player_test(bob);
        world.test_change_map(bob, map, Point::new(base.x, base.y - 1));
    }

    // Repairs: the leader pays the damage share from the funds.
    world.guild_toggle_castle_gates(alice);
    let dealt = world.test_damage(gate, bob, 1000);
    assert!(dealt > 0);
    world.test_set_guild_funds(knights, 50_000);
    world.guild_repair_castle_gates(alice);
    assert_eq!(world.objects[&gate].hp, world.objects[&gate].max_hp);
    assert!(world.guild_store.guilds[0].funds < 50_000);

    // Flag: Rogues stand beside it unopposed for 30 s and take the castle.
    world.remove_object(guard);
    if world.objects[&bob].dead {
        world.revive_player_test(bob);
    }
    world.test_set_hp(bob, 1_000_000);
    world.test_change_map(bob, map, Point::new(flag_at.x, flag_at.y - 1));
    world.test_change_map(alice, map, Point::new(base.x - 2, base.y));
    let start = 20_000;
    for t in 0..40 {
        world.tick(start + t * 1000);
    }
    assert_eq!(world.guild_store.castle_owner(castle.index), Some(rogues));
}

/// The client is told about currencies on entry and again when one moves,
/// so Zircon's `CurrencyDialog` has something to show.
#[test]
fn currencies_reach_the_client_on_entry_and_on_change() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Banker")).unwrap();
    let entry = drain(&mut world);
    let list = entry
        .iter()
        .find_map(|m| match m {
            ServerMessage::Currencies(c) => Some(c.clone()),
            _ => None,
        })
        .expect("entry sends the currency list");
    // Gold lives in the bag, so it is never one of these rows.
    assert!(!list.is_empty(), "the pack defines non-gold currencies");
    assert!(list.iter().all(|c| c.amount == 0));
    world.test_set_currency_by_type(me, world::fame::CURRENCY_FP, 42);
    let after = drain(&mut world);
    let list = after
        .iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::Currencies(c) => Some(c.clone()),
            _ => None,
        })
        .expect("a change resends the list");
    assert_eq!(list.iter().map(|c| c.amount).sum::<i64>(), 42);
}

#[test]
fn friends_persist_and_blocked_whispers_vanish() {
    use mir_proto::{online_state, ChatKind};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("zircon-social-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    world.set_store_dir(&dir);

    let mut arec = test_character("Alice");
    arec.id = 11;
    let mut brec = test_character("Bob");
    brec.id = 22;
    let alice = world.add_player(1, 1, &arec).unwrap();
    let bob = world.add_player(2, 2, &brec).unwrap();
    world.tick(0);
    drain(&mut world);

    // An unknown name is refused, and nothing is stored.
    world.friend_add(alice, None, "Nobody".into());
    assert!(world.social_store.friends.is_empty());
    // You cannot befriend yourself.
    world.friend_add(alice, Some((1, 11, "Alice".into())), "Alice".into());
    assert!(world.social_store.friends.is_empty());

    world.friend_add(alice, Some((2, 22, "Bob".into())), "Bob".into());
    let list = world.friend_list(11);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Bob");
    // Bob is logged in, so he reads as Online.
    assert_eq!(list[0].state, online_state::ONLINE);
    let index = list[0].index;
    // Adding the same character twice is refused.
    world.friend_add(alice, Some((2, 22, "Bob".into())), "Bob".into());
    assert_eq!(world.friend_list(11).len(), 1);

    // The store survives a reload from disk.
    let reloaded = crate::world::social::SocialStore::load(dir.join("social.json"));
    assert_eq!(reloaded.friends_of(11).count(), 1);
    assert_eq!(reloaded.friends_of(22).count(), 0);

    // Bob's chosen state reaches Alice's list.
    world.change_online_state(bob, online_state::BUSY);
    assert_eq!(world.friend_list(11)[0].state, online_state::BUSY);

    // Blocking is mutual and silent: Alice's whisper answers "could not
    // find", exactly as Zircon does, and Bob never hears it.
    world.block_add(bob, Some((1, 11, "Alice".into())), "Alice".into());
    assert!(world.is_blocking(1, 2));
    assert!(world.is_blocking(2, 1));
    drain(&mut world);
    world.chat(alice, "/Bob are you there".into());
    let msgs = drain(&mut world);
    assert!(!msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::Say { kind, .. } if *kind == ChatKind::WhisperIn)));
    assert!(msgs.iter().any(
        |m| matches!(m, ServerMessage::Say { text, .. } if text.contains("Could not find Bob"))
    ));

    // Unblocking restores the whisper.
    let bindex = world.block_list(2)[0].index;
    world.block_remove(bob, bindex);
    assert!(!world.is_blocking(1, 2));
    drain(&mut world);
    world.chat(alice, "/Bob hello again".into());
    let msgs = drain(&mut world);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, ServerMessage::Say { kind, .. } if *kind == ChatKind::WhisperIn)));

    // Removing the friend empties the list and the file.
    world.friend_remove(alice, index);
    assert!(world.friend_list(11).is_empty());
    let reloaded = crate::world::social::SocialStore::load(dir.join("social.json"));
    assert_eq!(reloaded.friends_of(11).count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The fortune checker is bought with a Fortune Checker item, and reports
/// the account's drop progress for one item (Zircon `FortuneCheck`).
#[test]
fn a_fortune_check_costs_a_checker_and_reports_progress() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Lucky")).unwrap();
    let checker = world
        .fortune_checker_item()
        .expect("the pack has a Fortune Checker item");
    // An item something actually drops, so the check is not refused.
    let wanted = world
        .data
        .drops
        .iter()
        .find(|d| d.chance > 0)
        .map(|d| d.item)
        .expect("the pack has drops");

    // Without a checker in the bag nothing is spent and nothing is sent.
    world.fortune_check(me, wanted);
    let refused = drain(&mut world);
    assert!(
        !refused
            .iter()
            .any(|m| matches!(m, ServerMessage::FortuneUpdate { .. })),
        "a check with no checker should not report anything"
    );

    world.test_give_item(me, checker, 2);
    drain(&mut world);
    world.fortune_check(me, wanted);
    let after = drain(&mut world);
    let fortunes = after
        .iter()
        .find_map(|m| match m {
            ServerMessage::FortuneUpdate { fortunes } => Some(fortunes.clone()),
            _ => None,
        })
        .expect("a paid check reports the item");
    assert_eq!(fortunes.len(), 1);
    assert_eq!(fortunes[0].item, wanted);
    // One checker was spent, so one is left.
    let left = world.objects[&me]
        .player()
        .map(|p| p.bag.count_of(checker))
        .unwrap_or(0);
    assert_eq!(left, 1, "exactly one checker is spent per check");
}

/// Progress banks the expected yield of every roll, and once it runs a
/// whole item ahead of what has actually dropped the next roll is forced
/// through however badly it lands (Zircon's `EnableFortune` pity rule).
#[test]
fn drop_progress_accumulates_and_eventually_forces_a_drop() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Grinder")).unwrap();
    let item = 1;
    // A 1-in-4 roll worth one item: each roll banks a quarter of an item.
    for _ in 0..3 {
        assert!(
            !world.add_drop_progress(Some(me), item, 1, 4, false),
            "under a whole item of expectation nothing is owed"
        );
    }
    // The fourth roll tips the expectation past one whole item.
    assert!(
        world.add_drop_progress(Some(me), item, 1, 4, false),
        "a full item of expectation forces the drop"
    );
    // Paying it out clears the debt.
    world.add_drop_count(Some(me), item, 1);
    assert!(
        !world.add_drop_progress(Some(me), item, 1, 4, false),
        "the count has caught up, so nothing is owed again"
    );
    let progress = world.objects[&me]
        .player()
        .and_then(|p| p.drops.iter().find(|d| d.item == item))
        .map(|d| (d.drop_count, d.progress))
        .expect("progress was recorded");
    assert_eq!(progress.0, 1);
    assert!(
        (progress.1 - 1.25).abs() < 1e-9,
        "five quarter rolls banked"
    );
}

/// `part_only` rows never drop the item itself, so Zircon does not let
/// them build the expectation either.
#[test]
fn part_only_rolls_bank_no_progress() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Parts")).unwrap();
    for _ in 0..10 {
        world.add_drop_progress(Some(me), 1, 1, 2, true);
    }
    let progress = world.objects[&me]
        .player()
        .and_then(|p| p.drops.iter().find(|d| d.item == 1))
        .map(|d| d.progress)
        .unwrap_or(0.0);
    assert_eq!(progress, 0.0, "part rows bank nothing");
}

/// Drop progress is account state, so it survives leaving and re-entering
/// the world the way storage does.
#[test]
fn fortunes_round_trip_through_the_account() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Saver")).unwrap();
    world.add_drop_progress(Some(me), 7, 1, 2, false);
    world.add_drop_count(Some(me), 7, 3);
    let (drops, fortunes) = world.fortunes_of(me).expect("read back");
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].drop_count, 3);
    world.remove_object(me);

    // A fresh session loads the saved numbers back.
    let again = world.add_player(2, 1, &test_character("Saver")).unwrap();
    world.set_fortunes(again, &drops, &fortunes);
    let loaded = world.objects[&again]
        .player()
        .map(|p| p.drops.clone())
        .unwrap_or_default();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].item, 7);
    assert_eq!(loaded[0].drop_count, 3);
    assert!((loaded[0].progress - 0.5).abs() < 1e-9);
}

#[test]
fn game_gold_tabs_charge_once_and_preserve_inventory_capacity() {
    let Some(mut world) = world() else { return };
    let mut rec = test_character("TabBuyer");
    let id = world.add_player(1, 1, &rec).unwrap();
    world.test_set_currency_by_type(id, 1, 99);
    assert!(!world.buy_tab(id, false));
    assert_eq!(world.objects[&id].player().unwrap().bag.inventory.len(), 48);

    world.test_set_currency_by_type(id, 1, 500);
    assert!(world.buy_tab(id, false));
    assert!(!world.buy_tab(id, false));
    assert!(world.buy_tab(id, true));
    let player = world.objects[&id].player().unwrap();
    assert_eq!(player.bag.inventory.len(), 96);
    assert_eq!(player.bag.storage.len(), 200);
    assert_eq!(world.test_currency_total(id), 200);
    world.snapshot(id, &mut rec);
    assert_eq!(rec.inventory_size, 96);
    let loaded =
        crate::items::Bag::from_stored_with_size(&rec.items, rec.gold, rec.inventory_size as usize);
    assert_eq!(loaded.inventory.len(), 96);
}

mod field_spells;

mod companion_bags;

#[test]
fn item_move_to_an_out_of_range_slot_is_refused() {
    use mir_proto::Grid;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    world.tick(0);
    drain(&mut world);
    let info = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .unwrap()
        .index;
    world.test_give_item(alice, info, 5);
    let slot = world.test_slot_of(alice, info).unwrap();
    // A client can name any u8 slot; the bag has far fewer.
    world.item_move(alice, Grid::Inventory, slot, Grid::Inventory, 200);
    world.item_move(alice, Grid::Inventory, 250, Grid::Inventory, slot);
    assert_eq!(world.test_slot_of(alice, info), Some(slot));
}

#[test]
fn affinity_element_picks_the_highest_in_zircon_order() {
    use crate::world::{affinity_element, AFFINITY_STATS};
    use mir_proto::element;
    assert_eq!(affinity_element(|_| 0), element::NONE);
    // Talisman Of Darkness carries stat 48: Dark.
    assert_eq!(affinity_element(|s| (s == 48) as i32), element::DARK);
    // Ties keep the earlier element, as Zircon's strict `>` does.
    assert_eq!(
        affinity_element(|s| (s == AFFINITY_STATS[1] || s == 49) as i32),
        element::FIRE
    );
    assert_eq!(
        affinity_element(|s| if s == 49 { 3 } else { 1 }),
        element::PHANTOM
    );
}

#[test]
fn mail_refuses_the_same_cell_attached_twice() {
    use mir_proto::Grid;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let _bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(20_000);
    drain(&mut world);
    let potion = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .unwrap()
        .index;
    world.test_give_item(alice, potion, 4);
    let slot = world.test_slot_of(alice, potion).unwrap();
    world.mail_send(
        alice,
        Some((2, "Bob".into())),
        "Dupe".into(),
        "x".into(),
        0,
        vec![(Grid::Inventory, slot, 1); 3],
    );
    assert!(world.mail_store.boxes.values().all(|b| b.is_empty()));
    assert_eq!(world.test_bag(alice).1, vec![(potion, 4)]);
}

#[test]
fn trade_cancels_when_an_offered_slot_is_swapped_after_confirm() {
    use mir_proto::Grid;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    drain(&mut world);
    let potion = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .unwrap()
        .index;
    let ore = world
        .data
        .items
        .values()
        .find(|d| d.effect == 20)
        .map(|d| d.index)
        .expect("black iron ore");
    world.test_give_item(alice, potion, 1);
    world.test_give_item(alice, ore, 1);
    let pslot = world.test_slot_of(alice, potion).unwrap();
    let oslot = world.test_slot_of(alice, ore).unwrap();
    let loc = world.objects[&alice].location;
    let map = world.objects[&alice].map;
    let (dir, cell) = Direction::ALL
        .iter()
        .map(|d| (*d, loc.step(*d, 1)))
        .find(|(_, p)| world.maps[&map].file.is_walkable(p.x, p.y))
        .expect("walkable neighbour");
    world.teleport(bob, cell);
    world.test_face(alice, dir);
    world.test_face(bob, dir.opposite());
    world.trade_request(alice);
    world.trade_response(bob, true);
    // Alice shows the ore; Bob confirms; Alice swaps the potion into that
    // slot and confirms herself.
    world.trade_add_item(alice, Grid::Inventory, oslot, 1);
    world.trade_confirm(bob);
    world.item_move(alice, Grid::Inventory, pslot, Grid::Inventory, oslot);
    assert_eq!(world.test_slot_of(alice, potion), Some(oslot));
    world.trade_confirm(alice);
    drain(&mut world);
    assert!(world.objects[&alice].player().unwrap().trade.is_none());
    assert_eq!(world.test_slot_of(bob, potion), None);
    assert_eq!(world.test_slot_of(bob, ore), None);
    assert_eq!(world.test_slot_of(alice, potion), Some(oslot));
    assert_eq!(world.test_slot_of(alice, ore), Some(pslot));
}

#[test]
fn weapon_refine_refuses_the_same_ore_cell_twice() {
    use mir_proto::{refine_quality, refine_type, Grid};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let me = world.add_player(1, 1, &test_character("Smith")).unwrap();
    world.tick(0);
    drain(&mut world);
    let weapon = world
        .data
        .items
        .values()
        .filter(|d| d.item_type == mir_proto::item_type::WEAPON && d.required_amount <= 1)
        .filter(|d| {
            d.effect != 5
                && crate::items::can_use(d, mir_proto::Class::Warrior, mir_proto::Gender::Male, 1)
                    .is_ok()
        })
        .min_by_key(|d| d.index)
        .map(|d| d.index)
        .expect("a starter weapon");
    let ore = world
        .data
        .items
        .values()
        .find(|d| d.effect == 20)
        .map(|d| d.index)
        .expect("black iron ore");
    world.test_give_item(me, weapon, 1);
    world.test_give_item(me, ore, 1);
    world.test_set_gold(me, 100_000);
    let wslot = world.test_slot_of(me, weapon).unwrap();
    world.item_move(me, Grid::Inventory, wslot, Grid::Equipment, 0);
    let oslot = world.test_slot_of(me, ore).unwrap();
    world.gain_experience_test(me, 1);
    let refine_page = world.test_page_with_dialog_type(3);
    world.test_open_page(me, refine_page);
    world.npc_refine(
        me,
        refine_type::DC,
        refine_quality::RUSH,
        vec![(Grid::Inventory, oslot, 1); 3],
        vec![],
        vec![],
    );
    let p = world.objects[&me].player().unwrap();
    assert!(p.refines.is_empty());
    assert!(p.bag.equipment[0].is_some());
    assert_eq!(p.bag.gold, 100_000);
    assert_eq!(world.test_slot_of(me, ore), Some(oslot));
}
