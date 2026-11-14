# The desktop app: Edits Studio

Edits Studio (`studio/`) is the desktop app of the engine: a Tauri v2 window
with a React + shadcn/ui interface. It shows a project while something else
edits it, and edits it too. Open it on a project file, let an agent (or a
text editor) change that file, and watch the change land: the preview, the
timeline, the inspector and every page follow the file. Change a value, drag
a bar or apply a preset, and your change is written to the same file.

The JSON file stays the only source of truth. Every change the studio makes
goes through the same code as the agent's `project_edit`
(`edits_core::edit`, actor `window`): it is checked before it is written,
written in one step, and kept in the history that both of you undo from.

It replaces the egui window (`crates/edits-gui`), which is still in the
repository and still built by `cargo build -p edits-gui`; it is described at
the end of this page ([The egui window](#the-egui-window-edits-gui)).
`edits gui` starts the studio when it is built and the egui window
otherwise.

```
edits gui demo/project.json                 # the studio if built, else edits-gui
edits-studio demo/project.json --page overview
```

```
edits-studio [project.json] [--page edit|overview|reference|library|activity|renders]
             [--select LAYER] [--at FRAME]
```

Without a project it opens on a first page with three cards: **Open
project**, **New from pictures + song** and **New from reference**. The
renderer's adapter is chosen by `EDITS_ADAPTER` (`auto`, `hardware`,
`software`), as for `edits render`.

## Design note: why the Edit page looks the way it does

The Edit page copies the layout that the desktop edit apps converged on,
so that someone who has used one of them finds things where they expect:

| | CapCut Desktop | Premiere Pro | DaVinci Resolve (Cut / Edit) | After Effects | Alight Motion | Edits Studio |
| --- | --- | --- | --- | --- | --- | --- |
| Library of media, effects, transitions, text | left panel, tabs across the top | Project and Effects panels, left | Media Pool / Effects Library, left | Effects & Presets panel | "+" sheet | **left browser**, tabs Media · Effects · Transitions · Presets · Text · Shapes · Audio |
| Picture | centre | Program monitor, centre | Viewer, centre | Composition panel | top | **centre viewer** with guides, zoom and transform handles |
| Properties of the selection | right "details" panel, grouped (Basic, Animation, Speed...) | Effect Controls: Motion, Opacity, then each effect, with stopwatches and keyframes | Inspector: Video, Audio, Effects, Transition tabs | Timeline twirl-downs | property sheet per layer | **right inspector**, sections Layer · kind · Transform · Effects · Transition in · Masks · Track matte · Keys |
| Time | full-width timeline at the bottom | timeline at the bottom | timeline at the bottom | timeline at the bottom | timeline at the bottom | **full-width timeline** at the bottom |

The rest follows the same apps:

- **Applying an effect.** Drag it from the library onto a clip (all of
  them) or double-click it to apply it to the selected clip (Resolve's Cut
  page, CapCut). The studio does both, and a drop on the viewer applies to
  the selected layer or the picture on screen. A transition is dropped on
  the clip that should come in through it.
- **Keyframes.** A diamond next to every animatable value (CapCut,
  Premiere, Resolve): hollow when the value is animated elsewhere, filled
  when a key is at the playhead, a click adds or removes that key. Keys
  show as small diamonds on the clip and, when a track is opened (the
  chevron, After Effects' twirl-down), on one lane per property where each
  key can be dragged, picked and deleted. Shapes say the ease that leaves
  the key (diamond linear, square hold, dot eased), as After Effects' key
  icons do. There is no graph editor yet; the Keys section sets the ease.
- **Beats.** CapCut's auto beat and Alight Motion's beat markers put
  marks on the music; here the song analysis (`edits audio`) puts beats,
  downbeats, kicks, snares, sections and the drop on a lane above the
  waveform, and moves snap to them. **Beat cut** and **Accent** are the
  "one picture per beat" and "an effect on every kick" that those apps do
  by hand.
- **Clips** show a strip of their picture, their effects as chips and their
  transition as a shaded wedge at the start, as in CapCut.
- **Snapping** (CapCut's magnet, `N`) to beats, the playhead, markers and
  other clips' edges; off, moves snap to frames.
- **Keys** are the editors' common set: Space, J / K / L, S split, Delete,
  Ctrl+Z, I / O for in and out points, Ctrl + wheel to zoom; `?` lists them.
- **Live preview.** Dragging a slider, a colour or a transform handle
  renders the frame at half size as you drag and writes once when you let
  go, so one drag is one entry in the history.

The look is stock shadcn/ui (`base-nova`, neutral, dark): the timeline and
the viewer are drawn by the studio but only with the theme's tokens
(`--background`, `--muted`, `--border`, `--primary`, `--ring`), so they look
like the rest of the window; no colour of their own.

## Build and run on Linux

The studio needs a Rust toolchain, Bun (or Node), and the WebKitGTK
libraries Tauri uses on Linux. On Ubuntu 24.04 / Debian:

```
sudo apt install build-essential curl wget file pkg-config libssl-dev libxdo-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libsoup-3.0-dev \
  mesa-vulkan-drivers ffmpeg
npm i -g bun                                   # or: curl -fsSL https://bun.sh/install | bash
cargo build --release -p edits-cli             # `edits`, for Renders and `edits gui`
cd studio
bun install
bun run tauri build --no-bundle                # target/release/edits-studio, next to `edits`
cd ..
./target/release/edits demo demo
./target/release/edits gui demo/project.json
```

For playing the project's audio the webview uses GStreamer
(`gstreamer1.0-plugins-good`, and `gstreamer1.0-libav` for MP3 and AAC);
without it the studio plays silently. On a machine without a display, run it
under Xvfb (`apt install xvfb xauth`):

```
xvfb-run -a -s "-screen 0 1400x900x24" ./target/release/edits-studio demo/project.json
```

While working on the interface:

```
cd studio
bun run tauri dev -- -- "$PWD/../demo/project.json"   # Vite with hot reload, debug build, debug panel
bun run typecheck && bun run lint && bun run build     # the frontend alone
cd src-tauri && cargo test                              # the Rust side, without a window
```

`studio/src-tauri` is not a member of the root Cargo workspace (it is in
`exclude`), so `cargo test --workspace` never needs WebKitGTK. It builds into
the workspace's `target/` folder (`studio/src-tauri/.cargo/config.toml`), so
the engine crates are compiled once for both and `edits-studio` lands next to
`edits`. The **debug panel** of tauri-ui (runtime, tracked invokes, events,
log stream) is in dev builds only: Ctrl+D toggles it.

## Layout

The window is shadcn's `dashboard-01` block: an inset sidebar on the left, a
header, and the page.

- **Sidebar.** *Edits Studio*; **Quick Render** (renders the project to
  `renders/<name>.mp4` and opens the Renders page) next to an Undo button;
  the six pages (the Edit page shows a badge with the number of errors in the
  file, Activity the number of edits in the history); *Project files* (the
  project, its audio, the `.edits` folder: open, show in folder, copy path);
  *Command palette* and *Open project…*; at the bottom the renderer (adapter
  name, `software renderer` or `GPU renderer`), whose menu shows the time the
  last frame took, the frame cache, the state of the audio and the theme.
- **Header.** The sidebar toggle, the page's title, `folder / project.json`,
  a `rev N` badge (a counter of loaded versions) and a red `N errors` badge
  while the file on disk has errors; on the right *Search commands…* (Ctrl+K),
  the note for the agent (a speech bubble: *Tell the agent*, a text box,
  **Save note** or Ctrl+Enter, **Clear**; the note goes into `ui.json` as
  `note` and `note_at`) and Undo / Redo, whose tooltips name the edit and who
  made it.

The theme is shadcn's dark neutral theme (`--background`, `--card`,
`--muted`, `--border`, `--primary` of `studio/src/index.css`, as the template
ships them), Inter as the template ships it, Lucide icons. The timeline and
the frame viewer are the only parts drawn by the studio itself; they use the
same tokens. `D` switches between dark and light (the template's shortcut);
the sidebar's renderer menu does too.

## Pages

### Edit

Four parts, the dividers between them can be dragged: the **browser** on
the left, the **viewer** with its transport in the middle, the
**inspector** on the right and the **timeline** across the bottom (see the
design note above). A line at the bottom says what your last edit did
(`d: position → [0.2, 0, 0]`) or why it was refused, and `?` opens the list
of keys.

SCREENSHOT_EDIT

**Browser.** Tabs *Media, Effects, Transitions, Presets, Text, Shapes,
Audio* and a search field (every word must match a name, a category or a
description). Items are cards: a small preview and one line saying what the
item does (hover for the whole text). The preview is the item rendered by
`edits-render` on a sample project of two generated pictures, 192 × 108: an
effect with its registry defaults (when they change almost nothing, each
parameter in turn at a telling value, 70 % of its range, and the clearest
result is kept), a preset applied at beat 1 and shown
just after (a text preset on a text layer), a transition half way. They are
made when a card first scrolls into view, one at a time on a thread with a
renderer of its own, and kept in `$XDG_CACHE_HOME/edits-studio/thumbs/v2/`
(else `~/.cache/...`), so each is made once per machine; a grey box shows
until it is ready. **Double-click** a card to apply it to the selected layer
at the playhead (with nothing selected: to the picture on screen); **drag**
it onto a clip of the timeline (that clip) or onto the viewer.

- *Media*: the project's slots with their pictures, how many layers use
  each, and a menu: **Replace picture…** (a file dialog; the slot's `path`
  changes, every layer that shows it follows) and *Add at b4*. **Add
  pictures…** adds files as new slots and image layers at the playhead.
  Double-clicking a slot adds an image layer of it.
- *Effects*: the registry by category (those that need a file, `custom`
  and `lut`, are left out).
- *Transitions*: every transition; applying one writes the
  `transition:<kind>` preset at the target clip's cut (`align: after`), so
  the clip comes in through it and the clip before stays until it is done.
- *Presets*: hits, zooms, motion, looks, 3D... (presets that add their own
  layers say *adds layer*).
- *Text*: **Title** adds a text layer one bar long at the playhead and
  selects it; the text animations (`pop_in`, `typewriter`, ...) go on the
  selected text layer from its start.
- *Shapes*: rectangle, ellipse, star, line and particle layers, and the
  ready-made shape and particle presets.
- *Audio*: the song, the analysis summary, **Analyse song**, **Beat cut…**,
  **Accent…** and the song's sections (click one to go there).

**Viewer.** The frame under the playhead, rendered by `edits-render` exactly
as `edits frame` renders it at that scale, on a worker thread, sent to the
page as JPEG and drawn on a canvas. While paused it renders the frame you are
on (with the project's motion blur unless *Blur* is off); while playing it
asks for a few frames ahead of the playhead and shows the newest one that is
ready. Frames are cached by revision, frame, scale and blur (384 MB of JPEGs
on the Rust side, 240 decoded frames in the page). At the top right:
transform handles on or off, guides (rule of thirds, 90 % and 80 % safe
areas) and the zoom (*Fit*, 50 %, 100 % of the project's pixels, then the
stage scrolls). The selected layer has a box with handles: drag inside it to
move it (Shift: along one axis), a corner to scale it, the knob above it to
turn it (Shift: 15° steps); the value shows next to the centre. Values are
in composition units (the frame is 1 high), as in the file. While you drag,
the frame is rendered with the new value at half the preview size, without
writing anything (`preview_prop`: the project with that one change, given to
the render thread ahead of every other frame, never cached); on release the
value is written once, as the inspector writes it: a key at the playhead
when the property is keyed, else the constant. A red alert at the top says
when the file on disk has errors; the last valid version stays on screen.

**Transport.** The time, the beat (`b4.92`, hover for the bar) and the frame;
start, a frame back, play/pause, a frame forward, last frame; loop; motion
blur while paused; the preview scale (0.25, 0.5 or 1 of the project's size).
Playback follows a real-time clock; the project's `audio` plays from
`audio.offset` plus the playhead (through the webview's `<audio>`, so through
GStreamer on Linux) and is re-synced on every seek and loop.

**Inspector.** With nothing selected: an empty state, the file's problems,
and the project's settings with a button for the camera. With a layer, its
id, kind, an on/off switch and a menu (duplicate, bring forward, send
backward, delete), then collapsible sections:

- *Layer*: in and out (beats or s), blend (every mode), opacity, visible
  (off: drawn only as a track matte).
- The kind's own: *Picture* (image, video: the slot as a picture, a slot
  select, **Replace picture…**; for video speed, time remap, frame blend,
  reverse, loop, volume; posterize fps), *Text* (the words, font, size in
  %h, weight, align, tracking, line height, fill, stroke) and *Animators*
  (each with its selector, on/off, remove), *Shapes* (each shape with fill,
  stroke and width, size or points, its operators, on/off, remove; + adds a
  rectangle, ellipse, star or line), *Particles* (sprite, rate, life, speed,
  direction, spread, size, drag, seed, gravity, colour), *Solid* (colour),
  *Cube* (a slot per face, size W H D), *Comp* (comp, offset, speed, loop).
- *Transform*: position, scale, rotation, anchor.
- *Effects*: a card per effect in the order they apply; drag a card by its
  handle to reorder, the switch turns it off, the menu moves or removes it,
  the chevron folds it; + adds one from the registry.
- *Transition in*: kind, duration, ease and what it does; or *Add
  transition…*.
- *Masks*: each mask's mode, feather and inverted; + adds an ellipse,
  rectangle or rounded mask. *Track matte*: the source layer and mode.
- *Keys*: the keys of the property whose name you clicked: time (click to
  go there), value, the ease that leaves it (a select), remove.

Every animatable value is a row: its name (dim when the file does not set
it; hover for its documentation, default and keys; click to pick it for
*Keys*), its value at the playhead (a slider and a number for values with a
range, a field per axis with X / Y / Z inside it otherwise, a swatch for
colours that opens R G B (A) sliders and a hex field) with its unit (`%h`,
`°`, `×`, `beats`), and the **key diamond**: filled when a key is at the
playhead (click removes it), outlined when the value is animated, faint when
it is not (click adds a key there holding the value). Dragging a slider or a
colour previews it live like the handles. Typing a value (Enter or leaving
the field; ↑ / ↓ step it, Shift ×10) or letting go of a slider writes the
change once: a constant changes; a keyframed property gets a key at the
playhead, or that key changes. Plain fields of the layer (text, colours of
shapes, masks...) are one `set` each. The logic of values and keys is the
egui window's (`engine/editing.rs`).

**Timeline.** A toolbar: **Add** (a picture, text, shape, particles, solid,
adjustment layer, cube, at the playhead), split and delete, *Ripple*, the
snap magnet, **Beat cut** and **Accent**, the in/out range when set, and
zoom out, zoom in, fit, keys. Then:

- the ruler in bars and beats (`1`, `1.2`, `1.3`...), with the project's
  markers; click or drag it to move the playhead;
- *Beats*: with the song analysis, its sections as bands, downbeats as tall
  ticks, beats as short ones, kicks as dots along the bottom and snares as
  small diamonds along the top (fainter when weaker), and the drop; without
  it, the project's own grid and an **Analyse** button;
- the waveform of the audio (peaks of the file decoded by ffmpeg, 100 a
  second, in project time);
- one track per layer, top of the picture first (`data-slot="timeline-row"`):
  a chevron (when the layer has keys), its kind's icon, id and what it is,
  the eye to turn it off; and its clip: a strip of its picture, its
  transition as a shaded region at the start with `⇢ fade`, its text, its
  effects as chips, its keys as small diamonds along the bottom.

Click a clip to select it (Shift or Ctrl adds or removes it from the
selection); drag on empty track space for a marquee (a plain click there
moves the playhead and deselects). Drag a clip to move it in time with all
its keys (all selected clips move together), its first or last 8 pixels to
trim `in` or `out`, a key diamond on a clip to move every key of the layer
at that time. With the magnet on (`N`), times snap to the song's beats, the
playhead, markers and other clips' edges within 8 pixels, else to quarter
beats; off, to frames. A dashed ghost shows where it lands; nothing is
written until you let go, and then it is one edit. The chevron opens one
lane per animated property; drag a key there to move that key alone, click
it to pick it (Delete removes it). **S** splits the selected clip at the
playhead into two that meet there (a video plays on through a time remap);
Delete removes the selected clips, and with *Ripple* the later clips move
back to close the gap. Drop a browser card on a clip to apply it to that
clip; drop a slot to add a picture there. Ctrl + wheel zooms about the
pointer.

**Beat cut** and **Accent** are the dialogs of `edits beats cut` and `edits
beats accent` (the same `edits_core::beatops` and `preset` functions). Beat
cut: the pictures to cycle through (ticked, in order), *Every N beats* (½,
1, 2, a bar) or *On the song* (beats, offbeats, downbeats, kicks, snares,
hats, with a minimum strength), *From* and *To* (any time: `b8`, `bar 3`,
`drop`; empty: start and end; the in/out range fills them), the layer names.
Accent: the preset, *On* (the same, and the drop), keep every one, every
other or every fourth, on which layer (the picture on screen, the selected
layer, or the whole frame through an `accents` adjustment layer), the range
and the minimum strength. Both say what they will do before you press the
button (`Adds 8 layers.`, `punch at 6 times.`, or why not, in red), and do
it as one edit. **Analyse song** runs the library behind `edits audio` on a
thread and keeps the result in `<project folder>/.edits/song.audio.json`
(the egui window's file), where the agent can pass it as `audio`.

### Overview

The project at a glance, as shadcn's dashboard: five cards (**Duration** in
seconds with frames, beats and bars; **Tempo**; **Layers** with how many are
on, effects, keys and slots; **Cuts**, the picture layers that start after
the beginning, with transitions and the mean spacing; **Match score**, the
overall score of the newest comparison with the reference and its trend
against the one before), an interactive area chart **Edit activity** (keys
and cuts per bar, beat or half beat, chosen with the toggle group), and a
data table with tabs *Layers · Effects · Markers · Slots*. In the Layers tab
the rows are the layers top of the picture first; drag a row's handle to move
the layer in the stack, type its in and out in place, click its id for a
drawer with its details (in, out, blend, Save), or use the row's menu (edit
in the Edit page, make a copy, disable, delete).

A screenshot after two comparisons shows the cards `3.9 s` (`98 frames`),
`123 BPM`, `5` layers (`5 on`), `2` cuts (`0 transitions`) and `75.6%` with a
`-24.4%` trending-down badge and `Further from the reference`; the chart's two
stacked grey areas over `b1` ... `b7`; and the table with drag handles,
checkboxes, `finish` (`adjustment`, a green `On` badge, `exposure, glow,
vignette`, `0` `8`, `7` keys, `normal`) and `tex` (`image tex`, `screen`).

### Reference

For copying the recipe of a reference edit ([ANALYSIS.md](ANALYSIS.md)). At
the top: the analysis (every `reference_analysis.json` up to three folders
below the project, or *Analysis…* for any file), the comparison report (the
studio's reports, or *Report…* for a file written by `edits-analyze compare
--json`), and **Compare a render…**, a list of the project's renders: it
analyses that video with the reference's beat grid and frame range, as
`edits-analyze compare` does (the same library call, on a thread), and saves
the comparison under `<project folder>/.edits/compare/<time>.json` with the
render's signature next to it (`<time>.signature.json`). A badge says
`comparing demo.mp4` meanwhile.

Then six cards: **Overall**, **Timing** (tempo and cuts), **Exposure**
(exposure curves and flashes), **Motion** (zoom curves), **Colour**
(saturation and tone hits) and **Blur**, each the mean of its scores, with
the change since the comparison before; an area chart **Signature over time**
of the reference's exposure, blur, saturation, scale or luminance per frame
against its beats, with the render's curve as a dashed line when the report
has it; and **Largest differences**, the report's sentences ranked by
severity, each with a **Jump to** button that moves the playhead to the start
of that shot (or the beat or frame the sentence names) and opens the Edit
page.

What the demo shows after comparing its own render (`ref.mp4`) and then a
render with layer `d` moved and turned: `75.6%` overall (`-24.4`), timing
`90.0%`, exposure `61.7%`, motion `99.8%`, colour `78.9%`, blur `43.5%`; the
exposure chart with a filled reference curve and a dashed render curve whose
flash at beat 4 comes late; differences `26.0×` *exposure* shot 3 `shot 3
(beats 3.54-8.05): exposure peak is 39 frames early`, and so on.

### Presets & Effects

Tabs **Presets**, **Transitions** and **Effects**, with counts, and a search
field over names, tags and descriptions (every word must match). Presets and
transitions are a table (name, group, description, number of variables, where
it comes from: `builtin`, `transition` or `project` for
`<project folder>/presets/*.json`). Click one for a sheet with its
description and tags, **On** (*Layer*: the selected layer, or the picture on
screen with none; *Screen*: the picture on screen, `--layer auto`;
*Accents*: an `accents` adjustment layer on top), **At** (the playhead, or the
layer's cut), and its variables (sliders with their range and default,
switches, selects, number fields for vectors and colours); **Apply at b3.61**
writes it through `edits_core::preset::apply`, one edit of the history. The
Effects tab lists the registry: each effect with its description, category,
parameters (default, unit, range) and options, and *Add to <layer>*.

### Activity

The history shared with the agent (`.edits/history/`). A card with a stacked
bar chart of edits per minute by the agent and by you, the counts, and Undo
and Redo; then a table: the edits that can be redone (struck through, with
Redo), then every edit newest first with its number, who made it (a robot
badge `agent`, a person `you`), what it did as a person reads it (hover for
the raw line), how long ago, Undo on the newest and **Restore** on the others.
Restore makes the version before that edit the project again, as a new edit
(so Undo takes the restore back).

### Renders

A card **Render video**: the output file (default
`renders/<project folder>.mp4`, numbered when taken; relative paths are inside
the project's folder), the size (1, 0.5 or 0.25 of the project's) and
**Render**, which runs `edits render <project> -o <file> [--scale S]` as a
child process. Its progress lines become a progress bar with `frame 32 / 98`,
the command's own line (`frame 32/98 (33%) 336 ms/frame 22 s left`) and the
time left; **Cancel** stops it; a toast says when it has finished or failed.
The `edits` command is looked for in `EDITS_BIN`, next to `edits-studio`, in
the workspace's other build folder, then on `PATH`. Below, **Outputs**: the
videos next to the project and in `renders/`, newest first, with their size
and age, and a menu (play in the system player, show in folder, compare with
the reference).

## Command palette and keys

Ctrl+K (or *Search commands…*) opens shadcn's command dialog: go to a page;
play, start, last frame, loop, go to a marker; undo and redo (naming the
edit), render; add a layer (solid, adjustment, an image of each slot, a cube
of each slot); for the selected layer enable or disable, duplicate, delete,
and *Add <effect> to <layer>* for every effect of the registry; select any
layer; apply any preset at the playhead.

| Key | What it does |
| --- | --- |
| Space | Play or pause |
| J / K / L | Back one beat / stop / play (Edit page) |
| Left / Right | One frame back / forward (pauses); with Shift, one beat |
| Home / End | First / last frame |
| Shift+L | Loop on or off (L alone, on other pages) |
| S | Split the selected clip (or the picture under the playhead) at the playhead (Edit page) |
| I / O, [ / ], X | In / out point at the playhead, go to them, clear them (Edit page) |
| N | Snapping to beats and edges on or off (Edit page) |
| ? | The list of keys (Edit page) |
| Ctrl+Z / Ctrl+Shift+Z, Ctrl+Y | Undo / redo the newest edit in the shared history, yours or the agent's |
| Ctrl+K | Command palette |
| Ctrl+1 ... Ctrl+6 | Edit, Overview, Reference, Presets & Effects, Activity, Renders |
| Delete, Backspace | Delete the selected clips, or the key picked on a lane (with *Ripple* on, later clips close the gap) |
| Esc | Deselect |
| D | Dark or light theme |
| Ctrl+D | The debug panel (dev builds) |

Keys are ignored while a text field, a select or a slider has the focus,
except Ctrl+K.

## Edits, the shared history, and the agent

Everything under [One history for you and the
agent](#one-history-for-you-and-the-agent) holds for the studio: edits are
written as actor `window` with the hash of the version they were prepared
against (an edit prepared against a version that is no longer on disk is
refused and the new version is shown), Ctrl+Z undoes the newest edit whoever
made it, and the agent's `project_undo` can undo yours. While the file on
disk has errors the studio does not edit it; Undo still works.

When the file changes, the studio reads it again (the watcher of
`engine/reload.rs`: file-system events plus a once-a-second check, a burst of
writes is one reload) and tells what happened by the shared history:

| Toast | When |
| --- | --- |
| *Agent edit* · `d: transform.rotation → [0, 0, 12]` with **Undo** | the agent's `project_edit` changed the file (8 s) |
| *The agent undid an edit* | the agent's `project_undo` |
| *Reloaded* · `The file changed on disk · revision 4` | a change without a history entry (a text editor, a picture) |
| *The file has N errors* · `Showing revision 3 until it is fixed` | the file on disk stopped being valid |
| *Not applied* and why | an edit of yours was refused |
| *Render finished* / *Render failed* / *Comparison finished* | background jobs |

**`ui.json`.** The studio writes `<project folder>/.edits/ui.json` (format
`edits.ui/1`, see [AGENTS.md](AGENTS.md#what-the-person-is-looking-at-editsuijson))
exactly as the egui window does, with the same timing (at once, at most four
times a second, when the selection, the paused playhead, the view, the loop
or the note change; every 2 s while playing; at least every 5 s while open)
and removes it when it closes. The agent's `ui_context` then answers, for
example after picking `position` on layer `d` at frame 60:

```json
{ "open": true, "playhead": { "frame": 60, "seconds": 2.4, "t": 4.92 }, "playing": false,
  "selection": { "layer": "d", "property": "/layers/@d/transform/position" },
  "loop": { "from": 0, "to": 8 }, "project": "project.json", "age_seconds": 4.44 }
```

## How it is built

- `studio/` was generated by `create-tauri-ui` (Vite template, dashboard-01
  starter, debug panel; see THIRD_PARTY.md) and its components by the shadcn
  CLI (`components.json`: style `base-nova` on Base UI, neutral base colour,
  Lucide). `studio/src/components/ui/` is what the CLI wrote. The pages are in
  `studio/src/components/pages/`, the Edit page's parts in
  `studio/src/components/edit/`, the shared state (snapshot, selection,
  playhead, playback, audio, `ui.json` reports) in `studio/src/lib/store.tsx`
  and the typed commands in `studio/src/lib/api.ts`. Charts are shadcn's
  `chart` (Recharts).
- The Rust side (`studio/src-tauri/src/`) owns the project. `project.rs` holds
  the last valid version and builds the snapshot the page draws (rows, layers,
  slots, markers, history, the JSON); `studio.rs` opens and watches the file,
  makes every edit (`edits_core::edit::apply` / `apply_with`, actor `window`,
  with the expected hash), undo, redo and restore, and tells the page what
  changed (`project` events); `render.rs` is the render thread (the newest
  current request replaces one that has not started, prefetch requests wait
  behind it, JPEG frames cached by revision); `inspect.rs` evaluates every
  property of a layer at a time with its unit, range and keys from the effect
  registry; `uictx.rs` writes `ui.json`; `renders.rs` runs `edits render`;
  `reference.rs` finds and reads analyses and reports and runs comparisons
  through `edits-analyze`; `commands.rs` is the list of Tauri commands.
  `engine/` is ported from `crates/edits-gui` (editing, time, rows, reload),
  so both windows make the same edits.
- The webview loads the project's audio through Tauri's asset protocol.
- Tests (`cd studio/src-tauri && cargo test`, no window needed): the snapshot,
  telling an agent's edit from an undo and from a plain reload, edits written
  as the window and undone through the shared history, a stale edit refused,
  restoring a version, the inspector's values, units, keys and the camera,
  moving a layer to any place, presets on the selected layer at the playhead
  or its cut, the effect registry, the frame cache and the render thread
  (skipped without a graphics adapter), render progress, outputs and a failing
  command, finding analyses and reports, `ui.json` content, timing and
  removal, and the command line.

## Known limits of the studio

- Not editable in the studio: project settings, markers, audio, layer ids and
  names, text animator values, shape operators and paths, particle bursts
  and emitters, mask shapes and positions (mode, feather and inversion
  are), shake and loop modifiers (their values under the shake can be
  edited), custom shaders' `source`; values keyed inside those show as
  *animated* and are edited by the agent or a text editor, which the studio
  follows.
- No graph editor: eases are chosen per key in *Keys*. Transform handles
  work in the layer's plane (they ignore the camera's perspective and the
  layer's X and Y rotation), and the box of a text, shape or particle layer
  is an estimate.
- The agent is not told that there is a note: say "see my note" in your
  chat.
- Audio plays through the webview: it needs GStreamer, and it follows the
  wall clock, not the sound card's.
- The camera's keys move one by one on its lanes, not together on its row.
- Tested on X11 under Xvfb at 1400 × 900 with Mesa lavapipe, driven with
  `xdotool`: opening the demo, typing a value in the inspector (written as
  actor `window`, `ui.json` updated), an agent's `project_edit` while open
  (reloaded, toast with Undo), dragging a key on the timeline, Ctrl+Z, the
  command palette, rendering from the Renders page, two comparisons on the
  Reference page and *Jump to*, a preset from the Presets sheet, playback,
  a note for the agent (read back by `ui_context`), the release build started
  through `edits gui`, and the debug panel of `bun run tauri dev` (Ctrl+D). Not run on
  Wayland, macOS or Windows, or on a HiDPI screen; audio could not be heard
  where it was developed.

# The egui window (`edits-gui`)

The older desktop window, written with egui. `edits gui` starts it when Edits
Studio is not built, with `EDITS_GUI=egui`, or with an option only it has
(`--screenshot`, `--delay`, `--size`, `--scale`, `--play`, `--cache-mb`,
`--adapter`); its screenshot mode is what automated checks of the window use.

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

## Layout

```
┌ top bar ───────────────────────────────────────────────────────────────────────┐
│ ▣ demo / project.json  rev 3  agent   [ Search or run a command…  Ctrl K ]  ↶ ↷  Export │
├────┬───────────────────────────────────────────────────────┬────────────────────┤
│rail│ preview on its stage (the picture at its true aspect) │ inspector          │
│    │                                                       │  (the selection)   │
│    │                                        toasts here ▸  │  ▾ Layer           │
│    │                                                       │  ▾ Transition in   │
│    │ 00:01.48 / 00:03.88  B 3.03  F 37    ⏮ ‹ ▶ › ⟳   0.5× │  ▾ Transform       │
├────┴───────────────────────────────────────────────────────┴────────────────────┤
│ + Layer  ¼ beat  Analyse song  5 layers · 8 beats · 123 BPM · 25 fps   − + fit  │
│ overview of the whole edit                                                      │
│ ruler · markers · song (once analysed) · audio waveform · one track per layer   │
├─────────────────────────────────────────────────────────────────────────────────┤
│ ● Revision 3   last action     Agent sees d · B 3.03 · adapter · ms/frame · ... │
└─────────────────────────────────────────────────────────────────────────────────┘
```

What a screenshot of the demo at 1600 × 1000 shows, with layer `d`
selected: a near-black window (greys from `#080808` behind the picture to
`#191919` for raised surfaces) whose brightest thing is the preview. The top
bar reads `demo / project.json` next to a small `rev 1` pill; a search field
sits in the middle with `Ctrl` `K` key caps, and undo and redo icons and a
violet `Export` button are on the right. The inspector opens with a blue
image badge, `d` and `image d`, and the line `Values at frame 37 · 3.034
beats` in a mono font; below it the *Layer* and *Transform* sections, each
row a grey label and mono value fields (`0 0 0`, `1.35`), the row of the
picked property tinted violet. Under the picture the transport reads
`00:01.48 / 00:03.88  B 3.03  F 37`, with the play button and the lit loop
toggle centred. The timeline draws each track as a dark fill of its kind's
colour (blue for pictures, plum for the cube, teal for the adjustment layer,
violet for the camera) with a thin bright stripe on top, effect names in
small mono type, white key glyphs, and a thin white playhead with a violet
cap on the ruler.

The window is dark, with one accent colour (violet) for selection, focus and
the primary button; green, amber and red appear only for status. Text is
Inter; every number (values, timecode, frame numbers) is JetBrains Mono, so
digits do not dance while they change; icons are Lucide. The fonts are
embedded in the program (`crates/edits-gui/assets/fonts/README.md` says
where they come from and under which licence). Colours, sizes and spacing
are design tokens in `crates/edits-gui/src/theme.rs`.

## Keys

| Key | What it does |
| --- | --- |
| Space | Play or pause |
| Left / Right | One frame back / forward (pauses) |
| Shift+Left / Shift+Right | One beat back / forward: to the first frame of the previous / next whole beat (one second in a seconds project) |
| Home / End | First / last frame |
| L | Loop on or off |
| Ctrl+Z | Undo the newest edit in the project's history, yours or the agent's |
| Ctrl+Enter | In the note to the agent: save it |
| Ctrl+Shift+Z, Ctrl+Y | Redo |
| Ctrl+K, Ctrl+Shift+P | Command palette (also from inside a text field) |
| Ctrl+D | Duplicate the selected layer |
| Delete, Backspace | Delete the selected layer (Ctrl+Z brings it back) |
| Ctrl+E | Export video |
| Ctrl+1 / Ctrl+2 / Ctrl+3 | Show or hide the Slots / History / Presets drawer |
| Esc | Close the shortcut sheet; else deselect; else close the drawer |
| ? | The sheet of keyboard shortcuts |
| Ctrl + wheel over the timeline | Zoom about the pointer |
| Shift + wheel over the timeline | Scroll sideways (so does a sideways swipe) |
| Alt while dragging on the timeline | Snap to frames instead of quarter beats |

Keys are ignored while a text field or a value being typed has the focus
(there, Ctrl+Z undoes typing, and Enter finishes the value), except Ctrl+K.
Every button's tooltip names its action and its keys.

## Command palette

Ctrl+K (or the search field in the top bar, or the ⌘ icon at the bottom of
the rail) opens a list of everything the window can do, built from what it
shows at that moment:

- **Playback**: play or pause, start, last frame, a frame or a beat back or
  forward, loop.
- **Edit**: undo and redo, naming the edit they would take back or bring
  back and whose it was.
- **Add layer**: a solid, an adjustment layer, an image layer from each
  slot, a cube with each slot.
- **Layer** (with a layer selected): enable or disable, duplicate, move up
  or down, delete; **Add effect**: every effect of the registry.
- **Select**: every layer by id, the camera, deselect. **Go to**: every marker.
- **Preset**: *Apply preset flash…* for every preset: opens it in the
  Presets drawer. **Transition** (with a layer selected): *Add transition
  whip_pan into d* for each of the 52 kinds, and *Remove the transition*.
- **Song**: *Analyse song* (or *again*), show or hide the song's downbeats,
  kicks, snares and sections, *Write the song's tempo and markers into the
  project*, *Cut on beats…*, *Accent on beats or onsets…*.
- **Agent**: *Tell the agent…* (the note in `ui.json`), *Clear the note*.
- **View**: the drawers, the preview size, motion blur while paused or
  playing, fit and zoom the timeline, the shortcut sheet. **File**: export, quit.

Type to narrow the list: the letters must appear in order, and a match at
the start of the name or of a word ranks first (`add gl` finds *Add glow to
tex* and *Add gaussian_blur to tex*; `tex` finds *Select tex*). Up and Down
move, Enter runs, Esc or a click outside closes. A screenshot of it shows a
raised panel with rounded corners over the dimmed window: the search line,
then rows with an icon, the command, its group in small grey type
(`Add glow to tex  Add effect`), key caps on the right of commands that have
keys, and a footer `↑ ↓ move · Enter run · Esc close · 8 commands`.

## Toasts

Short notes appear in the bottom-right corner of the preview for things that
happen while you look elsewhere. At most three are shown; the same note
again counts up (`×2`) instead of stacking; hovering one keeps it.

| Toast | When | Button |
| --- | --- | --- |
| *Agent edit* (robot icon) and what changed, e.g. `d: transform.rotation → [0, 0, 12]` | the agent's `project_edit` changed the file | **Undo**: undoes that edit if it is still the newest in the history; otherwise it says newer edits came after it |
| *Agent undid its edit* | the agent's `project_undo` | **Redo** |
| *Edit by …* | another program wrote through the shared history | – |
| *Reloaded* · `The file changed on disk · revision 4` | the file changed without a history entry (a text editor, a script) | – |
| *The file has N errors* · `Showing revision 3 until it is fixed` (red) | the file on disk stopped being valid | – |
| *Undid the agent's edit* / *Undid your edit* | Ctrl+Z, or an Undo button | **Redo** |
| *Not applied* and why (red) | an edit of yours was refused (the check found an error, or the agent wrote meanwhile) | – |
| *Export finished* (green) / *Export failed* (red) / *Export cancelled* | an export ended, also when its dialog is closed | **Show** (failed) opens the dialog |

Your own edits do not make toasts: what they did is in the status bar.
Toasts last 4 seconds, 8 with a button or for an error.

## What each part shows

**Top bar.** The project's folder and file name; a `rev N` pill (a counter
of loaded versions), or a red `N errors` pill while the file on disk has
errors; an `agent` pill for a minute after the agent's last edit (hover it
for what it did). In the middle, the field that opens the command palette.
On the right, undo and redo (their tooltips name the edit and who made it)
and **Export**.

**Rail** (left edge). Icons for the drawers, all closed at start: **Slots**
(Ctrl+1), **History** (Ctrl+2) and **Presets** (Ctrl+3, a wand); at the
bottom the command palette and the shortcut sheet. A drawer opens beside the rail and can be resized; its title
row has its own close button (and undo and redo, for History).

**Preview** (centre). The frame under the playhead, rendered by
`edits-render` exactly as `edits frame` would render it at that scale, on a
near-black stage. When the picture on screen is not the frame the transport
reads (a slow renderer during playback, or a frame still rendering after a
scrub), a note in its corner says which frame it is. Problems appear as a
slim red banner along the top of the stage while the last good frame stays,
and a thin green outline fades around the picture when a change has been
loaded.

**Transport** (the row under the preview). On the left the position: the
time as `00:01.48 / 00:03.88` (minutes, seconds, hundredths, then the whole
length), `B` the beat (in a beats project) and `F` the frame. In the middle:
start, a frame back, play/pause, a frame forward, loop (lit when on). On the
right the preview quality, `0.5×`: its popover chooses the size (0.25, 0.5
or 1 of the project's resolution) and whether the preview uses the project's
motion blur, separately while *paused* (on by default) and while *playing*
(off by default, because motion blur multiplies the cost of a frame).

**Timeline** (bottom).

- A toolbar: **+ Layer** (add a layer at the playhead), what drags snap to
  (`¼ beat`, or `frames` while Alt is held; hover it for how to drag and
  what the key shapes mean), **Analyse song** (then **Song**, see [Song
  tools](#song-tools)), the size of the edit, and zoom out, zoom in and fit.
- The **overview**: the whole edit in one strip, each layer a thin line in
  its kind's colour, the markers, the playhead and, when zoomed in, the part
  the tracks show as a violet box. Click or drag it to move the playhead.
- A ruler in the project's time base. In a beats project it counts beats
  from 0, with a stronger line every four beats (the format has no time
  signature; the grid assumes 4/4) and lines on half and quarter beats when
  zoomed in. In a seconds project it counts seconds.
- The project's `markers`, as amber flags with their labels.
- After *Analyse song*, the **Song** lane: the song's sections as
  alternating grey bands with their labels (`intro`, `high`, `drop`...),
  downbeats (bar starts) as light lines, kick onsets as orange dots under the
  middle and snare onsets as pink diamonds over it (bigger and brighter when
  stronger), the drop as an amber flag. Hover a mark for its time and
  strength; click the lane to move the playhead. The Song popover switches
  each kind on or off; with all off the lane goes away.
- The waveform of `audio`, placed by `audio.offset`; the part before the
  playhead is lighter.
- One track per layer, the top of the picture first (the last layer in the
  file is the first track). The header column has the kind's icon, the id
  and the kind, and on hover an eye that disables or enables the layer
  (always shown while it is disabled). The bar runs from `in` to `out`: a
  muted fill of the kind's colour (image blue, cube plum, solid grey,
  adjustment teal, camera violet) with a thin bright stripe on top, the
  names of its effects (and its blend mode when it is not `normal`) in small
  mono type, and its keyframes as white glyphs whose shape says the ease of
  the segment that starts there: a diamond for `linear`, a square for
  `hold`, a circle for any other ease (keys of several properties at one
  time with different eases show a diamond). Hover a keyframe to read its
  time and the properties keyed there. A layer that comes in through a
  transition has a thin violet wedge rising from its start over the
  transition's length (hover it for the kind). A `camera` track appears
  above the layers when the camera is animated. Disabled layers are dimmed. The
  selected layer's header has a violet edge and its bar a violet outline.
- The playhead: a thin white line with a violet cap on the ruler.

Click or drag on the ruler (or the marker and waveform lanes) to move the
playhead. Drag the top edge of the panel to resize it.

**Inspector** (right). Click a layer's track to see and edit what it
evaluates to at the playhead. Its title shows the kind, the id, and buttons
to enable or disable, duplicate, a `…` menu (move up, move down, delete) and
deselect. Then collapsible sections:

- *Layer*: slot (image), colour with a swatch (solid) or faces and size
  (cube), `in` and `out` with their frames, blend, and whether it is
  visible now.
- *Transition in*: how the layer comes in. See [Transitions](#transitions).
- *Transform*: position, scale, rotation, anchor and opacity.
- *Effects*: for each effect a switch (on or off), its name (hover it for
  its documentation), move up, move down and remove; then every parameter of
  its spec, with its unit. The `+` in the section's title adds one.

A property's name is dim grey when the file does not set it (the default
applies) and lighter when it does; a small icon after it marks a `shake` or
a `loop`; hover it for its documentation, its default and how it is
animated. The numbers change as the playhead moves. With no layer selected
the inspector says how to select one and shows the project's settings and
(folded) the camera, whose fov, position and rotation are editable too. See
[Editing](#editing).

**Problems** (top of the inspector, only when there are some). The findings
of `edits check` for the file as it is on disk: errors and warnings, each
with the path into the document, the message and the hint. Warnings alone
stay folded.

**Slots** (drawer). Every slot with its picture's path, how many layers use
it (hover for which), its fit and focus; and a field to add a slot.

**History** (drawer). The newest edits first, each with a badge for who made
it (a person for `you`, a robot for `agent`), what it did written plainly
(`d: transform.rotation → [0, 0, 12]`; hover for the raw change line) and
how long ago; above a line, the edits that were undone and can be redone.

**Status bar** (bottom edge). On the left the revision, `Reloaded` (green)
just after someone else's change, or the errors in the file (red); then what
your last edit did. On the right what the agent can see (`Agent sees d ·
B 3.03` after a sky-blue eye, with a speech bubble when you left a note;
amber when `ui.json` cannot be written; click it to write a note, see [What
the agent sees](#what-the-agent-sees-uijson)), the renderer's adapter and
whether it is a `GPU` or `software`, the time the last frame took to
render, the frame cache, the state of the audio, the progress of an export.

**Presets** (drawer). See [Presets](#presets).

## Editing

Every edit is one step: it is checked (`edits check` plus the keys the
renderer would ignore), and only a project without errors is written. The
file is replaced atomically (as stable, readable JSON, the same text the
agent's edits produce) and the version before it is saved under
`<project folder>/.edits/history/`. When an edit would leave an error, the
file is not touched, and a red toast (and the status bar) says why, for
example `Not applied (slot b = img/nope.png)` · `error /slots/b: image not found`.

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

The diamond at the end of a property's row is **Key here**: it turns a
constant into keyframes with one key at the playhead, or adds a key holding
the value the property has there. It is filled when a key is at the
playhead, outlined when the property is animated, and appears only when the
pointer is over the row otherwise. Click a property's name to *pick* it (its
row turns violet): the **Keys** card under its section then lists its key
times (the one at the playhead lit), removes the key at the playhead
(*Remove key*; with no keys left the property is a constant again), and sets
the ease of the segment the playhead is in: any name of
`edits_core::ease::NAMES`, `decay` with its rate, or `bezier` with its four
numbers. An animated property that is picked shows the card too; `scale`
is picked at start (it is what double-clicking a track keys) but shows its
card only once it is animated or picked by hand.

**Effects.** The `+` in the title of the *Effects* section (or *Add effect* in
the command palette) lists the registry, each with its one-line
documentation as a tooltip; the effect is added after the others with its
defaults. Each effect has a switch (enabled), up and down arrows (the order
they apply in) and a bin (remove).

**Layers.** *+ Layer* on the timeline (or *Add layer* in the command
palette) adds a solid, an adjustment layer, an image layer from a slot or a
cube with a slot on its faces, one bar long, at the whole beat under the
playhead (earlier when it would run past the end), directly above the
selected layer (or on top). The buttons in the inspector's title, the eye on
a track, the keys (Ctrl+D, Delete) and the palette enable or disable,
duplicate (a copy directly above, id `<id>-copy`), move one place up or down
in the stack, and delete the selected layer.

**Slots.** In the Slots drawer, type a path (relative to the project file)
and press Enter, or **drop a picture file on a slot**: the path is written
relative to the project when the picture is inside its folder. Dragging files
over the window opens the drawer and outlines the slot under the pointer.
Dropping works on X11; on Wayland it depends on the compositor and winit and
may not reach the window (the tooltip says so): type the path there. Fit is a
menu, focus two numbers. *New slot* · *Add* creates an empty one.

**Timeline.** Times snap to quarter beats in a beats project; hold Alt to snap
to frames instead (a seconds project always snaps to frames). The toolbar
says which.

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

## Presets

The Presets drawer (Ctrl+3, the wand on the rail, or *Apply preset …* in
the command palette) lists every preset the project can use: the built-in
ones (`presets/*.json` of the repository), one `transition:<kind>` per
transition, and the project's own `<project folder>/presets/*.json`. They
are grouped by their first tag (*Hits*, *Zoom*, *Move*, *3D*, *Blur*,
*Speed*, *Look*, ..., then *Transitions*, folded); the field at the top
searches names, tags and descriptions (`whip` finds `whip_pan`, `glitch`
the glitch hits), and the circular arrows read the files again.

Click a preset to open its card:

- its description, then **Variables**: one row each, with the preset's
  range and default. Numbers with a range are sliders (with the number,
  which can be typed), whole numbers step by one; switches for booleans,
  menus for choices (`direction`, `axis`) and for `ease` and `slot`, a
  colour button for colours, number fields for vectors. A changed
  variable's name turns violet, and the arrow at the end of its row puts
  the default back. Hover a name for its documentation, unit and range.
- **On**: *Layer* (the selected layer; with none selected, the picture on
  screen), *Screen* (the picture on screen at that time, `--layer auto`) or
  *Accents* (an `accents` adjustment layer on top, `--layer fx`). Presets
  that add their own layers (`letterbox`, `overlay_texture`) have no target.
- **At**: the *Playhead*, or the layer's *Cut* (its `in`): transitions and
  intros start at the cut.
- **Preview** (on by default): the preview shows the frame under the
  playhead with the preset applied, before anything is written; a violet
  chip in its corner says `Preview: punch at 2.46 on cube (on screen) · not
  applied`. Scrub to see other frames of it. Values the preset refuses are
  said in red under the switch.
- **Apply**: writes it. The line next to it (`at 2.46 beats on auto`) says
  where it goes; hover it for the same `edits apply` command.

Applying runs the code of `edits apply` and of the agent's `preset_apply`:
`edits_core::preset::apply` on the file's version, written by
`edits_core::edit::apply_with`, so it is checked first, written in one step
and is one entry of the shared history (`preset punch at 2.46 on auto`,
actor `window`): Ctrl+Z takes the whole preset back. The card then closes
and the layer it changed is selected. A preset writes ordinary keys,
effects and layers, which are edited afterwards like any other.

## Transitions

A layer can come in through a transition instead of a cut
([FORMAT.md](FORMAT.md#transitions)). The inspector's **Transition in**
section shows it for the selected layer:

- Without one: `None: the layer cuts in at its in`, and a **+** in the
  title that lists the 52 kinds with their default length (hover one for
  what it looks like). Adding one runs the `transition:<kind>` preset at the
  layer's `in` with `align: after`: the layer's `in` stays where it is, the
  transition starts there with its default length, and the picture it comes
  from is kept until it ends (its `out` moves later when needed). One edit.
- With one: **Kind** (a menu of the 52; another kind keeps the parameters
  it also has and drops the others, which `check` would refuse), its
  one-line description, **Duration** in the project's time base (drag or
  type; `ends 5` says where it ends, and the picture it comes from is kept
  until then), **Ease** (every ease name), and every parameter of the kind
  with its range: number fields, or a colour button. A parameter keyframed
  in the file is shown, not edited. The bin in the title removes the
  transition.

Every change is an `edits_core::edit` op on `/layers/@<id>/transition`, one
edit each (a drag is one edit however long it lasts). The command palette
has *Add transition <kind> into <layer>* and *Remove the transition into
<layer>*, and the Presets drawer the same transitions as presets (centred
on the cut by default, as `edits apply`).

## Song tools

**Analyse song** (timeline toolbar, or the command palette) analyses the
project's `audio` file with the library behind `edits audio`
(`edits_analyze::song`: tempo and beat grid, bars, kick, snare and hat
onsets, loudness, sections and the drop), on a thread of its own, so the
window keeps playing; the toolbar says `Analysing the song…` meanwhile.
It is disabled when the project has no audio. The result:

- is kept in `<project folder>/.edits/song.audio.json` (format
  `edits.audio/1`, what `edits audio -o` writes), so the agent can pass it
  as `audio` to `beats_cut`, `beats_accent` and `preset_apply`; the window
  reads it again when it opens, if it is newer than the audio file and was
  made of that file;
- appears as the Song lane of the timeline (see [What each part
  shows](#what-each-part-shows)), in the project's time (it follows the
  project's tempo and `audio.offset`), and a green toast says what was
  found: `123 BPM · 2 bars · 8 kicks · 16 snares · no drop`;
- feeds the times: `drop`, the sections' names and the song's bars work in
  the time fields below and in the presets' times.

The toolbar button is then **Song**: a popover with that summary, the
analysis' notes (`the bar phase is a guess`), a switch for each kind of
mark (downbeats, kicks, snares, sections and drop), and:

- **Cut on beats…**: the dialog of `edits beats cut`. Tick the slots to
  cycle through (in order); cut *Every N beats* (0.5, 1, 2 and 4 are at
  hand) or *On…* beats, offbeats, downbeats, kicks, snares or hats (onsets
  need the analysis; *Min strength* skips weak ones); *From* and *to* (any
  time: `b8`, `bar 3`, `drop`, `drop+2b`; empty = start and end); and the
  prefix of the layer ids. Before you press **Cut** it says what it will do
  (`Adds 4 layers.`) or why it cannot (in red). It calls
  `edits_core::beatops::cut`, as the command does, as one edit.
- **Accent on…**: the dialog of `edits beats accent`. A preset (any but the
  transitions); *On* beats, offbeats, downbeats, kicks, snares, hats or the
  drop; *keep* every Nth (`every other one`); *From* and *to*; the layer it
  goes on (as in the Presets drawer); and the preset's variables. It lists
  the times it found (`2 times: 2, 6`) before you press **Accent**.
  `beatops::times` and `preset::apply`, one edit.
- **Write tempo and markers**: what `edits audio --project` does: the
  song's tempo, its beat offset and its markers (the drop, the sections)
  go into the project (`edits_core::beatops::sync`), one edit.
- **Analyse again**.

Without an analysis, *Cut on beats…* and *Accent on…* (command palette)
work on the project's own beat grid: every N beats, beats, offbeats and
downbeats.

## What the agent sees: `ui.json`

While the window is open it writes `<project folder>/.edits/ui.json`
(format `edits.ui/1`, written with `edits_core::ui::write`; the format is in
[AGENTS.md](AGENTS.md#what-the-person-is-looking-at-editsuijson)), and the
agent reads it with the MCP tool `ui_context`, so "make *this* hit
stronger" means the selected layer at the playhead. What the window wrote
during the check described under [Known limits](#known-limits):

```json
{
  "format": "edits.ui/1",
  "project": "project.json",
  "updated": "20261113T140902.328Z",
  "pid": 29140,
  "playhead": { "t": 5.166, "seconds": 2.52, "frame": 63 },
  "playing": false,
  "selection": { "layer": "d", "property": "/layers/@d/transform/position" },
  "visible": { "from": -0.045401, "to": 8.081401 },
  "loop": { "from": 0.0, "to": 8.0 },
  "note": "make this hit stronger",
  "note_at": "20261113T140902.328Z"
}
```

- `selection`: the selected layer (none for the camera), and the property
  you picked by hand in the inspector, as a pointer `project_edit` takes,
  with `effect` (its effect's index) and `key` (the index of its key at the
  playhead) when there are. `visible`: the part of the timeline in view.
  `loop`: the whole project, while looping is on.
- When: at once when the selection, the paused playhead, the view, the loop
  or the note change (at most four times a second), every 2 s while
  playing, and at least every 5 s while the window is open (the agent takes
  a file older than 30 s for a window that is gone). It is written
  atomically, and removed when the window closes (unless another window on
  the same project wrote it since).
- **Tell the agent…** (command palette, or a click on `Agent sees …` in the
  status bar) opens a box for a note; **Save note** or Ctrl+Enter puts it in
  `note`, with the time in `note_at`, and the status bar shows a speech
  bubble. The agent is not told that there is a note: say "see my note in
  the window" in your chat.

`ui_context` then answers:

```
the window is open on project.json (state from 1.1 s ago):
playhead at 5.166 (2.52 s), frame 63
selected: layer d, /layers/@d/transform/position
visible on the timeline: -0.045..8.081
looping 0..8
note from the person: "make this hit stronger"
```

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
playback carries on silently and the status bar says `Audio silent` (in
amber) with the reason in its tooltip.

## Export

**Export** (top bar, Ctrl+E) opens a dialog that asks for an output file
(default: next to the project, named after its folder); **Render** runs
`edits render <project> -o <file>` as a separate process. Progress and the
result appear in the dialog and the status bar, and a toast says when it
has finished (or failed), also when the dialog is closed; `Cancel` stops it. The `edits` command is looked for in `EDITS_BIN`, then next
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
If the project is not valid, the picture shows the Problems list.

`--delay SECONDS` keeps the window running that long before the picture is
taken, and then waits for the preview again. That is how to check live
reload: start it with a delay, change the project file meanwhile, and look
for the new picture, the revision number, the toast and the Problems list in
the screenshot. Run it with `WINIT_X11_SCALE_FACTOR=2` (and a screen twice
the `--size`) for a picture of a 2× screen. `--select <layer id>` opens a layer in the inspector, `--play`
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
- The song analysis runs on a thread of its own (`edits_analyze::song`, the
  library of `edits audio`, which decodes through `ffmpeg`); presets,
  transitions and beat operations call `edits_core::preset`, `beatops` and
  `edit::apply_with` directly, never the `edits` program.
- Edits are made by `edits_core::edit` (shared with `edits-mcp`): ops
  addressed by JSON pointer with layers by id (`/layers/@tex/transform/scale`),
  validation, the atomic write and the history. The window turns gestures into
  those ops in `editing` (pure functions of the project it shows) and writes
  them as actor `window`, with the hash of the file text it prepared them
  against, so a stale edit is refused rather than applied over someone
  else's.
- The look is in one place: `theme` holds the design tokens (colours,
  spacing, radii, type sizes), the fonts (Inter and JetBrains Mono, variable
  fonts with their weight set through `FontTweak::coords`; Lucide from the
  `lucide-icons` crate as an icon font) and `apply`, which turns them into
  egui's `Style` and `Visuals`. `widgets` draws the window's own controls
  with them (icon buttons, key caps, sections, menu rows, the switch); other
  modules use the tokens and never write a colour of their own.
- Nothing repaints while nothing happens: the window draws a new frame on
  input, while playing, while a toast or an animation is on screen, and once
  a second while the History list has times like `12 s ago` to update. An
  idle window measured 0 CPU ticks over 10 s.
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
  effect ops, paths of dropped pictures, history lines and how they read; the
  command palette's fuzzy matching (`palette`), the toasts and the reading of
  the shared history behind them (`toasts`), the fonts and style
  (`theme`), `ui.json`'s content and when it is written, and its removal
  (`uictx`), the presets' grouping, search, edited variables and the change
  they make (`presets`), the transition section's ops and the added
  transition (`transitions`), and the song's marks in project time, their
  switches and the cut and accent forms (`song`). The history itself is
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

- Editing covers layers, effects, keys, eases, slots, the camera,
  transitions, presets and beat operations. Not in the window: project
  settings (size, fps, duration; BPM and markers only from a song analysis),
  audio, markers by hand,
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
- The window layout and settings are not remembered between runs (which
  sections are folded is remembered while the window is open).
- The command palette runs the window's own commands. The note to the agent
  goes into `ui.json` only: the agent reads it when it calls `ui_context`,
  it is not notified.
- The preset preview renders the frame under the playhead (scrub or play to
  see the rest of it).
- *Analyse song* estimates the tempo (as `edits audio` without `--bpm`);
  there is no field for a known tempo or for beats per bar (4). The song
  marks' switches are not remembered between runs.
- A transition parameter that is keyframed in the file is shown, not edited.
  Changing a transition's length moves the outgoing picture's `out` later,
  never earlier.
- Saving a preset from the window (`edits save-preset`) is not there.
- The text of the interface is English and Inter covers Latin, Greek and
  Cyrillic; other scripts fall back to egui's bundled fonts.
- Tested on X11 (under Xvfb, with llvmpipe), driven with `xdotool`: typing
  and dragging values, key here, the ease menu, adding an effect from the
  menu, adding, duplicating and moving layers, dragging a bar, an edge and a
  key marker, double-clicking to key, slot paths, undo and redo, and an
  agent's `project_edit` and `project_undo` while the window was open. The
  redesigned window was checked the same way at 1600 × 1000, 1280 × 800 and
  at a scale factor of 2 (`WINIT_X11_SCALE_FACTOR=2`): the command palette,
  folding sections, dragging a value and a bar, the agent's edit and its
  toast's Undo, the error banner and toasts, the drawers, the popovers and
  the shortcut sheet. The presets, transitions, song tools and `ui.json` were
  checked the same way on the demo at 1600 × 1000: two presets applied
  from the drawer (one with a variable changed, one through the palette on
  the picture on screen), a transition added and lengthened in the
  inspector, the song analysed and its lane shown, a cut on kicks and an
  accent on downbeats, a note saved; after each edit `edits check` passed
  and the history had one `window` entry; `ui_context` of `edits-mcp`
  returned the selection, the picked property and the note, and said the
  window was not open once it had closed. Not seen on a real HiDPI or fractional-scale screen.
  Wayland support is compiled in and is the same code path in `winit`, but
  was not run.
