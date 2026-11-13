//! Evaluate animatable properties at any time, including fractional frames.
//!
//! Evaluation is a pure function of (project, time): no state is carried from
//! one frame to the next. That is what makes scrubbing, sub-frame motion blur
//! and repeatable renders work.
//!
//! Video time (`source_seconds`, `source_frame`, `frame_position`) ports the
//! logic of EffectCraft (https://github.com/storytold/effectcraft,
//! MIT OR Apache-2.0, used under MIT): `frame_position` in
//! `crates/render/src/frames.rs` and `timewarp_integrate_speed` in
//! `crates/effects/src/time_fx.rs`. See `THIRD_PARTY.md`.

use crate::model::*;
use crate::{ease, effects, text_select};
use std::collections::BTreeMap;

pub struct Evaluator<'a> {
    pub project: &'a Project,
}

#[derive(Clone, Debug)]
pub struct ResolvedLayer {
    pub position: [f64; 3],
    pub scale: [f64; 3],
    pub rotation: [f64; 3],
    pub anchor: [f64; 2],
    pub opacity: f64,
}

#[derive(Clone, Debug)]
pub struct ResolvedCamera {
    pub fov: f64,
    pub position: [f64; 3],
    pub rotation: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct ResolvedEffect {
    pub kind: String,
    pub source: Option<String>,
    /// Every parameter of the effect's spec, with defaults filled in.
    pub params: BTreeMap<String, Vec<f64>>,
    /// Every option of the effect's spec, with defaults filled in.
    pub options: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct ResolvedTransition {
    pub kind: &'static str,
    /// 0 at the start (only the picture beneath), 1 at the end.
    pub progress: f64,
    /// Every parameter of the transition's spec, with defaults filled in.
    pub params: BTreeMap<String, Vec<f64>>,
}

/// What the core cannot know about a video by itself: the frame rate and
/// the number of frames of its source after the slot's `trim`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipInfo {
    pub fps: f64,
    pub frames: u64,
}

/// Which source frames a video layer shows at an instant: `frame`, crossed
/// with `next` by `weight` (0 shows `frame` alone). Indices count from the
/// first frame kept by the slot's `trim`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceFrame {
    pub frame: u64,
    pub next: u64,
    pub weight: f64,
}

/// Speed samples per project frame when an animated speed is integrated.
const SPEED_SAMPLES_PER_FRAME: f64 = 2.0;
/// Most speed samples one integration takes (longer spans widen the step).
const SPEED_MAX_SAMPLES: f64 = 50_000.0;

/// Position of `position` (in frames) among whole frames: the frame at or
/// before it and the fraction (0..1) of the way to the next. A position
/// within a millionth of a frame of a whole frame is that frame, so that
/// times that line up exactly with frames do not fall one frame short
/// through rounding. (After EffectCraft's `frame_position`, which does this
/// exactly in integer ticks.)
pub fn frame_position(position: f64) -> (i64, f64) {
    if !position.is_finite() {
        return (0, 0.0);
    }
    let whole = (position + 1e-6).floor();
    (whole as i64, (position - whole).clamp(0.0, 1.0 - f64::EPSILON))
}

