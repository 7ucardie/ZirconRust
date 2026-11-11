//! Zircon monster AI classes as data (`MonsterRegistrations.cs`): the `AI`
//! value of a `MonsterInfo` selects a profile that the shared monster loop in
//! `monster_ai.rs` interprets. See `rust/docs/research/monster-ai.md`.

use super::poison_kind;
use mir_proto::{element, magic_type};

/// Poison applied by a landed melee/ranged hit (Zircon `PoisonType`,
/// `PoisonTicks`, `PoisonFrequency` seconds, `PoisonRate`: 1-in-rate).
#[derive(Clone, Copy, Debug)]
pub struct HitPoison {
    pub kind: u16,
    pub ticks: i32,
    pub frequency_s: i32,
    pub rate: i32,
}

/// A monster spell (Zircon `MonsterObject.AttackMagic` and friends).
#[derive(Clone, Debug)]
pub enum Spell {
    /// Single-target bolt with travel time when `travel`.
    Bolt {
        magic: u16,
        element: u8,
        travel: bool,
        /// Damage scale in percent (Zircon passes explicit damage for some).
        scale: i32,
    },
    /// Area around the target (or around self when `at_self`).
    Aoe {
        radius: i32,
        magic: u16,
        element: u8,
        at_self: bool,
        scale: i32,
    },
    /// `LineAoE(len, min, max)`: beams along the facing direction rotated
    /// through `min..=max`, flank cells at half power.
    Line {
        len: i32,
        min: i8,
        max: i8,
        magic: u16,
        element: u8,
    },
    /// Every hostile within MaxViewRange, each with `pct` chance.
    Mass { magic: u16, element: u8, pct: i32 },
    /// Radius-2 storm around self.
    ThunderStorm,
    /// 5x5 poisonous cloud around self.
    PoisonousCloud,
    /// Five-cell fire cross on the target.
    FireWall,
    /// Pick one at random.
    Random(Vec<Spell>),
}

/// Hidden-until-close behaviour (CarnivorousPlant, GhostMage, statues).
#[derive(Clone, Copy, Debug)]
pub struct Hidden {
    /// Reveal when a target is within this many cells.
    pub find_range: i32,
    /// Hide again when the target is further than this (0 = never re-hide).
    pub hide_range: i32,
    /// Full heal and poison cleanse on hiding.
    pub heal_on_hide: bool,
    /// Wake every dormant monster of the same kind within this range too.
    pub wake_range: i32,
}

/// Summon phases for bosses (`Stage` / `SpawnMinions`).
#[derive(Clone, Debug)]
pub struct Stages {
    pub count: i32,
    pub fixed: i32,
    pub random: i32,
    /// `(MonsterFlag, weight)` of the minion kinds.
    pub list: Vec<(i32, i32)>,
    pub max_minions: i32,
}

