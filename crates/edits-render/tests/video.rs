//! Video layers: a 48-frame clip whose colour encodes the frame index is
//! encoded losslessly (FFV1) into the test folder, and each test reads the
//! index back from a rendered pixel. Skipped without ffmpeg or an adapter.

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
    let path = dir().join(name);
    if path.is_file() {
        return Some(name.to_string());
    }
    let child = Command::new(std::env::var("EDITS_FFMPEG").unwrap_or_else(|_| "ffmpeg".into()))
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", "16x16", "-framerate", &FPS.to_string(), "-i", "pipe:0", "-c:v", "ffv1"])
        .arg(&path)
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("skipping video tests: ffmpeg is not available");
        return None;
    };
    let mut raw = Vec::new();
    for i in 0..FRAMES {
        let [r, g, b] = colour(i);
        for _ in 0..16 * 16 {
            raw.extend_from_slice(&[r, g, b, 255]);
        }
    }
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
    let text = format!(
        r#"{{"format":"edits.project/1","settings":{{"width":32,"height":32,"fps":{FPS},"duration":{duration}}},
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
