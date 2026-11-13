# Transitions ported from gl-transitions

The `.wgsl` files in this folder are copied unchanged from the `gl-transitions`
crate, version 2.0.2 (crates.io, `generated/wgsl/`), which generates them from
the GLSL transitions of https://github.com/gl-transitions/gl-transitions. The
renderer (`crates/edits-render/src/transitions.rs`) rebinds their textures and
wraps their entry point when it compiles them; the files themselves keep their
original header (source, author, licence).

Every transition here is under the MIT licence; none of the BSD-licensed
transitions of the collection (InvertedPageCurl, StereoViewer) is included.

| File | Author | Licence | `kind` in edits |
| --- | --- | --- | --- |
| Fade.wgsl | gre | MIT | `fade` |
| FadeColor.wgsl | gre | MIT | `fade_color` |
| FlashFade.wgsl | gre | MIT | `flash_fade` |
| Overexposure.wgsl | Ben Zhang | MIT | `overexposure` |
| LightningFlash.wgsl | gre | MIT | `lightning_flash` |
| CrossZoom.wgsl | rectalogic | MIT | `cross_zoom` |
| SimpleZoom.wgsl | 0gust1 | MIT | `simple_zoom` |
| ZoomInOut.wgsl | OllyOllyOlly | MIT | `zoom_in_out` |
| DreamyZoom.wgsl | Zeh Fernando | MIT | `dreamy_zoom` |
| Directional.wgsl | Gaëtan Renaudeau | MIT | `directional` |
| DirectionalWarp.wgsl | pschroen | MIT | `directional_warp` |
| WhipPan.wgsl | gre | MIT | `whip_pan` |
| WhipSpin.wgsl | gre | MIT | `whip_spin` |
| WhipTilt.wgsl | gre | MIT | `whip_tilt` |
| CameraShake.wgsl | gre | MIT | `camera_shake` |
| GlitchMemories.wgsl | Gunnar Roth | MIT | `glitch_memories` |
| GlitchDisplace.wgsl | Matt DesLauriers | MIT | `glitch_displace` |
| Cube.wgsl | gre | MIT | `cube` |
| Swirl.wgsl | Sergey Kosarevsky | MIT | `swirl` |
| RotateScaleFade.wgsl | Fernando Kuteken | MIT | `rotate_scale_fade` |
| Pixelize.wgsl | gre | MIT | `pixelize` |
| FilmBurn.wgsl | Anastasia Dunbar | MIT | `film_burn` |

The licence of the collection (from the crate's `LICENSE` file):

```
MIT License

Copyright (c) 2017-present gl-transitions contributors

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

Note: Individual transitions in the transitions/ directory may have their own
license specified in their file header comments. When no license is specified,
the transition is covered by this MIT license.
```
