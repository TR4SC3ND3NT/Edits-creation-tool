//! Shared helpers: one renderer for the whole test binary, tiny projects and
//! procedurally generated pictures in a temporary folder.
#![allow(dead_code)]

use edits_core::Project;
use edits_render::{RenderOptions, Renderer};
use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

static RENDERER: OnceLock<Mutex<Option<Renderer>>> = OnceLock::new();

/// The shared renderer, or `None` (with a note on stderr) when this machine
/// has no usable adapter, so that tests skip instead of failing.
pub fn renderer() -> Option<MutexGuard<'static, Option<Renderer>>> {
    let cell = RENDERER.get_or_init(|| {
        Mutex::new(match Renderer::new(RenderOptions::default()) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("skipping GPU tests: {e:#}");
                None
            }
        })
    });
    let guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    guard.is_some().then_some(guard)
}

/// A folder for this test run's pictures.
pub fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("edits-render-tests-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn save(name: &str, img: &RgbaImage) -> String {
    img.save(dir().join(name)).unwrap();
    name.to_string()
}

/// Four quadrants: top-left red, top-right green, bottom-left blue, bottom-right yellow.
pub fn quadrants(name: &str, w: u32, h: u32) -> String {
    let img = RgbaImage::from_fn(w, h, |x, y| match (x < w / 2, y < h / 2) {
        (true, true) => Rgba([255, 0, 0, 255]),
        (false, true) => Rgba([0, 255, 0, 255]),
        (true, false) => Rgba([0, 0, 255, 255]),
        (false, false) => Rgba([255, 255, 0, 255]),
    });
    save(name, &img)
}

pub fn flat(name: &str, w: u32, h: u32, c: [u8; 4]) -> String {
    save(name, &RgbaImage::from_pixel(w, h, Rgba(c)))
}

/// A 64x64, 10 fps, 4 second project in seconds; `rest` is spliced in after the settings.
pub fn project(settings_extra: &str, rest: &str) -> Project {
    let text = format!(
        r#"{{"format":"edits.project/1",
            "settings":{{"width":64,"height":64,"fps":10,"duration":4{settings_extra}}},
            {rest}}}"#
    );
    Project::from_json(&text).unwrap_or_else(|e| panic!("bad test project: {e}\n{text}"))
}

pub fn render(r: &mut Renderer, p: &Project, frame: u64) -> RgbaImage {
    render_in(r, p, &dir(), frame, 1.0)
}

pub fn render_in(r: &mut Renderer, p: &Project, base: &Path, frame: u64, scale: f32) -> RgbaImage {
    let findings = edits_core::check::check(p, Some(base));
    assert!(!edits_core::check::has_errors(&findings), "{findings:#?}");
    r.set_motion_blur_samples(None);
    r.prepare(p, base).unwrap();
    r.render_frame(frame, scale).unwrap()
}

pub fn to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

pub fn to_srgb(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// The 8-bit value of a linear-light value.
pub fn byte(linear: f64) -> u8 {
    (to_srgb(linear) * 255.0).round() as u8
}

pub fn px(img: &RgbaImage, x: u32, y: u32) -> [u8; 3] {
    let p = img.get_pixel(x, y).0;
    [p[0], p[1], p[2]]
}

#[track_caller]
pub fn assert_px(img: &RgbaImage, x: u32, y: u32, want: [u8; 3], tolerance: i32) {
    let got = px(img, x, y);
    let ok = (0..3).all(|i| (got[i] as i32 - want[i] as i32).abs() <= tolerance);
    assert!(ok, "pixel ({x}, {y}) is {got:?}, expected {want:?} +-{tolerance}");
}

/// Sum of linear-light red, green and blue over the whole picture.
pub fn energy(img: &RgbaImage) -> f64 {
    img.pixels().map(|p| (0..3).map(|i| to_linear(p[i] as f64 / 255.0)).sum::<f64>()).sum()
}
