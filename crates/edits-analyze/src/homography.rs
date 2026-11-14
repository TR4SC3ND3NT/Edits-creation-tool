//! Plane homographies: estimation from point pairs (normalised DLT inside a
//! seeded RANSAC), refinement on pixels (ECC-style Gauss-Newton with a gain
//! and an offset, robust weights, coarse to fine), and the normalised
//! cross-correlation check.
//!
//! Coordinates are pixel centres (pixel i is at i). `H` maps a point of the
//! picture (full-size picture pixels) to a point of the frame.

use crate::img::{gaussian_blur, half, Gray};
use edits_core::pin::{apply, det, inverse, mul, Mat3};
use rayon::prelude::*;

/// Gaussian elimination with partial pivoting on an n x n system.
pub fn solve(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| a[i * n + c].abs().partial_cmp(&a[j * n + c].abs()).unwrap())?;
        if a[p * n + c].abs() < 1e-14 {
            return None;
        }
        if p != c {
            for k in 0..n {
                a.swap(p * n + k, c * n + k);
            }
            b.swap(p, c);
        }
        for r in c + 1..n {
            let f = a[r * n + c] / a[c * n + c];
            if f == 0.0 {
                continue;
            }
            for k in c..n {
                a[r * n + k] -= f * a[c * n + k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let s: f64 = (r + 1..n).map(|k| a[r * n + k] * x[k]).sum();
        x[r] = (b[r] - s) / a[r * n + r];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Similarity that centres points and scales their mean distance to sqrt 2 (Hartley).
fn normaliser(p: &[[f64; 2]]) -> Mat3 {
    let n = p.len().max(1) as f64;
    let (cx, cy) = (p.iter().map(|q| q[0]).sum::<f64>() / n, p.iter().map(|q| q[1]).sum::<f64>() / n);
    let d = p.iter().map(|q| (q[0] - cx).hypot(q[1] - cy)).sum::<f64>() / n;
    let s = if d > 1e-12 { std::f64::consts::SQRT_2 / d } else { 1.0 };
    [[s, 0.0, -s * cx], [0.0, s, -s * cy], [0.0, 0.0, 1.0]]
}

/// Least-squares homography (h22 = 1) from 4 or more pairs `src -> dst`.
pub fn fit(src: &[[f64; 2]], dst: &[[f64; 2]]) -> Option<Mat3> {
    if src.len() < 4 || src.len() != dst.len() {
        return None;
    }
    let (ts, td) = (normaliser(src), normaliser(dst));
    let mut ata = vec![0.0; 64];
    let mut atb = vec![0.0; 8];
    for (s, d) in src.iter().zip(dst) {
        let [x, y] = apply(&ts, s[0], s[1])?;
        let [u, v] = apply(&td, d[0], d[1])?;
        for (row, rhs) in [([x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y], u), ([0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y], v)] {
            for i in 0..8 {
                atb[i] += row[i] * rhs;
                for j in 0..8 {
                    ata[i * 8 + j] += row[i] * row[j];
                }
            }
        }
    }
    let h = solve(ata, atb, 8)?;
    let hn = [[h[0], h[1], h[2]], [h[3], h[4], h[5]], [h[6], h[7], 1.0]];
    let m = mul(&inverse(&td)?, &mul(&hn, &ts));
    normalise(m)
}

/// Scale so that m[2][2] = 1 (or the largest entry is 1 when that is ~0).
pub fn normalise(m: Mat3) -> Option<Mat3> {
    let s = if m[2][2].abs() > 1e-12 { m[2][2] } else { m.iter().flatten().fold(0.0f64, |a, b| if b.abs() > a.abs() { *b } else { a }) };
    if s.abs() < 1e-300 || !s.is_finite() {
        return None;
    }
    let r = m.map(|row| row.map(|v| v / s));
    r.iter().flatten().all(|v| v.is_finite()).then_some(r)
}

/// Deterministic xorshift for RANSAC samples.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn reproj2(h: &Mat3, s: [f64; 2], d: [f64; 2]) -> f64 {
    match apply(h, s[0], s[1]) {
        Some(p) => (p[0] - d[0]).powi(2) + (p[1] - d[1]).powi(2),
        None => f64::MAX,
    }
}

/// RANSAC homography from pairs, threshold `thr` pixels in the destination.
/// Returns the model refitted on its inliers and the inlier flags.
pub fn ransac(src: &[[f64; 2]], dst: &[[f64; 2]], thr: f64, iters: usize, seed: u64) -> Option<(Mat3, Vec<bool>)> {
    let n = src.len();
    if n < 4 {
        return None;
    }
    let mut rng = Rng(seed.max(1) ^ 0x9E37_79B9_7F4A_7C15);
    let thr2 = thr * thr;
    let (mut best, mut best_n) = (None, 0usize);
    let mut max_it = iters;
    let mut it = 0;
    while it < max_it {
        it += 1;
        let mut idx = [0usize; 4];
        for k in 0..4 {
            loop {
                let c = rng.below(n);
                if !idx[..k].contains(&c) {
                    idx[k] = c;
                    break;
                }
            }
        }
        let s: Vec<[f64; 2]> = idx.iter().map(|&i| src[i]).collect();
        let d: Vec<[f64; 2]> = idx.iter().map(|&i| dst[i]).collect();
        let Some(h) = fit(&s, &d) else { continue };
        // A plane seen from in front keeps the orientation of the sample.
        if det(&h) <= 0.0 && det(&h).abs() > 0.0 && orientation_flips(&h, &s) {
            continue;
        }
        let cnt = (0..n).filter(|&i| reproj2(&h, src[i], dst[i]) < thr2).count();
        if cnt > best_n {
            best_n = cnt;
            best = Some(h);
            let w = cnt as f64 / n as f64;
            let need = ((1.0 - 0.999f64).ln() / (1.0 - w.powi(4)).max(1e-12).ln()).ceil();
            if need.is_finite() && need > 0.0 {
                max_it = max_it.min(need as usize + 20);
            }
        }
    }
    let mut h = best?;
    let mut inl = vec![false; n];
    for _ in 0..3 {
        for i in 0..n {
            inl[i] = reproj2(&h, src[i], dst[i]) < thr2;
        }
        let (s, d): (Vec<_>, Vec<_>) = (0..n).filter(|&i| inl[i]).map(|i| (src[i], dst[i])).unzip();
        match fit(&s, &d) {
            Some(r) => h = r,
            None => break,
        }
    }
    for i in 0..n {
        inl[i] = reproj2(&h, src[i], dst[i]) < thr2;
    }
    Some((h, inl))
}

fn orientation_flips(h: &Mat3, s: &[[f64; 2]]) -> bool {
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let m: Vec<[f64; 2]> = s.iter().filter_map(|p| apply(h, p[0], p[1])).collect();
    if m.len() < 3 {
        return true;
    }
    cross(s[0], s[1], s[2]).signum() != cross(m[0], m[1], m[2]).signum()
}

/// A picture prepared for pixel refinement: a Gaussian pyramid with
/// gradients. Level 0 is the picture at `scale0` of its full size.
pub struct ArtPyramid {
    pub full_w: usize,
    pub full_h: usize,
    pub scale0: f64,
    levels: Vec<(Gray, Vec<f32>, Vec<f32>, f64)>,
}

fn grads(g: &Gray) -> (Vec<f32>, Vec<f32>) {
    let (w, h) = (g.w, g.h);
    let mut gx = vec![0.0; w * h];
    let mut gy = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            gx[y * w + x] = 0.5 * (g.at((x + 1).min(w - 1), y) - g.at(x.saturating_sub(1), y));
            gy[y * w + x] = 0.5 * (g.at(x, (y + 1).min(h - 1)) - g.at(x, y.saturating_sub(1)));
        }
    }
    (gx, gy)
}

