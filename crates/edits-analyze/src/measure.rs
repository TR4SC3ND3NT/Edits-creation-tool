//! A frame paired with its shot's reference frame, resampled into the frame's
//! geometry. Exposure, tone and blur are all measured on such a pair, so the
//! unknown picture cancels.

use crate::img::{Frames, Gray, Sim};

pub struct Pair<'a> {
    pub w: usize,
    pub h: usize,
    /// The frame being measured, RGB8.
    pub cur: &'a [u8],
    /// The reference frame warped onto it, RGB8.
    pub refw: Vec<u8>,
    /// Pixels to use: central region, not overlay, covered by the warped reference.
    pub mask: Vec<bool>,
    /// The reference frame was registered to this frame (pixels correspond).
    pub aligned: bool,
    /// Share of the central region covered by the warped reference.
    pub coverage: f64,
}

const BORDER: f64 = 0.12;

/// Build the pair. `sim` is the frame's track sample (reference picture ->
/// this frame); `None` leaves the reference frame unwarped.
pub fn make_pair<'a>(frames: &'a Frames, ref_frame: usize, cur_frame: usize, sim: Option<&Sim>, usable: &[bool]) -> Pair<'a> {
    let (w, h) = (frames.w, frames.h);
    let cur = frames.rgb(cur_frame);
    let src = frames.rgb(ref_frame);
    let (mx, my) = ((w as f64 * BORDER) as usize, (h as f64 * BORDER) as usize);
    let mut mask = vec![false; w * h];
    let mut refw = vec![0u8; w * h * 3];
    let mut central = 0usize;
    let mut covered = 0usize;
    match sim {
        None => {
            refw.copy_from_slice(src);
            for y in my..h - my {
                for x in mx..w - mx {
                    central += 1;
                    covered += 1;
                    mask[y * w + x] = usable[y * w + x];
                }
            }
        }
        Some(sim) => {
            let inv = sim.inverse();
            let (cx, cy, hh) = ((w as f64 - 1.0) / 2.0, (h as f64 - 1.0) / 2.0, h as f64);
            for y in 0..h {
                for x in 0..w {
                    let is_central = y >= my && y < h - my && x >= mx && x < w - mx;
                    central += is_central as usize;
                    let (u, v) = inv.apply((x as f64 - cx) / hh, (y as f64 - cy) / hh);
                    let (sx, sy) = (u * hh + cx, v * hh + cy);
                    if !(sx >= 0.0 && sy >= 0.0 && sx <= (w - 1) as f64 && sy <= (h - 1) as f64) {
                        continue;
                    }
                    covered += is_central as usize;
                    let (x0, y0) = ((sx as usize).min(w - 2), (sy as usize).min(h - 2));
                    let (fx, fy) = ((sx - x0 as f64) as f32, (sy - y0 as f64) as f32);
                    let i = (y0 * w + x0) * 3;
                    for c in 0..3 {
                        let a = src[i + c] as f32 + (src[i + 3 + c] as f32 - src[i + c] as f32) * fx;
                        let b = src[i + w * 3 + c] as f32 + (src[i + w * 3 + 3 + c] as f32 - src[i + w * 3 + c] as f32) * fx;
                        refw[(y * w + x) * 3 + c] = (a + (b - a) * fy).round() as u8;
                    }
                    // Overlay pixels of either frame must not enter the statistics.
                    let src_ok = usable[((sy + 0.5) as usize).min(h - 1) * w + ((sx + 0.5) as usize).min(w - 1)];
                    mask[y * w + x] = is_central && usable[y * w + x] && src_ok;
                }
            }
        }
    }
    Pair { w, h, cur, refw, mask, aligned: sim.is_some(), coverage: covered as f64 / central.max(1) as f64 }
}

/// Luma (0..1, gamma encoded) of an RGB8 buffer.
pub fn luma_image(rgb: &[u8], w: usize, h: usize) -> Gray {
    let mut d = Vec::with_capacity(w * h);
    for p in rgb.chunks_exact(3) {
        d.push((0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0);
    }
    Gray { w, h, d }
}
