// fractal_noise: grey fractal noise (octaves of smooth value noise, each half
// the size and half the weight of the last), with contrast and brightness on
// display values, blended over the picture inside its transparency.
//
// Written for edits (the fbm of shaders/fx/lib.wgsl). License: MIT OR
// Apache-2.0, as the rest of the repository.
//
// p0 = 1 / scale  p1 = octaves  p2 = evolution (steps)  p3 = contrast
// p4 = brightness  p5, p6 = offset  p7 = seed  p8 = amount
// OPT_BLEND: BLEND_REPLACE, BLEND_MULTIPLY, BLEND_SCREEN, BLEND_ADD, BLEND_OVERLAY

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let q = (fx_to_comp(uv) - vec2<f32>(fx.p5, fx.p6)) * fx.p0;
    let n = fxl_fbm(q, fx.p2, fx.p7, fx.p1);
    // fbm gathers around 0.5; twice the spread fills 0..1 at contrast 1.
    let v = clamp((n - 0.5) * 2.0 * fx.p3 + 0.5 + fx.p4, 0.0, 1.0);
    return fxl_generate(src(uv), vec3<f32>(fxl_dec1(v)), OPT_BLEND, fx.p8);
}
