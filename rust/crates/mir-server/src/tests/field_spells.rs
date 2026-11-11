use super::*;
use mir_proto::{magic_type, Class};

#[test]
fn meteor_selects_hostile_targets_only() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("MeteorTargets");
    record.class = Class::Wizard;
    record.level = 100;
    let me = world.add_player(1, 1, &record).unwrap();
    world.tick(0);
    let name = world.data.magics[&magic_type::METEOR_SHOWER].name.clone();
    let magic = learn(&mut world, me, &name);
    let map = world.objects[&me].map;
    let location = world.objects[&me].location;
    world.test_clear_monsters(map);
    let def = world
        .data
        .monsters
        .values()
        .find(|d| d.name == "Chicken")
        .unwrap()
        .index;
    let pet = world.test_spawn_monster(def, map, location, Some(me));
    let wild = world.test_spawn_monster(def, map, location, None);
    drain(&mut world);
    world.cast(me, magic, Direction::Down, None, location);
    world.tick(1);
    let messages = drain(&mut world);
    let targets = messages
        .iter()
        .find_map(|m| match m {
            ServerMessage::ObjectMagic {
                magic: cast_magic,
                targets,
                cast: true,
                ..
            } if *cast_magic == magic => Some(targets),
            _ => None,
        })
        .expect("successful Meteor cast");
    assert!(targets.contains(&wild));
    assert!(
        !targets.contains(&pet),
        "friendly pets must not consume meteor targets"
    );
}

#[test]
fn firewall_burns_on_entry_between_ticks_and_spares_pets() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("FirewallEntry");
    record.class = Class::Wizard;
    record.level = 100;
    let me = world.add_player(1, 1, &record).unwrap();
    world.tick(0);
    let name = world.data.magics[&magic_type::FIRE_WALL].name.clone();
    let magic = learn(&mut world, me, &name);
    let map = world.objects[&me].map;
    let location = world.objects[&me].location;
    world.test_clear_monsters(map);
    let cell = Direction::ALL
        .into_iter()
        .map(|d| location.step(d, 2))
        .find(|c| world.maps[&map].file.is_walkable(c.x, c.y))
        .unwrap();
    world.cast(me, magic, Direction::Down, None, cell);
    world.tick(600);
    assert!(world.maps[&map]
        .objects_at(cell)
        .iter()
        .any(|id| world.objects[id].is_spell()));
    let def = world
        .data
        .monsters
        .values()
        .find(|d| d.name == "Chicken")
        .unwrap()
        .index;
    let wild = world.test_spawn_monster(def, map, location, None);
    let pet = world.test_spawn_monster(def, map, location, Some(me));
    let wild_hp = world.objects[&wild].hp;
    let pet_hp = world.objects[&pet].hp;
    world.teleport(wild, cell);
    assert!(
        world.objects[&wild].hp < wild_hp,
        "entering fire must hurt before next periodic tick"
    );
    world.teleport(pet, cell);
    assert_eq!(world.objects[&pet].hp, pet_hp);
}

#[test]
fn targeted_pickup_preserves_range_and_ownership() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("TargetedPickup"))
        .unwrap();
    world.tick(0);
    let player = &world.objects[&me];
    let map = player.map;
    let at = player.location;
    let account = player.player().unwrap().account;
    let gold = world.data.gold_item;
    let before = player.player().unwrap().bag.gold;
    let mut drops = Vec::new();
    for (location, count, owner) in [
        (at, 10, account),
        (at, 20, account),
        (at, 30, account + 1),
        (at.step(Direction::Down, 3), 40, account),
    ] {
        world.test_drop_item(map, location, gold, count, owner);
        drops.push(*world.objects.keys().max().unwrap());
    }
    world.pick_up_item(me, drops[1]);
    assert_eq!(world.objects[&me].player().unwrap().bag.gold, before + 20);
    assert!(world.objects.contains_key(&drops[0]));
    assert!(!world.objects.contains_key(&drops[1]));
    world.pick_up_item(me, drops[2]);
    world.pick_up_item(me, drops[3]);
    assert_eq!(world.objects[&me].player().unwrap().bag.gold, before + 20);
    assert!(world.objects.contains_key(&drops[2]));
    assert!(world.objects.contains_key(&drops[3]));
}

#[test]
fn full_bag_does_not_hide_nearby_gold() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("FullBagGold"))
        .unwrap();
    world.tick(0);
    let item = world
        .data
        .items
        .values()
        .find(|d| d.item_type == mir_proto::item_type::WEAPON)
        .unwrap()
        .index;
    world.test_give_item(me, item, 1);
    let player = world.objects.get_mut(&me).unwrap();
    let map = player.map;
    let at = player.location;
    let p = player.player_mut().unwrap();
    let weapon = p
        .bag
        .inventory
        .iter()
        .flatten()
        .find(|i| i.info == item)
        .unwrap()
        .clone();
    p.bag.inventory.fill(Some(weapon));
    let account = p.account;
    let before = p.bag.gold;
    world.test_drop_item(map, at, item, 1, account);
    world.test_drop_item(map, at, world.data.gold_item, 23, account);
    world.pick_up(me);
    assert_eq!(world.objects[&me].player().unwrap().bag.gold, before + 23);
}

#[test]
fn ground_drop_expires_at_lifetime_and_notifies_viewer() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("DropExpiry"))
        .unwrap();
    world.tick(0);
    let map = world.objects[&me].map;
    let at = world.objects[&me].location;
    let account = world.objects[&me].player().unwrap().account;
    world.test_clear_monsters(map);
    world.test_drop_item(map, at, world.data.gold_item, 1, account);
    let drop = *world.objects.keys().max().unwrap();
    let expire = match &world.objects[&drop].kind {
        world::Kind::Item(item) => item.expire,
        _ => unreachable!(),
    };
    assert_eq!(expire, world::DROP_DURATION);
    world.tick(expire - 1);
    assert!(world.objects.contains_key(&drop));
    drain(&mut world);
    world.tick(expire);
    assert!(!world.objects.contains_key(&drop));
    assert!(!world.maps[&map].objects_at(at).contains(&drop));
    assert!(drain(&mut world)
        .iter()
        .any(|message| matches!(message, ServerMessage::ObjectRemove { id } if *id == drop)));
    world.test_drop_item(map, at, world.data.gold_item, 1, account);
    let unseen_drop = *world.objects.keys().max().unwrap();
    world.remove_object(me);
    world.tick(expire + world::DROP_DURATION);
    assert!(
        !world.objects.contains_key(&unseen_drop),
        "drops expire without nearby players"
    );
}

#[test]
fn enhanced_ground_drop_preserves_added_attribute_marker() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("EnhancedDrop"))
        .unwrap();
    world.tick(0);
    let item = world
        .data
        .items
        .values()
        .find(|d| d.item_type == mir_proto::item_type::WEAPON && d.can_drop)
        .map(|d| d.index);
    let Some(item) = item else {
        panic!("droppable weapon")
    };
    world.test_give_item(me, item, 1);
    let slot = world.test_slot_of(me, item).unwrap();
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
        .add_stat(
            mir_formats::mirdb::stat::MAX_MC,
            1,
            crate::items::StatSource::Added,
        );
    world.item_drop(me, slot, 1);
    assert!(world.objects.values().any(
        |o| matches!(o.appearance, Appearance::Item { info, enhanced: true, .. } if info == item)
    ));
}
