//! Per-shot transform track: similarity (scale, rotation, translation) of
//! every frame relative to the shot's reference frame.
//!
//! Direct alignment, coarse to fine, with an inverse-compositional
//! Gauss-Newton solver (Lucas-Kanade family). The images aligned are not
//! luminance but locally normalised gradient magnitude, and the cost is the
//! zero-mean normalised difference (as in ECC), so exposure flashes, fades,
//! tone inversion and constant grades do not move the optimum. A central mask
//! leaves out the frame border and the static overlay.

use crate::img::{gaussian_blur, half, Frames, Gray, Sim};

/// Zero-mean normalised correlation above which a registration is accepted.
pub const ZNCC_OK: f64 = 0.3;
const ZNCC_GOOD: f64 = 0.55;
/// Below this, a registration started from the neighbour's solution is retried from scratch.
const ZNCC_RETRY: f64 = 0.42;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Reference,
    Direct,
    Chained,
    Bridged,
}

#[derive(Clone, Copy, Debug)]
pub struct TrackSample {
    /// Where the reference frame's picture sits in this frame.
    pub sim: Sim,
    /// Normalised correlation of the registration, 0..1.
    pub zncc: f64,
    pub method: Method,
    /// How much better a free affine transform registers the frame than the
    /// similarity does: (ratio of its two scale factors - 1, gain in correlation).
    /// Large values mean the picture is stretched or tilted in 3D.
    pub affine: (f64, f64),
}

impl TrackSample {
    pub fn valid(&self) -> bool {
        matches!(self.method, Method::Reference | Method::Direct | Method::Chained)
    }
    pub fn confidence(&self) -> f64 {
        match self.method {
            Method::Reference => 1.0,
            Method::Bridged => 0.0,
            Method::Direct => ((self.zncc - 0.2) / 0.5).clamp(0.0, 1.0),
            Method::Chained => 0.8 * ((self.zncc - 0.2) / 0.5).clamp(0.0, 1.0),
        }
    }
}

fn box_blur(src: &Gray, r: usize) -> Gray {
    let (w, h) = (src.w, src.h);
    let mut tmp = Gray::new(w, h);
    for y in 0..h {
        let row = &src.d[y * w..(y + 1) * w];
        let mut cum = vec![0.0f64; w + 1];
        for x in 0..w {
            cum[x + 1] = cum[x] + row[x] as f64;
        }
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r + 1).min(w));
            tmp.d[y * w + x] = ((cum[b] - cum[a]) / (b - a) as f64) as f32;
        }
    }
    let mut out = Gray::new(w, h);
    let mut cum = vec![0.0f64; h + 1];
    for x in 0..w {
        for y in 0..h {
            cum[y + 1] = cum[y] + tmp.d[y * w + x] as f64;
        }
        for y in 0..h {
            let (a, b) = (y.saturating_sub(r), (y + r + 1).min(h));
            out.d[y * w + x] = ((cum[b] - cum[a]) / (b - a) as f64) as f32;
        }
    }
    out
}

/// Alignment feature: gradient magnitude of the lightly blurred luminance,
/// divided by its local mean. Invariant to gain, offset and tone inversion.
pub fn feature(lum: &Gray) -> Gray {
    let b = gaussian_blur(lum, 1.0);
    let (w, h) = (b.w, b.h);
    let mut g = Gray::new(w, h);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let gx = b.d[i + 1] - b.d[i - 1];
            let gy = b.d[i + w] - b.d[i - w];
            g.d[i] = (gx * gx + gy * gy).sqrt();
        }
    }
    let r = (h / 10).max(2);
    let local = box_blur(&box_blur(&g, r), r);
    let floor = 0.25 * g.mean() + 1e-4;
    for i in 0..g.d.len() {
        g.d[i] /= local.d[i] + floor;
    }
    gaussian_blur(&g, 0.8)
}

/// Feature pyramid of a frame, finest level first.
pub struct Pyramid {
    pub levels: Vec<Gray>,
}

