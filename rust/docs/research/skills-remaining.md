# Research: skills not yet in the Rust server (from the C# reference)

Extracted 2026-09-12. Wave four (commit after d06b2d4) implemented the
passives, self bursts, single-target spells, cell areas and self buffs
listed in the README; wave five added the charges, lines, bounces,
fields, summons, kicks, dashes, channels and augments. Wave six (2026-09-13,
`world/magic_wave6.rs`) closed the list: Shuriken, Elemental Hurricane,
Mirror Image, Frost Bite, Tornado, Burning/Shocked, Cursed Doll, Soul
Resonance, Augment Poison Dust, Infection, Corpse Exploder, Summon Dead
(corpses are clickable), Dragon Blood, Magic Combustion, Chain, Chain Of
Fire.

Parity corrections (2026-09-22): Elemental Hurricane uses the equipped Dark
Stone's strongest affinity, has no fixed timeout, and follows the reference
start, tick and stop mana rules. Mirror Image requires an equipped stone with
Fire/Ice/Lightning/Wind affinity, consumes no stone, and allows one decoy.
The stationary, invulnerable decoy selects its area spell by affinity, uses
the owner's MC, and preserves the reference's Ice damage for all four spells.
Cursed Doll and Summon Dead require shape-1 amulets. Dragon Blood automatically
rolls charges while attacking, requires equipped shape-2 poison, consumes it
on nine of ten successful arming rolls, and spends its charge on selection.
Frost Bite includes IceAttack in its starting pool and cap, rolls Slow against
attackers, excludes bosses and friendly monsters, and cannot refresh an active
buff. Cursed Doll rejects bosses, friendly targets and targets already linked
to a doll, and admits hostile players under the existing PvP rules.
Soul Resonance now lasts until removal, gives its HP bonus only to the target,
and clears both links on buff removal, group leave or disconnect. Delayed
completion rechecks target life, group membership and existing resonance.
Summon Dead rolls against `MagicMaxLevel + 1` (five outcomes). Burning/Shocked
already had the correct four-outcome roll; both now name the shared constant.
Field art for Ice Aura, Burning Fire and Dark Soul Prison uses placeholder
frames on the client.
Originally: 98 magics implemented in Rust; 92 missing with C#
code (Warrior 17, Wizard 18, Taoist 28, Assassin 29); 5 dead enum entries
(FlameArt 137, UnityWithNature 247, SupremeHealing 351, Unused 438,
ManaBurn 458); 23 monster-only MagicTypes (501-540). Power/cost/delay come
from `MagicInfo` (GetPower = MinBasePower + Level*MinLevelPower/3 ..
MaxBasePower + Level*MaxLevelPower/3, random in range; Cost = BaseCost +
Level*LevelCost/3; cooldown = Delay ms). `MagicInfo.School == 20`
(Discipline) means the cost is FP, not MP.

Pipelines: magic damage = sum of ModifyPowerAdditionner, then multipliers,
minus GetMR, plus element power*2, minus power*resistance/10. Physical:
GetDC, accuracy unless IgnoreAccuracy, additioners, minus GetAC unless
HasMassacre. GetSP = random over min(MinMC,MinSC)..min(MaxMC,MaxSC).

## Warrior (17)

- 119 AugmentDestructiveSurge: attack augment, `power += GetPower()`.
- 120 AugmentDefiance: Defiance becomes duration 60+30L (+10+10L), phys/mag
  def +5+5L %, DC -max(0, 20-5L) % instead of -20 %.
- 121 AugmentReflectDamage: ReflectDamage = 5+3L (+aug power), duration
  15+10L s (+5+5L).
- 122 AdvancedPotionMastery: potions +value*GetPower()/100 again.
- 124 SeismicSlam: self, IgnoreAccuracy, disc radius 3 centred 3 ahead,
  600 ms, power = DC*GetPower()/100 (players /2); applies Paralysis 1x3 s,
  WraithGrip 1x1500 ms, Silenced 1x5 s. Anim Combat3; effect MagicEx5
  4900/6/100 + MonMagicEx7 700/7/120 at 2 ahead.
- 125 Invincibility: self buff 5+L s, all damage and poison damage 0.
  Anim Combat15; MagicEx5 400/10/100.
- 126 CrushingWave: 12-cell line, primary delay 400+i*60, flanks 200+i*60
  (flank cells at ShiftDirection +-2 cardinal / +-1 diagonal); flanks =
  power*GetPower()/100, primary full DC; players /2. Anim Combat3; MagicEx6
  100/6/100, projectile MagicEx6 200/8/100, MagicEx6 300/9/150.
- 127 DefensiveMastery: passive stat DefensiveMastery = GetPower(); GetAC
  returns max when Random(10) < value (>= 10 always).
