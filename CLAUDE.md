# CLAUDE.md

Notes for agents working on **this codebase**. To *use* the engine (make or
change an edit), read `.claude/skills/edits/SKILL.md` and `docs/AGENTS.md`
instead.

## What this is

`edits` is a Rust engine for short beat-synced video edits: pictures cut to
music with flashes, scale punches, blur, 3D cubes and overlays. A project is
one JSON file (`edits.project/1`); the renderer turns it into frames on the
GPU (or a CPU rasteriser) and pipes them to `ffmpeg`. It is meant to be driven
by coding agents: every operation is a headless command with JSON output, and
there is an MCP server over the same commands.

## Layout

A Cargo workspace; every crate is under `crates/`.

| Crate | Binary | Owns |
| --- | --- | --- |
| `edits-core` | | The project model (`model.rs`), keyframe and easing evaluation (`eval.rs`, `ease.rs`), **the effect registry** (`effects/`, one file per category), validation (`check.rs`) and the text outline (`describe.rs`). No GPU or media dependencies; everything else depends on it. |
| `edits-render` | | The wgpu renderer: project + frame number -> RGBA image. Layers, blend modes, effects (`fx.rs`, `compose.rs`, `shaders/*.wgsl`), cubes, motion blur, custom WGSL effects. |
| `edits-cli` | `edits` | The command line: `render`, `frame`, `sheet`, `bench`, `describe`, `check`, `effects`, `doctor`, `demo`; `analyze`, `draft`, `compare`, `probe` through the `edits-analyze` library (`analysis.rs`); `gui` and `mcp` start the sibling executables (`launch.rs`). Encoding through the system `ffmpeg` (`encode.rs`), contact sheets (`sheet.rs`), the generated demo (`demo.rs`). |
| `edits-analyze` | `edits-analyze` | Reference analysis: `analyze` (recipe of a finished edit -> `reference_analysis.json`), `draft` (analysis -> project), `compare` (render vs analysis in signature space), `probe` (per-frame signatures). |
| `edits-mcp` | `edits-mcp` | The MCP server (stdio, `rmcp`). A thin adapter that runs `edits` and `edits-analyze` as subprocesses, plus `project_edit` / `project_undo` (validated, atomic edits with history), path confinement, background jobs and image size caps. |
| `edits-gui` | `edits-gui` | A desktop window that previews a project while something else edits it (transport, timeline, live reload). It does not edit projects. |
| `edits-calib` | | Round-trip calibration: random recipes with known parameters are rendered, analysed and scored against the truth. |

Other folders: `docs/` (format, effects, CLI, rendering, analysis, agents, window),
`examples/` (`beat-loop.json`, example shaders), `.claude/skills/edits/` (the
skill for agents using the engine), `.mcp.json` (registers the MCP server for
Claude Code through `scripts/edits-mcp.sh`, which starts the built binary).

## Build and test

```
cargo build --release -p edits-cli -p edits-analyze -p edits-mcp   # target/release/{edits,edits-analyze,edits-mcp}
cargo test -p edits-core                                           # model, easing, check
cargo test -p edits-render                                         # pixel tests; skip without an adapter
cargo test -p edits-cli
cargo test -p edits-analyze                                        # synthetic clips only, no footage needed
cargo test -p edits-mcp                                            # unit tests + a stdio session against the built server
cargo clippy -p <crate> --all-targets
```

- **No GPU is needed.** The renderer runs on any wgpu adapter, including the
  CPU rasteriser Mesa lavapipe (software Vulkan). On Debian or Ubuntu without
  a GPU: `apt install mesa-vulkan-drivers`. `edits doctor` says which adapter
  is used; `EDITS_ADAPTER=software` forces the CPU one. Renderer tests skip
  (with a note on stderr) when there is no adapter at all.
- **`ffmpeg` and `ffprobe` must be on `PATH`** for `edits render` and for
  everything in `edits-analyze`. `EDITS_FFMPEG` names another binary.
