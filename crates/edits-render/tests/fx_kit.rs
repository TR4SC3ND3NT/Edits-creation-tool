//! Pixel tests of the effects ported from the Death Note kit: shatter,
//! pinch, glitch_bands, film_scratches, grain_highlights and drip. Every
//! test skips when the machine has no adapter.

mod common;
mod fxcommon;
use common::*;
use fxcommon::*;
use image::RgbaImage;

macro_rules! gpu {
    ($r:ident) => {
        let Some(mut guard) = renderer() else { return };
        let $r = guard.as_mut().unwrap();
    };
}

/// Pixels whose red, green and blue are all below `v`.
fn dark(img: &RgbaImage, v: u8) -> usize {
    img.pixels().filter(|p| p.0[0] < v && p.0[1] < v && p.0[2] < v).count()
}

/// A full-frame grey solid (sRGB `v`) with `effects` on it.
fn grey(v: f64, effects: &str) -> edits_core::Project {
    solid([v, v, v], effects)
}

#[test]
fn shatter_is_whole_before_the_start_and_breaks_after_it() {
    gpu!(r);
    let pic = gradient("kit-gradient.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    // Before the start (t 0 < 1) every shard sits in place: the picture is unchanged
    // up to the rounding of the shards' homographies and their shared edges.
    let fx = r#"{"kind":"shatter","params":{"start":1,"spread":20,"depth":20,"spin":60,"jag":0}}"#;
    let before = render(r, &picture(64, 64, &pic, fx), 0);
    assert!(max_diff(&before, &plain) <= 3, "whole picture changed by {}", max_diff(&before, &plain));
    // After it, black gaps open between the shards (over black fill).
    let fx = r#"{"kind":"shatter","params":{"start":0,"spread":20,"depth":20,"spin":60,"jag":0},"options":{"fill":"black"}}"#;
    let after = render(r, &picture(64, 64, &pic, fx), 10);
    assert!(dark(&after, 8) > 60, "only {} black pixels after the break", dark(&after, 8));
    assert!(dark(&plain, 8) < 5);
    // Transparent fill: the gaps show the background (black here too), and the
    // same frame renders the same way twice.
    let again = render(r, &picture(64, 64, &pic, fx), 10);
    assert!(after == again, "shatter is not a pure function of the frame");
}

#[test]
fn shatter_draws_cracks_along_explicit_angles() {
    gpu!(r);
    // Four sectors split by the axes through the centre: cracks along the
    // middle row and column, nothing elsewhere.
    let fx = r#"{"kind":"shatter","params":{"start":10,"angles":[0,90,180,270],"jag":0,"cracks":1,"crack_width":3,"crack_color":[0,0,0]}}"#;
    let img = render(r, &grey(1.0, fx), 0);
    assert!(px(&img, 32, 50)[0] < 40 && px(&img, 50, 32)[0] < 40, "{:?} {:?}", px(&img, 32, 50), px(&img, 50, 32));
    assert!(px(&img, 16, 16)[0] > 240 && px(&img, 48, 48)[0] > 240);
    // `reach` stops them short of the edge.
    let fx = r#"{"kind":"shatter","params":{"start":10,"angles":[0,90,180,270],"jag":0,"cracks":1,"crack_width":3,"crack_color":[0,0,0],"reach":20}}"#;
    let img = render(r, &grey(1.0, fx), 0);
    assert!(px(&img, 32, 36)[0] < 40 && px(&img, 32, 60)[0] > 240, "{:?} {:?}", px(&img, 32, 36), px(&img, 32, 60));
}

#[test]
fn shatter_moves_an_explicit_shard_by_its_motion() {
    gpu!(r);
    // Two halves (left of the vertical line through the centre, and right);
    // the right one (angles -90..90, listed second) moves right by 25% of the
    // height per second; at t = 1 its left edge is 16 pixels right of the centre.
    let fx = r#"{"kind":"shatter","params":{"start":0,"angles":[90,270],"jag":0,"shade":0,"motion":[0,0,0,0,0, 0,0,25,0,0]},"options":{"fill":"black"}}"#;
    let img = render(r, &grey(1.0, fx), 10);
    assert!(px(&img, 20, 32)[0] > 240, "the left half stays: {:?}", px(&img, 20, 32));
    assert!(px(&img, 40, 32)[0] < 10, "the gap is black: {:?}", px(&img, 40, 32));
    assert!(px(&img, 52, 32)[0] > 240, "the right half moved: {:?}", px(&img, 52, 32));
}

#[test]
fn pinch_pulls_toward_the_centre() {
    gpu!(r);
    let pic = split("kit-split.png", 40);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    assert!(px(&plain, 36, 32)[0] < 10);
    // Pinching reads farther out: the white edge at x = 40 comes in toward the centre.
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"pinch","params":{"amount":1,"radius":40}}"#), 0);
    assert!(px(&img, 36, 32)[0] > 240, "{:?}", px(&img, 36, 32));
    // Far from the centre nothing moves.
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"pinch","params":{"amount":1,"radius":5}}"#), 0);
    assert!(max_diff(&img, &plain) <= 1 || px(&img, 10, 10) == px(&plain, 10, 10));
    assert!(render(r, &picture(64, 64, &pic, r#"{"kind":"pinch","params":{"amount":0}}"#), 0) == plain);
}

#[test]
fn glitch_bands_hold_their_pattern_for_hold_frames() {
    gpu!(r);
    let pic = gradient("kit-gradient.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    let fx = r#"{"kind":"glitch_bands","params":{"amount":1,"bands":8,"height":15,"shift":20,"blocks":4,"block_size":30,"hold":3,"seed":2}}"#;
    let p = picture(64, 64, &pic, fx);
    let (f0, f1, f3) = (render(r, &p, 0), render(r, &p, 1), render(r, &p, 3));
    assert!(f0 != plain, "glitch_bands changed nothing");
    assert!(f0 == f1, "the pattern changed inside a hold");
    assert!(f0 != f3, "the pattern did not change after the hold");
    assert!(render(r, &picture(64, 64, &pic, r#"{"kind":"glitch_bands","params":{"amount":0}}"#), 0) == plain);
}

#[test]
fn film_scratches_and_grain_highlights_leave_black_clean() {
    gpu!(r);
    for fx in [
        r#"{"kind":"film_scratches","params":{"scratches":1,"count":8,"width":20,"dust":64,"dust_size":40}}"#,
        r#"{"kind":"grain_highlights","params":{"amount":0.2}}"#,
    ] {
        let black = render(r, &grey(0.0, fx), 0);
        assert!(black.pixels().all(|p| p.0[0] == 0 && p.0[1] == 0 && p.0[2] == 0), "{fx} marked black");
        let light = render(r, &grey(0.6, fx), 0);
        let plain = render(r, &grey(0.6, ""), 0);
        assert!(light != plain, "{fx} did nothing on grey");
        // New on the next frame.
        assert!(render(r, &grey(0.6, fx), 1) != light, "{fx} did not change between frames");
    }
    // Scratches lighten, dust darkens.
    let s = render(r, &grey(0.6, r#"{"kind":"film_scratches","params":{"scratches":1,"count":8,"width":20,"length":100,"dust":0}}"#), 0);
    assert!(s.pixels().any(|p| p.0[0] > 160) && s.pixels().all(|p| p.0[0] >= 152), "scratches only lighten");
    let d = render(r, &grey(0.6, r#"{"kind":"film_scratches","params":{"scratches":0,"dust":64,"dust_size":30}}"#), 0);
    assert!(d.pixels().any(|p| p.0[0] < 140) && d.pixels().all(|p| p.0[0] <= 154), "dust only darkens");
}

#[test]
fn drip_runs_paint_below_the_edge() {
    gpu!(r);
    // A white bar across the top half on transparent: drips hang below it.
    let p = |effects: &str| {
        project(
            "",
            &format!(
                r#""layers":[{{"id":"bg","kind":"solid","color":[0,0,0,1]}},{{"id":"bar","kind":"solid","color":[1,1,1,1],"transform":{{"scale":[1,0.25],"position":[0,-0.2,0]}},"effects":[{effects}]}}]"#
            ),
        )
    };
    let plain = render(r, &p(""), 0);
    let below = |img: &RgbaImage| (0..64).flat_map(|x| (44..60).map(move |y| (x, y))).filter(|&(x, y)| img.get_pixel(x, y).0[0] > 128).count();
    assert_eq!(below(&plain), 0);
    let img = render(r, &p(r#"{"kind":"drip","params":{"roughness":0,"drips":1,"length":40,"width":4}}"#), 0);
    assert!(below(&img) > 10, "no drips below the bar: {}", below(&img));
    // grow 0: no drips.
    let img = render(r, &p(r#"{"kind":"drip","params":{"roughness":0,"drips":1,"length":40,"width":4,"grow":0}}"#), 0);
    assert!(img == plain);
    // Roughness moves the edge.
    let img = render(r, &p(r#"{"kind":"drip","params":{"roughness":3,"scale":5,"drips":0}}"#), 0);
    assert!(img != plain);
}
