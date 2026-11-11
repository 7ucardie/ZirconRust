# Content editor

The `mir-editor` crate. Back to the [README](../README.md).

## Editing content

`cargo run -p mir-editor` opens the data editor (egui): every `System.db`
collection as a sortable, filterable table with typed cells; a detail form for
the selected row (complex values as text: stats `2:10;8:3`, arrays `1,2,3`,
points `x,y`); item, magic, NPC and monster sprite previews; links from
foreign keys (Item, Monster, Map, Region, Page...) and a list of every record
that points back at the selected one (drops, stats, shop goods). Add,
Duplicate, Delete, Undo (Cmd+Z), Save (Cmd+S). Validate runs the server's
own loader plus consistency checks (dangling monster/item/region references,
spawn regions with no walkable cell, books without a magic) on the unsaved
data and links each finding to its row. Saving writes a
`System.db.bak-<timestamp>` copy first; a running `mir-server` notices the
changed file within 2 s and reloads it (SIGHUP forces a reload on unix), so
an edited potion heals for the new amount on its next use without a restart.
`--db FILE` edits another database; `ZIRCON_SCREENSHOT`,
`ZIRCON_EDITOR_COLLECTION`, `ZIRCON_EDITOR_FILTER`, `ZIRCON_EDITOR_ROW` and
`ZIRCON_EDITOR_VALIDATE` automate a visual check.

The **Regions** tab adds a walkability overview with region overlays. Search for
a map by name, file or index; use Fit, Zoom and scroll to navigate. Select a
region from the list or click its cells (small overlapping regions take
priority). **New region** creates a named empty region; **Add rectangle**
adds only walkable cells and **Erase rectangle** removes cells. Each gesture
is one undo step. The panel lists shared-region uses and links to their rows.

Choose a monster, count and delay to add a spawn group; **Make safe zone**
uses the selected region for both its area and bind area. These changes use
the existing Save/Undo/Validate workflow. Restart the server to apply new
placements or changed region shapes on maps already loaded into memory;
existing hot reload does not rebuild those placement caches. Tile painting is
available separately in the **Tiles** tab.

`ZIRCON_EDITOR_MAP=605 ZIRCON_EDITOR_REGION=1711` opens the Lost Land overview
with the Weapon Smith region selected, useful with `ZIRCON_SCREENSHOT`.
Always pass `--db /path/to/copy/System.db` when experimenting on a copy.

**Procedural cave drafts:** in Tiles, open a cave source map (for example
`D102.map` from the shipped pack), set an even size between 32 and 256, then
expand **Generate cave draft**, choose a seed and generate. Select the monster
for eight spawns. The editor shows the safe zone and paired portal locations;
**Test walk** runs the cave and its content in an isolated server. Validation
rejects disconnected floor and blocked/unreachable placements before map save,
export or testing.

**Export map + database snapshot** writes to a new folder whose parent already
exists. It includes `Map/GeneratedCave.map` (or a unique suffixed name), a
`.cave.json` placement sidecar, `System.db`, and launch instructions. This is a
full database snapshot, not a database overlay. Keep the sidecar beside the map
to restore generator placements when reopening it. Save map writes the map and
sidecar; export is needed to write the corresponding database records. To view
bundle regions, open its database with `ZIRCON_MAP_DIR=<bundle>/Map`.

**Organic cave** samples intact floor patches, then extracts complete wall/rock
formations and decorations from the source map. It groups touching sprites,
includes all animation frames, and copies their full layers and collision.
Oversized structures and ambiguous tall strips are skipped. Placements that
cut off a passage are rolled back; larger formations get priority over small
decorations. A seed reproduces the same result with the same source and assets.
Exposed blocked cells receive matching Wood wall art in a private 258-frame
library; the source formations remain intact. Save keeps a `.wood.zl` sidecar,
and export includes the private `Client` library required by the native game.

For a complete authored perimeter, choose **Enclosed chamber** instead. Open a
compact closed source such as `D1006.map`, click **Fit source artwork**, then
generate. This mode preserves the entire chamber and all frame extents, including
walls extending beyond their anchor cells. It refuses disconnected sources,
walkable map edges, and undersized canvases. The seed varies placement within
the canvas; it does not invent a new chamber outline. Spawns, the safe zone and
paired portals are generated inside the preserved chamber. The mode is retained
in the placement sidecar; older sidecars still load as organic drafts.

