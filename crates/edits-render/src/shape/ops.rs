//! Shape operators on kurbo paths: trim, repeater, round corners, offset
//! path, zigzag and wiggle path.
//!
//! The algorithms follow EffectCraft's `crates/path/src/ops.rs`
//! (https://github.com/storytold/effectcraft, MIT OR Apache-2.0, used under
//! MIT), read as descriptions and written anew here: trim intervals with a
//! wrap-around offset, cut by arc length; round corners as a cubic arc
//! between points cut at `min(radius, half of each neighbouring segment)`
//! with the handle factor `4/3 tan(phi / 4)`; a zigzag of equal arc-length
//! pieces displaced alternately along the normal (the corner bisector at a
//! vertex); a wiggle of hashed value noise per resampled point. Offset path
//! is kurbo's `expand_path`. The repeater is After Effects' (and Skottie's):
//! copy `i` gets `i` times the transform about the anchor.

use edits_core::shape::{Join, ResolvedRepeater};
use kurbo::{Affine, BezPath, CubicBez, Diagonal2, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathEl, PathSeg, Point, Vec2};
use std::f64::consts::PI;

/// Accuracy of arc lengths, in composition units.
const ACCURACY: f64 = 1e-7;

/// One copy of a shape's outline (a repeater makes several) and its opacity.
#[derive(Clone, Debug)]
pub(crate) struct Piece {
    pub path: BezPath,
    pub opacity: f64,
}

/// A subpath: its segments in order and whether it closes.
#[derive(Clone, Debug)]
pub(crate) struct Sub {
    pub segs: Vec<PathSeg>,
    pub closed: bool,
}

/// The subpaths of `p`, without zero-length segments. A closing segment
/// back to the start is made explicit.
pub(crate) fn subpaths(p: &BezPath) -> Vec<Sub> {
    let mut out: Vec<Sub> = Vec::new();
    let mut cur: Option<Sub> = None;
    let (mut start, mut last) = (Point::ZERO, Point::ZERO);
    let push = |seg: PathSeg, cur: &mut Option<Sub>| {
        if seg.arclen(ACCURACY) > 1e-12 {
            cur.get_or_insert_with(|| Sub { segs: Vec::new(), closed: false }).segs.push(seg);
        }
    };
    for el in p.elements() {
        match *el {
            PathEl::MoveTo(q) => {
                if let Some(s) = cur.take().filter(|s| !s.segs.is_empty()) {
                    out.push(s);
                }
                start = q;
                last = q;
                cur = Some(Sub { segs: Vec::new(), closed: false });
            }
            PathEl::LineTo(q) => {
                push(PathSeg::Line(kurbo::Line::new(last, q)), &mut cur);
                last = q;
            }
            PathEl::QuadTo(a, q) => {
                push(PathSeg::Quad(kurbo::QuadBez::new(last, a, q)), &mut cur);
                last = q;
            }
            PathEl::CurveTo(a, b, q) => {
                push(PathSeg::Cubic(CubicBez::new(last, a, b, q)), &mut cur);
                last = q;
            }
            PathEl::ClosePath => {
                if last.distance(start) > 1e-12 {
                    push(PathSeg::Line(kurbo::Line::new(last, start)), &mut cur);
                }
                if let Some(mut s) = cur.take().filter(|s| !s.segs.is_empty()) {
                    s.closed = true;
                    out.push(s);
                }
                last = start;
            }
        }
    }
    if let Some(s) = cur.filter(|s| !s.segs.is_empty()) {
        out.push(s);
    }
    out
}

/// Subpaths back into one path.
pub(crate) fn to_path(subs: &[Sub]) -> BezPath {
    let mut p = BezPath::new();
    for s in subs {
        let Some(first) = s.segs.first() else { continue };
        p.move_to(first.start());
        for seg in &s.segs {
            p.push(seg.as_path_el());
        }
        if s.closed {
            p.close_path();
        }
    }
    p
}

