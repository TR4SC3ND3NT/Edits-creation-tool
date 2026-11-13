//! Precomps: named compositions (`project.comps`) that `comp` layers draw as
//! one flat picture.
//!
//! A comp is a little project of its own: a size (the project's by default),
//! a length, an optional background and a stack of layers of any kind,
//! including other comp layers. A `comp` layer renders its comp at the comp
//! time that its own time maps to (the same `speed`, `time_remap`, `loop` and
//! `reverse` as a video layer, plus an `offset`), then treats the result like
//! an image: transform, masks, effects, matte, blend and transition apply to
//! it as a whole.
//!
//! Times inside a comp (its layers' `in` and `out`, their keys) are comp
//! time, in the project's time base, starting at 0. Space inside a comp is in
//! units of the comp's height, like the project's.

use crate::check::{Finding, Severity};
use crate::eval::Evaluator;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A composition: `project.comps.<name>`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Comp {
    /// Pixels at the project's size; `None` is the project's width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// `None` is the project's height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Length in the project's time base. Comp layers without `out` end here.
    pub duration: f64,
    /// An opaque background (sRGB); `None` leaves the comp transparent
    /// where none of its layers draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<[f32; 3]>,
    /// Bottom to top, like the project's layers.
    #[serde(default)]
    pub layers: Vec<Layer>,
}

/// A `comp` layer: which comp, and how its time runs.
///
/// Comp time is `offset` at the layer's `in` and runs at `speed` (animated,
/// it is integrated); `time_remap` instead gives the comp time to show at
/// each instant. Past the comp's end the last instant holds, unless `loop`;
/// `reverse` plays it backwards from the end.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CompLayer {
    /// A key of `project.comps`.
    pub comp: String,
    /// Playback rate: 1 is real time. Not negative (use `reverse`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<Prop>,
    /// Comp time (project time base) to show, as a function of project time.
    /// Overrides `speed` and `offset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_remap: Option<Prop>,
    /// Start again from comp time 0 after the comp's end.
    #[serde(default, rename = "loop", skip_serializing_if = "is_false")]
    pub looped: bool,
    /// Play the comp backwards: the layer starts on its last instant.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reverse: bool,
    /// Comp time at the layer's `in` (project time base).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
}

fn is_false(v: &bool) -> bool {
    !*v
}
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Project {
    /// The project's layers, then the layers of every comp (in name order):
    /// everything that may be drawn, for loading what layers use.
    pub fn all_layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers.iter().chain(self.comps.values().flat_map(|c| c.layers.iter()))
    }

    /// Comp `name` as a project of its own: its size (the project's by
    /// default), duration, layers, no motion blur, the default camera, and
    /// the project's slots, time base and comps. Renderer and `check` both
    /// see a comp this way.
    pub fn comp_project(&self, name: &str) -> Option<Project> {
        let c = self.comps.get(name)?;
        let mut settings = self.settings.clone();
        settings.width = c.width.unwrap_or(settings.width);
        settings.height = c.height.unwrap_or(settings.height);
        settings.duration = c.duration;
        settings.background = c.background.unwrap_or([0.0; 3]);
        settings.motion_blur = MotionBlur::default();
        let mut p = Project { layers: Vec::new(), markers: Vec::new(), audio_clips: Vec::new(), audio: None, ..self.clone() };
        p.settings = settings;
        p.camera = Camera::default();
        p.layers = c.layers.clone();
        Some(p)
    }

    /// The comps that `layers` draw directly (enabled or not), in order, once each.
    pub fn comps_used(layers: &[Layer]) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for l in layers {
            if let LayerKind::Comp(c) = &l.kind {
                if !out.contains(&c.comp.as_str()) {
                    out.push(&c.comp);
                }
            }
        }
        out
    }
}

