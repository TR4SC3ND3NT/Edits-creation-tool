//! Times written the way an editor thinks of them.
//!
//! Wherever a command or an edit takes a time (`--at`, `--from`, `--to`, a
//! layer's `in` / `out`, a key's `t`), it accepts a number in the project's
//! time base or a string:
//!
//! | Text | Means |
//! | --- | --- |
//! | `12.5` | project time (beats in a beats project, else seconds) |
//! | `b12.5`, `12.5b` | beat 12.5 (beat 0 is at `settings.beat_offset`) |
//! | `bar 4`, `bar4`, `4:1` | the first beat of bar 4 (bars are 1-based) |
//! | `4:3`, `bar 4:3` | bar 4, beat 3 (beats in a bar are 1-based) |
//! | `2.5s` | 2.5 seconds from the start of the project |
//! | `f60`, `60f` | the start of frame 60 |
//! | `drop`, `intro`, any marker | a marker of the project, or of a song analysis given to the command |
//! | `start`, `end` | 0 and the end of the project |
//! | `drop+2b`, `bar 3-0.5b`, `drop-1bar`, `b4+3f` | any of these plus or minus offsets in `b`, `bar`, `s` or `f` (no unit: project time) |
//!
//! Bars have `beats_per_bar` beats (4 unless a song analysis says
//! otherwise) and bar 1 starts at beat 0, or at the analysis' first downbeat.

use crate::model::{Project, TimeBase};
use crate::song::SongInfo;
use serde_json::Value;

/// What a time is resolved against.
#[derive(Clone, Debug)]
pub struct TimeCtx {
    pub time_base: TimeBase,
    pub bpm: Option<f64>,
    /// Seconds from the start of the project to beat 0.
    pub beat_offset: f64,
    pub fps: f64,
    /// Project length in project time.
    pub duration: f64,
    pub beats_per_bar: u32,
    /// Project beat number at which bar 1 starts.
    pub downbeat: f64,
    /// (label, project time).
    pub markers: Vec<(String, f64)>,
}

impl TimeCtx {
    pub fn from_project(p: &Project) -> Self {
        TimeCtx {
            time_base: p.settings.time_base,
            bpm: p.settings.bpm.filter(|b| *b > 0.0),
            beat_offset: p.settings.beat_offset,
            fps: p.settings.fps,
            duration: p.settings.duration,
            beats_per_bar: 4,
            downbeat: 0.0,
            markers: p.markers.iter().map(|m| (m.label.clone(), m.t)).collect(),
        }
    }

