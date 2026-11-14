//! Measurements for the trace method, all as parameters, never pixels:
//!
//! - [`measure_grade`]: per tracked key, three tone curves (red, green,
//!   blue) that turn the clean picture into its look in the reference, by
//!   quantile matching over the pixels the picture covers, and the Gaussian
//!   blur that best explains the reference's softness there;
//! - [`measure_blur`]: per frame, the Gaussian, zoom and directional blur
//!   that bring a render closest (SSIM) to the reference;
//! - [`metric`]: per-frame SSIM of a render against the reference, at the
//!   reference's size, the worst frames on a sheet (reference | ours |
//!   difference) and the author-pixel audit: the share of mid-tone pixels
//!   that equal the reference's within 4/255, which stays near 10% for an
//!   edit built from parameters and goes far above it when reference
//!   pixels leak into the render.

use crate::contact;
use crate::img::{gaussian_blur, Frames, Gray};
use crate::media;
use crate::planar::{self, Track};
use anyhow::{bail, Result};
use edits_core::pin::{apply, inverse, Mat3};
use rayon::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// ---- SSIM -------------------------------------------------------------------

/// BT.601 luma in 0..255 (what OpenCV's grey conversion gives).
pub fn luma601(frames: &Frames, i: usize) -> Gray {
    let d = frames.rgb(i).chunks_exact(3).map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32).collect();
    Gray { w: frames.w, h: frames.h, d }
}

/// Mean SSIM of two grey images in 0..255: 11x11 Gaussian window, sigma 1.5,
/// C1 = (0.01 * 255)^2, C2 = (0.03 * 255)^2 (Wang et al. 2004).
pub fn ssim(a: &Gray, b: &Gray) -> f64 {
    let n = a.d.len();
    let mk = |f: &dyn Fn(usize) -> f32| Gray { w: a.w, h: a.h, d: (0..n).map(f).collect() };
    let ma = gaussian_blur(a, 1.5);
    let mb = gaussian_blur(b, 1.5);
    let saa = gaussian_blur(&mk(&|i| a.d[i] * a.d[i]), 1.5);
    let sbb = gaussian_blur(&mk(&|i| b.d[i] * b.d[i]), 1.5);
    let sab = gaussian_blur(&mk(&|i| a.d[i] * b.d[i]), 1.5);
    let (c1, c2) = (6.5025f64, 58.5225f64);
    let mut s = 0.0f64;
    for i in 0..n {
        let (mu_a, mu_b) = (ma.d[i] as f64, mb.d[i] as f64);
        let va = saa.d[i] as f64 - mu_a * mu_a;
        let vb = sbb.d[i] as f64 - mu_b * mu_b;
        let cov = sab.d[i] as f64 - mu_a * mu_b;
        s += ((2.0 * mu_a * mu_b + c1) * (2.0 * cov + c2)) / ((mu_a * mu_a + mu_b * mu_b + c1) * (va + vb + c2));
    }
    s / n.max(1) as f64
}

// ---- metric -----------------------------------------------------------------

