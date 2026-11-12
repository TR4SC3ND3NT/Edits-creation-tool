//! The whole analysis: ingest -> beats -> shots -> layers -> reference frames
//! -> tracks -> exposure / tone / blur -> curves -> slots -> merge.

use crate::beats::{self, Onset, TempoOpts};
use crate::blur::{self, BlurSample};
use crate::curves::{self, decay, named};
use crate::exposure::{self, ExposureSample};
use crate::img::{median, save_rgb, Frames, Sim};
use crate::ingest;
use crate::layers::{self, Plate};
use crate::measure::{luma_image, make_pair};
use crate::media;
use crate::schema::*;
use crate::shots::{self, Cut};
use crate::signals::{self, FrameSig};
use crate::tone::{self, ToneSample};
use crate::track::{self, Method, TrackSample};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct AnalyzeOpts {
    /// Height in pixels of the frames the analysis works on.
    pub height: u32,
    /// Use this tempo instead of estimating it.
    pub bpm: Option<f64>,
    /// Seconds from the start of the video to beat 0 (overrides the estimate).
    pub beat_offset: Option<f64>,
    /// Keep raw per-frame track samples in the output.
    pub samples: bool,
    /// Analyse the whole clip even if it loops.
    pub whole: bool,
}

impl Default for AnalyzeOpts {
    fn default() -> Self {
        Self { height: 360, bpm: None, beat_offset: None, samples: false, whole: false }
    }
}

/// Result of [`analyze_frames`]: the document plus things worth keeping in memory.
pub struct Analyzed {
    pub analysis: Analysis,
    /// Overlay plate at analysis resolution.
    pub plate: Plate,
    pub sigs: Vec<FrameSig>,
}

#[derive(Clone, Copy)]
struct FrameMeasure {
    exp: ExposureSample,
    tone: ToneSample,
    blur: BlurSample,
    inverted: bool,
}

struct ShotWork {
    f0: usize,
    f1: usize,
    ref_frame: usize,
    track: Vec<TrackSample>,
    tracked: bool,
    reasons: Vec<String>,
    meas: Vec<FrameMeasure>,
}

/// The beat grid: tempo from the audio (or from the picture when there is no
/// usable audio), beat 0 at the grid beat nearest the first analysed frame,
/// phase nudged by at most one frame so that cuts land on the grid.
pub fn beat_grid(audio: Option<(&[f32], u32)>, sigs: &[FrameSig], fps: f64, opts: &AnalyzeOpts, a0: usize, cuts: &[usize]) -> Beats {
    let topts = TempoOpts { bpm: opts.bpm, ..Default::default() };
    let mut source = "audio";
    let mut tempo = audio.and_then(|(s, sr)| beats::tempo(&beats::onset_envelope(s, sr), &topts));
    if tempo.as_ref().map(|t| t.confidence < 0.08 && opts.bpm.is_none()).unwrap_or(true) {
        let env: Vec<f32> = sigs.iter().map(|s| s.diff).collect();
        let onset = Onset { rate: fps, t0: 0.0, env: beats::normalise_envelope(&env, fps) };
        let visual = beats::tempo(&onset, &topts);
        if visual.is_some() && (tempo.is_none() || visual.as_ref().unwrap().confidence > tempo.as_ref().unwrap().confidence) {
            tempo = visual;
            source = "visual";
        }
    }
    let Some(t) = tempo else {
        let off = opts.beat_offset.unwrap_or(a0 as f64 / fps);
        return Beats {
            source: "none".into(),
            bpm: Est::new(opts.bpm.unwrap_or(60.0), "bpm", 0.0, Ident::NeedsAgent),
            bpm_raw: opts.bpm.unwrap_or(60.0),
            offset_seconds: Est::new(off, "s", 0.0, Ident::NeedsAgent),
            audio_offset_seconds: None,
            refine_shift_frames: 0.0,
            frames_per_beat: fps * 60.0 / opts.bpm.unwrap_or(60.0),
            candidates: vec![],
        };
    };
    let period = 60.0 / t.bpm;
    let start = a0 as f64 / fps;
    let base = t.phase + ((start - t.phase) / period).round() * period;
    let mut offset = base;
    let mut shift = 0.0;
    if let Some(o) = opts.beat_offset {
        offset = o;
    } else {
        // Nudge the phase (within a frame) so that events keyed on the grid are
        // shown on the frames where the reference shows them.
        let events: Vec<usize> = std::iter::once(a0).chain(cuts.iter().copied()).collect();
        let score = |d: f64| {
            let off = base + d / fps;
            events
                .iter()
                .filter(|e| {
                    let b = (**e as f64 / fps - off) / period;
                    let h = (b * 2.0).round() / 2.0;
                    (((off + h * period) * fps - 1e-6).ceil() - **e as f64).abs() < 0.5
                })
                .count()
        };
        let steps: Vec<f64> = (-20..=20).map(|i| i as f64 * 0.05).collect();
        let best = steps.iter().map(|d| score(*d)).max().unwrap_or(0);
        if best > score(0.0) {
            let good: Vec<f64> = steps.iter().copied().filter(|d| score(*d) == best).collect();
            // The candidate closest to no shift, from the middle of its plateau.
            let near = good.iter().copied().fold(f64::MAX, |m, d| if d.abs() < m.abs() { d } else { m });
            let plateau: Vec<f64> = good.iter().copied().filter(|d| (d - near).abs() < 0.5 && d.signum() == near.signum()).collect();
            shift = median(&plateau);
            offset = base + shift / fps;
        }
    }
    let conf = if source == "override" || opts.bpm.is_some() { 1.0 } else { t.confidence };
    Beats {
        source: if opts.bpm.is_some() { "override".into() } else { source.into() },
        bpm: Est::measured(t.bpm, "bpm", conf),
        bpm_raw: round_to(t.bpm_raw, 3),
        offset_seconds: Est::measured(offset, "s", if opts.beat_offset.is_some() { 1.0 } else { (conf * 0.9).max(0.1) }),
        audio_offset_seconds: Some(round_to(base, 4)),
        refine_shift_frames: round_to(shift, 2),
        frames_per_beat: round_to(fps * period, 4),
        candidates: t.candidates.iter().map(|(b, s)| TempoCandidate { bpm: round_to(*b, 2), score: round_to(*s, 3) }).collect(),
    }
}

