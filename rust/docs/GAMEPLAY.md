# Gameplay guide

Controls, UI windows and the rules the server implements, as transcribed from Zircon. Back to the [README](../README.md).

## Controls

Controls in the world: hold left mouse to walk, right mouse to run, click an
adjacent monster to attack, click an NPC to talk (you walk over), click a
ground item to walk to it and pick it up, `Tab` picks up what is under you,
`W` opens the bag, `Q` the character window, `Esc` closes the open windows
and, with nothing open, asks whether to leave. In the bag, right-click uses or equips an item
(or sells it while a shop is open); in the character window right-click
unequips. Helmets and shields show on the character. The belt above the HUD
(toggle with `Z`) has ten slots on keys `1`-`9` and `0`: pick up a bag item
and click a slot (or hover the item and press the key) to link it. Press a
number key twice within 450 ms, or left-click the belt slot once, to use it;
right-click a linked slot to clear it. Modified number keys never use belt
items.
Potions heal at once and every consumable starts the item cooldown shown on
the belt (Zircon's `Durability` in ms, 250 ms floor). Shop lists scroll with
the wheel. When you die a Revive button returns you to town at once; the
server forces it after ten minutes. The backquote key toggles the debug line,
`Ctrl+Alt+S` saves a screenshot. `F1`–`F12` bind or cast skills; on macOS,
`Cmd` or `Option` with `1`–`0`, `-`, and `=` uses the same twelve slots.
You can also click a spell-bar icon to cast it.
Heals, shields, buffs and summons can be cast while walking or running, but
casting remains blocked while mounted.

The minimap sits under the buff icons in the top-right corner and draws
`MapInfo.MiniMap` from the `MiniMap` library, the same image Zircon uses, so
one map cell is `image size / map size` pixels. It scrolls to keep you in the
middle and clips at the panel edge (Zircon's `ClipMap`), with your
coordinates in the corner. `V` cycles it between shown, dimmed and hidden,
matching Zircon's one-key cycle. `X` opens the big map: the same image at
full size, centred on screen and titled with the map name, closed again by
`X`, `Esc` or its close button. Both windows dot everything you can see:
you in lime with a white pip, group mates lime, your partner pink, guild
mates sky blue, other players white, monsters red, your own pets orange and
NPCs yellow. If `MiniMap` is 0 or its image is unavailable, both windows show
a walkability overview at two pixels per cell. This includes generated maps
and editor Test walk sessions. Floor is tan, blocked terrain is dark, and
the usual object dots track your position and nearby creatures. The overview
is cached when the map loads and discarded when leaving it.

Chat follows Zircon's prefixes: `Enter` opens the chat bar, `Enter` sends
and `Esc` closes it. Plain text talks to players in view range (with a
bubble over your head), `/name text` whispers, `!text` shouts to the whole
map (level 2, once per 10 s), `!@text` shouts to every player (level 33,
once per 30 s), `!!text` goes to your group and `!~text` to your guild.
Typing `/`, `!` or `@` with the bar closed opens it with that prefix
already in place, and `/` on its own reuses the last name you whispered
(Zircon's `LastPM`); the arrow keys walk back through the lines you have
sent, and the target of the line being typed is shown beside the bar.
Characters with a saved server-side GM grant can type `@help` for the 33
classic admin commands. `@move map-file [x y]` travels between maps;
`@move x y` moves on the current map, and `@monster name [count]` spawns mobs
(`@spawn` is an alias). `@moblocation <id or name>` lists live monster
coordinates on loaded maps, plus saved spawn maps and area sizes. Event-only
spawns are marked. Its `@move` examples land beside live monsters or safely
on a spawn map. Account commands include `@ban`, `@chatban`, `@globalban`
and `@kick`. The command grant is checked on the server; a
client cannot grant itself GM access. On Rust, `@reboot` performs the
classic affected-listing cleanup, `@gcollect` is unnecessary, and
`@clearipblocks` reports that no persistent IP block list exists. Game Gold
is currently stored per character, so its GM commands follow that model.
Item IDs, types, and inventory sprite indices from the installed database are in
[data/item_ids.csv](data/item_ids.csv). The atlas exporter below regenerates it
with item pictures.
Character levels reach 1000. Beyond the source experience table's level 100,
each level adds the table's final 12-trillion XP step. Beyond the database's
level-90 base stats, HP and MP follow the final ten-level trend; each class's
primary maximum attack stat also gains one point per level.
Monster IDs, levels and boss flags are in [data/monster_ids.csv](data/monster_ids.csv).
Regenerate with `cargo run -q -p mir-server --example monster_ids -- /path/to/System.db > docs/data/monster_ids.csv`.

World boss hunts rotate every two UTC hours through five event-only boss
respawns linked by the installed database's legacy `EventInfo`, `EventTarget`
and `EventAction` rows. Defeat the announced boss before the slot ends for
normal boss drops and 100,000 Gold per nearby living group member. Type
`@event` as any player for the current map, location and time remaining.
Completed slots and pending Gold grants persist in `world_event.json` under
the server data directory. A character's paid-slot marker saves with Gold in
`accounts.json`, so a restart replays only unpaid grants. An unfinished hunt
resumes with a new boss location after restart. This
short hunt uses the source bosses, not the old multi-stage seal counters.

The log above the bar is Zircon's `ChatTab`: it keeps 250 lines, wraps
them to the panel, scrolls with the wheel, and has tabs (All, Local,
Group, Guild, Whisper, System) that filter by message kind and light up
when an unwatched tab has something new. Line colours are Zircon's
defaults (white talk, yellow shout, lime global, plum group, pink guild,
cyan whispers, red system on its light plate). Like Zircon's default tab
it fades out: with the bar closed and the pointer away only the recent
lines show, without the panel chrome. `ZIRCON_AUTO_CHAT=text` says
something two seconds after entry and `ZIRCON_DEV_CHAT_LINES=n` fills the
log with n sample lines of every kind for screenshots.

Groups follow Zircon's `PlayerObject.Social`: `P` opens the group window
with an "Allow group" toggle (invites are refused while it is off), a name
box with Invite, Kick buttons for the leader and Leave. The invitee gets a
prompt (Accept / Decline); the first member leads, groups hold 15 and a
group left with one member dissolves. Members on the same map within 18
cells share kills: the living ones split the experience (+6% per member,
weighted by level) and each rolls the drops with the chance and gold
divided by the member count, owning what they roll. `!!text` is group
chat. `ZIRCON_AUTO_GROUP=name` invites that player after entry and
`ZIRCON_AUTO_GROUP_ACCEPT=1` accepts any invite.

Storage (`S`, or `ZIRCON_OPEN_STORAGE=1`) is the account's 100-slot bank,
shared by its characters and kept in `accounts.json`: pick up a bag or
equipment item and click a storage slot to store it, or the reverse to take
it out. As in Zircon it only works inside a safe zone (`SafeZoneInfo`
regions), items flagged `CanStore = false` are refused and withdrawing
never checks weight. Trading (`T`) asks the player standing in the cell in
front of you who faces you; they get an Accept / Decline prompt. In the
trade window right-click bag items to offer them (15 at most, `CanTrade`
only), type a gold amount and press Set gold (it can only be raised), then
both press Confirm; any change to an offer clears both confirmations, a
side without enough free bag slots is unlocked to make room, and a step,
turn, death or disconnect closes the trade.

PvP uses Zircon's attack modes, cycled with `Ctrl+H` (Peaceful, Group, Guild,
War/Red/Brown, All) and shown in the status line: outside safe zones your
swings and spells that go through the hostility check hit other players
according to the mode. Hitting an innocent player turns your name brown
for a minute; killing one adds 50 PK points (yellow name at 50, red at
200, one point fades per minute) and red names are attacked by guards.
See `research/pvp.md` for what the C# does beyond that.
Pet modes cycle with `Ctrl+A`: Move + Attack, Move, Attack, PvP, None. The
active attack and pet modes appear above the HUD and the pet choice is saved
with the character.

Guilds (`G`) follow Zircon's `PlayerObject.Social`: founding one costs
7,500,000 gold plus 1,000,000 per member slot (the window buys ten) and
needs a unique 2-15 character alphanumeric name; the founder is "Guild
Leader" with every permission, joiners get the default rank "New Member".
Members with the AddMember permission invite by name (the invitee gets a
prompt), leaders kick, set ranks and permissions, the tax (a cut of every
gold pickup that feeds the guild funds) and buy member slots from the
funds; anyone with EditNotice sets the notice. `!~text` is guild chat,
the guild and rank show under the player's name, and the Guild attack
mode spares guild mates. Guilds persist in `guilds.json` under the data
directory; see `research/guilds.md` for the rest of the C# rules.

Guild wars and castles follow the same C# (`research/guild-wars-castles.md`).
Castle guards, gates and flags come from the `CastleGuardInfo`,
`CastleGateInfo` and `CastleFlagInfo` rows (this pack has none, so the
mechanics are exercised by the tests): guards shoot enemy guild members
within 15 cells during a war and are hurt only by them, gates block their
cells while shut, open by themselves for owner members walking up in
peacetime and shut again ten seconds later, the owner guild toggles them
with "Gates" and its leader repairs wrecked gates and guards with
"Repair" from the funds, and a flag changes hands after an enemy guild
stands beside it unopposed for thirty seconds.
A member with the StartWar permission types a guild name in the guild
window and presses "Declare war": 200,000 leaves the funds, both guilds are
told, and for two hours members of the two guilds are enemies in the
War/Red/Brown attack mode with no brown names or PK points for the kills
(both guilds hear who fell to whom); enemy-guild names show in orange. The
asset pack's castle, Sabuk Wall, opens its war every day at its start time
for its duration: a leader of a castle-less guild presses "Request
conquest" (it costs the castle's item) and the guild is signed up for the
war the day after tomorrow, when the castle lord appears in the castle
region, everyone not in the owner guild is moved to the attackers' region
and, while the war lasts, anyone on the map outside your guild is an enemy.
Only members of castle-less participating guilds can hurt the lord, one
point per hit, and the guild that fells him takes the castle: owners earn
10 % more experience and are announced when the war ends.
`ZIRCON_DEV_CONQUEST=1` opens the war for everyone as soon as the server
starts.

Mail (`,`) works like Zircon's: compose to a character name (offline is
fine) with a subject, message, gold and up to five bag items (right-click
them while composing; items need a safe zone), one mail per ten seconds.
Mail arrives live or waits in the account's box in `mail.json`; unread
mails show yellow, attachments are taken one by one in a safe zone with
bag room, and a mail can be deleted once it is empty. Recipients' boxes
hold fifty attachments.

Horses come from NPC pages with Zircon's `ChangeHorse` action (the horse
dealers): owning one adds bag weight and, from the white horse up,
attack and defence. `M` mounts and dismounts on maps that allow horses;
riding runs three cells a step but you cannot attack, cast, toggle
stances or use items from the saddle, and a push, death or a map that
forbids horses dismounts you. The horse draws under the rider with the
real horse libraries, horse armour picks the skin. Marriage runs through
the NPCs: the `Marriage` action proposes to the player facing you (both
level 22 with 500,000 gold, paid on acceptance), the wedding-ring page
turns a bag ring into the wedding ring (right-click it), the character
window shows the partner and a "To partner" button that teleports you
next to them once every two minutes while the ring is on, and `Divorce`
ends it. See `research/mounts-marriage.md`.
Mining and fishing follow Zircon's `PlayerObject.Movement`: with a Pick
Axe equipped (level 20) click an adjacent wall on a `CanMine` map
(Deserted Mine, Quartz Mine, Dragon Abyss) to swing at it; every
`MineInfo` row rolls its 1-in-N for an ore, the pickaxe loses 4
durability and rubble piles up under you. Fishing casts at a fishing zone
within throw range with a rod and robe on: click the water, the float
sits, and once a fish nibbles click to reel; each reel adds points, each
miss removes some, 50 lands a catch from the zone's table and 0 loses it.
This asset pack ships no fishing zones, rods, robes or bait, so
`ZIRCON_DEV_FISHING=<item index>` (server side; set it on the client too
to click without a rod) makes every unwalkable cell fishable for that
item. See `research/fishing-mining.md`.
Weapon refining follows Zircon's `NPCRefine`: at an NPC page of the Refine
dialog type a window opens under the dialog; pick the refine type (DC,
durability, spell power or an element) and a quality (Rush to Precise,
waiting one minute to one day), right-click black iron ore (up to five),
common jewellery (three) and a refine special (one) in the bag to add them,
and press Refine to pay 50,000 gold and send the equipped weapon into the
furnace. The chance is 60% minus 5% per weapon level, plus 1% per 2,000
ore durability and per 6 jewellery levels and 25% per superior item, under
a ceiling of 90% adjusted by quality. A RefineRetrieve page lists the
weapons and hands them back once ready: a success adds a bonus tagged with
the Refine source. Both outcomes clear readiness. Combat experience advances
weapon level and unlocks the next refine using C#'s weapon experience table;
quests and experience consumables also advance it. Pickaxes and permanently
non-refinable weapons do not gain this progress. The tooltip and Refine
button reflect readiness. Pending refines persist with the character. Oil Of
Benediction raises or lowers equipped weapon Luck using Zircon's original
rates (or consumes the oil with no change). Accessory pages now support
combining matching rings, bracelets and necklaces for experience; upgrading
ready accessories; resetting Refine-source stats for 1,000,000 gold; and
Corundum refining for 50,000 gold. Corundum refining can destroy the target.
Right-click bag items to select them in each accessory page. The supplied
asset database has no Corundum-specific NPC page, so that action also appears
on the existing accessory upgrade page.

