# Research: NPC actions, quests, events (from the C# reference)

Extracted 2026-09-12 from the C# server for the Rust content stage. File:line
references point into the C# projects (reference only, never edited).

## 1. NPC actions / checks

Rust `world/npc.rs` already has: `npc_run_page` (mirrors `NPCObject.NPCCall`,
NPCObject.cs:24-71), checks 0-21 (several stubbed), actions 0-4 only
(Teleport map-only, GiveGold, TakeGold, GiveItem, TakeItem).

`NPCCheckType` (NPCInfo.cs:890-930): Level=0, Class=1, Gender=2, Gold=3,
HasItem=4, PKPoints=5, HasWeapon=6, WeaponLevel=7, WeaponElement=8,
WeaponCanRefine=9, Horse=10, Marriage=11, WeddingRing=12, CanGainItem=13,
CanResetWeapon=14, Random=15, WeaponAddedStats=16, Currency=17, RollResult=18,
CheckDataList=19, CheckDataValue=20, CheckFame=21, Script=22.
Operator: Equal=0, NotEqual=1, LessThan=2, LessThanOrEqual=3, GreaterThan=4,
GreaterThanOrEqual=5 (`Compare`, NPCObject.cs:726-744, default false).
`CheckPage` sets failPage = check.FailPage before each check (NPCObject.cs:462);
a `continue` (missing parameter) counts as passing.

Check details (NPCObject.cs:457-724):
- PKPoints 5: `!Compare(op, PKPoint, Int1 == 0 ? Config.RedPoint : Int1) && Redemption == 0` -> fail.
- Horse 10: Compare(op, Account.Horse, Int1). Marriage 11: Equal needs Partner.
  WeddingRing 12: Equal needs Equipment[RingL] with UserItemFlags.Marriage.
- HasItem 4: GetItemCount counts inventory + companion inventory.
- CanGainItem 13: ItemCheck(Item1, Int1); CanGainItems(false, check).
- CanResetWeapon 14: Equal -> fail if Now < weapon.ResetCoolDown else fail if Now >= it.
- Random 15: Compare(op, Random.Next(Int1), Int2).
- Currency 17: CurrencyInfo by name (case-insensitive) else continue; Compare(op, amount, Int1).
- RollResult 18: NPCVals["ROLLRESULT"] must exist (else false); Compare(op, val, Int1).
- CheckDataList 19: category `{String1}_NameList`, key = GetDataTypeValue(Int1 as NPCDataType);
  fail if no GameNPCList row (Category, TypeValue).
- CheckDataValue 20: category String1; value 0 if missing; Compare(op, val, Int2).
- CheckFame 21: fail if no FP currency, no next fame title, or cost > amount.
- Script 22: Lua `ExecuteCheck(page.ScriptFile, String1, ...)`.
GetDataTypeValue (NPCObject.cs:746-768): None->"None", User->"User_{name}",
Guild->"Guild_{guild}" (null if none), Account->"Account_{email}".
NPCDataType: None=0, User=1, Guild=2, Account=3.

`NPCActionType` (NPCInfo.cs:942-980): Teleport=0, GiveGold=1, TakeGold=2,
GiveItem=3, TakeItem=4, ChangeElement=5, ChangeHorse=6, Message=7, Marriage=8,
Divorce=9, RemoveWeddingRing=10, ResetWeapon=11, GiveItemExperience=12,
SpecialRefine=13, Rebirth=14, GiveCurrency=15, TakeCurrency=16, AddDataList=17,
RemoveDataList=18, ClearDataList=19, ChangeDataValue=20, SetDataValue=21,
PromoteFame=22, Script=23. Fields: StringParameter1, IntParameter1/2,
ItemParameter1, MapParameter1, InstanceParameter1, StatParameter1.
`DoActions` (NPCObject.cs:73-347) runs all actions in order; a failing action
only skips itself.
- Teleport 0 (:79-120): needs Map1 or Instance1. Map path: `GetMap(Map1, current instance)`;
  Int1==0 && Int2==0 -> random location else Point(Int1, Int2).
