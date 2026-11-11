# Research: monster AI classes (from the C# reference)

Extracted 2026-09-12. Wave 1 of the port landed the same day in
`crates/mir-server/src/world/ai_profile.rs` + `monster_ai.rs` (base rules,
ranged/line/splash/self-area shapes, casters, poisons, blink-strikers,
hidden monsters, stages, guards, larva, corpse spawns). Wave two (2026-09-13)
added VoraciousGhost revives, the MagicWeakness debuff (a poison kind that
zeroes MR), JinchonDevil DeathCloud fields and the two gates (regions from
`ZIRCON_MYSTERY_SHIP_REGION` / `ZIRCON_LAIR_REGION`). Wave three added Shinsu
mode windows, Terracotta phasing and DoomClaw's parts (the class
percentages are damage dealt to players, not taken). Wave four added
the persistent FireWall/PoisonousCloud monster fields (the cloud's ally
buff is not modelled since monsters carry no buffs), Purification for
BanyoCaptain/BanyoLordGuzak, HealerAnt ally healing (a tenth of the ally's
health over ten seconds, since the pack's ant carries no Healing stats),
FrostLordHwa's scatter curse (the weekday affinity element is not
applied) and QueenOfDawn's map-wide scatter, and the Behaviours flags
(Heals/Teleports/Enrages; the pack's MonsterInfo has no such column, so
they only fire when a definition sets them). Still open: castle classes.
File:line references are into the C# projects
(ServerLibrary/Models/MonsterObject.cs unless a class file is named;
class files live in ServerLibrary/Models/Monsters/).

## Selection

`MonsterInfo.AI` (int) selects the class via `MonsterRegistry.Create`
(MonsterRegistry.cs:30-33); unknown ids fall back to plain `MonsterObject`.
`MonsterInfo.Flag` is only a lookup key (Skeleton 1, JinSkeleton 2, Shinsu 3,
InfernalSoldier 4, CursedDoll 5, SummonPuppet 6, MirrorImage 7, Tornado 8,
UndeadSoul 9, CastleObjective 10, CastleDefense 11, Blocker 20, Larva 100,
LesserWedgeMoth 110, Zuma* 120-123, Bone* 130-134, MatureEarwig 140,
GoldenArmouredBeetle 141, Millipede 142, FerociousFlameDemon 150, FlameDemon
151, Goru* 160-162, DragonLord 170 .. YumgonGeneral 178, BanyoCaptain 180,
SamaSorcerer 190, BloodStone 191, Quartz* 200-205, Sacrifice 210).

