//! Pixel tests of the built-in stylise effects added in WP4: screentone,
//! edge_detect, ink, toon, kuwahara, emboss, sharpen, glitch, scanlines, vhs,
//! film_damage and film_burn. Every test skips when the machine has no adapter.

mod common;
mod fxcommon;
use common::*;
use fxcommon::*;
use image::{Rgba, RgbaImage};

macro_rules! gpu {
    ($r:ident) => {
        let Some(mut guard) = renderer() else { return };
        let $r = guard.as_mut().unwrap();
    };
}

#[test]
fn stylise_effects_at_zero_strength_change_nothing() {
    gpu!(r);
    let pic = gradient("fxs-gradient64.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    for fx in [
        r#"{"kind":"screentone","params":{"amount":0}}"#,
        r#"{"kind":"screentone","params":{"size":0}}"#,
        r#"{"kind":"edge_detect","params":{"amount":0}}"#,
        r#"{"kind":"ink","params":{"amount":0}}"#,
        r#"{"kind":"toon","params":{"amount":0}}"#,
        r#"{"kind":"kuwahara","params":{"radius":0}}"#,
        r#"{"kind":"kuwahara","params":{"amount":0}}"#,
        r#"{"kind":"emboss","params":{"amount":0}}"#,
        r#"{"kind":"sharpen","params":{"amount":0}}"#,
        r#"{"kind":"glitch","params":{"amount":0,"block":0}}"#,
        r#"{"kind":"scanlines","params":{"amount":0}}"#,
        r#"{"kind":"vhs","params":{"noise":0,"roll":0,"bleed":0,"wobble":0}}"#,
        r#"{"kind":"film_damage","params":{"scratches":0,"dust":0,"flicker":0,"jitter":0}}"#,
        r#"{"kind":"film_burn","params":{"amount":0}}"#,
    ] {
        for frame in [0, 7] {
            assert!(render(r, &picture(64, 64, &pic, fx), frame) == render(r, &picture(64, 64, &pic, ""), frame), "{fx} changed frame {frame}");
        }
    }
    assert_eq!(plain.dimensions(), (64, 64));
}

