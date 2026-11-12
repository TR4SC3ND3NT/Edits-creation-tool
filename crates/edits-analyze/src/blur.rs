//! Blur amount relative to the reference frame.
//!
//! A Gaussian blur of sigma `s` pixels multiplies the power spectrum by
//! `exp(-4 pi^2 s^2 f^2)`. So the log of the radially averaged power-spectrum
//! ratio frame/reference is a straight line in `f^2`; its slope gives `s`.
//! A constant factor (exposure, contrast) only moves the intercept. The same
//! fit per orientation sector tells an isotropic blur from a directional one.

use crate::img::{radial_power, Gray};

#[derive(Clone, Copy, Debug, Default)]
pub struct BlurSample {
    /// Gaussian-equivalent sigma in pixels of the analysed image.
    pub sigma_px: f64,
    /// 0 = same in all directions, towards 1 = blurred along one direction only.
    pub anisotropy: f64,
    /// RMS of the straight-line fit (log power ratio).
    pub fit_rms: f64,
}

/// Side of the centred analysis window for a frame size.
pub fn window(w: usize, h: usize) -> usize {
    ((w.min(h) as f64 * 0.62 / 8.0).floor() as usize * 8).max(16)
}

/// Replace the pixels that are not `ok` (static overlay strokes, parts the
/// warped reference frame does not cover) by the mean of their nearest valid
/// neighbours. A sharp overlay stroke on top of a blurred picture keeps its
/// high frequencies; left in, it hides the blur (the spectrum ratio stays
/// near 1 where the stroke's power dominates). Returns the filled image and
/// the share of the analysis window that was valid.
pub fn fill_masked(img: &Gray, ok: &[bool]) -> (Gray, f64) {
    let (w, h) = (img.w, img.h);
    let n = window(w, h);
    let (x0, y0) = ((w - n) / 2, (h - n) / 2);
    let mut out = img.clone();
    let mut valid = 0usize;
    for y in y0..y0 + n {
        for x in x0..x0 + n {
            if ok[y * w + x] {
                valid += 1;
                continue;
            }
            for r in [2usize, 4, 8, 16] {
                let (mut acc, mut k) = (0.0f32, 0.0f32);
                for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                    for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                        if ok[yy * w + xx] {
                            acc += img.d[yy * w + xx];
                            k += 1.0;
                        }
                    }
                }
                if k >= 3.0 {
                    out.d[y * w + x] = acc / k;
                    break;
                }
            }
        }
    }
    (out, valid as f64 / (n * n) as f64)
}

