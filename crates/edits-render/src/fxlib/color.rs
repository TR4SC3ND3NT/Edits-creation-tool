//! Built-in colour effects that need a pass of their own (most colour
//! effects are fused into other passes instead: see `compose.rs`).

use super::stylize::display;
use super::{num, params, rgb, Builtin, Frame, Params};
use edits_core::ResolvedEffect;

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { kind: "lut", source: include_str!("../shaders/fx/lut.wgsl"), pack: lut },
    Builtin { kind: "gradient_map", source: include_str!("../shaders/fx/gradient_map.wgsl"), pack: gradient_map },
    Builtin { kind: "levels", source: include_str!("../shaders/fx/levels.wgsl"), pack: levels },
    Builtin { kind: "vibrance", source: include_str!("../shaders/fx/vibrance.wgsl"), pack: vibrance },
];

fn lut(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    (amount > 0.0).then(|| params(&[amount]))
}

fn gradient_map(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    let (s, m, h) = (display(rgb(e, "shadows")), display(rgb(e, "midtones")), display(rgb(e, "highlights")));
    (amount > 0.0).then(|| params(&[s[0], s[1], s[2], m[0], m[1], m[2], h[0], h[1], h[2], amount]))
}

fn levels(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (ib, mut iw, gamma, ob, ow) = (num(e, "in_black"), num(e, "in_white"), num(e, "gamma"), num(e, "out_black"), num(e, "out_white"));
    if (iw - ib).abs() < 1e-6 {
        iw = ib + 1e-6;
    }
    let identity = ib == 0.0 && iw == 1.0 && gamma == 1.0 && ob == 0.0 && ow == 1.0;
    (!identity).then(|| params(&[ib, iw, 1.0 / gamma, ob, ow]))
}

fn vibrance(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    (amount != 0.0).then(|| params(&[amount]))
}
