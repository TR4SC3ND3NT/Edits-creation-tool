//! Which kind of blur: gaussian, directional, zoom or spin, from
//! gradient-orientation statistics.
//!
//! A blur removes gradient energy along the directions it smears in, and
//! only those: a gaussian blur takes some off every orientation alike; a
//! directional blur takes it off one fixed orientation (smearing along x
//! kills the x-derivatives); a zoom blur smears along the rays from the
//! centre and so kills the *radial* component of the gradient at every
//! pixel; a spin blur smears along circles and kills the *tangential* one.
//! Gradient energy summed over the frame is the same wherever the picture
//! sits, so the frame can be compared with its shot's reference frame even
//! when the registration is rough: per orientation, `ln(E_frame / E_ref)`.
//!
//! - 8 fixed orientations, 22.5 degrees apart: the first harmonic of the
//!   log ratios gives a directional blur's direction and strength;
//! - radial against tangential (about the frame centre, the centre itself
//!   left out): negative for zoom, positive for spin.
//!
//! The renderer's `directional_blur`, `zoom_blur` and `spin_blur` are told
//! apart on renders in `edits-calib` (tests/blurkind.rs); synthetic CPU
//! blurs are told apart in this module's tests.

use crate::img::Gray;

/// Gradient energy of one image by orientation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Orient {
    /// Energy of the gradient projected on the directions 0, 22.5, ..., 157.5 degrees (clockwise from +x, y down).
    pub axis: [f64; 8],
    pub radial: f64,
    pub tangential: f64,
}

/// Log ratios of a frame's orientation energies to its reference's.
#[derive(Clone, Copy, Debug, Default)]
pub struct BlurDir {
    pub axis: [f64; 8],
    pub radial: f64,
    pub tangential: f64,
}

/// Orientation energies over the central window (the blur stage's window),
/// skipping pixels where `ok` is false and a disc of 6% of the height at the centre.
pub fn orientation(img: &Gray, ok: Option<&[bool]>) -> Orient {
    let (w, h) = (img.w, img.h);
    let n = crate::blur::window(w, h);
    let (x0, y0) = ((w - n) / 2, (h - n) / 2);
    let (cx, cy) = ((w as f64 - 1.0) / 2.0, (h as f64 - 1.0) / 2.0);
    let hole = 0.06 * h as f64;
    let dirs: Vec<(f64, f64)> = (0..8).map(|k| ((k as f64 * 22.5).to_radians().cos(), (k as f64 * 22.5).to_radians().sin())).collect();
    let mut o = Orient::default();
    let good = |x: usize, y: usize| ok.map(|m| m[y * w + x]).unwrap_or(true);
    let (mut sum, mut cnt) = (0.0f64, 0.0f64);
    for y in y0.max(1)..(y0 + n).min(h - 1) {
        for x in x0.max(1)..(x0 + n).min(w - 1) {
            if !(good(x, y) && good(x - 1, y) && good(x + 1, y) && good(x, y - 1) && good(x, y + 1)) {
                continue;
            }
            sum += img.at(x, y) as f64;
            cnt += 1.0;
            let gx = 0.5 * (img.at(x + 1, y) - img.at(x - 1, y)) as f64;
            let gy = 0.5 * (img.at(x, y + 1) - img.at(x, y - 1)) as f64;
            for (k, (c, s)) in dirs.iter().enumerate() {
                let p = gx * c + gy * s;
                o.axis[k] += p * p;
            }
            let (dx, dy) = (x as f64 - cx, y as f64 - cy);
            let r = (dx * dx + dy * dy).sqrt();
            if r > hole {
                let (ux, uy) = (dx / r, dy / r);
                let pr = gx * ux + gy * uy;
                let pt = -gx * uy + gy * ux;
                o.radial += pr * pr;
                o.tangential += pt * pt;
            }
        }
    }
    // Relative to the mean level squared: an exposure gain scales gradients
    // and level alike, so it cancels; a blur keeps the level.
    let m2 = (sum / cnt.max(1.0)).powi(2).max(1e-9);
    o.axis.iter_mut().for_each(|v| *v /= m2);
    o.radial /= m2;
    o.tangential /= m2;
    o
}

