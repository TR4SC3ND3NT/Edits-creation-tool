# Calibration and automatic fitting (`edits-calib`)

`edits-analyze` measures the recipe of a finished edit; `draft` turns the
measurements into a project; `compare` scores a render against the
reference in signature space. Each of those steps can be wrong in ways that
no single test shows. `crates/edits-calib` checks them with the renderer as
ground truth, and closes the loop for a real project with `fit`.

- **The harness** generates random recipes of this genre with known
  parameters, renders them, measures the render, and scores every estimator
  against the truth. It then drafts a project from the analysis, renders the
  draft and compares it with the analysis it came from (the *draft round
  trip*). The invariant it holds the code to: *analysing a render of a
  recipe returns that recipe within stated tolerances, and a project drafted
  from an analysis, rendered and analysed, reproduces the analysis.*
- **`fit`** adjusts the magnitudes of a drafted project (flash peaks and
  decay rates, blur radii, saturation, zoom and slide amounts) until its
  render measures like the reference, shot by shot. The pictures in the
  slots do not have to be the reference's.

Nothing here reads reference footage: the pictures are generated in code
(`pictures.rs`: flat poster art, ink line art, dark pictures, smooth
photographic texture), and so are the overlay plate and the click track.

## Running the harness

It is a library (`edits_calib::harness::run`) with an example binary in
front of it. Build in release; on 4 cores with the software rasteriser
(Mesa lavapipe, no GPU) a case of 6-8 shots at 320 px takes 8-15 s.

```
cargo run --release -p edits-calib --example calib -- run --cases 36 --height 320 --degrade 1500 --out calib/
cargo run --release -p edits-calib --example calib -- case calib/cases/case_20.json    # replay one case in detail
EDITS_CALIB_FRAMES=1 ...  case ...     # add a per-frame table: truth next to the measurement
EDITS_CALIB_KEEP=1   ...  case ...     # keep the case folder: the draft and both analyses of its round trip
cargo run --release -p edits-calib --example calib -- blur --height 320 [--degrade 1500]   # measured vs rendered Gaussian blur
cargo run --release -p edits-calib --example calib -- sig analysis/reference_analysis.json render.mp4   # both signatures side by side
```

`run` writes `report.txt` (the table below), `report.json` (the same,
machine-readable, schema `edits.calibration_report/1`) and the worst cases
as `cases/case_<seed>.json`, which `case` replays deterministically. With
`--degrade KBPS` every case is also scored after an H.264 round trip at
that bitrate (what a sharing platform does to an upload). 36 cases with and
without degradation and with the draft round trip: 7392 frames, about 7
minutes. `calib/` is ignored by git.

A case is 6-8 shots at 100-150 bpm, 25 or 30 fps, square, 4:5 or 16:9.
Each shot may get a punch-in (decay, `out` or `expo_out`), a slide, a slow
push or a punch with a turn; a Gaussian blur that clears over 3-8 frames; a
saturation, inversion or threshold hit; one or two exposure flashes (1-5
stops, decay rate 0.15-0.6); and the whole edit may have shake, a screen
overlay plate and a vignette. Flashes are keyed on beats, so, as in real
edits, the first frame that shows a flash usually starts a fraction of a
frame after its key and shows a decayed value; the truth is what each frame
shows (read back with `edits_core::Evaluator`, the renderer's own code).

## What is scored

Per frame, relative to the analysis's reference frame of the shot (as the
analysis reports it): exposure in stops, blur in % of height, saturation
ratio, scale, position and rotation. Per event: cuts, flash frames, peaks
and decay rates, saturation, inversion and threshold hits (matched within
one frame). Per clip: tempo, beat phase, vignette, overlay, slots. Per
draft round trip: the `compare` scores. Tolerances are in
`crates/edits-calib/src/report.rs` (`TOLERANCES`); the main ones:

