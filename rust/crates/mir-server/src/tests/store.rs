//! In-game store tests (Zircon `MarketPlaceStoreBuy` / `StoreInfo`).

use super::*;
use crate::world::store::{CURRENCY_GAME_GOLD, CURRENCY_HUNT_GOLD};

/// A store row whose item stacks, so the count paths can be exercised.
fn stackable_row(world: &World) -> (u32, i32, u64) {
    let row = world
        .data
        .stores
        .iter()
        .find(|s| {
            s.available
                && s.price > 0
                && world
                    .data
                    .items
                    .get(&s.item)
                    .is_some_and(|d| d.stack_size > 1)
        })
        .expect("a stackable store row");
    (row.index as u32, row.item, row.price as u64)
}

fn currency_total(world: &World, id: ObjectId, currency_type: i32) -> i64 {
    let idx = world
        .data
        .currencies
        .iter()
        .find(|c| c.currency_type == currency_type)
        .map(|c| c.index)
        .expect("that currency");
    world.objects[&id]
        .player()
        .and_then(|p| p.currencies.get(&idx).copied())
        .unwrap_or(0)
}

#[test]
fn the_store_loads_and_its_filters_split() {
    let Some(world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    // The pack ships 92 rows; every one of them resolves to a real item.
    assert_eq!(world.data.stores.len(), 92);
    let stock = world.store_stock();
    assert_eq!(stock.len(), 92, "every row should name an item that exists");

    // Zircon splits `Filter` on commas and trims, so no tag keeps a space.
    let row = world
        .data
        .stores
        .iter()
        .find(|s| s.filters.len() > 1)
        .expect("a multi-tag row");
    assert!(row.filters.iter().all(|f| f.trim() == f && !f.is_empty()));

    // Torches are tagged as such in this pack, which is what the drop-down
    // is built from.
    assert!(world
        .data
        .stores
        .iter()
        .any(|s| s.filters.iter().any(|f| f == "Torch")));
}

#[test]
fn buying_moves_currency_and_goods_exactly_once() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    world.tick(0);
    drain(&mut world);
    let (index, item, price) = stackable_row(&world);

    // With no game gold at all, nothing moves.
    world.store_buy(alice, index, 2, false);
    assert_eq!(world.test_bag(alice).1, vec![]);
    assert_eq!(currency_total(&world, alice, CURRENCY_GAME_GOLD), 0);

    // Funded, the same purchase takes exactly the cost and yields exactly
    // the count.
    let funds = price as i64 * 10;
    world.test_set_currency_by_type(alice, CURRENCY_GAME_GOLD, funds);
    drain(&mut world);
    world.store_buy(alice, index, 2, false);
    assert_eq!(
        currency_total(&world, alice, CURRENCY_GAME_GOLD),
        funds - price as i64 * 2
    );
    assert_eq!(world.test_bag(alice).1, vec![(item, 2)]);

    // A second purchase tops up the same stack rather than duplicating.
    world.store_buy(alice, index, 3, false);
    assert_eq!(
        currency_total(&world, alice, CURRENCY_GAME_GOLD),
        funds - price as i64 * 5
    );
    assert_eq!(world.test_bag(alice).1, vec![(item, 5)]);

    // A zero count is Zircon's early return: nothing at all happens.
    let before = currency_total(&world, alice, CURRENCY_GAME_GOLD);
    world.store_buy(alice, index, 0, false);
    assert_eq!(currency_total(&world, alice, CURRENCY_GAME_GOLD), before);
    assert_eq!(world.test_bag(alice).1, vec![(item, 5)]);

    // An unknown row does nothing either.
    world.store_buy(alice, 99_999, 1, false);
    assert_eq!(currency_total(&world, alice, CURRENCY_GAME_GOLD), before);
}