/// `ln(cur / reference)` per orientation.
pub fn ratios(cur: &Orient, reference: &Orient) -> BlurDir {
    let l = |a: f64, b: f64| ((a + 1e-9) / (b + 1e-9)).ln();
    let mut axis = [0.0; 8];
    for k in 0..8 {
        axis[k] = l(cur.axis[k], reference.axis[k]);
    }
    BlurDir { axis, radial: l(cur.radial, reference.radial), tangential: l(cur.tangential, reference.tangential) }
}

/// Summary statistics of a set of log ratios.
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    /// Mean loss of gradient energy (positive when blurred).
    pub loss: f64,
    /// Amplitude of the first (180-degree) harmonic over the 8 orientations.
    pub axis_amp: f64,
    /// Peak-to-peak of the 8 orientations.
    pub axis_contrast: f64,
    /// Direction of the streaks (the most reduced gradient orientation), degrees 0..180.
    pub angle: f64,
    /// Radial minus tangential log ratio.
    pub rt: f64,
}

pub fn stats(d: &BlurDir) -> Stats {
    let mean = d.axis.iter().sum::<f64>() / 8.0;
    let (mut re, mut im) = (0.0, 0.0);
    for (k, v) in d.axis.iter().enumerate() {
        let phi = 2.0 * (k as f64 * 22.5).to_radians();
        re += (v - mean) * phi.cos();
        im += (v - mean) * phi.sin();
    }
    let amp = 2.0 * (re * re + im * im).sqrt() / 8.0;
    // The harmonic peaks where energy is kept best; the streaks run 90 degrees from there.
    let keep = 0.5 * im.atan2(re).to_degrees();
    let angle = (keep + 90.0).rem_euclid(180.0);
    let hi = d.axis.iter().copied().fold(f64::MIN, f64::max);
    let lo = d.axis.iter().copied().fold(f64::MAX, f64::min);
    Stats { loss: -mean, axis_amp: amp, axis_contrast: hi - lo, angle, rt: d.radial - d.tangential }
}

/// Classify a blur from its (median) log ratios: kind and a confidence in
/// 0..1. `None` when too little gradient energy was lost to tell.
pub fn classify(d: &BlurDir) -> Option<(&'static str, f64, Stats)> {
    let s = stats(d);
    if s.loss < 0.12 {
        return None;
    }
    let scale = s.loss + 0.1;
    // Measured on CPU blurs of procedural pictures (tests below): relative to the
    // mean loss, a gaussian blur leaves a harmonic and a radial-tangential
    // difference under 0.08; a directional streak gives a harmonic of 0.25-0.4,
    // zoom and spin a radial-tangential difference of 0.2-0.6 and little harmonic.
    let dir = s.axis_amp / scale;
    let rt = s.rt.abs() / scale;
    let (kind, margin) = if rt > 0.17 && rt > 1.5 * dir {
        (if s.rt < 0.0 { "zoom" } else { "spin" }, rt - 0.17)
    } else if dir > 0.15 {
        ("directional", dir - 0.15)
    } else {
        ("gaussian", 0.15 - dir.max(rt))
    };
    let strength = ((s.loss - 0.12) / 0.4).clamp(0.0, 1.0);
    Some((kind, ((0.4 + 2.5 * margin) * (0.5 + 0.5 * strength)).clamp(0.05, 0.95), s))
}

/// Median of each log ratio over several frames.
pub fn median(ds: &[BlurDir]) -> BlurDir {
    let med = |f: &dyn Fn(&BlurDir) -> f64| crate::img::median(&ds.iter().map(f).collect::<Vec<_>>());
    let mut axis = [0.0; 8];
    for (k, a) in axis.iter_mut().enumerate() {
        *a = med(&|d: &BlurDir| d.axis[k]);
    }
    BlurDir { axis, radial: med(&|d| d.radial), tangential: med(&|d| d.tangential) }
}

