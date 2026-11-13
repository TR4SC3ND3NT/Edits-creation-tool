//! Validation with messages an agent can act on: every finding names the
//! place in the document and, where possible, the fix.

use crate::model::*;
use crate::effects::{ParamSpec, SourceKind};
use crate::{ease, effects};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Serialize, Clone, Debug)]
pub struct Finding {
    pub severity: Severity,
    /// Where in the document, e.g. `layers[3:"cube1"].effects[0].params.radius`.
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Check a project. `base_dir` is the folder that relative asset paths resolve
/// against; pass `None` to skip file-existence checks.
pub fn check(p: &Project, base_dir: Option<&Path>) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut err = |path: String, message: String, hint: Option<&str>| {
        out.push(Finding { severity: Severity::Error, path, message, hint: hint.map(str::to_string) })
    };

    if p.format != FORMAT {
        err("format".into(), format!("unknown format \"{}\"", p.format), Some("use \"edits.project/1\""));
    }
    let s = &p.settings;
    if s.width == 0 || s.height == 0 || !s.width.is_multiple_of(2) || !s.height.is_multiple_of(2) {
        err("settings".into(), format!("width and height must be positive even numbers, got {}x{}", s.width, s.height), None);
    }
    // NaN is not positive either.
    if s.fps.is_nan() || s.fps <= 0.0 {
        err("settings.fps".into(), "fps must be positive".into(), None);
    }
    if s.duration.is_nan() || s.duration <= 0.0 {
        err("settings.duration".into(), "duration must be positive".into(), None);
    }
    if s.time_base == TimeBase::Beats && !s.bpm.is_some_and(|b| b > 0.0) {
        err("settings.bpm".into(), "time_base is \"beats\" but bpm is missing".into(), Some("set settings.bpm, or use time_base \"seconds\""));
    }
    if s.motion_blur.samples == 0 || s.motion_blur.samples > 64 {
        err("settings.motion_blur.samples".into(), "samples must be between 1 and 64".into(), None);
    }

