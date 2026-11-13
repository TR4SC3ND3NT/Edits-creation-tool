//! Built-in stylise effects.

use super::{num, pct, pos, Builtin, Frame, Params};
use edits_core::ResolvedEffect;

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { kind: "chromatic_aberration", source: include_str!("../shaders/fx/chromatic_aberration.wgsl"), pack: chromatic_aberration },
    Builtin { kind: "pixelate", source: include_str!("../shaders/fx/pixelate.wgsl"), pack: pixelate },
    Builtin { kind: "halftone", source: include_str!("../shaders/fx/halftone.wgsl"), pack: halftone },
];

fn chromatic_aberration(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let c = pos(e, "center");
    // `amount` is the spread at the corners of the frame, seen from its centre.
    let corner = (f.aspect * f.aspect + 1.0).sqrt() * 0.5;
    let spread = pct(num(e, "amount") as f64) / corner;
    // Less than a tenth of a pixel at the corners changes nothing visible.
    (spread * corner * f.h >= 0.1).then(|| [spread, c[0], c[1], num(e, "samples").round(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
}

fn pixelate(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (size, c) = (pct(num(e, "size") as f64), pos(e, "center"));
    (size > 0.0).then(|| [size, c[0], c[1], 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
}

fn halftone(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (size, amount) = (pct(num(e, "size") as f64), num(e, "amount"));
    (size > 0.0 && amount > 0.0).then(|| [size, num(e, "angle").to_radians(), num(e, "smoothing"), amount, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
}
