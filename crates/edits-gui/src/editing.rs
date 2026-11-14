//! The edits the window makes, as pure functions from the project it shows
//! to [`edits_core::edit::EditOp`]s: changing a value (a constant, or a key
//! at the playhead), keys, eases, layers, effects, slots, and the drags on
//! the timeline. No window here, so all of it is unit tested.
//!
//! Every op addresses layers by id (`/layers/@<id>/...`), so an op stays
//! right when another layer is inserted. Values are written tidily: times
//! and values rounded to 1e-6, whole numbers as integers, `linear` eases
//! (the default) left out.

use crate::timemap::TimeMap;
use edits_core::edit::{layer_pointer, EditOp};
use edits_core::{effects, Anim, Ease, Evaluator, Key, Layer, Modifier, Project, Prop, Value as Num};
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
    Effect { index: usize, kind: String, param: String },
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
            PropRef::Effect { index, param, .. } => format!("{}/effects/{index}/params/{}", layer_pointer(layer), edits_core::pointer::escape(param)),
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
            PropRef::Effect { index, kind, param } => layer?.effects.get(*index).filter(|e| &e.kind == kind)?.params.get(param),
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
            PropRef::Transform(_) | PropRef::Camera(_) if *self != PropRef::Camera("fov") => vec![0.0, 0.0, 0.0],
            PropRef::Effect { kind, param, .. } => {
                effects::spec(kind).and_then(|s| s.params.iter().find(|p| p.name == param)).map(|p| p.default.to_vec()).unwrap_or_else(|| vec![0.0])
            }
            _ => vec![39.6],
        }
    }

    /// True when the layer still has this property (the effect is still at its index).
    pub fn fits(&self, layer: &Layer) -> bool {
        match self {
            PropRef::Effect { index, kind, .. } => layer.effects.get(*index).is_some_and(|e| &e.kind == kind),
            PropRef::Transform(_) => true,
            PropRef::Camera(_) => false,
        }
    }
}

/// Every editable property of a layer: its transform, then each effect parameter of the registry.
pub fn layer_props(layer: &Layer) -> Vec<PropRef> {
    let mut out: Vec<PropRef> = TRANSFORM_PROPS.iter().map(|n| PropRef::Transform(n)).collect();
    for (index, e) in layer.effects.iter().enumerate() {
        let names: Vec<String> = match effects::spec(&e.kind) {
            Some(spec) => spec.params.iter().map(|p| p.name.to_string()).collect(),
            None => e.params.keys().cloned().collect(),
        };
        for param in names {
            out.push(PropRef::Effect { index, kind: e.kind.clone(), param });
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
    keys_of(prop).iter().position(|k| (k.t - t).abs() < SAME_TIME)
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
            let loops: Vec<Modifier> = a.modifiers.iter().filter(|m| matches!(m, Modifier::Loop { .. })).cloned().collect();
            let plain = Prop::Anim(Anim { keys: a.keys.clone(), base: a.base.clone(), modifiers: loops });
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
            let ease = a.keys.get(at.wrapping_sub(1)).or(a.keys.get(at)).map(|k| k.ease.clone()).unwrap_or_default();
            a.keys.insert(at, Key { t, v: to_num(&v), ease });
        }
    }
}

/// The property with value `v`: a constant (or a base under modifiers)
/// changes; a keyframed property gets a key at `t` (replacing one that is
/// there), and its other keys stay.
pub fn with_value(prop: Option<&Prop>, t: f64, v: Vec<f64>) -> Prop {
    match prop {
        None | Some(Prop::Const(_)) => Prop::Const(to_num(&v)),
        Some(Prop::Anim(a)) if a.keys.is_empty() => Prop::Anim(Anim { base: Some(to_num(&v)), ..a.clone() }),
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
        _ => Anim { keys: Vec::new(), base: None, modifiers: Vec::new() },
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
    let Some(Prop::Anim(a)) = prop else { return None };
    let i = key_index_at(prop, t)?;
    let mut a = a.clone();
    let removed = a.keys.remove(i);
    if !a.keys.is_empty() {
        return Some(Prop::Anim(a));
    }
    if a.modifiers.is_empty() {
        Some(Prop::Const(removed.v))
    } else {
        Some(Prop::Anim(Anim { base: Some(removed.v), ..a }))
    }
}

/// The property with key `index` eased by `ease`.
pub fn with_ease(prop: Option<&Prop>, index: usize, ease: Ease) -> Option<Prop> {
    let Some(Prop::Anim(a)) = prop else { return None };
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
        if let Modifier::Shake { amplitude: Prop::Anim(env), .. } = m {
            moved |= move_keys_in(env, from, to);
        }
        if let Modifier::Oscillate(o) = m {
            if let Prop::Anim(env) = &mut o.amplitude {
                moved |= move_keys_in(env, from, to);
            }
        }
    }
    moved
}