/// Reference frame of a shot: sharp, not flashed, not in a tone hit, preferably still.
pub fn reference_frame(sigs: &[FrameSig], inverted: &[bool], f0: usize, f1: usize) -> usize {
    let n = f1 - f0;
    if n <= 2 {
        return f0 + n / 2;
    }
    let lum: Vec<f64> = (f0..f1).map(|i| sigs[i].lum_center as f64).collect();
    let sat: Vec<f64> = (f0..f1).map(|i| sigs[i].sat as f64).collect();
    let lum_cap = crate::img::percentile(&lum, 0.6) + 2.0;
    let lum_floor = 0.5 * median(&lum);
    let sat_floor = 0.6 * median(&sat);
    let ok = |i: usize| !inverted[i] && (sigs[i].lum_center as f64) <= lum_cap && (sigs[i].lum_center as f64) >= lum_floor && sigs[i].sat as f64 >= sat_floor && sigs[i].contrast > 4.0;
    let mut cands: Vec<usize> = (f0..f1).filter(|i| ok(*i)).collect();
    if cands.is_empty() {
        cands = (f0..f1).filter(|i| !inverted[*i]).collect();
    }
    if cands.is_empty() {
        cands = (f0..f1).collect();
    }
    let best = cands.iter().map(|i| sigs[*i].sharp).fold(0.0f32, f32::max);
    // Among the sharpest, the one that moves least (a hold).
    cands
        .iter()
        .copied()
        .filter(|i| sigs[*i].sharp >= 0.92 * best)
        .min_by(|a, b| {
            let da = sigs[*a].diff.min(sigs.get(a + 1).map(|s| s.diff).unwrap_or(f32::MAX));
            let db = sigs[*b].diff.min(sigs.get(b + 1).map(|s| s.diff).unwrap_or(f32::MAX));
            da.partial_cmp(&db).unwrap()
        })
        .unwrap_or(f0 + n / 2)
}

/// Is the reference frame a small panel on a uniform background?
fn inset_panel(frames: &Frames, ref_frame: usize, usable: &[bool]) -> bool {
    let g = frames.gray(ref_frame);
    let (w, h) = (g.w, g.h);
    let (mx, my) = ((w as f64 * 0.14) as usize, (h as f64 * 0.14) as usize);
    let (mut ring, mut centre) = (Vec::new(), Vec::new());
    for y in 0..h {
        for x in 0..w {
            if !usable[y * w + x] {
                continue;
            }
            if y < my || y >= h - my || x < mx || x >= w - mx {
                ring.push(g.d[y * w + x] as f64);
            } else {
                centre.push(g.d[y * w + x] as f64);
            }
        }
    }
    let sd = |v: &[f64]| {
        let m = crate::img::mean(v);
        (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt()
    };
    ring.len() > 100 && sd(&ring) < 0.05 && crate::img::mean(&ring) < 0.12 && sd(&centre) > 0.12
}

fn process_shot(frames: &Frames, sigs: &[FrameSig], toggles: &[bool], usable: &[bool], f0: usize, f1: usize) -> ShotWork {
    let n = f1 - f0;
    // Tone state from frame-to-frame sign flips; the majority state is "normal".
    let mut state = vec![false; n];
    for i in 1..n {
        state[i] = state[i - 1] ^ toggles[f0 + i];
    }
    if state.iter().filter(|s| **s).count() * 2 > n {
        state.iter_mut().for_each(|s| *s = !*s);
    }
    let mut inv_abs = vec![false; sigs.len()];
    for i in 0..n {
        inv_abs[f0 + i] = state[i];
    }
    let ref_frame = reference_frame(sigs, &inv_abs, f0, f1);
    let track = track::track_shot(frames, f0, f1, ref_frame, usable);
    let valid = track.iter().filter(|t| t.valid()).count();
    let mut reasons = Vec::new();
    let mut tracked = true;
    if n >= 2 && (valid as f64) < 0.6 * n as f64 {
        tracked = false;
        reasons.push(format!(
            "registration to the reference frame held on only {valid} of {n} frames: non-similarity motion (3D turn, cube), content changing inside the shot, or too little texture"
        ));
    }
    if inset_panel(frames, ref_frame, usable) {
        tracked = false;
        reasons.push("small inset panel on a uniform dark background (cube face or picture-in-picture): layout and 3D pose not measured".into());
    }
    let meas: Vec<FrameMeasure> = (f0..f1)
        .into_par_iter()
        .map(|i| {
            let ts = &track[i - f0];
            let sim: Option<&Sim> = if tracked { Some(&ts.sim) } else { None };
            let pair = make_pair(frames, ref_frame, i, sim, usable);
            let mut inverted = state[i - f0];
            if tracked && ts.valid() {
                let c = tone::lum_correlation(&pair);
                if c.abs() > 0.4 {
                    inverted = c < 0.0;
                }
            }
            let tone = tone::measure(&pair, inverted);
            let mut exp = exposure::measure(&pair, inverted);
            if tone.threshold.is_some() {
                exp = ExposureSample::default();
            }
            let cur = frames.gray(i);
            let reference = if pair.aligned && pair.coverage > 0.999 { luma_image(&pair.refw, pair.w, pair.h) } else { frames.gray(ref_frame) };
            let blur = if i == ref_frame { BlurSample::default() } else { blur::measure(&cur, &reference) };
            FrameMeasure { exp, tone, blur, inverted }
        })
        .collect();
    let mut meas = meas;
    if !tracked {
        // The reference frame does not show the same thing as the other frames
        // (the content turns or changes), so compare neighbours instead and
        // accumulate outwards from the reference frame.
        let step = |from: usize, to: usize| {
            let pair = make_pair(frames, from, to, None, usable);
            exposure::measure_between(&pair, meas[to - f0].inverted != meas[from - f0].inverted)
        };
        let steps_fwd: Vec<ExposureSample> = (ref_frame + 1..f1).into_par_iter().map(|i| step(i - 1, i)).collect();
        let steps_back: Vec<ExposureSample> = (f0..ref_frame).into_par_iter().map(|i| step(i + 1, i)).collect();
        let mut acc = 0.0;
        for (k, i) in (ref_frame + 1..f1).enumerate() {
            acc += steps_fwd[k].stops;
            meas[i - f0].exp = ExposureSample { stops: acc, confidence: 0.5 * steps_fwd[k].confidence, ..steps_fwd[k] };
        }
        acc = 0.0;
        for i in (f0..ref_frame).rev() {
            let s = steps_back[i - f0];
            acc += s.stops;
            meas[i - f0].exp = ExposureSample { stops: acc, confidence: 0.5 * s.confidence, ..s };
        }
    }
    ShotWork { f0, f1, ref_frame, track, tracked, reasons, meas }
}

struct Timing<'a> {
    beats: &'a Beats,
    fps: f64,
}
impl Timing<'_> {
    fn beat(&self, frame: usize) -> f64 {
        round_to(self.beats.beat_of_frame(frame as f64, self.fps), 4)
    }
    fn snap(&self, frame: usize) -> Option<f64> {
        self.beats.snap(frame as f64, self.fps)
    }
    fn key(&self, f0: usize, k: &curves::FitKey, decimals: i32) -> CurveKey {
        let frame = f0 + k.idx;
        CurveKey {
            frame,
            beat: self.beat(frame),
            beat_snapped: self.snap(frame),
            v: k.v.iter().map(|v| round_to(*v, decimals)).collect(),
            ease: k.ease.clone(),
            rms: if k.rms > 0.0 { Some(round_to(k.rms, decimals + 1)) } else { None },
            runner_up: k.runner_up.as_ref().map(|(e, r)| EaseAlt { ease: e.clone(), rms: round_to(*r, decimals + 1) }),
        }
    }
    fn plain_key(&self, frame: usize, v: f64, ease: edits_core::Ease) -> CurveKey {
        CurveKey { frame, beat: self.beat(frame), beat_snapped: self.snap(frame), v: vec![round_to(v, 4)], ease, rms: None, runner_up: None }
    }
    fn event(&self, kind: &str, f0: usize, f1: usize, frame: usize, params: BTreeMap<String, Est>, note: Option<String>) -> Event {
        Event { kind: kind.into(), f0, f1, frame, beat: self.beat(frame), beat_snapped: self.snap(frame), params, note }
    }
}

