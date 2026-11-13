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
  "slots": { "a": { "path": "img/a.png", "fit": "cover", "focus": [0.5, 0.4] } },
  "camera": { "fov": 39.6, "position": [0, 0, 0], "rotation": [0, 0, 0] },
  "layers": [ ... ]
}
```

## Layers

Every layer has `id`, `kind`, optional `in` / `out`, `transform`, `blend`, `effects`, `enabled`,
and optionally `transition` (how it comes in; see [Transitions](#transitions)) and `fps`
(posterize time; see [Video layers and layer time](#video-layers-and-layer-time)).

| `kind` | Extra fields | What it is |
| --- | --- | --- |
| `image` | `slot` | A flat picture from a slot |
| `solid` | `color` `[r, g, b, a]` | A flat colour |
| `cube` | `faces` (slot ids: front, right, back, left, top, bottom), `size` `[w, h, d]` | A real 3D box with a picture on each face |
| `adjustment` | none | Its effects apply to everything beneath it |
| `video` | `slot`, optional `speed`, `time_remap`, `loop`, `reverse`, `frame_blend`, `volume` | A clip from a slot, decoded by ffmpeg; see [Video layers](#video-layers-and-layer-time) |
| `text` | `text`, optional `font`, `size`, `weight`, `align`, `line_height`, `tracking`, `fill`, `stroke`, `max_width` | Lines of text in a font file (Inter when none is named); see [Text layers](#text-layers) |

`transform` has `position` `[x, y, z]`, `scale` (number, `[x, y]` or `[x, y, z]`),
`rotation` `[x, y, z]`, `anchor` `[x, y]`, `opacity`.

`blend`: `normal`, `add`, `screen`, `multiply`, `overlay`, `lighten`, `darken`, `difference`,
`color_burn`, `linear_burn`, `darker_color`, `color_dodge`, `linear_dodge` (the same as `add`),
`lighter_color`, `soft_light`, `hard_light`, `vivid_light`, `linear_light`, `pin_light`,
`hard_mix`, `exclusion`, `subtract`, `divide`, `hue`, `saturation`, `color`, `luminosity`.
They work in linear light and stay sensible above 1; `docs/RENDERING.md` has the formulas.

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
- `options`: strings naming a choice that cannot be animated, for example
  `{ "shape": "hex" }` on `pixelate` or `{ "mode": "cmyk", "shape": "line" }`
  on `halftone`. A missing option takes the first of its values.
- `source`: a file the effect reads (a WGSL file for `custom`). Effects that
  take none must not have one.

The registry in `crates/edits-core/src/effects/` (one file per category)
lists every kind with its parameters, units, ranges, options and defaults;
`edits effects` prints it and `edits check` validates against it.

colour: `exposure`, `fade`, `brightness_contrast`, `saturation`, `hue`, `invert`,
`threshold`, `posterize`, `tint`, `vignette`, `grain`; blur: `gaussian_blur`,
`directional_blur`, `zoom_blur`, `spin_blur`; light: `glow`; distort:
`transform2d`, `twirl`, `bulge`, `kaleidoscope`; stylise: `rgb_split`,
`chromatic_aberration`, `pixelate`, `halftone`; and `custom` (a WGSL file in `source`).

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
layer's `in` / `out`, a key's `t`, a marker's `t` and a transition's
`duration` (`"2b"`, `"0.5s"`) may be written this way; they are stored as
numbers. Markers come from `"markers": [{ "t": 16, "label": "drop" }]`;
`edits audio <song> --project <project>` writes the song's drop and
sections there.

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
  adds layers of its own (`letterbox`, `overlay_texture`) and needs none.
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
| `pulse` | Keys on one property, from `at`: `{ "transform": "scale" }`, `{ "effect": "fade", "param": "amount", "params": {...}, "base": 0 }`. `keys` are `{ "dt", "v", "ease" }`. Keys already there before `at` are kept and held until the pulse, keys after it are kept; a missing effect is added. `adjustment` names an effect parameter to use instead when the target is an adjustment layer. |
| `shake` | A `shake` modifier on a property (`transform` or `effect` + `param`, as in a pulse) whose amplitude follows `keys`, at `frequency`. Repeated hits share one modifier. |
| `effect` | Append an effect to the target. |
| `layer` | Insert a layer (`"place": "top"`, `"above"` or `"below"` the target). Its `id` may contain `{target}`; it gets a suffix when taken. |
| `set` | Set a value inside the target (`"pointer": "/transition"`), or inside the picture it cuts from (`"on": "previous"`). |
| `retime` | Replay the target's keyed motion between `at` and `at + length` through a speed `curve` (an ease); `otherwise` lists actions for a layer that does not move. |

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
| `frame_blend` | `none` | `mix` cross-fades the two neighbouring source frames (in linear light) by the fraction between them, for smoother slow motion. `none` shows the frame at or before the instant. |
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

