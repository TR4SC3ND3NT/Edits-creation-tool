// Ported from `crates/edits-gui/src/editing.rs` (the egui window), unchanged but for
// imports, so Edits Studio and the old window edit projects the same way.

//! The edits the window makes, as pure functions from the project it shows
//! to [`edits_core::edit::EditOp`]s: changing a value (a constant, or a key
//! at the playhead), keys, eases, layers, effects, slots, and the drags on
//! the timeline. No window here, so all of it is unit tested.
//!
//! Every op addresses layers by id (`/layers/@<id>/...`), so an op stays
//! right when another layer is inserted. Values are written tidily: times
//! and values rounded to 1e-6, whole numbers as integers, `linear` eases
//! (the default) left out.

use super::timemap::TimeMap;
use edits_core::edit::{layer_pointer, EditOp};
use edits_core::{
    effects, Anim, Ease, Evaluator, Key, Layer, Modifier, Project, Prop, Value as Num,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

/// Two key times closer than this are the same time.
pub const SAME_TIME: f64 = 1e-4;

// ---- numbers and snapping ----------------------------------------------------

/// Round to 1e-6 (and never write `-0`).
pub fn tidy(x: f64) -> f64 {
    let r = (x * 1e6).round() / 1e6;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

/// A JSON value with every number tidied, whole numbers as integers.
pub fn tidy_json(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => {
                let f = tidy(f);
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    json!(f as i64)
                } else {
                    json!(f)
                }
            }
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(tidy_json).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, tidy_json(v))).collect()),
        other => other,
    }
}

/// What times snap to while editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Snap {
    /// Quarter beats: a beats project, without Alt.
    QuarterBeat,
    /// The start of the nearest output frame: a seconds project, or Alt held.
    Frame,
}

impl Snap {
    pub fn of(tm: &TimeMap, alt: bool) -> Self {
        if tm.uses_beats() && !alt {
            Snap::QuarterBeat
        } else {
            Snap::Frame
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Snap::QuarterBeat => "snap ¼ beat (Alt: frames)",
            Snap::Frame => "snap to frames",
        }
    }
}

/// Snap a project time: to quarter beats in a beats project, or (with Alt,
/// or in a seconds project) to the start of the nearest frame.
pub fn snap(tm: &TimeMap, t: f64, alt: bool) -> f64 {
    match Snap::of(tm, alt) {
        Snap::QuarterBeat => tidy((t * 4.0).round() / 4.0),
        Snap::Frame => {
            let frame = (tm.to_seconds(t) * tm.fps).round();
            tidy(tm.from_seconds(frame / tm.fps))
        }
    }
}

// ---- properties ----------------------------------------------------------------

pub const TRANSFORM_PROPS: [&str; 5] = ["position", "scale", "rotation", "anchor", "opacity"];
pub const CAMERA_PROPS: [&str; 3] = ["fov", "position", "rotation"];

