//! Song tools: the analysis of the project's audio (`edits_analyze::song`,
//! the library behind `edits audio`, run on a worker thread), its marks on
//! the timeline (downbeats, kick and snare onsets, sections, the drop) with
//! switches for each, and the forms of "Cut on beats…" and "Accent on…",
//! which call the same `edits_core::beatops` and `edits_core::preset`
//! functions as `edits beats cut` and `edits beats accent`.
//!
//! The analysis is kept in `<project dir>/.edits/song.audio.json` (format
//! `edits.audio/1`, the file `edits audio -o` writes), so it is there the
//! next time the window opens and an agent can pass it as `audio`.

use edits_core::beatops::{self, CutOpts, Place};
use edits_core::preset::Preset;
use edits_core::song::SongInfo;
use edits_core::time::TimeCtx;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::SystemTime;

/// Where the window keeps the analysis of a project's audio.
pub fn analysis_path(project: &Path) -> PathBuf {
    edits_core::edit::project_dir(project).join(".edits").join("song.audio.json")
}

/// The analysis saved for this audio file, if it is newer than the audio and was made of it.
pub fn load_saved(project: &Path, audio: &Path) -> Option<Value> {
    let path = analysis_path(project);
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if modified(&path)? < modified(audio).unwrap_or(SystemTime::UNIX_EPOCH) {
        return None;
    }
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
    let source = PathBuf::from(v.get("source")?.as_str()?);
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    (canon(&source) == canon(audio)).then_some(v)
}

/// The analysis running on its own thread.
pub struct Job {
    rx: Receiver<Result<Value, String>>,
    pub audio: PathBuf,
}

impl Job {
    /// Analyse `audio` and save the result for `project`; `wake` is called when it is done.
    pub fn start(project: &Path, audio: &Path, wake: impl Fn() + Send + 'static) -> Job {
        let (tx, rx) = mpsc::channel();
        let (out, file) = (analysis_path(project), audio.to_path_buf());
        let dir = edits_core::edit::project_dir(project);
        std::thread::Builder::new()
            .name("edits-song".into())
            .spawn(move || {
                let result = (|| {
                    let song = edits_analyze::song::analyze_file(&file, &edits_analyze::song::SongOpts::default()).map_err(|e| format!("{e:#}"))?;
                    let value = serde_json::to_value(&song).map_err(|e| e.to_string())?;
                    let text = edits_analyze::jsonfmt::to_string(&song).map_err(|e| e.to_string())?;
                    let _ = edits_core::edit::state_dir(&dir, "");
                    edits_core::edit::write_atomic(&out, &text).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
                    Ok(value)
                })();
                let _ = tx.send(result);
                wake();
            })
            .expect("can start a thread");
        Job { rx, audio: audio.to_path_buf() }
    }

    /// The result, once.
    pub fn poll(&self) -> Option<Result<Value, String>> {
        self.rx.try_recv().ok()
    }
}

/// The analysis in project time, for the timeline.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marks {
    pub bpm: f64,
    pub downbeats: Vec<f64>,
    /// (time, strength 0..1).
    pub kicks: Vec<(f64, f64)>,
    pub snares: Vec<(f64, f64)>,
    /// (label, from, to).
    pub sections: Vec<(String, f64, f64)>,
    pub drop: Option<f64>,
    /// Notes of the analysis (a guessed bar phase, no drop).
    pub notes: Vec<String>,
}

fn hits(v: &Value, ptr: &str, units: &dyn Fn(f64) -> f64) -> Vec<(f64, f64)> {
    v.pointer(ptr).and_then(Value::as_array).into_iter().flatten().filter_map(|h| Some((units(h.get("t")?.as_f64()?), h.get("s").and_then(Value::as_f64).unwrap_or(1.0)))).collect()
}

/// Read an analysis (`edits.audio/1`, times in seconds of the audio file)
/// into project time: `audio_offset` is where the project starts in the file.
pub fn marks(analysis: &Value, tc: &TimeCtx, audio_offset: f64) -> Result<Marks, String> {
    let info = SongInfo::from_value(analysis)?;
    let units = |s: f64| crate::editing::tidy(tc.seconds_to_units(s - audio_offset));
    let sections = analysis
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| Some((s.get("label")?.as_str()?.to_string(), units(s.get("from")?.as_f64()?), units(s.get("to")?.as_f64()?))))
        .collect();
    Ok(Marks {
        bpm: info.bpm,
        downbeats: info.downbeats.iter().map(|t| units(*t)).collect(),
        kicks: hits(analysis, "/onsets/kick", &units),
        snares: hits(analysis, "/onsets/snare", &units),
        sections,
        drop: info.drop.map(units),
        notes: analysis.get("notes").and_then(Value::as_array).into_iter().flatten().filter_map(|n| n.as_str().map(str::to_string)).collect(),
    })
}

