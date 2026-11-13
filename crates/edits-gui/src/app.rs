//! The window: state, the per-frame logic that ties the clock, the cache and
//! the render thread together, the transport, preview, menus and status bar,
//! and the plumbing of editing: every change goes through
//! `edits_core::edit::apply` (validated, written atomically, backed up in the
//! history the agent shares), then the file is read back at once so the
//! window's own write never looks like someone else's.
//! The timeline, the inspector and the side panels are drawn by their own modules.

use crate::audio::{AudioEngine, AudioStatus};
use crate::cache::{FrameCache, FrameKey, Scale};
use crate::clock::{lead_frames, newest_ready, next_request, PlayClock, RenderAverage};
use crate::editing::{self, NewLayer, PropRef, TimeDrag};
use crate::export::{self, Export};
use crate::reload::{self, Applied, LoadReport, ProjectState, WatchHandle};
use crate::rows::{self, Row};
use crate::timemap::{fmt_seconds, TimeMap, View};
use crate::worker::{ProjectMsg, Worker, WorkerEvent};
use crate::{inspector, panels, timeline, Options};
use eframe::egui::{self, Align2, Color32, ColorImage, FontId, Key, Modifiers, Rect, RichText, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui};
use edits_core::edit::{self as core_edit, EditOp, EditOptions, HistoryList};
use edits_core::Project;
use edits_render::{AdapterInfo, RenderOptions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Colours shared by the panels.
pub(crate) mod pal {
    use eframe::egui::Color32;
    pub const GOOD: Color32 = Color32::from_rgb(96, 200, 120);
    pub const WARN: Color32 = Color32::from_rgb(235, 190, 80);
    pub const BAD: Color32 = Color32::from_rgb(240, 100, 90);
    pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 80, 80);
    pub const KEY_TRANSFORM: Color32 = Color32::from_rgb(255, 214, 90);
    pub const KEY_EFFECT: Color32 = Color32::from_rgb(90, 220, 235);
    pub const MARKER: Color32 = Color32::from_rgb(255, 150, 60);
    pub const WAVE: Color32 = Color32::from_rgb(90, 170, 160);
}

/// How long the "reloaded" flash stays.
const FLASH: Duration = Duration::from_millis(2500);
/// Screenshot mode gives up after this long (plus the asked delay).
const SHOT_TIMEOUT: Duration = Duration::from_secs(180);
/// Previews of a change still being made (a value being dragged) get
/// revisions from here up, far from the file's own.
const GHOST_BASE: u64 = 1 << 40;
/// How long a message stays in the status bar (an error: twice as long).
const STATUS_FOR: Duration = Duration::from_secs(8);

/// Which page the right-hand panel shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Inspector,
    Slots,
}

/// A change being made with one widget (a drag, or typing into a value): it
/// is previewed as it goes and written as one edit when the widget lets go.
struct Pending {
    id: egui::Id,
    ops: Vec<EditOp>,
    label: String,
    /// The version of the file the ops were prepared against.
    base: Option<u64>,
    /// The widget said this frame that it is still at work.
    seen: bool,
}

/// The project with a pending change applied, as the renderer previews it.
struct Ghost {
    revision: u64,
    project: Arc<Project>,
}

/// The last thing the window did, or why it did not.
pub(crate) struct Status {
    pub text: String,
    pub detail: String,
    pub error: bool,
    pub at: Instant,
}

/// Where a version of the project came from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// The file as the window opened it.
    First,
    /// The window's own write (an edit, undo or redo): no "reloaded" flash.
    Own,
    /// Someone else changed the file.
    Disk,
}

/// What the inspector shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Selected {
    Camera,
    /// A layer, by id, so the selection survives a reload.
    Layer(String),
}

pub(crate) struct Playing {
    pub clock: PlayClock,
    pub since: Instant,
    pub laps: u64,
}

#[derive(Default)]
struct ExportUi {
    open: bool,
    path: String,
    job: Option<Export>,
    /// The `edits` command, looked up once.
    edits: Option<Option<PathBuf>>,
    error: Option<String>,
}

enum ShotPhase {
    /// Waiting for the preview of the asked frame.
    Loading,
    /// Keeping the window running, so a change made meanwhile is in the picture.
    Delay(Instant),
    /// A few more frames so everything that was just uploaded is painted.
    Settle(u8),
    /// The screenshot was requested.
    Waiting,
    Closing,
}

struct Shot {
    path: PathBuf,
    phase: ShotPhase,
    delayed: bool,
}

pub(crate) struct App {
    ctx: egui::Context,
    pub path: PathBuf,
    pub base: PathBuf,
    options: Options,
    pub state: ProjectState,
    pub tm: Option<TimeMap>,
    pub rows: Arc<Vec<Row>>,

    worker: Worker,
    worker_events: Receiver<WorkerEvent>,
    reports: Receiver<LoadReport>,
    _watch: WatchHandle,

    adapter: Option<AdapterInfo>,
    no_adapter: Option<String>,
    prepare_error: Option<String>,
    cache: FrameCache<Arc<ColorImage>>,
    texture: Option<TextureHandle>,
    /// The picture on screen.
    shown: Option<FrameKey>,
    /// The last frame asked of the render thread and not answered yet.
    posted: Option<FrameKey>,
    failed: Option<(FrameKey, String)>,
    last_render: Option<(FrameKey, f64)>,
    average: RenderAverage,
    average_for: (Scale, bool),

    pub playhead: u64,
    pub playing: Option<Playing>,
    pub looping: bool,
    scale: Scale,
    blur_paused: bool,
    blur_playing: bool,

    /// The timeline's horizontal view; `view_fit` keeps the whole project in it.
    pub view: Option<View>,
    pub view_fit: bool,
    pub selected: Option<Selected>,

    pub audio: AudioEngine,
    reloaded_at: Option<Instant>,
    export: ExportUi,
    shot: Option<Shot>,
    started: Instant,
    outcome: Arc<Mutex<Option<Result<(), String>>>>,
    /// Height the timeline panel opens with: enough for every row of the first project.
    timeline_height: f32,

