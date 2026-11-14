//! The Edit page's own edits, as pure functions from the project to
//! [`EditOp`]s, so they are unit tested without a window: the keyframe lanes
//! of the timeline, moving one key of one property, splitting a layer at the
//! playhead, deleting layers (with ripple), moving several layers at once,
//! new text, shape and particle layers, pictures as slots and layers, and
//! new projects made from pictures and a song.

use crate::engine::editing::{
    self, layer_json, layer_props, set_field, set_prop, shifted, tidy, unique_id, with_key_moved,
    PropRef, CAMERA_PROPS, SAME_TIME,
};
use crate::engine::rows;
use crate::inspect::PropKey;
use edits_core::edit::EditOp;
use edits_core::{Layer, LayerKind, Project};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::Path;

// ---- keyframe lanes -----------------------------------------------------------

/// One animated property of a layer: a lane under its timeline row.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LaneDto {
    pub key: PropKey,
    /// `scale`, `glow.radius`, `fov`.
    pub name: String,
    /// (time, `linear` | `hold` | `eased`) in file order.
    pub keys: Vec<(f64, &'static str)>,
}

fn shape(s: rows::KeyShape) -> &'static str {
    match s {
        rows::KeyShape::Linear => "linear",
        rows::KeyShape::Hold => "hold",
        rows::KeyShape::Eased => "eased",
    }
}

/// The lanes of a layer: every property with keys, transform first.
pub fn layer_lanes(p: &Project, l: &Layer) -> Vec<LaneDto> {
    layer_props(l)
        .into_iter()
        .filter_map(|r| {
            let keys = rows::prop_keys(r.get(p, Some(l))?);
            (!keys.is_empty()).then(|| LaneDto {
                key: PropKey::of(&r),
                name: r.label(),
                keys: keys.into_iter().map(|(t, s)| (t, shape(s))).collect(),
            })
        })
        .collect()
}

/// The lanes of the camera.
pub fn camera_lanes(p: &Project) -> Vec<LaneDto> {
    CAMERA_PROPS
        .iter()
        .filter_map(|n| {
            let r = PropRef::Camera(n);
            let keys = rows::prop_keys(r.get(p, None)?);
            (!keys.is_empty()).then(|| LaneDto {
                key: PropKey::of(&r),
                name: n.to_string(),
                keys: keys.into_iter().map(|(t, s)| (t, shape(s))).collect(),
            })
        })
        .collect()
}

/// Move the key of one property at `from` to `to`.
pub fn key_move_ops(
    p: &Project,
    layer: Option<&str>,
    r: &PropRef,
    from: f64,
    to: f64,
) -> Result<Vec<EditOp>, String> {
    let l = match layer {
        Some(id) => Some(
            p.layers
                .iter()
                .find(|l| l.id == id)
                .ok_or_else(|| format!("there is no layer \"{id}\" any more"))?,
        ),
        None => None,
    };
    if (to - from).abs() < SAME_TIME {
        return Ok(Vec::new());
    }
    let moved = with_key_moved(r.get(p, l), from, tidy(to)).ok_or("there is no key there")?;
    Ok(vec![set_prop(layer.unwrap_or_default(), r, &moved)])
}

// ---- layers in time -------------------------------------------------------------

fn find<'a>(p: &'a Project, id: &str) -> Result<&'a Layer, String> {
    p.layers
        .iter()
        .find(|l| l.id == id)
        .ok_or_else(|| format!("there is no layer \"{id}\""))
}

/// Split a layer at `t` into two: the layer ends at `t`, a copy starts
/// there (its keys stay where they are, so the motion goes on). A video
/// without a time remap gets one on the second part, so it plays on from
/// where the first part stops.
pub fn split_ops(p: &Project, doc: &Value, id: &str, t: f64) -> Result<(Vec<EditOp>, String), String> {
    let l = find(p, id)?;
    let end = l.end.unwrap_or(p.settings.duration);
    let t = tidy(t);
    if t <= l.start + SAME_TIME || t >= end - SAME_TIME {
        return Err(format!(
            "the playhead ({t}) is not inside {id} ({}..{}); put it on the layer first",
            tidy(l.start),
            tidy(end)
        ));
    }
    let new_id = unique_id(p, &format!("{id}-2"));
    let mut second = layer_json(Some(doc), l);
    second.insert("id".into(), json!(new_id));
    second.insert("in".into(), json!(t));
    second.insert("out".into(), json!(tidy(end)));
    second.remove("transition");
    if let LayerKind::Video(v) = &l.kind {
        if v.time_remap.is_none() && v.speed.is_none() && !v.reverse {
            let s0 = p.to_seconds(t) - p.to_seconds(l.start);
            let s1 = p.to_seconds(end) - p.to_seconds(l.start);
            second.insert(
                "time_remap".into(),
                json!({ "keys": [ { "t": t, "v": tidy(s0) }, { "t": tidy(end), "v": tidy(s1) } ] }),
            );
        }
    }
    let ops = vec![
        set_field(id, "out", json!(t)),
        EditOp::insert_layer(editing::tidy_json(Value::Object(second)).as_object().cloned().unwrap_or_default(), Some(id.to_string()), None),
    ];
    Ok((ops, format!("split {id} at {t} ({new_id} after)")))
}

