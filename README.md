# edits

An engine for short beat-synced video edits: pictures cut to music with
exposure flashes, scale punches, blur, 3D cubes and texture overlays. The kind
of thing usually made by hand in After Effects, CapCut or Alight Motion.

It is built to be driven by a coding agent (Claude Code, Codex) as much as by
a person:

- **A project is one JSON file**: slots (the pictures), layers, keyframes and
  effects. Time is in beats, space in units of the frame height, so the same
  file renders identically at any resolution.
- **Everything is a headless command** with JSON output: validate, describe,
  render one frame, render a labelled contact sheet, render the video.
- **An MCP server** exposes the same operations as tools, with edits that are
  validated before they are written and can be undone, and frames returned as
  images the agent can look at.
- **The recipe of an existing edit can be extracted** and replayed with other
  pictures (see below).

Written in Rust. Rendering uses `wgpu`; encoding uses the system `ffmpeg`.

## Status

Early. The core loop works end to end on Linux; much of what a finished tool
needs is not there yet.

| Works today | |
| --- | --- |
| Project format `edits.project/1` | Image, solid, 3D cube and adjustment layers; keyframes with easing (including the exponential `decay` of this genre); shake and loop modifiers; eight blend modes |
| Renderer | 18 built-in effects (exposure, four kinds of blur, glow, vignette, RGB split, threshold, ...) and custom WGSL effects; motion blur; linear-light compositing; runs on a CPU rasteriser when there is no GPU |
| `edits` CLI | `render` (H.264 MP4 with AAC audio), `frame`, `sheet`, `bench`, `describe`, `check`, `effects`, `doctor`, `demo` |
| `edits-analyze` | `analyze` (tempo, loop, cuts, flashes, tone hits, blur, 2D motion curves, overlay, vignette), `draft`, `compare`, `probe` |
| `edits-mcp` | 17 tools over stdio (MCP 2026-07-28, also serves older clients): project read/edit/undo/check, frame and sheet images, background render and analysis jobs, reference tools |
| Agent files | A skill with step-by-step workflows, `CLAUDE.md`, `.mcp.json` |

| In progress | |
| --- | --- |
| `edits-gui` | A read-only preview window with transport and timeline that follows a project file while an agent edits it ([docs/GUI.md](docs/GUI.md)) |
| `edits-calib` | Measuring how accurate the analysis is, on generated edits with known parameters |

| Not there | |
| --- | --- |
| Video clips as layers | Sources are still pictures |
| Text and shape layers | |
| Audio analysis on its own | Tempo is measured from a video's audio track; for a bare song, wrap it in a blank video first (the skill shows how) or supply the BPM |
| Analysis of 3D moves | Cube turns, picture-in-picture and whip pans are detected and flagged `needs_agent`, not measured; the drafted project leaves them to you |
| Output formats | H.264 MP4 only |
| Platforms | Developed and tested only on Linux with software Vulkan (Mesa lavapipe). `wgpu` also targets hardware GPUs, macOS and Windows; those are untested |
| Packaging | No releases or installers; build from source |

## Quick start (Linux)

You need a recent stable Rust toolchain (the code is built with 1.97), `ffmpeg`
with an H.264 encoder, and a Vulkan driver. A GPU is not required: without
one, install Mesa's CPU rasteriser.

```
sudo apt install ffmpeg mesa-vulkan-drivers        # Debian, Ubuntu
cargo build --release -p edits-cli -p edits-analyze -p edits-mcp
```

```
./target/release/edits doctor                                  # which adapter and encoders; must end with "ready to render"
./target/release/edits demo demo                               # writes demo/project.json, pictures and a click track
./target/release/edits sheet demo/project.json -o sheet.png    # 16 labelled frames of the edit in one image
./target/release/edits render demo/project.json -o demo.mp4    # the video
```

The demo is 8 beats at 123 BPM, 1080 x 1080, 98 frames. On four CPU cores
without a GPU it renders in about 36 seconds; `--scale 0.5` takes about 20.

To make the same edit with your own pictures, change the `path` of each slot
in `demo/project.json` and render again.

## Extracting a recipe

1. A finished edit is pictures plus a recipe: tempo, where the cuts fall, how
   hard and how fast each flash and punch is, blur, tone hits, overlay.
2. Inside a shot the picture does not change. Measuring every frame against
   one reference frame of its own shot cancels the picture and leaves the
   effect.
3. `edits-analyze analyze ref.mp4 -o analysis/` measures the recipe this way,
   with a confidence for each number, and says which shots it could not
   measure.
4. `edits-analyze draft analysis/reference_analysis.json -o project.json`
   writes the recipe as a project with empty slots: put your pictures in.
5. `edits-analyze compare analysis/reference_analysis.json render.mp4` scores
   your render against the reference in the same terms and lists the largest
   differences in plain language. Pixels are never compared, so different
   pictures are not a difference. Fix, render, compare, repeat.

The analysis is honest about its limits: flashes clipped at white are lower
bounds, a picture's absolute size is unknowable, and 3D moves are left to
whoever is driving. [docs/ANALYSIS.md](docs/ANALYSIS.md) lists every stage
with its failure modes.

## With an agent

Open the checkout in Claude Code: `.mcp.json` registers the MCP server and
`.claude/skills/edits/` teaches the workflows. Build first (the command
above), then ask for an edit. For Codex and for use outside the checkout, see
[docs/AGENTS.md](docs/AGENTS.md).

```
./target/release/edits-mcp --tools      # the tool definitions, as JSON
```

## Documentation

| | |
| --- | --- |
| [docs/FORMAT.md](docs/FORMAT.md) | The project format |
| [docs/EFFECTS.md](docs/EFFECTS.md) | Every effect and its parameters; writing a custom effect in WGSL |
| [docs/CLI.md](docs/CLI.md) | The `edits` command |
| [docs/RENDERING.md](docs/RENDERING.md) | Colour, geometry, performance |
| [docs/ANALYSIS.md](docs/ANALYSIS.md) | Reference analysis, drafting and comparison |
| [docs/AGENTS.md](docs/AGENTS.md) | Driving the engine from an agent: JSON, CLI, MCP tools, security, client setup |
| [docs/GUI.md](docs/GUI.md) | The preview window (in progress) |
| [.claude/skills/edits/SKILL.md](.claude/skills/edits/SKILL.md) | Step-by-step workflows and starting values for this genre |
| [CLAUDE.md](CLAUDE.md) | For work on this codebase: layout, build, conventions |
| [examples/beat-loop.json](examples/beat-loop.json) | A complete project |

## Licence

MIT or Apache-2.0, at your option (as declared in `Cargo.toml`).
