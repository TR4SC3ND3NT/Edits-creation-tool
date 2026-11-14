//! Built-in effects ported from a hand-built 3D edit (the "Death Note" kit):
//! `shatter`, `pinch`, `glitch_bands`, `film_scratches`, `grain_highlights`
//! and `drip`. Each is one pass; `shatter` also binds a [`Table`] that holds,
//! for every shard, the homography from the output frame back to the
//! picture, computed here on the CPU for the instant being rendered.

use super::{num, params, pct, pos, rgb, seed, Builtin, Frame, Params, Table};
use edits_core::ResolvedEffect;

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { kind: "shatter", source: include_str!("../shaders/fx/shatter.wgsl"), pack: shatter },
    Builtin { kind: "pinch", source: include_str!("../shaders/fx/pinch.wgsl"), pack: pinch },
    Builtin { kind: "glitch_bands", source: include_str!("../shaders/fx/glitch_bands.wgsl"), pack: glitch_bands },
    Builtin { kind: "film_scratches", source: include_str!("../shaders/fx/film_scratches.wgsl"), pack: film_scratches },
    Builtin { kind: "grain_highlights", source: include_str!("../shaders/fx/grain_highlights.wgsl"), pack: grain_highlights },
    Builtin { kind: "drip", source: include_str!("../shaders/fx/drip.wgsl"), pack: drip },
];

/// Pixels at 1080 high -> composition units.
fn px(v: f32) -> f32 {
    v / 1080.0
}

fn pinch(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (amount, radius, c) = (num(e, "amount"), pct(num(e, "radius") as f64), pos(e, "center"));
    (amount != 0.0 && radius > 0.0).then(|| params(&[amount, radius, c[0], c[1]]))
}

fn glitch_bands(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    let (bands, blocks) = (num(e, "bands").round(), num(e, "blocks").round());
    // The pattern number is floor(output frame / hold), taken in the shader.
    let pattern = num(e, "hold").round().max(1.0);
    (amount > 0.0 && (bands > 0.0 || blocks > 0.0)).then(|| {
        params(&[
            amount,
            bands,
            pct(num(e, "height") as f64),
            num(e, "width"),
            pct(num(e, "shift") as f64),
            num(e, "stretch"),
            num(e, "smear"),
            blocks,
            pct(num(e, "block_size") as f64),
            num(e, "noise"),
            num(e, "colors"),
            pattern,
            seed(e),
        ])
    })
}

fn film_scratches(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (s, d) = (num(e, "scratches"), num(e, "dust").round());
    (s > 0.0 || d > 0.0).then(|| {
        params(&[
            s,
            num(e, "count").round(),
            pct(num(e, "length") as f64),
            px(num(e, "width")),
            num(e, "brightness"),
            d,
            px(num(e, "dust_size")),
            num(e, "dust_depth"),
            num(e, "light_only"),
            seed(e),
        ])
    })
}

fn grain_highlights(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    let (low, high) = (num(e, "low"), num(e, "high"));
    (amount > 0.0).then(|| params(&[amount, px(num(e, "size")).max(1e-6), low, high.max(low + 1e-4), seed(e)]))
}

fn drip(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (rough, drips) = (pct(num(e, "roughness") as f64), num(e, "drips"));
    let len = pct(num(e, "length") as f64) * num(e, "grow");
    (rough > 0.0 || (drips > 0.0 && len > 0.0)).then(|| {
        params(&[rough, pct(num(e, "scale") as f64), drips, len, pct(num(e, "width") as f64), seed(e), num(e, "grow")])
    })
}

// ---- shatter -------------------------------------------------------------------

/// Most shards (sectors x (rings + 1)).
pub(crate) const MAX_SHARDS: usize = 64;
/// Table entries per shard: the homography (hi and lo halves of each row),
/// then [a0, a1, shade] and [r0, r1, aa], each hi and lo.
pub(crate) const TABLE_ROW: usize = 10;
const ROW: usize = TABLE_ROW;

/// One shard at an instant.
#[derive(Clone, Debug)]
pub(crate) struct Shard {
    /// Sector between the angles a0 and a1 (radians, a0 < a1 < a0 + 2 pi),
    /// between the radii r0 and r1 from the impact point.
    pub a0: f64,
    pub a1: f64,
    pub r0: f64,
    pub r1: f64,
    /// Output frame -> picture (composition units), row major, scaled so
    /// that its largest entry is 1 (a positive scale).
    pub inv: [f64; 9],
    /// Distance from the camera (larger is farther).
    pub depth: f64,
    /// Brightness factor.
    pub shade: f64,
    /// Picture units per output unit at the shard's centre.
    pub scale: f64,
}

