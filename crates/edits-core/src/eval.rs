//! Evaluate animatable properties at any time, including fractional frames.
//!
//! Evaluation is a pure function of (project, time): no state is carried from
//! one frame to the next. That is what makes scrubbing, sub-frame motion blur
//! and repeatable renders work.

use crate::model::*;
use crate::{ease, effects};
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
}
#[derive(Clone, Debug)]
pub struct ResolvedTransition {
    pub kind: &'static str,
    /// 0 at the start (only the picture beneath), 1 at the end.
    pub progress: f64,
    /// Every parameter of the transition's spec, with defaults filled in.
    pub params: BTreeMap<String, Vec<f64>>,
}

impl ResolvedEffect {
    pub fn num(&self, name: &str) -> f64 {
        self.params.get(name).and_then(|v| v.first().copied()).unwrap_or(0.0)
    }
    pub fn vec(&self, name: &str, n: usize) -> Vec<f64> {
        pad(self.params.get(name).cloned().unwrap_or_default(), n)
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
        if let Some(spec) = effects::spec(&e.kind) {
            for p in spec.params {
                params.insert(p.name.to_string(), p.default.to_vec());
            }
        }
        for (k, v) in &e.params {
            let val = self.prop(v, t);
            // Keep the spec's arity: a single number given for a vector parameter repeats.
            let want = params.get(k).map(|d| d.len()).unwrap_or(val.len());
            params.insert(k.clone(), pad(val, want.max(1)));
        }
        ResolvedEffect { kind: e.kind.clone(), source: e.source.clone(), params }
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

    /// True when `layer` is visible at project time `t`.
    pub fn active(&self, layer: &Layer, t: f64) -> bool {
        let end = layer.end.unwrap_or(self.project.settings.duration);
        layer.enabled && t >= layer.start - 1e-9 && t < end - 1e-9
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
    }
}
