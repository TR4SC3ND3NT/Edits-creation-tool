//! Pixel tests of the built-in colour effects and generators added in WP4:
//! lut, gradient_map, levels, vibrance, fractal_noise and gradient_ramp.
//! Every test skips when the machine has no adapter.

mod common;
mod fxcommon;
use common::*;
use fxcommon::*;

macro_rules! gpu {
    ($r:ident) => {
        let Some(mut guard) = renderer() else { return };
        let $r = guard.as_mut().unwrap();
    };
}

/// Write a 3D `.cube` of size `n` whose entries are `f` of the grid point, and return its name.
fn cube(name: &str, n: usize, f: impl Fn([f64; 3]) -> [f64; 3]) -> String {
    let mut s = format!("TITLE \"{name}\"\nLUT_3D_SIZE {n}\n");
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let c = f([r, g, b].map(|i| i as f64 / (n - 1) as f64));
                s.push_str(&format!("{:.6} {:.6} {:.6}\n", c[0], c[1], c[2]));
            }
        }
    }
    std::fs::write(dir().join(name), s).unwrap();
    name.to_string()
}

#[test]
fn lut_identity_is_a_no_op_and_an_inverting_table_is_invert() {
    gpu!(r);
    let pic = gradient("fxc-gradient64.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    for n in [2, 17, 33] {
        let id = cube(&format!("fxc-identity{n}.cube"), n, |c| c);
        let img = render(r, &picture(64, 64, &pic, &format!(r#"{{"kind":"lut","source":"{id}"}}"#)), 0);
        assert!(max_diff(&img, &plain) <= 1, "identity {n}: differs by {}", max_diff(&img, &plain));
    }
    // A 1D identity (baked into a 3D table) is a no-op too.
    std::fs::write(dir().join("fxc-identity1d.cube"), "LUT_1D_SIZE 2\n0 0 0\n1 1 1\n").unwrap();
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"lut","source":"fxc-identity1d.cube"}"#), 0);
    assert!(max_diff(&img, &plain) <= 1);
    // Inverting table = the invert effect, at any amount.
    let inv = cube("fxc-invert.cube", 2, |c| c.map(|v| 1.0 - v));
    for amount in [1.0, 0.5, 0.25] {
        let lut = render(r, &picture(64, 64, &pic, &format!(r#"{{"kind":"lut","source":"{inv}","params":{{"amount":{amount}}}}}"#)), 0);
        let invert = render(r, &picture(64, 64, &pic, &format!(r#"{{"kind":"invert","params":{{"amount":{amount}}}}}"#)), 0);
        assert!(max_diff(&lut, &invert) <= 1, "amount {amount}: differs by {}", max_diff(&lut, &invert));
    }
    // Amount 0 is skipped; a table that swaps red and blue turns red into blue.
    assert!(render(r, &picture(64, 64, &pic, &format!(r#"{{"kind":"lut","source":"{inv}","params":{{"amount":0}}}}"#)), 0) == plain);
    let swap = cube("fxc-swap.cube", 5, |c| [c[2], c[1], c[0]]);
    assert_px(&render(r, &solid([1.0, 0.0, 0.0], &format!(r#"{{"kind":"lut","source":"{swap}"}}"#)), 0), 32, 32, [0, 0, 255], 1);
}

#[test]
fn lut_files_that_do_not_load_stop_the_render_with_the_reason() {
    gpu!(r);
    std::fs::write(dir().join("fxc-short.cube"), "LUT_3D_SIZE 2\n0 0 0\n").unwrap();
    let p = solid([0.5; 3], r#"{"kind":"lut","source":"fxc-short.cube"}"#);
    let err = format!("{:#}", r.prepare(&p, &dir()).unwrap_err());
    assert!(err.contains("fxc-short.cube") && err.contains("ask for 8 rows"), "{err}");
    let p = solid([0.5; 3], r#"{"kind":"lut","source":"fxc-missing.cube"}"#);
    let err = format!("{:#}", r.prepare(&p, &dir()).unwrap_err());
    assert!(err.contains("cannot read"), "{err}");
}

#[test]
fn gradient_map_recolours_by_brightness() {
    gpu!(r);
    let duo = r#"{"kind":"gradient_map","params":{"shadows":[0,0,1],"highlights":[1,0,0]},"options":{"mode":"duotone"}}"#;
    assert_px(&render(r, &solid([0.0; 3], duo), 0), 32, 32, [0, 0, 255], 0);
    assert_px(&render(r, &solid([1.0; 3], duo), 0), 32, 32, [255, 0, 0], 0);
    // Mid grey is half way, as display values.
    assert_px(&render(r, &solid([0.5; 3], duo), 0), 32, 32, [128, 0, 128], 1);
    // Tritone: mid grey becomes the midtone colour exactly.
    let tri = r#"{"kind":"gradient_map","params":{"shadows":[0,0,0],"midtones":[0,1,0],"highlights":[1,1,1]}}"#;
    assert_px(&render(r, &solid([0.5; 3], tri), 0), 32, 32, [0, 255, 0], 1);
    // The defaults are black and white by luminance: pure red is display grey 0.5.
    assert_px(&render(r, &solid([1.0, 0.0, 0.0], r#"{"kind":"gradient_map"}"#), 0), 32, 32, [128, 128, 128], 1);
    // amount 0 changes nothing.
    let pic = gradient("fxc-gradient64.png", 64);
    assert!(render(r, &picture(64, 64, &pic, r#"{"kind":"gradient_map","params":{"amount":0}}"#), 0) == render(r, &picture(64, 64, &pic, ""), 0));
}

#[test]
fn levels_map_display_values() {
    gpu!(r);
    let pic = gradient("fxc-gradient64.png", 64);
    assert!(render(r, &picture(64, 64, &pic, r#"{"kind":"levels"}"#), 0) == render(r, &picture(64, 64, &pic, ""), 0), "the defaults change nothing");
    assert_px(&render(r, &solid([0.25; 3], r#"{"kind":"levels","params":{"in_white":0.5}}"#), 0), 32, 32, [128; 3], 1);
    assert_px(&render(r, &solid([0.25; 3], r#"{"kind":"levels","params":{"gamma":2}}"#), 0), 32, 32, [128; 3], 1);
    assert_px(&render(r, &solid([0.1; 3], r#"{"kind":"levels","params":{"in_black":0.2}}"#), 0), 32, 32, [0; 3], 0);
    let out = r#"{"kind":"levels","params":{"out_black":0.2,"out_white":0.8}}"#;
    assert_px(&render(r, &solid([0.0; 3], out), 0), 32, 32, [51; 3], 1);
    assert_px(&render(r, &solid([1.0; 3], out), 0), 32, 32, [204; 3], 1);
}

#[test]
fn vibrance_boosts_dull_colours_more_than_vivid_ones() {
    gpu!(r);
    let spread = |p: [u8; 3]| p.iter().max().unwrap() - p.iter().min().unwrap();
    let grey = [0.4; 3];
    assert_px(&render(r, &solid(grey, r#"{"kind":"vibrance","params":{"amount":1}}"#), 0), 32, 32, px(&render(r, &solid(grey, ""), 0), 32, 32), 1);
    // A pure colour is left alone; a dull one gains saturation.
    assert_px(&render(r, &solid([1.0, 0.0, 0.0], r#"{"kind":"vibrance","params":{"amount":1}}"#), 0), 32, 32, [255, 0, 0], 1);
    let dull = [0.6, 0.5, 0.5];
    let before = px(&render(r, &solid(dull, ""), 0), 32, 32);
    let after = px(&render(r, &solid(dull, r#"{"kind":"vibrance","params":{"amount":1}}"#), 0), 32, 32);
    assert!(spread(after) > spread(before) + 10, "{before:?} -> {after:?}");
    // -1 is black and white.
    let bw = px(&render(r, &solid([0.9, 0.3, 0.1], r#"{"kind":"vibrance","params":{"amount":-1}}"#), 0), 32, 32);
    assert!(spread(bw) <= 1, "{bw:?}");
}

#[test]
fn fractal_noise_is_deterministic_grey_and_follows_its_parameters() {
    gpu!(r);
    let fx = |extra: &str| solid([0.2, 0.4, 0.6], &format!(r#"{{"kind":"fractal_noise","params":{{"scale":30{extra}}}}}"#));
    let a = render(r, &fx(""), 0);
    assert!(a == render(r, &fx(""), 9), "it does not read the time");
    assert!(a.pixels().all(|p| p.0[0] == p.0[1] && p.0[1] == p.0[2]), "replace draws grey");
    assert!(colours(&a) > 20, "it is not flat");
    assert!(a != render(r, &fx(r#","seed":3"#), 0));
    assert!(a != render(r, &fx(r#","evolution":180"#), 0));
    // Offset moves it: a quarter of the frame (16 px) to the right.
    let moved = render(r, &fx(r#","offset":[0.25,0]"#), 0);
    for (x, y) in [(20, 10), (40, 30), (63, 63)] {
        assert_px(&moved, x, y, px(&a, x - 16, y), 1);
    }
    // Contrast 0 leaves mid grey plus brightness.
    let flat_grey = render(r, &fx(r#","contrast":0,"brightness":0.25"#), 0);
    assert!(flat_grey.pixels().all(|p| p.0[..3] == [191; 3]), "{:?}", flat_grey.get_pixel(0, 0));
    // It stays inside the layer's transparency.
    let p = project("", r#""layers":[{"id":"s","kind":"solid","color":[1,1,1,1],"transform":{"scale":0.5},"effects":[{"kind":"fractal_noise"}]}]"#);
    let img = render(r, &p, 0);
    assert_px(&img, 2, 2, [0, 0, 0], 0);
    // Multiply over white is the noise itself; amount 0 is skipped.
    let white = |extra: &str| solid([1.0; 3], &format!(r#"{{"kind":"fractal_noise","params":{{"scale":30{extra}}}{}}}"#, if extra.is_empty() { r#","options":{"blend":"multiply"}"# } else { "" }));
    assert!(max_diff(&render(r, &white(""), 0), &render(r, &fx(""), 0)) <= 1);
    assert!(render(r, &white(r#","amount":0"#), 0) == render(r, &solid([1.0; 3], ""), 0));
}

#[test]
fn gradient_ramp_runs_from_start_to_end() {
    gpu!(r);
    // Default: black at the top edge to white at the bottom, as display values.
    let img = render(r, &solid([0.5; 3], r#"{"kind":"gradient_ramp"}"#), 0);
    for y in [0u32, 16, 40, 63] {
        let t = (y as f64 + 0.5) / 64.0;
        assert_px(&img, 10, y, [(t * 255.0).round() as u8; 3], 1);
        assert_eq!(img.get_pixel(10, y), img.get_pixel(50, y), "linear ramps are constant across");
    }
    // Radial: colour a at the start, colour b from the end's distance on.
    let img = render(r, &solid([0.5; 3], r#"{"kind":"gradient_ramp","params":{"start":[0,0],"end":[0.25,0],"color_a":[1,0,0],"color_b":[0,0,1]},"options":{"shape":"radial"}}"#), 0);
    // Pixel (32, 32) is 0.011 from the centre: t = 0.044.
    assert_px(&img, 32, 32, [244, 0, 11], 1);
    assert_px(&img, 2, 2, [0, 0, 255], 0);
    assert_eq!(img.get_pixel(32, 16), img.get_pixel(16, 32));
    // Multiply over black stays black.
    let img = render(r, &solid([0.0; 3], r#"{"kind":"gradient_ramp","options":{"blend":"multiply"}}"#), 0);
    assert!(img.pixels().all(|p| p.0[..3] == [0, 0, 0]));
}
