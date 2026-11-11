# Source wall assembly investigation — 2026-09-15

Current status (2026-09-16): a source-specific Wood wall ring generator now
assembles verified D1008 bodies and corners; see the final follow-up below.
Automatic physical entry integration remains open. Rectangular room/corridor
and organic generators still need their own complete perimeter recipes.

The initial investigation below produced no production map/render changes.
Whole-chamber generation already retained authored walls; it is distinct from
the new repeated-piece generator.

## Evidence from the source pack

D102 contains 5,925 anchored middle/front surfaces; 1,522 extend outside their
containing 2x2 cell block. D1006 contains 551 surfaces; 117 cross that boundary.
KR21 (`Wood/Dungeonsc`) walls include 48-pixel-wide columns 192–448 pixels high.
Adjacent columns are parts of larger drawings. Geometrically clear vertical
cuts do not establish that columns can be rearranged safely. Neither source
has identical complete repeated two-column or two-row signatures including
back tiles.

D1006 does have repeated diagonal wall-body pairs (raw map image indices):

| Side | KR21 images | Translation in cells |
|---|---|---|
| Southwest | 612 / 611 | +2, +2 |
| Southeast | 628 / 627 | +2, −2 |
| Northwest | 538 / 533 | +2, −2 |
| Northeast | 445 / 440 | +2, +2 |

These are complete tall columns, not independent 2x2 tile squares. Their floor
and outside collars vary. Straight diagonal insertion near x+y=52–56 crosses
an interior wall spur (including image 383), changes collision, or encounters
a different wall tail. Repeated body IDs alone are insufficient evidence for
a safe complete seam.

## Experiment and result

A temporary diagonal seam search compared complete middle/front references
and collision across candidate duplicated macroblocks, retained full cell
records, and required connectivity and added usable floor. Padding, relaxed
seam bends, and ignoring ordinary floor-image variations still found no useful
matching seam in D1006. The experiment was excluded from production; no failed
tests or generator controls remain in the workspace. Art matching was not
weakened further just to produce an output.

Local diagnostics: /tmp/zircon-wall-analysis.py, /tmp/zircon-wall-analysis.txt,
/tmp/zircon-wall-motifs.py, /tmp/zircon-chamber-stretch-experiment.rs and
/tmp/zircon-stretch-test.log.

## Next implementation work

Build an explicit diagonal wall kit with complete body pairs, corner caps and
collision/ground collars. Validate pixel joins and authored doorway ports.
Route chamber expansion seams through undecorated floor while moving each
corner, spur and prop as one group. A source-specific diamond-room generator
can then assemble verified pieces; arbitrary rectangular BSP walls need their
own compatible kit. Do not label portal-linked whole chambers as a solution
to wall or doorway synthesis.

## Follow-up: reusable authored pieces

The editor now supports named piece files and repeated stamping as the first
explicit-kit authoring step. A piece retains complete cell data and references
existing sprites. Placement checks full frame bounds, even-grid steps, conflicting
overlaps and generated-layout constraints before committing one undo operation.
Wall, Corner and Doorway are authoring labels; they do not infer compatible joins
or create doorway openings. Selections still need manual review for neighboring
sprites anchored outside the selected cells.

A concrete northwest strip in D1006 is `(10,8)` through `(11,17)` inclusive
(2x10 cells), adjoining `(12,6)` through `(13,15)`. It contains 538/533 and their
full 537–534 / 532–529 floor tails, ground backing and collision collar. Source
bounds inspection found no outside middle/front sprite intersecting this strip
and no included sprite escaping it. Two copies at step `(2,-2)` preserve the
source collision mask, and the rebuilt join was inspected in the native client.
Ground/decorative tile variants differ between the original repeats; this is not
a byte-identical source seam or a general-purpose wall generator.

The follow-up seam experiment found an additional flaw in the old diagnostic:
authored empty layers use file 255 while padding used zero. Treating both as
empty fixes that comparison. A cell-diagonal experiment then added connected
floor, but the native preview showed broken floor edges. It remains excluded
from production. Current diagnostic: /tmp/zircon-wall-next-experiment.rs and
/tmp/zircon-wall-next-experiment.log. Automatic chamber expansion, a complete
corner/doorway kit and arbitrary room perimeter synthesis remain unfinished.

