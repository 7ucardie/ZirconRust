# Research: sound (from the C# client)

Extracted 2026-09-12. Sources: Client/Envir/DXSoundManager.cs (index ->
file table, 724 entries), Client/Envir/DXSound.cs, LibraryCore/Enum.cs
`enum SoundIndex` (values 0..814; `B000 = 3`, `ButtonA = 100`, rest
sequential), Client/Models/*.cs for triggers. The Rust table is generated
by `rust/tools/gen_sound_table.py` into `mir-client/src/sound_table.rs`.

## Engine

- `SoundIndex -> filename` is a dictionary; most files are named
  (`M7-1.wav`, `220-2.wav`, `as_165.wav`, `B000.wav`), only a few are
  numeric. Missing file -> silent. Lookup must be case-insensitive
  (`m140-1.wav` on disk vs `M140-1.wav` in code).
- Channels (`SoundType`): System, Music, Magic, Monster, Player; each a
  0..100 volume (default 25) -> linear gain volume/100.
- Per-index overlap cap 5 (reuse a finished voice first). Looping clips
  are singletons: Play on a playing loop is a no-op; `Stop(index)` exists.
- Music = looping SoundIndex from `MapInfo.Music`; on map change
  `Stop(old); Play(new)` only when the value differs. 21 of 43 music
  indices have no file (silent maps).
- Login: `LoginScene2` (Main.wav, missing in this pack; `LoginScene` =
  Opening.wav exists), select screen `SelectScene` (SelChr.wav), enter
  world stops all.

## Triggers

- Footsteps: local player only, animation frames 1 and 4 of walk and run:
  `Foot1 + Random(3) + 1` = Foot2..Foot4 (2.wav..4.wav); Foot1 never plays.
- Player attack: by `LibraryWeaponShape`: 100 Wand 56.wav, 9/101 Wood
  51.wav, 102 Axe 54.wav, 103 Dagger 50.wav, 104 ShortSword 53.wav,
  26/105 IronSword 52.wav, else Fist 57.wav; assassin >= 1200 Claw 64.wav,
  >= 1100 Glaive 63.wav. Attack-skill sounds: Slaying (male/female),
  Thrusting EnergyBlast, HalfMoon, DestructiveSurge, FlamingSword,
  DragonRise, BladeStorm, DefensiveBlow, FlameSplash (BladeStorm),
  WaningMoon, CalamityOfFullMoon.
- Player struck: Male/FemaleStruck (138/139) + GenericStruckPlayer (61);
  die Male/FemaleDie (144/145).
- Monsters: per `MonsterImage` attack/struck/die (`<base>-2/-4/-5.wav`,
  base = legacy monster id, table generated); every monster struck also
  plays GenericStruckMonster (61.wav). Appear sounds for StoneGolem,
  ZumaKing, Shinsu.
- Magic: cast sound at cast, travel sound with the projectile, end sound
  at impact (tables generated from MapObject.SetAction).
- ObjectEffect: TeleportOut 110.wav, TeleportIn 109.wav, ThunderBolt ->
  LightningStrikeEnd, lotus effects, FlashOfLightEnd, ParasiteExplode,
  FrostBiteEnd -> FireStormEnd, ChainOfFireExplode, MirrorImage ->
  SummonSkeletonEnd. MapEffect: SummonSkeleton/Shinsu/CursedDoll/
  UndeadSoul ends, BurningFireExplode -> FireStormEnd, HundredFist,
  IceAuraEnd -> GreaterIceBoltEnd.
- Spell objects: FireWall loops FireWallDuration (M22-3), Tempest
  TempestDuration (M114-3), stopped when none of that effect is within 20
  tiles of the user; PoisonousCloud start, DarkSoulPrison, Rubble
  MiningStruck.
- UI: every button ButtonA (103.wav), NPC dialog links ButtonC (105.wav);
  item cell pick/put/use by ItemType (weapon 111, armour 112, helmet 116,
  necklace 115, bracelet 114, ring 113, shoes 117, potion (consumable
  shape 0) 108, else 118); gold gained 122; quest take Qtake.wav, quest
  complete Qcomp.wav. No NPC-open or level-up sound.
- Missing in this asset pack (21): 125/126 mining, SkyStinger, Yob,
  Main/Ending, dice/yut, ElementalSwords, Tornado, SummonDead,
  ChainOfFire, M123-3-1 (FlashOfLight), two undersea musics.
