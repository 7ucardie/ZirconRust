//! End-to-end checks for the recovered worktrees and their shared item model.
use super::*;
use crate::items::UserItem;
use mir_proto::Gender;

#[test]
fn harvested_loot_is_owned_retained_when_full_and_bones_are_per_viewer() {
    let Some(mut w) = world() else {
        return;
    };
    let alice = w.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = w.add_player(2, 2, &test_character("Bob")).unwrap();
    w.tick(0);
    let map = w.objects[&alice].map;
    let loc = w.objects[&alice].location;
    w.teleport(bob, loc);
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 1))
        .find(|p| w.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    let chicken = w.test_spawn_ai(1, map, cell).unwrap();
    w.tick(1);
    drain(&mut w);
    w.test_damage(chicken, alice, 100_000);
    w.tick(2);
    let death = std::mem::take(&mut w.outgoing);
    assert!(w.objects[&chicken].dead);
    assert!(!death.iter().any(
        |m| matches!(m, Outgoing::To(2, ServerMessage::ObjectHarvested { id }) if *id == chicken)
    ));
    assert!(!death.iter().any(
        |m| matches!(m, Outgoing::To(1, ServerMessage::ObjectHarvested { id }) if *id == chicken)
    ));
    let dir = Direction::from_points(loc, cell);
    w.harvest(bob, dir);
    assert_eq!(w.objects[&chicken].monster().unwrap().harvest_count, 2);
    assert!(w.test_bag(bob).1.is_empty());
    // A full bag keeps the owed loot intact, even after enough swings.
    let filler = UserItem::fresh(&w.data, 179, 1).unwrap();
    let p = w.objects.get_mut(&alice).unwrap().player_mut().unwrap();
    for (i, c) in p.bag.inventory.iter_mut().enumerate() {
        *c = Some(UserItem {
            id: i as u32 + 1,
            count: 1,
            colour: 123,
            ..filler.clone()
        });
    }
    for now in [602, 1202, 1802] {
        w.tick(now);
        w.harvest(alice, dir);
    }
    assert!(w.objects[&chicken]
        .monster()
        .unwrap()
        .harvest_drops
        .contains_key(&1));
    w.objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory
        .fill(None);
    w.tick(2402);
    w.harvest(alice, dir);
    w.tick(2403);
    assert!(w.test_bag(alice).1.iter().any(|(info, _)| *info == 179));
    assert!(w.objects[&chicken]
        .monster()
        .unwrap()
        .harvest_drops
        .is_empty());
    assert!(drain(&mut w)
        .iter()
        .any(|m| matches!(m, ServerMessage::ObjectHarvested { id } if *id == chicken)));
    let before = w.test_bag(alice);
    w.tick(3002);
    w.harvest(alice, dir);
    assert_eq!(
        w.test_bag(alice),
        before,
        "spent corpses cannot duplicate loot"
    );
    // Leave and re-enter visibility: spent bodies must still be bones.
    w.teleport(bob, Point::new(loc.x + 35, loc.y));
    w.tick(3003);
    drain(&mut w);
    w.teleport(bob, loc);
    w.tick(3004);
    assert!(w.outgoing.iter().any(|m| matches!(m, Outgoing::To(2, ServerMessage::ObjectShow(s)) if s.id == chicken && matches!(s.appearance, Appearance::Monster { skeleton: true, .. }))));
}