/// The property with its key at `from` moved to `to` (keys of a keyframed
/// shake amplitude too). `None` when it has no key at `from`.
pub fn with_key_moved(prop: Option<&Prop>, from: f64, to: f64) -> Option<Prop> {
    let Some(Prop::Anim(a)) = prop else { return None };
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
            Modifier::Shake { amplitude: Prop::Anim(env), .. } => shift_anim(env, dt),
            Modifier::Shake { .. } => {}
            Modifier::Oscillate(o) => {
                o.from = tidy(o.from + dt);
                if let Prop::Anim(env) = &mut o.amplitude {
                    shift_anim(env, dt);
                }
            }
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
    edits_core::ease::NAMES.iter().copied().chain(["bezier"]).collect()
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
                Ease::Params { name, args } if name == "decay" => args.get("rate").copied().unwrap_or(0.4),
                _ => 0.4,
            };
            Ease::Params { name: "decay".into(), args: BTreeMap::from([("rate".to_string(), rate)]) }
        }
        "bezier" => match current {
            Ease::Bezier(b) => Ease::Bezier(*b),
            _ => Ease::Bezier([0.25, 0.1, 0.25, 1.0]),
        },
        other => match current {
            // back_out's overshoot, elastic_out's period: kept when the name stays.
            Ease::Params { name, args } if name == other => Ease::Params { name: name.clone(), args: args.clone() },
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
    (2..).map(|n| format!("{base}{n}")).find(|id| !taken(id)).expect("some id is free")
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
        NewLayer::Solid => (unique_id(p, "solid"), json!({ "kind": "solid", "color": [0.5, 0.5, 0.5, 1] })),
        NewLayer::Adjustment => (unique_id(p, "adjust"), json!({ "kind": "adjustment" })),
        NewLayer::Cube(slot) => (unique_id(p, "cube"), json!({ "kind": "cube", "faces": [slot], "size": [0.6, 0.6, 0.6], "transform": { "rotation": [-20, 30, 0] } })),
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
    let raw = doc.and_then(|d| d.get("layers")?.as_array()?.iter().find(|x| x.get("id").and_then(Value::as_str) == Some(l.id.as_str()))?.as_object().cloned());
    raw.unwrap_or_else(|| clean(serde_json::to_value(l).unwrap_or(Value::Null)).as_object().cloned().unwrap_or_default())
}

/// A copy of the layer directly above it, with a new id.
pub fn duplicate_ops(p: &Project, doc: Option<&Value>, id: &str) -> Option<(Vec<EditOp>, String)> {
    let l = p.layers.iter().find(|l| l.id == id)?;
    let new_id = unique_id(p, &format!("{id}-copy"));
    let mut layer = layer_json(doc, l);
    layer.insert("id".into(), json!(new_id));
    Some((vec![EditOp::insert_layer(layer, Some(id.to_string()), None)], new_id))
}

/// Move a layer one place up (towards the front) or down in the stack.
/// `None` when it is already at that end.
pub fn move_layer_ops(p: &Project, doc: Option<&Value>, id: &str, up: bool) -> Option<Vec<EditOp>> {
    let i = p.layers.iter().position(|l| l.id == id)?;
    let j = if up { Some(i + 1).filter(|j| *j < p.layers.len())? } else { i.checked_sub(1)? };
    let neighbour = p.layers[j].id.clone();
    let layer = layer_json(doc, &p.layers[i]);
    let insert = if up { EditOp::insert_layer(layer, Some(neighbour), None) } else { EditOp::insert_layer(layer, None, Some(neighbour)) };
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
    Some(EditOp::set(format!("{}/effects/{index}/enabled", layer_pointer(&layer.id)), json!(!e.enabled)))
}

