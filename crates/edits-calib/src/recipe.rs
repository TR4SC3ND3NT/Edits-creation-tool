//! Random but plausible recipes of this genre, as `edits_core::Project`
//! values with known parameters, and the truth they imply frame by frame.
//!
//! The truth is read back from the project with `edits_core::Evaluator`, the
//! same code the renderer evaluates, so "truth" is exactly what was rendered.

use crate::pictures::{self, Kind, KINDS};
use crate::rng::Rng;
use anyhow::{Context, Result};
use edits_core::{Anim, Blend, Camera, Ease, Effect, Evaluator, Fit, Key, Layer, LayerKind, Modifier, MotionBlur, Project, Prop, Settings, Slot, TimeBase, Transform, Value, FORMAT};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// What makes a case: everything else follows from these numbers.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CaseSpec {
    pub seed: u64,
    /// Height in pixels of the rendered (and analysed) frames.
    pub height: u32,
    /// Include a click track, so tempo and beat phase are estimated from audio.
    pub audio: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PictureSpec {
    pub slot: String,
    pub kind: Kind,
    pub seed: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShotTruth {
    pub f0: usize,
    pub f1: usize,
    pub layer: String,
    pub slot: String,
    pub kind: Kind,
    /// `none`, `punch`, `slide`, `push`, `punch_rotate`.
    pub motion: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FlashTruth {
    pub shot: usize,
    /// First frame of the flash.
    pub frame: usize,
    /// Key value, and the value the peak frame actually shows (the key sits a fraction of a frame earlier).
    pub stops: f64,
    pub visible: f64,
    /// `decay` rate of the fall, and its length in frames.
    pub rate: f64,
    pub frames: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HitTruth {
    pub shot: usize,
    /// `saturation`, `invert` or `threshold`.
    pub kind: String,
    pub f0: usize,
    pub f1: usize,
    /// Saturation amount, 1 for an inversion, the level of a threshold.
    pub value: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlurTruth {
    pub shot: usize,
    pub frame: usize,
    /// Radius at the key, % of height, and the number of frames it clears over.
    pub radius: f64,
    pub frames: usize,
    pub ease: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct FrameTruth {
    pub shot: usize,
    pub stops: f64,
    pub blur: f64,
    pub sat: f64,
    pub invert: f64,
    pub threshold: Option<f64>,
    pub scale: f64,
    pub x: f64,
    pub y: f64,
    pub rot: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Truth {
    pub fps: f64,
    pub bpm: f64,
    pub beat_offset: f64,
    pub frames: usize,
    pub width: u32,
    pub height: u32,
    pub shots: Vec<ShotTruth>,
    pub flashes: Vec<FlashTruth>,
    pub hits: Vec<HitTruth>,
    pub blurs: Vec<BlurTruth>,
    /// amount, radius, softness.
    pub vignette: Option<[f64; 3]>,
    pub overlay: bool,
    /// Shake amplitude in frame heights, if the recipe shakes.
    pub shake: Option<f64>,
    /// Per output frame; `None` before the first shot (black lead-in).
    pub frame: Vec<Option<FrameTruth>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Case {
    pub spec: CaseSpec,
    pub pictures: Vec<PictureSpec>,
    pub overlay_seed: u64,
    pub project: Project,
    pub truth: Truth,
}

fn round(v: f64, d: i32) -> f64 {
    let m = 10f64.powi(d);
    (v * m).round() / m
}

pub fn decay(rate: f64) -> Ease {
    let mut args = BTreeMap::new();
    args.insert("rate".to_string(), round(rate, 3));
    Ease::Params { name: "decay".into(), args }
}

fn named(n: &str) -> Ease {
    Ease::Name(n.into())
}

fn key(t: f64, v: Value, ease: Ease) -> Key {
    Key { t: round(t, 5), v, ease }
}

fn keyed(keys: Vec<Key>) -> Prop {
    Prop::Anim(Anim { keys, base: None, modifiers: vec![] })
}

/// First output frame whose start time is at or after project time `t`.
pub fn first_frame(p: &Project, t: f64) -> usize {
    (p.to_seconds(t) * p.settings.fps - 1e-6).ceil().max(0.0) as usize
}

fn effect(kind: &str, params: Vec<(&str, Prop)>) -> Effect {
    Effect { kind: kind.into(), params: params.into_iter().map(|(k, v)| (k.to_string(), v)).collect(), source: None, enabled: true }
}

/// Smallest uniform scale multiplier that keeps the frame covered by the
/// layer through its move (sampled at every frame of the layer).
fn cover_multiplier(p: &Project, layer: &Layer) -> f64 {
    let ev = Evaluator::new(p);
    let aspect = p.settings.width as f64 / p.settings.height as f64;
    let (f0, f1) = (first_frame(p, layer.start), first_frame(p, layer.end.unwrap_or(p.settings.duration)));
    let mut m: f64 = 1.0;
    for f in f0..f1.max(f0 + 1) {
        for sub in 0..2 {
            let t = p.frame_time(f as u64) + 0.5 * sub as f64 * p.frame_len();
            let l = ev.layer(layer, t);
            let (s, th) = (l.scale[0].max(1e-3), l.rotation[2].to_radians());
            for (cx, cy) in [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)] {
                let (dx, dy) = (cx * aspect - l.position[0], cy - l.position[1]);
                let (lx, ly) = ((dx * th.cos() + dy * th.sin()) / s, (-dx * th.sin() + dy * th.cos()) / s);
                m = m.max(lx.abs() / (aspect / 2.0)).max(ly.abs() / 0.5);
            }
        }
    }
    m
}

fn scale_keys(prop: &mut Prop, m: f64) {
    match prop {
        Prop::Const(Value::Num(v)) => *v = round(*v * m, 4),
        Prop::Anim(a) => {
            for k in a.keys.iter_mut() {
                if let Value::Num(v) = &mut k.v {
                    *v = round(*v * m, 4);
                }
            }
        }
        _ => {}
    }
}

/// Generate a case from its spec. Deterministic.
pub fn generate(spec: &CaseSpec) -> Case {
    let mut r = Rng::new(spec.seed);
    let fps = if r.chance(0.7) { 25.0 } else { 30.0 };
    let bpm = r.int(100, 150) as f64;
    let beat_offset = round(r.range(0.08, 0.36), 3);
    let height = (spec.height.max(64) + 1) & !1;
    let aspect = *r.pick(&[1.0, 1.0, 1.0, 0.8, 16.0 / 9.0]);
    let width = ((height as f64 * aspect / 2.0).round() as u32) * 2;
    let fl = bpm / (60.0 * fps); // one frame in beats

    // Shots.
    let n_shots = r.int(6, 8) as usize;
    let lengths: Vec<f64> = (0..n_shots).map(|_| *r.pick(&[1.0, 1.0, 1.0, 0.5, 2.0])).collect();
    let duration: f64 = lengths.iter().sum();
    let n_pics = r.int(4, 5) as usize;
    let first_kind = r.int(0, 3) as usize;
    let pictures: Vec<PictureSpec> = (0..n_pics).map(|i| PictureSpec { slot: format!("pic{}", i + 1), kind: KINDS[(first_kind + i) % 4], seed: r.next_u64() >> 16 }).collect();
    let mut order: Vec<usize> = Vec::new();
    for i in 0..n_shots {
        let pic = if i < n_pics {
            i
        } else {
            let mut k = r.int(0, n_pics as i64 - 1) as usize;
            if k == order[i - 1] {
                k = (k + 1) % n_pics;
            }
            k
        };
        order.push(pic);
    }
    let shake = if r.chance(0.2) { Some((round(r.range(0.003, 0.008), 4), round(r.range(8.0, 14.0), 1))) } else { None };
    let overlay = r.chance(0.6);
    let vignette = if r.chance(0.7) { Some([round(r.range(0.25, 0.7), 2), round(r.range(0.3, 0.7), 2), round(r.range(0.3, 0.6), 2)]) } else { None };

    let mut project = Project {
        format: FORMAT.into(),
        settings: Settings { width, height, fps, duration, time_base: TimeBase::Beats, bpm: Some(bpm), beat_offset, background: [0.0; 3], motion_blur: MotionBlur::default() },
        audio: None,
        slots: BTreeMap::new(),
        camera: Camera::default(),
        layers: vec![],
        markers: vec![],
    };
    for p in &pictures {
        project.slots.insert(p.slot.clone(), Slot { path: Some(format!("img/{}.png", p.slot)), fit: Fit::Cover, focus: [0.5, 0.5], note: Some(format!("generated {} picture", p.kind.name())) });
    }

    let mut flashes: Vec<FlashTruth> = Vec::new();
    let mut blurs: Vec<BlurTruth> = Vec::new();
    let mut motions: Vec<String> = Vec::new();
    let mut b0 = 0.0;
    for (i, len) in lengths.iter().enumerate() {
        let b1 = b0 + len;
        let shot_frames = (len / fl).floor() as i64;
        let mut tr = Transform::default();
        // ---- motion ---------------------------------------------------------
        let move_ease = |r: &mut Rng| match r.int(0, 4) {
            0 => named("out"),
            1 => named("expo_out"),
            _ => decay(r.range(0.25, 0.5)),
        };
        let d = f64::min(*r.pick(&[0.5, 0.75, 1.0]), *len);
        let motion = *r.pick(&["none", "none", "punch", "punch", "punch", "punch", "slide", "slide", "push", "punch_rotate"]);
        match motion {
            "punch" | "punch_rotate" => {
                let z = round(r.range(1.08, 1.4), 3);
                tr.scale = keyed(vec![key(b0, Value::Num(z), move_ease(&mut r)), key(b0 + d, Value::Num(1.0), named("linear"))]);
                if motion == "punch_rotate" {
                    let deg = round(r.sign() * r.range(1.0, 4.0), 2);
                    tr.rotation = keyed(vec![key(b0, Value::Vec(vec![0.0, 0.0, deg]), move_ease(&mut r)), key(b0 + d, Value::Vec(vec![0.0, 0.0, 0.0]), named("linear"))]);
                }
            }
            "slide" => {
                let (dx, dy) = if r.chance(0.5) { (r.sign() * r.range(0.04, 0.12), r.range(-0.02, 0.02)) } else { (r.range(-0.02, 0.02), r.sign() * r.range(0.04, 0.12)) };
                tr.position = keyed(vec![key(b0, Value::Vec(vec![round(dx, 4), round(dy, 4), 0.0]), move_ease(&mut r)), key(b0 + d, Value::Vec(vec![0.0, 0.0, 0.0]), named("linear"))]);
            }
            "push" => {
                let z = round(r.range(1.04, 1.08), 3);
                tr.scale = keyed(vec![key(b0, Value::Num(1.0), named("linear")), key(b1, Value::Num(z), named("linear"))]);
            }
            _ => {}
        }
        if let Some((amp, freq)) = shake {
            let modifier = Modifier::Shake { amplitude: Prop::Const(Value::Vec(vec![amp, amp, 0.0])), frequency: freq, seed: i as u32 + 1 };
            match &mut tr.position {
                Prop::Anim(a) => a.modifiers.push(modifier),
                other => *other = Prop::Anim(Anim { keys: vec![], base: Some(Value::Vec(vec![0.0, 0.0, 0.0])), modifiers: vec![modifier] }),
            }
        }
        motions.push(motion.to_string());

        // ---- effects: blur, saturation, invert, threshold, exposure -----------
        let mut effects: Vec<Effect> = Vec::new();
        if r.chance(0.35) && shot_frames >= 6 {
            let radius = round(r.range(0.5, 3.0), 2);
            let m = r.int(3, 8).min(shot_frames - 2);
            let (ease, name) = match r.int(0, 2) {
                0 => (named("linear"), "linear"),
                1 => (named("out"), "out"),
                _ => (decay(r.range(0.3, 0.5)), "decay"),
            };
            effects.push(effect("gaussian_blur", vec![("radius", keyed(vec![key(b0, Value::Num(radius), ease), key(b0 + m as f64 * fl, Value::Num(0.0), named("hold"))]))]));
            blurs.push(BlurTruth { shot: i, frame: 0, radius, frames: m as usize, ease: name.into() });
        }
        if r.chance(0.5) && shot_frames >= 7 {
            let n = r.int(1, 6).min(shot_frames - 5) as f64;
            let quarters = (len * 4.0).round() as i64;
            let mut q = if r.chance(0.25) { 0 } else { r.int(1, (quarters - 1).max(1)) };
            // Leave at least two ordinary frames after the hit.
            while q > 0 && (q as f64 * 0.25) / fl + n + 2.0 > shot_frames as f64 {
                q -= 1;
            }
            let (t0, t1) = (b0 + q as f64 * 0.25, b0 + q as f64 * 0.25 + n * fl);
            let step = |neutral: f64, value: f64| {
                let mut keys = Vec::new();
                if q > 0 {
                    keys.push(key(b0, Value::Num(neutral), named("hold")));
                }
                keys.push(key(t0, Value::Num(value), named("hold")));
                keys.push(key(t1, Value::Num(neutral), named("hold")));
                keyed(keys)
            };
            match r.int(0, 3) {
                0 | 1 => {
                    let amount = if r.chance(0.75) { 0.0 } else { round(r.range(0.25, 0.5), 2) };
                    effects.push(effect("saturation", vec![("amount", step(1.0, amount))]));
                }
                2 => effects.push(effect("invert", vec![("amount", step(0.0, 1.0))])),
                _ => {
                    let level = round(r.range(0.35, 0.6), 2);
                    effects.push(effect("threshold", vec![("amount", step(0.0, 1.0)), ("level", Prop::Const(Value::Num(level)))]));
                }
            }
        }
        // Flashes: one on the cut, sometimes a second one half way.
        let mut times: Vec<f64> = Vec::new();
        if r.chance(0.65) {
            times.push(b0);
        }
        if *len >= 1.0 && r.chance(0.2) {
            times.push(b0 + len / 2.0);
        }
        if !times.is_empty() {
            let mut keys: Vec<Key> = Vec::new();
            if times[0] > b0 {
                keys.push(key(b0, Value::Num(0.0), named("hold")));
            }
            for (k, t) in times.iter().enumerate() {
                let stops = round(r.range(1.0, 5.0), 2);
                let rate = round(r.range(0.15, 0.6), 3);
                let limit = times.get(k + 1).map(|n| n - 1e-3).unwrap_or(b1);
                let end = f64::min(t + *r.pick(&[0.5f64, 1.0]), limit);
                keys.push(key(*t, Value::Num(stops), decay(rate)));
                keys.push(key(end, Value::Num(0.0), named("hold")));
                flashes.push(FlashTruth { shot: i, frame: 0, stops, visible: 0.0, rate, frames: (end - t) / fl });
            }
            // Store the key time in `frame` for now; resolved to frames below.
            let n = flashes.len();
            for (k, t) in times.iter().enumerate() {
                flashes[n - times.len() + k].visible = *t;
            }
            effects.push(effect("exposure", vec![("stops", keyed(keys))]));
        }
        project.layers.push(Layer {
            id: format!("shot{:02}", i + 1),
            kind: LayerKind::Image { slot: pictures[order[i]].slot.clone() },
            name: None,
            start: round(b0, 5),
            end: Some(round(b1, 5)),
            transform: tr,
            blend: Blend::Normal,
            effects,
            enabled: true, transition: None,
        });
        b0 = b1;
    }
    // Keep the frame covered through every move.
    for i in 0..project.layers.len() {
        let m = cover_multiplier(&project, &project.layers[i]);
        if m > 1.0 {
            scale_keys(&mut project.layers[i].transform.scale, (m * 1.004 * 1000.0).ceil() / 1000.0);
        }
    }
    let overlay_seed = r.next_u64() >> 16;
    if overlay {
        project.slots.insert("overlay".into(), Slot { path: Some("img/overlay.png".into()), fit: Fit::Stretch, focus: [0.5, 0.5], note: None });
        project.layers.push(Layer {
            id: "overlay".into(),
            kind: LayerKind::Image { slot: "overlay".into() },
            name: None,
            start: 0.0,
            end: None,
            transform: Transform::default(),
            blend: Blend::Screen,
            effects: vec![],
            enabled: true, transition: None,
        });
    }
    if let Some(v) = vignette {
        project.layers.push(Layer {
            id: "finish".into(),
            kind: LayerKind::Adjustment,
            name: None,
            start: 0.0,
            end: None,
            transform: Transform::default(),
            blend: Blend::Normal,
            effects: vec![effect("vignette", vec![("amount", v[0].into()), ("radius", v[1].into()), ("softness", v[2].into())])],
            enabled: true, transition: None,
        });
    }

    let mut truth = truth_of(&project);
    for (i, s) in truth.shots.iter_mut().enumerate() {
        s.kind = pictures[order[i]].kind;
        s.motion = motions[i].clone();
    }
    // Resolve event times to frames.
    for f in flashes.iter_mut() {
        let frame = first_frame(&project, f.visible);
        f.frame = frame;
        f.visible = truth.frame.get(frame).copied().flatten().map(|t| t.stops).unwrap_or(f.stops);
    }
    for b in blurs.iter_mut() {
        b.frame = truth.shots[b.shot].f0;
    }
    truth.flashes = flashes;
    truth.blurs = blurs;
    truth.vignette = vignette;
    truth.overlay = overlay;
    truth.shake = shake.map(|s| s.0);
    Case { spec: spec.clone(), pictures, overlay_seed, project, truth }
}

/// Is this a layer that shows one shot (an image layer limited in time)?
pub fn is_shot_layer(p: &Project, l: &Layer) -> bool {
    matches!(l.kind, LayerKind::Image { .. }) && l.enabled && (l.start > 1e-9 || l.end.map(|e| e < p.settings.duration - 1e-9).unwrap_or(false)) && l.blend == Blend::Normal
}

/// What a project shows frame by frame: the shot layer on screen and the
/// values of its transform and effects, evaluated as the renderer does.
pub fn truth_of(project: &Project) -> Truth {
    let ev = Evaluator::new(project);
    let s = &project.settings;
    let n = project.frame_count() as usize;
    let shot_layers: Vec<&Layer> = project.layers.iter().filter(|l| is_shot_layer(project, l)).collect();
    let mut frame: Vec<Option<FrameTruth>> = Vec::with_capacity(n);
    let mut shots: Vec<ShotTruth> = Vec::new();
    for f in 0..n {
        let t = project.frame_time(f as u64);
        // The topmost active shot layer is the one on screen.
        let Some((li, layer)) = shot_layers.iter().enumerate().rev().find(|(_, l)| ev.active(l, t)) else {
            frame.push(None);
            continue;
        };
        if shots.last().map(|s: &ShotTruth| s.layer != layer.id).unwrap_or(true) {
            let slot = match &layer.kind {
                LayerKind::Image { slot } => slot.clone(),
                _ => String::new(),
            };
            shots.push(ShotTruth { f0: f, f1: f, layer: layer.id.clone(), slot, kind: Kind::Poster, motion: String::new() });
        }
        let _ = li;
        let shot = shots.len() - 1;
        shots[shot].f1 = f + 1;
        let l = ev.layer(layer, t);
        let mut ft = FrameTruth { shot, stops: 0.0, blur: 0.0, sat: 1.0, invert: 0.0, threshold: None, scale: l.scale[0], x: l.position[0], y: l.position[1], rot: l.rotation[2] };
        for e in layer.effects.iter().filter(|e| e.enabled) {
            let re = ev.effect(e, t);
            match e.kind.as_str() {
                "exposure" => ft.stops += re.num("stops"),
                "gaussian_blur" => ft.blur = (ft.blur * ft.blur + re.num("radius").powi(2)).sqrt(),
                "saturation" => ft.sat *= re.num("amount"),
                "invert" => ft.invert = re.num("amount"),
                "threshold" if re.num("amount") > 0.5 => ft.threshold = Some(re.num("level")),
                _ => {}
            }
        }
        frame.push(Some(ft));
    }
    // Tone hits as runs of frames.
    let mut hits: Vec<HitTruth> = Vec::new();
    for (si, sh) in shots.iter().enumerate() {
        let get = |f: usize| frame[f].unwrap();
        let mut run = |kind: &str, on: &dyn Fn(&FrameTruth) -> bool, value: &dyn Fn(&FrameTruth) -> f64| {
            let mut f = sh.f0;
            while f < sh.f1 {
                if on(&get(f)) {
                    let a = f;
                    while f < sh.f1 && on(&get(f)) {
                        f += 1;
                    }
                    hits.push(HitTruth { shot: si, kind: kind.into(), f0: a, f1: f, value: value(&get(a)) });
                } else {
                    f += 1;
                }
            }
        };
        run("saturation", &|t| t.sat < 0.6, &|t| t.sat);
        run("invert", &|t| t.invert > 0.5, &|_| 1.0);
        run("threshold", &|t| t.threshold.is_some(), &|t| t.threshold.unwrap_or(0.5));
    }
    let vignette = project.layers.iter().filter(|l| l.enabled).flat_map(|l| l.effects.iter()).find(|e| e.kind == "vignette" && e.enabled).map(|e| {
        let re = ev.effect(e, 0.0);
        [re.num("amount"), re.num("radius"), re.num("softness")]
    });
    Truth {
        fps: s.fps,
        bpm: s.bpm.unwrap_or(0.0),
        beat_offset: s.beat_offset,
        frames: n,
        width: s.width,
        height: s.height,
        shots,
        flashes: vec![],
        hits,
        blurs: vec![],
        vignette,
        overlay: project.layers.iter().any(|l| l.enabled && l.blend == Blend::Screen && matches!(l.kind, LayerKind::Image { .. })),
        shake: None,
        frame,
    }
}

impl Case {
    /// Write the generated pictures (and the overlay plate) under `dir/img/`.
    pub fn write_media(&self, dir: &Path) -> Result<()> {
        let img = dir.join("img");
        std::fs::create_dir_all(&img).with_context(|| format!("creating {}", img.display()))?;
        let size = ((self.project.settings.height as f64 * 1.25) as u32).max(96);
        for p in &self.pictures {
            pictures::generate(p.kind, p.seed, size).save(img.join(format!("{}.png", p.slot)))?;
        }
        if self.truth.overlay {
            pictures::overlay(self.overlay_seed, self.project.settings.width, self.project.settings.height).save(img.join("overlay.png"))?;
        }
        Ok(())
    }

    /// A click on every beat (a decaying low tone), mono at `sr` Hz.
    pub fn click_track(&self, sr: u32) -> Vec<f32> {
        let t = &self.truth;
        let n = (t.frames as f64 / t.fps * sr as f64) as usize;
        let mut s = vec![0.0f32; n];
        let len = (0.07 * sr as f64) as usize;
        let mut k = 0;
        loop {
            let i0 = ((t.beat_offset + k as f64 * 60.0 / t.bpm) * sr as f64) as usize;
            if i0 + len >= n {
                break;
            }
            for j in 0..len {
                s[i0 + j] += 0.8 * (-(j as f32) / (0.011 * sr as f32)).exp() * (std::f32::consts::TAU * 110.0 * j as f32 / sr as f32).sin();
            }
            k += 1;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases_are_deterministic_valid_and_covered() {
        for seed in 0..12 {
            let spec = CaseSpec { seed, height: 180, audio: true };
            let a = generate(&spec);
            let b = generate(&spec);
            assert_eq!(a.project.to_json(), b.project.to_json());
            let findings = edits_core::check::check(&a.project, None);
            assert!(!edits_core::check::has_errors(&findings), "seed {seed}: {findings:?}");
            // Shots tile the project after the lead-in, and every shot layer covers the frame.
            assert!(a.truth.shots.len() >= 6);
            for w in a.truth.shots.windows(2) {
                assert_eq!(w[0].f1, w[1].f0);
            }
            for l in a.project.layers.iter().filter(|l| is_shot_layer(&a.project, l)) {
                assert!(cover_multiplier(&a.project, l) <= 1.0 + 1e-9, "seed {seed} layer {} does not cover the frame", l.id);
            }
            // The flash is keyed on a beat and the first frame that shows it
            // starts up to one frame later (the beat grid and the frame grid
            // are not aligned), so the peak frame shows the key value after
            // at most one frame of the `decay` ease: at least
            // `stops * (k - k^N) / (1 - k^N)` with `k = 1 - rate`. (This
            // used to require half the key, which a fast decay keyed late in
            // a frame does not reach: seed 4 has rate 0.56, visible 0.48x.)
            for f in &a.truth.flashes {
                let k = 1.0 - f.rate;
                let one_frame = (k - k.powf(f.frames)) / (1.0 - k.powf(f.frames));
                assert!(f.visible >= f.stops * one_frame - 1e-6 && f.visible <= f.stops + 1e-9, "seed {seed}: {f:?}");
            }
        }
    }
}