fn lengths(s: &Sub) -> Vec<f64> {
    s.segs.iter().map(|g| g.arclen(ACCURACY)).collect()
}

/// The unit tangent of `seg` at `t`.
fn tangent(seg: &PathSeg, t: f64) -> Vec2 {
    let d = match seg {
        PathSeg::Line(l) => l.p1 - l.p0,
        PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
    };
    let d = if d.hypot2() > 1e-24 {
        d
    } else {
        // A handle on its vertex: look a little further in.
        let e = 1e-4;
        let (a, b) = if t < 0.5 { (t, t + e) } else { (t - e, t) };
        seg.eval(b) - seg.eval(a)
    };
    let n = d.hypot();
    if n > 0.0 && n.is_finite() {
        d / n
    } else {
        Vec2::new(1.0, 0.0)
    }
}

/// The part of `seg` from arc length `a` to `b` (from its start).
fn cut_seg(seg: &PathSeg, len: f64, a: f64, b: f64) -> PathSeg {
    let ta = if a <= 0.0 { 0.0 } else { seg.inv_arclen(a, ACCURACY) };
    let tb = if b >= len { 1.0 } else { seg.inv_arclen(b, ACCURACY) };
    seg.subsegment(ta..tb)
}

// ---- trim ------------------------------------------------------------------------

/// The kept fractions of a trim: `None` keeps everything; an interval past
/// 1 is split in two (the second starting at 0).
fn intervals(start: f64, end: f64, offset: f64) -> Option<Vec<(f64, f64)>> {
    let (s, e) = if start <= end { (start, end) } else { (end, start) };
    let span = e - s;
    if span >= 1.0 - 1e-12 {
        return None;
    }
    if span <= 1e-12 {
        return Some(Vec::new());
    }
    let a = (s + offset).rem_euclid(1.0);
    let b = a + span;
    Some(if b <= 1.0 { vec![(a, b)] } else { vec![(a, 1.0), (0.0, b - 1.0)] })
}

/// The parts of `sub` within `ranges` (arc lengths from its start). On a
/// closed subpath, a range that runs to the end and one that starts at 0
/// are joined through the start.
fn cut(sub: &Sub, lens: &[f64], ranges: &[(f64, f64)]) -> Vec<Sub> {
    let total: f64 = lens.iter().sum();
    let mut out: Vec<Sub> = ranges
        .iter()
        .map(|&(a, b)| {
            let mut segs = Vec::new();
            let mut c0 = 0.0;
            for (seg, &len) in sub.segs.iter().zip(lens) {
                let c1 = c0 + len;
                let (x0, x1) = (a.max(c0), b.min(c1));
                if x1 - x0 > 1e-12 {
                    segs.push(cut_seg(seg, len, x0 - c0, x1 - c0));
                }
                c0 = c1;
            }
            Sub { segs, closed: false }
        })
        .collect();
    let eps = 1e-9 * total.max(1.0);
    if sub.closed && ranges.len() >= 2 && ranges[0].1 >= total - eps && ranges[ranges.len() - 1].0 <= eps {
        let tail = out.pop().expect("two ranges");
        out[0].segs.extend(tail.segs);
    }
    out.retain(|s| !s.segs.is_empty());
    out
}