- GiveGold 1: Gold += Int1 (milestone). TakeGold 2: Gold -= Int1, no floor at 0 (Rust saturates).
- GiveItem 3: ItemCheck(Item1, Int1); CanGainItems(false) all-or-nothing; GainItem stacks.
- TakeItem 4: inventory first then companion; removes even if fewer than Int1.
- ChangeElement 5 (:125-136): weapon.AddStat(WeaponElement, Int1 - current, Refine); RefreshStats.
- ChangeHorse 6: Account.Horse = Int1; RemoveMount; RefreshStats; Mount if != None.
- Message 7: no server case (dead).
- Marriage 8 / Divorce 9 / RemoveWeddingRing 10: MarriageRequest/MarriageLeave/MarriageRemoveRing.
- ResetWeapon 11: unequip weapon into RefineInfo{Chance 100, Precise, Type Reset, RetrieveTime}.
- GiveItemExperience 12: like GiveItem, item.Experience = Int2; level up when >= AccessoryExperienceList[level] (flag Refinable).
- SpecialRefine 13: weapon must be at max level (WeaponExperienceList.Count); AddStat(Stat1, Int1, Refine).
- Rebirth 14: only if Level >= 86 + Rebirth; Level=1, Exp/=200, Rebirth+1, SpentPoints=0, hermit stats cleared, discipline deleted.
- GiveCurrency 15 / TakeCurrency 16: by name, +=/-= Int1, CurrencyChanged.
- AddDataList 17 / RemoveDataList 18 / ClearDataList 19: GameNPCList rows, category `{String1}_NameList`.
- ChangeDataValue 20 (+= Int2) / SetDataValue 21 (= Int2): category String1, key by NPCDataType Int1.
- PromoteFame 22: next FameInfo by Order, needs FP >= Cost and bag space; grants ItemRewards, sets Fame, ApplyFameBuff.
- Script 23: Lua ExecuteAction.

Page extras: ScriptFile on-open hook (NPCObject.cs:30-39) returns a page description
("" closes; FindPage BFS over SuccessPage/FailPages/Buttons by Description).
`NPCValue` (NPCInfo.cs:796-888, NPCObject.cs:375-455): NPCResponse carries
Values {ID, Value}; NPCValueType None=0, DataList=1 ("True" if row exists),
DataValue=2 (IntValue1), Field=3 (NPCFieldType Name=1, GuildName=2,
FameCost=100), RollResult=4. `NPCRoll` sets ROLLRESULT = Random.Next(1,7) and
sends S.NPCRoll; `NPCRollResult` navigates to SuccessPage (DialogType RollDie/RollYut).

NPCGood: {Page, Item, Rate (default 1)}, Cost = round(Price * Rate). Buy sends the
good's Index; price = max(1, Cost * currency.ExchangeRate); currency =
NPCPage.Currency ?? Gold; bought items get Locked (+ NonRefinable for
wearables/books); CanGainItems(true). NPCType: item types the page buys back.
NPCButton {Page, ButtonID, DestinationPage}. NPCRequirement (visibility in
`CanBeSeenBy`, NPCObject.cs:784-838): MinLevel=0, MaxLevel=1, Accepted=2,
NotAccepted=3, HaveCompleted=4, HaveNotCompleted=5, Class=6, DaysOfWeek=7
(1 << DayOfWeek flag). NPCInfo has StartQuests/FinishQuests.
NPCDialogType: None, BuySell, Repair, Refine, RefineRetrieve, CompanionManage,
WeddingRing, RefinementStone, MasterRefine, WeaponReset, ItemFragment,
AccessoryRefineUpgrade, AccessoryRefineLevel, AccessoryReset, WeaponCraft,
AccessoryRefine, RollDie, RollYut.

## 2. Quests

