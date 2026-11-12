//! Cheap per-frame signatures (the `probe` table) and thumbnails.

use crate::img::{resize_area, Frames, Gray};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FrameSig {
    pub frame: usize,
    /// Mean luminance of the whole frame, 0..255.
    pub lum: f32,
    /// Mean luminance of the central 70% of the frame, 0..255.
    pub lum_center: f32,
    /// Spatial standard deviation of luminance, 0..255. Near 0 = a solid frame.
    pub contrast: f32,
    /// Mean HSV saturation of the centre, 0..255.
    pub sat: f32,
    /// Sharpness index: mean |Laplacian| / mean |gradient| in the centre. Lower = blurrier.
    pub sharp: f32,
    /// Mean absolute luminance difference to the previous frame, 0..255.
    pub diff: f32,
}

fn centre(w: usize, h: usize) -> (usize, usize, usize, usize) {
    let (mx, my) = ((w as f32 * 0.15) as usize, (h as f32 * 0.15) as usize);
    (mx.max(1), my.max(1), (w - mx).min(w - 1), (h - my).min(h - 1))
}

/// Sharpness index of a luminance image over its central region.
pub fn sharpness(g: &Gray) -> f32 {
    let (x0, y0, x1, y1) = centre(g.w, g.h);
    let (mut lap, mut grad) = (0.0f64, 0.0f64);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y * g.w + x;
            let c = g.d[i];
            lap += (g.d[i - 1] + g.d[i + 1] + g.d[i - g.w] + g.d[i + g.w] - 4.0 * c).abs() as f64;
            let gx = g.d[i + 1] - g.d[i - 1];
            let gy = g.d[i + g.w] - g.d[i - g.w];
            grad += ((gx * gx + gy * gy).sqrt() * 0.5) as f64;
        }
    }
    (lap / (grad + 1e-4 * ((x1 - x0) * (y1 - y0)) as f64)) as f32
}

/// Mean HSV saturation (0..1) of the central region of an RGB8 frame.
pub fn saturation(rgb: &[u8], w: usize, h: usize) -> f32 {
    let (x0, y0, x1, y1) = centre(w, h);
    let mut acc = 0.0f64;
    for y in y0..y1 {
        for x in x0..x1 {
            let p = &rgb[(y * w + x) * 3..(y * w + x) * 3 + 3];
            let mx = p[0].max(p[1]).max(p[2]) as f64;
            let mn = p[0].min(p[1]).min(p[2]) as f64;
            if mx > 0.0 {
                acc += (mx - mn) / mx;
            }
        }
    }
    (acc / ((x1 - x0) * (y1 - y0)) as f64) as f32
}

/// One row per frame. Runs in parallel.
pub fn signatures(frames: &Frames) -> Vec<FrameSig> {
    let n = frames.len();
    let (w, h) = (frames.w, frames.h);
    (0..n)
        .into_par_iter()
        .map(|i| {
            let g = frames.gray(i);
            let lum = g.mean();
            let contrast = g.std();
            let (x0, y0, x1, y1) = centre(w, h);
            let mut c = 0.0f64;
            for y in y0..y1 {
                c += g.d[y * w + x0..y * w + x1].iter().map(|v| *v as f64).sum::<f64>();
            }
            let lum_center = (c / ((x1 - x0) * (y1 - y0)) as f64) as f32;
            let diff = if i == 0 {
                0.0
            } else {
                let p = frames.gray(i - 1);
                g.d.iter().zip(&p.d).map(|(a, b)| (a - b).abs()).sum::<f32>() / g.d.len() as f32
            };
            FrameSig {
                frame: i,
                lum: lum * 255.0,
                lum_center: lum_center * 255.0,
                contrast: contrast * 255.0,
                sat: saturation(frames.rgb(i), w, h) * 255.0,
                sharp: sharpness(&g),
                diff: diff * 255.0,
            }
        })
        .collect()
}

/// Luminance thumbnails `size` px high.
pub fn thumbnails(frames: &Frames, size: usize) -> Vec<Gray> {
    let tw = ((frames.w as f64 * size as f64 / frames.h as f64).round() as usize).max(8);
    (0..frames.len()).into_par_iter().map(|i| resize_area(&frames.gray(i), tw, size)).collect()
}

/// Zero-mean, unit-norm copy of a thumbnail (a frame descriptor). `None` for a flat image.
pub fn znorm(g: &Gray) -> Option<Vec<f32>> {
    let m = g.mean();
    let n: f32 = g.d.iter().map(|v| (v - m) * (v - m)).sum::<f32>().sqrt();
    if n < 1e-3 * (g.d.len() as f32).sqrt() * 4.0 {
        return None;
    }
    Some(g.d.iter().map(|v| (v - m) / n).collect())
}
