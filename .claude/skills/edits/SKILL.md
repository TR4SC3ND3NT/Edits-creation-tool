---
name: edits
description: Make, change and render short beat-synced video edits (pictures cut to music with flashes, scale punches, blur, 3D cubes, overlays) with the `edits` engine, and recreate the recipe of a reference edit with different pictures. Use when asked to make an edit from a song and pictures, to copy the timing or style of an existing edit, to change or render a project JSON of format `edits.project/1`, to preview frames of one, or to write a custom WGSL effect for it.
---

# edits

`edits` renders a video from one JSON file. You do not operate an editor: you
write the JSON, check it, render pictures of it, look at them, and repeat.

Two routes do the same things. Use the MCP tools when the `edits` server is
connected (tool names such as `project_edit`, `render_sheet`); otherwise use
the shell. They can be mixed: the project file is the only state.

| Step | Shell | MCP tool |
| --- | --- | --- |
| Is this machine ready? | `edits doctor` | `doctor` |
| Outline of a project | `edits describe p.json` | `project_describe` |
| One part of the JSON | read the file | `project_read` with a pointer |
| Change it | edit the file | `project_edit` (atomic, validated, undoable) |
| Validate | `edits check p.json --json` | `project_check` |
| Effect reference | `edits effects` | `effects_list` |
| One frame | `edits frame p.json --at T -o f.png --scale 0.5`, then view `f.png` | `render_frame` (returns the image) |
| Many frames in one image | `edits sheet p.json -o s.png`, then view `s.png` | `render_sheet` (returns the image) |
| The video | `edits render p.json -o out.mp4` | `render_video`, then `job_status` |
| Recipe of a reference edit | `edits-analyze analyze / draft / compare / probe` | `reference_analyze`, `reference_read`, `reference_draft`, `reference_compare`, `reference_probe` |

From a checkout the binaries are `target/release/edits` and
`target/release/edits-analyze` after `cargo build --release`. With MCP every
path must be inside the workspace root (`doctor` prints it); relative paths
start there.

## Mental model

A project is **slots + layers + keyframes + effects**.

- **Slots** are named pictures: `"slots": { "p1": { "path": "img/01.jpg", "fit": "cover", "focus": [0.5, 0.4] } }`.
  Layers refer to slots by name. To make the same edit with other pictures,
  change only the slot paths. Paths are relative to the project file.
- **Layers** are listed **bottom to top**. Kinds: `image` (one slot), `solid`,
  `cube` (a real 3D box, one slot per face), `adjustment` (no picture; its
  effects apply to everything beneath it). Each has `id`, `in`, `out`,
  `transform`, `blend`, `effects`.
- **Time is in beats** when `settings.time_base` is `"beats"` (it needs
  `settings.bpm`; beat 0 is `settings.beat_offset` seconds into the video). A
  cut on beat 4 is `"in": 4`. One beat is `fps * 60 / bpm` frames: 12.2 frames
  at 123 BPM and 25 fps, so one frame is 0.082 beats.
- **Space is in units of the composition height.** Origin at the centre, x
  right, y down, z away. A layer with scale 1 fills the frame; position
  `[0.5, 0, 0]` moves it half a frame height to the right. Blur radius and
  similar sizes are percent of the height. Nothing is in pixels, so a render
  at `--scale 0.25` is a faithful small copy of the full one.
- **Any number can be keyframed**: `{ "keys": [ { "t": 4, "v": 1.3, "ease": { "name": "decay", "rate": 0.45 } }, { "t": 5, "v": 1.0 } ] }`.
  `ease` describes the segment from that key to the next. `hold` keeps the
  value until the next key. `decay` is the look of this genre: each frame
  covers `rate` of the remaining distance (a fast start that settles).
- **Effects** are `{ "kind": "exposure", "params": { "stops": ... } }` in a
  layer's `effects` list, applied in order. `edits effects` lists them all.

