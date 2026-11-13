// levels: per channel on display values, in_black..in_white is stretched to
// 0..1 (clamped), bent by 1 / gamma, and mapped to out_black..out_white.
//
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = in_black  p1 = in_white  p2 = 1 / gamma  p3 = out_black  p4 = out_white

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let x = clamp((fxl_display(c) - vec3<f32>(fx.p0)) / (fx.p1 - fx.p0), vec3<f32>(0.0), vec3<f32>(1.0));
    let y = pow(x, vec3<f32>(fx.p2));
    return fxl_from_display(vec3<f32>(fx.p3) + y * (fx.p4 - fx.p3), c.a);
}
