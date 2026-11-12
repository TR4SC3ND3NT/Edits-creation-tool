//! Background jobs for work that outlives one tool call (a render, a
//! reference analysis).
//!
//! The protocol is stateless, so a job is a folder, not an object in memory:
//!
//! ```text
//! <root>/.edits/jobs/<id>/
//!     job.json      what was started: kind, command, pid, the caller's notes
//!     stdout.txt    the command's stdout (its JSON summary)
//!     log.txt       the command's stderr (progress lines, errors)
//!     exit.json     written when the command ends: its exit code
//!     cancelled     present when job_cancel stopped it
//! ```
//!
//! Any later call, even from a restarted server, reads the state from there.
//! The command runs in its own process group, detached from the server, so
//! it keeps running if the client disconnects and can be stopped as a whole
//! (the renderer and its ffmpeg child together).

use crate::edit::{state_dir, utc_stamp, write_atomic};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Finished jobs kept on disk before the oldest are deleted.
const KEEP: usize = 40;

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Running,
    /// The command ended. `None` when the exit code was not recorded (the
    /// server that started it was gone): the caller judges by the output.
    Exited(Option<i32>),
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    /// The last progress line as the command printed it.
    pub line: String,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub id: String,
    pub kind: String,
    pub state: State,
    pub elapsed_seconds: f64,
    pub progress: Option<Progress>,
    /// What the caller stored when it started the job (paths to report back).
    pub meta: Value,
    pub stdout: String,
    pub log_tail: String,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn jobs_dir(root: &Path) -> PathBuf {
    root.join(".edits").join("jobs")
}

/// Job ids are folder names: refuse anything that could be a path.
fn check_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!("\"{id}\" is not a job id (ids look like render-20261112-145012-3fa9; call job_status without a job to list them)"))
    }
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // Signal 0 checks for existence without sending anything.
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    // Without a cheap liveness test, a job with no exit record counts as running.
    true
}

#[cfg(unix)]
fn stop(pid: u32, force: bool) {
    // The job leads its own process group: a negative pid signals the whole group.
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    unsafe {
        libc::kill(-(pid as libc::pid_t), signal);
    }
}

#[cfg(not(unix))]
fn stop(pid: u32, _force: bool) {
    let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn tail(path: &Path, max_bytes: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len > max_bytes {
        let _ = file.seek(SeekFrom::Start(len - max_bytes));
    }
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The newest `frame N/M` line in a log (the renderer prints one a second).
fn progress(log: &str) -> Option<Progress> {
    log.lines().rev().find_map(|line| {
        let rest = line.trim().strip_prefix("frame ")?;
        let (counts, _) = rest.split_once(' ').unwrap_or((rest, ""));
        let (done, total) = counts.split_once('/')?;
        Some(Progress { done: done.parse().ok()?, total: total.parse().ok()?, line: line.trim().to_string() })
    })
}

/// Start `bin args` as a detached job. Returns the job id at once.
pub fn start(root: &Path, kind: &str, bin: &Path, args: &[OsString], meta: Value) -> Result<String, String> {
    let base = state_dir(root, "jobs").map_err(|e| format!("cannot create the jobs folder under {}: {e}", root.display()))?;
    prune(&base);
    let stamp = utc_stamp(SystemTime::now());
    // 20261112T145012.123Z -> 20261112-145012; the suffix keeps two jobs of one second apart.
    let salt = (now_ms() as u32 ^ std::process::id().rotate_left(16)) & 0xffff;
    let mut id = format!("{kind}-{}-{}-{salt:04x}", &stamp[..8], &stamp[9..15]);
    let mut n = 0;
    while base.join(&id).exists() {
        n += 1;
        id = format!("{kind}-{}-{}-{:04x}", &stamp[..8], &stamp[9..15], (salt + n) & 0xffff);
    }
    let dir = base.join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let stdout = std::fs::File::create(dir.join("stdout.txt")).map_err(|e| e.to_string())?;
    let log = std::fs::File::create(dir.join("log.txt")).map_err(|e| e.to_string())?;

    let mut command = Command::new(bin);
    command.args(args).current_dir(root).stdin(Stdio::null()).stdout(stdout).stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| {
        let _ = std::fs::remove_dir_all(&dir);
        format!("cannot start {}: {e}", bin.display())
    })?;
    let record = json!({
        "id": id,
        "kind": kind,
        "pid": child.id(),
        "created_ms": now_ms(),
        "command": std::iter::once(bin.as_os_str().to_string_lossy().into_owned()).chain(args.iter().map(|a| a.to_string_lossy().into_owned())).collect::<Vec<_>>(),
        "meta": meta,
    });
    if let Err(e) = write_atomic(&dir.join("job.json"), &serde_json::to_string_pretty(&record).expect("JSON serialises")) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("cannot record the job in {}: {e}", dir.display()));
    }
    // Reap the child and record how it ended. If the server exits first the
    // job keeps running and `status` falls back to "is the pid alive".
    let exit_file = dir.join("exit.json");
    let _ = std::thread::Builder::new().name("edits-job".into()).spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        let _ = write_atomic(&exit_file, &json!({ "code": code, "finished_ms": now_ms() }).to_string());
    });
    Ok(id)
}

