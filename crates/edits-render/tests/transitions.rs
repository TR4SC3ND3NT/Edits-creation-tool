//! Pixel tests of every transition in the registry: it starts on the picture
//! beneath, ends on the plain cut, does something in between, and draws an
//! opaque, finite picture throughout. Every test skips when the machine has
//! no adapter.

mod common;
use common::*;
use edits_core::effects::TRANSITIONS;
use image::RgbaImage;

/// Mean absolute difference per channel, 0..255.
fn diff(a: &RgbaImage, b: &RgbaImage) -> f64 {
    let n = (a.width() * a.height() * 3) as f64;
    a.pixels().zip(b.pixels()).map(|(p, q)| (0..3).map(|i| (p[i] as f64 - q[i] as f64).abs()).sum::<f64>()).sum::<f64>() / n
}

/// Quadrants beneath, an orange picture coming in over 1 s from 1 s
/// (frames 10..20 at 10 fps), with `transition` on it.
fn scene(transition: &str) -> edits_core::Project {
    let from = quadrants("tr-from.png", 64, 64);
    let to = flat("tr-to.png", 64, 64, [250, 140, 30, 255]);
    project(
        "",
        &format!(
            r#""slots":{{"a":{{"path":"{from}"}},"b":{{"path":"{to}"}}}},
               "layers":[{{"id":"a","kind":"image","slot":"a","out":2}},
                         {{"id":"b","kind":"image","slot":"b","in":1{transition}}}]"#
        ),
    )
}

#[test]
fn every_transition_goes_from_the_picture_beneath_to_the_cut() {
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let plain = scene("");
    let before = render(r, &plain, 9);
    let after = render(r, &plain, 10);
    assert!(diff(&before, &after) > 40.0, "the two pictures differ");
    let mut report = Vec::new();
    for t in TRANSITIONS {
        let p = scene(&format!(r#","transition":{{"kind":"{}","duration":1}}"#, t.kind));
        let start = render(r, &p, 10);
        let middle = render(r, &p, 15);
        let end = render(r, &p, 20);
        assert!(start.pixels().chain(middle.pixels()).all(|q| q[3] == 255), "{}: the frame stays opaque", t.kind);
        let (d0, d1) = (diff(&start, &before), diff(&end, &after));
        let (m0, m1) = (diff(&middle, &before), diff(&middle, &after));
        report.push(format!("{:<18} start-from {d0:6.2}  end-cut {d1:6.2}  middle-from {m0:6.2}  middle-cut {m1:6.2}", t.kind));
        assert!(d0 < 12.0, "{}: progress 0 shows the picture beneath (difference {d0:.2})", t.kind);
        assert!(d1 < 0.5, "{}: after the transition the plain cut remains (difference {d1:.2})", t.kind);
        assert!(m0 > 3.0 && m1 > 3.0, "{}: something happens mid-way ({m0:.2} from the first picture, {m1:.2} from the second)", t.kind);
    }
    eprintln!("{}", report.join("\n"));
}

#[test]
fn a_fade_is_a_mix_in_srgb_and_its_parameters_reach_the_shader() {
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let black = flat("tr-black.png", 64, 64, [0, 0, 0, 255]);
    let white = flat("tr-white.png", 64, 64, [255, 255, 255, 255]);
    let make = |transition: &str| {
        project(
            "",
            &format!(
                r#""slots":{{"a":{{"path":"{black}"}},"b":{{"path":"{white}"}}}},
                   "layers":[{{"id":"a","kind":"image","slot":"a","out":2}},
                             {{"id":"b","kind":"image","slot":"b","in":1,"transition":{transition}}}]"#
            ),
        )
    };
    // Half way through a crossfade from black to white is sRGB mid grey, as in gl-transitions.
    let mid = render(r, &make(r#"{"kind":"fade","duration":1}"#), 15);
    assert_px(&mid, 32, 32, [128, 128, 128], 3);
    // A dip through red: the middle of the transition is the colour.
    let dip = render(r, &make(r#"{"kind":"fade_color","duration":1,"params":{"color":[1,0,0,1],"color_phase":0.6}}"#), 15);
    assert_px(&dip, 32, 32, [255, 0, 0], 3);
    // Easing reaches the progress: "hold" stays on the first picture until the end.
    let held = render(r, &make(r#"{"kind":"fade","duration":1,"ease":"hold"}"#), 18);
    assert_px(&held, 32, 32, [0, 0, 0], 2);
}