| Estimator | Tolerance |
| --- | --- |
| `flash.peak` | 0.5 stops |
| `flash.rate` | 0.1 per frame |
| `exposure.curve` / `exposure.rest` | 0.4 / 0.15 stops |
| `saturation.amount`, `saturation.curve` | 0.15 |
| `saturation.rest` / `saturation.rest_flash` | 0.1 / 0.25 (truth 1; the second under a flash over 1 stop) |
| `blur.radius` | 20% of the truth or 0.15% of height |
| `blur.rest` | 0.1% of height |
| `scale` / `position` / `rotation` | 0.01x / 0.005 frame heights / 0.3 degrees |
| `draft.*` | 0.1-0.2 below a perfect score |

## Accuracy, before and after

36 cases from seed 1 at 320 px, the same cases before (the code as the
previous agent left it, with the lead's two fixes) and after the changes
below. *Bias* is the mean of estimate minus truth (for blur, as a fraction
of the truth), *in tol* the share of samples within tolerance.

| Estimator | n | clean before: bias, in tol | clean after | H.264 1.5 Mbit/s before | H.264 after |
| --- | --- | --- | --- | --- | --- |
| cut frame (recall / precision) | 226 | 99% / 99% | 99% / 99% | 99% / 99% | 99% / 99% |
| flash peak, stops | 209 | +0.006, 98% | +0.004, 98% | +0.054, 98% | +0.047, 98% |
| flash decay rate | 198 | -0.008, 95% | -0.010, 94% | -0.017, 94% | -0.020, 93% |
| exposure per frame, stops | 909 | -0.012, 99% | -0.013, 99% | +0.021, 99% | +0.017, 99% |
| saturation hit amount | 25 | 0.000, 100% | 0.000, 100% | 0.000, 100% | +0.004, 100% |
| saturation under a flash (truth 1) | 427 | +0.007, 97% | -0.001, 100% | **-0.098, 84%** | **-0.013, 99%** |
| saturation hits (recall) | 30 | 87% | 83% | 87% | 80% |
| blur radius (relative) | 193 | **-12.8%, 86%** | **-9.2%, 90%** | **-25.8%, 72%** | **-17.6%, 79%** |
| blur radius, photo pictures | 60 | -25.9% | -17.8% | -58.3% | -40.7% |
| blur where there is none, % | 2159 | +0.004, 99% | +0.003, 99% | +0.004, 99% | +0.004, 99% |
| scale | 3339 | 0.000, 99% | 0.000, 99% | 0.000, 99% | 0.000, 99% |
| position x / y | 3339 | 0.000, 99% / 100% | same | same | same |
| threshold level | 28 | +0.019, 89% | +0.019, 89% | +0.012, 89% | +0.012, 89% |
| vignette falloff curve (largest gain error) | 17 | +0.24, 12% | unchanged | +0.23, 6% | unchanged |
| **draft round trip, overall** | 36 | **-0.118, 47%** | **-0.042, 92%** | | |
| draft round trip, exposure | 36 | -0.165, 44% | -0.039, 97% | | |
| draft round trip, saturation | 36 | -0.172, 50% | -0.063, 94% | | |
| draft round trip, flashes | 36 | -0.109, 86% | -0.007, 100% | | |
| draft round trip, blur | 36 | -0.142, 86% | -0.080, 94% | | |
| draft round trip, scale | 36 | -0.110, 83% | -0.040, 100% | | |

The estimators were already close to the truth on clean renders; the large
errors were in the *mapping* from measurements to a project (the draft
round trip lost 0.12 of the score on its own), in saturation after a codec,
and in blur.

## What was fixed, and why

Each fix is a reading of the renderer's real formulas
(`crates/edits-render/src/compose.rs`, `fx.rs`, `edits-core/src/ease.rs`),
not a fudge factor:

- **Exposure** is `c * 2^stops` on linear RGB, clipped at 1 on output; the
  estimator reads the gain from quantiles of the largest channel below
  white, which is exact for that formula. The decay is the `decay` ease,
  `(k^j - k^N) / (1 - k^N)` with `k = 1 - rate`, fitted as such. No table
  is needed: bias is under 0.05 stops clean and the decay rate under 0.02.
- **Draft timing.** The draft used to snap a key to the nearest quarter beat
  whenever the renderer would show it within a frame either way, so a flash
  or a punch could appear a frame early or late (on the real reference, flashes
  or punches on at least 3 of its 17 shots). Keys are now snapped only when the renderer shows them on the
  frame they were measured on.
- **Draft values.** A key snapped to a quarter beat can sit up to a frame
  before its frame, and a `decay` segment has moved a lot by then (rate 0.5,
  0.8 of a frame early: 57% of the key is left). The draft now sets each
  front-loaded key (`decay`, `out`, `quad_out`, `quint_out`, `expo_out`) that
  nothing ramps into to `(v - e * next) / (1 - e)`, where `e` is the eased
  fraction the renderer reaches at the frame, so the rendered curve passes
  through the measured value. This is the largest single gain (draft
  round trip 0.88 -> 0.96).
- **Saturation** is `mix(luma(c), c, amount)` in linear light, which
  multiplies chroma (largest minus smallest channel) by `amount` and keeps
  luminance: the measured chroma/luminance ratio *is* `saturation.amount`,
  whatever the exposure. What broke it: (1) after H.264 the grey of
  flash-clipped areas bleeds into neighbouring blocks (4:2:0 colour); blocks
  next to a block the flash clipped are now left out (-0.098 -> -0.013 under
  a flash after H.264). (2) Under a flash that clips most of the frame the
  few remaining blocks give ratios between 0 and the clamp at 4; the
  signature now records the neutral 1 where under 6% of the blocks are
  measurable. (3) A one-frame grey reading on a flash peak over 1.5 stops is
  no longer a saturation hit: whether it appears depends on the picture.
  (4) The reference frame of a shot is chosen against the most colourful
  frames (85th percentile), not the median, so a grey hit covering half the
  shot cannot become the reference (every coloured frame then read 3-4x).
  (5) Colour that moves outside hits (the real reference drains colour on
  each flash and returns it stronger as the flash decays) is drafted as a
  fitted `saturation.amount` curve, which the renderer reproduces exactly.
- **Blur** is a Gaussian of sigma `radius / 100 * height` per channel in
  linear light (halved copies above 2.5 texels; that adds about 2% to sigma,
  inside the tolerance). (1) Rendering, quantisation and the codec put a
  noise floor under both spectra, which flattens the ratio at the top of the
  fitted range and reads low on smooth pictures; the floor (median power
  above 0.42 cycles per pixel of the blurred spectrum) is now subtracted.
  (2) Whether the reference frame is warped onto the frame (needed whenever
  the picture zooms) was decided by a share that counted overlay strokes as
  missing, so in any edit with an overlay plate the warp and every
  refinement under a flash were off; the decision now uses the warp's
  coverage. On case 25 of the harness that took blur from -73% to -9%.
- **Scale and position** were already exact relative to the reference frame
  (bias under 0.001); the draft multiplies scale by the cover factor and
  now keys it on the right frame with the right value.

No calibration table is shipped: where an estimator is still biased (blur
after H.264, blur under a flash that clips a smooth picture), the bias is a
loss of information, not a monotone distortion (a frame whose detail the
codec removed reads 0 whatever its blur), and a table would only move the
error elsewhere. `fit` handles what is left, because it compares
measurements made by the same estimator on both sides.

`crates/edits-calib/tests/roundtrip.rs` turns three of the hardest cases
into a regression test (27 s in release, about a minute in debug) with the
tolerances above and required shares: cuts and flash frames 100%, flash
peaks 90%, rates 85%, exposure 90-95%, saturation 95-100%, blur 55% (one of
the three cases is the known limit below), scale 95%, draft round trip
within 0.1 per category.

## `fit`

```rust
use edits_calib::fit::{fit, fit_file, FitOpts, FitReport};
let report: FitReport = fit_file(Path::new("project.json"), Path::new("analysis/reference_analysis.json"), &FitOpts::default())?;
```

or `cargo run --release -p edits-calib --example calib -- fit project.json analysis/reference_analysis.json [--dry-run] [--budget 24] [--shots 3,4] [--params exposure,blur] [--json fit.json]`.

`FitOpts`: `shots` (reference shot numbers; empty = all), `params`
(`["auto"]` or groups: `exposure`, `exposure_decay`, `saturation`,
`threshold`, `blur`, `scale`, `scale_decay`, `position`, `position_decay`),
`budget` (renders per shot, default 24), `scale` (render scale; default:
the reference's analysis height), `dry_run`, `seed`.

What it does:

1. Renders the reference's analysed frame range of the project in-process at
   the reference's analysis height (one motion-blur sample), analyses it
   exactly as `compare` does and keeps that comparison as *before*. The
   overlay mask, vignette and saturation level of this render are kept for
   the per-shot measurements.
2. For each reference shot, finds the shot layer on screen at its reference
   frame and collects that layer's numbers: exposure key values other than 0
   and `decay` rates, saturation values other than 1, threshold level,
   Gaussian radii other than 0, scale key values other than the resting one
   (never below it, so the frame stays covered) and their decay rates,
   position key values other than the resting one and their decay rates.
   Bounds: stops +-2 (within -3..8), rates 0.03-0.95, saturation 0-2, radius
   0 to 3x, scale up to 2x the punch.
3. Renders only that shot's frames (plus the one before the cut), measures
   them with `pipeline::measure_shot` (the same stages, on the same cut) and
   scores them against the reference shot with `compare_analyses` on the
   shot alone. The number minimised is the sum of the curve distances
   (exposure, blur, saturation, scale, and a position distance with a floor
   of 0.01 frame heights) plus twice the shortfall of the flash and tone-hit
   scores.
4. Searches with a bounded coordinate search on parameters scaled to 0..1:
   steps start at 0.2 of the range, grow 1.5x after a success and shrink
   0.4x after a failure in both directions; the visiting order of each sweep
   is a shuffle from `seed`. Deterministic: same project, same result.
5. Writes the best values back (rounded: stops 0.01, rates and amounts
   0.001, scale and position 0.0001), also into the loop repeats of the
   layer (`<id>_r2`, ...) where they held the same value, renders the whole
   range again for the *after* comparison, and writes the file (formatted
   as `edits` writes projects) only if the whole-clip score did not get
   worse.

What it leaves alone: key times and easing names (they come from the beat
grid and the analysis), slot paths, layer order and in/out points, the
overlay and vignette layers, inversion and threshold amounts (on/off
toggles), rotation, shake, cubes and anything not on a shot's own layer. It
cannot add what the draft does not have: a flash the analysis missed, a
darkening the draft has no key for.

`FitReport` (serde, schema `edits.fit_report/1`): `before`/`after` whole-clip
scores and `overall_before`/`overall_after` (the in-process render has no
audio, so no tempo score), per shot the objective and the shot's own
`compare` scores before and after, every change as layer id, JSON pointer,
group, old and new value, `renders` (shot renders plus the two whole-clip
renders), `frames_rendered`, `seconds`, `written`, `notes`.

Tested in `crates/edits-calib/tests/fit.rs`: a harness case is rendered and
analysed as the reference, its flash peaks are lowered by 1.2 stops, decay
rates raised by 0.2, blur radii cut to 40% and saturation hits raised by
0.45, and `fit` is run against the reference analysis. With the reference's
own pictures the mean errors fall from 1.2 stops / 0.2 / 60% / 0.45 to
0.16 / 0.016 / 2% / 0; with four unrelated generated pictures in the slots,
to 0.28 / 0.06 / 2% / 0. Each test takes about 35-50 s in release (8 shots
at 160 px, budget 20).

## The real loop

A real 16-beat reference (1080 x 1080, 25 fps, 207 frames analysed, 17
shots, 40 flashes) was analysed, drafted without proxies, its 17 slots
filled with the four unrelated pictures of `edits demo`, rendered at
`--scale 0.5 --samples 1` and compared (`edits-analyze compare`):

| Stage | overall | tempo | cuts | flashes | tone hits | exposure | blur | saturation | scale |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| First run (reported before this work) | 0.67 | 1.00 | 0.94 | 0.72 | 0.80 | 0.40 | 0.54 | 0.28 | 0.66 |
| Same code, re-run for this comparison | 0.65 | 1.00 | 1.00 | 0.83 | 0.80 | 0.41 | 0.51 | 0.11 | 0.51 |
| After the estimator and draft fixes | 0.69 | 1.00 | 0.94 | 0.79 | 0.50 | 0.56 | 0.50 | 0.45 | 0.78 |
| After `fit` (budget 24) | **0.76** | 1.00 | 0.97 | 0.83 | 0.78 | 0.60 | 0.56 | 0.51 | 0.81 |

The fixes moved exposure (+0.15), saturation (+0.34) and zoom (+0.27):
flashes and punches now land on their frame with their measured value,
and the reference's colour curve is drafted. Tone hits fell (0.80 -> 0.50):
the re-analysed reference now has 9 hits instead of 12 (one-frame greys on
flash peaks are no longer hits) and the drafted colour curve dips into
extra hits on the unrelated pictures. `fit` then recovered tone hits and
gained on every curve category: 0.69 -> 0.76 overall, in 410 shot renders
(5502 frames at 360 px) and 343 s on 4 CPU cores without a GPU. Its own
in-process estimate (no audio, no H.264) was 0.620 -> 0.755.

Commands (working files outside the repository):

```
edits-analyze analyze ref.mp4 -o an
edits-analyze draft an/reference_analysis.json -o project.json      # no --proxies
# point the 17 slots at demo/img/{a,b,c,d}.png (edits demo demo)
edits render project.json -o render.mp4 --scale 0.5 --samples 1
edits-analyze compare an/reference_analysis.json render.mp4
cargo run --release -p edits-calib --example calib -- fit project.json an/reference_analysis.json
edits render project.json -o render_fit.mp4 --scale 0.5 --samples 1
edits-analyze compare an/reference_analysis.json render_fit.mp4
```

## Limits that remain

- **Blur under a flash that clips a smooth picture** reads low: clip
  boundaries are sharp whatever the blur and the unclipped tones barely
  change with it (one harness case: 0.26% for a true 2.78%). Photographic
  pictures after H.264 lose the detail blur is read from (small blurs read
  0 below about 1.5% of height at 320 px).
- **Large blurs** (above about 3% of height at 320 px) read 5-15% low: the
  analysis window has few frequency bins below the blur's cut-off.
- **Blur kind is not classified**: directional, zoom and spin blur are
  flagged anisotropic and drafted as a Gaussian of the same equivalent
  sigma.
- **Cube pose and 3D turns are not measured** (shots flagged `needs_agent`
  get a plain layer); `fit` does not touch rotation.
- **Static grades are unidentifiable**: a constant saturation, exposure or
  tint over a whole shot is indistinguishable from the picture itself;
  everything is relative to the shot's reference frame.
- **The vignette fit is weak** (largest gain error within 0.08 in only 12%
  of cases): it is fitted on flash frames, whose falloff is not the
  vignette's alone. It does not enter `compare`.
- **Flashes that are not a gain.** The real reference's flashes darken
  below the starting level after the peak and change colour along the way;
  the draft models a flash as a gain that decays to 0 and colour as a
  separate curve, so negative exposure dips are not drafted and `fit` can
  only trade them off.
- **Content dependence.** A gain flash of 6 stops on a bright picture is a
  white frame; on the reference it was not. Cut detection then sees a cut
  that is not there (the real loop's one extra cut). `fit` lowers such a
  flash to match the signature, but the measured peak of a fully clipped
  frame is only a lower bound on either side.
- **Cost.** `fit` renders each shot `budget` times; the real 17-shot,
  207-frame edit takes about 6 minutes (343 s, 410 shot renders) on 4 cores. Shots with 20-30 parameters
  get fewer than two visits per parameter at the default budget.
- Only one real reference was available for validation; the numbers above
  are for that clip with the demo's pictures. The harness's pictures and
  recipes are generated: real art (gradients, faces, text, grain) and real
  effects (glow, light leaks, additive flashes) are not in it.
