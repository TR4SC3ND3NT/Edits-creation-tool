//! Pixel tests of every effect in the registry. Every test skips when the
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

/// A full-frame solid of `color` (sRGB) with `effects` on it.
fn solid(color: [f64; 3], effects: &str) -> Project {
    project("", &format!(r#""layers":[{{"id":"s","kind":"solid","color":[{},{},{},1],"effects":[{effects}]}}]"#, color[0], color[1], color[2]))
}

/// A white box of `size` (fraction of the frame) at `position` on black, with
/// `effects` on an adjustment layer above it.
fn boxed(size: [f64; 2], position: [f64; 2], effects: &str) -> Project {
    project(
        "",
        &format!(
            r#""layers":[{{"id":"box","kind":"solid","color":[1,1,1,1],"transform":{{"scale":{size:?},"position":[{},{},0]}}}},
                         {{"id":"fx","kind":"adjustment","effects":[{effects}]}}]"#,
            position[0], position[1]
        ),
    )
}

fn lin(img: &RgbaImage, x: u32, y: u32) -> f64 {
    to_linear(img.get_pixel(x, y).0[0] as f64 / 255.0)
}

/// Standard normal cumulative distribution (Abramowitz and Stegun 7.1.26).
fn phi(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs() / std::f64::consts::SQRT_2);
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let erf = 1.0 - poly * (-x * x / 2.0).exp();
    if x >= 0.0 {
        0.5 * (1.0 + erf)
    } else {
        0.5 * (1.0 - erf)
    }
}

#[test]
fn effects_at_zero_strength_change_nothing() {
    gpu!(r);
    let q = quadrants("quad.png", 64, 64);
    let with = |effects: &str| project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.8}},"effects":[{effects}]}}]"#));
    let plain = render(r, &with(""), 0);
    let neutral = r#"
        {"kind":"exposure"}, {"kind":"fade"}, {"kind":"brightness_contrast"}, {"kind":"saturation"}, {"kind":"hue"},
        {"kind":"invert","params":{"amount":0}}, {"kind":"threshold","params":{"amount":0}}, {"kind":"tint","params":{"amount":0}},
        {"kind":"gaussian_blur","params":{"radius":0}}, {"kind":"directional_blur","params":{"length":0}},
        {"kind":"zoom_blur","params":{"strength":0}}, {"kind":"spin_blur","params":{"angle":0}},
        {"kind":"glow","params":{"intensity":0}}, {"kind":"vignette","params":{"amount":0}}, {"kind":"rgb_split","params":{"amount":0}},
        {"kind":"transform2d"}, {"kind":"grain","params":{"amount":0}},
        {"kind":"twirl","params":{"angle":0}}, {"kind":"bulge","params":{"amount":0}}, {"kind":"pixelate","params":{"size":0}},
        {"kind":"halftone","params":{"amount":0}}, {"kind":"chromatic_aberration","params":{"amount":0}}"#;
    assert!(plain == render(r, &with(neutral), 0), "neutral effects changed the picture");
    // The same on an adjustment layer.
    let adjusted = project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.8}}}},{{"id":"fx","kind":"adjustment","effects":[{neutral}]}}]"#));
    assert!(plain == render(r, &adjusted, 0), "neutral effects on an adjustment layer changed the picture");
}

