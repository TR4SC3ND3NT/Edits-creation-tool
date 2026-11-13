// shockwave: a ring expanding from a centre; inside the ring the picture is
// read nearer the centre, so it bulges outward as the ring passes.
//
// Written for edits (the look of pmndrs ShockWaveEffect and ISF Shockwave
// Pulse, no code taken). License: MIT OR Apache-2.0, as the rest of the
// repository.
//
// p0 = ring radius  p1 = half the ring's width  p2 = push (faded)  p3, p4 = centre

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p3, fx.p4);
    let v = fx_to_comp(uv) - c;
    let d = length(v);
    let x = (d - fx.p0) / fx.p1;
    if abs(x) >= 1.0 || d <= 0.0 {
        return src(uv);
    }
    let bump = cos(x * FXL_PI * 0.5);
    let r = max(d - fx.p2 * bump * bump, 0.0);
    return src(fx_to_uv(c + v * (r / d)));
}