    // ---- editing ----
    /// The property double-clicking a timeline row keys, and the key strip shows.
    pub selected_prop: Option<PropRef>,
    pending: Option<Pending>,
    ghost: Option<Ghost>,
    ghost_serial: u64,
    /// A drag on the timeline, and the version of the file it started on.
    pub drag: Option<(TimeDrag, Option<u64>)>,
    /// The shared history (the window's and the agent's edits), as on disk.
    pub history: HistoryList,
    pub status: Option<Status>,
    pub tab: Tab,
    /// Files dropped on the window this frame, and whether some are being dragged over it.
    pub dropped: Vec<PathBuf>,
    pub files_hovering: bool,
    /// Text being typed: the name of a new slot, slot paths.
    pub new_slot: String,
    pub slot_text: HashMap<String, String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: PathBuf, options: Options, first: LoadReport, outcome: Arc<Mutex<Option<Result<(), String>>>>) -> Self {
        let ctx = cc.egui_ctx.clone();
        ctx.set_theme(egui::Theme::Dark);

        let (worker_tx, worker_events) = mpsc::channel();
        let wake = ctx.clone();
        let worker = Worker::spawn(RenderOptions { adapter: options.adapter, ..Default::default() }, worker_tx, move || wake.request_repaint());

        let (report_tx, reports) = mpsc::channel();
        let wake = ctx.clone();
        let watch = reload::spawn_watcher(path.clone(), first.files.clone(), move |report| {
            let _ = report_tx.send(report);
            wake.request_repaint();
        });

        let budget = options.cache_mb.unwrap_or(384).max(16) * 1024 * 1024;
        let scale = options.scale.unwrap_or(Scale::Half);
        let shot = options.screenshot.clone().map(|path| Shot { path, phase: ShotPhase::Loading, delayed: false });
        let mut app = Self {
            base: reload::base_dir(&path),
            export: ExportUi { path: export::default_output(&path).display().to_string(), ..Default::default() },
            path,
            state: ProjectState::default(),
            tm: None,
            rows: Arc::default(),
            worker,
            worker_events,
            reports,
            _watch: watch,
            adapter: None,
            no_adapter: None,
            prepare_error: None,
            cache: FrameCache::new(budget),
            texture: None,
            shown: None,
            posted: None,
            failed: None,
            last_render: None,
            average: RenderAverage::default(),
            average_for: (scale, false),
            playhead: 0,
            playing: None,
            looping: true,
            scale,
            blur_paused: true,
            blur_playing: false,
            view: None,
            view_fit: true,
            selected: None,
            audio: AudioEngine::new(false),
            reloaded_at: None,
            shot,
            started: Instant::now(),
            outcome,
            options,
            timeline_height: 240.0,
            ctx: ctx.clone(),
            selected_prop: Some(PropRef::Transform("scale")),
            pending: None,
            ghost: None,
            ghost_serial: 0,
            drag: None,
            history: HistoryList::default(),
            status: None,
            tab: Tab::Inspector,
            dropped: Vec::new(),
            files_hovering: false,
            new_slot: String::new(),
            slot_text: HashMap::new(),
        };
        app.take_report(first, Origin::First);
        if let Some(p) = app.project() {
            // Every row visible if the window allows it, but the preview keeps most of the height.
            let window = app.options.size.map_or(860.0, |(_, h)| h as f32);
            app.timeline_height = timeline::natural_height(p, app.rows.len()).min(window * 0.4).max(150.0);
        }
        if let (Some(at), Some(tm)) = (app.options.at, app.tm) {
            app.playhead = at.min(tm.last_frame());
        }
        if let Some(id) = app.options.select.clone() {
            app.selected = Some(if id == "camera" { Selected::Camera } else { Selected::Layer(id) });
            app.drop_stale_selection();
        }
        if app.options.play {
            app.start_playing(app.playhead);
        }
        app
    }

    pub fn project(&self) -> Option<&Arc<Project>> {
        self.state.current.as_ref()
    }

    // ---- reload ----------------------------------------------------------------

    fn take_report(&mut self, report: LoadReport, origin: Origin) {
        let ctx = self.ctx.clone();
        self.history = core_edit::history_list(&self.path);
        match self.state.apply(report) {
            Applied::Swapped => {
                if origin == Origin::Disk && (self.pending.is_some() || self.drag.is_some()) {
                    self.pending = None;
                    self.drag = None;
                    self.fail("The project changed on disk while you were editing: your unfinished change was dropped.", "");
                }
                // The new version replaces any preview of an unfinished change.
                self.ghost = None;
                let Some(project) = self.state.current.clone() else { return };
                let tm = TimeMap::of(&project);
                let span_changed = self.tm.is_none_or(|old| old.start_time() != tm.start_time() || old.end_time() != tm.end_time());
                if span_changed && !self.view_fit {
                    // The old scroll position means nothing on a project of another length.
                    self.view_fit = true;
                }
                self.tm = Some(tm);
                self.rows = Arc::new(rows::rows(&project));
                self.playhead = self.playhead.min(tm.last_frame());
                // Pictures of the old version are of no use: a new revision starts empty.
                self.cache.clear();
                self.posted = None;
                self.failed = None;
                self.prepare_error = None;
                self.average.reset();
                self.worker.set_project(ProjectMsg { revision: self.state.revision, project: project.clone(), base_dir: self.base.clone() });
                let wake = ctx.clone();
                self.audio.set_file(project.audio.as_ref().map(|a| self.base.join(&a.path)), move || wake.request_repaint());
                self.drop_stale_selection();
                if self.playing.is_some() {
                    // Length, frame rate or audio offset may have changed: start the clock again from here.
                    self.start_playing(self.playhead);
                }
                if origin == Origin::Disk {
                    self.reloaded_at = Some(Instant::now());
                }
            }
            Applied::Rejected | Applied::Unchanged => {}
        }
    }

    fn drop_stale_selection(&mut self) {
        let keep = match (&self.selected, self.project()) {
            (Some(Selected::Layer(id)), Some(p)) => p.layers.iter().any(|l| &l.id == id),
            (Some(Selected::Camera), Some(_)) => true,
            _ => false,
        };
        if !keep {
            self.selected = None;
        }
    }

    // ---- editing -----------------------------------------------------------------

    /// The project the panels show: with a pending change applied, while there is one.
    pub fn shown_project(&self) -> Option<Arc<Project>> {
        self.ghost.as_ref().map(|g| g.project.clone()).or_else(|| self.state.current.clone())
    }

    /// A message in the status bar.
    pub fn say(&mut self, text: impl Into<String>) {
        // The bundled fonts have no arrow in every style; the core's change lines use one.
        self.status = Some(Status { text: text.into().replace('→', "->"), detail: String::new(), error: false, at: Instant::now() });
    }

    /// An error in the status bar; `detail` goes into its tooltip.
    pub fn fail(&mut self, text: impl Into<String>, detail: impl Into<String>) {
        self.status = Some(Status { text: text.into().replace('→', "->"), detail: detail.into().replace('→', "->"), error: true, at: Instant::now() });
    }

    /// Edits are prepared against the version on screen; when the file on
    /// disk is broken, that version is not what is on disk.
    fn can_edit(&mut self) -> bool {
        if self.state.rejected {
            self.fail("The file on disk has errors, so the window does not change it: fix them, or Undo (Ctrl+Z).", "");
            return false;
        }
        self.state.current.is_some()
    }

    /// Write `ops` now, as one step in the shared history.
    pub fn commit(&mut self, ops: Vec<EditOp>, label: impl Into<String>) {
        let base = self.state.text_hash;
        self.commit_with(ops, label.into(), base);
    }

    /// Write `ops`, prepared against the version of the file whose hash is `base`.
    pub fn commit_against(&mut self, ops: Vec<EditOp>, label: String, base: Option<u64>) {
        self.commit_with(ops, label, base);
    }

    /// Double-click on a timeline row: a key of the picked property at `t`.
    pub fn key_at(&mut self, id: &str, t: f64) {
        let Some(p) = self.project().cloned() else { return };
        let Some(l) = p.layers.iter().find(|l| l.id == id) else { return };
        let Some(r) = self.selected_prop.clone().filter(|r| r.fits(l)) else {
            self.fail("Pick a property first (click its name in the inspector), then double-click a row to key it there.", "");
            return;
        };
        match editing::with_key_here(&p, r.get(&p, Some(l)), &r.default_value(), t) {
            Some(new) => self.commit(vec![editing::set_prop(id, &r, &new)], format!("key {} of {id} at {}", r.label(), editing::tidy(t))),
            None => self.say(format!("{} of {id} already has a key at {}", r.label(), editing::tidy(t))),
        }
    }

    fn commit_with(&mut self, ops: Vec<EditOp>, label: String, base: Option<u64>) {
        self.pending = None;
        let had_ghost = self.ghost.take().is_some();
        let written = if ops.is_empty() || !self.can_edit() {
            false
        } else {
            let opts = EditOptions { actor: core_edit::WINDOW, label: Some(&label), expect: base, ..Default::default() };
            match core_edit::apply(&self.path, &ops, &opts) {
                Ok(out) if out.ok && out.written => {
                    self.say(label.clone());
                    true
                }
                Ok(out) if out.ok => false,
                Ok(out) => {
                    let errors: Vec<String> = out.findings.iter().filter(|f| f.severity == edits_core::check::Severity::Error).map(core_edit::finding_line).collect();
                    let first = errors.first().cloned().unwrap_or_default();
                    self.fail(format!("Not applied ({label}): {first}"), errors.join("\n"));
                    false
                }
                Err(e) if e == core_edit::STALE => {
                    self.fail("Not applied: the project changed on disk (the agent wrote it?) while you were editing. The window has caught up; do it again.", e);
                    // Catch up now instead of waiting for the file watcher.
                    self.take_report(reload::load(&self.path), Origin::Disk);
                    return;
                }
                Err(e) => {
                    self.fail(format!("Not applied: {}", shorten(&e, 120)), e);
                    false
                }
            }
        };
        if written {
            self.take_report(reload::load(&self.path), Origin::Own);
        } else if had_ghost {
            // Back to the file's version.
            self.send_current();
        }
    }

    fn send_current(&mut self) {
        if let Some(project) = self.state.current.clone() {
            self.worker.set_project(ProjectMsg { revision: self.state.revision, project, base_dir: self.base.clone() });
            self.posted = None;
        }
    }

    /// A widget changed a value and is still at work (dragging, typing):
    /// preview `ops`, and write them when it lets go.
    pub fn edit_live(&mut self, id: egui::Id, ops: Vec<EditOp>, label: impl Into<String>) {
        if self.pending.as_ref().is_some_and(|p| p.id != id) {
            self.finish_pending();
        }
        if self.state.rejected {
            self.can_edit();
            return;
        }
        let base = self.pending.as_ref().map_or(self.state.text_hash, |p| p.base);
        self.preview_ops(&ops);
        self.pending = Some(Pending { id, ops, label: label.into(), base, seen: true });
    }

    /// The widget with this id is still being dragged or typed into.
    pub fn keep_live(&mut self, id: egui::Id) {
        if let Some(p) = self.pending.as_mut().filter(|p| p.id == id) {
            p.seen = true;
        }
    }

    /// The pending change, if its widget let go this frame: one edit.
    fn finish_gestures(&mut self) {
        match &mut self.pending {
            Some(p) if p.seen => p.seen = false,
            Some(_) => self.finish_pending(),
            None => {}
        }
    }

    fn finish_pending(&mut self) {
        if let Some(p) = self.pending.take() {
            self.commit_with(p.ops, p.label, p.base);
        }
    }

    /// Show the project with `ops` applied without writing anything.
    fn preview_ops(&mut self, ops: &[EditOp]) {
        let Some(doc) = self.state.doc.clone() else { return };
        let Ok(changed) = core_edit::apply_ops(&doc, ops) else { return };
        let Ok(project) = serde_json::from_value::<Project>(changed) else { return };
        self.ghost_serial += 1;
        let revision = GHOST_BASE + self.ghost_serial;
        let project = Arc::new(project);
        self.worker.set_project(ProjectMsg { revision, project: project.clone(), base_dir: self.base.clone() });
        self.posted = None;
        self.ghost = Some(Ghost { revision, project });
    }

    /// Ctrl+Z: put back the version before the newest entry of the shared
    /// history, whoever made it.
    pub fn undo(&mut self) {
        self.finish_pending();
        match core_edit::undo(&self.path) {
            Ok(u) => {
                self.take_report(reload::load(&self.path), Origin::Own);
                self.say(format!("Undid {}: {}", who(&u.entry.actor), u.entry.summary));
            }
            Err(e) => self.fail(shorten(&e, 120), e),
        }
    }

    /// Ctrl+Shift+Z / Ctrl+Y: bring back what the last undo took.
    pub fn redo(&mut self) {
        self.finish_pending();
        match core_edit::redo(&self.path) {
            Ok(u) => {
                self.take_report(reload::load(&self.path), Origin::Own);
                self.say(format!("Redid {}: {}", who(&u.entry.actor), u.entry.summary));
            }
            Err(e) => self.fail(shorten(&e, 120), e),
        }
    }

    /// The project time of the playhead, snapped to whole beats in a beats project.
    fn new_layer_time(&self) -> f64 {
        let Some(tm) = self.tm else { return 0.0 };
        let t = tm.frame_time(self.playhead);
        if tm.uses_beats() {
            t.floor().max(0.0)
        } else {
            editing::snap(&tm, t, true).max(0.0)
        }
    }

    /// Add a layer at the playhead, above the selected one (or on top), and select it.
    pub fn add_layer(&mut self, kind: NewLayer) {
        let (Some(p), Some(tm)) = (self.project().cloned(), self.tm) else { return };
        let layer = editing::new_layer(&p, &kind, self.new_layer_time(), editing::new_layer_length(&tm));
        let id = layer.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let above = match &self.selected {
            Some(Selected::Layer(sel)) if p.layers.iter().any(|l| &l.id == sel) => Some(sel.clone()),
            _ => None,
        };
        let what = match kind {
            NewLayer::Image(_) => "image",
            NewLayer::Solid => "solid",
            NewLayer::Adjustment => "adjustment",
            NewLayer::Cube(_) => "cube",
        };
        self.commit(vec![EditOp::insert_layer(layer, above, None)], format!("add {what} layer {id}"));
        if self.project().is_some_and(|p| p.layers.iter().any(|l| l.id == id)) {
            self.selected = Some(Selected::Layer(id));
        }
    }

    /// The selected layer's id, if a layer is selected.
    pub fn selected_layer(&self) -> Option<String> {
        match &self.selected {
            Some(Selected::Layer(id)) => Some(id.clone()),
            _ => None,
        }
    }

    pub fn delete_layer(&mut self, id: &str) {
        self.commit(vec![EditOp::remove_layer(id)], format!("delete layer {id}"));
    }

    pub fn duplicate_layer(&mut self, id: &str) {
        let Some(p) = self.project().cloned() else { return };
        if let Some((ops, new_id)) = editing::duplicate_ops(&p, self.state.doc.as_deref(), id) {
            self.commit(ops, format!("duplicate {id} as {new_id}"));
            self.selected = Some(Selected::Layer(new_id));
            self.drop_stale_selection();
        }
    }

    pub fn move_layer(&mut self, id: &str, up: bool) {
        let Some(p) = self.project().cloned() else { return };
        match editing::move_layer_ops(&p, self.state.doc.as_deref(), id, up) {
            Some(ops) => self.commit(ops, format!("move {id} {}", if up { "up" } else { "down" })),
            None => self.say(format!("{id} is already at the {}", if up { "top" } else { "bottom" })),
        }
    }

    pub fn toggle_layer(&mut self, id: &str) {
        let Some(enabled) = self.project().and_then(|p| p.layers.iter().find(|l| l.id == id)).map(|l| l.enabled) else { return };
        self.commit(vec![editing::set_field(id, "enabled", serde_json::json!(!enabled))], format!("{} {id}", if enabled { "disable" } else { "enable" }));
    }

    // ---- transport ---------------------------------------------------------------

    fn start_playing(&mut self, frame: u64) {
        let Some(tm) = self.tm else { return };
        let frame = frame.min(tm.last_frame());
        self.playhead = frame;
        self.playing = Some(Playing { clock: PlayClock { fps: tm.fps, frames: tm.frames, looping: self.looping, start_frame: frame }, since: Instant::now(), laps: 0 });
        self.play_audio_from(tm.frame_seconds(frame));
    }

    fn play_audio_from(&mut self, project_seconds: f64) {
        if let Some(a) = self.project().and_then(|p| p.audio.clone()) {
            self.audio.play(a.offset + project_seconds, a.gain);
        }
    }

    pub fn pause(&mut self) {
        self.playing = None;
        self.audio.stop();
    }

    pub fn toggle_play(&mut self) {
        let Some(tm) = self.tm else { return };
        if self.playing.is_some() {
            self.pause();
        } else {
            // Play from the start when parked on the last frame.
            let from = if self.playhead >= tm.last_frame() { 0 } else { self.playhead };
            self.start_playing(from);
        }
    }

    /// Move the playhead. Playback carries on from the new place.
    pub fn seek(&mut self, frame: u64) {
        let Some(tm) = self.tm else { return };
        let frame = frame.min(tm.last_frame());
        if self.playing.is_some() {
            if frame != self.playhead {
                self.start_playing(frame);
            }
        } else {
            self.playhead = frame;
        }
    }

    /// Step by hand: playback stops, as on any editor.
    fn step_to(&mut self, frame: u64) {
        if self.playing.is_some() {
            self.pause();
        }
        self.seek(frame);
    }

    fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
        if self.playing.is_some() {
            self.start_playing(self.playhead);
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        // With Shift first: Ctrl+Z also matches when Shift is held.
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.redo();
        }
        if pressed(Modifiers::COMMAND, Key::Z) {
            self.undo();
        }
        let Some(tm) = self.tm else { return };
        // Counted, so that key repeat on a slow frame still moves one step per press.
        let presses = |m: Modifiers, k: Key| ctx.input_mut(|i| i.count_and_consume_key(m, k)) as u64;
        if pressed(Modifiers::NONE, Key::Space) {
            self.toggle_play();
        }
        // Shift first: a plain arrow also matches when Shift is held.
        let beats = presses(Modifiers::SHIFT, Key::ArrowRight) as i64 - presses(Modifiers::SHIFT, Key::ArrowLeft) as i64;
        let frames = presses(Modifiers::NONE, Key::ArrowRight) as i64 - presses(Modifiers::NONE, Key::ArrowLeft) as i64;
        if beats != 0 {
            let mut frame = self.playhead;
            for _ in 0..beats.unsigned_abs() {
                frame = tm.jump_unit(frame, beats > 0);
            }
            self.step_to(frame);
        }
        if frames != 0 {
            self.step_to(self.playhead.saturating_add_signed(frames));
        }
        if pressed(Modifiers::NONE, Key::Home) {
            self.seek(0);
        }
        if pressed(Modifiers::NONE, Key::End) {
            self.step_to(tm.last_frame());
        }
        if pressed(Modifiers::NONE, Key::L) {
            self.set_looping(!self.looping);
        }
    }

    // ---- per-frame logic -----------------------------------------------------------

    fn blur_wanted(&self) -> bool {
        let project_blurs = self.project().is_some_and(|p| p.settings.motion_blur.samples > 1);
        project_blurs && if self.playing.is_some() { self.blur_playing } else { self.blur_paused }
    }

    fn key(&self, frame: u64) -> FrameKey {
        FrameKey { revision: self.live_revision(), frame, scale: self.scale, blur: self.blur_wanted() }
    }

    /// The revision the preview shows: a pending change's, or the file's.
    fn live_revision(&self) -> u64 {
        self.ghost.as_ref().map_or(self.state.revision, |g| g.revision)
    }

    fn tick(&mut self, ctx: &egui::Context) {
        while let Ok(report) = self.reports.try_recv() {
            self.take_report(report, Origin::Disk);
        }
        while let Ok(event) = self.worker_events.try_recv() {
            self.on_worker(event);
        }
        if self.audio.poll() {
            // The sound arrived while playing: join in at the right place.
            if let Some(p) = &self.playing {
                let pos = p.clock.at(p.since.elapsed().as_secs_f64());
                self.play_audio_from(pos.seconds);
            }
        }
        if let Some(job) = &mut self.export.job {
            if job.running() {
                job.poll();
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }

        // The clock decides where the playhead is, never the renderer.
        if let Some(p) = &mut self.playing {
            let pos = p.clock.at(p.since.elapsed().as_secs_f64());
            self.playhead = pos.frame;
            if pos.ended {
                self.pause();
            } else if pos.laps != p.laps {
                p.laps = pos.laps;
                self.play_audio_from(pos.seconds);
            }
            ctx.request_repaint();
        }
        self.schedule(ctx);

        if self.reloaded_at.is_some_and(|t| t.elapsed() < FLASH) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn on_worker(&mut self, event: WorkerEvent) {
        let current = self.live_revision();
        match event {
            WorkerEvent::Adapter(info) => self.adapter = Some(info),
            WorkerEvent::NoAdapter(e) => self.no_adapter = Some(e),
            WorkerEvent::Prepared { .. } => {}
            WorkerEvent::PrepareFailed { revision, error } => {
                if revision == current {
                    self.prepare_error = Some(error);
                }
            }
            WorkerEvent::Frame { key, image, ms } => {
                if self.posted == Some(key) {
                    self.posted = None;
                }
                if key.revision == current {
                    let bytes = image.pixels.len() * 4;
                    self.cache.insert(key, image, bytes);
                    self.last_render = Some((key, ms));
                    if (key.scale, key.blur) == self.average_for {
                        self.average.push(ms);
                    }
                }
            }
            WorkerEvent::RenderFailed { key, error } => {
                if self.posted == Some(key) {
                    self.posted = None;
                }
                if key.revision == current {
                    self.failed = Some((key, error));
                }
            }
            WorkerEvent::Skipped { key } => {
                if self.posted == Some(key) {
                    self.posted = None;
                }
            }
        }
    }

    /// Decide which cached picture to show and which frame to ask for next.
    fn schedule(&mut self, ctx: &egui::Context) {
        let Some(tm) = self.tm else { return };
        let here = self.key(self.playhead);
        let quality = (here.scale, here.blur);
        if self.average_for != quality {
            self.average_for = quality;
            self.average.reset();
        }
        let at = |frame: u64| FrameKey { frame, ..here };

        let (show, render) = if self.playing.is_some() {
            let cache = &self.cache;
            let cached = |f: u64| cache.contains(&at(f));
            let show = newest_ready(self.playhead, tm.fps.ceil() as u64, cached).map(at);
            // Read ahead up to three seconds, but never more than half of what the cache holds.
            let per_frame = self.cache.bytes().checked_div(self.cache.len()).unwrap_or(0).max(1);
            let window = ((tm.fps * 3.0) as u64).min((self.cache.budget() / per_frame / 2) as u64).max(1);
            let lead = lead_frames(self.average.get(), tm.fps);
            let render = next_request(self.playhead, lead, tm.frames, self.looping, window, cached).map(at);
            (show, render)
        } else if self.cache.contains(&here) {
            (Some(here), None)
        } else {
            // Until the exact picture is ready, the same frame at the other blur setting is closer than an old frame.
            let other = FrameKey { blur: !here.blur, ..here };
            (self.cache.contains(&other).then_some(other), Some(here))
        };

        if let Some(key) = show {
            if self.shown != Some(key) {
                if let Some(image) = self.cache.get(&key).cloned() {
                    match &mut self.texture {
                        Some(t) => t.set(image, TextureOptions::LINEAR),
                        None => self.texture = Some(ctx.load_texture("preview", image, TextureOptions::LINEAR)),
                    }
                    self.shown = Some(key);
                }
            }
        }
        if let Some(key) = render {
            let can_render = self.no_adapter.is_none() && self.prepare_error.is_none();
            let failed_before = self.failed.as_ref().is_some_and(|(k, _)| *k == key);
            if can_render && !failed_before && self.posted != Some(key) {
                self.worker.request(key);
                self.posted = Some(key);
            }
        }
    }

    // ---- panels ----------------------------------------------------------------------

    fn transport(&mut self, ui: &mut Ui) {
        let Some(tm) = self.tm else {
            ui.add_space(6.0);
            ui.label(format!("{} is not a valid project yet. The window will update when the file changes.", self.path.display()));
            ui.add_space(6.0);
            return;
        };
        ui.add_space(5.0);
        // Wrapped: in a narrow window the preview controls move to a second line instead of overlapping.
        ui.horizontal_wrapped(|ui| {
            if icon_button(ui, Icon::Start, false).on_hover_text("Go to the start (Home)").clicked() {
                self.seek(0);
            }
            if icon_button(ui, Icon::StepBack, false).on_hover_text("Back one frame (Left). Shift+Left: back one beat").clicked() {
                self.step_to(self.playhead.saturating_sub(1));
            }
            let icon = if self.playing.is_some() { Icon::Pause } else { Icon::Play };
            if icon_button(ui, icon, self.playing.is_some()).on_hover_text("Play or pause (Space)").clicked() {
                self.toggle_play();
            }
            if icon_button(ui, Icon::StepForward, false).on_hover_text("Forward one frame (Right). Shift+Right: forward one beat").clicked() {
                self.step_to(self.playhead.saturating_add(1));
            }
            if ui.selectable_label(self.looping, "Loop").on_hover_text("Start again at the end (L)").clicked() {
                self.set_looping(!self.looping);
            }
            ui.separator();
            let digits = tm.last_frame().to_string().len();
            let mut readout = format!("frame {:>digits$} / {}   {} s", self.playhead, tm.last_frame(), fmt_seconds(tm.frame_seconds(self.playhead)));
            if tm.uses_beats() {
                readout.push_str(&format!("   beat {:>5.2}", tm.frame_time(self.playhead)));
            }
            ui.label(RichText::new(readout).monospace().size(13.0));
            ui.separator();

            ui.label("Preview scale");
            for s in Scale::ALL {
                if ui.selectable_label(self.scale == s, s.label()).on_hover_text("Preview size relative to the project's resolution").clicked() {
                    self.scale = s;
                }
            }
            ui.separator();
            let blurs = self.project().is_some_and(|p| p.settings.motion_blur.samples > 1);
            let hint = if blurs { "Render the preview with the project's motion blur" } else { "This project has no motion blur (settings.motion_blur.samples is 1)" };
            ui.label("Motion blur");
            ui.add_enabled(blurs, egui::Checkbox::new(&mut self.blur_paused, "paused")).on_hover_text(hint).on_disabled_hover_text(hint);
            ui.add_enabled(blurs, egui::Checkbox::new(&mut self.blur_playing, "playing")).on_hover_text(hint).on_disabled_hover_text(hint);
            ui.separator();
            if ui.button("Export…").on_hover_text("Render the project to a video file with `edits render`").clicked() {
                self.export.open = true;
            }
        });
        ui.add_space(2.0);
        let mut frame = self.playhead;
        ui.spacing_mut().slider_width = (ui.available_width() - 4.0).max(60.0);
        if ui.add(egui::Slider::new(&mut frame, 0..=tm.last_frame()).show_value(false)).changed() {
            self.seek(frame);
        }
        ui.add_space(3.0);
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.add_space(2.0);
        // Wrapped for the same reason as the transport. What the agent just did comes first.
        ui.horizontal_wrapped(|ui| {
            if self.state.rejected {
                let shown = if self.state.current.is_some() { format!("showing revision {}", self.state.revision) } else { "nothing to show yet".to_string() };
                ui.colored_label(pal::BAD, format!("{} error(s) in the file · {shown}", self.state.errors()));
            } else if let Some(t) = self.reloaded_at.filter(|t| t.elapsed() < FLASH) {
                let fade = 1.0 - t.elapsed().as_secs_f32() / FLASH.as_secs_f32();
                egui::Frame::new().fill(pal::GOOD.gamma_multiply(0.15 + 0.5 * fade)).corner_radius(3.0).inner_margin(egui::Margin::symmetric(6, 1)).show(ui, |ui| {
                    ui.label(RichText::new(format!("reloaded · revision {}", self.state.revision)).color(Color32::WHITE).strong());
                });
            } else {
                ui.label(format!("revision {}", self.state.revision)).on_hover_text("Counts the versions of the project file that were loaded; it goes up when a change lands");
            }
            ui.separator();
            if let Some(st) = self.status.as_ref().filter(|st| st.at.elapsed() < if st.error { STATUS_FOR * 2 } else { STATUS_FOR }) {
                let color = if st.error { pal::BAD } else { pal::GOOD };
                let r = ui.colored_label(color, shorten(&st.text, 110));
                let tip = if st.detail.is_empty() { st.text.clone() } else { format!("{}\n\n{}", st.text, st.detail) };
                r.on_hover_text(tip);
                ui.separator();
            }
            match (&self.adapter, &self.no_adapter) {
                (Some(a), _) => {
                    let (kind, color) = if a.software { ("software rasteriser", pal::WARN) } else { ("hardware GPU", pal::GOOD) };
                    ui.label(shorten(&a.name, 40)).on_hover_text(format!("{} · {} · {} · {}", a.name, a.backend, a.device_type, a.driver));
                    ui.colored_label(color, kind);
                }
                (None, Some(e)) => {
                    ui.colored_label(pal::BAD, "no graphics adapter").on_hover_text(e);
                }
                (None, None) => {
                    ui.weak("opening the graphics adapter…");
                }
            }
            ui.separator();
            match self.last_render {
                Some((k, ms)) => {
                    ui.label(format!("render {ms:.0} ms")).on_hover_text(format!("frame {} at scale {}, motion blur {}", k.frame, k.scale.label(), if k.blur { "on" } else { "off" }));
                }
                None => {
                    ui.weak("render –");
                }
            }
            ui.separator();
            ui.label(format!("cache {} frames · {:.0} MB", self.cache.len(), self.cache.bytes() as f64 / 1048576.0))
                .on_hover_text(format!("Rendered preview frames kept in memory (up to {:.0} MB)", self.cache.budget() as f64 / 1048576.0));
            ui.separator();
            match self.audio.status() {
                AudioStatus::None => ui.weak("no audio"),
                AudioStatus::Decoding => ui.weak("audio: decoding…"),
                AudioStatus::Ready { seconds } => ui.label(format!("audio {seconds:.1} s")),
                AudioStatus::Silent(why) => {
                    // The short reason is the part before the colon; the rest is in the tooltip.
                    let short = shorten(why.split(':').next().unwrap_or(&why), 44);
                    ui.colored_label(pal::WARN, format!("audio: silent ({short})")).on_hover_text(format!("{why}\nPlayback carries on without sound."))
                }
            };
            if let Some(job) = &self.export.job {
                ui.separator();
                let color = match &job.finished {
                    Some(Ok(_)) => pal::GOOD,
                    Some(Err(_)) => pal::BAD,
                    None => pal::WARN,
                };
                ui.colored_label(color, shorten(&job.status(), 90)).on_hover_text(job.status());
            }
        });
        ui.add_space(2.0);
    }

    fn preview(&mut self, ui: &mut Ui) {
        let area = ui.available_rect_before_wrap();
        let painter = ui.painter_at(area);
        let Some(project) = self.project().cloned() else {
            painter.text(area.center(), Align2::CENTER_CENTER, "Waiting for a valid project.\nThe problems are listed on the right.", FontId::proportional(16.0), Color32::GRAY);
            return;
        };
        let s = &project.settings;
        let aspect = s.width as f32 / s.height.max(1) as f32;
        let inner = area.shrink(10.0);
        let (w, h) = if inner.width() / inner.height().max(1.0) > aspect { (inner.height() * aspect, inner.height()) } else { (inner.width(), inner.width() / aspect) };
        let rect = Rect::from_center_size(inner.center(), egui::vec2(w.max(1.0), h.max(1.0)));
        painter.rect_filled(rect, 0.0, Color32::BLACK);

        let wanted = self.key(self.playhead);
        match (&self.texture, self.shown) {
            (Some(texture), Some(shown)) => {
                painter.image(texture.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                // Say so when the picture is not exactly what the transport reads.
                let mut notes = Vec::new();
                if shown.revision != wanted.revision {
                    notes.push(format!("revision {} (rendering revision {})", revision_name(shown.revision), revision_name(wanted.revision)));
                }
                if shown.frame != wanted.frame {
                    notes.push(format!("showing frame {}", shown.frame));
                }
                if shown.blur != wanted.blur && self.playing.is_none() {
                    notes.push(if wanted.blur { "rendering motion blur…".to_string() } else { "rendering without motion blur…".to_string() });
                }
                if !notes.is_empty() {
                    chip(&painter, rect.left_bottom() + egui::vec2(8.0, -8.0), Align2::LEFT_BOTTOM, &notes.join(" · "), Color32::from_black_alpha(170), Color32::from_gray(220));
                }
            }
            _ => {
                let text = if self.no_adapter.is_some() { "no picture: there is no graphics adapter" } else { "rendering…" };
                painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(15.0), Color32::GRAY);
            }
        }

        let mut top = rect.left_top() + egui::vec2(8.0, 8.0);
        let mut banner = |text: &str, fill: Color32| {
            let r = chip(&painter, top, Align2::LEFT_TOP, text, fill, Color32::WHITE);
            top.y = r.bottom() + 4.0;
        };
        if self.state.rejected {
            banner(&format!("The file has {} error(s): this is still revision {}, the last version without errors.", self.state.errors(), self.state.revision), Color32::from_rgb(150, 40, 35));
        }
        if let Some(e) = &self.no_adapter {
            banner(&shorten(e, 140), Color32::from_rgb(150, 40, 35));
        }
        if let Some(e) = &self.prepare_error {
            banner(&format!("The renderer could not load this version: {}", shorten(e, 140)), Color32::from_rgb(150, 40, 35));
        }
        if let Some((k, e)) = &self.failed {
            banner(&format!("Frame {} did not render: {}", k.frame, shorten(e, 140)), Color32::from_rgb(150, 40, 35));
        }

        if let Some(t) = self.reloaded_at.filter(|t| t.elapsed() < FLASH) {
            let fade = (1.0 - t.elapsed().as_secs_f32() / FLASH.as_secs_f32()).clamp(0.0, 1.0);
            painter.rect_stroke(rect.expand(2.0), 2.0, Stroke::new(3.0, pal::GOOD.gamma_multiply(fade)), StrokeKind::Outside);
            chip(&painter, rect.right_top() + egui::vec2(-8.0, 8.0), Align2::RIGHT_TOP, "reloaded", pal::GOOD.gamma_multiply(0.35 + 0.5 * fade), Color32::WHITE);
        }
    }

    pub fn open_export(&mut self) {
        self.export.open = true;
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        if !self.export.open {
            return;
        }
        let edits = self.export.edits.get_or_insert_with(export::locate_edits).clone();
        let mut open = true;
        let mut start = false;
        egui::Window::new("Export video").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("Renders the project file as it is on disk, at full size and quality, to H.264 with its audio.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Output file");
                ui.add(egui::TextEdit::singleline(&mut self.export.path).desired_width(440.0));
            });
            match &edits {
                Some(bin) => {
                    ui.weak(format!("runs: {} render {} -o …", bin.display(), self.path.display()));
                }
                None => {
                    ui.colored_label(pal::BAD, "The `edits` command was not found next to edits-gui or on PATH. Build it (cargo build --release -p edits-cli) or set EDITS_BIN.");
                }
            }
            ui.add_space(6.0);
            let running = self.export.job.as_ref().is_some_and(Export::running);
            ui.horizontal(|ui| {
                if running {
                    if ui.button("Cancel").clicked() {
                        if let Some(job) = &mut self.export.job {
                            job.cancel();
                        }
                    }
                    if let Some((done, total)) = self.export.job.as_ref().and_then(|j| j.progress) {
                        ui.add(egui::ProgressBar::new(done as f32 / total.max(1) as f32).desired_width(300.0).show_percentage());
                    } else {
                        ui.spinner();
                    }
                } else if ui.add_enabled(edits.is_some() && !self.export.path.trim().is_empty(), egui::Button::new("Render")).clicked() {
                    start = true;
                }
            });
            if let Some(job) = &self.export.job {
                ui.label(job.status());
            }
            if let Some(e) = &self.export.error {
                ui.colored_label(pal::BAD, e);
            }
        });
        if start {
            if let Some(bin) = edits {
                let wake = ctx.clone();
                match Export::start(&bin, &self.path, std::path::Path::new(self.export.path.trim()), move || wake.request_repaint()) {
                    Ok(job) => {
                        self.export.job = Some(job);
                        self.export.error = None;
                    }
                    Err(e) => self.export.error = Some(e),
                }
            }
        }
        if !open {
            self.export.open = false;
        }
    }

    // ---- screenshot mode ---------------------------------------------------------------

    /// The preview shows what was asked for (or it is clear that it never will).
    fn preview_settled(&self) -> bool {
        if !self.audio.settled() {
            return false;
        }
        if self.project().is_none() || self.no_adapter.is_some() || self.prepare_error.is_some() {
            return true;
        }
        let wanted = self.key(self.playhead);
        if self.failed.as_ref().is_some_and(|(k, _)| k.revision == wanted.revision) {
            return true;
        }
        if self.playing.is_some() {
            return self.shown.is_some_and(|k| k.revision == wanted.revision);
        }
        self.shown == Some(wanted)
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let settled = self.preview_settled();
        let delay = self.options.delay;
        let timed_out = self.started.elapsed() > SHOT_TIMEOUT + Duration::from_secs_f64(delay);
        let Some(shot) = &mut self.shot else { return };
        ctx.request_repaint();
        match shot.phase {
            ShotPhase::Loading if settled => {
                shot.phase = if delay > 0.0 && !shot.delayed {
                    shot.delayed = true;
                    ShotPhase::Delay(Instant::now() + Duration::from_secs_f64(delay))
                } else {
                    ShotPhase::Settle(3)
                };
            }
            ShotPhase::Delay(until) if Instant::now() >= until => shot.phase = ShotPhase::Loading,
            ShotPhase::Settle(0) => {
                let (path, outcome, wake) = (shot.path.clone(), self.outcome.clone(), ctx.clone());
                ctx.request_screenshot(move |image| {
                    let result = save_png(&path, &image);
                    if let Ok(mut o) = outcome.lock() {
                        *o = Some(result);
                    }
                    wake.request_repaint();
                });
                shot.phase = ShotPhase::Waiting;
            }
            ShotPhase::Settle(n) => shot.phase = ShotPhase::Settle(n - 1),
            ShotPhase::Waiting if self.outcome.lock().map(|o| o.is_some()).unwrap_or(true) => {
                shot.phase = ShotPhase::Closing;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
        if timed_out && !matches!(shot.phase, ShotPhase::Waiting | ShotPhase::Closing) {
            if let Ok(mut o) = self.outcome.lock() {
                *o = Some(Err(format!("timed out waiting for the preview of frame {}", self.playhead)));
            }
            shot.phase = ShotPhase::Closing;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Whose edit an entry of the history was, for a message.
fn who(actor: &str) -> String {
    match actor {
        core_edit::AGENT => "the agent's edit".to_string(),
        core_edit::WINDOW => "your edit".to_string(),
        "" | "unknown" => "an edit".to_string(),
        other => format!("{other}'s edit"),
    }
}

/// How a revision is named on screen: the preview of a change still being made has none yet.
fn revision_name(r: u64) -> String {
    if r >= GHOST_BASE {
        "of the change being made".to_string()
    } else {
        r.to_string()
    }
}

fn save_png(path: &std::path::Path, image: &ColorImage) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let [w, h] = image.size;
    image::save_buffer(path, image.as_raw(), w as u32, h as u32, image::ExtendedColorType::Rgba8).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Start,
    StepBack,
    Play,
    Pause,
    StepForward,
}

/// A transport button. The icons are drawn, not typed: the bundled fonts
/// have no media symbols.
fn icon_button(ui: &mut Ui, icon: Icon, selected: bool) -> egui::Response {
    let wide = matches!(icon, Icon::Play | Icon::Pause);
    let size = egui::vec2(if wide { 46.0 } else { 28.0 }, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let visuals = ui.style().interact_selectable(&response, selected);
    let painter = ui.painter();
    painter.rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    let color = visuals.fg_stroke.color;
    let c = rect.center();
    let tri = |painter: &egui::Painter, cx: f32, dir: f32, half: f32| {
        // A triangle pointing right (dir 1) or left (dir -1), centred on cx.
        let points = vec![egui::pos2(cx - dir * half * 0.8, c.y - half), egui::pos2(cx + dir * half * 0.8, c.y), egui::pos2(cx - dir * half * 0.8, c.y + half)];
        painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    };
    let bar = |painter: &egui::Painter, cx: f32, w: f32, half: f32| {
        painter.rect_filled(Rect::from_center_size(egui::pos2(cx, c.y), egui::vec2(w, half * 2.0)), 0.5, color);
    };
    let chevron = |painter: &egui::Painter, cx: f32, dir: f32| {
        let s = Stroke::new(2.0, color);
        painter.line_segment([egui::pos2(cx - dir * 2.5, c.y - 5.0), egui::pos2(cx + dir * 2.5, c.y)], s);
        painter.line_segment([egui::pos2(cx + dir * 2.5, c.y), egui::pos2(cx - dir * 2.5, c.y + 5.0)], s);
    };
    match icon {
        Icon::Start => {
            bar(painter, c.x - 5.0, 2.0, 5.5);
            tri(painter, c.x + 1.5, -1.0, 5.5);
        }
        Icon::StepBack => chevron(painter, c.x, -1.0),
        Icon::StepForward => chevron(painter, c.x, 1.0),
        Icon::Play => tri(painter, c.x + 0.5, 1.0, 6.0),
        Icon::Pause => {
            bar(painter, c.x - 3.5, 3.0, 5.5);
            bar(painter, c.x + 3.5, 3.0, 5.5);
        }
    }
    response
}

/// Text on a rounded background, anchored at `pos`. Returns where it ended up.
fn chip(painter: &egui::Painter, pos: egui::Pos2, anchor: Align2, text: &str, fill: Color32, color: Color32) -> Rect {
    let galley = painter.layout(text.to_string(), FontId::proportional(12.5), color, (painter.clip_rect().width() - 40.0).max(80.0));
    let pad = egui::vec2(7.0, 4.0);
    let rect = anchor.anchor_size(pos, galley.size() + pad * 2.0);
    painter.rect_filled(rect, 4.0, fill);
    painter.galley(rect.min + pad, galley, color);
    rect
}

pub(crate) fn shorten(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("");
    if line.chars().count() <= max {
        line.to_string()
    } else {
        let cut: String = line.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (dropped, hovering) = ctx.input(|i| (i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect::<Vec<_>>(), !i.raw.hovered_files.is_empty()));
        self.dropped = dropped;
        self.files_hovering = hovering;
        if hovering {
            // A picture dragged over the window goes on a slot: show them.
            self.tab = Tab::Slots;
        }
        self.keys(&ctx);
        self.tick(&ctx);

        egui::Panel::top("menu").show(ui, |ui| panels::menu_bar(self, ui));
        egui::Panel::top("transport").show(ui, |ui| self.transport(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        let tallest = (ctx.content_rect().height() * 0.6).max(140.0);
        egui::Panel::bottom("timeline").resizable(true).default_size(self.timeline_height).size_range(130.0..=tallest).show(ui, |ui| timeline::show(self, ui));
        egui::Panel::right("inspector").resizable(true).default_size(400.0).min_size(260.0).show(ui, |ui| inspector::show(self, ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_gray(18))).show(ui, |ui| self.preview(ui));

        self.export_window(&ctx);
        // A widget that let go of its value this frame: its change becomes one edit.
        self.finish_gestures();
        if !self.dropped.is_empty() {
            self.say("Drop pictures on a slot in the Slots tab (or type the path there).");
            self.dropped.clear();
        }
        if !self.history.undo.is_empty() || self.status.is_some() {
            // "12 s ago" in the History list, and the status message fading out.
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        self.screenshot_step(&ctx);
    }
}