- **The MCP integration tests need the `edits` executable** (and use
  `edits-analyze` when it is there). They look in `EDITS_BIN`, next to the
  test's server binary and in the workspace's `target*` folders, and skip
  with a note when it is missing. Build `edits-cli` first.
- Use the default `target/` directory. Folders named `target-*` are leftovers
  of parallel builds and are ignored by git; nothing depends on them.
- `edits-gui` pulls in windowing and audio libraries; build it only when you
  work on it (`cargo build -p edits-gui`), not as part of every check.

## Run the demo

```
cargo build --release -p edits-cli
./target/release/edits doctor                                # adapter and encoders; must end with "ready to render"
./target/release/edits demo demo                             # demo/project.json + generated pictures and a click track
./target/release/edits describe demo/project.json
./target/release/edits sheet demo/project.json -o sheet.png  # 16 labelled frames: view this image
./target/release/edits render demo/project.json -o demo.mp4  # about 36 s on 4 CPU cores without a GPU; --scale 0.5 takes about 20 s
```

The MCP server, by hand: `./target/release/edits-mcp --tools` prints the tool
definitions; `docs/AGENTS.md` has the client setup.

## Conventions

- **Effects are defined once**, in `edits-core::effects::EFFECTS` (kind,
  category, parameters with units, ranges and defaults, options, docs,
  credit; one file per category in `effects/`). The renderer implements
  exactly that list, `check` validates against it, `edits effects` and the
  MCP `effects_list` print it, and `docs/EFFECTS.md` explains it. To add an
  effect: add it to the registry, implement it in `edits-render` (a one-pass
  effect is a WGSL file in `shaders/fx/` plus a packer in `fxlib/`), add a pixel test in
  `crates/edits-render/tests/effects.rs`, document it in `docs/EFFECTS.md`.
  Never add an effect in the renderer alone.
- **The project model is `edits-core::model`.** `docs/FORMAT.md` is its short
  version; change both together. Times are in the project's time base
  (beats or seconds), space in units of composition height, effect sizes in
  percent of height. Nothing in a project is in pixels.
- **Every CLI command is usable by a program**: data on stdout as JSON
  (`--json` where a command also has a text form), progress and errors on
  stderr, a non-zero exit code and a line starting `error:` on failure. An
  error message says what to do next. New commands follow this, and get an
  entry in `docs/CLI.md`.
- **The MCP server stays thin.** A new capability goes into the CLI first,
  then gets a tool in `crates/edits-mcp/src/server.rs` that calls it. Tools
  are stateless (paths and job ids are arguments), confine every path to the
  workspace root (`paths.rs`), never use a shell, and keep results small.
  When the tool list changes, update `docs/AGENTS.md`, the skill, and the
  list in `crates/edits-mcp/tests/stdio.rs`.
- **A frame is a pure function of the project and the frame number.** No
  wall-clock time, no unseeded randomness, in the renderer or in effects.
- **Never commit media**: no pictures, audio, video, renders or analyses.
  Tests and the demo generate what they need in code. `.gitignore` covers
  `*.mp4`, `*.mov`, `/out`, `/analysis`, `/assets`; keep working files there
  or outside the repository.
- **Tests use generated content.** `edits-render` renders tiny procedural
  pictures; `edits-analyze` synthesises clips with known parameters;
  `edits-mcp` runs `edits demo` into a temporary folder.
- **Docs are written for a reader who has not seen the code**: say what a
  thing does and what to type, with real output. When behaviour changes,
  change the page that describes it in the same commit.

## Where things are documented

| Page | Content |
| --- | --- |
| `docs/FORMAT.md` | The project format |
| `docs/EFFECTS.md` | Every effect, and how to write a custom one |
| `docs/CLI.md` | The `edits` command, with example output |
| `docs/RENDERING.md` | Colour, geometry and what the renderer does to be fast |
| `docs/ANALYSIS.md` | `edits-analyze`: stages, the analysis file, draft, compare |
| `docs/AGENTS.md` | Driving the engine from an agent: JSON, CLI, MCP tools, security, client setup |
| `docs/GUI.md` | The preview window |
