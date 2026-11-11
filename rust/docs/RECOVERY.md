# Rust worktree recovery — 2026-09-14

Recovered the three unfinished Claude worktrees into `rust/prototype`, starting
at `870c207`. Their original directories under `.claude/worktrees/` remain
intact. The already-merged equipment-set implementation is retained.

## Completed work

- `agent-ab8e2a6802b9837e4`: corpse harvesting, delayed account-owned loot,
  partial collection when the bag is full, group shares, corpse lifetime,
  per-viewer skeleton state, animation and client input (`B` or hold left
  mouse over an adjacent corpse).
- `agent-abf3652ad7fd74d98`: Store tab (`C`), stock loading, tag/name filters,
  sorting, game-gold/hunt-gold purchases and timed items. Capacity checks and
  transfers share the same stacking rules; rejected purchases never charge.
  Expired inventory, equipment and storage items are removed while online.
- `agent-a50d7475e10f2ebc2`: hair/gender changes, armour dye, renaming and
  discipline training. `D` opens the client controls; right-clicking a change
  item also opens them. Server validation, consumption, persistence, original
  sprite dye layers, focus regeneration and immediate client stats updates
  are connected. Eligible discipline-school magics can be granted when data
  defines them.

Item transfers retain dye, expiry, durability and added stats through pickups,
mail and marketplace delivery. The recovery initially used protocol version **2** (crafting below advances it to **3**): rebuild the
client and server together. Existing JSON saves default the new fields.

## Validation

