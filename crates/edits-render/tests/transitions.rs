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
        // Half way, or at a third for the few that are not in between there (a star covers the
        // frame by then; glitch_flicker flashes back to the outgoing picture).
        let (mid_frame, middle) = [15, 13].into_iter().map(|f| (f, render(r, &p, f))).find(|(_, m)| diff(m, &before) > 3.0 && diff(m, &after) > 3.0).unwrap_or_else(|| (15, render(r, &p, 15)));
        let end = render(r, &p, 20);
        assert!(start.pixels().chain(middle.pixels()).all(|q| q[3] == 255), "{}: the frame stays opaque", t.kind);
        assert!(render(r, &p, mid_frame).as_raw() == middle.as_raw(), "{}: the same frame renders the same picture", t.kind);
        let (d0, d1) = (diff(&start, &before), diff(&end, &after));
        let (m0, m1) = (diff(&middle, &before), diff(&middle, &after));
        report.push(format!("{:<18} start-from {d0:6.2}  end-cut {d1:6.2}  middle({mid_frame})-from {m0:6.2}  middle-cut {m1:6.2}", t.kind));
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

/// The colour of pixel (x, y) is within `tolerance` of the same pixel in `want`.
fn like(img: &RgbaImage, want: &RgbaImage, x: u32, y: u32, tolerance: i32) -> bool {
    let (a, b) = (img.get_pixel(x, y), want.get_pixel(x, y));
    (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= tolerance)
}

