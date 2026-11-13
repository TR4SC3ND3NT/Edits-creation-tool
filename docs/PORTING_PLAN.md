<!-- Research note written for planning (2026-11-13). Licences and paths were read from shallow clones; re-check before porting. -->
# Gap analysis and porting plan: `edits` vs EffectCraft and other permissive sources

Date: 2026-11-13. Read-only research. No cloned project was built or run, and
`/home/user/Edits-creation-tool` was not modified. Clones were made in the scratchpad and deleted at the end.

Sources read:

- EffectCraft `storytold/effectcraft` at HEAD on 2026-11-13 (workspace version 0.24.1, MIT OR Apache-2.0).
- Licence files, cloned sparsely: Bevy (`bevy_post_process`, `bevy_core_pipeline`, `bevy_anti_alias` at 0.20.0-dev), three.js `examples/jsm/shaders` and `postprocessing`, pmndrs/postprocessing `src/effects`, glfx.js, Vidvox ISF-Files, GPUImage (file list), Skia `modules/skottie` (file list), libpag, ThorVG, Rive runtime, lottie-spec, lygia, parley, velato.
- crates.io metadata for every crate named below.

---

## 0. Where we stand

| Area | `edits` today | Files |
| --- | --- | --- |
| Layer kinds | `image`, `solid`, `cube`, `adjustment` | `crates/edits-core/src/model.rs` (405 lines) |
| Media | still pictures through slots (`image` crate). Video only in `edits-analyze/src/media.rs` (ffprobe/ffmpeg subprocess) | `edits-render/src/assets.rs` |
| Effects | 19, registry in `edits-core/src/effects.rs` (68 lines); colour ops fused in `compose.rs`; multi-pass in `fx.rs` (304 lines); 3 WGSL files (287 lines) | `fx.rs`, `compose.rs`, `shaders/passes.wgsl` |
| Custom effect | WGSL `fn effect(uv) -> vec4`, `p0..p7`, `src()` / `src_mirror()`, `edits_aux_texture` declared but unused | `shaders/custom.wgsl` |
| Blend modes | 8 (normal, add, screen, multiply, overlay, lighten, darken, difference) | `compose.rs::blend_expr` |
| Time | keyframes and eases, `shake` / `loop` modifiers, motion blur by sub-frame samples, beats time base | `eval.rs`, `ease.rs` |
| Missing | video, text, masks, track mattes, precomps, shapes, particles, time effects (echo, posterize time), time remap, extra blend modes, LUTs, distortions, glitch, halftone, flares | |

The renderer is a wgpu **fragment-shader** pipeline (RGBA16F, linear and premultiplied), tuned for lavapipe.
A frame is a pure function of the project and the frame number. Any feature that keeps state between
frames (feedback trails, a stepped particle simulation) has to become a closed-form function of time, or
re-render earlier times. This rule decides several port-versus-rewrite questions below.

---

## 1. EffectCraft inventory

### 1.1 Shape of the project

- About 600,000 lines of Rust in 32 crates and 3 apps. Their own `docs/gaps.md` says "≈ 361,000 lines of almost
  entirely agent-written Rust", first commit 1 October 2026, 216 open issues on 12 October, and fidelity against
  After Effects "unmeasured" beyond one scalar-ease comparison.
- The design is an **After Effects clone**. Parameter names, units (pixels, percent of layer, AE popups) and
  ids (`ec.distort.twirl`) follow AE. Every effect has a **CPU reference** (`crates/effects`, rayon over
  `effectcraft_raster::Image`, f32 premultiplied). 280 of 308 also have a **GPU twin** as a wgpu **compute** shader
  (`crates/gpu/src/shaders/fx_*.wgsl`, 16,177 lines), checked against the CPU within 1/255.
- The GPU kernels share one layout: `P.u` and `P.f` uniform arrays, `src` and `aux` textures read with `textureLoad`,
  `textureStore` to an output storage texture, and helpers in `common.wgsl` such as their own `sample_bilinear`,
  `asin_p` and `sincos_p`. **They do not drop into our fragment pipeline as files.** The math in each entry point is
  self-contained, though, and moves into our `fn effect(uv)` form with the I/O lines rewritten.
- Evaluation depends on their property tree (`PropGroup`, `EvalCtx`, `Tick` = 254,016,000,000 per second, `Buf` with
  "effect space" bounds). Anything in `render/`, `effects/` or `text` render code that reads `ctx.params` or
  `layer.props` is **entangled**. The pure leaf modules are not: `text/selectors.rs`, `path/ops.rs`,
  `raster/flow.rs`, `color/blend.rs`, `effects/utility.rs::parse_cube`, and the per-effect math closures.
- Licence: MIT OR Apache-2.0, the same as ours. Copyright line in `NOTICE`: "Copyright (c) 2026 ArtCraft Team and the
  EffectCraft contributors." The ArtCraft name and logos are trademarks; do not copy anything from `docs/brand/`.
- Video and audio codecs come from git dependencies (`storytold/filmcraft`, `storytold/craft-libs`). They are pure-Rust
  H.264, HEVC, ProRes, AV1 and VP9 decoders and encoders, plus AAC and their own Opus encoder. They are not in this
  repository.

### 1.2 Feature inventory: EffectCraft → ours → port source → licence → effort

Sizes are lines of Rust including tests. "Entangled" means it reads their property tree or `EvalCtx`. Effort is for
our version: S is under 2 agent-hours, M is 2–5, L is one session or more.