**Rooms + corridors** creates seeded dungeons with 2–12 separate rooms
and passages 2, 4 or 6 cells wide. It partitions the canvas, connects every room,
then reuses source floor patches and complete decorations. Art placement cannot
block corridor cells or room centres. Blue outlines label rooms in the editor.
Room bounds, passage cells, width and seed persist in the sidecar; painting a
passage blocked prevents save, export and Test walk until repaired. If a small
canvas cannot fit the requested room count, enlarge it or request fewer rooms.
Exposed blocked cells receive Wood wall art, and passable room-to-corridor
crossings receive threshold art. Save, reopen, export, and Test walk carry the
private library with the map.

Generated maps now have a collision-based minimap and large-map overview.
Their exits show pulsing blue teleport beacons in the world and cyan markers
in both map windows. Export, Save map and Test walk write a `.navigation.json`
sidecar alongside the map. Keep it with the map when distributing a bundle;
older bundles gain markers by reopening and saving/exporting in Tiles. Invalid
or stale marker metadata is ignored with a client log warning. The server's
movement records still control travel; regenerate metadata when changing exits.
The beacons reuse the shipped teleport animation through the existing effect
renderer. Illustrated minimap art remains open. The game renderer is unchanged.
For visual checks,
`ZIRCON_TILE_ZOOM=0.35` shows an overview; `ZIRCON_TILE_CENTER=x,y` overrides the
test position on reopened caves.

**Monster stat drafts:** in Tables, select a `MonsterInfo` row and expand
**Generate monster stat draft** below its sprite preview. Enter a new name and
target level, then **Preview stats** to inspect the proposed combat stats and
experience. **Create new monster** adds a new monster and its stat rows; one
Undo removes both. Save the database, then select the new monster in Regions
or in a generated map's spawn controls. Drafts retain the source appearance,
AI and movement/attack timing. Configure drops separately and playtest the
estimates before using them in progression.
The estimator groups references by AI and boss status, interpolates per-level
medians where samples bracket the requested levels, and labels fields using a
level-ratio fallback. Scaling is capped between 1/16 and 16; min/max pairs share
a factor. A changed database invalidates the preview and requires previewing
again before creation.

**Equipment drafts:** in ItemInfo, expand **Draft equipment variant**. Choose
a name and target minimum level, then preview the core stat budget and individual
changes before creating. Comparable items share slot, class requirement and
rarity. The estimator interpolates observed median budgets when possible, or
labels its bounded level-ratio fallback (¼–4). It preserves artwork, price,
durability, special/negative stats and restrictions. Budgets are comparison
estimates, not measured combat power. Minimum-level weapons, armour and combat
accessories are supported; other requirement types and ambiguous duplicate
source stats are rejected. Creation allocates new item/stat IDs atomically;
stale previews fail, and one Undo restores both tables even after saving. New
variants disable automatic starting-item grants.
The new item can then be selected in loot or shop goods.
`ZIRCON_EDITOR_ITEM_DRAFT=1` previews the selected source automatically for QA;
`ZIRCON_EDITOR_ITEM_LEVEL` chooses the target (default 30).

**Monster sprite variants:** expand **Sprite variant export** in MonsterInfo.
Preview any of the shape's 1000 animation slots, multiply RGB colour and resize
50–200%, then export to a new folder outside the original assets. The complete
`.Zl` library is written with other shapes retained byte-for-byte; the selected
shape preserves empty slots, shadow/overlay surfaces and scaled anchor offsets.
DXT re-encoding is lossy. Use the exported library in a separate asset pack.
This replaces an existing Image mapping, so every monster sharing that image
uses the variant; allocation of independent image IDs is still future work.
`ZIRCON_EDITOR_SPRITE_VARIANT=1` opens the panel for screenshots.

**Drop templates:** select a monster with no drops and expand **Apply drop
template** in its details. Donors are ranked by boss status, nearby level and
AI. Choose a donor, preview its items, base one-in odds and randomized quantity
ranges. Select individual rows or use the Consumables, Equipment, Materials
and Other category buttons, then **Apply selected drops**. The tool copies
only those rows in their authored order without scaling their
odds, preserves the donor, and supports one-step Undo. Save the database before
testing or exporting. It refuses an existing target loot table and stale
previews. DropSet and EasterEvent conditions are preserved and shown in the
preview. Disabled and PartOnly rows are
identified in the preview; PartOnly rows do not produce ordinary loot. Group
sharing and account drop progress can affect the actual yield.

