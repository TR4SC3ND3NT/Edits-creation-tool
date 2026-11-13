//! Pixel tests of the built-in distortions added in WP4: wave_warp, ripple,
//! shockwave, mirror, tile, polar, lens_distortion, displacement_map and
//! turbulent_displace. Every test skips when the machine has no adapter.

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

/// White pixels (red above 128) along row `y`.
fn white_in_row(img: &RgbaImage, y: u32) -> usize {
    (0..img.width()).filter(|x| img.get_pixel(*x, y).0[0] > 128).count()
}

#[test]
fn distortions_at_zero_strength_change_nothing() {
    gpu!(r);
    let pic = gradient("fxd-gradient64.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    let grey = flat("fxd-grey.png", 16, 16, [128, 128, 128, 255]);
    for fx in [
        r#"{"kind":"wave_warp","params":{"amplitude":0,"speed":3}}"#,
        r#"{"kind":"ripple","params":{"amplitude":0}}"#,
        r#"{"kind":"ripple","params":{"radius":0}}"#,
        r#"{"kind":"shockwave","params":{"progress":0}}"#,
        r#"{"kind":"shockwave","params":{"progress":1}}"#,
        r#"{"kind":"shockwave","params":{"strength":0}}"#,
        r#"{"kind":"tile","params":{"tiles":[1,1]}}"#,
        r#"{"kind":"polar","params":{"amount":0}}"#,
        r#"{"kind":"lens_distortion","params":{"amount":0}}"#,
        r#"{"kind":"turbulent_displace","params":{"amount":0}}"#,
    ] {
        assert!(render(r, &picture(64, 64, &pic, fx), 0) == plain, "{fx} changed the picture");
    }
    let map = |fx: &str| picture_with_map(&pic, &grey, fx);
    assert!(render(r, &map(r#"{"kind":"displacement_map","source":"slot:m","params":{"amount":[0,0]}}"#), 0) == plain);
    // A mid-grey map pushes by at most the rounding of 128 / 255 to 0.5.
    let img = render(r, &map(r#"{"kind":"displacement_map","source":"slot:m","params":{"amount":[5,5]}}"#), 0);
    assert!(max_diff(&img, &plain) <= 2, "grey map moved the picture by {}", max_diff(&img, &plain));
    // Channels set to off push nothing.
    let img = render(r, &map(r#"{"kind":"displacement_map","source":"slot:m","params":{"amount":[5,5]},"options":{"channel_x":"off","channel_y":"off"}}"#), 0);
    assert!(img == plain);
}

#[test]
fn wave_warp_pushes_sideways_by_the_wave() {
    gpu!(r);
    let pic = split("fxd-split.png", 32);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    assert_eq!(white_in_row(&plain, 24), 32);
    // Waves travel up (direction 0) and push sideways. A square wave of
    // amplitude 12.5% (8 px) and wavelength 50% (32 px): just above the
    // centre the wave is +1 and each pixel reads 8 px to its left, so the
    // edge moves right; just below it is -1 and the edge moves left.
    let fx = r#"{"kind":"wave_warp","params":{"amplitude":12.5,"wavelength":50,"direction":0},"options":{"shape":"square"}}"#;
    let img = render(r, &picture(64, 64, &pic, fx), 0);
    assert_eq!(white_in_row(&img, 24), 24, "above the centre the edge moves right by 8");
    assert_eq!(white_in_row(&img, 40), 40, "below the centre it moves left by 8");
    // A phase of half a wave swaps them.
    let fx = r#"{"kind":"wave_warp","params":{"amplitude":12.5,"wavelength":50,"direction":0,"phase":180},"options":{"shape":"square"}}"#;
    let img = render(r, &picture(64, 64, &pic, fx), 0);
    assert_eq!((white_in_row(&img, 24), white_in_row(&img, 40)), (40, 24));
    // Waves travelling right (direction 90) push up and down: a vertical edge stays put.
    let fx = r#"{"kind":"wave_warp","params":{"amplitude":12.5,"wavelength":50,"direction":90},"options":{"shape":"square"}}"#;
    let img = render(r, &picture(64, 64, &pic, fx), 0);
    assert!((0..64).all(|y| white_in_row(&img, y) == 32));
    // Pinned at all edges, the push fades to nothing at the frame's border
    // (an eighth of a pixel in the outermost rows) and is full in the middle.
    let fx = r#"{"kind":"wave_warp","params":{"amplitude":12.5,"wavelength":50,"direction":0},"options":{"shape":"square","pin":"all"}}"#;
    let img = render(r, &picture(64, 64, &pic, fx), 0);
    assert_eq!((white_in_row(&img, 0), white_in_row(&img, 63)), (32, 32));
    assert!(white_in_row(&img, 24) < 28 && white_in_row(&img, 40) > 36);
}

#[test]
fn wave_warp_moves_with_time_and_seed_deterministically() {
    gpu!(r);
    let pic = gradient("fxd-gradient64.png", 64);
    let moving = picture(64, 64, &pic, r#"{"kind":"wave_warp","params":{"amplitude":4,"speed":1.3}}"#);
    let a = render(r, &moving, 3);
    assert!(a == render(r, &moving, 3), "the same frame renders the same");
    assert!(a != render(r, &moving, 4), "with speed the waves move");
    let still = picture(64, 64, &pic, r#"{"kind":"wave_warp","params":{"amplitude":4}}"#);
    assert!(render(r, &still, 3) == render(r, &still, 9), "without speed nothing moves");
    let noise = |seed: u32| picture(64, 64, &pic, &format!(r#"{{"kind":"wave_warp","params":{{"amplitude":4,"seed":{seed}}},"options":{{"shape":"noise"}}}}"#));
    assert!(render(r, &noise(1), 0) != render(r, &noise(2), 0), "the noise shape changes with the seed");
    for shape in ["sine", "square", "triangle", "sawtooth", "circle", "semicircle", "uncircle", "noise", "smooth_noise"] {
        let p = picture(64, 64, &pic, &format!(r#"{{"kind":"wave_warp","params":{{"amplitude":4}},"options":{{"shape":"{shape}","pin":"center"}}}}"#));
        assert!(render(r, &p, 0) != render(r, &picture(64, 64, &pic, ""), 0), "{shape}");
    }
}

#[test]
fn ripple_and_shockwave_stay_inside_their_rings() {
    gpu!(r);
    let pic = gradient("fxd-gradient64.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    let outside = |img: &RgbaImage, radius_px: f64| {
        (0..64u32).flat_map(|y| (0..64u32).map(move |x| (x, y))).filter(|(x, y)| ((*x as f64 + 0.5 - 32.0).powi(2) + (*y as f64 + 0.5 - 32.0).powi(2)).sqrt() > radius_px).all(|(x, y)| img.get_pixel(x, y) == plain.get_pixel(x, y))
    };
    // Ripple: radius 25% = 16 px.
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"ripple","params":{"amplitude":3,"wavelength":8,"radius":25}}"#), 0);
    assert!(outside(&img, 17.0), "outside its radius a ripple changes nothing");
    assert!(img != plain);
    // A ripple with speed moves; without, it holds.
    let moving = picture(64, 64, &pic, r#"{"kind":"ripple","params":{"amplitude":3,"radius":40,"speed":2}}"#);
    assert!(render(r, &moving, 1) != render(r, &moving, 2));
    assert!(render(r, &moving, 2) == render(r, &moving, 2));
    // Shockwave: at progress 0.5 the ring is at 0.5 x 50% = 25% (16 px), 10% (6.4 px) wide.
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"shockwave","params":{"progress":0.5,"radius":50,"width":10,"strength":4}}"#), 0);
    assert!(outside(&img, 16.0 + 3.2 + 1.0), "beyond the ring nothing moves");
    for (x, y) in [(32u32, 32u32), (36, 32), (32, 28)] {
        assert_eq!(img.get_pixel(x, y), plain.get_pixel(x, y), "inside the ring nothing moves");
    }
    assert!(img.get_pixel(48, 32) != plain.get_pixel(48, 32), "on the ring the picture moves");
    // On the ring the picture is read nearer the centre: on a white box of
    // half the frame, the ring at its edge pushes the edge outward.
    let plain_box = render(r, &boxed(0.5, ""), 0);
    let pushed = render(r, &boxed(0.5, r#"{"kind":"shockwave","params":{"progress":0.5,"radius":50,"width":10,"strength":4}}"#), 0);
    assert!(white_in_row(&pushed, 32) > white_in_row(&plain_box, 32));
}

#[test]
fn mirror_reflects_one_half_onto_the_other() {
    gpu!(r);
    let pic = gradient("fxd-gradient64.png", 64);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"mirror"}"#), 0);
    for y in 0..64 {
        for x in 0..32 {
            assert_eq!(img.get_pixel(x, y), plain.get_pixel(x, y), "the left half is kept");
            assert_px(&img, 63 - x, y, px(&plain, x, y), 1);
        }
    }
    // 90 degrees: the top is mirrored onto the bottom; a line off-centre moves the mirror.
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"mirror","params":{"angle":90,"center":[0,0.25]}}"#), 0);
    // The line is at y = 48: pixel row 50 shows row 45.
    assert_px(&img, 10, 50, px(&plain, 10, 45), 1);
    assert_eq!(img.get_pixel(10, 40), plain.get_pixel(10, 40));
}

#[test]
fn tile_repeats_the_picture_mirrored_or_not() {
    gpu!(r);
    let q = quadrants("fxd-quad.png", 64, 64);
    let red = [255, 0, 0];
    let green = [0, 255, 0];
    // Two by two copies, one centred: the middle of the frame shows a whole
    // copy from 16 to 48.
    let plain_tiles = render(r, &picture(64, 64, &q, r#"{"kind":"tile","params":{"mirror":0}}"#), 0);
    assert_px(&plain_tiles, 22, 22, red, 0);
    assert_px(&plain_tiles, 41, 22, green, 0);
    // Left of it, the right half of the next copy (plain repeat) ...
    assert_px(&plain_tiles, 9, 22, green, 0);
    // ... or, mirrored, the left half reflected.
    let mirrored = render(r, &picture(64, 64, &q, r#"{"kind":"tile"}"#), 0);
    assert_px(&mirrored, 9, 22, red, 0);
    assert_px(&mirrored, 22, 22, red, 0);
    // An offset moves the grid: a quarter of the frame to the right.
    let moved = render(r, &picture(64, 64, &q, r#"{"kind":"tile","params":{"mirror":0,"offset":[0.25,0]}}"#), 0);
    assert_px(&moved, 38, 22, red, 0);
}

#[test]
fn polar_wraps_the_bottom_edge_into_the_centre() {
    gpu!(r);
    let pic = halves("fxd-halves.png", [255, 0, 0], [0, 0, 255]);
    // to_polar: the centre reads the bottom (blue); 25.6 px out reads 20% from the top (red).
    let img = render(r, &picture(64, 64, &pic, r#"{"kind":"polar"}"#), 0);
    assert_px(&img, 32, 32, [0, 0, 255], 0);
    assert_px(&img, 32, 6, [255, 0, 0], 0);
    assert_px(&img, 6, 32, [255, 0, 0], 0);
    // Rings: equal distances from the centre read the same row.
    assert_eq!(img.get_pixel(32, 16), img.get_pixel(16, 32));
    // to_rect of a disc made by to_polar gives back the halves (away from the seams).
    let p = project("", &format!(r#""slots":{{"a":{{"path":"{pic}"}}}},"layers":[{{"id":"a","kind":"image","slot":"a","effects":[{{"kind":"polar"}},{{"kind":"polar","options":{{"direction":"to_rect"}}}}]}}]"#));
    let back = render(r, &p, 0);
    assert_px(&back, 32, 12, [255, 0, 0], 2);
    assert_px(&back, 32, 52, [0, 0, 255], 2);
}

#[test]
fn lens_distortion_bends_by_the_square_of_the_distance() {
    gpu!(r);
    // A box half the frame wide; amount 0.5 at the corners of a square frame
    // is k = 1 per unit squared, so the box's edge is where d (1 + d^2) = 0.25:
    // d = 0.236, 30.2 px wide. Pincushion: d (1 - d^2) = 0.25, d = 0.270, 34.5 px.
    assert_eq!(white_in_row(&render(r, &boxed(0.5, ""), 0), 32), 32);
    let barrel = render(r, &boxed(0.5, r#"{"kind":"lens_distortion","params":{"amount":0.5}}"#), 0);
    let pincushion = render(r, &boxed(0.5, r#"{"kind":"lens_distortion","params":{"amount":-0.5}}"#), 0);
    assert!((29..=31).contains(&white_in_row(&barrel, 32)), "barrel {}", white_in_row(&barrel, 32));
    assert!((33..=36).contains(&white_in_row(&pincushion, 32)), "pincushion {}", white_in_row(&pincushion, 32));
    // The centre stays, and scale 2 alone doubles the box (it is read at half the distance).
    assert_px(&barrel, 32, 32, [255, 255, 255], 0);
    let zoomed = render(r, &boxed(0.25, r#"{"kind":"lens_distortion","params":{"amount":0,"scale":2}}"#), 0);
    assert!((31..=34).contains(&white_in_row(&zoomed, 32)), "zoomed {}", white_in_row(&zoomed, 32));
}

#[test]
fn displacement_map_pushes_by_the_map_colour() {
    gpu!(r);
    let pic = split("fxd-split.png", 32);
    let plain = render(r, &picture(64, 64, &pic, ""), 0);
    // A white map pushes the full amount: every pixel reads 12.5% (8 px) to
    // the right, so the edge moves 8 px left.
    let white = flat("fxd-white.png", 8, 8, [255, 255, 255, 255]);
    let img = render(r, &picture_with_map(&pic, &white, r#"{"kind":"displacement_map","source":"slot:m","params":{"amount":[12.5,0]}}"#), 0);
    assert_eq!(white_in_row(&img, 20), 40);
    // A black map pushes the other way.
    let black = flat("fxd-black.png", 8, 8, [0, 0, 0, 255]);
    let img = render(r, &picture_with_map(&pic, &black, r#"{"kind":"displacement_map","source":"slot:m","params":{"amount":[12.5,0]}}"#), 0);
    assert_eq!(white_in_row(&img, 20), 24);
    // Channels: a map that is red only pushes with red and not with green.
    let red = flat("fxd-red.png", 8, 8, [255, 0, 128, 255]);
    let img = render(r, &picture_with_map(&pic, &red, r#"{"kind":"displacement_map","source":"slot:m","params":{"amount":[12.5,0]},"options":{"channel_x":"green"}}"#), 0);
    assert_eq!(white_in_row(&img, 20), 24, "green is 0: the edge moves right");
    let img = render(r, &picture_with_map(&pic, &red, r#"{"kind":"displacement_map","source":"slot:m","params":{"amount":[12.5,0]},"options":{"channel_x":"blue"}}"#), 0);
    assert!((white_in_row(&img, 20) as i32 - 32).abs() <= 1, "blue is mid grey: nothing moves");
    // The map is stretched over the frame: a map whose left half is white
    // pushes only the left half of the frame (here down, by 8 px).
    let halves = halves("fxd-halves.png", [255, 0, 0], [0, 0, 255]);
    let left = save("fxd-leftwhite.png", &RgbaImage::from_fn(4, 4, |x, _| if x < 2 { image::Rgba([255, 255, 255, 255]) } else { image::Rgba([128, 128, 128, 255]) }));
    let img = render(r, &picture_with_map(&halves, &left, r#"{"kind":"displacement_map","source":"slot:m","params":{"amount":[0,12.5]},"options":{"channel_x":"off","channel_y":"red"}}"#), 0);
    assert_px(&img, 5, 28, [0, 0, 255], 0);
    assert_px(&img, 60, 28, [255, 0, 0], 0);
    assert_eq!(plain.dimensions(), (64, 64));
}

#[test]
fn turbulent_displace_is_deterministic_and_follows_its_mode() {
    gpu!(r);
    let pic = gradient("fxd-gradient64.png", 64);
    let fx = |extra: &str| picture(64, 64, &pic, &format!(r#"{{"kind":"turbulent_displace","params":{{"amount":6,"size":30{extra}}}}}"#));
    let a = render(r, &fx(""), 0);
    assert!(a == render(r, &fx(""), 5), "nothing in it reads the time");
    assert!(a != render(r, &fx(r#","seed":4"#), 0), "the seed changes it");
    assert!(a != render(r, &fx(r#","evolution":90"#), 0), "evolution changes it");
    // Evolution is smooth: a small step changes the picture a little.
    let near = render(r, &fx(r#","evolution":2"#), 0);
    assert!(max_diff(&a, &near) < 40, "{}", max_diff(&a, &near));
    // horizontal pushes only across: horizontal stripes do not change.
    let stripes = rows("fxd-rows.png");
    let plain = render(r, &picture(64, 64, &stripes, ""), 0);
    let img = render(r, &picture(64, 64, &stripes, r#"{"kind":"turbulent_displace","params":{"amount":10},"options":{"mode":"horizontal"}}"#), 0);
    assert!(img == plain);
    for mode in ["turbulent", "vertical", "twist", "bulge"] {
        let img = render(r, &picture(64, 64, &stripes, &format!(r#"{{"kind":"turbulent_displace","params":{{"amount":10}},"options":{{"mode":"{mode}"}}}}"#)), 0);
        assert!(img != plain, "{mode}");
    }
}

#[test]
fn distortions_are_resolution_independent() {
    gpu!(r);
    let pic = gradient("fxd-gradient128.png", 128);
    for fx in [
        r#"{"kind":"wave_warp","params":{"amplitude":3,"wavelength":20}}"#,
        r#"{"kind":"ripple","params":{"amplitude":2,"wavelength":10}}"#,
        r#"{"kind":"shockwave","params":{"progress":0.4,"strength":5}}"#,
        r#"{"kind":"mirror","params":{"angle":30}}"#,
        r#"{"kind":"tile","params":{"tiles":[3,2]}}"#,
        r#"{"kind":"polar"}"#,
        r#"{"kind":"lens_distortion","params":{"amount":0.4}}"#,
        r#"{"kind":"turbulent_displace","params":{"amount":4,"size":20}}"#,
    ] {
        let worst = scale_difference(r, &picture(256, 256, &pic, fx));
        assert!(worst < 0.04, "{fx}: blocks differ by up to {worst:.3} (linear) between scale 1 and 0.5");
    }
}
