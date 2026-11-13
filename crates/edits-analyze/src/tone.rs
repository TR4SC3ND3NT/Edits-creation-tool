//! Tone hits relative to the reference frame: saturation, inversion,
//! threshold and posterise.
//!
//! - saturation: ratio of chroma to luminance in linear light, over blocks not clipped by a flash;
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
    /// Share of the frame's blocks the saturation was measured on (not clipped, not too dark).
    pub sat_share: f64,
    /// Chroma/luminance, in linear light, of the frame and of the reference frame.
    pub sat_cur: f64,
    pub sat_ref: f64,
    /// Correlation of luminance with the warped reference frame (meaningful when registered).
    pub lum_corr: f64,
    /// Two-tone look: the level (gamma-encoded luminance of the reference frame) of the step.
    pub threshold: Option<f64>,
    /// Number of tone levels if the frame is posterised.
    pub posterize: Option<u32>,
}

/// Chroma and luminance sums, in linear light, of the frame and of the
/// reference frame over blocks that a flash has not clipped. The renderer's
/// `saturation` mixes linear RGB with its Rec. 709 luminance, which keeps the
/// luminance and multiplies the chroma (largest minus smallest channel) by
/// the amount, and an exposure gain multiplies both: so the ratio of
/// chroma / luminance between the two frames *is* the saturation amount,
/// whatever the exposure. (Measured on display values instead, a 3-stop flash
/// alone read as 0.87x.) Block means are used so that noise (which reads as
/// chroma in dark areas) does not pass for colour. Blocks next to a block
/// that the flash has clipped (white in the frame, not in the reference
/// frame) are left out too: video stores colour at half resolution (4:2:0)
/// and a codec smears it further, so the grey of a clipped area bleeds into
/// its neighbours (`edits calib`: line art under a flash read 0.74x after
/// H.264 at 1.5 Mbit/s with only the clipped blocks left out). Returns
/// (chroma cur, luminance cur, chroma ref, luminance ref, share of blocks used).
fn chroma_stats(pair: &Pair) -> (f64, f64, f64, f64, f64) {
    let lut = crate::img::srgb_lut();
    let b = (pair.h / 90).max(3);
    let (bw, bh) = (pair.w / b, pair.h / b);
    // Per block: channel means of the frame and of the reference, and whether any pixel is clipped.
    struct Block {
        c: [f64; 3],
        r: [f64; 3],
        ok: bool,
        /// Clipped in the frame; clipped by the flash (not already white in the reference frame).
        clipped: bool,
        flashed: bool,
    }
    let mut blocks: Vec<Block> = Vec::with_capacity(bw * bh);
    for by in 0..bh {
        for bx in 0..bw {
            let (mut c, mut r, mut k) = ([0.0f64; 3], [0.0f64; 3], 0.0f64);
            let (mut clipped, mut ref_clipped) = (false, false);
            for yy in by * b..(by + 1) * b {
                for xx in bx * b..(bx + 1) * b {
                    let i = yy * pair.w + xx;
                    // White is 255 only where there is no vignette.
                    let white = pair.flat.map(|f| f.white[i]).unwrap_or(255) as f32;
                    let px = &pair.cur[i * 3..i * 3 + 3];
                    if pair.mask[i] {
                        clipped |= px.iter().any(|v| *v as f32 >= 0.96 * white);
                        ref_clipped |= pair.refw[i * 3..i * 3 + 3].iter().any(|v| *v as f32 >= 0.96 * white);
                        for ch in 0..3 {
                            c[ch] += lut[px[ch] as usize] as f64;
                            r[ch] += lut[pair.refw[i * 3 + ch] as usize] as f64;
                        }
                        k += 1.0;
                    }
                }
            }
            let ok = k >= (b * b) as f64 * 0.5;
            if ok {
                for ch in 0..3 {
                    c[ch] /= k;
                    r[ch] /= k;
                }
            }
            blocks.push(Block { c, r, ok, clipped, flashed: clipped && !ref_clipped });
        }
    }
    let (mut cc, mut cv, mut rc, mut rv) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut total, mut used) = (0usize, 0usize);
    let mx = |v: &[f64; 3]| v[0].max(v[1]).max(v[2]);
    let mn = |v: &[f64; 3]| v[0].min(v[1]).min(v[2]);
    let lum = |v: &[f64; 3]| 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
    for by in 0..bh {
        for bx in 0..bw {
            let blk = &blocks[by * bw + bx];
            if !blk.ok {
                continue;
            }
            total += 1;
            let near_clip = blk.clipped || (by.saturating_sub(1)..(by + 2).min(bh)).any(|y| (bx.saturating_sub(1)..(bx + 2).min(bw)).any(|x| blocks[y * bw + x].flashed));
            // Too dark in the reference frame to carry a readable colour (about code 24).
            if near_clip || mx(&blk.r) <= 0.009 {
                continue;
            }
            used += 1;
            cc += mx(&blk.c) - mn(&blk.c);
            cv += lum(&blk.c);
            rc += mx(&blk.r) - mn(&blk.r);
            rv += lum(&blk.r);
        }
    }
    (cc, cv, rc, rv, used as f64 / total.max(1) as f64)
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
    // Saturation over areas that a flash has not clipped, the same areas in both frames.
    let (cc, cv, rc, rv, share) = chroma_stats(pair);
    let sat_cur = if cv > 0.0 { cc / cv } else { 0.0 };
    let sat_ref = if rv > 0.0 { rc / rv } else { 0.0 };
    let (sat_ratio, sat_confidence) = if sat_ref < 0.03 || inverted {
        (1.0, 0.0)
    } else {
        ((sat_cur / sat_ref).clamp(0.0, 4.0), (share / 0.3).min(1.0) * ((sat_ref - 0.02) / 0.06).clamp(0.2, 1.0))
    };

    let lum_corr = lum_correlation(pair);

    // Tone-curve shape from matched quantiles of gamma-encoded luminance.
    let hr = LumHist::new(&pair.refw, &pair.mask, false, pair.flat);
    let hc = LumHist::new(pair.cur, &pair.mask, inverted, pair.flat);
    let mut threshold = None;
    let mut posterize = None;
    if hr.count >= 200 {
        // An even grid, plus the top end: a threshold on dark art leaves only a few white specks.
        let ps: Vec<f64> = (1..=98).map(|i| i as f64 / 99.0).chain([0.993, 0.996, 0.998]).collect();
        let enc = |v: &f64| v.powf(1.0 / 2.2);
        let x: Vec<f64> = hr.quantiles(&ps).iter().map(enc).collect();
        let y: Vec<f64> = hc.quantiles(&ps).iter().map(enc).collect();
        let two_tone = |v: &[f64]| {
            let ext = v.iter().filter(|t| **t < 0.16 || **t > 0.84).count() as f64 / v.len() as f64;
            let mid = v.iter().filter(|t| **t > 0.35 && **t < 0.67).count() as f64 / v.len() as f64;
            ext > 0.82 && mid < 0.08
        };
        let dark = y.iter().filter(|t| **t < 0.3).count();
        let white = y.iter().filter(|t| **t > 0.84).count();
        if two_tone(&y) && !two_tone(&x) && dark >= 3 && white >= 2 {
            // Where does the transfer cross a quarter and three quarters of its range?
            let cross = |level: f64| x[y.iter().position(|t| *t >= level).unwrap_or(y.len() - 1)];
            let (x25, x50, x75) = (cross(0.25), cross(0.5), cross(0.75));
            if (x75 - x25) < 0.35 * x50.max(0.05) {
                // A strong flash also leaves only white and dark ink. The
                // difference: a threshold crushes the darks to black, a flash
                // lifts them. Without measurable darks, a small dark share
                // is read as a flash.
                let lifted: Vec<f64> = (0..y.len()).filter(|i| y[*i] < 0.3 && x[*i] > 0.08).map(|i| (y[i] / x[i]).powf(2.2)).collect();
                let is_flash = if lifted.len() >= 3 { crate::img::median(&lifted) > 1.3 } else { (dark as f64) < 0.15 * y.len() as f64 };
                if !is_flash {
                    threshold = Some(x50);
                }
            }
        }
        if threshold.is_none() {
            let (lc, lr) = (levels90(pair.cur, &pair.mask), levels90(&pair.refw, &pair.mask));
            if (3..=7).contains(&lc) && lr >= 2 * lc && lr >= 10 {
                posterize = Some(lc as u32);
            }
        }
    }
    ToneSample { sat_ratio, sat_confidence, sat_share: share, sat_cur, sat_ref, lum_corr, threshold, posterize }
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
