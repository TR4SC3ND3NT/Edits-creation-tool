//! Song tools for the Edit page: the analysis of the project's audio
//! (`edits_analyze::song`, the library behind `edits audio`, run on a
//! thread), its marks in project time for the timeline (beats, downbeats,
//! kick, snare and hat onsets, sections, the drop), the waveform of the
//! audio, and "Beat cut" and "Accent", which call the same
//! `edits_core::beatops` and `edits_core::preset` functions as
//! `edits beats cut` and `edits beats accent`.
//!
//! The analysis is kept in `<project dir>/.edits/song.audio.json` (format
//! `edits.audio/1`), the file the egui window keeps too, so either window
//! and the agent (as `audio` of `beats_cut`) find it.

use crate::engine::editing::tidy;
use edits_core::beatops::{self, CutOpts, Place};
use edits_core::preset::{self, Target, TargetKind};
use edits_core::song::SongInfo;
use edits_core::time::TimeCtx;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Where the analysis of a project's audio is kept.
pub fn analysis_path(project: &Path) -> PathBuf {
    edits_core::edit::project_dir(project)
        .join(".edits")
        .join("song.audio.json")
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

/// Analyse `audio` and keep the result next to `project`.
pub fn analyze(project: &Path, audio: &Path) -> Result<Value, String> {
    let song = edits_analyze::song::analyze_file(audio, &edits_analyze::song::SongOpts::default())
        .map_err(|e| format!("{e:#}"))?;
    let value = serde_json::to_value(&song).map_err(|e| e.to_string())?;
    let text = edits_analyze::jsonfmt::to_string(&song).map_err(|e| e.to_string())?;
    let dir = edits_core::edit::project_dir(project);
    let _ = edits_core::edit::state_dir(&dir, "");
    let out = analysis_path(project);
    edits_core::edit::write_atomic(&out, &text)
        .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    Ok(value)
}

/// The analysis in project time, for the timeline.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Marks {
    pub bpm: f64,
    pub beats: Vec<f64>,
    pub downbeats: Vec<f64>,
    /// (time, strength 0..1).
    pub kicks: Vec<(f64, f64)>,
    pub snares: Vec<(f64, f64)>,
    pub hats: Vec<(f64, f64)>,
    /// (label, from, to).
    pub sections: Vec<(String, f64, f64)>,
    pub drop: Option<f64>,
    pub notes: Vec<String>,
    pub summary: String,
}

fn hits(v: &Value, ptr: &str, units: &dyn Fn(f64) -> f64) -> Vec<(f64, f64)> {
    v.pointer(ptr)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|h| {
            Some((
                units(h.get("t")?.as_f64()?),
                h.get("s").and_then(Value::as_f64).unwrap_or(1.0),
            ))
        })
        .collect()
}

/// Read an analysis (times in seconds of the audio file) into project time:
/// `audio_offset` is where the project starts in the file.
pub fn marks(analysis: &Value, tc: &TimeCtx, audio_offset: f64) -> Result<Marks, String> {
    let info = SongInfo::from_value(analysis)?;
    let units = |s: f64| tidy(tc.seconds_to_units(s - audio_offset));
    let sections = analysis
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            Some((
                s.get("label")?.as_str()?.to_string(),
                units(s.get("from")?.as_f64()?),
                units(s.get("to")?.as_f64()?),
            ))
        })
        .collect();
    let mut m = Marks {
        bpm: info.bpm,
        beats: info.beats.iter().map(|t| units(*t)).collect(),
        downbeats: info.downbeats.iter().map(|t| units(*t)).collect(),
        kicks: hits(analysis, "/onsets/kick", &units),
        snares: hits(analysis, "/onsets/snare", &units),
        hats: hits(analysis, "/onsets/hat", &units),
        sections,
        drop: info.drop.map(units),
        notes: analysis
            .get("notes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|n| n.as_str().map(str::to_string))
            .collect(),
        summary: String::new(),
    };
    m.summary = summary(&m);
    Ok(m)
}

