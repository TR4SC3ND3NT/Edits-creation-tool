// polar: polar coordinates. to_polar wraps the picture into a disc about the
// frame's centre (its width becomes the turn, clockwise from the top, and its
// bottom edge the centre: a tiny planet); to_rect unrolls such a disc back
// into a strip.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/distort.rs `polar`. Copyright (c) 2026 ArtCraft Team and
// the EffectCraft contributors. MIT License (see THIRD_PARTY.md). Altered:
// options named after the result, composition units, WGSL.
//
// p0 = amount (0..1)
// OPT_DIRECTION: DIRECTION_TO_POLAR or DIRECTION_TO_RECT

fn effect(uv: vec2<f32>) -> vec4<f32> {
    // The disc touches the nearer pair of frame edges.
    let rmax = 0.5 * min(fx.aspect, 1.0);
    var m: vec2<f32>;
    if OPT_DIRECTION == DIRECTION_TO_POLAR {
        let d = fx_to_comp(uv);
        var a = atan2(d.x, -d.y);
        a = select(a, a + FXL_TAU, a < 0.0);
        m = vec2<f32>(a / FXL_TAU, 1.0 - length(d) / rmax);
    } else {
        let a = FXL_TAU * uv.x;
        let r = (1.0 - uv.y) * rmax;
        m = fx_to_uv(r * vec2<f32>(sin(a), -cos(a)));
    }
    return src(mix(uv, m, fx.p0));
}