Full references: `docs/FORMAT.md`, `docs/EFFECTS.md`, `docs/CLI.md`,
`docs/ANALYSIS.md`, `docs/AGENTS.md`. A complete example: `examples/beat-loop.json`
(`edits demo demo` writes it with pictures and audio).

## Workflow 1: an edit from a song and a folder of pictures

1. **Check the machine.** `edits doctor` (MCP: `doctor`). It must end with
   `ready to render`.
2. **Get the tempo.** Ask for the BPM if the user knows it. Otherwise measure
   it: wrap the song in a blank video and analyse that.
   ```
   ffmpeg -v error -y -f lavfi -i color=c=black:s=64x64:r=25 -i song.mp3 -shortest -c:v libx264 -pix_fmt yuv420p -c:a aac tempo.mp4
   edits-analyze analyze tempo.mp4 -o tempo_analysis      # prints: tempo 123.00 bpm (confidence 0.56, audio), beat 0 at -0.009 s
   ```
   Use the printed tempo as `settings.bpm` and the printed "beat 0 at" as
   `settings.beat_offset` (seconds from the start of the video to beat 0; it
   may be slightly negative). To start the edit N seconds into the song, set
   `audio.offset` to N and `beat_offset` to the time of the first beat at or
   after N, minus N.
3. **Look at the pictures** (view each file) and decide the order, which ones
   deserve a long hold, and where each one's subject is (`focus`).
4. **Write the project** next to the pictures. Start from this skeleton: one
   image layer per shot cut on the beat, each with a scale punch and a blur
   that clears; an overlay texture; and one adjustment layer on top that
   carries every flash and the vignette.
   ```json
   {
     "format": "edits.project/1",
     "settings": { "width": 1080, "height": 1920, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 123, "beat_offset": 0.0,
                   "motion_blur": { "samples": 6, "shutter": 0.5 } },
     "audio": { "path": "song.mp3", "offset": 0.0 },
     "slots": {
       "p1": { "path": "img/01.jpg" },
       "p2": { "path": "img/02.jpg", "focus": [0.5, 0.35] },
       "tex": { "path": "img/dust.png" }
     },
     "layers": [
       { "id": "shot1", "kind": "image", "slot": "p1", "in": 0, "out": 4,
         "transform": { "scale": { "keys": [ { "t": 0, "v": 1.3, "ease": { "name": "decay", "rate": 0.45 } }, { "t": 1, "v": 1.0 } ] } },
         "effects": [ { "kind": "gaussian_blur", "params": { "radius": { "keys": [ { "t": 0, "v": 1.2, "ease": "out" }, { "t": 0.4, "v": 0 } ] } } } ] },
       { "id": "shot2", "kind": "image", "slot": "p2", "in": 4, "out": 8,
         "transform": { "scale": { "keys": [ { "t": 4, "v": 1.3, "ease": { "name": "decay", "rate": 0.45 } }, { "t": 5, "v": 1.0 } ] } },
         "effects": [ { "kind": "gaussian_blur", "params": { "radius": { "keys": [ { "t": 4, "v": 1.2, "ease": "out" }, { "t": 4.4, "v": 0 } ] } } } ] },
       { "id": "tex", "kind": "image", "slot": "tex", "blend": "screen", "transform": { "opacity": 0.6 } },
       { "id": "finish", "kind": "adjustment",
         "effects": [
           { "kind": "exposure", "params": { "stops": { "keys": [
               { "t": 0, "v": 3.5, "ease": { "name": "decay", "rate": 0.25 } }, { "t": 1, "v": 0, "ease": "hold" },
               { "t": 4, "v": 3.5, "ease": { "name": "decay", "rate": 0.25 } }, { "t": 5, "v": 0 } ] } } },
           { "kind": "vignette", "params": { "amount": 0.5, "radius": 0.55, "softness": 0.6 } } ] }
     ]
   }
   ```
   No overlay texture? Leave out the `tex` slot and layer.
5. **Check.** `edits check project.json` (MCP: `project_check`). Fix every
   error; read the warnings.
