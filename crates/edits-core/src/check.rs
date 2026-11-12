//! Validation with messages an agent can act on: every finding names the
//! place in the document and, where possible, the fix.

use crate::model::*;
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
    if s.width == 0 || s.height == 0 || s.width % 2 != 0 || s.height % 2 != 0 {
        err("settings".into(), format!("width and height must be positive even numbers, got {}x{}", s.width, s.height), None);
    }
    if !(s.fps > 0.0) {
        err("settings.fps".into(), "fps must be positive".into(), None);
    }
    if !(s.duration > 0.0) {
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
                        if !spec.params.iter().any(|p| p.name == k) {
                            let names: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
                            err(format!("{ep}.params.{k}"), format!("\"{}\" has no parameter \"{k}\"", e.kind), Some(&format!("parameters: {}", names.join(", "))));
                        }
                        check_prop(v, &format!("{ep}.params.{k}"), &mut err);
                    }
                    if e.kind == "custom" && e.source.is_none() {
                        err(ep.clone(), "a custom effect needs \"source\"".into(), Some("path to a WGSL file relative to the project"));
                    }
                }
            }
            if let (Some(src), Some(dir)) = (&e.source, base_dir) {
                if !dir.join(src).is_file() {
                    err(format!("{ep}.source"), format!("shader file not found: {src}"), None);
                }
            }
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
}
