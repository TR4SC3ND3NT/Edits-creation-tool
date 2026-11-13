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
`edits-analyze` as subprocesses and returns their output; `project_edit` is
the one tool with logic of its own. It implements MCP revision 2026-07-28
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
| `doctor()` | R | Workspace root, the executables found, graphics adapter and backend, ffmpeg encoders |
| `effects_list(kind?)` | R | The effect registry: parameters, defaults, units, meaning |
| `job_cancel(job)` | D | Stop a background job; a cancelled render's partial file is removed |
| `job_status(job?)` | R | State, progress and result of a job; without `job`, the recent jobs |
| `project_check(project)` | R | Errors and warnings, each with a JSON pointer and a hint |
| `project_describe(project)` | R | Compact text outline: read this first |
| `project_edit(project, ops, dry_run?)` | D | Apply a batch of edits as one validated step |
| `project_read(project, pointer?)` | R | The JSON, or the value at a pointer (one layer, one effect) |
| `project_undo(project)` | D | Restore the version before the last edit |
| `reference_analyze(video, out_dir, bpm?, beat_offset?, whole?)` | W | Background job: extract the recipe of a finished edit |
| `reference_compare(analysis, video, top?)` | R | Scores and ranked differences in signature space |
| `reference_draft(analysis, output, proxies?, width?, height?, single_loop?, no_snap?)` | D | Write a project that replays an analysed recipe |
| `reference_probe(video, from_frame?, to_frame?)` | R | Per-frame luminance, saturation, sharpness, difference: summarised |
| `reference_read(analysis, section?, shot?, pointer?)` | R | One section of an analysis instead of the whole file |
| `render_frame(project, at? or frame?, scale?, samples?)` | R | One frame, returned as an image |
| `render_sheet(project, from?, to?, count? or every_frame?, cols?, scale?, samples?)` | R | A labelled grid of frames, returned as one image |
| `render_video(project, output, from?, to?, scale?, samples?)` | D | Background job: render the MP4 |

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
  50 are kept). `project_undo` restores the newest one; calling it again goes
  further back. The version an undo replaces is kept under `.edits/undone/`.
- **The file is the review surface.** It is written with a fixed key order
  and one value per line where lines get long, so `git diff` on a project
  shows only what changed. The first edit through the server reformats a
  hand-written file once.
- **Edits made by hand are respected.** The server reads the file fresh on
  every call and keeps no copy, so a person can edit the same project between
  calls, and the preview window ([GUI.md](GUI.md)) follows every change.

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
  function. It is parsed and validated when the project is loaded and runs
  on the GPU (or the CPU rasteriser) with no access to files or the network;
  it can read the layer's picture and eight numbers. A shader that does not
  validate stops the render with the compiler's message.
- **Nothing leaves the machine.** No tool makes a network request. Media is
  read from local files and decoded and encoded by the local `ffmpeg`. (The
  only network use is `cargo` downloading crates when you build from source.)
- **What an agent can do through the server**: write and overwrite files
  inside the root (project files, renders, analysis folders, `.edits/`), and
  spend CPU time. `project_edit`, `project_undo`, `reference_draft`,
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
- No tool isolates a single layer in a render, overlays ids or motion paths
  on a frame, or shows a frame side by side with the reference. Use
  `enabled: false` on other layers (and `project_undo`) for the first.
- Job cancellation and process groups are implemented for Unix; on Windows
  the code path uses `taskkill` and has not been run.