AI -> class (MonsterRegistrations.cs): -1 Guard; 1 base Passive harvest 2;
2 base Passive harvest 3; 3 base harvest 3; 4 TreeMonster; 5 CarnivorousPlant;
6 SpittingSpider Green; 7 SkeletonAxeThrower; 8 base Paralysis(1 tick, 5 s,
rate 12); 9 GhostSorcerer; 10 GhostMage; 11 VoraciousGhost; 12 HealerAnt;
13 LordNiJae; 14 SpittingSpider Green; 15 base; 16 UmaKing; 17 ArachnidGrazer
(spawns Larva); 18 Larva Green; 19 RedMoonTheFallen; 20 SkeletonAxeThrower
FearRate 2 FearDuration 4; 21 ZumaGuardian; 22 ZumaKing; 23 Monkey Green;
24 Monkey Red; 25 EvilElephant; 26 NumaMage; 27 GhostMage; 28
WindfurySorcerer; 29 SkeletonAxeThrower; 30 NetherworldGate; 31 SonicLizard;
33 GiantLizard range 9 IgnoreShield; 34 SkeletonAxeThrower range 9; 35/37/39/40
base; 36 NumaMage; 38 BanyaLeftGuard; 41 EmperorSaWoo; 42 SpittingSpider; 43
ArchLichTaedu; 44 WedgeMothLarva; 45 RazorTusk; 46 SpittingSpider Red(1,10,25);
47 SpittingSpider Green(7 ticks, rate 15); 48 SonicLizard IgnoreShield; 49
GiantLizard range 8 Paralysis; 50 GiantLizard range 8; 52 WhiteBone; 53
Shinsu; 54 GiantLizard cooldown 5 s; 56 CorrosivePoisonSpitter Green(7,15)
IgnoreShield; 57 CorrosivePoisonSpitter; 58 Stomper; 59 CrimsonNecromancer;
60 ChaosKnight; 61 PachontheChaosbringer; 62 NumaHighMage; 63
NumaStoneThrower; 64 Monkey; 65 IcyGoddess FindRange 3; 66 IcySpiritWarrior
Paralysis(1,5,25); 67 IcySpiritGeneral IgnoreShield; 68 Warewolf
IgnoreShield; 69 JinamStoneGate; 70 FrostLordHwa; 71 BanyoWarrior; 72
BanyoCaptain; 74 BanyoLordGuzak; 75/76 DepartedMonster (MatureEarwig /
GoldenArmouredBeetle); 77 EnragedLordNiJae; 78 JinchonDevil; 79 GiantLizard
range 10 cd 5 s; 80 SunFeralWarrior; 81 MoonFeralWarrior; 82 OxFeralGeneral;
83 FlameDemon Min -2 Max 2; 84 WingedHorror RangeChance 1; 85 EmperorSaWoo
Paralysis(1,5,8); 86 FlameDemon Passive Min 0 Max 8; 87 OmaWarlord Abyss
(1,7,15); 88 GoruSpearman; 89 GoruArcher Silenced(1,5,10); 90 OmaWarlord
Paralysis(1,5,25); 91 EnragedArchLichTaedu; 92 GiantLizard range 9; 93
EscortCommander; 94 FieryDancer; 95 FieryDancer Paralysis(1,5,15); 96
QueenOfDawn; 97 SonicLizard IgnoreShield Range 5; 98 YumgonWitch Lightning;
99 JinhwanSpirit; 100 YumgonWitch; 101 DragonQueen; 102 DragonLord; 103
InfernalSoldier range 5; 104 FerociousIceTiger; 105 GiantLizard range 5
IgnoreShield CanPvPRange; 106 GiantLizard range 7 CanPvPRange; 107-110 Sama
Fire/Ice/Lightning/Wind Guardian; 111 SamaPhoenix; 112-114
SamaBlack/Blue/White; 115 SamaProphet; 116 SamaScorcer; 117 BanyoWarrior
DoubleDamage; 118 OmaMage; 119 base Silenced(1,5,10); 120 DoomClaw; 121
PinkBat; 122 QuartzTurtleSub; 123 Larva Range 3; 124 QuartzTree; 125
CarnivorousPlant Hide/Find 1; 126 MonasteryBoss; 127 JinchonDevil CastDelay
8 s; 128 Doll; 129 Tornado Passive; 130 UndeadSoul; 131 Terracotta; 132
Terracotta CanPhase; 133 TerracottaSub Paralysis(1,5,15); 134 TerracottaBoss
Paralysis(1,5,15); 1001-1003 CastleFlag/Gate/Guard. Unregistered: 32, 51,
55, 73. Poison tuple = (ticks, frequency s, rate: applied when
Random(rate)==0). Defaults PoisonRate 10, PoisonTicks 5, PoisonFrequency 2.

## Base MonsterObject rules the Rust port lacks

- Tick order (:449): pet block, ProcessRegen, ProcessBehaviours, ProcessSearch,
  ProcessRoam, ProcessTarget.
- Target drop (:427-435): null/dead/other map/> MaxViewRange 18/Abyss-poisoned
  and out of ViewRange/!CanAttackTarget/Cloak > 2 cells (unless IgnoreStealth)/
  Transparency.
- ViewRange = Abyss ? 2 : info.ViewRange (:101-104).
- CanMove: not Silenced, MoveDelay > 0, pet mode; base excludes Paralysis,
  WraithGrip, Containment, Binding, DragonRepulse; needs now >= ActionTime,
  MoveTime, > ShockTime. CanAttack: not Silenced/Fear/DragonRepulse, AttackDelay > 0.
