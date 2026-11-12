//! Exposure: per-frame gain relative to the shot's reference frame, and flash events.
//!
//! The gain is read from the tone transfer between the two frames, obtained
//! by matching quantiles of the *largest channel* in linear light (no pixel
//! correspondence needed, so a few pixels of misregistration under a flash do
//! not matter). The renderer multiplies linear RGB by `2^stops` and clips
//! each channel at 1 on output, so a pixel is clipped exactly when its
//! largest channel is: the quantiles below white are untouched by clipping
//! and their ratio is the gain. (With luminance instead, a saturated colour
//! whose one channel has clipped would still count as unclipped and pull the
//! gain down; `edits calib` measured -0.6 stops of bias on poster art.)
//! Quantiles that are clipped at white in the frame are left out, which is
//! why a flash can still be measured from the dark ink that survives it. A
//! saturation change moves the largest channel too, so the reference frame
//! is desaturated by the measured amount before its quantiles are taken.

use crate::img::srgb_lut;
use crate::measure::{FlatField, Pair};

const BINS: usize = 1024;

/// Histograms of linear-light luminance, binned on a gamma axis so dark tones keep resolution.
pub struct LumHist {
    pub hist: Vec<u32>,
    pub count: u32,
}

fn decode(bin: f64) -> f64 {
    ((bin + 0.5) / BINS as f64).powf(2.2)
}