/// One animatable property the window can edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropRef {
    /// A transform property of a layer (one of [`TRANSFORM_PROPS`]).
    Transform(&'static str),
    /// A parameter of the layer's effect at `index` (whose kind was `kind`).
    Effect {
        index: usize,
        kind: String,
        param: String,
    },
    /// A camera property (one of [`CAMERA_PROPS`]).
    Camera(&'static str),
}

impl PropRef {
    /// `scale`, `glow.radius`, `camera.fov`.
    pub fn label(&self) -> String {
        match self {
            PropRef::Transform(n) => n.to_string(),
            PropRef::Effect { kind, param, .. } => format!("{kind}.{param}"),
            PropRef::Camera(n) => format!("camera.{n}"),
        }
    }

    /// Where the property is in the document. `layer` is ignored for the camera.
    pub fn pointer(&self, layer: &str) -> String {
        match self {
            PropRef::Transform(n) => format!("{}/transform/{n}", layer_pointer(layer)),
            PropRef::Effect { index, param, .. } => format!(
                "{}/effects/{index}/params/{}",
                layer_pointer(layer),
                edits_core::pointer::escape(param)
            ),
            PropRef::Camera(n) => format!("/camera/{n}"),
        }
    }

    /// The property as the file has it; `None` for an effect parameter the
    /// file does not set (the registry's default applies) or a missing effect.
    pub fn get<'a>(&self, p: &'a Project, layer: Option<&'a Layer>) -> Option<&'a Prop> {
        match self {
            PropRef::Transform(n) => {
                let t = &layer?.transform;
                Some(match *n {
                    "position" => &t.position,
                    "scale" => &t.scale,
                    "rotation" => &t.rotation,
                    "anchor" => &t.anchor,
                    _ => &t.opacity,
                })
            }
            PropRef::Effect { index, kind, param } => layer?
                .effects
                .get(*index)
                .filter(|e| &e.kind == kind)?
                .params
                .get(param),
            PropRef::Camera(n) => Some(match *n {
                "fov" => &p.camera.fov,
                "position" => &p.camera.position,
                _ => &p.camera.rotation,
            }),
        }
    }

    /// The value when the file does not say: the registry's default for an
    /// effect parameter, the model's default otherwise.
    pub fn default_value(&self) -> Vec<f64> {
        match self {
            PropRef::Transform("scale") | PropRef::Transform("opacity") => vec![1.0],
            PropRef::Transform("anchor") => vec![0.0, 0.0],
            PropRef::Transform(_) | PropRef::Camera(_) if *self != PropRef::Camera("fov") => {
                vec![0.0, 0.0, 0.0]
            }
            PropRef::Effect { kind, param, .. } => effects::spec(kind)
                .and_then(|s| s.params.iter().find(|p| p.name == param))
                .map(|p| p.default.to_vec())
                .unwrap_or_else(|| vec![0.0]),
            _ => vec![39.6],
        }
    }

    /// True when the layer still has this property (the effect is still at its index).
    pub fn fits(&self, layer: &Layer) -> bool {
        match self {
            PropRef::Effect { index, kind, .. } => {
                layer.effects.get(*index).is_some_and(|e| &e.kind == kind)
            }
            PropRef::Transform(_) => true,
            PropRef::Camera(_) => false,
        }
    }
}

/// Every editable property of a layer: its transform, then each effect parameter of the registry.
pub fn layer_props(layer: &Layer) -> Vec<PropRef> {
    let mut out: Vec<PropRef> = TRANSFORM_PROPS
        .iter()
        .map(|n| PropRef::Transform(n))
        .collect();
    for (index, e) in layer.effects.iter().enumerate() {
        let names: Vec<String> = match effects::spec(&e.kind) {
            Some(spec) => spec.params.iter().map(|p| p.name.to_string()).collect(),
            None => e.params.keys().cloned().collect(),
        };
        for param in names {
            out.push(PropRef::Effect {
                index,
                kind: e.kind.clone(),
                param,
            });
        }
    }
    out
}

fn to_num(v: &[f64]) -> Num {
    match v {
        [one] => Num::Num(*one),
        many => Num::Vec(many.to_vec()),
    }
}

/// JSON as the window writes it: tidy numbers, no `linear` eases (the default).
pub fn clean(v: Value) -> Value {
    fn drop_linear(v: &mut Value) {
        match v {
            Value::Object(o) => {
                if o.get("ease") == Some(&json!("linear")) {
                    o.remove("ease");
                }
                o.values_mut().for_each(drop_linear);
            }
            Value::Array(a) => a.iter_mut().for_each(drop_linear),
            _ => {}
        }
    }
    let mut v = tidy_json(v);
    drop_linear(&mut v);
    v
}

/// A property as JSON for an op.
pub fn prop_json(prop: &Prop) -> Value {
    clean(serde_json::to_value(prop).unwrap_or(Value::Null))
}

/// The keys of a property; empty for a constant.
pub fn keys_of(prop: Option<&Prop>) -> &[Key] {
    match prop {
        Some(Prop::Anim(a)) => &a.keys,
        _ => &[],
    }
}

pub fn key_index_at(prop: Option<&Prop>, t: f64) -> Option<usize> {
    keys_of(prop)
        .iter()
        .position(|k| (k.t - t).abs() < SAME_TIME)
}

/// The key whose segment the time is in: the last key at or before `t`.
pub fn key_at_or_before(prop: Option<&Prop>, t: f64) -> Option<usize> {
    keys_of(prop).iter().rposition(|k| k.t <= t + SAME_TIME)
}

/// The value a property holds at `t` without its shake: what its keys (or
/// its constant, or its base) say. `default` when the file does not set it.
pub fn base_value(p: &Project, prop: Option<&Prop>, default: &[f64], t: f64) -> Vec<f64> {
    let v = match prop {
        None => return default.to_vec(),
        Some(Prop::Const(v)) => v.to_vec(),
        Some(Prop::Anim(a)) => {
            let loops: Vec<Modifier> = a
                .modifiers
                .iter()
                .filter(|m| matches!(m, Modifier::Loop { .. }))
                .cloned()
                .collect();
            let plain = Prop::Anim(Anim {
                keys: a.keys.clone(),
                base: a.base.clone(),
                modifiers: loops,
            });
            Evaluator::new(p).prop(&plain, t)
        }
    };
    if v.is_empty() {
        default.to_vec()
    } else {
        v
    }
}

fn insert_key(a: &mut Anim, t: f64, v: Vec<f64>) {
    let t = tidy(t);
    match a.keys.iter().position(|k| (k.t - t).abs() < SAME_TIME) {
        Some(i) => a.keys[i].v = to_num(&v),
        None => {
            let at = a.keys.partition_point(|k| k.t < t);
            // The new key continues the curve of the segment it splits.
            let ease = a
                .keys
                .get(at.wrapping_sub(1))
                .or(a.keys.get(at))
                .map(|k| k.ease.clone())
                .unwrap_or_default();
            a.keys.insert(
                at,
                Key {
                    t,
                    v: to_num(&v),
                    ease,
                },
            );
        }
    }
}

/// The property with value `v`: a constant (or a base under modifiers)
/// changes; a keyframed property gets a key at `t` (replacing one that is
/// there), and its other keys stay.
pub fn with_value(prop: Option<&Prop>, t: f64, v: Vec<f64>) -> Prop {
    match prop {
        None | Some(Prop::Const(_)) => Prop::Const(to_num(&v)),
        Some(Prop::Anim(a)) if a.keys.is_empty() => Prop::Anim(Anim {
            base: Some(to_num(&v)),
            ..a.clone()
        }),
        Some(Prop::Anim(a)) => {
            let mut a = a.clone();
            insert_key(&mut a, t, v);
            Prop::Anim(a)
        }
    }
}

/// "Key here": a key at `t` holding the value the property has there. A
/// constant becomes one key; a keyframed property gets one more. `None`
/// when there already is a key at `t`.
pub fn with_key_here(p: &Project, prop: Option<&Prop>, default: &[f64], t: f64) -> Option<Prop> {
    if key_index_at(prop, t).is_some() {
        return None;
    }
    let v = base_value(p, prop, default, t);
    let mut a = match prop {
        Some(Prop::Anim(a)) => a.clone(),
        _ => Anim {
            keys: Vec::new(),
            base: None,
            modifiers: Vec::new(),
        },
    };
    if a.keys.is_empty() {
        // Keys replace the base.
        a.base = None;
    }
    insert_key(&mut a, t, v);
    Some(Prop::Anim(a))
}

/// The property without its key at `t`. With no keys left it is a constant
/// again (holding the removed key's value), or a base under its modifiers.
pub fn without_key(prop: Option<&Prop>, t: f64) -> Option<Prop> {
    let Some(Prop::Anim(a)) = prop else {
        return None;
    };
    let i = key_index_at(prop, t)?;
    let mut a = a.clone();
    let removed = a.keys.remove(i);
    if !a.keys.is_empty() {
        return Some(Prop::Anim(a));
    }
    if a.modifiers.is_empty() {
        Some(Prop::Const(removed.v))
    } else {
        Some(Prop::Anim(Anim {
            base: Some(removed.v),
            ..a
        }))
    }
}

/// The property with key `index` eased by `ease`.
pub fn with_ease(prop: Option<&Prop>, index: usize, ease: Ease) -> Option<Prop> {
    let Some(Prop::Anim(a)) = prop else {
        return None;
    };
    let mut a = a.clone();
    a.keys.get_mut(index)?.ease = ease;
    Some(Prop::Anim(a))
}

fn move_keys_in(a: &mut Anim, from: f64, to: f64) -> bool {
    let mut moved = false;
    if let Some(i) = a.keys.iter().position(|k| (k.t - from).abs() < SAME_TIME) {
        let mut k = a.keys.remove(i);
        k.t = tidy(to);
        // The moved key wins over one that was at its new time.
        a.keys.retain(|o| (o.t - k.t).abs() >= SAME_TIME);
        let at = a.keys.partition_point(|o| o.t < k.t);
        a.keys.insert(at, k);
        moved = true;
    }
    for m in &mut a.modifiers {
        if let Modifier::Shake {
            amplitude: Prop::Anim(env),
            ..
        } = m
        {
            moved |= move_keys_in(env, from, to);
        }
    }
    moved
}

/// The property with its key at `from` moved to `to` (keys of a keyframed
/// shake amplitude too). `None` when it has no key at `from`.
pub fn with_key_moved(prop: Option<&Prop>, from: f64, to: f64) -> Option<Prop> {
    let Some(Prop::Anim(a)) = prop else {
        return None;
    };
    let mut a = a.clone();
    move_keys_in(&mut a, from, to).then_some(Prop::Anim(a))
}

fn shift_anim(a: &mut Anim, dt: f64) {
    for k in &mut a.keys {
        k.t = tidy(k.t + dt);
    }
    for m in &mut a.modifiers {
        match m {
            Modifier::Loop { from, .. } => *from = tidy(*from + dt),
            Modifier::Shake {
                amplitude: Prop::Anim(env),
                ..
            } => shift_anim(env, dt),
            Modifier::Shake { .. } => {}
        }
    }
}

/// Every key of the property `dt` later (and a loop's start): the property
/// of a layer that moved in time. `None` for a constant.
pub fn shifted(prop: &Prop, dt: f64) -> Option<Prop> {
    let Prop::Anim(a) = prop else { return None };
    let mut a = a.clone();
    shift_anim(&mut a, dt);
    Some(Prop::Anim(a))
}

/// `set` the property at its pointer.
pub fn set_prop(layer: &str, r: &PropRef, prop: &Prop) -> EditOp {
    EditOp::set(r.pointer(layer), prop_json(prop))
}

// ---- eases -------------------------------------------------------------------

/// The names the ease picker offers: `edits_core::ease::NAMES`, then `bezier`.
pub fn ease_choices() -> Vec<&'static str> {
    edits_core::ease::NAMES
        .iter()
        .copied()
        .chain(["bezier"])
        .collect()
}

