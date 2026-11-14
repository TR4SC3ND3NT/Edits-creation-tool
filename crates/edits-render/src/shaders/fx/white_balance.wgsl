// white_balance: on display values, `tint` moves the colour along the Q
// (green to magenta) axis of YIQ, then `temperature` mixes toward the
// picture overlaid with an orange warming filter (0.93, 0.54, 0): positive
// warms, negative extrapolates away from it (cools).
//
// Ported from GPUImage (https://github.com/BradLarson/GPUImage),
// framework/Source/GPUImageWhiteBalanceFilter.m. Copyright (c) 2012, Brad
// Larson, Ben Cochran, Hugues Lismonde, Keitaroh Kobayashi, Alaric Cole,
// Matthew Clark, Jacob Gundersen, Chris Williams. All rights reserved.
// License: BSD-3-Clause (full text in THIRD_PARTY.md). Altered: the YIQ
// matrices are applied as written (row by row: GPUImage's GLSL applies them
// transposed), temperature and tint as -1..1, straight display values of a
// premultiplied linear picture, WGSL.
//
// p0 = temperature (the mix toward the warmed picture)  p1 = change of Q

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    var yiq = vec3<f32>(dot(d, vec3<f32>(0.299, 0.587, 0.114)), dot(d, vec3<f32>(0.596, -0.274, -0.322)), dot(d, vec3<f32>(0.212, -0.523, 0.311)));
    yiq.z = clamp(yiq.z + fx.p1, -0.5226, 0.5226);
    let rgb = vec3<f32>(dot(yiq, vec3<f32>(1.0, 0.956, 0.621)), dot(yiq, vec3<f32>(1.0, -0.272, -0.647)), dot(yiq, vec3<f32>(1.0, -1.105, 1.702)));
    let warm = vec3<f32>(0.93, 0.54, 0.0);
    let warmed = select(vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - rgb) * (vec3<f32>(1.0) - warm), 2.0 * rgb * warm, rgb < vec3<f32>(0.5));
    return fxl_from_display(mix(rgb, warmed, fx.p0), c.a);
}
