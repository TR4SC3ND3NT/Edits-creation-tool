// grain_highlights: Gaussian grey grain only on the light parts of the
// picture. The grain's strength is `amount` (display values, one standard
// deviation) times smoothstep(low, high, display luminance), so black stays
// clean. New on every output frame, a hash of the cell, the frame and the seed.
//
// After post4 of the user's make_edit4.py (Death Note kit). Written for
// edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = amount  p1 = cell (composition units)  p2 = low  p3 = high  p4 = seed

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    if c.a <= 0.0 {
        return c;
    }
    let cell = floor(fx_to_comp(uv) / fx.p1);
    let z = u32(fx.frame) * 2654435761u + bitcast<u32>(i32(fx.p4));
    let u1 = max(fx_hash(bitcast<u32>(i32(cell.x)), bitcast<u32>(i32(cell.y)), z), 1e-7);
    let u2 = fx_hash(bitcast<u32>(i32(cell.x)) + 9137u, bitcast<u32>(i32(cell.y)), z + 77u);
    let g = sqrt(-2.0 * log(u1)) * cos(FXL_TAU * u2);
    let d = fxl_display(c);
    let k = smoothstep(fx.p2, fx.p3, fxl_display_luma(c));
    return fxl_from_display(max(d + vec3<f32>(g * fx.p0 * k), vec3<f32>(0.0)), c.a);
}