**Multi-donor loot drafts:** use **Add selected to loot draft**, switch donors,
and add more selections for the same target. Review donor names, quantity
ranges, odds and conditions in the queue; **Remove** drops individual entries.
**Apply queued loot** writes the whole recipe atomically with one Undo, including
after saving. Repeated items from different source rows remain separate rolls;
the same source row cannot be queued twice. Database edits invalidate a queued
draft. **Discard queued loot** starts over. The queue stays in memory until
applied and is not included in database saves; closing warns about pending
entries, and reload requires applying or discarding them first.

**Tier + biome materials:** choose the tier donor in the main drop-template
picker, then expand **Tier + biome materials** and choose a material donor.
Only monsters with material drops appear in that picker, with material-row
counts. **Draft tier + materials** queues the tier donor's non-material rows
followed by the material donor's material rows, retaining order within each
group. Review/remove rows before applying. The same donor can fill both roles
without duplicating rows. Donor ranking suggests nearby levels and matching
boss status; choose biome membership yourself. Authored odds and conditions
stay unchanged, so this is a starting template, not automatic reward balancing.
Apply or discard an existing queue before drafting another.

**Base loot yield comparison:** selected donor rows and queued recipes show
average item units per 100 independent base rolls, grouped by item ID. Ungated
rows and extra mask/event-gated yield occupy separate columns. The latter
assumes every gate is satisfied; it is not an event-frequency prediction.
Disabled and PartOnly rows contribute no units. Repeated-item expectations add
for display without merging the underlying rolls. Quantity means match the
server's randomized amount rule, including its minimum-one clamp. Account drop
progress, group sharing and harvest timing affect actual results; these figures
are a comparison aid, not guaranteed rewards or an automatic balance score.

**Conditional loot:** a spawn's `RespawnInfo.DropSet` must contain every bit
required by the drop row's `DropSet`. A zero requirement permits every spawn.
Easter rows require a monster marked at spawn: set `EasterEventChance` to a
positive one-in chance and start the server with `ZIRCON_EASTER_EVENT_END`
set to a future UTC Unix timestamp in seconds. Unset, invalid or expired
deadlines disable marking; chance 0 also disables it. Eligibility is fixed
when each monster spawns, so marked monsters retain it after the event ends.
Generated maps expose these values under **Spawn loot conditions** and retain
them through save, export and Test walk. Test walk inherits the event deadline
from the editor environment. Reloaded spawn settings affect future spawns.

The **Tiles** tab edits `.map` files. Open a map or make a blank one, select
Back/Middle/Front, choose a thumbnail, then use Pencil or Rectangle. Erase
changes only that layer. Pick tile copies an existing selection; Copy area
and Stamp copy all cell properties, aligned to the back layer's 2×2 grid.
Collision and Light have their own brushes. Animation frame/blend settings
are stored with middle/front tiles. Grid and blocked-cell overlays help
check placement. Learn neighbour patterns from the current map to enable a
local adjacency-matching brush using tiles already observed in that library.

**Reusable wall pieces** saves copied areas as named Wall, Corner, Doorway or
Formation files. Copy an area, choose **Use copied area**, then **Save new file**
to a new `.json` path. Load it on another map and select **Place piece / run**.
Pieces retain all layers, animation, light, collision and opaque cell data;
the file references your existing asset libraries. Even dimensions of 2–64 cells
per side preserve the back grid. Existing piece files are never overwritten.

Set **Repeats** to build a wall run with an even X/Y step. The complete run must
fit, including tall artwork; conflicting overlaps and invalid generated-map
placements are refused before editing. One Undo restores the entire run.
Selections and joins are authored manually: a Doorway label does not create an
opening. Include surrounding ground/collision and inspect joins in Test Walk.
The **Wood wall perimeter** generator below assembles a checked source-specific
wall ring. **Wood connected rooms** adds two rooms joined by a physical floor
passage. Arbitrary perimeter synthesis and movable door objects remain separate
work.

A checked source example is D1006 cells `(10,8)` through `(11,17)` inclusive:
a complete northwest wall strip. Two repeats with step `(2,-2)` reproduce the
adjoining wall positions without changing the source collision mask.
`ZIRCON_WALL_PIECE=/path/to/piece.json` opens the panel with a saved piece;
`ZIRCON_WALL_REPEATS=2` previews repeat controls without placing anything.

**Connections** adds explicit walkable openings to a piece. Choose North, East,
South or West, an offset counted from the top/left, an inward inset, an opening
width and a compatibility profile. Inset zero selects the outer edge; larger
values locate a recessed edge inside an irregular assembly. The sketch marks floor green and openings gold. Validation
requires covered, walkable edge and inward cells, uncovered space immediately
outside, and a route between all declared openings.
This records an existing opening; it does not cut a doorway into blocked cells.

