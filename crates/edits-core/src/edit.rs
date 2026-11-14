//! Editing a project file: the one way the agent (through `edits-mcp`) and
//! the person (in `edits-gui`) change a project, so both are validated the
//! same way and share one history.
//!
//! - [`apply`] takes an ordered batch of [`EditOp`]s and makes it one step:
//!   the result is parsed and checked with [`crate::check`] first, and with
//!   any error nothing is written. Otherwise the previous version is copied
//!   to `<project dir>/.edits/history/` (with who made the edit and what it
//!   did) and the file is replaced atomically, as stable pretty JSON.
//! - [`undo`] puts back the newest version in the history, whoever made the
//!   edit, and keeps the version it replaced for [`redo`].
//! - [`redo`] re-applies the newest undone version. A new edit clears what
//!   can be redone (the undone versions stay in `.edits/undone/`).
//! - [`history_list`] lists the entries, for a History panel.
//!
//! Every step takes an advisory lock on `.edits/history/<stem>.lock`, so the
//! window and the agent's server can write the same project at the same time
//! without losing a step. Errors are messages that say what to do next.
//!
//! Everything works on plain `serde_json::Value`s, so keys the model does
//! not know survive an edit (and are reported as warnings).

use crate::check::{self, Finding, Severity};
use crate::{jsonfmt, pointer, Project};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Versions kept in each of `history/` and `undone/` before the oldest are deleted.
pub const HISTORY_KEEP: usize = 50;
/// The actor recorded for edits made through the MCP server.
pub const AGENT: &str = "agent";
/// The actor recorded for edits made in the window.
pub const WINDOW: &str = "window";
/// How long a step waits for another program's step to finish.
const LOCK_WAIT: Duration = Duration::from_secs(5);

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Set,
    Remove,
    InsertLayer,
    ReplaceLayer,
    RemoveLayer,
    SetSlot,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SlotFit {
    Cover,
    Contain,
    Stretch,
}

/// One edit. Which fields are used depends on `op`:
/// `set` pointer + value; `remove` pointer; `insert_layer` layer (+ above | below);
/// `replace_layer` id + layer; `remove_layer` id; `set_slot` name (+ path, fit, focus).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EditOp {
    /// What to do.
    pub op: OpKind,
    /// set, remove: JSON pointer. Layers by id, effects by kind or index:
    /// /layers/@finish/effects/@exposure/params/stops. In `set`, missing objects
    /// are created and /effects/- appends to an array.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// set: the new JSON value (number, string, array or object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// insert_layer, replace_layer: the layer object ({"id", "kind", ...}).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<Map<String, Value>>,
    /// replace_layer, remove_layer: id of the layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// insert_layer: put the new layer directly above (in front of) the layer with this id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub above: Option<String>,
    /// insert_layer: put the new layer directly below (behind) the layer with this id. With neither, it goes on top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below: Option<String>,
    /// set_slot: slot name (created if missing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// set_slot: image file, relative to the project file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// set_slot: how the picture fills its layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<SlotFit>,
    /// set_slot: point of interest kept centred by cover, [x, y] in 0..1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<[f64; 2]>,
}

/// The same op type under a shorter name.
pub type Op = EditOp;

impl EditOp {
    fn of(op: OpKind) -> Self {
        Self { op, pointer: None, value: None, layer: None, id: None, above: None, below: None, name: None, path: None, fit: None, focus: None }
    }
    /// `set`: put `value` at `pointer`.
    pub fn set(pointer: impl Into<String>, value: Value) -> Self {
        Self { pointer: Some(pointer.into()), value: Some(value), ..Self::of(OpKind::Set) }
    }
    /// `remove`: delete the key or array element at `pointer`.
    pub fn remove(pointer: impl Into<String>) -> Self {
        Self { pointer: Some(pointer.into()), ..Self::of(OpKind::Remove) }
    }
    /// `insert_layer`: directly above the layer `above`, below the layer `below`, or on top.
    pub fn insert_layer(layer: Map<String, Value>, above: Option<String>, below: Option<String>) -> Self {
        Self { layer: Some(layer), above, below, ..Self::of(OpKind::InsertLayer) }
    }
    /// `replace_layer`: the layer with this id becomes `layer`, in the same place.
    pub fn replace_layer(id: impl Into<String>, layer: Map<String, Value>) -> Self {
        Self { id: Some(id.into()), layer: Some(layer), ..Self::of(OpKind::ReplaceLayer) }
    }
    /// `remove_layer`.
    pub fn remove_layer(id: impl Into<String>) -> Self {
        Self { id: Some(id.into()), ..Self::of(OpKind::RemoveLayer) }
    }
    /// `set_slot`: create the slot or change what is given.
    pub fn set_slot(name: impl Into<String>, path: Option<String>, fit: Option<SlotFit>, focus: Option<[f64; 2]>) -> Self {
        Self { name: Some(name.into()), path, fit, focus, ..Self::of(OpKind::SetSlot) }
    }
}

/// The pointer of a layer by id: `/layers/@<id>`.
pub fn layer_pointer(id: &str) -> String {
    format!("/layers/@{}", pointer::escape(id))
}

fn need<'a, T>(field: &'a Option<T>, op: &str, name: &str) -> Result<&'a T, String> {
    field.as_ref().ok_or_else(|| format!("`{op}` needs \"{name}\""))
}

fn layers_mut(doc: &mut Value) -> Result<&mut Vec<Value>, String> {
    doc.get_mut("layers").and_then(Value::as_array_mut).ok_or_else(|| "the project has no \"layers\" array".to_string())
}

fn layer_ids(layers: &[Value]) -> String {
    let ids: Vec<&str> = layers.iter().filter_map(|l| l.get("id").and_then(Value::as_str)).collect();
    if ids.is_empty() {
        "(none)".into()
    } else {
        ids.join(", ")
    }
}

fn layer_index(layers: &[Value], id: &str) -> Result<usize, String> {
    layers
        .iter()
        .position(|l| l.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| format!("no layer has id \"{id}\"; layer ids, bottom to top: {}", layer_ids(layers)))
}

/// Apply one op to the document.
pub fn apply_op(doc: &mut Value, op: &EditOp) -> Result<(), String> {
    match op.op {
        OpKind::Set => {
            let ptr = need(&op.pointer, "set", "pointer")?;
            let value = op.value.clone().filter(|v| !v.is_null()).ok_or("`set` needs \"value\" (to delete something, use `remove`)")?;
            pointer::set(doc, ptr, value)
        }
        OpKind::Remove => pointer::remove(doc, need(&op.pointer, "remove", "pointer")?).map(|_| ()),
        OpKind::InsertLayer => {
            let layer = need(&op.layer, "insert_layer", "layer")?;
            let id = layer.get("id").and_then(Value::as_str).ok_or("`insert_layer`: the layer needs a string \"id\"")?.to_string();
            let layers = layers_mut(doc)?;
            if layers.iter().any(|l| l.get("id").and_then(Value::as_str) == Some(id.as_str())) {
                return Err(format!("`insert_layer`: a layer with id \"{id}\" already exists; choose another id, or use `replace_layer`"));
            }
            let at = match (&op.above, &op.below) {
                (Some(_), Some(_)) => return Err("`insert_layer`: give \"above\" or \"below\", not both".into()),
                (Some(a), None) => layer_index(layers, a)? + 1,
                (None, Some(b)) => layer_index(layers, b)?,
                (None, None) => layers.len(),
            };
            layers.insert(at, Value::Object(layer.clone()));
            Ok(())
        }
        OpKind::ReplaceLayer => {
            let id = need(&op.id, "replace_layer", "id")?;
            let mut layer = need(&op.layer, "replace_layer", "layer")?.clone();
            let layers = layers_mut(doc)?;
            let at = layer_index(layers, id)?;
            match layer.get("id").and_then(Value::as_str).map(str::to_string) {
                None => {
                    layer.insert("id".into(), json!(id));
                }
                Some(new_id) if new_id != *id => {
                    if layers.iter().any(|l| l.get("id").and_then(Value::as_str) == Some(new_id.as_str())) {
                        return Err(format!("`replace_layer`: cannot rename \"{id}\" to \"{new_id}\": that id is taken"));
                    }
                }
                Some(_) => {}
            }
            layers[at] = Value::Object(layer);
            Ok(())
        }
        OpKind::RemoveLayer => {
            let id = need(&op.id, "remove_layer", "id")?;
            let layers = layers_mut(doc)?;
            let at = layer_index(layers, id)?;
            layers.remove(at);
            Ok(())
        }
        OpKind::SetSlot => {
            let name = need(&op.name, "set_slot", "name")?;
            if op.path.is_none() && op.fit.is_none() && op.focus.is_none() {
                return Err("`set_slot` needs at least one of \"path\", \"fit\", \"focus\"".into());
            }
            let root = doc.as_object_mut().ok_or("the project is not a JSON object")?;
            let slots = root.entry("slots").or_insert_with(|| json!({}));
            let slots = slots.as_object_mut().ok_or("\"slots\" is not an object")?;
            let slot = slots.entry(name.clone()).or_insert_with(|| json!({}));
            let slot = slot.as_object_mut().ok_or_else(|| format!("slot \"{name}\" is not an object"))?;
            if let Some(p) = &op.path {
                slot.insert("path".into(), json!(p));
            }
            if let Some(f) = &op.fit {
                slot.insert("fit".into(), json!(f));
            }
            if let Some(f) = &op.focus {
                slot.insert("focus".into(), json!(f));
            }
            Ok(())
        }
    }
}

