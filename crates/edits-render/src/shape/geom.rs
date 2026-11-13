//! The outlines of the shape geometries, as kurbo paths in the shape's own
//! units (composition heights, origin at the shape's origin, y down).
//!
//! Closed outlines start at the top (an ellipse, a polystar's first point)
//! or the top-left corner (a rect) and run clockwise on screen, as in After
//! Effects, so a `trim` draws them on from there.

use edits_core::shape::ResolvedGeometry;
use kurbo::{BezPath, Point, Vec2};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Handle length of a quarter circle drawn as one cubic, per unit radius.
const KAPPA: f64 = 0.552_284_749_830_793_4;

pub(crate) fn outline(g: &ResolvedGeometry) -> BezPath {
    match g {
        ResolvedGeometry::Rect { size, roundness } => rect(*size, *roundness),
        ResolvedGeometry::Ellipse { size } => ellipse(*size),
        ResolvedGeometry::Polystar { points, outer_radius, inner_radius, outer_roundness, inner_roundness, polygon } => {
            polystar(*points, *outer_radius, *inner_radius, *outer_roundness, *inner_roundness, *polygon)
        }
        ResolvedGeometry::Path { points, closed } => path(points, *closed),
        ResolvedGeometry::Line { from, to } => {
            let mut p = BezPath::new();
            p.move_to((from[0], from[1]));
            p.line_to((to[0], to[1]));
            p
        }
    }
}

/// A `w` x `h` rectangle centred on the origin, corners rounded by `r`
/// (at most half the shorter side).
pub(crate) fn rect([w, h]: [f64; 2], r: f64) -> BezPath {
    let (x0, y0, x1, y1) = (-w / 2.0, -h / 2.0, w / 2.0, h / 2.0);
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut p = BezPath::new();
    if r <= 0.0 {
        p.move_to((x0, y0));
        p.line_to((x1, y0));
        p.line_to((x1, y1));
        p.line_to((x0, y1));
        p.close_path();
        return p;
    }
    let k = r * KAPPA;
    p.move_to((x0 + r, y0));
    p.line_to((x1 - r, y0));
    p.curve_to((x1 - r + k, y0), (x1, y0 + r - k), (x1, y0 + r));
    p.line_to((x1, y1 - r));
    p.curve_to((x1, y1 - r + k), (x1 - r + k, y1), (x1 - r, y1));
    p.line_to((x0 + r, y1));
    p.curve_to((x0 + r - k, y1), (x0, y1 - r + k), (x0, y1 - r));
    p.line_to((x0, y0 + r));
    p.curve_to((x0, y0 + r - k), (x0 + r - k, y0), (x0 + r, y0));
    p.close_path();
    p
}

/// An ellipse inside a `w` x `h` box centred on the origin: four cubic
/// quarters from the top, clockwise.
pub(crate) fn ellipse([w, h]: [f64; 2]) -> BezPath {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let mut p = BezPath::new();
    p.move_to((0.0, -ry));
    p.curve_to((kx, -ry), (rx, -ky), (rx, 0.0));
    p.curve_to((rx, ky), (kx, ry), (0.0, ry));
    p.curve_to((-kx, ry), (-rx, ky), (-rx, 0.0));
    p.curve_to((-rx, -ky), (-kx, -ry), (0.0, -ry));
    p.close_path();
    p
}

/// A star of `n` points (outer and inner vertices alternating) or a regular
/// polygon of `n` vertices, the first pointing up. Roundness (percent)
/// gives each vertex handles along the circle through it, after the
/// polystar of Lottie / Skottie: a handle is `roundness` times the arc a
/// vertex spans divided by 4 for a polygon or 2 for a star.
pub(crate) fn polystar(n: f64, outer: f64, inner: f64, outer_round: f64, inner_round: f64, polygon: bool) -> BezPath {
    let n = n.round().clamp(3.0, 1000.0) as usize;
    let count = if polygon { n } else { 2 * n };
    let step = TAU / count as f64;
    let mut verts: Vec<(Point, Vec2)> = Vec::with_capacity(count);
    for i in 0..count {
        let outer_vertex = polygon || i % 2 == 0;
        let (r, round) = if outer_vertex { (outer, outer_round) } else { (inner, inner_round) };
        let a = -FRAC_PI_2 + step * i as f64;
        let (s, c) = a.sin_cos();
        let v = Point::new(r * c, r * s);
        // Along the circle in the direction of travel (clockwise on screen).
        let arc = if polygon { TAU * r / (n as f64 * 4.0) } else { TAU * r / (n as f64 * 2.0) };
        let handle = Vec2::new(-s, c) * (arc * round / 100.0);
        verts.push((v, handle));
    }
    let mut p = BezPath::new();
    p.move_to(verts[0].0);
    for i in 0..count {
        let (a, ha) = verts[i];
        let (b, hb) = verts[(i + 1) % count];
        if ha == Vec2::ZERO && hb == Vec2::ZERO {
            p.line_to(b);
        } else {
            p.curve_to(a + ha, b - hb, b);
        }
    }
    p.close_path();
    p
}

/// Vertices `[x, y, in_x, in_y, out_x, out_y]`, handles relative to their vertex.
pub(crate) fn path(points: &[[f64; 6]], closed: bool) -> BezPath {
    let mut p = BezPath::new();
    let Some(first) = points.first() else { return p };
    p.move_to((first[0], first[1]));
    let n = points.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let a = points[i];
        let b = points[(i + 1) % n];
        let (out, inn) = (Vec2::new(a[4], a[5]), Vec2::new(b[2], b[3]));
        let (pa, pb) = (Point::new(a[0], a[1]), Point::new(b[0], b[1]));
        if out == Vec2::ZERO && inn == Vec2::ZERO {
            p.line_to(pb);
        } else {
            p.curve_to(pa + out, pb + inn, pb);
        }
    }
    if closed {
        p.close_path();
    }
    p
}
