//! Shapes to pixels: every outline is carried through the layer's matrix
//! into output pixels, rasterised to an 8-bit coverage mask by zeno (as
//! glyphs are, `text.rs`) and composited in order, premultiplied, in
//! linear light, into a frame-sized picture.
//!
//! When the layer's matrix keeps lines straight and parallel (any 2D move,
//! turn, scale or skew without perspective, the usual case) the beziers are
//! mapped exactly; under perspective they are flattened in layer space to a
//! tenth of a pixel and the points projected one by one. Either way the
//! outline is rasterised at the output resolution, so shapes are sharp at
//! any `--scale`.

use super::ops::{self, Piece};
use crate::assets;
use edits_core::shape::{Cap, FillRule, ResolvedOp, ResolvedShape};
use glam::{DMat4, DVec4};
use kurbo::{Affine, BezPath, PathEl, Point};

/// How layer points land on output pixels (x right, y down).
pub(crate) struct Projection {
    mvp: DMat4,
    w: f64,
    h: f64,
    /// Layer to pixels, when the matrix has no perspective.
    affine: Option<Affine>,
}

impl Projection {
    pub(crate) fn new(mvp: DMat4, w: u32, h: u32) -> Self {
        let mut p = Projection { mvp, w: w as f64, h: h as f64, affine: None };
        let (cx, cy) = (mvp.x_axis.w, mvp.y_axis.w);
        let w0 = mvp.w_axis.w;
        if w0 > 1e-9 && cx.abs() <= 1e-12 * w0.abs() && cy.abs() <= 1e-12 * w0.abs() {
            if let (Some(o), Some(x), Some(y)) = (p.point(Point::ZERO), p.point(Point::new(1.0, 0.0)), p.point(Point::new(0.0, 1.0))) {
                p.affine = Some(Affine::new([x.x - o.x, x.y - o.y, y.x - o.x, y.y - o.y, o.x, o.y]));
            }
        }
        p
    }

    /// A layer point in pixels, or `None` behind the camera.
    fn point(&self, q: Point) -> Option<Point> {
        let c = self.mvp * DVec4::new(q.x, q.y, 0.0, 1.0);
        (c.w > 1e-9).then(|| Point::new((c.x / c.w * 0.5 + 0.5) * self.w, (0.5 - c.y / c.w * 0.5) * self.h))
    }

    /// Output pixels per layer unit at the layer's origin (the larger axis).
    pub(crate) fn pixels_per_unit(&self) -> f64 {
        let d = 1e-3;
        match (self.point(Point::ZERO), self.point(Point::new(d, 0.0)), self.point(Point::new(0.0, d))) {
            (Some(o), Some(x), Some(y)) => {
                let s = o.distance(x).max(o.distance(y)) / d;
                if s.is_finite() && s > 0.0 {
                    s
                } else {
                    self.h
                }
            }
            _ => self.h,
        }
    }

    /// `path` (layer space) as zeno commands in pixels; `None` when part of
    /// it is behind the camera.
    fn commands(&self, path: &BezPath, tolerance: f64) -> Option<Vec<zeno::Command>> {
        let mut out = Vec::with_capacity(path.elements().len() + 1);
        let z = |p: Point| {
            let c = |v: f64| v.clamp(-1e7, 1e7) as f32;
            zeno::Point::new(c(p.x), c(p.y))
        };
        if let Some(a) = self.affine {
            for el in path.elements() {
                out.push(match *el {
                    PathEl::MoveTo(p) => zeno::Command::MoveTo(z(a * p)),
                    PathEl::LineTo(p) => zeno::Command::LineTo(z(a * p)),
                    PathEl::QuadTo(c, p) => zeno::Command::QuadTo(z(a * c), z(a * p)),
                    PathEl::CurveTo(c0, c1, p) => zeno::Command::CurveTo(z(a * c0), z(a * c1), z(a * p)),
                    PathEl::ClosePath => zeno::Command::Close,
                });
            }
            return Some(out);
        }
        let mut behind = false;
        kurbo::flatten(path.iter(), tolerance, |el| match el {
            PathEl::MoveTo(p) => match self.point(p) {
                Some(q) => out.push(zeno::Command::MoveTo(z(q))),
                None => behind = true,
            },
            PathEl::LineTo(p) => match self.point(p) {
                Some(q) => out.push(zeno::Command::LineTo(z(q))),
                None => behind = true,
            },
            PathEl::ClosePath => out.push(zeno::Command::Close),
            _ => {}
        });
        (!behind).then_some(out)
    }
}

