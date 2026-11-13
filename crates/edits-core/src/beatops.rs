//! Beat operations: the skeleton an editor builds by hand, in one step.
//!
//! - [`cut`] puts one image layer per beat (or every few beats, or on each
//!   onset of a song) between two times, cycling through slots.
//! - [`times`] lists the times of beats, bars, onsets or the drop, which
//!   [`crate::preset::apply`] then accents.
//! - [`sync`] writes a song analysis' tempo, beat offset and markers into a
//!   project.
//!
//! Like presets, these change a JSON document; the callers write it through
//! [`crate::edit::apply_with`], so the result is validated and undoable.

use crate::song::SongInfo;
use crate::time::TimeCtx;
use serde::Serialize;
use serde_json::{json, Value};

/// Where new layers go.
#[derive(Clone, Debug, Default)]
pub enum Place {
    /// On top, but below the adjustment layers at the top of the stack (so a
    /// finishing grade still applies).
    #[default]
    Default,
    Above(String),
    Below(String),
}

#[derive(Clone, Debug)]
pub struct CutOpts {
    pub slots: Vec<String>,
    /// Cut times (project time), or `None` for every `every` beats.
    pub times: Option<Vec<f64>>,
    /// Beats per picture when `times` is `None`.
    pub every: f64,
    pub from: f64,
    pub to: f64,
    /// Ids are `<prefix><n>`.
    pub prefix: String,
    pub place: Place,
}

#[derive(Serialize, Clone, Debug)]
pub struct CutReport {
    /// (id, slot, in, out) of every layer added.
    pub layers: Vec<(String, String, f64, f64)>,
    /// The new project length, when the cuts made it longer.
    pub extended_to: Option<f64>,
}

fn r(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

fn insert_index(layers: &[Value], place: &Place) -> Result<usize, String> {
    let find = |id: &str| {
        layers.iter().position(|l| l.get("id").and_then(Value::as_str) == Some(id)).ok_or_else(|| {
            let ids: Vec<&str> = layers.iter().filter_map(|l| l.get("id").and_then(Value::as_str)).collect();
            format!("no layer has id \"{id}\"; layers, bottom to top: {}", ids.join(", "))
        })
    };
    Ok(match place {
        Place::Above(id) => find(id)? + 1,
        Place::Below(id) => find(id)?,
        Place::Default => {
            let mut i = layers.len();
            while i > 0 && layers[i - 1].get("kind").and_then(Value::as_str) == Some("adjustment") {
                i -= 1;
            }
            i
        }
    })
}

/// Cut `slots` to the beat between `from` and `to`. See [`CutOpts`].
pub fn cut(doc: &Value, o: &CutOpts, tc: &TimeCtx) -> Result<(Value, CutReport), String> {
    if o.slots.is_empty() {
        return Err("give at least one slot to cut between".into());
    }
    let defined: Vec<String> = doc.get("slots").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for s in &o.slots {
        if !defined.contains(s) {
            return Err(format!("slot \"{s}\" is not defined; slots: {}", if defined.is_empty() { "(none)".into() } else { defined.join(", ") }));
        }
    }
    if !(o.to > o.from) {
        return Err(format!("the range {}..{} is empty", o.from, o.to));
    }
    let mut starts: Vec<f64> = match &o.times {
        Some(ts) => {
            let mut v: Vec<f64> = ts.iter().copied().filter(|t| *t > o.from + 1e-9 && *t < o.to - 1e-9).collect();
            v.insert(0, o.from);
            v
        }
        None => {
            if !(o.every > 0.0) {
                return Err("--every must be a positive number of beats".into());
            }
            let step = o.every * tc.beat_len();
            let n = ((o.to - o.from) / step - 1e-9).ceil().max(1.0) as usize;
            (0..n).map(|k| o.from + k as f64 * step).collect()
        }
    };
    starts.sort_by(|a, b| a.total_cmp(b));
    starts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let min_len = tc.frame_len();
    starts.retain(|t| *t < o.to - min_len * 0.5);
    let mut doc = doc.clone();
    let layers = doc.get("layers").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut at = insert_index(&layers, &o.place)?;
    let used: Vec<String> = layers.iter().filter_map(|l| l.get("id").and_then(Value::as_str).map(str::to_string)).collect();
    let mut n = 1;
    let mut report = CutReport { layers: Vec::new(), extended_to: None };
    let mut new_layers = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(o.to);
        while used.contains(&format!("{}{n}", o.prefix)) {
            n += 1;
        }
        let id = format!("{}{n}", o.prefix);
        n += 1;
        let slot = o.slots[k % o.slots.len()].clone();
        new_layers.push(json!({ "id": id, "kind": "image", "slot": slot, "in": r(start), "out": r(end) }));
        report.layers.push((id, slot, r(start), r(end)));
    }
    let list = doc.get_mut("layers").and_then(Value::as_array_mut).ok_or("the project has no \"layers\"")?;
    for l in new_layers {
        list.insert(at, l);
        at += 1;
    }
    let duration = doc.pointer("/settings/duration").and_then(Value::as_f64).unwrap_or(0.0);
    if o.to > duration + 1e-9 {
        doc["settings"]["duration"] = json!(r(o.to));
        report.extended_to = Some(r(o.to));
    }
    Ok((doc, report))
}