#[derive(Clone, Debug)]
pub struct AiProfile {
    pub passive: bool,
    pub immobile: bool,
    /// Takes no damage (Guard, gates, statues while dormant).
    pub invulnerable: bool,
    /// Every hit does 1 damage (TreeMonster).
    pub clamp_damage: bool,
    /// Guard rules: attacks wild non-passive monsters in view, kills them outright.
    pub guard: bool,
    /// 1 = melee only; larger allows ranged attacks.
    pub attack_range: i32,
    /// Only the eight straight/diagonal rays count as "in range".
    pub rays_only: bool,
    /// Ranged attacks only after this cooldown since the last one (GiantLizard).
    pub range_cooldown_ms: u64,
    /// Archer: walks away when the target is closer than `attack_range - 1`.
    pub kite: bool,
    /// After each shot, 1-in-`fear_rate` chance to stop shooting for
    /// `fear_duration_s + Random(4)` seconds.
    pub fear_rate: i32,
    pub fear_duration_s: i32,
    /// `LineAttack(n)`: pierce the first object of each cell along the facing.
    pub line_attack: i32,
    /// Splash radius around the target of a ranged hit.
    pub splash: i32,
    /// 1-in-n chance that a hit becomes the splash version (0 = always).
    pub splash_chance: i32,
    /// Melee hits everything within this radius of the monster itself.
    pub self_aoe: i32,
    /// 1-in-n chance for the self area version (0 = always).
    pub self_aoe_chance: i32,
    pub hit_poison: Option<HitPoison>,
    /// Blink-striker: each second when not adjacent, 1-in-`blink_chance` to
    /// blink next to the target; the next hit deals double when `double_on_blink`.
    pub blink_chance: i32,
    pub double_on_blink: bool,
    /// One-time teleport 7-12 cells away when HP drops to half.
    pub panic_teleport: bool,
    /// Blink adjacent when the target is further than `.0` cells (cooldown ms).
    pub blink_when_far: Option<(i32, u64)>,
    pub spell: Option<Spell>,
    /// In range: 1-in-n chance to cast instead of a normal attack (0 = never).
    pub spell_chance_near: i32,
    /// Out of range but within MagicRange: 1-in-n chance to cast (0 = never).
    pub spell_chance_far: i32,
    /// Spell cooldown in ms (Warewolf/ChaosKnight style timed casts).
    pub spell_cooldown_ms: u64,
    /// Larva: attacking means dying; death hits everything within this radius.
    pub suicide_range: i32,
    /// Dies within a few seconds of having no target (Larva).
    pub dies_without_target: bool,
    pub hidden: Option<Hidden>,
    pub stages: Option<Stages>,
    /// "Attacking" spawns minions of this flag instead (ArachnidGrazer).
    pub spawn_on_attack: Option<(i32, i32)>,
    /// On death: hit everything within `.0`, then spawn `.2` minions of flag
    /// `.1` per surviving player (DepartedMonster).
    pub die_splash: Option<(i32, i32, i32)>,
    /// Per-class damage taken multipliers in percent: warrior, wizard, taoist, assassin.
    pub class_mitigation: Option<[i32; 4]>,
    /// Voracious ghost: 0-3 revives at half the HP each time; drops only at
    /// the final death.
    pub revives: bool,
    /// Crimson Necromancer: every n ms every target within 3 loses its
    /// magic resistance for 10 s.
    pub weakness_every_ms: u64,
    /// Jinchon Devil: every n ms a death cloud (radius 2, one tick after
    /// 4-6 s) on each target in view.
    pub death_cloud_every_ms: u64,
    /// Gate: 0 = mystery ship, 1 = lair; teleports players in view every
    /// 3 s to the configured region, vanishes after 20 minutes.
    pub gate: Option<u8>,
    /// Shinsu: invisible and harmless except in 10 s fighting windows that
    /// open while it has a target (2 s transitions).
    pub mode_windows: bool,
    /// Terracotta: starts invisible, walks fast while unseen, shows itself
    /// within 2 cells of the target; `Some(true)` may also phase out again
    /// (1 in 45 per tick when further than 4 away, 5 s cooldown).
    pub phase: Option<bool>,
    /// Doom Claw: stationary parts boss; only attackers within 10 hurt it;
    /// its claws, spit and wave deal class-reduced damage with pushes.
    pub doom_claw: bool,
    /// Zircon `NeedHarvest` / `HarvestCount`: 0 means the corpse drops on
    /// death as usual. Anything higher and the corpse holds its drops until
    /// the killer butchers it, and this many swings land on nothing first.
    pub harvest_count: i32,
    /// Healer Ant: heals injured allies in view with a ticking heal instead
    /// of fighting when one needs it.
    pub heals_allies: bool,
    /// Purification on the target every 3 s with 1-in-n chance (Banyo
    /// Captain); 0 = never.
    pub purify_chance: i32,
    /// Mass purification of everything in view every n ms (Banyo Lord
    /// Guzak); 0 = never.
    pub purify_every_ms: u64,
    /// Frost Lord Hwa: every 60 s scatters everything in view and stacks
    /// Abyss, Silenced and Red poison for 10 s plus Wraith Grip for 5 s.
    pub scatter_curse: bool,
    /// Queen Of Dawn: every 60 s teleports every attackable object on the
    /// map to a random cell.
    pub map_scatter: bool,
}

