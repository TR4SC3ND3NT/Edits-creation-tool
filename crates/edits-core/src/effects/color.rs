//! Colour effects: pointwise, so the renderer fuses neighbours into one pass.

use super::{fx, opt, p, EffectSpec, SourceKind};

const INF: f64 = f64::INFINITY;

pub(super) const EFFECTS: &[EffectSpec] = &[
    fx!("color", "exposure", "Multiply by 2^stops. Black stays black, so a large value gives the white flash where only dark ink survives.",
        [p!("stops", [0.0], stops, "Exposure change in stops; 4 is a strong flash.")]),
    fx!("color", "fade", "Mix the whole image toward a colour.",
        [p!("amount", [0.0], num, 0.0, 1.0, "0 = unchanged, 1 = solid colour."), p!("color", [1.0, 1.0, 1.0], color, "Target colour.")]),
    fx!("color", "brightness_contrast", "Add brightness, then scale contrast about mid grey.",
        [p!("brightness", [0.0], num, "-1..1 added to the image."), p!("contrast", [0.0], num, -1.0, 1.0, "-1..1; positive increases contrast.")]),
    fx!("color", "saturation", "Scale colour saturation.",
        [p!("amount", [1.0], x, 0.0, INF, "0 = black and white, 1 = unchanged, 2 = double.")]),
    fx!("color", "hue", "Rotate hue.",
        [p!("degrees", [0.0], deg, "Hue rotation in degrees.")]),
    fx!("color", "invert", "Colour negative.",
        [p!("amount", [1.0], num, 0.0, 1.0, "0 = unchanged, 1 = fully inverted.")]),
    fx!("color", "threshold", "Two-tone ink look: luminance above `level` becomes white, below becomes black.",
        [p!("level", [0.5], num, "Luminance cut, 0..1."), p!("softness", [0.02], num, 0.0, INF, "Width of the transition."), p!("amount", [1.0], num, 0.0, 1.0, "Mix with the original.")]),
    fx!("color", "posterize", "Quantise each channel to a few levels.",
        [p!("levels", [4.0], count, 2.0, INF, "Number of levels per channel (2 or more).")]),
    fx!("color", "tint", "Recolour by luminance toward a colour.",
        [p!("color", [1.0, 1.0, 1.0], color, "Tint colour."), p!("amount", [1.0], num, 0.0, 1.0, "Mix with the original.")]),
    fx!("color", "vignette", "Darken toward the corners.",
        [p!("amount", [0.5], num, "0 = none, 1 = corners black."), p!("radius", [0.75], num, "Where darkening starts, as a fraction of the half-diagonal."), p!("softness", [0.5], num, "Width of the falloff.")]),
    fx!("color", "grain", "Film grain, deterministic per frame.",
        [p!("amount", [0.05], num, 0.0, INF, "Grain strength."), p!("size", [1.0], px1080, "Grain size, in output pixels at 1080.")]),
    fx!("color", "lut", "Apply a colour lookup table: a `.cube` file (1D or 3D) named by `source`, on display (sRGB) values.",
        [p!("amount", [1.0], num, 0.0, 1.0, "Mix with the original.")],
        source: SourceKind::Cube, credit: "EffectCraft utility::parse_cube (the .cube rules), MIT OR Apache-2.0"),
    fx!("color", "gradient_map", "Recolour by brightness: shadows, midtones and highlights each become a chosen colour (tritone), or two colours (duotone).",
        [p!("shadows", [0.0, 0.0, 0.0], color, "Colour of black."), p!("midtones", [0.214, 0.214, 0.214], color, "Colour of mid grey (tritone only)."),
         p!("highlights", [1.0, 1.0, 1.0], color, "Colour of white."), p!("amount", [1.0], num, 0.0, 1.0, "Mix with the original.")],
        options: &[opt!("mode", ["tritone", "duotone"], "tritone: shadows, midtones, highlights; duotone: shadows to highlights.")]),
    fx!("color", "levels", "Levels on display values: map `in_black`..`in_white` to `out_black`..`out_white` with a `gamma` midtone bend.",
        [p!("in_black", [0.0], num, "Input value that becomes `out_black`."), p!("in_white", [1.0], num, "Input value that becomes `out_white`."),
         p!("gamma", [1.0], x, 0.01, 10.0, "Above 1 brightens the midtones, below darkens them."),
         p!("out_black", [0.0], num, "Output for `in_black` and below."), p!("out_white", [1.0], num, "Output for `in_white` and above.")]),
    fx!("color", "vibrance", "Saturation that boosts dull colours more than vivid ones; negative mutes every colour evenly.",
        [p!("amount", [0.5], num, -1.0, 1.0, "-1 is black and white, 0 unchanged, 1 triples the saturation of grey-ish colours and leaves pure ones.")]),
];
