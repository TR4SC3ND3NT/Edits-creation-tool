//! The open project: the last valid version of the file, what is wrong with
//! the file on disk, its history, and the snapshot the frontend draws.
//!
//! The JSON file stays the only source of truth, as in the egui window: the
//! studio never keeps a project of its own. Edits go through
//! `edits_core::edit` (actor `window`) and the file is read back at once;
//! changes by anyone else arrive through the watcher (`engine::reload`).

use crate::engine::reload::{self, LoadReport, Problem, ProjectState};
use crate::engine::rows;
use crate::engine::timemap::TimeMap;
use edits_core::edit::{self, HistoryEntry};
use edits_core::{LayerKind, Project};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// The project the window has open.
pub struct Open {
    /// Absolute path of the project file.
    pub path: PathBuf,
    pub state: ProjectState,
    /// The newest history entry and the size of the redo stack when the
    /// file was last read: a new entry after a reload is someone's edit.
    seen: (Option<PathBuf>, usize),
    pub watcher: Option<reload::WatchHandle>,
}

/// Why a new snapshot was sent.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    /// The project was opened.
    Opened,
    /// The window's own edit, undo or redo.
    Window { summary: String },
    /// Another program edited through the shared history (the agent's `project_edit`).
    Edit { actor: String, summary: String },
    /// Another program undid an edit (the agent's `project_undo`).
    Undo { actor: String, summary: String },
    /// The file changed without a history entry (a text editor, a script, a picture).
    Reloaded,
    /// The file on disk stopped being valid; the last valid version stays.
    Invalid { errors: usize },
    /// Nothing that is drawn changed, but the problems did.
    Problems,
}

impl Open {
    pub fn new(path: PathBuf) -> (Self, LoadReport) {
        let report = reload::load(&path);
        let mut open = Open {
            path,
            state: ProjectState::default(),
            seen: (None, 0),
            watcher: None,
        };
        open.seen = open.history_marker();
        (open, report)
    }

    pub fn dir(&self) -> PathBuf {
        reload::base_dir(&self.path)
    }

    fn history_marker(&self) -> (Option<PathBuf>, usize) {
        (
            edit::history(&self.path).last().cloned(),
            edit::redo_stack(&self.path).len(),
        )
    }

    /// Who changed the file since it was last read, from the shared history.
    pub fn who_changed(&mut self) -> Reason {
        let now = self.history_marker();
        let before = std::mem::replace(&mut self.seen, now);
        let list = edit::history_list(&self.path);
        let newest = self.seen.0.clone();
        if newest.is_some() && newest != before.0 && self.seen.1 == 0 {
            if let Some(e) = list.undo.last() {
                return Reason::Edit {
                    actor: e.actor.clone(),
                    summary: e.summary.clone(),
                };
            }
        }
        if self.seen.1 > before.1 {
            if let Some(e) = list.redo.last() {
                return Reason::Undo {
                    actor: e.actor.clone(),
                    summary: e.summary.clone(),
                };
            }
        }
        Reason::Reloaded
    }

    /// The window wrote the file: remember the history as it is now, so the
    /// watcher does not take the window's own edit for someone else's.
    pub fn mark_own(&mut self) {
        self.seen = self.history_marker();
    }

    pub fn project(&self) -> Option<&Arc<Project>> {
        self.state.current.as_ref()
    }

    pub fn doc(&self) -> Option<&Value> {
        self.state.doc.as_deref()
    }
}

// ---- the snapshot the frontend draws ---------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct Snapshot {
    pub path: String,
    pub dir: String,
    pub file_name: String,
    /// Counts accepted versions; frames are cached per revision.
    pub revision: u64,
    /// `edits_core::edit::text_hash` of the version shown, as text (it does not fit a JS number).
    pub text_hash: Option<String>,
    /// The file on disk has errors: what is shown is the last valid version.
    pub rejected: bool,
    pub problems: Vec<ProblemDto>,
    pub info: Option<Info>,
    /// Timeline rows, top of the picture first (the camera first when it is animated).
    pub rows: Vec<RowDto>,
    /// The layers in file order (bottom first), for tables.
    pub layers: Vec<LayerDto>,
    pub markers: Vec<MarkerDto>,
    pub slots: Vec<SlotDto>,
    pub history: HistoryDto,
    /// The file's JSON as shown.
    pub doc: Option<Value>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProblemDto {
    pub error: bool,
    pub path: String,
    pub message: String,
    pub hint: Option<String>,
}