| Feature | EffectCraft (paths, size, deps) | Tests there | Ours | Port or rewrite | Licence | Effort |
| --- | --- | --- | --- | --- | --- | --- |
| Layer kinds | `project/src/layer.rs` `LayerSource`: Footage, Comp, Solid, Text, Shape, Null, Camera, Light, Model, Primitive; adjustment is a switch | yes | image, solid, cube, adjustment | Model only. Add `video`, `text`, `comp`, `particles`, `shape`; skip null, light and model | MIT/Apache | — |
| Footage decode + frame cache | `media/src/pool.rs` (1,493): LRU by byte budget, prefetch, keyed by path, frame, interpretation and size. Decoding via `filmcraft-media` / `filmcraft-codecs` (git deps) | 140 tests in crate | none | **Rewrite** around the system ffmpeg; borrow the cache design (byte-budget LRU, prefetch the next frame, key = path + frame index + size) | MIT/Apache (design only) | L |
| Frame position / frame blending | `render/src/frames.rs` (166): `frame_position` (exact tick math), `blend_frames` (mix or pixel motion) | `tests/frame_blend.rs` (176) | none | Port `frame_position` logic (trivial); mix is one pass | MIT/Apache | S |
| Optical-flow frame interpolation | `raster/src/flow.rs` (383, deps: rayon): hierarchical block matching, SAD, parabola sub-pixel fit, 3×3 vector median, `interpolate(a, b, w)`. GPU warp `gpu/src/shaders/fx_time.wgsl::ftm_motion` (60 lines) | 61 tests in raster crate | none | **Port** `flow.rs` nearly verbatim (pure, CPU); port `ftm_motion` into a fragment pass | MIT/Apache | M–L |
| Time remap / stretch | `render/src/eval.rs::source_time` (time remap property or stretch); `effects/src/time_fx.rs::timewarp_integrate_speed` (integrates an animated speed) | `tests/remap.rs`, `tests/time.rs` | none | Rewrite in `eval.rs`; port the idea of `timewarp_integrate_speed` for an animated `speed` | MIT/Apache | M |
| Posterize Time | `time_fx.rs::posterized_time` (6 lines) | yes | none | Rewrite as a layer field `fps` | — | S |
| Echo | `time_fx.rs::echo` (60): operators add, max, min, screen, composite in back or front; frames fetched by `fetch` through `EffectEnv` | yes | none | Port the operator math; rewrite fetching (re-render the layer at earlier times) | MIT/Apache | M |
| Masks | `render/src/masks.rs` (391): CPU coverage from `path` via tiny-skia, expansion, feather (box-blur grid), modes add, subtract, intersect, lighten, darken, difference; mask motion blur. `path/src/feather.rs` (345): variable feather | `tests/mask_*.rs` (≈440) | none | **Rewrite on the GPU** with analytic SDFs (rect, ellipse, polygon) and feather by smoothstep. Their CPU coverage is too slow per frame on lavapipe at 1080p, and is entangled | MIT/Apache (semantics) | M |
| Track mattes | `MatteKind` alpha, alpha inverted, luma, luma inverted (`project/src/layer.rs:440`), applied in `render/src/pipeline.rs` | in `render/tests` | none | Rewrite (one multiply pass) | — | M |
| Precomps | `LayerSource::Comp`; `pipeline.rs` (1,419) renders nested comps, collapse transformations, responsive time | `tests/nested.rs`, `tests/collapse.rs` | none | Rewrite (recursion in our renderer). Their code depends on the item and `Tick` model | — | M |
| Text engine | `text/` (6,524): `fonts.rs` (system font DB), `layout.rs` (1,347: harfrust shaping, unicode-bidi, unicode-linebreak), `lib.rs` (skrifa outlines into kurbo `BezPath`), `path_text.rs` | 97 | none | Do not port the layout; use **parley** (shaping, layout and font fallback in one crate, MIT/Apache). Their stack is the same family (harfrust + skrifa) assembled by hand | MIT/Apache | — |
| Text animators and selectors | `text/src/selectors.rs` (408, **no dependencies**): Range (start, end, offset, shapes square, ramp up, ramp down, triangle, round, smooth; smoothness, ease high and low), random order, Wiggly, combine modes | 6 unit tests | none | **Port verbatim** to `edits-core/src/text_select.rs` | MIT/Apache | S |
| Text rendering | `render/src/text.rs` (1,270): `CharXf` per-character state (offset, scale, rotation XYZ, skew, opacity, fill, stroke, tracking, blur, variable axes), CPU raster per glyph | `text/tests.rs` (813) | none | Rewrite: glyph atlas plus per-glyph quads in our 3D quad renderer. Keep the `CharXf` field list as a checklist | MIT/Apache (design) | L |
| Shape layers | `render/src/shapes.rs` (820) plus `path/` (4,419): `ops.rs` (695) trim, round corners, pucker and bloat, offset, twist, zig zag, wiggle, merge; `boolean.rs` (linesweeper); `offset.rs`; `varstroke.rs`; raster via tiny-skia | 66 in path, 747 lines in shapes tests | none | **Port `path/src/ops.rs` and `offset.rs`** (kurbo-only). Rewrite the stacking semantics in a smaller model | MIT/Apache | L |
| Particles | `effects/src/sim/` (≈2,600): `engine.rs` is a stepped simulation from layer time 0 through `SimCache`; `weather.rs` (rain, snow, bubbles) is **closed form**; `sprites.rs` is a band rasteriser; GPU `fx_particles.wgsl` (255) and curl noise | `sim/tests.rs`, `particle_golden.rs` | none | Rewrite: closed-form particles drawn as instanced quads. Borrow the `weather.rs` approach and the curl-noise WGSL | MIT/Apache | M–L |
| Blend modes | `color/src/blend.rs` (538, serde only): 38 modes | 26 tests in color crate | 8 | **Port the per-channel formulas** into `compose.rs::blend_expr` | MIT/Apache | S–M |
| Layer styles | `render/src/styles.rs` (765): drop shadow, outer and inner glow, stroke, bevel, satin and overlays; exact EDT (Felzenszwalb–Huttenlocher) on CPU | `tests/styles.rs` (540) | none | Rewrite the ones that matter (stroke or outline, drop shadow) as effects; use jump flooding on the GPU instead of the CPU EDT | — | M |
| Motion blur | per layer, shutter angle and phase, adaptive samples (`frames.rs::adaptive_samples`) | yes | global samples, still detection | Keep ours; optionally add per-layer `motion_blur: false` | — | S |
| 3D (cameras, lights, materials, Advanced 3D) | `render/src/three_d/` (≈7,000), `model/` glTF and OBJ | many | camera plus cubes | **Skip.** Low value for this genre | — | — |
| Tracking | `track/` (14,780): NCC plus Lucas–Kanade, homography, camera tracker | 21 files | none | Skip for now | — | — |
| Segmentation | `segment/` (MobileSAM, MediaPipe), `sam2/` (candle) | yes | none | Skip; if wanted later, cut out offline with an external tool | model licences vary | — |
| Expressions and scripting | `expr/` (boa JS, 5,706), `script/` (9,285) | yes | modifiers | **Skip.** Agents edit JSON directly; modifiers and presets cover it | — | — |
| Lottie | `lottie/` (3,943) on `craft-lottie` (git dep) | 3 fixtures | none | Use a crate instead (velato or rasterlottie) | — | M |
| PSD, SVG, PDF, AEP | `psd/`, `svg/`, `pdf/`, `aep/` (≈60,000) | yes | none | Skip; SVG via `resvg` if ever needed | — | — |
| Encode | pure-Rust H.264, HEVC, AV1, VP9, ProRes, Opus | yes | system ffmpeg | Skip (see patents, §2.4) | — | — |
| Audio | `render/src/audio.rs` (584) mixdown; 13 audio effects | yes | one audio track passed to ffmpeg | Rewrite as an ffmpeg filter graph | — | S–M |

### 1.3 EffectCraft effects: what this genre would use

308 effects in total. The full category list is in `docs/effects.md` of their repository. These are the ones worth
having in an anime or manga edit, with their source location:

