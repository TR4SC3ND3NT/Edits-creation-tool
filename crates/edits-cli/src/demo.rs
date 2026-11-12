//! `edits demo`: a self-contained project. The panel pictures, the overlay
//! texture and the click track are all generated here, so the repository
//! ships no media and the demo renders anywhere.

use anyhow::{Context, Result};
use edits_render::font;
use image::{Rgba, RgbaImage};
use std::path::Path;

/// The example recipe the demo is built from.
const RECIPE: &str = include_str!("../../../examples/beat-loop.json");
const SLOTS: [&str; 5] = ["a", "b", "c", "d", "tex"];
const AUDIO: &str = "audio/beat.wav";

type Rgb = [u8; 3];
const INK: Rgb = [22, 20, 26];
const PAPER: Rgb = [244, 234, 212];

/// Small deterministic generator (xorshift64*), so every run draws the same pictures.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }
}

struct Canvas {
    img: RgbaImage,
}

impl Canvas {
    fn new(w: u32, h: u32, c: Rgb) -> Self {
        Self { img: RgbaImage::from_pixel(w, h, Rgba([c[0], c[1], c[2], 255])) }
    }
    fn size(&self) -> (f32, f32) {
        (self.img.width() as f32, self.img.height() as f32)
    }
    fn blend(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        if x < 0 || y < 0 || x >= self.img.width() as i32 || y >= self.img.height() as i32 || a <= 0.0 {
            return;
        }
        let p = self.img.get_pixel_mut(x as u32, y as u32);
        let a = a.min(1.0);
        for i in 0..3 {
            p[i] = (p[i] as f32 + (c[i] as f32 - p[i] as f32) * a).round() as u8;
        }
    }
    /// Fill where the signed distance `d` (pixels, negative inside) says so, with a soft one-pixel edge.
    fn shape(&mut self, bounds: (f32, f32, f32, f32), c: Rgb, alpha: f32, d: impl Fn(f32, f32) -> f32) {
        let (w, h) = self.size();
        let (x0, y0) = ((bounds.0 - 1.0).max(0.0) as i32, (bounds.1 - 1.0).max(0.0) as i32);
        let (x1, y1) = ((bounds.2 + 2.0).min(w) as i32, (bounds.3 + 2.0).min(h) as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                let cover = (0.5 - d(x as f32 + 0.5, y as f32 + 0.5)).clamp(0.0, 1.0);
                self.blend(x, y, c, cover * alpha);
            }
        }
    }
    fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgb, alpha: f32) {
        self.shape((cx - r, cy - r, cx + r, cy + r), c, alpha, |x, y| (x - cx).hypot(y - cy) - r);
    }
    fn ring(&mut self, cx: f32, cy: f32, r: f32, t: f32, c: Rgb) {
        let o = r + t;
        self.shape((cx - o, cy - o, cx + o, cy + o), c, 1.0, |x, y| ((x - cx).hypot(y - cy) - r).abs() - t * 0.5);
    }
    fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgb, alpha: f32) {
        self.shape((cx - rx, cy - ry, cx + rx, cy + ry), c, alpha, |x, y| (((x - cx) / rx).hypot((y - cy) / ry) - 1.0) * rx.min(ry));
    }
    /// A soft blob that fades to nothing at radius `r`.
    fn glow(&mut self, cx: f32, cy: f32, r: f32, c: Rgb, alpha: f32) {
        let (w, h) = self.size();
        for y in (cy - r).max(0.0) as i32..(cy + r).min(h) as i32 {
            for x in (cx - r).max(0.0) as i32..(cx + r).min(w) as i32 {
                let k = 1.0 - (x as f32 - cx).hypot(y as f32 - cy) / r;
                if k > 0.0 {
                    self.blend(x, y, c, k * k * alpha);
                }
            }
        }
    }
    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb, alpha: f32) {
        self.shape((x0, y0, x1, y1), c, alpha, |x, y| (x0 - x).max(x - x1).max(y0 - y).max(y - y1));
    }
    #[allow(clippy::too_many_arguments)]
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: Rgb, alpha: f32) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let b = (x0.min(x1) - t, y0.min(y1) - t, x0.max(x1) + t, y0.max(y1) + t);
        self.shape(b, c, alpha, |x, y| {
            let k = (((x - x0) * dx + (y - y0) * dy) / len2).clamp(0.0, 1.0);
            (x - x0 - dx * k).hypot(y - y0 - dy * k) - t * 0.5
        });
    }
    /// A filled polygon (even-odd), four samples per pixel.
    fn poly(&mut self, pts: &[(f32, f32)], c: Rgb, alpha: f32) {
        let inside = |x: f32, y: f32| {
            let mut hit = false;
            let mut j = pts.len() - 1;
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[j]);
                if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                    hit = !hit;
                }
                j = i;
            }
            hit
        };
        let (w, h) = self.size();
        let fold = |f: fn(f32, f32) -> f32, pick: fn(&(f32, f32)) -> f32, start: f32| pts.iter().map(pick).fold(start, f);
        let (x0, x1) = (fold(f32::min, |p| p.0, w).max(0.0) as i32, fold(f32::max, |p| p.0, 0.0).min(w - 1.0) as i32);
        let (y0, y1) = (fold(f32::min, |p| p.1, h).max(0.0) as i32, fold(f32::max, |p| p.1, 0.0).min(h - 1.0) as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let n = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)].iter().filter(|o| inside(x as f32 + o.0, y as f32 + o.1)).count();
                self.blend(x, y, c, n as f32 / 4.0 * alpha);
            }
        }
    }
    /// A grid of dots turned 45 degrees; `radius(x, y)` sizes each dot (0 or less skips it).
    fn halftone(&mut self, spacing: f32, c: Rgb, radius: impl Fn(f32, f32) -> f32) {
        let (w, h) = self.size();
        let n = ((w + h) / spacing) as i32 + 2;
        for i in -n..n {
            for j in 0..n {
                let (x, y) = ((i + j) as f32 * spacing * 0.5, (j - i) as f32 * spacing * 0.5);
                if x < -spacing || y < -spacing || x > w + spacing || y > h + spacing {
                    continue;
                }
                let r = radius(x, y);
                if r > 0.3 {
                    self.circle(x, y, r, c, 1.0);
                }
            }
        }
    }
    fn text(&mut self, x: f32, y: f32, scale: u32, c: Rgb, text: &str) {
        font::draw_text(&mut self.img, x as i32, y as i32, scale, [c[0], c[1], c[2], 255], text);
    }
    /// A caption box: paper, an ink outline, one line of lettering and rows of text-like dashes.
    fn caption(&mut self, x: f32, y: f32, w: f32, h: f32, title: &str, rng: &mut Rng) {
        self.rect(x - 5.0, y - 5.0, x + w + 5.0, y + h + 5.0, INK, 1.0);
        self.rect(x, y, x + w, y + h, PAPER, 1.0);
        let scale = ((w - 28.0) / (title.len() as f32 * font::ADVANCE as f32)).floor().clamp(1.0, 6.0) as u32;
        self.text(x + 14.0, y + 12.0, scale, INK, title);
        let mut row = y + 24.0 + (font::GLYPH_H * scale) as f32;
        while row < y + h - 12.0 {
            let mut cx = x + 14.0;
            while cx < x + w - 40.0 {
                let len = rng.range(14.0, 64.0).min(x + w - 16.0 - cx);
                self.rect(cx, row, cx + len, row + 5.0, INK, 0.85);
                cx += len + rng.range(8.0, 16.0);
            }
            row += 15.0;
        }
    }
    fn border(&mut self, t: f32) {
        let (w, h) = self.size();
        for (x0, y0, x1, y1) in [(0.0, 0.0, w, t), (0.0, h - t, w, h), (0.0, 0.0, t, h), (w - t, 0.0, w, h)] {
            self.rect(x0, y0, x1, y1, INK, 1.0);
        }
    }
}