#[test]
fn an_unavailable_row_is_refused_and_hunt_gold_falls_back_to_price() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    world.tick(0);
    drain(&mut world);
    let (index, item, price) = stackable_row(&world);

    // Turn the row off the way an operator would, and it is refused with a
    // line rather than silently.
    if let Some(row) = world
        .data
        .stores
        .iter_mut()
        .find(|s| s.index as u32 == index)
    {
        row.available = false;
    }
    world.test_set_currency_by_type(alice, CURRENCY_GAME_GOLD, price as i64 * 10);
    drain(&mut world);
    world.store_buy(alice, index, 1, false);
    let said = drain(&mut world);
    assert!(said
        .iter()
        .any(|m| matches!(m, ServerMessage::Say { text, .. } if text.contains("not available"))));
    assert_eq!(world.test_bag(alice).1, vec![]);

    // Back on, and paid for in hunt gold. Every row in this pack has a zero
    // `HuntGoldPrice`, which Zircon reads as "costs `Price` in hunt gold"
    // rather than "cannot be bought with hunt gold".
    if let Some(row) = world
        .data
        .stores
        .iter_mut()
        .find(|s| s.index as u32 == index)
    {
        row.available = true;
    }
    let hunt = price as i64 * 4;
    world.test_set_currency_by_type(alice, CURRENCY_HUNT_GOLD, hunt);
    drain(&mut world);
    world.store_buy(alice, index, 2, true);
    assert_eq!(
        currency_total(&world, alice, CURRENCY_HUNT_GOLD),
        hunt - price as i64 * 2,
        "hunt gold pays the game-gold price when HuntGoldPrice is 0"
    );
    // Game gold is untouched by a hunt-gold purchase.
    assert_eq!(
        currency_total(&world, alice, CURRENCY_GAME_GOLD),
        price as i64 * 10
    );
    assert_eq!(world.test_bag(alice).1, vec![(item, 2)]);
}

#[test]
fn timed_goods_carry_an_expiry_and_are_swept() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    world.tick(0);
    drain(&mut world);

    // The pack's timed rows are the temporary mounts and roses.
    let timed = world
        .data
        .stores
        .iter()
        .find(|s| s.duration > 0 && s.available)
        .cloned()
        .expect("a timed store row");
    world.test_set_currency_by_type(alice, CURRENCY_GAME_GOLD, timed.price as i64 * 4);
    drain(&mut world);
    world.store_buy(alice, timed.index as u32, 1, false);

    let stamped = world.objects[&alice]
        .player()
        .unwrap()
        .bag
        .inventory
        .iter()
        .flatten()
        .find(|i| i.info == timed.item)
        .expect("the bought item")
        .expires_at
        .expect("a timed item should carry an expiry");
    assert!(stamped > crate::world::store::now_secs());

    // A sweep now leaves it alone; backdating it takes it away.
    world.expire_items(alice);
    assert_eq!(world.test_bag(alice).1.len(), 1);

    if let Some(it) = world
        .objects
        .get_mut(&alice)
        .and_then(|o| o.player_mut())
        .and_then(|p| {
            p.bag
                .inventory
                .iter_mut()
                .flatten()
                .find(|i| i.info == timed.item)
        })
    {
        it.expires_at = Some(1);
    }
    world.expire_items(alice);
    assert_eq!(world.test_bag(alice).1, vec![], "an expired item is gone");

    // An untimed purchase never gets an expiry.
    let (index, item, price) = stackable_row(&world);
    world.test_set_currency_by_type(alice, CURRENCY_GAME_GOLD, price as i64 * 4);
    drain(&mut world);
    world.store_buy(alice, index, 1, false);
    assert!(world.objects[&alice]
        .player()
        .unwrap()
        .bag
        .inventory
        .iter()
        .flatten()
        .find(|i| i.info == item)
        .is_some_and(|i| i.expires_at.is_none()));
}

