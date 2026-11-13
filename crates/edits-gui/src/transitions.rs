//! The inspector's "Transition in" section as edits: a layer's
//! `transition` (kind, duration, ease, parameters) changed through
//! `edits_core::edit` ops, and a new transition added through the
//! `transition:<kind>` preset (the one `edits apply` and the agent use), so
//! the picture it comes from stays visible while it runs.

use crate::editing::{tidy, tidy_json};
use edits_core::edit::{layer_pointer, EditOp};
use edits_core::effects::{self, TransitionSpec};
use edits_core::preset::{self, Target};
use edits_core::time::TimeCtx;
use edits_core::Ease;
use serde_json::{json, Map, Value};

pub fn specs() -> &'static [TransitionSpec] {
    effects::TRANSITIONS
}

fn ptr(id: &str) -> String {
    format!("{}/transition", layer_pointer(id))
}

fn layers(doc: &Value) -> &[Value] {
    doc.get("layers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn num(l: &Value, k: &str) -> Option<f64> {
    l.get(k).and_then(Value::as_f64)
}

/// The document with a `kind` transition added into layer `id` at its cut:
/// the `transition:<kind>` preset with its default length, starting at the
/// layer's `in` (which stays where it is) and keeping the picture it comes
/// from until it ends.
pub fn add(doc: &Value, id: &str, kind: &str, tc: &TimeCtx) -> Result<Value, String> {
    let spec = effects::transition(kind).ok_or_else(|| format!("no transition \"{kind}\""))?;
    let l = layers(doc).iter().find(|l| l.get("id").and_then(Value::as_str) == Some(id)).ok_or_else(|| format!("no layer has id \"{id}\""))?;
    let at = num(l, "in").unwrap_or(0.0);
    let p = preset::transition_preset(spec);
    let mut vars = Map::new();
    vars.insert("align".into(), json!("after"));
    preset::apply(doc, &p, &Target::Layer(id.to_string()), &[at], &vars, tc).map(|(d, _)| d)
}

/// Another kind: the parameters the new kind also has are kept, the others dropped.
pub fn kind_ops(doc: &Value, id: &str, kind: &str) -> Vec<EditOp> {
    let Some(spec) = effects::transition(kind) else { return Vec::new() };
    let old = current(doc, id).and_then(|t| t.get("params")).and_then(Value::as_object).cloned().unwrap_or_default();
    let kept: Map<String, Value> = old.into_iter().filter(|(k, _)| spec.params.iter().any(|p| p.name == k)).collect();
    let mut ops = vec![EditOp::set(format!("{}/kind", ptr(id)), json!(kind))];
    if kept.is_empty() {
        if current(doc, id).and_then(|t| t.get("params")).is_some() {
            ops.push(EditOp::remove(format!("{}/params", ptr(id))));
        }
    } else {
        ops.push(EditOp::set(format!("{}/params", ptr(id)), Value::Object(kept)));
    }
    ops
}

/// The transition object of layer `id` as the file has it.
pub fn current<'a>(doc: &'a Value, id: &str) -> Option<&'a Value> {
    layers(doc).iter().find(|l| l.get("id").and_then(Value::as_str) == Some(id))?.get("transition")
}

/// The picture the layer at `index` comes from: the topmost picture below it
/// (not an adjustment layer, normal blend) visible just before its `in`.
pub fn previous(doc: &Value, index: usize, at: f64, frame: f64) -> Option<usize> {
    let duration = doc.pointer("/settings/duration").and_then(Value::as_f64).unwrap_or(f64::MAX);
    let ls = layers(doc);
    let t = at - 0.5 * frame;
    (0..index.min(ls.len())).rev().find(|&i| {
        let l = &ls[i];
        let picture = l.get("kind").and_then(Value::as_str) != Some("adjustment") && l.get("blend").and_then(Value::as_str).unwrap_or("normal") == "normal";
        let on = l.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        let (start, end) = (num(l, "in").unwrap_or(0.0), num(l, "out").unwrap_or(duration));
        picture && on && t >= start - 1e-9 && t < end - 1e-9
    })
}

/// A new length; the picture it comes from is kept until it ends.
pub fn duration_ops(doc: &Value, id: &str, duration: f64, tc: &TimeCtx) -> Vec<EditOp> {
    let duration = tidy(duration.max(tc.frame_len()));
    let mut ops = vec![EditOp::set(format!("{}/duration", ptr(id)), json!(duration))];
    let ls = layers(doc);
    if let Some(index) = ls.iter().position(|l| l.get("id").and_then(Value::as_str) == Some(id)) {
        let start = num(&ls[index], "in").unwrap_or(0.0);
        if let Some(prev) = previous(doc, index, start, tc.frame_len()) {
            let p = &ls[prev];
            let end = num(p, "out").unwrap_or(tc.duration);
            let needed = tidy(start + duration);
            if end < needed - 1e-9 {
                if let Some(pid) = p.get("id").and_then(Value::as_str) {
                    ops.push(EditOp::set(format!("{}/out", layer_pointer(pid)), json!(needed)));
                }
            }
        }
    }
    ops
}