impl ArtPyramid {
    /// `img` is the picture already reduced to `scale0` of its full size.
    pub fn new(img: &Gray, full_w: usize, full_h: usize, scale0: f64) -> Self {
        let mut levels = Vec::new();
        let mut cur = gaussian_blur(img, 0.6);
        let mut s = scale0;
        loop {
            let (gx, gy) = grads(&cur);
            let next_ok = cur.w >= 64 && cur.h >= 64 && levels.len() < 9;
            levels.push((cur.clone(), gx, gy, s));
            if !next_ok {
                break;
            }
            let nxt = half(&gaussian_blur(&cur, 0.8));
            s *= nxt.w as f64 / cur.w as f64;
            cur = nxt;
        }
        Self { full_w, full_h, scale0, levels }
    }
}

#[inline]
fn bil(d: &[f32], w: usize, x0: usize, y0: usize, fx: f32, fy: f32) -> f32 {
    let i = y0 * w + x0;
    let a = d[i] + (d[i + 1] - d[i]) * fx;
    let b = d[i + w] + (d[i + w + 1] - d[i + w]) * fx;
    a + (b - a) * fy
}

/// The frame to refine against: blurred copies and a mask of usable pixels.
pub struct FrameStack {
    pub w: usize,
    pub h: usize,
    /// (sigma, blurred frame), coarse to fine.
    pub stages: Vec<(f32, Gray)>,
    pub mask: Vec<bool>,
}