    let mut warn: Vec<Finding> = Vec::new();
    let mut ids = BTreeSet::new();
    let mut used_slots = BTreeSet::new();
    for (i, l) in p.layers.iter().enumerate() {
        let lp = format!("layers[{i}:\"{}\"]", l.id);
        if !ids.insert(l.id.clone()) {
            err(lp.clone(), format!("duplicate layer id \"{}\"", l.id), Some("layer ids must be unique"));
        }
        let end = l.end.unwrap_or(s.duration);
        if end <= l.start {
            err(lp.clone(), format!("out ({end}) must be after in ({})", l.start), None);
        }
        if l.start >= s.duration {
            warn.push(Finding { severity: Severity::Warning, path: lp.clone(), message: format!("layer starts at {} but the project ends at {}", l.start, s.duration), hint: None });
        }
        let mut refs: Vec<(&String, &str)> = Vec::new();
        match &l.kind {
            LayerKind::Image { slot } => refs.push((slot, "slot")),
            LayerKind::Cube { faces, size } => {
                if faces.is_empty() {
                    err(format!("{lp}.faces"), "a cube needs at least one face slot".into(), None);
                }
                refs.extend(faces.iter().map(|f| (f, "faces")));
                if size.iter().any(|v| *v <= 0.0) {
                    err(format!("{lp}.size"), "cube size must be positive".into(), None);
                }
            }
            LayerKind::Solid { .. } | LayerKind::Adjustment => {}
        }
        for (slot, what) in refs {
            used_slots.insert(slot.clone());
            if !p.slots.contains_key(slot) {
                err(format!("{lp}.{what}"), format!("slot \"{slot}\" is not defined"), Some("add it under \"slots\""));
            }
        }
        let t = &l.transform;
        for (name, prop) in [("position", &t.position), ("scale", &t.scale), ("rotation", &t.rotation), ("anchor", &t.anchor), ("opacity", &t.opacity)] {
            check_prop(prop, &format!("{lp}.transform.{name}"), &mut err);
        }
        for (ei, e) in l.effects.iter().enumerate() {
            let ep = format!("{lp}.effects[{ei}]");
            match effects::spec(&e.kind) {
                None => {
                    let kinds: Vec<&str> = effects::EFFECTS.iter().map(|e| e.kind).collect();
                    err(ep.clone(), format!("unknown effect \"{}\"", e.kind), Some(&format!("known effects: {}", kinds.join(", "))));
                }
                Some(spec) => {
                    for (k, v) in &e.params {
                        let pp = format!("{ep}.params.{k}");
                        match spec.param(k) {
                            None => {
                                let names: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
                                err(pp.clone(), format!("\"{}\" has no parameter \"{k}\"", e.kind), Some(&format!("parameters: {}", names.join(", "))));
                            }
                            Some(ps) => check_param_values(ps, v, &pp, &mut err, &mut warn),
                        }
                        check_prop(v, &pp, &mut err);
                    }
                    for (k, v) in &e.options {
                        let op = format!("{ep}.options.{k}");
                        match spec.option(k) {
                            None if spec.options.is_empty() => err(op, format!("\"{}\" has no options", e.kind), Some("remove \"options\"")),
                            None => {
                                let names: Vec<&str> = spec.options.iter().map(|o| o.name).collect();
                                err(op, format!("\"{}\" has no option \"{k}\"", e.kind), Some(&format!("options: {}", names.join(", "))));
                            }
                            Some(o) if !o.values.contains(&v.as_str()) => {
                                err(op, format!("\"{v}\" is not a value of {}'s option \"{k}\"", e.kind), Some(&format!("one of: {} (default {})", o.values.join(", "), o.default())));
                            }
                            Some(_) => {}
                        }
                    }
                    match (spec.source, &e.source) {
                        (SourceKind::Shader, None) => err(ep.clone(), format!("a {} effect needs \"source\"", e.kind), Some("path to a WGSL file relative to the project")),
                        (SourceKind::None, Some(_)) => err(format!("{ep}.source"), format!("\"{}\" takes no \"source\"", e.kind), Some("remove \"source\"")),
                        (SourceKind::Shader, Some(src)) => {
                            if let Some(dir) = base_dir {
                                if !dir.join(src).is_file() {
                                    err(format!("{ep}.source"), format!("shader file not found: {src}"), Some("paths are relative to the project file"));
                                }
                            }
                        }
                        (SourceKind::None, None) => {}
                        (SourceKind::Cube, None) => err(ep.clone(), format!("a {} effect needs \"source\"", e.kind), Some("path to a .cube file relative to the project")),
                        (SourceKind::Cube, Some(src)) => {
                            if let Some(dir) = base_dir {
                                if let Err(msg) = crate::cube::load(&dir.join(src)) {
                                    err(format!("{ep}.source"), format!("lookup table {src}: {msg}"), Some("a .cube file (LUT_3D_SIZE or LUT_1D_SIZE and rows of three numbers), relative to the project file"));
                                }
                            }
                        }
                        (SourceKind::Slot, None) => err(ep.clone(), format!("a {} effect needs \"source\"", e.kind), Some("\"slot:<name>\", a slot of this project")),
                        (SourceKind::Slot, Some(src)) => match src.strip_prefix("slot:") {
                            Some(name) if p.slots.contains_key(name) => {
                                used_slots.insert(name.to_string());
                            }
                            Some(name) => {
                                let names: Vec<&str> = p.slots.keys().map(String::as_str).collect();
                                err(format!("{ep}.source"), format!("no slot \"{name}\""), Some(&format!("slots: {}", if names.is_empty() { "(none)".into() } else { names.join(", ") })));
                            }
                            None => err(format!("{ep}.source"), format!("\"{src}\" is not a slot"), Some("write \"slot:<name>\", naming a slot of this project")),
                        },
                    }
                }
            }
        }
    }
    for (i, l) in p.layers.iter().enumerate() {
        let Some(tr) = &l.transition else { continue };
        let tp = format!("layers[{i}:\"{}\"].transition", l.id);
        match effects::transition(&tr.kind) {
            None => {
                let kinds: Vec<&str> = effects::TRANSITIONS.iter().map(|t| t.kind).collect();
                err(tp.clone(), format!("unknown transition \"{}\"", tr.kind), Some(&format!("known transitions: {}", kinds.join(", "))));
            }
            Some(spec) => {
                for (k, v) in &tr.params {
                    if !spec.params.iter().any(|q| q.name == k) {
                        let names: Vec<&str> = spec.params.iter().map(|q| q.name).collect();
                        err(format!("{tp}.params.{k}"), format!("transition \"{}\" has no parameter \"{k}\"", tr.kind), Some(&format!("parameters: {}", if names.is_empty() { "(none)".into() } else { names.join(", ") })));
                    }
                    check_prop(v, &format!("{tp}.params.{k}"), &mut err);
                }
            }
        }
        let end = l.end.unwrap_or(s.duration);
        if tr.duration.is_nan() || tr.duration <= 0.0 {
            err(format!("{tp}.duration"), "duration must be positive".into(), None);
        } else if l.start + tr.duration > end + 1e-9 {
            err(format!("{tp}.duration"), format!("the transition ({}) is longer than the layer ({})", tr.duration, end - l.start), Some("shorten the transition or move the layer's out"));
        } else {
            // What it comes from: a picture beneath that is still there when the transition ends.
            let covered = p.layers[..i].iter().any(|b| b.enabled && !matches!(b.kind, LayerKind::Adjustment) && b.start <= l.start + 1e-9 && b.end.unwrap_or(s.duration) >= l.start + tr.duration - 1e-9);
            if !covered {
                warn.push(Finding {
                    severity: Severity::Warning,
                    path: tp.clone(),
                    message: format!("no layer beneath \"{}\" lasts until the transition ends at {}: it comes from the background", l.id, l.start + tr.duration),
                    hint: Some("move the outgoing layer's out to the end of the transition (the transition:<kind> presets do this)".into()),
                });
            }
        }
        if !ease::is_known(&tr.ease) {
            err(format!("{tp}.ease"), format!("unknown ease {:?}", tr.ease), Some(&format!("names: {}", ease::NAMES.join(", "))));
        }
    }
    for (name, prop) in [("fov", &p.camera.fov), ("position", &p.camera.position), ("rotation", &p.camera.rotation)] {
        check_prop(prop, &format!("camera.{name}"), &mut err);
    }