impl Default for AiProfile {
    fn default() -> Self {
        AiProfile {
            passive: false,
            immobile: false,
            invulnerable: false,
            clamp_damage: false,
            guard: false,
            attack_range: 1,
            rays_only: false,
            range_cooldown_ms: 0,
            kite: false,
            fear_rate: 0,
            fear_duration_s: 0,
            line_attack: 0,
            splash: 0,
            splash_chance: 0,
            self_aoe: 0,
            self_aoe_chance: 0,
            hit_poison: None,
            blink_chance: 0,
            double_on_blink: false,
            panic_teleport: false,
            blink_when_far: None,
            spell: None,
            spell_chance_near: 0,
            spell_chance_far: 0,
            spell_cooldown_ms: 0,
            suicide_range: 0,
            dies_without_target: false,
            hidden: None,
            stages: None,
            spawn_on_attack: None,
            die_splash: None,
            class_mitigation: None,
            revives: false,
            weakness_every_ms: 0,
            death_cloud_every_ms: 0,
            gate: None,
            mode_windows: false,
            phase: None,
            doom_claw: false,
            harvest_count: 0,
            heals_allies: false,
            purify_chance: 0,
            purify_every_ms: 0,
            scatter_curse: false,
            map_scatter: false,
        }
    }
}

fn poison(kind: u16, ticks: i32, frequency_s: i32, rate: i32) -> Option<HitPoison> {
    Some(HitPoison {
        kind,
        ticks,
        frequency_s,
        rate,
    })
}

fn bolt(magic: u16, element: u8, travel: bool) -> Spell {
    Spell::Bolt {
        magic,
        element,
        travel,
        scale: 100,
    }
}

fn aoe(radius: i32, magic: u16, element: u8) -> Spell {
    Spell::Aoe {
        radius,
        magic,
        element,
        at_self: false,
        scale: 100,
    }
}

fn line(len: i32, min: i8, max: i8, magic: u16, element: u8) -> Spell {
    Spell::Line {
        len,
        min,
        max,
        magic,
        element,
    }
}

/// Elemental guardian kit shared by the Sama guardians and bosses.
fn sama_kit(
    bolt_m: u16,
    line_m: u16,
    aoe_m: u16,
    big_m: u16,
    elem: u8,
    signature: Option<u16>,
) -> Spell {
    let mut moves = vec![
        bolt(bolt_m, elem, true),
        line(10, -2, 2, line_m, elem),
        aoe(2, aoe_m, elem),
        aoe(3, big_m, elem),
    ];
    if let Some(sig) = signature {
        moves.push(Spell::Aoe {
            radius: 3,
            magic: sig,
            element: elem,
            at_self: false,
            scale: 200,
        });
    }
    Spell::Random(moves)
}

/// The blink-striker package (OmaWarlord and friends).
fn blink_striker(p: &mut AiProfile) {
    p.blink_chance = 7;
    p.double_on_blink = true;
    p.panic_teleport = true;
}