/// The integral of `speed_at(u)` over `u` from 0 to `span` (seconds), by
/// the midpoint rule on a fixed grid anchored at 0 with `samples_per_second`
/// samples a second (at most [`SPEED_MAX_SAMPLES`]; a longer span widens the
/// step). Anchoring the grid at 0 means successive frames share their
/// samples, so a `hold` key lands the same way on every frame. Non-finite
/// samples count as 0. (A port of EffectCraft's `timewarp_integrate_speed`,
/// with the speed as a multiplier instead of a percentage.)
pub fn integrate_speed(span: f64, samples_per_second: f64, speed_at: impl Fn(f64) -> f64) -> f64 {
    if !span.is_finite() || span <= 0.0 {
        return 0.0;
    }
    let rate = if samples_per_second.is_finite() && samples_per_second > 0.0 { samples_per_second } else { 60.0 };
    let mut h = 1.0 / rate;
    if !(span / h).is_finite() || span / h >= SPEED_MAX_SAMPLES {
        h = span / SPEED_MAX_SAMPLES;
    }
    if !h.is_finite() || h <= 0.0 {
        return 0.0;
    }
    let whole = (span / h).floor().clamp(0.0, SPEED_MAX_SAMPLES);
    let rest = (span - whole * h).max(0.0);
    let sample = |u: f64| Some(speed_at(u)).filter(|v| v.is_finite()).unwrap_or(0.0);
    let full: f64 = (0..whole as u64).map(|i| sample((i as f64 + 0.5) * h)).sum::<f64>() * h;
    let part = if rest > 0.0 { sample(whole * h + rest * 0.5) * rest } else { 0.0 };
    let v = full + part;
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

impl ResolvedEffect {
    pub fn num(&self, name: &str) -> f64 {
        self.params.get(name).and_then(|v| v.first().copied()).unwrap_or(0.0)
    }
    pub fn vec(&self, name: &str, n: usize) -> Vec<f64> {
        pad(self.params.get(name).cloned().unwrap_or_default(), n)
    }
    /// An option's value (its default when not set), or `""` for an option the effect does not have.
    pub fn option(&self, name: &str) -> &str {
        self.options.get(name).map(String::as_str).unwrap_or("")
    }
}

impl<'a> Evaluator<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self { project }
    }

    /// Value of a property at project time `t`.
    pub fn prop(&self, prop: &Prop, t: f64) -> Vec<f64> {
        match prop {
            Prop::Const(v) => v.to_vec(),
            Prop::Anim(a) => self.anim(a, t),
        }
    }
    pub fn num(&self, prop: &Prop, t: f64) -> f64 {
        self.prop(prop, t).first().copied().unwrap_or(0.0)
    }
    pub fn vec2(&self, prop: &Prop, t: f64) -> [f64; 2] {
        let v = pad(self.prop(prop, t), 2);
        [v[0], v[1]]
    }
    pub fn vec3(&self, prop: &Prop, t: f64) -> [f64; 3] {
        let v = pad(self.prop(prop, t), 3);
        [v[0], v[1], v[2]]
    }
    /// Scale accepts a number (uniform), [x, y] (z = 1) or [x, y, z].
    pub fn scale3(&self, prop: &Prop, t: f64) -> [f64; 3] {
        let v = self.prop(prop, t);
        match v.len() {
            0 => [1.0, 1.0, 1.0],
            1 => [v[0], v[0], v[0]],
            2 => [v[0], v[1], 1.0],
            _ => [v[0], v[1], v[2]],
        }
    }

    fn anim(&self, a: &Anim, t: f64) -> Vec<f64> {
        let mut tk = t;
        for m in &a.modifiers {
            if let Modifier::Loop { from, period } = m {
                if *period > 0.0 && tk >= *from {
                    tk = from + (tk - from).rem_euclid(*period);
                }
            }
        }
        let mut out = self.keys(a, tk);
        for (mi, m) in a.modifiers.iter().enumerate() {
            if let Modifier::Shake { amplitude, frequency, seed } = m {
                let amp = self.prop(amplitude, t);
                if out.is_empty() {
                    out = vec![0.0; amp.len().max(1)];
                }
                let secs = self.project.to_seconds(t);
                for (c, o) in out.iter_mut().enumerate() {
                    let a = amp.get(c).or(amp.last()).copied().unwrap_or(0.0);
                    if a != 0.0 {
                        *o += a * noise(secs * frequency, seed.wrapping_add((mi as u32) << 8), c as u32);
                    }
                }
            }
        }
        out
    }

    fn keys(&self, a: &Anim, t: f64) -> Vec<f64> {
        let ks = &a.keys;
        if ks.is_empty() {
            return a.base.as_ref().map(|v| v.to_vec()).unwrap_or_default();
        }
        if t <= ks[0].t {
            return ks[0].v.to_vec();
        }
        let last = ks.len() - 1;
        if t >= ks[last].t {
            return ks[last].v.to_vec();
        }
        // Segment i such that ks[i].t <= t < ks[i + 1].t.
        let i = match ks.binary_search_by(|k| k.t.partial_cmp(&t).unwrap_or(std::cmp::Ordering::Less)) {
            Ok(i) => i.min(last - 1),
            Err(i) => i - 1,
        };
        let (k0, k1) = (&ks[i], &ks[i + 1]);
        let span = k1.t - k0.t;
        if span <= 0.0 {
            return k1.v.to_vec();
        }
        let u = (t - k0.t) / span;
        let frames = span / self.project.frame_len().max(1e-9);
        let f = ease::apply(&k0.ease, u, frames);
        let (a0, a1) = (k0.v.to_vec(), k1.v.to_vec());
        let n = a0.len().max(a1.len());
        let (a0, a1) = (pad(a0, n), pad(a1, n));
        (0..n).map(|c| a0[c] + (a1[c] - a0[c]) * f).collect()
    }

    pub fn layer(&self, layer: &Layer, t: f64) -> ResolvedLayer {
        let tr = &layer.transform;
        ResolvedLayer {
            position: self.vec3(&tr.position, t),
            scale: self.scale3(&tr.scale, t),
            rotation: self.vec3(&tr.rotation, t),
            anchor: self.vec2(&tr.anchor, t),
            opacity: self.num(&tr.opacity, t).clamp(0.0, 1.0),
        }
    }

    pub fn camera(&self, t: f64) -> ResolvedCamera {
        let c = &self.project.camera;
        ResolvedCamera {
            fov: self.num(&c.fov, t).clamp(1.0, 170.0),
            position: self.vec3(&c.position, t),
            rotation: self.vec3(&c.rotation, t),
        }
    }

    pub fn effect(&self, e: &Effect, t: f64) -> ResolvedEffect {
        let mut params = BTreeMap::new();
        let mut options = BTreeMap::new();
        if let Some(spec) = effects::spec(&e.kind) {
            for p in spec.params {
                params.insert(p.name.to_string(), p.default.to_vec());
            }
            for o in spec.options {
                let v = e.options.get(o.name).filter(|v| o.values.contains(&v.as_str())).map(String::as_str).unwrap_or(o.default());
                options.insert(o.name.to_string(), v.to_string());
            }
        }
        for (k, v) in &e.params {
            let val = self.prop(v, t);
            // Keep the spec's arity: a single number given for a vector parameter repeats.
            let want = params.get(k).map(|d| d.len()).unwrap_or(val.len());
            params.insert(k.clone(), pad(val, want.max(1)));
        }
        ResolvedEffect { kind: e.kind.clone(), source: e.source.clone(), params, options }
    }

    /// The transition into `layer` running at `t`, if any: its progress
    /// (eased, 0..1) and its parameters with defaults filled in.
    pub fn transition(&self, layer: &Layer, t: f64) -> Option<ResolvedTransition> {
        let tr = layer.transition.as_ref()?;
        let spec = effects::transition(&tr.kind)?;
        if tr.duration.is_nan() || tr.duration <= 0.0 || t < layer.start - 1e-9 || t >= layer.start + tr.duration - 1e-9 {
            return None;
        }
        let u = ((t - layer.start) / tr.duration).clamp(0.0, 1.0);
        let frames = tr.duration / self.project.frame_len().max(1e-9);
        let progress = ease::apply(&tr.ease, u, frames);
        let mut params = BTreeMap::new();
        for p in spec.params {
            let v = tr.params.get(p.name).map(|v| self.prop(v, t)).unwrap_or_else(|| p.default.to_vec());
            params.insert(p.name.to_string(), pad(v, p.default.len()));
        }
        Some(ResolvedTransition { kind: spec.kind, progress, params })
    }

    /// What the animators of `text` do to each of its `glyphs` at project
    /// time `t` (the layer's time): one state per glyph, in order. Without
    /// enabled animators every glyph is at rest with the text's fill.
    pub fn text_glyphs(&self, text: &Text, glyphs: &[text_select::GlyphRef], t: f64) -> Vec<text_select::GlyphState> {
        use text_select::*;
        let mut out = vec![GlyphState::rest(text.fill); glyphs.len()];
        let opt = |p: &Option<Prop>, d: f64| p.as_ref().map_or(d, |p| self.num(p, t));
        for a in text.animators.iter().filter(|a| a.enabled) {
            let props = AnimatorAt {
                position: a.position.as_ref().map(|p| self.vec3(p, t)),
                scale: a.scale.as_ref().map(|p| self.scale3(p, t)),
                rotation: a.rotation.as_ref().map(|p| self.vec3(p, t)),
                opacity: a.opacity.as_ref().map(|p| self.num(p, t)),
                fill: a.fill.as_ref().map(|p| {
                    let v = self.prop(p, t);
                    let c = pad(v.clone(), 3);
                    [c[0], c[1], c[2], if v.len() >= 4 { v[3] } else { 1.0 }]
                }),
                tracking: a.tracking.as_ref().map(|p| self.num(p, t)),
                blur: a.blur.as_ref().map(|p| self.num(p, t)),
            };
            let (unit, n) = units_of(&text.text, glyphs, a.selector.units());
            let selection: Vec<f64> = match &a.selector {
                Selector::Range(r) => {
                    let at = RangeAt {
                        start: opt(&r.start, 0.0),
                        end: opt(&r.end, 100.0),
                        offset: opt(&r.offset, 0.0),
                        smoothness: opt(&r.smoothness, 100.0),
                        ease_high: opt(&r.ease_high, 0.0),
                        ease_low: opt(&r.ease_low, 0.0),
                        amount: opt(&r.amount, 100.0),
                    };
                    let order = r.randomize.then(|| random_order(n, r.seed));
                    let values: Vec<f64> = (0..n).map(|u| range_value(r, order.as_ref().map_or(u, |o| o[u]), n, &at)).collect();
                    unit.iter().map(|&u| values[u]).collect()
                }
                Selector::Wiggly(w) => {
                    let at = WigglyAt { amount: opt(&w.amount, 100.0), frequency: opt(&w.frequency, 2.0), correlation: opt(&w.correlation, 50.0) };
                    let secs = self.project.to_seconds(t);
                    unit.iter().map(|&u| wiggly_value(u, secs, w.seed, &at)).collect()
                }
            };
            for (g, s) in out.iter_mut().zip(selection) {
                apply(g, &props, s);
            }
        }
        out
    }

    /// True when `layer` is visible at project time `t`.
    pub fn active(&self, layer: &Layer, t: f64) -> bool {
        let end = layer.end.unwrap_or(self.project.settings.duration);
        layer.enabled && t >= layer.start - 1e-9 && t < end - 1e-9
    }

    /// The instant at which `layer`'s own values (transform, effects, video
    /// frame) are evaluated at project time `t`: `t` itself, or with the
    /// layer's `fps` set, the start of the step of `1 / fps` seconds
    /// (counted from the layer's `in`) that `t` falls in.
    pub fn layer_time(&self, layer: &Layer, t: f64) -> f64 {
        let Some(fps) = layer.fps.filter(|f| f.is_finite() && *f > 0.0) else { return t };
        let p = self.project;
        let start = p.to_seconds(layer.start);
        let steps = ((p.to_seconds(t) - start) * fps + 1e-6).floor().max(0.0);
        p.from_seconds(start + steps / fps)
    }

    /// Seconds into a video layer's (trimmed) source at project time `t`,
    /// before looping, reversing or clamping to the clip: `time_remap` at
    /// that instant, or else the speed integrated from the layer's `in`.
    /// Never negative.
    pub fn source_seconds(&self, layer: &Layer, video: &Video, t: f64) -> f64 {
        if let Some(remap) = &video.time_remap {
            let s = self.num(remap, self.layer_time(layer, t));
            return if s.is_finite() { s.max(0.0) } else { 0.0 };
        }
        self.speed_seconds(layer, video.speed.as_ref(), t)
    }

    /// Seconds of source played from the layer's `in` to project time `t`
    /// (at the layer's time) at `speed` (1 when `None`): the speed
    /// integrated over the layer's time. Never negative. Video and comp
    /// layers share it.
    pub fn speed_seconds(&self, layer: &Layer, speed: Option<&Prop>, t: f64) -> f64 {
        let p = self.project;
        let lt = self.layer_time(layer, t);
        let start = p.to_seconds(layer.start);
        let span = (p.to_seconds(lt) - start).max(0.0);
        let s = match speed {
            None => span,
            Some(Prop::Const(v)) => span * v.to_vec().first().copied().unwrap_or(1.0).max(0.0),
            Some(speed) => {
                let rate = p.settings.fps * SPEED_SAMPLES_PER_FRAME;
                integrate_speed(span, rate, |u| self.num(speed, p.from_seconds(start + u)).max(0.0))
            }
        };
        if s.is_finite() {
            s
        } else {
            0.0
        }
    }

    /// The source frames a video layer shows at project time `t`, given its
    /// clip: past the end the last frame holds (or, with `loop`, the clip
    /// starts again), `reverse` counts from the last frame, and
    /// `frame_blend: mix` crosses into the next frame by the fraction of the
    /// way to it. `None` when the clip has no frames.
    pub fn source_frame(&self, layer: &Layer, video: &Video, t: f64, clip: ClipInfo) -> Option<SourceFrame> {
        if clip.frames == 0 || !clip.fps.is_finite() || clip.fps <= 0.0 {
            return None;
        }
        let n = clip.frames as f64;
        let mut pos = self.source_seconds(layer, video, t) * clip.fps;
        if video.looped {
            pos = pos.rem_euclid(n);
            if video.reverse {
                pos = (n - 1.0 - pos).rem_euclid(n);
            }
        } else {
            pos = pos.clamp(0.0, n - 1.0);
            if video.reverse {
                pos = n - 1.0 - pos;
            }
        }
        let (whole, fraction) = frame_position(pos);
        let last = clip.frames - 1;
        let frame = (whole.max(0) as u64).min(last);
        if video.frame_blend == FrameBlend::None || fraction < 1e-6 {
            return Some(SourceFrame { frame, next: frame, weight: 0.0 });
        }
        let next = if video.looped { (frame + 1) % clip.frames } else { (frame + 1).min(last) };
        let weight = if next == frame { 0.0 } else { fraction };
        Some(SourceFrame { frame, next, weight })
    }
}