impl FrameStack {
    pub fn new(g: &Gray, mask: Vec<bool>) -> Self {
        let stages = [3.0f32, 1.5, 0.7].iter().map(|s| (*s, gaussian_blur(g, *s))).collect();
        Self { w: g.w, h: g.h, stages, mask }
    }
}

/// Picture pixels per frame pixel around the centre of the picture's footprint.
fn local_scale(h: &Mat3, art: &ArtPyramid) -> Option<f64> {
    let c = apply(h, art.full_w as f64 * 0.5, art.full_h as f64 * 0.5)?;
    let inv = inverse(h)?;
    let p0 = apply(&inv, c[0], c[1])?;
    let px = apply(&inv, c[0] + 1.0, c[1])?;
    let py = apply(&inv, c[0], c[1] + 1.0)?;
    let a = ((px[0] - p0[0]) * (py[1] - p0[1]) - (px[1] - p0[1]) * (py[0] - p0[0])).abs();
    Some(a.sqrt())
}

/// The frame's bounding box (clamped) of the picture's footprint under `h`.
fn footprint(h: &Mat3, art: &ArtPyramid, w: usize, hh: usize) -> Option<(usize, usize, usize, usize)> {
    let (aw, ah) = (art.full_w as f64, art.full_h as f64);
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for (x, y) in [(0.0, 0.0), (aw, 0.0), (aw, ah), (0.0, ah)] {
        let p = apply(h, x - 0.5, y - 0.5)?;
        xs.push(p[0]);
        ys.push(p[1]);
    }
    let x0 = xs.iter().cloned().fold(f64::MAX, f64::min).floor().max(0.0) as usize;
    let x1 = (xs.iter().cloned().fold(f64::MIN, f64::max).ceil().min(w as f64 - 1.0)).max(0.0) as usize;
    let y0 = ys.iter().cloned().fold(f64::MAX, f64::min).floor().max(0.0) as usize;
    let y1 = (ys.iter().cloned().fold(f64::MIN, f64::max).ceil().min(hh as f64 - 1.0)).max(0.0) as usize;
    (x1 > x0 + 4 && y1 > y0 + 4).then_some((x0, y0, x1, y1))
}

/// Result of a pixel refinement.
#[derive(Clone, Copy, Debug)]
pub struct Refined {
    pub h: Mat3,
    /// Zero-mean normalised correlation on the finest stage, -1..1.
    pub ncc: f64,
    /// Fraction of the frame covered by usable picture pixels.
    pub coverage: f64,
}

struct Norms {
    tf: Mat3,
    tf_inv: Mat3,
    ta: Mat3,
    ta_inv: Mat3,
    sa: f64,
}

fn norms(fw: usize, fh: usize, art: &ArtPyramid) -> Norms {
    let sf = fh as f64 * 0.5;
    let tf = [[1.0 / sf, 0.0, -(fw as f64 - 1.0) * 0.5 / sf], [0.0, 1.0 / sf, -(fh as f64 - 1.0) * 0.5 / sf], [0.0, 0.0, 1.0]];
    let sa = art.full_h as f64 * 0.5;
    let ta = [[1.0 / sa, 0.0, -(art.full_w as f64 - 1.0) * 0.5 / sa], [0.0, 1.0 / sa, -(art.full_h as f64 - 1.0) * 0.5 / sa], [0.0, 0.0, 1.0]];
    Norms { tf_inv: inverse(&tf).unwrap(), tf, ta_inv: inverse(&ta).unwrap(), ta, sa }
}

