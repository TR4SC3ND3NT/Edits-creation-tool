# The `edits` command

```
cargo build --release -p edits-cli      # the binary is target/release/edits
```

Every command works from a plain shell with no window or display. Results that
are data go to **stdout** as JSON; progress and error messages go to **stderr**;
a failure always ends with a non-zero exit code and a line starting `error:`.

Times on the command line (`--from`, `--to`, `--at`) are in the project's
`time_base`: beats for a beats project, seconds otherwise; they can also be
written `b12.5` (beat), `bar 4`, `4:3` (bar 4, beat 3), `2.5s`, `f60`, a
marker such as `drop`, or with offsets, `drop+2b` (all of them in
[FORMAT.md](FORMAT.md#times-as-text)). Frame numbers start
at 0. `--scale 0.5` renders at half the project's width and height (sizes are
rounded to even numbers); effect sizes follow, so a small render is a faithful
preview of the full one.

| Command | What it does |
| --- | --- |
| [`render`](#render) | Project → H.264 MP4 (with AAC audio) through the system `ffmpeg` |
| [`frame`](#frame) | One frame → PNG |
| [`sheet`](#sheet) | A grid of frames → PNG, each labelled with frame number and time |
| [`bench`](#bench) | Time the renderer; optionally per kind of pass |
| [`describe`](#describe) | A compact text summary of a project |
| [`check`](#check) | Validate a project; exit 1 on errors |
| [`effects`](#effects) | The effect registry: kinds, parameters, defaults, meaning |
| [`doctor`](#doctor) | Which adapter and which encoders this machine has |
| [`demo`](#demo) | Write a self-contained demo project |
| [`analyze`](#analyze) | Extract the recipe of a finished edit into `reference_analysis.json` |
| [`draft`](#draft) | Recipe → project with one empty slot per picture |
| [`compare`](#compare) | Score a video against a recipe in signature space (any pictures); side-by-side pairs |
| [`vision export`](#vision-export-and-vision-merge) | Per-shot strips and a fixed questionnaire for a model to answer |
| [`vision merge`](#vision-export-and-vision-merge) | Validate the answers, check them against the signals, merge them into the analysis |
| [`probe`](#probe) | Per-frame signatures of a video |
| [`fit`](#fit) | Tune a project's effect values until its render matches a reference |
| [`trace`](#trace) | Reference + the arts in it → a draft of pinned, graded layers (and a scored preview) |
| [`track`](#track) | Find known pictures in a reference, frame by frame: homographies and corner pins |
| [`measure`](#measure) | Tone curves per tracked key (`grade`), blur per frame (`blur`), as effect keys |
| [`metric`](#metric) | Per-frame SSIM against the reference, worst frames on a sheet, the author-pixel audit |
| [`audio`](#audio) | Song → tempo, beat grid, bars, kick/snare/hat onsets, energy, sections, the drop |
| [`presets`](#presets) | The preset library: flashes, punches, shakes, slides, 3D flips, transitions... |
| [`apply`](#apply) | Apply a preset at one or more times, as one undoable edit |
| [`save-preset`](#save-preset) | Save what a layer does over a time range as a preset |
| [`beats cut`](#beats-cut) | One picture per beat (or bar, or kick) between two times |
| [`beats accent`](#beats-accent) | A preset on every beat, bar, kick, snare, hat or the drop |
| [`transitions`](#transitions) | The transitions between two layers, with their parameters |
| [`shader check`](#shader-check) | Check a custom effect shader without a GPU, with line numbers |
| [`gui`](#gui-and-mcp) | Open the desktop app (Edits Studio, or `edits-gui`) |
| [`mcp`](#gui-and-mcp) | Serve the MCP tools over stdio (`edits-mcp`) |

A first run:

```
edits doctor
edits demo demo
edits sheet demo/project.json -o sheet.png
edits render demo/project.json -o demo.mp4
```

## Options for every command

`--adapter auto|hardware|software` chooses the graphics adapter; the
environment variable `EDITS_ADAPTER` does the same. `auto` (the default) takes
a hardware GPU when there is one and a CPU rasteriser otherwise; `hardware`
and `software` fail when there is no such adapter. On a machine without a GPU
install Mesa lavapipe (`mesa-vulkan-drivers` on Debian and Ubuntu).

`EDITS_FFMPEG` names the ffmpeg binary when it is not `ffmpeg` on `PATH`.

## render

```
edits render <project.json> -o out.mp4
             [--from T] [--to T] [--scale S] [--samples N] [--crf N] [--no-audio] [-q]
```

Renders the frames whose start times lie in `[from, to)` and pipes them as raw
RGBA into ffmpeg: H.264, `yuv420p`, BT.709, `-movflags +faststart`. The
encoder is `libx264` when the ffmpeg has it, otherwise `libopenh264`; with
neither the command fails and says what to install. `--crf` is the x264
quality (default 18; lower is better); for `libopenh264`, which has no CRF, it
is turned into a bitrate.

When the project has `audio`, the file is encoded to AAC and muxed starting at
`audio.offset` plus the start of the rendered range, trimmed to the rendered
length, with `audio.gain` applied. `--no-audio` leaves it out.

The sound of `video` layers is mixed in too (`amix`, without normalising):
each from its layer's `in` to its `out`, from the slot's `trim`, at the
layer's `volume`, looped or reversed like the picture and at its constant
`speed` (pitch kept). A video with an animated `speed` or a `time_remap`
stays silent, with a warning on stderr; `"volume": 0` mutes one. `clip_audio`
in the output counts the layers heard.

So are the project's sound effects (`audio_clips`, see
[FORMAT.md](FORMAT.md#sound-effects-audio_clips)): each is cut to
`trim_in`..`trim_out` (`atrim`), faded (`afade`), set to its `volume` and
delayed to its `at` (`adelay`), then added to the mix at full level, so the
song does not get quieter. ffprobe gives the length of a clip without
`trim_out`. `audio_clips` in the output counts the clips heard in the
rendered range; a disabled or muted clip is not one.

`--samples N` overrides `settings.motion_blur.samples`; `--samples 1` is the
fast way to preview without motion blur. `-q` silences the progress lines.

```
$ edits render demo/project.json -o part.mp4 --from 2 --to 4 --scale 0.5
frame 8/24 (33%)  182 ms/frame  3 s left
frame 24/24 (100%)  161 ms/frame  0 s left
{
  "adapter": { "backend": "Vulkan", "driver": "llvmpipe Mesa 25.2.8-0ubuntu0.24.04.4 (LLVM 20.1.2)",
               "name": "llvmpipe (LLVM 20.1.2, 256 bits)", "software": true, "type": "Cpu" },
  "audio": true,
  "audio_clips": 0,
  "clip_audio": 0,
  "duration_seconds": 0.96,
  "encoder": "libx264",
  "first_frame": 25,
  "first_frame_ms": 323.2,
  "fps": 25.0,
  "frames": 24,
  "height": 540,
  "motion_blur_samples": 6,
  "ms_per_frame": 159.6,
  "output": "part.mp4",
  "seconds": 4.107,
  "startup_ms": 151.5,
  "width": 540
}
```

`seconds` is the whole render including encoding, `ms_per_frame` the average
time spent rendering a frame, `startup_ms` opening the adapter and loading the
project's pictures, `first_frame_ms` the first frame (which also compiles
shaders).

## frame

```
edits frame <project.json> (--at T | --frame N) -o frame.png [--scale S] [--samples N]
```

`--at T` renders the frame that contains project time `T`, so the picture is
exactly what the video shows at that moment; `--frame N` names the frame
directly. With neither, frame 0 is rendered.

```
$ edits frame demo/project.json --at 3 -o hit.png
{ "frame": 36, "time": 2.952, "time_unit": "beats", "seconds": 1.44,
  "width": 1080, "height": 1080, "render_ms": 776.6, "startup_ms": 140.5,
  "output": "hit.png", "adapter": { ... } }
```

Frames start every `1 / fps` seconds, so a beat rarely falls exactly on one:
beat 3 at 123 BPM is 1.4634 s, inside frame 36 (1.44 s to 1.48 s). The first
frame that starts at or after a beat is the one `--from` selects, here 37.

## sheet

```
edits sheet <project.json> -o sheet.png
            [--from T] [--to T] [--count N | --every-frame] [--cols C] [--scale S] [--samples N] [-q]
```

A contact sheet: the quickest way to look at a whole edit, or at every frame
around a hit. `--count N` (default 16) spreads N frames evenly over the range,
first and last included; `--every-frame` takes them all. Under each cell is
its frame number, project time and time in seconds, drawn with a built-in
bitmap font (`#37  BEAT 3.03  1.48S`). Without `--scale`, cells are about
320 pixels wide; without `--cols`, the grid is roughly square.

```
edits sheet demo/project.json -o overview.png                       # 16 frames of the whole edit
edits sheet demo/project.json --from 2.9 --to 3.4 --every-frame --cols 6 -o hit.png
```

The JSON summary lists the frame numbers, the grid and the cell size.

## bench

```
edits bench <project.json> [--from T] [--to T] [--count N] [--scale S] [--samples N] [--passes]
```

Renders frames and throws them away, to measure the renderer alone:
`ms_per_frame`, `ms_median` and `ms_slowest` leave the first frame out and
report it as `first_frame_ms`. `rendered_samples_per_frame` is lower than the
motion blur setting when parts of the edit hold still (identical sub-frame
instants are rendered once). `--passes` adds a table of render passes by kind,
with counts and times, which is where to look when a project renders slowly;
it waits for the GPU after every pass, so the totals are higher than normal.

## describe

```
edits describe <project.json>
```

Prints `edits_core::describe`: size, length, slots, one block per layer
with its keys and effects, the markers and the sound effects, in a few
hundred tokens. Read this before editing a project.

## check

```
edits check <project.json> [--json]
```

Validates the project with its own folder as the base for relative paths, so
missing pictures, shaders and audio are found (sound effects too, with their
trims and fades). Exit code 1 when there is at
least one error; warnings alone exit 0. A file that is not valid JSON is
reported the same way.

```
$ edits check broken.json --json
{
  "ok": false, "errors": 2, "warnings": 0,
  "findings": [
    { "severity": "error", "path": "layers[4:\"finish\"].effects[2]",
      "message": "unknown effect \"vinyette\"", "hint": "known effects: exposure, fade, ..." },
    { "severity": "error", "path": "slots.b", "message": "image not found: img/nope.png",
      "hint": "paths are relative to the project file" }
  ]
}
```

`render`, `frame`, `sheet` and `bench` run the same check first and refuse a
project with errors, listing them.

## effects

```
edits effects [--json]
```

The registry from `edits-core`: every effect kind by category with its
parameters (default, unit, range, meaning), its options and where its maths
comes from:

```
-- stylize --
...
pixelate: Mosaic: the picture in flat cells, each the colour at its centre.
    size         default 2                %h 0..     Cell size, % of height (the ring width for polar).
    center       default [0.0, 0.0]       pos        A point the grid is centred on, in composition units.
    option shape square | hex | triangle | polar (default square): Cell shape; polar cells are rings cut into segments about `center`.
    credit: GPUImage Pixellate and PolarPixellate, BSD-3-Clause; glfx.js hexagonalPixelate, MIT
```

`--json` gives `[{ "kind", "category", "doc", "params": [{ "name", "default",
"unit", "range", "doc" }], "options": [{ "name", "values", "default", "doc" }],
"source", "reads_time", "credit" }]`. `unit` is `""`, `"%h"`, `"deg"`,
`"pos"`, `"color"`, `"x"`, `"count"`, `"stops"`, `"px1080"` or `"time"`;
`range` is `[min, max]` with `null` for an open end, or `null`; `source` is
`"none"`, `"shader"` (a WGSL file), `"cube"` (a `.cube` file), `"slot"` (a
`"slot:<name>"` map) or `"optional_slot"` (a map, or the layer itself).
[EFFECTS.md](EFFECTS.md) has the details. The text ends with the names of the
transitions, which are listed by [`edits transitions`](#transitions).

## doctor

```
edits doctor [--json]
```

```
adapter:  llvmpipe (LLVM 20.1.2, 256 bits) (Vulkan, Cpu, software rasteriser)
driver:   llvmpipe Mesa 25.2.8-0ubuntu0.24.04.4 (LLVM 20.1.2)
ffmpeg:   ffmpeg version 6.1.1-3ubuntu5 Copyright (c) 2000-2023 the FFmpeg developers
h264:     libx264
aac:      aac
video:    will encode with libx264
ready to render
```

Exit code 0 when there is an adapter and an H.264 encoder, 1 otherwise. The
JSON form has `ok`, `adapter` (`name`, `backend`, `type`, `driver`,
`software`) or `adapter_error`, `ffmpeg` (`found`, `version`) and `encoders`
(`h264`, `aac`, `selected_h264`).

## demo

```
edits demo <dir>
```

Writes `<dir>/project.json` (the recipe in `examples/beat-loop.json` with its
slots filled), four generated panel pictures and a grungy overlay texture in
`<dir>/img/`, and a click track in `<dir>/audio/beat.wav`. Everything is drawn
by code, so the demo needs no downloads:

```
edits demo demo && edits render demo/project.json -o demo.mp4
```

To make the same edit with other pictures, change only the `path` of each slot.

## analyze

```
edits analyze <video> -o <dir> [--height 360] [--bpm N] [--beat-offset S] [--whole] [--samples] [--json]
```

Measures the recipe of a finished edit and writes `<dir>/reference_analysis.json`,
`<dir>/overlay.png` (the static overlay plate) and `<dir>/proxies/` (one still
per picture). The video is decoded through `ffmpeg`. `--bpm` and
`--beat-offset` override the tempo estimate; `--whole` analyses every repeat of
a looping edit instead of the first period. Stages, failure modes and the file
format are in [ANALYSIS.md](ANALYSIS.md).

```
$ edits analyze ref.mp4 -o an --json
{
  "analysis": "an/reference_analysis.json",
  "beat_offset_seconds": -0.23828,
  "bpm": 82.0,
  "needs_agent": [2, 3],
  "seconds": 2.5,
  "shots": 3,
  "slots": 2
}
```

`needs_agent` lists the shots whose motion could not be measured (3D moves,
cubes, insets): look at those frames yourself.

## draft

```
edits draft <dir>/reference_analysis.json -o project.json [--proxies] [--width W --height H] [--no-snap] [--single-loop] [--no-vision] [--json]
```

Writes a project that replays the analysed recipe: beats time base, the
reference's audio, one slot per picture (empty, or with `--proxies` the
reference's own stills so it renders at once), one layer per shot with its
measured motion and effects, the overlay and the vignette. Key times snap to
the quarter beat unless `--no-snap`. The project passes `check`, or the
command fails. With vision labels merged into the analysis it also picks
layer kinds, effects and presets the signals could not decide (cubes, cards,
glows, transitions, slot roles) and prints one line per change
(`--no-vision` ignores the labels; `--json` lists them under `vision`):

```
wrote edit/project.json: 37 layers, 18 slots, 14 markers, 33.86 beats at 123.531 bpm; check: 0 errors, 17 warnings
  shot 3: cube (faces p03 and the next picture) turning about y
  shot 8: inset card (scale x0.72)
  shot 8: outline for the glow border (outside)
  shot 10: gradient_map duotone with the measured colours
  shot 13: labelled cube, but the track held as a flat picture: not drafted, look again
```

## compare

```
edits compare <dir>/reference_analysis.json <video> [--json] [--top 25] [--sheet side.jpg] [--sheet-shots 3,7] [--reference-video ref.mp4]
```

Analyses `<video>` the same way and compares it with the reference in
signature space: tempo, cuts, flashes and tone hits matched on the beat grid,
and per shot the exposure, blur, saturation and zoom curves. Pixels are not
compared, so a render with different pictures is not penalised for them. The
report ends with the largest differences in plain language:

```
overall 0.67  blur 0.54  cuts 0.94  exposure 0.40  flashes 0.72  saturation 0.28  scale 0.66  tempo 1.00  tone_hits 0.80
...
  9. [exposure 8.7] shot 14 (beats 13.19-14.02): exposure starts at 6.83 stops, reference 2.46 stops
```

`--sheet side.jpg` also writes side-by-side pages (`side.jpg`, `side_2.jpg`,
...): the reference and the video at each shot's cut, strongest flash, first
tone hit, reference frame and last frame, reference left (blue bar), video
right (orange bar), worst shots first. The report and the JSON (`sheets`)
list them. What to ask of the pairs: [ANALYSIS.md](ANALYSIS.md#compare).

## vision export and vision merge

```
edits vision export <dir>/reference_analysis.json [--video ref.mp4] [-o <dir>/vision] [--shots 3,7] [--tile-height 180] [--json]
edits vision merge  <dir>/reference_analysis.json <dir>/vision/answers.json [-o merged.json] [--dry-run] [--json]
```

`export` writes, per shot, a labelled strip (`shot03.jpg`: previous frame,
cut, cut+1, cut+2, reference frame, last frame, and every frame around a
flash or tone hit), a detail image (`shot03_detail.jpg`, centre and left edge
at full resolution), `overview.jpg`, `questions.json` (the questions Q1-Q8,
their only allowed words, the tiles, what the signals measured) and an
`answers.json` template (kept if it exists):

```
wrote 17 shot strip(s) and detail crop(s), an overview and the questionnaire to analysis/vision (35 images, 13.4 megapixels)
1. look at analysis/vision/overview.jpg
2. for each shot, look at its strip (and its detail crop for textures, borders and glows):
   shot  3 frames 36-48 needs_agent shot03.jpg  not tracked (needs_agent): ...; exposure flashes: frame 36 4.6 stops
3. answer the questions in analysis/vision/questions.json by editing analysis/vision/answers.json (fixed words only; unsure when in doubt)
4. edits vision merge <analysis> <answers.json>, then draft
```

`merge` refuses any word outside the vocabulary (the error lists the valid
ones, nothing is written), checks each label against the signals and writes
the labels into the analysis (in place, or `-o`):

```
labelled 17 shot(s); 0 still all unsure
50 label(s) confirmed by the signals, 7 to look at again
  look again: shot 13: framing cube, but the track held as a flat 2D move
slot roles: p01=full_art, p02=full_art, p03=panel_colour, ... p10=emblem, p12=close_up
wrote analysis/reference_analysis.json
```

The vocabulary and what `draft` does with each word: [ANALYSIS.md](ANALYSIS.md#the-vision-pass).

## probe

```
edits probe <video> [--json] [--height 360]
```

One line per frame: mean and centre luminance, contrast, saturation, a
sharpness index and the difference from the previous frame. Cheap (about two
seconds for 17 seconds of video) and useful for finding flashes and cuts by eye.

## fit

```
edits fit <project.json> -r <dir>/reference_analysis.json [--shots 3,7] [--params auto|exposure,blur,...] [--budget 24] [--scale S] [--dry-run] [--json]
```

Tunes the project's numbers so that its render matches the reference recipe
in signature space, with whatever pictures are in the slots. For each shot it
renders that shot's frames at low resolution, measures them with the same
analysis `compare` uses, and searches the shot's parameters: flash peaks and
`decay` rates, saturation amounts, threshold level, blur radii, scale and
position values and their eases. It never moves key times and never changes
pictures, rotation or inversions. The project is written only if the
whole-clip score does not get worse, through the shared edit history (one
entry, undoable from the window or with `project_undo`); if the file changes
while the fit runs, nothing is written. A 17-second edit takes about six
minutes on four CPU cores. Details and measured accuracy: [CALIBRATION.md](CALIBRATION.md).

```
$ edits fit edit/project.json -r an/reference_analysis.json --dry-run
  shot  2 shot02     frames   25..  43: 9 params, 10 renders, objective 1.734 -> 1.576
  shot02 exposure_decay: exposure.stops key 2 decay rate: 0.21 -> 0.394
```

## trace

```
edits trace <reference> --arts <file|dir>... -o <project.json> [--window NAME=A-B,...] [--proxy NAME=FRAME:X,Y,W,H --fill NAME=FILE] [--min-ncc 0.5] [--extend 0] [--ignore dark,red] [--width W --height H] [--no-audio] [--preview] [--json]
```

The start of the trace method ([ANALYSIS.md](ANALYSIS.md#trace-or-recipe)):
for a reference that shows pictures you have. Finds each art (a quick
keypoint pass over every third frame, unless `--window` says where), tracks
it, measures its tone curves and blur, finds the beat grid of the
reference's sound and writes a project: one image layer per run of tracked
frames with `transform.pin` keyed per frame, `curves` and `gaussian_blur`
on it, cuts moved onto the beat when within 1.5 frames, the reference's
sound as `audio`. `--proxy` takes a rectangle of a reference frame as a
template for a plane that is not among the arts (it is never rendered);
`--fill` puts your picture on that plane. `--preview` renders the draft,
scores it like `metric` and writes `trace/worst.jpg` and
`trace/side_by_side.mp4`. Beside the project, `trace/` holds `track.json`
and `grade.json`.

```
$ edits trace ref.mp4 --arts arts -o draft/project.json --preview
a10_1: a10 frames [81,119] ncc 1.0
a12_1: a12 frames [0,40] ncc 0.88
a04_1: a04 frames [41,80] ncc 0.94
a18_1: a18 frames [12,40] ncc 0.99
not found in the reference: a05
draft SSIM 0.976 (author pixels 0.4104), worst frames [58,50,54,55,57,59,51,52]
sheet "draft/trace/worst.jpg", side by side "draft/trace/side_by_side.mp4"
wrote draft/project.json
```

(A 5-second synthetic reference made with the engine from four of the
arts: the draft scores 0.976. The author-pixel share is high here because
the "reference" was rendered from the very same files.)

## track

```
edits track <reference> --art <png>... [--window NAME=A-B,...] [--min-ncc 0.5] [--extend 0] [--max-gap 8] [--smooth 5] [--ignore dark,red] [--features 1500] [-o track.json] [--sheet quads.jpg] [--json]
```

Per art and frame, a homography found by keypoints and RANSAC, refined on
pixels (ECC-style), checked by correlation; gaps filled, corners smoothed.
Without `-o` the track (`edits.track/1`) goes to stdout. `--sheet` draws the
quads on the frames (red: filled). The `corners` of a key are the value of
a layer's `transform.pin` at `t`.

```
$ edits track pin.mp4 --art a04.png -o track.json
a04: 48 keys (48 found, 0 filled), 0 missing, mean ncc 0.995
wrote track.json
```

## measure

```
edits measure grade <reference> --track track.json [--points 9] [--every 1] [-o grade.json]
edits measure blur <render> <reference> [--frames 100-140,200] [--min-gain 0.01] [-o blur.json]
```

`grade`: per tracked key, red, green and blue tone curves (`curves` points)
that turn the clean picture into its look in the reference, and the
Gaussian blur that explains its softness; `arts[].effects` are ready to
paste. `blur`: per frame, the `gaussian_blur`, `zoom_blur` and
`directional_blur` values that bring the render closest to the reference
(frames where blur does not help by `--min-gain` are left out).

## metric

```
edits metric <render> <reference> [--pre-blur 2] [--worst 12] [--sheet worst.jpg] [--project project.json] [--side-by-side sbs.mp4] [--block NAME=A-B]... [-o frames.json] [--json]
```

The render is decoded at the reference's size and frame rate. Per frame:
SSIM of the luma after a Gaussian of `--pre-blur` px, the colour error and
the author-pixel share (mid-tone pixels equal to the reference's within
4/255; above 15% means reference pixels may be leaking in). `--sheet`: the
worst frames as reference | ours | difference. `--project`: each worst frame
lists the layers visible at it. `--side-by-side`: reference | render, with
the reference's sound.

```
$ edits metric draft/trace/draft.mp4 ref.mp4 --project draft/project.json --worst 4 --sheet w.jpg
120 frames at 576x576, pre-blur 2 px
SSIM mean 0.976, min 0.952; colour error 3.8/255
author pixels: mean 41.4%, max 70.6%, 80 frames over 15%
worst frames:
  f58   ( 2.42 s)  SSIM 0.952  colour error 4.8
  f50   ( 2.08 s)  SSIM 0.953  colour error 4.5
  f54   ( 2.25 s)  SSIM 0.953  colour error 4.7
  f57   ( 2.38 s)  SSIM 0.953  colour error 4.8
sheet: w.jpg
  f58 layers: ["a04_1"]
  f50 layers: ["a04_1"]
  f54 layers: ["a04_1"]
  f57 layers: ["a04_1"]
```

## audio

```
edits audio <song or video> [-o song.json] [--bpm N] [--beats-per-bar 4] [--project project.json] [--json]
```

Analyses the soundtrack for cutting to it: the tempo and a regular beat grid
(with the confidence of the estimate), the bars, onsets in three bands with
their strengths (low 30–150 Hz `kick`, mid 200–3500 Hz `snare`, high
6–11 kHz `hat`), the loudness of each beat (0..1), sections where the
loudness changes, and the drop, the largest rise in loudness, on a downbeat.
It is plain signal processing over one Fourier transform (no model, no GPL
code); the band onsets are what each band's flux says, not a drum
transcription. With `-o` the whole analysis is written as JSON (format
`edits.audio/1`: every time in seconds `t` and in beats `b`); `--json` then
prints a summary, and without `-o` it prints the analysis itself.

`--project` also writes the song's tempo into the project
(`settings.bpm`; beat 0 moves to the song's beat nearest to where it was),
its markers (`drop` and the sections) and, when the project has no audio, the
song as its audio: one undoable edit. After that, `--at drop` and `bar 9`
work in every command.

```
$ edits audio audio/beat.wav -o song.json --project project.json
audio/beat.wav: 3.90 s, 123 BPM (confidence 0.76), beat 0 at -0.006 s, 9 beats, 2 bars of 4 (bar 1 on beat 2, confidence 0.03)
onsets: 8 kick, 16 snare, 10 hat; 100% of kicks and snares on the grid
drop: none found
sections: high 0.00-3.90 s (bar 1, 2 bars, energy 0.83);
loudness: -15.5 dBFS RMS, peak -1.1 dBFS
note: the bar phase is a guess (confidence 0.03): bar 1 may start on another beat; check by ear
note: no clear drop: the energy does not rise sharply anywhere
wrote song.json in 0.1 s
project.json: tempo 123 BPM, beat 0 at -0.006 s
```

The bar phase comes from where the kicks, the backbeat snares and the
loudness changes fall; a loop with a kick on every beat is ambiguous (the
note says so). Loudness is RMS in dBFS, not LUFS.

## presets

```
edits presets [<name>] [--tag hit] [--project project.json] [--json]
```

Lists the presets: named building blocks that expand into ordinary keys,
effects and layers. 22 are built in (files in `presets/`), one
`transition:<kind>` per transition, and a project's own in
`<project dir>/presets/` with `--project`. With a name, every variable:

```
$ edits presets shake
shake: Camera shake on the hit that dies away: the picture jitters and rolls, zoomed in just enough that its edges never show.
  tags: hit, camera, shake, accent; target: a layer (--layer id, auto or fx)
  amplitude (number, default 0.03, 0..0.5) How far it jitters at the start, in frame heights.
  decay (number, default 1, 0.05..8, beats) Time to fall back to the picture.
  frequency (number, default 14, 1..60) Jitters per second.
  rotation (number, default 2, 0..45) How far it rolls at the start, in degrees.
```

Built in: `flash`, `flash_burn`, `punch`, `zoom_out`, `push_in`, `shake`,
`rgb_split`, `blur_in`, `blur_out`, `bw_hit`, `invert_hit`, `threshold_hit`,
`slide_in`, `slide_out`, `flip_3d`, `card_fall`, `cube_turn`, `spin`, `velocity`, `echo_hit`,
`glow_pulse`, `vignette_pulse`, `overlay_texture`, `letterbox`, the masks
`iris_in` (a circle opening on the picture) and `split_panels` (a slanted
split-screen panel), the
particle layers `sakura`, `sparkles`, `dust`, `beat_burst`, the shape layers
`speed_lines` (anime speed lines, radial or horizontal), `burst_lines` (manga
focus lines on a beat) and `ring_pulse` (a ring bursting out); in, out
and combo animations `fade_in`, `fade_out`, `bounce_in`, `drop_in`,
`rotate_in`, `swirl_in`, `wipe_in`, `squish`, `swing`, `rock`, `pendulum`,
`pulse_loop`, `party_rotate`, `party_flip`; time presets `freeze`,
`boomerang`, `hesitate` and the CapCut speed curves `velocity_montage`,
`velocity_hero`, `velocity_bullet`, `velocity_jump_cut`, `velocity_flash_in`,
`velocity_flash_out` (they retime keyed motion or a video's playback, and push
a still picture); and the text animations `typewriter`, `pop_in`,
`random_flicker`, `wave`, `fade_letters`, `flip_letters`, `slide_letters`,
`jitter_text`, `explode`, `word_pop`, `line_reveal`, `gradient_text`,
`scramble` (target: a text layer; `auto` takes the text layer on screen at
`--at`). The file format is in [FORMAT.md](FORMAT.md#presets-editspreset1).

## apply

```
edits apply <project> <preset> [--layer <id>|auto|fx] [--at <time>[,<time>...]] [--var name=value]... [--audio song.json] [--dry-run] [--json]
```

Applies a preset as one validated step of the project's history (the window's
undo, or the MCP `project_undo`, takes it back). `--layer auto` (the default)
picks the picture on screen at each time; `fx` uses an adjustment layer
`accents` on top (created when missing), so the preset changes everything
beneath; a transform preset on an adjustment layer uses `transform2d`
instead. `--at` defaults to the layer's `in`, which is what a
`transition:<kind>` wants: given the incoming layer, it overlaps the cut.

```
$ edits apply project.json transition:whip_pan --layer cut5 --var duration=0.5
at 4: set cut4/out = 4.25; set cut5/in = 3.75; set cut5/transition = { "kind": "whip_pan", "params": { "blur": 0.4, "direction":…
wrote project.json: 3 change(s) (undo restores the previous version)
```

A second hit of the same kind goes into the same effect's keys; keys before
the hit are held until it, keys after it are kept. Unknown presets,
variables and times are refused with the list of what exists; a result that
would not pass `check` is not written (exit 1).

## save-preset

```
edits save-preset <project> --layer <id> --from <time> --to <time> --name <name> [--description "..."] [--force] [--json]
```

Turns what a layer does between two times (its keyed transform and effect
parameters, and its constant effects) into a preset in
`<project dir>/presets/<name>.json`, with times as offsets from where it is
applied. Values are copied as they are; add `variables` to the file by hand
to make it adjustable.

## beats cut

```
edits beats cut <project> --slots a,b,c [--every 1 | --on kicks] [--from T] [--to T] [--audio song.json] [--above <id> | --below <id>] [--prefix cut] [--dry-run] [--json]
```

Adds one image layer per step between `--from` and `--to`, cycling through
the slots: every `--every` beats (4 = a bar, 0.5 = twice a beat), or on each
`beats`, `offbeats`, `downbeats`, `kicks`, `snares` or `hats` of a song
analysis, or on a list of times. Layers are named `cut1`, `cut2`, ... (after
the last existing one) and go on top, below the adjustment layers at the top
of the stack, unless `--above` / `--below` says otherwise. A range past the end
of the project makes it longer.

```
$ edits beats cut project.json --slots a,b,c,d --to 16 --below tex
16 layer(s): cut1=a@0..1 cut2=b@1..2 cut3=c@2..3 cut4=d@3..4 cut5=a@4..5 ... cut16=d@15..16
the project now runs to 16
wrote project.json: 17 change(s) (undo restores the previous version)
```

## beats accent

```
edits beats accent <project> --preset <name> --on beats|offbeats|downbeats|kicks|snares|hats|drop|<times> [--audio song.json] [--layer auto|fx|<id>] [--var name=value]... [--from T] [--to T] [--min-strength 0.3] [--every-nth N] [--dry-run] [--json]
```

Applies a preset at every time of `--on`, as one step. Beats and downbeats
come from the song analysis when given, else from the project's grid;
onsets and the drop need `--audio`. Onsets within 35 ms of a sixteenth note of
the project's grid are put on it, so a kick a few milliseconds before a cut
belongs to the picture after the cut.

```
$ edits beats accent project.json --preset punch --on kicks --audio song.json
punch on 8 time(s): cut1@0 cut2@1 cut3@2 cut4@3 cut5@4 cut6@5 cut7@6 cut8@7
wrote project.json: 8 change(s) (undo restores the previous version)
```

## transitions

```
edits transitions [--json]
```

The transitions a layer can come in with (its `transition` field), their
default length in beats and their parameters. See
[EFFECTS.md](EFFECTS.md#transitions).

```
$ edits transitions
A transition sits on the incoming layer: "transition": { "kind", "duration", "params", "ease" }; the preset transition:<kind> sets it and overlaps the cut.
fade (0.5 beats): A plain crossfade.
fade_color (1 beats): Dip through a colour (black by default) between the pictures.
    color          default [0.0, 0.0, 0.0, 1.0] Colour to dip through (sRGB, with alpha).
    color_phase    default 0.4                  How long the colour holds in the middle.
...
```

## shader check

```
edits shader check <file.wgsl> [--json]
```

Compiles a custom effect the way the renderer will, without a GPU, and
reports problems at their line in the file; exits 1 when it would not
compile. See [EFFECTS.md](EFFECTS.md#errors).

```
$ edits shader check shaders/bad.wgsl
error: shaders/bad.wgsl:3:24: no definition in scope for identifier: `nope`
    return c * fx.p0 + nope;
```

## gui and mcp

```
edits gui [project.json] [options]
edits mcp [--root DIR]
```

Start the desktop app ([GUI.md](GUI.md)) or the MCP server for agents
([AGENTS.md](AGENTS.md)). They are separate executables, so the main binary
carries no windowing, audio or async code; these subcommands find them next
to `edits` (or on `PATH`, or through `EDITS_STUDIO_BIN`, `EDITS_GUI_BIN`,
`EDITS_MCP_BIN`) and pass the arguments on.

`edits gui` starts Edits Studio (`edits-studio`, built from `studio/`) when
it is there, and the egui window (`edits-gui`) when it is not, when
`EDITS_GUI=egui` is set, or when an option only the egui window has is given
(`--screenshot`, `--delay`, `--size`, `--scale`, `--play`, `--cache-mb`,
`--adapter`); `EDITS_GUI=studio` insists on the studio. The studio takes
`--page`, `--select` and `--at`. When neither is built the error says how to
build them. `edits mcp` tells the
server to use this `edits` binary.
