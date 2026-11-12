//! Small image toolbox: grey images, blur, resampling, similarity transforms,
//! radially averaged power spectra. No OpenCV; everything here is plain Rust.

use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};

/// Decoded video frames at analysis resolution, packed RGB8.
#[derive(Clone)]
pub struct Frames {
    pub w: usize,
    pub h: usize,
    pub fps: f64,
    pub data: Vec<u8>,
}

impl Frames {
    pub fn new(w: usize, h: usize, fps: f64) -> Self {
        Self { w, h, fps, data: Vec::new() }
    }
    pub fn len(&self) -> usize {
        if self.w * self.h == 0 {
            0
        } else {
            self.data.len() / (self.w * self.h * 3)
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn rgb(&self, i: usize) -> &[u8] {
        let n = self.w * self.h * 3;
        &self.data[i * n..(i + 1) * n]
    }
    pub fn push(&mut self, rgb: &[u8]) {
        assert_eq!(rgb.len(), self.w * self.h * 3);
        self.data.extend_from_slice(rgb);
    }
    /// Gamma-encoded Rec.709 luma in 0..1.
    pub fn gray(&self, i: usize) -> Gray {
        let rgb = self.rgb(i);
        let mut d = Vec::with_capacity(self.w * self.h);
        for p in rgb.chunks_exact(3) {
            d.push((0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0);
        }
        Gray { w: self.w, h: self.h, d }
    }
}

/// A single-channel float image.
#[derive(Clone, Debug)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub d: Vec<f32>,
}

impl Gray {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, d: vec![0.0; w * h] }
    }
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.d[y * self.w + x]
    }
    /// Bilinear sample; `None` outside the image.
    #[inline]
    pub fn sample(&self, x: f32, y: f32) -> Option<f32> {
        if !(x >= 0.0 && y >= 0.0 && x <= (self.w - 1) as f32 && y <= (self.h - 1) as f32) {
            return None;
        }
        let x0 = (x as usize).min(self.w - 2);
        let y0 = (y as usize).min(self.h - 2);
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;
        let i = y0 * self.w + x0;
        let a = self.d[i] + (self.d[i + 1] - self.d[i]) * fx;
        let b = self.d[i + self.w] + (self.d[i + self.w + 1] - self.d[i + self.w]) * fx;
        Some(a + (b - a) * fy)
    }
    pub fn mean(&self) -> f32 {
        self.d.iter().sum::<f32>() / self.d.len().max(1) as f32
    }
    pub fn std(&self) -> f32 {
        let m = self.mean();
        (self.d.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / self.d.len().max(1) as f32).sqrt()
    }
}

pub fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let r = (sigma * 3.0).ceil().max(1.0) as i32;
    let mut k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let s: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= s);
    k
}

/// Separable Gaussian blur with clamped edges. `sigma` in pixels.
pub fn gaussian_blur(src: &Gray, sigma: f32) -> Gray {
    if sigma <= 0.05 {
        return src.clone();
    }
    let k = gaussian_kernel(sigma);
    let r = (k.len() / 2) as i32;
    let (w, h) = (src.w, src.h);
    let mut tmp = Gray::new(w, h);
    for y in 0..h {
        let row = &src.d[y * w..(y + 1) * w];
        for x in 0..w {
            let mut acc = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let xx = (x as i32 + j as i32 - r).clamp(0, w as i32 - 1) as usize;
                acc += kv * row[xx];
            }
            tmp.d[y * w + x] = acc;
        }
    }
    let mut out = Gray::new(w, h);
    for y in 0..h {
        for (j, kv) in k.iter().enumerate() {
            let yy = (y as i32 + j as i32 - r).clamp(0, h as i32 - 1) as usize;
            let (dst, srow) = (&mut out.d[y * w..(y + 1) * w], &tmp.d[yy * w..(yy + 1) * w]);
            for x in 0..w {
                dst[x] += kv * srow[x];
            }
        }
    }
    out
}

/// Halve the size with a 2x2 box filter.
pub fn half(src: &Gray) -> Gray {
    let (w, h) = (src.w / 2, src.h / 2);
    let mut out = Gray::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = 2 * y * src.w + 2 * x;
            out.d[y * w + x] = 0.25 * (src.d[i] + src.d[i + 1] + src.d[i + src.w] + src.d[i + src.w + 1]);
        }
    }
    out
}

