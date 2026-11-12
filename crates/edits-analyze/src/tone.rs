//! Tone hits relative to the reference frame: saturation, inversion,
//! threshold and posterise.
//!
//! - saturation: ratio of chroma to value, over pixels not clipped by a flash;
//! - inversion: negative correlation of luminance with the registered
//!   reference frame (and, without registration, sign flips of the
//!   frame-to-frame correlation, see `shots::invert_toggles`);
//! - threshold: the quantile-matched tone curve is a step (the frame is
//!   two-tone, the reference is not); the step position is the level;
//! - posterise: the frame's channel values sit on a few levels.

use crate::exposure::LumHist;
use crate::measure::Pair;

#[derive(Clone, Copy, Debug)]
pub struct ToneSample {
    /// Saturation of the frame divided by that of the reference frame.
    pub sat_ratio: f64,
    pub sat_confidence: f64,
    /// Chroma/value of the frame and of the reference frame (0..1).
    pub sat_cur: f64,
    pub sat_ref: f64,
    /// Correlation of luminance with the warped reference frame (meaningful when registered).
    pub lum_corr: f64,
    /// Two-tone look: the level (gamma-encoded luminance of the reference frame) of the step.
    pub threshold: Option<f64>,
    /// Number of tone levels if the frame is posterised.
    pub posterize: Option<u32>,
}

fn chroma_stats(rgb: &[u8], sel: impl Fn(usize) -> bool) -> (f64, f64) {
    let (mut c, mut v) = (0.0f64, 0.0f64);
    for (i, p) in rgb.chunks_exact(3).enumerate() {
        if !sel(i) {
            continue;
        }
        let mx = p[0].max(p[1]).max(p[2]);
        let mn = p[0].min(p[1]).min(p[2]);
        c += (mx - mn) as f64;
        v += mx as f64;
    }
    (c, v)
}

/// Smallest number of 32-level-wide bins that hold 90% of the channel values.
fn levels90(rgb: &[u8], mask: &[bool]) -> usize {
    let mut hist = [0u32; 32];
    let mut n = 0u32;
    for (i, p) in rgb.chunks_exact(3).enumerate() {
        if mask[i] {
            for c in p {
                hist[(*c >> 3) as usize] += 1;
                n += 1;
            }
        }
    }
    let mut h = hist.to_vec();
    h.sort_unstable_by(|a, b| b.cmp(a));
    let mut acc = 0u32;
    for (k, v) in h.iter().enumerate() {
        acc += v;
        if acc as f64 >= 0.9 * n as f64 {
            return k + 1;
        }
    }
    32
}