## Follow-up: explicit edge connections

Piece version 2 adds cardinal edge, offset, width and profile metadata. Each
opening must have a walkable edge and inward cell for its full width, and every
declared opening must connect within the piece. Joining requires opposite sides,
equal widths, exact profile equality and an even translation. It combines raw
cell records into a new piece without rotating art, consumes the joined ports
and remaps unused exterior ports. Joins exceeding 64 cells or enclosing an unused
port inside the bounding rectangle are rejected. Profiles are author declarations;
visual compatibility is still reviewed manually. These are physical floor
connections, separate from the game's teleport portals.

The real-source join fixture pairs D1006 A=(10,8) through (11,17) with
B=(12,6) through (13,17). A's east opening has offset 8, width 2; B's west opening
has offset 10, width 2. Profile `wood-nw-ground` matches, giving translation
(+2,-2). The combined 4x12 piece retains exterior west/east openings at offset
10. Placed at (10,6), it preserves the original source collision mask. Neither
this fixture nor metadata support completes a verified corner/doorway art kit.

Offset joins also carry a coverage mask in the version 2 file. Padding that
belongs to neither source piece is omitted from placement, rather than stamping
blank tiles into the destination. Coverage retains complete 2x2 blocks; malformed
masks are rejected and uncovered cells cannot supply fake walkable ports. This
fixed a black background patch found in the first native composition preview.
With the fix, the wall-pair fixture reassembles the original map byte-for-byte.

## Follow-up: recessed connections (version 3)

Connection insets now preserve openings on concave coverage boundaries. Offset
still runs along the chosen cardinal side; inset measures inward from the outer
bounding edge. An opening needs covered walkable edge/inward cells and uncovered
space immediately outside. Unused openings remain available after composition,
even when another piece extends the bounding rectangle around them. A further
piece can fill the recess. Empty padding may overlap, but covered cells may not;
joins sealing an unused opening are rejected. Version 1 and 2 loading is retained,
with absent insets defaulting to zero.

The three-piece native fixture uses repeated, undecorated D1006 floor cells
(20,24) through (23,27), forming an 8x8 + offset 8x8 assembly and an 8x4 filler.
The first join retains a north connection with inset 4; the second fills that
recess. The 16x12 result contains 160 connected walkable cells and retains its
remaining openings. This verifies composition and collision using existing art;
it is not a visually verified corner/doorway wall kit or automatic perimeter
synthesis. The earlier outer-boundary-only limitation above is now resolved.

## Follow-up: checked corner and doorway starter kits

The Wood starter kit now captures a real convex north wall bend from D1006.
Rectangles below are inclusive; connection offsets are local to each piece.

| Piece | Source rectangle | Connections, width 2 |
|---|---|---|
| NW approach | (14,2) through (15,13) | East offset 10, `wood-nw` |
| North corner | (16,2) through (19,13) | West offset 10, `wood-nw`; East offset 10, `wood-ne` |
| NE approach | (20,4) through (21,13) | West offset 8, `wood-ne` |

The corner contains KR21 images 538/533 turning into 421/416 with complete
floor tails. All three captures contain every intersecting sprite in full,
including back tiles; none cuts artwork anchored outside the selection. No
animated surfaces intersect these selections. The first join translates the
corner by (+2,0); the second places the NE approach (+4,+2) from the corner.
The result is an 8x12 envelope containing 27 connected walkable cells. Placement
at source position (14,2) reproduces the original whole map byte-for-byte.
The corner's extra floor rows are necessary: adjacent approaches do not have
a full two-cell opening at global rows 10–11, but do at rows 12–13.

A separate palace doorway uses `0_000`, rectangle (24,4) through (35,51), with
complete wall ends and the carpeted physical opening. Its west/east ports have
offsets 40/34 and profile `palace-doorway`. The 12x48 selection preserves tall
neighboring palace artwork. It is not visually compatible with the Wood kit;
the different profiles prevent automatic cross-theme suggestions.

Built-in capture verifies known source fingerprints and complete source sprite
bounds. The kit browser offers bounded, explicit candidate searches using the
same composition validator as manual joins, and reusable directory catalogues.
These are checked starter pieces, not arbitrary corner synthesis, automatic
visual compatibility or a complete perimeter generator. A complete compatible
set of orientations, wall bodies and doorways remains to be authored.