pub fn pyramid(lum: &Gray) -> Pyramid {
    let mut lums = vec![lum.clone()];
    while lums.last().unwrap().h >= 72 && lums.last().unwrap().w >= 72 && lums.len() < 5 {
        let next = half(&gaussian_blur(lums.last().unwrap(), 0.7));
        lums.push(next);
    }
    Pyramid { levels: lums.iter().map(feature).collect() }
}

/// Usable-pixel masks for every pyramid level (finest first) from a
/// full-resolution mask (`true` = usable).
pub fn mask_pyramid(mask: &[bool], w: usize, h: usize, levels: usize) -> Vec<(usize, usize, Vec<bool>)> {
    let mut out = vec![(w, h, mask.to_vec())];
    for _ in 1..levels {
        let (pw, ph, pm) = out.last().unwrap();
        let (nw, nh) = (pw / 2, ph / 2);
        let mut m = vec![false; nw * nh];
        for y in 0..nh {
            for x in 0..nw {
                let i = 2 * y * pw + 2 * x;
                m[y * nw + x] = pm[i] && pm[i + 1] && pm[i + pw] && pm[i + pw + 1];
            }
        }
        out.push((nw, nh, m));
    }
    out
}

struct TmplLevel {
    w: usize,
    h: usize,
    x: Vec<f32>,
    y: Vec<f32>,
    t: Vec<f32>,
    sd: Vec<[f32; 4]>,
    /// Template gradient, for the affine check.
    g: Vec<[f32; 2]>,
}

/// A reference image prepared for inverse-compositional alignment.
pub struct Template {
    levels: Vec<TmplLevel>,
}

/// Fraction of the frame left out at each border.
const BORDER: f32 = 0.12;

impl Template {
    pub fn new(pyr: &Pyramid, masks: &[(usize, usize, Vec<bool>)]) -> Self {
        let levels = pyr
            .levels
            .iter()
            .zip(masks)
            .map(|(f, (_, _, m))| {
                let (w, h) = (f.w, f.h);
                let (cx, cy) = ((w as f32 - 1.0) / 2.0, (h as f32 - 1.0) / 2.0);
                let (mx, my) = (((w as f32 * BORDER) as usize).max(2), ((h as f32 * BORDER) as usize).max(2));
                let mut l = TmplLevel { w, h, x: vec![], y: vec![], t: vec![], sd: vec![], g: vec![] };
                // Large levels are sampled on a 2-pixel lattice: still tens of
                // thousands of equations for four unknowns.
                let stride = if w * h >= 80_000 { 2 } else { 1 };
                for y in (my..h - my).step_by(stride) {
                    for x in (mx..w - mx).step_by(stride) {
                        let i = y * w + x;
                        if !m[i] {
                            continue;
                        }
                        let gx = 0.5 * (f.d[i + 1] - f.d[i - 1]);
                        let gy = 0.5 * (f.d[i + w] - f.d[i - w]);
                        let (px, py) = (x as f32 - cx, y as f32 - cy);
                        l.x.push(px);
                        l.y.push(py);
                        l.t.push(f.d[i]);
                        l.sd.push([gx * px + gy * py, -gx * py + gy * px, gx, gy]);
                        l.g.push([gx, gy]);
                    }
                }
                l
            })
            .collect();
        Template { levels }
    }
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }
}

fn solve4(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    for i in 0..4 {
        let mut p = i;
        for r in i + 1..4 {
            if a[r][i].abs() > a[p][i].abs() {
                p = r;
            }
        }
        if a[p][i].abs() < 1e-12 {
            return None;
        }
        a.swap(i, p);
        b.swap(i, p);
        for r in i + 1..4 {
            let f = a[r][i] / a[i][i];
            for c in i..4 {
                a[r][c] -= f * a[i][c];
            }
            b[r] -= f * b[i];
        }
    }
    let mut x = [0.0; 4];
    for i in (0..4).rev() {
        let mut s = b[i];
        for c in i + 1..4 {
            s -= a[i][c] * x[c];
        }
        x[i] = s / a[i][i];
    }
    Some(x)
}