/// Apply ops in order to a copy of the document. The error names the op that failed.
pub fn apply_ops(doc: &Value, ops: &[EditOp]) -> Result<Value, String> {
    let mut out = doc.clone();
    for (i, op) in ops.iter().enumerate() {
        apply_op(&mut out, op).map_err(|e| format!("ops[{i}]: {e}"))?;
    }
    Ok(out)
}

// ---- diff ------------------------------------------------------------------

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Change {
    /// Where, as a pointer `project_edit` and `project_read` accept.
    pub pointer: String,
    /// `added`, `removed`, `changed` or `reordered`.
    pub change: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
}

const BRIEF: usize = 72;

fn ids_of(a: &[Value]) -> Option<Vec<&str>> {
    let ids: Vec<&str> = a.iter().map(|e| e.get("id").and_then(Value::as_str)).collect::<Option<_>>()?;
    let mut seen = std::collections::BTreeSet::new();
    (!ids.is_empty() && ids.iter().all(|i| seen.insert(*i))).then_some(ids)
}

fn diff_into(old: &Value, new: &Value, path: &str, out: &mut Vec<Change>) {
    if old == new {
        return;
    }
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for k in keys {
                let p = format!("{path}/{}", pointer::escape(k));
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => diff_into(x, y, &p, out),
                    (None, Some(y)) => out.push(Change { pointer: p, change: "added", old: None, new: Some(jsonfmt::brief(y, BRIEF)) }),
                    (Some(x), None) => out.push(Change { pointer: p, change: "removed", old: Some(jsonfmt::brief(x, BRIEF)), new: None }),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            // Arrays of things with ids (layers) are compared by id, so an
            // inserted layer is one line, not a shift of everything above it.
            if let (Some(ia), Some(ib)) = (ids_of(a), ids_of(b)) {
                for (i, id) in ia.iter().enumerate() {
                    let p = format!("{path}/@{}", pointer::escape(id));
                    match ib.iter().position(|x| x == id) {
                        Some(j) => diff_into(&a[i], &b[j], &p, out),
                        None => out.push(Change { pointer: p, change: "removed", old: Some(jsonfmt::brief(&a[i], BRIEF)), new: None }),
                    }
                }
                for (j, id) in ib.iter().enumerate() {
                    if !ia.contains(id) {
                        out.push(Change { pointer: format!("{path}/@{}", pointer::escape(id)), change: "added", old: None, new: Some(jsonfmt::brief(&b[j], BRIEF)) });
                    }
                }
                let kept_a: Vec<&&str> = ia.iter().filter(|i| ib.contains(i)).collect();
                let kept_b: Vec<&&str> = ib.iter().filter(|i| ia.contains(i)).collect();
                if kept_a != kept_b {
                    out.push(Change { pointer: path.to_string(), change: "reordered", old: Some(ia.join(", ")), new: Some(ib.join(", ")) });
                }
            } else if a.len() == b.len() {
                for (i, (x, y)) in a.iter().zip(b).enumerate() {
                    diff_into(x, y, &format!("{path}/{i}"), out);
                }
            } else if a.iter().zip(b).all(|(x, y)| x == y) {
                // Appended to, or cut at, the end: name the elements that came or went.
                let common = a.len().min(b.len());
                for (i, y) in b.iter().enumerate().skip(common) {
                    out.push(Change { pointer: format!("{path}/{i}"), change: "added", old: None, new: Some(jsonfmt::brief(y, BRIEF)) });
                }
                for (i, x) in a.iter().enumerate().skip(common) {
                    out.push(Change { pointer: format!("{path}/{i}"), change: "removed", old: Some(jsonfmt::brief(x, BRIEF)), new: None });
                }
            } else {
                out.push(Change { pointer: path.to_string(), change: "changed", old: Some(format!("{} items", a.len())), new: Some(format!("{} items", b.len())) });
            }
        }
        _ => out.push(Change { pointer: path.to_string(), change: "changed", old: Some(jsonfmt::brief(old, BRIEF)), new: Some(jsonfmt::brief(new, BRIEF)) }),
    }
}

/// What differs between two versions of a project, deepest changed values first to last in document order.
pub fn diff(old: &Value, new: &Value) -> Vec<Change> {
    let mut out = Vec::new();
    diff_into(old, new, "", &mut out);
    out
}

pub fn change_line(c: &Change) -> String {
    let at = if c.pointer.is_empty() { "/" } else { &c.pointer };
    match (c.change, &c.old, &c.new) {
        ("added", _, Some(n)) => format!("+ {at}: {n}"),
        ("removed", Some(o), _) => format!("- {at}: {o}"),
        ("reordered", Some(o), Some(n)) => format!("~ {at} reordered: {o} → {n}"),
        (_, Some(o), Some(n)) => format!("~ {at}: {o} → {n}"),
        _ => format!("~ {at}"),
    }
}

/// One line that says what a set of changes did, for the history.
pub fn summarize(changes: &[Change]) -> String {
    match changes {
        [] => "no changes".to_string(),
        [one] => change_line(one),
        [first, rest @ ..] => format!("{} (and {} more change{})", change_line(first), rest.len(), if rest.len() == 1 { "" } else { "s" }),
    }
}

// ---- validation ------------------------------------------------------------

fn finding(severity: Severity, path: String, message: String, hint: Option<&str>) -> Finding {
    Finding { severity, path, message, hint: hint.map(str::to_string) }
}

