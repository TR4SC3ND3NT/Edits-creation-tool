//! The whole analysis: ingest -> beats -> shots -> layers -> reference frames
//! -> tracks -> exposure / tone / blur -> curves -> slots -> merge.

use crate::beats::{self, Onset, TempoOpts};
use crate::blur::{self, BlurSample};
use crate::blurkind;
use crate::look;
use crate::curves::{self, decay, named};
use crate::exposure::{self, ExposureSample};
use crate::img::{median, save_rgb, Frames, Sim};
use crate::ingest;
use crate::layers::{self, Plate};
use crate::measure::{linear_luma_image, make_pair, FlatField};
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
    /// Analyse exactly this frame range (`first..end`), overriding loop detection.
    pub range: Option<(usize, usize)>,
}

impl Default for AnalyzeOpts {
    fn default() -> Self {
        Self { height: 360, bpm: None, beat_offset: None, samples: false, whole: false, range: None }
    }
}

/// Result of [`analyze_frames`]: the document plus things worth keeping in memory.
pub struct Analyzed {
    pub analysis: Analysis,
    /// Overlay plate at analysis resolution.
    pub plate: Plate,
    pub sigs: Vec<FrameSig>,
    /// Pixels the measurements used: not under the overlay strokes or the static texture.
    pub usable: Vec<bool>,
}

struct ShotWork {
    card: Option<CardBox>,
    blur_kind: Option<BlurKind>,
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
    // Relative to the most colourful frames, not the median: when a
    // black-and-white hit covers half the shot or more, a median floor lets a
    // grey frame become the reference, and every coloured frame then reads
    // as 3-4x "saturation" (`edits calib`: a draft rendered with line art
    // put its reference frame inside its own saturation hit).
    let sat_floor = 0.6 * crate::img::percentile(&sat, 0.85);
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

/// The kind of a shot's blur from the gradient orientations it took away
/// (median over the blurred frames, see `blurkind`), and for a streak its
/// own parameter at the most blurred frame, by blurring the reference frame
/// the same way until it loses as much detail.
fn classify_blur(frames: &Frames, ref_frame: usize, meas: &[FrameMeasure], tracked: bool, usable: &[bool]) -> Option<BlurKind> {
    let h = frames.h as f64;
    let blurred: Vec<(usize, &FrameMeasure)> = meas.iter().enumerate().filter(|(_, m)| 100.0 * m.blur.sigma_px / h > 0.25 && m.blur_dir.is_some()).collect();
    let dirs: Vec<blurkind::BlurDir> = blurred.iter().filter_map(|(_, m)| m.blur_dir).collect();
    if dirs.is_empty() {
        return None;
    }
    let (kind, c, st) = blurkind::classify(&blurkind::median(&dirs))?;
    let c = round_to(c * if tracked { 1.0 } else { 0.6 } * (dirs.len() as f64 / 3.0).min(1.0), 2);
    let (mut peak, mut per_sigma) = (None, None);
    if kind != "gaussian" {
        // The most blurred frame that a flash has not clipped.
        let best = blurred.iter().filter(|(_, m)| m.exp.clipped < 0.1).max_by(|a, b| blurkind::stats(&a.1.blur_dir.unwrap()).loss.total_cmp(&blurkind::stats(&b.1.blur_dir.unwrap()).loss));
        if let Some((_, m)) = best {
            let reference = blur::fill_masked(&linear_luma_image(frames.rgb(ref_frame), frames.w, frames.h), usable);
            let loss = blurkind::stats(&m.blur_dir.unwrap()).loss;
            if let Some(p) = blurkind::magnitude(kind, st.angle, loss, &reference) {
                let sigma = 100.0 * m.blur.sigma_px / h;
                peak = Some(round_to(p, 3));
                per_sigma = (sigma > 0.05).then(|| round_to(p / sigma, 3));
            }
        }
    }
    Some(BlurKind {
        kind: kind.into(),
        confidence: c,
        angle: (kind == "directional").then(|| round_to(st.angle, 0)),
        axis_contrast: round_to(st.axis_contrast, 3),
        radial_tangential: round_to(st.rt, 3),
        frames: dirs.len(),
        peak,
        per_sigma,
    })
}

/// Where the picture sits in a frame that shows a card or a cube on a dark
/// background: the box holding the middle 96% of the pixels that stand
/// clearly above the background (the level of the frame's outer ring).
/// `None` when the picture fills the frame (or nothing stands out).
pub fn card_box(frames: &Frames, ref_frame: usize, usable: &[bool]) -> Option<CardBox> {
    let g = frames.gray(ref_frame);
    let (w, h) = (g.w, g.h);
    let m = (h as f64 * 0.05) as usize;
    let ring: Vec<f64> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|(x, y)| (*y < m || *y >= h - m || *x < m || *x >= w - m) && usable[y * w + x])
        .map(|(x, y)| g.d[y * w + x] as f64)
        .collect();
    if ring.len() < 50 {
        return None;
    }
    let bg = crate::img::percentile(&ring, 0.75);
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for y in 0..h {
        for x in 0..w {
            if usable[y * w + x] && g.d[y * w + x] as f64 > bg + 0.12 {
                xs.push(x as f64);
                ys.push(y as f64);
            }
        }
    }
    if xs.len() < (w * h) / 50 {
        return None;
    }
    let (x0, x1) = (crate::img::percentile(&xs, 0.02), crate::img::percentile(&xs, 0.98));
    let (y0, y1) = (crate::img::percentile(&ys, 0.02), crate::img::percentile(&ys, 0.98));
    let hh = h as f64;
    let (bw, bh) = ((x1 - x0) / hh, (y1 - y0) / hh);
    // A picture that fills (nearly) all of the frame is not a card.
    if bh > 0.9 && bw > 0.9 * w as f64 / hh {
        return None;
    }
    Some(CardBox {
        center: [round_to(((x0 + x1) / 2.0 - (w as f64 - 1.0) / 2.0) / hh, 3), round_to(((y0 + y1) / 2.0 - (hh - 1.0) / 2.0) / hh, 3)],
        size: [round_to(bw, 3), round_to(bh, 3)],
        background: round_to(bg, 3),
    })
}

