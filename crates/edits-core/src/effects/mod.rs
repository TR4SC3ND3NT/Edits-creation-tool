//! The built-in effect registry: the single source of truth for effect kinds,
//! their parameters, options and defaults. The renderer implements exactly
//! these.
//!
//! The registry is split by category, one file each, so that work on
//! different kinds of effect touches different files: `color`, `blur`,
//! `light`, `distort`, `stylize`, `time` and `generate`. [`EFFECTS`] is all of
//! them in that order, followed by `custom`.
//!
//! Units (see [`Unit`]): spatial parameters are percent of the composition
//! height (`%h`), positions are composition units (`pos`: height = 1, origin
//! at the centre, y down), colours are linear RGB in 0..1, angles are degrees.

use serde::Serialize;

mod blur;
mod color;
mod distort;
mod generate;
mod light;
mod stylize;
mod time;

/// What a number means, which decides how many components it has and how
/// the renderer scales it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Unit {
    /// A plain number: an amount, a mix, a threshold.
    #[serde(rename = "")]
    Number,
    /// Percent of the composition height: 2 is 21.6 pixels at 1080 high.
    #[serde(rename = "%h")]
    PercentHeight,
    /// Degrees; positive turns clockwise on screen.
    #[serde(rename = "deg")]
    Degrees,
    /// A point `[x, y]` in composition units (height = 1, origin at the centre, y down).
    #[serde(rename = "pos")]
    Position,
    /// Linear RGB `[r, g, b]`, 0..1 (more is allowed and stays bright).
    #[serde(rename = "color")]
    Color,
    /// A multiplier (1 = unchanged).
    #[serde(rename = "x")]
    Factor,
    /// A length of time in the project's time base (beats or seconds), or a rate per such unit.
    #[serde(rename = "time")]
    Time,
    /// A whole number (rounded).
    #[serde(rename = "count")]
    Count,
    /// Photographic stops (powers of two).
    #[serde(rename = "stops")]
    Stops,
    /// Output pixels at 1080 high (scaled with the frame).
    #[serde(rename = "px1080")]
    Pixels1080,
}

impl Unit {
    /// The short name used in the registry's JSON (`""`, `"%h"`, `"deg"`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Number => "",
            Unit::PercentHeight => "%h",
            Unit::Degrees => "deg",
            Unit::Position => "pos",
            Unit::Color => "color",
            Unit::Factor => "x",
            Unit::Time => "time",
            Unit::Count => "count",
            Unit::Stops => "stops",
            Unit::Pixels1080 => "px1080",
        }
    }

    /// Components a value of this unit has, when the unit fixes it.
    pub fn arity(self) -> Option<usize> {
        match self {
            Unit::Position => Some(2),
            Unit::Color => Some(3),
            _ => None,
        }
    }
}

pub struct ParamSpec {
    pub name: &'static str,
    pub default: &'static [f64],
    pub doc: &'static str,
    pub unit: Unit,
    /// The values the renderer uses: outside `[min, max]` a value is clamped
    /// (`edits check` warns). Either bound may be infinite.
    pub range: Option<(f64, f64)>,
}

/// A choice that is not a number and cannot be animated, such as a wave's
/// shape. Set in an effect's `"options"`; the first value is the default.
pub struct OptionSpec {
    pub name: &'static str,
    pub values: &'static [&'static str],
    pub doc: &'static str,
}

impl OptionSpec {
    pub fn default(&self) -> &'static str {
        self.values[0]
    }
}

/// What an effect's `"source"` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// The effect takes no source.
    None,
    /// A WGSL file (the `custom` effect).
    Shader,
    /// A `.cube` lookup table file (the `lut` effect).
    Cube,
    /// A picture slot of the project, written `slot:<name>` (a map).
    Slot,
}

pub struct EffectSpec {
    pub kind: &'static str,
    /// `color`, `blur`, `light`, `distort`, `stylize`, `time`, `generate` or `custom`.
    pub category: &'static str,
    pub doc: &'static str,
    pub params: &'static [ParamSpec],
    pub options: &'static [OptionSpec],
    pub source: SourceKind,
    /// The picture changes with time even when no value does (a glitch that
    /// jumps a few times per beat), so every motion blur sample is rendered.
    pub reads_time: bool,
    /// Where the effect's maths comes from and under which licence; empty when it is ours.
    pub credit: &'static str,
}

