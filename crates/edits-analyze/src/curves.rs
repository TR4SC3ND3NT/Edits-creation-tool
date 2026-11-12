//! Turn a sampled track into keyframes with easing.
//!
//! The samples of one property over a shot are split into segments by dynamic
//! programming; every candidate segment is fitted with each easing the
//! renderer knows (`hold`, `linear`, `out`, `in_out`, `in`, `expo_out`, and
//! `decay` with a fitted rate) using `edits_core::ease::apply`, so a fitted
//! curve replays exactly as fitted. A penalty per segment keeps the key count
//! low. Key values are then solved jointly (adjacent segments share a key).

use edits_core::ease;
use edits_core::Ease;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct FitKey {
    /// Sample index of the key.
    pub idx: usize,
    pub v: Vec<f64>,
    /// Easing from this key to the next.
    pub ease: Ease,
    /// RMS residual of that segment.
    pub rms: f64,
    /// Second-best easing family for the segment and its RMS.
    pub runner_up: Option<(Ease, f64)>,
}

#[derive(Clone, Debug)]
pub struct Fitted {
    pub keys: Vec<FitKey>,
    /// RMS residual over all valid samples.
    pub rms: f64,
    /// The fitted value at every sample.
    pub values: Vec<Vec<f64>>,
}

impl Fitted {
    pub fn is_constant(&self) -> bool {
        self.keys.len() <= 1
    }
}

pub fn decay(rate: f64) -> Ease {
    let mut args = BTreeMap::new();
    args.insert("rate".to_string(), (rate * 1000.0).round() / 1000.0);
    Ease::Params { name: "decay".into(), args }
}

pub fn named(name: &str) -> Ease {
    Ease::Name(name.into())
}

pub fn family(e: &Ease) -> String {
    match e {
        Ease::Name(n) | Ease::Params { name: n, .. } => n.clone(),
        Ease::Bezier(_) => "bezier".into(),
    }
}

const DECAY_RATES: [f64; 8] = [0.06, 0.1, 0.16, 0.25, 0.37, 0.5, 0.65, 0.8];

/// The fixed-shape easings tried on a segment of `len` frames.
fn candidates(len: usize) -> Vec<Ease> {
    let mut v = vec![named("hold"), named("linear")];
    if len >= 3 {
        v.extend([named("out"), named("in_out"), named("in"), named("expo_out")]);
    }
    v
}

/// Same formula as `edits_core::ease` uses for `decay`, without the lookups.
fn decay_basis(rate: f64, len: usize) -> Vec<f64> {
    let k = 1.0 - rate.clamp(1e-4, 0.9999);
    let n = len as f64;
    let end = 1.0 - k.powf(n);
    (0..=len).map(|t| if end <= 1e-9 { t as f64 / n } else { (1.0 - k.powi(t as i32)) / end }).collect()
}