/// Everything measured on one frame against its shot's reference frame.
#[derive(Clone, Copy, Debug)]
pub struct FrameMeasure {
    pub exp: ExposureSample,
    pub tone: ToneSample,
    pub blur: BlurSample,
    /// False where blur cannot be seen at all: a frame flashed to white, a threshold or inversion hit.
    pub blur_ok: bool,
    pub inverted: bool,
    /// Gradient-orientation log ratios against the reference frame, on blurred frames (see `blurkind`).
    pub blur_dir: Option<blurkind::BlurDir>,
    /// Duotone statistics of the frame itself (see `look`).
    pub duo: look::Duo,
}

/// Measure frame `i` against `ref_frame`. `sim` is the frame's track sample
/// (`None` for a shot that could not be tracked), `registered` says whether
/// that sample is a real registration (not interpolated), `inverted` is the
/// tone state suggested by frame-to-frame sign flips.
pub fn measure_frame(frames: &Frames, ref_frame: usize, i: usize, sim: Option<&Sim>, registered: bool, inverted: bool, usable: &[bool], flat: &FlatField) -> FrameMeasure {
    let pair = make_pair(frames, ref_frame, i, sim, usable).with_flat(flat);
    let mut inverted = inverted;
    if sim.is_some() && registered {
        let c = tone::lum_correlation(&pair);
        if c.abs() > 0.4 {
            inverted = c < 0.0;
        }
    }
    let sat_of = |t: &ToneSample| if t.sat_confidence > 0.3 { t.sat_ratio.min(1.0) } else { 1.0 };
    let mut tone = tone::measure(&pair, inverted);
    let mut exp = exposure::measure(&pair, inverted, sat_of(&tone));
    let mut blur = BlurSample::default();
    let mut blur_dir = None;
    if i != ref_frame {
        // Linear light, with overlay strokes filled in. The reference
        // frame is used in this frame's geometry when the warp covers
        // the analysis window (a zoom changes the spectrum by itself).
        let (fw, fh) = (frames.w, frames.h);
        let cur = blur::fill_masked(&linear_luma_image(frames.rgb(i), fw, fh), usable);
        // Whether the warp covers the analysis window; overlay strokes do not
        // count against it (they are filled in). Counting them, as this did
        // before, turned the warp and every refinement below off in any edit
        // with an overlay plate (`edits calib`: blur read 0.2% for a true
        // 1.35% on a punch-in under a flash).
        let warped = pair.aligned && blur::window_share(&pair.covered, fw, fh) > 0.9;
        let reference = if warped { blur::fill_masked(&linear_luma_image(&pair.refw, fw, fh), &pair.ref_ok) } else { blur::fill_masked(&linear_luma_image(frames.rgb(ref_frame), fw, fh), usable) };
        let (mut s2, first) = blur::measure_signed(&cur, &reference);
        blur = first;
        // Blur, exposure and saturation disturb each other's
        // measurement: a blurred picture has other tones (ink mixes
        // with paper), and a flash that clips re-sharpens blurred
        // edges. So when a frame is blurred or flashed, the reference
        // frame is put through the renderer's chain (blur in linear
        // light, then gain, then clipping) with the current estimates
        // and the estimates are refined against that.
        let floor = (0.0012 * fh as f64).powi(2);
        let measurable = warped && !inverted && tone.threshold.is_none();
        let mut searched = false;
        if measurable && (exp.clipped > 0.03 || exp.bound) {
            // The flash clips: search the blur whose tones explain the frame (see `joint`).
            if let Some(j) = crate::joint::blur_under_flash(&pair, false, sat_of(&tone), s2.max(0.0).sqrt()) {
                if j.mismatch < j.mismatch_sharp - 0.003 {
                    searched = true;
                    s2 = j.sigma_px * j.sigma_px;
                    blur = BlurSample { sigma_px: j.sigma_px, anisotropy: first.anisotropy, fit_rms: first.fit_rms };
                    let pb = pair.with_reference(pair.blurred_reference(j.sigma_px as f32));
                    let t2 = tone::measure(&pb, false);
                    (tone.sat_ratio, tone.sat_confidence, tone.sat_share, tone.sat_cur, tone.sat_ref) = (t2.sat_ratio, t2.sat_confidence, t2.sat_share, t2.sat_cur, t2.sat_ref);
                    exp = exposure::measure(&pb, false, sat_of(&tone));
                }
            }
        }
        // The power spectrum is the finer instrument once the gain is known:
        // it polishes the searched value, within bounds.
        let (lo, hi) = if searched { ((0.6 * blur.sigma_px).powi(2), (1.5 * blur.sigma_px + 0.0015 * fh as f64).powi(2)) } else { (0.0, f64::MAX) };
        if measurable && (searched || s2 > floor || exp.stops.abs() > 0.4) {
            s2 = s2.max(0.0);
            for _ in 0..4 {
                let pb = pair.with_reference(pair.blurred_reference(s2.sqrt() as f32));
                let t2 = tone::measure(&pb, false);
                (tone.sat_ratio, tone.sat_confidence, tone.sat_share, tone.sat_cur, tone.sat_ref) = (t2.sat_ratio, t2.sat_confidence, t2.sat_share, t2.sat_cur, t2.sat_ref);
                exp = exposure::measure(&pb, false, sat_of(&tone));
                if exp.bound {
                    break;
                }
                let synth = blur::fill_masked(&crate::measure::flashed_luma(&pb.refw, fw, fh, 2f32.powf(exp.stops as f32)), &pair.ref_ok);
                let (d2, b) = blur::measure_signed_at(&cur, &synth, s2.sqrt());
                let next = (s2 + d2).max(0.0).clamp(lo, hi);
                let moved = (next.sqrt() - s2.sqrt()).abs();
                s2 = next;
                blur = BlurSample { sigma_px: s2.sqrt(), anisotropy: first.anisotropy, fit_rms: b.fit_rms };
                if moved < 0.0002 * fh as f64 {
                    break;
                }
            }
        }
        // Which orientations the blur took away (blur kind, see `blurkind`).
        if blur.sigma_px > 0.0025 * fh as f64 && !inverted && tone.threshold.is_none() {
            blur_dir = Some(blurkind::ratios(&blurkind::orientation(&cur, None), &blurkind::orientation(&reference, None)));
        }
    }
    if tone.threshold.is_some() {
        exp = ExposureSample::default();
    }
    let blur_ok = !exp.bound && tone.threshold.is_none();
    if !blur_ok {
        blur = BlurSample::default();
    }
    // The histogram's comb finds posterisation that the level count misses (codec smear).
    if tone.posterize.is_none() && tone.threshold.is_none() {
        tone.posterize = look::posterized(pair.cur, &pair.refw, Some(&pair.mask));
    }
    let duo = look::duotone(frames.rgb(i), frames.w, frames.h, Some(usable));
    FrameMeasure { exp, tone, blur, blur_ok, inverted, blur_dir: if blur_ok { blur_dir } else { None }, duo }
}

