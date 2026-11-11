use super::*;
use crate::items::UserItem;
use mir_proto::Class;

fn fixture() -> Option<(World, ObjectId, i32)> {
    let mut w = world()?;
    let id = w.add_player(1, 1, &test_character("Crafter")).unwrap();
    w.tick(0);
    let page = w.test_page_with_dialog_type(14);
    let npc = w.test_open_npc(id, 137);
    w.objects.get_mut(&id).unwrap().player_mut().unwrap().npc = Some((npc, page));
    let template = w
        .data
        .items
        .values()
        .find(|d| d.effect == 60)
        .unwrap()
        .index;
    let yellow = w
        .data
        .items
        .values()
        .find(|d| d.effect == 70 && d.shape > 0)
        .unwrap()
        .index;
    let mut base = UserItem::fresh(&w.data, template, 2).unwrap();
    base.id = 100;
    let mut material = UserItem::fresh(&w.data, yellow, 2).unwrap();
    material.id = 101;
    let p = w.objects.get_mut(&id).unwrap().player_mut().unwrap();
    p.bag.inventory.fill(None);
    p.bag.inventory[0] = Some(base);
    p.bag.inventory[1] = Some(material);
    p.bag.gold = 100_000_000;
    p.max_bag = 100_000;
    p.next_item_id = 102;
    Some((w, id, page))
}

#[test]
fn crafting_rejects_stale_duplicate_wrong_colour_and_unfunded_requests_atomically() {
    let Some((mut w, id, _page)) = fixture() else {
        return;
    };
    for case in 0..6 {
        w.test_set_gold(id, if case == 4 { 0 } else { 100_000_000 });
        if case == 5 {
            w.objects.get_mut(&id).unwrap().player_mut().unwrap().npc = None;
        }
        let before =
            serde_json::to_string(&w.objects[&id].player().unwrap().bag.inventory).unwrap();
        let gold = w.test_bag(id).0;
        let mut materials = [None; 6];
        let base = if case == 0 { (0, 999) } else { (0, 100) };
        match case {
            1 => materials[0] = Some((0, 100)),
            2 => materials[1] = Some((1, 101)),
            3 => {
                materials[0] = Some((1, 101));
                materials[1] = Some((1, 101));
            }
            _ => {}
        }
        w.npc_weapon_craft(id, base, materials, Class::Warrior);
        assert_eq!(
            before,
            serde_json::to_string(&w.objects[&id].player().unwrap().bag.inventory).unwrap(),
            "case {case}"
        );
        assert_eq!(gold, w.test_bag(id).0);
    }
}

#[test]
fn template_crafts_all_classes_and_reroll_keeps_identity_and_durability() {
    for class in [
        Class::Warrior,
        Class::Wizard,
        Class::Taoist,
        Class::Assassin,
    ] {
        let Some((mut w, id, _)) = fixture() else {
            return;
        };
        w.npc_weapon_craft(
            id,
            (0, 100),
            [Some((1, 101)), None, None, None, None, None],
            class,
        );
        let p = w.objects[&id].player().unwrap();
        assert_eq!(p.bag.gold, 99_000_000);
        assert_eq!(p.bag.inventory[0].as_ref().unwrap().count, 1);
        assert_eq!(p.bag.inventory[1].as_ref().unwrap().count, 1);
        let weapon = p.bag.inventory[2].as_ref().unwrap().clone();
        assert!(!weapon.added.is_empty());
        assert_eq!(
            w.data.items[&weapon.info].effect,
            match class {
                Class::Warrior => 61,
                Class::Wizard => 63,
                Class::Taoist => 64,
                Class::Assassin => 65,
            }
        );
        w.npc_weapon_craft(id, (2, weapon.id), [None; 6], class);
        let rerolled = w.objects[&id].player().unwrap().bag.inventory[2]
            .as_ref()
            .unwrap();
        assert_eq!(rerolled.id, weapon.id);
        assert_eq!(rerolled.durability, weapon.durability);
        assert_eq!(rerolled.level, 1);
        assert!(rerolled.added.is_empty());
        let cost = match w.data.items[&weapon.info].rarity {
            0 => 30_000_000,
            1 => 60_000_000,
            _ => 80_000_000,
        };
        assert_eq!(w.test_bag(id).0, 99_000_000 - cost);
    }
}

#[test]
fn full_bag_template_crafting_consumes_nothing() {
    let Some((mut w, id, _)) = fixture() else {
        return;
    };
    let p = w.objects.get_mut(&id).unwrap().player_mut().unwrap();
    let filler = p.bag.inventory[0].clone();
    for slot in &mut p.bag.inventory[2..] {
        *slot = filler.clone();
    }
    let before = serde_json::to_string(&p.bag.inventory).unwrap();
    w.npc_weapon_craft(id, (0, 100), [None; 6], Class::Warrior);
    assert_eq!(w.test_bag(id).0, 100_000_000);
    assert_eq!(
        before,
        serde_json::to_string(&w.objects[&id].player().unwrap().bag.inventory).unwrap()
    );
}