/// A step curve: `neutral` outside the runs, `value` inside (hold easing).
fn step_curve(t: &Timing, f0: usize, n: usize, runs: &[(usize, usize, f64)], neutral: f64) -> Vec<CurveKey> {
    let mut keys: Vec<CurveKey> = Vec::new();
    let push = |frame: usize, v: f64, keys: &mut Vec<CurveKey>| {
        if let Some(last) = keys.last_mut() {
            if last.frame == frame {
                last.v = vec![round_to(v, 4)];
                return;
            }
        }
        keys.push(t.plain_key(frame, v, named("hold")));
    };
    push(f0, neutral, &mut keys);
    for (a, b, v) in runs {
        push(f0 + a, *v, &mut keys);
        if *b < n {
            push(f0 + b, neutral, &mut keys);
        }
    }
    keys
}

fn build_shot(id: usize, w: &ShotWork, sigs: &[FrameSig], dup: &[bool], t: &Timing, cut: Option<&Cut>, global_sat: f64, frame_h: usize, opts: &AnalyzeOpts) -> Shot {
    let (f0, f1) = (w.f0, w.f1);
    let n = f1 - f0;
    let mut curves_out: Vec<Curve> = Vec::new();
    let mut events: Vec<Event> = Vec::new();
    let mut notes = w.reasons.clone();
    let mut shake_out = None;
    let valid = w.track.iter().filter(|s| s.valid()).count();
    let confs: Vec<f64> = w.track.iter().filter(|s| s.valid()).map(|s| s.confidence()).collect();
    let track_conf = crate::img::mean(&confs);

    // ---- transform curves ---------------------------------------------------
    if w.tracked && n >= 2 {
        let wts: Vec<f64> = (0..n).map(|i| if w.track[i].valid() && !dup[f0 + i] { 1.0 } else { 0.0 }).collect();
        let scale: Vec<Vec<f64>> = w.track.iter().map(|s| vec![s.sim.scale()]).collect();
        let rot: Vec<Vec<f64>> = w.track.iter().map(|s| vec![s.sim.rotation_deg()]).collect();
        let pos: Vec<Vec<f64>> = w.track.iter().map(|s| vec![s.sim.tx, s.sim.ty]).collect();
        let fs = curves::fit(&scale, &wts, 0.003);
        if !(fs.is_constant() && (fs.keys[0].v[0] - 1.0).abs() < 0.004) {
            curves_out.push(Curve {
                prop: "scale".into(),
                unit: "x reference frame".into(),
                confidence: round_to(track_conf, 2),
                identifiability: Ident::RelativeOnly,
                keys: fs.keys.iter().map(|k| t.key(f0, k, 4)).collect(),
            });
        }
        let fp = curves::fit(&pos, &wts, 0.0015);
        if !(fp.is_constant() && fp.keys[0].v.iter().all(|v| v.abs() < 0.002)) {
            curves_out.push(Curve {
                prop: "position".into(),
                unit: "frame heights (x right, y down)".into(),
                confidence: round_to(track_conf, 2),
                identifiability: Ident::RelativeOnly,
                keys: fp.keys.iter().map(|k| t.key(f0, k, 4)).collect(),
            });
        }
        let fr = curves::fit(&rot, &wts, 0.15);
        if !(fr.is_constant() && fr.keys[0].v[0].abs() < 0.2) {
            curves_out.push(Curve {
                prop: "rotation".into(),
                unit: "degrees clockwise".into(),
                confidence: round_to(track_conf, 2),
                identifiability: Ident::RelativeOnly,
                keys: fr.keys.iter().map(|k| t.key(f0, k, 2)).collect(),
            });
        }
        let (rx, ry, ratio) = curves::shake(&pos, &wts, &fp);
        if rx.max(ry) > 0.0008 && n >= 6 {
            shake_out = Some(Shake {
                rms_x: Est::measured(rx, "frame heights", track_conf * 0.7),
                rms_y: Est::measured(ry, "frame heights", track_conf * 0.7),
                late_over_early: Est::new(ratio, "ratio", track_conf * 0.4, Ident::Qualitative),
            });
        }
    }

    // ---- exposure -------------------------------------------------------------
    let stops: Vec<f64> = w.meas.iter().map(|m| m.exp.stops).collect();
    let econf: Vec<f64> = w.meas.iter().map(|m| m.exp.confidence * if w.tracked { 1.0 } else { 0.6 }).collect();
    let fl = exposure::flashes(&stops, &econf);
    let mut ekeys: Vec<CurveKey> = Vec::new();
    for f in &fl {
        let clipped = w.meas[f.peak].exp.clipped;
        let mut params = BTreeMap::new();
        params.insert("peak".into(), Est::measured(f.peak_stops, "stops", f.confidence));
        params.insert("decay_rate".into(), Est::measured(f.rate, "fraction of the remainder per frame", f.rate_confidence));
        params.insert("peak_luminance".into(), Est::measured(sigs[f0 + f.peak].lum_center as f64, "0..255 (centre of frame)", 0.95));
        let note = if clipped > 0.6 { Some("most of the frame is clipped at white: the peak is a lower bound".to_string()) } else { None };
        events.push(t.event("flash", f0 + f.start, f0 + f.end, f0 + f.peak, params, note));
        // Keys: (0 before) -> peak with a decay ease -> back to 0.
        let last_idx = ekeys.last().map(|k| k.frame - f0);
        if f.start < f.peak {
            let at = f.start.saturating_sub(1);
            if last_idx.map(|l| at > l).unwrap_or(true) {
                ekeys.push(t.plain_key(f0 + at, if f.start == 0 { stops[0].max(0.0) } else { 0.0 }, named("linear")));
            } else if let Some(k) = ekeys.last_mut() {
                k.ease = named("linear");
            }
        } else if f.peak > 0 && ekeys.is_empty() {
            ekeys.push(t.plain_key(f0, 0.0, named("hold")));
        }
        if ekeys.last().map(|k| k.frame == f0 + f.peak).unwrap_or(false) {
            ekeys.pop();
        }
        ekeys.push(t.plain_key(f0 + f.peak, f.peak_stops, decay(f.rate)));
        if f.end >= n {
            if n - 1 > f.peak {
                let v = f.peak_stops * (1.0 - f.rate).powi((n - 1 - f.peak) as i32);
                ekeys.push(t.plain_key(f0 + n - 1, v, named("hold")));
            }
        } else {
            ekeys.push(t.plain_key(f0 + f.end, 0.0, named("hold")));
        }
    }
    if !ekeys.is_empty() {
        let conf = crate::img::mean(&fl.iter().map(|f| f.confidence).collect::<Vec<_>>());
        curves_out.push(Curve { prop: "exposure.stops".into(), unit: "stops".into(), confidence: round_to(conf, 2), identifiability: Ident::Measured, keys: ekeys });
    }
    // A mix toward a colour instead of a gain.
    let fades: Vec<bool> = w.meas.iter().map(|m| w.tracked && m.exp.fade.map(|f| f.0 > 0.3).unwrap_or(false)).collect();
    for (a, b) in tone::runs(&fades) {
        let amt = (a..b).filter_map(|i| w.meas[i].exp.fade.map(|f| f.0)).fold(0.0, f64::max);
        let lvl = median(&(a..b).filter_map(|i| w.meas[i].exp.fade.map(|f| f.1)).collect::<Vec<_>>());
        let mut params = BTreeMap::new();
        params.insert("amount".into(), Est::new(amt, "0..1", 0.4, Ident::Qualitative));
        params.insert("color_luminance".into(), Est::new(lvl, "linear 0..1", 0.3, Ident::Qualitative));
        events.push(t.event("fade", f0 + a, f0 + b, f0 + a, params, Some("tones look mixed toward a colour rather than multiplied; reported as exposure as well".into())));
    }

    // ---- blur -------------------------------------------------------------------
    let blur_pct: Vec<Vec<f64>> = w.meas.iter().map(|m| vec![100.0 * m.blur.sigma_px / frame_h as f64]).collect();
    let bw: Vec<f64> = (0..n).map(|i| if dup[f0 + i] { 0.0 } else { 1.0 }).collect();
    let fb = curves::fit(&blur_pct, &bw, 0.08);
    let peak_blur = fb.keys.iter().map(|k| k.v[0]).fold(0.0, f64::max);
    if peak_blur >= 0.15 {
        let blurred: Vec<f64> = w.meas.iter().filter(|m| 100.0 * m.blur.sigma_px / frame_h as f64 > 0.3).map(|m| m.blur.anisotropy).collect();
        let aniso = median(&blurred);
        let directional = !blurred.is_empty() && aniso > 0.45;
        let clipped = crate::img::mean(&w.meas.iter().map(|m| m.exp.clipped).collect::<Vec<_>>());
        let mut conf = if w.tracked { 0.75 } else { 0.45 };
        if clipped > 0.3 {
            conf *= 0.7;
        }
        if directional {
            notes.push(format!("blur is anisotropic (index {aniso:.2}): directional, zoom or spin blur; kind not classified, radius is a Gaussian equivalent"));
        }
        let mut keys: Vec<CurveKey> = fb.keys.iter().map(|k| t.key(f0, k, 3)).collect();
        keys.iter_mut().for_each(|k| k.v[0] = k.v[0].max(0.0));
        curves_out.push(Curve {
            prop: "gaussian_blur.radius".into(),
            unit: "% of frame height (sigma)".into(),
            confidence: round_to(conf, 2),
            identifiability: if directional { Ident::NeedsAgent } else { Ident::Measured },
            keys,
        });
    }

    // ---- tone hits ----------------------------------------------------------------
    let trust = if w.tracked { 1.0 } else { 0.6 };
    let bw_frames: Vec<bool> = w.meas.iter().map(|m| m.tone.sat_confidence > 0.5 && m.tone.sat_ratio < 0.5).collect();
    let sat_runs: Vec<(usize, usize, f64)> = tone::runs(&bw_frames)
        .into_iter()
        .map(|(a, b)| {
            let v = median(&(a..b).map(|i| w.meas[i].tone.sat_ratio).collect::<Vec<_>>());
            (a, b, if v < 0.1 { 0.0 } else { round_to(v, 2) })
        })
        .collect();
    for (a, b, v) in &sat_runs {
        let conf = crate::img::mean(&(*a..*b).map(|i| w.meas[i].tone.sat_confidence).collect::<Vec<_>>()) * trust;
        let mut params = BTreeMap::new();
        params.insert("amount".into(), Est::measured(*v, "x reference frame (0 = black and white)", conf));
        events.push(t.event("saturation", f0 + a, f0 + b, f0 + a, params, None));
    }
    if !sat_runs.is_empty() {
        curves_out.push(Curve { prop: "saturation.amount".into(), unit: "x reference frame".into(), confidence: round_to(0.8 * trust, 2), identifiability: Ident::Measured, keys: step_curve(t, f0, n, &sat_runs, 1.0) });
    } else {
        // Black and white throughout: cannot be told from a black-and-white picture.
        let abs_sat = median(&(f0..f1).map(|i| sigs[i].sat as f64).collect::<Vec<_>>());
        if abs_sat < 8.0 && global_sat > 25.0 {
            let mut params = BTreeMap::new();
            params.insert("amount".into(), Est::new(0.0, "0 = black and white", 0.5, Ident::NeedsSource));
            events.push(t.event("saturation", f0, f1, f0, params, Some("the whole shot is black and white; it may be the picture itself".into())));
        }
    }
    let inv: Vec<bool> = w.meas.iter().map(|m| m.inverted).collect();
    let inv_runs: Vec<(usize, usize, f64)> = tone::runs(&inv).into_iter().map(|(a, b)| (a, b, 1.0)).collect();
    for (a, b, _) in &inv_runs {
        let corr = median(&(*a..*b).map(|i| w.meas[i].tone.lum_corr).collect::<Vec<_>>());
        let mut params = BTreeMap::new();
        params.insert("amount".into(), Est::measured(1.0, "0..1", if w.tracked { 0.85 } else { 0.6 }));
        params.insert("luminance_correlation".into(), Est::measured(corr, "correlation with reference frame", if w.tracked { 0.8 } else { 0.2 }));
        events.push(t.event("invert", f0 + a, f0 + b, f0 + a, params, None));
    }
    if !inv_runs.is_empty() {
        curves_out.push(Curve { prop: "invert.amount".into(), unit: "0..1".into(), confidence: if w.tracked { 0.85 } else { 0.6 }, identifiability: Ident::Measured, keys: step_curve(t, f0, n, &inv_runs, 0.0) });
    }
    let th: Vec<bool> = w.meas.iter().map(|m| m.tone.threshold.is_some()).collect();
    let th_runs: Vec<(usize, usize, f64)> = tone::runs(&th).into_iter().map(|(a, b)| (a, b, 1.0)).collect();
    if !th_runs.is_empty() {
        let levels: Vec<f64> = w.meas.iter().filter_map(|m| m.tone.threshold).collect();
        let level = median(&levels);
        for (a, b, _) in &th_runs {
            let l = median(&(*a..*b).filter_map(|i| w.meas[i].tone.threshold).collect::<Vec<_>>());
            let mut params = BTreeMap::new();
            params.insert("level".into(), Est::new(l, "luminance 0..1 of the reference frame", 0.6 * trust, Ident::Measured));
            events.push(t.event("threshold", f0 + a, f0 + b, f0 + a, params, None));
        }
        curves_out.push(Curve { prop: "threshold.amount".into(), unit: "0..1".into(), confidence: round_to(0.7 * trust, 2), identifiability: Ident::Measured, keys: step_curve(t, f0, n, &th_runs, 0.0) });
        curves_out.push(Curve { prop: "threshold.level".into(), unit: "luminance 0..1".into(), confidence: round_to(0.6 * trust, 2), identifiability: Ident::Measured, keys: vec![t.plain_key(f0, level, named("hold"))] });
    }
    let po: Vec<bool> = w.meas.iter().map(|m| m.tone.posterize.is_some() && m.exp.clipped < 0.3 && m.exp.stops < 1.0).collect();
    for (a, b) in tone::runs(&po) {
        let l = median(&(a..b).filter_map(|i| w.meas[i].tone.posterize.map(|v| v as f64)).collect::<Vec<_>>());
        let mut params = BTreeMap::new();
        params.insert("levels".into(), Est::new(l, "levels per channel", 0.3, Ident::Qualitative));
        events.push(t.event("posterize", f0 + a, f0 + b, f0 + a, params, None));
    }
    events.sort_by_key(|e| (e.frame, e.kind.clone()));

    // ---- cut timing -----------------------------------------------------------
    let b0 = t.beats.beat_of_frame(f0 as f64, t.fps);
    let fpb = t.beats.frames_per_beat;
    let half = (b0 * 2.0).round() / 2.0;
    let quarter = (b0 * 4.0).round() / 4.0;
    let off_half = (b0 - half) * fpb;
    let (grid, off) = if off_half.abs() <= 1.0 {
        (if (half - half.round()).abs() < 1e-9 { "beat" } else { "half_beat" }, off_half)
    } else if ((b0 - quarter) * fpb).abs() <= 1.0 {
        ("quarter_beat", (b0 - quarter) * fpb)
    } else {
        ("off_grid", off_half)
    };
    let samples = if opts.samples {
        Some(Samples {
            scale: w.track.iter().map(|s| round_to(s.sim.scale(), 4)).collect(),
            rotation: w.track.iter().map(|s| round_to(s.sim.rotation_deg(), 2)).collect(),
            x: w.track.iter().map(|s| round_to(s.sim.tx, 4)).collect(),
            y: w.track.iter().map(|s| round_to(s.sim.ty, 4)).collect(),
            confidence: w.track.iter().map(|s| round_to(s.confidence(), 2)).collect(),
            zncc: w.track.iter().map(|s| round_to(s.zncc, 2)).collect(),
            method: w
                .track
                .iter()
                .map(|s| match s.method {
                    Method::Reference => 'r',
                    Method::Direct => 'd',
                    Method::Chained => 'c',
                    Method::Bridged => 'b',
                })
                .collect(),
        })
    } else {
        None
    };
    Shot {
        id,
        f0,
        f1,
        beat0: t.beat(f0),
        beat1: t.beat(f1),
        beat0_snapped: t.snap(f0),
        beat1_snapped: t.snap(f1),
        cut: CutInfo {
            offset_frames: Est::measured(off, "frames", t.beats.bpm.confidence),
            grid: grid.into(),
            strength: round_to(cut.map(|c| c.strength as f64).unwrap_or(1.0), 2),
        },
        ref_frame: w.ref_frame,
        slot: String::new(),
        status: if w.tracked { Ident::Measured } else { Ident::NeedsAgent },
        notes,
        track: TrackSummary {
            model: "similarity".into(),
            valid_fraction: round_to(valid as f64 / n.max(1) as f64, 2),
            mean_confidence: round_to(track_conf, 2),
            direct: w.track.iter().filter(|s| s.method == Method::Direct).count(),
            chained: w.track.iter().filter(|s| s.method == Method::Chained).count(),
            bridged: w.track.iter().filter(|s| s.method == Method::Bridged).count(),
        },
        curves: curves_out,
        events,
        shake: shake_out,
        samples,
    }
}

