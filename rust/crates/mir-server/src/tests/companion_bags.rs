use super::*;

#[test]
fn companion_bag_equipment_expands_capacity_and_survives_storage() {
    let Some(mut world) = world() else { return };
    let offer = world
        .data
        .companions
        .iter()
        .find(|offer| world.data.monsters.contains_key(&offer.monster))
        .unwrap()
        .index;
    let bag = world
        .data
        .items
        .values()
        .find(|item| item.item_type == mir_proto::item_type::COMPANION_BAG && item.stat(82) > 0)
        .unwrap()
        .index;
    let me = world.add_player(1, 1, &test_character("BagPet")).unwrap();
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions
        .push(world::StoredCompanion {
            index: 1,
            info: offer,
            name: "Rex".into(),
            level: 1,
            experience: 0,
            hunger: 100,
            items: Vec::new(),
            food: None,
            bag: None,
            bonuses: Vec::new(),
        });
    let page = world.test_page_with_dialog_type(5);
    world.test_open_page(me, page);
    world.companion_retrieve(me, 1);
    world.test_give_item(me, bag, 1);
    let slot = world.test_slot_of(me, bag).unwrap();
    let original = world.objects[&me].player().unwrap().bag.inventory[slot as usize]
        .as_ref()
        .unwrap()
        .id;
    world.companion_bag_equip(me, slot);
    let p = world.objects[&me].player().unwrap();
    assert!(p.bag.inventory[slot as usize].is_none());
    assert_eq!(p.companions[0].bag.as_ref().unwrap().id, original);
    assert!(world.outgoing.iter().any(|out| matches!(out,
        Outgoing::To(1, ServerMessage::Companions(list)) if list.first().is_some_and(|c| c.bag.is_some() && c.bag_size > 0)
    )));
    let saved = serde_json::to_string(&p.companions[0]).unwrap();
    let restored: world::StoredCompanion = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.bag.unwrap().id, original);
    let mut legacy: serde_json::Value = serde_json::from_str(&saved).unwrap();
    legacy.as_object_mut().unwrap().remove("bag");
    assert!(serde_json::from_value::<world::StoredCompanion>(legacy)
        .unwrap()
        .bag
        .is_none());
    world.companion_store(me);
    world.companion_retrieve(me, 1);
    assert_eq!(
        world.objects[&me].player().unwrap().companions[0]
            .bag
            .as_ref()
            .unwrap()
            .id,
        original
    );
    // A level-one pet has zero base slots. Prevent removing its occupied bag.
    let item = world.objects[&me].player().unwrap().companions[0]
        .bag
        .as_ref()
        .unwrap()
        .clone();
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions[0]
        .items
        .push(item);
    world.companion_bag_unequip(me);
    assert!(world.objects[&me].player().unwrap().companions[0]
        .bag
        .is_some());
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companions[0]
        .items
        .clear();
    world.companion_bag_unequip(me);
    assert!(world.objects[&me].player().unwrap().companions[0]
        .bag
        .is_none());
    assert_eq!(
        world.objects[&me]
            .player()
            .unwrap()
            .bag
            .inventory
            .iter()
            .flatten()
            .find(|item| item.info == bag)
            .unwrap()
            .count,
        1
    );
}

#[test]
fn companion_collects_with_combat_pets_stopped_and_merges_full_bag_stacks() {
    let Some(mut world) = world() else { return };
    let offer = world
        .data
        .companions
        .iter()
        .find(|offer| world.data.monsters.contains_key(&offer.monster))
        .unwrap()
        .index;
    let me = world
        .add_player(1, 1, &test_character("StoppedPet"))
        .unwrap();
    let potion = world
        .data
        .items
        .values()
        .find(|item| item.name == "Healing Potion")
        .unwrap()
        .index;
    let (map, loc, account) = {
        let o = &world.objects[&me];
        (o.map, o.location, o.player().unwrap().account)
    };
    let p = world.objects.get_mut(&me).unwrap().player_mut().unwrap();
    p.pet_mode = mir_proto::pet_mode::NONE;
    p.companions.push(world::StoredCompanion {
        index: 1,
        info: offer,
        name: "Rex".into(),
        level: 2,
        experience: 0,
        hunger: 100,
        items: Vec::new(),
        food: None,
        bag: None,
        bonuses: Vec::new(),
    });
    let page = world.test_page_with_dialog_type(5);
    world.test_open_page(me, page);
    world.companion_retrieve(me, 1);
    let at = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 3))
        .find(|p| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.test_drop_item(map, at, potion, 1, account);
    for t in (100..10_000).step_by(100) {
        world.tick(t);
    }
    assert_eq!(
        world.objects[&me].player().unwrap().companions[0]
            .items
            .len(),
        1,
        "companion should collect with combat pets stopped"
    );
    let cid = world.objects[&me].player().unwrap().companion.unwrap();
    let at = world.objects[&cid].location;
    world.test_drop_item(map, at, potion, 1, account);
    for t in (10_000..12_000).step_by(100) {
        world.tick(t);
    }
    let items = &world.objects[&me].player().unwrap().companions[0].items;
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].count, 2,
        "full companion bag should merge compatible stacks"
    );
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .companion_collection = true;
    let item = crate::items::UserItem::fresh(&world.data, potion, 1).unwrap();
    assert!(world.companion_auto_collect(me, &item));
    assert_eq!(
        world.objects[&me].player().unwrap().companions[0].items[0].count,
        3
    );
}
