# Reference analysis (`edits-analyze`)

Given a finished edit, `edits-analyze` measures its **recipe**: timing, cuts,
motion and easing, flashes, tone hits, blur, static layers. Nothing it measures
depends on which pictures are in the edit, so the same recipe can be replayed
with other pictures, and a render can be compared with the reference without
comparing pixels.

The trick used throughout: inside a shot the picture is unknown but constant.
Every frame is measured **relative to one reference frame of its shot** (sharp,
not flashed), so the picture cancels and what is left is the effect.

```
edits-analyze analyze  ref.mp4 -o analysis/            # reference_analysis.json, overlay.png, proxies/
edits-analyze draft    analysis/reference_analysis.json -o project.json [--proxies] [--width W --height H] [--no-snap] [--single-loop]
edits-analyze compare  analysis/reference_analysis.json render.mp4 [--json] [--top N]
edits-analyze probe    ref.mp4 [--json]                # per-frame luminance, saturation, sharpness, difference
```

`analyze` options: `--height 360` (analysis resolution), `--bpm`, `--beat-offset`
(seconds to beat 0), `--whole` (do not stop at one loop period), `--samples`
(keep raw per-frame track samples). Set `EDITS_ANALYZE_TIMING=1` for stage timings.

The library also analyses a song for cutting to it (`song::analyze_file`,
behind `edits audio`; see [CLI.md](CLI.md#audio)): from one short-time
Fourier transform it takes the spectral flux of the whole spectrum (the same
onset envelope the reference analysis uses for the tempo) and of three bands
(30–150, 200–3500 and 6000–11000 Hz). The tempo is estimated on the bands
added with equal weight (busy hats otherwise pull it to 2/3 or 3/2), the bar
phase from where the kicks, backbeat snares and changes of loudness fall,
onsets by peak picking each band, sections from changes of loudness per bar,
and the drop as the largest rise. `cargo test -p edits-analyze song` runs it
on synthetic songs with known tempo, bars, onsets and drop.

Needs `ffmpeg` and `ffprobe` on `PATH` (used as subprocesses). The library
(`crates/edits-analyze`) exposes every stage as a plain function returning
serialisable structs: `pipeline::analyze_video`, `pipeline::analyze_frames`,
`draft::draft`, `compare::compare`, `compare::compare_analyses`,
`signals::signatures`, `pipeline::measure_shot` (one shot on its own,
given a `pipeline::ShotContext` from a whole-clip analysis: what
`edits-calib`'s `fit` uses), and the per-stage modules below.
Accuracy per estimator, measured against generated recipes with known
parameters, and automatic fitting of a draft are in [CALIBRATION.md](CALIBRATION.md).

## Stages

| Stage (module) | What it measures | How | Known failure modes |
| --- | --- | --- | --- |
| ingest (`ingest`) | Black lead-in, solid or fading tail, stalled frames, loop | A frame is *blank* when its spatial contrast is below 2/255. A *stall* is a frame whose difference to its predecessor is under 30% of the median difference of its neighbours. Loop: the lag at which 12x12 z-normalised frame descriptors match again (correlation >= 0.8); the shortest such lag is the period. | Loops shorter than 1.5 s are not looked for. A loop whose passes differ (different flash, different picture) is not found. Stall cadence is reported only when it is regular. |
| beats (`beats`) | Tempo, beat phase, grid | Spectral flux of the audio (1024/256 STFT, log magnitude), autocorrelation with a broad prior around 120 BPM over 60-200 BPM, then period and phase refined together on a beat comb. A whole-number tempo is taken when it explains the envelope within 3%. Beat 0 is the grid beat nearest the first analysed frame; the phase is then nudged by at most one frame so that cuts keyed on the grid land on the frames where the reference shows them. Without usable audio the frame-difference signal is used (`source: "visual"`). | Octave errors on unusual music (candidates are listed). A clip of N seconds resolves tempo to roughly 5/N BPM. The grid is uniform: an edit cut on a different frame rate and converted can sit 1-2 frames off it (reported per cut). |
| shots (`shots`) | Cuts | 64 px thumbnails, band-passed, compared as z-normalised correlation after a search over shift (+-9 px) and zoom (up to 1.33x). A flash is affine in intensity, so it cancels; a punch-in is found by the search; a tone inversion gives a strong *negative* correlation and is kept as an invert toggle, not a cut. A cut needs similarity < 0.6 and the old picture not coming back within two frames. Among neighbouring candidates the frame where brightness jumps up wins (a flash peaks on its cut). | Fast 3D turns, whip blurs and picture-in-picture changes look like cuts or hide them. A cut between two shots of the same picture at a similar size is not a cut for this stage. Dissolves are not modelled. |
| reference frame, slots (`pipeline`) | One frame per shot to measure against; which shots share a picture | Sharpest frame that is not brighter than the shot's 60th percentile, not inverted, at least 0.6x as colourful as the shot's most colourful frames (85th percentile, so a grey hit covering half the shot cannot become the reference), preferring the stillest. Slots: reference frames that register to each other (correlation >= 0.38 after a coarse search over scale and shift) share a slot. Proxy still = the reference frame at full resolution. | A shot that is flashed or blurred throughout has no good reference frame. The same picture at very different crops, or on a cube face, lands in separate slots. Proxies contain the overlay and vignette. |
| layers (`layers`) | Static overlay plate, vignette | Overlay: per-pixel temporal median over the loop, white top-hat (median minus its morphological opening), part common to R, G and B, divided by `1 - background` (undo a screen blend). Vignette: radial profile, in linear light, of the upper quartile of the brightest frames (flashes are close to a white field); fitted with `1 - amount * smoothstep(radius, radius + softness, r)`, `r` in half-diagonals. | Needs at least 3 shots. Only thin bright strokes are recovered; a soft texture, a dark overlay or grain is not. Blend mode and opacity are a guess. The vignette profile is that of the flash frames, which may fall off more strongly than ordinary frames; without near-white frames it is only qualitative. The renderer's vignette formula is assumed, not verified. |
| track (`track`) | Scale, rotation, translation of every frame relative to the reference frame | Coarse-to-fine inverse-compositional Gauss-Newton (Lucas-Kanade) on locally normalised gradient magnitude, zero-mean normalised cost (as in ECC): invariant to gain, offset and inversion. Central mask without the overlay strokes. Warm start from the neighbouring frame, retry from an exhaustive coarse search, then frame-to-frame chaining. Frames below correlation 0.3 are marked `bridged` and interpolated. A free affine fit is run as a check: if it is clearly better with unequal scale factors the frame is *non-similarity*. | Flash peaks and heavy blur frames do not register (bridged; a move that happens entirely under a flash is not seen). 3D tilts, cube turns and inset panels are flagged `needs_agent`, not measured. Flat or tiny pictures have too little texture. Absolute scale is unknowable (see identifiability). |
| exposure (`exposure`) | Gain per frame in stops; flash events with peak and decay | Quantiles of linear-light luminance of the frame against those of the warped reference frame; the gain is the median ratio over quantiles not clipped at white (a pixel as bright as the white field at its position counts as clipped). Flash = local maximum >= 0.8 stops; decay = log-linear fit of the stops after the peak. For shots without a track, neighbouring frames are compared and accumulated. | When most of the frame is clipped the peak is a **lower bound** (flagged, low confidence). A flash made by adding white or glow is not a pure gain; an affine fit flags `fade` events but the split is weak. Chained estimates drift. |
| tone (`tone`) | Saturation, inversion, threshold, posterise hits | Saturation: chroma/luminance in linear light on block means, ratio to the reference frame; blocks that are clipped, and blocks next to one the flash clipped (codecs bleed its grey into them), are left out. Where under 6% of the blocks remain, the signature records the neutral 1. A grey frame on a flash peak over 1.5 stops is part of the flash, not a hit. Inversion: negative luminance correlation with the registered reference frame, or frame-to-frame sign flips. Threshold: the quantile-matched tone curve is a step and darks are crushed, not lifted. Posterise: channel values sit in few levels. | Under a flash colour washes out anyway; one-frame hits on flash peaks are not reported, so a real one-frame hit on a strong flash is lost. A black-and-white reference frame makes saturation unmeasurable (`needs_source`). A very strong flash and a threshold are the same thing on ink-on-paper art. Posterise is a weak flag. |
| blur (`blur`) | Gaussian-equivalent sigma per frame | Radially averaged power spectrum of a centred Hann window, noise floor (median power above 0.42 cycles/pixel) taken off both, ratio frame/reference; `log ratio = c - 4 pi^2 sigma^2 f^2` fitted where the blurred signal is above the noise. The reference frame is warped onto the frame when the warp covers the window (overlay strokes are filled in). Under a flash the reference is put through blur, gain and clip and the estimate refined against it. Per-orientation fits give an anisotropy index. | Directional, zoom and spin blur are not classified: flagged anisotropic, radius is an equivalent. Motion blur of a fast move reads as blur. Blur below about 0.1% of height is noise. Unreliable in shots without a track (the reference frame shows something else). Reads low under a flash that clips a smooth picture, after a codec on smooth pictures (small blurs read 0), and 5-15% low above about 3% of height (see CALIBRATION.md). |
| curves (`curves`) | Keyframes and easing for each track; shake | Dynamic programme over break points; each segment fitted with `hold`, `linear`, `out`, `in_out`, `in`, `expo_out` and `decay` (rate by golden-section search) through `edits_core::ease::apply`, penalty per segment, joint solve of key values. Reports segment RMS and the runner-up easing. Keys are snapped to quarter beats when that moves them by at most one frame. Shake = residual RMS per axis. | 5-12 samples per move: the easing family is indicative, not exact. Unregistered frames at the ends of a shot are held, not extrapolated, so a punch that starts under a flash loses its first frames. Shake frequency is not estimated. |

## `reference_analysis.json` (`edits.reference_analysis/1`)

Every numeric **estimate** is `{ "value", "unit", "confidence", "identifiability" }`.
Frame numbers, counts and scores are plain numbers. Curves carry unit,
confidence and identifiability once for all their keys.

`identifiability`:

- `measured`: read off the video; trust it up to `confidence`.
- `relative_only`: only the change relative to the shot's reference frame is
  known (scale and position of a full-frame picture).
- `needs_source`: cannot be separated from the picture itself (a shot that is
  black and white throughout).
- `qualitative`: the kind of thing is right, the number is a starting point.
- `needs_agent`: not measured; someone has to look.

Top level:

| Field | Content |
| --- | --- |
| `source` | path, size, fps, frame count, duration, audio, analysis size |
| `ingest` | `content_f0/f1`, `blank_runs` (`black`, `white`, `solid`, `fade`), `duplicates` (stalled frames, cadence), `loop` (first frame, period in frames and beats, repeats, score), `analysed_f0/f1` |
| `beats` | `bpm`, `bpm_raw`, `offset_seconds` (beat 0), `audio_offset_seconds`, `refine_shift_frames`, `frames_per_beat`, `candidates`, `source` |
| `shots[]` | see below |
| `slots[]` | `id`, `shots`, `ref_frame`, `proxy` (path relative to the analysis directory) |
| `layers` | `overlay` (path, coverage, strength, blend guess), `vignette` (profile, amount, radius, softness, corner ratio) |
| `signature` | per analysed frame: `lum`, `stops`, `blur`, `sat` (1 where saturation is unmeasurable), `scale` (`null` where not tracked); this is what `compare` uses |
| `notes` | plain-language remarks (loop, stalls, shots needing an agent) |

A shot, shortened (comments added):

```jsonc
{
  "id": 14, "f0": 172, "f1": 182,                 // frames, f1 exclusive
  "beat0": 13.1934, "beat0_snapped": 13.25,       // exact, and on the quarter-beat grid if within a frame
  "beat1": 14.017,  "beat1_snapped": 14.0,
  "cut": { "offset_frames": { "value": -0.68, "unit": "frames", "confidence": 0.77, "identifiability": "measured" },
           "grid": "quarter_beat", "strength": 0.78 },   // beat | half_beat | quarter_beat | off_grid
  "ref_frame": 176, "slot": "p14",
  "status": "measured",                           // or "needs_agent" with "notes" saying why
  "track": { "model": "similarity", "valid_fraction": 0.8, "mean_confidence": 0.67, "direct": 7, "chained": 0, "bridged": 2 },
  "curves": [
    { "prop": "position", "unit": "frame heights (x right, y down)", "confidence": 0.67, "identifiability": "relative_only",
      "keys": [ { "frame": 172, "beat": 13.1934, "beat_snapped": 13.25, "v": [0.0817, -0.0403], "ease": "hold" },
                { "frame": 174, "beat": 13.3581, "v": [0.0817, -0.0403], "ease": { "name": "decay", "rate": 0.46 },
                  "rms": 0.0009, "runner_up": { "ease": "out", "rms": 0.0031 } },
                { "frame": 177, "beat": 13.6052, "beat_snapped": 13.5, "v": [0.0, 0.0], "ease": "linear" } ] },
    { "prop": "exposure.stops", "unit": "stops", "confidence": 0.05, "identifiability": "measured",
      "keys": [ { "frame": 172, "beat": 13.1934, "v": [2.5], "ease": { "name": "decay", "rate": 0.53 } },
                { "frame": 176, "beat": 13.5228, "v": [0.0], "ease": "hold" } ] }
  ],
  "events": [
    { "kind": "flash", "f0": 172, "f1": 176, "frame": 172, "beat": 13.1934, "beat_snapped": 13.25,
      "params": { "peak":           { "value": 2.5,  "unit": "stops", "confidence": 0.05, "identifiability": "measured" },
                  "decay_rate":     { "value": 0.53, "unit": "fraction of the remainder per frame", "confidence": 0.4, "identifiability": "measured" },
                  "peak_luminance": { "value": 199,  "unit": "0..255 (centre of frame)", "confidence": 0.95, "identifiability": "measured" },
                  "luminance_excess_kept": { "value": 0.56, "unit": "fraction kept per frame", "confidence": 0.5, "identifiability": "measured" } } }
  ],
  "shake": { "rms_x": { "value": 0.0033, "unit": "frame heights", "confidence": 0.47, "identifiability": "measured" }, "...": "..." }
}
```

Curve `prop` names: `scale` (x reference frame), `position` (`[x, y]`, frame
heights), `rotation` (degrees clockwise), and `<effect>.<param>`:
`exposure.stops`, `gaussian_blur.radius` (% of height), `saturation.amount`,
`invert.amount`, `threshold.amount`, `threshold.level`. Event kinds: `flash`,
`saturation`, `invert`, `threshold`, `posterize`, `fade`.

Time: frame `f` starts at `f / fps`; its beat is
`(f / fps - beats.offset_seconds) * bpm / 60`. `beat_snapped` is the nearest
quarter beat, given only when the renderer would show it at most one frame
from `f`.

## `draft`

Builds `edits_core` types and serialises them; the result passes
`edits_core::check` (tested). Choices:

- **Time base** beats, `bpm` and `beat_offset` as measured; the project covers
  the whole reference, `audio` is the reference video with `offset` 0.
- **One image layer per shot**, `in`/`out` at the cut frames. A key or cut
  is put on the quarter-beat grid only when the renderer shows the grid time
  on the same frame (the grid time falls inside the frame before it);
  otherwise it keeps the frame's exact time. `--no-snap` never snaps.
- **Key values are what the frames show.** A snapped key can sit up to a
  frame before the frame it was measured on; for front-loaded easings
  (`decay`, `out`, `quad_out`, `quint_out`, `expo_out`) that nothing ramps
  into, the key value is raised so that the rendered value on that frame is
  the measured one (`(v - e * next) / (1 - e)`, `e` the eased fraction at
  the frame).
- **Effects sit on the shot's own layer** (measurements are per shot, and a
  shot can then be moved with its flash): `gaussian_blur`, `saturation`,
  `invert`, `threshold`, `exposure`, in that order. Exposure keys are
  `peak -> 0` with a `decay` ease at the fitted rate. Saturation hits are
  steps; when colour also moves outside the hits (at least two frames off by
  more than 0.2), the measured ratio is fitted as a curve instead (the
  renderer's `saturation.amount` multiplies exactly that ratio).
- **Scale** is relative, so each shot's scale is multiplied by the smallest
  factor that keeps the frame covered during its move.
- **Loop**: the shots are written once per repeat (`--single-loop` for one period).
- **Overlay** plate: top image layer, `screen`, slot `overlay` -> `overlay.png`.
  **Vignette**: adjustment layer `finish` above it.
- **Slots** are empty by default. `--proxies` points them at the proxy stills;
  because those already contain overlay and vignette, layers `overlay` and
  `finish` are then written `enabled: false`.
- **Solid tail** (a fade from white): a solid layer with keyed opacity.
- Shots with status `needs_agent` get a plain image layer and a marker with the reason.

Not drafted: shake modifiers, 3D (cube) layers, glow, blur kinds other than Gaussian.

## `compare`

The candidate (a render, with any pictures) is analysed with the same stages on
the **reference's beat grid and frame range**, and the two analyses are compared:

1. **Tempo**: the candidate's own audio tempo and phase against the reference's
   (`n/a` without audio).
2. **Events**: cuts, flash peaks and tone hits are matched one to one within
   +-1 frame. Reported: counts, precision, recall, F1, mean signed offset,
   unmatched frames. Matched flashes also compare peak (stops) and decay rate.
3. **Curves**, per reference shot, for exposure (stops), blur (% of height),
   saturation (ratio) and scale (ratio): the candidate curve is re-based at the
   reference's reference frame, both are resampled 16 times per beat, and the
   distance is `rms(ref - cand) / max(rms(ref - neutral), floor)` (floors: 0.3
   stops, 0.15 %, 0.15, 0.03). Start, end, peak and frames-to-peak are compared
   against tolerances (0.5 stops, 0.2 %, 0.2, 0.04, 2 frames).
4. **Scores** in 0..1: `cuts`, `tone_hits` = F1; `flashes` = F1 x (0.5 + 0.5 x
   peak agreement); each curve category = length-weighted mean of
   `1 - distance`; `overall` = mean of the categories that could be compared.
5. **Differences**: one sentence each with numbers and units, ranked by
   severity (difference / tolerance), for example
   `shot 1 (beats 0.00-1.00): zoom peaks at 1.12x, reference 1.30x`.

The same clip at another resolution and codec scores about 0.96 overall against
its own analysis; that is the noise floor of the comparison, not zero.

## Tests

`cargo test -p edits-analyze` uses only signals and images generated in code
(`synth`): click tracks at known tempi, and clips rendered from procedural
pictures with known zoom, slide, rotation, flash, blur, saturation, inversion,
threshold, overlay and vignette. No reference footage is in the repository and
none is needed.