/// Sample the current frame under `sim` at the template's pixels. Returns the
/// samples (NaN where invalid) and the valid count.
fn sample_level(l: &TmplLevel, cur: &Gray, cur_mask: &[bool], sim: &Sim, buf: &mut Vec<f32>) -> usize {
    let hh = l.h as f32;
    let (cx, cy) = ((l.w as f32 - 1.0) / 2.0, (l.h as f32 - 1.0) / 2.0);
    let (a, b, tx, ty) = (sim.a as f32, sim.b as f32, sim.tx as f32 * hh + cx, sim.ty as f32 * hh + cy);
    buf.clear();
    let mut n = 0;
    for k in 0..l.x.len() {
        let xw = a * l.x[k] - b * l.y[k] + tx;
        let yw = b * l.x[k] + a * l.y[k] + ty;
        let mut v = f32::NAN;
        if let Some(s) = cur.sample(xw, yw) {
            let mi = (yw + 0.5) as usize * cur.w + (xw + 0.5) as usize;
            if cur_mask[mi.min(cur_mask.len() - 1)] {
                v = s;
                n += 1;
            }
        }
        buf.push(v);
    }
    n
}

/// Zero-mean normalised correlation of the template with the samples.
fn zncc(l: &TmplLevel, buf: &[f32]) -> Option<(f64, f64, f64, f64, f64)> {
    let (mut n, mut st, mut si, mut stt, mut sii, mut sti) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for k in 0..buf.len() {
        let i = buf[k];
        if i.is_nan() {
            continue;
        }
        let (t, i) = (l.t[k] as f64, i as f64);
        n += 1.0;
        st += t;
        si += i;
        stt += t * t;
        sii += i * i;
        sti += t * i;
    }
    if n < 16.0 {
        return None;
    }
    let (mt, mi) = (st / n, si / n);
    let (vt, vi) = (stt / n - mt * mt, sii / n - mi * mi);
    if vt < 1e-10 || vi < 1e-10 {
        return None;
    }
    Some(((sti / n - mt * mi) / (vt * vi).sqrt(), mt, vt.sqrt(), mi, vi.sqrt()))
}

#[derive(Clone, Copy, Debug)]
pub struct AlignResult {
    pub sim: Sim,
    pub zncc: f64,
}

/// Align the current frame (`cur` pyramid, `masks`) to the template, starting at `init`.
pub fn align(tmpl: &Template, cur: &Pyramid, masks: &[(usize, usize, Vec<bool>)], init: Sim) -> Option<AlignResult> {
    align_from(tmpl, cur, masks, init, 0)
}

