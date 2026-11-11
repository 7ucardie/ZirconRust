# Research: guild wars and castles (from the C# server)

Extracted 2026-09-13 by direct reading. Sources: ServerLibrary/Models/
PlayerObject.Social.cs (`GuildWar` 973-1039, `GuildConquest` 1040-1123,
`AtWar` 1473-1495), ServerLibrary/Envir/SEnvir.cs (war tick 1825-1845,
conquest scheduling 1572-1577, `StartConquest` 2000-2040),
ServerLibrary/Models/ConquestWar.cs, Monsters/CastleObject.cs,
CastleLord.cs, CastleGuard.cs, CastleFlag.cs, CastleGate.cs,
LibraryCore/SystemModels/CastleInfo.cs, CastleGuardInfo.cs,
LibraryCore/Globals.cs:127 (`GuildWarCost`), LibraryCore/Network packets
(`C.GuildWar`, `C.GuildRequestConquest`, `S.GuildWarStarted/Finished`,
`S.GuildConquestDate/Started/Finished`, `S.GuildCastleInfo`),
PlayerObject.Combat.cs:1266-1280 (`GuildWarDeath`), PlayerObject.Stats.cs
`ApplyCastleBuff` 650-660.

## Guild war

- `C.GuildWar { GuildName }`: caller needs `GuildPermission.StartWar`;
  target guild by name (case-insensitive), not own guild, no war between
  the two yet; costs `Globals.GuildWarCost` = 200,000 from the guild funds
  (not the player's gold); creates `GuildWarInfo { Guild1, Guild2,
  Duration = 2 h }`; every member of both guilds gets
  `S.GuildWarStarted { GuildName, Duration }` (the declaring side also a
  `GuildFundsChanged`).
- The war tick subtracts elapsed time from `Duration`; at zero both
  guilds get `S.GuildWarFinished { GuildName }` and the row is deleted.
- `AtWar(player)`: if a conquest war runs on the current map, everyone
  not in the same guild (or without a guild) is an enemy; otherwise true
  when a `GuildWarInfo` links the two guilds.
- `CanAttackTarget` uses `AtWar` only in `WarRedBrown` mode (Peace never
  attacks, Group/Guild spare mates). `CheckBrown` skips at-war targets;
  `Die` with an at-war killer sends `GuildWarDeath` to both guilds and
  awards no PK points.

## Castles (`CastleInfo`)

- Fields: Name, Map, StartTime (time of day), Duration, CastleRegion,
  ObjectiveRegion, AttackSpawnRegion, Item (conquest request cost),
  Monster (the lord), Discount, plus Guards/Gates/Flags child lists. This
  asset pack has one row: Sabuk Wall on map 7, 19:00 for 30 minutes,
  regions 137/138, item 394, monster 220 (Sabuk Lord, AI 1000), discount
  0.10; no ObjectiveRegion column and no guard/gate/flag tables.
- `GuildConquest(index)`: leader only; guild owns no castle and has no
  pending `UserConquest`; castle exists; no war running; if the castle
  names an item the player must hold one and it is taken; `WarDate` =
  today + 2 days at midnight, plus one day when today's start time has
  passed; owner guild and requester guild get `S.GuildConquestDate`.
- Scheduler: once a day when the clock crosses `StartTime`,
  `StartConquest(castle, forced=false)`: participants = guilds whose
  `UserConquest.WarDate <= today` (rows consumed) plus the owner; none →
  no war. `forced` = empty participant list (anyone).
- `ConquestWar.StartWar`: broadcast "conquest started", hide NPCs in the
  objective region, `S.GuildConquestStarted`, `PingPlayers` (everyone on
  the map not in the owner guild teleports to AttackSpawnRegion), spawn the
  lord (AI 1000 `CastleLord`) or flag (1001) in ObjectiveRegion. `EndWar`
  after Duration: broadcast, NPCs back, ping, despawn lord,
  `S.GuildConquestFinished`, announce the owner.
- `CastleLord`: rays within 3, `LineAttack(3)` (1 in 3 or when the target
  is not within 2) else a frontal hit; death clouds on every target in
  view every 15 s; no regen, immune to poison. `Attacked`: only players
  (or pets of players) whose guild owns no castle and, when the list is
  non-empty, is a participant; every hit does 1 damage (`base.Attacked(
  attacker, 1, ...)`). `Die`: the EXP owner's guild takes the castle
  (previous owner loses it), `ConquestCapture` broadcast,
  `S.GuildCastleInfo`, ping players; the war runs on to its end time.
- `CastleGuard` (flag 11, `CastleGuardInfo` rows): immobile ranged guard
  (range 15) that during a war attacks players whose guild does not own
  the castle; hurt only by non-owner players. `CastleGate`: blocking
  object with open/close and repair. `CastleFlag` (AI 1001): 30 s capture
  by standing next to it. Gates/guards/flags need their info tables,
  absent here.
- Owner benefits: `ApplyCastleBuff` = ExperienceRate, DropRate and
  GoldRate +10 for members of the owner guild; `Discount` applies to
  castle NPC shops; `S.GuildCastleInfo { Index, Owner }` for every castle
  on login.

## Prototype

Implemented: guild wars (funds, permission, 2 h, both sides told, expiry,
war flags cached on `PlayerData.war_guilds` for `hostile_to`, brown/PK
exemptions, war-death lines), conquest requests with the item cost and the
day-after-tomorrow date, the daily start window plus
`ZIRCON_DEV_CONQUEST=1`, `start_conquest(forced)`, pinging non-owners to
the attackers' region, the lord (AI 1000 profile; 1 damage per hit from
eligible guilds; poison and regen immune), capture on death with the
broadcast and `CastleInfo`, ending on the clock, owners' +10 % experience.
Deviations: the pack has no ObjectiveRegion, so the lord spawns in the
castle region; guards/gates/flags are implemented from their info rows
(none in this pack; gate block layouts follow `FaceImage` 1-3); no drop/gold rate or
shop discount for owners; no NPC hiding; days are UTC; `GuildWarInfo` and
`UserConquest` live in `guilds.json`; the client shows castle state and
wars as text in the guild window and colours enemy-guild names orange
instead of a war tab.
