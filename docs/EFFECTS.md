# Effects

An effect is `{ "kind": "...", "params": { ... }, "options": { ... } }` in a
layer's `effects` list. On an `image`, `solid` or `cube` layer it changes that
layer before it is blended; on an `adjustment` layer it changes everything
beneath. Effects run in list order. `edits effects` prints this registry
(`--json` for machines; the MCP tool `effects_list` shows the same);
`crates/edits-core/src/effects/` is the source of truth, one file per
category (`color.rs`, `blur.rs`, `light.rs`, `distort.rs`, `stylize.rs`,
`time.rs`, `generate.rs`).

## Rules that hold for every effect

- **Any parameter can be keyframed** like a transform value, which is how
  hits are made: `"stops": { "keys": [ { "t": 4, "v": 4, "ease": { "name": "decay", "rate": 0.25 } }, { "t": 5, "v": 0 } ] }`.
  Before its first key a value holds that key's value, so an effect that
  should be off until a hit needs a key that says so:
  `{ "t": 0, "v": 0, "ease": "hold" }`.
- **Sizes are percent of the frame height** where the table says "% of
  height": a blur `radius` of 2 is 21.6 pixels at 1080 and 10.8 at `--scale 0.5`.
  Positions (`center`, `offset`) are in composition units like layer
  positions: height = 1, origin at the centre, x right, y down.
- **Colour is linear light, premultiplied alpha.** Colour parameters are
  linear RGB in 0..1. Values above 1 are kept between effects (an `exposure`
  flash followed by `glow` blooms hard) and clipped only at the output.
  Effects marked *display* work on sRGB-encoded values, because that is what
  the look they imitate does in an editor: `brightness_contrast`, `invert`,
  `threshold`, `posterize`, `grain`.
- **A layer's effects see a frame-sized picture** of that layer, transparent
  where the layer is not. A blur spreads into the transparent part; a
  `vignette` or `zoom_blur` is centred on the frame, not on the layer.
- **Every parameter has a unit and may have a range.** `edits effects --json`
  gives each parameter's `unit`: `""` (a plain number), `"%h"` (percent of
  height), `"deg"`, `"pos"` (`[x, y]` in composition units), `"color"`
  (linear `[r, g, b]`), `"x"` (a factor), `"count"` (rounded to a whole
  number), `"stops"`, `"px1080"` (pixels at 1080 high) or `"time"`; and its
  `range` `[min, max]` (`null` for an open end). The renderer clamps a value
  to its range, and `edits check` warns when a written value is outside it.
  A `color` must have 3 numbers and a `pos` 2 (or one number for all):
  anything else is an error.
- **Options are choices that cannot be animated**, such as a pixelate's cell
  shape: `"options": { "shape": "hex" }`. Each option has a fixed list of
  values (the first is the default); `edits check` rejects an unknown option
  or value and lists the allowed ones:
  ```
  error: layers[1:"fx"].effects[0].options.shape: "star" is not a value of pixelate's option "shape" (one of: square, hex, triangle, polar (default square))
  ```
- **Credits.** Effects whose maths was ported from another project say so in
  their `credit` (`edits effects` prints it); see
  [`THIRD_PARTY.md`](../THIRD_PARTY.md).
- **Effects at zero strength cost nothing**: they are skipped (for example
  `exposure` at 0 stops, `saturation` at 1, `gaussian_blur` at radius 0).
  `"enabled": false` skips an effect whatever its values.
- **Cost.** The colour effects (`exposure` to `tint`, `vignette`, `grain`) are
  cheap: neighbours in the list share one pass, and at the end of a layer's
  list they are folded into the blend. Each blur, `glow`, `rgb_split`,
  `transform2d` and `custom` adds at least one pass over the frame; each
  distort and stylise effect below (`twirl` to `halftone`) is exactly one.

