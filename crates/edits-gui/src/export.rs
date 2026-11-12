//! The Export button: `edits render <project> -o <file>` as a child process.
//!
//! The window does not encode video itself; it runs the same command a
//! person or an agent would, and follows its progress lines.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

/// `frame 8/24 (33%)  182 ms/frame  3 s left` → `(8, 24)`.
pub fn parse_progress(line: &str) -> Option<(u64, u64)> {
    let rest = line.trim_start().strip_prefix("frame ")?;
    let (done, rest) = rest.split_once('/')?;
    let total: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    Some((done.trim().parse().ok()?, total.parse().ok()?))
}

/// Where the `edits` command is: `EDITS_BIN`, next to this executable, or on `PATH`.
pub fn find_edits(exe: Option<&Path>, env_bin: Option<&str>, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let name = if cfg!(windows) { "edits.exe" } else { "edits" };
    if let Some(b) = env_bin.filter(|b| !b.is_empty()) {
        return Some(PathBuf::from(b));
    }
    if let Some(sibling) = exe.and_then(Path::parent).map(|d| d.join(name)) {
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    path_var.and_then(|p| std::env::split_paths(p).map(|d| d.join(name)).find(|c| c.is_file()))
}

pub fn locate_edits() -> Option<PathBuf> {
    find_edits(std::env::current_exe().ok().as_deref(), std::env::var("EDITS_BIN").ok().as_deref(), std::env::var_os("PATH").as_deref())
}

/// The file the export dialog proposes: `<project name>.mp4` next to the project.
pub fn default_output(project: &Path) -> PathBuf {
    let stem = project.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "edit".into());
    // `project.json` is the usual name; the folder says more.
    let name = match (stem.as_str(), project.parent().and_then(Path::file_name)) {
        ("project", Some(dir)) => dir.to_string_lossy().into_owned(),
        _ => stem,
    };
    crate::reload::base_dir(project).join(format!("{name}.mp4"))
}

/// A running or finished export.
pub struct Export {
    child: Child,
    lines: Receiver<String>,
    pub output: PathBuf,
    started: Instant,
    /// Frames done and frames in all, from the last progress line.
    pub progress: Option<(u64, u64)>,
    last_error: Option<String>,
    /// `Some` once the command has ended: how it went.
    pub finished: Option<Result<f64, String>>,
}

impl Export {
    /// Start `edits render`. `wake` is called whenever there is news.
    pub fn start(edits: &Path, project: &Path, output: &Path, wake: impl Fn() + Send + 'static) -> Result<Self, String> {
        let mut child = Command::new(edits)
            .arg("render")
            .arg(project)
            .arg("-o")
            .arg(output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run {}: {e}", edits.display()))?;
        let (tx, rx) = mpsc::channel();
        if let Some(stderr) = child.stderr.take() {
            let _ = std::thread::Builder::new().name("edits-export".into()).spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                    wake();
                }
                wake();
            });
        }
        Ok(Self { child, lines: rx, output: output.to_path_buf(), started: Instant::now(), progress: None, last_error: None, finished: None })
    }

    fn absorb(&mut self, line: &str) {
        if let Some(p) = parse_progress(line) {
            self.progress = Some(p);
        } else if let Some(e) = line.trim().strip_prefix("error:") {
            self.last_error = Some(e.trim().to_string());
        }
    }

    /// Read what the command has printed and see whether it has ended.
    pub fn poll(&mut self) {
        while let Ok(line) = self.lines.try_recv() {
            self.absorb(&line);
        }
        if self.finished.is_some() {
            return;
        }
        let status = match self.child.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return,
            Err(e) => {
                self.finished = Some(Err(format!("lost track of the export: {e}")));
                return;
            }
        };
        // The last lines may still be on their way through the pipe.
        let deadline = Instant::now() + std::time::Duration::from_millis(300);
        while let Ok(line) = self.lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            self.absorb(&line);
        }
        self.finished = Some(if status.success() {
            Ok(self.started.elapsed().as_secs_f64())
        } else {
            Err(self.last_error.clone().unwrap_or_else(|| format!("edits render ended with {status}")))
        });
    }

    pub fn running(&self) -> bool {
        self.finished.is_none()
    }

    pub fn cancel(&mut self) {
        if self.running() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.finished = Some(Err("cancelled".into()));
        }
    }

    /// One line for the status bar.
    pub fn status(&self) -> String {
        let name = self.output.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match (&self.finished, self.progress) {
            (Some(Ok(seconds)), _) => format!("exported {} in {seconds:.1} s", self.output.display()),
            (Some(Err(e)), _) => format!("export failed: {e}"),
            (None, Some((done, total))) => format!("exporting {name}: frame {done}/{total} ({:.0}%)", done as f64 * 100.0 / total.max(1) as f64),
            (None, None) => format!("exporting {name}: starting…"),
        }
    }
}

