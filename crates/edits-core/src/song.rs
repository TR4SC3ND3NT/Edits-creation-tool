//! Reading a song analysis (`edits.audio/1`, written by `edits audio`).
//!
//! The analysis is produced by `edits-analyze`; this is the small part of it
//! that the edit operations need (beats, bars, onsets, the drop, markers),
//! read loosely from JSON so the core has no dependency on the analyser.
//! Every time here is in seconds of the analysed file.

use serde_json::Value;
use std::path::Path;

pub const FORMAT: &str = "edits.audio/1";

#[derive(Clone, Debug, Default)]
pub struct SongInfo {
    pub bpm: f64,
    /// Seconds from the start of the file to beat 0.
    pub beat_offset: f64,
    pub beat_seconds: f64,
    pub beats_per_bar: u32,
    /// Beat number of bar 1's first beat.
    pub downbeat: u32,
    pub duration: f64,
    /// Beat times.
    pub beats: Vec<f64>,
    /// Bar start times.
    pub downbeats: Vec<f64>,
    /// (time, strength 0..1) per band.
    pub kick: Vec<(f64, f64)>,
    pub snare: Vec<(f64, f64)>,
    pub hat: Vec<(f64, f64)>,
    pub drop: Option<f64>,
    /// Named times (drop, section starts).
    pub markers: Vec<(String, f64)>,
}

fn num(v: &Value, ptr: &str) -> Option<f64> {
    v.pointer(ptr).and_then(Value::as_f64)
}

fn hits(v: &Value, ptr: &str) -> Vec<(f64, f64)> {
    v.pointer(ptr)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|h| Some((h.get("t")?.as_f64()?, h.get("s").and_then(Value::as_f64).unwrap_or(1.0))))
        .collect()
}

impl SongInfo {
    pub fn from_value(v: &Value) -> Result<Self, String> {
        if v.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(format!("not a song analysis (\"format\" is not \"{FORMAT}\"); make one with `edits audio <song> -o song.json`"));
        }
        let bpm = num(v, "/tempo/bpm").filter(|b| *b > 0.0).ok_or("the song analysis has no tempo")?;
        let times = |ptr: &str| -> Vec<f64> { v.pointer(ptr).and_then(Value::as_array).into_iter().flatten().filter_map(|b| b.get("t").and_then(Value::as_f64)).collect() };
        let markers = v
            .get("markers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| Some((m.get("label")?.as_str()?.to_string(), m.get("t")?.as_f64()?)))
            .collect();
        Ok(SongInfo {
            bpm,
            beat_offset: num(v, "/grid/beat_offset").unwrap_or(0.0),
            beat_seconds: num(v, "/grid/beat_seconds").unwrap_or(60.0 / bpm),
            beats_per_bar: num(v, "/grid/beats_per_bar").map(|b| b as u32).filter(|b| *b > 0).unwrap_or(4),
            downbeat: num(v, "/grid/downbeat").map(|b| b as u32).unwrap_or(0),
            duration: num(v, "/duration").unwrap_or(0.0),
            beats: times("/beats"),
            downbeats: times("/downbeats"),
            kick: hits(v, "/onsets/kick"),
            snare: hits(v, "/onsets/snare"),
            hat: hits(v, "/onsets/hat"),
            drop: num(v, "/drop/t"),
            markers,
        })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read the song analysis {}: {e}", path.display()))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?;
        Self::from_value(&v).map_err(|e| format!("{}: {e}", path.display()))
    }
}
