// Glyphs of a text layer (appended to common.wgsl). Each draw is one glyph
// quad placed by `u.mvp` (vs_geo, no margin): the cell of the glyph in the
// atlas `t0`, whose red channel is the coverage of the letter and green the
// coverage of its outline (stroke and letter together).
//
// p[0] = atlas cell: offset.xy, size.zw (uv); p[1] = premultiplied linear
// colour; p[2] = which channel to read ((1, 0, 0, 0) fill, (0, 1, 0, 0) stroke).
// The pipeline blends premultiplied "over", so later glyphs cover earlier ones.

@fragment
fn fs_glyph(in: VOut) -> @location(0) vec4<f32> {
    let uv = u.p[0].xy + clamp(in.uv, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0)) * u.p[0].zw;
    let coverage = dot(textureSampleLevel(t0, samp, uv, 0.0), u.p[2]);
    return u.p[1] * coverage;
}
