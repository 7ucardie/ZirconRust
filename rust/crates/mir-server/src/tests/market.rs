//! Marketplace tests (Zircon `MarketPlaceDialog` / `AuctionInfo`).

use super::*;

/// The newest `MarketSearchResults` the world sent.
fn last_search(world: &mut World) -> (u32, u32, Vec<mir_proto::MarketListing>) {
    drain(world)
        .into_iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::MarketSearchResults {
                total,
                page,
                results,
            } => Some((total, page, results)),
            _ => None,
        })
        .expect("a search result")
}

fn potion_of(world: &World) -> i32 {
    world
        .data
        .items
        .values()
        .find(|d| d.name == "Healing Potion")
        .unwrap()
        .index
}

#[test]
fn consign_buy_and_cancel_move_gold_and_items_exactly_once() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    drain(&mut world);
    let potion = potion_of(&world);
    world.test_give_item(alice, potion, 10);
    world.test_set_gold(bob, 10_000);
    let slot = world.test_slot_of(alice, potion).unwrap();

    // A zero price and an over-long note are refused before anything moves.
    world.market_consign(alice, slot, 4, 0, "free".into());
    world.market_consign(alice, slot, 4, 100, "x".repeat(151));
    assert!(world.market_store.listings.is_empty());
    assert_eq!(world.test_bag(alice).1, vec![(potion, 10)]);

    // A real listing takes exactly the consigned count out of the bag.
    world.market_consign(alice, slot, 4, 100, "cheap".into());
    assert_eq!(world.market_store.listings.len(), 1);
    let index = world.market_store.listings[0].index;
    assert_eq!(world.test_bag(alice).1, vec![(potion, 6)]);

    // Alice cannot buy her own listing, and Bob cannot overdraw it.
    world.market_buy(alice, index, 1);
    world.market_buy(bob, index, 99);
    assert_eq!(world.market_store.listings[0].item.count, 4);

    // Bob buys three: gold leaves once, the goods arrive once, the listing
    // shrinks by exactly three.
    let bob_gold = world.test_gold(bob);
    world.market_buy(bob, index, 3);
    assert_eq!(world.test_gold(bob), bob_gold - 300);
    assert_eq!(world.test_bag(bob).1, vec![(potion, 3)]);
    assert_eq!(world.market_store.listings[0].item.count, 1);

    // Alice is paid by mail, less the 7% tax, as Zircon does.
    let paid: u64 = world.mail_store.boxes[&1].iter().map(|m| m.gold).sum();
    assert_eq!(paid, 300 - 300 * 7 / 100);

    // Cancelling the remainder returns it and deletes the empty listing.
    world.market_cancel_consign(alice, index, 1);
    assert!(world.market_store.listings.is_empty());
    assert_eq!(world.test_bag(alice).1, vec![(potion, 7)]);
}

#[test]
fn buying_a_whole_listing_deletes_it() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    drain(&mut world);
    let potion = potion_of(&world);
    world.test_give_item(alice, potion, 2);
    world.test_set_gold(bob, 5_000);
    let slot = world.test_slot_of(alice, potion).unwrap();
    world.market_consign(alice, slot, 2, 250, "all of it".into());
    let index = world.market_store.listings[0].index;

    world.market_buy(bob, index, 2);
    assert!(world.market_store.listings.is_empty());
    assert_eq!(world.test_bag(bob).1, vec![(potion, 2)]);
    // A second attempt at the gone listing takes no more gold.
    let gold = world.test_gold(bob);
    world.market_buy(bob, index, 2);
    assert_eq!(world.test_gold(bob), gold);
}

