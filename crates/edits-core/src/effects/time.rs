//! Time effects: they render a layer at other times (echo, time
//! displacement). Each re-render is the layer as it is at that time, with the
//! effects before this one in its chain; on an adjustment layer, the layers
//! beneath it. Implemented in `edits-render/src/time.rs`.

use super::{fx, opt, p, EffectSpec, SourceKind};

pub(super) const EFFECTS: &[EffectSpec] = &[
    fx!("time", "echo", "Echoes: the layer's own picture at earlier (or later) times laid over it, each weaker than the last. Trails on fast motion, a stutter on a hit. Every echo re-renders the layer (or, on an adjustment layer, everything beneath it) at its time.",
        [p!("count", [3.0], count, 0.0, 16.0, "Echoes besides the current picture; 0 turns the effect off at no cost."),
         p!("interval", [-0.125], time, "Time from one echo to the next, in the project's time base; negative looks into the past."),
         p!("decay", [0.6], x, 0.0, 1.0, "Each echo's strength is the previous one's times this: echo i has start x decay^i."),
         p!("start", [1.0], num, 0.0, 1.0, "Strength of the current picture (echo 0).")],
        options: &[opt!("mode", ["add", "screen", "max", "min", "over_back", "over_front", "average"], "How echoes combine: add sums them (brightens), screen sums without passing white, max and min keep the brightest or darkest, over_back puts each echo behind the newer ones (the current picture stays in front), over_front puts each echo over the newer ones, average is the weighted mean (no brightening).")],
        credit: "after EffectCraft time_fx.rs echo operators (https://github.com/storytold/effectcraft), MIT OR Apache-2.0"),
    fx!("time", "time_displacement", "Each pixel shows the layer at a time shifted by the brightness of a map (a slot's picture stretched over the frame, or the layer's own luminance): a slit scan, a melting smear. The shifts are rounded to `levels` slices, each a re-render of the layer.",
        [p!("max", [-0.25], time, "Largest shift, in the project's time base: white shifts by `max`, black by -`max` (or by 0 with `neutral` black)."),
         p!("levels", [16.0], count, 2.0, 16.0, "Distinct times rendered; each pixel takes the nearest.")],
        options: &[opt!("neutral", ["grey", "black"], "The map value that shifts nothing: grey (black -max, white +max, as in After Effects) or black (black 0, white max: a one-sided slit scan).")],
        source: SourceKind::OptionalSlot,
        credit: "after EffectCraft time_displacement (https://github.com/storytold/effectcraft), MIT OR Apache-2.0"),
];
