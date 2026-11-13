//! What the person is looking at in the preview window, for the agent.
//!
//! The window (`edits-gui`) writes `<project dir>/.edits/ui.json` when its
//! state changes and at least every [`HEARTBEAT`] while it is open, and
//! removes it when it closes; the agent reads it (MCP `ui_context`) to know
//! which layer is selected, where the playhead is and what the person asked
//! for in the note box. Format `edits.ui/1`, described in `docs/AGENTS.md`.
//!
//! Every field but `format`, `project` and `updated` is optional, so the
//! window can start with a few and add more later.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const FORMAT: &str = "edits.ui/1";
/// How often the window rewrites the file while open, at the latest.
pub const HEARTBEAT: Duration = Duration::from_secs(5);
/// A file older than this is from a window that is gone (or hung).
pub const STALE: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct UiContext {
    pub format: String,
    /// File name of the project the window shows (relative to its folder).
    pub project: String,
    /// When the window last wrote this, as `edits_core::edit::utc_stamp` (UTC, `20261113T120000.000Z`).
    pub updated: String,
    /// The window's process id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playhead: Option<Playhead>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    /// The part of the timeline in view, in project time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Range>,
    /// The loop range, when looping.
    #[serde(rename = "loop", default, skip_serializing_if = "Option::is_none")]
    pub loop_range: Option<Range>,
    /// Free text the person typed for the agent ("make this hit stronger").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// When the note was written (same format as `updated`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Playhead {
    /// Project time (beats or seconds, as the project's time base).
    pub t: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Selection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// Index into the layer's effects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<usize>,
    /// The selected property as a pointer `project_edit` accepts, e.g. /layers/@a/transform/scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property: Option<String>,
    /// Index of the selected key of that property.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Range {
    pub from: f64,
    pub to: f64,
}

/// `<project dir>/.edits/ui.json`.
pub fn path(project: &Path) -> PathBuf {
    crate::edit::project_dir(project).join(".edits").join("ui.json")
}

/// Write the context for `project` atomically (what the window does).
pub fn write(project: &Path, ctx: &UiContext) -> std::io::Result<()> {
    let p = path(project);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut ctx = ctx.clone();
    ctx.format = FORMAT.into();
    if ctx.updated.is_empty() {
        ctx.updated = crate::edit::utc_stamp(SystemTime::now());
    }
    let text = serde_json::to_string_pretty(&ctx).map_err(std::io::Error::other)?;
    crate::edit::write_atomic(&p, &format!("{text}\n"))
}

/// What [`read`] found.
#[derive(Clone, Debug)]
pub enum Found {
    /// No file: the window is not open on this project.
    Closed,
    /// The context, how long ago it was written, and whether that is too long
    /// ago for the window to still be open.
    Open { ctx: UiContext, age: Duration, stale: bool },
}

/// Read the context the window wrote for `project`.
pub fn read(project: &Path) -> Result<Found, String> {
    let p = path(project);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Found::Closed),
        Err(e) => return Err(format!("cannot read {}: {e}", p.display())),
    };
    let ctx: UiContext = serde_json::from_str(&text).map_err(|e| format!("{} is not a window context ({e})", p.display()))?;
    if ctx.format != FORMAT {
        return Err(format!("{} has format \"{}\", expected \"{FORMAT}\"", p.display(), ctx.format));
    }
    let written = crate::edit::parse_stamp(&ctx.updated).or_else(|| std::fs::metadata(&p).and_then(|m| m.modified()).ok());
    let age = written.and_then(|w| SystemTime::now().duration_since(w).ok()).unwrap_or_default();
    Ok(Found::Open { stale: age > STALE, age, ctx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_contexts_read_back_and_age() {
        let dir = std::env::temp_dir().join(format!("edits-ui-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.join("project.json");
        assert!(matches!(read(&project).unwrap(), Found::Closed));
        let ctx = UiContext {
            project: "project.json".into(),
            playhead: Some(Playhead { t: 4.25, seconds: Some(2.07), frame: Some(51) }),
            selection: Some(Selection { layer: Some("cut5".into()), property: Some("/layers/@cut5/transform/scale".into()), ..Default::default() }),
            note: Some("make this hit stronger".into()),
            ..Default::default()
        };
        write(&project, &ctx).unwrap();
        match read(&project).unwrap() {
            Found::Open { ctx: got, stale, .. } => {
                assert!(!stale);
                assert_eq!(got.selection.unwrap().layer.as_deref(), Some("cut5"));
                assert_eq!(got.note.as_deref(), Some("make this hit stronger"));
            }
            Found::Closed => panic!("written but not found"),
        }
        let old = UiContext { updated: "20200101T000000.000Z".into(), ..ctx };
        write(&project, &old).unwrap();
        assert!(matches!(read(&project).unwrap(), Found::Open { stale: true, .. }));
        std::fs::remove_dir_all(&dir).ok();
    }
}
