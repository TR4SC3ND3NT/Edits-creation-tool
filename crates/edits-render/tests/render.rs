//! Pixel tests of the frame loop: colour, blending, timing, geometry and
//! motion blur. Every test skips when the machine has no adapter.

mod common;
use common::*;
use image::{Rgba, RgbaImage};

/// Unwrap the shared renderer or return from the test.
macro_rules! gpu {
    ($r:ident) => {
        let Some(mut guard) = renderer() else { return };
        let $r = guard.as_mut().unwrap();
    };
}

#[test]
fn solid_and_background_round_trip_srgb() {
    gpu!(r);
    let p = project(r#","background":[0.2,0.4,0.6]"#, r#""layers":[{"id":"s","kind":"solid","color":[0.5,0.25,0.75,1],"in":1}]"#);
    let before = render(r, &p, 0);
    assert_eq!(before.dimensions(), (64, 64));
    for (x, y) in [(0, 0), (32, 32), (63, 63)] {
        assert_px(&before, x, y, [51, 102, 153], 1);
        assert_eq!(before.get_pixel(x, y).0[3], 255);
    }
    let solid = render(r, &p, 10);
    for (x, y) in [(0, 0), (32, 32), (63, 63)] {
        assert_px(&solid, x, y, [128, 64, 191], 1);
    }
}

#[test]
fn exposure_one_stop_doubles_linear_value() {
    gpu!(r);
    let layers = |stops: f64| {
        format!(r#""layers":[{{"id":"s","kind":"solid","color":[0.2,0.3,0.4,1],"effects":[{{"kind":"exposure","params":{{"stops":{stops}}}}}]}}]"#)
    };
    let img = render(r, &project("", &layers(1.0)), 0);
    let want = [0.2, 0.3, 0.4].map(|c| byte(2.0 * to_linear(c)));
    assert_px(&img, 20, 40, want, 1);
    let img = render(r, &project("", &layers(-1.0)), 0);
    assert_px(&img, 20, 40, [0.2, 0.3, 0.4].map(|c| byte(0.5 * to_linear(c))), 1);
    // The same through an adjustment layer.
    let p = project(
        r#","background":[0.2,0.3,0.4]"#,
        r#""layers":[{"id":"fx","kind":"adjustment","effects":[{"kind":"exposure","params":{"stops":1}}]}]"#,
    );
    assert_px(&render(r, &p, 0), 5, 5, want, 1);
}

#[test]
fn invert_is_a_display_referred_negative() {
    gpu!(r);
    let p = project("", r#""layers":[{"id":"s","kind":"solid","color":[0.2,0.5,0.9,1],"effects":[{"kind":"invert"}]}]"#);
    assert_px(&render(r, &p, 0), 32, 32, [204, 128, 26], 1);
    let half = project("", r#""layers":[{"id":"s","kind":"solid","color":[0.2,0.5,0.9,1],"effects":[{"kind":"invert","params":{"amount":0.5}}]}]"#);
    assert_px(&render(r, &half, 0), 32, 32, [128, 128, 128], 1);
}

#[test]
fn threshold_gives_two_tones() {
    gpu!(r);
    let ramp = save("ramp.png", &RgbaImage::from_fn(64, 64, |x, _| Rgba([(x * 4) as u8, (x * 4) as u8, (x * 4) as u8, 255])));
    let p = project(
        "",
        &format!(
            r#""slots":{{"a":{{"path":"{ramp}"}}}},
               "layers":[{{"id":"a","kind":"image","slot":"a","effects":[{{"kind":"threshold","params":{{"level":0.5,"softness":0}}}}]}}]"#
        ),
    );
    let img = render(r, &p, 0);
    let (mut black, mut white) = (0, 0);
    for px in img.pixels() {
        match px.0[0] {
            0..=2 => black += 1,
            253..=255 => white += 1,
            v => panic!("a third tone: {v}"),
        }
    }
    assert!(black > 1500 && white > 1500, "black {black}, white {white}");
    // The cut is at display-referred mid grey: x = 32 is value 128.
    assert_px(&img, 28, 10, [0, 0, 0], 2);
    assert_px(&img, 36, 10, [255, 255, 255], 2);
}

#[test]
fn saturation_zero_gives_equal_channels() {
    gpu!(r);
    let p = project("", r#""layers":[{"id":"s","kind":"solid","color":[0.9,0.3,0.1,1],"effects":[{"kind":"saturation","params":{"amount":0}}]}]"#);
    let img = render(r, &p, 0);
    let l = 0.2126 * to_linear(0.9) + 0.7152 * to_linear(0.3) + 0.0722 * to_linear(0.1);
    assert_px(&img, 32, 32, [byte(l); 3], 1);
    // amount 1 is skipped: the colour is untouched.
    let same = project("", r#""layers":[{"id":"s","kind":"solid","color":[0.9,0.3,0.1,1],"effects":[{"kind":"saturation","params":{"amount":1}}]}]"#);
    assert_px(&render(r, &same, 0), 32, 32, [230, 77, 26], 1);
}

#[test]
fn every_blend_mode_matches_hand_computed_values() {
    gpu!(r);
    let (base, top) = ([0.6, 0.3, 0.2], [0.3, 0.7, 0.4]);
    let (b, s) = (base.map(to_linear), top.map(to_linear));
    type Mode = (&'static str, fn(f64, f64) -> f64);
    let modes: [Mode; 8] = [
        ("normal", |_, s| s),
        ("add", |b, s| b + s),
        ("screen", |b, s| b + s - b * s),
        ("multiply", |b, s| b * s),
        ("overlay", |b, s| if b < 0.5 { 2.0 * b * s } else { 1.0 - 2.0 * (1.0 - b) * (1.0 - s) }),
        ("lighten", f64::max),
        ("darken", f64::min),
        ("difference", |b, s| (b - s).abs()),
    ];
    // A full-frame layer without effects is drawn and blended in one pass;
    // with an effect (here one too weak to see) it goes through a layer
    // texture and the blend pass. Both must give the hand-computed value.
    for (name, f) in modes {
        for opacity in [1.0, 0.5] {
            for effects in ["", r#","effects":[{"kind":"exposure","params":{"stops":1e-9}}]"#] {
                let p = project(
                    &format!(r#","background":{base:?}"#),
                    &format!(
                        r#""layers":[{{"id":"s","kind":"solid","color":[{},{},{},1],"blend":"{name}","transform":{{"opacity":{opacity}}}{effects}}}]"#,
                        top[0], top[1], top[2]
                    ),
                );
                let img = render(r, &p, 0);
                let want = [0, 1, 2].map(|i| byte(b[i] + (f(b[i], s[i]) - b[i]) * opacity));
                let got = px(&img, 31, 33);
                assert!((0..3).all(|i| (got[i] as i32 - want[i] as i32).abs() <= 1), "{name} at opacity {opacity} {effects}: got {got:?}, expected {want:?}");
            }
        }
    }
    // A picture blends like a solid: screen a half-covering white box over the base.
    let p = project(&format!(r#","background":{base:?}"#), r#""layers":[{"id":"s","kind":"solid","color":[0.3,0.7,0.4,1],"blend":"screen","transform":{"scale":0.5}}]"#);
    let img = render(r, &p, 0);
    assert_px(&img, 32, 32, [0, 1, 2].map(|i| byte(b[i] + s[i] - b[i] * s[i])), 1);
    assert_px(&img, 4, 4, [153, 77, 51], 1);
}

#[test]
fn layers_appear_at_in_and_leave_at_out() {
    gpu!(r);
    let p = project("", r#""layers":[{"id":"s","kind":"solid","color":[1,1,1,1],"in":1,"out":2}]"#);
    for (frame, lit) in [(0, false), (9, false), (10, true), (15, true), (19, true), (20, false), (39, false)] {
        let img = render(r, &p, frame);
        assert_px(&img, 32, 32, if lit { [255; 3] } else { [0; 3] }, 0);
    }
    // Disabled layers and effects are skipped.
    let off = project(
        "",
        r#""layers":[{"id":"a","kind":"solid","color":[1,0,0,1],"enabled":false},
                     {"id":"b","kind":"solid","color":[0,0,1,1],"effects":[{"kind":"invert","enabled":false}]}]"#,
    );
    assert_px(&render(r, &off, 0), 32, 32, [0, 0, 255], 0);
}

#[test]
fn opacity_mixes_in_linear_light() {
    gpu!(r);
    let p = project("", r#""layers":[{"id":"s","kind":"solid","color":[1,1,1,1],"transform":{"opacity":0.5}}]"#);
    assert_px(&render(r, &p, 0), 32, 32, [188; 3], 1);
    // Straight alpha in the colour behaves the same way.
    let p = project("", r#""layers":[{"id":"s","kind":"solid","color":[1,1,1,0.5]}]"#);
    assert_px(&render(r, &p, 0), 32, 32, [188; 3], 1);
    // Animated opacity: 0 at t = 0, 1 at t = 1.
    let p = project("", r#""layers":[{"id":"s","kind":"solid","color":[1,1,1,1],"transform":{"opacity":{"keys":[{"t":0,"v":0},{"t":1,"v":1}]}}}]"#);
    assert_px(&render(r, &p, 0), 32, 32, [0; 3], 0);
    assert_px(&render(r, &p, 5), 32, 32, [188; 3], 1);
    assert_px(&render(r, &p, 10), 32, 32, [255; 3], 0);
}

#[test]
fn pictures_are_upright_and_transforms_follow_the_conventions() {
    gpu!(r);
    let q = quadrants("quad.png", 64, 64);
    let layer = |transform: &str| project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{{transform}}}}}]"#));
    let (red, green, blue, yellow, black) = ([255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0], [0, 0, 0]);

    let img = render(r, &layer(""), 0);
    assert_px(&img, 8, 8, red, 1);
    assert_px(&img, 56, 8, green, 1);
    assert_px(&img, 8, 56, blue, 1);
    assert_px(&img, 56, 56, yellow, 1);
    assert_px(&img, 0, 0, red, 1); // scale 1 fills the frame to its corners
    assert_px(&img, 63, 63, yellow, 1);

    // Scale 0.5 leaves a border of background.
    let img = render(r, &layer(r#""scale":0.5"#), 0);
    assert_px(&img, 8, 8, black, 0);
    assert_px(&img, 24, 24, red, 1);
    assert_px(&img, 40, 40, yellow, 1);

    // +x is right, +y is down.
    let img = render(r, &layer(r#""scale":0.5,"position":[0.25,0.25,0]"#), 0);
    assert_px(&img, 24, 24, black, 0);
    assert_px(&img, 40, 40, red, 1);
    assert_px(&img, 56, 56, yellow, 1);

    // +z rotation is clockwise: the left half moves to the top.
    let img = render(r, &layer(r#""rotation":[0,0,90]"#), 0);
    assert_px(&img, 8, 8, blue, 1);
    assert_px(&img, 56, 8, red, 1);
    assert_px(&img, 56, 56, green, 1);

    // The anchor is the pivot: with the right edge pinned to the centre the
    // half-size picture sits in the left half of the frame.
    let img = render(r, &layer(r#""scale":0.5,"anchor":[0.5,0]"#), 0);
    assert_px(&img, 8, 24, red, 1);
    assert_px(&img, 24, 40, yellow, 1);
    assert_px(&img, 40, 32, black, 0);

    // Farther away is smaller.
    let img = render(r, &layer(r#""position":[0,0,1.5]"#), 0);
    assert_px(&img, 4, 4, black, 0);
    assert_px(&img, 28, 28, red, 1);
}

#[test]
fn layer_edges_are_antialiased_by_coverage() {
    gpu!(r);
    // A white half-size square turned 30 degrees: its light must equal its
    // area (a quarter of the frame) and its slanted edges must have in-between pixels.
    for rotation in ["[0,0,30]", "[0,0,-17]", "[0,0,45]"] {
        let p = project("", &format!(r#""layers":[{{"id":"s","kind":"solid","color":[1,1,1,1],"transform":{{"scale":0.5,"rotation":{rotation}}}}}]"#));
        let img = render(r, &p, 0);
        let light = energy(&img) / 3.0;
        assert!((light - 1024.0).abs() < 6.0, "rotation {rotation}: light {light}, expected 1024");
        let partial = img.pixels().filter(|p| p.0[0] > 8 && p.0[0] < 248).count();
        assert!(partial > 60, "rotation {rotation}: only {partial} edge pixels are partly covered");
        assert_px(&img, 32, 32, [255; 3], 0);
        assert_px(&img, 2, 2, [0; 3], 0);
    }
    // Edges that fall between pixels are covered in proportion: a box from
    // x = 16.5 to 48.5 lights its edge columns by half.
    let p = project("", r#""layers":[{"id":"s","kind":"solid","color":[1,1,1,1],"transform":{"scale":0.5,"position":[0.0078125,0,0]}}]"#);
    let img = render(r, &p, 0);
    assert_px(&img, 16, 32, [byte(0.5); 3], 2);
    assert_px(&img, 48, 32, [byte(0.5); 3], 2);
    assert_px(&img, 17, 32, [255; 3], 0);
    assert_px(&img, 15, 32, [0; 3], 0);
    // A cube's faces meet without a seam and its outline is smoothed too.
    let white = flat("white.png", 8, 8, [255; 4]);
    let p = project("", &format!(r#""slots":{{"w":{{"path":"{white}"}}}},"layers":[{{"id":"c","kind":"cube","faces":["w"],"size":[0.4,0.4,0.4],"transform":{{"rotation":[20,35,10]}}}}]"#));
    let img = render(r, &p, 0);
    let partial = img.pixels().filter(|p| p.0[0] > 8 && p.0[0] < 248).count();
    assert!(partial > 40, "only {partial} partly covered pixels on the cube's outline");
    for (x, y) in [(32, 32), (30, 30), (34, 30), (30, 34), (34, 34), (32, 28), (32, 36), (28, 32), (36, 32)] {
        assert_px(&img, x, y, [255; 3], 0);
    }
}

#[test]
fn camera_moves_and_field_of_view() {
    gpu!(r);
    let q = quadrants("quad.png", 64, 64);
    let cam = |camera: &str| project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"camera":{{{camera}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.5}}}}]"#));
    let (red, yellow, black) = ([255, 0, 0], [255, 255, 0], [0, 0, 0]);
    // The plane z = 0 maps onto the frame whatever the field of view.
    for fov in [20, 90] {
        let img = render(r, &cam(&format!(r#""fov":{fov}"#)), 0);
        assert_px(&img, 14, 14, black, 0);
        assert_px(&img, 18, 18, red, 1);
        assert_px(&img, 46, 46, yellow, 1);
        assert_px(&img, 50, 50, black, 0);
    }
    // Moving the camera right moves the picture left.
    let img = render(r, &cam(r#""position":[0.25,0,0]"#), 0);
    assert_px(&img, 2, 18, red, 1);
    assert_px(&img, 40, 32, black, 0);
    // Moving it forward makes the picture bigger.
    let img = render(r, &cam(r#""position":[0,0,0.7]"#), 0);
    assert_px(&img, 4, 4, red, 1);
}

#[test]
fn cube_shows_the_face_turned_toward_the_viewer() {
    gpu!(r);
    let faces = [("front", [255, 0, 0]), ("right", [0, 255, 0]), ("back", [0, 0, 255]), ("left", [255, 255, 0]), ("top", [255, 0, 255]), ("bottom", [0, 255, 255])];
    let mut slots = Vec::new();
    for (name, c) in faces {
        let file = flat(&format!("face-{name}.png"), 8, 8, [c[0], c[1], c[2], 255]);
        slots.push(format!(r#""{name}":{{"path":"{file}"}}"#));
    }
    // A unit cube pushed back by half its depth: the face toward the viewer
    // lies in the plane z = 0 and fills the frame.
    let cube = |rotation: &str| {
        project(
            "",
            &format!(
                r#""slots":{{{}}},"layers":[{{"id":"c","kind":"cube","faces":["front","right","back","left","top","bottom"],
                   "transform":{{"position":[0,0,0.5],"rotation":{rotation}}}}}]"#,
                slots.join(",")
            ),
        )
    };
    for (rotation, face) in [("[0,0,0]", 0), ("[0,90,0]", 1), ("[0,180,0]", 2), ("[0,-90,0]", 3), ("[90,0,0]", 4), ("[-90,0,0]", 5)] {
        let img = render(r, &cube(rotation), 0);
        for (x, y) in [(32, 32), (3, 3), (60, 60)] {
            assert_px(&img, x, y, faces[face].1, 1);
        }
    }
    // Half way through the turn both faces are visible, the right-hand one on the right.
    let img = render(r, &cube("[0,45,0]"), 0);
    assert_px(&img, 22, 32, faces[0].1, 1);
    assert_px(&img, 42, 32, faces[1].1, 1);
}

#[test]
fn cube_faces_are_upright_and_repeat_the_last_slot() {
    gpu!(r);
    let q = quadrants("quad.png", 64, 64);
    let white = flat("white.png", 8, 8, [255; 4]);
    let cube = |faces: &str, rotation: &str| {
        project(
            "",
            &format!(
                r#""slots":{{"q":{{"path":"{q}"}},"w":{{"path":"{white}"}}}},
                   "layers":[{{"id":"c","kind":"cube","faces":{faces},"transform":{{"position":[0,0,0.5],"rotation":{rotation}}}}}]"#
            ),
        )
    };
    // Whichever face is turned to the front shows its picture the right way up, not mirrored.
    for rotation in ["[0,0,0]", "[0,90,0]", "[0,180,0]", "[0,-90,0]", "[90,0,0]", "[-90,0,0]"] {
        let img = render(r, &cube(r#"["q"]"#, rotation), 0);
        assert_px(&img, 8, 8, [255, 0, 0], 1);
        assert_px(&img, 56, 8, [0, 255, 0], 1);
        assert_px(&img, 8, 56, [0, 0, 255], 1);
        assert_px(&img, 56, 56, [255, 255, 0], 1);
    }
    // Two slots given: the second repeats for the remaining faces.
    let img = render(r, &cube(r#"["q","w"]"#, "[0,180,0]"), 0);
    assert_px(&img, 8, 8, [255; 3], 1);
    // A non-cubic box: `size` is width, height, depth. Its front face covers
    // half the frame's width and all of its height.
    let p = project(
        "",
        &format!(r#""slots":{{"w":{{"path":"{white}"}}}},"layers":[{{"id":"c","kind":"cube","faces":["w"],"size":[0.5,1,0.2],"transform":{{"position":[0,0,0.1]}}}}]"#),
    );
    let img = render(r, &p, 0);
    assert_px(&img, 12, 32, [0; 3], 0);
    assert_px(&img, 20, 2, [255; 3], 1);
    assert_px(&img, 44, 61, [255; 3], 1);
    assert_px(&img, 52, 32, [0; 3], 0);
}

#[test]
fn fit_cover_crops_and_contain_adds_bars() {
    gpu!(r);
    // Twice as wide as tall: left half red, right half blue.
    let wide = save("wide.png", &RgbaImage::from_fn(128, 64, |x, _| if x < 64 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 0, 255, 255]) }));
    let fit = |slot: &str| project(r#","background":[0,1,0]"#, &format!(r#""slots":{{"a":{{"path":"{wide}",{slot}}}}},"layers":[{{"id":"a","kind":"image","slot":"a"}}]"#));
    let (red, blue, green) = ([255, 0, 0], [0, 0, 255], [0, 255, 0]);

    let img = render(r, &fit(r#""fit":"cover""#), 0);
    for y in [1, 32, 62] {
        assert_px(&img, 8, y, red, 1);
        assert_px(&img, 56, y, blue, 1);
    }
    // `focus` chooses the part that survives the crop.
    let img = render(r, &fit(r#""fit":"cover","focus":[0,0.5]"#), 0);
    assert_px(&img, 8, 32, red, 1);
    assert_px(&img, 56, 32, red, 1);
    let img = render(r, &fit(r#""fit":"cover","focus":[1,0.5]"#), 0);
    assert_px(&img, 8, 32, blue, 1);

    let img = render(r, &fit(r#""fit":"contain""#), 0);
    assert_px(&img, 8, 4, green, 1); // bar above
    assert_px(&img, 56, 60, green, 1); // bar below
    assert_px(&img, 8, 32, red, 1);
    assert_px(&img, 56, 32, blue, 1);
    assert_px(&img, 2, 20, red, 1); // the picture spans the full width
    assert_px(&img, 8, 13, green, 1);

    let img = render(r, &fit(r#""fit":"stretch""#), 0);
    for y in [1, 62] {
        assert_px(&img, 8, y, red, 1);
        assert_px(&img, 56, y, blue, 1);
    }
}

#[test]
fn transparent_pictures_composite_without_fringes() {
    gpu!(r);
    // Opaque white on the left, fully transparent on the right. The colour
    // hidden under alpha 0 (black or white) must not leak into the edge.
    let cut = |name: &str, hidden: u8| save(name, &RgbaImage::from_fn(64, 64, |x, _| if x < 32 { Rgba([255, 255, 255, 255]) } else { Rgba([hidden, hidden, hidden, 0]) }));
    let (cut_black, cut_white) = (cut("cut-black.png", 0), cut("cut-white.png", 255));
    let half = save("half.png", &RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 128])));
    let p = |file: &str, scale: f64| {
        project(r#","background":[0,0,1]"#, &format!(r#""slots":{{"a":{{"path":"{file}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":{scale}}}}}]"#))
    };
    let img = render(r, &p(&cut_black, 1.0), 0);
    assert_px(&img, 20, 32, [255, 255, 255], 1);
    assert_px(&img, 44, 32, [0, 0, 255], 1);
    // Scaled so the edge falls between pixels: every pixel is white over blue
    // in some proportion, so blue stays full and red equals green.
    let img = render(r, &p(&cut_black, 0.77), 0);
    for x in 0..64 {
        let c = px(&img, x, 32);
        assert!(c[0] == c[1] && c[2] == 255, "fringe at x = {x}: {c:?}");
    }
    assert!(img == render(r, &p(&cut_white, 0.77), 0), "the colour hidden under alpha 0 leaked");
    // Half-transparent white over blue: linear 0.5 in red and green.
    let a = 128.0 / 255.0;
    assert_px(&render(r, &p(&half, 1.0), 0), 32, 32, [byte(a), byte(a), 255], 1);
}

#[test]
fn empty_slots_render_a_placeholder() {
    gpu!(r);
    let p = project("", r#""slots":{"a":{},"b":{}},"layers":[{"id":"a","kind":"image","slot":"a","out":1},{"id":"b","kind":"image","slot":"b","in":1}]"#);
    r.prepare(&p, &dir()).unwrap();
    let a = r.render_frame(0, 1.0).unwrap();
    let b = r.render_frame(10, 1.0).unwrap();
    assert!(energy(&a) > 100.0 && energy(&b) > 100.0);
    assert!(a != b, "placeholders should differ per slot");
}

#[test]
fn the_same_frame_twice_is_byte_identical() {
    gpu!(r);
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/beat-loop.json")).unwrap();
    let p = edits_core::Project::from_json(&text).unwrap();
    r.set_motion_blur_samples(None);
    r.prepare(&p, &dir()).unwrap();
    for frame in [5, 30, 37, 52, 80] {
        let a = r.render_frame(frame, 0.2).unwrap();
        let other = r.render_frame(frame + 1, 0.2).unwrap();
        let b = r.render_frame(frame, 0.2).unwrap();
        assert!(a == b, "frame {frame} differs between two renders");
        assert!(a != other, "frames {frame} and {} are identical", frame + 1);
    }
    // Preparing again from scratch gives the same bytes too.
    let a = r.render_frame(52, 0.2).unwrap();
    r.prepare(&p, &dir()).unwrap();
    assert!(a == r.render_frame(52, 0.2).unwrap(), "preparing again changed the frame");
    // render_at at a frame's start time is that frame.
    assert!(a == r.render_at(p.frame_time(52), 0.2).unwrap(), "render_at differs from render_frame");
}

#[test]
fn motion_blur_spreads_a_moving_layer() {
    gpu!(r);
    // A white bar a quarter of the frame wide, crossing the frame in 0.4 s.
    let moving = |samples: u32, shutter: f64| {
        project(
            &format!(r#","motion_blur":{{"samples":{samples},"shutter":{shutter}}}"#),
            r#""layers":[{"id":"bar","kind":"solid","color":[1,1,1,1],
                 "transform":{"scale":[0.25,1],"position":{"keys":[{"t":0,"v":[-0.5,0,0]},{"t":0.4,"v":[0.5,0,0]}]}}}]"#,
        )
    };
    let sharp = render(r, &moving(1, 1.0), 1);
    let blurred = render(r, &moving(8, 1.0), 1);
    let lit = |img: &RgbaImage| (0..64).filter(|x| img.get_pixel(*x, 32).0[0] > 4).count();
    let full = |img: &RgbaImage| (0..64).filter(|x| img.get_pixel(*x, 32).0[0] > 250).count();
    // One frame moves the bar a quarter of the frame (16 px): its footprint
    // grows by 14 px (7/8 of the travel) while only a sliver stays fully lit.
    assert_eq!(lit(&sharp), 16);
    assert_eq!(full(&sharp), 16);
    assert!((29..=31).contains(&lit(&blurred)), "lit {}", lit(&blurred));
    assert!(full(&blurred) <= 3, "fully lit {}", full(&blurred));
    // Averaging happens in linear light, so the total light is unchanged.
    let (a, b) = (energy(&sharp), energy(&blurred));
    assert!((a - b).abs() / a < 0.02, "energy {a} vs {b}");
    // The trail is a linear ramp: a pixel covered by half the samples is linear 0.5.
    let x = (0..64).find(|x| blurred.get_pixel(*x, 32).0[0] > 250).expect("a fully lit column");
    assert_px(&blurred, x - 8, 32, [byte(0.5); 3], 2);
    // A shorter shutter spreads less; a still layer is not changed at all.
    let short = render(r, &moving(8, 0.5), 1);
    assert!(lit(&short) < lit(&blurred) && lit(&short) > lit(&sharp));
    let still = |samples: u32| project(&format!(r#","motion_blur":{{"samples":{samples}}}"#), r#""layers":[{"id":"s","kind":"solid","color":[0.3,0.6,0.9,1],"transform":{"scale":0.5}}]"#);
    assert!(render(r, &still(1), 0) == render(r, &still(6), 0), "motion blur changed a still layer");
    // The sample count can be overridden without touching the project.
    r.prepare(&moving(1, 1.0), &dir()).unwrap();
    r.set_motion_blur_samples(Some(8));
    assert!(r.render_frame(1, 1.0).unwrap() == blurred, "the sample override differs from the project setting");
    r.set_motion_blur_samples(None);
}

#[test]
fn scale_renders_a_smaller_frame_of_the_same_picture() {
    gpu!(r);
    let q = quadrants("quad.png", 64, 64);
    let p = project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.5}}}}]"#));
    r.prepare(&p, &dir()).unwrap();
    assert_eq!(r.output_size(1.0).unwrap(), (64, 64));
    assert_eq!(r.output_size(0.5).unwrap(), (32, 32));
    assert_eq!(r.output_size(0.3).unwrap(), (20, 20)); // rounded to even
    let img = r.render_frame(0, 0.5).unwrap();
    assert_eq!(img.dimensions(), (32, 32));
    assert_px(&img, 4, 4, [0; 3], 0);
    assert_px(&img, 12, 12, [255, 0, 0], 1);
    assert_px(&img, 20, 20, [255, 255, 0], 1);
    let big = r.render_frame(0, 2.0).unwrap();
    assert_eq!(big.dimensions(), (128, 128));
    assert_px(&big, 48, 48, [255, 0, 0], 1);
}

#[test]
fn rendering_needs_a_prepared_project_and_reports_missing_pictures() {
    let Ok(mut fresh) = edits_render::Renderer::new(edits_render::RenderOptions::default()) else { return };
    assert!(fresh.render_frame(0, 1.0).unwrap_err().to_string().contains("prepare"));
    let p = project("", r#""slots":{"a":{"path":"nope.png"}},"layers":[{"id":"a","kind":"image","slot":"a"}]"#);
    let err = format!("{:#}", fresh.prepare(&p, &dir()).unwrap_err());
    assert!(err.contains("slot \"a\"") && err.contains("nope.png"), "{err}");
    assert!(!fresh.info().name.is_empty() && !fresh.info().backend.is_empty());
}

#[test]
fn render_stack_matches_frame() {
    gpu!(r);
    // The example project: pictures, cubes, adjustment layers, transitions.
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/beat-loop.json")).unwrap();
    let p = edits_core::Project::from_json(&text).unwrap();
    r.prepare(&p, &dir()).unwrap();
    r.set_motion_blur_samples(Some(1));
    for frame in [0, 12, 30, 37, 52, 80] {
        let whole = r.render_frame(frame, 0.25).unwrap();
        let stack = r.render_layers(&p.layers, p.frame_time(frame), 0.25).unwrap();
        assert!(whole == stack, "frame {frame}: the stack of all layers differs from the frame");
    }
    r.set_motion_blur_samples(None);

    // A subset of the layers is the frame of a project holding only those.
    let q = quadrants("quad.png", 64, 64);
    let layers = format!(
        r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[
            {{"id":"bg","kind":"solid","color":[0.2,0.3,0.4,1]}},
            {{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.6,"rotation":[0,0,10]}},"effects":[{{"kind":"gaussian_blur","params":{{"radius":3}}}},{{"kind":"exposure","params":{{"stops":1}}}}]}},
            {{"id":"c","kind":"cube","faces":["a"],"size":[0.4,0.4,0.4],"transform":{{"rotation":[20,30,0]}},"in":1}},
            {{"id":"fx","kind":"adjustment","effects":[{{"kind":"vignette","params":{{"amount":0.8}}}}],"blend":"multiply","transform":{{"opacity":0.7}}}}]"#
    );
    let full = project("", &layers);
    r.prepare(&full, &dir()).unwrap();
    let ids = ["bg", "a", "fx"];
    let subset: Vec<edits_core::Layer> = full.layers.iter().filter(|l| ids.contains(&l.id.as_str())).cloned().collect();
    let alone = r.render_layers(&subset, 2.0, 1.0).unwrap();
    let mut only = full.clone();
    only.layers = subset;
    assert!(alone == render(r, &only, 20), "a subset of layers differs from a project of only those layers");

    // One layer's own picture: through the camera, effects applied, opacity not.
    let solid = project("", r#""layers":[{"id":"s","kind":"solid","color":[1,0,0,1],"transform":{"scale":0.5,"opacity":0.5},"in":1,"effects":[{"kind":"exposure","params":{"stops":-1}}]},{"id":"fx","kind":"adjustment"}]"#);
    r.prepare(&solid, &dir()).unwrap();
    let img = r.render_layer(&solid.layers[0], 2.0, 1.0).unwrap().expect("visible at 2");
    assert_eq!(img.get_pixel(32, 32).0, [byte(0.5), 0, 0, 255]);
    assert_eq!(img.get_pixel(2, 2).0[3], 0, "outside the layer is transparent");
    assert!(r.render_layer(&solid.layers[0], 0.5, 1.0).unwrap().is_none(), "not visible before its in");
    assert!(r.render_layer(&solid.layers[1], 2.0, 1.0).unwrap().is_none(), "an adjustment layer has no picture");
}
