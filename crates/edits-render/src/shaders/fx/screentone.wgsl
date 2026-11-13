// screentone: manga screentone. The display luminance is quantised to a few
// tones between ink and paper, dithered by a pattern laid on a turned grid:
// round dots that grow into each other, an 8x8 Bayer matrix, or lines.
//
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = cell size  p1 = tones  p2, p3 = cos, sin of the screen angle  p4 = amount
// p5..p7 = ink (display)  p8..p10 = paper (display)
// OPT_PATTERN: PATTERN_DOTS, PATTERN_BAYER or PATTERN_LINES

// Index of (x, y) in the 8x8 Bayer matrix, 0..63.
fn st_bayer(x: u32, y: u32) -> f32 {
    var v = 0u;
    for (var i = 0u; i < 3u; i++) {
        let bx = (x >> i) & 1u;
        let by = (y >> i) & 1u;
        v = v | ((((bx ^ by) << 1u) | by) << (2u * (2u - i)));
    }
    return f32(v);
}

// The dither threshold at grid position `q` (cells), in 0..1.
fn st_threshold(q: vec2<f32>) -> f32 {
    if OPT_PATTERN == PATTERN_BAYER {
        let c = vec2<i32>(floor(q));
        return (st_bayer(u32(c.x & 7), u32(c.y & 7)) + 0.5) / 64.0;
    }
    var s: f32;
    if OPT_PATTERN == PATTERN_LINES {
        s = 0.5 + 0.5 * cos(FXL_TAU * q.y);
    } else {
        s = 0.5 + 0.25 * (cos(FXL_TAU * q.x) + cos(FXL_TAU * q.y));
    }
    // Ink where the tone is below `s`: high at the dot centres.
    return clamp(1.0 - s, 0.0, 0.9999);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let p = fx_to_comp(uv) / fx.p0;
    let q = vec2<f32>(p.x * fx.p2 + p.y * fx.p3, -p.x * fx.p3 + p.y * fx.p2);
    let n = fx.p1 - 1.0;
    let l = clamp(fxl_display_luma(c), 0.0, 1.0);
    let v = clamp(floor(l * n + st_threshold(q)), 0.0, n) / n;
    let toned = fxl_from_display(mix(vec3<f32>(fx.p5, fx.p6, fx.p7), vec3<f32>(fx.p8, fx.p9, fx.p10), v), c.a);
    return mix(c, toned, fx.p4);
}
