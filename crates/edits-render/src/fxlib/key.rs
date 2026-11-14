//! Built-in keying effects: they multiply the layer's alpha (see
//! `edits_core::effects::key`).

use super::stylize::display;
use super::{num, params, rgb, Builtin, Frame, Params};
use edits_core::ResolvedEffect;

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { kind: "chroma_key", source: include_str!("../shaders/fx/chroma_key.wgsl"), pack: chroma_key },
    Builtin { kind: "spill_suppress", source: include_str!("../shaders/fx/spill_suppress.wgsl"), pack: spill_suppress },
    Builtin { kind: "luma_key", source: include_str!("../shaders/fx/luma_key.wgsl"), pack: luma_key },
    Builtin { kind: "unmult", source: include_str!("../shaders/fx/unmult.wgsl"), pack: unmult },
    Builtin { kind: "difference_matte", source: include_str!("../shaders/fx/difference_matte.wgsl"), pack: difference_matte },
];

/// Whether a key with `amount` changes anything; the matte view always does.
fn keys(e: &ResolvedEffect, amount: f32) -> bool {
    amount > 0.0 || e.option("view") == "matte"
}

fn chroma_key(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (k, amount) = (display(rgb(e, "color")), num(e, "amount"));
    keys(e, amount).then(|| params(&[k[0], k[1], k[2], num(e, "tolerance"), num(e, "softness"), amount]))
}

fn spill_suppress(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (k, amount) = (rgb(e, "color"), num(e, "amount"));
    if amount <= 0.0 {
        return None;
    }
    if e.option("method") == "better" {
        // The key colour's chroma (its difference from its grey), as a unit vector.
        let g = (k[0] + k[1] + k[2]) / 3.0;
        let c = k.map(|v| v - g);
        let n = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        return (n > 1e-5).then(|| params(&[amount, c[0] / n, c[1] / n, c[2] / n]));
    }
    // The key's strongest channel and the other two, as one-hot masks.
    let first = if k[1] >= k[0] && k[1] >= k[2] {
        1
    } else if k[2] >= k[0] && k[2] >= k[1] {
        2
    } else {
        0
    };
    let hot = |i: usize| -> [f32; 3] { std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 }) };
    let (p, o1, o2) = (hot(first), hot((first + 1) % 3), hot((first + 2) % 3));
    Some(params(&[amount, p[0], p[1], p[2], o1[0], o1[1], o1[2], o2[0], o2[1], o2[2]]))
}

fn luma_key(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    keys(e, amount).then(|| params(&[num(e, "threshold"), num(e, "tolerance"), num(e, "softness"), amount]))
}

fn unmult(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    Some(params(&[num(e, "level"), num(e, "softness")]))
}

fn difference_matte(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    keys(e, amount).then(|| params(&[num(e, "tolerance"), num(e, "softness"), amount]))
}