/// Best `decay` rate for a segment: coarse grid, then golden-section search.
fn best_decay(y: &[Vec<f64>], w: &[f64], i: usize, j: usize) -> (f64, f64) {
    let len = j - i;
    let sse = |r: f64| fit_segment(y, w, i, j, &decay_basis(r, len), false).0;
    let coarse: Vec<f64> = DECAY_RATES.iter().map(|r| sse(*r)).collect();
    let b = (0..coarse.len()).min_by(|a, c| coarse[*a].partial_cmp(&coarse[*c]).unwrap()).unwrap();
    let (mut lo, mut hi) = (DECAY_RATES[b.saturating_sub(1)], DECAY_RATES[(b + 1).min(DECAY_RATES.len() - 1)]);
    let g = 0.618_033_988_75;
    let (mut x1, mut x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
    let (mut f1, mut f2) = (sse(x1), sse(x2));
    for _ in 0..9 {
        if f1 < f2 {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = hi - g * (hi - lo);
            f1 = sse(x1);
        } else {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = lo + g * (hi - lo);
            f2 = sse(x2);
        }
    }
    let (r, f) = if f1 < f2 { (x1, f1) } else { (x2, f2) };
    if f < coarse[b] {
        (f, r)
    } else {
        (coarse[b], DECAY_RATES[b])
    }
}

fn basis(e: &Ease, len: usize) -> Vec<f64> {
    let hold = family(e) == "hold";
    (0..=len).map(|t| if hold { 0.0 } else { ease::apply(e, t as f64 / len as f64, len as f64) }).collect()
}

/// Least-squares endpoints of one segment `i..=j` for a basis; returns (sse, a, b).
fn fit_segment(y: &[Vec<f64>], w: &[f64], i: usize, j: usize, e: &[f64], hold: bool) -> (f64, Vec<f64>, Vec<f64>) {
    let dims = y[0].len();
    let last = if hold { j - 1 } else { j };
    let (mut s11, mut s12, mut s22, mut sw) = (0.0, 0.0, 0.0, 0.0);
    for t in i..=last {
        let (p, q) = (1.0 - e[t - i], e[t - i]);
        s11 += w[t] * p * p;
        s12 += w[t] * p * q;
        s22 += w[t] * q * q;
        sw += w[t];
    }
    let det = s11 * s22 - s12 * s12;
    let (mut a, mut b) = (vec![0.0; dims], vec![0.0; dims]);
    let mut sse = 0.0;
    for d in 0..dims {
        let (mut b1, mut b2, mut sy) = (0.0, 0.0, 0.0);
        for t in i..=last {
            b1 += w[t] * (1.0 - e[t - i]) * y[t][d];
            b2 += w[t] * e[t - i] * y[t][d];
            sy += w[t] * y[t][d];
        }
        if sw <= 0.0 {
            a[d] = f64::NAN;
            b[d] = f64::NAN;
            continue;
        }
        if hold || det < 1e-9 * (s11 + s22).max(1e-12).powi(2) {
            a[d] = sy / sw;
            b[d] = a[d];
        } else {
            a[d] = (b1 * s22 - b2 * s12) / det;
            b[d] = (b2 * s11 - b1 * s12) / det;
        }
        for t in i..=last {
            let pred = a[d] * (1.0 - e[t - i]) + b[d] * e[t - i];
            sse += w[t] * (y[t][d] - pred).powi(2);
        }
    }
    (sse, a, b)
}

/// Fit keyframes to `y` (one vector per frame). `w[i] = 0` marks a sample to
/// ignore (not registered, duplicate frame). `tol` is the residual that does
/// not matter, in the unit of the samples.
pub fn fit(y: &[Vec<f64>], w: &[f64], tol: f64) -> Fitted {
    let n = y.len();
    let dims = y.first().map(|v| v.len()).unwrap_or(1);
    if n == 0 {
        return Fitted { keys: vec![], rms: 0.0, values: vec![] };
    }
    let wsum: f64 = w.iter().sum();
    if n == 1 || wsum < 1.0 {
        let v = if wsum > 0.0 { (0..dims).map(|d| (0..n).map(|t| w[t] * y[t][d]).sum::<f64>() / wsum).collect() } else { vec![0.0; dims] };
        return Fitted { keys: vec![FitKey { idx: 0, v: v.clone(), ease: named("hold"), rms: 0.0, runner_up: None }], rms: 0.0, values: vec![v; n] };
    }
    // Noise level from second differences of valid neighbours, bounded by the tolerance.
    let mut d2: Vec<f64> = Vec::new();
    for t in 1..n - 1 {
        if w[t - 1] > 0.0 && w[t] > 0.0 && w[t + 1] > 0.0 {
            for d in 0..dims {
                d2.push((y[t + 1][d] - 2.0 * y[t][d] + y[t - 1][d]).powi(2));
            }
        }
    }
    let noise2 = if d2.is_empty() { 0.0 } else { crate::img::median(&d2) / (6.0 * 0.455) };
    let s2 = noise2.clamp(tol * tol, 9.0 * tol * tol);
    let pen = (dims as f64 + 2.0) * (n as f64).ln().max(1.0);

    // Dynamic programme over break points.
    let mut bases: BTreeMap<usize, Vec<(Ease, Vec<f64>)>> = BTreeMap::new();
    let mut best = vec![f64::MAX; n];
    let mut back: Vec<(usize, Ease)> = vec![(0, named("hold")); n];
    best[0] = 0.0;
    for j in 1..n {
        for i in 0..j {
            if best[i] == f64::MAX {
                continue;
            }
            let len = j - i;
            let cands = bases.entry(len).or_insert_with(|| candidates(len).into_iter().map(|e| (e.clone(), basis(&e, len))).collect());
            for (e, b) in cands.iter() {
                let hold = family(e) == "hold";
                let (sse, _, _) = fit_segment(y, w, i, j, b, hold);
                // Eased shapes cost a little more than a straight line or a hold.
                let shape = if hold || family(e) == "linear" { 0.0 } else { 1.0 };
                let c = best[i] + sse / s2 + pen + shape;
                if c < best[j] {
                    best[j] = c;
                    back[j] = (i, e.clone());
                }
            }
            if len >= 3 {
                let (sse, rate) = best_decay(y, w, i, j);
                let c = best[i] + sse / s2 + pen + 1.0;
                if c < best[j] {
                    best[j] = c;
                    back[j] = (i, decay(rate));
                }
            }
        }
    }
    let mut idx = vec![n - 1];
    let mut eases = Vec::new();
    let mut j = n - 1;
    while j > 0 {
        let (i, e) = back[j].clone();
        idx.push(i);
        eases.push(e);
        j = i;
    }
    idx.reverse();
    eases.reverse();

    // Refine decay rates, then pick the runner-up family per segment.
    let mut alts: Vec<Option<(Ease, f64)>> = Vec::new();
    for s in 0..eases.len() {
        let (i, j) = (idx[s], idx[s + 1]);
        let len = j - i;
        let seg_w: f64 = (i..=j).map(|t| w[t]).sum::<f64>().max(1.0) * dims as f64;
        let fam = family(&eases[s]);
        let mut alt: Option<(Ease, f64)> = None;
        let mut others: Vec<(Ease, f64)> = candidates(len).into_iter().map(|e| (e.clone(), fit_segment(y, w, i, j, &basis(&e, len), false).0)).collect();
        if len >= 3 {
            let (sse, rate) = best_decay(y, w, i, j);
            others.push((decay(rate), sse));
        }
        for (e, sse) in others {
            let f = family(&e);
            if f == fam || f == "hold" {
                continue;
            }
            let rms = (sse / seg_w).sqrt();
            if alt.as_ref().map(|a| rms < a.1).unwrap_or(true) {
                alt = Some((e, rms));
            }
        }
        alts.push(alt);
    }

    let mut keys = solve_keys(y, w, &idx, &eases, tol);
    for (s, a) in alts.into_iter().enumerate() {
        if family(&keys[s].ease) != "hold" {
            keys[s].runner_up = a;
        }
    }
    // Drop keys that do nothing: flat segments next to each other.
    let flat = |a: &[f64], b: &[f64]| a.iter().zip(b).all(|(p, q)| (p - q).abs() < tol);
    let mut changed = true;
    while changed && keys.len() > 2 {
        changed = false;
        for s in 1..keys.len() - 1 {
            if flat(&keys[s - 1].v, &keys[s].v) && flat(&keys[s].v, &keys[s + 1].v) {
                keys.remove(s);
                changed = true;
                break;
            }
        }
    }
    if keys.len() >= 2 && (0..keys.len() - 1).all(|s| flat(&keys[s].v, &keys[s + 1].v)) {
        keys.truncate(1);
        keys[0].ease = named("hold");
        keys[0].runner_up = None;
    } else {
        let idx2: Vec<usize> = keys.iter().map(|k| k.idx).collect();
        let eases2: Vec<Ease> = keys[..keys.len() - 1].iter().map(|k| k.ease.clone()).collect();
        let ru: Vec<Option<(Ease, f64)>> = keys.iter().map(|k| k.runner_up.clone()).collect();
        keys = solve_keys(y, w, &idx2, &eases2, tol);
        for (k, r) in keys.iter_mut().zip(ru) {
            k.runner_up = r;
        }
    }
    let values = evaluate(&keys, n);
    let (mut sse, mut cnt) = (0.0, 0.0);
    for t in 0..n {
        for d in 0..dims {
            sse += w[t] * (y[t][d] - values[t][d]).powi(2);
            cnt += w[t];
        }
    }
    Fitted { keys, rms: (sse / cnt.max(1.0)).sqrt(), values }
}

/// Solve all key values at once for fixed key positions and easings.
fn solve_keys(y: &[Vec<f64>], w: &[f64], idx: &[usize], eases: &[Ease], tol: f64) -> Vec<FitKey> {
    let dims = y[0].len();
    let k = idx.len();
    let mut ata = vec![vec![0.0f64; k]; k];
    let mut atb = vec![vec![0.0f64; dims]; k];
    let bases: Vec<Vec<f64>> = eases.iter().enumerate().map(|(s, e)| basis(e, idx[s + 1] - idx[s])).collect();
    for s in 0..eases.len() {
        let (i, j) = (idx[s], idx[s + 1]);
        // The sample on a shared key belongs to the segment it starts (the last key closes the last segment).
        let last = if s + 1 == eases.len() { j } else { j - 1 };
        for t in i..=last {
            let q = if t == j { 1.0 } else { bases[s][t - i] };
            let p = 1.0 - q;
            ata[s][s] += w[t] * p * p;
            ata[s][s + 1] += w[t] * p * q;
            ata[s + 1][s] += w[t] * p * q;
            ata[s + 1][s + 1] += w[t] * q * q;
            for d in 0..dims {
                atb[s][d] += w[t] * p * y[t][d];
                atb[s + 1][d] += w[t] * q * y[t][d];
            }
        }
        // A weak pull between neighbouring keys keeps unobserved keys defined.
        let ridge = 1e-4;
        ata[s][s] += ridge;
        ata[s + 1][s + 1] += ridge;
        ata[s][s + 1] -= ridge;
        ata[s + 1][s] -= ridge;
    }
    let mut vals = vec![vec![0.0f64; dims]; k];
    for d in 0..dims {
        let mut a = ata.clone();
        let mut b: Vec<f64> = (0..k).map(|r| atb[r][d]).collect();
        for i in 0..k {
            let mut p = i;
            for r in i + 1..k {
                if a[r][i].abs() > a[p][i].abs() {
                    p = r;
                }
            }
            a.swap(i, p);
            b.swap(i, p);
            if a[i][i].abs() < 1e-14 {
                continue;
            }
            for r in i + 1..k {
                let f = a[r][i] / a[i][i];
                if f != 0.0 {
                    for c in i..k {
                        a[r][c] -= f * a[i][c];
                    }
                    b[r] -= f * b[i];
                }
            }
        }
        for i in (0..k).rev() {
            let mut s = b[i];
            for c in i + 1..k {
                s -= a[i][c] * vals[c][d];
            }
            vals[i][d] = if a[i][i].abs() < 1e-14 { 0.0 } else { s / a[i][i] };
        }
    }
    let mut keys: Vec<FitKey> = (0..k)
        .map(|s| FitKey { idx: idx[s], v: vals[s].clone(), ease: if s < eases.len() { eases[s].clone() } else { named("linear") }, rms: 0.0, runner_up: None })
        .collect();
    for s in 0..eases.len() {
        let (i, j) = (idx[s], idx[s + 1]);
        let (mut sse, mut cnt) = (0.0, 0.0);
        for t in i..=j {
            if t == j && family(&eases[s]) == "hold" {
                continue;
            }
            let q = bases[s][t - i];
            for d in 0..dims {
                sse += w[t] * (y[t][d] - (vals[s][d] * (1.0 - q) + vals[s + 1][d] * q)).powi(2);
                cnt += w[t];
            }
        }
        keys[s].rms = (sse / cnt.max(1.0)).sqrt();
        // A segment that does not move is a hold.
        if family(&eases[s]) != "hold" && vals[s].iter().zip(&vals[s + 1]).all(|(p, q)| (p - q).abs() < tol) {
            keys[s].ease = named("hold");
        }
    }
    keys
}

/// Evaluate fitted keys at samples `0..n` exactly as the renderer would.
pub fn evaluate(keys: &[FitKey], n: usize) -> Vec<Vec<f64>> {
    let mut out = Vec::with_capacity(n);
    for t in 0..n {
        let v = if keys.len() == 1 || t <= keys[0].idx {
            keys[0].v.clone()
        } else if t >= keys[keys.len() - 1].idx {
            keys[keys.len() - 1].v.clone()
        } else {
            let s = (0..keys.len() - 1).rev().find(|s| keys[*s].idx <= t).unwrap_or(0);
            let (k0, k1) = (&keys[s], &keys[s + 1]);
            let len = (k1.idx - k0.idx) as f64;
            let f = ease::apply(&k0.ease, (t - k0.idx) as f64 / len, len);
            k0.v.iter().zip(&k1.v).map(|(a, b)| a + (b - a) * f).collect()
        };
        out.push(v);
    }
    out
}

/// Residual shake of a fitted 2-D position track: RMS per axis and the ratio
/// of the RMS in the second half of the shot to the first half.
pub fn shake(y: &[Vec<f64>], w: &[f64], fitted: &Fitted) -> (f64, f64, f64) {
    let n = y.len();
    let rms = |d: usize, a: usize, b: usize| {
        let (mut s, mut c) = (0.0, 0.0);
        for t in a..b {
            s += w[t] * (y[t][d] - fitted.values[t][d]).powi(2);
            c += w[t];
        }
        (s / c.max(1.0)).sqrt()
    };
    let early = (rms(0, 0, n / 2).powi(2) + rms(1, 0, n / 2).powi(2)).sqrt();
    let late = (rms(0, n / 2, n).powi(2) + rms(1, n / 2, n).powi(2)).sqrt();
    (rms(0, 0, n), rms(1, 0, n), if early > 1e-9 { late / early } else { 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 33) as f64 / (1u64 << 31) as f64) - 0.5
    }

    #[test]
    fn decay_punch_then_linear_drift() {
        // Scale: 1.35 -> 1.05 with a 0.39 decay over 10 frames, then a linear drift to 1.12 over 30.
        let mut seed = 7;
        let e = decay(0.39);
        let mut y = Vec::new();
        for t in 0..=40 {
            let v = if t <= 10 { 1.35 + (1.05 - 1.35) * ease::apply(&e, t as f64 / 10.0, 10.0) } else { 1.05 + 0.07 * (t - 10) as f64 / 30.0 };
            y.push(vec![v + 0.002 * noise(&mut seed)]);
        }
        let mut w = vec![1.0; y.len()];
        w[4] = 0.0; // a duplicate frame is ignored
        y[4][0] = y[3][0];
        let f = fit(&y, &w, 0.002);
        assert_eq!(f.keys.len(), 3, "{:?}", f.keys);
        assert_eq!(family(&f.keys[0].ease), "decay");
        let rate = match &f.keys[0].ease {
            Ease::Params { args, .. } => args["rate"],
            _ => 0.0,
        };
        assert!((rate - 0.39).abs() < 0.06, "rate {rate}");
        assert!((f.keys[0].v[0] - 1.35).abs() < 0.01 && (f.keys[2].v[0] - 1.12).abs() < 0.01);
        assert!((f.keys[1].idx as i32 - 10).abs() <= 2, "middle key at {}", f.keys[1].idx);
        assert_eq!(family(&f.keys[1].ease), "linear");
        assert!(f.rms < 0.003);
        assert!(f.keys[0].runner_up.is_some());
    }

    #[test]
    fn a_still_shot_is_one_key_and_a_jump_is_a_hold() {
        let mut seed = 3;
        let y: Vec<Vec<f64>> = (0..20).map(|_| vec![1.0 + 0.0005 * noise(&mut seed)]).collect();
        let f = fit(&y, &vec![1.0; 20], 0.002);
        assert!(f.is_constant());
        let y: Vec<Vec<f64>> = (0..20).map(|t| vec![if t < 8 { 1.0 } else { 1.3 } + 0.0005 * noise(&mut seed)]).collect();
        let f = fit(&y, &vec![1.0; 20], 0.002);
        assert_eq!(f.keys.len(), 3, "{:?}", f.keys);
        assert_eq!(family(&f.keys[0].ease), "hold");
        assert_eq!(f.keys[1].idx, 8);
        assert!((f.keys[1].v[0] - 1.3).abs() < 0.003);
    }

    #[test]
    fn ease_families_are_told_apart() {
        for name in ["linear", "out", "in_out"] {
            let e = named(name);
            let mut seed = 11;
            let y: Vec<Vec<f64>> = (0..=14).map(|t| vec![0.1 * ease::apply(&e, t as f64 / 14.0, 14.0) + 0.0004 * noise(&mut seed), 0.0]).collect();
            let f = fit(&y, &vec![1.0; 15], 0.001);
            assert_eq!(f.keys.len(), 2, "{name}: {:?}", f.keys);
            assert_eq!(family(&f.keys[0].ease), name, "{name}: {:?}", f.keys[0]);
        }
    }

    #[test]
    fn shake_is_the_residual() {
        let mut seed = 5;
        let y: Vec<Vec<f64>> = (0..40).map(|t| vec![0.01 * noise(&mut seed) * if t < 20 { 1.0 } else { 0.2 }, 0.0]).collect();
        let w = vec![1.0; 40];
        let f = fit(&y, &w, 0.004);
        let (rx, ry, ratio) = shake(&y, &w, &f);
        assert!(rx > 0.0015 && ry < 1e-6 && ratio < 0.5, "{rx} {ry} {ratio}");
    }
}