pub fn ease_name(e: &Ease) -> String {
    match e {
        Ease::Name(n) | Ease::Params { name: n, .. } => n.clone(),
        Ease::Bezier(_) => "bezier".to_string(),
    }
}

/// The ease for a name chosen in the picker, keeping what `current` already
/// says where it can (a decay's rate, a bezier's points).
pub fn ease_from_choice(name: &str, current: &Ease) -> Ease {
    match name {
        "decay" => {
            let rate = match current {
                Ease::Params { name, args } if name == "decay" => {
                    args.get("rate").copied().unwrap_or(0.4)
                }
                _ => 0.4,
            };
            Ease::Params {
                name: "decay".into(),
                args: BTreeMap::from([("rate".to_string(), rate)]),
            }
        }
        "bezier" => match current {
            Ease::Bezier(b) => Ease::Bezier(*b),
            _ => Ease::Bezier([0.25, 0.1, 0.25, 1.0]),
        },
        other => match current {
            // back_out's overshoot, elastic_out's period: kept when the name stays.
            Ease::Params { name, args } if name == other => Ease::Params {
                name: name.clone(),
                args: args.clone(),
            },
            _ => Ease::Name(other.to_string()),
        },
    }
}

// ---- layers --------------------------------------------------------------------

/// `base` when no layer has that id, otherwise `base2`, `base3`, ...
pub fn unique_id(p: &Project, base: &str) -> String {
    let taken = |id: &str| p.layers.iter().any(|l| l.id == id);
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}{n}"))
        .find(|id| !taken(id))
        .expect("some id is free")
}

