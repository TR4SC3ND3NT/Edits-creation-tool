<!-- Research note, 2026-11-14. App inventories come from the sources listed below; licences of port sources were read from the repositories' licence files and per-file headers in this session. Re-check a file's header before porting it. -->
# Effect catalog: the edit apps vs `edits`

Every effect, transition, animation preset and blend mode found in the apps
people use for anime and manga edits, what `edits` has for each, and, for
what is missing, a permissive source to port. The implementation plan
turns the gaps into waves of work, one agent session each. The last
section, [Premium and plugin effects](#premium-and-plugin-effects), adds the
paid tiers of the apps and the After Effects plug-ins, with permissive
code and models to port and its own ranked waves.

## How to read this page

**Apps.** VS = Video Star (iOS), AM = Alight Motion, CC = CapCut, NV = Node
Video, AE = After Effects built-ins, PL = AE plug-ins editors use (Sapphire,
Deep Glow, Twixtor, RSMB, Saber, Optical Flares, Trapcode, Pixel Sorter,
Quick Chromatic Aberration), VN = VN Video Editor, IS = InShot. A row merges
the same effect across apps; the app's own name is given when it differs.

Where the lists come from:

- Alight Motion: the official effect guide, https://guide.alightmotion.com/effects/
  (169 effects, alphabetical, with tags; detail pages such as `/effects/block-noise`).
- Video Star: the official pack list "Where are my effects?",
  http://videostarapp.com/wheres-my-effects/index.html (every Power Pack and
  its effects), plus the App Store listing (Pro, Multi-Layer, Motion Madness,
  Cinema Pro, Velocity Mapper, VS Motion Blur, VS Motion Morph).
- After Effects: the effect list mirrored by EffectCraft's generated
  `docs/effects.md` (308 effects, AE names and categories),
  https://github.com/storytold/effectcraft/blob/main/docs/effects.md.
- Node Video: its store description ("Currently included effects") and
  release notes (Page Turn, Fracture, Posterize Time, Tritone, 2-Strip,
  Minimax, Long Shadow, 8Bit, Saber, Light Streak, Depth Generator...).
- CapCut, VN, InShot: category names from CapCut's own pages (transitions:
  Trending, Basic, Camera, Overlay, Light Effect, Split, Distortion, Glitch,
  Blur, Social Media, Slide, MG, Mask; speed curves Montage, Hero, Bullet,
  Jump Cut, Flash In, Flash Out) and from tutorials. These apps rename and
  rotate their effects with every update, so their rows list the stable,
  widely taught names; there is no official complete list.

**Ours** (status, then our name):

| Status | Meaning |
| --- | --- |
| **have** | `edits` already does it: the effect, layer field, transition or preset named. |
| **preset** | Buildable today from existing effects, layers and keys; the row says how. A `presets/*.json` file makes it one command. |
| **shader** | Missing; a one-pass shader would do it (a `shaders/fx/*.wgsl` file plus a packer in `fxlib/`, or a transition file). |
| **multipass** | Missing; needs several passes (blur pyramids, reductions, jump flood) in `fx.rs` / `passes.wgsl`. |
| **feature** | Missing; needs engine or model work beyond an effect (keyed text sources, mesh layers, audio analysis per frame, tracking, segmentation...). |

**Port source.** Only permissive code: MIT, BSD, zlib, Apache, or files
whose header says so. Links point at the exact file. Short names:

| Short name | Repository | Licence |
| --- | --- | --- |
| EffectCraft | https://github.com/storytold/effectcraft (`crates/effects/src/*.rs`), an AE clone, already our source for twirl, bulge, echo and others | MIT OR Apache-2.0 |
| gl-transitions | https://github.com/gl-transitions/gl-transitions (`transitions/*.glsl`, 233 files; 231 say `License: MIT`, `InvertedPageCurl` and `StereoViewer` say BSD) | MIT per file |
| ISF | https://github.com/Vidvox/ISF-Files (`ISF/*.fs`); repository MIT. Use files credited "by VIDVOX", "Carter Rosenberg" or "zoidberg". **Do not use** `ASCII Art`, `Smoke Screen`, `Smudged Lens`, `Dirty Lens`, `Broken LCD` (Shadertoy code), `Boxinator` (CC BY-NC-SA); check `v002 *` and `Frosted Glass` (third-party credit, no licence stated) first | MIT |
| three.js | https://github.com/mrdoob/three.js `examples/jsm/shaders/` and `examples/jsm/tsl/display/` | MIT |
| glfx.js | https://github.com/evanw/glfx.js `src/filters/` | MIT |
| GPUImage | https://github.com/BradLarson/GPUImage `framework/Source/` and GPUImage3 `Sources/GPUImage/Operations/` | BSD-3-Clause |
| postprocessing | https://github.com/pmndrs/postprocessing `src/effects/glsl/` | zlib |
| Bevy | https://github.com/bevyengine/bevy `crates/bevy_post_process/src/` | MIT OR Apache-2.0 |
| own | No suitable permissive code; the formula is standard or is described here. FFmpeg filters (LGPL/GPL) and Shadertoy (CC BY-NC-SA by default) were used for formulas only. lygia is not permissive (Prosperity licence). | — |

EffectCraft's own notes say its fidelity to AE is mostly unmeasured and its
units are AE pixels; take the math, convert units to % of height, and test.
See [PORTING_PLAN.md](PORTING_PLAN.md) and `THIRD_PARTY.md` for how ported
code is credited.

**Effort**: S is under 2 agent-hours, M is 2 to 5, L is a session or more.

---

