# Driving the engine from an agent

`edits` is built to be operated by a coding agent (Claude Code, Codex) as
well as by a person. There is no session to open and no editor state: a
project is one JSON file, and every operation is a command on that file.

This page is the map. The step-by-step workflows with exact commands are in
the skill, [`.claude/skills/edits/SKILL.md`](../.claude/skills/edits/SKILL.md).

## Three ways in

| Route | What it is | Use it when |
| --- | --- | --- |
| **Edit the JSON** | The project file, format `edits.project/1` ([FORMAT.md](FORMAT.md)). Change it with your own file tools. | Always available. Good for writing a project from scratch. |
| **CLI with `--json`** | `edits` ([CLI.md](CLI.md)) and `edits-analyze` ([ANALYSIS.md](ANALYSIS.md)). Data on stdout as JSON, progress and errors on stderr, non-zero exit on failure. | You have a shell. The base integration: everything else is built on it. |
| **MCP tools** | `edits-mcp`, a stdio server that wraps the two executables and adds validated, undoable edits and images returned inline. | The client has no shell, or you want edits that cannot leave a broken file, and frames you can see without a second step. |

The routes share one state, the project file, so they can be mixed freely:
write the file by hand, check it through MCP, render it from the shell.

## The MCP server

`edits-mcp` (crate `crates/edits-mcp`) is a thin adapter. It runs `edits` and
`edits-analyze` as subprocesses and returns their output; the tools that
change a project (`project_edit`, `preset_apply`, `beats_cut`,
`beats_accent`, `effect_define`) call the same `edits_core` functions as the
CLI (`edits_core::edit`, `preset`, `beatops`) in-process, so they can refuse
files outside the workspace before anything is written. It implements MCP revision 2026-07-28
through the official Rust SDK (`rmcp`): no `initialize` handshake, no
session, every request self-contained. Clients that still send `initialize`
(2025-11-25 and earlier) are served too. Because the protocol is stateless,
the server holds nothing between calls: the project path, the analysis path
and the job id are arguments.

### Tools

Listed in the order `tools/list` returns them. R = read-only, W = writes
files, D = can overwrite or remove something.

