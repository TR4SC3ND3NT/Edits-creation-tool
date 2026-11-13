//! `fit` must bring perturbed magnitudes back to the recipe a reference was
//! rendered from, measured in signature space only: with the reference's own
//! pictures in the slots, and with unrelated pictures.
//!
//! Needs a wgpu adapter (Mesa lavapipe is enough); skips with a note without one.

use edits_calib::fit::{fit, FitOpts};
use edits_calib::harness::analyse;
use edits_calib::pictures::{self, Kind};
use edits_calib::recipe::{generate, CaseSpec};
use edits_calib::render::Stage;
use serde_json::Value;
use std::collections::BTreeMap;

/// Numbers `fit` searches by default, with their true values: exposure peaks and decay rates, blur radii, saturation amounts, scale peaks.
fn magnitudes(doc: &Value) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for (li, l) in doc["layers"].as_array().unwrap().iter().enumerate() {
        if !l["id"].as_str().unwrap_or("").starts_with("shot") {
            continue;
        }
        for (ei, e) in l["effects"].as_array().map(|v| v.as_slice()).unwrap_or(&[]).iter().enumerate() {
            let (kind, name, neutral) = match e["kind"].as_str() {
                Some("exposure") => ("exposure", "stops", 0.0),
                Some("gaussian_blur") => ("blur", "radius", 0.0),
                Some("saturation") => ("saturation", "amount", 1.0),
                _ => continue,
            };
            for (k, key) in e["params"][name]["keys"].as_array().map(|v| v.as_slice()).unwrap_or(&[]).iter().enumerate() {
                let v = key["v"].as_f64().unwrap();
                if (v - neutral).abs() > 1e-6 {
                    out.insert(format!("{kind} /layers/{li}/effects/{ei}/params/{name}/keys/{k}/v"), v);
                }
                if let Some(r) = key["ease"]["rate"].as_f64() {
                    out.insert(format!("{kind}_decay /layers/{li}/effects/{ei}/params/{name}/keys/{k}/ease/rate"), r);
                }
            }
        }
    }
    out
}

/// Move every magnitude away from the truth, deterministically.
fn perturb(doc: &mut Value, truth: &BTreeMap<String, f64>) {
    for (name, v) in truth {
        let (what, ptr) = name.split_once(' ').unwrap();
        let new = match what {
            "exposure" => (v - 1.2).max(0.3),
            "exposure_decay" => (v + 0.2).min(0.85),
            "blur" => v * 0.4,
            "saturation" => v + 0.45,
            _ => *v,
        };
        *doc.pointer_mut(ptr).unwrap() = serde_json::json!((new * 1000.0).round() / 1000.0);
    }
}

struct Errors {
    /// Mean absolute error per group before and after the fit.
    before: BTreeMap<String, f64>,
    after: BTreeMap<String, f64>,
}

fn errors(truth: &BTreeMap<String, f64>, start: &Value, end: &Value) -> Errors {
    let mut acc: BTreeMap<String, (f64, f64, usize)> = BTreeMap::new();
    for (name, t) in truth {
        let (what, ptr) = name.split_once(' ').unwrap();
        let (a, b) = (start.pointer(ptr).unwrap().as_f64().unwrap(), end.pointer(ptr).unwrap().as_f64().unwrap());
        // Blur is held to a relative error.
        let scale = if what == "blur" { t.abs().max(0.3) } else { 1.0 };
        let e = acc.entry(what.to_string()).or_default();
        e.0 += (a - t).abs() / scale;
        e.1 += (b - t).abs() / scale;
        e.2 += 1;
    }
    Errors {
        before: acc.iter().map(|(k, v)| (k.clone(), v.0 / v.2 as f64)).collect(),
        after: acc.iter().map(|(k, v)| (k.clone(), v.1 / v.2 as f64)).collect(),
    }
}

/// Render case `seed`, analyse the render as the reference, write a perturbed
/// project (optionally with other pictures in the slots) and fit it.
fn run(seed: u64, other_pictures: bool) -> Option<(Errors, edits_calib::fit::FitReport)> {
    let mut stage = match Stage::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: no renderer ({e:#})");
            return None;
        }
    };
    let dir = std::env::temp_dir().join(format!("edits-calib-fit-{seed}-{other_pictures}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let case = generate(&CaseSpec { seed, height: 160, audio: false });
    case.write_media(&dir).unwrap();
    let frames = stage.render(&case.project, &dir, 1.0, None).unwrap();
    let reference = analyse(&frames, None, "reference").analysis;
    let mut doc: Value = serde_json::from_str(&case.project.to_json()).unwrap();
    let truth = magnitudes(&doc);
    perturb(&mut doc, &truth);
    if other_pictures {
        // Other kinds of picture, other seeds: nothing in the slots is what the reference showed.
        let other = [Kind::Photo, Kind::Dark, Kind::Poster, Kind::LineArt];
        for (i, p) in case.pictures.iter().enumerate() {
            let kind = other[(other.iter().position(|k| *k == p.kind).unwrap() + 1 + i) % 4];
            pictures::generate(kind, p.seed ^ 0x5eed, 200).save(dir.join(format!("img/other{i}.png"))).unwrap();
            doc["slots"][&p.slot]["path"] = Value::String(format!("img/other{i}.png"));
        }
    }
    let start = doc.clone();
    let path = dir.join("project.json");
    std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    let report = fit(&path, &reference, &FitOpts { budget: 20, ..Default::default() }).unwrap();
    let end: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let errs = errors(&truth, &start, &end);
    eprintln!("seed {seed} other pictures {other_pictures}: before {:?} after {:?}\n{}", errs.before, errs.after, edits_calib::fit::summary(&report));
    let _ = std::fs::remove_dir_all(&dir);
    Some((errs, report))
}

fn check(errs: &Errors, group: &str, tol: f64) {
    if let (Some(b), Some(a)) = (errs.before.get(group), errs.after.get(group)) {
        assert!(*a <= tol || *a <= 0.5 * b, "{group}: mean error {b:.3} before, {a:.3} after (tolerance {tol})");
    }
}

#[test]
fn fit_recovers_perturbed_magnitudes() {
    let Some((errs, report)) = run(3, false) else { return };
    assert!(report.written && report.overall_after > report.overall_before, "{report:?}");
    check(&errs, "exposure", 0.5);
    check(&errs, "exposure_decay", 0.12);
    check(&errs, "blur", 0.35);
    check(&errs, "saturation", 0.15);
}

#[test]
fn fit_works_with_unrelated_pictures() {
    let Some((errs, report)) = run(3, true) else { return };
    assert!(report.written && report.overall_after > report.overall_before, "{report:?}");
    check(&errs, "exposure", 0.6);
    check(&errs, "blur", 0.4);
    check(&errs, "saturation", 0.2);
}