    /// From a raw project document, which may not parse as a project yet
    /// (its times may still be strings).
    pub fn from_doc(doc: &Value) -> Self {
        let s = |k: &str| doc.pointer(&format!("/settings/{k}")).and_then(Value::as_f64);
        let markers = doc
            .get("markers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| Some((m.get("label")?.as_str()?.to_string(), m.get("t")?.as_f64()?)))
            .collect();
        TimeCtx {
            time_base: if doc.pointer("/settings/time_base").and_then(Value::as_str) == Some("beats") { TimeBase::Beats } else { TimeBase::Seconds },
            bpm: s("bpm").filter(|b| *b > 0.0),
            beat_offset: s("beat_offset").unwrap_or(0.0),
            fps: s("fps").filter(|f| *f > 0.0).unwrap_or(25.0),
            duration: s("duration").unwrap_or(0.0),
            beats_per_bar: 4,
            downbeat: 0.0,
            markers,
        }
    }

    /// Add a song analysis: its markers and bars. `audio_offset` is the
    /// project's `audio.offset` (seconds into the file where the project starts).
    pub fn with_song(mut self, song: &SongInfo, audio_offset: f64) -> Self {
        self.beats_per_bar = song.beats_per_bar.max(1);
        if self.bpm.is_some() {
            let first = song.beat_offset + song.downbeat as f64 * song.beat_seconds - audio_offset;
            let b = self.seconds_to_beats(first);
            // Bar 1 at the first downbeat at or after beat 0 of the project.
            let bar = self.beats_per_bar as f64;
            self.downbeat = b.rem_euclid(bar);
            if (self.downbeat - bar).abs() < 1e-6 {
                self.downbeat = 0.0;
            }
        }
        for (label, t) in &song.markers {
            let u = self.seconds_to_units(t - audio_offset);
            self.markers.retain(|(l, _)| !l.eq_ignore_ascii_case(label));
            self.markers.push((label.clone(), u));
        }
        self
    }

    fn need_bpm(&self, what: &str) -> Result<f64, String> {
        self.bpm.ok_or_else(|| format!("{what} needs a tempo, and the project has no settings.bpm (set it, or run `edits audio <song> --project <project>`)"))
    }

    /// Seconds from the start of the project to project time.
    pub fn seconds_to_units(&self, s: f64) -> f64 {
        match (self.time_base, self.bpm) {
            (TimeBase::Beats, Some(bpm)) => (s - self.beat_offset) * bpm / 60.0,
            _ => s,
        }
    }

    pub fn units_to_seconds(&self, u: f64) -> f64 {
        match (self.time_base, self.bpm) {
            (TimeBase::Beats, Some(bpm)) => self.beat_offset + u * 60.0 / bpm,
            _ => u,
        }
    }

    fn seconds_to_beats(&self, s: f64) -> f64 {
        self.bpm.map(|bpm| (s - self.beat_offset) * bpm / 60.0).unwrap_or(s)
    }

    /// Beat number to project time.
    pub fn beat(&self, b: f64) -> Result<f64, String> {
        match self.time_base {
            TimeBase::Beats => Ok(b),
            TimeBase::Seconds => Ok(self.beat_offset + b * 60.0 / self.need_bpm("a time in beats")?),
        }
    }

    /// Length of one beat in project time (1 in a beats project).
    pub fn beat_len(&self) -> f64 {
        match (self.time_base, self.bpm) {
            (TimeBase::Beats, _) => 1.0,
            (TimeBase::Seconds, Some(bpm)) => 60.0 / bpm,
            (TimeBase::Seconds, None) => 0.5,
        }
    }

    /// Length of one output frame in project time.
    pub fn frame_len(&self) -> f64 {
        self.seconds_to_units(1.0 / self.fps) - self.seconds_to_units(0.0)
    }

    /// Bar `bar` (1-based), beat `beat` (1-based) to project time.
    pub fn bar(&self, bar: f64, beat: f64) -> Result<f64, String> {
        self.need_bpm("a time in bars")?;
        self.beat(self.downbeat + (bar - 1.0) * self.beats_per_bar as f64 + (beat - 1.0))
    }

    fn marker(&self, name: &str) -> Result<f64, String> {
        if let Some((_, t)) = self.markers.iter().find(|(l, _)| l == name).or_else(|| self.markers.iter().find(|(l, _)| l.eq_ignore_ascii_case(name))) {
            return Ok(*t);
        }
        let known: Vec<&str> = self.markers.iter().map(|(l, _)| l.as_str()).collect();
        Err(if known.is_empty() {
            format!("\"{name}\" is not a time or a marker; the project has no markers (a song analysis adds `drop` and its sections: give it with --audio, or write them into the project with `edits audio <song> --project <project>`)")
        } else {
            format!("\"{name}\" is not a time or a marker; markers: {}", known.join(", "))
        })
    }

    /// Resolve a time to project time.
    pub fn resolve(&self, text: &str) -> Result<f64, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err("empty time".into());
        }
        let (base, offsets) = split(t);
        let mut v = self.base(base.trim()).map_err(|e| format!("cannot read time \"{text}\": {e}"))?;
        for (sign, off) in offsets {
            v += sign * self.length(off.trim()).map_err(|e| format!("cannot read time \"{text}\": {e}"))?;
        }
        if !v.is_finite() {
            return Err(format!("time \"{text}\" is not a finite number"));
        }
        Ok(v)
    }

    /// Resolve a JSON value (number or string) to project time.
    pub fn resolve_value(&self, v: &Value) -> Result<f64, String> {
        match v {
            Value::Number(n) => n.as_f64().ok_or_else(|| "not a number".into()),
            Value::String(s) => self.resolve(s),
            other => Err(format!("a time must be a number or a string such as \"b4\" or \"drop\", not {other}")),
        }
    }

    /// A length such as `2b`, `1bar`, `0.5s`, `3f` or `0.25` (project time).
    pub fn length(&self, text: &str) -> Result<f64, String> {
        let t = text.trim().to_ascii_lowercase();
        let (n, unit) = number_prefix(&t).ok_or_else(|| format!("\"{text}\" is not a length (a number with an optional unit b, bar, s or f)"))?;
        let unit = unit.trim();
        Ok(match unit {
            "" => n,
            "b" | "beat" | "beats" => n * self.beat_len_checked()?,
            "bar" | "bars" => n * self.beats_per_bar as f64 * self.beat_len_checked()?,
            "s" | "sec" | "secs" => self.seconds_to_units(n) - self.seconds_to_units(0.0),
            "f" | "frame" | "frames" => n * self.frame_len(),
            other => return Err(format!("unknown unit \"{other}\" in \"{text}\" (use b, bar, s or f)")),
        })
    }

    fn beat_len_checked(&self) -> Result<f64, String> {
        if self.time_base == TimeBase::Seconds {
            self.need_bpm("a length in beats")?;
        }
        Ok(self.beat_len())
    }

    fn base(&self, t: &str) -> Result<f64, String> {
        let lower = t.to_ascii_lowercase();
        match lower.as_str() {
            "start" => return Ok(0.0),
            "end" => return Ok(self.duration),
            _ => {}
        }
        // bar N, bar N:M, N:M
        let bar_text = lower.strip_prefix("bar").map(str::trim);
        if let Some(rest) = bar_text.or_else(|| lower.contains(':').then_some(lower.as_str())) {
            let (bar, beat) = match rest.split_once(':') {
                Some((a, b)) => (a.trim(), b.trim()),
                None => (rest, "1"),
            };
            let bar: f64 = bar.parse().map_err(|_| format!("\"{t}\": the bar number is not a number"))?;
            let beat: f64 = beat.parse().map_err(|_| format!("\"{t}\": the beat in the bar is not a number"))?;
            return self.bar(bar, beat);
        }
        if let Some(rest) = lower.strip_prefix('b') {
            if let Ok(b) = rest.trim().parse::<f64>() {
                return self.beat(b);
            }
        }
        if let Some(rest) = lower.strip_prefix('f') {
            if let Ok(f) = rest.trim().parse::<f64>() {
                return Ok(self.seconds_to_units(f / self.fps));
            }
        }
        if let Some((n, unit)) = number_prefix(&lower) {
            return match unit.trim() {
                "" => Ok(n),
                "b" | "beat" => self.beat(n),
                "s" | "sec" => Ok(self.seconds_to_units(n)),
                "f" | "frame" => Ok(self.seconds_to_units(n / self.fps)),
                "bar" => self.bar(n, 1.0),
                other => Err(format!("unknown unit \"{other}\" (use b, s, f or bar)")),
            };
        }
        self.marker(t)
    }
}

