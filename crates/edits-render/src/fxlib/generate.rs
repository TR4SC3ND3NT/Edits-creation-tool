//! Built-in generators.

use super::stylize::display;
use super::{num, params, pct, pos, rgb, seed, Builtin, Frame, Params};
use edits_core::ResolvedEffect;

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { kind: "fractal_noise", source: include_str!("../shaders/fx/fractal_noise.wgsl"), pack: fractal_noise },
    Builtin { kind: "gradient_ramp", source: include_str!("../shaders/fx/gradient_ramp.wgsl"), pack: gradient_ramp },
];

fn fractal_noise(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (scale, amount, off) = (pct(num(e, "scale") as f64), num(e, "amount"), pos(e, "offset"));
    (amount > 0.0 && scale > 0.0)
        .then(|| params(&[1.0 / scale, num(e, "complexity"), num(e, "evolution") / 360.0, num(e, "contrast"), num(e, "brightness"), off[0], off[1], seed(e), amount]))
}

fn gradient_ramp(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (a, b, amount) = (pos(e, "start"), pos(e, "end"), num(e, "amount"));
    let (ca, cb) = (display(rgb(e, "color_a")), display(rgb(e, "color_b")));
    (amount > 0.0).then(|| params(&[a[0], a[1], b[0], b[1], ca[0], ca[1], ca[2], cb[0], cb[1], cb[2], amount]))
}