    for (name, slot) in &p.slots {
        let sp = format!("slots.{name}");
        match (&slot.path, base_dir) {
            (None, _) => warn.push(Finding { severity: Severity::Warning, path: sp, message: "slot is empty; it renders as a placeholder".into(), hint: Some("set \"path\" to an image file".into()) }),
            (Some(path), Some(dir)) if !dir.join(path).is_file() => {
                err(sp, format!("image not found: {path}"), Some("paths are relative to the project file"))
            }
            _ => {}
        }
        if !used_slots.contains(name) {
            warn.push(Finding { severity: Severity::Warning, path: format!("slots.{name}"), message: "slot is not used by any layer".into(), hint: None });
        }
    }
    if let (Some(a), Some(dir)) = (&p.audio, base_dir) {
        if !dir.join(&a.path).is_file() {
            err("audio.path".into(), format!("audio file not found: {}", a.path), None);
        }
    }
    out.extend(warn);
    out
}

fn check_prop(prop: &Prop, path: &str, err: &mut impl FnMut(String, String, Option<&str>)) {
    let Prop::Anim(a) = prop else { return };
    if a.keys.is_empty() && a.base.is_none() && a.modifiers.is_empty() {
        err(path.into(), "animated property has no keys".into(), Some("give \"keys\", or a constant value"));
    }
    for (i, w) in a.keys.windows(2).enumerate() {
        if w[1].t < w[0].t {
            err(format!("{path}.keys[{}]", i + 1), format!("key times must ascend ({} after {})", w[1].t, w[0].t), None);
        }
    }
    for (i, k) in a.keys.iter().enumerate() {
        if !ease::is_known(&k.ease) {
            err(format!("{path}.keys[{i}].ease"), format!("unknown ease {:?}", k.ease), Some(&format!("names: {}; or [x1, y1, x2, y2]", ease::NAMES.join(", "))));
        }
    }
    for m in &a.modifiers {
        if let Modifier::Shake { amplitude, .. } = m {
            check_prop(amplitude, &format!("{path}.modifiers.shake.amplitude"), err);
        }
    }
}

