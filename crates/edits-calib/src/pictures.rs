//! Procedural test pictures of the kinds this genre is cut from. Everything
//! is drawn by code from a seed: the repository ships no media.
//!
//! - `poster`: flat saturated areas with hard edges (cel art, posters);
//! - `line_art`: black ink on white paper, a few colour accents;
//! - `dark`: a near-black picture with fine dim detail;
//! - `photo`: smooth multi-scale texture with soft colour, like a photograph.

// Angle ranges are written 3.14 and 6.28: they are arbitrary ranges for
// random strokes, and changing them would change every generated picture
// (and so the calibration baseline).
#![allow(clippy::approx_constant)]

use crate::rng::Rng;
use image::{Rgb, RgbImage};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Poster,
    LineArt,
    Dark,
    Photo,
}

pub const KINDS: [Kind; 4] = [Kind::Poster, Kind::LineArt, Kind::Dark, Kind::Photo];

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Poster => "poster",
            Kind::LineArt => "line_art",
            Kind::Dark => "dark",
            Kind::Photo => "photo",
        }
    }
}

/// A picture in display (sRGB) values 0..1.
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[f32; 3]>,
}

impl Canvas {
    fn new(w: usize, h: usize, c: [f32; 3]) -> Self {
        Canvas { w, h, px: vec![c; w * h] }
    }
    /// Fill where the signed distance `d(x, y)` (pixels, negative inside) says so, with a one-pixel soft edge.
    fn shape(&mut self, bounds: (f32, f32, f32, f32), c: [f32; 3], alpha: f32, d: impl Fn(f32, f32) -> f32) {
        let x0 = (bounds.0 - 1.0).floor().max(0.0) as usize;
        let y0 = (bounds.1 - 1.0).floor().max(0.0) as usize;
        let x1 = ((bounds.2 + 2.0).ceil().max(0.0) as usize).min(self.w);
        let y1 = ((bounds.3 + 2.0).ceil().max(0.0) as usize).min(self.h);
        for y in y0..y1 {
            for x in x0..x1 {
                let a = (0.5 - d(x as f32 + 0.5, y as f32 + 0.5)).clamp(0.0, 1.0) * alpha;
                if a > 0.0 {
                    let p = &mut self.px[y * self.w + x];
                    for k in 0..3 {
                        p[k] += (c[k] - p[k]) * a;
                    }
                }
            }
        }
    }
    fn disc(&mut self, cx: f32, cy: f32, r: f32, c: [f32; 3], alpha: f32) {
        self.shape((cx - r, cy - r, cx + r, cy + r), c, alpha, |x, y| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r);
    }
    fn ring(&mut self, cx: f32, cy: f32, r: f32, t: f32, c: [f32; 3], alpha: f32) {
        self.shape((cx - r - t, cy - r - t, cx + r + t, cy + r + t), c, alpha, |x, y| (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r).abs() - t * 0.5);
    }
    /// A rectangle with half sizes (hx, hy), turned by `ang` radians.
    fn bar(&mut self, cx: f32, cy: f32, hx: f32, hy: f32, ang: f32, c: [f32; 3], alpha: f32) {
        let (s, co) = ang.sin_cos();
        let r = hx.hypot(hy);
        self.shape((cx - r, cy - r, cx + r, cy + r), c, alpha, |x, y| {
            let (dx, dy) = (x - cx, y - cy);
            let (u, v) = (dx * co + dy * s, -dx * s + dy * co);
            (u.abs() - hx).max(v.abs() - hy)
        });
    }
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: [f32; 3], alpha: f32) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        self.shape((x0.min(x1) - t, y0.min(y1) - t, x0.max(x1) + t, y0.max(y1) + t), c, alpha, |x, y| {
            let u = (((x - x0) * dx + (y - y0) * dy) / len2).clamp(0.0, 1.0);
            ((x - x0 - u * dx).powi(2) + (y - y0 - u * dy).powi(2)).sqrt() - t * 0.5
        });
    }
    fn into_image(self) -> RgbImage {
        let mut img = RgbImage::new(self.w as u32, self.h as u32);
        for (i, p) in self.px.iter().enumerate() {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            img.put_pixel((i % self.w) as u32, (i / self.w) as u32, Rgb([q(p[0]), q(p[1]), q(p[2])]));
        }
        img
    }
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn poster(r: &mut Rng, n: usize) -> Canvas {
    let s = n as f32;
    let base = r.unit() as f32;
    // A small palette: three saturated colours, a dark and a light.
    let mut palette: Vec<[f32; 3]> = (0..3).map(|k| hsv(base + 0.21 * k as f32 + r.range(-0.04, 0.04) as f32, r.range(0.6, 0.95) as f32, r.range(0.55, 0.95) as f32)).collect();
    palette.push([0.07, 0.06, 0.09]);
    palette.push([0.95, 0.92, 0.85]);
    let mut c = Canvas::new(n, n, palette[0]);
    for i in 0..46 {
        let col = palette[r.int(0, 4) as usize];
        let big = i < 10;
        let size = if big { r.range(0.12, 0.34) } else { r.range(0.015, 0.08) } as f32 * s;
        let (cx, cy) = (r.range(0.0, 1.0) as f32 * s, r.range(0.0, 1.0) as f32 * s);
        if r.chance(0.4) {
            c.disc(cx, cy, size, col, 1.0);
        } else {
            c.bar(cx, cy, size, size * r.range(0.2, 1.0) as f32, r.range(0.0, 3.14) as f32, col, 1.0);
        }
    }
    // Ink outlines and a few stripes, as in printed art.
    for _ in 0..7 {
        let (x0, y0) = (r.range(0.0, 1.0) as f32 * s, r.range(0.0, 1.0) as f32 * s);
        let (x1, y1) = (r.range(0.0, 1.0) as f32 * s, r.range(0.0, 1.0) as f32 * s);
        c.line(x0, y0, x1, y1, r.range(0.004, 0.012) as f32 * s, palette[3], 1.0);
    }
    c
}