All checks below passed locally with `~/zircon-assets/Client`:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo build --workspace`
- `ZIRCON_ASSETS=~/zircon-assets/Client cargo test --workspace`:
  **153 passed, 9 explicitly ignored, 0 failed**.
- Both real-server integration tests: multiplayer/rate limits/shutdown saves,
  and store purchases/appearance changes/discipline training/persistence.
- `docker build -t zircon-recovery:local -f rust/Dockerfile rust` from the
  repository root, followed by the image's `--inspect` mode with assets
  mounted read-only.
- Repeated native client/server runs using disposable saves and headless GPU
  screenshots: the store, hunt-gold/category filtering, harvesting and its
  resulting loot, discipline/focus display, and dyed hair/armour. The first
  discipline screenshot exposed a missing stats notification; it is fixed
  and covered by the network test.

Screenshot and runtime logs from this local session are under
`/tmp/zircon-recovery-20260914/`. Reproduction switches are in `../README.md`.
The Windows CI job has not been run remotely; no changes were pushed.

## Existing limits

The store still inherits the rewrite's lack of item flags (`Bound`, `Locked`,
`Worthless`), as documented in the README. NPC weapon crafting was completed in the continuation below. The editor roadmap is tracked in PLANS.md.


## Continuation: NPC weapon crafting

Added the Weapon Smith's template and reroll flow, seven linked inventory
cells, class selection, displayed cost/roll count, server validation and
weighted stat application. Invalid requests cannot consume items or gold.
Rerolls preserve identity, durability, dye and expiry. Protocol is now **3**.

Focused tests cover all four classes, stale/duplicate/wrong-colour links,
insufficient gold, absent NPC, full bags, consumption and reroll preservation.
A real-server test opens the shipped Lost Land smith, crafts using all six
colours, checks client notifications and verifies the saved weapon/materials.

The original crafting milestone did not track enhancement sources or
refinement readiness. Both are addressed in the follow-up below. The earlier
claim that C# makes rerolled weapons permanently non-refinable was incorrect:
it clears Refinable readiness, leaving the separate NonRefinable flag intact.

Validation for this continuation: **157 tests passed, 9 explicitly ignored**;
workspace build, formatting and strict Clippy passed. The updated Docker
server image built and its asset-backed `--inspect` smoke test passed.
Two native client/server screenshot runs verified the populated crafting
window; the final image is `/tmp/zircon-craft-20260914/crafting-final.png`.
The disposable server was stopped. Changes remain local and uncommitted.


## Follow-up: enhancement preservation and refinement state

Protocol **4** carries enhancement totals and refinement flags. Persisted
item bonuses now include their source. Crafting retains Enhancement entries
and rolls Added entries independently, even for the same stat. Refine rewards
are tagged Refine. Combat stats and NPC checks sum every source once.

Rerolls set weapon level to 1 and clear readiness, preserving the permanent
NonRefinable flag. The furnace checks readiness; retrieval clears it on both
success and failure. Combat experience earns the next readiness level using
the C# table (fractional experience retained); quest and consumable rewards
do not. The client shows readiness and enhancement preservation.

Old item pairs migrate as Legacy without changing values. Source provenance
that an old save never recorded cannot be inferred. Missing readiness fields
default to the old ability to refine. Source and state differences prevent
otherwise identical items stacking together.

Validation: **160 passed, 9 explicitly ignored** across the workspace,
including the real-server enhancement reroll/persistence test. Formatting,
strict Clippy, workspace build and Docker build/asset smoke test passed.
The native client screenshot at
`/tmp/zircon-craft-20260914/enhancements.png` verifies the updated weapon
crafting window against protocol 4. The disposable server was stopped.
Changes remain local and uncommitted.

## Phase 2 completion: map, region and spawn panel

The editor now has a Maps tab with map search, fit/zoom/pan, walkability and
region overlays. Create a region, add/erase rectangles, attach a monster spawn
(count and delay) or a safe zone, and inspect all records sharing the region.
Existing records link to the table editor. Each operation uses undo and the
existing backup-on-save/validation workflow. New placements and changed shapes
on loaded maps currently require a server restart to rebuild placement caches.

An integration test exposed shipped `next_index` counters equal to an existing
record ID. New and duplicated records now allocate above every used ID. The
placement test verifies region cells, spawns and safe zones through save and
the server loader; it also checks backups, reload and undo after saving. A
previous refinement test now identifies the returned weapon by instance ID,
avoiding confusion with another starter weapon of the same type.

Validation: **162 passed, 9 explicitly ignored**; formatting, strict Clippy,
workspace build, Docker build and asset-backed container inspection passed.
Native editor screenshots used a disposable database copy; the final image is
`/tmp/zircon-editor-phase2/map-regions-final.png`. Original game assets and saves
were not edited. Phase 3 (map tile painting) is next in PLANS.md.
Changes remain local and uncommitted.

## Phase 3: tile painting and isolated test play

Added a Tiles workspace with sprite palettes (base and themed map libraries),
back/middle/front painting and erasure, pencil and rectangle gestures,
eyedropper, whole-cell stamps, animation settings, collision/light brushes,
and grid/blocked-cell overlays. Neighbour matching learns cardinal adjacency
frequencies from the current map/library. It is a local brush, not a global
procedural generator. Saved maps can be registered in MapInfo from this view.

Map changes have separate gesture undo and dirty-state tracking. Back tiles
and stamps respect 2×2 storage alignment, unrelated bytes survive edits, saves
retain backups, external file changes block overwrites, and closing prompts
about unsaved map or database work. Backups use nanosecond timestamps to avoid
replacing backups during rapid saves.

Test walk launches the built server/client with temporary map/database copies
and disposable saves; right-click picks the start cell. The shared
ZIRCON_MAP_DIR override isolates map loading, and the test definition uses
daylight. The editor stops its child processes on close and retains snapshots
and logs for inspection. The client renderer was not changed; the library
mapping was moved to mir-formats so both renderers use the same table.
The editor is an unlit preview; use the client for final lighting/screen-blend
inspection. Dedicated door/flight properties are not inferred from unknown
map bytes, which remain intact.

Validation: **167 passed, 9 explicitly ignored**, workspace build, formatting,
strict Clippy, Docker build and asset-backed container inspection passed.
Tests cover tile alignment, property-byte preservation, stamp clipping/undo,
adjacency selection, backups, dirty state and external-write conflicts.
Two native editor/client/server runs loaded an edited disposable map. Final
screenshots are `/tmp/zircon-editor-phase3/tile-editor-final.png` and
`/tmp/zircon-editor-phase3/game-map-final.png`. Original assets and personal
saves were not edited. Changes remain local and uncommitted.


## Phase 4 — playable cave drafts (2026-09-14)

Added `mir-editor/src/generate.rs`: deterministic cellular-automata topology,
largest-component filtering, observed intact floor patches, BFS validation,
safe arrival, eight selected monster spawns and two same-map portals. Each
portal lands beside its opposite trigger, avoiding recursive travel loops.
Native visual inspection rejected an initial wall-patch approach because it
split large composite wall art. Final output deliberately uses floor and empty
blocked surroundings; detailed wall/decoration/portal/minimap art is outstanding.

Tiles exposes generation, validation, region/portal overlays and export. Export
creates a fresh folder with the map, JSON placement sidecar, full database
snapshot and launch notes. It never changes the active database or original
assets. Map saves retain the sidecar; a reopened bundle can be tested again.
Test walk includes all generated content in its temporary database. Regions
and map selection respect `ZIRCON_MAP_DIR` for bundle inspection. No protocol
or game-renderer changes.

Validation: **170 passed, 9 explicitly ignored**; workspace build, formatting,
strict Clippy, Docker build and container asset inspection. The three new editor
tests cover seed reproducibility/serialization, invalid connectivity and
placements, and a real asset-backed export through the server loader. The last
test also starts a World, checks all eight monsters and the safe zone, then
walks through both portals and checks exact arrival cells. Real socket tests
needed execution outside the restricted sandbox and passed there.

Visual artifacts: `/tmp/zircon-cave-phase4-final/editor.png` and
`/tmp/zircon-cave-phase4-final/game.png` show the cave in the editor and real
client/server. Logs: `/tmp/zircon-gen-{tests,all-tests,clippy,build,docker}.log`.
Changes remain local and uncommitted. Next work is larger wall/edge templates
for cave art, then room/corridor and WFC generators; AI generation is not yet
implemented. Milestone 5's gameplay slice is runnable, but its finished-art
scope is still open.


## Phase 4 — complete source-art templates (2026-09-14)

Added `mir-editor/src/cave_art.rs` and integrated it into cave generation.
Sprite metadata supplies exact render bounds for every animation frame. A cell
grid groups touching artwork; capture bounds expand until all intersecting
sprites fit, then snap to the back-tile grid. Templates copy every cell field,
not isolated wall strips. Captures larger than 32×32 and ambiguous narrow/tall
strips are rejected. Missing source images/libraries produce an actionable
error instead of copying unverifiable artwork.

Placement is seeded, bounded, and prioritizes larger blocked formations before
small floor decorations. It requires substantial collision agreement, keeps
placements apart, and validates the complete floor graph after each candidate.
A rejected placement restores every byte. Safe/spawn/portal locations are
computed after accepted placements. The topology helper now rejects a small
isolated component before choosing portal endpoints, avoiding a panic during
candidate rejection. No changes to the game renderer or original assets.

D102, 64×64, seed 42: 112 reusable templates; five blocked formations and five
decorations placed; four oversized groups and three ambiguous strips skipped.
The editor's generation status reports counts. `ZIRCON_TILE_ZOOM` supports
reproducible overview screenshots, and explicit `ZIRCON_TILE_CENTER` overrides
placement start when reopening a cave for testing.

Validation: **173 passed, 9 ignored**, strict Clippy, formatting, workspace
build, Docker build and read-only asset inspection. New tests cover complete
capture expansion, oversized rejection, placement on floor, byte-exact rollback
when a corridor would close, and rejection of disconnected islands. The asset
bundle test verifies decorated-map determinism and still exercises eight actual
spawns, safe-zone detection, and both portal directions in the server World.
Native editor/game checks: `/tmp/zircon-cave-formations/editor.png`, `game.png`,
and `wall-game.png`. Logs: `/tmp/zircon-cave-art-{tests,all-tests,clippy,build,docker,docker-inspect}.log`.

Remaining cave art work: continuous wall/edge transitions around the full
perimeter, portal markers and minimap artwork. Whole formations are now safe
to reuse, but this is not yet a complete perimeter-wall generator. All changes
remain local and uncommitted.


## Phase 4 — enclosed source chambers (2026-09-14)

Added **Enclosed chamber** alongside **Organic draft** in Tiles. This provides
complete authored perimeters by preserving a whole compact source chamber,
not by synthesizing arbitrary walls around the cellular-automata outline.
Every source cell is copied intact at an even offset. Canvas margins include
complete frame extents for back/middle/front artwork, including all animation
frames. **Fit source artwork** calculates a valid canvas; insufficient space,
walkable outer source edges, odd source dimensions and disconnected floors
are rejected before modifying the current document. The seed changes chamber
placement within available canvas space; it does not change the source outline.

The placement sidecar records `enclosed`, defaulting to false for old files.
Reopening restores the mode. Existing export, safe-zone/spawn/portal generation
and isolated Test walk work for both modes. The shared asset-test gameplay
check now exercises both: eight spawned monsters, safe-zone detection, and
walking through each portal to its separate arrival cell.

Validation: **175 passed, 9 ignored**; workspace build, formatting, strict
Clippy, Docker build and read-only container inspection. Tests cover cell-field
preservation, even placement, full-frame margins, deterministic output,
undersized/open-edge source rejection, sidecar compatibility and full D1006
export roundtrip. D1006's 648 walkable cells are preserved. Native editor and
game screenshots: `/tmp/zircon-chamber-final/editor.png` and `game.png`. Logs:
`/tmp/zircon-chamber-{tests,all-tests,clippy,build,docker,docker-inspect}.log`.

Freeform continuous perimeter-wall synthesis remains open; the enclosed mode
is explicitly a source-template workflow. Other future work includes portal
markers, minimap art, room/corridor assembly, WFC and AI content generation.
Changes remain local and uncommitted; original assets and personal saves were
not modified.

## Phase 4 — room-and-corridor drafts (2026-09-14)

Added **Rooms + corridors** in Tiles. Seeded binary space partitioning places
2–12 non-overlapping rooms, connected by a minimum spanning tree of L-shaped
passages, with selectable widths of 2, 4 or 6 cells. Unsupported dimensions or
overcrowded room requests fail before changing the current map. The shared
floor painter preserves the existing organic generator's seeded output.

Source-art placement now optionally protects cells. Dungeon generation protects
every corridor cell and room centre, so decorations cannot block passage
clearance. Layout validation repeats these checks after editing. Room bounds,
corridor cells and width persist in the placement sidecar; reopening restores
the mode, controls and seed. Older cave/chamber sidecars remain compatible.
Blue room guides and numbered labels make the layout visible in Tiles.

Validation: **179 passed, 9 ignored**, workspace build, formatting, strict
Clippy, Docker build and read-only inspection. New tests cover deterministic
layouts across seeds and corridor widths, connectivity, room separation,
invalid requests, protected passages after source-art placement, byte-identical
generation, sidecar roundtrip/compatibility, and gameplay through both portals
with eight monsters spawned. D102 at 64×64, seed 42 produces eight rooms,
568 protected corridor cells at width four, five formations and five decorations.

Native editor and client screenshots were captured and inspected at
`/tmp/zircon-dungeon-final/editor.png` and `game.png`; the playable bundle is in
the same folder. Logs: `/tmp/zircon-dungeon-{tests,all-tests,clippy,build,docker,docker-inspect}.log`.
These are connected floor drafts: continuous perimeter and doorway artwork,
authored chamber assembly, portal visuals, minimap art, WFC and AI generation
remain open. Changes are local and uncommitted; original assets and personal
saves were not modified.

## Phase 4 — navigation overview (2026-09-14)

Maps without an available authored minimap image now show a walkability
outline in both client map windows. This works with existing generated
exports and disposable Test walk maps without new bundle files. Horizontal
floor runs are cached when loading the map, discarded on load failure or
leaving the world, and drawn at two pixels per cell. Clipping, scrolling,
opacity and existing player/creature/NPC dots apply to the overview. Authored
map images still use the existing sprite path. No world-rendering changes.

Validation: **181 passed, 9 ignored**, formatting, strict workspace Clippy,
workspace build, Docker build and read-only inspection. New regression tests
check exact floor coverage (including gaps, boundaries and partially set
collision flags) and clipping at all panel edges. Native client screenshots
check the generated dungeon in both map windows:
`/tmp/zircon-navigation-mini.png` and `/tmp/zircon-navigation-big.png`.
Logs: `/tmp/zircon-navigation-{all-tests,clippy,build,docker,docker-inspect}.log`.

The overview is collision geometry, not illustrated minimap artwork. Portal
markers, continuous walls/doorways, authored chamber assembly and the remaining
WFC/AI generators are still open. Changes remain local and uncommitted.

## Phase 4 — visible generated portal markers (2026-09-14)

Generated portal triggers now have pulsing blue beacons in the game and cyan
markers in both map windows. The effect reuses shipped Magic frames 110–119
at 100 ms/frame with original sprite offsets and the existing Screen effect
pipeline. It is a repeated teleport burst, not newly generated artwork.
Server MovementInfo records continue to perform travel.

The shared mir-formats navigation module writes versioned `.navigation.json`
sidecars with map dimensions, an exact map-byte checksum, exit positions and
opposite arrival cells. Validation rejects stale maps, unsupported versions,
blocked/outside cells, duplicate triggers and landings on triggers. Failed
sidecar saves retain an unsaved indication until a successful retry. The
checksum detects stale data; it is not authentication. Export, generated map
save and disposable Test walk write the sidecar. Old exports gain it after
reopening and saving/exporting in Tiles. Missing metadata is normal; invalid
metadata logs a warning and disables those markers. Map load failure, map
changes and leaving the world clear client markers.

Validation: **184 passed, 9 ignored**, strict workspace Clippy, workspace build,
formatting, Docker build and read-only inspection. Two shared tests cover
roundtrip, absent/malformed/stale metadata and invalid portal placements.
A save-failure regression verifies dirty state, blocked reopening and retry.
Existing cave/dungeon bundle tests now compare exported markers with actual
portal endpoints before testing server travel in both directions and spawning.
Native screenshot artifacts: `/tmp/zircon-portals-final/{editor,game}.png`.
Logs: `/tmp/zircon-portals-{final-tests,clippy,build,docker,docker-inspect}.log`.

Work was parallelized: one agent checked source artwork and rendering
references, another implemented shared metadata/export validation, and the
primary agent integrated client markers, editor saves and runtime checks.
Continuous wall/doorway artwork, illustrated minimaps, authored chamber
assembly, WFC and AI generation remain open. Changes are local and uncommitted.

## Phase 4 — monster stat drafts (2026-09-14)

MonsterInfo's detail panel now offers a target level and new name, a stat/XP
preview, and Create new monster. The estimator groups references by AI and
boss status, takes per-level medians, and interpolates within observed levels.
Fields lacking brackets use a clearly labelled target/source level ratio.
Factors are capped at 1/16–16; min/max pairs share a factor, positive stats stay
positive, and XP is rounded to hundredths. Unknown/nonpositive stats, source
appearance, AI, movement/attack timing and other source fields are retained.
No drop rows are copied. These are deterministic estimates, not proven balance
or AI-generated content.

Creation clones MonsterInfo and MonsterInfoStat rows with fresh IDs, prepares
and serializes the result before replacing the database, and rejects previews
when the database changed. Selected sources with duplicate stat rows are
rejected; unrelated duplicate stats follow the server's last-row-wins rule for
reference curves. One grouped undo restores both tables and ID counters,
including after saving. Existing editor actions retain their single-table undo.

Validation: **187 passed, 9 ignored**, formatting, strict Clippy, workspace
build, Docker build and read-only inspection. New tests cover interpolation,
fallback/caps, source preservation, stale previews, unique IDs, database/server
roundtrip, and grouped undo after save. A generated level-20 Draft Oma (#360)
was exported to a disposable dungeon and eight instances spawned after entry.
Native client inspection showed named monsters approaching and attacking the
level-70 test player. Editor preview and game screenshots:
`/tmp/zircon-monster-verified/{editor,game}.png`. The same folder contains the
playable map/database bundle. Logs:
`/tmp/zircon-monster-{tests,all-tests,clippy,build,docker,docker-inspect}.log`.
Screenshot automation: `ZIRCON_EDITOR_MONSTER_DRAFT=1` expands/previews the
selected monster; `ZIRCON_EDITOR_MONSTER_LEVEL=20` chooses a level without
creating or saving a record.

Backend work and integration review ran in parallel with UI, undo and runtime
validation. Drop templates, sprite variants, profile tuning, wall/doorway
synthesis, WFC and AI drafting remain open. Changes are local and uncommitted;
original assets and personal saves were not modified.

## Phase 4 — authored drop templates (2026-09-15)

Monster details now offer Apply drop template: select an authored donor ranked
by boss status, level distance and AI; preview item names, base one-in odds,
runtime quantity ranges and restrictions; then copy its table into a monster
with no existing drops. Base odds and authored quantities are preserved.
Fresh DropInfo IDs are allocated when the schema has Index (the shipped
schema does); full database snapshots reject stale previews. Application is
atomic, one-step Undo restores rows/counters, and donor choices refresh after
edits, undo or reload.

DropSet and EasterEvent tables are rejected because the current server does
not enforce their conditions. Chance-zero rows are labelled disabled; PartOnly
rows are labelled as skipped by ordinary loot rolls. The preview explains
group sharing/account drop progress, and that quest/world reward tables are
not copied. Automatic tier/biome composition is still future work.

Validation: **191 passed, 9 ignored**, formatting, strict Clippy, workspace
build, Docker build and read-only inspection. Three backend asset tests cover
source/row preservation, exact server-loaded odds/counts, identity allocation,
stale previews, invalid references/quantities and conditional-row handling.
The editor integration test applies a template, saves, undoes, reapplies and
exports. A normal melee attack followed by the server tick kills the drafted
monster and produces the configured single Healing Potion with account
ownership. The guaranteed one-potion donor is a disposable QA fixture, not a
change to original assets or default loot balance.

Native screenshots inspected: `/tmp/zircon-drops-verified/editor.png` shows the
preview, and `game.png` shows the drafted monster in combat. The same folder
contains the playable map/System.db bundle; Preview.db is the pre-application
fixture for the editor screenshot. Logs:
`/tmp/zircon-drops-{tests,all-tests,clippy,build,docker,docker-inspect}.log`.
`ZIRCON_EDITOR_DROP_DRAFT=1` opens/previews the controls and
`ZIRCON_EDITOR_DROP_DONOR=Oma` selects a donor without applying or saving.

Backend and runtime review ran in parallel with UI and end-to-end validation.
Drop-set/event runtime support, tier/biome composition, sprite variants and
remaining map/AI work are still open. Changes remain local and uncommitted.

## Conditional loot fixed; selective composition (2026-09-15)

The previous DropSet/EasterEvent limitation is fixed. The server loads spawn
DropSet/EasterEventChance and drop EasterEvent, snapshots mask/event eligibility
on each spawned monster, and filters drops before RNG or account progress.
Required masks use the reference all-bits test, including signed masks. Easter
marking requires a positive spawn chance and an active explicit event deadline:
ZIRCON_EASTER_EVENT_END is UTC Unix seconds, unset/invalid/expired means disabled.
Marked monsters retain eligibility after expiry or data reload; new spawns use
current settings. Common filtering covers normal, group and harvested loot.
Group-adjusted chance arithmetic now uses i64 to avoid overflow.

Generated map controls expose the spawn mask and event chance. These persist
in backward-compatible placement sidecars and populate RespawnInfo in exports
and Test walk. Conditional drop templates are now accepted and labelled.
The next slice also landed: select individual drop rows or category presets
(Consumables, Equipment, Materials, Other), then copy only the chosen rows.
Authored order, probabilities, quantities and conditions remain intact; empty,
duplicate, out-of-range or stale selections fail atomically. Undo remains one
operation. Automatic cross-donor biome composition remains future work.

Validation: **198 passed, 9 ignored**, strict Clippy, workspace build, formatting,
Docker build and read-only inspection. Four server tests cover loader fields,
mask/event gates before RNG/progress, event window boundaries, spawn snapshots
through expiry/reload, group and harvest paths, and large chances. Three new
selection tests cover subsets, preserved authored order, invalid/stale inputs
and category mapping. The editor integration selects only an event/mask-gated
potion, exports spawn mask 7/chance 1, verifies sidecar compatibility and kills
the marked monster through normal combat to confirm its owned drop.

Playable fixture: /tmp/zircon-loot-conditions (System.db + Map; Preview.db is
before applying the selected rows). Native client event/combat screenshot was
captured and inspected at /tmp/zircon-loot-conditions/game.png. Native editor
screenshot attempts stalled and were stopped; no editor screenshot is claimed
for this slice. A temporary readback-polling experiment was reverted. Logs:
/tmp/zircon-loot-conditions-{tests,clippy,build,docker,docker-inspect}.log.

Server conditions and drop composition ran in parallel with editor/spawn
integration and end-to-end validation. Changes remain local and uncommitted;
original assets and personal saves were not modified.

## 2026-09-15 — Multi-donor loot drafts

Added an in-memory loot queue in MonsterInfo: select rows from a donor, Add
selected to loot draft, switch donors, then review/remove entries and Apply
queued loot. Each entry identifies its donor, randomized quantity range, base
odds and conditions. Repeated items remain separate rolls and show a warning;
queueing the exact same source row twice is rejected. Removing the last entry
clears the queue so another target can start immediately.

Recipe application validates the original database snapshot and copies original
source records into one atomic database replacement. It preserves source rows,
conditions and unrelated collections and allocates fresh drop identities. One
Undo restores rows and allocator state, including after saving. Wrong-target,
invalid, duplicate and stale additions/applications fail without partial writes.
Queue previews cannot override authored values. Pending entries count as unsaved
changes and block reload until applied or discarded. Saving the DB does not
persist an unapplied queue. Database edits invalidate queued drafts.

Validation: **201 passed, 9 ignored**, formatting, strict workspace/all-target
Clippy, workspace build, Docker build and read-only server inspection passed.
Three new tests cover two-donor composition with repeated items and distinct
conditions, removal/re-addition, atomic rejection, source preservation, pending
change handling and one-step undo after save. Existing combat integration still
verifies conditional potion drops through the real server path.

Native editor screenshots now capture successfully; inspected updated controls
at /tmp/zircon-recipe-verified/editor.png and the playable generated-map client
at /tmp/zircon-recipe-verified/game.png. Test walk and editor exited normally.
The earlier screenshot stall did not recur; no rendering changes were made.
Logs: /tmp/zircon-recipe-{tests,clippy,build,docker,docker-inspect,editor,walk}.log.
Local Apple tool shims reported an Xcode license error; using
DEVELOPER_DIR=/Library/Developer/CommandLineTools allowed build tools to run
without changing machine configuration or accepting any license.

Automatic biome composition/reward tuning and replacement of existing target
loot remain future work. Changes are local and uncommitted. Original assets and
personal saves were not modified.

## 2026-09-15 — Tier template plus biome materials

Added Tier + biome materials to the monster drop-template panel. The main donor
supplies non-material rows; a second picker supplies material rows and lists
only donors that have them, with counts. Existing boss/level ranking provides
starting candidates. Biome membership is explicitly chosen by the author, not
inferred from monster names. Draft tier + materials builds the existing loot
queue for review/removal and one-step apply/undo. It cannot overwrite a pending
queue. Original probabilities, quantities, conditions and source rows remain
unchanged. Each donor's selected rows keep authored order within its group;
using one donor for both roles includes every row once. Missing categories,
invalid references, occupied targets and stale recipes are rejected.

Validation: **203 passed, 9 ignored**, strict Clippy, workspace build, formatting,
Docker build and read-only server inspection passed. Two new asset-backed tests
verify material counts, cross-donor category selection, every copied source
field, non-mutating draft construction, same-donor uniqueness, and atomic
rejection of missing categories/invalid donors. Existing queue undo and server
combat/conditional-loot tests also passed.

Native editor controls captured and inspected at
/tmp/zircon-tier-verified/editor.png. Generated-map client preview captured at
/tmp/zircon-tier-verified/game.png. Logs use /tmp/zircon-tier-*.log.
Automatic reward-budget tuning, automatic biome inference and existing-table
replacement remain open. Changes remain local and uncommitted; original assets
and personal saves were not modified.

## 2026-09-15 — Base loot yield comparison

Selected donor rows and queued loot recipes now show a per-item comparison of
average units per 100 independent base rolls. Repeated items sum for display;
source rolls remain separate and unmodified. Ungated yield and extra gated yield
are separate columns. The latter assumes every mask/event requirement is met,
not any inferred frequency of eligible spawns. Disabled and PartOnly rows are
excluded and counted. Typed gate fields drive the calculation instead of
parsing human-readable restriction labels. Very small positive values use
scientific notation rather than rounding to zero.

Quantity expectations match the runtime integer roll and minimum-one clamp.
These figures support manual reward comparisons; they do not simulate account
drop progress, group sharing or harvest timing, and do not tune probabilities.
Automatic reward balancing and biome inference remain open.

Validation: **205 passed, 9 ignored**, formatting, strict workspace/all-target
Clippy, workspace build, Docker build and read-only inspection passed. Two new
unit tests compare the mean to enumerated runtime quantity outcomes and cover
repeated items, separate IDs, mask/event gates, disabled/PartOnly rows, empty
selections and large values. Native editor and client screenshots captured and
inspected at /tmp/zircon-yield-verified/{editor,game}.png. Verification logs are
/tmp/zircon-yield-*.log. Changes remain local and uncommitted; original assets
and personal saves were not modified.

## 2026-09-15 — Parallel equipment, sprite and store batch

Three workstreams are implemented. Equipment drafts estimate positive core stats
from comparable equipment, preserve source restrictions and appearance, allocate
fresh item/stat IDs, and support one-step undo across save. New drafts explicitly
disable StartItem and do not copy drops or recipes. Duplicate peer stat rows use
the runtime loader's last-row-wins semantics.

Monster variants preview tint and 50–200% scale and export a complete Zl library
to a new directory. Other shapes remain byte-identical; selected body, overlay,
shadow and anchor data retain their relationships. Re-encoding is lossy. This
replaces an existing Image mapping, affecting every monster sharing that Image;
new Image allocation remains open. Original libraries are untouched.

Store purchases now persist Worthless/Bound/Locked flags, enforce the relevant
sale/drop/transfer restrictions, show flags in tooltips, and support inventory
Ctrl+right-click lock toggling. Protocol is now 5: rebuild client and server
together. Real-server coverage includes unlock/relock and restart persistence.
Tooltips now stay within the viewport and receive the actual item instance.

Native QA also fixed Test Walk's client database selection: client and server now
read the same edited snapshot, so new equipment resolves in the client catalogue.
Editor screenshot automation requests repaint while awaiting GPU capture.

Validation: **218 passed, 9 ignored**; workspace build, strict all-target Clippy,
formatting, Docker build and inspection passed. Subsequent editor-only catalogue
and layout fixes passed editor build/Clippy and native Test Walk verification.
Native screenshots inspected in /tmp/zircon-parallel-verified:
item-editor-final.png, sprite-editor-final.png, store-flags-readable.png,
item-catalog.png and variant-catalog.png. The item screenshot confirms the new
weapon name, +8/+20 DC and level-30 requirement (preview character is level 70).
Both final Test Walk/editor sessions exited successfully. Logs are
/tmp/zircon-parallel-*.log; wire persistence log is /tmp/store-flags-wire.log.

The fourth workstream, generated walls/corners/doorways, remains unfinished.
Source analysis and a diagonal chamber-stretch experiment did not find safe
complete seams. The experiment was removed from production. Evidence and the
required explicit wall/corner/door kit are documented in WALL_ASSEMBLY.md.
Existing whole-chamber generation still retains authored walls; arbitrary
room/corridor boundaries are not fully dressed. Do not mark this batch's map
work complete. All changes remain local and uncommitted; C# files, original
assets and personal saves were not modified.

## 2026-09-15 — Reusable wall pieces and repeated placement

The next wall-authoring step is implemented in wall_piece.rs and the Tiles panel.
Copy an area, name/category it, then save a new versioned JSON piece. Load pieces
across maps; all layer references, animation/light bytes, collision and opaque
cell data survive. Files are bounded to 512 KiB and even 2–64-cell dimensions;
load recomputes complete frame extents from the installed assets. Existing files
are never overwritten. Piece selection remains manual, including neighboring
art anchored outside the rectangle.

Repeats (1–16) with even X/Y steps build wall runs. Placement validates every
piece's complete frame bounds and rejects conflicting cell overlaps before any
map mutation. Generated layouts also validate connectivity and content positions.
One Undo restores the entire run, including after save. Green/red outlines preview
frame bounds; they do not certify neighboring artwork or doorway clearance.

A source-backed fixture exports D1006's complete northwest strip at (10,8),
2x10 cells, and places two copies at step (2,-2). It preserves the authored
collision mask and its native wall join was inspected. Final disposable assets:
/tmp/zircon-wall-piece-final/{northwest-wall.json,WallKit.map,editor.png,game.png}.
The editor screenshot shows repeated-placement controls; the client screenshot
shows the assembled northwest wall. Native session exited successfully.

Validation: **221 passed, 9 ignored**, workspace build, formatting, strict
all-target Clippy, Docker build and read-only container inspection passed.
Three new tests cover save/load byte preservation, no-overwrite export, whole-run
undo after save, partial-failure atomicity, conflicting/invalid repeats, generated
content protection, malformed files, tall sprite bounds and actual source joins.
Logs: /tmp/zircon-wall-piece-*.log. Protocol remains 5. Changes remain local and
uncommitted; original assets, personal saves and C# sources were not modified.

Automatic walls/corners/doorways are still unfinished. A follow-up seam experiment
corrected empty-layer sentinel normalization but produced visibly broken floor
edges; it was removed from production. WALL_ASSEMBLY.md records the findings.
Next map work is a complete verified corner/doorway kit with explicit compatible
joins and physical doorway ports, followed by automatic assembly. Category names
alone do not create those ports or prove compatibility.

## 2026-09-15 — Explicit piece connections and composition

Added piece_ports.rs plus connection authoring/composition controls to wall_piece.rs.
Connections declare a cardinal edge, offset, width and compatibility profile.
Validation requires walkable edge/inward cells and internal routes between all
openings. Joining two opposite compatible openings requires equal widths, exact
profiles and even translation; no artwork is rotated. The result is a new piece
for review/placement/save, with joined connections consumed and unused exterior
connections remapped. Source files and the open map are untouched by composition.
Connections enclosed inside the new bounding rectangle and composites over 64
cells per side are rejected. Physical connections are distinct from teleport
portals; declaring one does not dig through blocked cells.

Piece files now save as version 2, retaining version 1 loading without ports.
A coverage mask makes unowned padding transparent during placement, including
after reload. Masks preserve complete 2x2 blocks; malformed coverage and fake
ports in uncovered cells are rejected. This fixed a black background patch found
during the first native check of offset composition. Profiles remain manual art
compatibility declarations; collision checks do not certify visual seams or
minimum passage width throughout the entire interior route.

Six new tests cover opening depth/routes, directional/width/profile/parity
compatibility, corner composition and source-cell preservation, old/new file
loading, invalid joins and blocked ports, unused enclosed connections, transparent
padding persistence and undo, malformed coverage, and real-source wall composition.
The D1006 pair composes to 4x12 cells and placing it back at (10,6) reproduces the
original source map bytes exactly. Final disposable fixtures/screenshots:
/tmp/zircon-ports-final/{wall-a.json,wall-b.json,joined-wall.json,JoinedWall.map,
editor.png,game.png}. Both screenshots were inspected; the final native editor
and isolated Test Walk exited successfully. Earlier /tmp/zircon-ports-verified
artifacts predate the padding fix and should not be used as final evidence.

Validation: **227 passed, 9 ignored**, workspace build, formatting, strict
all-target Clippy, Docker build and read-only container inspection passed.
Logs: /tmp/zircon-ports-*.log. Protocol remains 5. All changes remain local and
uncommitted. C# sources, original assets and personal saves were not modified.

Still open: a complete visually verified corner/doorway art kit, connections on
concave interior boundaries, automatic art compatibility and automatic perimeter
assembly. The connection model and pair composer are delivered; full automatic
wall/corner/doorway generation is not. See WALL_ASSEMBLY.md for the exact source
fixture and current constraints.

## 2026-09-15 — Recessed connections and three-piece composition

Resolved the previous outer-boundary-only limitation. Ports now have an inset
measured inward from their cardinal bounding edge; offset still runs along that
edge. Translation retains exact opening positions when another piece extends the
bounds around them. Boundary validation uses coverage: edge/inward cells must be
covered and walkable, and outward cells must be uncovered. Route checks cannot
use uncovered padding. Joins reject covered-cell overlaps and sealing any unused
opening; overlapping empty padding is allowed. Source pieces remain unchanged on
rejection, and completed assemblies retain one-step placement undo.

Piece files now save as version 3. Version 1 and 2 loading remains supported;
missing insets default to zero, and older versions cannot declare nonzero insets.
Tests cover version-2 ports with omitted insets as well as legacy version 1 files.
The UI exposes Inset beside Offset/Width and sketches recessed openings.

Three new tests cover recessed ownership/depth, chained composition through a
reloaded recess, overlap/sealed-opening refusal, undo, version downgrade refusal,
and an asset-backed three-piece fixture. That fixture uses undecorated D1006 floor
art, two offset 8x8 pieces and an 8x4 filler, producing 160 connected floor cells
in a 16x12 assembly. This is a composition/collision fixture, not a completed
wall/corner/doorway art kit. Native editor and client screenshots were inspected:
/tmp/zircon-recess-final/{editor.png,game.png}. The folder also contains
recess.json, filler.json, filled.json and Recess.map. The editor shows the staged
north connection at inset 4; the native map shows the completed filled assembly.
The isolated Test Walk/editor session exited successfully.

Validation: **230 passed, 9 ignored**, workspace build, formatting, strict
all-target Clippy, Docker build and read-only container inspection passed.
Final extra legacy/sealed-opening assertions passed the editor suite and Clippy.
Logs: /tmp/zircon-recess-*.log. Game protocol remains 5. Changes are local and
uncommitted; original assets, personal saves and C# sources were not modified.

Still open: a complete visually verified wall/corner/doorway kit, automatic art
compatibility and automatic perimeter generation. Recessed connections are now
implemented and should no longer be listed as unfinished. See WALL_ASSEMBLY.md
and PLANS.md for the current boundary between composition tools and art-kit
work.

## Parallel follow-up: checked starter kits and piece browser — 2026-09-15

Added real source-art starter kits and bounded join discovery. D1006 provides
the Wood north convex corner plus NW/NE approaches; `0_000` provides a separate
palace doorway retaining its wall ends, carpeted threshold and neighboring tall
art. Fingerprints reject incompatible source packs. `cave_art::validate_selection`
checks complete source sprite bounds across all layers and animation frames,
including artwork anchored outside a selected rectangle. Tests accept the three
Wood captures and reject both an inside-anchored crop and an outside intrusion.

The Wood kit uses `wood-nw` / `wood-ne` profiles and assembles its source positions
into an 8x12 piece without changing any source map bytes. The palace doorway uses
`palace-doorway`, remains a separate theme, and does not finish the Wood kit.
Exact coordinates and current limitations are in WALL_ASSEMBLY.md.

The Piece kit browser loads either starter kit or a directory catalogue, chooses
a starting piece, searches on request and builds a checked candidate. Search
limits are 16 sources and 256 matching port pairs; every result passes full
composition validation and is checked again on Build. Catalogue loading scans
at most 128 direct directory entries, orders JSON filenames deterministically,
and returns no partial results. Export serializes and validates all pieces before
creating a fresh directory, uses exclusive fixed filenames and copies no assets.
I/O failures report that the new folder may contain partial results.

Automation: `ZIRCON_BUILTIN_KIT=wood-corner` or `palace-doorway`;
`ZIRCON_PIECE_CATALOGUE=/path/to/folder` takes precedence. Full automatic perimeter
generation, automatic visual compatibility and a complete interchangeable art
kit remain unfinished.

Validation: asset-backed workspace tests passed (242 passed, 9 ignored); fmt,
strict all-target Clippy, workspace build and whitespace checks passed. Docker
build and read-only asset inspection passed. Native editor and isolated Test Walk
client/server runs exited successfully for both kits. Inspected editor and game
screenshots in `/tmp/zircon-kit-corner-final/{editor,game}.png` and
`/tmp/zircon-kit-palace-final/{editor,game}.png`. The Wood browser showed one
checked initial corner join; the palace browser showed two same-profile joins.
Source-restoration tests verify assembled Wood and captured palace placement
without changing source map bytes. Work remains local and uncommitted.

## Parallel follow-up: expanded north and checked south kits — 2026-09-15

Three parallel investigations checked NE continuation, NW repeatability and
another corner orientation. The north built-in now has five pieces: NW
approach, north corner, extended NE approach, authored NE bend and terminal
body. The result occupies a 14x14 envelope at source (14,2). A separate
three-piece south kit occupies 14x24 at (20,26), preserving the south corner,
adjoining wall sections and tall decoration. Both assemblies restore the
entire original D1006 map byte-for-byte. The south SE capture includes row
26 to retain an animated frame extending above the static sprite bounds.
Native inspection caught an incomplete circular floor motif despite passing
individual sprite-bound checks. The final south selection extends the SE
section to six columns and adds a covered 2x2 cap to the corner, preserving
all 72 motif cells and nine animated anchor columns. Regression checks now
cover the complete group. The result has 280 covered cells and 182 connected
walkable cells. It is an authored decorated assembly, not a repeatable body.
North interior joins were independently inspected; its outer segments are
selection boundaries, not finished wall endcaps.

NE bend/body profiles constrain that authored sequence. Extending its
terminal wall collar cuts shrine artwork anchored outside the selection;
the attempted NW diagonal collars also intersect neighboring decoration.
No unverified repeatable wall body was added. Exact rectangles, connections
and blockers are recorded in WALL_ASSEMBLY.md. Full perimeter generation,
arbitrary repetition and a compatible Wood doorway remain unfinished.

The browser can add an independent current-piece copy with edited name,
type and connections, or remove an entry. Successful edits invalidate cached
join results; failed additions preserve the catalogue and previous results.
The cap remains 16 entries, with new-folder export for persistence.
`ZIRCON_BUILTIN_KIT=wood-corner` continues to select the expanded north kit;
`wood-south-corner` selects the south kit. Palace remains a separate theme.

Focused source-backed north, NE and south tests passed. Disposable source
fixtures are `/tmp/zircon-extended-north-final` and
`/tmp/zircon-south-corner-complete`; the older south-corner-final fixture
predates the motif fix and is not final evidence.

Final validation: 249 workspace tests passed, 9 ignored, with real assets and
local socket tests enabled. Workspace build, fmt, strict all-target Clippy and
whitespace checks passed. Docker build and read-only asset inspection passed.
Both assembled selections were loaded in the native editor and isolated Test
Walk client/server; all processes exited successfully. Inspected editor and
game screenshots under the two final fixture directories above. The corrected
south editor screenshot shows the whole circular motif. Full perimeter synthesis
remains open; these are authored wall groups with reviewed internal joins.
Changes remain local and uncommitted.

## Parallel follow-up: whole-kit assembly — 2026-09-15

Parallel work added bounded whole-kit composition, checked its integration and
investigated a compatible Wood doorway. **Assemble whole kit** starts from the
first catalogue entry and uses each of at most 16 entries once. Every step
requires exactly one fully valid next join among unused entries. Ambiguity,
a stalled route or exhaustion of the shared 256-trial discovery budget fails
without replacing the current piece with a partial result. Chosen joins receive
up to 15 additional revalidations (271 attempts total). Success creates a new current piece;
it changes the map only when placed.

Catalogue **↑** / **↓** controls reorder entries, invalidate cached candidates
and preserve that order on export. `ZIRCON_ASSEMBLE_KIT=1` invokes assembly after
built-in or folder autoload. Real-source north and south assembly tests passed,
including byte-exact restoration of the original map artwork and collision.
Disposable fixtures are under `/tmp/zircon-auto-assembly-final`.

The doorway investigation added no unverified piece: D1006 has a freestanding
spur and stair boundary, D10032 needs a complete room/corridor junction group,
and newly inspected D1012 is an open decorated chamber. Native D1012 evidence
is `/tmp/zircon-door-D1012.png`; detailed limits are in WALL_ASSEMBLY.md.

At this point automatic perimeter generation and a compatible Wood entry were
still open; the next follow-up below supersedes those limits. The original
whole-kit workspace run caught a stale candidate-limit error-message assertion
(`256 connection pairs`); the assertion was corrected to match the shared-budget
message, without weakening the search bound. Do not treat that historical failed
run as a passing final workspace result. Source assembly fixtures passed;
consolidated validation is recorded with the subsequent work.

## Parallel follow-up: generated Wood wall rings and MCP authoring — 2026-09-16

D1008 now supplies four repeatable diagonal bodies and four compatible convex
corners. Source fingerprints, complete frame/selection bounds and native
body/corner seam inspections passed. The new simultaneous composer validates
fixed placements and consumes all explicit seams together, closing cycles while
preserving strict pairwise join behavior. Five focused composition tests cover
cycle closure, one undo, unchanged inputs, padding/recesses, invalid placements,
duplicate or missing seams and disconnected floor.

**Wood wall perimeter** generates new closed wall rings from NW/NE counts 3–10,
with SE=NW+2 and SW=NE, then fills unowned interior blocks with source floor.
Outside remains blocked. All 64 count pairs pass source-backed generation checks.
The UI exposes this mode separately from copying an entire source chamber.
Saved `wall_repeats` metadata preserves the counts and reloads the mode;
legacy layouts load without it. Enclosed-layout revalidation rejects a connected
walkable breach to the map border. New metadata/enclosure regressions passed.

A separate D10032 entry group has a 16x38 shaped envelope, 380 owned cells and
111 connected walkable cells. Twelve wall fingerprints and coverage checks
retain the room exit and corridor turns. Two source-backed tests pass, including
exact source restoration, roundtrip and rejection of a removed wall-tip block.
Native source comparison and the exported entry were inspected successfully.
Final entry fixtures are `/tmp/zircon-wood-entry-verified`; the earlier
`zircon-wood-entry-final` preview used an unsuitable water background and is not
the final visual artifact. Corner/body fixtures are under
`/tmp/zircon-perimeter-kit-verified` and `/tmp/zircon-wall-bodies-verified`.

A second D1008 **Wood wall junction** is now captured in `wood_wall_entry.rs`
and available in the kit browser as `wood-wall-junction`. Its shaped 20x32
source envelope (92,182)–(112,214), half-open, retains both authored wall turns,
small complete floor decorations and the physical passage between them. Four
explicit seams match the checked diagonal bodies: west NE offset10, recessed
east NW inset4/offset8, east SE offset10, and east NE offset30. Source capture
restores the original bytes; simultaneous attachment of all four body pieces
preserves source collision. Native source and isolated captures passed under
`/tmp/zircon-wall-junction-verified`. Surrounding room geometry and floor fill
are required; this group is not automatically substituted into a ring.

Two 6x10 D1008 concave turns now have passing source/body collision tests and
native inspections: the valley transitions NE to NW, and the notch SE to SW.
The full groups have 12 and 24 connected walkable cells respectively. Their
fixtures and native valley/notch previews are under
`/tmp/zircon-concave-verified`. These parts now feed the connected-room recipe
described below.

Whole generated rings were inspected in the native editor and client at two
sizes, 48x54 and 58x64. Their walls and corners are continuous, interiors filled,
and client spawns valid. Evidence is
`/tmp/zircon-perimeter-first/{3-3,5-6}-{editor,game}.png`. Those captures predate
the final generator labels, so they establish map geometry/art rather than the
finished UI text.

The editor's twelve stdio MCP authoring tools include `world_perimeter`,
`map_copy_area`, `world_spawn`, draft variants and playable bundle population.
A real-process smoke run passed 41 JSON-RPC requests covering all twelve tools
under `/tmp/zircon-connected-mcp`; `smoke-report.json` records status
`passed`, the exact tool list and unchanged-source checks from the smoke run.
Read [MCP_AUTHORING.md](MCP_AUTHORING.md) for setup and workflows. Original sprite
creation, animation contracts and PNG/sprite-sheet import remain planned in
[PLANS.md](PLANS.md). Existing recolour/rescale
exports do not constitute new animated-art authoring.

**Wood connected rooms** is now implemented in `wood_connected.rs` and the UI.
Six convex and two concave corners enclose two rooms with a real 6x4 floor
passage. NW/NE counts each accept 3–6, and all 16 pairs pass source-backed
connectivity and enclosure checks. Tests also block the passage column and
verify it separates two substantial rooms, proving this is a physical connection
rather than a dependency on teleport metadata.

`ZIRCON_CAVE_MODE=wood-connected` selects the mode. `wall_rooms` and
`wall_repeats` retain the room count and dimensions through saving/reloading;
legacy layouts default to zero room metadata. MCP `world_perimeter` accepts
`rooms: 2` and defaults to one room. The connected bundle's native editor/client
views were inspected under `/tmp/zircon-connected-mcp/connected/{editor,game}.png`.
The maximum 6/6 case also passed native inspection at
`/tmp/zircon-connected-final/{editor-6-6,game-6-6}.png`, with the player standing
in the physical passage and both rooms visible on the minimap.

The real-server integration test in `mir-server/tests/connected_rooms.rs` now
logs in an actor and observer, then verifies 16 authoritative movement steps
from (32,31), through passage column 40, to (48,31). The private database has
all teleport and monster-spawn records disabled, and source map/database bytes
stay unchanged. The first harness incorrectly expected self-movement echoes;
the corrected observer checks the server broadcasts used by other players.

The bounded Wood wall-assembly phase is complete, including physical entry
integration, source-art preservation, playable export, native review, and MCP
access. Arbitrary BSP/organic perimeter artwork, movable door objects and original
sprite creation remain separate future capabilities. Changes remain local and
uncommitted.


Final verification for the Wood wall-assembly phase:

- Workspace formatting, strict all-target Clippy and workspace build passed.
- Asset-backed workspace tests passed: **289 passed, 9 ignored**, including the
  real connected-room traversal and all three existing socket integration tests.
  Log: `/tmp/zircon-connected-final-tests.log`. Local socket tests require socket
  permission; an earlier restricted run failed at binding, then passed with it.
- All 64 single-room and 16 connected-room count pairs preserve full source wall
  groups and pass collision/enclosure checks.
- Native editor/client review passed for single-room sizes 3/3 and 5/6, and
  connected sizes 3/4 and 6/6. Full connected overview and in-passage client views:
  `/tmp/zircon-connected-mcp/connected/{overview,neck-game}.png`.
- The real MCP process passed 41 requests covering all 12 tools, with source
  hashes unchanged: `/tmp/zircon-connected-mcp/smoke-report.json`.
- Server Docker build and read-only asset inspection passed; logs are
  `/tmp/zircon-authoring-docker.log` and `/tmp/zircon-authoring-inspect.log`.
  Subsequent changes affect editor authoring and the integration harness only.

To reproduce the physical traversal fixture and check:

```sh
cd rust
DEVELOPER_DIR=/Library/Developer/CommandLineTools \
  ZIRCON_ASSETS="$HOME/zircon-assets/Client" \
  ZIRCON_CONNECTED_QA_DIR=/tmp/zircon-connected-check \
  cargo test -p mir-editor wood_connected::tests
