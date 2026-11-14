//! Keying effects: pointwise, they make part of the picture transparent (or,
//! for `spill_suppress`, clean up what a key leaves). A key multiplies the
//! layer's alpha where it runs in the effect list: after the layer's masks
//! (applied to what the layer draws, before its effects) and before its
//! track matte (applied after its effects).

use super::{fx, opt, p, EffectSpec, SourceKind};

pub(super) const EFFECTS: &[EffectSpec] = &[
    fx!("key", "chroma_key", "Green (or any colour) screen: colours close to `color` in chroma (YCbCr, display values) become transparent.",
        [p!("color", [0.0, 1.0, 0.0], color, "The screen's colour (linear RGB)."),
         p!("tolerance", [0.4], num, 0.0, 2.0, "Chroma distance within which a colour is keyed out completely; pure green to grey is 0.53."),
         p!("softness", [0.1], num, 0.0, 2.0, "Width of the ramp from keyed to kept, in the same distance."),
         p!("amount", [1.0], num, 0.0, 1.0, "How transparent a keyed colour becomes.")],
        options: &[opt!("view", ["final", "matte"], "final: the keyed picture; matte: the key's alpha as grey, to tune it.")],
        credit: "GPUImage GPUImageChromaKeyFilter (Brad Larson and contributors), BSD-3-Clause"),
    fx!("key", "spill_suppress", "Takes the screen's colour cast out of what a key leaves (green fringes and reflections).",
        [p!("color", [0.0, 1.0, 0.0], color, "The screen's colour (linear RGB)."),
         p!("amount", [1.0], num, 0.0, 1.0, "How much of the spill is removed.")],
        options: &[opt!("method", ["fast", "better"], "fast: the key colour's strongest channel is limited to the larger of the other two; better: the part of each colour that points along the key colour's chroma is removed (any key colour).")],
        credit: "EffectCraft keying.rs (Spill Suppressor), MIT OR Apache-2.0"),
    fx!("key", "luma_key", "Black (or white) out: transparency from the brightness of the layer itself.",
        [p!("threshold", [0.1], num, 0.0, 1.0, "Luminance (display, 0..1) where the key cuts."),
         p!("tolerance", [0.1], num, 0.0, 1.0, "similar / dissimilar: half the width of the band about `threshold`."),
         p!("softness", [0.05], num, 0.0, 1.0, "Width of the ramp from keyed to kept."),
         p!("amount", [1.0], num, 0.0, 1.0, "How transparent a keyed pixel becomes.")],
        options: &[opt!("key_out", ["darker", "brighter", "similar", "dissimilar"], "What becomes transparent: darker or brighter than `threshold`, or within (similar) or outside (dissimilar) `tolerance` of it."),
                   opt!("view", ["final", "matte"], "final: the keyed picture; matte: the key's alpha as grey.")],
        credit: "EffectCraft keying.rs (Luma Key), MIT OR Apache-2.0"),
    fx!("key", "unmult", "Black (or white) to transparent, for overlays shot on black: alpha becomes the brightest channel and the colour is divided by it, so the result over black is the original.",
        [p!("level", [0.0], num, 0.0, 1.0, "Distance from the background (the brightest channel on black, 1 - the darkest on white; linear) at which the alpha starts to rise."),
         p!("softness", [1.0], num, 0.0, 1.0, "Width of the alpha ramp as a share of what is left above `level`: 1 is the classic unmultiply, 0 a hard cut.")],
        options: &[opt!("background", ["black", "white"], "The background to remove: black (alpha from the brightest channel) or white (from the darkest, mirrored).")],
        credit: "EffectCraft keying2.rs (Unmult), MIT OR Apache-2.0"),
    fx!("key", "difference_matte", "Transparent where the picture matches a clean plate (the same shot without the subject), given as a slot.",
        [p!("tolerance", [0.05], num, 0.0, 1.0, "Largest channel difference (display) that still counts as background."),
         p!("softness", [0.1], num, 0.0, 1.0, "Width of the ramp from background to kept."),
         p!("amount", [1.0], num, 0.0, 1.0, "How transparent the background becomes.")],
        options: &[opt!("view", ["final", "matte"], "final: the keyed picture; matte: the key's alpha as grey.")],
        source: SourceKind::Slot, credit: "EffectCraft keying2.rs (Difference Matte), MIT OR Apache-2.0"),
];
