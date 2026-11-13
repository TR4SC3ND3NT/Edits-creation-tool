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
  `threshold`, `posterize`, `grain`, and the later ones marked *display*.
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
- **`source`** names what an effect reads besides the picture: a WGSL file
  for `custom`, a `.cube` file for `lut` (both relative to the project
  file), and `"slot:<name>"`, a slot of the project, for
  `displacement_map` and (optionally) `time_displacement`. `edits check` reports a missing or unreadable file, a
  slot that does not exist, and a `source` on an effect that takes none.
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
  built-in one-pass effect below (distort, stylise, the colour passes `lut`
  to `vibrance`, and the generators) is exactly one.

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

## More distortions

Each reads the picture somewhere else; sizes are % of height, positions
composition units. Where a distortion would read outside the frame it gets
the frame's edge pixels (or, where noted, the picture mirrored at the edge).
Effects that say *moves with time* change from frame to frame even when no
value is keyed (their `speed` is in the project's time unit: per beat in a
beats project); everything else is a function of the values alone.

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `wave_warp` | `amplitude` (2), `wavelength` (10), `direction` (90), `speed` (0), `phase` (0), `seed` (0); option `shape` = `sine` / `square` / `triangle` / `sawtooth` / `circle` / `semicircle` / `uncircle` / `noise` / `smooth_noise`; option `pin` = `none` / `all` / `center` / `left` / `top` / `right` / `bottom` / `horizontal` / `vertical` | Waves travel in `direction` (degrees: 90 travels right, 0 up) and push each line across it sideways: a pixel reads the picture `amplitude` x shape(along / `wavelength` + `phase` / 360 - `speed` x t) away, a quarter turn anticlockwise from the direction of travel. `noise` is a random level per half wave (from `seed`), `smooth_noise` blends between them. `pin` fades the push to nothing toward the frame's edges (`all`, `left`, ...; `horizontal` = top and bottom, `vertical` = left and right) or its centre. *Moves with time* when `speed` is not 0. Skipped at amplitude 0. |
| `ripple` | `amplitude` (1), `wavelength` (5), `radius` (50), `center` ([0, 0]), `phase` (0), `speed` (1) | Rings about `center`: a pixel at distance d < `radius` reads the picture at d + `amplitude` x (1 - d / radius) x sin(360 x (d / `wavelength` + `phase` / 360 - `speed` x t)) degrees, so the push falls to nothing at the radius and outside it nothing moves. Rings leave the centre at `speed` per time unit. *Moves with time.* Skipped at amplitude 0. |
| `shockwave` | `progress` (0.5, 0..1), `center` ([0, 0]), `radius` (60), `width` (5), `strength` (3) | A ring `width` wide at `progress` x `radius` from `center`. Inside the ring the picture is read up to `strength` nearer the centre (a cos² bump across the ring), so it bulges outward as the ring passes; inside and outside the ring nothing moves. The push swells in over the first tenth of `progress` and fades to nothing at 1: key `progress` from 0 to 1 over a beat on a hit. Skipped at progress 0 and 1. |
| `mirror` | `center` ([0, 0]), `angle` (0) | Reflects in the line through `center` whose normal points at `angle` degrees (clockwise from right): that side shows the other side mirrored. 0 mirrors the left half onto the right, 90 the top onto the bottom. Always runs. |
| `tile` | `tiles` ([2, 2]), `mirror` (1, 0..1), `offset` ([0, 0]) | The picture repeated `tiles` times across and down, one copy centred on the frame (moved by `offset`). With `mirror` 1 neighbouring copies are mirror images, so the seams vanish. Skipped at tiles [1, 1] with no offset. |
| `polar` | `amount` (1, 0..1); option `direction` = `to_polar` / `to_rect` | Polar coordinates about the frame's centre, in a disc that touches the nearer pair of edges. `to_polar` wraps the picture into the disc: its width becomes the turn (clockwise from the top) and its bottom edge the centre, a tiny planet. `to_rect` unrolls such a disc back into the frame. `amount` moves each pixel's reading point part of the way. Skipped at amount 0. |
| `lens_distortion` | `amount` (0.2, -2..2), `center` ([0, 0]), `scale` (1) | A lens: a point at distance d from `center` reads the picture at d x (1 + k d²) / `scale`, where k makes the frame's corners (seen from its centre) read `amount` farther out. Positive is barrel distortion, negative pincushion; `scale` zooms. The frame's edges are mirrored, so no smear shows. Skipped at amount 0 and scale 1. |
| `displacement_map` | `amount` ([3, 3]); `source` = `"slot:<name>"`; options `channel_x` = `red` / `green` / `blue` / `alpha` / `luminance` / `off`, `channel_y` = `green` / `red` / `blue` / `alpha` / `luminance` / `off` | Pushes each pixel by the colour of a map: the picture of a slot of the project, stretched over the frame. A display value of 0.5 (sRGB 128) pushes nothing, 1 pushes `amount` (% of height; across, down), 0 the same the other way. `channel_x` and `channel_y` pick which map channel pushes along each axis. Skipped at amount [0, 0]. |
| `turbulent_displace` | `amount` (2), `size` (10), `complexity` (3, 1..8), `evolution` (0), `seed` (0); option `mode` = `turbulent` / `horizontal` / `vertical` / `twist` / `bulge` | Pushes the picture by fractal noise (`complexity` octaves of smooth value noise, the largest features `size` across): up to about `amount` in `turbulent` (two noises, one per axis), along one axis in `horizontal` and `vertical`, along the noise's contours in `twist` and up its slopes in `bulge`. `evolution` changes the noise smoothly (360 is one step to an unrelated noise): key it to animate. Skipped at amount 0. |

