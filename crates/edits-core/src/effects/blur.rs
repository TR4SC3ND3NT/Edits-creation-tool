//! Blurs: multi-pass, sized in percent of the frame height.

use super::{fx, p, EffectSpec};

const INF: f64 = f64::INFINITY;

pub(super) const EFFECTS: &[EffectSpec] = &[
    fx!("blur", "gaussian_blur", "Soft defocus.",
        [p!("radius", [1.0], pct, 0.0, INF, "Blur sigma, % of height.")]),
    fx!("blur", "directional_blur", "Blur along a line, like a fast slide.",
        [p!("angle", [0.0], deg, "Direction in degrees; 0 is horizontal."), p!("length", [2.0], pct, "Streak length, % of height.")]),
    fx!("blur", "zoom_blur", "Blur along rays from a centre, like a fast push-in.",
        [p!("strength", [0.1], num, "Streak length as a fraction of the distance from the centre."), p!("center", [0.0, 0.0], pos, "Centre in composition units.")]),
    fx!("blur", "spin_blur", "Blur along arcs about a centre.",
        [p!("angle", [5.0], deg, "Arc swept, degrees."), p!("center", [0.0, 0.0], pos, "Centre in composition units.")]),
];
