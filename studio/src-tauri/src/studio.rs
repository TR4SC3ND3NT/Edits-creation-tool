//! The studio's state: the open project, the render thread, `ui.json`, and
//! the one way the window changes the file (`edits_core::edit`, actor
//! `window`, with the hash of the version the edit was prepared against).

use crate::engine::reload::{self, Applied, LoadReport};
use crate::project::{Open, Reason, Snapshot};
use crate::render::{ProjectMsg, RenderService};
use crate::renders::RenderJob;
use crate::uictx::UiFile;
use edits_core::edit::{self, EditOp, EditOptions};
use edits_core::Project;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// Sends an event to the frontend: (name, payload).
pub type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub struct Studio {
    pub open: Mutex<Option<Open>>,
    pub render: RenderService,
    pub ui: Mutex<Option<UiFile>>,
    pub job: Mutex<Option<RenderJob>>,
    pub compare: Mutex<crate::reference::CompareRun>,
    pub emit: Emit,
}

/// What a project event carries.
#[derive(Serialize)]
pub struct ProjectEvent<'a> {
    pub snapshot: &'a Snapshot,
    pub reason: Reason,
}

/// What an edit made by the window did.
#[derive(Serialize, Clone, Debug)]
pub struct EditDone {
    pub summary: String,
    /// Warnings `check` found in the result (errors block an edit).
    pub warnings: Vec<String>,
    pub revision: u64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Studio {
    pub fn new(render: RenderService, emit: Emit) -> Arc<Self> {
        Arc::new(Studio {
            open: Mutex::new(None),
            render,
            ui: Mutex::new(None),
            job: Mutex::new(None),
            compare: Mutex::default(),
            emit,
        })
    }

    fn send(&self, snapshot: &Snapshot, reason: Reason) {
        if let Ok(v) = serde_json::to_value(ProjectEvent { snapshot, reason }) {
            (self.emit)("project", v);
        }
    }

    fn to_renderer(&self, open: &Open) {
        if let Some(p) = open.project() {
            self.render.set_project(ProjectMsg {
                revision: open.state.revision,
                project: p.clone(),
                base_dir: open.dir(),
            });
        }
    }

    /// Open a project file (closing the one that was open) and watch it.
    pub fn open_project(self: &Arc<Self>, path: &Path) -> Result<Snapshot, String> {
        if !path.is_file() {
            return Err(format!("project file not found: {}", path.display()));
        }
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let (mut open, report) = Open::new(path.clone());
        let files = report.files.clone();
        open.state.apply(report);
        self.to_renderer(&open);
        let weak = Arc::downgrade(self);
        open.watcher = Some(reload::spawn_watcher(path.clone(), files, move |report| {
            if let Some(s) = weak.upgrade() {
                s.absorb(report);
            }
        }));
        let snapshot = open.snapshot();
        let old = lock(&self.open).replace(open);
        // Dropping the old project stops its watcher (outside the lock: it may be waiting for it).
        drop(old);
        let old_ui = lock(&self.ui).replace(UiFile::new(&path));
        drop(old_ui);
        self.send(&snapshot, Reason::Opened);
        Ok(snapshot)
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        lock(&self.open).as_ref().map(Open::snapshot)
    }

    pub fn path(&self) -> Option<PathBuf> {
        lock(&self.open).as_ref().map(|o| o.path.clone())
    }

    /// The watcher read the file again.
    pub fn absorb(&self, report: LoadReport) {
        let mut g = lock(&self.open);
        let Some(open) = g.as_mut() else { return };
        let was_rejected = open.state.rejected;
        let reason = match open.state.apply(report) {
            Applied::Swapped => {
                self.to_renderer(open);
                open.who_changed()
            }
            Applied::Rejected => Reason::Invalid {
                errors: open.state.errors(),
            },
            Applied::Unchanged if was_rejected => Reason::Problems,
            Applied::Unchanged => {
                // The window's own write coming back, or a write that changed nothing: keep the history current.
                open.mark_own();
                return;
            }
        };
        let snapshot = open.snapshot();
        drop(g);
        self.send(&snapshot, reason);
    }

    /// Read the file back after the window wrote it.
    fn reload_own(&self, open: &mut Open, summary: &str) -> Snapshot {
        if open.state.apply(reload::load(&open.path)) == Applied::Swapped {
            self.to_renderer(open);
        }
        open.mark_own();
        let snapshot = open.snapshot();
        self.send(
            &snapshot,
            Reason::Window {
                summary: summary.to_string(),
            },
        );
        snapshot
    }

    /// Run `f` on the project shown and its JSON, and apply the ops it returns
    /// as one edit of the window.
    pub fn edit(
        &self,
        label: Option<&str>,
        f: impl FnOnce(&Project, &Value) -> Result<Vec<EditOp>, String>,
    ) -> Result<EditDone, String> {
        self.edit_labelled(|p, doc| Ok((f(p, doc)?, label.map(str::to_string))))
    }

    /// Like [`Studio::edit`], with the history line made with the ops.
    pub fn edit_labelled(
        &self,
        f: impl FnOnce(&Project, &Value) -> Result<(Vec<EditOp>, Option<String>), String>,
    ) -> Result<EditDone, String> {
        let mut g = lock(&self.open);
        let open = g.as_mut().ok_or("no project is open")?;
        let (p, doc, hash) = ready(open)?;
        let (ops, label) = f(&p, &doc)?;
        if ops.is_empty() {
            return Err("nothing to change".into());
        }
        let opts = EditOptions {
            actor: edit::WINDOW,
            label: label.as_deref().filter(|l| !l.is_empty()),
            expect: Some(hash),
            ..Default::default()
        };
        let outcome = edit::apply(&open.path, &ops, &opts);
        self.finish(open, outcome)
    }

    /// Like [`Studio::edit`], with the change as a function of the document
    /// as it is on disk (presets, restoring a version).
    pub fn edit_doc(
        &self,
        label: &str,
        change: &dyn Fn(&Value) -> Result<Value, String>,
    ) -> Result<EditDone, String> {
        let mut g = lock(&self.open);
        let open = g.as_mut().ok_or("no project is open")?;
        let (_, _, hash) = ready(open)?;
        let opts = EditOptions {
            actor: edit::WINDOW,
            label: Some(label),
            expect: Some(hash),
            ..Default::default()
        };
        let outcome = edit::apply_with(&open.path, &opts, change);
        self.finish(open, outcome)
    }

    fn finish(
        &self,
        open: &mut Open,
        outcome: Result<edit::EditOutcome, String>,
    ) -> Result<EditDone, String> {
        match outcome {
            Err(e) => {
                if e == edit::STALE {
                    // Someone wrote meanwhile: show what is there now.
                    self.reload_own(open, "reloaded");
                }
                Err(e)
            }
            Ok(o) if !o.ok => {
                let errors: Vec<String> = o
                    .findings
                    .iter()
                    .filter(|f| f.severity == edits_core::check::Severity::Error)
                    .map(edit::finding_line)
                    .collect();
                Err(format!("not applied: {}", errors.join("; ")))
            }
            Ok(o) if !o.written => Err("nothing changed".into()),
            Ok(o) => {
                let snapshot = self.reload_own(open, &o.summary);
                let warnings = o
                    .findings
                    .iter()
                    .filter(|f| f.severity != edits_core::check::Severity::Error)
                    .map(edit::finding_line)
                    .collect();
                Ok(EditDone {
                    summary: o.summary,
                    warnings,
                    revision: snapshot.revision,
                })
            }
        }
    }

    /// Undo (or redo) the newest edit in the shared history, whoever made it.
    pub fn step(&self, redo: bool) -> Result<EditDone, String> {
        let mut g = lock(&self.open);
        let open = g.as_mut().ok_or("no project is open")?;
        let out = if redo {
            edit::redo(&open.path)?
        } else {
            edit::undo(&open.path)?
        };
        let whose = match out.entry.actor.as_str() {
            edit::WINDOW => "your edit".to_string(),
            edit::AGENT => "the agent's edit".to_string(),
            other => format!("the edit by {other}"),
        };
        let summary = format!(
            "{} {whose}: {}",
            if redo { "Redid" } else { "Undid" },
            crate::engine::editing::readable_summary(&out.entry.summary)
        );
        let snapshot = self.reload_own(open, &summary);
        Ok(EditDone {
            summary,
            warnings: Vec::new(),
            revision: snapshot.revision,
        })
    }

    /// Make a saved version (from the history) the file again, as a new edit.
    pub fn restore(&self, version: &Path) -> Result<EditDone, String> {
        let path = self.path().ok_or("no project is open")?;
        let history = edit::project_dir(&path).join(".edits");
        let version = version
            .canonicalize()
            .map_err(|e| format!("cannot read {}: {e}", version.display()))?;
        if !version.starts_with(history.canonicalize().unwrap_or(history)) {
            return Err("only a version under .edits/ next to the project can be restored".into());
        }
        let text = std::fs::read_to_string(&version)
            .map_err(|e| format!("cannot read {}: {e}", version.display()))?;
        let doc: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not JSON: {e}", version.display()))?;
        let name = version
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.edit_doc(&format!("restore the version {name}"), &|_| Ok(doc.clone()))
    }

    pub fn close(&self) {
        if let Some(mut ui) = lock(&self.ui).take() {
            ui.remove();
        }
        if let Some(job) = lock(&self.job).as_mut() {
            job.cancel();
        }
    }
}

/// The project shown, its JSON and its hash, when the window may edit it.
fn ready(open: &Open) -> Result<(Arc<Project>, Value, u64), String> {
    if open.state.rejected {
        return Err(
            "the file on disk has errors, so the window does not edit it; fix them (or undo) first"
                .into(),
        );
    }
    match (open.project(), open.doc(), open.state.text_hash) {
        (Some(p), Some(d), Some(h)) => Ok((p.clone(), d.clone(), h)),
        _ => Err("the project has not loaded".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use edits_render::RenderOptions;
    use serde_json::json;
    use std::sync::mpsc;
    use std::time::Duration;

    fn studio() -> (Arc<Studio>, mpsc::Receiver<(String, Value)>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let emit: Emit = Arc::new(move |name, v| {
            let _ = tx.lock().unwrap().send((name.to_string(), v));
        });
        // A tiny cache; the render thread is not used by these tests.
        (
            Studio::new(
                RenderService::spawn(RenderOptions::default(), 1 << 20),
                emit,
            ),
            rx,
        )
    }

    fn next_project(rx: &mpsc::Receiver<(String, Value)>) -> Value {
        loop {
            let (name, v) = rx.recv_timeout(Duration::from_secs(10)).expect("an event");
            if name == "project" {
                return v;
            }
        }
    }

    #[test]
    fn edits_are_written_as_the_window_and_undone_through_the_shared_history() {
        let path = temp("studio-edit");
        let (s, rx) = studio();
        s.open_project(&path).unwrap();
        assert_eq!(next_project(&rx)["reason"]["kind"], "opened");

        let done = s
            .edit(Some("bpm to 128"), |_, _| {
                Ok(vec![EditOp::set("/settings/bpm", json!(128))])
            })
            .unwrap();
        assert_eq!(done.summary, "bpm to 128");
        let ev = next_project(&rx);
        assert_eq!(ev["reason"]["kind"], "window");
        assert_eq!(ev["snapshot"]["info"]["bpm"], 128.0);
        let list = edit::history_list(&path);
        assert_eq!(
            (list.undo.len(), list.undo[0].actor.as_str()),
            (1, "window")
        );

        // An edit that check refuses is not written.
        let e = s
            .edit(None, |_, _| {
                Ok(vec![EditOp::set("/settings/fps", json!(-1))])
            })
            .unwrap_err();
        assert!(e.starts_with("not applied"), "{e}");

        let undone = s.step(false).unwrap();
        assert_eq!(undone.summary, "Undid your edit: bpm to 128");
        assert_eq!(
            next_project(&rx)["snapshot"]["info"]["bpm"],
            120.0,
            "a refused edit sends no event; this is the undo's"
        );
        s.step(true).unwrap();
        s.close();
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn an_edit_prepared_against_an_old_version_is_refused() {
        let path = temp("studio-stale");
        let (s, _rx) = studio();
        s.open_project(&path).unwrap();
        // Another program writes, and the watcher has not caught up yet.
        let opts = EditOptions {
            actor: edit::AGENT,
            ..Default::default()
        };
        edit::apply(&path, &[EditOp::set("/settings/bpm", json!(100))], &opts).unwrap();
        let e = s.edit(None, |_, _| {
            Ok(vec![EditOp::set("/settings/bpm", json!(90))])
        });
        // Either the watcher already reloaded (the edit then applies on the new version) or it is refused as stale.
        if let Err(e) = e {
            assert_eq!(e, edit::STALE);
        }
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_version_from_the_history_is_restored_as_a_new_edit() {
        let path = temp("studio-restore");
        let (s, _rx) = studio();
        s.open_project(&path).unwrap();
        s.edit(None, |_, _| {
            Ok(vec![EditOp::set("/settings/bpm", json!(128))])
        })
        .unwrap();
        let before = edit::history_list(&path).undo[0].file.clone();
        s.restore(&before).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"bpm\": 120"), "{text}");
        assert_eq!(edit::history_list(&path).undo.len(), 2);
        assert!(s.restore(Path::new("/etc/hostname")).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
