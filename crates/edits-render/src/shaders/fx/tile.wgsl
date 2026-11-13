// tile: the picture repeated in a grid of smaller copies, one of them centred
// on the frame (moved by the offset); neighbours mirrored when asked.
//
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0, p1 = copies across and down  p2 = mirror (0 or 1)  p3, p4 = offset (uv)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let w = (uv - vec2<f32>(0.5) - vec2<f32>(fx.p3, fx.p4)) * vec2<f32>(fx.p0, fx.p1) + vec2<f32>(0.5);
    let k = floor(w);
    var f = w - k;
    if fx.p2 > 0.5 {
        let odd = abs(k - 2.0 * floor(k * 0.5)) > vec2<f32>(0.5);
        f = select(f, vec2<f32>(1.0) - f, odd);
    }
    return src(f);
}