- MoveDelay/AttackDelay are runtime fields (Enrage: AttackDelay *= 0.6 for
  10 min RageTime; RageTime makes it attack other wild monsters).
- Behaviours flags (MonsterInfo.Behaviours: HasPoison 1, Summons 2, Heals 4,
  Teleports 8, AOEAttack 16, RangeAttack 32, Enrages 64): Heals every 30 s
  max(1, Health*0.10); Teleports once at HP <= 1/4 -> TeleportNearby(5,15);
  Enrages once at HP <= 1/4.
- Regen: RegenDelay, skipped under Hemorrhage, max(1, Health*0.02).
- ProcessSearch (:583-644): with a target and (CanMove||CanAttack) it
  re-searches EVERY tick (nearest player or player pet on the map passing
  ShouldAttackTarget; ties random). ProperSearch (Guard/HealerAnt): expanding
  rings 0..ViewRange, may target monsters.
- ProcessRoam (:694-732): needs SeenByPlayers; if a blocking object shares the
  cell, walk in a random direction; else Target != null || Random(10) > 0 ->
  return; Random(3) > 0 ? Walk(Direction) : Turn(random).
- ProcessTarget (:733-766): Fear -> walk away; not in range -> MoveTo
  (straight line then rotate +-1 .. all 8); if CanAttack -> Attack().
- Attack: face, broadcast, UpdateAttackTime, delayed 400 ms hit GetDC()
  with AttackElement = affinity element of the stats.
- Damage (:1194-1264): Abyss on attacker -> 50 % miss; element None: dodge
  if Random(Agility) > Accuracy, damage = power - AC; else damage = power - MR
  (no dodge); res > 0: -= damage*res/10, res < 0: -= damage*res/5; <= 0 ->
  Blocked; poison roll Random(PoisonRate)==0 -> Poison{Value=GetSC(),
  TickFrequency, TickCount}.
- UpdateAttackTime: AttackTime = now + AttackDelay; ActionTime = now +
  min(MoveDelay, AttackDelay-100); + Slow.Value*100 ms; Neutralize doubles.
  UpdateMoveTime: MoveTime = now + MoveDelay; ActionTime = now +
  min(MoveDelay-100, AttackDelay).
- Attacked (:1892-1981): Invincibility -> 0; clears ShockTime; EXPOwner +
  20 s; Red poison power *= 1.2; MagicShield; crit Random(100) <
  CriticalChance -> power += power + power*CriticalDamage/100; retaliation:
  if CanAttackTarget(attacker) && (PetOwner == null || Target == null) Target = attacker.
- ShouldAttackTarget (:980-1178): Passive -> false; Invisibility blocks unless
  CoolEye; Cloak blocks unless CoolEye && within 2 && ob.Level < Level;
  Transparency blocks; ClearRing players ignored; wild-vs-wild only under
  RageTime; wild always attacks pets; pet matrix.
- CoolEye = Random(100) < info.CoolEye at spawn. Spawn: ActionTime = now + 1 s;
  SearchDelay 3 s, RoamDelay 2 s.
- Activate/DeActivate: only ticks with players near (or boss/pet/queued).
- SummonLevel/GrowthLevel: +level/10 on stats; CriticalChance = 1 always;
  MagicWeakness buff zeroes MR.
- Walk honours Chain poison, AvoidFireWall (refuses hostile FireWall/Tempest
  cells), removes Invisibility/Transparency.
- SpawnMinions(fixed, random, target): count = min(MaxMinions(20) -
  minions, Random(random+1)+fixed), spawn at GetRandomLocation(loc, 6).