fn star(cx: f32, cy: f32, outer: f32, inner: f32, points: usize, turn: f32) -> Vec<(f32, f32)> {
    (0..points * 2)
        .map(|i| {
            let a = turn + i as f32 * std::f32::consts::PI / points as f32;
            let r = if i % 2 == 0 { outer } else { inner };
            (cx + a.cos() * r, cy + a.sin() * r)
        })
        .collect()
}

/// Slot `a`: a warm action panel. Sunburst, halftone, an impact star and big lettering.
fn panel_a() -> RgbaImage {
    let (w, h) = (1200.0, 1500.0);
    let mut c = Canvas::new(w as u32, h as u32, PAPER);
    let mut rng = Rng::new(11);
    let (fx, fy) = (600.0, 660.0);
    for i in 0..24 {
        let (a0, a1) = (i as f32 * 0.2618, i as f32 * 0.2618 + 0.1309);
        c.poly(&[(fx, fy), (fx + a0.cos() * 1700.0, fy + a0.sin() * 1700.0), (fx + a1.cos() * 1700.0, fy + a1.sin() * 1700.0)], [240, 138, 52], 1.0);
    }
    c.halftone(34.0, [196, 52, 40], |x, y| ((x - fx).hypot(y - fy) - 420.0) / 62.0);
    c.poly(&star(fx, fy, 520.0, 330.0, 14, 0.1), INK, 1.0);
    c.poly(&star(fx, fy, 496.0, 344.0, 14, 0.1), PAPER, 1.0);
    c.circle(fx, fy, 292.0, [214, 44, 46], 1.0);
    c.halftone(22.0, [150, 24, 40], |x, y| if (x - fx).hypot(y - fy) < 280.0 { (y - fy + 60.0) / 44.0 } else { 0.0 });
    c.ring(fx, fy, 292.0, 14.0, INK);
    // A caped figure in front of the sun.
    c.poly(&[(470.0, 1010.0), (560.0, 640.0), (640.0, 640.0), (760.0, 1010.0), (690.0, 960.0), (600.0, 1030.0), (520.0, 960.0)], INK, 1.0);
    c.circle(600.0, 590.0, 62.0, INK, 1.0);
    c.poly(&[(560.0, 660.0), (300.0, 560.0), (330.0, 520.0), (585.0, 610.0)], INK, 1.0);
    c.poly(&[(640.0, 660.0), (900.0, 500.0), (880.0, 450.0), (618.0, 610.0)], INK, 1.0);
    for _ in 0..90 {
        let a = rng.range(0.0, std::f32::consts::TAU);
        let (r0, r1) = (rng.range(560.0, 700.0), rng.range(820.0, 1400.0));
        c.line(fx + a.cos() * r0, fy + a.sin() * r0, fx + a.cos() * r1, fy + a.sin() * r1, rng.range(2.0, 7.0), INK, 1.0);
    }
    c.text(252.0, 1152.0, 24, INK, "BEAT!");
    c.text(240.0, 1140.0, 24, [250, 214, 70], "BEAT!");
    c.caption(60.0, 60.0, 430.0, 150.0, "PANEL A - THE DROP", &mut rng);
    c.border(22.0);
    c.img
}

