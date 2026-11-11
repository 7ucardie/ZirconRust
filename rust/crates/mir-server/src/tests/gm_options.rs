use super::*;
use mir_formats::mirdb::stat;
use mir_proto::{slot, Gender};

#[test]
fn gm_stat_aliases_refine_and_gender_options_persist() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("CommandTester");
    record.gm = true;
    let me = world.add_player(1, 1, &record).unwrap();
    world.tick(0);
    for (alias, stat) in [
        ("mc", stat::MAX_MC),
        ("dc", stat::MAX_DC),
        ("ac", stat::MAX_AC),
        ("mr", stat::MAX_MR),
    ] {
        world.chat(me, format!("@sethermitstat {alias} 20"));
        assert_eq!(world.objects[&me].player().unwrap().hermit_stats[&stat], 20);
    }
    world.chat(me, "@sethermistat mc 25".into());
    assert_eq!(
        world.objects[&me].player().unwrap().hermit_stats[&stat::MAX_MC],
        25
    );
    let before = world.objects[&me].stats.max_dc;
    world.chat(me, "@refineweapon dc 3".into());
    assert_eq!(world.objects[&me].stats.max_dc, before + 3);
    let weapon = world.objects[&me].player().unwrap().bag.equipment[slot::WEAPON]
        .as_ref()
        .unwrap();
    assert!(weapon.added.iter().any(|s| s.stat == stat::MAX_DC
        && s.amount == 3
        && s.source == crate::items::StatSource::Refine));
    let original = world.objects[&me].player().unwrap().gender;
    assert!(world.objects[&me].player().unwrap().bag.equipment[slot::ARMOUR].is_some());
    world.chat(me, "@changegender".into());
    assert_eq!(world.objects[&me].player().unwrap().gender, original);
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .player_mut()
        .unwrap()
        .bag
        .equipment[slot::ARMOUR] = None;
    world.chat(me, "@changegender female".into());
    assert_eq!(world.objects[&me].player().unwrap().gender, Gender::Female);
    world.snapshot(me, &mut record);
    assert_eq!(record.gender, Gender::Female);
    assert!(record.hermit_stats.contains(&(stat::MAX_MC, 25)));
}

#[test]
fn experience_and_new_stun_send_feedback() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("FeedbackTester"))
        .unwrap();
    world.tick(0);
    drain(&mut world);
    world.gain_experience_test(me, 10);
    assert!(drain(&mut world)
        .iter()
        .any(|m| matches!(m, ServerMessage::Say { text, .. } if text == "Gained 10 experience.")));
    world.objects.get_mut(&me).unwrap().stats.poison_resistance = 0;
    world.test_apply_owned_poison(me, me, world::poison_kind::PARALYSIS, 1);
    assert!(drain(&mut world)
        .iter()
        .any(|m| matches!(m, ServerMessage::Say { text, .. } if text == "You are stunned.")));
    world.test_apply_owned_poison(me, me, world::poison_kind::PARALYSIS, 1);
    assert!(!drain(&mut world)
        .iter()
        .any(|m| matches!(m, ServerMessage::Say { text, .. } if text == "You are stunned.")));
}

#[test]
fn paralysis_blocks_player_actions_then_clears_without_losing_green_poison() {
    let Some(mut world) = world() else { return };
    let mut record = test_character("StunTester");
    record.class = mir_proto::Class::Wizard;
    record.level = 100;
    let me = world.add_player(1, 1, &record).unwrap();
    world.tick(0);
    let spell = learn(&mut world, me, "Fire Ball");
    world.tick(3000);
    let map = world.objects[&me].map;
    world.test_clear_monsters(map);
    let location = world.objects[&me].location;
    let direction = Direction::ALL
        .into_iter()
        .find(|direction| {
            let cell = location.step(*direction, 1);
            world.maps[&map].file.is_walkable(cell.x, cell.y)
                && world.maps[&map].objects_at(cell).is_empty()
                && !world.test_movement_cells(map).contains(&cell)
        })
        .unwrap();
    world.objects.get_mut(&me).unwrap().stats.poison_resistance = 0;
    world.test_apply_owned_poison(me, me, world::poison_kind::GREEN, 1);
    world.test_apply_owned_poison(me, me, world::poison_kind::PARALYSIS, 1);
    let action_time = world.objects[&me].action_time;
    let mp = world.objects[&me].player().unwrap().mp;
    world.player_move(me, direction, false);
    world.player_turn(me, direction);
    world.player_attack(me, direction, None);
    world.cast(me, spell, direction, None, location);
    assert_eq!(world.objects[&me].location, location);
    assert_eq!(world.objects[&me].action_time, action_time);
    assert_eq!(world.objects[&me].player().unwrap().mp, mp);
    world
        .objects
        .get_mut(&me)
        .unwrap()
        .poisons
        .iter_mut()
        .find(|poison| poison.kind == world::poison_kind::PARALYSIS)
        .unwrap()
        .ticks_left = 0;
    world.tick(4000);
    assert!(drain(&mut world).iter().any(|message| matches!(message,
        ServerMessage::ObjectPoisoned { id, kinds } if *id == me && *kinds == world::poison_kind::GREEN)));
    world.player_move(me, direction, false);
    assert_eq!(world.objects[&me].location, location.step(direction, 1));
}
