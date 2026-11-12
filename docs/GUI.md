# The window (`edits-gui`)

`edits-gui` shows a project while something else edits it. Open it on a
project file, let an agent (or a text editor) change that file, and watch the
change land: the preview, the timeline and the inspector follow the file.

The window is read-only. Nothing in it changes the project; there is no
editing with the mouse in this version. The JSON file stays the only source of
truth, and the window is one more reader of it, next to `edits render`.

```
cargo build --release -p edits-gui      # the binary is target/release/edits-gui
edits-gui demo/project.json
```

```
edits-gui <project.json> [--scale 0.25|0.5|1] [--at FRAME] [--play] [--select LAYER]
                         [--size WxH] [--cache-mb N] [--adapter auto|hardware|software]
                         [--screenshot out.png [--delay SECONDS]]
```

The project file must exist, but it does not have to be valid: a file with
errors opens a window that lists them and waits for the file to change.

Other programs can open the same window with
`edits_gui::run(project: PathBuf, options: edits_gui::Options) -> anyhow::Result<()>`.

## Keys

| Key | What it does |
| --- | --- |
| Space | Play or pause |
| Left / Right | One frame back / forward (pauses) |
| Shift+Left / Shift+Right | One beat back / forward: to the first frame of the previous / next whole beat (one second in a seconds project) |
| Home / End | First / last frame |
| L | Loop on or off |
| Ctrl + wheel over the timeline | Zoom about the pointer |
| Shift + wheel over the timeline | Scroll sideways (so does a sideways swipe) |

Keys are ignored while the export dialog's text field has the focus.

## What each part shows

**Transport** (top). Start, step back, play/pause, step forward, Loop, and the
position three ways: frame number, seconds, and beats when the project counts
in beats. Under it, a scrub bar over the whole project. Then the preview
quality: the scale (0.25, 0.5 or 1.0 of the project's resolution) and whether
the preview uses the project's motion blur, separately for *paused* (on by
default) and *playing* (off by default, because motion blur multiplies the
cost of a frame). `Export…` is at the end of the row.

**Preview** (centre). The frame under the playhead, rendered by
`edits-render` exactly as `edits frame` would render it at that scale. When
the picture on screen is not the frame the transport reads (a slow renderer
during playback, or a frame still rendering after a scrub), a note in the
corner says which frame it is. A red banner appears when the file on disk has
errors (see *Live reload*), and a green frame flashes when a change has been
loaded.

**Timeline** (bottom).

- A ruler in the project's time base. In a beats project it counts beats from
  0, with a strong line every four beats (the format has no time signature;
  the grid assumes 4/4) and lines on half and quarter beats when zoomed in.
  In a seconds project it counts seconds.
- The project's `markers`, as orange flags with their labels.
- The waveform of `audio`, placed by `audio.offset`.
- One row per layer, the top of the picture first (the last layer in the file
  is the first row). Each row has the layer's id and kind, a bar from `in` to
  `out`, the names of its effects (and its blend mode when it is not
  `normal`), and its keyframes: a yellow diamond where a transform property
  has a key, a cyan dot where an effect parameter has one. Hover a keyframe to
  read its time and the properties keyed there. A `camera` row appears above
  the layers when the camera is animated. Disabled layers are dimmed.
- The playhead, in red.

Click or drag on the ruler (or the marker and waveform lanes) to move the
playhead. `−`, `+` and `Fit` zoom; when zoomed in, the bar at the bottom
scrolls, and the view follows the playhead during playback. Drag the top edge
of the panel to resize it.

**Inspector** (right). Click a layer's row to see what it evaluates to at the
playhead: kind, slot or faces, `in..out` (also in frames), blend, whether it
is visible now; position, scale, rotation, anchor and opacity as the
`Evaluator` resolves them; and for each effect every parameter of its spec,
with the ones the file does not set marked `default`. Animated values say how
(`3 keys`, `shake`, `loop`). The numbers change as the playhead moves, so this
is where to check that a value an agent set is the value the renderer uses.
With no layer selected it shows the project's settings and the camera.

