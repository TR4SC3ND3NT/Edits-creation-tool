// chroma_key: transparent where the colour's chroma (Cb, Cr of YCbCr, on
// display values) is close to the key colour's: fully keyed within
// `tolerance`, kept from `tolerance + softness` on, a smooth ramp between.
//
// Ported from GPUImage (https://github.com/BradLarson/GPUImage),
// framework/Source/GPUImageChromaKeyFilter.m. Copyright (c) 2012, Brad
// Larson, Ben Cochran, Hugues Lismonde, Keitaroh Kobayashi, Alaric Cole,
// Matthew Clark, Jacob Gundersen, Chris Williams. All rights reserved.
// License: BSD-3-Clause (full text in THIRD_PARTY.md). Altered: straight
// display values of a premultiplied linear picture, an amount, a matte view,
// a hard cut at softness 0, WGSL.
//
// p0..p2 = key colour (display)  p3 = tolerance  p4 = softness  p5 = amount
// OPT_VIEW: VIEW_FINAL or VIEW_MATTE

fn ck_cbcr(c: vec3<f32>) -> vec2<f32> {
    let y = 0.2989 * c.r + 0.5866 * c.g + 0.1145 * c.b;
    return vec2<f32>(0.7132 * (c.r - y), 0.5647 * (c.b - y));
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = distance(ck_cbcr(fxl_display(c)), ck_cbcr(vec3<f32>(fx.p0, fx.p1, fx.p2)));
    let m = mix(1.0, fxl_ramp(d, fx.p3, fx.p4), fx.p5);
    if OPT_VIEW == VIEW_MATTE {
        return vec4<f32>(vec3<f32>(c.a * m), 1.0);
    }
    return c * m;
}
