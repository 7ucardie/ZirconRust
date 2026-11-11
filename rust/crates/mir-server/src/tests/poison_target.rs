use mir_proto::{attack_mode, item_type, magic_type, slot, Class, Direction};

use super::*;

#[test]
fn poison_dust_respects_peaceful_mode_for_owned_shinsu() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("PeacefulTaoist");
    record.class = Class::Taoist;
    record.level = 100;
    let me = world.add_player(1, 1, &record).unwrap();
    world.tick(0);
    let dust = learn(&mut world, me, "Poison Dust");
    let augment_name = world.data.magics[&magic_type::AUGMENT_POISON_DUST]
        .name
        .clone();
    learn(&mut world, me, &augment_name);
    let poison = world
        .data
        .items
        .values()
        .find(|def| def.item_type == item_type::POISON && def.shape == 0)
        .unwrap()
        .index;
    world.test_give_item(me, poison, 5);
    let bag_slot = world.test_slot_of(me, poison).unwrap();
    world.item_use(me, bag_slot);
    assert!(world.objects[&me].player().unwrap().bag.equipment[slot::POISON].is_some());

    let map = world.objects[&me].map;
    let loc = world.objects[&me].location;
    world.test_clear_monsters(map);
    let cell = Direction::ALL
        .into_iter()
        .map(|direction| loc.step(direction, 1))
        .find(|cell| {
            world.maps[&map].file.is_walkable(cell.x, cell.y)
                && world.maps[&map].objects_at(*cell).is_empty()
        })
        .expect("a clear cell near Taoist");
    let shinsu = world.test_spawn_ai(53, map, cell).expect("Shinsu");
    let wild_cell = Direction::ALL
        .into_iter()
        .map(|direction| loc.step(direction, 1))
        .find(|other| {
            *other != cell
                && world.maps[&map].file.is_walkable(other.x, other.y)
                && world.maps[&map].objects_at(*other).is_empty()
        })
        .expect("second clear cell");
    let chicken = world
        .data
        .monsters
        .values()
        .find(|def| def.name == "Chicken")
        .unwrap()
        .index;
    let wild = world.test_spawn_monster(chicken, map, wild_cell, None);
    let pet = world.objects.get_mut(&shinsu).unwrap();
    pet.monster_mut().unwrap().owner = Some(me);
    pet.monster_mut().unwrap().tame_until = u64::MAX;
    pet.stats.poison_resistance = 0;
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .pets
        .push(shinsu);

    world.tick(3000);
    world.set_attack_mode(me, attack_mode::PEACE);
    assert_eq!(world.objects[&shinsu].monster().unwrap().owner, Some(me));
    assert!(
        world.objects[&shinsu].poisons.is_empty(),
        "Shinsu poisoned before cast"
    );
    world.cast(me, dust, Direction::Down, Some(shinsu), cell);
    world.tick(3600);
    assert!(
        world.objects[&shinsu].poisons.is_empty(),
        "Peaceful Taoist poisoned own Shinsu"
    );
    world.tick(6000);
    world.teleport(shinsu, cell);
    world.teleport(wild, wild_cell);
    world.cast(me, dust, Direction::Down, Some(wild), wild_cell);
    world.tick(6600);
    assert!(
        !world.objects[&wild].poisons.is_empty(),
        "wild monster must be poisoned"
    );
    assert!(
        world.objects[&shinsu].poisons.is_empty(),
        "Augment poisoned own Shinsu"
    );

    world.tick(9000);
    world.cast(me, dust, Direction::Down, Some(me), loc);
    world.tick(9600);
    assert!(
        world.objects[&me].poisons.is_empty(),
        "Taoist poisoned self"
    );
}

#[test]
fn poison_dust_respects_pvp_attack_mode() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("PvpTaoist");
    record.class = Class::Taoist;
    record.level = 100;
    let me = world.add_player(1, 1, &record).unwrap();
    let other = world
        .add_player(2, 2, &test_character("PvpTarget"))
        .unwrap();
    world.tick(0);
    let dust = learn(&mut world, me, "Poison Dust");
    world.test_give_item(me, 229, 5);
    let bag_slot = world.test_slot_of(me, 229).unwrap();
    world.item_use(me, bag_slot);
    world
        .objects
        .get_mut(&other)
        .unwrap()
        .stats
        .poison_resistance = 0;

    let map = world.objects[&me].map;
    let start = world.objects[&me].location;
    let outside = world.test_cell_outside_safe_zone(map, start).unwrap();
    let next = Direction::ALL
        .into_iter()
        .map(|direction| outside.step(direction, 1))
        .find(|cell| {
            world.maps[&map].file.is_walkable(cell.x, cell.y)
                && !world.test_in_safe_zone_at(map, *cell)
        })
        .unwrap();
    world.teleport(me, outside);
    world.test_change_map(other, map, next);
    world.tick(3000);
    assert!(!world.test_in_safe_zone(me) && !world.test_in_safe_zone(other));
    assert_eq!(world.objects[&other].location, next);
    assert_eq!(world.objects[&other].map, map);
    assert!(world.objects[&me].location.distance(next) <= mir_proto::MAGIC_RANGE);
    assert!(world.objects[&me].player().unwrap().bag.equipment[slot::POISON].is_some());
    drain(&mut world);
    world.set_attack_mode(me, attack_mode::PEACE);
    world.cast(me, dust, Direction::Down, Some(other), next);
    world.tick(3600);
    assert!(world.objects[&other].poisons.is_empty());

    world.tick(6000);
    world.set_attack_mode(me, attack_mode::ALL);
    assert!(world.objects[&me].hostile_to(&world.objects[&other]));
    world.cast(me, dust, Direction::Down, Some(other), next);
    world.tick(6600);
    let messages = drain(&mut world);
    assert!(messages.iter().any(|message| matches!(message,
        ServerMessage::ObjectMagic { magic, targets, cast, .. }
        if *magic == dust && *cast && targets.contains(&other))));
    assert!(world.objects[&other]
        .poisons
        .iter()
        .any(|poison| poison.kind == world::poison_kind::GREEN && poison.owner == Some(me)));
}
