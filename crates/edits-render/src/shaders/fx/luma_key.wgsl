// luma_key: transparency from the layer's own brightness, measured as a
// `luma` track matte does (Rec. 709 weights on display values). Pixels
// darker or brighter than `threshold`, or within or outside `tolerance` of
// it, are keyed out, with a smooth ramp `softness` wide.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/keying.rs (Luma Key). Copyright (c) 2026 ArtCraft Team
// and the EffectCraft contributors. MIT License (see THIRD_PARTY.md).
// Altered: a softness ramp in place of edge thin / feather (which need more
// passes), an amount, a matte view, WGSL.
//
// p0 = threshold  p1 = tolerance  p2 = softness  p3 = amount
// OPT_KEY_OUT: KEY_OUT_DARKER, KEY_OUT_BRIGHTER, KEY_OUT_SIMILAR, KEY_OUT_DISSIMILAR
// OPT_VIEW: VIEW_FINAL or VIEW_MATTE

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let l = fxl_matte_luma(fxl_display(c));
    var keep = 1.0;
    if OPT_KEY_OUT == KEY_OUT_DARKER {
        keep = fxl_ramp(l, fx.p0, fx.p2);
    } else if OPT_KEY_OUT == KEY_OUT_BRIGHTER {
        keep = 1.0 - fxl_ramp(l, fx.p0 - fx.p2, fx.p2);
    } else if OPT_KEY_OUT == KEY_OUT_SIMILAR {
        keep = fxl_ramp(abs(l - fx.p0), fx.p1, fx.p2);
    } else {
        keep = 1.0 - fxl_ramp(abs(l - fx.p0), fx.p1, fx.p2);
    }
    let m = mix(1.0, keep, fx.p3);
    if OPT_VIEW == VIEW_MATTE {
        return vec4<f32>(vec3<f32>(c.a * m), 1.0);
    }
    return c * m;
}