pub struct MetricOpts {
    pub pre_blur: f64,
    pub worst: usize,
    /// (name, first, last) frame ranges to average over.
    pub blocks: Vec<(String, usize, usize)>,
    pub sheet: Option<PathBuf>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FrameMetric {
    pub frame: usize,
    pub ssim: f64,
    /// Mean absolute RGB difference, 0..255.
    pub color_error: f64,
    /// Share of the reference's mid-tone pixels that ours equals within 4/255
    /// on every channel; `None` when the frame has almost no mid-tones.
    pub author_share: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Metric {
    pub render: String,
    pub reference: String,
    pub size: [usize; 2],
    pub fps: f64,
    pub frames_compared: usize,
    pub pre_blur: f64,
    pub ssim_mean: f64,
    pub ssim_min: f64,
    pub color_error_mean: f64,
    /// Mean over frames with mid-tones, and the highest frame.
    pub author_share_mean: f64,
    pub author_share_max: f64,
    /// Frames whose author share is above 15%.
    pub author_frames_over_15: Vec<usize>,
    pub blocks: Vec<Value>,
    pub worst: Vec<FrameMetric>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    pub frames: Vec<FrameMetric>,
}

fn author_share(r: &[u8], o: &[u8]) -> Option<f64> {
    let (mut mid, mut same) = (0usize, 0usize);
    for (a, b) in r.chunks_exact(3).zip(o.chunks_exact(3)) {
        let l = 0.299 * a[0] as f32 + 0.587 * a[1] as f32 + 0.114 * a[2] as f32;
        if !(40.0..=215.0).contains(&l) {
            continue;
        }
        mid += 1;
        if (0..3).all(|c| (a[c] as i32 - b[c] as i32).abs() <= 4) {
            same += 1;
        }
    }
    (mid * 100 >= r.len() / 3).then(|| same as f64 / mid as f64)
}

/// Compare a render with the reference frame by frame at the reference's
/// size and frame rate.
pub fn metric(render: &Path, reference: &Path, o: &MetricOpts) -> Result<Metric> {
    let info = media::probe(reference)?;
    let rf = media::decode_frames(reference, &info, info.height)?;
    let of = media::decode_frames_as(render, rf.w, rf.h, rf.fps)?;
    let n = rf.len().min(of.len());
    if n == 0 {
        bail!("no frames to compare");
    }
    if of.len() + 1 < rf.len() {
        eprintln!("note: the render has {} frames at {} fps, the reference {}; comparing the first {n}", of.len(), rf.fps, rf.len());
    }
    let pre = o.pre_blur as f32;
    let frames: Vec<FrameMetric> = (0..n)
        .into_par_iter()
        .map(|i| {
            let (a, b) = (gaussian_blur(&luma601(&rf, i), pre), gaussian_blur(&luma601(&of, i), pre));
            let (ra, ob) = (rf.rgb(i), of.rgb(i));
            let ce = ra.iter().zip(ob).map(|(x, y)| (*x as i32 - *y as i32).abs() as f64).sum::<f64>() / ra.len() as f64;
            FrameMetric { frame: i, ssim: r4(ssim(&a, &b)), color_error: r2(ce), author_share: author_share(ra, ob).map(r4) }
        })
        .collect();
    let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    let ss: Vec<f64> = frames.iter().map(|f| f.ssim).collect();
    let au: Vec<f64> = frames.iter().filter_map(|f| f.author_share).collect();
    let mut order: Vec<&FrameMetric> = frames.iter().collect();
    order.sort_by(|a, b| a.ssim.partial_cmp(&b.ssim).unwrap());
    let worst: Vec<FrameMetric> = order.iter().take(o.worst).map(|f| (*f).clone()).collect();
    let blocks = o
        .blocks
        .iter()
        .map(|(name, s, e)| {
            let v: Vec<f64> = frames.iter().filter(|f| (*s..=*e).contains(&f.frame)).map(|f| f.ssim).collect();
            json!({ "name": name, "frames": [s, e], "ssim": r4(mean(&v)) })
        })
        .collect();
    let mut sheet = None;
    if let Some(path) = &o.sheet {
        let cells: Vec<contact::Cell> = worst
            .iter()
            .flat_map(|f| {
                let i = f.frame;
                let a = contact::frame_image(&rf, i);
                let b = contact::frame_image(&of, i);
                let mut d = a.clone();
                for (p, (x, y)) in d.pixels_mut().zip(a.pixels().zip(b.pixels())) {
                    for c in 0..3 {
                        p[c] = ((x[c] as i32 - y[c] as i32).abs() * 3).min(255) as u8;
                    }
                }
                let h = rf.h.min(288) as u32;
                [
                    contact::Cell { image: contact::fit_height(&a, h), label: format!("ref f{i}"), accent: Some([90, 90, 230]) },
                    contact::Cell { image: contact::fit_height(&b, h), label: format!("ours f{i} ssim {:.3}", f.ssim), accent: Some([230, 160, 40]) },
                    contact::Cell { image: contact::fit_height(&d, h), label: "|diff| x3".into(), accent: None },
                ]
            })
            .collect();
        contact::save(&contact::compose(&cells, 3, 4, Some("worst frames: reference | ours | difference")), path)?;
        sheet = Some(path.display().to_string());
    }
    Ok(Metric {
        render: render.display().to_string(),
        reference: reference.display().to_string(),
        size: [rf.w, rf.h],
        fps: rf.fps,
        frames_compared: n,
        pre_blur: o.pre_blur,
        ssim_mean: r4(mean(&ss)),
        ssim_min: r4(ss.iter().cloned().fold(1.0, f64::min)),
        color_error_mean: r2(mean(&frames.iter().map(|f| f.color_error).collect::<Vec<_>>())),
        author_share_mean: r4(mean(&au)),
        author_share_max: r4(au.iter().cloned().fold(0.0, f64::max)),
        author_frames_over_15: frames.iter().filter(|f| f.author_share.is_some_and(|a| a > 0.15)).map(|f| f.frame).collect(),
        blocks,
        worst,
        sheet,
        frames,
    })
}

pub fn metric_report(m: &Metric) -> String {
    let mut s = format!(
        "{} frames at {}x{}, pre-blur {} px\nSSIM mean {:.3}, min {:.3}; colour error {:.1}/255\nauthor pixels: mean {:.1}%, max {:.1}%, {} frames over 15%\n",
        m.frames_compared, m.size[0], m.size[1], m.pre_blur, m.ssim_mean, m.ssim_min, m.color_error_mean, m.author_share_mean * 100.0, m.author_share_max * 100.0, m.author_frames_over_15.len()
    );
    for b in &m.blocks {
        s += &format!("block {} {}: SSIM {}\n", b["name"].as_str().unwrap_or(""), b["frames"], b["ssim"]);
    }
    s += "worst frames:\n";
    for f in &m.worst {
        s += &format!("  f{:<4} ({:5.2} s)  SSIM {:.3}  colour error {:.1}\n", f.frame, f.frame as f64 / m.fps, f.ssim, f.color_error);
    }
    if let Some(p) = &m.sheet {
        s += &format!("sheet: {p}\n");
    }
    s
}

fn r4(v: f64) -> f64 {
    (v * 10000.0).round() / 10000.0
}
fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ---- grade ------------------------------------------------------------------

/// A picture as three channel pyramids (each level half the previous).
struct RgbPyr {
    full: (usize, usize),
    levels: Vec<([Gray; 3], f64)>,
}

fn rgb_pyramid(path: &Path) -> Result<RgbPyr> {
    let img = image::open(path)?.to_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let ch = |c: usize| Gray { w, h, d: img.pixels().map(|p| p[c] as f32 / 255.0).collect() };
    let mut cur = [ch(0), ch(1), ch(2)];
    let mut s = 1.0;
    let mut levels = Vec::new();
    loop {
        let small = cur[0].w < 64 || cur[0].h < 64 || levels.len() > 10;
        levels.push((cur.clone(), s));
        if small {
            break;
        }
        let nxt = [crate::img::half(&cur[0]), crate::img::half(&cur[1]), crate::img::half(&cur[2])];
        s *= nxt[0].w as f64 / cur[0].w as f64;
        cur = nxt;
    }
    Ok(RgbPyr { full: (w, h), levels })
}

/// The picture warped into a `w` x `h` frame by `hm` (picture pixel edges ->
/// frame pixel edges), sampled from the pyramid level that matches its size.
/// Channels 0..1 and a coverage mask.
fn warp(p: &RgbPyr, hm: &Mat3, w: usize, h: usize) -> Option<([Gray; 3], Vec<bool>)> {
    let inv = inverse(hm)?;
    let c = apply(hm, p.full.0 as f64 * 0.5, p.full.1 as f64 * 0.5)?;
    let a = apply(&inv, c[0], c[1])?;
    let b = apply(&inv, c[0] + 1.0, c[1] + 1.0)?;
    let scale = ((b[0] - a[0]).abs() * (b[1] - a[1]).abs()).sqrt().max(1e-6);
    let lvl = (scale.log2().floor().max(0.0) as usize).min(p.levels.len() - 1);
    let (ch, ls) = &p.levels[lvl];
    let mut out = [Gray::new(w, h), Gray::new(w, h), Gray::new(w, h)];
    let mut mask = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let Some(q) = apply(&inv, x as f64 + 0.5, y as f64 + 0.5) else { continue };
            let (lx, ly) = (q[0] * ls - 0.5, q[1] * ls - 0.5);
            if !(lx >= 0.0 && ly >= 0.0 && lx <= (ch[0].w - 1) as f64 && ly <= (ch[0].h - 1) as f64) {
                continue;
            }
            for k in 0..3 {
                out[k].d[y * w + x] = ch[k].sample(lx as f32, ly as f32).unwrap_or(0.0);
            }
            mask[y * w + x] = true;
        }
    }
    Some((out, mask))
}

