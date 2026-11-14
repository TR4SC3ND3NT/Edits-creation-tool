// replace_color: pixels whose hue, lightness and saturation (HSL of display
// values) are each within their tolerance of `from` change toward `to`;
// beyond a tolerance the change fades out over `softness`. `change` picks
// what changes (hue always), `by` whether it takes `to`'s values (setting)
// or shifts by `to` - `from` (transforming).
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/color_fx.rs (Change to Color). Copyright (c) 2026
// ArtCraft Team and the EffectCraft contributors. MIT License (see
// THIRD_PARTY.md). Altered: an amount, values above 1 kept, WGSL.
//
// p0..p2 = from  p3..p5 = to (display)  p6..p8 = tolerance (hue, lightness, saturation)
// p9 = softness  p10 = amount
// OPT_CHANGE: CHANGE_HUE, CHANGE_HUE_LIGHTNESS, CHANGE_HUE_SATURATION, CHANGE_ALL
// OPT_BY: BY_SETTING or BY_TRANSFORMING

// The signed shortest turn from hue a to hue b.
fn rc_turn(a: f32, b: f32) -> f32 {
    return fract(b - a + 0.5) - 0.5;
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    let base = clamp(d, vec3<f32>(0.0), vec3<f32>(1.0));
    let x = fxl_rgb2hsl(base);
    let f = fxl_rgb2hsl(clamp(vec3<f32>(fx.p0, fx.p1, fx.p2), vec3<f32>(0.0), vec3<f32>(1.0)));
    let t = fxl_rgb2hsl(clamp(vec3<f32>(fx.p3, fx.p4, fx.p5), vec3<f32>(0.0), vec3<f32>(1.0)));
    // Distances: hue (0..1, half a turn is 1), lightness, saturation.
    let dist = vec3<f32>(abs(rc_turn(f.x, x.x)) * 2.0, abs(x.z - f.z), abs(x.y - f.y));
    let over = dist - vec3<f32>(fx.p6, fx.p7, fx.p8);
    var k = 1.0;
    for (var i = 0; i < 3; i++) {
        if over[i] > 0.0 {
            k = min(k, select(clamp(1.0 - over[i] / fx.p9, 0.0, 1.0), 0.0, fx.p9 <= 1e-6));
        }
    }
    if k <= 0.0 {
        return c;
    }
    let ch_l = OPT_CHANGE == CHANGE_HUE_LIGHTNESS || OPT_CHANGE == CHANGE_ALL;
    let ch_s = OPT_CHANGE == CHANGE_HUE_SATURATION || OPT_CHANGE == CHANGE_ALL;
    var n = x;
    if OPT_BY == BY_TRANSFORMING {
        n.x = x.x + rc_turn(f.x, t.x) * k;
        n.y = select(x.y, x.y + (t.y - f.y) * k, ch_s);
        n.z = select(x.z, x.z + (t.z - f.z) * k, ch_l);
    } else {
        n.x = x.x + rc_turn(x.x, t.x) * k;
        n.y = select(x.y, x.y + (t.y - x.y) * k, ch_s);
        n.z = select(x.z, x.z + (t.z - x.z) * k, ch_l);
    }
    let o = fxl_hsl2rgb(vec3<f32>(fract(n.x + 1.0), clamp(n.y, 0.0, 1.0), clamp(n.z, 0.0, 1.0))) + (d - base);
    return fxl_from_display(mix(d, o, fx.p10), c.a);
}