**Problems** (above the inspector). The findings of `edits check` for the file
as it is on disk: errors and warnings, each with the path into the document,
the message and the hint.

**Status bar** (bottom edge). The revision (a counter of loaded versions) or
`reloaded` just after a change; the renderer's adapter and whether it is a
hardware GPU or a software rasteriser; the time the last frame took to
render; the frame cache; the state of the audio; the progress of an export.

## Live reload

The window watches the project file and every file it names (slot pictures,
the audio file, custom shaders) with file-system notifications, plus a
once-a-second check of sizes and times for folders where notifications do not
arrive. A burst of writes becomes one reload (0.12 s of quiet, at most 1 s).

On a change the file is parsed and checked.

- **Valid** (warnings allowed): the new version replaces the old one. The
  revision goes up, cached frames are dropped, the playhead stays where it
  is, playback continues, and `reloaded` flashes in the preview and the
  status bar. Replacing a picture without touching the JSON counts too.
- **Not valid** (not JSON, wrong shape, or errors from `check`): nothing is
  replaced. The preview keeps showing the last version that was valid, a red
  banner and the status bar say so, and the Problems panel lists what is
  wrong. The next valid save clears it.

## Playback

The playhead follows a real-time clock, not the renderer. When frames take
longer than their slot, the preview shows the newest frame that is finished
and skips the rest; it never slows down. The renderer is asked for a frame a
little ahead of the playhead (as far ahead as a frame takes), so the picture
is close to the playhead when it appears.

Rendered frames are cached (384 MB by default, least recently used first
out; `--cache-mb` changes it), keyed by revision, frame, scale and motion
blur. Scrubbing back is immediate, and a short loop plays smoothly once it
has gone round: during playback the renderer fills in the frames it skipped.
At scale 0.5 a 1080 x 1080 frame is 1.2 MB, so the default holds about 330
frames; at scale 1.0, about 80.

On four CPU cores with Mesa lavapipe (no GPU) and a release build, the
1080 x 1080 demo takes about 40 ms per frame at scale 0.5 without motion blur
and about 110 ms at scale 1.0: at 0.5 the first pass of the 25 fps loop
already shows most frames, at 1.0 every third, and later passes show all of
them once they are cached. A debug build renders about twice as slowly.

## Audio

When the project has `audio`, the file is decoded once by the system `ffmpeg`
(`EDITS_FFMPEG` names another binary) to 48 kHz stereo and played through the
default output device during playback, starting at `audio.offset` plus the
playhead, with `audio.gain` applied. Seeking and looping restart it at the
right place.

If there is no sound device, no `ffmpeg`, or the file cannot be decoded,
playback carries on silently and the status bar says `audio: silent` with the
reason (the full message is in its tooltip).

## Export

`Export…` asks for an output file (default: next to the project, named after
its folder) and runs `edits render <project> -o <file>` as a separate
process. Progress and the result appear in the dialog and the status bar;
`Cancel` stops it. The `edits` command is looked for in `EDITS_BIN`, then next
to `edits-gui`, then on `PATH`. It renders the file as it is on disk, at full
size, with the project's motion blur.

## Screenshot mode

For automated checks, by an agent or in CI:

```
edits-gui demo/project.json --screenshot shot.png --at 37 --size 1400x880
```

opens the window, waits until the preview of frame 37 is on screen (rendered
at the paused quality, waveform included), saves a picture of the whole
window and exits with code 0. It exits with code 1 and a line starting
`error:` if the picture could not be taken (after three minutes at most).
If the project is not valid, the picture shows the Problems panel.

`--delay SECONDS` keeps the window running that long before the picture is
taken, and then waits for the preview again. That is how to check live
reload: start it with a delay, change the project file meanwhile, and look
for the new picture, the revision number and the Problems panel in the
screenshot. `--select <layer id>` opens a layer in the inspector, `--play`
starts playback, `--scale` sets the preview scale.

A window needs a display. On a machine without one:

```
xvfb-run -a -s "-screen 0 1600x1000x24" edits-gui demo/project.json --screenshot shot.png
```

## How it is built

