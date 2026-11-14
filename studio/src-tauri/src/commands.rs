//! The Tauri commands the frontend calls. Each is a thin wrapper: the
//! logic is in `studio`, `project`, `inspect`, `engine::editing`,
//! `renders` and `reference`, which are unit tested without a window.

use crate::engine::editing::{self, DragKind, NewLayer, TimeDrag};
use crate::engine::timemap::TimeMap;
use crate::inspect::{self, Inspection, PropKey};
use crate::project::Snapshot;
use crate::reference;
use crate::render::{FrameKey, RenderStatus};
use crate::renders::{self, JobStatus, RenderJob};
use crate::studio::{EditDone, Studio};
use crate::uictx::{self, UiInputs};
use crate::Startup;
use edits_core::edit::{layer_pointer, EditOp};
use edits_core::preset::{self, Target, TargetKind, VarType};
use edits_core::{effects, Project};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::ipc::Response;
use tauri::State;

type S<'a> = State<'a, Arc<Studio>>;

// ---- project ---------------------------------------------------------------------

#[tauri::command]
pub fn startup(startup: State<'_, Startup>) -> Startup {
    startup.inner().clone()
}

#[tauri::command]
pub fn open_project(studio: S<'_>, path: String) -> Result<Snapshot, String> {
    studio.open_project(Path::new(&path))
}

#[tauri::command]
pub fn snapshot(studio: S<'_>) -> Option<Snapshot> {
    studio.snapshot()
}

#[tauri::command]
pub fn render_status(studio: S<'_>) -> RenderStatus {
    studio.render.status()
}

// ---- frames ----------------------------------------------------------------------

/// The frame as JPEG bytes. `prefetch` frames wait behind the current one.
#[tauri::command]
pub async fn frame(
    studio: S<'_>,
    revision: u64,
    frame: u64,
    scale: f32,
    blur: bool,
    prefetch: bool,
) -> Result<Response, String> {
    let rx = studio
        .render
        .request(FrameKey::new(revision, frame, scale, blur), prefetch);
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        rx.recv().map_err(|_| "the renderer stopped".to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    let bytes = bytes?;
    Ok(Response::new(bytes.to_vec()))
}

#[tauri::command]
pub fn prefetch_clear(studio: S<'_>) {
    studio.render.clear_prefetch();
}

// ---- inspector -------------------------------------------------------------------

#[tauri::command]
pub fn inspect(studio: S<'_>, layer: Option<String>, t: f64) -> Result<Inspection, String> {
    let g = studio.open.lock().map_err(|e| e.to_string())?;
    let p = g
        .as_ref()
        .and_then(|o| o.project())
        .ok_or("no project is open")?;
    inspect::inspect(p, layer.as_deref(), t)
}

// ---- edits -----------------------------------------------------------------------

#[tauri::command]
pub fn edit_ops(
    studio: S<'_>,
    ops: Vec<EditOp>,
    label: Option<String>,
) -> Result<EditDone, String> {
    studio.edit(label.as_deref(), |_, _| Ok(ops))
}

fn layer_of<'a>(
    p: &'a Project,
    layer: Option<&str>,
) -> Result<Option<&'a edits_core::Layer>, String> {
    match layer {
        None => Ok(None),
        Some(id) => p
            .layers
            .iter()
            .find(|l| l.id == id)
            .map(Some)
            .ok_or_else(|| format!("there is no layer \"{id}\" any more")),
    }
}

