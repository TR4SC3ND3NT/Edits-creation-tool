//! Shape layers, measured by the light they leave on a black frame: white
//! shapes, so a pixel's linear value is its coverage and the sum over the
//! frame is the area in pixels. Skipped without an adapter.

mod common;

use common::*;
use edits_core::Project;
use image::RgbaImage;

const W: u32 = 320;
const H: u32 = 180;

/// A 320x180, 10 fps, 4 second project with one shape layer `id` holding `shapes`.
fn shape_project(id: &str, shapes: &str, extra: &str) -> Project {
    let text = format!(
        r#"{{"format":"edits.project/1","settings":{{"width":{W},"height":{H},"fps":10,"duration":4}},
            "layers":[{{"id":"{id}","kind":"shape","shapes":[{shapes}]{extra}}}]}}"#
    );
    Project::from_json(&text).unwrap_or_else(|e| panic!("bad test project: {e}\n{text}"))
}

fn draw(p: &Project, frame: u64, scale: f32) -> Option<RgbaImage> {
    let mut guard = renderer()?;
    let r = guard.as_mut().unwrap();
    Some(render_in(r, p, &dir(), frame, scale))
}

fn cov(img: &RgbaImage, x: u32, y: u32) -> f64 {
    to_linear(img.get_pixel(x, y).0[1] as f64 / 255.0)
}

/// Covered pixels, summed over the frame.
fn area(img: &RgbaImage) -> f64 {
    (0..img.height()).flat_map(|y| (0..img.width()).map(move |x| (x, y))).map(|(x, y)| cov(img, x, y)).sum()
}