/// `123 BPM · 2 bars · 8 kicks · 16 snares · no drop`.
pub fn summary(m: &Marks) -> String {
    let mut s = format!(
        "{} BPM · {} bars · {} kicks · {} snares",
        tidy(m.bpm),
        m.downbeats.len(),
        m.kicks.len(),
        m.snares.len()
    );
    match m.drop {
        Some(d) => s.push_str(&format!(" · drop at {}", tidy(d))),
        None => s.push_str(" · no drop"),
    }
    s
}

// ---- the waveform ----------------------------------------------------------------

/// Peaks of the audio file: `rate` values a second from the start of the file, each the largest |sample| in its window (0..1).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Waveform {
    pub rate: f64,
    pub peaks: Vec<f32>,
}

/// Samples are decoded at this rate for the waveform.
const WAVE_SR: u32 = 8000;

pub fn peaks(samples: &[f32], sr: u32, rate: f64) -> Waveform {
    let win = ((sr as f64 / rate).round() as usize).max(1);
    let mut peaks: Vec<f32> = samples
        .chunks(win)
        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect();
    let top = peaks.iter().copied().fold(0.0f32, f32::max);
    if top > 0.0 {
        for p in &mut peaks {
            *p = ((*p / top) * 1000.0).round() / 1000.0;
        }
    }
    Waveform {
        rate: sr as f64 / win as f64,
        peaks,
    }
}

pub fn waveform(audio: &Path) -> Result<Waveform, String> {
    let samples = edits_analyze::media::decode_audio_file(audio, WAVE_SR)
        .map_err(|e| format!("cannot decode {}: {e:#}", audio.display()))?;
    Ok(peaks(&samples, WAVE_SR, 100.0))
}

// ---- state ---------------------------------------------------------------------------

/// What the timeline shows of the song.
#[derive(Serialize, Clone, Debug, Default)]
pub struct SongDto {
    /// The project's audio file, if it exists.
    pub audio: Option<String>,
    pub running: bool,
    pub error: Option<String>,
    pub marks: Option<Marks>,
}

/// The song tools' state, managed by Tauri next to the studio.
#[derive(Default)]
pub struct SongService {
    pub running: Mutex<Option<PathBuf>>,
    pub error: Mutex<Option<String>>,
    /// Waveforms by audio path and modification time.
    pub waves: Mutex<HashMap<(PathBuf, Option<SystemTime>), Arc<Waveform>>>,
}

impl SongService {
    pub fn wave(&self, audio: &Path) -> Result<Arc<Waveform>, String> {
        let key = (
            audio.to_path_buf(),
            std::fs::metadata(audio).and_then(|m| m.modified()).ok(),
        );
        if let Some(w) = self.waves.lock().ok().and_then(|m| m.get(&key).cloned()) {
            return Ok(w);
        }
        let w = Arc::new(waveform(audio)?);
        if let Ok(mut m) = self.waves.lock() {
            m.insert(key, w.clone());
        }
        Ok(w)
    }
}

/// The saved analysis of the project's audio, as song info for times.
pub fn song_info(project: &Path, doc: &Value) -> Option<SongInfo> {
    let audio = audio_file(project, doc)?;
    SongInfo::from_value(&load_saved(project, &audio)?).ok()
}

/// The project's audio file, absolute.
pub fn audio_file(project: &Path, doc: &Value) -> Option<PathBuf> {
    let p = doc.pointer("/audio/path")?.as_str()?;
    let full = edits_core::edit::project_dir(project).join(p);
    full.is_file().then_some(full)
}

pub fn audio_offset(doc: &Value) -> f64 {
    doc.pointer("/audio/offset")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

pub fn time_ctx(doc: &Value, song: Option<&SongInfo>) -> TimeCtx {
    let tc = TimeCtx::from_doc(doc);
    match song {
        Some(s) => tc.with_song(s, audio_offset(doc)),
        None => tc,
    }
}

// ---- beat cut and accent -----------------------------------------------------------

/// The range a form works in: `from` and `to` as typed (`bar 2`, `drop`, `b8`), empty = start / end.
fn range(tc: &TimeCtx, from: &str, to: &str) -> Result<(f64, f64), String> {
    let from_t = if from.trim().is_empty() {
        0.0
    } else {
        tc.resolve(from).map_err(|e| format!("from: {e}"))?
    };
    let to_t = if to.trim().is_empty() {
        tc.duration
    } else {
        tc.resolve(to).map_err(|e| format!("to: {e}"))?
    };
    if to_t <= from_t {
        return Err(format!("to ({to_t}) must be after from ({from_t})"));
    }
    Ok((from_t, to_t))
}

/// "Beat cut": the fields of `edits beats cut`.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct CutRequest {
    pub slots: Vec<String>,
    /// Beats per picture; used when `on` is empty.
    #[serde(default)]
    pub every: Option<f64>,
    /// `beats`, `offbeats`, `downbeats`, `kicks`, `snares`, `hats`, or a list of times.
    #[serde(default)]
    pub on: Option<String>,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default = "strength")]
    pub min_strength: f64,
    #[serde(default)]
    pub prefix: String,
}

