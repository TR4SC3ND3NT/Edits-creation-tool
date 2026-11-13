//! Built-in distortions.

use super::{num, pct, pos, Builtin, Frame, Params};
use edits_core::ResolvedEffect;

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { kind: "twirl", source: include_str!("../shaders/fx/twirl.wgsl"), pack: twirl },
    Builtin { kind: "bulge", source: include_str!("../shaders/fx/bulge.wgsl"), pack: bulge },
    Builtin { kind: "kaleidoscope", source: include_str!("../shaders/fx/kaleidoscope.wgsl"), pack: kaleidoscope },
];

fn twirl(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (angle, radius, c) = (num(e, "angle").to_radians(), pct(num(e, "radius") as f64), pos(e, "center"));
    (angle != 0.0 && radius > 0.0).then(|| [angle, radius, c[0], c[1], 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
}

fn bulge(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (amount, radius, c) = (num(e, "amount"), pct(num(e, "radius") as f64), pos(e, "center"));
    let exponent = 1.0 + 3.0 * num(e, "taper");
    (amount != 0.0 && radius > 0.0).then(|| [amount, radius, c[0], c[1], exponent, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
}

fn kaleidoscope(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let c = pos(e, "center");
    Some([num(e, "segments").round(), num(e, "rotation").to_radians(), c[0], c[1], num(e, "zoom"), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
}
