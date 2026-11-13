//! The presets drawer's logic: the library grouped by tag and searched,
//! the variables being edited, and the change a preset makes, through the
//! same `edits_core::preset` functions as `edits apply` (and the MCP tool
//! `preset_apply`). The window writes that change with
//! `edits_core::edit::apply_with`, so it is one step of the shared history.

use edits_core::preset::{self, Preset, Target, TargetKind, VarType, Variable};
use edits_core::song::SongInfo;
use edits_core::time::TimeCtx;
use serde_json::{json, Map, Value};
use std::path::Path;

/// The order of the groups; tags not listed follow in alphabetical order,
/// transitions come last (there are many, and the inspector has them too).
const TAG_ORDER: [&str; 8] = ["hit", "zoom", "move", "3d", "blur", "speed", "look", "overlay"];

/// Every preset the project can use: built-in, the transitions, and the
/// project's own `presets/` folder.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub presets: Vec<Preset>,
    /// Project presets that could not be read.
    pub problems: Vec<String>,
}

impl Library {
    pub fn load(project_dir: Option<&Path>) -> Self {
        let (presets, problems) = preset::library(project_dir);
        Library { presets, problems }
    }

    pub fn get(&self, name: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.name == name)
    }
}

pub fn is_transition(p: &Preset) -> bool {
    p.name.starts_with("transition:")
}

/// The group a preset is listed under: its first tag.
pub fn group_of(p: &Preset) -> String {
    if is_transition(p) {
        return "transition".into();
    }
    p.tags.first().cloned().unwrap_or_else(|| "other".into())
}

fn group_rank(tag: &str) -> (usize, String) {
    match TAG_ORDER.iter().position(|t| *t == tag) {
        Some(i) => (i, String::new()),
        None if tag == "transition" => (usize::MAX, String::new()),
        None => (TAG_ORDER.len(), tag.to_string()),
    }
}

/// The presets matching `query` (fuzzy, over name, description and tags),
/// grouped by their first tag: (tag, indices into `presets`). With a query,
/// the best matches come first in each group.
pub fn groups(presets: &[Preset], query: &str) -> Vec<(String, Vec<usize>)> {
    let q = query.trim();
    let mut scored: Vec<(usize, i64)> = presets
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            if q.is_empty() {
                return Some((i, 0));
            }
            let name = crate::palette::score(q, &p.name).map(|s| s + 1000);
            let rest = crate::palette::score(q, &format!("{} {}", p.tags.join(" "), p.description));
            name.max(rest).map(|s| (i, s))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, _) in scored {
        let g = group_of(&presets[i]);
        match out.iter_mut().find(|(t, _)| *t == g) {
            Some((_, list)) => list.push(i),
            None => out.push((g, vec![i])),
        }
    }
    if q.is_empty() {
        out.sort_by_key(|(t, _)| group_rank(t));
        for (_, list) in out.iter_mut() {
            list.sort_by(|a, b| presets[*a].name.cmp(&presets[*b].name));
        }
    }
    out
}

/// The variables' values as edited: every variable, starting at its default.
pub fn defaults(p: &Preset) -> Map<String, Value> {
    p.variables.iter().map(|(n, v)| (n.clone(), v.default.clone())).collect()
}

/// The values that differ from the defaults: what `edits apply --var` would be given.
pub fn given(p: &Preset, values: &Map<String, Value>) -> Map<String, Value> {
    values.iter().filter(|(n, v)| p.variables.get(*n).is_some_and(|var| !same(&var.default, v))).map(|(n, v)| (n.clone(), v.clone())).collect()
}

fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => match (a.as_array(), b.as_array()) {
            (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
            _ => a == b,
        },
    }
}

/// `--var` assignments for the values that differ from the defaults, as a person would type them.
pub fn var_line(p: &Preset, values: &Map<String, Value>) -> String {
    given(p, values).iter().map(|(n, v)| format!("{n}={}", edits_core::jsonfmt::inline(v))).collect::<Vec<_>>().join(" ")
}

/// A number variable's drag range and step: its own range, or a sensible one around the default.
pub fn number_range(v: &Variable) -> (f64, f64, f64) {
    let d = v.default.as_f64().unwrap_or(0.0);
    let lo = v.min.unwrap_or(if d >= 0.0 { 0.0 } else { d * 4.0 });
    let hi = v.max.unwrap_or(if d > 0.0 { d * 4.0 } else { (lo + 1.0).max(1.0) });
    let step = if v.ty == VarType::Integer { 1.0 } else { ((hi - lo) / 200.0).max(1e-4) };
    (lo, hi, step)
}