/// Which marks the timeline shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Toggles {
    pub downbeats: bool,
    pub kicks: bool,
    pub snares: bool,
    /// Sections and the drop.
    pub sections: bool,
}

impl Default for Toggles {
    fn default() -> Self {
        Toggles { downbeats: true, kicks: true, snares: true, sections: true }
    }
}

/// One switch of [`Toggles`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Downbeats,
    Kicks,
    Snares,
    Sections,
}

impl Layer {
    pub const ALL: [Layer; 4] = [Layer::Downbeats, Layer::Kicks, Layer::Snares, Layer::Sections];

    pub fn name(self) -> &'static str {
        match self {
            Layer::Downbeats => "downbeats",
            Layer::Kicks => "kicks",
            Layer::Snares => "snares",
            Layer::Sections => "sections and drop",
        }
    }
}

impl Toggles {
    pub fn get(&self, l: Layer) -> bool {
        match l {
            Layer::Downbeats => self.downbeats,
            Layer::Kicks => self.kicks,
            Layer::Snares => self.snares,
            Layer::Sections => self.sections,
        }
    }

    pub fn flip(&mut self, l: Layer) {
        let v = !self.get(l);
        match l {
            Layer::Downbeats => self.downbeats = v,
            Layer::Kicks => self.kicks = v,
            Layer::Snares => self.snares = v,
            Layer::Sections => self.sections = v,
        }
    }

    pub fn any(&self) -> bool {
        Layer::ALL.iter().any(|l| self.get(*l))
    }
}

/// A mark to draw.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Downbeat(f64),
    Kick(f64, f64),
    Snare(f64, f64),
    Section(String, f64, f64),
    Drop(f64),
}

/// The marks switched on that fall in [from, to] (sections: that overlap it).
pub fn visible(m: &Marks, on: &Toggles, from: f64, to: f64) -> Vec<Mark> {
    let inside = |t: f64| t >= from && t <= to;
    let mut out = Vec::new();
    if on.sections {
        out.extend(m.sections.iter().filter(|(_, a, b)| *b >= from && *a <= to).map(|(l, a, b)| Mark::Section(l.clone(), *a, *b)));
    }
    if on.downbeats {
        out.extend(m.downbeats.iter().filter(|t| inside(**t)).map(|t| Mark::Downbeat(*t)));
    }
    if on.kicks {
        out.extend(m.kicks.iter().filter(|(t, _)| inside(*t)).map(|(t, s)| Mark::Kick(*t, *s)));
    }
    if on.snares {
        out.extend(m.snares.iter().filter(|(t, _)| inside(*t)).map(|(t, s)| Mark::Snare(*t, *s)));
    }
    if on.sections {
        out.extend(m.drop.filter(|t| inside(*t)).map(Mark::Drop));
    }
    out
}

/// A one-line summary of an analysis.
pub fn summary(m: &Marks) -> String {
    let mut s = format!("{} BPM · {} bars · {} kicks · {} snares", crate::editing::tidy(m.bpm), m.downbeats.len(), m.kicks.len(), m.snares.len());
    match m.drop {
        Some(d) => s.push_str(&format!(" · drop at {}", crate::editing::tidy(d))),
        None => s.push_str(" · no drop"),
    }
    s
}

// ---- forms ----------------------------------------------------------------------

/// The range a form works in: `from` and `to` as typed (`bar 2`, `drop`, `b8`), empty = start / end.
fn range(tc: &TimeCtx, from: &str, to: &str) -> Result<(f64, f64), String> {
    let from_t = if from.trim().is_empty() { 0.0 } else { tc.resolve(from).map_err(|e| format!("from: {e}"))? };
    let to_t = if to.trim().is_empty() { tc.duration } else { tc.resolve(to).map_err(|e| format!("to: {e}"))? };
    if to_t <= from_t {
        return Err(format!("to ({to_t}) must be after from ({from_t})"));
    }
    Ok((from_t, to_t))
}

