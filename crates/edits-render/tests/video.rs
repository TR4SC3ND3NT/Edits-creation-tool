//! Video layers: a 48-frame clip whose colour encodes the frame index is
//! encoded losslessly (FFV1) into the test folder, and each test reads the
//! index back from a rendered pixel. `frame_blend: flow` is tested on clips
//! of a noise texture that moves by a known number of pixels per frame.
//! Skipped without ffmpeg or an adapter.

mod common;

use common::*;
use edits_render::Renderer;
use std::io::Write;
use std::process::{Command, Stdio};

const FRAMES: u32 = 48;
const FPS: u32 = 24;

/// The colour of source frame `i`: red counts up by 5, green down.
fn colour(i: u32) -> [u8; 3] {
    [(i * 5) as u8, (255 - i * 5) as u8, 40]
}

/// Encode the counting clip as `name` in the test folder; `None` (with a
/// note) when ffmpeg cannot.
fn clip(name: &str) -> Option<String> {
    encode(name, 16, 16, FRAMES, |i| {
        let [r, g, b] = colour(i);
        [r, g, b, 255].repeat(16 * 16)
    })
}

/// Encode `frames` RGBA frames of `w` x `h` (made by `frame`) losslessly as
/// `name` in the test folder at `FPS`; `None` (with a note) when ffmpeg cannot.
fn encode(name: &str, w: u32, h: u32, frames: u32, frame: impl Fn(u32) -> Vec<u8>) -> Option<String> {
    let path = dir().join(name);
    if path.is_file() {
        return Some(name.to_string());
    }
    let child = Command::new(std::env::var("EDITS_FFMPEG").unwrap_or_else(|_| "ffmpeg".into()))
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"), "-framerate", &FPS.to_string(), "-i", "pipe:0", "-c:v", "ffv1"])
        .arg(&path)
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("skipping video tests: ffmpeg is not available");
        return None;
    };
    let raw: Vec<u8> = (0..frames).flat_map(&frame).collect();
    let wrote = child.stdin.take().expect("piped").write_all(&raw);
    let status = child.wait().ok();
    if wrote.is_err() || !status.is_some_and(|s| s.success()) {
        eprintln!("skipping video tests: ffmpeg could not encode an FFV1 clip");
        return None;
    }
    Some(name.to_string())
}

/// A 32x32, 24 fps project of `duration` seconds with one video layer;
/// `layer` and `slot` are spliced into the layer and the slot.
fn video_project(file: &str, duration: f64, layer: &str, slot: &str) -> edits_core::Project {
    sized_project(file, 32, 32, duration, layer, slot)
}

