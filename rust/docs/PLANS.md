# Plans

The design plans used to build the editor, sprite authoring and generated rooms, merged into one document. Delivered behaviour is in [EDITOR](EDITOR.md) and [SPRITE_AUTHORING](SPRITE_AUTHORING.md).

## 1. Editor and content generation


Status: Phase 1 (writers) landed 2026-09-12 in `mir-formats`; phase 2's
generic data editor (`mir-editor`: tables, form, previews, relations, undo,
save with backup, Validate = server loader + consistency checks) landed the
same day, and `mir-server` hot-reloads `System.db` when the file changes or
on SIGHUP. The remaining phase-2 map/region/spawn panel landed 2026-09-14:
walkability overview, region overlays, rectangle placement/erasure, spawn
and safe-zone creation, links to existing NPCs/movements, undo and validation.
Phase 3 core tile editing landed 2026-09-14 (details below). Phase 4 now has
a seeded cave-draft generator, connectivity/content validation and playable
bundle export. Whole source formations and decorations are now reused;
an enclosed-chamber mode preserves complete authored perimeters, and seeded
room-and-corridor drafts preserve passage clearance. Freeform boundary-wall
synthesis, WFC and AI generators remain open.

### What exists today

| Tool | Where | What it edits |
|---|---|---|
| Server data views | C# `Server/Views/*View.cs` (WinForms grids over `System.db`) | items, item stats, monsters, monster stats, drops, maps, regions, respawns, safe zones, movements, NPCs and pages, magics, base stats, sets, stores, quests, events... |
| Map viewer | C# `Server/Views/MapViewer.cs` | view a map with regions/spawns (no tile painting) |
| Library editor | C# `LibraryEditor/` | import/export images in `.Zl` sprite libraries (DXT1/DXT5), offsets, shadows |
| Rust formats | `mir-formats` | read/write `.Zl`, `.map`, `System.db`; backup on save |
| Rust editor | `mir-editor` | database tables/forms, previews, map-region overview, spawn and safe-zone placement |

The Rust editor edits game content in `System.db` and tiles/collision/light in
`.map` files. Cave terrain drafts can also be generated and tested;
sprite-library authoring and the remaining content generators are later stages. The C# tools remain the reference implementation.

### Goal

A single desktop tool, `mir-editor`, that edits everything the Rust game
consumes: `System.db` records, `.map` tiles/flags, `.Zl` sprites, and can
generate new content (maps, monsters, items) with procedural generators and
AI assistance, always validating against the game's rules before saving.

### Phase 1: writers (foundation) — done

Landed: `MirDb::to_bytes/save` (byte-identical on the shipped `System.db`),
`Collection::set/push_record`, `MapFile::to_bytes/save/blank/cell_mut` with
raw-byte retention (all 248 shipped maps byte-identical), `dxt::encode`
(DXT1 with 1-bit alpha, DXT5) and `ZlBuilder` (raw copy of existing images
plus `push_rgba` import; small shipped libraries byte-identical), and
`save_with_backup` writing `<file>.bak-<unix seconds>` before every save.

Original plan:

1. `mir-formats::mirdb` write support. `System.db` is self-describing (type
   mapping, then per-collection `next_index`, `count`, and raw records), so a
   round-trip writer only needs to re-encode `Value`s. Acceptance: load and
   save the real database byte-identical; unit test on every collection.
2. `mir-formats::map` writer: header, back layer (`(w/2)*(h/2)*3` bytes),
   14-byte cells. Acceptance: round-trip all shipped maps.
3. `mir-formats::zl` writer: DXT1/DXT5 encode (crate `texpresso` or
   `intel_tex_2`), 25-byte image headers, shadow/overlay surfaces.
   Acceptance: re-encode `StoreItems.Zl`, decode, compare with a tolerance.
4. Backups: every save writes `<file>.bak-<timestamp>` first.

### Phase 2: data editor (`mir-editor` crate, egui/eframe)

