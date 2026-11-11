# Sprite authoring

The first sprite-authoring workflow is implemented: export source references and
editable PNGs, import frames or sheets, review animation, validate the runtime
contract, build a library, and create private item/monster content. The checked-in
Jade Beetle demonstration supplies a new item icon and new eight-direction monster
art. It uses explicit key-pose holds; it is a working authoring example, not a
claim of polished bespoke motion for every future creature.

## Editor

Select an ItemInfo or MonsterInfo row and expand **Original sprite authoring**.
Choose a project folder outside installed assets and a new revision name. Export
creates `project.json`, editable `frames/*.png`, an immutable `source.zl` subset,
identity/animation metadata and a reference sheet. Point **Editable manifest** to
`project:revision/project.json`.

Import a PNG into a new revision, validate it, or build a pack. Exact-size import
is the default. **Fit artwork** explicitly preserves the target canvas and anchor;
**Convert alpha** explicitly thresholds alpha at128 for DXT1's binary transparency.
Sources with partial alpha are otherwise rejected by DXT1 validation.

For item icons, **Fit visible artwork** ignores transparent margins before fitting.
**Choose a new icon canvas** lets you replace a tiny inherited canvas;32×32 is a
useful inventory-cell size. These are explicit import choices, not renderer scale
changes. Monster previews keep their frame anchors; the UI limits margin trimming
to item imports. Clothing icons and worn clothing are separate: changing an item
Image updates icons, while its cloned Shape still selects the original wearable
body art. The equipped-art modes described below publish separate worn shapes.

**Load animation preview** offers action, direction, playback/manual frame,
original-reference toggle and a ground-anchor grid. Edited PNGs remain normal
files; reload preview after external edits. Advanced sheet mapping and explicit
new frame canvases are available through MCP. Publication uses the saved database,
so save database changes before creating new content.

## MCP

The core sprite tools are listed below; `tools/list` gives the complete current set.

| Tool | Output |
| --- | --- |
| `sprite_reference` | Editable reference project, animation inventory and contact sheet. |
| `sprite_export_project` | The same complete editable export for an item or monster Image. |
| `sprite_validate_project` | PNG, source identity, slot and runtime animation validation. |
| `sprite_import_image` | New project revision with one imported surface. |
| `sprite_import_sheet` | New revision with explicitly mapped sheet cells. |
| `sprite_preview_sheet` | New PNG contact sheet from edited frames. |
| `sprite_build_pack` | Validated range-local `Library.Zl`. |
| `sprite_publish` | New database record and private `Client` asset view. |
| `equipment_publish` | New equipped weapon, helmet or shield Shape with edited family frames in a private asset view. |

Project arguments point to `project:revision/project.json`; input PNGs must also
use project paths. Every output is a fresh revision name. Sheet mappings contain
`slot`, `layer`, `column` and `row`, using range-local frame slots. `cell_width` and
`cell_height` define the source grid; partial edge cells are padded transparently.
`fit`, `binary_alpha` and `trim_transparent` are explicit options.
Trimming requires fit and refuses a fully transparent input. When binary alpha
is requested, its threshold is applied before detecting visible bounds.
Single-image import also accepts `canvas:[32,32]` with required explicit
`offsets:[-16,-16]` for a new image canvas. To author new body frames, including
previously absent slots, specify `canvas:[64,64]`, `offsets:[-32,-50]` and optionally
`replace_layers:true` to clear the selected frames' old shadows/overlays. Unmapped
slots remain unchanged.

The runtime and editor share `mir-formats::monster_animation`. Required monster
slots include all eight directions of standing, walking, attack, struck, dying,
dead and harvested-corpse frames. Some original creatures lack the last eight
slots: export records this, but strict build/publication requires authored images.
Fully transparent required images are also rejected. Reference export is not a
claim that an original library meets the strict new-content contract.

PNG dimensions, offsets, paths, slot count, decoded-pixel budgets and codec alpha
are checked. Unchanged compressed surfaces remain byte-identical. Modified
surfaces are encoded, decoded and checked; DXT encoding is lossy. Paths cannot
escape their project. Source checksums detect accidental changes, not malicious
re-signing of an authoring project.

## Private publication

Items receive a new Image index across StoreItems, Inventory and Ground. Monsters
receive an existing runtime-mapped Image whose entire library/shape is unused by
all saved MonsterInfo records, including aliases. The installed database currently
allocates Image28. An exhausted mapping catalogue is an explicit error; no renderer
change or silent reskinning is performed.

