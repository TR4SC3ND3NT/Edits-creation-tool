//! Scale- and rotation-invariant keypoints with SIFT-style descriptors, and
//! nearest-neighbour matching with Lowe's ratio test.
//!
//! Detection: Harris corners on a pyramid of images, each 1/sqrt(2) the size
//! of the previous one, kept where they are the strongest in a 5x5
//! neighbourhood. Orientation: the peak of a 36-bin histogram of gradient
//! directions around the point. Descriptor: 4x4 cells x 8 directions of
//! gradient magnitude over a rotated 16x16 patch, normalised, clipped at 0.2
//! and normalised again (Lowe 2004). The gradients make the descriptor
//! indifferent to brightness and contrast, which a graded or flashed frame
//! needs. Plain Rust, no OpenCV.

use crate::img::{gaussian_blur, resize_area, Gray};

pub const DESC: usize = 128;

#[derive(Clone, Debug)]
pub struct Keypoint {
    /// Position in the base image, pixel-centre coordinates (pixel i is at i).
    pub x: f32,
    pub y: f32,
    /// Size of the pyramid level relative to the base image (1, 0.707, 0.5 ...).
    pub level_scale: f32,
    pub angle: f32,
    pub response: f32,
}

pub struct Features {
    pub points: Vec<Keypoint>,
    /// `points.len()` descriptors of `DESC` values each.
    pub desc: Vec<f32>,
}

impl Features {
    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
    pub fn d(&self, i: usize) -> &[f32] {
        &self.desc[i * DESC..(i + 1) * DESC]
    }
}

struct Level {
    img: Gray,
    gx: Vec<f32>,
    gy: Vec<f32>,
    scale: f32,
}

fn gradients(g: &Gray) -> (Vec<f32>, Vec<f32>) {
    let (w, h) = (g.w, g.h);
    let mut gx = vec![0.0; w * h];
    let mut gy = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let xl = x.saturating_sub(1);
            let xr = (x + 1).min(w - 1);
            let yu = y.saturating_sub(1);
            let yd = (y + 1).min(h - 1);
            gx[y * w + x] = 0.5 * (g.at(xr, y) - g.at(xl, y));
            gy[y * w + x] = 0.5 * (g.at(x, yd) - g.at(x, yu));
        }
    }
    (gx, gy)
}

/// Harris response with a Gaussian window of `sigma`.
fn harris(w: usize, h: usize, gx: &[f32], gy: &[f32], sigma: f32) -> Vec<f32> {
    let mk = |f: &dyn Fn(usize) -> f32| Gray { w, h, d: (0..w * h).map(f).collect() };
    let xx = gaussian_blur(&mk(&|i| gx[i] * gx[i]), sigma);
    let yy = gaussian_blur(&mk(&|i| gy[i] * gy[i]), sigma);
    let xy = gaussian_blur(&mk(&|i| gx[i] * gy[i]), sigma);
    (0..w * h).map(|i| xx.d[i] * yy.d[i] - xy.d[i] * xy.d[i] - 0.04 * (xx.d[i] + yy.d[i]).powi(2)).collect()
}