fn line_art(r: &mut Rng, n: usize) -> Canvas {
    let s = n as f32;
    let paper = [0.97, 0.96, 0.94];
    let ink = [0.06, 0.06, 0.07];
    let mut c = Canvas::new(n, n, paper);
    // A few solid ink masses.
    for _ in 0..4 {
        let (cx, cy) = (r.range(0.1, 0.9) as f32 * s, r.range(0.1, 0.9) as f32 * s);
        let size = r.range(0.05, 0.16) as f32 * s;
        if r.chance(0.5) {
            c.disc(cx, cy, size, ink, 1.0);
        } else {
            c.bar(cx, cy, size, size * r.range(0.3, 1.0) as f32, r.range(0.0, 3.14) as f32, ink, 1.0);
        }
    }
    // Contours.
    for _ in 0..14 {
        let (cx, cy) = (r.range(0.0, 1.0) as f32 * s, r.range(0.0, 1.0) as f32 * s);
        c.ring(cx, cy, r.range(0.04, 0.3) as f32 * s, r.range(0.003, 0.009) as f32 * s, ink, 1.0);
    }
    for _ in 0..60 {
        let (x0, y0) = (r.range(0.0, 1.0) as f32 * s, r.range(0.0, 1.0) as f32 * s);
        let ang = r.range(0.0, 6.28) as f32;
        let len = r.range(0.05, 0.5) as f32 * s;
        c.line(x0, y0, x0 + len * ang.cos(), y0 + len * ang.sin(), r.range(0.002, 0.008) as f32 * s, ink, 1.0);
    }
    // Hatching.
    for _ in 0..5 {
        let (cx, cy) = (r.range(0.1, 0.9) as f32 * s, r.range(0.1, 0.9) as f32 * s);
        let ang = r.range(0.0, 3.14) as f32;
        let (dx, dy) = (ang.cos(), ang.sin());
        let gap = r.range(0.008, 0.016) as f32 * s;
        let len = r.range(0.05, 0.14) as f32 * s;
        for k in -6..=6 {
            let (ox, oy) = (-dy * gap * k as f32, dx * gap * k as f32);
            c.line(cx + ox - dx * len, cy + oy - dy * len, cx + ox + dx * len, cy + oy + dy * len, 0.0025 * s, ink, 0.9);
        }
    }
    // Colour accents.
    let accent = hsv(r.unit() as f32, 0.85, 0.85);
    for _ in 0..3 {
        c.disc(r.range(0.1, 0.9) as f32 * s, r.range(0.1, 0.9) as f32 * s, r.range(0.03, 0.09) as f32 * s, accent, 0.9);
    }
    c
}

