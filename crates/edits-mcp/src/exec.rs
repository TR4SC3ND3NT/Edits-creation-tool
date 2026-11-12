//! Finding and running the `edits` and `edits-analyze` executables.
//!
//! Arguments are passed as an argument vector, never through a shell; every
//! run has a time limit and a cap on how much output is kept.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

/// One of the two executables the server drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Edits,
    Analyze,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Edits => "edits",
            Tool::Analyze => "edits-analyze",
        }
    }
    pub fn env(self) -> &'static str {
        match self {
            Tool::Edits => "EDITS_BIN",
            Tool::Analyze => "EDITS_ANALYZE_BIN",
        }
    }
    fn package(self) -> &'static str {
        match self {
            Tool::Edits => "edits-cli",
            Tool::Analyze => "edits-analyze",
        }
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Find an executable: the explicit path (configuration or environment), then
/// next to the running executable, then `PATH`. Looked up on every call, so a
/// binary built after the server started is found.
pub fn locate(tool: Tool, explicit: Option<&Path>) -> Result<PathBuf, String> {
    let file = format!("{}{}", tool.name(), std::env::consts::EXE_SUFFIX);
    let from_env = std::env::var_os(tool.env()).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(path) = explicit.map(Path::to_path_buf).or(from_env) {
        return if is_executable(&path) {
            Ok(path)
        } else {
            Err(format!("{} is set to {}, which is not an executable file. Point it at the `{}` binary (cargo build --release -p {}), or unset it.", tool.env(), path.display(), tool.name(), tool.package()))
        };
    }
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(&file)));
    if let Some(path) = beside.filter(|p| is_executable(p)) {
        return Ok(path);
    }
    if let Some(found) = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join(&file)).find(|p| is_executable(p))) {
        return Ok(found);
    }
    Err(format!(
        "the `{}` executable was not found next to the server or on PATH. Build it with `cargo build --release -p {}`, or set {} to its path.",
        tool.name(),
        tool.package(),
        tool.env()
    ))
}

#[derive(Debug)]
pub struct Output {
    /// Exit code; `None` when the process was killed by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    /// What the tool said went wrong: its `error:` lines when it printed any, else the end of stderr.
    pub fn error_text(&self) -> String {
        let text = self.stderr.trim();
        let from = text.find("error:").unwrap_or(0);
        let tail = tail_chars(&text[from..], 3000);
        if tail.is_empty() {
            match self.code {
                Some(c) => format!("the command failed with exit code {c} and printed nothing"),
                None => "the command was killed by a signal".into(),
            }
        } else {
            tail
        }
    }
}

/// The last `max` characters of a text, on a character boundary.
pub fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let skipped: String = text.chars().skip(count - max).collect();
    format!("…{skipped}")
}

/// Read a stream, keeping at most `cap` bytes. For data on stdout
/// (`keep_head`) reading stops as soon as the cap is passed, and the caller
/// stops the process; for progress and errors on stderr the last `cap` bytes
/// are kept and reading goes on to the end, so the child never blocks on a
/// full pipe.
async fn drain<R: AsyncRead + Unpin>(mut reader: R, cap: usize, keep_head: bool) -> (Vec<u8>, bool) {
    let mut kept: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                kept.extend_from_slice(&buf[..n]);
                if keep_head && kept.len() > cap {
                    kept.truncate(cap);
                    return (kept, true);
                }
                if !keep_head && kept.len() > 2 * cap {
                    let cut = kept.len() - cap;
                    kept.drain(..cut);
                }
            }
        }
    }
    let over = !keep_head && kept.len() > cap;
    if over {
        let cut = kept.len() - cap;
        kept.drain(..cut);
    }
    (kept, over)
}

/// Run an executable to completion.
///
/// - `timeout`: the process is killed when it runs longer, and the error says so.
/// - `stdout_cap`: more output than this is an error (the caller asked for
///   something that cannot be returned); stderr keeps only its last 64 KiB.
pub async fn run(bin: &Path, args: &[OsString], cwd: &Path, timeout: Duration, stdout_cap: usize) -> Result<Output, String> {
    let mut command = tokio::process::Command::new(bin);
    command.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = command.spawn().map_err(|e| format!("cannot start {}: {e}", bin.display()))?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let name = bin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let work = async {
        let mut out_fut = std::pin::pin!(drain(stdout, stdout_cap, true));
        let mut err_fut = std::pin::pin!(drain(stderr, 64 * 1024, false));
        let (mut out, mut err) = (None, None);
        while out.is_none() || err.is_none() {
            tokio::select! {
                r = &mut out_fut, if out.is_none() => {
                    if r.1 {
                        // Too much output: stop the process instead of reading on.
                        let _ = child.start_kill();
                    }
                    out = Some(r);
                }
                r = &mut err_fut, if err.is_none() => err = Some(r),
            }
        }
        let status = child.wait().await;
        (out.expect("loop ended"), err.expect("loop ended"), status)
    };
    let ((out, out_truncated), (err, _), status) = match tokio::time::timeout(timeout, work).await {
        Ok(done) => done,
        Err(_) => {
            // Returning drops the child handle; `kill_on_drop` ends the process.
            return Err(format!(
                "`{name}` did not finish within {} s and was stopped. Ask for less (a lower scale, fewer frames, a shorter range, samples 1), or use the background tools (render_video, reference_analyze) for long work.",
                timeout.as_secs()
            ));
        }
    };
    let status = status.map_err(|e| format!("waiting for `{name}` failed: {e}"))?;
    if out_truncated {
        return Err(format!("`{name}` printed more than {} MB; ask for a smaller range or a section", stdout_cap / (1024 * 1024)));
    }
    Ok(Output { code: status.code(), stdout: String::from_utf8_lossy(&out).into_owned(), stderr: String::from_utf8_lossy(&err).into_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_must_be_an_executable() {
        let e = locate(Tool::Edits, Some(Path::new("/nonexistent/edits"))).unwrap_err();
        assert!(e.contains("EDITS_BIN") && e.contains("not an executable"), "{e}");
        let me = std::env::current_exe().unwrap();
        assert_eq!(locate(Tool::Analyze, Some(&me)).unwrap(), me);
    }

    #[test]
    fn tails_are_cut_on_character_boundaries() {
        assert_eq!(tail_chars("héllo", 10), "héllo");
        assert_eq!(tail_chars("héllo wörld", 5), "…wörld");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_without_a_shell_and_stops_at_the_time_limit() {
        let cwd = std::env::temp_dir();
        // The argument is passed as one word: no shell expands or splits it.
        let out = run(Path::new("/bin/echo"), &["$HOME; echo two".into()], &cwd, Duration::from_secs(10), 1 << 20).await.unwrap();
        assert!(out.ok());
        assert_eq!(out.stdout.trim(), "$HOME; echo two");
        let e = run(Path::new("/bin/sleep"), &["30".into()], &cwd, Duration::from_millis(200), 1 << 20).await.unwrap_err();
        assert!(e.contains("did not finish"), "{e}");
        // `cat /dev/zero` never ends: the output cap stops it long before the time limit.
        let started = std::time::Instant::now();
        let e = run(Path::new("/bin/cat"), &["/dev/zero".into()], &cwd, Duration::from_secs(30), 1 << 20).await.unwrap_err();
        assert!(e.contains("printed more than"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
