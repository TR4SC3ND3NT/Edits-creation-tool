//! The `reference_analysis.json` document (`edits.reference_analysis/1`).
//!
//! Every numeric *estimate* is an [`Est`]: a value with its unit, a confidence
//! in 0..1 and an identifiability class, so a reader can tell a measurement
//! from a guess. Frame numbers, counts and scores are plain numbers.

use edits_core::Ease;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "edits.reference_analysis/1";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Ident {
    /// Read off the video; trust it up to `confidence`.
    Measured,
    /// Only the change relative to the shot's reference frame is known
    /// (absolute scale of a full-frame picture, a constant grade, ...).
    RelativeOnly,
    /// Cannot be separated from the picture without the source image.
    NeedsSource,
    /// The kind of thing is detected; the number is a starting point.
    Qualitative,
    /// The tool could not measure this; an agent has to look.
    NeedsAgent,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Est {
    pub value: f64,
    pub unit: String,
    pub confidence: f64,
    pub identifiability: Ident,
}

pub fn round_to(v: f64, decimals: i32) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    let m = 10f64.powi(decimals);
    (v * m).round() / m
}

impl Est {
    pub fn new(value: f64, unit: &str, confidence: f64, identifiability: Ident) -> Self {
        Est { value: round_to(value, 5), unit: unit.into(), confidence: round_to(confidence.clamp(0.0, 1.0), 2), identifiability }
    }
    pub fn measured(value: f64, unit: &str, confidence: f64) -> Self {
        Self::new(value, unit, confidence, Ident::Measured)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Analysis {
    pub schema: String,
    pub tool: String,
    pub source: Source,
    pub ingest: Ingest,
    pub beats: Beats,
    pub shots: Vec<Shot>,
    pub slots: Vec<SlotInfo>,
    pub layers: Layers,
    /// Per-frame signature curves over the analysed range, used by `compare`.
    pub signature: Signature,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// What an agent saw in the whole edit, merged by `vision merge`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<GlobalVision>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Source {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frames: usize,
    pub duration_seconds: f64,
    pub has_audio: bool,
    pub analysis_width: usize,
    pub analysis_height: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlankRun {
    /// First frame of the run.
    pub f0: usize,
    /// One past the last frame.
    pub f1: usize,
    /// `black`, `white`, `solid` or `fade`.
    pub kind: String,
    /// Mean luminance (0..255) of the first and last frame of the run.
    pub lum_start: f64,
    pub lum_end: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Duplicates {
    /// Frames that repeat their predecessor while their neighbours move.
    pub frames: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_frames: Option<Est>,
    /// If the stalls are periodic, the frame rate the edit was probably authored at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fps_guess: Option<Est>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LoopInfo {
    pub first_frame: usize,
    pub period_frames: Est,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_beats: Option<Est>,
    /// How many times the period plays (may be fractional).
    pub repeats: f64,
    /// Mean frame-descriptor correlation at the period lag, 0..1.
    pub score: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Ingest {
    /// First frame with picture content and one past the last.
    pub content_f0: usize,
    pub content_f1: usize,
    pub blank_runs: Vec<BlankRun>,
    pub duplicates: Duplicates,
    #[serde(rename = "loop", default, skip_serializing_if = "Option::is_none")]
    pub loop_info: Option<LoopInfo>,
    /// The frame range the per-shot stages analysed (one loop period when the edit loops).
    pub analysed_f0: usize,
    pub analysed_f1: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TempoCandidate {
    pub bpm: f64,
    pub score: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Beats {
    /// `audio`, `visual` (no usable audio), `override` or `none`.
    pub source: String,
    pub bpm: Est,
    /// Tempo before snapping to a whole number.
    pub bpm_raw: f64,
    /// Seconds from the start of the video to beat 0.
    pub offset_seconds: Est,
    /// The audio-only phase estimate for beat 0, before visual refinement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_offset_seconds: Option<f64>,
    /// Shift (frames) applied to the audio phase so that cuts fall on the grid.
    pub refine_shift_frames: f64,
    pub frames_per_beat: f64,
    pub candidates: Vec<TempoCandidate>,
}

impl Beats {
    pub fn period(&self) -> f64 {
        60.0 / self.bpm.value.max(1e-6)
    }
    /// Beat position of the start of `frame`.
    pub fn beat_of_frame(&self, frame: f64, fps: f64) -> f64 {
        (frame / fps - self.offset_seconds.value) / self.period()
    }
    pub fn frame_of_beat(&self, beat: f64, fps: f64) -> f64 {
        (self.offset_seconds.value + beat * self.period()) * fps
    }
    /// Nearest quarter beat if the frame on which it would be shown is at most
    /// one frame away from `frame`.
    pub fn snap(&self, frame: f64, fps: f64) -> Option<f64> {
        let b = self.beat_of_frame(frame, fps);
        let q = (b * 4.0).round() / 4.0;
        let shown = (self.frame_of_beat(q, fps) - 1e-6).ceil();
        if (shown - frame).abs() <= 1.0 + 1e-9 {
            Some(q)
        } else {
            None
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CutInfo {
    /// Frames between the cut and the nearest grid position (positive = late).
    pub offset_frames: Est,
    /// `beat`, `half_beat`, `quarter_beat` or `off_grid`.
    pub grid: String,
    /// 1 - similarity across the cut (0..1+).
    pub strength: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TrackSummary {
    /// `similarity` (scale, rotation, translation).
    pub model: String,
    /// Share of the shot's frames registered to the reference frame.
    pub valid_fraction: f64,
    pub mean_confidence: f64,
    pub direct: usize,
    pub chained: usize,
    pub bridged: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EaseAlt {
    pub ease: Ease,
    pub rms: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CurveKey {
    pub frame: usize,
    pub beat: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_snapped: Option<f64>,
    pub v: Vec<f64>,
    /// Easing of the segment that starts at this key.
    pub ease: Ease,
    /// RMS residual of that segment, in the curve's unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_up: Option<EaseAlt>,
}

/// A fitted keyframe curve for one property of a shot.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Curve {
    /// `scale`, `position`, `rotation`, or `<effect>.<param>` such as `exposure.stops`.
    pub prop: String,
    pub unit: String,
    pub confidence: f64,
    pub identifiability: Ident,
    pub keys: Vec<CurveKey>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Event {
    /// `flash`, `saturation`, `invert`, `threshold`, `posterize`, `fade`.
    pub kind: String,
    pub f0: usize,
    pub f1: usize,
    /// Frame of the peak (flash) or of the start (hits).
    pub frame: usize,
    pub beat: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_snapped: Option<f64>,
    pub params: BTreeMap<String, Est>,
    /// Colours the event is made of, sRGB 0..1 (`duotone`: `shadows`, `highlights`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, [f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Shake {
    pub rms_x: Est,
    pub rms_y: Est,
    /// RMS in the second half of the shot divided by the first half.
    pub late_over_early: Est,
}

/// Raw per-frame samples of a shot (only with `--samples`).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Samples {
    pub scale: Vec<f64>,
    pub rotation: Vec<f64>,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub confidence: Vec<f64>,
    /// Normalised correlation of the registration (best attempt for bridged frames).
    pub zncc: Vec<f64>,
    /// Scale-factor ratio - 1 of a free affine registration, and its gain in
    /// correlation over the similarity (both large = 3D tilt or stretch).
    pub anisotropy: Vec<f64>,
    pub affine_gain: Vec<f64>,
    /// `d` direct, `c` chained, `b` bridged, `r` reference frame.
    pub method: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Shot {
    pub id: usize,
    pub f0: usize,
    pub f1: usize,
    pub beat0: f64,
    pub beat1: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat0_snapped: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat1_snapped: Option<f64>,
    pub cut: CutInfo,
    pub ref_frame: usize,
    pub slot: String,
    /// `measured` when the transform track is usable, otherwise `needs_agent`.
    pub status: Ident,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub track: TrackSummary,
    pub curves: Vec<Curve>,
    pub events: Vec<Event>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shake: Option<Shake>,
    /// Kind of the blur (gaussian, directional, zoom, spin) from gradient-orientation statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blur_kind: Option<BlurKind>,
    /// For a shot that could not be tracked: where a card, cube or inset sits on its reference frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<CardBox>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<Samples>,
    /// What an agent saw in the shot's frames, merged by `vision merge`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<ShotVision>,
}

/// The box a card, cube or inset picture fills on a dark background.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CardBox {
    /// Centre, frame heights from the frame centre (x right, y down).
    pub center: [f64; 2],
    /// Width and height, frame heights.
    pub size: [f64; 2],
    /// Gamma-encoded luminance (0..1) of the background around it.
    pub background: f64,
}

/// The kind of a shot's blur, told apart by which gradient orientations it
/// removes: all alike (gaussian), one direction (directional), the radial
/// direction (zoom) or the tangential one (spin).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlurKind {
    /// `gaussian`, `directional`, `zoom` or `spin`.
    pub kind: String,
    pub confidence: f64,
    /// Directional: the direction of the streaks, degrees (0 = horizontal, clockwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    /// Spread, in log energy (frame / reference), between the most and the
    /// least blurred of 8 fixed gradient orientations.
    pub axis_contrast: f64,
    /// Log energy ratio of radial minus tangential gradients (negative: zoom; positive: spin).
    pub radial_tangential: f64,
    /// Blurred frames the decision rests on.
    pub frames: usize,
    /// The kind's own parameter at the most blurred frame, found by blurring
    /// the reference frame until it loses as much detail
    /// (`directional_blur.length` % of height, `zoom_blur.strength`, `spin_blur.angle` degrees).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak: Option<f64>,
    /// That parameter per % of height of the Gaussian-equivalent radius the
    /// blur curve records at the same frame (draft scales the curve by it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_sigma: Option<f64>,
}

/// What an agent saw in one shot, answering the fixed questions Q1-Q8 of
/// `vision::QUESTIONS` (`vision merge` validates and writes it). Labels are
/// words of the fixed vocabulary; a few take an argument after a colon
/// (`slide:left`, `3d_turn:y`, `directional:30`); colours given as
/// arguments (`glow:red`, `duotone:navy/yellow`, `tinted:green`) are moved
/// to `colors` as sRGB 0..1.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ShotVision {
    /// Q1: panel_bw, panel_colour, full_art, close_up, detail_crop, emblem, text_card, solid.
    pub picture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    /// Q2: full_frame, inset_card, tilted_card, cube, split, picture_in_picture.
    pub framing: String,
    /// Q2: none, thin_light, thin_dark, glow.
    pub border: String,
    /// Q3: normal, inverted, threshold, posterized, duotone, tinted, desaturated, burnt_white.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tone: Vec<String>,
    /// Q4: white_flash, black_flash, glow, light_rays, flare.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub light: Vec<String>,
    /// Q5: static, punch_in, zoom_out, push_in, shake, slide:<dir>, spin, 3d_turn:<axis>, whip:<dir>.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub motion: Vec<String>,
    /// Q6: none, gaussian, directional[:<angle>], zoom, spin.
    pub blur: String,
    /// Q7: paper, handwriting, grain, scratches, speed_lines, focus_lines, screentone, particles, caption, sfx_in_art.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<String>,
    /// Q8: hard_cut, flash_cut, dissolve, whip, zoom_through, cube, push, glitch.
    pub entry: String,
    /// Point of the picture the shot keeps in view, 0..1 from the top left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<[f64; 2]>,
    /// `glow`, `shadows`, `highlights`, `tint`: sRGB 0..1.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, [f64; 3]>,
    /// Evidence: label -> the tile numbers (1-based) of the strip that show it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tiles: BTreeMap<String, Vec<usize>>,
    /// The same evidence as frame numbers (resolved by `vision merge`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub frames: BTreeMap<String, Vec<usize>>,
    /// How the labels compare with the signals, written by `vision merge`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// What an agent saw across the whole edit (`vision merge`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct GlobalVision {
    /// Q7 for the static layer over every shot: paper, handwriting, grain, scratches, screentone, ...
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<String>,
    /// How the static overlay sits on the pictures: lighter, darker or both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_tone: Option<String>,
    /// none, light, medium, strong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignette: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SlotInfo {
    pub id: String,
    pub shots: Vec<usize>,
    /// Frame exported as the proxy still.
    pub ref_frame: usize,
    /// Path of the proxy still relative to the analysis directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    /// What kind of picture the slot holds (`edits_core::SLOT_ROLES`), from the vision labels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OverlayInfo {
    /// Plate image relative to the analysis directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Share of the frame covered by the plate.
    pub coverage: Est,
    /// Mean plate luminance (0..1) where it is present.
    pub strength: Est,
    pub blend_guess: String,
    pub method: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VignetteInfo {
    /// Relative brightness at radius `i / (n - 1)` of the half-diagonal.
    pub profile: Vec<f64>,
    pub amount: Est,
    pub radius: Est,
    pub softness: Est,
    /// Corner brightness divided by centre brightness.
    pub corner_ratio: Est,
    pub method: String,
}

/// A static texture over the whole edit (paper, grain, halftone, a soft
/// plate): fine detail that stays put while the pictures change.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TextureInfo {
    /// Share of the frames' fine detail that is the same in every shot, with
    /// the share expected by chance taken off (0 = none, 1 = all of it), in
    /// the quarter of the frame where it is highest.
    pub presence: Est,
    /// RMS of the static detail where it is present, in log2 luminance (stops).
    pub strength: Est,
    /// Share of the frame (8 x 8 blocks) where it is present.
    pub coverage: Est,
    /// Plate relative to the analysis directory: mid grey plus the static
    /// detail (blend `soft_light` or `overlay`, neutral at grey).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub blend_guess: String,
    pub method: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Layers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<OverlayInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignette: Option<VignetteInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<TextureInfo>,
}

/// Content-independent per-frame curves; index 0 is frame `f0`.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Signature {
    pub f0: usize,
    /// Mean luminance, 0..255.
    pub lum: Vec<f64>,
    /// Exposure relative to the shot's reference frame, stops.
    pub stops: Vec<f64>,
    /// Gaussian-equivalent blur sigma relative to the reference frame, % of height.
    pub blur: Vec<f64>,
    /// Saturation relative to the reference frame (1 = unchanged).
    pub sat: Vec<f64>,
    /// Picture scale relative to the reference frame; `null` where not tracked.
    pub scale: Vec<Option<f64>>,
}
