# The `edits` command

```
cargo build --release -p edits-cli      # the binary is target/release/edits
```

Every command works from a plain shell with no window or display. Results that
are data go to **stdout** as JSON; progress and error messages go to **stderr**;
a failure always ends with a non-zero exit code and a line starting `error:`.

Times on the command line (`--from`, `--to`, `--at`) are in the project's
`time_base`: beats for a beats project, seconds otherwise. Frame numbers start
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
| [`compare`](#compare) | Score a video against a recipe in signature space (any pictures) |
| [`probe`](#probe) | Per-frame signatures of a video |
| [`fit`](#fit) | Tune a project's effect values until its render matches a reference |
| [`gui`](#gui-and-mcp) | Open the preview window (`edits-gui`) |
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

Prints `edits_core::describe`: size, length, slots, and one block per layer
with its keys and effects, in a few hundred tokens. Read this before editing a
project.

## check

```
edits check <project.json> [--json]
```

Validates the project with its own folder as the base for relative paths, so
missing pictures, shaders and audio are found. Exit code 1 when there is at
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

The registry from `edits-core`: every effect kind with its parameters, their
defaults and what they mean. `--json` gives
`[{ "kind", "doc", "params": [{ "name", "default", "doc" }] }]`.
[EFFECTS.md](EFFECTS.md) has the details.

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
edits draft <dir>/reference_analysis.json -o project.json [--proxies] [--width W --height H] [--no-snap] [--single-loop] [--json]
```

Writes a project that replays the analysed recipe: beats time base, the
reference's audio, one slot per picture (empty, or with `--proxies` the
reference's own stills so it renders at once), one layer per shot with its
measured motion and effects, the overlay and the vignette. Key times snap to
the quarter beat unless `--no-snap`. The project passes `check`, or the
command fails.

## compare

```
edits compare <dir>/reference_analysis.json <video> [--json] [--top 25]
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

## gui and mcp

```
edits gui <project.json> [edits-gui options]
edits mcp [--root DIR]
```

Start the preview window ([GUI.md](GUI.md)) or the MCP server for agents
([AGENTS.md](AGENTS.md)). They are separate executables, `edits-gui` and
`edits-mcp`, so the main binary carries no windowing, audio or async code;
these subcommands find them next to `edits` (or on `PATH`, or through
`EDITS_GUI_BIN` / `EDITS_MCP_BIN`) and pass the arguments on. When one is
missing the error says which `cargo build` produces it. `edits mcp` tells the
server to use this `edits` binary.
