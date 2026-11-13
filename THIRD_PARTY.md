# Third-party code

Code in this repository that was ported or adapted from other projects, with
where it came from, its licence and what was taken. Each ported file also
keeps a header naming its source. Licence texts are reproduced below.

The 22 transitions in `crates/edits-render/src/shaders/transitions/` come from
the gl-transitions collection; their authors and licence (MIT) are listed in
`crates/edits-render/src/shaders/transitions/LICENSE.md`.

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