/// Slot `b`: a cool night city. Gradient sky, moon, lit windows, rain and a neon sign.
fn panel_b() -> RgbaImage {
    let (w, h) = (1080.0, 1080.0);
    let mut c = Canvas::new(w as u32, h as u32, [10, 14, 40]);
    let mut rng = Rng::new(23);
    for y in 0..h as u32 {
        let k = y as f32 / h;
        let (top, mid, low): (Rgb, Rgb, Rgb) = ([10, 14, 44], [58, 36, 112], [226, 66, 128]);
        let (p, q, t) = if k < 0.6 { (top, mid, k / 0.6) } else { (mid, low, (k - 0.6) / 0.4) };
        let col = [0, 1, 2].map(|i| (p[i] as f32 + (q[i] as f32 - p[i] as f32) * t) as u8);
        c.rect(0.0, y as f32, w, y as f32 + 1.0, col, 1.0);
    }
    for _ in 0..170 {
        c.circle(rng.range(0.0, w), rng.range(0.0, h * 0.6), rng.range(0.8, 2.6), [236, 240, 255], rng.range(0.4, 1.0));
    }
    c.glow(790.0, 250.0, 330.0, [250, 240, 200], 0.35);
    c.circle(790.0, 250.0, 118.0, [250, 242, 206], 1.0);
    for (dx, dy, r) in [(-40.0, -30.0, 26.0), (36.0, 22.0, 34.0), (-8.0, 58.0, 16.0), (52.0, -48.0, 14.0)] {
        c.circle(790.0 + dx, 250.0 + dy, r, [214, 200, 170], 0.8);
    }
    for (depth, shade) in [[36u8, 30, 84], [22, 20, 56], [12, 10, 28]].into_iter().enumerate() {
        let mut x = -20.0;
        while x < w {
            let (bw, bh) = (rng.range(70.0, 170.0), rng.range(180.0, 420.0) + depth as f32 * 90.0 - 60.0);
            let top = h - bh;
            c.rect(x, top, x + bw, h, shade, 1.0);
            if rng.unit() < 0.35 {
                c.rect(x + bw * 0.45, top - 60.0, x + bw * 0.55, top, shade, 1.0);
            }
            let lit: Rgb = if rng.unit() < 0.5 { [255, 214, 110] } else { [120, 230, 240] };
            let mut wy = top + 16.0;
            while wy < h - 20.0 {
                let mut wx = x + 12.0;
                while wx < x + bw - 16.0 {
                    if rng.unit() < 0.42 {
                        c.rect(wx, wy, wx + 12.0, wy + 16.0, lit, 0.55 + 0.2 * depth as f32);
                    }
                    wx += 24.0;
                }
                wy += 30.0;
            }
            x += bw + rng.range(-10.0, 24.0);
        }
    }
    c.glow(300.0, 520.0, 300.0, [255, 60, 160], 0.4);
    c.rect(118.0, 438.0, 482.0, 602.0, INK, 1.0);
    c.rect(130.0, 450.0, 470.0, 590.0, [40, 8, 50], 1.0);
    c.text(153.0, 478.0, 11, [130, 20, 90], "EDITS");
    c.text(148.0, 473.0, 11, [255, 96, 190], "EDITS");
    for _ in 0..150 {
        let (x, y, l) = (rng.range(-100.0, w), rng.range(-100.0, h), rng.range(40.0, 130.0));
        c.line(x, y, x + l * 0.35, y + l, rng.range(1.2, 2.6), [190, 214, 255], rng.range(0.15, 0.45));
    }
    c.caption(620.0, 880.0, 400.0, 140.0, "PANEL B - NIGHT", &mut rng);
    c.border(22.0);
    c.img
}

