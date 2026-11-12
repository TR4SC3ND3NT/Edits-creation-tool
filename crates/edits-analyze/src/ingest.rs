//! Ingest: blank lead-in and tail, fades, duplicate frames, loop detection.

use crate::img::{median, resize_area, Gray};
use crate::schema::{BlankRun, Duplicates, Est, Ident, Ingest, LoopInfo};
use crate::signals::{znorm, FrameSig};

/// A frame with (almost) no spatial contrast: black, white or a solid colour.
pub fn is_blank(s: &FrameSig) -> bool {
    s.contrast < 2.0
}

pub fn blank_runs(sigs: &[FrameSig]) -> Vec<BlankRun> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < sigs.len() {
        if !is_blank(&sigs[i]) {
            i += 1;
            continue;
        }
        let f0 = i;
        while i < sigs.len() && is_blank(&sigs[i]) {
            i += 1;
        }
        let lum: Vec<f64> = sigs[f0..i].iter().map(|s| s.lum as f64).collect();
        let (lo, hi) = lum.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        let kind = if hi < 6.0 {
            "black"
        } else if lo > 245.0 {
            "white"
        } else if hi - lo > 20.0 {
            "fade"
        } else {
            "solid"
        };
        runs.push(BlankRun { f0, f1: i, kind: kind.into(), lum_start: lum[0].round(), lum_end: lum[lum.len() - 1].round() });
    }
    runs
}

/// Frames that repeat their predecessor while the frames around them move
/// (frame-rate conversion stalls or authored stutter), within `f0..f1`.
pub fn duplicates(sigs: &[FrameSig], f0: usize, f1: usize, fps: f64) -> Duplicates {
    let mut frames = Vec::new();
    for i in (f0 + 1)..f1 {
        let d = sigs[i].diff as f64;
        let lo = i.saturating_sub(4).max(f0 + 1);
        let hi = (i + 5).min(f1);
        let around: Vec<f64> = (lo..hi).filter(|j| *j != i).map(|j| sigs[j].diff as f64).collect();
        if around.len() < 3 {
            continue;
        }
        let m = median(&around);
        if m > 1.0 && d < 0.3 * m && d < 10.0 {
            frames.push(i);
        }
    }
    let gaps: Vec<f64> = frames.windows(2).map(|w| (w[1] - w[0]) as f64).collect();
    let (mut spacing, mut guess) = (None, None);
    if gaps.len() >= 4 {
        let g = median(&gaps);
        let near = gaps.iter().filter(|v| (**v - g).abs() <= 1.5 || ((**v / g).round() >= 1.0 && (**v - g * (**v / g).round()).abs() <= 1.5)).count();
        let conf = near as f64 / gaps.len() as f64;
        if g >= 2.0 && conf > 0.5 {
            // Mean spacing over the whole run is steadier than the median gap.
            let span = (frames[frames.len() - 1] - frames[0]) as f64;
            let periods: f64 = gaps.iter().map(|v| (v / g).round().max(1.0)).sum();
            let s = span / periods;
            spacing = Some(Est::measured(s, "frames", conf));
            guess = Some(Est::new(fps * (1.0 - 1.0 / s), "fps", 0.5 * conf, Ident::Qualitative));
        }
    }
    Duplicates { frames, spacing_frames: spacing, source_fps_guess: guess }
}

fn corr(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Find a repeating sequence: the lag at which frame descriptors match again.
pub fn detect_loop(thumbs: &[Gray], sigs: &[FrameSig], c0: usize, c1: usize, fps: f64) -> Option<LoopInfo> {
    let len = c1.saturating_sub(c0);
    let min_lag = ((1.5 * fps) as usize).max(12);
    if len < 2 * min_lag {
        return None;
    }
    let desc: Vec<Option<Vec<f32>>> = (c0..c1)
        .map(|i| if is_blank(&sigs[i]) { None } else { znorm(&resize_area(&thumbs[i], 12, 12)) })
        .collect();
    let max_lag = len / 2;
    let step = (len / 600).max(1);
    let mut scores = vec![0.0f64; max_lag + 1];
    for (lag, score) in scores.iter_mut().enumerate().skip(min_lag) {
        let (mut acc, mut k) = (0.0f64, 0usize);
        let mut i = 0;
        while i + lag < len {
            if let (Some(a), Some(b)) = (&desc[i], &desc[i + lag]) {
                acc += corr(a, b) as f64;
                k += 1;
            }
            i += step;
        }
        if k >= 8 {
            *score = acc / k as f64;
        }
    }
    let best = scores.iter().cloned().fold(0.0, f64::max);
    if best < 0.8 {
        return None;
    }
    // The fundamental: the shortest lag that is nearly as good as the best.
    let lag = (min_lag..=max_lag).find(|l| scores[*l] >= 0.97 * best && scores[*l] >= scores[l - 1] && (*l == max_lag || scores[*l] >= scores[l + 1]))?;
    // Extent of the repetition: frames whose descriptor matches the one a period later.
    let good: Vec<bool> = (0..len - lag)
        .map(|i| match (&desc[i], &desc[i + lag]) {
            (Some(a), Some(b)) => corr(a, b) > 0.6,
            _ => true,
        })
        .collect();
    let (mut best_run, mut run_start, mut cur) = ((0usize, 0usize), 0usize, 0usize);
    let mut misses = 0;
    for (i, g) in good.iter().enumerate() {
        if *g {
            if cur == 0 {
                run_start = i;
            }
            cur += 1;
            misses = 0;
        } else {
            misses += 1;
            if misses > 2 {
                cur = 0;
            } else if cur > 0 {
                cur += 1;
            }
        }
        if cur > best_run.1 {
            best_run = (run_start, cur);
        }
    }
    if best_run.1 < lag / 2 {
        return None;
    }
    let first = c0 + best_run.0;
    let repeats = (best_run.1 + lag) as f64 / lag as f64;
    // Sub-frame period from the parabola through the peak.
    let (l, c, r) = (scores[lag - 1], scores[lag], scores[(lag + 1).min(max_lag)]);
    let den = l - 2.0 * c + r;
    let frac = if den.abs() > 1e-9 { (0.5 * (l - r) / den).clamp(-0.5, 0.5) } else { 0.0 };
    let _ = frac; // the period is a whole number of frames by construction of the repeat
    Some(LoopInfo {
        first_frame: first,
        period_frames: Est::measured(lag as f64, "frames", scores[lag].clamp(0.0, 1.0)),
        period_beats: None,
        repeats: (repeats * 100.0).round() / 100.0,
        score: (scores[lag] * 1000.0).round() / 1000.0,
    })
}

/// Run the whole ingest stage.
pub fn ingest(thumbs: &[Gray], sigs: &[FrameSig], fps: f64) -> Ingest {
    let runs = blank_runs(sigs);
    let n = sigs.len();
    let c0 = (0..n).find(|i| !is_blank(&sigs[*i])).unwrap_or(0);
    let c1 = (0..n).rev().find(|i| !is_blank(&sigs[*i])).map(|i| i + 1).unwrap_or(n);
    let loop_info = detect_loop(thumbs, sigs, c0, c1, fps);
    let (a0, a1) = match &loop_info {
        Some(l) => (l.first_frame, (l.first_frame + l.period_frames.value as usize).min(c1)),
        None => (c0, c1),
    };
    Ingest {
        content_f0: c0,
        content_f1: c1,
        blank_runs: runs,
        duplicates: duplicates(sigs, a0, a1, fps),
        loop_info,
        analysed_f0: a0,
        analysed_f1: a1,
    }
}
