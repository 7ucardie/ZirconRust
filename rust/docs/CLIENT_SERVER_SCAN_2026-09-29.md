# Client and server scan — 2026-09-29

Scope: Rust client, server, protocol, installed `System.db` and sprite pack, with the C# client/server as the rule and art reference. This is a static and automated runtime scan, not certification that every gameplay path has been played manually.

## Validation

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`, and `ZIRCON_ASSETS=~/zircon-assets/Client cargo test --workspace -- --test-threads=1` pass. The socket-backed heartbeat test needs localhost access outside the restricted sandbox.
- Tests cover client rendering and input, server item/spell/map rules, protocol and network load, account persistence, and editor/asset round trips. The live server was gracefully restarted after the build; it loaded one account, 244 maps, 309 monster definitions, and special drops enabled.
- The built macOS client opened on Metal, connected to the live server, rendered its login screen, and saved a screenshot in an unrestricted GUI smoke test. Gameplay still needs an interactive play pass.

## Confirmed defects fixed during this scan

- A closed client network channel returned `Disconnected` on every poll. The client drained all available events before rendering, so this could loop forever. The channel now reports closure once; a regression test failed before the change and passes after it.
- Town teleport scrolls changed location without `TELEPORT_OUT` or `TELEPORT_IN` events. Both effects now reach the client; tests cover same-map teleport and arrival after a map change.
- Royal and Blue Dragon horse armour used a missing or clamped mounted shape. Shape 7 now remains 7; both armour items equip and draw available related Gold/Blue horse art. Their exact C# Royal/Blue Dragon horse libraries are absent from this asset pack.
- Installed Janitor and Chaotic weapons now have equip, icon, world-frame, and effect checks. The database makes Janitors Dual Blade and Chaotic Heaven Blade 2 Assassin-only; Janitors Scimitar works for Wizard.
- `NPCValue` now resolves the installed Fame page's name and next-title cost. Colored C# dialog markup is shown as readable plain text in the Rust client.
- Dark Stone shapes 1–4 now show their animated source-library overlays in the character panel.
- The installed legacy event actions for player recall (7) and escape (8) now move players using the database map and region. Together with existing message and spawn actions, every executable action type in this pack is covered; type 0 is `None`.
- The dungeon dashboard shows a labeled blue horse-armour fallback for Blue Dragon Armour's absent source icon 1805. The login screen now displays ZirconRust over the original background.

## Data and parity findings still open

- Of 1,078 installed items, only Blue Dragon Armour (ID 1167, image 1805) lacks its database-selected item icon. The runtime and dashboard both show a related fallback; the dashboard identifies the missing source picture.
- The [equipment appearance crosswalk](equipment_appearance_audit.md) checks 1,226 class/gender paths against the C# library mapping: zero mapping mismatches; 8 paths require absent Royal/Blue Dragon horse libraries, 4 Assassin helmet paths lack their source frame, 24 items have no C# world mapping, and 80 boot paths have no separate world layer in either client.
- `CurrencyInfoImage` (8 rows) and `FlagInfo` (4 rows) have no Rust readers. No active C# use of those records was found; `NPCValue` (2 rows) is now read for the installed Fame page.
- The C# `DungeonFinderDialog`, `LootBoxDialog`, `BundleDialog`, and `CaptionDialog` have no equivalent Rust flow. This installed database has no instance, loot-box, bundle, or costume items and no caption-change item, so those flows cannot be exercised with this content pack. Separate costume equipment remains unavailable until such items are authored.
- The exact Royal and Blue Dragon mounted horse-art libraries, Blue Dragon icon 1805, and four Assassin helmet frames are absent from the installed source art. Related fallback art is used where possible; exact visual parity needs the missing assets.

Passing tests and a clean mapping audit do not prove frame-by-frame spell or manual interaction parity.