| Their effect (id) | Source (CPU / GPU) | Note for us |
| --- | --- | --- |
| Twirl `ec.distort.twirl` | `effects/src/distort.rs::twirl` / `fx_distort.wgsl::dst_twirl` | 18 lines; radius is a % of the layer's larger side (ours: % of height) |
| Bulge `ec.distort.bulge` | `distort.rs::bulge` / `dst_bulge` | taper and pin all edges |
| Wave Warp `ec.distort.wavewarp` | `distort.rs::wave_shape`, `wave_pin`, `wave_warp` / `dst_wavewarp` | 9 wave shapes, 9 pinning modes |
| Ripple `ec.distort.ripple` | `distort.rs::ripple` / `dst_ripple` | |
| Mirror, Offset, Polar Coordinates | `distort.rs::mirror`, `offset`, `polar` | "Polar to Rect" sets the flag named `to_polar` (index 1): the naming is confusing; check before porting |
| Spherize | `distort.rs::spherize` | |
| Displacement Map, Turbulent Displace | `effects/src/distort2.rs` / `dst_displace`, `dst_turbulent` (fbm) | displacement map needs a second texture |
| Lens Distortion / Optics Compensation | `distort2.rs` / `dst_cclens` | |
| Mosaic `ec.stylize.mosaic` | `misc.rs` / `dst_mosaic_*` (4 kernels: row and tile averages, then output) | averaging needs a reduction pass; a "sharp" variant samples the cell centre |
| Find Edges, Emboss | `misc.rs` / `dst_findedges`, `dst_emboss` | |
| Kaleidoscope `ec.stylize.cckaleida` | `stylize2.rs::kaleida` (20 lines) | 5 modes: 6, 8, 4, 12 or 3 segments |
| Motion Tile, Edge Tile Repeat | `stylize2.rs` | |
| Hexagonal Tiles, Glass Relief, Film Burn (`ccburnfilm`), Block Reveal | `stylize3.rs` / `fx_pixel2.wgsl` | |
| Glow `ec.stylize.glow` | `misc.rs` plus `fx_stylize.wgsl` extras | ours exists |
| Strobe | `stylize2.rs::strobe_on` | keyframes already do this |
| Light Rays (CC) | `generate2.rs::light_rays` / `fx_light.wgsl::flt_rays` | 32 radial samples with a mask; four transfer modes |
| Radial Light Burst, Light Sweep | `generate2.rs` / `fx_light.wgsl` | |
| Lens Flare | `generate2.rs::lens_flare`, `flare_ghosts` (tables of ghost positions, sizes and colours) | 3 lens types |
| VR Digital Glitch | `vr.rs::vr_glitch` | horizontal bands with hash-based shift and RGB split, scanlines. **Bug:** `scanlineSpacing` is in buffer pixels and is not scaled by `b.scale`, so half-resolution renders differ |
| Echo, Posterize Time, Time Displacement | `time_fx.rs` / `fx_time.wgsl` | |
| Add Grain, Noise, Fractal Noise | `noise.rs`, `noise3.rs::fractal_gpu` | ours has `grain` |
| Gradient Ramp, 4-Color Gradient, Fill, Stroke, Write-on | `generate*.rs` | |
| Drop Shadow | `misc.rs` | |
| Apply Color LUT | `utility.rs::parse_cube` (`.cube` 1D and 3D, DOMAIN_MIN and DOMAIN_MAX) | port the parser; sample the 3D LUT ourselves |
| Tritone, Colorama, Levels, Curves, Hue/Saturation, Vibrance, Channel Mixer, Black & White, Shift Channels, Channel Blur | `color_fx.rs`, `color2.rs`, `color3.rs`, `channel*.rs` / `fx_color.wgsl`, `fx_tone.wgsl` | |
| Particle World, Rainfall, Snowfall, Star Burst, Bubbles | `sim/` / `fx_particles.wgsl` | |

Not in EffectCraft: **halftone or screentone**, **film scratches or dust** (their "Dust & Scratches" is a denoiser),
**VHS or bad TV**, **scanlines** (only inside VR glitch), **pixel-sort or JPEG block glitch**, **shockwave ring**,
**Kuwahara**, **toon**. These come from the sources in §2.

### 1.4 Code-quality notes on EffectCraft (agent-written)

- Their own assessment: fidelity "≈ 50%, mostly unmeasured"; many features marked done had bugs found within days.
  Treat behaviour as a starting point, not a reference.
- Units are AE units in pixels of the layer buffer, scaled by `b.scale`. Some effects forget the scale (VR Digital
  Glitch scanline spacing above). Every ported constant must be converted to our "% of height".
- Doc comments have become detached during edits. In `render/src/frames.rs` the comment "The layer's frame blending
  mode…" sits on `field_frame`, and "Convenience: render one frame of a comp…" sits on `adaptive_samples`.
  Read code, not comments.
- Echo's Minimum operator uses an `f32::MAX` sentinel and tests only channel 0 when resetting it. This is harmless
  but sloppy.
- The CPU/GPU twin discipline is good: a test checks every GPU kernel against the CPU at 1/255, and a test checks
  that every effect that reads the clock is listed in `TIME_DEPENDENT`. The pure modules worth porting
  (`selectors.rs`, `flow.rs`, `ops.rs`, `blend.rs`, `parse_cube`) all have unit tests.
- `#![deny(clippy::unwrap_used, …, panic)]` and "never crash" rules hold across crates, so ported code is free of
  panics.

---

## 2. Other permissive sources

Licences below were read from the repositories' licence files in this session, except where marked "crates.io",
which comes from crates.io metadata.

### 2.1 Licence table

| Source | Licence (verified) | Use | Keep this attribution |
| --- | --- | --- | --- |
| EffectCraft | MIT OR Apache-2.0 | port: selectors, path ops, flow, blend formulas, `.cube` parser, effect math | "Portions derived from EffectCraft (https://github.com/storytold/effectcraft), Copyright (c) 2026 ArtCraft Team and the EffectCraft contributors, used under the MIT License." Plus the MIT text in `THIRD_PARTY.md` |
| Bevy 0.20-dev `bevy_post_process`, `bevy_core_pipeline`, `bevy_anti_alias` (WESL) | MIT OR Apache-2.0 | port: `bloom/bloom.wesl` (193), `effect_stack/chromatic_aberration.wesl` (87), `effect_stack/lens_distortion.wesl` (52), `effect_stack/vignette.wesl` (76), `dof/dof.wesl` (241), `tonemapping.wesl` (438), `robust_contrast_adaptive_sharpening.wesl` (95) | "Portions derived from Bevy (https://github.com/bevyengine/bevy), Copyright the Bevy contributors, MIT License." CAS derives from AMD FidelityFX CAS (MIT); keep that note |
| three.js `examples/jsm/shaders` | MIT ("Copyright © 2010-2026 three.js authors") | port GLSL to WGSL: HalftoneShader, DotScreenShader, DigitalGlitch, FilmShader, RGBShiftShader, KaleidoShader, MirrorShader, SobelOperatorShader, TechnicolorShader, BleachBypassShader, AfterimageShader (only as an idea: it is feedback, see Echo) | "Portions derived from three.js, Copyright © 2010-2026 three.js authors, MIT License." Some files credit outside authors in comments; copy those credits |
| pmndrs/postprocessing `src/effects` | **zlib** ("Copyright © 2015 Raoul van Rüschen") | port: ChromaticAberration, Glitch, Scanline, Noise, DotScreen, Pixelation, **ShockWave**, GodRays, LUT3D, TiltShift, Bloom (mipmap blur), Outline | zlib: keep the notice in ported source files and mark them as altered |
| glfx.js | MIT ("Copyright (C) 2011 by Evan Wallace") | port: `warp/bulgepinch.js`, `warp/swirl.js`, `fun/hexagonalpixelate.js`, `fun/dotscreen.js`, `fun/colorhalftone.js`, `fun/ink.js`, `fun/edgework.js`, `blur/tiltshift.js`, `adjust/vibrance.js`, `adjust/curves.js` (spline), `adjust/huesaturation.js` | MIT notice |
| Vidvox ISF-Files (371 files) | MIT for the repository ("Copyright (c) 2018"), with a per-file `CREDIT` | port files credited "by VIDVOX" or "by zoidberg": RGB Halftone, CMYK Halftone, Bad TV, Glitch Shifter, JPEG Block Corruption, Chromatic Aberration, Shockwave Pulse, Kaleidoscope, Lens Flare, God Rays, Slit Scan, Pixel Shifter, Echo Trace, Toon, Sketch, Edges, Neon, Thermal Camera | MIT notice plus the file's CREDIT. **Reference only (Shadertoy origin, CC BY-NC-SA by default):** ASCII Art, Broken LCD, Dirty Lens, Perlin Transition, Smoke Screen, Smudged Lens, Boxinator. **Check the origin first:** VHS Glitch ("original by Staffan Widegarn Åhlvik"), Dither-Bayer ("Hugh Kennedy", probably glsl-dither, MIT). Files "converted from gl-transitions" belong to the other agent |
| GPUImage | BSD-3-Clause ("Copyright (c) 2012, Brad Larson, Ben Cochran, …") | port shaders: Halftone, PolkaDot, Crosshatch, Sketch, ThresholdSketch, Toon, SmoothToon, **Kuwahara**, Pixellate, PolarPixellate, Swirl, Bulge, Pinch, Stretch, SphereRefraction, GlassSphere, Emboss, Posterize, CGAColorspace, ZoomBlur, MotionBlur, TiltShift, Vibrance, Lookup, **blend modes** (SoftLight, HardLight, ColorDodge, ColorBurn, Exclusion, Divide, Subtract, Hue, Saturation, Color, Luminosity), JFAVoronoi (jump-flooding reference) | BSD-3: reproduce the notice in docs; do not use the authors' names to endorse. Skip Amatorka, MissEtikate and SoftElegance (third-party LUT images) |
| Skia / Skottie | BSD-3 ("Copyright (c) 2011 Google Inc.") | reference or port: `modules/skottie/src/text/{RangeSelector,TextAnimator,TextAdapter}.cpp`; `layers/shapelayer/{TrimPaths,Repeater,RoundCorners,OffsetPaths,PuckerBloat,MergePaths}.cpp` | BSD-3 notice |
| libpag | Apache-2.0, except third-party components listed in its LICENSE.txt | reference (PAG text animators, mattes); C++ | Apache NOTICE if code is ported |
| ThorVG | MIT ("Copyright (c) 2020 - 2026 ThorVG Project") | reference for the Lottie text and shape-modifier implementation | MIT notice |
| Rive runtime | MIT ("Copyright (c) 2020 Rive") | reference (state machines are not needed) | — |
| Lottie spec | Community Specification License 1.0 | implement the format (only for Lottie import) | — |
| lygia | **Prosperity Public License 3.0 (non-commercial, 30-day commercial trial)** | **reference only, do not copy** | — |
| Shadertoy (default) | CC BY-NC-SA 3.0 | **reference only** | — |
| parley 0.13 | MIT OR Apache-2.0 | dependency: text layout, shaping and fallback | crate dependency |
| skrifa 0.50, harfrust 0.16 | MIT OR Apache-2.0 / MIT (crates.io) | dependency (through parley, or directly for outlines) | — |
| kurbo 0.13, linesweeper 0.5 | MIT OR Apache-2.0 (crates.io) | dependency: paths, path ops, booleans | — |
| tiny-skia 0.12 | BSD-3-Clause (crates.io) | dependency: CPU path raster for shapes, masks and glyphs | — |
| vello 0.12, vello_cpu 0.4, velato 0.12 | Apache-2.0 OR MIT | optional: vector raster on the CPU (vello_cpu); Lottie (velato) | — |
| rasterlottie 0.3 | MIT OR Apache-2.0 (crates.io) | optional: headless Lottie rasteriser (pure Rust, deterministic) | — |
| fontdue 0.9, swash 0.2 | MIT/Apache/Zlib (crates.io) | alternatives for glyph rasterisation | — |
| ffmpeg-sidecar 2.7 | MIT (crates.io) | optional: subprocess ffmpeg frame iterator; our own `media.rs` already does this | — |
| ffmpeg-next 9.1 | WTFPL bindings, but links libav* (LGPL or GPL) | **avoid**: linking and build burden | — |
| symphonia 0.6 | MPL-2.0 (crates.io) | optional audio decode; acceptable (file-level copyleft) | — |
| aubio-rs | GPL-3.0 | **avoid** | — |