/// Keep the part of each subpath from `start` to `end` (fractions of its
/// length), moved along by `offset` (fractions; wraps). With
/// `individually`, the subpaths are trimmed as one path, one after another.
pub(crate) fn trim(p: &BezPath, start: f64, end: f64, offset: f64, individually: bool) -> BezPath {
    let Some(iv) = intervals(start, end, offset) else { return p.clone() };
    if iv.is_empty() {
        return BezPath::new();
    }
    let subs = subpaths(p);
    let lens: Vec<Vec<f64>> = subs.iter().map(lengths).collect();
    let mut out = Vec::new();
    if individually {
        let total: f64 = lens.iter().flatten().sum();
        let mut o = 0.0;
        for (sub, l) in subs.iter().zip(&lens) {
            let len: f64 = l.iter().sum();
            let ranges: Vec<(f64, f64)> = iv.iter().map(|&(a, b)| ((a * total).max(o) - o, (b * total).min(o + len) - o)).filter(|(a, b)| b - a > 1e-12).collect();
            out.extend(cut(sub, l, &ranges));
            o += len;
        }
    } else {
        for (sub, l) in subs.iter().zip(&lens) {
            let len: f64 = l.iter().sum();
            let ranges: Vec<(f64, f64)> = iv.iter().map(|&(a, b)| (a * len, b * len)).collect();
            out.extend(cut(sub, l, &ranges));
        }
    }
    to_path(&out)
}

// ---- round corners -----------------------------------------------------------------

/// Round every sharp corner with an arc of radius `r` (less where a
/// neighbouring segment is shorter than `2 r`).
pub(crate) fn round_corners(p: &BezPath, r: f64) -> BezPath {
    if r <= 0.0 || !r.is_finite() {
        return p.clone();
    }
    let mut out = Vec::new();
    for sub in subpaths(p) {
        let n = sub.segs.len();
        let lens = lengths(&sub);
        let mut cut_start = vec![0.0; n];
        let mut cut_end = vec![0.0; n];
        let mut corner = vec![false; n];
        for j in 0..n {
            if j == 0 && !sub.closed {
                continue;
            }
            let prev = (j + n - 1) % n;
            if prev == j {
                continue;
            }
            let (a, b) = (tangent(&sub.segs[prev], 1.0), tangent(&sub.segs[j], 0.0));
            let phi = a.cross(b).atan2(a.dot(b)).abs();
            if phi > 1e-3 && phi < PI - 1e-3 {
                let d = r.min(lens[prev] / 2.0).min(lens[j] / 2.0);
                if d > 1e-12 {
                    cut_end[prev] = d;
                    cut_start[j] = d;
                    corner[j] = true;
                }
            }
        }
        // Where each segment is cut (parameters), and what is left of it.
        let params: Vec<(f64, f64)> = (0..n)
            .map(|j| {
                let seg = &sub.segs[j];
                let a = if cut_start[j] > 0.0 { seg.inv_arclen(cut_start[j], ACCURACY) } else { 0.0 };
                let b = if cut_end[j] > 0.0 { seg.inv_arclen(lens[j] - cut_end[j], ACCURACY) } else { 1.0 };
                (a, b.max(a))
            })
            .collect();
        let mut segs = Vec::with_capacity(2 * n);
        for j in 0..n {
            let seg = &sub.segs[j];
            if corner[j] {
                let prev = (j + n - 1) % n;
                let (p1, p2) = (sub.segs[prev].eval(params[prev].1), seg.eval(params[j].0));
                // Tangents of the original segments where they are cut.
                let (t1, t2) = (tangent(&sub.segs[prev], params[prev].1), tangent(seg, params[j].0));
                let phi = t1.cross(t2).atan2(t1.dot(t2)).abs().clamp(1e-6, PI - 1e-6);
                let radius = cut_start[j] / (phi / 2.0).tan();
                let h = 4.0 / 3.0 * (phi / 4.0).tan() * radius;
                segs.push(PathSeg::Cubic(CubicBez::new(p1, p1 + t1 * h, p2 - t2 * h, p2)));
            }
            if lens[j] - cut_start[j] - cut_end[j] > 1e-12 {
                segs.push(seg.subsegment(params[j].0..params[j].1));
            }
        }
        out.push(Sub { segs, closed: sub.closed });
    }
    to_path(&out)
}

// ---- offset path -------------------------------------------------------------------