fn erode(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let mut out = mask.to_vec();
    for y in 0..h {
        for x in 0..w {
            if !mask[y * w + x] {
                continue;
            }
            'k: for dy in -(r as i64)..=r as i64 {
                for dx in -(r as i64)..=r as i64 {
                    let (xx, yy) = (x as i64 + dx, y as i64 + dy);
                    if xx < 0 || yy < 0 || xx >= w as i64 || yy >= h as i64 || !mask[yy as usize * w + xx as usize] {
                        out[y * w + x] = false;
                        break 'k;
                    }
                }
            }
        }
    }
    out
}

/// Clear `mask` inside a convex quad (frame pixel edges), grown by `grow` pixels.
fn cover_quad(mask: &mut [bool], w: usize, h: usize, q: &[[f64; 2]], grow: f64) {
    let c = [(q[0][0] + q[1][0] + q[2][0] + q[3][0]) / 4.0, (q[0][1] + q[1][1] + q[2][1] + q[3][1]) / 4.0];
    let x0 = q.iter().map(|p| p[0]).fold(f64::MAX, f64::min).floor().max(0.0) as usize;
    let x1 = (q.iter().map(|p| p[0]).fold(f64::MIN, f64::max).ceil().max(0.0) as usize).min(w);
    let y0 = q.iter().map(|p| p[1]).fold(f64::MAX, f64::min).floor().max(0.0) as usize;
    let y1 = (q.iter().map(|p| p[1]).fold(f64::MIN, f64::max).ceil().max(0.0) as usize).min(h);
    for y in y0.saturating_sub(grow as usize)..(y1 + grow as usize).min(h) {
        for x in x0.saturating_sub(grow as usize)..(x1 + grow as usize).min(w) {
            let p = [x as f64 + 0.5, y as f64 + 0.5];
            let inside = (0..4).all(|i| {
                let (a, b) = (q[i], q[(i + 1) % 4]);
                let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
                let len = ex.hypot(ey).max(1e-9);
                // Signed distance to the edge, positive on the centre's side.
                let side = |pt: [f64; 2]| (ex * (pt[1] - a[1]) - ey * (pt[0] - a[0])) / len;
                side(p) * side(c).signum() >= -grow
            });
            if inside {
                mask[y * w + x] = false;
            }
        }
    }
}

