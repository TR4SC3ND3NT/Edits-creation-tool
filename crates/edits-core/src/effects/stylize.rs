//! Stylise: print, pixel and lens looks.

use super::{fx, opt, p, EffectSpec};

const INF: f64 = f64::INFINITY;

pub(super) const EFFECTS: &[EffectSpec] = &[
    fx!("stylize", "rgb_split", "Chromatic aberration: shift red and blue apart.",
        [p!("amount", [0.5], pct, "Shift, % of height."), p!("angle", [0.0], deg, "Direction in degrees.")]),
    fx!("stylize", "chromatic_aberration", "Radial lens fringing: colours spread apart toward the edges of the frame, red outward and blue inward, as a smooth spectrum.",
        [p!("amount", [1.0], pct, 0.0, INF, "Spread at the frame's corners, % of height; it grows linearly from 0 at `center`."), p!("center", [0.0, 0.0], pos, "Centre of the lens in composition units."),
         p!("samples", [8.0], count, 2.0, 32.0, "Samples along the spread: more is smoother and slower.")],
        credit: "Bevy chromatic_aberration.wesl (the INSIDE technique), MIT OR Apache-2.0"),
    fx!("stylize", "pixelate", "Mosaic: the picture in flat cells, each the colour at its centre.",
        [p!("size", [2.0], pct, 0.0, INF, "Cell size, % of height (the ring width for polar)."), p!("center", [0.0, 0.0], pos, "A point the grid is centred on, in composition units.")],
        options: &[opt!("shape", ["square", "hex", "triangle", "polar"], "Cell shape; polar cells are rings cut into segments about `center`.")],
        credit: "GPUImage Pixellate and PolarPixellate, BSD-3-Clause; glfx.js hexagonalPixelate, MIT"),
    fx!("stylize", "halftone", "Print screen: the picture as a grid of ink dots (or lines, or squares) whose area follows the light, so the average brightness is kept.",
        [p!("size", [1.2], pct, 0.0, INF, "Distance between dots, % of height."), p!("angle", [45.0], deg, "Angle of the screen."),
         p!("smoothing", [0.15], num, 0.0, 1.0, "Softness of the dot edges."), p!("amount", [1.0], num, 0.0, 1.0, "Mix with the original.")],
        options: &[opt!("mode", ["mono", "rgb", "cmyk"], "mono: black ink on white by luminance; rgb: a screen per light channel; cmyk: cyan, magenta, yellow and black ink screens at print angles."),
                   opt!("shape", ["dot", "line", "square"], "Shape of the ink.")],
        credit: "screen angles and CMYK separation from ISF CMYK Halftone by zoidberg (Vidvox ISF-Files), MIT"),
];