/// Swap effect `index` with the one before (`up`) or after it: the order
/// effects are applied in. The effects are taken from the file (`doc`) when it has them.
pub fn move_effect_op(doc: Option<&Value>, layer: &Layer, index: usize, up: bool) -> Option<EditOp> {
    let j = if up { index.checked_sub(1)? } else { index + 1 };
    if j >= layer.effects.len() || index >= layer.effects.len() {
        return None;
    }
    let raw = layer_json(doc, layer).get("effects").and_then(Value::as_array).cloned().filter(|a| a.len() == layer.effects.len());
    let mut list = raw.unwrap_or_else(|| layer.effects.iter().map(|e| clean(serde_json::to_value(e).unwrap_or(Value::Null))).collect());
    list.swap(index, j);
    Some(EditOp::set(format!("{}/effects", layer_pointer(&layer.id)), Value::Array(list)))
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
        Self { layer: layer.into(), kind, grab, now: grab, alt }
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
                DragResult::Span { start: s, end: tidy(end + (s - start)) }
            }
            DragKind::TrimIn => {
                let s = snap(tm, start + delta, self.alt);
                DragResult::Span { start: if s > end - shortest { tidy(end - shortest) } else { s }, end }
            }
            DragKind::TrimOut => {
                let e = snap(tm, end + delta, self.alt);
                DragResult::Span { start, end: if e < start + shortest { tidy(start + shortest) } else { e } }
            }
            DragKind::Keys { from } => DragResult::Key { to: snap(tm, from + delta, self.alt) },
        })
    }

    /// The edit the drag makes, as one batch, and a line for the history.
    /// Empty when nothing would change.
    pub fn ops(&self, p: &Project, tm: &TimeMap) -> (Vec<EditOp>, String) {
        let none = (Vec::new(), String::new());
        let (Some(l), Some(result)) = (p.layers.iter().find(|l| l.id == self.layer), self.result(p, tm)) else { return none };
        let id = l.id.as_str();
        let (start, end) = (l.start, l.end.unwrap_or(p.settings.duration));
        let unit = tm.unit();
        match (&self.kind, result) {
            (DragKind::Move, DragResult::Span { start: s, end: e }) => {
                let dt = s - start;
                if dt.abs() < SAME_TIME {
                    return none;
                }
                let mut ops = vec![set_field(id, "in", json!(s)), set_field(id, "out", json!(e))];
                for r in layer_props(l) {
                    if let Some(moved) = r.get(p, Some(l)).and_then(|prop| shifted(prop, dt)) {
                        ops.push(set_prop(id, &r, &moved));
                    }
                }
                (ops, format!("move {id} by {:+} {unit} (to {}..{})", tidy(dt), s, e))
            }
            (DragKind::TrimIn, DragResult::Span { start: s, .. }) if (s - start).abs() >= SAME_TIME => (vec![set_field(id, "in", json!(s))], format!("in of {id}: {} to {s}", tidy(start))),
            (DragKind::TrimOut, DragResult::Span { end: e, .. }) if (e - end).abs() >= SAME_TIME => (vec![set_field(id, "out", json!(e))], format!("out of {id}: {} to {e}", tidy(end))),
            (DragKind::Keys { from }, DragResult::Key { to }) if (to - from).abs() >= SAME_TIME => {
                let ops: Vec<EditOp> = layer_props(l).iter().filter_map(|r| with_key_moved(r.get(p, Some(l)), *from, to).map(|m| set_prop(id, r, &m))).collect();
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
    let Some(then) = then else { return String::new() };
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
    let Some((sign, rest)) = line.split_at_checked(2).filter(|(s, _)| matches!(*s, "~ " | "+ " | "- ")) else { return summary.to_string() };
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
        ["layers", id, rest @ ..] if id.starts_with('@') && !rest.is_empty() => format!("{}: {}", &id[1..], rest.join(".")),
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

#[cfg(test)]
mod tests {
    use super::*;
    use edits_core::edit::apply_ops;

    const PROJECT: &str = r#"{"format":"edits.project/1",
        "settings":{"width":64,"height":64,"fps":25,"duration":16,"time_base":"beats","bpm":120},
        "slots":{"a":{},"b":{}},
        "layers":[
          {"id":"a","kind":"image","slot":"a","in":0,"out":4,
           "transform":{"scale":{"keys":[{"t":1,"v":1.2,"ease":"out"},{"t":2,"v":1.0}]},
                        "position":{"base":[0,0,0],"modifiers":[{"kind":"shake","amplitude":{"keys":[{"t":1,"v":0.02},{"t":2,"v":0}]}}]}},
           "effects":[{"kind":"glow","params":{"radius":{"keys":[{"t":1,"v":6},{"t":3,"v":0}]},"intensity":2}},{"kind":"vignette"}]},
          {"id":"b","kind":"image","slot":"b","in":4},
          {"id":"top","kind":"adjustment"}
        ]}"#;

    fn project() -> Project {
        Project::from_json(PROJECT).unwrap()
    }

    fn tm(p: &Project) -> TimeMap {
        TimeMap::of(p)
    }

    fn doc() -> Value {
        serde_json::from_str(PROJECT).unwrap()
    }

    fn after(ops: &[EditOp]) -> Project {
        let v = apply_ops(&doc(), ops).expect("ops apply");
        serde_json::from_value(v).expect("still a project")
    }

    #[test]
    fn times_snap_to_quarter_beats_or_with_alt_to_frames() {
        let p = project();
        let tm = tm(&p);
        assert_eq!(snap(&tm, 1.13, false), 1.25);
        assert_eq!(snap(&tm, 1.12, false), 1.0);
        assert_eq!(snap(&tm, -0.1, false), 0.0);
        // 120 BPM, 25 fps: a frame is 0.08 beats.
        assert_eq!(snap(&tm, 1.13, true), 1.12);
        assert_eq!(snap(&tm, 0.03, true), 0.0);
        assert_eq!(Snap::of(&tm, false), Snap::QuarterBeat);
        assert_eq!(Snap::of(&tm, true), Snap::Frame);
        // A seconds project always snaps to frames.
        let secs = Project::from_json(r#"{"format":"edits.project/1","settings":{"width":64,"height":64,"fps":25,"duration":4},"layers":[]}"#).unwrap();
        let st = TimeMap::of(&secs);
        assert_eq!(snap(&st, 1.13, false), 1.12);
        assert_eq!(Snap::of(&st, false), Snap::Frame);
    }

    #[test]
    fn numbers_are_written_tidily() {
        assert_eq!(tidy(0.1 + 0.2), 0.3);
        assert_eq!(tidy(-0.0000001), 0.0);
        assert_eq!(tidy_json(json!([1.0, 2.5, 3.0000000001, -0.0])), json!([1, 2.5, 3, 0]));
        let keyed: Prop = serde_json::from_str(r#"{"keys":[{"t":1,"v":1.0},{"t":2,"v":2,"ease":"out"}]}"#).unwrap();
        assert_eq!(prop_json(&keyed), json!({ "keys": [{ "t": 1, "v": 1 }, { "t": 2, "v": 2, "ease": "out" }] }));
    }

    #[test]
    fn editing_a_constant_changes_the_constant() {
        let c: Prop = serde_json::from_str("1.0").unwrap();
        assert_eq!(prop_json(&with_value(Some(&c), 3.0, vec![1.5])), json!(1.5));
        let v: Prop = serde_json::from_str("[0, 0, 0]").unwrap();
        assert_eq!(prop_json(&with_value(Some(&v), 3.0, vec![0.1, 0.0, 0.0])), json!([0.1, 0, 0]));
        // An effect parameter the file does not set becomes a constant.
        assert_eq!(prop_json(&with_value(None, 3.0, vec![5.0])), json!(5));
        // A base under modifiers: the base changes, the shake stays.
        let shaken: Prop = serde_json::from_str(r#"{"base":[0,0,0],"modifiers":[{"kind":"shake","amplitude":0.01}]}"#).unwrap();
        let v = prop_json(&with_value(Some(&shaken), 3.0, vec![0.2, 0.0, 0.0]));
        assert_eq!(v["base"], json!([0.2, 0, 0]));
        assert_eq!(v["modifiers"][0]["kind"], json!("shake"));
    }

    #[test]
    fn editing_an_animated_value_sets_or_inserts_a_key_at_the_playhead() {
        let p = project();
        let scale = p.layers[0].transform.scale.clone();
        // Between the keys: a new key, the others stay; it continues the "out" curve.
        let v = prop_json(&with_value(Some(&scale), 1.5, vec![1.4]));
        assert_eq!(v, json!({ "keys": [{ "t": 1, "v": 1.2, "ease": "out" }, { "t": 1.5, "v": 1.4, "ease": "out" }, { "t": 2, "v": 1 }] }));
        // On a key: that key's value changes, its ease stays.
        let v = prop_json(&with_value(Some(&scale), 1.0, vec![2.0]));
        assert_eq!(v, json!({ "keys": [{ "t": 1, "v": 2, "ease": "out" }, { "t": 2, "v": 1 }] }));
        // Before the first key and after the last.
        assert_eq!(prop_json(&with_value(Some(&scale), 0.0, vec![0.5]))["keys"][0], json!({ "t": 0, "v": 0.5, "ease": "out" }));
        assert_eq!(prop_json(&with_value(Some(&scale), 3.0, vec![0.5]))["keys"][2], json!({ "t": 3, "v": 0.5 }));
    }

    #[test]
    fn key_here_turns_a_constant_into_keys_and_adds_keys() {
        let p = project();
        let c: Prop = serde_json::from_str("[0.1, 0, 0]").unwrap();
        let keyed = with_key_here(&p, Some(&c), &[0.0, 0.0, 0.0], 2.0).unwrap();
        assert_eq!(prop_json(&keyed), json!({ "keys": [{ "t": 2, "v": [0.1, 0, 0] }] }));
        // The value it holds there now, halfway down the linear segment of glow.radius.
        let radius = p.layers[0].effects[0].params["radius"].clone();
        let more = with_key_here(&p, Some(&radius), &[3.0], 2.0).unwrap();
        assert_eq!(prop_json(&more)["keys"], json!([{ "t": 1, "v": 6 }, { "t": 2, "v": 3 }, { "t": 3, "v": 0 }]));
        assert!(with_key_here(&p, Some(&radius), &[3.0], 1.0).is_none(), "there is a key there already");
        // A parameter the file does not set: its default, keyed.
        assert_eq!(prop_json(&with_key_here(&p, None, &[0.7], 1.0).unwrap()), json!({ "keys": [{ "t": 1, "v": 0.7 }] }));
        // The shake does not leak into the key: the value under it is the base.
        let pos = p.layers[0].transform.position.clone();
        let keyed = prop_json(&with_key_here(&p, Some(&pos), &[0.0, 0.0, 0.0], 1.3).unwrap());
        assert_eq!(keyed["keys"], json!([{ "t": 1.3, "v": [0, 0, 0] }]));
        assert!(keyed.get("base").is_none() && keyed["modifiers"][0]["kind"] == json!("shake"));
    }

    #[test]
    fn removing_the_last_key_leaves_a_constant() {
        let p = project();
        let scale = p.layers[0].transform.scale.clone();
        let one = without_key(Some(&scale), 1.0).unwrap();
        assert_eq!(prop_json(&one), json!({ "keys": [{ "t": 2, "v": 1 }] }));
        assert_eq!(prop_json(&without_key(Some(&one), 2.0).unwrap()), json!(1));
        assert!(without_key(Some(&scale), 1.5).is_none(), "no key at 1.5");
        assert!(without_key(None, 1.0).is_none());
    }

    #[test]
    fn eases_are_picked_by_name_and_keep_their_numbers() {
        let p = project();
        let scale = p.layers[0].transform.scale.clone();
        assert_eq!(key_at_or_before(Some(&scale), 1.5), Some(0));
        assert_eq!(key_at_or_before(Some(&scale), 0.5), None);
        assert_eq!(key_at_or_before(Some(&scale), 2.0), Some(1));
        let decay = ease_from_choice("decay", &Ease::Name("out".into()));
        assert_eq!(serde_json::to_value(&decay).unwrap(), json!({ "name": "decay", "rate": 0.4 }));
        let eased = with_ease(Some(&scale), 0, decay.clone()).unwrap();
        assert_eq!(prop_json(&eased)["keys"][0]["ease"], json!({ "name": "decay", "rate": 0.4 }));
        let faster = Ease::Params { name: "decay".into(), args: BTreeMap::from([("rate".to_string(), 0.7)]) };
        assert_eq!(ease_from_choice("decay", &faster), faster);
        assert_eq!(ease_from_choice("bezier", &Ease::Name("in".into())), Ease::Bezier([0.25, 0.1, 0.25, 1.0]));
        assert_eq!(ease_from_choice("hold", &faster), Ease::Name("hold".into()));
        assert_eq!(ease_name(&Ease::Bezier([0.0, 0.0, 1.0, 1.0])), "bezier");
        assert!(ease_choices().contains(&"decay") && ease_choices().last() == Some(&"bezier"));
        // Every choice is an ease `check` accepts.
        for name in ease_choices() {
            assert!(edits_core::ease::is_known(&ease_from_choice(name, &Ease::default())), "{name}");
        }
    }

    #[test]
    fn a_drag_is_one_edit_however_many_moves_it_took() {
        let p = project();
        let tm = tm(&p);
        let mut d = TimeDrag::new("a", DragKind::Move, 2.0, false);
        // Many pointer moves; only the last one counts.
        for t in [2.1, 2.4, 3.3, 3.0, 2.95] {
            d.update(t, false);
        }
        let mut direct = TimeDrag::new("a", DragKind::Move, 2.0, false);
        direct.update(2.95, false);
        assert_eq!(d.ops(&p, &tm), direct.ops(&p, &tm));
        // 0.95 beats snaps the start to 1: in, out, and every animated property, all in one batch.
        assert_eq!(d.result(&p, &tm), Some(DragResult::Span { start: 1.0, end: 5.0 }));
        let (ops, label) = d.ops(&p, &tm);
        assert_eq!(label, "move a by +1 beats (to 1..5)");
        let moved = after(&ops);
        let a = &moved.layers[0];
        assert_eq!((a.start, a.end), (1.0, Some(5.0)));
        assert_eq!(edits_core::Evaluator::new(&moved).num(&a.transform.scale, 2.0), 1.2, "the keys moved with the layer");
        assert_eq!(keys_of(a.effects[0].params.get("radius")).iter().map(|k| k.t).collect::<Vec<_>>(), [2.0, 4.0]);
        let Prop::Anim(pos) = &a.transform.position else { panic!() };
        let Modifier::Shake { amplitude, .. } = &pos.modifiers[0] else { panic!() };
        assert_eq!(keys_of(Some(amplitude)).iter().map(|k| k.t).collect::<Vec<_>>(), [2.0, 3.0], "the shake envelope too");
        // Released where it started: no edit at all.
        let mut back = TimeDrag::new("a", DragKind::Move, 2.0, false);
        back.update(2.05, false);
        assert!(back.ops(&p, &tm).0.is_empty());
    }

    #[test]
    fn edges_trim_in_and_out_and_keep_the_layer_at_least_a_frame_long() {
        let p = project();
        let tm = tm(&p);
        let mut d = TimeDrag::new("a", DragKind::TrimOut, 4.0, false);
        d.update(6.6, false);
        let (ops, label) = d.ops(&p, &tm);
        assert_eq!(ops, vec![EditOp::set("/layers/@a/out", json!(6.5))]);
        assert_eq!(label, "out of a: 4 to 6.5");
        // With Alt: frames (0.08 beats), not quarter beats. 6.6 beats is frame 82.5, which rounds to 83.
        d.update(6.6, true);
        assert_eq!(d.ops(&p, &tm).0, vec![EditOp::set("/layers/@a/out", json!(6.64))]);
        // An `in` dragged past the `out` stops a frame before it.
        let mut d = TimeDrag::new("a", DragKind::TrimIn, 0.0, false);
        d.update(9.0, false);
        assert_eq!(d.result(&p, &tm), Some(DragResult::Span { start: 3.92, end: 4.0 }));
        // A layer without `out` gets one when its end is dragged.
        let mut d = TimeDrag::new("b", DragKind::TrimOut, 16.0, false);
        d.update(12.2, false);
        assert_eq!(d.ops(&p, &tm).0, vec![EditOp::set("/layers/@b/out", json!(12.25))]);
        let checked = after(&d.ops(&p, &tm).0);
        assert!(!edits_core::check::has_errors(&edits_core::check::check(&checked, None)));
    }

    #[test]
    fn dragging_a_keyframe_marker_moves_every_key_at_that_time() {
        let p = project();
        let tm = tm(&p);
        let mut d = TimeDrag::new("a", DragKind::Keys { from: 1.0 }, 1.0, false);
        d.update(1.6, false);
        let (ops, label) = d.ops(&p, &tm);
        assert_eq!(label, "keys of a: 1 to 1.5");
        // scale, the shake envelope of position, glow.radius: three properties keyed at 1.
        let pointers: Vec<&str> = ops.iter().filter_map(|o| o.pointer.as_deref()).collect();
        assert_eq!(pointers, ["/layers/@a/transform/position", "/layers/@a/transform/scale", "/layers/@a/effects/0/params/radius"]);
        let moved = after(&ops);
        assert_eq!(keys_of(Some(&moved.layers[0].transform.scale)).iter().map(|k| k.t).collect::<Vec<_>>(), [1.5, 2.0]);
        // Dragged onto another key: the dragged one replaces it, the keys stay in order.
        let mut d = TimeDrag::new("a", DragKind::Keys { from: 1.0 }, 1.0, false);
        d.update(3.0, false);
        let moved = after(&d.ops(&p, &tm).0);
        let radius = keys_of(moved.layers[0].effects[0].params.get("radius"));
        assert_eq!(radius.iter().map(|k| (k.t, k.v.to_vec()[0])).collect::<Vec<_>>(), [(3.0, 6.0)]);
        let scale = keys_of(Some(&moved.layers[0].transform.scale));
        assert_eq!(scale.iter().map(|k| k.t).collect::<Vec<_>>(), [2.0, 3.0]);
    }

    #[test]
    fn layers_are_added_copied_moved_and_their_effects_edited() {
        let p = project();
        assert_eq!(unique_id(&p, "a"), "a2");
        assert_eq!(unique_id(&p, "solid"), "solid");
        let layer = new_layer(&p, &NewLayer::Image("b".into()), 2.0, 4.0);
        assert_eq!(Value::Object(layer.clone()), json!({ "id": "b2", "kind": "image", "slot": "b", "in": 2, "out": 6 }));
        // Near the end, a new layer starts early enough to fit.
        assert_eq!(new_layer(&p, &NewLayer::Solid, 15.0, 4.0)["in"], json!(12));
        let added = after(&[EditOp::insert_layer(layer, Some("a".into()), None), EditOp::insert_layer(new_layer(&p, &NewLayer::Cube("a".into()), 0.0, 4.0), None, None)]);
        assert_eq!(added.layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["a", "b2", "b", "top", "cube"]);

        let d = doc();
        let (ops, id) = duplicate_ops(&p, Some(&d), "a").unwrap();
        assert_eq!(id, "a-copy");
        let copied = after(&ops);
        assert_eq!(copied.layers[1].id, "a-copy");
        assert_eq!(serde_json::to_value(&copied.layers[1].transform).unwrap(), serde_json::to_value(&p.layers[0].transform).unwrap());
        // The copy is the layer as written in the file, not the model's longer form.
        assert_eq!(ops[0].layer.as_ref().unwrap().get("blend"), None);

        let up = after(&move_layer_ops(&p, Some(&d), "a", true).unwrap());
        assert_eq!(up.layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["b", "a", "top"]);
        let down = after(&move_layer_ops(&p, Some(&d), "top", false).unwrap());
        assert_eq!(down.layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["a", "top", "b"]);
        assert!(move_layer_ops(&p, Some(&d), "top", true).is_none() && move_layer_ops(&p, None, "a", false).is_none());

        let a = &p.layers[0];
        let more = after(&[add_effect_op(a, "rgb_split")]);
        assert_eq!(more.layers[0].effects.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["glow", "vignette", "rgb_split"]);
        let first = after(&[add_effect_op(&p.layers[2], "exposure")]);
        assert_eq!(first.layers[2].effects[0].kind, "exposure", "a layer without effects gets the array");
        let swapped = after(&[move_effect_op(Some(&d), a, 1, true).unwrap()]);
        assert_eq!(swapped.layers[0].effects.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["vignette", "glow"]);
        assert!(move_effect_op(Some(&d), a, 0, true).is_none() && move_effect_op(None, a, 1, false).is_none());
        let off = after(&[toggle_effect_op(a, 0).unwrap()]);
        assert!(!off.layers[0].effects[0].enabled);
        let gone = after(&[remove_effect_op(a, 0)]);
        assert_eq!(gone.layers[0].effects.len(), 1);
        assert!(addable_effects().all(|e| e.kind != "custom"));
    }

    #[test]
    fn props_know_where_they_are_and_what_they_default_to() {
        let p = project();
        let a = &p.layers[0];
        let props = layer_props(a);
        assert_eq!(props.len(), 5 + 4 + 3, "transform, glow's four parameters, vignette's three");
        let radius = PropRef::Effect { index: 0, kind: "glow".into(), param: "radius".into() };
        assert_eq!(radius.pointer("a"), "/layers/@a/effects/0/params/radius");
        assert_eq!(radius.label(), "glow.radius");
        assert!(radius.get(&p, Some(a)).is_some());
        let threshold = PropRef::Effect { index: 0, kind: "glow".into(), param: "threshold".into() };
        assert!(threshold.get(&p, Some(a)).is_none(), "not in the file");
        assert_eq!(threshold.default_value(), vec![0.7]);
        assert_eq!(PropRef::Transform("anchor").default_value(), vec![0.0, 0.0]);
        assert_eq!(PropRef::Camera("fov").default_value(), vec![39.6]);
        assert_eq!(PropRef::Camera("fov").pointer("ignored"), "/camera/fov");
        assert!(!PropRef::Effect { index: 1, kind: "glow".into(), param: "radius".into() }.fits(a), "effect 1 is a vignette");
        assert_eq!(unit_of("Blur sigma, % of height."), "% h");
        assert_eq!(unit_of("Direction in degrees; 0 is horizontal."), "°");
        // Writing an effect parameter the file lacks creates "params" on the way.
        let set = set_prop("a", &PropRef::Effect { index: 1, kind: "vignette".into(), param: "amount".into() }, &Prop::Const(Num::Num(0.8)));
        assert_eq!(after(&[set]).layers[0].effects[1].params["amount"].clone().to_json(), json!(0.8));
    }

    #[test]
    fn history_lines_read_naturally() {
        let now = SystemTime::now();
        assert_eq!(ago(now, Some(now)), "just now");
        assert_eq!(ago(now, Some(now - Duration::from_secs(42))), "42 s ago");
        assert_eq!(ago(now, Some(now - Duration::from_secs(185))), "3 min ago");
        assert_eq!(ago(now, Some(now - Duration::from_secs(7300))), "2 h ago");
        assert_eq!(ago(now, None), "");
        assert_eq!(actor_label("agent"), "agent");
        assert_eq!(actor_label("window"), "you");
        assert_eq!(actor_label("unknown"), "?");
    }

    #[test]
    fn dropped_pictures_get_paths_relative_to_the_project() {
        let dir = std::env::temp_dir().join(format!("edits-gui-slotpath-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("img")).unwrap();
        std::fs::write(dir.join("img/x.png"), b"").unwrap();
        assert_eq!(slot_path(&dir.join("img/x.png"), &dir), "img/x.png");
        let outside = std::env::temp_dir();
        assert!(std::path::Path::new(&slot_path(&outside, &dir)).is_absolute());
        let _ = std::fs::remove_dir_all(&dir);
    }

    trait ToJson {
        fn to_json(self) -> Value;
    }
    impl ToJson for Prop {
        fn to_json(self) -> Value {
            prop_json(&self)
        }
    }

    #[test]
    fn summaries_read_like_sentences() {
        assert_eq!(readable_summary("~ /layers/@d/transform/rotation: { \"keys\": [] } → [0, 0, 12]"), "d: transform.rotation → [0, 0, 12]");
        assert_eq!(readable_summary("~ /settings/bpm: 123 → 130"), "settings.bpm → 130");
        assert_eq!(readable_summary("+ /layers/@tex/effects/0: { \"kind\": \"glow\" } (and 2 more changes)"), "tex: effects.0 added (and 2 more changes)");
        assert_eq!(readable_summary("- /layers/@a: { \"id\": \"a\" }"), "layer a removed");
        assert_eq!(readable_summary("~ /layers reordered: a, b → b, a"), "layers reordered: b, a");
        assert_eq!(readable_summary("opacity of tex = 0.33"), "opacity of tex = 0.33");
        assert_eq!(readable_summary("reference_draft"), "reference_draft");
    }
}