## Follow-up: expanded north and complete south selections

The north kit now extends the historical three-piece starter above. Rectangles
in this table are half-open; all connections have width 2 and inset 0.

| Piece | Source rectangle | Connections |
|---|---|---|
| NW approach | (14,2)–(16,14) | East 10, `wood-nw` |
| North corner | (16,2)–(20,14) | West 10, `wood-nw`; East 10, `wood-ne` |
| NE approach | (20,4)–(22,16) | West 8, `wood-ne`; East 10, `wood-ne-bend` |
| NE bend | (22,4)–(26,16) | West 10, `wood-ne-bend`; East 8, `wood-ne-body` |
| NE terminal body | (26,4)–(28,14) | West 8, `wood-ne-body` |

The additional floor rows under the NE approach reach the bend's opening at
global rows 14–15. The five-piece result has a 14x14 envelope at (14,2), no
remaining ports, and restores the entire source map byte-for-byte. Separate
bend/body profiles reject repeating the asymmetric bend as a generic wall.
Native inspection shows coherent interior masonry and floor joins. The outer
wall segments end at the selection boundaries; the terminal body is the final
selected segment, not a physical wall endcap.

The NE terminal body contains KR21 445/440. A diagonal repeat would require
another opening at global rows 14–15 and therefore a bottom boundary of 16.
That selection cuts KR21 889 anchored at (27,18), whose artwork begins at row
15. The next strip, (28,6)–(30,16), similarly cuts image 883 anchored at
(28,17). Extending further requires preserving larger shrine groups instead
of copying partial wall collars.

NW repetition is also blocked by outside decoration (raw middle-file ID 26).
The historical strip (10,8)–(12,18) would need two more floor rows for West 10 /
East 8 ports and the diagonal (+2,−2) translation. Extending through row 19
cuts image 1375 anchored at (11,25), beginning at row 18. The next strip
(12,6)–(14,16) similarly encounters image 1345 at (13,25), beginning at row
17. Adding a west opening at global rows 14–15 to the current NW approach
cuts image 1315 at (15,25), beginning at row 15. The available same-offset
ports would instead produce a horizontal translation. No clean repeatable
NW body was added by discarding these outside-anchor checks.

A second Wood kit captures the south corner. These rectangles are also
half-open; all connections have width 2 and inset 0.

| Piece | Source rectangle | Connections |
|---|---|---|
| SW approach | (20,34)–(22,46) | East 2, `wood-sw` |
| Decorated south corner | (22,28)–(28,50), plus (26,26)–(28,28) | West 10, `wood-sw`; East 10, `wood-se` |
| SE decorated section | (28,26)–(34,46) | West 10, `wood-se` |

This retains tall decoration together with the wall corner. The SE selection
starts at row 26 because an animated frame reaches row 27; the static-only
top boundary of 28 would crop it. The corner's 2x2 cap yields a 6x24 envelope
at (22,26), with uncovered padding elsewhere in its upper two rows. The SE
section is 6x20. Assembly has a 14x24 envelope at (20,26), 280 covered cells
and 182 connected walkable cells, and restores the entire source map byte-for-byte.

An earlier narrower capture passed individual sprite-bound checks but native
inspection showed that it cut a circular floor motif composed of many sprites.
The final bounds preserve all 72 motif cells and nine animated anchor columns;
regression checks cover that complete group. The corner and decorated section
must therefore be understood as parts of the authored assembly, not independent
repeatable wall bodies. Complete individual sprites alone do not establish a
complete decorative group or a visually finished seam.
The assembled north and south sections are separate authored components,
not a complete room perimeter or a new Wood doorway.

The kit browser exposes both orientations and permits adding an independent
copy of the current piece with its edited name, type and ports. Removal and
addition invalidate cached candidates, and catalogues remain bounded to 16
pieces. `wood-corner` remains the north automation name; `wood-south-corner`
selects the south kit. Arbitrary repetition, automatic visual compatibility
and automatic perimeter synthesis remain unfinished.

## Follow-up: bounded whole-kit assembly