### 2.2 2026 Rust motion-graphics crates found (crates.io)

| Crate | Version, licence | What it is | Use |
| --- | --- | --- | --- |
| `oxframe` | 0.24.3, MIT OR Apache-2.0 | "Motion graphics and video from Rust, built for AI agents" | A direct competitor. Worth a read for API ideas; not a port source (not reviewed here) |
| `rasterlottie` | 0.3.0, MIT OR Apache-2.0 | pure-Rust headless Lottie rasteriser | Candidate for a Lottie overlay layer |
| `velato` | 0.12.0, Apache-2.0 OR MIT | Lottie on vello (linebender) | Alternative for the same layer |
| `rustmotion-core` | 0.8.1, MIT | motion rendering core | Not reviewed in depth |
| `reelforge-*` | 0.2.1, MIT | programmatic video composition | Not reviewed in depth |
| `motion-rs`, `motionloom`, `luminafx-text` | very young, few downloads | — | Ignore |
| `bevy_video_glitch` | 0.2.0, MIT OR Apache-2.0 | glitch shader for Bevy | Small reference for glitch |

### 2.3 Decode paths for video layers (no new codecs)

- **What we have.** `crates/edits-analyze/src/media.rs` probes with `ffprobe -show_streams -of json` (fps from
  `avg_frame_rate`, falling back to `r_frame_rate`, plus `nb_frames`, duration and audio presence). It decodes through
  `ffmpeg … -f rawvideo` into memory. `crates/edits-cli/src/encode.rs` pipes RGBA into `ffmpeg` and trims and attaches
  the audio. Both find the binary through `EDITS_FFMPEG` or `PATH`.
- **Recommended.** Use the same subprocess approach inside `edits-render`: one long-lived decoder process per clip
  and size:
  `ffmpeg -ss <t0> -i clip -an -fps_mode cfr -r <clip_fps> -vf scale=W:H:flags=bicubic -pix_fmt rgba -f rawvideo pipe:1`.
  Read frames in order. Restart on a backwards seek or a jump of more than 2 seconds. Keep a byte-budget LRU of decoded
  frames, keyed by (path, frame index, size), and prefetch the next frame. Input-side `-ss` is frame-accurate when
  transcoding. Forcing CFR makes the frame index equal `floor((t − start_time) × fps + 1e-6)` even for variable-frame-rate
  phone clips.
- **Without ffmpeg.** Animated GIF and WebP stickers decode through the `image` crate we already depend on (features
  `gif` and `webp` are on). Image sequences (`frame_%04d.png`) also need no ffmpeg. A `edits media import` command
  could convert a clip to a PNG or QOI sequence once, which turns seeking into file reads.
- **Pure-Rust codecs (FilmCraft, openh264 built from source, and similar) are not recommended.** H.264 and HEVC are
  covered by patent pools (Via LA, Access Advance). MIT code carries no patent licence; Cisco's royalty coverage only
  applies to Cisco's own prebuilt openh264 binary. AV1 (rav1d, BSD-2) and VP9 are royalty-free but rare as inputs. We
  already depend on ffmpeg for output, so the system ffmpeg is the only path that avoids both patent questions and new
  codec code.

---

## 3. What to port from EffectCraft, and what to rewrite

| Port (pure, tested, convention-neutral) | Why |
| --- | --- |
| `text/src/selectors.rs` → `edits-core/src/text_select.rs` | No dependencies; exactly the range and wiggly maths an animator needs; 6 tests come with it |
| `path/src/ops.rs` and `offset.rs` (kurbo) → `edits-render/src/shape/ops.rs` | Trim, round corners, zig zag, pucker and bloat, twist, wiggle, offset are fiddly to get right; already tested |
| `raster/src/flow.rs` → `edits-render/src/flow.rs` | Block-matching optical flow for "twixtor-like" slow motion; rayon only |
| `color/src/blend.rs` formulas → `compose.rs::blend_expr` | 30 extra blend modes as one-line expressions each |
| `effects/src/utility.rs::parse_cube` → `edits-render/src/lut.rs` | `.cube` parser with domain handling and tests |
| Per-effect math (twirl, bulge, wave shapes and pinning, ripple, kaleidoscope, polar, mirror, light rays, flare ghost tables, echo operators, glitch bands, displacement, fbm) | Moved into our `fn effect(uv)` WGSL form; constants converted to % of height |