/// Grow (positive `amount`) or shrink the filled outline.
pub(crate) fn offset_path(p: &BezPath, amount: f64, join: Join, miter_limit: f64, tolerance: f64) -> BezPath {
    if amount == 0.0 || !amount.is_finite() || p.elements().is_empty() {
        return p.clone();
    }
    // Closed subpaths only: an open one is closed back to its start.
    let mut closed = BezPath::new();
    for s in subpaths(p) {
        closed.extend(to_path(&[Sub { closed: true, ..s }]));
    }
    kurbo::expand_path(&closed, Diagonal2::new(amount, amount), kurbo_join(join), miter_limit, tolerance)
}

pub(crate) fn kurbo_join(j: Join) -> kurbo::Join {
    match j {
        Join::Miter => kurbo::Join::Miter,
        Join::Round => kurbo::Join::Round,
        Join::Bevel => kurbo::Join::Bevel,
    }
}

// ---- zigzag and wiggle -------------------------------------------------------------

/// A point on a subpath and the (unit) direction of the path there.
#[derive(Clone, Copy)]
struct Sample {
    p: Point,
    dir: Vec2,
}

/// `per_seg` points per segment at equal arc lengths, the first on the
/// segment's start vertex (where the direction is the average of the two
/// segments'), and for an open subpath its end point.
fn samples(sub: &Sub, per_seg: u32) -> Vec<Sample> {
    let lens = lengths(sub);
    let n = sub.segs.len();
    let m = per_seg.max(1);
    let mut out = Vec::with_capacity(n * m as usize + 1);
    for (j, (seg, &len)) in sub.segs.iter().zip(&lens).enumerate() {
        for k in 0..m {
            let s = len * k as f64 / m as f64;
            let t = if k == 0 { 0.0 } else { seg.inv_arclen(s, ACCURACY) };
            let mut dir = tangent(seg, t);
            if k == 0 && (sub.closed || j > 0) {
                let prev = tangent(&sub.segs[(j + n - 1) % n], 1.0);
                let avg = prev + dir;
                dir = if avg.hypot2() > 1e-12 { avg.normalize() } else { dir };
            }
            out.push(Sample { p: seg.eval(t), dir });
        }
    }
    if !sub.closed {
        if let Some(last) = sub.segs.last() {
            out.push(Sample { p: last.end(), dir: tangent(last, 1.0) });
        }
    }
    out
}

/// Samples joined by lines, or with `smooth` by cubics whose handles follow
/// each sample's direction.
fn join_samples(s: &[Sample], closed: bool, smooth: bool, out: &mut BezPath) {
    let Some(first) = s.first() else { return };
    out.move_to(first.p);
    let n = s.len();
    let count = if closed { n } else { n - 1 };
    for i in 0..count {
        let (a, b) = (s[i], s[(i + 1) % n]);
        if smooth {
            let h = a.p.distance(b.p) * 0.36;
            out.curve_to(a.p + a.dir * h, b.p - b.dir * h, b.p);
        } else {
            out.line_to(b.p);
        }
    }
    if closed {
        out.close_path();
    }
}

/// `ridges` zigzags of `size` on every segment.
pub(crate) fn zigzag(p: &BezPath, size: f64, ridges: u32, smooth: bool) -> BezPath {
    if size == 0.0 || !size.is_finite() {
        return p.clone();
    }
    let mut out = BezPath::new();
    for sub in subpaths(p) {
        let mut s = samples(&sub, ridges + 1);
        for (i, q) in s.iter_mut().enumerate() {
            let normal = Vec2::new(-q.dir.y, q.dir.x);
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            q.p += normal * size * sign;
        }
        join_samples(&s, sub.closed, smooth, &mut out);
    }
    out
}

/// Every point of `detail` per segment moved by up to `size` in x and y, by
/// smooth value noise at `phase` (seconds times speed) drawn from `seed`.
pub(crate) fn wiggle(p: &BezPath, size: f64, detail: u32, phase: f64, seed: u32) -> BezPath {
    if size == 0.0 || !size.is_finite() {
        return p.clone();
    }
    let mut out = BezPath::new();
    let mut index = 0u64;
    for sub in subpaths(p) {
        let mut s = samples(&sub, detail);
        for q in s.iter_mut() {
            q.p += Vec2::new(noise(seed, index, 0, phase), noise(seed, index, 1, phase)) * size;
            index += 1;
        }
        join_samples(&s, sub.closed, true, &mut out);
    }
    out
}