#[test]
fn store_search_matches_by_tag_and_name() {
    use crate::world::store::store_matches;
    let entry = mir_proto::StoreEntry {
        index: 1,
        item: 1,
        stack_size: 1,
        price: 100,
        hunt_gold_price: 0,
        filters: vec!["Torch".into(), "Temporary".into(), "Warrior".into()],
        available: true,
        duration: 0,
    };
    // No tag and no name matches everything.
    assert!(store_matches(&entry, "Iron Torch", None, ""));
    // A tag it carries matches; one it does not carry does not.
    assert!(store_matches(&entry, "Iron Torch", Some("Torch"), ""));
    assert!(store_matches(&entry, "Iron Torch", Some("torch"), ""));
    assert!(!store_matches(&entry, "Iron Torch", Some("Mount"), ""));
    // The name test is a case-insensitive substring, as Zircon's is.
    assert!(store_matches(&entry, "Iron Torch", None, "torch"));
    assert!(store_matches(&entry, "Iron Torch", None, "IRON"));
    assert!(!store_matches(&entry, "Iron Torch", None, "sword"));
    // Both must hold.
    assert!(!store_matches(&entry, "Iron Torch", Some("Mount"), "torch"));
}

#[test]
fn store_flags_use_account_level_and_separate_stacks() {
    let Some(mut world) = world() else { return };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let (index, info, price) = stackable_row(&world);
    world.test_set_currency_by_type(alice, CURRENCY_GAME_GOLD, price as i64 * 10);
    world.test_set_currency_by_type(alice, CURRENCY_HUNT_GOLD, price as i64 * 10);
    world.test_give_item(alice, info, 1);
    world.store_buy_for_account(alice, index, 1, false, 39);
    world.store_buy_for_account(alice, index, 1, false, 40);
    world.store_buy_for_account(alice, index, 1, true, 99);
    let p = world.objects[&alice].player().unwrap();
    let items: Vec<_> = p
        .bag
        .inventory
        .iter()
        .flatten()
        .filter(|i| i.info == info)
        .collect();
    assert_eq!(items.len(), 3);
    assert_eq!(items.iter().find(|i| !i.worthless).unwrap().count, 1);
    assert_eq!(items.iter().find(|i| i.bound).unwrap().count, 2);
    assert_eq!(
        items
            .iter()
            .find(|i| i.worthless && !i.bound)
            .unwrap()
            .count,
        1
    );
    let item = items.iter().find(|i| i.bound).unwrap();
    assert!(item.locked && item.worthless);
    let restored: crate::items::UserItem =
        serde_json::from_str(&serde_json::to_string(item).unwrap()).unwrap();
    assert!(restored.bound && restored.locked && restored.worthless);
    assert!(restored.instance().bound);
    let mut legacy = serde_json::to_value(item).unwrap();
    for key in ["bound", "locked", "worthless"] {
        legacy.as_object_mut().unwrap().remove(key);
    }
    let legacy: crate::items::UserItem = serde_json::from_value(legacy).unwrap();
    assert!(!legacy.bound && !legacy.locked && !legacy.worthless);
}

#[test]
fn locked_drops_are_refused_and_unlocked_bound_drops_never_become_public() {
    use mir_proto::Grid;
    let Some(mut world) = world() else { return };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    let info = world
        .data
        .items
        .values()
        .find(|d| d.can_drop && d.stack_size > 1 && d.item_type == 1)
        .unwrap()
        .index;
    world.test_give_item(alice, info, 2);
    let slot = world.test_slot_of(alice, info).unwrap();
    let item = world
        .objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[slot as usize]
        .as_mut()
        .unwrap();
    item.bound = true;
    item.locked = true;
    world.item_drop(alice, slot, 1);
    assert_eq!(
        world.objects[&alice].player().unwrap().bag.inventory[slot as usize]
            .as_ref()
            .unwrap()
            .count,
        2
    );
    world.item_lock(alice, Grid::Inventory, slot, false);
    world.item_drop(alice, slot, 1);
    let ground = world
        .objects
        .iter()
        .find_map(|(id, o)| {
            matches!(&o.kind, crate::world::Kind::Item(i) if i.item.info == info).then_some(*id)
        })
        .unwrap();
    let loc = world.objects[&ground].location;
    world.teleport(bob, loc);
    world.now += 1_000_000;
    world.pick_up(bob);
    assert!(
        world.objects.contains_key(&ground),
        "bound ownership survives public share timeout"
    );
    world.teleport(alice, loc);
    world.pick_up(alice);
    assert!(!world.objects.contains_key(&ground));
    assert_eq!(
        world.objects[&alice].player().unwrap().bag.inventory[slot as usize]
            .as_ref()
            .unwrap()
            .count,
        2
    );
}

