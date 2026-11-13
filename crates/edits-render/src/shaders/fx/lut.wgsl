// lut: a colour lookup table (`.cube`, baked into a 3D table on load) applied
// to display values (sRGB-encoded, clamped to 0..1), mixed with the original
// by `amount` in display space, the way `invert` is.
//
// Written for edits (the .cube rules follow EffectCraft's parse_cube; see
// crates/edits-core/src/cube.rs). License: MIT OR Apache-2.0, as the rest of
// the repository.
//
// p0 = amount

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = clamp(fxl_display(c), vec3<f32>(0.0), vec3<f32>(1.0));
    return fxl_from_display(mix(d, fx_lut(d), fx.p0), c.a);
}
