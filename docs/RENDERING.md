# How frames are rendered

`crates/edits-render` turns a project and a frame number into pixels. This
page fixes the conventions [FORMAT.md](FORMAT.md) leaves open and says what
the renderer does to be fast. Effects have their own page: [EFFECTS.md](EFFECTS.md).

## Using the library

```rust
let project = edits_core::Project::from_json(&std::fs::read_to_string("project.json")?)?;
let mut renderer = edits_render::Renderer::new(edits_render::RenderOptions::default())?;
renderer.prepare(&project, std::path::Path::new("."))?;   // loads pictures, compiles custom shaders
let frame = renderer.render_frame(37, 1.0)?;              // image::RgbaImage, straight sRGB, top row first
let preview = renderer.render_at(3.0, 0.25)?;             // the picture at project time 3.0 exactly, quarter size
```

Nothing needs a window. `RenderOptions::adapter` (or `EDITS_ADAPTER`) is
`auto`, `hardware` or `software`; `Renderer::info()` reports the adapter's
name, backend and whether it is a CPU rasteriser. `Renderer::new` returns an
error, not a panic, when there is no adapter. `prepare` can be called again
with an edited project: unchanged pictures and shaders are reused (files are
recognised by path, size and modification time).

## Colour

- Working textures are RGBA16Float, linear light, premultiplied alpha.
- `settings.background` and a solid's `color` are sRGB (a solid's alpha is
  straight). Colours in effect parameters are linear.
- Pictures are decoded from sRGB when sampled and are premultiplied at load,
  in linear light, so transparent edges filter without fringes. They carry
  mip levels and are sampled trilinearly.
- Opacity, all eight blend modes and motion blur work in linear light. Blend
  modes use the usual formulas on un-premultiplied colour; `screen` is
  `b + s - clamp(b) * clamp(s)` so that values above 1 are not darkened.
- The output is clipped to 0..1 and encoded to sRGB RGBA8; alpha is 255.

## Geometry

Units are the composition height; the origin is the frame's centre; x points
right, y down, z away from the viewer.

- **Rotation** is degrees about X, then Y, then Z. Positive Z turns clockwise
  on screen. Positive Y brings the right-hand side toward the viewer (a cube
  turned +90 about Y shows its right face). Positive X brings the top toward
  the viewer.
- **Layer transform**: anchor, scale, rotate, translate. An image or solid
  layer's box is the frame (aspect x 1) centred on the origin; `anchor` is
  the pivot, in the same units from the box's centre.
- **Camera**: a perspective camera with vertical `fov` at distance
  0.5 / tan(fov / 2) in front of the plane z = 0, so that plane always fills
  the frame. `camera.position` moves it (x right, y down, z forward);
  `camera.rotation` turns it with the same matrices as a layer: +Y pans
  right, +X tilts up, +Z rolls clockwise.
- **Cube**: six quads of `size` [w, h, d] centred on the layer's origin,
  drawn with a depth buffer and back-face culling. Faces are front, right,
  back, left, top, bottom (a short `faces` list repeats its last slot). Each
  picture is cover-fitted to its face and is upright when its face is turned
  to the front: side faces by a Y rotation, top and bottom by an X rotation.
- **Fit**: `cover` crops the picture to fill its box, keeping `focus` as
  close to the centre as the picture's edges allow; `contain` fits it inside
  and leaves the rest of the box transparent; `stretch` ignores its aspect.
- **Empty slots** (no `path`) render a 1:1 placeholder: a checker tinted by
  the slot's name, a frame, two diagonals and the name.
- **Edges**: flat layers fade their edges by pixel coverage; cubes are drawn
  with 4x multisampling (`RenderOptions::msaa`).

## A frame

For each motion blur sample, the renderer starts from the background and goes
through the active layers bottom to top: draw the layer, run its effects,
blend it with its mode and opacity. An adjustment layer runs its effects on
the image so far (its opacity and blend mode mix the result with the
original). The samples are averaged in linear light.

In the code (`src/stack.rs`) this is two functions of time that everything
else is built on:

- `layer_content(layer, t) -> Option<Tex>`: one layer's own picture at
  project time `t`, drawn through the camera with its whole effect chain into
  a frame-sized texture, before its opacity, blend mode and transition. `None`
  for an adjustment layer or a layer not visible at `t`.
- `render_stack(layers, t, w, h) -> Tex`: a list of layers composited bottom
  to top over the background at `t` (`render_stack_over` takes another
  background, such as transparent black).

A frame is `render_stack(project.layers, t)` at each motion blur instant,
averaged. A layer's `layer_signature(t)` lists everything its picture at `t`
depends on (transform, opacity, transition progress, effect values, and `t`
itself for effects that read the clock), so two instants with equal
signatures share a picture. The public `Renderer::render_layers(layers, t,
scale)` and `Renderer::render_layer(layer, t, scale)` read these back as
images, which is how the tests check that a stack of all layers is the frame.

Sample `i` of `N` is at the frame's start time plus
`i / N x shutter x frame length`; `shutter` is clamped to 0..1, so a frame
never looks past the next cut. A frame is a pure function of the project and
the frame number: rendering it twice gives the same bytes on the same adapter.

## Blend modes