#[test]
fn a_buyer_without_the_gold_gets_nothing() {
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    let bob = world.add_player(2, 2, &test_character("Bob")).unwrap();
    world.tick(0);
    drain(&mut world);
    let potion = potion_of(&world);
    world.test_give_item(alice, potion, 1);
    world.test_set_gold(bob, 10);
    let slot = world.test_slot_of(alice, potion).unwrap();
    world.market_consign(alice, slot, 1, 9_999, "dear".into());
    let index = world.market_store.listings[0].index;

    world.market_buy(bob, index, 1);
    assert_eq!(world.test_gold(bob), 10);
    assert!(world.test_bag(bob).1.is_empty());
    assert_eq!(world.market_store.listings.len(), 1);
}

#[test]
fn search_filters_sorts_and_pages() {
    use mir_proto::{MarketSort, MARKET_PAGE};
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    world.tick(0);
    // Twelve listings, so the second page holds the remaining three.
    world.dev_seed_market(12);
    drain(&mut world);

    world.market_search(alice, String::new(), None, MarketSort::LowestPrice, 0);
    let (total, page, results) = last_search(&mut world);
    assert_eq!((total, page, results.len()), (12, 0, MARKET_PAGE));
    assert!(results.windows(2).all(|w| w[0].price <= w[1].price));
    // The seed puts every fifth row on the first account, which is Alice, so
    // the viewer sees a mix of her own rows and other people's.
    assert!(results.iter().any(|l| l.is_owner));
    assert!(results.iter().any(|l| !l.is_owner));

    world.market_search(alice, String::new(), None, MarketSort::HighestPrice, 1);
    let (total, page, results) = last_search(&mut world);
    assert_eq!((total, page, results.len()), (12, 1, 12 - MARKET_PAGE));

    // A name that matches nothing returns an empty page, not everything.
    world.market_search(alice, "zzzznotanitem".into(), None, MarketSort::Newest, 0);
    let (total, _, results) = last_search(&mut world);
    assert_eq!((total, results.len()), (0, 0));

    // The type filter keeps only that kind.
    let weapons = world
        .market_store
        .listings
        .iter()
        .filter(|l| world.data.items[&l.item.info].item_type == mir_proto::item_type::WEAPON)
        .count() as u32;
    world.market_search(
        alice,
        String::new(),
        Some(mir_proto::item_type::WEAPON),
        MarketSort::Newest,
        0,
    );
    let (total, _, _) = last_search(&mut world);
    assert_eq!(total, weapons);
}

#[test]
fn tax_arithmetic_and_consign_limit_match_zircon() {
    use crate::world::market::{after_tax, consign_limit, tax_on};
    // Globals.MarketPlaceTax is 7%.
    assert_eq!(tax_on(1000), 70);
    assert_eq!(after_tax(1000), 930);
    // Integer division rounds the tax down, so the seller never loses a coin
    // to rounding and the two halves always add back up.
    assert_eq!(tax_on(10), 0);
    assert_eq!(after_tax(10), 10);
    assert_eq!(tax_on(0), 0);
    for total in [1u64, 7, 99, 1234, 1_000_000] {
        assert_eq!(tax_on(total) + after_tax(total), total);
    }
    // HighestLevel() * 3, with a floor so a new character can list at all.
    assert_eq!(consign_limit(1), 5);
    assert_eq!(consign_limit(10), 30);
    assert_eq!(consign_limit(0), 5);
}

#[test]
fn listings_survive_a_reload() {
    use crate::world::market::MarketStore;
    let Some(mut world) = world() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let alice = world.add_player(1, 1, &test_character("Alice")).unwrap();
    world.tick(0);
    let potion = potion_of(&world);
    world.test_give_item(alice, potion, 2);
    let slot = world.test_slot_of(alice, potion).unwrap();
    world.market_consign(alice, slot, 2, 500, "keeps".into());

    let json = serde_json::to_vec(&world.market_store).unwrap();
    let back: MarketStore = serde_json::from_slice(&json).unwrap();
    assert_eq!(back.listings.len(), 1);
    assert_eq!(back.listings[0].price, 500);
    assert_eq!(back.listings[0].item.count, 2);
    assert_eq!(back.listings[0].seller, "Alice");
    assert_eq!(back.next_index, world.market_store.next_index);
}