Models (LibraryCore/SystemModels/QuestInfo.cs): QuestInfo {QuestName,
QuestType, AcceptText, ProgressText, CompletedText, ArchiveText,
Requirements, StartNPC, FinishNPC, Rewards, Tasks}; OnCreated adds a
HaveNotCompleted requirement on itself (non-repeatable by default).
QuestReward {Item, Amount, Choice, Bound, Duration, Class}. QuestRequirement
{Requirement, IntParameter1, QuestParameter, Class}. QuestTask {Task,
ItemParameter, RegionParameter, MobDescription, Amount, MonsterDetails}.
QuestTaskMonsterDetails {Monster, Map, Chance, Amount, DropSet}.
Enums: QuestType General=0, Daily=1, Weekly=2, Repeatable=3, Story=4,
Account=5; QuestRequirementType MinLevel=0, MaxLevel=1, NotAccepted=2,
HaveCompleted=3, HaveNotCompleted=4, Class=5; QuestTaskType KillMonster=0,
GainItem=1, Region=2.

User state (UserQuest.cs): UserQuest {QuestInfo, Completed, SelectedReward,
Track (default true), DateTaken, DateCompleted, Tasks}; IsComplete = all
tasks present and Amount >= Task.Amount. Player.Quests = character + account.

Accept (PlayerObject.Quests.cs:34-58): not dead, NPC open, quest in
NPC.StartQuests, QuestCanAccept (not already held; requirements as above);
Account quests attach to the account. Tasks are created lazily. Sends
S.QuestChanged{ClientUserQuest}.

Progress: MonsterObject.Drop (MonsterObject.cs:2391-2502): per open quest,
per task, first QuestTaskMonsterDetails with Monster match, Map null or match,
Random.Next(Chance)==0, DropSet subset; count = details.Amount. KillMonster:
Amount = min(Task.Amount, Amount + count). GainItem: spawns a quest item
(QuestItem flag, UserTask set, owner account) on the ground; credited on
pickup (Inventory.cs:148-177: item never enters the bag; remaining ground
objects despawn when the task completes). Region: SEnvir.CreateQuestRegions
puts tasks on cells; Cell.GetMovement (Map.cs:537-572) sets Amount = 1.

Complete (Quests.cs:109-197): quest in NPC.FinishQuests, !Completed &&
IsComplete; rewards filtered by Class flag; Choice rewards need
p.ChoiceIndex (else chat QuestSelectReward); bag space (QuestNeedSpace);
Bound/Expirable flags; then Track=false, Completed=true, DateCompleted.
Track: sets Track. Abandon: character quests only, S.QuestCancelled.
ProcessQuests (PlayerObject.cs:685-735) every 20 s: Daily removed when
completed on another UTC day, Weekly on another week, Repeatable as soon as
completed.
Packets: C.QuestAccept{Index}, C.QuestComplete{Index, ChoiceIndex},
C.QuestTrack{Index, Track}, C.QuestAbandon{Index}; S.QuestChanged{Quest},
S.QuestCancelled{Index}; ClientUserQuest {Index, QuestIndex, Track,
Completed, SelectedReward, DateTaken, DateCompleted, Tasks[{Index,
TaskIndex, Amount}]}; StartGame payload carries Quests.

## 3. Events (World / Player / Monster)

Models (LibraryCore/SystemModels/EventInfo.cs): WorldEventInfo
{Description, MaxValue, ResetWhenMax, Triggers, Actions}; WorldEventTrigger
{Type, Value, MaxTriggers, CronExpression}; PlayerEventInfo {Description,
TrackingType, MaxValue, ResetWhenMax, Triggers, Actions}; PlayerEventTrigger
{Type, Value, StringParameter1, MapParameter1, RegionParameter1,
InstanceParameter1, MaxTriggers}; MonsterEventInfo (same shape, triggers
{Type, Monster, DropSet, Map1, Region1, Instance1, Value, MaxTriggers},
attached via MonsterInfo.Events); BaseEventAction {Type, Restrict,
TriggerValue, StringParameter1, MonsterParameter1, RespawnParameter1,
MapParameter1, RegionParameter1, InstanceParameter1, ItemParameter1,
Stats -> CalculatedStats}.
Enums: EventTrackingType Global=0, Player=1, Group=2, Guild=3, Instance=10;
WorldEventTriggerType Dawn=0, Day=1, Dusk=2, Night=3, ScheduledTime=4;
PlayerEventTriggerType PlayerEnter=0, PlayerLeave=1, PlayerDie=2,
PlayerCommand=3, TimerMinute=10; MonsterEventTriggerType MonsterDie=0,
MonsterClear=1; EventActionType MonsterSpawn=0, MonsterPlayerSpawn=1,
MonsterBuffAdd=2, MonsterBuffRemove=3, PlayerMessage=10, PlayerTeleport=11,
PlayerEscape=12, PlayerBuffAdd=13, PlayerBuffRemove=14, TimerStart=20,
TimerStop=21, TimerReset=22, ItemDrop=30, ItemGive=31, FireEvent=40.
EventLog (in memory): Key, CurrentValue, per-trigger counts.