/// Group shots that show the same picture: their reference frames register to each other.
pub fn cluster_slots(frames: &Frames, refs: &[usize], usable: &[bool]) -> Vec<usize> {
    let n = refs.len();
    let pyrs: Vec<track::Pyramid> = refs.par_iter().map(|r| track::pyramid(&frames.gray(*r))).collect();
    if n == 0 {
        return vec![];
    }
    let masks = track::mask_pyramid(usable, frames.w, frames.h, pyrs[0].levels.len());
    let tmpls: Vec<track::Template> = pyrs.par_iter().map(|p| track::Template::new(p, &masks)).collect();
    let pairs: Vec<(usize, usize)> = (0..n).flat_map(|i| (i + 1..n).map(move |j| (i, j))).collect();
    let same: Vec<bool> = pairs
        .par_iter()
        .map(|(i, j)| track::align_unknown(&tmpls[*i], &pyrs[*j], &masks).map(|r| r.zncc >= 0.45).unwrap_or(false))
        .collect();
    let mut group: Vec<usize> = (0..n).collect();
    fn find(g: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while g[r] != r {
            r = g[r];
        }
        g[i] = r;
        r
    }
    for (k, (i, j)) in pairs.iter().enumerate() {
        if same[k] {
            let (a, b) = (find(&mut group, *i), find(&mut group, *j));
            let (lo, hi) = (a.min(b), a.max(b));
            group[hi] = lo;
        }
    }
    (0..n).map(|i| find(&mut group, i)).collect()
}

