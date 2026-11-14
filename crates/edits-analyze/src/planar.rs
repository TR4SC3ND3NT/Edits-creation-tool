//! Planar tracking of known pictures in a finished edit (`edits track`).
//!
//! When the reference shows the same pictures we have (the same arts, only
//! moved, graded and blurred), the most exact recipe is a trace: for every
//! frame, where each picture is (a homography), not what kind of motion it
//! roughly makes. Per picture and frame:
//!
//! 1. keypoints of the picture and of the frame (`features`), matched with
//!    the ratio test, and a homography by RANSAC (`homography::ransac`);
//! 2. the same homography refined on pixels, coarse to fine, against the
//!    frame (`homography::refine`, an ECC-style Gauss-Newton with gain,
//!    offset and robust weights); the previous frame's result, refined the
//!    same way, competes with it, and the better correlation wins;
//! 3. a normalised cross-correlation check: below `min_ncc` the frame is
//!    not trusted;
//! 4. a backward pass from trusted frames into the frames before them;
//! 5. gaps up to `max_gap` frames (motion blur breaks tracking) are filled
//!    by interpolating the corners, then the corners are smoothed with a
//!    Savitzky-Golay filter.
//!
//! The result maps each picture onto the frame, as homography keys and as
//! the four corners in composition units that a layer's `transform.pin`
//! takes (`edits_core::pin`).

use crate::features::{self, Features};
use crate::homography::{self, ArtPyramid, FrameStack};
use crate::img::{resize_area, Frames, Gray};
use edits_core::pin::{self, apply, Mat3};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "edits.track/1";

