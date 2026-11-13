---
name: edits
description: Make, change and render short beat-synced video edits (pictures cut to music with flashes, scale punches, blur, 3D cubes, transitions, overlays) with the `edits` engine, from a song analysis, beat cuts and named presets, and recreate the recipe of a reference edit with different pictures. Use when asked to make an edit from a song and pictures, to copy the timing or style of an existing edit, to change or render a project JSON of format `edits.project/1`, to preview frames of one, or to write a custom WGSL effect for it.
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
| Recipe of a reference edit | `edits analyze / draft / compare / probe / fit` | `reference_analyze`, `reference_read`, `reference_draft`, `reference_compare`, `reference_probe`, `project_fit` |
| Song: tempo, bars, kicks, snares, drop | `edits audio song.mp3 -o song.json --project p.json` | `audio_analyze` |
| Pictures cut to the beat | `edits beats cut p.json --slots a,b,c --every 1 --from 0 --to 16` | `beats_cut` |
| A hit on every beat / kick / bar | `edits beats accent p.json --preset flash --on downbeats` | `beats_accent` |
| One preset (flash, punch, shake, slide, 3D flip, transition...) | `edits presets`, `edits apply p.json punch --layer cut3 --at b4` | `presets_list`, `preset_apply` |
| Check a custom shader | `edits shader check fx.wgsl` | `effect_define` (checks, writes, adds) |
| What the person selected in the window | read `.edits/ui.json` | `ui_context` |

From a checkout the binary is `target/release/edits` after
`cargo build --release -p edits-cli -p edits-analyze -p edits-mcp`
(`edits-analyze` still exists as a separate binary with the same four commands). With MCP every
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
  layer's `effects` list, applied in order. `edits effects` lists them all,
  by category, with each parameter's unit (`%h` = percent of frame height,
  `pos` = `[x, y]` composition units, `deg`, `color` = linear RGB) and range.
  Some have `"options"` (choices that cannot be keyed), e.g.
  `{ "kind": "pixelate", "params": { "size": 4 }, "options": { "shape": "hex" } }`;
  `check` rejects an unknown option or value and lists the allowed ones.
  Looks for this genre: `twirl` / `bulge` / `kaleidoscope` (key `angle`,
  `amount` or `zoom` on a hit), `pixelate` (key `size` to 0 for a reveal),
  `halftone` (`mode` mono/rgb/cmyk; manga print), `chromatic_aberration`
  (radial lens fringe, zero in the middle).
- **Light and layer styles**: `bloom` (soft, wide glow above `threshold`;
  key `intensity` on a hit after an `exposure` flash), `light_rays` (god rays
  streaming out from `center`), `lens_flare` (put it on an adjustment layer,
  key `position`), `outline` (a stroke around a cut-out picture's alpha:
  `width` % of height, option `position` outside/inside/center),
  `drop_shadow` (`offset` in composition units), `tilt_shift` (sharp band,
  blurred top and bottom). **Blend modes** beyond `screen` / `add` /
  `multiply`: `soft_light`, `hard_light`, `overlay`, `color_dodge` (hot
  light overlays), `linear_light`, `vivid_light`, `pin_light`, `hard_mix`,
  `color_burn`, `linear_burn`, `exclusion`, `subtract`, `divide`,
  `lighter_color`, `darker_color`, and `hue` / `saturation` / `color` /
  `luminosity` (recolour with a solid). All work in linear light and keep
  values above 1 bright.
