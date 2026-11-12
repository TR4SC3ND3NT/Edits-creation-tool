//! `project_edit` and `project_undo`: batches of small edits applied to the
//! project JSON as one step that either passes validation and is written, or
//! leaves the file exactly as it was.
//!
//! Everything here works on plain `serde_json::Value`s and files; nothing
//! talks to MCP, so it is tested directly.

use crate::paths::Workspace;
use crate::{jsonfmt, pointer};
use edits_core::check::{self, Finding, Severity};
use edits_core::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Backups kept per project before the oldest are deleted.
const HISTORY_KEEP: usize = 50;

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Set,
    Remove,
    InsertLayer,
    ReplaceLayer,
    RemoveLayer,
    SetSlot,
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SlotFit {
    Cover,
    Contain,
    Stretch,
}

/// One edit. Which fields are used depends on `op`:
/// `set` pointer + value; `remove` pointer; `insert_layer` layer (+ above | below);
/// `replace_layer` id + layer; `remove_layer` id; `set_slot` name (+ path, fit, focus).
#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug)]
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

// ---- validation ------------------------------------------------------------

fn finding(severity: Severity, path: String, message: String, hint: Option<&str>) -> Finding {
    Finding { severity, path, message, hint: hint.map(str::to_string) }
}

/// Keys the renderer would silently ignore: present in the document, absent
/// from what the parsed project writes back. Usually a typo (`"transfrom"`).
fn ignored_keys(doc: &Value, parsed: &Value, path: &str, out: &mut Vec<Finding>) {
    match (doc, parsed) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                let p = format!("{path}/{}", pointer::escape(k));
                match b.get(k) {
                    Some(w) => ignored_keys(v, w, &p, out),
                    // Empty and null values are written back as "absent"; they are not typos.
                    None if v.is_null() || v.as_array().is_some_and(Vec::is_empty) || v.as_object().is_some_and(Map::is_empty) => {}
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
                    Some(id) if path == "/layers" => format!("{path}/@{}", pointer::escape(id)),
                    _ => format!("{path}/{i}"),
                };
                ignored_keys(v, w, &p, out);
            }
        }
        _ => {}
    }
}

/// Files a project refers to: (pointer, path as written).
fn file_refs(doc: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(p) = doc.pointer("/audio/path").and_then(Value::as_str) {
        out.push(("/audio/path".to_string(), p.to_string()));
    }
    if let Some(slots) = doc.get("slots").and_then(Value::as_object) {
        for (name, slot) in slots {
            if let Some(p) = slot.get("path").and_then(Value::as_str) {
                out.push((format!("/slots/{}/path", pointer::escape(name)), p.to_string()));
            }
        }
    }
    for (li, layer) in doc.get("layers").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let lp = match layer.get("id").and_then(Value::as_str) {
            Some(id) => format!("/layers/@{}", pointer::escape(id)),
            None => format!("/layers/{li}"),
        };
        for (ei, effect) in layer.get("effects").and_then(Value::as_array).into_iter().flatten().enumerate() {
            if let Some(p) = effect.get("source").and_then(Value::as_str) {
                out.push((format!("{lp}/effects/{ei}/source"), p.to_string()));
            }
        }
    }
    out
}

/// Errors for every file reference that leaves the workspace root.
pub fn outside_refs(doc: &Value, project_dir: &Path, ws: &Workspace) -> Vec<Finding> {
    file_refs(doc)
        .into_iter()
        .filter_map(|(ptr, path)| {
            ws.resolve_from(project_dir, &path, "file").err().map(|_| {
                finding(
                    Severity::Error,
                    ptr,
                    format!("\"{path}\" is outside the workspace root {}", ws.root().display()),
                    Some("copy the file into the workspace and use a path relative to the project file, or restart the server with EDITS_ROOT set to a folder that contains it"),
                )
            })
        })
        .collect()
}

/// Validate a document the way the renderer will see it: parse it as a
/// project, run `edits_core::check`, and add what only the raw JSON can show
/// (ignored keys, files outside the workspace). Errors first.
pub fn validate(doc: &Value, project_dir: &Path, ws: Option<&Workspace>) -> Vec<Finding> {
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
    findings.extend(raw_findings(doc, &project, project_dir, ws));
    findings.sort_by_key(|f| f.severity != Severity::Error);
    findings
}