/// Run every stage on decoded frames (and audio, if any).
pub fn analyze_frames(frames: &Frames, audio: Option<(&[f32], u32)>, mut source: Source, opts: &AnalyzeOpts) -> Analyzed {
    let fps = frames.fps;
    source.analysis_width = frames.w;
    source.analysis_height = frames.h;
    source.frames = frames.len();
    let sigs = signals::signatures(frames);
    let thumbs = signals::thumbnails(frames, shots::THUMB);
    let mut ing = ingest::ingest(&thumbs, &sigs, fps);
    if opts.whole {
        ing.analysed_f0 = ing.content_f0;
        ing.analysed_f1 = ing.content_f1;
        ing.duplicates = ingest::duplicates(&sigs, ing.content_f0, ing.content_f1, fps);
    }
    let (a0, a1) = (ing.analysed_f0, ing.analysed_f1);
    let mut notes: Vec<String> = Vec::new();

    let prepared = shots::prepare(&thumbs);
    let sims = shots::pair_sims(&prepared);
    let lum: Vec<f32> = sigs.iter().map(|s| s.lum).collect();
    let cuts = if a1 > a0 { shots::detect_cuts(&prepared, &sims, &lum, a0, a1) } else { vec![] };
    let toggles = shots::invert_toggles(&sims);
    let cut_frames: Vec<usize> = cuts.iter().map(|c| c.frame).collect();
    let beats = beat_grid(audio, &sigs, fps, opts, a0, &cut_frames);
    if let Some(l) = &mut ing.loop_info {
        let b = l.period_frames.value / beats.frames_per_beat;
        let whole = (b - b.round()).abs() < 0.15;
        l.period_beats = Some(Est::measured(if whole { b.round() } else { b }, "beats", if whole { beats.bpm.confidence } else { 0.3 * beats.bpm.confidence }));
        notes.push(format!(
            "the edit loops: {} frames ({:.2} beats) from frame {}, played {:.2} times; shots are analysed for the first period only",
            l.period_frames.value, b, l.first_frame, l.repeats
        ));
    }
    if let Some(s) = &ing.duplicates.spacing_frames {
        notes.push(format!(
            "every ~{:.1} frames one frame repeats its predecessor (frame-rate conversion from about {:.1} fps?); those frames are left out of curve fits and can put events one frame off the beat grid",
            s.value,
            ing.duplicates.source_fps_guess.as_ref().map(|e| e.value).unwrap_or(0.0)
        ));
    }

    // Static layers.
    let content: Vec<usize> = (a0..a1).filter(|i| !ingest::is_blank(&sigs[*i])).collect();
    let stride = (content.len() / 90).max(1);
    let sample: Vec<usize> = content.iter().copied().step_by(stride).collect();
    let n_shots = cuts.len() + 1;
    let (plate, overlay) = if sample.len() >= 12 && n_shots >= 3 {
        let p = layers::overlay_plate(frames, &sample);
        let info = layers::overlay_info(&p);
        (p, info)
    } else {
        if a1 > a0 {
            notes.push("too few shots to separate a static overlay from the pictures".into());
        }
        (Plate { w: frames.w, h: frames.h, rgb: vec![0; frames.w * frames.h * 3] }, None)
    };
    let usable = if overlay.is_some() { layers::usable_mask(&plate) } else { vec![true; frames.w * frames.h] };
    let vignette = if a1 > a0 { layers::vignette(frames, &sigs, a0, a1) } else { None };

    // Shots.
    let mut bounds = vec![a0];
    bounds.extend(cut_frames.iter().copied());
    bounds.push(a1);
    let ranges: Vec<(usize, usize)> = bounds.windows(2).map(|w| (w[0], w[1])).filter(|(a, b)| b > a).collect();
    let work: Vec<ShotWork> = ranges.par_iter().map(|(f0, f1)| process_shot(frames, &sigs, &toggles, &usable, *f0, *f1)).collect();

    let mut dup = vec![false; frames.len()];
    for d in &ing.duplicates.frames {
        dup[*d] = true;
    }
    let timing = Timing { beats: &beats, fps };
    let global_sat = median(&(a0..a1).map(|i| sigs[i].sat as f64).collect::<Vec<_>>());
    let mut shots_out: Vec<Shot> = work
        .iter()
        .enumerate()
        .map(|(i, w)| build_shot(i + 1, w, &sigs, &dup, &timing, if i == 0 { None } else { cuts.get(i - 1) }, global_sat, frames.h, opts))
        .collect();

    // Slots.
    let refs: Vec<usize> = work.iter().map(|w| w.ref_frame).collect();
    let groups = cluster_slots(frames, &refs, &usable);
    let mut slots: Vec<SlotInfo> = Vec::new();
    let mut group_slot: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, g) in groups.iter().enumerate() {
        let si = *group_slot.entry(*g).or_insert_with(|| {
            slots.push(SlotInfo { id: format!("p{:02}", slots.len() + 1), shots: vec![], ref_frame: refs[i], proxy: None });
            slots.len() - 1
        });
        slots[si].shots.push(i + 1);
        shots_out[i].slot = slots[si].id.clone();
    }

    // Signature curves.
    let mut sig = Signature { f0: a0, ..Default::default() };
    for w in &work {
        for (k, m) in w.meas.iter().enumerate() {
            let i = w.f0 + k;
            sig.lum.push(round_to(sigs[i].lum as f64, 1));
            sig.stops.push(round_to(m.exp.stops, 3));
            sig.blur.push(round_to(100.0 * m.blur.sigma_px / frames.h as f64, 3));
            sig.sat.push(round_to(m.tone.sat_ratio, 3));
            sig.scale.push(if w.tracked && w.track[k].valid() { Some(round_to(w.track[k].sim.scale(), 4)) } else { None });
        }
    }
    let agent = shots_out.iter().filter(|s| s.status == Ident::NeedsAgent).count();
    if agent > 0 {
        notes.push(format!("{agent} of {} shots need an agent: their motion is not a 2D similarity of a full-frame picture (see each shot's notes)", shots_out.len()));
    }
    let analysis = Analysis {
        schema: SCHEMA.into(),
        tool: format!("edits-analyze {}", env!("CARGO_PKG_VERSION")),
        source,
        ingest: ing,
        beats,
        shots: shots_out,
        slots,
        layers: Layers { overlay, vignette },
        signature: sig,
        notes,
    };
    Analyzed { analysis, plate, sigs }
}

