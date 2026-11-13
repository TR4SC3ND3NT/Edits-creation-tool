//! The MCP tools. Each one resolves its paths inside the workspace, runs an
//! executable or edits a file, and returns a short text (plus an image or
//! structured data). No state is kept between calls: the project path, the
//! analysis path and the job id are arguments.

mod toolkit;

pub use toolkit::{AudioAnalyzeArgs, BeatsAccentArgs, BeatsCutArgs, EffectDefineArgs, PresetApplyArgs, PresetsArgs};

use crate::edit::{self, EditOp};
use crate::exec::{self, Tool};
use crate::images;
use crate::jobs::{self, State};
use crate::paths::Workspace;
use crate::{jsonfmt, pointer, summarize, Config};
use edits_core::check::Severity;
use edits_core::Project;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Longest text a tool returns (about 10,000 tokens). Larger answers are
/// refused with advice on how to ask for a part.
const TEXT_CAP: usize = 40_000;
/// Most stdout kept from one command.
const STDOUT_CAP: usize = 64 * 1024 * 1024;
/// Most frames on one contact sheet.
const SHEET_MAX: u64 = 64;
/// Long edge, in pixels, of a frame rendered without an explicit scale.
const FRAME_EDGE: f64 = 540.0;

pub const INSTRUCTIONS: &str = "edits renders short beat-synced video edits from one JSON project: slots (pictures) + layers (bottom to top) + keyframes + effects; time in beats, space in composition-height units. \
Loop: project_describe, project_edit (atomic, validated; project_undo reverts), render_sheet or render_frame and look at the image, then render_video (a background job: poll job_status). \
To reuse the recipe of a reference edit with other pictures: reference_analyze, reference_read, reference_draft, set the slots, then reference_compare and fix the largest differences. \
To make an edit from a song and pictures: audio_analyze (tempo, bars, kick/snare/hat onsets, the drop), beats_cut (one picture per beat), beats_accent and preset_apply (flashes, punches, shakes, transitions: presets_list), then look at a render_sheet. \
Times can be written \"b12.5\", \"bar 4\", \"drop+2b\". ui_context says what the person has selected in the preview window. \
Every path must be inside the workspace root; relative paths start there. doctor reports the root and the renderer.";

// ---- arguments -------------------------------------------------------------

/// A time: a number in the project's time base, or text such as "b12.5",
/// "bar 4", "4:3", "2.5s", "f60", a marker ("drop") or "drop+2b".
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum TimeArg {
    Num(f64),
    Text(String),
}

impl JsonSchema for TimeArg {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Time".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": ["number", "string"],
            "description": "Project time (beats for a beats project, else seconds), or text: \"b12.5\" (beat), \"bar 4\", \"4:3\" (bar 4 beat 3), \"2.5s\", \"f60\" (frame), a marker such as \"drop\", with offsets like \"drop+2b\"."
        })
    }
}

impl TimeArg {
    /// As a command-line argument.
    fn text(&self, name: &str) -> Result<String, String> {
        match self {
            TimeArg::Num(v) if !v.is_finite() => Err(format!("`{name}` must be a finite number")),
            TimeArg::Num(v) => Ok(v.to_string()),
            TimeArg::Text(t) if t.trim().is_empty() => Err(format!("`{name}` is empty")),
            TimeArg::Text(t) => Ok(t.trim().to_string()),
        }
    }
    /// In project time.
    fn resolve(&self, tc: &edits_core::time::TimeCtx, name: &str) -> Result<f64, String> {
        match self {
            TimeArg::Num(v) if v.is_finite() => Ok(*v),
            other => tc.resolve(&other.text(name)?).map_err(|e| format!("`{name}`: {e}")),
        }
    }
}

