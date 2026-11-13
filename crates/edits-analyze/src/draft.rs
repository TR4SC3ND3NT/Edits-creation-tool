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
    /// With snapping, the nearest quarter beat is used only when the renderer
    /// shows it on that same frame (the quarter beat falls inside the frame
    /// before it): a key moved to another frame would not reproduce the
    /// reference. (`edits calib` measured the earlier rule, which allowed a
    /// frame either way, putting the real reference's flashes and punches a
    /// frame late on at least 3 of its 17 shots.)
    fn time(&self, frame: usize) -> f64 {
        if self.snap {
            if let Some(q) = self.beats.snap(frame as f64, self.fps) {
                let shown = (self.beats.frame_of_beat(q, self.fps) - 1e-6).ceil();
                if (shown - frame as f64).abs() < 0.5 {
                    return q;
                }
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

/// Key values that make the rendered curve pass through the measured values
/// on the key frames. A key snapped to a quarter beat sits up to one frame
/// before the frame on which it was measured, so by the time that frame is
/// shown its segment has already moved: a `decay` at rate 0.5 keyed 0.8 of a
/// frame early shows only 57% of its key value. For each segment the eased
/// fraction `e` reached at the frame is computed with the renderer's easing,
/// and the key value is set to `(v - e * next) / (1 - e)` (last key first, so
/// `next` is already corrected). Segments that have moved more than 60% of
/// the way, and segments that are not front-loaded (`decay`, `out`,
/// `quad_out`, `quint_out`, `expo_out`) or that another segment ramps into
/// (the correction would bend that ramp), are left alone.
fn on_frames(c: &Curve, times: &[f64], clock: &Clock, shift: usize) -> Vec<CurveKey> {
    let mut keys = c.keys.clone();
    for i in (0..keys.len().saturating_sub(1)).rev() {
        let (t0, t1) = (times[i], times[i + 1]);
        if t1 <= t0 {
            continue;
        }
        let tf = clock.beats.beat_of_frame((keys[i].frame + shift) as f64, clock.fps);
        let u = (tf - t0) / (t1 - t0);
        if u <= 1e-4 {
            continue;
        }
        // Only easings that move fastest at their start need it; a linear
        // ramp or a hold is off by a fraction of a frame's share at most, and
        // correcting a ramp would pull the frames before the key the wrong way.
        let front_loaded = matches!(&keys[i].ease, edits_core::Ease::Name(n) | edits_core::Ease::Params { name: n, .. } if matches!(n.as_str(), "decay" | "out" | "quad_out" | "quint_out" | "expo_out"));
        let e = ease::apply(&keys[i].ease, u, (t1 - t0) / clock.frame_len());
        // The segment before the key ends on the corrected value too: only
        // correct a key that nothing ramps into (the first key, or one after a hold).
        let ramped_into = i > 0 && !matches!(&keys[i - 1].ease, edits_core::Ease::Name(n) if n == "hold");
        if !front_loaded || ramped_into || !(1e-4..0.6).contains(&e) {
            continue;
        }
        let next = keys[i + 1].v.clone();
        for (v, n) in keys[i].v.iter_mut().zip(&next) {
            *v = (*v - e * n) / (1.0 - e);
        }
    }
    keys
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
            let keys = on_frames(c, &t, clock, shift);
            match c.prop.as_str() {
                "scale" => {
                    has_scale = true;
                    transform.scale = anim(&t, &keys, |k| Value::Num(round_to(k.v[0] * m, 4)));
                }
                "position" => transform.position = anim(&t, &keys, |k| Value::Vec(vec![round_to(k.v[0], 4), round_to(k.v[1], 4), 0.0])),
                "rotation" => transform.rotation = anim(&t, &keys, |k| Value::Vec(vec![0.0, 0.0, round_to(k.v[0], 2)])),
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
                    let t = key_times(c);
                    let keys = on_frames(c, &t, clock, shift);
                    // Blur radii and saturation amounts cannot go below 0.
                    let floor = if matches!(kind, "gaussian_blur" | "saturation") { 0.0 } else { f64::MIN };
                    params.insert(p.to_string(), anim(&t, &keys, |key| Value::Num(round_to(key.v[0].max(floor), 4))));
                }
            }
        }
        if !params.is_empty() {
            effects.push(Effect { kind: kind.into(), params, options: Default::default(), source: None, enabled: true });
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
        enabled: true, transition: None, fps: None, masks: Vec::new(), matte: None, visible: true,
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
                trim: None,
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
                enabled: true, transition: None, fps: None, masks: Vec::new(), matte: None, visible: true,
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
                Slot { path: Some(rel(&opts.analysis_dir.join(p))), fit: Fit::Stretch, focus: [0.5, 0.5], note: Some("static overlay plate recovered from the reference".into()), trim: None },
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
                enabled: static_on, transition: None, fps: None, masks: Vec::new(), matte: None, visible: true,
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
            effects: vec![Effect { kind: "vignette".into(), params, options: Default::default(), source: None, enabled: true }],
            enabled: static_on, transition: None, fps: None, masks: Vec::new(), matte: None, visible: true,
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
        audio_clips: Vec::new(),
        slots,
        camera: Camera::default(),
        layers,
        markers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beats(bpm: f64, offset: f64, fps: f64) -> Beats {
        serde_json::from_value(serde_json::json!({
            "source": "override",
            "bpm": { "value": bpm, "unit": "bpm", "confidence": 1.0, "identifiability": "measured" },
            "bpm_raw": bpm,
            "offset_seconds": { "value": offset, "unit": "s", "confidence": 1.0, "identifiability": "measured" },
            "refine_shift_frames": 0.0,
            "frames_per_beat": fps * 60.0 / bpm,
            "candidates": []
        }))
        .unwrap()
    }

    #[test]
    fn keys_stay_on_their_frame_and_show_the_measured_value() {
        // 120 bpm at 25 fps: 12.5 frames per beat, beat 0 at 0.1 s (frame 2.5).
        let b = beats(120.0, 0.1, 25.0);
        let clock = Clock { beats: &b, fps: 25.0, snap: true };
        // Frame 6 starts at beat 0.28; the quarter beat 0.25 is shown on frame 6 (it starts at frame 5.625): snapped.
        assert_eq!(clock.time(6), 0.25);
        // Frame 5 starts at beat 0.2; the quarter beat 0.25 would be shown on frame 6: not snapped.
        assert!((clock.time(5) - 0.2).abs() < 1e-3, "{}", clock.time(5));
        // A flash measured at 3 stops on frame 6, decaying at rate 0.5 to 0 on frame 12 (keyed at 0.25 and 0.75).
        let key = |frame: usize, v: f64, ease: edits_core::Ease| CurveKey { frame, beat: 0.0, beat_snapped: None, v: vec![v], ease, rms: None, runner_up: None };
        let mut args = std::collections::BTreeMap::new();
        args.insert("rate".to_string(), 0.5);
        let c = Curve {
            prop: "exposure.stops".into(),
            unit: "stops".into(),
            confidence: 1.0,
            identifiability: Ident::Measured,
            keys: vec![key(6, 3.0, edits_core::Ease::Params { name: "decay".into(), args }), key(12, 0.0, edits_core::Ease::Name("hold".into()))],
        };
        let times = clock.times(&[6, 12]);
        assert_eq!(times, vec![0.25, 0.75]);
        let keys = on_frames(&c, &times, &clock, 0);
        // The key sits 0.375 frames before frame 6: the key value is raised so that frame 6 shows 3 stops.
        let (t0, t1) = (times[0], times[1]);
        let u = (b.beat_of_frame(6.0, 25.0) - t0) / (t1 - t0);
        let shown = keys[0].v[0] * (1.0 - ease::apply(&keys[0].ease, u, (t1 - t0) * 12.5));
        assert!(keys[0].v[0] > 3.3 && (shown - 3.0).abs() < 1e-9, "key {} shows {shown}", keys[0].v[0]);
        assert_eq!(keys[1].v[0], 0.0);
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative_path(Path::new("/a/b/proj"), Path::new("/a/b/analysis/overlay.png")), "../analysis/overlay.png");
        assert_eq!(relative_path(Path::new("/a/b"), Path::new("/a/b/x/y.png")), "x/y.png");
        assert_eq!(relative_path(Path::new("/a/b/c"), Path::new("/q/v.mp4")), "../../../q/v.mp4");
    }
}
