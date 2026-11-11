# Installed equipment appearance audit

[`equipment_appearance_audit.csv`](data/equipment_appearance_audit.csv) crosswalks every installed `ItemInfo` weapon, armour, helmet, shield, boot, and horse armour against the original C# `PlayerObject` library keys, Rust's runtime mapping, and the installed source sprite at the standing frame. Rows cover each gender and Assassin/non-Assassin rendering path. The CSV records the database's required class and gender flags; ineligible combinations are explicitly marked.

Regenerate after changing the installed database or sprite collection:

```sh
cd rust
cargo run -q -p mir-formats --example equipment_audit -- \
  /path/to/Database/System.db /path/to/Client > docs/data/equipment_appearance_audit.csv
```

The current source pack has **zero C#–Rust library mismatches** across 1,226 rows. Of eligible paths, 498 have their standing sprite, 80 boots have no separate world layer in either client, 24 have no C# world mapping, 8 Royal/Blue Dragon horse-armour paths lack their source libraries, and 4 Assassin helmet paths lack the mapped source frame. The remaining 612 rows are ineligible class/gender combinations. The unmapped items are four shape-0 helmets and four shape-1000 thuribles; the thuribles still have their separate C# exterior effect. The missing frames are Assassin appearances for Headcover Of The Dragon Lord (ID 905) and Helmet Of Dragon Abyss (ID 935). This report distinguishes absent authored art from a wrong Rust library mapping; it does not claim every animation frame is present.

Royal Horse Armour (1166, shape 6) and Blue Dragon Armour (1167, shape 7) are level-60 horse armour usable by all classes. The source database gives Royal an empty icon (image 0) and Blue Dragon an icon index (1805) absent from the installed `StoreItems` library. The source pack also lacks `Data/Horse_Royal.Zl` (79), `Data/Horse_RoyalEffect.Zl` (80), `Data/Horse_BlueDragon.Zl` (81), and `Data/Horse_BlueDragonEffect.Zl` (82), including case variants in configured asset roots. ZirconRust displays existing Gold/Blue horse-armour icons (1800/1804) and Gold/Blue mounted models (75/76) until those files and bespoke icons are supplied. It preserves the distinct item names, stats, and shape IDs; the substitutes are visual only.

Chaotic Heaven Blade (1162), Janitors Scimitar (1163), Janitors Dual Blade (1164), and Chaotic Heaven Blade 2 (1165) all have installed icon, equipped-image, standing-world, and effect frames. The first two permit Warrior/Wizard/Taoist; the latter two require Assassin in the installed database. Their Rust weapon libraries match C#. Chaotic Heaven Blade 2's shape 1263 uses C#'s Assassin right-hand sprite; its nominal left-hand ADL6 standing frame is absent in the source pack and is not used by C# for that shape. Janitors Dual Blade's C# effect 112 is explicitly mapped to the shared effect 111 artwork in the original source.

Odyn Son (1069), Mythical (1070), Elemental (1072), and Sin (1079) all have weapon shape 136. The original C# maps them to M-Weapon13/WM-Weapon13; both installed libraries contain standing frame 30000. The Rust mapping and Assassin dual-weapon layering now follow those C# keys.

The installed database gives **no armour item** an `ExteriorEffect`, so an exact C# implementation has no world armour glows. ZirconRust adds a small, explicit visual override in `mir_proto::equipment_visual::armour_effect` for named high-tier armours. Wyvern Armour Of Protection (350/351), including Lazero's equipped female item 351, uses the original blue-aura frames. Argent/Blessed, Sacred, Phoenix, Azure, Demonic/Abyss, and Embers/Comet sets use matching original aura or wing families. Database effects, if later authored, take precedence. The character panel retains C#'s item-image preset overlay and adds the authored aura; commoner outfits (127/128) remain plain.

Four source weapons have database exterior effects; Odyn's default records have none. The available Chaotic/Janitor weapon-effect overlays are blade-shaped and do not fit Odyn's short mace/staff art: Odyn M-Weapon13 standing frame 30000 is 20×60 at offset (24,−51), while the candidate effect frames are 32×128 at (20,−94) or 36×128 at (19,−106). Rather than draw a giant blade over the mace, ZirconRust renders a low-opacity coloured screen-blended outline from Odyn's own equipped sprite. Son, Mythical, Elemental, and Sin get distinct blue, gold, green, and red glows in the world and character preview. Any future database `ExteriorEffect` overrides the authored glow.