/// What a person can pick as the layer a preset changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetChoice {
    /// The selected layer (or the picture on screen when none is selected).
    Selected,
    /// The picture on screen at the time (`--layer auto`).
    Auto,
    /// An `accents` adjustment layer on top (`--layer fx`).
    Fx,
}

impl TargetChoice {
    pub const ALL: [TargetChoice; 3] = [TargetChoice::Selected, TargetChoice::Auto, TargetChoice::Fx];

    pub fn label(self) -> &'static str {
        match self {
            TargetChoice::Selected => "Layer",
            TargetChoice::Auto => "Screen",
            TargetChoice::Fx => "Accents",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            TargetChoice::Selected => "The selected layer (the picture on screen when no layer is selected)",
            TargetChoice::Auto => "The picture on screen at that time (--layer auto)",
            TargetChoice::Fx => "An `accents` adjustment layer on top, so the effect applies to everything under it (--layer fx)",
        }
    }
}

/// The target `edits apply` would get.
pub fn target(p: &Preset, choice: TargetChoice, selected: Option<&str>) -> Target {
    if p.target == TargetKind::Project {
        return Target::None;
    }
    match (choice, selected) {
        (TargetChoice::Selected, Some(id)) => Target::Layer(id.to_string()),
        (TargetChoice::Selected, None) | (TargetChoice::Auto, _) => Target::Auto,
        (TargetChoice::Fx, _) => Target::Fx,
    }
}

pub fn target_text(t: &Target) -> String {
    match t {
        Target::Layer(id) => id.clone(),
        Target::Auto => "auto".into(),
        Target::Fx => "fx".into(),
        Target::None => "project".into(),
    }
}

/// Where in time a preset goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtChoice {
    Playhead,
    /// The target layer's `in` (its cut): where a transition belongs.
    LayerIn,
}

/// The default: transitions at the cut, everything else at the playhead.
pub fn default_at(p: &Preset) -> AtChoice {
    if is_transition(p) || p.tags.iter().any(|t| t == "intro") {
        AtChoice::LayerIn
    } else {
        AtChoice::Playhead
    }
}

/// The project time to apply at.
pub fn at_time(doc: &Value, choice: AtChoice, target: &Target, playhead: f64) -> f64 {
    match (choice, target) {
        (AtChoice::LayerIn, Target::Layer(id)) => doc
            .get("layers")
            .and_then(Value::as_array)
            .and_then(|ls| ls.iter().find(|l| l.get("id").and_then(Value::as_str) == Some(id.as_str())))
            .map(|l| l.get("in").and_then(Value::as_f64).unwrap_or(0.0))
            .unwrap_or(playhead),
        _ => playhead,
    }
}

/// The times of a project, with a song analysis' bars and markers when there is one (as `edits apply --audio`).
pub fn time_ctx(doc: &Value, song: Option<&SongInfo>) -> TimeCtx {
    let tc = TimeCtx::from_doc(doc);
    match song {
        Some(s) => tc.with_song(s, doc.pointer("/audio/offset").and_then(Value::as_f64).unwrap_or(0.0)),
        None => tc,
    }
}

/// The document with the preset applied: what `edits apply` writes.
pub fn apply(doc: &Value, p: &Preset, target: &Target, at: f64, values: &Map<String, Value>, tc: &TimeCtx) -> Result<Value, String> {
    preset::apply(doc, p, target, &[at], &given(p, values), tc).map(|(d, _)| d)
}

/// The history line, in the words `edits apply` uses.
pub fn label(p: &Preset, at: f64, target: &Target) -> String {
    format!("preset {} at {} on {}", p.name, crate::editing::tidy(at), target_text(target))
}

/// A value for the colour picker: the first three numbers, in 0..1.
pub fn rgb_of(v: &Value) -> [f32; 3] {
    let a: Vec<f32> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect()).unwrap_or_default();
    [a.first().copied().unwrap_or(1.0), a.get(1).copied().unwrap_or(1.0), a.get(2).copied().unwrap_or(1.0)]
}