## More stylise

Effects marked *display* work on sRGB-encoded values. Ink and paper colours
are linear RGB like every colour parameter, mixed as display values.

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `screentone` | `size` (0.6), `levels` (2, 2..16), `angle` (45), `ink` ([0, 0, 0]), `paper` ([1, 1, 1]), `amount` (1); option `pattern` = `dots` / `bayer` / `lines` | *Display.* Manga screentone: the luminance is cut to `levels` tones from `ink` to `paper`, dithered by a pattern on a grid `size` % of height apart turned to `angle`: `dots` (round dots that grow into each other at 50%), `bayer` (an 8x8 ordered dither, one cell per `size`) or `lines` (hatching). With 2 levels every pixel is ink or paper; with the Bayer pattern exactly the luminance's share of each 8x8 block is paper. Skipped at amount 0 or size 0. |
| `edge_detect` | `strength` (1), `threshold` (0.1, 0..1), `width` (0.1), `color` ([1, 1, 1]), `invert` (0, 0..1), `amount` (1) | *Display.* Sobel edges of the luminance, sampled `width` % of height apart (at least a pixel); a hard step from black to white measures 1. Edges are `color` on black, scaled by (`strength` x edge - `threshold`) / (1 - `threshold`); `invert` 1 gives the negative (dark lines on white). Skipped at amount 0. |
| `ink` | `strength` (1), `threshold` (0.03), `width` (0.25), `ink` ([0, 0, 0]), `paper` ([1, 1, 1]), `amount` (1) | *Display.* Manga lineart: a pixel is ink where it is darker than its surroundings (the mean of a 5x5 grid spanning `width` % of height around it, against the mean of it and its four nearest samples) by more than `threshold`, and where it is nearly black (luminance below 0.12 to 0.25); everything else is flat `paper`. Shading and tone areas disappear, lines and solid blacks stay. Skipped at amount 0. |
| `toon` | `levels` (6, 2..64), `edge` (0.2, 0..1), `width` (0.1), `color` ([0, 0, 0]), `amount` (1) | *Display.* Cel shading: each channel rounded to `levels` evenly spaced values, and a `color` outline where the Sobel edge (as in `edge_detect`) passes `edge`; 1 draws no outlines. Skipped at amount 0. |
| `kuwahara` | `radius` (0.5, 0..5), `amount` (1) | Oil paint: each pixel takes the mean colour of whichever of the four squares meeting at it (`radius` % of height on a side) has the least variance, so flat areas smooth out and edges stay sharp. One pass with a fixed 4x4 grid of samples per square: resolution independent, but large radii are sampled coarsely (hence the limit of 5; a smooth large-radius version needs several passes). Skipped below half a pixel. |
| `emboss` | `angle` (45), `strength` (1), `height` (0.15), `amount` (1) | *Display.* Grey relief: mid grey plus `strength` x the luminance difference between the point `height` % of height away from the light and the point as far toward it. The light comes from `angle` degrees (clockwise from right), so edges facing it are lit and the far edges shaded. Skipped at amount 0. |
| `sharpen` | `amount` (0.5, 0..1) | Contrast-adaptive sharpening (AMD FSR 1 RCAS): a negative lobe on the four neighbouring pixels, as strong as `amount` allows without leaving the range of those neighbours, so soft detail gets crisper and hard edges get no halo. Works per output pixel. Skipped at 0. |
| `glitch` | `amount` (0.3, 0..1), `rate` (8), `bands` (16, 1..256), `shift` (5), `rgb` (1), `block` (0, 0..1), `seed` (0) | Digital glitch, changing `rate` times per time unit (frame t shows change floor(rate x t)): of `bands` horizontal bands, a share `amount` jumps sideways by up to `shift` % of height with red and blue split `rgb` apart, and a share `block` of square blocks (two bands tall) is read from a few blocks away with red and blue swapped. Everything is a hash of the band or block, the change and `seed`. *Moves with time.* Skipped at amount 0 and block 0. |
| `scanlines` | `count` (300), `amount` (0.3, 0..1), `speed` (0) | CRT lines: multiplies by 1 - `amount` x (0.5 + 0.5 cos(360 x (y x `count` + scroll))), y from 0 at the top to 1 at the bottom, scrolling `speed` line spacings per time unit. Where lines would be closer than 2 pixels (a small preview) they fade into an even darkening by `amount` / 2, the same mean. Skipped at amount 0. |
| `vhs` | `noise` (0.2, 0..1), `roll` (0), `bleed` (0.5), `wobble` (0.3), `seed` (0) | A worn tape: the picture rolls up by `roll` frame heights per time unit, its lines wobble sideways by up to `wobble` % of height, colour (not luminance) is smeared `bleed` % of height to the right, and static, a drifting torn tracking band and a few bright streaks scale with `noise`. Static changes every output frame. *Moves with time.* Skipped when all four are 0. |
| `film_damage` | `scratches` (0.3, 0..1), `dust` (0.3, 0..1), `flicker` (0.1, 0..1), `jitter` (0.2), `seed` (0) | Old film, new on every output frame: the picture jumps by up to `jitter` % of height (mirrored at the edges), its brightness flickers by up to ±`flicker` / 2, up to five thin wandering vertical scratches (bright or dark) appear with chance `scratches`, and dark dust specks scatter with density `dust`. All hashed from the frame number and `seed`. *Moves with time.* Skipped when all four are 0. |
| `film_burn` | `amount` (0.5, 0..1), `color` ([1, 0.45, 0.1]), `center` ([0, 0]), `size` (30), `speed` (0.5), `seed` (0) | A burn spreading from `center`: fractal noise (features `size` % of height) plus the closeness to the centre is compared with a threshold that falls as `amount` rises. Past it the picture chars dark, then glows `color`, then burns through to white-hot light (values above 1, so a `glow` after it blooms). 0.1 barely starts, 0.5 takes about half the frame, 1 all of it; key `amount` for a burn transition. The ragged edge churns at `speed` per time unit. *Moves with time.* Skipped at amount 0. |