#[test]
fn npc_sell_respects_worthless_and_lock_without_rejecting_bound_items() {
    use mir_proto::Grid;
    let Some(mut world) = world() else { return };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    world.test_give_item(alice, 179, 1); // meat shop page accepts this ordinary meat
    world.test_open_page(alice, 4);
    let slot = world.test_slot_of(alice, 179).unwrap();
    let item = world
        .objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[slot as usize]
        .as_mut()
        .unwrap();
    item.worthless = true;
    world.npc_sell(alice, vec![(slot, 1)]);
    assert!(world.test_slot_of(alice, 179).is_some());
    let item = world
        .objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[slot as usize]
        .as_mut()
        .unwrap();
    item.worthless = false;
    item.bound = true;
    item.locked = true;
    world.npc_sell(alice, vec![(slot, 1)]);
    assert!(world.test_slot_of(alice, 179).is_some());
    world.item_lock(alice, Grid::Inventory, slot, false);
    world.npc_sell(alice, vec![(slot, 1)]);
    assert!(
        world.test_slot_of(alice, 179).is_none(),
        "Bound alone does not prevent NPC sales"
    );
}

#[test]
fn npc_sell_processes_multiple_selected_slots_once() {
    let Some(mut world) = world() else { return };
    let alice = world.add_player(1, 1, &test_character("Seller")).unwrap();
    for info in [179, 180, 181] {
        world.test_give_item(alice, info, 1);
    }
    world.test_open_page(alice, 4); // Meat shop accepts these item types.
    let slots: Vec<_> = [179, 180, 181]
        .into_iter()
        .map(|info| world.test_slot_of(alice, info).unwrap())
        .collect();
    world
        .objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[slots[2] as usize]
        .as_mut()
        .unwrap()
        .locked = true;
    let gold = world.objects[&alice].player().unwrap().bag.gold;
    drain(&mut world);

    world.npc_sell(
        alice,
        vec![(slots[0], 1), (slots[1], 1), (slots[0], 1), (slots[2], 1)],
    );
    assert!(world.test_slot_of(alice, 179).is_none());
    assert!(world.test_slot_of(alice, 180).is_none());
    assert!(world.test_slot_of(alice, 181).is_some());
    assert!(world.objects[&alice].player().unwrap().bag.gold > gold);
    assert!(drain(&mut world).iter().any(|m| matches!(m,
        ServerMessage::Chat { text } if text.starts_with("Sold 2 item(s)")
    )));
}

