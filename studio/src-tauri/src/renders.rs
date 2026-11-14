//! The Renders page: `edits render <project> -o <file>` as a child process,
//! its progress lines turned into events, and the list of finished outputs.
//! The studio does not encode video itself; it runs the command a person or
//! an agent would.

use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Instant, UNIX_EPOCH};

/// `frame 8/24 (33%)  182 ms/frame  3 s left` → `(8, 24)`.
pub fn parse_progress(line: &str) -> Option<(u64, u64)> {
    let rest = line.trim_start().strip_prefix("frame ")?;
    let (done, rest) = rest.split_once('/')?;
    let total: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    Some((done.trim().parse().ok()?, total.parse().ok()?))
}

/// Where the `edits` command is: `EDITS_BIN`, next to this executable, the
/// workspace's other build folder, or `PATH`.
pub fn locate(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let env = if name == "edits" {
        "EDITS_BIN"
    } else {
        "EDITS_ANALYZE_BIN"
    };
    if let Some(b) = std::env::var_os(env).filter(|b| !b.is_empty()) {
        return Some(PathBuf::from(b));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        for candidate in [
            dir.join(&file),
            dir.join("../release").join(&file),
            dir.join("../debug").join(&file),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(&file))
            .find(|c| c.is_file())
    })
}

/// Where a new render goes: `<project dir>/renders/<name>.mp4`, numbered when taken.
pub fn default_output(project: &Path) -> PathBuf {
    let stem = project
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "edit".into());
    let name = match (stem.as_str(), project.parent().and_then(Path::file_name)) {
        ("project", Some(dir)) => dir.to_string_lossy().into_owned(),
        _ => stem,
    };
    let dir = edits_core::edit::project_dir(project).join("renders");
    let mut out = dir.join(format!("{name}.mp4"));
    let mut n = 1;
    while out.exists() {
        n += 1;
        out = dir.join(format!("{name}-{n}.mp4"));
    }
    out
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct JobStatus {
    pub output: String,
    pub running: bool,
    pub done: u64,
    pub total: u64,
    /// The last progress line (`frame 8/24 (33%)  182 ms/frame  3 s left`).
    pub line: String,
    pub elapsed: f64,
    /// Set once it has ended: `ok`, `failed` or `cancelled`.
    pub outcome: Option<String>,
    pub error: Option<String>,
}

/// A running or finished render.
pub struct RenderJob {
    child: Arc<Mutex<Child>>,
    pub status: Arc<Mutex<JobStatus>>,
}

impl RenderJob {
    /// Start `edits render`. `on_change` is called with the status after every line and at the end.
    pub fn start(
        edits: &Path,
        project: &Path,
        output: &Path,
        scale: Option<f64>,
        on_change: impl Fn(JobStatus) + Send + 'static,
    ) -> Result<Self, String> {
        if let Some(dir) = output.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let mut cmd = Command::new(edits);
        cmd.arg("render").arg(project).arg("-o").arg(output);
        if let Some(s) = scale.filter(|s| (*s - 1.0).abs() > 1e-9) {
            cmd.arg("--scale").arg(format!("{s}"));
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run {}: {e}", edits.display()))?;
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        let status = Arc::new(Mutex::new(JobStatus {
            output: output.display().to_string(),
            running: true,
            ..Default::default()
        }));
        let (c, s) = (child.clone(), status.clone());
        let started = Instant::now();
        std::thread::Builder::new()
            .name("edits-render-job".into())
            .spawn(move || {
                let mut last_error = None;
                if let Some(stderr) = stderr {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        let snap = {
                            let mut st = s.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some((d, t)) = parse_progress(&line) {
                                st.done = d;
                                st.total = t;
                                st.line = line.trim().to_string();
                            } else if let Some(e) = line.trim().strip_prefix("error:") {
                                last_error = Some(e.trim().to_string());
                            }
                            st.elapsed = started.elapsed().as_secs_f64();
                            st.clone()
                        };
                        on_change(snap);
                    }
                }
                let exit = c.lock().unwrap_or_else(|e| e.into_inner()).wait();
                let snap = {
                    let mut st = s.lock().unwrap_or_else(|e| e.into_inner());
                    st.running = false;
                    st.elapsed = started.elapsed().as_secs_f64();
                    if st.outcome.is_none() {
                        match exit {
                            Ok(code) if code.success() => {
                                st.outcome = Some("ok".into());
                                st.done = st.total.max(st.done);
                            }
                            Ok(code) => {
                                st.outcome = Some("failed".into());
                                st.error =
                                    Some(last_error.unwrap_or_else(|| {
                                        format!("edits render ended with {code}")
                                    }));
                            }
                            Err(e) => {
                                st.outcome = Some("failed".into());
                                st.error = Some(format!("lost track of the render: {e}"));
                            }
                        }
                    }
                    st.clone()
                };
                on_change(snap);
            })
            .map_err(|e| format!("cannot start a thread: {e}"))?;
        Ok(RenderJob { child, status })
    }