impl From<&Problem> for ProblemDto {
    fn from(p: &Problem) -> Self {
        ProblemDto {
            error: p.error,
            path: p.path.clone(),
            message: p.message.clone(),
            hint: p.hint.clone(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    /// `beats` or `seconds`.
    pub time_base: &'static str,
    pub bpm: Option<f64>,
    pub beat_offset: f64,
    pub frames: u64,
    pub seconds: f64,
    /// Project time of the first frame and at the end of the last.
    pub start_time: f64,
    pub end_time: f64,
    pub audio: Option<AudioDto>,
    pub motion_blur: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct AudioDto {
    /// Absolute path of the audio file (the frontend plays it through the asset protocol).
    pub path: String,
    pub exists: bool,
    pub offset: f64,
    pub gain: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct MarkDto {
    pub t: f64,
    /// `linear`, `hold` or `eased`.
    pub shape: &'static str,
    pub what: Vec<String>,
    pub effect: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct RowDto {
    pub id: String,
    /// `camera`, or the layer's kind (`image`, `solid`, `cube`, ...).
    pub kind: String,
    /// `image a`, `cube`, `text "hello"`.
    pub label: String,
    pub index: Option<usize>,
    pub start: f64,
    pub end: f64,
    pub enabled: bool,
    pub effects: Vec<String>,
    pub blend: Option<String>,
    pub marks: Vec<MarkDto>,
    pub transition: Option<(String, f64)>,
    /// Every animated property with its keys, for the keyframe lanes.
    pub lanes: Vec<crate::editor::LaneDto>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LayerDto {
    pub id: String,
    pub index: usize,
    pub kind: String,
    pub label: String,
    pub name: Option<String>,
    pub start: f64,
    pub end: f64,
    pub enabled: bool,
    pub blend: String,
    pub effects: Vec<EffectBrief>,
    pub keys: usize,
    pub transition: Option<String>,
    pub slot: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct EffectBrief {
    pub kind: String,
    pub enabled: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct MarkerDto {
    pub t: f64,
    pub label: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct SlotDto {
    pub name: String,
    pub path: Option<String>,
    pub exists: bool,
    pub fit: String,
    /// The file, absolute (for thumbnails).
    pub file: Option<String>,
    /// Layers that show it.
    pub used_by: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct HistoryDto {
    /// Newest first.
    pub undo: Vec<HistoryItem>,
    /// The next redo first.
    pub redo: Vec<HistoryItem>,
}

#[derive(Serialize, Clone, Debug)]
pub struct HistoryItem {
    pub file: String,
    /// Milliseconds since 1970, when the edit was made (for a redo entry: when it was undone).
    pub time: Option<f64>,
    pub actor: String,
    pub summary: String,
    /// The summary as a person reads it (`d: transform.rotation → [0, 0, 12]`).
    pub readable: String,
}

fn millis(t: Option<SystemTime>) -> Option<f64> {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
}

fn item(e: &HistoryEntry) -> HistoryItem {
    HistoryItem {
        file: e.file.display().to_string(),
        time: millis(e.time),
        actor: e.actor.clone(),
        summary: e.summary.clone(),
        readable: crate::engine::editing::readable_summary(&e.summary),
    }
}

pub fn history(path: &Path) -> HistoryDto {
    let list = edit::history_list(path);
    HistoryDto {
        undo: list.undo.iter().rev().map(item).collect(),
        redo: list.redo.iter().rev().map(item).collect(),
    }
}

/// `image`, `solid`, `cube`, ...: the tag of a layer kind.
pub fn kind_tag(kind: &LayerKind) -> &'static str {
    match kind {
        LayerKind::Image { .. } => "image",
        LayerKind::Solid { .. } => "solid",
        LayerKind::Cube { .. } => "cube",
        LayerKind::Adjustment => "adjustment",
        LayerKind::Video(_) => "video",
        LayerKind::Particles(_) => "particles",
        LayerKind::Text(_) => "text",
        LayerKind::Comp(_) => "comp",
        LayerKind::Shape(_) => "shape",
    }
}

fn shape_text(s: rows::KeyShape) -> &'static str {
    match s {
        rows::KeyShape::Linear => "linear",
        rows::KeyShape::Hold => "hold",
        rows::KeyShape::Eased => "eased",
    }
}

fn marks(m: &[rows::KeyMark], tolerance: f64) -> Vec<MarkDto> {
    rows::group_marks(m, tolerance)
        .into_iter()
        .map(|g| MarkDto {
            t: g.t,
            shape: shape_text(g.shape),
            what: g.what,
            effect: g.effect && !g.transform,
        })
        .collect()
}

pub fn info(p: &Project, dir: &Path) -> Info {
    let tm = TimeMap::of(p);
    let s = &p.settings;
    Info {
        width: s.width,
        height: s.height,
        fps: s.fps,
        duration: s.duration,
        time_base: if tm.uses_beats() { "beats" } else { "seconds" },
        bpm: tm.bpm,
        beat_offset: s.beat_offset,
        frames: p.frame_count(),
        seconds: p.duration_seconds(),
        start_time: tm.start_time(),
        end_time: tm.end_time(),
        audio: p.audio.as_ref().map(|a| {
            let path = dir.join(&a.path);
            let path = path.canonicalize().unwrap_or(path);
            AudioDto {
                exists: path.is_file(),
                path: path.display().to_string(),
                offset: a.offset,
                gain: a.gain,
            }
        }),
        motion_blur: s.motion_blur.samples > 1,
    }
}

impl Open {
    pub fn snapshot(&self) -> Snapshot {
        let dir = self.dir();
        let p = self.project();
        let tolerance = p.map(|p| p.frame_len() * 0.5).unwrap_or(1e-4);
        let rows = p
            .map(|p| {
                rows::rows(p)
                    .into_iter()
                    .map(|r| RowDto {
                        kind: match r.layer {
                            Some(i) => kind_tag(&p.layers[i].kind).to_string(),
                            None => "camera".into(),
                        },
                        id: r.id,
                        label: r.kind,
                        index: r.layer,
                        start: r.start,
                        end: r.end,
                        enabled: r.enabled,
                        effects: r.effects,
                        blend: r.blend,
                        marks: marks(&r.marks, tolerance),
                        transition: r.transition,
                        lanes: match r.layer {
                            Some(i) => crate::editor::layer_lanes(p, &p.layers[i]),
                            None => crate::editor::camera_lanes(p),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();
        let layers = p
            .map(|p| {
                p.layers
                    .iter()
                    .enumerate()
                    .map(|(index, l)| LayerDto {
                        id: l.id.clone(),
                        index,
                        kind: kind_tag(&l.kind).into(),
                        label: rows::kind_text(&l.kind),
                        name: l.name.clone(),
                        start: l.start,
                        end: l.end.unwrap_or(p.settings.duration),
                        enabled: l.enabled,
                        blend: l.blend.name().into(),
                        effects: l
                            .effects
                            .iter()
                            .map(|e| EffectBrief {
                                kind: e.kind.clone(),
                                enabled: e.enabled,
                            })
                            .collect(),
                        keys: rows::layer_marks(l).len(),
                        transition: l.transition.as_ref().map(|t| t.kind.clone()),
                        slot: match &l.kind {
                            LayerKind::Image { slot } => Some(slot.clone()),
                            LayerKind::Video(v) => Some(v.slot.clone()),
                            LayerKind::Cube { faces, .. } => faces.first().cloned(),
                            _ => None,
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();
        let slots = p
            .map(|p| {
                p.slots
                    .iter()
                    .map(|(name, s)| SlotDto {
                        name: name.clone(),
                        exists: s.path.as_ref().is_some_and(|f| dir.join(f).is_file()),
                        path: s.path.clone(),
                        file: s.path.as_ref().map(|f| dir.join(f).display().to_string()),
                        fit: serde_json::to_value(s.fit)
                            .ok()
                            .and_then(|v| v.as_str().map(str::to_string))
                            .unwrap_or_default(),
                        used_by: p
                            .layers
                            .iter()
                            .filter(|l| match &l.kind {
                                LayerKind::Image { slot } => slot == name,
                                LayerKind::Video(v) => &v.slot == name,
                                LayerKind::Cube { faces, .. } => faces.contains(name),
                                _ => false,
                            })
                            .map(|l| l.id.clone())
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Snapshot {
            path: self.path.display().to_string(),
            dir: dir.display().to_string(),
            file_name: self
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            revision: self.state.revision,
            text_hash: self.state.text_hash.map(|h| h.to_string()),
            rejected: self.state.rejected,
            problems: self.state.problems.iter().map(ProblemDto::from).collect(),
            info: p.map(|p| info(p, &dir)),
            rows,
            layers,
            markers: p
                .map(|p| {
                    p.markers
                        .iter()
                        .map(|m| MarkerDto {
                            t: m.t,
                            label: m.label.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            slots,
            history: history(&self.path),
            doc: self.doc().cloned(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const PROJECT: &str = r#"{
      "format": "edits.project/1",
      "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 120 },
      "slots": { "a": {}, "b": {} },
      "markers": [ { "t": 4, "label": "drop" } ],
      "layers": [
        { "id": "a", "kind": "image", "slot": "a", "in": 0, "out": 4,
          "transform": { "scale": { "keys": [ { "t": 0, "v": 1.3, "ease": "hold" }, { "t": 1, "v": 1 } ] } } },
        { "id": "b", "kind": "image", "slot": "b", "in": 4 },
        { "id": "finish", "kind": "adjustment", "effects": [ { "kind": "vignette" } ] }
      ]
    }"#;

    pub fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edits-studio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("project.json");
        std::fs::write(&path, PROJECT).unwrap();
        path
    }

    #[test]
    fn a_snapshot_lists_rows_top_first_and_layers_in_file_order() {
        let path = temp("snapshot");
        let (mut open, report) = Open::new(path.clone());
        open.state.apply(report);
        let s = open.snapshot();
        assert_eq!(s.revision, 1);
        assert!(!s.rejected);
        let ids: Vec<&str> = s.rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["finish", "b", "a"]);
        assert_eq!(
            s.layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "finish"]
        );
        let a = s.rows.iter().find(|r| r.id == "a").unwrap();
        assert_eq!(a.marks.len(), 2);
        assert_eq!(a.marks[0].shape, "hold");
        let info = s.info.unwrap();
        assert_eq!(
            (info.frames, info.time_base, info.bpm),
            (100, "beats", Some(120.0))
        );
        assert_eq!(s.markers[0].label, "drop");
        assert_eq!(s.slots.len(), 2);
        assert_eq!(s.slots[0].used_by, ["a"]);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn an_edit_through_the_history_is_told_apart_from_a_plain_reload() {
        let path = temp("who");
        let (mut open, report) = Open::new(path.clone());
        open.state.apply(report);
        let opts = edit::EditOptions {
            actor: edit::AGENT,
            ..Default::default()
        };
        edit::apply(
            &path,
            &[edit::EditOp::set("/settings/bpm", serde_json::json!(128))],
            &opts,
        )
        .unwrap();
        match open.who_changed() {
            Reason::Edit { actor, summary } => {
                assert_eq!(actor, "agent");
                assert!(summary.contains("bpm"), "{summary}");
            }
            other => panic!("{other:?}"),
        }
        edit::undo(&path).unwrap();
        assert!(matches!(open.who_changed(), Reason::Undo { .. }));
        std::fs::write(&path, PROJECT.replace("\"duration\": 8", "\"duration\": 6")).unwrap();
        assert_eq!(open.who_changed(), Reason::Reloaded);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