#[test]
fn screentone_dithers_to_ink_and_paper() {
    gpu!(r);
    // A cell per pixel, square to the frame: an 8x8 Bayer matrix turns a
    // display value of 0.5 into exactly half paper, 0.25 into a quarter.
    let fx = r#"{"kind":"screentone","params":{"size":1.5625,"angle":0},"options":{"pattern":"bayer"}}"#;
    for (grey, share) in [(0.5, 0.5), (0.25, 0.25), (0.0, 0.0), (1.0, 1.0)] {
        let img = render(r, &solid([grey; 3], fx), 0);
        let paper = img.pixels().filter(|p| p.0[..3] == [255, 255, 255]).count();
        let ink = img.pixels().filter(|p| p.0[..3] == [0, 0, 0]).count();
        assert_eq!(paper + ink, 64 * 64, "only ink and paper");
        assert_eq!(paper, (share * 4096.0) as usize, "grey {grey}");
    }
    // Every pattern with 2 tones gives two colours; with 4 tones, four; ink and paper are the colours asked for.
    let pic = gradient("fxs-gradient64.png", 64);
    for pattern in ["dots", "bayer", "lines"] {
        let img = render(r, &picture(64, 64, &pic, &format!(r#"{{"kind":"screentone","options":{{"pattern":"{pattern}"}}}}"#)), 0);
        assert_eq!(colours(&img), 2, "{pattern}");
        let img = render(r, &picture(64, 64, &pic, &format!(r#"{{"kind":"screentone","params":{{"levels":4,"ink":[0,0,0.5],"paper":[1,1,0.5]}},"options":{{"pattern":"{pattern}"}}}}"#)), 0);
        assert!(colours(&img) <= 4 && colours(&img) >= 3, "{pattern}: {}", colours(&img));
        assert!(img.pixels().all(|p| p.0[0] == p.0[1] && p.0[2] == byte(0.5)), "{pattern}: tones run from ink to paper");
    }
    // Brighter pictures get more paper.
    let mut paper = |grey: f64| render(r, &solid([grey; 3], r#"{"kind":"screentone","params":{"size":5}}"#), 0).pixels().filter(|p| p.0[0] == 255).count();
    assert!(paper(0.2) < paper(0.5) && paper(0.5) < paper(0.8));
}

#[test]
fn edge_detect_draws_the_outline_only() {
    gpu!(r);
    // A white box from pixel 16 to 47 on black.
    let img = render(r, &boxed(0.5, r#"{"kind":"edge_detect"}"#), 0);
    assert_px(&img, 32, 32, [0, 0, 0], 0);
    assert_px(&img, 5, 5, [0, 0, 0], 0);
    assert_px(&img, 16, 32, [255, 255, 255], 0);
    assert_px(&img, 47, 32, [255, 255, 255], 0);
    let img = render(r, &boxed(0.5, r#"{"kind":"edge_detect","params":{"invert":1,"color":[1,0,0]}}"#), 0);
    assert_px(&img, 32, 32, [255, 255, 255], 0);
    assert_px(&img, 16, 32, [0, 255, 255], 0);
    // Below the threshold nothing is drawn; amount mixes with the picture.
    let img = render(r, &boxed(0.5, r#"{"kind":"edge_detect","params":{"strength":0.05}}"#), 0);
    assert!(img.pixels().all(|p| p.0[..3] == [0, 0, 0]));
    let img = render(r, &boxed(0.5, r#"{"kind":"edge_detect","params":{"amount":0.5}}"#), 0);
    assert_px(&img, 32, 32, [byte(0.5); 3], 1);
}

#[test]
fn ink_keeps_lines_and_drops_shading() {
    gpu!(r);
    // Grey shading on the left, white on the right, a black line 2 px wide at x = 47.
    let pic = save(
        "fxs-ink.png",
        &RgbaImage::from_fn(64, 64, |x, _| match x {
            47 | 48 => Rgba([0, 0, 0, 255]),
            x if x < 32 => Rgba([128, 128, 128, 255]),
            _ => Rgba([255, 255, 255, 255]),
        }),
    );
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"ink"}"#), 0);
    assert_px(&img, 10, 32, [255, 255, 255], 0);
    assert_px(&img, 58, 32, [255, 255, 255], 0);
    assert_px(&img, 47, 32, [0, 0, 0], 0);
    assert_px(&img, 48, 32, [0, 0, 0], 0);
    // Ink and paper colours.
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"ink","params":{"ink":[0,0,1],"paper":[1,1,0]}}"#), 0);
    assert_px(&img, 10, 32, [255, 255, 0], 0);
    assert_px(&img, 47, 32, [0, 0, 255], 0);
    // Solid blacks stay ink.
    let img = render(r, &solid([0.05; 3], r#"{"kind":"ink"}"#), 0);
    assert_px(&img, 32, 32, [0, 0, 0], 0);
}

#[test]
fn toon_bands_colours_and_outlines_edges() {
    gpu!(r);
    let fx = |levels: u32| format!(r#"{{"kind":"toon","params":{{"levels":{levels}}}}}"#);
    assert_px(&render(r, &solid([0.3, 0.7, 0.3], &fx(2)), 0), 32, 32, [0, 255, 0], 0);
    assert_px(&render(r, &solid([0.3, 0.7, 0.2], &fx(3)), 0), 32, 32, [128, 128, 0], 1);
    let img = render(r, &boxed(0.5, &fx(4)), 0);
    assert_px(&img, 32, 32, [255, 255, 255], 0);
    assert_px(&img, 16, 32, [0, 0, 0], 0);
    let img = render(r, &boxed(0.5, r#"{"kind":"toon","params":{"edge":1}}"#), 0);
    assert_px(&img, 16, 32, [255, 255, 255], 0);
}

#[test]
fn kuwahara_smooths_flat_areas_and_keeps_edges() {
    gpu!(r);
    let fx = r#"{"kind":"kuwahara","params":{"radius":5}}"#;
    let img = render(r, &boxed(0.5, fx), 0);
    for (x, want) in [(14, 0), (15, 0), (16, 255), (17, 255), (47, 255), (48, 0)] {
        assert_px(&img, x, 32, [want; 3], 0);
    }
    assert_px(&render(r, &solid([0.6, 0.3, 0.9], fx), 0), 32, 32, px(&render(r, &solid([0.6, 0.3, 0.9], ""), 0), 32, 32), 1);
    // Noise is calmed: neighbouring pixels differ less.
    let noise = noisy("fxs-noise.png");
    let roughness = |img: &RgbaImage| (0..63u32).flat_map(|y| (0..63u32).map(move |x| (x, y))).map(|(x, y)| (img.get_pixel(x, y).0[1] as i32 - img.get_pixel(x + 1, y).0[1] as i32).abs()).sum::<i32>();
    let plain = render(r, &picture(64, 64, &noise, ""), 0);
    let smooth = render(r, &picture(64, 64, &noise, fx), 0);
    assert!(roughness(&smooth) * 2 < roughness(&plain), "{} vs {}", roughness(&smooth), roughness(&plain));
}

#[test]
fn emboss_lights_edges_from_its_angle() {
    gpu!(r);
    assert_px(&render(r, &solid([0.8, 0.2, 0.4], r#"{"kind":"emboss"}"#), 0), 32, 32, [128, 128, 128], 1);
    // Light from the right (0 degrees): the box's right edge is lit, its left edge in shadow.
    let img = render(r, &boxed(0.5, r#"{"kind":"emboss","params":{"height":3}}"#), 0);
    assert_px(&img, 47, 32, [255, 255, 255], 0);
    assert_px(&img, 16, 32, [0, 0, 0], 0);
    assert_px(&img, 32, 32, [128, 128, 128], 1);
    // From the left (180) it is the other way round.
    let img = render(r, &boxed(0.5, r#"{"kind":"emboss","params":{"height":3,"angle":180}}"#), 0);
    assert_px(&img, 47, 32, [0, 0, 0], 0);
    assert_px(&img, 16, 32, [255, 255, 255], 0);
}

#[test]
fn sharpen_steepens_soft_edges_and_keeps_flat_areas() {
    gpu!(r);
    let c = [0.7, 0.4, 0.2];
    assert_px(&render(r, &solid(c, r#"{"kind":"sharpen","params":{"amount":1}}"#), 0), 32, 32, px(&render(r, &solid(c, ""), 0), 32, 32), 1);
    let pic = split("fxs-split.png", 32);
    // How far the row is from pure black and white: smaller is a crisper edge.
    let softness = |img: &RgbaImage| (0..64).map(|x| { let v = img.get_pixel(x, 32).0[0] as i32; v.min(255 - v) }).sum::<i32>();
    let soft = render(r, &picture(64, 64, &pic, r#"{"kind":"gaussian_blur","params":{"radius":3}}"#), 0);
    let sharp = render(r, &picture(64, 64, &pic, r#"{"kind":"gaussian_blur","params":{"radius":3}},{"kind":"sharpen","params":{"amount":1}}"#), 0);
    assert!(softness(&sharp) < softness(&soft), "{} vs {}", softness(&sharp), softness(&soft));
    // No halo: the result stays within black and white.
    assert!(sharp.pixels().all(|p| p.0[0] == p.0[1]));
}

#[test]
fn glitch_changes_at_its_rate_and_with_its_seed() {
    gpu!(r);
    let pic = gradient("fxs-gradient64.png", 64);
    let fx = |extra: &str| picture(64, 64, &pic, &format!(r#"{{"kind":"glitch","params":{{"amount":0.6,"block":0.3{extra}}}}}"#));
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    // 8 changes per second at 10 fps: frames 0 and 1 share a pattern, frame 2 has the next.
    let f0 = render(r, &fx(""), 0);
    assert!(f0 != plain);
    assert!(f0 == render(r, &fx(""), 0), "the same frame renders the same");
    assert!(f0 == render(r, &fx(""), 1));
    assert!(f0 != render(r, &fx(""), 2));
    assert!(f0 != render(r, &fx(r#","seed":5"#), 0), "the seed changes the pattern");
    let held = fx(r#","rate":0"#);
    assert!(render(r, &held, 3) == render(r, &held, 30), "rate 0 holds one pattern");
    // Amount 1 moves every band; the rows stay rows (a band jumps sideways only).
    let rows_pic = rows("fxs-rows.png");
    let img = render(r, &picture(64, 64, &rows_pic, r#"{"kind":"glitch","params":{"amount":1,"rgb":0}}"#), 0);
    assert!(img == render(r, &picture(64, 64, &rows_pic, ""), 0));
}

#[test]
fn scanlines_follow_the_cosine_and_keep_their_mean() {
    gpu!(r);
    // 16 lines over 64 px: 4 px per line, fully visible.
    let img = render(r, &solid([1.0; 3], r#"{"kind":"scanlines","params":{"count":16,"amount":0.5}}"#), 0);
    for y in 0..8u32 {
        let w = 0.5 + 0.5 * (std::f64::consts::TAU * (y as f64 + 0.5) / 64.0 * 16.0).cos();
        assert_px(&img, 10, y, [byte(1.0 - 0.5 * w); 3], 1);
    }
    // 300 lines over 64 px would alias: an even darkening of the same mean.
    let img = render(r, &solid([1.0; 3], r#"{"kind":"scanlines","params":{"count":300,"amount":0.5}}"#), 0);
    assert!(img.pixels().all(|p| p.0[..3] == [byte(0.75); 3]));
    // Scrolling moves them with time.
    let moving = solid([1.0; 3], r#"{"kind":"scanlines","params":{"count":16,"amount":0.5,"speed":2.5}}"#);
    assert!(render(r, &moving, 0) != render(r, &moving, 1));
    assert!(render(r, &moving, 1) == render(r, &moving, 1));
}

#[test]
fn vhs_film_damage_and_film_burn_are_deterministic_per_frame() {
    gpu!(r);
    let pic = gradient("fxs-gradient64.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    for fx in [
        r#"{"kind":"vhs","params":{"noise":0.5,"wobble":0.5,"roll":0.3}}"#,
        r#"{"kind":"film_damage","params":{"scratches":1,"dust":1,"flicker":0.3,"jitter":1}}"#,
        r#"{"kind":"film_burn","params":{"amount":0.5,"speed":2}}"#,
    ] {
        let with_seed = |seed: u32| picture(64, 64, &pic, &fx.replacen("\"params\":{", &format!("\"params\":{{\"seed\":{seed},"), 1));
        let a = render(r, &with_seed(0), 3);
        assert!(a != plain, "{fx} changes the picture");
        assert!(a == render(r, &with_seed(0), 3), "{fx}: the same frame renders the same");
        assert!(a != render(r, &with_seed(0), 4), "{fx}: the next frame differs");
        assert!(a != render(r, &with_seed(7), 3), "{fx}: the seed changes it");
    }
}

#[test]
fn film_burn_spreads_with_its_amount() {
    gpu!(r);
    let pic = gradient("fxs-gradient64.png", 64);
    let mut lit = |amount: f64| {
        let img = render(r, &picture(64, 64, &pic, &format!(r#"{{"kind":"film_burn","params":{{"amount":{amount}}}}}"#)), 0);
        img.pixels().filter(|p| p.0[0] == 255 && p.0[1] > 200).count()
    };
    let counts: Vec<usize> = [0.1, 0.4, 0.6, 1.0].iter().map(|a| lit(*a)).collect();
    assert!(counts.windows(2).all(|w| w[0] <= w[1]), "{counts:?}");
    assert_eq!(counts[3], 64 * 64, "amount 1 burns the whole frame");
    // It starts at its centre.
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"film_burn","params":{"amount":0.35,"center":[-0.3,-0.3]}}"#), 0);
    let near = (0..16u32).flat_map(|y| (0..16u32).map(move |x| (x, y))).filter(|(x, y)| img.get_pixel(*x, *y).0[0] == 255).count();
    let far = (48..64u32).flat_map(|y| (48..64u32).map(move |x| (x, y))).filter(|(x, y)| img.get_pixel(*x, *y).0[0] == 255).count();
    assert!(near > far, "near {near}, far {far}");
}

#[test]
fn stylise_effects_are_resolution_independent() {
    gpu!(r);
    let pic = gradient("fxs-gradient128.png", 128);
    for fx in [
        r#"{"kind":"edge_detect","params":{"width":1}}"#,
        r#"{"kind":"ink","params":{"width":2}}"#,
        r#"{"kind":"kuwahara","params":{"radius":3}}"#,
        r#"{"kind":"emboss","params":{"height":1}}"#,
        r#"{"kind":"glitch","params":{"amount":0.5,"block":0.2}}"#,
        r#"{"kind":"scanlines","params":{"count":40,"amount":0.5}}"#,
        r#"{"kind":"film_burn","params":{"amount":0.5}}"#,
        r#"{"kind":"screentone","params":{"size":3}}"#,
    ] {
        let worst = scale_difference(r, &picture(256, 256, &pic, fx));
        assert!(worst < 0.06, "{fx}: blocks differ by up to {worst:.3} (linear) between scale 1 and 0.5");
    }
}
