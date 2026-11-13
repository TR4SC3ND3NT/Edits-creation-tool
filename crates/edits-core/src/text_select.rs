//! Text animators: per-character animation of `text` layers.
//!
//! An animator has a selector, which gives every unit of the text
//! (character, word or line) a selection value, and animatable properties,
//! which the selection scales: a glyph selected at 1 gets the whole of the
//! animator's `position` offset, `rotation`, `tracking` and `blur`, its
//! `scale` and `opacity` become the animator's, and its colour becomes the
//! animator's `fill`; at 0 it is untouched; in between it is part way.
//!
//! Two selectors:
//! - `range`: the units between `start` and `end` (percent of the units,
//!   moved by `offset`), with a `shape` (square, ramps, triangle, round,
//!   smooth), `ease_high` / `ease_low`, an `amount`, an optional `ease` of
//!   each unit's transition, and a seeded random order. The semantics are
//!   those of the After Effects range selector, written after Skottie's
//!   `RangeSelector.cpp` (BSD-3-Clause) and EffectCraft's
//!   `crates/text/src/selectors.rs` (MIT OR Apache-2.0); no code was copied.
//! - `wiggly`: smooth seeded noise per unit in `-amount..amount`, with a
//!   `frequency` and a `correlation` between units.
//!
//! Everything here is a pure function of its arguments: the same text, time
//! and seed give the same selection.

use crate::ease;
use crate::model::{Ease, Prop};
use serde::{Deserialize, Serialize};

/// One animator of a text layer.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct TextAnimator {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Which characters it changes, and by how much.
    #[serde(default)]
    pub selector: Selector,
    /// Offset `[x, y, z]` in composition units at full selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Prop>,
    /// Scale of the glyph at full selection (a number, `[x, y]` or `[x, y, z]`; 1 leaves it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Prop>,
    /// Turn `[x, y, z]` in degrees about the middle of the glyph, at full selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Prop>,
    /// Opacity of the glyph at full selection (0..1; 1 leaves it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Prop>,
    /// Colour of the letters at full selection: straight sRGB `[r, g, b]` or `[r, g, b, a]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Prop>,
    /// Extra space after the glyph, in percent of the composition height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking: Option<Prop>,
    /// Gaussian blur of the glyph (sigma), in percent of the composition height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blur: Option<Prop>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

impl TextAnimator {
    /// The names of the properties it sets.
    pub fn properties(&self) -> Vec<&'static str> {
        let all = [
            ("position", &self.position),
            ("scale", &self.scale),
            ("rotation", &self.rotation),
            ("opacity", &self.opacity),
            ("fill", &self.fill),
            ("tracking", &self.tracking),
            ("blur", &self.blur),
        ];
        all.iter().filter(|(_, p)| p.is_some()).map(|(n, _)| *n).collect()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Selector {
    Range(Box<RangeSelector>),
    Wiggly(Box<WigglySelector>),
}
impl Default for Selector {
    fn default() -> Self {
        Selector::Range(Box::default())
    }
}
impl Selector {
    pub fn units(&self) -> Units {
        match self {
            Selector::Range(r) => r.units,
            Selector::Wiggly(w) => w.units,
        }
    }
}

/// Selects the units from `start` to `end` (percent), moved by `offset`.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct RangeSelector {
    /// Percent of the units where the range starts (default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<Prop>,
    /// Percent of the units where the range ends (default 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<Prop>,
    /// Percent added to both `start` and `end` (default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Prop>,
    #[serde(default, skip_serializing_if = "Units::is_default")]
    pub units: Units,
    #[serde(default, skip_serializing_if = "Shape::is_default")]
    pub shape: Shape,
    /// `square` only: width of the soft edge in percent of a unit (default
    /// 100: a unit half inside the range is selected at 0.5; 0: hard steps).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smoothness: Option<Prop>,
    /// -100..100: positive eases into full selection, negative makes it abrupt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease_high: Option<Prop>,
    /// -100..100: positive eases out of no selection, negative makes it abrupt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease_low: Option<Prop>,
    /// Percent, -100..100, scales every selection value (default 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<Prop>,
    /// The curve each unit follows from selected (1) to not selected (0),
    /// any ease name: `back_out` overshoots as a letter settles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease: Option<Ease>,
    /// Shuffle the order of the units (by `seed`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub randomize: bool,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub seed: u32,
}

