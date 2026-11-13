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