fn sized_project(file: &str, w: u32, h: u32, duration: f64, layer: &str, slot: &str) -> edits_core::Project {
    let text = format!(
        r#"{{"format":"edits.project/1","settings":{{"width":{w},"height":{h},"fps":{FPS},"duration":{duration}}},
            "slots":{{"v":{{"path":"{file}"{slot}}}}},
            "layers":[{{"id":"clip","kind":"video","slot":"v"{layer}}}]}}"#
    );
    edits_core::Project::from_json(&text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

/// The source frame shown at output frame `frame`, read from the centre pixel.
#[track_caller]
fn shown(r: &mut Renderer, p: &edits_core::Project, frame: u64) -> u32 {
    let img = render(r, p, frame);
    let [red, green, _] = px(&img, 16, 16);
    let i = (red as f64 / 5.0).round() as u32;
    assert!((red as i32 - colour(i)[0] as i32).abs() <= 1 && (green as i32 - colour(i)[1] as i32).abs() <= 1, "frame {frame}: {red},{green} is no source frame");
    i
}

/// Run `body` with the shared renderer and a clip; skips without either.
fn with_clip(name: &str, body: impl FnOnce(&mut Renderer, &str)) {
    let Some(file) = clip(name) else { return };
    let Some(mut guard) = renderer() else { return };
    body(guard.as_mut().expect("checked by renderer()"), &file);
}

#[test]
fn speed_one_two_and_a_half() {
    with_clip("count.mkv", |r, f| {
        let p = video_project(f, 2.0, "", "");
        assert_eq!([0, 1, 5, 13, 23, 47].map(|n| shown(r, &p, n)), [0, 1, 5, 13, 23, 47]);
        let p = video_project(f, 4.0, r#","speed":2"#, "");
        assert_eq!([0, 3, 10, 23, 30].map(|n| shown(r, &p, n)), [0, 6, 20, 46, 47], "double speed holds the last frame");
        let p = video_project(f, 4.0, r#","speed":0.5"#, "");
        assert_eq!([0, 1, 2, 7, 60].map(|n| shown(r, &p, n)), [0, 0, 1, 3, 30]);
    });
}

#[test]
fn time_remap_reverse_and_loop() {
    with_clip("count.mkv", |r, f| {
        // Source seconds 1 -> 0 over the first second: frame n shows 24 - n.
        let p = video_project(f, 2.0, r#","time_remap":{"keys":[{"t":0,"v":1},{"t":1,"v":0}]}"#, "");
        assert_eq!([0, 4, 20, 24, 30].map(|n| shown(r, &p, n)), [24, 20, 4, 0, 0]);
        let p = video_project(f, 2.0, r#","reverse":true"#, "");
        assert_eq!([0, 10, 47].map(|n| shown(r, &p, n)), [47, 37, 0]);
        let p = video_project(f, 4.0, r#","loop":true"#, "");
        assert_eq!([47, 48, 50, 95].map(|n| shown(r, &p, n)), [47, 0, 2, 47]);
        // A layer that starts later starts its clip later.
        let p = video_project(f, 4.0, r#","in":1"#, "");
        assert_eq!(shown(r, &p, 30), 6);
    });
}

#[test]
fn layer_fps_holds_frames() {
    with_clip("count.mkv", |r, f| {
        let p = video_project(f, 2.0, r#","fps":12"#, "");
        assert_eq!([0, 1, 2, 3, 4, 5, 9].map(|n| shown(r, &p, n)), [0, 0, 2, 2, 4, 4, 8]);
    });
}

#[test]
fn trim_picks_the_part_of_the_source() {
    with_clip("count.mkv", |r, f| {
        let p = video_project(f, 4.0, "", r#","trim":[0.5,1.5]"#);
        assert_eq!([0, 10, 23, 30].map(|n| shown(r, &p, n)), [12, 22, 35, 35]);
        let p = video_project(f, 4.0, r#","loop":true"#, r#","trim":[0.5,1.5]"#);
        assert_eq!(shown(r, &p, 25), 13);
    });
}

#[test]
fn mix_at_a_half_frame_averages_in_linear_light() {
    with_clip("count.mkv", |r, f| {
        let p = video_project(f, 2.0, r#","speed":0.5,"frame_blend":"mix""#, "");
        // Output frame 7 is source position 3.5.
        let img = render(r, &p, 7);
        let average = |c: usize| byte((to_linear(colour(3)[c] as f64 / 255.0) + to_linear(colour(4)[c] as f64 / 255.0)) / 2.0);
        assert_px(&img, 16, 16, [average(0), average(1), average(2)], 1);
        // On a whole frame it is that frame.
        assert_eq!(shown(r, &p, 8), 4);
    });
}

#[test]
fn decoding_reads_forward_and_restarts_backwards() {
    with_clip("seek.mkv", |r, f| {
        let p = video_project(f, 2.0, "", "");
        let before = r.video_stats();
        assert_eq!(shown(r, &p, 40), 40);
        assert_eq!(r.video_stats().decoders_started, before.decoders_started + 1);
        // Backwards: a new decoder, and the right frame.
        assert_eq!(shown(r, &p, 3), 3);
        let restarted = r.video_stats();
        assert_eq!(restarted.decoders_started, before.decoders_started + 2);
        // Forwards from there: the same decoder reads on.
        for n in 4..20 {
            assert_eq!(shown(r, &p, n), n as u32);
        }
        assert_eq!(r.video_stats().decoders_started, restarted.decoders_started);
        // Frames read before come from the cache: the same pixels, no decoding.
        let decoded = r.video_stats().frames_decoded;
        assert_eq!(shown(r, &p, 40), 40);
        assert_eq!(r.video_stats().frames_decoded, decoded);
    });
}

#[test]
fn empty_video_slots_show_a_placeholder() {
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let p = project("", r#""slots":{"v":{}},"layers":[{"id":"clip","kind":"video","slot":"v"}]"#);
    let img = render(r, &p, 0);
    assert_ne!(px(&img, 32, 32), [0, 0, 0]);
}

#[test]
fn layer_fps_holds_motion_and_shares_motion_blur_samples() {
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let p = project(
        r#","motion_blur":{"samples":4,"shutter":1}"#,
        r#""layers":[{"id":"box","kind":"solid","color":[1,1,1,1],"fps":5,
             "transform":{"scale":0.25,"position":{"keys":[{"t":0,"v":[-0.5,0,0]},{"t":4,"v":[0.5,0,0]}]}}}]"#,
    );
    // A 10 fps project with the layer at 5: frames 0 and 1 are one step, so
    // every motion blur instant of either frame is the same picture.
    let a = render(r, &p, 0);
    assert_eq!(r.stats().rendered_samples, 1);
    assert_eq!(a, render(r, &p, 1));
    assert_ne!(a, render(r, &p, 2));
}

// --- frame_blend: flow -------------------------------------------------

const TW: u32 = 96;
const TH: u32 = 64;
/// Pixels the texture moves per source frame.
const STEP: (f64, f64) = (3.0, 1.0);

fn hash(x: i64, y: i64, seed: u64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x8da6_b343) ^ (y as u64).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f64 / 65535.0
}

/// Smooth value noise with features about `cell` pixels across.
fn noise(x: f64, y: f64, cell: f64, seed: u64) -> f64 {
    let (gx, gy) = (x / cell, y / cell);
    let (i, j) = (gx.floor() as i64, gy.floor() as i64);
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (smooth(gx - i as f64), smooth(gy - j as f64));
    let top = hash(i, j, seed) * (1.0 - tx) + hash(i + 1, j, seed) * tx;
    let bottom = hash(i, j + 1, seed) * (1.0 - tx) + hash(i + 1, j + 1, seed) * tx;
    top * (1.0 - ty) + bottom * ty
}

/// The texture moved by (dx, dy) pixels, as RGBA bytes.
fn textured(dx: f64, dy: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity((TW * TH * 4) as usize);
    for y in 0..TH {
        for x in 0..TW {
            let (fx, fy) = (x as f64 + 0.5 - dx, y as f64 + 0.5 - dy);
            for seed in 1..4 {
                out.push(((0.7 * noise(fx, fy, 6.0, seed) + 0.3 * noise(fx, fy, 3.0, seed + 7)) * 255.0).round() as u8);
            }
            out.push(255);
        }
    }
    out
}

/// A 12-frame clip of the texture moving by `STEP` per frame (`moving`) or standing still.
fn texture_clip(name: &str, moving: bool) -> Option<String> {
    let k = if moving { 1.0 } else { 0.0 };
    encode(name, TW, TH, 12, |i| textured(k * STEP.0 * i as f64, k * STEP.1 * i as f64))
}

/// The project frame of a `TW` x `TH` project showing `file` at half speed.
fn half_speed(r: &mut Renderer, file: &str, blend: &str, frame: u64) -> image::RgbaImage {
    let p = sized_project(file, TW, TH, 1.0, &format!(r#","speed":0.5,"frame_blend":"{blend}""#), "");
    render(r, &p, frame)
}

/// Root mean square difference of the colour bytes, `MARGIN` pixels in from the edges.
const MARGIN: u32 = 10;
fn rms(img: &image::RgbaImage, truth: &[u8]) -> f64 {
    let (mut sum, mut n) = (0.0, 0.0);
    for y in MARGIN..TH - MARGIN {
        for x in MARGIN..TW - MARGIN {
            for c in 0..3 {
                let e = img.get_pixel(x, y)[c] as f64 - truth[((y * TW + x) * 4) as usize + c] as f64;
                sum += e * e;
                n += 1.0;
            }
        }
    }
    (sum / n).sqrt()
}

/// Mean squared difference of neighbouring bytes (sharpness).
fn gradient_energy(img: &image::RgbaImage) -> f64 {
    let (mut sum, mut n) = (0.0, 0.0);
    for y in MARGIN..TH - MARGIN {
        for x in MARGIN..TW - MARGIN {
            for c in 0..3 {
                let v = img.get_pixel(x, y)[c] as f64;
                let (dx, dy) = (img.get_pixel(x + 1, y)[c] as f64 - v, img.get_pixel(x, y + 1)[c] as f64 - v);
                sum += dx * dx + dy * dy;
                n += 1.0;
            }
        }
    }
    sum / n
}

#[test]
fn flow_in_between_is_the_texture_half_way() {
    let Some(file) = texture_clip("moving.mkv", true) else { return };
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    // A whole source frame is that frame.
    assert!(rms(&half_speed(r, &file, "flow", 2), &textured(STEP.0, STEP.1)) < 1.0);
    // Output frame 3 is source position 1.5: the texture moved 1.5 steps.
    let flow = half_speed(r, &file, "flow", 3);
    let mix = half_speed(r, &file, "mix", 3);
    // The shift that best explains the in-between, on a quarter-pixel grid around the truth.
    let truth = (1.5 * STEP.0, 1.5 * STEP.1);
    let mut best = (f64::INFINITY, (0.0, 0.0));
    for i in -8..=8 {
        for j in -8..=8 {
            let s = (truth.0 + i as f64 * 0.25, truth.1 + j as f64 * 0.25);
            let e = rms(&flow, &textured(s.0, s.1));
            if e < best.0 {
                best = (e, s);
            }
        }
    }
    let off = ((best.1 .0 - truth.0).powi(2) + (best.1 .1 - truth.1).powi(2)).sqrt();
    assert!(off < 1.0, "the in-between is shifted by {:?}, {off} px from the half-way shift {truth:?}", best.1);
    let reference = textured(truth.0, truth.1);
    let (e_flow, e_mix) = (rms(&flow, &reference), rms(&mix, &reference));
    assert!(e_flow < e_mix / 3.0, "error against the truth: flow {e_flow}, mix {e_mix}");
    let (g_flow, g_mix) = (gradient_energy(&flow), gradient_energy(&mix));
    assert!(g_flow > 1.3 * g_mix, "flow is not sharper than mix: gradient energy {g_flow} vs {g_mix}");
}

#[test]
fn flow_on_a_still_clip_is_the_mix() {
    let Some(file) = texture_clip("still.mkv", false) else { return };
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    for frame in [3, 5] {
        assert_eq!(half_speed(r, &file, "flow", frame), half_speed(r, &file, "mix", frame));
    }
}

#[test]
fn flow_is_estimated_once_per_frame_pair_and_is_deterministic() {
    let Some(file) = texture_clip("moving-cache.mkv", true) else { return };
    let Some(again) = texture_clip("moving-again.mkv", true) else { return };
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    // Quarter speed: output frames 5, 6 and 7 are source positions 1.25, 1.5, 1.75.
    let p = sized_project(&file, TW, TH, 1.0, r#","speed":0.25,"frame_blend":"flow""#, "");
    let before = r.video_stats().flows_estimated;
    let frames: Vec<_> = (5..8).map(|n| render(r, &p, n)).collect();
    assert_eq!(r.video_stats().flows_estimated, before + 1);
    assert!(frames[0] != frames[1] && frames[1] != frames[2]);
    // Another file with the same frames: estimated afresh, the same pixels.
    let q = sized_project(&again, TW, TH, 1.0, r#","speed":0.25,"frame_blend":"flow""#, "");
    assert_eq!(render(r, &q, 6), frames[1]);
    assert_eq!(r.video_stats().flows_estimated, before + 2);
}