impl EffectSpec {
    pub fn param(&self, name: &str) -> Option<&'static ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }
    pub fn option(&self, name: &str) -> Option<&'static OptionSpec> {
        self.options.iter().find(|o| o.name == name)
    }
}

/// A parameter: `p!(name, default, unit, doc)` or `p!(name, default, unit, min, max, doc)`.
/// `unit` is one of `num`, `pct`, `deg`, `pos`, `color`, `x`, `time`, `count`, `stops`, `px1080`.
macro_rules! p {
    ($n:literal, $d:expr, $u:ident, $doc:literal) => {
        $crate::effects::ParamSpec { name: $n, default: &$d, doc: $doc, unit: $crate::effects::unit!($u), range: None }
    };
    ($n:literal, $d:expr, $u:ident, $lo:expr, $hi:expr, $doc:literal) => {
        $crate::effects::ParamSpec { name: $n, default: &$d, doc: $doc, unit: $crate::effects::unit!($u), range: Some(($lo, $hi)) }
    };
}
macro_rules! unit {
    (num) => {
        $crate::effects::Unit::Number
    };
    (pct) => {
        $crate::effects::Unit::PercentHeight
    };
    (deg) => {
        $crate::effects::Unit::Degrees
    };
    (pos) => {
        $crate::effects::Unit::Position
    };
    (color) => {
        $crate::effects::Unit::Color
    };
    (x) => {
        $crate::effects::Unit::Factor
    };
    (time) => {
        $crate::effects::Unit::Time
    };
    (count) => {
        $crate::effects::Unit::Count
    };
    (stops) => {
        $crate::effects::Unit::Stops
    };
    (px1080) => {
        $crate::effects::Unit::Pixels1080
    };
}
/// An option: `opt!(name, [values...], doc)`; the first value is the default.
macro_rules! opt {
    ($n:literal, [$($v:literal),+ $(,)?], $doc:literal) => {
        $crate::effects::OptionSpec { name: $n, values: &[$($v),+], doc: $doc }
    };
}
/// An effect with no options, no source, not reading the clock and no credit:
/// `fx!(category, kind, doc, [params])`. More fields can follow as `field: value`.
macro_rules! fx {
    ($cat:literal, $kind:literal, $doc:literal, [$($p:expr),* $(,)?] $(, $f:ident: $v:expr)* $(,)?) => {
        $crate::effects::EffectSpec {
            $($f: $v,)*
            ..$crate::effects::EffectSpec {
                kind: $kind,
                category: $cat,
                doc: $doc,
                params: &[$($p),*],
                options: &[],
                source: $crate::effects::SourceKind::None,
                reads_time: false,
                credit: "",
            }
        }
    };
}
pub(crate) use {fx, opt, p, unit};

const CUSTOM: &[EffectSpec] = &[fx!("custom", "custom", "A WGSL fragment supplied by `source` (see docs/EFFECTS.md). Parameters p0..p15 are passed through as numbers.", [
    p!("p0", [0.0], num, ""), p!("p1", [0.0], num, ""), p!("p2", [0.0], num, ""), p!("p3", [0.0], num, ""),
    p!("p4", [0.0], num, ""), p!("p5", [0.0], num, ""), p!("p6", [0.0], num, ""), p!("p7", [0.0], num, ""),
    p!("p8", [0.0], num, ""), p!("p9", [0.0], num, ""), p!("p10", [0.0], num, ""), p!("p11", [0.0], num, ""),
    p!("p12", [0.0], num, ""), p!("p13", [0.0], num, ""), p!("p14", [0.0], num, ""), p!("p15", [0.0], num, ""),
], source: SourceKind::Shader, reads_time: true)];

const PARTS: &[&[EffectSpec]] = &[color::EFFECTS, blur::EFFECTS, light::EFFECTS, distort::EFFECTS, stylize::EFFECTS, time::EFFECTS, generate::EFFECTS, CUSTOM];

const fn total(parts: &[&[EffectSpec]]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < parts.len() {
        n += parts[i].len();
        i += 1;
    }
    n
}

const COUNT: usize = total(PARTS);