fn dark(r: &mut Rng, n: usize) -> Canvas {
    let s = n as f32;
    let mut c = Canvas::new(n, n, [0.035, 0.035, 0.05]);
    let base = r.unit() as f32;
    // Large dim masses.
    for _ in 0..9 {
        let col = hsv(base + r.range(-0.12, 0.12) as f32, r.range(0.4, 0.9) as f32, r.range(0.08, 0.28) as f32);
        let size = r.range(0.1, 0.3) as f32 * s;
        c.bar(r.range(0.0, 1.0) as f32 * s, r.range(0.0, 1.0) as f32 * s, size, size * r.range(0.3, 1.0) as f32, r.range(0.0, 3.14) as f32, col, 1.0);
    }
    // Fine detail: thin strokes and points, some of them bright.
    for i in 0..150 {
        let bright = i % 9 == 0;
        let col = hsv(base + r.range(-0.2, 0.2) as f32, r.range(0.3, 0.9) as f32, if bright { r.range(0.6, 0.95) } else { r.range(0.15, 0.45) } as f32);
        let (x0, y0) = (r.range(0.0, 1.0) as f32 * s, r.range(0.0, 1.0) as f32 * s);
        if r.chance(0.35) {
            c.disc(x0, y0, r.range(0.004, 0.02) as f32 * s, col, 1.0);
        } else {
            let ang = r.range(0.0, 6.28) as f32;
            let len = r.range(0.03, 0.22) as f32 * s;
            c.line(x0, y0, x0 + len * ang.cos(), y0 + len * ang.sin(), r.range(0.002, 0.007) as f32 * s, col, 1.0);
        }
    }
    c
}

/// Smooth value noise on a lattice of `cells` x `cells`.
fn value_noise(r: &mut Rng, n: usize, cells: usize) -> Vec<f32> {
    let g = cells + 2;
    let lat: Vec<f32> = (0..g * g).map(|_| r.unit() as f32).collect();
    let mut out = vec![0.0f32; n * n];
    for y in 0..n {
        let fy = y as f32 / n as f32 * cells as f32;
        let (iy, ty) = (fy as usize, fy.fract());
        let sy = ty * ty * (3.0 - 2.0 * ty);
        for x in 0..n {
            let fx = x as f32 / n as f32 * cells as f32;
            let (ix, tx) = (fx as usize, fx.fract());
            let sx = tx * tx * (3.0 - 2.0 * tx);
            let a = lat[iy * g + ix] + (lat[iy * g + ix + 1] - lat[iy * g + ix]) * sx;
            let b = lat[(iy + 1) * g + ix] + (lat[(iy + 1) * g + ix + 1] - lat[(iy + 1) * g + ix]) * sx;
            out[y * n + x] = a + (b - a) * sy;
        }
    }
    out
}

fn photo(r: &mut Rng, n: usize) -> Canvas {
    // Luminance: octaves of value noise with a 1/f-like spectrum.
    let mut lum = vec![0.0f32; n * n];
    let mut amp = 1.0;
    let mut total = 0.0;
    for cells in [3usize, 6, 12, 24, 48, 96] {
        let o = value_noise(r, n, cells);
        for (l, v) in lum.iter_mut().zip(&o) {
            *l += amp * (v - 0.5);
        }
        total += amp;
        amp *= 0.62;
    }
    let (cu, cv) = (value_noise(r, n, 4), value_noise(r, n, 7));
    let hue0 = r.unit() as f32;
    let mut c = Canvas::new(n, n, [0.5; 3]);
    for i in 0..n * n {
        let l = (0.48 + 1.9 * lum[i] / total).clamp(0.03, 0.97);
        let tint = hsv(hue0 + 0.35 * (cu[i] - 0.5), 0.25 + 0.5 * cv[i], 1.0);
        let m = (tint[0] * 0.2126 + tint[1] * 0.7152 + tint[2] * 0.0722).max(0.05);
        for k in 0..3 {
            c.px[i][k] = (l * tint[k] / m).clamp(0.0, 1.0);
        }
    }
    // A few soft-edged subjects.
    let s = n as f32;
    for _ in 0..5 {
        let col = hsv(r.unit() as f32, r.range(0.2, 0.6) as f32, r.range(0.2, 0.9) as f32);
        let (cx, cy, rad) = (r.range(0.1, 0.9) as f32 * s, r.range(0.1, 0.9) as f32 * s, r.range(0.05, 0.16) as f32 * s);
        c.shape((cx - rad, cy - rad, cx + rad, cy + rad), col, 0.75, |x, y| (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - rad) * 0.25);
    }
    // Sensor grain.
    for p in c.px.iter_mut() {
        let g = (r.unit() + r.unit() - 1.0) as f32 * 0.025;
        for v in p.iter_mut() {
            *v = (*v + g).clamp(0.0, 1.0);
        }
    }
    c
}