#[test]
fn appearance_changes_consume_items_only_on_success_and_survive_snapshot() {
    let Some(mut w) = world() else {
        return;
    };
    let alice = w.add_player(1, 1, &test_character("Alice")).unwrap();
    w.tick(0);
    let hair = w.change_item(51).unwrap();
    w.test_give_item(alice, hair, 2);
    w.hair_change(alice, 3, 0x00123456);
    assert_eq!(w.objects[&alice].player().unwrap().bag.count_of(hair), 2);
    w.hair_change(alice, 3, 0xff123456);
    assert_eq!(w.objects[&alice].player().unwrap().bag.count_of(hair), 1);
    let rename = w.change_item(53).unwrap();
    w.test_give_item(alice, rename, 1);
    w.name_change(alice, "Taken".into(), Some((2, 9, "Taken".into())));
    assert_eq!(w.objects[&alice].player().unwrap().name, "Alice");
    w.name_change(alice, "NewAlice".into(), None);
    assert_eq!(w.objects[&alice].player().unwrap().bag.count_of(rename), 0);
    let gender = w.change_item(50).unwrap();
    w.test_give_item(alice, gender, 1);
    // Changing gender while wearing armour must preserve the item.
    w.gender_change(alice, Gender::Female, 2, 0xffabcdef);
    assert_eq!(w.objects[&alice].player().unwrap().bag.count_of(gender), 1);
    w.item_move(
        alice,
        mir_proto::Grid::Equipment,
        mir_proto::slot::ARMOUR as u8,
        mir_proto::Grid::Inventory,
        10,
    );
    w.gender_change(alice, Gender::Female, 2, 0xffabcdef);
    let mut rec = test_character("Alice");
    w.snapshot(alice, &mut rec);
    assert_eq!(
        (rec.name.as_str(), rec.gender, rec.hair, rec.hair_colour),
        ("NewAlice", Gender::Female, 2, 0xffabcdef)
    );
    let saved = serde_json::to_vec(&rec).unwrap();
    let restored: crate::accounts::CharacterRecord = serde_json::from_slice(&saved).unwrap();
    assert_eq!(restored.hair_colour, rec.hair_colour);
}

#[test]
fn discipline_checks_requirements_charges_once_and_grants_eligible_magic() {
    let Some(mut w) = world() else {
        return;
    };
    let alice = w.add_player(1, 1, &test_character("Alice")).unwrap();
    w.tick(0);
    w.data.disciplines = vec![crate::data::DisciplineDef {
        index: 1,
        level: 1,
        required_level: 10,
        required_experience: 5,
        required_gold: 100,
        focus_points: 20,
    }];
    w.test_set_gold(alice, 100);
    w.increase_discipline(alice);
    assert_eq!(w.test_gold(alice), 100);
    w.objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .level = 10;
    w.increase_discipline(alice);
    assert_eq!(w.test_gold(alice), 100, "experience gate");
    let mut magic = w.data.magics.values().next().unwrap().clone();
    magic.magic = 60_000;
    magic.school = 20;
    magic.class = 1;
    magic.need_level = [10; 3];
    w.data.magics.insert(magic.magic, magic);
    w.objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .discipline_experience = 5;
    w.increase_discipline(alice);
    let p = w.objects[&alice].player().unwrap();
    assert_eq!((p.discipline, p.max_fp, p.bag.gold), (1, 20, 0));
    assert!(p.magics.iter().any(|m| m.magic == 60_000));
    w.gain_discipline_experience(alice, 3);
    let mut rec = test_character("Alice");
    w.snapshot(alice, &mut rec);
    assert_eq!((rec.discipline, rec.discipline_experience), (1, 8));
    w.increase_discipline(alice);
    assert_eq!(w.test_gold(alice), 0);
}

