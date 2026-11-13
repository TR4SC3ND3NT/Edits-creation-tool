//! Built-in distortions.

use super::{fract, num, params, pct, pos, seed, vec2, Builtin, Frame, Params};
use edits_core::ResolvedEffect;

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { kind: "twirl", source: include_str!("../shaders/fx/twirl.wgsl"), pack: twirl },
    Builtin { kind: "bulge", source: include_str!("../shaders/fx/bulge.wgsl"), pack: bulge },
    Builtin { kind: "kaleidoscope", source: include_str!("../shaders/fx/kaleidoscope.wgsl"), pack: kaleidoscope },
    Builtin { kind: "wave_warp", source: include_str!("../shaders/fx/wave_warp.wgsl"), pack: wave_warp },
    Builtin { kind: "ripple", source: include_str!("../shaders/fx/ripple.wgsl"), pack: ripple },
    Builtin { kind: "shockwave", source: include_str!("../shaders/fx/shockwave.wgsl"), pack: shockwave },
    Builtin { kind: "mirror", source: include_str!("../shaders/fx/mirror.wgsl"), pack: mirror },
    Builtin { kind: "tile", source: include_str!("../shaders/fx/tile.wgsl"), pack: tile },
    Builtin { kind: "polar", source: include_str!("../shaders/fx/polar.wgsl"), pack: polar },
    Builtin { kind: "lens_distortion", source: include_str!("../shaders/fx/lens_distortion.wgsl"), pack: lens_distortion },
    Builtin { kind: "displacement_map", source: include_str!("../shaders/fx/displacement_map.wgsl"), pack: displacement_map },
    Builtin { kind: "turbulent_displace", source: include_str!("../shaders/fx/turbulent_displace.wgsl"), pack: turbulent_displace },
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

/// A phase in cycles: `degrees` / 360 minus `speed` x `t`, wrapped to 0..1 in f64.
fn cycles(degrees: f32, speed: f32, t: f64) -> f32 {
    fract(degrees as f64 / 360.0 - speed as f64 * t)
}

fn wave_warp(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let (amp, wavelength) = (pct(num(e, "amplitude") as f64), pct(num(e, "wavelength") as f64));
    let a = num(e, "direction").to_radians();
    let phase = cycles(num(e, "phase"), num(e, "speed"), f.t);
    (amp != 0.0 && wavelength > 0.0).then(|| params(&[amp, wavelength, a.sin(), -a.cos(), phase, seed(e)]))
}

fn ripple(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let (amp, wavelength, radius, c) = (pct(num(e, "amplitude") as f64), pct(num(e, "wavelength") as f64), pct(num(e, "radius") as f64), pos(e, "center"));
    let phase = cycles(num(e, "phase"), num(e, "speed"), f.t);
    (amp != 0.0 && wavelength > 0.0 && radius > 0.0).then(|| params(&[amp, wavelength, radius, c[0], c[1], phase]))
}

fn shockwave(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let progress = num(e, "progress");
    let (radius, width, strength, c) = (pct(num(e, "radius") as f64), pct(num(e, "width") as f64), pct(num(e, "strength") as f64), pos(e, "center"));
    // It swells in over the first tenth and fades out by the end.
    let fade = (progress * 10.0).min(1.0) * (1.0 - progress);
    (fade > 0.0 && width > 0.0 && strength != 0.0).then(|| params(&[progress * radius, width * 0.5, strength * fade, c[0], c[1]]))
}

fn mirror(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (c, a) = (pos(e, "center"), num(e, "angle").to_radians());
    Some(params(&[c[0], c[1], a.cos(), a.sin()]))
}

fn tile(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let (tiles, off) = (vec2(e, "tiles"), pos(e, "offset"));
    let mirror = if num(e, "mirror") > 0.5 { 1.0 } else { 0.0 };
    let identity = tiles == [1.0, 1.0] && off == [0.0, 0.0];
    (!identity).then(|| params(&[tiles[0], tiles[1], mirror, off[0] / f.aspect, off[1]]))
}

fn polar(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    (amount > 0.0).then(|| params(&[amount]))
}

fn lens_distortion(e: &ResolvedEffect, f: &Frame) -> Option<Params> {
    let (amount, scale, c) = (num(e, "amount"), num(e, "scale"), pos(e, "center"));
    // `amount` is the relative stretch at the frame's corners (seen from its centre).
    let corner2 = (f.aspect * f.aspect + 1.0) * 0.25;
    (amount != 0.0 || scale != 1.0).then(|| params(&[amount / corner2, c[0], c[1], 1.0 / scale]))
}

fn displacement_map(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let a = e.vec("amount", 2);
    let (x, y) = (pct(a[0]), pct(a[1]));
    (x != 0.0 || y != 0.0).then(|| params(&[x, y]))
}

fn turbulent_displace(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (amount, size) = (pct(num(e, "amount") as f64), pct(num(e, "size") as f64));
    (amount != 0.0 && size > 0.0).then(|| params(&[amount, 1.0 / size, num(e, "complexity"), num(e, "evolution") / 360.0, seed(e)]))
}
