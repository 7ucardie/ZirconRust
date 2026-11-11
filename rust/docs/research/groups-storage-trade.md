# Groups, Storage and Trading — C# reference rules

Research notes for the Rust rewrite. All paths are relative to the repo root; `Social.cs` =
`ServerLibrary/Models/PlayerObject.Social.cs`, `Inventory.cs` = `ServerLibrary/Models/PlayerObject.Inventory.cs`,
`SConn` = `ServerLibrary/Envir/SConnection.cs`, `CConn` = `Client/Envir/CConnection.cs`,
`CP` = `LibraryCore/Network/ClientPackets.cs`, `SP` = `LibraryCore/Network/ServerPackets.cs`.

Shared state on `PlayerObject` (`ServerLibrary/Models/PlayerObject.cs`):
- `GroupMembers: List<PlayerObject>` lives on `MapObject` (`MapObject.cs:108`); the **same list instance is shared** by
  every member, `GroupMembers[0]` is the leader. `null` = not grouped.
- `GroupInvitation: PlayerObject` (pending invite, `PlayerObject.cs:160`), `GroupInvitationRequest: HashSet<PlayerObject>` (LFG join requests, `:161`).
- `TradePartner`, `TradePartnerRequest` (`:163`), `TradeItems: Dictionary<UserItem, CellLinkInfo>` (`:164`), `TradeConfirmed` (`:165`), `TradeGold: long` (`:166`).
- `Storage = new UserItem[1000]`, `PartsStorage = new UserItem[1000]` (`:153-154`); account-level, filled from `Character.Account.Items` (`:194-203`).

---

## 1. GROUPS

### Constants
| Name | Value | Source |
|---|---|---|
| `Globals.GroupLimit` | 15 | `LibraryCore/Globals.cs:95` |
| `Globals.LookingForGroupMinutes` | 60 | `Globals.cs:96` |
| `Config.MaxViewRange` (exp/drop share radius) | 18 | `ServerLibrary/Envir/Config.cs:96` |
| `AccountInfo.AllowGroup` | persisted bool, default `false` | `ServerLibrary/DBModels/AccountInfo.cs:339-352` |

### AllowGroup toggle — `C.GroupSwitch { Allow: bool }` (`CP:332`) → `PlayerObject.GroupSwitch` (`Social.cs:1574-1595`)
- No-op if unchanged (`:1576`). Refused with `InstanceNoAction` if any member is inside an instance map (`:1578`).
- Sets `Account.AllowGroup`, replies `S.GroupSwitch { Allow }` (`SP:710`, `Social.cs:1589`).
- **Turning it off while grouped leaves the group** (`:1591-1592`).
- Client: checkbox in GroupDialog sends `C.GroupSwitch { Allow = !AllowGroup }` (`Client/Scenes/Views/GroupDialog.cs:263`); `S.GroupSwitch` sets `GroupBox.AllowGroup` (`CConn:3337`).

### Invite — `C.GroupInvite { Name }` (`CP:337`) → `GroupInvite(name)` (`Social.cs:1641-1723`)
Checks, in order (each failure is a system chat message, English text in `ServerLibrary/Envir/Translations/EnglishMessages.cs:214-221`):
1. If already grouped, caller must be leader (`GroupMembers[0] == this`) else `GroupNotLeader` (`:1643`). An ungrouped player may invite (the group is created on accept).
2. Target resolved by character name, case-insensitive (`SEnvir.GetPlayerByCharacter`, `SEnvir.cs:4016`); offline → `CannotFindPlayer` (`:1653`).
3. Target already grouped → `GroupAlreadyGrouped` (`:1663`).
4. Target already has a pending `GroupInvitation` → `GroupAlreadyInvited` (`:1672`).
5. Target `!AllowGroup` or either account blocks the other (`SEnvir.IsBlocking`, `SEnvir.cs:3948`) → `GroupInviteNotAllowed` (`:1681`).
6. Self → `GroupSelf` (`:1690`).
7. Instance-map sequence check (`:1699-1710`) — skip for prototype.
8. If target previously sent an LFG join request (`GroupInvitationRequest.Contains`), auto-join immediately (`:1712-1718`).
9. Otherwise `player.GroupInvitation = this; player.Enqueue(S.GroupInvite { Name })` (`:1721-1722`, `SP:723`).

**No timeout** on the pending invite. It is cleared only when: the invitee responds (`SConn:715`), the inviter's object is gone (`GroupInvitation.Node == null`, checked in `Process`, `PlayerObject.cs:299` and `Social.cs:1800`), or the invitee disconnects (`PlayerObject.cs:1463`).