/// Smooth seeded noise per unit.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct WigglySelector {
    #[serde(default, skip_serializing_if = "Units::is_default")]
    pub units: Units,
    /// Percent: the selection stays within `-amount..amount` (default 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<Prop>,
    /// Wiggles per second (default 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency: Option<Prop>,
    /// Percent: 100 moves every unit together, 0 each on its own (default 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<Prop>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub seed: u32,
}

/// What a selector counts.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    /// Every character of the text except line breaks (spaces count).
    #[default]
    Characters,
    /// Runs of characters between spaces; a space belongs to the word before it.
    Words,
    /// Lines as laid out (after wrapping at `max_width`).
    Lines,
}
impl Units {
    fn is_default(&self) -> bool {
        *self == Units::Characters
    }
    pub fn name(self) -> &'static str {
        match self {
            Units::Characters => "characters",
            Units::Words => "words",
            Units::Lines => "lines",
        }
    }
}

/// How selection rises and falls across the range.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// 1 inside the range, 0 outside (soft edges by `smoothness`).
    #[default]
    Square,
    /// 0 before the range, rising to 1 across it, 1 after.
    RampUp,
    /// 1 before the range, falling to 0 across it, 0 after.
    RampDown,
    /// 0 at both ends of the range, 1 in its middle, straight sides.
    Triangle,
    /// Like `triangle`, a half circle.
    Round,
    /// Like `triangle`, smoothstepped.
    Smooth,
}
impl Shape {
    fn is_default(&self) -> bool {
        *self == Shape::Square
    }
    pub fn name(self) -> &'static str {
        match self {
            Shape::Square => "square",
            Shape::RampUp => "ramp_up",
            Shape::RampDown => "ramp_down",
            Shape::Triangle => "triangle",
            Shape::Round => "round",
            Shape::Smooth => "smooth",
        }
    }
}

fn yes() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

// ---- units ------------------------------------------------------------------

/// What a selector needs to know about a glyph: the character of the text
/// it comes from (counted in chars, line breaks included) and its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphRef {
    pub char_index: usize,
    pub line: usize,
}

/// The unit of every glyph and the number of units, for `units`.
pub fn units_of(text: &str, glyphs: &[GlyphRef], units: Units) -> (Vec<usize>, usize) {
    match units {
        Units::Lines => {
            let n = glyphs.iter().map(|g| g.line + 1).max().unwrap_or(1);
            (glyphs.iter().map(|g| g.line).collect(), n)
        }
        Units::Characters | Units::Words => {
            let mut of_char = Vec::new();
            let mut count = 0usize;
            let mut in_word = false;
            for c in text.chars() {
                let newline = c == '\n' || c == '\r';
                match units {
                    Units::Characters => {
                        of_char.push(count);
                        if !newline {
                            count += 1;
                        }
                    }
                    _ => {
                        if c.is_whitespace() {
                            in_word = false;
                            // A space belongs to the word before it.
                            of_char.push(count.saturating_sub(1));
                        } else {
                            if !in_word {
                                count += 1;
                                in_word = true;
                            }
                            of_char.push(count - 1);
                        }
                    }
                }
            }
            let n = count.max(1);
            (glyphs.iter().map(|g| of_char.get(g.char_index).copied().unwrap_or(0).min(n - 1)).collect(), n)
        }
    }
}

// ---- range ------------------------------------------------------------------

/// A range selector's values at one instant, in percent.
#[derive(Clone, Copy, Debug)]
pub struct RangeAt {
    pub start: f64,
    pub end: f64,
    pub offset: f64,
    pub smoothness: f64,
    pub ease_high: f64,
    pub ease_low: f64,
    pub amount: f64,
}
impl Default for RangeAt {
    fn default() -> Self {
        RangeAt { start: 0.0, end: 100.0, offset: 0.0, smoothness: 100.0, ease_high: 0.0, ease_low: 0.0, amount: 100.0 }
    }
}