/// Keys the renderer would silently ignore: present in the document, absent
/// from what the parsed project writes back. Usually a typo (`"transfrom"`).
/// A known field written at its default value (`"units": "characters"`) is
/// also left out when the project is written back; `read_by_parser` tells
/// the two apart. `raw` is the path with plain array indices.
fn ignored_keys(doc: &Value, parsed: &Value, path: &str, raw: &str, read_by_parser: &dyn Fn(&str, &str) -> bool, out: &mut Vec<Finding>) {
    match (doc, parsed) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                let p = format!("{path}/{}", pointer::escape(k));
                let r = format!("{raw}/{}", pointer::escape(k));
                match b.get(k) {
                    Some(w) => ignored_keys(v, w, &p, &r, read_by_parser, out),
                    // Empty and null values are written back as "absent"; they are not typos.
                    None if v.is_null() || v.as_array().is_some_and(Vec::is_empty) || v.as_object().is_some_and(Map::is_empty) => {}
                    None if read_by_parser(raw, k) => {}
                    None => {
                        let known: Vec<&str> = b.keys().map(String::as_str).collect();
                        out.push(finding(Severity::Warning, p, format!("unknown key \"{k}\": the renderer ignores it"), Some(&format!("known keys here: {}", known.join(", ")))));
                    }
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (v, w)) in a.iter().zip(b).enumerate() {
                let p = match v.get("id").and_then(Value::as_str) {
                    Some(id) if path == "/layers" || (path.starts_with("/comps/") && path.ends_with("/layers")) => format!("{path}/@{}", pointer::escape(id)),
                    _ => format!("{path}/{i}"),
                };
                ignored_keys(v, w, &p, &format!("{raw}/{i}"), read_by_parser, out);
            }
        }
        _ => {}
    }
}

/// Whether the parser reads `key` of the object at `parent` (a plain pointer):
/// the key is given a value no field accepts, and a parse that then fails read it.
fn read_by_parser(doc: &Value, parent: &str, key: &str) -> bool {
    let mut probe = doc.clone();
    let Some(obj) = probe.pointer_mut(parent).and_then(Value::as_object_mut) else { return false };
    obj.insert(key.to_string(), serde_json::json!({ "\u{0}edits-probe": [null, {}] }));
    Project::from_json(&jsonfmt::to_file_text(&probe)).is_err()
}

/// Files a project refers to: (pointer, path as written).
pub fn file_refs(doc: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(p) = doc.pointer("/audio/path").and_then(Value::as_str) {
        out.push(("/audio/path".to_string(), p.to_string()));
    }
    for (i, clip) in doc.get("audio_clips").and_then(Value::as_array).into_iter().flatten().enumerate() {
        if let Some(p) = clip.get("path").and_then(Value::as_str) {
            let at = match clip.get("id").and_then(Value::as_str) {
                Some(id) => format!("/audio_clips/@{}", pointer::escape(id)),
                None => format!("/audio_clips/{i}"),
            };
            out.push((format!("{at}/path"), p.to_string()));
        }
    }
    if let Some(slots) = doc.get("slots").and_then(Value::as_object) {
        for (name, slot) in slots {
            if let Some(p) = slot.get("path").and_then(Value::as_str) {
                out.push((format!("/slots/{}/path", pointer::escape(name)), p.to_string()));
            }
        }
    }
    // The project's layers, then the layers of every comp.
    let comps = doc.get("comps").and_then(Value::as_object).into_iter().flatten();
    let stacks = std::iter::once(("/layers".to_string(), doc.get("layers"))).chain(comps.map(|(name, c)| (format!("/comps/{}/layers", pointer::escape(name)), c.get("layers"))));
    let layers = stacks.flat_map(|(at, layers)| layers.and_then(Value::as_array).into_iter().flatten().enumerate().map(move |(i, l)| (at.clone(), i, l)));
    for (at, li, layer) in layers {
        let lp = match layer.get("id").and_then(Value::as_str) {
            Some(id) => format!("{at}/@{}", pointer::escape(id)),
            None => format!("{at}/{li}"),
        };
        for (ei, effect) in layer.get("effects").and_then(Value::as_array).into_iter().flatten().enumerate() {
            if let Some(p) = effect.get("source").and_then(Value::as_str) {
                out.push((format!("{lp}/effects/{ei}/source"), p.to_string()));
            }
        }
    }
    out
}

/// More findings for a document, beyond what [`validate`] finds: the MCP
/// server uses this to refuse files outside its workspace. Called with the
/// document and the project's folder.
pub type ExtraCheck<'a> = &'a dyn Fn(&Value, &Path) -> Vec<Finding>;

/// Validate a document the way the renderer will see it: parse it as a
/// project, run `edits_core::check`, and add what only the raw JSON can show
/// (ignored keys, and whatever `extra` finds). Errors first; paths are pointers.
pub fn validate(doc: &Value, project_dir: &Path, extra: Option<ExtraCheck>) -> Vec<Finding> {
    let text = jsonfmt::to_file_text(doc);
    let project = match Project::from_json(&text) {
        Ok(p) => p,
        Err(e) => {
            // serde reports a line and column of the text it was given; show that line.
            let message = format!("{e:#}");
            let line = message.rsplit_once(" at line ").and_then(|(_, rest)| rest.split_whitespace().next()).and_then(|n| n.parse::<usize>().ok());
            let context = line.and_then(|n| text.lines().nth(n.saturating_sub(1))).map(|l| format!("near: {}", l.trim().chars().take(160).collect::<String>()));
            let hint = match context {
                Some(c) => format!("{c} (the result is not a valid project; see docs/FORMAT.md)"),
                None => "the result is not a valid project; see docs/FORMAT.md".to_string(),
            };
            return vec![finding(Severity::Error, String::new(), message, Some(&hint))];
        }
    };
    let mut findings: Vec<Finding> = check::check(&project, Some(project_dir));
    for f in &mut findings {
        if let Some(p) = pointer::from_check_path(&f.path) {
            f.path = p;
        }
    }
    if let Some(extra) = extra {
        findings.extend(extra(doc, project_dir));
    }
    findings.extend(ignored_key_findings(doc, &project));
    findings.sort_by_key(|f| f.severity != Severity::Error);
    findings
}

/// Keys in the raw document that the renderer ignores (warnings).
pub fn ignored_key_findings(doc: &Value, project: &Project) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Ok(parsed) = serde_json::to_value(project) {
        ignored_keys(doc, &parsed, "", "", &|parent, key| read_by_parser(doc, parent, key), &mut findings);
    }
    findings
}

pub fn finding_line(f: &Finding) -> String {
    let level = if f.severity == Severity::Error { "error" } else { "warning" };
    let at = if f.path.is_empty() { String::new() } else { format!(" {}", f.path) };
    let hint = f.hint.as_deref().map(|h| format!(" ({h})")).unwrap_or_default();
    format!("{level}{at}: {}{hint}", f.message)
}

// ---- files -----------------------------------------------------------------

/// `20261112T145012.123Z`: sorts by time, safe in a file name.
pub fn utc_stamp(now: SystemTime) -> String {
    let d = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}T{:02}{:02}{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, d.subsec_millis())
}

/// The time a stamp written by [`utc_stamp`] stands for (a `-n` serial is ignored).
pub fn parse_stamp(stamp: &str) -> Option<SystemTime> {
    let core = stamp.split_once('-').map_or(stamp, |(s, _)| s);
    if !is_stamp(core) {
        return None;
    }
    let n = |a: usize, b: usize| core[a..b].parse::<i64>().ok();
    let (y, m, d) = (n(0, 4)?, n(4, 6)?, n(6, 8)?);
    let (hh, mm, ss, ms) = (n(9, 11)?, n(11, 13)?, n(13, 15)?, n(16, 19)?);
    // Day count from a civil date (the inverse of the algorithm above).
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hh * 3600 + mm * 60 + ss;
    let secs = u64::try_from(secs).ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_millis(ms as u64))
}

fn is_stamp(s: &str) -> bool {
    let (stamp, serial) = s.split_once('-').unwrap_or((s, "0"));
    let b = stamp.as_bytes();
    b.len() == 20
        && b[8] == b'T'
        && b[15] == b'.'
        && b[19] == b'Z'
        && b.iter().enumerate().all(|(i, c)| matches!(i, 8 | 15 | 19) || c.is_ascii_digit())
        && !serial.is_empty()
        && serial.bytes().all(|c| c.is_ascii_digit())
}