/// Decode a video and analyse it. With `out_dir`, also writes
/// `reference_analysis.json`, `overlay.png` and `proxies/<slot>.png` there.
pub fn analyze_video(path: &Path, out_dir: Option<&Path>, opts: &AnalyzeOpts) -> Result<Analysis> {
    let info = media::probe(path)?;
    let (frames, audio) = rayon::join(|| media::decode_frames(path, &info, opts.height), || media::decode_audio(path, &info, 22050));
    let frames = frames?;
    let audio = audio.unwrap_or(None);
    let abs = std::fs::canonicalize(path).unwrap_or(path.to_path_buf());
    let source = Source {
        path: abs.to_string_lossy().into_owned(),
        width: info.width,
        height: info.height,
        fps: info.fps,
        frames: frames.len(),
        duration_seconds: round_to(frames.len() as f64 / info.fps, 4),
        has_audio: audio.is_some(),
        analysis_width: frames.w,
        analysis_height: frames.h,
    };
    let done = analyze_frames(&frames, audio.as_deref().map(|a| (a, 22050)), source, opts);
    let mut analysis = done.analysis;
    if let Some(dir) = out_dir {
        std::fs::create_dir_all(dir.join("proxies")).with_context(|| format!("creating {}", dir.display()))?;
        // Full-resolution frames: one proxy still per slot, plus a sample for the overlay plate.
        let (a0, a1) = (analysis.ingest.analysed_f0, analysis.ingest.analysed_f1);
        let mut wanted: Vec<usize> = analysis.slots.iter().map(|s| s.ref_frame).collect();
        let content: Vec<usize> = (a0..a1).filter(|i| !ingest::is_blank(&done.sigs[*i])).collect();
        let stride = (content.len() / 40).max(1);
        let plate_frames: Vec<usize> = if analysis.layers.overlay.is_some() { content.iter().copied().step_by(stride).collect() } else { vec![] };
        wanted.extend(plate_frames.iter().copied());
        wanted.sort_unstable();
        wanted.dedup();
        let full = media::decode_selected(path, &info, &wanted)?;
        if full.len() == wanted.len() {
            let pos = |f: usize| wanted.binary_search(&f).unwrap();
            for s in analysis.slots.iter_mut() {
                let rel = format!("proxies/{}.png", s.id);
                save_rgb(&dir.join(&rel), full.w, full.h, full.rgb(pos(s.ref_frame)))?;
                s.proxy = Some(rel);
            }
            if let Some(o) = analysis.layers.overlay.as_mut() {
                let idx: Vec<usize> = plate_frames.iter().map(|f| pos(*f)).collect();
                let plate = layers::overlay_plate(&full, &idx);
                save_rgb(&dir.join("overlay.png"), plate.w, plate.h, &plate.rgb)?;
                o.path = Some("overlay.png".into());
            }
        } else {
            analysis.notes.push("full-resolution frames could not be extracted; proxies and overlay were written at analysis resolution".into());
            for s in analysis.slots.iter_mut() {
                let rel = format!("proxies/{}.png", s.id);
                save_rgb(&dir.join(&rel), frames.w, frames.h, frames.rgb(s.ref_frame))?;
                s.proxy = Some(rel);
            }
            if let Some(o) = analysis.layers.overlay.as_mut() {
                save_rgb(&dir.join("overlay.png"), done.plate.w, done.plate.h, &done.plate.rgb)?;
                o.path = Some("overlay.png".into());
            }
        }
        std::fs::write(dir.join("reference_analysis.json"), crate::jsonfmt::to_string(&analysis)?)?;
    }
    Ok(analysis)
}