#[test]
fn timed_purchase_with_incompatible_full_bag_never_charges_and_expiry_runs_online() {
    let Some(mut w) = world() else {
        return;
    };
    let alice = w.add_player(1, 1, &test_character("Alice")).unwrap();
    w.tick(0);
    let row = w
        .data
        .stores
        .iter()
        .find(|s| {
            s.available && s.duration == 0 && s.price > 0 && w.data.items[&s.item].stack_size > 1
        })
        .unwrap()
        .clone();
    let item = UserItem::fresh(&w.data, row.item, 1).unwrap();
    let funds = i64::from(row.price) * 5;
    w.test_set_currency_by_type(alice, 1, funds);
    for cell in &mut w
        .objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory
    {
        *cell = Some(item.clone());
    }
    w.data
        .stores
        .iter_mut()
        .find(|s| s.index == row.index)
        .unwrap()
        .duration = 3600;
    let before = w.objects[&alice].player().unwrap().currencies.clone();
    w.store_buy(alice, row.index as u32, 1, false);
    assert_eq!(w.objects[&alice].player().unwrap().currencies, before);
    assert!(w.objects[&alice]
        .player()
        .unwrap()
        .bag
        .inventory
        .iter()
        .flatten()
        .all(|i| i.expires_at.is_none()));
    w.objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[0] = None;
    w.store_buy(alice, row.index as u32, 1, false);
    let p = w.objects.get_mut(&alice).unwrap().player_mut().unwrap();
    assert!(p.bag.inventory[0].as_ref().unwrap().expires_at.is_some());
    p.bag.inventory[0].as_mut().unwrap().expires_at = Some(1);
    p.bag.storage[0] = Some(UserItem {
        expires_at: Some(1),
        ..item
    });
    w.tick(1000);
    let p = w.objects[&alice].player().unwrap();
    assert!(p.bag.inventory[0].is_none());
    assert!(p.bag.storage[0].is_none());
}

#[test]
fn item_transfers_preserve_dye_expiry_and_durability() {
    let Some(w) = world() else {
        return;
    };
    let mut bag = crate::items::Bag::new();
    let mut item = UserItem::fresh(&w.data, 179, 1).unwrap();
    item.colour = 0xff123456;
    item.expires_at = Some(u64::MAX);
    item.durability = 1;
    let mut next = 0;
    assert!(bag.can_gain_item(&w.data, &item, i32::MAX));
    bag.gain_item(&w.data, item.clone(), &mut next);
    let transferred = bag.inventory[0].as_ref().unwrap();
    assert!(transferred.stacks_with(&item));
    assert!(!transferred.stacks_with(&UserItem::fresh(&w.data, 179, 1).unwrap()));
    let stored = bag.to_stored();
    let restored = crate::items::Bag::from_stored(&stored, 0);
    assert!(restored.inventory[0].as_ref().unwrap().stacks_with(&item));
}

#[test]
fn harvesting_one_group_share_does_not_hide_another_members_share() {
    let Some(mut w) = world() else {
        return;
    };
    let alice = w.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = w.add_player(2, 2, &test_character("Bob")).unwrap();
    w.tick(0);
    let loc = w.objects[&alice].location;
    let map = w.objects[&alice].map;
    w.teleport(bob, loc);
    w.group_switch(alice, true);
    w.group_switch(bob, true);
    w.group_invite(alice, "Bob".into());
    w.group_response(bob, true);
    let cell = Direction::ALL
        .iter()
        .map(|d| loc.step(*d, 1))
        .find(|p| w.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    let chicken = w.test_spawn_ai(1, map, cell).unwrap();
    w.tick(1);
    w.test_damage(chicken, alice, 100_000);
    w.tick(2);
    assert_eq!(
        w.objects[&chicken].monster().unwrap().harvest_drops.len(),
        2
    );
    drain(&mut w);
    let dir = Direction::from_points(loc, cell);
    for now in [602, 1202, 1802] {
        w.tick(now);
        w.harvest(alice, dir);
    }
    w.tick(1803);
    let messages = std::mem::take(&mut w.outgoing);
    assert!(messages
        .iter()
        .any(|m| matches!(m,Outgoing::To(1,ServerMessage::ObjectHarvested{id})if *id==chicken)));
    assert!(!messages
        .iter()
        .any(|m| matches!(m,Outgoing::To(2,ServerMessage::ObjectHarvested{id})if *id==chicken)));
    assert!(w.objects[&chicken]
        .monster()
        .unwrap()
        .harvest_drops
        .contains_key(&2));
    assert!(w.objects[&chicken].monster().unwrap().dead_time > 300_000);
    w.harvest(bob, dir);
    w.tick(1804);
    assert!(w.objects[&chicken]
        .monster()
        .unwrap()
        .harvest_drops
        .is_empty());
    assert!(w.objects[&chicken].monster().unwrap().dead_time < 100_000);
}
