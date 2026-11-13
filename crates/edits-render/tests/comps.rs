//! Precomps: a comp layer looks like its layers inline, its time runs by
//! offset, speed, loop and reverse, comps nest, and a comp has its own size
//! and background. Skipped without an adapter.

mod common;

use common::*;
use edits_core::Project;
use image::RgbaImage;

/// A 96x64, 10 fps, 4 s project in seconds over black.
fn project(slots: &str, comps: &str, layers: &str) -> Project {
    let text = format!(
        r#"{{"format":"edits.project/1","settings":{{"width":96,"height":64,"fps":10,"duration":4}},
            "slots":{{{slots}}},"comps":{{{comps}}},"layers":[{layers}]}}"#
    );
    Project::from_json(&text).unwrap_or_else(|e| panic!("bad test project: {e}\n{text}"))
}

fn frames(p: &Project, list: &[u64], scale: f32) -> Option<Vec<RgbaImage>> {
    let mut guard = renderer()?;
    let r = guard.as_mut().unwrap();
    Some(list.iter().map(|&f| render_in(r, p, &dir(), f, scale)).collect())
}

fn solid(id: &str, color: &str, extra: &str) -> String {
    format!(r#"{{"id":"{id}","kind":"solid","color":{color}{extra}}}"#)
}

#[test]
fn a_comp_layer_equals_its_layers_inline() {
    let slot = format!(r#""q":{{"path":"{}"}}"#, quadrants("comp-quadrants.png", 48, 32));
    let inner = [
        solid("bg", "[0.2,0.3,0.4,1]", ""),
        r#"{"id":"pic","kind":"image","slot":"q","transform":{"scale":0.5,"rotation":{"keys":[{"t":0,"v":[0,0,0]},{"t":2,"v":[0,0,90]}]},"position":[0.3,0.1,0]}}"#.to_string(),
        solid("box", "[1,1,1,0.5]", r#","transform":{"scale":0.25,"position":[-0.4,0,0]},"blend":"screen","in":0.5,"out":3"#),
    ]
    .join(",");
    let inline = project(&slot, "", &inner);
    let comp = project(&slot, &format!(r#""all":{{"duration":4,"layers":[{inner}]}}"#), r#"{"id":"c","kind":"comp","comp":"all"}"#);
    let list = [0, 7, 20, 35];
    let Some(a) = frames(&inline, &list, 1.0) else { return };
    let Some(b) = frames(&comp, &list, 1.0) else { return };
    for (i, (a, b)) in a.iter().zip(&b).enumerate() {
        let differ = a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count();
        assert_eq!(differ, 0, "frame {}: {differ} pixels differ", list[i]);
    }
    // Over the background (nothing opaque inside), the comp is the same up to rounding.
    let inner = solid("box", "[1,0.5,0,0.6]", r#","transform":{"scale":0.5}"#);
    let inline = project("", "", &inner);
    let comp = project("", &format!(r#""c":{{"duration":4,"layers":[{inner}]}}"#), r#"{"id":"c","kind":"comp","comp":"c"}"#);
    let (Some(a), Some(b)) = (frames(&inline, &[0], 1.0), frames(&comp, &[0], 1.0)) else { return };
    let worst = a[0].pixels().zip(b[0].pixels()).flat_map(|(x, y)| (0..3).map(move |i| (x[i] as i32 - y[i] as i32).abs())).max().unwrap();
    assert!(worst <= 1, "{worst}");
}

/// A 2 s comp: red for its first second, green for its second.
const RED_GREEN: &str = r#""rg":{"duration":2,"layers":[
    {"id":"r","kind":"solid","color":[1,0,0,1],"out":1},
    {"id":"g","kind":"solid","color":[0,1,0,1],"in":1}]}"#;

/// The colour (`'r'`, `'g'` or `'-'` for black) of the centre pixel of each frame.
fn colours(layer_extra: &str, list: &[u64]) -> Option<String> {
    let p = project("", RED_GREEN, &format!(r#"{{"id":"l","kind":"comp","comp":"rg"{layer_extra}}}"#));
    let imgs = frames(&p, list, 1.0)?;
    Some(
        imgs.iter()
            .map(|img| match px(img, 48, 32) {
                [255, 0, 0] => 'r',
                [0, 255, 0] => 'g',
                [0, 0, 0] => '-',
                other => panic!("unexpected colour {other:?}"),
            })
            .collect(),
    )
}

#[test]
fn comp_time_follows_offset_speed_loop_and_reverse() {
    // Frames at 0.5, 1.5, 2.5 and 3.5 s.
    let at = [5, 15, 25, 35];
    let Some(plain) = colours("", &at) else { return };
    assert_eq!(plain, "rggg", "past its end a comp holds its last instant");
    assert_eq!(colours(r#","loop":true"#, &at).unwrap(), "rgrg");
    assert_eq!(colours(r#","offset":1"#, &at).unwrap(), "gggg");
    assert_eq!(colours(r#","offset":1,"loop":true"#, &at).unwrap(), "grgr");
    assert_eq!(colours(r#","speed":2,"loop":true"#, &at).unwrap(), "gggg");
    assert_eq!(colours(r#","speed":2,"loop":true"#, &[2, 7, 12, 17]).unwrap(), "rgrg");
    assert_eq!(colours(r#","speed":0.5"#, &at).unwrap(), "rrgg");
    assert_eq!(colours(r#","reverse":true"#, &at).unwrap(), "grrr");
    assert_eq!(colours(r#","in":1,"out":3"#, &at).unwrap(), "-rg-");
    assert_eq!(colours(r#","time_remap":{"keys":[{"t":0,"v":1.75},{"t":4,"v":0.25}]}"#, &at).unwrap(), "ggrr");
}

#[test]
fn comps_nest_with_their_own_size_and_background() {
    // inner: 32x32 pixels of blue background with a red square in its middle.
    // outer: the project's size, with inner a quarter of the frame height to the right.
    let comps = r#""inner":{"width":32,"height":32,"duration":4,"background":[0,0,1],
                     "layers":[{"id":"sq","kind":"solid","color":[1,0,0,1],"transform":{"scale":0.5}}]},
                   "outer":{"duration":4,"layers":[{"id":"in","kind":"comp","comp":"inner","transform":{"position":[0.25,0,0]}}]}"#;
    let p = project("", comps, r#"{"id":"out","kind":"comp","comp":"outer"}"#);
    let Some(img) = frames(&p, &[0], 1.0) else { return };
    let img = &img[0];
    // inner spans x 48..80, y 16..48; its red square x 56..72, y 24..40.
    assert_px(img, 64, 32, [255, 0, 0], 0);
    assert_px(img, 50, 18, [0, 0, 255], 0);
    assert_px(img, 78, 46, [0, 0, 255], 0);
    for (x, y) in [(46, 32), (81, 32), (64, 14), (64, 49), (10, 10)] {
        assert_px(img, x, y, [0, 0, 0], 0);
    }
    // Scaled from the root, the whole group scales about the frame's centre.
    let p = project("", comps, r#"{"id":"out","kind":"comp","comp":"outer","transform":{"scale":0.5}}"#);
    let Some(img) = frames(&p, &[0], 1.0) else { return };
    // inner now spans x 48..64, y 24..40.
    assert_px(&img[0], 56, 32, [255, 0, 0], 0);
    assert_px(&img[0], 49, 25, [0, 0, 255], 0);
    assert_px(&img[0], 66, 32, [0, 0, 0], 0);
    // At half scale the comp renders at half its size and lands in the same place.
    let p = project("", comps, r#"{"id":"out","kind":"comp","comp":"outer"}"#);
    let Some(half) = frames(&p, &[0], 0.5) else { return };
    assert_eq!(half[0].dimensions(), (48, 32));
    assert_px(&half[0], 32, 16, [255, 0, 0], 0);
    assert_px(&half[0], 25, 9, [0, 0, 255], 0);
    assert_px(&half[0], 22, 16, [0, 0, 0], 0);
}

#[test]
fn a_comp_takes_masks_effects_and_opacity_as_one_picture() {
    let comps = r#""two":{"duration":4,"layers":[
        {"id":"a","kind":"solid","color":[1,1,1,1]},
        {"id":"b","kind":"solid","color":[1,1,1,1],"transform":{"opacity":0.5}}]}"#;
    // The comp's opacity applies once to the flattened group, not to each layer.
    let p = project("", comps, r#"{"id":"c","kind":"comp","comp":"two","transform":{"opacity":0.5},"masks":[{"shape":"rect","size":[0.75,1]}]}"#);
    let Some(img) = frames(&p, &[0], 1.0) else { return };
    let half = byte(0.5);
    assert_px(&img[0], 48, 32, [half, half, half], 1);
    // The mask cuts the group at x 24..72.
    assert_px(&img[0], 20, 32, [0, 0, 0], 0);
    assert_px(&img[0], 75, 32, [0, 0, 0], 0);
    // A hidden comp layer works as a track matte: green shows where the comp's square is.
    let comps = r#""sq":{"duration":4,"layers":[{"id":"w","kind":"solid","color":[1,1,1,1],"transform":{"scale":0.5}}]}"#;
    let layers = [
        solid("red", "[1,0,0,1]", ""),
        r#"{"id":"m","kind":"comp","comp":"sq","visible":false}"#.to_string(),
        solid("green", "[0,1,0,1]", r#","matte":{"layer":"m"}"#),
    ];
    let p = project("", comps, &layers.join(","));
    let Some(img) = frames(&p, &[0], 1.0) else { return };
    assert_px(&img[0], 48, 32, [0, 255, 0], 0);
    assert_px(&img[0], 10, 32, [255, 0, 0], 0);
}

#[test]
fn a_still_comp_costs_one_motion_blur_sample() {
    let comps = r#""still":{"duration":4,"layers":[{"id":"a","kind":"solid","color":[1,0,0,1],"transform":{"scale":0.5}}]},
                   "moving":{"duration":4,"layers":[{"id":"a","kind":"solid","color":[1,0,0,1],"transform":{"position":{"keys":[{"t":0,"v":[0,0,0]},{"t":4,"v":[1,0,0]}]}}}]}"#;
    let text = |comp: &str| {
        let p = project("", comps, &format!(r#"{{"id":"c","kind":"comp","comp":"{comp}"}}"#));
        let mut doc: serde_json::Value = serde_json::from_str(&p.to_json()).unwrap();
        doc["settings"]["motion_blur"] = serde_json::json!({ "samples": 4, "shutter": 0.5 });
        Project::from_json(&doc.to_string()).unwrap()
    };
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    for (comp, want) in [("still", 1), ("moving", 4)] {
        let p = text(comp);
        r.prepare(&p, &dir()).unwrap();
        r.render_frame(3, 1.0).unwrap();
        assert_eq!(r.stats().rendered_samples, want, "{comp}");
    }
}

#[test]
fn a_comp_that_contains_itself_fails_to_render_with_a_message() {
    let p = project("", r#""a":{"duration":4,"layers":[{"id":"b","kind":"comp","comp":"a"}]}"#, r#"{"id":"c","kind":"comp","comp":"a"}"#);
    let Some(mut guard) = renderer() else { return };
    let r = guard.as_mut().unwrap();
    r.prepare(&p, &dir()).unwrap();
    let e = format!("{:#}", r.render_frame(0, 1.0).unwrap_err());
    assert!(e.contains("contains itself"), "{e}");
}