/// As [`align`], stopping at pyramid level `finest` (0 = full resolution).
pub fn align_from(tmpl: &Template, cur: &Pyramid, masks: &[(usize, usize, Vec<bool>)], init: Sim, finest: usize) -> Option<AlignResult> {
    let mut sim = init;
    let mut buf = Vec::new();
    let mut score = None;
    let finest = finest.min(tmpl.levels.len() - 1);
    for li in (finest..tmpl.levels.len()).rev() {
        let l = &tmpl.levels[li];
        if l.x.len() < 64 {
            continue;
        }
        let (f, m) = (&cur.levels[li], &masks[li].2);
        let hh = l.h as f64;
        let reach = 0.5 * (l.w.max(l.h) as f64);
        let iters = if li == 0 { 8 } else { 25 };
        let li_last = li == finest;
        let mut last = None;
        for _ in 0..iters {
            let n = sample_level(l, f, m, &sim, &mut buf);
            if n * 4 < l.x.len() {
                break;
            }
            let Some((rho, mt, st, mi, si)) = zncc(l, &buf) else { break };
            last = Some(rho);
            let mut hm = [[0.0f64; 4]; 4];
            let mut g = [0.0f64; 4];
            for k in 0..buf.len() {
                let i = buf[k];
                if i.is_nan() {
                    continue;
                }
                let e = (i as f64 - mi) / si - (l.t[k] as f64 - mt) / st;
                let sd = l.sd[k];
                for r in 0..4 {
                    let sr = sd[r] as f64 / st;
                    g[r] += sr * e;
                    for c in r..4 {
                        hm[r][c] += sr * sd[c] as f64 / st;
                    }
                }
            }
            for r in 0..4 {
                for c in 0..r {
                    hm[r][c] = hm[c][r];
                }
                hm[r][r] *= 1.0 + 1e-4;
            }
            let Some(dp) = solve4(hm, g) else { break };
            let inc = Sim { a: 1.0 + dp[0], b: dp[1], tx: dp[2] / hh, ty: dp[3] / hh };
            if !inc.is_finite() || inc.scale() < 0.5 || inc.scale() > 2.0 {
                break;
            }
            sim = sim.compose(&inc.inverse());
            let moved = (dp[0].abs() + dp[1].abs()) * reach + dp[2].abs() + dp[3].abs();
            if moved < 0.01 {
                break;
            }
        }
        if li_last {
            // Score at the final parameters.
            let n = sample_level(l, f, m, &sim, &mut buf);
            score = if n * 4 < l.x.len() { None } else { zncc(l, &buf).map(|z| z.0) };
        } else if score.is_none() {
            score = last;
        }
    }
    let z = score?;
    if !sim.is_finite() || sim.scale() < 0.2 || sim.scale() > 5.0 {
        return None;
    }
    Some(AlignResult { sim, zncc: z })
}

/// A general affine map in level pixels about the image centre: `p' = M p + t`.
#[derive(Clone, Copy)]
struct Aff {
    m: [f64; 4],
    t: [f64; 2],
}

