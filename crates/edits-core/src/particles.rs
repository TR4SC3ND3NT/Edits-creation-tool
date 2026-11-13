//! Particle layers: petals, sparkles, dust, a burst on a beat.
//!
//! Nothing is simulated step by step. Every particle is born at a time that
//! follows from the emission (`rate`, `bursts`) alone, draws its random
//! numbers from a hash of (`seed`, stream, index), and its state at any
//! instant is a closed-form function of its age: ballistic motion under
//! constant gravity with linear drag, plus a bounded curl-noise drift. A
//! frame is therefore a pure function of the project and the frame number,
//! seeking anywhere costs the same, and motion blur samples are just more
//! instants.
//!
//! The closed-form idea (birth times from the emission rate, per-item hashed
//! constants, only the items whose birth falls in the live window evaluated)
//! follows EffectCraft's `crates/effects/src/sim/weather.rs`
//! (https://github.com/storytold/effectcraft, MIT OR Apache-2.0, used under
//! MIT); the turbulence is the curl of a scalar noise potential (the
//! potential's gradient turned by 90 degrees, as in EffectCraft's
//! `fx_particles.wgsl` curl passes), evaluated here on the CPU and applied
//! as a displacement instead of advecting pixels. The code is our own.
//!
//! Units: positions and emitter sizes in composition heights (layer space,
//! origin at the centre, y down); `size` in % of the height; times (`rate`,
//! `bursts`, `life`) in the project's time base; motion (`speed`, `gravity`,
//! `drag`, `spin`, turbulence) per second; angles in degrees, 0 pointing
//! right and 90 down.

