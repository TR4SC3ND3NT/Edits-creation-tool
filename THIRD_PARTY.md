# Third-party code

Code in this repository that was ported or adapted from other projects, with
where it came from, its licence and what was taken. Each ported file also
keeps a header naming its source. Licence texts are reproduced below.

The 22 transitions in `crates/edits-render/src/shaders/transitions/` come from
the gl-transitions collection; their authors and licence (MIT) are listed in
`crates/edits-render/src/shaders/transitions/LICENSE.md`.

## Video time

| Our file | Source | Licence | What was taken |
| --- | --- | --- | --- |
| `crates/edits-core/src/eval.rs` (`frame_position`) | EffectCraft, https://github.com/storytold/effectcraft, `crates/render/src/frames.rs` (`frame_position`) | MIT OR Apache-2.0 (used under MIT) | The split of a source time into the frame at or before it and the fraction to the next one. Ours works on a floating frame position with a millionth-of-a-frame tolerance instead of integer ticks. |
| `crates/edits-core/src/eval.rs` (`integrate_speed`, `Evaluator::source_seconds`) | EffectCraft, `crates/effects/src/time_fx.rs` (`timewarp_integrate_speed`) | MIT OR Apache-2.0 (used under MIT) | Integrating an animated speed by the midpoint rule on a fixed grid anchored at 0 (two samples per frame, at most 50,000 samples, a partial last cell), so successive frames share samples and a hold key lands the same way on every frame. Speed is a multiplier instead of a percentage, non-finite samples count as 0, and only forward time from the layer's `in` is integrated. |
| `crates/edits-render/src/flow.rs` | EffectCraft, https://github.com/storytold/effectcraft, `crates/raster/src/flow.rs` (block flow, Pixel Motion) and `crates/gpu/src/shaders/fx_time.wgsl` (`ftm_motion`) | MIT OR Apache-2.0 (used under MIT) | Written after the published description, not from the code: hierarchical block matching on luma (box pyramid, exhaustive search at the top, 3x3 search around the doubled coarser vectors of the tile and its 4 neighbours, a small length penalty), a parabola through the costs for sub-pixel vectors, a 3x3 median, and the in-between as frame `a` sampled at -w x the vector and `b` at (1 - w) x the vector, cross-faded by w, falling back to the unwarped frames where the two disagree. Ours adds flow both ways, per-pixel vectors, a per-pixel choice of motion confirmed by each frame (forward-backward consistency along the path), occlusion handling and a linear-light blend. |

## Effects

