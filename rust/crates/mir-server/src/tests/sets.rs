//! Equipment set bonuses (Zircon `SetInfo` / `SetInfoStat`, applied in
//! `PlayerObject.RefreshStats`) and the weapon-craft roll table.

use std::collections::HashMap;

use mir_proto::Class;

use crate::data::{GameData, SetDef, SetStatDef};
use crate::items::{set_bonuses, weapon_craft_pick, weapon_craft_total, Bag, UserItem};

/// `RequiredClass` flags, as the data stores them.
const WARRIOR: u8 = 1;
const WIZARD: u8 = 2;
const ALL: u8 = 15;

const MAX_DC: i32 = 9;
const ACCURACY: i32 = 14;
const CRIT: i32 = 55;

/// A pack-backed `GameData` with one synthetic three-piece set bolted on, so
/// the gates can be driven without depending on which sets ship in the pack.
/// Returns the data and the three item indices that make up the set.
fn data_with_test_set() -> Option<(GameData, [i32; 3])> {
    let assets = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from)?;
    let mut data = GameData::load(assets.join("../Database/System.db")).ok()?;

    // Three real items, re-pointed at a set index nothing else uses.
    let set_index = data.sets.keys().copied().max().unwrap_or(0) + 100;
    let mut picked: Vec<i32> = data
        .items
        .values()
        .filter(|d| d.set == 0 && d.durability > 0)
        .map(|d| d.index)
        .collect();
    picked.sort_unstable();
    let pieces: [i32; 3] = [picked[0], picked[1], picked[2]];
    for i in pieces {
        data.items.get_mut(&i).unwrap().set = set_index;
    }
    data.sets.insert(
        set_index,
        SetDef {
            index: set_index,
            name: "Test Set".into(),
            pieces: 3,
            stats: vec![
                // Open to everyone from level 1.
                SetStatDef {
                    stat: MAX_DC,
                    amount: 7,
                    class: ALL,
                    level: 0,
                },
                // Held back until level 30.
                SetStatDef {
                    stat: CRIT,
                    amount: 25,
                    class: ALL,
                    level: 30,
                },
                // Wizards only.
                SetStatDef {
                    stat: ACCURACY,
                    amount: 4,
                    class: WIZARD,
                    level: 0,
                },
            ],
        },
    );
    Some((data, pieces))
}

fn equip(bag: &mut Bag, slot: usize, info: i32) {
    bag.equipment[slot] = Some(UserItem {
        id: 1000 + slot as u32,
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
        colour: 0,
        expires_at: None,
    });
}

#[test]
fn a_set_pays_out_only_when_every_piece_is_worn() {
    let Some((data, pieces)) = data_with_test_set() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };

    // Two of three: Zircon's `Items.Count != value.Count` bails, so nothing.
    let mut bag = Bag::new();
    equip(&mut bag, 0, pieces[0]);
    equip(&mut bag, 1, pieces[1]);
    let partial = bag.equipment_stats(&data, 50, Class::Warrior);
    let base_dc = partial.get(&MAX_DC).copied().unwrap_or(0);

    // The third piece completes it and the +7 lands.
    equip(&mut bag, 2, pieces[2]);
    let full = bag.equipment_stats(&data, 50, Class::Warrior);
    assert_eq!(
        full.get(&MAX_DC).copied().unwrap_or(0),
        base_dc + 7,
        "a complete set grants its stat"
    );
    assert!(
        !partial.contains_key(&CRIT),
        "a partial set grants nothing at all"
    );
}

#[test]
fn set_lines_respect_the_level_and_class_gates() {
    let Some((data, pieces)) = data_with_test_set() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let mut bag = Bag::new();
    for (slot, info) in pieces.iter().enumerate() {
        equip(&mut bag, slot, *info);
    }

    // Level 29 is one short of the critical-chance line.
    let low = bag.equipment_stats(&data, 29, Class::Wizard);
    assert_eq!(
        low.get(&CRIT).copied().unwrap_or(0),
        0,
        "a line above the character's level stays off"
    );
    let high = bag.equipment_stats(&data, 30, Class::Wizard);
    assert_eq!(
        high.get(&CRIT).copied().unwrap_or(0),
        25,
        "reaching the level turns it on"
    );

    // Accuracy is a wizard line, so a warrior never sees it.
    let wizard_acc = high.get(&ACCURACY).copied().unwrap_or(0);
    let warrior = bag.equipment_stats(&data, 30, Class::Warrior);
    assert_eq!(
        warrior.get(&ACCURACY).copied().unwrap_or(0),
        wizard_acc - 4,
        "the class gate excludes a warrior from a wizard line"
    );
    // The open line still applies to both.
    assert_eq!(
        warrior.get(&CRIT).copied().unwrap_or(0),
        25,
        "an all-class line applies regardless"
    );
}

