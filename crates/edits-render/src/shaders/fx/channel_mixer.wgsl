// channel_mixer: a 3x4 colour matrix on display values: each output channel
// is [r, g, b] . its row + the row's constant, kept at 0 or above. In HSV
// mode the same matrix takes hue (turns), saturation and value, and the
// output hue wraps around the wheel.
//
// After GPUImage (https://github.com/BradLarson/GPUImage),
// framework/Source/GPUImageColorMatrixFilter.m. Copyright (c) 2012, Brad
// Larson, Ben Cochran, Hugues Lismonde, Keitaroh Kobayashi, Alaric Cole,
// Matthew Clark, Jacob Gundersen, Chris Williams. All rights reserved.
// License: BSD-3-Clause (full text in THIRD_PARTY.md); and EffectCraft
// (https://github.com/storytold/effectcraft), crates/effects/src/color_fx.rs
// (Channel Mixer: a constant per row, no negative output). Copyright (c) 2026
// ArtCraft Team and the EffectCraft contributors. MIT License. Altered: rows
// of [r, g, b, constant] on straight display values, an HSV mode, WGSL.
//
// p0..p3 = red row  p4..p7 = green row  p8..p11 = blue row  p12 = amount
// OPT_SPACE: SPACE_RGB or SPACE_HSV

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    let r = vec4<f32>(fx.p0, fx.p1, fx.p2, fx.p3);
    let g = vec4<f32>(fx.p4, fx.p5, fx.p6, fx.p7);
    let b = vec4<f32>(fx.p8, fx.p9, fx.p10, fx.p11);
    var o: vec3<f32>;
    if OPT_SPACE == SPACE_HSV {
        let x = vec4<f32>(fxl_rgb2hsv(max(d, vec3<f32>(0.0))), 1.0);
        o = fxl_hsv2rgb(vec3<f32>(fract(dot(x, r)), clamp(dot(x, g), 0.0, 1.0), max(dot(x, b), 0.0)));
    } else {
        let x = vec4<f32>(d, 1.0);
        o = max(vec3<f32>(dot(x, r), dot(x, g), dot(x, b)), vec3<f32>(0.0));
    }
    return fxl_from_display(mix(d, o, fx.p12), c.a);
}