Companions come from a CompanionManage page (`research/refining-
companions.md`): adopt one of the `CompanionInfo` looks for its price with
a name (locked looks need their unlock item), then bring it out, store it
or release it there. A companion follows you, teleports after you when it
falls behind, and picks up your own drops within eight cells into its bag
(gold goes straight to you); its bag size and weight come from
`CompanionLevelInfo` (level one carries nothing), it gains experience once
a minute and levels up, and it loses one hunger a minute outside safe
zones, refusing to gather when starving until fed with a consumable that
has the companion hunger stat. `U` opens the companion window with its
level, hunger and bag; Take moves an item into yours. Companions earn skill
bonuses at levels 3, 5, 7, 10, 11, 13 and 15. Arm a learned level in that
window or type `@EnableLevelN`, then use the matching Elixir Of Regret to
reroll it. The safety toggle resets on login; the new bonus persists.

What exists in the world: real spawns with Zircon's drop tables (1 in N per
row), ground items that only the killer can take for two minutes, gold, the
125 NPCs with their data-driven dialog pages, buy/sell shops with Zircon's
prices, and every map exit from `MovementInfo` (with level requirements).

Skills: learn them from books (right-click in the bag), open the skill window
with `E`, hover a skill and press `F1`-`F12` to bind it, then press the key to
cast. Implemented with Zircon's power, cost, cooldown, skill-experience rules
and the original effect sprites:

