//! The activity log: one line in `<project dir>/.edits/activity.jsonl` when a
//! tool call starts and one when it ends (`edits_core::activity`, format
//! `edits.activity/1`, described in `docs/AGENTS.md`). Edits Studio follows
//! it to show where the agent is working. Writing it never fails a call.
//!
//! `EDITS_ACTIVITY=off` turns it off; `EDITS_ACTIVITY_VIA` is written into
//! every entry as `via`.

use crate::paths::Workspace;
use edits_core::activity::{self, Entry, Phase};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

static SERIAL: AtomicU64 = AtomicU64::new(0);

/// A call in progress: written at its start, finished with [`Call::end`].
pub struct Call {
    log: PathBuf,
    entry: Entry,
    started: Instant,
}

pub fn enabled() -> bool {
    !matches!(std::env::var("EDITS_ACTIVITY").as_deref().map(str::trim), Ok("off" | "0" | "false" | "no"))
}

/// Log the start of `tool` with `args`. The log is next to the project the
/// call names, or in the workspace root for calls that name none (or name a
/// project outside the root, which the tool itself refuses).
pub fn start(ws: &Workspace, tool: &str, args: &Value, client: Option<String>) -> Option<Call> {
    if !enabled() {
        return None;
    }
    let project = args.get("project").and_then(Value::as_str).and_then(|p| ws.file(p, "project").ok());
    let log = match &project {
        Some(p) => activity::path(p),
        None => activity::path_in(ws.root()),
    };
    let mut target = activity::target(tool, args);
    if let Some(doc) = project.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        activity::resolve(&mut target, &doc);
    }
    let entry = Entry {
        at: edits_core::edit::utc_stamp(SystemTime::now()),
        phase: Phase::Start,
        call: format!("{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)),
        tool: tool.to_string(),
        actor: edits_core::edit::AGENT.to_string(),
        label: target.label,
        project: project.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()),
        layers: target.layers,
        pointers: target.pointers,
        time: target.time,
        t: target.t,
        frame: target.frame,
        client,
        via: std::env::var("EDITS_ACTIVITY_VIA").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()),
        ..Default::default()
    };
    let _ = activity::append(&log, &entry);
    Some(Call { log, entry, started: Instant::now() })
}

impl Call {
    /// Log the end: whether it worked, its first error line, and the job it started.
    pub fn end(self, ok: bool, error: Option<String>, job: Option<String>) {
        let mut e = self.entry;
        e.at = edits_core::edit::utc_stamp(SystemTime::now());
        e.phase = Phase::End;
        e.ok = Some(ok);
        e.error = error.map(|t| t.lines().next().unwrap_or("").chars().take(200).collect());
        e.ms = Some(self.started.elapsed().as_millis() as u64);
        e.job = job;
        let _ = activity::append(&self.log, &e);
    }
}
