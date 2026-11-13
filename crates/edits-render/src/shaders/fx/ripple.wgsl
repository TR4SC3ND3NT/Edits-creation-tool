// ripple: circular waves about a centre push the picture toward and away
// from it; the push falls linearly to nothing at the radius.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/distort.rs `ripple`. Copyright (c) 2026 ArtCraft Team and
// the EffectCraft contributors. MIT License (see THIRD_PARTY.md). Altered:
// composition units with sizes in % of height, radial push only, the phase in
// cycles computed on the CPU, WGSL.
//
// p0 = amplitude  p1 = wavelength  p2 = radius  p3, p4 = centre  p5 = phase (cycles)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p3, fx.p4);
    let v = fx_to_comp(uv) - c;
    let d = length(v);
    if d >= fx.p2 || d <= 0.0 {
        return src(uv);
    }
    let falloff = 1.0 - d / fx.p2;
    let push = fx.p0 * falloff * sin(FXL_TAU * (d / fx.p1 + fx.p5));
    return src(fx_to_uv(c + v * ((d + push) / d)));
}