### Response — `C.GroupResponse { Name, Accept }` (`CP:347`) → `SConn:706-716`
- `Accept` → `GroupJoin()`; else `GroupDecline(name)` (only meaningful for LFG requests, `Social.cs:1869-1886`). Then `GroupInvitation = null` always.
- Client shows a YesNo message box "Do you want to group with {Name}?" (`CConn:3388-3395`).

### Join — `GroupJoin()` (`Social.cs:1798-1867`)
1. Return if no valid invitation or already grouped (`:1800-1802`).
2. If inviter has no group: inviter gets `GroupSwitch(true)` (forces AllowGroup on), `GroupMembers = [inviter]`, and inviter is sent `S.GroupMember { ObjectID, Name }` for **itself** (`:1804-1811`).
3. Else inviter must still be leader (`:1812`) and `Count < Globals.GroupLimit` (15) else `GroupMemberLimit` (`:1820`).
4. Joiner must not be on an instance map (`:1829`).
5. `GroupMembers = inviter.GroupMembers; GroupMembers.Add(this)` (`:1838-1839`).
6. Broadcast: for every existing member `ob`: `ob.Enqueue(S.GroupMember{joiner})` and joiner gets `S.GroupMember{ob}` (`:1845-1846`); each member re-runs `AddAllObjects`, `RefreshStats`, `ApplyGuildBuff` (`:1848-1850`). Finally joiner gets `S.GroupMember{self}` (`:1867`).
   So a joiner receives one `GroupMember` per existing member **in list order (leader first)** then itself last; every member's own entry is sent to itself.
7. Client (`CConn:3341-3354`): appends to `GroupBox.Members`, prints `GroupJoin` chat line (MessageType.Group), refreshes big/mini map markers. **Leader = `Members[0]`** on the client too: Remove button enabled only when `Members[0].ObjectID == User.ObjectID` (`GroupDialog.cs:101`). There is no explicit leader packet.

### Leave / kick
- `GroupLeave()` (`Social.cs:1888-1926`): removes self, sends `S.GroupRemove { ObjectID = self }` (`SP:719`) to every remaining member and to self (`:1890-1901`, `:1921`); all of them `RemoveAllObjects/RefreshStats/ApplyGuildBuff`. **If one member remains, that member is also removed** (`:1910`) — groups of size 1 do not exist. Leader leaving: the list simply drops index 0, so the next member becomes leader (no notification beyond `GroupRemove`).
- Kick `C.GroupRemove { Name }` (`CP:342`) → `GroupRemove(name)` (`:1597-1639`): requires a group (`GroupNoGroup`), leader (`GroupNotLeader`), no instance; finds member by name (case-insensitive) and calls `member.GroupLeave()`; else `GroupMemberNotFound`. A leader may remove themself this way.
- Auto-leave on disconnect/logout (`PlayerObject.cs:964`) and on `GroupSwitch(false)` (`Social.cs:1591`).
- Client `S.GroupRemove` (`CConn:3355-3386`): if the removed ObjectID is self, clear the whole member list; else remove that entry; prints `GroupRemove` chat line.

### What a group grants
- Group chat: text starting with `"!!"` (`ServerLibrary/Models/PlayerObject.Chat.cs:94-107`) → each member gets `"{Name}: {text[2..]}"` as `MessageType.Group`, skipping blocked pairs and honoring chat bans.
- Cross-map data visibility: `CanDataBeSeenBy` returns true for group members (`MapObject.cs:1037`), so members are sent data objects (map markers/HP) wherever they are on the same instance; cloaked/transparent members are visible to their group (`MapObject.cs:1073`).
- Pets don't attack group members (`MonsterObject.cs:840,927,1028`).
- 8+ members with ≥2 of every class (Warrior/Wizard/Taoist/Assassin) gain +10% base HP/MP (`PlayerObject.Stats.cs:256-284`).
- Instances, guild buff propagation and `@GroupRecall` (leader-only, 3-min cooldown, `Commands/Command/Player/GroupRecall.cs`) — out of scope.

