use mir_proto::{item_type, slot, Class, Grid};

use super::*;

#[test]
fn poison_merges_between_bag_and_equipment_up_to_999() {
    let Some(mut world) = world() else { return };
    let mut character = test_character("PoisonStackTester");
    character.class = Class::Taoist;
    let me = world.add_player(1, 1, &character).unwrap();
    let player = world.objects[&me].player().unwrap();
    let poison = world
        .data
        .items
        .values()
        .find(|def| {
            def.item_type == item_type::POISON
                && crate::items::can_use(def, player.class, player.gender, player.level).is_ok()
        })
        .expect("usable poison")
        .index;
    assert!(world.data.items[&poison].stack_size >= 999);

    world.test_give_item(me, poison, 600);
    let bag_slot = world.test_slot_of(me, poison).unwrap();
    world.item_move(
        me,
        Grid::Inventory,
        bag_slot,
        Grid::Equipment,
        slot::POISON as u8,
    );

    world.test_give_item(me, poison, 300);
    let bag_slot = world.test_slot_of(me, poison).unwrap();
    world.item_move(
        me,
        Grid::Inventory,
        bag_slot,
        Grid::Equipment,
        slot::POISON as u8,
    );
    let bag = &world.objects[&me].player().unwrap().bag;
    assert_eq!(bag.equipment[slot::POISON].as_ref().unwrap().count, 900);
    assert!(bag.inventory[bag_slot as usize].is_none());

    world.test_give_item(me, poison, 300);
    let bag_slot = world.test_slot_of(me, poison).unwrap();
    world.item_move(
        me,
        Grid::Inventory,
        bag_slot,
        Grid::Equipment,
        slot::POISON as u8,
    );
    let bag = &world.objects[&me].player().unwrap().bag;
    assert_eq!(bag.equipment[slot::POISON].as_ref().unwrap().count, 999);
    assert_eq!(
        bag.inventory[bag_slot as usize].as_ref().unwrap().count,
        201
    );

    world.item_move(
        me,
        Grid::Equipment,
        slot::POISON as u8,
        Grid::Inventory,
        bag_slot,
    );
    let bag = &world.objects[&me].player().unwrap().bag;
    assert_eq!(bag.equipment[slot::POISON].as_ref().unwrap().count, 201);
    assert_eq!(
        bag.inventory[bag_slot as usize].as_ref().unwrap().count,
        999
    );

    world.item_move(
        me,
        Grid::Equipment,
        slot::POISON as u8,
        Grid::Inventory,
        bag_slot,
    );
    let bag = &world.objects[&me].player().unwrap().bag;
    assert_eq!(bag.equipment[slot::POISON].as_ref().unwrap().count, 201);
    assert_eq!(
        bag.inventory[bag_slot as usize].as_ref().unwrap().count,
        999
    );
}
