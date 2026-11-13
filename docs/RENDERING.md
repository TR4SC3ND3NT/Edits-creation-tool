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
through the active layers bottom to top (skipping those with `visible:
false`): draw the layer, cut it to its masks, run its effects, multiply it by
its track matte, blend it with its mode and opacity. An adjustment layer runs
its effects on the image so far, cuts the result to its masks and matte, and
its opacity and blend mode mix that with the original. The samples are
averaged in linear light.

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
itself for effects that read the clock, mask values, the signature of its
matte source), so two instants with equal
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

## Time effects

`echo` and `time_displacement` (`src/time.rs`, passes `fs_echo` and
`fs_time_pick` in `shaders/time.wgsl`) read their input at other times, and
the renderer keeps no earlier frame, so each other time is rendered again
through the same functions that draw a layer anywhere (`src/stack.rs`):

- on a content layer, `content_before(layer, t + dt, i)` draws the layer at
  `t + dt` (camera, transform, video frame, particles, comp picture, text
  animators at that time), cuts it by its masks and runs its first `i`
  effects, the ones before the time effect, evaluated at that time;
- on an adjustment layer, `below_before` composites the layers beneath it at
  `t + dt` with `render_stack_over` (over the background of the stack, so a
  comp's transparent one inside a comp), then runs its first `i` effects.

A planned chain with a time step runs through `run_chain`, which hands the
plain runs to `run_effects` and the time steps to `time.rs`. An echo adds
one pass per picture (`fs_echo`, the operator chosen by a uniform; the
current picture at weight 1 costs no pass); a time displacement draws each
slice into one target with `fs_time_pick`, which keeps the pixels whose map
value rounds to that slice and discards the rest. A shift of 0 reuses the
input, so `interval` 0 and the middle slice of an odd `levels` render
nothing.

Every echo and slice is a full render of its input, and the motion blur
samples of a frame each render theirs: the cost is about samples x (count +
1) renders of the layer for an echo, samples x `levels` for a time
displacement, and the stack beneath for an adjustment layer. Nesting
multiplies (an echo whose input has an echo; an adjustment echo over a comp
whose layer echoes); `MAX_TIME_DEPTH` (4) refuses deeper nesting with an
error rather than run for minutes. The recursion always ends anyway: a time
effect re-renders only the effects before it or the layers beneath it, and
comps cannot contain themselves.

The motion blur sample sharing still works: a content layer's signature
includes the signatures of its input at each shifted time (without following
their time effects again), so a still layer whose echoes are still too
renders one sample. A layer with two time effects, and an adjustment layer
with one, count as changing at every instant.

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

## Video layers

`src/video.rs` reads clips through the system `ffmpeg`; nothing is linked.

- **Probing.** `prepare` runs `ffprobe -count_packets` once per file (cached by
  path, size and modification time): size, sample aspect ratio, frame rate
  (`avg_frame_rate`, else `r_frame_rate`), the number of frames (packets of
  the video stream, which needs no decoding) and whether there is sound. The
  slot's `trim` turns into a first frame and a frame count.
- **Decode size.** Frames are decoded at square pixels and the displayed
  aspect, no larger than the project's longer side and never upscaled
  (`scale=W:H:flags=bicubic` when that differs from the stored size), as
  premultiplied sRGB RGBA8. They are uploaded like pictures, with mip levels.
- **One decoder per clip, read forward.** A decoder is `ffmpeg -ss <t> -i clip
  -map 0:v:0 -pix_fmt rgba -f rawvideo pipe:1`, started half a frame before
  the frame wanted, so that the first frame it gives is exactly that frame
  whatever the container's timestamp rounding. It is then read in order: a
  forward render reads each source frame once from one process, and speed 2
  reads through the frames it skips. A frame behind the decoder, or more than
  two seconds ahead of it, restarts it at that frame.
- **Frame cache.** Every frame read goes into a least-recently-used cache with
  a byte budget (512 MB; `EDITS_VIDEO_CACHE_MB` changes it), keyed by file,
  decode size and frame index, so stepping back, motion blur samples and
  frame blending find their frames there. The last six uploaded frames stay on
  the GPU.
- **Which frame.** `edits_core::Evaluator::source_frame` maps project time to
  source frames: the layer's time (posterized by its `fps`), `time_remap` or
  the integral of `speed` (midpoint rule on a grid of two samples per project
  frame anchored at the layer's `in`, so a `hold` key lands the same way on
  every frame), then `loop` or hold, then `reverse`. `frame_blend: mix` crosses
  the frame with the next by the fraction between them, on the CPU in linear
  light; `frame_blend: flow` makes the in-between from the optical flow of the
  two frames (below). A layer's signature includes the source frames, so motion blur
  instants that show the same frame share a picture.

None of this changes what a frame looks like: the picture of source frame
`n` is the same whichever way it was reached, so a frame stays a pure
function of the project and the frame number. `Renderer::video_stats()`
counts decoders started, frames decoded, cache hits and flow fields estimated.

### Flow frame blending

`src/flow.rs` makes the picture between source frames `a` and `b` at a
fraction `w` for `frame_blend: flow`. It is written after EffectCraft's
block flow and Pixel Motion (`raster/flow.rs`, `fx_time.wgsl::ftm_motion`;
see `THIRD_PARTY.md`), with flow both ways and occlusion handling added.

1. **Flow level.** Luma (plus a quarter of alpha) of both frames, box-halved
   until the longer side is at most 1024 (1080p is estimated at 960 x 540,
   vectors doubled for the frame).
2. **Tile vectors, coarse to fine.** A box pyramid of up to 6 levels down to a
   shorter side of 16. Tiles of 8 pixels, each compared over a 12-pixel window
   by mean absolute difference: every vector within 4 pixels at the coarsest
   level; at the others every vector within 2 (so a small object the coarse
   levels blurred away is still found) and 3x3 around the doubled coarser
   vectors of the tile and its 4 neighbours and around the tile on the left.
   A 3x3 median after each level; on the finest, a parabola through the
   squared differences at the vector and either side gives sub-pixel vectors
   (an exact match stays whole).
3. **Per-pixel vectors.** Each pixel picks, from the bilinear tile vector and
   the vectors of the 3x3 tiles around it, the one whose 5x5 neighbourhood
   matches best, so motion edges follow the picture rather than the tile grid.
   Where the tiles agree (most of a frame) nothing is compared.
   Steps 2 and 3 run from `a` to `b` and from `b` to `a`.
4. **Motion at the instant.** For each pixel of the in-between, candidate
   motions are the forward vector there, the backward one reversed, and each
   followed once into the two frames. A motion `v` is *confirmed by `a`* when
   the pixel it comes from (`p - w v` in `a`) has forward vector `v`, and *by
   `b`* when the pixel it goes to (`p + (1 - w) v` in `b`) has backward vector
   `-v` (within a pixel plus 5 % of the length, falling off to nothing at
   twice that). Both together are the forward-backward consistency check
   along the path. The best confirmed candidate wins.
5. **Occlusion and fallback.** Confirmed by one frame only, a pixel is
   uncovered or about to be covered: it is taken from that frame alone,
   provided the other frame has something else there that moves consistently
   (the occluder). Otherwise (a cut, a flash, a wrong vector) neither frame is
   trusted and the pixel is the plain cross-fade, `mix`. Where both frames
   confirm the motion but the two warped samples differ by more than 0.15 in
   linear light, it also leans to `mix` (fully at 0.3).
6. **Warp and blend.** `a` is sampled at `p - w v` and `b` at `p + (1 - w) v`,
   bicubic (Catmull-Rom; plain bilinear visibly softens fine detail at
   half-pixel offsets) in linear light, and cross-faded by `w` (by the
   confirmation weights where only one frame confirms). The motion field is
   computed at the flow level and read bilinearly at the frame size.

Why this design: it is deterministic and the same on every adapter (the
pixels are a function of the two frames and `w` alone, computed in `f32` on
the CPU, split by rows over at most 8 threads with each row computed alone;
a test checks 1, 3 and 8 threads give the same bytes). It slots into the
existing CPU path of `mix` (decoded frames are on the CPU; the result is
uploaded with mip levels like any frame) and needs no GPU readback. Block
matching with per-pixel refinement copes with the large motions and flat
colour areas of anime footage better than gradient-based Lucas-Kanade, which
needs texture everywhere and many iterations. A field of zero vectors gives
exactly the `mix` picture, and two identical frames skip the estimate.

**Caching.** Flow fields are kept per (clip, frame, next frame) in a
least-recently-used list of 8 (about 8 MB each at 1080p), so the three
in-betweens of a pair at quarter speed share one estimate; the in-between
picture itself is cached like any video frame on the GPU.

**Cost** (release build, 1920 x 1080, a 2.1 GHz Xeon shared with other
builds; `cargo test --release -p edits-render --lib flow_cost -- --ignored
--nocapture`):

| | 1 thread | 4 threads |
| --- | --- | --- |
| Estimate (once per frame pair, both ways) | 106 ms | 44 ms |
| In-between (per output frame) | 260 to 340 ms | 87 to 106 ms |
| `mix`, for comparison | 35 ms | 35 ms |

At quarter speed a 1080p frame costs about 0.1 s more with `flow` than with
`mix` on 4 cores. Clips decode no larger than the project, so a 720p project
costs less than half that.

**Limits.** Large areas are followed up to about 150 pixels of the flow
level between two frames (300 at 1080p). Something small is followed only as
far as the coarser levels still see it (in the tests, a 40-pixel textured
square moving 6 pixels over a still background is); what is not followed
falls back to `mix`. Motion edges are exact to about one pixel of the flow
level (two at 1080p). Lighting changes, transparency and motion blur in the
source are cross-faded, not modelled.

## Text layers

`src/text.rs` draws `text` layers; nothing is read from the system (no
system fonts), so a frame depends only on the project and its files.

- **Layout, once per text.** The text is split into paragraphs at `\n` and
  each paragraph is shaped by harfrust (a Rust port of HarfBuzz) with the
  font at the layer's `weight` (`wght` axis). Lines are broken greedily at
  `max_width`, aligned in the block and the block centred on the layer's
  origin (see `docs/FORMAT.md`). The result is a list of glyphs, each with
  its origin in layer space (composition-height units), its advance, the
  character it comes from and its line: what per-character animation needs.
  Layouts are cached by font file and text settings.
- **One quad per glyph.** Every frame, each glyph is a quad of its own, drawn
  through the layer's matrix and the camera (`vs_geo`, `shaders/text.wgsl`),
  so text turns and moves in 3D like any layer, and a glyph can later get a
  transform of its own.
- **Masks at the output size.** A glyph's outline (skrifa) is rasterised to a
  coverage mask (zeno) for an em of its size in output pixels, to a quarter
  of a pixel, measured through the layer's matrix: text is as sharp at
  `--scale 0.5` as at 4K, and growing text is rasterised anew rather than
  magnified (up to an em of 640 pixels; beyond that it is magnified). When
  the layer is only moved and uniformly scaled, the glyph is also rasterised
  at its quarter-pixel offset and its quad put on whole pixels, so the mask
  is sampled texel for pixel. A mask has two channels: the letter, and the
  letter with its stroke (zeno's stroker, round joins, twice the `width`
  wide, centred on the outline).
- **Atlas.** The masks a layer needs at one size are packed (in the order the
  glyphs come, with a transparent pixel around each) into one `Rg8Unorm`
  texture, cached by the list of masks it holds; masks are cached by font,
  weight, glyph, size, stroke and sub-pixel offset (64 MB), the last eight
  atlases stay on the GPU. Because a mask and an atlas depend only on their
  key, the caches never change a picture.
- **Drawing.** The stroke quads of all glyphs are drawn first, then the fill
  quads, blended premultiplied "over" into a transparent frame-sized
  texture; that picture then goes through the layer's effects and blend like
  any other layer's.
- **Animators.** The core evaluates the animators
  (`edits_core::text_select`, `Evaluator::text_glyphs`) at the layer's time
  into one state per glyph: offset, scale, turn, opacity, colour, tracking
  and blur. Each glyph's quad then gets a matrix of its own (pivot at the
  middle of its advance, half a cap height above the baseline; offset,
  turn X-Y-Z, scale) between its quad and the layer's matrix, so a turned
  letter is seen in perspective through the camera. A glyph that is only
  moved in the plane is still put on whole pixels and rasterised at its
  quarter-pixel offset; a scaled or turned one is rasterised at the layer's
  size (larger when scaled up, in steps of half an octave) and drawn through
  its matrix. Opacity and colour are the draw's colour; a glyph with opacity
  or scale 0 is not drawn. Blur is done on the CPU on the glyph's mask
  (three box passes each way approximating a gaussian, sigma to a quarter
  pixel, at most 96 pixels), so it costs one small mask per glyph and blur
  value, not a pass over the frame. Glyphs do not hide each other by depth:
  they are drawn in reading order. A text layer with animators changes at
  every instant, so its picture is never reused from the previous frame.