impl Drop for Export {
    fn drop(&mut self) {
        // Closing the window ends an export in progress rather than leaving it orphaned.
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_progress_lines_of_edits_render() {
        assert_eq!(parse_progress("frame 8/24 (33%)  182 ms/frame  3 s left"), Some((8, 24)));
        assert_eq!(parse_progress("frame 24/24 (100%)  161 ms/frame  0 s left"), Some((24, 24)));
        assert_eq!(parse_progress("  frame 1/195 (1%)"), Some((1, 195)));
        assert_eq!(parse_progress("error: no H.264 encoder"), None);
        assert_eq!(parse_progress("frame x/24"), None);
        assert_eq!(parse_progress("frames 3/4"), None);
        assert_eq!(parse_progress(""), None);
    }

    #[test]
    fn proposes_an_output_next_to_the_project() {
        assert_eq!(default_output(Path::new("/work/demo/project.json")), PathBuf::from("/work/demo/demo.mp4"));
        assert_eq!(default_output(Path::new("/work/demo/cube-edit.json")), PathBuf::from("/work/demo/cube-edit.mp4"));
        assert_eq!(default_output(Path::new("edit.json")), PathBuf::from("./edit.mp4"));
    }

    #[test]
    fn finds_the_edits_command() {
        let dir = std::env::temp_dir().join(format!("edits-gui-test-bin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        let name = if cfg!(windows) { "edits.exe" } else { "edits" };
        let path_var = std::env::join_paths([dir.join("a"), dir.join("b")]).unwrap();
        let exe = dir.join("a").join("edits-gui");

        assert_eq!(find_edits(Some(&exe), None, Some(&path_var)), None);
        std::fs::write(dir.join("b").join(name), b"").unwrap();
        assert_eq!(find_edits(Some(&exe), None, Some(&path_var)), Some(dir.join("b").join(name)), "found on PATH");
        std::fs::write(dir.join("a").join(name), b"").unwrap();
        assert_eq!(find_edits(Some(&exe), None, Some(&path_var)), Some(dir.join("a").join(name)), "the one next to the executable wins");
        assert_eq!(find_edits(Some(&exe), Some("/opt/edits"), Some(&path_var)), Some(PathBuf::from("/opt/edits")), "EDITS_BIN wins over both");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn follows_a_child_process_to_its_end() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("edits-gui-test-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            let p = dir.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        let wait = |e: &mut Export| {
            for _ in 0..200 {
                e.poll();
                if !e.running() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        };

        let good = script("good", r#"echo "frame 3/10 (30%)  5 ms/frame  0 s left" >&2; echo "frame 10/10 (100%)  5 ms/frame  0 s left" >&2; echo "$@" > "$4.args""#);
        let out = dir.join("out.mp4");
        let mut e = Export::start(&good, Path::new("p.json"), &out, || {}).unwrap();
        wait(&mut e);
        assert!(matches!(e.finished, Some(Ok(_))), "{:?}", e.finished);
        assert_eq!(e.progress, Some((10, 10)));
        assert!(e.status().starts_with("exported "));
        let args = std::fs::read_to_string(dir.join("out.mp4.args")).unwrap();
        assert_eq!(args.trim(), format!("render p.json -o {}", out.display()));

        let bad = script("bad", r#"echo "error: no H.264 encoder in this ffmpeg" >&2; exit 1"#);
        let mut e = Export::start(&bad, Path::new("p.json"), &out, || {}).unwrap();
        wait(&mut e);
        assert_eq!(e.finished, Some(Err("no H.264 encoder in this ffmpeg".to_string())));
        assert_eq!(e.status(), "export failed: no H.264 encoder in this ffmpeg");

        let slow = script("slow", "exec sleep 30");
        let mut e = Export::start(&slow, Path::new("p.json"), &out, || {}).unwrap();
        e.poll();
        assert!(e.running());
        e.cancel();
        assert_eq!(e.finished, Some(Err("cancelled".to_string())));

        assert!(Export::start(&dir.join("missing"), Path::new("p.json"), &out, || {}).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