/// Area-average resize to an arbitrary smaller size.
pub fn resize_area(src: &Gray, w: usize, h: usize) -> Gray {
    let mut out = Gray::new(w, h);
    let sx = src.w as f32 / w as f32;
    let sy = src.h as f32 / h as f32;
    for y in 0..h {
        let y0 = (y as f32 * sy) as usize;
        let y1 = (((y + 1) as f32 * sy).ceil() as usize).clamp(y0 + 1, src.h);
        for x in 0..w {
            let x0 = (x as f32 * sx) as usize;
            let x1 = (((x + 1) as f32 * sx).ceil() as usize).clamp(x0 + 1, src.w);
            let mut acc = 0.0;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    acc += src.d[yy * src.w + xx];
                }
            }
            out.d[y * w + x] = acc / ((y1 - y0) * (x1 - x0)) as f32;
        }
    }
    out
}

/// sRGB transfer function, 8-bit code value to linear light.
pub fn srgb_lut() -> [f32; 256] {
    let mut t = [0.0f32; 256];
    for (i, v) in t.iter_mut().enumerate() {
        let c = i as f32 / 255.0;
        *v = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    }
    t
}

pub fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// A similarity transform of the picture, in units of frame height with the
/// origin at the frame centre (x right, y down): `p' = [[a, -b], [b, a]] p + t`.
/// `scale = hypot(a, b)`, `rotation = atan2(b, a)`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sim {
    pub a: f64,
    pub b: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Sim {
    pub const IDENTITY: Sim = Sim { a: 1.0, b: 0.0, tx: 0.0, ty: 0.0 };
    pub fn from_parts(scale: f64, rot_deg: f64, tx: f64, ty: f64) -> Self {
        let r = rot_deg.to_radians();
        Sim { a: scale * r.cos(), b: scale * r.sin(), tx, ty }
    }
    pub fn scale(&self) -> f64 {
        self.a.hypot(self.b)
    }
    pub fn rotation_deg(&self) -> f64 {
        self.b.atan2(self.a).to_degrees()
    }
    #[inline]
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x - self.b * y + self.tx, self.b * x + self.a * y + self.ty)
    }
    /// `self ∘ o`: apply `o` first, then `self`.
    pub fn compose(&self, o: &Sim) -> Sim {
        let (tx, ty) = self.apply(o.tx, o.ty);
        Sim { a: self.a * o.a - self.b * o.b, b: self.b * o.a + self.a * o.b, tx, ty }
    }
    pub fn inverse(&self) -> Sim {
        let d = self.a * self.a + self.b * self.b;
        let (a, b) = (self.a / d, -self.b / d);
        Sim { a, b, tx: -(a * self.tx - b * self.ty), ty: -(b * self.tx + a * self.ty) }
    }
    pub fn is_finite(&self) -> bool {
        self.a.is_finite() && self.b.is_finite() && self.tx.is_finite() && self.ty.is_finite()
    }
}

/// Resample `src` so that the output shows the picture moved by `sim`:
/// `out(p) = src(sim^-1 p)`. Returns the image and a coverage mask.
pub fn warp_gray(src: &Gray, sim: &Sim) -> (Gray, Vec<bool>) {
    let inv = sim.inverse();
    let (w, h) = (src.w, src.h);
    let (cx, cy, hh) = ((w as f64 - 1.0) / 2.0, (h as f64 - 1.0) / 2.0, h as f64);
    let mut out = Gray::new(w, h);
    let mut mask = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let (u, v) = inv.apply((x as f64 - cx) / hh, (y as f64 - cy) / hh);
            if let Some(val) = src.sample((u * hh + cx) as f32, (v * hh + cy) as f32) {
                out.d[y * w + x] = val;
                mask[y * w + x] = true;
            }
        }
    }
    (out, mask)
}

