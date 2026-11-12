//! Blur under a flash that clips.
//!
//! The renderer blurs a layer in linear light, then multiplies it by the
//! exposure gain, and the output clips at white. Where the gain clips, the
//! two cannot be measured one after the other: a blurred picture has other
//! tones than the sharp reference frame (ink mixes with paper, so the darks
//! that carry the gain reading are lifted), and clipping re-sharpens blurred
//! edges, which hides the blur from the power spectrum. `edits calib` showed
//! both: blur read 60-80% low and exposure 2-3 stops high on poster and line
//! art flashed and blurred on the same cut.
//!
//! So the chain is searched instead: for a candidate sigma the reference
//! frame is blurred as the renderer would, and the tone transfer from it to
//! the frame is read off matched quantiles. For the right sigma that transfer
//! is a pure gain with a clip (`y = min(1, g x)`); the sigma whose transfer
//! is closest to that shape is the estimate, and its gain is the exposure.
//! Histograms need no pixel correspondence, so this also works on the frames
//! of a flash peak, where registration is at its worst.

use crate::exposure::{self, ExposureSample, LumHist};
use crate::img::{gaussian_blur, srgb_lut, Gray};
use crate::measure::Pair;

/// Height in pixels of the reduced copy of the reference frame the search runs on.
const WORK: usize = 180;

pub struct Joint {
    /// Gaussian sigma in pixels of the full-size frame.
    pub sigma_px: f64,
    /// Exposure against the reference frame blurred by that sigma.
    pub exposure: ExposureSample,
    /// Mean distance of the tone transfer from a gain with a clip, in display values (0 = exact).
    pub mismatch: f64,
    /// The same for no blur at all.
    pub mismatch_sharp: f64,
}

/// Reference frame reduced by an integer factor: linear-light channels (mean
/// over the valid pixels of each block) and the mask of blocks to use.
struct Reduced {
    w: usize,
    h: usize,
    step: usize,
    ch: [Gray; 3],
    mask: Vec<bool>,
}

fn reduce(pair: &Pair) -> Reduced {
    let lut = srgb_lut();
    let step = (pair.h / WORK).max(1);
    let (w, h) = (pair.w / step, pair.h / step);
    let mut ch = [Gray::new(w, h), Gray::new(w, h), Gray::new(w, h)];
    let mut mask = vec![false; w * h];
    let mut holes: Vec<usize> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let (mut acc, mut k, mut used) = ([0.0f32; 3], 0.0f32, 0usize);
            for yy in y * step..(y + 1) * step {
                for xx in x * step..(x + 1) * step {
                    let i = yy * pair.w + xx;
                    if pair.ref_ok[i] {
                        for c in 0..3 {
                            acc[c] += lut[pair.refw[i * 3 + c] as usize];
                        }
                        k += 1.0;
                    }
                    used += pair.mask[i] as usize;
                }
            }
            let o = y * w + x;
            if k > 0.0 {
                for c in 0..3 {
                    ch[c].d[o] = acc[c] / k;
                }
            } else {
                holes.push(o);
            }
            mask[o] = used * 2 > step * step;
        }
    }
    // Blocks without a valid pixel (overlay strokes, uncovered parts) take a neighbour's value.
    for o in holes {
        let (x, y) = (o % w, o / w);
        let mut found = None;
        'search: for r in 1..6usize {
            for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                    if mask[yy * w + xx] {
                        found = Some(yy * w + xx);
                        break 'search;
                    }
                }
            }
        }
        if let Some(n) = found {
            for c in ch.iter_mut() {
                c.d[o] = c.d[n];
            }
        }
    }
    Reduced { w, h, step, ch, mask }
}

fn encode(v: f64) -> f64 {
    v.clamp(0.0, 1.0).powf(1.0 / 2.2)
}

/// Distance of the tone transfer `x -> y` from `y = min(1, g x)`.
fn transfer_mismatch(x: &[f64], y: &[f64], gain: f64) -> f64 {
    let n = x.len().max(1) as f64;
    x.iter().zip(y).map(|(a, b)| (encode(*b) - encode((gain * a).min(1.0))).abs()).sum::<f64>() / n
}

/// Search the blur. `sat` is the frame's saturation relative to the reference
/// frame, `start_px` the estimate the power spectrum gave (it is one of the
/// candidates). `None` when too little of the frame is measurable.
pub fn blur_under_flash(pair: &Pair, inverted: bool, sat: f64, start_px: f64) -> Option<Joint> {
    let red = reduce(pair);
    let hc = LumHist::value(pair.cur, &pair.mask, inverted, pair.flat, 1.0, true);
    if hc.count < 200 || red.mask.iter().filter(|m| **m).count() < 100 {
        return None;
    }
    let ps = exposure::quantile_grid();
    let y = hc.quantiles(&ps);
    let eval = |sigma_px: f64| -> (f64, ExposureSample) {
        let s = (sigma_px / red.step as f64) as f32;
        let b: Vec<Gray> = red.ch.iter().map(|c| gaussian_blur(c, s)).collect();
        let hr = LumHist::from_linear(&b[0].d, &b[1].d, &b[2].d, &red.mask, sat as f32);
        let x = hr.quantiles(&ps);
        let e = exposure::measure_quantiles(&x, &y);
        (transfer_mismatch(&x, &y, 2f64.powf(e.stops)), e)
    };
    let _ = (red.w, red.h);
    let h = pair.h as f64;
    // Coarse candidates in % of height, then two rounds of halving around the best.
    let mut cands: Vec<f64> = [0.0, 0.3, 0.6, 1.0, 1.5, 2.0, 2.7, 3.5].iter().map(|p| p * h / 100.0).collect();
    cands.push(start_px);
    cands.sort_by(|a, b| a.total_cmp(b));
    let mut scored: Vec<(f64, f64, ExposureSample)> = cands.iter().map(|s| (*s, eval(*s))).map(|(s, (m, e))| (s, m, e)).collect();
    let sharp = scored[0].1;
    for _ in 0..3 {
        let best = (0..scored.len()).min_by(|a, b| scored[*a].1.total_cmp(&scored[*b].1)).unwrap();
        let mut extra: Vec<f64> = Vec::new();
        if best > 0 {
            extra.push(0.5 * (scored[best].0 + scored[best - 1].0));
        }
        if best + 1 < scored.len() {
            extra.push(0.5 * (scored[best].0 + scored[best + 1].0));
        }
        for s in extra {
            let (m, e) = eval(s);
            scored.push((s, m, e));
        }
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    if std::env::var_os("EDITS_JOINT_DEBUG").is_some() {
        eprintln!("joint: {}", scored.iter().map(|(s, m, e)| format!("{:.2}%:{:.4}/{:+.2}{}", 100.0 * s / h, m, e.stops, if e.bound { "b" } else { "" })).collect::<Vec<_>>().join(" "));
    }
    let best = scored.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    Some(Joint { sigma_px: best.0, exposure: best.2, mismatch: best.1, mismatch_sharp: sharp })
}
