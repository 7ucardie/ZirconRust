# ZirconRust

A purpose-built Rust client and server for Legend of Mir 3, reading the
original Zircon assets (`.Zl` sprite libraries, `.map` maps, `System.db`).
The C# projects in the parent directory are reference only.

## Crates

| Crate | What it is |
|---|---|
| `mir-formats` | Readers for `.Zl` (DXT1/DXT5 sprites), `.map`, and MirDB `System.db` |
| `mir-proto` | Client/server messages, postcard-encoded with a length prefix |
| `mir-server` | Game server: accounts, characters, maps, spawns, movement, melee combat, monster AI |
| `mir-client` | Native client on winit + wgpu: login, character select/create, game scene |

## Assets

Not in the repository. Extract the Zircon client package and database so that:

```
~/zircon-assets/Client/Data/*.Zl
~/zircon-assets/Client/Data/Map Data/**/*.Zl
~/zircon-assets/Client/Map/*.map
~/zircon-assets/Database/System.db
```

Override the location with `ZIRCON_ASSETS=/path/to/Client` (and `ZIRCON_DB`).

Server startup rates: `ZIRCON_DROP_SPECIALS=1` enables a 1-in-15 chance
(1-in-30 for non-common equipment) to roll added stats on dropped weapons,
armour, shields, helmets, jewellery, or shoes, including signed elemental
resistances. Default is `0` (off).
`ZIRCON_EXPERIENCE_MULTIPLIER=2` doubles all earned experience; default is `1`.
Set both before starting the server. Invalid values stop startup.
The Inventory and Storage windows can each unlock one extra tab with Game Gold.
The default costs are 100 and 200 Game Gold respectively; set
`ZIRCON_INVENTORY_TAB_PRICE` and `ZIRCON_STORAGE_TAB_PRICE` to positive integers
before server startup to change them. Inventory tabs belong to the character;
storage tabs belong to the account.

## Run

```
cd rust
cargo run -p mir-server              # listens on 0.0.0.0:7000
cargo run -p mir-client              # connects to 127.0.0.1:7000
```

On macOS, `./scripts/build_mac_app.sh` builds `target/debug/ZirconRust.app` with a
Finder/Dock icon; open that app to play without a terminal. Pass `--release`
for `target/release/ZirconRust.app`. Resize the window freely, or start with
`cargo run -p mir-client -- --size 1920x1080` (also accepts ultrawide sizes).
Set `ZIRCON_WINDOW_SIZE=1920x1080` to size the app when launched from Finder.

The client opens on the login screen: create an account, log in, create a
character (class, gender, hair) and start the game. Accounts and characters
are stored by the server in `data/accounts.json` (Argon2 password hashes),
override the directory with `--data DIR` or `ZIRCON_DATA`.

## Docker

```
docker compose -f rust/docker-compose.yml up --build
```

Only the server runs in a container; the client is a GPU application and runs
natively. Assets are mounted read-only from `ZIRCON_ASSETS_DIR`
(default `~/zircon-assets`).

## Tests

`cargo test --workspace`. Tests that need the real assets skip themselves
unless `ZIRCON_ASSETS` is set.

## Where the rules come from

Timings, formulas and formats were transcribed from the C# code:
`Client/Models/MirLibrary.cs` (`.Zl`), `Client/Scenes/Views/MapControl.cs`
(`.map` and draw order), `LibraryCore/FrameSet.cs` (animations),
`LibraryCore/MirDB/*` (`System.db`), `ServerLibrary/Models/PlayerObject*.cs`
and `MonsterObject.cs` (movement, combat, AI). `tools/gen_monster_table.py`
regenerates the monster sprite table from `Client/Models/MonsterObject.cs`.

## Documentation

| Guide | Covers |
|---|---|
| [Gameplay](docs/GAMEPLAY.md) | Controls, HUD, chat, groups, guilds, castles, skills, quests, monster AI |
| [Content editor](docs/EDITOR.md) | `mir-editor`: tables, maps, sprites, round-trip saving |
| [Operations](docs/OPERATIONS.md) | Multiplayer limits, live GM MCP, sprite authoring, Dungeon Atlas |
| [Plans](docs/PLANS.md) · [Recovery log](docs/RECOVERY.md) | Design plans and the build history |
| [Research notes](docs/research/) | How each C# system maps to Rust |
| [Data tables](docs/data/) | Item, monster and map CSV exports |
