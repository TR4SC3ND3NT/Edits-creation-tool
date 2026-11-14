//! Pixel tests of the built-in colour effects and generators added in WP4
//! (lut, gradient_map, levels, vibrance, fractal_noise and gradient_ramp) and
//! of wave 3's colour and keying passes (white_balance to haze, chroma_key to
//! difference_matte, gradient_map stops). Every test skips when the machine
//! has no adapter.

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

// ---- wave 3: keying ---------------------------------------------------------

/// A full-frame solid of `color` (sRGB) with `effects`, over a full-frame solid `bg`.
fn over(bg: [f64; 3], color: [f64; 3], effects: &str) -> edits_core::Project {
    project(
        "",
        &format!(
            r#""layers":[{{"id":"bg","kind":"solid","color":[{},{},{},1]}},{{"id":"s","kind":"solid","color":[{},{},{},1],"effects":[{effects}]}}]"#,
            bg[0], bg[1], bg[2], color[0], color[1], color[2]
        ),
    )
}

const MAGENTA: [f64; 3] = [1.0, 0.0, 1.0];

/// The colour a solid of `color` (sRGB) shows with `effect` over magenta.
fn keyed(r: &mut edits_render::Renderer, color: [f64; 3], effect: &str) -> [u8; 3] {
    px(&render(r, &over(MAGENTA, color, effect), 0), 32, 32)
}

#[track_caller]
fn near(got: [u8; 3], want: [u8; 3], tolerance: i32) {
    assert!((0..3).all(|i| (got[i] as i32 - want[i] as i32).abs() <= tolerance), "{got:?}, expected {want:?} +-{tolerance}");
}