/// Slot `c`: a pop-art portrait. Stripes, a face, halftone cheeks and a speech bubble.
fn panel_c() -> RgbaImage {
    let (w, h) = (1080.0, 1080.0);
    let mut c = Canvas::new(w as u32, h as u32, [250, 208, 62]);
    let mut rng = Rng::new(37);
    for i in -12..24 {
        let x = i as f32 * 90.0;
        c.poly(&[(x, 0.0), (x + 40.0, 0.0), (x + 40.0 + h * 0.5, h), (x + h * 0.5, h)], [255, 232, 130], 1.0);
    }
    c.halftone(30.0, [240, 96, 150], |x, y| (360.0 - (x - 80.0).hypot(y - 1000.0)) / 34.0);
    c.poly(&[(140.0, 1080.0), (260.0, 880.0), (460.0, 820.0), (620.0, 820.0), (820.0, 880.0), (940.0, 1080.0)], [28, 150, 142], 1.0);
    c.rect(470.0, 720.0, 610.0, 860.0, [236, 178, 150], 1.0);
    c.ellipse(540.0, 520.0, 250.0, 310.0, INK, 1.0);
    c.ellipse(540.0, 520.0, 238.0, 298.0, [250, 204, 174], 1.0);
    c.halftone(18.0, [244, 120, 140], |x, y| ((70.0 - (x - 400.0).hypot(y - 640.0)).max(70.0 - (x - 680.0).hypot(y - 640.0)) / 10.0).min(6.0));
    // Hair: a dark cap with jagged bangs.
    let mut hair = vec![(270.0, 560.0), (250.0, 380.0), (330.0, 210.0), (540.0, 150.0), (750.0, 210.0), (840.0, 380.0), (820.0, 580.0)];
    for i in 0..9 {
        let x = 800.0 - i as f32 * 62.0;
        hair.push((x, if i % 2 == 0 { 330.0 } else { 440.0 + rng.range(-20.0, 30.0) }));
    }
    c.poly(&hair, [34, 30, 78], 1.0);
    for (ex, look) in [(440.0, 8.0), (640.0, 8.0)] {
        c.ellipse(ex, 540.0, 56.0, 38.0, INK, 1.0);
        c.ellipse(ex, 540.0, 48.0, 30.0, [255, 255, 255], 1.0);
        c.circle(ex + look, 542.0, 22.0, [40, 110, 180], 1.0);
        c.circle(ex + look, 542.0, 10.0, INK, 1.0);
        c.circle(ex + look + 8.0, 534.0, 5.0, [255, 255, 255], 1.0);
        c.line(ex - 60.0, 478.0, ex + 56.0, 462.0 + (ex - 540.0) * 0.06, 12.0, [34, 30, 78], 1.0);
    }
    c.line(540.0, 560.0, 520.0, 660.0, 7.0, INK, 0.9);
    c.line(520.0, 660.0, 556.0, 664.0, 7.0, INK, 0.9);
    c.ellipse(540.0, 730.0, 74.0, 26.0, INK, 1.0);
    c.ellipse(540.0, 730.0, 66.0, 19.0, [222, 48, 70], 1.0);
    // Speech bubble with scribbled lettering.
    c.poly(&[(760.0, 300.0), (700.0, 400.0), (820.0, 320.0)], INK, 1.0);
    c.ellipse(850.0, 200.0, 190.0, 130.0, INK, 1.0);
    c.ellipse(850.0, 200.0, 180.0, 120.0, [255, 255, 255], 1.0);
    c.poly(&[(772.0, 300.0), (716.0, 384.0), (812.0, 312.0)], [255, 255, 255], 1.0);
    c.text(752.0, 140.0, 9, INK, "OH?!");
    for (i, len) in [150.0f32, 110.0, 60.0].into_iter().enumerate() {
        c.rect(760.0, 222.0 + i as f32 * 18.0, 760.0 + len + rng.range(0.0, 40.0), 229.0 + i as f32 * 18.0, INK, 0.85);
    }
    for i in 0..7 {
        let y = 150.0 + i as f32 * 46.0;
        c.line(60.0, y, 120.0, y + 23.0, 9.0, INK, 1.0);
        c.line(120.0, y + 23.0, 60.0, y + 46.0, 9.0, INK, 1.0);
    }
    c.caption(60.0, 900.0, 380.0, 120.0, "PANEL C - HER", &mut rng);
    c.border(22.0);
    c.img
}

