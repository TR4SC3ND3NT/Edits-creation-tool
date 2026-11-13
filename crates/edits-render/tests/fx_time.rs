//! Pixel tests of the time effects: echo and time displacement. Every test
//! skips when the machine has no adapter.

mod common;
mod fxcommon;
use common::*;
use edits_core::Project;
use fxcommon::max_diff;
use image::{Rgba, RgbaImage};

macro_rules! gpu {
    ($r:ident) => {
        let Some(mut guard) = renderer() else { return };
        let $r = guard.as_mut().unwrap();
    };
}

/// A square of `size` (fraction of the 64 x 64 frame) and sRGB grey `grey`
/// moving right at half a frame height per second: x = 0 at t = 1 s (frame
/// 10), so 8 pixels every 0.25 s. `effects` are on the square.
fn square(size: f64, grey: f64, effects: &str) -> String {
    format!(
        r#"{{"id":"sq","kind":"solid","color":[{grey},{grey},{grey},1],
            "transform":{{"scale":[{size},{size}],"position":{{"keys":[{{"t":0,"v":[-0.5,0,0]}},{{"t":2,"v":[0.5,0,0]}}]}}}},
            "effects":[{effects}]}}"#
    )
}

fn layers(layers: &str) -> Project {
    project("", &format!(r#""layers":[{layers}]"#))
}

fn lin(img: &RgbaImage, x: u32, y: u32) -> f64 {
    to_linear(img.get_pixel(x, y).0[0] as f64 / 255.0)
}

#[track_caller]
fn assert_lin(img: &RgbaImage, x: u32, want: f64) {
    let got = lin(img, x, 32);
    assert!((got - want).abs() < 0.015, "pixel ({x}, 32) is {got:.4} linear, expected {want:.4}");
}

#[test]
fn echo_add_shows_three_squares_at_1_d_d2() {
    gpu!(r);
    let d = 0.5;
    let p = layers(&square(0.0625, 1.0, &format!(r#"{{"kind":"echo","params":{{"count":2,"interval":-0.25,"decay":{d}}}}}"#)));
    let img = render(r, &p, 10);
    // The square (4 pixels wide) is centred on x = 32 now, 24 a quarter second ago, 16 half a second ago.
    assert_lin(&img, 31, 1.0);
    assert_lin(&img, 32, 1.0);
    assert_lin(&img, 23, d);
    assert_lin(&img, 24, d);
    assert_lin(&img, 15, d * d);
    assert_lin(&img, 16, d * d);
    // Nothing between them, nor where a third echo would be.
    for x in [8, 20, 28, 40] {
        assert_lin(&img, x, 0.0);
    }
    // `start` weighs the current picture as well.
    let p = layers(&square(0.0625, 1.0, r#"{"kind":"echo","params":{"count":1,"interval":-0.25,"decay":0.5,"start":0.8}}"#));
    let img = render(r, &p, 10);
    assert_lin(&img, 32, 0.8);
    assert_lin(&img, 24, 0.4);
}

#[test]
fn echo_modes_combine_overlapping_echoes() {
    gpu!(r);
    // A 12-pixel square with echoes 8 pixels apart: x 26..38 now, 18..30 before; 26..30 overlap.
    let c = to_linear(0.66);
    let with = |mode: &str| layers(&square(0.1875, 0.66, &format!(r#"{{"kind":"echo","params":{{"count":1,"interval":-0.25,"decay":0.5}},"options":{{"mode":"{mode}"}}}}"#)));
    let add = render(r, &with("add"), 10);
    assert_lin(&add, 28, 1.5 * c);
    assert_lin(&add, 22, 0.5 * c);
    assert_lin(&add, 34, c);
    let max = render(r, &with("max"), 10);
    assert_lin(&max, 28, c);
    assert_lin(&max, 22, 0.5 * c);
    let min = render(r, &with("min"), 10);
    assert_lin(&min, 28, 0.5 * c);
    assert_lin(&min, 22, 0.0);
    assert_lin(&min, 34, 0.0);
    let screen = render(r, &with("screen"), 10);
    assert_lin(&screen, 28, c + 0.5 * c - 0.5 * c * c);
    let average = render(r, &with("average"), 10);
    assert_lin(&average, 28, c);
    assert_lin(&average, 34, c / 1.5);
    assert_lin(&average, 22, 0.5 * c / 1.5);
    // over_back keeps the current picture in front; the half-transparent
    // echo behind it shows where it is alone.
    let back = render(r, &with("over_back"), 10);
    assert_lin(&back, 28, c);
    assert_lin(&back, 22, 0.5 * c);
    let front = render(r, &with("over_front"), 10);
    assert_lin(&front, 28, 0.5 * c + c * 0.5);
}

#[test]
fn echo_over_back_and_front_order_the_echoes() {
    gpu!(r);
    // A red square now and, a quarter second earlier, the same square at full
    // strength (decay 1) but turned green by a hue shift that is keyed in time.
    let fx = |mode: &str| {
        format!(
            r#"{{"kind":"hue","params":{{"degrees":{{"keys":[{{"t":0.75,"v":120,"ease":"hold"}},{{"t":1,"v":0}}]}}}}}},
               {{"kind":"echo","params":{{"count":1,"interval":-0.25,"decay":1}},"options":{{"mode":"{mode}"}}}}"#
        )
    };
    let red_square = |mode: &str| layers(&square(0.1875, 1.0, &fx(mode)).replace(r#""color":[1.0,1.0,1.0,1]"#, r#""color":[1,0,0,1]"#).replace(r#""color":[1,1,1,1]"#, r#""color":[1,0,0,1]"#));
    let back = render(r, &red_square("over_back"), 10);
    let front = render(r, &red_square("over_front"), 10);
    // In the overlap (x 26..30) the current (red) picture is in front with
    // over_back, the earlier (green) one with over_front.
    let (b, f) = (back.get_pixel(28, 32).0, front.get_pixel(28, 32).0);
    assert!(b[0] > 200 && b[1] < 40, "over_back: {b:?}");
    assert!(f[1] > 200 && f[0] < 40, "over_front: {f:?}");
    // Where only the earlier one is, both show it.
    assert!(back.get_pixel(21, 32).0[1] > 200 && front.get_pixel(21, 32).0[1] > 200);
}

#[test]
fn echo_on_an_adjustment_layer_rerenders_the_layers_beneath() {
    gpu!(r);
    let echo = r#"{"kind":"echo","params":{"count":2,"interval":-0.25,"decay":0.5}}"#;
    let on_layer = render(r, &layers(&square(0.0625, 1.0, echo)), 10);
    let adjusted = render(r, &layers(&format!(r#"{},{{"id":"fx","kind":"adjustment","effects":[{echo}]}}"#, square(0.0625, 1.0, ""))), 10);
    assert!(max_diff(&on_layer, &adjusted) <= 1, "an adjustment echo over black differs from the layer's own: {}", max_diff(&on_layer, &adjusted));
    // A layer above the adjustment layer is not echoed.
    let above = render(
        r,
        &layers(&format!(r#"{{"id":"fx","kind":"adjustment","effects":[{echo}]}},{}"#, square(0.0625, 1.0, ""))),
        10,
    );
    assert_lin(&above, 32, 1.0);
    assert_lin(&above, 24, 0.0);
}

#[test]
fn echo_inside_a_comp_and_under_another_echo() {
    gpu!(r);
    let echo = r#"{"kind":"echo","params":{"count":2,"interval":-0.25,"decay":0.5}}"#;
    let inline = render(r, &layers(&square(0.0625, 1.0, echo)), 10);
    let comp = project("", &format!(r#""comps":{{"c":{{"duration":4,"layers":[{}]}}}},"layers":[{{"id":"k","kind":"comp","comp":"c"}}]"#, square(0.0625, 1.0, echo)));
    let nested = render(r, &comp, 10);
    assert!(max_diff(&inline, &nested) <= 1, "an echo in a comp differs from inline: {}", max_diff(&inline, &nested));
    // An adjustment echo over that comp echoes the echoes: weights 1, 1, 0.75 and 0.25 at 0 to 3 steps back.
    let twice = project(
        "",
        &format!(
            r#""comps":{{"c":{{"duration":4,"layers":[{}]}}}},"layers":[{{"id":"k","kind":"comp","comp":"c"}},{{"id":"fx","kind":"adjustment","effects":[{echo}]}}]"#,
            square(0.0625, 1.0, echo)
        ),
    );
    let img = render(r, &twice, 10);
    // x = 24 holds echo 1 of the current picture and echo 0 of the earlier one: 0.5 + 0.5.
    assert_lin(&img, 32, 1.0);
    assert!(lin(&img, 24, 32) > 0.95);
    assert_lin(&img, 16, 0.75);
    assert_lin(&img, 8, 0.25);
}

#[test]
fn echo_count_zero_costs_nothing() {
    gpu!(r);
    let plain = layers(&square(0.0625, 1.0, ""));
    let a = render(r, &plain, 10);
    let plain_passes = r.stats().passes;
    for fx in [r#"{"kind":"echo","params":{"count":0}}"#, r#"{"kind":"echo","params":{"decay":0}}"#, r#"{"kind":"time_displacement","params":{"max":0}}"#] {
        let b = render(r, &layers(&square(0.0625, 1.0, fx)), 10);
        assert!(a == b, "{fx} changed the picture");
        assert_eq!(r.stats().passes, plain_passes, "{fx} added passes");
        let adjusted = layers(&format!(r#"{},{{"id":"fx","kind":"adjustment","effects":[{fx}]}}"#, square(0.0625, 1.0, "")));
        let c = render(r, &adjusted, 10);
        assert!(a == c, "{fx} on an adjustment layer changed the picture");
    }
    // An interval of 0 needs no re-render either: each echo is the picture itself.
    let b = render(r, &layers(&square(0.0625, 1.0, r#"{"kind":"echo","params":{"count":2,"interval":0,"decay":0.5}}"#)), 10);
    assert_lin(&b, 32, 1.0);
}

/// 64 x 64 horizontal ramp: black at the left, white at the right.
fn ramp() -> String {
    save("time-ramp.png", &RgbaImage::from_fn(64, 64, |x, _| {
        let v = (x * 255 / 63) as u8;
        Rgba([v, v, v, 255])
    }))
}

/// A white full-frame solid faded to black over 4 seconds (linear amount
/// t / 4: linear value 1 - t / 4), then `effects`.
fn fading(effects: &str) -> String {
    format!(
        r#"{{"id":"s","kind":"solid","color":[1,1,1,1],"effects":[{{"kind":"fade","params":{{"amount":{{"keys":[{{"t":0,"v":0}},{{"t":4,"v":1}}]}},"color":[0,0,0]}}}},{effects}]}}"#
    )
}

#[test]
fn time_displacement_shows_other_times_across_a_ramp() {
    gpu!(r);
    let map = ramp();
    let p = project("", &format!(r#""slots":{{"m":{{"path":"{map}"}}}},"layers":[{}]"#, fading(r#"{"kind":"time_displacement","params":{"max":1,"levels":16},"source":"slot:m"}"#)));
    // At t = 2: the left edge (black) shows t = 1, the right edge (white) t = 3.
    let img = render(r, &p, 20);
    assert_lin(&img, 0, 0.75);
    assert_lin(&img, 63, 0.25);
    assert!(lin(&img, 8, 32) > lin(&img, 32, 32) && lin(&img, 32, 32) > lin(&img, 56, 32), "the shift does not grow across the frame");
    // 16 slices: at most 16 distinct values along a row.
    let mut row: Vec<u8> = (0..64).map(|x| img.get_pixel(x, 32).0[0]).collect();
    row.dedup();
    assert!(row.len() <= 16 && row.len() >= 8, "{} distinct values", row.len());
    // neutral black: black shifts nothing, white the whole of `max`.
    let p = project("", &format!(r#""slots":{{"m":{{"path":"{map}"}}}},"layers":[{}]"#, fading(r#"{"kind":"time_displacement","params":{"max":1},"options":{"neutral":"black"},"source":"slot:m"}"#)));
    let img = render(r, &p, 20);
    assert_lin(&img, 0, 0.5);
    assert_lin(&img, 63, 0.25);
    // Without a map the layer's own luminance picks: 0.5 linear is a display
    // value of 0.735, which two levels round up to white.
    let p = layers(&fading(r#"{"kind":"time_displacement","params":{"max":1,"levels":2}}"#));
    let img = render(r, &p, 20);
    assert_lin(&img, 10, 0.25);
    assert_lin(&img, 50, 0.25);
}

#[test]
fn time_effects_are_deterministic() {
    gpu!(r);
    let map = ramp();
    let p = project(
        "",
        &format!(
            r#""slots":{{"m":{{"path":"{map}"}}}},"layers":[{},{},{{"id":"fx","kind":"adjustment","effects":[{{"kind":"echo","params":{{"count":3}},"options":{{"mode":"screen"}}}}]}}]"#,
            fading(r#"{"kind":"time_displacement","params":{"max":0.5,"levels":8},"source":"slot:m"}"#),
            square(0.2, 0.8, r#"{"kind":"echo","params":{"count":4,"interval":-0.1,"decay":0.7},"options":{"mode":"over_back"}}"#)
        ),
    );
    let a = render(r, &p, 15);
    let _ = render(r, &p, 3);
    let b = render(r, &p, 15);
    assert!(a == b, "the same frame rendered twice differs");
}

#[test]
fn time_effects_with_motion_blur() {
    gpu!(r);
    // Motion blur samples of a still layer with an echo are shared; a moving one renders each.
    let still = project(r#","motion_blur":{"samples":4,"shutter":0.5}"#, r#""layers":[{"id":"s","kind":"solid","color":[1,1,1,1],"transform":{"scale":0.2},"effects":[{"kind":"echo"}]}]"#);
    render(r, &still, 10);
    assert_eq!(r.stats().rendered_samples, 1);
    let moving = project(r#","motion_blur":{"samples":4,"shutter":0.5}"#, &format!(r#""layers":[{}]"#, square(0.0625, 1.0, r#"{"kind":"echo"}"#)));
    render(r, &moving, 10);
    assert_eq!(r.stats().rendered_samples, 4);
    // A still layer whose echoes reach back to where it moved: the samples differ.
    let stops = project(
        r#","motion_blur":{"samples":4,"shutter":0.5}"#,
        r#""layers":[{"id":"s","kind":"solid","color":[1,1,1,1],"transform":{"scale":0.1,"position":{"keys":[{"t":0,"v":[-0.5,0,0]},{"t":0.95,"v":[0,0,0]}]}},"effects":[{"kind":"echo","params":{"interval":-0.1}}]}]"#,
    );
    render(r, &stops, 10);
    assert!(r.stats().rendered_samples > 1, "the echoes of a stopped layer still move");
}

#[test]
fn deeply_nested_time_effects_are_refused() {
    gpu!(r);
    let echo = r#"{"kind":"echo","params":{"count":1}}"#;
    // Each echo's input holds the echoes before it: four levels render, five do not.
    let chain = |n: usize| layers(&square(0.0625, 1.0, &vec![echo; n].join(",")));
    render(r, &chain(4), 10);
    r.prepare(&chain(5), &dir()).unwrap();
    let err = r.render_frame(10, 1.0).unwrap_err();
    assert!(format!("{err:#}").contains("nested more than 4 deep"), "{err:#}");
    // The renderer is fine afterwards.
    render(r, &chain(1), 10);
}
