use super::*;
use crate::items::{StatSource, UserItem};
use mir_formats::mirdb::stat;
use mir_proto::refine_type;
use rand::{Rng, SeedableRng};

fn fixture() -> Option<(World, ObjectId, i32)> {
    let mut w = world()?;
    let id = w
        .add_player(1, 1, &test_character("AccessoryTester"))
        .unwrap();
    w.tick(0);
    let jewel = w
        .data
        .items
        .values()
        .find(|d| d.item_type == mir_proto::item_type::RING && d.rarity == 0)
        .expect("common ring in asset database")
        .index;
    let p = w.objects.get_mut(&id).unwrap().player_mut().unwrap();
    p.bag.inventory.fill(None);
    for slot in 0..3 {
        let mut item = UserItem::fresh(&w.data, jewel, 1).unwrap();
        item.id = slot as u32 + 100;
        p.bag.inventory[slot] = Some(item);
    }
    p.bag.gold = 2_000_000;
    Some((w, id, jewel))
}

fn page(w: &mut World, id: ObjectId, dialog_type: i32) {
    assert!(
        w.data
            .npc_pages
            .values()
            .any(|page| page.dialog_type == dialog_type),
        "asset database has no accessory page {dialog_type}"
    );
    let page = w.test_page_with_dialog_type(dialog_type);
    let npc = w.test_open_npc(id, 137);
    w.objects.get_mut(&id).unwrap().player_mut().unwrap().npc = Some((npc, page));
}

#[test]
fn duplicate_ring_unlocks_upgrade_and_reset_keeps_non_refine_bonus() {
    let Some((mut w, id, _)) = fixture() else {
        return;
    };
    page(&mut w, id, 12);
    w.npc_accessory_level(
        id,
        (Grid::Inventory, 0, 100),
        vec![(Grid::Inventory, 1, 101)],
    );
    let p = w.objects[&id].player().unwrap();
    let ring = p.bag.inventory[0].as_ref().unwrap();
    assert_eq!(ring.level, 1);
    assert!(ring.refinable);
    assert!(p.bag.inventory[1].is_none());

    page(&mut w, id, 11);
    w.npc_accessory_upgrade(id, (Grid::Inventory, 0, 100), refine_type::DC);
    let ring = w.objects[&id].player().unwrap().bag.inventory[0]
        .as_ref()
        .unwrap();
    assert!(!ring.refinable);
    assert_eq!(ring.bonus(stat::MAX_DC), 1);
    assert_eq!(ring.added[0].source, StatSource::Refine);

    // Other sources survive an accessory reset, as in Zircon.
    w.objects
        .get_mut(&id)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[0]
        .as_mut()
        .unwrap()
        .add_stat(stat::LUCK, 2, StatSource::Enhancement);
    page(&mut w, id, 13);
    w.npc_accessory_reset(id, (Grid::Inventory, 0, 100));
    let p = w.objects[&id].player().unwrap();
    let ring = p.bag.inventory[0].as_ref().unwrap();
    assert_eq!(ring.bonus(stat::MAX_DC), 0);
    assert_eq!(ring.bonus(stat::LUCK), 2);
    assert_eq!(p.bag.gold, 1_000_000);
}

#[test]
fn accessory_refine_rejects_stale_or_reused_material_without_charging() {
    let Some((mut w, id, _)) = fixture() else {
        return;
    };
    page(&mut w, id, 11);
    let ore_info = w
        .data
        .items
        .values()
        .find(|d| d.effect == 25)
        .expect("corundum ore in asset database")
        .index;
    let mut ore = UserItem::fresh(&w.data, ore_info, 1).unwrap();
    ore.id = 103;
    w.objects
        .get_mut(&id)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[3] = Some(ore);
    let before = serde_json::to_string(&w.objects[&id].player().unwrap().bag.inventory).unwrap();
    let gold = w.objects[&id].player().unwrap().bag.gold;
    w.npc_accessory_refine(
        id,
        (Grid::Inventory, 0, 999),
        (Grid::Inventory, 3, 103),
        vec![(Grid::Inventory, 1, 101)],
        refine_type::DC,
    );
    let chance = (100
        - w.objects[&id].player().unwrap().bag.inventory[3]
            .as_ref()
            .unwrap()
            .durability
            / 1000)
        .max(1);
    let seed = (0..100)
        .find(|seed| {
            let mut rng = rand::rngs::StdRng::seed_from_u64(*seed);
            rng.random_range(0..chance) < 30
        })
        .expect("success seed");
    w = w.with_seed(seed);
    w.npc_accessory_refine(
        id,
        (Grid::Inventory, 0, 100),
        (Grid::Inventory, 3, 103),
        vec![(Grid::Inventory, 0, 100)],
        refine_type::DC,
    );
    assert_eq!(
        before,
        serde_json::to_string(&w.objects[&id].player().unwrap().bag.inventory).unwrap()
    );
    assert_eq!(gold, w.objects[&id].player().unwrap().bag.gold);

    w.npc_accessory_refine(
        id,
        (Grid::Inventory, 0, 100),
        (Grid::Inventory, 3, 103),
        vec![(Grid::Inventory, 1, 101)],
        refine_type::DC,
    );
    let p = w.objects[&id].player().unwrap();
    assert_eq!(p.bag.gold, gold - 50_000);
    assert!(p.bag.inventory[1].is_none());
    assert!(p.bag.inventory[3].is_none());
    let ring = p.bag.inventory[0].as_ref().expect("seeded success");
    assert!(ring.bonus(stat::MAX_DC) == 1 || ring.bonus(stat::MAX_DC) == 2);
    assert_eq!(ring.added[0].source, StatSource::Added);
}

#[test]
fn failed_corundum_refine_destroys_only_target_and_ore() {
    let Some((mut w, id, _)) = fixture() else {
        return;
    };
    page(&mut w, id, 11);
    let ore_info = w
        .data
        .items
        .values()
        .find(|d| d.effect == 25)
        .unwrap()
        .index;
    let mut ore = UserItem::fresh(&w.data, ore_info, 1).unwrap();
    ore.id = 103;
    let chance = (100 - ore.durability / 1000).max(1);
    w.objects
        .get_mut(&id)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[3] = Some(ore);
    let seed = (0..100)
        .find(|seed| {
            let mut rng = rand::rngs::StdRng::seed_from_u64(*seed);
            rng.random_range(0..chance) >= 30
        })
        .expect("failure seed");
    w = w.with_seed(seed);
    w.npc_accessory_refine(
        id,
        (Grid::Inventory, 0, 100),
        (Grid::Inventory, 3, 103),
        Vec::new(),
        refine_type::DC,
    );
    let p = w.objects[&id].player().unwrap();
    assert!(p.bag.inventory[0].is_none());
    assert!(p.bag.inventory[3].is_none());
    assert!(p.bag.inventory[1].is_some());
    assert_eq!(p.bag.gold, 1_950_000);
}
