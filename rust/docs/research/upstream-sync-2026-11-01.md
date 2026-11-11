# Upstream sync check, 2026-11-01

Upstream `Suprcode/Zircon` master is 115 commits ahead of our fork point
(`092a9d7`, 2026-05-27). This note records which upstream issues and
commits apply to the Rust port, what was fixed here, and what cannot be
ported because the installed asset pack lacks the data.

## Fixed in the Rust port today

| Upstream | Rust change |
|---|---|
| #284 hidden chat area still swallowed clicks | `chat_panel.rs`: a panel with no line newer than `FADE_MS` and the bar closed is `hidden`; `contains_mouse` is false and hovering does not bring it back (Zircon `ChatTab.HideChat`). Test `a_hidden_panel_lets_clicks_through_to_the_map`. |
| #283 corpses drawn over living actors | `game/render.rs`: dead objects are drawn in their own pass before the row loop (middle/front tiles, living objects), as upstream `b4e3e9b`. |
| `e6d31f1` harden item transactions | Our handlers already check counts, `can_trade`, and use `checked_mul`/`checked_add`. One real hole found: `item_move_inner` indexed `grid_mut(to)[to_slot]` without a bounds check, so `ItemMove` to slot 200 on a 48-slot bag panicked the server. Guarded; test `item_move_to_an_out_of_range_slot_is_refused`. |

## Open upstream issues, triaged for the Rust port

| # | Issue | Applies? |
|---|---|---|
| 285 | combo box arrow re-opens list | No: our market combo box is a plain cycling button. |
| 282 | distorted sprites on Vulkan after hours | No: C# Silk Vulkan buffer bug; wgpu client unaffected. |
| 276 | wing frame odd on kill | Open upstream; wings use the same frame table here, revisit when fixed there. |
| 268 | costume invisible while fishing cast | We have no costume layer. |
| 263, 262, 259 | suggestions (war horses, gem ordering) | Feature requests, not bugs. |
| 182 | Shuriken uses magic cast and one projectile shape | Our throw already plays the melee swing, but the client drew the generic fireball (`MAGIC` 1640) for it. Fixed: `monster_projectile` now uses Zircon's `MagicEx` 1270 block (3 frames, 16 directions, no blend). The two extra shapes at 1440/1610 stay unmapped, as upstream. |
| 181, 72 | weapon glow / armour overlay colours | Library data problems, not code. |
| 180 | mask import/export in library editor | Editor feature; `mir-formats` has no mask writer. |
| 170, 169 | castle-owner shop discount / dropdown | Not implemented; NPC `check` 20-21 stubs exist. |
| 160, 141 | pet and siege suggestions | Feature requests. |
| 138 | Swift Blade colour with element stone | Element colouring of spell effects is not implemented at all. |
| 74 | player drawn twice to show behind roofs | We draw once; the translucent copy under roofs is missing. |

## Upstream features since the fork, by portability