- Shared spells: AttackMagic (500 + dist*48 ms), AttackAoE(radius) 500 ms,
  LineAoE(distance, min, max) 500 + i*75 ms with half damage on flanks,
  FireWall (5 cells, 15 ticks x 2 s), DeathCloud (radius 2, 4000 +
  Random(rnd) ms), MassLightningBall (all in MaxViewRange, 500 + dist*48),
  MassThunderBolt (50 % of all), MassCyclone (75 % each), MonsterThunderStorm
  (radius 2 around self), PoisonousCloud (5x5, Power 20, 20 s),
  Purification, DragonRepulse (6 s buff, 500 ms ticks, damage + push 1),
  SamaGuardianFire (radius 5). Globals: MagicRange 10, ProjectileSpeed 48,
  MaxViewRange 18, DeadDuration 1 min, HarvestDuration 5 min.
- Rust MonsterDef lacks `Behaviours`; is_passive() hardcodes ai 1/2/4 and
  misses AI 86 and 129 (passive by factory).

## Class behaviours (most used first)

- GiantLizard (33,49,50,54,79,92,105,106): AttackRange 7 default; after
  RangeCooldown (RangeTime) may fire at range (players only unless
  CanPvPRange), otherwise melee range 1; attack/move/attack per tick; ranged
  = ObjectRangeAttack projectile 400 ms GetDC() AttackElement.
- SpittingSpider (6,14,42,46,47): InAttackRange = rays within 2 (dx<=2,
  dy<=2, dx==0||dy==0||dx==dy); LineAttack(2): first object per cell along
  Direction, GetDC() at 400 ms. Base of JinchonDevil, GoruSpearman, Shinsu.
- SkeletonAxeThrower (7,20,29,34): AttackRange 7, FearRate 6, FearDuration 2;
  in range but within AttackRange-1 -> walk AWAY; attack only if now >=
  FearTime; ranged GetDC() at 400 + dist*48; after shot Random(FearRate)==0
  -> FearTime = now + (FearDuration + Random(4)) s. Base of HealerAnt,
  NumaStoneThrower, OmaMage, GoruArcher.
- Monkey (23,24,64): radius 2; 50 % attack before move, then attack; melee or
  projectile, same GetDC() 400 ms.
- SonicLizard (31,48,97): Range 3 (5 for 97) rays; InRange(1) && Random(4)>0
  melee else LineAttack(Range).
- GhostMage (10,27): invisible until a target within 5 (checked every 3 s);
  spawns ZombieHole spell (1 min) on reveal; SpectralHit random element.
- NumaMage (26,36): out of range 50 % RangeAttack then move; in range
  Random(5)>0 melee else RangeAttack = AttackMagic(ThunderBolt, Lightning) in
  MagicRange, 500 ms GetDC().
- CarnivorousPlant (5,125): hidden until target within FindRange 5 (checked
  3 s); hides again beyond HideRange 5: full heal, poisons cleared.
- Larva (18,123): dies with no target; Attack = SetHP(0); Die hits all within
  Range (1 / 3) after 800 ms.
- EmperorSaWoo (41,85): 1/3 range when far, 20 % in range; RangeAttack:
  Random(3)==0 MassCyclone else splash radius 2 around target 400 ms.
- CorrosivePoisonSpitter (56,57): if target > 3 away and cooldown, blink
  adjacent (TeleportTime + 5 s), then melee.
- DepartedMonster (75,76): Die: random element, hits all within 2 for GetDC()
  400 ms, then for each surviving player SpawnMinions(4, 0) on own cell.
- FlameDemon (83,86): if (Cast <= 0 || now > CastTime) && InRange(8): Cast=3,
  CastTime + 5 s, LineAoE(12, Min, Max, ScorchedEarth, Fire); Attack
  decrements Cast. 86 is passive with Min 0 Max 8 (all directions).
- OmaWarlord (87,90) blink-striker template: Attacked once at HP <= 1/2 ->
  TeleportNearby(7,12); when not adjacent every 1 s 1-in-7 blink beside
  target with Bonus; Attack damage *2 when Bonus. Same in GoruArcher,
  GoruSpearman, EnragedArchLichTaedu, MoonFeralWarrior, BanyoWarrior
  (DoubleDamage only), QueenOfDawn (1-in-10).
- FieryDancer (94,95): radius 10; 50 % attack/move/attack; Random(5)>0 single
  else splash radius 2 around target.
