//! A compact text view of a project, meant to be read by an agent before it
//! edits anything: a few hundred tokens instead of the whole JSON.

use crate::model::*;
use std::fmt::Write;

pub fn describe(p: &Project) -> String {
    let s = &p.settings;
    let unit = match s.time_base {
        TimeBase::Beats => "beats",
        TimeBase::Seconds => "s",
    };
    let mut o = String::new();
    let _ = writeln!(
        o,
        "project {}x{} @ {} fps, {} {unit} ({:.2} s, {} frames){}",
        s.width,
        s.height,
        s.fps,
        s.duration,
        p.duration_seconds(),
        p.frame_count(),
        s.bpm.map(|b| format!(", {b} BPM, beat 0 at {:.3} s", s.beat_offset)).unwrap_or_default()
    );
    if s.motion_blur.samples > 1 {
        let _ = writeln!(o, "motion blur: {} samples, shutter {}", s.motion_blur.samples, s.motion_blur.shutter);
    }
    if let Some(a) = &p.audio {
        let _ = writeln!(o, "audio: {} (offset {} s)", a.path, a.offset);
    }
    let _ = writeln!(o, "slots:");
    for (name, slot) in &p.slots {
        let trim = slot.trim.map(|[a, b]| format!(" trim {a}..{b} s")).unwrap_or_default();
        let _ = writeln!(o, "  {name}: {} [{:?}]{trim}", slot.path.as_deref().unwrap_or("(empty)"), slot.fit);
    }
    let cam = [("fov", &p.camera.fov), ("position", &p.camera.position), ("rotation", &p.camera.rotation)];
    let animated: Vec<String> = cam.iter().filter_map(|(n, pr)| prop_summary(pr).map(|s| format!("{n} {s}"))).collect();
    if !animated.is_empty() {
        let _ = writeln!(o, "camera: {}", animated.join("; "));
    }
    let _ = writeln!(o, "layers (bottom to top):");
    for l in &p.layers {
        let end = l.end.unwrap_or(s.duration);
        let what = match &l.kind {
            LayerKind::Image { slot } => format!("image {slot}"),
            LayerKind::Solid { color } => format!("solid {color:?}"),
            LayerKind::Cube { faces, size } => format!("cube {size:?} faces {}", faces.join(",")),
            LayerKind::Adjustment => "adjustment".to_string(),
            LayerKind::Video(v) => {
                let mut parts = vec![format!("video {}", v.slot)];
                if let Some(remap) = &v.time_remap {
                    parts.push(format!("remap {}", prop_summary(remap).unwrap_or_else(|| const_text(remap))));
                } else if let Some(speed) = &v.speed {
                    parts.push(format!("speed {}", prop_summary(speed).unwrap_or_else(|| const_text(speed))));
                }
                for (on, word) in [(v.looped, "loop"), (v.reverse, "reverse"), (v.frame_blend == FrameBlend::Mix, "mix")] {
                    if on {
                        parts.push(word.to_string());
                    }
                }
                if v.volume != 1.0 {
                    parts.push(if v.volume == 0.0 { "muted".to_string() } else { format!("volume {}", v.volume) });
                }
                parts.join(" ")
            }
        };
        let what = match l.fps {
            Some(fps) => format!("{what} fps={fps}"),
            None => what,
        };
        let _ = writeln!(
            o,
            "  {:<14} {:>7.2}..{:<7.2} {what}{}{}",
            l.id,
            l.start,
            end,
            if l.blend != Blend::Normal { format!(" blend={}", l.blend.name()) } else { String::new() },
            if l.enabled { "" } else { " (disabled)" }
        );
        if let Some(tr) = &l.transition {
            let params: Vec<String> = tr.params.iter().map(|(k, v)| format!("{k}={}", prop_summary(v).unwrap_or_else(|| const_text(v)))).collect();
            let ease = if tr.ease == Ease::default() { String::new() } else { format!(" {}", serde_json::to_string(&tr.ease).unwrap_or_default()) };
            let _ = writeln!(o, "      transition in: {} over {}{ease} {}", tr.kind, tr.duration, params.join("; "));
        }
        let t = &l.transform;
        for (n, pr) in [("position", &t.position), ("scale", &t.scale), ("rotation", &t.rotation), ("anchor", &t.anchor), ("opacity", &t.opacity)] {
            if let Some(sum) = prop_summary(pr) {
                let _ = writeln!(o, "      {n}: {sum}");
            }
        }
        for e in &l.effects {
            let options = e.options.iter().map(|(k, v)| format!("{k}={v}"));
            let params: Vec<String> = options
                .chain(e.params.iter().map(|(k, v)| match prop_summary(v) {
                    Some(sum) => format!("{k} {sum}"),
                    None => format!("{k}={}", const_text(v)),
                }))
                .collect();
            let _ = writeln!(o, "      fx {}{} {}", e.kind, if e.enabled { "" } else { " (off)" }, params.join("; "));
        }
    }
    for m in &p.markers {
        let _ = writeln!(o, "marker {:.2}: {}", m.t, m.label);
    }
    o
}

fn const_text(p: &Prop) -> String {
    match p {
        Prop::Const(Value::Num(n)) => format!("{n}"),
        Prop::Const(Value::Vec(v)) => format!("{v:?}"),
        Prop::Anim(_) => "animated".into(),
    }
}

/// `None` for constants; otherwise a one-line summary of keys and modifiers.
fn prop_summary(p: &Prop) -> Option<String> {
    let Prop::Anim(a) = p else { return None };
    let mut parts: Vec<String> = a
        .keys
        .iter()
        .map(|k| {
            let v = match &k.v {
                Value::Num(n) => format!("{n}"),
                Value::Vec(v) => format!("{v:?}"),
            };
            let e = match &k.ease {
                Ease::Name(n) if n == "linear" => String::new(),
                Ease::Name(n) => format!(" {n}"),
                Ease::Bezier(b) => format!(" bezier{b:?}"),
                Ease::Params { name, args } => format!(" {name}{args:?}"),
            };
            format!("{}→{v}{e}", k.t)
        })
        .collect();
    for m in &a.modifiers {
        parts.push(match m {
            Modifier::Shake { frequency, amplitude, .. } => format!("+shake {} @ {frequency} Hz", const_text(amplitude)),
            Modifier::Loop { from, period } => format!("+loop from {from} every {period}"),
        });
    }
    Some(parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_show_their_options_and_values() {
        let p = Project::from_json(
            r#"{"format":"edits.project/1","settings":{"width":64,"height":64,"fps":25,"duration":4},
                "layers":[{"id":"fx","kind":"adjustment","effects":[{"kind":"halftone","options":{"mode":"cmyk"},"params":{"size":2}}]}]}"#,
        )
        .unwrap();
        assert!(describe(&p).contains("fx halftone mode=cmyk; size=2"), "{}", describe(&p));
    }

    #[test]
    fn video_layers_show_their_timing() {
        let p = Project::from_json(
            r#"{"format":"edits.project/1","settings":{"width":64,"height":64,"fps":25,"duration":4},
                "slots":{"op":{"path":"op.mp4","trim":[1.5,4]}},
                "layers":[{"id":"clip","kind":"video","slot":"op","speed":{"keys":[{"t":0,"v":1},{"t":2,"v":0.25}]},"loop":true,"frame_blend":"mix","volume":0,"fps":12}]}"#,
        )
        .unwrap();
        let d = describe(&p);
        assert!(d.contains("op: op.mp4 [Cover] trim 1.5..4 s"), "{d}");
        assert!(d.contains("video op speed 0→1, 2→0.25 loop mix muted fps=12"), "{d}");
    }
}
