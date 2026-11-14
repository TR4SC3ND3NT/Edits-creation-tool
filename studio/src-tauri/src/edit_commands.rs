//! The Tauri commands of the Edit page's editor: live previews while
//! dragging, keyframe lanes, split, ripple delete, moving several layers,
//! new text / shape / particle layers, pictures, browser thumbnails, the
//! song (analysis, waveform, beat cut, accent) and new projects. Thin
//! wrappers: the logic is in `editor`, `song` and `thumbs`.

use crate::editor;
use crate::engine::editing;
use crate::engine::timemap::TimeMap;
use crate::inspect::PropKey;
use crate::song::{self, AccentRequest, CutRequest, SongDto, SongService, Waveform};
use crate::studio::{EditDone, Studio};
use crate::thumbs::Thumbs;
use edits_core::edit::EditOp;
use edits_core::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::ipc::Response;
use tauri::State;

type S<'a> = State<'a, Arc<Studio>>;

/// The project shown, its JSON and its folder.
fn current(studio: &Studio) -> Result<(Arc<Project>, Value, PathBuf), String> {
    let g = studio.open.lock().map_err(|e| e.to_string())?;
    let o = g.as_ref().ok_or("no project is open")?;
    match (o.project(), o.doc()) {
        (Some(p), Some(d)) => Ok((p.clone(), d.clone(), o.path.clone())),
        _ => Err("the project has not loaded".into()),
    }
}

fn layer_of<'a>(p: &'a Project, layer: Option<&str>) -> Result<Option<&'a edits_core::Layer>, String> {
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

async fn render_preview(studio: &Studio, doc: Value, frame: u64, scale: f32) -> Result<Response, String> {
    let p = Project::from_json(&doc.to_string()).map_err(|e| format!("{e:#}"))?;
    let rx = studio.render.preview(Arc::new(p), frame, scale);
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        rx.recv().map_err(|_| "the renderer stopped".to_string())
    })
    .await
    .map_err(|e| e.to_string())???;
    Ok(Response::new(bytes.to_vec()))
}

/// The frame as it would look with `ops` applied (nothing is written).
#[tauri::command]
pub async fn preview_ops(studio: S<'_>, ops: Vec<EditOp>, frame: u64, scale: f32) -> Result<Response, String> {
    let (_, doc, _) = current(&studio)?;
    let doc = edits_core::edit::apply_ops(&doc, &ops)?;
    render_preview(&studio, doc, frame, scale).await
}

/// The frame as it would look with a property set to `value` at `t`, as `prop_set` would write it.
#[tauri::command]
pub async fn preview_prop(
    studio: S<'_>,
    layer: Option<String>,
    key: PropKey,
    t: f64,
    value: Vec<f64>,
    frame: u64,
    scale: f32,
) -> Result<Response, String> {
    let r = key.to_ref()?;
    let (p, doc, _) = current(&studio)?;
    let l = layer_of(&p, layer.as_deref())?;
    let new = editing::with_value(r.get(&p, l), t, value);
    let op = editing::set_prop(layer.as_deref().unwrap_or_default(), &r, &new);
    let doc = edits_core::edit::apply_ops(&doc, &[op])?;
    render_preview(&studio, doc, frame, scale).await
}

/// Move one key of one property from `from` to `to`.
#[tauri::command]
pub fn key_move(studio: S<'_>, layer: Option<String>, key: PropKey, from: f64, to: f64) -> Result<EditDone, String> {
    let r = key.to_ref()?;
    let label = format!(
        "{}: key of {} from {} to {}",
        layer.as_deref().unwrap_or("camera"),
        r.label(),
        editing::tidy(from),
        editing::tidy(to)
    );
    studio.edit(Some(&label), |p, _| editor::key_move_ops(p, layer.as_deref(), &r, from, to))
}

#[tauri::command]
pub fn layer_split(studio: S<'_>, id: String, t: f64) -> Result<EditDone, String> {
    studio.edit_labelled(|p, doc| editor::split_ops(p, doc, &id, t).map(|(o, l)| (o, Some(l))))
}

#[tauri::command]
pub fn layers_remove(studio: S<'_>, ids: Vec<String>, ripple: bool) -> Result<EditDone, String> {
    studio.edit_labelled(|p, _| editor::remove_ops(p, &ids, ripple).map(|(o, l)| (o, Some(l))))
}

#[tauri::command]
pub fn layers_shift(studio: S<'_>, ids: Vec<String>, dt: f64) -> Result<EditDone, String> {
    studio.edit_labelled(|p, _| editor::shift_ops(p, &ids, dt).map(|(o, l)| (o, Some(l))))
}