/// Detect and describe up to `max_points` keypoints of `img`. Levels stop
/// when the image is smaller than 40 pixels on a side.
pub fn detect(img: &Gray, max_points: usize) -> Features {
    let mut levels = Vec::new();
    let mut cur = gaussian_blur(img, 0.7);
    let mut scale = 1.0f32;
    while cur.w >= 40 && cur.h >= 40 && levels.len() < 10 {
        let (gx, gy) = gradients(&cur);
        levels.push(Level { img: cur.clone(), gx, gy, scale });
        let (nw, nh) = (((cur.w as f32) / std::f32::consts::SQRT_2).round() as usize, ((cur.h as f32) / std::f32::consts::SQRT_2).round() as usize);
        scale = nw as f32 / img.w as f32;
        cur = gaussian_blur(&resize_area(&cur, nw.max(1), nh.max(1)), 0.5);
    }
    // Candidates on every level, then the strongest overall with a per-level share.
    let mut cands: Vec<(usize, usize, usize, f32)> = Vec::new();
    for (li, l) in levels.iter().enumerate() {
        let (w, h) = (l.img.w, l.img.h);
        let r = harris(w, h, &l.gx, &l.gy, 1.5);
        let max = r.iter().cloned().fold(0.0f32, f32::max);
        if max <= 0.0 {
            continue;
        }
        let thr = max * 1e-3;
        let border = 12;
        let mut lv = Vec::new();
        for y in border..h.saturating_sub(border) {
            for x in border..w.saturating_sub(border) {
                let v = r[y * w + x];
                if v <= thr {
                    continue;
                }
                let mut best = true;
                'n: for dy in -2i32..=2 {
                    for dx in -2i32..=2 {
                        if (dx, dy) != (0, 0) && r[(y as i32 + dy) as usize * w + (x as i32 + dx) as usize] >= v {
                            best = false;
                            break 'n;
                        }
                    }
                }
                if best {
                    lv.push((li, x, y, v));
                }
            }
        }
        // Each level keeps its share of points by area, so coarse levels are not drowned.
        lv.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());
        let share = ((max_points as f32) * (w * h) as f32 / (img.w * img.h) as f32 * 0.6).ceil() as usize + 30;
        lv.truncate(share);
        cands.extend(lv);
    }
    cands.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());
    cands.truncate(max_points);
    let mut points = Vec::with_capacity(cands.len());
    let mut desc = Vec::with_capacity(cands.len() * DESC);
    for (li, x, y, v) in cands {
        let l = &levels[li];
        let angle = orientation(l, x as f32, y as f32);
        let d = describe(l, x as f32, y as f32, angle);
        let s = l.scale;
        points.push(Keypoint { x: (x as f32 + 0.5) / s - 0.5, y: (y as f32 + 0.5) / s - 0.5, level_scale: s, angle, response: v });
        desc.extend_from_slice(&d);
    }
    Features { points, desc }
}

fn grad_at(l: &Level, x: f32, y: f32) -> Option<(f32, f32)> {
    let (w, h) = (l.img.w, l.img.h);
    if !(x >= 0.0 && y >= 0.0 && x <= (w - 1) as f32 && y <= (h - 1) as f32) {
        return None;
    }
    let x0 = (x as usize).min(w - 2);
    let y0 = (y as usize).min(h - 2);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let i = y0 * w + x0;
    let bil = |a: &[f32]| {
        let t = a[i] + (a[i + 1] - a[i]) * fx;
        let b = a[i + w] + (a[i + w + 1] - a[i + w]) * fx;
        t + (b - t) * fy
    };
    Some((bil(&l.gx), bil(&l.gy)))
}

fn orientation(l: &Level, x: f32, y: f32) -> f32 {
    let mut hist = [0.0f32; 36];
    let r = 8i32;
    let sig2 = 2.0 * 4.0f32 * 4.0;
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = (dx * dx + dy * dy) as f32;
            if d2 > (r * r) as f32 {
                continue;
            }
            let (xx, yy) = (x as i32 + dx, y as i32 + dy);
            if xx < 0 || yy < 0 || xx >= l.img.w as i32 || yy >= l.img.h as i32 {
                continue;
            }
            let i = yy as usize * l.img.w + xx as usize;
            let (gx, gy) = (l.gx[i], l.gy[i]);
            let m = (gx * gx + gy * gy).sqrt() * (-d2 / sig2).exp();
            let a = gy.atan2(gx).rem_euclid(std::f32::consts::TAU);
            hist[((a / std::f32::consts::TAU * 36.0) as usize).min(35)] += m;
        }
    }
    // Smooth the histogram, then take the peak with a parabola through its neighbours.
    let mut sm = [0.0f32; 36];
    for i in 0..36 {
        sm[i] = 0.25 * hist[(i + 35) % 36] + 0.5 * hist[i] + 0.25 * hist[(i + 1) % 36];
    }
    let (mut bi, mut bv) = (0, -1.0);
    for (i, v) in sm.iter().enumerate() {
        if *v > bv {
            bi = i;
            bv = *v;
        }
    }
    let (a, b, c) = (sm[(bi + 35) % 36], sm[bi], sm[(bi + 1) % 36]);
    let den = a - 2.0 * b + c;
    let off = if den.abs() > 1e-12 { 0.5 * (a - c) / den } else { 0.0 };
    (bi as f32 + 0.5 + off) / 36.0 * std::f32::consts::TAU
}

