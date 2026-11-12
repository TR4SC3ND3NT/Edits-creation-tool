//! CPU-side picture preparation: loading, premultiplying and the placeholder
//! drawn for empty slots.

use crate::font;
use anyhow::{anyhow, Result};
use image::{Rgba, RgbaImage};
use std::path::Path;

pub(crate) fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Load a picture as straight-alpha sRGB RGBA8, no larger than `max_side`.
pub(crate) fn load(path: &Path, max_side: u32) -> Result<RgbaImage> {
    let img = image::open(path).map_err(|e| anyhow!("cannot load image {}: {e}", path.display()))?;
    let mut img = img.into_rgba8();
    if img.width() == 0 || img.height() == 0 {
        return Err(anyhow!("image {} is empty", path.display()));
    }
    let side = img.width().max(img.height());
    if side > max_side {
        let k = max_side as f64 / side as f64;
        let (w, h) = (((img.width() as f64 * k).round() as u32).max(1), ((img.height() as f64 * k).round() as u32).max(1));
        img = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
    }
    Ok(img)
}

/// Multiply colour by alpha in linear light, keeping the sRGB encoding, so the
/// texture filters without dark or bright fringes at transparent edges.
pub(crate) fn premultiply(img: &mut RgbaImage) {
    let decode: Vec<f32> = (0..256).map(|i| srgb_to_linear(i as f32 / 255.0)).collect();
    for px in img.pixels_mut() {
        let a = px[3];
        if a == 255 {
            continue;
        }
        if a == 0 {
            *px = Rgba([0, 0, 0, 0]);
            continue;
        }
        let af = a as f32 / 255.0;
        for c in 0..3 {
            px[c] = (linear_to_srgb(decode[px[c] as usize] * af) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn name_hash(name: &str) -> u32 {
    // FNV-1a: stable across runs and platforms.
    name.bytes().fold(0x811C_9DC5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let k = |n: f32| {
        let x = (n + h * 6.0) % 6.0;
        v - v * s * x.min(4.0 - x).clamp(0.0, 1.0)
    };
    [(k(5.0) * 255.0) as u8, (k(3.0) * 255.0) as u8, (k(1.0) * 255.0) as u8]
}

/// The stand-in for a slot without a picture: a checker tinted by the slot's
/// name, a frame, two diagonals and the name itself.
pub fn placeholder(name: &str, size: u32) -> RgbaImage {
    let hue = (name_hash(name) % 360) as f32 / 360.0;
    let (dark, light, ink) = (hsv(hue, 0.55, 0.38), hsv(hue, 0.45, 0.62), hsv(hue, 0.25, 0.95));
    let cell = (size / 8).max(1);
    let border = (size / 64).max(1);
    let mut img = RgbaImage::from_fn(size, size, |x, y| {
        let edge = x < border || y < border || x >= size - border || y >= size - border;
        let diagonal = x.abs_diff(y) < border / 2 + 1 || x.abs_diff(size - 1 - y) < border / 2 + 1;
        let c = if edge || diagonal {
            ink
        } else if (x / cell + y / cell) % 2 == 0 {
            dark
        } else {
            light
        };
        Rgba([c[0], c[1], c[2], 255])
    });
    let label: String = name.chars().take(12).collect();
    let scale = (size / (label.chars().count().max(4) as u32 * font::ADVANCE * 2)).max(1);
    let (tw, th) = (font::text_width(&label, scale), font::GLYPH_H * scale);
    let (x, y) = ((size.saturating_sub(tw)) / 2, (size.saturating_sub(th)) / 2);
    let pad = scale * 2;
    for py in y.saturating_sub(pad)..(y + th + pad).min(size) {
        for px in x.saturating_sub(pad)..(x + tw + pad).min(size) {
            img.put_pixel(px, py, Rgba([10, 10, 12, 255]));
        }
    }
    font::draw_text(&mut img, x as i32, y as i32, scale, [ink[0], ink[1], ink[2], 255], &label);
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_scales_in_linear_light() {
        let mut img = RgbaImage::from_pixel(1, 1, Rgba([255, 128, 0, 128]));
        premultiply(&mut img);
        let p = img.get_pixel(0, 0).0;
        // Half of linear 1.0 is sRGB 188; half of sRGB 128 (linear 0.216) is sRGB 93.
        assert!((p[0] as i32 - 188).abs() <= 1 && (p[1] as i32 - 93).abs() <= 1 && p[2] == 0 && p[3] == 128, "{p:?}");
    }

    #[test]
    fn placeholders_differ_by_slot_name() {
        let (a, b) = (placeholder("a", 64), placeholder("b", 64));
        assert_eq!(a.dimensions(), (64, 64));
        assert_ne!(a.get_pixel(12, 4), b.get_pixel(12, 4));
        assert_eq!(a, placeholder("a", 64));
    }
}
