//! Tempo and beat phase from an onset envelope.
//!
//! The envelope is the spectral flux of the audio track (or, without audio,
//! the frame-difference signal of the picture). Tempo comes from the
//! autocorrelation of the envelope weighted by a broad prior around 120 BPM,
//! then period and phase are refined together by maximising the envelope
//! sampled on a beat comb.

use rustfft::{num_complex::Complex, FftPlanner};

#[derive(Clone, Debug)]
pub struct Onset {
    /// Envelope samples per second.
    pub rate: f64,
    /// Time of sample 0 in seconds.
    pub t0: f64,
    pub env: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct TempoOpts {
    pub min_bpm: f64,
    pub max_bpm: f64,
    /// Use this tempo instead of estimating it.
    pub bpm: Option<f64>,
}
impl Default for TempoOpts {
    fn default() -> Self {
        Self { min_bpm: 60.0, max_bpm: 200.0, bpm: None }
    }
}

#[derive(Clone, Debug)]
pub struct Tempo {
    pub bpm: f64,
    /// Before snapping to a whole number of BPM.
    pub bpm_raw: f64,
    /// Time in seconds of one beat (0 <= phase < period).
    pub phase: f64,
    pub confidence: f64,
    /// (bpm, score) of autocorrelation peaks, best first.
    pub candidates: Vec<(f64, f64)>,
}

const N_FFT: usize = 1024;
const HOP: usize = 256;

/// Log-compressed spectral flux of mono audio.
pub fn onset_envelope(samples: &[f32], sr: u32) -> Onset {
    let rate = sr as f64 / HOP as f64;
    // An onset reaches full weight in the analysis window about three hops
    // after it enters; this constant was set on synthetic click trains.
    let t0 = (N_FFT as f64 - 1.5 * HOP as f64) / sr as f64;
    if samples.len() < N_FFT * 2 {
        return Onset { rate, t0, env: Vec::new() };
    }
    let peak = samples.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    let hann: Vec<f32> = (0..N_FFT).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N_FFT as f32).cos()).collect();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N_FFT);
    let frames = (samples.len() - N_FFT) / HOP + 1;
    let bins = N_FFT / 2;
    let mut prev = vec![0.0f32; bins];
    let mut buf = vec![Complex::new(0.0f32, 0.0); N_FFT];
    let mut env = Vec::with_capacity(frames);
    for t in 0..frames {
        let s = &samples[t * HOP..t * HOP + N_FFT];
        for i in 0..N_FFT {
            buf[i] = Complex::new(s[i] / peak * hann[i], 0.0);
        }
        fft.process(&mut buf);
        let mut flux = 0.0f32;
        for k in 1..bins {
            let m = (1.0 + 100.0 * buf[k].norm() / (N_FFT as f32 / 4.0)).ln();
            let d = m - prev[k];
            if d > 0.0 {
                flux += d;
            }
            prev[k] = m;
        }
        env.push(if t == 0 { 0.0 } else { flux });
    }
    Onset { rate, t0, env: normalise_envelope(&env, rate) }
}

/// Remove the slow trend (0.4 s moving mean), keep the positive part, unit variance.
pub fn normalise_envelope(env: &[f32], rate: f64) -> Vec<f32> {
    let n = env.len();
    let half = ((0.2 * rate).round() as usize).max(1);
    let mut cum = vec![0.0f64; n + 1];
    for i in 0..n {
        cum[i + 1] = cum[i] + env[i] as f64;
    }
    let mut out: Vec<f32> = (0..n)
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + half + 1).min(n);
            (env[i] as f64 - (cum[b] - cum[a]) / (b - a) as f64).max(0.0) as f32
        })
        .collect();
    let m = out.iter().sum::<f32>() / n.max(1) as f32;
    let sd = (out.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / n.max(1) as f32).sqrt().max(1e-9);
    out.iter_mut().for_each(|v| *v /= sd);
    out
}

fn interp(env: &[f32], x: f64) -> f64 {
    if x < 0.0 || x >= (env.len() - 1) as f64 {
        return 0.0;
    }
    let i = x as usize;
    let f = x - i as f64;
    env[i] as f64 * (1.0 - f) + env[i + 1] as f64 * f
}