- Warrior: Swordsmanship, Potion Mastery, Slaying, Thrusting, Half Moon,
  Shoulder Dash (dash that shoves weaker monsters aside), Flaming Sword,
  Dragon Rise and Blade Storm (12 s charges consumed by the next swing),
  Destructive Surge (stance hitting all eight neighbours), Defiance and Might
  (self buffs, mutually exclusive), Interchange (swap places), Beckon and
  Mass Beckon (pull and paralyse), Swift Blade (sure-hit slash), Assault
  (dash that stuns), Endurance (poison immunity), Reflect Damage, Fetter
  (slow).
- Wizard: Fire Ball, Lightning Ball, Ice Bolt, Gust Blast, Repulsion,
  Teleportation, Adamantine Fire Ball, Thunder Bolt, Ice Blades (slow),
  Cyclone, Scorched Earth, Lightning Beam, Frozen Earth, Blow Earth (lines
  of 8 cells with 30 % flanks), Fire Wall (burning cells for 20-60 s), Magic
  Shield (halves damage, drains with hits), Electric Shock (tame), Expel
  Undead, Geo Manipulation, Fire Storm, Lightning Wave, Ice Storm, Dragon
  Tornado, Greater Frozen Earth, Chain Lightning (hops), Meteor Shower,
  Renounce (MC for HP), Tempest (wind cells that repel).
- Taoist: Heal, Spirit Sword, Poison Dust, Explosive Talisman (uses a
  talisman), Evil Slayer and Greater Evil Slayer (holy talisman bonus), Magic
  Resistance and Resilience (area buffs costing talismans), Mass Heal,
  Summon Skeleton, Summon Shinsu and Summon Jin Skeleton (pets that follow,
  fight and return when recalled), Strength Of Faith (pet DC), Invisibility,
  Mass Invisibility and Transparency (monsters lose hidden targets), Trap
  Octagon (shock ring), Combat Kick, Elemental Superiority, Blood Lust,
  Resurrection, Purification, Celestial Light (survive a killing blow).
- Assassin: Willow Dance, Vine Tree Dance, Discipline, Bloody Flower (life
  steal), Poisonous Cloud (agility cloud), Full Bloom, White Lotus, Red Lotus
  and Sweetbrier (press the key to arm the next swing; each opens the next
  in the chain), Flaming Daggers, Shredding, Cloak (HP-priced stealth that
  drains every 2 s, breaks on running, swinging or casting; Pledge Of Blood
  cuts the drain, Ghost Walk rolls a deeper stealth), Calamity Of Full Moon
  and Waning Moon (self-arming power swings, out of and inside the cloak),
  Wraith Grip (roots; Touch Of The Departed adds paralysis), Hell Fire
  (fire strike plus burn), Rake (from the cloak, guaranteed slow), Summon
  Puppet (decoys that explode, then blink away cloaked), Karma (armed
  execute from the cloak costing HP; Resolution and Release scale it),
  Flame Splash (toggle: up to four extra directions per swing),
  Rejuvenation (faster regeneration).