/// A leading number and the rest.
fn number_prefix(t: &str) -> Option<(f64, &str)> {
    let end = t.char_indices().take_while(|(i, c)| c.is_ascii_digit() || *c == '.' || (*i == 0 && (*c == '-' || *c == '+'))).map(|(i, c)| i + c.len_utf8()).last()?;
    let n: f64 = t[..end].parse().ok()?;
    Some((n, &t[end..]))
}

/// "drop+2b-1f" -> ("drop", [(+1, "2b"), (-1, "1f")]).
fn split(t: &str) -> (&str, Vec<(f64, &str)>) {
    let mut cuts: Vec<usize> = t.char_indices().filter(|(i, c)| *i > 0 && (*c == '+' || *c == '-')).map(|(i, _)| i).collect();
    // A sign right after "bar" or ":" belongs to the number, not an offset ("bar -1" is unusual but legal).
    cuts.retain(|&i| {
        let before = t[..i].trim_end();
        !(before.ends_with("bar") || before.ends_with(':'))
    });
    if cuts.is_empty() {
        return (t, Vec::new());
    }
    let base = &t[..cuts[0]];
    let mut out = Vec::new();
    for (k, &i) in cuts.iter().enumerate() {
        let end = cuts.get(k + 1).copied().unwrap_or(t.len());
        let sign = if t[i..].starts_with('-') { -1.0 } else { 1.0 };
        out.push((sign, &t[i + 1..end]));
    }
    (base, out)
}