impl Evaluator<'_> {
    /// The comp time a comp layer shows at project time `t` (of the stack it
    /// is in), for a comp `duration` long: `time_remap` at the layer's time,
    /// or `offset` plus the speed integrated from the layer's `in`; then
    /// looped, held at the last instant, or reversed. Always in
    /// `[0, duration)`, so the comp's layers that run to its end are visible.
    pub fn comp_time(&self, layer: &Layer, c: &CompLayer, t: f64, duration: f64) -> f64 {
        let p = self.project;
        let raw = match &c.time_remap {
            Some(remap) => self.num(remap, self.layer_time(layer, t)),
            None => {
                let per_second = p.from_seconds(1.0) - p.from_seconds(0.0);
                c.offset + self.speed_seconds(layer, c.speed.as_ref(), t) * per_second
            }
        };
        let pos = if raw.is_finite() { raw } else { 0.0 };
        if !(duration.is_finite() && duration > 0.0) {
            return pos.max(0.0);
        }
        // Just short of the end: the comp's layers end at `duration`.
        let last = (duration - 1e-6).max(0.0);
        let pos = if c.looped {
            // A time a rounding error short of a whole number of loops is the start of the next.
            let pos = (pos - duration * (pos / duration + 1e-9).floor()).clamp(0.0, last);
            if c.reverse {
                last - pos
            } else {
                pos
            }
        } else {
            let pos = pos.clamp(0.0, last);
            if c.reverse {
                last - pos
            } else {
                pos
            }
        };
        pos.clamp(0.0, last)
    }
}

/// Checks of a comp layer in the stack of `p` (the project, or a comp seen
/// as one): the comp exists, its times are numbers and not negative.
pub(crate) fn check_layer(c: &CompLayer, p: &Project, lp: &str, err: &mut impl FnMut(String, String, Option<&str>), warn: &mut Vec<Finding>) {
    if !p.comps.contains_key(&c.comp) {
        let names: Vec<&str> = p.comps.keys().map(String::as_str).collect();
        err(format!("{lp}.comp"), format!("comp \"{}\" is not defined", c.comp), Some(&format!("comps: {}; add it under \"comps\"", if names.is_empty() { "(none)".into() } else { names.join(", ") })));
    }
    for (name, prop) in [("speed", &c.speed), ("time_remap", &c.time_remap)] {
        let Some(prop) = prop else { continue };
        let pp = format!("{lp}.{name}");
        crate::check::check_prop(prop, &pp, err);
        let values: Vec<f64> = match prop {
            Prop::Const(v) => v.to_vec(),
            Prop::Anim(a) => a.keys.iter().flat_map(|k| k.v.to_vec()).chain(a.base.iter().flat_map(Value::to_vec)).collect(),
        };
        if let Some(bad) = values.iter().find(|x| !x.is_finite() || **x < 0.0) {
            let hint = if name == "speed" { "use \"reverse\": true to play backwards" } else { "comp time starts at 0" };
            err(pp, format!("{name} must not be negative, got {bad}"), Some(hint));
        }
    }
    if c.time_remap.is_some() && (c.speed.is_some() || c.offset != 0.0) {
        warn.push(Finding { severity: Severity::Warning, path: format!("{lp}.time_remap"), message: "time_remap overrides speed and offset".into(), hint: Some("remove one of them".into()) });
    }
    if !c.offset.is_finite() {
        err(format!("{lp}.offset"), format!("offset must be a number, got {}", c.offset), None);
    }
}