fn audio_offset(doc: &Value) -> f64 {
    doc.pointer("/audio/offset").and_then(Value::as_f64).unwrap_or(0.0)
}

/// What to cut on.
#[derive(Clone, Debug, PartialEq)]
pub enum CutOn {
    /// Every N beats (4 = a bar, 0.5 = two per beat).
    Every(f64),
    /// `beats`, `offbeats`, `downbeats`, `kicks`, `snares`, `hats`, or a list of times.
    On(String),
}

/// "Cut on beats…": the fields of `edits beats cut`.
#[derive(Clone, Debug)]
pub struct CutForm {
    /// Every slot of the project, and whether it is used.
    pub slots: Vec<(String, bool)>,
    pub on: CutOn,
    pub from: String,
    pub to: String,
    pub min_strength: f64,
    pub prefix: String,
}

impl CutForm {
    pub fn new(slot_names: impl IntoIterator<Item = String>) -> Self {
        CutForm { slots: slot_names.into_iter().map(|s| (s, true)).collect(), on: CutOn::Every(1.0), from: String::new(), to: String::new(), min_strength: 0.3, prefix: "cut".into() }
    }

    /// The options of `beatops::cut` for `doc`, the time context and the history line.
    pub fn plan(&self, doc: &Value, song: Option<&SongInfo>) -> Result<(CutOpts, TimeCtx, String), String> {
        let tc = crate::presets::time_ctx(doc, song);
        let (from, to) = range(&tc, &self.from, &self.to)?;
        let slots: Vec<String> = self.slots.iter().filter(|(_, on)| *on).map(|(s, _)| s.clone()).collect();
        if slots.is_empty() {
            return Err("pick at least one slot".into());
        }
        let (times, every) = match &self.on {
            CutOn::Every(n) if *n > 0.0 => (None, *n),
            CutOn::Every(_) => return Err("cut every how many beats? (more than 0)".into()),
            CutOn::On(on) => (Some(beatops::times(on, song, &tc, from, to, self.min_strength, audio_offset(doc))?), 1.0),
        };
        if times.as_ref().is_some_and(Vec::is_empty) {
            return Err("no cut times in that range".into());
        }
        let prefix = if self.prefix.trim().is_empty() { "cut".to_string() } else { self.prefix.trim().to_string() };
        let label = format!("beats cut {} from {from} to {to}", slots.join(","));
        Ok((CutOpts { slots, times, every, from, to, prefix, place: Place::Default }, tc, label))
    }

    /// The document after the cut, and how many layers it adds.
    pub fn apply(&self, doc: &Value, song: Option<&SongInfo>) -> Result<(Value, usize), String> {
        let (opts, tc, _) = self.plan(doc, song)?;
        beatops::cut(doc, &opts, &tc).map(|(d, r)| (d, r.layers.len()))
    }
}

/// "Accent on…": the fields of `edits beats accent`.
#[derive(Clone, Debug)]
pub struct AccentForm {
    pub preset: String,
    pub values: Map<String, Value>,
    /// `beats`, `offbeats`, `downbeats`, `kicks`, `snares`, `hats`, `drop`, or a list of times.
    pub on: String,
    pub target: crate::presets::TargetChoice,
    pub from: String,
    pub to: String,
    pub min_strength: f64,
    pub every_nth: usize,
}

impl AccentForm {
    pub fn new(p: &Preset) -> Self {
        AccentForm {
            preset: p.name.clone(),
            values: crate::presets::defaults(p),
            on: "downbeats".into(),
            target: crate::presets::TargetChoice::Auto,
            from: String::new(),
            to: String::new(),
            min_strength: 0.3,
            every_nth: 1,
        }
    }

    /// The times the preset goes at (project time).
    pub fn times(&self, doc: &Value, song: Option<&SongInfo>) -> Result<Vec<f64>, String> {
        let tc = crate::presets::time_ctx(doc, song);
        let (from, to) = range(&tc, &self.from, &self.to)?;
        let mut times = beatops::times(&self.on, song, &tc, from, to, self.min_strength, audio_offset(doc))?;
        if self.every_nth > 1 {
            times = times.into_iter().step_by(self.every_nth).collect();
        }
        if times.is_empty() {
            return Err(format!("no {} between {from} and {to}", self.on));
        }
        Ok(times)
    }

