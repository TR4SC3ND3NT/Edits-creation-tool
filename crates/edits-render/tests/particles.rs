//! Pixel tests of particles layers: frames are pure functions of the time,
//! bursts land on their beat, gravity pulls by g t² / 2, the seed matters,
//! every sprite draws, and the layer blends and blurs like any other.
//! Every test skips when the machine has no adapter.

mod common;
use common::*;
use image::RgbaImage;

macro_rules! gpu {
    ($r:ident) => {
        let Some(mut guard) = renderer() else { return };
        let $r = guard.as_mut().unwrap();
    };
}

/// A 64x64, 10 fps project with one particles layer over black.
fn particles(settings_extra: &str, layer: &str) -> edits_core::Project {
    project(settings_extra, &format!(r#""layers":[{{"id":"p","kind":"particles",{layer}}}]"#))
}

/// Total brightness (sum of channels) and its centroid in pixels.
fn light(img: &RgbaImage) -> (f64, [f64; 2]) {
    let (mut sum, mut x, mut y) = (0.0, 0.0, 0.0);
    for (px, py, p) in img.enumerate_pixels() {
        let v = p[0] as f64 + p[1] as f64 + p[2] as f64;
        sum += v;
        x += v * (px as f64 + 0.5);
        y += v * (py as f64 + 0.5);
    }
    if sum == 0.0 {
        return (0.0, [0.0, 0.0]);
    }
    (sum, [x / sum, y / sum])
}

#[test]
fn a_frame_is_a_pure_function_of_its_time() {
    gpu!(r);
    let p = particles("", r#""rate":30,"life":1.5,"speed":0.3,"size":5,"sprite":"star","turbulence":{"amount":4},"spin":120"#);
    let a = render(r, &p, 17);
    let _ = render(r, &p, 3);
    let _ = render(r, &p, 31);
    let b = render(r, &p, 17);
    assert!(light(&a).0 > 0.0);
    assert_eq!(a.as_raw(), b.as_raw(), "the same frame twice differs");
    assert_ne!(a.as_raw(), render(r, &p, 18).as_raw(), "particles do not move");
}

#[test]
fn a_burst_shows_on_its_beat_and_not_a_frame_before() {
    gpu!(r);
    // 120 BPM at 10 fps: a frame is 0.2 beats, beat 2 is frame 10.
    let p = particles(r#","time_base":"beats","bpm":120"#, r#""bursts":[{"t":2,"count":40}],"life":1,"speed":0.4,"size":4"#);
    assert_eq!(light(&render(r, &p, 9)).0, 0.0, "the burst shows a frame early");
    assert!(light(&render(r, &p, 10)).0 > 0.0, "the burst is missing on its beat");
    assert!(light(&render(r, &p, 14)).0 > 0.0);
    // Gone once its life (1 beat = 5 frames) is over.
    assert_eq!(light(&render(r, &p, 15)).0, 0.0);
}

#[test]
fn text_times_are_accepted_for_bursts() {
    let mut doc = serde_json::json!({
        "format": "edits.project/1",
        "settings": { "width": 64, "height": 64, "fps": 10, "duration": 8, "time_base": "beats", "bpm": 120 },
        "markers": [ { "t": 3, "label": "drop" } ],
        "layers": [ { "id": "p", "kind": "particles", "bursts": [ { "t": "b6", "count": 5 }, { "t": "drop+1b", "count": 2 } ] } ]
    });
    edits_core::time::resolve_doc(&mut doc, None).unwrap();
    assert_eq!(doc["layers"][0]["bursts"][0]["t"], serde_json::json!(6.0));
    assert_eq!(doc["layers"][0]["bursts"][1]["t"], serde_json::json!(4.0));
    let p: edits_core::Project = serde_json::from_value(doc).unwrap();
    assert!(!edits_core::check::has_errors(&edits_core::check::check(&p, None)));
}

#[test]
fn gravity_pulls_the_centroid_down_by_half_g_t_squared() {
    gpu!(r);
    // A row of still particles from t = 0; g = 0.5 heights / s^2.
    let p = particles("", r#""bursts":[{"t":0,"count":12}],"life":4,"speed":0,"size":6,"gravity":0.5,"emitter":{"shape":"line","size":[0.6,0],"position":[0,-0.3]}"#);
    let y0 = light(&render(r, &p, 0)).1[1];
    for frame in [5u64, 10] {
        let tau = frame as f64 / 10.0;
        let (sum, c) = light(&render(r, &p, frame));
        assert!(sum > 0.0);
        let want = 0.5 * 0.5 * tau * tau * 64.0;
        assert!((c[1] - y0 - want).abs() < 0.6, "after {tau} s the centroid moved {} px, not {want}", c[1] - y0);
    }
}

#[test]
fn the_seed_changes_the_picture() {
    gpu!(r);
    let layer = |seed: u32| format!(r#""rate":20,"life":2,"speed":0.25,"size":5,"seed":{seed}"#);
    let a = render(r, &particles("", &layer(1)), 12);
    let b = render(r, &particles("", &layer(2)), 12);
    assert!(light(&a).0 > 0.0 && light(&b).0 > 0.0);
    assert_ne!(a.as_raw(), b.as_raw());
}

#[test]
fn every_sprite_draws() {
    gpu!(r);
    let pic = flat("sprite-green.png", 8, 8, [0, 255, 0, 255]);
    for sprite in ["dot", "spark", "star", "petal", "slot:pic"] {
        let p = project(
            "",
            &format!(
                r#""slots":{{"pic":{{"path":"{pic}"}}}},
                   "layers":[{{"id":"p","kind":"particles","bursts":[{{"t":0,"count":6}}],"life":4,"speed":0.3,"size":12,"sprite":"{sprite}","color":[1,0.6,0.8]}}]"#
            ),
        );
        let img = render(r, &p, 3);
        let (sum, _) = light(&img);
        assert!(sum > 2000.0, "{sprite} draws almost nothing ({sum})");
        if sprite == "slot:pic" {
            // The picture's green, tinted by the colour's green.
            let (g, other) = img.pixels().fold((0u64, 0u64), |(g, o), p| (g + p[1] as u64, o + p[0] as u64 + p[2] as u64));
            assert!(g > 10 * other.max(1), "the picture is not shown: green {g}, red and blue {other}");
        }
    }
}

#[test]
fn a_spark_streaks_along_its_velocity() {
    gpu!(r);
    // Two particles flying right: the streak is wider than tall.
    let p = particles("", r#""bursts":[{"t":0,"count":1}],"life":4,"speed":1,"direction":0,"spread":0,"size":3,"streak":0.2,"sprite":"spark","emitter":{"position":[-0.4,0]}"#);
    let img = render(r, &p, 2);
    let lit: Vec<(u32, u32)> = img.enumerate_pixels().filter(|(_, _, p)| p[0] > 40).map(|(x, y, _)| (x, y)).collect();
    let span = |f: fn(&(u32, u32)) -> u32| lit.iter().map(f).max().unwrap() - lit.iter().map(f).min().unwrap();
    assert!(span(|p| p.0) > 3 * span(|p| p.1).max(1), "not a horizontal streak: {lit:?}");
}

#[test]
fn the_layer_blends_moves_and_blurs_like_any_other() {
    gpu!(r);
    let base = r#""rate":20,"life":2,"speed":0.3,"size":6,"color":[1,0.2,0.2]"#;
    // Add over grey brightens; multiply by white-ish dots changes nothing much darker than grey.
    let grey = r#","background":[0.5,0.5,0.5]"#;
    let normal = render(r, &particles(grey, base), 15);
    let added = render(r, &particles(grey, &format!(r#"{base},"blend":"add""#)), 15);
    assert_ne!(normal.as_raw(), added.as_raw());
    assert!(light(&added).0 > light(&normal).0);
    // The layer's position moves every particle.
    let still = r#""bursts":[{"t":0,"count":10}],"life":4,"speed":0,"size":6,"emitter":{"shape":"rect","size":[0.3,0.3]}"#;
    let here = light(&render(r, &particles("", still), 2)).1;
    let moved = light(&render(r, &particles("", &format!(r#"{still},"transform":{{"position":[0.25,0,0]}}"#)), 2)).1;
    assert!((moved[0] - here[0] - 16.0).abs() < 0.5, "{here:?} -> {moved:?}");
    // Motion blur: sub-frame samples smear fast particles.
    let fast = r#""bursts":[{"t":0,"count":4}],"life":4,"speed":0.5,"size":4"#;
    let sharp = render(r, &particles("", fast), 5);
    let blurred = render(r, &particles(r#","motion_blur":{"samples":8,"shutter":0.5}"#, fast), 5);
    let lit = |img: &RgbaImage| img.pixels().filter(|p| p[0] > 8).count();
    assert!(lit(&blurred) > lit(&sharp), "motion blur does not smear: {} vs {}", lit(&blurred), lit(&sharp));
}
