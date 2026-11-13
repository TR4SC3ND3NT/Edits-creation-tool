//! Time effects: `echo` and `time_displacement`. They read the input of
//! their place in the chain at other times, by rendering it again:
//!
//! - on a content layer, the layer drawn at the other time (through
//!   [`Renderer::layer_content`]'s machinery: its transform, masks, video
//!   frame, particles, comp picture or text at that time), with the effects
//!   that come before the time effect in its chain at that time;
//! - on an adjustment layer, the stack beneath it at the other time
//!   ([`Renderer::render_stack_over`]), then its effects before this one.
//!
//! So a frame stays a function of the project and the frame number: no
//! picture of an earlier frame is kept. The price is one render per echo or
//! slice; `count` 0 (or a zero `max`) plans no step at all, and a shift of 0
//! reuses the picture already there.
//!
//! The echo operators follow EffectCraft's `time_fx.rs` echo
//! (https://github.com/storytold/effectcraft, MIT OR Apache-2.0): picture i
//! at `t + i * interval` weighs `start * decay^i`, premultiplied colour and
//! alpha alike, combined in order from the current picture outward, without
//! normalising (except `average`). Time displacement follows its
//! `time_displacement` (a map value picks a time per pixel), with the times
//! rounded to `levels` slices instead of to a frame rate.

use crate::fx::Step;
use crate::renderer::{Pipe, Renderer, Tex};
use anyhow::{anyhow, bail, Result};
use edits_core::{Layer, ResolvedEffect};

/// Time effects nested deeper than this are refused (an echo whose input
/// contains another echo, inside an echo of an adjustment layer, ...). The
/// structure of a project already ends every recursion (an echo re-renders
/// only what is beneath it or before it), but each level multiplies the
/// renders by its count.
const MAX_TIME_DEPTH: u32 = 4;
/// Largest `count` of an echo and `levels` of a time displacement.
const MAX_PICTURES: f64 = 16.0;

/// What a time effect does, resolved at the instant being rendered.
pub(crate) enum TimeOp {
    /// (time offset, weight) of every picture, the current one first; the
    /// operator's code in `fs_echo`.
    Echo { pictures: Vec<(f64, f32)>, mode: f32 },
    /// The time offset of each slice (slice k is picked by map value
    /// k / (n - 1)), the map (`None`: the input itself), the map value of a
    /// transparent pixel.
    Displace { offsets: Vec<f64>, map: Option<Tex>, neutral: f32 },
}

/// A time effect in a planned chain.
pub(crate) struct TimeStep {
    /// Its position among the layer's enabled effects: its input is the
    /// effects before it.
    pub upto: usize,
    pub op: TimeOp,
}

/// Where the input of a time effect at another time comes from.
#[derive(Clone, Copy)]
pub(crate) enum When<'a> {
    /// `layer`'s own picture; `layers` is the stack it is in.
    Layer { layers: &'a [Layer], layer: &'a Layer },
    /// What lies beneath adjustment layer `index` of `layers`, over
    /// `background` (premultiplied linear).
    Below { layers: &'a [Layer], index: usize, background: [f32; 4] },
}

pub(crate) fn is_time(kind: &str) -> bool {
    matches!(kind, "echo" | "time_displacement")
}

/// The pictures of an echo and its operator code, or `None` when it would
/// change nothing (`count` 0, or only the current picture at full weight).
fn echo_pictures(e: &ResolvedEffect) -> Option<(Vec<(f64, f32)>, f32)> {
    let count = e.num("count").round().clamp(0.0, MAX_PICTURES) as usize;
    let (interval, decay, start) = (e.num("interval"), e.num("decay").clamp(0.0, 1.0), e.num("start").clamp(0.0, 1.0));
    let option = e.option("mode");
    let mode = match option {
        "screen" => 1.0,
        "max" => 2.0,
        "min" => 3.0,
        "over_back" => 4.0,
        "over_front" => 5.0,
        _ => 0.0,
    };
    let weights: Vec<f64> = (0..=count).map(|i| start * decay.powi(i as i32)).collect();
    let sum: f64 = weights.iter().sum();
    let norm = if option == "average" && sum > 0.0 { 1.0 / sum } else { 1.0 };
    // A picture of weight 0 adds nothing, except to `min`; the current one is always there.
    let pictures: Vec<(f64, f32)> =
        weights.iter().enumerate().filter(|&(i, w)| i == 0 || *w > 0.0 || mode == 3.0).map(|(i, w)| (i as f64 * interval, (w * norm) as f32)).collect();
    (pictures.len() > 1 || pictures[0].1 != 1.0).then_some((pictures, mode))
}

/// The slice offsets of a time displacement and the neutral map value, or
/// `None` when it shifts nothing.
fn displace_offsets(e: &ResolvedEffect) -> Option<(Vec<f64>, f32)> {
    let max = e.num("max");
    if max == 0.0 || !max.is_finite() {
        return None;
    }
    let n = e.num("levels").round().clamp(2.0, MAX_PICTURES) as usize;
    let grey = e.option("neutral") != "black";
    let offsets = (0..n)
        .map(|k| {
            let x = k as f64 / (n - 1) as f64;
            if grey {
                max * (2.0 * x - 1.0)
            } else {
                max * x
            }
        })
        .collect();
    Some((offsets, if grey { 0.5 } else { 0.0 }))
}