- Wave four (all classes): passives Defensive Mastery, Physical Immunity,
  Magic Immunity, Advent Of Demon, Advent Of Devil, Vitality, Last Stand,
  Fatal Blow, Dual Weapon Skills, Massacre; self bursts Seismic Slam,
  Taecheon Sword, Fire Sword, Thunder Strike, Ice Breaker, Frozen Dragon,
  Heavenly Sky, Poison Cloud, Four Wheels, Crescent Moon, Flash Of Light;
  targets Ice Dragon, Searing Light, Hemorrhage, Abyss, Containment,
  Neutralize, Parasite; cell areas Ice Rain, Asteroid, Life Steal; self
  buffs Invincibility, Evasion, Raging Wind, Concentration, The New
  Beginning, Judgement Of Heaven, Superior Magic Shield, Dark Conversion,
  Spiritualism.
- Wave five: Defensive Blow and Offensive Blow (12 s charges; the latter
  shoves, paralyses and silences), Crushing Wave (12-cell line with
  flanks), Fire Bounce and Lightning Strike (bounce to nearby monsters),
  Ice Aura (paralysing field on the line), Burning Fire (mines), Dark Soul
  Prison (dark field), Summon Demonic Creature, Demon Explosion and Demonic
  Recovery, Thunder Kick (push then strike), Dance Of Swallow (blink
  behind and strike), Hundred Fist (straight dash and shove), Dragon
  Repulse (6 s repelling channel paid in HP and MP), Elemental Swords
  (five auto-firing swords), Binding Talisman and Brain Storm, Improved
  Explosive Talisman; augments Augment Defiance, Augment Reflect Damage,
  Augment Destructive Surge, Advanced Potion Mastery, Empowered Healing,
  Stealth, Art Of Shadows, Dragon Wave.
- Wave six: Elemental Hurricane (channelled 8-cell beam with flanks, ticking
  every half second until you move, turn, are struck or run dry), Mirror
  Image (a Dark Stone decoy), Frost Bite (banks the damage you take and
  bursts it on monsters within 3 when it ends), Tornado (a 10 s summon),
  Cursed Doll (a doll that forwards what it suffers to its victim), Soul
  Resonance (linked group members die together), Corpse Exploder and
  Summon Dead (target a corpse; click one), Dragon Blood (poisoned blades),
  Chain (tethers monsters within 2 of the target to it) with Chain Of
  Fire, and the passives Shuriken (thrown attacks with a shuriken weapon),
  Burning, Shocked, Augment Poison Dust, Infection and Magic Combustion.
  Zircon's ~174 player magics are now all present; see
  `research/skills-remaining.md` for the simplifications.

Buff icons with their remaining time sit top-right. Other skills show in the
window but cannot be cast yet. Hovering an icon names the buff and the
seconds it has left (Zircon `BuffDialog`).

Screen overlays (`../crates/mir-client/src/overlay.rs`, drawn outside the
windows):

- **Target panel** (Zircon `MonsterDialog`) at (250, 50): the name box and a
  health bar with `hp/max` over it, for the monster or player last pointed
  at. It follows the cursor to a new target and clears when that target dies
  or leaves sight. Poison and visible buffs on the target show as small chips
  beside the bar.
- **Quest tracker** (Zircon `QuestTrackerDialog`) top-right under the buff
  icons: every quest flagged as tracked, with each task's progress; complete
  tasks turn green. Clicking a quest title in the quest log (`J`) toggles
  tracking, and the client applies the flag at once because the server stores
  it silently. Its background fades in while the mouse is over it.
- **Damage numbers** use Zircon's `DamageInfo` tiers: blue when it heals, then
  red, green, orange and white as the hit grows.

