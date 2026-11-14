// highlights_shadows: GPUImage's tone curves on the display luminance (with
// its weights 0.3, 0.3, 0.3): the shadows are lifted and the highlights
// pulled down, and each pixel's colour is scaled to its new luminance.
//
// Ported from GPUImage (https://github.com/BradLarson/GPUImage),
// framework/Source/GPUImageHighlightShadowFilter.m. Copyright (c) 2012, Brad
// Larson, Ben Cochran, Hugues Lismonde, Keitaroh Kobayashi, Alaric Cole,
// Matthew Clark, Jacob Gundersen, Chris Williams. All rights reserved.
// License: BSD-3-Clause (full text in THIRD_PARTY.md). Altered: `highlights`
// counts up from 0 (GPUImage's 1 - highlights), the curves see the luminance
// clamped to 0..1 (values above 1 keep their brightness), WGSL.
//
// p0 = shadows (0..1)  p1 = GPUImage's highlights (1 = unchanged, 0 = strongest)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    let lum = clamp(dot(d, vec3<f32>(0.3)), 0.0, 1.0);
    if lum <= 1e-5 {
        return c;
    }
    let shadow = clamp((pow(lum, 1.0 / (fx.p0 + 1.0)) - 0.76 * pow(lum, 2.0 / (fx.p0 + 1.0))) - lum, 0.0, 1.0);
    let inv = 1.0 - lum;
    let highlight = clamp((1.0 - (pow(inv, 1.0 / (2.0 - fx.p1)) - 0.8 * pow(inv, 2.0 / (2.0 - fx.p1)))) - lum, -1.0, 0.0);
    return fxl_from_display(d * ((lum + shadow + highlight) / lum), c.a);
}
