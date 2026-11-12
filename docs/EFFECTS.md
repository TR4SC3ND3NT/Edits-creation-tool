# Effects

An effect is `{ "kind": "...", "params": { ... } }` in a layer's `effects`
list. On an `image`, `solid` or `cube` layer it changes that layer before it
is blended; on an `adjustment` layer it changes everything beneath. Effects
run in list order. `edits effects` prints this registry (`--json` for
machines); `crates/edits-core/src/effects.rs` is the source of truth.

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
- **Effects at zero strength cost nothing**: they are skipped (for example
  `exposure` at 0 stops, `saturation` at 1, `gaussian_blur` at radius 0).
  `"enabled": false` skips an effect whatever its values.
- **Cost.** The colour effects (`exposure` to `tint`, `vignette`, `grain`) are
  cheap: neighbours in the list share one pass, and at the end of a layer's
  list they are folded into the blend. Each blur, `glow`, `rgb_split`,
  `transform2d` and `custom` adds at least one pass over the frame.

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

## Geometry

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `transform2d` | `scale` (1), `rotation` (0), `offset` ([0, 0]), `mirror` (1) | Scales the picture about the frame centre, turns it (degrees, clockwise) and moves it by `offset`. With `mirror` 1 the area this uncovers is filled with the picture mirrored at its edges; with 0 it is left transparent. On an adjustment layer this is a camera punch or shake applied to the finished image. |

## Custom effects

`{ "kind": "custom", "source": "shaders/mine.wgsl", "params": { "p0": 0.5 } }`
runs your own WGSL fragment. `source` is relative to the project file.
`params` are eight numbers `p0` to `p7` (default 0), animatable like any other.

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
| `fx.p0` ... `fx.p7` | `f32` | The effect's `params`. |
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
  `src_sampler`, `src_mirror_sampler`, and anything starting with `edits_` or
  `Edits`.
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

`edits check` verifies that the file exists; compiling needs the adapter, so
use `edits frame` to test a shader.

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