Developer automation (also used for visual checks):
`ZIRCON_SCREENSHOT=out.png:5` saves a frame after 5 s and exits;
`ZIRCON_HEADLESS=1` renders to an offscreen texture instead of the window
(screenshots then work even with the screen locked, where macOS stalls the
swapchain);
`ZIRCON_MOUSE=x,y` parks the pointer so a screenshot can show a hover state
(HUD tooltips);
`ZIRCON_AUTOLOGIN=email:password` logs in, creating the account if missing;
`ZIRCON_AUTOSTART=name` enters the world with that character, creating a
warrior if needed; `ZIRCON_OPEN_CREATE=1` opens the character creation dialog;
`ZIRCON_OPEN_WINDOWS=1` opens the bag and character windows on entry;
`ZIRCON_OPEN=storage,group,guild,mail,quests,inventory,character,skills,`
`companion,menu,help,exit,currency,autopotion,dropfilter,bigmap,nominimap,`
`ranking,friends,blocked,market,marketconsign,marketstore,fortune,chatoptions,characterservices`
opens any set of windows. `ZIRCON_DEV_SEARCH=<text>` prefills the fortune
window's search box so a screenshot has rows; server side
`ZIRCON_DEV_TIMER=<seconds>` starts a HUD countdown on entry and
`ZIRCON_DEV_FORTUNES=<n>` pre-checks the first `n` droppable items with
made-up progress; server side `ZIRCON_DAY_TIME=0.1` pins the
daylight (night screenshots);
`ZIRCON_AUTO_NPC=name` walks to that NPC and opens its dialog;
`ZIRCON_AUTOCLASS=wizard|taoist|assassin` picks the auto-created class;
`ZIRCON_OPEN_SKILLS=1` opens the skill window; `ZIRCON_AUTO_CAST=<F key>`
casts that key's spell at the nearest monster (`m<id>` casts by magic id, e.g.
`m216` for Fire Wall; self casts such as `m415` Summon Puppet need no monster); `ZIRCON_AUTO_BELT=1` links every
consumable type in the bag to the belt and `ZIRCON_AUTO_USE=<slot>` presses
that belt key once. Server side, `ZIRCON_DEV_LEVEL=n` starts new characters at
level n, `ZIRCON_DEV_SKILLS=1` grants every class skill the level allows, and
`ZIRCON_DEV_ITEMS="Bronze Helmet;Healing Potion*5"` hands out (and wears)
the named items on entry, `ZIRCON_DEV_HORSE=2` gives a horse (Zircon
`HorseType`), `ZIRCON_DEV_START=2:150,182` starts new characters on that
map file and cell; client side `ZIRCON_AUTO_MOUNT=1` mounts after entry,
`ZIRCON_AUTO_MINE=1` walks to the nearest wall and keeps swinging the
pickaxe at it, `ZIRCON_AUTO_FISH=1` casts at the nearest water within four
cells and reels every bite, `ZIRCON_AUTO_TARGET=1` latches the nearest
monster into the target panel (no cursor in headless runs) and
`ZIRCON_MOUSE=x,y` parks the cursor so hover-only overlays such as the buff
tooltip can be captured; `ZIRCON_DEV_QUESTS=n` fills the quest log and the
tracker with the first n catalogue quests, half done, so neither needs an NPC.
Overlay check: server
`ZIRCON_DEV_LEVEL=60 ZIRCON_DEV_SKILLS=1`, client
`ZIRCON_AUTO_TARGET=1 ZIRCON_AUTO_CAST=m111 ZIRCON_MOUSE=1000,16
ZIRCON_DEV_QUESTS=2`. Mining check: server
`ZIRCON_DEV_LEVEL=30 ZIRCON_DEV_ITEMS="Pick Axe" ZIRCON_DEV_START=D201:227,28`
(Deserted Mine Lv 1), client `ZIRCON_AUTO_MINE=1`. Fishing check: server
`ZIRCON_DEV_FISHING=133` (Healing Potion as the catch), client
`ZIRCON_DEV_FISHING=1 ZIRCON_AUTO_FISH=1` (start next to a wall, e.g.
server `ZIRCON_DEV_START=0:168,240`); both with `ZIRCON_HEADLESS=1
ZIRCON_SCREENSHOT=out.png:8 ZIRCON_AUTOLOGIN=... ZIRCON_AUTOSTART=...`.
This asset pack's body libraries have no fishing frames (2000-2239), so
the client shows the swing and standing poses while fishing.

`cargo run -p mir-server -- --inspect` prints the start map, player stats and
spawn table without opening a port. `--map <file>` forces a start map.
`ZIRCON_SEED=n` seeds the server's random rolls (the tests use a fixed seed and
ordered object maps, so a failing roll reproduces).

## HUD menu and system dialogs

The eight icons at the right of the main panel are Zircon's `MainPanel`
buttons, drawn from the same `GameInter` sprites at the same offsets:
Character (82), Inventory (87), Skills (92), Quest log (112), Mail (97),
Belt (107), Group (102) and Menu (117). Each one opens or closes its
window, lights up under the pointer and shows a hint naming the window and
its key. Zircon's cash shop button (index 122, `GameStore`) has no window
here, so it is not drawn.