/// A new text, shape (`shape:rect`, ...) or particles layer at `at`, one bar long, above `above`.
#[tauri::command]
pub fn layer_new(studio: S<'_>, kind: String, at: f64, above: Option<String>) -> Result<EditDone, String> {
    studio.edit(Some(&format!("add a {kind} layer")), |p, _| {
        let tm = TimeMap::of(p);
        let at = editing::snap(&tm, at.max(0.0), false);
        let layer = editor::new_layer(p, &kind, at, editing::new_layer_length(&tm))?;
        Ok(vec![EditOp::insert_layer(layer, above, None)])
    })
}

/// Add a picture file as a slot, and as an image layer at `at` when `layer`.
#[tauri::command]
pub fn picture_add(studio: S<'_>, path: String, at: f64, layer: bool, above: Option<String>) -> Result<EditDone, String> {
    let file = PathBuf::from(&path);
    if !file.is_file() {
        return Err(format!("no file {path}"));
    }
    let dir = edits_core::edit::project_dir(&studio.path().unwrap_or_default());
    studio.edit_labelled(|p, _| {
        let tm = TimeMap::of(p);
        let at = editing::snap(&tm, at.max(0.0), false);
        let (ops, label) = editor::picture_ops(p, &dir, &file, at, editing::new_layer_length(&tm), layer, above);
        Ok((ops, Some(label)))
    })
}

/// Point a slot at another picture file.
#[tauri::command]
pub fn slot_replace(studio: S<'_>, slot: String, path: String) -> Result<EditDone, String> {
    let file = PathBuf::from(&path);
    if !file.is_file() {
        return Err(format!("no file {path}"));
    }
    let dir = edits_core::edit::project_dir(&studio.path().unwrap_or_default());
    studio.edit(Some(&format!("slot {slot}: {}", file.display())), |p, _| {
        editor::slot_replace_ops(p, &dir, &slot, &file)
    })
}

// ---- thumbnails -----------------------------------------------------------------------

/// The preview picture of an effect, preset or transition (a file in the cache), made on first use.
#[tauri::command]
pub async fn thumbnail(thumbs: State<'_, Arc<Thumbs>>, kind: String, name: String) -> Result<String, String> {
    let t = thumbs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || t.get(&kind, &name))
        .await
        .map_err(|e| e.to_string())?
}

// ---- the song --------------------------------------------------------------------------

fn song_dto(studio: &Studio, songs: &SongService) -> SongDto {
    let Ok((_, doc, path)) = current(studio) else {
        return SongDto::default();
    };
    let audio = song::audio_file(&path, &doc);
    let marks = audio
        .as_ref()
        .and_then(|a| song::load_saved(&path, a))
        .and_then(|v| song::marks(&v, &song::time_ctx(&doc, None), song::audio_offset(&doc)).ok());
    SongDto {
        audio: audio.map(|a| a.display().to_string()),
        running: songs.running.lock().map(|r| r.is_some()).unwrap_or(false),
        error: songs.error.lock().ok().and_then(|e| e.clone()),
        marks,
    }
}

#[tauri::command]
pub fn song_status(studio: S<'_>, songs: State<'_, Arc<SongService>>) -> SongDto {
    song_dto(&studio, &songs)
}