### Experience sharing — `MonsterObject.YieldReward()` (`ServerLibrary/Models/MonsterObject.cs:2034-2174`)
- `EXPOwner` = the first player (or pet owner) to damage the monster; it is never reassigned while set (`MonsterObject.cs:1914`), cleared on the owner's death (`PlayerObject.Combat.cs:1141`) and on monster death (`:2031`). `EXPOwnerTime` is written (`:1917`) but never checked — no tag expiry.
- If `EXPOwner.GroupMembers != null`, iterate members; a member **qualifies for drops** (`dPlayers`) when `CurrentMap == monster map && InRange(member, monster, Config.MaxViewRange=18)` (`:2051`); it **qualifies for exp** (`ePlayers`) if additionally `!Dead` (`:2070`). `totalLevels += level` over `ePlayers` (`:2087`). No level-difference rule (the level penalty in `GainExperience` is commented out, `PlayerObject.Stats.cs:37-49`).
- Class-diversity bonus: let `m = min(count of each of the 4 classes)` among qualifying members. Exp rate ×1.1/1.25/1.5 for m=1/2/3 (`:2100-2111`); drop rate ×1.1/1.2/1.3 (`:2090-2099`).
- `eRate` also × map exp rate and growth level (`:2114-2118`); `exp = min(Experience * eRate, 500_000_000)` (`:2120`).
- If `ePlayers` is empty (solo, or nobody qualifying): owner gains full `exp` only if alive, same map, within 18 (`:2124-2141`).
- Else: `if ePlayers.Count > 1: exp += exp * 0.06 * ePlayers.Count` (+6% per nearby member, `:2145-2146`), then each player gets `exp * player.Level / totalLevels` (`:2150`) via `GainExperience(amount, PlayerTagged, monsterLevel)` — which further applies the player's own `ExperienceRate`/`BaseExperienceRate` stats and rebirth halving (`Stats.cs:24-34`).

### Drops in a group (`MonsterObject.cs:2160-2171`, `Drop()` `:2176+`)
- If no `dPlayers`: `Drop(EXPOwner, players=1, dRate)` (owner must be alive, same map, in range). Else **every** qualifying member gets an independent `Drop(member, dPlayers.Count, dRate)` roll: item chance is divided by `players` (`chance = int.MaxValue / (drop.Chance * players) * rate`, `:2208`), gold amount is divided by `players` (`:2196`). So a 3-person group rolls 3 times at 1/3 chance each and each successful roll spawns an item owned by that member.
- Each spawned `ItemObject` gets `Account = owner.Character.Account`, `MonsterDrop = true` (`:2292-2297`), `ExpireTime = now + Config.DropDuration` (60 min, `ItemObject.cs:186`, `Config.cs:121`), placed within `Config.DropDistance = 5` (`Config.cs:122`).
- Pickup ownership `ItemObject.CanPickUpItem` (`ItemObject.cs:52-76`): if `Account != null && Account != picker.Account` → false, unless `Config.DropVisibleOtherPlayers` (default **false**, `Config.cs:133`) in which case: anyone after 10 min, same guild after 5 min, **same group after 2 min** (`:60-70`). Player-dropped items have `Account = null` unless Bound (`Inventory.cs:2384-2390`), i.e. free for all.
- Pickup is explicit (`PickUp()`, `Inventory.cs:2491`) scanning `Stats[PickUpRadius]` around the player.

### LFG (Looking For Group) — optional
`C.GroupLFGUpdate{Enabled,Name,Type,MaxCount}`, `C.GroupNotify{Receive}`, `C.GroupRequest{Name}` (`CP:352-368`); `S.GroupRequest{Name,Level,Class}`, `S.GroupLFG{List}`, `S.GroupUpdate{Group}` (`SP:727-742`); `ClientLookingForGroup` (`Globals.cs:1200-1211`). Listing expires after 60 min (`Social.cs:2020`). Skip for prototype.

---

## 2. STORAGE

### Model
- Storage is **per account**, shared by all characters: items with `UserItem.Account` set; `Slot < 2000` = main storage, `Slot >= Globals.PartsStorageOffset (2000)` = parts storage (`PlayerObject.cs:194-203`, `Globals.cs:293`). Arrays are 1000 long (`PlayerObject.cs:153`), usable size is `Account.StorageSize`.
- `AccountInfo.StorageSize` (`AccountInfo.cs:462-475`) defaults to `Globals.StorageSize = 100` (`Globals.cs:292`; `AccountInfo.cs:627,643`).
- Expansion: item with `ItemEffect` case 17 "Storage Increase" adds +10, refused with `StorageLimit` if `size >= Storage.Length` (1000) (`Inventory.cs:699-711`); sends `S.StorageSize { Size }` (`SP:1297`). Client stores it in `GameScene.StorageSize` (`CConn:4549`) and resizes the grid to `10 × ceil(size/10)` rows, min 10 (`Client/Scenes/Views/StorageDialog.cs:337`).
- No gold in storage; gold is a currency on the account (`StartInformation.Currencies`).