pub fn ease_op(id: &str, ease: &Ease) -> EditOp {
    EditOp::set(format!("{}/ease", ptr(id)), serde_json::to_value(ease).unwrap_or(json!("linear")))
}

/// One parameter: a number, or the list of numbers for a vector or colour.
pub fn param_op(id: &str, name: &str, v: &[f64]) -> EditOp {
    let value = match v {
        [one] => json!(tidy(*one)),
        many => json!(many.iter().map(|x| tidy(*x)).collect::<Vec<_>>()),
    };
    EditOp::set(format!("{}/params/{}", ptr(id), edits_core::pointer::escape(name)), tidy_json(value))
}

pub fn remove_op(id: &str) -> EditOp {
    EditOp::remove(ptr(id))
}

/// A line for the history.
pub fn describe(kind: &str, duration: f64, unit: &str) -> String {
    format!("{kind} over {} {unit}", tidy(duration))
}

#[cfg(test)]
mod tests {
    use super::*;
    use edits_core::edit::apply_ops;

    fn doc() -> Value {
        json!({
            "format": "edits.project/1",
            "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 120 },
            "slots": { "a": {}, "b": {} },
            "layers": [
                { "id": "a", "kind": "image", "slot": "a", "in": 0, "out": 4 },
                { "id": "fx", "kind": "adjustment", "in": 0, "out": 8 },
                { "id": "b", "kind": "image", "slot": "b", "in": 4, "out": 8 }
            ]
        })
    }

    fn tc(d: &Value) -> TimeCtx {
        TimeCtx::from_doc(d)
    }

    #[test]
    fn there_are_the_twenty_two_kinds_with_docs() {
        assert_eq!(specs().len(), 22);
        assert!(specs().iter().all(|s| !s.doc.is_empty() && s.duration > 0.0));
    }

    #[test]
    fn adding_keeps_the_cut_and_the_outgoing_picture() {
        let d = doc();
        let out = add(&d, "b", "whip_pan", &tc(&d)).unwrap();
        let t = out.pointer("/layers/2/transition").unwrap();
        assert_eq!(t["kind"], "whip_pan");
        assert_eq!(t["duration"], json!(0.5));
        assert_eq!(out.pointer("/layers/2/in"), Some(&json!(4.0)), "{out}");
        // a, the picture under b at its cut, now lasts until the transition ends.
        assert_eq!(out.pointer("/layers/0/out").and_then(Value::as_f64), Some(4.5));
        assert!(add(&d, "nope", "fade", &tc(&d)).is_err());
        assert!(add(&d, "b", "nope", &tc(&d)).is_err());
    }

    #[test]
    fn edits_are_ops_on_the_transition() {
        let d = add(&doc(), "b", "whip_pan", &tc(&doc())).unwrap();
        // Another kind: whip_pan's blur and direction mean nothing to fade.
        let d2 = apply_ops(&d, &kind_ops(&d, "b", "fade")).unwrap();
        assert_eq!(d2.pointer("/layers/2/transition/kind"), Some(&json!("fade")));
        assert!(d2.pointer("/layers/2/transition/params").is_none());
        // ...but cross_zoom has no direction either, while whip_spin keeps blur.
        let d3 = apply_ops(&d, &kind_ops(&d, "b", "whip_spin")).unwrap();
        assert_eq!(d3.pointer("/layers/2/transition/params").unwrap().as_object().unwrap().keys().collect::<Vec<_>>(), vec!["blur"]);
        // Length: the outgoing picture grows with it.
        let d4 = apply_ops(&d, &duration_ops(&d, "b", 1.5, &tc(&d))).unwrap();
        assert_eq!(d4.pointer("/layers/2/transition/duration"), Some(&json!(1.5)));
        assert_eq!(d4.pointer("/layers/0/out").and_then(Value::as_f64), Some(5.5));
        // Shorter: nothing else moves.
        let ops = duration_ops(&d, "b", 0.25, &tc(&d));
        assert_eq!(ops.len(), 1);
        // Ease and a parameter.
        let d5 = apply_ops(&d, &[ease_op("b", &Ease::Name("in_out".into())), param_op("b", "blur", &[0.75]), param_op("b", "direction", &[1.0, 0.0])]).unwrap();
        let t = d5.pointer("/layers/2/transition").unwrap();
        assert_eq!((t["ease"].clone(), t["params"]["blur"].clone(), t["params"]["direction"].clone()), (json!("in_out"), json!(0.75), json!([1.0, 0.0])));
        // Remove.
        let d6 = apply_ops(&d, &[remove_op("b")]).unwrap();
        assert!(d6.pointer("/layers/2/transition").is_none());
        // Every result is a valid project.
        for x in [&d2, &d3, &d4, &d5, &d6] {
            let p: edits_core::Project = serde_json::from_value(x.clone()).unwrap();
            assert!(!edits_core::check::has_errors(&edits_core::check::check(&p, None)), "{x}");
        }
    }

    #[test]
    fn the_outgoing_picture_skips_adjustment_layers() {
        let d = doc();
        assert_eq!(previous(&d, 2, 4.0, 0.08), Some(0));
        assert_eq!(previous(&d, 0, 0.0, 0.08), None);
        assert_eq!(describe("fade", 0.5, "beats"), "fade over 0.5 beats");
    }
}