/// Correlation of the picture under `h` with the frame stage `stage`, over
/// usable pixels (every `stride`-th). (ncc, number of pixels).
pub fn ncc(h: &Mat3, art: &ArtPyramid, fr: &FrameStack, stage: usize, stride: usize) -> (f64, usize) {
    let Some(inv) = inverse(h) else { return (-1.0, 0) };
    let Some((x0, y0, x1, y1)) = footprint(h, art, fr.w, fr.h) else { return (-1.0, 0) };
    let (sig, img) = (&fr.stages[stage].0, &fr.stages[stage].1);
    let lvl = level_for(h, art, *sig);
    let (a, _, _, ls) = &art.levels[lvl];
    let mut s = [0.0f64; 5];
    let mut n = 0usize;
    for y in (y0..=y1).step_by(stride) {
        for x in (x0..=x1).step_by(stride) {
            if !fr.mask[y * fr.w + x] {
                continue;
            }
            let Some(p) = apply(&inv, x as f64, y as f64) else { continue };
            let (lx, ly) = ((p[0] + 0.5) * ls - 0.5, (p[1] + 0.5) * ls - 0.5);
            if !(lx >= 1.0 && ly >= 1.0 && lx < (a.w - 2) as f64 && ly < (a.h - 2) as f64) {
                continue;
            }
            let (xi, yi) = (lx as usize, ly as usize);
            let av = bil(&a.d, a.w, xi, yi, (lx - xi as f64) as f32, (ly - yi as f64) as f32) as f64;
            let fv = img.d[y * fr.w + x] as f64;
            s[0] += av;
            s[1] += fv;
            s[2] += av * av;
            s[3] += fv * fv;
            s[4] += av * fv;
            n += 1;
        }
    }
    if n < 50 {
        return (-1.0, n);
    }
    let nf = n as f64;
    let va = s[2] / nf - (s[0] / nf).powi(2);
    let vf = s[3] / nf - (s[1] / nf).powi(2);
    let cov = s[4] / nf - s[0] * s[1] / (nf * nf);
    if va < 1e-8 || vf < 1e-8 {
        return (0.0, n);
    }
    (cov / (va * vf).sqrt(), n)
}

fn level_for(h: &Mat3, art: &ArtPyramid, sigma: f32) -> usize {
    let m = local_scale(h, art).unwrap_or(1.0) * art.scale0;
    let k = (m * sigma as f64 / 0.8).max(1.0).log2().round() as usize;
    k.min(art.levels.len() - 1)
}