/// Pad or truncate to `n` components; a shorter vector repeats its last value.
pub fn pad(mut v: Vec<f64>, n: usize) -> Vec<f64> {
    let last = v.last().copied().unwrap_or(0.0);
    v.resize(n, last);
    v
}

/// Smooth deterministic noise in about -1..1: two octaves of 1D value noise.
pub fn noise(x: f64, seed: u32, channel: u32) -> f64 {
    let n = |x: f64, salt: u32| {
        let i = x.floor();
        let f = x - i;
        let s = f * f * (3.0 - 2.0 * f);
        let a = hash(i as i64, seed, channel.wrapping_add(salt));
        let b = hash(i as i64 + 1, seed, channel.wrapping_add(salt));
        a + (b - a) * s
    };
    (n(x, 0) + 0.5 * n(x * 2.03 + 17.0, 101)) / 1.5
}

fn hash(i: i64, seed: u32, channel: u32) -> f64 {
    let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ ((seed as u64) << 32 | channel as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project::from_json(
            r#"{"format":"edits.project/1",
                "settings":{"width":64,"height":64,"fps":25,"duration":8,"time_base":"beats","bpm":120},
                "layers":[]}"#,
        )
        .unwrap()
    }

    #[test]
    fn beats_convert_to_seconds() {
        let p = project();
        assert!((p.to_seconds(4.0) - 2.0).abs() < 1e-9);
        assert!((p.from_seconds(2.0) - 4.0).abs() < 1e-9);
        assert_eq!(p.frame_count(), 100);
    }

    #[test]
    fn keys_interpolate_and_clamp() {
        let p = project();
        let e = Evaluator::new(&p);
        let prop: Prop = serde_json::from_str(r#"{"keys":[{"t":1,"v":[0,0]},{"t":3,"v":[2,4],"ease":"hold"},{"t":5,"v":[9,9]}]}"#).unwrap();
        assert_eq!(e.prop(&prop, 0.0), vec![0.0, 0.0]);
        assert_eq!(e.prop(&prop, 2.0), vec![1.0, 2.0]);
        assert_eq!(e.prop(&prop, 4.0), vec![2.0, 4.0]); // hold until the next key
        assert_eq!(e.prop(&prop, 9.0), vec![9.0, 9.0]);
    }

    #[test]
    fn shake_is_deterministic_and_bounded() {
        let p = project();
        let e = Evaluator::new(&p);
        let prop: Prop = serde_json::from_str(r#"{"base":[0,0,0],"modifiers":[{"kind":"shake","amplitude":0.02,"frequency":10,"seed":3}]}"#).unwrap();
        let a = e.prop(&prop, 1.37);
        assert_eq!(a, e.prop(&prop, 1.37));
        assert_eq!(a.len(), 3);
        assert!(a.iter().all(|v| v.abs() <= 0.02 + 1e-9));
        assert!(a[0] != a[1]);
    }

    #[test]
    fn effect_defaults_are_filled() {
        let p = project();
        let e = Evaluator::new(&p);
        let fx: Effect = serde_json::from_str(r#"{"kind":"glow","params":{"radius":5}}"#).unwrap();
        let r = e.effect(&fx, 0.0);
        assert_eq!(r.num("radius"), 5.0);
        assert_eq!(r.num("threshold"), 0.7);
        assert_eq!(r.vec("color", 3), vec![1.0, 1.0, 1.0]);
        // Options: the default when missing or not one of the allowed values.
        let fx: Effect = serde_json::from_str(r#"{"kind":"halftone","options":{"mode":"cmyk"}}"#).unwrap();
        let r = e.effect(&fx, 0.0);
        assert_eq!((r.option("mode"), r.option("shape"), r.option("nope")), ("cmyk", "dot", ""));
        let fx: Effect = serde_json::from_str(r#"{"kind":"halftone","options":{"mode":"sepia"}}"#).unwrap();
        assert_eq!(e.effect(&fx, 0.0).option("mode"), "mono");
    }

    /// A 24 fps project in seconds with one video layer from `json` (spliced into the layer).
    fn video_project(json: &str) -> Project {
        Project::from_json(&format!(
            r#"{{"format":"edits.project/1","settings":{{"width":64,"height":64,"fps":24,"duration":10}},
                "slots":{{"v":{{"path":"clip.mkv"}}}},
                "layers":[{{"id":"clip","kind":"video","slot":"v","in":1{json}}}]}}"#
        ))
        .unwrap()
    }

    fn video(p: &Project) -> &Video {
        match &p.layers[0].kind {
            LayerKind::Video(v) => v,
            other => panic!("not a video: {other:?}"),
        }
    }

    #[test]
    fn video_layers_round_trip_with_defaults_left_out() {
        let p = video_project(r#","loop":true,"frame_blend":"mix","volume":0,"fps":12"#);
        let v = video(&p);
        assert!(v.looped && !v.reverse && v.frame_blend == FrameBlend::Mix && v.volume == 0.0 && v.speed.is_none());
        assert_eq!(p.layers[0].fps, Some(12.0));
        let text = p.to_json();
        assert!(text.contains("\"loop\": true") && !text.contains("reverse") && !text.contains("speed"), "{text}");
        let plain = video_project("");
        assert_eq!(video(&plain).volume, 1.0);
        assert!(!plain.to_json().contains("volume"));
    }

    #[test]
    fn animated_speed_integrates_to_the_analytic_value() {
        // Speed ramps linearly from 1 at t = 1 (the layer's in) to 3 at t = 3, then holds:
        // source(t) = (t - 1) + (t - 1)^2 / 2 up to t = 3 (= 4), then 4 + 3 (t - 3).
        let p = video_project(r#","speed":{"keys":[{"t":1,"v":1},{"t":3,"v":3}]}"#);
        let e = Evaluator::new(&p);
        let (l, v) = (&p.layers[0], video(&p));
        for t in [1.0, 1.5, 2.0, 2.7, 3.0, 4.25, 7.0] {
            let u: f64 = t - 1.0;
            let want = if t <= 3.0 { u + u * u / 2.0 } else { 4.0 + 3.0 * (t - 3.0) };
            let got = e.source_seconds(l, v, t);
            assert!((got - want).abs() < 1e-9, "t={t}: {got} vs {want}");
        }
        // A constant speed is exact, and a remap overrides it.
        let p = video_project(r#","speed":0.5"#);
        assert!((Evaluator::new(&p).source_seconds(&p.layers[0], video(&p), 3.0) - 1.0).abs() < 1e-12);
        let p = video_project(r#","speed":4,"time_remap":{"keys":[{"t":1,"v":2},{"t":3,"v":0}]}"#);
        assert!((Evaluator::new(&p).source_seconds(&p.layers[0], video(&p), 2.5) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn hold_keys_land_the_same_way_on_every_frame() {
        // Speed 1, then 0.5 from a hold key between two samples: past it every frame
        // advances the source by exactly half a frame.
        let p = video_project(r#","speed":{"keys":[{"t":1,"v":1,"ease":"hold"},{"t":2.01,"v":0.5}]}"#);
        let e = Evaluator::new(&p);
        let at = |k: u32| e.source_seconds(&p.layers[0], video(&p), 1.0 + k as f64 / 24.0);
        for k in 30..90 {
            assert!((at(k) - at(k - 1) - 0.5 / 24.0).abs() < 1e-9, "k={k}: {}", at(k) - at(k - 1));
        }
        assert_eq!(integrate_speed(f64::NAN, 48.0, |_| 1.0), 0.0);
        assert!(integrate_speed(1e12, 48.0, |_| 1.0).is_finite());
    }

    #[test]
    fn source_frames_hold_loop_reverse_and_mix() {
        let clip = ClipInfo { fps: 24.0, frames: 48 };
        let frame = |json: &str, t: f64| {
            let p = video_project(json);
            Evaluator::new(&p).source_frame(&p.layers[0], video(&p), t, clip).unwrap()
        };
        let at = |n: u64| 1.0 + n as f64 / 24.0;
        assert_eq!(frame("", at(5)).frame, 5);
        assert_eq!(frame("", at(100)).frame, 47, "holds the last frame");
        assert_eq!(frame(r#","loop":true"#, at(50)).frame, 2);
        assert_eq!(frame(r#","reverse":true"#, at(0)).frame, 47);
        assert_eq!(frame(r#","reverse":true"#, at(47)).frame, 0);
        assert_eq!(frame(r#","reverse":true,"loop":true"#, at(48)).frame, 47);
        assert_eq!(frame(r#","speed":2"#, at(7)).frame, 14);
        let half = frame(r#","speed":0.5,"frame_blend":"mix""#, at(7));
        assert!((half.frame, half.next) == (3, 4) && (half.weight - 0.5).abs() < 1e-9, "{half:?}");
        assert_eq!(frame(r#","speed":0.5"#, at(7)), SourceFrame { frame: 3, next: 3, weight: 0.0 });
        // Mixing past the last frame of a loop crosses into the first.
        let wrap = frame(r#","speed":0.5,"loop":true,"frame_blend":"mix""#, at(95));
        assert_eq!((wrap.frame, wrap.next), (47, 0));
        let plain = video_project("");
        assert_eq!(Evaluator::new(&plain).source_frame(&plain.layers[0], video(&plain), 1.0, ClipInfo { fps: 24.0, frames: 0 }), None);
    }

    #[test]
    fn layer_fps_posterizes_time_from_the_layer_in() {
        let p = video_project(r#","fps":12"#);
        let e = Evaluator::new(&p);
        let l = &p.layers[0];
        // Steps of 1/12 s from t = 1: frames 0 and 1 of the project share a step.
        let at = |n: u64| e.layer_time(l, 1.0 + n as f64 / 24.0);
        assert!((at(0) - 1.0).abs() < 1e-12 && (at(1) - 1.0).abs() < 1e-12);
        assert!((at(2) - (1.0 + 1.0 / 12.0)).abs() < 1e-12 && (at(3) - at(2)).abs() < 1e-12);
        let clip = ClipInfo { fps: 24.0, frames: 48 };
        let frames: Vec<u64> = (0..6).map(|n| e.source_frame(l, video(&p), 1.0 + n as f64 / 24.0, clip).unwrap().frame).collect();
        assert_eq!(frames, [0, 0, 2, 2, 4, 4]);
        // Without fps the time is untouched.
        let plain = video_project("");
        assert_eq!(Evaluator::new(&plain).layer_time(&plain.layers[0], 1.37), 1.37);
    }

    #[test]
    fn trim_keeps_the_frames_between_in_and_out() {
        let slot = |trim: &str| -> Slot { serde_json::from_str(&format!(r#"{{"path":"a.mp4","trim":{trim}}}"#)).unwrap() };
        assert_eq!(slot("[0.5, 1.5]").trim_frames(24.0, 48), (12, 24));
        assert_eq!(slot("[0, 100]").trim_frames(24.0, 48), (0, 48));
        assert_eq!(slot("[5, 6]").trim_frames(24.0, 48), (47, 1));
        assert_eq!(Slot::default().trim_frames(24.0, 48), (0, 48));
    }
}
