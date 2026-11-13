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

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FORMAT: &str = "edits.project/1";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub format: String,
    pub settings: Settings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<Audio>,
    /// Named media placeholders. The recipe refers to slots; content is swapped
    /// by changing a slot's `path`.
    #[serde(default)]
    pub slots: BTreeMap<String, Slot>,
    #[serde(default)]
    pub camera: Camera,
    pub layers: Vec<Layer>,
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Effect {
    pub kind: String,
    #[serde(default)]
    pub params: BTreeMap<String, Prop>,
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
