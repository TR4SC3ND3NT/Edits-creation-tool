# Project format (`edits.project/1`)

A project is one JSON file. It is the only thing the renderer reads, and it is
meant to be edited by hand or by an agent. Paths are relative to the project file.
The Rust types in `crates/edits-core/src/model.rs` are the authority; this page
is the short version.

## Conventions

- **Time.** `in`, `out`, key `t` and `duration` are in the project's `time_base`:
  `beats` (needs `settings.bpm`; beat 0 is at `settings.beat_offset` seconds) or `seconds`.
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

Every layer has `id`, `kind`, optional `in` / `out`, `transform`, `blend`, `effects`, `enabled`.

| `kind` | Extra fields | What it is |
| --- | --- | --- |
| `image` | `slot` | A flat picture from a slot |
| `solid` | `color` `[r, g, b, a]` | A flat colour |
| `cube` | `faces` (slot ids: front, right, back, left, top, bottom), `size` `[w, h, d]` | A real 3D box with a picture on each face |
| `adjustment` | none | Its effects apply to everything beneath it |

`transform` has `position` `[x, y, z]`, `scale` (number, `[x, y]` or `[x, y, z]`),
`rotation` `[x, y, z]`, `anchor` `[x, y]`, `opacity`.

`blend`: `normal`, `add`, `screen`, `multiply`, `overlay`, `lighten`, `darken`, `difference`.

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

`{ "kind": "...", "params": { ... } }`, on a layer or on an adjustment layer.
The registry in `crates/edits-core/src/effects.rs` lists every kind with its
parameters and defaults; `edits effects` prints it.

`exposure`, `fade`, `brightness_contrast`, `saturation`, `hue`, `invert`, `threshold`,
`posterize`, `tint`, `gaussian_blur`, `directional_blur`, `zoom_blur`, `spin_blur`,
`glow`, `vignette`, `rgb_split`, `transform2d`, `grain`, `custom` (a WGSL file in `source`).

## Rendering model

For each output frame the renderer evaluates the whole project at one or more
instants (motion blur samples), draws each active layer, applies its effects,
blends it over the layers beneath, and averages the samples in linear light.
A frame is a pure function of the project and the frame number.
