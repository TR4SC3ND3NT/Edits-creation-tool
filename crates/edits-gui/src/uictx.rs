//! What the person is looking at, for the agent: the window writes
//! `<project dir>/.edits/ui.json` (format `edits.ui/1`, the type and writer
//! of `edits_core::ui`) and the agent reads it with the MCP tool
//! `ui_context`, so "make *this* hit stronger" can be resolved to a layer
//! and a time.
//!
//! [`context`] builds the content from what the window shows; [`Throttle`]
//! decides when to write it: at once (but at most every [`MIN_GAP`]) when
//! the selection, the paused playhead, the view, the loop or the note
//! change; every [`PLAYING_EVERY`] while playing; at least every
//! [`edits_core::ui::HEARTBEAT`] while the window is open. [`UiFile`]
//! writes it and removes it when the window closes.

use crate::editing::PropRef;
use edits_core::ui::{self, Playhead, Range, Selection, UiContext};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Changes closer together than this are written together.
pub const MIN_GAP: Duration = Duration::from_millis(250);
/// While playing, the playhead is written this often.
pub const PLAYING_EVERY: Duration = Duration::from_secs(2);

/// A note the person typed for the agent, and when (as `edits_core::edit::utc_stamp`).
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    pub at: String,
}

impl Note {
    pub fn new(text: &str, now: SystemTime) -> Option<Note> {
        let text = text.trim();
        (!text.is_empty()).then(|| Note { text: text.to_string(), at: edits_core::edit::utc_stamp(now) })
    }
}

/// What the window shows, in its own terms.
#[derive(Clone, Debug, Default)]
pub struct Inputs<'a> {
    /// File name of the project.
    pub project_file: &'a str,
    /// Project time, seconds and frame of the playhead.
    pub t: f64,
    pub seconds: f64,
    pub frame: u64,
    pub playing: bool,
    /// The selected layer, if a layer is selected.
    pub layer: Option<&'a str>,
    /// The camera is selected.
    pub camera: bool,
    /// The property picked by hand in the inspector.
    pub prop: Option<&'a PropRef>,
    /// Index of that property's key at the playhead.
    pub key: Option<usize>,
    /// The part of the timeline in view, in project time.
    pub visible: Option<(f64, f64)>,
    /// The loop range, when looping.
    pub looping: Option<(f64, f64)>,
    pub note: Option<&'a Note>,
}

/// Numbers in the file: a millionth is plenty, and keeps a view that did not
/// really move from looking like a change.
fn r(v: f64) -> f64 {
    let x = (v * 1e6).round() / 1e6;
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

fn range((from, to): (f64, f64)) -> Range {
    Range { from: r(from), to: r(to) }
}

/// The content of `ui.json` (without `updated`, which the writer stamps).
pub fn context(i: &Inputs) -> UiContext {
    let selection = if i.layer.is_some() || i.camera {
        // A property of another layer kind (the camera's while a layer is selected) is not part of the selection.
        let prop = i.prop.filter(|p| matches!(p, PropRef::Camera(_)) == i.camera);
        let property = match (prop, i.layer) {
            (Some(p), Some(id)) => Some(p.pointer(id)),
            (Some(p @ PropRef::Camera(_)), None) => Some(p.pointer("")),
            _ => None,
        };
        let effect = match prop {
            Some(PropRef::Effect { index, .. }) => Some(*index),
            _ => None,
        };
        Some(Selection { layer: i.layer.map(str::to_string), effect, key: property.as_ref().and(i.key), property })
    } else {
        None
    };
    UiContext {
        format: ui::FORMAT.into(),
        project: i.project_file.to_string(),
        updated: String::new(),
        pid: Some(std::process::id()),
        playhead: Some(Playhead { t: r(i.t), seconds: Some(r(i.seconds)), frame: Some(i.frame) }),
        playing: Some(i.playing),
        selection,
        visible: i.visible.map(range),
        loop_range: i.looping.map(range),
        note: i.note.map(|n| n.text.clone()),
        note_at: i.note.map(|n| n.at.clone()),
    }
}

/// The part of a context whose change is written at once: everything but
/// the stamp, and while playing, the moving playhead and view.
fn significant(c: &UiContext) -> UiContext {
    let mut c = c.clone();
    c.updated.clear();
    if c.playing == Some(true) {
        c.playhead = None;
        c.visible = None;
    }
    c
}

/// What [`Throttle::decide`] says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision {
    Write,
    /// Nothing to write now; look again within this long.
    Wait(Duration),
}

/// When to write: see the module documentation.
#[derive(Default)]
pub struct Throttle {
    last: Option<(Instant, UiContext)>,
}

