//! Easing: maps progress `u` in 0..1 to an eased fraction (which may overshoot).

use crate::model::Ease;

pub const NAMES: &[&str] = &[
    "linear", "hold", "in", "out", "in_out", "quad_out", "quint_out", "expo_in", "expo_out",
    "sine_in_out", "back_out", "elastic_out", "bounce_out", "decay",
];

/// `frames` is the length of the segment in output frames; only `decay` uses it.
pub fn apply(ease: &Ease, u: f64, frames: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    match ease {
        Ease::Name(n) => named(n, u, &Default::default(), frames),
        Ease::Bezier(b) => bezier(*b, u),
        Ease::Params { name, args } => named(name, u, args, frames),
    }
}

pub fn is_known(ease: &Ease) -> bool {
    match ease {
        Ease::Name(n) | Ease::Params { name: n, .. } => NAMES.contains(&n.as_str()),
        Ease::Bezier(b) => (0.0..=1.0).contains(&b[0]) && (0.0..=1.0).contains(&b[2]),
    }
}

fn named(name: &str, u: f64, args: &std::collections::BTreeMap<String, f64>, frames: f64) -> f64 {
    let arg = |k: &str, d: f64| args.get(k).copied().unwrap_or(d);
    match name {
        "hold" => {
            if u >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
        "in" => u * u * u,
        "out" => 1.0 - (1.0 - u).powi(3),
        "in_out" => {
            if u < 0.5 {
                4.0 * u * u * u
            } else {
                1.0 - (-2.0 * u + 2.0).powi(3) / 2.0
            }
        }
        "quad_out" => 1.0 - (1.0 - u) * (1.0 - u),
        "quint_out" => 1.0 - (1.0 - u).powi(5),
        "expo_in" => {
            if u <= 0.0 {
                0.0
            } else {
                (2f64).powf(10.0 * u - 10.0)
            }
        }
        "expo_out" => {
            if u >= 1.0 {
                1.0
            } else {
                1.0 - (2f64).powf(-10.0 * u)
            }
        }
        "sine_in_out" => -((std::f64::consts::PI * u).cos() - 1.0) / 2.0,
        "back_out" => {
            let c1 = arg("overshoot", 1.70158);
            let c3 = c1 + 1.0;
            1.0 + c3 * (u - 1.0).powi(3) + c1 * (u - 1.0).powi(2)
        }
        "elastic_out" => {
            if u <= 0.0 {
                0.0
            } else if u >= 1.0 {
                1.0
            } else {
                let period = arg("period", 0.3);
                (2f64).powf(-10.0 * u) * ((u - period / 4.0) * std::f64::consts::TAU / period).sin() + 1.0
            }
        }
        "bounce_out" => bounce_out(u),
        // Exponential approach: each frame covers `rate` of the remaining distance.
        // Normalised so the segment ends exactly on its target.
        "decay" => {
            let rate = arg("rate", 0.4).clamp(1e-4, 0.9999);
            let n = arg("steps", frames).max(1.0);
            let k = 1.0 - rate; // fraction remaining after one frame
            let end = 1.0 - k.powf(n);
            if end <= 1e-9 {
                u
            } else {
                (1.0 - k.powf(u * n)) / end
            }
        }
        _ => u, // "linear" and anything unknown (reported by `check`)
    }
}

fn bounce_out(u: f64) -> f64 {
    let (n1, d1) = (7.5625, 2.75);
    if u < 1.0 / d1 {
        n1 * u * u
    } else if u < 2.0 / d1 {
        let u = u - 1.5 / d1;
        n1 * u * u + 0.75
    } else if u < 2.5 / d1 {
        let u = u - 2.25 / d1;
        n1 * u * u + 0.9375
    } else {
        let u = u - 2.625 / d1;
        n1 * u * u + 0.984375
    }
}

/// CSS-style cubic bezier through (0,0), (x1,y1), (x2,y2), (1,1): solve x(s) = u, return y(s).
fn bezier(b: [f64; 4], u: f64) -> f64 {
    let (x1, y1, x2, y2) = (b[0].clamp(0.0, 1.0), b[1], b[2].clamp(0.0, 1.0), b[3]);
    let cubic = |a: f64, c: f64, s: f64| {
        let m = 1.0 - s;
        3.0 * m * m * s * a + 3.0 * m * s * s * c + s * s * s
    };
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let mut s = u;
    for _ in 0..40 {
        let x = cubic(x1, x2, s);
        if (x - u).abs() < 1e-7 {
            break;
        }
        if x < u {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) / 2.0;
    }
    cubic(y1, y2, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_exact() {
        for n in NAMES {
            let e = Ease::Name((*n).into());
            assert!((apply(&e, 1.0, 10.0) - 1.0).abs() < 1e-6, "{n} at 1");
            if *n != "hold" {
                assert!(apply(&e, 0.0, 10.0).abs() < 1e-3, "{n} at 0");
            }
        }
    }

    #[test]
    fn decay_matches_a_measured_step_ratio() {
        // A move whose per-frame steps shrink by 0.61 each frame covers 39% of the
        // remaining distance per frame.
        let mut args = std::collections::BTreeMap::new();
        args.insert("rate".to_string(), 0.39);
        let e = Ease::Params { name: "decay".into(), args };
        let n = 8.0;
        let p: Vec<f64> = (0..=4).map(|i| apply(&e, i as f64 / n, n)).collect();
        let steps: Vec<f64> = p.windows(2).map(|w| w[1] - w[0]).collect();
        for w in steps.windows(2) {
            assert!((w[1] / w[0] - 0.61).abs() < 1e-6);
        }
    }

    #[test]
    fn linear_bezier_is_identity() {
        let e = Ease::Bezier([0.25, 0.25, 0.75, 0.75]);
        for i in 0..=10 {
            let u = i as f64 / 10.0;
            assert!((apply(&e, u, 1.0) - u).abs() < 1e-4);
        }
    }
}