- **More looks** (all one pass, sizes in % of height). Hits: `shockwave`
  (key `progress` 0 to 1 over a beat), `glitch` (`rate` changes per time
  unit, `seed`), `film_burn` (key `amount` 0 to 1 as a transition),
  `ripple`, `wave_warp`, `lens_distortion`, `turbulent_displace` (key
  `evolution`). Manga and anime: `screentone` (dots/bayer/lines, `levels` 2
  is pure black and white), `ink` (lineart: keeps lines and solid blacks,
  drops shading), `toon`, `edge_detect`, `kuwahara` (paint), `emboss`,
  `sharpen`. Texture: `scanlines`, `vhs`, `film_damage` (new every frame),
  `mirror`, `tile`, `polar` (`to_polar` = tiny planet). Colour:
  `gradient_map` (duotone/tritone), `levels`, `vibrance`, `lut` (a `.cube`
  file in `"source"`). Generators drawn over the layer: `fractal_noise`,
  `gradient_ramp` (option `blend`). `displacement_map` takes a slot as its
  map: `"source": "slot:<name>"`. Effects that change on their own every
  frame (`glitch`, `vhs`, `film_damage`, moving waves) are still the same
  for the same frame and `seed`.
- **Transitions** sit on the incoming layer: `"transition": { "kind": "whip_pan", "duration": 0.5 }`
  mixes the picture beneath into this one over `duration` from its `in`
  (`edits transitions` lists 22). The outgoing layer must last until the end.
- **Masks and track mattes** limit where a layer shows. `"masks": [ { "shape": "ellipse", "size": 0.6, "feather": 4 } ]`
  (shapes `rect`, `ellipse`, `rounded_rect` with `radius`, `polygon` with
  3 to 32 `points`; `position`, `size`, `rotation`, `feather` and `expansion`
  (% of height) keyable; `mode` add/subtract/intersect/difference;
  `inverted`) are in the layer's own space and move with it: key `size`
  from 0 for an iris reveal (preset `iris_in`), use `polygon` masks for
  slanted split-screen panels (preset `split_panels`, side left and right). On an
  adjustment layer they limit where its effects apply. `"matte": { "layer": "title", "mode": "alpha" }`
  cuts the layer to another layer's picture at the same instant (`alpha`,
  `alpha_inverted`, `luma`, `luma_inverted`): a picture inside the letters
  of a text layer. Give the source `"visible": false` so it works only as
  the matte.
- **Presets** are named building blocks (`flash`, `punch`, `shake`,
  `rgb_split`, `slide_in`, `flip_3d`, `cube_turn`, `velocity`, `letterbox`,
  `transition:<kind>`, ...) with typed variables. Applying one writes ordinary
  keys, effects and layers: you can read and change the result like anything
  you wrote, and undo it in one step.
- **Times can be text** wherever a time is taken: `"b12.5"`, `"bar 4"`,
  `"4:3"` (bar 4, beat 3), `"2.5s"`, `"f60"`, markers such as `"drop"`, and
  offsets: `"drop+2b"`, `"bar 3-0.5b"`.

Full references: `docs/FORMAT.md`, `docs/EFFECTS.md`, `docs/CLI.md`,
`docs/ANALYSIS.md`, `docs/AGENTS.md`. A complete example: `examples/beat-loop.json`
(`edits demo demo` writes it with pictures and audio).

## Workflow 1: an edit from a song and pictures, with presets and beat operations

The fast route: build the skeleton and the hits with a handful of commands,
look, then refine individual values. Every command below is one validated,
undoable step (`project_undo`, or Ctrl+Z in the window).

1. **Check the machine.** `edits doctor` (MCP: `doctor`); it must end with
   `ready to render`.
2. **Start a project** next to the pictures: a minimal file with the size,
   fps, a beats time base and one slot per picture (copy the `settings` and
   `slots` of the skeleton in Workflow 1b; `"layers": []` is fine).
3. **Analyse the song and sync the project to it.**
   `edits audio song.mp3 -o song.json --project project.json` (MCP:
   `audio_analyze` with `project`). It prints the tempo, bars, the number of
   kicks, snares and hats, the sections and the drop, writes them all to
   `song.json`, and writes the tempo, `beat_offset`, the song as `audio` and
   the markers (`drop`, sections) into the project. Read its notes: a low
   tempo confidence means pass `--bpm`; a low bar confidence means bar 1 may
   be on another beat (listen, or count bars from the drop).
4. **Look at the pictures** (view each file): order, which ones deserve a
   long hold, and where each subject is (`set_slot` with `focus`).