- YumgonWitch (98,100): radius 10; Random(5)>0 || !InRange(3) single else
  self-centred AoE radius 3 in AoEElement.
- BanyoWarrior (71,117): blink-striker, x2 only with DoubleDamage.
- JinchonDevil (78,127): SpittingSpider rays within 3; every CastDelay (15 /
  8 s) DeathCloud on each target in ViewRange (50 % skip above half HP);
  Attack: Random(3)==0 || !InRange(2) LineAttack(3) else frontal melee.
- Terracotta (131,132): starts invisible; CanPhase: Random(45)==0 phase out
  when > 4 away; phase in within 2; while invisible moves every 20 ms.
- Guard (-1): blocking, immobile, invulnerable, ProperSearch, attacks in
  ViewRange: red players (PKPoint >= 200, Redemption 0), wild non-passive
  monsters, red players' pets; 300 ms hit; monsters take CurrentHP (no exp).
- TreeMonster (4): immobile passive, every hit clamped to 1, no poison.
- GhostSorcerer (9): rays within 6, LineAttack(6).
- VoraciousGhost (11): ReviveCount = Random(4); revives after 3-8 s with
  Health/2^deaths; no drops until ReviveCount 0; exp halved per revive.
- HealerAnt (12): heals injured allies (ranged Heal buff ticking 1 s).
- LordNiJae (13): burrower; hits every target in radius 10 (500 + dist*48);
  on damage 1/5 Green (SC, 2 s x 10) and 1/10 Paralysis 5 s.
- UmaKing (16): 20 % range: Random(3)==0 MassLightningBall else MassThunderBolt.
- ArachnidGrazer (17): immobile, spawns 1 Larva per "attack" (600 ms).
- RedMoonTheFallen (19): immobile, hits every target within 40 cells 500 ms.
- ZumaGuardian (21): dormant statue (invulnerable, no poison) until target
  within 3; wakes all others within 7 with same target.
- ZumaKing (22): 7 HP stages each SpawnMinions(4, 8); 1/5 range: Random(3)==0
  FireWall else LineAoE(12,-2,2, ScorchedEarth, Fire).
- EvilElephant (25): Random(6)==0 stomp all within 1 + Green poison SC 2 s x 10.
- WindfurySorcerer (28): melee shown as range attack.
- NetherworldGate (30) / JinamStoneGate (69): 20 min gates teleporting
  players (and monsters for 30) in ViewRange every 3 s to a special region.
- BanyaLeftGuard (38): 50 % / 20 % AttackMagic(FireBall, Fire, travel).
- ArchLichTaedu (43): 7 HP stages SpawnMinions(20, 5) (MaxMinions 50);
  50 % / 20 % projectile 400 ms.
- WedgeMothLarva (44): immobile spawner of LesserWedgeMoth.
- RazorTusk (45): radius 3; Random(3)>0 attack/move/attack; melee or projectile.
- WhiteBone (52) / UndeadSoul (130): 1 s spawn animation then base melee.
- Shinsu (53): fights only in 10 s Mode windows (2 s transitions).
- Stomper (58): melee hits every target within 1.
- CrimsonNecromancer (59): every 10 s MagicWeakness 10 s on targets within 3.
- ChaosKnight (60): adjacent && 20 s cooldown -> PoisonousCloud 5x5.
- PachontheChaosbringer (61): radius 2; > 8 away blink (10 s); in range
  20 s cooldown DragonRepulse.
- NumaHighMage (62): rays within 6, LineAttack(6) after projectile.
- NumaStoneThrower (63): kiting archer with splash radius 2 boulder.
- IcyGoddess (65): burrower with range 8 projectile (400 + dist*48).
- IcySpiritWarrior (66) / General (67): range 8; 50/50 bolt vs
  AttackAoE(1, IceStorm); General bolt is Dark and drains MP by damage.
- Warewolf (68): every 10 s within 8: LineAoE(8,-2,2, GreaterFrozenEarth, Ice).
- FrostLordHwa (70): affinity by weekday; every 60 s teleports targets and
  stacks Abyss/Silenced/Red 10 s + WraithGrip 5 s; blink > 3 away (5 s);
  kills monster targets outright.