#[test]
fn chroma_key_removes_the_key_colour_and_keeps_far_colours() {
    gpu!(r);
    let key = r#"{"kind":"chroma_key"}"#;
    assert_eq!(keyed(r, [0.0, 1.0, 0.0], key), [255, 0, 255], "pure green is keyed out");
    assert_eq!(keyed(r, [1.0, 0.0, 0.0], key), [255, 0, 0], "red stays");
    near(keyed(r, [0.5, 0.5, 0.5], key), [128, 128, 128], 1);
    // Any key colour: blue screen.
    assert_eq!(keyed(r, [0.0, 0.0, 1.0], r#"{"kind":"chroma_key","params":{"color":[0,0,1]}}"#), [255, 0, 255]);
    // A colour inside the softness ramp (0.16 from green in chroma) is partly transparent.
    let part = keyed(r, [0.3, 1.0, 0.3], r#"{"kind":"chroma_key","params":{"tolerance":0.1,"softness":0.3}}"#);
    assert!(part[0] < 250 && part[1] > 20 && part[1] < 200, "{part:?}");
    // amount scales the transparency; 0 changes nothing.
    assert_eq!(keyed(r, [0.0, 1.0, 0.0], r#"{"kind":"chroma_key","params":{"amount":0}}"#), [0, 255, 0]);
    near(keyed(r, [0.0, 1.0, 0.0], r#"{"kind":"chroma_key","params":{"amount":0.5}}"#), [byte(0.5); 3], 1);
    // The matte view: keyed is black, kept is white.
    let matte = r#"{"kind":"chroma_key","options":{"view":"matte"}}"#;
    assert_eq!(keyed(r, [0.0, 1.0, 0.0], matte), [0, 0, 0]);
    assert_eq!(keyed(r, [1.0, 0.0, 0.0], matte), [255, 255, 255]);
    // On a picture: the green quadrant shows the background.
    let q = quadrants("fxc-quad.png", 64, 64);
    let p = project("", &format!(r#""slots":{{"a":{{"path":"{q}"}}}},"layers":[{{"id":"bg","kind":"solid","color":[1,0,1,1]}},{{"id":"a","kind":"image","slot":"a","effects":[{key}]}}]"#));
    let img = render(r, &p, 0);
    assert_px(&img, 48, 16, [255, 0, 255], 0);
    assert_px(&img, 16, 16, [255, 0, 0], 0);
    assert_px(&img, 48, 48, [255, 255, 0], 0);
}

#[test]
fn a_key_multiplies_what_the_masks_leave() {
    gpu!(r);
    // A mask keeps the left half; the key then halves the alpha of what is left.
    let p = project(
        "",
        r#""layers":[{"id":"bg","kind":"solid","color":[1,0,1,1]},
                     {"id":"s","kind":"solid","color":[0,1,0,1],"masks":[{"shape":"rect","size":[0.5,1],"position":[-0.25,0]}],"effects":[{"kind":"chroma_key","params":{"amount":0.5}}]}]"#,
    );
    let img = render(r, &p, 0);
    assert_px(&img, 16, 32, [byte(0.5); 3], 1);
    assert_px(&img, 48, 32, [255, 0, 255], 0);
}

#[test]
fn spill_suppress_limits_the_key_channel() {
    gpu!(r);
    let c = [0.5, 0.8, 0.4];
    // fast: green is limited to the larger of red and blue (in linear light, so exactly red here).
    near(one(r, c, r#"{"kind":"spill_suppress"}"#), [128, 128, 102], 1);
    // better: the chroma along green's chroma direction is removed.
    let lin = c.map(to_linear);
    let dir = [-1.0, 2.0, -1.0].map(|v: f64| v / 6f64.sqrt());
    let g = (lin[0] + lin[1] + lin[2]) / 3.0;
    let proj: f64 = (0..3).map(|i| (lin[i] - g) * dir[i]).sum();
    let want: [u8; 3] = std::array::from_fn(|i| byte(lin[i] - proj * dir[i]));
    near(one(r, c, r#"{"kind":"spill_suppress","options":{"method":"better"}}"#), want, 1);
    // Colours without spill stay; amount 0 changes nothing.
    near(one(r, [1.0, 0.2, 0.2], r#"{"kind":"spill_suppress"}"#), [255, 51, 51], 1);
    near(one(r, c, r#"{"kind":"spill_suppress","params":{"amount":0}}"#), [128, 204, 102], 1);
}

#[test]
fn luma_key_keys_by_brightness() {
    gpu!(r);
    let dark = r#"{"kind":"luma_key"}"#;
    assert_eq!(keyed(r, [0.0; 3], dark), [255, 0, 255], "black is keyed out");
    assert_eq!(keyed(r, [1.0; 3], dark), [255, 255, 255], "white stays");
    near(keyed(r, [0.5; 3], dark), [128, 128, 128], 1);
    let bright = r#"{"kind":"luma_key","params":{"threshold":0.9},"options":{"key_out":"brighter"}}"#;
    assert_eq!(keyed(r, [1.0; 3], bright), [255, 0, 255]);
    assert_eq!(keyed(r, [0.0; 3], bright), [0, 0, 0]);
    let similar = r#"{"kind":"luma_key","params":{"threshold":0.5,"tolerance":0.1,"softness":0},"options":{"key_out":"similar"}}"#;
    assert_eq!(keyed(r, [0.5; 3], similar), [255, 0, 255]);
    near(keyed(r, [0.9; 3], similar), [230; 3], 1);
    let dissimilar = r#"{"kind":"luma_key","params":{"threshold":0.5,"tolerance":0.1,"softness":0},"options":{"key_out":"dissimilar"}}"#;
    near(keyed(r, [0.5; 3], dissimilar), [128; 3], 1);
    assert_eq!(keyed(r, [0.9; 3], dissimilar), [255, 0, 255]);
    assert_eq!(keyed(r, [0.0; 3], r#"{"kind":"luma_key","params":{"amount":0}}"#), [0, 0, 0]);
}

#[test]
fn unmult_turns_black_into_transparency_and_keeps_the_picture_over_black() {
    gpu!(r);
    let u = r#"{"kind":"unmult"}"#;
    assert_eq!(keyed(r, [0.0; 3], u), [255, 0, 255], "black becomes transparent");
    assert_eq!(keyed(r, [1.0; 3], u), [255, 255, 255], "white stays");
    // Mid grey (linear 0.214) becomes white at alpha 0.214: over magenta 0.214 + (1 - 0.214) x magenta.
    near(keyed(r, [0.5; 3], u), [255, 128, 255], 1);
    // Over black it is the original.
    let pic = gradient("fxc-gradient64.png", 64);
    assert!(max_diff(&render(r, &picture(64, 64, &pic, u), 0), &render(r, &picture(64, 64, &pic, ""), 0)) <= 1);
    // On white: white goes, black stays.
    let w = r#"{"kind":"unmult","options":{"background":"white"}}"#;
    assert_eq!(keyed(r, [1.0; 3], w), [255, 0, 255]);
    assert_eq!(keyed(r, [0.0; 3], w), [0, 0, 0]);
    // A level with no softness is a hard cut.
    let cut = r#"{"kind":"unmult","params":{"level":0.5,"softness":0}}"#;
    assert_eq!(keyed(r, [0.5; 3], cut), [255, 0, 255], "linear 0.214 is under 0.5");
    near(keyed(r, [0.9; 3], cut), [230; 3], 1);
}

#[test]
fn difference_matte_keys_out_what_matches_the_clean_plate() {
    gpu!(r);
    let shot = halves("fxc-shot.png", [255, 0, 0], [0, 0, 255]);
    let plate = halves("fxc-plate.png", [255, 0, 0], [0, 255, 0]);
    let img = render(r, &picture_with_map(&shot, &plate, r#"{"kind":"difference_matte","source":"slot:m"}"#), 0);
    assert_px(&img, 32, 10, [0, 0, 0], 0);
    assert_px(&img, 32, 50, [0, 0, 255], 0);
    let matte = render(r, &picture_with_map(&shot, &plate, r#"{"kind":"difference_matte","source":"slot:m","options":{"view":"matte"}}"#), 0);
    assert_px(&matte, 32, 10, [0, 0, 0], 0);
    assert_px(&matte, 32, 50, [255, 255, 255], 0);
}

// ---- wave 3: colour ---------------------------------------------------------

/// The pixel at the centre of a solid of `color` (sRGB) with `effect`.
fn one(r: &mut edits_render::Renderer, color: [f64; 3], effect: &str) -> [u8; 3] {
    px(&render(r, &solid(color, effect), 0), 32, 32)
}

/// A display value (0..1) as a byte.
fn d8(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[test]
fn wave3_effects_at_their_neutral_values_change_nothing() {
    gpu!(r);
    let pic = gradient("fxc-gradient64.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    let neutral = r#"
        {"kind":"white_balance"}, {"kind":"lift_gamma_gain"}, {"kind":"highlights_shadows"}, {"kind":"channel_mixer"},
        {"kind":"curves"}, {"kind":"hsl"}, {"kind":"colorama","params":{"amount":0}}, {"kind":"replace_color","params":{"amount":0}},
        {"kind":"leave_color","params":{"amount":0}}, {"kind":"solarize","params":{"amount":0}}, {"kind":"bleach_bypass","params":{"amount":0}},
        {"kind":"technicolor","params":{"amount":0}}, {"kind":"haze","params":{"distance":0}}, {"kind":"spill_suppress","params":{"amount":0}},
        {"kind":"chroma_key","params":{"amount":0}}, {"kind":"luma_key","params":{"amount":0}},
        {"kind":"gradient_map","params":{"amount":0},"options":{"mode":"stops"}},
        {"kind":"curves","params":{"master":[0,0,0.5,0.5,1,1],"red":[0,0,1,1]}}"#;
    assert!(render(r, &picture(64, 64, &pic, neutral), 0) == plain, "neutral effects changed the picture");
    // Neutral values that still run a pass change nothing beyond rounding.
    let running = r#"{"kind":"lift_gamma_gain","params":{"saturation":1.0001}}, {"kind":"hsl","params":{"reds":[0.0001,0,0]}},
        {"kind":"curves","params":{"master":[0,0,0.25,0.25,1,1],"amount":0.0001}}, {"kind":"unmult","params":{"level":0,"softness":1}}"#;
    assert!(max_diff(&render(r, &picture(64, 64, &pic, running), 0), &plain) <= 1);
}

#[test]
fn white_balance_warms_cools_and_tints() {
    gpu!(r);
    // Mid grey overlaid with the warming filter is the filter colour.
    near(one(r, [0.5; 3], r#"{"kind":"white_balance","params":{"temperature":1}}"#), [237, 138, 0], 1);
    let cool = one(r, [0.5; 3], r#"{"kind":"white_balance","params":{"temperature":-0.3}}"#);
    assert!(cool[2] > cool[0] + 30, "{cool:?}");
    // Tint 1 moves Q by 0.10452: grey 0.5 + 0.10452 x (0.621, -0.647, 1.702).
    let want = [0.621, -0.647, 1.702].map(|k: f64| d8(0.5 + 0.10452 * k));
    near(one(r, [0.5; 3], r#"{"kind":"white_balance","params":{"tint":1}}"#), want, 1);
}

#[test]
fn lift_gamma_gain_follow_their_formulas() {
    gpu!(r);
    near(one(r, [0.0; 3], r#"{"kind":"lift_gamma_gain","params":{"lift":0.5}}"#), [128; 3], 1);
    near(one(r, [1.0; 3], r#"{"kind":"lift_gamma_gain","params":{"gain":0.5}}"#), [128; 3], 1);
    near(one(r, [0.25; 3], r#"{"kind":"lift_gamma_gain","params":{"gamma":2}}"#), [128; 3], 1);
    // Per channel: lift only red.
    near(one(r, [0.0; 3], r#"{"kind":"lift_gamma_gain","params":{"lift":[0.5,0,0]}}"#), [128, 0, 0], 1);
    // Saturation 0 is the Rec. 709 luminance of the display values.
    near(one(r, [1.0, 0.0, 0.0], r#"{"kind":"lift_gamma_gain","params":{"saturation":0}}"#), [d8(0.2126); 3], 1);
}

#[test]
fn highlights_shadows_follow_gpuimage() {
    gpu!(r);
    let expect = |v: f64, s: f64, h: f64| {
        let l = (v * 0.9).clamp(0.0, 1.0);
        let shadow = (l.powf(1.0 / (s + 1.0)) - 0.76 * l.powf(2.0 / (s + 1.0)) - l).clamp(0.0, 1.0);
        let hg = 1.0 - h;
        let highlight = (1.0 - ((1.0 - l).powf(1.0 / (2.0 - hg)) - 0.8 * (1.0 - l).powf(2.0 / (2.0 - hg))) - l).clamp(-1.0, 0.0);
        [d8(v * (l + shadow + highlight) / l); 3]
    };
    let lifted = expect(0.25, 1.0, 0.0);
    let pulled = expect(0.8, 0.0, 1.0);
    assert!(lifted[0] > 70 && pulled[0] < 204, "{lifted:?} {pulled:?}");
    near(one(r, [0.25; 3], r#"{"kind":"highlights_shadows","params":{"shadows":1}}"#), lifted, 1);
    near(one(r, [0.8; 3], r#"{"kind":"highlights_shadows","params":{"highlights":1}}"#), pulled, 1);
}

#[test]
fn channel_mixer_mixes_rgb_and_hsv() {
    gpu!(r);
    let swap = r#"{"kind":"channel_mixer","params":{"red":[0,0,1,0],"blue":[1,0,0,0]}}"#;
    near(one(r, [1.0, 0.0, 0.0], swap), [0, 0, 255], 0);
    near(one(r, [0.2, 0.6, 0.9], swap), [230, 153, 51], 1);
    // The constant is added (display values).
    near(one(r, [0.0; 3], r#"{"kind":"channel_mixer","params":{"green":[0,1,0,0.5]}}"#), [0, 128, 0], 1);
    // The sepia matrix (three.js / glfx.js) on display values.
    let c = [0.6, 0.4, 0.2];
    let rows = [[0.393, 0.769, 0.189], [0.349, 0.686, 0.168], [0.272, 0.534, 0.131]];
    let want = rows.map(|row| d8(row[0] * c[0] + row[1] * c[1] + row[2] * c[2]));
    let sepia = r#"{"kind":"channel_mixer","params":{"red":[0.393,0.769,0.189,0],"green":[0.349,0.686,0.168,0],"blue":[0.272,0.534,0.131,0]}}"#;
    near(one(r, c, sepia), want, 1);
    // HSV: hue set to a third of a turn takes red to green.
    let hsv = r#"{"kind":"channel_mixer","params":{"red":[0,0,0,0.333333]},"options":{"space":"hsv"}}"#;
    near(one(r, [1.0, 0.0, 0.0], hsv), [0, 255, 0], 1);
    // amount 0.5 is half way, as display values.
    near(one(r, [1.0, 0.0, 0.0], r#"{"kind":"channel_mixer","params":{"red":[0,0,0,0],"amount":0.5}}"#), [128, 0, 0], 1);
}

#[test]
fn curves_map_display_values_through_splines() {
    gpu!(r);
    let invert = r#"{"kind":"curves","params":{"master":[0,1,1,0]}}"#;
    near(one(r, [0.0; 3], invert), [255; 3], 0);
    near(one(r, [1.0; 3], invert), [0; 3], 0);
    near(one(r, [0.25; 3], invert), [191; 3], 1);
    // A curve through (0.5, 0.75) on red only: mid grey becomes (0.75, 0.5, 0.5).
    let lift = r#"{"kind":"curves","params":{"red":[0,0,0.5,0.75,1,1]}}"#;
    near(one(r, [0.5; 3], lift), [191, 128, 128], 1);
    // Between the points, the natural spline: its second derivative at the
    // middle point is 3 (d1 - d0) / (x2 - x0), 0 at the ends.
    let m1 = 3.0 * ((1.0 - 0.75) / 0.5 - 0.75 / 0.5) / 1.0;
    let y = 0.5 * 0.75 + (0.125 - 0.5) * m1 * 0.25 / 6.0;
    near(one(r, [0.25; 3], lift), [d8(y), 64, 64], 1);
    // The master curve runs first, then the channel's.
    near(one(r, [0.5; 3], r#"{"kind":"curves","params":{"master":[0,1,1,0],"red":[0,0,0.5,0.75,1,1]}}"#), [191, 128, 128], 1);
    // amount mixes.
    near(one(r, [0.0; 3], r#"{"kind":"curves","params":{"master":[0,1,1,0],"amount":0.5}}"#), [128; 3], 1);
    // An animated curve gets the table of its value at each frame: half way
    // from the identity to the inversion is flat at 0.5.
    let keyed = r#"{"kind":"curves","params":{"master":{"keys":[{"t":0,"v":[0,0,1,1]},{"t":4,"v":[0,1,1,0]}]}}}"#;
    near(px(&render(r, &solid([0.0; 3], keyed), 20), 32, 32), [128; 3], 1);
    near(px(&render(r, &solid([0.0; 3], keyed), 0), 32, 32), [0; 3], 0);
}

#[test]
fn hsl_shifts_one_hue_band() {
    gpu!(r);
    let reds = r#"{"kind":"hsl","params":{"reds":[120,0,0]}}"#;
    near(one(r, [1.0, 0.0, 0.0], reds), [0, 255, 0], 1);
    near(one(r, [0.0, 0.0, 1.0], reds), [0, 0, 255], 0);
    near(one(r, [0.5; 3], reds), [128; 3], 1);
    // Saturation -1 on the blues: blue becomes grey of the same lightness.
    near(one(r, [0.0, 0.0, 1.0], r#"{"kind":"hsl","params":{"blues":[0,-1,0]}}"#), [128; 3], 1);
    // Lightness +1 on the greens: green becomes white; -1 black.
    near(one(r, [0.0, 1.0, 0.0], r#"{"kind":"hsl","params":{"greens":[0,0,1]}}"#), [255; 3], 1);
    near(one(r, [0.0, 1.0, 0.0], r#"{"kind":"hsl","params":{"greens":[0,0,-1]}}"#), [0; 3], 1);
    // Half way between two band centres (yellow at 60, green at 120: hue 90) each band counts half.
    near(one(r, [0.5, 1.0, 0.0], r#"{"kind":"hsl","params":{"yellows":[0,0,-1]}}"#), [64, 128, 0], 2);
    near(one(r, [0.5, 1.0, 0.0], r#"{"kind":"hsl","params":{"yellows":[0,0,-1],"greens":[0,0,-1]}}"#), [0; 3], 1);
}

#[test]
fn colorama_cycles_a_palette() {
    gpu!(r);
    let grey = |extra: &str| format!(r#"{{"kind":"colorama","params":{{{extra}}},"options":{{"palette":"grey"}}}}"#);
    near(one(r, [0.0; 3], &grey("")), [0; 3], 1);
    near(one(r, [0.5; 3], &grey("")), [127; 3], 1);
    near(one(r, [1.0; 3], &grey("")), [255; 3], 1);
    // phase 180 moves black half way round.
    near(one(r, [0.0; 3], &grey(r#""phase":180"#)), [127; 3], 1);
    // Two cycles: a quarter is half way through the first.
    near(one(r, [0.25; 3], &grey(r#""cycles":2"#)), [127; 3], 1);
    // The hue cycle: black is red, display luminance 0.5 (pure red) is cyan.
    near(one(r, [0.0; 3], r#"{"kind":"colorama"}"#), [255, 0, 0], 1);
    near(one(r, [1.0, 0.0, 0.0], r#"{"kind":"colorama"}"#), [0, 255, 255], 4);
    // A custom palette of linear colours with hue as the input: green (hue 1/3) takes the second colour.
    let custom = r#"{"kind":"colorama","params":{"colors":[1,1,1,0,0,1,1,1,0]},"options":{"palette":"custom","input":"hue"}}"#;
    near(one(r, [0.0, 1.0, 0.0], custom), [0, 0, 255], 2);
    // Fire: white is white, at the end of the cycle.
    near(one(r, [1.0; 3], r#"{"kind":"colorama","options":{"palette":"fire"}}"#), [255; 3], 1);
}

#[test]
fn replace_color_and_leave_color() {
    gpu!(r);
    near(one(r, [1.0, 0.0, 0.0], r#"{"kind":"replace_color"}"#), [0, 0, 255], 1);
    near(one(r, [0.0, 1.0, 0.0], r#"{"kind":"replace_color"}"#), [0, 255, 0], 0);
    // Only the hue changes unless asked: red to a dark blue is still a bright blue.
    near(one(r, [1.0, 0.0, 0.0], r#"{"kind":"replace_color","params":{"to":[0,0,0.2]}}"#), [0, 0, 255], 1);
    near(one(r, [1.0, 0.0, 0.0], r#"{"kind":"replace_color","params":{"to":[0,0,0.214]},"options":{"change":"all"}}"#), [0, 0, 128], 1);
    // Transforming shifts by to - from: red to green is +1/3 turn, and a slightly
    // orange red goes the same way.
    let got = one(r, [1.0, 0.1, 0.0], r#"{"kind":"replace_color","params":{"to":[0,1,0]},"options":{"by":"transforming"}}"#);
    assert!(got[1] == 255 && got[0] < 40 && got[2] < 40, "{got:?}");
    // leave_color keeps red and greys the rest (to its linear luminance).
    near(one(r, [1.0, 0.0, 0.0], r#"{"kind":"leave_color"}"#), [255, 0, 0], 0);
    near(one(r, [0.0, 1.0, 0.0], r#"{"kind":"leave_color"}"#), [byte(0.7152); 3], 1);
    near(one(r, [0.0, 0.0, 1.0], r#"{"kind":"leave_color","options":{"match":"rgb"}}"#), [byte(0.0722); 3], 1);
}

#[test]
fn solarize_bleach_bypass_technicolor_haze() {
    gpu!(r);
    let sol = r#"{"kind":"solarize"}"#;
    near(one(r, [1.0; 3], sol), [0; 3], 0);
    near(one(r, [0.0; 3], sol), [0; 3], 0);
    near(one(r, [0.8; 3], sol), [51; 3], 1);
    near(one(r, [0.2; 3], sol), [51; 3], 1);
    near(one(r, [0.0; 3], r#"{"kind":"solarize","options":{"invert":"below"}}"#), [255; 3], 0);
    near(one(r, [0.8; 3], r#"{"kind":"solarize","options":{"invert":"below"}}"#), [204; 3], 1);
    // Bleach bypass: multiply by the luminance in the shadows, screen in the highlights.
    near(one(r, [0.3; 3], r#"{"kind":"bleach_bypass"}"#), [d8(2.0 * 0.3 * 0.3); 3], 1);
    near(one(r, [0.8; 3], r#"{"kind":"bleach_bypass"}"#), [d8(1.0 - 2.0 * 0.2 * 0.2); 3], 1);
    // Technicolor: green and blue averaged.
    near(one(r, [1.0, 0.0, 1.0], r#"{"kind":"technicolor"}"#), [255, 128, 128], 1);
    near(one(r, [0.2, 0.6, 1.0], r#"{"kind":"technicolor","params":{"amount":0.5}}"#), [51, d8(0.7), d8(0.9)], 1);
    // Haze: (value - d) / (1 - d) on display values, d growing down the frame with slope.
    near(one(r, [0.5; 3], r#"{"kind":"haze"}"#), [d8(0.3 / 0.8); 3], 1);
    near(one(r, [0.5; 3], r#"{"kind":"haze","params":{"distance":-0.5}}"#), [d8(1.0 / 1.5); 3], 1);
    let sloped = render(r, &solid([0.5; 3], r#"{"kind":"haze","params":{"distance":0,"slope":0.5}}"#), 0);
    let at = |y: f64| [d8((0.5 - 0.5 * y) / (1.0 - 0.5 * y)); 3];
    near(px(&sloped, 32, 0), at(0.5 / 64.0), 1);
    near(px(&sloped, 32, 63), at(63.5 / 64.0), 1);
}

#[test]
fn gradient_map_stops_run_through_their_colours() {
    gpu!(r);
    let stops = r#"{"kind":"gradient_map","params":{"stops":[0,0,0,1,0,0,1,1,1]},"options":{"mode":"stops"}}"#;
    near(one(r, [0.0; 3], stops), [0; 3], 0);
    near(one(r, [0.5; 3], stops), [255, 0, 0], 1);
    near(one(r, [1.0; 3], stops), [255; 3], 0);
    near(one(r, [0.25; 3], stops), [128, 0, 0], 1);
    // Eight thermal stops render a picture in many colours; one colour twice is flat.
    let thermal = r#"{"kind":"gradient_map","params":{"stops":[0,0,0, 0.06,0,0.13, 0,0,1, 0,1,1, 0,1,0, 1,1,0, 1,0.21,0, 1,0,0]},"options":{"mode":"stops"}}"#;
    let pic = gradient("fxc-gradient64.png", 64);
    assert!(colours(&render(r, &picture(64, 64, &pic, thermal), 0)) > 50);
    near(one(r, [0.3; 3], r#"{"kind":"gradient_map","params":{"stops":[0,0,1,0,0,1]},"options":{"mode":"stops"}}"#), [0, 0, 255], 0);
}

