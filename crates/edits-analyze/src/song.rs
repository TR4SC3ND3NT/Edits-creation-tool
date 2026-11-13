//! Song analysis for cutting pictures to music: tempo and the beat grid,
//! bars and downbeats, onsets in three frequency bands (kick, snare, hats),
//! loudness per beat, sections and the drop.
//!
//! Everything is computed from one short-time Fourier transform of the mono
//! signal (see [`beats::band_flux`]) with plain signal processing written
//! here: no model, no GPL code. The tempo comes from the same estimator the
//! reference analysis uses ([`beats::tempo`]); the grid is regular (one tempo
//! for the whole file), which is what an edit is cut to.
//!
//! The band onsets are what each band's spectral flux says, not a drum
//! transcription: a snare that coincides with a kick shows up in both the low
//! and the mid band, and a bass line also lands in the low band.

use crate::beats::{self, Onset, TempoOpts};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "edits.audio/1";
/// Sample rate the audio is decoded at.
pub const RATE: u32 = 22050;
/// The low (kick), mid (snare, claps) and high (hats, cymbals) bands, in Hz.
pub const BANDS: [(f32, f32); 3] = [(30.0, 150.0), (200.0, 3500.0), (6000.0, 11000.0)];

#[derive(Clone, Debug)]
pub struct SongOpts {
    /// Use this tempo instead of estimating it.
    pub bpm: Option<f64>,
    pub beats_per_bar: u32,
}
impl Default for SongOpts {
    fn default() -> Self {
        Self { bpm: None, beats_per_bar: 4 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Song {
    pub format: String,
    /// The analysed file, as given.
    pub source: String,
    /// Seconds.
    pub duration: f64,
    pub tempo: TempoInfo,
    pub grid: Grid,
    /// Every beat of the grid inside the file.
    pub beats: Vec<Beat>,
    /// The first beat of every bar.
    pub downbeats: Vec<Downbeat>,
    pub onsets: Onsets,
    /// Loudness of each beat (index = beat number), 0 (the quietest part) to 1 (the loudest).
    pub energy: Vec<f64>,
    pub sections: Vec<Section>,
    /// The largest rise in energy, on a downbeat; `None` when nothing rises clearly.
    pub drop: Option<Drop>,
    pub loudness: Loudness,
    /// Named times: `drop` and the start of each section, usable as times
    /// (`--at drop`) by the `edits` commands that take this file.
    pub markers: Vec<Marker>,
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TempoInfo {
    pub bpm: f64,
    /// 0..1: how clearly the onsets repeat at this tempo.
    pub confidence: f64,
    /// Other tempi the autocorrelation liked, best first.
    pub candidates: Vec<f64>,
    /// Share of the kick and snare onsets within 50 ms of a beat or half beat.
    pub on_grid: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Grid {
    /// Seconds from the start of the file to beat 0 (the first beat).
    pub beat_offset: f64,
    /// Seconds per beat.
    pub beat_seconds: f64,
    pub beats_per_bar: u32,
    /// Beat number of the first downbeat (bar 1, beat 1).
    pub downbeat: u32,
    /// 0..1: how much better that bar phase explains the kicks and snares than the next best.
    pub downbeat_confidence: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Beat {
    /// Beat number from beat 0.
    pub b: i64,
    /// Seconds.
    pub t: f64,
    /// Bar number (bar 1 starts at the first downbeat; 0 is a pickup).
    pub bar: i64,
    /// Position in the bar, 1-based.
    pub beat: u32,
    /// 0..1: the onset strength at this beat.
    pub s: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Downbeat {
    pub bar: i64,
    pub t: f64,
    pub b: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Onsets {
    /// Low band (30–150 Hz): kicks, bass hits.
    pub kick: Vec<Hit>,
    /// Mid band (200–3500 Hz): snares, claps, most melodic attacks.
    pub snare: Vec<Hit>,
    /// High band (6–11 kHz): hats, cymbals, noise sweeps.
    pub hat: Vec<Hit>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Hit {
    /// Seconds.
    pub t: f64,
    /// Beats from beat 0 (fractional).
    pub b: f64,
    /// 0..1, relative to the strongest onset of the band.
    pub s: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Section {
    /// `intro`, `build`, `drop`, `outro`, or the energy level `low`, `mid`, `high`
    /// (numbered when repeated: `high2`).
    pub label: String,
    /// Seconds.
    pub from: f64,
    pub to: f64,
    pub from_beat: f64,
    pub to_beat: f64,
    /// First bar of the section.
    pub bar: i64,
    pub bars: i64,
    /// Mean energy, 0..1.
    pub energy: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Drop {
    pub t: f64,
    pub b: f64,
    pub bar: i64,
    /// Energy after minus energy before (0..1 scale).
    pub rise: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Loudness {
    /// Root mean square of the whole file, dB relative to full scale (not LUFS).
    pub rms_dbfs: f64,
    pub peak_dbfs: f64,
    /// Quietest and loudest beat, dBFS.
    pub beat_min_dbfs: f64,
    pub beat_max_dbfs: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Marker {
    pub label: String,
    pub t: f64,
    pub b: f64,
}

fn r(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (v * k).round() / k
}

fn db(v: f64) -> f64 {
    20.0 * v.max(1e-9).log10()
}

fn percentile(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    s[((s.len() - 1) as f64 * q).round() as usize]
}

/// Peaks of a normalised envelope: local maxima over ±30 ms that stand
/// `threshold` standard deviations above the local mean, at least `gap` s apart.
fn pick(env: &[f32], rate: f64, t0: f64, gap: f64, threshold: f32) -> Vec<(f64, f32)> {
    let n = env.len();
    let w_max = ((0.03 * rate).round() as usize).max(1);
    let w_mean = ((0.1 * rate).round() as usize).max(2);
    let mut out: Vec<(f64, f32)> = Vec::new();
    for i in 0..n {
        let v = env[i];
        if v < threshold {
            continue;
        }
        let (a, b) = (i.saturating_sub(w_max), (i + w_max + 1).min(n));
        if env[a..b].iter().any(|&u| u > v) {
            continue;
        }
        let (a, b) = (i.saturating_sub(w_mean), (i + w_mean + 1).min(n));
        let mean = env[a..b].iter().sum::<f32>() / (b - a) as f32;
        if v < mean + 0.5 * threshold {
            continue;
        }
        let t = t0 + i as f64 / rate;
        match out.last_mut() {
            Some(last) if t - last.0 < gap => {
                if v > last.1 {
                    *last = (t, v);
                }
            }
            _ => out.push((t, v)),
        }
    }
    out
}

/// Largest envelope value within ±`radius` seconds of `t`.
fn near(env: &[f32], rate: f64, t0: f64, t: f64, radius: f64) -> f64 {
    let c = (t - t0) * rate;
    let a = (c - radius * rate).floor().max(0.0) as usize;
    let b = ((c + radius * rate).ceil().max(0.0) as usize + 1).min(env.len());
    env.get(a..b).map(|s| s.iter().fold(0.0f32, |m, &v| m.max(v)) as f64).unwrap_or(0.0)
}

/// Analyse mono samples at `sr` Hz. `source` is recorded as given.
pub fn analyze_samples(samples: &[f32], sr: u32, source: &str, opts: &SongOpts) -> anyhow::Result<Song> {
    let duration = samples.len() as f64 / sr as f64;
    if duration < 2.0 {
        anyhow::bail!("{source} is {duration:.2} s long; a song analysis needs at least 2 s of audio");
    }
    let bpb = opts.beats_per_bar.clamp(1, 16);
    // Silence in front, so that a hit on the very first sample is an onset too.
    let pad = 1024usize;
    let mut padded = vec![0.0f32; pad];
    padded.extend_from_slice(samples);
    let flux = beats::band_flux(&padded, sr, &BANDS);
    let rate = flux.rate;
    let t0 = flux.t0 - pad as f64 / sr as f64;
    let full = beats::normalise_envelope(&flux.full, rate);
    let bands: Vec<Vec<f32>> = flux.bands.iter().map(|b| beats::normalise_envelope(b, rate)).collect();
    // The tempo is read from the bands added with equal weight, so that busy
    // hats do not outvote the kick and snare (the full-band flux alone often
    // locks onto 2/3 or 3/2 of the tempo).
    let mixed: Vec<f32> = (0..full.len()).map(|i| bands[0][i] + bands[1][i] + 0.5 * bands[2][i]).collect();
    let onset = Onset { rate, t0, env: beats::normalise_envelope(&mixed, rate) };
    let tempo = beats::tempo(&onset, &TempoOpts { bpm: opts.bpm, ..Default::default() })
        .ok_or_else(|| anyhow::anyhow!("no tempo found in {source}: the audio has no repeating onsets (is it silent?)"))?;
    let period = 60.0 / tempo.bpm;
    // The estimator gives the phase in [0, period): a beat a few milliseconds
    // before the start of the file (the first hit of a loop) would otherwise
    // become the second beat.
    let offset = if tempo.phase > period - 0.035 { tempo.phase - period } else { tempo.phase };
    let beat_time = |b: f64| offset + b * period;
    let beat_of = |t: f64| (t - offset) / period;
    let n_beats = ((duration - offset) / period).floor().max(0.0) as i64 + 1;
    let n_beats = n_beats.max(1) as usize;

    // Per-beat features.
    let full_at: Vec<f64> = (0..n_beats).map(|b| near(&full, rate, t0, beat_time(b as f64), 0.035)).collect();
    let low_at: Vec<f64> = (0..n_beats).map(|b| near(&bands[0], rate, t0, beat_time(b as f64), 0.035)).collect();
    let mid_at: Vec<f64> = (0..n_beats).map(|b| near(&bands[1], rate, t0, beat_time(b as f64), 0.035)).collect();
    let strength_ref = percentile(&full_at, 0.95).max(1e-6);

    // Loudness per beat, from the analysis windows' RMS.
    let win_centre = |i: usize| (i as f64 * 256.0 + 512.0 - pad as f64) / sr as f64;
    let mut beat_rms = vec![(0.0f64, 0usize); n_beats];
    for (i, &v) in flux.rms.iter().enumerate() {
        let b = beat_of(win_centre(i)).floor();
        if b >= 0.0 && (b as usize) < n_beats {
            let e = &mut beat_rms[b as usize];
            e.0 += (v as f64).powi(2);
            e.1 += 1;
        }
    }
    let beat_db: Vec<f64> = beat_rms.iter().map(|(s, n)| if *n > 0 { db((s / *n as f64).sqrt()) } else { -120.0 }).collect();
    let (lo, hi) = (percentile(&beat_db, 0.05), percentile(&beat_db, 0.95));
    let energy: Vec<f64> = beat_db.iter().map(|d| r((1.0 - (hi - d) / (hi - lo).max(12.0)).clamp(0.0, 1.0), 2)).collect();

    // Bar phase: kicks on the downbeat, snares on the backbeat, and changes
    // of loudness (sections start on a downbeat). Kick-on-every-beat music is
    // symmetric under a half-bar shift; only the loudness changes break that tie.
    let change: Vec<f64> = (0..n_beats)
        .map(|b| {
            let m = |a: usize, c: usize| {
                let s = &energy[a.min(n_beats)..c.min(n_beats)];
                if s.is_empty() {
                    0.0
                } else {
                    s.iter().sum::<f64>() / s.len() as f64
                }
            };
            if b < 2 || b + 2 > n_beats {
                0.0
            } else {
                (m(b, b + bpb as usize) - m(b.saturating_sub(bpb as usize), b)).abs()
            }
        })
        .collect();
    let change_total: f64 = change.iter().map(|c| c * c).sum::<f64>().max(1e-9);
    let mut scores: Vec<(u32, f64)> = Vec::new();
    for k in 0..bpb {
        let (mut on, mut on_n, mut back, mut back_n) = (0.0, 0.0f64, 0.0, 0.0f64);
        for b in 0..n_beats {
            // Beats cut off by the ends of the file would bias the vote.
            let t = beat_time(b as f64);
            if t < 0.02 || t > duration - 0.15 {
                continue;
            }
            // Loud beats count more, silent ones (a tail, a break) not at all.
            let w = energy[b];
            let pos = (b as i64 - k as i64).rem_euclid(bpb as i64) as u32;
            if pos == 0 {
                on += w * (low_at[b] - 0.5 * mid_at[b]);
                on_n += w;
            } else if bpb == 4 && (pos == 1 || pos == 3) {
                back += w * mid_at[b];
                back_n += w;
            }
        }
        let at_bar: f64 = (0..n_beats).filter(|b| (*b as i64 - k as i64).rem_euclid(bpb as i64) == 0).map(|b| change[b] * change[b]).sum();
        let sections = if change_total > 0.05 { at_bar / change_total } else { 0.0 };
        let score = on / on_n.max(1e-6) + 0.5 * back / back_n.max(1e-6) + sections + if k == 0 { 0.02 } else { 0.0 };
        scores.push((k, score));
    }
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    let downbeat = scores[0].0;
    let downbeat_confidence = if scores.len() > 1 { ((scores[0].1 - scores[1].1) / (scores[0].1.abs() + 1e-6)).clamp(0.0, 1.0) } else { 1.0 };
    let bar_of = |b: f64| ((b - downbeat as f64) / bpb as f64).floor() as i64 + 1;

    let beats_out: Vec<Beat> = (0..n_beats)
        .map(|b| {
            let pos = (b as i64 - downbeat as i64).rem_euclid(bpb as i64) as u32 + 1;
            Beat { b: b as i64, t: r(beat_time(b as f64).max(0.0), 3), bar: bar_of(b as f64), beat: pos, s: r((full_at[b] / strength_ref).clamp(0.0, 1.0), 2) }
        })
        .collect();
    let downbeats: Vec<Downbeat> = beats_out.iter().filter(|b| b.beat == 1).map(|b| Downbeat { bar: b.bar, t: b.t, b: b.b as f64 }).collect();

    // Band onsets.
    let gaps = [0.1, 0.1, 0.06];
    let mut lists: Vec<Vec<Hit>> = Vec::new();
    for (i, env) in bands.iter().enumerate() {
        let peaks = pick(env, rate, t0, gaps[i], 1.5);
        let top = peaks.iter().fold(0.0f32, |m, p| m.max(p.1)).max(1e-6);
        lists.push(
            peaks
                .into_iter()
                .filter(|p| p.0 >= -0.05 && p.0 < duration)
                .map(|(t, v)| Hit { t: r(t.max(0.0), 3), b: r(beat_of(t), 2), s: r((v / top) as f64, 2) })
                .filter(|h| h.s >= 0.1)
                .collect(),
        );
    }
    let hat = lists.pop().unwrap_or_default();
    let snare = lists.pop().unwrap_or_default();
    let kick = lists.pop().unwrap_or_default();
    let on_grid = {
        let all: Vec<f64> = kick.iter().chain(snare.iter()).map(|h| h.t).collect();
        let near_grid = all.iter().filter(|&&t| ((beat_of(t) * 2.0).round() - beat_of(t) * 2.0).abs() * period / 2.0 <= 0.05).count();
        if all.is_empty() {
            0.0
        } else {
            near_grid as f64 / all.len() as f64
        }
    };

    // Sections from changes in bar energy; the drop is the largest rise.
    let first_bar = bar_of(0.0).max(1);
    let last_bar = bar_of((n_beats - 1) as f64);
    let bar_energy: Vec<(i64, f64)> = (first_bar..=last_bar)
        .map(|bar| {
            let v: Vec<f64> = beats_out.iter().filter(|b| b.bar == bar).map(|b| energy[b.b as usize]).collect();
            (bar, if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 })
        })
        .collect();
    let nb = bar_energy.len();
    let w = if nb >= 8 { 2 } else { 1 };
    let mean = |a: usize, b: usize| {
        let s = &bar_energy[a.min(nb)..b.min(nb)];
        if s.is_empty() {
            0.0
        } else {
            s.iter().map(|x| x.1).sum::<f64>() / s.len() as f64
        }
    };
    let novelty: Vec<f64> = (0..nb).map(|k| if k == 0 { 0.0 } else { mean(k, k + w) - mean(k.saturating_sub(w), k) }).collect();
    let mut cuts: Vec<usize> = Vec::new();
    for k in 1..nb {
        let v = novelty[k].abs();
        if v < 0.2 || (k > 1 && novelty[k - 1].abs() > v) || (k + 1 < nb && novelty[k + 1].abs() > v) {
            continue;
        }
        if cuts.last().is_some_and(|&c| k - c < 2) || k + 2 > nb {
            continue;
        }
        cuts.push(k);
    }
    let drop_k = cuts.iter().copied().filter(|&k| novelty[k] >= 0.25).max_by(|&a, &b| novelty[a].total_cmp(&novelty[b]));
    let bar_start = |bar: i64| beat_time((downbeat as i64 + (bar - 1) * bpb as i64) as f64);
    let drop = drop_k.map(|k| {
        let bar = bar_energy[k].0;
        let t = bar_start(bar);
        Drop { t: r(t, 3), b: r(beat_of(t), 2), bar, rise: r(novelty[k], 2) }
    });
    let mut bounds = vec![0usize];
    bounds.extend(cuts.iter().copied());
    bounds.push(nb);
    let mut sections: Vec<Section> = Vec::new();
    for pair in bounds.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a >= b {
            continue;
        }
        let e = mean(a, b);
        let label = if Some(a) == drop_k {
            "drop"
        } else if e >= 0.7 {
            "high"
        } else if e >= 0.4 {
            "mid"
        } else {
            "low"
        };
        let from = if a == 0 { 0.0 } else { bar_start(bar_energy[a].0) };
        let to = if b >= nb { duration } else { bar_start(bar_energy[b].0) };
        sections.push(Section { label: label.into(), from: r(from, 3), to: r(to, 3), from_beat: r(beat_of(from), 2), to_beat: r(beat_of(to), 2), bar: bar_energy[a].0, bars: (b - a) as i64, energy: r(e, 2) });
    }
    let drop_index = sections.iter().position(|s| s.label == "drop");
    if sections.len() > 1 && sections[0].label != "drop" && sections[0].energy + 0.15 < sections[1].energy {
        sections[0].label = "intro".into();
    }
    if let Some(d) = drop_index {
        if d >= 1 && sections[d - 1].label != "intro" && sections[d - 1].energy < sections[d].energy {
            sections[d - 1].label = "build".into();
        }
        let last = sections.len() - 1;
        if last > d && sections[last].energy + 0.2 < sections[d].energy {
            sections[last].label = "outro".into();
        }
    }
    let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
    for s in &mut sections {
        let n = seen.entry(s.label.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            s.label = format!("{}{}", s.label, n);
        }
    }

    let mut markers: Vec<Marker> = Vec::new();
    if let Some(d) = &drop {
        markers.push(Marker { label: "drop".into(), t: d.t, b: d.b });
    }
    for s in sections.iter().filter(|_| sections.len() > 1) {
        if s.label != "drop" {
            markers.push(Marker { label: s.label.clone(), t: s.from, b: s.from_beat });
        }
    }
    markers.sort_by(|a, b| a.t.total_cmp(&b.t));

    let rms = (samples.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
    let peak = samples.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
    let mut notes = Vec::new();
    if tempo.confidence < 0.3 {
        notes.push(format!("the tempo is uncertain (confidence {:.2}); other candidates: {:?}. Pass --bpm if you know it.", tempo.confidence, tempo.candidates.iter().map(|c| r(c.0, 1)).collect::<Vec<_>>()));
    }
    if downbeat_confidence < 0.15 {
        notes.push(format!("the bar phase is a guess (confidence {downbeat_confidence:.2}): bar 1 may start on another beat; check by ear"));
    }
    if drop.is_none() {
        notes.push("no clear drop: the energy does not rise sharply anywhere".into());
    }
    Ok(Song {
        format: FORMAT.into(),
        source: source.into(),
        duration: r(duration, 3),
        tempo: TempoInfo { bpm: r(tempo.bpm, 2), confidence: r(tempo.confidence, 2), candidates: tempo.candidates.iter().skip(1).map(|c| r(c.0, 1)).collect(), on_grid: r(on_grid, 2) },
        grid: Grid { beat_offset: r(offset, 4), beat_seconds: r(period, 5), beats_per_bar: bpb, downbeat, downbeat_confidence: r(downbeat_confidence, 2) },
        beats: beats_out,
        downbeats,
        onsets: Onsets { kick, snare, hat },
        energy,
        sections,
        drop,
        loudness: Loudness { rms_dbfs: r(db(rms), 1), peak_dbfs: r(db(peak), 1), beat_min_dbfs: r(lo, 1), beat_max_dbfs: r(hi, 1) },
        markers,
        notes,
    })
}

/// Decode and analyse a song or the soundtrack of a video.
pub fn analyze_file(path: &std::path::Path, opts: &SongOpts) -> anyhow::Result<Song> {
    let samples = crate::media::decode_audio_file(path, RATE)?;
    analyze_samples(&samples, RATE, &path.to_string_lossy(), opts)
}

/// A few lines for a person or an agent.
pub fn summary(s: &Song) -> String {
    let mut out = format!(
        "{}: {:.2} s, {} BPM (confidence {:.2}), beat 0 at {:.3} s, {} beats, {} bars of {} (bar 1 on beat {}, confidence {:.2})\n",
        s.source,
        s.duration,
        s.tempo.bpm,
        s.tempo.confidence,
        s.grid.beat_offset,
        s.beats.len(),
        s.downbeats.len(),
        s.grid.beats_per_bar,
        s.grid.downbeat,
        s.grid.downbeat_confidence
    );
    out.push_str(&format!("onsets: {} kick, {} snare, {} hat; {:.0}% of kicks and snares on the grid\n", s.onsets.kick.len(), s.onsets.snare.len(), s.onsets.hat.len(), s.tempo.on_grid * 100.0));
    match &s.drop {
        Some(d) => out.push_str(&format!("drop: {:.3} s (beat {}, bar {}), energy rise {:.2}\n", d.t, d.b, d.bar, d.rise)),
        None => out.push_str("drop: none found\n"),
    }
    out.push_str("sections:");
    for sec in &s.sections {
        out.push_str(&format!(" {} {:.2}-{:.2} s (bar {}, {} bars, energy {:.2});", sec.label, sec.from, sec.to, sec.bar, sec.bars, sec.energy));
    }
    out.push('\n');
    out.push_str(&format!("loudness: {} dBFS RMS, peak {} dBFS\n", s.loudness.rms_dbfs, s.loudness.peak_dbfs));
    for n in &s.notes {
        out.push_str(&format!("note: {n}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic song: kicks on every beat, a clap on beats 2 and 4, hats on
    /// the off-beats; quiet for `quiet_bars`, then four times louder.
    fn song(bpm: f64, first: f64, bars: usize, quiet_bars: usize) -> Vec<f32> {
        let sr = RATE as f64;
        let period = 60.0 / bpm;
        let n = ((first + (bars * 4) as f64 * period + 1.0) * sr) as usize;
        let mut s = vec![0.0f32; n];
        let mut seed = 7u32;
        let mut rnd = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        for b in 0..bars * 4 {
            let gain = if b / 4 < quiet_bars { 0.25 } else { 1.0 };
            let t = first + b as f64 * period;
            let i0 = (t * sr) as usize;
            for j in 0..(0.25 * sr) as usize {
                let tt = j as f64 / sr;
                if let Some(v) = s.get_mut(i0 + j) {
                    // Kick: falling sine.
                    let ph = std::f64::consts::TAU * (50.0 * tt + 100.0 * (1.0 - (-tt * 30.0).exp()) / 30.0);
                    *v += (ph.sin() * (-tt * 12.0).exp() * 0.7 * gain) as f32;
                }
            }
            if b % 4 == 1 || b % 4 == 3 {
                for j in 0..(0.15 * sr) as usize {
                    let tt = j as f32 / sr as f32;
                    if let Some(v) = s.get_mut(i0 + j) {
                        *v += rnd() * (-tt * 25.0).exp() * 0.6 * gain as f32;
                    }
                }
            }
            let ih = ((t + period / 2.0) * sr) as usize;
            let mut last = 0.0f32;
            for j in 0..(0.04 * sr) as usize {
                let tt = j as f32 / sr as f32;
                let x = rnd();
                if let Some(v) = s.get_mut(ih + j) {
                    *v += (x - last) * (-tt * 90.0).exp() * 0.3 * gain as f32;
                }
                last = x;
            }
        }
        s
    }

    #[test]
    fn finds_tempo_bars_bands_and_the_drop() {
        let audio = song(128.0, 0.3, 16, 8);
        let s = analyze_samples(&audio, RATE, "synthetic", &SongOpts::default()).unwrap();
        assert!((s.tempo.bpm - 128.0).abs() < 0.6, "{}", summary(&s));
        let period = 60.0 / 128.0;
        let mut d = (s.grid.beat_offset - 0.3).rem_euclid(period);
        if d > period / 2.0 {
            d -= period;
        }
        assert!(d.abs() < 0.02, "beat 0 at {} {}", s.grid.beat_offset, summary(&s));
        // Beat 0 is the first kick, which is a downbeat.
        assert_eq!(s.grid.downbeat, 0, "{}", summary(&s));
        assert!(s.beats.len() >= 64);
        // Kicks on the beats, claps on the backbeats, hats between.
        let frac = |b: f64| b - b.round();
        assert!(s.onsets.kick.len() >= 56 && s.onsets.kick.len() <= 70, "{} kicks", s.onsets.kick.len());
        assert!(s.onsets.kick.iter().filter(|h| frac(h.b).abs() < 0.1).count() >= s.onsets.kick.len() * 9 / 10);
        let snares_on_backbeat = s.onsets.snare.iter().filter(|h| frac(h.b).abs() < 0.1 && (h.b.round() as i64).rem_euclid(2) == 1).count();
        assert!(snares_on_backbeat >= 24, "{snares_on_backbeat} snares on 2 and 4");
        let hats_off = s.onsets.hat.iter().filter(|h| (frac(h.b).abs() - 0.5).abs() < 0.1).count();
        assert!(hats_off >= 48, "{hats_off} hats on the off-beats of {}", s.onsets.hat.len());
        // The drop is bar 9, where it gets loud.
        let drop = s.drop.as_ref().expect("a drop");
        assert_eq!(drop.bar, 9, "{}", summary(&s));
        assert!((drop.t - (0.3 + 32.0 * period)).abs() < 0.03);
        assert!(s.markers.iter().any(|m| m.label == "drop"));
        assert!(s.sections.len() >= 2 && s.sections.iter().any(|x| x.label == "drop"), "{:?}", s.sections);
        assert!(s.energy[4] < 0.5 && s.energy[40] > 0.9, "{:?}", s.energy);
    }

    #[test]
    fn a_constant_loop_has_no_drop() {
        let audio = song(100.0, 0.1, 8, 0);
        let s = analyze_samples(&audio, RATE, "loop", &SongOpts::default()).unwrap();
        assert!((s.tempo.bpm - 100.0).abs() < 0.6);
        assert!(s.drop.is_none(), "{}", summary(&s));
        assert!(s.notes.iter().any(|n| n.contains("no clear drop")));
    }
}