- `eframe`/`egui` 0.37 with the **glow (OpenGL)** backend, on X11 and Wayland.
  OpenGL was chosen over eframe's wgpu backend because the window only draws
  panels and one texture: OpenGL does that on every Linux desktop, including
  under Xvfb with Mesa's llvmpipe, and it keeps the window's graphics context
  separate from the renderer's, so a slow or failing Vulkan driver cannot take
  the interface down with it.
- The renderer runs on its own thread with its own wgpu device. The window
  posts the frame it wants into a one-place mailbox; a request that is
  overtaken before the renderer gets to it is dropped. Frames come back as
  images and are uploaded as one texture.
- The logic without a window in it is unit tested (`cargo test -p edits-gui`):
  time and ruler arithmetic (`timemap`), the clock and frame skipping
  (`clock`), the frame cache (`cache`), keyframe collection (`rows`), the
  reload debounce and state machine and the file watcher (`reload`), the
  mailbox and render thread (`worker`), waveform peaks (`audio`), the export
  process (`export`).

## System packages

To build: a Rust toolchain (1.95 or newer), a C linker, `pkg-config` and the
ALSA headers. To run: OpenGL (any Mesa or vendor driver), the X11 or Wayland
client libraries, xkbcommon, ALSA, and for the renderer a Vulkan driver (a
GPU's, or Mesa lavapipe) as for `edits render`. `ffmpeg` is needed for audio
and for Export.

| | Build | Run | No display (Xvfb) |
| --- | --- | --- | --- |
| Ubuntu / Debian | `build-essential pkg-config libasound2-dev` | `libgl1 libegl1 libgl1-mesa-dri libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libx11-6 libxcursor1 libxi6 libxrandr2 libasound2t64 mesa-vulkan-drivers ffmpeg` | `xvfb xauth` |
| Fedora | `gcc pkgconf-pkg-config alsa-lib-devel` | `mesa-libGL mesa-libEGL mesa-dri-drivers libxkbcommon libxkbcommon-x11 libwayland-client libX11 libXcursor libXi libXrandr alsa-lib vulkan-loader mesa-vulkan-drivers ffmpeg-free` | `xorg-x11-server-Xvfb xorg-x11-xauth` |
| Arch | `base-devel pkgconf alsa-lib` | `mesa libxkbcommon libxkbcommon-x11 wayland libx11 libxcursor libxi libxrandr alsa-lib vulkan-icd-loader ffmpeg`, and a Vulkan driver (`vulkan-radeon`, `vulkan-intel`, `nvidia-utils` or `vulkan-swrast`) | `xorg-server-xvfb xorg-xauth` |

A desktop install of any of these already has the run-time libraries; the
list matters for containers and minimal systems. On Ubuntu releases before
24.04 `libasound2t64` is called `libasound2`.

## Known limits

- Read-only: no selecting in the preview, no dragging keys or layers.
- Audio follows the wall clock, not the sound card's clock, and the output
  latency is not compensated: expect the picture and the sound to agree to
  within a few tens of milliseconds, not to the sample. Audio playback could
  not be listened to where this was developed (no sound device there): the
  decode, the waveform, the silent fallback and playback into an ALSA `null`
  device were checked, the sound itself and its sync by ear were not.
- Loop always covers the whole project; there is no in/out range.
- The bar grid assumes four beats to a bar.
- Keys of a property with a `loop` modifier are drawn once, where they are in
  the file, not at every repetition. Keys of a keyframed `shake` amplitude are
  drawn as keys of the property that shakes.
- At scale 1.0 the default cache holds about 80 frames of a 1080 x 1080
  project, so a longer loop never becomes fully cached; raise `--cache-mb` or
  preview at 0.5.
- The export dialog has a text field for the path, not a file chooser, and
  exports the whole project at the project's settings (use `edits render` for
  a range or a smaller size).
- Closing the window waits for the frame being rendered at that moment.
- The window layout and settings are not remembered between runs.
- Tested on X11 (under Xvfb, with llvmpipe). Wayland support is compiled in
  and is the same code path in `winit`, but was not run.