impl Throttle {
    pub fn decide(&self, now: Instant, ctx: &UiContext) -> Decision {
        let Some((at, last)) = &self.last else { return Decision::Write };
        let since = now.saturating_duration_since(*at);
        let due = |every: Duration| if since >= every { Decision::Write } else { Decision::Wait(every - since) };
        if significant(ctx) != significant(last) {
            return due(MIN_GAP);
        }
        let heartbeat = due(ui::HEARTBEAT);
        let moving = ctx.playing == Some(true) && (ctx.playhead != last.playhead || ctx.visible != last.visible);
        match (moving, due(PLAYING_EVERY), heartbeat) {
            (true, Decision::Write, _) => Decision::Write,
            (true, Decision::Wait(a), Decision::Wait(b)) => Decision::Wait(a.min(b)),
            (_, _, h) => h,
        }
    }

    /// `ctx` was written at `now`.
    pub fn wrote(&mut self, now: Instant, ctx: &UiContext) {
        self.last = Some((now, ctx.clone()));
    }
}

/// The window's `ui.json`: written through [`Throttle`], removed on drop.
pub struct UiFile {
    project: PathBuf,
    throttle: Throttle,
    /// The last write failed: why (shown in the status bar).
    pub error: Option<String>,
    /// The file was written at least once.
    pub written: bool,
}

impl UiFile {
    pub fn new(project: &Path) -> Self {
        UiFile { project: project.to_path_buf(), throttle: Throttle::default(), error: None, written: false }
    }

    pub fn path(&self) -> PathBuf {
        ui::path(&self.project)
    }

    /// Write `ctx` if it is time; returns how long until it should be looked at again.
    pub fn update(&mut self, now: Instant, ctx: &UiContext) -> Duration {
        match self.throttle.decide(now, ctx) {
            Decision::Wait(d) => d,
            Decision::Write => {
                // The `.edits` folder gets its .gitignore the first time.
                let _ = edits_core::edit::state_dir(&edits_core::edit::project_dir(&self.project), "");
                match ui::write(&self.project, ctx) {
                    Ok(()) => {
                        self.error = None;
                        self.written = true;
                    }
                    Err(e) => self.error = Some(format!("cannot write {}: {e}", self.path().display())),
                }
                // A failed write is not retried at once either.
                self.throttle.wrote(now, ctx);
                MIN_GAP.min(ui::HEARTBEAT)
            }
        }
    }

    /// Remove the file if this window wrote it (another window on the same project may have written it since).
    pub fn remove(&mut self) {
        if !self.written {
            return;
        }
        self.written = false;
        let ours = match ui::read(&self.project) {
            Ok(ui::Found::Open { ctx, .. }) => ctx.pid.is_none_or(|p| p == std::process::id()),
            Ok(ui::Found::Closed) => false,
            Err(_) => true,
        };
        if ours {
            let _ = std::fs::remove_file(self.path());
        }
    }
}

impl Drop for UiFile {
    fn drop(&mut self) {
        self.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs<'static> {
        Inputs { project_file: "project.json", t: 4.25, seconds: 2.0732, frame: 51, ..Default::default() }
    }

