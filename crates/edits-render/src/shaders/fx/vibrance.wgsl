// vibrance: saturation about the luminance (as `saturation` does), by a
// factor that depends on how saturated the pixel already is: positive amounts
// boost dull colours most and leave fully saturated ones alone; negative
// amounts mute every colour evenly.
//
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = amount (-1..1)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let s = fx_straight(c);
    let hi = max(s.r, max(s.g, s.b));
    let lo = min(s.r, min(s.g, s.b));
    let sat = select(0.0, (hi - lo) / hi, hi > 1e-5);
    let gain = select(1.0 + fx.p0, 1.0 + 2.0 * fx.p0 * (1.0 - sat) * (1.0 - sat), fx.p0 > 0.0);
    let l = fx_luma(s);
    return vec4<f32>(max(mix(vec3<f32>(l), s, gain), vec3<f32>(0.0)) * c.a, c.a);
}
