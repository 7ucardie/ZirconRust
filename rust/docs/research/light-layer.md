# Research: map lighting (from the C# client)

Extracted 2026-09-12. Source: Client/Scenes/Views/MapControl.cs (nested
`Light` layer, lines ~1514-1695), Client/Rendering/LightGenerator.cs and
the blend tables in Client/Rendering/*.

## Model

Two passes. (A) A viewport-sized offscreen target ("light RT") is cleared
to a darkness colour, then additive light blobs are drawn into it with
blend `Src=SrcAlpha, Dst=One` (D3D `COLORFY`). (B) After background,
floor, tiles, objects and effects are drawn, the light RT is blitted over
the whole view with blend `Src=Zero, Dst=SrcColor` (`LIGHTMAP`, i.e. a
multiply). Names, chat bubbles, health bars, damage numbers and all UI are
drawn after the multiply, unlit.

## Darkness colour (`UpdateLights`, MapControl.cs:1657-1684)

`LightSetting` (LibraryCore/Enum.cs:333): Default 0, Light 1, Night 2,
Twilight 3, stored in `MapInfo.Light`.
- Default: grey `255 * DayTime` in each channel.
- Night: (15,15,15). Twilight: (100,100,100). Light: (255,255,255).
- User dead: whole map multiplied by IndianRed (205,92,92), no lights.
- User Abyss-poisoned: clear black + a single white blob at the user with
  scale 0.18, nothing else.

`DayTime` (0..1) comes from the server: `S.DayChanged { float DayTime }`
(broadcast on change, initial value in the start information).
SEnvir.CalculateLights: game minutes = real minutes * Config.DayCycleCount
mod 1440; Night 00:00-04:59 = 0; Dawn 05:00-07:59 ramps 0 to 1; Day
08:00-16:59 = 1; Dusk 17:00-19:59 ramps 1 to 0; Night 20:00+ = 0.
`S.TimeOfDayChanged` is cosmetic (label/events).

## Light sprite

Generated at runtime (LightGenerator.cs): 1024x768 ARGB, elliptical
radial gradient from centre rgba(200,200,200,255) to transparent at the
ellipse edge. Always drawn whole, scaled by `scale`, centred on the light
origin; the 4:3 ratio makes blobs wider than tall. Tint = source colour;
contribution = texel.rgb*tint.rgb * (texel.a*tint.a).

## Sources (`lightScale = 0.02`, `baseSize = 0.1`, cell 48x32)

- Objects with `Light > 0` (alive, or the user, or spells): scale = 0.1 +
  Light*0.04; x centre = cell centre (+CellWidth/2), y anchor = cell top
  (no +CellHeight/2); not shifted by screen shake.
  - Player: `Light = Stat.Light` from equipment (torch slot); the local user
    gets `max(3, Stat.Light)` and colour ARGB(120,255,255,255) when the stat
    is 0 (dim white), else white.
  - Monster: `Stat.Light` from MonsterInfo stats. NPC: 10 (hard-coded).
  - Spell objects: FireWall 15 OrangeRed, Tempest 15 LightSeaGreen, IceAura
    15 PaleTurquoise, BurningFire 15 OrangeRed; other fields 0.
- Effects (`MirEffect.StartLight..EndLight` interpolated, `LightColours`):
  scale = 0.1 + frameLight*0.008; centred (+CellWidth/2, +CellHeight/2).
- Map cells (`Cell.Light` = nibble*2, 0..30, within +-15 cells of view):
  scale = 0.1 + Light*0.6 (huge, deliberate), white, centred.

Colours (Globals.cs): NoneColour White, FireColour OrangeRed
(255,69,0), IceColour PaleTurquoise (175,238,238), LightningColour
LightSkyBlue (135,206,250), WindColour LightSeaGreen (32,178,170),
HolyColour DarkKhaki (189,183,107), DarkColour SaddleBrown (139,69,19),
PhantomColour Purple (128,0,128).