    /// The document with the preset at every time, and the history line.
    pub fn apply(&self, doc: &Value, p: &Preset, selected: Option<&str>, song: Option<&SongInfo>) -> Result<(Value, String), String> {
        let times = self.times(doc, song)?;
        let tc = crate::presets::time_ctx(doc, song);
        let target = crate::presets::target(p, self.target, selected);
        let (out, _) = edits_core::preset::apply(doc, p, &target, &times, &crate::presets::given(p, &self.values), &tc)?;
        Ok((out, format!("beats accent {} on {} ({} times)", p.name, self.on, times.len())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Value {
        json!({
            "format": "edits.project/1",
            "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 120, "beat_offset": 0.0 },
            "audio": { "path": "song.wav", "offset": 0.5 },
            "slots": { "a": {}, "b": {}, "c": {} },
            "layers": [ { "id": "base", "kind": "image", "slot": "a", "in": 0, "out": 8 } ]
        })
    }

    /// 120 BPM from 0.5 s into the file: file seconds 0.5 + 0.5 b = project beat b.
    fn analysis() -> Value {
        json!({
            "format": "edits.audio/1", "source": "song.wav", "duration": 5.0,
            "tempo": { "bpm": 120, "confidence": 0.9, "candidates": [], "on_grid": 1.0 },
            "grid": { "beat_offset": 0.5, "beat_seconds": 0.5, "beats_per_bar": 4, "downbeat": 0, "downbeat_confidence": 0.5 },
            "beats": (0..8).map(|b| json!({ "b": b, "t": 0.5 + 0.5 * b as f64, "bar": 1 + b / 4, "beat": 1 + b % 4, "s": 1.0 })).collect::<Vec<_>>(),
            "downbeats": [ { "bar": 1, "t": 0.5, "b": 0 }, { "bar": 2, "t": 2.5, "b": 4 } ],
            "onsets": {
                "kick": [ { "t": 0.5, "b": 0, "s": 1.0 }, { "t": 1.5, "b": 2, "s": 0.2 }, { "t": 2.5, "b": 4, "s": 0.9 } ],
                "snare": [ { "t": 1.0, "b": 1, "s": 0.8 }, { "t": 2.0, "b": 3, "s": 0.7 } ],
                "hat": []
            },
            "energy": [], "sections": [ { "label": "intro", "from": 0.5, "to": 2.5, "from_beat": 0, "to_beat": 4, "bar": 1, "bars": 1, "energy": 0.3 },
                                        { "label": "drop", "from": 2.5, "to": 4.5, "from_beat": 4, "to_beat": 8, "bar": 2, "bars": 1, "energy": 0.9 } ],
            "drop": { "t": 2.5, "b": 4, "bar": 2, "rise": 0.6 },
            "loudness": {}, "markers": [ { "label": "drop", "t": 2.5, "b": 4 }, { "label": "intro", "t": 0.5, "b": 0 } ],
            "notes": ["a note"]
        })
    }

    #[test]
    fn an_analysis_lands_in_project_time() {
        let d = doc();
        let m = marks(&analysis(), &TimeCtx::from_doc(&d), 0.5).unwrap();
        assert_eq!(m.downbeats, vec![0.0, 4.0]);
        assert_eq!(m.kicks, vec![(0.0, 1.0), (2.0, 0.2), (4.0, 0.9)]);
        assert_eq!(m.snares[0], (1.0, 0.8));
        assert_eq!(m.sections[1], ("drop".to_string(), 4.0, 8.0));
        assert_eq!(m.drop, Some(4.0));
        assert_eq!(m.notes, vec!["a note".to_string()]);
        assert_eq!(summary(&m), "120 BPM · 2 bars · 3 kicks · 2 snares · drop at 4");
        assert!(marks(&json!({ "format": "nope" }), &TimeCtx::from_doc(&d), 0.0).is_err());
    }

    #[test]
    fn toggles_choose_the_marks() {
        let d = doc();
        let m = marks(&analysis(), &TimeCtx::from_doc(&d), 0.5).unwrap();
        let mut on = Toggles::default();
        assert!(on.any());
        let all = visible(&m, &on, 0.0, 8.0);
        assert_eq!(all.len(), 2 + 2 + 3 + 2 + 1);
        on.flip(Layer::Kicks);
        on.flip(Layer::Sections);
        assert!(!on.kicks && !on.sections && on.get(Layer::Snares));
        let some = visible(&m, &on, 0.0, 8.0);
        assert!(some.iter().all(|x| matches!(x, Mark::Downbeat(_) | Mark::Snare(..))));
        // Only what is in view.
        assert_eq!(visible(&m, &on, 1.5, 3.5), vec![Mark::Snare(3.0, 0.7)]);
        on.flip(Layer::Downbeats);
        on.flip(Layer::Snares);
        assert!(!on.any());
        assert!(visible(&m, &on, 0.0, 8.0).is_empty());
    }

    #[test]
    fn a_cut_form_is_the_cut_command() {
        let d = doc();
        let mut f = CutForm::new(["a".to_string(), "b".to_string(), "c".to_string()]);
        f.slots[2].1 = false;
        f.on = CutOn::Every(2.0);
        f.from = "b2".into();
        let (opts, _, label) = f.plan(&d, None).unwrap();
        assert_eq!((opts.from, opts.to, opts.every), (2.0, 8.0, 2.0));
        assert_eq!(opts.slots, vec!["a", "b"]);
        assert_eq!(label, "beats cut a,b from 2 to 8");
        let (out, n) = f.apply(&d, None).unwrap();
        assert_eq!(n, 3);
        let ids: Vec<&str> = out["layers"].as_array().unwrap().iter().filter_map(|l| l["id"].as_str()).collect();
        assert_eq!(ids, vec!["base", "cut1", "cut2", "cut3"]);
        // Kicks need the analysis; with it, the strong ones only.
        f.on = CutOn::On("kicks".into());
        f.from.clear();
        assert!(f.plan(&d, None).unwrap_err().contains("song analysis"));
        let info = SongInfo::from_value(&analysis()).unwrap();
        let (opts, _, _) = f.plan(&d, Some(&info)).unwrap();
        assert_eq!(opts.times, Some(vec![0.0, 4.0]));
        f.slots.iter_mut().for_each(|s| s.1 = false);
        assert!(f.plan(&d, Some(&info)).is_err());
        let mut g = CutForm::new(["a".to_string()]);
        g.to = "b0".into();
        assert!(g.plan(&d, None).unwrap_err().contains("after"));
    }

    #[test]
    fn an_accent_form_is_the_accent_command() {
        let d = doc();
        let lib = crate::presets::Library::load(None);
        let flash = lib.get("flash").unwrap();
        let mut f = AccentForm::new(flash);
        assert_eq!(f.times(&d, None).unwrap(), vec![0.0, 4.0]);
        f.on = "beats".into();
        f.every_nth = 2;
        f.to = "b6".into();
        assert_eq!(f.times(&d, None).unwrap(), vec![0.0, 2.0, 4.0]);
        let (out, label) = f.apply(&d, flash, None, None).unwrap();
        assert_eq!(label, "beats accent flash on beats (3 times)");
        assert!(out.pointer("/layers/0/effects").is_some(), "the flash lands on the picture on screen: {out}");
        f.on = "snares".into();
        f.every_nth = 1;
        f.to.clear();
        let info = SongInfo::from_value(&analysis()).unwrap();
        assert_eq!(f.times(&d, Some(&info)).unwrap(), vec![1.0, 3.0]);
        f.on = "drop".into();
        assert_eq!(f.times(&d, Some(&info)).unwrap(), vec![4.0]);
        f.on = "hats".into();
        assert!(f.times(&d, Some(&info)).unwrap_err().contains("no hats"));
    }

    #[test]
    fn a_saved_analysis_is_found_for_its_audio_only() {
        let dir = std::env::temp_dir().join(format!("edits-gui-song-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".edits")).unwrap();
        let project = dir.join("project.json");
        let audio = dir.join("song.wav");
        std::fs::write(&audio, b"RIFF").unwrap();
        assert!(load_saved(&project, &audio).is_none());
        let mut a = analysis();
        a["source"] = json!(audio.display().to_string());
        std::fs::write(analysis_path(&project), a.to_string()).unwrap();
        assert!(load_saved(&project, &audio).is_some());
        assert!(load_saved(&project, &dir.join("other.wav")).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