#[test]
fn every_registered_effect_renders_and_changes_the_picture() {
    gpu!(r);
    let shader = dir().join("registry.wgsl");
    std::fs::write(&shader, "fn effect(uv: vec2<f32>) -> vec4<f32> { return src(vec2<f32>(1.0 - uv.x, uv.y)); }").unwrap();
    // A lookup table that swaps red and blue.
    std::fs::write(dir().join("registry.cube"), "LUT_3D_SIZE 2\n0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n").unwrap();
    let q = quadrants("quad.png", 64, 64);
    let with = |effect: &str| project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.8}},"effects":[{effect}]}}]"#));
    let plain = render(r, &with(""), 0);
    for spec in edits_core::effects::EFFECTS {
        let params = match spec.kind {
            "exposure" => r#"{"stops":-1}"#,
            "fade" => r#"{"amount":0.5,"color":[0,0,1]}"#,
            "brightness_contrast" => r#"{"brightness":-0.2,"contrast":0.3}"#,
            "saturation" => r#"{"amount":0.3}"#,
            "hue" => r#"{"degrees":90}"#,
            "invert" => r#"{}"#,
            "threshold" => r#"{"level":0.6}"#,
            "posterize" => r#"{"levels":2}"#,
            "tint" => r#"{"color":[1,0.5,0]}"#,
            "gaussian_blur" => r#"{"radius":4}"#,
            "directional_blur" => r#"{"angle":30,"length":10}"#,
            "zoom_blur" => r#"{"strength":0.3}"#,
            "spin_blur" => r#"{"angle":20}"#,
            "glow" => r#"{"threshold":0.1,"radius":5}"#,
            "vignette" => r#"{"amount":1,"radius":0.2}"#,
            "rgb_split" => r#"{"amount":4}"#,
            "transform2d" => r#"{"scale":1.3,"rotation":10}"#,
            "grain" => r#"{"amount":0.3}"#,
            "custom" => r#"{"p0":1}"#,
            "twirl" => r#"{"angle":120}"#,
            "bulge" => r#"{"amount":0.8}"#,
            "kaleidoscope" => r#"{"segments":5}"#,
            "pixelate" => r#"{"size":10}"#,
            "halftone" => r#"{"size":8}"#,
            "chromatic_aberration" => r#"{"amount":10}"#,
            "wave_warp" => r#"{"amplitude":5}"#,
            "ripple" => r#"{"amplitude":3,"wavelength":10}"#,
            "shockwave" => r#"{"progress":0.3,"strength":8}"#,
            "mirror" => r#"{}"#,
            "tile" => r#"{}"#,
            "polar" => r#"{}"#,
            "lens_distortion" => r#"{"amount":0.5}"#,
            "displacement_map" => r#"{"amount":[10,10]}"#,
            "turbulent_displace" => r#"{"amount":5}"#,
            "screentone" => r#"{"size":3}"#,
            "edge_detect" => r#"{}"#,
            "ink" => r#"{}"#,
            "toon" => r#"{"levels":3}"#,
            "kuwahara" => r#"{"radius":3}"#,
            "emboss" => r#"{}"#,
            "sharpen" => r#"{"amount":1}"#,
            "glitch" => r#"{"amount":1}"#,
            "scanlines" => r#"{"count":16,"amount":0.8}"#,
            "vhs" => r#"{"noise":0.5,"bleed":3}"#,
            "film_damage" => r#"{"scratches":1,"dust":1,"flicker":0.5}"#,
            "film_burn" => r#"{"amount":0.6}"#,
            "lut" => r#"{}"#,
            "gradient_map" => r#"{"shadows":[0,0,0.3],"highlights":[1,0.9,0.5]}"#,
            "levels" => r#"{"gamma":2}"#,
            "vibrance" => r#"{"amount":-0.5}"#,
            "fractal_noise" => r#"{"amount":0.5}"#,
            "gradient_ramp" => r#"{"amount":0.5}"#,
            other => panic!("the registry has an effect this test does not cover: {other}"),
        };
        let source = match spec.kind {
            "custom" => r#","source":"registry.wgsl""#,
            "lut" => r#","source":"registry.cube""#,
            "displacement_map" => r#","source":"slot:a""#,
            _ => "",
        };
        // Sharpening leaves hard 0-to-1 edges alone (no halo): give it a soft picture.
        let (before, base) = match spec.kind {
            "sharpen" => (r#"{"kind":"gaussian_blur","params":{"radius":1}},"#, render(r, &with(r#"{"kind":"gaussian_blur","params":{"radius":1}}"#), 0)),
            _ => ("", plain.clone()),
        };
        let img = render(r, &with(&format!(r#"{before}{{"kind":"{}","params":{params}{source}}}"#, spec.kind)), 0);
        assert!(img != base, "{} did not change the picture", spec.kind);
        // Defaults must render too.
        if spec.kind != "custom" {
            render(r, &with(&format!(r#"{{"kind":"{}"{source}}}"#, spec.kind)), 0);
        }
    }
}

#[test]
fn fade_brightness_contrast_hue_posterize_tint() {
    gpu!(r);
    let c = [0.8, 0.4, 0.2];
    // fade: a linear mix toward the (linear) colour.
    let img = render(r, &solid(c, r#"{"kind":"fade","params":{"amount":1,"color":[0.5,0.25,1]}}"#), 0);
    assert_px(&img, 32, 32, [byte(0.5), byte(0.25), 255], 1);
    let img = render(r, &solid(c, r#"{"kind":"fade","params":{"amount":0.5,"color":[1,1,1]}}"#), 0);
    assert_px(&img, 32, 32, c.map(|v| byte(0.5 * to_linear(v) + 0.5)), 1);
    // brightness adds to the display value; contrast scales about mid grey.
    let img = render(r, &solid([0.4, 0.4, 0.4], r#"{"kind":"brightness_contrast","params":{"brightness":0.2}}"#), 0);
    assert_px(&img, 32, 32, [153; 3], 1);
    let img = render(r, &solid(c, r#"{"kind":"brightness_contrast","params":{"contrast":-1}}"#), 0);
    assert_px(&img, 32, 32, [128; 3], 1);
    let img = render(r, &solid([0.6, 0.5, 0.4], r#"{"kind":"brightness_contrast","params":{"contrast":0.5}}"#), 0);
    let gain = (1.5f64 * std::f64::consts::FRAC_PI_4).tan();
    assert_px(&img, 32, 32, [0.6, 0.5, 0.4].map(|v: f64| (((v - 0.5) * gain + 0.5) * 255.0).round() as u8), 1);
    // hue: a third of a turn takes red to green to blue.
    let img = render(r, &solid([1.0, 0.0, 0.0], r#"{"kind":"hue","params":{"degrees":120}}"#), 0);
    assert_px(&img, 32, 32, [0, 255, 0], 2);
    let img = render(r, &solid([1.0, 0.0, 0.0], r#"{"kind":"hue","params":{"degrees":-120}}"#), 0);
    assert_px(&img, 32, 32, [0, 0, 255], 2);
    let img = render(r, &solid(c, r#"{"kind":"hue","params":{"degrees":360}}"#), 0);
    assert_px(&img, 32, 32, [204, 102, 51], 1);
    // posterize: evenly spaced display levels.
    let ramp = save("ramp.png", &RgbaImage::from_fn(64, 64, |x, _| image::Rgba([(x * 4) as u8, (x * 4) as u8, (x * 4) as u8, 255])));
    let p = project("", &format!(r#""slots":{{"a":{{"path":"{ramp}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","effects":[{{"kind":"posterize","params":{{"levels":4}}}}]}}]"#));
    let img = render(r, &p, 0);
    let mut seen: Vec<u8> = img.pixels().map(|p| p.0[0]).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen, vec![0, 85, 170, 255]);
    // tint: luminance times the colour.
    let img = render(r, &solid(c, r#"{"kind":"tint","params":{"color":[1,0,0.5]}}"#), 0);
    let l = 0.2126 * to_linear(0.8) + 0.7152 * to_linear(0.4) + 0.0722 * to_linear(0.2);
    assert_px(&img, 32, 32, [byte(l), 0, byte(0.5 * l)], 1);
}

#[test]
fn colour_effects_run_in_order_and_in_long_chains() {
    gpu!(r);
    let c = [0.3, 0.3, 0.3];
    let exposure = r#"{"kind":"exposure","params":{"stops":1}}"#;
    let a = render(r, &solid(c, &format!(r#"{exposure},{{"kind":"invert"}}"#)), 0);
    let b = render(r, &solid(c, &format!(r#"{{"kind":"invert"}},{exposure}"#)), 0);
    let twice = to_srgb(2.0 * to_linear(0.3));
    assert_px(&a, 32, 32, [((1.0 - twice) * 255.0).round() as u8; 3], 1);
    assert_px(&b, 32, 32, [byte(2.0 * to_linear(0.7)); 3], 1);
    // Fifteen colour effects do not fit one pass: +1 stop eight times, -1 stop seven times.
    let chain: Vec<String> = (0..15).map(|i| format!(r#"{{"kind":"exposure","params":{{"stops":{}}}}}"#, if i % 2 == 0 { 1 } else { -1 })).collect();
    let img = render(r, &solid(c, &chain.join(",")), 0);
    assert_px(&img, 32, 32, [byte(2.0 * to_linear(0.3)); 3], 1);
    // A blur between colour effects splits them into separate passes, in order.
    let img = render(r, &solid(c, &format!(r#"{exposure},{{"kind":"gaussian_blur","params":{{"radius":2}}}},{{"kind":"invert"}}"#)), 0);
    assert_px(&img, 32, 32, [((1.0 - twice) * 255.0).round() as u8; 3], 1);
}

#[test]
fn gaussian_blur_has_the_documented_sigma_at_any_size() {
    gpu!(r);
    // The left half of the frame is white. Blurred, the edge becomes the
    // normal distribution's S-curve: half at the edge, about 16% one sigma out.
    for radius in [2.0, 3.5, 8.0, 14.0] {
        let p = boxed([0.5, 1.0], [-0.25, 0.0], &format!(r#"{{"kind":"gaussian_blur","params":{{"radius":{radius}}}}}"#));
        for scale in [1.0f32, 2.0] {
            let img = render_in(r, &p, &dir(), 0, scale);
            let (w, sigma) = (img.width(), radius / 100.0 * img.height() as f64);
            for x in (0..w).step_by(scale as usize) {
                let distance = x as f64 + 0.5 - w as f64 / 2.0;
                let (want, got) = (phi(-distance / sigma), lin(&img, x, img.height() / 2));
                assert!((want - got).abs() < 0.035, "radius {radius}% at scale {scale}: x = {x} is {got:.3}, expected {want:.3}");
            }
        }
    }
    // A blurred box keeps its light and stays symmetric.
    let plain = render(r, &boxed([0.25, 0.25], [0.0, 0.0], ""), 0);
    for radius in [3.0, 6.0] {
        let img = render(r, &boxed([0.25, 0.25], [0.0, 0.0], &format!(r#"{{"kind":"gaussian_blur","params":{{"radius":{radius}}}}}"#)), 0);
        let (a, b) = (energy(&plain), energy(&img));
        assert!((a - b).abs() / a < 0.04, "radius {radius}: energy {a} became {b}");
        for d in 1..20 {
            let (left, right, up) = (lin(&img, 31 - d, 31), lin(&img, 32 + d, 32), lin(&img, 31, 31 - d));
            assert!((left - right).abs() < 0.02 && (left - up).abs() < 0.02, "radius {radius}: not symmetric {d} px out: {left} {right} {up}");
        }
        assert!(lin(&img, 32, 20) > 0.01 && (radius < 6.0 || lin(&img, 32, 32) < 0.99));
    }
}

#[test]
fn streak_blurs_follow_their_direction() {
    gpu!(r);
    // A 4 px box a quarter of the frame right of the centre: x 46..50, y 30..34.
    let dot = |effects: &str| boxed([0.0625, 0.0625], [0.25, 0.0], effects);
    let lit = |img: &RgbaImage, x, y| img.get_pixel(x, y).0[0] > 12;
    let plain = render(r, &dot(""), 0);
    assert!(lit(&plain, 48, 32) && !lit(&plain, 54, 32) && !lit(&plain, 48, 38));

    // directional: 25% of the height = 16 px, centred on each pixel.
    let img = render(r, &dot(r#"{"kind":"directional_blur","params":{"angle":0,"length":25}}"#), 0);
    assert!(lit(&img, 40, 32) && lit(&img, 56, 32) && !lit(&img, 36, 32) && !lit(&img, 60, 32) && !lit(&img, 48, 37));
    let (a, b) = (energy(&plain), energy(&img));
    assert!((a - b).abs() / a < 0.05, "energy {a} became {b}");
    // A box filter: the middle of the streak carries 4/16 of the light.
    assert!((lin(&img, 48, 32) - 0.25).abs() < 0.04, "{}", lin(&img, 48, 32));
    let img = render(r, &dot(r#"{"kind":"directional_blur","params":{"angle":90,"length":25}}"#), 0);
    assert!(lit(&img, 48, 24) && lit(&img, 48, 40) && !lit(&img, 55, 32) && !lit(&img, 41, 32));
    let img = render(r, &dot(r#"{"kind":"directional_blur","params":{"angle":45,"length":25}}"#), 0);
    assert!(lit(&img, 52, 36) && lit(&img, 44, 28) && !lit(&img, 52, 28) && !lit(&img, 44, 36));
    // A long streak takes the three-pass path and still conserves light.
    let img = render_in(r, &boxed([0.0625, 0.0625], [0.0, 0.0], r#"{"kind":"directional_blur","params":{"length":60}}"#), &dir(), 0, 6.0);
    let plain_big = render_in(r, &boxed([0.0625, 0.0625], [0.0, 0.0], ""), &dir(), 0, 6.0);
    let (a, b) = (energy(&plain_big), energy(&img));
    assert!((a - b).abs() / a < 0.05, "energy {a} became {b}");
    let row: Vec<f64> = (90..294).map(|x| lin(&img, x, 192)).collect();
    let (lo, hi) = row.iter().fold((1.0f64, 0.0f64), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    assert!(hi - lo < 0.02 && (lo - 24.0 / 230.4).abs() < 0.02, "the streak is uneven: {lo}..{hi}");

    // zoom: along the ray from the centre, not across it.
    let img = render(r, &dot(r#"{"kind":"zoom_blur","params":{"strength":0.5}}"#), 0);
    assert!(lit(&img, 44, 32) && lit(&img, 53, 32) && !lit(&img, 48, 37) && !lit(&img, 48, 26) && !lit(&img, 32, 32));
    // ... about another centre: the box itself. Nothing moves.
    let img = render(r, &dot(r#"{"kind":"zoom_blur","params":{"strength":0.5,"center":[0.25,0]}}"#), 0);
    assert!(lit(&img, 48, 32) && !lit(&img, 53, 32) && !lit(&img, 43, 32));

    // spin: along the circle about the centre, not along the ray.
    let img = render(r, &dot(r#"{"kind":"spin_blur","params":{"angle":40}}"#), 0);
    assert!(lit(&img, 47, 36) && lit(&img, 47, 27) && !lit(&img, 56, 32) && !lit(&img, 40, 32));
}

#[test]
fn glow_adds_tinted_light_around_bright_areas_only() {
    gpu!(r);
    let glow = r#"{"kind":"glow","params":{"threshold":0.5,"radius":4,"intensity":1,"color":[1,0,0]}}"#;
    let plain = render(r, &boxed([0.25, 0.25], [0.0, 0.0], ""), 0);
    let img = render(r, &boxed([0.25, 0.25], [0.0, 0.0], glow), 0);
    // Outside the box: red light only, fading with distance.
    let near = px(&img, 43, 32);
    let far = px(&img, 50, 32);
    assert!(near[0] > 45 && near[1] <= 1 && near[2] <= 1, "{near:?}");
    assert!(far[0] < near[0] && px(&plain, 43, 32) == [0, 0, 0]);
    assert_px(&img, 32, 32, [255, 255, 255], 0);
    // A box below the threshold does not glow at all.
    let dim = |effects: &str| {
        project("", &format!(r#""layers":[{{"id":"box","kind":"solid","color":[0.5,0.5,0.5,1],"transform":{{"scale":0.25}}}},{{"id":"fx","kind":"adjustment","effects":[{effects}]}}]"#))
    };
    assert!(render(r, &dim(""), 0) == render(r, &dim(glow), 0), "a dim box glowed");
    // On a layer, the halo reaches outside the layer's own shape and survives later colour effects.
    let layer = project(
        r#","background":[0,0,0.5]"#,
        &format!(r#""layers":[{{"id":"box","kind":"solid","color":[1,1,1,1],"transform":{{"scale":0.25}},"effects":[{glow},{{"kind":"exposure","params":{{"stops":0.5}}}}]}}]"#),
    );
    let img = render(r, &layer, 0);
    let halo = px(&img, 43, 32);
    assert!(halo[0] > near[0] && halo[1] <= 1, "{halo:?} should be brighter than {near:?}");
    assert_px(&img, 4, 4, [0, 0, 128], 1);
}

#[test]
fn vignette_darkens_toward_the_corners() {
    gpu!(r);
    let img = render(r, &solid([1.0, 1.0, 1.0], r#"{"kind":"vignette","params":{"amount":0.5,"radius":0.5,"softness":0.5}}"#), 0);
    assert_px(&img, 32, 32, [255; 3], 0);
    assert_px(&img, 32, 12, [255; 3], 0); // inside the radius
    let (corner, edge) = (lin(&img, 0, 0), lin(&img, 0, 32));
    // The corner is the full distance out: amount 0.5 leaves half the light.
    assert!((corner - 0.5).abs() < 0.03, "{corner}");
    assert!(edge > corner && edge < 1.0, "{edge}");
    assert_eq!(px(&img, 0, 0), px(&img, 63, 63));
    assert_eq!(px(&img, 5, 20), px(&img, 58, 43));
    let img = render(r, &solid([1.0, 1.0, 1.0], r#"{"kind":"vignette","params":{"amount":1,"radius":0.2,"softness":0.3}}"#), 0);
    assert_px(&img, 1, 1, [0; 3], 1);
}

#[test]
fn rgb_split_moves_red_and_blue_apart() {
    gpu!(r);
    // A white bar from x = 24 to 40; 6.25% of 64 px is a 4 px shift.
    let bar = |angle: f64| boxed([0.25, 1.0], [0.0, 0.0], &format!(r#"{{"kind":"rgb_split","params":{{"amount":6.25,"angle":{angle}}}}}"#));
    let img = render(r, &bar(0.0), 0);
    assert_px(&img, 32, 32, [255, 255, 255], 1);
    assert_px(&img, 21, 32, [255, 0, 0], 1);
    assert_px(&img, 42, 32, [0, 0, 255], 1);
    assert_px(&img, 25, 32, [255, 255, 0], 1);
    assert_px(&img, 38, 32, [0, 255, 255], 1);
    assert_px(&img, 17, 32, [0, 0, 0], 1);
    // Turned a quarter: a vertical bar is unchanged by a vertical shift.
    assert!(render(r, &bar(90.0), 0) == render(r, &boxed([0.25, 1.0], [0.0, 0.0], ""), 0), "a vertical shift changed a vertical bar");
}

#[test]
fn transform2d_moves_zooms_turns_and_mirrors() {
    gpu!(r);
    let q = quadrants("quad.png", 64, 64);
    let fx = |params: &str| {
        project(
            r#","background":[0.5,0.5,0.5]"#,
            &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a"}},{{"id":"fx","kind":"adjustment","effects":[{{"kind":"transform2d","params":{params}}}]}}]"#),
        )
    };
    let (red, green, blue, yellow) = ([255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]);
    // offset: a quarter of the height to the right and an eighth down.
    let img = render(r, &fx(r#"{"offset":[0.25,0.125],"mirror":0}"#), 0);
    assert_px(&img, 40, 30, red, 1);
    assert_px(&img, 56, 30, green, 1);
    assert_px(&img, 40, 48, blue, 1);
    // Uncovered parts are transparent: the adjustment's result has no background there.
    assert_px(&img, 8, 30, [0, 0, 0], 1);
    // mirror: the uncovered parts repeat the picture, mirrored.
    let img = render(r, &fx(r#"{"offset":[0.25,0.125]}"#), 0);
    assert_px(&img, 8, 30, red, 1);
    let img = render(r, &fx(r#"{"offset":[0.75,0]}"#), 0);
    assert_px(&img, 8, 8, green, 1); // 0.62 of a frame past the left edge, folded back
    assert_px(&img, 40, 8, red, 1); // 0.12 past it
    assert_px(&img, 56, 56, blue, 1); // the picture itself, moved right
    // scale 2 zooms about the centre: each quadrant fills its own quarter still.
    let img = render(r, &fx(r#"{"scale":2}"#), 0);
    assert_px(&img, 4, 4, red, 1);
    assert_px(&img, 30, 30, red, 1);
    assert_px(&img, 34, 34, yellow, 1);
    // scale 0.5 without mirror: a half-size picture in the middle.
    let img = render(r, &fx(r#"{"scale":0.5,"mirror":0}"#), 0);
    assert_px(&img, 20, 20, red, 1);
    assert_px(&img, 44, 44, yellow, 1);
    assert_px(&img, 8, 8, [0, 0, 0], 1);
    // rotation: clockwise, like a layer's z rotation.
    let img = render(r, &fx(r#"{"rotation":90}"#), 0);
    assert_px(&img, 8, 8, blue, 1);
    assert_px(&img, 56, 8, red, 1);
    assert_px(&img, 56, 56, green, 1);
}

#[test]
fn grain_is_deterministic_per_frame() {
    gpu!(r);
    let p = solid([0.5, 0.5, 0.5], r#"{"kind":"grain","params":{"amount":0.2}}"#);
    let a = render(r, &p, 3);
    let b = render(r, &p, 3);
    let c = render(r, &p, 4);
    assert!(a == b, "the same frame's grain differs between renders");
    assert!(a != c, "grain does not change from frame to frame");
    let values: Vec<f64> = a.pixels().map(|p| p.0[0] as f64).collect();
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let spread = (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt();
    // Display-referred and centred: the mean stays at mid grey; triangular noise of amount 0.2 has sigma 0.2 / sqrt(6).
    assert!((mean - 128.0).abs() < 2.5, "{mean}");
    assert!((spread - 0.2 * 255.0 / 6f64.sqrt()).abs() < 3.0, "{spread}");
    // Grey grain: the three channels move together.
    assert!(a.pixels().all(|p| p.0[0] == p.0[1] && p.0[1] == p.0[2]));
    // `size` is in pixels at 1080: at this 64 px height, size 67.5 makes 4 px cells.
    let coarse = render(r, &solid([0.5, 0.5, 0.5], r#"{"kind":"grain","params":{"amount":0.2,"size":67.5}}"#), 0);
    assert_eq!(coarse.get_pixel(0, 0), coarse.get_pixel(3, 3));
    assert_eq!(coarse.get_pixel(8, 4), coarse.get_pixel(11, 7));
    assert!((0..16).any(|i| coarse.get_pixel(i * 4, 0) != coarse.get_pixel((i * 4 + 4) % 64, 0)));
}

#[test]
fn adjustment_layers_affect_only_what_is_beneath() {
    gpu!(r);
    let p = |adjust: &str| {
        project(
            r#","background":[0.2,0.2,0.2]"#,
            &format!(
                r#""layers":[{{"id":"low","kind":"solid","color":[1,0,0,1],"transform":{{"scale":0.5}}}},
                             {{"id":"fx","kind":"adjustment","effects":[{{"kind":"invert"}}]{adjust}}},
                             {{"id":"high","kind":"solid","color":[1,0,0,1],"transform":{{"scale":0.125}}}}]"#
            ),
        )
    };
    let img = render(r, &p(""), 0);
    assert_px(&img, 4, 4, [204; 3], 1); // background, inverted
    assert_px(&img, 20, 20, [0, 255, 255], 1); // lower layer, inverted
    assert_px(&img, 32, 32, [255, 0, 0], 1); // upper layer, untouched
    // An adjustment layer's opacity mixes the adjusted picture with the original.
    let img = render(r, &p(r#","transform":{"opacity":0.5}"#), 0);
    assert_px(&img, 4, 4, [byte(0.5 * to_linear(0.2) + 0.5 * to_linear(0.8)); 3], 1);
    // Its in and out apply too.
    let img = render(r, &p(r#","in":1"#), 0);
    assert_px(&img, 20, 20, [255, 0, 0], 1);
    // A layer's own effects stop at the layer's edges.
    let own = project(r#","background":[0.2,0.2,0.2]"#, r#""layers":[{"id":"low","kind":"solid","color":[1,0,0,1],"transform":{"scale":0.5},"effects":[{"kind":"invert"}]}]"#);
    let img = render(r, &own, 0);
    assert_px(&img, 4, 4, [51; 3], 1);
    assert_px(&img, 20, 20, [0, 255, 255], 1);
}

#[test]
fn custom_shaders_get_the_documented_inputs() {
    gpu!(r);
    let d = dir();
    std::fs::write(d.join("half.wgsl"), "fn effect(uv: vec2<f32>) -> vec4<f32> {\n    let c = src(uv);\n    return vec4<f32>(c.rgb * fx.p0 + vec3<f32>(fx.p7), c.a);\n}\n").unwrap();
    std::fs::write(
        d.join("inputs.wgsl"),
        "fn effect(uv: vec2<f32>) -> vec4<f32> {\n    if uv.x < 0.5 {\n        return vec4<f32>(fx.time / 4.0, fx.beats / 8.0, fx.frame / 40.0, 1.0);\n    }\n    return vec4<f32>(fx.resolution.x / 128.0, fx.aspect / 2.0, uv.y, 1.0);\n}\n",
    )
    .unwrap();
    let custom = |file: &str, params: &str| {
        project(
            r#","time_base":"beats","bpm":120"#,
            &format!(r#""layers":[{{"id":"s","kind":"solid","color":[0.5,0.5,0.5,1],"effects":[{{"kind":"custom","source":"{file}","params":{params}}}]}}]"#),
        )
    };
    let half = to_linear(0.5);
    let img = render(r, &custom("half.wgsl", r#"{"p0":0.5}"#), 0);
    assert_px(&img, 32, 32, [byte(0.5 * half); 3], 1);
    let img = render(r, &custom("half.wgsl", r#"{"p0":0.5,"p7":0.25}"#), 0);
    assert_px(&img, 32, 32, [byte(0.5 * half + 0.25); 3], 1);
    // Frame 15 at 10 fps and 120 BPM: 1.5 s, beat 3.
    let img = render(r, &custom("inputs.wgsl", "{}"), 15);
    assert_px(&img, 10, 32, [byte(1.5 / 4.0), byte(3.0 / 8.0), byte(15.0 / 40.0)], 1);
    assert_px(&img, 50, 16, [byte(0.5), byte(0.5), byte(16.5 / 64.0)], 1);
    // Parameters animate like any other.
    let img = render(r, &custom("half.wgsl", r#"{"p0":{"keys":[{"t":0,"v":0},{"t":4,"v":1}]}}"#), 10);
    assert_px(&img, 32, 32, [byte(0.5 * half); 3], 1);
    // Time-dependent shaders are sampled across the shutter, not once per frame.
    let blurred = project(
        r#","motion_blur":{"samples":4,"shutter":1}"#,
        r#""layers":[{"id":"s","kind":"solid","color":[0.5,0.5,0.5,1],"effects":[{"kind":"custom","source":"inputs.wgsl"}]}]"#,
    );
    let img = render(r, &blurred, 10);
    let mean_time = (1.0 + 1.025 + 1.05 + 1.075) / 4.0;
    assert_px(&img, 10, 32, [byte(mean_time / 4.0), 0, byte(10.0 / 40.0)], 1);
}

#[test]
fn custom_shader_errors_name_the_file_and_the_problem() {
    gpu!(r);
    let d = dir();
    std::fs::write(d.join("broken.wgsl"), "fn effect(uv: vec2<f32>) -> vec4<f32> {\n    return src(uv) * undefined_thing;\n}\n").unwrap();
    std::fs::write(d.join("no-entry.wgsl"), "fn something_else(uv: vec2<f32>) -> vec4<f32> { return src(uv); }\n").unwrap();
    std::fs::write(d.join("wrong-type.wgsl"), "fn effect(uv: vec2<f32>) -> vec3<f32> { return src(uv).rgb; }\n").unwrap();
    let custom = |file: &str| project("", &format!(r#""layers":[{{"id":"shady","kind":"solid","color":[1,1,1,1],"effects":[{{"kind":"custom","source":"{file}"}}]}}]"#));
    for (file, needle) in [("broken.wgsl", "undefined_thing"), ("no-entry.wgsl", "effect"), ("wrong-type.wgsl", "edits_fs")] {
        let err = format!("{:#}", r.prepare(&custom(file), &d).unwrap_err());
        assert!(err.contains(file) && err.contains(needle) && err.contains("shady"), "{file}: {err}");
        assert!(err.contains("fn effect(uv: vec2<f32>) -> vec4<f32>"), "{file}: {err}");
    }
    let err = format!("{:#}", r.prepare(&custom("missing.wgsl"), &d).unwrap_err());
    assert!(err.contains("missing.wgsl") && err.contains("cannot read"), "{err}");
    // A failed prepare leaves the renderer usable.
    let img = render(r, &solid([1.0, 1.0, 1.0], ""), 0);
    assert_px(&img, 1, 1, [255; 3], 0);
}

#[test]
fn the_example_shaders_compile_and_run() {
    gpu!(r);
    let shaders = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/shaders"));
    let mut count = 0;
    for entry in std::fs::read_dir(shaders).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if !name.ends_with(".wgsl") {
            continue;
        }
        let p = project(
            "",
            &format!(r#""slots":{{"a":{{}}}},"layers":[{{"id":"a","kind":"image","slot":"a","effects":[{{"kind":"custom","source":"{name}","params":{{"p0":0.5,"p1":0.5}}}}]}}]"#),
        );
        r.prepare(&p, shaders).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let with = r.render_frame(3, 1.0).unwrap();
        let plain = project("", r#""slots":{"a":{}},"layers":[{"id":"a","kind":"image","slot":"a"}]"#);
        r.prepare(&plain, shaders).unwrap();
        assert!(with != r.render_frame(3, 1.0).unwrap(), "{name} did not change the picture");
        count += 1;
    }
    assert!(count >= 1, "examples/shaders has no .wgsl files");
}

#[test]
fn effect_sizes_scale_with_the_output() {
    gpu!(r);
    // The same project at twice the size, shrunk back, matches the small render.
    let q = quadrants("quad.png", 64, 64);
    let fx = r#"{"kind":"gaussian_blur","params":{"radius":3}},{"kind":"rgb_split","params":{"amount":3}},
                {"kind":"directional_blur","params":{"length":8,"angle":20}},{"kind":"glow","params":{"threshold":0.3,"radius":5}},
                {"kind":"zoom_blur","params":{"strength":0.2}},{"kind":"vignette"}"#;
    let p = project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.7}},"effects":[{fx}]}}]"#));
    let small = render_in(r, &p, &dir(), 0, 1.0);
    let big = render_in(r, &p, &dir(), 0, 2.0);
    let mut worst = 0.0f64;
    for y in 0..64 {
        for x in 0..64 {
            for c in 0..3 {
                let mean: f64 = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|(dx, dy)| to_linear(big.get_pixel(2 * x + dx, 2 * y + dy).0[c] as f64 / 255.0)).sum::<f64>() / 4.0;
                worst = worst.max((mean - to_linear(small.get_pixel(x, y).0[c] as f64 / 255.0)).abs());
            }
        }
    }
    assert!(worst < 0.08, "the two sizes differ by up to {worst}");
}

#[test]
fn extreme_values_render_without_errors() {
    gpu!(r);
    let q = quadrants("quad.png", 64, 64);
    let effects = [
        r#"{"kind":"gaussian_blur","params":{"radius":300}}"#,
        r#"{"kind":"gaussian_blur","params":{"radius":-5}}"#,
        r#"{"kind":"directional_blur","params":{"length":500,"angle":1e6}}"#,
        r#"{"kind":"zoom_blur","params":{"strength":5,"center":[9,9]}}"#,
        r#"{"kind":"zoom_blur","params":{"strength":-2}}"#,
        r#"{"kind":"spin_blur","params":{"angle":720}}"#,
        r#"{"kind":"glow","params":{"radius":0,"threshold":-1,"intensity":50}}"#,
        r#"{"kind":"glow","params":{"radius":200}}"#,
        r#"{"kind":"vignette","params":{"amount":5,"radius":-1,"softness":0}}"#,
        r#"{"kind":"rgb_split","params":{"amount":200}}"#,
        r#"{"kind":"transform2d","params":{"scale":0}}"#,
        r#"{"kind":"transform2d","params":{"scale":-1,"rotation":1e7,"offset":[50,-50]}}"#,
        r#"{"kind":"transform2d","params":{"scale":1e6,"mirror":0}}"#,
        r#"{"kind":"grain","params":{"amount":9,"size":0}}"#,
        r#"{"kind":"posterize","params":{"levels":1}}"#,
        r#"{"kind":"posterize","params":{"levels":100000}}"#,
        r#"{"kind":"brightness_contrast","params":{"brightness":-9,"contrast":5}}"#,
        r#"{"kind":"fade","params":{"amount":7,"color":[-1,2,300]}}"#,
        r#"{"kind":"exposure","params":{"stops":200}}"#,
        r#"{"kind":"exposure","params":{"stops":-200}}"#,
        r#"{"kind":"threshold","params":{"level":-3,"softness":-1}}"#,
        r#"{"kind":"saturation","params":{"amount":-4}}"#,
        r#"{"kind":"invert","params":{"amount":40}}"#,
    ];
    for fx in effects {
        for kind in [r#""kind":"image","slot":"a","transform":{"scale":0.7}"#, r#""kind":"adjustment""#] {
            let p = project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"base","kind":"image","slot":"a"}},{{"id":"x",{kind},"effects":[{fx}]}}]"#));
            r.prepare(&p, &dir()).unwrap();
            let img = r.render_frame(0, 1.0).unwrap_or_else(|e| panic!("{fx}: {e:#}"));
            assert_eq!(img.dimensions(), (64, 64), "{fx}");
            assert!(img.pixels().all(|p| p.0[3] == 255), "{fx}");
        }
    }
    let transforms = [
        r#""scale":0"#,
        r#""scale":[1e6,1e-6]"#,
        r#""rotation":[1e6,-1e6,12345.6]"#,
        r#""rotation":[0,90,0]"#,
        r#""position":[0,0,-1.4]"#,
        r#""position":[0,0,-50]"#,
        r#""position":[1e6,1e6,1e6]"#,
        r#""opacity":3"#,
        r#""opacity":-1"#,
        r#""anchor":[100,100]"#,
    ];
    for tr in transforms {
        for camera in [r#"{}"#, r#"{"fov":1}"#, r#"{"fov":170,"rotation":[80,170,33],"position":[0.2,0.1,1.0]}"#] {
            let p = project(
                "",
                &format!(
                    r#""slots":{{"a":{{"path":"{q}"}}}},"camera":{camera},
                       "layers":[{{"id":"i","kind":"image","slot":"a","transform":{{{tr}}}}},
                                 {{"id":"c","kind":"cube","faces":["a"],"size":[0.5,2,0.01],"transform":{{{tr}}}}},
                                 {{"id":"s","kind":"solid","color":[1,0,0,0.5],"blend":"overlay","transform":{{{tr}}}}}]"#
                ),
            );
            r.prepare(&p, &dir()).unwrap();
            let img = r.render_frame(0, 1.0).unwrap_or_else(|e| panic!("{tr} with camera {camera}: {e:#}"));
            assert_eq!(img.dimensions(), (64, 64));
        }
    }
    // A frame too large for the adapter is an error, not a crash.
    let p = solid([1.0, 1.0, 1.0], "");
    r.prepare(&p, &dir()).unwrap();
    let err = r.render_frame(0, 2000.0).unwrap_err().to_string();
    assert!(err.contains("too large"), "{err}");
    assert_eq!(r.render_frame(0, 1.0).unwrap().dimensions(), (64, 64));
}

// ---- built-in one-pass effects (fxlib) -----------------------------------------

/// A `w` x `h` project (seconds, 10 fps) showing the picture `slot_file` full
/// frame, with `effects` on it.
fn picture(w: u32, h: u32, file: &str, effects: &str) -> Project {
    let text = format!(
        r#"{{"format":"edits.project/1","settings":{{"width":{w},"height":{h},"fps":10,"duration":1}},
            "slots":{{"a":{{"path":"{file}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","effects":[{effects}]}}]}}"#
    );
    Project::from_json(&text).unwrap()
}

/// Four sectors about the centre: top red, right green, bottom blue, left yellow.
fn sectors(name: &str) -> String {
    let img = RgbaImage::from_fn(64, 64, |x, y| {
        let (dx, dy) = (x as f64 + 0.5 - 32.0, y as f64 + 0.5 - 32.0);
        let c = if dy.abs() > dx.abs() {
            if dy < 0.0 { [255, 0, 0] } else { [0, 0, 255] }
        } else if dx > 0.0 {
            [0, 255, 0]
        } else {
            [255, 255, 0]
        };
        image::Rgba([c[0], c[1], c[2], 255])
    });
    save(name, &img)
}

/// A smooth colour picture: red across, green down, blue the other way.
fn gradient(name: &str, size: u32) -> String {
    let img = RgbaImage::from_fn(size, size, |x, y| {
        let (u, v) = (x as f64 / (size - 1) as f64, y as f64 / (size - 1) as f64);
        image::Rgba([(255.0 * u) as u8, (255.0 * v) as u8, (255.0 * (1.0 - 0.5 * (u + v))) as u8, 255])
    });
    save(name, &img)
}

/// Mean linear value of each channel over an `n` x `n` grid of blocks.
fn blocks(img: &RgbaImage, n: u32) -> Vec<[f64; 3]> {
    let (bw, bh) = (img.width() / n, img.height() / n);
    let mut out = Vec::new();
    for by in 0..n {
        for bx in 0..n {
            let mut sum = [0.0; 3];
            for y in by * bh..(by + 1) * bh {
                for x in bx * bw..(bx + 1) * bw {
                    let p = img.get_pixel(x, y).0;
                    for c in 0..3 {
                        sum[c] += to_linear(p[c] as f64 / 255.0);
                    }
                }
            }
            out.push(sum.map(|v| v / (bw * bh) as f64));
        }
    }
    out
}

fn mean_luminance(img: &RgbaImage) -> f64 {
    let b = blocks(img, 1)[0];
    0.2126 * b[0] + 0.7152 * b[1] + 0.0722 * b[2]
}

#[test]
fn twirl_turns_inside_its_radius_and_leaves_the_rest() {
    gpu!(r);
    let pic = sectors("sectors.png");
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    // Radius 40% = 25.6 px. A pixel 12.5 px right of the centre turns by
    // 360 x (1 - 12.5/25.6)^2, about 94 degrees: it shows what was above the centre.
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"twirl","params":{"angle":360,"radius":40}}"#), 0);
    assert_px(&plain, 44, 32, [0, 255, 0], 0);
    assert_px(&img, 44, 32, [255, 0, 0], 8);
    // Outside the radius nothing changes at all.
    for (x, y) in (0..64).flat_map(|y| (0..64).map(move |x| (x, y))) {
        let d = ((x as f64 + 0.5 - 32.0).powi(2) + (y as f64 + 0.5 - 32.0).powi(2)).sqrt();
        if d > 26.5 {
            assert_eq!(img.get_pixel(x, y), plain.get_pixel(x, y), "({x}, {y}) is outside the radius");
        }
    }
    // The centre maps to itself: on a gradient, a twirl centred on pixel
    // (44, 40) turns everything around it but keeps that pixel.
    let pic = gradient("gradient64.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"twirl","params":{"angle":720,"radius":15,"center":[0.1953125,0.1328125]}}"#), 0);
    assert_px(&img, 44, 40, px(&plain, 44, 40), 1);
    assert!(px(&img, 47, 40) != px(&plain, 47, 40));
}

#[test]
fn bulge_magnifies_and_pinches_a_disc() {
    gpu!(r);
    let wide = |img: &RgbaImage| (0..64).filter(|x| img.get_pixel(*x, 32).0[0] > 128).count();
    let plain = render(r, &boxed([0.125, 0.125], [0.0, 0.0], ""), 0);
    let bulged = render(r, &boxed([0.125, 0.125], [0.0, 0.0], r#"{"kind":"bulge","params":{"amount":1,"radius":40}}"#), 0);
    let pinched = render(r, &boxed([0.125, 0.125], [0.0, 0.0], r#"{"kind":"bulge","params":{"amount":-1,"radius":40}}"#), 0);
    // Amount 1 doubles the size at the centre; -1 shrinks it to two thirds.
    assert_eq!(wide(&plain), 8);
    assert!((14..=17).contains(&wide(&bulged)), "bulged {}", wide(&bulged));
    assert!((4..=6).contains(&wide(&pinched)), "pinched {}", wide(&pinched));
    // A box outside the radius is untouched.
    let far = render(r, &boxed([0.1, 0.1], [0.4, 0.4], ""), 0);
    assert!(far == render(r, &boxed([0.1, 0.1], [0.4, 0.4], r#"{"kind":"bulge","params":{"amount":1,"radius":30}}"#), 0));
}

#[test]
fn kaleidoscope_is_symmetric() {
    gpu!(r);
    let pic = gradient("gradient64.png", 64);
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"kaleidoscope","params":{"segments":4}}"#), 0);
    let mut worst = (0, 0);
    for y in 0..64u32 {
        for x in 0..64u32 {
            let a = img.get_pixel(x, y).0;
            // Mirrored about the horizontal line through the centre, and a quarter turn.
            for b in [img.get_pixel(x, 63 - y).0, img.get_pixel(63 - y, x).0] {
                let d = (0..3).map(|c| (a[c] as i32 - b[c] as i32).abs()).max().unwrap();
                worst = worst.max((d, 0));
            }
        }
    }
    assert!(worst.0 <= 3, "largest difference between symmetric pixels: {}", worst.0);
    // Without mirroring the wedges are copies, so it is no longer symmetric about the axis.
    let copies = render(r, &picture(64, 64, &pic, r#"{"kind":"kaleidoscope","params":{"segments":4},"options":{"mirror":"off"}}"#), 0);
    assert!(copies != img);
}

#[test]
fn pixelate_cells_are_uniform() {
    gpu!(r);
    let pic = gradient("gradient64.png", 64);
    // Cells of 12.5% = 8 px, one of them centred on the frame's centre:
    // cells span 4..12, 12..20, ... in both directions.
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"pixelate","params":{"size":12.5}}"#), 0);
    for cy in 0..7 {
        for cx in 0..7 {
            let (x0, y0) = (4 + 8 * cx, 4 + 8 * cy);
            let first = img.get_pixel(x0, y0);
            for y in y0..y0 + 8 {
                for x in x0..x0 + 8 {
                    assert_eq!(img.get_pixel(x, y), first, "cell at ({x0}, {y0}) is not uniform at ({x}, {y})");
                }
            }
        }
    }
    // Its colour is the picture's at the cell centre.
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    let (p, q) = (img.get_pixel(20, 20).0, plain.get_pixel(23, 23).0);
    assert!((0..3).all(|c| (p[c] as i32 - (q[c] as i32 + plain.get_pixel(24, 24).0[c] as i32) / 2).abs() <= 3), "{p:?} vs {q:?}");
    // Every shape is piecewise flat: few distinct colours.
    for shape in ["square", "hex", "triangle", "polar"] {
        let img = render(r, &picture(64, 64, &pic, &format!(r#"{{"kind":"pixelate","params":{{"size":20}},"options":{{"shape":"{shape}"}}}}"#)), 0);
        let mut colours: Vec<[u8; 4]> = img.pixels().map(|p| p.0).collect();
        colours.sort_unstable();
        colours.dedup();
        assert!(colours.len() < 100, "{shape}: {} colours", colours.len());
        assert!(img != plain, "{shape}");
    }
}

#[test]
fn halftone_keeps_the_mean_luminance() {
    gpu!(r);
    let pic = gradient("gradient128.png", 128);
    let plain = mean_luminance(&render(r, &picture(128, 128, &pic, ""), 0));
    for (mode, shape) in [("mono", "dot"), ("mono", "line"), ("mono", "square"), ("rgb", "dot"), ("cmyk", "dot")] {
        let fx = format!(r#"{{"kind":"halftone","params":{{"size":6}},"options":{{"mode":"{mode}","shape":"{shape}"}}}}"#);
        let img = render(r, &picture(128, 128, &pic, &fx), 0);
        let mean = mean_luminance(&img);
        assert!((mean - plain).abs() / plain < 0.05, "{mode} {shape}: mean luminance {mean:.4} vs {plain:.4}");
        // It really is a screen: mono pixels are mostly ink or paper.
        if mode == "mono" {
            let extreme = img.pixels().filter(|p| p.0[0] < 40 || p.0[0] > 215).count();
            assert!(extreme > 128 * 128 / 2, "{shape}: only {extreme} pixels are ink or paper");
        }
    }
    // Amount mixes with the original.
    let half = render(r, &picture(128, 128, &pic, r#"{"kind":"halftone","params":{"size":6,"amount":0.5}}"#), 0);
    assert!((mean_luminance(&half) - plain).abs() / plain < 0.05);
}

#[test]
fn chromatic_aberration_leaves_the_centre_and_splits_colours_outward() {
    gpu!(r);
    let fx = r#"{"kind":"chromatic_aberration","params":{"amount":20}}"#;
    let plain = render(r, &boxed([0.6, 0.6], [0.0, 0.0], ""), 0);
    let img = render(r, &boxed([0.6, 0.6], [0.0, 0.0], fx), 0);
    // The box spans pixels 13..51. At the centre nothing moves.
    assert_px(&img, 32, 32, px(&plain, 32, 32), 1);
    assert_px(&img, 31, 31, px(&plain, 31, 31), 1);
    // Outside the right and left edges, red spreads outward and blue stays in.
    for x in [52, 11] {
        let p = px(&img, x, 32);
        assert!(p[0] as i32 - p[2] as i32 > 40, "pixel ({x}, 32) is {p:?}: red should lead outward");
    }
    // The blue picture is the smallest: just inside the edge blue is already missing.
    let p = px(&img, 50, 32);
    assert!(p[0] == 255 && p[0] as i32 - p[2] as i32 > 60, "pixel (50, 32) is {p:?}");
    // The far corners of the black frame stay black.
    assert_px(&img, 0, 0, [0, 0, 0], 0);
}

#[test]
fn builtin_effects_are_resolution_independent() {
    gpu!(r);
    let pic = gradient("gradient128.png", 128);
    let q = quadrants("quad128.png", 128, 128);
    let cases = [
        (&pic, r#"{"kind":"twirl","params":{"angle":200,"radius":35,"center":[0.1,0]}}"#),
        (&q, r#"{"kind":"bulge","params":{"amount":0.8,"radius":30}}"#),
        (&q, r#"{"kind":"kaleidoscope","params":{"segments":6,"rotation":10}}"#),
        (&pic, r#"{"kind":"pixelate","params":{"size":5},"options":{"shape":"hex"}}"#),
        (&pic, r#"{"kind":"halftone","params":{"size":5},"options":{"mode":"cmyk"}}"#),
        (&q, r#"{"kind":"chromatic_aberration","params":{"amount":4}}"#),
    ];
    for (file, fx) in cases {
        let p = picture(256, 256, file, fx);
        let full = blocks(&render_in(r, &p, &dir(), 0, 1.0), 8);
        let half = blocks(&render_in(r, &p, &dir(), 0, 0.5), 8);
        let worst = full.iter().zip(&half).flat_map(|(a, b)| (0..3).map(move |c| (a[c] - b[c]).abs())).fold(0.0, f64::max);
        assert!(worst < 0.04, "{fx}: blocks differ by up to {worst:.3} (linear) between scale 1 and 0.5");
    }
}

#[test]
fn builtin_effects_take_options_and_render_on_layers() {
    gpu!(r);
    let q = quadrants("quad.png", 64, 64);
    // On a layer smaller than the frame, the outside stays transparent black.
    let p = project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","transform":{{"scale":0.5}},"effects":[{{"kind":"halftone","params":{{"size":6}},"options":{{"mode":"rgb","shape":"square"}}}}]}}]"#));
    let img = render(r, &p, 0);
    assert_px(&img, 2, 2, [0, 0, 0], 0);
    assert!(energy(&img) > 10.0);
    // Different options are different pictures; the same frame twice is the same.
    let shape = |s: &str| project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","effects":[{{"kind":"pixelate","params":{{"size":20}},"options":{{"shape":"{s}"}}}}]}}]"#));
    let (a, b) = (render(r, &shape("hex"), 0), render(r, &shape("polar"), 0));
    assert!(a != b);
    assert!(a == render(r, &shape("hex"), 0));
}
