//! Live reload: watch the project file and the files it names, and decide
//! what the window shows after each change.
//!
//! The rule is simple: the window always shows the last version of the
//! project that parsed and passed `edits_core::check`. A change that does not
//! is reported in the Problems panel and otherwise ignored, so a half-written
//! or wrong edit never blanks the preview.

use edits_core::check::{check, Finding, Severity};
use edits_core::Project;
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One line of the Problems panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub error: bool,
    /// Where in the document, e.g. `layers[3:"cube1"].effects[0]`.
    pub path: String,
    pub message: String,
    pub hint: Option<String>,
}

impl From<Finding> for Problem {
    fn from(f: Finding) -> Self {
        Problem { error: f.severity == Severity::Error, path: f.path, message: f.message, hint: f.hint }
    }
}

/// The result of reading the project file once.
#[derive(Clone, Debug)]
pub struct LoadReport {
    /// The parsed project, when the file is JSON of the right shape. It may
    /// still have errors: see [`LoadReport::valid`].
    pub project: Option<Project>,
    /// The parse error, or the findings of `check` (errors and warnings).
    pub problems: Vec<Problem>,
    /// Changes whenever the project text or one of its files changes.
    pub fingerprint: u64,
    /// The project file and every file it refers to.
    pub files: Vec<PathBuf>,
    /// The file as JSON, when it is JSON: edits copy layers from it, so keys
    /// the model does not know survive.
    pub doc: Option<Arc<serde_json::Value>>,
    /// [`edits_core::edit::text_hash`] of the file's text: an edit made in
    /// the window is refused when the file is no longer this text.
    pub text_hash: u64,
}

impl LoadReport {
    pub fn valid(&self) -> bool {
        self.project.is_some() && !self.problems.iter().any(|p| p.error)
    }
}

