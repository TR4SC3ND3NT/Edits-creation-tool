// lens_distortion: a radial lens model. A point at distance d from the
// centre reads the picture at d x (1 + k d^2) / scale: positive k is barrel
// distortion, negative pincushion. The edges are mirrored, so barrel
// distortion never shows a smear.
//
// Written for edits (the one-coefficient Brown-Conrady model). License: MIT OR
// Apache-2.0, as the rest of the repository.
//
// p0 = k (per composition unit squared)  p1, p2 = centre  p3 = 1 / scale

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p1, fx.p2);
    let v = fx_to_comp(uv) - c;
    return src_mirror(fx_to_uv(c + v * (1.0 + fx.p0 * dot(v, v)) * fx.p3));
}
