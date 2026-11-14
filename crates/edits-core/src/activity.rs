//! What the agent is doing, for the people watching: the activity log.
//!
//! The MCP server (`edits-mcp`) appends one JSON line to
//! `<project dir>/.edits/activity.jsonl` when a tool call starts and one when
//! it ends: the tool, what it targets (layer ids, JSON pointers, a time or a
//! frame) and a short label a person reads ("applying flash at b12 on layer
//! d"). Edits Studio follows the file and shows where the agent is working:
//! the layer row, the inspector field, a marker on the timeline, the frame it
//! looks at. Format `edits.activity/1`, described in `docs/AGENTS.md`.
//!
//! The same [`target`] function reads the arguments of a tool call that the
//! studio sees in its own agent panel (the ACP stream), so both sources point
//! at the same places.

use crate::pointer;
use crate::time::TimeCtx;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "edits.activity/1";
/// The log's file name, in `<project dir>/.edits/`.
pub const FILE: &str = "activity.jsonl";
/// The log is moved to `activity.1.jsonl` (replacing an older one) when it
/// grows past this, so it never grows without bound.
pub const MAX_BYTES: u64 = 1 << 20;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Start,
    End,
}

/// One line of the log.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// When, as `edits_core::edit::utc_stamp` (UTC, `20261113T120000.000Z`).
    pub at: String,
    pub phase: Phase,
    /// Ties the end of a call to its start: unique within one log.
    pub call: String,
    /// The tool's name as the server serves it (`project_edit`).
    pub tool: String,
    /// Who: `agent` for the MCP server.
    #[serde(default = "agent")]
    pub actor: String,
    /// What a person reads: "applying flash at b12 on layer d".
    pub label: String,
    /// File name of the project the call is about, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Ids of the layers the call reads or changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<String>,
    /// JSON pointers the call changes or reads (`/layers/@d/transform/scale`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pointers: Vec<String>,
    /// The time argument as given (`b12`, `drop+2b`, `4.5`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// That time in project time (beats or seconds), when it could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<f64>,
    /// The frame the call renders or looks at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u64>,
    /// End only: whether the call succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    /// End only: the first line of the error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// End only: how long the call took, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// End only: the id of the background job the call started (`render_video`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    /// The MCP client's name, as it introduced itself (`claude-code`, `codex-mcp-client`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// Which client the server serves, from `EDITS_ACTIVITY_VIA` (the studio
    /// sets `studio` for the agent of its own panel, which it already follows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

fn agent() -> String {
    "agent".into()
}

/// Where a tool call points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    pub layers: Vec<String>,
    pub pointers: Vec<String>,
    pub time: Option<String>,
    pub t: Option<f64>,
    pub frame: Option<u64>,
    pub label: String,
}

/// The log of the project at `project`.
pub fn path(project: &Path) -> PathBuf {
    crate::edit::project_dir(project).join(".edits").join(FILE)
}

/// The log in a folder (for calls that name no project: the workspace root).
pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(".edits").join(FILE)
}

/// Append one entry. Creates `.edits/` (with its `.gitignore`) when needed and
/// moves a log larger than [`MAX_BYTES`] to `activity.1.jsonl` first.
pub fn append(log: &Path, entry: &Entry) -> std::io::Result<()> {
    if let Some(edits) = log.parent() {
        if let Some(dir) = edits.parent() {
            crate::edit::state_dir(dir, "")?;
        }
    }
    if std::fs::metadata(log).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(log, log.with_file_name("activity.1.jsonl"));
    }
    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');
    // One write of a whole line in append mode: lines of two servers do not interleave.
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    f.write_all(line.as_bytes())
}

/// The entries of a piece of the log. Lines that are not entries (a partial
/// last line, a line of a newer format) are skipped.
pub fn parse(text: &str) -> Vec<Entry> {
    text.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str::<Entry>(l).ok()).collect()
}