/// A picture to look for, already decoded.
pub struct Art {
    pub name: String,
    pub path: String,
    pub w: usize,
    pub h: usize,
    /// Luma 0..1 at full size.
    pub gray: Gray,
    /// Frame ranges (inclusive) to look in; empty = every frame.
    pub windows: Vec<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub struct TrackOpts {
    /// Correlation needed to trust a frame.
    pub min_ncc: f64,
    /// Longest run of untrusted frames filled by interpolation.
    pub max_gap: usize,
    /// Savitzky-Golay window (odd); 0 or 1 = no smoothing.
    pub smooth: usize,
    /// Leave near-black frame pixels out of the refinement (vignettes, letterbox).
    pub ignore_dark: bool,
    /// Leave saturated red frame pixels out (red overlays drawn over the picture).
    pub ignore_red: bool,
    /// Keypoints per frame and per picture.
    pub features: usize,
    pub seed: u64,
}

impl Default for TrackOpts {
    fn default() -> Self {
        Self { min_ncc: 0.5, max_gap: 8, smooth: 5, ignore_dark: false, ignore_red: false, features: 1500, seed: 1 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Track {
    pub format: String,
    pub video: VideoMeta,
    pub arts: Vec<ArtTrack>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VideoMeta {
    pub path: String,
    pub width: usize,
    pub height: usize,
    pub fps: f64,
    pub frames: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArtTrack {
    pub name: String,
    pub path: String,
    pub size: [usize; 2],
    pub windows: Vec<[usize; 2]>,
    /// Frames where the picture was found (or filled), ascending.
    pub keys: Vec<TrackKey>,
    /// Frames of the windows where it was not.
    pub missing: Vec<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TrackKey {
    pub frame: usize,
    /// Seconds: frame / fps, the instant the frame shows.
    pub t: f64,
    /// Picture pixels -> frame pixels, row-major, pixel-edge coordinates
    /// ((0, 0) is the top-left corner of the top-left pixel).
    pub h: [f64; 9],
    /// The picture's corners in composition units (height 1, origin at the
    /// centre, y down): top-left, top-right, bottom-right, bottom-left.
    /// `None` when a corner is past the horizon (use `pin` then).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corners: Option<[f64; 8]>,
    /// The same map as a 3x3 pin matrix: picture coordinates 0..1 -> composition units.
    pub pin: [f64; 9],
    pub ncc: f64,
    /// Keypoint inliers behind the estimate (0 when it came from the neighbour).
    pub inliers: usize,
    /// `features`, `propagated` (refined from a neighbouring frame) or `filled`.
    pub method: String,
}

/// Read a picture (PNG or JPEG) as an [`Art`].
pub fn load_art(path: &std::path::Path, name: &str, windows: Vec<(usize, usize)>) -> anyhow::Result<Art> {
    use anyhow::Context;
    let img = image::open(path).with_context(|| format!("reading {}", path.display()))?.to_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let d = img.pixels().map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0).collect();
    Ok(Art { name: name.into(), path: path.display().to_string(), w, h, gray: Gray { w, h, d }, windows })
}

/// Mask of usable frame pixels.
fn frame_mask(frames: &Frames, i: usize, o: &TrackOpts) -> Vec<bool> {
    frames
        .rgb(i)
        .chunks_exact(3)
        .map(|p| {
            let (r, g, b) = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            !(o.ignore_dark && lum < 0.06) && !(o.ignore_red && r - g.max(b) > 0.25)
        })
        .collect()
}

fn upscale2(g: &Gray) -> Gray {
    let (w, h) = (g.w * 2, g.h * 2);
    let mut o = Gray::new(w, h);
    for y in 0..h {
        for x in 0..w {
            o.d[y * w + x] = g.sample(((x as f32 + 0.5) / 2.0 - 0.5).max(0.0), ((y as f32 + 0.5) / 2.0 - 0.5).max(0.0)).unwrap_or_else(|| g.at((x / 2).min(g.w - 1), (y / 2).min(g.h - 1)));
        }
    }
    o
}

/// Frame features, in frame pixel-centre coordinates. Small frames are
/// detected at twice their size, so small details of the picture still match.
fn frame_features(g: &Gray, n: usize) -> Features {
    if g.h < 720 {
        let mut f = features::detect(&upscale2(g), n);
        for p in &mut f.points {
            p.x = (p.x + 0.5) / 2.0 - 0.5;
            p.y = (p.y + 0.5) / 2.0 - 0.5;
        }
        f
    } else {
        features::detect(g, n)
    }
}

struct Prepared {
    feats: Features,
    /// Picture full-size pixels per feature-image pixel.
    feat_scale: f64,
    pyr: ArtPyramid,
}

fn prepare(a: &Art, n: usize) -> Prepared {
    let long = a.w.max(a.h) as f64;
    let fs = (1400.0 / long).min(1.0);
    let small = if fs < 1.0 { resize_area(&a.gray, (a.w as f64 * fs).round() as usize, (a.h as f64 * fs).round() as usize) } else { a.gray.clone() };
    let feats = features::detect(&small, n * 2);
    let ps = (2048.0 / long).min(1.0);
    let base = if ps < 1.0 { resize_area(&a.gray, (a.w as f64 * ps).round() as usize, (a.h as f64 * ps).round() as usize) } else { a.gray.clone() };
    let pyr = ArtPyramid::new(&base, a.w, a.h, base.w as f64 / a.w as f64);
    Prepared { feats, feat_scale: a.w as f64 / small.w as f64, pyr }
}

/// The picture's quad in the frame is a convex quad of sensible size.
fn plausible(h: &Mat3, a: &Art, fw: usize, fh: usize) -> bool {
    let q: Option<Vec<[f64; 2]>> = [(0.0, 0.0), (a.w as f64, 0.0), (a.w as f64, a.h as f64), (0.0, a.h as f64)].iter().map(|(x, y)| apply(h, x - 0.5, y - 0.5)).collect();
    let Some(q) = q else { return false };
    let q = [q[0], q[1], q[2], q[3]];
    if !pin::convex(&q) {
        return false;
    }
    let area = 0.5 * (0..4).map(|i| q[i][0] * q[(i + 1) % 4][1] - q[(i + 1) % 4][0] * q[i][1]).sum::<f64>().abs();
    let fa = (fw * fh) as f64;
    area > fa * 0.002 && area < fa * 400.0
}

fn feature_estimate(p: &Prepared, a: &Art, ff: &Features, fw: usize, fh: usize, seed: u64) -> Option<(Mat3, usize)> {
    let m = features::match_ratio(&p.feats, ff, 0.8);
    if m.len() < 8 {
        return None;
    }
    let src: Vec<[f64; 2]> = m.iter().map(|(i, _)| {
        let k = &p.feats.points[*i];
        [(k.x as f64 + 0.5) * p.feat_scale - 0.5, (k.y as f64 + 0.5) * p.feat_scale - 0.5]
    }).collect();
    let dst: Vec<[f64; 2]> = m.iter().map(|(_, j)| [ff.points[*j].x as f64, ff.points[*j].y as f64]).collect();
    let thr = (fh as f64 / 576.0 * 3.0).max(2.0);
    let (h, inl) = homography::ransac(&src, &dst, thr, 3000, seed)?;
    let n = inl.iter().filter(|b| **b).count();
    (n >= 8 && plausible(&h, a, fw, fh)).then_some((h, n))
}

#[derive(Clone)]
struct Raw {
    h: Mat3,
    ncc: f64,
    inliers: usize,
    method: &'static str,
}

/// Track every picture through `frames`. `progress` gets (picture, frame).
pub fn track(frames: &Frames, video_path: &str, arts: &[Art], o: &TrackOpts, progress: &(dyn Fn(&str, usize) + Sync)) -> Track {
    let n = frames.len();
    let in_window = |a: &Art, i: usize| a.windows.is_empty() || a.windows.iter().any(|(s, e)| (*s..=*e).contains(&i));
    let needed: Vec<usize> = (0..n).filter(|i| arts.iter().any(|a| in_window(a, *i))).collect();
    let prepared: Vec<Prepared> = arts.par_iter().map(|a| prepare(a, o.features)).collect();
    // Frame keypoints, once per frame, in parallel.
    let feats: Vec<Option<Features>> = {
        let mut v: Vec<Option<Features>> = (0..n).map(|_| None).collect();
        let done: Vec<(usize, Features)> = needed.par_iter().map(|&i| (i, frame_features(&frames.gray(i), o.features))).collect();
        for (i, f) in done {
            v[i] = Some(f);
        }
        v
    };
    let mut out = Vec::new();
    for (ai, a) in arts.iter().enumerate() {
        let p = &prepared[ai];
        let windows: Vec<(usize, usize)> = if a.windows.is_empty() { vec![(0, n.saturating_sub(1))] } else { a.windows.iter().map(|(s, e)| (*s.min(&n.saturating_sub(1)), *e.min(&n.saturating_sub(1)))).collect() };
        let mut raw: Vec<Option<Raw>> = vec![None; n];
        let stack = |i: usize| FrameStack::new(&frames.gray(i), frame_mask(frames, i, o));
        for &(s, e) in &windows {
            // Forward: keypoints and the previous frame's result compete.
            for i in s..=e {
                progress(&a.name, i);
                let fr = stack(i);
                let mut best: Option<Raw> = None;
                let mut consider = |r: Option<homography::Refined>, inliers: usize, method: &'static str| {
                    if let Some(r) = r {
                        if r.ncc.is_finite() && plausible(&r.h, a, frames.w, frames.h) && best.as_ref().is_none_or(|b| r.ncc > b.ncc) {
                            best = Some(Raw { h: r.h, ncc: r.ncc, inliers, method });
                        }
                    }
                };
                if let Some(ff) = &feats[i] {
                    if let Some((h, ninl)) = feature_estimate(p, a, ff, frames.w, frames.h, o.seed ^ (i as u64 * 7919)) {
                        consider(homography::refine(&h, &p.pyr, &fr, 12), ninl, "features");
                    }
                }
                let prev = (s..i).rev().take(2).find_map(|j| raw[j].clone().filter(|r| r.ncc >= o.min_ncc));
                if let Some(pr) = prev {
                    consider(homography::refine(&pr.h, &p.pyr, &fr, 12), 0, "propagated");
                }
                raw[i] = best;
            }
            // Backward: from trusted frames into untrusted ones before them.
            for i in (s..e).rev() {
                if raw[i].as_ref().is_some_and(|r| r.ncc >= o.min_ncc) {
                    continue;
                }
                let Some(next) = raw[i + 1].clone().filter(|r| r.ncc >= o.min_ncc) else { continue };
                let fr = stack(i);
                if let Some(r) = homography::refine(&next.h, &p.pyr, &fr, 12) {
                    if r.ncc >= o.min_ncc && plausible(&r.h, a, frames.w, frames.h) && raw[i].as_ref().is_none_or(|b| r.ncc > b.ncc) {
                        raw[i] = Some(Raw { h: r.h, ncc: r.ncc, inliers: 0, method: "propagated" });
                    }
                }
            }
        }
        out.push(finish(a, &windows, raw, frames, o));
    }
    Track { format: FORMAT.into(), video: VideoMeta { path: video_path.into(), width: frames.w, height: frames.h, fps: frames.fps, frames: n }, arts: out }
}

/// The picture's corners in frame pixel-edge coordinates.
fn quad(h: &Mat3, a: &Art) -> Option<[f64; 8]> {
    let mut q = [0.0; 8];
    for (k, (x, y)) in [(0.0, 0.0), (a.w as f64, 0.0), (a.w as f64, a.h as f64), (0.0, a.h as f64)].iter().enumerate() {
        let p = apply(h, x - 0.5, y - 0.5)?;
        q[2 * k] = p[0] + 0.5;
        q[2 * k + 1] = p[1] + 0.5;
    }
    Some(q)
}

/// Savitzky-Golay smoothing (quadratic) of a sequence, window `w` (odd).
pub fn savgol(y: &[f64], w: usize) -> Vec<f64> {
    let n = y.len();
    if w < 3 || n < w {
        return y.to_vec();
    }
    let r = (w / 2) as isize;
    // Quadratic least-squares fit over the window, evaluated at the point
    // (at the ends, at its offset inside the first or last full window).
    (0..n as isize)
        .map(|i| {
            let c = i.clamp(r, n as isize - 1 - r);
            let mut ata = vec![0.0; 9];
            let mut atb = vec![0.0; 3];
            for k in -r..=r {
                let x = k as f64;
                let row = [1.0, x, x * x];
                let v = y[(c + k) as usize];
                for a in 0..3 {
                    atb[a] += row[a] * v;
                    for b in 0..3 {
                        ata[a * 3 + b] += row[a] * row[b];
                    }
                }
            }
            match homography::solve(ata, atb, 3) {
                Some(cf) => {
                    let x = (i - c) as f64;
                    cf[0] + cf[1] * x + cf[2] * x * x
                }
                None => y[i as usize],
            }
        })
        .collect()
}

fn finish(a: &Art, windows: &[(usize, usize)], raw: Vec<Option<Raw>>, frames: &Frames, o: &TrackOpts) -> ArtTrack {
    let (fw, fh) = (frames.w as f64, frames.h as f64);
    let mut keys: Vec<TrackKey> = Vec::new();
    let mut missing = Vec::new();
    for &(s, e) in windows {
        // Trusted corners per frame of the window.
        let mut q: Vec<Option<([f64; 8], &Raw)>> = (s..=e).map(|i| raw[i].as_ref().filter(|r| r.ncc >= o.min_ncc).and_then(|r| quad(&r.h, a).map(|c| (c, r)))).collect();
        let len = q.len();
        let mut method: Vec<&'static str> = q.iter().map(|x| x.map(|(_, r)| r.method).unwrap_or("")).collect();
        let mut filled: Vec<Option<[f64; 8]>> = q.iter().map(|x| x.map(|(c, _)| c)).collect();
        // Fill short gaps between trusted frames.
        let mut i = 0;
        while i < len {
            if filled[i].is_some() {
                i += 1;
                continue;
            }
            let start = i;
            while i < len && filled[i].is_none() {
                i += 1;
            }
            if start > 0 && i < len && i - start <= o.max_gap {
                let (c0, c1) = (filled[start - 1].unwrap(), filled[i].unwrap());
                for k in start..i {
                    let u = (k - start + 1) as f64 / (i - start + 1) as f64;
                    let mut c = [0.0; 8];
                    for j in 0..8 {
                        c[j] = c0[j] + (c1[j] - c0[j]) * u;
                    }
                    filled[k] = Some(c);
                    method[k] = "filled";
                }
            }
        }
        // Smooth each run of known frames.
        let mut k = 0;
        while k < len {
            if filled[k].is_none() {
                k += 1;
                continue;
            }
            let st = k;
            while k < len && filled[k].is_some() {
                k += 1;
            }
            if o.smooth >= 3 && k - st >= o.smooth {
                for j in 0..8 {
                    let ys: Vec<f64> = (st..k).map(|m| filled[m].unwrap()[j]).collect();
                    let sm = savgol(&ys, o.smooth | 1);
                    for (m, v) in (st..k).zip(sm) {
                        if let Some(c) = filled[m].as_mut() {
                            c[j] = v;
                        }
                    }
                }
            }
        }
        for (off, c) in filled.iter().enumerate() {
            let f = s + off;
            let Some(c) = c else {
                missing.push(f);
                continue;
            };
            let qc = [[c[0], c[1]], [c[2], c[3]], [c[4], c[5]], [c[6], c[7]]];
            // Picture 0..1 -> frame pixel edges.
            let Some(sq) = pin::square_to_quad(qc) else {
                missing.push(f);
                continue;
            };
            let to_comp: Mat3 = [[1.0 / fh, 0.0, -fw * 0.5 / fh], [0.0, 1.0 / fh, -0.5], [0.0, 0.0, 1.0]];
            let pm = homography::normalise(pin::mul(&to_comp, &sq)).unwrap_or(sq);
            let hm = homography::normalise(pin::mul(&sq, &[[1.0 / a.w as f64, 0.0, 0.0], [0.0, 1.0 / a.h as f64, 0.0], [0.0, 0.0, 1.0]])).unwrap_or(sq);
            let corners = pin::corners(&pm).map(|cs| [cs[0][0], cs[0][1], cs[1][0], cs[1][1], cs[2][0], cs[2][1], cs[3][0], cs[3][1]]);
            let r = q[off].take().map(|(_, r)| r);
            let round = |v: f64| (v * 1e6).round() / 1e6;
            keys.push(TrackKey {
                frame: f,
                t: round(f as f64 / frames.fps),
                h: flat(&hm).map(|v| (v * 1e9).round() / 1e9),
                corners: corners.map(|c| c.map(round)),
                pin: flat(&pm).map(|v| (v * 1e9).round() / 1e9),
                ncc: r.map(|r| (r.ncc * 1000.0).round() / 1000.0).unwrap_or(0.0),
                inliers: r.map(|r| r.inliers).unwrap_or(0),
                method: method[off].to_string(),
            });
        }
    }
    ArtTrack { name: a.name.clone(), path: a.path.clone(), size: [a.w, a.h], windows: windows.iter().map(|(s, e)| [*s, *e]).collect(), keys, missing }
}

fn flat(m: &Mat3) -> [f64; 9] {
    [m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2]]
}

/// Draw each tracked quad (and its diagonals) over a copy of the frame, for
/// looking at the track. Returns packed RGB.
pub fn overlay(frames: &Frames, i: usize, track: &Track) -> Vec<u8> {
    let (w, h) = (frames.w, frames.h);
    let mut rgb = frames.rgb(i).to_vec();
    let colours = [[0u8, 255, 0], [255, 220, 0], [0, 200, 255], [255, 0, 255], [255, 128, 0], [128, 255, 128]];
    for (ai, a) in track.arts.iter().enumerate() {
        let Some(k) = a.keys.iter().find(|k| k.frame == i) else { continue };
        let col = if k.method == "filled" { [255, 60, 60] } else { colours[ai % colours.len()] };
        let hm = [[k.h[0], k.h[1], k.h[2]], [k.h[3], k.h[4], k.h[5]], [k.h[6], k.h[7], k.h[8]]];
        let pts: Vec<[f64; 2]> = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].iter().filter_map(|(u, v)| apply(&hm, u * a.size[0] as f64, v * a.size[1] as f64)).collect();
        if pts.len() < 4 {
            continue;
        }
        for (p, q) in [(0, 1), (1, 2), (2, 3), (3, 0), (0, 2), (1, 3)] {
            let (a0, b0) = (pts[p], pts[q]);
            let steps = ((b0[0] - a0[0]).abs().max((b0[1] - a0[1]).abs()) as usize).clamp(1, 20000);
            for s in 0..=steps {
                let u = s as f64 / steps as f64;
                let (x, y) = (a0[0] + (b0[0] - a0[0]) * u, a0[1] + (b0[1] - a0[1]) * u);
                if x >= 0.0 && y >= 0.0 && (x as usize) < w && (y as usize) < h {
                    let o = (y as usize * w + x as usize) * 3;
                    rgb[o..o + 3].copy_from_slice(&col);
                }
            }
        }
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture with texture at every scale: blobs, bars and rings.
    fn picture(w: usize, h: usize) -> Gray {
        let mut g = Gray::new(w, h);
        let mut s = 12345u64;
        let mut rnd = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % 10000) as f64 / 10000.0
        };
        let shapes: Vec<[f64; 5]> = (0..90).map(|_| [rnd() * w as f64, rnd() * h as f64, 3.0 + rnd() * 30.0, rnd(), rnd()]).collect();
        for y in 0..h {
            for x in 0..w {
                let mut v = 0.5 + 0.15 * ((x as f64 * 0.05).sin() * (y as f64 * 0.031).cos());
                for sh in &shapes {
                    let d = ((x as f64 - sh[0]).powi(2) + (y as f64 - sh[1]).powi(2)).sqrt();
                    if sh[4] < 0.5 {
                        if d < sh[2] {
                            v = sh[3];
                        }
                    } else if (d - sh[2]).abs() < 2.0 {
                        v = 1.0 - sh[3];
                    }
                }
                g.d[y * w + x] = v as f32;
            }
        }
        g
    }

    fn render(art: &Gray, hm: &Mat3, w: usize, h: usize, gain: f32) -> Vec<u8> {
        let inv = pin::inverse(hm).unwrap();
        let mut out = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                // 2x2 supersampling.
                let mut acc = 0.0;
                let mut n = 0.0;
                for (dx, dy) in [(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)] {
                    if let Some(p) = apply(&inv, x as f64 + dx, y as f64 + dy) {
                        if let Some(v) = art.sample(p[0] as f32, p[1] as f32) {
                            acc += v;
                            n += 1.0;
                        }
                    }
                }
                let v = if n > 0.0 { acc / n * gain + 0.05 } else { 0.08 };
                let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                out[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&[b, b, b]);
            }
        }
        out
    }

