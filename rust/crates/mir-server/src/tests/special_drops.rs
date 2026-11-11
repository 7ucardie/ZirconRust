use std::collections::BTreeSet;

use mir_formats::mirdb::stat;
use mir_proto::{element, item_type, slot, Class, Gender, Grid, Point};

use crate::accounts::test_character;
use crate::items::{StatSource, UserItem};

use super::world;

fn blank(info: i32) -> UserItem {
    UserItem {
        id: 1,
        info,
        count: 1,
        durability: 100,
        max_durability: 100,
        added: Vec::new(),
        level: 0,
        refinable: false,
        non_refinable: false,
        bound: false,
        worthless: false,
        locked: false,
        special_repair_until: 0,
        weapon_experience_tenths: 0,
        accessory_experience: 0,
        expires_at: None,
        colour: 0,
    }
}

#[test]
fn special_drops_follow_slot_rules_and_signed_element_values() {
    let Some(mut world) = world() else { return };
    world.drop_specials = true;
    let attack: BTreeSet<i32> = super::world::ELEMENT_ATTACK_STATS[1..]
        .iter()
        .copied()
        .collect();
    let resist: BTreeSet<i32> = super::world::ELEMENT_RESIST_STATS.iter().copied().collect();
    for kind in [
        item_type::WEAPON,
        item_type::SHIELD,
        item_type::ARMOUR,
        item_type::HELMET,
        item_type::NECKLACE,
        item_type::BRACELET,
        item_type::RING,
        item_type::SHOES,
    ] {
        let info = world
            .data
            .items
            .values()
            .filter(|d| d.item_type == kind)
            .map(|d| d.index)
            .min()
            .unwrap();
        let mut seen = BTreeSet::new();
        for _ in 0..15_000 {
            let mut item = blank(info);
            world.test_roll_drop_bonus(&mut item);
            for bonus in &item.added {
                assert_eq!(bonus.source, StatSource::Added);
                seen.insert((bonus.stat, bonus.amount));
                if resist.contains(&bonus.stat) {
                    assert_eq!(
                        bonus.amount.abs(),
                        if kind == item_type::ARMOUR || kind == item_type::SHIELD {
                            2
                        } else {
                            1
                        }
                    );
                }
                if attack.contains(&bonus.stat) {
                    assert!(matches!(
                        kind,
                        item_type::WEAPON | item_type::NECKLACE | item_type::RING
                    ));
                }
            }
        }
        match kind {
            item_type::WEAPON => {
                assert!(seen.iter().any(|(s, v)| attack.contains(s) && *v >= 2));
                assert!(!seen.iter().any(|(s, _)| resist.contains(s)));
            }
            item_type::ARMOUR | item_type::SHIELD => {
                assert!(seen.iter().any(|(s, v)| resist.contains(s) && *v == 2));
                assert!(seen.iter().any(|(s, v)| resist.contains(s) && *v == -2));
                if kind == item_type::SHIELD {
                    for stat in [
                        stat::DC_PERCENT,
                        stat::MC_PERCENT,
                        stat::SC_PERCENT,
                        stat::BLOCK_CHANCE,
                        stat::EVASION_CHANCE,
                        stat::POISON_RESISTANCE,
                    ] {
                        assert!(seen.iter().any(|(s, v)| *s == stat && (1..=3).contains(v)));
                    }
                }
            }
            item_type::HELMET | item_type::BRACELET | item_type::SHOES => {
                assert!(seen.iter().any(|(s, v)| resist.contains(s) && *v == 1));
                assert!(seen.iter().any(|(s, v)| resist.contains(s) && *v == -1));
            }
            item_type::RING => {
                assert!(seen
                    .iter()
                    .any(|(s, v)| { *s == stat::PICK_UP_RADIUS && (1..=3).contains(v) }));
            }
            _ => assert!(!seen.iter().any(|(s, _)| resist.contains(s))),
        }
    }
}