/// Slot `d`: a wide sunset page cut into three panels by gutters.
fn panel_d() -> RgbaImage {
    let (w, h) = (1600.0, 1000.0);
    let mut c = Canvas::new(w as u32, h as u32, [70, 40, 120]);
    let mut rng = Rng::new(53);
    for y in 0..620 {
        let t = y as f32 / 620.0;
        let (p, q): (Rgb, Rgb) = ([64, 36, 122], [252, 150, 70]);
        c.rect(0.0, y as f32, w, y as f32 + 1.0, [0, 1, 2].map(|i| (p[i] as f32 + (q[i] as f32 - p[i] as f32) * t * t) as u8), 1.0);
    }
    for (i, r) in (0..9).map(|i| (i, 300.0 - i as f32 * 32.0)) {
        c.circle(800.0, 560.0, r, if i % 2 == 0 { [255, 226, 150] } else { [250, 120, 70] }, 1.0);
    }
    for _ in 0..14 {
        let (x, y, s) = (rng.range(120.0, 1480.0), rng.range(90.0, 330.0), rng.range(14.0, 30.0));
        c.line(x - s, y - s * 0.5, x, y, 5.0, INK, 1.0);
        c.line(x, y, x + s, y - s * 0.6, 5.0, INK, 1.0);
    }
    for (peak, base, shade) in [((330.0, 300.0), 420.0, [58u8, 30, 96]), ((1180.0, 250.0), 520.0, [46, 24, 82]), ((760.0, 410.0), 360.0, [34, 18, 62])] {
        let (px, py) = peak;
        c.poly(&[(px - base, 640.0), (px - base * 0.3, py + 130.0), (px, py), (px + base * 0.4, py + 170.0), (px + base, 640.0)], shade, 1.0);
        c.poly(&[(px - base * 0.3, py + 130.0), (px, py), (px + base * 0.4, py + 170.0), (px + base * 0.12, py + 120.0), (px - base * 0.05, py + 160.0)], [250, 232, 220], 0.9);
        for k in 0..12 {
            let x = px - base * 0.6 + k as f32 * base * 0.1;
            c.line(x, 630.0, x + 60.0, 630.0 - (base * 0.35 - (x - px).abs() * 0.3).max(10.0), 3.0, [120, 80, 170], 0.5);
        }
    }
    for band in 0..9 {
        let y = 640.0 + band as f32 * 40.0;
        let shade: Rgb = if band % 2 == 0 { [36, 60, 110] } else { [58, 36, 104] };
        c.rect(0.0, y, w, y + 40.0, shade, 1.0);
        let mut x = rng.range(-60.0, 0.0);
        while x < w {
            let len = rng.range(40.0, 150.0);
            c.line(x, y + 20.0, x + len, y + 20.0 + rng.range(-5.0, 5.0), 4.0 + band as f32 * 0.5, [250, 170, 110], 0.6);
            x += len + rng.range(30.0, 110.0);
        }
    }
    c.glow(800.0, 700.0, 420.0, [255, 190, 110], 0.35);
    // Gutters: two slanted white bars with ink edges split the page in three.
    for x in [520.0, 1060.0] {
        c.poly(&[(x - 8.0, 0.0), (x + 52.0, 0.0), (x - 12.0, h), (x - 72.0, h)], INK, 1.0);
        c.poly(&[(x, 0.0), (x + 44.0, 0.0), (x - 20.0, h), (x - 64.0, h)], PAPER, 1.0);
    }
    c.caption(56.0, 56.0, 380.0, 132.0, "MEANWHILE...", &mut rng);
    c.text(1150.0, 850.0, 9, INK, "FIN");
    c.text(1144.0, 844.0, 9, [255, 236, 190], "FIN");
    c.border(22.0);
    c.img
}