6. **Look.** `edits sheet project.json -o sheet.png` and view the image (MCP:
   `render_sheet`). Sixteen frames of the whole edit, each labelled with frame
   number, beat and seconds. Then look at each hit frame by frame:
   `edits sheet project.json --from 3.9 --to 4.5 --every-frame --cols 4 -o hit.png`
   (MCP: `render_sheet` with `from`, `to`, `every_frame: true`). Check that the
   cut, the flash and the punch start on the same frame, that the flash is
   gone within about a beat, and that no picture shows its edges.
7. **Change and repeat.** Shell: edit the JSON, then `edits check`. MCP:
   ```json
   { "project": "my-edit/project.json", "ops": [
     { "op": "set", "pointer": "/layers/@shot2/out", "value": 6 },
     { "op": "insert_layer", "above": "shot2", "layer": { "id": "shot3", "kind": "image", "slot": "p3", "in": 6, "out": 8 } },
     { "op": "set_slot", "name": "p3", "path": "img/03.jpg", "focus": [0.5, 0.4] },
     { "op": "set", "pointer": "/layers/@finish/effects/-", "value": { "kind": "saturation", "params": { "amount": { "keys": [
         { "t": 0, "v": 1, "ease": "hold" }, { "t": 7, "v": 0, "ease": "hold" }, { "t": 7.33, "v": 1 } ] } } } }
   ] }
   ```
   The batch is applied as one step. If the result has an error, nothing is
   written and the findings say where. `project_undo` restores the previous
   version.
8. **Render.** `edits render project.json -o out.mp4` (MCP: `render_video`
   with `output`, then `job_status` with the returned job id until
   `completed`). Preview first with `--scale 0.5`; render full size once.

## Workflow 2: the recipe of a reference edit, with different pictures

The goal is not to copy pixels. It is to recover **what the editor did**
(tempo, where the cuts fall, how hard and how fast each flash and punch is,
blur, tone hits, overlay, vignette) and replay it on other pictures.

1. **Probe** (seconds): `edits-analyze probe ref.mp4` (MCP: `reference_probe`).
   A per-frame table of luminance, saturation, sharpness and frame difference.
   Big frame differences are cuts or hits; brightness jumps are flashes. Use
   it to see whether the clip is the kind of edit this engine makes.
2. **Analyse** (about a second per ten frames):
   `edits-analyze analyze ref.mp4 -o analysis/` (MCP: `reference_analyze`, then
   `job_status`). Writes `analysis/reference_analysis.json`, `overlay.png` and
   `proxies/` (one still per picture). Pass `--bpm` if you know the tempo.
3. **Read the analysis** (MCP: `reference_read`, sections `summary`, `grid`,
   `shots`, `shot` + id, `layers`; shell: the printed summary, then the JSON).
   Every estimate carries `confidence` and `identifiability`:
   - `measured`: read off the video. **Trust it**, weighted by confidence.
   - `relative_only`: only the change within the shot is known (a picture's
     scale and position). The starting size is yours to choose.
   - `qualitative`: the right kind of thing, the number is a starting point.
   - `needs_agent` (a shot's `status`) and `needs_source`: **not measured.
     Look at the reference yourself.** Typical cases: a 3D cube turn, a
     picture-in-picture, a whip pan. The shot's `notes` say why.
   To look, extract the frames of that shot from the reference:
   `ffmpeg -v error -y -ss 1.00 -i ref.mp4 -frames:v 1 -vf "scale=-2:360,tile=4x3" shot2.png`
   (twelve consecutive frames in one image; `-ss` is the shot's first frame
   divided by fps), view it, and build the shot by hand (a `cube` layer,
   keyed `rotation`, a second image layer).
4. **Draft**: `edits-analyze draft analysis/reference_analysis.json -o edit/project.json --proxies`
   (MCP: `reference_draft` with `proxies: true`). One image layer per shot on
   the beat grid, with the measured flashes, blur, tone hits and motion as
   keyframes. With `--proxies` the slots point at stills taken from the
   reference, so the draft renders at once and can be compared with it.
5. **Set the slots** to the user's pictures (MCP: `project_edit` with
   `set_slot` ops; shell: edit `slots.*.path`). Draft without `--proxies` for
   empty slots. When the pictures are new, enable the `overlay` and `finish`
   layers that `--proxies` disabled (set `enabled` to `true`), and build the
   `needs_agent` shots.