fn process_shot(frames: &Frames, sigs: &[FrameSig], toggles: &[bool], usable: &[bool], flat: &FlatField, f0: usize, f1: usize) -> ShotWork {
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
    inv_abs[f0..f0 + n].copy_from_slice(&state);
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
    let tilted: Vec<&TrackSample> = track.iter().filter(|t| t.non_similarity()).collect();
    if tracked && tilted.len() >= 2 && tilted.len() * 4 >= valid {
        tracked = false;
        let worst = tilted.iter().map(|t| t.affine.0).fold(0.0, f64::max);
        reasons.push(format!(
            "{} of {valid} registered frames fit an affine transform clearly better than a similarity (scale factors differ by up to {:.0}%): the picture tilts in 3D or is stretched; rotation about X/Y not measured",
            tilted.len(),
            100.0 * worst
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
            measure_frame(frames, ref_frame, i, if tracked { Some(&ts.sim) } else { None }, ts.valid(), state[i - f0], usable, flat)
        })
        .collect();
    let mut meas = meas;
    if !tracked {
        // The reference frame does not show the same thing as the other frames
        // (the content turns or changes), so compare neighbours instead and
        // accumulate outwards from the reference frame.
        let step = |from: usize, to: usize| {
            let pair = make_pair(frames, from, to, None, usable).with_flat(flat);
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
    // Exposure is reported relative to the shot's least exposed frame: the
    // reference frame is the sharpest, not necessarily the darkest (a flash
    // that is still fading on it would turn up as negative exposure later).
    let mut normal: Vec<f64> = meas.iter().filter(|m| !m.inverted && m.tone.threshold.is_none() && !m.exp.bound && m.exp.confidence > 0.2).map(|m| m.exp.stops).collect();
    normal.sort_by(|a, b| a.total_cmp(b));
    if let Some(base) = normal.get(if normal.len() >= 8 { 1 } else { 0 }).copied().filter(|b| *b < -0.03) {
        for m in meas.iter_mut().filter(|m| m.tone.threshold.is_none()) {
            m.exp.stops -= base;
        }
    }
    let card = if tracked { None } else { card_box(frames, ref_frame, usable) };
    let blur_kind = classify_blur(frames, ref_frame, &meas, tracked, usable);
    ShotWork { f0, f1, ref_frame, track, tracked, reasons, meas, card, blur_kind }
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
        Event { kind: kind.into(), f0, f1, frame, beat: self.beat(frame), beat_snapped: self.snap(frame), params, colors: BTreeMap::new(), note }
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
    let mut blur_kind: Option<BlurKind> = None;
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
    let bound: Vec<bool> = w.meas.iter().map(|m| m.exp.bound).collect();
    let lum_c: Vec<f64> = (f0..f1).map(|i| sigs[i].lum_center as f64).collect();
    let lum_all: Vec<f64> = (f0..f1).map(|i| sigs[i].lum as f64).collect();
    let fl = exposure::flashes(&stops, &econf, &lum_all, &bound);
    let mut ekeys: Vec<CurveKey> = Vec::new();
    for (fi, f) in fl.iter().enumerate() {
        let mut params = BTreeMap::new();
        let peak_ident = if f.extrapolated || f.lower_bound.is_some() { Ident::Qualitative } else { Ident::Measured };
        params.insert("peak".into(), Est::new(f.peak_stops, "stops", f.confidence, peak_ident));
        if let Some(lb) = f.lower_bound {
            params.insert("lower_bound".into(), Est::measured(lb, "stops", 0.8));
        }
        params.insert("decay_rate".into(), Est::measured(f.rate, "fraction of the remainder per frame (`decay` ease from the peak frame to `frames` later)", f.rate_confidence));
        params.insert("frames".into(), Est::measured((f.end - f.peak) as f64, "frames from the peak back to 0", f.rate_confidence));
        params.insert("peak_luminance".into(), Est::measured(sigs[f0 + f.peak].lum_center as f64, "0..255 (centre of frame)", 0.95));
        // The same decay read directly off the luminance: share of the excess
        // over the settled level that is left after each frame.
        let last = f.end.min(n - 1);
        let settle = lum_c[last].min(lum_c[w.ref_frame - f0]);
        let ratios: Vec<f64> = (f.peak..last).filter(|i| lum_c[*i] - settle > 12.0).map(|i| (lum_c[i + 1] - settle) / (lum_c[i] - settle)).collect();
        if ratios.len() >= 2 {
            params.insert("luminance_excess_kept".into(), Est::measured(median(&ratios), "fraction kept per frame", ((ratios.len() as f64 - 1.0) / 4.0).min(0.8)));
        }
        let mut note = match (f.lower_bound, f.extrapolated) {
            (Some(lb), true) => Some(format!("the peak frame is clipped at white (at least {lb:.1} stops): the peak is extrapolated from the frames after it")),
            (Some(_), false) => Some("everything measurable is clipped at white: the peak is a lower bound".to_string()),
            _ => None,
        };
        if f.end >= n && f.peak + 2 >= n && f.start < f.peak {
            note = Some("rises into the cut: the attack of the next shot's flash".into());
        }
        events.push(t.event("flash", f0 + f.start, f0 + f.end.min(n), f0 + f.peak, params, note));
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
        // The fall ends where the fitted curve reaches 0, or is cut off by
        // the next flash or the end of the shot. A `decay` segment cut off
        // early and ended on the curve's value there is the same curve.
        let mut stop = match fl.get(fi + 1) {
            Some(g) => f.end.min(g.start.saturating_sub(1).max(f.peak)),
            None => f.end,
        };
        if stop >= n {
            stop = n - 1;
        }
        if stop > f.peak {
            let v = if stop == f.end { 0.0 } else { f.peak_stops * exposure::decay_curve(f.rate, (stop - f.peak) as f64, (f.end - f.peak) as f64) };
            ekeys.push(t.plain_key(f0 + stop, v, named("hold")));
        }
    }
    if !ekeys.is_empty() {
        let conf = crate::img::mean(&fl.iter().map(|f| f.confidence).collect::<Vec<_>>());
        let ident = if fl.iter().any(|f| f.lower_bound.is_some()) { Ident::Qualitative } else { Ident::Measured };
        curves_out.push(Curve { prop: "exposure.stops".into(), unit: "stops".into(), confidence: round_to(conf, 2), identifiability: ident, keys: ekeys });
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
    let bw: Vec<f64> = (0..n).map(|i| if dup[f0 + i] || !w.meas[i].blur_ok { 0.0 } else { 1.0 }).collect();
    let fb = curves::fit(&blur_pct, &bw, 0.08);
    let peak_blur = fb.keys.iter().map(|k| k.v[0]).fold(0.0, f64::max);
    if peak_blur >= 0.15 {
        let blurred: Vec<f64> = w.meas.iter().filter(|m| 100.0 * m.blur.sigma_px / frame_h as f64 > 0.3).map(|m| m.blur.anisotropy).collect();
        let aniso = median(&blurred);
        let clipped = crate::img::mean(&w.meas.iter().map(|m| m.exp.clipped).collect::<Vec<_>>());
        let mut conf = if w.tracked { 0.75 } else { 0.45 };
        if clipped > 0.3 {
            conf *= 0.7;
        }
        // The kind, from the gradient orientations the blur took away (see `process_shot`).
        blur_kind = w.blur_kind.clone();
        let classified = blur_kind.as_ref().map(|k| k.confidence >= 0.4).unwrap_or(false);
        let directional = !blurred.is_empty() && aniso > 0.45 && !classified;
        if directional {
            notes.push(format!("blur is anisotropic (index {aniso:.2}) but its kind could not be classified; radius is a Gaussian equivalent"));
        }
        if let Some(k) = blur_kind.as_ref().filter(|k| k.kind != "gaussian" && k.confidence >= 0.4) {
            notes.push(format!(
                "blur looks {}{} (confidence {:.2}); the radius is its Gaussian equivalent, draft converts it",
                k.kind,
                k.angle.map(|a| format!(" at {a:.0} degrees")).unwrap_or_default(),
                k.confidence
            ));
        }
        let mut keys: Vec<CurveKey> = fb.keys.iter().map(|k| t.key(f0, k, 3)).collect();
        keys.iter_mut().for_each(|k| k.v[0] = k.v[0].max(0.0));
        curves_out.push(Curve {
            prop: "gaussian_blur.radius".into(),
            unit: "% of frame height (sigma)".into(),
            confidence: round_to(conf, 2),
            identifiability: if directional { Ident::NeedsAgent } else if blur_kind.as_ref().map(|k| k.kind != "gaussian").unwrap_or(false) { Ident::Qualitative } else { Ident::Measured },
            keys,
        });
    }

    // ---- tone hits ----------------------------------------------------------------
    let trust = if w.tracked { 1.0 } else { 0.6 };
    // A flash (glow, clipping) washes colour out by itself: under one, only a
    // near-total loss of colour counts as a saturation hit.
    let low = |m: &FrameMeasure| m.tone.sat_ratio < if m.exp.stops > 1.0 { 0.1 } else { 0.5 };
    // A threshold leaves black and white only: that is the threshold, not a saturation hit.
    let mut bw_frames: Vec<bool> = w.meas.iter().map(|m| m.tone.threshold.is_none() && m.tone.sat_confidence > 0.5 && low(m)).collect();
    // A confident run extends over neighbours that agree but are hard to read (the flash peak).
    let weak = |m: &FrameMeasure| m.tone.threshold.is_none() && m.tone.sat_confidence > 0.1 && low(m);
    for i in 1..n {
        if bw_frames[i - 1] && !bw_frames[i] && weak(&w.meas[i]) {
            bw_frames[i] = true;
        }
    }
    for i in (0..n.saturating_sub(1)).rev() {
        if bw_frames[i + 1] && !bw_frames[i] && weak(&w.meas[i]) {
            bw_frames[i] = true;
        }
    }
    // A single grey frame on a flash peak (over 1.5 stops) is read as part of
    // the flash, not as a saturation hit: what survives a strong flash is a
    // few dark blocks whose colour a codec has smeared with the grey of their
    // clipped neighbours, and whether that happens depends on the picture,
    // so the same edit would report the hit on one picture and not on another.
    let sat_runs: Vec<(usize, usize, f64)> = tone::runs(&bw_frames)
        .into_iter()
        .filter(|(a, b)| !(b - a == 1 && w.meas[*a].exp.stops > 1.5))
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
    // Saturation that moves outside the hits (an edit that drains colour on a
    // flash and brings it back stronger as the flash decays) is drafted as a
    // fitted curve: the renderer's `saturation.amount` multiplies exactly the
    // ratio measured here, so the curve is the measurement. Without such
    // movement the hits are written as steps.
    let sat_ok: Vec<f64> = w.meas.iter().enumerate().map(|(i, m)| if m.tone.sat_share >= 0.06 && m.tone.threshold.is_none() && !m.inverted && !dup[f0 + i] { 1.0 } else { 0.0 }).collect();
    let in_hit = |i: usize| sat_runs.iter().any(|(a, b, _)| (*a..*b).contains(&i));
    let moving = (0..n).filter(|i| sat_ok[*i] > 0.0 && !in_hit(*i) && (w.meas[*i].tone.sat_ratio - 1.0).abs() > 0.2).count();
    if moving >= 2 && n >= 3 {
        let y: Vec<Vec<f64>> = w.meas.iter().map(|m| vec![m.tone.sat_ratio.clamp(0.0, 2.5)]).collect();
        let fsat = curves::fit(&y, &sat_ok, 0.04);
        let mut keys: Vec<CurveKey> = fsat.keys.iter().map(|k| t.key(f0, k, 3)).collect();
        keys.iter_mut().for_each(|k| k.v[0] = k.v[0].max(0.0));
        curves_out.push(Curve { prop: "saturation.amount".into(), unit: "x reference frame".into(), confidence: round_to(0.6 * trust, 2), identifiability: Ident::Measured, keys });
    } else if !sat_runs.is_empty() {
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
    // Duotone: a colour look that is a function of luminance (see `look`).
    let duo: Vec<bool> = w.meas.iter().map(|m| m.duo.is_duotone() && !m.inverted && m.tone.threshold.is_none()).collect();
    let ref_duo = duo[w.ref_frame - f0];
    for (a, b) in tone::runs(&duo) {
        if b - a < 2 && n > 2 {
            continue;
        }
        let med = |f: &dyn Fn(&look::Duo) -> f64| round_to(median(&(a..b).map(|i| f(&w.meas[i].duo)).collect::<Vec<_>>()), 3);
        let mut params = BTreeMap::new();
        let whole = ref_duo && a == 0 && b == n;
        params.insert("amount".into(), Est::new(1.0, "0..1", if whole { 0.4 } else { 0.7 * trust }, if whole { Ident::NeedsSource } else { Ident::Measured }));
        params.insert("spread".into(), Est::measured(med(&|d| d.spread), "chroma left within luminance bins (0 = pure duotone)", 0.6));
        let mut ev = t.event("duotone", f0 + a, f0 + b, f0 + a, params, whole.then(|| "the whole shot is duotone: it may be the picture itself; drafted as a gradient_map either way".to_string()));
        ev.colors.insert("shadows".into(), [med(&|d| d.shadows[0]), med(&|d| d.shadows[1]), med(&|d| d.shadows[2])]);
        ev.colors.insert("highlights".into(), [med(&|d| d.highlights[0]), med(&|d| d.highlights[1]), med(&|d| d.highlights[2])]);
        events.push(ev);
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
            anisotropy: w.track.iter().map(|s| round_to(s.affine.0, 3)).collect(),
            affine_gain: w.track.iter().map(|s| round_to(s.affine.1, 3)).collect(),
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
        blur_kind,
        card: w.card.clone(),
        samples,
        vision: None,
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
        .map(|(i, j)| track::align_unknown(&tmpls[*i], &pyrs[*j], &masks).map(|r| r.zncc >= 0.38).unwrap_or(false))
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

/// Stage timings on stderr when `EDITS_ANALYZE_TIMING` is set.
struct StageTimer {
    on: bool,
    last: std::time::Instant,
}
impl StageTimer {
    fn new() -> Self {
        Self { on: std::env::var_os("EDITS_ANALYZE_TIMING").is_some(), last: std::time::Instant::now() }
    }
    fn lap(&mut self, stage: &str) {
        if self.on {
            eprintln!("  {stage:<28} {:6.2} s", self.last.elapsed().as_secs_f64());
        }
        self.last = std::time::Instant::now();
    }
}

/// Run every stage on decoded frames (and audio, if any).
pub fn analyze_frames(frames: &Frames, audio: Option<(&[f32], u32)>, mut source: Source, opts: &AnalyzeOpts) -> Analyzed {
    let mut timer = StageTimer::new();
    let fps = frames.fps;
    source.analysis_width = frames.w;
    source.analysis_height = frames.h;
    source.frames = frames.len();
    let sigs = signals::signatures(frames);
    let thumbs = signals::thumbnails(frames, shots::THUMB);
    timer.lap("signatures, thumbnails");
    let mut ing = ingest::ingest(&thumbs, &sigs, fps);
    timer.lap("ingest (loop, duplicates)");
    if opts.whole {
        ing.analysed_f0 = ing.content_f0;
        ing.analysed_f1 = ing.content_f1;
        ing.duplicates = ingest::duplicates(&sigs, ing.content_f0, ing.content_f1, fps);
    }
    if let Some((r0, r1)) = opts.range {
        ing.analysed_f0 = r0.min(frames.len());
        ing.analysed_f1 = r1.clamp(ing.analysed_f0, frames.len());
        ing.duplicates = ingest::duplicates(&sigs, ing.analysed_f0, ing.analysed_f1, fps);
    }
    let (a0, a1) = (ing.analysed_f0, ing.analysed_f1);
    let mut notes: Vec<String> = Vec::new();

    let prepared = shots::prepare(&thumbs);
    let sims = shots::pair_sims(&prepared);
    let lum: Vec<f32> = sigs.iter().map(|s| s.lum).collect();
    let cuts = if a1 > a0 { shots::detect_cuts(&prepared, &sims, &lum, a0, a1) } else { vec![] };
    let toggles = shots::invert_toggles(&sims);
    let cut_frames: Vec<usize> = cuts.iter().map(|c| c.frame).collect();
    timer.lap("shots (cut detection)");
    let beats = beat_grid(audio, &sigs, fps, opts, a0, &cut_frames);
    timer.lap("beats");
    if let Some(l) = &mut ing.loop_info {
        let b = l.period_frames.value / beats.frames_per_beat;
        let whole = (b - b.round()).abs() < 0.15;
        l.period_beats = Some(Est::measured(if whole { b.round() } else { b }, "beats", if whole { beats.bpm.confidence } else { 0.3 * beats.bpm.confidence }));
        notes.push(format!(
            "the edit loops: {} frames ({:.2} beats) from frame {}, played {:.2} times{}",
            l.period_frames.value,
            b,
            l.first_frame,
            l.repeats,
            if opts.range.is_some() || opts.whole { "" } else { "; shots are analysed for the first period only" }
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
    let mut usable = if overlay.is_some() { layers::usable_mask(&plate) } else { vec![true; frames.w * frames.h] };
    let vignette = if a1 > a0 { layers::vignette(frames, &sigs, a0, a1) } else { None };

    timer.lap("layers (overlay, vignette)");
    // Shots.
    let mut bounds = vec![a0];
    bounds.extend(cut_frames.iter().copied());
    bounds.push(a1);
    let ranges: Vec<(usize, usize)> = bounds.windows(2).map(|w| (w[0], w[1])).filter(|(a, b)| b > a).collect();
    // A static texture: fine detail common to one frame of every shot (three
    // fifths in, past the cut's flash). Where it is, the pixels are left out of
    // the measurements like the overlay strokes: a sharp texture over a
    // blurred picture would hide the blur, in the reference and in a render alike.
    let groups: Vec<Vec<usize>> = ranges.iter().map(|(a, b)| a + (b - a) * 3 / 5).filter(|f| !ingest::is_blank(&sigs[*f])).map(|f| vec![f]).collect();
    let texture = layers::texture(frames, &groups, &usable).and_then(|t| {
        if timer.on {
            eprintln!("  texture presence {:.3}, strength {:.3} stops, coverage {:.2}, {} over {} pictures", t.presence, t.strength, t.coverage, t.blend, groups.len());
        }
        let info = layers::texture_info(&t)?;
        layers::mask_texture(&mut usable, frames.w, frames.h, &t.detail);
        Some(info)
    });
    timer.lap("texture");
    let flat = match &vignette {
        Some(v) => FlatField::from_profile(frames.w, frames.h, &v.profile),
        None => FlatField::flat(frames.w, frames.h),
    };
    let work: Vec<ShotWork> = ranges.par_iter().map(|(f0, f1)| process_shot(frames, &sigs, &toggles, &usable, &flat, *f0, *f1)).collect();

    timer.lap("track, exposure, tone, blur");
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

    timer.lap("curves, events");
    // Slots.
    let refs: Vec<usize> = work.iter().map(|w| w.ref_frame).collect();
    let groups = cluster_slots(frames, &refs, &usable);
    let mut slots: Vec<SlotInfo> = Vec::new();
    let mut group_slot: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, g) in groups.iter().enumerate() {
        let si = *group_slot.entry(*g).or_insert_with(|| {
            slots.push(SlotInfo { id: format!("p{:02}", slots.len() + 1), shots: vec![], ref_frame: refs[i], proxy: None, role: None });
            slots.len() - 1
        });
        slots[si].shots.push(i + 1);
        shots_out[i].slot = slots[si].id.clone();
    }

    timer.lap("slots");
    // Signature curves.
    let mut sig = Signature { f0: a0, ..Default::default() };
    for w in &work {
        for (k, m) in w.meas.iter().enumerate() {
            let i = w.f0 + k;
            sig.lum.push(round_to(sigs[i].lum as f64, 1));
            sig.stops.push(round_to(m.exp.stops, 3));
            sig.blur.push(round_to(100.0 * m.blur.sigma_px / frames.h as f64, 3));
            sig.sat.push(round_to(signature_sat(m), 3));
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
        layers: Layers { overlay, vignette, texture },
        signature: sig,
        notes,
        vision: None,
    };
    Analyzed { analysis, plate, sigs, usable }
}

/// Saturation as the signature records it: the neutral 1 where the
/// measurement rests on too few blocks to mean anything (under 6% of them) (under a flash that
/// clips most of the frame the ratio of a handful of dark blocks swings
/// between 0 and the clamp at 4, and `compare` would score that noise).
fn signature_sat(m: &FrameMeasure) -> f64 {
    if m.tone.sat_share < 0.06 {
        1.0
    } else {
        m.tone.sat_ratio
    }
}

/// What measuring one shot on its own needs from the rest of its clip: the
/// beat grid, the overlay mask and the vignette (both measured on the whole
/// clip), and the clip's median saturation.
#[derive(Clone, Debug)]
pub struct ShotContext {
    pub beats: Beats,
    /// Pixels not covered by the static overlay, at analysis size.
    pub usable: Vec<bool>,
    /// The vignette's radial profile, if one was measured.
    pub vignette: Option<Vec<f64>>,
    pub global_sat: f64,
}

impl ShotContext {
    /// The context [`analyze_frames`] used for `done`.
    pub fn of(done: &Analyzed) -> ShotContext {
        let a = &done.analysis;
        let usable = done.usable.clone();
        let (a0, a1) = (a.ingest.analysed_f0, a.ingest.analysed_f1.min(done.sigs.len()));
        let global_sat = if a1 > a0 { median(&(a0..a1).map(|i| done.sigs[i].sat as f64).collect::<Vec<_>>()) } else { 0.0 };
        ShotContext { beats: a.beats.clone(), usable, vignette: a.layers.vignette.as_ref().map(|v| v.profile.clone()), global_sat }
    }
}

/// Measure one shot without the rest of its clip, exactly as
/// [`analyze_frames`] measures it once the cuts are known: `frames` holds
/// frames `first..first + frames.len()` of the clip and the shot is `f0..f1`
/// (absolute frame numbers, `first <= f0 < f1`). Give it the frame before the
/// cut as well when there is one (`first = f0 - 1`): the choice of the
/// reference frame looks at the difference to the previous frame. Returns the
/// shot (numbered `id`) and its signature curves (`f0` = the shot's first frame).
pub fn measure_shot(frames: &Frames, first: usize, f0: usize, f1: usize, id: usize, ctx: &ShotContext, opts: &AnalyzeOpts) -> (Shot, Signature) {
    assert!(first <= f0 && f0 < f1 && f1 - first <= frames.len(), "measure_shot: frames {f0}..{f1} are not all in the {} frames from {first}", frames.len());
    let rel = signals::signatures(frames);
    let thumbs = signals::thumbnails(frames, shots::THUMB);
    let toggles = shots::invert_toggles(&shots::pair_sims(&shots::prepare(&thumbs)));
    let flat = match &ctx.vignette {
        Some(p) => FlatField::from_profile(frames.w, frames.h, p),
        None => FlatField::flat(frames.w, frames.h),
    };
    let usable: Vec<bool> = if ctx.usable.len() == frames.w * frames.h { ctx.usable.clone() } else { vec![true; frames.w * frames.h] };
    let mut w = process_shot(frames, &rel, &toggles, &usable, &flat, f0 - first, f1 - first);
    w.f0 += first;
    w.f1 += first;
    w.ref_frame += first;
    // Signatures indexed by absolute frame number.
    let mut sigs: Vec<FrameSig> = Vec::with_capacity(first + rel.len());
    sigs.extend(std::iter::repeat_n(rel[0].clone(), first));
    sigs.extend(rel.iter().cloned());
    let dup = vec![false; sigs.len()];
    let timing = Timing { beats: &ctx.beats, fps: frames.fps };
    let shot = build_shot(id, &w, &sigs, &dup, &timing, None, ctx.global_sat, frames.h, opts);
    let mut sig = Signature { f0, ..Default::default() };
    for (k, m) in w.meas.iter().enumerate() {
        sig.lum.push(round_to(sigs[f0 + k].lum as f64, 1));
        sig.stops.push(round_to(m.exp.stops, 3));
        sig.blur.push(round_to(100.0 * m.blur.sigma_px / frames.h as f64, 3));
        sig.sat.push(round_to(signature_sat(m), 3));
        sig.scale.push(if w.tracked && w.track[k].valid() { Some(round_to(w.track[k].sim.scale(), 4)) } else { None });
    }
    (shot, sig)
}

/// Decode a video and analyse it. With `out_dir`, also writes
/// `reference_analysis.json`, `overlay.png` and `proxies/<slot>.png` there.
pub fn analyze_video(path: &Path, out_dir: Option<&Path>, opts: &AnalyzeOpts) -> Result<Analysis> {
    let mut timer = StageTimer::new();
    let info = media::probe(path)?;
    let (frames, audio) = rayon::join(|| media::decode_frames(path, &info, opts.height), || media::decode_audio(path, &info, 22050));
    let frames = frames?;
    timer.lap("decode (ffmpeg)");
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
    timer.lap("analysis total");
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
            if let Some(t) = analysis.layers.texture.as_mut() {
                let groups: Vec<Vec<usize>> = analysis.slots.iter().map(|s| vec![pos(s.ref_frame)]).collect();
                if let Some(tex) = layers::texture(&full, &groups, &vec![true; full.w * full.h]) {
                    let plate = layers::texture_plate(full.w, full.h, &tex);
                    save_rgb(&dir.join("texture.png"), plate.w, plate.h, &plate.rgb)?;
                    t.path = Some("texture.png".into());
                }
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
        timer.lap("proxies, overlay, json");
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
    if let Some(t) = &a.layers.texture {
        s += &format!(", static texture (presence {:.2}, {:.3} stops)", t.presence.value, t.strength.value);
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
        let mut cv: Vec<String> = sh.curves.iter().filter(|c| !c.prop.contains('.') || c.prop.starts_with("gaussian")).map(|c| format!("{}[{}]", c.prop, c.keys.len())).collect();
        if let Some(k) = sh.blur_kind.as_ref().filter(|k| k.kind != "gaussian") {
            cv.push(format!("blur:{}{}", k.kind, k.angle.map(|a| format!("@{a:.0}")).unwrap_or_default()));
        }
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
