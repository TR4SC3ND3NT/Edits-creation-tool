# Project format (`edits.project/1`)

A project is one JSON file. It is the only thing the renderer reads, and it is
meant to be edited by hand or by an agent. Paths are relative to the project file.
The Rust types in `crates/edits-core/src/model.rs` are the authority; this page
is the short version.

## Conventions

- **Time.** `in`, `out`, key `t` and `duration` are in the project's `time_base`:
  `beats` (needs `settings.bpm`; beat 0 is at `settings.beat_offset` seconds) or `seconds`.
  The file always holds numbers, but an edit (and every command that takes a
  time) may write them as text; see [Times as text](#times-as-text).
- **Space.** Units of composition height. Origin at the centre of the frame,
  x right, y down, z away from the viewer. A layer at z = 0 with scale 1 fills the frame.
- **Rotation.** Degrees, applied X, then Y, then Z.
- **Effect sizes** (blur radius, split amount) are percent of composition height.
- **Layers** are listed bottom to top.
- **Slots** are named media placeholders. The recipe refers to slots; to put
  different pictures into the same edit, change only the slots' `path`.

## Shape

```json
{
  "format": "edits.project/1",
  "settings": { "width": 1080, "height": 1080, "fps": 25, "duration": 16,
                "time_base": "beats", "bpm": 123, "beat_offset": 0.0,
                "background": [0, 0, 0],
                "motion_blur": { "samples": 8, "shutter": 0.5 } },
  "audio": { "path": "ref.mp4", "offset": 0.48 },
  "audio_clips": [ { "id": "whoosh1", "path": "sfx/whoosh.wav", "at": 15.5 } ],
  "slots": { "a": { "path": "img/a.png", "fit": "cover", "focus": [0.5, 0.4] } },
  "camera": { "fov": 39.6, "position": [0, 0, 0], "rotation": [0, 0, 0] },
  "layers": [ ... ],
  "comps": { "card": { "duration": 4, "layers": [ ... ] } }
}
```

## Layers

Every layer has `id`, `kind`, optional `in` / `out`, `transform`, `blend`, `effects`, `enabled`,
and optionally `transition` (how it comes in; see [Transitions](#transitions)), `fps`
(posterize time; see [Video layers and layer time](#video-layers-and-layer-time)), `masks`,
`matte` and `visible` (see [Masks and track mattes](#masks-and-track-mattes)).

| `kind` | Extra fields | What it is |
| --- | --- | --- |
| `image` | `slot` | A flat picture from a slot |
| `solid` | `color` `[r, g, b, a]` | A flat colour |
| `cube` | `faces` (slot ids: front, right, back, left, top, bottom), `size` `[w, h, d]` | A real 3D box with a picture on each face |
| `adjustment` | none | Its effects apply to everything beneath it |
| `video` | `slot`, optional `speed`, `time_remap`, `loop`, `reverse`, `frame_blend`, `volume` | A clip from a slot, decoded by ffmpeg; see [Video layers](#video-layers-and-layer-time) |
| `text` | `text`, optional `font`, `size`, `weight`, `align`, `line_height`, `tracking`, `fill`, `stroke`, `max_width`, `animators` | Lines of text in a font file (Inter when none is named), optionally animated letter by letter; see [Text layers](#text-layers) and [Text animators](#text-animators) |
| `particles` | `emitter`, `rate`, `bursts`, `life`, `speed`, `direction`, `spread`, `gravity`, `sprite`, ... | Petals, sparkles, dust, a burst on a beat; see [Particle layers](#particle-layers) |
| `comp` | `comp`, optional `offset`, `speed`, `time_remap`, `loop`, `reverse` | A group of layers from `comps`, drawn as one picture; see [Comps](#comps-precomps) |
| `shape` | `shapes` | Vector shapes (rects, ellipses, stars, paths, lines) with fills, strokes and operators (trim, repeater, ...): speed lines, focus lines, rings, panel borders; see [Shape layers](#shape-layers) |

`transform` has `position` `[x, y, z]`, `scale` (number, `[x, y]` or `[x, y, z]`),
`rotation` `[x, y, z]`, `anchor` `[x, y]`, `opacity`.

`blend`: `normal`, `add`, `screen`, `multiply`, `overlay`, `lighten`, `darken`, `difference`,
`color_burn`, `linear_burn`, `darker_color`, `color_dodge`, `linear_dodge` (the same as `add`),
`lighter_color`, `soft_light`, `hard_light`, `vivid_light`, `linear_light`, `pin_light`,
`hard_mix`, `exclusion`, `subtract`, `divide`, `hue`, `saturation`, `color`, `luminosity`.
They work in linear light and stay sensible above 1; `docs/RENDERING.md` has the formulas.

## Masks and track mattes

A character revealed through a shape, a picture inside the letters of a
title, a frame split into panels: these limit where a layer shows. Every
layer (an adjustment layer too) can have **masks**, shapes in its own space,
and a **track matte**, another layer whose picture it is cut to.

```json
{ "id": "logo", "kind": "text", "text": "NAME", "size": 30, "visible": false },
{ "id": "hero", "kind": "image", "slot": "hero",
  "masks": [
    { "shape": "ellipse", "size": { "keys": [ { "t": 0, "v": 0, "ease": "quint_out" }, { "t": 1, "v": 2.2 } ] }, "feather": 4 },
    { "shape": "rect", "size": [0.3, 0.05], "position": [0, 0.3], "mode": "subtract" } ] },
{ "id": "inside", "kind": "image", "slot": "panel", "matte": { "layer": "logo", "mode": "alpha" } }
```

**`masks`** is a list; each mask has:

| Field | Default | Meaning |
| --- | --- | --- |
| `shape` | (required) | `rect`, `ellipse`, `rounded_rect` or `polygon`. |
| `position` | `[0, 0]` | Centre of the shape in layer space: composition units from the layer's origin, y down. Keyable. |
| `size` | `[1, 1]` | Width and height in composition units (one number: a square or a circle). For a polygon, a scale of its points. Keyable. |
| `rotation` | 0 | Degrees, clockwise, about `position`. Keyable. |
| `radius` | 0 | `rounded_rect` only: the corner radius in composition units. Keyable. |
| `points` | none | `polygon` only: 3 to 32 corners `[[x, y], ...]` in layer space (scaled by `size`, turned by `rotation`, moved by `position`). Self-crossing outlines fill by the even-odd rule. |
| `feather` | 0 | Width of the soft edge in percent of the composition height, centred on the outline (a linear ramp; without feather the edge is anti-aliased over a pixel). Not negative. Keyable. |
| `expansion` | 0 | Grows (positive) or shrinks (negative) the shape, in percent of the height. Keyable. |
| `opacity` | 1 | 0..1: how strongly the mask counts. Keyable. |
| `mode` | `add` | How the mask combines with the masks before it: `add` (union), `subtract` (cut out), `intersect` (keep the overlap), `difference`. |
| `inverted` | false | Use the outside of the shape (`invert` is accepted too). |

Masks are combined in order and the result multiplies the layer's alpha.
With `a` the coverage so far and `m` the mask's (inverted first, then times
its opacity): `add` gives `a + m - a m`, `subtract` `a (1 - m)`, `intersect`
`a m`, `difference` `|a - m|`. Before the first mask the coverage is empty,
except when that mask subtracts or intersects: then it starts full (so a
lone `subtract` mask cuts a hole, and a first `intersect` acts as `add`).
These are After Effects' rules. Layer space is the layer's own: a mask moves,
scales and turns with the layer (and tilts with it in 3D). For an image,
video or solid layer at scale 1 the layer box is the frame, so `size [1, 1]`
at `[0, 0]` is a square as high as the frame, centred. Masks are applied to
what the layer draws, before its effects, so a blur on a masked layer
softens the mask's edge too. On an adjustment layer, masks limit where its
effects show.

**`matte`** is `{ "layer": "<id>", "mode": "alpha" }`: the layer `<id>` is
drawn at the same instant (with its own transform, masks, effects, opacity
and matte) and multiplies this layer's alpha after this layer's effects:

| `mode` | Shows the layer where the matte source is |
| --- | --- |
| `alpha` (default) | opaque |
| `alpha_inverted` | transparent |
| `luma` | bright: the Rec. 709 luminance of its display (sRGB) colour, times its alpha |
| `luma_inverted` | dark or transparent |

A matte source that is not there at that instant (before its `in`, after its
`out`, opacity 0) counts as transparent. The source can be anywhere in the
list and can have a matte of its own, but not one that leads back to this
layer (`check` reports cycles, unknown ids, adjustment layers as sources).

**`visible`** (default `true`): `false` means the layer is not drawn, but
still works as a matte source. A matte source usually has it, otherwise it
also shows on its own (`check` warns). `enabled: false` turns a layer off
everywhere, as a matte too.

## Animatable values

Any number or vector in `transform`, `camera` and effect `params` can be a constant
or keyframed:

```json
"scale": { "keys": [ { "t": 0, "v": 1.2, "ease": { "name": "decay", "rate": 0.4 } },
                     { "t": 1, "v": 1.0 } ] }
```

`ease` describes the segment from that key to the next:

- a name: `linear`, `hold`, `in`, `out`, `in_out`, `quad_out`, `quint_out`,
  `expo_in`, `expo_out`, `sine_in_out`, `back_out`, `elastic_out`, `bounce_out`
- `[x1, y1, x2, y2]`: a CSS-style cubic bezier
- `{ "name": "decay", "rate": r }`: each frame covers fraction `r` of the remaining
  distance (a measured step ratio of 0.61 per frame is `rate` 0.39)
- `{ "name": "back_out", "overshoot": 1.7 }`, `{ "name": "elastic_out", "period": 0.3 }`

Modifiers add procedural motion on top of keys:

```json
"position": { "base": [0, 0, 0],
              "modifiers": [ { "kind": "shake", "amplitude": 0.01, "frequency": 14, "seed": 1 } ] }
```

`shake.amplitude` can itself be keyframed, which gives an envelope. `loop` repeats a
range: `{ "kind": "loop", "from": 0, "period": 4 }`.

## Effects

`{ "kind": "...", "params": { ... }, "options": { ... }, "source": "...", "enabled": true }`,
on a layer or on an adjustment layer. Only `kind` is required.

- `params`: numbers or vectors, each animatable like a transform value. Each
  has a unit (`%h` percent of height, `pos` composition units, `deg`,
  `color` linear RGB, ...) and may have a range the renderer clamps to.
  A single number given for a vector parameter repeats (`"gamma": 1.2` is
  `[1.2, 1.2, 1.2]`); a list keeps its length, which is how `curves` takes
  any number of control points and `gradient_map` / `colorama` any number of
  colours: `"master": [0, 0, 0.25, 0.18, 0.75, 0.82, 1, 1]`.
- `options`: strings naming a choice that cannot be animated, for example
  `{ "shape": "hex" }` on `pixelate` or `{ "mode": "cmyk", "shape": "line" }`
  on `halftone`. A missing option takes the first of its values.
- `source`: what the effect reads besides the picture: a WGSL file for
  `custom`, a `.cube` file for `lut` (paths relative to the project file), or
  `"slot:<name>"`, a slot of the project, for `displacement_map` and
  `difference_matte` (required) and `time_displacement` (optional: without
  it the layer's own picture is the map). Effects that take none must not have one.

The registry in `crates/edits-core/src/effects/` (one file per category)
lists every kind with its parameters, units, ranges, options and defaults;
`edits effects` prints it and `edits check` validates against it.

colour: `exposure`, `fade`, `brightness_contrast`, `saturation`, `hue`, `invert`,
`threshold`, `posterize`, `tint`, `vignette`, `grain`, `lut` (a `.cube` file
in `source`), `gradient_map` (with a `stops` mode), `levels`, `vibrance`,
`white_balance`, `lift_gamma_gain`, `highlights_shadows`, `channel_mixer`,
`curves` (control points as lists), `hsl`, `colorama`, `replace_color`,
`leave_color`, `solarize`, `bleach_bypass`, `technicolor`, `haze`; key (they
make the layer transparent, after its masks and before its track matte):
`chroma_key`, `spill_suppress`, `luma_key`, `unmult`, `difference_matte` (a
clean plate slot in `source`); blur: `gaussian_blur`,
`directional_blur`, `zoom_blur`, `spin_blur`; light: `glow`; distort:
`transform2d`, `twirl`, `bulge`, `kaleidoscope`, `wave_warp`, `ripple`,
`shockwave`, `mirror`, `tile`, `polar`, `lens_distortion`, `displacement_map`
(a map slot in `source`: `"slot:<name>"`), `turbulent_displace`; stylise:
`rgb_split`, `chromatic_aberration`, `pixelate`, `halftone`, `screentone`,
`edge_detect`, `ink`, `toon`, `kuwahara`, `emboss`, `sharpen`, `glitch`,
`scanlines`, `vhs`, `film_damage`, `film_burn`; time: `echo`,
`time_displacement` (both render the layer, or on an adjustment layer the
layers beneath it, again at other times: see [EFFECTS.md](EFFECTS.md#time));
generate: `fractal_noise`, `gradient_ramp`; and `custom` (a WGSL file in
`source`). Posterize time is a layer's `fps` field, not an effect.

```json
{ "kind": "pixelate", "params": { "size": { "keys": [ { "t": 8, "v": 6, "ease": "out" }, { "t": 9, "v": 0 } ] } },
  "options": { "shape": "hex" } }
```

## Transitions

A layer can come in through a transition instead of a cut:

```json
{ "id": "cut5", "kind": "image", "slot": "a", "in": 3.75, "out": 5,
  "transition": { "kind": "whip_pan", "duration": 0.5, "params": { "blur": 0.4 }, "ease": "linear" } }
```

From `in` to `in + duration` the frame is the transition `kind` applied to
two pictures: "from", the frame as it is beneath this layer, and "to", the
frame with this layer composited normally. The progress goes from 0 to 1,
through `ease` (any ease name, default `linear`); after the end, the layer is
simply there, so the last frame of a transition is the plain cut. The layer
it comes from must stay visible until the transition ends (`check` warns
when nothing beneath lasts that long). `edits transitions` lists the 22
kinds and their parameters; [EFFECTS.md](EFFECTS.md#transitions) describes
them. The preset `transition:<kind>` writes this field and moves the two
layers so that they overlap around the cut.

## Times as text

Wherever an edit or a command takes a time, it also accepts text, resolved
against the project (and a song analysis, when the command is given one with
`--audio`):

| Text | Means |
| --- | --- |
| `12.5` | project time (beats in a beats project, else seconds) |
| `b12.5`, `12.5b` | beat 12.5; beat 0 is at `settings.beat_offset` |
| `bar 4`, `bar4`, `4:1` | the first beat of bar 4 (bars count from 1) |
| `4:3`, `bar 4:3` | bar 4, beat 3 (beats in a bar count from 1) |
| `2.5s` | 2.5 seconds from the start of the project |
| `f60`, `60f` | the start of frame 60 |
| `drop`, `intro`, any marker label | a marker of the project, or of the song analysis |
| `start`, `end` | 0, and the end of the project |
| `drop+2b`, `bar 3-0.5b`, `drop-1bar`, `b4+3f` | offsets in `b`, `bar`, `s` or `f` (no unit: project time) |

Bars have 4 beats and bar 1 starts at beat 0, unless a song analysis says
where its downbeats are. In `project_edit` and the other edit commands, a
layer's `in` / `out`, a key's `t`, a marker's `t`, an audio clip's `at`, a
particle burst's `t` and a transition's `duration` (`"2b"`, `"0.5s"`) may be written this way; they are stored as
numbers. Markers come from `"markers": [{ "t": 16, "label": "drop" }]`;
`edits audio <song> --project <project>` writes the song's drop and
sections there.

## Sound effects (`audio_clips`)

`audio_clips` lists sounds mixed over the song by `edits render`: whooshes
into a cut, impacts on a hit, a riser before the drop. Each clip plays once,
from project time `at`, at the level the file has times `volume`. The song
keeps its level (the mix adds, it does not average), so keep effects below
the song or lower their `volume`.

```json
"audio_clips": [
  { "id": "whoosh1", "path": "sfx/whoosh.wav", "at": 15.5, "trim_in": 0.1, "fade_out": 0.2 },
  { "id": "riser", "path": "sfx/riser.wav", "at": 12, "trim_out": 4, "volume": 0.5, "fade_in": 1 }
]
```

| Field | Default | Meaning |
| --- | --- | --- |
| `id` | (required) | A unique name, used by edits: `/audio_clips/@whoosh1/volume`. |
| `path` | (required) | Any file with sound that ffmpeg reads (WAV, MP3, FLAC, OGG, M4A, a video), relative to the project. |
| `at` | (required) | When the clip starts, in project time. Edits may write it as text (`"drop-1b"`, `"bar 5"`, `"12.5s"`); see [Times as text](#times-as-text). |
| `trim_in` | 0 | Seconds into the file where the clip starts. |
| `trim_out` | end of the file | Seconds into the file where it stops. |
| `volume` | 1 | **Linear** gain, like a video layer's `volume`: 1 keeps the file's level, 0.5 is about -6 dB, 2 about +6 dB, 0 mutes. |
| `fade_in`, `fade_out` | 0 | Seconds of fade from and to silence at the two ends of the trimmed clip. |
| `enabled` | true | `false` keeps the clip in the file but leaves it out of the render. |

`trim_*` and `fade_*` are seconds of the sound, not project time, so a clip
sounds the same at any tempo. A clip that runs past the end of the project is
cut there; one that starts before 0 (or before a `--from`) plays from where
the render starts. `edits check` reports a missing file, a duplicate id,
`trim_out` before `trim_in`, negative values, and fades longer than a trimmed
clip; without a `trim_out` the render shortens fades that do not fit the
file. To start a sound a little before a cut, subtract from the cut's time
(`"drop-0.25s"`). The preview window and `edits frame` do not play them.

## Presets (`edits.preset/1`)

A preset is a named building block (a flash, a punch-in, a slide, a
transition) that expands into ordinary layers, keyframes and effects, so the
result is a plain project: editable, diffable and undoable. The built-in
presets are the files in [`presets/`](../presets) (compiled into the
program); a project can add its own as `<project dir>/presets/<name>.json`
(`edits save-preset` writes one from a layer), and `transition:<kind>` exists
for every transition. `edits presets` lists them; `edits apply` and
`edits beats accent` (or the MCP tools `preset_apply`, `beats_accent`) apply
them.

```json
{
  "format": "edits.preset/1",
  "name": "punch",
  "description": "A zoom punch on the hit: the picture jumps in and settles back.",
  "tags": ["hit", "zoom", "camera", "accent"],
  "variables": {
    "amount": { "type": "number", "default": 1.15, "min": 1, "max": 3, "doc": "Scale at the peak." },
    "decay":  { "type": "number", "default": 0.5, "min": 0.05, "max": 8, "unit": "beats", "doc": "Time to fall back." }
  },
  "apply": [
    { "pulse": { "transform": "scale",
                 "adjustment": { "effect": "transform2d", "param": "scale", "base": 1 },
                 "keys": [ { "dt": 0, "v": "=base * $amount", "ease": { "name": "decay", "rate": 0.35 } },
                           { "dt": "=$decay * beat", "v": "=orig" } ] } }
  ]
}
```

- **`target`**: `layer` (default) changes the layer it is applied to; `project`
  adds layers of its own (`letterbox`, `overlay_texture`, and the particle
  layers `sakura`, `sparkles`, `dust` and `beat_burst`) and needs none.
- **`variables`**: `type` is `number`, `integer`, `boolean`, `color` (`[r, g, b]`
  in 0..1, `"r,g,b"` or `"#rrggbb"`), `vector`, `enum` or `string`, with a
  `default`, optional `min` / `max` / `unit`, and a `doc`. An `enum`'s
  `options` map each choice to numbers it sets: `"direction": { "type": "enum",
  "options": { "from_left": { "dx": -1, "dy": 0 }, ... } }` makes `$dx` and `$dy`
  available. A given value outside the range, or an unknown variable, is an
  error that lists what is allowed.
- **Templates.** A string starting with `=` is an expression: numbers, `+ - * /`,
  parentheses, `min`, `max`, `abs`, `round`, `vec(a, b, ...)`; vectors combine
  element-wise with numbers and with vectors of the same length. A string that
  is exactly `$name` is the variable's value, of any type. Names in expressions:
  `$variable`, `at` (the time it is applied at), `beat` and `frame` (their
  lengths in project time), `aspect` (width / height), `end` (the project's
  length), `in` and `out` of the target layer, `prev_in` and `prev_out` of the
  picture it cuts from, and, inside a pulse, `base` (the value at `at` before
  the preset) and `orig` (the value before the preset at that key's time), with
  components `base0`, `orig1`, ...
- **Actions** (`apply`, in order):

| Action | What it does |
| --- | --- |
| `pulse` | Keys on one property, from `at`: `{ "transform": "scale" }`, `{ "effect": "fade", "param": "amount", "params": {...}, "base": 0 }`. `keys` are `{ "dt", "v", "ease" }`. Keys already there before `at` are kept and held until the pulse, keys after it are kept; a missing effect is added (with `params` and, if given, `options`, e.g. `"options": { "mode": "$mode" }`). `adjustment` names an effect parameter to use instead when the target is an adjustment layer. |
| `shake` | A `shake` modifier on a property (`transform` or `effect` + `param`, as in a pulse) whose amplitude follows `keys`, at `frequency`. Repeated hits share one modifier. |
| `effect` | Append an effect to the target. |
| `layer` | Insert a layer (`"place": "top"`, `"above"` or `"below"` the target). Its `id` may contain `{target}`; it gets a suffix when taken. |
| `set` | Set a value inside the target (`"pointer": "/transition"`), or inside the picture it cuts from (`"on": "previous"`). |
| `retime` | Replay the target's keyed motion between `at` and `at + length` through a speed `curve` (an ease); `otherwise` lists actions for a layer that does not move. |
| `animator` | Append a [text animator](#text-animators) to the target, which must be a `text` layer. |

A preset with `"layer_kind": "text"` changes a text layer: with the target
`auto` it takes the topmost text layer visible at the time, and any other
kind of layer is an error. The built-in `typewriter`, `pop_in`,
`random_flicker` and `wave` are such presets: `edits apply p.json typewriter
--at 8 --var duration=2` types out the caption on screen at beat 8.

## Rendering model

For each output frame the renderer evaluates the whole project at one or more
instants (motion blur samples), draws each active layer, applies its effects,
blends it over the layers beneath, and averages the samples in linear light.
A layer with a running transition is drawn into "to" first and then mixed with
"from" by its transition. A frame is a pure function of the project and the
frame number.

## Video layers and layer time

A `video` layer plays a clip from a slot. The slot's `path` is any file the
system ffmpeg decodes (MP4, MOV, MKV, WebM, GIF, ...); ffmpeg and ffprobe
must be on `PATH` (or named by `EDITS_FFMPEG` and `EDITS_FFPROBE`).

```json
"slots": { "op": { "path": "clips/op.mp4", "trim": [12.5, 16] } },
"layers": [
  { "id": "clip", "kind": "video", "slot": "op", "in": 8, "out": 16,
    "speed": { "keys": [ { "t": 8, "v": 1, "ease": "in_out" }, { "t": 9, "v": 0.2 } ] },
    "frame_blend": "mix", "volume": 0, "fps": 12 }
]
```

| Field | Default | Meaning |
| --- | --- | --- |
| slot `trim` | the whole file | `[in, out]` seconds of the file that play. It belongs to the slot, so it swaps with the content. |
| `speed` | 1 | Playback rate (0.5 half speed, 2 double). Keyed in project time like any value; an animated speed is integrated from the layer's `in`, so a ramp plays on from where the clip is. Not negative (use `reverse`). |
| `time_remap` | none | Seconds of the trimmed clip to show, as a function of project time. Overrides `speed`. |
| `loop` | false | Start again after the last frame. Without it the last frame holds. |
| `reverse` | false | Play backwards: the layer starts on the last frame. |
| `frame_blend` | `none` | What shows between two source frames (slow motion). `none` shows the frame at or before the instant. `mix` cross-fades the two neighbouring frames (in linear light) by the fraction between them: smooth, but a moving edge shows twice. `flow` estimates the optical flow between the two frames and moves both to the instant between them before the cross-fade, so a moving edge shows once, in between; where the motion cannot be followed (a cut, a flash, something too fast or too small) it falls back to `mix`. A still clip looks the same with either. `flow` costs about 0.05 s per frame pair and 0.1 s per in-between at 1080p on 4 CPU cores; see [RENDERING.md](RENDERING.md#flow-frame-blending). |
| `volume` | 1 | Gain of the clip's own sound in `edits render`; 0 mutes it. Mixed only while the speed is constant and there is no `time_remap` (a constant speed keeps the pitch). |

Source time starts at the layer's `in`: at `in` the layer shows the first
frame of the trim (the last with `reverse`). Frames are counted from the
first frame of the file, `frame = floor(seconds x clip fps)`; clips with a
variable frame rate are read frame by frame, so convert them to a constant
rate (`ffmpeg -i in.mp4 -fps_mode cfr -r 30 out.mp4`) for exact timing. The
picture fits its layer box like an image, by the slot's `fit` and `focus`. An
empty slot shows its placeholder.

**`fps` on any layer** posterizes its time: the layer's transform, effects
and video frame are evaluated only at steps of `1 / fps` seconds counted from
its `in` (`floor((t - in) x fps) / fps`), so `"fps": 12` in a 24 fps project
holds every picture for two frames ("on twos"). Motion blur samples inside a
step are one picture. The layer still comes in and goes out at its own `in`
and `out`, and its transition runs at the project's rate.

## Comps (precomps)

`comps` holds named compositions: groups of layers that one `comp` layer
draws as a single flat picture. Use them for a reusable title card or panel
animated as one, a group that should take one transform, mask, matte or
effect chain, or a little beat loop repeated along the edit.

```json
"comps": {
  "card": { "width": 1080, "height": 400, "duration": 4, "background": [0, 0, 0],
            "layers": [
              { "id": "bg", "kind": "image", "slot": "a" },
              { "id": "title", "kind": "text", "text": "CHAPTER 2", "size": 30,
                "transform": { "opacity": { "keys": [ { "t": 0, "v": 0 }, { "t": 0.5, "v": 1 } ] } } } ] }
},
"layers": [
  { "id": "card1", "kind": "comp", "comp": "card", "in": 8, "out": 12,
    "transform": { "position": [0, 0.3, 0], "rotation": [0, 0, -4] },
    "effects": [ { "kind": "glow" } ] },
  { "id": "card2", "kind": "comp", "comp": "card", "in": 24, "out": 30, "loop": true, "speed": 2 }
]
```

A comp:

| Field | Default | Meaning |
| --- | --- | --- |
| `width`, `height` | the project's | Size in pixels at the project's size (it scales with `--scale` like the frame). |
| `duration` | required | Length in the project's time base. Its layers without `out` end here. |
| `background` | none (transparent) | An opaque sRGB colour `[r, g, b]` under its layers. |
| `layers` | `[]` | Layers of any kind, bottom to top, including `comp` layers (a comp may not contain itself, directly or through others). |

Inside a comp, time is comp time: its layers' `in`, `out` and keys count
from 0 at the comp's start, in the project's time base. Space is in units of
the comp's height, with the origin at the comp's centre, as in the project;
the camera is the default one (the project's camera moves the comp layer, not
what is inside). Layer ids and track mattes are per stack: a comp's layers
may reuse ids of the project's layers, and a matte names a layer of its own
stack. Motion blur is the project's: a comp is rendered at each sample's
instant. Video layers inside a comp play, but their sound is not mixed.

A `comp` layer:

| Field | Default | Meaning |
| --- | --- | --- |
| `comp` | required | A key of `comps`. |
| `offset` | 0 | Comp time at the layer's `in`. |
| `speed` | 1 | Playback rate, keyed in project time; an animated speed is integrated from `in`, as for video. Not negative. |
| `time_remap` | none | Comp time to show, as a function of project time. Overrides `speed` and `offset`. |
| `loop` | false | Start again from comp time 0 after the comp's end. Without it the comp's last instant holds. |
| `reverse` | false | Play backwards: the layer starts on the comp's last instant. |

The comp's picture fills a box of the comp's size (a comp of the project's
size fills the frame at scale 1, exactly like its layers drawn inline), and
then behaves like an image layer: `transform`, `masks`, `effects`, `matte`,
`blend`, `opacity`, `transition` and `fps` apply to the group as a whole.
`edits check` reports an unknown comp, a cycle (`comps form a cycle: a -> b
-> a`) and everything wrong inside a comp, with paths such as
`comps.card.layers[1:"title"].size`; `project_edit` takes the same places as
pointers (`/comps/card/layers/@title/size`).

## Particle layers

A `particles` layer draws many small sprites: falling sakura petals,
twinkles, floating dust, sparks flying out on a hit. Nothing is simulated
frame by frame: each particle's birth time follows from `rate` and `bursts`,
its random numbers from `seed`, and its state at any instant is a formula of
its age, so any frame renders on its own and looks the same however it was
reached.

```json
{ "id": "petals", "kind": "particles", "in": 0, "out": 32,
  "sprite": "petal", "prewarm": true,
  "emitter": { "shape": "rect", "position": [0, -0.62], "size": [2.2, 0.05] },
  "rate": 3, "life": 10, "speed": 0.24, "speed_random": 0.3,
  "direction": 75, "spread": 20, "gravity": 0.01,
  "turbulence": { "amount": 6, "scale": 25, "speed": 0.4 },
  "size": 3, "size_random": 0.3, "color": [1, 0.5, 0.6],
  "fade_in": 0.04, "fade_out": 0.08, "spin": 40, "spin_random": 80, "seed": 1 }
```

```json
{ "id": "hit", "kind": "particles", "in": "b16", "out": "b18", "blend": "add",
  "sprite": "spark", "bursts": [ { "t": "b16", "count": 60 } ],
  "life": 2, "speed": 1.2, "speed_random": 0.6, "drag": 2.5, "gravity": 0.4,
  "size": 0.8, "color": [1, 0.75, 0.35], "color_end": [1, 0.3, 0.1], "fade_out": 0.5 }
```

| Field | Default | Meaning |
| --- | --- | --- |
| `emitter.shape` | `point` | `point`, `line` (along x, `size[0]` long), `rect` (fills `size`) or `circle` (fills the ellipse inside `size`). |
| `emitter.position` | `[0, 0]` | Centre of the emitter in layer space (composition heights from the centre, y down). Can be keyed: each particle is born where the emitter is at its birth. |
| `emitter.size` | `[0, 0]` | `[width, height]` in composition heights. |
| `emitter.angle`, `emitter.edge` | 0, false | Turn of the shape in degrees; `edge` emits from the outline only (a rect's border, a circle's ring). |
| `rate` | 0 | Particles per project time unit (per beat in a beats project), evenly spaced from `in`. Can be keyed; it is integrated. |
| `bursts` | none | `[{ "t", "count" }]`: `count` particles born at `t`. `t` accepts text times (`"b12"`, `"drop"`). A burst at beat `b` shows from the frame at `b`, not a frame before. |
| `life` | 2 | How long each particle lives, in project time units. `life_random` (0..1) shortens each life by up to that fraction. |
| `prewarm` | false | Start emitting one `life` before `in`, so the layer starts full. |
| `speed` | 0.2 | Initial speed in composition heights per second; `speed_random` (0..1) varies it by up to that fraction. |
| `direction`, `spread` | -90, 360 | Direction of the initial velocity in degrees (0 right, 90 down, -90 up) and the width of the cone around it. |
| `gravity` | 0 | Constant acceleration in heights per second squared: a number pulls down (negative: up), `[x, y]` also pushes sideways (wind). |
| `drag` | 0 | Velocity decays as `e^(-drag t)` towards `gravity / drag`. |
| `turbulence` | off | `{ "amount", "scale", "speed" }`: a swirling drift of about `amount` % of the height, from curl noise with eddies `scale` % of the height across, changing `speed` times a second. |
| `size` | 1 | Diameter in % of the height; `size_end` is the size at the end of life (linear), `size_random` (0..1) varies each one. |
| `color`, `color_end` | `[1, 1, 1]`, none | Linear RGB at birth and at the end of life. |
| `opacity`, `opacity_end` | 1, none | Opacity at birth and at the end of life; `fade_in` and `fade_out` are fractions of the life spent fading from and to transparent. |
| `spin`, `spin_random` | 0, 0 | Turning speed and its random part, in degrees per second. Each particle also starts at a random angle. |
| `sprite` | `dot` | `dot`, `spark` (a streak along the velocity, `streak` seconds of travel long, default 0.05), `star` (a four-point twinkle), `petal` (a sakura petal that tumbles about its long axis) or `slot:<name>` (a slot's picture, upright at angle 0, tinted by `color`). |
| `seed` | 0 | Another number gives other particles. |
| `max` | 10000 | Most particles drawn at once (the oldest beyond it are left out). |

Values that can be keyed (`rate`, `emitter.position`, `speed`, `direction`,
`spread`, `size`, `color`, `opacity`) are read at each particle's birth, so a
particle keeps them for its life; the others are plain numbers. Lengths and
positions are in composition heights and sizes in % of the height, so a
layer looks the same at any resolution. Times (`rate`, `bursts`, `life`) are
in the project's time base; motion (`speed`, `gravity`, `drag`, `spin`,
`turbulence`) is per second. The layer's `transform` moves, scales and turns
the whole system (in 3D too), its `blend` mode and `opacity` apply to it as
a whole, its `fps` posterizes it, and motion blur samples see the particles
at each sub-frame instant. `check` reports a negative `rate`, a `life` that
is not positive, an unknown `sprite` and a sprite slot that does not exist.
The presets `sakura`, `sparkles`, `dust` and `beat_burst` add ready-made
particle layers.

## Shape layers

A `shape` layer draws vector shapes: a ring bursting out on a beat, manga
focus lines, anime speed lines, a panel border, a star. `shapes` is a list,
drawn in order (the first at the bottom). Each shape has a geometry (`type`),
a transform of its own, a `fill` and/or a `stroke` (the stroke over the
fill), and `operators` that change the geometry, in the order listed, before
it is painted.

```json
{ "id": "ring", "kind": "shape", "in": "b16", "out": "b17", "blend": "screen",
  "shapes": [
    { "type": "ellipse", "size": { "keys": [ { "t": "b16", "v": [0.05, 0.05], "ease": "expo_out" },
                                            { "t": "b17", "v": [1.2, 1.2] } ] },
      "stroke": { "color": [1, 1, 1], "width": 3 } } ] }
```

```json
{ "id": "spokes", "kind": "shape",
  "shapes": [
    { "type": "line", "from": [0.3, 0], "to": [1.4, 0],
      "stroke": { "width": 0.35, "cap": "round" },
      "operators": [
        { "kind": "repeater", "copies": 60, "rotation": 6,
          "jitter": { "rotation": 3, "scale": 0.2, "opacity": 0.7, "every": 0, "seed": 1 } } ] } ] }
```

Geometry (`type`), centred on the shape's origin, in composition units:

| `type` | Fields (default) | What it is |
| --- | --- | --- |
| `rect` | `size` `[w, h]` or one number (`[0.5, 0.5]`), `roundness` (0: a corner radius) | A rectangle; the outline starts at the top-left corner |
| `ellipse` | `size` (`[0.5, 0.5]`) | The ellipse inside that box; the outline starts at the top |
| `polystar` | `points` (5), `outer_radius` (0.25), `inner_radius` (0.1), `outer_roundness`, `inner_roundness` (0, percent), `polygon` (false) | A star of `points` points, or with `polygon` a regular polygon; the first point is up |
| `path` | `points`, `closed` (false) | Vertices `[x, y]` or `[x, y, in_x, in_y, out_x, out_y]` with bezier handles relative to the vertex (`in` towards the previous vertex). Each vertex can be keyed. At least 2 |
| `line` | `from` (`[-0.25, 0]`), `to` (`[0.25, 0]`) | A straight line |

Every shape also has `position` (`[0, 0]`), `rotation` (degrees,
clockwise) and `scale` (a number or `[x, y]`; it scales the stroke too),
applied after the operators, and `enabled`. Outlines run clockwise on screen.

| Paint | Fields (default) |
| --- | --- |
| `fill` | `color` (`[1, 1, 1]`; straight sRGB `[r, g, b]` or `[r, g, b, a]`), `opacity` (1), `rule` (`nonzero` or `even_odd`) |
| `stroke` | `color` (`[1, 1, 1]`), `opacity` (1), `width` (1, % of height), `cap` (`butt`, `round`, `square`), `join` (`miter`, `round`, `bevel`), `miter_limit` (4), `dash` (`[dash, gap, ...]` in % of height; none: solid), `dash_offset` (% of height) |

| Operator (`kind`) | Fields (default) | What it does |
| --- | --- | --- |
| `trim` | `start` (0), `end` (100), `offset` (0), `individually` (false) | Keeps the part of each path from `start` to `end` percent of its length, moved along by `offset` percent (100 is a whole turn; it wraps on a closed path). Key `end` from 0 to 100 to draw a line on. `individually` trims all the shape's paths as one long path, one after another |
| `repeater` | `copies` (3), `offset` (0), `anchor`, `position`, `rotation`, `scale` (1), `start_opacity`, `end_opacity` (1), `jitter` | Copies everything so far. Copy `i` (from 0) is moved by `i + offset` times the transform: `position` added, `rotation` added (degrees) and `scale` multiplied about `anchor`; its opacity goes from `start_opacity` (first) to `end_opacity` (last). The first copy is drawn at the bottom |
| `round_corners` | `radius` | Rounds every sharp corner with an arc of `radius` (less where a neighbouring segment is shorter than twice the radius) |
| `offset_path` | `amount`, `join` (`miter`), `miter_limit` (4) | Grows (positive) or shrinks (negative) the filled outline by `amount` |
| `zigzag` | `size`, `ridges` (5), `smooth` (false) | Puts `ridges` zigzags of `size` on every segment (waves with `smooth`) |
| `wiggle_path` | `size`, `detail` (6), `speed` (2), `seed` (0) | Moves `detail` points per segment about by up to `size`, `speed` times a second (0: a still, deformed outline), the same way every time for a `seed` |

A repeater's `jitter` changes each copy at random: `position` `[x, y]` (up
to that far), `rotation` (up to that many degrees), `scale` (0.3: 70 % to
130 %), `opacity` (fades by up to that fraction), `seed`, and `every`: how
often the random values are drawn again, in the project's time unit counted
from the layer's `in` (0: at every frame, the flicker of speed lines;
omitted: never). The values come from a hash of (`seed`, copy, time step),
so a frame is the same however it is reached.

Units: geometry, radii, `round_corners`, `offset_path`, `zigzag` and
`wiggle_path` sizes in composition units; stroke `width` and `dash` in % of
the height; angles in degrees; `trim` in percent. Every number field except
`miter_limit`, `dash`, `speed` and the jitter can be keyed. The layer's
`transform` moves, scales and turns the whole layer (in 3D too), and its
masks, effects, matte, opacity, blend mode, transition and `fps` apply as
for any layer. Shapes are rasterised at the output resolution, so they are
sharp at any `--scale`. `check` reports a polystar with fewer than 3
points, a negative stroke width or size, a path with fewer than 2 points,
a colour that is not 3 or 4 numbers, a trim outside 0..100, more than 1000
copies, ridges or wiggle points, and warns about a shape with neither fill
nor stroke. The presets `speed_lines`, `burst_lines` and `ring_pulse` add
ready-made shape layers.

## Text layers

A `text` layer sets its `text` in a font and draws it like any other layer:
`transform` moves, scales and turns it (in 3D too), effects, blend modes and
transitions apply as usual.

```json
{ "id": "title", "kind": "text", "text": "SAY MY\nNAME", "in": 8, "out": 12,
  "size": 14, "weight": 800, "align": "center", "line_height": 1.0, "tracking": 0.5,
  "fill": [1, 1, 1], "stroke": { "color": [0, 0, 0], "width": 0.8 },
  "transform": { "position": [0, 0.3, 0] } }
```

| Field | Default | Meaning |
| --- | --- | --- |
| `text` | (required) | The words; `\n` starts a new line. |
| `font` | Inter | A `.ttf` or `.otf` file, relative to the project file. Without it, Inter (variable, built into the renderer, SIL Open Font Licence) is used. `check` reports a missing file. |
| `size` | 10 | The em, in percent of the composition height (10 = a tenth of the frame's height). Must be positive. |
| `weight` | the font's default | Position on the font's `wght` axis: 100 thin, 400 regular, 700 bold, 900 black. Fonts without that axis (not variable) ignore it. |
| `align` | `center` | `left`, `center` or `right`: how the lines line up in the block. |
| `line_height` | 1.2 | Baseline to baseline, in multiples of `size`. |
| `tracking` | 0 | Extra space after every character, in percent of the composition height; negative tightens. |
| `fill` | `[1, 1, 1]` | Colour of the letters: straight-alpha sRGB, `[r, g, b]` or `[r, g, b, a]` in 0..1. |
| `stroke` | none | `{ "color": [r, g, b(, a)], "width": w }`: an outline reaching `w` percent of the composition height beyond the edge of the letters, drawn beneath every letter's fill (so outlines of neighbouring letters never cover a letter). `color` defaults to black. |
| `max_width` | none | Wrap lines longer than this, in composition units (the frame is 1 high, `16/9` wide in 16:9). Lines break at spaces and dashes and between CJK characters (not before closing or after opening punctuation); a word longer than the line stays whole. |

**Where the block is.** The block is centred on the layer's origin (its
`position`, or `anchor` from it): horizontally, the box is `max_width` wide,
or as wide as the longest line, and lines are aligned inside that box;
vertically, the middle of the block is halfway between the cap height of the
first line and the baseline of the last, so capitals are centred exactly and
descenders hang below. A single line is therefore centred whatever its
`align`; to set left-aligned captions from the left edge, give `max_width`
and move the layer.

Shaping (kerning, ligatures, Arabic and Indic forms) comes from the font.
A right-to-left paragraph is set right to left; a paragraph that mixes
directions is not reordered. There is no fallback to other fonts: a
character the font lacks shows the font's missing-glyph box. Text renders
sharp at any output size (`--scale`): glyphs are rasterised at their size
in output pixels.

### Text animators

`animators` animate a text layer letter by letter: a typewriter, letters
popping in on a beat, a flicker, a wave. Each animator has a `selector`,
which gives every character (or word, or line) a selection between 0 and 1,
and properties, which the selection scales: a glyph selected at 1 gets the
animator's values in full, at 0 it is untouched, at 0.5 it is half way.

```json
{ "id": "title", "kind": "text", "text": "SAY MY NAME", "size": 14,
  "animators": [
    { "name": "typewriter",
      "selector": { "kind": "range", "smoothness": 0,
                    "start": { "keys": [ { "t": 8, "v": 0 }, { "t": 10, "v": 100 } ] } },
      "opacity": 0 },
    { "selector": { "kind": "wiggly", "amount": 60, "frequency": 3 },
      "position": [0, 0.01, 0] } ] }
```

Here the characters from `start` to 100 % are selected and so invisible;
`start` runs from 0 to 100 % between beats 8 and 10, so the text is typed
out. Every value of a selector or a property is animatable (a constant or
keys, like any value).

| Property | Meaning at full selection |
| --- | --- |
| `position` | Offset `[x, y, z]` in composition units, added. |
| `scale` | Scale of the glyph (a number, `[x, y]` or `[x, y, z]`); 0 hides it. Several animators multiply. |
| `rotation` | `[x, y, z]` degrees about the middle of the glyph (half a cap height above the baseline), X then Y then Z like layers: `[0, 90, 0]` turns each letter edge-on, in 3D. Added. |
| `opacity` | Opacity of the glyph, 0..1 (0 hides it). Several animators multiply. |
| `fill` | Colour of the letters, `[r, g, b]` or `[r, g, b, a]` straight sRGB; the glyph's colour mixes toward it. The stroke keeps its colour. |
| `tracking` | Extra space after the glyph, percent of the composition height; the lines stay aligned (a centred line grows both ways). |
| `blur` | Gaussian blur of the glyph (sigma), percent of the composition height, in the text's own space. |

An animator also takes `name` (shown by `describe`) and `enabled` (default
true). Animators apply in order; offsets, turns, tracking and blur add,
scale and opacity multiply.

**`range` selector** (the default when `selector` is left out). After the
After Effects range selector.

| Field | Default | Meaning |
| --- | --- | --- |
| `start`, `end` | 0, 100 | The range, in percent of the units. Reversed values are swapped. |
| `offset` | 0 | Percent added to `start` and `end`: animate it to slide a range of fixed width through the text. |
| `units` | `characters` | `characters` (every character but line breaks; spaces count), `words` (a space belongs to the word before it) or `lines` (as laid out, after wrapping). |
| `shape` | `square` | `square` (1 inside, 0 outside), `ramp_up` (0 before the range, rising across it, 1 after), `ramp_down` (the reverse), `triangle`, `round`, `smooth` (0 at both ends, 1 in the middle). Each unit is measured at its middle. |
| `smoothness` | 100 | `square` only: the width of its soft edge in percent of a unit. 100 selects a unit half inside the range at 0.5; 0 switches each unit at once (a typewriter). |
| `ease_high`, `ease_low` | 0 | -100..100: positive eases into full selection (high) or out of none (low), negative makes the change abrupt. |
| `amount` | 100 | -100..100 percent: scales the selection (negative inverts the properties). |
| `ease` | linear | Any ease name: the curve each unit follows as it goes from selected (1) to not selected (0). `back_out` makes letters overshoot as they settle (`pop_in`). |
| `randomize`, `seed` | false, 0 | Shuffle the order of the units, the same way for the same seed. |

**`wiggly` selector**: smooth seeded noise per unit, between `-amount` and
`amount` percent. Negative selections push the properties the other way
(a `position` of `[0, 0.02, 0]` moves letters up and down). Its fields:
`units`, `amount` (100), `frequency` (wiggles per second, 2), `correlation`
(percent, 50: 100 moves every unit together, 0 each on its own) and `seed`
(0). One value per glyph: all dimensions of a property move together.

`check` reports animator values out of range (`smoothness` 0..100,
`ease_high`, `ease_low` and `amount` -100..100, `correlation` 0..100), a
`fill` that is not a colour, a negative `blur`, an unknown `ease`, and warns
about an animator that sets no property.