/// `old` with its first three numbers replaced by `rgb` (an alpha stays).
pub fn with_rgb(old: &Value, rgb: [f32; 3]) -> Value {
    let mut a: Vec<f64> = old.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    a.resize(a.len().max(3), 1.0);
    for (i, c) in rgb.iter().enumerate() {
        a[i] = crate::editing::tidy(*c as f64);
    }
    json!(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Value {
        json!({
            "format": "edits.project/1",
            "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 120 },
            "slots": { "a": {}, "b": {} },
            "layers": [
                { "id": "a", "kind": "image", "slot": "a", "in": 0, "out": 4 },
                { "id": "b", "kind": "image", "slot": "b", "in": 4, "out": 8 }
            ]
        })
    }

    #[test]
    fn the_library_has_the_builtins_and_every_transition() {
        let lib = Library::load(None);
        assert!(lib.get("flash").is_some() && lib.get("punch").is_some());
        let transitions = lib.presets.iter().filter(|p| is_transition(p)).count();
        assert_eq!(transitions, edits_core::effects::TRANSITIONS.len());
        assert!(lib.problems.is_empty());
    }

    #[test]
    fn presets_are_grouped_by_first_tag_and_searched() {
        let lib = Library::load(None);
        let all = groups(&lib.presets, "");
        let names: Vec<&str> = all.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(names.first(), Some(&"hit"));
        assert_eq!(names.last(), Some(&"transition"));
        assert_eq!(all.iter().map(|(_, l)| l.len()).sum::<usize>(), lib.presets.len());
        // Sorted by name inside a group.
        let hits: Vec<&str> = all[0].1.iter().map(|i| lib.presets[*i].name.as_str()).collect();
        let mut sorted = hits.clone();
        sorted.sort();
        assert_eq!(hits, sorted);
        // A search: the name match first.
        let found = groups(&lib.presets, "flash");
        assert_eq!(lib.presets[found[0].1[0]].name, "flash");
        let whip = groups(&lib.presets, "whip_pan");
        assert_eq!(whip[0].0, "transition");
        assert_eq!(lib.presets[whip[0].1[0]].name, "transition:whip_pan");
        assert!(groups(&lib.presets, "zzzzqqq").is_empty());
    }

    #[test]
    fn edited_variables_become_the_given_ones() {
        let lib = Library::load(None);
        let flash = lib.get("flash").unwrap();
        let mut values = defaults(flash);
        assert!(given(flash, &values).is_empty());
        assert_eq!(var_line(flash, &values), "");
        values.insert("strength".into(), json!(0.5));
        values.insert("color".into(), with_rgb(&values["color"], [1.0, 0.5, 0.0]));
        let g = given(flash, &values);
        assert_eq!(g.len(), 2);
        assert_eq!(g["strength"], json!(0.5));
        assert_eq!(rgb_of(&g["color"]), [1.0, 0.5, 0.0]);
        // Accepted by the preset's own checks.
        assert!(preset::variables(flash, &g).is_ok());
        // A number's range comes from the preset.
        assert_eq!(number_range(&flash.variables["strength"]), (0.0, 1.0, 0.005));
    }

    #[test]
    fn applying_changes_the_target_layer_at_the_time() {
        let lib = Library::load(None);
        let punch = lib.get("punch").unwrap();
        let d = doc();
        let tc = time_ctx(&d, None);
        let mut values = defaults(punch);
        values.insert("amount".into(), json!(1.5));
        let t = target(punch, TargetChoice::Selected, Some("b"));
        assert_eq!(t, Target::Layer("b".into()));
        let at = at_time(&d, AtChoice::Playhead, &t, 5.0);
        let out = apply(&d, punch, &t, at, &values, &tc).unwrap();
        let scale = out.pointer("/layers/1/transform/scale").expect("punch keys the scale of b");
        let keys = scale["keys"].as_array().unwrap();
        assert!(keys.iter().any(|k| k["t"].as_f64() == Some(5.0) && k["v"].as_f64() == Some(1.5)), "{scale}");
        assert!(out.pointer("/layers/0/transform").is_none(), "a is untouched");
        assert_eq!(label(punch, at, &t), "preset punch at 5 on b");
        // Out of range: the preset's own error, nothing applied.
        values.insert("amount".into(), json!(9.0));
        assert!(apply(&d, punch, &t, at, &values, &tc).unwrap_err().contains("maximum"));
    }

    #[test]
    fn transitions_go_at_the_cut_of_the_target() {
        let lib = Library::load(None);
        let fade = lib.get("transition:fade").unwrap();
        assert_eq!(default_at(fade), AtChoice::LayerIn);
        assert_eq!(default_at(lib.get("flash").unwrap()), AtChoice::Playhead);
        let d = doc();
        let t = target(fade, TargetChoice::Selected, Some("b"));
        let at = at_time(&d, AtChoice::LayerIn, &t, 6.0);
        assert_eq!(at, 4.0);
        let out = apply(&d, fade, &t, at, &defaults(fade), &time_ctx(&d, None)).unwrap();
        assert_eq!(out.pointer("/layers/1/transition/kind"), Some(&json!("fade")));
        // No layer selected: the picture on screen; a project preset needs none.
        assert_eq!(target(fade, TargetChoice::Selected, None), Target::Auto);
        assert_eq!(target(lib.get("letterbox").unwrap(), TargetChoice::Fx, Some("a")), Target::None);
        assert_eq!(target(fade, TargetChoice::Fx, Some("a")), Target::Fx);
    }
}
