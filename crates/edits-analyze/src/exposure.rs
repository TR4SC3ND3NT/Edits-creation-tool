//! Exposure: per-frame gain relative to the shot's reference frame, and flash events.
//!
//! The gain is read from the tone transfer between the two frames, obtained
//! by matching quantiles of linear-light luminance (no pixel correspondence
//! needed, so a few pixels of misregistration under a flash do not matter).
//! Quantiles that are clipped at white in the frame are left out, which is
//! why a flash can still be measured from the dark ink that survives it.

use crate::img::srgb_lut;
use crate::measure::Pair;

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
    pub fn new(rgb: &[u8], mask: &[bool], invert: bool) -> Self {
        let lut = srgb_lut();
        let mut hist = vec![0u32; BINS];
        let mut count = 0;
        for (i, p) in rgb.chunks_exact(3).enumerate() {
            if !mask[i] {
                continue;
            }
            let v = |c: u8| lut[if invert { 255 - c } else { c } as usize];
            let y = 0.2126 * v(p[0]) + 0.7152 * v(p[1]) + 0.0722 * v(p[2]);
            let b = ((y.powf(1.0 / 2.2) * BINS as f32) as usize).min(BINS - 1);
            hist[b] += 1;
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
    /// Share of the tone range clipped at white in this frame.
    pub clipped: f64,
    /// A mix toward a colour explains the frame better than a gain: (amount, luminance of the colour).
    pub fade: Option<(f64, f64)>,
}

pub fn quantile_grid() -> Vec<f64> {
    (1..=48).map(|i| i as f64 / 49.0).collect()
}

/// Gain of the frame relative to the (warped) reference frame.
pub fn measure(pair: &Pair, inverted: bool) -> ExposureSample {
    let hr = LumHist::new(&pair.refw, &pair.mask, false);
    let hc = LumHist::new(pair.cur, &pair.mask, inverted);
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
    let hr = LumHist::new(&pair.refw, &pair.mask, false);
    let hc = LumHist::new(pair.cur, &pair.mask, invert_cur);
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
    let clipped = y.iter().filter(|v| **v >= WHITE).count() as f64 / n as f64;
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
    if ratios.len() < 3 {
        // Everything measurable is clipped: only a lower bound is available.
        let dark = x.iter().copied().find(|v| *v > FLOOR_X).unwrap_or(0.05);
        let bound = if clipped > 0.5 { (WHITE / dark).max(1.0) } else { 1.0 };
        return ExposureSample { stops: bound.log2(), confidence: if clipped > 0.5 { 0.2 } else { 0.0 }, clipped, fade: None };
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
    ExposureSample { stops, confidence, clipped, fade }
}

#[derive(Clone, Debug)]
pub struct Flash {
    /// First frame above the floor, the peak frame and one past the last frame (indices into the input).
    pub start: usize,
    pub peak: usize,
    pub end: usize,
    pub peak_stops: f64,
    /// Fraction of the remaining exposure lost per frame after the peak (the `decay` ease rate).
    pub rate: f64,
    pub rate_confidence: f64,
    pub confidence: f64,
}

/// Flash events in a shot's exposure curve (`stops[i]` relative to the reference frame).
pub fn flashes(stops: &[f64], conf: &[f64]) -> Vec<Flash> {
    const PEAK: f64 = 0.8;
    const FLOOR: f64 = 0.12;
    let n = stops.len();
    let mut out: Vec<Flash> = Vec::new();
    let mut i = 0;
    while i < n {
        // A local maximum that stands out.
        let is_peak = stops[i] >= PEAK && (i == 0 || stops[i] > stops[i - 1]) && (i + 1 == n || stops[i] >= stops[i + 1]);
        if !is_peak {
            i += 1;
            continue;
        }
        let mut start = i;
        while start > 0 && stops[start - 1] > FLOOR && stops[start - 1] < stops[start] {
            start -= 1;
        }
        if let Some(prev) = out.last() {
            start = start.max(prev.end);
        }
        let mut end = i + 1;
        while end < n && stops[end] > FLOOR && stops[end] <= stops[end - 1] * 1.02 + 0.02 {
            end += 1;
        }
        // Decay: log-linear fit of the samples after the peak.
        let pts: Vec<(f64, f64)> = (i..end).filter(|j| stops[*j] > 0.05).map(|j| ((j - i) as f64, stops[j].ln())).collect();
        let (mut rate, mut rate_conf) = (0.3, 0.1);
        if pts.len() >= 3 {
            let m = pts.len() as f64;
            let (sx, sy): (f64, f64) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
            let (sxx, sxy): (f64, f64) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0 * p.0, a.1 + p.0 * p.1));
            let slope = (m * sxy - sx * sy) / (m * sxx - sx * sx);
            rate = (1.0 - slope.exp()).clamp(0.02, 0.98);
            rate_conf = ((m - 2.0) / 4.0).min(1.0) * 0.8;
        } else if pts.len() == 2 {
            rate = (1.0 - (pts[1].1 - pts[0].1).exp()).clamp(0.02, 0.98);
            rate_conf = 0.25;
        }
        out.push(Flash { start, peak: i, end, peak_stops: stops[i], rate, rate_confidence: rate_conf, confidence: conf[i].clamp(0.0, 1.0) });
        i = end.max(i + 1);
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
            let e = measure(&pair, false);
            let tol = if *s >= 3.0 { 0.45 } else { 0.15 };
            assert!((e.stops - s).abs() < tol, "frame {f}: measured {:.2} stops, truth {s}", e.stops);
            assert!(e.fade.is_none(), "frame {f}: {:?}", e.fade);
        }
    }

    #[test]
    fn flash_events_report_peak_and_decay() {
        // Two flashes: 4 stops decaying by 0.25 per frame, then 2 stops decaying by 0.5.
        let mut stops = vec![0.0; 30];
        for k in 0..10 {
            stops[3 + k] = 4.0 * 0.75f64.powi(k as i32);
            stops[18 + k] += 2.0 * 0.5f64.powi(k as i32);
        }
        let conf = vec![1.0; 30];
        let fl = flashes(&stops, &conf);
        assert_eq!(fl.len(), 2, "{fl:?}");
        assert_eq!((fl[0].peak, fl[1].peak), (3, 18));
        assert!((fl[0].peak_stops - 4.0).abs() < 1e-9);
        assert!((fl[0].rate - 0.25).abs() < 0.02, "rate {}", fl[0].rate);
        assert!((fl[1].rate - 0.5).abs() < 0.03, "rate {}", fl[1].rate);
    }
}