#[test]
fn reroll_preserves_only_enhancements_even_when_sources_share_a_stat() {
    use crate::items::StatSource;
    let Some((mut w, id, _)) = fixture() else {
        return;
    };
    let mut weapon = UserItem::fresh(&w.data, 968, 1).unwrap();
    weapon.id = 200;
    weapon.refinable = true;
    weapon.colour = 0xff123456;
    weapon.expires_at = Some(u64::MAX);
    weapon.weapon_experience_tenths = 123;
    weapon.add_stat(9, 7, StatSource::Enhancement);
    weapon.add_stat(9, 11, StatSource::Added);
    weapon.add_stat(9, 3, StatSource::Refine);
    weapon.add_stat(10, 4, StatSource::Other);
    assert_eq!(weapon.bonus(9), 21);
    w.objects
        .get_mut(&id)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[0] = Some(weapon.clone());
    w.npc_weapon_craft(id, (0, 200), [None; 6], Class::Warrior);
    let rerolled = w.objects[&id].player().unwrap().bag.inventory[0]
        .as_ref()
        .unwrap();
    assert_eq!(rerolled.bonus_totals(), vec![(9, 7)]);
    assert_eq!(rerolled.instance().enhancements, vec![(9, 7)]);
    assert!(!rerolled.refinable);
    assert!(!rerolled.non_refinable);
    assert_eq!(rerolled.weapon_experience_tenths, 123);
    assert_eq!(rerolled.colour, weapon.colour);
    assert_eq!(rerolled.expires_at, weapon.expires_at);
    let saved = serde_json::to_string(rerolled).unwrap();
    let loaded: UserItem = serde_json::from_str(&saved).unwrap();
    assert!(loaded.stacks_with(rerolled));
    // Returned weapons cannot enter the furnace until readiness is earned.
    let p = w.objects.get_mut(&id).unwrap().player_mut().unwrap();
    p.bag.equipment[0] = p.bag.inventory[0].take();
    let gold = p.bag.gold;
    let page = w.test_page_with_dialog_type(3);
    w.test_open_page(id, page);
    w.npc_refine(
        id,
        mir_proto::refine_type::DC,
        mir_proto::refine_quality::RUSH,
        vec![],
        vec![],
        vec![],
    );
    assert!(w.objects[&id].player().unwrap().refines.is_empty());
    assert_eq!(w.test_bag(id).0, gold);
    w.gain_experience_test(id, 3_000_000 - 123);
    let weapon = w.objects[&id].player().unwrap().bag.equipment[0]
        .as_ref()
        .unwrap();
    assert!(weapon.refinable);
    assert_eq!(weapon.level, 2);
    assert!(drain(&mut w).iter().any(|m| matches!(m,ServerMessage::ItemChanged {grid:mir_proto::Grid::Equipment,item:Some(i),..} if i.refinable)));
}

#[test]
fn legacy_item_pairs_migrate_without_losing_stats_and_sources_do_not_stack() {
    use crate::items::StatSource;
    let old = r#"{"id":1,"info":968,"count":1,"durability":100,"max_durability":200,"added":[[9,5],[14,2]],"level":3}"#;
    let mut item: UserItem = serde_json::from_str(old).unwrap();
    assert_eq!(item.bonus_totals(), vec![(9, 5), (14, 2)]);
    assert!(item.refinable); // Preserve the old save's ability to refine.
    assert!(!item.non_refinable);
    assert!(item.added.iter().all(|s| s.source == StatSource::Legacy));
    let legacy = item.clone();
    item.added[0].source = StatSource::Enhancement;
    assert_eq!(item.bonus_totals(), legacy.bonus_totals());
    assert!(!item.stacks_with(&legacy));
    let mut flags = item.clone();
    flags.non_refinable = true;
    assert!(!item.stacks_with(&flags));
    let restored: UserItem = serde_json::from_str(&serde_json::to_string(&item).unwrap()).unwrap();
    assert_eq!(restored.added, item.added);
}

#[test]
fn weapon_readiness_preserves_fractional_progress_and_obeys_permanent_restriction() {
    let json = r#"{"id":1,"info":968,"count":1,"durability":100,"max_durability":100,"level":1,"refinable":false,"weapon_experience_tenths":2999998}"#;
    let mut weapon: UserItem = serde_json::from_str(json).unwrap();
    assert!(!weapon.gain_weapon_experience(1));
    assert!(weapon.gain_weapon_experience(1));
    assert!(weapon.refinable);
    assert_eq!(weapon.level, 2);
    assert_eq!(weapon.weapon_experience_tenths, 0);
    assert!(!weapon.gain_weapon_experience(u64::MAX));
    weapon.refinable = false;
    weapon.non_refinable = true;
    assert!(!weapon.gain_weapon_experience(u64::MAX));
    assert_eq!(weapon.level, 2);
    weapon.non_refinable = false;
    weapon.level = 16;
    assert!(weapon.gain_weapon_experience(20_000_000));
    assert_eq!(weapon.level, 17);
    assert!(!weapon.refinable);
    assert!(!weapon.gain_weapon_experience(u64::MAX));
}