/// Half way through, the shapes of the wipes are where their formulas put them
/// (the quadrants go out, flat orange comes in; image y grows downward).
#[test]
fn the_wipes_put_their_edges_where_the_formula_says() {
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let plain = scene("");
    let (from, to) = (render(r, &plain, 9), render(r, &plain, 10));
    let check = |img: &RgbaImage, name: &str, want_to: &[(u32, u32)], want_from: &[(u32, u32)]| {
        for &(x, y) in want_to {
            assert!(like(img, &to, x, y, 4), "{name}: ({x}, {y}) shows the next picture, got {:?}", img.get_pixel(x, y));
        }
        for &(x, y) in want_from {
            assert!(like(img, &from, x, y, 4), "{name}: ({x}, {y}) still shows the outgoing picture, got {:?}", img.get_pixel(x, y));
        }
    };
    let mut mid = |t: &str| render(r, &scene(&format!(r#","transition":{t}"#)), 15);
    // A left-to-right wipe is half way across.
    let img = mid(r#"{"kind":"linear_wipe","duration":1,"params":{"direction":[1,0],"smoothness":0.01}}"#);
    check(&img, "linear_wipe", &[(4, 10), (20, 50)], &[(44, 10), (60, 50)]);
    // A clock hand from 12 o'clock, counter-clockwise: the left half is done.
    let img = mid(r#"{"kind":"clock_wipe","duration":1}"#);
    check(&img, "clock_wipe", &[(8, 32), (20, 12), (20, 52)], &[(56, 32), (44, 12), (44, 52)]);
    // An opening circle: the centre is open, the corners are not.
    let img = mid(r#"{"kind":"circle_open","duration":1,"params":{"smoothness":0.05}}"#);
    check(&img, "circle_open", &[(32, 32), (24, 24)], &[(1, 1), (62, 62)]);
    // ... and a closing one the other way round.
    let img = mid(r#"{"kind":"circle_open","duration":1,"params":{"smoothness":0.05,"opening":0}}"#);
    check(&img, "circle_open closing", &[(1, 1), (62, 62)], &[(32, 32), (24, 24)]);
    // A horizontal opening: the middle rows first, the top and bottom last.
    let img = mid(r#"{"kind":"horizontal_open","duration":1}"#);
    check(&img, "horizontal_open", &[(10, 32), (54, 31)], &[(10, 1), (54, 62)]);
    // Two halves sliding in: the top one from the left, the bottom one from the right.
    let img = mid(r#"{"kind":"split_slide","duration":1}"#);
    check(&img, "split_slide", &[(8, 8), (56, 56)], &[(56, 8), (8, 56)]);
    let img = mid(r#"{"kind":"split_slide","duration":1,"params":{"reverse":1}}"#);
    check(&img, "split_slide reversed", &[(56, 8), (8, 56)], &[(8, 8), (56, 56)]);
}

#[test]
fn dissolves_switch_whole_pixels_and_static_is_grey() {
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let plain = scene("");
    let (from, to) = (render(r, &plain, 9), render(r, &plain, 10));
    for t in [r#"{"kind":"noise_dissolve","duration":1}"#, r#"{"kind":"random_squares","duration":1,"params":{"smoothness":0}}"#] {
        let img = render(r, &scene(&format!(r#","transition":{t}"#)), 15);
        let (mut n_from, mut n_to) = (0, 0);
        for (x, y, _) in img.enumerate_pixels() {
            match (like(&img, &from, x, y, 3), like(&img, &to, x, y, 3)) {
                (true, _) => n_from += 1,
                (_, true) => n_to += 1,
                _ => panic!("{t}: ({x}, {y}) is neither picture: {:?}", img.get_pixel(x, y)),
            }
        }
        assert!(n_from > 200 && n_to > 200, "{t}: both pictures show half way ({n_from} and {n_to} pixels)");
    }
    let img = render(r, &scene(r#","transition":{"kind":"tv_static","duration":1}"#), 15);
    assert!(img.pixels().all(|p| p[0] == p[1] && p[1] == p[2]), "tv_static: the static is grey");
    let levels: std::collections::BTreeSet<u8> = img.pixels().map(|p| p[0]).collect();
    assert!(levels.len() > 32, "tv_static: the static is noise ({} levels)", levels.len());
}

#[test]
fn a_luma_wipe_follows_its_map() {
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let from = quadrants("tr-luma-from.png", 64, 64);
    let to = flat("tr-luma-to.png", 64, 64, [250, 140, 30, 255]);
    // Black on the left to white on the right.
    let ramp = save("tr-luma-ramp.png", &RgbaImage::from_fn(64, 64, |x, _| image::Rgba([(x * 4 + 2) as u8, (x * 4 + 2) as u8, (x * 4 + 2) as u8, 255])));
    let make = |transition: &str| {
        project(
            "",
            &format!(
                r#""slots":{{"a":{{"path":"{from}"}},"b":{{"path":"{to}"}},"m":{{"path":"{ramp}"}}}},
                   "layers":[{{"id":"a","kind":"image","slot":"a","out":2}},
                             {{"id":"b","kind":"image","slot":"b","in":1,"transition":{transition}}}]"#
            ),
        )
    };
    let plain = make(r#"{"kind":"fade","duration":0.001}"#);
    let (before, after) = (render(r, &plain, 9), render(r, &plain, 11));
    let wipe = make(r#"{"kind":"luma_wipe","duration":1,"source":"slot:m","params":{"softness":0}}"#);
    assert!(like(&render(r, &wipe, 10), &before, 60, 60, 4), "progress 0 is the outgoing picture");
    // Half way (in sRGB brightness), the dark left side has switched, the bright right side has not.
    let img = render(r, &wipe, 15);
    for y in [8, 56] {
        assert!(like(&img, &after, 8, y, 4) && like(&img, &after, 24, y, 4), "the dark side shows the next picture");
        assert!(like(&img, &before, 40, y, 4) && like(&img, &before, 56, y, 4), "the bright side still shows the outgoing picture");
    }
    // Inverted, the other way round.
    let img = render(r, &make(r#"{"kind":"luma_wipe","duration":1,"source":"slot:m","params":{"softness":0,"invert":1}}"#), 15);
    assert!(like(&img, &before, 8, 8, 4) && like(&img, &after, 56, 8, 4), "invert sends the bright side first");
    // A soft edge is a gradient: between the switched and the unswitched sides there are mixes.
    let img = render(r, &make(r#"{"kind":"luma_wipe","duration":1,"source":"slot:m","params":{"softness":0.5}}"#), 15);
    let mixed = (0..64).filter(|&x| !like(&img, &before, x, 8, 6) && !like(&img, &after, x, 8, 6)).count();
    assert!(mixed > 8, "a soft edge mixes the pictures ({mixed} columns)");
    // A map that is not a slot, or a map on a transition that reads none, is an error of `check`.
    for bad in [r#"{"kind":"luma_wipe","duration":1,"source":"slot:nope"}"#, r#"{"kind":"fade","duration":1,"source":"slot:m"}"#] {
        assert!(edits_core::check::has_errors(&edits_core::check::check(&make(bad), None)), "{bad}");
    }
}