/// A small exact hash to 0..1 (the CPU twin of the shaders' `fx_hash`).
fn hash(x: u32, y: u32, z: u32) -> f64 {
    let mut h = x.wrapping_mul(0x9E3779B1).wrapping_add(y.wrapping_mul(0x85EBCA77)).wrapping_add(z.wrapping_mul(0xC2B2AE3D));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A2D39);
    h ^= h >> 15;
    (h >> 8) as f64 / 16777216.0
}

type M3 = [[f64; 3]; 3];

fn mul(a: &M3, b: &M3) -> M3 {
    let mut o = [[0.0; 3]; 3];
    for (i, row) in o.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    o
}

fn det(m: &M3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn inverse(m: &M3) -> Option<M3> {
    let d = det(m);
    if !d.is_finite() || d.abs() < 1e-15 {
        return None;
    }
    let c = |a: usize, b: usize, x: usize, y: usize| m[a][b] * m[x][y] - m[a][y] * m[x][b];
    Some([
        [c(1, 1, 2, 2) / d, -c(0, 1, 2, 2) / d, c(0, 1, 1, 2) / d],
        [-c(1, 0, 2, 2) / d, c(0, 0, 2, 2) / d, -c(0, 0, 1, 2) / d],
        [c(1, 0, 2, 1) / d, -c(0, 0, 2, 1) / d, c(0, 0, 1, 1) / d],
    ])
}

/// Rotation by `th` radians about the unit axis `u` (Rodrigues).
fn rot_axis(u: [f64; 3], th: f64) -> M3 {
    let n = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt().max(1e-12);
    let u = [u[0] / n, u[1] / n, u[2] / n];
    let (s, c) = th.sin_cos();
    let k: M3 = [[0.0, -u[2], u[1]], [u[2], 0.0, -u[0]], [-u[1], u[0], 0.0]];
    let kk = mul(&k, &k);
    let mut o = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            o[i][j] = if i == j { 1.0 } else { 0.0 } + s * k[i][j] + (1.0 - c) * kk[i][j];
        }
    }
    o
}