/// Create `<dir>/.edits/<sub>`, with a `.gitignore` so nothing in it is committed by accident.
pub fn state_dir(dir: &Path, sub: &str) -> std::io::Result<PathBuf> {
    let base = dir.join(".edits");
    let path = base.join(sub);
    std::fs::create_dir_all(&path)?;
    let ignore = base.join(".gitignore");
    if !ignore.exists() {
        let _ = std::fs::write(&ignore, "# written by edits: undo history, render previews and job logs\n*\n");
    }
    Ok(path)
}

pub fn project_dir(project: &Path) -> PathBuf {
    match project.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn stem(project: &Path) -> String {
    project.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into())
}

/// `<stem>.<stamp>.json` files of this project in `<project dir>/.edits/<sub>`, oldest first.
fn stamped(project: &Path, sub: &str) -> Vec<PathBuf> {
    let dir = project_dir(project).join(".edits").join(sub);
    let prefix = format!("{}.", stem(project));
    let core = |p: &Path| -> Option<String> {
        let name = p.file_name()?.to_str()?;
        let core = name.strip_prefix(&prefix)?.strip_suffix(".json")?;
        is_stamp(core).then(|| core.to_string())
    };
    let mut found: Vec<(String, u64, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| {
            let c = core(&p)?;
            // Same-millisecond files carry a serial (`-1`, `-2`); compare it as a number.
            let (stamp, serial) = c.split_once('-').unwrap_or((&c, "0"));
            Some((stamp.to_string(), serial.parse::<u64>().unwrap_or(0), p))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, _, p)| p).collect()
}

/// Backups of this project, oldest first.
pub fn history(project: &Path) -> Vec<PathBuf> {
    stamped(project, "history")
}

/// Versions that [`redo`] can bring back, the next one last.
pub fn redo_stack(project: &Path) -> Vec<PathBuf> {
    stamped(project, "redo")
}

fn unique(dir: &Path, stem: &str) -> PathBuf {
    let stamp = utc_stamp(SystemTime::now());
    let mut path = dir.join(format!("{stem}.{stamp}.json"));
    let mut serial = 0;
    while path.exists() {
        serial += 1;
        path = dir.join(format!("{stem}.{stamp}-{serial}.json"));
    }
    path
}

/// Replace a file's content in one step: a reader sees the old text or the new, never half.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// What is recorded next to each saved version: who made the edit that
/// followed it, and what that edit did.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct EntryMeta {
    /// `agent`, `window`, or another name a program gives itself.
    pub actor: String,
    /// One line: what the edit did.
    pub summary: String,
}

/// `<stem>.<stamp>.json` → `<stem>.<stamp>.meta.json`.
fn meta_path(version: &Path) -> PathBuf {
    version.with_extension("meta.json")
}

fn read_meta(version: &Path) -> EntryMeta {
    std::fs::read_to_string(meta_path(version)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| EntryMeta { actor: "unknown".into(), summary: String::new() })
}

fn write_meta(version: &Path, meta: &EntryMeta) {
    if let Ok(text) = serde_json::to_string(meta) {
        let _ = std::fs::write(meta_path(version), text);
    }
}

fn remove_version(version: &Path) {
    let _ = std::fs::remove_file(version);
    let _ = std::fs::remove_file(meta_path(version));
}

fn prune(all: &[PathBuf]) {
    for old in all.iter().take(all.len().saturating_sub(HISTORY_KEEP)) {
        remove_version(old);
    }
}

/// Copy `text` to a new version in `.edits/<sub>/` with its meta.
fn save_version(project: &Path, sub: &str, text: &str, meta: &EntryMeta) -> std::io::Result<PathBuf> {
    let dir = state_dir(&project_dir(project), sub)?;
    let to = unique(&dir, &stem(project));
    std::fs::write(&to, text)?;
    write_meta(&to, meta);
    prune(&stamped(project, sub));
    Ok(to)
}

/// Forget what can be redone (a new edit makes it meaningless). The undone
/// versions themselves stay in `.edits/undone/`.
fn clear_redo(project: &Path) {
    for v in redo_stack(project) {
        remove_version(&v);
    }
}

/// Save the current file under `.edits/history/` next to it, before a
/// program other than [`apply`] replaces it (`reference_draft`). The next
/// [`undo`] restores it. Returns the backup's path.
pub fn backup(project: &Path, actor: &str, summary: &str) -> std::io::Result<PathBuf> {
    let text = std::fs::read_to_string(project)?;
    let saved = save_version(project, "history", &text, &EntryMeta { actor: actor.into(), summary: summary.into() })?;
    clear_redo(project);
    Ok(saved)
}

/// A hash of a file's text (FNV-1a), to tell whether a file is still the
/// version an edit was prepared against.
pub fn text_hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// An advisory lock on the project's history, held for one step. Released when dropped.
struct StepLock {
    _file: std::fs::File,
}

fn lock(project: &Path) -> Result<StepLock, String> {
    let dir = state_dir(&project_dir(project), "history").map_err(|e| format!("cannot create the history folder, so nothing was changed: {e}"))?;
    let path = dir.join(format!("{}.lock", stem(project)));
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(StepLock { _file: file }),
            // A file system without locks: carry on unlocked rather than not at all.
            Err(std::fs::TryLockError::Error(_)) => return Ok(StepLock { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(format!("another program has been changing {} for over {} s ({} is locked); nothing was changed, try again", project.display(), LOCK_WAIT.as_secs(), path.display()))
            }
        }
    }
}

// ---- the steps -------------------------------------------------------------

/// How [`apply`] runs.
#[derive(Clone, Copy)]
pub struct EditOptions<'a> {
    /// Validate and report, write nothing.
    pub dry_run: bool,
    /// Who makes the edit, recorded in the history: [`AGENT`], [`WINDOW`], ...
    pub actor: &'a str,
    /// What the edit does, in a few words, for the history. Without it the
    /// first change is described.
    pub label: Option<&'a str>,
    /// The [`text_hash`] of the file the ops were prepared against. When the
    /// file is no longer that text (someone else wrote it meanwhile), nothing
    /// is changed and the error says so.
    pub expect: Option<u64>,
    /// More checks of the result; an error among them blocks the edit.
    pub extra: Option<ExtraCheck<'a>>,
}

impl Default for EditOptions<'_> {
    fn default() -> Self {
        Self { dry_run: false, actor: AGENT, label: None, expect: None, extra: None }
    }
}

#[derive(Debug)]
pub struct EditOutcome {
    /// False when the result had errors: nothing was written.
    pub ok: bool,
    /// True when the file on disk was replaced.
    pub written: bool,
    pub dry_run: bool,
    pub changes: Vec<Change>,
    pub findings: Vec<Finding>,
    pub backup: Option<PathBuf>,
    /// How many of the errors were in the file before this edit.
    pub preexisting: usize,
    /// The line recorded in the history.
    pub summary: String,
}

/// The error of [`apply`] when [`EditOptions::expect`] does not match the file.
pub const STALE: &str = "the project file changed on disk after this edit was prepared (another program or the agent wrote it); nothing was changed";

/// Apply `ops` to the project file as one step: validated, written
/// atomically, backed up for [`undo`]. See the module documentation.
pub fn apply(project: &Path, ops: &[EditOp], opts: &EditOptions) -> Result<EditOutcome, String> {
    if ops.is_empty() {
        return Err("\"ops\" is empty: give at least one edit".into());
    }
    apply_with(project, opts, &|doc| apply_ops(doc, ops))
}