Why not parley: parley (layout, shaping and font fallback in one crate) would
do the shaping and line breaking, but it needs the ICU4X crates, which are
under the Unicode licence, outside the MIT, Apache, zlib and OFL licences
this project accepts, and it brings about thirty crates. skrifa and harfrust
are already in the build (egui uses them), so text adds only zeno. What that
costs: no bidirectional reordering within a paragraph, no font fallback, and
line breaking by simple rules instead of the Unicode line-breaking algorithm.

## Particle layers

A particles layer's state comes from `edits_core::particles::evaluate(layer,
t)`, which lists the particles alive at the layer's time `t` in closed form:
the rate particles born in `(t - life, t]` (evenly spaced at the rate; a keyed
rate is integrated on a grid of half frames anchored at the emission start, so
a particle's birth time never changes), the burst particles of bursts in that
window, each with random numbers hashed from (`seed`, stream, index). Motion
is ballistic with linear drag, `x(t) = x0 + v0 f + g (t - f) / k` with
`f = (1 - e^(-k t)) / k` (and `v0 t + g t² / 2` without drag), plus a curl
noise drift: the curl of a two-octave 3D value-noise potential at the
particle's position and the eddies' phase, minus the same at its birth.
Nothing is carried between frames, so seeking costs nothing and every motion
blur sample is exact.

`src/particles.rs` turns that list into one instance per particle (centre,
axis, half extents, colour and opacity, in layer space) and draws them all
with one instanced draw of `shaders/particles.wgsl` into a frame-sized
transparent texture, through the camera and the layer's model matrix like a
flat layer, blended "over" (premultiplied) in list order (oldest first). The
sprites are drawn procedurally in the fragment shader (`dot`, `spark` along
the velocity, `star`, `petal`) or sampled from a slot's picture. A particle
smaller than a pixel and a half across is drawn at that size and fainter by
the area it lacks, so it neither flickers nor disappears. The texture comes
back to the stack as one quad covering the frame, so the layer's effects,
opacity, blend mode and transition apply as for any layer. Its signature
includes its time, so every motion blur instant is rendered.

## Shape layers

A shape layer is drawn on the CPU and handed to the GPU as one picture.
`edits_core::shape::resolve` reads every value of its shapes at the layer's
time; `src/shape/geom.rs` makes kurbo bezier paths of the geometries;
`src/shape/ops.rs` applies the operators in order (trim by arc length,
cutting at `inv_arclen`; the repeater as one path per copy with its own
opacity; round corners as a cubic arc with handles `4/3 tan(phi / 4) r`
between points cut at `min(radius, half of each neighbouring segment)`;
`offset_path` by kurbo's `expand_path`; zigzag and wiggle by resampling each
segment at equal arc lengths and moving the points along the normal or by
hashed value noise). Strokes, with their caps, joins and dashes, are
expanded to outlines by kurbo in the shape's own space, so the shape's and
the layer's scale scale them too.

`src/shape/raster.rs` then carries every outline through the layer's matrix
(through the camera) into output pixels. When that matrix has no
perspective, the beziers map exactly (a 2D affine); under perspective they
are flattened to a tenth of a pixel in layer space and the points projected.
zeno rasterises each outline to 8-bit coverage over its bounding box clipped
to the frame (nonzero, or even-odd for a fill that asks), and the colours
are composited "over" in linear light, premultiplied, into a frame-sized
picture uploaded as RGBA16Float. That picture comes back to the stack as a
quad covering the frame, like a particles layer's, so effects, masks,
matte, opacity, blend mode and transitions apply as usual.

Shapes are drawn at the output size, so they are as sharp at `--scale 0.5`
as at 4K. The last picture of each shape layer is kept with everything it
depends on (the resolved values, the matrix, the output size; at most 8
layers): a still shape on a still layer is rasterised once however many
frames show it (`Renderer::shape_rasterisations` counts them). A layer's
signature includes its resolved shapes, so keyed shapes, a moving wiggle
and a jitter drawn every frame render at every motion blur instant, while
still ones share one. zeno rather than tiny-skia: it was already in the
build for glyphs, it only has to fill (kurbo strokes), and the colour stays
in linear light like the rest of the renderer.

## Masks and track mattes

Masks are not rasterised paths. Each is a signed distance evaluated per pixel
(`src/mask.rs`, `shaders/mask.wgsl`; the same formulas as `edits_core::mask`
on the CPU): the pixel is mapped back onto the layer's plane through the
inverse of the 3 x 3 homography that the layer's model-view-projection
matrix gives on its plane z = 0 (so a mask stays on a layer turned in 3D;
pixels whose ray misses the plane in front of the camera count as outside),
turned into the mask's own frame, and measured against the shape: exact
distances for rectangles and rounded rectangles, an approximation that is
exact on the outline for ellipses (`k0 (k0 - 1) / k1` with `k0 = |q / r|`,
`k1 = |q / r²|`), and the nearest edge with an even-odd sign for polygons.
After `expansion` is taken off, the coverage is `clamp(0.5 - d / w, 0, 1)`
with `w = hypot(feather, fwidth(d))`: a linear ramp `feather` wide centred
on the outline, or one pixel wide (anti-aliasing) without feather. One
full-frame pass per mask combines that coverage with the coverage so far
(read from the previous pass) by the mask's mode; the last pass multiplies
the layer's drawn picture, before its effects run. Polygon corners travel in
the pass's uniform block, which is why a polygon has at most 32.

A track matte is one more full-frame pass, after the layer's effects: the
source is drawn by `layer_content` at the same instant (its own masks,
effects and matte included; chains are followed at most 16 deep), and the
layer's premultiplied picture is multiplied by `alpha x opacity`, or by
`luma(encode(rgb / alpha)) x alpha x opacity` (Rec. 709 weights on the
display-referred colour, so a mid-grey source gives a half), or one minus
either. A layer with masks or a matte never takes the single-pass path of
flat layers that cover the frame. Colour effects at the end of a chain still
ride along with the blend: they work on straight colour, which a mask or a
matte does not change.


## Comps (precomps)

`prepare` turns every comp the project draws into a scene of its own
(`src/comp.rs`): `Project::comp_project` gives the comp as a project (its
size, duration and layers, no motion blur, the default camera) that shares
the project's prepared pictures, clips, shaders, fonts and tables, plus the
scenes of the comps it draws in turn. A comp that would contain itself is
left out of its own scene, so drawing it fails with "contains itself"
instead of recursing (`edits check` names the cycle).

A `comp` layer's picture is `render_stack_over(comp.layers, comp_time, w,
h, background)` with the comp's scene swapped in while it renders, so the
evaluator, layer ends, track mattes, the camera and the aspect all see the
comp. `comp_time` (`Evaluator::comp_time` in the core) is the video time
mapping (`speed` integrated from `in`, or `time_remap`) in the project's time
base, plus `offset`, then looped, held just short of the end, or reversed.
The comp is `width x height` project pixels times the output scale; a comp
of the project's size is exactly the output size. Its texture is drawn as a
quad of the comp's size in the parent's height units, then masks, effects,
matte, blend, opacity and transition apply as for an image. When that quad
lands on the frame pixel for pixel (a frame-sized comp, no transform), the
blend reads it texel for texel instead of resampling it, so a comp of
opaque layers gives the same bytes as its layers inline.

Comp pictures are cached for the output frame by (comp, comp time, size),
four at most: a comp drawn twice at one instant (repeated panels, a comp used
as a matte and drawn) renders once. A comp layer's signature is the
signature of the comp's stack at its comp time, so a still comp costs one
motion blur sample and a moving one is rendered at every sample.
