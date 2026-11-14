// leave_color: colours unlike `color` lose their colour (toward their linear
// Rec. 709 luminance, as `saturation` 0). Likeness is the hue distance (half
// a turn is 1; greys never match) or the RGB distance of display values
// (divided by the square root of 3); within `tolerance` a colour is kept,
// beyond it it greys over `softness`.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/color_fx.rs (Leave Color). Copyright (c) 2026 ArtCraft
// Team and the EffectCraft contributors. MIT License (see THIRD_PARTY.md).
// Altered: likeness on display values, greying in linear light, WGSL.
//
// p0..p2 = colour to keep (display)  p3 = tolerance  p4 = softness  p5 = amount
// OPT_MATCH: MATCH_HUE or MATCH_RGB

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let s = fx_straight(c);
    let d = fxl_display(c);
    let key = vec3<f32>(fx.p0, fx.p1, fx.p2);
    var dist = 0.0;
    if OPT_MATCH == MATCH_RGB {
        dist = distance(d, key) / sqrt(3.0);
    } else {
        let x = fxl_rgb2hsl(clamp(d, vec3<f32>(0.0), vec3<f32>(1.0)));
        let kh = fxl_rgb2hsl(clamp(key, vec3<f32>(0.0), vec3<f32>(1.0))).x;
        dist = select(abs(fract(x.x - kh + 0.5) - 0.5) * 2.0, 1.0, x.y < 0.02);
    }
    var keep = 0.0;
    if fx.p3 >= 1.0 || dist <= fx.p3 {
        keep = 1.0;
    } else if fx.p4 > 1e-6 {
        keep = clamp(1.0 - (dist - fx.p3) / fx.p4, 0.0, 1.0);
    }
    let grey = vec3<f32>(fx_luma(s));
    return vec4<f32>(mix(s, grey, fx.p5 * (1.0 - keep)) * c.a, c.a);
}
