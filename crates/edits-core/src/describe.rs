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
    layer_lines(&mut o, &p.layers, s.duration);
    if !p.comps.is_empty() {
        let _ = writeln!(o, "comps (layers in comp time):");
    }
    for (name, c) in &p.comps {
        let _ = writeln!(o, "  {}", crate::comp::describe_heading(p, name, c, unit));
        let mut inner = String::new();
        layer_lines(&mut inner, &c.layers, c.duration);
        for line in inner.lines() {
            let _ = writeln!(o, "  {line}");
        }
    }
    for m in &p.markers {
        let _ = writeln!(o, "marker {:.2}: {}", m.t, m.label);
    }
    if !p.audio_clips.is_empty() {
        let _ = writeln!(o, "audio clips (sound effects):");
    }
    for c in &p.audio_clips {
        let mut parts = vec![c.path.clone()];
        if c.trim_in != 0.0 || c.trim_out.is_some() {
            parts.push(format!("trim {}..{} s", c.trim_in, c.trim_out.map(|b| b.to_string()).unwrap_or_else(|| "end".into())));
        }
        if c.volume != 1.0 {
            parts.push(if c.volume == 0.0 { "muted".to_string() } else { format!("volume {}", c.volume) });
        }
        if c.fade_in > 0.0 {
            parts.push(format!("fade in {} s", c.fade_in));
        }
        if c.fade_out > 0.0 {
            parts.push(format!("fade out {} s", c.fade_out));
        }
        let _ = writeln!(o, "  {:<14} at {:>7.2} {}{}", c.id, c.at, parts.join(", "), if c.enabled { "" } else { " (disabled)" });
    }
    o
}