const fn concat(parts: &[&[EffectSpec]]) -> [EffectSpec; COUNT] {
    let mut out = [const { fx!("", "", "", []) }; COUNT];
    let (mut i, mut k) = (0, 0);
    while i < parts.len() {
        let mut j = 0;
        while j < parts[i].len() {
            let e = &parts[i][j];
            out[k] = EffectSpec {
                kind: e.kind,
                category: e.category,
                doc: e.doc,
                params: e.params,
                options: e.options,
                source: e.source,
                reads_time: e.reads_time,
                credit: e.credit,
            };
            k += 1;
            j += 1;
        }
        i += 1;
    }
    out
}

/// Every effect, by category (colour, blur, light, distort, stylize, time,
/// generate), then `custom`.
pub const EFFECTS: &[EffectSpec] = &concat(PARTS);

pub fn spec(kind: &str) -> Option<&'static EffectSpec> {
    EFFECTS.iter().find(|e| e.kind == kind)
}

// ---- transitions ----------------------------------------------------------------

/// A parameter of a transition. Colours are sRGB in 0..1; directions and
/// centres are in the transition's own frame (0..1, y up).
pub struct TransitionParam {
    pub name: &'static str,
    pub default: &'static [f64],
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// An RGB(A) colour.
    pub color: bool,
    pub doc: &'static str,
}

/// A two-picture transition: on a layer, `"transition": { "kind", "duration", "params" }`
/// replaces the cut into that layer by this effect over `duration` from the layer's `in`.
pub struct TransitionSpec {
    pub kind: &'static str,
    pub doc: &'static str,
    pub params: &'static [TransitionParam],
    /// A good default length, in beats.
    pub duration: f64,
    /// Where it comes from and under which licence.
    pub credit: &'static str,
}

macro_rules! tp {
    ($n:literal, $d:expr, $doc:literal) => {
        TransitionParam { name: $n, default: &$d, min: None, max: None, color: false, doc: $doc }
    };
    ($n:literal, $d:expr, $lo:expr, $hi:expr, $doc:literal) => {
        TransitionParam { name: $n, default: &$d, min: Some($lo), max: Some($hi), color: false, doc: $doc }
    };
    (color $n:literal, $d:expr, $doc:literal) => {
        TransitionParam { name: $n, default: &$d, min: Some(0.0), max: Some(1.0), color: true, doc: $doc }
    };
}

macro_rules! credit {
    ($name:literal, $author:literal) => {
        concat!("gl-transitions ", $name, " by ", $author, ", MIT licence (crates.io gl-transitions 2.0.2)")
    };
}

