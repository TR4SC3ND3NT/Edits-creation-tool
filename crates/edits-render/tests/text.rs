//! Text layers, measured by the box of the pixels they cover on a black
//! frame. The font is Inter, built into the renderer (and, by path, the same
//! file in `assets/fonts`). Skipped without an adapter.

mod common;

use common::*;
use edits_core::Project;
use image::RgbaImage;
use std::path::Path;

const W: u32 = 320;
const H: u32 = 180;

/// A 320x180 project with `layers` over black.
fn text_project(layers: &str) -> Project {
    let text = format!(r#"{{"format":"edits.project/1","settings":{{"width":{W},"height":{H},"fps":10,"duration":2}},"layers":[{layers}]}}"#);
    Project::from_json(&text).unwrap_or_else(|e| panic!("bad test project: {e}\n{text}"))
}

fn draw(p: &Project, scale: f32) -> Option<RgbaImage> {
    let mut guard = renderer()?;
    let r = guard.as_mut().unwrap();
    Some(render_in(r, p, &dir(), 0, scale))
}

/// The box (left, top, right, bottom; inclusive) of the pixels brighter than
/// half, within rows `rows`.
fn coverage(img: &RgbaImage, rows: std::ops::Range<u32>) -> Option<[u32; 4]> {
    let mut b: Option<[u32; 4]> = None;
    for y in rows {
        for x in 0..img.width() {
            let p = img.get_pixel(x, y).0;
            if p[0].max(p[1]).max(p[2]) > 127 {
                b = Some(match b {
                    None => [x, y, x, y],
                    Some(b) => [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)],
                });
            }
        }
    }
    b
}

fn all(img: &RgbaImage) -> [u32; 4] {
    coverage(img, 0..img.height()).expect("the text drew something")
}

/// The covered row bands, top to bottom.
fn bands(img: &RgbaImage) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for y in 0..img.height() {
        if coverage(img, y..y + 1).is_some() {
            match out.last_mut() {
                Some(last) if last.1 + 1 == y => last.1 = y,
                _ => out.push((y, y)),
            }
        }
    }
    out
}

fn centre(b: [u32; 4]) -> [f64; 2] {
    [(b[0] + b[2] + 1) as f64 / 2.0, (b[1] + b[3] + 1) as f64 / 2.0]
}