/// Every value a property can take as written: the constant, the keys and the base.
fn written_values(prop: &Prop) -> Vec<Vec<f64>> {
    match prop {
        Prop::Const(v) => vec![v.to_vec()],
        Prop::Anim(a) => a.keys.iter().map(|k| k.v.to_vec()).chain(a.base.iter().map(Value::to_vec)).collect(),
    }
}

/// A parameter's values against its unit (a colour has three numbers, a
/// position two) and its range (values outside are clamped: a warning).
fn check_param_values(spec: &ParamSpec, prop: &Prop, path: &str, err: &mut impl FnMut(String, String, Option<&str>), warn: &mut Vec<Finding>) {
    let values = written_values(prop);
    if let Some(n) = spec.unit.arity() {
        if let Some(bad) = values.iter().find(|v| v.len() != n && v.len() != 1) {
            let (what, hint) = match spec.unit {
                effects::Unit::Color => ("a linear RGB colour", "give [r, g, b] in 0..1, or one number for a grey"),
                _ => ("a position in composition units", "give [x, y] (height = 1, origin at the centre, y down), or one number for both"),
            };
            err(path.into(), format!("{} is {what} with {n} numbers, not {}", spec.name, bad.len()), Some(hint));
            return;
        }
    }
    if let Some((lo, hi)) = spec.range {
        if let Some(v) = values.iter().flatten().find(|v| !(lo..=hi).contains(*v)) {
            let unit = spec.unit.as_str();
            let unit = if unit.is_empty() { String::new() } else { format!(" ({unit})") };
            let bound = |b: f64| if b.is_infinite() { "any".to_string() } else { format!("{b}") };
            warn.push(Finding {
                severity: Severity::Warning,
                path: path.into(),
                message: format!("{v} is outside {}'s range {}..{}{unit}; the renderer clamps it", spec.name, bound(lo), bound(hi)),
                hint: None,
            });
        }
    }
}