/// Like [`apply`], but the change is a function of the current document:
/// what presets and beat operations use, since what they write depends on
/// what is there (existing keys, free layer ids). The function runs under
/// the step lock, on the file as it is now. Times written as strings
/// (`"b4"`, `"drop"`) are resolved before validation, here as in [`apply`].
pub fn apply_with(project: &Path, opts: &EditOptions, change: &dyn Fn(&Value) -> Result<Value, String>) -> Result<EditOutcome, String> {
    let _lock = if opts.dry_run { None } else { Some(lock(project)?) };
    let text = std::fs::read_to_string(project).map_err(|e| format!("cannot read {}: {e}", project.display()))?;
    if opts.expect.is_some_and(|h| h != text_hash(&text)) {
        return Err(STALE.to_string());
    }
    let old: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON ({e}); fix the file by hand or restore a backup with project_undo", project.display()))?;
    let mut new = change(&old).map_err(|e| format!("{e}. Nothing was changed."))?;
    crate::time::resolve_doc(&mut new, None).map_err(|e| format!("{e}. Nothing was changed."))?;
    let dir = project_dir(project);
    let mut findings = validate(&new, &dir, opts.extra);
    let changes = diff(&old, &new);
    let summary = opts.label.map(str::to_string).unwrap_or_else(|| summarize(&changes));
    if check::has_errors(&findings) {
        // An error that was in the file before this edit blocks it all the same
        // (a broken project is never written), so say which ones those are: the
        // fix belongs in the same batch.
        let before: std::collections::BTreeSet<(String, String)> =
            validate(&old, &dir, opts.extra).into_iter().filter(|f| f.severity == Severity::Error).map(|f| (f.path, f.message)).collect();
        let mut preexisting = 0;
        for f in findings.iter_mut().filter(|f| f.severity == Severity::Error) {
            if before.contains(&(f.path.clone(), f.message.clone())) {
                preexisting += 1;
                let hint = f.hint.take().map(|h| format!("{h}; ")).unwrap_or_default();
                f.hint = Some(format!("{hint}already in the file before this edit: fix it in the same batch"));
            }
        }
        return Ok(EditOutcome { ok: false, written: false, dry_run: opts.dry_run, changes, findings, backup: None, preexisting, summary });
    }
    if opts.dry_run || changes.is_empty() {
        return Ok(EditOutcome { ok: true, written: false, dry_run: opts.dry_run, changes, findings, backup: None, preexisting: 0, summary });
    }
    let meta = EntryMeta { actor: opts.actor.to_string(), summary: summary.clone() };
    let saved = save_version(project, "history", &text, &meta).map_err(|e| format!("cannot write the backup, so nothing was changed: {e}"))?;
    if let Err(e) = write_atomic(project, &jsonfmt::to_file_text(&new)) {
        remove_version(&saved);
        return Err(format!("cannot write {}: {e}", project.display()));
    }
    clear_redo(project);
    Ok(EditOutcome { ok: true, written: true, dry_run: false, changes, findings, backup: Some(saved), preexisting: 0, summary })
}

#[derive(Debug)]
pub struct UndoOutcome {
    /// The saved version that was put back (it is removed from its stack).
    pub restored: PathBuf,
    /// Where the version that was replaced is kept: for an undo, a copy in
    /// `.edits/undone/` (it is also on the redo stack); for a redo, the
    /// backup in `.edits/history/`.
    pub kept: PathBuf,
    pub changes: Vec<Change>,
    /// Versions left on the stack this step took from.
    pub remaining: usize,
    /// Who made the edit that was undone or redone, and what it did.
    pub entry: EntryMeta,
}

fn changes_between(a: &str, b: &str) -> Vec<Change> {
    match (serde_json::from_str::<Value>(a), serde_json::from_str::<Value>(b)) {
        (Ok(a), Ok(b)) => diff(&a, &b),
        _ => Vec::new(),
    }
}

/// Restore the newest backup of the project, whoever made the edit after it.
/// Repeated calls walk further back. The version it replaces can be brought
/// back with [`redo`].
pub fn undo(project: &Path) -> Result<UndoOutcome, String> {
    let _lock = lock(project)?;
    let all = history(project);
    let Some(latest) = all.last() else {
        return Err(format!("no backups for {}: nothing to undo (backups are made by project_edit and reference_draft, under .edits/history/ next to the project)", project.display()));
    };
    let restored_text = std::fs::read_to_string(latest).map_err(|e| format!("cannot read the backup {}: {e}", latest.display()))?;
    let current_text = std::fs::read_to_string(project).unwrap_or_default();
    let entry = read_meta(latest);
    let kept = save_version(project, "undone", &current_text, &entry).map_err(|e| format!("cannot save the current version, so nothing was changed: {e}"))?;
    let redo_copy = save_version(project, "redo", &current_text, &entry).map_err(|e| format!("cannot save the current version, so nothing was changed: {e}"))?;
    if let Err(e) = write_atomic(project, &restored_text) {
        remove_version(&redo_copy);
        return Err(format!("cannot write {}: {e}", project.display()));
    }
    remove_version(latest);
    Ok(UndoOutcome { restored: latest.clone(), kept, changes: changes_between(&current_text, &restored_text), remaining: all.len() - 1, entry })
}

/// Bring back the version the last [`undo`] replaced. Only possible until
/// the next edit.
pub fn redo(project: &Path) -> Result<UndoOutcome, String> {
    let _lock = lock(project)?;
    let all = redo_stack(project);
    let Some(next) = all.last() else {
        return Err(format!("nothing to redo for {} (only an undo can be redone, and only until the next edit)", project.display()));
    };
    let redo_text = std::fs::read_to_string(next).map_err(|e| format!("cannot read {}: {e}", next.display()))?;
    let current_text = std::fs::read_to_string(project).unwrap_or_default();
    let entry = read_meta(next);
    let kept = save_version(project, "history", &current_text, &entry).map_err(|e| format!("cannot write the backup, so nothing was changed: {e}"))?;
    if let Err(e) = write_atomic(project, &redo_text) {
        remove_version(&kept);
        return Err(format!("cannot write {}: {e}", project.display()));
    }
    remove_version(next);
    Ok(UndoOutcome { restored: next.clone(), kept, changes: changes_between(&current_text, &redo_text), remaining: all.len() - 1, entry })
}

/// One entry of the history: an edit that can be undone (or redone).
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    /// The saved version: before the edit for an undo entry, after it for a redo entry.
    pub file: PathBuf,
    /// When the version was saved (for an undo entry: when the edit was made).
    pub time: Option<SystemTime>,
    pub actor: String,
    pub summary: String,
}

/// The shared history of a project.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HistoryList {
    /// Edits that [`undo`] takes back, oldest first: the last one goes first.
    pub undo: Vec<HistoryEntry>,
    /// Edits that [`redo`] brings back, the next one last.
    pub redo: Vec<HistoryEntry>,
}

fn entries(files: Vec<PathBuf>, project: &Path) -> Vec<HistoryEntry> {
    let prefix = format!("{}.", stem(project));
    files
        .into_iter()
        .map(|file| {
            let meta = read_meta(&file);
            let stamp = file.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix(&prefix)).and_then(|n| n.strip_suffix(".json")).unwrap_or_default().to_string();
            HistoryEntry { time: parse_stamp(&stamp), actor: meta.actor, summary: meta.summary, file }
        })
        .collect()
}

