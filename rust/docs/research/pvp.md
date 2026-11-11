# Research: PvP (from the C# server)

Extracted 2026-09-13 by direct reading. Sources: ServerLibrary/Models/
PlayerObject.Combat.cs (`CanAttackTarget` 890, `CanHelpTarget` 958, `Die`
~1225-1345, `DeathDrop` 1347, `CheckBrown` 1517, `IncreasePKPoints` 1559),
PlayerObject.cs (`ProcessNameColour` 748), MapObject.cs (PKPoint buff tick
536), Monsters/Guard.cs (`CanAttackTarget` 53), Envir/Config.cs 103-108,
SConnection.cs 1024 (`C.ChangeAttackMode`), LibraryCore/Enum.cs
(`AttackMode` 20, `FightSetting` 350), Client/Envir/CEnvir.cs 674 (Ctrl+H).

## Attack modes (`AttackMode`: Peace 0, Group 1, Guild 2, WarRedBrown 3, All 4)

`CanAttackTarget(ob)` for a player: never guards, GMs, the dead, invisible,
self; never when either side `InSafeZone`. Then by mode: Peace false;
Group false for group mates; Guild false for guild mates; WarRedBrown false
unless the target is brown, red (`PKPoint >= Config.RedPoint` 200) or at
war; All true. Pets use their owner's mode against players; your own pets
are hittable only in All (puppets always). `C.ChangeAttackMode { Mode }`
is echoed as `S.ChangeAttackMode`; the client cycles it with Ctrl+H.
`CanHelpTarget` is the mirror image (Peace helps everyone).

## Brown and PK points

- `CheckBrown(ob)` runs on every damaging hit (`Attack` 305, magic 608,
  pets 1124): skipped on Safe/Fight maps (`MapInfo.Fight`), in safe zones,
  when the victim is already brown or red, or at war. Otherwise the
  attacker gets buff `Brown` (Stat.Brown = 1) for `Config.BrownDuration`
  = 60 s.
- `Die`: if the killer is a player and the victim is innocent (not brown,
  not red): "murdered by" messages and `attacker.IncreasePKPoints(
  Config.PKPointRate = 50)`; a red killer has a 1 in `PvPCurseRate` (4)
  chance of a PvPCurse buff (-1 luck, 60 min, stacking). Killing a
  brown/red victim is "protected" (no points). Red players who die to
  monsters lose 10% durability on every equipped item.
- `IncreasePKPoints` keeps the total in a permanent `PKPoint` buff whose
  tick (`Config.PKPointTickRate` = 60 s) subtracts one point; reaching red
  moves the bind point to a `RedZone` safe zone.
- `ProcessNameColour`: white; DeepPink for rebirths; red at >= 200
  points; else brown while the Brown buff lasts; else yellow at >= 50.
  Sent as `NameColour` in the object data / `S.ObjectNameColour`.
- Guards (`Guard.CanAttackTarget`) attack red players (and red players'
  pets) unless `Stat.Redemption`, plus non-passive wild monsters.
- `DeathDrop` only runs with `Stat.DeathDrops > 0` (an item/buff stat):
  each `CanDeathDrop`, unbound inventory item has a 1 in 10 chance to drop
  a random part of its stack within 4 cells.
- Rebirth characters that die to monsters lose all experience of the
  level (given to a random low-level player).

## Prototype (what the Rust server does)

Attack modes persisted per character; `Object.in_safe_zone` refreshed on
every move; `hostile_to` applies the mode rules (Guild mode = All while
there are no guilds; pets never target players); brown flag for 60 s on
hitting an innocent; 50 PK points per murder, decaying one per minute;
name colour codes 0-3 in `Appearance::Player`; guards hostile at 200.
Not done: Fight/Safe map settings, PvPCurse, death drops, red durability
loss, redemption, red bind points, rebirth experience loss.