Publication clones the selected record and stats with a new name/ID. Drops and
spawns are not copied; new items are not starter items. Other database records and
unrelated compressed library slots remain unchanged. Private Client views use
read-only source dependencies via symlinks on Unix and copies elsewhere. Keep the
source assets available when using a linked view.

For chained publications, pass **both** `database:"project:previous/System.db"`
and `assets:"project:previous/Client"`. If the next project comes from a library
changed by the previous publication, re-export from that private Client first.
Launch the game with the final private Client and matching database. A generated
map bundle uses its own database snapshot plus that same private Client.

## Reproduce the demonstration

Build the Rust workspace, then run from the repository root with a new project:

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools python3 rust/scripts/sprite_demo.py \
  --binary rust/target/debug/mir-editor \
  --assets "$HOME/zircon-assets/Client" \
  --db "$HOME/zircon-assets/Database/System.db" \
  --project /tmp/zircon-sprite-demo-new
```

This imports the checked-in generated PNGs, maps64 key poses to232 required
monster slots, publishes the Jade Beetle Ring and Jade Beetle, and generates a
connected Wood map with the new monster. `demo-report.json` identifies the private
assets, database, map and allocated IDs. Existing artwork, libraries and records
are unchanged. The authoring client does not invoke an image API or require a key;
new PNGs can come from an external art editor or the AI host's image tools.

`rust/scripts/sprite_smoke.py` separately exercises all eight sprite tools,
including invalid-source refusal and source-hash preservation. Native evidence for
the completed demonstration is under `/tmp/zircon-sprite-demo-verified`: `game.png`
shows the new equipped ring and a live Jade Beetle; `editor.png` shows the map.
The demo's generated art is intentionally a starting point for manual animation
review and refinement. Arbitrary animation rigs, equipment layering and additional
art-production phases are outside this completed workflow.

## Wearable clothing and production tools (2026-09-22)

Armour now has separate editable body projects for normal male/female and
Assassin male/female appearances. In an armour item's authoring panel, select
**Author worn clothing**, choose the body family, and export. Item `Shape`
selects the body; `Image` still selects its inventory icon. Body contracts use the
same animation mapping as the client, including riding, fishing fallback and
Assassin offsets. Animation preview follows the project's actual contract.
Wearable contact sheets show the first frame of each action/direction; playback
and validation cover the full animation.

MCP export uses the existing `sprite_export_project` tool with `kind` equal to
`armour-male`, `armour-female`, `armour-assassin-male`, or
`armour-assassin-female`; `source_image` means the source armour's **Shape** for
these kinds. PNG and sheet import retain image, shadow and overlay layers.
`wearable_publish` requires exactly the body projects allowed by the source
item's gender/class flags, allocates an unused armour Shape, copies item stats
and restrictions, and writes a private Client view plus System.db. Missing,
duplicate or mismatched families fail before publication. Existing icons and
weapon/hair/helmet/shield layers remain unchanged. Optional `database` and
`assets` project paths allow chaining previous private content revisions.

Example publication:

```json
{"projects":["project:robe-male/project.json"],"source_id":194,
 "name":"Blue Flame Robe","output":"blue-robe"}