Portable logic (no new data needed):
- Group Loot System (`6459fbe`, `4c25b0f`): loot modes, round-robin, voting. Our groups split drop chance per member instead.
- Scrollable item pickup / compact loot piles (`5dbf32a`, `2c55666`).
- Player `SizePercent` as a packet (`cbd601d`, #247) and milestone titles sync.
- Item socket system and gem combining (`fb09c81`, `a499bfa`, `146704d`).
- Auto-activate accounts, account-spam ignore (`1755966`, `5221517`).
- Item chat linking (`1135f4e`), currency goods buy/sell (`7c2496c`, `705fccc`).
- Big map NPC/monster list and NPC categories (`bcb174f`) — categories need the new `NPCInfo` columns.
- `CLEARBAG`, `LevelWeapon`, `LevelCrafting` GM commands; MAKE/MONSTER autocomplete.

Blocked by the installed `System.db` (no table or column):
- Crafting system (`f8dfba1`): needs `CraftingInfo`, absent.
- Auto pathing and waypoints (`78225d4`): needs `MapInfo` waypoint rows, absent.
- DungeonInfo / instances (`efa204c`): `InstanceInfo` has 0 rows.
- Numa monsters, Castle flag images 1000+, gem animation levels: new library content.

C#-only (rendering engine, tooling, localisation):
- Silk DX11/Vulkan pipelines, shadow pixel shaders, Window/UI scale options,
  DPI fixes, atlas/caching fixes, library editor fixes, Chinese messages,
  decoupled simulation timing, patcher reports, Server WinForms views.

## Ponytail audit of the Rust workspace (same day)

Three read-only audits (server net/persistence, server world handlers,
client/proto/formats). Expected load: a public server with a few hundred
players and untrusted clients. Fixed today, each with a regression test:

- `mail_send` accepted the same bag cell several times: five copies
  attached, one taken. Now refused (`refine::distinct_cells`).
- `npc_refine` counted a duplicated ore or jewel cell several times for the
  chance but consumed it once. Now refused.
- A trade offer stored only the slot; swapping another item into the slot
  after the partner confirmed delivered the swap. Offers now carry the item
  id and the trade cancels on mismatch.
- `item_move` to an out-of-range bag slot panicked the server.
- `listener.accept()` errors (EMFILE, ECONNABORTED) ended the listener and
  exited the process; now logged with a 100 ms back-off.
- A connection that said hello but never logged in held its slot forever;
  dropped after `LOGIN_TIMEOUT` (5 min).
- guilds/mail/market/social/NPC stores wrote non-atomically and a corrupt
  file silently became an empty store on the next save. One shared
  `load_json`/`save_json` (temp file + rename, corrupt file refuses startup).
- `item_drop` used `clamp(1, count)`, which panics on a zero-count cell.
- `MapFile::parse` allocated `w*h` cells before checking the data length;
  MirDB and `.Zl` readers sized vectors from counts in the file; `.Zl`
  surfaces are now bounds-checked against the file length and DXT sizes
  are computed in `usize`. Truncated-input tests added for map and db.
- Client `Storage { size }` is capped at 200 slots like the inventory.

Second pass, same day (the items first reported as "not fixed"):

- Login / new-account: at most three attempts per connection per three
  seconds (the login screen's failed login, create, login sequence fits).
- Items moved into the market or mail stores, and mail/market gold, mark
  `World::account_dirty`; main saves accounts that tick (one-tick window
  instead of 30 s). A disconnect sets `save_soon` instead of a full
  rewrite per connection, so a mass disconnect costs one save.
- Writer task: `write_all` under `IDLE_TIMEOUT`, so a client that stops
  reading cannot hold a dropped connection open.
- `guild_tax_gold` marks the guild store dirty; `World::tick` flushes once.
- `npc_repair` goes through `npc_session` and refuses storage cells
  outside a safe zone, like every other storage path.
- `guild_broadcast_info` builds one online set instead of a player scan
  per member.
- Client: `SpriteRenderer::reset` drops the sprite cache and atlas pages
  on map change (64 MB VRAM per page); a sprite wider than a page is
  "missing" instead of an `expect`.
- Tests: `valid()` size limits, `Gate` per-address cap, `mir_proto::rules`
  (attack delay floor, `/3` costs, power clamp, name and password rules).
- Deleted: `magic_type::needs_cell`, `SpriteRenderer::is_loaded`,
  `castles::has`, `market_account`, `online_of_account`, the chat panel's
  word wrap; one `WindowLayout::contains_mouse` for the three windows.

Also upstream #138 (element colour on physical skills): `CombatStats` now
carries `attack_element` (Zircon `GetAffinityElement`, affinity stats
43-49; the pack's Talisman Of Darkness is Dark), `ObjectAttack` and
`PlayerStats` carry it, and the white attack recipes (Slaying, Thrusting,
Half Moon, Destructive Surge, Dragon Rise, Blade Storm) draw in the
element colour. Swift Blade stays white, as it does upstream.

Still open: the `Inbound::Reload | Shutdown => {}` arm is required for
exhaustiveness; `wearable_animation::supported_actions` ignores its
argument but is test-only; the store tab re-sorts its stock per frame.
