// drip: ragged paint edges and drips on a layer's shapes (made for text).
// The picture is read through a small noise displacement (`roughness`,
// features `scale` wide), which roughens every edge. Then the frame is cut
// into columns 2.5 drip widths wide; a share `drips` of them holds a drip at
// a seeded place: below an opaque part of the (roughened) layer, paint runs
// down for up to `length`, narrowing from `width` to 40% of it, and ends in
// a round drop. The drip takes the colour of the paint it runs from.
//
// After the measured "thats right.." strokes with drips (text4.py of the
// user's Death Note kit), as a procedural style instead of fixed strokes.
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = roughness  p1 = scale  p2 = share of columns  p3 = longest drip
// (already times grow)  p4 = drip width  p5 = seed  (lengths in composition units)

const DR_STEPS: i32 = 24;

fn dr_read(p: vec2<f32>) -> vec4<f32> {
    var q = p;
    if fx.p0 > 0.0 {
        let f = 1.0 / fx.p1;
        q = p + fx.p0 * 2.0 * (vec2<f32>(fx_noise(p * f, fx.p5), fx_noise(p * f + vec2<f32>(31.7, 11.3), fx.p5 + 1.0)) - vec2<f32>(0.5));
    }
    return src(fx_to_uv(q));
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let p = fx_to_comp(uv);
    let base = dr_read(p);
    if fx.p2 <= 0.0 || fx.p3 <= 0.0 || base.a >= 0.999 {
        return base;
    }
    let spacing = fx.p4 * 2.5;
    let col = floor(p.x / spacing);
    let sd = bitcast<u32>(i32(fx.p5));
    let ci = bitcast<u32>(i32(col));
    if fx_hash(ci, sd, 101u) >= fx.p2 {
        return base;
    }
    let xc = (col + 0.3 + 0.4 * fx_hash(ci, sd, 102u)) * spacing;
    let len = fx.p3 * (0.3 + 0.7 * fx_hash(ci, sd, 103u));
    let half_w = fx.p4 * 0.5;
    if abs(p.x - xc) > half_w + fx_pixel() {
        return base;
    }
    // The nearest opaque paint above, in the column's middle.
    var d = -1.0;
    var paint = vec4<f32>(0.0);
    for (var k = 1; k <= DR_STEPS; k++) {
        let y = p.y - len * f32(k) / f32(DR_STEPS);
        let s = dr_read(vec2<f32>(xc, y));
        if s.a > 0.5 {
            d = len * f32(k) / f32(DR_STEPS);
            paint = s;
            break;
        }
    }
    if d < 0.0 {
        return base;
    }
    let px = fx_pixel();
    let bulb = half_w * 0.75;
    var cov = 0.0;
    if d <= len - bulb {
        let w = half_w * (1.0 - 0.6 * d / len);
        cov = 1.0 - smoothstep(w - px * 0.5, w + px * 0.5, abs(p.x - xc));
    }
    let to_drop = length(vec2<f32>(p.x - xc, d - (len - bulb)));
    cov = max(cov, 1.0 - smoothstep(bulb - px * 0.5, bulb + px * 0.5, to_drop));
    let drip = vec4<f32>(fx_straight(paint), 1.0) * cov;
    return base + (1.0 - base.a) * drip;
}
