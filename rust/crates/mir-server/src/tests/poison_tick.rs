use mir_proto::ServerMessage;

use super::*;

#[test]
fn poison_kill_after_reward_owner_disconnects_does_not_panic() {
    let Some(mut world) = world() else { return };
    let owner = world
        .add_player(1, 1, &test_character("DepartedOwner"))
        .unwrap();
    let caster = world
        .add_player(2, 2, &test_character("PoisonCaster"))
        .unwrap();
    world.tick(0);
    let map = world.objects[&caster].map;
    let loc = world.objects[&caster].location;
    let def = world
        .data
        .monsters
        .values()
        .find(|def| def.name == "Chicken")
        .unwrap()
        .index;
    let monster = world.test_spawn_monster(def, map, loc, None);
    let object = world.objects.get_mut(&monster).unwrap();
    object.stats.poison_resistance = 0;
    object.monster_mut().unwrap().exp_owner = Some(owner);
    world.test_set_hp(monster, 1);
    world.test_apply_owned_poison(monster, caster, world::poison_kind::GREEN, 10);
    world.remove_object(owner);
    assert!(!world.objects.contains_key(&owner));

    world.tick(1000);

    assert!(world.objects[&monster].dead);
    assert_eq!(world.objects[&monster].hp, 0);
}

#[test]
fn player_green_poison_kills_regular_monster_but_spares_boss_and_player() {
    let Some(mut world) = world() else { return };
    let me = world
        .add_player(1, 1, &test_character("PoisonOwner"))
        .unwrap();
    let other = world
        .add_player(2, 2, &test_character("PoisonVictim"))
        .unwrap();
    world.tick(0);
    let map = world.objects[&me].map;
    let loc = world.objects[&me].location;
    let ordinary_def = world
        .data
        .monsters
        .values()
        .filter(|def| !def.is_boss && def.experience >= 10.0 && def.flag == 0)
        .min_by_key(|def| def.index)
        .expect("ordinary monster with experience")
        .index;
    let boss_def = world
        .data
        .monsters
        .values()
        .filter(|def| def.is_boss)
        .min_by_key(|def| def.index)
        .expect("boss monster")
        .index;
    let ordinary = world.test_spawn_monster(ordinary_def, map, loc, None);
    let boss = world.test_spawn_monster(boss_def, map, loc, None);
    for id in [ordinary, boss] {
        world.objects.get_mut(&id).unwrap().stats.poison_resistance = 0;
    }
    world
        .objects
        .get_mut(&other)
        .unwrap()
        .stats
        .poison_resistance = 0;
    world.test_set_hp(ordinary, 1);
    world.test_set_hp(boss, 5);
    world.test_set_hp(other, 5);
    let before = world.objects[&me].player().unwrap().experience;
    world.test_apply_owned_poison(ordinary, me, world::poison_kind::GREEN, 10);
    world.test_apply_owned_poison(boss, me, world::poison_kind::GREEN, 10);
    world.test_apply_owned_poison(other, me, world::poison_kind::GREEN, 10);
    world.tick(1000);

    assert!(
        world.objects[&ordinary].dead,
        "poison must finish ordinary monster"
    );
    assert_eq!(
        world.objects[&ordinary].monster().unwrap().exp_owner,
        Some(me)
    );
    assert!(
        world.objects[&me].player().unwrap().experience > before
            || world.objects[&me].player().unwrap().level > 1,
        "poison kill must award experience"
    );
    assert!(world.outgoing.iter().any(|message| matches!(message,
        Outgoing::To(_, ServerMessage::ObjectDie { id }) if *id == ordinary)));
    assert_eq!(world.objects[&boss].hp, 5, "boss immune to poison damage");
    assert_eq!(world.objects[&other].hp, 1, "poison cannot kill players");
}
