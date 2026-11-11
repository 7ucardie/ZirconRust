# Research: fishing and mining (from the C# server)

Extracted 2026-09-13 by direct reading. Sources: ServerLibrary/Models/
PlayerObject.Movement.cs (`FishingCast` 262-475, `Mining` 865-1001),
PlayerObject.cs 131-139 (CanMove/CanCast while fishing, FishingRobe check),
PlayerObject.Inventory.cs 2554 (`DamageItem`), LibraryCore/Functions.cs
675-725 (`ValidFishingDistance`, `FishingThrowQuality`, `FishingZone`),
LibraryCore/SystemModels/FishingInfo.cs + MineInfo.cs + MapInfo.cs 168
(`CanMine`), LibraryCore/Stat.cs 868-874, Enum.cs (`FishingState` 2109,
`EquipmentSlot` 109-113 Hook/Float/Bait/Finder/Reel, `ItemType` 29-33,
`ItemEffect` PickAxe = 5, FishingRod = 82, FishingRobe = 83, `SpellEffect.Rubble`),
Envir/Config.cs 146-153, LibraryCore/FrameSet.cs 94-96, Client/Models/
PlayerObject.cs 621-631 and 901-931 (animations, sounds, float effects),
Client/Scenes/Views/MapControl.cs 903-944 (click handling), packets
`C.FishingCast { State, Direction, FloatLocation, CaughtFish }`,
`C.Mining { Direction }`, `S.ObjectFishing { ObjectID, State, Direction,
FloatLocation, FishFound }`, `S.FishingStats { CanAutoCast, CurrentPoints,
ThrowQuality, RequiredPoints, MovementSpeed, RequiredAccuracy }`,
`S.ObjectMining { ObjectID, Direction, Location, Slow, Effect }`.

## Fishing

- Needs a weapon with `ItemEffect.FishingRod` and armour with
  `FishingRobe`; the rod loses 1 durability per cast (a dead rod reels
  in). Hook/Float/Finder/Reel accessory slots lose 4 each per cast, one
  bait (`UseBait`, slot Bait) is consumed per new cast or the cast ends
  with "Not enough bait".
- The float must land in a `FishingInfo` zone (a `MapRegion` of the map)
  within throw range: `Stat.ThrowDistance` level 1..4 allows 4/6/8/9
  cells (`ValidFishingDistance`); `FishingThrowQuality` is 1 (<= 4), 2
  (5-6), 3 (7-8), 4 (9). Cannot move or cast while fishing.
- A new cast rolls start points `Random(50 - 10)` plus
  `points * FinderChance / 100` (capped at 50) and the required accuracy
  `10 + clamp(Flexibility, 0, 15)`. Each `Cast` packet (the client re-sends
  one every `AttackDelay + 100` ms; `FishingCastTime` in `Process` resets
  fishing when it lapses) rolls the nibble: `Random(100) <
  10 (Config.FishNibbleChanceBase) + NibbleChance`.
- Once found, every cast is an attempt: `CaughtFish` adds
  `clamp(2 + ReelBonus, 2, 5)` points, a miss subtracts
  `clamp(5 - FloatStrength, 0, 5)` and counts a fail. At >= 50 the catch
  succeeds (perfect when more than one attempt and no fails), at <= 0 the
  fish is lost; both end the cast (state Reel).
- The drop walks the zone's `FishingDropInfo` rows by descending chance:
  skip `PerfectCatch` rows unless perfect, rows with a non-zero
  `ThrowQuality` other than the cast's, `Random(Chance) > 0`, or no bag
  room (`CanGainItems` without weight); the first hit gives one bound item.
- `S.FishingStats` goes to the caster after every cast; `S.ObjectFishing`
  is broadcast. Animations: FishingCast (2000, 8 frames, 100 ms),
  FishingWait (2080, 6, 120 ms), FishingReel (2160, 8, 100 ms); the float
  effect is `MagicEx5` 1420/1430 (1400/1410 while a fish nibbles) replayed
  at frame 1 of every wait cycle with sounds FishingCast/FishingBob/
  FishingReel.

## Mining

- `Mining(direction)` uses the attack timing (`AttackTime` 600 ms, delay
  max(800, AttackDelay - speed)); needs `MapInfo.CanMine` and a cell in
  front that is not a map cell (`GetCell(front) == null`: a wall), a weapon
  with `ItemEffect.PickAxe` that has durability (or none defined); the
  pickaxe loses 4 durability (`DamageItem`, which for weapons is skipped
  with chance `Strength`).
- Every `MineInfo` row of the map rolls `Random(Chance) == 0` for one
  bound item (newer schemas add a `Region`, `Quantity` and
  `RestockTimeInMinutes`; this asset pack has only Map/Item/Chance:
  Deserted Mine Lv 1 and 2, Quartz Mine, Dragon Abyss).
- A `Rubble` spell object grows under the miner (`Power++`, tick reset to
  one minute) or is spawned with a one-minute tick; the client draws
  ProgUse 230-234 by pile size. `S.ObjectMining { Effect }` is broadcast
  (true when rock was hit) and the client plays the weapon swing.

## Prototype

- Both loops are implemented server side (`world/gathering.rs`) with the
  point rules above; the client casts by clicking an unwalkable cell within
  range with a rod (or `ZIRCON_DEV_FISHING`) and mines by clicking an
  adjacent wall with a pickaxe; a click while fishing reels.
- This asset pack ships no `FishingInfo`/`FishingDropInfo` rows, rods,
  robes or bait, so `ZIRCON_DEV_FISHING=<item index>` makes every
  unwalkable cell a zone dropping that item and waives the tackle and
  bait checks.
- Deviations: `DamageItem` has no Strength save; a move or turn cancels
  the cast instead of being refused; rubble is drawn at the smallest pile;
  no perfect-catch milestones; the reel timing mini game is a single
  click per recast window rather than Zircon's moving-marker dialog;
  monsters on mining maps are not otherwise affected.