/// Slot `tex`: a light grungy overlay on black, meant for the `screen` blend.
fn texture() -> RgbaImage {
    let (w, h) = (1080.0f32, 1080.0f32);
    let mut c = Canvas::new(w as u32, h as u32, [0, 0, 0]);
    let mut rng = Rng::new(71);
    for _ in 0..16 {
        let warm: Rgb = if rng.unit() < 0.5 { [200, 170, 140] } else { [150, 160, 190] };
        c.glow(rng.range(0.0, w), rng.range(0.0, h), rng.range(160.0, 420.0), warm, rng.range(0.03, 0.09));
    }
    // Wear gathers toward the edges.
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let edge = (x as f32).min(w - x as f32).min(y as f32).min(h - y as f32) / 160.0;
            let grain = rng.unit().powi(5);
            c.blend(x, y, [210, 204, 190], grain * (0.10 + 0.5 * (1.0 - edge).max(0.0).powi(2)));
        }
    }
    for _ in 0..700 {
        c.circle(rng.range(0.0, w), rng.range(0.0, h), rng.range(0.5, 2.4), [236, 230, 214], rng.range(0.2, 0.85));
    }
    for _ in 0..46 {
        let (x, y, len) = (rng.range(0.0, w), rng.range(-200.0, h), rng.range(120.0, 900.0));
        c.line(x, y, x + rng.range(-28.0, 28.0), y + len, rng.range(0.8, 1.8), [226, 222, 210], rng.range(0.12, 0.5));
    }
    // Hairs: short curls made of a few segments.
    for _ in 0..18 {
        let (mut x, mut y, mut a) = (rng.range(0.0, w), rng.range(0.0, h), rng.range(0.0, std::f32::consts::TAU));
        let turn = rng.range(-0.3, 0.3);
        for _ in 0..rng.range(6.0, 16.0) as u32 {
            let (nx, ny) = (x + a.cos() * 14.0, y + a.sin() * 14.0);
            c.line(x, y, nx, ny, 1.3, [230, 226, 212], 0.55);
            (x, y, a) = (nx, ny, a + turn + rng.range(-0.15, 0.15));
        }
    }
    c.img
}