| Rewrite | Why |
| --- | --- |
| Footage pool and decoding | Built on FilmCraft codecs (patents, git deps); our ffmpeg path is simpler |
| Masks | Their CPU coverage plus box-blur feather costs too much per frame on lavapipe; GPU SDFs fit our pipeline |
| Text rendering and layout | Entangled with `PropGroup`; parley does layout, and our 3D quad renderer can draw per-glyph quads (per-character 3D for free) |
| Precomps, mattes, time remap | Entangled with their item model and `Tick`; in ours each is a small change to the render loop |
| Particles | Their particle systems are stepped from time 0 through a cache. We need closed form, as in their `weather.rs` |
| Layer styles | Their CPU EDT is slow per frame; jump flooding on the GPU is cheap |
| Any GPU kernel file as a file | Compute shaders with their bind layout; only the bodies carry over |

---

## 4. Ranked list for anime and manga edits

Score = value to an edit maker (1–5) × ease (1–5). This confirms the expected list, with three corrections:

- **Echo / posterize-time and the video speed ramp rank above precomps.** Velocity edits and choppy 12 fps "anime
  cuts" are staples of the genre.
- **Halftone, screentone and lineart effects are not in EffectCraft.** They come from ISF, three.js, glfx.js and
  GPUImage.
- **Glow should become a Bevy-style mip-chain bloom.** "Deep glow" is the most requested look in this genre.

| Rank | Item | Value | Ease | Score | Main source |
| ---: | --- | ---: | ---: | ---: | --- |
| 1 | Single-pass effect pack (distort, stylize, colour) | 5 | 5 | 25 | EffectCraft math, ISF, three.js, glfx.js, GPUImage |
| 2 | Video layers: speed, time remap, loop, reverse, freeze, frame mix, per-layer `fps` | 5 | 3 | 15 | ours (ffmpeg); EffectCraft time logic |
| 3 | Extra blend modes | 3 | 5 | 15 | EffectCraft `blend.rs`, GPUImage |
| 4 | Light and glow pack: bloom, light rays, lens flare, outline (JFA), drop shadow, film burn, light leak, film damage | 4 | 3 | 12 | Bevy, EffectCraft, pmndrs |
| 5 | Text layers plus per-character animators | 5 | 2 | 10 | parley + EffectCraft `selectors.rs` + Skottie (reference) |
| 6 | Masks and track mattes | 4 | 3 | 12 | ours (SDF); EffectCraft semantics |
| 7 | Echo or trails, time displacement | 4 | 3 | 12 | EffectCraft `time_fx.rs` |
| 8 | Precomps | 3 | 3 | 9 | ours |
| 9 | Particles (sparkles, sakura, dust, speed streaks, bursts on beats) | 3 | 3 | 9 | ours; EffectCraft `weather.rs` and curl noise |
| 10 | Optical-flow slow motion (twixtor-like) | 4 | 2 | 8 | EffectCraft `flow.rs`, `ftm_motion` |
| 11 | Shape layers with path operators | 3 | 2 | 6 | EffectCraft `path/ops.rs`, Skottie |
| 12 | Audio clips / SFX on hits | 2 | 4 | 8 | ours (ffmpeg filter graph) |
| 13 | Lottie overlay layer | 2 | 3 | 6 | rasterlottie or velato |
| — | Skip: 3D lights and models, tracking, segmentation, expressions, keying, OCIO, PSD/AEP/PDF, pure-Rust codecs, audio effects | | | | |

---

## 5. Model additions in our conventions

Conventions apply throughout: times are in project units (beats or seconds); space is in composition-height units with
the origin at the centre; sizes are % of height; colours are linear RGB; angles are degrees. Every number can be
animated (`Prop`).

### 5.1 New effect fields (WP2)

- `Effect.options: BTreeMap<String, String>`: non-animatable choices, for example `{ "shape": "sine" }` or
  `{ "mode": "cmyk" }`. The registry lists the allowed values per option, and `check` validates them.
- `Effect.source` (already exists) is reused for a `.cube` file (`lut`) and a slot or layer id (`displacement_map`):
  `"source": "slot:noise"` or `"source": "layer:title"`.
- Registry `ParamSpec` gains `unit` (`"%h"`, `"deg"`, `"pos"`, `"color"`, `"x"`, `"time"`) and `range`. The
  registry is split per category: `effects/{color,blur,distort,stylize,light,time,generate}.rs`, so packages append
  to different files.

### 5.2 Layers

```json
{ "id": "clip1", "kind": "video", "slot": "op_clip",
  "speed": { "keys": [ { "t": 8, "v": 1, "ease": "in_out" }, { "t": 9, "v": 0.2 } ] },
  "time_remap": null, "loop": false, "reverse": false,
  "frame_blend": "mix", "fps": null, "audio_gain": 0 }
```

- `Slot` gains `trim: [in_s, out_s]` (seconds of the source; content-specific, so it swaps with the slot). A slot
  whose `path` is a video, GIF, WebP or image-sequence pattern can feed a `video` layer.
- `video`: `speed` (multiplier, animatable, integrated over layer time), or `time_remap` (source seconds as a function
  of layer time; overrides `speed`), `loop`, `reverse`, `frame_blend` = `none | mix | flow`, `audio_gain` (mixed by the
  encoder).
- `fps` on **every** layer (posterize time): the layer's transform, effects and source are evaluated at
  `floor(t_s × fps) / fps`. This gives the choppy anime look and stutter; it is a cheap core-only change.
- `text`:
  ```json
  { "kind": "text", "text": "SAY MY NAME", "font": "fonts/Anton.ttf",
    "size": 12, "color": [1,1,1], "align": "center", "tracking": 0, "line_height": 1.1,
    "stroke": { "color": [0,0,0], "width": 0.8 },
    "animators": [ { "based_on": "chars",
        "range": { "start": 0, "end": { "keys": [ … ] }, "offset": 0, "shape": "ramp_up",
                   "ease_high": 0, "ease_low": 0, "random_order": false, "seed": 1, "smoothness": 100 },
        "wiggly": null,
        "position": [0, 0.05, 0], "scale": 1.5, "rotation": [0, 0, 20], "opacity": 0, "blur": 0,
        "fill": null, "tracking": 0 } ] }
  ```
  `size`, stroke `width` and `tracking` are % of height. Animator `range.start`, `end` and `offset` are percent of
  units (0–100, as AE). Animator property values are offsets applied in proportion to the selection.
- `comp` (precomp): `"comp": "intro"` refers to `project.comps.intro = { "width"?, "height"?, "duration", "background"?, "layers": [...] }`,
  with `speed`, `time_remap`, `fps` and `loop` as for video. A precomp's `in` and `out` are in its own time units.
- `particles`: `emitter { shape: point|line|box|ring|layer_edge, position, size }`, `rate` (per time unit),
  `bursts: [{ t, count }]` (beat hits), `life`, `speed`, `direction`, `spread` (deg), `gravity` (height units / s²),
  `drag`, `turbulence { amount, scale }`, `size` (% h) with `size_end`, `opacity_end`, `rotation_speed`,
  `color` / `color_end`, `sprite` (`dot | spark | star | slot:<id>`), `seed`, `max` (cap).
- `shape`: `items: [ { "rect": { size, roundness } } | { "ellipse": { size } } | { "star": { points, inner, outer, roundness } } | { "path": { points: [[x, y, in_x, in_y, out_x, out_y], …], closed } } | { "fill": { color, opacity } } | { "stroke": { color, width, cap, join, dash } } | { "gradient": { … } } | { "trim": { start, end, offset } } | { "repeater": { copies, offset, transform } } | { "round_corners": { radius } } | { "offset_path": { amount } } | { "zigzag": { size, ridges } } | { "wiggle_path": { size, detail, speed, seed } } ]`.
- Every layer gains `masks: [ { "shape": "rect" | "ellipse" | "polygon" | "path", "size"?, "points"?, "position", "rotation", "roundness", "feather" (% h), "expansion" (% h), "opacity", "mode": "add | subtract | intersect | difference", "invert" } ]`
  in layer-box space (unit = composition height, so a mask moves with the layer). Every layer also gains
  `matte: { "layer": "<id>", "mode": "alpha | alpha_inverted | luma | luma_inverted" }`. The matte layer still draws
  unless it sets `"visible": false`, a new flag meaning "render only as a matte".
