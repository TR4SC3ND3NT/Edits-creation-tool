//! Turn a reference analysis into an `edits.project/1` project that replays
//! the recipe. The result is a deterministic baseline for an agent to edit.
//!
//! Choices (documented in `docs/ANALYSIS.md`):
//! - the project covers the whole reference: beat 0 sits at the measured
//!   offset, the audio is the reference video itself with offset 0;
//! - one image layer per shot; exposure flashes, blur and tone hits are
//!   effects on that shot's layer (the measurements are per shot), in the
//!   order blur -> saturation -> invert -> threshold -> exposure;
//! - a looping edit is written out once per repeat, so the draft has the
//!   reference's full length;
//! - the overlay plate is a `screen` image layer on top, the vignette an
//!   adjustment layer above it;
//! - scale is known only relative to the shot's reference frame, so each
//!   shot's scale is multiplied by the smallest factor that keeps the frame
//!   covered through its move;
//! - shots flagged `needs_agent` become a plain image layer and a marker.

use crate::schema::*;
use edits_core::{ease, Anim, Audio, Blend, Camera, Effect, Fit, Key, Layer, LayerKind, Marker, MotionBlur, Project, Prop, Settings, Slot, TimeBase, Transform, Value, FORMAT};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub struct DraftOpts {
    /// Point the slots at the proxy stills so the draft renders immediately.
    pub proxies: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Put key and cut times on the quarter-beat grid when that moves them by at most one frame.
    pub snap: bool,
    /// Write every repeat of a looping edit (otherwise only the first period).
    pub repeats: bool,
    /// Directory holding `reference_analysis.json`, `overlay.png` and `proxies/`.
    pub analysis_dir: PathBuf,
    /// Directory the project file will live in (paths are made relative to it).
    pub project_dir: PathBuf,
}