/// A click track: a kick on every beat, a hat between beats and a clap on beats 2 and 6.
fn beat_track(bpm: f64, beats: u32) -> Vec<i16> {
    const RATE: f64 = 44_100.0;
    let beat = 60.0 / bpm;
    let total = (beats as f64 * beat * RATE).round() as usize;
    let mut out = vec![0f64; total];
    let mut rng = Rng::new(97);
    let noise: Vec<f64> = (0..(0.2 * RATE) as usize).map(|_| rng.unit() as f64 * 2.0 - 1.0).collect();
    for b in 0..beats * 2 {
        let start = (b as f64 * 0.5 * beat * RATE) as usize;
        for i in 0..(0.26 * RATE) as usize {
            let Some(s) = out.get_mut(start + i) else { break };
            let t = i as f64 / RATE;
            if b % 2 == 0 {
                // Kick: a sine that drops from 150 Hz to 48 Hz.
                let phase = std::f64::consts::TAU * (48.0 * t + (150.0 - 48.0) * (1.0 - (-t * 28.0).exp()) / 28.0);
                *s += phase.sin() * (-t * 11.0).exp() * 0.85;
                if (b / 2) % 4 == 1 && i < noise.len() {
                    *s += noise[i] * (-t * 26.0).exp() * 0.35;
                }
            } else if i > 0 && i < noise.len() {
                // Hat: differenced noise is mostly treble.
                *s += (noise[i] - noise[i - 1]) * (-t * 70.0).exp() * 0.22;
            }
        }
    }
    out.iter().map(|v| (v.clamp(-1.0, 1.0) * 30_000.0) as i16).collect()
}

