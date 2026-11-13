// mirror: the side of a line its normal points to shows the other side,
// reflected in the line.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/distort.rs `mirror`. Copyright (c) 2026 ArtCraft Team and
// the EffectCraft contributors. MIT License (see THIRD_PARTY.md). Altered:
// composition units, WGSL.
//
// p0, p1 = a point on the line  p2, p3 = unit normal

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p0, fx.p1);
    let n = vec2<f32>(fx.p2, fx.p3);
    let p = fx_to_comp(uv);
    let d = dot(p - c, n);
    return src(select(uv, fx_to_uv(p - 2.0 * d * n), d > 0.0));
}