The browser now assembles a catalogue from its first entry, using each of at
most 16 entries once. Every step must have exactly one legal next join across
all unused entries. Full composition validation still checks coverage, source
art bounds, collision openings and unused ports. Ambiguous choices, disconnected
entries and exhaustion of the shared 256-trial discovery budget return an error
without adopting a partial assembly. Chosen joins are revalidated separately,
adding at most 15 attempts (271 total). A successful result becomes the current piece; the map
is unchanged until placement. Open ports may remain.

Reordering entries with **↑** / **↓** changes the starting piece, clears cached
candidate results and persists through folder export. `ZIRCON_ASSEMBLE_KIT=1`
automates assembly after built-in or folder catalogue loading. Real-asset tests
assemble both the north and south Wood kits and reproduce the source map
byte-for-byte at their original positions. Fixtures are under
`/tmp/zircon-auto-assembly-final`. This is authored-group assembly, not automatic
perimeter generation or a guarantee of visual compatibility for arbitrary kits.

## Follow-up: bounded Wood doorway search

Native inspection of the modest Wood-theme sources did not establish a new
compatible doorway. D1006 (50x50) contains a freestanding interior wall spur
and a perimeter stair entrance. D10032 (100x100) has continuous room/corridor
junctions around authored wall ends; no compact, matched doorway group was
verified. Such a junction would need its complete surrounding group and its own
profiles before becoming a reusable entry piece. D1012 (100x100) is a single
decorated chamber with a central platform and no physical doorway through a wall.

Read-only map inspection found that the remaining KR21 Wood sources considered
are 200–400 cells per axis. No doorway piece or compatibility claim was added
from this bounded search. Native evidence includes
`/tmp/zircon-wall-source-editor.png`, `/tmp/zircon-door-D10032.png` and the new
`/tmp/zircon-door-D1012.png`. A compatible Wood doorway remains open.

## Follow-up: complete orientations and generated Wood wall rings — 2026-09-16

D1008 provides clear repeatable wall collars absent from the smaller decorated
D1006 source. Bounds below are half-open. Each body is two cells wide and has
west/east width-two ports with the listed offsets.

| Body | Source bounds | West / east offsets | Profile |
|---|---|---|---|
| NW | (10,278)–(12,290) | 10 / 8 | `wood-wall-nw` |
| NE | (40,248)–(42,260) | 8 / 10 | `wood-wall-ne` |
| SW | (14,180)–(16,190) | 0 / 2 | `wood-wall-sw` |
| SE | (26,276)–(28,286) | 2 / 0 | `wood-wall-se` |

Four D1008 corner groups join these orientations: north
(168,276)–(178,292), east (42,90)–(50,110), south (70,316)–(76,328),
and west (4,276)–(14,300). Native inspections verify adjoining body/corner
masonry and floor; exact fingerprints and ports live in `wood_perimeter_kit.rs`.
Artifacts are under `/tmp/zircon-perimeter-kit-verified` and
`/tmp/zircon-wall-bodies-verified`.

The new fixed-placement composer accepts explicit seams and consumes every
paired endpoint simultaneously. This permits a four-piece closing cycle that
pairwise joins correctly reject while another unused port becomes covered.
It validates asset-frame bounds, even placement, unique ownership, exact seam
alignment, unique endpoints, the connected seam graph, remaining exposed ports
and connected floor before one stamp transaction. Limits are 64 pieces,
256x256 canvas cells and 512 seams. Coverage padding may overlap; owned cells
may not. Composition alone does not establish enclosure or fill a room.

The **Wood wall perimeter** recipe chooses NW/NE body counts in 3–10 and derives
SE=NW+2, SW=NE. These asymmetric counts close the measured corner offsets
exactly. The generator fills unowned interior floor blocks and retains blocked
exterior cells. All 64 supported count combinations pass source-backed checks.
Saved `wall_repeats` metadata restores the recipe parameters. Enclosed-layout
validation now rejects all walkable map-border cells, including connected
breaches from later edits. This is a newly assembled wall ring, not a copied
whole source chamber. See [PLANS.md](PLANS.md).