fn write_wav(path: &Path, samples: &[i16]) -> Result<()> {
    let data = samples.len() as u32 * 2;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 0x0001_0001, 44_100, 88_200, 0x0010_0002] {
        // chunk size; PCM, mono; sample rate; byte rate; block align 2, 16 bits
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    bytes.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

/// The example recipe with its slots pointing at the generated pictures and
/// the click track attached.
pub fn project_text() -> String {
    let mut text = RECIPE.to_string();
    for slot in SLOTS {
        let empty = format!("\"{slot}\": {{ \"fit\"");
        assert!(text.contains(&empty), "examples/beat-loop.json no longer has an empty slot \"{slot}\"");
        text = text.replace(&empty, &format!("\"{slot}\": {{ \"path\": \"img/{slot}.png\", \"fit\""));
    }
    let slots = "\n  \"slots\": {";
    assert!(text.contains(slots), "examples/beat-loop.json no longer has a top-level \"slots\"");
    text.replacen(slots, &format!("\n  \"audio\": {{ \"path\": \"{AUDIO}\", \"offset\": 0.0 }},{slots}"), 1)
}

/// Write the demo into `dir` and return the files written, the project first.
pub fn write(dir: &Path) -> Result<Vec<String>> {
    let text = project_text();
    let project = edits_core::Project::from_json(&text).context("the built-in demo project")?;
    std::fs::create_dir_all(dir.join("img")).with_context(|| format!("cannot create {}", dir.join("img").display()))?;
    std::fs::create_dir_all(dir.join("audio")).with_context(|| format!("cannot create {}", dir.join("audio").display()))?;
    let mut files = vec!["project.json".to_string()];
    std::fs::write(dir.join("project.json"), &text).with_context(|| format!("cannot write {}", dir.join("project.json").display()))?;
    type Picture = (&'static str, fn() -> RgbaImage);
    let pictures: [Picture; 5] = [("a", panel_a), ("b", panel_b), ("c", panel_c), ("d", panel_d), ("tex", texture)];
    for (slot, draw) in pictures {
        let name = format!("img/{slot}.png");
        draw().save(dir.join(&name)).with_context(|| format!("cannot write {}", dir.join(&name).display()))?;
        files.push(name);
    }
    let s = &project.settings;
    write_wav(&dir.join(AUDIO), &beat_track(s.bpm.unwrap_or(120.0), s.duration.ceil() as u32))?;
    files.push(AUDIO.to_string());
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_project_is_valid_and_complete() {
        let dir = std::env::temp_dir().join(format!("edits-demo-test-{}", std::process::id()));
        let files = write(&dir).unwrap();
        assert_eq!(files.len(), 7);
        let project = edits_core::Project::from_json(&std::fs::read_to_string(dir.join("project.json")).unwrap()).unwrap();
        let findings = edits_core::check::check(&project, Some(&dir));
        assert!(findings.is_empty(), "{findings:#?}");
        // Every picture has its own palette and real detail.
        let mut means = Vec::new();
        for slot in ["a", "b", "c", "d"] {
            let img = image::open(dir.join(format!("img/{slot}.png"))).unwrap().to_rgb8();
            let n = (img.width() * img.height()) as f64;
            let mean = [0, 1, 2].map(|i| img.pixels().map(|p| p[i] as f64).sum::<f64>() / n);
            let spread = img.pixels().map(|p| (p[0] as f64 - mean[0]).abs()).sum::<f64>() / n;
            assert!(spread > 25.0, "{slot} looks flat: {spread}");
            means.push(mean);
        }
        for i in 0..means.len() {
            for j in 0..i {
                let d: f64 = (0..3).map(|k| (means[i][k] - means[j][k]).abs()).sum();
                assert!(d > 20.0, "pictures {i} and {j} have the same palette");
            }
        }
        // The overlay stays dark so the screen blend does not wash the picture out.
        let tex = image::open(dir.join("img/tex.png")).unwrap().to_luma8();
        let mean = tex.pixels().map(|p| p[0] as f64).sum::<f64>() / (tex.width() * tex.height()) as f64;
        assert!(mean > 1.0 && mean < 30.0, "{mean}");
        // 8 beats at 123 BPM of 16-bit mono at 44.1 kHz.
        let wav = std::fs::metadata(dir.join(AUDIO)).unwrap().len();
        assert_eq!(wav, 44 + 2 * (8.0 * 60.0 / 123.0 * 44_100.0f64).round() as u64);
        let again = write(&dir).unwrap();
        assert_eq!(files, again);
        std::fs::remove_dir_all(&dir).ok();
    }
}