/// The time offsets a time effect with these values reads, other than 0
/// (for the signature of the layer it is on).
pub(crate) fn offsets(e: &ResolvedEffect) -> Vec<f64> {
    let all = match e.kind.as_str() {
        "echo" => echo_pictures(e).map(|(p, _)| p.into_iter().map(|(dt, _)| dt).collect()).unwrap_or_default(),
        "time_displacement" => displace_offsets(e).map(|(o, _)| o).unwrap_or_default(),
        _ => Vec::new(),
    };
    all.into_iter().filter(|dt| *dt != 0.0).collect()
}

impl Renderer {
    /// The step of time effect `e`, at position `upto` among `layer`'s enabled
    /// effects, or `None` when it would change nothing.
    pub(crate) fn plan_time(&mut self, layer: &str, upto: usize, e: &ResolvedEffect) -> Result<Option<Step>> {
        let op = match e.kind.as_str() {
            "echo" => echo_pictures(e).map(|(pictures, mode)| TimeOp::Echo { pictures, mode }),
            _ => match displace_offsets(e) {
                None => None,
                Some((offsets, neutral)) => {
                    let map = match e.source.as_deref() {
                        None => None,
                        Some(source) => {
                            let name = source.strip_prefix("slot:").unwrap_or("");
                            let tex = self.scene.as_ref().and_then(|s| s.slot_texture(name));
                            Some(tex.ok_or_else(|| anyhow!("layer \"{layer}\": the map \"{source}\" was not prepared (write \"slot:<name>\" naming a slot)"))?)
                        }
                    };
                    Some(TimeOp::Displace { offsets, map, neutral })
                }
            },
        };
        Ok(op.map(|op| Step::Time(TimeStep { upto, op })))
    }