fn fmt_value(v: &[f64]) -> String {
    match v {
        [one] => format!("{}", editing::tidy(*one)),
        many => format!(
            "[{}]",
            many.iter()
                .map(|x| editing::tidy(*x).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Set a property to `value` at project time `t`: a constant changes; a
/// keyframed property gets (or changes) a key at `t`.
#[tauri::command]
pub fn prop_set(
    studio: S<'_>,
    layer: Option<String>,
    key: PropKey,
    t: f64,
    value: Vec<f64>,
) -> Result<EditDone, String> {
    let r = key.to_ref()?;
    let id = layer.clone().unwrap_or_default();
    let label = format!(
        "{}: {} → {}",
        if layer.is_some() {
            id.as_str()
        } else {
            "camera"
        },
        r.label(),
        fmt_value(&value)
    );
    studio.edit(Some(&label), |p, _| {
        let l = layer_of(p, layer.as_deref())?;
        let new = editing::with_value(
            r.get(p, l),
            t,
            value.iter().map(|x| editing::tidy(*x)).collect(),
        );
        Ok(vec![editing::set_prop(&id, &r, &new)])
    })
}

#[tauri::command]
pub fn prop_key_here(
    studio: S<'_>,
    layer: Option<String>,
    key: PropKey,
    t: f64,
) -> Result<EditDone, String> {
    let r = key.to_ref()?;
    let id = layer.clone().unwrap_or_default();
    let label = format!("key {} at {}", r.label(), editing::tidy(t));
    studio.edit(Some(&label), |p, _| {
        let l = layer_of(p, layer.as_deref())?;
        let new = editing::with_key_here(p, r.get(p, l), &r.default_value(), t)
            .ok_or("there is a key here already")?;
        Ok(vec![editing::set_prop(&id, &r, &new)])
    })
}

#[tauri::command]
pub fn prop_remove_key(
    studio: S<'_>,
    layer: Option<String>,
    key: PropKey,
    t: f64,
) -> Result<EditDone, String> {
    let r = key.to_ref()?;
    let id = layer.clone().unwrap_or_default();
    let label = format!("remove the key of {} at {}", r.label(), editing::tidy(t));
    studio.edit(Some(&label), |p, _| {
        let l = layer_of(p, layer.as_deref())?;
        let new = editing::without_key(r.get(p, l), t).ok_or("there is no key at the playhead")?;
        Ok(vec![editing::set_prop(&id, &r, &new)])
    })
}

#[tauri::command]
pub fn prop_ease(
    studio: S<'_>,
    layer: Option<String>,
    key: PropKey,
    index: usize,
    ease: String,
) -> Result<EditDone, String> {
    let r = key.to_ref()?;
    let id = layer.clone().unwrap_or_default();
    let label = format!("ease of {} key {index}: {ease}", r.label());
    studio.edit(Some(&label), |p, _| {
        let l = layer_of(p, layer.as_deref())?;
        let prop = r.get(p, l);
        let current = editing::keys_of(prop)
            .get(index)
            .map(|k| k.ease.clone())
            .ok_or("no such key")?;
        let new = editing::with_ease(prop, index, editing::ease_from_choice(&ease, &current))
            .ok_or("no such key")?;
        Ok(vec![editing::set_prop(&id, &r, &new)])
    })
}

#[tauri::command]
pub fn layer_add(
    studio: S<'_>,
    kind: String,
    slot: Option<String>,
    at: f64,
    above: Option<String>,
) -> Result<EditDone, String> {
    let new = match (kind.as_str(), slot) {
        ("solid", _) => NewLayer::Solid,
        ("adjustment", _) => NewLayer::Adjustment,
        ("image", Some(s)) => NewLayer::Image(s),
        ("cube", Some(s)) => NewLayer::Cube(s),
        (k, _) => return Err(format!("cannot add a layer of kind {k} here (solid, adjustment, image with a slot, cube with a slot)")),
    };
    studio.edit(Some(&format!("add a {kind} layer")), |p, _| {
        let tm = TimeMap::of(p);
        let at = editing::snap(&tm, at.floor().max(0.0), false);
        let layer = editing::new_layer(p, &new, at, editing::new_layer_length(&tm));
        Ok(vec![EditOp::insert_layer(layer, above, None)])
    })
}

#[tauri::command]
pub fn layer_duplicate(studio: S<'_>, id: String) -> Result<EditDone, String> {
    studio.edit(Some(&format!("duplicate {id}")), |p, doc| {
        editing::duplicate_ops(p, Some(doc), &id)
            .map(|(ops, _)| ops)
            .ok_or_else(|| format!("no layer {id}"))
    })
}

#[tauri::command]
pub fn layer_remove(studio: S<'_>, id: String) -> Result<EditDone, String> {
    studio.edit(Some(&format!("delete {id}")), |_, _| {
        Ok(vec![EditOp::remove_layer(id.clone())])
    })
}

#[tauri::command]
pub fn layer_toggle(studio: S<'_>, id: String) -> Result<EditDone, String> {
    studio.edit(None, |p, _| {
        let l = layer_of(p, Some(&id))?.ok_or("no layer")?;
        Ok(vec![editing::set_field(&id, "enabled", json!(!l.enabled))])
    })
}

#[tauri::command]
pub fn layer_move(studio: S<'_>, id: String, up: bool) -> Result<EditDone, String> {
    let label = format!("move {id} {}", if up { "up" } else { "down" });
    studio.edit(Some(&label), |p, doc| {
        editing::move_layer_ops(p, Some(doc), &id, up).ok_or_else(|| {
            format!(
                "{id} is already at the {}",
                if up { "top" } else { "bottom" }
            )
        })
    })
}

/// Move a layer to `index` in the file's order (0 = bottom of the picture).
#[tauri::command]
pub fn layer_reorder(studio: S<'_>, id: String, index: usize) -> Result<EditDone, String> {
    studio.edit(
        Some(&format!("move {id} to place {}", index + 1)),
        |p, doc| reorder_ops(p, doc, &id, index),
    )
}

pub fn reorder_ops(
    p: &Project,
    doc: &Value,
    id: &str,
    index: usize,
) -> Result<Vec<EditOp>, String> {
    let from = p
        .layers
        .iter()
        .position(|l| l.id == id)
        .ok_or_else(|| format!("no layer {id}"))?;
    let index = index.min(p.layers.len() - 1);
    if from == index {
        return Ok(Vec::new());
    }
    let others: Vec<&str> = p
        .layers
        .iter()
        .filter(|l| l.id != id)
        .map(|l| l.id.as_str())
        .collect();
    let layer = editing::layer_json(Some(doc), &p.layers[from]);
    let insert = if index == 0 {
        EditOp::insert_layer(layer, None, Some(others[0].to_string()))
    } else {
        EditOp::insert_layer(layer, Some(others[index - 1].to_string()), None)
    };
    Ok(vec![EditOp::remove_layer(id), insert])
}

/// A drag on the timeline, released: `kind` is `move`, `trim_in`, `trim_out` or `keys` (with `from`, the keys' time).
#[tauri::command]
pub fn layer_drag(
    studio: S<'_>,
    layer: String,
    kind: String,
    from: Option<f64>,
    grab: f64,
    now: f64,
    alt: bool,
) -> Result<EditDone, String> {
    let kind = match kind.as_str() {
        "move" => DragKind::Move,
        "trim_in" => DragKind::TrimIn,
        "trim_out" => DragKind::TrimOut,
        "keys" => DragKind::Keys {
            from: from.ok_or("a key drag needs `from`")?,
        },
        k => return Err(format!("unknown drag {k}")),
    };
    let mut drag = TimeDrag::new(layer, kind, grab, alt);
    drag.update(now, alt);
    studio.edit_labelled(move |p, _| {
        let (ops, label) = drag.ops(p, &TimeMap::of(p));
        Ok((ops, Some(label)))
    })
}

#[tauri::command]
pub fn effect_add(studio: S<'_>, layer: String, kind: String) -> Result<EditDone, String> {
    if effects::spec(&kind).is_none() {
        return Err(format!("unknown effect {kind}"));
    }
    studio.edit(Some(&format!("add {kind} to {layer}")), |p, _| {
        Ok(vec![editing::add_effect_op(
            layer_of(p, Some(&layer))?.ok_or("no layer")?,
            &kind,
        )])
    })
}

#[tauri::command]
pub fn effect_remove(studio: S<'_>, layer: String, index: usize) -> Result<EditDone, String> {
    studio.edit(None, |p, _| {
        Ok(vec![editing::remove_effect_op(
            layer_of(p, Some(&layer))?.ok_or("no layer")?,
            index,
        )])
    })
}

#[tauri::command]
pub fn effect_toggle(studio: S<'_>, layer: String, index: usize) -> Result<EditDone, String> {
    studio.edit(None, |p, _| {
        editing::toggle_effect_op(layer_of(p, Some(&layer))?.ok_or("no layer")?, index)
            .map(|o| vec![o])
            .ok_or_else(|| "no such effect".into())
    })
}

#[tauri::command]
pub fn effect_move(
    studio: S<'_>,
    layer: String,
    index: usize,
    up: bool,
) -> Result<EditDone, String> {
    studio.edit(None, |p, doc| {
        editing::move_effect_op(
            Some(doc),
            layer_of(p, Some(&layer))?.ok_or("no layer")?,
            index,
            up,
        )
        .map(|o| vec![o])
        .ok_or_else(|| "it is already at that end".into())
    })
}

#[tauri::command]
pub fn effect_option(
    studio: S<'_>,
    layer: String,
    index: usize,
    name: String,
    value: String,
) -> Result<EditDone, String> {
    let label = format!("{layer}: effect {index} {name} = {value}");
    studio.edit(Some(&label), |_, _| {
        Ok(vec![EditOp::set(
            format!(
                "{}/effects/{index}/options/{}",
                layer_pointer(&layer),
                edits_core::pointer::escape(&name)
            ),
            json!(value),
        )])
    })
}

#[tauri::command]
pub fn undo(studio: S<'_>) -> Result<EditDone, String> {
    studio.step(false)
}

#[tauri::command]
pub fn redo(studio: S<'_>) -> Result<EditDone, String> {
    studio.step(true)
}

#[tauri::command]
pub fn restore(studio: S<'_>, file: String) -> Result<EditDone, String> {
    studio.restore(Path::new(&file))
}

// ---- the libraries -----------------------------------------------------------------

#[derive(Serialize)]
pub struct ParamDto {
    name: &'static str,
    default: &'static [f64],
    unit: &'static str,
    range: Option<(f64, f64)>,
    doc: &'static str,
}

#[derive(Serialize)]
pub struct EffectSpecDto {
    kind: &'static str,
    category: &'static str,
    doc: &'static str,
    params: Vec<ParamDto>,
    options: Vec<(&'static str, Vec<&'static str>)>,
    reads_time: bool,
    credit: &'static str,
    /// Needs a file (`custom` shaders, `lut` tables) and cannot be added with defaults alone.
    needs_source: bool,
}

/// The effect registry (`edits effects`).
#[tauri::command]
pub fn effects_list() -> Vec<EffectSpecDto> {
    effects::EFFECTS
        .iter()
        .map(|e| EffectSpecDto {
            kind: e.kind,
            category: e.category,
            doc: e.doc,
            params: e
                .params
                .iter()
                .map(|p| ParamDto {
                    name: p.name,
                    default: p.default,
                    unit: p.unit.as_str(),
                    range: p.range,
                    doc: p.doc,
                })
                .collect(),
            options: e
                .options
                .iter()
                .map(|o| (o.name, o.values.to_vec()))
                .collect(),
            reads_time: e.reads_time,
            credit: e.credit,
            needs_source: !matches!(
                e.source,
                effects::SourceKind::None | effects::SourceKind::OptionalSlot
            ),
        })
        .collect()
}

#[derive(Serialize)]
pub struct VariableDto {
    name: String,
    #[serde(rename = "type")]
    ty: &'static str,
    default: Value,
    min: Option<f64>,
    max: Option<f64>,
    unit: Option<String>,
    doc: String,
    options: Vec<String>,
}

#[derive(Serialize)]
pub struct PresetDto {
    name: String,
    description: String,
    tags: Vec<String>,
    group: String,
    /// `layer` or `project` (adds its own layers).
    target: &'static str,
    transition: bool,
    variables: Vec<VariableDto>,
    credit: Option<String>,
    /// `builtin`, `transition` or `project`.
    origin: &'static str,
}

fn var_type(t: VarType) -> &'static str {
    match t {
        VarType::Number => "number",
        VarType::Integer => "integer",
        VarType::Boolean => "boolean",
        VarType::Color => "color",
        VarType::Vector => "vector",
        VarType::Enum => "enum",
        VarType::String => "string",
    }
}

/// Every preset the project can use: built-in, one per transition, and the project's `presets/`.
#[tauri::command]
pub fn presets_list(studio: S<'_>) -> Vec<PresetDto> {
    let dir = studio.path().map(|p| edits_core::edit::project_dir(&p));
    let builtin: Vec<String> = preset::builtin().into_iter().map(|p| p.name).collect();
    let (list, _) = preset::library(dir.as_deref());
    list.into_iter()
        .map(|p| {
            let transition = p.name.starts_with("transition:");
            PresetDto {
                group: if transition {
                    "transition".into()
                } else {
                    p.tags.first().cloned().unwrap_or_else(|| "other".into())
                },
                origin: if transition {
                    "transition"
                } else if builtin.contains(&p.name) {
                    "builtin"
                } else {
                    "project"
                },
                target: if p.target == TargetKind::Project {
                    "project"
                } else {
                    "layer"
                },
                variables: p
                    .variables
                    .iter()
                    .map(|(n, v)| VariableDto {
                        name: n.clone(),
                        ty: var_type(v.ty),
                        default: v.default.clone(),
                        min: v.min,
                        max: v.max,
                        unit: v.unit.clone(),
                        doc: v.doc.clone(),
                        options: v
                            .options
                            .as_ref()
                            .map(|o| o.keys().cloned().collect())
                            .unwrap_or_default(),
                    })
                    .collect(),
                transition,
                name: p.name,
                description: p.description,
                tags: p.tags,
                credit: p.credit,
            }
        })
        .collect()
}

#[derive(Deserialize, Debug)]
pub struct PresetRequest {
    pub name: String,
    /// `selected`, `auto` (the picture on screen) or `fx` (an `accents` layer on top).
    pub target: String,
    pub selected: Option<String>,
    /// `playhead` or `cut` (the target layer's `in`).
    pub at: String,
    pub playhead: f64,
    #[serde(default)]
    pub values: Map<String, Value>,
}

pub fn preset_change(
    req: &PresetRequest,
    project_dir: Option<&Path>,
    doc: &Value,
) -> Result<(Value, String), String> {
    let p = preset::find(&req.name, project_dir)?;
    let target = if p.target == TargetKind::Project {
        Target::None
    } else {
        match (req.target.as_str(), req.selected.as_deref()) {
            ("selected", Some(id)) => Target::Layer(id.to_string()),
            ("fx", _) => Target::Fx,
            _ => Target::Auto,
        }
    };
    let at = match (req.at.as_str(), &target) {
        ("cut", Target::Layer(id)) => doc
            .get("layers")
            .and_then(Value::as_array)
            .and_then(|ls| {
                ls.iter()
                    .find(|l| l.get("id").and_then(Value::as_str) == Some(id.as_str()))
            })
            .and_then(|l| l.get("in").and_then(Value::as_f64))
            .unwrap_or(req.playhead),
        _ => req.playhead,
    };
    let given: Map<String, Value> = req
        .values
        .iter()
        .filter(|(n, v)| p.variables.get(*n).is_some_and(|var| &var.default != *v))
        .map(|(n, v)| (n.clone(), v.clone()))
        .collect();
    let tc = edits_core::time::TimeCtx::from_doc(doc);
    let (out, _) = preset::apply(doc, &p, &target, &[at], &given, &tc)?;
    let on = match &target {
        Target::Layer(id) => id.clone(),
        Target::Auto => "auto".into(),
        Target::Fx => "fx".into(),
        Target::None => "project".into(),
    };
    Ok((
        out,
        format!("preset {} at {} on {on}", p.name, editing::tidy(at)),
    ))
}

#[tauri::command]
pub fn preset_apply(studio: S<'_>, request: PresetRequest) -> Result<EditDone, String> {
    let path = studio.path().ok_or("no project is open")?;
    let dir = edits_core::edit::project_dir(&path);
    let doc = studio
        .snapshot()
        .and_then(|s| s.doc)
        .ok_or("the project has not loaded")?;
    let (_, label) = preset_change(&request, Some(&dir), &doc)?;
    studio.edit_doc(&label, &|current| {
        preset_change(&request, Some(&dir), current).map(|(d, _)| d)
    })
}

// ---- what the agent sees -------------------------------------------------------------

#[tauri::command]
pub fn ui_report(studio: S<'_>, inputs: UiInputs) -> Option<String> {
    let name = studio
        .path()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))?;
    let mut ui = studio.ui.lock().ok()?;
    let f = ui.as_mut()?;
    f.set(uictx::context(&name, &inputs));
    f.tick(std::time::Instant::now());
    f.error.clone()
}

// ---- renders ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct RendersDto {
    outputs: Vec<renders::Output>,
    default_output: Option<String>,
    job: Option<JobStatus>,
    edits: Option<String>,
}