5. **Cut to the beat.** One picture per beat up to the drop, two per beat after it:
   ```
   edits beats cut project.json --slots p1,p2,p3,p4 --every 1 --from 0 --to drop
   edits beats cut project.json --slots p5,p6,p7,p8 --every 0.5 --from drop --to drop+16b
   ```
   (MCP: `beats_cut`.) Or cut on the song itself: `--on kicks --audio song.json`.
   Layers are `cut1`, `cut2`, ...; they go below the adjustment layers at the
   top, or `--above` / `--below` a layer you name.
6. **Accent.** `edits presets` lists the building blocks (MCP: `presets_list`;
   with a name, its variables). Typical:
   ```
   edits beats accent project.json --preset flash_burn --on downbeats --var stops=3
   edits beats accent project.json --preset punch --on kicks --audio song.json --var amount=1.12
   edits beats accent project.json --preset shake --on drop --audio song.json --layer fx
   edits apply project.json transition:whip_pan --layer cut9          # at the cut into cut9
   edits apply project.json rgb_split --layer cut12 --at "bar 6+2b"
   ```
   (MCP: `beats_accent`, `preset_apply`.) `--layer auto` (default) changes the
   picture on screen at each time; `--layer fx` puts the hit on one adjustment
   layer on top, so it covers everything (flashes look best there). A second
   hit of the same kind goes into the same effect's keys.
7. **Look.** `edits sheet project.json -o sheet.png` and view it (MCP:
   `render_sheet`), then every frame around the drop and each transition:
   `edits sheet project.json --from drop-1b --to drop+1b --every-frame --cols 5 -o drop.png`.
   Check that cuts, flashes and punches start on the same frame, that flashes
   fade within about a beat, that no picture shows its edges, and that
   transitions read as one move.
8. **Refine.** Change a value with `project_edit` (or `edits apply ... --var`
   again on another layer); undo a whole preset with `project_undo`. When the
   person is watching in the window, `ui_context` tells you which layer they
   selected and what they asked for in their note.
9. **Render.** `edits render project.json -o out.mp4 --scale 0.5` for a
   preview, then full size (MCP: `render_video`, then `job_status`).

## Workflow 1b: writing the JSON by hand

When you need full control (or to understand what the presets write):

1. **Get the tempo**: `edits audio song.mp3` prints it and "beat 0 at"
   (seconds into the song). Use them as `settings.bpm` and
   `settings.beat_offset`. To start the edit N seconds into the song, set
   `audio.offset` to N and `beat_offset` to the time of the first beat at or
   after N, minus N.
2. **Look at the pictures** and decide the order.
3. **Write the project** as below.
   Start from this skeleton: one
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
4. **Check.** `edits check project.json` (MCP: `project_check`). Fix every
   error; read the warnings.
5. **Look.** `edits sheet project.json -o sheet.png` and view the image (MCP:
   `render_sheet`). Sixteen frames of the whole edit, each labelled with frame
   number, beat and seconds. Then look at each hit frame by frame:
   `edits sheet project.json --from 3.9 --to 4.5 --every-frame --cols 4 -o hit.png`
   (MCP: `render_sheet` with `from`, `to`, `every_frame: true`). Check that the
   cut, the flash and the punch start on the same frame, that the flash is
   gone within about a beat, and that no picture shows its edges.
6. **Change and repeat.** Shell: edit the JSON, then `edits check`. MCP:
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
7. **Render.** `edits render project.json -o out.mp4` (MCP: `render_video`
   with `output`, then `job_status` with the returned job id until
   `completed`). Preview first with `--scale 0.5`; render full size once.