fn absolute(p: &Path) -> PathBuf {
    let p = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `target` as a path relative to `from_dir` (forward slashes).
pub fn relative_path(from_dir: &Path, target: &Path) -> String {
    let (from, to) = (absolute(from_dir), absolute(target));
    let (a, b): (Vec<_>, Vec<_>) = (from.components().collect(), to.components().collect());
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".to_string(); a.len() - common];
    parts.extend(b[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

struct Clock<'a> {
    beats: &'a Beats,
    fps: f64,
    snap: bool,
}

impl Clock<'_> {
    /// Project time at which something first seen on `frame` should be keyed.
    fn time(&self, frame: usize) -> f64 {
        if self.snap {
            if let Some(q) = self.beats.snap(frame as f64, self.fps) {
                return q;
            }
        }
        // A hair before the frame's own time, so rounding cannot push it to the next frame.
        ((self.beats.beat_of_frame(frame as f64, self.fps) - 0.0005) * 1e4).floor() / 1e4
    }
    fn exact(&self, frame: usize) -> f64 {
        ((self.beats.beat_of_frame(frame as f64, self.fps) - 0.0005) * 1e4).floor() / 1e4
    }
    fn frame_len(&self) -> f64 {
        1.0 / self.beats.frames_per_beat.max(1e-6)
    }
    /// Strictly ascending times for a list of ascending frames.
    fn times(&self, frames: &[usize]) -> Vec<f64> {
        let mut t: Vec<f64> = frames.iter().map(|f| self.time(*f)).collect();
        for k in 1..t.len() {
            if t[k] <= t[k - 1] + 1e-6 {
                t[k] = self.exact(frames[k]).max(t[k - 1] + 0.25 * self.frame_len());
            }
        }
        t
    }
}

/// Per-frame values of a fitted curve over frames `f0..f1`, evaluated as the renderer does.
pub fn sample_curve(c: &Curve, f0: usize, f1: usize) -> Vec<Vec<f64>> {
    let keys = &c.keys;
    (f0..f1)
        .map(|f| {
            if keys.len() == 1 || f <= keys[0].frame {
                return keys[0].v.clone();
            }
            let last = &keys[keys.len() - 1];
            if f >= last.frame {
                return last.v.clone();
            }
            let s = (0..keys.len() - 1).rev().find(|s| keys[*s].frame <= f).unwrap_or(0);
            let (k0, k1) = (&keys[s], &keys[s + 1]);
            let len = (k1.frame - k0.frame) as f64;
            let u = ease::apply(&k0.ease, (f - k0.frame) as f64 / len, len);
            k0.v.iter().zip(&k1.v).map(|(a, b)| a + (b - a) * u).collect()
        })
        .collect()
}

/// Smallest scale multiplier that keeps the frame covered by a `cover`-fitted
/// picture through the shot's move.
fn cover_factor(shot: &Shot, aspect: f64) -> f64 {
    let get = |name: &str| shot.curves.iter().find(|c| c.prop == name).map(|c| sample_curve(c, shot.f0, shot.f1));
    let (scale, pos, rot) = (get("scale"), get("position"), get("rotation"));
    let mut m: f64 = 1.0;
    for i in 0..shot.f1 - shot.f0 {
        let s = scale.as_ref().map(|v| v[i][0]).unwrap_or(1.0).max(1e-3);
        let (x, y) = pos.as_ref().map(|v| (v[i][0], v[i][1])).unwrap_or((0.0, 0.0));
        let th = rot.as_ref().map(|v| v[i][0]).unwrap_or(0.0).to_radians();
        for (cx, cy) in [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)] {
            // Frame corner in the picture's own axes.
            let (dx, dy) = (cx * aspect - x, cy - y);
            let (lx, ly) = ((dx * th.cos() + dy * th.sin()) / s, (-dx * th.sin() + dy * th.cos()) / s);
            m = m.max(lx.abs() / (aspect / 2.0)).max(ly.abs() / 0.5);
        }
    }
    (m.min(4.0) * 1000.0).ceil() / 1000.0
}

fn anim(times: &[f64], keys: &[CurveKey], value: impl Fn(&CurveKey) -> Value) -> Prop {
    if keys.len() == 1 {
        return Prop::Const(value(&keys[0]));
    }
    Prop::Anim(Anim {
        keys: keys.iter().zip(times).map(|(k, t)| Key { t: *t, v: value(k), ease: k.ease.clone() }).collect(),
        base: None,
        modifiers: vec![],
    })
}

const EFFECT_ORDER: [&str; 5] = ["gaussian_blur", "saturation", "invert", "threshold", "exposure"];

fn shot_layer(shot: &Shot, clock: &Clock, shift: usize, repeat: usize, aspect: f64) -> Layer {
    let key_times = |c: &Curve| clock.times(&c.keys.iter().map(|k| k.frame + shift).collect::<Vec<_>>());
    let mut transform = Transform::default();
    if shot.status == Ident::Measured {
        let m = cover_factor(shot, aspect);
        let mut has_scale = false;
        for c in &shot.curves {
            let t = key_times(c);
            match c.prop.as_str() {
                "scale" => {
                    has_scale = true;
                    transform.scale = anim(&t, &c.keys, |k| Value::Num(round_to(k.v[0] * m, 4)));
                }
                "position" => transform.position = anim(&t, &c.keys, |k| Value::Vec(vec![k.v[0], k.v[1], 0.0])),
                "rotation" => transform.rotation = anim(&t, &c.keys, |k| Value::Vec(vec![0.0, 0.0, k.v[0]])),
                _ => {}
            }
        }
        if !has_scale && m > 1.0 {
            transform.scale = Prop::Const(Value::Num(m));
        }
    }
    let mut effects: Vec<Effect> = Vec::new();
    for kind in EFFECT_ORDER {
        let mut params: BTreeMap<String, Prop> = BTreeMap::new();
        for c in &shot.curves {
            if let Some((k, p)) = c.prop.split_once('.') {
                if k == kind {
                    params.insert(p.to_string(), anim(&key_times(c), &c.keys, |key| Value::Num(key.v[0])));
                }
            }
        }
        if !params.is_empty() {
            effects.push(Effect { kind: kind.into(), params, source: None, enabled: true });
        }
    }
    let suffix = if repeat == 0 { String::new() } else { format!("_r{}", repeat + 1) };
    Layer {
        id: format!("shot{:02}{suffix}", shot.id),
        kind: LayerKind::Image { slot: shot.slot.clone() },
        name: Some(format!(
            "shot {} · frames {}-{} · {}",
            shot.id,
            shot.f0 + shift,
            shot.f1 + shift,
            if shot.status == Ident::Measured { "measured" } else { "needs an agent: plain layer" }
        )),
        start: clock.time(shot.f0 + shift),
        end: Some(clock.time(shot.f1 + shift)),
        transform,
        blend: Blend::Normal,
        effects,
        enabled: true,
    }
}

/// Build the project.
pub fn draft(a: &Analysis, opts: &DraftOpts) -> Project {
    let fps = a.source.fps;
    let even = |v: u32| (v.max(2) + 1) & !1;
    let (mut w, mut h) = (a.source.width, a.source.height);
    match (opts.width, opts.height) {
        (Some(ow), Some(oh)) => (w, h) = (ow, oh),
        (Some(ow), None) => (w, h) = (ow, (ow as f64 * h as f64 / w as f64).round() as u32),
        (None, Some(oh)) => (w, h) = ((oh as f64 * w as f64 / h as f64).round() as u32, oh),
        (None, None) => {}
    }
    let (w, h) = (even(w), even(h));
    let aspect = w as f64 / h as f64;
    let bpm = a.beats.bpm.value.max(1.0);
    let offset = a.beats.offset_seconds.value;
    let clock = Clock { beats: &a.beats, fps, snap: opts.snap };
    let duration = ((a.source.duration_seconds - offset) * bpm / 60.0).max(1e-3);

    let rel = |p: &Path| relative_path(&opts.project_dir, p);
    let mut slots: BTreeMap<String, Slot> = BTreeMap::new();
    for s in &a.slots {
        let shots: Vec<String> = s.shots.iter().map(|i| i.to_string()).collect();
        let proxy = s.proxy.as_ref().map(|p| opts.analysis_dir.join(p));
        slots.insert(
            s.id.clone(),
            Slot {
                path: if opts.proxies { proxy.as_ref().map(|p| rel(p)) } else { None },
                fit: Fit::Cover,
                focus: [0.5, 0.5],
                note: Some(format!(
                    "shot{} {}; reference frame {}{}",
                    if shots.len() > 1 { "s" } else { "" },
                    shots.join(", "),
                    s.ref_frame,
                    proxy.map(|p| format!("; proxy still {}", rel(&p))).unwrap_or_default()
                )),
            },
        );
    }

    let mut layers: Vec<Layer> = Vec::new();
    let mut markers: Vec<Marker> = Vec::new();
    let period = a.ingest.loop_info.as_ref().map(|l| l.period_frames.value.round() as usize);
    let repeats = match (&a.ingest.loop_info, opts.repeats) {
        (Some(l), true) => (l.repeats + 1e-6).floor().max(1.0) as usize,
        _ => 1,
    };
    for r in 0..repeats {
        let shift = r * period.unwrap_or(0);
        if repeats > 1 {
            markers.push(Marker { t: clock.time(a.ingest.analysed_f0 + shift), label: format!("loop pass {} of {repeats}", r + 1) });
        }
        for shot in &a.shots {
            layers.push(shot_layer(shot, &clock, shift, r, aspect));
            if shot.status != Ident::Measured {
                markers.push(Marker {
                    t: clock.time(shot.f0 + shift),
                    label: format!("shot {} needs an agent (not measured: transform track): {}", shot.id, shot.notes.join("; ")),
                });
            } else if r == 0 {
                for n in &shot.notes {
                    markers.push(Marker { t: clock.time(shot.f0), label: format!("shot {}: {n}", shot.id) });
                }
            }
        }
    }

    // Solid frames (a fade through white, a white tail) outside the black lead-in.
    for (i, run) in a.ingest.blank_runs.iter().enumerate() {
        if run.kind == "black" || run.f1 <= run.f0 {
            continue;
        }
        let (t0, t1) = (clock.time(run.f0), clock.exact(run.f1).min(duration));
        let (l0, l1) = (run.lum_start / 255.0, run.lum_end / 255.0);
        let mut transform = Transform::default();
        let mut color = [1.0f32, 1.0, 1.0, 1.0];
        if run.kind == "fade" {
            // The reference ramps evenly in display values; with linear-light
            // blending that is close to a quadratic ramp of opacity.
            let ease_name = if l1 < l0 { "quad_out" } else { "in" };
            transform.opacity = Prop::Anim(Anim {
                keys: vec![
                    Key { t: t0, v: Value::Num(round_to(l0.powf(2.2), 3)), ease: edits_core::Ease::Name(ease_name.into()) },
                    Key { t: clock.exact(run.f1 - 1).max(t0 + 0.25 * clock.frame_len()), v: Value::Num(round_to(l1.powf(2.2), 3)), ease: edits_core::Ease::Name("linear".into()) },
                ],
                base: None,
                modifiers: vec![],
            });
        } else {
            let v = ((l0 + l1) / 2.0).powf(2.2) as f32;
            color = [v, v, v, 1.0];
        }
        if t1 > t0 {
            layers.push(Layer {
                id: format!("solid{:02}", i + 1),
                kind: LayerKind::Solid { color },
                name: Some(format!("{} frames {}-{} (luminance {} -> {})", run.kind, run.f0, run.f1, run.lum_start, run.lum_end)),
                start: t0,
                end: Some(t1),
                transform,
                blend: Blend::Normal,
                effects: vec![],
                enabled: true,
            });
        }
    }

    let content = (clock.time(a.ingest.content_f0), clock.time(a.ingest.content_f1));
    // With proxy stills in the slots the overlay and vignette are already in
    // the pictures, so those layers are written disabled.
    let static_on = !opts.proxies;
    if let Some(o) = &a.layers.overlay {
        if let Some(p) = &o.path {
            slots.insert(
                "overlay".into(),
                Slot { path: Some(rel(&opts.analysis_dir.join(p))), fit: Fit::Stretch, focus: [0.5, 0.5], note: Some("static overlay plate recovered from the reference".into()) },
            );
            layers.push(Layer {
                id: "overlay".into(),
                kind: LayerKind::Image { slot: "overlay".into() },
                name: Some(format!("static overlay plate; blend mode and opacity are a guess ({})", o.blend_guess)),
                start: content.0,
                end: Some(content.1),
                transform: Transform::default(),
                blend: Blend::Screen,
                effects: vec![],
                enabled: static_on,
            });
        }
    }
    if let Some(v) = &a.layers.vignette {
        let mut params = BTreeMap::new();
        params.insert("amount".to_string(), Prop::from(round_to(v.amount.value, 3)));
        params.insert("radius".to_string(), Prop::from(round_to(v.radius.value, 3)));
        params.insert("softness".to_string(), Prop::from(round_to(v.softness.value, 3)));
        layers.push(Layer {
            id: "finish".into(),
            kind: LayerKind::Adjustment,
            name: Some("vignette over everything".into()),
            start: content.0,
            end: Some(content.1),
            transform: Transform::default(),
            blend: Blend::Normal,
            effects: vec![Effect { kind: "vignette".into(), params, source: None, enabled: true }],
            enabled: static_on,
        });
    }
    if opts.proxies && (a.layers.overlay.is_some() || a.layers.vignette.is_some()) {
        markers.push(Marker {
            t: content.0,
            label: "proxy stills already contain the overlay and the vignette: layers \"overlay\" and \"finish\" are disabled; enable them when the slots get new pictures".into(),
        });
    }
    markers.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());

    Project {
        format: FORMAT.into(),
        settings: Settings {
            width: w,
            height: h,
            fps,
            duration: round_to(duration, 4),
            time_base: TimeBase::Beats,
            bpm: Some(round_to(bpm, 3)),
            beat_offset: round_to(offset, 4),
            background: [0.0, 0.0, 0.0],
            motion_blur: MotionBlur::default(),
        },
        audio: if a.source.has_audio { Some(Audio { path: rel(Path::new(&a.source.path)), offset: 0.0, gain: 1.0 }) } else { None },
        slots,
        camera: Camera::default(),
        layers,
        markers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(relative_path(Path::new("/a/b/proj"), Path::new("/a/b/analysis/overlay.png")), "../analysis/overlay.png");
        assert_eq!(relative_path(Path::new("/a/b"), Path::new("/a/b/x/y.png")), "x/y.png");
        assert_eq!(relative_path(Path::new("/a/b/c"), Path::new("/q/v.mp4")), "../../../q/v.mp4");
    }
}
