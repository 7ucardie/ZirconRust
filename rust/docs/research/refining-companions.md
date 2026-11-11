# Research: weapon refining and companions (from the C# server)

Extracted 2026-09-13 by direct reading. Sources: ServerLibrary/Models/
PlayerObject.NPC.cs (`NPCRefine` 1703-2037, `NPCRefineRetrieve` 2038-2180),
PlayerObject.Social.cs (`CompanionUnlock` 294, `CompanionAdopt` 364,
`CompanionRetrieve` 427, `CompanionRelease` 452, `CompanionStore` 483,
`CompanionSpawn` 498, `CompanionApplyBuff` 522), Monsters/Companion.cs
(`ProcessSearch` 181, `ProcessRoam` 227, `ProcessTarget` 374, `RefreshStats`
102), MapObject.cs 430-460 (companion buff tick), PlayerObject.Inventory.cs
1054-1067 (feeding), LibraryCore/Globals.cs 130-131, 289, 309-315, 843-860,
1016-1040, LibraryCore/Enum.cs (`NPCDialogType` 535, `RefineType` 1577,
`RefineQuality` 1604, `ItemType.RefineSpecial` 409, `ItemEffect.BlackIronOre`
1693), LibraryCore/Stat.cs (`Stat` ids), SystemModels/CompanionInfo.cs,
CompanionLevelInfo.cs, DBModels/RefineInfo.cs, UserCompanion.cs.

## Refining

- Pages: `NPCDialogType.Refine` (3) starts, `RefineRetrieve` (4) collects.
  `C.NPCRefine { RefineType, RefineQuality, Ores, Items, Specials }`,
  `C.NPCRefineRetrieve { Index }`; `S.RefineList { List<ClientRefineInfo> }`
  (Index, Weapon, Type, Quality, Chance, MaxChance, ReadyDuration) is sent
  on login and after a refine; `S.NPCRefineRetrieve { Index }` on collect.
- `RefineType`: None, Durability, DC, SpellPower, Fire, Ice, Lightning, Wind,
  Holy, Dark, Phantom, Reset. `RefineQuality`: Rush, Quick, Standard,
  Careful, Precise with waits 1 min, 30 min, 1 h, 6 h, 1 day
  (`Globals.RefineTimes`).
- Rules (`NPCRefine`): not dead, page is Refine, the equipped weapon must
  carry `UserItemFlags.Refinable` and not `NonRefinable`; cost 50,000 gold
  (`RefineCost`); up to 5 ore cells (ItemEffect BlackIronOre; `ore +=
  CurrentDurability`), 3 item cells (Necklace/Bracelet/Ring only; `items +=
  RequiredAmount`, `quality++` when Rarity != Common), 1 special
  (ItemType RefineSpecial, Shape 1; `special += Stats[MaxRefineChance]`).
  Cells may come from Inventory, Storage or the companion bag.
- Chance: `maxChance = 90 - weapon.Level + special` then Rush -5, Quick 0,
  Standard +5, Careful +10, Precise +20, capped at 100; `chance = 60 -
  weapon.Level * 5 + ore / 2000 + items / 6 + quality * 25`, capped at
  maxChance. Materials are consumed, the weapon leaves the equipment slot,
  a `RefineInfo` stores it with `RetrieveTime`.
- Retrieve (`NPCRefineRetrieve`): page RefineRetrieve, ready time passed,
  bag room; `Random(100) < Chance` succeeds: Durability `MaxDurability +=
  2000`; DC `AddStat(MaxDC, 1)`; SpellPower `MaxMC`/`MaxSC` +1 by what the
  weapon has (both when it has neither); elements add the matching
  `*Attack` +1 and set `WeaponElement` (Fire 1 .. Phantom 7); Reset puts
  Level to 1 and converts refine stats to enhancement stats at a fifth.
  Failure returns the weapon unchanged. Either way the weapon goes back
  to the bag with `GainItem`.
- Not modelled here: the `Refinable` flag (all weapons refine), Reset,
  MasterRefine / RefinementStone / WeaponReset pages, weapon experience
  levels, the companion bag as a material source. `weapon.Level` here
  counts successful refines.

## Companions

- Data: `CompanionInfo` (MonsterInfo look, Description, Price, Currency,
  Available, UnlockItem); `CompanionLevelInfo` (Level, MaxExperience,
  InventorySpace, InventoryWeight, MaxHunger; level 1 carries 0 items, 50
  weight, 100 hunger in this asset pack); `CompanionSkillInfo` (random
  stats at levels 3/5/7/10/11/13/15). `UserCompanion` (Account, Character,
  Info, Name, Level, Hunger, Experience, the level skills) is per account
  with the current owner character.
- Page `NPCDialogType.CompanionManage` (5): `C.CompanionUnlock { Index }`
  (consumes the UnlockItem, account-wide unlock), `C.CompanionAdopt {
  Index, Name }` (Available or unlocked, `GuildNameRegex` name, price in
  the info's currency; `S.CompanionAdopt { UserCompanion }`),
  `C.CompanionRetrieve { Index }` (only the owning character; despawn
  then spawn), `C.CompanionRelease { Index }`, `C.CompanionStore`.
- `Companion` object: `Blocking => false`, spawns beside the owner, roams
  to the cell behind the owner, `ProcessSearch` takes the nearest
  `ItemObject` in the owner's `VisibleObjects` within ViewRange that is
  the owner's account drop (`MonsterDrop`) and fits `CanGainItems(true)`
  (bag space from `Stat.CompanionInventory`, weight from
  `Stat.CompanionBagWeight`, both from the level table); gold pickups pay
  the guild tax. Teleports to the owner when out of range.
- Upkeep (companion buff tick every minute, MapObject.cs 430-460): hunger
  -1 unless in a safe zone at level >= 15; `Experience += max(highest
  companion level of the account, 1) + Stat.CompanionRate`; when it
  reaches `MaxExperience` (> 0) the level rises and skills are rolled.
  Feeding: consumables with `Stat.CompanionHunger` restore up to
  MaxHunger (PlayerObject.Inventory.cs 1054). A hungry companion (0)
  stops picking up.
- Client packets: `S.CompanionUnlock`, `S.CompanionAdopt`,
  `S.CompanionRetrieve`, `S.CompanionRelease`, `S.CompanionStore`,
  `S.CompanionWeightUpdate { BagWeight, MaxBagWeight, InventorySize }`,
  `S.CompanionShapeUpdate`, `S.CompanionItemsGained`, `S.CompanionUpdate {
  Level, Experience, Hunger }`, `S.CompanionSkillUpdate`.
- Not modelled here: level skills and their owner buff, companion
  equipment (head/back shapes), the companion bag as an item grid
  (items are taken one by one), speech, hunger-based auto feed from the
  bag, account-level ownership (per character here).