/// Fallback conversions from the Gaussian-equivalent sigma the blur stage
/// measures (% of height) to the main parameter of each kind, for when no
/// magnitude was measured ([`magnitude`] is the measurement). Read off renders
/// of the engine's effects at moderate strengths; the equivalent sigma of a
/// streak hardly grows with its length (the other orientations keep their
/// detail), so these are rough.
pub const DIRECTIONAL_LENGTH_PER_SIGMA: f64 = 21.0;
/// `zoom_blur.strength` per % of height of equivalent sigma.
pub const ZOOM_STRENGTH_PER_SIGMA: f64 = 0.8;
/// `spin_blur.angle` (degrees) per % of height of equivalent sigma.
pub const SPIN_DEGREES_PER_SIGMA: f64 = 42.0;

/// The parameter of `kind` that gives the measured equivalent sigma (% of height).
pub fn param_for(kind: &str, sigma_pct: f64) -> (&'static str, &'static str, f64) {
    match kind {
        "directional" => ("directional_blur", "length", sigma_pct * DIRECTIONAL_LENGTH_PER_SIGMA),
        "zoom" => ("zoom_blur", "strength", sigma_pct * ZOOM_STRENGTH_PER_SIGMA),
        "spin" => ("spin_blur", "angle", sigma_pct * SPIN_DEGREES_PER_SIGMA),
        _ => ("gaussian_blur", "radius", sigma_pct),
    }
}