## Colour

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `exposure` | `stops` (0) | Multiplies by 2^stops. +1 doubles the light. Black stays black, so +4 is the white flash in which only dark ink survives. |
| `fade` | `amount` (0), `color` ([1, 1, 1]) | Mixes toward `color`: 0 unchanged, 1 solid colour. On a layer the layer's shape is kept. |
| `brightness_contrast` | `brightness` (0), `contrast` (0) | *Display.* Adds `brightness` (-1..1), then scales about mid grey by tan((contrast + 1) x 45 deg): -1 is flat grey, 0 unchanged, 0.5 is x2.4, 1 is two-tone. |
| `saturation` | `amount` (1) | Mixes with the luminance (Rec. 709): 0 is black and white, 2 doubles saturation. |
| `hue` | `degrees` (0) | Rotates colours about the grey axis: 120 takes red to green. |
| `invert` | `amount` (1) | *Display.* Negative: each channel becomes 1 - value (after clipping to 0..1). `amount` mixes with the original. |
| `threshold` | `level` (0.5), `softness` (0.02), `amount` (1) | *Display.* Two-tone ink: luminance above `level` becomes white, below black, over a transition `softness` wide. `level` 0.5 is mid grey (sRGB 128). |
| `posterize` | `levels` (4) | *Display.* Rounds each channel down to one of `levels` evenly spaced values (at least 2). |
| `tint` | `color` ([1, 1, 1]), `amount` (1) | Replaces colour by luminance x `color`, mixed by `amount`. |
| `vignette` | `amount` (0.5), `radius` (0.75), `softness` (0.5) | Multiplies by 1 - amount x smoothstep(radius, radius + softness, d), where d is the distance from the frame centre as a fraction of the half-diagonal (so it is round, and 1 at the corners). |
| `grain` | `amount` (0.05), `size` (1) | *Display.* Adds grey noise of up to +-`amount` (triangular). `size` is the grain's cell in pixels at 1080 high. The pattern depends only on the pixel and the frame number: the same frame always gets the same grain. |

## Blur and light

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `gaussian_blur` | `radius` (1) | Gaussian blur whose sigma is `radius` % of height. Large blurs are computed on a reduced copy, so cost barely grows with radius. The frame's edge pixels are repeated outward. |
| `directional_blur` | `angle` (0), `length` (2) | Even streak along a line, `length` % of height long in total and centred on each pixel. Angle 0 is horizontal; positive turns clockwise. |
| `zoom_blur` | `strength` (0.1), `center` ([0, 0]) | Even streak along rays from `center`: each pixel averages the picture scaled between e^(-strength/2) and e^(+strength/2), so the streak's length is about `strength` x the distance from the centre. The centre itself stays sharp. |
| `spin_blur` | `angle` (5), `center` ([0, 0]) | Even streak along circles about `center`, sweeping `angle` degrees in total. |
| `glow` | `threshold` (0.7), `radius` (3), `intensity` (1), `color` ([1, 1, 1]) | Bloom. Keeps the light above `threshold` (linear luminance L: colour x max(L - threshold, 0) / L), blurs it with sigma `radius` % of height, multiplies by `intensity` x `color` and adds it back. Around a layer the halo carries its own transparency, so later effects and blend modes treat it as part of the layer. |
| `rgb_split` | `amount` (0.5), `angle` (0) | Chromatic aberration: red is moved `amount` % of height against the direction `angle`, blue the same distance along it, green stays. |
| `tilt_shift` | `center` ([0, 0]), `angle` (0), `width` (20), `blur` (2) | Miniature look. A band `width` % of height wide through `center` (turned `angle` degrees clockwise from horizontal) stays sharp; beyond it the blur grows smoothly over another half `width` to `blur` % of height, the half-width of a triangle filter run along the band and then across it. Skipped below a quarter pixel of blur. |

## Light (multi-pass)