/// The shape's value (0..1) for the unit at position `pos` (0..n) of `n`,
/// before easing and amount. Each unit is measured at its middle.
pub fn shape_value(shape: Shape, pos: usize, n: usize, r: &RangeAt) -> f64 {
    let n = n.max(1) as f64;
    let (mut s, mut e) = ((r.start + r.offset) / 100.0 * n, (r.end + r.offset) / 100.0 * n);
    if e < s {
        std::mem::swap(&mut s, &mut e);
    }
    let c = pos as f64 + 0.5;
    let clamp = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let ramp = || if e - s < 1e-9 { f64::from(c >= s) } else { clamp((c - s) / (e - s)) };
    match shape {
        Shape::Square => {
            let w = r.smoothness / 100.0;
            if w.is_nan() || w <= 1e-9 {
                return f64::from(c >= s && c < e);
            }
            // The part of a unit-wide window (`w` wide) that lies in the range.
            (clamp((c - s) / w + 0.5) + clamp((e - c) / w + 0.5) - 1.0).max(0.0)
        }
        Shape::RampUp => ramp(),
        Shape::RampDown => 1.0 - ramp(),
        Shape::Triangle | Shape::Round | Shape::Smooth => {
            if e - s < 1e-9 {
                return 0.0;
            }
            let u = (c - s) / (e - s);
            if !(0.0..=1.0).contains(&u) {
                return 0.0;
            }
            let x = 2.0 * u - 1.0;
            let tri = 1.0 - x.abs();
            match shape {
                Shape::Triangle => tri,
                Shape::Round => (1.0 - x * x).max(0.0).sqrt(),
                _ => tri * tri * (3.0 - 2.0 * tri),
            }
        }
    }
}

/// `ease_high` and `ease_low` (percent, -100..100) applied to a selection
/// value in 0..1: a cubic from (0, 0) to (1, 1) whose first handle is
/// `(low, 0)` (or `(0, -low)` when negative) and second `(1 - high, 1)`
/// (or `(1, 1 + high)`).
pub fn ease_high_low(v: f64, high: f64, low: f64) -> f64 {
    let (hi, lo) = ((high / 100.0).clamp(-1.0, 1.0), (low / 100.0).clamp(-1.0, 1.0));
    if hi.abs() < 1e-9 && lo.abs() < 1e-9 {
        return v;
    }
    let c0 = if lo >= 0.0 { [lo, 0.0] } else { [0.0, -lo] };
    let c1 = if hi >= 0.0 { [1.0 - hi, 1.0] } else { [1.0, 1.0 + hi] };
    ease::apply(&Ease::Bezier([c0[0], c0[1], c1[0], c1[1]]), v, 1.0)
}

/// The full range selection (`-1..1` with a negative amount; may overshoot
/// with an `ease` such as `back_out`) of the unit at `pos` of `n`.
pub fn range_value(sel: &RangeSelector, pos: usize, n: usize, r: &RangeAt) -> f64 {
    let mut v = shape_value(sel.shape, pos, n, r);
    v = ease_high_low(v, r.ease_high, r.ease_low);
    if let Some(e) = &sel.ease {
        v = 1.0 - ease::apply(e, 1.0 - v.clamp(0.0, 1.0), 1.0);
    }
    v * (r.amount / 100.0).clamp(-1.0, 1.0)
}

/// A seeded shuffle of `0..n`: `order[unit]` is the position the unit takes.
pub fn random_order(n: usize, seed: u32) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut state = (seed as u64) ^ 0x005E_ED7E_47A1_1CE5_u64;
    let mut next = || {
        // SplitMix64.
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    for i in (1..n).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order
}

// ---- wiggly -----------------------------------------------------------------

/// A wiggly selector's values at one instant.
#[derive(Clone, Copy, Debug)]
pub struct WigglyAt {
    /// Percent.
    pub amount: f64,
    /// Wiggles per second.
    pub frequency: f64,
    /// Percent.
    pub correlation: f64,
}
impl Default for WigglyAt {
    fn default() -> Self {
        WigglyAt { amount: 100.0, frequency: 2.0, correlation: 50.0 }
    }
}