- `Blend` adds `soft_light, hard_light, color_dodge, color_burn, linear_burn, linear_dodge, linear_light, vivid_light,
  pin_light, hard_mix, exclusion, subtract, divide, hue, saturation, color, luminosity, lighter_color, darker_color`.
- `audio_clips: [ { "path", "at", "in", "gain", "fade_in", "fade_out" } ]` at project level.

### 5.3 Effects to add (effect by effect)

Type: P1 = one fragment pass in the builtin `fn effect(uv)` form (WP2); MP = multi-pass `Step` in `fx.rs`;
T = renders the layer at other times (needs WP1).

| Kind | Parameters (default) | Type | Port source | Licence | Effort |
| --- | --- | --- | --- | --- | --- |
| `twirl` | `angle` (90), `radius` (30 %h), `center` ([0,0]) | P1 | EffectCraft `distort.rs::twirl` / `dst_twirl` | MIT/Apache | S |
| `bulge` | `amount` (0.5; negative pinches), `radius` (25 %h), `center`, `taper` (0) | P1 | EffectCraft `bulge`; glfx `bulgepinch.js` | MIT | S |
| `wave_warp` | `amplitude` (2 %h), `wavelength` (10 %h), `direction` (90), `speed` (cycles per time unit, 0), `phase` (0); options `shape` = sine…smooth_noise, `pin` | P1 | EffectCraft `wave_shape`, `wave_pin`, `wave_warp` | MIT/Apache | S |
| `ripple` | `amplitude` (1 %h), `wavelength` (5 %h), `radius` (50 %h), `center`, `phase`, `speed` | P1 | EffectCraft `ripple` | MIT/Apache | S |
| `shockwave` | `progress` (0..1, key it on a hit), `center`, `radius` (60 %h), `width` (5 %h), `strength` (3 %h) | P1 | pmndrs `ShockWaveEffect`; ISF Shockwave Pulse | zlib / MIT | S |
| `kaleidoscope` | `segments` (6), `center`, `rotation`, `zoom` (1); option `mirror` | P1 | EffectCraft `kaleida`; three `KaleidoShader` | MIT/Apache | S |
| `mirror` | `center`, `angle` | P1 | EffectCraft `mirror` | MIT/Apache | S |
| `tile` | `tiles` ([2,2]), `mirror` (1), `offset` | P1 | EffectCraft `motiontile` | MIT/Apache | S |
| `polar` | `amount` (1); option `direction` = to_polar / to_rect | P1 | EffectCraft `polar` | MIT/Apache | S |
| `lens_distortion` | `amount` (0.2), `center`, `scale` (1) | P1 | Bevy `lens_distortion.wesl` | MIT/Apache | S |
| `pixelate` | `size` (2 %h), `center`; option `shape` = square, hex, triangle, polar | P1 (sample at the cell centre); averaged variant MP | GPUImage Pixellate and PolarPixellate; glfx hexagonalpixelate; EffectCraft mosaic | BSD-3 / MIT | S |
| `halftone` | `size` (1.2 %h), `angle` (45), `smoothing` (0.15), `amount` (1); option `mode` = mono, rgb, cmyk; option `shape` = dot, line, square | P1 | ISF RGB/CMYK Halftone (zoidberg); three `HalftoneShader`; glfx `colorhalftone` | MIT | S |
| `screentone` | `size` (0.6 %h), `levels` (4), `angle`; option `pattern` = bayer, dots, lines | P1 | ISF Dither-Bayer (check origin); GPUImage PolkaDot | MIT / BSD-3 | S |
| `edge_detect` | `strength` (1), `threshold` (0.1), `invert` (0), `color` | P1 (3×3) | three `SobelOperatorShader`; EffectCraft `findedges`; glfx `edgework` | MIT | S |
| `ink` (manga lineart) | `strength`, `threshold`, `paper` ([1,1,1]) | P1 | glfx `ink.js`; GPUImage Sketch / ThresholdSketch | MIT / BSD-3 | S |
| `toon` | `levels` (6), `edge` (0.2) | P1 | GPUImage Toon / SmoothToon | BSD-3 | S |
| `kuwahara` | `radius` (0.5 %h) | P1 (expensive; cap the radius) | GPUImage Kuwahara | BSD-3 | M |
| `emboss` | `angle`, `strength` | P1 | EffectCraft `emboss`; GPUImage | MIT/Apache | S |
| `sharpen` | `amount` (0.5) | P1 | Bevy CAS (`robust_contrast_adaptive_sharpening.wesl`) | MIT/Apache | S |
| `chromatic_aberration` (radial) | `amount` (1 %h at the corners), `center`, `samples` (8) | P1 | Bevy `chromatic_aberration.wesl` (INSIDE technique, spectral LUT) | MIT/Apache | S |
| `glitch` | `amount` (1), `rate` (changes per time unit, 8), `bands` (16), `shift` (5 %h), `rgb` (1 %h), `block` (0), `seed` | P1 | EffectCraft `vr_glitch` (bands); three `DigitalGlitch`; pmndrs `GlitchEffect`; ISF Glitch Shifter / JPEG Block Corruption | MIT/Apache, MIT, zlib | M |
| `scanlines` | `count` (300 per frame height), `amount` (0.3), `speed` | P1 | three `FilmShader`; pmndrs `ScanlineEffect` | MIT / zlib | S |
| `vhs` | `noise`, `roll`, `bleed` (%h), `wobble` | P1 | ISF Bad TV (VIDVOX) | MIT | S |
| `film_damage` | `scratches` (0.3), `dust` (0.3), `flicker` (0.1), `jitter` (0.2 %h), `seed` | P1 | write ourselves (hashes per frame) | ours | S |
| `film_burn` / `light_leak` | `amount`, `color`, `speed`, `seed` | P1 | EffectCraft `ccburnfilm` (`fx_pixel2.wgsl`); fbm | MIT/Apache | S |
| `lut` | `amount` (1); `source` = `.cube` path | P1 + 3D texture | EffectCraft `parse_cube`; pmndrs `LUT3DEffect` | MIT/Apache, zlib | M |
| `gradient_map` (duotone and tritone) | `shadows`, `midtones`, `highlights`, `amount` | P1 (colour op: fuse into `compose.rs`) | EffectCraft Tritone; ISF Duotone | MIT/Apache | S |
| `levels` | `in_black`, `in_white`, `gamma`, `out_black`, `out_white` | colour op | EffectCraft Levels | MIT/Apache | S |
| `vibrance`, `channel_mixer`, `black_white` | … | colour op | glfx vibrance; EffectCraft | MIT | S |
| `displacement_map` | `amount` ([3, 3] %h), `source` = slot or layer; options `channel_x`, `channel_y` | P1 + aux texture | EffectCraft `dst_displace` | MIT/Apache | M |
| `turbulent_displace` | `amount` (2 %h), `size` (10 %h), `complexity` (3), `evolution` (deg), `seed` | P1 | EffectCraft `dst_turbulent` (fbm) | MIT/Apache | S |
| `fractal_noise` (generator) | `scale` (20 %h), `complexity`, `evolution`, `contrast`, `brightness`; option `blend` | P1 | EffectCraft `fractal_gpu`; stegu webgl-noise (MIT, licence not checked here) | MIT/Apache | S |
| `gradient_ramp` | `start`, `end`, `color_a`, `color_b`; option `shape` = linear, radial | P1 | EffectCraft generate | MIT/Apache | S |
| `bloom` (deep glow; keep `glow`) | `threshold` (0.8), `knee` (0.2), `intensity` (1), `radius` (0..1 → mip count), `color` | MP (mip chain) | Bevy `bloom.wesl` (COD dual filter, Karis average, soft threshold) | MIT/Apache | M |
| `light_rays` | `center`, `length` (0.5), `intensity` (1), `threshold` (0.6), `color`; option `mode` = add, screen, lighten | MP (bright pass + our zoom streak + add) | EffectCraft `light_rays` / `flt_rays`; pmndrs `GodRaysEffect` | MIT/Apache, zlib | M |
| `lens_flare` | `position`, `brightness` (1), `size` (1); option `lens` = 0, 1, 2 | P1 (analytic ghosts) | EffectCraft `lens_flare` + `flare_ghosts`; ISF Lens Flare | MIT/Apache | M |
| `outline` (stroke around alpha) | `width` (0.6 %h), `color`, `softness` (0), `opacity` (1); option `position` = outside, inside, center | MP (jump flood log2(w) passes) | write ourselves; GPUImage JFAVoronoi as reference | BSD-3 ref | M |
| `drop_shadow` | `offset` (1 %h), `angle` (135), `softness` (1 %h), `color` ([0,0,0]), `opacity` (0.6) | MP (reuse gaussian) | EffectCraft drop shadow | MIT/Apache | S |
| `tilt_shift` | `center`, `width` (20 %h), `blur` (1.5 %h), `angle` | MP (two gaussians + mix) | glfx `tiltshift.js`; pmndrs `TiltShiftEffect` | MIT / zlib | S |
| `echo` | `count` (3), `interval` (−0.125 time units), `decay` (0.6), `start` (1); option `mode` = add, screen, max, min, behind, front | T | EffectCraft `time_fx.rs::echo` | MIT/Apache | M |
| `time_displacement` (slit scan) | `max` (−0.25 time units), `slices` (16); option `axis`; `source` = slot or layer (gradient map) | T (N re-renders) | EffectCraft `time_displacement`; ISF Slit Scan | MIT/Apache | M |

