//! What the person is looking at, for the agent: `<project dir>/.edits/ui.json`
//! (format `edits.ui/1`, written with `edits_core::ui::write`), which the
//! agent reads with the MCP tool `ui_context`. The same content and the same
//! timing as the egui window (`crates/edits-gui/src/uictx.rs`): at once (at
//! most every [`MIN_GAP`]) when the selection, the paused playhead, the view,
//! the loop or the note change; every [`PLAYING_EVERY`] while playing; at
//! least every [`ui::HEARTBEAT`] while the window is open; removed on close.

use edits_core::ui::{self, Playhead, Range, Selection, UiContext};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub const MIN_GAP: Duration = Duration::from_millis(250);
pub const PLAYING_EVERY: Duration = Duration::from_secs(2);

/// What the frontend reports.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct UiInputs {
    pub t: f64,
    pub seconds: f64,
    pub frame: u64,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub layer: Option<String>,
    #[serde(default)]
    pub camera: bool,
    /// The property picked in the inspector, as a pointer `project_edit` accepts.
    #[serde(default)]
    pub property: Option<String>,
    #[serde(default)]
    pub effect: Option<usize>,
    #[serde(default)]
    pub key: Option<usize>,
    #[serde(default)]
    pub visible: Option<(f64, f64)>,
    #[serde(default)]
    pub looping: Option<(f64, f64)>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub note_at: Option<String>,
}

