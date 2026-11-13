//! Pixel tests of the multi-pass light effects: bloom, light rays, lens
//! flare, outline, drop shadow and tilt shift. Every test skips when the
//! machine has no adapter.

mod common;
use common::*;
use edits_core::Project;
use image::RgbaImage;

macro_rules! gpu {
    ($r:ident) => {
        let Some(mut guard) = renderer() else { return };
        let $r = guard.as_mut().unwrap();
    };
}

/// A `w` x `h` project with `layers` (a JSON array body) on a black background.
fn sized(w: u32, h: u32, layers: &str) -> Project {
    let text = format!(r#"{{"format":"edits.project/1","settings":{{"width":{w},"height":{h},"fps":10,"duration":1}},"layers":[{layers}]}}"#);
    Project::from_json(&text).unwrap_or_else(|e| panic!("bad test project: {e}\n{text}"))
}

/// A white box of `size` (fraction of the frame) at `position` with
/// `effects` on the box layer itself.
fn box_layer(size: f64, position: [f64; 2], effects: &str) -> String {
    format!(r#"{{"id":"box{}","kind":"solid","color":[1,1,1,1],"transform":{{"scale":{size},"position":[{},{},0]}},"effects":[{effects}]}}"#, (size * 1000.0) as u32, position[0], position[1])
}

fn lin(img: &RgbaImage, x: u32, y: u32, c: usize) -> f64 {
    to_linear(img.get_pixel(x, y).0[c] as f64 / 255.0)
}

/// `img` halved by averaging 2x2 blocks in linear light.
fn halve(img: &RgbaImage) -> Vec<[f64; 3]> {
    let (w, h) = (img.width() / 2, img.height() / 2);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut c = [0.0; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                for (i, v) in c.iter_mut().enumerate() {
                    *v += lin(img, 2 * x + dx, 2 * y + dy, i) / 4.0;
                }
            }
            out.push(c);
        }
    }
    out
}

#[test]
fn light_effects_at_zero_strength_change_nothing() {
    gpu!(r);
    let q = quadrants("light-quad.png", 64, 64);
    let with = |effects: &str| project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.8}},"effects":[{effects}]}}]"#));
    let plain = render(r, &with(""), 0);
    let neutral = r#"
        {"kind":"bloom","params":{"intensity":0}}, {"kind":"light_rays","params":{"intensity":0}}, {"kind":"lens_flare","params":{"brightness":0}},
        {"kind":"outline","params":{"width":0}}, {"kind":"drop_shadow","params":{"opacity":0}}, {"kind":"tilt_shift","params":{"blur":0}}"#;
    assert!(plain == render(r, &with(neutral), 0), "neutral light effects changed the picture");
}