`N` or the last icon opens the menu (Zircon's `MenuDialog`): Help, Guild,
Storage, Companion, Currency, Ranking, Settings, Auto potion, Drop filter and
Leave game. Settings persists five sound-channel levels and mute beside the
client executable in `client-settings.json`; active audio updates immediately.
`ZIRCON_MUTE` and `ZIRCON_VOLUME` remain explicit environment overrides.
Settings also offers resolution presets from 800x600 to 3840x2160. A new size
applies immediately; Save keeps it for the next launch. `--size` and
`ZIRCON_WINDOW_SIZE` override the saved size.
Currency, Auto potion and Drop filter are reached from the menu here rather
than from the places the C# client puts them.

`H` opens the key list (Zircon binds `H` to `HelpWindow`; `F1` is a spell
key in this client, as it is in Zircon). The list is built from the real
bindings in `game/input.rs` and scrolls with the wheel if the screen is too
short for it. The client now follows Zircon's `KeyBindAction` defaults:

| Key | Does |
|---|---|
| `Q` `W` `E` | character, inventory, skills |
| `J` `,` `Z` | quest log, mail, belt |
| `P` `G` `S` `U` | group, guild, storage, companion |
| `F` `R` `C` | friends and blocked names, rankings, market place |
| `V` `X` | cycle the minimap, open the big map |
| `K` `O` | fortune checker, chat options |
| `N` `H` `A` | menu, help, auto potion |
| `M` `T` | mount or dismount, ask to trade |
| `Ctrl+H` | attack mode |
| `Ctrl+A` | pet mode |
| `Alt+Q` | leave the game |
| `Tab` | pick up |
| `B` | harvest a nearby corpse (or hold left mouse over an adjacent corpse) |
| `D` | character appearance changes and discipline training |
| `1`-`9` `0` twice | use a belt slot |
| `F1`-`F12` | cast a skill (Mac: `Cmd` or `Option` with `1`-`0`, `-`, `=`) |

`Alt+Q`, the menu's Leave game row, or `Esc` with nothing open asks the
question Zircon's `ExitDialog` asks: Character list logs out to the
character list, Exit game closes the client.

The currency window lists the player's `CurrencyInfo` holdings beside the
gold in the bag. The server sends `ServerMessage::Currencies` on entry and
again whenever an amount moves (NPC currency actions, quest rewards, fame
titles and companion purchases).

The ranking board (`R`, Zircon `RankingDialog`) lists every character on the
server, offline ones included, ordered by level and then experience. Class
tabs and an "online only" toggle narrow it, the wheel scrolls it, and the
player's own row carries a red plate. Two rules come straight from Zircon's
`GetRanks`: the rank number counts everyone the **class** filter admits, so
hiding offline players leaves the numbers alone, and the "move" column is how
many places a name has gained since the last daily reset. The server answers
`ClientMessage::RankRequest` with a page of 21 rows and the filtered total;
the board re-asks every five seconds while it is open.
`ZIRCON_RANK_CLASS=wizard` and `ZIRCON_RANK_ONLINE=1` preset its filters for
headless screenshots.

The friends window (`F`, Zircon's `CommunicationDialog` minus its two mail
tabs, which are our separate mail window) lists friends sorted by online
state, each with a coloured `(State)` suffix, and blocked names on a second
tab. A status row picks what the player advertises (Online, Busy, Away,
Offline) and a filter row narrows the list to one state. Selecting a friend
offers Whisper, which pre-fills the chat bar, and Invite.

Zircon stores the two differently, and so do we (`social.json` under the
data dir): a friend belongs to a **character**, a block to an **account**,
so blocking one name silences every character that person owns. Blocking is
mutual, as `SEnvir.IsBlocking` is: it stops whispers, group invites and
trade requests in both directions. A blocked whisper is answered with
"Could not find <name>", so the sender is never told they were blocked.
Friends see each other come and go: entering the world pushes a status line
to everyone holding that name, and leaving flips them to Offline.
`ZIRCON_DEV_FRIENDS=1` on the server seeds one friend per state and one
blocked name on a character's first entry, so the window can be
screenshotted without a second account.

### Market place

`C` opens the market place (Zircon `MarketPlaceDialog`, which binds the same
key), also reachable from the menu. It has three tabs: two for the player
auction house, and the Store.

- **Search** browses everything on sale, nine rows a page like Zircon, with a
  name box, an item-type filter and a sort (newest, oldest, highest price,
  lowest price). Pick a row, set a count and Buy. Your own rows show the
  seller in green and cannot be bought, which is Zircon's rule.
- **Consign** lists an item out of the bag at a price per unit with a note,
  and takes listings back. It shows what the sale would fetch after tax
  before you commit.

The rules follow `PlayerObject.Quests`:

| Rule | Value |
|---|---|
| Listing fee | none (`Globals.MarketPlaceFee` is 0) |
| Tax on a sale | 7% (`Globals.MarketPlaceTax`), taken from the seller |
| Expiry | none; a listing stands until bought or cancelled |
| Where | consigning needs a safe zone |
| Limit | level x 3 listings, never fewer than 5 |

The seller is paid by mail, not in gold, with a receipt naming the buyer, the
sub total, the tax and the net. Goods go straight to the buyer's bag when
they are in a safe zone with room, and by mail otherwise, so a full bag never
eats a purchase. Listings live in `market.json` beside `mail.json` and
`guilds.json`, and a part-bought listing shrinks rather than vanishing.

### The Store tab

The third tab is the in-game store (Zircon `MarketPlaceStoreBuy`), a cash
shop rather than part of the auction house: its stock is the `StoreInfo`
table, 92 rows in this pack, it never runs out, and it is paid for in **game
gold** or **hunt gold** rather than the gold carried in the bag. Both are
ordinary `CurrencyInfo` rows (types 1 and 2), so the existing currency
system holds them.

The tag list beside the name box is built from the data, exactly as Zircon
builds its drop-down: every `Filter` column is split on commas and trimmed,
and the distinct tags are sorted with "All" first. In this pack that yields
Assassin, Companion, Consumable, Function, Lazy, Mount, Permanent, Refine,
Rose, Storage, Taoist, Temporary, Torch, Warrior and Wizard. Sorting offers
A-Z, highest price and lowest price; Zircon's fourth option, Favourite, is a
`TODO` in its own client and sorts nothing, so it is left out.

Two rules are easy to misread in the C# and are worth stating:

- A **zero `HuntGoldPrice` means the row costs its `Price` in hunt gold**,
  not that hunt gold is refused. Every row in this pack has a zero there, so
  without this rule the whole store would be free for hunt gold.
- **Room is checked before money.** A full bag answers "not enough space"
  and costs nothing.

`Duration` is a lifetime in seconds: the pack uses it on the five temporary
mounts (7 days) and the two roses (3.5 days). A bought item carries the
resulting expiry, its tooltip counts down, and anything past its time is
swept on entry and once per second while online, including equipment and storage.
Transfers preserve expiry, dye, durability and added stats; stacks with different
properties stay separate. Store purchases are `Worthless` and `Locked`.
They are also `Bound` when paid with hunt gold or when the account's highest
character level is below 40. Bound items cannot be mailed, consigned or traded;
manual ground drops remain private to that account even after the usual share
timeout. Locked items cannot be dropped or sold; Ctrl+right-click an inventory
item to toggle its lock. Worthless items cannot be sold to NPCs even after
unlocking. Tooltips show these instance flags along with bonuses and expiry.
Flags persist through saves/transfers and distinguish otherwise identical stacks.
Client and server must both use protocol **5**; older saves default flags off.
Refinement readiness and the permanent NonRefinable flag remain separate.

The store is sent whole on entry rather than searched per keystroke, because
the table is static and small; the window filters and sorts its own copy.

`ZIRCON_DEV_MARKET=<n>` seeds `n` listings on an empty market so the board is
not blank in a screenshot; every fifth one is put on the first account, so a
dev login also sees the owner colouring and its own consignments.
`ZIRCON_DEV_GAMEGOLD=<n>` and `ZIRCON_DEV_HUNTGOLD=<n>` stock the two store
currencies, which nothing else grants yet, so the Buy button can be
exercised. `ZIRCON_OPEN=market`, `marketconsign` and `marketstore` open the
window on each tab, and on the Store tab `ZIRCON_STORE_TAG=Mount` and
`ZIRCON_STORE_HUNT=1` preset the filter and the currency.

Auto potion (`A`) drinks from a belt slot when a pool drops under its
percentage: health is checked first, then mana, and the drink goes through
the same path as pressing the belt key, so the server sees an ordinary item
use and the usual cooldown applies. It is client-side and off by default.
The drop filter marks ground items whose name contains one of up to five
words with a gold plate, so wanted drops stand out; it is client-side and
changes nothing about who may pick the item up.

The fortune checker (`K`, or the menu) is Zircon's `FortuneCheckerDialog`.
Every drop roll a player makes banks that roll's expected yield against the
account, whether or not the roll came in; once the expectation runs a whole
item ahead of what actually dropped, the next roll is forced through. So a
long dry streak on a rare item eventually pays out. Search the item
catalogue by name and spend one Fortune Checker item (`ItemEffect` 54,
present in this pack) to snapshot an item's numbers: how many have dropped,
how far the expectation has run ahead as a percentage, and how stale the
reading is. The progress itself is account state and is saved beside the
account's storage.

Chat options (`O`, or the menu) is Zircon's `ChatOptionsDialog`. The chat
tabs are ordinary state rather than a fixed table: add and remove tabs,
rename them, choose which message types each one shows, and set whether it
fades when idle or hides its button. Save writes `chat-tabs.json` beside the
client binary and Reload reads it back; a missing or unreadable file falls
back to the six defaults. Zircon also lists Observer, Gains and Alert
message types, which this client has no lines for, so those checkboxes are
absent rather than present and dead.

The HUD countdown is Zircon's `TimerDialog`: the server names each timer, so
sending the same key again refreshes it instead of stacking a second, and
the one expiring soonest is the one drawn, above the right end of the main
panel. Zircon's only caller is instance expiry; this pack has no instances
(its `InstanceInfo` table is empty), so the conquest window drives it. Two
asset gaps: the pack has no egg-timer frames (`GameInter` 6600+ and 961-965
are all absent), so the digits are drawn alone, and the art at 6590 -- the
separator index the C# uses -- is a CJK glyph in this pack rather than a
colon, so that is what shows between the pairs.

## Light and sound

The map is lit like Zircon's light layer: an offscreen target is cleared to
the map's darkness (`MapInfo.Light`: day cycle from the server's daylight,
night, twilight or full light; blood red while dead) and additive light
blobs are added for lit objects (torch light from equipment, a minimum
glow around your own character, NPCs, fire walls and other fields), then
the target is multiplied over the world before names and UI are drawn.
The Candle has light stat 15; the Bright Candle has 25. Equipping either
updates the client's light immediately.
The server broadcasts the daylight (`ZIRCON_DAY_CYCLE` game days per real
day, default 12).

