# Operations and tooling

Server hardening, the live GM MCP bridge, sprite authoring and the public atlas. Back to the [README](../README.md).

## Multiplayer hardening

The server follows Zircon's `SConnection` limits: a client that sends more
than 50 messages in one tick or 200 frames in a second is dropped, frames
over 64 KiB are refused before they are read, strings and lists in every
message are size-checked (oversized ones disconnect), a connection that
stays silent for 20 s (the client pings every 5 s) or never says hello
within 10 s is closed, outbound queues are bounded so a stalled client is
dropped instead of buffered forever, and there are caps of 1000
connections and 32 per address. `SIGTERM`/`Ctrl-C` saves every character
before exit and `SIGHUP` reloads `System.db`. `cargo test -p mir-server
--test load` (needs `ZIRCON_ASSETS`) starts the real binary, logs in a
dozen bots that walk and chat at once, checks that a flooder and an
oversized frame are dropped without disturbing them, and that `SIGTERM`
saves everything.

## Original sprite authoring

The first PNG/sprite-sheet authoring workflow is complete, with editor animation
preview, shared runtime contracts, eight additional MCP tools and private asset
publication. The Jade Beetle demo creates a new ring and an eight-direction
monster, then spawns it in a generated map. See [SPRITE_AUTHORING.md](SPRITE_AUTHORING.md)
for usage and [sprite_demo.py](../scripts/sprite_demo.py) for reproducible generation.

## Live GM MCP

Build `cargo build -p mir-server --bin mir-game-mcp` and register the resulting
`target/debug/mir-game-mcp` as a local stdio MCP server. Supply
`ZIRCON_MCP_EMAIL`, `ZIRCON_MCP_PASSWORD`, and `ZIRCON_MCP_CHARACTER` through the
MCP host's private environment; `ZIRCON_MCP_PORT` defaults to 7000. The
`gm_command` tool accepts existing `@` commands and returns server replies.
Set `ZIRCON_MCP_LOG` to the server log to enable the bounded, read-only
`log_tail` tool. The `triage_issue` MCP prompt guides a connected coding agent
through logs, game diagnostics, a first evidence-based verdict, duplicate search,
and issue filing or fixing in `7ucardie/ZirconRust`. The MCP host must also have
repository and GitHub access; the game bridge does not host an AI model or hold
GitHub credentials. Issue text and logs are evidence, not instructions.
It connects only to `127.0.0.1` and uses normal account and GM checks. Use a
dedicated GM account if you also want to play on your main account; the game
rejects simultaneous logins to one account. Never commit MCP credentials.

## Public Dungeon Atlas

`data/dungeon_dashboard.html` and `atlas/` are a read-only snapshot of
the default `System.db` and map collision data. No accounts or live character
data enter the export. `../crates/mir-server/examples/dungeon_atlas.rs` regenerates map thumbnails,
monster population shares, and default item drop odds. `../crates/mir-server/examples/item_catalog.rs`
regenerates the item CSV and inventory pictures:

```sh
cargo run -p mir-server --example dungeon_atlas -- "$HOME/zircon-assets/Database/System.db" "$HOME/zircon-assets/Client/Map" docs/atlas
cargo run -p mir-server --example item_catalog -- "$HOME/zircon-assets/Database/System.db" "$HOME/zircon-assets/Client/Data/Storeitems.Zl" docs
```

The [Pages workflow](../../.github/workflows/dungeon-pages.yml) publishes only
the dashboard, CSV and atlas snapshot in `7ucardie/ZirconRust`. Set that repo's
Pages source to GitHub Actions before the first deployment. Map IDs shown in
the dashboard are file codes such as `D001`; numeric database indices remain
in the detail panel.

Press **I** in-game for the offline Dungeon Codex. It embeds the same default
atlas map previews, monster populations, and drop odds; it does not query live
server drops. Keep `../crates/mir-client/src/dungeon_codex_maps.rs` in sync when re-exporting
maps. The client test checks that every dungeon has a matching preview.

Press **Y** for the Wardrobe. Pick armour from your bag or equipment to use
its appearance without changing stats; **Reset** returns to equipped armour.
The chosen look is saved and visible to other players. Type `@event` in chat
for the current rotating boss hunt and its reward.