/// What kind of layer to add.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewLayer {
    Image(String),
    Solid,
    Adjustment,
    Cube(String),
}

/// One bar (four beats), or two seconds in a seconds project: how long a new layer is.
pub fn new_layer_length(tm: &TimeMap) -> f64 {
    if tm.uses_beats() {
        4.0
    } else {
        2.0
    }
}

/// A new layer starting at `at` (already snapped), one bar long, as JSON.
pub fn new_layer(p: &Project, kind: &NewLayer, at: f64, length: f64) -> Map<String, Value> {
    let (id, mut layer) = match kind {
        NewLayer::Image(slot) => (unique_id(p, slot), json!({ "kind": "image", "slot": slot })),
        NewLayer::Solid => (
            unique_id(p, "solid"),
            json!({ "kind": "solid", "color": [0.5, 0.5, 0.5, 1] }),
        ),
        NewLayer::Adjustment => (unique_id(p, "adjust"), json!({ "kind": "adjustment" })),
        NewLayer::Cube(slot) => (
            unique_id(p, "cube"),
            json!({ "kind": "cube", "faces": [slot], "size": [0.6, 0.6, 0.6], "transform": { "rotation": [-20, 30, 0] } }),
        ),
    };
    let start = tidy(at.clamp(0.0, (p.settings.duration - length).max(0.0)));
    layer["id"] = json!(id);
    layer["in"] = json!(start);
    layer["out"] = json!(tidy((start + length).min(p.settings.duration)));
    tidy_json(layer).as_object().cloned().unwrap_or_default()
}

