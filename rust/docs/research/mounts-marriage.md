# Research: mounts (horses) and marriage (from the C# server)

Extracted 2026-09-13 by direct reading. Sources: LibraryCore/Enum.cs
(`HorseType` 1788, `ItemType.HorseArmour = 18` 411, `EquipmentSlot.
HorseArmour = 13` 104), LibraryCore/Libraries.cs (`LibraryFile` Horse 71,
HorseS 72, HorseIron 73, HorseSilver 74, HorseGold 75, HorseBlue 76,
HorseDark 77 + Effect 78, HorseRoyal 79 + Effect 80, HorseBlueDragon 81 +
Effect 82), LibraryCore/FrameSet.cs 97-100, LibraryCore/Network
(`C.Mount` ClientPackets 105, `S.ObjectMount` ServerPackets 111,
`S.MountFailed` 910, `C.MarriageResponse/MakeRing/Teleport` 632-645,
`S.MarriageInvite/Info/RemoveRing/MakeRing/OnlineChanged` 1182-1200),
ServerLibrary/Models/PlayerObject.Movement.cs (`Mount` 211-262, `Move`
517-600), PlayerObject.cs (`CanAttack`/`CanCast` 132-133, `RemoveMount`
1478-1484, map change 1373), PlayerObject.Stats.cs 159-219,
PlayerObject.Combat.cs (`Pushed` 1096, `Die` 1134), PlayerObject.Social.cs
24-290 (marriage region), NPCObject.cs (`ChangeHorse` 137-142,
`Marriage`/`Divorce`/`RemoveWeddingRing` 155-163, checks `Horse` 600,
`Marriage` 603, `WeddingRing` 613), LibraryCore/SystemModels/NPCInfo.cs
(`NPCCheckType` Horse 10, Marriage 11, WeddingRing 12; `NPCActionType`
ChangeHorse 6, Marriage 8, Divorce 9, RemoveWeddingRing 10),
Client/Models/PlayerObject.cs (horse libraries 345-380, animation choice
583-612, `ArmourShift` 764-780, draw 985-1015), Client/Scenes/Views/
MapControl.cs 1090 (mounted run), Client/Envir/CEnvir.cs 703 (M key).

## Horses

- `Account.Horse: HorseType` (None, Brown, White, Red, Black, WhiteUnicorn,
  RedUnicorn) is set only by the NPC action `ChangeHorse { IntParameter1 }`
  (NPCObject.cs 138), which also dismounts and refreshes stats. There is
  no horse item; `HorseArmour` (ItemType 18, slot 13) is equipment whose
  `Shape` picks the drawn horse skin and whose stats count only with a
  horse (Stats.cs 219).
- Stats.cs 159-198: owning a horse always adds BagWeight 50/100/150/200/
  250 and (from White) Comfort 2/5/7/9 plus MaxAC/MR/DC/MC/SC 5/12/25/30.
- `Mount()` (Movement.cs 211): after `ActionTime`; refused when dead
  ("HorseDead"), without a horse ("HorseOwner") or on a map without
  `MapInfo.CanHorse` ("HorseMap"), each answered with `S.MountFailed`.
  Toggles `Horse`, sets `ActionTime = now + TurnTime`, removes Cloak and
  Transparency, broadcasts `S.ObjectMount { ObjectID, Horse }`.
- While mounted: `CanAttack` and `CanCast` are false (PlayerObject.cs
  132-133), `MagicToggle` returns (Movement.cs 853), item use is blocked
  for consumables (Inventory.cs 1071) and several move/equip paths
  (807-999), fishing/mining/harvest need `Horse == None`. Movement: the
  client adds a step to a run when mounted (MapControl.cs 1090) and the
  server allows `distance == 3` only on a horse (Movement.cs 543).
- `RemoveMount()` runs on `Pushed` (Combat.cs 1096), `Die` (1134), on
  entering a map without `CanHorse` (PlayerObject.cs 1373) and on
  `ChangeHorse`.
- Client: animations HorseStanding/Walking/Running/Struck use frame sets
  2240x4 (500 ms), 2320x6, 2400x6, 2480x3 (100 ms) with 10 frames per
  direction; `ArmourShift = 80` for those animations shifts the armour,
  helmet and shield frames; hair and weapon use the plain frame. The
  horse is drawn first: `HorseFrame = DrawFrame + (Horse - 1) * 5000` in
  Horse.Zl (shape 0) or HorseIron/Silver/Gold (shapes 1-3); HorseBlue (4)
  and HorseDark/HorseRoyal (5, 6, with effect libraries) use `DrawFrame`.
  Default keybind MountToggle = M.

## Marriage

- `MarriageRequest()` (Social.cs 26, from the NPC `Marriage` action):
  refused when married, below level 22, under 500,000 gold, without a
  facing player in the cell in front, when the target is married, already
  proposed to, below 22, under 500,000 gold, or either is dead. Sets
  `player.MarriageInvitation` and sends `S.MarriageInvite { Name }`.
- `MarriageJoin()` (103): both must still be single and hold 500,000 gold;
  `Character.Partner` is set both ways, 500,000 taken from each, both get
  `S.MarriageInfo { Partner }`.
- `MarriageLeave()` (144, NPC `Divorce`): clears both partners (the
  offline partner's `CharacterInfo` directly), removes the wedding ring
  flags, sends `MarriageInfo` to whoever is online.
- `MarriageMakeRing(index)` (169, wedding-ring NPC dialog type 6): married,
  no wedding ring on RingL, the bag item is a wearable ring; it moves to
  RingL with `UserItemFlags.Marriage`. `MarriageRemoveRing` (NPC action)
  clears the flag.
- `MarriageTeleport()` (189, `C.MarriageTeleport` from the ring cell or
  keybind): needs a partner, the flagged ring on RingL, alive, PK points
  under `Config.RedPoint`, `Character.MarriageTeleportTime` passed, partner
  online and alive on a map with `CanMarriageRecall` (and instance/`AllowTT`
  rules); teleports to `GetRandomLocation(partner, 10)` and sets the next
  time to now + 120 s.
- Login sends `MarriageInfo` and `S.MarriageOnlineChanged` to the partner.

## Prototype

Mounts: `horse` per character (Zircon: per account), NPC `ChangeHorse`,
`ClientMessage::Mount` (key R; M is mail here), stats as above (Comfort
skipped), riding gates attack/cast/toggle/item use, run of three cells,
dismount on push/death/no-horse map, `Appearance::Player { horse,
horse_shape }` with horse layer and +80 armour shift on the client.
Marriage: NPC actions 8/9/10 and checks 11/12, proposal prompt, wedding
ring recorded as the ring's item id (no per-item flags yet), teleport with
the rules above (instances ignored), divorce of an offline partner applied
to the account record through `World::take_divorced`. Not done:
`MarriageOnlineChanged`, HorseS/BlueDragon skins (no files in the pack).