fn strength() -> f64 {
    0.3
}

impl CutRequest {
    pub fn plan(
        &self,
        doc: &Value,
        song: Option<&SongInfo>,
    ) -> Result<(CutOpts, TimeCtx, String), String> {
        let tc = time_ctx(doc, song);
        let (from, to) = range(&tc, &self.from, &self.to)?;
        if self.slots.is_empty() {
            return Err("pick at least one picture".into());
        }
        let on = self.on.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let (times, every) = match on {
            Some(on) => (
                Some(beatops::times(
                    on,
                    song,
                    &tc,
                    from,
                    to,
                    self.min_strength,
                    audio_offset(doc),
                )?),
                1.0,
            ),
            None => match self.every.unwrap_or(1.0) {
                n if n > 0.0 => (None, n),
                _ => return Err("cut every how many beats? (more than 0)".into()),
            },
        };
        if times.as_ref().is_some_and(Vec::is_empty) {
            return Err("no cut times in that range".into());
        }
        let prefix = if self.prefix.trim().is_empty() {
            "cut".to_string()
        } else {
            self.prefix.trim().to_string()
        };
        let label = format!(
            "beats cut {} from {} to {}",
            self.slots.join(","),
            tidy(from),
            tidy(to)
        );
        Ok((
            CutOpts {
                slots: self.slots.clone(),
                times,
                every,
                from,
                to,
                prefix,
                place: Place::Default,
            },
            tc,
            label,
        ))
    }

    /// The document after the cut, the number of layers it adds and the history line.
    pub fn apply(
        &self,
        doc: &Value,
        song: Option<&SongInfo>,
    ) -> Result<(Value, usize, String), String> {
        let (opts, tc, label) = self.plan(doc, song)?;
        beatops::cut(doc, &opts, &tc).map(|(d, r)| (d, r.layers.len(), label))
    }
}

/// "Accent": the fields of `edits beats accent`.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct AccentRequest {
    pub preset: String,
    /// `beats`, `offbeats`, `downbeats`, `kicks`, `snares`, `hats`, `drop`, or a list of times.
    pub on: String,
    /// `selected`, `auto` or `fx`.
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub selected: Option<String>,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default = "strength")]
    pub min_strength: f64,
    #[serde(default)]
    pub every_nth: usize,
    #[serde(default)]
    pub values: Map<String, Value>,
}

impl AccentRequest {
    pub fn times(&self, doc: &Value, song: Option<&SongInfo>) -> Result<Vec<f64>, String> {
        let tc = time_ctx(doc, song);
        let (from, to) = range(&tc, &self.from, &self.to)?;
        let mut times = beatops::times(
            &self.on,
            song,
            &tc,
            from,
            to,
            self.min_strength,
            audio_offset(doc),
        )?;
        if self.every_nth > 1 {
            times = times.into_iter().step_by(self.every_nth).collect();
        }
        if times.is_empty() {
            return Err(format!(
                "no {} between {} and {}",
                self.on,
                tidy(from),
                tidy(to)
            ));
        }
        Ok(times)
    }