6. **Check**, then **render a sheet and look at it**. Numbers do not tell you
   that a face is cropped or that the cube shows the wrong side.
7. **Compare in signature space.** Render a preview and compare it with the
   analysis:
   ```
   edits render edit/project.json -o edit/preview.mp4 --scale 0.5
   edits-analyze compare analysis/reference_analysis.json edit/preview.mp4
   ```
   (MCP: `render_video`, `job_status`, then `reference_compare`.) It prints
   scores from 0 to 1 (cuts, flashes, tone hits, exposure, blur, saturation,
   scale, tempo, overall) and the largest differences in plain language, most
   severe first, for example
   `shot 2 (beats 2.07-3.54): exposure peaks at 1.01 stops, reference 2.51 stops`.

   **Comparing pixels is meaningless here**: the pictures are different on
   purpose, so every pixel differs even when the edit is identical. The
   comparison measures each video against its own shots (how much brighter
   than the shot's normal frame, how much blurrier, how much larger), so the
   pictures cancel and what is compared is the recipe. This is the yardstick.
   The same clip re-encoded scores about 0.96 against itself: that is the
   noise floor, so do not chase the last few hundredths.
8. **Fix the largest differences first**, one or two at a time, in the
   project (a missing cut: change `in`/`out`; a weak flash: raise `stops`; a
   slow decay: raise `rate`). Render, compare, look again. Stop when the
   listed differences are small and the sheet looks right next to the
   reference.
9. **Final render** at full size: `edits render edit/project.json -o final.mp4`.

## Workflow 3: a custom effect shader

1. Write a WGSL file next to the project, for example `shaders/stripes.wgsl`.
   It must define `fn effect(uv: vec2<f32>) -> vec4<f32>`; `uv` is (0, 0) top
   left to (1, 1) bottom right; return linear-light RGBA with premultiplied
   alpha.
   ```wgsl
   // Darken horizontal bands. p0 = strength (0..1), p1 = bands per frame height.
   fn effect(uv: vec2<f32>) -> vec4<f32> {
       let c = src(uv);
       let band = 0.5 + 0.5 * cos(uv.y * fx.p1 * 6.2831853);
       return vec4<f32>(c.rgb * (1.0 - fx.p0 * band), c.a);
   }
   ```
   Available: `src(uv)` (the picture so far), `src_mirror(uv)`, `fx.p0` to
   `fx.p7` (your parameters), `fx.time` (seconds), `fx.beats`, `fx.frame`,
   `fx.resolution`, `fx.inv_resolution`, `fx.aspect`.
2. Add it to a layer's effects:
   `{ "kind": "custom", "source": "shaders/stripes.wgsl", "params": { "p0": 0.5, "p1": 40 } }`
   (MCP: `project_edit` with `{ "op": "set", "pointer": "/layers/@shot1/effects/-", "value": { ... } }`).
   `p0` to `p7` can be keyframed like any parameter.
3. Compile it by rendering one frame: `edits frame project.json --at 2 -o f.png --scale 0.5`
   (MCP: `render_frame`). `check` only verifies that the file exists; a
   shader that does not compile fails here with the compiler's message and
   line numbers of your file.
4. Look at the frame. Then render a sheet across a keyed parameter.

Rules: express sizes as fractions of the frame (`uv` units), never pixel
counts, so a small preview matches the full render; derive randomness only
from `uv`, `fx.frame` and your parameters. Examples: `examples/shaders/`.
Details: `docs/EFFECTS.md`, "Custom effects".

## Starting values for this genre

Measured on a real reference edit at 123 BPM and 25 fps (12.2 frames per
beat). Start here and adjust by eye and by `compare`.

| What | Where | Values |
| --- | --- | --- |
| Cuts | layer `in` / `out` | On beats and half beats; one or two beats per picture, a longer hold now and then |
| Exposure flash on a cut | `exposure.stops` on the top adjustment layer | Peak 3 to 4 stops on the cut frame, `decay` with `rate` 0.2 to 0.28 (the excess brightness falls to 0.72 to 0.80 of itself each frame), back to 0 within one beat |
| Scale punch | layer `transform.scale` | 1.2 to 1.35 on the cut, down to 1.0, `decay` with `rate` 0.4 to 0.5, over about one beat |
| Position move | layer `transform.position` | Offset of a few percent of the height to `[0, 0, 0]`, `decay` with `rate` 0.39 (each step is 0.61 of the one before) |
| Blur on a cut | `gaussian_blur.radius` on the shot's layer | 1 to 1.5 (% of height) on the cut frame, 0 within 4 to 6 frames (0.33 to 0.5 beats), ease `out` |
| Black-and-white hit | `saturation.amount` | 0 for about 4 frames (0.33 beats), keyed with `hold`, then back to 1 |
| Overlay texture | an image layer above the shots | `"blend": "screen"`, opacity 0.5 to 1 |
| Vignette | `vignette` on the top adjustment layer | `amount` 0.4 to 0.6, `radius` 0.55, `softness` 0.6 |
| Motion blur | `settings.motion_blur` | `samples` 6 to 8, `shutter` 0.5 |

A flash with a hold before it, so that nothing happens until the hit:

```json
{ "kind": "exposure", "params": { "stops": { "keys": [
  { "t": 0, "v": 0, "ease": "hold" },
  { "t": 4, "v": 3.5, "ease": { "name": "decay", "rate": 0.25 } },
  { "t": 5, "v": 0, "ease": "hold" } ] } } }
```

A black-and-white hit of four frames at beat 6:

```json
{ "kind": "saturation", "params": { "amount": { "keys": [
  { "t": 0, "v": 1, "ease": "hold" }, { "t": 6, "v": 0, "ease": "hold" }, { "t": 6.33, "v": 1 } ] } } }
```

## Pitfalls

- **Hold before a hit.** Before its first key a value sits at that key's
  value, and between two keys it ramps. A flash keyed only as
  `4 -> 3.5, 5 -> 0` is at 3.5 stops from beat 0 to beat 4. Put a key with
  the resting value and `"ease": "hold"` at the start (or at the end of the
  previous hit), as in the snippets above.
- **Layers are bottom to top.** The last layer in the list is in front.
  Overlay and adjustment layers go last.
- **Keep flashes on an adjustment layer** above everything, so they cover
  the pictures, the overlay and the cube alike. An `exposure` on one image
  layer brightens only that layer.
- **Check after every edit.** `edits check` (MCP: `project_check`; `project_edit`
  already refuses a result with errors). A misspelled key is ignored by the
  renderer without an error; the MCP tools warn about unknown keys.
- **Iterate small, render big once.** Sheets and frames at low scale and
  `--samples 1` take a second; the full-size video with motion blur takes
  minutes on a machine without a GPU.
- **A frame is 0.082 beats** at 123 BPM and 25 fps. `--at 4` shows the frame
  that contains beat 4, which starts just before it; a hit keyed at beat 4
  appears in full on the next frame. Look at hits with `--every-frame`.
- **Cover the frame while moving.** A picture scaled to 1.0 that also moves
  or rotates shows the background at its edges. Keep scale at 1.05 or more
  during a move, or end the move at `[0, 0, 0]`.
- **`decay` takes its length from the keys.** The value reaches the next
  key's value exactly at that key; `rate` sets how front-loaded the move is.
- **Never guess effect or parameter names.** `edits effects` (MCP:
  `effects_list`) is the registry; `check` rejects unknown ones and lists the
  valid names.
- **Do not put media in git.** Pictures, songs, renders and analyses stay
  out of the repository (`*.mp4`, `/out`, `/analysis`, `/assets` are ignored).
