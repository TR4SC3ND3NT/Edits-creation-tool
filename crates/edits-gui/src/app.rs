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
use crate::palette::{self, Command, Palette};
use crate::reload::{self, Applied, LoadReport, ProjectState, WatchHandle};
use crate::rows::{self, Row};
use crate::theme::{self, color, radius, size, space, text};
use crate::timemap::{TimeMap, View};
use crate::toasts::{self, Toasts};
use crate::widgets::{self, tip, Icon};
use crate::worker::{ProjectMsg, Worker, WorkerEvent};
use crate::{inspector, panels, timeline, Options};
use eframe::egui::{self, pos2, vec2, Align2, Color32, ColorImage, FontId, Key, Modifiers, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui};
use edits_core::edit::{self as core_edit, EditOp, EditOptions, HistoryList};
use edits_core::Project;
use edits_render::{AdapterInfo, RenderOptions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the "reloaded" flash stays.
const FLASH: Duration = Duration::from_millis(2500);
/// Screenshot mode gives up after this long (plus the asked delay).
const SHOT_TIMEOUT: Duration = Duration::from_secs(180);
/// Previews of a change still being made (a value being dragged) get
/// revisions from here up, far from the file's own.
const GHOST_BASE: u64 = 1 << 40;
/// How long a message stays in the status bar (an error: twice as long).
const STATUS_FOR: Duration = Duration::from_secs(8);

/// The drawers the rail on the left opens (one at a time, all closed at start).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Drawer {
    Slots,
    History,
}

impl Drawer {
    pub const ALL: [Drawer; 2] = [Drawer::Slots, Drawer::History];

    pub fn title(self) -> &'static str {
        match self {
            Drawer::Slots => "Slots",
            Drawer::History => "History",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Drawer::Slots => Icon::Images,
            Drawer::History => Icon::History,
        }
    }

    pub fn keys(self) -> &'static str {
        match self {
            Drawer::Slots => "Ctrl+1",
            Drawer::History => "Ctrl+2",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Drawer::Slots => "Slots: the pictures of the edit",
            Drawer::History => "History: your edits and the agent's",
        }
    }
}