fn time_text(t: &Option<TimeArg>, name: &str) -> Result<Option<String>, String> {
    t.as_ref().map(|t| t.text(name)).transpose()
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectArgs {
    /// Project file (.json), absolute or relative to the workspace root.
    pub project: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadArgs {
    /// Project file (.json).
    pub project: String,
    /// JSON pointer to one part, e.g. /settings, /slots/a, /layers/@finish, /layers/@a/effects/0. Omit for the whole document.
    pub pointer: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditArgs {
    /// Project file (.json).
    pub project: String,
    /// Edits, applied in order as one step.
    pub ops: Vec<EditOp>,
    /// Validate and report what would change without writing.
    pub dry_run: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectsArgs {
    /// One effect kind (e.g. "exposure"). Omit for all.
    pub kind: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameArgs {
    /// Project file (.json).
    pub project: String,
    /// When: project time or text such as "b12.5", "bar 4", "drop"; the frame containing it is rendered. Default: frame 0.
    pub at: Option<TimeArg>,
    /// Frame number from 0, instead of `at`.
    pub frame: Option<u64>,
    /// Fraction of the project resolution. Default: long edge about 540 px.
    pub scale: Option<f64>,
    /// Motion blur samples (1 = off, fastest). Default: the project's.
    pub samples: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetArgs {
    /// Project file (.json).
    pub project: String,
    /// Start of the range (project time, or "b8", "bar 2", "drop"...). Default: the start.
    pub from: Option<TimeArg>,
    /// End of the range, written the same way. Default: the end.
    pub to: Option<TimeArg>,
    /// Frames spread evenly over the range (default 16, at most 64).
    pub count: Option<u32>,
    /// Every frame of the range instead of `count` (at most 64 frames): use it around a hit.
    pub every_frame: Option<bool>,
    /// Columns. Default: a roughly square grid.
    pub cols: Option<u32>,
    /// Cell size as a fraction of the project resolution. Default: cells about 320 px wide.
    pub scale: Option<f64>,
    /// Motion blur samples (1 = off, fastest). Default: the project's.
    pub samples: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VideoArgs {
    /// Project file (.json).
    pub project: String,
    /// Output file (.mp4). Overwritten if it exists.
    pub output: String,
    /// Start of the range (project time, or "b8", "bar 2", "drop"...). Default: the start.
    pub from: Option<TimeArg>,
    /// End of the range, written the same way. Default: the end.
    pub to: Option<TimeArg>,
    /// Fraction of the project resolution (default 1; 0.5 for a quick preview).
    pub scale: Option<f64>,
    /// Motion blur samples (1 = off). Default: the project's.
    pub samples: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobStatusArgs {
    /// Job id returned by render_video, reference_analyze or reference_compare. Omit to list recent jobs.
    pub job: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobArgs {
    /// Job id.
    pub job: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProbeArgs {
    /// Video file.
    pub video: String,
    /// First frame of the range to show (default 0).
    pub from_frame: Option<u64>,
    /// End of the range, exclusive (default: the last frame). Ranges of up to 60 frames are listed frame by frame.
    pub to_frame: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeArgs {
    /// Reference video file.
    pub video: String,
    /// Folder for reference_analysis.json, overlay.png and proxies/ (created if missing).
    pub out_dir: String,
    /// Tempo, if you know it; otherwise it is estimated from the audio.
    pub bpm: Option<f64>,
    /// Seconds from the start of the video to beat 0, if you know it.
    pub beat_offset: Option<f64>,
    /// Analyse the whole clip even when it loops (default: one period of the loop).
    pub whole: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferenceReadArgs {
    /// reference_analysis.json written by reference_analyze.
    pub analysis: String,
    /// Which part (default summary). "shot" needs `shot`.
    pub section: Option<Section>,
    /// Shot id, for section "shot".
    pub shot: Option<u64>,
    /// JSON pointer into the file instead of a section, e.g. /shots/0/curves/1.
    pub pointer: Option<String>,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Summary,
    Grid,
    Shots,
    Shot,
    Slots,
    Layers,
    Signature,
    Source,
    Ingest,
    Notes,
}

impl Section {
    fn name(self) -> &'static str {
        match self {
            Section::Summary => "summary",
            Section::Grid => "grid",
            Section::Shots => "shots",
            Section::Shot => "shot",
            Section::Slots => "slots",
            Section::Layers => "layers",
            Section::Signature => "signature",
            Section::Source => "source",
            Section::Ingest => "ingest",
            Section::Notes => "notes",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DraftArgs {
    /// reference_analysis.json written by reference_analyze.
    pub analysis: String,
    /// Project file to write (.json). An existing file is backed up first.
    pub output: String,
    /// Point the slots at the proxy stills taken from the reference, so the draft renders at once.
    pub proxies: Option<bool>,
    /// Project width in pixels (default: the reference's).
    pub width: Option<u32>,
    /// Project height in pixels (default: the reference's).
    pub height: Option<u32>,
    /// Write only the first period of a looping edit.
    pub single_loop: Option<bool>,
    /// Keep measured frame times instead of snapping keys to the quarter-beat grid.
    pub no_snap: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareArgs {
    /// reference_analysis.json of the reference edit.
    pub analysis: String,
    /// Video to compare with it: usually a render of your project.
    pub video: String,
    /// How many differences to list (default 15).
    pub top: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FitArgs {
    /// Project to improve (.json), usually drafted from the same analysis; any pictures may be in its slots.
    pub project: String,
    /// reference_analysis.json of the reference edit.
    pub analysis: String,
    /// Only these reference shots (ids as in the analysis). Default: every shot.
    pub shots: Option<Vec<u32>>,
    /// Parameter groups: "auto" (default) or any of exposure, exposure_decay, saturation, threshold, blur, scale, scale_decay, position, position_decay.
    pub params: Option<Vec<String>>,
    /// Renders allowed per shot (default 24). Time grows with it: about a second per render on a CPU.
    pub budget: Option<u32>,
    /// Report the changes it would make without writing the project.
    pub dry_run: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

// ---- results ---------------------------------------------------------------

/// What a tool hands back before it is turned into MCP content.
struct Reply {
    text: String,
    image: Option<images::Prepared>,
    structured: Option<Value>,
    /// The tool ran, and its answer is "no" (an edit that did not validate).
    failed: bool,
}

impl Reply {
    fn text(text: impl Into<String>) -> Self {
        Reply { text: text.into(), image: None, structured: None, failed: false }
    }
    fn data(text: impl Into<String>, structured: Value) -> Self {
        Reply { text: text.into(), image: None, structured: Some(structured), failed: false }
    }
}

type Out = Result<Reply, String>;

fn finish(out: Out) -> CallToolResult {
    match out {
        Ok(reply) if reply.text.chars().count() > TEXT_CAP => CallToolResult::error(vec![ContentBlock::text(format!(
            "the result is {} characters, over this server's limit of {TEXT_CAP}. Ask for a part: a JSON pointer or section, a shorter range, or fewer items.",
            reply.text.chars().count()
        ))]),
        Ok(reply) => {
            let mut content = Vec::new();
            if let Some(image) = &reply.image {
                content.push(ContentBlock::image(image.base64.clone(), image.mime));
            }
            content.push(ContentBlock::text(reply.text));
            let mut result = if reply.failed { CallToolResult::error(content) } else { CallToolResult::success(content) };
            result.structured_content = reply.structured;
            result
        }
        Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
    }
}

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn dir_of(path: &Path) -> PathBuf {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn stem_of(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn finite(value: Option<f64>, name: &str) -> Result<Option<f64>, String> {
    match value {
        Some(v) if !v.is_finite() => Err(format!("`{name}` must be a finite number")),
        other => Ok(other),
    }
}

fn adapter_text(adapter: &Value) -> String {
    if adapter.is_null() {
        return "unknown adapter".into();
    }
    format!("{} ({}{})", adapter["name"].as_str().unwrap_or("?"), adapter["backend"].as_str().unwrap_or("?"), if adapter["software"].as_bool() == Some(true) { ", software" } else { "" })
}

/// Replace every `{"$ref": "#/$defs/X"}` by the definition itself. Tool
/// schemas here are small and not recursive, and some clients do not resolve
/// references inside tool schemas.
fn inline_refs(schema: &mut Value, defs: &serde_json::Map<String, Value>, depth: usize) {
    match schema {
        Value::Object(map) => {
            let target = map.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/$defs/")).and_then(|name| defs.get(name)).cloned();
            if let (Some(Value::Object(def)), true) = (target, depth < 16) {
                map.remove("$ref");
                for (k, v) in def {
                    // What the use site says (its description) wins over the definition.
                    map.entry(k).or_insert(v);
                }
            }
            for value in map.values_mut() {
                inline_refs(value, defs, depth + 1);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| inline_refs(v, defs, depth + 1)),
        _ => {}
    }
}

/// Make the generated schemas smaller and friendlier to strict clients:
/// `"type": ["number", "null"]` becomes `"type": "number"` (an optional
/// argument is simply left out), descriptions are single lines, and
/// generator metadata is dropped.
fn tidy_schema(schema: &mut Value, top: bool) {
    match schema {
        Value::Object(map) => {
            if top {
                map.remove("$schema");
                map.remove("title");
                map.remove("$defs");
                map.entry("properties").or_insert_with(|| json!({}));
            }
            if let Some(Value::Array(types)) = map.get("type") {
                let kept: Vec<Value> = types.iter().filter(|t| t.as_str() != Some("null")).cloned().collect();
                if kept.len() == 1 {
                    map.insert("type".into(), kept[0].clone());
                }
            }
            // Option<Enum> and Option<Struct> come out as anyOf [T, null].
            if let Some(Value::Array(any)) = map.get("anyOf") {
                let kept: Vec<Value> = any.iter().filter(|t| t.get("type").and_then(Value::as_str) != Some("null")).cloned().collect();
                if kept.len() == 1 {
                    if let Value::Object(inner) = kept[0].clone() {
                        map.remove("anyOf");
                        for (k, v) in inner {
                            map.entry(k).or_insert(v);
                        }
                    }
                }
            }
            if matches!(map.get("type").and_then(Value::as_str), Some("number" | "integer")) {
                map.remove("format");
            }
            if let Some(Value::String(text)) = map.get_mut("description") {
                *text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            }
            for value in map.values_mut() {
                tidy_schema(value, false);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| tidy_schema(v, false)),
        _ => {}
    }
}

// ---- the server ------------------------------------------------------------

#[derive(Clone)]
pub struct EditsServer {
    ws: Workspace,
    config: Arc<Config>,
    serial: Arc<AtomicU64>,
    /// Held while a project file is read, changed and written back.
    writing: Arc<std::sync::Mutex<()>>,
    tool_router: ToolRouter<Self>,
}

impl EditsServer {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let root = match &config.root {
            Some(root) => root.clone(),
            None => std::env::current_dir()?,
        };
        let ws = Workspace::new(&root).map_err(anyhow::Error::msg)?;
        let mut tool_router = Self::tool_router();
        for route in tool_router.map.values_mut() {
            let mut schema = Value::Object((*route.attr.input_schema).clone());
            let defs = schema.get("$defs").and_then(Value::as_object).cloned().unwrap_or_default();
            inline_refs(&mut schema, &defs, 0);
            tidy_schema(&mut schema, true);
            if let Value::Object(map) = schema {
                route.attr.input_schema = Arc::new(map);
            }
        }
        Ok(Self { ws, config: Arc::new(config), serial: Arc::new(AtomicU64::new(0)), writing: Arc::default(), tool_router })
    }

    /// The tools as served, in the order `tools/list` returns them (by name).
    pub fn tools(&self) -> Vec<rmcp::model::Tool> {
        self.tool_router.list_all()
    }

    fn root(&self) -> &Path {
        self.ws.root()
    }

    fn bin(&self, tool: Tool) -> Result<PathBuf, String> {
        let explicit = match tool {
            Tool::Edits => self.config.edits_bin.as_deref(),
            Tool::Analyze => self.config.analyze_bin.as_deref(),
        };
        exec::locate(tool, explicit)
    }

    async fn run(&self, tool: Tool, args: Vec<OsString>) -> Result<exec::Output, String> {
        let bin = self.bin(tool)?;
        exec::run(&bin, &args, self.root(), self.config.timeout, STDOUT_CAP).await
    }

    /// Run a command whose stdout is one JSON document.
    async fn run_json(&self, tool: Tool, args: Vec<OsString>) -> Result<Value, String> {
        let out = self.run(tool, args).await?;
        if !out.ok() {
            return Err(out.error_text());
        }
        serde_json::from_str(&out.stdout).map_err(|e| format!("`{}` printed something that is not JSON ({e}): {}", tool.name(), exec::tail_chars(out.stdout.trim(), 400)))
    }

    /// The project as raw JSON and, when it parses, as a project.
    fn load(&self, path: &Path) -> Result<(Value, Option<Project>), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", self.ws.show(path)))?;
        let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {e}", self.ws.show(path)))?;
        let project = serde_json::from_value::<Project>(doc.clone()).ok();
        Ok((doc, project))
    }

    /// Refuse to render a project that reads files outside the workspace.
    fn confine_refs(&self, doc: &Value, project: &Path) -> Result<(), String> {
        let outside = edit::outside_refs(doc, &dir_of(project), &self.ws);
        if outside.is_empty() {
            return Ok(());
        }
        let lines: Vec<String> = outside.iter().map(edit::finding_line).collect();
        Err(format!("{} refers to files outside the workspace root, so it is not rendered:\n{}", self.ws.show(project), lines.join("\n")))
    }

    /// A fresh path under `<root>/.edits/renders/`.
    fn preview_path(&self, project: &Path, what: &str) -> Result<PathBuf, String> {
        let dir = edit::state_dir(self.root(), "renders").map_err(|e| format!("cannot create the previews folder: {e}"))?;
        let n = self.serial.fetch_add(1, Ordering::Relaxed);
        Ok(dir.join(format!("{}-{what}.tmp-{}-{n}.png", stem_of(project), std::process::id())))
    }

    // ---- project ----

    async fn describe(&self, a: ProjectArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let out = self.run(Tool::Edits, vec!["describe".into(), project.into()]).await?;
        if !out.ok() {
            return Err(format!("{}\nproject_check lists what is wrong and where.", out.error_text()));
        }
        Ok(Reply::text(out.stdout.trim_end()))
    }

    async fn read(&self, a: ReadArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let (doc, _) = self.load(&project)?;
        let ptr = a.pointer.unwrap_or_default();
        let value = pointer::get(&doc, &ptr)?;
        let text = jsonfmt::pretty(value);
        if text.chars().count() > TEXT_CAP {
            let ids: Vec<&str> = doc.get("layers").and_then(Value::as_array).into_iter().flatten().filter_map(|l| l.get("id").and_then(Value::as_str)).collect();
            return Err(format!(
                "that part of {} is {} characters, over the limit of {TEXT_CAP}. Read it in parts: /settings, /slots, /camera, /layers/@<id> (layer ids: {}), or project_describe for an outline.",
                self.ws.show(&project),
                text.chars().count(),
                ids.join(", ")
            ));
        }
        let structured = json!({ "pointer": pointer::join(&pointer::tokens(&ptr)), "value": value });
        Ok(Reply::data(text, structured))
    }

    async fn check(&self, a: ProjectArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let out = self.run(Tool::Edits, os(&["check", "--json"]).into_iter().chain([project.clone().into()]).collect()).await?;
        // `check` exits 1 when the project has errors and still prints its JSON.
        let mut report: Value = serde_json::from_str(&out.stdout).map_err(|_| out.error_text())?;
        let mut findings: Vec<Value> = report["findings"].as_array().cloned().unwrap_or_default();
        for f in &mut findings {
            if let Some(p) = f["path"].as_str().and_then(pointer::from_check_path) {
                f["pointer"] = json!(p);
            }
        }
        // What only the raw JSON shows: ignored keys and files outside the workspace.
        if let Ok((doc, Some(parsed))) = self.load(&project) {
            for f in edit::raw_findings(&doc, &parsed, &dir_of(&project), Some(&self.ws)) {
                findings.push(json!({ "severity": f.severity, "path": f.path, "pointer": f.path, "message": f.message, "hint": f.hint }));
            }
        }
        let errors = findings.iter().filter(|f| f["severity"] == "error").count();
        let warnings = findings.len() - errors;
        let mut text = format!("{}: {errors} error(s), {warnings} warning(s)", self.ws.show(&project));
        for f in &findings {
            let at = f["pointer"].as_str().or(f["path"].as_str()).filter(|s| !s.is_empty()).map(|s| format!(" {s}")).unwrap_or_default();
            let hint = f["hint"].as_str().map(|h| format!(" ({h})")).unwrap_or_default();
            text.push_str(&format!("\n{}{at}: {}{hint}", f["severity"].as_str().unwrap_or("finding"), f["message"].as_str().unwrap_or("")));
        }
        report["ok"] = json!(errors == 0);
        report["errors"] = json!(errors);
        report["warnings"] = json!(warnings);
        report["findings"] = json!(findings);
        Ok(Reply::data(text, report))
    }

    async fn edit(&self, a: EditArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let dry_run = a.dry_run.unwrap_or(false);
        // Read, change and write as one step even when a client sends two edits at once.
        let outcome = {
            let _one_at_a_time = self.writing.lock().unwrap_or_else(|e| e.into_inner());
            edit::edit_file(&project, &a.ops, dry_run, Some(&self.ws))?
        };
        let shown = self.ws.show(&project);
        let errors: Vec<&edits_core::check::Finding> = outcome.findings.iter().filter(|f| f.severity == Severity::Error).collect();
        let warnings: Vec<&edits_core::check::Finding> = outcome.findings.iter().filter(|f| f.severity == Severity::Warning).collect();
        let mut text = if !outcome.ok {
            let already = if outcome.preexisting > 0 { format!(" ({} of them already in the file: include their fixes)", outcome.preexisting) } else { String::new() };
            format!("NOT applied: the result would have {} error(s){already}, so {shown} is unchanged. Fix the ops and send the whole batch again.", errors.len())
        } else if outcome.changes.is_empty() {
            format!("no changes: {shown} already has these values; nothing was written")
        } else if dry_run {
            format!("dry run, nothing written: {} op(s) would change {} value(s) in {shown}", a.ops.len(), outcome.changes.len())
        } else {
            format!("applied {} op(s) to {shown}: {} change(s)", a.ops.len(), outcome.changes.len())
        };
        for f in &errors {
            text.push_str(&format!("\n{}", edit::finding_line(f)));
        }
        if outcome.ok {
            const SHOWN: usize = 40;
            for c in outcome.changes.iter().take(SHOWN) {
                text.push_str(&format!("\n{}", edit::change_line(c)));
            }
            if outcome.changes.len() > SHOWN {
                text.push_str(&format!("\n… and {} more", outcome.changes.len() - SHOWN));
            }
            for f in &warnings {
                text.push_str(&format!("\n{}", edit::finding_line(f)));
            }
        }
        if let Some(backup) = &outcome.backup {
            text.push_str(&format!("\nprevious version: {} (project_undo restores it)", self.ws.show(backup)));
        }
        let structured = json!({
            "ok": outcome.ok,
            "written": outcome.written,
            "dry_run": dry_run,
            "changes": outcome.changes,
            "findings": outcome.findings,
            "backup": outcome.backup.as_ref().map(|b| self.ws.show(b)),
        });
        Ok(Reply { text, image: None, structured: Some(structured), failed: !outcome.ok })
    }

    async fn undo(&self, a: ProjectArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let u = {
            let _one_at_a_time = self.writing.lock().unwrap_or_else(|e| e.into_inner());
            edit::undo_file(&project)?
        };
        let mut text = format!("restored {} from {}: {} change(s)", self.ws.show(&project), self.ws.show(&u.restored), u.changes.len());
        // The history is shared with the window: say whose edit this was.
        let who = match u.entry.actor.as_str() {
            edit::AGENT => "an agent's edit".to_string(),
            "window" => "an edit made by the person in the edits-gui window".to_string(),
            other => format!("an edit by {other}"),
        };
        if !u.entry.summary.is_empty() {
            text.push_str(&format!("\nundid {who}: {}", u.entry.summary));
        }
        for c in u.changes.iter().take(40) {
            text.push_str(&format!("\n{}", edit::change_line(c)));
        }
        text.push_str(&format!("\n{} older version(s) remain; the version that was replaced is kept at {}", u.remaining, self.ws.show(&u.kept)));
        let structured = json!({ "restored": self.ws.show(&u.restored), "changes": u.changes, "remaining": u.remaining, "replaced_version": self.ws.show(&u.kept), "undone_edit": { "actor": u.entry.actor, "summary": u.entry.summary } });
        Ok(Reply::data(text, structured))
    }

    async fn effects(&self, a: EffectsArgs) -> Out {
        let all = self.run_json(Tool::Edits, os(&["effects", "--json"])).await?;
        let list = all.as_array().cloned().unwrap_or_default();
        let transitions = self.run_json(Tool::Edits, os(&["transitions", "--json"])).await?.as_array().cloned().unwrap_or_default();
        let chosen: Vec<Value> = match &a.kind {
            None => list,
            Some(kind) => {
                let kinds: Vec<&str> = list.iter().filter_map(|e| e["kind"].as_str()).collect();
                let found: Vec<Value> = list.iter().filter(|e| e["kind"].as_str() == Some(kind)).cloned().collect();
                if found.is_empty() {
                    if let Some(t) = transitions.iter().find(|t| t["kind"].as_str() == Some(kind.trim_start_matches("transition:"))) {
                        let mut text = format!("transition {}: {} (default {} beats; {})\nOn the incoming layer: \"transition\": {{ \"kind\": \"{}\", \"duration\": d, \"params\": {{...}}, \"ease\": \"linear\" }}, or preset_apply transition:{} which also overlaps the cut.", t["kind"].as_str().unwrap_or("?"), t["doc"].as_str().unwrap_or(""), t["duration"], t["credit"].as_str().unwrap_or(""), t["kind"].as_str().unwrap_or("?"), t["kind"].as_str().unwrap_or("?"));
                        for p in t["params"].as_array().into_iter().flatten() {
                            text.push_str(&format!("\n    {} = {}  {}", p["name"].as_str().unwrap_or("?"), jsonfmt::inline(&p["default"]), p["doc"].as_str().unwrap_or("")));
                        }
                        return Ok(Reply::data(text, json!({ "transition": t })));
                    }
                    let tkinds: Vec<&str> = transitions.iter().filter_map(|t| t["kind"].as_str()).collect();
                    return Err(format!("no effect \"{kind}\"; effects: {}; transitions: {}", kinds.join(", "), tkinds.join(", ")));
                }
                found
            }
        };
        let mut text = String::from("An effect is { \"kind\", \"params\": { ... }, \"options\": { ... } } in a layer's \"effects\"; every parameter can be keyframed, options cannot. Units: %h = percent of frame height, pos = [x, y] composition units, deg, color = linear RGB.\n");
        for e in &chosen {
            text.push_str(&format!("{} ({}): {}\n", e["kind"].as_str().unwrap_or("?"), e["category"].as_str().unwrap_or(""), e["doc"].as_str().unwrap_or("")));
            for p in e["params"].as_array().into_iter().flatten() {
                let doc = p["doc"].as_str().filter(|d| !d.is_empty()).map(|d| format!("  {d}")).unwrap_or_default();
                let unit = p["unit"].as_str().filter(|u| !u.is_empty()).map(|u| format!(" {u}")).unwrap_or_default();
                let range = p["range"].as_array().map(|r| format!(" [{}..{}]", r[0].as_f64().map(|v| v.to_string()).unwrap_or_default(), r[1].as_f64().map(|v| v.to_string()).unwrap_or_default())).unwrap_or_default();
                text.push_str(&format!("    {} = {}{unit}{range}{doc}\n", p["name"].as_str().unwrap_or("?"), jsonfmt::inline(&p["default"])));
            }
            for o in e["options"].as_array().into_iter().flatten() {
                let values: Vec<&str> = o["values"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).collect();
                text.push_str(&format!("    option {} = {}  {}\n", o["name"].as_str().unwrap_or("?"), values.join(" | "), o["doc"].as_str().unwrap_or("")));
            }
        }
        if a.kind.is_none() {
            let tkinds: Vec<&str> = transitions.iter().filter_map(|t| t["kind"].as_str()).collect();
            text.push_str(&format!("Transitions (a layer's \"transition\" field, between the picture beneath and the layer; pass one as `kind` for its parameters): {}\n", tkinds.join(", ")));
            return Ok(Reply::data(text.trim_end(), json!({ "effects": chosen, "transitions": transitions })));
        }
        Ok(Reply::data(text.trim_end(), json!({ "effects": chosen })))
    }

    // ---- rendering ----

    async fn frame(&self, a: FrameArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let at = time_text(&a.at, "at")?;
        if at.is_some() && a.frame.is_some() {
            return Err("give `at` (project time) or `frame` (frame number), not both".into());
        }
        let (doc, parsed) = self.load(&project)?;
        self.confine_refs(&doc, &project)?;
        // Default: long edge about 540 px. Whatever is asked for, never render
        // more pixels than the image budget lets through.
        let size = parsed.as_ref().map(|p| (p.settings.width, p.settings.height)).filter(|(w, h)| *w > 0 && *h > 0);
        let mut scale = match (finite(a.scale, "scale")?, size) {
            (Some(s), _) => s,
            (None, Some((w, h))) => (FRAME_EDGE / f64::from(w.max(h))).min(1.0),
            (None, None) => 0.5,
        };
        if !(scale > 0.0 && scale <= 8.0) {
            return Err("`scale` must be greater than 0 and at most 8".into());
        }
        if let Some((w, h)) = size {
            let (rw, rh) = ((f64::from(w) * scale) as u32, (f64::from(h) * scale) as u32);
            scale *= images::fit_scale(rw.max(1), rh.max(1), images::MAX_TOKENS);
        }
        let tmp = self.preview_path(&project, "frame")?;
        let mut args: Vec<OsString> = vec!["frame".into(), project.clone().into(), "-o".into(), tmp.clone().into(), "--scale".into(), format!("{scale:.5}").into()];
        if let Some(t) = at {
            args.push(format!("--at={t}").into());
        }
        if let Some(n) = a.frame {
            args.extend(os(&["--frame", &n.to_string()]));
        }
        if let Some(n) = a.samples {
            args.extend(os(&["--samples", &n.to_string()]));
        }
        let info = self.run_json(Tool::Edits, args).await.inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        let number = info["frame"].as_u64().unwrap_or(0);
        let saved = tmp.with_file_name(format!("{}-f{number}.png", stem_of(&project)));
        let saved = if std::fs::rename(&tmp, &saved).is_ok() { saved } else { tmp };
        let image = images::prepare(&saved, images::MAX_TOKENS, images::MAX_BYTES)?;
        let unit = info["time_unit"].as_str().unwrap_or("seconds");
        let time = if unit == "beats" { format!("beat {} ({} s)", info["time"], info["seconds"]) } else { format!("{} s", info["seconds"]) };
        let text = format!(
            "frame {number} of {} at {time}; {}; adapter {}; rendered in {} ms; saved as {}",
            self.ws.show(&project),
            image.note(),
            adapter_text(&info["adapter"]),
            info["render_ms"],
            self.ws.show(&saved)
        );
        Ok(Reply { text, image: Some(image), structured: None, failed: false })
    }

    async fn sheet(&self, a: SheetArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let scale = finite(a.scale, "scale")?;
        let every = a.every_frame.unwrap_or(false);
        if every && a.count.is_some() {
            return Err("give `count` or `every_frame`, not both".into());
        }
        if a.count.is_some_and(|c| c == 0 || u64::from(c) > SHEET_MAX) {
            return Err(format!("`count` must be between 1 and {SHEET_MAX}; for more detail, make several sheets over shorter ranges"));
        }
        let (doc, parsed) = self.load(&project)?;
        self.confine_refs(&doc, &project)?;
        let tc = edits_core::time::TimeCtx::from_doc(&doc);
        let from = a.from.as_ref().map(|t| t.resolve(&tc, "from")).transpose()?;
        let to = a.to.as_ref().map(|t| t.resolve(&tc, "to")).transpose()?;
        if let (true, Some(p)) = (every, &parsed) {
            // The same frame arithmetic as `edits sheet`: frames whose start lies in [from, to).
            let index = |t: f64| (p.to_seconds(t) * p.settings.fps - 1e-6).ceil().max(0.0) as u64;
            let total = p.frame_count();
            let (first, end) = (from.map(index).unwrap_or(0), to.map(index).unwrap_or(total).min(total));
            if end.saturating_sub(first) > SHEET_MAX {
                return Err(format!(
                    "`every_frame` over this range is {} frames; one sheet holds at most {SHEET_MAX}. Narrow `from`/`to` (one beat is about {:.0} frames here), or use `count`.",
                    end - first,
                    1.0 / p.frame_len().max(1e-9)
                ));
            }
        }
        let tmp = self.preview_path(&project, "sheet")?;
        let mut args: Vec<OsString> = vec!["sheet".into(), project.clone().into(), "-o".into(), tmp.clone().into(), "-q".into()];
        for (flag, value) in [("--from", from), ("--to", to), ("--scale", scale)] {
            if let Some(v) = value {
                args.push(format!("{flag}={v}").into());
            }
        }
        if every {
            args.push("--every-frame".into());
        }
        for (flag, value) in [("--count", a.count), ("--cols", a.cols), ("--samples", a.samples)] {
            if let Some(v) = value {
                args.extend(os(&[flag, &v.to_string()]));
            }
        }
        let info = self.run_json(Tool::Edits, args).await.inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        let frames: Vec<u64> = info["frames"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        let (first, last) = (frames.first().copied().unwrap_or(0), frames.last().copied().unwrap_or(0));
        let saved = tmp.with_file_name(format!("{}-sheet-f{first}-f{last}-n{}.png", stem_of(&project), frames.len()));
        let saved = if std::fs::rename(&tmp, &saved).is_ok() { saved } else { tmp };
        let image = images::prepare(&saved, images::MAX_TOKENS, images::MAX_BYTES)?;
        let list = if frames.len() <= 24 { frames.iter().map(|f| format!("#{f}")).collect::<Vec<_>>().join(" ") } else { format!("#{first} to #{last}") };
        let text = format!(
            "contact sheet of {}: {} frames ({list}), {} columns x {} rows, read left to right then down; each cell is labelled with its frame number, project time and seconds; {}; adapter {}; saved as {}",
            self.ws.show(&project),
            frames.len(),
            info["cols"],
            info["rows"],
            image.note(),
            adapter_text(&info["adapter"]),
            self.ws.show(&saved)
        );
        Ok(Reply { text, image: Some(image), structured: None, failed: false })
    }

    async fn video(&self, a: VideoArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let output = self.ws.resolve(&a.output, "output")?;
        if output.is_dir() {
            return Err(format!("output: {} is a folder; give a file name ending in .mp4", self.ws.show(&output)));
        }
        let (from, to, scale) = (time_text(&a.from, "from")?, time_text(&a.to, "to")?, finite(a.scale, "scale")?);
        // Fail now, with findings, rather than in a job the agent has to poll to learn about.
        let (doc, _) = self.load(&project)?;
        let findings = edit::validate(&doc, &dir_of(&project), Some(&self.ws));
        let errors: Vec<String> = findings.iter().filter(|f| f.severity == Severity::Error).map(edit::finding_line).collect();
        if !errors.is_empty() {
            return Err(format!("{} has {} error(s), so no render was started:\n{}", self.ws.show(&project), errors.len(), errors.join("\n")));
        }
        let bin = self.bin(Tool::Edits)?;
        let mut args: Vec<OsString> = vec!["render".into(), project.clone().into(), "-o".into(), output.clone().into()];
        for (flag, value) in [("--from", from), ("--to", to), ("--scale", scale.map(|v| v.to_string()))] {
            if let Some(v) = value {
                args.push(format!("{flag}={v}").into());
            }
        }
        if let Some(n) = a.samples {
            args.extend(os(&["--samples", &n.to_string()]));
        }
        let meta = json!({ "project": self.ws.show(&project), "output": self.ws.show(&output) });
        let job = jobs::start(self.root(), "render", &bin, &args, meta)?;
        let text = format!("started render job {job}: {} -> {}. Call job_status with this job id until its state is completed (it reports frames done and time left); job_cancel stops it.", self.ws.show(&project), self.ws.show(&output));
        Ok(Reply::data(text, json!({ "job": job, "kind": "render", "state": "running", "output": self.ws.show(&output) })))
    }

    // ---- jobs ----

    /// One job as text and JSON. Finished jobs are read according to their kind.
    fn job_report(&self, s: &jobs::Status, top: usize) -> (String, Value) {
        let mut data = json!({ "job": s.id, "kind": s.kind, "elapsed_seconds": (s.elapsed_seconds * 10.0).round() / 10.0 });
        let head = format!("job {} ({})", s.id, s.kind);
        let analysis_path = s.meta["analysis"].as_str().map(|p| self.root().join(p));
        // Without a recorded exit code (the server that started the job is
        // gone), a job counts as completed when its output is complete.
        let complete = match (&s.state, s.kind.as_str()) {
            (State::Exited(Some(code)), _) => *code == 0,
            (State::Exited(None), "analyze") => s.stdout.contains("wrote ") && analysis_path.as_ref().is_some_and(|p| p.is_file()),
            (State::Exited(None), _) => serde_json::from_str::<Value>(&s.stdout).is_ok(),
            _ => false,
        };
        let text = match &s.state {
            State::Running => {
                data["state"] = json!("running");
                let progress = match &s.progress {
                    Some(p) => {
                        data["progress"] = json!({ "frames_done": p.done, "frames": p.total, "percent": (p.done as f64 * 100.0 / p.total.max(1) as f64).round() });
                        format!("; {}", p.line)
                    }
                    None => String::new(),
                };
                format!("{head} is running, {:.0} s so far{progress}. Call job_status again in a few seconds.", s.elapsed_seconds)
            }
            State::Cancelled => {
                data["state"] = json!("cancelled");
                format!("{head} was cancelled after {:.0} s", s.elapsed_seconds)
            }
            State::Exited(_) if complete => {
                data["state"] = json!("completed");
                match s.kind.as_str() {
                    "render" => {
                        let r: Value = serde_json::from_str(&s.stdout).unwrap_or(Value::Null);
                        data["result"] = r.clone();
                        data["output"] = s.meta["output"].clone();
                        format!(
                            "{head} completed in {:.1} s: {} ({}x{}, {} frames, {} s at {} fps, {} ms per frame, encoder {}, audio: {}, adapter {})",
                            s.elapsed_seconds,
                            s.meta["output"].as_str().unwrap_or("?"),
                            r["width"],
                            r["height"],
                            r["frames"],
                            r["duration_seconds"],
                            r["fps"],
                            r["ms_per_frame"],
                            r["encoder"].as_str().unwrap_or("?"),
                            r["audio"],
                            adapter_text(&r["adapter"])
                        )
                    }
                    "analyze" => {
                        let shown = s.meta["analysis"].as_str().unwrap_or("reference_analysis.json");
                        let summary = analysis_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|a| summarize::analysis_summary(&a));
                        data["analysis"] = json!(shown);
                        match summary {
                            Some(summary) => {
                                let text = summarize::analysis_summary_text(&summary);
                                data["summary"] = summary;
                                format!("{head} completed in {:.1} s. Analysis: {shown} (overlay.png and proxies/ are beside it).\n{text}\nNext: reference_read for a section (shots, shot, grid, layers), reference_draft to write a project from it.", s.elapsed_seconds)
                            }
                            None => format!("{head} completed in {:.1} s, but {shown} could not be read back.\n{}", s.elapsed_seconds, s.stdout.trim()),
                        }
                    }
                    "compare" => match serde_json::from_str::<Value>(&s.stdout) {
                        Ok(c) => {
                            let (text, structured) = summarize::comparison(&c, top);
                            data["comparison"] = structured;
                            format!("{head} completed in {:.1} s: {} against {}\n{text}", s.elapsed_seconds, s.meta["video"].as_str().unwrap_or("the video"), s.meta["analysis"].as_str().unwrap_or("the analysis"))
                        }
                        Err(_) => format!("{head} completed, but its output is not JSON:\n{}", exec::tail_chars(s.stdout.trim(), 1500)),
                    },
                    "fit" => match serde_json::from_str::<Value>(&s.stdout) {
                        Ok(r) => {
                            let (text, structured) = summarize::fit_report(&r);
                            data["fit"] = structured;
                            format!("{head} completed in {:.1} s.\n{text}", s.elapsed_seconds)
                        }
                        Err(_) => format!("{head} completed, but its output is not JSON:\n{}", exec::tail_chars(s.stdout.trim(), 1500)),
                    },
                    _ => format!("{head} completed in {:.1} s\n{}", s.elapsed_seconds, exec::tail_chars(s.stdout.trim(), 1500)),
                }
            }
            State::Exited(code) => {
                data["state"] = json!("failed");
                let log = s.log_tail.trim();
                let from = log.find("error:").or_else(|| log.find("Error:")).unwrap_or(0);
                let error = exec::tail_chars(&log[from..], 2500);
                data["error"] = json!(error);
                let code = code.map(|c| format!("exit code {c}")).unwrap_or_else(|| "no exit code recorded and no complete output".into());
                format!("{head} failed after {:.1} s ({code}):\n{}", s.elapsed_seconds, if error.is_empty() { "it printed nothing" } else { &error })
            }
        };
        (text, data)
    }

    async fn job_status(&self, a: JobStatusArgs) -> Out {
        let Some(id) = a.job else {
            let all = jobs::list(self.root());
            if all.is_empty() {
                return Ok(Reply::data("no jobs in this workspace yet (render_video, reference_analyze and reference_compare start them)", json!({ "jobs": [] })));
            }
            let mut text = String::from("jobs, newest first:");
            let mut listed = Vec::new();
            for s in all.iter().take(12) {
                let (_, data) = self.job_report(s, 0);
                text.push_str(&format!("\n{}  {}  {:.0} s", s.id, data["state"].as_str().unwrap_or("?"), s.elapsed_seconds));
                listed.push(json!({ "job": s.id, "kind": s.kind, "state": data["state"], "elapsed_seconds": data["elapsed_seconds"] }));
            }
            return Ok(Reply::data(text, json!({ "jobs": listed })));
        };
        let status = jobs::status(self.root(), &id)?;
        let (text, data) = self.job_report(&status, 15);
        let failed = data["state"] == "failed";
        Ok(Reply { text, image: None, structured: Some(data), failed })
    }

    async fn job_cancel(&self, a: JobArgs) -> Out {
        let before = jobs::status(self.root(), &a.job)?;
        if before.state != State::Running {
            let (text, data) = self.job_report(&before, 15);
            return Ok(Reply::data(format!("nothing to cancel: {text}"), data));
        }
        let root = self.root().to_path_buf();
        let id = a.job.clone();
        let after = tokio::task::spawn_blocking(move || jobs::cancel(&root, &id)).await.map_err(|e| format!("cancelling failed: {e}"))??;
        let (mut text, mut data) = self.job_report(&after, 15);
        if after.state == State::Running {
            text = format!("job {} was told to stop but is still running; call job_status in a moment", after.id);
        } else if after.kind == "render" {
            // A half-written MP4 is not a video: remove it.
            if let Some(output) = after.meta["output"].as_str().and_then(|p| self.ws.resolve(p, "output").ok()) {
                if std::fs::remove_file(&output).is_ok() {
                    text.push_str(&format!("; the partial output {} was removed", self.ws.show(&output)));
                    data["removed_output"] = json!(self.ws.show(&output));
                }
            }
        }
        Ok(Reply::data(text, data))
    }

    // ---- reference ----

    async fn probe(&self, a: ProbeArgs) -> Out {
        let video = self.ws.file(&a.video, "video")?;
        let data = self.run_json(Tool::Analyze, vec!["probe".into(), video.clone().into(), "--json".into()]).await?;
        let (text, mut structured) = summarize::probe(&data, a.from_frame, a.to_frame, 60)?;
        structured["video"]["path"] = json!(self.ws.show(&video));
        Ok(Reply::data(text.replacen(data["video"]["path"].as_str().unwrap_or("\u{0}"), &self.ws.show(&video), 1), structured))
    }

    async fn analyze(&self, a: AnalyzeArgs) -> Out {
        let video = self.ws.file(&a.video, "video")?;
        let out_dir = self.ws.resolve(&a.out_dir, "out_dir")?;
        if out_dir.is_file() {
            return Err(format!("out_dir: {} is a file; give a folder", self.ws.show(&out_dir)));
        }
        let (bpm, beat_offset) = (finite(a.bpm, "bpm")?, finite(a.beat_offset, "beat_offset")?);
        if bpm.is_some_and(|b| b <= 0.0) {
            return Err("`bpm` must be positive".into());
        }
        let bin = self.bin(Tool::Analyze)?;
        let mut args: Vec<OsString> = vec!["analyze".into(), video.clone().into(), "-o".into(), out_dir.clone().into()];
        if let Some(b) = bpm {
            args.extend(os(&["--bpm", &b.to_string()]));
        }
        if let Some(o) = beat_offset {
            // `=` so that a negative offset is not read as a flag.
            args.push(format!("--beat-offset={o}").into());
        }
        if a.whole.unwrap_or(false) {
            args.push("--whole".into());
        }
        let analysis = out_dir.join("reference_analysis.json");
        let meta = json!({ "video": self.ws.show(&video), "out_dir": self.ws.show(&out_dir), "analysis": self.ws.show(&analysis) });
        let job = jobs::start(self.root(), "analyze", &bin, &args, meta)?;
        let text = format!(
            "started analysis job {job}: {} -> {}. It takes roughly a second per ten frames. Call job_status with this job id until its state is completed; the result is the analysis path and a summary.",
            self.ws.show(&video),
            self.ws.show(&analysis)
        );
        Ok(Reply::data(text, json!({ "job": job, "kind": "analyze", "state": "running", "analysis": self.ws.show(&analysis) })))
    }

    async fn fit(&self, a: FitArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let analysis = self.ws.file(&a.analysis, "analysis")?;
        let bin = self.bin(Tool::Edits)?;
        let mut args: Vec<OsString> = vec!["fit".into(), project.clone().into(), "--reference".into(), analysis.clone().into(), "--json".into()];
        if let Some(shots) = a.shots.as_ref().filter(|v| !v.is_empty()) {
            args.push(format!("--shots={}", shots.iter().map(u32::to_string).collect::<Vec<_>>().join(",")).into());
        }
        if let Some(params) = a.params.as_ref().filter(|v| !v.is_empty()) {
            args.push(format!("--params={}", params.join(",")).into());
        }
        if let Some(b) = a.budget {
            if b == 0 || b > 400 {
                return Err("`budget` must be between 1 and 400 renders per shot".into());
            }
            args.extend(os(&["--budget", &b.to_string()]));
        }
        let dry = a.dry_run.unwrap_or(false);
        if dry {
            args.push("--dry-run".into());
        }
        let meta = json!({ "project": self.ws.show(&project), "analysis": self.ws.show(&analysis), "dry_run": dry });
        let job = jobs::start(self.root(), "fit", &bin, &args, meta)?;
        let text = format!(
            "started fit job {job}: {} against {}{}. It renders each shot up to {} times and takes minutes on a CPU. Call job_status with this job id; the result lists the values changed and the scores before and after. {}",
            self.ws.show(&project),
            self.ws.show(&analysis),
            if dry { " (dry run)" } else { "" },
            a.budget.unwrap_or(24),
            if dry { "Nothing is written." } else { "The project is changed only if the whole-clip score does not get worse, as one history entry (project_undo takes it back); if the project changes meanwhile, the fit is not written." }
        );
        Ok(Reply::data(text, json!({ "job": job, "kind": "fit", "state": "running" })))
    }

    async fn reference_read(&self, a: ReferenceReadArgs) -> Out {
        let path = self.ws.file(&a.analysis, "analysis")?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", self.ws.show(&path)))?;
        let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {e}", self.ws.show(&path)))?;
        if doc.get("shots").is_none() || doc.get("beats").is_none() {
            return Err(format!("{} does not look like a reference analysis (no \"shots\" and \"beats\"); pass the reference_analysis.json written by reference_analyze", self.ws.show(&path)));
        }
        if a.pointer.is_some() && a.section.is_some() {
            return Err("give `section` or `pointer`, not both".into());
        }
        let (value, text) = match &a.pointer {
            Some(ptr) => {
                let v = pointer::get(&doc, ptr)?.clone();
                let t = jsonfmt::pretty(&v);
                (v, t)
            }
            None => {
                // A shot id alone is enough to mean "that shot".
                let section = a.section.map(Section::name).unwrap_or(if a.shot.is_some() { "shot" } else { "summary" });
                let v = summarize::analysis_section(&doc, section, a.shot)?;
                let t = if section == "summary" { summarize::analysis_summary_text(&v) } else { jsonfmt::pretty(&v) };
                (v, t)
            }
        };
        if text.chars().count() > TEXT_CAP {
            return Err(format!(
                "that part of the analysis is {} characters, over the limit of {TEXT_CAP}. Ask for less: one shot (section \"shot\" with `shot`), or a pointer such as /shots/0/curves/0 or /signature/lum.",
                text.chars().count()
            ));
        }
        let structured = match value {
            Value::Object(_) => value,
            other => json!({ "value": other }),
        };
        Ok(Reply::data(text, structured))
    }

    async fn draft(&self, a: DraftArgs) -> Out {
        let analysis = self.ws.file(&a.analysis, "analysis")?;
        let output = self.ws.resolve(&a.output, "output")?;
        if output.is_dir() {
            return Err(format!("output: {} is a folder; give a file name ending in .json", self.ws.show(&output)));
        }
        let backup = if output.is_file() { Some(edit::backup(&output, "reference_draft replaced the project with a new draft").map_err(|e| format!("cannot back up the existing {}: {e}", self.ws.show(&output)))?) } else { None };
        let mut args: Vec<OsString> = vec!["draft".into(), analysis.clone().into(), "-o".into(), output.clone().into()];
        for (flag, on) in [("--proxies", a.proxies), ("--single-loop", a.single_loop), ("--no-snap", a.no_snap)] {
            if on.unwrap_or(false) {
                args.push(flag.into());
            }
        }
        for (flag, value) in [("--width", a.width), ("--height", a.height)] {
            if let Some(v) = value {
                args.extend(os(&[flag, &v.to_string()]));
            }
        }
        let out = self.run(Tool::Analyze, args).await?;
        if !output.is_file() {
            return Err(out.error_text());
        }
        let (doc, _) = self.load(&output)?;
        let findings = edit::validate(&doc, &dir_of(&output), Some(&self.ws));
        let errors = findings.iter().filter(|f| f.severity == Severity::Error).count();
        let mut text = format!("{}\n", out.stdout.trim().replace(&output.display().to_string(), &self.ws.show(&output)));
        if let Some(b) = &backup {
            text.push_str(&format!("the previous {} is kept at {} (project_undo restores it)\n", self.ws.show(&output), self.ws.show(b)));
        }
        // The outline is the cheapest way to see what was drafted; it needs the `edits` binary.
        match self.run(Tool::Edits, vec!["describe".into(), output.clone().into()]).await {
            Ok(d) if d.ok() => text.push_str(&format!("\n{}\n", d.stdout.trim_end())),
            Ok(d) => text.push_str(&format!("\n(no outline: {})\n", d.error_text())),
            Err(e) => text.push_str(&format!("\n(no outline: {e})\n")),
        }
        text.push_str(&format!("\ncheck: {errors} error(s), {} warning(s)", findings.len() - errors));
        for f in &findings {
            text.push_str(&format!("\n{}", edit::finding_line(f)));
        }
        text.push_str("\nNext: point the slots at your pictures (project_edit set_slot), look at a render_sheet, then reference_compare a render against the analysis.");
        let structured = json!({ "project": self.ws.show(&output), "ok": errors == 0 && out.ok(), "findings": findings, "backup": backup.as_ref().map(|b| self.ws.show(b)) });
        Ok(Reply { text, image: None, structured: Some(structured), failed: errors > 0 || !out.ok() })
    }

    async fn compare(&self, a: CompareArgs) -> Out {
        let analysis = self.ws.file(&a.analysis, "analysis")?;
        let video = self.ws.file(&a.video, "video")?;
        let top = a.top.unwrap_or(15).clamp(1, 60) as usize;
        let bin = self.bin(Tool::Analyze)?;
        let args: Vec<OsString> = vec!["compare".into(), analysis.clone().into(), video.clone().into(), "--json".into()];
        let meta = json!({ "analysis": self.ws.show(&analysis), "video": self.ws.show(&video) });
        // The candidate video is analysed in full, which can outlast a client's
        // tool timeout: run it as a job and answer inline when it is quick.
        let job = jobs::start(self.root(), "compare", &bin, &args, meta)?;
        let status = jobs::wait(self.root(), &job, self.config.timeout.saturating_sub(Duration::from_secs(5))).await?;
        let (text, mut data) = self.job_report(&status, top);
        if status.state == State::Running {
            data["state"] = json!("running");
            return Ok(Reply::data(format!("the comparison is still running as job {job} ({:.0} s so far). Call job_status with this job id for the result.", status.elapsed_seconds), data));
        }
        let failed = data["state"] != "completed";
        let text = match text.split_once('\n') {
            Some((_, body)) if !failed => format!("{} against {}: signature-space comparison (pixels are not compared, so different pictures are fine)\n{body}", self.ws.show(&video), self.ws.show(&analysis)),
            _ => text,
        };
        Ok(Reply { text, image: None, structured: Some(data), failed })
    }

    async fn doctor_report(&self) -> Out {
        let mut data = json!({
            "server": { "name": "edits-mcp", "version": env!("CARGO_PKG_VERSION"), "workspace_root": self.root().display().to_string() },
        });
        let mut text = format!("workspace root: {} (every path must be inside it)\n", self.root().display());
        for tool in [Tool::Edits, Tool::Analyze] {
            match self.bin(tool) {
                Ok(path) => {
                    text.push_str(&format!("{}: {}\n", tool.name(), path.display()));
                    data["executables"][tool.name()] = json!(path.display().to_string());
                }
                Err(e) => {
                    text.push_str(&format!("{}: NOT FOUND ({e})\n", tool.name()));
                    data["executables"][tool.name()] = Value::Null;
                }
            }
        }
        let mut ok = false;
        if self.bin(Tool::Edits).is_ok() {
            let out = self.run(Tool::Edits, os(&["doctor", "--json"])).await?;
            // `doctor` exits 1 when something is missing and still prints its JSON.
            let d: Value = serde_json::from_str(&out.stdout).map_err(|_| out.error_text())?;
            ok = d["ok"].as_bool().unwrap_or(false);
            match d["adapter"].as_object() {
                Some(_) => text.push_str(&format!("adapter: {}, type {}, driver {}\n", adapter_text(&d["adapter"]), d["adapter"]["type"].as_str().unwrap_or("?"), d["adapter"]["driver"].as_str().unwrap_or("?"))),
                None => text.push_str(&format!("adapter: NOT AVAILABLE: {} (on Linux without a GPU install Mesa lavapipe: mesa-vulkan-drivers)\n", d["adapter_error"].as_str().unwrap_or("unknown error"))),
            }
            match d["ffmpeg"]["version"].as_str() {
                Some(v) => text.push_str(&format!("ffmpeg: {v}\n")),
                None => text.push_str("ffmpeg: NOT FOUND (install ffmpeg, or set EDITS_FFMPEG)\n"),
            }
            text.push_str(&format!("encoders: h264 {}, aac {}, will use {}\n", jsonfmt::inline(&d["encoders"]["h264"]), jsonfmt::inline(&d["encoders"]["aac"]), d["encoders"]["selected_h264"].as_str().unwrap_or("none")));
            if let Value::Object(map) = d {
                for (k, v) in map {
                    data[k] = v;
                }
            }
        }
        data["ok"] = json!(ok);
        text.push_str(if ok { "ready to render" } else { "NOT ready to render: see above" });
        Ok(Reply { text, image: None, structured: Some(data), failed: false })
    }
}

#[tool_router(router = tool_router)]
impl EditsServer {
    #[tool(
        name = "project_describe",
        description = "Compact text outline of a project: size, length, tempo, slots, and each layer (bottom to top) with its keys and effects. A few hundred tokens: read this before editing.",
        annotations(title = "Describe project", read_only_hint = true, open_world_hint = false)
    )]
    async fn project_describe(&self, Parameters(a): Parameters<ProjectArgs>) -> CallToolResult {
        finish(self.describe(a).await)
    }

    #[tool(
        name = "project_read",
        description = "The project JSON, or the value at a JSON pointer. Layers are addressed by id and effects by kind or index: /layers/@finish, /layers/@a/effects/@exposure/params/stops.",
        annotations(title = "Read project JSON", read_only_hint = true, open_world_hint = false)
    )]
    async fn project_read(&self, Parameters(a): Parameters<ReadArgs>) -> CallToolResult {
        finish(self.read(a).await)
    }

    #[tool(
        name = "project_check",
        description = "Validate a project. Returns errors and warnings, each with a JSON pointer to the place and a hint for the fix (unknown effects and parameters, missing files, undefined slots, bad key times, ignored keys).",
        annotations(title = "Check project", read_only_hint = true, open_world_hint = false)
    )]
    async fn project_check(&self, Parameters(a): Parameters<ProjectArgs>) -> CallToolResult {
        finish(self.check(a).await)
    }

    #[tool(
        name = "project_edit",
        description = "Apply an ordered batch of edits to a project as one step. The result is validated first: with any error nothing is written and the findings are returned. On success returns the changed pointers and warnings; the previous version is kept for project_undo. Ops: set {pointer, value}, remove {pointer}, insert_layer {layer, above|below}, replace_layer {id, layer}, remove_layer {id}, set_slot {name, path, fit, focus}. Times are in beats for a beats project.",
        annotations(title = "Edit project", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn project_edit(&self, Parameters(a): Parameters<EditArgs>) -> CallToolResult {
        finish(self.edit(a).await)
    }

    #[tool(
        name = "project_undo",
        description = "Restore the project to the version before the last project_edit (or reference_draft overwrite). Call again to go further back.",
        annotations(title = "Undo last edit", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn project_undo(&self, Parameters(a): Parameters<ProjectArgs>) -> CallToolResult {
        finish(self.undo(a).await)
    }

    #[tool(
        name = "effects_list",
        description = "The effect registry: every effect kind with its parameters, defaults, units and meaning. Pass `kind` for one effect.",
        annotations(title = "List effects", read_only_hint = true, open_world_hint = false)
    )]
    async fn effects_list(&self, Parameters(a): Parameters<EffectsArgs>) -> CallToolResult {
        finish(self.effects(a).await)
    }

    #[tool(
        name = "render_frame",
        description = "Render one frame and return it as an image, with its frame number and time. Give `at` (project time) or `frame`. Small by default (long edge about 540 px); use it to look closely at one moment.",
        annotations(title = "Render frame", read_only_hint = true, open_world_hint = false)
    )]
    async fn render_frame(&self, Parameters(a): Parameters<FrameArgs>) -> CallToolResult {
        finish(self.frame(a).await)
    }

    #[tool(
        name = "render_sheet",
        description = "Render a contact sheet: a grid of frames over a time range, each labelled with frame number and time, returned as one image. The quickest way to see a whole edit (default 16 frames), or every frame around a hit (`every_frame` with a short range).",
        annotations(title = "Render contact sheet", read_only_hint = true, open_world_hint = false)
    )]
    async fn render_sheet(&self, Parameters(a): Parameters<SheetArgs>) -> CallToolResult {
        finish(self.sheet(a).await)
    }

    #[tool(
        name = "render_video",
        description = "Start rendering the project to an H.264 MP4 (with the project's audio) as a background job. Returns a job id at once; poll job_status for progress and the result.",
        annotations(title = "Render video (background job)", read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn render_video(&self, Parameters(a): Parameters<VideoArgs>) -> CallToolResult {
        finish(self.video(a).await)
    }

    #[tool(
        name = "job_status",
        description = "State of a background job: running (with frames done and time left), completed (with the result), failed (with the error) or cancelled. Without `job`, lists recent jobs.",
        annotations(title = "Job status", read_only_hint = true, open_world_hint = false)
    )]
    async fn job_status_tool(&self, Parameters(a): Parameters<JobStatusArgs>) -> CallToolResult {
        finish(self.job_status(a).await)
    }

    #[tool(
        name = "job_cancel",
        description = "Stop a running background job. A cancelled render's partial output file is removed.",
        annotations(title = "Cancel job", read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn job_cancel_tool(&self, Parameters(a): Parameters<JobArgs>) -> CallToolResult {
        finish(self.job_cancel(a).await)
    }

    #[tool(
        name = "reference_probe",
        description = "Cheap per-frame signature of any video (luminance, saturation, sharpness, frame difference), summarised: ranges, cut and flash candidates, and a compact table. Use it to find where things happen before a full analysis, or to check a render.",
        annotations(title = "Probe video", read_only_hint = true, open_world_hint = false)
    )]
    async fn reference_probe(&self, Parameters(a): Parameters<ProbeArgs>) -> CallToolResult {
        finish(self.probe(a).await)
    }

    #[tool(
        name = "reference_analyze",
        description = "Start extracting the recipe of a finished edit (tempo, loop, cuts, flashes, tone hits, blur, motion curves, overlay, vignette) as a background job. Returns a job id; job_status gives the path of reference_analysis.json and a summary when done.",
        annotations(title = "Analyze reference (background job)", read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn reference_analyze(&self, Parameters(a): Parameters<AnalyzeArgs>) -> CallToolResult {
        finish(self.analyze(a).await)
    }

    #[tool(
        name = "reference_read",
        description = "Read one section of a reference analysis instead of the whole file: summary (default), grid (tempo and beat grid), shots (one line each), shot (everything about one shot: curves, events, notes), slots, layers (overlay, vignette), signature. Values marked `measured` can be trusted; `needs_agent` and `qualitative` mean look at the reference yourself.",
        annotations(title = "Read reference analysis", read_only_hint = true, open_world_hint = false)
    )]
    async fn reference_read_tool(&self, Parameters(a): Parameters<ReferenceReadArgs>) -> CallToolResult {
        finish(self.reference_read(a).await)
    }

    #[tool(
        name = "reference_draft",
        description = "Write a project that replays an analysed recipe: one slot per picture, cuts on the beat grid, measured flashes, tone hits, blur and motion as keyframes. Returns the outline and check findings. Then set the slots to your own pictures.",
        annotations(title = "Draft project from analysis", read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn reference_draft(&self, Parameters(a): Parameters<DraftArgs>) -> CallToolResult {
        finish(self.draft(a).await)
    }

    #[tool(
        name = "reference_compare",
        description = "Compare a video (usually a render of your project) with a reference analysis in signature space: timing of cuts, flashes and tone hits, and exposure, blur, saturation and zoom curves per shot. Returns scores and the largest differences in plain language, most severe first. Works when the pictures differ; pixels are not compared. Long videos continue as a job (poll job_status).",
        annotations(title = "Compare with reference", read_only_hint = true, open_world_hint = false)
    )]
    async fn reference_compare(&self, Parameters(a): Parameters<CompareArgs>) -> CallToolResult {
        finish(self.compare(a).await)
    }

    #[tool(
        name = "project_fit",
        description = "Start tuning the project's effect values (flash strength and decay, saturation, threshold level, blur, zoom and move amounts and their eases) so that its render matches a reference analysis in signature space, as a background job. Key times, pictures, rotation and inversions are never changed. Works with any pictures in the slots. Poll job_status; the result lists every value changed and the scores before and after.",
        annotations(title = "Fit project to reference (background job)", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn project_fit(&self, Parameters(a): Parameters<FitArgs>) -> CallToolResult {
        finish(self.fit(a).await)
    }

    #[tool(
        name = "audio_analyze",
        description = "Analyse a song (or a video's soundtrack) for cutting: tempo and beat grid, bars, onsets in three bands (kick, snare, hat) with strengths, energy per beat, sections and the drop. Writes a JSON file to pass as `audio` to beats_cut, beats_accent and preset_apply; with `project`, also writes the tempo and markers (drop, sections) into the project.",
        annotations(title = "Analyse song", read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn audio_analyze_tool(&self, Parameters(a): Parameters<AudioAnalyzeArgs>) -> CallToolResult {
        finish(self.audio_analyze(a).await)
    }

    #[tool(
        name = "presets_list",
        description = "The preset library: named building blocks (flash, punch, shake, rgb_split, blur_in, slide_in, flip_3d, cube_turn, velocity, echo_hit, glow_pulse, letterbox, sakura, sparkles, dust, beat_burst, iris_in, split_panels, the text animations typewriter, pop_in, random_flicker and wave, ... and transition:<kind>) with their typed variables and defaults. Pass `name` for one preset's variables in full.",
        annotations(title = "List presets", read_only_hint = true, open_world_hint = false)
    )]
    async fn presets_list_tool(&self, Parameters(a): Parameters<PresetsArgs>) -> CallToolResult {
        finish(self.presets_list(a).await)
    }

    #[tool(
        name = "preset_apply",
        description = "Apply a preset to a layer at one or more times, as one validated, undoable edit. It expands into ordinary keys, effects and layers that stay editable. `at` takes beats, bars and markers (\"b8\", \"bar 3\", \"drop\"); for transition:<kind>, give the incoming layer and it overlaps the cut.",
        annotations(title = "Apply preset", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn preset_apply_tool(&self, Parameters(a): Parameters<PresetApplyArgs>) -> CallToolResult {
        finish(self.preset_apply(a).await)
    }

    #[tool(
        name = "beats_cut",
        description = "Cut pictures to the music in one step: one image layer per beat (or every N beats, or on each kick or snare of a song analysis) between two times, cycling through slots. One undoable edit; the project grows if the range runs past its end.",
        annotations(title = "Cut to the beat", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn beats_cut_tool(&self, Parameters(a): Parameters<BeatsCutArgs>) -> CallToolResult {
        finish(self.beats_cut(a).await)
    }

    #[tool(
        name = "beats_accent",
        description = "Apply a preset on every beat, offbeat, downbeat, kick, snare or hat, or on the drop (onsets come from a song analysis), on the picture on screen at each time or on one adjustment layer. One undoable edit.",
        annotations(title = "Accent beats", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn beats_accent_tool(&self, Parameters(a): Parameters<BeatsAccentArgs>) -> CallToolResult {
        finish(self.beats_accent(a).await)
    }

    #[tool(
        name = "effect_define",
        description = "Write a custom WGSL effect next to the project (shaders/<name>.wgsl) after checking it against the custom-effect contract (errors come back with line numbers and nothing is written), then add it as a `custom` effect to a layer, or to a new adjustment layer on top. One undoable edit.",
        annotations(title = "Define custom effect", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn effect_define_tool(&self, Parameters(a): Parameters<EffectDefineArgs>) -> CallToolResult {
        finish(self.effect_define(a).await)
    }

    #[tool(
        name = "ui_context",
        description = "What the person is looking at in the preview window (edits-gui) for this project: the selected layer and property, the playhead, the visible range and loop, and the note they typed for you; or that the window is not open.",
        annotations(title = "Window context", read_only_hint = true, open_world_hint = false)
    )]
    async fn ui_context_tool(&self, Parameters(a): Parameters<ProjectArgs>) -> CallToolResult {
        finish(self.ui_context(&a.project).await)
    }

    #[tool(
        name = "doctor",
        description = "What this machine renders with: workspace root, the edits executables, graphics adapter and backend (and whether it is a software rasteriser), ffmpeg and its H.264 and AAC encoders.",
        annotations(title = "Doctor", read_only_hint = true, open_world_hint = false)
    )]
    async fn doctor(&self, Parameters(_): Parameters<NoArgs>) -> CallToolResult {
        finish(self.doctor_report().await)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EditsServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("edits-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}
