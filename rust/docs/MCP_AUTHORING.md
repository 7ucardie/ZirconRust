# Editor authoring through MCP

The editor exposes its map and database authoring operations over local MCP stdio. Each editing tool writes a new revision directory. Source maps, installed assets, source databases and previous revisions stay unchanged. The server has no HTTP listener and does not connect to a running game server or publish changes to production.

## Launch and client configuration

Build with `cargo build --manifest-path rust/Cargo.toml -p mir-editor`. On a Mac with the Command Line Tools workaround, prefix the command with `DEVELOPER_DIR=/Library/Developer/CommandLineTools`.

Configure an MCP client to launch this executable. Substitute absolute paths; the following is a common client configuration shape, not a file this repository installs automatically:

```json
{
  "mcpServers": {
    "zircon-authoring": {
      "command": "/absolute/path/Zircon/rust/target/debug/mir-editor",
      "args": [
        "--mcp", "--project", "/absolute/path/zircon-authoring-project",
        "--assets", "/absolute/path/zircon-assets/Client",
        "--db", "/absolute/path/zircon-assets/Database/System.db"
      ]
    }
  }
}
```

The project parent must already exist. The project may be new or existing but must be outside the installed Client assets directory. Standard output contains only newline-delimited JSON-RPC messages; diagnostics use standard error. The connection implements MCP `2025-11-25`, including initialization, the initialized notification, ping, tool listing and tool calls. Input messages are limited to 1 MiB. These conventions follow the official [stdio transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports), [lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle), and [tools specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).

## Paths and revisions

Map inputs use `asset:D1006.map` for the installed `Client/Map` directory or `project:revision/Map.map` for an authored revision. Database inputs optionally use `project:revision/System.db`; omitting `database` selects the read-only source database. To build on a previous database change, pass its revision explicitly.

An `output` value is a fresh directory name of 1–64 ASCII letters, digits, underscores or hyphens. Existing destinations are refused. Input paths reject traversal and symlinks escaping their selected directory. Layout and navigation sidecars are bounded, checked and confined to the map directory. Validation failures produce no revision; a disk-write failure can leave a partial new directory, reported in the tool error.

## Tools