/// Everything the command palette, the keys and the toolbars can do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Cmd {
    TogglePlay,
    Start,
    End,
    FrameBack,
    FrameForward,
    BeatBack,
    BeatForward,
    ToggleLoop,
    Undo,
    Redo,
    Export,
    Drawer(Drawer),
    AddLayer(NewLayer),
    ToggleLayer(String),
    Duplicate(String),
    MoveLayer(String, bool),
    Delete(String),
    AddEffect(String, &'static str),
    Select(Option<Selected>),
    Seek(u64),
    Scale(Scale),
    BlurPaused,
    BlurPlaying,
    ZoomFit,
    Zoom(f64),
    Shortcuts,
    Palette,
    Quit,
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
    /// The property was picked by hand (not the default): its key strip shows even when it has no keys.
    pub picked_by_hand: bool,
    pending: Option<Pending>,
    ghost: Option<Ghost>,
    ghost_serial: u64,
    /// A drag on the timeline, and the version of the file it started on.
    pub drag: Option<(TimeDrag, Option<u64>)>,
    /// The shared history (the window's and the agent's edits), as on disk.
    pub history: HistoryList,
    pub status: Option<Status>,
    /// The drawer open next to the rail, if any.
    pub drawer: Option<Drawer>,
    pub palette: Palette,
    pub toasts: Toasts,
    shortcuts_open: bool,
    /// Where the preview stage is: toasts sit in its corner.
    stage: Rect,
    /// A zoom asked for by a command: a factor, applied by the timeline about the playhead.
    pub zoom_request: Option<f64>,
    /// The export whose end was already announced.
    export_announced: bool,
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
        theme::apply(&ctx);

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
            picked_by_hand: false,
            pending: None,
            ghost: None,
            ghost_serial: 0,
            drag: None,
            history: HistoryList::default(),
            status: None,
            drawer: None,
            palette: Palette::default(),
            toasts: Toasts::default(),
            shortcuts_open: false,
            stage: Rect::NOTHING,
            zoom_request: None,
            export_announced: false,
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
        let before = std::mem::replace(&mut self.history, core_edit::history_list(&self.path));
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
                    let change = toasts::disk_change(&before, &self.history);
                    let (kind, title, body, action) = toasts::for_disk_change(&change, self.state.revision);
                    self.toasts.push(Instant::now(), kind, title, body, action);
                }
            }
            Applied::Rejected => {
                if origin == Origin::Disk {
                    let n = self.state.errors();
                    let body = if self.state.current.is_some() { format!("Still showing revision {}. The problems are listed in the inspector.", self.state.revision) } else { "The problems are listed in the inspector.".to_string() };
                    self.toasts.push(Instant::now(), toasts::Kind::Error, format!("The file has {n} error{}", if n == 1 { "" } else { "s" }), body, None);
                }
            }
            Applied::Unchanged => {}
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
        self.status = Some(Status { text: text.into(), detail: String::new(), error: false, at: Instant::now() });
    }

    /// An error: a toast, and the status bar (`detail` goes into its tooltip).
    pub fn fail(&mut self, text: impl Into<String>, detail: impl Into<String>) {
        let (text, detail) = (text.into(), detail.into());
        // "Not applied (what): why" reads best as a title and a line.
        let (title, body) = match text.split_once(": ") {
            Some((t, b)) if t.len() < 60 => (t.to_string(), b.to_string()),
            _ => (text.clone(), String::new()),
        };
        self.toasts.push(Instant::now(), toasts::Kind::Error, title, body, None);
        self.status = Some(Status { text, detail, error: true, at: Instant::now() });
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
                self.toasts.push(Instant::now(), toasts::Kind::Info, format!("Undid {}", who(&u.entry.actor)), editing::readable_summary(&u.entry.summary), Some(("Redo".into(), toasts::Action::Redo)));
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
        // The palette works from anywhere, even from a text field.
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::P) || pressed(Modifiers::COMMAND, Key::K) {
            if self.palette.open {
                self.palette.close();
            } else {
                self.palette.open();
            }
        }
        if ctx.egui_wants_keyboard_input() || self.palette.open {
            return;
        }
        // With Shift first: Ctrl+Z also matches when Shift is held.
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.redo();
        }
        if pressed(Modifiers::COMMAND, Key::Z) {
            self.undo();
        }
        if pressed(Modifiers::COMMAND, Key::E) {
            self.open_export();
        }
        if pressed(Modifiers::COMMAND, Key::Num1) {
            self.run(Cmd::Drawer(Drawer::Slots));
        }
        if pressed(Modifiers::COMMAND, Key::Num2) {
            self.run(Cmd::Drawer(Drawer::History));
        }
        if pressed(Modifiers::SHIFT, Key::Questionmark) || pressed(Modifiers::NONE, Key::Questionmark) {
            self.shortcuts_open = !self.shortcuts_open;
        }
        if let Some(id) = self.selected_layer() {
            if pressed(Modifiers::COMMAND, Key::D) {
                self.duplicate_layer(&id);
            }
            if pressed(Modifiers::NONE, Key::Delete) || pressed(Modifiers::NONE, Key::Backspace) {
                self.delete_layer(&id);
            }
        }
        if pressed(Modifiers::NONE, Key::Escape) {
            if self.shortcuts_open {
                self.shortcuts_open = false;
            } else if self.selected.is_some() {
                self.selected = None;
            } else {
                self.drawer = None;
            }
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

    /// Do what a command, a button or a key asks.
    pub fn run(&mut self, cmd: Cmd) {
        let tm = self.tm;
        match cmd {
            Cmd::TogglePlay => self.toggle_play(),
            Cmd::Start => self.seek(0),
            Cmd::End => {
                if let Some(tm) = tm {
                    self.step_to(tm.last_frame());
                }
            }
            Cmd::FrameBack => self.step_to(self.playhead.saturating_sub(1)),
            Cmd::FrameForward => self.step_to(self.playhead.saturating_add(1)),
            Cmd::BeatBack | Cmd::BeatForward => {
                if let Some(tm) = tm {
                    self.step_to(tm.jump_unit(self.playhead, cmd == Cmd::BeatForward));
                }
            }
            Cmd::ToggleLoop => self.set_looping(!self.looping),
            Cmd::Undo => self.undo(),
            Cmd::Redo => self.redo(),
            Cmd::Export => self.open_export(),
            Cmd::Drawer(d) => self.drawer = if self.drawer == Some(d) { None } else { Some(d) },
            Cmd::AddLayer(kind) => self.add_layer(kind),
            Cmd::ToggleLayer(id) => self.toggle_layer(&id),
            Cmd::Duplicate(id) => self.duplicate_layer(&id),
            Cmd::MoveLayer(id, up) => self.move_layer(&id, up),
            Cmd::Delete(id) => self.delete_layer(&id),
            Cmd::AddEffect(id, kind) => {
                if let Some(layer) = self.project().and_then(|p| p.layers.iter().find(|l| l.id == id).cloned()) {
                    self.commit(vec![editing::add_effect_op(&layer, kind)], format!("add {kind} to {id}"));
                }
            }
            Cmd::Select(sel) => self.selected = sel,
            Cmd::Seek(frame) => self.seek(frame),
            Cmd::Scale(s) => self.scale = s,
            Cmd::BlurPaused => self.blur_paused = !self.blur_paused,
            Cmd::BlurPlaying => self.blur_playing = !self.blur_playing,
            Cmd::ZoomFit => self.view_fit = true,
            Cmd::Zoom(f) => self.zoom_request = Some(f),
            Cmd::Shortcuts => self.shortcuts_open = !self.shortcuts_open,
            Cmd::Palette => self.palette.open(),
            Cmd::Quit => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    /// Every command, for the palette: built from what the window shows now.
    fn commands(&self) -> Vec<Command<Cmd>> {
        use Command as C;
        let mut c = vec![
            C::new("Playback", if self.playing.is_some() { Icon::Pause } else { Icon::Play }, if self.playing.is_some() { "Pause" } else { "Play" }, Cmd::TogglePlay).keys("Space"),
            C::new("Playback", Icon::SkipBack, "Go to the start", Cmd::Start).keys("Home"),
            C::new("Playback", Icon::SkipForward, "Go to the last frame", Cmd::End).keys("End"),
            C::new("Playback", Icon::ChevronLeft, "One frame back", Cmd::FrameBack).keys("←"),
            C::new("Playback", Icon::ChevronRight, "One frame forward", Cmd::FrameForward).keys("→"),
            C::new("Playback", Icon::ChevronsLeft, "One beat back", Cmd::BeatBack).keys("Shift+←"),
            C::new("Playback", Icon::ChevronsRight, "One beat forward", Cmd::BeatForward).keys("Shift+→"),
            C::new("Playback", Icon::Repeat, if self.looping { "Turn loop off" } else { "Turn loop on" }, Cmd::ToggleLoop).keys("L"),
        ];
        let undo = self.history.undo.last().map_or("Undo".to_string(), |e| format!("Undo {} ({})", shorten(&e.summary, 40), editing::actor_label(&e.actor)));
        let redo = self.history.redo.last().map_or("Redo".to_string(), |e| format!("Redo {} ({})", shorten(&e.summary, 40), editing::actor_label(&e.actor)));
        c.push(C::new("Edit", Icon::Undo2, undo, Cmd::Undo).keys("Ctrl+Z"));
        c.push(C::new("Edit", Icon::Redo2, redo, Cmd::Redo).keys("Ctrl+Shift+Z"));
        if let Some(p) = self.project() {
            c.push(C::new("Add layer", Icon::Square, "Add a solid layer", Cmd::AddLayer(NewLayer::Solid)));
            c.push(C::new("Add layer", Icon::SlidersHorizontal, "Add an adjustment layer", Cmd::AddLayer(NewLayer::Adjustment)));
            for name in p.slots.keys() {
                c.push(C::new("Add layer", Icon::Image, format!("Add an image layer from slot {name}"), Cmd::AddLayer(NewLayer::Image(name.clone()))));
            }
            for name in p.slots.keys() {
                c.push(C::new("Add layer", Icon::Box, format!("Add a cube with slot {name}"), Cmd::AddLayer(NewLayer::Cube(name.clone()))));
            }
            if let Some(l) = self.selected_layer().and_then(|id| p.layers.iter().find(|l| l.id == id)) {
                let id = l.id.clone();
                c.push(C::new("Layer", if l.enabled { Icon::EyeOff } else { Icon::Eye }, format!("{} {id}", if l.enabled { "Disable" } else { "Enable" }), Cmd::ToggleLayer(id.clone())));
                c.push(C::new("Layer", Icon::Copy, format!("Duplicate {id}"), Cmd::Duplicate(id.clone())).keys("Ctrl+D"));
                c.push(C::new("Layer", Icon::ArrowUp, format!("Move {id} up (to the front)"), Cmd::MoveLayer(id.clone(), true)));
                c.push(C::new("Layer", Icon::ArrowDown, format!("Move {id} down (to the back)"), Cmd::MoveLayer(id.clone(), false)));
                c.push(C::new("Layer", Icon::Trash2, format!("Delete {id}"), Cmd::Delete(id.clone())).keys("Delete"));
                for spec in editing::addable_effects() {
                    c.push(C::new("Add effect", Icon::Sparkles, format!("Add {} to {id}", spec.kind), Cmd::AddEffect(id.clone(), spec.kind)));
                }
            }
            for l in p.layers.iter().rev() {
                c.push(C::new("Select", widgets::kind_icon(&rows::kind_text(&l.kind)), format!("Select {}", l.id), Cmd::Select(Some(Selected::Layer(l.id.clone())))));
            }
            c.push(C::new("Select", Icon::Video, "Select the camera", Cmd::Select(Some(Selected::Camera))));
            if self.selected.is_some() {
                c.push(C::new("Select", Icon::X, "Deselect", Cmd::Select(None)).keys("Esc"));
            }
            if let Some(tm) = self.tm {
                for m in &p.markers {
                    c.push(C::new("Go to", Icon::Flag, format!("Go to marker {}", m.label), Cmd::Seek(tm.first_frame_from(m.t))));
                }
            }
        }
        for d in Drawer::ALL {
            let verb = if self.drawer == Some(d) { "Hide" } else { "Show" };
            c.push(C::new("View", d.icon(), format!("{verb} {}", d.title().to_lowercase()), Cmd::Drawer(d)).keys(d.keys()));
        }
        for s in Scale::ALL {
            c.push(C::new("View", Icon::Gauge, format!("Preview at {}× size", s.label()), Cmd::Scale(s)));
        }
        c.push(C::new("View", Icon::Waves, format!("Motion blur while paused: {}", if self.blur_paused { "turn off" } else { "turn on" }), Cmd::BlurPaused));
        c.push(C::new("View", Icon::Waves, format!("Motion blur while playing: {}", if self.blur_playing { "turn off" } else { "turn on" }), Cmd::BlurPlaying));
        c.push(C::new("View", Icon::Maximize2, "Fit the whole edit in the timeline", Cmd::ZoomFit));
        c.push(C::new("View", Icon::ZoomIn, "Zoom the timeline in", Cmd::Zoom(1.6)));
        c.push(C::new("View", Icon::ZoomOut, "Zoom the timeline out", Cmd::Zoom(1.0 / 1.6)));
        c.push(C::new("View", Icon::Keyboard, "Keyboard shortcuts", Cmd::Shortcuts).keys("?"));
        c.push(C::new("File", Icon::Download, "Export video…", Cmd::Export).keys("Ctrl+E"));
        c.push(C::new("File", Icon::X, "Quit", Cmd::Quit));
        c
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

    /// The centre: the preview on its stage, and the transport row under it.
    fn central(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        let bar = Rect::from_min_max(pos2(full.left(), full.bottom() - size::TRANSPORT), full.right_bottom());
        let stage = Rect::from_min_max(full.left_top(), pos2(full.right(), bar.top()));
        ui.painter().rect_filled(bar, 0, color::BG);
        ui.painter().hline(bar.x_range(), bar.top(), Stroke::new(1.0, color::LINE));
        self.stage = stage;
        self.preview(ui, stage);
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(space::L, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| self.transport(ui));
    }

    /// Timecode on the left, the transport in the middle, loop and preview quality on the right.
    fn transport(&mut self, ui: &mut Ui) {
        let Some(tm) = self.tm else {
            ui.label(RichText::new("Waiting for a valid project").color(color::TEXT_FAINT));
            return;
        };
        let row = ui.max_rect();
        // ---- timecode ----
        let left = ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            let now = timemap_clock(tm.frame_seconds(self.playhead));
            ui.label(RichText::new(now).text_style(theme::timecode_style()).color(color::TEXT)).on_hover_text("Position: minutes, seconds, hundredths");
            ui.label(RichText::new(format!("/ {}", timemap_clock(tm.frame_seconds(tm.last_frame())))).font(theme::mono()).color(color::TEXT_FAINT));
            ui.add_space(space::L);
            let digits = tm.last_frame().to_string().len();
            if tm.uses_beats() {
                ui.label(RichText::new("B").font(theme::small_medium()).color(color::TEXT_FAINT));
                ui.label(RichText::new(format!("{:<6.2}", tm.frame_time(self.playhead))).font(theme::mono()).color(color::TEXT_MUTED)).on_hover_text("Beat under the playhead, counted from 0");
            }
            ui.label(RichText::new("F").font(theme::small_medium()).color(color::TEXT_FAINT));
            ui.label(RichText::new(format!("{:<digits$}", self.playhead)).font(theme::mono()).color(color::TEXT_MUTED)).on_hover_text(format!("Frame under the playhead, of 0 to {}", tm.last_frame()));
        });
        // ---- transport, centred under the picture when there is room ----
        let group_w = 4.0 * size::ICON_BUTTON + 36.0 + 4.0 * 2.0;
        let centred = row.center().x - group_w / 2.0;
        let x0 = if centred > left.response.rect.right() + space::XL { centred } else { left.response.rect.right() + space::XL };
        let group = Rect::from_min_size(pos2(x0, row.top()), vec2(group_w, row.height()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(group).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if tip(widgets::icon_button(ui, Icon::SkipBack), "Go to the start", &["Home"]).clicked() {
                self.run(Cmd::Start);
            }
            if tip(widgets::icon_button(ui, Icon::ChevronLeft), "One frame back (Shift: one beat)", &["←"]).clicked() {
                self.run(Cmd::FrameBack);
            }
            let playing = self.playing.is_some();
            if tip(play_button(ui, playing), if playing { "Pause" } else { "Play" }, &["Space"]).clicked() {
                self.run(Cmd::TogglePlay);
            }
            if tip(widgets::icon_button(ui, Icon::ChevronRight), "One frame forward (Shift: one beat)", &["→"]).clicked() {
                self.run(Cmd::FrameForward);
            }
            if tip(widgets::icon_toggle(ui, Icon::Repeat, self.looping), if self.looping { "Loop is on: playback starts again at the end" } else { "Loop is off" }, &["L"]).clicked() {
                self.run(Cmd::ToggleLoop);
            }
        });
        // ---- preview quality ----
        let right = Rect::from_min_max(pos2(group.right() + space::M, row.top()), row.right_bottom());
        ui.scope_builder(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| self.quality(ui));
    }

    /// The preview-quality button and its popover: scale and motion blur.
    fn quality(&mut self, ui: &mut Ui) {
        let label = format!("{}×", self.scale.label().trim_end_matches(".0"));
        let blurs = self.project().is_some_and(|p| p.settings.motion_blur.samples > 1);
        let galley = ui.painter().layout_no_wrap(label, theme::mono(), color::TEXT);
        let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 52.0, size::ICON_BUTTON), Sense::click());
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
        let fill = if open { color::CONTROL_ACTIVE } else if resp.hovered() { color::CONTROL_HOVER } else { Color32::TRANSPARENT };
        ui.painter().rect_filled(rect, radius::SMALL, fill);
        widgets::paint_icon(ui.painter(), pos2(rect.left() + 14.0, rect.center().y), Icon::MonitorPlay, text::ICON_SMALL, color::TEXT_MUTED);
        ui.painter().galley(pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0), galley, color::TEXT);
        widgets::paint_icon(ui.painter(), pos2(rect.right() - 11.0, rect.center().y), Icon::ChevronDown, 12.0, color::TEXT_FAINT);
        let resp = tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), "Preview quality: size and motion blur", &[]);
        egui::Popup::from_toggle_button_response(&resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::RectAlign::TOP_END)
            .frame(widgets::popover_frame().inner_margin(egui::Margin::same(space::L as i8)))
            .show(|ui| {
                ui.set_width(236.0);
                ui.spacing_mut().item_spacing.y = space::S;
                ui.label(widgets::eyebrow("Preview size"));
                let mut scale = self.scale;
                let options: Vec<(Scale, &str)> = vec![(Scale::Quarter, "0.25×"), (Scale::Half, "0.5×"), (Scale::Full, "1×")];
                if widgets::segmented(ui, &mut scale, &options) {
                    self.scale = scale;
                }
                ui.label(RichText::new("Of the project's resolution. Smaller renders faster.").font(theme::small()).color(color::TEXT_FAINT));
                ui.add_space(space::S);
                ui.label(widgets::eyebrow("Motion blur"));
                let hint = if blurs { "Render the preview with the project's motion blur" } else { "This project has no motion blur (settings.motion_blur.samples is 1)" };
                for (label, value) in [("While paused", &mut self.blur_paused), ("While playing", &mut self.blur_playing)] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label).color(color::TEXT_MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_enabled_ui(blurs, |ui| widgets::switch(ui, value)).response.on_hover_text(hint).on_disabled_hover_text(hint);
                        });
                    });
                }
                if !blurs {
                    ui.label(RichText::new("This project has no motion blur.").font(theme::small()).color(color::TEXT_FAINT));
                } else {
                    ui.label(RichText::new("Off while playing by default: it multiplies the cost of a frame.").font(theme::small()).color(color::TEXT_FAINT));
                }
            });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.x = space::XS;
        let dot = |ui: &mut Ui, c: Color32| {
            let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 3.0, c);
        };
        let small = |t: String, c: Color32| RichText::new(t).font(theme::small()).color(c);
        // What the file is doing comes first.
        if self.state.rejected {
            dot(ui, color::ERROR);
            let shown = if self.state.current.is_some() { format!("showing revision {}", self.state.revision) } else { "nothing to show yet".to_string() };
            ui.label(small(format!("{} error(s) in the file · {shown}", self.state.errors()), color::ERROR_TEXT));
        } else if self.reloaded_at.is_some_and(|t| t.elapsed() < FLASH) {
            dot(ui, color::OK);
            ui.label(small(format!("Reloaded · revision {}", self.state.revision), color::OK_TEXT));
        } else {
            dot(ui, color::CONTROL_ACTIVE);
            ui.label(small(format!("Revision {}", self.state.revision), color::TEXT_FAINT))
                .on_hover_text("Counts the versions of the project file that were loaded; it goes up when a change lands");
        }
        if let Some(st) = self.status.as_ref().filter(|st| st.at.elapsed() < if st.error { STATUS_FOR * 2 } else { STATUS_FOR }) {
            ui.add_space(space::L);
            let c = if st.error { color::ERROR_TEXT } else { color::TEXT_MUTED };
            let tip = if st.detail.is_empty() { st.text.clone() } else { format!("{}\n\n{}", st.text, st.detail) };
            ui.add(egui::Label::new(small(st.text.clone(), c)).truncate()).on_hover_text(tip);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            let gap = |ui: &mut Ui| ui.add_space(space::L);
            if let Some(job) = &self.export.job {
                let c = match &job.finished {
                    Some(Ok(_)) => color::OK_TEXT,
                    Some(Err(_)) => color::ERROR_TEXT,
                    None => color::TEXT_MUTED,
                };
                ui.label(small(shorten(&job.status(), 60), c)).on_hover_text(job.status());
                gap(ui);
            }
            match self.audio.status() {
                AudioStatus::None => ui.label(small("No audio".into(), color::TEXT_FAINT)),
                AudioStatus::Decoding => ui.label(small("Audio decoding…".into(), color::TEXT_FAINT)),
                AudioStatus::Ready { seconds } => ui.label(small(format!("Audio {seconds:.1} s"), color::TEXT_FAINT)),
                AudioStatus::Silent(why) => ui.label(small("Audio silent".into(), color::WARN)).on_hover_text(format!("{why}\nPlayback carries on without sound.")),
            };
            gap(ui);
            ui.label(small(format!("Cache {} · {:.0} MB", self.cache.len(), self.cache.bytes() as f64 / 1048576.0), color::TEXT_FAINT))
                .on_hover_text(format!("Rendered preview frames kept in memory (up to {:.0} MB)", self.cache.budget() as f64 / 1048576.0));
            gap(ui);
            match self.last_render {
                Some((k, ms)) => {
                    ui.label(small(format!("{ms:.0} ms/frame"), color::TEXT_FAINT))
                        .on_hover_text(format!("The last frame took {ms:.0} ms: frame {} at scale {}, motion blur {}", k.frame, k.scale.label(), if k.blur { "on" } else { "off" }));
                }
                None => {
                    ui.label(small("– ms/frame".into(), color::TEXT_FAINT));
                }
            }
            gap(ui);
            match (&self.adapter, &self.no_adapter) {
                (Some(a), _) => {
                    let (kind, c) = if a.software { ("software", color::WARN) } else { ("GPU", color::OK_TEXT) };
                    ui.label(small(kind.into(), c));
                    ui.label(small(shorten(&a.name, 32), color::TEXT_FAINT)).on_hover_text(format!(
                        "{} · {} · {} · {}\n{}",
                        a.name,
                        a.backend,
                        a.device_type,
                        a.driver,
                        if a.software { "A software rasteriser: the CPU renders the preview." } else { "A hardware GPU renders the preview." }
                    ));
                }
                (None, Some(e)) => {
                    ui.label(small("No graphics adapter".into(), color::ERROR_TEXT)).on_hover_text(e);
                }
                (None, None) => {
                    ui.label(small("Opening the graphics adapter…".into(), color::TEXT_FAINT));
                }
            }
        });
    }

    fn preview(&mut self, ui: &mut Ui, area: Rect) {
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0, color::STAGE);
        let Some(project) = self.project().cloned() else {
            let r = Rect::from_center_size(area.center(), vec2(area.width().min(360.0), 160.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                widgets::empty_state(ui, Icon::FileWarning, "Waiting for a valid project", "The problems are listed in the inspector. The window updates when the file changes.", &[]);
            });
            return;
        };
        let s = &project.settings;
        let aspect = s.width as f32 / s.height.max(1) as f32;
        let inner = area.shrink(space::XL);
        let (w, h) = if inner.width() / inner.height().max(1.0) > aspect { (inner.height() * aspect, inner.height()) } else { (inner.width(), inner.width() / aspect) };
        let rect = Rect::from_center_size(inner.center(), vec2(w.max(1.0), h.max(1.0)));
        painter.rect_filled(rect, 0, Color32::BLACK);

        let wanted = self.key(self.playhead);
        match (&self.texture, self.shown) {
            (Some(texture), Some(shown)) => {
                painter.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
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
                    widgets::chip(&painter, rect.left_bottom() + vec2(space::M, -space::M), Align2::LEFT_BOTTOM, &notes.join(" · "), Color32::from_black_alpha(180), color::TEXT_MUTED);
                }
            }
            _ => {
                let text = if self.no_adapter.is_some() { "No picture: there is no graphics adapter" } else { "Rendering…" };
                painter.text(rect.center(), Align2::CENTER_CENTER, text, theme::body(), color::TEXT_FAINT);
            }
        }

        // Problems: a slim banner along the top of the stage, the last good frame stays.
        let mut top = area.top() + space::M;
        let mut banner = |text: &str| {
            let galley = painter.layout(text.to_string(), theme::small(), color::ERROR_TEXT, (area.width() - 80.0).max(120.0));
            let size = galley.size() + vec2(36.0, 12.0);
            let r = Rect::from_min_size(pos2(area.center().x - size.x / 2.0, top), size);
            painter.rect(r, radius::NORMAL, color::ERROR_BG, Stroke::new(1.0, color::ERROR.gamma_multiply(0.35)), StrokeKind::Inside);
            widgets::paint_icon(&painter, pos2(r.left() + 14.0, r.center().y), Icon::CircleAlert, 13.0, color::ERROR_TEXT);
            painter.galley(pos2(r.left() + 26.0, r.top() + 6.0), galley, color::ERROR_TEXT);
            top = r.bottom() + space::XS;
        };
        if self.state.rejected {
            banner(&format!("The file has {} error(s). Showing revision {}, the last version without errors.", self.state.errors(), self.state.revision));
        }
        if let Some(e) = &self.no_adapter {
            banner(&shorten(e, 140));
        }
        if let Some(e) = &self.prepare_error {
            banner(&format!("The renderer could not load this version: {}", shorten(e, 140)));
        }
        if let Some((k, e)) = &self.failed {
            banner(&format!("Frame {} did not render: {}", k.frame, shorten(e, 140)));
        }

        if let Some(t) = self.reloaded_at.filter(|t| t.elapsed() < FLASH) {
            let fade = (1.0 - t.elapsed().as_secs_f32() / FLASH.as_secs_f32()).clamp(0.0, 1.0);
            painter.rect_stroke(rect.expand(1.0), 0, Stroke::new(1.5, color::OK.gamma_multiply(fade)), StrokeKind::Outside);
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
        let mut start = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("export")).backdrop_color(color::BACKDROP).frame(
            egui::Frame::new()
                .fill(color::RAISED)
                .stroke(Stroke::new(1.0, color::CONTROL_HOVER))
                .corner_radius(egui::CornerRadius::same(radius::LARGE))
                .inner_margin(egui::Margin::same(20))
                .shadow(egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(160) }),
        );
        let resp = modal.show(ctx, |ui| {
            ui.set_width(480.0);
            ui.spacing_mut().item_spacing.y = space::M;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Export video").font(FontId::new(text::HEADLINE, theme::strong())).color(color::TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if tip(widgets::small_icon_button(ui, Icon::X), "Close", &["Esc"]).clicked() {
                        close = true;
                    }
                });
            });
            ui.label(RichText::new("Renders the project file as it is on disk, at full size and quality, to H.264 with its audio.").color(color::TEXT_MUTED));
            ui.add_space(space::XS);
            ui.label(widgets::eyebrow("Output file"));
            ui.add(egui::TextEdit::singleline(&mut self.export.path).font(theme::mono()).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
            match &edits {
                Some(bin) => {
                    ui.label(RichText::new(format!("{} render {} -o …", bin.display(), self.path.display())).font(theme::mono_small()).color(color::TEXT_FAINT));
                }
                None => {
                    ui.label(RichText::new("The `edits` command was not found next to edits-gui or on PATH. Build it (cargo build --release -p edits-cli) or set EDITS_BIN.").color(color::ERROR_TEXT));
                }
            }
            let running = self.export.job.as_ref().is_some_and(Export::running);
            if let Some(job) = &self.export.job {
                if running {
                    let frac = job.progress.map_or(0.0, |(done, total)| done as f32 / total.max(1) as f32);
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
                    ui.painter().rect_filled(r, 2, color::CONTROL);
                    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width() * frac, r.height())), 2, color::ACCENT);
                }
                let c = match &job.finished {
                    Some(Ok(_)) => color::OK_TEXT,
                    Some(Err(_)) => color::ERROR_TEXT,
                    None => color::TEXT_MUTED,
                };
                ui.label(RichText::new(job.status()).font(theme::small()).color(c));
            }
            if let Some(e) = &self.export.error {
                ui.label(RichText::new(e).color(color::ERROR_TEXT));
            }
            ui.add_space(space::XS);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if running {
                    if widgets::button(ui, Some(Icon::X), "Cancel", false).clicked() {
                        if let Some(job) = &mut self.export.job {
                            job.cancel();
                        }
                    }
                } else {
                    let ok = edits.is_some() && !self.export.path.trim().is_empty();
                    if ui.add_enabled_ui(ok, |ui| widgets::button(ui, Some(Icon::Download), "Render", true)).inner.clicked() {
                        start = true;
                    }
                    if widgets::button(ui, None, "Close", false).clicked() {
                        close = true;
                    }
                }
            });
        });
        if start {
            if let Some(bin) = edits {
                let wake = ctx.clone();
                match Export::start(&bin, &self.path, std::path::Path::new(self.export.path.trim()), move || wake.request_repaint()) {
                    Ok(job) => {
                        self.export.job = Some(job);
                        self.export.error = None;
                        self.export_announced = false;
                    }
                    Err(e) => self.export.error = Some(e),
                }
            }
        }
        if close || resp.should_close() {
            self.export.open = false;
        }
    }

    /// Tell when an export ends (the dialog may be closed by then).
    fn announce_export(&mut self) {
        let Some(job) = &self.export.job else { return };
        if self.export_announced || job.finished.is_none() {
            return;
        }
        self.export_announced = true;
        let now = Instant::now();
        match &job.finished {
            Some(Ok(seconds)) => {
                let name = job.output.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.toasts.push(now, toasts::Kind::Ok, "Export finished", format!("{name} in {seconds:.1} s · {}", job.output.display()), None);
            }
            Some(Err(e)) if e == "cancelled" => self.toasts.push(now, toasts::Kind::Info, "Export cancelled", "", None),
            Some(Err(e)) => self.toasts.push(now, toasts::Kind::Error, "Export failed", e.clone(), Some(("Show".into(), toasts::Action::OpenExport))),
            None => {}
        }
    }

    fn toast_action(&mut self, action: toasts::Action) {
        match action {
            toasts::Action::UndoEdit(entry) => {
                if self.history.undo.last().is_some_and(|e| e.file == entry.file) {
                    self.undo();
                } else {
                    self.fail("Not undone: newer edits came after it. Undo those first (Ctrl+Z), newest first.", "");
                }
            }
            toasts::Action::Redo => self.redo(),
            toasts::Action::OpenExport => self.open_export(),
        }
    }

    /// The sheet of keyboard shortcuts (`?`).
    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_open {
            return;
        }
        let modal = egui::Modal::new(egui::Id::unique("shortcuts")).backdrop_color(color::BACKDROP).frame(
            egui::Frame::new()
                .fill(color::RAISED)
                .stroke(Stroke::new(1.0, color::CONTROL_HOVER))
                .corner_radius(egui::CornerRadius::same(radius::LARGE))
                .inner_margin(egui::Margin::same(20))
                .shadow(egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(160) }),
        );
        let mut close = false;
        let resp = modal.show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Keyboard shortcuts").font(FontId::new(text::HEADLINE, theme::strong())).color(color::TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if tip(widgets::small_icon_button(ui, Icon::X), "Close", &["Esc"]).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(space::M);
            ui.columns(2, |cols| {
                for (col, groups) in cols.iter_mut().zip([&SHORTCUTS[..3], &SHORTCUTS[3..]]) {
                    for (title, keys) in groups {
                        col.label(widgets::eyebrow(*title));
                        col.add_space(space::XXS);
                        for (k, what) in *keys {
                            col.allocate_ui_with_layout(vec2(col.available_width(), 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.label(RichText::new(*what).color(color::TEXT_MUTED));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_space(space::L);
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    // Right to left: the last alternative, and its last key, first.
                                    for (i, alt) in k.split(" / ").collect::<Vec<_>>().into_iter().rev().enumerate() {
                                        if i > 0 {
                                            ui.label(RichText::new("/").font(theme::small()).color(color::TEXT_FAINT));
                                        }
                                        for key in alt.split('+').rev() {
                                            widgets::kbd(ui, key);
                                        }
                                    }
                                });
                            });
                        }
                        col.add_space(space::L);
                    }
                }
            });
        });
        if close || resp.should_close() {
            self.shortcuts_open = false;
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

/// The play button: a filled square, the one control that is always lit.
fn play_button(ui: &mut Ui, playing: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, size::ICON_BUTTON), Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        color::CONTROL_ACTIVE
    } else if resp.hovered() {
        color::CONTROL_HOVER
    } else {
        color::CONTROL
    };
    ui.painter().rect_filled(rect, radius::SMALL, fill);
    widgets::paint_icon(ui.painter(), rect.center() + vec2(if playing { 0.0 } else { 1.0 }, 0.0), if playing { Icon::Pause } else { Icon::Play }, text::ICON, color::TEXT);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn timemap_clock(seconds: f64) -> String {
    crate::timemap::fmt_clock(seconds)
}

