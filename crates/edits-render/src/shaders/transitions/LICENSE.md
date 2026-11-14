# Transitions ported from gl-transitions

The `.wgsl` files in this folder are copied unchanged from the `gl-transitions`
crate, version 2.0.2 for the first 22 and 2.0.3 for the 30 of wave 2 (crates.io,
`generated/wgsl/`; the first 22 are the same in both versions), which generates
them from the GLSL transitions of
https://github.com/gl-transitions/gl-transitions. The renderer
(`crates/edits-render/src/transitions.rs`) rebinds their textures and wraps
their entry point when it compiles them; the files themselves keep their
original header (source, author, licence).

One file is changed: `LumaWipe.wgsl` reads its map (any picture slot) through
the renderer's colour conversion and its Rec. 709 luminance instead of its red
channel, has a soft edge (`softness`) that still starts and ends exactly on the
two pictures, and an `invert` switch. Its header says so.

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
| SimpleZoomOut.wgsl | Tianshuo | MIT | `zoom_out` (2.0.3) |
| DirectionalEased.wgsl | Max Plotnikov | MIT | `directional_eased` (2.0.3) |
| DirectionalWipe.wgsl | gre | MIT | `linear_wipe` (2.0.3) |
| ClockWipe.wgsl | Fernando Kuteken | MIT | `clock_wipe` (2.0.3) |
| RadialWipe.wgsl | Xaychru | MIT | `radial_wipe` (2.0.3) |
| CircleOpen.wgsl | gre | MIT | `circle_open` (2.0.3) |
| HeartWipe.wgsl | gre | MIT | `heart_wipe` (2.0.3) |
| StarWipe.wgsl | Ben Lucas | MIT | `star_wipe` (2.0.3) |
| DiamondWipe.wgsl | gre | MIT | `diamond_wipe` (2.0.3) |
| RandomSquares.wgsl | gre | MIT | `random_squares` (2.0.3) |
| NoiseDissolve.wgsl | towrabbit | MIT | `noise_dissolve` (2.0.3) |
| LumaWipe.wgsl | gre | MIT | `luma_wipe` (2.0.3, changed) |
| WindowBlinds.wgsl | Fabien Benetou | MIT | `window_blinds` (2.0.3) |
| WindowSlices.wgsl | gre | MIT | `window_slices` (2.0.3) |
| GridFlip.wgsl | TimDonselaar | MIT | `grid_flip` (2.0.3) |
| InsideCube.wgsl | gre | MIT | `inside_cube` (2.0.3) |
| SimpleFlip.wgsl | nwoeanhinnogaehr | MIT | `simple_flip` (2.0.3) |
| BookFlip.wgsl | hong | MIT | `book_flip` (2.0.3) |
| Fold.wgsl | nwoeanhinnogaehr | MIT | `fold` (2.0.3) |
| Doorway.wgsl | gre | MIT | `doorway` (2.0.3) |
| HorizontalOpen.wgsl | martiniti | MIT | `horizontal_open` (2.0.3) |
| SplitSlideInHorizontal.wgsl | OllyOllyOlly | MIT | `split_slide` (2.0.3) |
| Ripple.wgsl | gre | MIT | `ripple` (2.0.3) |
| WaterDrop.wgsl | Paweł Płóciennik | MIT | `water_drop` (2.0.3) |
| GlitchBands.wgsl | Yoni Maltsman @friendlyspinach | MIT | `glitch_bands` (2.0.3) |
| GlitchFlicker.wgsl | bread | MIT | `glitch_flicker` (2.0.3) |
| VhsTracking.wgsl | gre | MIT | `vhs_tracking` (2.0.3) |
| TvStatic.wgsl | Brandon Anzaldi | MIT | `tv_static` (2.0.3) |
| StretchSlide.wgsl | gre | MIT | `stretch_slide` (2.0.3) |
| Squeeze.wgsl | gre | MIT | `squeeze` (2.0.3) |

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