- BanyoCaptain (72): every 3 s 1/5 Purification else ThunderBolt fixed
  100/150/200 by distance; melee 1/10 x2.
- BanyoLordGuzak (74): Pachon + every 20 s purify all in view + every 45 s
  cull far minions and refill 5 BanyoCaptains.
- EnragedLordNiJae (77): every 30 s refill 5 Millipedes (MaxMinions 200).
- SunFeralWarrior (80): first damage spawns 2-8 flame demons; 50/50 FireBall
  / AttackAoE(1, FireStorm).
- MoonFeralWarrior (81): NumaMage + blink-striker.
- OxFeralGeneral (82): Cast cycle: Random(4) LineAoE(12,-2,2) of
  GreaterFrozenEarth/ScorchedEarth/LightningBeam/BlowEarth.
- WingedHorror (84): never melees; Random(3) MassLightningBall /
  LineAoE(10, 0, 8, LightningBeam) / MassThunderBolt.
- GoruSpearman (88), GoruArcher (89), EnragedArchLichTaedu (91): blink-striker
  variants (91 has 7 stages SpawnMinions(5,5) and Red poison).
- EscortCommander (93): 50/50; within 2 && Random(2)==0 MonsterThunderStorm
  else ThunderBolt.
- QueenOfDawn (96): every 60 s teleports every attackable object on the map;
  1/10 blink; Random(5)>0 single else splash radius 2.
- JinhwanSpirit (99): 1 % on death SpawnMinions(20, 10) of itself.
- DragonQueen (101): every 15 s refill 10 adds; Die spawns DragonLord.
- DragonLord (102): radius 12; every 15 s refill 10; Attack: each target 4/10
  chance, splash radius 2, 1000 ms.
- InfernalSoldier (103): GiantLizard shape + 2 s spawn animation.
- FerociousIceTiger (104): box 3; Random(5)==0 || Spinning -> spin hits all
  within 3 (Warriors -20 %), keeps spinning until it moves; else frontal
  cleave 3 ahead radius 3 at GetDC()/2, 600 ms.
- Sama guardians (107-110): radius 2, 20 % cast, Random(4) among
  bolt / LineAoE(10,-2,2) / AttackAoE(2) / AttackAoE(3) per element; Sama
  bosses (111-114) add a 5th move AttackAoE(3, signature, GetDC()*2).
- SamaProphet (115): radius 12; invulnerable while any BloodStone or
  SamaSorcerer alive; yanks far targets; Random(3) AttackAoE(15, ...).
- SamaScorcer (116): radius 3; invulnerable while BloodStone alive; yanks.
- OmaMage (118): kiting; Random(3) LightningBall 2/3 DC / ThunderBolt DC /
  AttackAoE(1, LightningWave) 2/3 DC.
- DoomClaw (120): stationary multi-part boss; class mitigation Warrior -40 %,
  Wizard -30 %, Taoist -60 %, Assassin -20 %; Wave pushes 15 cells.
- PinkBat (121): PinkFireBall Phantom. MonasteryBoss (126): GreenSludgeBall
  Wind, Die spawns Sacrifice.
- QuartzTurtleSub (122): every 20 s refill 10 mini turtles. QuartzTree (124):
  tree that spawns sub boss at HP < 1/4, refills adds every 60 s, no regen.
- Doll (128): forwards all damage to DollTarget.
- Tornado (129): drifts every second, dies at VisibleTime, hits all within 1
  after 100 ms, hostile to everything.
- TerracottaSub (133): 50/50 sweep vs cone. TerracottaBoss (134): range 11
  dark beam LineAoE(12,1,1) or frontal cleave at GetMC().
- Castle classes (1001-1003): conquest only.
- Non-registry: Puppet (matches Rust puppet_explode), MirrorImage (15 s decoy
  casting AoE by element with owner's MC), Halloween/Christmas event monsters.