#[test]
fn shield_and_ring_specials_change_player_stats() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("ShieldRing"))
        .unwrap();
    let before = world.objects[&me].stats;
    let shield = world
        .data
        .items
        .values()
        .filter(|d| d.item_type == item_type::SHIELD)
        .find(|d| crate::items::can_use(d, Class::Warrior, Gender::Male, 1).is_ok())
        .unwrap()
        .index;
    world.test_give_item(me, shield, 1);
    let shield_slot = world.test_slot_of(me, shield).unwrap();
    let shield_item = world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[shield_slot as usize]
        .as_mut()
        .unwrap();
    for stat in [
        stat::DC_PERCENT,
        stat::MC_PERCENT,
        stat::SC_PERCENT,
        stat::BLOCK_CHANCE,
        stat::EVASION_CHANCE,
        stat::POISON_RESISTANCE,
    ] {
        shield_item.add_stat(stat, 100, StatSource::Added);
    }
    world.item_move(
        me,
        Grid::Inventory,
        shield_slot,
        Grid::Equipment,
        slot::SHIELD as u8,
    );
    let s = world.objects[&me].stats;
    assert_eq!(s.block_chance, before.block_chance + 100);
    assert_eq!(s.evasion_chance, before.evasion_chance + 100);
    assert_eq!(s.poison_resistance, before.poison_resistance + 100);
    assert!(s.max_dc > before.max_dc);
    assert!(s.max_mc >= before.max_mc);
    assert!(s.max_sc >= before.max_sc);

    let player = &world.objects[&me];
    let (map, at, account, gold) = (
        player.map,
        player.location,
        player.player().unwrap().account,
        player.player().unwrap().bag.gold,
    );
    let monster = world.test_spawn_ai(1, map, at).unwrap();
    assert_eq!(world.test_damage(me, monster, 20), 0);
    assert_eq!(world.test_damage_element(me, monster, 20, element::FIRE), 0);
    world.test_apply_poison(me, super::world::poison_kind::RED);
    assert!(world.objects[&me].poisons.is_empty());
    world.test_drop_item(
        map,
        Point::new(at.x + 2, at.y),
        world.data.gold_item,
        11,
        account,
    );
    world.pick_up(me);
    assert_eq!(world.objects[&me].player().unwrap().bag.gold, gold);

    let ring = world
        .data
        .items
        .values()
        .filter(|d| d.item_type == item_type::RING)
        .find(|d| crate::items::can_use(d, Class::Warrior, Gender::Male, 1).is_ok())
        .unwrap()
        .index;
    world.test_give_item(me, ring, 1);
    let ring_slot = world.test_slot_of(me, ring).unwrap();
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .inventory[ring_slot as usize]
        .as_mut()
        .unwrap()
        .add_stat(stat::PICK_UP_RADIUS, 2, StatSource::Added);
    world.item_move(
        me,
        Grid::Inventory,
        ring_slot,
        Grid::Equipment,
        slot::RING_L as u8,
    );
    assert_eq!(
        world.objects[&me].stats.pickup_radius,
        before.pickup_radius + 2
    );
    world.pick_up(me);
    assert_eq!(world.objects[&me].player().unwrap().bag.gold, gold + 11);
}

#[test]
fn equipped_specials_reach_combat_element_stats() {
    let Some(mut world) = world() else { return };
    let me = world.add_player(1, 1, &test_character("Elements")).unwrap();
    let weapon = world
        .data
        .items
        .values()
        .filter(|d| {
            d.item_type == item_type::WEAPON
                && d.effect != 5
                && crate::items::can_use(d, Class::Warrior, Gender::Male, 1).is_ok()
        })
        .map(|d| d.index)
        .min()
        .unwrap();
    let armour = world
        .data
        .items
        .values()
        .filter(|d| {
            d.item_type == item_type::ARMOUR
                && crate::items::can_use(d, Class::Warrior, Gender::Male, 1).is_ok()
        })
        .map(|d| d.index)
        .min()
        .unwrap();
    let before_attack = world.objects[&me].stats.element_attack[1];
    let before_resist = world.objects[&me].stats.element_resist[1];
    world.test_give_item(me, weapon, 1);
    let slot = world.test_slot_of(me, weapon).unwrap();
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
        .add_stat(20, 2, StatSource::Added);
    world.item_move(me, Grid::Inventory, slot, Grid::Equipment, 0);
    world.test_give_item(me, armour, 1);
    let slot = world.test_slot_of(me, armour).unwrap();
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
        .add_stat(21, -2, StatSource::Added);
    world.item_move(me, Grid::Inventory, slot, Grid::Equipment, 1);
    assert_eq!(
        world.objects[&me].stats.element_attack[1],
        before_attack + 2
    );
    assert_eq!(
        world.objects[&me].stats.element_resist[1],
        before_resist - 2
    );
}