/// One line per layer of `layers` (bottom to top), and an indented line per
/// transition, mask, animated value and effect. `duration` is where a layer
/// without `out` ends.
fn layer_lines(o: &mut String, layers: &[Layer], duration: f64) {
    for l in layers {
        let end = l.end.unwrap_or(duration);
        let what = match &l.kind {
            LayerKind::Image { slot } => format!("image {slot}"),
            LayerKind::Solid { color } => format!("solid {color:?}"),
            LayerKind::Cube { faces, size } => format!("cube {size:?} faces {}", faces.join(",")),
            LayerKind::Adjustment => "adjustment".to_string(),
            LayerKind::Particles(ps) => ps.describe(),
            LayerKind::Video(v) => {
                let mut parts = vec![format!("video {}", v.slot)];
                if let Some(remap) = &v.time_remap {
                    parts.push(format!("remap {}", prop_summary(remap).unwrap_or_else(|| const_text(remap))));
                } else if let Some(speed) = &v.speed {
                    parts.push(format!("speed {}", prop_summary(speed).unwrap_or_else(|| const_text(speed))));
                }
                for (on, word) in [(v.looped, "loop"), (v.reverse, "reverse"), (v.frame_blend == FrameBlend::Mix, "mix"), (v.frame_blend == FrameBlend::Flow, "flow")] {
                    if on {
                        parts.push(word.to_string());
                    }
                }
                if v.volume != 1.0 {
                    parts.push(if v.volume == 0.0 { "muted".to_string() } else { format!("volume {}", v.volume) });
                }
                parts.join(" ")
            }
            LayerKind::Text(t) => text_summary(t),
            LayerKind::Comp(c) => crate::comp::describe_layer(c),
            LayerKind::Shape(s) => s.describe(),
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
            match (l.enabled, l.visible) {
                (false, _) => " (disabled)",
                (true, false) => " hidden (matte only)",
                (true, true) => "",
            }
        );
        if let Some(tr) = &l.transition {
            let params: Vec<String> = tr.params.iter().map(|(k, v)| format!("{k}={}", prop_summary(v).unwrap_or_else(|| const_text(v)))).collect();
            let ease = if tr.ease == Ease::default() { String::new() } else { format!(" {}", serde_json::to_string(&tr.ease).unwrap_or_default()) };
            let map = tr.source.as_deref().map(|s| format!(" through {s}")).unwrap_or_default();
            let _ = writeln!(o, "      transition in: {}{map} over {}{ease} {}", tr.kind, tr.duration, params.join("; "));
        }
        let mut lines = Vec::new();
        crate::mask::describe(l, &mut lines);
        for line in lines {
            let _ = writeln!(o, "      {line}");
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
}

/// `text "SAY MY NAME" size 12 fonts/Anton.ttf w800 left stroke 0.8 [0.0, 0.0, 0.0, 1.0] wrap 1.2`
fn text_summary(t: &Text) -> String {
    const SHOWN: usize = 40;
    let words: String = t.text.chars().take(SHOWN).collect();
    let more = if t.text.chars().count() > SHOWN { "…" } else { "" };
    let mut parts = vec![format!("text {words:?}{more} size {}", t.size), t.font.clone().unwrap_or_else(|| "Inter".into())];
    if let Some(w) = t.weight {
        parts.push(format!("w{w}"));
    }
    match t.align {
        Align::Left => parts.push("left".into()),
        Align::Right => parts.push("right".into()),
        Align::Center => {}
    }
    if t.fill != [1.0, 1.0, 1.0, 1.0] {
        parts.push(format!("fill {:?}", t.fill));
    }
    if let Some(s) = &t.stroke {
        parts.push(format!("stroke {} {:?}", s.width, s.color));
    }
    if t.tracking != 0.0 {
        parts.push(format!("tracking {}", t.tracking));
    }
    if t.line_height != 1.2 {
        parts.push(format!("line_height {}", t.line_height));
    }
    if let Some(m) = t.max_width {
        parts.push(format!("wrap {m}"));
    }
    if !t.animators.is_empty() {
        let list: Vec<String> = t.animators.iter().map(animator_summary).collect();
        parts.push(format!("animators [{}]", list.join("; ")));
    }
    parts.join(" ")
}

/// `opacity+scale by range characters ramp_up random` or `position by wiggly words`.
fn animator_summary(a: &crate::text_select::TextAnimator) -> String {
    use crate::text_select::Selector;
    let props = a.properties();
    let mut s = format!("{} by ", if props.is_empty() { "nothing".to_string() } else { props.join("+") });
    match &a.selector {
        Selector::Range(r) => {
            s.push_str(&format!("range {} {}", r.units.name(), r.shape.name()));
            let keyed = [&r.start, &r.end, &r.offset].iter().any(|p| matches!(p, Some(Prop::Anim(_))));
            if keyed {
                s.push_str(" keyed");
            }
            if let Some(Ease::Name(n)) = &r.ease {
                s.push_str(&format!(" {n}"));
            }
            if r.randomize {
                s.push_str(" random");
            }
        }
        Selector::Wiggly(w) => s.push_str(&format!("wiggly {}", w.units.name())),
    }
    if let Some(n) = &a.name {
        s = format!("{n}: {s}");
    }
    if !a.enabled {
        s.push_str(" (disabled)");
    }
    s
}

pub(crate) fn const_text(p: &Prop) -> String {
    match p {
        Prop::Const(Value::Num(n)) => format!("{n}"),
        Prop::Const(Value::Vec(v)) => format!("{v:?}"),
        Prop::Anim(_) => "animated".into(),
    }
}

/// `None` for constants; otherwise a one-line summary of keys and modifiers.
pub(crate) fn prop_summary(p: &Prop) -> Option<String> {
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
    fn text_layers_show_what_they_say() {
        let p = Project::from_json(
            r#"{"format":"edits.project/1","settings":{"width":64,"height":64,"fps":25,"duration":4},
                "layers":[{"id":"title","kind":"text","text":"SAY MY NAME","font":"fonts/Anton.ttf","size":12,"weight":800,"align":"left",
                           "stroke":{"color":[0,0,0],"width":0.8},"max_width":1.2},
                          {"id":"cap","kind":"text","text":"hello\nworld"}]}"#,
        )
        .unwrap();
        let d = describe(&p);
        assert!(d.contains(r#"text "SAY MY NAME" size 12 fonts/Anton.ttf w800 left stroke 0.8 [0.0, 0.0, 0.0, 1.0] wrap 1.2"#), "{d}");
        assert!(d.contains(r#"text "hello\nworld" size 10 Inter"#), "{d}");
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
        let flow = Project::from_json(&serde_json::to_string(&p).unwrap().replace(r#""frame_blend":"mix""#, r#""frame_blend":"flow""#)).unwrap();
        assert!(describe(&flow).contains("loop flow muted"), "{}", describe(&flow));
    }

    #[test]
    fn audio_clips_are_listed() {
        let p = Project::from_json(
            r#"{"format":"edits.project/1","settings":{"width":64,"height":64,"fps":25,"duration":4},"layers":[],
                "audio_clips":[{"id":"whoosh","path":"sfx/whoosh.wav","at":1.5,"trim_in":0.2,"volume":0.5,"fade_out":0.25},
                               {"id":"hit","path":"hit.wav","at":2,"enabled":false}]}"#,
        )
        .unwrap();
        let d = describe(&p);
        assert!(d.contains("audio clips (sound effects):"), "{d}");
        assert!(d.contains("  whoosh         at    1.50 sfx/whoosh.wav, trim 0.2..end s, volume 0.5, fade out 0.25 s\n"), "{d}");
        assert!(d.contains("  hit            at    2.00 hit.wav (disabled)\n"), "{d}");
    }
}