/// Analyse the project's audio on a thread; a `song` event says when it is done.
#[tauri::command]
pub fn song_analyze(studio: S<'_>, songs: State<'_, Arc<SongService>>) -> Result<SongDto, String> {
    let (_, doc, path) = current(&studio)?;
    let audio = song::audio_file(&path, &doc).ok_or("the project has no audio file to analyse")?;
    {
        let mut r = songs.running.lock().map_err(|e| e.to_string())?;
        if r.is_some() {
            return Ok(song_dto(&studio, &songs));
        }
        *r = Some(audio.clone());
    }
    if let Ok(mut e) = songs.error.lock() {
        *e = None;
    }
    let (s, sv) = (studio.inner().clone(), songs.inner().clone());
    std::thread::Builder::new()
        .name("edits-song".into())
        .spawn(move || {
            let result = song::analyze(&path, &audio);
            if let Ok(mut r) = sv.running.lock() {
                *r = None;
            }
            if let Err(e) = &result {
                if let Ok(mut err) = sv.error.lock() {
                    *err = Some(e.clone());
                }
            }
            let dto = song_dto(&s, &sv);
            if let Ok(v) = serde_json::to_value(&dto) {
                (s.emit)("song", v);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(song_dto(&studio, &songs))
}

/// Peaks of the project's audio, `rate` a second from the start of the file.
#[tauri::command]
pub async fn waveform(studio: S<'_>, songs: State<'_, Arc<SongService>>) -> Result<Option<Waveform>, String> {
    let (_, doc, path) = current(&studio)?;
    let Some(audio) = song::audio_file(&path, &doc) else {
        return Ok(None);
    };
    let sv = songs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sv.wave(&audio).map(|w| Some((*w).clone())))
        .await
        .map_err(|e| e.to_string())?
}

/// What a beat operation would do, before it is done.
#[derive(Serialize)]
pub struct DryRun {
    pub count: usize,
    pub label: String,
}

#[tauri::command]
pub fn beats_cut(studio: S<'_>, request: CutRequest, dry: bool) -> Result<Value, String> {
    let (_, doc, path) = current(&studio)?;
    let info = song::song_info(&path, &doc);
    let (_, count, label) = request.apply(&doc, info.as_ref())?;
    if dry {
        return serde_json::to_value(DryRun { count, label }).map_err(|e| e.to_string());
    }
    let done = studio.edit_doc(&label, &|d| request.apply(d, info.as_ref()).map(|(v, _, _)| v))?;
    serde_json::to_value(done).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn beats_accent(studio: S<'_>, request: AccentRequest, dry: bool) -> Result<Value, String> {
    let (_, doc, path) = current(&studio)?;
    let dir = edits_core::edit::project_dir(&path);
    let info = song::song_info(&path, &doc);
    let (_, count, label) = request.apply(&doc, Some(&dir), info.as_ref())?;
    if dry {
        return serde_json::to_value(DryRun { count, label }).map_err(|e| e.to_string());
    }
    let done = studio.edit_doc(&label, &|d| request.apply(d, Some(&dir), info.as_ref()).map(|(v, _, _)| v))?;
    serde_json::to_value(done).map_err(|e| e.to_string())
}

// ---- new projects -----------------------------------------------------------------------

#[derive(Deserialize, Debug)]
pub struct NewProject {
    /// The folder to write `project.json` into (created if missing).
    pub dir: String,
    pub pictures: Vec<String>,
    pub audio: Option<String>,
    /// `9:16`, `1:1` or `16:9`.
    #[serde(default)]
    pub aspect: String,
}

fn size_of(aspect: &str) -> (u32, u32) {
    match aspect {
        "16:9" => (1920, 1080),
        "1:1" => (1080, 1080),
        _ => (1080, 1920),
    }
}

fn free_project_file(dir: &Path) -> PathBuf {
    let first = dir.join("project.json");
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|i| dir.join(format!("project-{i}.json")))
        .find(|p| !p.exists())
        .expect("a name is free")
}

/// A new project of pictures cut to a song (analysed for its tempo), opened.
#[tauri::command]
pub async fn project_new(studio: S<'_>, request: NewProject) -> Result<String, String> {
    let s = studio.inner().clone();
    let path = tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf, String> {
        let dir = PathBuf::from(&request.dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let pictures: Vec<PathBuf> = request.pictures.iter().map(PathBuf::from).collect();
        let audio = request.audio.as_ref().map(PathBuf::from).filter(|a| a.is_file());
        let path = free_project_file(&dir);
        let analysis = match &audio {
            Some(a) => Some(song::analyze(&path, a)?),
            None => None,
        };
        let doc = editor::new_project(&dir, &pictures, audio.as_deref(), analysis.as_ref(), size_of(&request.aspect), 64.0)?;
        let text = edits_core::jsonfmt::to_file_text(&doc);
        edits_core::edit::write_atomic(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(path)
    })
    .await
    .map_err(|e| e.to_string())??;
    s.open_project(&path)?;
    Ok(path.display().to_string())
}

/// A new project drafted from a reference edit (`edits-analyze analyze` then `draft`, with the proxies as pictures), opened.
#[tauri::command]
pub async fn project_from_reference(studio: S<'_>, video: String, dir: String) -> Result<String, String> {
    let s = studio.inner().clone();
    let path = tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf, String> {
        let dir = PathBuf::from(&dir);
        let analysis_dir = dir.join("analysis");
        std::fs::create_dir_all(&analysis_dir).map_err(|e| format!("cannot create {}: {e}", analysis_dir.display()))?;
        let an = edits_analyze::pipeline::analyze_video(Path::new(&video), Some(&analysis_dir), &edits_analyze::AnalyzeOpts::default())
            .map_err(|e| format!("{e:#}"))?;
        let opts = edits_analyze::draft::DraftOpts {
            proxies: true,
            width: None,
            height: None,
            snap: true,
            repeats: true,
            analysis_dir: analysis_dir.clone(),
            project_dir: dir.clone(),
        };
        let project = edits_analyze::draft::draft(&an, &opts);
        let path = free_project_file(&dir);
        std::fs::write(&path, project.to_json()).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(path)
    })
    .await
    .map_err(|e| e.to_string())??;
    s.open_project(&path)?;
    Ok(path.display().to_string())
}