#[test]
fn npc_shop_uses_page_currency_and_can_sell_part_of_stack() {
    let Some(mut world) = world() else { return };
    assert!(world
        .data
        .npc_pages
        .values()
        .any(|page| page.currency.is_some()));
    let currency = world
        .data
        .currencies
        .iter()
        .find(|currency| currency.currency_type == 2)
        .expect("Hunt Gold currency")
        .clone();
    let (page_id, info, rate) = world
        .data
        .npc_pages
        .values()
        .filter(|page| page.dialog_type == 1)
        .find_map(|page| {
            page.goods.iter().find_map(|&(info, rate)| {
                let item = world.data.items.get(&info)?;
                (item.stack_size >= 2 && item.can_sell && page.types.contains(&item.item_type))
                    .then_some((page.index, info, rate))
            })
        })
        .expect("shop selling a stackable item");
    world.data.npc_pages.get_mut(&page_id).unwrap().currency = Some(currency.index);
    let item = world.data.items[&info].clone();
    let base = ((item.price as f64 * rate).round() as u64).max(1);
    let cost = (base as f64 * currency.exchange_rate).max(1.0) as u64 * 2;
    let alice = world.add_player(1, 1, &test_character("Seller")).unwrap();
    world.test_set_gold(alice, 0);
    world.test_set_currency_by_type(alice, currency.currency_type, 100_000);
    world.test_open_page(alice, page_id);
    world.npc_buy(alice, info, 2);
    let slot = world.test_slot_of(alice, info).unwrap();
    let p = world.objects[&alice].player().unwrap();
    assert_eq!(p.bag.gold, 0);
    assert_eq!(p.currencies[&currency.index], 100_000 - cost as i64);
    let mut part = p.bag.inventory[slot as usize].as_ref().unwrap().clone();
    assert_eq!(part.count, 2);
    assert!(part.locked, "NPC purchases start locked");
    part.count = 1;
    let sale = (crate::items::sell_price(&item, &part) as f64 * currency.exchange_rate) as i64;

    world.item_lock(alice, mir_proto::Grid::Inventory, slot, false);
    world.npc_sell(alice, vec![(slot, 1)]);
    let p = world.objects[&alice].player().unwrap();
    assert_eq!(p.bag.inventory[slot as usize].as_ref().unwrap().count, 1);
    assert_eq!(p.bag.gold, 0);
    assert_eq!(p.currencies[&currency.index], 100_000 - cost as i64 + sale);
    assert!(drain(&mut world).iter().any(|message| matches!(message,
        ServerMessage::Chat { text } if text.contains(&currency.name)
    )));

    world.test_set_currency_by_type(alice, currency.currency_type, 0);
    world.test_set_gold(alice, 100_000);
    world.npc_buy(alice, info, 1);
    let p = world.objects[&alice].player().unwrap();
    assert_eq!(p.bag.inventory[slot as usize].as_ref().unwrap().count, 1);
    assert_eq!(p.bag.gold, 100_000, "Gold cannot pay a non-gold shop");
}

#[test]
fn installed_fame_point_shop_spends_saved_currency_not_focus_or_gold() {
    let Some(mut world) = world() else { return };
    let page = &world.data.npc_pages[&338];
    assert_eq!(page.currency, Some(4));
    assert!(
        page.types.is_empty(),
        "installed Fame Point page does not buy items"
    );
    let (info, rate) = page.goods[0];
    let currency = world
        .data
        .currencies
        .iter()
        .find(|c| c.index == 4)
        .unwrap()
        .clone();
    assert_eq!(currency.name, "Fame Point");
    let base = ((world.data.items[&info].price as f64 * rate).round() as u64).max(1);
    let cost = (base as f64 * currency.exchange_rate).max(1.0) as i64;
    let mut saved = test_character("FameBuyer");
    let buyer = world.add_player(1, 1, &saved).unwrap();
    let focus = world.objects[&buyer].player().unwrap().fp;
    world.test_set_gold(buyer, 1_000_000);
    world.test_set_currency_by_type(buyer, currency.currency_type, 0);
    world.test_open_page(buyer, 338);
    world.npc_buy(buyer, info, 1);
    assert!(world.test_slot_of(buyer, info).is_none());
    assert_eq!(world.objects[&buyer].player().unwrap().bag.gold, 1_000_000);

    world.test_set_currency_by_type(buyer, currency.currency_type, cost + 10);
    world.npc_buy(buyer, info, 1);
    let player = world.objects[&buyer].player().unwrap();
    assert_eq!(player.currencies[&4], 10);
    assert_eq!(player.fp, focus);
    assert_eq!(player.bag.gold, 1_000_000);
    assert!(world.test_slot_of(buyer, info).is_some());
    world.snapshot(buyer, &mut saved);
    let restored: crate::accounts::CharacterRecord =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(restored.currencies.contains(&(4, 10)));
}

#[test]
fn walking_out_of_npc_range_closes_shop_and_blocks_late_transactions() {
    let Some(mut world) = world() else { return };
    let buyer = world.add_player(1, 1, &test_character("Walker")).unwrap();
    world.test_give_item(buyer, 179, 1);
    let slot = world.test_slot_of(buyer, 179).unwrap();
    world.test_set_gold(buyer, 100_000);
    world.test_open_page(buyer, 4);
    drain(&mut world);
    world.teleport(buyer, mir_proto::Point { x: 0, y: 0 });
    assert!(world.objects[&buyer].player().unwrap().npc.is_none());
    assert!(drain(&mut world)
        .iter()
        .any(|message| matches!(message, ServerMessage::NpcClose)));

    world.npc_sell(buyer, vec![(slot, 1)]);
    world.npc_buy(buyer, 179, 1);
    let player = world.objects[&buyer].player().unwrap();
    assert_eq!(player.bag.gold, 100_000);
    assert_eq!(
        player.bag.inventory[slot as usize].as_ref().unwrap().count,
        1
    );
}