/// Every key of a layer and its span `dt` later.
fn shift_layer_ops(p: &Project, l: &Layer, dt: f64) -> Vec<EditOp> {
    let id = l.id.as_str();
    let end = l.end.unwrap_or(p.settings.duration);
    let mut ops = vec![
        set_field(id, "in", json!(tidy(l.start + dt))),
        set_field(id, "out", json!(tidy(end + dt))),
    ];
    for r in layer_props(l) {
        if let Some(moved) = r.get(p, Some(l)).and_then(|prop| shifted(prop, dt)) {
            ops.push(set_prop(id, &r, &moved));
        }
    }
    ops
}

/// Move several layers by `dt` (with their keys), as one edit.
pub fn shift_ops(p: &Project, ids: &[String], dt: f64) -> Result<(Vec<EditOp>, String), String> {
    if dt.abs() < SAME_TIME {
        return Ok((Vec::new(), String::new()));
    }
    let mut ops = Vec::new();
    for id in ids {
        let l = find(p, id)?;
        if l.start + dt < -SAME_TIME {
            return Err(format!("{id} would start before 0"));
        }
        ops.extend(shift_layer_ops(p, l, dt));
    }
    Ok((ops, format!("move {} by {:+}", ids.join(", "), tidy(dt))))
}

/// Delete layers. With `ripple`, every layer that starts at or after the
/// end of a deleted one moves earlier by the deleted span (spans of
/// deleted layers that overlap count once), keys and all.
pub fn remove_ops(p: &Project, ids: &[String], ripple: bool) -> Result<(Vec<EditOp>, String), String> {
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for id in ids {
        let l = find(p, id)?;
        spans.push((l.start, l.end.unwrap_or(p.settings.duration)));
    }
    let mut ops: Vec<EditOp> = ids.iter().map(EditOp::remove_layer).collect();
    if ripple {
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (a, b) in spans {
            match merged.last_mut() {
                Some(last) if a <= last.1 + SAME_TIME => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        for l in p.layers.iter().filter(|l| !ids.contains(&l.id)) {
            let dt: f64 = merged
                .iter()
                .filter(|(_, b)| l.start >= b - SAME_TIME)
                .map(|(a, b)| b - a)
                .sum();
            if dt > SAME_TIME {
                ops.extend(shift_layer_ops(p, l, -dt));
            }
        }
    }
    let label = format!(
        "delete {}{}",
        ids.join(", "),
        if ripple { " (ripple)" } else { "" }
    );
    Ok((ops, label))
}

// ---- new layers -------------------------------------------------------------------

/// A new text, shape or particles layer at `at`, `length` long.
pub fn new_layer(p: &Project, kind: &str, at: f64, length: f64) -> Result<Map<String, Value>, String> {
    let (base, mut layer) = match kind {
        "text" => (
            "title",
            json!({ "kind": "text", "text": "TITLE", "size": 12, "weight": 800 }),
        ),
        "shape:rect" => (
            "rect",
            json!({ "kind": "shape", "shapes": [ { "type": "rect", "size": [0.5, 0.3], "stroke": { "color": [1, 1, 1], "width": 0.8 } } ] }),
        ),
        "shape:ellipse" => (
            "ellipse",
            json!({ "kind": "shape", "shapes": [ { "type": "ellipse", "size": [0.4, 0.4], "stroke": { "color": [1, 1, 1], "width": 0.8 } } ] }),
        ),
        "shape:polystar" => (
            "star",
            json!({ "kind": "shape", "shapes": [ { "type": "polystar", "points": 5, "outer_radius": 0.2, "inner_radius": 0.08, "fill": { "color": [1, 1, 1] } } ] }),
        ),
        "shape:line" => (
            "line",
            json!({ "kind": "shape", "shapes": [ { "type": "line", "from": [-0.3, 0], "to": [0.3, 0], "stroke": { "color": [1, 1, 1], "width": 0.6, "cap": "round" } } ] }),
        ),
        "particles" => (
            "particles",
            json!({ "kind": "particles", "sprite": "star", "prewarm": true, "blend": "add",
                    "emitter": { "shape": "rect", "size": [1.6, 1] }, "rate": 6, "life": 2,
                    "speed": 0.02, "size": 1.5, "fade_in": 0.3, "fade_out": 0.5 }),
        ),
        other => return Err(format!("cannot add a layer of kind {other}")),
    };
    let start = tidy(at.clamp(0.0, (p.settings.duration - length).max(0.0)));
    layer["id"] = json!(unique_id(p, base));
    layer["in"] = json!(start);
    layer["out"] = json!(tidy((start + length).min(p.settings.duration)));
    Ok(editing::tidy_json(layer).as_object().cloned().unwrap_or_default())
}

// ---- pictures ----------------------------------------------------------------------

/// A slot name for a file: its stem in lower case, letters, digits and `_` only, not taken.
pub fn slot_name(p: &Project, file: &Path) -> String {
    let stem: String = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let stem = stem.trim_matches('_');
    let base = if stem.is_empty() { "picture".to_string() } else { stem.chars().take(24).collect() };
    let taken = |n: &str| p.slots.contains_key(n) || p.layers.iter().any(|l| l.id == n);
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|i| format!("{base}{i}"))
        .find(|n| !taken(n))
        .expect("a name is free")
}

/// Add a picture as a new slot and, with `layer`, an image layer of it at `at`.
pub fn picture_ops(
    p: &Project,
    project_dir: &Path,
    file: &Path,
    at: f64,
    length: f64,
    layer: bool,
    above: Option<String>,
) -> (Vec<EditOp>, String) {
    let name = slot_name(p, file);
    let mut ops = vec![EditOp::set_slot(
        name.clone(),
        Some(editing::slot_path(file, project_dir)),
        None,
        None,
    )];
    if layer {
        let start = tidy(at.clamp(0.0, (p.settings.duration - length).max(0.0)));
        let l = json!({ "id": unique_id(p, &name), "kind": "image", "slot": name,
                        "in": start, "out": tidy((start + length).min(p.settings.duration)) });
        ops.push(EditOp::insert_layer(
            editing::tidy_json(l).as_object().cloned().unwrap_or_default(),
            above,
            None,
        ));
    }
    let label = if layer {
        format!("add the picture {name} as a layer")
    } else {
        format!("add the picture {name}")
    };
    (ops, label)
}

/// Point a slot at another picture.
pub fn slot_replace_ops(p: &Project, project_dir: &Path, slot: &str, file: &Path) -> Result<Vec<EditOp>, String> {
    if !p.slots.contains_key(slot) {
        return Err(format!("there is no slot \"{slot}\""));
    }
    Ok(vec![EditOp::set_slot(
        slot,
        Some(editing::slot_path(file, project_dir)),
        None,
        None,
    )])
}

// ---- new projects ----------------------------------------------------------------------

/// A new project of `pictures` cut to a song: one picture per beat for the
/// song's length (at most `max_beats`), at `width` x `height`, 30 fps.
/// `song` is an `edits.audio/1` analysis of `audio` (None: 120 BPM, 16 beats).
pub fn new_project(
    dir: &Path,
    pictures: &[std::path::PathBuf],
    audio: Option<&Path>,
    song: Option<&Value>,
    size: (u32, u32),
    max_beats: f64,
) -> Result<Value, String> {
    if pictures.is_empty() {
        return Err("pick at least one picture".into());
    }
    let info = song.map(edits_core::song::SongInfo::from_value).transpose()?;
    let (bpm, offset, beats) = match &info {
        Some(s) => {
            let beats = ((s.duration - s.beat_offset) / s.beat_seconds).floor();
            (s.bpm, s.beat_offset, beats.clamp(4.0, max_beats))
        }
        None => (120.0, 0.0, 16.0_f64.min(max_beats)),
    };
    let mut slots = Map::new();
    let mut names: Vec<String> = Vec::new();
    for (i, pic) in pictures.iter().enumerate() {
        let stem: String = pic
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .take(20)
            .collect();
        let mut name = if stem.trim_matches('_').is_empty() { format!("p{}", i + 1) } else { stem.trim_matches('_').to_string() };
        while names.contains(&name) {
            name = format!("{name}_{}", i + 1);
        }
        slots.insert(name.clone(), json!({ "path": editing::slot_path(pic, dir) }));
        names.push(name);
    }
    let mut doc = json!({
        "format": "edits.project/1",
        "settings": { "width": size.0, "height": size.1, "fps": 30, "duration": beats,
                      "time_base": "beats", "bpm": tidy(bpm), "beat_offset": 0 },
        "slots": slots,
        "layers": []
    });
    if let Some(a) = audio {
        doc["audio"] = json!({ "path": editing::slot_path(a, dir), "offset": tidy(offset) });
    }
    let tc = edits_core::time::TimeCtx::from_doc(&doc);
    let opts = edits_core::beatops::CutOpts {
        slots: names,
        times: None,
        every: 1.0,
        from: 0.0,
        to: beats,
        prefix: "cut".into(),
        place: edits_core::beatops::Place::Default,
    };
    let (out, _) = edits_core::beatops::cut(&doc, &opts, &tc)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (Project, Value) {
        let doc = json!({
            "format": "edits.project/1",
            "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 120 },
            "slots": { "a": {}, "b": {} },
            "layers": [
                { "id": "a", "kind": "image", "slot": "a", "in": 0, "out": 4,
                  "transform": { "scale": { "keys": [ { "t": 1, "v": 1.2 }, { "t": 2, "v": 1 } ] } },
                  "effects": [ { "kind": "glow", "params": { "intensity": { "keys": [ { "t": 0, "v": 0 }, { "t": 3, "v": 1, "ease": "hold" } ] } } } ] },
                { "id": "b", "kind": "image", "slot": "b", "in": 4, "out": 8, "transition": { "kind": "fade", "duration": 0.5 } },
                { "id": "v", "kind": "video", "slot": "a", "in": 4, "out": 8 }
            ]
        });
        (Project::from_json(&doc.to_string()).unwrap(), doc)
    }

    fn apply(doc: &Value, ops: &[EditOp]) -> Value {
        edits_core::edit::apply_ops(doc, ops).unwrap()
    }

    #[test]
    fn lanes_list_each_animated_property() {
        let (p, _) = project();
        let lanes = layer_lanes(&p, &p.layers[0]);
        assert_eq!(lanes.len(), 2);
        assert_eq!(lanes[0].name, "scale");
        assert_eq!(lanes[0].keys, vec![(1.0, "linear"), (2.0, "linear")]);
        assert_eq!(lanes[1].name, "glow.intensity");
        assert_eq!(lanes[1].keys[1], (3.0, "hold"));
        assert!(layer_lanes(&p, &p.layers[1]).is_empty());
        assert!(camera_lanes(&p).is_empty());
    }

    #[test]
    fn one_key_of_one_property_moves() {
        let (p, doc) = project();
        let ops = key_move_ops(&p, Some("a"), &PropRef::Transform("scale"), 2.0, 2.5).unwrap();
        let out = apply(&doc, &ops);
        assert_eq!(out.pointer("/layers/0/transform/scale/keys/1/t"), Some(&json!(2.5)));
        // The glow's keys stay.
        assert_eq!(out.pointer("/layers/0/effects/0/params/intensity/keys/1/t"), Some(&json!(3)));
        assert!(key_move_ops(&p, Some("a"), &PropRef::Transform("scale"), 7.0, 2.5).is_err());
    }

    #[test]
    fn a_split_makes_two_layers_that_meet() {
        let (p, doc) = project();
        let (ops, label) = split_ops(&p, &doc, "b", 6.0).unwrap();
        let out = apply(&doc, &ops);
        let ls = out["layers"].as_array().unwrap();
        assert_eq!(ls.len(), 4);
        assert_eq!((ls[1]["id"].as_str(), ls[1]["out"].as_f64()), (Some("b"), Some(6.0)));
        assert_eq!((ls[2]["id"].as_str(), ls[2]["in"].as_f64(), ls[2]["out"].as_f64()), (Some("b-2"), Some(6.0), Some(8.0)));
        assert!(ls[2].get("transition").is_none(), "only the first part comes in through the transition");
        assert!(label.contains("split b at 6"));
        // A video plays on: 2 beats at 120 BPM = 1 s into the clip.
        let (ops, _) = split_ops(&p, &doc, "v", 6.0).unwrap();
        let out = apply(&doc, &ops);
        let second = out["layers"].as_array().unwrap().iter().find(|l| l["id"] == "v-2").unwrap();
        assert_eq!(second.pointer("/time_remap/keys/0/v"), Some(&json!(1)));
        assert_eq!(second.pointer("/time_remap/keys/1/v"), Some(&json!(2)));
        assert!(split_ops(&p, &doc, "b", 2.0).is_err(), "outside the layer");
    }

    #[test]
    fn ripple_delete_closes_the_gap() {
        let (p, doc) = project();
        let (ops, _) = remove_ops(&p, &["a".to_string()], true).unwrap();
        let out = apply(&doc, &ops);
        let ls = out["layers"].as_array().unwrap();
        assert_eq!(ls[0]["id"], "b");
        assert_eq!((ls[0]["in"].as_f64(), ls[0]["out"].as_f64()), (Some(0.0), Some(4.0)));
        let (ops, _) = remove_ops(&p, &["a".to_string()], false).unwrap();
        let out = apply(&doc, &ops);
        assert_eq!(out["layers"][0]["in"].as_f64(), Some(4.0));
    }

    #[test]
    fn several_layers_move_with_their_keys() {
        let (p, doc) = project();
        let (ops, _) = shift_ops(&p, &["a".to_string(), "b".to_string()], 1.0).unwrap();
        let out = apply(&doc, &ops);
        assert_eq!(out.pointer("/layers/0/in").and_then(Value::as_f64), Some(1.0));
        assert_eq!(out.pointer("/layers/0/transform/scale/keys/0/t"), Some(&json!(2)));
        assert_eq!(out.pointer("/layers/1/out").and_then(Value::as_f64), Some(9.0));
        assert!(shift_ops(&p, &["a".to_string()], -1.0).is_err());
    }

    #[test]
    fn new_layers_and_pictures() {
        let (p, doc) = project();
        for kind in ["text", "shape:rect", "shape:ellipse", "shape:polystar", "shape:line", "particles"] {
            let l = new_layer(&p, kind, 2.0, 4.0).unwrap();
            let out = apply(&doc, &[EditOp::insert_layer(l, None, None)]);
            let parsed = Project::from_json(&out.to_string());
            assert!(parsed.is_ok(), "{kind}: {parsed:?}");
            let findings = edits_core::check::check(&parsed.unwrap(), None);
            assert!(
                findings.iter().all(|f| f.severity != edits_core::check::Severity::Error),
                "{kind}: {findings:?}"
            );
        }
        assert!(new_layer(&p, "teapot", 0.0, 1.0).is_err());
        assert_eq!(slot_name(&p, Path::new("/x/My Photo.JPG")), "my_photo");
        assert_eq!(slot_name(&p, Path::new("/x/a.png")), "a2");
        let (ops, _) = picture_ops(&p, Path::new("/x"), Path::new("/x/pics/new.png"), 1.0, 4.0, true, None);
        let out = apply(&doc, &ops);
        assert_eq!(out.pointer("/slots/new/path"), Some(&json!("pics/new.png")));
        assert_eq!(out["layers"].as_array().unwrap().last().unwrap()["slot"], "new");
        let ops = slot_replace_ops(&p, Path::new("/x"), "a", Path::new("/y/other.png")).unwrap();
        assert_eq!(apply(&doc, &ops).pointer("/slots/a/path"), Some(&json!("/y/other.png")));
        assert!(slot_replace_ops(&p, Path::new("/x"), "zz", Path::new("/y.png")).is_err());
    }

    #[test]
    fn a_new_project_cuts_the_pictures_on_beats() {
        let pics: Vec<std::path::PathBuf> = ["/p/one.png", "/p/two.png"].iter().map(Into::into).collect();
        let doc = new_project(Path::new("/p"), &pics, None, None, (1080, 1920), 64.0).unwrap();
        let p = Project::from_json(&doc.to_string()).unwrap();
        assert_eq!(p.settings.duration, 16.0);
        assert_eq!(p.layers.len(), 16);
        assert_eq!(p.slots.len(), 2);
        assert!(new_project(Path::new("/p"), &[], None, None, (1, 1), 4.0).is_err());
    }
}