/// A square picture of `size` pixels.
pub fn generate(kind: Kind, seed: u64, size: u32) -> RgbImage {
    let mut r = Rng::new(seed ^ 0xA11C_E5ED);
    let n = size as usize;
    match kind {
        Kind::Poster => poster(&mut r, n),
        Kind::LineArt => line_art(&mut r, n),
        Kind::Dark => dark(&mut r, n),
        Kind::Photo => photo(&mut r, n),
    }
    .into_image()
}

/// A static overlay plate for a `screen` blend: thin white ink and dust on
/// black, mostly near the edges of the frame.
pub fn overlay(seed: u64, w: u32, h: u32) -> RgbImage {
    let mut r = Rng::new(seed ^ 0x0E71_A7ED);
    let (fw, fh) = (w as f32, h as f32);
    let mut c = Canvas::new(w as usize, h as usize, [0.0; 3]);
    let white = [1.0, 1.0, 1.0];
    let t = (0.004 * fh).max(1.2);
    // A frame line and corner marks.
    let m = 0.045 * fh;
    for (x0, y0, x1, y1) in [(m, m, fw - m, m), (m, fh - m, fw - m, fh - m), (m, m, m, fh - m), (fw - m, m, fw - m, fh - m)] {
        c.line(x0, y0, x1, y1, t, white, 0.9);
    }
    // Handwriting-like strokes along the top and bottom bands.
    for band in [0.09f32, 0.91] {
        let mut x = r.range(0.08, 0.2) as f32 * fw;
        while x < 0.9 * fw {
            let len = r.range(0.01, 0.04) as f32 * fw;
            let y = band * fh + r.range(-0.012, 0.012) as f32 * fh;
            c.line(x, y, x + len, y + r.range(-0.015, 0.015) as f32 * fh, t, white, 0.85);
            x += len + r.range(0.008, 0.03) as f32 * fw;
        }
    }
    // Dust and scratches.
    for _ in 0..40 {
        let edge = r.chance(0.7);
        let (x, y) = if edge {
            let side = r.int(0, 3);
            let a = r.range(0.0, 1.0) as f32;
            let b = r.range(0.0, 0.12) as f32;
            match side {
                0 => (a * fw, b * fh),
                1 => (a * fw, (1.0 - b) * fh),
                2 => (b * fw, a * fh),
                _ => ((1.0 - b) * fw, a * fh),
            }
        } else {
            (r.range(0.0, 1.0) as f32 * fw, r.range(0.0, 1.0) as f32 * fh)
        };
        c.disc(x, y, r.range(0.002, 0.005) as f32 * fh + 0.6, white, r.range(0.5, 0.9) as f32);
    }
    c.into_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(img: &RgbImage) -> (f64, f64) {
        let n = (img.width() * img.height()) as f64;
        let (mut lum, mut sat) = (0.0, 0.0);
        for p in img.pixels() {
            let (mx, mn) = (p[0].max(p[1]).max(p[2]) as f64, p[0].min(p[1]).min(p[2]) as f64);
            lum += 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
            sat += if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        }
        (lum / n, sat / n)
    }

    #[test]
    fn kinds_differ_and_are_deterministic() {
        let a = generate(Kind::LineArt, 3, 128);
        assert_eq!(a.as_raw(), generate(Kind::LineArt, 3, 128).as_raw());
        assert_ne!(a.as_raw(), generate(Kind::LineArt, 4, 128).as_raw());
        let (line_lum, _) = stats(&a);
        let (dark_lum, _) = stats(&generate(Kind::Dark, 3, 128));
        let (_, poster_sat) = stats(&generate(Kind::Poster, 3, 128));
        assert!(line_lum > 170.0, "line art is mostly paper: {line_lum}");
        assert!(dark_lum < 60.0, "dark art is dark: {dark_lum}");
        assert!(poster_sat > 0.35, "poster art is saturated: {poster_sat}");
    }
}
