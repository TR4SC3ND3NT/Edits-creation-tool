// difference_matte: transparent where the picture matches a clean plate (a
// slot's picture stretched over the frame): the largest difference of the
// display channels and of alpha, keyed out up to `tolerance` and kept from
// `tolerance + softness` on.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/keying2.rs (Difference Matte). Copyright (c) 2026
// ArtCraft Team and the EffectCraft contributors. MIT License (see
// THIRD_PARTY.md). Altered: display values, a smooth ramp, no blur before
// the difference (that needs another pass), an amount, WGSL.
//
// p0 = tolerance  p1 = softness  p2 = amount
// OPT_VIEW: VIEW_FINAL or VIEW_MATTE

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let o = fx_aux(uv);
    let dc = abs(fxl_display(c) - fxl_display(o));
    let d = max(max(dc.r, dc.g), max(dc.b, abs(c.a - o.a)));
    let m = mix(1.0, fxl_ramp(d, fx.p0, fx.p1), fx.p2);
    if OPT_VIEW == VIEW_MATTE {
        return vec4<f32>(vec3<f32>(c.a * m), 1.0);
    }
    return c * m;
}