/// The state of a job, read from its folder.
pub fn status(root: &Path, id: &str) -> Result<Status, String> {
    check_id(id)?;
    let dir = jobs_dir(root).join(id);
    let Some(record) = read_json(&dir.join("job.json")) else {
        let known: Vec<String> = list(root).into_iter().take(8).map(|s| s.id).collect();
        let known = if known.is_empty() { "there are no jobs in this workspace".to_string() } else { format!("recent jobs: {}", known.join(", ")) };
        return Err(format!("no job \"{id}\" ({known})"));
    };
    let created = record["created_ms"].as_u64().unwrap_or(0);
    let pid = record["pid"].as_u64().unwrap_or(0) as u32;
    let exit = read_json(&dir.join("exit.json"));
    let cancelled = dir.join("cancelled").exists();
    let running = exit.is_none() && pid != 0 && alive(pid);
    let state = if running {
        State::Running
    } else if cancelled {
        State::Cancelled
    } else {
        State::Exited(exit.as_ref().and_then(|e| e["code"].as_i64()).map(|c| c as i32))
    };
    let finished = match (&state, &exit) {
        (State::Running, _) => now_ms(),
        (_, Some(e)) => e["finished_ms"].as_u64().unwrap_or(created),
        // No exit record: the last write to its output is the best evidence of when it ended.
        _ => ["stdout.txt", "log.txt"]
            .iter()
            .filter_map(|name| std::fs::metadata(dir.join(name)).and_then(|m| m.modified()).ok())
            .filter_map(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .max()
            .unwrap_or(created),
    };
    let log_tail = tail(&dir.join("log.txt"), 16 * 1024);
    Ok(Status {
        id: id.to_string(),
        kind: record["kind"].as_str().unwrap_or("job").to_string(),
        elapsed_seconds: (finished.saturating_sub(created)) as f64 / 1000.0,
        progress: progress(&log_tail),
        meta: record["meta"].clone(),
        stdout: if state == State::Running { String::new() } else { tail(&dir.join("stdout.txt"), 8 * 1024 * 1024) },
        log_tail,
        state,
    })
}

/// Stop a running job (the whole process group). A job that already ended is returned as it is.
pub fn cancel(root: &Path, id: &str) -> Result<Status, String> {
    let before = status(root, id)?;
    if before.state != State::Running {
        return Ok(before);
    }
    let dir = jobs_dir(root).join(id);
    let pid = read_json(&dir.join("job.json")).and_then(|r| r["pid"].as_u64()).unwrap_or(0) as u32;
    std::fs::write(dir.join("cancelled"), utc_stamp(SystemTime::now())).map_err(|e| format!("cannot mark the job as cancelled: {e}"))?;
    stop(pid, false);
    let asked = std::time::Instant::now();
    let mut forced = false;
    loop {
        let now = status(root, id)?;
        if now.state != State::Running || asked.elapsed() > Duration::from_secs(8) {
            return Ok(now);
        }
        if !forced && asked.elapsed() > Duration::from_secs(3) {
            stop(pid, true);
            forced = true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Every job in the workspace, newest first.
pub fn list(root: &Path) -> Vec<Status> {
    let mut all: Vec<(u64, Status)> = std::fs::read_dir(jobs_dir(root))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            let created = read_json(&e.path().join("job.json"))?["created_ms"].as_u64()?;
            Some((created, status(root, &id).ok()?))
        })
        .collect();
    all.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.id.cmp(&a.1.id)));
    all.into_iter().map(|(_, s)| s).collect()
}