#[test]
fn npc_purchased_equipment_is_locked_and_non_refinable() {
    let Some(mut world) = world() else { return };
    let (page, info, rate) = world
        .data
        .npc_pages
        .values()
        .filter(|page| page.currency.is_none())
        .find_map(|page| {
            page.goods.iter().find_map(|&(info, rate)| {
                let item = world.data.items.get(&info)?;
                matches!(
                    item.item_type,
                    mir_proto::item_type::WEAPON
                        | mir_proto::item_type::ARMOUR
                        | mir_proto::item_type::HELMET
                        | mir_proto::item_type::RING
                )
                .then_some((page.index, info, rate))
            })
        })
        .expect("gold shop with equipment");
    let price = ((world.data.items[&info].price as f64 * rate).round() as u64).max(1);
    let buyer = world
        .add_player(1, 1, &test_character("GearBuyer"))
        .unwrap();
    world.test_set_gold(buyer, price + 100);
    world.test_open_page(buyer, page);
    world.npc_buy(buyer, info, 1);
    let slot = world.test_slot_of(buyer, info).unwrap();
    let item = world.objects[&buyer].player().unwrap().bag.inventory[slot as usize]
        .as_ref()
        .unwrap();
    assert!(item.locked);
    assert!(item.non_refinable);
}

#[test]
fn bound_items_cannot_leave_account_through_mail_market_or_trade() {
    use mir_proto::Grid;
    let Some(mut world) = world() else { return };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    let info = world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .unwrap()
        .index;
    world.test_give_item(alice, info, 3);
    let slot = world.test_slot_of(alice, info).unwrap();
    world
        .objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[slot as usize]
        .as_mut()
        .unwrap()
        .bound = true;
    world.market_consign(alice, slot, 1, 100, String::new());
    assert!(world.market_store.listings.is_empty());
    world.mail_send(
        alice,
        Some((2, "Bob".into())),
        "Gift".into(),
        String::new(),
        0,
        vec![(Grid::Inventory, slot, 1)],
    );
    assert!(world.mail_store.boxes.get(&2).is_none_or(|m| m.is_empty()));
    let loc = world.objects[&alice].location;
    let map = world.objects[&alice].map;
    let (dir, cell) = Direction::ALL
        .iter()
        .map(|d| (*d, loc.step(*d, 1)))
        .find(|(_, p)| world.maps[&map].file.is_walkable(p.x, p.y))
        .unwrap();
    world.teleport(bob, cell);
    world.test_face(alice, dir);
    world.test_face(bob, dir.opposite());
    world.trade_request(alice);
    world.trade_response(bob, true);
    assert!(world.objects[&alice].player().unwrap().trade.is_some());
    world.trade_add_item(alice, Grid::Inventory, slot, 1);
    assert!(world.objects[&alice]
        .player()
        .unwrap()
        .trade
        .as_ref()
        .unwrap()
        .items
        .is_empty());
    assert_eq!(
        world.objects[&alice].player().unwrap().bag.inventory[slot as usize]
            .as_ref()
            .unwrap()
            .count,
        3
    );
    // Lock and Worthless protect against destruction / NPC value, not player trade.
    let item = world
        .objects
        .get_mut(&alice)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[slot as usize]
        .as_mut()
        .unwrap();
    item.bound = false;
    item.locked = true;
    item.worthless = true;
    world.trade_add_item(alice, Grid::Inventory, slot, 1);
    assert_eq!(
        world.objects[&alice]
            .player()
            .unwrap()
            .trade
            .as_ref()
            .unwrap()
            .items
            .len(),
        1
    );
}