/// Profile for a `MonsterInfo.AI` value.
pub fn profile(ai: i32) -> AiProfile {
    use magic_type as m;
    let mut p = AiProfile::default();
    match ai {
        -1 => {
            p.guard = true;
            p.immobile = true;
            p.invulnerable = true;
            p.attack_range = 9;
        }
        1 | 2 => p.passive = true,
        4 => {
            p.passive = true;
            p.immobile = true;
            p.clamp_damage = true;
        }
        5 => {
            p.hidden = Some(Hidden {
                find_range: 5,
                hide_range: 5,
                heal_on_hide: true,
                wake_range: 0,
            })
        }
        125 => {
            p.hidden = Some(Hidden {
                find_range: 1,
                hide_range: 1,
                heal_on_hide: true,
                wake_range: 0,
            })
        }
        6 | 14 | 42 => {
            p.attack_range = 2;
            p.rays_only = true;
            p.line_attack = 2;
            if ai != 42 {
                p.hit_poison = poison(poison_kind::GREEN, 5, 2, 10);
            }
        }
        46 => {
            p.attack_range = 2;
            p.rays_only = true;
            p.line_attack = 2;
            p.hit_poison = poison(poison_kind::RED, 1, 10, 25);
        }
        47 => {
            p.attack_range = 2;
            p.rays_only = true;
            p.line_attack = 2;
            p.hit_poison = poison(poison_kind::GREEN, 7, 2, 15);
        }
        7 | 29 | 34 | 20 => {
            p.attack_range = if ai == 34 { 9 } else { 7 };
            p.kite = true;
            p.fear_rate = if ai == 20 { 2 } else { 6 };
            p.fear_duration_s = if ai == 20 { 4 } else { 2 };
        }
        8 => p.hit_poison = poison(poison_kind::PARALYSIS, 1, 5, 12),
        9 => {
            p.attack_range = 6;
            p.rays_only = true;
            p.line_attack = 6;
        }
        10 | 27 => {
            p.hidden = Some(Hidden {
                find_range: 5,
                hide_range: 0,
                heal_on_hide: false,
                wake_range: 0,
            })
        }
        12 => {
            // Healer ant: keeps its distance like an archer and heals
            // injured allies in view with a ticking heal.
            p.attack_range = 7;
            p.kite = true;
            p.heals_allies = true;
        }
        13 => {
            p.hidden = Some(Hidden {
                find_range: 5,
                hide_range: 5,
                heal_on_hide: true,
                wake_range: 0,
            });
            p.attack_range = 10;
            p.splash = 10;
            p.hit_poison = poison(poison_kind::GREEN, 10, 2, 5);
        }
        16 | 84 => {
            p.spell = Some(Spell::Random(vec![
                Spell::Mass {
                    magic: m::LIGHTNING_BALL,
                    element: element::LIGHTNING,
                    pct: 100,
                },
                Spell::Mass {
                    magic: m::THUNDER_BOLT,
                    element: element::LIGHTNING,
                    pct: 50,
                },
            ]));
            p.spell_chance_near = if ai == 84 { 1 } else { 5 };
            p.spell_chance_far = if ai == 84 { 1 } else { 5 };
        }
        17 => {
            p.immobile = true;
            p.attack_range = 40;
            p.spawn_on_attack = Some((100, 1));
        }
        44 => {
            p.immobile = true;
            p.attack_range = 40;
            p.spawn_on_attack = Some((110, 1));
        }
        18 => {
            p.suicide_range = 1;
            p.dies_without_target = true;
            p.hit_poison = poison(poison_kind::GREEN, 5, 2, 10);
        }
        123 => {
            p.suicide_range = 3;
            p.dies_without_target = true;
        }
        19 => {
            p.immobile = true;
            p.attack_range = 40;
            p.splash = 40;
        }
        21 => {
            p.hidden = Some(Hidden {
                find_range: 3,
                hide_range: 0,
                heal_on_hide: false,
                wake_range: 7,
            });
            p.invulnerable = true; // only while dormant (checked in code)
        }
        22 => {
            p.hidden = Some(Hidden {
                find_range: 3,
                hide_range: 0,
                heal_on_hide: false,
                wake_range: 7,
            });
            p.invulnerable = true;
            p.stages = Some(Stages {
                count: 7,
                fixed: 4,
                random: 8,
                list: vec![(120, 50), (122, 25), (121, 25), (123, 1)],
                max_minions: 20,
            });
            p.spell = Some(Spell::Random(vec![
                Spell::FireWall,
                line(12, -2, 2, m::MONSTER_SCORCHED_EARTH, element::FIRE),
                line(12, -2, 2, m::MONSTER_SCORCHED_EARTH, element::FIRE),
            ]));
            p.spell_chance_near = 5;
            p.spell_chance_far = 5;
        }
        23 => {
            p.attack_range = 2;
            p.hit_poison = poison(poison_kind::GREEN, 5, 2, 10);
        }
        24 => {
            p.attack_range = 2;
            p.hit_poison = poison(poison_kind::RED, 5, 2, 10);
        }
        64 | 137 => p.attack_range = 2,
        25 => {
            p.self_aoe = 1;
            p.self_aoe_chance = 6;
            p.hit_poison = poison(poison_kind::GREEN, 10, 2, 1);
        }
        26 | 36 => {
            p.spell = Some(bolt(m::THUNDER_BOLT, element::LIGHTNING, false));
            p.spell_chance_near = 5;
            p.spell_chance_far = 2;
        }
        28 => p.attack_range = 1,
        31 | 48 => {
            p.attack_range = 3;
            p.rays_only = true;
            p.line_attack = 3;
        }
        97 => {
            p.attack_range = 5;
            p.rays_only = true;
            p.line_attack = 5;
        }
        33 | 92 => {
            p.attack_range = 9;
        }
        49 => {
            p.attack_range = 8;
            p.hit_poison = poison(poison_kind::PARALYSIS, 1, 5, 10);
        }
        50 => p.attack_range = 8,
        54 => {
            p.attack_range = 7;
            p.range_cooldown_ms = 5000;
        }
        79 => {
            p.attack_range = 10;
            p.range_cooldown_ms = 5000;
        }
        105 => p.attack_range = 5,
        106 => p.attack_range = 7,
        103 => p.attack_range = 5,
        38 => {
            p.spell = Some(bolt(m::FIRE_BALL, element::FIRE, true));
            p.spell_chance_near = 5;
            p.spell_chance_far = 2;
        }
        41 | 85 => {
            p.spell = Some(Spell::Random(vec![
                Spell::Mass {
                    magic: m::CYCLONE,
                    element: element::WIND,
                    pct: 75,
                },
                aoe(2, m::MONSTER_SPLASH, element::NONE),
                aoe(2, m::MONSTER_SPLASH, element::NONE),
            ]));
            p.spell_chance_near = 5;
            p.spell_chance_far = 3;
            if ai == 85 {
                p.hit_poison = poison(poison_kind::PARALYSIS, 1, 5, 8);
            }
        }
        43 => {
            p.stages = Some(Stages {
                count: 7,
                fixed: 20,
                random: 5,
                list: vec![(130, 90), (133, 15), (132, 15), (131, 15), (134, 1)],
                max_minions: 50,
            });
            p.attack_range = 7;
            p.range_cooldown_ms = 0;
        }
        91 => {
            p.stages = Some(Stages {
                count: 7,
                fixed: 5,
                random: 5,
                list: vec![(161, 10), (162, 5), (160, 5)],
                max_minions: 50,
            });
            p.hit_poison = poison(poison_kind::RED, 1, 25, 5);
            blink_striker(&mut p);
        }
        45 => p.attack_range = 3,
        52 | 130 => {}
        53 => {
            p.attack_range = 2;
            p.rays_only = true;
            p.line_attack = 2;
            p.mode_windows = true;
        }
        56 => {
            p.blink_when_far = Some((3, 5000));
            p.hit_poison = poison(poison_kind::GREEN, 7, 2, 15);
        }
        57 => p.blink_when_far = Some((3, 5000)),
        58 => p.self_aoe = 1,
        59 => {}
        60 => {
            p.spell = Some(Spell::PoisonousCloud);
            p.spell_chance_near = 1;
            p.spell_cooldown_ms = 20_000;
        }
        61 | 74 => {
            p.attack_range = 2;
            p.blink_when_far = Some((8, 10_000));
        }
        62 => {
            p.attack_range = 6;
            p.rays_only = true;
            p.line_attack = 6;
        }
        63 => {
            p.attack_range = 7;
            p.kite = true;
            p.fear_rate = 6;
            p.fear_duration_s = 2;
            p.splash = 2;
        }
        65 => {
            p.hidden = Some(Hidden {
                find_range: 3,
                hide_range: 5,
                heal_on_hide: true,
                wake_range: 0,
            });
            p.attack_range = 8;
        }
        66 | 67 => {
            p.attack_range = 8;
            p.spell = Some(aoe(1, m::MONSTER_ICE_STORM, element::ICE));
            p.spell_chance_near = 2;
            if ai == 66 {
                p.hit_poison = poison(poison_kind::PARALYSIS, 1, 5, 25);
            }
        }
        68 => {
            p.spell = Some(line(8, -2, 2, m::GREATER_FROZEN_EARTH, element::ICE));
            p.spell_chance_near = 1;
            p.spell_chance_far = 1;
            p.spell_cooldown_ms = 10_000;
        }
        70 => {
            p.blink_when_far = Some((3, 5000));
            p.scatter_curse = true;
        }
        71 => {
            p.blink_chance = 7;
        }
        117 => {
            p.blink_chance = 7;
            p.double_on_blink = true;
        }
        72 => {
            p.spell = Some(bolt(m::THUNDER_BOLT, element::LIGHTNING, false));
            p.spell_chance_near = 5;
            p.spell_chance_far = 5;
            p.spell_cooldown_ms = 3000;
            p.purify_chance = 5;
        }
        75 => p.die_splash = Some((2, 140, 4)),
        76 => p.die_splash = Some((2, 141, 4)),
        77 => {
            p.hidden = Some(Hidden {
                find_range: 5,
                hide_range: 5,
                heal_on_hide: true,
                wake_range: 0,
            });
            p.attack_range = 10;
            p.splash = 10;
            p.hit_poison = poison(poison_kind::GREEN, 10, 2, 5);
        }
        78 | 127 => {
            p.attack_range = 3;
            p.rays_only = true;
            p.line_attack = 3;
        }
        80 => {
            p.spell = Some(Spell::Random(vec![
                bolt(m::FIRE_BALL, element::FIRE, true),
                aoe(1, m::FIRE_STORM, element::FIRE),
            ]));
            p.spell_chance_near = 5;
            p.spell_chance_far = 2;
        }
        81 => {
            p.spell = Some(bolt(m::THUNDER_BOLT, element::LIGHTNING, false));
            p.spell_chance_near = 5;
            p.spell_chance_far = 2;
            blink_striker(&mut p);
        }
        82 => {
            p.spell = Some(Spell::Random(vec![
                line(12, -2, 2, m::GREATER_FROZEN_EARTH, element::ICE),
                line(12, -2, 2, m::MONSTER_SCORCHED_EARTH, element::FIRE),
                line(12, -2, 2, m::LIGHTNING_BEAM, element::LIGHTNING),
                line(12, -2, 2, m::BLOW_EARTH, element::WIND),
            ]));
            p.spell_chance_near = 1;
            p.spell_chance_far = 1;
            p.spell_cooldown_ms = 5000;
        }
        83 => {
            p.spell = Some(line(12, -2, 2, m::MONSTER_SCORCHED_EARTH, element::FIRE));
            p.spell_chance_near = 1;
            p.spell_chance_far = 1;
            p.spell_cooldown_ms = 5000;
        }
        86 => {
            p.passive = true;
            p.spell = Some(line(12, 0, 8, m::MONSTER_SCORCHED_EARTH, element::FIRE));
            p.spell_chance_near = 1;
            p.spell_chance_far = 1;
            p.spell_cooldown_ms = 5000;
        }
        87 => {
            blink_striker(&mut p);
            p.hit_poison = poison(poison_kind::ABYSS, 1, 7, 15);
        }
        90 => {
            blink_striker(&mut p);
            p.hit_poison = poison(poison_kind::PARALYSIS, 1, 5, 25);
        }
        88 => {
            p.attack_range = 2;
            p.rays_only = true;
            blink_striker(&mut p);
        }
        89 => {
            p.attack_range = 7;
            p.kite = true;
            p.fear_rate = 6;
            p.fear_duration_s = 2;
            blink_striker(&mut p);
            p.hit_poison = poison(poison_kind::SILENCED, 1, 5, 10);
        }
        93 => {
            p.spell = Some(Spell::Random(vec![
                Spell::ThunderStorm,
                bolt(m::THUNDER_BOLT, element::LIGHTNING, false),
            ]));
            p.spell_chance_near = 2;
            p.spell_chance_far = 2;
        }
        94 | 95 => {
            p.attack_range = 10;
            p.splash = 2;
            p.splash_chance = 5;
            if ai == 95 {
                p.hit_poison = poison(poison_kind::PARALYSIS, 1, 5, 15);
            }
        }
        96 => {
            p.blink_chance = 10;
            p.self_aoe = 2;
            p.self_aoe_chance = 5;
            p.map_scatter = true;
        }
        98 | 100 | 101 => {
            p.attack_range = 10;
            p.self_aoe = 3;
            p.self_aoe_chance = 5;
        }
        102 => {
            p.attack_range = 12;
            p.splash = 2;
        }
        104 => {
            p.attack_range = 3;
            p.self_aoe = 3;
            p.self_aoe_chance = 5;
            p.class_mitigation = Some([80, 100, 100, 100]);
        }
        107 => {
            p.spell = Some(sama_kit(
                m::ADAMANTINE_FIRE_BALL,
                m::SCORCHED_EARTH,
                m::FIRE_STORM,
                m::SAMA_GUARDIAN_FIRE,
                element::FIRE,
                None,
            ))
        }
        108 => {
            p.spell = Some(sama_kit(
                m::ICE_BLADES,
                m::GREATER_FROZEN_EARTH,
                m::ICE_STORM,
                m::SAMA_GUARDIAN_ICE,
                element::ICE,
                None,
            ))
        }
        109 => {
            p.spell = Some(sama_kit(
                m::THUNDER_BOLT,
                m::LIGHTNING_BEAM,
                m::LIGHTNING_WAVE,
                m::SAMA_GUARDIAN_LIGHTNING,
                element::LIGHTNING,
                None,
            ))
        }
        110 => {
            p.spell = Some(sama_kit(
                m::CYCLONE,
                m::BLOW_EARTH,
                m::DRAGON_TORNADO,
                m::SAMA_GUARDIAN_WIND,
                element::WIND,
                None,
            ))
        }
        111 => {
            p.spell = Some(sama_kit(
                m::ADAMANTINE_FIRE_BALL,
                m::SCORCHED_EARTH,
                m::FIRE_STORM,
                m::SAMA_GUARDIAN_FIRE,
                element::FIRE,
                Some(m::SAMA_PHOENIX_FIRE),
            ))
        }
        112 => {
            p.spell = Some(sama_kit(
                m::ICE_BLADES,
                m::GREATER_FROZEN_EARTH,
                m::ICE_STORM,
                m::SAMA_GUARDIAN_ICE,
                element::ICE,
                Some(m::SAMA_BLACK_ICE),
            ))
        }
        113 => {
            p.spell = Some(sama_kit(
                m::THUNDER_BOLT,
                m::LIGHTNING_BEAM,
                m::LIGHTNING_WAVE,
                m::SAMA_GUARDIAN_LIGHTNING,
                element::LIGHTNING,
                Some(m::SAMA_BLUE_LIGHTNING),
            ))
        }
        114 => {
            p.spell = Some(sama_kit(
                m::CYCLONE,
                m::BLOW_EARTH,
                m::DRAGON_TORNADO,
                m::SAMA_GUARDIAN_WIND,
                element::WIND,
                Some(m::SAMA_WHITE_WIND),
            ))
        }
        115 => {
            p.attack_range = 12;
            p.spell = Some(Spell::Random(vec![
                aoe(15, m::SAMA_PROPHET_FIRE, element::FIRE),
                aoe(15, m::SAMA_PROPHET_WIND, element::ICE),
                aoe(15, m::SAMA_PROPHET_LIGHTNING, element::LIGHTNING),
            ]));
            p.spell_chance_near = 1;
            p.spell_chance_far = 1;
        }
        116 => p.attack_range = 3,
        118 => {
            p.attack_range = 7;
            p.kite = true;
            p.fear_rate = 6;
            p.fear_duration_s = 2;
            p.spell = Some(Spell::Random(vec![
                Spell::Bolt {
                    magic: m::LIGHTNING_BALL,
                    element: element::LIGHTNING,
                    travel: false,
                    scale: 66,
                },
                bolt(m::THUNDER_BOLT, element::LIGHTNING, false),
                Spell::Aoe {
                    radius: 1,
                    magic: m::LIGHTNING_WAVE,
                    element: element::LIGHTNING,
                    at_self: false,
                    scale: 66,
                },
            ]));
            p.spell_chance_near = 1;
        }
        119 => p.hit_poison = poison(poison_kind::SILENCED, 1, 5, 10),
        120 => {
            p.immobile = true;
            p.attack_range = 18;
            p.doom_claw = true;
        }
        121 | 126 => {
            p.spell = Some(bolt(
                if ai == 126 {
                    m::GREEN_SLUDGE_BALL
                } else {
                    m::PINK_FIRE_BALL
                },
                if ai == 126 {
                    element::WIND
                } else {
                    element::PHANTOM
                },
                true,
            ));
            p.spell_chance_near = 5;
            p.spell_chance_far = 2;
        }
        124 => {
            p.passive = true;
            p.immobile = true;
            p.clamp_damage = true;
        }
        129 => {
            p.passive = true;
            p.self_aoe = 1;
        }
        131..=134 => {
            if ai != 134 {
                p.phase = Some(ai == 133);
            }
            if ai == 133 || ai == 134 {
                p.hit_poison = poison(poison_kind::PARALYSIS, 1, 5, 15);
            }
            if ai == 134 {
                p.attack_range = 11;
                p.spell = Some(line(12, 1, 1, m::MONSTER_DARK_BEAM, element::DARK));
                p.spell_chance_near = 1;
            }
        }
        _ => {}
    }
    // ---- Wave two class behaviours ----
    match ai {
        11 => p.revives = true,
        59 => p.weakness_every_ms = 10_000,
        78 => p.death_cloud_every_ms = 15_000,
        127 => p.death_cloud_every_ms = 8_000,
        // Castle lord: rays within 3, line attack, death clouds every 15 s.
        1000 => {
            p.attack_range = 3;
            p.rays_only = true;
            p.line_attack = 3;
            p.death_cloud_every_ms = 15_000;
        }
        30 | 69 => {
            p.gate = Some(if ai == 30 { 0 } else { 1 });
            p.passive = true;
            p.immobile = true;
            p.invulnerable = true;
        }
        74 => p.purify_every_ms = 20_000,
        _ => {}
    }
    if (107..=114).contains(&ai) {
        p.attack_range = 2;
        p.spell_chance_near = 5;
        p.spell_chance_far = 5;
    }
    // `NeedHarvest` / `HarvestCount` sit beside the class in Zircon's
    // registry rather than inside it, so they are applied separately: an
    // AI can be any shape and still need butchering.
    p.harvest_count = harvest_count(ai);
    p
}