### How the client gets storage contents
- Sent once at login in `S.Login { Characters, Items }` where `Items = account.Items` (`SP:20-28`, `SEnvir.cs:3343`); client `CEnvir.FillStorage` splits by slot ≥ 2000 (`Client/Envir/CEnvir.cs:511-527`). `StartInformation` carries `StorageSize`, `AllowGroup`, `AllowTrade`, character `Items` (`Globals.cs:336+`, `PlayerObject.cs:826-874`).
- Thereafter storage changes arrive as ordinary `S.ItemMove`/`S.ItemSort`/`S.ItemSplit`/`S.ItemsChanged` with `GridType.Storage`/`PartsStorage`.

### How storage "opens"
- **There is no NPC storage action / packet.** The storage window is a normal toggle (`MenuDialog.cs:151`) and is also auto-shown when the NPC repair dialog opens (`NPCDialog.cs:1289`, the NPC "Storage" button just bulk-moves storage items into the repair grid `NPCDialog.cs:1388-1403`). `NPCDialogType` has no storage entry (`LibraryCore/Enum.cs:535-556`).
- The server-side gate is **`InSafeZone`** (or TempAdmin): any move whose from- or to-grid is `Storage`/`PartsStorage` outside a safe zone is refused with `StorageSafeZone` ("You cannot access storage outside of SafeZone.", `Inventory.cs:1370-1387,1439-1462`; `EnglishMessages.cs:85`). The client pre-checks the same (`Client/Controls/DXItemCell.cs:1227-1239`).

### Grid moves — `C.ItemMove { FromGrid, ToGrid, FromSlot, ToSlot, MergeItem }` (`CP:141-148`) → `ItemMove` (`Inventory.cs:1343-1885`)
- Reply is always `S.ItemMove { same fields, Success }` (`SP:542-551`), sent **before** validation with `Success=false`, then `Success=true` set on the same object at the end (`:1345-1356`, `:1871`). Rust: send once after validation.
- Refused if `Dead` or from==to (`:1358`). Marriage-flagged items never move (`:1483,1503`).
- Slot bounds: `FromSlot/ToSlot < Account.StorageSize` for `Storage` (`:1388,1462`); array length for others (`:1479,1498`).
- `ToGrid == Storage`: item must not be an `ItemEffect.ItemPart` (`:1457`); `ToGrid == PartsStorage`: must be an ItemPart (`:1445`). **`ItemInfo.CanStore` is only enforced client-side** (`DXItemCell.cs:1237`); the server does not check it. Bound items are allowed in storage.
- Client only allows dragging into `Storage` from `Inventory`/`Equipment` (`DXItemCell.cs:1236`); the server accepts any of Inventory/Equipment/Storage/PartsStorage/GuildStorage/Companion grids as source (`:1362-1418`).
- Equipment source: cannot move to Equipment; destination must be empty unless merging (`:1505-1511`). To Equipment: `CanWearItem` (`:1532`).
- **Weight is not checked** for Storage↔Inventory moves (only Companion inventory checks weight, `:1546-1595`); `RefreshStats()` at the end recomputes `BagWeight` (`:1873`), so a player can become overweight by withdrawing.
- Merge (`MergeItem`, `:1598-1699`): same `Info`, dest not full, same `ExpireTime`, same Bound/Worthless/Expirable/NonRefinable flags, `Stats.Compare` equal; fills dest up to `StackSize`, deletes source if emptied; result `Success=true`, `RefreshWeight()`.
- Swap (`:1716-1717`): `from[FromSlot] = toItem; to[ToSlot] = fromItem`, then update `Slot`/owner: Inventory → `Slot = ToSlot, Character = char`; Equipment → `Slot = ToSlot + EquipmentOffSet(1000)`; Storage → `Slot = ToSlot, Account = acc`; PartsStorage → `Slot = ToSlot + 2000, Account = acc` (`:1807-1834`).
- `C.ItemSort { Grid }` / `C.ItemSplit { Grid, Slot, Count }` also accept Storage/PartsStorage, length-capped by `StorageSize` (`:1893-1907`, `:2201-2234`).
- Trade can draw items directly from Storage/PartsStorage while in a safe zone (`Social.cs:2188-2207`).