/// The name of an edits tool in the forms agents show it: `project_edit`,
/// `mcp__edits__project_edit` (Claude Code), `edits.project_edit`,
/// `edits/project_edit` (Codex). `None` for another server's tool.
pub fn tool_name(name: &str) -> Option<&str> {
    let name = name.trim();
    for prefix in ["mcp__edits__", "edits.", "edits/", "edits__", "edits:"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    TOOLS.contains(&name).then_some(name)
}

/// Every tool `edits-mcp` serves.
pub const TOOLS: [&str; 25] = [
    "project_describe",
    "project_read",
    "project_check",
    "project_edit",
    "project_undo",
    "effects_list",
    "render_frame",
    "render_sheet",
    "render_video",
    "job_status",
    "job_cancel",
    "reference_probe",
    "reference_analyze",
    "reference_read",
    "reference_draft",
    "reference_compare",
    "project_fit",
    "audio_analyze",
    "presets_list",
    "preset_apply",
    "beats_cut",
    "beats_accent",
    "effect_define",
    "ui_context",
    "doctor",
];

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn time_arg(args: &Value, key: &str) -> Option<String> {
    match args.get(key)? {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

/// The layer a pointer names: `@d` as `d`, an index as `#3` (see [`resolve`]).
fn layer_of(ptr: &str) -> Option<String> {
    let t = pointer::tokens(ptr);
    if t.first().map(String::as_str) != Some("layers") {
        return None;
    }
    let tok = t.get(1)?;
    if let Some(id) = tok.strip_prefix('@') {
        Some(id.to_string())
    } else if tok.parse::<usize>().is_ok() {
        Some(format!("#{tok}"))
    } else {
        None
    }
}

/// `/layers/@d/transform/scale` as `d transform.scale`.
pub fn short_pointer(ptr: &str) -> String {
    let t = pointer::tokens(ptr);
    match t.first().map(String::as_str) {
        Some("layers") if t.len() >= 2 => {
            let id = t[1].trim_start_matches('@');
            if t.len() == 2 {
                format!("layer {id}")
            } else {
                let rest: Vec<&str> = t[2..].iter().map(|s| s.trim_start_matches('@')).filter(|s| !matches!(*s, "params" | "effects")).collect();
                format!("{id} {}", rest.join("."))
            }
        }
        _ => t.join("."),
    }
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

/// What a call of `tool` with `args` points at, and a label for it. The tool
/// name may be in any form [`tool_name`] reads.
pub fn target(tool: &str, args: &Value) -> Target {
    let tool = tool_name(tool).unwrap_or(tool);
    let mut t = Target::default();
    let layer = str_arg(args, "layer").map(str::to_string);
    if let Some(l) = &layer {
        if l != "auto" {
            t.layers.push(l.clone());
        }
    }
    let on_layer = |t: &Target| t.layers.first().map(|l| format!(" on layer {l}")).unwrap_or_default();
    match tool {
        "project_edit" => {
            let ops = args.get("ops").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut what = Vec::new();
            for op in &ops {
                let kind = op.get("op").and_then(Value::as_str).unwrap_or("");
                match kind {
                    "set" | "remove" => {
                        if let Some(p) = str_arg(op, "pointer") {
                            push_unique(&mut t.pointers, p.to_string());
                            if let Some(l) = layer_of(p) {
                                push_unique(&mut t.layers, l);
                            }
                            let short = short_pointer(p);
                            let toks = pointer::tokens(p);
                            let new_effect = kind == "set" && toks.len() == 4 && toks[0] == "layers" && toks[2] == "effects" && (toks[3] == "-" || toks[3].parse::<usize>().is_ok());
                            let effect_kind = op.get("value").and_then(|v| v.get("kind")).and_then(Value::as_str);
                            what.push(match (new_effect, effect_kind) {
                                (true, Some(k)) => format!("adding {k} to {}", toks[1].trim_start_matches('@')),
                                _ if kind == "set" => format!("setting {short}"),
                                _ => format!("removing {short}"),
                            });
                        }
                    }
                    "insert_layer" | "replace_layer" => {
                        let id = op.get("layer").and_then(|l| l.get("id")).and_then(Value::as_str).or_else(|| str_arg(op, "id"));
                        if let Some(id) = id {
                            push_unique(&mut t.pointers, format!("/layers/@{id}"));
                            push_unique(&mut t.layers, id.to_string());
                            what.push(format!("{} layer {id}", if kind == "insert_layer" { "adding" } else { "replacing" }));
                        }
                    }
                    "remove_layer" => {
                        if let Some(id) = str_arg(op, "id") {
                            push_unique(&mut t.layers, id.to_string());
                            what.push(format!("removing layer {id}"));
                        }
                    }
                    "set_slot" => {
                        if let Some(name) = str_arg(op, "name") {
                            push_unique(&mut t.pointers, format!("/slots/{}", pointer::escape(name)));
                            what.push(format!("setting slot {name}"));
                        }
                    }
                    _ => {}
                }
            }
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
            t.label = match what.len() {
                0 => "editing the project".into(),
                1 => what.remove(0),
                n => format!("{} (+{} more)", what.remove(0), n - 1),
            };
            if dry {
                t.label = format!("trying {}", t.label);
            }
        }
        "project_read" => {
            if let Some(p) = str_arg(args, "pointer") {
                t.pointers.push(p.to_string());
                if let Some(l) = layer_of(p) {
                    push_unique(&mut t.layers, l);
                }
                t.label = format!("reading {}", short_pointer(p));
            } else {
                t.label = "reading the project".into();
            }
        }
        "project_describe" => t.label = "reading the project".into(),
        "project_check" => t.label = "checking the project".into(),
        "project_undo" => t.label = "undoing its last edit".into(),
        "effects_list" => {
            t.label = match str_arg(args, "kind") {
                Some(k) => format!("looking up effect {k}"),
                None => "looking through the effects".into(),
            }
        }
        "presets_list" => t.label = "looking through the presets".into(),
        "render_frame" => {
            t.frame = args.get("frame").and_then(Value::as_u64);
            t.time = time_arg(args, "at");
            t.label = match (t.frame, &t.time) {
                (Some(f), _) => format!("looking at frame {f}"),
                (None, Some(at)) => format!("looking at {at}"),
                _ => "looking at the first frame".into(),
            };
        }
        "render_sheet" => {
            t.time = time_arg(args, "from");
            t.label = "rendering a contact sheet".into();
        }
        "render_video" => t.label = "rendering the video".into(),
        "job_status" => t.label = "checking on a job".into(),
        "job_cancel" => t.label = "cancelling a job".into(),
        "preset_apply" => {
            t.time = time_arg(args, "at");
            let preset = str_arg(args, "preset").unwrap_or("a preset");
            let at = t.time.as_ref().map(|a| format!(" at {a}")).unwrap_or_default();
            let on = if layer.as_deref() == Some("auto") { " on the picture on screen".to_string() } else { on_layer(&t) };
            t.label = format!("applying {preset}{at}{on}");
        }
        "beats_cut" => {
            t.time = time_arg(args, "from");
            let n = args.get("slots").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
            t.label = format!("cutting {n} picture{} on the beat", if n == 1 { "" } else { "s" });
        }
        "beats_accent" => {
            t.time = time_arg(args, "from");
            let preset = str_arg(args, "preset").unwrap_or("a preset");
            let on = str_arg(args, "on").unwrap_or("the beat");
            t.label = format!("accenting {on} with {preset}{}", on_layer(&t));
        }
        "effect_define" => {
            let name = str_arg(args, "name").unwrap_or("an effect");
            t.label = format!("writing the shader {name}{}", on_layer(&t));
        }
        "audio_analyze" => t.label = "listening to the song".into(),
        "reference_probe" => t.label = "probing the reference video".into(),
        "reference_analyze" => t.label = "analysing the reference".into(),
        "reference_read" => t.label = "reading the reference analysis".into(),
        "reference_draft" => t.label = "drafting a project from the reference".into(),
        "reference_compare" => t.label = "comparing the render with the reference".into(),
        "project_fit" => t.label = "fitting the project to the reference".into(),
        "ui_context" => t.label = "checking what you have selected".into(),
        "doctor" => t.label = "checking the renderer".into(),
        other => t.label = format!("running {other}"),
    }
    t
}

/// Fill in what needs the project: layer indices as ids, and the time as
/// project time.
pub fn resolve(t: &mut Target, doc: &Value) {
    let layers = doc.get("layers").and_then(Value::as_array);
    for l in t.layers.iter_mut() {
        if let Some(i) = l.strip_prefix('#').and_then(|i| i.parse::<usize>().ok()) {
            if let Some(id) = layers.and_then(|ls| ls.get(i)).and_then(|l| l.get("id")).and_then(Value::as_str) {
                *l = id.to_string();
            }
        }
    }
    let tc = TimeCtx::from_doc(doc);
    if t.t.is_none() {
        if let Some(text) = &t.time {
            t.t = tc.resolve(text).ok();
        } else if let Some(f) = t.frame {
            t.t = Some(tc.seconds_to_units(f as f64 / tc.fps));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_names_in_every_form() {
        assert_eq!(tool_name("mcp__edits__project_edit"), Some("project_edit"));
        assert_eq!(tool_name("edits.render_frame"), Some("render_frame"));
        assert_eq!(tool_name("edits/preset_apply"), Some("preset_apply"));
        assert_eq!(tool_name("render_sheet"), Some("render_sheet"));
        assert_eq!(tool_name("Bash"), None);
        assert_eq!(tool_name("mcp__other__thing"), None);
    }

    #[test]
    fn an_edit_points_at_its_layers_and_pointers() {
        let t = target(
            "mcp__edits__project_edit",
            &json!({ "project": "p.json", "ops": [
                { "op": "set", "pointer": "/layers/@d/transform/scale", "value": 1.2 },
                { "op": "set", "pointer": "/layers/2/effects/@blur/params/radius", "value": 3 },
                { "op": "insert_layer", "layer": { "id": "flash", "kind": "solid" }, "above": "d" }
            ]}),
        );
        assert_eq!(t.layers, ["d", "#2", "flash"]);
        assert_eq!(t.pointers, ["/layers/@d/transform/scale", "/layers/2/effects/@blur/params/radius", "/layers/@flash"]);
        assert_eq!(t.label, "setting d transform.scale (+2 more)");
        let add = target("project_edit", &json!({ "ops": [{ "op": "set", "pointer": "/layers/@d/effects/-", "value": { "kind": "glow" } }] }));
        assert_eq!(add.label, "adding glow to d");
        let doc = json!({ "settings": { "time_base": "beats", "bpm": 120, "fps": 25, "duration": 16 },
                          "layers": [{ "id": "a" }, { "id": "b" }, { "id": "c" }] });
        let mut t = t;
        resolve(&mut t, &doc);
        assert_eq!(t.layers, ["d", "c", "flash"]);
    }

    #[test]
    fn presets_and_frames_carry_a_time() {
        let doc = json!({ "settings": { "time_base": "beats", "bpm": 120, "fps": 25, "duration": 16 }, "layers": [] });
        let mut t = target("preset_apply", &json!({ "project": "p.json", "preset": "punch", "layer": "d", "at": "b12" }));
        assert_eq!(t.label, "applying punch at b12 on layer d");
        resolve(&mut t, &doc);
        assert_eq!((t.layers.clone(), t.t), (vec!["d".to_string()], Some(12.0)));
        let mut f = target("edits.render_frame", &json!({ "project": "p.json", "frame": 50 }));
        assert_eq!(f.label, "looking at frame 50");
        resolve(&mut f, &doc);
        assert_eq!(f.t, Some(4.0)); // 50 frames at 25 fps = 2 s = 4 beats at 120 BPM
        let auto = target("preset_apply", &json!({ "preset": "flash", "layer": "auto" }));
        assert_eq!((auto.label.as_str(), auto.layers.len()), ("applying flash on the picture on screen", 0));
    }

    #[test]
    fn the_log_round_trips_and_skips_what_it_cannot_read() {
        let dir = std::env::temp_dir().join(format!("edits-activity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.join("project.json");
        let log = path(&project);
        let start = Entry { at: "20261114T100000.000Z".into(), call: "1-0".into(), tool: "render_frame".into(), actor: "agent".into(), label: "looking at frame 60".into(), frame: Some(60), ..Default::default() };
        let end = Entry { phase: Phase::End, ok: Some(true), ms: Some(420), ..start.clone() };
        append(&log, &start).unwrap();
        append(&log, &end).unwrap();
        assert!(dir.join(".edits/.gitignore").exists());
        let mut text = std::fs::read_to_string(&log).unwrap();
        text.push_str("not json\n{\"at\":\"x\",\"phase\":\"sideways\"}\n{\"at\":\"20261114T1");
        let back = parse(&text);
        assert_eq!(back, vec![start, end]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
