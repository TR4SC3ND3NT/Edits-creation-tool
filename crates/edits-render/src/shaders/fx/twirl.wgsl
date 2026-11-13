// twirl: the picture turned about a centre by an angle that falls off with the
// square of the distance, reaching zero at the radius.
//
// Ported from EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/distort.rs `twirl` and crates/gpu/src/shaders/fx_distort.wgsl
// `dst_twirl`. Copyright (c) 2026 ArtCraft Team and the EffectCraft contributors.
// MIT License (see THIRD_PARTY.md). Altered: composition units instead of
// pixels, the radius is % of height, written as a fragment `effect`.
//
// p0 = angle (radians, clockwise)  p1 = radius  p2, p3 = centre (composition units)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p2, fx.p3);
    let d = fx_to_comp(uv) - c;
    let r = length(d);
    let t = max(1.0 - r / fx.p1, 0.0);
    let a = -fx.p0 * t * t;
    let sn = sin(a);
    let cs = cos(a);
    let turned = vec2<f32>(d.x * cs - d.y * sn, d.x * sn + d.y * cs);
    // Outside the radius the picture is read where it is, untouched.
    return src(select(fx_to_uv(c + turned), uv, r >= fx.p1));
}
