//! Light: glows, rays, flares, and the layer styles made of light and shade
//! (outline, drop shadow). All of them are multi-pass effects in
//! `edits-render/src/fx.rs`.

use super::{fx, opt, p, EffectSpec};

const INF: f64 = f64::INFINITY;

pub(super) const EFFECTS: &[EffectSpec] = &[
    fx!("light", "glow", "Bloom: blur the bright parts and add them back.",
        [p!("threshold", [0.7], num, "Luminance above which pixels glow."), p!("radius", [3.0], pct, 0.0, INF, "Glow sigma, % of height."),
         p!("intensity", [1.0], num, 0.0, INF, "Strength of the added glow."), p!("color", [1.0, 1.0, 1.0], color, "Tint of the glow.")]),
    fx!("light", "bloom", "Deep, soft bloom: the bright parts spread light at many sizes at once (a mip chain), like light scattering in a lens. Softer and wider than `glow`.",
        [p!("threshold", [0.8], num, 0.0, INF, "Brightness (the largest of r, g, b, linear) above which light blooms."),
         p!("knee", [0.2], num, 0.0, 1.0, "Softness of the threshold, as a fraction of it: 0 cuts hard, 1 starts blooming at 0."),
         p!("intensity", [1.0], num, 0.0, INF, "Strength of the added light; the energy added grows in proportion."),
         p!("radius", [10.0], pct, 0.0, INF, "Size of the widest part of the bloom, % of height."),
         p!("color", [1.0, 1.0, 1.0], color, "Tint of the bloom.")],
        credit: "Bevy bloom.wesl (Call of Duty 13-tap downsample, tent upsample, Karis average, soft threshold), MIT OR Apache-2.0"),
    fx!("light", "light_rays", "God rays: the bright parts streak away from a point, as light through haze.",
        [p!("center", [0.0, 0.0], pos, "Where the rays come from, in composition units."),
         p!("length", [0.5], num, 0.0, 0.95, "Length of the rays as a fraction of the distance to `center`."),
         p!("intensity", [1.0], num, 0.0, INF, "Strength of the rays."), p!("threshold", [0.6], num, 0.0, INF, "Luminance above which pixels cast rays."),
         p!("color", [1.0, 1.0, 1.0], color, "Tint of the rays.")],
        options: &[opt!("mode", ["add", "screen", "lighten"], "How the rays combine with the picture.")],
        credit: "EffectCraft light_rays / flt_rays (samples scaled toward the centre; transfer modes), MIT OR Apache-2.0"),
    fx!("light", "lens_flare", "A lens flare: a bright core with streaks and a halo at `position`, and coloured ghosts along the line through the frame's centre. Drawn over the picture; put it on an adjustment layer.",
        [p!("position", [-0.4, -0.25], pos, "Where the light source is, in composition units."),
         p!("brightness", [1.0], num, 0.0, INF, "Strength of the flare."), p!("size", [1.0], x, 0.0, INF, "Scale of the core, halo and ghosts.")],
        options: &[opt!("lens", ["zoom", "prime35", "prime105"], "The ghost pattern: a 50-300 mm zoom (seven ghosts), a 35 mm prime (four) or a 105 mm prime (three).")],
        credit: "EffectCraft lens_flare and flare_ghosts (core, streaks, halo and the ghost tables), MIT OR Apache-2.0"),
    fx!("light", "outline", "A stroke around the layer's shape (its alpha), found with a jump flood.",
        [p!("width", [0.6], pct, 0.0, INF, "Width of the stroke, % of height."), p!("color", [1.0, 1.0, 1.0], color, "Colour of the stroke."),
         p!("softness", [0.0], pct, 0.0, INF, "Extra softness of the stroke's edges, % of height."), p!("opacity", [1.0], num, 0.0, 1.0, "Opacity of the stroke.")],
        options: &[opt!("position", ["outside", "inside", "center"], "Where the stroke sits relative to the shape's edge.")]),
    fx!("light", "drop_shadow", "A shadow of the layer's shape, offset and softened, drawn beneath it.",
        [p!("offset", [0.01, 0.015], pos, "Shift of the shadow, composition units (y down)."), p!("softness", [1.0], pct, 0.0, INF, "Blur of the shadow (sigma), % of height."),
         p!("color", [0.0, 0.0, 0.0], color, "Colour of the shadow."), p!("opacity", [0.6], num, 0.0, 1.0, "Opacity of the shadow.")]),
];