#[test]
fn a_broken_piece_breaks_the_set() {
    let Some((data, pieces)) = data_with_test_set() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let mut bag = Bag::new();
    for (slot, info) in pieces.iter().enumerate() {
        equip(&mut bag, slot, *info);
    }
    let set_index = data.items.get(&pieces[0]).unwrap().set;
    assert_eq!(bag.set_progress(&data), vec![(set_index, 3, 3)]);

    // Zircon skips a worn-out item entirely, so it cannot count as a piece.
    bag.equipment[1].as_mut().unwrap().durability = 0;
    let stats = bag.equipment_stats(&data, 50, Class::Warrior);
    assert_eq!(
        stats.get(&CRIT).copied().unwrap_or(0),
        0,
        "a broken piece drops the set"
    );
}

#[test]
fn set_progress_counts_distinct_pieces() {
    let Some((data, pieces)) = data_with_test_set() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    // The same piece twice is one piece, as Zircon's `Contains` guard makes it.
    let mut bag = Bag::new();
    equip(&mut bag, 0, pieces[0]);
    equip(&mut bag, 1, pieces[0]);
    let progress = bag.set_progress(&data);
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].1, 1, "a duplicate piece counts once");
    assert_eq!(progress[0].2, 3);
}

#[test]
fn set_bonuses_ignores_a_set_with_no_pieces_recorded() {
    let Some((mut data, pieces)) = data_with_test_set() else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let set_index = data.items.get(&pieces[0]).unwrap().set;
    data.sets.get_mut(&set_index).unwrap().pieces = 0;
    let mut worn = HashMap::new();
    worn.insert(set_index, vec![pieces[0]]);
    assert!(
        set_bonuses(&data, &worn, 99, Class::Warrior).is_empty(),
        "an empty set must not pay out for free"
    );
}

#[test]
fn the_pack_ships_usable_sets() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let Ok(data) = GameData::load(assets.join("../Database/System.db")) else {
        return;
    };
    // 30 sets ship, and every one names at least one item and one bonus.
    assert_eq!(data.sets.len(), 30);
    let usable = data
        .sets
        .values()
        .filter(|s| s.pieces > 0 && !s.stats.is_empty())
        .count();
    assert_eq!(usable, 30, "every set has pieces and stats");
}

#[test]
fn weapon_craft_rolls_walk_the_weight_table() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
        eprintln!("ZIRCON_ASSETS not set; skipping");
        return;
    };
    let Ok(data) = GameData::load(assets.join("../Database/System.db")) else {
        return;
    };
    assert!(
        !data.weapon_craft_stats.is_empty(),
        "the pack has the table"
    );

    let total = weapon_craft_total(&data, Class::Warrior);
    assert!(total > 0);

    // Roll 0 takes the first row the class admits.
    let first = data
        .weapon_craft_stats
        .iter()
        .find(|s| s.class & WARRIOR != 0)
        .unwrap();
    let (stat, min, max) = weapon_craft_pick(&data, Class::Warrior, 0).unwrap();
    assert_eq!(stat, first.stat);
    assert_eq!((min, max), (first.min_value, first.max_value));

    // Every roll inside the total lands on some row, and none outside it does.
    for roll in 0..total {
        assert!(
            weapon_craft_pick(&data, Class::Warrior, roll).is_some(),
            "roll {roll} of {total} found no row"
        );
    }
    assert!(weapon_craft_pick(&data, Class::Warrior, total).is_none());

    // A class only ever draws rows that admit it.
    let flag = Class::Wizard.flag();
    for roll in 0..weapon_craft_total(&data, Class::Wizard) {
        let (stat, _, _) = weapon_craft_pick(&data, Class::Wizard, roll).unwrap();
        assert!(data
            .weapon_craft_stats
            .iter()
            .any(|s| s.stat == stat && s.class & flag != 0));
    }
}