#[tauri::command]
pub fn renders_list(studio: S<'_>) -> RendersDto {
    let path = studio.path();
    RendersDto {
        outputs: path.as_deref().map(renders::outputs).unwrap_or_default(),
        default_output: path
            .as_deref()
            .map(|p| renders::default_output(p).display().to_string()),
        job: studio.job.lock().ok().and_then(|j| {
            j.as_ref()
                .and_then(|j| j.status.lock().ok().map(|s| s.clone()))
        }),
        edits: renders::locate("edits").map(|p| p.display().to_string()),
    }
}

#[tauri::command]
pub fn render_start(
    studio: S<'_>,
    app: tauri::AppHandle,
    output: Option<String>,
    scale: Option<f64>,
) -> Result<JobStatus, String> {
    use tauri::Emitter;
    let project = studio.path().ok_or("no project is open")?;
    let mut job = studio.job.lock().map_err(|e| e.to_string())?;
    if job.as_ref().is_some_and(RenderJob::running) {
        return Err("a render is running already; cancel it first".into());
    }
    let edits = renders::locate("edits").ok_or("`edits` was not found next to edits-studio or on PATH; build it with `cargo build --release -p edits-cli`, or set EDITS_BIN")?;
    let output = output
        .filter(|o| !o.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| renders::default_output(&project));
    let output = if output.is_relative() {
        edits_core::edit::project_dir(&project).join(output)
    } else {
        output
    };
    let new = RenderJob::start(&edits, &project, &output, scale, move |s| {
        let _ = app.emit("render-job", s);
    })?;
    let status = new.status.lock().map(|s| s.clone()).unwrap_or_default();
    *job = Some(new);
    Ok(status)
}