pub fn has_errors(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.severity == Severity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_what_is_wrong_and_where() {
        let p = Project::from_json(
            r#"{"format":"edits.project/1",
                "settings":{"width":64,"height":64,"fps":25,"duration":4,"time_base":"beats"},
                "slots":{"a":{}},
                "layers":[
                  {"id":"x","kind":"image","slot":"missing"},
                  {"id":"x","kind":"adjustment","effects":[{"kind":"glow","params":{"radus":1}},{"kind":"nope"}]}
                ]}"#,
        )
        .unwrap();
        let f = check(&p, None);
        let has = |needle: &str| f.iter().any(|x| x.message.contains(needle));
        assert!(has("bpm is missing"));
        assert!(has("slot \"missing\" is not defined"));
        assert!(has("duplicate layer id"));
        assert!(has("has no parameter \"radus\""));
        assert!(has("unknown effect \"nope\""));
        assert!(f.iter().any(|x| x.severity == Severity::Warning && x.message.contains("not used")));
        assert!(has_errors(&f));
    }

    #[test]
    fn options_units_ranges_and_sources_are_checked() {
        let p = Project::from_json(
            r#"{"format":"edits.project/1",
                "settings":{"width":64,"height":64,"fps":25,"duration":4},
                "layers":[{"id":"fx","kind":"adjustment","effects":[
                  {"kind":"pixelate","options":{"shape":"star"}},
                  {"kind":"pixelate","options":{"shap":"hex"}},
                  {"kind":"twirl","options":{"shape":"hex"}},
                  {"kind":"fade","params":{"color":[1,0]}},
                  {"kind":"twirl","params":{"center":{"keys":[{"t":0,"v":[0,0,0]}]}}},
                  {"kind":"bulge","params":{"amount":3}},
                  {"kind":"halftone","source":"x.cube"},
                  {"kind":"custom","source":"missing.wgsl"},
                  {"kind":"custom"},
                  {"kind":"halftone","options":{"mode":"cmyk","shape":"line"},"params":{"size":2,"center":0}}
                ]}]}"#,
        )
        .unwrap();
        let f = check(&p, Some(Path::new("/nonexistent-dir")));
        let at = |i: usize| -> Vec<&Finding> { f.iter().filter(|x| x.path.starts_with(&format!("layers[0:\"fx\"].effects[{i}]"))).collect() };
        let says = |i: usize, needle: &str, sev: Severity| at(i).iter().any(|x| x.message.contains(needle) && x.severity == sev);
        assert!(says(0, "\"star\" is not a value", Severity::Error) && at(0)[0].hint.as_deref().unwrap().contains("square, hex, triangle, polar"), "{f:#?}");
        assert!(says(1, "has no option \"shap\"", Severity::Error) && at(1)[0].hint.as_deref().unwrap().contains("shape"));
        assert!(says(2, "has no options", Severity::Error));
        assert!(says(3, "linear RGB colour with 3 numbers, not 2", Severity::Error));
        assert!(says(4, "position in composition units with 2 numbers, not 3", Severity::Error));
        assert!(says(5, "outside amount's range -1..1", Severity::Warning));
        assert!(says(6, "takes no \"source\"", Severity::Error));
        assert!(says(7, "shader file not found", Severity::Error));
        assert!(says(8, "needs \"source\"", Severity::Error));
        assert!(says(9, "has no parameter \"center\"", Severity::Error) && at(9).len() == 1, "{:#?}", at(9));
    }

    #[test]
    fn lookup_tables_and_map_slots_are_checked() {
        let dir = std::env::temp_dir().join(format!("edits-check-cube-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ok.cube"), "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
        std::fs::write(dir.join("short.cube"), "LUT_3D_SIZE 2\n0 0 0\n").unwrap();
        let p = Project::from_json(
            r#"{"format":"edits.project/1",
                "settings":{"width":64,"height":64,"fps":25,"duration":4},
                "slots":{"map":{},"other":{}},
                "layers":[{"id":"fx","kind":"adjustment","effects":[
                  {"kind":"lut","source":"ok.cube"},
                  {"kind":"lut","source":"short.cube"},
                  {"kind":"lut","source":"missing.cube"},
                  {"kind":"lut"},
                  {"kind":"displacement_map","source":"slot:map"},
                  {"kind":"displacement_map","source":"slot:nope"},
                  {"kind":"displacement_map","source":"map.png"},
                  {"kind":"displacement_map"},
                  {"kind":"displacement_map","source":"slot:map","options":{"channel_x":"purple"}}
                ]}]}"#,
        )
        .unwrap();
        let f = check(&p, Some(&dir));
        let at = |i: usize| -> Vec<&Finding> { f.iter().filter(|x| x.path.starts_with(&format!("layers[0:\"fx\"].effects[{i}]"))).collect() };
        let says = |i: usize, needle: &str| at(i).iter().any(|x| x.message.contains(needle) && x.severity == Severity::Error);
        assert!(at(0).is_empty() && at(4).is_empty(), "{f:#?}");
        assert!(says(1, "ask for 8 rows"), "{f:#?}");
        assert!(says(2, "cannot read"));
        assert!(says(3, "needs \"source\""));
        assert!(says(5, "no slot \"nope\"") && at(5)[0].hint.as_deref().unwrap().contains("map, other"));
        assert!(says(6, "is not a slot"));
        assert!(says(7, "needs \"source\""));
        assert!(says(8, "\"purple\" is not a value"));
        // A slot used only as a map is used.
        assert!(!f.iter().any(|x| x.path == "slots.map" && x.message.contains("not used")));
        assert!(f.iter().any(|x| x.path == "slots.other" && x.message.contains("not used")));
        std::fs::remove_dir_all(&dir).ok();
    }
}