/// Replace every time written as a string in a project document (layer
/// `in` / `out`, key `t`, marker `t`, transition `duration`) by its number.
/// Errors name the place as a JSON pointer.
pub fn resolve_doc(doc: &mut Value, extra: Option<&TimeCtx>) -> Result<(), String> {
    let mut ctx = match extra {
        Some(c) => c.clone(),
        None => TimeCtx::from_doc(doc),
    };
    let mut errors = Vec::new();
    if let Some(markers) = doc.get_mut("markers").and_then(Value::as_array_mut) {
        for (i, m) in markers.iter_mut().enumerate() {
            fix(m, "t", &format!("/markers/{i}/t"), &ctx, false, &mut errors);
        }
    }
    // Markers just resolved can be used by the layers.
    for (label, t) in TimeCtx::from_doc(doc).markers {
        if !ctx.markers.iter().any(|(l, _)| *l == label) {
            ctx.markers.push((label, t));
        }
    }
    if let Some(layers) = doc.get_mut("layers").and_then(Value::as_array_mut) {
        for (i, layer) in layers.iter_mut().enumerate() {
            let at = match layer.get("id").and_then(Value::as_str) {
                Some(id) => format!("/layers/@{}", crate::pointer::escape(id)),
                None => format!("/layers/{i}"),
            };
            fix(layer, "in", &format!("{at}/in"), &ctx, false, &mut errors);
            fix(layer, "out", &format!("{at}/out"), &ctx, false, &mut errors);
            if let Some(tr) = layer.get_mut("transition") {
                fix(tr, "duration", &format!("{at}/transition/duration"), &ctx, true, &mut errors);
            }
            keys(layer, &at, &ctx, &mut errors);
        }
    }
    if let Some(camera) = doc.get_mut("camera") {
        keys(camera, "/camera", &ctx, &mut errors);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn fix(obj: &mut Value, key: &str, at: &str, ctx: &TimeCtx, length: bool, errors: &mut Vec<String>) {
    if let Some(Value::String(s)) = obj.get(key) {
        let r = if length { ctx.length(s) } else { ctx.resolve(s) };
        match r {
            Ok(v) => obj[key] = Value::from(round(v)),
            Err(e) => errors.push(format!("{at}: {e}")),
        }
    }
}

/// Times are kept to 1e-6 so that "b1/3" style values stay readable.
fn round(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// Every `"keys": [ { "t": ... } ]` below `v`.
fn keys(v: &mut Value, at: &str, ctx: &TimeCtx, errors: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map.iter_mut() {
                let here = format!("{at}/{}", crate::pointer::escape(k));
                if k == "keys" {
                    if let Value::Array(list) = child {
                        for (i, key) in list.iter_mut().enumerate() {
                            fix(key, "t", &format!("{here}/{i}/t"), ctx, false, errors);
                        }
                    }
                }
                keys(child, &here, ctx, errors);
            }
        }
        Value::Array(list) => {
            for (i, child) in list.iter_mut().enumerate() {
                keys(child, &format!("{at}/{i}"), ctx, errors);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn beats() -> TimeCtx {
        TimeCtx { time_base: TimeBase::Beats, bpm: Some(120.0), beat_offset: 0.5, fps: 25.0, duration: 32.0, beats_per_bar: 4, downbeat: 0.0, markers: vec![("drop".into(), 16.0)] }
    }

    #[test]
    fn beats_bars_seconds_frames_markers() {
        let c = beats();
        assert_eq!(c.resolve("12.5").unwrap(), 12.5);
        assert_eq!(c.resolve("b12.5").unwrap(), 12.5);
        assert_eq!(c.resolve("12.5b").unwrap(), 12.5);
        assert_eq!(c.resolve("bar 3").unwrap(), 8.0);
        assert_eq!(c.resolve("bar3").unwrap(), 8.0);
        assert_eq!(c.resolve("3:2").unwrap(), 9.0);
        assert_eq!(c.resolve("bar 3:4").unwrap(), 11.0);
        // 2.5 s is 2 s after beat 0 at 120 BPM: beat 4.
        assert!((c.resolve("2.5s").unwrap() - 4.0).abs() < 1e-9);
        assert!((c.resolve("f25").unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(c.resolve("drop").unwrap(), 16.0);
        assert_eq!(c.resolve("DROP").unwrap(), 16.0);
        assert_eq!(c.resolve("drop+2b").unwrap(), 18.0);
        assert_eq!(c.resolve("drop-1bar").unwrap(), 12.0);
        assert_eq!(c.resolve("drop - 0.5").unwrap(), 15.5);
        assert!((c.resolve("b4+5f").unwrap() - 4.2).abs() < 1e-9);
        assert_eq!(c.resolve("end").unwrap(), 32.0);
        let e = c.resolve("dorp").unwrap_err();
        assert!(e.contains("markers: drop"), "{e}");
    }

    #[test]
    fn seconds_projects_need_a_tempo_for_beats() {
        let mut c = beats();
        c.time_base = TimeBase::Seconds;
        assert!((c.resolve("b2").unwrap() - 1.5).abs() < 1e-9);
        assert!((c.resolve("1.25").unwrap() - 1.25).abs() < 1e-9);
        c.bpm = None;
        assert!(c.resolve("b2").unwrap_err().contains("settings.bpm"));
        assert_eq!(c.resolve("2s").unwrap(), 2.0);
    }

    #[test]
    fn a_song_adds_markers_and_moves_bar_one() {
        let song = SongInfo { bpm: 120.0, beat_offset: 0.25, beat_seconds: 0.5, beats_per_bar: 4, downbeat: 1, markers: vec![("drop".into(), 8.75)], ..Default::default() };
        // Project starts 0.25 s into the file, beat 0 at project 0 s.
        let c = TimeCtx { beat_offset: 0.0, markers: vec![], ..beats() }.with_song(&song, 0.25);
        assert_eq!(c.downbeat, 1.0);
        assert_eq!(c.resolve("bar 1").unwrap(), 1.0);
        assert_eq!(c.resolve("bar 2").unwrap(), 5.0);
        assert_eq!(c.resolve("drop").unwrap(), 17.0);
    }

    #[test]
    fn documents_get_numbers() {
        let mut doc = json!({ "settings": { "time_base": "beats", "bpm": 120, "fps": 25, "duration": 16 },
            "markers": [ { "t": "bar 2", "label": "hit" } ],
            "layers": [ { "id": "a", "in": "b2", "out": "hit+1b",
                          "transform": { "scale": { "keys": [ { "t": "bar 1", "v": 1 }, { "t": "b0.5", "v": 2 } ] } } } ] });
        resolve_doc(&mut doc, None).unwrap();
        assert_eq!(doc["markers"][0]["t"], json!(4.0));
        assert_eq!(doc["layers"][0]["in"], json!(2.0));
        assert_eq!(doc["layers"][0]["transform"]["scale"]["keys"][1]["t"], json!(0.5));
        let mut bad = json!({ "settings": { "time_base": "beats", "bpm": 120 }, "layers": [ { "id": "a", "in": "nope" } ] });
        let e = resolve_doc(&mut bad, None).unwrap_err();
        assert!(e.starts_with("/layers/@a/in:"), "{e}");
    }
}