Dispatch (EventInfoHandler.cs:109-301): per trigger: skip if MaxTriggers > 0
and count >= MaxTriggers; Check; count++; start = CurrentValue,
end = min(MaxValue, max(0, start + trigger.Value)); fire actions with
start < TriggerValue <= end; ResetWhenMax && CurrentValue == MaxValue ->
Reset. Keys: Global "Global", Player "Player:{idx}", Group only with > 1
members, Guild, Instance. Monster events key off monster.EXPOwner (none ->
no event). FireWorldEvent runs all actions.

Triggers: TIMEOFDAY (SEnvir.TimeOfDay setter); SCHEDULEDTIME (per minute,
cron); TIMERMINUTE (per minute for started EventTimers by name+key);
PLAYERDIE (Combat.cs:1160, optional map/region/instance); PLAYERMOVEMAP
(OnMapChanged, Enter/Leave by MapParameter1); PLAYERMOVEREGION
(OnLocationChanged, Enter/Leave by RegionParameter1); PLAYERCOMMAND (@event
command); MONSTERDIE (MonsterObject.cs:2017; DropSet subset, map/region
filters); MONSTERCLEAR (when a spawn's AliveCount hits 0 and no alive
monsters remain on map/region).

Actions: MonsterSpawn (respawn entry DoSpawn, or MonsterParameter1 in
region/random map cell); MonsterPlayerSpawn (within 10 cells of each target
player); MonsterBuffAdd/Remove (BuffType by name, CalculatedStats, infinite);
PlayerMessage (system chat broadcast); PlayerTeleport (RegionParameter1);
PlayerEscape (bind point); PlayerBuffAdd/Remove; TimerStart/Stop/Reset;
ItemDrop (one item, stats as Enhancement, owner per tracking); ItemGive;
FireEvent (world event by Description, depth cap 10).

## 4. Guards, mines, fishing

GuardInfo {Map, Monster, X, Y, Direction}: Map.CreateGuards at map load
spawns the monster facing Direction. Guard AI (Monsters/Guard.cs): blocking,
immobile, invulnerable, sky-blue name, attacks anything in view range:
players with PKPoint >= Config.RedPoint and Redemption == 0, wild non-passive
monsters (or pets of red players); 300 ms delayed hit; monsters take
CurrentHP (no exp), players take DC.

MineInfo {Map, Item, Chance, Region, Quantity, RestockTimeInMinutes}: mining
(Movement.cs:915-960) needs map CanMine, a non-walkable cell in front, a
PickAxe weapon (damaged 4 per swing); each MineInfo rolls Next(Chance)==0,
region contains the cell, remaining quantity; grants one Bound item;
restock after RestockTimeInMinutes (< 0 never).

FishingInfo {Name, Region, Drops[{Item, Chance, ThrowQuality,
PerfectCatch}]}: fishing needs FishingRod weapon + FishingRobe armour, a
valid zone/distance; on FishPointsCurrent >= Config.FishPointsRequired, drops
by descending Chance: PerfectCatch needs a perfect reel, ThrowQuality match,
Next(Chance)==0, first hit grants one Bound item.
