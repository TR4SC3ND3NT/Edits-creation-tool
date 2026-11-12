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

`edits bench project.json --passes` shows where a project's time goes.

Measured on 4 cores with lavapipe (Mesa 25.2, LLVM 20), the demo at
1080 x 1080 takes about 60 ms per frame with one motion blur sample and about
350 to 500 ms with the project's six; at `--scale 0.5`, about 20 ms and
100 to 150 ms. Opening the adapter and loading the demo's pictures takes about
0.15 s, and the first frame about 0.2 to 0.5 s more than a later one because
shaders are compiled on first use.
