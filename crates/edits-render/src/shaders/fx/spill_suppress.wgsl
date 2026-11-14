// spill_suppress: takes the key colour's cast out of the straight linear
// colour. Fast: the key's strongest channel is limited to the larger of the
// other two. Better: the part of the colour's chroma (its difference from
// its grey) along the key colour's chroma is removed.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/keying.rs (Spill Suppressor). Copyright (c) 2026
// ArtCraft Team and the EffectCraft contributors. MIT License (see
// THIRD_PARTY.md). Altered: premultiplied linear input, WGSL; the key's
// channels are worked out by the packer.
//
// p0 = amount
// fast:   p1..p3 = the key's strongest channel (one-hot), p4..p6 and p7..p9 = the other two
// better: p1..p3 = the unit direction of the key colour's chroma
// OPT_METHOD: METHOD_FAST or METHOD_BETTER

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let s = fx_straight(c);
    let k = vec3<f32>(fx.p1, fx.p2, fx.p3);
    var o = s;
    if OPT_METHOD == METHOD_BETTER {
        let g = (s.r + s.g + s.b) / 3.0;
        let proj = dot(s - vec3<f32>(g), k);
        o = s - k * max(proj, 0.0) * fx.p0;
    } else {
        let others = max(dot(s, vec3<f32>(fx.p4, fx.p5, fx.p6)), dot(s, vec3<f32>(fx.p7, fx.p8, fx.p9)));
        let spill = max(dot(s, k) - others, 0.0);
        o = s - k * spill * fx.p0;
    }
    return vec4<f32>(o * c.a, c.a);
}
