//! A frame paired with its shot's reference frame, resampled into the frame's
//! geometry. Exposure, tone and blur are all measured on such a pair, so the
//! unknown picture cancels.

use crate::img::{Frames, Gray, Sim};

/// The vignette as a per-pixel map, so that "clipped at white" can be told
/// where white is no longer 255 (a flash under a vignette is grey in the corners).
pub struct FlatField {
    /// Linear-light brightness of a white field at each pixel, relative to the centre (0..1).
    pub gain: Vec<f32>,
    /// The 8-bit code value a white field has at each pixel.
    pub white: Vec<u8>,
}

impl FlatField {
    /// No vignette.
    pub fn flat(w: usize, h: usize) -> Self {
        Self { gain: vec![1.0; w * h], white: vec![255; w * h] }
    }
    /// From a radial profile (`profile[i]` at radius `i / (n - 1)` of the half-diagonal).
    pub fn from_profile(w: usize, h: usize, profile: &[f64]) -> Self {
        if profile.len() < 2 {
            return Self::flat(w, h);
        }
        let (cx, cy) = ((w as f64 - 1.0) / 2.0, (h as f64 - 1.0) / 2.0);
        let half_diag = (cx * cx + cy * cy).sqrt();
        let mut gain = Vec::with_capacity(w * h);
        let mut white = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let r = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt() / half_diag * (profile.len() - 1) as f64;
                let i = (r as usize).min(profile.len() - 2);
                let g = (profile[i] + (profile[i + 1] - profile[i]) * (r - i as f64)).clamp(0.05, 1.0) as f32;
                gain.push(g);
                white.push((crate::img::linear_to_srgb(g) * 255.0).round() as u8);
            }
        }
        Self { gain, white }
    }
}

pub struct Pair<'a> {
    pub w: usize,
    pub h: usize,
    /// The frame being measured, RGB8.
    pub cur: &'a [u8],
    /// The reference frame warped onto it, RGB8.
    pub refw: Vec<u8>,
    /// Pixels to use: central region, not overlay, covered by the warped reference.
    pub mask: Vec<bool>,
    /// Pixels of the warped reference frame that hold picture: covered, and not an overlay stroke of the reference frame.
    pub ref_ok: Vec<bool>,
    /// The reference frame was registered to this frame (pixels correspond).
    pub aligned: bool,
    /// Share of the central region covered by the warped reference.
    pub coverage: f64,
    /// The vignette, if one was measured.
    pub flat: Option<&'a FlatField>,
}

impl<'a> Pair<'a> {
    pub fn with_flat(mut self, flat: &'a FlatField) -> Self {
        self.flat = Some(flat);
        self
    }
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
    let mut ref_ok = vec![false; w * h];
    let mut refw = vec![0u8; w * h * 3];
    let mut central = 0usize;
    let mut covered = 0usize;
    match sim {
        None => {
            refw.copy_from_slice(src);
            ref_ok.copy_from_slice(usable);
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
                    ref_ok[y * w + x] = src_ok;
                    mask[y * w + x] = is_central && usable[y * w + x] && src_ok;
                }
            }
        }
    }
    Pair { w, h, cur, refw, mask, ref_ok, aligned: sim.is_some(), coverage: covered as f64 / central.max(1) as f64, flat: None }
}

/// Luma (0..1, gamma encoded) of an RGB8 buffer.
pub fn luma_image(rgb: &[u8], w: usize, h: usize) -> Gray {
    let mut d = Vec::with_capacity(w * h);
    for p in rgb.chunks_exact(3) {
        d.push((0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0);
    }
    Gray { w, h, d }
}

/// Luminance in linear light (0..1) of an RGB8 buffer. Blur is measured on
/// this: the renderer blurs in linear light, and only there is a Gaussian
/// blur a multiplication of the spectrum by a Gaussian.
pub fn linear_luma_image(rgb: &[u8], w: usize, h: usize) -> Gray {
    let lut = crate::img::srgb_lut();
    let mut d = Vec::with_capacity(w * h);
    for p in rgb.chunks_exact(3) {
        d.push(0.2126 * lut[p[0] as usize] + 0.7152 * lut[p[1] as usize] + 0.0722 * lut[p[2] as usize]);
    }
    Gray { w, h, d }
}