/// The wiggly selection of `unit` at `seconds`: a mix of a track shared by
/// every unit and one of the unit's own, in `-amount..amount` (/ 100).
pub fn wiggly_value(unit: usize, seconds: f64, seed: u32, w: &WigglyAt) -> f64 {
    let x = seconds * w.frequency;
    if !x.is_finite() {
        return 0.0;
    }
    let corr = (w.correlation / 100.0).clamp(0.0, 1.0);
    let shared = crate::eval::noise(x, seed, 0x7F00_0000);
    let own = crate::eval::noise(x, seed, unit as u32);
    (corr * shared + (1.0 - corr) * own).clamp(-1.0, 1.0) * (w.amount / 100.0).clamp(-1.0, 1.0)
}

// ---- what a glyph becomes -----------------------------------------------------

/// The animators' result for one glyph.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphState {
    /// Offset in composition units.
    pub offset: [f64; 3],
    pub scale: [f64; 3],
    /// Degrees, X then Y then Z, about the middle of the glyph.
    pub rotation: [f64; 3],
    /// 0..1, multiplies the layer's.
    pub opacity: f64,
    /// Straight sRGB.
    pub fill: [f32; 4],
    /// Extra advance after the glyph, percent of the composition height.
    pub tracking: f64,
    /// Blur sigma, percent of the composition height.
    pub blur: f64,
}

impl GlyphState {
    pub fn rest(fill: [f32; 4]) -> Self {
        GlyphState { offset: [0.0; 3], scale: [1.0; 3], rotation: [0.0; 3], opacity: 1.0, fill, tracking: 0.0, blur: 0.0 }
    }
    /// Moved at most: no scale, turn or blur.
    pub fn is_translation(&self) -> bool {
        self.scale == [1.0; 3] && self.rotation == [0.0; 3] && self.blur == 0.0
    }
}

/// One animator's properties at one instant.
#[derive(Clone, Debug, Default)]
pub struct AnimatorAt {
    pub position: Option<[f64; 3]>,
    pub scale: Option<[f64; 3]>,
    pub rotation: Option<[f64; 3]>,
    pub opacity: Option<f64>,
    pub fill: Option<[f64; 4]>,
    pub tracking: Option<f64>,
    pub blur: Option<f64>,
}