/// A frame of premultiplied linear RGBA.
struct Canvas {
    w: u32,
    h: u32,
    px: Vec<[f32; 4]>,
    painted: bool,
}

impl Canvas {
    fn new(w: u32, h: u32) -> Self {
        Canvas { w, h, px: vec![[0.0; 4]; (w * h) as usize], painted: false }
    }

    /// Paint `colour` (premultiplied linear) through the coverage of `cmds`.
    fn paint(&mut self, cmds: &[zeno::Command], rule: zeno::Fill, colour: [f32; 4]) {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in cmds {
            let pts: &[zeno::Point] = match c {
                zeno::Command::MoveTo(p) | zeno::Command::LineTo(p) => std::slice::from_ref(p),
                zeno::Command::QuadTo(a, b) => &[*a, *b][..],
                zeno::Command::CurveTo(a, b, d) => &[*a, *b, *d][..],
                zeno::Command::Close => &[],
            };
            for p in pts {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
        }
        let bx0 = (x0.floor() - 1.0).max(0.0) as u32;
        let by0 = (y0.floor() - 1.0).max(0.0) as u32;
        let bx1 = (x1.ceil() + 1.0).min(self.w as f32).max(0.0) as u32;
        let by1 = (y1.ceil() + 1.0).min(self.h as f32).max(0.0) as u32;
        if bx1 <= bx0 || by1 <= by0 {
            return;
        }
        let (bw, bh) = (bx1 - bx0, by1 - by0);
        let (mask, _) = zeno::Mask::new(cmds).style(rule).size(bw, bh).offset(zeno::Vector::new(-(bx0 as f32), -(by0 as f32))).render();
        for row in 0..bh {
            let dst = ((by0 + row) * self.w + bx0) as usize;
            let src = (row * bw) as usize;
            for (d, &m) in self.px[dst..dst + bw as usize].iter_mut().zip(&mask[src..src + bw as usize]) {
                if m == 0 {
                    continue;
                }
                let a = m as f32 / 255.0;
                let keep = 1.0 - colour[3] * a;
                for i in 0..4 {
                    d[i] = colour[i] * a + d[i] * keep;
                }
            }
        }
        self.painted = true;
    }

    /// The picture as RGBA16Float bytes.
    fn to_f16(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 8);
        for p in &self.px {
            for v in p {
                out.extend_from_slice(&f16_bits(*v).to_le_bytes());
            }
        }
        out
    }
}

/// An IEEE half float for `v` in 0..65504 (rounded to nearest; what a
/// shape writes: premultiplied colour and coverage, 0..1).
fn f16_bits(v: f32) -> u16 {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    let v = v.min(65504.0);
    let bits = v.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let man = bits & 0x7f_ffff;
    if exp <= 0 {
        // Subnormal half.
        if exp < -10 {
            return 0;
        }
        let m = man | 0x80_0000;
        let shift = (14 - exp) as u32;
        let half = m >> shift;
        let rest = m & ((1 << shift) - 1);
        let round = (rest > (1 << (shift - 1))) || (rest == (1 << (shift - 1)) && half & 1 == 1);
        return (half + round as u32) as u16;
    }
    let half = ((exp as u32) << 10) | (man >> 13);
    let rest = man & 0x1fff;
    let round = rest > 0x1000 || (rest == 0x1000 && half & 1 == 1);
    (half + round as u32) as u16
}

fn premultiplied(c: [f64; 4], alpha: f64) -> [f32; 4] {
    let a = alpha.clamp(0.0, 1.0) as f32;
    let l = |v: f64| assets::srgb_to_linear(v.clamp(0.0, 1.0) as f32) * a;
    [l(c[0]), l(c[1]), l(c[2]), a]
}

