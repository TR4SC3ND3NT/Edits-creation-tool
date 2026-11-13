// pixelate: flat cells, each the colour of the picture at the cell's centre.
//
// square: GPUImage Pixellate. polar: after GPUImage PolarPixellate ("@fattjake
// based on vid by toneburst"), with a whole number of segments per ring.
// Copyright (c) 2012, Brad Larson, Ben Cochran, Hugues Lismonde, Keitaroh
// Kobayashi, Alaric Cole, Matthew Clark, Jacob Gundersen, Chris Williams. All
// rights reserved. BSD-3-Clause (see THIRD_PARTY.md).
// hex: glfx.js `hexagonalPixelate`, Copyright (C) 2011 by Evan Wallace, MIT
// License (see THIRD_PARTY.md). triangle: ours, on the same skewed lattice.
// Altered: composition units, a grid centred on `center`, WGSL.
//
// p0 = cell size  p1, p2 = centre
// OPT_SHAPE: SHAPE_SQUARE, SHAPE_HEX, SHAPE_TRIANGLE or SHAPE_POLAR

const PIX_TAU: f32 = 6.2831853;
const PIX_ROW: f32 = 0.866025404;

// The centre of the cell containing `p` (both relative to the grid's centre, in cells).
fn pix_cell(p: vec2<f32>) -> vec2<f32> {
    if OPT_SHAPE == SHAPE_SQUARE {
        return floor(p + vec2<f32>(0.5));
    }
    if OPT_SHAPE == SHAPE_POLAR {
        let r = length(p);
        let ring = floor(r);
        let rc = ring + 0.5;
        let n = max(floor(PIX_TAU * rc), 1.0);
        let step = PIX_TAU / n;
        let a = (floor(atan2(p.y, p.x) / step) + 0.5) * step;
        return rc * vec2<f32>(cos(a), sin(a));
    }
    // Hexagons and triangles: a lattice skewed so that rows are 60 degrees apart.
    var t = p;
    t.y = t.y / PIX_ROW;
    t.x = t.x - t.y * 0.5;
    var choice: vec2<f32>;
    if OPT_SHAPE == SHAPE_HEX {
        var a: vec2<f32>;
        if t.x + t.y - floor(t.x) - floor(t.y) < 1.0 {
            a = floor(t);
        } else {
            a = ceil(t);
        }
        let b = vec2<f32>(ceil(t.x), floor(t.y));
        let c = vec2<f32>(floor(t.x), ceil(t.y));
        let tt = vec3<f32>(t.x, t.y, 1.0 - t.x - t.y);
        let la = length(tt - vec3<f32>(a.x, a.y, 1.0 - a.x - a.y));
        let lb = length(tt - vec3<f32>(b.x, b.y, 1.0 - b.x - b.y));
        let lc = length(tt - vec3<f32>(c.x, c.y, 1.0 - c.x - c.y));
        if la < lb {
            choice = select(c, a, la < lc);
        } else {
            choice = select(c, b, lb < lc);
        }
    } else {
        // Each rhombus of the lattice is two triangles; take the centroid of
        // the one the point is in.
        let f = floor(t);
        let lower = t.x + t.y - f.x - f.y < 1.0;
        choice = f + select(vec2<f32>(2.0 / 3.0), vec2<f32>(1.0 / 3.0), lower);
    }
    choice.x = choice.x + choice.y * 0.5;
    choice.y = choice.y * PIX_ROW;
    return choice;
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p1, fx.p2);
    let cell = pix_cell((fx_to_comp(uv) - c) / fx.p0);
    return src(fx_to_uv(c + cell * fx.p0));
}