fn quantiles(v: &mut [f32], n: usize) -> Vec<f64> {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (0..n).map(|k| v[((k as f64 / (n - 1) as f64) * (v.len() - 1) as f64).round() as usize] as f64).collect()
}

fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    if x <= xs[0] {
        return ys[0];
    }
    for i in 1..xs.len() {
        if x <= xs[i] {
            let u = if xs[i] > xs[i - 1] { (x - xs[i - 1]) / (xs[i] - xs[i - 1]) } else { 1.0 };
            return ys[i - 1] + (ys[i] - ys[i - 1]) * u;
        }
    }
    *ys.last().unwrap()
}

#[derive(Serialize, Clone, Debug)]
pub struct GradeKey {
    pub frame: usize,
    pub t: f64,
    /// `curves` points [x0, y0, x1, y1, ...] per channel, display values 0..1.
    pub red: Vec<f64>,
    pub green: Vec<f64>,
    pub blue: Vec<f64>,
    /// Gaussian blur that best explains the reference here, % of height (the `gaussian_blur` radius).
    pub blur: f64,
    /// Mean absolute error left after the curves, 0..255.
    pub error: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct ArtGrade {
    pub art: String,
    pub keys: Vec<GradeKey>,
    /// Ready to paste into the picture's layer: a `curves` effect and a `gaussian_blur`, keyed in seconds.
    pub effects: Vec<Value>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Grade {
    pub format: String,
    pub video: String,
    pub arts: Vec<ArtGrade>,
}

/// Monotone curve through matched quantiles, resampled to `points` evenly
/// spaced inputs (with the ends at 0 and 1).
fn curve(src: &[f64], dst: &[f64], points: usize) -> Vec<f64> {
    let mut xs = src.to_vec();
    for i in 1..xs.len() {
        if xs[i] <= xs[i - 1] {
            xs[i] = xs[i - 1] + 1e-4;
        }
    }
    let mut ys = dst.to_vec();
    for i in 1..ys.len() {
        ys[i] = ys[i].max(ys[i - 1]);
    }
    let mut out = Vec::with_capacity(points * 2);
    for k in 0..points {
        let x = k as f64 / (points - 1) as f64;
        out.push(r4(x));
        out.push(r4(interp(&xs, &ys, x).clamp(0.0, 1.0)));
    }
    out
}

/// Tone curves and blur per tracked key (every `every`th found key).
pub fn measure_grade(video: &Path, tr: &Track, points: usize, every: usize) -> Result<Grade> {
    let info = media::probe(video)?;
    let frames = media::decode_frames(video, &info, info.height)?;
    let (w, h) = (frames.w, frames.h);
    let points = points.clamp(3, 33);
    let mut arts = Vec::new();
    for at in &tr.arts {
        let pyr = rgb_pyramid(Path::new(&at.path))?;
        let keys: Vec<&planar::TrackKey> = at.keys.iter().filter(|k| k.method != "filled" && k.frame < frames.len()).step_by(every).collect();
        let measured: Vec<GradeKey> = keys
            .par_iter()
            .filter_map(|k| {
                let hm = [[k.h[0], k.h[1], k.h[2]], [k.h[3], k.h[4], k.h[5]], [k.h[6], k.h[7], k.h[8]]];
                let (ch, mask) = warp(&pyr, &hm, w, h)?;
                let mut inner = erode(&mask, w, h, 3);
                // Smaller pictures tracked in the same frame lie over this one: leave them out.
                let area = k.corners.as_ref().map(quad_area).unwrap_or(f64::MAX);
                for other in tr.arts.iter().filter(|o| o.name != at.name) {
                    let Some(ok) = other.keys.iter().find(|x| x.frame == k.frame) else { continue };
                    if ok.corners.as_ref().map(quad_area).unwrap_or(f64::MAX) >= area {
                        continue;
                    }
                    let hm = [[ok.h[0], ok.h[1], ok.h[2]], [ok.h[3], ok.h[4], ok.h[5]], [ok.h[6], ok.h[7], ok.h[8]]];
                    let corners: Vec<[f64; 2]> = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].iter().filter_map(|(u, v)| apply(&hm, u * other.size[0] as f64, v * other.size[1] as f64)).collect();
                    if corners.len() == 4 {
                        cover_quad(&mut inner, w, h, &corners, 3.0);
                    }
                }
                let idx: Vec<usize> = (0..w * h).filter(|i| inner[*i]).collect();
                if idx.len() < 1500 {
                    return None;
                }
                let rgb = frames.rgb(k.frame);
                let q = 64;
                let refq: Vec<Vec<f64>> = (0..3).map(|c| quantiles(&mut idx.iter().map(|i| rgb[i * 3 + c] as f32 / 255.0).collect::<Vec<_>>(), q)).collect();
                let mut best: Option<(f64, f64, Vec<Vec<f64>>)> = None;
                for sigma in [0.0f32, 0.6, 1.0, 1.5, 2.2, 3.0, 4.0, 6.0] {
                    let b: Vec<Gray> = ch.iter().map(|g| gaussian_blur(g, sigma)).collect();
                    let srcq: Vec<Vec<f64>> = (0..3).map(|c| quantiles(&mut idx.iter().map(|i| b[c].d[*i]).collect::<Vec<_>>(), q)).collect();
                    let err = (0..3)
                        .map(|c| idx.iter().step_by(3).map(|i| (interp(&srcq[c], &refq[c], b[c].d[*i] as f64) - rgb[i * 3 + c] as f64 / 255.0).abs()).sum::<f64>())
                        .sum::<f64>()
                        / (3.0 * idx.len().div_ceil(3) as f64)
                        * 255.0;
                    if best.as_ref().is_none_or(|(e, _, _)| err < *e) {
                        best = Some((err, sigma as f64, srcq));
                    }
                }
                let (err, sigma, srcq) = best?;
                let c: Vec<Vec<f64>> = (0..3).map(|i| curve(&srcq[i], &refq[i], points)).collect();
                Some(GradeKey { frame: k.frame, t: k.t, red: c[0].clone(), green: c[1].clone(), blue: c[2].clone(), blur: r4(sigma / h as f64 * 100.0), error: r2(err) })
            })
            .collect();
        let keyed = |f: &dyn Fn(&GradeKey) -> Value| Value::from(measured.iter().map(|k| json!({ "t": k.t, "v": f(k) })).collect::<Vec<_>>());
        let effects = if measured.is_empty() {
            Vec::new()
        } else {
            vec![
                json!({ "kind": "curves", "params": { "red": { "keys": keyed(&|k| json!(k.red)) }, "green": { "keys": keyed(&|k| json!(k.green)) }, "blue": { "keys": keyed(&|k| json!(k.blue)) } } }),
                json!({ "kind": "gaussian_blur", "params": { "radius": { "keys": keyed(&|k| json!(k.blur)) } } }),
            ]
        };
        arts.push(ArtGrade { art: at.name.clone(), keys: measured, effects });
    }
    Ok(Grade { format: "edits.grade/1".into(), video: video.display().to_string(), arts })
}

// ---- blur -------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct BlurFrame {
    pub frame: usize,
    pub t: f64,
    /// SSIM before and after the blur.
    pub base: f64,
    pub best: f64,
    /// `gaussian_blur` radius, % of height (0 = none).
    pub gaussian: f64,
    /// `zoom_blur` strength and centre in composition units (strength 0 = none).
    pub zoom: [f64; 3],
    /// `directional_blur` length (% of height) and angle in degrees (length 0 = none).
    pub directional: [f64; 2],
}

