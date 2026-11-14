// haze: removes a haze of `color` (display values): (value - d x color) /
// (1 - d), where d = distance + slope x y (y from 0 at the top of the frame
// to 1 at the bottom). Positive d removes haze, negative adds it. Results
// below 0 are clipped.
//
// Ported from GPUImage (https://github.com/BradLarson/GPUImage),
// framework/Source/GPUImageHazeFilter.m. Copyright (c) 2012, Brad Larson,
// Ben Cochran, Hugues Lismonde, Keitaroh Kobayashi, Alaric Cole, Matthew
// Clark, Jacob Gundersen, Chris Williams. All rights reserved. License:
// BSD-3-Clause (full text in THIRD_PARTY.md). Altered: the haze colour as a
// parameter (GPUImage's todo), straight display values of a premultiplied
// linear picture (GPUImage's other todo), WGSL.
//
// p0 = distance  p1 = slope  p2..p4 = haze colour (display)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = min(fx.p0 + fx.p1 * uv.y, 0.95);
    let o = (fxl_display(c) - d * vec3<f32>(fx.p2, fx.p3, fx.p4)) / (1.0 - d);
    return fxl_from_display(max(o, vec3<f32>(0.0)), c.a);
}
