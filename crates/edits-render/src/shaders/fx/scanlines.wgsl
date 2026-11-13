// scanlines: dark horizontal lines, `count` over the frame's height. When the
// lines would be closer than two pixels they fade into an even darkening of
// the same mean, so the frame's brightness does not depend on its size.
//
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = lines  p1 = darkness  p2 = scroll (line spacings, 0..1)  p3 = visibility (0..1)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let w = 0.5 + 0.5 * cos(FXL_TAU * (uv.y * fx.p0 + fx.p2));
    let shade = 1.0 - fx.p1 * mix(0.5, w, fx.p3);
    return vec4<f32>(c.rgb * shade, c.a);
}
