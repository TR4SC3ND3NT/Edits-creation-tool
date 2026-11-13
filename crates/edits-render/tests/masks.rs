//! Masks and track mattes, measured on white layers over black: the linear
//! value of a pixel is then the coverage there. Skipped without an adapter.

mod common;

use common::*;
use edits_core::Project;
use image::RgbaImage;

const S: u32 = 128;

/// A 128x128 project over black with `layers` (and `slots`).
fn masked(slots: &str, layers: &str) -> Project {
    let text = format!(r#"{{"format":"edits.project/1","settings":{{"width":{S},"height":{S},"fps":10,"duration":2}},"slots":{{{slots}}},"layers":[{layers}]}}"#);
    Project::from_json(&text).unwrap_or_else(|e| panic!("bad test project: {e}\n{text}"))
}

fn draw(p: &Project) -> Option<RgbaImage> {
    let mut guard = renderer()?;
    let r = guard.as_mut().unwrap();
    Some(render(r, p, 0))
}

/// A white solid with `extra` fields (masks, matte, transform).
fn white(id: &str, extra: &str) -> String {
    format!(r#"{{"id":"{id}","kind":"solid","color":[1,1,1,1]{extra}}}"#)
}

/// Linear coverage of a pixel of a white layer over black (red channel).
fn cov(img: &RgbaImage, x: u32, y: u32) -> f64 {
    to_linear(img.get_pixel(x, y).0[0] as f64 / 255.0)
}

/// Sum of coverage over the picture, in pixels.
fn area(img: &RgbaImage) -> f64 {
    (0..S).flat_map(|y| (0..S).map(move |x| (x, y))).map(|(x, y)| cov(img, x, y)).sum()
}

#[track_caller]
fn assert_cov(img: &RgbaImage, x: u32, y: u32, want: f64, tolerance: f64) {
    let got = cov(img, x, y);
    assert!((got - want).abs() <= tolerance, "pixel ({x}, {y}) covers {got:.4}, expected {want} +-{tolerance}");
}

#[test]
fn a_rect_mask_hides_the_outside() {
    let p = masked("", &white("w", r#","masks":[{"shape":"rect","size":[0.5,0.25]}]"#));
    let Some(img) = draw(&p) else { return };
    // x from 32 to 96, y from 48 to 80, exactly on pixel edges.
    assert_cov(&img, 64, 64, 1.0, 0.0);
    assert_cov(&img, 32, 48, 1.0, 0.0);
    assert_cov(&img, 95, 79, 1.0, 0.0);
    for (x, y) in [(31, 64), (96, 64), (64, 47), (64, 80), (5, 5), (120, 120)] {
        assert_cov(&img, x, y, 0.0, 0.0);
    }
    assert!((area(&img) - 64.0 * 32.0).abs() < 1.0, "{}", area(&img));
    // The mask moves with the layer: a quarter of the frame to the right.
    let moved = masked("", &white("w", r#","transform":{"position":[0.25,0,0]},"masks":[{"shape":"rect","size":[0.5,0.25]}]"#));
    let Some(img) = draw(&moved) else { return };
    assert_cov(&img, 64, 64, 1.0, 0.0);
    assert_cov(&img, 63, 64, 0.0, 0.0);
    assert_cov(&img, 127, 64, 1.0, 0.0);
}

#[test]
fn feather_is_a_monotonic_ramp_of_its_width() {
    // 10 % of 128 pixels: 12.8 pixels, centred on the edge at x = 32.
    let p = masked("", &white("w", r#","masks":[{"shape":"rect","size":[0.5,0.5],"feather":10}]"#));
    let Some(img) = draw(&p) else { return };
    let row: Vec<f64> = (0..S / 2).map(|x| cov(&img, x, 64)).collect();
    assert!(row.windows(2).all(|w| w[1] >= w[0] - 1e-9), "not monotonic: {row:?}");
    let soft = row.iter().filter(|c| **c > 0.0 && **c < 1.0).count() as f64;
    assert!((soft - 12.8).abs() <= 1.0, "{soft} soft pixels: {row:?}");
    // Half way at the outline.
    assert!((row[31] + row[32] - 1.0).abs() < 0.05, "{} {}", row[31], row[32]);
    // The feather keeps the area of a straight-edged shape (but for its corners).
    let ideal = 64.0 * 64.0;
    assert!((area(&img) - ideal).abs() / ideal < 0.02, "{} vs {ideal}", area(&img));
}

#[test]
fn subtract_intersect_difference_and_invert() {
    let big = r#"{"shape":"rect","size":[0.75,0.75]}"#;
    let sub = masked("", &white("w", &format!(r#","masks":[{big},{{"shape":"rect","size":[0.25,0.25],"mode":"subtract"}}]"#)));
    let Some(img) = draw(&sub) else { return };
    assert_cov(&img, 64, 64, 0.0, 0.0);
    assert_cov(&img, 30, 64, 1.0, 0.0);
    assert_cov(&img, 5, 64, 0.0, 0.0);
    // A first mask that subtracts starts from the whole layer.
    let hole = masked("", &white("w", r#","masks":[{"shape":"ellipse","size":0.25,"mode":"subtract"}]"#));
    let Some(img) = draw(&hole) else { return };
    assert_cov(&img, 64, 64, 0.0, 0.0);
    assert_cov(&img, 5, 5, 1.0, 0.0);
    // Two overlapping rects: x in -0.375..0.125 and -0.125..0.375.
    let pair = |mode: &str| masked("", &white("w", &format!(r#","masks":[{{"shape":"rect","size":0.5,"position":[-0.125,0]}},{{"shape":"rect","size":0.5,"position":[0.125,0],"mode":"{mode}"}}]"#)));
    let Some(img) = draw(&pair("intersect")) else { return };
    assert_cov(&img, 64, 64, 1.0, 0.0);
    assert_cov(&img, 40, 64, 0.0, 0.0);
    assert_cov(&img, 88, 64, 0.0, 0.0);
    assert!((area(&img) - 32.0 * 64.0).abs() < 1.0, "{}", area(&img));
    let Some(img) = draw(&pair("difference")) else { return };
    assert_cov(&img, 64, 64, 0.0, 0.0);
    assert_cov(&img, 40, 64, 1.0, 0.0);
    assert_cov(&img, 88, 64, 1.0, 0.0);
    let Some(img) = draw(&pair("add")) else { return };
    assert!((area(&img) - 96.0 * 64.0).abs() < 1.0, "{}", area(&img));
    // Inverted: the outside of the shape; opacity scales the inverted mask.
    let inv = masked("", &white("w", r#","masks":[{"shape":"rect","size":0.5,"inverted":true,"opacity":0.5}]"#));
    let Some(img) = draw(&inv) else { return };
    assert_cov(&img, 64, 64, 0.0, 0.0);
    assert_cov(&img, 5, 5, 0.5, 0.01);
}

#[test]
fn ellipse_rounded_rect_and_polygon_areas() {
    let cases = [
        // A circle 0.5 across: pi * 32^2 pixels.
        (r#"{"shape":"ellipse","size":0.5}"#, std::f64::consts::PI * 32.0 * 32.0),
        (r#"{"shape":"ellipse","size":[0.75,0.25],"rotation":30}"#, std::f64::consts::PI * 48.0 * 16.0),
        // A square 0.5 across with corners of radius 0.125: 64^2 - (4 - pi) 16^2.
        (r#"{"shape":"rounded_rect","size":0.5,"radius":0.125}"#, 64.0 * 64.0 - (4.0 - std::f64::consts::PI) * 16.0 * 16.0),
        // A triangle 0.5 wide and 0.5 high, scaled by size: 32 x 64 / 2... then grown.
        (r#"{"shape":"polygon","points":[[0,-0.25],[0.25,0.25],[-0.25,0.25]]}"#, 64.0 * 64.0 / 2.0),
        (r#"{"shape":"polygon","points":[[0,-0.5],[0.5,0.5],[-0.5,0.5]],"size":[0.5,0.5],"rotation":45,"position":[0.05,0.1]}"#, 64.0 * 64.0 / 2.0),
        // A star (concave): five outer points on a circle of radius 0.4, inner 0.16.
        (&star(), star_area() * (S * S) as f64),
        // Expansion grows a square by 5 % on each side.
        (r#"{"shape":"rect","size":0.5,"expansion":5}"#, (64.0 + 12.8) * (64.0 + 12.8) - (4.0 - std::f64::consts::PI) * 6.4 * 6.4),
    ];
    for (mask, want) in cases {
        let p = masked("", &white("w", &format!(r#","masks":[{mask}]"#)));
        let Some(img) = draw(&p) else { return };
        let got = area(&img);
        assert!((got - want).abs() / want < 0.01, "{mask}: area {got:.1}, expected {want:.1}");
    }
}

fn star_points() -> Vec<[f64; 2]> {
    (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { 0.4 } else { 0.16 };
            let a = (i as f64 * 36.0 - 90.0).to_radians();
            [r * a.cos(), r * a.sin()]
        })
        .collect()
}

fn star() -> String {
    format!(r#"{{"shape":"polygon","points":{}}}"#, serde_json::to_string(&star_points()).unwrap())
}

/// Shoelace formula.
fn star_area() -> f64 {
    let v = star_points();
    (0..v.len()).map(|i| v[i][0] * v[(i + 1) % v.len()][1] - v[(i + 1) % v.len()][0] * v[i][1]).sum::<f64>().abs() / 2.0
}

#[test]
fn mattes_against_hand_computed_values() {
    // The matte source is a hidden solid; the matted layer is white.
    let cases = [
        // Alpha 0.25 (colour does not matter).
        ("[0.2,0.9,0.4,0.25]", "alpha", 0.25),
        ("[0.2,0.9,0.4,0.25]", "alpha_inverted", 0.75),
        // Luma of the display colour: a mid grey is 0.5, pure red 0.2126.
        ("[0.5,0.5,0.5,1]", "luma", 0.5),
        ("[1,0,0,1]", "luma", 0.2126),
        ("[1,0,0,1]", "luma_inverted", 0.7874),
        // Over black: luma times alpha.
        ("[0.5,0.5,0.5,0.5]", "luma", 0.25),
        ("[0.5,0.5,0.5,0.5]", "luma_inverted", 0.75),
    ];
    for (color, mode, want) in cases {
        let layers = format!(r#"{{"id":"m","kind":"solid","color":{color},"visible":false}},{}"#, white("w", &format!(r#","matte":{{"layer":"m","mode":"{mode}"}}"#)));
        let Some(img) = draw(&masked("", &layers)) else { return };
        for (x, y) in [(64, 64), (3, 120)] {
            let got = cov(&img, x, y);
            assert!((got - want).abs() < 0.012, "{color} {mode}: {got:.4}, expected {want}");
        }
    }
    // The source's opacity counts, and so does its shape: outside a smaller
    // source an alpha matte hides the layer and an inverted one shows it.
    let layers = format!(
        r#"{{"id":"m","kind":"solid","color":[1,1,1,1],"transform":{{"scale":0.5,"opacity":0.5}},"visible":false}},{},{}"#,
        white("a", r#","transform":{"position":[0,-0.25,0],"scale":[1,0.5]},"matte":{"layer":"m"}"#),
        white("b", r#","transform":{"position":[0,0.25,0],"scale":[1,0.5]},"matte":{"layer":"m","mode":"alpha_inverted"}"#)
    );
    let Some(img) = draw(&masked("", &layers)) else { return };
    assert_cov(&img, 64, 40, 0.5, 0.01);
    assert_cov(&img, 10, 40, 0.0, 0.0);
    assert_cov(&img, 64, 80, 0.5, 0.01);
    assert_cov(&img, 10, 80, 1.0, 0.0);
    // A source that is not there yet: an alpha matte shows nothing, an inverted one everything.
    let later = |mode: &str| masked("", &format!(r#"{{"id":"m","kind":"solid","color":[1,1,1,1],"in":1,"visible":false}},{}"#, white("w", &format!(r#","matte":{{"layer":"m","mode":"{mode}"}}"#))));
    let Some(img) = draw(&later("alpha")) else { return };
    assert_eq!(area(&img), 0.0);
    let Some(img) = draw(&later("luma_inverted")) else { return };
    assert_eq!(area(&img), (S * S) as f64);
}

#[test]
fn a_hidden_matte_source_does_not_draw() {
    // A red square in the middle, hidden: only the white layer it mattes shows.
    let source = r#"{"id":"m","kind":"solid","color":[1,0,0,1],"transform":{"scale":0.5},"visible":false}"#;
    let Some(img) = draw(&masked("", &format!("{source},{}", white("w", r#","matte":{"layer":"m"}"#)))) else { return };
    assert_px(&img, 64, 64, [255, 255, 255], 0);
    assert_px(&img, 10, 10, [0, 0, 0], 0);
    // Alone it draws nothing at all.
    let Some(img) = draw(&masked("", source)) else { return };
    assert_eq!(energy(&img), 0.0);
    // Visible, it draws as well: red where the matted layer does not cover it.
    let both = masked("", &format!(r#"{{"id":"m","kind":"solid","color":[1,0,0,1],"transform":{{"scale":0.5}}}},{}"#, white("w", r#","transform":{"position":[0.375,0,0]},"matte":{"layer":"m"}"#)));
    let Some(img) = draw(&both) else { return };
    assert_px(&img, 40, 64, [255, 0, 0], 0);
    assert_px(&img, 80, 64, [255, 255, 255], 0);
    assert_px(&img, 110, 64, [0, 0, 0], 0);
}

#[test]
fn a_picture_inside_letters() {
    let pic = quadrants("masks-quadrants.png", 64, 64);
    let slots = format!(r#""pic":{{"path":"{pic}"}}"#);
    let title = r#"{"id":"title","kind":"text","text":"HI","size":60,"fill":[1,1,1]}"#;
    let Some(letters) = draw(&masked(&slots, title)) else { return };
    let hidden = title.replace(r#""fill":[1,1,1]"#, r#""fill":[1,1,1],"visible":false"#);
    let p = masked(&slots, &format!(r#"{hidden},{{"id":"img","kind":"image","slot":"pic","matte":{{"layer":"title"}}}}"#));
    let Some(img) = draw(&p) else { return };
    let mut inside = 0;
    for y in 0..S {
        for x in 0..S {
            let text = letters.get_pixel(x, y).0[0] as i32;
            let p = img.get_pixel(x, y).0;
            let lit = p[0].max(p[1]).max(p[2]) as i32;
            // The picture shows where the letters are, and only there.
            let quadrant = match (x < S / 2, y < S / 2) {
                (true, true) => [255, 0, 0],
                (false, true) => [0, 255, 0],
                (true, false) => [0, 0, 255],
                (false, false) => [255, 255, 0],
            };
            if text == 0 {
                assert_eq!(lit, 0, "({x}, {y}) is outside the letters");
            } else if text == 255 && x.abs_diff(S / 2) > 2 && y.abs_diff(S / 2) > 2 {
                inside += 1;
                for c in 0..3 {
                    assert!((p[c] as i32 - quadrant[c] as i32).abs() <= 2, "({x}, {y}) {p:?} vs {quadrant:?}");
                }
            }
        }
    }
    assert!(inside > 500, "the letters cover {inside} pixels");
}

#[test]
fn masks_on_an_adjustment_layer_limit_its_effects() {
    let grey = flat("masks-grey.png", 8, 8, [64, 64, 64, 255]);
    let slots = format!(r#""g":{{"path":"{grey}"}}"#);
    let layers = r#"{"id":"bg","kind":"image","slot":"g"},
        {"id":"fx","kind":"adjustment","effects":[{"kind":"invert"}],"masks":[{"shape":"rect","size":[0.5,1],"position":[-0.25,0]}]}"#;
    let Some(img) = draw(&masked(&slots, layers)) else { return };
    // Left half inverted (display-referred), right half untouched.
    assert_px(&img, 10, 64, [191, 191, 191], 1);
    assert_px(&img, 60, 10, [191, 191, 191], 1);
    assert_px(&img, 70, 64, [64, 64, 64], 1);
    assert_px(&img, 120, 120, [64, 64, 64], 1);
    // With a feathered, inverted ellipse the middle is untouched and the corners inverted.
    let soft = r#"{"id":"bg","kind":"image","slot":"g"},
        {"id":"fx","kind":"adjustment","effects":[{"kind":"invert"}],"masks":[{"shape":"ellipse","size":0.6,"feather":10,"inverted":true}]}"#;
    let Some(img) = draw(&masked(&slots, soft)) else { return };
    assert_px(&img, 64, 64, [64, 64, 64], 1);
    assert_px(&img, 2, 2, [191, 191, 191], 1);
    // An adjustment layer can have a matte too.
    let matted = r#"{"id":"bg","kind":"image","slot":"g"},{"id":"m","kind":"solid","color":[1,1,1,1],"transform":{"scale":[0.5,1],"position":[0.25,0,0]},"visible":false},
        {"id":"fx","kind":"adjustment","effects":[{"kind":"invert"}],"matte":{"layer":"m"}}"#;
    let Some(img) = draw(&masked(&slots, matted)) else { return };
    assert_px(&img, 10, 64, [64, 64, 64], 1);
    assert_px(&img, 100, 64, [191, 191, 191], 1);
}

#[test]
fn masked_frames_are_deterministic_and_follow_keys() {
    let p = masked(
        "",
        &white("w", r#","masks":[{"shape":"ellipse","size":{"keys":[{"t":0,"v":0.2},{"t":1,"v":1.2}]},"feather":4}]"#),
    );
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let a = render(r, &p, 0);
    let b = render(r, &p, 0);
    assert_eq!(a.as_raw(), b.as_raw());
    // The iris opens: more of the layer shows at frame 5 than at frame 0.
    let c = render(r, &p, 5);
    assert!(area(&c) > 4.0 * area(&a), "{} vs {}", area(&c), area(&a));
}
