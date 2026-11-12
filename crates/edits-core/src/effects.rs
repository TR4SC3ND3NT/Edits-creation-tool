//! The built-in effect registry: the single source of truth for effect kinds,
//! their parameters and defaults. The renderer implements exactly these.
//!
//! Spatial parameters marked "% of height" are percent of the composition
//! height. Colours are linear RGB in 0..1. Angles are degrees.

pub struct ParamSpec {
    pub name: &'static str,
    pub default: &'static [f64],
    pub doc: &'static str,
}

pub struct EffectSpec {
    pub kind: &'static str,
    pub doc: &'static str,
    pub params: &'static [ParamSpec],
}

macro_rules! p {
    ($n:literal, $d:expr, $doc:literal) => {
        ParamSpec { name: $n, default: &$d, doc: $doc }
    };
}

pub const EFFECTS: &[EffectSpec] = &[
    EffectSpec { kind: "exposure", doc: "Multiply by 2^stops. Black stays black, so a large value gives the white flash where only dark ink survives.",
        params: &[p!("stops", [0.0], "Exposure change in stops; 4 is a strong flash.")] },
    EffectSpec { kind: "fade", doc: "Mix the whole image toward a colour.",
        params: &[p!("amount", [0.0], "0 = unchanged, 1 = solid colour."), p!("color", [1.0, 1.0, 1.0], "Target colour.")] },
    EffectSpec { kind: "brightness_contrast", doc: "Add brightness, then scale contrast about mid grey.",
        params: &[p!("brightness", [0.0], "-1..1 added to the image."), p!("contrast", [0.0], "-1..1; positive increases contrast.")] },
    EffectSpec { kind: "saturation", doc: "Scale colour saturation.",
        params: &[p!("amount", [1.0], "0 = black and white, 1 = unchanged, 2 = double.")] },
    EffectSpec { kind: "hue", doc: "Rotate hue.",
        params: &[p!("degrees", [0.0], "Hue rotation in degrees.")] },
    EffectSpec { kind: "invert", doc: "Colour negative.",
        params: &[p!("amount", [1.0], "0 = unchanged, 1 = fully inverted.")] },
    EffectSpec { kind: "threshold", doc: "Two-tone ink look: luminance above `level` becomes white, below becomes black.",
        params: &[p!("level", [0.5], "Luminance cut, 0..1."), p!("softness", [0.02], "Width of the transition."), p!("amount", [1.0], "Mix with the original.")] },
    EffectSpec { kind: "posterize", doc: "Quantise each channel to a few levels.",
        params: &[p!("levels", [4.0], "Number of levels per channel (2 or more).")] },
    EffectSpec { kind: "tint", doc: "Recolour by luminance toward a colour.",
        params: &[p!("color", [1.0, 1.0, 1.0], "Tint colour."), p!("amount", [1.0], "Mix with the original.")] },
    EffectSpec { kind: "gaussian_blur", doc: "Soft defocus.",
        params: &[p!("radius", [1.0], "Blur sigma, % of height.")] },
    EffectSpec { kind: "directional_blur", doc: "Blur along a line, like a fast slide.",
        params: &[p!("angle", [0.0], "Direction in degrees; 0 is horizontal."), p!("length", [2.0], "Streak length, % of height.")] },
    EffectSpec { kind: "zoom_blur", doc: "Blur along rays from a centre, like a fast push-in.",
        params: &[p!("strength", [0.1], "Streak length as a fraction of the distance from the centre."), p!("center", [0.0, 0.0], "Centre in composition units.")] },
    EffectSpec { kind: "spin_blur", doc: "Blur along arcs about a centre.",
        params: &[p!("angle", [5.0], "Arc swept, degrees."), p!("center", [0.0, 0.0], "Centre in composition units.")] },
    EffectSpec { kind: "glow", doc: "Bloom: blur the bright parts and add them back.",
        params: &[p!("threshold", [0.7], "Luminance above which pixels glow."), p!("radius", [3.0], "Glow sigma, % of height."), p!("intensity", [1.0], "Strength of the added glow."), p!("color", [1.0, 1.0, 1.0], "Tint of the glow.")] },
    EffectSpec { kind: "vignette", doc: "Darken toward the corners.",
        params: &[p!("amount", [0.5], "0 = none, 1 = corners black."), p!("radius", [0.75], "Where darkening starts, as a fraction of the half-diagonal."), p!("softness", [0.5], "Width of the falloff.")] },
    EffectSpec { kind: "rgb_split", doc: "Chromatic aberration: shift red and blue apart.",
        params: &[p!("amount", [0.5], "Shift, % of height."), p!("angle", [0.0], "Direction in degrees.")] },
    EffectSpec { kind: "transform2d", doc: "Scale, rotate and move the image in the frame (a camera punch on an adjustment layer).",
        params: &[p!("scale", [1.0], "Zoom factor about the centre."), p!("rotation", [0.0], "Degrees."), p!("offset", [0.0, 0.0], "Shift in composition units."), p!("mirror", [1.0], "1 = mirror at the frame edges, 0 = leave transparent.")] },
    EffectSpec { kind: "grain", doc: "Film grain, deterministic per frame.",
        params: &[p!("amount", [0.05], "Grain strength."), p!("size", [1.0], "Grain size, in output pixels at 1080.")] },
    EffectSpec { kind: "custom", doc: "A WGSL fragment supplied by `source`. Parameters p0..p7 are passed through as numbers.",
        params: &[p!("p0", [0.0], ""), p!("p1", [0.0], ""), p!("p2", [0.0], ""), p!("p3", [0.0], ""), p!("p4", [0.0], ""), p!("p5", [0.0], ""), p!("p6", [0.0], ""), p!("p7", [0.0], "")] },
];

pub fn spec(kind: &str) -> Option<&'static EffectSpec> {
    EFFECTS.iter().find(|e| e.kind == kind)
}