#[test]
fn bloom_below_its_threshold_leaves_the_picture_unchanged() {
    gpu!(r);
    let q = quadrants("bloom-quad.png", 64, 64);
    let with = |effects: &str| project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.8}},"effects":[{effects}]}}]"#));
    let plain = render(r, &with(""), 0);
    // The brightest value is 1; with the knee the bloom starts at 1.2.
    let below = render(r, &with(r#"{"kind":"bloom","params":{"threshold":1.5,"knee":0.2,"intensity":3,"radius":20}}"#), 0);
    assert!(plain == below, "a bloom below its threshold changed the picture");
    // Lowering the threshold makes it glow.
    let above = render(r, &with(r#"{"kind":"bloom","params":{"threshold":0.5,"intensity":3,"radius":20}}"#), 0);
    assert!(energy(&above) > energy(&plain) * 1.05);
}

#[test]
fn bloom_energy_grows_with_intensity() {
    gpu!(r);
    let at = |r: &mut edits_render::Renderer, k: f64| {
        let p = sized(96, 96, &box_layer(0.15, [0.0, 0.0], &format!(r#"{{"kind":"bloom","params":{{"threshold":0.5,"intensity":{k},"radius":8}}}}"#)));
        energy(&render(r, &p, 0))
    };
    let e: Vec<f64> = [0.0, 0.25, 0.5, 1.0].iter().map(|k| at(r, *k)).collect();
    assert!(e.windows(2).all(|w| w[1] > w[0] * 1.01), "energy does not grow with intensity: {e:?}");
    // Light is added in proportion (outside the clipped box), so twice the
    // intensity adds about twice the light.
    let ratio = (e[2] - e[0]) / (e[1] - e[0]);
    assert!((1.6..2.4).contains(&ratio), "doubling the intensity added {ratio}x the light: {e:?}");
}

#[test]
fn bloom_looks_the_same_at_half_scale() {
    gpu!(r);
    // Two grey boxes (bright enough to bloom, dim enough not to clip), a
    // bloom on the small one and another on an adjustment layer over both.
    let scene = |k: f64| {
        let grey = |size: f64, position: [f64; 2], fx: &str| box_layer(size, position, fx).replace("[1,1,1,1]", "[0.8,0.8,0.8,1]");
        sized(
            256,
            144,
            &format!(
                r#"{}, {}, {{"id":"fx","kind":"adjustment","effects":[{{"kind":"bloom","params":{{"threshold":0.3,"intensity":{k},"radius":12,"color":[1,0.6,0.3]}}}}]}}"#,
                grey(0.25, [-0.3, 0.0], ""),
                grey(0.1, [0.4, 0.2], &format!(r#"{{"kind":"bloom","params":{{"threshold":0.3,"intensity":{k},"radius":5}}}}"#))
            ),
        )
    };
    let added = |r: &mut edits_render::Renderer, scale: f32| {
        let with = render_in(r, &scene(0.5), &dir(), 0, scale);
        let without = render_in(r, &scene(0.0), &dir(), 0, scale);
        (energy(&with) - energy(&without), with)
    };
    let (added_full, full) = added(r, 1.0);
    let (added_half, half) = added(r, 0.5);
    assert_eq!((half.width(), half.height()), (128, 72));
    let reduced = halve(&full);
    let mut diff = 0.0;
    for y in 0..72 {
        for x in 0..128 {
            let a = reduced[(y * 128 + x) as usize];
            for (i, v) in a.iter().enumerate() {
                diff += (v - lin(&half, x, y, i)).abs();
            }
        }
    }
    let mean = diff / (128.0 * 72.0 * 3.0);
    assert!(mean < 0.01, "mean difference {mean} between full and half scale");
    // The light the bloom adds, per pixel of a half-size frame.
    let ratio = added_half * 4.0 / added_full;
    assert!((ratio - 1.0).abs() < 0.05, "the bloom adds {ratio} times as much light at half scale");
    // And the bloom reaches well beyond the boxes.
    let far = lin(&full, 128, 72, 0);
    assert!(far > 0.002, "the bloom does not reach the middle of the frame: {far}");
}

#[test]
fn outline_width_in_pixels_is_width_percent_of_height() {
    gpu!(r);
    // A 200 x 200 frame; the box spans pixels 60..140.
    for (width, position) in [(5.0, "outside"), (2.0, "outside"), (5.0, "inside"), (6.0, "center")] {
        let fx = format!(r#"{{"kind":"outline","params":{{"width":{width},"color":[1,0,0]}},"options":{{"position":"{position}"}}}}"#);
        let img = render(r, &sized(200, 200, &box_layer(0.4, [0.0, 0.0], &fx)), 0);
        let red = |x: u32| {
            let p = img.get_pixel(x, 100).0;
            p[0] > 128 && p[1] < 128
        };
        let count = (0..200).filter(|&x| red(x)).count() as f64 / 2.0;
        let want = width / 100.0 * 200.0;
        assert!((count - want).abs() <= 1.0, "{position} outline of {width}%: {count} px on each side, expected {want}");
        // Where the stroke sits: outside starts at the box's edge, inside ends there.
        match position {
            "outside" => assert!(red(140) && !red(139), "{position}: the stroke should start at x = 140"),
            "inside" => assert!(red(139) && !red(140), "{position}: the stroke should end at x = 139"),
            _ => assert!(red(134) && red(145) && !red(133) && !red(146), "{position}: the stroke should straddle x = 140"),
        }
        // The box stays white inside the stroke.
        assert_px(&img, 100, 100, [255, 255, 255], 0);
    }
}

#[test]
fn drop_shadow_lands_at_its_offset() {
    gpu!(r);
    // A 200 x 200 frame; the box spans 60..140; the shadow moves 20 px right and 10 down.
    let fx = r#"{"kind":"drop_shadow","params":{"offset":[0.1,0.05],"softness":0,"color":[1,0,0],"opacity":1}}"#;
    let img = render(r, &sized(200, 200, &box_layer(0.4, [0.0, 0.0], fx)), 0);
    assert_px(&img, 100, 100, [255, 255, 255], 0);
    for (x, y) in [(150, 100), (159, 120), (100, 145), (85, 149)] {
        assert_px(&img, x, y, [255, 0, 0], 2);
    }
    for (x, y) in [(161, 100), (100, 151), (70, 145), (150, 65)] {
        assert_px(&img, x, y, [0, 0, 0], 2);
    }
    // Softness spreads it about its edge: half way at the edge itself.
    let soft = r#"{"kind":"drop_shadow","params":{"offset":[0.1,0.05],"softness":2,"color":[1,1,1],"opacity":1}}"#;
    let img = render(r, &sized(200, 200, &box_layer(0.4, [0.0, 0.0], soft)), 0);
    let edge = lin(&img, 160, 100, 0);
    assert!((0.35..0.65).contains(&edge), "a soft shadow at its edge is {edge}");
    assert!(lin(&img, 150, 100, 0) > 0.9 && lin(&img, 170, 100, 0) < 0.1);
}

#[test]
fn light_rays_stream_away_from_their_centre() {
    gpu!(r);
    // A small bright box right of the centre; rays from the centre.
    let rays = r#"{"kind":"light_rays","params":{"center":[0,0],"length":0.6,"intensity":2,"threshold":0.2}}"#;
    let plain = render(r, &sized(128, 128, &box_layer(0.1, [0.15, 0.0], "")), 0);
    let img = render(r, &sized(128, 128, &box_layer(0.1, [0.15, 0.0], rays)), 0);
    // Beyond the box (away from the centre) it is lit; between the centre and the box it is not.
    assert!(lin(&img, 100, 64, 0) > lin(&plain, 100, 64, 0) + 0.02, "no ray beyond the box");
    assert_eq!(px(&img, 70, 64), px(&plain, 70, 64), "light between the centre and the box");
    // Above and below the box's row there is nothing either.
    assert_eq!(px(&img, 100, 20), px(&plain, 100, 20));
    for mode in ["screen", "lighten"] {
        let fx = format!(r#"{{"kind":"light_rays","params":{{"length":0.6,"intensity":2,"threshold":0.2}},"options":{{"mode":"{mode}"}}}}"#);
        let img = render(r, &sized(128, 128, &box_layer(0.1, [0.15, 0.0], &fx)), 0);
        assert!(lin(&img, 100, 64, 0) > 0.02, "{mode}: no ray beyond the box");
    }
}

#[test]
fn lens_flare_is_brightest_at_its_light() {
    gpu!(r);
    let p = sized(160, 90, r#"{"id":"fx","kind":"adjustment","effects":[{"kind":"lens_flare","params":{"position":[-0.4,-0.2]}}]}"#);
    let img = render(r, &p, 0);
    // The light is at uv (0.5 - 0.4 / (16/9), 0.5 - 0.2) -> pixel (44, 27).
    assert!(lin(&img, 44, 27, 0) > 0.9, "the core is not bright");
    assert!(lin(&img, 44, 27, 0) > lin(&img, 150, 80, 0));
    for lens in ["prime35", "prime105"] {
        let p = sized(160, 90, &format!(r#"{{"id":"fx","kind":"adjustment","effects":[{{"kind":"lens_flare","options":{{"lens":"{lens}"}}}}]}}"#));
        assert!(render(r, &p, 0) != img, "{lens} looks like the zoom lens");
    }
}

#[test]
fn tilt_shift_keeps_its_band_sharp_and_blurs_away_from_it() {
    gpu!(r);
    // Vertical stripes, two pixels wide.
    let stripes = image::RgbaImage::from_fn(128, 128, |x, _| if (x / 2) % 2 == 0 { image::Rgba([255, 255, 255, 255]) } else { image::Rgba([0, 0, 0, 255]) });
    let s = save("stripes.png", &stripes);
    let layer = |fx: &str| project("", &format!(r#""slots":{{"a":{{"path":"{s}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","effects":[{fx}]}}]"#));
    let plain = render(r, &layer(""), 0);
    let img = render(r, &layer(r#"{"kind":"tilt_shift","params":{"width":20,"blur":6}}"#), 0);
    // The middle rows are untouched; the top and bottom rows are smoothed toward grey.
    for x in 0..64 {
        assert_eq!(px(&img, x, 32), px(&plain, x, 32), "the sharp band changed at x = {x}");
    }
    let contrast = |img: &RgbaImage, y: u32| (8..56).map(|x| lin(img, x, y, 0)).fold(0.0f64, f64::max) - (8..56).map(|x| lin(img, x, y, 0)).fold(1.0f64, f64::min);
    assert!(contrast(&img, 2) < 0.3 && contrast(&img, 61) < 0.3, "the edges are not blurred: {} {}", contrast(&img, 2), contrast(&img, 61));
    assert!(contrast(&plain, 2) > 0.9);
}

#[test]
fn light_effects_are_deterministic() {
    gpu!(r);
    let fx = [
        r#"{"kind":"bloom","params":{"threshold":0.3,"radius":10}}"#,
        r#"{"kind":"light_rays","params":{"threshold":0.3}}"#,
        r#"{"kind":"lens_flare"}"#,
        r#"{"kind":"outline","params":{"width":3,"softness":1}}"#,
        r#"{"kind":"drop_shadow","params":{"color":[1,0.5,0]}}"#,
        r#"{"kind":"tilt_shift","params":{"blur":30}}"#,
    ];
    let other = sized(96, 64, &box_layer(0.5, [0.0, 0.0], ""));
    for f in fx {
        let p = sized(96, 64, &box_layer(0.3, [0.2, 0.1], f));
        let a = render(r, &p, 0);
        render(r, &other, 0);
        let b = render(r, &p, 0);
        assert!(a == b, "{f} is not the same twice");
    }
}
