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

The interface of these effects (a `fn effect(uv)` per file and the shared
prelude in `shaders/fx/prelude.wgsl`) is ours.

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