The larger D10032 entry investigation also succeeded after replacing a
rectangle with complete 2x2 ownership blocks. The seed rectangle
(74,34)–(90,54) otherwise cascades down neighboring tall sprites beyond the
piece height limit. Its shaped closure has envelope (74,24)–(90,62), 380 owned
cells and 111 walkable cells in one component. West offset10 uses
`wood-entry-room`; east offset29 uses `wood-entry-corridor`; both have width2.
Twelve wall fingerprints, all-frame/outside-anchor checks, source restoration,
roundtrip and cropped-tip rejection tests protect the group. Native evidence
is `/tmp/zircon-wood-entry-verified/editor.png`, compared with
`/tmp/zircon-entry-source.png`. It preserves the room exit and adjoining
corridor turns together; it does not claim a compact door or a compatible
D1008 ring seam.

At this single-room checkpoint, physical corridor integration was still under
research. The connected-room follow-up below supplies a physical passage using
concave corners. Arbitrary BSP/organic perimeter synthesis still needs separate
art recipes.

## Follow-up: body-compatible physical junction and concave turns

The larger D1008 search found actual passages between matching wall runs, but
rectangular captures also cut adjacent corridor walls. For example, the NE run
between (138,233) and (154,249) crosses two authored turns and exposes additional
NW/SE wall ends. Expanding that entire rectangle to retain every intersecting
sprite grows past the 64-cell piece limit. It is a multiwall junction, not a
compact removable wall pair.

A translated version at (92,182)–(112,214), half-open, is now captured as the
**Wood wall junction** in `wood_wall_entry.rs`. Its 20×32 envelope uses complete
2×2 ownership columns to retain both wall turns, their tails, small complete
floor decorations and the physical floor passage. It has four width-two ports:

| Side | Inset | Offset | Profile |
|---|---:|---:|---|
| West | 0 | 10 | `wood-wall-ne` |
| East | 4 | 8 | `wood-wall-nw` |
| East | 0 | 10 | `wood-wall-se` |
| East | 0 | 30 | `wood-wall-ne` |

All 25 major wall-column fingerprints and covered-source frame bounds are
checked. Source placement restores the original bytes. Simultaneous attachment
of all four generic bodies preserves the source collision on every covered
cell. Native source and isolated views passed at
`/tmp/zircon-wall-junction-verified/{source,isolated}-{editor,game}.png`.
The isolated fixture intentionally leaves surrounding room floor unowned.
Use `wood-wall-junction` for browser automation. The group is available for
manual authoring; automatic insertion into generated rooms is not delivered yet.

`wood_concave.rs` supplies two additional complete turns from D1008:

| Turn | Half-open bounds | West / east profiles | Both offsets |
|---|---|---|---:|
| Valley | (128,84)–(134,94) | `wood-wall-ne` / `wood-wall-nw` | 8 |
| Notch | (36,378)–(42,388) | `wood-wall-se` / `wood-wall-sw` | 2 |

Each is 6×10 cells with width-two ports. Source capture and both attached-body
collision checks pass. Fixtures and verified native valley/notch previews are under
`/tmp/zircon-concave-verified`. The valley has 12 connected walkable cells and
the notch 24.

## Follow-up: generated rooms with a physical connection

`wood_connected.rs` now assembles six convex and two concave corners into a
closed two-room outline. The complete valley and notch stand 14 rows apart,
leaving a verified 6×4 walkable passage between their collars. Each NW/NE repeat
count accepts 3–6. All 16 combinations pass enclosure, connected-floor and
passage checks; blocking the passage column separates two room components of
more than 64 cells each. This passage is actual map geometry and does not rely
on teleport triggers.

The UI exposes **Wood connected rooms** and automation uses
`ZIRCON_CAVE_MODE=wood-connected`. Saved `wall_rooms: 2` and `wall_repeats`
restore the recipe. MCP `world_perimeter` accepts `rooms: 2`, defaulting to one
room. The checked standalone four-seam junction remains available for authoring;
it is not the group used by the automatic connected-room recipe.

Native editor/client views passed under `/tmp/zircon-connected-mcp/connected`.
The largest supported 6/6 case also passed native inspection under
`/tmp/zircon-connected-final/{editor-6-6,game-6-6}.png`.
A real actor and observer client verified 16 authoritative movement steps from
(32,31) through passage column 40 to (48,31), with all teleport routes disabled.
This completes the bounded Wood wall-assembly phase. See RECOVERY.md for final
validation totals. Arbitrary BSP/organic perimeter recipes and movable door
objects remain separate capabilities.