- 128 PhysicalImmunity: passive, physical damage -= power*GetPower()/100.
- 129 MagicImmunity: passive, elemental damage -= power*GetPower()/100.
- 130 DefensiveBlow: 12 s charge (cost+cooldown on arming, MagicToggle);
  on hit target gets DefensiveBlow debuff 10 s: MagicDefencePercent and
  PhysicalDefencePercent = -GetPower(). MagicEx7 800/9/100.
- 131 ElementalSwords: self; SwordCount 5, buff; every 5 s fires at a random
  target within 5 that targets the player (delay 500+dist*48); power =
  GetPower(); on kill Random(4)==0 restores (Mana-MP)*(10+10L)/100 MP.
- 132 Shuriken: ranged auto attack when weapon shape == ShurikenLibraryWeaponShape;
  IgnoreAccuracy; projectile delay clamp(dist*50, 100, 750).
- 133 HundredFist: dash to the cell before the target along a straight
  line (300 ms); push distance = travelled*2 with gate Random(MaxLevel+12)
  >= 6+3L+Level-obLevel fails, Endurance blocks, CanPush; damage only when
  push blocked early: GetPower() + DC*pushed. Anim Combat8; MagicEx 1270/3.
- 134 OffensiveBlow: 12 s charge; power = power*GetPower()/100; on hit
  Pushed(dir, L+3) and if pushed: Paralysis 1x3 s + Silenced 1x3 s.
  MagicEx5 2305/5/100 on frame 3.
- 135 TaecheonSword: self radius 2, Fire, 1500 ms, power += GetPower() +
  DC*max(0, 4-distance). Anim Combat2; MagicEx5 5000/31/100.
- 136 FireSword: self radius 2 spiral (anticlockwise order), delay
  1300+100*index, power += GetPower() + DC, Fire. MagicEx5 5100/39/100.

## Wizard (18)

- 229 JudgementOfHeaven: self buff 30+30L s, stat (2+L)*20 % chance when
  hit to strike back: PvE MC/5 + LightningAttack*2, PvP min(50, ...) after
  300 ms, Lightning, ThunderBolt effect.
- 230 ThunderStrike: self random AoE radius 3 (6 at level > 3), 500 ms,
  each object 50 %; power = (GetPower() + MC)*1.5. Magic 1450/3/150.
- 231 FireBounce: target bolt, bounces L+2 times to a random monster within
  3 (delay dist*48, first +500); power = GetPower()+MC. Projectile Magic
  1640/6/100, impact Magic 1800/10/100, cast Magic 1560/9/65.
- 232 ElementalHurricane: channelled 8-cell beam with flanks (0.3), ticks
  every 500 ms costing Cost MP; stopping costs Mana*Cost/1000; element from a
  DarkStone amulet affinity; cancelled by being struck. Anim ChannellingStart.
- 233 SuperiorMagicShield: self, pool = Mana*(0.25+0.05L) absorbs raw
  damage; removes MagicShield. 1100 ms. MagicEx2 1900/17/60.
- 234 Burning / 235 Shocked: augments: burn/shock values for fire/lightning
  spells (`Random(MaxLevel) <= aug.Level` -> shock GetPower()).
- 236 LightningStrike: bolt chaining L+2 times like FireBounce; power =
  MC+GetPower() (multiplier is a no-op bug). MagicEx6 500/8/100, cast 400/8.
- 237 MirrorImage: cell within MagicRange, needs DarkStone amulet, element =
  first non-zero affinity; one decoy (flag 7) for L*5 s. MagicEx2 1260/6.
- 238 IceRain: cell radius 3, L passes, each cell 30 %, delay 980+idx*200;
  power = GetPower()+MC. MagicEx7 700/7 projectile + 720/7; cast Magic 1430/12/50.
- 239 FrostBite: self buff 3+3L s storing damage taken (FrostBiteDamage =
  MC+GetPower()+IceAttack*2 initially; chance 5+5L % to Slow attackers);
  on expiry hits non-boss monsters within 3 for min(stored, MaxMC*50 +
  IceAttack*70). MagicEx5 500/16/60.
- 240 Asteroid: cell radius 3, 1200 ms, GetPower()+MC, Fire; with FireWall
  known also drops FireWall diamond (|dx|+|dy| < 3) at 2250 ms. MagicEx5
  1300/10 projectile + 1320/8.
- 241 Storm: empty in C#.
- 242 Tornado: cell, summons Tornado monster (flag 8) for 10 s with stats
  derived from MC/HP/AC/MR/10 * max(1, L).
- 243 IceAura: 8-cell line, spawns one IceAura spell on the first cell with a
  target: TickCount 2, frequency 5+3L s, applies Paralysis value (3+L)*2.
  MagicEx5 2500/6 projectile.
- 244 IceDragon: target, Ice, Slow 2/level 3, delay 500+dist*48, power
  GetPower()+MC. MagicEx5 2800/2900 projectiles, impact 3000/12, cast Magic 2620/6/80.