/// Refine `h` on pixels: per stage (coarse to fine), Gauss-Newton on the 8
/// homography parameters plus a gain and an offset, with Huber weights so
/// that occluders and overlays count less.
pub fn refine(h0: &Mat3, art: &ArtPyramid, fr: &FrameStack, iters: usize) -> Option<Refined> {
    let nm = norms(fr.w, fr.h, art);
    // g: normalised frame -> normalised picture.
    let mut g = normalise(mul(&nm.ta, &mul(&inverse(h0)?, &nm.tf_inv)))?;
    let mut gain = 1.0f64;
    let mut off = 0.0f64;
    for (si, (sig, img)) in fr.stages.iter().enumerate() {
        let stride = if si + 1 == fr.stages.len() { 2 } else { 3 };
        let mut have_gain = si > 0;
        for _ in 0..iters {
            let hcur = mul(&nm.tf_inv, &mul(&inverse(&g)?, &nm.ta));
            let hcur = normalise(hcur)?;
            let (x0, y0, x1, y1) = footprint(&hcur, art, fr.w, fr.h)?;
            let lvl = level_for(&hcur, art, *sig);
            let (a, gx, gy, ls) = &art.levels[lvl];
            let dl = nm.sa * ls; // level pixels per normalised picture unit
            if !have_gain {
                // Start the photometric part from the moments of the overlap.
                let (c, n) = moments(&g, &nm, a, *ls, img, &fr.mask, fr.w, (x0, y0, x1, y1), stride);
                if n < 50 {
                    return None;
                }
                gain = if c[2] > 1e-8 { (c[3] / c[2]).sqrt() } else { 1.0 };
                off = c[1] - gain * c[0];
                have_gain = true;
            }
            let rows: Vec<usize> = (y0..=y1).step_by(stride).collect();
            let acc = rows
                .par_iter()
                .fold(
                    || (vec![0.0f64; 100], vec![0.0f64; 10], 0usize, 0.0f64),
                    |mut st, &y| {
                        for x in (x0..=x1).step_by(stride) {
                            if !fr.mask[y * fr.w + x] {
                                continue;
                            }
                            let (fxn, fyn) = (nm.tf[0][0] * x as f64 + nm.tf[0][2], nm.tf[1][1] * y as f64 + nm.tf[1][2]);
                            let den = g[2][0] * fxn + g[2][1] * fyn + 1.0;
                            if den.abs() < 1e-9 {
                                continue;
                            }
                            let ax = (g[0][0] * fxn + g[0][1] * fyn + g[0][2]) / den;
                            let ay = (g[1][0] * fxn + g[1][1] * fyn + g[1][2]) / den;
                            // Normalised picture -> level pixels.
                            let pfx = nm.ta_inv[0][0] * ax + nm.ta_inv[0][2];
                            let pfy = nm.ta_inv[1][1] * ay + nm.ta_inv[1][2];
                            let (lx, ly) = ((pfx + 0.5) * ls - 0.5, (pfy + 0.5) * ls - 0.5);
                            if !(lx >= 1.0 && ly >= 1.0 && lx < (a.w - 2) as f64 && ly < (a.h - 2) as f64) {
                                continue;
                            }
                            let (xi, yi) = (lx as usize, ly as usize);
                            let (tx, ty) = ((lx - xi as f64) as f32, (ly - yi as f64) as f32);
                            let av = bil(&a.d, a.w, xi, yi, tx, ty) as f64;
                            let ix = bil(gx, a.w, xi, yi, tx, ty) as f64 * dl;
                            let iy = bil(gy, a.w, xi, yi, tx, ty) as f64 * dl;
                            let r = img.d[y * fr.w + x] as f64 - (gain * av + off);
                            let k = 0.06;
                            let wgt = if r.abs() < k { 1.0 } else { k / r.abs() };
                            let j = [
                                gain * ix * fxn / den,
                                gain * ix * fyn / den,
                                gain * ix / den,
                                gain * iy * fxn / den,
                                gain * iy * fyn / den,
                                gain * iy / den,
                                -gain * (ix * ax + iy * ay) * fxn / den,
                                -gain * (ix * ax + iy * ay) * fyn / den,
                                av,
                                1.0,
                            ];
                            for p in 0..10 {
                                st.1[p] += wgt * j[p] * r;
                                for q in p..10 {
                                    st.0[p * 10 + q] += wgt * j[p] * j[q];
                                }
                            }
                            st.2 += 1;
                            st.3 += wgt * r * r;
                        }
                        st
                    },
                )
                .reduce(
                    || (vec![0.0f64; 100], vec![0.0f64; 10], 0usize, 0.0f64),
                    |mut a, b| {
                        for i in 0..100 {
                            a.0[i] += b.0[i];
                        }
                        for i in 0..10 {
                            a.1[i] += b.1[i];
                        }
                        (a.0, a.1, a.2 + b.2, a.3 + b.3)
                    },
                );
            let (mut ata, atb, n, _) = acc;
            if n < 60 {
                return None;
            }
            for p in 0..10 {
                for q in 0..p {
                    ata[p * 10 + q] = ata[q * 10 + p];
                }
                // A little damping keeps a poorly constrained step small.
                ata[p * 10 + p] *= 1.0 + 1e-4;
                ata[p * 10 + p] += 1e-9;
            }
            let d = solve(ata, atb, 10)?;
            g[0][0] += d[0];
            g[0][1] += d[1];
            g[0][2] += d[2];
            g[1][0] += d[3];
            g[1][1] += d[4];
            g[1][2] += d[5];
            g[2][0] += d[6];
            g[2][1] += d[7];
            gain += d[8];
            off += d[9];
            let step = d[..8].iter().map(|v| v * v).sum::<f64>().sqrt();
            if !step.is_finite() || gain.abs() < 1e-3 {
                return None;
            }
            if step < 1e-5 {
                break;
            }
        }
    }
    let h = normalise(mul(&nm.tf_inv, &mul(&inverse(&g)?, &nm.ta)))?;
    let last = fr.stages.len() - 1;
    let (c, n) = ncc(&h, art, fr, last, 2);
    Some(Refined { h, ncc: c, coverage: n as f64 * 4.0 / (fr.w * fr.h) as f64 })
}