| Our file | Source | Licence | What was taken |
| --- | --- | --- | --- |
| `crates/edits-render/src/shaders/fx/twirl.wgsl` | EffectCraft, https://github.com/storytold/effectcraft, `crates/effects/src/distort.rs` (`twirl`) and `crates/gpu/src/shaders/fx_distort.wgsl` (`dst_twirl`) | MIT OR Apache-2.0 (used under MIT) | The twirl mapping: turn by -angle x (1 - d / r)^2 inside the radius. Rewritten in composition units with the radius as % of height. |
| `crates/edits-render/src/shaders/fx/bulge.wgsl` | EffectCraft, `crates/effects/src/distort.rs` (`bulge`), `fx_distort.wgsl` (`dst_bulge`) | MIT OR Apache-2.0 (used under MIT) | The bulge profile: amount x (1 - d^2)^taper / 2 and the inverse mapping. One radius instead of two; taper as 0..1; no edge pinning. |
| `crates/edits-render/src/shaders/fx/kaleidoscope.wgsl` | EffectCraft, `crates/effects/src/stylize2.rs` (`kaleida`) | MIT OR Apache-2.0 (used under MIT) | The wedge fold: angle modulo the segment, mirrored past half a segment. Any number of segments, mirroring as an option, zoom. |
| `crates/edits-render/src/shaders/fx/chromatic_aberration.wgsl` | Bevy, https://github.com/bevyengine/bevy, `crates/bevy_post_process/src/effect_stack/chromatic_aberration.wesl` (the technique of Playdead's INSIDE) | MIT OR Apache-2.0 (used under MIT) | Averaging samples along the line to the centre, each weighted by a spectral lookup (Bevy's default red-green-blue table, written as a function) and normalised per channel. The spread is centred on the pixel, given at the corners in % of height, with a fixed sample count. |
| `crates/edits-render/src/shaders/fx/pixelate.wgsl` (square, polar) | GPUImage, https://github.com/BradLarson/GPUImage, `GPUImagePixellateFilter.m`, `GPUImagePolarPixellateFilter.m` ("@fattjake based on vid by toneburst") | BSD-3-Clause | Sampling each cell at its centre; polar cells by radius and angle. A whole number of segments per ring is ours. |
| `crates/edits-render/src/shaders/fx/pixelate.wgsl` (hex) | glfx.js, https://github.com/evanw/glfx.js, `src/filters/fun/hexagonalpixelate.js` | MIT | The skewed-lattice search for the nearest hexagon centre. The triangle cells on the same lattice are ours. |
| `crates/edits-render/src/shaders/fx/halftone.wgsl` | Vidvox ISF-Files, https://github.com/Vidvox/ISF-Files, `ISF/RGB Halftone.fs` and `ISF/CMYK Halftone.fs` ("by zoidberg") | MIT | The screen angles (15, 45, 75 for RGB; 15, 45, 0, 75 for C, M, Y, K), the rotated-grid cell lookup, and the RGB to CMYK separation. The area-proportional dot (solving the radius of a clipped disc), line and square inks, the mono mode and the five-point cell average are ours. |
| `crates/edits-render/src/shaders/fx/wave_warp.wgsl` | EffectCraft, `crates/effects/src/distort.rs` (`wave_shape`, `wave_pin`, `wave_warp`) | MIT OR Apache-2.0 (used under MIT) | The nine wave shapes, the nine pinning weights and the sideways push. Composition units, the phase computed on the CPU, pinning against the frame. |
| `crates/edits-render/src/shaders/fx/ripple.wgsl` | EffectCraft, `crates/effects/src/distort.rs` (`ripple`) | MIT OR Apache-2.0 (used under MIT) | The radial push amplitude x (1 - d / r) x sin(...). Radial only (no tangential component). |
| `crates/edits-render/src/shaders/fx/mirror.wgsl` | EffectCraft, `crates/effects/src/distort.rs` (`mirror`) | MIT OR Apache-2.0 (used under MIT) | Reflecting the half-plane the normal points to. |
| `crates/edits-render/src/shaders/fx/polar.wgsl` | EffectCraft, `crates/effects/src/distort.rs` (`polar`) | MIT OR Apache-2.0 (used under MIT) | The two polar mappings and the interpolation amount; options named after the result. |
| `crates/edits-render/src/shaders/fx/displacement_map.wgsl` | EffectCraft, `crates/effects/src/distort2.rs` (Displacement Map) | MIT OR Apache-2.0 (used under MIT) | (channel - 0.5) x 2 x max displacement per axis. The map is always stretched to the frame. |
| `crates/edits-render/src/shaders/fx/turbulent_displace.wgsl` | EffectCraft, `crates/effects/src/distort2.rs` (Turbulent Displace) | MIT OR Apache-2.0 (used under MIT) | Displacement by fbm, the twist and bulge modes from the noise gradient by central differences. Our value noise, no pinning. |
| `crates/edits-render/src/shaders/fx/edge_detect.wgsl` | EffectCraft, `crates/effects/src/misc.rs` (Find Edges) | MIT OR Apache-2.0 (used under MIT) | Sobel on luminance, edge strength clamped to 1, invert. Threshold, gain, sample distance and line colour are ours. |
| `crates/edits-render/src/shaders/fx/emboss.wgsl` | EffectCraft, `crates/effects/src/misc.rs` (Emboss) | MIT OR Apache-2.0 (used under MIT) | 0.5 + the luminance difference of two samples along the light's direction x contrast. |
| `crates/edits-render/src/shaders/fx/film_burn.wgsl` | EffectCraft, `crates/effects/src/stylize3.rs` (CC Burn Film) | MIT OR Apache-2.0 (used under MIT) | f = 0.65 noise + 0.35 (1 - d), the threshold 1 - 1.25 amount, the band and the char-then-hot edge. The hole burns to light, a colour, animated noise. |
| `crates/edits-core/src/cube.rs` | EffectCraft, `crates/effects/src/utility.rs` (`parse_cube`) | MIT OR Apache-2.0 (used under MIT) | The `.cube` rules: keywords, 1D then 3D rows, domain, the size checks. Error messages and baking into one 3D table are ours. |
| `crates/edits-render/src/shaders/fx/ink.wgsl` | glfx.js, `src/filters/fun/ink.js` | MIT | A large minus a small neighbourhood average as the ink measure. Display luminance, a threshold, ink and paper colours, solid blacks are ours. |
| `crates/edits-render/src/shaders/fx/toon.wgsl` | GPUImage, `GPUImageToonFilter.m` | BSD-3-Clause | Quantised colour plus outlines where the Sobel magnitude passes a threshold. |
| `crates/edits-render/src/shaders/fx/kuwahara.wgsl` | GPUImage, `GPUImageKuwaharaFilter.m` | BSD-3-Clause | Four quadrants, mean and variance, the calmest wins. A fixed 4x4 sample grid per quadrant. |
| `crates/edits-render/src/shaders/fx/sharpen.wgsl` | AMD FidelityFX FSR 1 RCAS, as used in Bevy `crates/bevy_anti_alias/src/contrast_adaptive_sharpening/robust_contrast_adaptive_sharpening.wesl` | MIT (both) | The RCAS lobe: min / max of the four neighbours, the limit 0.25 - 1/16, the normalised weighted sum. Written from the published formula; no denoise. |