    #[test]
    fn tracks_a_picture_under_moving_homographies() {
        let art = picture(480, 320);
        let (w, h) = (256, 256);
        let mut frames = Frames::new(w, h, 24.0);
        let mut truth = Vec::new();
        for i in 0..6 {
            let t = i as f64;
            let q = [[40.0 + 3.0 * t, 60.0 - t], [220.0 - 2.0 * t, 50.0 + 2.0 * t], [230.0, 200.0 + t], [30.0 + t, 190.0 - 2.0 * t]];
            // Picture pixel-centre coordinates -> frame pixel-centre coordinates.
            let sq = pin::square_to_quad(q).unwrap();
            let hm = pin::mul(&sq, &[[1.0 / 479.0, 0.0, 0.0], [0.0, 1.0 / 319.0, 0.0], [0.0, 0.0, 1.0]]);
            frames.push(&render(&art, &hm, w, h, 0.8 - 0.05 * i as f32));
            truth.push(hm);
        }
        let a = Art { name: "pic".into(), path: "pic.png".into(), w: 480, h: 320, gray: art.clone(), windows: vec![] };
        let tr = track(&frames, "synthetic", &[a], &TrackOpts { smooth: 0, ..Default::default() }, &|_, _| {});
        let at = &tr.arts[0];
        assert_eq!(at.keys.len(), 6, "missing {:?}", at.missing);
        for k in &at.keys {
            let hm = [[k.h[0], k.h[1], k.h[2]], [k.h[3], k.h[4], k.h[5]], [k.h[6], k.h[7], k.h[8]]];
            for (u, v) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.5, 0.5)] {
                // Edge coordinates of the picture point vs the truth (centre coordinates).
                let p = apply(&hm, u * 480.0, v * 320.0).unwrap();
                let q = apply(&truth[k.frame], u * 480.0 - 0.5, v * 320.0 - 0.5).unwrap();
                let err = (p[0] - 0.5 - q[0]).hypot(p[1] - 0.5 - q[1]);
                assert!(err < 0.6, "frame {} corner ({u}, {v}) off by {err:.2} px ({})", k.frame, k.method);
            }
            assert!(k.ncc > 0.95, "ncc {}", k.ncc);
            let c = k.corners.unwrap();
            assert!(edits_core::pin::from_values(&c).is_ok());
        }
    }

    #[test]
    fn savgol_keeps_a_parabola() {
        let y: Vec<f64> = (0..12).map(|i| 0.3 * (i * i) as f64 - 2.0 * i as f64 + 1.0).collect();
        let s = savgol(&y, 5);
        for (a, b) in y.iter().zip(&s) {
            assert!((a - b).abs() < 1e-9);
        }
    }
}