/// Mean envelope on a beat comb of period `p` samples, maximised over phase.
/// Returns (response, phase in samples).
fn comb(env: &[f32], p: f64, phase_step: f64) -> (f64, f64) {
    let n = env.len() as f64;
    let steps = (p / phase_step).ceil() as usize;
    let mut best = (f64::MIN, 0.0);
    for s in 0..steps {
        let ph = s as f64 * phase_step;
        let mut acc = 0.0;
        let mut k = 0usize;
        let mut x = ph;
        while x < n - 1.0 {
            // A little width so a comb tooth does not fall between samples.
            acc += interp(env, x);
            k += 1;
            x += p;
        }
        let v = if k > 0 { acc / k as f64 } else { 0.0 };
        if v > best.0 {
            best = (v, ph);
        }
    }
    best
}

/// Estimate tempo and beat phase from an onset envelope.
pub fn tempo(onset: &Onset, opts: &TempoOpts) -> Option<Tempo> {
    let env = &onset.env;
    let n = env.len();
    if n < 16 {
        return None;
    }
    let rate = onset.rate;
    // Smooth slightly so the comb is tolerant of half-sample jitter.
    let smooth: Vec<f32> = (0..n).map(|i| 0.25 * env[i.saturating_sub(1)] + 0.5 * env[i] + 0.25 * env[(i + 1).min(n - 1)]).collect();
    let mut candidates: Vec<(f64, f64)> = Vec::new();
    let (period, confidence) = if let Some(bpm) = opts.bpm {
        (rate * 60.0 / bpm, 1.0)
    } else {
        let lo = (rate * 60.0 / opts.max_bpm).floor().max(2.0) as usize;
        let hi = ((rate * 60.0 / opts.min_bpm).ceil() as usize).min(n / 2);
        if hi <= lo + 1 {
            return None;
        }
        let r0: f64 = env.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / n as f64;
        let acf: Vec<f64> = (lo..=hi)
            .map(|lag| (0..n - lag).map(|i| env[i] as f64 * env[i + lag] as f64).sum::<f64>() / (n - lag) as f64 / r0.max(1e-12))
            .collect();
        let weight = |lag: f64| {
            let bpm = rate * 60.0 / lag;
            (-0.5 * ((bpm / 120.0).log2() / 0.8).powi(2)).exp()
        };
        let score: Vec<f64> = acf.iter().enumerate().map(|(i, a)| a * weight((lo + i) as f64)).collect();
        for i in 0..score.len() {
            let l = if i > 0 { score[i - 1] } else { f64::MIN };
            let r = if i + 1 < score.len() { score[i + 1] } else { f64::MIN };
            if score[i] > l && score[i] >= r && score[i] > 0.0 {
                // Parabolic refinement of the peak position.
                let d = if i > 0 && i + 1 < score.len() {
                    let den = l - 2.0 * score[i] + r;
                    if den.abs() > 1e-12 {
                        (0.5 * (l - r) / den).clamp(-0.5, 0.5)
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                candidates.push((rate * 60.0 / ((lo + i) as f64 + d), score[i]));
            }
        }
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        candidates.truncate(5);
        let best = *candidates.first()?;
        let raw_acf = best.1 / weight(rate * 60.0 / best.0);
        let rival = candidates.get(1).map(|c| c.1 / best.1).unwrap_or(0.0);
        let conf = (raw_acf * 2.5).clamp(0.0, 1.0) * (1.0 - 0.5 * rival.clamp(0.0, 1.0));
        (rate * 60.0 / best.0, conf)
    };

    // Joint refinement of period and phase on the comb.
    let phase_step = 0.25;
    let mut best = (f64::MIN, period, 0.0);
    if opts.bpm.is_some() {
        let (v, ph) = comb(&smooth, period, phase_step);
        best = (v, period, ph);
    } else {
        for i in -60i32..=60 {
            let p = period * (1.0 + i as f64 * 0.0005);
            let (v, ph) = comb(&smooth, p, phase_step);
            if v > best.0 {
                best = (v, p, ph);
            }
        }
    }
    let bpm_raw = rate * 60.0 / best.1;
    let mut bpm = bpm_raw;
    let mut phase_samples = best.2;
    if opts.bpm.is_none() {
        // Music is usually cut to a whole tempo: take it when it explains the
        // envelope as well as the raw optimum.
        let whole = bpm_raw.round();
        if (whole - bpm_raw).abs() < 0.02 * bpm_raw {
            let (v, ph) = comb(&smooth, rate * 60.0 / whole, phase_step);
            if v >= 0.97 * best.0 {
                bpm = whole;
                phase_samples = ph;
            }
        }
    }
    let period_s = 60.0 / bpm;
    let phase = (onset.t0 + phase_samples / rate).rem_euclid(period_s);
    Some(Tempo { bpm, bpm_raw, phase, confidence, candidates })
}

/// Beat-comb response for a range of tempi: (bpm, mean envelope on the comb,
/// phase in seconds). For inspection and for choosing between candidates.
pub fn comb_scan(onset: &Onset, bpm_lo: f64, bpm_hi: f64, step: f64) -> Vec<(f64, f64, f64)> {
    let n = onset.env.len();
    let smooth: Vec<f32> = (0..n).map(|i| 0.25 * onset.env[i.saturating_sub(1)] + 0.5 * onset.env[i] + 0.25 * onset.env[(i + 1).min(n - 1)]).collect();
    let mut out = Vec::new();
    let mut bpm = bpm_lo;
    while bpm <= bpm_hi + 1e-9 {
        let (v, ph) = comb(&smooth, onset.rate * 60.0 / bpm, 0.25);
        out.push((bpm, v, (onset.t0 + ph / onset.rate).rem_euclid(60.0 / bpm)));
        bpm += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clicks on the beat, quieter clicks on the off-beat, and noise.
    pub fn click_track(bpm: f64, first: f64, seconds: f64, sr: u32) -> Vec<f32> {
        let n = (seconds * sr as f64) as usize;
        let mut s = vec![0.0f32; n];
        let mut seed = 12345u32;
        let mut rnd = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        for v in s.iter_mut() {
            *v = 0.02 * rnd();
        }
        let period = 60.0 / bpm;
        let mut k = 0;
        loop {
            let t = first + k as f64 * period * 0.5;
            let i0 = (t * sr as f64) as usize;
            if i0 + 2000 >= n {
                break;
            }
            let amp = if k % 2 == 0 { 0.8 } else { 0.25 };
            for j in 0..2000 {
                let e = (-(j as f32) / 300.0).exp();
                let f = if k % 2 == 0 { 90.0 } else { 3000.0 };
                s[i0 + j] += amp * e * (std::f32::consts::TAU * f * j as f32 / sr as f32).sin() + amp * 0.3 * e * rnd();
            }
            k += 1;
        }
        s
    }

    #[test]
    fn recovers_tempo_and_phase_of_a_click_track() {
        for (bpm, first) in [(123.0, 0.48), (96.0, 0.2), (150.0, 0.05)] {
            let sr = 22050;
            let audio = click_track(bpm, first, 17.0, sr);
            let onset = onset_envelope(&audio, sr);
            let t = tempo(&onset, &TempoOpts::default()).unwrap();
            assert!((t.bpm - bpm).abs() < 0.5, "bpm {} vs {bpm}", t.bpm);
            let period = 60.0 / bpm;
            let mut d = (t.phase - first).rem_euclid(period);
            if d > period / 2.0 {
                d -= period;
            }
            assert!(d.abs() < 0.015, "phase error {d} s at {bpm} bpm");
            assert!(t.confidence > 0.3);
        }
    }

    #[test]
    fn override_keeps_the_given_tempo() {
        let sr = 22050;
        let audio = click_track(123.0, 0.3, 10.0, sr);
        let onset = onset_envelope(&audio, sr);
        let t = tempo(&onset, &TempoOpts { bpm: Some(123.0), ..Default::default() }).unwrap();
        assert_eq!(t.bpm, 123.0);
        let mut d = (t.phase - 0.3).rem_euclid(60.0 / 123.0);
        if d > 0.25 {
            d -= 60.0 / 123.0;
        }
        assert!(d.abs() < 0.015, "phase {d}");
    }
}