- Generic table editor built from the MirDB mapping (like the C# grids): sort,
  filter, add, delete, edit cells with typed widgets; foreign keys resolved by
  name (Item -> Monster drops, Map -> Regions -> Spawns/Safe zones).
- Specialised panels:
  - Items: icon preview from `StoreItems.Zl`, stats sub-table, class/level
    requirements, price and weight, book -> magic link.
  - Monsters: sprite preview (all directions, animation loop), stats, AI type,
    drop table with expected value per kill, respawn groups.
  - Magics: powers/costs per level, icon preview, need levels.
  - NPCs: pages, checks, actions, goods with prices; dialog preview text.
  - Maps: list with region overlay, spawn and safe zone placement.
- Live check: the tool runs the same `GameData::load` as the server and
  reports errors (dangling references, unwalkable spawn cells).
- Hot reload: `mir-server --reload` (SIGHUP or admin command) re-reads
  `System.db` so edits show up without restarting.

#### Map/region panel delivered 2026-09-14

- Maps tab: searchable map selector, fit/zoom/pan, walkability from `MapFile`,
  red spawn regions, green safe zones, blue other regions and yellow selection.
- Create named regions; click or drag rectangles to add walkable cells or
  erase cells. Existing bit regions decode with the server's coordinate order;
  edits write point regions and remove the old bit representation.
- Select a monster, count and delay to create a spawn on the selected region;
  make a safe zone with the same region as its bind area. Existing linked
  records open in Tables for editing/removal. Shared-region uses are listed.
- Each gesture/creation is one undo step. Save retains backups; validation
  runs on unsaved data. New/duplicate IDs avoid collisions with shipped rows.
- New placements and changed shapes on already loaded maps require a server
  restart: existing hot reload updates data and existing spawn parameters,
  but does not rebuild map placement caches. Tile artwork is not edited here.

### Phase 3: map editor — core delivered

The **Tiles** workspace now provides:

- Existing-map selection and new blank maps; separate map and database saves.
  Saved maps can be registered as `MapInfo` rows, then saved from Tables.
- Thumbnail palettes for all 14 base libraries plus Forest/Sand/Snow/Wood
  variants, using the same library mapping as the client.
- Back/middle/front painting, erasure, pencil gestures, rectangle fill,
  eyedropper and whole-cell copy/stamp. Back tiles and stamps align to the
  format's 2×2 anchors. Stamps include animation, flags, light and padding.
- Animation frame count/blend settings, collision and light brushes, grid and
  blocked-cell overlays. The collision brush uses the server's exact two-bit
  walkability rule and preserves the other bits.
- Optional local neighbour matching learned from tiles in the open map and
  selected library. Choices are scored against observed cardinal adjacencies;
  this is a local brush, not a global map generator.
- Gesture undo, dirty-state tracking, backup-on-save, rejection of external
  file conflicts, and an unsaved-changes close dialog. Backup names now use
  nanoseconds so rapid saves do not replace a previous same-second backup.
- **Test walk in game** snapshots the current map and database, creates a
  temporary map definition and saves directory, and launches a local server
  and client. Right-click selects the start cell. Closing the editor stops
  its child processes; snapshots and logs remain in the temporary directory.

The editor preview is unlit; use Test walk to inspect the game's lighting and
screen blending. No game-renderer behavior was changed. Dedicated door/flight
properties are not inferred from the format's undocumented bytes, which remain
intact. NPCs and other region-linked records still use Regions/Tables.

Original phase outline:


- Tile painting with the 14 map libraries (`Data/Map Data/*.Zl`): back layer,
  middle, front, object animation, light, door and walk/fly flags.
- Brushes: single tile, rectangle fill, stamp (copy a region), autotile sets
  built by clustering tiles that appear adjacent in shipped maps.
- Walkability preview using the exact rule the server uses.
- Region tools: paint bit regions for spawns, safe zones and movements; place
  NPCs; test-walk from inside the editor by launching the client on the map.

### Phase 4: content generation

#### Maps (procedural first, AI for intent)

Landed 2026-09-14: **Tiles → Generate cave draft** accepts an even size
32–256 and a seed. Cellular automata retain the largest connected component;
intact, undecorated 2×2 floor patches come from the open source map. BFS checks
all walkable cells and every placement, with at least 64 free cells and eight
spawn cells. The draft includes a safe arrival region, eight selected monsters,
and two portals joining opposite ends of the cave. Separate arrival cells
prevent the server's recursive travel logic from looping.

Export writes a new folder containing a `.map`, `.cave.json` editor placements,
and a complete `System.db` snapshot. It neither merges an overlay nor updates
the active database. Reopening the map restores its editor placements; Test
walk includes them in its disposable server snapshot. Export refuses existing
folders and allocates a fresh map filename if necessary. The Regions workspace
honours `ZIRCON_MAP_DIR` when inspecting a bundle.

The next art slice landed 2026-09-14: source sprites are grouped by their full
rendered bounds (including all animation frames), and capture rectangles expand
to include intersecting artwork. Whole templates up to 32×32 cells preserve
all layers and source collision. Large or ambiguous narrow-strip structures
are skipped. Placement prioritizes large formations, checks shape agreement,
prevents overlaps, and rolls back any change that disconnects the floor.
Spawns and portal positions are derived after art placement. The shipped D102
source at seed 42 yields five blocked formations and five decorations.

Enclosed-chamber mode landed 2026-09-14. It preserves a whole connected source
chamber, including its authored perimeter and every image frame, with an even
translation into a larger canvas. **Fit source artwork** computes the required
canvas from all layer extents. Sources with walkable outer edges, disconnected
floor, or artwork exceeding the canvas are rejected. D1006 exports as a complete
648-cell chamber with safe arrival, eight spawns and paired portals. This mode
retains the source outline; it is a whole-chamber template operation, not new
freeform wall synthesis.

Room-and-corridor drafts landed 2026-09-14. Seeded binary space partitioning
places 2–12 rooms, joined by a minimum spanning tree of L-shaped corridors.
Passage widths are 2, 4 or 6 cells. Whole source-art placement protects every
corridor cell and room centre; validation enforces that protection after edits.
Room guides appear in Tiles, and sidecars retain room bounds, passage cells,
width and seed. Export and Test walk include safe arrival, eight monsters and
paired portals. This generates connected floor geometry; assembling authored
chambers with complete doorway/perimeter transitions remains open.

Navigation overview landed 2026-09-14. Maps with no available authored minimap
image display their walkable terrain in both client map windows, using cached
horizontal floor runs and the existing player/creature markers. Generated
exports and Test walk work without additional files. Authored minimap images
retain their existing rendering path.

Portal markers landed 2026-09-14. Generated exports, map saves and Test walk
write `.navigation.json` containing paired exit/arrival coordinates. Shared
validation checks map identity, version, dimensions, walkability, duplicate
triggers and arrival cells. The client ignores missing or invalid metadata,
clears markers on map changes, and draws exits as pulsing teleport beacons
plus cyan dots in both map windows. The beacons reuse Magic frames 110–119
through the existing effect pipeline. Travel remains server-controlled.

Organic and room drafts still need arbitrary wall-edge transitions. The
2026-09-15 parallel batch investigated source wall reuse and a conservative
chamber stretch, but no safe useful seam passed. See [wall assembly findings](WALL_ASSEMBLY.md)
for repeated diagonal motifs, rejected approaches and the required wall kit.
No production wall synthesis was added in that batch.
Illustrated minimap art, authored chamber assembly, WFC and AI intent remain
future work.

1. Generators that only use tile ids seen in shipped maps of the chosen
   biome (forest, sand, snow, wood, cave): cellular-automata caves, room and
   corridor dungeons, and wave-function-collapse outdoors trained on the
   shipped maps' 3x3 tile neighbourhoods so walls, cliffs and roofs stay
   consistent.
2. Validation: connectivity by BFS from every exit, no unreachable spawn
   regions, minimum free area, exits paired with `MovementInfo` rows.
3. AI layer: a prompt ("a snowy mountain pass, 200x120, two exits, mid-level
   monsters") is turned into generator parameters, spawn tables and NPC
   placement by an LLM through a JSON schema; the generator does the geometry,
   the LLM never writes tiles directly. Names, descriptions and quest hooks
   also come from the model.

#### Monsters

Stat drafting landed 2026-09-14. The MonsterInfo detail panel previews a
named variant at a target level before creating the new monster and linked
MonsterInfoStat rows. Source appearance, AI and timing are retained. Creation
is atomic, stale previews are rejected, and one undo restores both collections
and their ID counters, including after saving. Drops and sprite variants remain
separate future work. These are reviewable balance estimates; playtesting is
still needed before placing them into progression.
The initial estimator uses per-level medians from matching AI/boss records,
interpolation within observed levels and an explicit level-ratio fallback
outside them. Factors are capped; paired minimum/maximum stats scale together.
This is a local deterministic draft tool. Dedicated profile tuning, learned
global balance curves and AI-authored drafts remain future work.

Authored drop templates landed 2026-09-15. Monster details can rank donor
monsters by boss status, level distance and AI, preview their items/base odds/
random quantity ranges, then copy the table to a monster with no drops. The
operation preserves authored values, allocates fresh row IDs when the schema
has them, rejects stale previews and supports one-step undo. DropSet and
EasterEvent conditions are enforced by the server and labelled in previews;
disabled/PartOnly rows are labelled. A combat integration test verifies that
killing a drafted monster creates its configured loot with account ownership.
Automatic tier/biome loot composition and reward-budget tuning remain open.

Conditional loot and selective composition landed 2026-09-15. Spawns snapshot
their mask and Easter eligibility; event marking requires an explicitly active
server deadline and a positive spawn chance. All loot paths check conditions
before RNG/account progress. Generated-map sidecars and controls retain spawn
loot conditions. Template previews support individual-row and item-category
selection, preserving authored order, values and restrictions. Existing-table
replacement and automatic cross-donor biome composition remain outside this
slice. Manual multi-donor drafts landed 2026-09-15: queue selected source rows,
review/remove entries, and apply atomically with one Undo. Duplicate source rows
are rejected; repeated items from distinct rows retain their independent odds
and conditions. Pending queues warn on close and block reload; edits invalidate
stale drafts without partially applying them.

Tier-plus-material composition landed 2026-09-15. The selected tier donor
supplies non-material drops and an explicitly chosen biome donor supplies
material drops. The picker lists material-bearing donors and row counts.
Composition builds the existing reviewable queue without changing the database;
same-donor composition does not duplicate rows. Missing required categories
are rejected. Automatic biome inference and reward-budget tuning remain open.
Base-yield comparison landed next: selected rows and queued recipes show
per-item units per 100 independent base rolls, separating ungated and conditional
yield and excluding disabled/PartOnly rows. This supports manual comparisons;
account progress and group adjustments are not simulated and odds are not tuned.

1. Stats from a level curve fitted to the shipped `MonsterInfo` rows (HP, AC,
   DC, experience, speed per level) with a "boss" and "passive animal" profile,
   so a level-30 generated monster sits between shipped level-28 and 32 ones.
2. Drops from a template per tier plus the biome's material items.
3. Sprites: animated 8-direction sets are the hard part. Plan in order of
   reliability: (a) recolour and rescale variants of existing sprite sets
   (palette shift in `.Zl`, new `MonsterImage` id), (b) mix parts from sets
   that share a skeleton (same frame layout), (c) generated sheets from an
   image model with a sprite-sheet control net, reviewed frame by frame in the
   editor before import. Ship (a) and (b) first; treat (c) as research.

#### Sprite authoring delivered 2026-09-15

MonsterInfo now previews recolour/rescale variants and exports all 1000 slots of
one shape into a complete private replacement library. Other shapes stay raw;
shadows, overlays, empty slots and anchors survive. Export refuses existing
folders and original asset roots. It does not allocate a new MonsterImage ID:
all records sharing the existing Image use the override. Part mixing and AI
sprite-sheet generation remain open.

#### Items

Local equipment drafts landed 2026-09-15: minimum-level weapons, armour and
combat accessories use same-slot/class/rarity median core budgets, with bounded
level-ratio fallback. Explicit preview precedes atomic new item/stat creation;
one Undo works after save. Appearance, restrictions and special stats remain
unchanged. These are estimates, not measured balance. AI drafting, generated
icons and broader requirement support remain open.

1. LLM-drafted items validated against the equipment stat budget per level
   (derived from shipped items: stat points per required level and slot).
2. Icons: recoloured shipped icons or generated 36x36 icons via the same
   review-and-import path; ground sprites likewise.

#### Wall-piece authoring delivered 2026-09-15

Named reusable wall/corner/doorway/formation pieces now save all cell data to
bounded versioned files, load across maps, and stamp as one undo operation.
Repeat counts and even diagonal steps assemble wall runs. Full frame bounds,
conflicting overlaps and generated-layout restrictions are checked atomically.
D1006 northwest wall strips were reassembled and checked in the native client.
Selections and joins remain manual. A category label does not provide doorway
ports or corner compatibility. Automatic wall/corner/doorway synthesis and a
complete verified kit remain open; see WALL_ASSEMBLY.md.

#### Explicit piece connections delivered 2026-09-15

Pieces now declare cardinal edge openings with offset, width and an explicit
compatibility profile. Collision validation checks opening depth and routes
between connections. Two pieces can compose through compatible opposite edges,
without rotating assets or shifting back-tile parity. Composition consumes the
joined pair, remaps unused exterior connections, and creates a reviewable new
piece; source files and the open map remain unchanged. Version 2 files persist
connections and transparent padding coverage, with version 1 loading retained.
Offset joins preserve destination cells outside their source pieces. A real D1006 wall-pair join and
corner route/rejection cases are covered. Automatic art compatibility, a full
verified corner/door kit and arbitrary
perimeter synthesis remain open.

#### Recessed connections delivered 2026-09-15

Version 3 piece files retain inset connections along concave coverage boundaries.
Joins preserve unused recessed openings and can fill them with further pieces.
Coverage, not just bounding rectangles, determines overlap and exposed openings;
covered-cell overlap or sealing an unused opening is rejected atomically. Version
1/2 files remain readable. Three-piece assembly, reload, invalid overlap and
native source-floor rendering were checked. This is composition support; the
complete visually verified wall/corner/doorway kit and automatic perimeter
assembly remain open.

#### Checked starter kits and candidate browser delivered 2026-09-15

The editor can capture a real D1006 convex north wall corner and its two
source-adjacent approaches, plus a separate `0_000` palace doorway with its
wall ends and carpeted opening. Source fingerprints and full sprite bounds
checks reject incompatible packs and cropped selections, including intrusion
from outside anchors. Wood NW, Wood NE and palace doorway have separate profiles.
The doorway is not a Wood component, and these starter kits do not complete an
interchangeable wall/corner/doorway set.

The piece browser searches up to 16 loaded pieces and 256 compatible connection
pairs on request. Every candidate passes full join validation; Build validates
again. Folder catalogues load direct JSON files in deterministic filename order
and reject malformed or oversized catalogues without returning partial results.
Export validates all pieces first and writes only to a new directory, retaining
asset references. Automatic perimeter generation and automatic visual seam
compatibility remain open.

#### Extended north and checked south kits delivered 2026-09-15

The north starter now has five pieces: the original NW approach and north
corner, a deeper NE approach, the authored NE bend and a terminal wall body.
Its 14x14 envelope restores D1006 exactly at (14,2). Dedicated bend/body
profiles constrain the authored sequence rather than treating the irregular
bend as a repeatable strip. A separate three-piece south kit preserves the
south corner, adjoining wall sections and their tall decoration in a 14x24
envelope at (20,26). Full animation bounds matter: a frame outside the static
SE selection requires extending its upper boundary to row 26. Native inspection
also caught a circular motif cut between complete individual sprites. The final
south assembly extends the SE selection and adds a covered 2x2 corner cap to
preserve all 72 motif cells and nine animated anchor columns. These are authored
group pieces, not independent repeatable bodies. The north terminal segment
likewise does not supply a physical wall endcap.

The browser can add the current piece with its edited name, category and
connections as an independent copy, and remove catalogue entries. Changes
invalidate cached join results; the existing 16-piece limit and new-folder
export apply. North, south and palace built-ins remain available separately.
Further NW/NE wall repetition intersects neighboring decoration in D1006;
see WALL_ASSEMBLY.md for exact constraints. A complete compatible perimeter
kit and automatic perimeter generation remain open.

#### Whole-kit assembly delivered 2026-09-15

**Assemble whole kit** starts from the first catalogue entry and consumes each
of at most 16 entries once. Each step requires exactly one fully valid next
join among unused entries. Ambiguity, a stalled assembly or exhaustion of the
shared 256-trial discovery budget fails without adopting a partial result.
Chosen joins are revalidated separately, at most 15 more times. Success
makes a new current piece; map changes still require placement.

Catalogue **↑** / **↓** controls choose the starting entry and export order;
reordering clears cached candidate results. `ZIRCON_ASSEMBLE_KIT=1` assembles
an autoloaded built-in or folder catalogue. Real-asset tests assemble both
Wood kits and restore source artwork and collision byte-for-byte; disposable
fixtures are under `/tmp/zircon-auto-assembly-final`.

This automates assembly of an authored kit. It does not infer visual seams,
resolve ambiguous routes, supply a missing Wood doorway or generate a complete
perimeter. The bounded doorway investigation is recorded in WALL_ASSEMBLY.md.

#### Source-specific Wood wall perimeter delivered 2026-09-16

D1008 supplies checked repeatable bodies for all four diagonal orientations and
four compatible authored corners. Source fingerprints, full-frame bounds and
native corner/body joins are verified. A dedicated fixed-placement composer
consumes all explicit seam pairs simultaneously, permitting a closing cycle
without weakening pairwise joining. It accepts up to 64 placements on an even
256x256 canvas, rejects duplicate ownership/endpoints, invalid alignment,
unpaired covered ports and disconnected floor, and publishes one stamp/undo.

**Wood wall perimeter** uses NW and NE counts from 3–10, deriving SE=NW+2 and
SW=NE from measured corner geometry. It closes the outer wall ring and fills
only unowned interior floor blocks, leaving the exterior blocked. Source-backed
tests cover all 64 count pairs. This produces new room dimensions from source
pieces; it is separate from whole-source enclosed chamber copying.

`Layout.wall_repeats` retains counts and restores the generator mode on reload;
legacy layouts default to no repeat metadata. Counts require enclosure and the
supported range. Enclosed-layout revalidation now rejects walkable border cells,
even when a later edit keeps the floor connected.

The separate D10032 **Wood entry** captures a shaped 16x38 room/corridor group
with 380 owned cells and 111 connected walkable cells. Full source restoration,
coverage and native artwork checks pass. Its room/corridor profiles remain
separate from the D1008 wall bodies.

The D1008 **Wood wall junction** adds a checked physical passage with four
body-compatible seams in a shaped 20x32 envelope. Source restoration, all four
body joins and native source/isolated previews pass. It retains both complete
wall turns and is available as an authored group; a generated ring cannot
simply replace one wall segment with it.

**Wood connected rooms** is implemented in `wood_connected.rs`. Six convex and
two complete concave corners surround two rooms and a real 6x4 floor passage.
Both counts accept 3–6; all 16 pairs pass source-backed checks. Blocking the
passage column in tests separates two substantial room components, while the
normal map has one connected floor and blocked borders. Native editor/client
views of the generated bundle pass under `/tmp/zircon-connected-mcp/connected`.
A real actor and observer client verified 16 server-authoritative steps through
the passage with all teleport routes disabled. The bounded Wood wall-assembly
phase is complete; see RECOVERY.md for consolidated verification.

`ZIRCON_CAVE_MODE=wood-connected` selects the new mode. `wall_rooms` records one
or two rooms alongside `wall_repeats`; legacy layouts default to zero room
metadata. Reload restores the mode and count bounds. Arbitrary BSP/organic
perimeter art and movable door objects remain separate work.

Twelve stdio [MCP authoring tools](MCP_AUTHORING.md), including `world_perimeter`,
`map_copy_area` and `world_spawn`, are available. A real-process smoke run passed
41 JSON-RPC requests covering all twelve tools under
`/tmp/zircon-connected-mcp`. `world_perimeter` accepts `rooms: 2` for a connected
room bundle and defaults to one room. New silhouettes and animation import are
planned in [PLANS.md](PLANS.md); existing sprite
recolour/rescale exports are delivered. New sprite import/creation remains a
separate planned phase.

### Milestones

| # | Deliverable | Visible result |
|---|---|---|
| 1 | Writers + round-trip tests | `cargo test` proves every shipped file survives load/save |
| 2 | Data editor (items, monsters, drops, magics) | edit a potion's heal and see it in-game after reload |
| 3 | Map/NPC/spawn editing | move a spawn and see it in-game |
| 4 | Map editor with painting | a new hand-made map reachable from Bichon |
| 5 | Procedural map generator + validator | a generated cave with spawns, exits and safe zone |
| 6 | Monster/item generation with AI drafts | a new monster family (recoloured) with drops |
| 7 | AI map intent + sprite-sheet import | prompt-to-map end to end |

Each milestone lands as its own commit with CI green and a screenshot.

### Decisions to confirm

- Editor UI: egui/eframe desktop app (single binary, no web stack) is the
  proposal; the alternative is a web UI served by the server.
- Where AI calls run: from the editor via an API key in the environment, never
  from the game server.
- Generated content is written to a separate `Generated.db` merged at load
  time, so shipped data stays untouched and a bad generation is one file
  delete away.

### Delivered: first original sprite-authoring workflow

PNG/reference export, explicit image/sheet imports, missing-slot authoring,
runtime animation validation, editor playback preview, library builds and private
new-appearance publication are implemented. Eight sprite tools bring MCP to20
tools. The generated Jade Beetle Ring and232-slot Jade Beetle demonstration both
appear in the native client. See [SPRITE_AUTHORING.md](SPRITE_AUTHORING.md).
This supersedes earlier notes calling the entire sprite workflow planned;
additional animation rigs and art-production features remain outside this slice.
The user requested stopping after this task to conserve their weekly allowance.

### Follow-up: item scale and wearable clothing

The user resumed work and noted that items looked small and clothing should have
its own appearance. Item import now has explicit visible-art fitting and new
canvas sizing; the Jade ring demo uses32×32 after removing transparent margins.
This fixes authoring scale without changing the renderer or existing game assets.

Worn clothing remains a separate next slice: item Image supplies icons, while
Shape selects player-body artwork. The current publisher deliberately preserves
that Shape. Clothing authoring needs its own male/female/class-compatible body
contracts, equipment layers and native equipped-motion review. Do not present a
new clothing icon as a new wearable outfit.

### Delivered follow-up: clothing, maps, sprite production and polish (2026-09-22)

The user authorized these workstreams in that order, with independent tasks in
parallel. This supersedes the earlier instruction to stop after item scaling.

- Wearable body export/import, full-contract playback, strict family validation,
  unused-Shape publication and four-family native demonstrations are delivered.
  Shared C# body offsets correct Assassin and mounted rendering; Assassin's
  previously unsupported generic Cast2 uses its valid Cast1 body poses.
- Wood generation supports three connected rooms, and more asymmetric two-room
  sizes. All 39 connected layouts and 64 single-room sizes pass asset checks.
  Both three-room necks pass real server/observer traversal without portals.
  Wood floors use bounded backtracking over learned source adjacency; organic
  and BSP floors retain their previous behavior.
- Castle gate creation/relocation includes closed-footprint validation and a
  native editor preview. Missing stock DB gate definitions can be authored from
  existing Sabuk gate art without changing source monster records. Existing
  castle ownership/open/close rules remain authoritative.
- Consistent image/overlay tint revisions and atomic multi-project pack builds
  with a shared manifest are delivered. Existing per-frame layered import and
  the new wardrobe families share these operations.
- Documented wave-six spell simplifications have been corrected against C#;
  missing field artwork now resolves real frames. Settings now persists five
  audio channels and mute and updates active playback. Ranking already existed.

Remaining larger scope: arbitrary branching/organic wall perimeters need more
verified concave source orientations; generic room doors need their own artwork
and interaction contract. Automatic creation of polished new motion, arbitrary
rigs and an integrated drawing/compositing editor remain outside the delivered
sprite tools. AI prompt-to-map orchestration (item 3) was not requested in this
batch. See RECOVERY.md for exact checks and native artifacts.

### Follow-up: varied Wood rooms and static thresholds (2026-09-23)

Connected Wood rooms now accept independent NW/NE wall repeats for each of two
or three rooms. The editor restores these sizes from saved layout metadata.
MCP `world_perimeter` accepts `repeats: [[nw,ne],...]` and returns each checked
doorway center and six-by-four walkable footprint. Original D1008 concave wall
art frames each threshold; tests reject blocked floor or missing wall frames.
The four-seam Wood wall junction is also available through `map_create` and
`map_stamp` for manual placement, with source cell and collision checks.

The C# reference only has movable castle gates; it defines no ordinary
room-door interaction. A separate Rust-only `room_door_place` authoring tool
now turns a checked Wood threshold into a clickable ordinary door. It writes
an immutable private map/database bundle with a bounded `doors.json` sidecar;
the map, navigation and database contents remain byte-identical. The server
loads at most eight doors, starts each closed, checks interaction range, blocks
the eight-cell cut while closed, and refuses to close over an actor. Opening,
crossing, occupied-close rejection, closing, blocked return and closed reload
were exercised through real client/server sockets. The native client rendered
the door using the installed wooden Sabuk gate profile (`Mon-54.Zl`, image 543).
This reuses source art rather than adding a bespoke Wood door sprite; it has
no castle ownership or repair behavior. See [MCP authoring](MCP_AUTHORING.md)
for placement and protocol requirements.

### Follow-up: branched custom Wood walls (2026-09-23)

`world_branch` now exports a T-shaped playable map with independently sized
west, north and east passages (2–8 floor blocks each). It appends 64 generated
wall frames to a private Wood dungeon library while retaining all original
library payloads and source floor art. Four room centers and the required
private Client asset path are returned. Asset checks cover all generated wall
slots, room connectivity and three passage cuts; native client QA confirms the
branch renders. The repeated stone texture remains draft art. Other branch
graphs, organic perimeters and a general-purpose wall drawing UI remain larger
authoring work.

### Follow-up: connected Wood graphs, private art, and sprite motion (2026-09-23)

The earlier remaining-work notes above describe prior checkpoints. MCP
`world_branch` now supports T, Y, cross, and connected graphs of 2–12 rooms
on a 5×5 grid, including cycles and optional indented room edges. It writes
a playable private bundle with generated Wood wall art. A seam-compatible
texture rim reduces tile joins, but native captures still show repeated square
wall motifs; the generated art remains a draft for visual review.

`room_door_place` now builds distinct Wood plank door frames in a private
`Mon-54.Zl`. Doors keep the server interaction and occupancy rules described
above. Map revisions and world generation/population carry private sprite
assets forward; an explicit `assets` path can select a different private view.

Sprite authoring now includes a zoomed pixel canvas, painting/erasing behind
existing body art, canvas padding, previous-frame guides, and keyframe polygon
interpolation across one animation action and direction. Four larger garment
examples publish all required body frames and appear in native standing and
mounted captures. Their flat procedural artwork is a working authoring example,
not final hand-painted clothing. Automatic pose design, arbitrary rigs, and
production sprite illustration remain separate art work.

### Current authoring pass (2026-09-23)

The prior checkpoint above is superseded for these four items:

- Wood graph walls now use 16 painted stone-and-timber panels from
  `crates/mir-editor/art/wood_wall_atlas_16.png`. They replace the repeated
  wall-face pattern while retaining the checked cap and seam shape. Original
  source sprites remain intact; private output adds 256 wall slots.
- Organic caves and 2–12-room dungeons now frame every exposed blocked cell
  with private Wood art and mark walkable room crossings with threshold art.
  MCP bundles and the desktop editor's preview, save/reopen, export and Test
  Walk carry the private 258-frame library. Save uses a `.wood.zl` sidecar.
- The MCP `create_world` prompt takes an ordinary-language brief and guides an
  MCP host model through the existing authoring tools. It is host-side prompt
  orchestration, not a server-side language model or a one-call generator.
- Four painted fabric variants now dress 4,896 source-drawn garment frames,
  covering every required action without procedural silhouettes. The editor
  and MCP `sprite_pose_from_reference` transfer full-body poses and shadows
  from another original armour appearance in the same family. This retargets
  existing animation, not an independently invented skeleton motion. MCP
  `sprite_auto_motion` still designs cloth sway, lunge, recoil and settling
  from one drawn polygon.

Private map and database asset views must be combined deliberately. An explicit
`assets` path is caller-managed and must contain each required private library.
Saving a map outside its bundle retains Wood art but does not also copy other
private sprite libraries from that bundle.


## 2. Original sprite authoring


Status: the first authoring workflow and Jade Beetle demonstration are implemented.
See [SPRITE_AUTHORING.md](SPRITE_AUTHORING.md) for the delivered behavior and
limitations. The sections below retain the design and later-work scope.

The installed assets provide animation layouts, frame timing, viewing directions,
foot anchors, shadows, overlays, palette and scale references. Those are useful
constraints for making new art. Recolouring or resizing an existing monster is
already supported by `sprite_variant.rs`; genuinely new silhouettes and motion
need an authoring workflow and visual review across the entire animation.

### Deliverable and first scope

Start with a new static item icon and one small eight-direction monster using an
existing animation contract. Keep the original libraries read-only. Export each
new asset as a separate versioned pack, with an import manifest mapping new
ItemInfo/MonsterInfo records to the generated library and frame slots. Do not
change the renderer to accommodate an inconsistent asset.

1. **Reference and animation inventory.** Export a contact sheet and JSON contract
   for a selected library: every action, direction, frame index/duration, empty
   slot, image/shadow/overlay dimensions, foot anchor and offsets. Resolve frame
   layouts from the same action tables the client uses. Record source library
   checksums so later builds cannot silently use a different pack.
2. **Editable authoring package.** Export lossless PNG layers and a manifest into
   a new folder. Keep absent slots distinct from transparent frames. Support
   authored PNG imports and a sprite-sheet importer with explicit cell dimensions
   and action/direction mapping. Preserve scale and ground-contact guides.
3. **New artwork.** Allow manual pixel-art drawing, part composition from selected
   reference frames, and optional AI-generated concepts/frames. The AI receives
   the animation contract and contact sheets; it produces editable PNGs, never
   writes raw library indices or live game records. Start with a static icon,
   then a single facing idle/walk cycle before expanding to all required actions.
4. **Consistency checks and editor preview.** Reject missing required directions,
   duplicate/conflicting slots, oversized surfaces, incompatible transparency,
   out-of-range offsets and missing shadow/overlay declarations. Preview actions
   in a loop with a ground grid, foot-anchor marker, original-reference toggle
   and contact sheets. Check flicker, body-size drift, foot sliding and shadows
   visually; dimensional validation alone cannot establish motion quality.
5. **Pack builder.** Feed validated frames through the existing ZlBuilder codec.
   Roundtrip-decode each written slot, compare alpha/shape within compression
   tolerance, preserve slot numbering and explicit blanks, and export to a new
   pack. Neutral export of an untouched source must preserve its raw slots.
6. **Game integration.** Allocate an unused appearance mapping supported by the
   client asset resolver and create draft ItemInfo/MonsterInfo records in a new
   database snapshot. Show all actions in the editor, then spawn the monster in
   an isolated Test Walk bundle. Keep balance and drops separate from artwork.

### MCP extension

Expose `sprite_reference`, `sprite_export_project`, `sprite_validate_project`,
`sprite_build_pack`, and `sprite_preview_sheet` after the underlying authoring
operations exist and are tested. Inputs and output PNG paths stay within the MCP
project; installed libraries are only reference inputs. Return structured frame
errors so an AI can repair one action/direction instead of regenerating an entire
library. New packs and database revisions are explicit outputs; publication or
replacement of installed assets remains a separate user action.

### Completion criteria

A new item icon and a recognizably new monster can be authored/imported, validated,
previewed, packaged, linked to new database records and seen in the running client.
All required idle, movement, attack, hit and death slots for the chosen contract
work in every required direction; empty/unused slots remain valid. Rebuilds are
repeatable from the authoring folder. Existing assets and render quality remain
unchanged. AI-assisted motion generation is optional; it must pass the same
contract and visual checks as hand-authored frames.

Later work: multiple body-part layers and equipment variants, additional action
contracts, palette tools, batch pack generation, animated effects and shared
project manifests. Arbitrary new animation rigs are outside this initial scope.


## 3. Wood perimeter generation


Status: closed single-room and connected two-room Wood generation are implemented
and visually checked. Final server traversal proof and consolidated verification
are being recorded.
This document describes the delivered recipes and remaining verification; it
is not a claim of arbitrary room-outline or doorway synthesis.

### Checked source roles

D1008 supplies four repeatable diagonal wall bodies and four convex corner
groups. Each retains complete source cells, collision, ground backing and wall
art. Source fingerprints and all-frame bounds protect captures, including
artwork anchored outside the selection. Neither sprites nor indices are rotated
or mirrored. Ground variants can differ between repeated bodies; the tests
separately establish exact source capture and matching repeated collision.

The checked body and corner coordinates are recorded in
[WALL_ASSEMBLY.md](WALL_ASSEMBLY.md), with implementation in
`wood_wall_bodies.rs` and `wood_perimeter_kit.rs`. The earlier decorated D1006
north/south kits remain reusable authored groups; their nonrepeatable approaches
are not used as generic perimeter bodies.

### Geometry and atomic composition

`wood_perimeter.rs` places the west corner, NW bodies, north corner, NE bodies,
east corner, SE bodies, south corner and SW bodies in clockwise order. NW and
NE counts each accept 3–10. Measured corner approaches require SE=NW+2 and
SW=NE. Accumulated even-grid translations must return exactly to the first seam.
All 64 supported count combinations pass source-backed generation checks.

`piece_composition.rs` accepts fixed placements and explicit seam pairs. It
validates canvas bounds, complete asset frames, even placement, unique ownership,
unique seam endpoints, profiles, widths and exact world alignment. Every paired
endpoint is consumed simultaneously, permitting cycles without weakening
`Piece::join`'s unused-port checks. The seam graph and walkable floor must be
connected; remaining ports must face unowned space. A failed composition does
not publish a partial map. A successful composition uses one document stamp.

Limits are 64 placements, 512 seams and an even canvas of 2–256 cells per side.
Covered cells cannot overlap; transparent coverage padding can. The resulting
Document can exceed the reusable Piece format's 64×64 size limit without
changing that format. Port profile equality is a geometry declaration; the
built-in recipe additionally depends on reviewed body/corner artwork seams.

### Interior, enclosure and persisted metadata

The generator starts from blocked empty space and reserves each group's complete
2×2 owned blocks. Flooding unowned blocks from outside identifies the exterior;
only unowned blocks enclosed by the ring receive undecorated source floor from
`generate::paint_floor`. Perimeter collision and artwork are never overwritten
by the fill. Exterior blocks must remain nonwalkable. Final layout validation
requires at least 64 walkable cells, one connected floor component and blocked
map-border cells.

The editor exposes **Wood wall perimeter** separately from **Enclosed chamber**,
which copies a whole authored source room. `Layout.wall_repeats` records the
counts; `Layout.wall_rooms` distinguishes one or two generated rooms and
restores the mode on reload. Older layouts default to no repeat/room metadata. Existing navigation, export, private database bundles and Test Walk
operate on the resulting map. `world_perimeter` exposes the same recipe through
[MCP authoring](MCP_AUTHORING.md).

Whole 48×54 and 58×64 rooms passed native editor/client inspection. Evidence is
under `/tmp/zircon-perimeter-first`, with individual body and corner fixtures
under `/tmp/zircon-wall-bodies-verified` and `/tmp/zircon-perimeter-kit-verified`.
Final consolidated validation totals belong in [RECOVERY.md](RECOVERY.md).

### Physical passage extension

The game's teleport triggers do not create physical openings in a wall. The
single-room recipe currently closes every wall and uses existing portal metadata.
A physical entry needs complete wall ends plus a bounded landing, another room
or a terminating corridor. An unexplained walkable breach to the canvas border
is not a completed room connection.

Two authored entry groups are available. The D10032 16×38 entry retains a whole
room/corridor junction and uses its own profiles. The D1008 20×32 masked group in
`wood_wall_entry.rs` retains a real passage between two complete wall turns and
has four body-compatible seams: west NE, recessed east NW, east SE and east NE.
Its exact source capture, four attached body seams and native source/isolated
views pass. All four wall ends require surrounding geometry; it cannot replace
one straight segment of the single-room recipe.

Two clean D1008 concave turns also have passing source/body collision tests and
native inspections: a 6×10 valley transitions NE to NW, and a 6×10 notch
transitions SE to SW. `wood_connected.rs` now uses these with six convex corners
and repeated bodies to form one closed outline around two rooms. Both NW and NE
counts accept 3–6, giving 16 checked combinations.

The concave origins must be separated by exactly 14 rows. A 6×4 rectangle of
walkable floor between their complete collars forms the physical passage.
Tests verify every floor cell is reachable, each side contains a substantial
room, and blocking the passage column separates the two sides. The composer and
interior fill retain the same ownership and enclosure guarantees as the single
room. The separately captured four-seam junction is not used by this recipe.

The UI mode is **Wood connected rooms**, selected by
`ZIRCON_CAVE_MODE=wood-connected`; metadata uses `wall_rooms: 2` with the two
repeat counts. MCP `world_perimeter` accepts `rooms: 2` and defaults to one room.
Native editor/client inspection passed for the bundle under
`/tmp/zircon-connected-mcp/connected`; the maximum 6/6 case passed under
`/tmp/zircon-connected-final/{editor-6-6,game-6-6}.png`. The real-process MCP smoke run passed 41
requests covering all twelve tools. A real actor and observer client verified
16 authoritative walking steps through the passage with teleport routes disabled.
The bounded Wood perimeter/physical-entry phase is complete. Consolidated checks
are recorded in RECOVERY.md; this does not mark the whole editor roadmap complete.

Arbitrary BSP and organic outlines need additional authored geometry recipes.
Original sprite creation is a separate planned workflow in
[PLANS.md](PLANS.md); renderer changes are not
required by the completed wall recipe.