/// The history of a project as it is on disk now.
pub fn history_list(project: &Path) -> HistoryList {
    HistoryList { undo: entries(history(project), project), redo: entries(redo_stack(project), project) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = r#"{
      "format": "edits.project/1",
      "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 123 },
      "slots": { "a": {}, "b": {} },
      "layers": [
        { "id": "a", "kind": "image", "slot": "a", "in": 0, "out": 4 },
        { "id": "b", "kind": "image", "slot": "b", "in": 4 },
        { "id": "finish", "kind": "adjustment", "effects": [
          { "kind": "exposure", "params": { "stops": 0 } },
          { "kind": "vignette" } ] }
      ]
    }"#;

    fn doc() -> Value {
        serde_json::from_str(PROJECT).unwrap()
    }

    fn op(v: Value) -> EditOp {
        serde_json::from_value(v).expect("op parses")
    }

    #[test]
    fn a_default_value_is_not_an_ignored_key() {
        let mut d = doc();
        d["layers"][0]["blend"] = json!("normal");
        d["layers"][0]["transfrom"] = json!({});
        d["layers"][0]["opacitty"] = json!(0.5);
        d["layers"].as_array_mut().unwrap().push(json!({ "id": "t", "kind": "text", "text": "HI",
            "animators": [ { "selector": { "kind": "range", "units": "characters", "randomize": false }, "opacity": 0 } ] }));
        let project = Project::from_json(&jsonfmt::to_file_text(&d)).unwrap();
        let paths: Vec<String> = ignored_key_findings(&d, &project).into_iter().map(|f| f.path).collect();
        assert_eq!(paths, ["/layers/@a/opacitty"], "only the typo with a value is reported");
    }

    fn ids(d: &Value) -> Vec<&str> {
        d["layers"].as_array().unwrap().iter().map(|l| l["id"].as_str().unwrap()).collect()
    }

    pub(crate) struct Temp(pub PathBuf);
    impl Temp {
        pub fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("edits-core-edit-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("project.json"), PROJECT).unwrap();
            Temp(dir)
        }
        pub fn project(&self) -> PathBuf {
            self.0.join("project.json")
        }
        pub fn text(&self) -> String {
            std::fs::read_to_string(self.project()).unwrap()
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn edit(project: &Path, ops: &[EditOp], dry_run: bool) -> Result<EditOutcome, String> {
        apply(project, ops, &EditOptions { dry_run, ..Default::default() })
    }

    #[test]
    fn op_set() {
        let d = apply_ops(&doc(), &[
            op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 128 })),
            op(json!({ "op": "set", "pointer": "/layers/@a/transform/scale", "value": { "keys": [ { "t": 0, "v": 1.3, "ease": { "name": "decay", "rate": 0.45 } }, { "t": 1, "v": 1 } ] } })),
            op(json!({ "op": "set", "pointer": "/layers/@finish/effects/@exposure/params/stops", "value": 3.5 })),
            op(json!({ "op": "set", "pointer": "/layers/@finish/effects/-", "value": { "kind": "grain" } })),
        ])
        .unwrap();
        assert_eq!(d["settings"]["bpm"], json!(128));
        assert_eq!(d["layers"][0]["transform"]["scale"]["keys"][0]["v"], json!(1.3));
        assert_eq!(d["layers"][2]["effects"][0]["params"]["stops"], json!(3.5));
        assert_eq!(d["layers"][2]["effects"][2]["kind"], json!("grain"));
        let e = apply_ops(&doc(), &[op(json!({ "op": "set", "pointer": "/settings/bpm" }))]).unwrap_err();
        assert!(e.contains("ops[0]") && e.contains("needs \"value\""), "{e}");
    }

    #[test]
    fn constructors_make_the_same_ops_as_json() {
        assert_eq!(EditOp::set("/settings/bpm", json!(128)), op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 128 })));
        assert_eq!(EditOp::remove("/layers/@a/out"), op(json!({ "op": "remove", "pointer": "/layers/@a/out" })));
        assert_eq!(EditOp::remove_layer("a"), op(json!({ "op": "remove_layer", "id": "a" })));
        assert_eq!(EditOp::set_slot("a", None, Some(SlotFit::Contain), None), op(json!({ "op": "set_slot", "name": "a", "fit": "contain" })));
        let layer = json!({ "id": "x", "kind": "adjustment" }).as_object().unwrap().clone();
        assert_eq!(EditOp::insert_layer(layer.clone(), Some("a".into()), None), op(json!({ "op": "insert_layer", "layer": layer, "above": "a" })));
        assert_eq!(layer_pointer("a/b"), "/layers/@a~1b");
    }

    #[test]
    fn op_remove() {
        let d = apply_ops(&doc(), &[op(json!({ "op": "remove", "pointer": "/layers/@finish/effects/@vignette" })), op(json!({ "op": "remove", "pointer": "/layers/@a/out" }))]).unwrap();
        assert_eq!(d["layers"][2]["effects"].as_array().unwrap().len(), 1);
        assert!(d["layers"][0].get("out").is_none());
        let e = apply_ops(&doc(), &[op(json!({ "op": "remove", "pointer": "/layers/@a/nope" }))]).unwrap_err();
        assert!(e.contains("has no key \"nope\""), "{e}");
    }

    #[test]
    fn op_insert_layer_goes_where_it_is_told() {
        let layer = |id: &str| json!({ "id": id, "kind": "solid", "color": [1, 1, 1, 1] });
        let top = apply_ops(&doc(), &[op(json!({ "op": "insert_layer", "layer": layer("x") }))]).unwrap();
        assert_eq!(ids(&top), ["a", "b", "finish", "x"]);
        let above = apply_ops(&doc(), &[op(json!({ "op": "insert_layer", "layer": layer("x"), "above": "a" }))]).unwrap();
        assert_eq!(ids(&above), ["a", "x", "b", "finish"]);
        let below = apply_ops(&doc(), &[op(json!({ "op": "insert_layer", "layer": layer("x"), "below": "finish" }))]).unwrap();
        assert_eq!(ids(&below), ["a", "b", "x", "finish"]);
        let e = apply_ops(&doc(), &[op(json!({ "op": "insert_layer", "layer": layer("a") }))]).unwrap_err();
        assert!(e.contains("already exists"), "{e}");
        let e = apply_ops(&doc(), &[op(json!({ "op": "insert_layer", "layer": layer("x"), "above": "zz" }))]).unwrap_err();
        assert!(e.contains("layer ids, bottom to top: a, b, finish"), "{e}");
        let e = apply_ops(&doc(), &[op(json!({ "op": "insert_layer", "layer": { "kind": "adjustment" } }))]).unwrap_err();
        assert!(e.contains("needs a string \"id\""), "{e}");
    }

    #[test]
    fn op_replace_layer_keeps_the_position() {
        let d = apply_ops(&doc(), &[op(json!({ "op": "replace_layer", "id": "b", "layer": { "kind": "solid", "color": [0, 0, 0, 1] } }))]).unwrap();
        assert_eq!(ids(&d), ["a", "b", "finish"]);
        assert_eq!(d["layers"][1]["kind"], json!("solid"));
        let renamed = apply_ops(&doc(), &[op(json!({ "op": "replace_layer", "id": "b", "layer": { "id": "c", "kind": "image", "slot": "b" } }))]).unwrap();
        assert_eq!(ids(&renamed), ["a", "c", "finish"]);
        let e = apply_ops(&doc(), &[op(json!({ "op": "replace_layer", "id": "b", "layer": { "id": "a", "kind": "image", "slot": "b" } }))]).unwrap_err();
        assert!(e.contains("that id is taken"), "{e}");
    }

    #[test]
    fn op_remove_layer() {
        let d = apply_ops(&doc(), &[op(json!({ "op": "remove_layer", "id": "b" }))]).unwrap();
        assert_eq!(ids(&d), ["a", "finish"]);
        assert!(apply_ops(&doc(), &[op(json!({ "op": "remove_layer", "id": "nope" }))]).is_err());
    }

    #[test]
    fn op_set_slot_creates_and_merges() {
        let d = apply_ops(&doc(), &[
            op(json!({ "op": "set_slot", "name": "a", "path": "img/a.png" })),
            op(json!({ "op": "set_slot", "name": "a", "fit": "contain", "focus": [0.5, 0.3] })),
            op(json!({ "op": "set_slot", "name": "new", "path": "img/n.png" })),
        ])
        .unwrap();
        assert_eq!(d["slots"]["a"], json!({ "path": "img/a.png", "fit": "contain", "focus": [0.5, 0.3] }));
        assert_eq!(d["slots"]["new"]["path"], json!("img/n.png"));
        assert!(apply_ops(&doc(), &[op(json!({ "op": "set_slot", "name": "a" }))]).is_err());
    }

    #[test]
    fn ops_are_parsed_strictly() {
        assert!(serde_json::from_value::<EditOp>(json!({ "op": "set", "pointer": "/x", "value": 1, "valeu": 2 })).is_err());
        assert!(serde_json::from_value::<EditOp>(json!({ "op": "sett", "pointer": "/x" })).is_err());
        assert!(serde_json::from_value::<EditOp>(json!({ "pointer": "/x" })).unwrap_err().to_string().contains("missing field `op`"));
    }

    #[test]
    fn diff_names_layers_by_id() {
        let old = doc();
        let new = apply_ops(&old, &[
            op(json!({ "op": "insert_layer", "layer": { "id": "x", "kind": "adjustment" }, "below": "a" })),
            op(json!({ "op": "set", "pointer": "/layers/@finish/effects/0/params/stops", "value": 4 })),
            op(json!({ "op": "remove_layer", "id": "b" })),
            op(json!({ "op": "set", "pointer": "/settings/fps", "value": 30 })),
        ])
        .unwrap();
        let lines: Vec<String> = diff(&old, &new).iter().map(change_line).collect();
        assert_eq!(lines, [
            "- /layers/@b: { \"id\": \"b\", \"kind\": \"image\", \"slot\": \"b\", \"in\": 4 }",
            "~ /layers/@finish/effects/0/params/stops: 0 → 4",
            "+ /layers/@x: { \"id\": \"x\", \"kind\": \"adjustment\" }",
            "~ /settings/fps: 25 → 30",
        ]);
        assert!(diff(&old, &old).is_empty());
        assert_eq!(summarize(&diff(&old, &new)), "- /layers/@b: { \"id\": \"b\", \"kind\": \"image\", \"slot\": \"b\", \"in\": 4 } (and 3 more changes)");
        // an appended effect is one added element; a removed first effect shifts the rest
        let appended = apply_ops(&old, &[op(json!({ "op": "set", "pointer": "/layers/@finish/effects/-", "value": { "kind": "grain" } }))]).unwrap();
        assert_eq!(diff(&old, &appended).iter().map(change_line).collect::<Vec<_>>(), ["+ /layers/@finish/effects/2: { \"kind\": \"grain\" }"]);
        let shifted = apply_ops(&old, &[op(json!({ "op": "remove", "pointer": "/layers/@finish/effects/0" }))]).unwrap();
        assert_eq!(diff(&old, &shifted).iter().map(change_line).collect::<Vec<_>>(), ["~ /layers/@finish/effects: 2 items → 1 items"]);
        // moving a layer is reported as a reorder, not as a rewrite of every layer
        let mut moved = old.clone();
        moved["layers"].as_array_mut().unwrap().swap(0, 1);
        let lines: Vec<String> = diff(&old, &moved).iter().map(change_line).collect();
        assert_eq!(lines, ["~ /layers reordered: a, b, finish → b, a, finish"]);
    }

    #[test]
    fn a_valid_edit_is_written_with_a_backup() {
        let t = Temp::new("valid");
        let before = t.text();
        let out = edit(&t.project(), &[op(json!({ "op": "set", "pointer": "/layers/@finish/effects/@exposure/params/stops", "value": 3 }))], false).unwrap();
        assert!(out.ok && out.written);
        assert_eq!(out.changes.len(), 1);
        assert_eq!(out.changes[0].pointer, "/layers/@finish/effects/0/params/stops");
        // warnings (empty slots) do not stop an edit
        assert!(out.findings.iter().all(|f| f.severity == Severity::Warning) && !out.findings.is_empty());
        let after: Value = serde_json::from_str(&t.text()).unwrap();
        assert_eq!(after["layers"][2]["effects"][0]["params"]["stops"], json!(3));
        assert!(Project::from_json(&t.text()).is_ok());
        let backup = out.backup.unwrap();
        assert!(backup.starts_with(t.0.join(".edits/history")));
        assert_eq!(std::fs::read_to_string(backup).unwrap(), before);
        assert!(t.0.join(".edits/.gitignore").exists());
    }

    #[test]
    fn an_invalid_edit_leaves_the_file_untouched() {
        let t = Temp::new("invalid");
        let before = t.text();
        // The first op is fine, the second names an effect that does not exist:
        // the batch is one step, so neither is applied.
        let out = edit(&t.project(), &[op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 140 })), op(json!({ "op": "set", "pointer": "/layers/@finish/effects/-", "value": { "kind": "vinyette" } }))], false).unwrap();
        assert!(!out.ok && !out.written && out.backup.is_none());
        let f = &out.findings[0];
        assert_eq!(f.severity, Severity::Error);
        assert_eq!(f.path, "/layers/@finish/effects/2");
        assert!(f.message.contains("unknown effect \"vinyette\""));
        assert_eq!(t.text(), before, "the file must be byte-for-byte what it was");
        assert!(history(&t.project()).is_empty(), "no backup for an edit that did not happen");

        // A result that is not a project at all (a layer without a kind).
        let out = edit(&t.project(), &[op(json!({ "op": "insert_layer", "layer": { "id": "x" } }))], false).unwrap();
        assert!(!out.ok && out.findings[0].hint.as_deref().unwrap().contains("not a valid project"));
        assert_eq!(t.text(), before);

        // An op that cannot be applied is an error before validation, and changes nothing either.
        let e = edit(&t.project(), &[op(json!({ "op": "remove_layer", "id": "zz" }))], false).unwrap_err();
        assert!(e.contains("ops[0]") && e.contains("Nothing was changed"), "{e}");
        assert_eq!(t.text(), before);
        assert!(edit(&t.project(), &[], false).is_err());
    }

    #[test]
    fn errors_that_were_already_there_are_told_apart() {
        let t = Temp::new("preexisting");
        // The file on disk is already broken: slot "a" points at a picture that is not there.
        let broken = PROJECT.replace(r#""a": {}"#, r#""a": { "path": "img/missing.png" }"#);
        std::fs::write(t.project(), &broken).unwrap();
        let set_bpm = op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 140 }));
        let out = edit(&t.project(), std::slice::from_ref(&set_bpm), false).unwrap();
        assert!(!out.ok && out.preexisting == 1);
        assert!(out.findings[0].hint.as_deref().unwrap().contains("already in the file before this edit"), "{:?}", out.findings[0]);
        assert_eq!(t.text(), broken);
        // The fix in the same batch goes through.
        let out = edit(&t.project(), &[set_bpm, op(json!({ "op": "remove", "pointer": "/slots/a/path" }))], false).unwrap();
        assert!(out.ok && out.written);
    }

    #[test]
    fn dry_run_reports_and_writes_nothing() {
        let t = Temp::new("dry");
        let before = t.text();
        let out = edit(&t.project(), &[op(json!({ "op": "remove_layer", "id": "b" }))], true).unwrap();
        assert!(out.ok && !out.written && out.dry_run);
        assert_eq!(out.changes[0].change, "removed");
        // removing the layer leaves slot b unused: the warning is part of the preview
        assert!(out.findings.iter().any(|f| f.message.contains("not used")));
        assert_eq!(t.text(), before);
        assert!(history(&t.project()).is_empty());
        assert!(!t.0.join(".edits").exists(), "a dry run leaves no trace");
    }

    #[test]
    fn undo_walks_back_through_the_edits() {
        let t = Temp::new("undo");
        let v0 = t.text();
        let set = |n: u32| [op(json!({ "op": "set", "pointer": "/settings/bpm", "value": n }))];
        edit(&t.project(), &set(130), false).unwrap();
        let v1 = t.text();
        edit(&t.project(), &set(140), false).unwrap();
        assert_eq!(history(&t.project()).len(), 2);
        // an edit that changes nothing makes no backup
        assert!(!edit(&t.project(), &set(140), false).unwrap().written);
        assert_eq!(history(&t.project()).len(), 2);

        let u = undo(&t.project()).unwrap();
        assert_eq!(t.text(), v1);
        assert_eq!(u.remaining, 1);
        assert_eq!(u.changes.iter().map(change_line).collect::<Vec<_>>(), ["~ /settings/bpm: 140 → 130"]);
        assert!(std::fs::read_to_string(&u.kept).unwrap().contains("140"), "the undone version is kept");
        assert!(u.kept.starts_with(t.0.join(".edits/undone")));
        undo(&t.project()).unwrap();
        assert_eq!(t.text(), v0, "back to the original bytes");
        let e = undo(&t.project()).unwrap_err();
        assert!(e.contains("nothing to undo"), "{e}");
    }

    #[test]
    fn redo_brings_back_what_undo_took_until_the_next_edit() {
        let t = Temp::new("redo");
        let set = |n: u32| [op(json!({ "op": "set", "pointer": "/settings/bpm", "value": n }))];
        assert!(redo(&t.project()).unwrap_err().contains("nothing to redo"));
        edit(&t.project(), &set(130), false).unwrap();
        edit(&t.project(), &set(140), false).unwrap();
        let v2 = t.text();
        undo(&t.project()).unwrap();
        let v1 = t.text();
        undo(&t.project()).unwrap();
        assert_eq!(redo_stack(&t.project()).len(), 2);

        let r = redo(&t.project()).unwrap();
        assert_eq!(t.text(), v1);
        assert_eq!(r.changes.iter().map(change_line).collect::<Vec<_>>(), ["~ /settings/bpm: 123 → 130"]);
        assert_eq!(r.remaining, 1);
        redo(&t.project()).unwrap();
        assert_eq!(t.text(), v2, "redo restores the exact bytes");
        assert!(redo(&t.project()).is_err());
        // a redone edit can be undone again
        undo(&t.project()).unwrap();
        assert_eq!(t.text(), v1);
        // a new edit clears what could be redone; the undone version stays in .edits/undone
        edit(&t.project(), &set(150), false).unwrap();
        assert!(redo_stack(&t.project()).is_empty());
        assert!(redo(&t.project()).unwrap_err().contains("nothing to redo"));
        assert!(!stamped(&t.project(), "undone").is_empty());
    }

    #[test]
    fn the_history_says_who_did_what() {
        let t = Temp::new("actors");
        let agent = EditOptions { actor: AGENT, ..Default::default() };
        let window = EditOptions { actor: WINDOW, label: Some("scale of a"), ..Default::default() };
        apply(&t.project(), &[op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 130 }))], &agent).unwrap();
        apply(&t.project(), &[op(json!({ "op": "set", "pointer": "/layers/@a/transform/scale", "value": 1.5 }))], &window).unwrap();
        let h = history_list(&t.project());
        assert_eq!(h.undo.len(), 2);
        assert!(h.redo.is_empty());
        assert_eq!((h.undo[0].actor.as_str(), h.undo[0].summary.as_str()), ("agent", "~ /settings/bpm: 123 → 130"));
        assert_eq!((h.undo[1].actor.as_str(), h.undo[1].summary.as_str()), ("window", "scale of a"));
        let now = SystemTime::now();
        for e in &h.undo {
            let age = now.duration_since(e.time.expect("the stamp parses")).unwrap_or_default();
            assert!(age < Duration::from_secs(60), "{age:?}");
        }
        // Undo takes back the newest entry, whoever made it, and it moves to the redo side.
        let u = undo(&t.project()).unwrap();
        assert_eq!(u.entry, EntryMeta { actor: "window".into(), summary: "scale of a".into() });
        let h = history_list(&t.project());
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.redo.len(), 1);
        assert_eq!(h.redo[0].actor, "window");
        let u = undo(&t.project()).unwrap();
        assert_eq!(u.entry.actor, "agent");
        // A backup made by another program (reference_draft) records who made it.
        backup(&t.project(), AGENT, "reference_draft").unwrap();
        let h = history_list(&t.project());
        assert_eq!(h.undo.last().map(|e| e.summary.as_str()), Some("reference_draft"));
        assert!(h.redo.is_empty(), "a new version clears the redo side");
        // Meta files are not versions.
        assert!(history(&t.project()).iter().all(|p| !p.to_string_lossy().ends_with(".meta.json")));
    }

    #[test]
    fn an_edit_prepared_against_an_old_version_is_refused() {
        let t = Temp::new("stale");
        let seen = text_hash(&t.text());
        // Someone else writes the file in between.
        apply(&t.project(), &[op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 130 }))], &EditOptions::default()).unwrap();
        let after = t.text();
        let window = EditOptions { actor: WINDOW, expect: Some(seen), ..Default::default() };
        let e = apply(&t.project(), &[op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 90 }))], &window).unwrap_err();
        assert_eq!(e, STALE);
        assert_eq!(t.text(), after);
        let fresh = EditOptions { expect: Some(text_hash(&after)), ..window };
        assert!(apply(&t.project(), &[op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 90 }))], &fresh).unwrap().written);
    }

    #[test]
    fn two_writers_at_once_lose_no_step() {
        let t = Temp::new("race");
        let project = t.project();
        let threads: Vec<_> = (0..4)
            .map(|i| {
                let project = project.clone();
                std::thread::spawn(move || {
                    for j in 0..5 {
                        let v = json!([i as f64, j as f64, 0]);
                        apply(&project, &[EditOp::set("/layers/@a/transform/position", v)], &EditOptions::default()).unwrap();
                    }
                })
            })
            .collect();
        for th in threads {
            th.join().unwrap();
        }
        // Every write made exactly one backup; walking them all back reaches the original.
        assert_eq!(history(&project).len(), 20);
        for _ in 0..20 {
            undo(&project).unwrap();
        }
        assert_eq!(t.text(), PROJECT);
    }

    #[test]
    fn extra_checks_block_an_edit() {
        let t = Temp::new("extra");
        let no_paths = |doc: &Value, _: &Path| -> Vec<Finding> {
            file_refs(doc).into_iter().map(|(p, path)| finding(Severity::Error, p, format!("\"{path}\" is not allowed"), None)).collect()
        };
        let opts = EditOptions { extra: Some(&no_paths), ..Default::default() };
        let out = apply(&t.project(), &[EditOp::set_slot("a", Some("x.png".into()), None, None)], &opts).unwrap();
        assert!(!out.ok);
        assert!(out.findings.iter().any(|f| f.path == "/slots/a/path" && f.message.contains("not allowed")));
        let clip = serde_json::json!([{ "id": "hit", "path": "../hit.wav", "at": "end-1s" }]);
        let out = apply(&t.project(), &[EditOp::set("/audio_clips", clip)], &opts).unwrap();
        assert!(!out.ok && out.findings.iter().any(|f| f.path == "/audio_clips/@hit/path" && f.message.contains("not allowed")), "{:?}", out.findings);
    }

    #[test]
    fn stamps_sort_and_parse() {
        assert_eq!(utc_stamp(UNIX_EPOCH), "19700101T000000.000Z");
        let t = UNIX_EPOCH + Duration::from_millis(1_794_495_012_123);
        assert_eq!(utc_stamp(t), "20261112T145012.123Z");
        assert_eq!(parse_stamp("20261112T145012.123Z"), Some(t));
        assert_eq!(parse_stamp("20261112T145012.123Z-3"), Some(t));
        assert_eq!(parse_stamp("19700101T000000.000Z"), Some(UNIX_EPOCH));
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
        assert_eq!(parse_stamp(&utc_stamp(leap)), Some(leap));
        assert!(is_stamp("20261112T143012.123Z") && is_stamp("20261112T143012.123Z-2"));
        assert!(!is_stamp("backup") && !is_stamp("20261112T143012.123Z-"));
        assert_eq!(parse_stamp("backup"), None);
    }
}