/// Zircon `MonsterRegistrations.RegisterAll`: the `HarvestCount` each AI
/// class registers alongside `NeedHarvest = true`. 0 means the corpse is
/// not harvested and drops on death.
pub fn harvest_count(ai: i32) -> i32 {
    match ai {
        // Chicken.
        1 => 2,
        // Deer, Sheep, Cow, Pig.
        2 => 3,
        // Scorpion, Wolf.
        3 => 3,
        // Carnivorous Plant.
        5 => 2,
        // Spitting Spider, Visceral Worm, Blood Tiger.
        6 => 2,
        // Wedge Moth, Spider Bat, Cave Maggot.
        8 => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_cover_the_registered_ids() {
        assert!(profile(1).passive);
        assert!(profile(-1).guard && profile(-1).invulnerable);
        assert_eq!(profile(33).attack_range, 9);
        assert!(profile(7).kite && profile(7).fear_rate == 6);
        assert_eq!(profile(6).line_attack, 2);
        assert!(matches!(profile(26).spell, Some(Spell::Bolt { .. })));
        assert!(profile(87).hit_poison.unwrap().kind == poison_kind::ABYSS);
        assert!(profile(22).stages.as_ref().unwrap().count == 7);
        // Unregistered ids are plain melee.
        let p = profile(32);
        assert!(!p.passive && p.attack_range == 1 && p.spell.is_none());
    }

    /// The six classes Zircon registers with `NeedHarvest = true`, and their
    /// counts. Nothing else is harvested.
    #[test]
    fn harvest_counts_match_the_registry() {
        assert_eq!(profile(1).harvest_count, 2);
        assert_eq!(profile(2).harvest_count, 3);
        assert_eq!(profile(3).harvest_count, 3);
        assert_eq!(profile(5).harvest_count, 2);
        assert_eq!(profile(6).harvest_count, 2);
        assert_eq!(profile(8).harvest_count, 2);
        // The classes that share an arm with 6 do not harvest.
        assert_eq!(profile(14).harvest_count, 0);
        assert_eq!(profile(42).harvest_count, 0);
        assert_eq!(profile(0).harvest_count, 0);
        assert_eq!(profile(-1).harvest_count, 0);
    }
}