/// The shards of a `shatter` effect at project time `t`, nearest first.
pub(crate) fn shards(e: &ResolvedEffect, f: &Frame) -> Vec<Shard> {
    let aspect = f.aspect as f64;
    let sd = seed(e) as i32 as u32;
    let center = pos(e, "center").map(f64::from);
    let jitter = num(e, "jitter") as f64;
    // Sector boundaries.
    let given = e.params.get("angles").cloned().unwrap_or_default();
    let mut angles: Vec<f64> = if given.len() >= 2 {
        given.iter().filter(|v| v.is_finite()).take(32).map(|v| v.to_radians()).collect()
    } else {
        let n = num(e, "pieces").round().clamp(2.0, 32.0) as usize;
        let base = hash(sd, 7, 1) * std::f64::consts::TAU;
        (0..n).map(|i| base + (i as f64 + (hash(sd, i as u32, 2) - 0.5) * jitter * 0.8) * std::f64::consts::TAU / n as f64).collect()
    };
    angles.iter_mut().for_each(|a| *a = a.rem_euclid(std::f64::consts::TAU));
    angles.sort_by(|a, b| a.total_cmp(b));
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    if angles.len() < 2 {
        angles = vec![0.0, std::f64::consts::PI];
    }
    let rings = num(e, "rings").round().clamp(0.0, 3.0) as usize;
    let ring = pct(num(e, "ring_size") as f64) as f64;
    let n = angles.len();
    let mut cells = Vec::new();
    for s in 0..n {
        let a0 = angles[s];
        let mut a1 = angles[(s + 1) % n];
        if a1 <= a0 {
            a1 += std::f64::consts::TAU;
        }
        let mut r0 = 0.0;
        for k in 0..=rings {
            let r1 = if k == rings { 1.0e4 } else { ring * (k as f64 + 1.0 + (hash(sd, (s * 8 + k) as u32, 3) - 0.5) * jitter * 0.8) };
            cells.push((a0, a1, r0, r1.max(r0 + 1e-3)));
            r0 = r1;
        }
    }
    cells.truncate(MAX_SHARDS);
    // Centroids of the shards clipped to the frame, on a grid.
    let g = 64usize;
    let mut sum = vec![[0.0f64; 3]; cells.len()];
    for iy in 0..g {
        for ix in 0..g {
            let x = ((ix as f64 + 0.5) / g as f64 - 0.5) * aspect;
            let y = (iy as f64 + 0.5) / g as f64 - 0.5;
            let (dx, dy) = (x - center[0], y - center[1]);
            let (r, th) = ((dx * dx + dy * dy).sqrt(), dy.atan2(dx).rem_euclid(std::f64::consts::TAU));
            if let Some(j) = cells.iter().position(|&(a0, a1, r0, r1)| {
                let d = (th - a0).rem_euclid(std::f64::consts::TAU);
                d < a1 - a0 && r >= r0 && r < r1
            }) {
                sum[j][0] += x;
                sum[j][1] += y;
                sum[j][2] += 1.0;
            }
        }
    }
    let start = num(e, "start") as f64;
    let gamma = num(e, "gamma") as f64;
    let ex = (f.t - start).max(0.0).powf(gamma);
    let fov = (num(e, "fov") as f64).to_radians();
    let focal = 0.5 / (fov * 0.5).tan();
    let motion = e.params.get("motion").cloned().unwrap_or_default();
    let (spin, spread, depth) = (num(e, "spin") as f64, pct(num(e, "spread") as f64) as f64, pct(num(e, "depth") as f64) as f64);
    let drift = pos(e, "drift").map(f64::from);
    let pan = pos(e, "pan").map(f64::from);
    let zoom = (num(e, "zoom") as f64 * ex).exp();
    let roll = (num(e, "roll") as f64 * ex).to_radians();
    let shade_k = num(e, "shade") as f64;
    let twist = num(e, "twist") as f64;
    // Camera: magnification, principal point shift, roll about the axis.
    let kc: M3 = [[focal * zoom, 0.0, pan[0] * ex], [0.0, focal * zoom, pan[1] * ex], [0.0, 0.0, 1.0]];
    let (rs, rc) = roll.sin_cos();
    let rr: M3 = [[rc, -rs, 0.0], [rs, rc, 0.0], [0.0, 0.0, 1.0]];
    let cam = mul(&kc, &rr);
    let mut out = Vec::new();
    for (j, &(a0, a1, r0, r1)) in cells.iter().enumerate() {
        let c = if sum[j][2] > 0.0 {
            [sum[j][0] / sum[j][2], sum[j][1] / sum[j][2]]
        } else {
            let mid = (a0 + a1) * 0.5;
            let r = if r1 > 100.0 { r0 + ring.max(0.2) } else { (r0 + r1) * 0.5 };
            [center[0] + mid.cos() * r, center[1] + mid.sin() * r]
        };
        let ju = j as u32;
        let explicit = motion.len() >= 5 * (j + 1) && motion.len() >= 5;
        let (phi, om, v) = if explicit {
            let m = &motion[5 * j..5 * j + 5];
            (m[0].to_radians(), m[1], [m[2] / 100.0, m[3] / 100.0, m[4] / 100.0])
        } else {
            let phi = hash(sd, ju, 11) * std::f64::consts::TAU;
            let sign = if hash(sd, ju, 12) < 0.5 { -1.0 } else { 1.0 };
            let om = spin * (0.5 + hash(sd, ju, 13)) * sign;
            let (dx, dy) = (c[0] - center[0], c[1] - center[1]);
            let l = (dx * dx + dy * dy).sqrt().max(1e-6);
            let sp = spread * (0.5 + hash(sd, ju, 14));
            let vz = depth * (0.6 + 0.8 * hash(sd, ju, 15));
            (phi, om, [dx / l * sp, dy / l * sp, vz])
        };
        let v = [v[0] + drift[0], v[1] + drift[1], v[2]];
        let r = rot_axis([phi.cos(), phi.sin(), 0.0], (om * ex).to_radians());
        // A turn in the sheet's plane (about the view axis), after the tilt.
        let tw = twist * (hash(sd, ju, 16) * 2.0 - 1.0) * ex;
        let r = if tw != 0.0 { mul(&rot_axis([0.0, 0.0, 1.0], tw.to_radians()), &r) } else { r };
        // A point (x, y) of the picture sits at (x, y, focal) in front of the
        // camera; the shard turns about its centre and moves (toward the
        // camera is -z).
        let cz = [c[0], c[1], focal];
        let t = [v[0] * ex, v[1] * ex, -v[2] * ex];
        let rc_ = [0, 1, 2].map(|i| r[i][0] * cz[0] + r[i][1] * cz[1] + r[i][2] * cz[2]);
        let col2 = [0, 1, 2].map(|i| r[i][2] * focal + cz[i] - rc_[i] + t[i]);
        let m: M3 = [[r[0][0], r[0][1], col2[0]], [r[1][0], r[1][1], col2[1]], [r[2][0], r[2][1], col2[2]]];
        let h = mul(&cam, &m);
        let Some(inv) = inverse(&h) else { continue };
        let big = inv.iter().flatten().fold(0.0f64, |a, v| a.max(v.abs()));
        if !(big.is_finite() && big > 0.0) {
            continue;
        }
        let depth_now = cz[2] + t[2];
        if depth_now <= 1e-3 {
            continue;
        }
        // Scale at the centre: picture units per output unit.
        let q = [0, 1, 2].map(|i| h[i][0] * c[0] + h[i][1] * c[1] + h[i][2]);
        let w = q[2];
        let jac = (det(&h) / (w * w * w)).abs();
        let scale = if jac > 1e-12 { 1.0 / jac.sqrt() } else { 1.0 };
        let shade = 1.0 + shade_k * (r[2][2].abs() - 1.0);
        let mut flat = [0.0; 9];
        for i in 0..3 {
            for k in 0..3 {
                flat[i * 3 + k] = inv[i][k] / big;
            }
        }
        out.push(Shard { a0, a1, r0, r1, inv: flat, depth: depth_now, shade, scale });
    }
    out.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    out
}