/// Checks of `project.comps`: sizes and durations, every comp's layers (as
/// `comps.<name>.layers[i:"id"]...`), comps that contain themselves, and
/// slots that only comps use. `out` holds the findings of the project's own
/// stack and gets the rest.
pub(crate) fn check_comps(p: &Project, base_dir: Option<&Path>, out: &mut Vec<Finding>) {
    if p.comps.is_empty() {
        return;
    }
    let mut found: Vec<Finding> = Vec::new();
    let mut err = |path: String, message: String, hint: Option<&str>| found.push(Finding { severity: Severity::Error, path, message, hint: hint.map(str::to_string) });
    // Slots the project's stack does not use, then those no comp uses either.
    let unused = |f: &[Finding]| -> BTreeSet<String> {
        f.iter().filter(|x| x.severity == Severity::Warning && x.message.starts_with("slot is not used")).filter_map(|x| x.path.strip_prefix("slots.")).map(str::to_string).collect()
    };
    let mut unused_everywhere = unused(out);
    let mut nested: Vec<Finding> = Vec::new();
    for (name, c) in &p.comps {
        let cp = format!("comps.{name}");
        if name.is_empty() || name.contains(['.', '[', ']', '/', '"']) {
            err(cp.clone(), format!("comp name {name:?} cannot be used in paths"), Some("use letters, digits, - and _"));
        }
        for (what, v) in [("width", c.width), ("height", c.height)] {
            if v == Some(0) {
                err(format!("{cp}.{what}"), format!("{what} must be positive"), Some("pixels at the project's size; remove it to use the project's"));
            }
        }
        if c.duration.is_nan() || c.duration <= 0.0 {
            err(format!("{cp}.duration"), format!("duration must be positive, got {}", c.duration), Some("the comp's length in the project's time base"));
        }
        if let Some(bg) = c.background.filter(|b| b.iter().any(|v| !(0.0..=1.0).contains(v))) {
            nested.push(Finding { severity: Severity::Warning, path: format!("{cp}.background"), message: format!("{bg:?} is outside 0..1; the renderer clamps it"), hint: None });
        }
        if c.layers.is_empty() {
            nested.push(Finding { severity: Severity::Warning, path: format!("{cp}.layers"), message: "the comp has no layers: it draws nothing".into(), hint: None });
        }
        let Some(sub) = p.comp_project(name) else { continue };
        let findings = crate::check::check_stack(&sub, base_dir);
        let used_here: BTreeSet<String> = p.slots.keys().filter(|s| !unused(&findings).contains(*s)).cloned().collect();
        unused_everywhere.retain(|s| !used_here.contains(s));
        for mut f in findings.into_iter().filter(|f| f.path.starts_with("layers[")) {
            f.path = format!("{cp}.{}", f.path);
            nested.push(f);
        }
    }
    check_cycles(p, &mut err);
    out.retain(|f| !(f.severity == Severity::Warning && f.message.starts_with("slot is not used") && f.path.strip_prefix("slots.").is_some_and(|s| !unused_everywhere.contains(s))));
    // Errors first, like `check` reports them.
    let (errors, warnings): (Vec<Finding>, Vec<Finding>) = found.into_iter().chain(nested).partition(|f| f.severity == Severity::Error);
    let first_warning = out.iter().position(|f| f.severity == Severity::Warning).unwrap_or(out.len());
    out.splice(first_warning..first_warning, errors);
    out.extend(warnings);
}

/// A comp that, through its comp layers, contains itself cannot be drawn.
fn check_cycles(p: &Project, err: &mut impl FnMut(String, String, Option<&str>)) {
    let edges: BTreeMap<&str, Vec<&str>> = p.comps.iter().map(|(n, c)| (n.as_str(), Project::comps_used(&c.layers))).collect();
    // The comps reachable from `from`, with the way there, depth first.
    fn path_to<'a>(edges: &BTreeMap<&'a str, Vec<&'a str>>, from: &'a str, to: &str, seen: &mut BTreeSet<&'a str>) -> Option<Vec<&'a str>> {
        if from == to {
            return Some(vec![from]);
        }
        if !seen.insert(from) {
            return None;
        }
        for next in edges.get(from).into_iter().flatten() {
            if let Some(mut rest) = path_to(edges, next, to, seen) {
                rest.insert(0, from);
                return Some(rest);
            }
        }
        None
    }
    for (name, c) in &p.comps {
        for (i, l) in c.layers.iter().enumerate() {
            let LayerKind::Comp(cl) = &l.kind else { continue };
            if !p.comps.contains_key(&cl.comp) {
                continue;
            }
            if let Some(way) = path_to(&edges, &cl.comp, name, &mut BTreeSet::new()) {
                let chain: Vec<&str> = std::iter::once(name.as_str()).chain(way).collect();
                err(
                    format!("comps.{name}.layers[{i}:\"{}\"].comp", l.id),
                    format!("comps form a cycle: {}", chain.join(" -> ")),
                    Some("a comp cannot contain itself, directly or through other comps"),
                );
            }
        }
    }
}

