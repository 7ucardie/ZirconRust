<p align="center"><img src="rust/icons/zircon.png" width="128" alt="ZirconRust logo"></p>

<h1 align="center">ZirconRust</h1>

<p align="center">
  <a href="https://github.com/7ucardie/ZirconRust/actions/workflows/rust.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/7ucardie/ZirconRust/rust.yml?branch=main&label=CI&style=for-the-badge"></a>
  <a href="https://7ucardie.github.io/ZirconRust/"><img alt="World Atlas" src="https://img.shields.io/badge/atlas-live-009688?style=for-the-badge"></a>
  <img alt="Rust" src="https://img.shields.io/badge/Rust-edition%202024-DEA584.svg?logo=rust&logoColor=white&style=for-the-badge">
  <img alt="wgpu" src="https://img.shields.io/badge/wgpu-winit-3B82F6.svg?style=for-the-badge">
  <img alt="Platforms" src="https://img.shields.io/badge/Windows%20%7C%20macOS%20%7C%20Linux-555?style=for-the-badge">
</p>

A client, server and content editor for Legend of Mir 3, written in Rust. It reads the original [Zircon](https://github.com/Suprcode/Zircon) assets (`.Zl` sprite libraries, `.map` maps, `System.db`) and re-implements its rules.

## What it does

- **Plays the game.** A native client on winit + wgpu: login, character creation, maps, combat, skills for all four classes, quests, NPC shops, mail, market, groups, guilds, castles, mounts and companions.
- **Runs the world.** A tokio server with a single-threaded tick loop, Argon2 accounts, per-connection rate limits and atomic saves.
- **Edits content.** An egui editor for every `System.db` table, maps, spawns and sprites, with byte-exact round-trip saving.
- **Publishes a World Atlas.** A [read-only web page](https://7ucardie.github.io/ZirconRust/) of dungeons, monsters, drops and items.

## Quick start

The game assets are not in this repository. Put them in `~/zircon-assets` (see [assets](rust/README.md#assets)), then:

```sh
cd rust
cargo run -p mir-server   # listens on 0.0.0.0:7000
cargo run -p mir-client   # connects to 127.0.0.1:7000
cargo run -p mir-editor   # data editor
```

## Layout

| Path | Contents |
|---|---|
| [`rust/crates/`](rust/crates) | `mir-formats`, `mir-proto`, `mir-server`, `mir-client`, `mir-editor` |
| [`rust/docs/`](rust/docs) | Guides, plans, research notes and data exports |
| [`rust/scripts/`](rust/scripts) | Build and smoke-test helpers |

## Documentation

Start at the [Rust guide](rust/README.md). Then [Gameplay](rust/docs/GAMEPLAY.md), [Content editor](rust/docs/EDITOR.md), [Operations](rust/docs/OPERATIONS.md) and [Plans](rust/docs/PLANS.md).

## Contributing

Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` from `rust/` before pushing. Accounts, assets and build output are never committed. See [AGENTS.md](AGENTS.md).