#[test]
fn ellipse_and_rect_fill_their_analytic_areas() {
    let h = H as f64;
    let Some(img) = draw(&shape_project("e", r#"{"type":"ellipse","size":0.5,"fill":{}}"#, ""), 0, 1.0) else { return };
    let want = std::f64::consts::PI * (0.25 * h).powi(2);
    assert!((area(&img) - want).abs() < want * 0.005, "ellipse {} vs {want}", area(&img));
    let Some(img) = draw(&shape_project("r", r#"{"type":"rect","size":[0.6,0.3],"fill":{},"position":[0.1,0.05]}"#, ""), 0, 1.0) else { return };
    let want = 0.6 * h * 0.3 * h;
    assert!((area(&img) - want).abs() < want * 0.005, "rect {} vs {want}", area(&img));
    // The rect is where it was put: its centre 0.1 right and 0.05 down.
    assert!(cov(&img, (W as f64 / 2.0 + 0.1 * h + 0.25 * h) as u32, (h / 2.0 + 0.05 * h + 0.1 * h) as u32) > 0.99);
    assert!(cov(&img, (W as f64 / 2.0 - 0.1 * h - 0.25 * h) as u32, (h / 2.0) as u32) < 0.01);
    // Half the opacity, half the light.
    let Some(half) = draw(&shape_project("r", r#"{"type":"rect","size":[0.6,0.3],"fill":{"opacity":0.5}}"#, ""), 0, 1.0) else { return };
    assert!((area(&half) / want - 0.5).abs() < 0.01, "{}", area(&half) / want);
}

#[test]
fn stroke_width_is_percent_of_the_height_at_any_scale() {
    let p = shape_project("s", r#"{"type":"line","from":[-0.5,0],"to":[0.5,0],"stroke":{"width":5}}"#, "");
    for scale in [1.0f32, 0.5, 2.0] {
        let Some(img) = draw(&p, 0, scale) else { return };
        let x = img.width() / 2;
        let px: f64 = (0..img.height()).map(|y| cov(&img, x, y)).sum();
        let want = 0.05 * img.height() as f64;
        assert!((px - want).abs() <= 1.0, "scale {scale}: {px} px vs {want}");
        // And the line is one unit long (with butt caps).
        let y = img.height() / 2;
        let len: f64 = (0..img.width()).map(|x| cov(&img, x, y)).sum();
        assert!((len - img.height() as f64).abs() <= 1.0, "scale {scale}: length {len}");
    }
}

#[test]
fn trim_to_half_halves_the_stroke() {
    let line = r#""type":"path","points":[[-0.6,0.2],[0,-0.3],[0.6,0.2]],"stroke":{"width":3}"#;
    let Some(full) = draw(&shape_project("t", &format!("{{{line}}}"), ""), 0, 1.0) else { return };
    let Some(half) = draw(&shape_project("t", &format!(r#"{{{line},"operators":[{{"kind":"trim","start":0,"end":50}}]}}"#), ""), 0, 1.0) else { return };
    let ratio = area(&half) / area(&full);
    assert!((ratio - 0.5).abs() < 0.025, "{ratio}");
    // Keyed from 0 to 100 over 2 s: half way at 1 s (frame 10), nothing at 0.
    let keyed = format!(r#"{{{line},"operators":[{{"kind":"trim","end":{{"keys":[{{"t":0,"v":0}},{{"t":2,"v":100}}]}}}}]}}"#);
    let p = shape_project("t", &keyed, "");
    let Some(mid) = draw(&p, 10, 1.0) else { return };
    assert!((area(&mid) / area(&full) - 0.5).abs() < 0.025);
    let Some(start) = draw(&p, 0, 1.0) else { return };
    assert!(area(&start) < 1.0);
    // On a closed ellipse, the first half (offset a quarter turn) is a half ring.
    let ring = r#""type":"ellipse","size":0.6,"stroke":{"width":2}"#;
    let Some(all) = draw(&shape_project("t", &format!("{{{ring}}}"), ""), 0, 1.0) else { return };
    let Some(part) = draw(&shape_project("t", &format!(r#"{{{ring},"operators":[{{"kind":"trim","end":50,"offset":25}}]}}"#), ""), 0, 1.0) else { return };
    assert!((area(&part) / area(&all) - 0.5).abs() < 0.025);
    // From the right (3 o'clock) round the bottom: the left and right of the bottom row are lit, the top is not.
    assert!(cov(&part, W / 2, H / 2 + (0.3 * H as f64) as u32) > 0.5);
    assert!(cov(&part, W / 2, H / 2 - (0.3 * H as f64) as u32) < 0.01);
}

/// The runs of covered pixels along row `y`.
fn runs(img: &RgbaImage, y: u32) -> usize {
    let mut n = 0;
    let mut inside = false;
    for x in 0..img.width() {
        let on = cov(img, x, y) > 0.5;
        if on && !inside {
            n += 1;
        }
        inside = on;
    }
    n
}

#[test]
fn a_repeater_makes_its_copies() {
    for copies in [1, 3, 5, 7] {
        let shapes = format!(r#"{{"type":"rect","size":0.08,"fill":{{}},"operators":[{{"kind":"repeater","copies":{copies},"offset":{},"position":[0.2,0]}}]}}"#, -(copies as f64 - 1.0) / 2.0);
        let Some(img) = draw(&shape_project("rep", &shapes, ""), 0, 1.0) else { return };
        assert_eq!(runs(&img, H / 2), copies, "copies {copies}");
        let want = copies as f64 * (0.08 * H as f64).powi(2);
        assert!((area(&img) - want).abs() < want * 0.01, "{} vs {want}", area(&img));
    }
    // Turned copies around the centre: twelve spokes cross a ring of radius 0.35.
    let spokes = r#"{"type":"line","from":[0.1,0],"to":[0.45,0],"stroke":{"width":2},"operators":[{"kind":"repeater","copies":12,"rotation":30}]}"#;
    let Some(img) = draw(&shape_project("rep", spokes, ""), 0, 1.0) else { return };
    let mut crossings = 0;
    let mut inside = false;
    for i in 0..720 {
        let a = i as f64 / 720.0 * std::f64::consts::TAU;
        let (x, y) = (W as f64 / 2.0 + 0.35 * H as f64 * a.cos(), H as f64 / 2.0 + 0.35 * H as f64 * a.sin());
        let on = cov(&img, x as u32, y as u32) > 0.5;
        if on && !inside {
            crossings += 1;
        }
        inside = on;
    }
    assert!((11..=13).contains(&crossings), "{crossings}");
}

#[test]
fn round_corners_keeps_the_area_of_a_rounded_square() {
    let h = H as f64;
    let Some(img) = draw(&shape_project("rc", r#"{"type":"rect","size":0.5,"fill":{},"operators":[{"kind":"round_corners","radius":0.1}]}"#, ""), 0, 1.0) else { return };
    let (s, r) = (0.5 * h, 0.1 * h);
    let want = s * s - (4.0 - std::f64::consts::PI) * r * r;
    assert!((area(&img) - want).abs() < want * 0.004, "{} vs {want}", area(&img));
    // The same as the rect's own roundness.
    let Some(own) = draw(&shape_project("rc", r#"{"type":"rect","size":0.5,"roundness":0.1,"fill":{}}"#, ""), 0, 1.0) else { return };
    assert!((area(&own) - area(&img)).abs() < 2.0);
}

#[test]
fn still_shapes_rasterise_once() {
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let still = shape_project("still_once", r#"{"type":"polystar","points":5,"outer_radius":0.3,"inner_radius":0.12,"fill":{"color":[1,0.8,0]},"stroke":{"width":1,"dash":[2,1]},"operators":[{"kind":"zigzag","size":0.01,"ridges":3}]}"#, "");
    let before = r.shape_rasterisations();
    let first = render_in(r, &still, &dir(), 0, 1.0);
    for f in 1..6 {
        assert_eq!(render_in(r, &still, &dir(), f, 1.0), first);
    }
    assert_eq!(r.shape_rasterisations() - before, 1);
    // Keyed, it is drawn again at every frame that differs.
    let keyed = shape_project("keyed_each", r#"{"type":"ellipse","size":{"keys":[{"t":0,"v":[0.1,0.1]},{"t":4,"v":[0.9,0.9]}]},"fill":{}}"#, "");
    let before = r.shape_rasterisations();
    for f in 0..4 {
        render_in(r, &keyed, &dir(), f, 1.0);
    }
    assert_eq!(r.shape_rasterisations() - before, 4);
    // A moving layer moves the picture, without a new rasterisation of a
    // still shape only when the matrix is the same.
    let moving = shape_project("moving_layer", r#"{"type":"rect","size":0.2,"fill":{}}"#, r#","transform":{"position":{"keys":[{"t":0,"v":[0,0,0]},{"t":4,"v":[0.4,0,0]}]}}"#);
    let a = render_in(r, &moving, &dir(), 0, 1.0);
    let b = render_in(r, &moving, &dir(), 20, 1.0);
    assert_ne!(a, b);
}

#[test]
fn frames_are_pure_functions_of_the_frame_number() {
    let shapes = r#"{"type":"line","from":[0.05,0],"to":[0.5,0],"stroke":{"width":0.6,"cap":"round"},
        "operators":[{"kind":"repeater","copies":40,"rotation":9,"jitter":{"rotation":9,"scale":0.4,"opacity":0.5,"every":0,"seed":3}},{"kind":"wiggle_path","size":0.01,"detail":4,"speed":3,"seed":2}]},
        {"type":"ellipse","size":0.4,"stroke":{"width":1},"operators":[{"kind":"wiggle_path","size":0.02,"speed":2}]}"#;
    let p = shape_project("pure", shapes, "");
    let Some(a) = draw(&p, 3, 1.0) else { return };
    let Some(b) = draw(&p, 7, 1.0) else { return };
    let Some(c) = draw(&p, 3, 1.0) else { return };
    assert_eq!(a, c);
    assert_ne!(a, b, "the jitter and the wiggle change from frame to frame");
    // A fresh renderer draws the same bytes.
    if let Ok(mut fresh) = edits_render::Renderer::new(Default::default()) {
        assert_eq!(render_in(&mut fresh, &p, &dir(), 3, 1.0), a);
    }
}

#[test]
fn a_shape_layer_turns_in_perspective_and_takes_effects() {
    let star = r#"{"type":"polystar","points":6,"outer_radius":0.35,"inner_radius":0.2,"fill":{}}"#;
    let Some(flat) = draw(&shape_project("persp", star, ""), 0, 1.0) else { return };
    let Some(turned) = draw(&shape_project("persp", star, r#","transform":{"rotation":[0,60,0]}"#), 0, 1.0) else { return };
    let ratio = area(&turned) / area(&flat);
    // Seen at 60 degrees, about half as wide (perspective makes it a little more or less).
    assert!(ratio > 0.4 && ratio < 0.62, "{ratio}");
    // A blur spreads the edges but keeps the light.
    let Some(blurred) = draw(&shape_project("persp", star, r#","effects":[{"kind":"gaussian_blur","params":{"radius":2}}]"#), 0, 1.0) else { return };
    assert!((area(&blurred) / area(&flat) - 1.0).abs() < 0.03);
    assert_ne!(blurred, flat);
}