fn solve_n(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for i in 0..n {
        let mut p = i;
        for r in i + 1..n {
            if a[r][i].abs() > a[p][i].abs() {
                p = r;
            }
        }
        if a[p][i].abs() < 1e-12 {
            return None;
        }
        a.swap(i, p);
        b.swap(i, p);
        for r in i + 1..n {
            let f = a[r][i] / a[i][i];
            for c in i..n {
                a[r][c] -= f * a[i][c];
            }
            b[r] -= f * b[i];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for c in i + 1..n {
            s -= a[i][c] * x[c];
        }
        x[i] = s / a[i][i];
    }
    Some(x)
}

fn sample_affine(l: &TmplLevel, cur: &Gray, cur_mask: &[bool], a: &Aff, buf: &mut Vec<f32>) -> usize {
    let (cx, cy) = ((l.w as f32 - 1.0) / 2.0, (l.h as f32 - 1.0) / 2.0);
    let (m, t) = ([a.m[0] as f32, a.m[1] as f32, a.m[2] as f32, a.m[3] as f32], [a.t[0] as f32 + cx, a.t[1] as f32 + cy]);
    buf.clear();
    let mut n = 0;
    for k in 0..l.x.len() {
        let xw = m[0] * l.x[k] + m[1] * l.y[k] + t[0];
        let yw = m[2] * l.x[k] + m[3] * l.y[k] + t[1];
        let mut v = f32::NAN;
        if let Some(s) = cur.sample(xw, yw) {
            let mi = (yw + 0.5) as usize * cur.w + (xw + 0.5) as usize;
            if cur_mask[mi.min(cur_mask.len() - 1)] {
                v = s;
                n += 1;
            }
        }
        buf.push(v);
    }
    n
}

/// Starting from a similarity registration, let all six affine parameters go
/// and report (ratio of the two scale factors - 1, correlation reached).
/// Used only as a check: a clear gain with unequal scale factors means the
/// motion is not a similarity (a 3D tilt, a stretch).
pub fn affine_check(tmpl: &Template, cur: &Pyramid, masks: &[(usize, usize, Vec<bool>)], sim: &Sim) -> Option<(f64, f64)> {
    let top = tmpl.levels.len().saturating_sub(1).min(2);
    let mut aff: Option<Aff> = None;
    let mut buf = Vec::new();
    let mut score = None;
    for li in (0..=top).rev() {
        let l = &tmpl.levels[li];
        if l.x.len() < 64 {
            continue;
        }
        let hh = l.h as f64;
        // Carry the transform between levels in height units.
        let mut a = match aff {
            None => Aff { m: [sim.a, -sim.b, sim.b, sim.a], t: [sim.tx * hh, sim.ty * hh] },
            Some(p) => Aff { m: p.m, t: [p.t[0] * 2.0, p.t[1] * 2.0] },
        };
        let (f, m) = (&cur.levels[li], &masks[li].2);
        let reach = 0.5 * l.w.max(l.h) as f64;
        for _ in 0..(if li == 0 { 6 } else { 15 }) {
            let n = sample_affine(l, f, m, &a, &mut buf);
            if n * 4 < l.x.len() {
                return None;
            }
            let (_, mt, st, mi, si) = zncc(l, &buf)?;
            let mut hm = vec![vec![0.0f64; 6]; 6];
            let mut g = vec![0.0f64; 6];
            for k in 0..buf.len() {
                let i = buf[k];
                if i.is_nan() {
                    continue;
                }
                let e = (i as f64 - mi) / si - (l.t[k] as f64 - mt) / st;
                let (gx, gy, x, y) = (l.g[k][0] as f64 / st, l.g[k][1] as f64 / st, l.x[k] as f64, l.y[k] as f64);
                let sd = [gx * x, gx * y, gy * x, gy * y, gx, gy];
                for r in 0..6 {
                    g[r] += sd[r] * e;
                    for c in r..6 {
                        hm[r][c] += sd[r] * sd[c];
                    }
                }
            }
            for r in 0..6 {
                for c in 0..r {
                    hm[r][c] = hm[c][r];
                }
                hm[r][r] *= 1.0 + 1e-4;
            }
            let dp = solve_n(hm, g)?;
            // W <- W o dW^-1
            let d = [1.0 + dp[0], dp[1], dp[2], 1.0 + dp[3]];
            let det = d[0] * d[3] - d[1] * d[2];
            if !(det.abs() > 0.2 && det.abs() < 5.0) {
                return None;
            }
            let inv = [d[3] / det, -d[1] / det, -d[2] / det, d[0] / det];
            let it = [-(inv[0] * dp[4] + inv[1] * dp[5]), -(inv[2] * dp[4] + inv[3] * dp[5])];
            a = Aff {
                m: [a.m[0] * inv[0] + a.m[1] * inv[2], a.m[0] * inv[1] + a.m[1] * inv[3], a.m[2] * inv[0] + a.m[3] * inv[2], a.m[2] * inv[1] + a.m[3] * inv[3]],
                t: [a.m[0] * it[0] + a.m[1] * it[1] + a.t[0], a.m[2] * it[0] + a.m[3] * it[1] + a.t[1]],
            };
            if (dp[0].abs() + dp[1].abs() + dp[2].abs() + dp[3].abs()) * reach + dp[4].abs() + dp[5].abs() < 0.02 {
                break;
            }
        }
        if li == 0 {
            let n = sample_affine(l, f, m, &a, &mut buf);
            if n * 4 >= l.x.len() {
                score = zncc(l, &buf).map(|z| z.0);
            }
        }
        aff = Some(a);
    }
    let a = aff?;
    // Singular values of M.
    let (p, q, r, s2) = (a.m[0], a.m[1], a.m[2], a.m[3]);
    let e = (p * p + q * q + r * r + s2 * s2) / 2.0;
    let d = ((p * p + q * q - r * r - s2 * s2).powi(2) / 4.0 + (p * r + q * s2).powi(2)).sqrt();
    let (s_hi, s_lo) = ((e + d).sqrt(), (e - d).max(1e-12).sqrt());
    Some((s_hi / s_lo - 1.0, score?))
}

/// Exhaustive search over scale and shift at the coarsest level; a starting
/// point when nothing is known about the relation of two frames.
pub fn coarse_search(tmpl: &Template, cur: &Pyramid, masks: &[(usize, usize, Vec<bool>)]) -> Option<Sim> {
    let li = tmpl.levels.len() - 1;
    let l = &tmpl.levels[li];
    if l.x.len() < 64 {
        return None;
    }
    let (f, m) = (&cur.levels[li], &masks[li].2);
    let hh = l.h as f64;
    let range = (0.3 * hh) as i32;
    let mut buf = Vec::new();
    let mut best: Option<(f64, Sim)> = None;
    let mut try_at = |s: f64, dx: i32, dy: i32, best: &mut Option<(f64, Sim)>| {
        let sim = Sim { a: s, b: 0.0, tx: dx as f64 / hh, ty: dy as f64 / hh };
        let n = sample_level(l, f, m, &sim, &mut buf);
        if n * 2 >= l.x.len() {
            if let Some((rho, ..)) = zncc(l, &buf) {
                if best.map(|b| rho > b.0).unwrap_or(true) {
                    *best = Some((rho, sim));
                }
            }
        }
    };
    // Two-pixel grid over shift for each scale; the refinement that follows
    // (Gauss-Newton from the best cell) takes care of the rest.
    for s in [0.55, 0.65, 0.75, 0.85, 0.93, 1.0, 1.08, 1.18, 1.33, 1.55, 1.8] {
        let mut dy = -range;
        while dy <= range {
            let mut dx = -range;
            while dx <= range {
                try_at(s, dx, dy, &mut best);
                dx += 2;
            }
            dy += 2;
        }
    }
    if let Some((_, c)) = best {
        let (cx, cy) = ((c.tx * hh).round() as i32, (c.ty * hh).round() as i32);
        for dy in cy - 1..=cy + 1 {
            for dx in cx - 1..=cx + 1 {
                try_at(c.a, dx, dy, &mut best);
            }
        }
    }
    best.map(|b| b.1)
}

/// Align two arbitrary frames with no prior: coarse search, then refinement.
pub fn align_unknown(tmpl: &Template, cur: &Pyramid, masks: &[(usize, usize, Vec<bool>)]) -> Option<AlignResult> {
    // Half resolution is plenty to decide whether two frames show the same picture.
    let finest = if tmpl.levels.len() >= 3 { 1 } else { 0 };
    let a = align_from(tmpl, cur, masks, Sim::IDENTITY, finest);
    let b = coarse_search(tmpl, cur, masks).and_then(|s| align_from(tmpl, cur, masks, s, finest));
    match (a, b) {
        (Some(a), Some(b)) => Some(if a.zncc >= b.zncc { a } else { b }),
        (a, b) => a.or(b),
    }
}

fn lerp_sim(a: &Sim, b: &Sim, u: f64) -> Sim {
    let s = (a.scale().ln() * (1.0 - u) + b.scale().ln() * u).exp();
    let mut dr = b.rotation_deg() - a.rotation_deg();
    if dr > 180.0 {
        dr -= 360.0;
    } else if dr < -180.0 {
        dr += 360.0;
    }
    Sim::from_parts(s, a.rotation_deg() + dr * u, a.tx + (b.tx - a.tx) * u, a.ty + (b.ty - a.ty) * u)
}

/// Track frames `f0..f1` against `ref_frame`. `usable` marks pixels that are
/// not part of a static overlay (`true` = may be used).
pub fn track_shot(frames: &Frames, f0: usize, f1: usize, ref_frame: usize, usable: &[bool]) -> Vec<TrackSample> {
    let n = f1 - f0;
    let mut out = vec![TrackSample { sim: Sim::IDENTITY, zncc: 0.0, method: Method::Bridged, affine: (0.0, 0.0) }; n];
    let ref_pyr = pyramid(&frames.gray(ref_frame));
    let masks = mask_pyramid(usable, frames.w, frames.h, ref_pyr.levels.len());
    let tmpl = Template::new(&ref_pyr, &masks);
    out[ref_frame - f0] = TrackSample { sim: Sim::IDENTITY, zncc: 1.0, method: Method::Reference, affine: (0.0, 0.0) };
    let forward: Vec<usize> = (ref_frame + 1..f1).collect();
    let backward: Vec<usize> = (f0..ref_frame).rev().collect();
    for order in [forward, backward] {
        // Neighbour nearer to the reference frame: its pyramid and its solution.
        let mut nb: Option<(Pyramid, TrackSample)> = None;
        let mut last_good = Sim::IDENTITY;
        for (step, &i) in order.iter().enumerate() {
            let pyr = pyramid(&frames.gray(i));
            let mut best = align(&tmpl, &pyr, &masks, last_good);
            if best.map(|b| b.zncc < ZNCC_RETRY).unwrap_or(true) {
                for alt in [align(&tmpl, &pyr, &masks, Sim::IDENTITY), coarse_search(&tmpl, &pyr, &masks).and_then(|s| align(&tmpl, &pyr, &masks, s))].into_iter().flatten() {
                    if best.map(|b| alt.zncc > b.zncc).unwrap_or(true) {
                        best = Some(alt);
                    }
                }
            }
            let mut sample = TrackSample { sim: last_good, zncc: best.map(|b| b.zncc).unwrap_or(0.0), method: Method::Bridged, affine: (0.0, 0.0) };
            match best {
                Some(b) if b.zncc >= ZNCC_OK => {
                    let affine = affine_check(&tmpl, &pyr, &masks, &b.sim).map(|(an, z)| (an, z - b.zncc)).unwrap_or((0.0, 0.0));
                    sample = TrackSample { sim: b.sim, zncc: b.zncc, method: Method::Direct, affine }
                }
                _ => {
                    // Chain through the neighbour: frame-to-frame motion is small
                    // and the two frames share blur and exposure.
                    let prev = if step == 0 { Some((&ref_pyr, out[ref_frame - f0])) } else { nb.as_ref().map(|(p, s)| (p, *s)) };
                    if let Some((npyr, ns)) = prev {
                        if ns.valid() {
                            let nt = Template::new(npyr, &masks);
                            if let Some(st) = align(&nt, &pyr, &masks, Sim::IDENTITY) {
                                if st.zncc >= ZNCC_GOOD {
                                    sample = TrackSample { sim: st.sim.compose(&ns.sim), zncc: st.zncc.min(ns.zncc.max(ZNCC_OK)), method: Method::Chained, affine: (0.0, 0.0) };
                                }
                            }
                        }
                    }
                }
            }
            if sample.valid() {
                last_good = sample.sim;
            }
            out[i - f0] = sample;
            nb = Some((pyr, sample));
        }
    }
    bridge(&mut out);
    out
}

/// Fill frames that could not be registered by interpolating between their
/// valid neighbours (held at the ends). They stay marked `Bridged`.
pub fn bridge(track: &mut [TrackSample]) {
    let valid: Vec<usize> = (0..track.len()).filter(|i| track[*i].valid()).collect();
    if valid.is_empty() {
        return;
    }
    for i in 0..track.len() {
        if track[i].valid() {
            continue;
        }
        let before = valid.iter().rev().find(|v| **v < i).copied();
        let after = valid.iter().find(|v| **v > i).copied();
        track[i].sim = match (before, after) {
            (Some(a), Some(b)) => lerp_sim(&track[a].sim, &track[b].sim, (i - a) as f64 / (b - a) as f64),
            (Some(a), None) => track[a].sim,
            (None, Some(b)) => track[b].sim,
            (None, None) => Sim::IDENTITY,
        };
    }
}

impl TrackSample {
    /// The frame registers clearly better with unequal scale factors than
    /// with a similarity: a 3D tilt or a stretch.
    pub fn non_similarity(&self) -> bool {
        self.method == Method::Direct && self.affine.0 > 0.08 && self.affine.1 > 0.05
    }
}

/// Mask of pixels that may be used for alignment when nothing is known about overlays.
pub fn all_usable(frames: &Frames) -> Vec<bool> {
    vec![true; frames.w * frames.h]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    #[test]
    fn recovers_a_known_zoom_rotation_and_slide() {
        let n = 16;
        let mut clip = synth::Clip::new(160, 160, n);
        clip.noise = 1.5;
        clip.vignette = 0.5;
        let truth: Vec<(f64, f64, f64, f64)> = (0..n)
            .map(|f| {
                let k = f as f64;
                (1.0 + 0.3 * 0.62f64.powf(k), 6.0 * 0.7f64.powf(k), 0.08 * 0.6f64.powf(k), -0.03 * 0.6f64.powf(k))
            })
            .collect();
        for f in 0..n {
            let fr = clip.frame_mut(f);
            (fr.scale, fr.rot_deg, fr.tx, fr.ty) = truth[f];
            // A flash on the first frames and an inversion hit later must not disturb the track.
            fr.stops = if f < 4 { 2.5 * 0.5f64.powf(f as f64) } else { 0.0 };
            fr.invert = f == 9;
        }
        let frames = clip.render();
        let r = n - 1;
        let track = track_shot(&frames, 0, n, r, &all_usable(&frames));
        let (rs, rr, rx, ry) = truth[r];
        let mut valid = 0;
        for f in 0..n {
            let t = &track[f];
            if !t.valid() {
                continue;
            }
            valid += 1;
            // Expected transform relative to the reference frame.
            let want = Sim::from_parts(truth[f].0, truth[f].1, truth[f].2, truth[f].3).compose(&Sim::from_parts(rs, rr, rx, ry).inverse());
            assert!((t.sim.scale() / want.scale() - 1.0).abs() < 0.006, "frame {f}: scale {} vs {}", t.sim.scale(), want.scale());
            assert!((t.sim.rotation_deg() - want.rotation_deg()).abs() < 0.25, "frame {f}: rot {} vs {}", t.sim.rotation_deg(), want.rotation_deg());
            assert!((t.sim.tx - want.tx).abs() < 0.004 && (t.sim.ty - want.ty).abs() < 0.004, "frame {f}: t {} {} vs {} {}", t.sim.tx, t.sim.ty, want.tx, want.ty);
        }
        assert!(valid >= n - 2, "only {valid} of {n} frames registered: {:?}", track.iter().map(|t| (t.zncc * 100.0).round()).collect::<Vec<_>>());
    }

    #[test]
    fn a_stretch_is_flagged_as_non_similarity() {
        // Frame 1 is the picture squeezed vertically (as a card tilting back would look).
        let clip = synth::Clip::new(160, 160, 3);
        let mut frames = clip.render();
        let src = frames.rgb(0).to_vec();
        let (w, h) = (frames.w, frames.h);
        let squeezed: Vec<u8> = (0..h)
            .flat_map(|y| {
                let sy = ((y as f64 - 79.5) / 0.75 + 79.5).round().clamp(0.0, 159.0) as usize;
                src[sy * w * 3..(sy + 1) * w * 3].to_vec()
            })
            .collect();
        frames.data[w * h * 3..2 * w * h * 3].copy_from_slice(&squeezed);
        let track = track_shot(&frames, 0, 3, 0, &all_usable(&frames));
        assert!(track[1].non_similarity(), "squeezed frame: {:?}", track[1]);
        assert!((track[1].affine.0 - 0.333).abs() < 0.08, "anisotropy {:?}", track[1].affine);
        assert!(!track[2].non_similarity(), "identical frame: {:?}", track[2]);
    }

    #[test]
    fn unrelated_pictures_do_not_register() {
        let mut clip = synth::Clip::new(128, 128, 2);
        clip.frame_mut(1).picture = 7;
        let frames = clip.render();
        let usable = all_usable(&frames);
        let a = pyramid(&frames.gray(0));
        let b = pyramid(&frames.gray(1));
        let masks = mask_pyramid(&usable, frames.w, frames.h, a.levels.len());
        let t = Template::new(&a, &masks);
        let r = align_unknown(&t, &b, &masks);
        assert!(r.map(|r| r.zncc < ZNCC_OK).unwrap_or(true), "{r:?}");
    }
}