/// The layer as it is in the file (`doc` is the file's JSON), so keys the
/// model does not know survive a copy or a move; the model's JSON when the
/// file does not have it.
pub fn layer_json(doc: Option<&Value>, l: &Layer) -> Map<String, Value> {
    let raw = doc.and_then(|d| {
        d.get("layers")?
            .as_array()?
            .iter()
            .find(|x| x.get("id").and_then(Value::as_str) == Some(l.id.as_str()))?
            .as_object()
            .cloned()
    });
    raw.unwrap_or_else(|| {
        clean(serde_json::to_value(l).unwrap_or(Value::Null))
            .as_object()
            .cloned()
            .unwrap_or_default()
    })
}

/// A copy of the layer directly above it, with a new id.
pub fn duplicate_ops(p: &Project, doc: Option<&Value>, id: &str) -> Option<(Vec<EditOp>, String)> {
    let l = p.layers.iter().find(|l| l.id == id)?;
    let new_id = unique_id(p, &format!("{id}-copy"));
    let mut layer = layer_json(doc, l);
    layer.insert("id".into(), json!(new_id));
    Some((
        vec![EditOp::insert_layer(layer, Some(id.to_string()), None)],
        new_id,
    ))
}

/// Move a layer one place up (towards the front) or down in the stack.
/// `None` when it is already at that end.
pub fn move_layer_ops(p: &Project, doc: Option<&Value>, id: &str, up: bool) -> Option<Vec<EditOp>> {
    let i = p.layers.iter().position(|l| l.id == id)?;
    let j = if up {
        Some(i + 1).filter(|j| *j < p.layers.len())?
    } else {
        i.checked_sub(1)?
    };
    let neighbour = p.layers[j].id.clone();
    let layer = layer_json(doc, &p.layers[i]);
    let insert = if up {
        EditOp::insert_layer(layer, Some(neighbour), None)
    } else {
        EditOp::insert_layer(layer, None, Some(neighbour))
    };
    Some(vec![EditOp::remove_layer(id), insert])
}