## Colour passes

Colour effects that need a pass of their own (they are not folded into the
blend like the colour effects above).

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `lut` | `amount` (1, 0..1); `source` = a `.cube` file | *Display.* A colour lookup table: `source` is a `.cube` file (the Adobe / Resolve text format: `LUT_3D_SIZE` up to 256 and/or `LUT_1D_SIZE`, `DOMAIN_MIN` / `DOMAIN_MAX`, `TITLE`, `#` comments) relative to the project. It is applied to display values clamped to 0..1, with trilinear interpolation, and `amount` mixes with the original as display values (so a table that inverts equals `invert` at any amount). A 1D table, or a domain other than 0..1, is baked into a 33³ table when the project is loaded. `edits check` reports a file that does not load and why. Skipped at amount 0. |
| `gradient_map` | `shadows` ([0, 0, 0]), `midtones` ([0.214, 0.214, 0.214]), `highlights` ([1, 1, 1]), `amount` (1); option `mode` = `tritone` / `duotone` | *Display.* Recolours by luminance: black becomes `shadows`, mid grey `midtones` and white `highlights` (`duotone`: shadows straight to highlights), mixed as display values. The defaults are a black-and-white conversion. Skipped at amount 0. |
| `levels` | `in_black` (0), `in_white` (1), `gamma` (1, 0.01..10), `out_black` (0), `out_white` (1) | *Display.* Per channel: (value - `in_black`) / (`in_white` - `in_black`), clamped to 0..1, raised to 1 / `gamma`, then mapped to `out_black`..`out_white`. Values above 1 (a flash) are clipped. Skipped at the defaults. |
| `vibrance` | `amount` (0.5, -1..1) | Saturation (about the luminance, as `saturation`) by a factor that depends on how saturated the colour already is: 1 + 2 x `amount` x (1 - s)², where s = (max - min) / max, so grey-ish colours are boosted up to three times and pure ones not at all. Negative amounts mute every colour evenly (-1 is black and white). Skipped at 0. |