/// The folder relative paths in the project resolve against.
pub fn base_dir(project_path: &Path) -> PathBuf {
    match project_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn file_stamp(path: &Path) -> (u64, u128) {
    match std::fs::metadata(path) {
        Ok(m) => {
            let modified = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
            (m.len(), modified)
        }
        Err(_) => (u64::MAX, 0),
    }
}

/// Size and modification time of each file: cheap to compare, and what the
/// renderer itself uses to recognise a changed picture.
pub fn stamp(files: &[PathBuf]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in files {
        f.hash(&mut h);
        file_stamp(f).hash(&mut h);
    }
    h.finish()
}

/// The files a project refers to: slot pictures, the audio file and custom shaders.
pub fn asset_files(project: &Project, base: &Path) -> Vec<PathBuf> {
    let mut out = BTreeSet::new();
    for slot in project.slots.values() {
        if let Some(p) = &slot.path {
            out.insert(base.join(p));
        }
    }
    if let Some(a) = &project.audio {
        out.insert(base.join(&a.path));
    }
    for l in &project.layers {
        for e in &l.effects {
            if let Some(s) = &e.source {
                out.insert(base.join(s));
            }
        }
    }
    out.into_iter().collect()
}

/// Turn the text of a project file into a report. `base` is given to
/// `check` so missing pictures are found; `None` skips the file checks.
pub fn parse(text: &str, base: Option<&Path>) -> (Option<Project>, Vec<Problem>) {
    match Project::from_json(text) {
        Ok(project) => {
            let problems = check(&project, base).into_iter().map(Problem::from).collect();
            (Some(project), problems)
        }
        Err(e) => {
            let (path, message) = match e.downcast_ref::<serde_json::Error>() {
                Some(j) if j.line() > 0 => (format!("line {}, column {}", j.line(), j.column()), j.to_string()),
                _ => ("(whole file)".to_string(), format!("{e:#}")),
            };
            let problem = Problem { error: true, path, message, hint: Some("the file is not a valid project; fix the JSON (see docs/FORMAT.md)".into()) };
            (None, vec![problem])
        }
    }
}

/// Read and check the project file.
pub fn load(project_path: &Path) -> LoadReport {
    let base = base_dir(project_path);
    let mut files = vec![project_path.to_path_buf()];
    let (project, problems, text) = match std::fs::read_to_string(project_path) {
        Ok(text) => {
            let (project, problems) = parse(&text, Some(&base));
            (project, problems, text)
        }
        Err(e) => {
            let problem = Problem { error: true, path: project_path.display().to_string(), message: format!("cannot read the project file: {e}"), hint: None };
            (None, vec![problem], String::new())
        }
    };
    if let Some(p) = &project {
        files.extend(asset_files(p, &base));
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    stamp(&files[1..]).hash(&mut h);
    let doc = project.as_ref().and_then(|_| serde_json::from_str(&text).ok()).map(Arc::new);
    LoadReport { project, problems, fingerprint: h.finish(), files, doc, text_hash: edits_core::edit::text_hash(&text) }
}

// ---- what the window shows ---------------------------------------------------

/// What a report did to the window's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// A new valid version was taken: a new revision.
    Swapped,
    /// The file is broken: the last good version stays.
    Rejected,
    /// Nothing that matters changed (or a broken file went back to the
    /// version already shown).
    Unchanged,
}

/// The project the window shows, and what is wrong with the file on disk.
#[derive(Clone, Debug, Default)]
pub struct ProjectState {
    /// The last valid version, if there has been one.
    pub current: Option<Arc<Project>>,
    /// Counts accepted versions, starting at 1. Rendered frames are cached per revision.
    pub revision: u64,
    /// Errors and warnings of the file as it is on disk now.
    pub problems: Vec<Problem>,
    /// The file on disk is not what is shown, because it has errors.
    pub rejected: bool,
    /// Fingerprint of the version in `current`.
    fingerprint: Option<u64>,
    /// The JSON of the version in `current`.
    pub doc: Option<Arc<serde_json::Value>>,
    /// [`edits_core::edit::text_hash`] of the version in `current`: what the
    /// window's edits are prepared against.
    pub text_hash: Option<u64>,
}

impl ProjectState {
    pub fn apply(&mut self, report: LoadReport) -> Applied {
        let valid = report.valid();
        self.problems = report.problems;
        if !valid {
            self.rejected = true;
            return Applied::Rejected;
        }
        self.rejected = false;
        if self.current.is_some() && self.fingerprint == Some(report.fingerprint) {
            return Applied::Unchanged;
        }
        self.current = report.project.map(Arc::new);
        self.fingerprint = Some(report.fingerprint);
        self.doc = report.doc;
        self.text_hash = Some(report.text_hash);
        self.revision += 1;
        Applied::Swapped
    }

    pub fn errors(&self) -> usize {
        self.problems.iter().filter(|p| p.error).count()
    }

    pub fn warnings(&self) -> usize {
        self.problems.iter().filter(|p| !p.error).count()
    }
}

// ---- debounce ------------------------------------------------------------------

/// Collects a burst of file events into one reload: it fires once the file
/// has been quiet for `quiet`, or `max_wait` after the first event of a
/// burst that never goes quiet.
#[derive(Clone, Copy, Debug)]
pub struct Debounce {
    quiet: Duration,
    max_wait: Duration,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Debounce {
    pub fn new(quiet: Duration, max_wait: Duration) -> Self {
        Self { quiet, max_wait, first: None, last: None }
    }

    /// A file event happened at `now`.
    pub fn event(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    pub fn pending(&self) -> bool {
        self.first.is_some()
    }

    /// When the pending reload should happen.
    pub fn deadline(&self) -> Option<Instant> {
        let (first, last) = (self.first?, self.last?);
        Some((last + self.quiet).min(first + self.max_wait))
    }

    /// True once: when the deadline has passed. The burst is then over.
    pub fn fire(&mut self, now: Instant) -> bool {
        match self.deadline() {
            Some(d) if now >= d => {
                self.first = None;
                self.last = None;
                true
            }
            _ => false,
        }
    }
}

impl Default for Debounce {
    fn default() -> Self {
        Self::new(Duration::from_millis(120), Duration::from_millis(1000))
    }
}

// ---- the watcher thread ------------------------------------------------------

fn watch_target(file: &Path) -> Option<(PathBuf, PathBuf)> {
    let name = file.file_name()?;
    let dir = base_dir(file).canonicalize().ok()?;
    let full = dir.join(name);
    Some((dir, full))
}

fn is_change(kind: &EventKind) -> bool {
    // Reading a file raises access events; only changes count.
    !matches!(kind, EventKind::Access(_))
}

/// Watch `project_path` and the files it names. `on_report` is called on the
/// watcher's thread after every (debounced) change, with a fresh
/// [`LoadReport`]. The thread ends when the returned handle is dropped.
pub fn spawn_watcher(project_path: PathBuf, initial_files: Vec<PathBuf>, on_report: impl Fn(LoadReport) + Send + 'static) -> WatchHandle {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("edits-watch".into())
        .spawn(move || {
            let (fs_tx, fs_rx) = mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |res| {
                let _ = fs_tx.send(res);
            })
            .ok();
            let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
            let mut targets: BTreeSet<PathBuf> = BTreeSet::new();
            let mut files = initial_files;
            let mut retarget = |files: &[PathBuf], dirs: &mut BTreeSet<PathBuf>, targets: &mut BTreeSet<PathBuf>| {
                let pairs: Vec<(PathBuf, PathBuf)> = files.iter().filter_map(|f| watch_target(f)).collect();
                let want: BTreeSet<PathBuf> = pairs.iter().map(|(d, _)| d.clone()).collect();
                *targets = pairs.into_iter().map(|(_, f)| f).collect();
                if let Some(w) = watcher.as_mut() {
                    for gone in dirs.difference(&want) {
                        let _ = w.unwatch(gone);
                    }
                    for new in want.difference(dirs) {
                        let _ = w.watch(new, RecursiveMode::NonRecursive);
                    }
                }
                *dirs = want;
            };
            retarget(&files, &mut dirs, &mut targets);
            let mut last_stamp = stamp(&files);
            let mut debounce = Debounce::default();
            // File events can be missed (network folders, a full inotify
            // queue), so sizes and times are also compared once a second.
            let poll = Duration::from_millis(1000);
            let mut next_poll = Instant::now() + poll;
            loop {
                if !matches!(stop_rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                    break;
                }
                let now = Instant::now();
                let wake = debounce.deadline().map_or(next_poll, |d| d.min(next_poll));
                match fs_rx.recv_timeout(wake.saturating_duration_since(now).min(Duration::from_millis(250))) {
                    Ok(Ok(event)) => {
                        if is_change(&event.kind) && event.paths.iter().any(|p| targets.contains(p)) {
                            debounce.event(Instant::now());
                        }
                    }
                    Ok(Err(_)) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        // No file events on this system: fall back to polling alone.
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                let now = Instant::now();
                if now >= next_poll {
                    next_poll = now + poll;
                    if !debounce.pending() && stamp(&files) != last_stamp {
                        debounce.event(now);
                    }
                }
                if debounce.fire(now) {
                    let report = load(&project_path);
                    files = report.files.clone();
                    retarget(&files, &mut dirs, &mut targets);
                    last_stamp = stamp(&files);
                    on_report(report);
                }
            }
        })
        .ok();
    WatchHandle { stop: Some(stop_tx), thread }
}

pub struct WatchHandle {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"format":"edits.project/1",
        "settings":{"width":64,"height":64,"fps":25,"duration":4},
        "layers":[{"id":"bg","kind":"solid","color":[1,0,0,1]}]}"#;

    fn report(text: &str, fingerprint: u64) -> LoadReport {
        let (project, problems) = parse(text, None);
        let doc = serde_json::from_str(text).ok().map(Arc::new);
        LoadReport { project, problems, fingerprint, files: Vec::new(), doc, text_hash: edits_core::edit::text_hash(text) }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn debounce_waits_for_quiet() {
        let t0 = Instant::now();
        let mut d = Debounce::new(ms(100), ms(1000));
        assert!(!d.pending());
        assert!(!d.fire(t0));
        d.event(t0);
        assert!(d.pending());
        assert!(!d.fire(t0 + ms(99)));
        // Another write arrives: the wait starts again.
        d.event(t0 + ms(60));
        assert!(!d.fire(t0 + ms(140)));
        assert_eq!(d.deadline(), Some(t0 + ms(160)));
        assert!(d.fire(t0 + ms(160)));
        // It fires once per burst.
        assert!(!d.pending());
        assert!(!d.fire(t0 + ms(500)));
    }

    #[test]
    fn debounce_does_not_wait_forever() {
        let t0 = Instant::now();
        let mut d = Debounce::new(ms(100), ms(1000));
        // A file that is written every 50 ms never goes quiet.
        let mut fired_at = None;
        for i in 0..60 {
            let now = t0 + ms(i * 50);
            d.event(now);
            if d.fire(now) {
                fired_at = Some(now);
                break;
            }
        }
        assert_eq!(fired_at, Some(t0 + ms(1000)));
    }

    #[test]
    fn a_valid_project_is_swapped_in() {
        let mut s = ProjectState::default();
        assert_eq!(s.apply(report(GOOD, 1)), Applied::Swapped);
        assert_eq!(s.revision, 1);
        assert!(s.current.is_some());
        assert!(!s.rejected);
        assert_eq!(s.errors(), 0);
        // The same file again (a touch, or an editor saving without changes): nothing happens.
        assert_eq!(s.apply(report(GOOD, 1)), Applied::Unchanged);
        assert_eq!(s.revision, 1);
        // A real change: a new revision.
        assert_eq!(s.apply(report(&GOOD.replace("[1,0,0,1]", "[0,1,0,1]"), 2)), Applied::Swapped);
        assert_eq!(s.revision, 2);
    }

    #[test]
    fn a_broken_file_keeps_the_last_good_project() {
        let mut s = ProjectState::default();
        s.apply(report(GOOD, 1));
        let shown = s.current.clone().unwrap();

        // Not JSON at all (a half-written file).
        assert_eq!(s.apply(report(&GOOD[..40], 2)), Applied::Rejected);
        assert!(s.rejected);
        assert_eq!(s.revision, 1);
        assert!(Arc::ptr_eq(s.current.as_ref().unwrap(), &shown));
        assert_eq!(s.errors(), 1);
        assert!(s.problems[0].path.starts_with("line "), "{:?}", s.problems[0]);

        // Valid JSON with an error the checker finds.
        let unknown = GOOD.replace(r#""color":[1,0,0,1]"#, r#""color":[1,0,0,1],"effects":[{"kind":"sparkle"}]"#);
        assert_eq!(s.apply(report(&unknown, 3)), Applied::Rejected);
        assert_eq!(s.revision, 1);
        assert!(Arc::ptr_eq(s.current.as_ref().unwrap(), &shown));
        let p = &s.problems[0];
        assert!(p.error);
        assert_eq!(p.path, r#"layers[0:"bg"].effects[0]"#);
        assert!(p.message.contains("unknown effect \"sparkle\""));
        assert!(p.hint.as_deref().unwrap().contains("glow"));

        // The agent undoes the mistake: no new revision, the problems go away.
        assert_eq!(s.apply(report(GOOD, 1)), Applied::Unchanged);
        assert!(!s.rejected);
        assert!(s.problems.is_empty());
        assert_eq!(s.revision, 1);

        // Or fixes it differently: a new revision.
        assert_eq!(s.apply(report(&unknown.replace("sparkle", "glow"), 4)), Applied::Swapped);
        assert_eq!(s.revision, 2);
        assert!(!s.rejected);
    }

    #[test]
    fn warnings_do_not_block_a_reload() {
        let warned = GOOD.replace(r#""layers""#, r#""slots":{"unused":{}},"layers""#);
        let mut s = ProjectState::default();
        assert_eq!(s.apply(report(&warned, 1)), Applied::Swapped);
        assert_eq!((s.errors(), s.warnings()), (0, 2));
        assert!(!s.rejected);
        assert!(s.problems.iter().all(|p| !p.error));
    }

    #[test]
    fn a_first_load_that_fails_shows_nothing_but_the_problems() {
        let mut s = ProjectState::default();
        assert_eq!(s.apply(report("{", 1)), Applied::Rejected);
        assert!(s.current.is_none());
        assert_eq!(s.revision, 0);
        assert_eq!(s.apply(report(GOOD, 2)), Applied::Swapped);
        assert_eq!(s.revision, 1);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edits-gui-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_reports_missing_files_and_fingerprints_changes() {
        let dir = temp_dir("load");
        let path = dir.join("project.json");
        assert!(!load(&path).valid(), "a missing file is a problem, not a panic");

        let with_slot = GOOD.replace(r#""layers":["#, r#""slots":{"a":{"path":"a.png"}},"layers":[{"id":"pic","kind":"image","slot":"a"},"#);
        std::fs::write(&path, &with_slot).unwrap();
        let r = load(&path);
        assert!(!r.valid());
        assert!(r.problems.iter().any(|p| p.error && p.message.contains("image not found")));
        assert_eq!(r.files, vec![path.clone(), dir.join("a.png")]);

        std::fs::write(dir.join("a.png"), b"one").unwrap();
        let a = load(&path);
        assert!(a.valid(), "{:?}", a.problems);
        assert_eq!(a.fingerprint, load(&path).fingerprint, "reading twice changes nothing");
        // The picture changes but the JSON does not: still a different fingerprint.
        std::fs::write(dir.join("a.png"), b"other bytes").unwrap();
        assert_ne!(a.fingerprint, load(&path).fingerprint);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_watcher_reports_a_change_once() {
        let dir = temp_dir("watch");
        let path = dir.join("project.json");
        std::fs::write(&path, GOOD).unwrap();
        let (tx, rx) = mpsc::channel();
        let first = load(&path);
        let handle = spawn_watcher(path.clone(), first.files.clone(), move |r| {
            let _ = tx.send(r);
        });
        std::thread::sleep(ms(300));
        assert!(rx.try_recv().is_err(), "nothing changed yet");

        // Written the way editors and agents do it: a temporary file renamed over the original.
        let tmp = dir.join("project.json.tmp");
        std::fs::write(&tmp, GOOD.replace("[1,0,0,1]", "[0,0,1,1]")).unwrap();
        std::fs::rename(&tmp, &path).unwrap();
        let r = rx.recv_timeout(Duration::from_secs(5)).expect("a report after the change");
        assert!(r.valid());
        assert_ne!(r.fingerprint, first.fingerprint);
        std::thread::sleep(ms(400));
        assert!(rx.try_recv().is_err(), "one change, one report");

        std::fs::write(&path, "{ not json").unwrap();
        let r = rx.recv_timeout(Duration::from_secs(5)).expect("a report for the broken file");
        assert!(!r.valid());
        drop(handle);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
