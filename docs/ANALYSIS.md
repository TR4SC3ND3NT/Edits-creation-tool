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
edits-analyze vision export analysis/reference_analysis.json      # strips + questionnaire for a model to answer
edits-analyze vision merge  analysis/reference_analysis.json analysis/vision/answers.json
edits-analyze compare  analysis/reference_analysis.json render.mp4 --sheet side.jpg   # plus side-by-side pairs
```

The same commands are `edits analyze`, `edits vision export`, ... ([CLI.md](CLI.md)).

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

## Trace or recipe

Two ways to start from a reference:

- **Trace** (`edits trace`, `edits track`, `edits measure`, `edits metric`),
  when the reference shows **pictures you have** (the same arts, moved,
  graded, blurred, perhaps cropped). Each picture is found in every frame
  and its exact place is measured as a homography; the draft pins your
  copy of the picture to those corners, frame by frame, with the measured
  tone curves and blur. 3D moves, card flips and perspective come out
  exactly, because they are measured, not classified. This is how a
  hand-made recreation reached SSIM 0.88 over every frame of a 3D edit.
- **Recipe** (`analyze`, `draft`, `compare`, below), when the pictures are
  different. The recipe (cuts, flashes, motion kinds, blur, tone hits) is
  measured independently of the pictures and replayed with yours.

A mix is common: `edits trace --proxy` tracks a plane of the reference
that is not among your pictures (a photo, a card, a background), using a
rectangle of a reference frame as a **measurement template only**, and
`--fill` shows your picture on that plane. Graphics that are not pictures
(text, shapes, lines, glows) are built from their measurements (sizes,
positions, colours per frame) with the engine's layers and effects.

**The rule.** Only parameters come from the reference: homographies,
curves, blur amounts, timings, colours, sizes. Never pixels: no frame,
crop, patch or local tone transfer of the reference goes into a render.
`edits metric` audits it: the share of mid-tone pixels that equal the
reference's within 4/255 stays near 10% for a render built from parameters
(it rises for a near-exact trace with the same arts and a clean source,
and to 50-90% where reference pixels leak in); above 15% on a frame, find
out why.

### The loop

```
edits trace ref.mp4 --arts arts/ -o work/project.json --preview
#   finds each art (quick pass), tracks it, measures its curves and blur,
#   snaps cuts within 1.5 frames of the beat grid, writes the project and
#   trace/{track.json, grade.json, draft.mp4, worst.jpg, side_by_side.mp4}
#   look at trace/worst.jpg and the reference frames yourself
edits render work/project.json -o work/ours.mp4
edits metric work/ours.mp4 ref.mp4 --project work/project.json --sheet work/worst.jpg --side-by-side work/sbs.mp4
#   per-frame SSIM, the worst frames with the layers visible there, the audit
edits measure blur work/ours.mp4 ref.mp4 --frames 100-140 -o work/blur.json
#   blur keys where the reference is softer than the render
```

Then repeat: look at the worst frames (reference | ours | difference)
yourself, name the cause (a missing layer, a wrong blur kind, a glow, a
grade, a timing), search for how the effect is done professionally if it
is not obvious, fix that one thing, render, `metric`, look again. Metrics
point at frames; your eyes decide what is wrong. A change that raises SSIM
but looks worse (blurrier than the reference) is rejected.

### What each step does

**`edits track`** (`planar` module). Per picture and frame: keypoints
(Harris corners on a sqrt(2) pyramid, orientation, a SIFT-like 4x4x8
gradient descriptor; the frame is detected at twice its size when it is
small), matched with the ratio test, a homography by seeded RANSAC; then
the same homography refined on pixels (`homography::refine`: Gauss-Newton
on the 8 parameters plus a gain and an offset, Huber weights against
overlays and occluders, three blur stages, 3 to 0.7 px). The previous
frame's result refined the same way competes, and the better normalised
correlation (NCC) wins; below `--min-ncc` (0.5) the frame is not trusted.
A backward pass carries trusted frames into the frames before them. Gaps
up to `--max-gap` frames (motion blur breaks tracking) are filled by
interpolating the corners, then the corners are smoothed (Savitzky-Golay,
`--smooth 5`). A key whose quad jumps to more than twice or under half
the size of the last kept one is a false match and is dropped; a folded
quad is never written. `--extend N` carries the corners on at their last
speed for N frames before and after each window's trusted keys (the
blurred entry and exit of a shot); it is off by default because an abrupt
move (a collapse into a line, a cut) is not a constant speed: look before
you keep it. `--ignore dark,red` leaves black or red frame pixels out of
the refinement (vignettes, red arrows drawn over the picture). On a
synthetic 6-frame warp the corners land within 0.1 px; through H.264 at
576 px, within 0.15 px.

The track (`edits.track/1`): per art, its `windows` and `keys`, each with
`frame`, `t` (seconds), `h` (picture pixels to frame pixels, pixel-edge
coordinates), `corners` (composition units: the value of a layer's
`transform.pin`), `pin` (the same as a 3x3 matrix), `ncc`, `inliers` and
`method` (`features`, `propagated` or `filled`). `--sheet` draws the quads
on the frames (red = filled): look at it.

**`edits measure grade`.** Per tracked key, the picture is warped into the
frame and its pixels matched to the reference's by quantiles, per channel
(64 quantiles, resampled to `--points` curve points), after the Gaussian
blur (0 to 6 px) that leaves the least error; pictures tracked over it
(smaller ones in the same frame) are left out. The result is `curves` and
`gaussian_blur` keys per key, ready to paste (`arts[].effects`). With a
different picture (a `--fill`) the same matching gives a tone curve per
key: one look per shot, from the reference's colours only; its blur is not
measured (another picture's softness says nothing, and SSIM rewards
blurring a mismatched picture: a metric that lies). In the draft, curves
that barely move over a run become one constant curve.

**`edits measure blur`.** Per frame, on top of a render: the Gaussian, then
the zoom blur (strength, one of 9 centres), then the directional blur
(length, angle) that raise SSIM against the reference most, in the engine's
units, measured at half size.

**`edits metric`.** Both videos decoded at the reference's size and frame
rate; per frame the SSIM of the BT.601 luma after a Gaussian of
`--pre-blur` px (2: placement and shape, not grain), the mean colour
error and the author-pixel share; the mean, the worst frames (with
`--project`, the layers visible at each), `--block NAME=A-B` means, a sheet
of the worst frames and a side-by-side video.

**`edits trace`.** All of the above in one command: windows by a quick
pass over every third frame (an art never found is listed under
`arts_not_found`), `track`, `measure grade`, the song's beat grid from the
reference's sound, and a draft: one image layer per run of tracked frames,
pinned per frame, larger pictures beneath smaller ones, `curves` and
`gaussian_blur` keyed on each, `in`/`out` moved onto a beat when within
1.5 frames, the reference's sound as `audio`, motion blur 8 samples. What
the draft lacks is what is not a tracked picture: text, shapes, flashes,
glitches, light. Add those from your measurements, then loop.

## Stages

| Stage (module) | What it measures | How | Known failure modes |
| --- | --- | --- | --- |
| ingest (`ingest`) | Black lead-in, solid or fading tail, stalled frames, loop | A frame is *blank* when its spatial contrast is below 2/255. A *stall* is a frame whose difference to its predecessor is under 30% of the median difference of its neighbours. Loop: the lag at which 12x12 z-normalised frame descriptors match again (correlation >= 0.8); the shortest such lag is the period. | Loops shorter than 1.5 s are not looked for. A loop whose passes differ (different flash, different picture) is not found. Stall cadence is reported only when it is regular. |
| beats (`beats`) | Tempo, beat phase, grid | Spectral flux of the audio (1024/256 STFT, log magnitude), autocorrelation with a broad prior around 120 BPM over 60-200 BPM, then period and phase refined together on a beat comb. A whole-number tempo is taken when it explains the envelope within 3%. Beat 0 is the grid beat nearest the first analysed frame; the phase is then nudged by at most one frame so that cuts keyed on the grid land on the frames where the reference shows them. Without usable audio the frame-difference signal is used (`source: "visual"`). | Octave errors on unusual music (candidates are listed). A clip of N seconds resolves tempo to roughly 5/N BPM. The grid is uniform: an edit cut on a different frame rate and converted can sit 1-2 frames off it (reported per cut). |
| shots (`shots`) | Cuts | 64 px thumbnails, band-passed, compared as z-normalised correlation after a search over shift (+-9 px) and zoom (up to 1.33x). A flash is affine in intensity, so it cancels; a punch-in is found by the search; a tone inversion gives a strong *negative* correlation and is kept as an invert toggle, not a cut. A cut needs similarity < 0.6 and the old picture not coming back within two frames. Among neighbouring candidates the frame where brightness jumps up wins (a flash peaks on its cut). | Fast 3D turns, whip blurs and picture-in-picture changes look like cuts or hide them. A cut between two shots of the same picture at a similar size is not a cut for this stage. Dissolves are not modelled. |
| reference frame, slots (`pipeline`) | One frame per shot to measure against; which shots share a picture | Sharpest frame that is not brighter than the shot's 60th percentile, not inverted, at least 0.6x as colourful as the shot's most colourful frames (85th percentile, so a grey hit covering half the shot cannot become the reference), preferring the stillest. Slots: reference frames that register to each other (correlation >= 0.38 after a coarse search over scale and shift) share a slot. Proxy still = the reference frame at full resolution. | A shot that is flashed or blurred throughout has no good reference frame. The same picture at very different crops, or on a cube face, lands in separate slots. Proxies contain the overlay and vignette. |
| layers (`layers`) | Static overlay plate, static texture, vignette | Overlay: per-pixel temporal median over the loop, white top-hat (median minus its morphological opening), part common to R, G and B, divided by `1 - background` (undo a screen blend). Texture: high-pass of log luminance (sigma 0.6% of height, clipped at 0.4 stops) on one frame per shot, averaged: picture detail cancels, a static texture stays. Read on an 8 x 8 grid (overlays sit in a column or a corner); presence = share of detail common to every picture beyond chance, mean of the best quarter of blocks. A second reading turns the sign where the picture is light, for overlays that are light on dark frames and dark on light ones (`difference`). Its pixels are left out of every measurement, like the strokes. `texture.png`: mid grey + detail (`soft_light`) or detail on black (`difference`), faded where the blocks show none. Vignette: radial profile, in linear light, of the upper quartile of the brightest frames and of the 90th-percentile ceiling of all frames, the brighter kept ring by ring; fitted with `1 - amount * smoothstep(radius, radius + softness, r)`, `r` in half-diagonals (the renderer's formula, `compose.rs`). | Needs at least 3 shots (texture: 4). Overlay: only thin bright strokes. Texture: a texture that moves, or one the pictures share by chance, is not separated; the plate keeps a faint residue of the pictures; blend and strength are a guess. The vignette is the darkest a frame gets at each radius: art that is dark at the edges in every shot reads as vignette. |
| track (`track`) | Scale, rotation, translation of every frame relative to the reference frame | Coarse-to-fine inverse-compositional Gauss-Newton (Lucas-Kanade) on locally normalised gradient magnitude, zero-mean normalised cost (as in ECC): invariant to gain, offset and inversion. Central mask without the overlay strokes. Warm start from the neighbouring frame, retry from an exhaustive coarse search, then frame-to-frame chaining. Frames below correlation 0.3 are marked `bridged` and interpolated. A free affine fit is run as a check: if it is clearly better with unequal scale factors the frame is *non-similarity*. | Flash peaks and heavy blur frames do not register (bridged; a move that happens entirely under a flash is not seen). 3D tilts, cube turns and inset panels are flagged `needs_agent`, not measured. Flat or tiny pictures have too little texture. Absolute scale is unknowable (see identifiability). |
| exposure (`exposure`) | Gain per frame in stops; flash events with peak and decay | Quantiles of linear-light luminance of the frame against those of the warped reference frame; the gain is the median ratio over quantiles not clipped at white (a pixel as bright as the white field at its position counts as clipped). Flash = local maximum >= 0.8 stops; decay = log-linear fit of the stops after the peak. For shots without a track, neighbouring frames are compared and accumulated. | When most of the frame is clipped the peak is a **lower bound** (flagged, low confidence). A flash made by adding white or glow is not a pure gain; an affine fit flags `fade` events but the split is weak. Chained estimates drift. |
| tone (`tone`) | Saturation, inversion, threshold, posterise hits | Saturation: chroma/luminance in linear light on block means, ratio to the reference frame; blocks that are clipped, and blocks next to one the flash clipped (codecs bleed its grey into them), are left out. Where under 6% of the blocks remain, the signature records the neutral 1. A grey frame on a flash peak over 1.5 stops is part of the flash, not a hit. Inversion: negative luminance correlation with the registered reference frame, or frame-to-frame sign flips. Threshold: the quantile-matched tone curve is a step and darks are crushed, not lifted. Posterise: channel values sit in few levels, or (histogram comb, `look`) the frame occupies under 32% of the codes in separate clusters while its reference frame occupies 1.8 times more; levels = clusters per channel. Duotone (`look`, no reference needed): chroma binned by display luminance; when what is left inside the bins is under 35% of the chroma (and there is chroma), the colours are a function of luminance (a gradient map or a tint); `shadows` and `highlights` are the fitted line at the darkest and brightest pixels, as display colours. | Under a flash colour washes out anyway; one-frame hits on flash peaks are not reported, so a real one-frame hit on a strong flash is lost. A black-and-white reference frame makes saturation unmeasurable (`needs_source`). A very strong flash and a threshold are the same thing on ink-on-paper art. A picture that is itself two-coloured (sepia art, a yellow emblem on black) reads as duotone over the whole shot (`needs_source`; drafted as a gradient map either way). |
| blur (`blur`) | Gaussian-equivalent sigma per frame; blur kind and its own amount | Radially averaged power spectrum of a centred Hann window, noise floor (median power above 0.42 cycles/pixel) taken off both, ratio frame/reference; `log ratio = c - 4 pi^2 sigma^2 f^2` fitted where the blurred signal is above the noise. The reference frame is warped onto the frame when the warp covers the window (overlay strokes are filled in). Under a flash the reference is put through blur, gain and clip and the estimate refined against it. Kind (`blurkind`): gradient energy of frame and reference along 8 fixed orientations and along the radial and tangential directions about the centre, normalised by the mean level (a gain cancels); log ratios, median over the blurred frames: a first harmonic over the orientations is a directional streak (its phase the direction), radial lost more than tangential is zoom, the reverse spin, neither gaussian. Amount of a streak: the reference frame is blurred on the CPU the same way until it loses as much gradient energy as the most blurred frame (the picture's own detail cancels); the result is in the renderer's units (`directional_blur.length`, `zoom_blur.strength`, `spin_blur.angle`), `per_sigma` converts the curve. | The Gaussian-equivalent radius of a streak hardly grows with its length: use the kind's `peak`. A fast punch reads as zoom or directional (motion blur is real blur). Motion blur of a fast move reads as blur. Blur below about 0.1% of height is noise. Unreliable in shots without a track (the reference frame shows something else). Reads low under a flash that clips a smooth picture, after a codec on smooth pictures (small blurs read 0), and 5-15% low above about 3% of height (see CALIBRATION.md). |
| curves (`curves`) | Keyframes and easing for each track; shake | Dynamic programme over break points; each segment fitted with `hold`, `linear`, `out`, `in_out`, `in`, `expo_out` and `decay` (rate by golden-section search) through `edits_core::ease::apply`, penalty per segment, joint solve of key values. Reports segment RMS and the runner-up easing. Keys are snapped to quarter beats when that moves them by at most one frame. Shake = residual RMS per axis. | 5-12 samples per move: the easing family is indicative, not exact. Unregistered frames at the ends of a shot are held, not extrapolated, so a punch that starts under a flash loses its first frames. Shake frequency is not estimated. |

| cards (`pipeline::card_box`) | Where a card, cube or inset sits | On the reference frame of a shot that could not be tracked: the box of the middle 96% of the pixels 0.12 brighter than the frame's outer ring. | Only on a dark background; a card that turns is boxed as seen on its reference frame. |

Renders of the engine's own `directional_blur` (0, 30, 45, 90 degrees), `zoom_blur` and `spin_blur` are classified and their amounts recovered within 25% (`cargo test -p edits-calib --test blurkind`); synthetic clips check the duotone colours, posterise levels through noise, a static texture against unrelated pictures and a vignette without flash frames (`cargo test -p edits-analyze`).

## The vision pass

Signals give numbers; they cannot say what kind of thing a shot is: a cube
or a flat card, a red glow around a card, a paper texture, a duotone. A
model looking at frames can, if it is asked closed questions in the
engine's vocabulary and never asked for speeds or frame counts
([EDIT_LOGIC.md](EDIT_LOGIC.md) section 4). So the pass has three steps:

1. **Export** (`edits vision export analysis/reference_analysis.json`, MCP
   `reference_vision_export`) writes to `analysis/vision/`:
   - `shotNN.jpg`, a labelled strip per shot: the last frame of the previous
     shot, the cut, cut+1, cut+2, the reference frame and the last frame on the
     first row; every frame around the strongest flash or tone hit on the
     second (one-frame hits are invisible otherwise). Tiles are numbered and
     labelled with frame, beat and what they are; under 1568 px wide.
   - `shotNN_detail.jpg`: the reference frame's centre and left edge at full
     resolution (textures, handwriting, card borders and glows need pixels).
   - `overview.jpg`: every shot's reference frame, for the questions about the whole edit.
   - `questions.json`: the questions and, per shot, the tiles and what the
     signals measured (to confirm or contradict); `answers.json`: a template
     with every answer `unsure`.
2. **Answer**: the agent looks at the images and fills `answers.json`, only
   with the listed words (`unsure` when in doubt), with the tile numbers that
   show each label:

   | Field | Question | Words |
   | --- | --- | --- |
   | `picture` (Q1) | What is the picture? (becomes the slot's `role`) | `panel_bw`, `panel_colour`, `full_art`, `close_up`, `detail_crop`, `emblem`, `text_card`, `solid` |
   | `framing` (Q2) | How is it framed? | `full_frame`, `inset_card`, `tilted_card`, `cube`, `split`, `picture_in_picture` |
   | `border` (Q2) | Border of the card | `none`, `thin_light`, `thin_dark`, `glow[:colour]` |
   | `tone` (Q3) | Tone treatments | `normal`, `inverted`, `threshold`, `posterized`, `duotone[:dark/light]`, `tinted[:colour]`, `desaturated`, `burnt_white` |
   | `light` (Q4) | Light | `white_flash`, `black_flash`, `glow`, `light_rays`, `flare` |
   | `motion` (Q5) | Kind of motion | `static`, `punch_in`, `zoom_out`, `push_in`, `shake`, `slide:<dir>`, `spin`, `3d_turn:<x/y/z>`, `whip:<dir>` |
   | `blur` (Q6) | Kind of blur | `none`, `gaussian`, `directional[:degrees]`, `zoom`, `spin` |
   | `overlays` (Q7) | Over the picture | `paper`, `handwriting`, `grain`, `scratches`, `speed_lines`, `focus_lines`, `screentone`, `particles`, `caption`, `sfx_in_art` |
   | `entry` (Q8) | How it comes in | `hard_cut`, `flash_cut`, `dissolve`, `whip`, `zoom_through`, `cube`, `push`, `glitch` |
   | `global` | The whole edit | `overlays` (G1, as Q7 plus `dust`, `light_leak`), `overlay_tone` (`lighter`, `darker`, `both`), `vignette` (`none`, `light`, `medium`, `strong`) |

   Colours are names (`red`, `gold`, `navy`, ... listed in `questions.json`) or `#rrggbb`.