#[derive(Serialize, Clone, Debug)]
pub struct BlurReport {
    pub format: String,
    pub render: String,
    pub reference: String,
    pub frames: Vec<BlurFrame>,
    pub mean_gain: f64,
}

fn sample_clamped(g: &Gray, x: f32, y: f32) -> f32 {
    g.sample(x.clamp(0.0, (g.w - 1) as f32), y.clamp(0.0, (g.h - 1) as f32)).unwrap_or(0.0)
}

/// The engine's `zoom_blur`: the average of the picture scaled between
/// e^(-s/2) and e^(s/2) about `c` (pixels).
pub fn zoom_blur(g: &Gray, s: f64, c: [f64; 2], n: usize) -> Gray {
    if s <= 0.0 {
        return g.clone();
    }
    let mut out = Gray::new(g.w, g.h);
    for k in 0..n {
        let f = (s * ((k as f64 + 0.5) / n as f64 - 0.5)).exp();
        for y in 0..g.h {
            for x in 0..g.w {
                let (sx, sy) = (c[0] + (x as f64 - c[0]) / f, c[1] + (y as f64 - c[1]) / f);
                out.d[y * g.w + x] += sample_clamped(g, sx as f32, sy as f32) / n as f32;
            }
        }
    }
    out
}

/// The engine's `directional_blur`: an even streak `len` pixels long in
/// total, centred on each pixel, at `angle` degrees (clockwise on screen).
pub fn directional_blur(g: &Gray, len: f64, angle: f64, n: usize) -> Gray {
    if len < 1.0 {
        return g.clone();
    }
    let (dx, dy) = (angle.to_radians().cos(), angle.to_radians().sin());
    let mut out = Gray::new(g.w, g.h);
    for k in 0..n {
        let o = len * ((k as f64 + 0.5) / n as f64 - 0.5);
        for y in 0..g.h {
            for x in 0..g.w {
                out.d[y * g.w + x] += sample_clamped(g, (x as f64 + o * dx) as f32, (y as f64 + o * dy) as f32) / n as f32;
            }
        }
    }
    out
}

