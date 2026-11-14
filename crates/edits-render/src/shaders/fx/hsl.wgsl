// hsl: hue, saturation and lightness adjustments per hue band, on display
// values. The packer bakes the eight bands into a table indexed by hue (each
// band full strength at its centre, blending smoothly into its neighbours):
// .x the hue shift in turns, .y the saturation step, .z the lightness step.
// Greys (saturation under 0.05) take less of it, pure greys none.
//
// Written for edits; the saturation and lightness steps follow EffectCraft's
// Hue/Saturation (https://github.com/storytold/effectcraft,
// crates/effects/src/color_fx.rs, adjust_sat / adjust_light). Copyright (c)
// 2026 ArtCraft Team and the EffectCraft contributors. MIT License (see
// THIRD_PARTY.md). License of this file: MIT OR Apache-2.0.
//
// (no numbers: everything is in the table)

fn hsl_sat(s: f32, k: f32) -> f32 {
    return select(s * (1.0 + k), s + (1.0 - s) * k * min(s, 1.0), k >= 0.0);
}
fn hsl_light(l: f32, k: f32) -> f32 {
    return select(l * (1.0 + k), l + (1.0 - l) * k, k >= 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    let base = clamp(d, vec3<f32>(0.0), vec3<f32>(1.0));
    let hsl = fxl_rgb2hsl(base);
    let a = fxl_table(hsl.x, 0u) * min(hsl.y * 20.0, 1.0);
    let h = fract(hsl.x + a.x + 1.0);
    let s = clamp(hsl_sat(hsl.y, a.y), 0.0, 1.0);
    let l = clamp(hsl_light(hsl.z, a.z), 0.0, 1.0);
    // What lay outside 0..1 (a flash) is added back unchanged.
    return fxl_from_display(fxl_hsl2rgb(vec3<f32>(h, s, l)) + (d - base), c.a);
}
