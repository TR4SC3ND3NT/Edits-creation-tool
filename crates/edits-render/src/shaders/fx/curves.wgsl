// curves: tone curves on display values. The packer bakes, for x in 0..1,
// each channel's curve after the master curve into one table row (red in .r,
// green in .g, blue in .b); a value outside 0..1 continues the curve's end
// with slope 1, so values above 1 keep their excess.
//
// Written for edits from the documented method of glfx.js curves
// (https://github.com/evanw/glfx.js, src/filters/adjust/curves.js; Copyright
// (c) 2011 Evan Wallace, MIT License) and EffectCraft's Curves
// (https://github.com/storytold/effectcraft, crates/effects/src/color2.rs;
// MIT OR Apache-2.0): a natural cubic spline through the control points,
// sampled into a table on the CPU (see fxlib/color.rs). License: MIT OR
// Apache-2.0, as the rest of the repository.
//
// p0 = amount

// The curve's value at x (`inside`, read from the table), continued past its
// ends `lo` (at 0) and `hi` (at 1) with slope 1.
fn cv_channel(x: f32, inside: f32, lo: f32, hi: f32) -> f32 {
    if x < 0.0 {
        return lo + x;
    }
    if x > 1.0 {
        return hi + x - 1.0;
    }
    return inside;
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    let lo = fxl_table(0.0, 0u);
    let hi = fxl_table(1.0, 0u);
    let o = vec3<f32>(
        cv_channel(d.r, fxl_table(d.r, 0u).r, lo.r, hi.r),
        cv_channel(d.g, fxl_table(d.g, 0u).g, lo.g, hi.g),
        cv_channel(d.b, fxl_table(d.b, 0u).b, lo.b, hi.b),
    );
    return fxl_from_display(mix(d, o, fx.p0), c.a);
}