/// Set one field of a layer (`in`, `out`, `enabled`, `blend`, `slot`, `color`, `size`...).
pub fn set_field(id: &str, field: &str, value: Value) -> EditOp {
    EditOp::set(format!("{}/{field}", layer_pointer(id)), tidy_json(value))
}

// ---- effects -------------------------------------------------------------------

/// The effects the "Add effect" menu offers: the registry without `custom`,
/// which needs a shader file.
pub fn addable_effects() -> impl Iterator<Item = &'static effects::EffectSpec> {
    effects::EFFECTS.iter().filter(|e| e.kind != "custom")
}

/// Add an effect (with the registry's defaults) after the layer's others.
pub fn add_effect_op(layer: &Layer, kind: &str) -> EditOp {
    let base = layer_pointer(&layer.id);
    if layer.effects.is_empty() {
        EditOp::set(format!("{base}/effects"), json!([{ "kind": kind }]))
    } else {
        EditOp::set(format!("{base}/effects/-"), json!({ "kind": kind }))
    }
}

pub fn remove_effect_op(layer: &Layer, index: usize) -> EditOp {
    EditOp::remove(format!("{}/effects/{index}", layer_pointer(&layer.id)))
}

pub fn toggle_effect_op(layer: &Layer, index: usize) -> Option<EditOp> {
    let e = layer.effects.get(index)?;
    Some(EditOp::set(
        format!("{}/effects/{index}/enabled", layer_pointer(&layer.id)),
        json!(!e.enabled),
    ))
}

/// Swap effect `index` with the one before (`up`) or after it: the order
/// effects are applied in. The effects are taken from the file (`doc`) when it has them.
pub fn move_effect_op(
    doc: Option<&Value>,
    layer: &Layer,
    index: usize,
    up: bool,
) -> Option<EditOp> {
    let j = if up { index.checked_sub(1)? } else { index + 1 };
    if j >= layer.effects.len() || index >= layer.effects.len() {
        return None;
    }
    let raw = layer_json(doc, layer)
        .get("effects")
        .and_then(Value::as_array)
        .cloned()
        .filter(|a| a.len() == layer.effects.len());
    let mut list = raw.unwrap_or_else(|| {
        layer
            .effects
            .iter()
            .map(|e| clean(serde_json::to_value(e).unwrap_or(Value::Null)))
            .collect()
    });
    list.swap(index, j);
    Some(EditOp::set(
        format!("{}/effects", layer_pointer(&layer.id)),
        Value::Array(list),
    ))
}

/// A short unit for an effect parameter, read from its documentation.
pub fn unit_of(doc: &str) -> &'static str {
    let d = doc.to_lowercase();
    if d.contains("% of height") {
        "% h"
    } else if d.contains("degree") {
        "°"
    } else if d.contains("stops") {
        "stops"
    } else if d.contains("composition units") {
        "units"
    } else if d.contains("output pixels") {
        "px"
    } else {
        ""
    }
}

// ---- slots ---------------------------------------------------------------------

/// The path to write for a picture: relative to the project's folder when it is inside it.
pub fn slot_path(picture: &std::path::Path, project_dir: &std::path::Path) -> String {
    let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let (pic, dir) = (canon(picture), canon(project_dir));
    match pic.strip_prefix(&dir) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => pic.to_string_lossy().into_owned(),
    }
}