| Tool | | Purpose |
| --- | --- | --- |
| `audio_analyze(input, output?, bpm?, beats_per_bar?, project?)` | W | Song → tempo, beats, bars, kick/snare/hat onsets, energy, sections, the drop; written as JSON; with `project`, tempo and markers go into it |
| `beats_accent(project, preset, on, audio?, layer?, vars?, from?, to?, min_strength?, every_nth?, dry_run?)` | D | A preset on every beat, offbeat, downbeat, kick, snare, hat or the drop, as one step |
| `beats_cut(project, slots, every? or on?, from?, to?, audio?, min_strength?, prefix?, above? or below?, dry_run?)` | D | One image layer per beat (or bar, or onset), cycling through slots, as one step |
| `doctor()` | R | Workspace root, the executables found, graphics adapter and backend, ffmpeg encoders |
| `effect_define(project, name, wgsl, params?, layer?)` | D | Check a WGSL custom effect (line numbers on error, nothing written), save it as `shaders/<name>.wgsl` and add it to a layer or a new adjustment layer |
| `effects_list(kind?)` | R | The effect registry and the transitions: parameters, defaults, units, meaning |
| `job_cancel(job)` | D | Stop a background job; a cancelled render's partial file is removed |
| `job_status(job?)` | R | State, progress and result of a job; without `job`, the recent jobs |
| `preset_apply(project, preset, layer?, at?, vars?, audio?, dry_run?)` | D | Apply a preset (or `transition:<kind>` on the incoming layer) at one or more times, as one step |
| `presets_list(name?, tag?, project?)` | R | The presets with their typed variables; with `name`, one in full |
| `project_check(project)` | R | Errors and warnings, each with a JSON pointer and a hint |
| `project_describe(project)` | R | Compact text outline: read this first |
| `project_edit(project, ops, dry_run?)` | D | Apply a batch of edits as one validated step |
| `project_fit(project, analysis, shots?, params?, budget?, dry_run?)` | D | Background job: tune effect values until the render matches a reference in signature space; one history entry |
| `project_read(project, pointer?)` | R | The JSON, or the value at a pointer (one layer, one effect) |
| `project_undo(project)` | D | Restore the version before the newest edit in the shared history (yours or the person's) |
| `reference_analyze(video, out_dir, bpm?, beat_offset?, whole?)` | W | Background job: extract the recipe of a finished edit |
| `reference_compare(analysis, video, top?)` | R | Scores and ranked differences in signature space |
| `reference_draft(analysis, output, proxies?, width?, height?, single_loop?, no_snap?)` | D | Write a project that replays an analysed recipe |
| `reference_probe(video, from_frame?, to_frame?)` | R | Per-frame luminance, saturation, sharpness, difference: summarised |
| `reference_read(analysis, section?, shot?, pointer?)` | R | One section of an analysis instead of the whole file |
| `render_frame(project, at? or frame?, scale?, samples?)` | R | One frame, returned as an image |
| `render_sheet(project, from?, to?, count? or every_frame?, cols?, scale?, samples?)` | R | A labelled grid of frames, returned as one image |
| `render_video(project, output, from?, to?, scale?, samples?)` | D | Background job: render the MP4 |
| `ui_context(project)` | R | What the person has selected and is looking at in the preview window, and their note for you; or that the window is not open |

Every time argument (`at`, `from`, `to`) is a number in the project's time
base or text: `"b12.5"`, `"bar 4"`, `"4:3"`, `"2.5s"`, `"f60"`, a marker such
as `"drop"`, `"drop+2b"` ([FORMAT.md](FORMAT.md#times-as-text)). In
`project_edit`, a layer's `in` / `out` and a key's `t` can be written the same
way.

Results carry a text block written to be read, and for data tools the same
facts as `structuredContent`. `render_frame` and `render_sheet` return an
image block followed by one line of text (frame number, time, size, adapter,
where the file was saved).

`edits-mcp --tools` prints the definitions (names, descriptions, input
schemas, annotations) as JSON without starting a server.

### Addressing

Layers are addressed by `id`, never by position, so an inserted layer does
not invalidate what you knew. Pointers are JSON pointers with one addition:
in an array, `@name` selects the element whose `id` is `name`, or, where the
elements have no id (effects), whose `kind` is `name`.

```
/layers/@finish                                   one layer
/layers/@finish/effects/@exposure/params/stops    one parameter
/layers/@a/effects/0                              an effect by index
/slots/p1/path                                    a slot's picture
```

`project_edit` takes `ops`, applied in order:

| `op` | Fields | Effect |
| --- | --- | --- |
| `set` | `pointer`, `value` | Set a value. Missing objects on the way are created; `/…/effects/-` appends to an array. |
| `remove` | `pointer` | Remove a key or an array element. |
| `insert_layer` | `layer`, `above` or `below` (a layer id) | Insert a layer. With neither, it goes on top. |
| `replace_layer` | `id`, `layer` | Replace a layer in place. |
| `remove_layer` | `id` | Remove a layer. |
| `set_slot` | `name`, `path`, `fit`, `focus` | Create or update a slot; only the given fields change. |

### Background jobs

A full render or an analysis outlasts a tool call (Codex stops waiting after
60 s by default). `render_video` and `reference_analyze` therefore start a
detached process and return a job id at once; `job_status` reports
`running` (with frames done and time left for renders), `completed` (with
the result), `failed` (with the error) or `cancelled`. `reference_compare`
answers inline when it is quick and hands back a job id when it is not.

A job is a folder, `<root>/.edits/jobs/<id>/` (command, pid, stdout, log,
exit code), so any later call, even to a restarted server, can report on it,
and the job keeps running if the client disconnects.

### Making an edit from a song: the high-level tools

Instead of writing keyframes by hand, build the skeleton and the hits with a
few calls, then refine with `project_edit`:

1. `audio_analyze {input: "song.mp3", project: "edit/project.json"}`: the
   tempo, bars, onsets and the drop, written to `song.audio.json`; the
   project gets the tempo and markers (`drop`, the sections).
2. `beats_cut {project, slots: ["p1", "p2", "p3"], every: 1, from: "bar 1", to: "drop"}`,
   then a faster cut after the drop: `{every: 0.5, from: "drop", to: "drop+16b"}`,
   or `{on: "kicks", audio: "song.audio.json"}`.
3. `beats_accent {project, preset: "flash", on: "downbeats"}`,
   `{preset: "punch", on: "kicks", audio}`, `{preset: "shake", on: "drop", audio}`.
   `layer: "fx"` puts the accents on one adjustment layer instead of each picture.
4. `preset_apply {project, preset: "transition:whip_pan", layer: "cut9"}` at a
   cut; `presets_list` shows the rest (`velocity`, `flip_3d`, `rgb_split`,
   `letterbox`, ...) and `presets_list {name}` their variables.
5. `render_sheet` (16 frames), or `{from: "drop-1b", to: "drop+1b", every_frame: true}`
   around a hit, and look. Every step above is one history entry:
   `project_undo` takes back the last one whole.

### What the person is looking at: `.edits/ui.json`

The preview window (`edits-gui`) shares what the person has selected and
where they are, through a file next to the project. `ui_context` reads it, so
"make *this* hit stronger" can be resolved to a layer and a time. The format,
`edits.ui/1` (type `edits_core::ui::UiContext`, with `edits_core::ui::write`
for the writing side):

```json
{
  "format": "edits.ui/1",
  "project": "project.json",
  "updated": "20261113T120000.000Z",
  "pid": 41213,
  "playhead": { "t": 4.25, "seconds": 2.07, "frame": 51 },
  "playing": false,
  "selection": { "layer": "cut5", "effect": 0, "property": "/layers/@cut5/transform/scale", "key": 2 },
  "visible": { "from": 0, "to": 16 },
  "loop": { "from": 4, "to": 8 },
  "note": "make this hit stronger",
  "note_at": "20261113T115930.000Z"
}
```

- Written to `<project dir>/.edits/ui.json` atomically (write a temporary
  file, rename), whenever the selection, playhead, view or note changes, and
  at least every 5 s while the window is open; removed when it closes.
- `format`, `project` (the project's file name) and `updated` (UTC, in the
  format of `edits_core::edit::utc_stamp`) are required; everything else is
  optional and omitted when unknown. Times are project time (beats or
  seconds, as the project's `time_base`); `property` is a pointer
  `project_edit` accepts; `effect` and `key` are indices.
- `ui_context` answers "the window is not open" when there is no file, and
  "the window seems closed" (with the last state) when `updated` is more than
  30 s old.

## The perception ladder

Look at the cheapest thing that answers the question. Each step costs more
tokens or more time than the one before.

| Level | What you get | Shell | MCP | Cost |
| --- | --- | --- | --- | --- |
| 0. Structure | Text outline, or one node of the JSON | `edits describe` | `project_describe`, `project_read` | A few hundred tokens |
| 1. Numbers | Findings; per-frame signatures; scores and ranked differences against a reference | `edits check --json`, `edits-analyze probe`, `edits-analyze compare` | `project_check`, `reference_probe`, `reference_compare` | A few hundred to 2,000 tokens |
| 2. One frame | The picture at one moment | `edits frame --at T` | `render_frame` | About 400 tokens at the default size; about a second |
| 3. A sheet | 16 frames of the edit, or every frame around a hit, labelled | `edits sheet` | `render_sheet` | 2,000 to 4,800 tokens; a few seconds |
| 4. The video | The real thing, for the person | `edits render` | `render_video` + `job_status` | No tokens; seconds to minutes |

Numbers come before pixels because they are exact and cheap: "the cut at
beat 3.5 is missing" is one line. Pixels are for what numbers cannot say: a
cropped face, the wrong side of a cube, an ugly overlay.

Image cost: on Claude an image costs about (width / 28) x (height / 28)
tokens. The server renders frames with a long edge of about 540 px unless
asked otherwise, scales any image down to fit 4,800 tokens, and re-encodes
as JPEG when a PNG would exceed 0.9 MB, so one result stays far below a
client's limit for tool results (25,000 tokens in Claude Code). The
full-quality PNG is always saved under `<root>/.edits/renders/` and its path
is in the text.

When the task is to match a reference edit that uses other pictures, pixel
comparison says nothing. `reference_compare` works in signature space: each
video is measured relative to its own shots, the pictures cancel, and what
is compared is timing and effect curves. See [ANALYSIS.md](ANALYSIS.md).

## Review and undo

- **An edit is all or nothing.** `project_edit` applies the whole batch to a
  copy, parses the result as a project and runs the same validation as
  `edits check`. With any error it writes nothing and returns the findings
  with pointers. `dry_run` does the same and never writes.
- **Every write is reported.** The result lists what changed, by pointer
  (`~ /layers/@finish/effects/0/params/stops: 0 → 3.5`), and the warnings.
  It also warns about keys the renderer would ignore, which catches typos
  such as `"transfrom"`.
- **Every write can be undone.** Before a project file is replaced, the
  previous version is copied to `<project folder>/.edits/history/` (the last
  50 are kept), with a note of who made the edit (`agent` for the server,
  `window` for a person in `edits-gui`) and what it did. `project_undo`
  restores the newest one; calling it again goes further back. The version an
  undo replaces is kept under `.edits/undone/`.
- **The file is the review surface.** It is written with a fixed key order
  and one value per line where lines get long, so `git diff` on a project
  shows only what changed. The first edit through the server reformats a
  hand-written file once.
- **Edits made by hand are respected.** The server reads the file fresh on
  every call and keeps no copy, so a person can edit the same project between
  calls, and the preview window ([GUI.md](GUI.md)) follows every change.

### The person and the agent share one history

The window (`edits-gui`) is also an editor, and its edits go through the same
code as `project_edit` (`edits_core::edit`): validated the same way, written
atomically, and saved in the same `.edits/history/`. So there is one history
for the project, not one per side:

- **`project_undo` undoes the newest edit, whoever made it.** If the person
  changed something in the window after your last `project_edit`, your undo
  takes back *their* change, not yours. The answer says whose it was
  (`undid an edit made by the person in the edits-gui window: add glow to
  tex`, and `undone_edit: {actor, summary}` in the structured result). Read
  that line; when it was not yours, tell the person, or redo it with
  `project_edit`.
- **The person's Ctrl+Z in the window does the same**, so it can take back
  your last edit. The window's History list shows each entry as `you` or
  `agent`.
- **Redo** exists in the window (Ctrl+Shift+Z) until the next edit by either
  side; there is no redo tool for the agent. A new edit from either side
  clears it (the undone versions stay in `.edits/undone/`).
- **Neither clobbers the other.** Each step locks the history
  (`.edits/history/<name>.lock`), so simultaneous edits are applied one after
  the other, each as its own entry. The window refuses an edit it prepared
  against a version that is no longer on disk (your edit landed meanwhile)
  and reloads; your `project_edit` always applies to the file as it is on
  disk, including the person's latest change, so read before you overwrite a
  value the person may be working on (`project_read`).

`.edits/` folders carry their own `.gitignore`; nothing in them is committed.

## Security

- **Path confinement.** Every path argument must resolve inside the workspace
  root: the `--root` argument, else `EDITS_ROOT`, else the directory the
  server was started in. `..`, absolute paths and symbolic links that lead
  out are refused with a message that names the root. The files a project
  refers to (slot pictures, audio, shader sources) are held to the same rule:
  `project_edit` rejects an edit that points outside, and the render tools
  refuse a project that does. This applies to the MCP server; the CLI reads
  whatever paths it is given.
- **No shell.** Subprocesses are started with an argument vector. Each has a
  time limit (`EDITS_MCP_TIMEOUT`, default 50 s for foreground tools) and a
  cap on the output that is kept.
- **Custom shaders are validated.** A `custom` effect is a WGSL fragment
  function. `effect_define` checks it (without a GPU) before writing it, and
  writes only `shaders/<name>.wgsl` next to the project. It is parsed and
  validated again when the project is loaded and runs
  on the GPU (or the CPU rasteriser) with no access to files or the network;
  it can read the layer's picture and eight numbers. A shader that does not
  validate stops the render with the compiler's message.
- **Nothing leaves the machine.** No tool makes a network request. Media is
  read from local files and decoded and encoded by the local `ffmpeg`. (The
  only network use is `cargo` downloading crates when you build from source.)
- **What an agent can do through the server**: write and overwrite files
  inside the root (project files, renders, analysis folders, `.edits/`), and
  spend CPU time. `project_edit`, `project_undo`, `preset_apply`,
  `beats_cut`, `beats_accent`, `effect_define`, `reference_draft`,
  `render_video` and `job_cancel` are annotated as destructive so that a
  client can ask before running them.

## Client setup

Build once (a release build of the three packages; the server looks for the
other two executables next to itself, then on `PATH`):

```
cargo build --release -p edits-cli -p edits-analyze -p edits-mcp
```

### Claude Code

The repository's [`.mcp.json`](../.mcp.json) registers the server for anyone
who opens the checkout:

```json
{ "mcpServers": { "edits": { "command": "./scripts/edits-mcp.sh", "args": [] } } }
```

`scripts/edits-mcp.sh` starts the built `target/release/edits-mcp` at once.
Run the build above first: when the binaries are missing the script exits
with the build command on stderr, rather than compiling while the client
waits (Claude Code gives a server 30 seconds to start). The workspace root is
the folder Claude Code was opened in.

Faster, and usable from any folder: point the client at the built binary.

```
claude mcp add --transport stdio edits -- /path/to/Edits-creation-tool/target/release/edits-mcp --root /path/to/your/workspace
```

or put the three binaries on `PATH` (`cargo install --path crates/edits-cli`,
`--path crates/edits-analyze`, `--path crates/edits-mcp`) and use
`"command": "edits-mcp"`.

The skill in `.claude/skills/edits/` is picked up automatically in this
repository. Copy that folder to `~/.claude/skills/` to have it everywhere.

### Codex

`~/.codex/config.toml` (or `.codex/config.toml` in a trusted project):

```toml
[mcp_servers.edits]
command = "/path/to/Edits-creation-tool/target/release/edits-mcp"
args = ["--root", "/path/to/your/workspace"]
# startup_timeout_sec = 10 and tool_timeout_sec = 60 are Codex's defaults; the
# server answers within 50 s and runs long work as jobs, so they can stay.
```

or `codex mcp add edits -- /path/to/edits-mcp --root /path/to/your/workspace`.

The skill is plain Markdown: tell Codex to read
`.claude/skills/edits/SKILL.md`, or copy it to where your Codex version keeps
skills.

### Configuration

| Setting | Meaning | Default |
| --- | --- | --- |
| `--root DIR`, `EDITS_ROOT` | Workspace root | The server's working directory |
| `--edits PATH`, `EDITS_BIN` | The `edits` executable | Next to `edits-mcp`, then `PATH` |
| `--edits-analyze PATH`, `EDITS_ANALYZE_BIN` | The `edits-analyze` executable | Next to `edits-mcp`, then `PATH` |
| `EDITS_MCP_TIMEOUT` | Seconds a foreground tool may run | 50 |
| `EDITS_ADAPTER`, `EDITS_FFMPEG` | Passed on to `edits` (see [CLI.md](CLI.md)) | |

Call `doctor` first in a new setup: it prints the root, which executables
were found, and whether this machine can render.

### As a library

```rust
edits_mcp::serve_stdio(edits_mcp::Config::from_env())?;   // blocks until the client disconnects
```

`Config` has `root`, `edits_bin`, `analyze_bin` and `timeout`. The library
has no `main`-only assumptions, so a subcommand of another binary can call it.

## Not there yet

- MCP tasks (the `io.modelcontextprotocol/tasks` extension) are not used;
  jobs are ordinary tools, which every client can call.
- No MCP resources or prompts; the skill is a file, not served over MCP.
- The window does not write `.edits/ui.json` yet: the format above is the
  contract for it, and `ui_context` already reads it.
- Presets are applied, not previewed: there are no poster images; look with
  `render_sheet` after `preset_apply` (`dry_run` reports the changes only).
- No tool isolates a single layer in a render, overlays ids or motion paths
  on a frame, or shows a frame side by side with the reference. Use
  `enabled: false` on other layers (and `project_undo`) for the first.
- Job cancellation and process groups are implemented for Unix; on Windows
  the code path uses `taskkill` and has not been run.
