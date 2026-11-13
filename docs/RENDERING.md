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
  light. A layer's signature includes the source frames, so motion blur
  instants that show the same frame share a picture.

None of this changes what a frame looks like: the picture of source frame
`n` is the same whichever way it was reached, so a frame stays a pure
function of the project and the frame number. `Renderer::video_stats()`
counts decoders started, frames decoded and cache hits.
