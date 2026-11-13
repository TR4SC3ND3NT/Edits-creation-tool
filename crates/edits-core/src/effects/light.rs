//! Light: glows, rays, flares.

use super::{fx, p, EffectSpec};

const INF: f64 = f64::INFINITY;

pub(super) const EFFECTS: &[EffectSpec] = &[
    fx!("light", "glow", "Bloom: blur the bright parts and add them back.",
        [p!("threshold", [0.7], num, "Luminance above which pixels glow."), p!("radius", [3.0], pct, 0.0, INF, "Glow sigma, % of height."),
         p!("intensity", [1.0], num, 0.0, INF, "Strength of the added glow."), p!("color", [1.0, 1.0, 1.0], color, "Tint of the glow.")]),
];