/// Correlation of luminance between the frame and the warped reference
/// frame, on block means (tolerant of small misregistration). Negative =
/// inverted tones.
pub fn lum_correlation(pair: &Pair) -> f64 {
    let step = (pair.h / 60).max(1);
    let (mut s, mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let lum = |p: &[u8]| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
    let mut y = step;
    while y + step < pair.h {
        let mut x = step;
        while x + step < pair.w {
            // Block means.
            let (mut a, mut b, mut k) = (0.0, 0.0, 0.0);
            for yy in y..y + step {
                for xx in x..x + step {
                    let i = yy * pair.w + xx;
                    if pair.mask[i] {
                        a += lum(&pair.cur[i * 3..i * 3 + 3]);
                        b += lum(&pair.refw[i * 3..i * 3 + 3]);
                        k += 1.0;
                    }
                }
            }
            if k > 0.0 {
                let (a, b) = (a / k, b / k);
                s += 1.0;
                sa += a;
                sb += b;
                saa += a * a;
                sbb += b * b;
                sab += a * b;
            }
            x += step;
        }
        y += step;
    }
    if s > 10.0 {
        let (va, vb) = (saa / s - (sa / s).powi(2), sbb / s - (sb / s).powi(2));
        if va > 1e-6 && vb > 1e-6 {
            (sab / s - sa / s * sb / s) / (va * vb).sqrt()
        } else {
            0.0
        }
    } else {
        0.0
    }
}

pub fn measure(pair: &Pair, inverted: bool) -> ToneSample {
    let n = pair.w * pair.h;
    // Saturation over pixels that a flash has not clipped, same pixels in both frames.
    let ok = |i: usize| {
        let p = &pair.cur[i * 3..i * 3 + 3];
        let q = &pair.refw[i * 3..i * 3 + 3];
        pair.mask[i] && p[0].max(p[1]).max(p[2]) < 250 && q[0].max(q[1]).max(q[2]) > 24
    };
    let used = (0..n).filter(|i| ok(*i)).count();
    let (cc, cv) = chroma_stats(pair.cur, ok);
    let (rc, rv) = chroma_stats(&pair.refw, ok);
    let sat_cur = if cv > 0.0 { cc / cv } else { 0.0 };
    let sat_ref = if rv > 0.0 { rc / rv } else { 0.0 };
    let masked = pair.mask.iter().filter(|m| **m).count().max(1);
    let (sat_ratio, sat_confidence) = if sat_ref < 0.04 || inverted {
        (1.0, 0.0)
    } else {
        ((sat_cur / sat_ref).clamp(0.0, 4.0), ((used as f64 / masked as f64) / 0.3).min(1.0) * ((sat_ref - 0.04) / 0.1).clamp(0.2, 1.0))
    };

    let lum_corr = lum_correlation(pair);

    // Tone-curve shape from matched quantiles of gamma-encoded luminance.
    let hr = LumHist::new(&pair.refw, &pair.mask, false);
    let hc = LumHist::new(pair.cur, &pair.mask, inverted);
    let mut threshold = None;
    let mut posterize = None;
    if hr.count >= 200 {
        let ps: Vec<f64> = (1..=98).map(|i| i as f64 / 99.0).collect();
        let enc = |v: &f64| v.powf(1.0 / 2.2);
        let x: Vec<f64> = hr.quantiles(&ps).iter().map(enc).collect();
        let y: Vec<f64> = hc.quantiles(&ps).iter().map(enc).collect();
        let two_tone = |v: &[f64]| {
            let ext = v.iter().filter(|t| **t < 0.16 || **t > 0.84).count() as f64 / v.len() as f64;
            let mid = v.iter().filter(|t| **t > 0.35 && **t < 0.67).count() as f64 / v.len() as f64;
            ext > 0.82 && mid < 0.08
        };
        let dark = y.iter().filter(|t| **t < 0.3).count();
        if two_tone(&y) && !two_tone(&x) && dark >= 3 && dark + 3 <= y.len() {
            // Where does the transfer cross a quarter and three quarters of its range?
            let cross = |level: f64| x[y.iter().position(|t| *t >= level).unwrap_or(y.len() - 1)];
            let (x25, x50, x75) = (cross(0.25), cross(0.5), cross(0.75));
            if (x75 - x25) < 0.35 * x50.max(0.05) {
                threshold = Some(x50);
            }
        }
        if threshold.is_none() {
            let (lc, lr) = (levels90(pair.cur, &pair.mask), levels90(&pair.refw, &pair.mask));
            if (3..=7).contains(&lc) && lr >= 2 * lc && lr >= 10 {
                posterize = Some(lc as u32);
            }
        }
    }
    ToneSample { sat_ratio, sat_confidence, sat_cur, sat_ref, lum_corr, threshold, posterize }
}

/// Runs of `true` as (start, end) index pairs.
pub fn runs(flags: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < flags.len() {
        if flags[i] {
            let s = i;
            while i < flags.len() && flags[i] {
                i += 1;
            }
            out.push((s, i));
        } else {
            i += 1;
        }
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
    fn saturation_inversion_and_threshold_are_told_apart() {
        let mut clip = synth::Clip::new(128, 128, 7);
        clip.noise = 1.0;
        clip.frame_mut(1).saturation = 0.0;
        clip.frame_mut(2).saturation = 0.5;
        clip.frame_mut(3).invert = true;
        clip.frame_mut(4).threshold = Some(0.45);
        clip.frame_mut(5).stops = 3.5; // a flash must not read as a threshold
        clip.frame_mut(6).stops = 2.0;
        clip.frame_mut(6).saturation = 0.0; // black-and-white under a flash
        let frames = clip.render();
        let usable = all_usable(&frames);
        let m = |f: usize, inv: bool| measure(&make_pair(&frames, 0, f, Some(&crate::img::Sim::IDENTITY), &usable), inv);
        let same = m(0, false);
        assert!((same.sat_ratio - 1.0).abs() < 0.02 && same.lum_corr > 0.99 && same.threshold.is_none() && same.posterize.is_none());
        assert!(m(1, false).sat_ratio < 0.06, "bw {}", m(1, false).sat_ratio);
        assert!((m(2, false).sat_ratio - 0.5).abs() < 0.12, "half {}", m(2, false).sat_ratio);
        assert!(m(3, false).lum_corr < -0.8, "invert corr {}", m(3, false).lum_corr);
        let th = m(4, false);
        assert!(th.threshold.map(|l| (l - 0.45).abs() < 0.06).unwrap_or(false), "threshold {:?}", th.threshold);
        assert!(m(5, false).threshold.is_none(), "flash read as threshold");
        assert!(m(6, false).sat_ratio < 0.1, "bw under flash {}", m(6, false).sat_ratio);
    }
}