fn shatter(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let c = pos(e, "center");
    let col = rgb(e, "crack_color");
    let n = shards(e, f).len() as f32;
    Some(params(&[
        n,
        c[0],
        c[1],
        pct(num(e, "gap") as f64),
        num(e, "cracks"),
        pct(num(e, "crack_width") as f64),
        pct(num(e, "reach") as f64),
        col[0],
        col[1],
        col[2],
        pct(num(e, "jag") as f64),
        seed(e),
        num(e, "glint"),
    ]))
}

/// `v` rounded to 11 significant bits, so that a half float holds it exactly
/// (in the half float's normal range).
fn round11(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return 0.0;
    }
    let q = 2f64.powi(v.abs().log2().floor() as i32 - 10);
    (v / q).round() * q
}

/// A number as two half floats whose sum is the number to about 22 bits.
fn split(v: f64) -> (f32, f32) {
    let hi = round11(v);
    (hi as f32, round11(v - hi) as f32)
}

/// The table `kind` binds at this instant, if it binds one.
pub(crate) fn table(kind: &str, e: &ResolvedEffect, f: &Frame) -> Option<Table> {
    if kind != "shatter" {
        return None;
    }
    let shards = shards(e, f);
    let mut t: Table = Vec::with_capacity(ROW * shards.len().max(1));
    for s in &shards {
        let mut hi = [[0.0f32; 3]; 5];
        let mut lo = [[0.0f32; 3]; 5];
        let rows = [
            [s.inv[0], s.inv[1], s.inv[2]],
            [s.inv[3], s.inv[4], s.inv[5]],
            [s.inv[6], s.inv[7], s.inv[8]],
            [s.a0, s.a1, s.shade],
            [s.r0.min(60000.0), s.r1.min(60000.0), s.scale.clamp(1e-3, 1e3)],
        ];
        for (k, row) in rows.iter().enumerate() {
            for i in 0..3 {
                let (h, l) = split(row[i]);
                hi[k][i] = h;
                lo[k][i] = l;
            }
        }
        for k in 0..5 {
            t.push(hi[k]);
            t.push(lo[k]);
        }
    }
    if shards.is_empty() {
        t.resize(ROW, [0.0; 3]);
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn effect(params: &[(&str, Vec<f64>)]) -> ResolvedEffect {
        let spec = edits_core::effects::spec("shatter").unwrap();
        let mut p: BTreeMap<String, Vec<f64>> = spec.params.iter().map(|q| (q.name.to_string(), q.default.to_vec())).collect();
        for (k, v) in params {
            p.insert(k.to_string(), v.clone());
        }
        ResolvedEffect { kind: "shatter".into(), source: None, params: p, options: BTreeMap::new() }
    }

    #[test]
    fn before_the_start_every_shard_is_the_identity() {
        let e = effect(&[("start", vec![2.0])]);
        let s = shards(&e, &Frame { h: 100.0, aspect: 1.0, t: 1.0 });
        assert_eq!(s.len(), 7);
        for sh in &s {
            let k = sh.inv[8];
            for (i, v) in sh.inv.iter().enumerate() {
                let want = if i % 4 == 0 { k } else { 0.0 };
                assert!((v - want).abs() < 1e-9, "{:?}", sh.inv);
            }
        }
    }

    #[test]
    fn explicit_angles_and_motion_are_used() {
        let e = effect(&[("angles", vec![0.0, 90.0, 180.0, 270.0]), ("motion", vec![0.0, 0.0, 10.0, 0.0, 0.0]), ("start", vec![0.0])]);
        let s = shards(&e, &Frame { h: 100.0, aspect: 1.0, t: 1.0 });
        assert_eq!(s.len(), 4);
        // The first shard (0..90 degrees) moved right by 0.1: its inverse maps x back by 0.1.
        let first = s.iter().find(|x| x.a0.abs() < 1e-9).unwrap();
        let k = first.inv[8];
        assert!((first.inv[2] / k + 0.1).abs() < 1e-9, "{:?}", first.inv);
    }

    #[test]
    fn the_split_keeps_22_bits() {
        for v in [0.123456789, -3.987654, 1234.5678, 1e-4] {
            let (h, l) = split(v);
            assert!(((h as f64 + l as f64) - v).abs() < v.abs() * 1e-6 + 1e-9, "{v}");
        }
    }
}