**Video clips.** A `video` layer plays a clip from a slot (any file ffmpeg
reads; the slot's `"trim": [in_s, out_s]` picks the seconds that play):
`{ "id": "op", "kind": "video", "slot": "clip", "in": 8, "out": 12 }`. Its
time starts at `in`. `speed` (1 = real time; keyed, e.g. 1 → 0.2 over a beat
for a velocity ramp) or `time_remap` (source seconds at each key) set what
plays; `loop`, `reverse`, and `"frame_blend": "mix"` (smooth slow motion) do
what they say; `"volume": 0` mutes the clip's sound, which is otherwise mixed
into the render when the speed is constant. `"fps": 12` on any layer holds its
picture and motion for two frames at 24 fps, the choppy anime look. `edits
check` flags a missing clip, a negative speed and a bad trim; see
`docs/FORMAT.md` (Video layers).

**Particles.** Sakura petals, sparkles, dust and sparks on a hit are a
`particles` layer; the quickest way in is a preset with target `project`:
`edits apply p.json sakura --at 0 --var length=32`, `sparkles`, `dust`, or
`beat_burst --at drop` (sparks flying out on that beat; `--var position=0.3,-0.1`
moves it). By hand: `{ "id": "hit", "kind": "particles", "sprite": "spark",
"bursts": [{ "t": "b16", "count": 60 }], "life": 2, "speed": 1.2, "drag": 2.5,
"gravity": 0.4, "blend": "add", "in": 16, "out": 18 }`; `rate` (per beat) emits
continuously, `"prewarm": true` starts full, `sprite` is `dot`, `spark`,
`star`, `petal` or `slot:<name>`, and `seed` gives other particles. Sizes are %
of height, speeds heights per second. Every frame is computed from the time
alone, so frames and sheets anywhere are exact; see `docs/FORMAT.md`
(Particle layers).

**Sound effects.** Whooshes, impacts and risers on cuts go in the top-level
`audio_clips` list, mixed over the song at render without lowering it:
`{ "id": "whoosh1", "path": "sfx/whoosh.wav", "at": "drop-0.25s", "trim_in": 0, "trim_out": 0.6, "volume": 0.7, "fade_in": 0, "fade_out": 0.1 }`.
Set it with `project_edit` (`{"op": "set", "pointer": "/audio_clips", "value": [...]}`,
or `/audio_clips/-` to append to an existing list); `at` takes text times like a layer's `in`
and is stored as a number. `volume` is a linear gain (0.5 is about -6 dB);
`trim_*` and `fade_*` are seconds of the sound file. `edits check` flags a
missing file and fades longer than the clip, `describe` lists the clips, and
`edits render` reports how many were heard as `audio_clips`. There is no
preset for them: write the clip directly.

**Text.** A `text` layer sets titles and captions:
`{ "id": "title", "kind": "text", "text": "SAY MY\nNAME", "size": 14, "weight": 800, "stroke": { "color": [0, 0, 0], "width": 0.8 } }`.
`size` and stroke `width` are percent of the frame's height, `fill` and
stroke `color` are sRGB `[r, g, b]` (white and black by default), `weight`
works on variable fonts (the built-in Inter is one: 100 to 900). The block is
centred on the layer's `position`, capitals exactly; `align` (`left`,
`center`, `right`) lines the lines up, and `max_width` (composition units,
e.g. 1.2) wraps a long caption. Without `font` it is Inter; `"font":
"fonts/Anton.ttf"` (relative to the project) uses another file, and `edits
check` reports it when missing. Animate it like any layer (scale punch,
position, opacity, effects); look at the frame with `edits frame` to check
it fits. See `docs/FORMAT.md` (Text layers).

## Workflow 2: the recipe of a reference edit, with different pictures

The goal is not to copy pixels. It is to recover **what the editor did**
(tempo, where the cuts fall, how hard and how fast each flash and punch is,
blur, tone hits, overlay, vignette) and replay it on other pictures.

1. **Probe** (seconds): `edits probe ref.mp4` (MCP: `reference_probe`).
   A per-frame table of luminance, saturation, sharpness and frame difference.
   Big frame differences are cuts or hits; brightness jumps are flashes. Use
   it to see whether the clip is the kind of edit this engine makes.
