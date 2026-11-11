# ZirconRust parity audit — 2026-09-25

Source: `~/zircon-assets/Database/System.db`, its client asset pack, and the C# `Client/` and `ServerLibrary/` reference. Reproduce collection and effect inventories with `mir-server --example db_inventory` and `--example effect_inventory`. Raw inventories: [database_collections.csv](database_collections.csv) and [equipment_effect_items.csv](equipment_effect_items.csv).

Update, 2026-09-29: [the client/server scan](CLIENT_SERVER_SCAN_2026-09-29.md) records the later `NPCValue`, Dark Stone overlay, legacy event recall/escape, and login-branding changes. The findings below describe the original 2026-09-25 snapshot.

## Equipment effects

The C# client selects world sprites in `Client/Models/Player/ExteriorEffectManager.cs` and character-panel sprites in `Client/Scenes/Views/Character/EquipEffectDecider.cs`. ZirconRust now reads `ItemInfo.ExteriorEffect`, sends the four equipped effect values to clients, and draws the C# world frame groups for auras, wings, weapons, shields and emblems. The character panel uses the C# UI frame table and original image-based armour presets. Existing generic white armour glow remains a fallback.

This `System.db` assigns exterior effects to **8 items only**: four thuribles (IDs 1158–1161, effects 50–53) and four weapon records (IDs 1162–1165, effects 110–112). **No item assigns a coloured aura, wings, or emblem effect.** Those effects can render when authored into an item with the new `item_exterior_effect` MCP tool, but they will not appear on existing armour drops from this database. `EquipEffect-Part.Zl`, `EquipEffect-Full*.Zl`, and `EquipEffect-UI.Zl` exist. `MonMagicEx26.Zl` does not, so C#'s emblem-ring effects 200–203 have no source art in this pack. C# also lacks a world draw case for `A_FlameAura2` (9) and character-panel frames for effects 14–18. The installed `EquipEffect-Part.Zl` has no sampled south-facing standing frame for 14–18 (indices 4602, 4490, 4142, 4338, 4758). These are source-data gaps, not Rust-only gaps.

## Database systems still absent

`System.db` has **51 collections, 45 populated**. Rust now reads `EventInfo`, `EventTarget`, `EventAction`, and `CompanionSkillInfo`. Three populated collections still lack Rust readers:

| Collection | Rows | C# use / missing behavior |
|---|---:|---|
| `CurrencyInfoImage` | 8 | Currency display images |
| `FlagInfo` | 4 | No matching active C# model found; may be legacy data. |
| `NPCValue` | 2 | Dynamic NPC page values in `ServerLibrary/Models/NPCObject.cs` |

This is a collection-level scan. Rust uses five legacy event records for rotating boss hunts, but does not run every C# event trigger/action. Companion skill rolls, hunger, and Elixir Of Regret rerolls work. The C# reroll safety gate is a session toggle; this database has no separate skill-lock consumable. A referenced collection can still have fields or behaviors Rust does not use.

## Client gaps visible in the C# reference

The C# `Client/Scenes/Views` directory contains 39 top-level dialog files. This scan compared their visible flows with Rust client modules; it does not certify full behavior for dialogs that have Rust counterparts.

- `NPCObject` animates quest markers from `QuestIcons.Zl`. That library is absent from this asset pack. Rust now sends per-viewer quest status and draws `?`/`!` as a text fallback; original animated icon art is still absent.
- C# has a costume equipment slot and body-hiding rules in `PlayerObject`; Rust's wardrobe applies owned armour looks, but separate costume items remain unsupported. Emblem slot/effect transport exists, but this database has no active emblem-effect item.
- C# animates Dark Stone overlays in `ItemEffectDecider`; Rust shows item images without that overlay.
- C# has `DungeonFinderDialog`, `LootBoxDialog`, `BundleDialog`, and `CaptionDialog`; no corresponding Rust client flow was found. Their database use and gameplay priority need a separate feature pass.
- C# supports additional particle and line-effect paths in `Client/Models/Particles/`, `MirLineEffect.cs`, and `MirProjectile.cs`. Rust renders many spell sprites, but this audit did not prove frame-by-frame parity for every spell.

Priority for functional parity: remaining event trigger chains, content dialogs, and visual variants. Asset-specific screenshots with coloured wings require an item-to-effect assignment or the original database that contains those items.