/// What to accent or cut on.
pub const ON: &[&str] = &["beats", "offbeats", "downbeats", "bars", "kicks", "snares", "hats", "drop"];

/// The times (project time, ascending) of `on` within [from, to): `beats`,
/// `offbeats`, `downbeats` (= `bars`), `kicks`, `snares`, `hats`, `drop`, or a
/// comma-separated list of times (`"b4,b8,drop+1b"`). Onsets weaker than
/// `min_strength` (0..1) are skipped. `audio_offset` is the project's `audio.offset`.
pub fn times(on: &str, song: Option<&SongInfo>, tc: &TimeCtx, from: f64, to: f64, min_strength: f64, audio_offset: f64) -> Result<Vec<f64>, String> {
    let need = |what: &str| -> Result<&SongInfo, String> { song.ok_or_else(|| format!("--on {what} needs a song analysis: run `edits audio <song> -o song.json` and pass --audio song.json")) };
    let units = |s: f64| tc.seconds_to_units(s - audio_offset);
    let grid = |step: f64, phase: f64| -> Result<Vec<f64>, String> {
        let bl = tc.beat_len();
        if tc.bpm.is_none() && tc.time_base == crate::model::TimeBase::Seconds {
            return Err("this project has no tempo (settings.bpm); give --audio, or set the tempo with `edits audio <song> --project <project>`".into());
        }
        let first_beat = ((from / bl - phase) / step).ceil() * step + phase;
        let mut out = Vec::new();
        let mut b = first_beat;
        while b * bl < to - 1e-9 {
            out.push(tc.beat(b)?);
            b += step;
        }
        Ok(out)
    };
    let mut out: Vec<f64> = match on.trim() {
        "beats" | "beat" => match song {
            Some(s) => s.beats.iter().map(|t| units(*t)).collect(),
            None => grid(1.0, 0.0)?,
        },
        "offbeats" => match song {
            Some(s) => s.beats.iter().map(|t| units(*t + s.beat_seconds / 2.0)).collect(),
            None => grid(1.0, 0.5)?,
        },
        "downbeats" | "bars" | "bar" => match song {
            Some(s) => s.downbeats.iter().map(|t| units(*t)).collect(),
            None => grid(tc.beats_per_bar as f64, tc.downbeat)?,
        },
        "kicks" | "kick" => need("kicks")?.kick.iter().filter(|h| h.1 >= min_strength).map(|h| units(h.0)).collect(),
        "snares" | "snare" => need("snares")?.snare.iter().filter(|h| h.1 >= min_strength).map(|h| units(h.0)).collect(),
        "hats" | "hat" => need("hats")?.hat.iter().filter(|h| h.1 >= min_strength).map(|h| units(h.0)).collect(),
        "drop" => match song.and_then(|s| s.drop) {
            Some(d) => vec![units(d)],
            None => vec![tc.resolve("drop").map_err(|_| "no drop: the song analysis found none and the project has no \"drop\" marker; give a time instead (--on b16)".to_string())?],
        },
        list => list.split(',').map(|t| tc.resolve(t)).collect::<Result<Vec<_>, _>>().map_err(|e| format!("--on is one of {} or a list of times: {e}", ON.join(", ")))?,
    };
    out.retain(|t| *t >= from - 1e-9 && *t < to - 1e-9);
    out.sort_by(|a, b| a.total_cmp(b));
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    Ok(out.into_iter().map(r).collect())
}

