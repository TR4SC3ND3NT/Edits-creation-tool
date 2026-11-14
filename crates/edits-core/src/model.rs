//! The project document: what an agent or a person edits, and what the renderer reads.
//!
//! Conventions
//! - Times (`in`, `out`, key `t`, `duration`) are in the project's `time_base`:
//!   beats when `settings.bpm` is set and `time_base` is `beats`, otherwise seconds.
//! - Space: units of composition height. Origin at the centre of the frame,
//!   x to the right, y down, z away from the viewer. A layer at z = 0 with
//!   scale 1 and fit `cover` exactly fills the frame.
//! - Rotations are degrees, applied in X, then Y, then Z order.
//! - Spatial effect parameters (blur radius, split amount) are percent of the
//!   composition height, so a project renders the same at any resolution.
//! - Layers are listed bottom to top.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

pub const FORMAT: &str = "edits.project/1";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub format: String,
    pub settings: Settings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<Audio>,
    /// Sound effects mixed over the song at render (whooshes, impacts, risers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_clips: Vec<AudioClip>,
    /// Named media placeholders. The recipe refers to slots; content is swapped
    /// by changing a slot's `path`.
    #[serde(default)]
    pub slots: BTreeMap<String, Slot>,
    #[serde(default)]
    pub camera: Camera,
    pub layers: Vec<Layer>,
    /// Named compositions that `comp` layers draw (see `crate::comp`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub comps: BTreeMap<String, crate::comp::Comp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Length of the project in `time_base` units.
    pub duration: f64,
    #[serde(default)]
    pub time_base: TimeBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    /// Seconds from the start of the project to beat 0.
    #[serde(default)]
    pub beat_offset: f64,
    #[serde(default)]
    pub background: [f32; 3],
    #[serde(default)]
    pub motion_blur: MotionBlur,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimeBase {
    #[default]
    Seconds,
    Beats,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct MotionBlur {
    /// Sub-frame samples accumulated per output frame. 1 disables motion blur.
    #[serde(default = "one_u32")]
    pub samples: u32,
    /// Shutter as a fraction of the frame interval (0.5 = 180 degrees).
    #[serde(default = "half")]
    pub shutter: f64,
}
impl Default for MotionBlur {
    fn default() -> Self {
        Self { samples: 1, shutter: 0.5 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Audio {
    pub path: String,
    /// Seconds into the audio file where project time 0 begins.
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "one")]
    pub gain: f64,
}

/// A sound effect: part of an audio file, played once from project time
/// `at`, mixed over the song. Its own times (`trim_*`, `fade_*`) are seconds
/// of the source, not project time, so a clip sounds the same at any tempo.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AudioClip {
    pub id: String,
    /// Audio (or video with sound) file, relative to the project.
    pub path: String,
    /// When the clip starts, in project time.
    pub at: f64,
    /// Seconds into the file where the clip starts.
    #[serde(default)]
    pub trim_in: f64,
    /// Seconds into the file where the clip stops. `None` = end of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim_out: Option<f64>,
    /// Linear gain: 1 keeps the file's level, 0.5 is about -6 dB, 0 mutes.
    #[serde(default = "one")]
    pub volume: f64,
    /// Seconds of fade from silence at the start of the clip.
    #[serde(default)]
    pub fade_in: f64,
    /// Seconds of fade to silence at the end of the clip.
    #[serde(default)]
    pub fade_out: f64,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Slot {
    /// Image file. `None` means the slot is empty and renders as a placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub fit: Fit,
    /// Point of interest in the image (0..1, 0..1) kept centred by `cover` fitting.
    #[serde(default = "centre2")]
    pub focus: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// For a video: the part of the source that plays, `[in, out]` in
    /// seconds of the file. The rest of the file is ignored, as if it had been
    /// cut out. It belongs to the content, so it swaps with the slot's `path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<[f64; 2]>,
}

impl Slot {
    /// The source frames `[first, first + count)` that `trim` keeps out of
    /// `frames` frames at `fps`: the frames that start at or after `in` and
    /// before `out`. At least one frame when the file has any.
    pub fn trim_frames(&self, fps: f64, frames: u64) -> (u64, u64) {
        if frames == 0 {
            return (0, 0);
        }
        let Some([a, b]) = self.trim.filter(|_| fps.is_finite() && fps > 0.0) else { return (0, frames) };
        let first = ((a.max(0.0) * fps - 1e-6).ceil().max(0.0) as u64).min(frames - 1);
        let end = if b.is_finite() { ((b * fps - 1e-6).ceil().max(0.0) as u64).min(frames) } else { frames };
        (first, end.saturating_sub(first).max(1))
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Fill the layer box, cropping the overflow.
    #[default]
    Cover,
    /// Fit inside the layer box, leaving bars.
    Contain,
    /// Ignore the image's aspect ratio.
    Stretch,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Camera {
    /// Vertical field of view in degrees. The camera distance is derived so that
    /// the plane z = 0 always maps exactly onto the frame.
    #[serde(default = "default_fov")]
    pub fov: Prop,
    /// Offset of the camera from its rest position, in composition-height units.
    #[serde(default = "zero3")]
    pub position: Prop,
    /// Camera rotation in degrees (x, y, z).
    #[serde(default = "zero3")]
    pub rotation: Prop,
}
impl Default for Camera {
    fn default() -> Self {
        Self { fov: default_fov(), position: zero3(), rotation: zero3() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Layer {
    pub id: String,
    #[serde(flatten)]
    pub kind: LayerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// First instant at which the layer is visible.
    #[serde(rename = "in", default)]
    pub start: f64,
    /// Instant at which the layer stops being visible. `None` = end of project.
    #[serde(rename = "out", default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    #[serde(default)]
    pub transform: Transform,
    #[serde(default)]
    pub blend: Blend,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// How the layer comes in: instead of cutting at `in`, it replaces the
    /// picture beneath it through a transition over `duration`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    /// Posterize time: the layer's transform, effects and video frame are
    /// evaluated only `fps` times a second (counted from `in`), which gives
    /// the choppy "on twos" look. `None` follows the project's frame rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,
    /// Shapes in layer space that limit where the layer shows, combined in
    /// order (see `crate::mask`). On an adjustment layer they limit where
    /// its effects apply.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<crate::mask::Mask>,
    /// A track matte: another layer's alpha or luminance at the same instant
    /// multiplies this layer's alpha.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matte: Option<crate::mask::Matte>,
    /// `false`: the layer is not drawn, but still works as another layer's matte.
    #[serde(default = "yes", skip_serializing_if = "crate::mask::is_true")]
    pub visible: bool,
}

/// A transition into a layer. From `in` to `in + duration`, the picture is
/// `kind` applied to what is beneath the layer ("from") and what the frame
/// looks like with the layer ("to"), at a progress going from 0 to 1. The
/// outgoing layer has to stay visible until the end of the transition.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transition {
    /// One of `crate::effects::TRANSITIONS`.
    pub kind: String,
    /// Length in the project's time base.
    pub duration: f64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Prop>,
    /// Easing of the progress.
    #[serde(default)]
    pub ease: Ease,
    /// For a transition that reads a map (`luma_wipe`): `slot:<name>`, a
    /// picture slot of the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayerKind {
    /// A flat image from a slot.
    Image { slot: String },
    /// A flat colour. `color` is straight-alpha sRGB.
    Solid { color: [f32; 4] },
    /// A box with a slot on each face. `faces` order: front, right, back, left,
    /// top, bottom; missing faces repeat the last one given. `size` is the box's
    /// width, height and depth in composition-height units.
    Cube {
        faces: Vec<String>,
        #[serde(default = "unit3")]
        size: [f64; 3],
    },
    /// No content: its effects are applied to everything beneath it.
    Adjustment,
    /// A video clip from a slot, played from the slot's `trim` in.
    Video(Video),
    /// Particles (petals, sparkles, dust, bursts), closed form per instant.
    /// See `crate::particles`.
    Particles(Box<crate::particles::Particles>),
    /// Lines of text set in a font file (the bundled Inter by default).
    Text(Text),
    /// A comp of `project.comps` drawn as one flat picture. See `crate::comp`.
    Comp(crate::comp::CompLayer),
    /// Vector shapes (rects, ellipses, stars, paths, lines) with fills,
    /// strokes and operators (trim, repeater, ...). See `crate::shape`.
    Shape(crate::shape::Shapes),
}

/// A `text` layer: what it says and how it is set.
///
/// The text block is centred on the layer's origin: horizontally, the box is
/// `max_width` wide (or as wide as the longest line) and lines are aligned
/// inside it; vertically, the middle of the block runs from the cap height of
/// the first line to the baseline of the last. `transform` moves, scales and
/// turns the block like any other layer. Sizes are percent of the
/// composition height, like effect sizes; `max_width` is in composition units.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Text {
    /// The words. `\n` starts a new line.
    pub text: String,
    /// A font file (TrueType or OpenType, `.ttf` or `.otf`), relative to the
    /// project. `None` is Inter, which is built into the renderer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Font size (the em) in percent of the composition height.
    #[serde(default = "default_text_size")]
    pub size: f64,
    /// Weight on the font's `wght` axis (100 thin .. 400 regular .. 900
    /// black). Only variable fonts have one; others ignore it. `None` is the
    /// font's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Align::is_center")]
    pub align: Align,
    /// Distance from one baseline to the next, in multiples of `size`.
    #[serde(default = "default_line_height")]
    pub line_height: f64,
    /// Extra space after every character, in percent of the composition
    /// height (negative tightens).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tracking: f64,
    /// Colour of the letters: straight-alpha sRGB, `[r, g, b]` or `[r, g, b, a]`.
    #[serde(default = "white", deserialize_with = "rgba")]
    pub fill: [f32; 4],
    /// An outline around the letters, drawn beneath every letter's fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<TextStroke>,
    /// Wrap lines longer than this, in composition units (height = 1), at
    /// spaces and between CJK characters. A word longer than the line stays whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<f64>,
    /// Per-character animation: each animator selects characters, words or
    /// lines and moves, scales, turns, fades, colours, spaces or blurs them.
    /// See `crate::text_select`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animators: Vec<crate::text_select::TextAnimator>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TextStroke {
    /// Straight-alpha sRGB, `[r, g, b]` or `[r, g, b, a]`.
    #[serde(default = "black", deserialize_with = "rgba")]
    pub color: [f32; 4],
    /// How far the outline reaches beyond the edge of the letters, in percent
    /// of the composition height.
    pub width: f64,
}

/// How the lines of a text block line up.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}
impl Align {
    fn is_center(&self) -> bool {
        *self == Align::Center
    }
}

/// A `video` layer: which clip, and how its time runs.
///
/// Source time starts at the layer's `in`. `speed` multiplies it (animated,
/// it is integrated, so a ramp plays on from where the clip is); `time_remap`
/// instead gives the source second to show at each instant and overrides
/// `speed`. Past the end the clip holds its last frame, unless `loop`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Video {
    pub slot: String,
    /// Playback rate: 1 is real time, 0.5 half speed, 2 double. Not negative
    /// (use `reverse`). Keyed in project time like every other value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<Prop>,
    /// Seconds of the (trimmed) source to show, as a function of project time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_remap: Option<Prop>,
    /// Start again from the first frame after the last one.
    #[serde(default, rename = "loop", skip_serializing_if = "is_false")]
    pub looped: bool,
    /// Play the clip backwards: the layer starts on the last frame.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reverse: bool,
    /// What shows between two source frames (slow motion).
    #[serde(default, skip_serializing_if = "FrameBlend::is_none")]
    pub frame_blend: FrameBlend,
    /// Gain of the clip's own sound in the render (0 mutes it). It is mixed
    /// only while the speed is constant and there is no `time_remap`.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub volume: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrameBlend {
    /// The source frame at or before the instant.
    #[default]
    None,
    /// A cross-fade of the two neighbouring source frames, in linear light.
    Mix,
    /// Both neighbouring source frames warped along their optical flow to the
    /// instant between them, then cross-faded (plainly where the flow is not
    /// trusted).
    Flow,
}
impl FrameBlend {
    fn is_none(&self) -> bool {
        *self == FrameBlend::None
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transform {
    #[serde(default = "zero3")]
    pub position: Prop,
    /// A number (uniform) or [x, y] or [x, y, z].
    #[serde(default = "one_prop")]
    pub scale: Prop,
    #[serde(default = "zero3")]
    pub rotation: Prop,
    /// Pivot inside the layer box, in layer-box units from its centre.
    #[serde(default = "zero2")]
    pub anchor: Prop,
    #[serde(default = "one_prop")]
    pub opacity: Prop,
}
impl Default for Transform {
    fn default() -> Self {
        Self { position: zero3(), scale: one_prop(), rotation: zero3(), anchor: zero2(), opacity: one_prop() }
    }
}

/// How a layer combines with the image beneath it. The formulas are in
/// `docs/RENDERING.md` (and `edits-render/src/compose.rs`); they work on
/// linear light and stay sensible for values above 1.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Blend {
    #[default]
    Normal,
    Add,
    Screen,
    Multiply,
    Overlay,
    Lighten,
    Darken,
    Difference,
    // Darkening.
    ColorBurn,
    LinearBurn,
    DarkerColor,
    // Lightening.
    ColorDodge,
    /// The same as `add`.
    LinearDodge,
    LighterColor,
    // Contrast.
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    // Inversion and cancellation.
    Exclusion,
    Subtract,
    Divide,
    // Component (non-separable).
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Blend {
    /// Every mode, `normal` first.
    pub const ALL: [Blend; 27] = [
        Blend::Normal,
        Blend::Add,
        Blend::Screen,
        Blend::Multiply,
        Blend::Overlay,
        Blend::Lighten,
        Blend::Darken,
        Blend::Difference,
        Blend::ColorBurn,
        Blend::LinearBurn,
        Blend::DarkerColor,
        Blend::ColorDodge,
        Blend::LinearDodge,
        Blend::LighterColor,
        Blend::SoftLight,
        Blend::HardLight,
        Blend::VividLight,
        Blend::LinearLight,
        Blend::PinLight,
        Blend::HardMix,
        Blend::Exclusion,
        Blend::Subtract,
        Blend::Divide,
        Blend::Hue,
        Blend::Saturation,
        Blend::Color,
        Blend::Luminosity,
    ];

    /// The name used in project files, e.g. `soft_light`.
    pub fn name(self) -> &'static str {
        match self {
            Blend::Normal => "normal",
            Blend::Add => "add",
            Blend::Screen => "screen",
            Blend::Multiply => "multiply",
            Blend::Overlay => "overlay",
            Blend::Lighten => "lighten",
            Blend::Darken => "darken",
            Blend::Difference => "difference",
            Blend::ColorBurn => "color_burn",
            Blend::LinearBurn => "linear_burn",
            Blend::DarkerColor => "darker_color",
            Blend::ColorDodge => "color_dodge",
            Blend::LinearDodge => "linear_dodge",
            Blend::LighterColor => "lighter_color",
            Blend::SoftLight => "soft_light",
            Blend::HardLight => "hard_light",
            Blend::VividLight => "vivid_light",
            Blend::LinearLight => "linear_light",
            Blend::PinLight => "pin_light",
            Blend::HardMix => "hard_mix",
            Blend::Exclusion => "exclusion",
            Blend::Subtract => "subtract",
            Blend::Divide => "divide",
            Blend::Hue => "hue",
            Blend::Saturation => "saturation",
            Blend::Color => "color",
            Blend::Luminosity => "luminosity",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Effect {
    pub kind: String,
    #[serde(default)]
    pub params: BTreeMap<String, Prop>,
    /// Choices that are not numbers and cannot be animated, such as
    /// `{ "shape": "hex" }`. The registry lists each effect's options and
    /// their values; a missing option takes its first value.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    /// For `kind: "custom"`: path to a WGSL fragment file, relative to the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Marker {
    pub t: f64,
    pub label: String,
}

/// An animatable value: a constant, or keyframes with optional modifiers.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Prop {
    Const(Value),
    Anim(Anim),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Value {
    Num(f64),
    Vec(Vec<f64>),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Anim {
    /// Keyframes in ascending `t`. May be empty when only modifiers are used;
    /// the base value is then `base`.
    #[serde(default)]
    pub keys: Vec<Key>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<Modifier>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Key {
    pub t: f64,
    pub v: Value,
    /// Interpolation from this key to the next one.
    #[serde(default)]
    pub ease: Ease,
}

/// How a segment between two keys is interpolated.
///
/// - a name: `linear`, `hold`, `in`, `out`, `in_out`, `quad_out`, `quint_out`,
///   `expo_in`, `expo_out`, `sine_in_out`, `back_out`, `elastic_out`, `bounce_out`
/// - four numbers: a CSS-style cubic bezier `[x1, y1, x2, y2]`
/// - `{ "name": "decay", "rate": r }`: exponential approach, where `rate` is the
///   fraction of the remaining distance covered per frame-length step
///   (`steps` per segment, default the segment's length in frames is supplied
///   by the evaluator). `{ "name": "back_out", "overshoot": 1.7 }` etc.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Ease {
    Name(String),
    Bezier([f64; 4]),
    Params {
        name: String,
        #[serde(flatten)]
        args: BTreeMap<String, f64>,
    },
}
impl Default for Ease {
    fn default() -> Self {
        Ease::Name("linear".into())
    }
}

/// Procedural changes applied on top of the keyframed value.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Modifier {
    /// Deterministic smooth noise added to the value. `amplitude` may be a
    /// number, a per-component vector, or keyframed (an envelope).
    Shake {
        amplitude: Prop,
        /// Oscillations per second.
        #[serde(default = "default_freq")]
        frequency: f64,
        #[serde(default)]
        seed: u32,
    },
    /// Repeat the keyframed range `[from, from + period)` forever.
    Loop {
        #[serde(default)]
        from: f64,
        period: f64,
    },
    /// A periodic wave added to the value: `amplitude × wave(cycles)`, with
    /// `cycles = frequency × (time since from) + phase`. No randomness.
    Oscillate(Box<Oscillate>),
}

/// The fields of an `oscillate` modifier (see [`Modifier::Oscillate`]).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Oscillate {
    /// Peak offset: a number, a per-component vector, or keyframed (an envelope).
    pub amplitude: Prop,
    /// Cycles per `per` (default 1).
    #[serde(default = "one")]
    pub frequency: f64,
    /// `beat` (default) or `second`: what `frequency` counts cycles per.
    #[serde(default)]
    pub per: OscPer,
    #[serde(default)]
    pub wave: Wave,
    /// Where in the cycle the wave is at `from`, in cycles (0.25 is a quarter period).
    #[serde(default)]
    pub phase: f64,
    /// Project time where the cycles are counted from.
    #[serde(default)]
    pub from: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OscPer {
    #[default]
    Beat,
    Second,
}

/// The shape of one cycle of an `oscillate` modifier, in -1..1 from phase 0.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Wave {
    /// 0, up to 1 at a quarter, 0 at half, -1 at three quarters.
    #[default]
    Sine,
    /// The same turning points as `sine`, joined by straight lines.
    Triangle,
    /// 1 for the first half of the cycle, -1 for the second.
    Square,
    /// Rises straight from -1 to 1 over the cycle, then drops back.
    Saw,
}

impl Wave {
    /// The wave at `cycles` (any real number).
    pub fn at(self, cycles: f64) -> f64 {
        let f = cycles.rem_euclid(1.0);
        match self {
            Wave::Sine => (f * std::f64::consts::TAU).sin(),
            Wave::Triangle => {
                if f < 0.25 {
                    4.0 * f
                } else if f < 0.75 {
                    2.0 - 4.0 * f
                } else {
                    4.0 * f - 4.0
                }
            }
            Wave::Square => {
                if f < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Saw => 2.0 * f - 1.0,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Wave::Sine => "sine",
            Wave::Triangle => "triangle",
            Wave::Square => "square",
            Wave::Saw => "saw",
        }
    }
}

// ---- serde defaults --------------------------------------------------------

fn one() -> f64 {
    1.0
}
fn one_u32() -> u32 {
    1
}
fn half() -> f64 {
    0.5
}
fn yes() -> bool {
    true
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn is_one(v: &f64) -> bool {
    *v == 1.0
}
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}
fn default_text_size() -> f64 {
    10.0
}
fn default_line_height() -> f64 {
    1.2
}
fn white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}
fn black() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}
/// A colour written as `[r, g, b]` (opaque) or `[r, g, b, a]`.
fn rgba<'de, D: Deserializer<'de>>(d: D) -> Result<[f32; 4], D::Error> {
    match Vec::<f32>::deserialize(d)?.as_slice() {
        [r, g, b] => Ok([*r, *g, *b, 1.0]),
        [r, g, b, a] => Ok([*r, *g, *b, *a]),
        other => Err(serde::de::Error::custom(format!("a colour is [r, g, b] or [r, g, b, a] in 0..1, not {} numbers", other.len()))),
    }
}
fn centre2() -> [f64; 2] {
    [0.5, 0.5]
}
fn unit3() -> [f64; 3] {
    [1.0, 1.0, 1.0]
}
fn default_freq() -> f64 {
    12.0
}
fn default_fov() -> Prop {
    Prop::Const(Value::Num(39.6))
}
fn zero3() -> Prop {
    Prop::Const(Value::Vec(vec![0.0, 0.0, 0.0]))
}
fn zero2() -> Prop {
    Prop::Const(Value::Vec(vec![0.0, 0.0]))
}
fn one_prop() -> Prop {
    Prop::Const(Value::Num(1.0))
}

impl Value {
    pub fn to_vec(&self) -> Vec<f64> {
        match self {
            Value::Num(n) => vec![*n],
            Value::Vec(v) => v.clone(),
        }
    }
}
impl From<f64> for Prop {
    fn from(n: f64) -> Self {
        Prop::Const(Value::Num(n))
    }
}
impl From<[f64; 3]> for Prop {
    fn from(v: [f64; 3]) -> Self {
        Prop::Const(Value::Vec(v.to_vec()))
    }
}

impl Project {
    pub fn from_json(text: &str) -> anyhow::Result<Self> {
        let p: Project = serde_json::from_str(text)?;
        Ok(p)
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("project serialises")
    }
    /// Convert a time in project units to seconds from the start of the project.
    pub fn to_seconds(&self, t: f64) -> f64 {
        match (self.settings.time_base, self.settings.bpm) {
            (TimeBase::Beats, Some(bpm)) if bpm > 0.0 => self.settings.beat_offset + t * 60.0 / bpm,
            _ => t,
        }
    }
    /// Convert seconds from the start of the project to project units.
    pub fn from_seconds(&self, s: f64) -> f64 {
        match (self.settings.time_base, self.settings.bpm) {
            (TimeBase::Beats, Some(bpm)) if bpm > 0.0 => (s - self.settings.beat_offset) * bpm / 60.0,
            _ => s,
        }
    }
    /// Length of the project in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.to_seconds(self.settings.duration)
    }
    pub fn frame_count(&self) -> u64 {
        (self.duration_seconds() * self.settings.fps).round().max(1.0) as u64
    }
    /// Length of one frame in project units.
    pub fn frame_len(&self) -> f64 {
        self.from_seconds(1.0 / self.settings.fps) - self.from_seconds(0.0)
    }
    /// Project time at the start of output frame `frame`.
    pub fn frame_time(&self, frame: u64) -> f64 {
        self.from_seconds(frame as f64 / self.settings.fps)
    }
}
