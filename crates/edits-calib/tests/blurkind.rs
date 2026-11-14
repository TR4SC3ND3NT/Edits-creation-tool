//! The analysis tells the renderer's blur kinds apart and recovers each
//! kind's own parameter.
//!
//! Renders one generated picture with `gaussian_blur`, `directional_blur`
//! (three angles), `zoom_blur` and `spin_blur` at known values, measures each
//! frame against the sharp one with the analysis' own blur and blur-kind
//! code, and checks the kind and the parameter recovered: the Gaussian
//! radius, or for a streak the length (strength, angle) that takes as much
//! detail off the sharp frame (`blurkind::magnitude`). Needs a wgpu adapter (Mesa lavapipe is enough); skips without one.

use edits_analyze::blurkind::{self, param_for};
use edits_analyze::img::Frames;
use edits_analyze::measure::linear_luma_image;
use edits_calib::pictures::{self, Kind};
use edits_calib::render::Stage;
use edits_core::Project;

const SIZE: u32 = 360;

fn project(effect: &str) -> Project {
    let fx = if effect.is_empty() { String::new() } else { format!(r#", "effects": [{effect}]"#) };
    let text = format!(
        r#"{{ "format": "edits.project/1",
            "settings": {{ "width": {SIZE}, "height": {SIZE}, "fps": 25, "duration": 0.04, "time_base": "seconds" }},
            "slots": {{ "a": {{ "path": "a.png" }} }},
            "layers": [ {{ "id": "a", "kind": "image", "slot": "a"{fx} }} ] }}"#
    );
    Project::from_json(&text).expect("test project parses")
}

#[test]
fn renderer_blur_kinds_are_classified_and_converted() {
    let mut stage = match Stage::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: no renderer ({e:#})");
            return;
        }
    };
    let dir = std::env::temp_dir().join(format!("edits-calib-blurkind-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut failures = Vec::new();
    for (pk, seed) in [(Kind::Photo, 3), (Kind::Poster, 5)] {
        pictures::generate(pk, seed, SIZE).save(dir.join("a.png")).unwrap();
        let render = |stage: &mut Stage, e: &str| -> Frames { stage.render(&project(e), &dir, 1.0, Some((0, 1))).unwrap() };
        let sharp = render(&mut stage, "");
        let lum = |f: &Frames| linear_luma_image(f.rgb(0), f.w, f.h);
        let reference = lum(&sharp);
        let cases: Vec<(&str, String, f64)> = vec![
            ("gaussian", r#"{ "kind": "gaussian_blur", "params": { "radius": 1.0 } }"#.into(), 1.0),
            ("directional", r#"{ "kind": "directional_blur", "params": { "length": 4.0, "angle": 0 } }"#.into(), 4.0),
            ("directional", r#"{ "kind": "directional_blur", "params": { "length": 4.0, "angle": 45 } }"#.into(), 4.0),
            ("directional", r#"{ "kind": "directional_blur", "params": { "length": 4.0, "angle": 90 } }"#.into(), 4.0),
            ("directional", r#"{ "kind": "directional_blur", "params": { "length": 9.0, "angle": 30 } }"#.into(), 9.0),
            ("zoom", r#"{ "kind": "zoom_blur", "params": { "strength": 0.15 } }"#.into(), 0.15),
            ("zoom", r#"{ "kind": "zoom_blur", "params": { "strength": 0.35 } }"#.into(), 0.35),
            ("spin", r#"{ "kind": "spin_blur", "params": { "angle": 8 } }"#.into(), 8.0),
            ("spin", r#"{ "kind": "spin_blur", "params": { "angle": 18 } }"#.into(), 18.0),
        ];
        for (kind, e, truth) in cases {
            let f = render(&mut stage, &e);
            let cur = lum(&f);
            let sigma_pct = 100.0 * edits_analyze::blur::measure(&cur, &reference).sigma_px / f.h as f64;
            let d = blurkind::ratios(&blurkind::orientation(&cur, None), &blurkind::orientation(&reference, None));
            let got = blurkind::classify(&d);
            // The parameter: the Gaussian radius itself, or for a streak the
            // length (strength, angle) that takes as much detail off the sharp frame.
            let value = match got {
                Some((k, _, st)) if kind != "gaussian" && k == kind => blurkind::magnitude(kind, st.angle, st.loss, &reference).unwrap_or(0.0),
                _ => param_for(kind, sigma_pct).2,
            };
            eprintln!(
                "{:?} {e}: equivalent sigma {sigma_pct:.3}% -> {kind} parameter {value:.3} (truth {truth}, ratio {:.2}); classified {:?}",
                pk,
                truth / sigma_pct.max(1e-6),
                got.map(|g| (g.0, (g.1 * 100.0).round() / 100.0, g.2.angle.round()))
            );
            match got {
                Some((k, _, st)) if k == kind => {
                    if kind == "directional" {
                        let want = e.split("\"angle\": ").nth(1).and_then(|s| s.trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<f64>().ok()).unwrap_or(0.0);
                        let err = ((st.angle - want + 90.0).rem_euclid(180.0) - 90.0).abs();
                        if err > 15.0 {
                            failures.push(format!("{pk:?} {e}: angle read {:.0}", st.angle));
                        }
                    }
                }
                other => failures.push(format!("{pk:?} {e}: classified {:?}", other.map(|g| g.0))),
            }
            if (value / truth - 1.0).abs() > 0.25 {
                failures.push(format!("{pk:?} {e}: parameter recovered {value:.3}, truth {truth}"));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
