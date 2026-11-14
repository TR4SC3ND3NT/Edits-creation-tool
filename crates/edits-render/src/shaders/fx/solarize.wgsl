// solarize: pixels whose luminance (display values, Rec. 709 weights) is
// above (or below) `threshold` are inverted: each channel becomes
// |1 - value|. `softness` makes the cut a smooth ramp; `amount` mixes with
// the original.
//
// Ported from GPUImage (https://github.com/BradLarson/GPUImage),
// framework/Source/GPUImageSolarizeFilter.m. Copyright (c) 2012, Brad
// Larson, Ben Cochran, Hugues Lismonde, Keitaroh Kobayashi, Alaric Cole,
// Matthew Clark, Jacob Gundersen, Chris Williams. All rights reserved.
// License: BSD-3-Clause (full text in THIRD_PARTY.md). GPUImage inverts at
// or below the threshold (`invert` = below); altered: the side as an option
// (above, the classic photographic look, by default), a softness and an
// amount, straight display values of a premultiplied linear picture, WGSL.
//
// p0 = threshold  p1 = softness  p2 = amount
// OPT_INVERT: INVERT_ABOVE or INVERT_BELOW

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    let lum = dot(d, vec3<f32>(0.2125, 0.7154, 0.0721));
    var w = fxl_ramp(lum, fx.p0, fx.p1);
    if OPT_INVERT == INVERT_BELOW {
        w = 1.0 - w;
    }
    return fxl_from_display(mix(d, abs(vec3<f32>(1.0) - d), w * fx.p2), c.a);
}