#[test]
fn capitals_are_centred_on_the_position() {
    let Some(img) = draw(&text_project(r#"{"id":"t","kind":"text","text":"HOH","size":20}"#), 1.0) else { return };
    let c = centre(all(&img));
    assert!((c[0] - W as f64 / 2.0).abs() <= 1.0 && (c[1] - H as f64 / 2.0).abs() <= 1.0, "{c:?}");
    // The layer's position moves it (composition units: the frame is 1 high).
    let Some(img) = draw(&text_project(r#"{"id":"t","kind":"text","text":"HOH","size":20,"transform":{"position":[0.25,-0.1,0]}}"#), 1.0) else { return };
    let moved = centre(all(&img));
    assert!((moved[0] - c[0] - 0.25 * H as f64).abs() <= 1.0 && (moved[1] - c[1] + 0.1 * H as f64).abs() <= 1.0, "{moved:?}");
}

#[test]
fn lines_align_left_centre_and_right() {
    for (align, edge) in [("left", 0), ("center", 1), ("right", 2)] {
        let layer = format!(r#"{{"id":"t","kind":"text","text":"H\nHHHH","size":16,"align":"{align}"}}"#);
        let Some(img) = draw(&text_project(&layer), 1.0) else { return };
        let rows = bands(&img);
        assert_eq!(rows.len(), 2, "{align}: {rows:?}");
        let a = coverage(&img, rows[0].0..rows[0].1 + 1).unwrap();
        let b = coverage(&img, rows[1].0..rows[1].1 + 1).unwrap();
        assert!(b[2] - b[0] > 3 * (a[2] - a[0]), "{a:?} {b:?}");
        let (pa, pb) = match edge {
            0 => (a[0] as f64, b[0] as f64),
            1 => (centre(a)[0], centre(b)[0]),
            _ => (a[2] as f64, b[2] as f64),
        };
        assert!((pa - pb).abs() <= 1.0, "{align}: {a:?} {b:?}");
        // The block is centred: the longest line's middle is the frame's.
        assert!((centre(b)[0] - W as f64 / 2.0).abs() <= 1.0, "{align}: {b:?}");
        assert!((centre([0, a[1], 0, b[3]])[1] - H as f64 / 2.0).abs() <= 6.0, "{align}: {a:?} {b:?}");
    }
}

#[test]
fn a_stroke_widens_the_coverage() {
    let plain = r#"{"id":"t","kind":"text","text":"EDIT","size":20,"fill":[1,1,1]}"#;
    let stroked = r#"{"id":"t","kind":"text","text":"EDIT","size":20,"fill":[1,1,1],"stroke":{"color":[1,0,0],"width":2}}"#;
    let (Some(a), Some(b)) = (draw(&text_project(plain), 1.0), draw(&text_project(stroked), 1.0)) else { return };
    let (a, bb) = (all(&a), all(&b));
    // 2 % of 180 pixels: 3.6 pixels on every side.
    for (side, grow) in [(a[0] as i64 - bb[0] as i64), (bb[2] as i64 - a[2] as i64), (a[1] as i64 - bb[1] as i64), (bb[3] as i64 - a[3] as i64)].into_iter().enumerate() {
        assert!((3..=5).contains(&grow), "side {side}: {a:?} -> {bb:?}");
    }
    // The outline is red and the letters stay white over it.
    let edge = b.get_pixel(bb[0] + 1, (bb[1] + bb[3]) / 2).0;
    assert!(edge[0] > 200 && edge[1] < 40, "{edge:?}");
    let inside = (bb[0]..bb[2]).map(|x| b.get_pixel(x, (bb[1] + bb[3]) / 2).0).filter(|p| p[0] > 240 && p[1] > 240).count();
    assert!(inside > 5, "{inside}");
}

#[test]
fn text_follows_the_output_scale() {
    let p = text_project(r#"{"id":"t","kind":"text","text":"Sharp","size":12}"#);
    let (Some(one), Some(two)) = (draw(&p, 1.0), draw(&p, 2.0)) else { return };
    let (a, b) = (all(&one), all(&two));
    let (wa, wb) = ((a[2] - a[0] + 1) as f64, (b[2] - b[0] + 1) as f64);
    assert!((wb / wa - 2.0).abs() < 0.05, "{wa} {wb}");
    // Rasterised at the output size, not magnified: edges stay a pixel or two wide.
    let row = (b[1] + b[3]) / 2;
    let soft = (b[0]..=b[2]).filter(|&x| (40..215).contains(&two.get_pixel(x, row).0[0])).count();
    let solid = (b[0]..=b[2]).filter(|&x| two.get_pixel(x, row).0[0] >= 215).count();
    assert!(soft * 2 < solid + 8, "soft {soft} solid {solid}");
}

#[test]
fn max_width_wraps_into_two_lines() {
    let one = text_project(r#"{"id":"t","kind":"text","text":"ONE TWO","size":14}"#);
    let two = text_project(r#"{"id":"t","kind":"text","text":"ONE TWO","size":14,"max_width":0.5}"#);
    let (Some(a), Some(b)) = (draw(&one, 1.0), draw(&two, 1.0)) else { return };
    assert_eq!(bands(&a).len(), 1);
    let rows = bands(&b);
    assert_eq!(rows.len(), 2, "{rows:?}");
    // 1.2 lines apart: 14 % of 180 pixels times 1.2.
    let gap = rows[1].1 as f64 - rows[0].1 as f64;
    assert!((gap - 0.14 * 1.2 * H as f64).abs() <= 1.5, "{gap}");
    assert!(all(&b)[2] - all(&b)[0] < all(&a)[2] - all(&a)[0]);
}

#[test]
fn frames_are_deterministic() {
    let p = text_project(
        r#"{"id":"t","kind":"text","text":"SAY MY\nNAME","size":15,"weight":800,"stroke":{"color":[0,0,0],"width":0.6},
            "transform":{"position":{"keys":[{"t":0,"v":[-0.2,0.05,0]},{"t":2,"v":[0.3,-0.1,0]}]},"rotation":{"keys":[{"t":0,"v":[0,0,0]},{"t":2,"v":[20,30,10]}]}}}"#,
    );
    let other = text_project(r#"{"id":"u","kind":"text","text":"something else entirely","size":9}"#);
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    let frames: Vec<RgbaImage> = (0..4).map(|f| render_in(r, &p, &dir(), f * 5, 1.0)).collect();
    render_in(r, &other, &dir(), 0, 0.5);
    for (i, f) in frames.iter().enumerate() {
        let again = render_in(r, &p, &dir(), i as u64 * 5, 1.0);
        assert!(again.as_raw() == f.as_raw(), "frame {} differs on a second render", i * 5);
        assert!(coverage(f, 0..H).is_some(), "frame {} is empty", i * 5);
    }
    assert!(frames[0].as_raw() != frames[3].as_raw());
}

#[test]
fn fonts_are_files_next_to_the_project() {
    let inter = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/InterVariable.ttf");
    let by_path = text_project(&format!(r#"{{"id":"t","kind":"text","text":"Ag","size":30,"font":{:?}}}"#, inter.display().to_string()));
    let built_in = text_project(r#"{"id":"t","kind":"text","text":"Ag","size":30}"#);
    let (Some(a), Some(b)) = (draw(&by_path, 1.0), draw(&built_in, 1.0)) else { return };
    assert!(a.as_raw() == b.as_raw(), "the built-in font is the Inter of assets/fonts");
    let missing = text_project(r#"{"id":"t","kind":"text","text":"Ag","font":"fonts/none.ttf"}"#);
    let Some(mut guard) = renderer() else { return };
    let e = guard.as_mut().unwrap().prepare(&missing, &dir()).unwrap_err();
    assert!(format!("{e:#}").contains("font not found"), "{e:#}");
}