/// The pieces of `shape` after its operators, in the shape's own units.
/// `tolerance` is the flattening tolerance there.
pub(crate) fn pieces(shape: &ResolvedShape, tolerance: f64) -> Vec<Piece> {
    let mut pieces = vec![Piece { path: super::geom::outline(&shape.geometry), opacity: 1.0 }];
    for op in &shape.operators {
        pieces = match op {
            ResolvedOp::Repeater(r) => ops::repeat(pieces, r),
            op => pieces
                .into_iter()
                .map(|p| Piece {
                    path: match op {
                        ResolvedOp::Trim { start, end, offset, individually } => ops::trim(&p.path, *start, *end, *offset, *individually),
                        ResolvedOp::RoundCorners { radius } => ops::round_corners(&p.path, *radius),
                        ResolvedOp::OffsetPath { amount, join, miter_limit } => ops::offset_path(&p.path, *amount, *join, *miter_limit, tolerance),
                        ResolvedOp::Zigzag { size, ridges, smooth } => ops::zigzag(&p.path, *size, *ridges, *smooth),
                        ResolvedOp::WigglePath { size, detail, phase, seed } => ops::wiggle(&p.path, *size, *detail, *phase, *seed),
                        ResolvedOp::Repeater(_) => unreachable!("handled above"),
                    },
                    opacity: p.opacity,
                })
                .collect(),
        };
    }
    pieces
}

/// The shapes drawn through `mvp` (layer space to clip space) into a
/// `w` x `h` RGBA16Float picture (premultiplied linear), or `None` when
/// nothing of them lands in the frame.
pub(crate) fn rasterise(shapes: &[ResolvedShape], mvp: DMat4, w: u32, h: u32) -> Option<Vec<u8>> {
    let proj = Projection::new(mvp, w, h);
    let tol_layer = (0.1 / proj.pixels_per_unit()).max(1e-8);
    let mut canvas = Canvas::new(w, h);
    for s in shapes {
        let m = Affine::translate((s.position[0], s.position[1])) * Affine::rotate(s.rotation.to_radians()) * Affine::scale_non_uniform(s.scale[0], s.scale[1]);
        let local = s.scale[0].abs().max(s.scale[1].abs());
        if local <= 1e-12 {
            continue;
        }
        let tol = tol_layer / local;
        for piece in pieces(s, tol) {
            if piece.opacity <= 0.0 || piece.path.elements().is_empty() {
                continue;
            }
            if let Some(f) = &s.fill {
                let a = f.color[3] * piece.opacity;
                if a > 0.0 {
                    if let Some(cmds) = proj.commands(&(m * piece.path.clone()), tol_layer) {
                        let rule = if f.rule == FillRule::EvenOdd { zeno::Fill::EvenOdd } else { zeno::Fill::NonZero };
                        canvas.paint(&cmds, rule, premultiplied(f.color, a));
                    }
                }
            }
            if let Some(st) = &s.stroke {
                let a = st.color[3] * piece.opacity;
                if a > 0.0 && st.width > 0.0 {
                    let cap = match st.cap {
                        Cap::Butt => kurbo::Cap::Butt,
                        Cap::Round => kurbo::Cap::Round,
                        Cap::Square => kurbo::Cap::Square,
                    };
                    let mut style = kurbo::Stroke::new(st.width).with_caps(cap).with_join(ops::kurbo_join(st.join)).with_miter_limit(st.miter_limit);
                    if !st.dash.is_empty() && st.dash.iter().sum::<f64>() > 0.0 {
                        style = style.with_dashes(st.dash_offset, st.dash.iter().copied());
                    }
                    let outline = kurbo::stroke(piece.path.iter(), &style, &kurbo::StrokeOpts::default(), tol);
                    if let Some(cmds) = proj.commands(&(m * outline), tol_layer) {
                        canvas.paint(&cmds, zeno::Fill::NonZero, premultiplied(st.color, a));
                    }
                }
            }
        }
    }
    canvas.painted.then(|| canvas.to_f16())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_round_trip_the_unit_range() {
        assert_eq!(f16_bits(0.0), 0);
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(65504.0), 0x7bff);
        // The smallest subnormal and a value between.
        assert_eq!(f16_bits(5.960_464_5e-8), 1);
        assert_eq!(f16_bits(0.333_333), 0x3555);
    }

    #[test]
    fn an_affine_matrix_maps_beziers_exactly() {
        // The frame's own matrix: x in [-aspect/2, aspect/2], y in [-0.5, 0.5] (y down).
        let mvp = DMat4::from_scale(glam::DVec3::new(2.0 * 100.0 / 200.0, -2.0, 1.0));
        let p = Projection::new(mvp, 200, 100);
        assert!(p.affine.is_some());
        assert!((p.pixels_per_unit() - 100.0).abs() < 1e-6);
        let o = p.point(Point::ZERO).unwrap();
        assert!((o.x - 100.0).abs() < 1e-9 && (o.y - 50.0).abs() < 1e-9);
    }
}