These run several passes each (`crates/edits-render/src/fx.rs`, passes in
`shaders/passes.wgsl`). Light they add outside a layer brings its own
coverage, as `glow` does, so later effects and blend modes see it as part of
the layer. Each is skipped when its strength is zero.

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `bloom` | `threshold` (0.8), `knee` (0.2, 0..1), `intensity` (1), `radius` (10), `color` ([1, 1, 1]) | A deep, soft glow, wider and smoother than `glow`. Each pixel keeps the light above `threshold` (on its largest channel), with a soft start `knee` x `threshold` wide below it; the result is halved again and again (a mip chain), and the levels are added back up, each weighted by its size: levels near `radius` % of height weigh most, much smaller and much larger ones little. The sum (weights add up to 1) is multiplied by `intensity` x `color` and added to the picture, so the light added grows in proportion to `intensity`. Below `threshold` - `knee` x `threshold` nothing changes at all. Because the weights follow sizes in % of height, a render at `--scale 0.5` blooms the same (the tests check it within 5 %). |
| `light_rays` | `center` ([0, 0]), `length` (0.5, 0..0.95), `intensity` (1), `threshold` (0.6), `color` ([1, 1, 1]); option `mode` = `add` / `screen` / `lighten` | God rays. The light above `threshold` (linear luminance, as `glow`) is streaked away from `center`: each pixel averages the bright pass scaled toward `center` by factors from 1 - `length` to 1, so a bright shape casts rays outward that are `length` x its distance from the centre long, and nothing appears between the centre and the shape. The rays, times `intensity` x `color`, are added (`add`), screened (`screen`) or kept where brighter (`lighten`). The rays are computed at half size. |
| `lens_flare` | `position` ([-0.4, -0.25]), `brightness` (1), `size` (1); option `lens` = `zoom` / `prime35` / `prime105` | A flare from a light at `position`: a bright core, streaks (8, 6 or 12 by lens) and a thin halo ring around it, and coloured ghosts (discs and rings) along the line from the light through the frame's centre and beyond. Sizes are fractions of the frame's diagonal times `size`; everything times `brightness` is added to the picture. It draws over the whole frame, so put it on an adjustment layer, and key `position` to make it travel. |
| `outline` | `width` (0.6), `color` ([1, 1, 1]), `softness` (0), `opacity` (1, 0..1); option `position` = `outside` / `inside` / `center` | A stroke around the layer's shape (where its alpha is at least one half), `width` % of height wide: beneath the layer just outside the edge, over it just inside, or half of each. Found with a jump flood, so the cost grows with the logarithm of the width only. With `softness` 0 the stroke's edges are antialiased over one pixel; `softness` (% of height) widens that ramp. On an adjustment layer it outlines everything beneath. |
| `drop_shadow` | `offset` ([0.01, 0.015]), `softness` (1), `color` ([0, 0, 0]), `opacity` (0.6, 0..1) | A copy of the layer's alpha, moved by `offset` (composition units, y down), blurred with sigma `softness` % of height, in `color` at `opacity`, drawn beneath the layer. Nothing is pulled in from outside the frame. |

## Distort

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `transform2d` | `scale` (1), `rotation` (0), `offset` ([0, 0]), `mirror` (1) | Scales the picture about the frame centre, turns it (degrees, clockwise) and moves it by `offset`. With `mirror` 1 the area this uncovers is filled with the picture mirrored at its edges; with 0 it is left transparent. On an adjustment layer this is a camera punch or shake applied to the finished image. |
| `twirl` | `angle` (90), `radius` (30), `center` ([0, 0]) | Twists the picture about `center`. A point at distance d < `radius` (% of height) is turned clockwise by `angle` x (1 - d / radius)^2 degrees, so the centre turns most and the edge of the disc not at all; outside the disc nothing moves. Key `angle` from 0 for a twist-in. Skipped at angle 0. |
| `bulge` | `amount` (0.5, -1..1), `radius` (25), `center` ([0, 0]), `taper` (0, 0..1) | A lens over a disc of `radius` % of height: inside it each pixel reads the picture at the distance d x (1 - amount x (1 - d^2)^(1 + 3 taper) / 2) from the centre (d in radii). `amount` 1 doubles the size at the centre, -1 pinches it to two thirds; `taper` makes the dome sharper. Outside the disc nothing moves. Skipped at amount 0. |
| `kaleidoscope` | `segments` (6, 2..64), `center` ([0, 0]), `rotation` (0), `zoom` (1); option `mirror` = `on` / `off` | Folds the picture into `segments` wedges about `center`: every wedge shows the first one (from `rotation` degrees, clockwise), mirrored in turn with `on`, repeated with `off`. `zoom` magnifies the folded picture. The frame's edges are mirrored, so zooming out shows no smear. Always runs. |

## Stylise

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `chromatic_aberration` | `amount` (1), `center` ([0, 0]), `samples` (8, 2..32) | Radial lens fringing. Each pixel averages `samples` points on the line through it from `center`, weighted by a spectrum from red (the point nearest the centre) through green to blue (the farthest), so the red picture is larger and the blue one smaller. The spread grows linearly with the distance from `center` and is `amount` % of height at the frame's corners (from the frame centre); at `center` nothing moves. Unlike `rgb_split`, it is zero in the middle and smooth. Skipped below a tenth of a pixel. |
| `pixelate` | `size` (2), `center` ([0, 0]); option `shape` = `square` / `hex` / `triangle` / `polar` | Mosaic: flat cells `size` % of height across, each the colour of the picture at its centre, on a grid with a cell centred on `center`. `hex` are hexagons `size` wide, `triangle` equilateral triangles with sides of `size`, `polar` rings `size` wide about `center`, cut into segments about `size` long. Key `size` down to 0 for a de-pixelate reveal. Skipped at size 0. |
| `halftone` | `size` (1.2), `angle` (45), `smoothing` (0.15, 0..1), `amount` (1, 0..1); option `mode` = `mono` / `rgb` / `cmyk`; option `shape` = `dot` / `line` / `square` | A print screen: a grid of cells `size` % of height apart turned to `angle` degrees, each holding a dot (or a line, or a square) whose area is the fraction of the cell the colour asks for, in linear light, so the picture's mean brightness is kept (within a few percent). The colour of a cell is the average of five points of it. `mono`: black ink on white by luminance. `rgb`: red, green and blue light on black, screens at `angle` - 30, `angle`, `angle` + 30. `cmyk`: cyan, magenta, yellow and black ink on white at `angle` - 30, `angle`, `angle` - 45, `angle` + 30 (15, 45, 0 and 75 by default). `smoothing` softens the dot edges; `amount` mixes with the original. Transparent parts stay transparent. Skipped at amount 0. |

