//! Helpers for the tests of the built-in one-pass effects (`fx_*.rs`):
//! small projects and generated pictures.
#![allow(dead_code)]

use crate::common::*;
use edits_core::Project;
use image::{Rgba, RgbaImage};

/// A `w` x `h` project (seconds, 10 fps) showing the picture `file` full
/// frame, with `effects` on it.
pub fn picture(w: u32, h: u32, file: &str, effects: &str) -> Project {
    let text = format!(
        r#"{{"format":"edits.project/1","settings":{{"width":{w},"height":{h},"fps":10,"duration":4}},
            "slots":{{"a":{{"path":"{file}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","effects":[{effects}]}}]}}"#
    );
    Project::from_json(&text).unwrap()
}

/// Like [`picture`] with a second slot `m` (a map) holding `map`.
pub fn picture_with_map(file: &str, map: &str, effects: &str) -> Project {
    let text = format!(
        r#"{{"format":"edits.project/1","settings":{{"width":64,"height":64,"fps":10,"duration":4}},
            "slots":{{"a":{{"path":"{file}"}},"m":{{"path":"{map}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","effects":[{effects}]}}]}}"#
    );
    Project::from_json(&text).unwrap()
}

/// A 64 x 64 full-frame solid of `color` (sRGB) with `effects` on it.
pub fn solid(color: [f64; 3], effects: &str) -> Project {
    project("", &format!(r#""layers":[{{"id":"s","kind":"solid","color":[{},{},{},1],"effects":[{effects}]}}]"#, color[0], color[1], color[2]))
}

/// A white box of `size` (fraction of the frame) at the centre of a black
/// 64 x 64 frame, with `effects` on an adjustment layer above it.
pub fn boxed(size: f64, effects: &str) -> Project {
    project(
        "",
        &format!(
            r#""layers":[{{"id":"bg","kind":"solid","color":[0,0,0,1]}},{{"id":"box","kind":"solid","color":[1,1,1,1],"transform":{{"scale":[{size},{size}]}}}},
                         {{"id":"fx","kind":"adjustment","effects":[{effects}]}}]"#
        ),
    )
}

/// A smooth colour picture: red across, green down, blue the other way.
pub fn gradient(name: &str, size: u32) -> String {
    let img = RgbaImage::from_fn(size, size, |x, y| {
        let (u, v) = (x as f64 / (size - 1) as f64, y as f64 / (size - 1) as f64);
        Rgba([(255.0 * u) as u8, (255.0 * v) as u8, (255.0 * (1.0 - 0.5 * (u + v))) as u8, 255])
    });
    save(name, &img)
}

/// 64 x 64, black left of x = `split`, white from it.
pub fn split(name: &str, split: u32) -> String {
    save(name, &RgbaImage::from_fn(64, 64, |x, _| if x < split { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) }))
}

/// 64 x 64, top half `top`, bottom half `bottom`.
pub fn halves(name: &str, top: [u8; 3], bottom: [u8; 3]) -> String {
    save(name, &RgbaImage::from_fn(64, 64, |_, y| {
        let c = if y < 32 { top } else { bottom };
        Rgba([c[0], c[1], c[2], 255])
    }))
}

/// 64 x 64 horizontal stripes 4 pixels high, colours varying down only.
pub fn rows(name: &str) -> String {
    save(name, &RgbaImage::from_fn(64, 64, |_, y| {
        let k = (y / 4) as u8;
        Rgba([k * 16, 255 - k * 16, (k % 2) * 255, 255])
    }))
}

/// 64 x 64 noise (a fixed integer hash), for effects that smooth.
pub fn noisy(name: &str) -> String {
    save(name, &RgbaImage::from_fn(64, 64, |x, y| {
        let h = (x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663)).wrapping_mul(2654435761);
        Rgba([(h >> 24) as u8, (h >> 16) as u8, (h >> 8) as u8, 255])
    }))
}

/// Mean linear value of each channel over an `n` x `n` grid of blocks.
pub fn blocks(img: &RgbaImage, n: u32) -> Vec<[f64; 3]> {
    let (bw, bh) = (img.width() / n, img.height() / n);
    let mut out = Vec::new();
    for by in 0..n {
        for bx in 0..n {
            let mut sum = [0.0; 3];
            for y in by * bh..(by + 1) * bh {
                for x in bx * bw..(bx + 1) * bw {
                    let p = img.get_pixel(x, y).0;
                    for c in 0..3 {
                        sum[c] += to_linear(p[c] as f64 / 255.0);
                    }
                }
            }
            out.push(sum.map(|v| v / (bw * bh) as f64));
        }
    }
    out
}

/// Largest difference of any channel between two pictures.
pub fn max_diff(a: &RgbaImage, b: &RgbaImage) -> i32 {
    a.pixels().zip(b.pixels()).flat_map(|(p, q)| (0..4).map(move |c| (p.0[c] as i32 - q.0[c] as i32).abs())).max().unwrap_or(0)
}

/// The number of distinct colours in a picture.
pub fn colours(img: &RgbaImage) -> usize {
    let mut c: Vec<[u8; 4]> = img.pixels().map(|p| p.0).collect();
    c.sort_unstable();
    c.dedup();
    c.len()
}

/// Render `p` at scales 1 and 0.5 and compare 8 x 8 block means (linear).
pub fn scale_difference(r: &mut edits_render::Renderer, p: &Project) -> f64 {
    let full = blocks(&render_in(r, p, &dir(), 0, 1.0), 8);
    let half = blocks(&render_in(r, p, &dir(), 0, 0.5), 8);
    full.iter().zip(&half).flat_map(|(a, b)| (0..3).map(move |c| (a[c] - b[c]).abs())).fold(0.0, f64::max)
}
