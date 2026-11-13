//! `project_edit` and `project_undo`: the server's side of
//! [`edits_core::edit`], which owns the operations, validation, atomic
//! writes and the history shared with the window (`edits-gui`).
//!
//! What this module adds is the workspace: a project may not refer to files
//! outside the server's root, so those references are errors here. Edits
//! made through the server are recorded in the history as the `agent`'s.

use crate::paths::Workspace;
use edits_core::check::{Finding, Severity};
use edits_core::edit::{self as core, EditOptions};
use edits_core::Project;
use serde_json::Value;
use std::path::Path;

pub use edits_core::edit::{apply_op, apply_ops, change_line, diff, finding_line, history, state_dir, utc_stamp, write_atomic, Change, EditOp, EditOutcome, OpKind, SlotFit, UndoOutcome, AGENT};

/// Errors for every file reference that leaves the workspace root.
pub fn outside_refs(doc: &Value, project_dir: &Path, ws: &Workspace) -> Vec<Finding> {
    core::file_refs(doc)
        .into_iter()
        .filter_map(|(ptr, path)| {
            ws.resolve_from(project_dir, &path, "file").err().map(|_| Finding {
                severity: Severity::Error,
                path: ptr,
                message: format!("\"{path}\" is outside the workspace root {}", ws.root().display()),
                hint: Some("copy the file into the workspace and use a path relative to the project file, or restart the server with EDITS_ROOT set to a folder that contains it".into()),
            })
        })
        .collect()
}

/// Validate a document the way the renderer will see it (see
/// [`edits_core::edit::validate`]), plus files outside the workspace. Errors first.
pub fn validate(doc: &Value, project_dir: &Path, ws: Option<&Workspace>) -> Vec<Finding> {
    match ws {
        Some(ws) => {
            let confine = |doc: &Value, dir: &Path| outside_refs(doc, dir, ws);
            core::validate(doc, project_dir, Some(&confine))
        }
        None => core::validate(doc, project_dir, None),
    }
}

/// What only the raw JSON can show, beyond `edits_core::check`: files outside
/// the workspace (errors) and keys the renderer ignores (warnings).
pub fn raw_findings(doc: &Value, project: &Project, project_dir: &Path, ws: Option<&Workspace>) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Some(ws) = ws {
        findings.extend(outside_refs(doc, project_dir, ws));
    }
    findings.extend(core::ignored_key_findings(doc, project));
    findings
}

/// Apply `ops` to the project file as the agent. See [`edits_core::edit::apply`].
pub fn edit_file(project: &Path, ops: &[EditOp], dry_run: bool, ws: Option<&Workspace>) -> Result<EditOutcome, String> {
    match ws {
        Some(ws) => {
            let confine = |doc: &Value, dir: &Path| outside_refs(doc, dir, ws);
            core::apply(project, ops, &EditOptions { dry_run, actor: AGENT, extra: Some(&confine), ..Default::default() })
        }
        None => core::apply(project, ops, &EditOptions { dry_run, actor: AGENT, ..Default::default() }),
    }
}

/// Change the project file with a function of its current document, as the
/// agent: presets and beat operations. See [`edits_core::edit::apply_with`].
pub fn edit_with(project: &Path, label: &str, dry_run: bool, ws: &Workspace, change: &dyn Fn(&Value) -> Result<Value, String>) -> Result<EditOutcome, String> {
    let confine = |doc: &Value, dir: &Path| outside_refs(doc, dir, ws);
    core::apply_with(project, &EditOptions { dry_run, actor: AGENT, label: Some(label), extra: Some(&confine), ..Default::default() }, change)
}

/// Restore the newest version in the shared history, whoever made the edit
/// after it. Repeated calls walk further back.
pub fn undo_file(project: &Path) -> Result<UndoOutcome, String> {
    core::undo(project)
}

/// Save the current file in the history before something else replaces it
/// (`reference_draft`), as the agent's step.
pub fn backup(project: &Path, summary: &str) -> std::io::Result<std::path::PathBuf> {
    core::backup(project, AGENT, summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    const PROJECT: &str = r#"{
      "format": "edits.project/1",
      "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 123 },
      "slots": { "a": {}, "b": {} },
      "layers": [
        { "id": "a", "kind": "image", "slot": "a", "in": 0, "out": 4 },
        { "id": "b", "kind": "image", "slot": "b", "in": 4 }
      ]
    }"#;

    fn op(v: Value) -> EditOp {
        serde_json::from_value(v).expect("op parses")
    }

    struct Temp(PathBuf);
    impl Temp {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("edits-mcp-edit-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("project.json"), PROJECT).unwrap();
            Temp(dir)
        }
        fn project(&self) -> PathBuf {
            self.0.join("project.json")
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn typos_and_outside_files_are_found() {
        let t = Temp::new("lint");
        let ws = Workspace::new(&t.0).unwrap();
        let out = edit_file(&t.project(), &[op(json!({ "op": "set", "pointer": "/layers/@a/transfrom", "value": { "scale": 2 } }))], true, Some(&ws)).unwrap();
        let w = out.findings.iter().find(|f| f.path == "/layers/@a/transfrom").expect("the typo is reported");
        assert_eq!(w.severity, Severity::Warning);
        assert!(w.hint.as_deref().unwrap().contains("transform"));

        let out = edit_file(&t.project(), &[op(json!({ "op": "set_slot", "name": "a", "path": "../../../etc/hostname" }))], false, Some(&ws)).unwrap();
        assert!(!out.ok);
        assert!(out.findings.iter().any(|f| f.path == "/slots/a/path" && f.message.contains("outside the workspace root")));
    }

    #[test]
    fn the_agent_is_recorded_as_the_actor() {
        let t = Temp::new("actor");
        let out = edit_file(&t.project(), &[op(json!({ "op": "set", "pointer": "/settings/bpm", "value": 130 }))], false, None).unwrap();
        assert!(out.written);
        let h = edits_core::edit::history_list(&t.project());
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.undo[0].actor, "agent");
        assert_eq!(h.undo[0].summary, "~ /settings/bpm: 123 → 130");
        let u = undo_file(&t.project()).unwrap();
        assert_eq!(u.entry.actor, "agent");
    }
}