use crate::check::{Finding, Severity};
use crate::model::{Layer, Project, Prop, Value};
use crate::Evaluator;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A `particles` layer.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Particles {
    /// Where particles are born.
    #[serde(default)]
    pub emitter: Emitter,
    /// Particles born per project time unit (beat or second), evenly spaced
    /// from the layer's `in`. Can be keyed (it is integrated).
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub rate: Prop,
    /// Particles born all at once, e.g. on a beat.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bursts: Vec<Burst>,
    /// How long each particle lives, in project time units.
    #[serde(default = "default_life")]
    pub life: f64,
    /// Random shortening of each life, 0..1 (0.3: between 70 % and 100 %).
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub life_random: f64,
    /// Start emitting one `life` before the layer's `in`, so the layer
    /// starts already full (falling petals, floating dust).
    #[serde(default, skip_serializing_if = "is_false")]
    pub prewarm: bool,
    /// Initial speed in composition heights per second (at birth).
    #[serde(default = "default_speed")]
    pub speed: Prop,
    /// Random change of each speed, 0..1 (0.5: 50 % to 150 %).
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub speed_random: f64,
    /// Direction of the initial velocity in degrees: 0 right, 90 down, -90 up.
    #[serde(default = "default_direction")]
    pub direction: Prop,
    /// Width of the cone of directions around `direction`, in degrees (360: all round).
    #[serde(default = "default_spread")]
    pub spread: Prop,
    /// Constant acceleration in composition heights per second squared: a
    /// number pulls down (negative: up), or `[x, y]` (wind).
    #[serde(default = "zero_value", skip_serializing_if = "is_zero_value")]
    pub gravity: Value,
    /// Air resistance per second: velocity decays as e^(-drag t) towards
    /// the terminal velocity gravity / drag.
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub drag: f64,
    /// Swirling drift from curl noise.
    #[serde(default, skip_serializing_if = "Turbulence::is_off")]
    pub turbulence: Turbulence,
    /// Diameter in % of the composition height (at birth).
    #[serde(default = "default_size")]
    pub size: Prop,
    /// Diameter at the end of life (% of height); linear over life. `None` keeps `size`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_end: Option<f64>,
    /// Random change of each size, 0..1.
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub size_random: f64,
    /// Linear RGB (at birth).
    #[serde(default = "white")]
    pub color: Prop,
    /// Colour at the end of life; linear over life. `None` keeps `color`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_end: Option<[f64; 3]>,
    /// Opacity at birth, 0..1.
    #[serde(default = "one")]
    pub opacity: Prop,
    /// Opacity at the end of life; linear over life. `None` keeps `opacity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity_end: Option<f64>,
    /// Fraction of the life spent fading in from transparent.
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub fade_in: f64,
    /// Fraction of the life spent fading out to transparent.
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub fade_out: f64,
    /// Turning speed in degrees per second (each particle also starts at a random angle).
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub spin: f64,
    /// Random change of each turning speed, ± degrees per second.
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub spin_random: f64,
    /// For `spark`: seconds of travel the streak shows behind the particle.
    #[serde(default = "default_streak")]
    pub streak: f64,
    /// `dot`, `spark` (a streak along the velocity), `star` (a four-point
    /// twinkle), `petal` (a tumbling sakura petal) or `slot:<name>` (a picture).
    #[serde(default = "default_sprite")]
    pub sprite: String,
    #[serde(default)]
    pub seed: u32,
    /// Most particles drawn at once; the oldest beyond it are left out.
    #[serde(default = "default_max")]
    pub max: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Emitter {
    #[serde(default)]
    pub shape: EmitterShape,
    /// Centre of the emitter in layer space (at each birth; can be keyed).
    #[serde(default = "zero2")]
    pub position: Prop,
    /// `[width, height]` in composition heights: a line's length is the
    /// width, a rect fills the box, a circle the ellipse inside it.
    #[serde(default = "zero_size")]
    pub size: [f64; 2],
    /// Turn of the shape, in degrees.
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub angle: f64,
    /// Emit from the outline only (a rect's border, a circle's ring).
    #[serde(default, skip_serializing_if = "is_false")]
    pub edge: bool,
}
impl Default for Emitter {
    fn default() -> Self {
        Self { shape: EmitterShape::Point, position: zero2(), size: [0.0, 0.0], angle: 0.0, edge: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmitterShape {
    #[default]
    Point,
    Line,
    Rect,
    Circle,
}

/// `count` particles born at `t` (project time; text times such as `"b12"` are accepted).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Burst {
    pub t: f64,
    pub count: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Turbulence {
    /// How far particles wander, in % of the composition height.
    #[serde(default)]
    pub amount: f64,
    /// Size of the eddies, in % of the height.
    #[serde(default = "default_turb_scale")]
    pub scale: f64,
    /// How fast the eddies change, per second.
    #[serde(default = "one_f")]
    pub speed: f64,
}
impl Default for Turbulence {
    fn default() -> Self {
        Self { amount: 0.0, scale: default_turb_scale(), speed: 1.0 }
    }
}
impl Turbulence {
    fn is_off(&self) -> bool {
        *self == Turbulence::default()
    }
}

/// The picture of each particle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sprite<'a> {
    Dot,
    Spark,
    Star,
    Petal,
    Slot(&'a str),
}

pub const SPRITES: [&str; 5] = ["dot", "spark", "star", "petal", "slot:<name>"];

impl Particles {
    /// The sprite, or `None` when `sprite` is not a known one.
    pub fn sprite(&self) -> Option<Sprite<'_>> {
        Some(match self.sprite.as_str() {
            "dot" => Sprite::Dot,
            "spark" => Sprite::Spark,
            "star" => Sprite::Star,
            "petal" => Sprite::Petal,
            s => Sprite::Slot(s.strip_prefix("slot:").filter(|n| !n.is_empty())?),
        })
    }
    /// The slot a `slot:<name>` sprite shows.
    pub fn sprite_slot(&self) -> Option<&str> {
        match self.sprite() {
            Some(Sprite::Slot(name)) => Some(name),
            _ => None,
        }
    }

    /// One line for `describe`.
    pub fn describe(&self) -> String {
        let mut parts = vec![format!("particles {}", self.sprite)];
        let shape = match self.emitter.shape {
            EmitterShape::Point => "point",
            EmitterShape::Line => "line",
            EmitterShape::Rect => "rect",
            EmitterShape::Circle => "circle",
        };
        parts.push(format!("from {shape}{}", if self.emitter.edge { " edge" } else { "" }));
        if !is_zero(&self.rate) {
            parts.push(format!("rate {}", prop_text(&self.rate)));
        }
        if !self.bursts.is_empty() {
            let list: Vec<String> = self.bursts.iter().map(|b| format!("{}@{}", b.count, b.t)).collect();
            parts.push(format!("bursts {}", list.join(",")));
        }
        parts.push(format!("life {}", self.life));
        parts.push(format!("speed {}", prop_text(&self.speed)));
        let g = self.gravity.to_vec();
        if g.iter().any(|v| *v != 0.0) {
            parts.push(format!("gravity {}", if g.len() == 1 { g[0].to_string() } else { format!("{g:?}") }));
        }
        if self.turbulence.amount != 0.0 {
            parts.push(format!("turbulence {}", self.turbulence.amount));
        }
        parts.push(format!("size {}%", prop_text(&self.size)));
        if self.prewarm {
            parts.push("prewarm".into());
        }
        parts.push(format!("seed {}", self.seed));
        parts.join(" ")
    }
}

fn prop_text(p: &Prop) -> String {
    match p {
        Prop::Const(Value::Num(n)) => format!("{n}"),
        Prop::Const(Value::Vec(v)) => format!("{v:?}"),
        Prop::Anim(a) => format!("keyed ({} keys)", a.keys.len()),
    }
}

/// Validation of a particles layer (called by `check`).
pub fn check(ps: &Particles, p: &Project, lp: &str, err: &mut impl FnMut(String, String, Option<&str>), warn: &mut Vec<Finding>, used_slots: &mut BTreeSet<String>) {
    let mut warning = |path: String, message: String| warn.push(Finding { severity: Severity::Warning, path, message, hint: None });
    let values = |prop: &Prop| -> Vec<f64> {
        match prop {
            Prop::Const(v) => v.to_vec(),
            Prop::Anim(a) => a.keys.iter().flat_map(|k| k.v.to_vec()).chain(a.base.iter().flat_map(Value::to_vec)).collect(),
        }
    };
    if let Some(bad) = values(&ps.rate).into_iter().find(|v| !v.is_finite() || *v < 0.0) {
        err(format!("{lp}.rate"), format!("rate must be 0 or more particles per time unit, got {bad}"), Some("0 emits only the bursts"));
    }
    if !ps.life.is_finite() || ps.life <= 0.0 {
        err(format!("{lp}.life"), format!("life must be positive, got {}", ps.life), Some("in the project's time unit (beats or seconds)"));
    }
    for (name, v) in [("life_random", ps.life_random), ("speed_random", ps.speed_random), ("size_random", ps.size_random), ("fade_in", ps.fade_in), ("fade_out", ps.fade_out)] {
        if !(0.0..=1.0).contains(&v) {
            err(format!("{lp}.{name}"), format!("{name} must be between 0 and 1, got {v}"), None);
        }
    }
    if ps.fade_in + ps.fade_out > 1.0 + 1e-9 {
        warning(format!("{lp}.fade_in"), format!("fade_in + fade_out is {} (more than the whole life): particles never reach full opacity", ps.fade_in + ps.fade_out));
    }
    if let Some(bad) = values(&ps.speed).into_iter().chain(values(&ps.size)).chain(ps.size_end).find(|v| !v.is_finite() || *v < 0.0) {
        err(format!("{lp}.speed"), format!("speed and size must not be negative, got {bad}"), Some("turn the direction instead of using a negative speed"));
    }
    if !ps.drag.is_finite() || ps.drag < 0.0 {
        err(format!("{lp}.drag"), format!("drag must be 0 or more, got {}", ps.drag), None);
    }
    let g = ps.gravity.to_vec();
    if g.is_empty() || g.len() > 2 || g.iter().any(|v| !v.is_finite()) {
        err(format!("{lp}.gravity"), "gravity is a number (down) or [x, y]".into(), None);
    }
    if ps.emitter.size.iter().any(|v| !v.is_finite() || *v < 0.0) {
        err(format!("{lp}.emitter.size"), format!("emitter size must not be negative, got {:?}", ps.emitter.size), None);
    }
    if ps.turbulence.amount != 0.0 && (!ps.turbulence.scale.is_finite() || ps.turbulence.scale <= 0.0) {
        err(format!("{lp}.turbulence.scale"), "turbulence scale must be positive".into(), Some("the size of the eddies in % of height, e.g. 20"));
    }
    if !ps.streak.is_finite() || ps.streak < 0.0 {
        err(format!("{lp}.streak"), "streak must be 0 or more seconds".into(), None);
    }
    match ps.sprite() {
        None => err(format!("{lp}.sprite"), format!("unknown sprite \"{}\"", ps.sprite), Some(&format!("one of: {}", SPRITES.join(", ")))),
        Some(Sprite::Slot(name)) => {
            used_slots.insert(name.to_string());
            if !p.slots.contains_key(name) {
                let names: Vec<&str> = p.slots.keys().map(String::as_str).collect();
                err(format!("{lp}.sprite"), format!("no slot \"{name}\" for the sprite"), Some(&format!("slots: {}", if names.is_empty() { "(none)".into() } else { names.join(", ") })));
            }
        }
        Some(_) => {}
    }
    for (i, b) in ps.bursts.iter().enumerate() {
        if !b.t.is_finite() {
            err(format!("{lp}.bursts[{i}].t"), "a burst needs a time".into(), None);
        }
    }
    let rate_max = values(&ps.rate).into_iter().fold(0.0f64, f64::max);
    let burst_max = ps.bursts.iter().map(|b| b.count as f64).sum::<f64>();
    if rate_max == 0.0 && ps.bursts.is_empty() {
        warning(format!("{lp}.rate"), "no rate and no bursts: the layer shows nothing".into());
    }
    if ps.life.is_finite() && rate_max * ps.life + burst_max > ps.max as f64 {
        warning(format!("{lp}.max"), format!("up to {:.0} particles can be alive at once, more than max {}: the oldest are left out", rate_max * ps.life + burst_max, ps.max));
    }
}

/// One particle at an instant, in layer space.
#[derive(Clone, Debug, PartialEq)]
pub struct Particle {
    /// Unique within the layer: stream (0 = rate, 1 + i = burst i) and index.
    pub id: (u32, u64),
    /// Birth time in project time.
    pub birth: f64,
    /// Fraction of its life lived, 0..1.
    pub age: f64,
    pub position: [f64; 2],
    /// Composition heights per second.
    pub velocity: [f64; 2],
    /// Diameter in composition heights.
    pub size: f64,
    /// Degrees.
    pub rotation: f64,
    /// Width factor of a tumbling petal, -1..1 (1 for the other sprites).
    pub flip: f64,
    /// Linear RGB.
    pub color: [f64; 3],
    pub opacity: f64,
}

/// Every particle of `layer` alive at layer time `t` (see
/// [`Evaluator::layer_time`]), oldest first. At most `max`: the newest.
pub fn evaluate(ev: &Evaluator, layer: &Layer, ps: &Particles, t: f64) -> Vec<Particle> {
    let life = ps.life;
    if !life.is_finite() || life <= 0.0 || !t.is_finite() {
        return Vec::new();
    }
    let mut born: Vec<(u32, u64, f64)> = Vec::new();
    let e0 = layer.start - if ps.prewarm { life } else { 0.0 };
    for (k, b) in rate_births(ev, &ps.rate, e0, t, life) {
        born.push((0, k, b));
    }
    for (j, burst) in ps.bursts.iter().enumerate() {
        if burst.t <= t + EPS && burst.t > t - life - EPS {
            born.extend((0..burst.count as u64).map(|i| (1 + j as u32, i, burst.t)));
        }
    }
    born.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    let mut out: Vec<Particle> = born.into_iter().filter_map(|(stream, k, b)| particle(ev, ps, stream, k, b, t)).collect();
    let max = ps.max as usize;
    if out.len() > max {
        out.drain(..out.len() - max);
    }
    out
}

/// Times are compared with this tolerance, so that a burst on a beat that
/// falls exactly on a frame shows on that frame.
const EPS: f64 = 1e-9;

/// (index, birth) of the rate particles born in (t - life, t]. Births are
/// spaced evenly from `e0` at the rate; a keyed rate is integrated on a grid
/// of half frames anchored at `e0`, so a particle's birth time is the same at
/// every instant it is evaluated.
fn rate_births(ev: &Evaluator, rate: &Prop, e0: f64, t: f64, life: f64) -> Vec<(u64, f64)> {
    let mut out = Vec::new();
    if t < e0 - EPS {
        return out;
    }
    let lo = t - life;
    if let Prop::Const(v) = rate {
        let r = v.to_vec().first().copied().unwrap_or(0.0);
        if !r.is_finite() || r <= 0.0 {
            return out;
        }
        let first = ((lo - e0) * r - 1e-7).floor().max(0.0) as u64;
        let last = ((t - e0) * r + 1e-7).floor().max(0.0) as u64;
        for k in first..=last {
            let b = e0 + k as f64 / r;
            if b <= t + EPS && b > lo + EPS {
                out.push((k, b));
            }
        }
        return out;
    }
    let mut dt = (ev.project.frame_len() * 0.5).max(1e-6);
    if (t - e0) / dt > 200_000.0 {
        dt = (t - e0) / 200_000.0;
    }
    let mut n = 0.0f64;
    let mut c = 0u64;
    loop {
        let c0 = e0 + c as f64 * dt;
        if c0 > t + EPS {
            break;
        }
        let r = ev.num(rate, c0 + dt * 0.5);
        let r = if r.is_finite() { r.max(0.0) } else { 0.0 };
        let n1 = n + r * dt;
        if c0 + dt > lo {
            let mut k = (n - 1e-9).ceil().max(0.0);
            while k < n1 - 1e-9 {
                let b = c0 + dt * (k - n) / (n1 - n);
                if b <= t + EPS && b > lo + EPS {
                    out.push((k as u64, b));
                }
                k += 1.0;
            }
        }
        n = n1;
        c += 1;
    }
    out
}

/// One particle's state at `t`, or `None` when it is not alive then.
fn particle(ev: &Evaluator, ps: &Particles, stream: u32, k: u64, birth: f64, t: f64) -> Option<Particle> {
    let r = |n: u32| rand(ps.seed, stream, k, n);
    let life = ps.life * (1.0 - ps.life_random.clamp(0.0, 1.0) * r(0));
    let lived = t - birth;
    if lived < -EPS || lived >= life - EPS {
        return None;
    }
    let age = (lived / life).clamp(0.0, 1.0);
    let p = ev.project;
    let tau = (p.to_seconds(t) - p.to_seconds(birth)).max(0.0);

    // Where and how it is born.
    let e = &ps.emitter;
    let [ex, ey] = ev.vec2(&e.position, birth);
    let [w, h] = e.size;
    let (u1, u2) = (r(1), r(2));
    let local = match e.shape {
        EmitterShape::Point => [0.0, 0.0],
        EmitterShape::Line => [(u1 - 0.5) * w, 0.0],
        EmitterShape::Rect if e.edge => {
            // A point on the border, evenly by length.
            let s = u1 * 2.0 * (w + h);
            if s < w {
                [s - w * 0.5, -h * 0.5]
            } else if s < w + h {
                [w * 0.5, s - w - h * 0.5]
            } else if s < 2.0 * w + h {
                [w * 1.5 + h - s, h * 0.5]
            } else {
                [-w * 0.5, 2.0 * (w + h) - s - h * 0.5]
            }
        }
        EmitterShape::Rect => [(u1 - 0.5) * w, (u2 - 0.5) * h],
        EmitterShape::Circle => {
            let rad = if e.edge { 1.0 } else { u1.sqrt() };
            let a = u2 * std::f64::consts::TAU;
            [a.cos() * w * 0.5 * rad, a.sin() * h * 0.5 * rad]
        }
    };
    let (sa, ca) = e.angle.to_radians().sin_cos();
    let p0 = [ex + local[0] * ca - local[1] * sa, ey + local[0] * sa + local[1] * ca];
    let theta = (ev.num(&ps.direction, birth) + ev.num(&ps.spread, birth) * (r(3) - 0.5)).to_radians();
    let speed = (ev.num(&ps.speed, birth) * (1.0 + ps.speed_random * (2.0 * r(4) - 1.0))).max(0.0);
    let v0 = [speed * theta.cos(), speed * theta.sin()];

    // Motion.
    let g = gravity(&ps.gravity);
    let (dx, v) = motion(v0, g, ps.drag.max(0.0), tau);
    let mut pos = [p0[0] + dx[0], p0[1] + dx[1]];
    let mut vel = v;
    let tb = &ps.turbulence;
    if tb.amount != 0.0 && tb.scale > 0.0 {
        let turb = |pos: [f64; 2], tau: f64| turbulence(tb, ps.seed, p0, pos, tau);
        let d = turb(pos, tau);
        // The velocity of the drift, by a short backward difference.
        let hstep = 1.0 / 240.0;
        if tau > hstep {
            let (dx2, _) = motion(v0, g, ps.drag.max(0.0), tau - hstep);
            let back = turb([p0[0] + dx2[0], p0[1] + dx2[1]], tau - hstep);
            vel = [vel[0] + (d[0] - back[0]) / hstep, vel[1] + (d[1] - back[1]) / hstep];
        }
        pos = [pos[0] + d[0], pos[1] + d[1]];
    }

    // Looks.
    let size_birth = ev.num(&ps.size, birth).max(0.0);
    let factor = (1.0 + ps.size_random * (2.0 * r(5) - 1.0)).max(0.0);
    let size = lerp(size_birth, ps.size_end.unwrap_or(size_birth), age).max(0.0) * factor / 100.0;
    let spin = ps.spin + ps.spin_random * (2.0 * r(6) - 1.0);
    let rotation = r(7) * 360.0 + spin * tau;
    let flip = if ps.sprite == "petal" {
        // A slow tumble: each petal turns about its long axis at its own pace.
        let rate = 0.35 + 0.6 * r(8);
        (r(9) * std::f64::consts::TAU + rate * std::f64::consts::TAU * tau).cos()
    } else {
        1.0
    };
    let c0 = ev.vec3(&ps.color, birth);
    let color = match ps.color_end {
        Some(c1) => [0, 1, 2].map(|i| lerp(c0[i], c1[i], age)),
        None => c0,
    };
    let o0 = ev.num(&ps.opacity, birth);
    let mut opacity = lerp(o0, ps.opacity_end.unwrap_or(o0), age);
    if ps.fade_in > 0.0 {
        opacity *= (age / ps.fade_in).min(1.0);
    }
    if ps.fade_out > 0.0 {
        opacity *= ((1.0 - age) / ps.fade_out).min(1.0);
    }
    Some(Particle { id: (stream, k), birth, age, position: pos, velocity: vel, size, rotation, flip, color, opacity: opacity.clamp(0.0, 1.0) })
}

fn lerp(a: f64, b: f64, u: f64) -> f64 {
    a + (b - a) * u
}

/// Gravity as `[x, y]`: a number pulls down (+y).
fn gravity(g: &Value) -> [f64; 2] {
    match g.to_vec().as_slice() {
        [y] => [0.0, *y],
        [x, y, ..] => [*x, *y],
        [] => [0.0, 0.0],
    }
}

/// Displacement and velocity after `tau` seconds, from velocity `v0` under
/// constant acceleration `g` and linear drag `k`:
/// v(t) = g/k + (v0 - g/k) e^(-k t); without drag, x = v0 t + g t² / 2.
pub fn motion(v0: [f64; 2], g: [f64; 2], k: f64, tau: f64) -> ([f64; 2], [f64; 2]) {
    let kt = k * tau;
    // f = (1 - e^(-k t)) / k and q = (t - f) / k, with their series near k = 0.
    let (f, q, decay) = if kt < 1e-4 {
        (tau - k * tau * tau * 0.5, tau * tau * 0.5 - k * tau * tau * tau / 6.0, 1.0 - kt)
    } else {
        let decay = (-kt).exp();
        let f = (1.0 - decay) / k;
        (f, (tau - f) / k, decay)
    };
    let dx = [v0[0] * f + g[0] * q, v0[1] * f + g[1] * q];
    let v = [v0[0] * decay + g[0] * f, v0[1] * decay + g[1] * f];
    (dx, v)
}

/// The curl-noise drift of a particle born at `p0`, now (without the drift)
/// at `pos`, `tau` seconds after its birth: the curl of a noise potential at
/// its position and the eddies' current phase, minus the same at its birth,
/// so it starts where it is born and wanders by about `amount`.
fn turbulence(tb: &Turbulence, seed: u32, p0: [f64; 2], pos: [f64; 2], tau: f64) -> [f64; 2] {
    let s = tb.scale / 100.0;
    let a = tb.amount / 100.0;
    let now = curl(pos[0] / s, pos[1] / s, tau * tb.speed, seed);
    let then = curl(p0[0] / s, p0[1] / s, 0.0, seed);
    [(now[0] - then[0]) * a, (now[1] - then[1]) * a]
}

/// Curl of the potential: (dψ/dy, -dψ/dx), normalised to about -1..1.
pub fn curl(x: f64, y: f64, z: f64, seed: u32) -> [f64; 2] {
    let e = 1e-3;
    let dy = (potential(x, y + e, z, seed) - potential(x, y - e, z, seed)) / (2.0 * e);
    let dx = (potential(x + e, y, z, seed) - potential(x - e, y, z, seed)) / (2.0 * e);
    [dy * 0.35, -dx * 0.35]
}

/// Two octaves of smooth 3D value noise, about -1..1.
fn potential(x: f64, y: f64, z: f64, seed: u32) -> f64 {
    (value_noise(x, y, z, seed) + 0.5 * value_noise(x * 2.01 + 7.3, y * 2.01 - 3.1, z * 1.7 + 11.0, seed ^ 0x5bd1)) / 1.5
}

fn value_noise(x: f64, y: f64, z: f64, seed: u32) -> f64 {
    let (ix, iy, iz) = (x.floor(), y.floor(), z.floor());
    let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v, w) = (fade(x - ix), fade(y - iy), fade(z - iz));
    let c = |dx: i64, dy: i64, dz: i64| {
        let h = mix64((ix as i64 + dx) as u64 ^ ((iy as i64 + dy) as u64).rotate_left(21) ^ ((iz as i64 + dz) as u64).rotate_left(42) ^ ((seed as u64) << 7));
        (h >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    let l = |a: f64, b: f64, t: f64| a + (b - a) * t;
    l(
        l(l(c(0, 0, 0), c(1, 0, 0), u), l(c(0, 1, 0), c(1, 1, 0), u), v),
        l(l(c(0, 0, 1), c(1, 0, 1), u), l(c(0, 1, 1), c(1, 1, 1), u), v),
        w,
    )
}

fn mix64(mut h: u64) -> u64 {
    h = h.wrapping_add(0x9E37_79B9_7F4A_7C15);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

/// Random number `n` of particle `k` of `stream`, in 0..1.
fn rand(seed: u32, stream: u32, k: u64, n: u32) -> f64 {
    let h = mix64(mix64(mix64(seed as u64 ^ ((stream as u64) << 32)) ^ k) ^ n as u64);
    (h >> 11) as f64 / (1u64 << 53) as f64
}

// ---- serde defaults --------------------------------------------------------

fn zero() -> Prop {
    Prop::Const(Value::Num(0.0))
}
fn is_zero(p: &Prop) -> bool {
    matches!(p, Prop::Const(Value::Num(n)) if *n == 0.0)
}
fn is_zero_f(v: &f64) -> bool {
    *v == 0.0
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn zero_value() -> Value {
    Value::Num(0.0)
}
fn is_zero_value(v: &Value) -> bool {
    v.to_vec().iter().all(|x| *x == 0.0)
}
fn zero2() -> Prop {
    Prop::Const(Value::Vec(vec![0.0, 0.0]))
}
fn zero_size() -> [f64; 2] {
    [0.0, 0.0]
}
fn one() -> Prop {
    Prop::Const(Value::Num(1.0))
}
fn one_f() -> f64 {
    1.0
}
fn white() -> Prop {
    Prop::Const(Value::Vec(vec![1.0, 1.0, 1.0]))
}
fn default_life() -> f64 {
    2.0
}
fn default_speed() -> Prop {
    Prop::Const(Value::Num(0.2))
}
fn default_direction() -> Prop {
    Prop::Const(Value::Num(-90.0))
}
fn default_spread() -> Prop {
    Prop::Const(Value::Num(360.0))
}
fn default_size() -> Prop {
    Prop::Const(Value::Num(1.0))
}
fn default_streak() -> f64 {
    0.05
}
fn default_sprite() -> String {
    "dot".into()
}
fn default_max() -> u32 {
    10_000
}
fn default_turb_scale() -> f64 {
    20.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayerKind, Project};

    fn project(time_base: &str, layer: serde_json::Value) -> Project {
        let doc = serde_json::json!({
            "format": "edits.project/1",
            "settings": { "width": 64, "height": 64, "fps": 25, "duration": 16, "time_base": time_base, "bpm": 120 },
            "layers": [layer]
        });
        serde_json::from_value(doc).unwrap()
    }
    fn ps(p: &Project) -> &Particles {
        match &p.layers[0].kind {
            LayerKind::Particles(ps) => ps,
            _ => unreachable!(),
        }
    }
    fn at(p: &Project, t: f64) -> Vec<Particle> {
        evaluate(&Evaluator::new(p), &p.layers[0], ps(p), t)
    }

    #[test]
    fn the_same_instant_gives_the_same_particles() {
        let p = project("seconds", serde_json::json!({ "id": "p", "kind": "particles", "rate": 40, "life": 1.5, "speed": 0.3, "turbulence": { "amount": 5 }, "spin": 90, "sprite": "petal" }));
        let a = at(&p, 2.37);
        assert!(!a.is_empty());
        assert_eq!(a, at(&p, 2.37));
        // Evaluating other instants in between changes nothing.
        let _ = at(&p, 9.0);
        assert_eq!(a, at(&p, 2.37));
    }

    #[test]
    fn live_count_is_rate_times_life() {
        for (rate, life) in [(10.0, 2.0), (33.0, 0.7), (4.5, 3.0)] {
            let p = project("beats", serde_json::json!({ "id": "p", "kind": "particles", "rate": rate, "life": life }));
            for t in [3.0, 5.1, 7.77, 11.0] {
                let n = at(&p, t).len() as f64;
                assert!((n - rate * life).abs() <= 1.0, "rate {rate} life {life} t {t}: {n}");
            }
            // Before a whole life has passed, only what has been born.
            let n = at(&p, life * 0.5).len() as f64;
            assert!((n - rate * life * 0.5).abs() <= 1.0, "{n}");
        }
        // A keyed rate: as many as its integral over the last life.
        let p = project("seconds", serde_json::json!({ "id": "p", "kind": "particles", "life": 1,
            "rate": { "keys": [ { "t": 0, "v": 0 }, { "t": 4, "v": 40 } ] } }));
        let n = at(&p, 3.0).len() as f64;
        // Integral of 10 u over [2, 3] = 25.
        assert!((n - 25.0).abs() <= 1.5, "{n}");
        // Prewarm: full from the start.
        let p = project("seconds", serde_json::json!({ "id": "p", "kind": "particles", "in": 2, "rate": 10, "life": 2, "prewarm": true }));
        assert!((at(&p, 2.0).len() as f64 - 20.0).abs() <= 1.0);
    }

    #[test]
    fn a_burst_appears_on_its_beat_and_not_a_frame_before() {
        let p = project("beats", serde_json::json!({ "id": "p", "kind": "particles", "bursts": [ { "t": 6, "count": 30 } ], "life": 2 }));
        let frame = p.frame_len();
        assert_eq!(at(&p, 6.0).len(), 30);
        assert_eq!(at(&p, 6.0 - frame).len(), 0);
        assert_eq!(at(&p, 7.9).len(), 30);
        assert_eq!(at(&p, 8.0).len(), 0);
        // On the frame that starts at beat 6 (frame 75 at 120 BPM, 25 fps).
        assert_eq!(at(&p, p.frame_time(75)).len(), 30);
        assert_eq!(at(&p, p.frame_time(74)).len(), 0);
    }

    #[test]
    fn gravity_moves_the_centroid_down_by_half_g_t_squared() {
        let p = project("seconds", serde_json::json!({ "id": "p", "kind": "particles", "bursts": [ { "t": 1, "count": 50 } ], "life": 5,
            "speed": 0, "gravity": 0.4, "emitter": { "shape": "rect", "size": [0.5, 0.2], "position": [0.1, -0.3] } }));
        for tau in [0.0, 0.5, 1.2, 2.0] {
            let list = at(&p, 1.0 + tau);
            let cy = list.iter().map(|q| q.position[1]).sum::<f64>() / list.len() as f64;
            let c0 = at(&p, 1.0).iter().map(|q| q.position[1]).sum::<f64>() / 50.0;
            assert!((cy - c0 - 0.5 * 0.4 * tau * tau).abs() < 1e-9, "tau {tau}: {}", cy - c0);
        }
        // With drag, the speed tends to g / k.
        let (_, v) = motion([0.0, 0.0], [0.0, 1.0], 2.0, 20.0);
        assert!((v[1] - 0.5).abs() < 1e-6);
        // The series and the exact form agree near k = 0.
        let (a, _) = motion([0.3, -0.2], [0.1, 0.5], 1e-6, 1.0);
        let (b, _) = motion([0.3, -0.2], [0.1, 0.5], 0.0, 1.0);
        assert!((a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6);
    }

    #[test]
    fn the_seed_changes_the_particles_and_a_burst_spreads_by_speed() {
        let doc = |seed: u32| serde_json::json!({ "id": "p", "kind": "particles", "bursts": [ { "t": 0, "count": 20 } ], "speed": 0.5, "seed": seed });
        let (a, b) = (project("seconds", doc(1)), project("seconds", doc(2)));
        assert_ne!(at(&a, 0.5), at(&b, 0.5));
        // Every particle of a 360-degree burst is speed x t from the centre.
        for q in at(&a, 0.5) {
            assert!((q.position[0].hypot(q.position[1]) - 0.25).abs() < 1e-9);
        }
    }

    #[test]
    fn the_layer_round_trips_and_checks() {
        let p = project("seconds", serde_json::json!({ "id": "p", "kind": "particles", "rate": -1, "life": 0, "sprite": "slot:nope" }));
        let text = p.to_json();
        assert!(text.contains("\"kind\": \"particles\""), "{text}");
        let f = crate::check::check(&p, None);
        let says = |path: &str, needle: &str| f.iter().any(|x| x.path == path && x.message.contains(needle) && x.severity == Severity::Error);
        assert!(says("layers[0:\"p\"].rate", "rate must be 0 or more"), "{f:#?}");
        assert!(says("layers[0:\"p\"].life", "life must be positive"));
        assert!(says("layers[0:\"p\"].sprite", "no slot \"nope\""));
        let p = project("seconds", serde_json::json!({ "id": "p", "kind": "particles", "rate": 5, "sprite": "heart" }));
        let f = crate::check::check(&p, None);
        assert!(f.iter().any(|x| x.message.contains("unknown sprite \"heart\"") && x.hint.as_deref().unwrap().contains("petal")), "{f:#?}");
        let d = crate::describe::describe(&p);
        assert!(d.contains("particles heart") && d.contains("rate 5"), "{d}");
    }
}