3. **Merge** (`edits vision merge analysis/reference_analysis.json analysis/vision/answers.json`,
   MCP `reference_vision_merge`) checks every word (an unknown one fails the
   merge, changing nothing, and the error lists the valid words), turns tile
   numbers into frames, checks each label against the signals (`inverted`
   needs a negative luminance correlation, `white_flash` an exposure peak,
   `shake` a residual, `punch_in` a scale change; `cube` on a shot that tracked
   as a flat picture is suspect) and writes `shots[].vision`, `vision` and
   `slots[].role`. Disagreements are listed: look at every frame of that span again.

`draft` then uses the labels for what the signals could not decide, and keeps
the measured numbers for amounts and times:

- **Shots the track could not follow** (`needs_agent`): `cube` becomes a
  `cube` layer (the shot's slot and the next one on its faces, turning about
  the labelled axis, placed and sized from the measured card box);
  `tilted_card`, `inset_card` and `picture_in_picture` scale and place the
  picture at the card box (a tilted card also turns in 3D); `border` adds an
  `outline` (a glow is a wide soft outline in the labelled colour); tone hits
  the signals did not measure are drafted over the frames their tiles name
  (none without tiles); `push_in`, `zoom_out`, `slide`, `punch_in` and
  `3d_turn` become keys or presets; `whip`, `zoom_through`, `dissolve`,
  `cube`, `push` and `glitch` entries become transitions starting at the cut.