```

The item shown here is male-only; an item permitting both genders needs both
projects. A normal-plus-Assassin item requires all allowed body families.

`sprite_tint` creates a new revision with the same RGB multipliers applied to
all frames of an `image` or `overlay` layer. `[255,255,255]` is neutral. Alpha,
frame sizes, anchors, shadows and other layers remain unchanged. This supports
consistent palette variants without changing the renderer.

`sprite_build_batch` validates and builds 1–32 projects into a new directory,
with a versioned `project-set.json` listing each identity and relative library.
Duplicate projects or a failed build reject the batch; no database publication
is implied. These tools support existing monster/item projects and wearable
families. Arbitrary new rigs and automatic motion synthesis are not implemented.

`rust/scripts/wearable_smoke.py` verifies the four body families, tinting, private
publication, mismatched-family refusal and batch builds against installed assets.
The demonstrated clothes are colour variants of source garments, not newly drawn
silhouettes. Native evidence is in `/tmp/zircon-wearable-smoke3`: four gender/class
captures and four mounted captures. Riding was enabled only in private QA map
snapshots. Source libraries and the installed database were not modified.

Castle gate monster images 540–543 use their own SabukGate animation contract:
25 unique required slots cover closed damage stages, open idle, closing/opening
and death. All four installed orientations have complete source art. Publication
allocates another unused gate appearance and preserves the source FaceImage and
art orientation. Ordinary monsters cannot allocate gate appearances, and gates
cannot allocate ordinary monster appearances. Exhausted gate slots fail explicitly.
`rust/scripts/gate_smoke.py` also verifies gate export and private publication.

## Drawing new silhouettes and frame motion (2026-09-23)

`sprite_paint` draws opaque or transparent polygons into one new project revision.
Each polygon names a range-local `slot`, ordered pixel `points`, an RGBA `color`,
and optional `under:true`. Underpainting fills only transparent pixels, so a
cape can sit behind the existing body. `padding:[left,top,right,bottom]` extends
the image canvas by up to 64 pixels per side and adjusts its offset to keep the
original body at the same ground anchor. Opaque foreground painting and
transparent erasing are also supported. Existing shadows, overlays, reference
art, and source libraries stay unchanged. The operation validates all inputs
before creating a revision, then runs normal project and build checks.

For example, on an exported body project with a present local frame slot 0:

```json
{"project":"project:body/project.json","output":"body-with-cloak",
 "padding":[12,0,12,4],
 "polygons":[{"slot":0,"points":[[12,16],[22,16],[42,58],[-7,58]],
              "color":[34,112,145,255],"under":true}]}
```

The earlier cloak demonstration exported normal and Assassin male/female body
art, painted a cape silhouette on all 3,152 required frames, varied its hem
across each action, and published four private wearable items. Native game
captures and a four-frame
motion strip from that first run are in `/tmp/zircon-cloak-demo-1`. The current
`sprite_outfit_demo.py` uses source-drawn poses and painted fabric instead.

## Draw and interpolate garment motion

The editor now has a zoomed pixel canvas for a selected frame. It can paint,
erase, or draw behind existing art, with optional canvas padding. The animation
preview can show the previous frame as a guide. Polygon mode lets an artist
keep poses on the first and last frames of one action and direction; the editor
interpolates the frames between them into a new private project revision.

MCP `sprite_paint_motion` exposes the same operation to an AI authoring client.
Supply an ordered contiguous `slots` run from one animation direction and
`keyframes` with matching polygon point counts. Keyframes must include the first
and last slot. Points and RGB colors interpolate linearly. Alpha stays binary;
`under` uses the nearer keyframe. The source project, untouched slots, other
image layers, anchors and installed library remain unchanged. For example:

```json
{"project":"project:body/project.json","output":"cape-motion",
 "padding":[8,0,8,0],"slots":[20,21,22],
 "keyframes":[
   {"slot":20,"points":[[-3,8],[-1,8],[-1,16],[-3,16]],"color":[94,43,140,255],"under":true},
   {"slot":22,"points":[[-1,8],[1,8],[1,16],[-1,16]],"color":[120,54,160,255],"under":true}
 ]}
```

Those slot numbers are illustrative; select real slots from the exported
project's `contract.json`. A real three-frame armour MCP run and native editor
capture are in `/tmp/zircon-motion-process-smoke2` and
`/tmp/zircon-sprite-draw-qa/editor-motion.png`. This tool interpolates poses
supplied by an artist or AI. The single-pose preset below designs cloth motion.

An earlier four-family demonstration added darker back cloth and animated
highlight folds to all 3,152 frames. Native standing and mounted captures are
in `/tmp/zircon-outfit-demo-4`. The clothes now read more clearly at world
scale, though the example still uses procedural polygons rather than painted
fabric texture.

### Design motion from one drawn pose

Draw one cloth polygon on a frame in the current animation preview, then use
**Design motion from drawn polygon**. Set **Cloth motion** to 1–12 pixels. The
editor reads the selected action and direction from `contract.json`, keeps the
polygon's upper edge attached, and moves lower vertices through walking/running
sway, attack lunge, struck recoil, death settle, or quiet idle motion. It writes
all frames in that action/direction run as a new private project revision.

MCP `sprite_auto_motion` provides the same operation. For example:

```json
{"project":"project:body/project.json","output":"cape-designed",
 "padding":[8,0,8,0],"slots":[20,21,22],"strength":3,
 "seed":{"slot":21,"points":[[-3,8],[-1,8],[-1,16],[-3,16]],
         "color":[94,43,140,255],"under":true}}