/// The shortcut sheet: (group, [(keys, what)]). Alternatives are separated by " / ".
const SHORTCUTS: [(&str, &[(&str, &str)]); 6] = [
    ("Playback", &[("Space", "Play or pause"), ("← / →", "One frame back / forward"), ("Shift+← / Shift+→", "One beat back / forward"), ("Home / End", "First / last frame"), ("L", "Loop on or off")]),
    ("Edit", &[("Ctrl+Z", "Undo (yours or the agent's)"), ("Ctrl+Shift+Z / Ctrl+Y", "Redo"), ("Ctrl+D", "Duplicate the selected layer"), ("Delete", "Delete the selected layer")]),
    ("Timeline", &[("Ctrl+Wheel", "Zoom about the pointer"), ("Shift+Wheel", "Scroll sideways"), ("Alt", "While dragging: snap to frames")]),
    ("Window", &[("Ctrl+K / Ctrl+Shift+P", "Command palette"), ("Ctrl+1", "Slots"), ("Ctrl+2", "History"), ("Ctrl+E", "Export video"), ("?", "This sheet")]),
    ("Selection", &[("Esc", "Deselect, then close the drawer")]),
    ("Values", &[("Drag", "Change a value"), ("Click", "Type a value; Enter finishes")]),
];

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
            self.drawer = Some(Drawer::Slots);
        }
        self.keys(&ctx);
        self.tick(&ctx);
        self.announce_export();

        let bar = |fill: Color32, x: i8| egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(x, 0));
        egui::Panel::top("top-bar").exact_size(size::TOP_BAR).resizable(false).frame(bar(color::BG, space::L as i8)).show(ui, |ui| panels::top_bar(self, ui));
        egui::Panel::bottom("status").exact_size(size::STATUS_BAR).resizable(false).frame(bar(color::BG, space::L as i8)).show(ui, |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| self.status_bar(ui));
        });
        let tallest = (ctx.content_rect().height() * 0.6).max(160.0);
        egui::Panel::bottom("timeline").resizable(true).default_size(self.timeline_height).size_range(150.0..=tallest).frame(egui::Frame::new().fill(color::BG)).show(ui, |ui| timeline::show(self, ui));
        egui::Panel::left("rail").exact_size(size::RAIL).resizable(false).frame(egui::Frame::new().fill(color::BG)).show(ui, |ui| panels::rail(self, ui));
        if let Some(d) = self.drawer {
            egui::Panel::left("drawer").resizable(true).default_size(size::DRAWER).size_range(240.0..=460.0).frame(egui::Frame::new().fill(color::BG)).show(ui, |ui| panels::drawer(self, ui, d));
        }
        egui::Panel::right("inspector").resizable(true).default_size(size::INSPECTOR).size_range(280.0..=560.0).frame(egui::Frame::new().fill(color::BG)).show(ui, |ui| inspector::show(self, ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(color::STAGE)).show(ui, |ui| self.central(ui));

        self.export_window(&ctx);
        self.shortcuts_window(&ctx);
        if self.palette.open {
            let commands = self.commands();
            if let Some(cmd) = palette::show(&ctx, &mut self.palette, &commands) {
                self.run(cmd);
            }
        }
        if let Some(action) = toasts::show(&ctx, self.stage, &mut self.toasts) {
            self.toast_action(action);
        }
        // A widget that let go of its value this frame: its change becomes one edit.
        self.finish_gestures();
        if !self.dropped.is_empty() {
            self.say("Drop pictures on a slot in the Slots drawer (or type the path there).");
            self.dropped.clear();
        }
        if !self.history.undo.is_empty() || self.status.is_some() {
            // "12 s ago" in the History list, and the status message fading out.
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        self.screenshot_step(&ctx);
    }
}