    pub fn running(&self) -> bool {
        self.status.lock().map(|s| s.running).unwrap_or(false)
    }

    pub fn cancel(&mut self) {
        if !self.running() {
            return;
        }
        if let Ok(mut s) = self.status.lock() {
            s.outcome = Some("cancelled".into());
        }
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Output {
    pub path: String,
    pub name: String,
    /// Folder relative to the project's.
    pub folder: String,
    pub bytes: u64,
    /// Milliseconds since 1970.
    pub modified: f64,
}

/// The videos next to the project and in its `renders/` folder, newest first.
pub fn outputs(project: &Path) -> Vec<Output> {
    let dir = edits_core::edit::project_dir(project);
    let mut out = Vec::new();
    for (folder, d) in [(".", dir.clone()), ("renders", dir.join("renders"))] {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let video = p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                matches!(
                    x.to_ascii_lowercase().as_str(),
                    "mp4" | "mov" | "webm" | "mkv"
                )
            });
            let Ok(meta) = e.metadata() else { continue };
            if !video || !meta.is_file() {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            out.push(Output {
                name: e.file_name().to_string_lossy().into_owned(),
                folder: folder.into(),
                path: p.display().to_string(),
                bytes: meta.len(),
                modified,
            });
        }
    }
    out.sort_by(|a, b| b.modified.total_cmp(&a.modified));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines_are_read() {
        assert_eq!(
            parse_progress("frame 8/24 (33%)  182 ms/frame  3 s left"),
            Some((8, 24))
        );
        assert_eq!(parse_progress("  frame 100/100 (100%)"), Some((100, 100)));
        assert_eq!(parse_progress("wrote out.mp4"), None);
    }

    #[test]
    fn outputs_go_to_a_renders_folder_and_are_listed_newest_first() {
        let dir = std::env::temp_dir().join(format!("edits-studio-renders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("demo")).unwrap();
        let project = dir.join("demo/project.json");
        std::fs::write(&project, "{}").unwrap();
        let first = default_output(&project);
        assert_eq!(first, dir.join("demo/renders/demo.mp4"));
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(
            default_output(&project),
            dir.join("demo/renders/demo-2.mp4")
        );
        std::fs::write(dir.join("demo/old.mov"), b"xy").unwrap();
        std::fs::write(dir.join("demo/notes.txt"), b"no").unwrap();
        let list = outputs(&project);
        assert_eq!(list.len(), 2);
        assert!(list
            .iter()
            .any(|o| o.name == "demo.mp4" && o.folder == "renders" && o.bytes == 1));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failing_command_reports_its_error_line() {
        let dir = std::env::temp_dir().join(format!("edits-studio-job-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake-edits");
        std::fs::write(
            &script,
            "#!/bin/sh\necho 'frame 1/2 (50%)' >&2\necho 'error: no ffmpeg' >&2\nexit 3\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let tx = Mutex::new(tx);
            let _job = RenderJob::start(
                &script,
                Path::new("p.json"),
                &dir.join("out.mp4"),
                None,
                move |s| {
                    let _ = tx.lock().unwrap().send(s);
                },
            )
            .unwrap();
            let last = loop {
                let s = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
                if !s.running {
                    break s;
                }
            };
            assert_eq!(last.outcome.as_deref(), Some("failed"));
            assert_eq!(last.error.as_deref(), Some("no ffmpeg"));
            assert_eq!((last.done, last.total), (1, 2));
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
