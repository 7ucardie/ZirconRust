# Research: fame titles and Lua NPC scripts (from the C# server)

Extracted 2026-09-13 by direct reading. Rust: `world/fame.rs`, `world/lua.rs`,
NPC check/action arms in `world/npc.rs`.

## Fame

- Model: `CharacterInfo.Fame` (ServerLibrary/DBModels/CharacterInfo.cs:586)
  is the held `FameInfo.Index`; `Stats[Stat.Fame]` mirrors it
  (PlayerObject.Stats.cs:415). `FameInfo` (LibraryCore/SystemModels/
  FameInfo.cs): Name, Shape, Description, Cost, Order, `BuffStats`
  (FameInfoStat: Stat, Amount) and `ItemRewards` (FameInfoReward: Item,
  Amount). This asset pack ships 9 titles ("Unknown Novice" ... ), 44 stat
  rows and 7 rewards (gold).
- Currency: titles cost Fame Points, the `CurrencyInfo` with
  `Type == CurrencyType.FP` (4) (`PromoteFame`, PlayerObject.NPC.cs:2236).
  Nothing in the server grants FP by itself; it arrives through NPC
  `GiveCurrency` actions and quest currency rewards like any currency.
- `GetNextFameTitle` (PlayerObject.NPC.cs:2283): the title with the lowest
  `Order` above the held one's order (-1 when none held).
- `NPCCheckType.CheckFame` (21, NPCObject.cs:701): fails when there is no
  next title, no FP currency, or `nextFame.Cost > FP amount`.
- `NPCActionType.PromoteFame` (22, NPCObject.cs:217 -> `PromoteFame`):
  re-checks the cost, refuses with `FameNeedSpace` unless every item reward
  fits (`CanGainItems` without weight), deducts the cost, sets
  `Character.Fame`, hands over the rewards, `ApplyFameBuff` (a permanent
  `BuffType.Fame` buff carrying the title's stats, PlayerObject.Stats.cs:
  627-649) and `RefreshStats`.
- Display: `Stat.Fame` reaches the client in the stats; `CharacterDialog`
  (Client/Scenes/Views/CharacterDialog.cs:404-417) shows the title's icon
  and description from `FameInfoList`; inspecting another player carries
  `Fame` (PlayerObject.Chat.cs:402).
- Rust: `CharacterRecord.fame` persisted; `PlayerData.fame`; check 21 and
  action 22 as above; the title's stats are folded into the equipment stat
  map in `refresh_stats` (same effect as the buff); `PlayerStats` carries
  `fame` and `fame_title` and the character window shows a "Fame" row.
  Gold rewards go straight to the purse (the pack's rewards are gold).

## Lua scripts

- Host: `ServerLibrary/Envir/NpcScriptEngine.cs` (MoonSharp 2.0,
  `CoreModules.Preset_SoftSandbox`). Scripts live in `Scripts/NPC/` next to
  the server executable, keyed by `NPCPage.ScriptFile`
  (LibraryCore/SystemModels/NPCInfo.cs:221); cached per file name.
- Hooks: `on_open(player, npc)` runs when a scripted page opens
  (`ExecuteOnOpen`): `npc.navigate("Page description")` sends the player to
  that page (found by description), `npc.navigate("")` cancels the page.
  `NPCCheckType.Script` (22, NPCObject.cs:714) calls
  `StringParameter1(player, npc)` and fails only when the function returns
  boolean false (a non-boolean return passes; a missing script or function
  passes; a runtime error fails). `NPCActionType.Script` (23,
  NPCObject.cs:341) calls the function for its side effects.
- API (`LuaPlayerProxy` / `LuaNpcProxy`): `player.name`, `player.level`,
  `player.gold`, `player.has_item(name [, count])`; `npc.navigate(page)`,
  `npc.give_gold(n)`, `npc.take_gold(n)` (only when affordable),
  `npc.give_item(name [, count])` (only when it fits), `npc.take_item(name
  [, count])`, `npc.message(text)` (system chat).
- This asset pack's `NPCPage` table has no `ScriptFile` column (the column
  is read when present), so no shipped page is scripted; the Rust server
  loads scripts from `<data dir>/scripts/npc/`, then `$ZIRCON_SCRIPTS`,
  then the crate's `scripts/npc/` (which holds `example.lua`).

### Deviations

- Rust runs scripts through `mlua` (Lua 5.4, vendored) with only the
  math/string/table/utf8 libraries plus the base functions; MoonSharp's
  soft sandbox is close to that.
- Script effects are recorded as commands while the function runs and
  applied to the world afterwards (the world cannot be borrowed mutably
  from inside Lua callbacks); ordering within one call is preserved.
- A Lua return of `nil` passes a check, as Zircon's code does (it only
  fails on boolean `false`, whatever its doc comment says).
- `player.has_item` counts bag items only (Zircon: inventory only too).