#[allow(clippy::too_many_arguments)]
fn moments(g: &Mat3, nm: &Norms, a: &Gray, ls: f64, img: &Gray, mask: &[bool], fw: usize, bb: (usize, usize, usize, usize), stride: usize) -> ([f64; 4], usize) {
    let (x0, y0, x1, y1) = bb;
    let (mut sa, mut sf, mut saa, mut sff, mut n) = (0.0, 0.0, 0.0, 0.0, 0usize);
    for y in (y0..=y1).step_by(stride) {
        for x in (x0..=x1).step_by(stride) {
            if !mask[y * fw + x] {
                continue;
            }
            let (fxn, fyn) = (nm.tf[0][0] * x as f64 + nm.tf[0][2], nm.tf[1][1] * y as f64 + nm.tf[1][2]);
            let Some(p) = apply(g, fxn, fyn) else { continue };
            let (pfx, pfy) = (nm.ta_inv[0][0] * p[0] + nm.ta_inv[0][2], nm.ta_inv[1][1] * p[1] + nm.ta_inv[1][2]);
            let (lx, ly) = ((pfx + 0.5) * ls - 0.5, (pfy + 0.5) * ls - 0.5);
            if !(lx >= 1.0 && ly >= 1.0 && lx < (a.w - 2) as f64 && ly < (a.h - 2) as f64) {
                continue;
            }
            let (xi, yi) = (lx as usize, ly as usize);
            let av = bil(&a.d, a.w, xi, yi, (lx - xi as f64) as f32, (ly - yi as f64) as f32) as f64;
            let fv = img.d[y * fw + x] as f64;
            sa += av;
            sf += fv;
            saa += av * av;
            sff += fv * fv;
            n += 1;
        }
    }
    let nf = n.max(1) as f64;
    let (ma, mf) = (sa / nf, sf / nf);
    ([ma, mf, (saa / nf - ma * ma).max(0.0), (sff / nf - mf * mf).max(0.0)], n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_recovers_a_homography() {
        let h: Mat3 = [[1.1, 0.2, 30.0], [-0.1, 0.9, 12.0], [0.0004, -0.0002, 1.0]];
        let src: Vec<[f64; 2]> = (0..20).map(|i| [(i * 37 % 200) as f64, (i * 53 % 170) as f64]).collect();
        let dst: Vec<[f64; 2]> = src.iter().map(|p| apply(&h, p[0], p[1]).unwrap()).collect();
        let g = fit(&src, &dst).unwrap();
        for r in 0..3 {
            for c in 0..3 {
                assert!((g[r][c] - h[r][c]).abs() < 1e-6 * h[r][c].abs().max(1.0), "{g:?}");
            }
        }
        // With a third of the pairs wrong, RANSAC still finds it.
        let mut bad = dst.clone();
        for (i, p) in bad.iter_mut().enumerate().filter(|(i, _)| i % 3 == 0) {
            p[0] += 40.0 + i as f64;
        }
        let (g, inl) = ransac(&src, &bad, 1.0, 500, 7).unwrap();
        assert_eq!(inl.iter().filter(|b| **b).count(), 13);
        let p = apply(&g, 100.0, 50.0).unwrap();
        let q = apply(&h, 100.0, 50.0).unwrap();
        assert!((p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() < 1e-6);
    }
}