impl LumHist {
    /// With `flat`, a pixel as bright as a white field is at its position
    /// (under a vignette that is less than 255) counts as white, i.e. clipped.
    /// Values are not divided by the vignette: the falloff measured on flash
    /// frames can be stronger than the one on ordinary frames.
    pub fn new(rgb: &[u8], mask: &[bool], invert: bool, flat: Option<&FlatField>) -> Self {
        let lut = srgb_lut();
        let mut hist = vec![0u32; BINS];
        let mut count = 0;
        for (i, p) in rgb.chunks_exact(3).enumerate() {
            if !mask[i] {
                continue;
            }
            let white = flat.map(|f| f.white[i]).unwrap_or(255);
            let v = |c: u8| lut[if invert { white.saturating_sub(c) } else { c } as usize];
            let mut y = 0.2126 * v(p[0]) + 0.7152 * v(p[1]) + 0.0722 * v(p[2]);
            if white < 250 && !invert && p[0].max(p[1]).max(p[2]) as f32 >= 0.96 * white as f32 {
                y = 1.0;
            }
            let b = ((y.powf(1.0 / 2.2) * BINS as f32) as usize).min(BINS - 1);
            hist[b] += 1;
            count += 1;
        }
        LumHist { hist, count }
    }
    /// Histogram of the largest channel (linear light) after applying a
    /// saturation of `sat` to each pixel, as the renderer's `saturation`
    /// effect does (mix with Rec. 709 luminance). `clip` marks pixels that
    /// are as bright as a white field at their position as white.
    pub fn value(rgb: &[u8], mask: &[bool], invert: bool, flat: Option<&FlatField>, sat: f32, clip: bool) -> Self {
        let lut = srgb_lut();
        let mut hist = vec![0u32; BINS];
        let mut count = 0;
        for (i, p) in rgb.chunks_exact(3).enumerate() {
            if !mask[i] {
                continue;
            }
            let white = flat.map(|f| f.white[i]).unwrap_or(255);
            let v = |c: u8| lut[if invert { white.saturating_sub(c) } else { c } as usize];
            let (r, g, b) = (v(p[0]), v(p[1]), v(p[2]));
            let mut y = r.max(g).max(b);
            if sat != 1.0 {
                let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                y = (l + sat * (y - l)).max(0.0);
            }
            if clip && white < 250 && !invert && p[0].max(p[1]).max(p[2]) as f32 >= 0.96 * white as f32 {
                y = 1.0;
            }
            let b = ((y.min(1.0).powf(1.0 / 2.2) * BINS as f32) as usize).min(BINS - 1);
            hist[b] += 1;
            count += 1;
        }
        LumHist { hist, count }
    }
    /// As [`LumHist::value`], from linear-light channel planes.
    pub fn from_linear(r: &[f32], g: &[f32], b: &[f32], mask: &[bool], sat: f32) -> Self {
        let mut hist = vec![0u32; BINS];
        let mut count = 0;
        for i in 0..r.len() {
            if !mask[i] {
                continue;
            }
            let mut y = r[i].max(g[i]).max(b[i]);
            if sat != 1.0 {
                let l = 0.2126 * r[i] + 0.7152 * g[i] + 0.0722 * b[i];
                y = (l + sat * (y - l)).max(0.0);
            }
            hist[((y.clamp(0.0, 1.0).powf(1.0 / 2.2) * BINS as f32) as usize).min(BINS - 1)] += 1;
            count += 1;
        }
        LumHist { hist, count }
    }
    /// Linear luminance at each of the given cumulative fractions (ascending).
    pub fn quantiles(&self, ps: &[f64]) -> Vec<f64> {
        let mut out = Vec::with_capacity(ps.len());
        let mut cum = 0u32;
        let mut b = 0usize;
        for p in ps {
            let target = (p * self.count as f64).ceil().max(1.0) as u32;
            while b < BINS - 1 && cum + self.hist[b] < target {
                cum += self.hist[b];
                b += 1;
            }
            // Position inside the bin.
            let frac = if self.hist[b] > 0 { (target - cum) as f64 / self.hist[b] as f64 } else { 0.5 };
            out.push(decode(b as f64 + frac - 0.5));
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExposureSample {
    /// log2 of the gain relative to the reference frame.
    pub stops: f64,
    pub confidence: f64,
    /// Share of the reference frame's tones below white that are at white in this frame.
    pub clipped: f64,
    /// A mix toward a colour explains the frame better than a gain: (amount, luminance of the colour).
    pub fade: Option<(f64, f64)>,
    /// Nothing measurable is left below white: `stops` is only a lower bound.
    pub bound: bool,
}

/// Cumulative fractions at which tones are compared: an even grid, plus a
/// few low ones so that a small share of dark ink that survives a strong
/// flash still gives several readings.
pub fn quantile_grid() -> Vec<f64> {
    [0.0025, 0.005, 0.01, 0.015].into_iter().chain((1..=48).map(|i| i as f64 / 49.0)).collect()
}

/// Gain of the frame relative to the (warped) reference frame. `sat` is the
/// frame's saturation relative to the reference frame (1 = unchanged).
pub fn measure(pair: &Pair, inverted: bool, sat: f64) -> ExposureSample {
    let hr = LumHist::value(&pair.refw, &pair.mask, false, pair.flat, sat.clamp(0.0, 2.0) as f32, false);
    let hc = LumHist::value(pair.cur, &pair.mask, inverted, pair.flat, 1.0, true);
    if hr.count < 200 {
        return ExposureSample::default();
    }
    let ps = quantile_grid();
    let (x, y) = (hr.quantiles(&ps), hc.quantiles(&ps));
    measure_quantiles(&x, &y)
}

/// Gain between two neighbouring frames (`pair.refw` is the other frame,
/// unwarped). Both may be clipped, so only tones below white in both count.
pub fn measure_between(pair: &Pair, invert_cur: bool) -> ExposureSample {
    let hr = LumHist::value(&pair.refw, &pair.mask, false, pair.flat, 1.0, true);
    let hc = LumHist::value(pair.cur, &pair.mask, invert_cur, pair.flat, 1.0, true);
    if hr.count < 200 {
        return ExposureSample::default();
    }
    let ps = quantile_grid();
    let (x, y) = (hr.quantiles(&ps), hc.quantiles(&ps));
    let keep: Vec<usize> = (0..x.len()).filter(|i| x[*i] < WHITE).collect();
    if keep.len() < 4 {
        return ExposureSample::default();
    }
    let mut e = measure_quantiles(&keep.iter().map(|i| x[*i]).collect::<Vec<_>>(), &keep.iter().map(|i| y[*i]).collect::<Vec<_>>());
    e.fade = None;
    if e.confidence <= 0.2 {
        // Nothing comparable: assume no change rather than a bound.
        e.stops = 0.0;
    }
    e
}

const WHITE: f64 = 0.72;

/// The same from matched quantiles of linear luminance (`x` reference, `y` frame).
pub fn measure_quantiles(x: &[f64], y: &[f64]) -> ExposureSample {
    const FLOOR_X: f64 = 0.004;
    const FLOOR_Y: f64 = 0.002;
    let n = x.len();
    // Share of the tones that are below white in the reference frame and at white in this one.
    let white_ref = x.iter().filter(|v| **v >= WHITE).count();
    let white_cur = y.iter().filter(|v| **v >= WHITE).count();
    let clipped = if white_ref < n { (white_cur.saturating_sub(white_ref)) as f64 / (n - white_ref) as f64 } else { 0.0 };
    let mut ratios: Vec<f64> = Vec::new();
    let (mut sx, mut sy, mut sxx, mut sxy, mut k) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        if x[i] > FLOOR_X && y[i] > FLOOR_Y && y[i] < WHITE && x[i] < 0.9 {
            ratios.push((y[i] / x[i]).ln());
            sx += x[i];
            sy += y[i];
            sxx += x[i] * x[i];
            sxy += x[i] * y[i];
            k += 1.0;
        }
    }
    if ratios.is_empty() {
        // Everything measurable is clipped: only a lower bound is available,
        // the gain that takes the darkest tone seen at white up to white.
        let dark = (0..n).filter(|i| x[*i] > FLOOR_X && y[*i] >= WHITE).map(|i| x[i]).fold(f64::MAX, f64::min);
        let bound = if clipped > 0.5 && dark < f64::MAX { (WHITE / dark).max(1.0) } else { 1.0 };
        return ExposureSample { stops: bound.log2(), confidence: if bound > 1.0 { 0.2 } else { 0.0 }, clipped, fade: None, bound: bound > 1.0 };
    }
    if ratios.len() < 3 {
        // One or two tones survive: a reading, but a weak one.
        let stops = crate::img::median(&ratios) / std::f64::consts::LN_2;
        return ExposureSample { stops, confidence: 0.15 * ratios.len() as f64, clipped, fade: None, bound: false };
    }
    let med = crate::img::median(&ratios);
    let spread = crate::img::median(&ratios.iter().map(|r| (r - med).abs()).collect::<Vec<_>>());
    let stops = med / std::f64::consts::LN_2;
    // A mix toward a colour is affine with a positive offset: y = (1 - amount) x + amount * c.
    let den = k * sxx - sx * sx;
    let mut fade = None;
    if den > 1e-9 && ratios.len() >= 8 {
        let a = (k * sxy - sx * sy) / den;
        let b = (sy - a * sx) / k;
        if b > 0.1 && a < 0.75 && a > -0.05 {
            let amount = (1.0 - a).clamp(0.0, 1.0);
            fade = Some((amount, (b / amount.max(1e-6)).clamp(0.0, 1.0)));
        }
    }
    let confidence = ((ratios.len() as f64 / 16.0).min(1.0) * (1.0 - (spread / 0.5).min(1.0)) * if fade.is_some() { 0.5 } else { 1.0 }).clamp(0.05, 1.0);
    ExposureSample { stops, confidence, clipped, fade, bound: false }
}

#[derive(Clone, Debug)]
pub struct Flash {
    /// First frame above the floor, the peak frame and the frame at which the
    /// flash is back at 0 (indices into the input; `end` can lie past the
    /// last sample when the shot is cut before the flash is over).
    pub start: usize,
    pub peak: usize,
    pub end: usize,
    /// Exposure at the peak frame: the value of the fitted curve there. When
    /// the peak frame is clipped this is extrapolated from the frames after
    /// it and is never below `lower_bound`.
    pub peak_stops: f64,
    /// What was measured on the peak frame itself.
    pub measured: f64,
    /// Set when the peak frame only gave a lower bound (everything measurable at white).
    pub lower_bound: Option<f64>,
    /// The peak comes from the fitted curve, not from the peak frame.
    pub extrapolated: bool,
    /// Rate of the renderer's `decay` ease from the peak frame to `end`: the
    /// curve is `peak * (k^j - k^N) / (1 - k^N)` with `k = 1 - rate`, `j`
    /// frames after the peak and `N = end - peak`.
    pub rate: f64,
    pub rate_confidence: f64,
    pub confidence: f64,
    /// RMS residual of the fitted curve over the frames used, in stops.
    pub fit_rms: f64,
}

/// The renderer's `decay` ease, falling from 1 at `j = 0` to 0 at `j = n`.
pub fn decay_curve(rate: f64, j: f64, n: f64) -> f64 {
    let k = (1.0 - rate).clamp(1e-4, 0.9999);
    let end = 1.0 - k.powf(n.max(1.0));
    if j >= n {
        0.0
    } else if end <= 1e-9 {
        1.0 - j / n
    } else {
        (k.powf(j) - k.powf(n)) / end
    }
}

/// Least-squares fit of `peak * decay_curve(rate, j, n)` to `s[j]` (weights
/// `w`). With `pin`, the peak is fixed. Returns (peak, rate, sum of squares).
fn fit_decay(s: &[f64], w: &[f64], n: f64, pin: Option<f64>) -> (f64, f64, f64) {
    let mut best = (pin.unwrap_or(s[0]), 0.3, f64::MAX);
    for step in 2..=96 {
        let rate = step as f64 / 100.0;
        let phi: Vec<f64> = (0..s.len()).map(|j| decay_curve(rate, j as f64, n)).collect();
        let peak = match pin {
            Some(p) => p,
            None => {
                let (num, den) = (0..s.len()).fold((0.0, 0.0), |a, j| (a.0 + w[j] * s[j] * phi[j], a.1 + w[j] * phi[j] * phi[j]));
                if den > 1e-9 {
                    num / den
                } else {
                    s[0]
                }
            }
        };
        let sse: f64 = (0..s.len()).map(|j| w[j] * (s[j] - peak * phi[j]).powi(2)).sum();
        if sse < best.2 {
            best = (peak, rate, sse);
        }
    }
    best
}

/// Flash events in a shot's exposure curve (`stops[i]` relative to the
/// shot's least exposed frame). `lum[i]` (any scale) settles which frame is
/// the peak when several are within 15% of the maximum: under clipping the
/// measured gain is only a lower bound, the brightest frame is the peak.
/// `bound[i]` marks frames whose value is such a lower bound.
///
/// The fall of each flash is fitted with the curve the renderer draws for a
/// `decay` ease that ends on 0 (see [`Flash::rate`]); frames that only gave a
/// lower bound are left out of the fit, so a clipped peak is extrapolated
/// from the frames in which the flash is measurable again.
pub fn flashes(stops: &[f64], conf: &[f64], lum: &[f64], bound: &[bool]) -> Vec<Flash> {
    const PEAK: f64 = 0.45;
    const FLOOR: f64 = 0.1;
    let n = stops.len();
    let mut out: Vec<Flash> = Vec::new();
    let mut next = 0;
    while next < n {
        let i = next;
        // A local maximum that stands out.
        let is_peak = stops[i] >= PEAK && (i == 0 || stops[i] > stops[i - 1]) && (i + 1 == n || stops[i] >= stops[i + 1]);
        if !is_peak {
            next += 1;
            continue;
        }
        let mut start = i;
        while start > 0 && stops[start - 1] > FLOOR && stops[start - 1] < stops[start] {
            start -= 1;
        }
        if let Some(prev) = out.last() {
            start = start.max(prev.end.min(i));
        }
        let mut end = i + 1;
        while end < n && stops[end] > FLOOR && stops[end] <= stops[end - 1] * 1.02 + 0.02 {
            end += 1;
        }
        let top = stops[i];
        let mut peak = (start..end).filter(|j| stops[*j] >= 0.85 * top).max_by(|a, b| lum[*a].partial_cmp(&lum[*b]).unwrap()).unwrap_or(i);
        // A clipped frame before the measured maximum that is at least as
        // bright is the real peak: its own value is only a lower bound.
        if let Some(j) = (start..peak).find(|j| bound[*j] && lum[*j] >= lum[peak]) {
            peak = j;
        }
        let scan_from = i;
        let i = peak;
        // The fall: frames i..end, then 0 at `end` unless the cut or another flash comes first.
        let cut_short = end >= n || stops[end] > FLOOR;
        let mut s: Vec<f64> = stops[i..end].to_vec();
        let mut w: Vec<f64> = (i..end).map(|j| if bound[j] || conf[j] < 0.1 { 0.0 } else { 1.0 }).collect();
        if !cut_short {
            s.push(stops[end].max(0.0));
            w.push(1.0);
        }
        let usable = w.iter().filter(|v| **v > 0.0).count();
        let peak_bound = bound[i];
        let lower = if peak_bound { Some(stops[i]) } else { None };
        // Candidate lengths: back at 0 on the first frame below the floor, or,
        // when the cut comes first, somewhere past it.
        let lengths: Vec<f64> = if cut_short { [0.0, 1.0, 2.0, 4.0, 8.0].iter().map(|e| (end - i) as f64 + e).collect() } else { vec![(end - i) as f64] };
        let (mut peak_stops, mut rate, mut sse, mut len) = (stops[i], 0.3, f64::MAX, lengths[0]);
        let mut extrapolated = false;
        if usable >= 3 || (usable >= 2 && peak_bound) {
            for l in &lengths {
                let (p, r, e) = fit_decay(&s, &w, *l, None);
                if e < sse - 1e-9 {
                    (peak_stops, rate, sse, len) = (p, r, e, *l);
                }
            }
            extrapolated = peak_bound;
            if let Some(lb) = lower {
                // Never below what the peak frame itself shows, and not far
                // above it either: a tail of two or three frames cannot carry
                // an extrapolation over more than a few stops.
                if peak_stops < lb || peak_stops > lb + 3.0 {
                    peak_stops = peak_stops.clamp(lb, lb + 3.0);
                    let lb = peak_stops;
                    for l in &lengths {
                        let (_, r, e) = fit_decay(&s, &w, *l, Some(lb));
                        if e < sse || *l == lengths[0] {
                            (rate, sse, len) = (r, e, *l);
                        }
                    }
                }
            } else if (peak_stops - stops[i]).abs() > 0.35 {
                // The peak frame was measured: a curve that misses it badly is not the flash's curve.
                peak_stops = stops[i];
                let (_, r, e) = fit_decay(&s, &w, len, Some(peak_stops));
                (rate, sse) = (r, e);
            }
        } else if usable == 2 && s.len() >= 2 {
            let (_, r, e) = fit_decay(&s, &w, len, Some(stops[i]));
            (rate, sse) = (r, e);
        }
        let rate_confidence = if usable >= 3 { ((usable as f64 - 2.0) / 4.0).min(1.0) * 0.8 } else if usable == 2 { 0.25 } else { 0.1 };
        let fit_rms = if sse < f64::MAX { (sse / usable.max(1) as f64).sqrt() } else { 0.0 };
        let confidence = if extrapolated { 0.3 * rate_confidence.max(0.3) } else { conf[i].clamp(0.0, 1.0) };
        out.push(Flash {
            start: start.min(i),
            peak: i,
            end: i + len.round() as usize,
            peak_stops,
            measured: stops[i],
            lower_bound: lower,
            extrapolated,
            rate,
            rate_confidence,
            confidence,
            fit_rms,
        });
        next = end.max(scan_from + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::make_pair;
    use crate::synth;
    use crate::track::all_usable;

    #[test]
    fn gain_is_recovered_through_clipping() {
        let truth = [0.0, 0.5, 1.0, 2.0, 3.0, 4.0, -1.0];
        let mut clip = synth::Clip::new(128, 128, truth.len());
        clip.noise = 1.0;
        clip.vignette = 0.4;
        for (f, s) in truth.iter().enumerate() {
            clip.frame_mut(f).stops = *s;
        }
        let frames = clip.render();
        let usable = all_usable(&frames);
        for (f, s) in truth.iter().enumerate() {
            let pair = make_pair(&frames, 0, f, None, &usable);
            let e = measure(&pair, false, 1.0);
            let tol = if *s >= 3.0 { 0.45 } else { 0.15 };
            assert!((e.stops - s).abs() < tol, "frame {f}: measured {:.2} stops, truth {s}", e.stops);
            assert!(e.fade.is_none(), "frame {f}: {:?}", e.fade);
        }
    }

    #[test]
    fn flash_events_report_peak_and_decay() {
        // Two flashes as the renderer draws them (a `decay` ease that ends on
        // 0): 4 stops at rate 0.25 over 10 frames, then 2 stops at rate 0.5
        // over 8. (This test used to feed pure exponentials, which the
        // renderer's normalised `decay` never produces; the log-linear fit
        // that passed it overestimated real rates by 0.07 on average.)
        let mut stops = vec![0.0; 30];
        for k in 0..10 {
            stops[3 + k] = 4.0 * decay_curve(0.25, k as f64, 10.0);
        }
        for k in 0..8 {
            stops[18 + k] = 2.0 * decay_curve(0.5, k as f64, 8.0);
        }
        let conf = vec![1.0; 30];
        let free = vec![false; 30];
        let fl = flashes(&stops, &conf, &stops, &free);
        assert_eq!(fl.len(), 2, "{fl:?}");
        assert_eq!((fl[0].peak, fl[1].peak), (3, 18));
        assert!((fl[0].peak_stops - 4.0).abs() < 0.05, "{fl:?}");
        assert!((fl[0].rate - 0.25).abs() < 0.03, "rate {}", fl[0].rate);
        assert!((fl[1].rate - 0.5).abs() < 0.06, "rate {}", fl[1].rate);
        assert!(fl[0].lower_bound.is_none() && !fl[0].extrapolated);
    }

    #[test]
    fn a_clipped_peak_is_extrapolated_from_its_tail() {
        // 5 stops at rate 0.3 over 12 frames; the first three frames only give a lower bound of 2.5.
        let truth: Vec<f64> = (0..16).map(|k| 5.0 * decay_curve(0.3, k as f64, 12.0)).collect();
        let mut stops = truth.clone();
        let mut bound = vec![false; 16];
        for k in 0..3 {
            stops[k] = 2.5;
            bound[k] = true;
        }
        let lum: Vec<f64> = truth.clone();
        let fl = flashes(&stops, &[1.0; 16], &lum, &bound);
        assert_eq!(fl.len(), 1, "{fl:?}");
        assert_eq!(fl[0].peak, 0);
        assert_eq!(fl[0].lower_bound, Some(2.5));
        assert!(fl[0].extrapolated && (fl[0].peak_stops - 5.0).abs() < 0.4, "{fl:?}");
        assert!((fl[0].rate - 0.3).abs() < 0.05, "{fl:?}");
        assert!(fl[0].confidence < 0.4);
    }
}
