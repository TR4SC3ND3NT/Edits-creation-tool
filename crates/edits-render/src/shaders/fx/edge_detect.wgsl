// edge_detect: the Sobel operator on display luminance; edges become lines
// of a colour on black, or the negative.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/misc.rs (Find Edges: Sobel on luminance, edge strength
// clamped to 1, invert). Copyright (c) 2026 ArtCraft Team and the EffectCraft
// contributors. MIT License (see THIRD_PARTY.md). Altered: the sample
// distance in % of height, a threshold and gain, a line colour, WGSL.
//
// p0 = gain  p1 = threshold  p2 = sample distance (composition units)
// p3 = invert  p4..p6 = line colour (display)  p7 = amount

fn ed_l(uv: vec2<f32>, d: vec2<f32>, i: f32, j: f32) -> f32 {
    return fxl_display_luma(src(uv + d * vec2<f32>(i, j)));
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fx_delta_uv(vec2<f32>(max(fx.p2, fx_pixel())));
    let tl = ed_l(uv, d, -1.0, -1.0);
    let t = ed_l(uv, d, 0.0, -1.0);
    let tr = ed_l(uv, d, 1.0, -1.0);
    let l = ed_l(uv, d, -1.0, 0.0);
    let r = ed_l(uv, d, 1.0, 0.0);
    let bl = ed_l(uv, d, -1.0, 1.0);
    let b = ed_l(uv, d, 0.0, 1.0);
    let br = ed_l(uv, d, 1.0, 1.0);
    let gx = (tr + 2.0 * r + br) - (tl + 2.0 * l + bl);
    let gy = (bl + 2.0 * b + br) - (tl + 2.0 * t + tr);
    // A hard step from black to white measures 1.
    let g = length(vec2<f32>(gx, gy)) * 0.25;
    let e = clamp((g * fx.p0 - fx.p1) / max(1.0 - fx.p1, 1e-3), 0.0, 1.0);
    var shown = vec3<f32>(fx.p4, fx.p5, fx.p6) * e;
    shown = mix(shown, vec3<f32>(1.0) - shown, fx.p3);
    return mix(c, fxl_from_display(shown, c.a), fx.p7);
}