2. **Analyse** (about a second per ten frames):
   `edits analyze ref.mp4 -o analysis/` (MCP: `reference_analyze`, then
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
4. **Draft**: `edits draft analysis/reference_analysis.json -o edit/project.json --proxies`
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
   edits compare analysis/reference_analysis.json edit/preview.mp4
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
8. **Let `fit` tune the numbers.** `edits fit edit/project.json -r analysis/reference_analysis.json`
   (MCP: `project_fit`, a background job; poll `job_status`). It renders each
   shot and adjusts flash strength and decay, saturation, threshold level,
   blur and zoom or move amounts until the signatures match, with whatever
   pictures are in the slots. It never moves key times or changes pictures,
   rotation or inversions, writes only if the whole-clip score does not get
   worse, and records one history entry (`project_undo` takes it back). It
   takes minutes on a CPU; `--dry-run` reports without writing, `--shots 3,7`
   limits it. On the reference it was built on it raised the overall score
   from 0.69 to 0.76.
9. **Fix what fit cannot**, largest difference first, one or two at a time:
   a missing or extra cut (`in`/`out`), shots the analysis marked
   `needs_agent` (cubes, 3D turns, insets: look at the reference frames and
   build them yourself), rotations, colour grades. Render, compare, look
   again. Stop when the listed differences are small and the sheet looks
   right next to the reference.
10. **Build the moves the analysis cannot measure with presets** instead of
    keyframes: a cube turn (`cube_turn` on a `cube` layer, or
    `transition:cube` between two pictures), a whip pan
    (`transition:whip_pan`), a 3D flip (`flip_3d`), a speed ramp (`velocity`),
    a slide (`slide_in`, `slide_out`). Apply each at the shot's start
    (`--at b<beat>` from `reference_read` section `shots`), look at the
    reference frames and the sheet side by side, adjust the variables, then
    `compare` again. The reference's own soundtrack gives its kicks and
    snares: `edits audio ref.mp4 -o ref_song.json`; `edits beats accent ...
    --on snares --audio ref_song.json` puts a hit where the editor did when the
    analysis missed it.
11. **Final render** at full size: `edits render edit/project.json -o final.mp4`.

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
   `fx.p15` (your parameters), `fx.time` (seconds), `fx.beats`, `fx.frame`,
   `fx.resolution`, `fx.inv_resolution`, `fx.aspect`, and helpers
   `fx_to_comp(uv)` / `fx_to_uv(p)` (composition units), `fx_hash`, `fx_noise`.
2. Add it to a layer's effects:
   `{ "kind": "custom", "source": "shaders/stripes.wgsl", "params": { "p0": 0.5, "p1": 40 } }`
   (MCP: `project_edit` with `{ "op": "set", "pointer": "/layers/@shot1/effects/-", "value": { ... } }`).
   `p0` to `p15` can be keyframed like any parameter.
3. Check it: `edits shader check shaders/stripes.wgsl` compiles it the way
   the renderer will, without a GPU, and points at the line and column of
   each problem (exit 1). With MCP, `effect_define {project, name, wgsl,
   params, layer?}` does steps 1 to 3 at once: it checks the source (nothing
   is written when it fails), saves `shaders/<name>.wgsl` next to the
   project and adds the effect to the layer, or to a new adjustment layer on
   top; `project_undo` takes the effect back out.
   Then render one frame: `edits frame project.json --at 2 -o f.png --scale 0.5`
   (MCP: `render_frame`).
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
- **Presets read what is there.** `punch` multiplies the scale the layer has
  at that moment (`base`), so a punch on top of a zoom is relative to the
  zoom; `flash` keyed twice shares one `fade` effect. Read `edits describe`
  after a few presets; it shows the keys they wrote.
- **A transition needs the outgoing picture underneath.** Apply
  `transition:<kind>` to the *incoming* layer; it moves that layer's `in`
  earlier and the previous picture's `out` later so they overlap. Applied to
  a layer with nothing below it, it is refused.
- **Never guess effect or parameter names.** `edits effects` (MCP:
  `effects_list`) is the registry; `check` rejects unknown ones and lists the
  valid names.
- **Do not put media in git.** Pictures, songs, renders and analyses stay
  out of the repository (`*.mp4`, `/out`, `/analysis`, `/assets` are ignored).