- **Every shot**: `glow`, `light_rays` and `flare` add the effect (glow on
  tracked shots only for emblems); `speed_lines`, `focus_lines`, `particles`,
  `screentone` and `grain` add the preset or effect; a labelled blur kind
  converts the Gaussian blur when the signals measured that kind's amount.
- **Tracked shots** keep their measured structure: a label that contradicts
  the track (a cube on a shot that tracked as a flat picture) is a note.
- **The whole edit**: the slot `role`s; the overlay plate's blend from
  `overlay_tone`; a labelled `vignette` that disagrees with the measurement
  replaces it; overlays seen but not recovered add a disabled `texture`
  layer waiting for a picture.

`draft --no-vision` ignores the labels. Every change is printed, one line each.

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
| `slots[]` | `id`, `shots`, `ref_frame`, `proxy` (path relative to the analysis directory), `role` (from the vision labels) |
| `layers` | `overlay` (path, coverage, strength, blend guess), `texture` (presence, strength, coverage, path, blend guess), `vignette` (profile, amount, radius, softness, corner ratio) |
| `vision` | global labels from `vision merge`: `overlays`, `overlay_tone`, `vignette`, `checks` |
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

A shot can also carry `blur_kind` (`kind`, `confidence`, `angle`,
`axis_contrast`, `radial_tangential`, `frames`, `peak`, `per_sigma`), `card`
(`center`, `size` in frame heights, `background`) and `vision` (the answers
to Q1-Q8, `colors`, `tiles` and their `frames`, `checks`). A `duotone`
event carries `colors.shadows` and `colors.highlights` (display sRGB).