## Generate

Generators draw new content over the picture they are given, inside its
transparency: on a solid layer they fill the solid, on an adjustment layer
they cover what is beneath, on a picture layer only the picture. The option
`blend` = `replace` / `multiply` / `screen` / `add` / `overlay` says how,
and `amount` is the opacity.

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `fractal_noise` | `scale` (20), `complexity` (4, 1..8), `evolution` (0), `contrast` (1), `brightness` (0), `offset` ([0, 0]), `seed` (0), `amount` (1); option `blend` | Grey fractal noise (clouds, smoke): `complexity` octaves of smooth value noise, each half the size and half the weight of the last, the largest `scale` % of height across. The grey value is (noise - 0.5) x 2 x `contrast` + 0.5 + `brightness` as a display value, clamped to 0..1. `evolution` changes it smoothly (360 is one step); `offset` moves it. Skipped at amount 0. |
| `gradient_ramp` | `start` ([0, -0.5]), `end` ([0, 0.5]), `color_a` ([0, 0, 0]), `color_b` ([1, 1, 1]), `amount` (1); option `shape` = `linear` / `radial`; option `blend` | Two colours mixed as display values: `linear` along the line from `start` to `end` (constant across it, held beyond the ends), `radial` by the distance from `start`, reaching `color_b` at the distance of `end`. The default is black at the top edge to white at the bottom. Skipped at amount 0. |