    /// Run a planned chain on `input`, whose time effects read their input at
    /// other times from `when` (`t` is the project time being rendered).
    /// `None` when nothing changed; `input` is never released or written to.
    pub(crate) fn run_chain(&mut self, input: &Tex, steps: &[Step], when: When, t: f64) -> Result<Option<Tex>> {
        let mut cur: Option<Tex> = None;
        let mut i = 0;
        while i < steps.len() {
            let j = steps[i..].iter().position(|s| matches!(s, Step::Time(_))).map_or(steps.len(), |k| i + k);
            let s = cur.clone().unwrap_or_else(|| input.clone());
            let out = if j > i {
                Ok(self.run_effects(&s, &steps[i..j]))
            } else if let Step::Time(step) = &steps[j] {
                self.time_step(&s, step, when, t)
            } else {
                unreachable!("j is a time step")
            };
            match out {
                Ok(Some(out)) => {
                    if let Some(old) = cur.replace(out) {
                        self.release(old);
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    if let Some(old) = cur {
                        self.release(old);
                    }
                    return Err(err);
                }
            }
            i = if j > i { j } else { j + 1 };
        }
        Ok(cur)
    }

    fn time_step(&mut self, input: &Tex, step: &TimeStep, when: When, t: f64) -> Result<Option<Tex>> {
        if self.time_depth >= MAX_TIME_DEPTH {
            bail!("time effects (echo, time_displacement) are nested more than {MAX_TIME_DEPTH} deep; each level multiplies the renders, so nest fewer (or put one in a comp rendered once)");
        }
        self.time_depth += 1;
        let out = match &step.op {
            TimeOp::Echo { pictures, mode } => self.echo(input, pictures, *mode, step.upto, when, t),
            TimeOp::Displace { offsets, map, neutral } => self.displace(input, offsets, map.as_ref(), *neutral, step.upto, when, t),
        };
        self.time_depth -= 1;
        out
    }

    /// The input of the time effect at `upto` at project time `t`, or `None`
    /// where there is nothing (the layer is not active then).
    fn input_at(&mut self, when: When, t: f64, upto: usize) -> Result<Option<Tex>> {
        match when {
            When::Layer { layers, layer } => self.content_before(layers, layer, t, upto),
            When::Below { layers, index, background } => self.below_before(layers, index, t, upto, background).map(Some),
        }
    }

    /// `input` (offset 0) or its re-render at `t + dt`, as an owned texture
    /// when it is a re-render.
    fn shifted(&mut self, when: When, t: f64, dt: f64, upto: usize) -> Result<Option<Option<Tex>>> {
        if dt == 0.0 {
            return Ok(Some(None));
        }
        Ok(self.input_at(when, t + dt, upto)?.map(Some))
    }

    #[allow(clippy::too_many_arguments)]
    fn echo(&mut self, input: &Tex, pictures: &[(f64, f32)], mode: f32, upto: usize, when: When, t: f64) -> Result<Option<Tex>> {
        // The result so far: `None` with `is_input` false before the first picture.
        let mut acc: Option<Tex> = None;
        let mut is_input = false;
        for &(dt, weight) in pictures {
            let shifted = match self.shifted(when, t, dt, upto) {
                Ok(Some(s)) => s,
                // The layer is not there at that time: nothing to add.
                Ok(None) => continue,
                Err(err) => {
                    if let Some(a) = acc {
                        self.release(a);
                    }
                    return Err(err);
                }
            };
            let first = acc.is_none() && !is_input;
            if first && weight == 1.0 {
                match shifted {
                    Some(p) => acc = Some(p),
                    None => is_input = true,
                }
                continue;
            }
            let picture = shifted.clone().unwrap_or_else(|| input.clone());
            let base = if is_input { input.clone() } else { acc.clone().unwrap_or_else(|| picture.clone()) };
            let out = self.acquire(input.w, input.h);
            self.pass(Pipe::Pass("fs_echo"), &out, false, &base, Some(&picture), &[[weight, mode, if first { 1.0 } else { 0.0 }, 0.0]]);
            if let Some(old) = acc.replace(out) {
                self.release(old);
            }
            is_input = false;
            if let Some(p) = shifted {
                self.release(p);
            }
        }
        Ok(acc)
    }

    #[allow(clippy::too_many_arguments)]
    fn displace(&mut self, input: &Tex, offsets: &[f64], map: Option<&Tex>, neutral: f32, upto: usize, when: When, t: f64) -> Result<Option<Tex>> {
        let n = offsets.len() as f32;
        let map = map.cloned().unwrap_or_else(|| input.clone());
        let out = self.acquire(input.w, input.h);
        // Every pixel is written by exactly one slice: the first pass clears
        // the target, the others load it.
        let mut loaded = false;
        for (k, &dt) in offsets.iter().enumerate() {
            let shifted = match self.shifted(when, t, dt, upto) {
                Ok(Some(s)) => s,
                Ok(None) => {
                    // No picture at that time: its pixels stay transparent.
                    if !loaded {
                        self.pass(Pipe::Copy, &out, false, input, None, &[[0.0; 4]]);
                        loaded = true;
                    }
                    continue;
                }
                Err(err) => {
                    self.release(out);
                    return Err(err);
                }
            };
            let picture = shifted.clone().unwrap_or_else(|| input.clone());
            self.pass(Pipe::Pass("fs_time_pick"), &out, loaded, &picture, Some(&map), &[[k as f32, n, neutral, 0.0]]);
            loaded = true;
            if let Some(p) = shifted {
                self.release(p);
            }
        }
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn effect(kind: &str, params: &[(&str, f64)], mode: &str) -> ResolvedEffect {
        let spec = edits_core::effects::spec(kind).unwrap();
        let mut values: BTreeMap<String, Vec<f64>> = spec.params.iter().map(|p| (p.name.to_string(), p.default.to_vec())).collect();
        for (k, v) in params {
            values.insert(k.to_string(), vec![*v]);
        }
        let options = spec.options.iter().map(|o| (o.name.to_string(), if mode.is_empty() { o.default().to_string() } else { mode.to_string() })).collect();
        ResolvedEffect { kind: kind.into(), params: values, options, source: None }
    }

    #[test]
    fn echo_weights_and_offsets() {
        let (p, mode) = echo_pictures(&effect("echo", &[("count", 2.0), ("interval", -0.5), ("decay", 0.5)], "")).unwrap();
        assert_eq!(p, vec![(0.0, 1.0), (-0.5, 0.5), (-1.0, 0.25)]);
        assert_eq!(mode, 0.0);
        assert!(echo_pictures(&effect("echo", &[("count", 0.0)], "")).is_none(), "count 0 plans nothing");
        assert!(echo_pictures(&effect("echo", &[("decay", 0.0)], "")).is_none(), "decay 0 leaves only the current picture");
        let (p, _) = echo_pictures(&effect("echo", &[("count", 1.0), ("decay", 1.0)], "average")).unwrap();
        assert_eq!(p, vec![(0.0, 0.5), (-0.125, 0.5)]);
        assert_eq!(offsets(&effect("echo", &[("count", 2.0), ("interval", 0.25)], "")), vec![0.25, 0.5]);
    }

    #[test]
    fn displacement_slices() {
        let (o, neutral) = displace_offsets(&effect("time_displacement", &[("max", 1.0), ("levels", 3.0)], "")).unwrap();
        assert_eq!((o, neutral), (vec![-1.0, 0.0, 1.0], 0.5));
        let (o, neutral) = displace_offsets(&effect("time_displacement", &[("max", 1.0), ("levels", 3.0)], "black")).unwrap();
        assert_eq!((o, neutral), (vec![0.0, 0.5, 1.0], 0.0));
        assert!(displace_offsets(&effect("time_displacement", &[("max", 0.0)], "")).is_none());
        assert_eq!(offsets(&effect("time_displacement", &[("max", 1.0), ("levels", 3.0)], "")), vec![-1.0, 1.0]);
    }
}