---

## 6. Work packages

Each package is sized for one agent in one session, and ordered so that packages running at the same time touch
different files. Shared files that several packages append to, and how to merge them:

- `edits-core/src/effects/*.rs`: append rows to the per-category file.
- `docs/EFFECTS.md`: append a section.
- `edits-core/src/describe.rs`: one match arm per new layer kind.
- `.claude/skills/edits/SKILL.md`: one paragraph.

**Phase 0: wait for the other agent.** It is changing `model.rs` (beat-time syntax), `effects.rs`, `fx.rs` and the
shaders (transitions), `renderer.rs::compile_custom` (shader validation), presets and the analysis crate. Start
WP1 only after that lands, and rebase on it.

| WP | Title | Depends on | Crates and files touched | Port from | Tests to add | Effort |
| --- | --- | --- | --- | --- | --- | --- |
| **WP1** | Renderer restructure: layer content as a function | phase 0 | `edits-render/src/renderer.rs` (split `render` into `render_stack(layers, t, w, h) -> Tex` and `layer_content(layer, t) -> Option<Tex>`; make `signature` time-aware per layer; offscreen per-layer textures from the pool), `fx.rs` (signatures only) | — | Existing `tests/render.rs` and `tests/effects.rs` must pass byte-identical; add `render_stack_matches_frame` | M |
| **WP2** | Effect plumbing + first six effects | WP1 | core: split `effects.rs` → `effects/{mod,color,blur,distort,stylize,light,time,generate}.rs`, add `unit`, `range`, `options`; `model.rs` `Effect.options`; `check.rs` (options, units, `source` kinds); render: new `fxlib/{mod,distort,stylize,color}.rs` (kind → WGSL file + uniform packer), `shaders/fx/prelude.wgsl` (shared with `custom.wgsl`: 16 params, `src`, `src_mirror`, `aux`, `lut3d`, helpers `to_uv` / `from_comp`, `hash`, `noise`), `fx.rs` `Step::Builtin`, `renderer.rs` pipeline cache + 3D LUT upload; docs `EFFECTS.md`, `FORMAT.md` | EffectCraft twirl, bulge, kaleida; ISF halftone; Bevy chromatic aberration; GPUImage pixellate | `tests/effects.rs`: twirl leaves the outside unchanged and maps the centre to itself; kaleidoscope is symmetric; pixelate cells are uniform; halftone mean luminance stays within ±5 % of the input; chromatic aberration leaves the centre unchanged and splits colours at the corners. Core: unknown option, bad unit, missing `source` file | M–L |
| **WP3** | Video layers | WP1 (runs alongside WP4/WP5) | core `model.rs` (`LayerKind::Video`, `Slot.trim`, `Layer.fps`), `eval.rs` (`source_time`: speed integration, remap, loop, reverse, posterize), `check.rs`; render: new `video.rs` (ffprobe, decoder processes, LRU, prefetch), `renderer.rs` (prepare, layer content, signature includes the source frame), `passes.wgsl` (mix blend); cli: `encode.rs` (clip audio via `amix`), `CLI.md`; docs `FORMAT.md`, `RENDERING.md` | EffectCraft `frame_position`, `timewarp_integrate_speed` (logic) | Generate a 48-frame clip in the test by encoding frames whose colour encodes the index (ffv1 or x264 `-qp 0`; skip without ffmpeg). Check frame n at speed 1, 2 and 0.5; a `time_remap` key; `reverse`; `loop`; `fps` 12 holds frames; `mix` at a half frame averages; a seek backwards restarts correctly. Core: speed integration against the analytic value; `fps` posterize | L |
| **WP4** | Effect pack 1: distort, stylize, colour (P1 only) | WP2 | `shaders/fx/*.wgsl` (new files), `fxlib/{distort,stylize,color}.rs`, `effects/{distort,stylize,color}.rs`, new `tests/fx_distort.rs`, `tests/fx_stylize.rs`, `EFFECTS.md` | §5.3 rows marked P1 (wave_warp, ripple, shockwave, mirror, tile, polar, lens_distortion, screentone, edge_detect, ink, toon, kuwahara, emboss, sharpen, glitch, scanlines, vhs, film_damage, film_burn, lut, gradient_map, levels, vibrance, displacement_map (slot source only), turbulent_displace, fractal_noise, gradient_ramp) | One test per effect: identity at zero strength (skipped); a known pixel moves where the formula says; deterministic per frame; changes with `seed`; LUT identity `.cube` = no-op and invert `.cube` = `invert` | L |
| **WP5** | Light pack, multi-pass, and blend modes | WP2 (alongside WP3/WP4) | `fx.rs` (new `Step`s: Bloom, Jfa, Shadow, TiltShift, Rays), `shaders/passes.wgsl` (new entry points), `compose.rs` (blend modes), `model.rs` `Blend` enum, `effects/light.rs`, `tests/fx_light.rs`, `tests/render.rs` (blends), `RENDERING.md`, `EFFECTS.md` | Bevy `bloom.wesl`; EffectCraft `light_rays`, `lens_flare`, `flare_ghosts`, drop shadow, `color/blend.rs`; GPUImage blends; glfx tilt shift | Bloom: below threshold → unchanged, energy grows with intensity, same at `--scale 0.5` within tolerance. Outline width in pixels = width % × height ± 1. Each blend mode against a CPU formula on a 2×2 grid of values, including values above 1 | L |
| **WP6** | Text layers (static) | WP1 (after WP3 merges, because of `renderer.rs` and `model.rs`) | render `Cargo.toml` (+ parley, skrifa, tiny-skia or vello_cpu), new `text.rs` (layout, glyph atlas cache keyed by font, glyph and size bucket; quads), `renderer.rs` (layer content for text), core `model.rs` (`LayerKind::Text`), `check.rs` (font exists, size > 0), `describe.rs`; docs | parley (dependency); EffectCraft `CharXf` field list | Do not commit new fonts. Use `crates/edits-gui/assets/fonts/InterVariable.ttf` (OFL, already in the repository) by path from the test. Also ship it as the default `font` when none is given (move it to a shared `assets/fonts/` with its OFL text). Check: centring and alignment of a known string; stroke widens the coverage; the text scales with `--scale`; a missing font file is a `check` error | L |
| **WP7** | Text animators | WP6 | core new `text_select.rs` (port of EffectCraft `selectors.rs`, with its tests), `eval.rs` (per-character transforms), `model.rs` (`animators`), render `text.rs` (per-glyph model matrices, opacity, fill, blur), docs, skill presets ("typewriter", "pop in", "random flicker", "wave") | EffectCraft `selectors.rs`; Skottie `RangeSelector.cpp` (reference) | Core: range ramp values, random order is a permutation and seed-stable, wiggly is bounded. Render: opacity animator start 0 → end 50 % leaves the first half visible; per-character Y rotation shows a 3D foreshortened glyph | M |
| **WP8** | Masks and track mattes | WP1 (after WP7, since both touch `model.rs` and `renderer.rs`) | core `model.rs` (`Layer.masks`, `Layer.matte`, `Layer.visible`), `eval.rs`, `check.rs` (matte cycles, unknown ids, polygon of fewer than 3 points); render `passes.wgsl` (`fs_geo` mask SDF uniforms or a mask pass), `renderer.rs` (matte source offscreen + multiply), `geometry.rs` | ours; EffectCraft mask modes and `MatteKind` semantics | Rect mask: outside transparent; feather gives a monotonic ramp of the right width; subtract, intersect and invert; alpha, alpha_inverted, luma and luma_inverted mattes against hand-computed values; a matte source with `visible: false` does not draw | M–L |
| **WP9** | Precomps | WP8 | core `model.rs` (`Project.comps`, `LayerKind::Comp`), `check.rs` (cycles, unknown comp), `eval.rs` (time mapping shared with video), render `renderer.rs` (recursion through `render_stack`), `describe.rs`, docs; MCP: `project_edit` paths still work (JSON pointer) | — | A comp layer equals the same layers inline (byte-equal at scale 1); time offset, speed and loop; a cycle reports an error in `check` | M |
| **WP10** | Time effects: echo, time displacement | WP1, WP2 (alongside WP9 if `renderer.rs` edits are coordinated; otherwise after it) | `fx.rs` (`Step::Temporal` that calls `layer_content(layer, t_i)`), `effects/time.rs`, `renderer.rs` (temporal step on layers and adjustment layers: an adjustment echo re-renders the stack below), `tests/fx_time.rs` | EffectCraft `echo` operators, `time_displacement` | A moving square with 2 echoes shows 3 squares with intensities 1, d, d²; mode `max`; adjustment echo; cost of `count` 0 is zero | M |
| **WP11** | Particles | WP1 (any time after WP8) | core `model.rs` (`LayerKind::Particles`), `check.rs`; render new `particles.rs` (closed-form state per particle from seed and birth time; instance buffer; one draw), `shaders/particles.wgsl` (sprites dot, spark (streak along velocity), star; curl noise), docs, presets (sakura, sparkles, dust, beat burst) | EffectCraft `sim/weather.rs` (closed-form idea), `fx_particles.wgsl` curl noise | Deterministic per frame; count of live particles at t equals rate × life (± the boundary); a burst at beat b appears at b and not at b − 1 frame; gravity moves the centroid down by ½ g t² | M–L |
| **WP12** | Optical-flow frame blending | WP3 | render new `flow.rs` (port of EffectCraft `raster/flow.rs`), `video.rs` (`frame_blend: flow`, cache vectors per frame pair), `passes.wgsl` (`fs_flow_warp`, port of `ftm_motion`) | EffectCraft `flow.rs`, `fx_time.wgsl::ftm_motion` | A translating textured image: interpolation at w = 0.5 matches the half-way shift within 1 px RMS; `mix` vs `flow` sharpness | M–L |
| **WP13** | Shape layers | WP8 | render `Cargo.toml` (+ kurbo, tiny-skia, linesweeper), new `shape/{mod,ops,offset}.rs` (port EffectCraft `path/ops.rs`, `offset.rs`), raster to a texture per frame (CPU tiny-skia; cached when static), core `model.rs` (`LayerKind::Shape`), `check.rs`, docs, presets (speed lines, burst lines, ring pulse) | EffectCraft `path/src/ops.rs`, `offset.rs`, `lib.rs::{rect, ellipse, polystar}`; Skottie shape-layer operators (reference) | Trim 0–50 % halves the stroke length (coverage); repeater count; round corners keeps the area within tolerance; static shapes rasterise once (stats) | L |
| **WP14** | Audio clips / SFX | phase 0 (independent; only touches cli) | core `model.rs` (`audio_clips`), `check.rs`; cli `encode.rs` (`-filter_complex` `adelay`, `volume`, `afade`, `amix`), `CLI.md`; gui (optional preview) | — | `arguments()` unit tests as in the existing `encode.rs` tests; a render with two clips has audio of the right length (skip without ffmpeg) | S–M |
| **WP15** | Lottie overlay layer (optional) | WP8 | render `Cargo.toml` (+ rasterlottie or velato with vello_cpu), `lottie.rs` (frame → texture), core `LayerKind::Lottie { path, speed, loop }` | rasterlottie / velato | A fixture generated in code (a tiny JSON written by the test), not a committed file | M |