Under **Join another piece**, load a second file, select one connection on each
piece and choose **Build joined piece**. Opposite directions, equal widths and
exactly matching profiles are required. The editor translates the second piece
without rotating artwork and refuses offsets that misalign the back tiles. The
result becomes a new piece to inspect, place and save; the open map and source
files are unchanged until you explicitly place/save it. Joined connections are
consumed; unused connections retain their positions, including recessed edges.
A later piece can fill that recess. Covered-cell overlaps and joins that seal
an unused opening are refused; empty padding may overlap.
Combined pieces are limited to 64 cells per side. Matching profiles express
an author's compatibility choice, not an automatic visual seam guarantee.

Offset joins keep uncovered padding transparent during placement, including
after save/load. Coverage always preserves complete 2x2 back tiles.

Piece files now save as version 3 with connection insets and coverage. Version 1
files still load without connections; version 2 connections default to inset zero. `ZIRCON_JOIN_PIECE=/path/to/other.json` preloads
the adjoining file for visual checks. Existing files are never overwritten.

**Piece kit browser** loads checked Wood north/south kits or a separate palace
doorway. Choose **Use piece**, then **Find compatible joins** to discover valid
connections across the loaded pieces. Each result passes the full composition
checks; **Build** checks it again before making the joined piece available for
placement. Searches run on request, with at most 16 source pieces and 256 matching
connection pairs. Specific profiles distinguish Wood NW, Wood NE and palace
doorway connections; matching profiles still require visual review in Test Walk.

The five-piece Wood north kit contains D1006's convex north corner, its two
approaches, an authored NE bend and terminal wall body. It assembles to 14x14
cells at source position (14,2). The three-piece Wood south kit retains the
south corner and adjoining walls with their complete tall decoration; it
assembles to 14x24 cells at (20,26), including the complete circular floor
motif and neighboring animated decoration. Both restore the original source map
byte-for-byte when placed there. Distinct profiles constrain the authored NE
bend sequence; it is not a repeatable diagonal wall strip. The north terminal
body is the last selected wall segment, not a physical wall endcap. The south
decorated pieces preserve one authored group when assembled.
The palace doorway comes from `0_000` and retains
both wall ends, the carpeted threshold and neighboring tall artwork. They are
separate themes, not a complete interchangeable wall/corner/doorway kit.
Built-in captures check source fingerprints and reject selections that cut any
sprite, including artwork anchored outside the selected cells.

Under **Kit folder**, load direct `.json` children or save the current kit to a
new folder. Loading uses filename order, rejects any malformed piece, and limits
the directory to 128 entries. Export validates all pieces before creating the
folder, writes fixed `piece-01.json` names, and never overwrites existing paths.
Files reference installed artwork; they do not copy asset libraries. An I/O
failure can leave a partial new folder, which is reported explicitly.
**Add current piece to kit** stores an independent copy with the edited name,
type and connections, up to 16 entries. **Remove** drops a catalogue entry;
**↑** and **↓** reorder entries. These changes invalidate earlier join results.
Export preserves the displayed order. Save to a new folder to retain kit edits.

**Assemble whole kit** starts with the first catalogue entry and uses each entry
once. Each step must have exactly one legal next join across the unused entries;
ambiguous or disconnected kits fail without replacing the current piece. The
whole operation allows at most 256 discovery trials plus 15 chosen-join
revalidations. Success produces a new piece ready for placement; the
map changes only when you place it. Reorder the catalogue to choose its starting
piece, or use manual joins when the next connection is ambiguous. Remaining
connections may stay open; successful assembly does not establish a perimeter.
`ZIRCON_BUILTIN_KIT=wood-corner`, `wood-south-corner` or `palace-doorway`
preloads a built-in kit; `wood-corner` keeps its existing name for the north kit.
`ZIRCON_PIECE_CATALOGUE=/path/to/folder` preloads a saved kit instead.
Add `ZIRCON_ASSEMBLE_KIT=1` to assemble the loaded built-in or folder catalogue
on startup; combining it with `ZIRCON_WALL_PIECE` is an error because both select
the current piece.
The browser also loads **Wood entry**, **Wood wall bodies** and **Wood perimeter
corners**, plus the **Wood wall junction**; automation names are `wood-entry`,
`wood-wall-bodies`, `wood-perimeter-corners` and `wood-wall-junction`. The D10032 entry is a complete room/corridor junction
with shaped ownership, not a compact doorway sprite. Its separate profiles do
not yet connect it automatically into the generated ring. The D1008 wall junction
preserves a real physical passage in a 20×32 shaped group. Four checked wall
seams connect it to the diagonal bodies; all four need surrounding geometry.
It is available for authoring and is not automatically inserted into the ring.