fn mix64(mut h: u64) -> u64 {
    h = h.wrapping_add(0x9E37_79B9_7F4A_7C15);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

/// A hash of the arguments in 0..1.
pub(crate) fn rand(seed: u32, a: u64, b: u64, c: u64) -> f64 {
    let h = mix64(mix64(mix64(mix64(seed as u64) ^ a) ^ b) ^ c);
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Smooth 1D value noise in -1..1 for point `i`, axis `axis`, at `x`.
fn noise(seed: u32, i: u64, axis: u64, x: f64) -> f64 {
    let x = if x.is_finite() { x } else { 0.0 };
    let k = x.floor();
    let f = x - k;
    let k = k as i64 as u64;
    let v = |n: u64| rand(seed, i, axis, n) * 2.0 - 1.0;
    let u = f * f * (3.0 - 2.0 * f);
    v(k) + (v(k.wrapping_add(1)) - v(k)) * u
}

// ---- repeater ----------------------------------------------------------------------

/// `copies` copies of every piece, copy `i` moved by `i + offset` times the
/// transform, with its opacity between the start and end opacity (and the
/// jitter, if any). The first copy comes first (drawn at the bottom).
pub(crate) fn repeat(pieces: Vec<Piece>, r: &ResolvedRepeater) -> Vec<Piece> {
    let n = r.copies;
    let mut out = Vec::with_capacity(pieces.len() * n as usize);
    let anchor = Vec2::new(r.anchor[0], r.anchor[1]);
    let about = |m: Affine| Affine::translate(anchor) * m * Affine::translate(-anchor);
    let pow = |s: f64, k: f64| {
        let flip = s < 0.0 && (k.round() as i64).rem_euclid(2) == 1;
        let v = s.abs().max(1e-9).powf(k);
        if flip {
            -v
        } else {
            v
        }
    };
    for piece in &pieces {
        for i in 0..n {
            let k = i as f64 + r.offset;
            let m = Affine::translate((r.position[0] * k, r.position[1] * k))
                * about(Affine::rotate((r.rotation * k).to_radians()) * Affine::scale_non_uniform(pow(r.scale[0], k), pow(r.scale[1], k)));
            let u = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
            let mut opacity = piece.opacity * (r.start_opacity + (r.end_opacity - r.start_opacity) * u);
            let mut j = Affine::IDENTITY;
            if let Some((jit, step)) = &r.jitter {
                let rnd = |n: u64| rand(jit.seed, i as u64, *step, n) * 2.0 - 1.0;
                let pos = Vec2::new(jit.position[0] * rnd(0), jit.position[1] * rnd(1));
                let s = (1.0 + jit.scale * rnd(3)).max(0.0);
                j = Affine::translate(pos) * about(Affine::rotate((jit.rotation * rnd(2)).to_radians()) * Affine::scale(s));
                opacity *= 1.0 - jit.opacity.clamp(0.0, 1.0) * (rnd(4) * 0.5 + 0.5);
            }
            out.push(Piece { path: (m * j) * piece.path.clone(), opacity: opacity.clamp(0.0, 1.0) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    fn length(p: &BezPath) -> f64 {
        subpaths(p).iter().flat_map(lengths).sum()
    }

    #[test]
    fn trim_keeps_the_fraction_of_the_length() {
        let circle = super::super::geom::ellipse([1.0, 1.0]);
        let full = length(&circle);
        assert!((full - PI).abs() < 1e-3, "{full}");
        for (s, e, o) in [(0.0, 0.5, 0.0), (0.2, 0.3, 0.0), (0.0, 0.25, 0.9), (0.6, 0.1, 0.0)] {
            let t = trim(&circle, s, e, o, false);
            let want = (e - s).abs() * full;
            assert!((length(&t) - want).abs() < 1e-6, "{s} {e} {o}: {} vs {want}", length(&t));
            // A wrapped range is still one piece.
            assert_eq!(subpaths(&t).len(), 1);
        }
        assert!(trim(&circle, 0.3, 0.3, 0.0, false).elements().is_empty());
        // Individually: two lines of length 1 and 3 trimmed to the first half.
        let mut two = BezPath::new();
        two.move_to((0.0, 0.0));
        two.line_to((1.0, 0.0));
        two.move_to((0.0, 1.0));
        two.line_to((3.0, 1.0));
        let t = trim(&two, 0.0, 0.5, 0.0, true);
        assert!((length(&t) - 2.0).abs() < 1e-9);
        let t = trim(&two, 0.0, 0.5, 0.0, false);
        assert!((length(&t) - 2.0).abs() < 1e-9);
        assert_eq!(subpaths(&t).len(), 2);
    }

    #[test]
    fn round_corners_of_a_square_take_four_minus_pi_r_squared() {
        let sq = super::super::geom::rect([1.0, 1.0], 0.0);
        for r in [0.05, 0.2, 0.5, 0.8] {
            let rounded = round_corners(&sq, r);
            let rr: f64 = r.min(0.5);
            let want = 1.0 - (4.0 - PI) * rr * rr;
            assert!((rounded.area().abs() - want).abs() < 1e-3, "r {r}: {} vs {want}", rounded.area());
        }
        // The same as a rect with that roundness.
        let a = round_corners(&sq, 0.1).area().abs();
        let b = super::super::geom::rect([1.0, 1.0], 0.1).area().abs();
        assert!((a - b).abs() < 1e-6);
    }

    #[test]
    fn repeater_makes_copies_and_offset_grows_the_area() {
        let r = ResolvedRepeater { copies: 5, offset: 0.0, anchor: [0.0, 0.0], position: [0.1, 0.0], rotation: 0.0, scale: [1.0, 1.0], start_opacity: 1.0, end_opacity: 0.0, jitter: None };
        let out = repeat(vec![Piece { path: super::super::geom::rect([0.05, 0.05], 0.0), opacity: 1.0 }], &r);
        assert_eq!(out.len(), 5);
        assert!((out[4].path.bounding_box().center().x - 0.4).abs() < 1e-9);
        assert!((out[2].opacity - 0.5).abs() < 1e-9 && out[4].opacity == 0.0);
        let circle = super::super::geom::ellipse([1.0, 1.0]);
        let grown = offset_path(&circle, 0.1, Join::Round, 4.0, 1e-4).area().abs();
        assert!((grown - PI * 0.36).abs() < 5e-3, "{grown}");
        let shrunk = offset_path(&circle, -0.1, Join::Round, 4.0, 1e-4).area().abs();
        assert!((shrunk - PI * 0.16).abs() < 5e-3, "{shrunk}");
    }

    #[test]
    fn zigzag_and_wiggle_are_bounded_and_repeatable() {
        let mut line = BezPath::new();
        line.move_to((0.0, 0.0));
        line.line_to((1.0, 0.0));
        let z = zigzag(&line, 0.1, 4, false);
        let ys: Vec<f64> = z.elements().iter().filter_map(|e| e.end_point()).map(|p| p.y).collect();
        assert_eq!(ys.len(), 6);
        assert!(ys.iter().all(|y| (y.abs() - 0.1).abs() < 1e-9), "{ys:?}");
        let a = wiggle(&line, 0.05, 8, 1.3, 7);
        assert_eq!(a, wiggle(&line, 0.05, 8, 1.3, 7));
        assert_ne!(a, wiggle(&line, 0.05, 8, 1.3, 8));
        assert!(a.bounding_box().height() <= 0.2);
    }
}
