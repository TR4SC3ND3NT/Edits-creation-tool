// gradient_ramp: two colours mixed (as display values) along the line from
// `start` to `end`, or by the distance from `start` (radial), blended over
// the picture inside its transparency.
//
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0, p1 = start  p2, p3 = end  p4..p6 = colour a  p7..p9 = colour b (display)  p10 = amount
// OPT_SHAPE: SHAPE_LINEAR or SHAPE_RADIAL   OPT_BLEND: BLEND_REPLACE .. BLEND_OVERLAY

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let p = fx_to_comp(uv);
    let a = vec2<f32>(fx.p0, fx.p1);
    let ab = vec2<f32>(fx.p2, fx.p3) - a;
    let len2 = max(dot(ab, ab), 1e-10);
    var t: f32;
    if OPT_SHAPE == SHAPE_RADIAL {
        t = length(p - a) / sqrt(len2);
    } else {
        t = dot(p - a, ab) / len2;
    }
    let d = mix(vec3<f32>(fx.p4, fx.p5, fx.p6), vec3<f32>(fx.p7, fx.p8, fx.p9), clamp(t, 0.0, 1.0));
    return fxl_generate(src(uv), fxl_dec(d), OPT_BLEND, fx.p10);
}