/// Weighted least squares of `y = c - k x` with `x = f^2`; returns (k, rms).
fn fit(pc: &[f64], pr: &[f64], n: usize, f_hi: f64) -> Option<(f64, f64)> {
    let (mut sw, mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut pts = Vec::new();
    for k in 2..pc.len() {
        let f = k as f64 / n as f64;
        if f > f_hi {
            break;
        }
        if pc[k] <= 0.0 || pr[k] <= 0.0 {
            continue;
        }
        let (x, y, w) = (f * f, (pc[k] / pr[k]).ln(), k as f64);
        sw += w;
        sx += w * x;
        sy += w * y;
        sxx += w * x * x;
        sxy += w * x * y;
        pts.push((x, y, w));
    }
    if pts.len() < 4 {
        return None;
    }
    let den = sw * sxx - sx * sx;
    if den.abs() < 1e-18 {
        return None;
    }
    let slope = (sw * sxy - sx * sy) / den;
    let c = (sy - slope * sx) / sw;
    let rms = (pts.iter().map(|(x, y, w)| w * (y - c - slope * x).powi(2)).sum::<f64>() / sw).sqrt();
    Some((-slope, rms))
}

/// Blur of `cur` relative to `reference` (luminance images of the same size and geometry).
pub fn measure(cur: &Gray, reference: &Gray) -> BlurSample {
    let n = window(cur.w, cur.h);
    let sectors = 4;
    let pc = radial_power(cur, n, sectors);
    let pr = radial_power(reference, n, sectors);
    let sigma_of = |k: f64| k.max(0.0).sqrt() / std::f64::consts::TAU;
    // Fit only where the blurred signal is still above the noise: up to the
    // frequency at which the fitted Gaussian has fallen to e^-4. Start with the
    // lowest frequencies (always above the noise) and widen from there.
    let mut f_hi: f64 = (8.0 / n as f64).max(0.06);
    let mut res = (0.0, 0.0);
    for _ in 0..6 {
        match fit(&pc[0], &pr[0], n, f_hi) {
            Some(r) => res = r,
            None => return BlurSample::default(),
        }
        let s = sigma_of(res.0);
        let next = if s > 0.05 { (1.0 / (std::f64::consts::PI * s)).clamp(0.05, 0.4) } else { 0.4 };
        // Widen gradually so one noisy low-frequency fit cannot throw the range.
        f_hi = next.min(f_hi * 2.0);
    }
    let sigma = sigma_of(res.0);
    let mut anisotropy = 0.0;
    if sigma > 0.6 {
        let ks: Vec<f64> = (0..sectors).filter_map(|s| fit(&pc[1 + s], &pr[1 + s], n, f_hi).map(|r| r.0.max(0.0))).collect();
        if ks.len() == sectors {
            let (lo, hi) = ks.iter().fold((f64::MAX, 0.0f64), |a, v| (a.0.min(*v), a.1.max(*v)));
            anisotropy = (hi - lo) / (hi + lo + 1e-9);
        }
    }
    BlurSample { sigma_px: sigma, anisotropy, fit_rms: res.1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    #[test]
    fn gaussian_blur_radius_is_recovered() {
        let truth = [0.0, 0.4, 0.8, 1.5, 2.5];
        let mut clip = synth::Clip::new(192, 192, truth.len() + 1);
        clip.noise = 1.0;
        for (f, b) in truth.iter().enumerate() {
            clip.frame_mut(f).blur = *b;
        }
        // Blur under a flash: exposure must not change the estimate much.
        clip.frame_mut(truth.len()).blur = 1.5;
        clip.frame_mut(truth.len()).stops = 1.0;
        let frames = clip.render();
        let reference = frames.gray(0);
        for (f, b) in truth.iter().enumerate() {
            let m = measure(&frames.gray(f), &reference);
            let got = 100.0 * m.sigma_px / 192.0;
            assert!((got - b).abs() < 0.08 + 0.12 * b, "frame {f}: {got:.2}% vs {b}%");
            assert!(m.anisotropy < 0.3, "frame {f}: anisotropy {}", m.anisotropy);
        }
        let m = measure(&frames.gray(truth.len()), &reference);
        let got = 100.0 * m.sigma_px / 192.0;
        assert!((got - 1.5).abs() < 0.4, "blur under flash {got:.2}% vs 1.5%");
    }

    #[test]
    fn directional_blur_is_flagged_anisotropic() {
        let mut clip = synth::Clip::new(192, 192, 1);
        clip.noise = 1.0;
        let frames = clip.render();
        let sharp = frames.gray(0);
        // Blur along x only.
        let k = crate::img::gaussian_kernel(4.0);
        let r = (k.len() / 2) as i32;
        let mut streak = sharp.clone();
        for y in 0..sharp.h {
            for x in 0..sharp.w {
                let mut acc = 0.0;
                for (j, kv) in k.iter().enumerate() {
                    let xx = (x as i32 + j as i32 - r).clamp(0, sharp.w as i32 - 1) as usize;
                    acc += kv * sharp.at(xx, y);
                }
                streak.d[y * sharp.w + x] = acc;
            }
        }
        let m = measure(&streak, &sharp);
        assert!(m.sigma_px > 0.6 && m.anisotropy > 0.5, "{m:?}");
    }
}
