// toon: cel shading. Display colour quantised to a few bands per channel,
// with outlines where the Sobel edge strength passes a threshold.
//
// After GPUImage (https://github.com/BradLarson/GPUImage),
// GPUImageToonFilter.m. Copyright (c) 2012, Brad Larson, Ben Cochran, Hugues
// Lismonde, Keitaroh Kobayashi, Alaric Cole, Matthew Clark, Jacob Gundersen,
// Chris Williams. All rights reserved. BSD-3-Clause License (see
// THIRD_PARTY.md). Altered: the edge measured on display luminance and
// normalised, a soft outline edge, an outline colour, sizes in % of height,
// WGSL.
//
// p0 = bands  p1 = edge threshold  p2 = sample distance (composition units)
// p3 = amount  p4..p6 = outline colour (display)

fn toon_l(uv: vec2<f32>, d: vec2<f32>, i: f32, j: f32) -> f32 {
    return fxl_display_luma(src(uv + d * vec2<f32>(i, j)));
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fx_delta_uv(vec2<f32>(max(fx.p2, fx_pixel())));
    let tl = toon_l(uv, d, -1.0, -1.0);
    let t = toon_l(uv, d, 0.0, -1.0);
    let tr = toon_l(uv, d, 1.0, -1.0);
    let l = toon_l(uv, d, -1.0, 0.0);
    let r = toon_l(uv, d, 1.0, 0.0);
    let bl = toon_l(uv, d, -1.0, 1.0);
    let b = toon_l(uv, d, 0.0, 1.0);
    let br = toon_l(uv, d, 1.0, 1.0);
    let g = length(vec2<f32>((tr + 2.0 * r + br) - (tl + 2.0 * l + bl), (bl + 2.0 * b + br) - (tl + 2.0 * t + tr))) * 0.25;
    let n = fx.p0 - 1.0;
    let banded = floor(clamp(fxl_display(c), vec3<f32>(0.0), vec3<f32>(1.0)) * n + vec3<f32>(0.5)) / n;
    let outline = smoothstep(fx.p1, fx.p1 + 0.05, g);
    let shown = mix(banded, vec3<f32>(fx.p4, fx.p5, fx.p6), outline);
    return mix(c, fxl_from_display(shown, c.a), fx.p3);
}