## Blur

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Gaussian blur | AM Gaussian Blur; AE Gaussian Blur; NV; CC Blur; VS Pro Blur | **have** `gaussian_blur` | — | — |
| Box / fast box blur | AM Box Blur, Precise Box Blur; AE Fast Box Blur | **preset** `gaussian_blur` (same look at matching sigma) | — | — |
| Directional blur | AM Directional Blur; AE; NV; VS Motion Blur (Cinema Pro) | **have** `directional_blur` | — | — |
| Zoom / radial blur | AM Zoom Blur; AE Radial Blur (zoom), CC Radial Fast Blur; NV Radial Blur; VS Zoom Blur, Zoom Blur 2 | **have** `zoom_blur` | — | — |
| Spin blur | AM Spin Blur; AE Radial Blur (spin); VS Spin Blur | **have** `spin_blur` | — | — |
| Zoom focus / spin focus (clear centre) | VS Zoom Focus, Spin Focus | **preset** `zoom_blur`/`spin_blur` on an adjustment layer with an inverted, feathered ellipse mask | — | S |
| Tilt shift | CC "Miniature"; GPUImage TiltShift | **have** `tilt_shift` | — | — |
| Edge blur (blur grows toward the frame edge) | VS Edge Blur; ISF Edge Blur | **preset** adjustment layer + `gaussian_blur` + inverted feathered ellipse mask; or **shader** for a radial ramp | [ISF Edge Blur.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Edge%20Blur.fs) MIT | S |
| Cross blur | AE CC Cross Blur; NV Cross Blur | **preset** two `directional_blur` (0° and 90°) | — | S |
| Lens blur / bokeh / defocus | AM Lens Blur; AE Camera Lens Blur; NV Camera Lens Blur; VS Lens Blur; CC Defocus; PL S_RackDefocus | **multipass** hexagonal or disc bokeh with highlight boost | [glfx.js blur/lensblur.js](https://github.com/evanw/glfx.js/blob/master/src/filters/blur/lensblur.js) MIT; [postprocessing bokeh.frag](https://github.com/pmndrs/postprocessing/blob/main/src/effects/glsl/bokeh.frag) zlib; Bevy `dof/dof.wesl` MIT/Apache | M |
| Glow blur / ring blur / Maximax hex | VS Glow Blur 1-2, Ring Blur, Maximax Hex, Hex Blur | **preset** `gaussian_blur` + `glow`; hex/ring needs lens blur above | as lens blur | S |
| Inner blur (blur inside, keep the edge) | AM Inner Blur | **shader** option `keep_alpha` on `gaussian_blur` | own | S |
| Mask blur / feather / smooth edges (blur alpha only) | AM Mask Blur, Feather, Smooth Edges | **multipass** alpha-only blur option (reuse the blur pass) | own | S |
| Bilateral / smart / surface blur (skin smoothing) | AE Bilateral Blur, Smart Blur; CC Retouch (smooth) | **shader** | [GPUImage BilateralFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageBilateralFilter.m) BSD; [three.js BilateralBlurNode.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/tsl/display/BilateralBlurNode.js) MIT; [glfx.js adjust/denoise.js](https://github.com/evanw/glfx.js/blob/master/src/filters/adjust/denoise.js) MIT | M |
| Channel blur | AE Channel Blur | **shader** (per-channel sigma on the blur pass) | [EffectCraft blur2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/blur2.rs) | S |
| Compound / variable blur (blur by a map) | AE Compound Blur, Variable Radial Blur, CC Vector Blur | **multipass** (mip chain sampled at a map's level) | [EffectCraft blur2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/blur2.rs), [blur3.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/blur3.rs) | M |
| Unsharp mask | AM Unsharp Mask; AE | **multipass** blur, then `x + k (x - blur)` above a threshold | [glfx.js adjust/unsharpmask.js](https://github.com/evanw/glfx.js/blob/master/src/filters/adjust/unsharpmask.js) MIT; [GPUImage UnsharpMaskFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageUnsharpMaskFilter.m) BSD | S |
| Sharpen | AM Sharpen; AE Sharpen; CC Adjust > Sharpen | **have** `sharpen` | — | — |
| Median / minimax / dust & scratches | AE Median, Minimax, Dust & Scratches; NV Minimax | **shader** (3x3 or 5x5 median; min/max per channel) | [GPUImage MedianFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageMedianFilter.m) BSD; [ISF Dilate.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Dilate.fs), [Erode.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Erode.fs) MIT | S |
| Background blur (blurred copy fills the frame behind) | CC Background > Blur; VS BG Pro Blur | **preset** duplicate image layer scaled to cover + `gaussian_blur` beneath the fitted one | — | S |
| Motion blur from layer motion | AM Motion Blur; VS Motion Blur (VS Motion Blur pack); AE layer motion blur, CC Force Motion Blur | **have** sub-frame motion blur (`settings.motion_blur`) | — | — |
| Motion blur inside footage (RSMB, Pixel Motion Blur) | PL RSMB; AE Pixel Motion Blur; NV Optical Flow X | **feature** blur each video frame along the optical flow we already compute for `frame_blend: flow` | own (`crates/edits-render/src/flow.rs`) | M |

## Colour

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Brightness / contrast | AM; AE; CC Adjust; NV; IS | **have** `brightness_contrast` | — | — |
| Exposure / gamma | AM Exposure / Gamma; AE Exposure; CC Exposure | **have** `exposure`, `levels` (gamma) | — | — |
| Saturation / vibrance | AM Saturation / Vibrance; AE Vibrance; CC Saturation | **have** `saturation`, `vibrance` | — | — |
| Hue shift | AM Hue Shift; AE Hue/Saturation; CC Hue | **have** `hue` | — | — |
| Animated hue cycle / colour shift | VS Color Shift, Multi Hue, Beats Color, Color Music; ISF Multi Hue Shift | **preset** `hue` with a `loop`ed key or hits from `beats_accent` | — | S |
| Colorize / tint | AM Colorize; AE Tint, Hue/Saturation (colorize); VS Tint Up | **have** `tint` | — | — |
| Black & white / monochrome | AE Black & White; VS Black & White, B&W Magic, Mono Magic; CC B&W filter | **have** `saturation` 0 (`bw_hit`); tinted mono **preset** + `tint` | — | — |
| Sepia | CC/IS filters; three.js Sepia | **have** preset `sepia` (the sepia matrix as `channel_mixer`) | — | — |
| Invert | AM; AE; NV | **have** `invert` (`invert_hit`) | — | — |
| X-ray / NeXt-Ray / X-Rageous | VS Chromatic Creations | **have** preset `xray` (`invert` + `saturation` 0 + `tint`) | — | — |
| Threshold | AM; AE | **have** `threshold` | — | — |
| Posterize / colour depth | AM Posterize; AE; postprocessing ColorDepth | **have** `posterize` | — | — |
| Levels | AE Levels; NV | **have** `levels` | — | — |
| Auto levels / auto contrast / equalize | AE Auto Levels, Auto Contrast, Auto Color, Equalize | **multipass** (min/max or histogram reduction, then levels) | [ISF Auto Levels.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Auto%20Levels.fs) MIT; [EffectCraft color2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/color2.rs) | M |
| Curves (RGB, per channel) | AE Curves; NV RGB Curve; CC Curves; VS Pro Color Lab | **have** `curves` (control points per channel, natural spline baked into a table) | [glfx.js adjust/curves.js](https://github.com/evanw/glfx.js/blob/master/src/filters/adjust/curves.js) MIT; [GPUImage ToneCurveFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageToneCurveFilter.m) BSD | — |
| HSL curves / HSL per hue / selective colour | NV HSL Curve; CC HSL; AE Selective Color, Change Color | **have** `hsl` (eight hue bands: hue, saturation, lightness) | [EffectCraft color2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/color2.rs) (Selective Color), [color3.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/color3.rs) (Change Color) | — |
| Colour temperature / white balance / tint | AM Color Temperature; CC Temperature, Tint; NV White Balance; AE Photo Filter | **have** `white_balance` | [GPUImage WhiteBalanceFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageWhiteBalanceFilter.m) BSD; [EffectCraft color_fx.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/color_fx.rs) (Photo Filter) | — |
| Lift / gamma / gain, colour wheels, colour tune | AM Color Tune; NV Color Wheel; CC Color wheels; AE Gamma/Pedestal/Gain, Color Balance, Lumetri | **have** `lift_gamma_gain` | [ISF LGG.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/LGG.fs) MIT; [EffectCraft color_fx.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/color_fx.rs) | — |
| Highlights and shadows | AM Highlights and Shadows; CC Highlight, Shadow; AE Shadow/Highlight | **have** `highlights_shadows` | [GPUImage HighlightShadowFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageHighlightShadowFilter.m) BSD | — |
| Channel mixer / channel remap / shift channels | AM Channel Remap (RGB), Channel Remap (HSV); AE Channel Mixer, Shift Channels, Set Channels; NV Shift Channels | **have** `channel_mixer` (3x4 matrix; option `space` `hsv`) | [GPUImage ColorMatrixFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageColorMatrixFilter.m) BSD; [EffectCraft channel.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/channel.rs) | — |
| Tritone / duotone / gradient map | NV Tritone; AE Tritone; VS Duo Magic; CC duotone filters | **have** `gradient_map` | — | — |
| Hot colour / false colour / thermal | AM Hot Color; ISF Thermal Camera, False Color | **have** `gradient_map` option `mode` `stops` (2 to 16 colours), preset `thermal` | [ISF Thermal Camera.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Thermal%20Camera.fs) MIT; [GPUImage FalseColorFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageFalseColorFilter.m) BSD | — |
| Colorama / spectral map / palette map / colour map | AM Spectral Map, Palette Map; AE Colorama, Palette Map; VS Color Map, Color Map Go | **have** `colorama` (palettes, custom colours, key `phase`) | [EffectCraft color_fx.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/color_fx.rs) (Colorama), [stylize3.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/stylize3.rs) (Palette Map) | — |
| Replace colour / change to colour | AM Replace Color; AE Change to Color, Change Color | **have** `replace_color` | [ISF Color Replacement.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Color%20Replacement.fs) MIT; [EffectCraft color_fx.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/color_fx.rs) | — |
| Spot colour / leave colour / colour pop | AM Spot Color; AE Leave Color; CC "Color pop" | **have** `leave_color` | [EffectCraft color_fx.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/color_fx.rs) (Leave Color) | — |
| Solarize | VS Solarize C, Solarize C Go | **have** `solarize` | [GPUImage SolarizeFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageSolarizeFilter.m) BSD; [ISF Solarize.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Solarize.fs) MIT | — |
| Film looks: bleach bypass, Technicolor, 2-strip | NV Film, 2-Strip Process; VS CinePro Retro | **have** `bleach_bypass`, `technicolor` | [three.js BleachBypassShader.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/shaders/BleachBypassShader.js), [TechnicolorShader.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/shaders/TechnicolorShader.js) MIT | — |
| Filters / LUTs / colour presets | CC Filters; IS Filters; VS Color Magic (Bright, Dark, Warm, Cool, Vivid, Subtle), CinePro Cool; NV | **have** `lut` (`.cube`); a filter pack is **preset**s of `lut` or effect chains | — | S |
| Fade (lifted blacks) | CC Adjust > Fade | **have** preset `fade_film` (`levels` + `saturation`) | — | — |
| Vignette | AM; AE; CC; VS Vignette Pro; NV | **have** `vignette` (`vignette_pulse`) | — | — |
| Grain / noise | AM Noise; AE Add Grain, Noise; CC Grain | **have** `grain` | — | — |
| Haze / dehaze | GPUImage Haze; CC "Clarity" | **have** `haze` | [GPUImage HazeFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageHazeFilter.m) BSD | — |
| Custom colouring by picked colours | VS Custom Coloring | **preset** `gradient_map` | — | — |
| Hair dye (recolour hair) | VS Hair Dye Pack | **feature** needs segmentation (see Keying) | — | L |

## Light and glow

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Glow | AM Glow, Light Glow; AE Glow; NV Glow; CC glow effects | **have** `glow` (`glow_pulse`) | — | — |
| Soft / deep glow (multi-size bloom) | AM Soft Glow; PL Deep Glow, S_Glow, Real Glow | **have** `bloom` | — | — |
| Dark glow (shadows spread and darken) | AM Dark Glow | **shader** option `mode: dark` on `glow`/`bloom` (bloom of the inverted picture, multiplied) | own | S |
| Edge glow / neon edges / neon outline | AM Edge Glow; VS Neon Edge, Color Edge; CC body "Neon"; ISF Neon | **preset** duplicate layer: `edge_detect` + `gradient_map` + `glow`, blend `add` | [ISF Neon.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Neon.fs) MIT for a one-pass version | S |
| Glow scan (glow in a moving luminance band) | AM Glow Scan | **shader** (luminance band mask x glow) | own | S |
| Inner glow | AM Inner Glow; AE layer style | **multipass** distance to the alpha edge (jump flood, as `outline`) | own (reuse `outline`'s JFA) | M |
| Light rays / god rays / radial rays | AM Rays, Radial Rays; AE CC Light Rays; VS Light Rays, Light Rays 2; PL Shine | **have** `light_rays` | — | — |
| Zoom streaks / radial light burst | AM Zoom Streaks; AE CC Radial Light Burst | **have** `light_rays` (bright pass streaked from a point) | — | — |
| Spin streaks | AM Spin Streaks | **preset** duplicate + `threshold` + `spin_blur`, blend `add` | — | S |
| Linear streaks / anamorphic streak / light streak | AM Linear Streaks; NV Light Streak | **preset** duplicate + `threshold` + `directional_blur`, blend `add` | — | S |
| Star glints (Starglow) | PL Trapcode Starglow; CC "Star" sparkle filters | **multipass** bright pass streaked in 4 or 8 directions | own | M |
| Lens flare | AM; AE; NV; PL Optical Flares | **have** `lens_flare` | — | — |
| Light sweep / shine across text | AE CC Light Sweep; CC text "Shine" | **shader** | [EffectCraft generate2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/generate2.rs) (Light Sweep) | S |
| Light leaks (procedural) | VS Light Leaks (Color, Cool, Warm, Particles, Smooth); CC Light leak | **preset** `overlay_texture` with a slot; procedural: **shader** (animated soft colour blobs, screen) | own; ISF `v002 Light Leak.fs` only after its licence is confirmed | M |
| Strobe / flash on beat | AE Strobe Light; VS Strobe, Flash, Zoom Strobe; CC Strobe | **have** `flash`, `flash_burn` via `beats_accent` | — | — |
| Flicker / blink | AM Flicker, Blink; VS Flicker | **preset** `shake` modifier on `opacity` (flicker) or hold keys (blink) | — | S |
| Lightning | AM Lightning; AE Advanced Lightning; CC lightning effects | **have** as transition `lightning_flash`; as an overlay effect **shader** | [EffectCraft generate2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/generate2.rs) (Advanced Lightning); reuse our `LightningFlash.wgsl` bolt code | M |
| Electric edges / electric aura | AM Electric Edges; VS E-Aura, E-Waves | **preset** `edge_detect` + `turbulent_displace` + `glow` on a duplicate | — | S |
| Saber / energy stroke | PL Video Copilot Saber; NV Saber; VS E-Saber | **preset** shape layer stroke + `wiggle_path` + `glow` + `bloom` | — | S |
| Beam | AE Beam | **preset** shape `line` + `trim` + `glow` | — | S |
| Aura around a cut-out | VS Action Auras (Fade, Pulse, Shrink, Throb, Ring, Colors); CC body "Aura" | **preset** `outline` + `bloom` + keyed scale pulse on a duplicate (needs a cut-out picture; on live footage see segmentation) | — | S |
| Drop shadow | AE; AM shadow; text shadows everywhere | **have** `drop_shadow` | — | — |
| Long shadow | AM Long Shadow; NV Long Shadow | **shader** (march along the direction through the alpha) | own | M |
| Radial shadow | AM Radial Shadow; AE Radial Shadow | **shader** (alpha projected from a light point) | [EffectCraft perspective.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/perspective.rs) | S |
| Bevel / smooth bevel | AM Smooth Bevel; AE Bevel Alpha, Bevel Edges | **multipass** distance field (JFA), then lit normals | [EffectCraft perspective.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/perspective.rs) | M |
| Bump map | AM Bump Map | **shader** normals from luminance, lit from a direction | [three.js NormalMapShader.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/shaders/NormalMapShader.js) MIT | S |
| Spotlight | AE Surface Spotlight; VS spotlight looks | **preset** `gradient_ramp` radial, blend `multiply` | — | S |

## Distort

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Transform | AE Transform; AM Transform2 | **have** `transform2d` | — | — |
| Twirl / swirl | AM Swirl; AE Twirl; GPUImage Swirl | **have** `twirl` | — | — |
| Pinch / bulge / spherize | AM Pinch/Bulge, Inner Pinch/Bulge, Spherize; AE Bulge, Spherize; VS Bloat, Bloat Music | **have** `bulge` | — | — |
| Magnify / magnify background / glass magnify | AM Magnify Background; AE Magnify; VS Glass Magnify, Glass Shrink | **shader** (a disc magnified, with a rim) | [EffectCraft distort2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/distort2.rs) (Magnify) | S |
| Kaleidoscope | AM; AE CC Kaleida; VS Krazy Kaleido, Reflected 4, Reflected 6, Kaleidoscope; CC | **have** `kaleidoscope` | — | — |
| Mirror | AM; AE; NV; CC Mirror; VS Vert Mirror, Upside Down | **have** `mirror` | — | — |
| Offset / scroll / tile shift | AM Offset, Tile Shift; AE Offset | **preset** `tile` with `tiles` [1, 1], `mirror` 0 and keyed `offset` | — | S |
| Tiles / motion tile / grid repeat | AM Tiles, Grid Repeat; AE Motion Tile, Edge Tile Repeat; NV Motion Tile; VS Grid, Four More | **have** `tile` | — | — |
| Tile rotate / hexagon tile rotate / hexagon tile shift / hexagon tiling | AM Tile Rotate, Hexagon Tile Rotate, Hexagon Tile Shift, Hexagon Tiling, Hexagon Array | **shader** (each square or hex cell turned or shifted, alternating) | [ISF Kaleidoscope Tile.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Kaleidoscope%20Tile.fs) MIT for the cell lookup; hex lattice as in our `pixelate` | S |
| Polar coordinates | AM; AE; NV | **have** `polar` | — | — |
| Polar displacement map | AM Polar Displacement Map | **shader** option `polar` on `displacement_map` (push along radius and angle) | own | S |
| Displacement map | AM; AE; NV | **have** `displacement_map` | — | — |
| Turbulent displace / fractal warp | AM Turbulent Displace, Fractal Warp; AE; VS Turbulence Lab | **have** `turbulent_displace` | — | — |
| Random displacement / heat haze | AM Random Displacement; PL Heatwave | **preset** `turbulent_displace` with fast `evolution` (masked for haze) | — | S |
| Wave warp | AM; AE; CC "Wave" | **have** `wave_warp` | — | — |
| Ripple / circular ripple / glass ripples | AM Circular Ripple; AE Ripple, CC Ripple Pulse; VS Glass Ripples, Color Ripples | **have** `ripple` | — | — |
| Shockwave | postprocessing ShockWave; VS Shockwaves | **have** `shockwave` | — | — |
| Lens distortion / fisheye / optics compensation | AE Optics Compensation; NV Lens Distortion; CC Fisheye; VS Fisheye World | **have** `lens_distortion` | — | — |
| Bend | AM Bend; AE CC Bend It, Two-Point Bend, Flexible Bend | **shader** | [EffectCraft distort2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/distort2.rs) (Two-Point Bend), [distort3.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/distort3.rs) (Flexible Bend) | S |
| Squeeze / stretch axis | AM Squeeze, Stretch Axis; VS Squish | **preset** keyed non-uniform `scale` | — | S |
| Stretch segment (a strip of pixels pulled out) | AM Stretch Segment; GPUImage StretchDistortion | **shader** (clamp the sample coordinate to a band) | [GPUImage StretchDistortionFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageStretchDistortionFilter.m) BSD; own | S |
| Corner pin / perspective / skew | AE Corner Pin, CC Power Pin; NV Skew; glfx perspective | **preset** 3D `rotation`; exact four corners: **shader** (homography) | [glfx.js warp/perspective.js](https://github.com/evanw/glfx.js/blob/master/src/filters/warp/perspective.js) MIT | S |
| Glass / glass distort / glass relief | AM Glass, Omino Glass; VS Glass Distort, Glass Edge, Glass Move H/V; NV Glass, Liquid Glass | **multipass** refraction by normals from a blurred alpha or luminance | [EffectCraft stylize3.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/stylize3.rs) (Glass Relief); [three.js WaterRefractionShader.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/shaders/WaterRefractionShader.js) MIT | M |
| Droste (picture inside itself, spiralling) | VS Droste | **shader** (log-polar mapping) | own | M |
| Tunnel | AM Tunnel; VS 3D Tunnel 1/2 | **shader** (1/r depth, angle as u) | own; [ISF Hyperspace.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Hyperspace.fs) MIT for the streak variant | S |
| Scatter | AE Scatter | **shader** (sample from a random nearby pixel) | [EffectCraft stylize2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/stylize2.rs) | S |
| Roughen edges | AM Roughen Edges; AE Roughen Edges | **shader** (noise on the alpha edge) | [EffectCraft stylize2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/stylize2.rs) | S |
| 360 viewer / reorient sphere | AM 360º Viewer, 360º Reorient Sphere; AE VR Rotate Sphere | **shader** (equirectangular to rectilinear) | [EffectCraft vr.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/vr.rs) | S |
| Mesh warp / liquify / puppet / bezier warp | AE Mesh Warp, Liquify, Puppet, Bezier Warp; NV Puppet Pin, Mesh Warp | **feature** a deformable grid layer | [EffectCraft distort2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/distort2.rs), [puppet.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/puppet.rs) | L |
| Face distort (big eyes, bloat face, alien) | VS Freaky Face Fun; CC body/face effects | **feature** face landmarks | — | L |

## Stylize

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| RGB split | AM RGB Split; CC Chromatic, "RGB"; VS; PL Quick Chromatic Aberration | **have** `rgb_split` | — | — |
| Chromatic aberration (radial) | AE VR Chromatic Aberrations; PL | **have** `chromatic_aberration` | — | — |
| Digital glitch | CC Glitch; VS Glitch D1-D9; AE VR Digital Glitch; IS Glitch | **have** `glitch`; the nine VS looks are **preset**s of its params | — | S |
| Analog glitch | VS Glitch A1-A9; ISF Glitch Shifter | **have** `vhs` + `glitch`; nine looks as **preset**s | — | S |
| Block noise | AM Block Noise | **shader** (random rectangles multiplied) | own | S |
| JPEG / compression blocks | ISF JPEG Block Corruption; CC "Signal" glitches | **shader** | [ISF JPEG Block Corruption.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/JPEG%20Block%20Corruption.fs) MIT | S |
| Pixel shift / vertical tearing / time glitch RGB | ISF; CC glitch variants | **shader** (one generic "slice shift" with axis) | [ISF Pixel Shifter.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Pixel%20Shifter.fs), [Vertical Tearing.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Vertical%20Tearing.fs) MIT | S |
| Pixel sort | VS Psycho Sorter; PL Pixel Sorter; CC "Pixel sort" | **shader** approximation (smear runs above a threshold), true sort is **multipass** (bitonic per row) | [ISF Sorting Smear.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Sorting%20Smear.fs) MIT | M |
| Datamosh | PL Datamosh; ISF FastMosh | **feature** warp the previous frame by the flow field and skip keyframes (we have `flow.rs`) | own; [gl-transitions StripDatamoshGlitch.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/StripDatamoshGlitch.glsl) MIT for a transition | L |
| VHS / retro DV / bad TV | CC VHS, Retro; IS Retro DV; ISF Bad TV | **have** `vhs` | — | — |
| TV static / snow | IS Noise, snowy TV; CC Static | **preset** `grain` amount 1 + `saturation` 0 (+ `scanlines`) | — | S |
| Scanlines / interlace | CC; ISF Interlace | **have** `scanlines` | — | — |
| CRT (curvature, RGB mask) | CC "CRT"; ISF v002-CRT | **shader** | [three.js tsl/display/CRT.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/tsl/display/CRT.js) MIT | S |
| Old film / old movie | NV Old Movie; CC Old Film; VS Vintage Movies (B&W Film, Silent Era, Industrial, Hand-Cranked) | **have** `film_damage`; the VS looks are **preset**s (`film_damage` + `saturation` 0 + layer `fps` 15 + gate weave) | — | S |
| Film frame / sprocket holes / film strip | VS Sprocket; CC film-frame overlays | **shader** (procedural perforations and frame line) | own; [gl-transitions FilmStrip.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/FilmStrip.glsl) MIT | S |
| Film burn | AE CC Burn Film; VS | **have** `film_burn` | — | — |
| Pixelate / mosaic | AM Pixelate, Mosaic; AE Mosaic; NV; CC Mosaic; IS Style; VS Pixelate | **have** `pixelate` (square, hex, triangle, polar) | — | — |
| Halftone (dots, lines, CMYK) | AM Halftone Dots, Halftone Lines, CMYK Halftone Dots, Dots; VS Halftone Color | **have** `halftone` | — | — |
| Manga screentone / comic | NV Manga; VS Comic, Halftone Sketch; CC Comic, Manga | **have** `screentone`, `ink` | — | — |
| Sketch / pencil | NV Sketch; VS Line Sketch, Mono Sketch, Two Tone Sketch, Color Sketch, Deluxe Sketch; CC Sketch | **preset** `edge_detect` inverted + `levels`; a pencil look: **shader** | [GPUImage SketchFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageSketchFilter.m) BSD; [ISF Sketch.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Sketch.fs) MIT | S |
| Crosshatch | VS Line Sketch variants; GPUImage | **shader** | [GPUImage CrosshatchFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageCrosshatchFilter.m) BSD | S |
| Cartoon / toon | NV Cartoon; AE Cartoon; CC Cartoon | **have** `toon`, `kuwahara` | — | — |
| Oil paint / abstract art / colour paint | VS Color Paint, Abstract Art | **have** `kuwahara` | — | — |
| Brush strokes | AE Brush Strokes | **shader** (oriented strokes sampled along the gradient) | [EffectCraft stylize2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/stylize2.rs) | M |
| Pointillism | VS Pointillism | **shader** (jittered dots coloured from the picture) | own | S |
| Pop art / Euro pop / Warhol grid | VS Pop Art, Euro Pop, Multi Pop Art | **preset** `posterize` + `gradient_map` in a comp, `tile` 2x2; per-tile colours need a **shader** option on `gradient_map` | own | S |
| Find edges | AM Find Edges; AE; NV | **have** `edge_detect` | — | — |
| Emboss / colour emboss | AE Emboss, Color Emboss; NV Emboss | **have** `emboss` | — | — |
| Contour lines / contour gradient / contour strips | AM Contour Lines, Contour Gradient, Contour Strips | **shader** (iso-lines of luminance or alpha; strips lit as paper layers) | own | S |
| Texturize | AE Texturize | **shader** (`emboss` of a slot over the picture) | [EffectCraft stylize2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/stylize2.rs) | S |
| Crystal / prism / fly eye | VS Crystal Dimensions (Crystal 3-8); CC "Prism"; ISF Fly Eye | **shader** (facets that each offset the picture) | [ISF Fly Eye.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Fly%20Eye.fs) MIT | S |
| Voronoi / stained glass cells | AM Voronoi Cells; AE Cell Pattern | **shader** (cells coloured from their seed) | [ISF Worley Cells.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Worley%20Cells.fs) MIT | S |
| Dither / 8-bit / error diffusion | NV Stylize 8Bit; AM Omino Diffusion+; GPUImage CGA colourspace | **have** `screentone` `bayer` for 1-bit; palette dither: **shader** | [ISF Dither-Bayer.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Dither-Bayer.fs) MIT (after glsl-dither, MIT) | S |
| Outline / stroke (sharp, fuzzy) | VS Sublime Outlines (Outline Sharp, Outline Fuzzy); AM Stroke; CC Outline | **have** `outline` (`softness` for fuzzy) | — | — |
| Hollow outline / blob | VS Hollow Sharp, Hollow Fuzz, Blob Sharp, Blob Fuzzy | **shader** option on `outline`: `knockout` (stroke only) and `blob` (fill the expanded shape) | own | S |
| Fill behind / solid fill | AM Fill Behind, Solid Color, Solid Matte; AE Fill | **have** `fade` amount 1 recolours the layer; behind it: a `solid` with an alpha `matte` | — | — |
| Double vision / ghost | VS Ghost, Ghost 3 Go; CC "Double vision"; ISF Double Vision | **preset** duplicate layer offset, opacity 0.5, or `echo` | — | S |
| Split screen / clones | CC Split (2, 3, 4, 6, 9 screens); VS Split Screen Effects (Thirds, Fourths, Fifths, Quarters, Diamond, Ninths, Slice), Triple Shot, Five Shot, Two Shot Go | **have** `tile`, `split_panels`; diamond and slices are **preset**s of masks | — | S |
| Widescreen bars | VS Widescreen B/W; CC "Open", "Close" | **have** `letterbox` | — | — |
| Circle frame | VS Circles B/W, Circle In/Out/Pause, Circles Go | **have** `iris_in` (mask) | — | — |
| Mondrian / multi-panel colour grids | VS Mondrian Attacks (Block Enlarge, Stripe V/H, Grid Magnify, Multi Hue, Multi Tint, Multi Mono) | **preset** comps with rect masks and per-panel colour effects | — | M |
| Kaleido patterns / Action Mosaics | VS Action Mosaics (BG Pattern, Perspective) | **have** `kaleidoscope`, `tile` + 3D `rotation` | — | — |

## Generate and overlay

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Solid colour | AM Solid Color; AE Fill | **have** `solid` layer | — | — |
| Linear / radial gradient | AM Gradient Overlay; AE Gradient Ramp | **have** `gradient_ramp` | — | — |
| Four-colour gradient | AM Four-color Gradient; AE 4-Color Gradient; NV 4 Color Gradient | **shader** (inverse-distance mix of four points) | [EffectCraft generate.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/generate.rs) | S |
| Multi-stop gradient | ISF Multi Gradient | **shader** option on `gradient_ramp` (up to 8 stops) | [ISF Multi Gradient.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Multi%20Gradient.fs) MIT | S |
| Checker / stripes / grid / dots patterns | AM Checker, Stripes, Grid, Dots; AE Checkerboard, Grid; NV Fill Grid, Checkerboard | **shader** one `pattern` effect with a `shape` option | [ISF Checkerboard.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Checkerboard.fs), [Stripes.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Stripes.fs), [Graph Paper.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Graph%20Paper.fs) MIT; [GPUImage PolkaDotFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImagePolkaDotFilter.m) BSD | S |
| Clouds / turbulence / fractal noise | AM Clouds, Turbulence; AE Fractal Noise, Turbulent Noise; NV Fractal Noise | **have** `fractal_noise` | — | — |
| Fractal ridges | AM Fractal Ridges | **shader** option `ridged` on `fractal_noise` | [EffectCraft noise3.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/noise3.rs) | S |
| Iridescence / ribbon | AM Iridescence, Ribbon | **shader** (flowing hue bands from fbm, inside the alpha) | own | S |
| Particles (general) | AE CC Particle World, PL Particular; VS Endless Amazement, Pro Particle Lab, Particles B/W/C; CC "Bling" | **have** `particles` layer | — | — |
| Sakura, sparkles, dust, beat burst | CC Dreamy (Sparkle, Stars); VS E-Glitter | **have** presets `sakura`, `sparkles`, `dust`, `beat_burst` | — | — |
| Snow / rain / bubbles | CC Nature; IS Weather; AE CC Snowfall, CC Rainfall, Bubbles; VS Bubbles B/W | **preset** `particles` (snow: dot + turbulence; rain: spark sprite + gravity; bubbles: ring sprite) | — | S |
| Confetti / hearts / emoji rain | CC Party, Love; VS Hearts, Notes, Sheep, Skulls, Boys, Girls | **preset** `particles` with `slot:` sprites and spin | — | S |
| Starfield / warp stars | AM Starfield, Simple Starfield; AE Starfield; VS Star Field, Star Gate, Star Vortex | **preset** `particles` from the centre, `spark` sprites, growing size | [ISF Hyperspace.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Hyperspace.fs) MIT for a one-pass version | S |
| Bokeh overlay | CC Bokeh | **preset** `particles` large soft dots, low opacity, `screen` | — | S |
| Fire / flames | VS Flame On! (Flames, Hot Stuff, Rings of Fire, Fire Jets); CC fire effects | **shader** (fbm flame rising, gradient coloured) | own | M |
| Smoke / fog | CC Smoke; AE Fog 3D | **preset** animated `fractal_noise` (`screen`) with a gradient mask | — | S |
| Speed lines / focus lines | CC anime effects | **have** `speed_lines`, `burst_lines` | — | — |
| Ring pulse / radio waves | AE Radio Waves; VS Rings, Ring ♩ | **have** `ring_pulse` | — | — |
| Shapes (rect, ellipse, star, polygon, path) | AM shape layers; AE shape layers | **have** `shape` layer | — | — |
| Write-on / stroke draw / drawing progress | AE Write-on, Stroke, Vegas, Scribble; AM Drawing Progress; VS Draw Line | **have** shape `trim` | — | — |
| Stroke taper | AM Stroke Taper | **feature** variable-width stroke on shape layers | own | M |
| Overlay textures (grain, paper, dust) | CC overlays; VS Green Screen Patterns | **have** `overlay_texture` | — | — |
| Audio spectrum / waveform / meters | AE Audio Spectrum, Audio Waveform; VS Audio Meters (M Fuzz, M Bars, M Spikes, M Decay, M Circle), Beats & Freqs (Strip, Box, Side, Center Bars, Center Burst, Freq Out!) | **feature** per-frame spectrum of the song (analysis already decodes it) exposed to a `spectrum` effect and to param links | own | L |
| Counter / timecode / count up-down | AM Count Up/Down, Timecode; AE Numbers, Timecode | **feature** a keyed number as text source | own | S |
| Green-screen backgrounds (photos, patterns) | VS Green Screen Photos, Chaos, Dreams | **have** image and `solid` layers | — | — |
| Caustics, foam, wave world, card dance, fluid | AE Simulation; NV Fluid fill | **feature** (simulations; low value for this genre) | [EffectCraft sim2/](https://github.com/storytold/effectcraft/tree/main/crates/effects/src/sim2) | L |

## Keying and mattes

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Chroma key (green screen) | AM Chroma Key; CC Chroma key; VS Green Screen; NV; AE Keylight, Color Key, Linear Color Key, Color Range | **have** `chroma_key` | [GPUImage ChromaKeyFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageChromaKeyFilter.m), [GPUImage3 ChromaKey.metal](https://github.com/BradLarson/GPUImage3/blob/main/Sources/GPUImage/Operations/ChromaKey.metal) BSD; [ISF Chroma Mask.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Chroma%20Mask.fs) MIT; [EffectCraft keying.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/keying.rs) | — |
| Spill suppression | AE Spill Suppressor, Advanced Spill Suppressor | **have** `spill_suppress` | [EffectCraft keying.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/keying.rs) | — |
| Luma key (black or white out) | AM Luma Key; AE Luma Key, Extract; NV Luma Fade | **have** `luma_key` | [EffectCraft keying.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/keying.rs) | — |
| Unmult (black to transparent, for overlays) | AE Unmult (Red Giant) | **have** `unmult` | [EffectCraft keying2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/keying2.rs) | — |
| Matte choker / simple choker (grow, shrink, soften alpha) | AM Matte Choker; AE Simple Choker, Matte Choker, Refine Soft Matte | **multipass** alpha erode/dilate + blur + threshold | [EffectCraft matte.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/matte.rs); [GPUImage ErosionFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageErosionFilter.m), [DilationFilter](https://github.com/BradLarson/GPUImage/blob/master/framework/Source/GPUImageDilationFilter.m) BSD | S |
| Difference matte | AE Difference Matte | **have** `difference_matte` | [EffectCraft keying2.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/keying2.rs) | — |
| Track matte / clipping mask | NV Clipping Mask; AE track mattes; AM masking; CC overlay mask | **have** `matte` (alpha, luma, inverted) | — | — |
| Shape masks (linear, mirror, circle, rectangle, heart, star) | CC Mask; NV Shape Mask; AM masks | **have** `masks` (rect, ellipse, rounded rect, polygon); heart and star: **preset** a `shape` layer as `matte`; bezier masks: **feature** | own (`edits-core/src/shape.rs` paths) | S |
| Copy background / effects on what is behind | AM Copy Background | **have** `adjustment` layer (with masks) | — | — |
| Remove background / human matting / roto | CC Remove background; NV Human Matting, Smart Matting; AE Roto Brush, Object Matte; VS Hair Dye | **feature** segmentation; keep it outside the renderer: cut out with an external tool, import as a video or image sequence with alpha (`edits` already reads alpha) | — | L |

## Time

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Echo / trails / afterimage | AM Echo Keyframes; AE Echo; VS Clone City (Echo, Faded Echo, Frozen Trail, Faded Trail, Meet Up, Catch Up), Onion Skin; CC "Afterimage"; ISF RGB Trails | **have** `echo` (`echo_hit`) | — | — |
| Long exposure / temporal blend / wide time | AE CC Wide Time, Temporal Blend; ISF Long Exposure | **have** `echo` mode `average` with many copies | — | — |
| Posterize time / time quantization / stutter / 15 fps | AM Time Quantization; AE Posterize Time; NV; VS 15 fps, Hand-Cranked; CC "Stop motion" | **have** layer `fps` | — | — |
| Time displacement / slit scan | AE Time Displacement; VS Time Slicer; ISF Slit Scan | **have** `time_displacement` | — | — |
| Speed / time remap | AE Time Remap; NV Time Remap; VS Change Speed | **have** video `speed`, `time_remap` | — | — |
| Velocity / speed curves | CC Montage, Hero, Bullet, Jump Cut, Flash In, Flash Out; VS Velocity & Time Mapper; NV Speed Ramp | **have** `velocity` preset; the six CapCut curves are **preset**s of `retime` curves | — | S |
| Reverse / boomerang / repeat / sections | VS Motion Madness (Reverse, Forward, Repeat, Fast Section, Slow Section, Reverse Section, Jump, Hesitate); CC Reverse | **have** `reverse`, `loop`, `speed`; boomerang and hesitate are **preset**s (comp with forward + reversed copy, hold keys) | — | S |
| Freeze frame / random freeze | CC Freeze; ISF Freeze Frame, Random Freeze | **preset** `time_remap` hold keys | — | S |
| Optical-flow slow motion | PL Twixtor; VS Motion Morph; CC Smooth slow-mo; NV Optical Flow | **have** `frame_blend: flow` | — | — |
| Frame mix | AE Frame Blending | **have** `frame_blend: mix` | — | — |
| Feedback (previous output, warped) | VS Super Mega Me (Prev Frame, Warp Prev, Decay), Crazy Party (Time Tunnel, Time Decay, Time Tripper); ISF Zooming Feedback | **feature** per-copy transform on `echo` (each echo scaled or turned a little more), which gives a closed-form feedback tunnel without frame state | own | M |
| Time difference / motion map | AE Time Difference; ISF Motion Heat Map | **shader** (difference of the layer and its echo) on the `echo` machinery | own | S |

## Transitions

We have 52 (`edits transitions`), all from gl-transitions; wave 2 added
`zoom_out`, `directional_eased`, `linear_wipe`, `clock_wipe`, `radial_wipe`,
`circle_open`, `heart_wipe`, `star_wipe`, `diamond_wipe`, `random_squares`,
`noise_dissolve`, `luma_wipe` (map from a slot), `window_blinds`,
`window_slices`, `grid_flip`, `inside_cube`, `simple_flip`, `book_flip`,
`fold`, `doorway`, `horizontal_open`, `split_slide`, `ripple`, `water_drop`,
`glitch_bands`, `glitch_flicker`, `vhs_tracking`, `tv_static`,
`stretch_slide` and `squeeze`, so rows below that name them are done. Every missing one
below has a gl-transitions file (MIT unless noted); porting is GLSL to WGSL
into `crates/edits-render/src/shaders/transitions/`, a `TransitionSpec` in
`crates/edits-core/src/effects/mod.rs` and a `Def` in
`crates/edits-render/src/transitions.rs`. Base URL:
`https://github.com/gl-transitions/gl-transitions/blob/master/transitions/`.

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Mix / dissolve / cross fade | CC Mix; VN; IS; AE Cross Dissolve | **have** `fade` | — | — |
| Black fade / white fade / dip to colour | CC Black Fade, White Flash; VS | **have** `fade_color`, `flash_fade` | — | — |
| Flash / overexposure | CC Flash; IS | **have** `overexposure`, `flash_fade` | — | — |
| Lightning | CC Light Effect | **have** `lightning_flash` | — | — |
| Zoom in / pull in / zoom blur | CC Pull In, Zoom In; VS Zoom Blur, Zoom Move; IS Zoom | **have** `cross_zoom`, `simple_zoom`, `zoom_in_out`, `dreamy_zoom` | — | — |
| Zoom out / pull out | CC Pull Out, Zoom Out | **shader** | [SimpleZoomOut.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/SimpleZoomOut.glsl) | S |
| Slide / push (left, right, up, down) | CC Slide; VS Slide Blur; IS Slide | **have** `directional`, `whip_pan` | — | — |
| Slide with eased stop / scaled slide | CC Slide variants | **shader** | [DirectionalEased.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/DirectionalEased.glsl), [DirectionalScaled.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/DirectionalScaled.glsl) | S |
| Whip pan / spin / tilt / swipe / rotate | CC Camera (Rotate, Swipe); VS Zoom Spin | **have** `whip_pan`, `whip_spin`, `whip_tilt`, `rotate_scale_fade` | — | — |
| Shake | CC Shake; IS Shake pack | **have** `camera_shake` | — | — |
| Linear wipe | CC Wipe; AE Linear Wipe; AM Wipe; VS Color Wipe | **shader** | [DirectionalWipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/DirectionalWipe.glsl) | S |
| Radial / clock wipe | AM Radial Wipe; AE Radial Wipe | **shader** | [ClockWipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/ClockWipe.glsl), [RadialWipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/RadialWipe.glsl) | S |
| Iris / circle open / circle close | AE Iris Wipe; CC Circle; VS Circular Wipe, Wipe Circles | **preset** `iris_in`; as a transition **shader** | [CircleOpen.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/CircleOpen.glsl), [CircleIris.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/CircleIris.glsl) | S |
| Heart / star / diamond / shape wipes | CC Mask (Heart, Star); VS Shape Morph | **shader** | [HeartWipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/HeartWipe.glsl), [StarWipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/StarWipe.glsl), [DiamondWipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/DiamondWipe.glsl) | S |
| Block dissolve / random squares | AM Block Dissolve; AE Block Dissolve; VS Transcendent Tiles (□ Fade) | **shader** | [RandomSquares.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/RandomSquares.glsl), [CellDissolve.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/CellDissolve.glsl) | S |
| Noise dissolve | AM Dissolve; AE Dissolve blend | **shader** | [NoiseDissolve.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/NoiseDissolve.glsl), [PerlinDissolve.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/PerlinDissolve.glsl) | S |
| Gradient / luma wipe (through a map) | AE Gradient Wipe, Luminance Image Wipe; VS Color Morph 1/2, Light Leaks Transitions | **shader** (map from a slot) | [LumaWipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/LumaWipe.glsl), [LuminanceMelt.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/LuminanceMelt.glsl) | S |
| Venetian / window blinds / slices | AE Venetian Blinds; VS Bar Swipe; CC Split | **shader** | [WindowBlinds.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/WindowBlinds.glsl), [WindowSlices.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/WindowSlices.glsl) | S |
| Card wipe / grid flip / tiles flip | AE Card Wipe, Grid Wipe; VS Transcendent Tiles (Flip, Shrink) | **shader** | [GridFlip.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/GridFlip.glsl), [ZoomInCircles.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/ZoomInCircles.glsl), [SquaresWipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/SquaresWipe.glsl) | S |
| Cube rotate | CC 3D Cube; VS Cube Rotate, Half Cube Rotate | **have** `cube` | — | — |
| Inside cube | VS 3D Tunnel; CC 3D | **shader** | [InsideCube.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/InsideCube.glsl) | S |
| Flip / back flip / book flip | CC Flip; VS Back Flip, Half Back Flip, Diagonal Flip | **preset** `flip_3d`; as a transition **shader** | [SimpleFlip.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/SimpleFlip.glsl), [BookFlip.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/BookFlip.glsl) | S |
| Page curl / page turn | VS Page Curl; NV 3D Page Turn; AE CC Page Turn; CC page turn | **shader** | [InvertedPageCurl.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/InvertedPageCurl.glsl) (BSD) | M |
| Fold | CC Fold | **shader** | [Fold.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Fold.glsl), [PaperFold.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/PaperFold.glsl) | S |
| Doors / open / close | CC Open, Split; VN | **shader** | [Doorway.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Doorway.glsl), [HorizontalOpen.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/HorizontalOpen.glsl), [VerticalOpen.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/VerticalOpen.glsl), [HorizontalClose.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/HorizontalClose.glsl) | S |
| Split slide | CC Split | **shader** | [SplitSlideInHorizontal.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/SplitSlideInHorizontal.glsl) (and the five siblings) | S |
| Swirl / whirl / vortex | CC Whirl; VS Boogie Twirl | **have** `swirl`; vortex **shader** | [Vortex.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Vortex.glsl) | S |
| Ripple / water drop | CC Ripple; VS Color Ripples | **shader** | [Ripple.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Ripple.glsl), [WaterDrop.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/WaterDrop.glsl) | S |
| Glitch | CC Glitch, Color glitch; VS Glitch D | **have** `glitch_memories`, `glitch_displace` | — | — |
| Glitch bands / flicker / datamosh strip | CC Glitch | **shader** | [GlitchBands.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/GlitchBands.glsl), [GlitchFlicker.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/GlitchFlicker.glsl), [StripDatamoshGlitch.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/StripDatamoshGlitch.glsl) | S |
| VHS tracking / TV static / TV roll / TV off | CC Glitch, Retro | **shader** | [VhsTracking.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/VhsTracking.glsl), [TvStatic.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/TvStatic.glsl), [TvRoll.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/TvRoll.glsl), [TvTurnOff.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/TvTurnOff.glsl) | S |
| RGB split transition | CC Glitch | **preset** `rgb_split` hit across the cut + `fade`; or **shader** | [ColorPhase.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/ColorPhase.glsl) | S |
| Blur transition | CC Blur; VS Motion Blur 1/2 | **preset** `blur_out` + `blur_in` across the cut; or **shader** | [DefocusBlur.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/DefocusBlur.glsl), [TangentMotionBlur.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/TangentMotionBlur.glsl) | S |
| Film burn / light leak / light wipe | CC Light Effect, Overlay; VS Light Leaks Transitions | **have** `film_burn`; light wipe and streaks **shader** | [LightWipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/LightWipe.glsl), [LightStreaks.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/LightStreaks.glsl), [LightRays.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/LightRays.glsl), [LensFlare.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/LensFlare.glsl) | S |
| Stretch / squeeze | CC Distortion (Stretch, Squeeze); VS Boogie Squeeze | **shader** | [StretchSlide.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/StretchSlide.glsl), [Squeeze.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Squeeze.glsl), [PixelStretch.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/PixelStretch.glsl) | S |
| Pixelize | CC; IS | **have** `pixelize` | — | — |
| Mosaic / tiles shuffle | CC MG | **shader** | [Mosaic.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Mosaic.glsl) | S |
| Morph | VS Shape Morph, Color Morph | **shader** | [Morph.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Morph.glsl) | S |
| Shatter / break up / puzzle | VS Break Up (Shatter, Shapes, Paper, Blocks, Bricks, Puzzle); AE Shatter; NV Fracture | **shader** | [Shatter.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Shatter.glsl), [Puzzle.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Puzzle.glsl), [PaperTear.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/PaperTear.glsl), [Reassemble.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Reassemble.glsl) | M |
| Burn through | VS Flame On! | **shader** | [BurnThrough.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/BurnThrough.glsl), [UndulatingBurnOut.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/UndulatingBurnOut.glsl) | S |
| Ink / smoke transition | CC Overlay ("smoke transition") | **shader** | [InkBleed.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/InkBleed.glsl), [SmokeDissolve.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/SmokeDissolve.glsl) | S |
| Comic panels | CC MG | **shader** | [ComicPanels.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/ComicPanels.glsl) | S |
| Halftone transition | CC MG | **shader** | [Halftone.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Halftone.glsl) | S |
| Kaleidoscope transition | VS Kaleidoscope | **shader** | [Kaleidoscope.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Kaleidoscope.glsl), [PowerKaleido.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/PowerKaleido.glsl) | S |
| Magnify / lens push | VS Magnify | **shader** | [LensPush.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/LensPush.glsl), [GravityLens.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/GravityLens.glsl) | S |
| Tunnel / warp speed | CC Camera | **shader** | [Tunnel.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Tunnel.glsl), [WarpSpeed.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/WarpSpeed.glsl) | S |
| Bounce | CC Social Media | **shader** | [Bounce.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Bounce.glsl) | S |
| Swap / card swipe / card deal | CC Social Media, MG | **shader** | [Swap.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Swap.glsl), [CardSwipe.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/CardSwipe.glsl), [CardDeal.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/CardDeal.glsl) | S |
| Cross warp / dreamy / directional warp | VS Trippy Transitions; CC Distortion | **have** `directional_warp`; **shader** | [CrossWarp.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/CrossWarp.glsl), [Dreamy.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/Dreamy.glsl) | S |
| Edge glow transition | CC Light Effect | **shader** | [EdgeGlow.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/EdgeGlow.glsl) | S |
| Droste transition | VS Wild Warp Droste | **shader** (as the Droste effect) | own | M |
| Boogie box transitions (3D boxes) | VS Boogie Box Transitions (Build, Down, Off, Twirl, Away, Magnets, Bounce, Squeeze), More Boogie Box | **preset** `cube` layer + keyed transforms + `cube_turn` | — | M |
| Tile pop / block pop | VS Tile Pop (□ ◇ ▭ ⬡), Power Pop (Block, Triangle, Ball, Star) | **shader** cells of the picture scale or pop out in turn, as an effect and a transition | own; [GridFlip.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/GridFlip.glsl) for cell timing | M |

## Motion and animation presets

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Shake / auto-shake / jitter / earthquake | AM Auto-Shake, Random Jitter, Jitter; CC Shake (effect and In/Out/Combo); VS Earthquake; PL S_Shake; AE `wiggle()` | **have** `shake` preset, `shake` modifier | — | — |
| Swing / rock / pendulum / oscillate | AM Swing, Oscillate; CC Combo (Swing, Rock, Pendulum) | **preset** `loop`ed rotation keys; a sine **feature** `oscillate` modifier makes it one value | own | S |
| Spin | AM Spin; CC Rotate | **have** `spin` | — | — |
| Pulse size / pulse opacity / heartbeat / throb | AM Pulse Size, Pulse Opacity; VS Pulse ♩, Throb ♩; CC "Heartbeat" | **have** `punch` on beats; continuous: **preset** `loop`ed scale keys | — | S |
| Zoom punch on the beat | VS Zoom ♩, In/Out ♩, Zoom Music; CC Combo "Mini zoom" | **have** `punch` | — | — |
| Push in / Ken Burns / zoom out | VS Marvelous Moving Pictures (MP Zoom In/Out), Zoomtastic; CC In "Zoom in/out" | **have** `push_in`, `zoom_out` | — | — |
| Fade in / fade out | AM Fade In/Out; CC In/Out "Fade"; VN; IS | **preset** opacity keys (`fade_in`, `fade_out`) | — | S |
| Slide in / slide out | CC In/Out Slide (left, right, up, down); VS Flying Fotos | **have** `slide_in`, `slide_out` | — | — |
| Bounce in / spring / elastic | CC In "Bounce", "Spring"; VS Bounce | **preset** scale keys with `back_out`/`elastic_out` (`bounce_in`) | — | S |
| Rotate in / swirl in | CC In "Rotate", "Swirl" | **preset** `spin` + scale keys | — | S |
| Wipe in | CC In "Wipe" | **preset** keyed rect mask | — | S |
| Flip in / 3D flip | CC In "Flip"; VS The Third Dimension (3D Spin, 3D Fall) | **have** `flip_3d` | — | — |
| Squish / squeeze / stretch bounce | VS Squish ♩; CC Combo "Squeeze" | **preset** keyed non-uniform scale | — | S |
| Drop / fall in | VS 3D Fall; CC In "Drop" | **preset** position keys + `bounce_out` | — | S |
| Velocity | CC Velocity; VS Velocity Mapper | **have** `velocity` | — | — |
| Beat-synced effects (zoom, bloat, rotate, pixelate, strobe on beats) | VS Music in Motion (Zoom ♩, Bloat ♩, Rotate ♩, Pixelate ♩, Strobe ♩, Flash ♩, Color ♩) | **have** `beats_accent` + presets | — | — |
| Party looks (rotated, small rotate, shrunk, flip flop, juggler, zoom strobe) | VS Crazy Party Effects | **preset**s (hold keys of rotation, scale, mirror on beats) | — | S |
| Flip layer | AM Flip Layer; CC Mirror | **preset** `scale` [-1, 1] | — | — |
| Keyframes with easing graphs | AM; VS Keyframes; CC Keyframes; AE graph editor | **have** keys, named eases, cubic bezier | — | — |
| Move along path | AM Move Along Path; AE motion paths | **feature** `position` follows a shape path, optional auto-orient | own (`edits-core/src/shape.rs`) | M |
| Repeat (linear, radial, grid, scatter, along path) | AM Repeat, Linear Repeat, Radial Repeat, Scatter Repeat, Repeat Along Path, Grid Repeat | **have** shape `repeater` for shapes; for any layer: **feature** a `repeater` on layers (instanced copies with stepped transforms) | own | M |
| Motion tracking / colour tracking | CC Tracking; NV 3D camera tracking; AE Tracker; VS Killer Kinetics, Magic Color Tracker, Zoom (color-tracking) | **feature** tracker writing position keys (offline command) | [EffectCraft camera_tracker.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/camera_tracker.rs) (large) | L |
| Stabilize | CC Stabilize; AE Warp Stabilizer | **feature** | [EffectCraft warp_stab.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/warp_stab.rs) | L |
| Photo parallax / 3D zoom from depth | CC "3D zoom"; NV AI Depth Generator | **feature** depth map (external) + displacement or mesh | — | L |

## Text animations

Our text layers have `range` and `wiggly` selectors that animate `position`,
`scale`, `rotation`, `opacity`, `fill`, `tracking` and `blur` per character,
word or line; presets `typewriter`, `pop_in`, `random_flicker`, `wave`.

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Typewriter | CC "Typewriter"; NV Typewriter; AM Text Progress; VS Draw | **have** `typewriter` | — | — |
| Typewriter cursor | NV Typewriter with cursor | **preset** a blinking text or shape layer keyed along; exact: **feature** `cursor` field | own | S |
| Fade in by letter or word | CC "Fade in"; VS Animated Text Fade, Rand Fade | **preset** `range` selector on `opacity` | — | S |
| Pop / bump / bounce letters | VS Bump Text (Speed, Sine, Bumpy, Pop, Rand Bump), Text Squish (Bounce Up/Down); CC "Pop up" | **have** `pop_in`; the variants are **preset**s | — | S |
| Wave / sine bump | CC "Wave"; VS Sine Bump | **have** `wave` | — | — |
| Flip letters (H, V, both) | VS Animated Text Flip H/V/A, Text Squish Flip Up/Down | **preset** `rotation` [90, 0, 0] / [0, 90, 0] per letter | — | S |
| Slip / slide letters | VS Slip H/V/A, Slide H/V/A; CC "Slide" | **preset** `position` per letter | — | S |
| Zoom letters | VS Animated Text Zoom | **preset** `scale` per letter | — | S |
| Jitter / jello / spiky / shake text | VS Random Text (Jitter, Jello, Spikey); AM Random Jitter | **preset** `wiggly` selector on position/rotation/scale | — | S |
| Flicker | VS Flicker, Flicker 2 | **have** `random_flicker` | — | — |
| Scramble / jumble / randomizer / decode | VS Scramble, Jumble, Jumble 2; AM Text Randomizer; AE Character Offset | **feature** `character_offset` animator property (cycle glyphs) | own; [EffectCraft textfx.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/textfx.rs) | S |
| Explode / implode | VS Explode 1-4, Implode | **preset** `range` (randomized) + `wiggly` position/rotation with large amount | — | S |
| Fall / jump / float | VS Text Squish (Fall, Jump, Float Up/Down) | **preset** | — | S |
| Line reveal / line slide / swing reveal / swing down | VS Title Toolkit, Totally Hyper Titles, World of Wordcraft | **preset** `units` `lines` with rotation/position | — | S |
| Word pop / caption pop | VS Word Pop, Line Pop; CC caption styles | **preset** `units` `words` + `pop_in` | — | S |
| Tracking / spacing animation | AM Text Spacing | **have** `tracking` property | — | — |
| Per-letter transform | AM Text Transform | **have** animators | — | — |
| Gradient / rainbow text | VS Rainbow, Change Color; CC gradient fill | **preset** `gradient_ramp` layer with an alpha `matte` from the text | — | S |
| Sparkle / glowing titles | VS Sparkle | **preset** `sparkles` matted to the text + `glow` | — | S |
| Title styles (banner, stencil, far shadow, bloat, huge) | VS Big Bossy Bold Titles, Title Toolkit (Dark/Light Shade, Stripe, Ghosted) | **preset** text + `drop_shadow`/long shadow + shape banner behind; stencil = inverted `matte` | — | S |
| Text background box | CC text background | **preset** rounded rect shape behind; **feature** `background` field for an auto-sized box | own | S |
| Multiple strokes | AM multi-border; CC | **preset** stacked text layers with `stroke`; **feature** list of strokes | own | S |
| Write-on handwriting (stroke the glyph outlines) | VS Draw, Erase; CC "Writing"; AE Stroke on text outlines | **feature** glyph outlines as shape paths, then `trim` | own (`font.rs` outlines + shape renderer) | M |
| Text on a path / curved text | AE Path Text; CC Curve | **feature** | [EffectCraft textfx.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/textfx.rs) (Path Text) | M |
| Counting numbers | AM Count Up/Down | **feature** (see Generate) | own | S |
| Auto captions | CC Auto captions | **feature** speech to text, outside the renderer | — | L |
| 3D extruded text | CC 3D text; PL Element 3D | **feature** (see 3D) | — | L |

## Blend modes

We have 27: `normal`, `add` / `linear_dodge`, `screen`, `multiply`,
`overlay`, `lighten`, `darken`, `difference`, `color_burn`, `linear_burn`,
`darker_color`, `color_dodge`, `lighter_color`, `soft_light`, `hard_light`,
`vivid_light`, `linear_light`, `pin_light`, `hard_mix`, `exclusion`,
`subtract`, `divide`, `hue`, `saturation`, `color`, `luminosity`. CapCut's
11 overlay modes, Video Star's Multi-Layer modes and Alight Motion's
blending list are all among these.

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Normal ... luminosity (the 27 above) | AE; AM; NV; CC (11); VS Multi-Layer | **have** | — | — |
| Dissolve / dancing dissolve | AE Dissolve, Dancing Dissolve | **shader** (random pixel coverage by opacity; dancing: reseeded each frame) in `compose.rs` | own | S |
| Classic colour burn / dodge | AE Classic Color Burn, Classic Color Dodge | **preset** `color_burn` / `color_dodge` (differ only above 1) | — | — |
| Stencil / silhouette (alpha, luma): cut every layer below | AE Stencil Alpha, Stencil Luma, Silhouette Alpha, Silhouette Luma; AM mask-type blending | **feature** blend modes that multiply the stack beneath by this layer's alpha or luma; today: a `matte` on each layer or an adjustment comp | [EffectCraft color/src/blend.rs](https://github.com/storytold/effectcraft/blob/main/crates/color/src/blend.rs) | S |
| Alpha add / luminescent premul | AE Alpha Add, Luminescent Premul | **shader** (blend variants in `compose.rs`) | [EffectCraft color/src/blend.rs](https://github.com/storytold/effectcraft/blob/main/crates/color/src/blend.rs) | S |

## 3D

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| 3D layers, rotation X/Y/Z, camera | AE 3D layers and camera; VS 3D Basic; AM 3D transform; AE Basic 3D | **have** 3D `transform`, `camera` | — | — |
| Cube / box / cuboid | AM Cube, Box; VS Cuboid, ☐ Spin, ☐ Expand; CC 3D cube | **have** `cube` layer | — | — |
| Hollow box / open faces | AM Hollow Box; VS ☐ Unfold, ☐ Fold, ☐ Chomp | **preset** `cube` with faces left out; unfolding faces: **feature** per-face transforms | own | M |
| Cylinder / sphere / torus / ellipsoid | AM Cylinder, Ellipsoid, Torus; AE CC Cylinder, CC Sphere, Sphere Wrap, Cylinder Wrap | **feature** a `mesh` layer with procedural primitives textured by a slot (the cube renderer generalised) | [EffectCraft perspective.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/perspective.rs) (Sphere/Cylinder Wrap) | M |
| Prisms and polyhedra (pyramid, octahedron, hexagonal prism, star prism, star polyhedron, three-axis cross, heart, star) | AM Pyramid, Octahedron, Hexagonal Prism, Star Prism, Star Polyhedron, Three-axis Cross, Heart, Star; VS Multi-Dimensional Magic (△ ⬠ ⬡ Expand/Spin/Unfold, Extrude Star, Ring 1-3) | **feature** more primitives for the same `mesh` layer | own | M |
| Raster extrude / 3D extruded picture or text | AM Raster Extrude; CC 3D text; PL Element 3D | **feature** extrude the alpha outline (sides from the jump-flood outline) | own | L |
| Lights and shading on 3D layers | AE lights; AM Raster Extrude lighting | **feature** | — | M |
| Depth of field through the 3D camera | AE camera DOF | **feature** after lens blur (Blur) | Bevy `dof/dof.wesl` | M |
| 3D tunnel of the picture | VS Multi Mashup 3D Tunnel 1/2 | **shader** (see Distort, Tunnel) | own | S |
| Card dance / shatter into pieces in 3D | AE Card Dance, Shatter; NV Fracture Pro | **feature** instanced pieces; the transition-only version is a **shader** (Transitions) | [EffectCraft sim2/pieces.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/sim2/pieces.rs) | L |
| 3D emoji and objects | VS 3D Emoji Pack, 3D Expansion Pack; NV 3D models | **feature** glTF models: skip; use picture slots on cubes or particles | — | L |
| 3D camera tracking | NV 3D camera tracking; AE 3D Camera Tracker | **feature** (see Motion, tracking) | [EffectCraft camera_tracker.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/camera_tracker.rs) | L |

---

## Totals

Rows merge the same effect across apps, so one row is one thing to build.
The counts below are rows, by the first status in the "Ours" column.

| Category | Rows | have | preset | shader | multipass | feature |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Blur | 22 | 7 | 6 | 4 | 4 | 1 |
| Colour | 34 | 14 | 6 | 12 | 1 | 1 |
| Light and glow | 27 | 8 | 10 | 6 | 3 | 0 |
| Distort | 30 | 13 | 4 | 10 | 1 | 2 |
| Stylize | 42 | 22 | 5 | 14 | 0 | 1 |
| Generate and overlay | 26 | 11 | 5 | 6 | 0 | 4 |
| Keying and mattes | 10 | 3 | 0 | 5 | 1 | 1 |
| Time | 12 | 9 | 1 | 1 | 0 | 1 |
| Transitions | 53 | 14 | 5 | 34 | 0 | 0 |
| Motion and animation presets | 24 | 11 | 9 | 0 | 0 | 4 |
| Text animations | 27 | 6 | 15 | 0 | 0 | 6 |
| Blend modes | 5 | 1 | 1 | 2 | 0 | 1 |
| 3D | 12 | 2 | 1 | 1 | 0 | 8 |
| Premium: Video Star packs | 18 | 4 | 13 | 1 | 0 | 0 |
| Premium: mobile Pro tiers | 10 | 2 | 1 | 0 | 4 | 3 |
| Premium: plug-ins, light and glow | 14 | 2 | 3 | 4 | 5 | 0 |
| Premium: plug-ins, blur, motion, time | 9 | 4 | 1 | 0 | 1 | 3 |
| Premium: plug-ins, distort, stylize, looks | 20 | 6 | 7 | 6 | 0 | 1 |
| Premium: plug-ins, particles, 3D, text, transitions | 18 | 7 | 4 | 2 | 0 | 5 |
| Premium: AI features | 9 | 0 | 1 | 0 | 1 | 7 |
| **Total** | **422** | **146** | **98** | **108** | **21** | **49** |

Counted from the tables above. A row often says "have" for the base effect
and lists a preset or a small option for its variants; it is counted once,
by its first status. The 324 rows stand for about 1,100 named items in the
apps: Alight Motion's 169 effects; about 615 Video Star pack items (not
counting the 84 3D emoji); the After Effects effects an edit uses (about 150
of AE's 308, leaving out audio, OCIO, expression controls, 3D-channel and
utility effects); Node Video's 35 listed effects and its release-note
additions; and the CapCut, VN and InShot effects, transitions, animations
and speed curves. The 98 premium rows add the Video Star packs the other
rows missed, the mobile Pro tiers, the parts edit makers use of about 750
plug-in effects and transitions (Sapphire about 270, Continuum about 350
current, Universe 101, plus Trapcode, Video Copilot, RE:Vision, Magic
Bullet and the aescripts tools), and the AI features; about 20 of them
refine a row above (light leaks, star glints,
saber, lens blur, footage motion blur, cut-out, depth...) with plug-in
parameters and new sources, so they are counted twice.

---

## Implementation plan

Order: what anime and manga edit makers reach for most (motion presets and
transitions on every cut, colour and glow on every clip, green-screen
overlays), then looks, then engine features. Each wave is sized for one
agent session. Every wave follows the rules in `CLAUDE.md`: registry first
(`crates/edits-core/src/effects/*.rs`), renderer second, a pixel test in
`crates/edits-render/tests/effects.rs` (transitions: the transition tests),
`docs/EFFECTS.md` / `docs/FORMAT.md` updated, `THIRD_PARTY.md` entries and
file headers for ported code, `cargo test -p edits-core -p edits-render
-p edits-cli` and clippy.

### Wave 1: motion and text presets (JSON, two small modifiers)

About 30 presets and 2 small features. Highest value per hour: every edit
uses in/out/combo animations.

**Done.** The presets below plus `scramble` (decode text with
`character_offset`); `retime` also takes a `progress` list and retimes a
video's playback (a `time_remap`), and `set` appends with a `/-` pointer.

- Presets: `fade_in`, `fade_out`, `bounce_in`, `drop_in`, `rotate_in`,
  `swirl_in`, `wipe_in`, `squish`, `swing`, `rock`, `pendulum`,
  `pulse_loop`, `party_rotate`, `party_flip`, `freeze`, `boomerang`,
  `hesitate`, the six CapCut speed curves (`velocity_montage`, `_hero`,
  `_bullet`, `_jump_cut`, `_flash_in`, `_flash_out`), text: `fade_letters`,
  `flip_letters`, `slide_letters`, `jitter_text`, `explode`, `word_pop`,
  `line_reveal`, `gradient_text`.
- Features: an `oscillate` modifier (sine on any value; `edits-core/src/eval.rs`,
  `model.rs`) and a `character_offset` text animator property
  (`edits-core/src/text_select.rs`, `edits-render/src/text.rs`).
- Files: `presets/*.json`, `crates/edits-core/src/{model,eval,preset}.rs`,
  `crates/edits-render/src/text.rs`, `docs/FORMAT.md`, the skill.

### Wave 2: transitions, batch A (30 one-pass ports)

The CapCut basics and the shapes editors use on beats: `zoom_out`,
`directional_eased`, `linear_wipe`, `clock_wipe`, `radial_wipe`,
`circle_open`, `heart_wipe`, `star_wipe`, `diamond_wipe`, `random_squares`,
`noise_dissolve`, `luma_wipe`, `window_blinds`, `window_slices`,
`grid_flip`, `inside_cube`, `simple_flip`, `book_flip`, `fold`, `doorway`,
`horizontal_open`, `split_slide`, `ripple`, `water_drop`, `glitch_bands`,
`glitch_flicker`, `vhs_tracking`, `tv_static`, `stretch_slide`, `squeeze`.

- Files: `crates/edits-render/src/shaders/transitions/*.wgsl` (+ its
  `LICENSE.md` authors), `crates/edits-core/src/effects/mod.rs`
  (`TRANSITIONS`), `crates/edits-render/src/transitions.rs` (`DEFS`),
  `docs/EFFECTS.md`.

### Wave 3: colour and keying (24 one-pass shaders)

`chroma_key`, `spill_suppress`, `luma_key`, `unmult`, `difference_matte`,
`white_balance`, `lift_gamma_gain`, `highlights_shadows`, `channel_mixer`
(RGB and HSV), `curves`, `hsl` (selective colour), `colorama` (spectral,
palette), `replace_color`, `leave_color`, `solarize`, `bleach_bypass`,
`technicolor`, `haze`, `gradient_map` `stops` option, plus presets `sepia`,
`xray`, `fade_film`, `thermal`.

- Files: `crates/edits-core/src/effects/color.rs`, a new
  `crates/edits-core/src/effects/key.rs` (category `key`),
  `crates/edits-render/src/shaders/fx/*.wgsl`,
  `crates/edits-render/src/fxlib/color.rs`, a new `fxlib/key.rs`,
  `presets/*.json`. Curves need a small 1D LUT texture upload in
  `fxlib/color.rs` (the `lut` effect's path).

### Wave 4: multi-pass light and blur (8 items)

`lens_blur` (bokeh, highlight boost), `unsharp_mask`, `bilateral_blur`,
alpha-only blur (`feather` / mask blur / inner blur option), `matte_choker`
(erode/dilate + soften), `inner_glow` and `bevel` (jump-flood distance,
shared with `outline`), `star_glow`, `glow` `mode: dark`.

- Files: `crates/edits-render/src/fx.rs`,
  `crates/edits-render/src/shaders/passes.wgsl`,
  `crates/edits-core/src/effects/{blur,light}.rs`.

### Wave 5: stylize, distort and generate (26 one-pass shaders)

`block_noise`, `jpeg_blocks`, `slice_shift`, `crt`, `film_frame`, `sketch`,
`crosshatch`, `pointillism`, `contour`, `texturize`, `prism` (fly eye /
crystal), `voronoi`, `dither` (palette), `outline` `knockout`/`blob`
options, `magnify`, `bend`, `stretch_segment`, `tile_rotate` (square and
hex), `tunnel`, `scatter`, `roughen_edges`, `corner_pin`, `four_color_gradient`,
`pattern` (checker, stripes, grid, dots), `fractal_noise` `ridged` option,
`light_sweep`, `radial_shadow`, `bump_map`, `glow_scan`.

- Files: `crates/edits-core/src/effects/{stylize,distort,generate,light}.rs`,
  `crates/edits-render/src/shaders/fx/*.wgsl`,
  `crates/edits-render/src/fxlib/{stylize,distort,generate}.rs`.

### Wave 6: transitions batch B and Video Star specials (about 25)

`page_curl` (BSD), `shatter`, `puzzle`, `paper_tear`, `burn_through`,
`ink_bleed`, `smoke_dissolve`, `comic_panels`, `halftone`, `kaleidoscope`,
`lens_push`, `tunnel`, `warp_speed`, `bounce`, `swap`, `card_swipe`,
`cross_warp`, `dreamy`, `edge_glow`, `light_wipe`, `light_streaks`,
`morph`, `mosaic`, `tv_turn_off`, `color_phase`; plus `tile_pop` (effect
and transition) and `droste`.

- Files: as wave 2, plus `crates/edits-render/src/shaders/fx/` for
  `tile_pop` and `droste`.

### Wave 7: generators and overlays that need more than a pass (7 items)

`lightning` (overlay effect, reusing the transition's bolts), procedural
`light_leak`, `fire`, `long_shadow`, `glass` (refraction by alpha normals),
`pixel_sort` (smear, then optional bitonic sort), weather and party presets
(`snow`, `rain`, `bubbles`, `confetti`, `hearts`, `starfield`, `bokeh`).

- Files: `crates/edits-render/src/{fx.rs,particles.rs}`,
  `shaders/fx/*.wgsl`, `crates/edits-core/src/effects/{generate,light,stylize}.rs`,
  `presets/*.json`.

### Wave 8: engine features, medium (7 items)

Layer `repeater` (linear, radial, grid, scatter, along path), move along
path (with auto-orient), `echo` per-copy transform (feedback tunnels),
counter text source, text `background` and multiple strokes, stencil and
silhouette blend modes, footage motion blur along the flow (RSMB).

- Files: `crates/edits-core/src/{model,eval,shape}.rs`,
  `crates/edits-render/src/{renderer,stack,compose,time,text,flow}.rs`,
  `docs/FORMAT.md`.

### Wave 9 and later: large features (one session each)

- Audio-reactive: per-frame spectrum and loudness from the song, a
  `spectrum` effect (bars, circle, waveform) and links from any param to
  loudness or a band (`edits-analyze`, `edits-core/src/song.rs`, `eval.rs`).
- `mesh` layer: sphere, cylinder, torus, prisms, polyhedra, unfolding cube
  faces (`edits-render/src/geometry.rs`, the cube path).
- Write-on text (glyph outlines into the shape renderer) and text on a path.
- Raster/text extrusion with simple lighting.
- Mesh warp / puppet.
- Tracking and stabilisation as offline commands that write keys.
- Datamosh on video layers (flow-warped previous frame).
- Segmentation and depth stay outside: document the external cut-out and
  depth workflow and make sure alpha video and depth slots import cleanly.

### Not planned

3D models (glTF), AE simulations (caustics, foam, wave world), face
landmarks, auto captions: low value for this genre or outside a renderer.

---

## Premium and plugin effects

The paid tiers of the mobile apps and the paid (and a few free) After
Effects plug-ins that anime and manga edit makers rely on: what each does,
what `edits` has, and the best permissive open-source code or model to port
or depend on. Statuses are the same as above (**have**, **preset**,
**shader**, **multipass**, **feature**). Where a row refines a row of the
categories above, it says so; the new parts are the parameters and sources.
Researched 2026-11-14.

Where the lists come from:

- Video Star: the same official page, "Where are my effects?" (90 headings:
  packs, tools and features), and the App Store listing, which says the Pro
  subscription (month, 3 months, year) unlocks every current and future
  Power Pack. The table below lists every pack the categories above did not
  already cover, so with them every pack on the page is covered.
- Alight Motion: premium effects are marked with a lock in the app only;
  neither the effect guide nor the store page names them. All 169 effects of
  the guide are in the rows above, so the premium ones are covered whatever
  the lock says.
- CapCut Pro, InShot Pro, VN Pro: the vendors publish no itemised list and
  rotate their "Pro" effects; the Pro features below are the ones review
  sites and CapCut's own pages agree on (unlimited background removal,
  motion tracking, noise reduction, AI upscaling, smooth slow motion,
  retouch, relight, auto reframe, flicker removal, AI style effects).
- Node Video Pro: the App Store description and version history (Optical
  Flow X, Fracture Pro, AI Depth Generator, Human Matting, Blob Tracer,
  Stylized Glass, Plastic, Blobbylize, Liquid Glass, Mercury).
- Sapphire: Boris FX's name index by category,
  https://borisfx.com/documentation/sapphire/ofx/name-index/ (about 270
  effects and transitions), and the S_Shake page,
  https://borisfx.com/documentation/sapphire/ae/shake/.
- Continuum (BCC): the 2026 (v19) master list,
  https://borisfx.com/documentation/continuum/bcc-effects-list/.
- Red Giant Universe: Maxon's tools locator (101 tools),
  https://help.maxon.net/rg/en-us/Content/html/universe-tools-locator.html.
- Trapcode Suite (Particular, Form, Mir, Tao, Shine, Starglow, 3D Stroke,
  Echospace, Horizon, Lux, Sound Keys), Video Copilot (Optical Flares,
  Element 3D, Twitch; Saber and Heat Distortion are free), RE:Vision
  (Twixtor, RSMB), Magic Bullet (Looks, Colorista, Film, Cosmo, Denoiser),
  aescripts sellers (Deep Glow, Pixel Sorter 3, Glitchify, Datamosh 2, Flow,
  Motion 4; Quick Chromatic Aberration 3 and Displacer Pro are free from
  Plugin Everything): vendor product pages. Which of them editors call
  "must-have" comes from AMV project packs and tutorials, which list
  Sapphire, BCC, Deep Glow, Glitchify, RSMB, Twixtor and Optical Flares
  again and again, plus Universe, Saber and shake packs.

App short names in this section, besides those above: SA = Sapphire,
BCC = Boris Continuum, UN = Red Giant Universe, TC = Trapcode, VC = Video
Copilot, RV = RE:Vision Effects, MB = Magic Bullet, AS = aescripts and
Plugin Everything tools.

New port sources (licences read from the repositories and model cards in
this session):

| Short name | Repository | Licence |
| --- | --- | --- |
| Bevy | https://github.com/bevyengine/bevy: `crates/bevy_post_process/src/{bloom/bloom.wesl, dof/dof.wesl, motion_blur/motion_blur.wesl, auto_exposure/auto_exposure.wesl, effect_stack/chromatic_aberration.wesl, effect_stack/lens_distortion.wesl}`, `crates/bevy_core_pipeline/src/tonemapping.wesl`. Already WGSL. | MIT OR Apache-2.0 |
| postprocessing | `src/effects/glsl/{bokeh,god-rays,ascii,glitch,lens-distortion}.frag`, `src/materials/glsl/convolution.bokeh.frag`. Not `realistic-bokeh.frag`: it follows Martins Upitis' shader, published under CC BY 3.0 | zlib |
| three.js | `examples/jsm/tsl/display/{LensflareNode,lensDirt,MotionBlur,DepthOfFieldNode}.js`. Not `shaders/BokehShader2.js` (Upitis, as above); `shaders/DigitalGlitch.js` says it is based on staffantan's unityglitch, whose licence is not stated | MIT |
| Anime4K | https://github.com/bloc97/Anime4K (`glsl/`: real-time anime upscale and line passes, mpv hooks) | MIT |
| Tony McMapface | https://github.com/h3r2tic/tony-mc-mapface (a tonemapping LUT; Bevy ships it) | MIT |
| crt-lottes | https://github.com/libretro/slang-shaders/blob/master/crt/shaders/crt-lottes.slang; most other slang files are GPL or CC BY: read each header | public domain |
| AI models | see [AI features](#ai-features-permissive-models-only); inference through [ort](https://github.com/pykeio/ort) (ONNX Runtime) or [burn](https://github.com/tracel-ai/burn) | MIT OR Apache-2.0 |

AI models never run inside a frame: a frame must stay a pure function of
the project and the frame number. They run as offline commands in
`edits-analyze` that write files (an alpha video, a depth sequence, frames)
under `assets/`, which the project then uses like any other media.

### Video Star paid packs not covered above

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Boogie boxes (Boogie On, Zoom, Zoom Spin, 'Round, In, Out, Woogie) | VS Boogie Boxes | **preset** `cube` layer with the picture on its faces, keyed `rotation` and `scale` on beats (`cube_turn`, `punch`) | — | S |
| Electric rays and anime energy (E-Rays, E-Anime) | VS Elektra | **preset** duplicate layer: `edge_detect` + `turbulent_displace` (fast `evolution`) + `glow`, and `light_rays` from the subject, blend `add` | — | S |
| Smears (Tint Smear Go, Static Smear), Enhanced Sat | VS Special Freebies | **preset** `echo` (many copies, `decay` 0.8) + `tint` or `grain`; Enhanced Sat is `vibrance` | — | S |
| Green-screen 3D worlds (Another World, Bigger World, Fisheye World, Crazy World) | VS Green Screen 3D Worlds | **preset** `chroma_key` on the clip over a picture with `lens_distortion` or `polar` and keyed 3D `rotation` | — | S |
| Hypnotic backgrounds (Hypno Bright, Muted, Gray, ♩) | VS Hip Hypnotics | **preset** radial `gradient_ramp` + `colorama` (`cycles` 8, keyed `phase`) + `twirl`; an exact spiral: **shader** option `spiral` on the Wave 5 `pattern` | own | S |
| Frame layouts (Half Rev/Flip, Rotate Sides, Quarter Flips, Side Panels, Framed/Circles/Progress Go and ♩) | VS I've Been Framed! | **preset** comps with rect and ellipse `masks`, mirrored copies, and a progress bar from a shape `trim` | — | S |
| Multi-clip panels | VS Amazing Multi-Panel Mixer | **preset** `split_panels` with a `video` slot per panel | — | S |
| Animated GIF stickers (Adv GIF, Adv GIF ♩, GIF Pop, GIF Track) | VS Animated GIF Kit | **have** a `video` layer reads GIF through ffmpeg; the pop on beats is `punch`; GIF Track needs tracking (Motion row) | — | — |
| Photo overlays and ghost snapshots (Double Photo, Bright Photo, Ghost Snap, Ghost Timed, Ghost ♩, Go variants) | VS Photo Essentials | **preset** an image layer `screen` or `lighten` at part opacity; a ghost is a held copy (`time_remap` hold keys) that fades | — | S |
| Flying photos (Photo Track, Photo Pop, Multi Pop) | VS Flying Fotos | **preset** image layers with `slide_in` / `pop_in` on beats; Photo Track needs tracking | — | S |
| Doodle overlays: zaps, swirls, arrows, marquees | VS Zaps!, Zaps! #2 | **preset** shape layers: `path` + `trim` + `wiggle_path` + `glow`; marquee lights are a stroke `dash` with keyed `dash_offset` | — | S |
| Drawing and transforming frames | VS Draw Lab, Art Studio | **preset** shape `path` layers keyed per beat; hand-drawn frames come in as an image sequence | — | S |
| Fridge-magnet letters | VS World of Wordcraft (Fridge Magnet) | **preset** text animators: `wiggly` rotation per letter and a random per-letter `fill` | — | S |
| Explosions (Ka-BOOM, Shockers, Sparky) | VS Flame On! | **preset** `particles` burst + `shockwave` + `flash`; flames need the Wave 7 `fire` | — | S |
| Crash, Wipe Lines, Zoom Flow | VS Wild Warp Transitions, Motion Blur Transitions | **have** `glitch_displace` / `camera_shake` (Crash), `window_slices` (Wipe Lines), `cross_zoom` (Zoom Flow) | — | — |
| Shape Shifting, Totally Terrific transitions (items not published) | VS | **shader** the Wave 6 gl-transitions ports (`morph`, `swap`, `card_swipe`, shapes) cover what tutorials show | gl-transitions | S |
| Premium fonts | VS BFF (Best Fantasy Font) | **have** text `font` takes any OTF or TTF; ship OFL fonts only, never the app's licensed fonts | — | — |
| Recording tools (Advanced AutoStop, Clap to Record, Onion Skin Overlay) | VS Advanced AutoStop, Perks Pack | **have** onion skin is `echo`; recording is outside the engine | — | — |

### Mobile Pro tiers: CapCut Pro, Node Video Pro, InShot and VN Pro

AI items (cut-out, depth, interpolation, upscale, tracking, style) are in
[AI features](#ai-features-permissive-models-only).

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| "Pro" effects, filters, transitions and text templates | CC Pro; IS Pro; VN Pro; AM membership | **have** the stable ones are the rows of the categories above, by name; the vendors do not itemise them | — | — |
| Smooth slow motion, Optical Flow X | CC Pro; NV Pro Optical Flow X | **have** `frame_blend: flow`; its motion-blur half is the footage motion blur row (Blur, below) | own `flow.rs` | — |
| Stylised materials (Glass, Plastic, Blobbylize, Liquid Glass, Mercury; Glass material in 3D) | NV Pro; AE CC Blobbylize, CC Plastic | **multipass** blur the alpha and threshold it (blobs merge), normals from the blurred alpha, then lit: refraction (glass), specular highlight (plastic), the picture as environment (mercury); refines the Distort row "Glass" | [three.js NormalMapShader.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/shaders/NormalMapShader.js) MIT; own | M |
| Blob tracking (boxes, labels and lines over moving bright blobs) | NV Blob Tracer; CC and AE templates | **feature** blobs per frame (threshold + connected components on the CPU, as `flow.rs` works), drawn as shape and text layers by an offline command that writes keys | own | M |
| Embers | NV particles (Snow, Rain, Embers) | **preset** `particles`: spark sprites rising, orange, blend `add` | — | S |
| Retouch (skin smoothing) | CC Pro Retouch; SA S_Beauty; BCC+ Beauty Studio | **multipass** `bilateral_blur` (Wave 4) under a soft mask; face and body reshaping needs face landmarks: not planned | as the Blur row | M |
| Colour match to a reference shot | CC Pro AI colour correction; BCC Color Match | **multipass** mean and spread of each Lab channel of both pictures (a reduction), then a per-channel affine map (Reinhard colour transfer) | own | M |
| Deflicker | CC Pro Remove flickers; SA S_FlickerRemove; BCC Flicker Fixer | **feature** per-frame mean luminance of a video smoothed over time and applied as `exposure` keys, by an offline command | own | M |
| Auto reframe (crop to 9:16 following the subject) | CC Pro Auto reframe; BCC Reframer; UN Reframe | **feature** the subject's box per frame from the cut-out mask, smoothed into `position` keys | own | M |
| Noise reduction / enhance | CC Pro Reduce noise; BCC+ DeNoise ML; MB Denoiser | **multipass** bilateral or non-local means (anime sources are rarely noisy: low value) | [glfx.js adjust/denoise.js](https://github.com/evanw/glfx.js/blob/master/src/filters/adjust/denoise.js) MIT | M |

### After Effects plug-ins: light and glow

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Deep, physically based glow | AS Deep Glow 2, Real Glow (free); SA S_Glow, S_UltraGlow; BCC+ Glow; UN Glow, Glo Fi II | **have** `bloom` (a mip chain weighted by size, the same at any render scale) | [Bevy bloom.wesl](https://github.com/bevyengine/bevy/blob/main/crates/bevy_post_process/src/bloom/bloom.wesl) to compare the down/up-sampling | — |
| Glow controls: falloff, chromatic glow, anamorphic aspect, glow from darks | AS Deep Glow (falloff, aberration, aspect); SA S_GlowRainbow, S_GlowDarks, S_GlowDist; UN Chromatic Glow; BCC+ Glow Darks | **multipass** options on `bloom`: `falloff` (level weights following 1/size², Deep Glow's inverse-square look), `chroma` (red and blue levels at different sizes), `aspect` (anamorphic stretch), `mode` `dark` (the Light-and-glow "Dark glow" row) | [Bevy bloom.wesl](https://github.com/bevyengine/bevy/blob/main/crates/bevy_post_process/src/bloom/bloom.wesl) MIT/Apache (its `scale` uniform is the anamorphic stretch); own | M |
| Glow on edges, glow aura, glow rings, ecto | SA S_GlowEdges, S_GlowAura, S_GlowRings; BCC Glow Alpha Edges, Colorize Glow; UN Edge Glow, Ecto | **preset** `edge_detect` or `outline` on a duplicate + `bloom`, blend `add`; rings: `ring_pulse` + `bloom` | — | S |
| Diffusion, Pro-Mist, soft focus, Orton | MB Looks (Diffusion); BCC+ Diffusion, Mist, Silk, Halo, Film Glow; SA S_SoftFocus, S_Diffuse; UN Overlight | **preset** `bloom` with a low `threshold` (about 0.3) and a wide `radius`, plus `levels` lifting the blacks; Orton: a blurred copy blended `screen` | — | S |
| Halation (red glow around highlights) | SA S_Halation; BCC+ Film Stocks; MB Looks | **preset** `bloom` `color` [1, 0.3, 0.1], `threshold` 0.7, small `radius` | — | S |
| Star glints | TC Starglow; SA S_Glint, S_GlintRainbow, S_Glare; BCC+ Multi-Star; UN Glimmer | **multipass** the bright pass streaked in 4, 6 or 8 directions, a colour per direction (the Light-and-glow row, Wave 4 `star_glow`) | own (Blender's glare "Streaks" is GPL: formula only) | M |
| Shine (volumetric rays, colourised) | TC Shine; SA S_Rays, S_EdgeRays; BCC+ Rays | **have** `light_rays`; colourised rays: **preset** + `gradient_map` on a duplicate | [postprocessing god-rays.frag](https://github.com/pmndrs/postprocessing/blob/main/src/effects/glsl/god-rays.frag) zlib | — |
| Flare designer (element lists: glows, streaks, rings, polygon ghosts, iris, anamorphic streak) | VC Optical Flares; SA S_LensFlare; BCC Lens Flare 3D, BCC+ Lens Flare; UN Knoll Light Factory EZ | **shader** `lens_flare` option `elements`: typed elements with size, colour, place along the axis through the centre and shape (disc, ring, n-sided polygon, streak); our three `lens` values become presets | own; the vendors' flare presets and lens textures cannot be copied | M |
| Flares from highlights (auto-tracked flare, lens dirt) | SA S_LensFlareAutoTrack; VC Optical Flares (tracked); BCC+ Lens Flare | **multipass** pseudo lens flare: ghosts and a halo sampled from the bloom's bright pass, mirrored through the centre, over a generated dirt texture | [three.js LensflareNode.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/tsl/display/LensflareNode.js), [lensDirt.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/tsl/display/lensDirt.js) MIT (after John Chapman's pseudo lens flare) | M |
| Light leaks (procedural, animated) | SA S_LightLeak, DissolveLightLeak; BCC+ Light Leaks; MB Looks | **shader** soft colour blobs drifting, blended `screen` (refines the Light-and-glow row) | own | M |
| Lightning, zaps | SA S_Zap, S_UltraZap, S_ZapTo; BCC Lightning; UN Electrify | **shader** lightning overlay (Light-and-glow row, Wave 7), point to point or along a shape path | reuse `LightningFlash.wgsl` | M |
| Saber / energy stroke | VC Saber (free); NV Saber; SA S_LaserBeam; BCC Laser Beam | **multipass** a `saber` effect on a layer's alpha (shape, text, mask): distance from the jump flood `outline` already runs, a white core plus a coloured exponential glow, the distance distorted by noise and flickered per frame (refines the Light-and-glow row, which only had a preset) | own (reuse `outline`'s JFA) | M |
| Light wrap (background light over a cut-out's edge) | BCC+ Light Wrap; Red Giant Supercomp | **multipass** blur what is beneath, keep a band inside this layer's edge (blurred inverse alpha x alpha), add or screen it | own | M |
| Aurora, night sky | SA S_Aurora, S_NightSky | **shader** noise curtains coloured by height (a generator) | own | S |

### After Effects plug-ins: blur, motion and time

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Rack / lens defocus with bokeh | SA S_RackDefocus, S_AdvancedDefocus, S_DefocusPrism; BCC+ Lens Blur, Rack Focus; UN Bokeh, Spot Blur; Frischluft Lenscare | **multipass** `lens_blur` (Blur row, Wave 4): disc or hexagon bokeh, highlight boost, keyed `amount` for racks | [Bevy dof.wesl](https://github.com/bevyengine/bevy/blob/main/crates/bevy_post_process/src/dof/dof.wesl) MIT/Apache (two-pass hexagonal bokeh); [postprocessing bokeh.frag](https://github.com/pmndrs/postprocessing/blob/main/src/effects/glsl/bokeh.frag), [convolution.bokeh.frag](https://github.com/pmndrs/postprocessing/blob/main/src/materials/glsl/convolution.bokeh.frag) zlib; [three.js DepthOfFieldNode.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/tsl/display/DepthOfFieldNode.js) MIT | M |
| Depth-based defocus, fog and wipe | SA S_ZDefocus, S_ZBlur, S_ZFogLinear; BCC+ Depth of Field, Depth Wipe ML; NV Depth Generator | **feature** a depth slot (from the AI depth row) read through `fx_aux` by `lens_blur`, a `depth_fog` and `luma_wipe` | as above | M |
| Footage motion blur (RSMB) | RV ReelSmart Motion Blur; BCC+ Motion Blur ML; NV Optical Flow X | **feature** blur each video frame along the flow `flow.rs` already estimates (the Blur row; Wave 8) | own `flow.rs`; [Bevy motion_blur.wesl](https://github.com/bevyengine/bevy/blob/main/crates/bevy_post_process/src/motion_blur/motion_blur.wesl) MIT/Apache, [three.js MotionBlur.js](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/tsl/display/MotionBlur.js) MIT for sampling along a velocity | M |
| Optical-flow retiming | RV Twixtor; SA S_Retime; BCC+ Retimer ML | **have** `frame_blend: flow` with `speed` / `time_remap`; a model-quality interpolator is in the AI table | own | — |
| Blur along keyed motion | SA S_BlurMoCurves | **have** `settings.motion_blur` (sub-frame) | — | — |
| Trails, feedback, RGB time split | SA S_Trails, S_Feedback, S_TimeWarpRGB; BCC Trails; TC Echospace | **have** `echo`; RGB time split (red now, green and blue a little later): **shader** option `channels` on `echo` | own | S |
| Shake with styles, per-axis noise, RGB shake and mirrored edges | SA S_Shake (Normal, Twitchy, Jumpy; X, Y, Z, tilt; per-channel amplitude; wrap Reflect); BCC+ Camera Shake; UN Camera Shake; shake packs (ShakeDat, Easy Shake, Klain Shake Pack, AEJuice) | **feature** small: `shake` modifier `octaves` (fractal noise) and `style` `smooth` / `twitchy` (bursts after stillness) / `jumpy` (jumps and drift) / `stepped` (held random values); preset `shake_rgb` (the same noise drives `rgb_split`); mirrored edges: shake the `offset` of `tile` (`tiles` [1, 1], `mirror` 1) | own (parameters as the S_Shake page lists them) | S |
| Twitch (random jolts of blur, colour, light, scale, slide and time) | VC Twitch | **preset** `stepped` shake on scale and position + `rgb_split`, `exposure` and `directional_blur` hits from `beats_accent`; time jumps with `time_remap` hold keys | — | S |
| Easing tools | AS Flow, Motion 4, Ease and Wizz | **have** named eases and cubic bezier keys `[x1, y1, x2, y2]`; a library of named curves is JSON | — | — |

### After Effects plug-ins: distort, stylize, glitch and looks

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Chroma warps (spectral spread on any warp) | SA S_WarpChroma, S_DistortChroma, S_DistortRGB; BCC+ Prism, Swish Prism; UN Prism Displacement | **shader** option `spectrum` (3 to 16 hue-weighted taps) on `zoom_blur`, `lens_distortion`, `turbulent_displace` and `displacement_map` | [Bevy chromatic_aberration.wesl](https://github.com/bevyengine/bevy/blob/main/crates/bevy_post_process/src/effect_stack/chromatic_aberration.wesl) MIT/Apache (taps weighted by a spectrum table) | S |
| Self displacement (distort by the picture's own luminance) | SA S_Distort, S_DistortBlur | **shader** `displacement_map` `source` `self` (the blurred gradient of the layer's luminance) | own | S |
| Displacement by a map | AS Displacer Pro (free); BCC Displacement Map | **have** `displacement_map` | — | — |
| Chromatic aberration | AS Quick Chromatic Aberration 3 (free); BCC+ Chromatic Aberration; UN Chromatic Aberration, RGB Separation | **have** `rgb_split`, `chromatic_aberration` | — | — |
| Heat distortion | VC Heat Distortion (free); UN Heatwave | **preset** masked `turbulent_displace` with a fast `evolution` (Distort row) | — | S |
| Warps (bubble, puddle, vortex, waves, fisheye, magnify, perspective) | SA S_WarpBubble, S_WarpPuddle, S_WarpVortex, S_WarpWaves, S_WarpFishEye, S_WarpMagnify, S_WarpCornerPin; BCC Bulge, Ripple, Twirl, Wave | **have** `bulge`, `ripple`, `twirl`, `wave_warp`, `lens_distortion`; magnify and corner pin: Wave 5 | — | — |
| Infinite zoom, warp repeat, stretched frame edges | SA S_InfiniteZoom, S_WarpRepeat, S_StretchFrameEdges | **preset** nested image layers scaled by powers of one factor with exponential keys; edges: mirrored `tile` | — | S |
| Glitch suites | UN Glitch, VHS, Retrograde, Analog, AV Club, MisFire; SA S_DigitalDamage, S_JpegDamage, S_TVDamage, S_VHSDamage; BCC+ Video Glitch, Damaged TV, RGB Pixel Noise; AS Glitchify, Data Glitch | **have** `glitch`, `vhs`, `film_damage`, `scanlines`; block and JPEG damage are Wave 5 shaders | [postprocessing glitch.frag](https://github.com/pmndrs/postprocessing/blob/main/src/effects/glsl/glitch.frag) zlib | — |
| Hologram | UN Holomatrix II | **preset** cyan `tint` + moving `scanlines` + light `glitch` + `bloom`, opacity flicker | — | S |
| Pixel sort | AS Pixel Sorter 3; SA S_PixelSort | **shader** smear, true sort **multipass** (Stylize row, Wave 7) | [ISF Sorting Smear.fs](https://github.com/Vidvox/ISF-Files/blob/master/ISF/Sorting%20Smear.fs) MIT | M |
| Datamosh | AS Datamosh 2; SA S_PixelMosh | **feature** (Stylize row) | own `flow.rs` | L |
| ASCII / symbol mosaic | UN Symbol Mapper | **shader** cells drawn with glyphs from our font atlas, chosen by luminance | [postprocessing ascii.frag](https://github.com/pmndrs/postprocessing/blob/main/src/effects/glsl/ascii.frag) zlib | S |
| Watercolour, charcoal, pencil | BCC Water Color, Charcoal Sketch, Pencil Sketch, Artists Poster; SA S_AutoPaint, S_Etching | **shader** watercolour: `kuwahara` + darkened edges + paper noise; charcoal and pencil: the Stylize sketch row | own | S |
| Stylize suites (halftone, cartoon, posterize, mosaic, kaleido, fly's eye, emboss, scanlines) | SA S_HalfTone, S_Cartoon, S_Posterize, S_Mosaic, S_Kaleido, S_FlysEye, S_Emboss, S_ScanLines; BCC Cartoon Look, Halftone, Tile Mosaic; UN Sketchify, Dither and Palettes | **have** `halftone`, `toon`, `posterize`, `pixelate`, `kaleidoscope`, `emboss`, `scanlines`; fly's eye and dither: Wave 5 | — | — |
| Film looks (stock emulation, gate weave, grain, damage) | SA S_FilmEffect, S_FilmDamage, S_VintageColor2Strip/3Strip; BCC+ Film Stocks, Film Grunge, Three Strip, Two Strip; MB Film; UN Retrograde, Grain 16, Noir | **preset** `film_damage` + `grain` + `technicolor` or `bleach_bypass` + `curves`; gate weave: a slow `shake` on position; stock looks as `curves` + `lift_gamma_gain` presets with our own numbers | own; [Tony McMapface](https://github.com/h3r2tic/tony-mc-mapface) MIT for a tonemapping curve | S |
| Look stacks (Mojo, Cosmo, Colorista, diffusion, lens, vignette) | MB Looks, Colorista, Mojo, Cosmo; UN Finisher; BCC+ Looks | **preset** families: chains of `lift_gamma_gain`, `curves`, `hsl`, `bloom` (diffusion), `lens_distortion`, `vignette`, `grain`; Mojo is teal shadows and warm skin (`hsl` + split toning) | own; the vendors' look libraries cannot be copied | S |
| Split tone, tritone, gradients | BCC+ Split Tone, Tritone, Colorize Gradient, Gels, Sunset; SA S_DuoTone, S_TriTone, S_QuadTone | **have** `gradient_map`, `lift_gamma_gain`, `gradient_ramp` | — | — |
| Day for night, night vision, infrared | BCC+ Day For Night, Night Vision, Infrared, Color Infrared, X-Ray | **preset** `levels` + blue `tint` + `saturation` (day for night); green `tint` + `grain` + `vignette` (night vision); `channel_mixer` (infrared); `xray` | — | S |
| Effect only where a colour, luma or mask selects | BCC Pixel Chooser, BCC+ Pixel Chooser | **preset** an adjustment layer whose `matte` is a duplicate keyed with `chroma_key`, `luma_key` or `leave_color`; or `masks` | — | S |
| Long shadow | UN Long Shadow | **shader** (Light-and-glow row) | own | M |

### After Effects plug-ins: particles, 3D, text and transitions

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Particle systems and emitter libraries | TC Particular; BCC Particle Illusion, 2D Particles, Particle Emitter 3D; SA S_Sparkles, S_Dust | **have** `particles` layer; a library is presets | — | — |
| Emit from a layer, text or path; trails; turbulence | TC Particular (layer and text emitters, aux system, air turbulence); BCC Particle Emitter 3D | **feature** emitter `from` a layer's alpha or a shape path, sub-emitter trails, curl-noise turbulence | own (`particles.rs`) | M |
| Picture to particles (disintegrate, assemble) | TC Form; BCC Pin Art 3D; AE CC Pixel Polly | **feature** one particle per cell of a slot, coloured by it, pushed by noise and a keyed `progress` (the "dust away" cut) | own | M |
| Rain, snow | BCC Rain, Snow | **preset** (Generate row) | — | S |
| 3D strokes, ribbons, terrain, echo space | TC 3D Stroke, Tao, Mir, Echospace | **preset** 3D Stroke: shape `trim` on a layer with 3D `rotation`; Echospace: layer `repeater` (Wave 8); Tao and Mir need a `mesh` layer (low value) | — | S |
| Extruded 3D text and logos | VC Element 3D; BCC Extruded Text, Extruded Spline, Title Studio | **feature** (3D row: extrusion); Element 3D's model packs are not portable | own | L |
| 3D shatter | BCC 3D Image Shatter, 3D Extruded Image Shatter | **feature** (3D row) | [EffectCraft sim2/pieces.rs](https://github.com/storytold/effectcraft/blob/main/crates/effects/src/sim2/pieces.rs) | L |
| Cube, sphere, cylinder, page turn, flipper | BCC Cube, Sphere, Cylinder, Page Turn, Fast Flipper | **have** `cube`; the others: `mesh` layer (Wave 9) and `page_curl` (Wave 6) | — | — |
| Planar tracking (pin a picture or text to a surface) | Mocha AE and Pro; BCC Match Move, Corner Pin Studio | **feature** (Motion row: tracking) four tracked corners into `corner_pin` (Wave 5) | own `flow.rs`; TAPNet (AI table) | L |
| Text effects (hacker decode, type-on, text tile) | UN Hacker Text, Type On, Text Tile, Typographic; BCC Type-On Text | **have** `scramble`, `typewriter`; a text tile is `tile` on a text layer | — | — |
| Audio-reactive | TC Sound Keys; BCC Beat Reactor, BCC+ Audio Visualizer | **have** hits on beats (`beats_accent`); loudness links and a `spectrum` effect: **feature** (Wave 9) | own | L |
| HUD and motion-graphics kits | UN HUD Components, Array Gun, Progresso, Line | **preset** shape layers (`rect`, `ellipse`, `path`, `dash`, `trim`, `repeater`) | — | S |
| Effect dissolves (glow, flare, rays, light leak, shake, zap, prism, blur) | SA Dissolve* (30, e.g. DissolveGlow, DissolveLensFlare, DissolveShake, DissolveZap); BCC+ Atmospheric Glow, Camera Shake, Crash Zoom, Light Leaks, Rays, Prism Dissolves; UN Exposure Blur, Knoll Light Transition, Bokeh Transition | **preset** a `dissolve_*` family: the effect peaks at the cut on both pictures (pulse keys) while `fade` crosses | — | S |
| Procedural wipes (blobs, cells, clouds, plasma, dots, weave, rings, stripes) | SA Wipe* (26); BCC Blobs Wipe, Checker Wipe, Rings Wipe, Tile Wipe, Texture Wipe | **shader** option `map` on `luma_wipe`: a built-in noise, cell or ring pattern instead of a slot | own | S |
| Film roll, flutter cut, channel change, swish | SA FilmRoll, FlutterCut, TVChannelChange, SwishPan, Swish3D, WhipLash; BCC+ Film Roll, Flutter Cut, Swish Pan; UN Channel Surf, Flicker Cut, Swish Pan | **have** `whip_pan`, `whip_tilt`, `tv_static`; flutter cut: **preset** opacity hold keys; film roll: **shader** | [gl-transitions FilmStrip.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/FilmStrip.glsl) MIT | S |
| 3D transitions (Rubix Cube, Carousel, Cube, Inside Cube, Fold, Unfold, Diamond Wave, Triangle Wave, Color Mosaic) | UN | **have** `cube`, `inside_cube`, `fold`; Rubix Cube and Carousel: **preset** `cube` layers; waves and mosaic: **shader** (Wave 6 `mosaic`) | gl-transitions | S |
| Parallax strips | SA S_ParallaxStrips, ParallaxStripsTransition; BCC Multi-Stripe Wipe, Multi-Stretch Wipe | **shader** strips of the picture moving at different speeds (effect and transition) | own | S |
| Transition preset packs | Animation Composer, Motion Bro, AEJuice packs | **have** the Transitions table and presets | — | — |

### AI features (permissive models only)

Each is an offline command that writes media, never a step inside a frame.
Model weights are downloaded by the user (like ffmpeg), never committed.

| Effect | Apps | Ours | Port source | Effort |
| --- | --- | --- | --- | --- |
| Anime character cut-out (remove background, human matting) | CC Pro; NV Pro Human Matting; IS Pro AI cutout; AE Roto Brush; needed by VS Hair Dye, auras, light wrap, auto reframe | **feature** `edits cutout`: a segmentation model per frame, written as a PNG sequence or an alpha video (the renderer already reads alpha) | [anime-segmentation](https://github.com/SkyTNT/anime-segmentation) Apache-2.0, weights [skytnt/anime-seg](https://huggingface.co/skytnt/anime-seg) Apache-2.0; [ToonOut](https://github.com/MatteoKartoon/BiRefNet) MIT, weights [joelseytre/toonout](https://huggingface.co/joelseytre/toonout) MIT (a BiRefNet fine-tuned on anime); general: [BiRefNet](https://github.com/ZhengPeng7/BiRefNet) MIT, [DIS / IS-Net](https://github.com/xuebinqin/DIS) Apache-2.0, [U²-Net](https://github.com/xuebinqin/U-2-Net) Apache-2.0; people: [MODNet](https://github.com/ZHKKKe/MODNet) Apache-2.0; prompted video: [SAM 2](https://github.com/facebookresearch/sam2) Apache-2.0 | L |
| Depth map (3D photo parallax, depth blur, fog, depth wipe, relight) | NV Pro AI Depth Generator; CC "3D zoom"; BCC+ Depth Map ML | **feature** `edits depth` writes a 16-bit depth sequence; a `depth_parallax` effect moves pixels by depth (with an occlusion fill) through `fx_aux` | [Depth Anything V2](https://github.com/DepthAnything/Depth-Anything-V2) **Small only**, Apache-2.0 ([weights](https://huggingface.co/depth-anything/Depth-Anything-V2-Small)); video: [Video Depth Anything](https://github.com/DepthAnything/Video-Depth-Anything) Small, Apache-2.0; [MiDaS](https://github.com/isl-org/MiDaS) MIT | L |
| AI frame interpolation | CC Pro smooth slow-mo; RV Twixtor; BCC+ Retimer ML | **feature** `edits interpolate` writes in-between frames for a slow-motion section; `frame_blend: flow` stays the built-in default | [Practical-RIFE](https://github.com/hzwer/Practical-RIFE) MIT, [rife-ncnn-vulkan](https://github.com/nihui/rife-ncnn-vulkan) MIT; [IFRNet](https://github.com/ltkong218/IFRNet) MIT; [FILM](https://github.com/google-research/frame-interpolation) Apache-2.0 | M |
| Upscale low-resolution anime clips | CC Pro enhance; BCC+ UpRes ML | **multipass** Anime4K's passes ported to WGSL (real time); offline: `edits upscale` with an anime model | [Anime4K glsl/](https://github.com/bloc97/Anime4K/tree/master/glsl) MIT; [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN) BSD-3-Clause (anime video model), [Real-ESRGAN-ncnn-vulkan](https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan) MIT; [Real-CUGAN](https://github.com/bilibili/ailab/tree/main/Real-CUGAN) MIT; [nunif (waifu2x)](https://github.com/nagadomi/nunif) MIT | M |
| Model-quality optical flow (for RSMB, datamosh, interpolation) | RV; BCC+ Motion Blur ML | **feature** an optional offline flow that `flow.rs` users can read | [RAFT](https://github.com/princeton-vl/RAFT) BSD-3-Clause | M |
| Point tracking | CC Pro tracking; Mocha | **feature** offline points written as `position` keys (Motion row) | [TAPNet / TAPIR](https://github.com/google-deepmind/tapnet) Apache-2.0 | L |
| Anime or manga look on live footage | CC Pro AI effects | **preset** `toon` + `kuwahara` + `ink` + `screentone` (no model); a learned style needs weights we train ourselves | [pytorch/examples fast_neural_style](https://github.com/pytorch/examples/tree/main/fast_neural_style) BSD-3-Clause (code only) | L |
| Relight | CC Pro Relight; BCC+ ReLight | **feature** normals from the depth map, lit as `bump_map` would light them | own, after the depth row | M |
| Face retouch and reshape | CC Pro Retouch; BCC+ Face ML | **feature** face landmarks: not planned (live action, low value here); smoothing alone is `bilateral_blur` | — | L |

### Sources that cannot be used

| Source | Licence | Use instead |
| --- | --- | --- |
| BRIA RMBG-1.4 | BRIA licence, non-commercial | anime-segmentation, ToonOut, BiRefNet |
| BRIA RMBG-2.0 | CC BY-NC 4.0 | as above |
| Depth Anything V2 Base, Large, Giant; Video Depth Anything Base, Large | CC BY-NC 4.0 | the Small models (Apache-2.0) |
| Apple Depth Pro weights | `apple-amlr` (Apple machine-learning research licence) on the model card | Depth Anything V2-Small |
| AnimeGANv2, AnimeGANv3 | non-commercial only | presets; own-trained fast style transfer |
| White-box Cartoonization | CC BY-NC-SA 4.0 | as above |
| CoTracker | CC BY-NC 4.0 | TAPNet |
| RobustVideoMatting | GPL-3.0 | MODNet, BiRefNet |
| APISR | GPL-3.0 | Real-ESRGAN, Real-CUGAN, Anime4K |
| OpenFX-Misc and Natron plug-ins | GPL-2.0 | own (the OpenFX API itself is BSD-3-Clause) |
| libretro `crt-royale` and the other GPL slang files | GPL-2.0 | `crt-lottes` (public domain), three.js `CRT.js` |
| Blender compositor (glare, defocus, Kuwahara) | GPL-2.0 or later | formulas only, our own code |
| godotshaders.com entries marked GPL v3 | GPL-3.0 | the entries marked CC0 or MIT |
| Shadertoy without a stated licence | CC BY-NC-SA 3.0 by default | formulas only |
| Upitis' bokeh (three.js `BokehShader2.js`, postprocessing `realistic-bokeh.frag`), slang `film-grain` | CC BY 3.0 at the origin: usable only with attribution, so prefer the alternatives | Bevy `dof.wesl`, postprocessing `bokeh.frag`, our `grain` |
| The plug-ins and apps themselves: binaries, presets, flare and lens textures, emitter libraries, look libraries, Element 3D models, Video Star's packs and fonts | proprietary | behaviour rebuilt from public documentation, our own presets, OFL fonts |
| Product names as effect names (Twixtor, Deep Glow, Optical Flares, Sapphire, Particular...) | trademarks | generic names (`bloom`, `lens_flare`, `frame_blend: flow`) |

### Premium waves

Ranked by value for anime and manga edits: what shows on every beat first
(shake, glow, motion blur), then the signature looks (flares, sabers, leaks,
film), then the AI features that open new edits (depth parallax, cut-out).
Each wave is one agent session and follows the rules of the implementation
plan above (registry, renderer, pixel test, docs, `THIRD_PARTY.md`). Items
already scheduled in Waves 4 to 9 are pulled forward here where they are
the premium ones.

1. **Shake (S_Shake quality).** `shake` modifier `octaves` and `style`
   (`smooth`, `twitchy`, `jumpy`, `stepped`); presets `shake_hard`,
   `shake_twitch` (Twitch), `shake_rgb`, `shake_mirror` (mirrored edges via
   `tile`), all with `settings.motion_blur` on. Files:
   `crates/edits-core/src/{model,eval,preset}.rs`, `presets/*.json`,
   `docs/FORMAT.md`, the skill.
2. **Deep glow.** `bloom` options `falloff`, `chroma`, `aspect`, `mode`
   `dark`; presets `diffusion` (Pro-Mist), `halation`, `edge_glow`,
   `orton`. Files: `crates/edits-render/src/fx.rs`,
   `shaders/passes.wgsl`, `crates/edits-core/src/effects/light.rs`,
   `presets/*.json`. Source: Bevy `bloom.wesl`.
3. **Footage motion blur (RSMB).** Per-pixel blur of video frames along the
   `flow.rs` field, `motion_blur` on `video` layers, shutter from
   `settings.motion_blur`. Files: `crates/edits-render/src/{flow,video,time}.rs`,
   a new `shaders/fx/vector_blur.wgsl`, `docs/FORMAT.md`,
   `docs/RENDERING.md`. Source: Bevy `motion_blur.wesl`.
4. **Saber and lightning.** A `saber` effect on alpha (JFA distance, core,
   glow, noise, flicker) and the lightning overlay sharing the transition's
   bolt code; presets `lightsaber`, `energy_outline`, `neon_sign`. Files:
   `crates/edits-render/src/fx.rs`, `shaders/passes.wgsl`,
   `shaders/fx/lightning.wgsl`, `crates/edits-core/src/effects/light.rs`.
5. **Flares.** `lens_flare` `elements` (the flare designer) and a
   `flare_from_highlights` pass with generated lens dirt; the three current
   lenses become presets. Files: `shaders/fx/lens_flare.wgsl`,
   `crates/edits-render/src/{fx.rs,fxlib/generate.rs}`,
   `crates/edits-core/src/effects/light.rs`. Source: three.js
   `LensflareNode.js`, `lensDirt.js`.
6. **Star glints and light leaks.** `star_glow` (4, 6, 8 streaks, colours)
   and a procedural `light_leak`; `dissolve_*` transition presets (glow,
   flare, leak, rays, shake). Files: `fx.rs`, `passes.wgsl`,
   `shaders/fx/light_leak.wgsl`, `effects/light.rs`, `presets/*.json`.
7. **Film and look stacks.** Presets for film stocks, gate weave, halation,
   Mojo-style teal and orange, day for night, night vision, hologram,
   infinite zoom; the `echo` `channels` option (RGB time split). Files:
   `presets/*.json`, `crates/edits-core/src/effects/time.rs`,
   `crates/edits-render/src/time.rs`.
8. **Lens and rack defocus.** `lens_blur` (disc and hexagon bokeh,
   highlight boost) with an optional depth slot through `fx_aux`, and the
   `spectrum` option for chroma warps. Files: `fx.rs`, `passes.wgsl`,
   `effects/{blur,distort}.rs`, `shaders/fx/{zoom_blur,lens_distortion,turbulent_displace}.wgsl`.
   Source: Bevy `dof.wesl`, `chromatic_aberration.wesl`.
9. **Depth (3D photo parallax).** `edits depth` in `edits-analyze` (Depth
   Anything V2-Small through `ort`), a `depth_parallax` effect, depth fog and
   the depth `luma_wipe`; presets `parallax_push`, `dolly_zoom_3d`. Files:
   a new `crates/edits-analyze/src/depth.rs`, `crates/edits-cli`,
   `shaders/fx/depth_parallax.wgsl`, `effects/distort.rs`, `docs/ANALYSIS.md`.
10. **Subject cut-out.** `edits cutout` (anime-segmentation or ToonOut
    through `ort`), then `light_wrap`, aura presets on the cut-out, auto
    reframe keys. Files: `crates/edits-analyze/src/cutout.rs`,
    `crates/edits-cli`, `fx.rs`, `effects/light.rs`, `presets/*.json`,
    `docs/ANALYSIS.md`, `docs/AGENTS.md` and the MCP tool list if it gets a
    tool.
11. **Particles like Particular and Form.** Emitters from a layer's alpha or
    a path, trails, turbulence, and picture-to-particles (`progress`).
    Files: `crates/edits-render/src/particles.rs`,
    `crates/edits-core/src/model.rs`, `docs/FORMAT.md`.
12. **Suite transitions and glitches.** `luma_wipe` built-in `map`
    patterns, film roll, parallax strips, ASCII mosaic, watercolour, and
    the Wave 5 and 7 glitch items (JPEG blocks, slice shift, pixel sort).
    Files: as Waves 2, 5 and 7.
13. **AI interpolation and upscale.** `edits interpolate` (RIFE) and
    `edits upscale` (Real-ESRGAN anime model), plus Anime4K ported to WGSL
    as an `upscale` effect for low-resolution clips. Files:
    `crates/edits-analyze/src/{interpolate,upscale}.rs`,
    `crates/edits-render/src/fx.rs`, `shaders/passes.wgsl`.
14. **Materials and blob tracking.** Blobbylize, glass, plastic and mercury
    looks on alpha; the blob-tracker command. Files: `fx.rs`,
    `passes.wgsl`, `effects/stylize.rs`, `crates/edits-analyze`.
