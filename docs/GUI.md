# The window (`edits-gui`)

`edits-gui` shows a project while something else edits it, and lets you
edit it by hand at the same time. Open it on a project file, let an agent (or
a text editor) change that file, and watch the change land: the preview, the
timeline and the inspector follow the file. Drag a value, a layer or a key,
and your change is written to the same file.

The JSON file stays the only source of truth. The window does not keep a
project of its own: every change it makes goes through the same code as the
agent's `project_edit` (`edits_core::edit`), so it is checked before it is
written, written in one step, and kept in the history that both of you undo
from. See [Editing](#editing) and [One history for you and the
agent](#one-history-for-you-and-the-agent).

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
| Ctrl+Z | Undo the newest edit in the project's history, yours or the agent's |
| Ctrl+Shift+Z, Ctrl+Y | Redo |
| Ctrl + wheel over the timeline | Zoom about the pointer |
| Shift + wheel over the timeline | Scroll sideways (so does a sideways swipe) |
| Alt while dragging on the timeline | Snap to frames instead of quarter beats |

Keys are ignored while a text field or a value being typed has the focus
(there, Ctrl+Z undoes typing, and Enter finishes the value).

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

**Menu bar** (top). *File*: Export…, Quit. *Edit*: Undo and Redo, each
naming the edit it would take back or bring back and who made it. *Layer*:
add a layer, and for the selected layer Disable/Enable, Duplicate, Move
up/down, Add effect ▸, Delete.

**Inspector** (right). Click a layer's row to see and edit what it evaluates
to at the playhead: slot (image), colour (solid) or faces and size (cube),
`in` and `out` (also in frames), blend, whether it is visible now; position,
scale, rotation, anchor and opacity; and for each effect every parameter of
its spec, with its unit, and the ones the file does not set marked `default`
(hover a name for its documentation and default). Animated values say how
(`3 keys`, `shake`, `loop`). The numbers change as the playhead moves. With
no layer selected it shows the project's settings and the camera (whose fov,
position and rotation are editable too). See [Editing](#editing).

**Slots** (a second page of the right panel). Every slot with its picture,
the layers that use it, its fit and focus; and a field to add a slot.

**History** (bottom of the right panel). The newest edits first, each tagged
`you` (made in this window) or `agent` (made through the MCP server), with
what it did and how long ago; above a line, the edits that were undone and
can be redone. Undo and Redo buttons. Drag the panel's top edge to resize it.

**Problems** (top of the right panel). The findings of `edits check` for the
file as it is on disk: errors and warnings, each with the path into the
document, the message and the hint. Warnings alone stay folded.

**Status bar** (bottom edge). The revision (a counter of loaded versions) or
`reloaded` just after someone else's change; what your last edit did, or in
red why it was not applied (hover it for every error); the renderer's adapter
and whether it is a hardware GPU or a software rasteriser; the time the last
frame took to render; the frame cache; the state of the audio; the progress
of an export.

## Editing

Every edit is one step: it is checked (`edits check` plus the keys the
renderer would ignore), and only a project without errors is written. The
file is replaced atomically (as stable, readable JSON, the same text the
agent's edits produce) and the version before it is saved under
`<project folder>/.edits/history/`. When an edit would leave an error, the
file is not touched and the status bar says why, for example
`Not applied (slot b = img/nope.png): error /slots/b: image not found`.

**Values in the inspector.** Drag a number sideways, or click it and type
(Enter or clicking elsewhere finishes). The preview follows while you drag or
type, rendered as a temporary version that is never written; when you let go
the change is written once, so a drag is one undo step however long it was.

- A constant (`"scale": 1`) changes: `"scale": 1.25`.
- A keyframed property gets a key at the playhead (the start of the frame
  under the playhead), or that key changes if there is one; its other keys
  stay. A new key continues the ease of the segment it splits.
- An effect parameter the file does not set is written as a constant.
- A shaken property: the value under the shake changes, the shake stays.

The diamond after a property is **Key here**: it turns a constant into
keyframes with one key at the playhead, or adds a key holding the value the
property has there. It is filled when a key is at the playhead. Click a
property's name to *pick* it: the **key strip** under its group then lists
its keys, removes the key at the playhead (*Remove key*; with no keys left the
property is a constant again), and sets the ease of the segment the playhead
is in: any name of `edits_core::ease::NAMES`, `decay` with its rate, or
`bezier` with its four numbers.

**Effects.** *+ Add effect* (in the inspector, or Layer ▸ Add effect) lists
the registry, each with its one-line documentation as a tooltip; the effect
is added after the others with its defaults. Each effect has *on* (enabled),
*up*/*down* (the order they apply in) and × (remove).

**Layers.** Layer ▸ (or *+ Layer* on the timeline) adds an image layer (choose
its slot), a solid, an adjustment layer or a cube (choose the slot on its
faces), one bar long, at the whole beat under the playhead (earlier when it
would run past the end), directly above the selected layer (or on top). The inspector's buttons and the Layer menu
enable or disable, duplicate (a copy directly above, id `<id>-copy`), move one
place up or down in the stack, and delete the selected layer.

**Slots.** On the Slots page, type a path (relative to the project file) and
press Enter, or **drop a picture file on a slot**: the path is written
relative to the project when the picture is inside its folder. Dragging files
over the window opens the page and outlines the slot under the pointer.
Dropping works on X11; on Wayland it depends on the compositor and winit and
may not reach the window (the tooltip says so): type the path there. Fit is a
menu, focus two numbers. *Add slot* creates an empty one.

**Timeline.** Times snap to quarter beats in a beats project; hold Alt to snap
to frames instead (a seconds project always snaps to frames). The header says
which.

- Drag a layer's **bar** to move the layer in time. Its keys move with it
  (every key of its transform and effect parameters, a shake envelope's keys
  and a loop's start), so its animation stays on the layer.
- Drag its **left or right edge** (the upper half of the bar) to change `in`
  or `out`. A layer stays at least one frame long. A layer without `out` gets
  one.
- Drag a **keyframe marker** to move every key of that layer at that time.
  A key dropped on another key replaces it.
- **Double-click** a layer's row to add a key of the picked property (scale
  until you pick another) at that time.

While you drag, a white ghost shows where the bar or the keys will land and
the times; nothing is written until you release the button, and then it is
one edit.

**The window's own writes.** After writing, the window reads the file back
at once, so its own edit shows without the green `reloaded` flash and the
playhead stays where it is; only the current frame is rendered again first.
The file watcher then sees the same text and does nothing.

## One history for you and the agent

The window and the agent's MCP server (`project_edit`, `project_undo`) use the
same history, kept next to the project:

```
<project folder>/.edits/history/<name>.<time>.json        the version before an edit
<project folder>/.edits/history/<name>.<time>.meta.json   {"actor": "window", "summary": "add glow to tex"}
<project folder>/.edits/redo/...                          what an undo took back, until the next edit
<project folder>/.edits/undone/...                        a copy of every version an undo replaced
```

- **Undo takes back the newest edit, whoever made it.** If the agent edited
  last, Ctrl+Z in the window undoes the agent's edit; if you edited last, the
  agent's `project_undo` undoes yours (and its answer says it was an edit made
  in the window). The status bar and the History list say whose edit an undo
  took back.
- **Redo** brings back what the last undo took, until the next edit by either
  side; then the redo list is cleared (the undone versions stay in
  `.edits/undone/`). The agent has no redo tool; the window does.
- **Neither clobbers the other.** Each step holds a lock on the history, so
  two writers at the same moment are done one after the other, each with its
  own entry. An edit the window prepared against a version of the file that
  is no longer on disk (the agent wrote meanwhile) is refused: the status bar
  says so, the window loads the new version, and you do it again. A drag in
  progress when the agent's change lands is dropped the same way.
- While the file on disk has errors, the window does not edit it (it would be
  editing a version that is not on disk); Undo still works and may be the fix.
- The last 50 versions are kept.

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
- Edits are made by `edits_core::edit` (shared with `edits-mcp`): ops
  addressed by JSON pointer with layers by id (`/layers/@tex/transform/scale`),
  validation, the atomic write and the history. The window turns gestures into
  those ops in `editing` (pure functions of the project it shows) and writes
  them as actor `window`, with the hash of the file text it prepared them
  against, so a stale edit is refused rather than applied over someone
  else's.
- A value being dragged is previewed by giving the render thread a temporary
  revision of the project with the change applied (pictures stay loaded, so
  this is cheap); the edit is written when the widget lets go.
- The logic without a window in it is unit tested (`cargo test -p edits-gui`):
  time and ruler arithmetic (`timemap`), the clock and frame skipping
  (`clock`), the frame cache (`cache`), keyframe collection (`rows`), the
  reload debounce and state machine and the file watcher (`reload`), the
  mailbox and render thread (`worker`), waveform peaks (`audio`), the export
  process (`export`), and editing (`editing`): snapping, a drag becoming one
  edit, key insertion and removal on animated properties, eases, layer and
  effect ops, paths of dropped pictures, history lines. The history itself is
  tested in `edits-core` (`edit`), including two writers at once.

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

- Editing covers layers, effects, keys, eases, slots and the camera. Not in
  the window: project settings (size, fps, BPM, duration), audio, markers,
  layer ids and names, cube faces, shake and loop modifiers (their values
  under the shake can be edited, not the shake), custom shaders' `source`.
  Use the agent or a text editor for those; the window follows.
- Nothing is selected or moved in the preview itself; edit in the inspector
  and the timeline.
- Camera keys are shown on the timeline but are moved from the inspector
  only (no dragging on the camera row).
- A property with a `loop` modifier gets its key at the playhead's time, not
  at the looped time.
- Rewriting a property writes its numbers tidily (`1.0` becomes `1`, times
  are rounded to 1e-6): the value is the same, the diff shows the line.
- Dropping files was not tested (Xvfb has no drag source); typing the path
  was. On Wayland, dropping may not work at all.
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
- Tested on X11 (under Xvfb, with llvmpipe), driven with `xdotool`: typing
  and dragging values, key here, the ease menu, adding an effect from the
  menu, adding, duplicating and moving layers, dragging a bar, an edge and a
  key marker, double-clicking to key, slot paths, undo and redo, and an
  agent's `project_edit` and `project_undo` while the window was open.
  Wayland support is compiled in and is the same code path in `winit`, but
  was not run.