Curve `prop` names: `scale` (x reference frame), `position` (`[x, y]`, frame
heights), `rotation` (degrees clockwise), and `<effect>.<param>`:
`exposure.stops`, `gaussian_blur.radius` (% of height), `saturation.amount`,
`invert.amount`, `threshold.amount`, `threshold.level`. Event kinds: `flash`,
`saturation`, `invert`, `threshold`, `posterize`, `duotone`, `fade`.

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
- Shots with status `needs_agent` get a plain image layer and a marker with the reason
  (and, with vision labels, the structure the labels name: see [the vision pass](#the-vision-pass)).
- **Blur kinds**: a shot whose blur is directional, zoom or spin with its
  amount measured (`blur_kind.per_sigma`) gets that effect instead of
  `gaussian_blur`, the curve scaled into its parameter.
- **Posterise and duotone** hits become `posterize` (levels keyed) and
  `gradient_map` (duotone, the measured colours) on the shot's layer when
  the vision labels saw them (`posterized`, `duotone` or `tinted`): on their
  own both are weak (a flash thins a histogram; a picture of few flat
  colours reads as duotone), and drafting them unconfirmed cost the
  calibration round trip a tenth of its exposure score.
- **Shake**: the measured residual becomes a `shake` modifier on the
  position (about 1.6 x the RMS), with a constant zoom that keeps the edges covered.
- **Texture** plate: an image layer `texture` (`soft_light` or `difference`,
  as measured) over the pictures.

Not drafted: the 3D pose of cubes and cards (only their box), dissolves the labels do not name.

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

**Side by side** (`--sheet side.jpg`, MCP `reference_compare` with `sheet`):
pairs of the reference and the video at the same frames (each shot's cut,
strongest flash, first tone hit, reference frame and last frame; the
reference left with a blue bar, the video right with an orange one), worst
shots first, 16 pairs per page (`side_2.jpg`, ...). `--sheet-shots 3,7`
limits it. Signatures say *how much*; the pairs show *what*: ask of each pair,
in the vision vocabulary, whether the picture kind and framing, the light
(flash, glow), the tone, the motion and its direction, the blur kind and the
overlays agree, and name the change (layer, effect, value). The pictures
differ on purpose; ignore what they show.

The same clip at another resolution and codec scores about 0.96 overall against
its own analysis; that is the noise floor of the comparison, not zero.

## Your own pictures: the whole procedure

To remake a reference edit (say a Kira edit) with your own pictures (L, Light,
Kira: your scans or screenshots; never commit them):

```
edits analyze ref.mp4 -o work/analysis                       # the recipe
edits vision export work/analysis/reference_analysis.json    # strips + questions
#   look at work/analysis/vision/overview.jpg and each shotNN.jpg / shotNN_detail.jpg,
#   fill work/analysis/vision/answers.json (fixed words, tiles)
edits vision merge work/analysis/reference_analysis.json work/analysis/vision/answers.json
edits draft work/analysis/reference_analysis.json -o work/edit/project.json
#   put your pictures in work/pictures/, point each slot at one (project_edit set_slot,
#   or edit slots.*.path); match the slot's `role`: an emblem slot gets an emblem,
#   a close_up slot a face; the order of the pictures is yours
edits render work/edit/project.json -o work/edit/preview.mp4 --scale 0.5
edits compare work/analysis/reference_analysis.json work/edit/preview.mp4 --sheet work/edit/side.jpg
#   look at side.jpg (and side_2.jpg, ...), fix the kinds that differ, then
edits fit work/edit/project.json -r work/analysis/reference_analysis.json   # tune the amounts
```

A good stand-in while you have no pictures: the reference's own stills
(`analysis/proxies/`) moved to other slots (keep emblems on emblem slots), or
the generated pictures of `edits demo`.

Measured on the Kira reference (16 s, 17 shots, 1080 x 1080), drafts without
`fit`, rendered at half size:

| Pictures | Earlier pipeline | New detectors, no labels | With vision labels |
| --- | --- | --- | --- |
| The reference's stills, moved five slots on (emblem slot kept on an emblem with labels) | 0.70 (0.69 against the new analysis) | 0.67 | 0.68 (tone hits 0.56 -> 0.67, scale 0.76 -> 0.80, exposure 0.49 -> 0.43) |
| `edits demo` pictures | 0.73 (0.70 against the new analysis) | 0.70 | 0.68 |

The labels make the shots the track could not follow look like the
reference in the side-by-side sheets (a cube, tilted and inset cards, the
red glow, the emblem on black) without raising the signature score: a card
on black measures differently from the reference's card until its pose and
size match, and `compare` scores only what both measure. Judge those shots
on the sheet.

## Tests

`cargo test -p edits-analyze` uses only signals and images generated in code
(`synth`): click tracks at known tempi, and clips rendered from procedural
pictures with known zoom, slide, rotation, flash, blur, saturation, inversion,
threshold, overlay and vignette. No reference footage is in the repository and
none is needed.