DEVELOPER_DIR=/Library/Developer/CommandLineTools \
  ZIRCON_ASSETS="$HOME/zircon-assets/Client" \
  ZIRCON_CONNECTED_MAP=/tmp/zircon-connected-check/connected-3-4.map \
  cargo test -p mir-server --test connected_rooms -- --nocapture
```

## Completed: first original sprite-authoring workflow

Delivered PNG/reference export, lossless preservation of unchanged compressed
surfaces, explicit image/sheet imports, new-frame canvases, shared runtime
animation contracts, editor playback preview, contact sheets, validated library
builds and private new-appearance publication. MCP now exposes20 tools including
8 sprite operations. [SPRITE_AUTHORING.md](SPRITE_AUTHORING.md) is the usage guide.

The generated Jade Beetle Ring (ItemInfo1168/Image7547) and Jade Beetle
(MonsterInfo360/Image28) are published together in a private asset view. The
64-pose source atlas fills232 required slots with explicit key-pose holds,
including eight corpse slots missing in the original Beetle reference. All frame
surfaces pass strict contract validation and codec rebuilding. This is a new-art
workflow demonstration, not a claim that its short held-pose animation equals
polished production animation. Original images, database records and unrelated
compressed slots remain unchanged.

Reproducible generation: `rust/scripts/sprite_demo.py`; successful bundle and
report: `/tmp/zircon-sprite-demo-verified`. Native `game.png` shows the new ring
equipped and a live Jade Beetle near the player; `editor.png` shows its generated
connected-room map. The private view must be used with the matching database.

Validation passed:

- Formatting, strict workspace all-target Clippy and workspace build.
- **308 tests passed, 9 ignored**, with installed assets and actual socket tests.
  Log: `/tmp/zircon-sprite-final-tests.log`.
- All8 sprite MCP tools exercised over real stdio in16 requests:
  `/tmp/zircon-sprite-smoke-final/sprite-smoke-report.json`.
- Complete new-art item/monster/map workflow passed12 additional requests:
  `/tmp/zircon-sprite-demo-verified/demo-report.json`.
- Server Docker build and read-only asset inspection passed:
  `/tmp/zircon-sprite-docker.log`, `/tmp/zircon-sprite-inspect.log`.

During integration, strict tests caught missing source Skeleton frames, partial
alpha in a DXT1 import, selected-range versus whole-library count mismatches and
case-sensitive override matching on macOS. These were resolved explicitly:
authored corpse frames, opt-in binary alpha, selected-slot validation, and actual
filesystem spelling with no writes through source symlinks. Atlas edge cells are
transparently padded rather than dropping source pixels; a regression covers it.

The user requested stopping after this current task to conserve their weekly
allowance. **Stop here; do not start another phase automatically.** All changes
remain local and uncommitted. Model advice was to retain Astra for this connected
integration task and consider Sol for later routine work; no model switch occurred.

## Item-size follow-up after the user resumed

The user asked to continue and noted small items and different clothing artwork.
The imported Jade ring inherited a32×22 canvas and fitted the full generated PNG,
including transparent margins. A deterministic import regression reproduced an
8-pixel shape shrinking to1 visible pixel. Explicit `trim_transparent` fitting
now uses the nontransparent bounds; binary-alpha thresholding occurs first when
requested. Empty visible art and trimming without fit are rejected atomically.

Single-image MCP imports now accept an explicit new canvas with required offsets;
the editor exposes these controls for item icons. The demo uses32×32, offsets
[-16,-16], and visible-art fitting. Monster UI imports retain their frame anchors.
The publisher continues to copy worn Shape, and its UI now says that icon changes
do not change worn clothing/weapon artwork. Separate clothing body contracts and
equipped animation review are recorded as the next wearable-art slice.

Validation:135 editor tests pass with installed assets, strict workspace all-target
Clippy and editor build pass, and the12-request new-art demo succeeds under
`/tmp/zircon-icon-scale-demo`. Regression logs are
`/tmp/zircon-icon-scale-{red,green,tests,clippy}.log`. No renderer, server or original
asset changes were made. Latest broad workspace/Docker checks remain the prior
sprite-phase results; this follow-up changes only editor authoring and its demo.
Native review passed: `/tmp/zircon-icon-scale-demo/game.png` shows the new ring
filling its equipment cell at a readable size, alongside live Jade Beetles.
The earlier tiny-ring capture remains under `/tmp/zircon-sprite-demo-verified`
for comparison. Source appearance and rendering behavior remain unchanged.
The game screenshot completed, but the parent editor did not auto-close for its
scheduled capture in this run. Its specific editor/server processes were stopped
after review; no parent-editor screenshot is claimed for this follow-up.

## Authorized parallel follow-up — 2026-09-22

The user requested clothing, then maps, then sprite production and gameplay
polish, with parallel work where possible. Earlier stop instructions were revoked
by this explicit request. Changes remain local and uncommitted.

Wearable projects use kinds armour-male/female and armour-assassin-male/female,
with source_image interpreted as Shape. Shared wearable_animation maps the
client and editor, fixing existing Assassin signed offsets and normal mounted
shift. Assassin Cast2 had no complete reference block; it explicitly aliases
Cast1's valid five-frame body pose. All other mapped body actions remain exact.
Full required normal/Assassin source slots decode successfully. Existing layers
and renderer quality are preserved.

wearable_publish validates all allowed gender/class families, clones an item and
stats into an unused armour Shape, and writes a private Client/System.db revision.
An asset-backed test caught over-reserving weapon Shape IDs; allocation now
reserves only the armour namespace. The four-family MCP run passes export, tint,
validation, publication, family mismatch rejection and atomic batch build.
Latest script: rust/scripts/wearable_smoke.py; report /tmp/zircon-wearable-final.
Native source-garment colour variants were verified for all four families at
/tmp/zircon-wearable-smoke3/armour-*.png. The first mounted attempt correctly
failed because generated maps prohibit riding; private QA snapshots then enabled
CanHorse, and replacement mounted captures show the actual horses and body art.
No installed asset or database was modified. Temporary native processes exited.

Wood generation now supports 36 two-room size pairs (sum <=13, each >=3) and
three three-room pairs (3/3,3/4,4/3), within64placements. Both narrow passages in
the three-room map passed16 acknowledged server/observer steps each with no
teleports: /tmp/zircon-three-neck-{0,1}.log. All39 connected and64 single-room
layouts pass strict floor constraints. Learned directional constraints govern
complete floor patches only; wall-collar seams retain existing authored art.
Organic/BSP generation keeps its old behavior because source adjacency does not
always admit a valid tiling. Bounded strict solving fails explicitly.

Castle gate authoring creates/relocates genuine castle gates with closed-cell
bounds/collision/overlap checks. The shipped DB lacks CastleGateInfo and gate
monster records, so a validated Collection constructor and exact C# schema are
used to add private records. Optional gate_image540..543 clones a source monster
and stats into a separate gate appearance; no source reskin, copied drops or
spawns. UI and MCP expose previews and private output. Runtime tests verify
open/close collision across the full footprint. Fixture:
/tmp/zircon-gate-authoring-final; MCP creation/relocation passes7requests at
/tmp/zircon-gate-mcp. These remain castle gates, not ordinary Wood room doors.

Sprite production adds layer-specific all-frame tinting (alpha/anchors/shadows
preserved) and sprite_build_batch (1–32 projects, atomic pack set plus versioned
project-set.json). Existing sprite MCP smoke and all original map/database MCP
operations still pass at /tmp/zircon-sprite-next and /tmp/zircon-mcp-next.

Gameplay corrections include Frost Bite targeting/Slow/IceAttack, Cursed Doll
hostility/boss/link/reagent checks, Soul Resonance reciprocal cleanup and target
HP bonus, Summon Dead probability/reagents, Hurricane focus affinity/channel/mana,
Mirror Image equipped focus/one decoy/stationary invulnerable area spells, and
Dragon Blood automatic poison-gated charges. Reference quirks remain explicit,
including Mirror's Ice damage for every affinity. See skills-remaining.md.
Ice Aura, Burning Fire, Dark Soul Prison and Death Cloud use real C# field art;
38 frames decode. Settings has persistent sound channels/mute and live gain
updates. Native Settings screenshot: /tmp/zircon-settings-qa/settings.png;
visual verification only, persistence/gain tested separately, no audible test.

Consolidated baseline after main changes:327passed/0failed/9ignored, including
real socket/load tests. Docker build and read-only inspect pass. Logs:
/tmp/zircon-next-workspace-tests.log, /tmp/zircon-next-docker.log,
/tmp/zircon-next-docker-inspect.log. Later gate/UI checks supplement this baseline;
final totals are recorded below when finished.

Native gate QA exposed two existing bugs: gates used ordinary monster idle
frames, and closing selected direction 4 instead of the closed damage stage.
The shared SabukGate profile now supplies one-frame idle poses, seven-frame
opening/reversed closing, and the reference death poses. Closed directions are
0–2; open is 7. Client and server regressions cover these mappings. Corrected
native captures are /tmp/zircon-gate-native-corrected/{closed,open}.png.
The private QA map uses daylight for visibility. Placement demonstrates valid
gate behavior near Sabuk's doorway, not exact architectural alignment.

Final verification: 337 workspace tests passed, zero failed; all nine normally
ignored developer reports also passed when run explicitly. The three socket/load
tests are included. Logs: /tmp/zircon-next-final-workspace-tests.log and
/tmp/zircon-next-final-ignored-tests.log. Workspace build, strict all-target
Clippy, formatting and diff whitespace checks passed. Final Docker rebuild and
read-only inspection passed (/tmp/zircon-next-final-docker*.log).

Gate sprite authoring now shares the special runtime profile: all four source
orientations contain all 25 required unique slots. Publishing keeps gate and
ordinary monster appearance allocation separate and preserves source art and
FaceImage orientation. Final MCP gate smoke passes nine requests, including
creation, relocation, export and sprite publication, at
/tmp/zircon-gate-final-mcp. Earlier sprite/map/wearable MCP runs remain valid.
Remaining larger map and sprite scope is listed in PLANS.md; this batch
does not claim arbitrary layouts or automatic creation of polished new motion.

## Varied rooms, static thresholds, and painted outfits (2026-09-23)

Wood connected rooms now accept independent side repeat pairs for each of two
or three rooms. The 64-placement bound and checked source corner orientations
still apply. Saved layout metadata restores varied room controls in the editor;
MCP world_perimeter accepts `repeats:[[nw,ne],...]`. It reports each static
doorway's center and checked six-by-four walkable footprint. The source-art
Wood wall junction can also be stamped manually. Private varied bundle and
native editor preview: `/tmp/zircon-door-threshold-qa`.

Asset checks cover diverse varied outlines, source art, closed borders, neck
separation, metadata and invalid requests. Real server/observer socket runs
crossed both nonuniform room necks with 16 acknowledged steps each, no portal:
`/tmp/zircon-varied-neck-{0,1}.log` and the MCP bundle door tests. Source D1008
and installed database hashes stayed unchanged.

Sprite projects now support bounded polygon painting, canvas padding,
underpainting and erasing as private revisions. The four-family cape demo paints
all 3,152 required body frames and varies hems by animation frame. Four private
items publish and validate. Four native standing and four mounted captures,
plus a four-frame motion strip, are in `/tmp/zircon-cloak-demo-1`; the generated
world DB alone enabled riding for mounted QA. Original assets stayed unchanged.

This does not supply T/Y branching or arbitrary organic Wood boundaries. A
targeted scan of 23 Wood-motif maps found only the already checked concave turn
families; composing a safe new branch requires additional authored matching
corner art. Static room passages have no open/close action: the reference game
only defines movable castle gates. The polygon cape is a working silhouette
and motion example, not polished bespoke garment art.

Final 2026-09-23 checks: workspace build, formatting, strict all-target Clippy,
and diff whitespace checks pass. Asset-backed workspace tests pass 342 active
tests with zero failures; nine server developer reports are ignored by the
normal test command and passed in the preceding explicit run. Local socket
permissions are required for three integration tests, which pass in the final
run. Logs: `/tmp/zircon-remaining-final-{build,clippy,tests}.log`. The original
MCP process smoke also passes after these changes with 41 RPC requests:
`/tmp/zircon-remaining-final-mcp.log`.

## Interactive room doors and branched Wood maps (2026-09-23)

Ordinary Wood thresholds can now receive a private, server-authoritative room
door through MCP `room_door_place`. The door starts closed on load, blocks its
eight-cell cut, opens on a nearby click, rejects distant toggles and closing
over an actor, and resets closed on reload. It uses existing Sabuk wooden gate
art through a separate room-door role, without castle ownership or combat.
The bounded `.doors.json` sidecar follows map copies and populated world
exports; invalid copied door footprints are rejected. Real client/server
socket coverage and a native closed-door capture are in
`/tmp/zircon-door-threshold-qa`.

MCP `world_branch` now exports a private T-shaped Wood map with independently
bounded west, north and east arms. Its 64 new wall images are appended to a
private Wood library derived from source textures; source assets stay intact.
The map has four rooms and three checked walkable passages. A real socket test
crossed all three branches with 36 acknowledged steps; fixture and native
capture are in `/tmp/zircon-wood-branch-qa`. This is a functional draft of
branch art. Arbitrary graph/Y/organic layouts and polished seamless wall art
are still outside this bounded generator.

The first final test run exposed a parallel-test-only temp directory collision
in the editor catalogue fixture. Its directory names now include a process
counter. The complete asset-backed workspace rerun passes 348 active tests,
zero failures, with nine developer reports ignored as expected:
`/tmp/zircon-all-remaining-tests.log`. Workspace build, strict all-target
Clippy, formatting and diff whitespace checks pass. The final MCP process
smoke passes 46 requests, including private door and branch authoring, at
`/tmp/zircon-final-remaining-mcp.log`. Docker rebuild and original-data inspect
pass at `/tmp/zircon-doors-final-docker*.log`.

## Connected Wood graph and authored sprite motion (2026-09-23)

MCP `world_branch` now accepts T, Y, cross, and 2–12-room connected Wood graphs
on a 5×5 grid, including cycles and indented exposed room edges. Graphs that
fit the original 3×3 grid retain their 112×112 map size; larger graphs use
176×176. The private Wood library contains 64 source-textured wall frames with
matching decoded tile rims. A 12-room, 17-link graph passes asset-backed
enclosure, route and layout checks. Native wall captures remain functional
draft art with repeated square motifs, not finished scenery.
The real server/observer also crossed a 60-cell route and a 120-cell cycle in
the 12-room graph (`/tmp/zircon-wood-art-review/grid12-socket.log`); native
capture: `/tmp/zircon-wood-art-review/grid12-native.png`.

`room_door_place` builds private Wood plank door frames in `Mon-54.Zl`; native
closed/open captures are in `/tmp/zircon-wood-door-native-qa`. Source libraries
are untouched. The map MCP tools now preserve private art across flat map
copies, generated/perimeter/populated bundles, branched worlds and chamber
generation from a private source map. Population
rejects a room door whose source monster is missing from the chosen database.
Database-only `draft_variant` and `world_spawn` revisions also retain private
art; `world_spawn` returns the matching `ZIRCON_ASSETS` launch path.
The focused art-chain test and 67-request stdio MCP smoke pass; the latter is
recorded at `/tmp/zircon-mcp-final-map-chain.log`.

The sprite editor has zoomed painting, erasing, underpainting, padding and a
previous-frame guide. MCP `sprite_paint_motion` interpolates artist-supplied
keyframe polygons within a contiguous action/direction; it does not generate
poses. The three-frame armour process smoke passed 19 requests at
`/tmp/zircon-motion-process-smoke2.log`. Four published garment examples cover
all 3,152 required body frames. Standing and mounted native captures are in
`/tmp/zircon-outfit-demo-4`. Fabric folds and shading are visible at world
scale; art remains procedural rather than hand-painted.

Final asset-backed workspace suite: 356 passed, zero failed, nine ignored
developer reports (`/tmp/zircon-final-tests3.log`). Workspace build,
strict all-target Clippy, formatting and diff whitespace checks pass at
`/tmp/zircon-final-{build3,clippy3}.log`.
