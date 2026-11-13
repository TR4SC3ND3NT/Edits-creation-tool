//! Built-in stylise effects.

use super::{fract, num, params, pct, pos, rgb, seed, Builtin, Frame, Params};
use edits_core::ResolvedEffect;

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { kind: "chromatic_aberration", source: include_str!("../shaders/fx/chromatic_aberration.wgsl"), pack: chromatic_aberration },
    Builtin { kind: "pixelate", source: include_str!("../shaders/fx/pixelate.wgsl"), pack: pixelate },
    Builtin { kind: "halftone", source: include_str!("../shaders/fx/halftone.wgsl"), pack: halftone },
    Builtin { kind: "screentone", source: include_str!("../shaders/fx/screentone.wgsl"), pack: screentone },
    Builtin { kind: "edge_detect", source: include_str!("../shaders/fx/edge_detect.wgsl"), pack: edge_detect },
    Builtin { kind: "ink", source: include_str!("../shaders/fx/ink.wgsl"), pack: ink },
    Builtin { kind: "toon", source: include_str!("../shaders/fx/toon.wgsl"), pack: toon },
    Builtin { kind: "kuwahara", source: include_str!("../shaders/fx/kuwahara.wgsl"), pack: kuwahara },
    Builtin { kind: "emboss", source: include_str!("../shaders/fx/emboss.wgsl"), pack: emboss },
    Builtin { kind: "sharpen", source: include_str!("../shaders/fx/sharpen.wgsl"), pack: sharpen },
    Builtin { kind: "glitch", source: include_str!("../shaders/fx/glitch.wgsl"), pack: glitch },
    Builtin { kind: "scanlines", source: include_str!("../shaders/fx/scanlines.wgsl"), pack: scanlines },
    Builtin { kind: "vhs", source: include_str!("../shaders/fx/vhs.wgsl"), pack: vhs },
    Builtin { kind: "film_damage", source: include_str!("../shaders/fx/film_damage.wgsl"), pack: film_damage },
    Builtin { kind: "film_burn", source: include_str!("../shaders/fx/film_burn.wgsl"), pack: film_burn },
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

/// A linear colour as display (sRGB-encoded) values.
pub(super) fn display(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| {
        let a = v.abs();
        let e = if a <= 0.0031308 { a * 12.92 } else { 1.055 * a.powf(1.0 / 2.4) - 0.055 };
        e.copysign(v)
    })
}

fn screentone(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (size, amount, a) = (pct(num(e, "size") as f64), num(e, "amount"), num(e, "angle").to_radians());
    let (ink, paper) = (display(rgb(e, "ink")), display(rgb(e, "paper")));
    (size > 0.0 && amount > 0.0).then(|| params(&[size, num(e, "levels").round(), a.cos(), a.sin(), amount, ink[0], ink[1], ink[2], paper[0], paper[1], paper[2]]))
}

fn edge_detect(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    let c = display(rgb(e, "color"));
    (amount > 0.0).then(|| params(&[num(e, "strength"), num(e, "threshold"), pct(num(e, "width") as f64), num(e, "invert"), c[0], c[1], c[2], amount]))
}

fn ink(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    let (ink, paper) = (display(rgb(e, "ink")), display(rgb(e, "paper")));
    (amount > 0.0).then(|| params(&[num(e, "strength"), num(e, "threshold"), pct(num(e, "width") as f64), amount, ink[0], ink[1], ink[2], paper[0], paper[1], paper[2]]))
}

fn toon(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    let c = display(rgb(e, "color"));
    (amount > 0.0).then(|| params(&[num(e, "levels").round(), num(e, "edge"), pct(num(e, "width") as f64), amount, c[0], c[1], c[2]]))
}

fn kuwahara(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let (radius, amount) = (pct(num(e, "radius") as f64), num(e, "amount"));
    // Below half a pixel the squares are all the same pixel.
    (radius * f.h >= 0.5 && amount > 0.0).then(|| params(&[radius, amount]))
}

fn emboss(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (a, height, amount) = (num(e, "angle").to_radians(), pct(num(e, "height") as f64), num(e, "amount"));
    (amount > 0.0).then(|| params(&[a.cos() * height, a.sin() * height, num(e, "strength"), amount]))
}

fn sharpen(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    (amount > 0.0).then(|| params(&[amount]))
}

fn glitch(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let (amount, block) = (num(e, "amount"), num(e, "block"));
    // The change number, wrapped so that it stays exact as an f32.
    let step = ((num(e, "rate") as f64 * f.t).floor().rem_euclid(1048576.0)) as f32;
    (amount > 0.0 || block > 0.0).then(|| params(&[amount, step, num(e, "bands").round(), pct(num(e, "shift") as f64), pct(num(e, "rgb") as f64), block, seed(e)]))
}

fn scanlines(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let (count, amount) = (num(e, "count").round(), num(e, "amount"));
    let scroll = fract(num(e, "speed") as f64 * f.t);
    // Lines closer than two pixels alias: fade them into their mean by 1.5 pixels.
    let visible = ((f.h / count - 1.5) / 0.5).clamp(0.0, 1.0);
    (amount > 0.0).then(|| params(&[count, amount, scroll, visible]))
}

fn vhs(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let (noise, roll, bleed, wobble) = (num(e, "noise"), num(e, "roll"), pct(num(e, "bleed") as f64), pct(num(e, "wobble") as f64));
    let roll = fract(roll as f64 * f.t);
    let t = (f.t.rem_euclid(4096.0)) as f32;
    (noise > 0.0 || roll != 0.0 || bleed > 0.0 || wobble != 0.0).then(|| params(&[noise, roll, bleed, wobble, seed(e), t]))
}

fn film_damage(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (s, d, fl, j) = (num(e, "scratches"), num(e, "dust"), num(e, "flicker"), pct(num(e, "jitter") as f64));
    (s > 0.0 || d > 0.0 || fl > 0.0 || j > 0.0).then(|| params(&[s, d, fl, j, seed(e)]))
}

fn film_burn(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let (amount, c, centre, size) = (num(e, "amount"), rgb(e, "color"), pos(e, "center"), pct(num(e, "size") as f64));
    let z = ((num(e, "speed") as f64 * f.t).rem_euclid(4096.0)) as f32;
    let diagonal = (f.aspect * f.aspect + 1.0).sqrt();
    (amount > 0.0).then(|| params(&[amount, c[0], c[1], c[2], centre[0], centre[1], 1.0 / size, z, seed(e), diagonal]))
}