/// Per frame, the blur that brings the render closest to the reference:
/// a Gaussian first, then a zoom blur on top, then a directional one.
/// Measured at half the reference's size.
pub fn measure_blur(render: &Path, reference: &Path, only: Option<&[usize]>, min_gain: f64) -> Result<BlurReport> {
    let info = media::probe(reference)?;
    let (w, h) = media::analysis_size(&info, (info.height / 2).max(64));
    let rf = media::decode_frames(reference, &info, h as u32)?;
    let of = media::decode_frames_as(render, w, h, rf.fps)?;
    let n = rf.len().min(of.len());
    let list: Vec<usize> = match only {
        Some(v) => v.iter().copied().filter(|i| *i < n).collect(),
        None => (0..n).collect(),
    };
    let hh = h as f64;
    let frames: Vec<BlurFrame> = list
        .par_iter()
        .map(|&i| {
            let (a, m) = (luma601(&rf, i), luma601(&of, i));
            let base = ssim(&a, &m);
            let mut best = (base, 0.0f64, [0.0f64; 3], [0.0f64; 2]);
            for g in [1.0f32, 2.0, 3.0, 5.0, 8.0] {
                let s = ssim(&a, &gaussian_blur(&m, g / 2.0));
                if s > best.0 {
                    best = (s, g as f64, [0.0; 3], [0.0; 2]);
                }
            }
            let mg = gaussian_blur(&m, best.1 as f32 / 2.0);
            let base_z = best.0;
            for st in [0.05, 0.1, 0.2, 0.35, 0.5] {
                for cy in [-0.17, 0.0, 0.17] {
                    for cx in [-0.17, 0.0, 0.17] {
                        let c = [w as f64 * 0.5 + cx * hh, hh * 0.5 + cy * hh];
                        let s = ssim(&a, &zoom_blur(&mg, st, c, 8));
                        if s > best.0 {
                            best = (s, best.1, [st, cx, cy], [0.0; 2]);
                        }
                    }
                }
            }
            let mz = if best.0 > base_z { zoom_blur(&mg, best.2[0], [w as f64 * 0.5 + best.2[1] * hh, hh * 0.5 + best.2[2] * hh], 8) } else { mg };
            for l in [3.0, 6.0, 12.0, 20.0] {
                for ang in [0.0, 30.0, 60.0, 90.0, 120.0, 150.0] {
                    let s = ssim(&a, &directional_blur(&mz, l, ang, 9));
                    if s > best.0 {
                        best = (s, best.1, best.2, [l, ang]);
                    }
                }
            }
            // Pixels at half size -> % of the height.
            BlurFrame {
                frame: i,
                t: r4(i as f64 / rf.fps),
                base: r4(base),
                best: r4(best.0),
                gaussian: r4(best.1 / 2.0 / hh * 100.0),
                zoom: [r4(best.2[0]), r4(best.2[1]), r4(best.2[2])],
                directional: [r4(best.3[0] / hh * 100.0), best.3[1]],
            }
        })
        .collect();
    let gain = if frames.is_empty() { 0.0 } else { frames.iter().map(|f| f.best - f.base).sum::<f64>() / frames.len() as f64 };
    Ok(BlurReport {
        format: "edits.blur/1".into(),
        render: render.display().to_string(),
        reference: reference.display().to_string(),
        frames: frames.into_iter().filter(|f| f.best - f.base >= min_gain).collect(),
        mean_gain: r4(gain),
    })
}

// ---- draft ------------------------------------------------------------------

/// What a traced draft needs besides the track.
pub struct DraftOpts {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Seconds.
    pub duration: f64,
    /// Path of the sound for `audio` (relative to the project), if any.
    pub audio: Option<String>,
    /// (bpm, beat offset in seconds, beat times in seconds): cuts within 1.5
    /// frames of a beat are moved onto it.
    pub beats: Option<(f64, f64, Vec<f64>)>,
    /// How a picture's path is written in the project (relative to it).
    pub rel: Box<dyn Fn(&str) -> String>,
    /// Motion blur samples (1 = none).
    pub samples: u32,
}

/// The run of a picture over consecutive frames: one layer of the draft.
struct Run<'a> {
    art: &'a planar::ArtTrack,
    keys: Vec<&'a planar::TrackKey>,
    area: f64,
}

fn quad_area(c: &[f64; 8]) -> f64 {
    0.5 * (0..4).map(|i| c[2 * i] * c[(2 * i + 3) % 8] - c[(2 * i + 2) % 8] * c[2 * i + 1]).sum::<f64>().abs()
}