```

Use real contiguous slots from one action/direction in `contract.json`; the
seed slot must belong to that run. The result preserves original art and its
source file. This is a cloth-polygon motion preset, not a skeleton or automatic
design of arms, legs, or facial expression. Review each generated run in the
native client before publishing.

Real-asset MCP smoke output is in `/tmp/zircon-auto-sprite-smoke-2/body-designed`.
The native editor control is captured in
`/tmp/zircon-auto-pose-qa-3/editor-selected.png` using a small five-frame test
project.

## Painted fabric and complete reference poses (2026-09-23)

The current four-family example transfers hand-drawn armour poses and shadows
from original appearances 5 and 2 into new private projects, then paints four
fabric patterns from `crates/mir-editor/art/garment-textiles.png` onto their
existing garment pixels. No procedural silhouette is added. It changes 4,896
drawn frames, including every required animation frame, without changing the
installed database or sprite libraries. Run `rust/scripts/sprite_outfit_demo.py`
with Python Pillow installed. Equipped standing and mounted captures and the report are in
`/tmp/zircon-pose-garment-20260923-a`.

In the editor, select **Author worn clothing**, choose a body family and a
reference armour shape, then click **Create full-body poses from reference**.
MCP `sprite_pose_from_reference` provides the same operation:

```json
{"kind":"armour-male","source_image":4,"donor_image":5,"output":"new-body-poses"}
```

The returned editable project keeps source item identity and uses the donor's
complete action, direction, frame, offset and shadow pose set. Paint new art in
the PNGs and validate before publication. It transfers an existing animation;
inventing new limb or facial motion still needs drawn keyframes or another
reference.

The editor's **Design motion from drawn polygon** action and MCP
`sprite_auto_motion` generate cloth sway, lunge, recoil and settling from one
seed pose in a chosen action/direction. They preserve frame anchors and
validate the source animation contract. The result is a garment motion preset;
New limb, face and skeleton motion still needs authored work.

## Equipped art and AI-drawn poses (2026-09-23)

The editor's **Author equipped weapon/helmet/shield art** mode exports editable
PNG projects for the relevant gender and class families. `equipment_publish`
checks all required families, allocates a new unused ItemInfo.Shape, and writes
the modified equipment libraries and cloned item to a private Client/database
revision. Item icons remain separate. MCP uses `sprite_export_project` with
`weapon-male`, `weapon-female`, `helmet-male`, `helmet-female`,
`helmet-assassin-male`, `helmet-assassin-female`, `shield-male`,
`shield-female`, `shield-assassin-male`, or `shield-assassin-female`; pass the
source item's Shape as `source_image`. Shield allocation checks both normal and
Assassin frame ranges to avoid overwriting other items. Stock equipment omits
some animation slots; those remain optional, while all source-visible frames
must stay valid.

An AI can draw new body motion as PNG keyframes or full sprite sheets. Import
them with the existing image/sheet tools using exact slots from `contract.json`,
then validate and review every action and direction in native playback. The MCP
`design_sprite_motion` prompt describes this workflow to a host AI with image
generation tools. It does not call an image model inside the editor or guarantee
consistent eight-direction frames. Reference-pose transfer remains available
when original motion is acceptable; genuinely new limb motion needs newly drawn
frames and visual correction.

As a concrete import proof, [ai_walk_cycle_draft.png](../crates/mir-editor/art/ai_walk_cycle_draft.png) contains six newly
generated right-facing walking poses. `sprite_import_sheet` fitted them to six
Walking/Right body slots in a private project; validation and pack build passed
without changing the source library (`/tmp/zircon-ai-pose-proof-20260923`).
It remains a draft: the other directions and actions still use source motion,
so the pack was not published as a complete new animation.

The [Azure Wayfarer Coat](../crates/mir-editor/art/azure-wayfarer/README.md) is a complete
male Warrior wearable built from 1,058 source-drawn body poses, a consistent
blue/gold fabric treatment, six newly drawn Walking/Right images and a new
40×40 inventory icon. `rust/scripts/azure_wayfarer.py` rebuilds and validates
the body and icon, then publishes them as one private item revision. The final
item uses a new armour Shape and icon Image; source assets remain unchanged.
Native [standing](../crates/mir-editor/art/azure-wayfarer/ingame.png) and
[walking](../crates/mir-editor/art/azure-wayfarer/walk-ingame.gif) captures show the result.
Hair, weapon, helmet and shield continue to use the player's independent
equipment layers; only the Walking/Right motion is newly drawn.