---

## 3. TRADING

### Constants / flags
- `AccountInfo.AllowTrade` (`AccountInfo.cs:354-367`), toggled by chat command `@AllowTrade`/ToggleTrade (`Commands/Command/Player/ToggleTrade.cs:14`). Sent in `StartInformation.AllowTrade`.
- Max 15 items per side (`Social.cs:2171`). Distance: **exactly 1 cell and facing each other**. No timeout anywhere.
- Untradeable: `UserItemFlags.Bound` (2) or `!ItemInfo.CanTrade` (`LibraryCore/SystemModels/ItemInfo.cs:313`) unless either party is admin (`Social.cs:2233`); `UserItemFlags.Marriage` (128) always refused (`:2234`). Flags enum at `Enum.cs:1773-1786`.

### Packets
Client→Server (`CP:486-508`): `TradeRequest {}`, `TradeRequestResponse { Accept }`, `TradeClose {}`, `TradeAddGold { Gold: long }`, `TradeAddItem { Cell: CellLinkInfo }`, `TradeConfirm {}`. `CellLinkInfo { GridType, Slot, Count: long }` (`Globals.cs:824-829`).
Server→Client (`SP:923-954`): `TradeRequest { Name }`, `TradeOpen { Name }`, `TradeClose {}`, `TradeAddItem { Cell, Success }` (echo of own add), `TradeAddGold { Gold }` (own total), `TradeItemAdded { Item: ClientUserItem }` (partner's item, `Count` = offered count), `TradeGoldAdded { Gold }` (partner's total), `TradeUnlock {}` (re-enable partner's Confirm button). Plus `S.ItemsChanged { Links, Success }` (`SP:644`) and `S.ItemsGained`/currency updates on completion.

### Request — `TradeRequest()` (`Social.cs:2078-2148`), dispatched from `SConn:1071`
1. `TradePartner != null` → `TradeAlreadyTrading`; `TradePartnerRequest != null` (an inbound request pending on **me**) → `TradeAlreadyHaveRequest` (`:2080-2089`).
2. Target = first `PlayerObject` in the cell directly in front (`Functions.Move(CurrentLocation, Direction)`, `:2091-2099`); must be facing me (`player.Direction == ShiftDirection(Direction, 4)`) else `TradeNeedFace` (`:2102`).
3. Blocking → `TradeTargetNotAllowed` (`:2108`); target trading → `TradeTargetAlreadyTrading`; target has pending request → `TradeTargetAlreadyHaveRequest`; target `!AllowTrade` → both get messages (`:2128-2133`); either dead → `TradeTargetDead` (`:2136`).
4. `player.TradePartnerRequest = this; player.Enqueue(S.TradeRequest { Name })`; requester gets `TradeRequested` chat (`:2144-2147`). Client shows YesNo box; No/close sends `TradeRequestResponse{Accept=false}` (`CConn:3718-3725`).

### Accept — `C.TradeRequestResponse` → `SConn:1077-1085` → `TradeAccept()` (`Social.cs:2149-2160`)
- Handler always clears `TradePartnerRequest` afterwards. Accept is silently ignored unless the requester is still online, not trading, not dead, `Distance == 1` and still facing me (`:2151-2152`).
- On success both sides link (`TradePartner`) and each receives `S.TradeOpen { Name = other }` (`:2158-2159`). Client opens `TradeBox`, sets `IsTrading` (`CConn:3726-3731`).

### Add item — `TradeAddItem(cell)` (`Social.cs:2162-2247`)
- Echoes `S.TradeAddItem { Cell, Success=false }` first (`:2164-2169`); `Success=true` set later on the same object. Rust: send once.
- Reject if `!ParseLinks(cell)`, no partner, or already 15 items (`:2171`). Source grids: Inventory, Equipment, CompanionInventory, PartsStorage/Storage (safe zone only) (`:2175-2229`). GuildStorage is commented out.
- Reject if slot empty, `Count > item.Count`, Bound/!CanTrade (non-admin), Marriage, or the same `UserItem` already offered (`:2233-2236`).
- Store `TradeItems[item] = cell`; partner gets `S.TradeItemAdded { Item }` with `Item.Count = cell.Count` (`:2240-2246`). Items are **not removed** from the bag; the client links the cell (`CConn:3737-3785`). There is no "remove item from trade" packet — cancel the trade instead.

### Add gold — `TradeAddGold(gold)` (`Social.cs:2248-2270`)
- Echo `S.TradeAddGold { Gold = TradeGold }`. Only accepted if `gold > TradeGold` (can only raise), `gold > 0`, `gold <= Gold.Amount` (`:2256-2258`). Partner gets `S.TradeGoldAdded { Gold }`. Gold is not escrowed until completion.

### Confirm — `TradeConfirm()` (`Social.cs:2272-2578`)
1. `TradeConfirmed = true`. If partner not yet confirmed: `TradeWaiting` to me, `TradePartnerWaiting` to partner; return (`:2276-2284`). Client disables its Confirm button on send (`TradeDialog.cs:238-240`).
2. Gold checks: `myGold + partner.TradeGold - TradeGold < 0` or the mirror → `TradeNoGold`/`TradePartnerNoGold` and **`TradeClose()`** (`:2286-2306`).
3. Item validity re-check for both sides: the array slot must still hold the same `UserItem` with `Count >= offered` else `TradeFailedItemsChanged` / `TradeFailedPartnerItemsChanged` and `TradeClose()` (`:2311-2366`, `:2394-2440`). Offered items are aggregated into `ItemCheck`s (stackable by Info + identical flags, `:2372-2390`).
4. Space check `partner.CanGainItems(checkWeight=false, checks)` (`Inventory.cs:82-147`: stacks into existing partial stacks then needs free inventory slots; **weight is ignored** because `checkWeight=false`). Failure → the side lacking space gets `TradeNotEnoughSpace`, its `TradeConfirmed = false` and `S.TradeUnlock`; the other gets `TradeWaiting`; the trade stays open (`:2392-2399`, `:2465-2473`).
5. Transfer (`:2475-2557`): `S.ItemsChanged { Links = my cells, Success=true }` to each side, then for each offered item: partial stacks → decrement source and `GainItem(fresh copy with Count)`; whole item → `fromArray[slot] = null; RemoveItem(item); partner.GainItem(item)`. Then `RefreshStats`, `SendShapeUpdate`, gold `Gold.Amount += partner.TradeGold - TradeGold` on both with `GoldChanged()` (`:2564-2568`), `TradeComplete` chat, `TradeClose()` (`:2575-2577`).

### Close / cancel — `TradeClose()` (`Social.cs:2058-2076`)
- Sends `S.TradeClose` to both, clears `TradePartner`, `TradeItems`, `TradeGold`, `TradeConfirmed` on both. Idempotent when no partner.
- Triggered by: `C.TradeClose` (`SConn:1086`, client sends it when the window hides, `TradeDialog.cs:30-32`); **any direction change / step** (`PlayerObject.Movement.cs:46-47`, `OnLocationChanged` `PlayerObject.cs:1388`, also fired by teleport/map change via `MapObject.LocationChanged` `MapObject.cs:868-873`); death (`PlayerObject.Combat.cs:1136`); disconnect (`PlayerObject.cs:951`). Client clears the window on `S.TradeClose` (`CConn:3732`).

---

## 4. Acceptable prototype deviations
- Drop the "echo packet first, then mutate Success" pattern: send `S.ItemMove`/`S.TradeAddItem`/`S.TradeAddGold` once with the final `Success`.
- Ignore observers (`ObserverPacket`, `Connection.Observers`), instances (`InstanceNoAction`), LFG, guild storage, companion grids, `PartsStorage`, marriage flag, admin bypasses, milestones, `IsBlocking`.
- Group: implement invite/accept/leave/kick, leader = index 0, `GroupLimit=15`, dissolve at 1 member, `!!` chat, exp split `exp*(1+0.06*n if n>1) * level/totalLevels` over alive same-map members within 18 cells, per-member drop rolls with chance/gold ÷ n. The class-diversity multipliers, 8-member HP bonus and cross-map data visibility can wait.
- Drop ownership: keep `Account` owner + 60-min expiry; with `DropVisibleOtherPlayers=false` (the default) others can never pick up, so the 2/5/10-minute unlocks are optional.
- Storage: no NPC involvement; gate on safe zone, cap at `StorageSize` (100), no weight check on withdraw, skip `CanStore` (client-only) or enforce it server-side — either is fine. Send account items in the login packet and keep the +10 expansion item.
- Trade: keep face-to-face adjacency, 15-item cap, raise-only gold, Bound/CanTrade rejection, unlock-on-no-space, close on any move. Fixed-timer expiry of requests may be added (C# has none). `CanGainItems` may be simplified to "free slots ≥ distinct stacks".