The map/content tools below and [sprite-authoring tools](SPRITE_AUTHORING.md#mcp)
are available. `tools/list` gives the current complete list.

| Tool | Result |
| --- | --- |
| `content_search` | Search item, monster or map names; returns IDs and a bounded result list. |
| `draft_variant` | New monster or equipment variant and full database snapshot. Uses existing level estimators; retains source appearance. Monster drops are not copied. |
| `item_exterior_effect` | Set `ItemInfo.ExteriorEffect` on a project database item in a new revision. Use an effect ID from C# `ExteriorEffect` after `draft_variant`, `wearable_publish`, or `equipment_publish`. |
| `map_inspect` | Dimensions, walkable count and optional cell references/collision. |
| `map_create` | Blank even-sized map, optionally containing an assembled Wood north/south, Wood room/corridor entry, Wood wall junction, or palace doorway kit. |
| `map_copy` | New map revision retaining valid layout, navigation, room doors and private art. |
| `map_copy_area` | Copy a complete source-art rectangle to a target map, preserving all copied cells and target metadata. |
| `map_collision` | Explicit walkability edits; refuses invalid generated layout or blocked portal endpoints. |
| `map_stamp` | Place a complete assembled source-art kit with placement and navigation validation. |
| `room_door_place` | Add a clickable Wood room door at a checked connected-room threshold in a new playable bundle. |
| `world_from_brief` | One-call playable chamber, organic cave or 2–12-room Wood dungeon from a simple English brief with one installed monster. |
| `world_generate` | Playable chamber, organic cave or room-dungeon bundle with private wall art where generated, safe zone, portals and eight selected-monster spawns. Optional `rooms: 2–12` for room dungeons (default 6). |
| `world_branch` | Playable T, Y, cross, or connected Wood room graph of up to 12 rooms on a 5×5 grid, with private wall art and optional organic room edges. |
| `world_perimeter` | One enclosed Wood room, or two to three joined rooms with uniform `nw`/`ne` or varied `repeats`, source wall art, checked static thresholds, and a playable database bundle. |
| `world_populate` | Build a playable bundle around an existing connected map. Retains layout, room doors and private art; seed is used only when no layout exists. |
| `world_spawn` | Add a monster population at chosen walkable points to a new database revision for an already registered map. |
| `equipment_publish` | Publish edited weapon, helmet or shield projects to an unused equipped Shape in a private item and asset revision. |

The current tool schemas returned by `tools/list` define exact arguments. Successful calls return both a text JSON representation and structured content. Tool validation/execution failures return `isError: true`; malformed protocol requests return JSON-RPC errors.

## Natural-language map brief

Call `world_from_brief` with `{"brief":"six-room Wood dungeon with chickens","output":"chicken-world","seed":42}` to create a private playable bundle in one tool call. It resolves the exact installed monster name, accepts simple plurals, and returns the interpreted mode, room count, monster ID, source, and bundle paths. It also accepts “organic Wood cave with Chicken” and “Wood chamber with Chicken.” Size defaults to 80×80 cells; `width` and `height` are optional. Unsupported descriptors, extra monsters/items, ambiguous names, and unknown monsters fail before writing a revision. This direct tool uses a deliberately small grammar; it does not use an AI model.

The `design_sprite_motion` prompt guides a host AI through generating and
importing new pose PNGs with the exact frame contract, validation and native
review. The host needs an image-generation tool; the editor does not call one.

MCP clients with prompt support can select `create_world` from `prompts/list`
and enter an ordinary-language `request`, for example “a mossy Wood maze with
six rooms and chickens.” The prompt tells the client's AI to search installed
content, choose a supported generator, call the authoring tools, inspect the
result and return its private map/database/assets paths. The editor itself
does not call an AI provider; `prompts/get` supplies the workflow to the
client's model. Prompt retrieval creates no files. Generation still runs
through the validated `world_*` tools and creates private revisions. Clients
without MCP prompt support can follow the workflow below manually. See the
[MCP prompts specification](https://modelcontextprotocol.io/specification/2025-11-25/server/prompts).

## Minimal workflow

1. Call `content_search` with `{"kind":"monsters","filter":"Chicken"}` and select the exact source ID returned.
2. Call `draft_variant` with `kind: "monster"`, that `source_id`, `level: 20`, a new `name`, and `output: "monster-v1"`.
3. Call `world_generate` with `source: "asset:D1006.map"`, `mode: "chamber"`, `width: 80`, `height: 80`, `seed: 42`, the returned `created_id` as `monster_id`, `database: "project:monster-v1/System.db"`, and `output: "world-v1"`.
4. Read the resulting bundle's `README.txt` for server/client launch settings. Open its map in the editor for visual review and Test Walk. Tools return the output directory; bundles place maps under `Map/`, and the generated map filename can receive a numeric suffix to avoid an existing database filename.

To generate a new closed wall perimeter, call `world_perimeter` with `nw: 3`, `ne: 4`, `seed: 23`, the chosen `monster_id`, `database: "project:monster-v1/System.db"`, and `output: "perimeter-v1"`. The two repeat counts each accept 3–10 and determine room dimensions; the checked recipe derives opposite wall lengths and joins all four corners. Add `rooms: 2` for two rooms joined by a physical walkable neck; this recipe accepts repeat counts 3–6. `rooms` defaults to 1. The bundle retains `wall_repeats` and `wall_rooms` metadata through later copying and population.

Chamber mode preserves an entire authored source chamber. `world_perimeter` assembles one to three enclosed Wood rooms. Organic and room modes now frame every exposed floor boundary in a private Wood library; room corridors receive passable threshold strips. Use `asset:D1008.map` as their source: D1006 lacks the clear 2×2 floor patches needed to sample floor art. Set `ZIRCON_ASSETS` to the returned private `assets` path. Desktop Tiles uses the same full-art generators and keeps their library through preview, save/reopen, export and Test Walk. Map kits are authored groups, not arbitrary new sprites. The sprite workflow is in [SPRITE_AUTHORING.md](SPRITE_AUTHORING.md).

`world_populate` retains enclosure, rooms, corridors, spawn regions, portal routes and other existing generated layout fields. A map with standalone navigation but no matching layout is refused for population rather than replacing its portals. Copying/stamping/editing such a map is supported with portal checks.

For arbitrary source art, call `map_copy_area` with `source`, target `map`, `bounds: [left, top, right, bottom]`, target `x`/`y`, and a fresh `output`. Bounds are half-open, even, and at most 64 cells per side; target coordinates must be even. The tool checks all sprite-frame extents, including source art anchored outside the rectangle, before copying. Larger drawings made from several separate sprites still need visual review. The `wood-entry` kit preserves a checked room/corridor passage as one group; it does not imply compatibility with every other Wood wall seam.

To place mobs at exact authored locations, first finish map edits, then use `world_generate`, `world_perimeter` or `world_populate` to register the map. Search that bundle's `MapInfo` with `content_search` to obtain its `map_id`. Call `world_spawn` with the immutable map path, matching `map_id`, project `database`, `monster_id`, `points`, `count`, `delay_seconds`, and a fresh `output`. Select 1–128 walkable points, a total population of 1–100 monsters across those points, and a delay of 1–3600 seconds. Safe-zone regions, portal triggers and landings are excluded using both provided database records and attached map metadata.

`world_spawn` writes a new `System.db` and carries private art when the map or
source database has a private `Client`. Its response identifies the existing
map dependency and provides database/map-directory launch settings. An explicit
`assets` path selects a combined private view when map and database art differ.
`draft_variant` also carries the source database's private art. Map tools
validate their attached layout/navigation metadata; they do not inspect
arbitrary external database revisions. Later map edits are not automatically
linked to earlier database registrations or spawn regions. Revalidate and
recreate the registration/spawns against the finished map revision before
launching it.

## Reproducible process smoke test

Python 3.9 or newer is required. This launches the real binary, checks the core authoring tools, creates chained monster/item database drafts, copies and edits maps, builds and repopulates a playable chamber and single-room and physically connected two-room wall perimeters, adds explicit spawns, validates sidecar checksums, and verifies source hashes are unchanged:

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools python3 rust/scripts/mcp_smoke.py \
  --binary rust/target/debug/mir-editor \
  --assets "$HOME/zircon-assets/Client" \
  --db "$HOME/zircon-assets/Database/System.db" \
  --project /tmp/zircon-mcp-smoke-new
```

Use a new project path for each run. The script keeps revisions, `smoke-report.json`, and `smoke.stderr.log` for inspection. It does not launch the graphical client; native Test Walk is a separate visual verification step.

## Clothing, batch packs, and castle gates

The additional tools are `wearable_publish`, `sprite_tint`, `sprite_build_batch`,
`castle_gate_preview`, and `castle_gate_place`. See [sprite authoring](SPRITE_AUTHORING.md)
for the four body-family export kinds and complete-family publication rules.

`world_perimeter` accepts `rooms: 3`. For uniform sizes, two-room repeats are
at least 3 and sum to at most 13; three-room repeats sum to at most 7. All fit the existing
64-piece limit. Generated Wood floor patches obey directional adjacency learned
from D1008; the bounded solver fails explicitly if it cannot find a tiling.
This constrains floor-to-floor joins, not the preserved wall-collar seams.
Organic/BSP drafts retain their existing floor selection.

`world_branch` accepts `seed`, `monster_id`, `output`, optional `database`, and
three layout forms. The default T uses optional `arms:[west,north,east]`;
`shape:"y"` uses `[northwest,northeast,south]`, and `shape:"cross"` uses
`[west,north,east,south]`. Each arm is 2–8 floor blocks. For a custom connected
graph, pass 2–12 unique `nodes:[[column,row],...]` on a 5×5 grid and 1–20
`links:[[node_index,node_index],...]` between cardinal neighbors. Cycles are
allowed. `organic:true` indents exposed room edges without blocking passages.
The response returns room centers and a playable map/database bundle. Its 256 wall frames
are appended to a **private** `Client/Data/Map Data/Wood/Dungeonsc.Zl`; set
`ZIRCON_ASSETS` to the returned `assets` path when launching the client. The
original library and D1008 map remain unchanged. The wall masks use the
painted Wood atlas. Review placement in the editor and native client before
shipping a level.
If the selected `database` has a sibling private `Client`, `world_branch`
inherits its sprite art. Pass `assets:"project:revision/Client"` explicitly
when the content database and private assets come from different revisions.
`world_generate`, `world_perimeter`, `world_populate`, and `world_spawn` follow
the same private-asset rule. If a map and database carry different private
`Client` views, the tool refuses to guess which artwork to drop. Publish
content against the map's asset view, or pass an explicit combined private
`assets` path. Explicit paths are caller-managed: they must contain every
private library needed by both map and database. Each bundle returns its
`assets` path for client launch.

For different room sizes, pass `repeats: [[3,3],[4,3],[3,3]]` in place of
`nw`/`ne`; `rooms` is inferred or must match the list length. Each result's
`doorways` gives center and half-open six-by-four bounds. Every cell is
walkable, and source Wood wall art frames the crossing. `interactive: false`
means these are initially static passages. The editor restores varied repeats
when reopening a saved map. `wood-wall-junction` is
also available to `map_create` and `map_stamp` for a manually placed four-seam
passage; inspect neighboring wall joins in the editor.

Use `room_door_place` to make one of these checked Wood thresholds interactive.
Pass its `center` from `world_perimeter`'s `doorways`, plus the `map` and
`database` from the same private bundle, an existing `source_monster` ID in
that database, and a fresh `output`. For example:

```json
{"map":"project:perimeter-v1/Map/GeneratedCave.map","database":"project:perimeter-v1/System.db","center":[39,30],"source_monster":1,"output":"perimeter-with-door"}
```

Replace the example center and monster ID with values returned for your bundle.
The new bundle copies the map, navigation and database byte-for-byte, and adds
`Map/GeneratedCave.doors.json`. Up to eight nonoverlapping doors can be placed
by calling the tool again on the latest bundle. A door starts closed on each
server launch. Click it within two cells to open or close it; the closed door
blocks its eight-cell crossing, and closing fails while an actor occupies that
crossing. The server checks range and occupancy. The bundle returns a private
`assets` path with a new Wood plank door in the required `Mon-54.Zl` animation
slots; launch the client with `ZIRCON_ASSETS` set to that path and
`ZIRCON_MAP_DIR` set to the bundle's `Map` directory. All other sprite
slots and the original library stay unchanged. This Rust-only object has no
castle ownership, guild rules or repair cost. Client and server must both use
protocol 6 for the interaction.

Gate tools accept a nested `placement` with `x`, `y`, `repair_cost` and either
an existing `gate` ID or a genuine `castle` and source `monster` ID. An optional
`gate_image` (540–543) creates a new monster definition using the installed Sabuk
gate artwork; source stats are copied and original records remain unchanged.
Preview returns the closed collision footprint. Place requires `output`, writes
a new System.db revision, and rejects blocked, out-of-bounds or overlapping
footprints. Optional `database` continues a project revision. These are castle
gates governed by existing castle rules. `room_door_place` creates a separate
ordinary door with its own collision and interaction.