/// Delete the oldest finished jobs so the folder does not grow without bound.
fn prune(base: &Path) {
    let Some(root) = base.parent().and_then(Path::parent) else { return };
    let finished: Vec<Status> = list(root).into_iter().filter(|s| s.state != State::Running).collect();
    for old in finished.iter().skip(KEEP) {
        let _ = std::fs::remove_dir_all(base.join(&old.id));
    }
}

/// Poll a job until it ends or `limit` passes; returns its state at that moment.
pub async fn wait(root: &Path, id: &str, limit: Duration) -> Result<Status, String> {
    let started = std::time::Instant::now();
    let mut pause = Duration::from_millis(40);
    loop {
        let now = status(root, id)?;
        if now.state != State::Running || started.elapsed() >= limit {
            return Ok(now);
        }
        tokio::time::sleep(pause).await;
        pause = (pause * 2).min(Duration::from_millis(400));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edits-mcp-jobs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn a_job_runs_to_the_end_and_its_state_is_on_disk() {
        let root = temp("done");
        let id = start(&root, "render", Path::new("/bin/echo"), &["{\"frames\": 3}".into()], json!({ "output": "x.mp4" })).unwrap();
        assert!(id.starts_with("render-"), "{id}");
        let s = wait(&root, &id, Duration::from_secs(10)).await.unwrap();
        // The exit record is written by a thread a moment after the process ends.
        let s = if s.state == State::Exited(None) { wait(&root, &id, Duration::from_millis(300)).await.unwrap() } else { s };
        assert!(matches!(s.state, State::Exited(Some(0)) | State::Exited(None)), "{:?}", s.state);
        assert_eq!(s.stdout.trim(), "{\"frames\": 3}");
        assert_eq!(s.meta["output"], json!("x.mp4"));
        assert_eq!(s.kind, "render");
        // Nothing but the folder is needed to answer: forget the exit record, as if
        // the server that started the job had been killed before the job ended.
        std::thread::sleep(Duration::from_millis(100));
        let _ = std::fs::remove_file(root.join(".edits/jobs").join(&id).join("exit.json"));
        let again = status(&root, &id).unwrap();
        assert_eq!(again.state, State::Exited(None));
        assert_eq!(again.stdout.trim(), "{\"frames\": 3}");
        assert_eq!(list(&root).len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_running_job_can_be_cancelled() {
        let root = temp("cancel");
        let id = start(&root, "analyze", Path::new("/bin/sleep"), &["60".into()], json!({})).unwrap();
        assert_eq!(status(&root, &id).unwrap().state, State::Running);
        let s = cancel(&root, &id).unwrap();
        assert_eq!(s.state, State::Cancelled);
        assert!(s.elapsed_seconds < 10.0);
        // cancelling again changes nothing
        assert_eq!(cancel(&root, &id).unwrap().state, State::Cancelled);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ids_cannot_be_paths_and_unknown_jobs_are_explained() {
        let root = temp("ids");
        assert!(status(&root, "../../etc").unwrap_err().contains("is not a job id"));
        assert!(status(&root, "render-1").unwrap_err().contains("no job"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn progress_is_read_from_the_last_frame_line() {
        let p = progress("frame 8/24 (33%)  182 ms/frame  3 s left\nframe 16/24 (67%)  170 ms/frame  1 s left\n").unwrap();
        assert_eq!((p.done, p.total), (16, 24));
        assert!(progress("warning: something\n").is_none());
    }
}
