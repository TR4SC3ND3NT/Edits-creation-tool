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
    /// A slot as for [`SourceKind::Slot`], or nothing: the effect then reads
    /// the layer's own picture (`time_displacement`).
    OptionalSlot,
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
    /// What the transition's `source` may name: [`SourceKind::None`], or
    /// [`SourceKind::OptionalSlot`] for a map picture (`luma_wipe`).
    pub source: SourceKind,
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
    ($name:literal, $author:literal, $version:literal) => {
        concat!("gl-transitions ", $name, " by ", $author, ", MIT licence (crates.io gl-transitions ", $version, ")")
    };
}

/// A transition of wave 2 (crates.io gl-transitions 2.0.3) that takes no map.
macro_rules! tr {
    ($kind:literal, $file:literal, $author:literal, $duration:expr, $doc:literal, [$($p:expr),* $(,)?]) => {
        TransitionSpec { kind: $kind, doc: $doc, params: &[$($p),*], duration: $duration, credit: credit!($file, $author, "2.0.3"), source: SourceKind::None }
    };
}

/// Every transition the renderer implements (ported from gl-transitions; see
/// `crates/edits-render/src/shaders/transitions/LICENSE.md`).
pub const TRANSITIONS: &[TransitionSpec] = &[
    TransitionSpec { kind: "fade", doc: "A plain crossfade.", params: &[], duration: 0.5, credit: credit!("Fade", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "fade_color", doc: "Dip through a colour (black by default) between the pictures.",
        params: &[tp!(color "color", [0.0, 0.0, 0.0, 1.0], "Colour to dip through (sRGB, with alpha)."), tp!("color_phase", [0.4], 0.0, 1.0, "How long the colour holds in the middle.")], duration: 1.0, credit: credit!("FadeColor", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "flash_fade", doc: "A crossfade that flares toward a colour in the middle.",
        params: &[tp!(color "color", [1.0, 1.0, 1.0], "Colour of the flare (sRGB).")], duration: 0.5, credit: credit!("FlashFade", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "overexposure", doc: "A crossfade through a washed-out white.",
        params: &[tp!("strength", [0.6], 0.0, 4.0, "Amount of overexposure.")], duration: 0.5, credit: credit!("Overexposure", "Ben Zhang"), source: SourceKind::None },
    TransitionSpec { kind: "lightning_flash", doc: "Lightning bolts crack down, the screen strobes white and the next picture appears lit by the last flickers.",
        params: &[tp!("bolts", [2.0], 1.0, 3.0, "Number of bolts."), tp!("strike", [0.55], 0.0, 1.0, "Where the main bolt strikes, left (0) to right (1)."), tp!("seed", [0.0], 0.0, 100.0, "Changes the shape of the bolts."),
                  tp!(color "color", [0.75, 0.85, 1.0], "Colour of the light (sRGB)."), tp!("glow", [0.5], 0.0, 1.0, "Glow around the bolts."), tp!("flashes", [3.0], 1.0, 4.0, "Number of strobes.")], duration: 1.0, credit: credit!("LightningFlash", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "cross_zoom", doc: "A crossfade through a strong zoom blur, the classic hit cut.",
        params: &[tp!("strength", [0.4], 0.0, 1.0, "Blur strength.")], duration: 0.5, credit: credit!("CrossZoom", "rectalogic"), source: SourceKind::None },
    TransitionSpec { kind: "simple_zoom", doc: "Zoom into the outgoing picture, then fade to the next.",
        params: &[tp!("zoom_quickness", [0.8], 0.2, 1.0, "Part of the transition spent zooming.")], duration: 0.5, credit: credit!("SimpleZoom", "0gust1"), source: SourceKind::None },
    TransitionSpec { kind: "zoom_in_out", doc: "Zoom into the outgoing picture until it blurs into flat colour, then out of the next.", params: &[], duration: 1.0, credit: credit!("ZoomInOut", "OllyOllyOlly"), source: SourceKind::None },
    TransitionSpec { kind: "dreamy_zoom", doc: "Zoom and rotate into a bright flash, then out to the next picture.",
        params: &[tp!("rotation", [6.0], -90.0, 90.0, "Rotation at the peak, degrees."), tp!("scale", [1.2], 0.5, 4.0, "Zoom at the peak.")], duration: 1.0, credit: credit!("DreamyZoom", "Zeh Fernando"), source: SourceKind::None },
    TransitionSpec { kind: "directional", doc: "The next picture pushes the outgoing one out of frame (a smooth slide).",
        params: &[tp!("direction", [0.0, 1.0], "Direction of the push: [1, 0] right, [0, 1] up.")], duration: 0.5, credit: credit!("Directional", "Gaëtan Renaudeau"), source: SourceKind::None },
    TransitionSpec { kind: "directional_warp", doc: "A diagonal wipe with a warped, smeared edge.",
        params: &[tp!("smoothness", [0.1], 0.0, 1.0, "Softness of the edge."), tp!("direction", [-1.0, 1.0], "Direction of the wipe.")], duration: 0.5, credit: credit!("DirectionalWarp", "pschroen"), source: SourceKind::None },
    TransitionSpec { kind: "whip_pan", doc: "A fast camera pan that streaks both pictures with motion blur.",
        params: &[tp!("direction", [-1.0, 0.0], "Direction the pictures move: [-1, 0] left, [0, 1] up."), tp!("blur", [0.4], 0.0, 1.0, "Length of the motion blur.")], duration: 0.5, credit: credit!("WhipPan", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "whip_spin", doc: "The camera rolls faster and faster in a spin blur; the next picture takes over mid-spin and slows to a stop.",
        params: &[tp!("turns", [1.0], -3.0, 3.0, "Total rotation in turns (negative: clockwise)."), tp!("blur", [0.5], 0.0, 1.0, "Motion blur along the spin."), tp!("chroma", [0.6], 0.0, 1.0, "Colour split as it speeds up."), tp!("zoom", [0.25], 0.0, 1.0, "Zoom at the fastest point.")], duration: 0.75, credit: credit!("WhipSpin", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "whip_tilt", doc: "A fast tilt to the next picture set above (or below) at an angle, with vertical motion blur.",
        params: &[tp!("down", [0.0], 0.0, 1.0, "1 tilts down to a picture below instead of above."), tp!("blur", [0.5], 0.0, 1.0, "Motion blur along the tilt."), tp!("chroma", [0.5], 0.0, 1.0, "Colour split as it speeds up."), tp!("perspective", [0.7], 0.0, 1.0, "Strength of the perspective.")], duration: 0.75, credit: credit!("WhipTilt", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "camera_shake", doc: "The camera jolts in random directions with motion blur and colour fringes; the cut happens at the peak.",
        params: &[tp!("strength", [0.5], 0.0, 1.0, "How far the camera jolts."), tp!("jolts", [12.0], 4.0, 32.0, "Number of jolts."), tp!("blur", [0.5], 0.0, 1.0, "Motion blur along each jolt."), tp!("split", [0.5], 0.0, 1.0, "Colour split along each jolt.")], duration: 0.75, credit: credit!("CameraShake", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "glitch_memories", doc: "A crossfade with glitchy, colour-shifted blocks.", params: &[], duration: 0.5, credit: credit!("GlitchMemories", "Gunnar Roth"), source: SourceKind::None },
    TransitionSpec { kind: "glitch_displace", doc: "Blocks of the picture are displaced in a glitch that flashes into the next.", params: &[], duration: 0.5, credit: credit!("GlitchDisplace", "Matt DesLauriers"), source: SourceKind::None },
    TransitionSpec { kind: "cube", doc: "The two pictures are faces of a box that turns a quarter turn, above a reflective floor.",
        params: &[tp!("persp", [0.7], 0.0, 1.0, "Amount of perspective."), tp!("unzoom", [0.3], 0.0, 1.0, "How far the camera pulls back mid-turn."), tp!("reflection", [0.4], 0.0, 1.0, "Strength of the reflection."), tp!("floating", [3.0], 0.0, 20.0, "Gap between box and reflection, % of height."), tp!("shading", [0.5], 0.0, 1.0, "Darkening of the faces as they turn away.")], duration: 1.0, credit: credit!("Cube", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "swirl", doc: "The picture twists into a swirl that peaks mid-way, where it dissolves into the next, then untwists.",
        params: &[tp!("strength", [1.0], -3.0, 3.0, "Turns of the swirl at its centre."), tp!("radius", [0.9], 0.1, 2.0, "Radius in frame heights."), tp!("center", [0.5, 0.5], "Centre (0..1, y up).")], duration: 1.0, credit: credit!("Swirl", "Sergey Kosarevsky"), source: SourceKind::None },
    TransitionSpec { kind: "rotate_scale_fade", doc: "The outgoing picture spins and zooms while fading to the next, over a colour.",
        params: &[tp!("center", [0.5, 0.5], "Centre of the rotation (0..1, y up)."), tp!("rotations", [1.0], -8.0, 8.0, "Number of turns."), tp!("scale", [8.0], 0.0, 32.0, "Maximum zoom."), tp!(color "back_color", [0.15, 0.15, 0.15, 1.0], "Background colour (sRGB, with alpha).")], duration: 1.0, credit: credit!("RotateScaleFade", "Fernando Kuteken"), source: SourceKind::None },
    TransitionSpec { kind: "pixelize", doc: "Both pictures pixelate, crossfade, then sharpen again.",
        params: &[tp!("squares_min", [20.0, 20.0], "Squares across and down at the strongest pixelation."), tp!("steps", [50.0], 0.0, 200.0, "Number of steps; 0 is continuous.")], duration: 0.5, credit: credit!("Pixelize", "gre"), source: SourceKind::None },
    TransitionSpec { kind: "film_burn", doc: "Glowing film-burn light leaks wash over the picture and reveal the next.",
        params: &[tp!("seed", [2.31], "Random seed.")], duration: 1.0, credit: credit!("FilmBurn", "Anastasia Dunbar"), source: SourceKind::None },
    // Wave 2 (docs/EFFECT_CATALOG.md): wipes, shapes, flips, folds, ripples and glitches.
    tr!("zoom_out", "SimpleZoomOut", "Tianshuo", 0.5, "The next picture zooms out into place from close up while fading in.",
        [tp!("zoom_quickness", [0.8], 0.2, 1.0, "Part of the transition spent zooming."), tp!("fade", [1.0], 0.0, 1.0, "1 fades the next picture in; 0 cuts to it as the zoom starts.")]),
    tr!("directional_eased", "DirectionalEased", "Max Plotnikov", 0.5, "The next picture pushes the outgoing one out of frame, fast at first and easing to a stop.",
        [tp!("direction", [0.0, 1.0], "Direction of the push: [1, 0] right, [0, 1] up.")]),
    tr!("linear_wipe", "DirectionalWipe", "gre", 0.5, "A straight wipe in any direction, diagonal and soft-edged by default.",
        [tp!("direction", [1.0, -1.0], "Direction the edge travels: [1, 0] left to right, [0, 1] bottom to top, [1, -1] top-left to bottom-right."), tp!("smoothness", [0.5], 0.0, 1.0, "Softness of the edge (0: hard).")]),
    tr!("clock_wipe", "ClockWipe", "Fernando Kuteken", 0.5, "A hard edge sweeps counter-clockwise around the centre like a clock hand, revealing the next picture.",
        [tp!("start_angle", [90.0], 0.0, 360.0, "Where the sweep starts, degrees: 0 at 9 o'clock, 90 at 12, 180 at 3, 270 at 6.")]),
    tr!("radial_wipe", "RadialWipe", "Xaychru", 0.5, "A soft-edged sweep around the centre, starting at 9 o'clock and turning counter-clockwise, like a radar.",
        [tp!("smoothness", [1.0], 0.01, 3.0, "Softness of the sweeping edge, in radians of the turn.")]),
    tr!("circle_open", "CircleOpen", "gre", 0.5, "A soft-edged circle opens from the centre to reveal the next picture (or closes toward it).",
        [tp!("smoothness", [0.3], 0.0, 1.0, "Softness of the circle's edge."), tp!("opening", [1.0], 0.0, 1.0, "1 opens from the centre; 0 closes toward it.")]),
    tr!("heart_wipe", "HeartWipe", "gre", 1.0, "A heart grows from a point with a little heartbeat and a glowing edge to reveal the next picture.",
        [tp!("center", [0.5, 0.5], "Centre of the heart (0..1, y up)."), tp!("smoothness", [0.006], 0.0, 0.1, "Softness of the heart's edge."), tp!("pulse", [0.5], 0.0, 1.0, "Strength of the heartbeat throb as it grows."),
         tp!(color "glow_color", [1.0, 0.85, 0.9], "Colour of the light along the edge (sRGB)."), tp!("glow", [0.3], 0.0, 1.0, "Strength of the glow (0: a clean edge).")]),
    tr!("star_wipe", "StarWipe", "Ben Lucas", 1.0, "A five-pointed star with a border grows from a point to reveal the next picture.",
        [tp!("border", [1.0], 0.0, 10.0, "Width of the star's border, % of height."), tp!("rotation", [54.0], -360.0, 360.0, "Turn of the star, degrees."),
         tp!(color "border_color", [1.0, 1.0, 1.0, 1.0], "Colour of the border (sRGB, with alpha)."), tp!("center", [0.5, 0.5], "Centre of the star (0..1, y up).")]),
    tr!("diamond_wipe", "DiamondWipe", "gre", 0.5, "A soft-edged diamond grows from a point to reveal the next picture.",
        [tp!("center", [0.5, 0.5], "Centre of the diamond (0..1, y up)."), tp!("smoothness", [0.03], 0.0, 0.5, "Softness of the diamond's edge.")]),
    tr!("random_squares", "RandomSquares", "gre", 0.5, "The next picture appears in a grid of squares that switch in random order (a block dissolve).",
        [tp!("squares", [10.0, 10.0], "Squares across and down."), tp!("smoothness", [0.5], 0.0, 1.0, "How gradually each square switches.")]),
    tr!("noise_dissolve", "NoiseDissolve", "towrabbit", 0.5, "The next picture appears pixel by pixel in random noise.", []),
    TransitionSpec { kind: "luma_wipe", doc: "A wipe that follows the brightness of a map, darkest parts first: \"source\": \"slot:<name>\" names the map (a gradient, a texture, a shape); without one, the outgoing picture's own brightness leads the way.",
        params: &[tp!("softness", [0.1], 0.0, 1.0, "Softness of the edge, as a part of the map's brightness range (0: hard)."), tp!("invert", [0.0], 0.0, 1.0, "1 makes the brightest parts go first.")],
        duration: 1.0, credit: "gl-transitions LumaWipe by gre, MIT licence (crates.io gl-transitions 2.0.3); luminance of any map, softness and invert added in edits", source: SourceKind::OptionalSlot },
    tr!("window_blinds", "WindowBlinds", "Fabien Benetou", 0.5, "A crossfade through thin horizontal blinds that flicker between the pictures.", []),
    tr!("window_slices", "WindowSlices", "gre", 0.5, "The next picture appears through vertical slices that widen from left to right.",
        [tp!("count", [10.0], 1.0, 100.0, "Number of slices."), tp!("smoothness", [0.5], 0.0, 1.0, "How gradually the slices widen across the frame.")]),
    tr!("grid_flip", "GridFlip", "TimDonselaar", 1.0, "The picture splits into a grid of cards that lift apart and flip over in perspective one after another, showing the next picture on their backs.",
        [tp!("tiles", [4.0, 4.0], "Tiles across and down."), tp!("pause", [0.1], 0.0, 0.4, "Part of the transition at each end during which the gaps open and close."),
         tp!("gap", [0.05], 0.0, 0.4, "Width of the gaps, as a part of a tile's shorter side."), tp!(color "background", [0.0, 0.0, 0.0, 1.0], "Colour behind the tiles (sRGB, with alpha)."),
         tp!("randomness", [0.1], 0.0, 1.0, "Order of the flips: 0 a diagonal wave, 1 random.")]),
    tr!("inside_cube", "InsideCube", "gre", 1.0, "The camera stands inside a cube and turns from the wall with the outgoing picture to the next wall, in perspective, with shaded corners.",
        [tp!("direction", [1.0, 0.0], "Where the view turns: [1, 0] right, [-1, 0] left, [0, 1] up, [0, -1] down."), tp!("fov", [90.0], 60.0, 120.0, "Field of view along the turn, degrees: wider makes the walls recede more."),
         tp!("shading", [0.6], 0.0, 1.0, "Darkening of the walls as they turn away, and the corner shadow."), tp!(color "background", [0.09, 0.09, 0.1, 1.0], "Colour of the cube's other faces, seen where the walls leave gaps (sRGB, with alpha).")]),
    tr!("simple_flip", "SimpleFlip", "nwoeanhinnogaehr", 0.5, "The picture flips around its vertical axis like a card, the next one on its back, over the background.", []),
    tr!("book_flip", "BookFlip", "hong", 1.0, "The right half of the picture is a book page that turns over from right to left, curling, with the next picture on its back.",
        [tp!("perspective", [0.5], 0.0, 1.0, "Strength of the perspective as the page lifts."), tp!("shading", [0.6], 0.0, 1.0, "Darkening of the page as it turns, and its shadows."), tp!("curl", [0.5], 0.0, 1.0, "How much the page bends near its free edge.")]),
    tr!("fold", "Fold", "nwoeanhinnogaehr", 0.5, "The next picture unfolds from the left while the outgoing one folds away to the right.", []),
    tr!("doorway", "Doorway", "gre", 1.0, "The picture opens like double doors swinging toward the viewer as the next one comes forward from behind, with a floor reflection.",
        [tp!("reflection", [0.4], 0.0, 1.0, "Strength of the floor reflection."), tp!("perspective", [0.4], 0.0, 1.0, "Amount of perspective on the doors."), tp!("depth", [3.0], 1.0, 10.0, "How far back the next picture starts (1: no distance).")]),
    tr!("horizontal_open", "HorizontalOpen", "martiniti", 0.5, "The next picture opens from a line across the middle toward the top and bottom edges.", []),
    tr!("split_slide", "SplitSlideInHorizontal", "OllyOllyOlly", 0.5, "The next picture slides in as two halves from opposite sides: the top half from the left, the bottom half from the right.",
        [tp!("reverse", [0.0], 0.0, 1.0, "1 swaps the sides the halves come from.")]),
    tr!("ripple", "Ripple", "gre", 1.0, "Ripples spread from the centre through the outgoing picture as it crossfades.",
        [tp!("frequency", [100.0], 0.0, 300.0, "Frequency of the ripples."), tp!("speed", [50.0], 0.0, 200.0, "Speed of the ripples.")]),
    tr!("water_drop", "WaterDrop", "Paweł Płóciennik", 1.0, "A drop falls into the picture like into still water: round ripples spread with glints, the next picture shows inside the leading ripple, and the water calms.",
        [tp!("frequency", [30.0], 10.0, 80.0, "Frequency of the ripples: higher makes them closer together."), tp!("speed", [30.0], 0.0, 80.0, "Speed of the ripples as they run outward."),
         tp!("center", [0.5, 0.5], "Where the drop falls (0..1, y up)."), tp!("refraction", [1.5], 0.0, 8.0, "How far the ripples bend the picture, % of height."), tp!("highlight", [0.5], 0.0, 1.0, "Brightness of the glints.")]),
    tr!("glitch_bands", "GlitchBands", "Yoni Maltsman", 0.5, "A crossfade in which the outgoing picture breaks into glitchy bands driven by its own colours.",
        [tp!("horizontal", [1.0], 0.0, 5.0, "Horizontal amplitude of the glitch."), tp!("vertical", [1.0], 0.0, 5.0, "Vertical amplitude of the glitch.")]),
    tr!("glitch_flicker", "GlitchFlicker", "bread", 0.5, "A stepped digital glitch with RGB offset, torn blocks, ghosting and scanlines that flickers between the pictures.",
        [tp!("flickers", [24.0], 1.0, 60.0, "Glitch steps over the transition."), tp!("rgb_offset", [1.4], 0.0, 10.0, "Red/blue offset at the peak, % of the frame."),
         tp!("blocks", [0.72], 0.0, 1.0, "Amount of torn blocks."), tp!("ghost", [0.62], 0.0, 1.0, "Amount of ghosting between the pictures."),
         tp!("red_cyan", [0.58], 0.0, 1.0, "Strength of the red/cyan split."), tp!("scanlines", [0.075], 0.0, 1.0, "Strength of the scanlines.")]),
    tr!("vhs_tracking", "VhsTracking", "gre", 1.0, "Tape-tracking bands of snow roll down the frame, tearing and smearing the lines, as the next picture is recorded over the outgoing one.",
        [tp!("bands", [3.0], 1.0, 4.0, "Number of noise bands (the first carries the cut)."), tp!("noise", [0.6], 0.0, 1.0, "Snow and tearing in the bands."),
         tp!("bleed", [0.5], 0.0, 1.0, "How far the colour smears to the right."), tp!("wobble", [0.5], 0.0, 1.0, "How much the lines wobble sideways.")]),
    tr!("tv_static", "TvStatic", "Brandon Anzaldi", 0.5, "Cuts to full-screen TV static, then to the next picture.",
        [tp!("offset", [0.05], 0.0, 0.5, "Part of the transition at each end that shows the pictures instead of static.")]),
    tr!("stretch_slide", "StretchSlide", "gre", 0.75, "The outgoing picture stretches long as it shoots away to one side, pulling in the next one squashed behind it, which overshoots and springs back.",
        [tp!("direction", [-1.0, 0.0], "Direction the pictures slide: [-1, 0] left, [0, 1] up."), tp!("stretch", [0.6], 0.0, 1.0, "How long the outgoing picture stretches."), tp!("bounce", [0.6], 0.0, 1.0, "How far the next picture overshoots.")]),
    tr!("squeeze", "Squeeze", "gre", 0.5, "The outgoing picture is squeezed into a horizontal line with colour separation, revealing the next.",
        [tp!("color_separation", [4.0], 0.0, 20.0, "Vertical red/blue separation at the end, % of height.")]),
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

    #[test]
    fn the_transitions_are_consistent() {
        let mut kinds: Vec<&str> = TRANSITIONS.iter().map(|t| t.kind).collect();
        let n = kinds.len();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), n, "transition kinds are unique");
        for t in TRANSITIONS {
            assert!(!t.doc.is_empty() && t.duration > 0.0 && t.credit.contains("licence"), "{}", t.kind);
            assert!(matches!(t.source, SourceKind::None | SourceKind::OptionalSlot), "{}: a transition's source is a map slot or nothing", t.kind);
            for p in t.params {
                if let (Some(lo), Some(hi)) = (p.min, p.max) {
                    assert!(lo <= hi && p.default.iter().all(|v| (lo..=hi).contains(v)), "{}.{}: the default is outside its range", t.kind, p.name);
                }
                assert!(!p.color || (3..=4).contains(&p.default.len()), "{}.{}: a colour is RGB or RGBA", t.kind, p.name);
            }
        }
        assert_eq!(transition("luma_wipe").map(|t| t.source), Some(SourceKind::OptionalSlot));
    }
}
