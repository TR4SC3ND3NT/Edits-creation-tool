// halftone: the picture as screens of ink (or, in rgb mode, of light), one
// screen per channel, each a grid of dots, lines or squares turned to its own
// angle. A cell's ink covers the same fraction of the cell as the colour at
// the cell's centre asks for (in linear light), so seen from afar the
// brightness is kept.
//
// The screen angles (15, 45, 75 degrees for rgb; 15, 45, 0, 75 for C, M, Y,
// K) and the CMYK separation follow ISF "RGB Halftone" and "CMYK Halftone"
// by zoidberg, Vidvox ISF-Files (https://github.com/Vidvox/ISF-Files),
// Copyright (c) 2018, MIT License (see THIRD_PARTY.md). Altered: ink area
// proportional to the value (the radius of a dot is solved so that the dot,
// clipped to its cell, has that area), line and square inks, a mono mode,
// composition units, WGSL. The dot area maths is ours.
//
// p0 = cell size  p1 = angle (radians)  p2 = smoothing  p3 = amount
// OPT_MODE: MODE_MONO, MODE_RGB or MODE_CMYK
// OPT_SHAPE: SHAPE_DOT, SHAPE_LINE or SHAPE_SQUARE

const HT_PI: f32 = 3.14159265;

// Area of a disc of radius r centred in a unit cell, clipped to the cell.
fn ht_disc_area(r: f32) -> f32 {
    if r <= 0.5 {
        return HT_PI * r * r;
    }
    let s = sqrt(max(r * r - 0.25, 0.0));
    return HT_PI * r * r - 4.0 * r * r * acos(0.5 / r) + 2.0 * s;
}

// The radius of a clipped disc covering `cover` of the cell (Newton's method).
fn ht_disc_radius(cover: f32) -> f32 {
    let k = clamp(cover, 0.0, 1.0);
    var r = min(sqrt(k / HT_PI), 0.70710678);
    for (var i = 0; i < 4; i++) {
        if r > 0.5 {
            let slope = 2.0 * r * (HT_PI - 4.0 * acos(0.5 / r));
            r = clamp(r - (ht_disc_area(r) - k) / max(slope, 1e-3), 0.5, 0.70710678);
        }
    }
    return r;
}

// Where a screen at angle `a` puts the cell containing `q`: its centre (in
// composition units) and the position of `q` in the cell (-0.5..0.5 cells).
struct HtCell {
    centre: vec2<f32>,
    local: vec2<f32>,
    // The screen's x axis (a unit vector).
    axis: vec2<f32>,
}

fn ht_cell(q: vec2<f32>, a: f32) -> HtCell {
    let cs = cos(a);
    let sn = sin(a);
    let g = vec2<f32>(q.x * cs + q.y * sn, -q.x * sn + q.y * cs) / fx.p0;
    let gi = floor(g) + vec2<f32>(0.5);
    let m = gi * fx.p0;
    var out: HtCell;
    out.centre = vec2<f32>(m.x * cs - m.y * sn, m.x * sn + m.y * cs);
    out.local = g - gi;
    out.axis = vec2<f32>(cs, sn);
    return out;
}

// How much of a pixel at `local` is covered by ink that should cover `cover` of the cell.
fn ht_ink(local: vec2<f32>, cover: f32) -> f32 {
    // Half the width of the soft edge, in cells: at least most of a pixel.
    let w = max(fx.p2 * 0.25, 0.75 * fx_pixel() / fx.p0);
    let k = clamp(cover, 0.0, 1.0);
    if OPT_SHAPE == SHAPE_LINE {
        let h = 0.5 * k;
        return 1.0 - smoothstep(h - w, h + w, abs(local.y));
    }
    if OPT_SHAPE == SHAPE_SQUARE {
        let h = 0.5 * sqrt(k);
        return 1.0 - smoothstep(h - w, h + w, max(abs(local.x), abs(local.y)));
    }
    let r = ht_disc_radius(k);
    return 1.0 - smoothstep(r - w, r + w, length(local));
}

// The colour a cell stands for: the average of five points of the cell (its
// centre and four points toward its corners), so that detail smaller than a
// cell is averaged rather than hit or missed.
fn ht_colour(s: HtCell) -> vec3<f32> {
    let k = 0.3 * fx.p0;
    let u = s.axis * k;
    let v = vec2<f32>(-s.axis.y, s.axis.x) * k;
    var sum = src(fx_to_uv(s.centre)) * 2.0;
    sum += src(fx_to_uv(s.centre + u + v)) + src(fx_to_uv(s.centre + u - v));
    sum += src(fx_to_uv(s.centre - u + v)) + src(fx_to_uv(s.centre - u - v));
    return max(fx_straight(sum), vec3<f32>(0.0));
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let q = fx_to_comp(uv);
    let a = fx.p1;
    let deg = 0.017453293;
    let here = src(uv);
    var rgb: vec3<f32>;
    if OPT_MODE == MODE_MONO {
        // Black ink on white paper.
        let s = ht_cell(q, a);
        let l = fx_luma(ht_colour(s));
        rgb = vec3<f32>(1.0 - ht_ink(s.local, 1.0 - l));
    } else if OPT_MODE == MODE_RGB {
        // Dots of red, green and blue light on black.
        let r = ht_cell(q, a - 30.0 * deg);
        let g = ht_cell(q, a);
        let b = ht_cell(q, a + 30.0 * deg);
        rgb = vec3<f32>(ht_ink(r.local, ht_colour(r).r), ht_ink(g.local, ht_colour(g).g), ht_ink(b.local, ht_colour(b).b));
    } else {
        // Cyan, magenta, yellow and black ink on white paper.
        let sc = ht_cell(q, a - 30.0 * deg);
        let sm = ht_cell(q, a);
        let sy = ht_cell(q, a - 45.0 * deg);
        let sk = ht_cell(q, a + 30.0 * deg);
        let cc = min(ht_colour(sc), vec3<f32>(1.0));
        let cm = min(ht_colour(sm), vec3<f32>(1.0));
        let cy = min(ht_colour(sy), vec3<f32>(1.0));
        let ck = min(ht_colour(sk), vec3<f32>(1.0));
        // Each screen separates the colour under its own dot.
        let kc = 1.0 - max(cc.r, max(cc.g, cc.b));
        let km = 1.0 - max(cm.r, max(cm.g, cm.b));
        let ky = 1.0 - max(cy.r, max(cy.g, cy.b));
        let kk = 1.0 - max(ck.r, max(ck.g, ck.b));
        let c = (1.0 - cc.r - kc) / max(1.0 - kc, 1e-5);
        let m = (1.0 - cm.g - km) / max(1.0 - km, 1e-5);
        let y = (1.0 - cy.b - ky) / max(1.0 - ky, 1e-5);
        let ink = vec4<f32>(ht_ink(sc.local, c), ht_ink(sm.local, m), ht_ink(sy.local, y), ht_ink(sk.local, kk));
        rgb = (vec3<f32>(1.0) - ink.xyz) * (1.0 - ink.w);
    }
    let out = vec4<f32>(rgb * here.a, here.a);
    return mix(here, out, fx.p3);
}