/// The renderer parameter of a streak blur of `kind` that removes as much
/// gradient energy from `reference` as the frame lost (`observed_loss`, the
/// mean log energy loss of [`stats`]), found by blurring the reference frame
/// itself with [`synth`] and bisecting: the picture's own detail sets how
/// much a streak of a given length takes away, so it cancels.
/// Directional: `length` in % of height along `angle`; zoom: `strength`;
/// spin: `angle` in degrees.
pub fn magnitude(kind: &str, angle: f64, observed_loss: f64, reference: &Gray) -> Option<f64> {
    let base = orientation(reference, None);
    let h = reference.h as f64;
    let loss_at = |p: f64| -> f64 {
        let blurred = match kind {
            "directional" => synth::directional(reference, angle, p * h / 100.0),
            "zoom" => synth::zoom(reference, p),
            "spin" => synth::spin(reference, p),
            _ => return 0.0,
        };
        stats(&ratios(&orientation(&blurred, None), &base)).loss
    };
    let (mut lo, mut hi) = match kind {
        "directional" => (0.1, 40.0),
        "zoom" => (0.005, 1.5),
        "spin" => (0.5, 120.0),
        _ => return None,
    };
    if loss_at(hi) < observed_loss {
        return Some(hi);
    }
    if loss_at(lo) > observed_loss {
        return Some(lo);
    }
    // Bisect in log space: the loss grows about with the log of the streak.
    for _ in 0..10 {
        let mid = (lo * hi).sqrt();
        if loss_at(mid) < observed_loss {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some((lo * hi).sqrt())
}

/// CPU blurs of the three streaking kinds, for tests (bilinear samples
/// along the streak, centred on the pixel).
pub mod synth {
    use crate::img::Gray;

    fn sample(g: &Gray, x: f64, y: f64) -> f32 {
        let x = x.clamp(0.0, g.w as f64 - 1.0);
        let y = y.clamp(0.0, g.h as f64 - 1.0);
        g.sample(x as f32, y as f32).unwrap_or(0.0)
    }

    /// Smear along `angle` degrees (clockwise from +x) over `len` pixels.
    pub fn directional(g: &Gray, angle: f64, len: f64) -> Gray {
        let (c, s) = (angle.to_radians().cos(), angle.to_radians().sin());
        let taps = (len.ceil() as usize * 2 + 1).max(3);
        let mut out = g.clone();
        for y in 0..g.h {
            for x in 0..g.w {
                let mut acc = 0.0;
                for t in 0..taps {
                    let u = (t as f64 / (taps - 1) as f64 - 0.5) * len;
                    acc += sample(g, x as f64 + u * c, y as f64 + u * s);
                }
                out.d[y * g.w + x] = acc / taps as f32;
            }
        }
        out
    }

    /// Smear along the rays from the centre: at radius r over `strength * r` pixels.
    pub fn zoom(g: &Gray, strength: f64) -> Gray {
        let (cx, cy) = ((g.w as f64 - 1.0) / 2.0, (g.h as f64 - 1.0) / 2.0);
        let taps = 25;
        let mut out = g.clone();
        for y in 0..g.h {
            for x in 0..g.w {
                let mut acc = 0.0;
                for t in 0..taps {
                    let k = 1.0 + (t as f64 / (taps - 1) as f64 - 0.5) * strength;
                    acc += sample(g, cx + (x as f64 - cx) * k, cy + (y as f64 - cy) * k);
                }
                out.d[y * g.w + x] = acc / taps as f32;
            }
        }
        out
    }

    /// Smear along circles about the centre over `degrees`.
    pub fn spin(g: &Gray, degrees: f64) -> Gray {
        let (cx, cy) = ((g.w as f64 - 1.0) / 2.0, (g.h as f64 - 1.0) / 2.0);
        let taps = 25;
        let mut out = g.clone();
        for y in 0..g.h {
            for x in 0..g.w {
                let mut acc = 0.0;
                for t in 0..taps {
                    let a = ((t as f64 / (taps - 1) as f64 - 0.5) * degrees).to_radians();
                    let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                    acc += sample(g, cx + dx * a.cos() - dy * a.sin(), cy + dx * a.sin() + dy * a.cos());
                }
                out.d[y * g.w + x] = acc / taps as f32;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> Gray {
        let mut c = crate::synth::Clip::new(192, 192, 1);
        c.noise = 1.0;
        c.render().gray(0)
    }

    fn kind_of(blurred: &Gray, sharp: &Gray) -> (&'static str, f64, Stats) {
        classify(&ratios(&orientation(blurred, None), &orientation(sharp, None))).expect("blur seen")
    }

    #[test]
    fn the_four_kinds_are_told_apart() {
        let sharp = picture();
        let (k, c, s) = kind_of(&crate::img::gaussian_blur(&sharp, 2.5), &sharp);
        assert_eq!(k, "gaussian", "{s:?}");
        assert!(c > 0.3, "{c}");
        for angle in [0.0, 30.0, 90.0, 135.0] {
            let (k, _, s) = kind_of(&synth::directional(&sharp, angle, 10.0), &sharp);
            assert_eq!(k, "directional", "angle {angle}: {s:?}");
            let err = ((s.angle - angle + 90.0).rem_euclid(180.0) - 90.0).abs();
            assert!(err < 12.0, "angle {angle} read {:.1}", s.angle);
        }
        let (k, _, s) = kind_of(&synth::zoom(&sharp, 0.12), &sharp);
        assert_eq!(k, "zoom", "{s:?}");
        let (k, _, s) = kind_of(&synth::spin(&sharp, 8.0), &sharp);
        assert_eq!(k, "spin", "{s:?}");
    }

    #[test]
    fn a_sharp_frame_or_a_shifted_one_is_not_a_blur() {
        let sharp = picture();
        assert!(classify(&ratios(&orientation(&sharp, None), &orientation(&sharp, None))).is_none());
        // A whole-frame gain changes every orientation alike: still no blur.
        let mut bright = sharp.clone();
        bright.d.iter_mut().for_each(|v| *v *= 1.6);
        assert!(classify(&ratios(&orientation(&bright, None), &orientation(&sharp, None))).is_none());
    }
}