    #[test]
    fn the_content_names_the_selection_and_the_playhead() {
        let prop = PropRef::Effect { index: 1, kind: "glow".into(), param: "radius".into() };
        let note = Note { text: "make this hit stronger".into(), at: "20261113T115930.000Z".into() };
        let i = Inputs { layer: Some("cut5"), prop: Some(&prop), key: Some(2), visible: Some((0.0, 16.0)), looping: Some((0.0, 8.0)), note: Some(&note), ..inputs() };
        let c = context(&i);
        assert_eq!(c.format, "edits.ui/1");
        assert_eq!(c.project, "project.json");
        assert_eq!(c.playhead, Some(Playhead { t: 4.25, seconds: Some(2.0732), frame: Some(51) }));
        let s = c.selection.clone().unwrap();
        assert_eq!(s.layer.as_deref(), Some("cut5"));
        assert_eq!(s.property.as_deref(), Some("/layers/@cut5/effects/1/params/radius"));
        assert_eq!((s.effect, s.key), (Some(1), Some(2)));
        assert_eq!(c.visible, Some(Range { from: 0.0, to: 16.0 }));
        assert_eq!(c.loop_range, Some(Range { from: 0.0, to: 8.0 }));
        assert_eq!(c.note.as_deref(), Some("make this hit stronger"));
        assert_eq!(c.note_at.as_deref(), Some("20261113T115930.000Z"));
        // As JSON: the shape `ui_context` reads, `loop` by its name.
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["loop"]["to"], 8.0);
        assert_eq!(v["selection"]["layer"], "cut5");
    }

    #[test]
    fn no_selection_and_a_camera_selection() {
        let c = context(&inputs());
        assert!(c.selection.is_none());
        assert!(c.note.is_none() && c.note_at.is_none() && c.visible.is_none());
        let fov = PropRef::Camera("fov");
        let c = context(&Inputs { camera: true, prop: Some(&fov), ..inputs() });
        let s = c.selection.unwrap();
        assert_eq!((s.layer, s.property.as_deref()), (None, Some("/camera/fov")));
        // A layer's property is not the camera's.
        let scale = PropRef::Transform("scale");
        let c = context(&Inputs { camera: true, prop: Some(&scale), key: Some(0), ..inputs() });
        let s = c.selection.unwrap();
        assert_eq!((s.property, s.key), (None, None));
    }

    #[test]
    fn notes_are_trimmed_and_stamped() {
        assert!(Note::new("   ", SystemTime::UNIX_EPOCH).is_none());
        let n = Note::new("  brighter here \n", SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(n.text, "brighter here");
        assert_eq!(n.at, "19700101T000000.000Z");
    }

    #[test]
    fn changes_are_written_at_once_but_not_more_than_four_times_a_second() {
        let t0 = Instant::now();
        let mut th = Throttle::default();
        let a = context(&inputs());
        assert_eq!(th.decide(t0, &a), Decision::Write);
        th.wrote(t0, &a);
        // Nothing changed: wait for the heartbeat.
        assert_eq!(th.decide(t0 + Duration::from_secs(1), &a), Decision::Wait(Duration::from_secs(4)));
        assert_eq!(th.decide(t0 + Duration::from_secs(5), &a), Decision::Write);
        // A new selection: at once, unless the last write was very recent.
        let b = context(&Inputs { layer: Some("d"), ..inputs() });
        assert_eq!(th.decide(t0 + Duration::from_millis(100), &b), Decision::Wait(Duration::from_millis(150)));
        assert_eq!(th.decide(t0 + Duration::from_millis(300), &b), Decision::Write);
        // The paused playhead is a change too.
        let c = context(&Inputs { frame: 52, t: 4.3, ..inputs() });
        assert_eq!(th.decide(t0 + Duration::from_secs(1), &c), Decision::Write);
        // So is the note.
        let note = Note { text: "x".into(), at: "s".into() };
        let d = context(&Inputs { note: Some(&note), ..inputs() });
        assert_eq!(th.decide(t0 + Duration::from_secs(1), &d), Decision::Write);
    }

    #[test]
    fn while_playing_the_playhead_is_written_every_two_seconds() {
        let t0 = Instant::now();
        let mut th = Throttle::default();
        let playing = |frame: u64, from: f64| context(&Inputs { playing: true, frame, t: frame as f64 / 12.0, visible: Some((from, from + 8.0)), ..inputs() });
        let a = playing(0, 0.0);
        th.wrote(t0, &a);
        // The playhead and a following view move every frame: not a reason to write.
        assert_eq!(th.decide(t0 + Duration::from_millis(500), &playing(12, 0.5)), Decision::Wait(Duration::from_millis(1500)));
        assert_eq!(th.decide(t0 + Duration::from_secs(2), &playing(48, 2.0)), Decision::Write);
        // Pausing is written at once.
        let paused = context(&Inputs { playing: false, frame: 13, t: 13.0 / 12.0, ..inputs() });
        assert_eq!(th.decide(t0 + Duration::from_millis(600), &paused), Decision::Write);
        // A selection made while playing is written at once too.
        let sel = context(&Inputs { playing: true, frame: 14, layer: Some("a"), ..inputs() });
        assert_eq!(th.decide(t0 + Duration::from_millis(600), &sel), Decision::Write);
    }

    #[test]
    fn the_file_is_written_and_removed() {
        let dir = std::env::temp_dir().join(format!("edits-gui-uictx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.join("project.json");
        std::fs::write(&project, "{}").unwrap();
        let mut f = UiFile::new(&project);
        let ctx = context(&Inputs { layer: Some("d"), ..inputs() });
        f.update(Instant::now(), &ctx);
        assert!(f.error.is_none(), "{:?}", f.error);
        match ui::read(&project).unwrap() {
            ui::Found::Open { ctx: got, stale, .. } => {
                assert!(!stale);
                assert_eq!(got.selection.unwrap().layer.as_deref(), Some("d"));
                assert_eq!(got.pid, Some(std::process::id()));
                assert!(!got.updated.is_empty());
            }
            ui::Found::Closed => panic!("not written"),
        }
        assert!(dir.join(".edits/.gitignore").exists());
        drop(f);
        assert!(matches!(ui::read(&project).unwrap(), ui::Found::Closed));
        // Another window's file is left alone.
        let other = UiContext { pid: Some(1), ..ctx };
        ui::write(&project, &other).unwrap();
        let mut f = UiFile::new(&project);
        f.written = true;
        f.remove();
        assert!(matches!(ui::read(&project).unwrap(), ui::Found::Open { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }
}