/// What only the raw JSON can show, beyond `edits_core::check`: files outside
/// the workspace (errors) and keys the renderer ignores (warnings).
pub fn raw_findings(doc: &Value, project: &Project, project_dir: &Path, ws: Option<&Workspace>) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Some(ws) = ws {
        findings.extend(outside_refs(doc, project_dir, ws));
    }
    if let Ok(parsed) = serde_json::to_value(project) {
        ignored_keys(doc, &parsed, "", &mut findings);
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
        let _ = std::fs::write(&ignore, "# written by edits-mcp: undo history, render previews and job logs\n*\n");
    }
    Ok(path)
}

fn project_dir(project: &Path) -> PathBuf {
    match project.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn stem(project: &Path) -> String {
    project.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into())
}

/// Backups of this project, oldest first.
pub fn history(project: &Path) -> Vec<PathBuf> {
    let dir = project_dir(project).join(".edits").join("history");
    let prefix = format!("{}.", stem(project));
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix(&prefix))
                .and_then(|n| n.strip_suffix(".json"))
                .is_some_and(is_stamp)
        })
        .collect();
    // Same-millisecond backups carry a serial (`-1`, `-2`); compare it as a number.
    found.sort_by_key(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let core = name.strip_prefix(&prefix).and_then(|n| n.strip_suffix(".json")).unwrap_or_default();
        let (stamp, serial) = core.split_once('-').unwrap_or((core, "0"));
        (stamp.to_string(), serial.parse::<u64>().unwrap_or(0))
    });
    found
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

/// Save the current file under `.edits/history/` next to it. Returns the backup's path.
pub fn backup(project: &Path) -> std::io::Result<PathBuf> {
    let dir = state_dir(&project_dir(project), "history")?;
    let to = unique(&dir, &stem(project));
    std::fs::copy(project, &to)?;
    let all = history(project);
    for old in all.iter().take(all.len().saturating_sub(HISTORY_KEEP)) {
        let _ = std::fs::remove_file(old);
    }
    Ok(to)
}

// ---- the two operations ----------------------------------------------------

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
}

/// Apply `ops` to the project file. See the module documentation for the contract.
pub fn edit_file(project: &Path, ops: &[EditOp], dry_run: bool, ws: Option<&Workspace>) -> Result<EditOutcome, String> {
    if ops.is_empty() {
        return Err("\"ops\" is empty: give at least one edit".into());
    }
    let text = std::fs::read_to_string(project).map_err(|e| format!("cannot read {}: {e}", project.display()))?;
    let old: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON ({e}); fix the file by hand or restore a backup with project_undo", project.display()))?;
    let new = apply_ops(&old, ops).map_err(|e| format!("{e}. Nothing was changed."))?;
    let mut findings = validate(&new, &project_dir(project), ws);
    let changes = diff(&old, &new);
    if check::has_errors(&findings) {
        // An error that was in the file before this edit blocks it all the same
        // (a broken project is never written), so say which ones those are: the
        // fix belongs in the same batch.
        let before: std::collections::BTreeSet<(String, String)> =
            validate(&old, &project_dir(project), ws).into_iter().filter(|f| f.severity == Severity::Error).map(|f| (f.path, f.message)).collect();
        let mut preexisting = 0;
        for f in findings.iter_mut().filter(|f| f.severity == Severity::Error) {
            if before.contains(&(f.path.clone(), f.message.clone())) {
                preexisting += 1;
                let hint = f.hint.take().map(|h| format!("{h}; ")).unwrap_or_default();
                f.hint = Some(format!("{hint}already in the file before this edit: fix it in the same batch"));
            }
        }
        return Ok(EditOutcome { ok: false, written: false, dry_run, changes, findings, backup: None, preexisting });
    }
    if dry_run || changes.is_empty() {
        return Ok(EditOutcome { ok: true, written: false, dry_run, changes, findings, backup: None, preexisting: 0 });
    }
    let saved = backup(project).map_err(|e| format!("cannot write the backup, so nothing was changed: {e}"))?;
    write_atomic(project, &jsonfmt::to_file_text(&new)).map_err(|e| format!("cannot write {}: {e}", project.display()))?;
    Ok(EditOutcome { ok: true, written: true, dry_run, changes, findings, backup: Some(saved), preexisting: 0 })
}