Suggested order and parallelism:

1. **Phase 0.** Wait for the other agent.
2. **WP1.** Solo, because it touches `renderer.rs` and `fx.rs`.
3. **WP2.** Solo. **WP14** can run alongside it (cli only).
4. **WP3 ‖ WP4 ‖ WP5.**
   - WP3 owns `renderer.rs`, `model.rs`, `eval.rs` and `video.rs`.
   - WP4 owns `shaders/fx/*`, `fxlib/*`, `effects/{distort,stylize,color}.rs` and the new test files.
   - WP5 owns `fx.rs`, `compose.rs`, `passes.wgsl`, `effects/light.rs` and the `Blend` enum. The `Blend` enum is the
     one `model.rs` hunk that collides with WP3; it merges trivially.
5. **WP6 → WP7.** Text.
6. **WP8 → WP9.** Masks and mattes, then precomps.
7. **WP10, WP11, WP12, WP13, WP15.** Each one after the package it depends on; two can run together if one of them
   stays out of `renderer.rs` (WP12 and WP13 mostly add new files).

Every package also:

- Updates `docs/FORMAT.md` (or `EFFECTS.md`), `edits-core/src/describe.rs` and `.claude/skills/edits/SKILL.md`.
- Keeps all spatial parameters in % of height and positions in composition units.
- Adds `THIRD_PARTY.md` entries and file-header attributions for ported code.
- Generates all test media in code. No fonts, clips or LUTs are committed beyond what the repository already has.
- Runs `cargo test -p edits-core -p edits-render -p edits-cli` and clippy.

Total: about 13–15 agent sessions for WP1–WP13, with WP4 and WP5 the largest in effect count.