The built-in one-pass effects (the last three tables, except `transform2d`)
are WGSL files in `crates/edits-render/src/shaders/fx/`, written against the
same contract as a custom effect (below), with their options compiled in. To
add one: a row in the category's file in `crates/edits-core/src/effects/`, a
WGSL file, a packer in `crates/edits-render/src/fxlib/` (it turns the
effect's values into `fx.p0` .. `fx.p15` and says when it changes nothing), a
pixel test in `crates/edits-render/tests/effects.rs`, and a line here. Ported
code keeps its licence header and gets an entry in `THIRD_PARTY.md`.

## Custom effects

`{ "kind": "custom", "source": "shaders/mine.wgsl", "params": { "p0": 0.5 } }`
runs your own WGSL fragment. `source` is relative to the project file.
`params` are sixteen numbers `p0` to `p15` (default 0), animatable like any other.

### The contract

The file must define one function:

```wgsl
fn effect(uv: vec2<f32>) -> vec4<f32>
```

It is called once per pixel. `uv` is the pixel's position in the frame:
(0, 0) is the top-left corner, (1, 1) the bottom-right. Return the new pixel
as **linear-light RGBA with premultiplied alpha** (the same form `src`
returns). The file may define any other functions, constants and structs it
needs.

The renderer provides:

| Name | Type | Meaning |
| --- | --- | --- |
| `src(uv)` | `fn(vec2<f32>) -> vec4<f32>` | The picture the effect applies to, bilinear, clamped at the edges. |
| `src_mirror(uv)` | `fn(vec2<f32>) -> vec4<f32>` | The same, mirrored at the edges. |
| `fx.resolution` | `vec2<f32>` | Size of the frame being rendered, in pixels. |
| `fx.inv_resolution` | `vec2<f32>` | 1 / resolution: one pixel in `uv` units. |
| `fx.aspect` | `f32` | Width / height. |
| `fx.time` | `f32` | Project time in seconds. |
| `fx.beats` | `f32` | Project time in beats (0 when the project has no `bpm`). |
| `fx.frame` | `f32` | Output frame number, from 0. |
| `fx.p0` ... `fx.p15` | `f32` | The effect's `params`. |
| `fx_to_comp(uv)`, `fx_to_uv(p)` | `fn(vec2<f32>) -> vec2<f32>` | uv to composition units (height 1, origin at the centre, y down) and back, so positions and sizes can be written like a layer's. |
| `fx_delta_uv(d)`, `fx_pixel()` | | A displacement in composition units as uv; one pixel in composition units. |
| `fx_hash(x, y, z)`, `fx_hash2(p, seed)`, `fx_noise(p, seed)` | `-> f32` | Exact integer hash to 0..1; the hash of the cell `floor(p)`; smooth value noise in 0..1 with features one unit apart. |
| `fx_luma(c)`, `fx_straight(c)` | | Rec. 709 luminance; premultiplied to straight colour. |
| `fx_aux(uv)`, `fx_lut(c)` | | A second picture and a 3D lookup table, for effects that have them; for a custom effect they are opaque white and the identity. |
| `src_texture`, `src_sampler`, `src_mirror_sampler` | texture and samplers | For calling `textureSample*` yourself. |

Rules worth knowing:

- **Stay resolution independent.** The shader runs at whatever size is being
  rendered (`--scale 0.25` for a contact sheet, 1 for the final video), so
  express sizes as fractions of the frame (`uv` units, or multiples of
  `fx.inv_resolution.y` scaled by a parameter), not as pixel counts.
- **Time moves within a frame.** With motion blur the shader runs once per
  sub-frame sample, each with its own `fx.time` and `fx.beats`, and the
  results are averaged. `fx.frame` is the same for all samples of a frame.
- **Be deterministic.** No inputs other than the ones above exist, so the
  same frame renders the same way every time; derive any randomness from
  `uv`, `fx.frame` and your parameters.
- **Names to leave alone**: `fx`, `src`, `src_mirror`, `src_texture`,
  `src_sampler`, `src_mirror_sampler`, and anything starting with `fx_`,
  `edits_` or `Edits`.
- A custom effect is never skipped, whatever its parameters.

### Errors

Custom shaders are compiled when the project is loaded (before the first
frame of `render`, `frame`, `sheet` or `bench`). A shader that does not
compile stops the command with the layer, the file and the compiler's message;
line numbers are those of your file:

```
error: layer "a", effect 0: custom effect shader "shaders/broken.wgsl" does not compile:
Validation Error

Caused by:
  In Device::create_shader_module, label = 'shaders/broken.wgsl'

Shader 'shaders/broken.wgsl' parsing error: no definition in scope for identifier: `strength`
  ┌─ wgsl:3:16
  │
3 │     return c * strength;
  │                ^^^^^^^^ unknown identifier
(the file must define `fn effect(uv: vec2<f32>) -> vec4<f32>`; see docs/EFFECTS.md)
```

`edits check` verifies that the file exists. To check the shader itself
without a GPU, `edits shader check` compiles it the way the renderer will
(your file plus the appended contract, through naga, the WGSL compiler wgpu
uses) and points at the line in your file; it exits 1 when the shader would
not compile:

```
$ edits shader check shaders/bad.wgsl
error: shaders/bad.wgsl:3:24: no definition in scope for identifier: `nope`
    return c * fx.p0 + nope;
$ edits shader check examples/shaders/ripple.wgsl
examples/shaders/ripple.wgsl: ok (reads p0, p1, changes with time)
```

It also refuses a file without `fn effect(uv: vec2<f32>) -> vec4<f32>` or
with another signature. With `--json` it prints `{ ok, issues: [{ line,
column, code, message }], params, animated }`. The MCP tool `effect_define`
runs it before writing a shader next to a project.

### Examples

[`examples/shaders/scanlines.wgsl`](../examples/shaders/scanlines.wgsl): CRT
scanlines that roll slowly, with colour bleed. `p0` is the strength of the
lines, `p1` the bleed.

```wgsl
fn effect(uv: vec2<f32>) -> vec4<f32> {
    // Sizes are fractions of the height, so the look is the same at any scale.
    let lines = 180.0;
    let roll = fx.time * 0.05;
    let wave = 0.5 + 0.5 * cos((uv.y + roll) * lines * 6.2831853);
    let shade = 1.0 - fx.p0 * wave * wave;

    let bleed = vec2<f32>(fx.p1 * 0.004 / fx.aspect, 0.0);
    let centre = src(uv);
    let rgb = vec3<f32>(src(uv + bleed).r, centre.g, src(uv - bleed).b);
    return vec4<f32>(rgb * shade, centre.a);
}
```

[`examples/shaders/ripple.wgsl`](../examples/shaders/ripple.wgsl): a ring that
spreads from the centre on every beat (`fx.beats`) and bends the picture as
it passes. `p0` is the push, `p1` the ring's width.

A project that uses both:

```json
{
  "format": "edits.project/1",
  "settings": { "width": 640, "height": 360, "fps": 25, "duration": 4, "time_base": "beats", "bpm": 120 },
  "slots": { "a": {} },
  "layers": [
    { "id": "a", "kind": "image", "slot": "a",
      "effects": [
        { "kind": "custom", "source": "shaders/ripple.wgsl", "params": { "p0": 1, "p1": 0.5 } },
        { "kind": "custom", "source": "shaders/scanlines.wgsl", "params": { "p0": 0.6, "p1": 1 } } ] }
  ]
}
```

Saved next to a `shaders` folder holding the two files, `edits sheet
project.json -o sheet.png` shows the ring leaving the centre at every beat.
The slot is empty, so the picture is the renderer's placeholder.

## Transitions

A transition is not an effect in a layer's `effects` list: it is the layer's
`transition` field, `{ "kind", "duration", "params", "ease" }`, and it
replaces the cut into the layer by a mix of two pictures over `duration`
from the layer's `in`: the frame beneath the layer ("from") and the frame
with the layer ("to"). See [FORMAT.md](FORMAT.md#transitions). The registry
is `TRANSITIONS` in `crates/edits-core/src/effects/mod.rs`; `edits transitions`
prints it (`--json` for machines), `edits check` validates kinds and
parameters against it, and the MCP `effects_list` lists it too. The preset
`transition:<kind>` sets the field and overlaps the two layers around the
cut (`duration` in beats, `align` `center` / `after` / `before`).

The 22 transitions are ported from the
[gl-transitions](https://gl-transitions.com) collection: the WGSL files of the
`gl-transitions` crate 2.0.2 are used unchanged (their headers keep source,
author and licence; all are MIT; see
`crates/edits-render/src/shaders/transitions/LICENSE.md`). The renderer
rebinds their textures, reads both pictures in sRGB (the space they were
written for) and writes the result back in linear light. Colour parameters
are sRGB 0..1; directions and centres are in the shader's own frame (0..1,
y up). Default length in beats in brackets.

| Kind | What it looks like | Parameters (defaults) |
| --- | --- | --- |
| `fade` (0.5) | A plain crossfade | none |
| `fade_color` (1) | Dip through a colour between the pictures | `color` [0, 0, 0, 1], `color_phase` 0.4 |
| `flash_fade` (0.5) | A crossfade that flares toward a colour | `color` [1, 1, 1] |
| `overexposure` (0.5) | A crossfade through a washed-out white | `strength` 0.6 |
| `lightning_flash` (1) | Lightning bolts, white strobes, the next picture lit by the last flickers | `bolts` 2, `strike` 0.55, `seed` 0, `color` [0.75, 0.85, 1], `glow` 0.5, `flashes` 3 |
| `cross_zoom` (0.5) | A crossfade through a strong zoom blur | `strength` 0.4 |
| `simple_zoom` (0.5) | Zoom into the outgoing picture, then fade | `zoom_quickness` 0.8 |
| `zoom_in_out` (1) | Zoom in until the picture is flat colour, then out of the next | none |
| `dreamy_zoom` (1) | Zoom and rotate into a bright flash, then out | `rotation` 6, `scale` 1.2 |
| `directional` (0.5) | The next picture pushes the outgoing one out (smooth slide) | `direction` [0, 1] |
| `directional_warp` (0.5) | A diagonal wipe with a smeared edge | `smoothness` 0.1, `direction` [-1, 1] |
| `whip_pan` (0.5) | A fast pan that streaks both pictures | `direction` [-1, 0], `blur` 0.4 |
| `whip_spin` (0.75) | A roll that speeds up in a spin blur; the next picture slows to a stop | `turns` 1, `blur` 0.5, `chroma` 0.6, `zoom` 0.25 |
| `whip_tilt` (0.75) | A fast tilt to the next picture, with perspective | `down` 0, `blur` 0.5, `chroma` 0.5, `perspective` 0.7 |
| `camera_shake` (0.75) | Violent jolts with blur and colour fringes; the cut at the peak | `strength` 0.5, `jolts` 12, `blur` 0.5, `split` 0.5 |
| `glitch_memories` (0.5) | A crossfade with glitchy colour-shifted blocks | none |
| `glitch_displace` (0.5) | Displaced blocks that flash into the next picture | none |
| `cube` (1) | The pictures are faces of a box that turns, over a reflective floor | `persp` 0.7, `unzoom` 0.3, `reflection` 0.4, `floating` 3, `shading` 0.5 |
| `swirl` (1) | A swirl that peaks mid-way and untwists on the next picture | `strength` 1, `radius` 0.9, `center` [0.5, 0.5] |
| `rotate_scale_fade` (1) | The outgoing picture spins and zooms away over a colour | `center` [0.5, 0.5], `rotations` 1, `scale` 8, `back_color` [0.15, 0.15, 0.15, 1] |
| `pixelize` (0.5) | Both pictures pixelate, crossfade and sharpen | `squares_min` [20, 20], `steps` 50 |
| `film_burn` (1) | Glowing film-burn light leaks reveal the next picture | `seed` 2.31 |

`crates/edits-render/tests/transitions.rs` renders every one: it starts on the
picture beneath, ends on the plain cut, does something visible in between and
stays opaque. To add one: copy its WGSL from the crate (check its licence
header), add it to `TRANSITIONS` and to the table in
`crates/edits-render/src/transitions.rs` (parameter offsets come from the
crate's uniform layout), and document it here.
