// emboss: grey relief. The display luminance a step away from the light is
// compared with the luminance a step toward it; flat areas are mid grey.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/misc.rs (Emboss: two samples along the direction,
// 0.5 + difference x contrast). Copyright (c) 2026 ArtCraft Team and the
// EffectCraft contributors. MIT License (see THIRD_PARTY.md). Altered: the
// distance in % of height, the light's direction clockwise on screen, WGSL.
//
// p0, p1 = step toward the light (composition units)  p2 = strength  p3 = amount

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fx_delta_uv(vec2<f32>(fx.p0, fx.p1));
    let toward = fxl_display_luma(src(uv + d));
    let away = fxl_display_luma(src(uv - d));
    let v = clamp(0.5 + (away - toward) * fx.p2, 0.0, 1.0);
    return mix(c, fxl_from_display(vec3<f32>(v), c.a), fx.p3);
}