// ---- timeline drags ------------------------------------------------------------

/// What a drag on the timeline holds.
#[derive(Clone, Debug, PartialEq)]
pub enum DragKind {
    /// The bar: the layer moves in time, with its keys.
    Move,
    /// The left edge: `in`.
    TrimIn,
    /// The right edge: `out`.
    TrimOut,
    /// A keyframe marker: every key of the layer at `from`.
    Keys { from: f64 },
}

/// One drag on the timeline, from press to release. It is shown as a
/// ghost while it lasts and becomes one edit (one undo step) on release.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeDrag {
    pub layer: String,
    pub kind: DragKind,
    /// Project time under the pointer when it was pressed.
    pub grab: f64,
    /// Project time under the pointer now.
    pub now: f64,
    /// Alt is held: snap to frames instead of quarter beats.
    pub alt: bool,
}

/// Where a drag puts things, snapped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragResult {
    /// The layer's new `in` and `out`.
    Span { start: f64, end: f64 },
    /// The new time of the keys.
    Key { to: f64 },
}

impl TimeDrag {
    pub fn new(layer: impl Into<String>, kind: DragKind, grab: f64, alt: bool) -> Self {
        Self {
            layer: layer.into(),
            kind,
            grab,
            now: grab,
            alt,
        }
    }

    /// The pointer moved (or Alt changed).
    pub fn update(&mut self, now: f64, alt: bool) {
        self.now = now;
        self.alt = alt;
    }

    /// Where the drag puts the layer or the keys if released now.
    pub fn result(&self, p: &Project, tm: &TimeMap) -> Option<DragResult> {
        let l = p.layers.iter().find(|l| l.id == self.layer)?;
        let (start, end) = (l.start, l.end.unwrap_or(p.settings.duration));
        let delta = self.now - self.grab;
        let shortest = p.frame_len().max(1e-6);
        Some(match self.kind {
            DragKind::Move => {
                let s = snap(tm, start + delta, self.alt);
                DragResult::Span {
                    start: s,
                    end: tidy(end + (s - start)),
                }
            }
            DragKind::TrimIn => {
                let s = snap(tm, start + delta, self.alt);
                DragResult::Span {
                    start: if s > end - shortest {
                        tidy(end - shortest)
                    } else {
                        s
                    },
                    end,
                }
            }
            DragKind::TrimOut => {
                let e = snap(tm, end + delta, self.alt);
                DragResult::Span {
                    start,
                    end: if e < start + shortest {
                        tidy(start + shortest)
                    } else {
                        e
                    },
                }
            }
            DragKind::Keys { from } => DragResult::Key {
                to: snap(tm, from + delta, self.alt),
            },
        })
    }