The interface of these effects (a `fn effect(uv)` per file and the shared
prelude in `shaders/fx/prelude.wgsl`) is ours. So are `shockwave`, `tile`,
`lens_distortion`, `screentone`, `glitch`, `scanlines`, `vhs`,
`film_damage`, `gradient_map`, `levels`, `vibrance`, `lut` (the shader),
`fractal_noise`, `gradient_ramp` and the helpers in `shaders/fx/lib.wgsl`;
where a header names another project for a look (three.js, pmndrs
postprocessing, ISF), no code was taken from it.

## Multi-pass light effects and blend modes

| Our file | Source | Licence | What was taken |
| --- | --- | --- | --- |
| `crates/edits-render/src/shaders/passes.wgsl` (`fs_bloom_*`), `crates/edits-render/src/fx.rs` (`bloom`) | Bevy, https://github.com/bevyengine/bevy, `crates/bevy_post_process/src/bloom/bloom.wesl` and `mod.rs` | MIT OR Apache-2.0 (used under MIT) | The 13-tap downsample (Call of Duty: Advanced Warfare, Jimenez 2014) and its weights, Karis' average on the first level, the soft threshold and its precomputed constants, the 3x3 tent upsample. The threshold applied per pixel before the chain, the level weights by size in % of height (instead of Bevy's blend constants) and the add with coverage are ours. |
| `crates/edits-render/src/shaders/passes.wgsl` (`fs_rays_mix`), `crates/edits-render/src/fx.rs` (`light_rays`) | EffectCraft, https://github.com/storytold/effectcraft, `crates/effects/src/generate2.rs` (`light_rays`) and `crates/gpu/src/shaders/fx_light.wgsl` (`flt_rays`) | MIT OR Apache-2.0 (used under MIT) | Rays as samples of the picture scaled toward the centre (from 1 down to 1 - length), and the add, screen and lighten transfer modes. The bright pass, the half-size multi-pass streak and the screen kept from darkening above 1 are ours. |
| `crates/edits-render/src/shaders/passes.wgsl` (`fs_lens_flare`), `crates/edits-render/src/fx.rs` (`flare_lens`) | EffectCraft, `crates/effects/src/generate2.rs` (`lens_flare`, `flare_ghosts`) | MIT OR Apache-2.0 (used under MIT) | The core, streak and halo formulas and the three tables of ghosts (position, radius, colour, strength, ring). Measured on the frame in composition units instead of the layer, with a `size` factor. |
| `crates/edits-render/src/shaders/passes.wgsl` (`fs_tilt`) | glfx.js, https://github.com/evanw/glfx.js, `src/filters/blur/tiltshift.js` | MIT | Two passes of a triangle filter whose radius grows by smoothstep with the distance from a line, and the jittered sample positions. The sharp band of its own width, the tap count from the radius, and the jitter from a hash of the pixel (instead of a random number) are ours. |
| `crates/edits-render/src/compose.rs` (blend modes) | EffectCraft, `crates/color/src/blend.rs`; the formulas of W3C "Compositing and Blending Level 1" | MIT OR Apache-2.0 (used under MIT) | The list of modes, the burn, dodge, soft light, vivid light, pin light, hard mix and divide formulas with their edge cases, and SetLum / SetSat / ClipColor. Rec. 709 luminance, and the rules that keep each mode sensible above 1 (clamped product terms, the backdrop's excess added back, scaling for the component modes), are ours. |

`outline` (jump flooding after Rong and Tan, 2006) and `drop_shadow` are ours.

## Particles

| Our file | Source | Licence | What was taken |
| --- | --- | --- | --- |
| `crates/edits-core/src/particles.rs` (`evaluate`, `rate_births`) | EffectCraft, https://github.com/storytold/effectcraft, `crates/effects/src/sim/weather.rs` | MIT OR Apache-2.0 (used under MIT) | The closed-form idea only: particles born at fixed intervals of the rate, per-item constants hashed from (index, seed), and only the items whose birth falls in the live window `[t - life, t]` evaluated. The emitter shapes, bursts, keyed-rate integration, drag and gravity formula and the code are ours; nothing was copied. |
| `crates/edits-core/src/particles.rs` (`curl`, `turbulence`) | EffectCraft, `crates/gpu/src/shaders/fx_particles.wgsl` (`fxp_curl_pot`, `fxp_curl_vel`) | MIT OR Apache-2.0 (used under MIT) | A divergence-free field as the curl of a scalar noise potential (its gradient by central differences, turned by 90 degrees). Our version evaluates a 3D value-noise potential on the CPU per particle and applies it as a bounded drift (curl now minus curl at birth) instead of advecting pixels. |

The particle sprites in `crates/edits-render/src/shaders/particles.wgsl` are ours.

## Text animators

| Our file | Source | Licence | What was taken |
| --- | --- | --- | --- |
| `crates/edits-core/src/text_select.rs` (range selector) | Skottie, https://skia.googlesource.com/skia, `modules/skottie/src/text/RangeSelector.cpp`; EffectCraft, `crates/text/src/selectors.rs` | BSD-3-Clause (Skia); MIT OR Apache-2.0 (EffectCraft, used under MIT) | Written after their semantics (those of the After Effects range selector), not from their code, which was not copied: start, end and offset in percent of the units, the shapes, each unit measured at its middle, `ease_high` / `ease_low` as a cubic whose handles move with the two values, a seeded shuffle of the units. The `smoothness` window of `square`, the per-unit `ease`, the word and line grouping and the code are ours. |
| `crates/edits-core/src/text_select.rs` (wiggly selector) | EffectCraft, `crates/text/src/selectors.rs` | MIT OR Apache-2.0 (used under MIT) | The idea of mixing a noise track shared by every unit with one per unit by a `correlation`. The noise is the engine's own (`eval::noise`). |

## Licence texts

### EffectCraft (MIT)

```
MIT License

Copyright (c) 2026 ArtCraft Team and the EffectCraft contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The ArtCraft name and logos are trademarks of the ArtCraft Team; nothing of
them is used here.

### Bevy (MIT)

Bevy's `LICENSE-MIT` names no copyright holder; the Bevy contributors are credited here.

```
MIT License

Copyright (c) the Bevy contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### glfx.js (MIT)

```
Copyright (C) 2011 by Evan Wallace

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

### Vidvox ISF-Files (MIT)

```
MIT License

Copyright (c) 2018

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The ported files credit "zoidberg" as their author, as the ISF files do.

### GPUImage (BSD-3-Clause)

```
Copyright (c) 2012, Brad Larson, Ben Cochran, Hugues Lismonde, Keitaroh Kobayashi, Alaric Cole, Matthew Clark, Jacob Gundersen, Chris Williams.
All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.
Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.
Neither the name of the GPUImage framework nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.
THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### AMD FidelityFX FSR (MIT)

```
Copyright (c) 2021 Advanced Micro Devices, Inc. All rights reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```
