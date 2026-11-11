# Harvesting, and what the AI coverage report was hiding

Status: implemented during worktree recovery on 2026-09-14. The historical
gap analysis below explains the scope; the server now holds loot on corpses
and the client supports harvesting with `B` or a nearby corpse click.

Written 2026-09-14, from `ServerLibrary/Models/MonsterObject.cs` and
`ServerLibrary/Models/MonsterRegistrations.cs`.

## The finding

`report_monster_ai_coverage` (an ignored test in `mir-server/src/tests.rs`)
prints 17 AI classes as "DEFAULT", meaning their profile is identical to the
fallback. That reads like a large gap. It is not: cross-referencing every one
of them against `MonsterRegistrations.RegisterAll` shows most are plain
`MonsterObject` in the C# too, so the fallback is the correct behaviour.

```
cargo run -p mir-formats --example dbdump -- ~/zircon-assets/Database/System.db <Table>
ZIRCON_ASSETS=~/zircon-assets/Client cargo test -p mir-server \
  report_monster_ai_coverage -- --ignored --nocapture
```

AI classes on the fallback profile that are also plain `MonsterObject` in the
C#, so nothing is missing: 0, 3, 15, 35, 37, 39, 40, and the passive -2 group.

AI classes on the fallback profile that DO have a bespoke C# class:

| AI | C# class | Spawns in this pack |
|---|---|---|
| 28 | `WindfurySorcerer` | 1 |
| 52 | `WhiteBone` | 0 |
| 99 | `JinhwanSpirit` (self-summoning) | 3 |
| 122 | `QuartzTurtleSub` | 0 |
| 128 | `Doll` | 0 |
| 130 | `UndeadSoul` | 0 |
| 1001 | `CastleFlag` | 0 (flags are handled in `castle_parts.rs`) |

Four spawns in the whole pack. Low value.

## The real gap: harvesting

`MonsterObject` carries `NeedHarvest` and `HarvestCount`, and the registry
sets them on AI **1, 2, 3, 5, 6 and 8**. Such a corpse does not drop on death.
The killer has to harvest it, up to `HarvestCount` times, and the server
answers each swing with `S.ObjectHarvested`. `MonsterObject.cs` line 2507 is
the entry point and 2636 is the packet; 2653 shows the corpse becomes a
skeleton once it is spent and nobody is owed a drop.

Nothing in `crates/mir-server/src/` mentions harvest at all, so this is not
implemented.

It is not a rare mechanic. In this pack it covers 14 monster types across 54
spawns, and they are the first things a new character meets:

| AI | Monsters | Spawns |
|---|---|---|
| 1 | Chicken | 4 |
| 2 | Deer, Sheep, Cow, Pig | 15 |
| 3 | Scorpion, Wolf | 9 |
| 5 | Carnivorous Plant | 3 |
| 6 | Spitting Spider, Visceral Worm, Blood Tiger | 7 |
| 8 | Wedge Moth, Spider Bat, Cave Maggot | 16 |

So today a player kills a chicken in Bichon Town and gets its drop straight
away, where Zircon makes them butcher the corpse for it. That is a visible
divergence in the opening minutes of the game, and it is the best remaining
monster-side target.

## Suggested scope

- Server: `need_harvest` / `harvest_count` on the monster definition from the
  AI class, corpses holding their drops until harvested, the harvest action
  itself, the skeleton state when spent, and drop ownership unchanged.
- Client: the harvest swing and whatever `ObjectHarvested` drives, plus the
  skeleton corpse sprite.
