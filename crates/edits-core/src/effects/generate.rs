//! Generators: effects that draw new content (noise, gradients).
//!
//! They draw inside the picture they are given: the result keeps that
//! picture's transparency, so on a solid layer they fill the solid and on an
//! adjustment layer they cover what is beneath.

use super::{fx, opt, p, EffectSpec};

const INF: f64 = f64::INFINITY;

pub(super) const EFFECTS: &[EffectSpec] = &[
    fx!("generate", "fractal_noise", "Fractal noise (clouds, smoke, texture), grey, blended over the picture.",
        [p!("scale", [20.0], pct, 0.01, INF, "Size of the largest features, % of height."), p!("complexity", [4.0], count, 1.0, 8.0, "Octaves: each adds detail at half the size."),
         p!("evolution", [0.0], deg, "Changes the noise smoothly; key it to animate (360 = one step)."), p!("contrast", [1.0], x, 0.0, INF, "Contrast about mid grey."),
         p!("brightness", [0.0], num, "Added to the grey value (display, -1..1)."), p!("offset", [0.0, 0.0], pos, "Moves the noise, composition units."),
         p!("seed", [0.0], num, "A different noise."), p!("amount", [1.0], num, 0.0, 1.0, "Opacity of the noise.")],
        options: &[opt!("blend", ["replace", "multiply", "screen", "add", "overlay"], "How the noise is combined with the picture.")]),
    fx!("generate", "gradient_ramp", "A two-colour gradient between two points, linear or radial, blended over the picture.",
        [p!("start", [0.0, -0.5], pos, "Where `color_a` is full, composition units."), p!("end", [0.0, 0.5], pos, "Where `color_b` is full (radial: the edge of the circle)."),
         p!("color_a", [0.0, 0.0, 0.0], color, "Colour at `start`."), p!("color_b", [1.0, 1.0, 1.0], color, "Colour at `end`."),
         p!("amount", [1.0], num, 0.0, 1.0, "Opacity of the gradient.")],
        options: &[opt!("shape", ["linear", "radial"], "linear: bands across the line from start to end; radial: circles about start."),
                   opt!("blend", ["replace", "multiply", "screen", "add", "overlay"], "How the gradient is combined with the picture.")]),
];