/// A project that replays the trace: one image layer per run of tracked
/// frames, its picture pinned to the tracked corners frame by frame, with the
/// measured tone curves and blur keyed on it; larger pictures beneath
/// smaller ones. Returns the project and a short report.
pub fn draft(track: &Track, grade: Option<&Grade>, o: &DraftOpts) -> (Value, Value) {
    let fl = 1.0 / o.fps;
    let mut runs: Vec<Run> = Vec::new();
    for at in &track.arts {
        let mut cur: Vec<&planar::TrackKey> = Vec::new();
        for k in &at.keys {
            if cur.last().is_some_and(|p| k.frame != p.frame + 1) {
                runs.push(Run { art: at, keys: std::mem::take(&mut cur), area: 0.0 });
            }
            cur.push(k);
        }
        if !cur.is_empty() {
            runs.push(Run { art: at, keys: cur, area: 0.0 });
        }
    }
    for r in &mut runs {
        let a: Vec<f64> = r.keys.iter().filter_map(|k| k.corners.as_ref().map(quad_area)).collect();
        r.area = if a.is_empty() { 1.0 } else { a.iter().sum::<f64>() / a.len() as f64 };
    }
    runs.sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap());
    let snap = |t: f64| -> (f64, bool) {
        if let Some((_, _, beats)) = &o.beats {
            if let Some(b) = beats.iter().min_by(|x, y| (*x - t).abs().partial_cmp(&(*y - t).abs()).unwrap()) {
                if (b - t).abs() <= 1.5 * fl && (b - t).abs() > 1e-9 {
                    return (r4(*b), true);
                }
            }
        }
        (r4(t), false)
    };
    let mut layers = Vec::new();
    let mut slots = serde_json::Map::new();
    let mut snapped = 0;
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    let mut report_layers = Vec::new();
    for r in &runs {
        let name = &r.art.name;
        slots.insert(name.clone(), json!({ "path": (o.rel)(&r.art.path), "note": "traced picture" }));
        let n = counts.entry(name.clone()).or_insert(0);
        *n += 1;
        let id = format!("{name}_{n}");
        let (first, last) = (r.keys[0].frame, r.keys.last().unwrap().frame);
        let (t_in, s1) = snap(first as f64 * fl);
        let (t_out, s2) = snap((last + 1) as f64 * fl);
        snapped += s1 as usize + s2 as usize;
        // Corners when every key has them, else the 3x3 pin.
        let corners = r.keys.iter().all(|k| k.corners.is_some());
        let value = |k: &planar::TrackKey| -> Value { if corners { json!(k.corners.unwrap()) } else { json!(k.pin) } };
        let pin = if r.keys.len() == 1 { value(r.keys[0]) } else { json!({ "keys": r.keys.iter().map(|k| json!({ "t": k.t, "v": value(k) })).collect::<Vec<_>>() }) };
        let mut effects = Vec::new();
        if let Some(g) = grade.and_then(|g| g.arts.iter().find(|a| &a.art == name)) {
            let ks: Vec<&GradeKey> = g.keys.iter().filter(|k| (first..=last).contains(&k.frame)).collect();
            if !ks.is_empty() {
                // Per-frame measurements are noisy: a value that barely moves over the
                // run becomes one constant (its median), one that moves is a running
                // median over 5 keys, keyed every 3rd frame.
                let keyed = |f: &dyn Fn(&GradeKey) -> Vec<f64>, tol: f64| -> Value {
                    let vals: Vec<Vec<f64>> = ks.iter().map(|k| f(k)).collect();
                    let n = vals[0].len();
                    let med = |rows: &[Vec<f64>]| -> Vec<f64> { (0..n).map(|c| crate::img::median(&rows.iter().map(|r| r[c]).collect::<Vec<_>>())).map(r4).collect() };
                    let all = med(&vals);
                    let flat = vals.iter().all(|v| v.iter().zip(&all).all(|(a, b)| (a - b).abs() <= tol));
                    if ks.len() == 1 || flat {
                        return if n == 1 { json!(all[0]) } else { json!(all) };
                    }
                    let mut keys = Vec::new();
                    for i in (0..ks.len()).step_by(3).chain(std::iter::once(ks.len() - 1)) {
                        if keys.last().is_some_and(|(j, _): &(usize, Value)| *j == i) {
                            continue;
                        }
                        let (a, b) = (i.saturating_sub(2), (i + 3).min(ks.len()));
                        let m = med(&vals[a..b]);
                        keys.push((i, json!({ "t": ks[i].t, "v": if n == 1 { json!(m[0]) } else { json!(m) } })));
                    }
                    json!({ "keys": keys.into_iter().map(|(_, k)| k).collect::<Vec<_>>() })
                };
                effects.push(json!({ "kind": "curves", "params": { "red": keyed(&|k| k.red.clone(), 0.03), "green": keyed(&|k| k.green.clone(), 0.03), "blue": keyed(&|k| k.blue.clone(), 0.03) } }));
                if ks.iter().any(|k| k.blur > 0.0) {
                    effects.push(json!({ "kind": "gaussian_blur", "params": { "radius": keyed(&|k| vec![k.blur], 0.05) } }));
                }
            }
        }
        let mut layer = json!({ "id": id, "kind": "image", "slot": name, "in": t_in, "out": t_out, "transform": { "pin": pin } });
        if !effects.is_empty() {
            layer["effects"] = json!(effects);
        }
        report_layers.push(json!({ "id": id, "art": name, "frames": [first, last], "in": t_in, "out": t_out, "mean_ncc": r2(r.keys.iter().map(|k| k.ncc).sum::<f64>() / r.keys.len() as f64) }));
        layers.push(layer);
    }
    let mut settings = json!({
        "width": o.width, "height": o.height, "fps": o.fps, "duration": r4(o.duration), "time_base": "seconds",
        "background": [0, 0, 0], "motion_blur": { "samples": o.samples.max(1), "shutter": 0.5 }
    });
    if let Some((bpm, off, _)) = &o.beats {
        settings["bpm"] = json!(r4(*bpm));
        settings["beat_offset"] = json!(r4(*off));
    }
    let mut project = json!({ "format": edits_core::FORMAT, "settings": settings, "slots": slots, "layers": layers });
    if let Some(a) = &o.audio {
        project["audio"] = json!({ "path": a });
    }
    let not_found: Vec<&str> = track.arts.iter().filter(|a| a.keys.is_empty()).map(|a| a.name.as_str()).collect();
    let report = json!({ "layers": report_layers, "cuts_snapped_to_beats": snapped, "arts_not_found": not_found });
    (project, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssim_is_one_for_equal_images_and_drops_with_blur() {
        let mut g = Gray::new(64, 64);
        for (i, v) in g.d.iter_mut().enumerate() {
            *v = if (i / 64 / 8 + i % 64 / 8) % 2 == 0 { 200.0 } else { 40.0 };
        }
        assert!((ssim(&g, &g) - 1.0).abs() < 1e-9);
        let b = gaussian_blur(&g, 2.0);
        let s = ssim(&g, &b);
        assert!(s < 0.9 && s > 0.2, "{s}");
        // Directional and zoom blur keep a flat image flat.
        let flat = Gray { w: 32, h: 32, d: vec![0.5; 1024] };
        assert!(directional_blur(&flat, 8.0, 30.0, 9).d.iter().all(|v| (v - 0.5).abs() < 1e-5));
        assert!(zoom_blur(&flat, 0.3, [10.0, 12.0], 8).d.iter().all(|v| (v - 0.5).abs() < 1e-5));
    }

    #[test]
    fn author_share_counts_equal_mid_tones() {
        let r: Vec<u8> = (0..300).map(|i| (100 + i % 50) as u8).collect();
        assert_eq!(author_share(&r, &r), Some(1.0));
        let o: Vec<u8> = r.iter().map(|v| v + 20).collect();
        assert_eq!(author_share(&r, &o), Some(0.0));
        assert_eq!(author_share(&[0u8; 300], &[0u8; 300]), None);
    }

    #[test]
    fn a_draft_pins_each_run_and_snaps_cuts_to_beats() {
        let key = |frame: usize, x: f64| planar::TrackKey {
            frame,
            t: frame as f64 / 24.0,
            h: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            corners: Some([x - 0.3, -0.2, x + 0.3, -0.2, x + 0.3, 0.2, x - 0.3, 0.2]),
            pin: [0.6, 0.0, x - 0.3, 0.0, 0.4, -0.2, 0.0, 0.0, 1.0],
            ncc: 0.9,
            inliers: 20,
            method: "features".into(),
        };
        let art = planar::ArtTrack { name: "a".into(), path: "arts/a.png".into(), size: [600, 400], windows: vec![[0, 30]], keys: vec![key(0, 0.0), key(1, 0.01), key(2, 0.02), key(10, 0.1), key(11, 0.1)], missing: vec![] };
        let small = planar::ArtTrack { name: "b".into(), path: "arts/b.png".into(), size: [100, 100], windows: vec![], keys: vec![], missing: vec![] };
        let tr = Track { format: planar::FORMAT.into(), video: planar::VideoMeta { path: "ref.mp4".into(), width: 576, height: 576, fps: 24.0, frames: 30 }, arts: vec![art, small] };
        let o = DraftOpts { width: 576, height: 576, fps: 24.0, duration: 1.25, audio: None, beats: Some((120.0, 0.0, vec![0.0, 0.5, 1.0])), rel: Box::new(|p: &str| p.to_string()), samples: 4 };
        let (p, report) = draft(&tr, None, &o);
        let layers = p["layers"].as_array().unwrap();
        assert_eq!(layers.len(), 2, "{p}");
        let by_id = |id: &str| layers.iter().find(|l| l["id"] == id).unwrap().clone();
        assert_eq!(by_id("a_1")["transform"]["pin"]["keys"].as_array().unwrap().len().max(by_id("a_2")["transform"]["pin"]["keys"].as_array().unwrap().len()), 3);
        // Frame 10 starts at 0.4167 s, within 1.5 frames of no beat; frame 12 ends at 0.5 s, on a beat.
        assert!(layers.iter().any(|l| l["out"] == json!(0.5)), "{p}");
        assert_eq!(report["arts_not_found"], json!(["b"]));
        let project = edits_core::Project::from_json(&p.to_string()).unwrap();
        let findings = edits_core::check::check(&project, None);
        assert!(findings.iter().all(|f| f.severity != edits_core::check::Severity::Error), "{findings:?}");
    }

    #[test]
    fn curves_are_monotone_and_span_the_unit_range() {
        let src = vec![0.1, 0.2, 0.2, 0.5, 0.9];
        let dst = vec![0.0, 0.3, 0.25, 0.6, 0.8];
        let c = curve(&src, &dst, 5);
        assert_eq!(c.len(), 10);
        assert_eq!(c[0], 0.0);
        assert_eq!(c[8], 1.0);
        for k in 1..5 {
            assert!(c[2 * k + 1] >= c[2 * k - 1]);
        }
    }
}