Sound follows Zircon's `SoundIndex` tables, generated from the C# client by
`../tools/gen_sound_table.py` into `mir-client/src/sound_table.rs` (index to
file and channel, monster attack/struck/die by `MonsterImage`, magic
cast/travel/impact sounds, attack-skill sounds). The client plays login and
character-select music, each map's music on entry, footsteps on walk and
run frames of your own character, weapon swings by weapon shape, player
and monster struck and death sounds, spell casts with their projectile and
impact sounds, teleport and lotus effects, the fire wall and tempest hums
while one is within 20 cells, item cell sounds by item type, gold gained,
quest accepted and completed, and NPC dialog links. Files are looked up
case-insensitively under `Sound/`; missing files are silent (21 indices of
this asset pack have no file, see `research/sound.md`). Channels
(system, music, magic, monster, player) have their own volume;
`ZIRCON_VOLUME=0.5` scales all of them and `ZIRCON_MUTE=1` silences
everything (also used by the screenshot runs).

## NPC scripts

Dialog pages run Zircon's checks and actions: levels, class, gold, items,
random rolls, currencies by name (`CurrencyInfo`), named data lists and
values (`GameNPCList`, persisted in `npc_lists.json` under `--data`) and
rebirth at level 86 + rebirths, weapon level/element/added stats from
refining, horses, marriage and fame. `NPCRequirement` rows hide an NPC
from players who do not meet them.

Fame follows Zircon's `FameInfo`: an NPC page with the `CheckFame` check
and `PromoteFame` action sells the next title in order for Fame Points
(the `FP` currency, granted by NPC currency actions and quest rewards); a
held title adds its stats permanently, pays its item rewards once and
shows in the character window's "Fame" row.

Pages can be scripted in Lua as in Zircon's `NpcScriptEngine`: a page's
`ScriptFile` names a file under `<data dir>/scripts/npc/` (then
`$ZIRCON_SCRIPTS`, then the crate's `../scripts/npc/`, which holds
`example.lua`); `Script` checks and actions call the named function with
`(player, npc)`, and `on_open` may redirect (`npc.navigate("Page
description")`) or cancel (`npc.navigate("")`) the page. Scripts see
`player.name/level/gold/has_item(name, count)` and can `npc.give_gold`,
`take_gold`, `give_item`, `take_item`, `message`; they run sandboxed
(no io/os) and errors fail the check. See `research/fame-lua.md`.

## Character services and discipline

`D` opens character services; right-clicking a Hair Change, Gender Change,
Armour Dye or Name Change item in the bag also opens it. Each successful
change consumes one matching usable item. Hair styles and colours are
validated by class and gender; armour must be removed before changing
gender, and assassins cannot dye armour. Dyes use the original armour
overlay and hair sprites. Renames take effect in account lookups immediately;
all changes persist with the character and items.

The same window shows the discipline level, focus pool and requirements for
the next level. Training checks level, gold and accumulated discipline
experience, charges gold, and grants an eligible discipline-school magic
when one exists in the data. Focus regenerates alongside mana. This asset
pack has four discipline levels and no discipline-school magics.
`ZIRCON_OPEN=characterservices` opens the window for a screenshot;
`ZIRCON_DEV_DISCIPLINE=1` on the server grants the first discipline level.

## Harvesting

Chickens and the other harvesting AI classes keep their rolled loot on the
corpse. Stand next to it and hold left mouse over the body, or press `B` to
harvest nearby. Chickens take three swings; deer and pigs take four. Loot
belongs to the account that earned it; a full bag leaves the uncollected
items on the corpse for a later attempt. Group members retain their own
shares. After harvesting, viewers with no share left see bones, including
when they leave and re-enter view. Loot-bearing corpses remain for five
extra minutes, ending when all shares have been collected.
Server-side `ZIRCON_DEV_HARVEST=1` creates a loot-bearing chicken corpse
beside the player; client-side `ZIRCON_AUTO_HARVEST=1` walks to a nearby
corpse and harvests it for visual checks. See `research/harvesting.md` for the C# rules.