- 245 IceBreaker: self radius 2, delay 500+500*dist, Slow 5/5. MagicEx5 5200/37.
- 246 FrozenDragon: self radius 2 twice (delay 500+500*dist*i), Slow 2/5.
  MagicEx5 5300/41.

## Taoist (28)

- 319 EmpoweredHealing: Heal/MassHeal bonus GetPower(), cap 30+(1+L)*30.
- 320 LifeSteal: cell radius 3 ally buff (2 amulets shape 0): LifeSteal
  4+2L for GetPower()+SC+DarkAttack*2 s. MagicEx2 2500/10, cast 2410/9.
- 321 ImprovedExplosiveTalisman: 1 amulet per target; extra targets with
  augment 328; power GetPower()+SC, +60 % with DarkAffinity amulet, x0.65
  secondary. MagicEx2 980/6 projectile, impact 1160/10.
- 322 AugmentPoisonDust: PoisonDust hits GetPower()+1 extra targets within 3.
- 323 CursedDoll: target (not boss, level <= player+2), 1 amulet shape 1,
  doll for 10+5L s forwarding damage (flag 5). MagicEx3 690/10/60.
- 324 ThunderKick: front cell radius 1, push GetPower() cells with gate
  Random(16) < 6+3L+Level-obLevel, then a plain DC attack. Anim Combat7.
  MagicEx2 1190/10.
- 325 SoulResonance: link with a group member; target HealthPercent
  GetPower(); when one dies both die.