/// Read a `reference_analysis.json`.
pub fn load(path: &Path) -> Result<Analysis> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let a: Analysis = serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    anyhow::ensure!(a.schema == SCHEMA, "{} is not an {} document (schema \"{}\")", path.display(), SCHEMA, a.schema);
    Ok(a)
}

/// A short human-readable account of an analysis.
pub fn summary(a: &Analysis) -> String {
    let mut s = String::new();
    let src = &a.source;
    s += &format!("{}x{} {} fps, {} frames ({:.2} s), audio: {}\n", src.width, src.height, src.fps, src.frames, src.duration_seconds, src.has_audio);
    s += &format!(
        "tempo {:.2} bpm (confidence {:.2}, {}), beat 0 at {:.3} s, {:.2} frames per beat\n",
        a.beats.bpm.value, a.beats.bpm.confidence, a.beats.source, a.beats.offset_seconds.value, a.beats.frames_per_beat
    );
    let ing = &a.ingest;
    s += &format!("content frames {}..{}, analysed {}..{}", ing.content_f0, ing.content_f1, ing.analysed_f0, ing.analysed_f1);
    if let Some(l) = &ing.loop_info {
        s += &format!(", loop of {} frames ({} beats) x{:.2}", l.period_frames.value, l.period_beats.as_ref().map(|b| b.value).unwrap_or(0.0), l.repeats);
    }
    s += &format!(", {} stalled frames\n", ing.duplicates.frames.len());
    for r in &ing.blank_runs {
        s += &format!("  blank {}..{} {} (luminance {} -> {})\n", r.f0, r.f1, r.kind, r.lum_start, r.lum_end);
    }
    s += &format!("{} shots, {} slots", a.shots.len(), a.slots.len());
    if let Some(o) = &a.layers.overlay {
        s += &format!(", overlay covering {:.1}% of the frame", 100.0 * o.coverage.value);
    }
    if let Some(v) = &a.layers.vignette {
        s += &format!(", vignette amount {:.2} radius {:.2} softness {:.2}", v.amount.value, v.radius.value, v.softness.value);
    }
    s += "\n";
    for sh in &a.shots {
        let ev: Vec<String> = sh
            .events
            .iter()
            .map(|e| match e.kind.as_str() {
                "flash" => format!("flash@{} {:.1}st", e.frame, e.params["peak"].value),
                k => format!("{k}@{}-{}", e.f0, e.f1),
            })
            .collect();
        let cv: Vec<String> = sh.curves.iter().filter(|c| !c.prop.contains('.') || c.prop.starts_with("gaussian")).map(|c| format!("{}[{}]", c.prop, c.keys.len())).collect();
        s += &format!(
            "  shot {:2} frames {:4}..{:4} beats {:6.2}..{:6.2} cut {:+.1}f {:10} ref {:4} slot {} {:11} track {:.2} | {} | {}\n",
            sh.id,
            sh.f0,
            sh.f1,
            sh.beat0,
            sh.beat1,
            sh.cut.offset_frames.value,
            sh.cut.grid,
            sh.ref_frame,
            sh.slot,
            if sh.status == Ident::Measured { "measured" } else { "needs_agent" },
            sh.track.valid_fraction,
            ev.join(" "),
            cv.join(" ")
        );
    }
    s
}