A layer's `blend` combines its straight colour `s` with the straight colour
`b` of the image beneath, in linear light; the result is mixed in by the
layer's coverage (alpha x opacity) as in the W3C "Compositing and Blending"
model: `out = sa (1 - da) s + sa da B(b, s) + (1 - sa) b_premultiplied`. The
formulas are W3C's, with EffectCraft's mode list, and kept sensible for
light above 1 (a flash, a glow, an exposure push) in three ways:

- **As they are**: `normal` (s), `add` and `linear_dodge` (b + s),
  `multiply` (b s), `lighten` (max), `darken` (min), `difference` |b - s|,
  `subtract` max(b - s, 0), `linear_burn` max(b + s - 1, 0), `linear_light`
  max(b + 2s - 1, 0), `pin_light` (s <= 0.5: min(b, 2s), else max(b, 2s - 1)),
  `lighter_color` / `darker_color` (whichever colour has the higher / lower
  Rec. 709 luminance). In `screen` (b + s - b' s'), `overlay` (b <= 0.5:
  2 b s, else screen(s, 2b - 1)), `hard_light` (overlay with b and s
  swapped) and `exclusion` (b + s - 2 b' s') the product terms use b' and s'
  clamped to 0..1, so bright light never turns dark or negative.
- **Clamped, plus the backdrop's excess**: `color_burn`, `color_dodge`,
  `soft_light` (W3C's), `vivid_light`, `hard_mix` and `divide` (b / s, at
  most 1) are defined for 0..1. They run on b and s clamped to 0..1, and then
  add back max(b - 1, 0), so a highlight beneath stays as bright as it was.
- **Scaled**: `hue`, `saturation`, `color` and `luminosity` (W3C's SetLum,
  SetSat and ClipColor, with Rec. 709 luminance) run on b and s divided by
  their brightest component when that is above 1, and the result is
  multiplied back.

Each mode is its own generated pass (`src/compose.rs`), compiled the first
time it is used; `tests/render.rs` checks every mode against these formulas
on a grid of values below and above 1.

## Multi-pass effects

`bloom`, `light_rays`, `outline`, `drop_shadow` and `tilt_shift` are chains
of fixed passes (`src/fx.rs`, entry points in `shaders/passes.wgsl`, each
pipeline compiled the first time it is used):

- **bloom**: a soft threshold pixel by pixel, then Bevy's mip chain: a 13-tap
  downsample to half size with Karis' average, more 13-tap downsamples, and
  3x3 tent upsamples back to half size that add each level times its weight.
  The weight of a level depends on its texel size in % of height (`x^2
  exp(-x^2/2)` with x = texel / radius), so a frame at half scale, which lacks
  only the finest level, blooms the same. The last pass is the glow's final
  add, shared with the colour effects after it.
- **light_rays**: the glow's bright pass at half size, then a zoom streak
  that reaches from each pixel toward the centre only (the first of the
  streak passes is shifted by half a streak), then a pass that adds, screens
  or lightens it into the picture.
- **outline**: a jump flood (Rong and Tan 2006) in a half-float texture that
  holds, per pixel, the offset to the nearest pixel inside the shape and to
  the nearest outside: log2(width) + 2 passes of nine reads, and one pass
  that draws the stroke from the signed distance.
- **drop_shadow**: the gaussian blur of the layer (skipped at softness 0),
  then one pass that draws its offset alpha beneath the layer.
- **tilt_shift**: two passes of glfx.js's variable-radius triangle filter,
  along the band and across it.

`lens_flare` is a single analytic pass.

## What keeps it fast

The renderer is built to be usable on a CPU rasteriser (Mesa lavapipe), where
every pass over the frame costs milliseconds.

- **Still moments cost one sample.** Sub-frame instants at which every value
  of the project evaluates the same are rendered once and weighted.
- **Effects at zero strength are skipped**, and a chain that is entirely
  skipped costs nothing.
- **Pointwise work shares passes.** Blending, colour effects and the glow's
  final add are generated as straight-line shaders per combination in use
  (`src/compose.rs`): a full-frame layer whose effects are all colour effects
  is drawn, graded and blended in one pass.
- **Big blurs run small.** Gaussian blurs and glows work on a copy halved
  until sigma is about two texels; streak blurs take four samples per pass
  and repeat with shorter streaks, so three passes give 64 even samples.
- **Textures are pooled** and bind groups cached, so a frame allocates
  nothing after the first.
- **One-pass effects are compiled per option.** The built-in distort and
  stylise effects (`src/fxlib/`, `shaders/fx/`) are each one full-frame pass
  with the custom-effect contract (`shaders/fx/prelude.wgsl`: sixteen
  parameters, `src`, `src_mirror`, an auxiliary picture and a 3D lookup
  table). Their options are written into the source as constants, so every
  combination in use is its own pipeline, compiled the first time it is
  needed and cached; a CPU rasteriser never runs the branches of the other
  options. Lookup tables are uploaded as RGBA16Float 3D textures and sampled
  trilinearly; an effect without one gets a 2x2x2 identity.

`edits bench project.json --passes` shows where a project's time goes.

Measured on 4 cores with lavapipe (Mesa 25.2, LLVM 20), the demo at
1080 x 1080 takes about 60 ms per frame with one motion blur sample and about
350 to 500 ms with the project's six; at `--scale 0.5`, about 20 ms and
100 to 150 ms. Opening the adapter and loading the demo's pictures takes about
0.15 s, and the first frame about 0.2 to 0.5 s more than a later one because
shaders are compiled on first use.