The built-in one-pass effects (the distort and stylise tables except
`transform2d` and `rgb_split`, the colour passes and the generators) are WGSL
files in `crates/edits-render/src/shaders/fx/`, written against the same
contract as a custom effect (below) plus shared helpers
(`shaders/fx/lib.wgsl`: sRGB transfer, fractal noise, generator blends), with
their options compiled in. To add one: a row in the category's file in
`crates/edits-core/src/effects/`, a WGSL file, a packer in
`crates/edits-render/src/fxlib/` (it turns the effect's values into `fx.p0`
.. `fx.p15` and says when it changes nothing), a pixel test in
`crates/edits-render/tests/` (`effects.rs`, or `fx_distort.rs`,
`fx_stylize.rs`, `fx_color.rs` by category), and a line here. Ported code
keeps its licence header and gets an entry in `THIRD_PARTY.md`.

## Time

Time effects show the layer at other times. They keep no memory of earlier
frames (a frame is still a function of the project and the frame number):
each other time is **rendered again**. On a picture, solid, text, video,
particles or comp layer that is the layer as it is at that time (its
position, keys, video frame, particles, comp picture) with its masks and the
effects *before* the time effect in its list, at that time; opacity, blend
mode, transition and track matte are not part of it (they apply once, to the
result). On an adjustment layer it is every visible layer beneath it at that
time, over the background, then the adjustment's effects before this one. A
time where the layer is not active (before its `in`, after its `out`) adds
nothing. Times are in the project's time base (beats or seconds).

| Kind | Parameters (default) | What it does |
| --- | --- | --- |
| `echo` | `count` (3, 0..16), `interval` (-0.125), `decay` (0.6, 0..1), `start` (1, 0..1); option `mode` = `add` / `screen` / `max` / `min` / `over_back` / `over_front` / `average` | Picture i (0 is the current one, 1..`count` the echoes) is the input at t + i x `interval`, weighted `start` x `decay`^i (colour and alpha alike: a weaker echo is fainter, not darker). They are combined from the current picture outward: `add` sums them (no normalising: 3 echoes at decay 1 are 4 times as bright), `screen` sums without passing white, `max` / `min` keep the brightest / darkest of each channel, `over_back` puts each echo behind the ones before it (the current picture stays in front: trails behind a moving cut-out), `over_front` puts each echo over them, `average` divides the sum by the sum of the weights (never brightens: good on a full-frame picture or an adjustment layer). Alpha is clamped to 1. `count` 0, `decay` 0 (with `start` 1) and `enabled: false` plan nothing and cost nothing; an `interval` of 0 reuses the picture itself. |
| `time_displacement` | `max` (-0.25), `levels` (16, 2..16); option `neutral` = `grey` / `black`; optional `source` = `"slot:<name>"` | Each pixel shows the input at a time shifted by the brightness of a map: the picture of a slot stretched over the frame, or, without `source`, the layer's own picture (its luminance, display-encoded). With `neutral` `grey` a map value of 0.5 shifts nothing, white shifts by `max` and black by -`max` (as in After Effects); with `black`, black shifts nothing and white by `max` (a one-sided slit scan: a ramp map makes the frame lag from one side to the other). The shifts are rounded to `levels` evenly spaced times, each rendered once and picked per pixel; a transparent map pixel is neutral. Skipped at `max` 0. |

**Cost.** Each echo and each slice is a full render of the layer (or, on an
adjustment layer, of everything beneath it), so an `echo` with `count` 3
costs about four times its layer, a `time_displacement` with 16 `levels` up
to sixteen times. Motion blur multiplies: every motion blur sample renders
its echoes again, so 8 samples x 4 pictures is 32 renders of the layer per
frame. Instants at which the layer and every time it echoes are the same are
still rendered once. Time effects can nest (an echo of a layer whose effects
before it echo, an echo inside a comp under an adjustment echo); each level
multiplies, and more than 4 levels is refused with an error. Use a small
`count`, a small `levels`, and key `count` or `decay` to 0 between hits
(the `echo_hit` preset does) so the cost is paid only where the effect shows.
`posterize time` is not an effect: a layer's `fps` field holds its frames
(see [FORMAT.md](FORMAT.md#video-layers-and-layer-time)).

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