#[tauri::command]
pub fn render_cancel(studio: S<'_>) {
    if let Ok(mut j) = studio.job.lock() {
        if let Some(j) = j.as_mut() {
            j.cancel();
        }
    }
}

// ---- reference ---------------------------------------------------------------------------

#[tauri::command]
pub fn analyses_find(studio: S<'_>) -> Vec<String> {
    studio
        .path()
        .map(|p| reference::find_analyses(&edits_core::edit::project_dir(&p), 3))
        .unwrap_or_default()
}

#[tauri::command]
pub fn analysis_load(path: String) -> Result<reference::AnalysisDto, String> {
    reference::load_analysis(Path::new(&path))
}

#[tauri::command]
pub fn reports_list(studio: S<'_>) -> Vec<reference::ReportBrief> {
    studio
        .path()
        .map(|p| reference::list_reports(&p))
        .unwrap_or_default()
}

#[tauri::command]
pub fn report_load(path: String) -> Result<reference::Report, String> {
    reference::load_report(Path::new(&path))
}

#[tauri::command]
pub fn compare_status(studio: S<'_>) -> reference::CompareRun {
    studio.compare.lock().map(|c| c.clone()).unwrap_or_default()
}

/// Compare a render with the analysis in the background; the `compare` event says when it is done.
#[tauri::command]
pub fn compare_start(
    studio: S<'_>,
    app: tauri::AppHandle,
    analysis: String,
    video: String,
) -> Result<(), String> {
    use tauri::Emitter;
    let project = studio.path().ok_or("no project is open")?;
    {
        let mut c = studio.compare.lock().map_err(|e| e.to_string())?;
        if c.running {
            return Err("a comparison is running already".into());
        }
        *c = reference::CompareRun {
            running: true,
            video: Some(video.clone()),
            error: None,
            report: None,
        };
    }
    let s = studio.inner().clone();
    std::thread::Builder::new()
        .name("edits-compare".into())
        .spawn(move || {
            let result = reference::run_compare(Path::new(&analysis), Path::new(&video))
                .and_then(|(c, sig)| reference::save_report(&project, &c, &sig));
            let state = {
                let mut c = s.compare.lock().unwrap_or_else(|e| e.into_inner());
                c.running = false;
                match result {
                    Ok(p) => c.report = Some(p.display().to_string()),
                    Err(e) => c.error = Some(e),
                }
                c.clone()
            };
            let _ = app.emit("compare", state);
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (Project, Value) {
        let text = crate::project::tests::PROJECT;
        (
            Project::from_json(text).unwrap(),
            serde_json::from_str(text).unwrap(),
        )
    }

    fn order(doc: &Value) -> Vec<String> {
        doc["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn a_layer_is_moved_to_any_place_in_the_stack() {
        let (p, doc) = project();
        for (id, index, want) in [
            ("finish", 0, ["finish", "a", "b"]),
            ("a", 2, ["b", "finish", "a"]),
            ("a", 1, ["b", "a", "finish"]),
        ] {
            let ops = reorder_ops(&p, &doc, id, index).unwrap();
            let out = edits_core::edit::apply_ops(&doc, &ops).unwrap();
            assert_eq!(order(&out), want, "{id} to {index}");
        }
        assert!(reorder_ops(&p, &doc, "a", 0).unwrap().is_empty());
    }

    #[test]
    fn a_preset_is_applied_on_the_selected_layer_at_the_playhead_or_its_cut() {
        let (_, doc) = project();
        let req = PresetRequest {
            name: "flash".into(),
            target: "selected".into(),
            selected: Some("b".into()),
            at: "cut".into(),
            playhead: 1.0,
            values: Map::new(),
        };
        let (out, label) = preset_change(&req, None, &doc).unwrap();
        assert_eq!(label, "preset flash at 4 on b");
        assert_ne!(out["layers"][1], doc["layers"][1], "layer b changed");
        let fx = PresetRequest {
            target: "fx".into(),
            at: "playhead".into(),
            ..req
        };
        let (out, label) = preset_change(&fx, None, &doc).unwrap();
        assert!(label.ends_with("at 1 on fx"), "{label}");
        assert!(order(&out).contains(&"accents".to_string()));
    }

    #[test]
    fn the_registry_lists_every_effect_with_units() {
        let list = effects_list();
        assert_eq!(list.len(), effects::EFFECTS.len());
        let glow = list.iter().find(|e| e.kind == "glow").unwrap();
        assert!(glow
            .params
            .iter()
            .any(|p| p.name == "radius" && p.unit == "%h"));
        assert!(
            list.iter()
                .find(|e| e.kind == "custom")
                .unwrap()
                .needs_source
        );
    }
}