/// `comp panel speed 0.5 loop offset 2`.
pub(crate) fn describe_layer(c: &CompLayer) -> String {
    let text = |p: &Prop| crate::describe::prop_summary(p).unwrap_or_else(|| crate::describe::const_text(p));
    let mut parts = vec![format!("comp {}", c.comp)];
    if let Some(remap) = &c.time_remap {
        parts.push(format!("remap {}", text(remap)));
    } else {
        if let Some(speed) = &c.speed {
            parts.push(format!("speed {}", text(speed)));
        }
        if c.offset != 0.0 {
            parts.push(format!("offset {}", c.offset));
        }
    }
    for (on, word) in [(c.looped, "loop"), (c.reverse, "reverse")] {
        if on {
            parts.push(word.to_string());
        }
    }
    parts.join(" ")
}

/// The heading line of a comp in `describe`: `panel 960x540, 4 beats, background [0, 0, 0]`.
pub(crate) fn describe_heading(p: &Project, name: &str, c: &Comp, unit: &str) -> String {
    let (w, h) = (c.width.unwrap_or(p.settings.width), c.height.unwrap_or(p.settings.height));
    let bg = c.background.map(|b| format!(", background {b:?}")).unwrap_or_default();
    format!("{name}: {w}x{h}, {} {unit}{bg}", c.duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;

    fn project(comps: &str, layers: &str) -> Project {
        Project::from_json(&format!(
            r#"{{"format":"edits.project/1",
                "settings":{{"width":64,"height":32,"fps":24,"duration":8,"time_base":"beats","bpm":120}},
                "slots":{{"a":{{}},"b":{{}}}},
                "comps":{{{comps}}},
                "layers":[{layers}]}}"#
        ))
        .unwrap()
    }

    fn comp_of(p: &Project, i: usize) -> &CompLayer {
        match &p.layers[i].kind {
            LayerKind::Comp(c) => c,
            other => panic!("not a comp layer: {other:?}"),
        }
    }

    #[test]
    fn comps_round_trip_with_defaults_left_out() {
        let p = project(r#""panel":{"duration":4,"layers":[{"id":"x","kind":"image","slot":"a"}]}"#, r#"{"id":"p","kind":"comp","comp":"panel","loop":true,"in":1}"#);
        let c = comp_of(&p, 0);
        assert!(c.looped && !c.reverse && c.offset == 0.0 && c.speed.is_none());
        assert_eq!(p.comps["panel"].layers[0].id, "x");
        let text = p.to_json();
        assert!(text.contains("\"comps\"") && text.contains("\"loop\": true") && !text.contains("\"offset\""), "{text}");
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(doc["comps"]["panel"].get("width").is_none() && doc["comps"]["panel"].get("background").is_none(), "{text}");
        assert!(!project("", "").to_json().contains("comps"));
        let sub = p.comp_project("panel").unwrap();
        assert_eq!((sub.settings.width, sub.settings.height, sub.settings.duration), (64, 32, 4.0));
        assert_eq!(sub.layers.len(), 1);
        assert_eq!(p.all_layers().count(), 2);
    }

    #[test]
    fn comp_time_follows_offset_speed_loop_and_reverse() {
        let at = |json: &str, t: f64| {
            let p = project(r#""c":{"duration":2,"layers":[]}"#, &format!(r#"{{"id":"l","kind":"comp","comp":"c","in":1{json}}}"#));
            Evaluator::new(&p).comp_time(&p.layers[0], comp_of(&p, 0), t, 2.0)
        };
        let near = |a: f64, b: f64| (a - b).abs() < 1e-5;
        assert!(near(at("", 1.0), 0.0) && near(at("", 1.5), 0.5));
        assert!(near(at("", 0.0), 0.0), "before in: the first instant");
        assert!(near(at("", 5.0), 2.0), "past the end it holds");
        assert!(at("", 5.0) < 2.0);
        assert!(near(at(r#","offset":0.5"#, 1.0), 0.5));
        assert!(near(at(r#","speed":0.5"#, 2.0), 0.5));
        assert!(near(at(r#","speed":2,"offset":0.25"#, 1.5), 1.25));
        // Loop: whole loops land exactly on the start, not a rounding error short of the end.
        assert_eq!(at(r#","loop":true"#, 3.0), 0.0);
        assert!(near(at(r#","loop":true"#, 3.5), 0.5));
        assert!(near(at(r#","loop":true,"speed":2"#, 2.25), 0.5));
        assert!(near(at(r#","reverse":true"#, 1.0), 2.0) && near(at(r#","reverse":true"#, 2.5), 0.5));
        assert!(near(at(r#","reverse":true,"loop":true"#, 3.5), 1.5));
        // A remap gives comp time directly.
        assert!(near(at(r#","time_remap":{"keys":[{"t":1,"v":1.5},{"t":3,"v":0.5}]}"#, 2.0), 1.0));
        // Speed animates in beats: from 1 to 3 over two beats, integrated.
        let ramp = at(r#","speed":{"keys":[{"t":1,"v":1},{"t":2,"v":3}]}"#, 1.5);
        // Half a beat (0.25 s) at a mean speed of 1.5: 0.375 s of comp, 0.75 beats.
        assert!(near(ramp, 0.75), "{ramp}");
    }

    #[test]
    fn check_reports_unknown_comps_cycles_and_nested_layers() {
        let p = project(
            r#""a":{"duration":2,"layers":[{"id":"inner","kind":"comp","comp":"b"},{"id":"x","kind":"image","slot":"nope"}]},
               "b":{"duration":2,"layers":[{"id":"back","kind":"comp","comp":"a"}]},
               "self":{"duration":0,"width":0,"layers":[{"id":"me","kind":"comp","comp":"self"}]},
               "ok":{"duration":2,"layers":[{"id":"pic","kind":"image","slot":"b","effects":[{"kind":"nope"}]}]}"#,
            r#"{"id":"top","kind":"comp","comp":"ok","speed":-1},{"id":"lost","kind":"comp","comp":"missing"}"#,
        );
        let f = check(&p, None);
        let says = |path: &str, needle: &str, sev: Severity| f.iter().any(|x| x.path == path && x.message.contains(needle) && x.severity == sev);
        assert!(says("layers[1:\"lost\"].comp", "comp \"missing\" is not defined", Severity::Error), "{f:#?}");
        assert!(says("layers[0:\"top\"].speed", "must not be negative", Severity::Error));
        assert!(says("comps.a.layers[0:\"inner\"].comp", "comps form a cycle: a -> b -> a", Severity::Error), "{f:#?}");
        assert!(says("comps.b.layers[0:\"back\"].comp", "comps form a cycle: b -> a -> b", Severity::Error));
        assert!(says("comps.self.layers[0:\"me\"].comp", "comps form a cycle: self -> self", Severity::Error));
        assert!(says("comps.self.duration", "duration must be positive", Severity::Error));
        assert!(says("comps.self.width", "width must be positive", Severity::Error));
        assert!(says("comps.a.layers[1:\"x\"].slot", "slot \"nope\" is not defined", Severity::Error));
        assert!(says("comps.ok.layers[0:\"pic\"].effects[0]", "unknown effect \"nope\"", Severity::Error));
        // Slot b is used inside a comp, slot a nowhere.
        assert!(!f.iter().any(|x| x.path == "slots.b" && x.message.contains("not used")), "{f:#?}");
        assert!(says("slots.a", "not used", Severity::Warning));
        // Errors come before warnings.
        let first_warning = f.iter().position(|x| x.severity == Severity::Warning).unwrap();
        assert!(f[first_warning..].iter().all(|x| x.severity == Severity::Warning), "{f:#?}");
        // A check path inside a comp is a pointer project_edit takes.
        assert_eq!(crate::pointer::from_check_path("comps.ok.layers[0:\"pic\"].effects[0]").as_deref(), Some("/comps/ok/layers/@pic/effects/0"));
        let fine = project(r#""c":{"duration":2,"layers":[{"id":"x","kind":"image","slot":"a"},{"id":"y","kind":"image","slot":"b"}]}"#, r#"{"id":"l","kind":"comp","comp":"c"}"#);
        assert!(check(&fine, None).iter().all(|x| x.severity == Severity::Warning && !x.path.starts_with("comps") && !x.message.contains("not used")), "{:#?}", check(&fine, None));
    }

    #[test]
    fn layer_ids_and_mattes_are_per_stack() {
        // The same id in the project and in a comp is fine; a matte looks in its own stack.
        let p = project(
            r#""c":{"duration":2,"layers":[{"id":"x","kind":"solid","color":[1,1,1,1],"visible":false},{"id":"y","kind":"image","slot":"a","matte":{"layer":"x"}},{"id":"z","kind":"image","slot":"b","matte":{"layer":"top"}}]}"#,
            r#"{"id":"x","kind":"comp","comp":"c"},{"id":"top","kind":"solid","color":[1,1,1,1]}"#,
        );
        let f = check(&p, None);
        assert!(!f.iter().any(|x| x.message.contains("duplicate")), "{f:#?}");
        assert!(f.iter().any(|x| x.path == "comps.c.layers[2:\"z\"].matte.layer" && x.message.contains("no layer \"top\"")), "{f:#?}");
        assert!(!f.iter().any(|x| x.path.starts_with("comps.c.layers[1")), "{f:#?}");
    }

    #[test]
    fn edits_reach_layers_inside_comps() {
        let p = project(r#""c":{"duration":2,"layers":[{"id":"x","kind":"image","slot":"a"}]}"#, r#"{"id":"l","kind":"comp","comp":"c"}"#);
        let doc: serde_json::Value = serde_json::from_str(&p.to_json()).unwrap();
        let ops = [
            crate::edit::EditOp::set("/comps/c/layers/@x/transform/scale", serde_json::json!(0.5)),
            crate::edit::EditOp::set("/comps/c/layers/-", serde_json::json!({"id": "y", "kind": "solid", "color": [1, 0, 0, 1]})),
            crate::edit::EditOp::set("/layers/@l/speed", serde_json::json!(2)),
        ];
        let out = crate::edit::apply_ops(&doc, &ops).unwrap();
        let q = Project::from_json(&out.to_string()).unwrap();
        assert_eq!(q.comps["c"].layers.len(), 2);
        assert!(matches!(q.comps["c"].layers[0].transform.scale, Prop::Const(Value::Num(s)) if s == 0.5));
        assert!(comp_of(&q, 0).speed.is_some());
        assert_eq!(crate::pointer::get(&out, "/comps/c/layers/@y/kind").unwrap(), "solid");
        let changes = crate::edit::diff(&doc, &out);
        assert!(changes.iter().any(|c| c.pointer == "/comps/c/layers/@y"), "{changes:?}");
        // Effect files inside comps are listed with the rest (the MCP server confines them).
        let doc = serde_json::json!({ "layers": [], "comps": { "c": { "layers": [ { "id": "x", "effects": [ { "kind": "custom", "source": "fx.wgsl" } ] } ] } } });
        assert_eq!(crate::edit::file_refs(&doc), vec![("/comps/c/layers/@x/effects/0/source".to_string(), "fx.wgsl".to_string())]);
        // A typo inside a comp's layer is reported at its pointer.
        let mut typo: serde_json::Value = serde_json::from_str(&p.to_json()).unwrap();
        typo["comps"]["c"]["layers"][0]["transfrom"] = serde_json::json!({});
        typo["comps"]["c"]["layers"][0]["opacty"] = serde_json::json!(1);
        let f = crate::edit::ignored_key_findings(&typo, &p);
        assert!(f.iter().any(|x| x.path == "/comps/c/layers/@x/opacty"), "{f:#?}");
    }

    #[test]
    fn describe_shows_comps_and_comp_layers() {
        let p = project(
            r#""card":{"width":32,"height":16,"duration":2,"background":[0,0,0],"layers":[{"id":"t","kind":"text","text":"HI","in":0.5}]}"#,
            r#"{"id":"c1","kind":"comp","comp":"card","speed":0.5,"loop":true,"offset":1,"in":2}"#,
        );
        let d = crate::describe::describe(&p);
        assert!(d.contains("comp card speed 0.5 offset 1 loop"), "{d}");
        assert!(d.contains("comps (layers in comp time):\n  card: 32x16, 2 beats, background [0.0, 0.0, 0.0]"), "{d}");
        assert!(d.lines().any(|l| l.starts_with("    t ") && l.contains("0.50..2.00") && l.contains("text \"HI\"")), "{d}");
    }
}