fn describe(l: &Level, x: f32, y: f32, angle: f32) -> [f32; DESC] {
    let mut d = [0.0f32; DESC];
    let (c, s) = (angle.cos(), angle.sin());
    // 16x16 samples, 1.5 pixels apart: a 24-pixel patch at this level.
    let step = 1.5f32;
    for j in 0..16 {
        for i in 0..16 {
            let (u, v) = ((i as f32 - 7.5) * step, (j as f32 - 7.5) * step);
            let (px, py) = (x + c * u - s * v, y + s * u + c * v);
            let Some((gx, gy)) = grad_at(l, px, py) else { continue };
            let wgt = (-((i as f32 - 7.5).powi(2) + (j as f32 - 7.5).powi(2)) / (2.0 * 8.0 * 8.0)).exp();
            let m = (gx * gx + gy * gy).sqrt() * wgt;
            let a = (gy.atan2(gx) - angle).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 8.0;
            let (b0, f) = (a.floor() as usize % 8, a - a.floor());
            // Spatial bins with bilinear weights between cell centres.
            let (cx, cy) = (i as f32 / 4.0 - 0.375, j as f32 / 4.0 - 0.375);
            for (yc, wy) in [(cy.floor(), 1.0 - (cy - cy.floor())), (cy.floor() + 1.0, cy - cy.floor())] {
                for (xc, wx) in [(cx.floor(), 1.0 - (cx - cx.floor())), (cx.floor() + 1.0, cx - cx.floor())] {
                    if !(0.0..4.0).contains(&xc) || !(0.0..4.0).contains(&yc) {
                        continue;
                    }
                    let cell = (yc as usize * 4 + xc as usize) * 8;
                    d[cell + b0] += m * wx * wy * (1.0 - f);
                    d[cell + (b0 + 1) % 8] += m * wx * wy * f;
                }
            }
        }
    }
    let norm = |d: &mut [f32; DESC]| {
        let n = d.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
        d.iter_mut().for_each(|v| *v /= n);
    };
    norm(&mut d);
    d.iter_mut().for_each(|v| *v = v.min(0.2));
    norm(&mut d);
    d
}

/// For each point of `a`, its nearest point in `b` when the nearest is
/// clearly nearer than the second (distance ratio below `ratio`). Pairs (index in a, index in b).
pub fn match_ratio(a: &Features, b: &Features, ratio: f32) -> Vec<(usize, usize)> {
    use rayon::prelude::*;
    if b.len() < 2 {
        return Vec::new();
    }
    (0..a.len())
        .into_par_iter()
        .filter_map(|i| {
            let da = a.d(i);
            let (mut b1, mut b2, mut bi) = (f32::MAX, f32::MAX, 0);
            for j in 0..b.len() {
                let db = b.d(j);
                let mut s = 0.0f32;
                for k in 0..DESC {
                    let t = da[k] - db[k];
                    s += t * t;
                    if s > b2 {
                        break;
                    }
                }
                if s < b1 {
                    b2 = b1;
                    b1 = s;
                    bi = j;
                } else if s < b2 {
                    b2 = s;
                }
            }
            (b1 < ratio * ratio * b2).then_some((i, bi))
        })
        .collect()
}