fn r(v: f64) -> f64 {
    let x = (v * 1e6).round() / 1e6;
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

fn range((from, to): (f64, f64)) -> Range {
    Range {
        from: r(from),
        to: r(to),
    }
}

/// The content of `ui.json` (without `updated`, which the writer stamps).
pub fn context(project_file: &str, i: &UiInputs) -> UiContext {
    let selection = (i.layer.is_some() || i.camera).then(|| Selection {
        layer: i.layer.clone(),
        effect: i.property.as_ref().and(i.effect),
        key: i.property.as_ref().and(i.key),
        property: i.property.clone(),
    });
    UiContext {
        format: ui::FORMAT.into(),
        project: project_file.to_string(),
        updated: String::new(),
        pid: Some(std::process::id()),
        playhead: Some(Playhead {
            t: r(i.t),
            seconds: Some(r(i.seconds)),
            frame: Some(i.frame),
        }),
        playing: Some(i.playing),
        selection,
        visible: i.visible.map(range),
        loop_range: i.looping.map(range),
        note: i.note.clone().filter(|n| !n.trim().is_empty()),
        note_at: i.note.as_ref().filter(|n| !n.trim().is_empty()).map(|_| {
            i.note_at
                .clone()
                .unwrap_or_else(|| edits_core::edit::utc_stamp(SystemTime::now()))
        }),
    }
}

fn significant(c: &UiContext) -> UiContext {
    let mut c = c.clone();
    c.updated.clear();
    if c.playing == Some(true) {
        c.playhead = None;
        c.visible = None;
    }
    c
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision {
    Write,
    Wait(Duration),
}

#[derive(Default)]
pub struct Throttle {
    last: Option<(Instant, UiContext)>,
}

impl Throttle {
    pub fn decide(&self, now: Instant, ctx: &UiContext) -> Decision {
        let Some((at, last)) = &self.last else {
            return Decision::Write;
        };
        let since = now.saturating_duration_since(*at);
        let due = |every: Duration| {
            if since >= every {
                Decision::Write
            } else {
                Decision::Wait(every - since)
            }
        };
        if significant(ctx) != significant(last) {
            return due(MIN_GAP);
        }
        let heartbeat = due(ui::HEARTBEAT);
        let moving = ctx.playing == Some(true)
            && (ctx.playhead != last.playhead || ctx.visible != last.visible);
        match (moving, due(PLAYING_EVERY), heartbeat) {
            (true, Decision::Write, _) => Decision::Write,
            (true, Decision::Wait(a), Decision::Wait(b)) => Decision::Wait(a.min(b)),
            (_, _, h) => h,
        }
    }

    pub fn wrote(&mut self, now: Instant, ctx: &UiContext) {
        self.last = Some((now, ctx.clone()));
    }
}

/// The window's `ui.json` for one project.
pub struct UiFile {
    project: PathBuf,
    throttle: Throttle,
    /// The newest content, rewritten by the heartbeat.
    pub latest: Option<UiContext>,
    pub error: Option<String>,
    pub written: bool,
}

impl UiFile {
    pub fn new(project: &Path) -> Self {
        UiFile {
            project: project.to_path_buf(),
            throttle: Throttle::default(),
            latest: None,
            error: None,
            written: false,
        }
    }

    pub fn path(&self) -> PathBuf {
        ui::path(&self.project)
    }

    pub fn set(&mut self, ctx: UiContext) {
        self.latest = Some(ctx);
    }

    /// Write the latest content if it is time.
    pub fn tick(&mut self, now: Instant) {
        let Some(ctx) = self.latest.clone() else {
            return;
        };
        if self.throttle.decide(now, &ctx) != Decision::Write {
            return;
        }
        let _ = edits_core::edit::state_dir(&edits_core::edit::project_dir(&self.project), "");
        match ui::write(&self.project, &ctx) {
            Ok(()) => {
                self.error = None;
                self.written = true;
            }
            Err(e) => self.error = Some(format!("cannot write {}: {e}", self.path().display())),
        }
        self.throttle.wrote(now, &ctx);
    }

    /// Remove the file if this window wrote it.
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

    #[test]
    fn the_content_names_the_selection_and_the_playhead() {
        let i = UiInputs {
            t: 4.25,
            seconds: 2.0732,
            frame: 51,
            layer: Some("cut5".into()),
            property: Some("/layers/@cut5/effects/1/params/radius".into()),
            effect: Some(1),
            key: Some(2),
            visible: Some((0.0, 16.0)),
            note: Some("make this hit stronger".into()),
            note_at: Some("20261113T115930.000Z".into()),
            ..Default::default()
        };
        let c = context("project.json", &i);
        assert_eq!(c.format, "edits.ui/1");
        assert_eq!(
            c.playhead,
            Some(Playhead {
                t: 4.25,
                seconds: Some(2.0732),
                frame: Some(51)
            })
        );
        let s = c.selection.unwrap();
        assert_eq!(
            (s.layer.as_deref(), s.effect, s.key),
            (Some("cut5"), Some(1), Some(2))
        );
        assert_eq!(c.note_at.as_deref(), Some("20261113T115930.000Z"));
        let none = context("project.json", &UiInputs::default());
        assert!(none.selection.is_none() && none.note.is_none());
    }

    #[test]
    fn writes_at_once_on_change_and_on_a_heartbeat_otherwise() {
        let mut th = Throttle::default();
        let t0 = Instant::now();
        let a = context(
            "p.json",
            &UiInputs {
                t: 1.0,
                ..Default::default()
            },
        );
        assert_eq!(th.decide(t0, &a), Decision::Write);
        th.wrote(t0, &a);
        assert!(matches!(
            th.decide(t0 + Duration::from_secs(1), &a),
            Decision::Wait(_)
        ));
        assert_eq!(th.decide(t0 + ui::HEARTBEAT, &a), Decision::Write);
        let b = context(
            "p.json",
            &UiInputs {
                t: 2.0,
                ..Default::default()
            },
        );
        assert!(
            matches!(
                th.decide(t0 + Duration::from_millis(100), &b),
                Decision::Wait(_)
            ),
            "not closer than MIN_GAP"
        );
        assert_eq!(th.decide(t0 + MIN_GAP, &b), Decision::Write);
    }

    #[test]
    fn the_file_is_written_and_removed() {
        let dir = std::env::temp_dir().join(format!("edits-studio-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.join("project.json");
        std::fs::write(&project, "{}").unwrap();
        let mut f = UiFile::new(&project);
        f.set(context(
            "project.json",
            &UiInputs {
                layer: Some("d".into()),
                ..Default::default()
            },
        ));
        f.tick(Instant::now());
        match ui::read(&project).unwrap() {
            ui::Found::Open { ctx, stale, .. } => {
                assert!(!stale);
                assert_eq!(ctx.selection.unwrap().layer.as_deref(), Some("d"));
            }
            ui::Found::Closed => panic!("not written"),
        }
        drop(f);
        assert!(matches!(ui::read(&project).unwrap(), ui::Found::Closed));
        std::fs::remove_dir_all(&dir).ok();
    }
}