**Wood wall perimeter** in the cave generator assembles four checked D1008
corners and repeated diagonal wall bodies into a new closed room. Choose NW and
NE repeat counts from 3–10; the recipe derives SE as NW+2 and SW as NE, closes
all seams simultaneously, and fills unowned interior blocks with source floor.
Exterior cells remain blocked. All 64 supported count combinations pass the
source-backed generation checks. This differs from **Enclosed chamber**, which
copies a complete authored source chamber. `ZIRCON_CAVE_MODE=wood-perimeter`
selects the new mode on startup.

**Wood connected rooms** uses six convex and two concave corners to enclose
two rooms and a real 6×4 floor passage between them. NW and NE counts each
accept 3–6; all 16 combinations pass source-backed enclosure, connectivity and
passage checks. `ZIRCON_CAVE_MODE=wood-connected` selects this mode. The passage
works independently of teleport triggers; this recipe does not insert a movable
door object or the separate authored junction group.

Saved `wall_repeats` and `wall_rooms` metadata restore the mode and counts.
Enclosed-layout validation rejects any walkable map-border cell, including a
connected breach introduced by later collision edits. Arbitrary BSP and organic
perimeter art still need separate recipes.

The local stdio [MCP authoring](MCP_AUTHORING.md) server supports map,
database, item, monster, and sprite tools. Its `create_world` MCP prompt accepts
a natural-language brief and guides an MCP host's AI through the tool calls;
the `world_from_brief` tool directly creates a simple chamber, organic cave, or
2–12-room Wood dungeon with one named installed monster. The server does not
include a separate language model. `world_generate` gives
organic and room dungeons full Wood wall art, while `world_branch` assembles a
bounded graph of up to 12 rooms. The sprite editor and `sprite_auto_motion`
can design cloth motion from one drawn polygon. `sprite_pose_from_reference`
transfers complete hand-drawn body poses from an original armour appearance. See
[SPRITE_AUTHORING.md](SPRITE_AUTHORING.md) for its scope and examples.
Equipped weapon, helmet and shield art now has editable family projects;
`equipment_publish` allocates a new private Shape. The `design_sprite_motion`
MCP prompt guides a host AI through drawing and importing original pose frames.
Every required direction needs visual review before publication.

Map undo/save are separate from database undo/save; Cmd+S/Cmd+Z act on the
selected workspace. Maps save with backups and refuse to overwrite a file
changed externally since opening it. Opening another map requires saving or
discarding edits, and closing warns about unsaved map or database changes.
**Register saved map in database** adds a MapInfo record; save the database in
Tables and keep the map file in the server's map directory.

Right-click a walkable cell, then **Test walk in game**. It launches the built
client/server beside the editor against temporary copies of the current map
and database, with disposable saves. Both the server and client catalogue use
that database snapshot, so new items and changed tooltips appear immediately. It uses daylight so edits are visible;
the game supplies the final lighting and screen blending. Test sessions stop
when the editor closes; Stop test game ends one earlier. Logs/snapshots stay
in the displayed temporary folder. Build the workspace first.

For visual automation, use `ZIRCON_EDITOR_TILE_MAP=/path/to/copy.map` and
`ZIRCON_TILE_CENTER=164,223`. `ZIRCON_EDITOR_TEST_WALK=1` starts the test session;
`ZIRCON_TILE_TEST_SCREENSHOT=/tmp/game.png:5` captures its client. Set the
editor's own `ZIRCON_SCREENSHOT` delay longer than the game screenshot delay.
The launcher uses `ZIRCON_MAP_DIR` to give client and server an isolated Map
folder while reusing the original sprite/sound assets.

Underneath, `mir-formats` writes as well as reads: `MirDb::save`,
`MapFile::save` and `ZlBuilder::save` re-encode `System.db`, `.map` and `.Zl`
files (the shipped files round-trip byte for byte in the tests) and leave a
`.bak-<timestamp>` copy of the previous file. New sprites go in through
`ZlBuilder::push_rgba` (DXT1/DXT5 encoding). The rest of the editor plan is in
PLANS.md.

