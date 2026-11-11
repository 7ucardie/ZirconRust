# Azure Wayfarer Coat

Male Warrior wearable. The full 1,064-frame body animation has 1,058
source-drawn poses from armour Shape 5, with blue/gold fabric from
`crates/mir-editor/art/garment-textiles.png`. Six Walking/Right images use the
AI-drawn strip in this folder. Source assets are never overwritten.

Generate the editable project, validated sprite pack, preview and private
Client/database revision:

```sh
python3 rust/scripts/azure_wayfarer.py \
  --binary rust/target/debug/mir-editor \
  --assets ~/zircon-assets/Client \
  --db ~/zircon-assets/Database/System.db \
  --project /tmp/azure-wayfarer-build
```

Set `ZIRCON_ASSETS=/tmp/azure-wayfarer-build/published/Client` and
`ZIRCON_DB=/tmp/azure-wayfarer-build/published/System.db` for local play.
The cloned item ID and full paths appear in `report.json`. This item retains
separate hair, weapon, helmet and shield layers. A distinct 40×40 inventory icon
is published first, then carried into the final wearable record.

Native client captures: [standing in Bichon](ingame.png) and
[right-walking cycle](walk-ingame.gif). The preview retains the test character's
white hair and equipped sword as separate layers.

Both original images were made with the built-in image-generation tool. These
are the prompts used, with the prior walking strip and an existing body frame
as references for the first, and the new strip plus source icon for the second:

> Edit target image 1, a six-panel walking sprite strip, using image 2 as the
> definitive character and outfit identity reference. Produce a
> transparent-background horizontal strip of EXACTLY six equal square-ish
> cells, one right-facing side-view character per cell, no text. Preserve the
> six distinct walk poses and silhouette scale across the strip, but repaint
> the character as the hooded Azure Wayfarer: same dark teal long robe/coat
> with gold embroidery and hem, hooded face, gloves, boots, palette and
> pixel-art style seen in image 2. No bare brown-haired face; do not change
> character identity between cells. Six clearly different gait phases with
> alternating legs and arms, stable ground line, steady body height, crisp
> pixel-art edges, no motion blur, no background or shadows outside the
> sprite. This is game sprite source art; keep panels separate with generous
> transparent padding and consistent foot placement.

> Create ONE original inventory item icon for a fantasy RPG. Design: the
> Azure Wayfarer Coat from reference image 1, folded compactly as a wearable
> garment, with a visible deep teal hood, long teal coat panels, warm gold
> trim and small gold embroidery, dark gloves/leather belt accents. Reference
> image 2 is only an example of a compact inventory-icon composition; replace
> its gray garment with this new coat. Hand-painted pixel art matching
> classic Legend of Mir-style sprites, crisp readable silhouette, stronger
> color contrast, clear details at 40x40 pixels. Transparent square canvas,
> object centered and filling roughly 85% of canvas, no person, no text, no
> UI border, no background, no cast shadow outside the garment. Output a
> single icon, not a sheet.
