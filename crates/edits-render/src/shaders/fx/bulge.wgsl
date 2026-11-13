// bulge: a lens over a disc. Inside the radius each pixel reads the picture
// closer to (or, pinching, farther from) the centre, by an amount that falls
// off as (1 - d^2)^taper, so the edge of the disc joins the picture smoothly.
//
// Ported from EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/distort.rs `bulge` and crates/gpu/src/shaders/fx_distort.wgsl
// `dst_bulge`. Copyright (c) 2026 ArtCraft Team and the EffectCraft contributors.
// MIT License (see THIRD_PARTY.md). Altered: composition units, one radius,
// taper given as 0..1, no edge pinning, written as a fragment `effect`.
//
// p0 = amount (-1..1)  p1 = radius  p2, p3 = centre  p4 = taper exponent (1..4)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p2, fx.p3);
    let d = (fx_to_comp(uv) - c) / fx.p1;
    let d2 = dot(d, d);
    let amount = fx.p0 * pow(max(1.0 - d2, 0.0), fx.p4) * 0.5;
    let k = 1.0 - amount;
    return src(select(fx_to_uv(c + d * fx.p1 * k), uv, d2 >= 1.0));
}