#[derive(Debug)]
pub struct UndoOutcome {
    /// The backup that was restored (it is removed from the history).
    pub restored: PathBuf,
    /// Where the version that was replaced is kept, in case the undo was a mistake.
    pub kept: PathBuf,
    pub changes: Vec<Change>,
    pub remaining: usize,
}

/// Restore the newest backup of the project. Repeated calls walk further back.
pub fn undo_file(project: &Path) -> Result<UndoOutcome, String> {
    let all = history(project);
    let Some(latest) = all.last() else {
        return Err(format!("no backups for {}: nothing to undo (backups are made by project_edit and reference_draft, under .edits/history/ next to the project)", project.display()));
    };
    let restored_text = std::fs::read_to_string(latest).map_err(|e| format!("cannot read the backup {}: {e}", latest.display()))?;
    let current_text = std::fs::read_to_string(project).unwrap_or_default();
    let undone = state_dir(&project_dir(project), "undone").map_err(|e| format!("cannot create the undo folder: {e}"))?;
    let kept = unique(&undone, &stem(project));
    std::fs::write(&kept, &current_text).map_err(|e| format!("cannot save the current version, so nothing was changed: {e}"))?;
    write_atomic(project, &restored_text).map_err(|e| format!("cannot write {}: {e}", project.display()))?;
    let _ = std::fs::remove_file(latest);
    let changes = match (serde_json::from_str::<Value>(&current_text), serde_json::from_str::<Value>(&restored_text)) {
        (Ok(a), Ok(b)) => diff(&a, &b),
        _ => Vec::new(),
    };
    Ok(UndoOutcome { restored: latest.clone(), kept, changes, remaining: all.len() - 1 })
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

    fn ids(d: &Value) -> Vec<&str> {
        d["layers"].as_array().unwrap().iter().map(|l| l["id"].as_str().unwrap()).collect()
    }

    struct Temp(PathBuf);
    impl Temp {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("edits-mcp-edit-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("project.json"), PROJECT).unwrap();
            Temp(dir)
        }
        fn project(&self) -> PathBuf {
            self.0.join("project.json")
        }
        fn text(&self) -> String {
            std::fs::read_to_string(self.project()).unwrap()
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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
        let out = edit_file(&t.project(), &[op(json!({ "op": "set", "pointer": "/layers/@finish/effects/@exposure/params/stops", "value": 3 }))], false, None).unwrap();
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
        let out = edit_file(
            &t.project(),
            &[op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 140 })), op(json!({ "op": "set", "pointer": "/layers/@finish/effects/-", "value": { "kind": "vinyette" } }))],
            false,
            None,
        )
        .unwrap();
        assert!(!out.ok && !out.written && out.backup.is_none());
        let f = &out.findings[0];
        assert_eq!(f.severity, Severity::Error);
        assert_eq!(f.path, "/layers/@finish/effects/2");
        assert!(f.message.contains("unknown effect \"vinyette\""));
        assert_eq!(t.text(), before, "the file must be byte-for-byte what it was");
        assert!(history(&t.project()).is_empty(), "no backup for an edit that did not happen");

        // A result that is not a project at all (a layer without a kind).
        let out = edit_file(&t.project(), &[op(json!({ "op": "insert_layer", "layer": { "id": "x" } }))], false, None).unwrap();
        assert!(!out.ok && out.findings[0].hint.as_deref().unwrap().contains("not a valid project"));
        assert_eq!(t.text(), before);

        // An op that cannot be applied is an error before validation, and changes nothing either.
        let e = edit_file(&t.project(), &[op(json!({ "op": "remove_layer", "id": "zz" }))], false, None).unwrap_err();
        assert!(e.contains("ops[0]") && e.contains("Nothing was changed"), "{e}");
        assert_eq!(t.text(), before);
        assert!(edit_file(&t.project(), &[], false, None).is_err());
    }

    #[test]
    fn errors_that_were_already_there_are_told_apart() {
        let t = Temp::new("preexisting");
        // The file on disk is already broken: slot "a" points at a picture that is not there.
        let broken = PROJECT.replace(r#""a": {}"#, r#""a": { "path": "img/missing.png" }"#);
        std::fs::write(t.project(), &broken).unwrap();
        let set_bpm = op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 140 }));
        let out = edit_file(&t.project(), std::slice::from_ref(&set_bpm), false, None).unwrap();
        assert!(!out.ok && out.preexisting == 1);
        assert!(out.findings[0].hint.as_deref().unwrap().contains("already in the file before this edit"), "{:?}", out.findings[0]);
        assert_eq!(t.text(), broken);
        // The fix in the same batch goes through.
        let out = edit_file(&t.project(), &[set_bpm, op(json!({ "op": "remove", "pointer": "/slots/a/path" }))], false, None).unwrap();
        assert!(out.ok && out.written);
    }

    #[test]
    fn dry_run_reports_and_writes_nothing() {
        let t = Temp::new("dry");
        let before = t.text();
        let out = edit_file(&t.project(), &[op(json!({ "op": "remove_layer", "id": "b" }))], true, None).unwrap();
        assert!(out.ok && !out.written && out.dry_run);
        assert_eq!(out.changes[0].change, "removed");
        // removing the layer leaves slot b unused: the warning is part of the preview
        assert!(out.findings.iter().any(|f| f.message.contains("not used")));
        assert_eq!(t.text(), before);
        assert!(history(&t.project()).is_empty());
    }

    #[test]
    fn undo_walks_back_through_the_edits() {
        let t = Temp::new("undo");
        let v0 = t.text();
        let set = |n: u32| [op(json!({ "op": "set", "pointer": "/settings/bpm", "value": n }))];
        edit_file(&t.project(), &set(130), false, None).unwrap();
        let v1 = t.text();
        edit_file(&t.project(), &set(140), false, None).unwrap();
        assert_eq!(history(&t.project()).len(), 2);
        // an edit that changes nothing makes no backup
        assert!(!edit_file(&t.project(), &set(140), false, None).unwrap().written);
        assert_eq!(history(&t.project()).len(), 2);

        let u = undo_file(&t.project()).unwrap();
        assert_eq!(t.text(), v1);
        assert_eq!(u.remaining, 1);
        assert_eq!(u.changes.iter().map(change_line).collect::<Vec<_>>(), ["~ /settings/bpm: 140 → 130"]);
        assert!(std::fs::read_to_string(&u.kept).unwrap().contains("140"), "the undone version is kept");
        undo_file(&t.project()).unwrap();
        assert_eq!(t.text(), v0, "back to the original bytes");
        let e = undo_file(&t.project()).unwrap_err();
        assert!(e.contains("nothing to undo"), "{e}");
    }

    #[test]
    fn typos_and_outside_files_are_found() {
        let t = Temp::new("lint");
        let ws = Workspace::new(&t.0).unwrap();
        let out = edit_file(&t.project(), &[op(json!({ "op": "set", "pointer": "/layers/@a/transfrom", "value": { "scale": 2 } }))], true, Some(&ws)).unwrap();
        let w = out.findings.iter().find(|f| f.path == "/layers/@a/transfrom").expect("the typo is reported");
        assert_eq!(w.severity, Severity::Warning);
        assert!(w.hint.as_deref().unwrap().contains("transform"));

        let out = edit_file(&t.project(), &[op(json!({ "op": "set_slot", "name": "a", "path": "../../../etc/hostname" }))], false, Some(&ws)).unwrap();
        assert!(!out.ok);
        assert!(out.findings.iter().any(|f| f.path == "/slots/a/path" && f.message.contains("outside the workspace root")));
    }

    #[test]
    fn stamps_sort_and_parse() {
        assert_eq!(utc_stamp(UNIX_EPOCH), "19700101T000000.000Z");
        assert_eq!(utc_stamp(UNIX_EPOCH + std::time::Duration::from_millis(1_794_495_012_123)), "20261112T145012.123Z");
        assert!(is_stamp("20261112T143012.123Z") && is_stamp("20261112T143012.123Z-2"));
        assert!(!is_stamp("backup") && !is_stamp("20261112T143012.123Z-"));
    }
}
