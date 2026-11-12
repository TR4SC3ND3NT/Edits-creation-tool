//! Synthetic clips with a known recipe, generated in code. Used by the tests
//! (and usable for calibration): no reference footage is ever needed to check
//! an estimator.

use crate::img::{gaussian_blur, linear_to_srgb, Frames, Gray, Sim};

#[derive(Clone, Debug)]
pub struct FrameSpec {
    /// Which procedural picture is shown (0 = black frame).
    pub picture: u32,
    pub scale: f64,
    pub rot_deg: f64,
    /// Translation in units of frame height.
    pub tx: f64,
    pub ty: f64,
    /// Exposure in stops, applied in linear light and clipped at white.
    pub stops: f64,
    /// Gaussian blur sigma in % of frame height.
    pub blur: f64,
    /// 1 = unchanged, 0 = black and white.
    pub saturation: f64,
    pub invert: bool,
    /// Two-tone threshold at this (gamma-encoded) luminance level.
    pub threshold: Option<f64>,
}

impl Default for FrameSpec {
    fn default() -> Self {
        Self { picture: 1, scale: 1.0, rot_deg: 0.0, tx: 0.0, ty: 0.0, stops: 0.0, blur: 0.0, saturation: 1.0, invert: false, threshold: None }
    }
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub w: usize,
    pub h: usize,
    pub fps: f64,
    pub frames: Vec<FrameSpec>,
    /// Draw a static line overlay at the left and right edges (screen blend).
    pub overlay: bool,
    /// Vignette amount (0 = none); radius 0.5, softness 0.5 of the half-diagonal.
    pub vignette: f64,
    /// Standard deviation of per-pixel noise in 8-bit code values.
    pub noise: f64,
}

struct Shape {
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    cos: f64,
    sin: f64,
    disc: bool,
    color: [f32; 3],
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f64) / (1u64 << 31) as f64
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
}

fn shapes(seed: u32) -> Vec<Shape> {
    let mut r = Rng(seed as u64 * 7919 + 17);
    (0..70)
        .map(|i| {
            let big = i < 12;
            let size = if big { r.range(0.15, 0.5) } else { r.range(0.02, 0.14) };
            let ang = r.range(0.0, std::f64::consts::PI);
            let dark = r.next() < 0.3;
            let color = if dark {
                let v = r.range(0.0, 0.05) as f32;
                [v, v, v]
            } else {
                [r.range(0.03, 0.9) as f32, r.range(0.03, 0.9) as f32, r.range(0.03, 0.9) as f32]
            };
            Shape {
                cx: r.range(-1.0, 1.0),
                cy: r.range(-1.0, 1.0),
                rx: size,
                ry: size * r.range(0.25, 1.0),
                cos: ang.cos(),
                sin: ang.sin(),
                disc: r.next() < 0.4,
                color,
            }
        })
        .collect()
}

/// Linear-light colour of picture `shapes` at (u, v) in height units.
fn picture(sh: &[Shape], seed: u32, u: f64, v: f64) -> [f32; 3] {
    let s = seed as f64;
    let mut c = [
        (0.35 + 0.25 * (1.7 * u + s).sin()) as f32,
        (0.3 + 0.2 * (2.3 * v - s).cos()) as f32,
        (0.4 + 0.25 * (1.1 * (u + v) + 2.0 * s).sin()) as f32,
    ];
    for sp in sh {
        let (dx, dy) = (u - sp.cx, v - sp.cy);
        let (x, y) = (dx * sp.cos + dy * sp.sin, -dx * sp.sin + dy * sp.cos);
        let inside = if sp.disc { (x / sp.rx).powi(2) + (y / sp.ry).powi(2) < 1.0 } else { x.abs() < sp.rx && y.abs() < sp.ry };
        if inside {
            c = sp.color;
        }
    }
    let t = (0.04 * (53.0 * u + s).sin() * (47.0 * v - s).sin()) as f32;
    [(c[0] + t).clamp(0.0, 1.0), (c[1] + t).clamp(0.0, 1.0), (c[2] + t).clamp(0.0, 1.0)]
}

pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0).max(1e-9)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The vignette model the analysis fits: brightness multiplier at radius `r`
/// (fraction of the half-diagonal).
pub fn vignette_gain(amount: f64, radius: f64, softness: f64, r: f64) -> f64 {
    1.0 - amount * smoothstep(radius, radius + softness, r)
}

impl Clip {
    pub fn new(w: usize, h: usize, n: usize) -> Self {
        Self { w, h, fps: 25.0, frames: vec![FrameSpec::default(); n], overlay: false, vignette: 0.0, noise: 0.0 }
    }
    pub fn frame_mut(&mut self, i: usize) -> &mut FrameSpec {
        &mut self.frames[i]
    }
    /// The overlay plate value (0..1) at a pixel.
    pub fn overlay_at(&self, x: usize, y: usize) -> f32 {
        if !self.overlay {
            return 0.0;
        }
        let edge = (self.w as f64 * 0.1) as usize;
        let in_band = x >= 2 && (x < edge || x >= self.w - edge) && x < self.w - 2;
        if in_band && (y % 7 == 3 || (x % 9 == 4 && y % 14 < 7)) {
            0.85
        } else {
            0.0
        }
    }

    pub fn render(&self) -> Frames {
        let (w, h) = (self.w, self.h);
        let mut out = Frames::new(w, h, self.fps);
        let (cx, cy, hh) = ((w as f64 - 1.0) / 2.0, (h as f64 - 1.0) / 2.0, h as f64);
        let half_diag = (cx * cx + cy * cy).sqrt();
        let mut cache: std::collections::BTreeMap<u32, Vec<Shape>> = Default::default();
        let mut rng = Rng(99);
        for spec in &self.frames {
            let mut ch = [Gray::new(w, h), Gray::new(w, h), Gray::new(w, h)];
            if spec.picture != 0 {
                let sh = cache.entry(spec.picture).or_insert_with(|| shapes(spec.picture));
                let inv = Sim::from_parts(spec.scale, spec.rot_deg, spec.tx, spec.ty).inverse();
                for y in 0..h {
                    for x in 0..w {
                        let mut acc = [0.0f32; 3];
                        for (ox, oy) in [(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)] {
                            let (u, v) = inv.apply((x as f64 + ox - cx) / hh, (y as f64 + oy - cy) / hh);
                            let c = picture(sh, spec.picture, u, v);
                            for k in 0..3 {
                                acc[k] += 0.25 * c[k];
                            }
                        }
                        for k in 0..3 {
                            ch[k].d[y * w + x] = acc[k];
                        }
                    }
                }
                if spec.blur > 0.0 {
                    let sigma = (spec.blur * hh / 100.0) as f32;
                    for c in ch.iter_mut() {
                        *c = gaussian_blur(c, sigma);
                    }
                }
            }
            let gain = 2f64.powf(spec.stops) as f32;
            let mut rgb = vec![0u8; w * h * 3];
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    let mut c = [ch[0].d[i], ch[1].d[i], ch[2].d[i]];
                    let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                    for v in c.iter_mut() {
                        *v = l + (*v - l) * spec.saturation as f32;
                    }
                    if spec.invert {
                        for v in c.iter_mut() {
                            *v = 1.0 - *v;
                        }
                    }
                    if let Some(level) = spec.threshold {
                        let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                        let t = if linear_to_srgb(l) as f64 > level { 1.0 } else { 0.0 };
                        c = [t, t, t];
                    }
                    let r = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt() / half_diag;
                    let vg = vignette_gain(self.vignette, 0.5, 0.5, r) as f32;
                    // A black frame (no picture) is a true black frame: nothing on top.
                    let o = if spec.picture == 0 { 0.0 } else { self.overlay_at(x, y) };
                    for k in 0..3 {
                        let v = (c[k] * gain).clamp(0.0, 1.0) * vg;
                        let v = 1.0 - (1.0 - v) * (1.0 - o);
                        let mut code = linear_to_srgb(v) as f64 * 255.0;
                        if self.noise > 0.0 {
                            // Sum of uniforms: close enough to Gaussian.
                            code += self.noise * (rng.next() + rng.next() + rng.next() - 1.5) * 2.0;
                        }
                        rgb[i * 3 + k] = code.round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
            out.push(&rgb);
        }
        out
    }
}