/// Write a song's tempo, beat offset and markers into a project document.
/// The project's time base is kept: a beats project keeps its beat numbers
/// (beat 0 moves to the song's beat 0), a seconds project keeps its seconds.
/// When the project has no audio, `audio_path` (relative to the project) is set.
pub fn sync(doc: &Value, song: &SongInfo, audio_path: Option<&str>) -> Result<(Value, Vec<String>), String> {
    let mut doc = doc.clone();
    let mut said = Vec::new();
    if doc.get("audio").is_none() {
        if let Some(p) = audio_path {
            doc["audio"] = json!({ "path": p, "offset": 0.0 });
            said.push(format!("audio: {p}"));
        }
    }
    let offset = doc.pointer("/audio/offset").and_then(Value::as_f64).unwrap_or(0.0);
    doc["settings"]["bpm"] = json!(song.bpm);
    // Beat 0 of the project moves to the song's beat nearest to where it was,
    // so what is placed in beats stays where it was, now on the song's grid.
    let was = doc.pointer("/settings/beat_offset").and_then(Value::as_f64).unwrap_or(0.0);
    let first = song.beat_offset - offset;
    let beat0 = first + ((was - first) / song.beat_seconds).round() * song.beat_seconds;
    doc["settings"]["beat_offset"] = json!(r(beat0));
    said.push(format!("tempo {} BPM, beat 0 at {:.3} s", song.bpm, beat0));
    let tc = TimeCtx::from_doc(&doc);
    let mut markers: Vec<Value> = doc.get("markers").and_then(Value::as_array).cloned().unwrap_or_default();
    for (label, t) in &song.markers {
        let u = r(tc.seconds_to_units(t - offset));
        if u < 0.0 {
            continue;
        }
        markers.retain(|m| m.get("label").and_then(Value::as_str) != Some(label.as_str()));
        markers.push(json!({ "t": u, "label": label }));
        said.push(format!("marker {label} at {u}"));
    }
    markers.sort_by(|a, b| a["t"].as_f64().unwrap_or(0.0).total_cmp(&b["t"].as_f64().unwrap_or(0.0)));
    if !markers.is_empty() {
        doc["markers"] = json!(markers);
    }
    Ok((doc, said))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Value {
        json!({ "format": "edits.project/1",
            "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 120 },
            "slots": { "a": {}, "b": {}, "c": {} },
            "layers": [ { "id": "bg", "kind": "solid", "color": [0, 0, 0, 1] }, { "id": "grade", "kind": "adjustment" } ] })
    }

    #[test]
    fn cuts_cycle_slots_below_the_grade_and_extend_the_project() {
        let d = doc();
        let tc = TimeCtx::from_doc(&d);
        let o = CutOpts { slots: vec!["a".into(), "b".into(), "c".into()], times: None, every: 2.0, from: 0.0, to: 12.0, prefix: "cut".into(), place: Place::Default };
        let (out, rep) = cut(&d, &o, &tc).unwrap();
        assert_eq!(rep.layers.len(), 6);
        assert_eq!(rep.layers[3], ("cut4".to_string(), "a".to_string(), 6.0, 8.0));
        assert_eq!(rep.extended_to, Some(12.0));
        let ids: Vec<&str> = out["layers"].as_array().unwrap().iter().map(|l| l["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["bg", "cut1", "cut2", "cut3", "cut4", "cut5", "cut6", "grade"]);
        let project: crate::Project = serde_json::from_value(out.clone()).unwrap();
        assert!(!crate::check::has_errors(&crate::check::check(&project, None)));
        // Ids continue where the last cut left off.
        let (again, rep) = cut(&out, &o, &tc).unwrap();
        assert_eq!(rep.layers[0].0, "cut7");
        assert!(cut(&again, &CutOpts { slots: vec!["zz".into()], ..o.clone() }, &tc).unwrap_err().contains("slots: a, b, c"));
    }

    #[test]
    fn times_of_beats_bars_and_onsets() {
        let d = doc();
        let tc = TimeCtx::from_doc(&d);
        assert_eq!(times("beats", None, &tc, 0.0, 4.0, 0.0, 0.0).unwrap(), vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(times("offbeats", None, &tc, 0.0, 2.0, 0.0, 0.0).unwrap(), vec![0.5, 1.5]);
        assert_eq!(times("downbeats", None, &tc, 1.0, 16.0, 0.0, 0.0).unwrap(), vec![4.0, 8.0, 12.0]);
        assert_eq!(times("b1, bar 2", None, &tc, 0.0, 8.0, 0.0, 0.0).unwrap(), vec![1.0, 4.0]);
        assert!(times("kicks", None, &tc, 0.0, 8.0, 0.0, 0.0).unwrap_err().contains("edits audio"));
        // A song: 120 BPM, beat 0 at 0.25 s; the project starts 0.25 s into it.
        let song = SongInfo { bpm: 120.0, beat_offset: 0.25, beat_seconds: 0.5, beats_per_bar: 4, kick: vec![(0.25, 1.0), (0.75, 0.2), (1.25, 0.9)], drop: Some(2.25), ..Default::default() };
        assert_eq!(times("kicks", Some(&song), &tc, 0.0, 8.0, 0.5, 0.25).unwrap(), vec![0.0, 2.0]);
        assert_eq!(times("drop", Some(&song), &tc, 0.0, 8.0, 0.0, 0.25).unwrap(), vec![4.0]);
    }

    #[test]
    fn sync_sets_tempo_offset_and_markers() {
        let d = doc();
        let song = SongInfo { bpm: 128.0, beat_offset: 0.1, beat_seconds: 60.0 / 128.0, beats_per_bar: 4, downbeat: 1, markers: vec![("drop".into(), 0.1 + 33.0 * 60.0 / 128.0)], ..Default::default() };
        let (out, said) = sync(&d, &song, Some("song.mp3")).unwrap();
        assert_eq!(out["settings"]["bpm"], json!(128.0));
        assert_eq!(out["audio"]["path"], "song.mp3");
        // Beat 0 stays near 0 s: on the song's beat 0 at 0.1 s.
        let b0 = out["settings"]["beat_offset"].as_f64().unwrap();
        assert!((b0 - 0.1).abs() < 1e-6, "{b0}");
        assert_eq!(out["markers"][0]["label"], "drop");
        assert!((out["markers"][0]["t"].as_f64().unwrap() - 33.0).abs() < 1e-6);
        assert!(said.iter().any(|s| s.contains("128 BPM")));
    }
}