/// Every transition the renderer implements (ported from gl-transitions; see
/// `crates/edits-render/src/shaders/transitions/LICENSE.md`).
pub const TRANSITIONS: &[TransitionSpec] = &[
    TransitionSpec { kind: "fade", doc: "A plain crossfade.", params: &[], duration: 0.5, credit: credit!("Fade", "gre") },
    TransitionSpec { kind: "fade_color", doc: "Dip through a colour (black by default) between the pictures.",
        params: &[tp!(color "color", [0.0, 0.0, 0.0, 1.0], "Colour to dip through (sRGB, with alpha)."), tp!("color_phase", [0.4], 0.0, 1.0, "How long the colour holds in the middle.")], duration: 1.0, credit: credit!("FadeColor", "gre") },
    TransitionSpec { kind: "flash_fade", doc: "A crossfade that flares toward a colour in the middle.",
        params: &[tp!(color "color", [1.0, 1.0, 1.0], "Colour of the flare (sRGB).")], duration: 0.5, credit: credit!("FlashFade", "gre") },
    TransitionSpec { kind: "overexposure", doc: "A crossfade through a washed-out white.",
        params: &[tp!("strength", [0.6], 0.0, 4.0, "Amount of overexposure.")], duration: 0.5, credit: credit!("Overexposure", "Ben Zhang") },
    TransitionSpec { kind: "lightning_flash", doc: "Lightning bolts crack down, the screen strobes white and the next picture appears lit by the last flickers.",
        params: &[tp!("bolts", [2.0], 1.0, 3.0, "Number of bolts."), tp!("strike", [0.55], 0.0, 1.0, "Where the main bolt strikes, left (0) to right (1)."), tp!("seed", [0.0], 0.0, 100.0, "Changes the shape of the bolts."),
                  tp!(color "color", [0.75, 0.85, 1.0], "Colour of the light (sRGB)."), tp!("glow", [0.5], 0.0, 1.0, "Glow around the bolts."), tp!("flashes", [3.0], 1.0, 4.0, "Number of strobes.")], duration: 1.0, credit: credit!("LightningFlash", "gre") },
    TransitionSpec { kind: "cross_zoom", doc: "A crossfade through a strong zoom blur, the classic hit cut.",
        params: &[tp!("strength", [0.4], 0.0, 1.0, "Blur strength.")], duration: 0.5, credit: credit!("CrossZoom", "rectalogic") },
    TransitionSpec { kind: "simple_zoom", doc: "Zoom into the outgoing picture, then fade to the next.",
        params: &[tp!("zoom_quickness", [0.8], 0.2, 1.0, "Part of the transition spent zooming.")], duration: 0.5, credit: credit!("SimpleZoom", "0gust1") },
    TransitionSpec { kind: "zoom_in_out", doc: "Zoom into the outgoing picture until it blurs into flat colour, then out of the next.", params: &[], duration: 1.0, credit: credit!("ZoomInOut", "OllyOllyOlly") },
    TransitionSpec { kind: "dreamy_zoom", doc: "Zoom and rotate into a bright flash, then out to the next picture.",
        params: &[tp!("rotation", [6.0], -90.0, 90.0, "Rotation at the peak, degrees."), tp!("scale", [1.2], 0.5, 4.0, "Zoom at the peak.")], duration: 1.0, credit: credit!("DreamyZoom", "Zeh Fernando") },
    TransitionSpec { kind: "directional", doc: "The next picture pushes the outgoing one out of frame (a smooth slide).",
        params: &[tp!("direction", [0.0, 1.0], "Direction of the push: [1, 0] right, [0, 1] up.")], duration: 0.5, credit: credit!("Directional", "Gaëtan Renaudeau") },
    TransitionSpec { kind: "directional_warp", doc: "A diagonal wipe with a warped, smeared edge.",
        params: &[tp!("smoothness", [0.1], 0.0, 1.0, "Softness of the edge."), tp!("direction", [-1.0, 1.0], "Direction of the wipe.")], duration: 0.5, credit: credit!("DirectionalWarp", "pschroen") },
    TransitionSpec { kind: "whip_pan", doc: "A fast camera pan that streaks both pictures with motion blur.",
        params: &[tp!("direction", [-1.0, 0.0], "Direction the pictures move: [-1, 0] left, [0, 1] up."), tp!("blur", [0.4], 0.0, 1.0, "Length of the motion blur.")], duration: 0.5, credit: credit!("WhipPan", "gre") },
    TransitionSpec { kind: "whip_spin", doc: "The camera rolls faster and faster in a spin blur; the next picture takes over mid-spin and slows to a stop.",
        params: &[tp!("turns", [1.0], -3.0, 3.0, "Total rotation in turns (negative: clockwise)."), tp!("blur", [0.5], 0.0, 1.0, "Motion blur along the spin."), tp!("chroma", [0.6], 0.0, 1.0, "Colour split as it speeds up."), tp!("zoom", [0.25], 0.0, 1.0, "Zoom at the fastest point.")], duration: 0.75, credit: credit!("WhipSpin", "gre") },
    TransitionSpec { kind: "whip_tilt", doc: "A fast tilt to the next picture set above (or below) at an angle, with vertical motion blur.",
        params: &[tp!("down", [0.0], 0.0, 1.0, "1 tilts down to a picture below instead of above."), tp!("blur", [0.5], 0.0, 1.0, "Motion blur along the tilt."), tp!("chroma", [0.5], 0.0, 1.0, "Colour split as it speeds up."), tp!("perspective", [0.7], 0.0, 1.0, "Strength of the perspective.")], duration: 0.75, credit: credit!("WhipTilt", "gre") },
    TransitionSpec { kind: "camera_shake", doc: "The camera jolts in random directions with motion blur and colour fringes; the cut happens at the peak.",
        params: &[tp!("strength", [0.5], 0.0, 1.0, "How far the camera jolts."), tp!("jolts", [12.0], 4.0, 32.0, "Number of jolts."), tp!("blur", [0.5], 0.0, 1.0, "Motion blur along each jolt."), tp!("split", [0.5], 0.0, 1.0, "Colour split along each jolt.")], duration: 0.75, credit: credit!("CameraShake", "gre") },
    TransitionSpec { kind: "glitch_memories", doc: "A crossfade with glitchy, colour-shifted blocks.", params: &[], duration: 0.5, credit: credit!("GlitchMemories", "Gunnar Roth") },
    TransitionSpec { kind: "glitch_displace", doc: "Blocks of the picture are displaced in a glitch that flashes into the next.", params: &[], duration: 0.5, credit: credit!("GlitchDisplace", "Matt DesLauriers") },
    TransitionSpec { kind: "cube", doc: "The two pictures are faces of a box that turns a quarter turn, above a reflective floor.",
        params: &[tp!("persp", [0.7], 0.0, 1.0, "Amount of perspective."), tp!("unzoom", [0.3], 0.0, 1.0, "How far the camera pulls back mid-turn."), tp!("reflection", [0.4], 0.0, 1.0, "Strength of the reflection."), tp!("floating", [3.0], 0.0, 20.0, "Gap between box and reflection, % of height."), tp!("shading", [0.5], 0.0, 1.0, "Darkening of the faces as they turn away.")], duration: 1.0, credit: credit!("Cube", "gre") },
    TransitionSpec { kind: "swirl", doc: "The picture twists into a swirl that peaks mid-way, where it dissolves into the next, then untwists.",
        params: &[tp!("strength", [1.0], -3.0, 3.0, "Turns of the swirl at its centre."), tp!("radius", [0.9], 0.1, 2.0, "Radius in frame heights."), tp!("center", [0.5, 0.5], "Centre (0..1, y up).")], duration: 1.0, credit: credit!("Swirl", "Sergey Kosarevsky") },
    TransitionSpec { kind: "rotate_scale_fade", doc: "The outgoing picture spins and zooms while fading to the next, over a colour.",
        params: &[tp!("center", [0.5, 0.5], "Centre of the rotation (0..1, y up)."), tp!("rotations", [1.0], -8.0, 8.0, "Number of turns."), tp!("scale", [8.0], 0.0, 32.0, "Maximum zoom."), tp!(color "back_color", [0.15, 0.15, 0.15, 1.0], "Background colour (sRGB, with alpha).")], duration: 1.0, credit: credit!("RotateScaleFade", "Fernando Kuteken") },
    TransitionSpec { kind: "pixelize", doc: "Both pictures pixelate, crossfade, then sharpen again.",
        params: &[tp!("squares_min", [20.0, 20.0], "Squares across and down at the strongest pixelation."), tp!("steps", [50.0], 0.0, 200.0, "Number of steps; 0 is continuous.")], duration: 0.5, credit: credit!("Pixelize", "gre") },
    TransitionSpec { kind: "film_burn", doc: "Glowing film-burn light leaks wash over the picture and reveal the next.",
        params: &[tp!("seed", [2.31], "Random seed.")], duration: 1.0, credit: credit!("FilmBurn", "Anastasia Dunbar") },
];

pub fn transition(kind: &str) -> Option<&'static TransitionSpec> {
    TRANSITIONS.iter().find(|t| t.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_is_consistent() {
        let mut kinds: Vec<&str> = EFFECTS.iter().map(|e| e.kind).collect();
        let n = kinds.len();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), n, "effect kinds are unique");
        for e in EFFECTS {
            assert!(!e.doc.is_empty() && !e.category.is_empty(), "{}", e.kind);
            for p in e.params {
                if let Some(n) = p.unit.arity() {
                    assert_eq!(p.default.len(), n, "{}.{}: a {:?} has {n} components", e.kind, p.name, p.unit);
                }
                if let Some((lo, hi)) = p.range {
                    assert!(lo <= hi && p.default.iter().all(|v| (lo..=hi).contains(v)), "{}.{}: the default is outside its range", e.kind, p.name);
                }
            }
            for o in e.options {
                assert!(!o.values.is_empty(), "{}.{}", e.kind, o.name);
            }
        }
        assert_eq!(EFFECTS.last().map(|e| e.kind), Some("custom"));
        assert_eq!(spec("glow").map(|e| e.category), Some("light"));
    }
}