/// Radially averaged power spectrum of the central `n`x`n` window (Hann
/// weighted, mean removed). Bin `k` covers spatial frequency `k / n` cycles
/// per pixel; the result has `n / 2` bins. `sectors` > 1 additionally splits
/// the spectrum by orientation (returned after the isotropic profile).
pub fn radial_power(img: &Gray, n: usize, sectors: usize) -> Vec<Vec<f64>> {
    let n = n.min(img.w).min(img.h);
    let x0 = (img.w - n) / 2;
    let y0 = (img.h - n) / 2;
    let hann: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * (i as f32 + 0.5) / n as f32).cos()).collect();
    let mut mean = 0.0f64;
    let mut wsum = 0.0f64;
    for y in 0..n {
        for x in 0..n {
            let wv = (hann[x] * hann[y]) as f64;
            mean += wv * img.d[(y0 + y) * img.w + x0 + x] as f64;
            wsum += wv;
        }
    }
    let mean = (mean / wsum) as f32;
    let mut buf: Vec<Complex<f32>> = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            buf.push(Complex::new((img.d[(y0 + y) * img.w + x0 + x] - mean) * hann[x] * hann[y], 0.0));
        }
    }
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    for row in buf.chunks_exact_mut(n) {
        fft.process(row);
    }
    let mut col = vec![Complex::new(0.0f32, 0.0); n];
    for x in 0..n {
        for y in 0..n {
            col[y] = buf[y * n + x];
        }
        fft.process(&mut col);
        for y in 0..n {
            buf[y * n + x] = col[y];
        }
    }
    let bins = n / 2;
    let groups = 1 + if sectors > 1 { sectors } else { 0 };
    let mut sum = vec![vec![0.0f64; bins]; groups];
    let mut cnt = vec![vec![0u32; bins]; groups];
    for y in 0..n {
        let fy = if y <= n / 2 { y as f32 } else { y as f32 - n as f32 };
        for x in 0..=n / 2 {
            let fx = x as f32;
            let r = (fx * fx + fy * fy).sqrt();
            let k = r.round() as usize;
            if k == 0 || k >= bins {
                continue;
            }
            let p = buf[y * n + x].norm_sqr() as f64;
            sum[0][k] += p;
            cnt[0][k] += 1;
            if sectors > 1 {
                // Orientation of the frequency vector, folded to 0..pi.
                let ang = fy.atan2(fx).rem_euclid(std::f32::consts::PI);
                let s = ((ang / std::f32::consts::PI * sectors as f32 + 0.5) as usize) % sectors;
                sum[1 + s][k] += p;
                cnt[1 + s][k] += 1;
            }
        }
    }
    for g in 0..groups {
        for k in 0..bins {
            if cnt[g][k] > 0 {
                sum[g][k] /= cnt[g][k] as f64;
            }
        }
    }
    sum
}

// ---- small statistics helpers ----------------------------------------------

pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return 0.0;
    }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        0.5 * (s[n / 2 - 1] + s[n / 2])
    }
}

pub fn percentile(v: &[f64], p: f64) -> f64 {
    let mut s: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return 0.0;
    }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pos = (p.clamp(0.0, 1.0) * (s.len() - 1) as f64).round() as usize;
    s[pos]
}

pub fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Write an RGB8 buffer as PNG.
pub fn save_rgb(path: &std::path::Path, w: usize, h: usize, rgb: &[u8]) -> anyhow::Result<()> {
    image::save_buffer(path, rgb, w as u32, h as u32, image::ExtendedColorType::Rgb8)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_compose_and_inverse_round_trip() {
        let s = Sim::from_parts(1.3, 12.0, 0.05, -0.02);
        let i = s.compose(&s.inverse());
        assert!((i.a - 1.0).abs() < 1e-12 && i.b.abs() < 1e-12 && i.tx.abs() < 1e-12 && i.ty.abs() < 1e-12);
        assert!((s.scale() - 1.3).abs() < 1e-12);
        assert!((s.rotation_deg() - 12.0).abs() < 1e-9);
    }

    #[test]
    fn blur_preserves_mean_and_reduces_contrast() {
        let mut g = Gray::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                g.d[y * 64 + x] = if (x / 4 + y / 4) % 2 == 0 { 1.0 } else { 0.0 };
            }
        }
        let b = gaussian_blur(&g, 2.0);
        assert!((b.mean() - g.mean()).abs() < 0.01);
        assert!(b.std() < 0.5 * g.std());
    }

    #[test]
    fn radial_power_of_a_sine_peaks_at_its_frequency() {
        let n = 64;
        let mut g = Gray::new(n, n);
        for y in 0..n {
            for x in 0..n {
                g.d[y * n + x] = (std::f32::consts::TAU * 8.0 * x as f32 / n as f32).sin();
            }
        }
        let p = &radial_power(&g, n, 1)[0];
        let k = p.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).unwrap().0;
        assert_eq!(k, 8);
    }
}