    /// The edit the drag makes, as one batch, and a line for the history.
    /// Empty when nothing would change.
    pub fn ops(&self, p: &Project, tm: &TimeMap) -> (Vec<EditOp>, String) {
        let none = (Vec::new(), String::new());
        let (Some(l), Some(result)) = (
            p.layers.iter().find(|l| l.id == self.layer),
            self.result(p, tm),
        ) else {
            return none;
        };
        let id = l.id.as_str();
        let (start, end) = (l.start, l.end.unwrap_or(p.settings.duration));
        let unit = tm.unit();
        match (&self.kind, result) {
            (DragKind::Move, DragResult::Span { start: s, end: e }) => {
                let dt = s - start;
                if dt.abs() < SAME_TIME {
                    return none;
                }
                let mut ops = vec![
                    set_field(id, "in", json!(s)),
                    set_field(id, "out", json!(e)),
                ];
                for r in layer_props(l) {
                    if let Some(moved) = r.get(p, Some(l)).and_then(|prop| shifted(prop, dt)) {
                        ops.push(set_prop(id, &r, &moved));
                    }
                }
                (
                    ops,
                    format!("move {id} by {:+} {unit} (to {}..{})", tidy(dt), s, e),
                )
            }
            (DragKind::TrimIn, DragResult::Span { start: s, .. })
                if (s - start).abs() >= SAME_TIME =>
            {
                (
                    vec![set_field(id, "in", json!(s))],
                    format!("in of {id}: {} to {s}", tidy(start)),
                )
            }
            (DragKind::TrimOut, DragResult::Span { end: e, .. })
                if (e - end).abs() >= SAME_TIME =>
            {
                (
                    vec![set_field(id, "out", json!(e))],
                    format!("out of {id}: {} to {e}", tidy(end)),
                )
            }
            (DragKind::Keys { from }, DragResult::Key { to }) if (to - from).abs() >= SAME_TIME => {
                let ops: Vec<EditOp> = layer_props(l)
                    .iter()
                    .filter_map(|r| {
                        with_key_moved(r.get(p, Some(l)), *from, to).map(|m| set_prop(id, r, &m))
                    })
                    .collect();
                let label = format!("keys of {id}: {} to {to}", tidy(*from));
                (ops, label)
            }
            _ => none,
        }
    }
}

// ---- history -------------------------------------------------------------------

/// `just now`, `12 s ago`, `3 min ago`, `2 h ago`, `4 d ago`.
pub fn ago(now: SystemTime, then: Option<SystemTime>) -> String {
    let Some(then) = then else {
        return String::new();
    };
    let d = now.duration_since(then).unwrap_or(Duration::ZERO).as_secs();
    match d {
        0..=4 => "just now".to_string(),
        5..=59 => format!("{d} s ago"),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        _ => format!("{} d ago", d / 86_400),
    }
}

/// A history summary as a person reads it. The core's change lines
/// (`~ /layers/@d/transform/rotation: [0, 0, 0] → [0, 0, 12]`) become
/// `d: transform.rotation → [0, 0, 12]`; anything else is kept as it is.
pub fn readable_summary(summary: &str) -> String {
    let (line, more) = match summary.find(" (and ") {
        Some(i) if summary.ends_with(')') => (&summary[..i], &summary[i..]),
        _ => (summary, ""),
    };
    let Some((sign, rest)) = line
        .split_at_checked(2)
        .filter(|(s, _)| matches!(*s, "~ " | "+ " | "- "))
    else {
        return summary.to_string();
    };
    if !rest.starts_with('/') {
        return summary.to_string();
    }
    let (pointer, tail) = match rest.find(": ") {
        Some(i) => (&rest[..i], &rest[i + 2..]),
        None => (rest, ""),
    };
    let (pointer, reordered) = match pointer.strip_suffix(" reordered") {
        Some(p) => (p, true),
        None => (pointer, false),
    };
    let parts: Vec<&str> = pointer.split('/').filter(|p| !p.is_empty()).collect();
    let place = match parts.as_slice() {
        ["layers", id, rest @ ..] if id.starts_with('@') && !rest.is_empty() => {
            format!("{}: {}", &id[1..], rest.join("."))
        }
        ["layers", id] if id.starts_with('@') => format!("layer {}", &id[1..]),
        [] => "project".to_string(),
        all => all.join("."),
    };
    let new = tail.rsplit(" → ").next().unwrap_or(tail);
    let text = match sign {
        "+ " => format!("{place} added"),
        "- " => format!("{place} removed"),
        _ if reordered => format!("{place} reordered: {new}"),
        _ if tail.is_empty() => format!("{place} changed"),
        _ => format!("{place} → {new}"),
    };
    format!("{text}{more}")
}

/// How an actor is shown in the History list.
pub fn actor_label(actor: &str) -> &str {
    match actor {
        edits_core::edit::AGENT => "agent",
        edits_core::edit::WINDOW => "you",
        "" | "unknown" => "?",
        other => other,
    }
}
