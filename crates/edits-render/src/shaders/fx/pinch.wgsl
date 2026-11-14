// pinch: pull the picture toward a point with a Gaussian falloff. A pixel
// at offset d from the centre reads the picture at d x (1 + amount x
// exp(-|d|^2 / radius^2)): positive amounts shrink the middle toward the
// point, negative ones bulge it. Unlike `bulge`, the falloff never ends.
//
// After the user's own pinch in trans4.py of the Death Note kit. Written for
// edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = amount  p1 = radius  p2, p3 = centre (composition units)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p2, fx.p3);
    let d = fx_to_comp(uv) - c;
    let f = 1.0 + fx.p0 * exp(-dot(d, d) / (fx.p1 * fx.p1));
    return src_mirror(fx_to_uv(c + d * f));
}
