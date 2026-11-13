//! Distortions: each output pixel reads the picture somewhere else.

use super::{fx, opt, p, EffectSpec};

const INF: f64 = f64::INFINITY;

pub(super) const EFFECTS: &[EffectSpec] = &[
    fx!("distort", "transform2d", "Scale, rotate and move the image in the frame (a camera punch on an adjustment layer).",
        [p!("scale", [1.0], x, "Zoom factor about the centre."), p!("rotation", [0.0], deg, "Degrees."), p!("offset", [0.0, 0.0], pos, "Shift in composition units."),
         p!("mirror", [1.0], num, 0.0, 1.0, "1 = mirror at the frame edges, 0 = leave transparent.")]),
    fx!("distort", "twirl", "Twist the picture about a centre: the turn is `angle` at the centre and falls off to nothing at `radius`.",
        [p!("angle", [90.0], deg, "Turn at the centre, degrees; positive is clockwise."), p!("radius", [30.0], pct, 0.0, INF, "Radius of the twisted disc, % of height."),
         p!("center", [0.0, 0.0], pos, "Centre in composition units.")],
        credit: "EffectCraft distort::twirl, MIT OR Apache-2.0"),
    fx!("distort", "bulge", "A lens bulge: magnify (or, negative, pinch) a disc of the picture.",
        [p!("amount", [0.5], num, -1.0, 1.0, "Positive magnifies the middle (1 = twice as big), negative pinches it."), p!("radius", [25.0], pct, 0.0, INF, "Radius of the disc, % of height."),
         p!("center", [0.0, 0.0], pos, "Centre in composition units."), p!("taper", [0.0], num, 0.0, 1.0, "0 = a rounded dome, 1 = a sharp peak with shallow sides.")],
        credit: "EffectCraft distort::bulge, MIT OR Apache-2.0"),
    fx!("distort", "kaleidoscope", "Fold the picture into mirrored wedges about a centre.",
        [p!("segments", [6.0], count, 2.0, 64.0, "Number of wedges in a full turn."), p!("center", [0.0, 0.0], pos, "Centre in composition units."),
         p!("rotation", [0.0], deg, "Turns the wedges, degrees."), p!("zoom", [1.0], x, 0.01, 100.0, "Magnification of the folded picture.")],
        options: &[opt!("mirror", ["on", "off"], "on: neighbouring wedges are mirror images; off: every wedge is the same copy.")],
        credit: "EffectCraft stylize2::kaleida, MIT OR Apache-2.0"),
];