- 326 Parasite: target poison Value GetPower(), 10+5L ticks x 2 s,
  Infection spreads each tick; explodes at end hitting radius 1 around the
  caster (C# quirk); power GetPower()+SC/2. MagicEx5 800/6 projectile.
- 327 Spiritualism: self, amulet shape 0: MaxAC/MaxMR +5+L, or the amulet's
  affinity resistance +5+L; duration GetPower()+SC*2. MagicEx2 1580/11.
- 328-331 augments for ExplosiveTalisman/EvilSlayer/Purification/
  Resurrection: GetPower()+1 extra targets within 2-3, own cooldown.
- 336 SummonDemonicCreature: 25 amulets, InfernalSoldier pet (flag 4)
  behind the caster, SummonLevel 2L, max 2 pets; recall if it exists.
- 337 DemonExplosion: 20 amulets; pet loses 75 % HP; targets within 2 of
  the pet take petHealth*GetPower()/100 + SC*3 (+PhantomAttack*8 with
  affinity) after 800 ms; fire wall radius 3 for L+2 ticks.
- 338 Infection: passive: each Parasite tick spreads poisons to one monster
  within 1 with Value (v*L+1)/10 and TickCount GetPower().
- 339 DemonicRecovery: toggle heals the pet Health*GetPower()/100.
- 340 Neutralize: 1 amulet per target, delay 1400+dist*48, target level <
  player: Neutralize poison 5+2L s (doubles attack and move delay).
  MagicEx7 300/4 projectile, impact 460/10, cast Magic 2080/6.
- 342 DarkSoulPrison: cell radius 3 field, L+5 ticks x 2 s, damage
  (GetPower()+SP)*0.4 Dark; player hits consume ticks. MagicEx6 600/9.
- 343 SearingLight: target Holy, GetPower()+SC; if damaged and level <=
  player+2, Random(3)==0 Fear 1 x (L+2) s. MagicEx3 1210/10 projectile,
  impact 1300/10, cast 1190/8/70.
- 344 AugmentCelestialLight: CelestialLight (L+1)*12 with aug at L>=3, plus
  SCPercent and MagicDefencePercent 2+2L.
- 345 CorpseExploder: dead target, 2 amulets, radius 1, delay 1000+dist*48;
  GetPower()+SC. MagicEx7 300/4 projectile, impact 1000/17.
- 346 SummonDead: raise a corpse as UndeadSoul pet (10 amulets shape 1),
  success Random(MaxLevel+1) <= L, SummonLevel 2L.
- 347 BindingTalisman: 1 amulet, Binding poison GetPower() ticks x 1 s with
  Value = damage (SC based); blocks movement. MagicEx5 3600/1, cast 3500/4.
- 348 BrainStorm: needs Binding on target; removes it; power SC*(L+1);
  shock GetPower(). MagicEx5 3200/5 projectile, impact 3400/15, cast 4600/10.
- 349 HeavenlySky: self radius 2, 1000 ms, power += GetPower()*DC,
  Lightning. MagicEx5 5400/39.
- 350 PoisonCloud: self radius 2, 2500 ms, Green poison Value L+1+Level/14,
  duration (GetPower()+SC+DarkAttack*2)/2 ticks x 2 s. MagicEx5 5500/56.

## Assassin (29)

- 427 TheNewBeginning: self stack buff min(L+2, stacks+1) for 1 min;
  removes Cloak; keeps cloak on later casts (Stealth). MagicEx4 2200/8.
- 428 DanceOfSwallow: blink adjacent to target, instant attack (400 ms)
  power += DC; Silenced 1 x (GetPower()+1) s and Paralysis 1 s when target
  level < player; cooldown x10 within 30 s of PvP.
- 429 DarkConversion: toggle buff ticking 2 s: MP -GetPower(), HP +2x.
- 430 DragonRepulse: 6 s channel, 500 ms ticks, radius 5 (4-direction
  distance), power = DC*GetPower()/100 + Level, push 1; costs HP and MP
  Health*Cost/1000 each. MagicEx4 1000/10/60 + 1020/10/60.
- 431 AdventOfDemon: passive MaxAC += GetPower(). 432 AdventOfDevil: MaxMR.
- 433 Abyss: target (level < player, not boss): Abyss poison (L+3)*2 s
  (x2 monsters), monster drops target and view range 2. MagicEx4 2000/14/70.
- 434 FlashOfLight: two cells ahead, 400 ms, power += DC*GetPower()/100 +
  80 per TheNewBeginning stack (+1); consumes a stack. MagicEx4 2300/8/60.
- 435 Stealth: passive: with TheNewBeginning stacks, Random(100) >
  GetPower() burns a stack, else cloak survives the cast.
- 436 Evasion: self buff GetPower() s, EvasionChance 4+2L (elemental only,
  Random(100 or 200 PvP) <= chance -> miss). MagicEx4 2500/12/70.
- 437 RagingWind: self buff GetPower() s: AC/MR pools re-split 30/70 with
  +4+6L. MagicEx4 2600/12/70.
- 439 Massacre: passive: swing ignores AC/resistance; overkill damage *
  GetPower()/100 hits non-boss monsters within 1 of the corpse after 600 ms.
- 440 ArtOfShadows: SummonPuppet count += GetPower(), range 3.
- 441 DragonBlood: self-arming attack (Random(5)==0 re-arm needs green
  poison item shape 2, 90 % consumed): Green poison 10 x 2 s Value
  SP*GetPower()/100 stacking to 4. MagicEx5 200/7.
- 442 FatalBlow: always-on: target HP < 30 % and Random(MaxLevel+1) <= L ->
  power += power*GetPower()/100.
- 443 LastStand: passive: HP < 30 % -> Random(MaxLevel+1) <= L -> buff
  PhysicalDefencePercent GetPower() until HP recovers.
- 444 MagicCombustion: PvP only MP drain DC*GetPower()/100.
- 445 Vitality: passive at HP < 30 %: potions +GetPower() %, regen rate
  +0.5+0.5L.
- 446 Chain: tether up to 5+2L non-boss monsters within 2 of the target for
  GetPower() s (followers stay within 2 of the leader); ChainOfFire tiers.
  MagicEx7 20/7, cast 0/7/140.
- 447 Concentration: self buff GetPower() s CriticalChance 5+L. MagicEx5 300/15.
- 448 DualWeaponSkills: attack skill with DualWield weapons: power += power*GetPower()/100.
- 449 Containment: 1+L random targets within 3 (level <= player+2, success
  Random(MaxLevel+1) <= L): Containment 3 x 1 s Value SP (root + tick).
  MagicEx3 590/9/60.
- 450 DragonWave: FlameSplash augment: power += power*GetPower()/100; at
  L>=3 all 8 directions.
- 451 Hemorrhage: target GetPower() ticks x 1 s Value SP (blocks regen);
  power += SP. MagicEx7 1100/6 projectile, impact 1270/10.
- 452 BurningFire: placed mine (max clamp(L,1,3)), 15 ticks x 1 s, explodes
  radius 1 when stepped on: GetPower()+SP Fire. MagicEx6 900/10/60.
- 453 ChainOfFire: Chain augment; explosion radius 2 power MC*GetPower()/100.
- 456 FourWheels: self radius 2, 1500 ms, power += GetPower()*SP. MagicEx5 5600/35.
- 457 CrescentMoon: self radius 3, 1500 ms, power += GetPower()*SP. MagicEx5 5700/21.

## Monster-only MagicTypes

501 MonsterScortchedEarth, 502 MonsterIceStorm, 503 MonsterDeathCloud, 504
MonsterThunderStorm, 505-508 SamaGuardianFire/Ice/Lightning/Wind, 509-512
SamaPhoenixFire/SamaBlackIce/SamaBlueLightning/SamaWhiteWind, 513-515
SamaProphetFire/Lightning/Wind, 520-525 DoomClaw*, 530 PinkFireBall, 540
GreenSludgeBall. Horse riding, mining, fishing and companions are not
magics (MirAction states).