    pub fn apply(
        &self,
        doc: &Value,
        dir: Option<&Path>,
        song: Option<&SongInfo>,
    ) -> Result<(Value, usize, String), String> {
        let p = preset::find(&self.preset, dir)?;
        let times = self.times(doc, song)?;
        let tc = time_ctx(doc, song);
        let target = if p.target == TargetKind::Project {
            Target::None
        } else {
            match (self.target.as_str(), self.selected.as_deref()) {
                ("selected", Some(id)) => Target::Layer(id.to_string()),
                ("fx", _) => Target::Fx,
                _ => Target::Auto,
            }
        };
        let given: Map<String, Value> = self
            .values
            .iter()
            .filter(|(n, v)| p.variables.get(*n).is_some_and(|var| &var.default != *v))
            .map(|(n, v)| (n.clone(), v.clone()))
            .collect();
        let (out, _) = preset::apply(doc, &p, &target, &times, &given, &tc)?;
        let n = times.len();
        Ok((
            out,
            n,
            format!("beats accent {} on {} ({n} times)", p.name, self.on),
        ))
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
            "energy": [], "sections": [ { "label": "intro", "from": 0.5, "to": 2.5 }, { "label": "drop", "from": 2.5, "to": 4.5 } ],
            "drop": { "t": 2.5, "b": 4, "bar": 2, "rise": 0.6 },
            "markers": [ { "label": "drop", "t": 2.5, "b": 4 } ],
            "notes": ["a note"]
        })
    }

    #[test]
    fn an_analysis_lands_in_project_time() {
        let d = doc();
        let m = marks(&analysis(), &TimeCtx::from_doc(&d), 0.5).unwrap();
        assert_eq!(m.beats.len(), 8);
        assert_eq!(m.beats[3], 3.0);
        assert_eq!(m.downbeats, vec![0.0, 4.0]);
        assert_eq!(m.kicks, vec![(0.0, 1.0), (2.0, 0.2), (4.0, 0.9)]);
        assert_eq!(m.sections[1], ("drop".to_string(), 4.0, 8.0));
        assert_eq!(m.drop, Some(4.0));
        assert_eq!(
            m.summary,
            "120 BPM · 2 bars · 3 kicks · 2 snares · drop at 4"
        );
    }

    #[test]
    fn peaks_are_normalised_windows() {
        let samples: Vec<f32> = (0..800)
            .map(|i| if i < 400 { 0.25 } else { -0.5 })
            .collect();
        let w = peaks(&samples, 8000, 20.0);
        assert_eq!(w.rate, 20.0);
        assert_eq!(w.peaks.len(), 2);
        assert_eq!(w.peaks, vec![0.5, 1.0]);
    }

    #[test]
    fn beat_cut_is_the_cut_command() {
        let d = doc();
        let req = CutRequest {
            slots: vec!["a".into(), "b".into()],
            every: Some(2.0),
            from: "b2".into(),
            ..Default::default()
        };
        let (out, n, label) = req.apply(&d, None).unwrap();
        assert_eq!(n, 3);
        assert_eq!(label, "beats cut a,b from 2 to 8");
        assert_eq!(out["layers"].as_array().unwrap().len(), 4);
        let kicks = CutRequest {
            slots: vec!["a".into()],
            on: Some("kicks".into()),
            min_strength: 0.3,
            ..Default::default()
        };
        assert!(kicks.plan(&d, None).unwrap_err().contains("song analysis"));
        let info = SongInfo::from_value(&analysis()).unwrap();
        let (opts, _, _) = kicks.plan(&d, Some(&info)).unwrap();
        assert_eq!(opts.times, Some(vec![0.0, 4.0]));
        assert!(CutRequest::default().plan(&d, None).is_err());
    }

    #[test]
    fn accent_puts_a_preset_on_every_time() {
        let d = doc();
        let req = AccentRequest {
            preset: "flash".into(),
            on: "beats".into(),
            to: "b6".into(),
            every_nth: 2,
            ..Default::default()
        };
        assert_eq!(req.times(&d, None).unwrap(), vec![0.0, 2.0, 4.0]);
        let (out, n, label) = req.apply(&d, None, None).unwrap();
        assert_eq!(n, 3);
        assert_eq!(label, "beats accent flash on beats (3 times)");
        assert!(out.pointer("/layers/0/effects").is_some());
        let info = SongInfo::from_value(&analysis()).unwrap();
        let kicks = AccentRequest {
            preset: "punch".into(),
            on: "kicks".into(),
            min_strength: 0.3,
            ..Default::default()
        };
        assert_eq!(kicks.times(&d, Some(&info)).unwrap(), vec![0.0, 4.0]);
    }
}