/// Apply an animator selected at `s` to a glyph: offsets, turns, tracking
/// and blur add; scale and opacity multiply; the fill mixes toward the
/// animator's (by `s` clamped to 0..1).
pub fn apply(g: &mut GlyphState, a: &AnimatorAt, s: f64) {
    if s == 0.0 || !s.is_finite() {
        return;
    }
    if let Some(p) = a.position {
        (0..3).for_each(|i| g.offset[i] += s * p[i]);
    }
    if let Some(r) = a.rotation {
        (0..3).for_each(|i| g.rotation[i] += s * r[i]);
    }
    if let Some(k) = a.scale {
        (0..3).for_each(|i| g.scale[i] *= 1.0 + s * (k[i] - 1.0));
    }
    if let Some(o) = a.opacity {
        g.opacity = (g.opacity * (1.0 + s * (o - 1.0))).clamp(0.0, 1.0);
    }
    if let Some(c) = a.fill {
        let m = s.clamp(0.0, 1.0) as f32;
        (0..4).for_each(|i| g.fill[i] += (c[i] as f32 - g.fill[i]) * m);
    }
    if let Some(t) = a.tracking {
        g.tracking += s * t;
    }
    if let Some(b) = a.blur {
        g.blur = (g.blur + s * b).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: f64, end: f64, offset: f64) -> RangeAt {
        RangeAt { start, end, offset, ..Default::default() }
    }

    #[test]
    fn ramps_rise_across_the_range() {
        // Ten units; the range 20..60 % covers units 2..6 (their middles 2.5..5.5).
        let r = at(20.0, 60.0, 0.0);
        let up: Vec<f64> = (0..10).map(|i| shape_value(Shape::RampUp, i, 10, &r)).collect();
        assert_eq!(&up[..2], &[0.0, 0.0]);
        for (i, v) in up.iter().enumerate().take(6).skip(2) {
            assert!((v - (i as f64 + 0.5 - 2.0) / 4.0).abs() < 1e-12, "{i}: {v}");
        }
        assert_eq!(&up[6..], &[1.0; 4]);
        let down: Vec<f64> = (0..10).map(|i| shape_value(Shape::RampDown, i, 10, &r)).collect();
        assert!(up.iter().zip(&down).all(|(a, b)| (a + b - 1.0).abs() < 1e-12));
        // Offset moves both ends; start after end is the same range.
        let moved = at(0.0, 40.0, 20.0);
        let swapped = at(60.0, 20.0, 0.0);
        for (i, v) in up.iter().enumerate() {
            assert_eq!(shape_value(Shape::RampUp, i, 10, &moved), *v);
            assert_eq!(shape_value(Shape::RampUp, i, 10, &swapped), *v);
        }
    }

    #[test]
    fn square_is_the_covered_part_of_each_unit() {
        // Four units, 0..50 %: units 0 and 1 in full. 0..37.5 %: unit 1 half in.
        let sq = |s: f64, e: f64, smooth: f64| -> Vec<f64> {
            (0..4).map(|i| shape_value(Shape::Square, i, 4, &RangeAt { start: s, end: e, smoothness: smooth, ..Default::default() })).collect()
        };
        assert_eq!(sq(0.0, 50.0, 100.0), vec![1.0, 1.0, 0.0, 0.0]);
        assert_eq!(sq(0.0, 37.5, 100.0), vec![1.0, 0.5, 0.0, 0.0]);
        // No smoothness: hard steps at the units' middles.
        assert_eq!(sq(0.0, 40.0, 0.0), vec![1.0, 1.0, 0.0, 0.0]);
        assert_eq!(sq(0.0, 30.0, 0.0), vec![1.0, 0.0, 0.0, 0.0]);
        assert_eq!(sq(100.0, 100.0, 0.0), vec![0.0; 4]);
    }

    #[test]
    fn peaked_shapes_are_zero_at_the_ends_and_one_in_the_middle() {
        let r = at(0.0, 100.0, 0.0);
        for shape in [Shape::Triangle, Shape::Round, Shape::Smooth] {
            // An odd count puts a unit's middle at the range's middle.
            let v: Vec<f64> = (0..5).map(|i| shape_value(shape, i, 5, &r)).collect();
            assert!((v[2] - 1.0).abs() < 1e-12, "{shape:?} {v:?}");
            assert!(v[0] > 0.0 && v[0] < v[1] && (v[0] - v[4]).abs() < 1e-12, "{shape:?} {v:?}");
        }
        let tri = shape_value(Shape::Triangle, 0, 5, &r);
        assert!((tri - 0.2).abs() < 1e-12 && shape_value(Shape::Round, 0, 5, &r) > tri && shape_value(Shape::Smooth, 0, 5, &r) < tri);
    }

    #[test]
    fn ease_high_and_low_keep_the_ends_and_bend_the_middle() {
        for (hi, lo) in [(50.0, 0.0), (0.0, 50.0), (-50.0, -50.0), (100.0, 100.0)] {
            assert!(ease_high_low(0.0, hi, lo).abs() < 1e-6 && (ease_high_low(1.0, hi, lo) - 1.0).abs() < 1e-6);
            let mut last = 0.0;
            for i in 1..=20 {
                let v = ease_high_low(i as f64 / 20.0, hi, lo);
                assert!(v >= last - 1e-9, "not monotonic at {hi} {lo}");
                last = v;
            }
        }
        // Easing out of 0 holds low values down; easing into 1 lifts them.
        assert!(ease_high_low(0.25, 0.0, 80.0) < 0.25 && ease_high_low(0.25, 80.0, 0.0) > 0.25);
        assert_eq!(ease_high_low(0.3, 0.0, 0.0), 0.3);
    }

    #[test]
    fn amount_and_ease_scale_the_selection() {
        let sel = RangeSelector { shape: Shape::RampUp, ..Default::default() };
        let r = RangeAt { amount: -50.0, ..at(0.0, 100.0, 0.0) };
        assert!((range_value(&sel, 1, 2, &r) + 0.375).abs() < 1e-12);
        // back_out: a unit just past its transition goes below 0 (overshoots).
        let sel = RangeSelector { shape: Shape::RampUp, ease: Some(Ease::Name("back_out".into())), ..Default::default() };
        let v = range_value(&sel, 0, 10, &at(0.0, 100.0, 0.0));
        assert!(v < 0.0, "{v}");
    }

    #[test]
    fn random_order_is_a_seed_stable_permutation() {
        for n in [0, 1, 2, 7, 40] {
            let mut o = random_order(n, 3);
            assert_eq!(o, random_order(n, 3));
            o.sort_unstable();
            assert_eq!(o, (0..n).collect::<Vec<_>>());
        }
        assert_ne!(random_order(40, 1), random_order(40, 2));
        assert_ne!(random_order(40, 1), (0..40).collect::<Vec<_>>());
    }

    #[test]
    fn wiggly_is_bounded_deterministic_and_correlated() {
        let w = WigglyAt { amount: 60.0, frequency: 5.0, correlation: 0.0 };
        let mut seen = (0.0f64, 0.0f64);
        for k in 0..400 {
            let t = k as f64 * 0.013;
            for u in 0..6 {
                let v = wiggly_value(u, t, 9, &w);
                assert!(v.abs() <= 0.6 + 1e-12, "{v}");
                assert_eq!(v, wiggly_value(u, t, 9, &w));
                seen = (seen.0.min(v), seen.1.max(v));
            }
        }
        assert!(seen.0 < -0.2 && seen.1 > 0.2, "{seen:?}");
        assert_ne!(wiggly_value(0, 1.3, 9, &w), wiggly_value(1, 1.3, 9, &w));
        assert_ne!(wiggly_value(0, 1.3, 9, &w), wiggly_value(0, 1.3, 10, &w));
        // Full correlation: every unit the same.
        let all = WigglyAt { correlation: 100.0, ..w };
        assert_eq!(wiggly_value(0, 1.3, 9, &all), wiggly_value(5, 1.3, 9, &all));
    }

    #[test]
    fn units_group_characters_words_and_lines() {
        let text = "AB CD\nEF";
        let glyphs: Vec<GlyphRef> = [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (6, 1), (7, 1)].iter().map(|&(c, l)| GlyphRef { char_index: c, line: l }).collect();
        let (chars, n) = units_of(text, &glyphs, Units::Characters);
        assert_eq!((chars, n), (vec![0, 1, 2, 3, 4, 5, 6], 7), "the line break is not a character");
        let (words, n) = units_of(text, &glyphs, Units::Words);
        assert_eq!((words, n), (vec![0, 0, 0, 1, 1, 2, 2], 3), "the space goes with the word before it");
        let (lines, n) = units_of(text, &glyphs, Units::Lines);
        assert_eq!((lines, n), (vec![0, 0, 0, 0, 0, 1, 1], 2));
        assert_eq!(units_of("", &[], Units::Words).1, 1);
    }

    #[test]
    fn animators_add_offsets_and_multiply_scale_and_opacity() {
        let mut g = GlyphState::rest([1.0, 1.0, 1.0, 1.0]);
        let a = AnimatorAt { position: Some([0.0, 0.1, 0.0]), scale: Some([0.0; 3]), opacity: Some(0.5), fill: Some([1.0, 0.0, 0.0, 1.0]), ..Default::default() };
        apply(&mut g, &a, 0.5);
        apply(&mut g, &a, 0.5);
        assert!((g.offset[1] - 0.1).abs() < 1e-12);
        assert!((g.scale[0] - 0.25).abs() < 1e-12 && (g.opacity - 0.5625).abs() < 1e-12);
        assert!((g.fill[1] - 0.25).abs() < 1e-6 && g.fill[0] == 1.0);
        let mut h = GlyphState::rest([1.0; 4]);
        apply(&mut h, &a, 0.0);
        assert_eq!(h, GlyphState::rest([1.0; 4]));
    }
}