## Equipment sets

Wearing every piece of a set (Zircon `SetInfo`) grants its bonus stats. A
set's membership is not stored as a list: it is every `ItemInfo` whose `Set`
column names it, so the 30 sets in this pack are counted from the item side.
Each bonus line (`SetInfoStat`) still has to clear two gates of its own, a
minimum character level and a class mask, which is why a set can be complete
and some of its lines still dark.

Three details come straight from `PlayerObject.RefreshStats`: a broken item
(zero durability, on an item that has durability) counts for nothing, so it
neither adds its own stats nor fills its slot in the set; two copies of the
same piece count once; and a set pays out only when the pieces worn equal the
pieces that exist, never partially.

The character window lists every set the player has a piece of, with its
progress and its lines coloured green when they apply and grey when they do
not, and the item tooltip shows the same for the set an item belongs to.

`ZIRCON_DEV_ITEMS="Gale Ring;Gale Necklace"` with `ZIRCON_DEV_LEVEL=20` wears
a complete two-piece set for a screenshot; dropping one of the two shows the
incomplete state.

Visit the **Weapon Smith** in Lost Land and choose **Craft Weapon**. Open
inventory and right-click a Weapon Template (or an unequipped weapon), then
up to six colour ingredients. Click a linked slot to remove it, choose the
output/stat class, and press **Craft**. Templates cost 1,000,000 gold;
rerolling common/superior/elite weapons costs 30/60/80 million gold.
Each ingredient's shape determines how many weighted stat rolls it grants.
Zero ingredients are allowed and grant no new bonuses. Rerolling preserves
enhancement bonuses, replaces other added bonuses and sets weapon level to 1. Identity, durability,
dye and expiry survive rerolling. Spirit Blades cannot be used.

The server validates the open NPC, distance, item identities, distinct slots,
colours, capacity and gold before consuming anything. Protocol version **4**
requires rebuilding client and server together. Bonuses retain their source
(Added, Refine, Enhancement, Other or legacy unknown), including when multiple
sources affect the same stat. Rerolling keeps Enhancement entries, clears
refinement readiness and preserves the separate permanent NonRefinable flag.
A rerolled eligible weapon can earn readiness again through combat.

Old saves containing `[stat, amount]` pairs load as legacy bonuses without
changing their values; their unknown provenance cannot be reconstructed.
Old saves lacking readiness fields retain their previous permission to refine.
New saves preserve sources, both refinement flags and fractional weapon
experience through inventory transfers, storage, mail and market persistence.

For a populated screenshot, `ZIRCON_CRAFT_PREVIEW=1` selects a template and
colour ingredients already in inventory when the crafting page first opens;
it does not submit a craft. Use `ZIRCON_CRAFT_PREVIEW=weapon` to select an
existing weapon instead. Combine with `ZIRCON_AUTO_NPC="Weapon Smith"`
and `ZIRCON_AUTO_NPC_BUTTON=1` while near the smith. See "Where the rules come
from" for the reference implementation.

## Quests

NPC dialogs list the quests an NPC starts (`[Accept]`) or finishes
(`[Complete]`, or "in progress"); the quest log opens with `J` and shows
the progress text with Zircon's name tags filled in, each task's count and
the class-filtered rewards. Requirements (level, class, other quests
completed or not), kill and gather tasks (`QuestTaskMonsterDetails` with map
filter and chance roll, capped at the task amount), region tasks, choice
rewards, experience and currency rewards, daily/weekly/repeatable resets
and abandon follow the C# rules; gather tasks credit on the kill instead of
dropping a quest item to pick up.

## Monster AI

`MonsterInfo.AI` selects a behaviour profile (`world/ai_profile.rs`, ported
from Zircon's `MonsterRegistrations`): town guards (immobile, untouchable,
kill wild monsters on sight), passive farm animals and trees, ranged
spitters and archers (with kiting and self-scare), ray-limited line
attacks, splash and self-area hits, caster classes (Thunder Bolt, Fire Ball,
mass lightning, beams, storms and the Sama guardian kits), poison-on-hit
tables (green, red, paralysis, silence, abyss), blink-strikers with a
one-time panic teleport, burrowers and ambushers that stay invisible until
prey is close, suicide larvae, corpse spawners and boss summon phases.
Elemental monster hits ignore dodge and go through MR; monsters that lose
their target re-search every half second like Zircon does every tick.

Wave two adds the timed class behaviours the first pass skipped: Voracious
Ghosts revive up to three times with half their HP each time and only drop
loot at the last death, Crimson Necromancers strip the magic resistance of
everything within three cells every ten seconds, Jinchon Devils lay death
clouds around their targets that burst after a few seconds, and the
Netherworld and Jinam gates stand invulnerable for twenty minutes, sweeping
players in view to the region named by `ZIRCON_MYSTERY_SHIP_REGION` /
`ZIRCON_LAIR_REGION` (Zircon's `Config` region indices; unset = no
teleport). Wave three covers the odd bosses: Shinsu fights only inside ten
second windows and is unseen between them, Terracotta warriors rush in
invisible and show themselves two cells from their target (the sub boss
fades out again when far), and Doom Claw stands still while its claws,
maw and wave strike whoever stands in each part's reach with class-reduced
damage and pushes, ignoring attackers more than ten cells away.
Wave four makes monster fire walls and poison clouds real fields (a cross
of five walls burning for 30 s, a 5x5 cloud for 20 s), gives Banyo
Captain and Banyo Lord Guzak Zircon's Purification (stripping a player's
helpful buffs, always when the target is no higher level and otherwise 42
% of the time), lets Healer Ants tend injured allies with a ticking heal,
scatters and curses everyone in view around Frost Lord Hwa every minute,
scatters the whole map around the Queen Of Dawn, and honours
`MonsterInfo.Behaviours` flags (Heals every 30 s, Teleports and Enrages
once at a quarter HP) when the pack sets them. `cargo test -p mir-server
report_monster_ai -- --ignored --nocapture` prints which AI ids still fall
back to the plain melee profile.

