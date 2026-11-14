//! What `draft` adds to a shot's layer beyond the measured curves: the blur
//! kind, the posterise and duotone hits the colour detectors found, and the
//! vision labels (`shots[].vision`, see `vision`).
//!
//! The rule: labels choose the *kind* of thing (a cube instead of a flat
//! picture, an outline and its colour, a gradient map, a transition); the
//! signals keep the *amounts and times* wherever they measured them. A label
//! only adds an effect the signals did not already put there, and then with
//! the engine's preset defaults, spread over the frames its tiles name (or
//! the whole shot). Every change is reported as one line.

use crate::blurkind;
use crate::schema::*;
use crate::vision::has;
use edits_core::{Anim, Effect, Ease, Key, Layer, LayerKind, Prop, Value};
use serde_json::{json, Map, Value as Json};

/// A preset to apply once the project is built (presets work on the JSON document).
#[derive(Clone, Debug)]
pub struct PresetOp {
    pub preset: String,
    pub layer: String,
    pub at: f64,
    pub vars: Map<String, Json>,
}

#[derive(Default, Debug)]
pub struct Dressed {
    /// One line per change, `shot N: ...`.
    pub notes: Vec<String>,
    pub presets: Vec<PresetOp>,
    /// Layers whose `out` should reach this time (the picture under a picture-in-picture): (layer index offset -1, time).
    pub extend_previous_to: Option<f64>,
}

/// sRGB display colour to the linear RGB that effect colour parameters take.
pub fn linear(c: [f64; 3]) -> Value {
    let dec = |e: f64| if e <= 0.04045 { e / 12.92 } else { ((e + 0.055) / 1.055).powf(2.4) };
    Value::Vec(c.iter().map(|v| (dec(v.clamp(0.0, 1.0)) * 1000.0).round() / 1000.0).collect())
}

fn num(v: f64) -> Prop {
    Prop::Const(Value::Num(v))
}

fn hold() -> Ease {
    Ease::Name("hold".into())
}

/// `on` from `t_on` to `t_off`, `off` elsewhere in `t0..t1`.
fn step(t0: f64, t_on: f64, t_off: f64, t1: f64, on: f64, off: f64) -> Prop {
    if t_on <= t0 + 1e-6 && t_off >= t1 - 1e-6 {
        return num(on);
    }
    let mut keys = Vec::new();
    if t_on > t0 + 1e-6 {
        keys.push(Key { t: t0, v: Value::Num(off), ease: hold() });
    }
    keys.push(Key { t: t_on, v: Value::Num(on), ease: hold() });
    if t_off < t1 - 1e-6 {
        keys.push(Key { t: t_off, v: Value::Num(off), ease: hold() });
    }
    Prop::Anim(Anim { keys, base: None, modifiers: vec![] })
}

fn ramp(t0: f64, t1: f64, a: Value, b: Value, ease: &str) -> Prop {
    Prop::Anim(Anim { keys: vec![Key { t: t0, v: a, ease: Ease::Name(ease.into()) }, Key { t: t1, v: b, ease: hold() }], base: None, modifiers: vec![] })
}

fn scale_prop(p: &Prop, f: f64) -> Prop {
    let sc = |v: &Value| match v {
        Value::Num(x) => Value::Num((x * f * 1000.0).round() / 1000.0),
        Value::Vec(xs) => Value::Vec(xs.iter().map(|x| x * f).collect()),
    };
    match p {
        Prop::Const(v) => Prop::Const(sc(v)),
        Prop::Anim(a) => Prop::Anim(Anim { keys: a.keys.iter().map(|k| Key { t: k.t, v: sc(&k.v), ease: k.ease.clone() }).collect(), base: a.base.clone(), modifiers: a.modifiers.clone() }),
    }
}

fn effect(kind: &str, params: Vec<(&str, Prop)>, options: Vec<(&str, &str)>) -> Effect {
    Effect {
        kind: kind.into(),
        params: params.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        options: options.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        source: None,
        enabled: true,
    }
}

/// Insert a colour effect before the exposure (a flash then whitens the coloured picture).
fn insert_colour(layer: &mut Layer, e: Effect) {
    let at = layer.effects.iter().position(|x| x.kind == "exposure").unwrap_or(layer.effects.len());
    layer.effects.insert(at, e);
}

fn has_effect(layer: &Layer, kind: &str) -> bool {
    layer.effects.iter().any(|e| e.kind == kind)
}

/// The time range a label covers: the frames its tiles name, else the whole shot.
fn span(v: &ShotVision, label: &str, time: &dyn Fn(usize) -> f64, t0: f64, t1: f64, f1: usize) -> (f64, f64) {
    match v.frames.get(label).filter(|f| !f.is_empty()) {
        Some(fr) => {
            let a = *fr.iter().min().unwrap();
            let b = (*fr.iter().max().unwrap() + 1).min(f1);
            (time(a).max(t0), if b >= f1 { t1 } else { time(b).min(t1) })
        }
        None => (t0, t1),
    }
}

/// Convert the shot's Gaussian-equivalent blur to its kind (labelled, or
/// classified by `blurkind` with confidence 0.45 or more).
fn blur_kind(layer: &mut Layer, shot: &Shot, label: Option<&str>, out: &mut Dressed) {
    let Some(i) = layer.effects.iter().position(|e| e.kind == "gaussian_blur") else { return };
    let measured = shot.blur_kind.as_ref().filter(|k| k.confidence >= 0.45 && k.kind != "gaussian");
    let (kind, angle, why) = match label.map(|l| l.split_once(':').map(|(a, b)| (a, b.parse::<f64>().ok())).unwrap_or((l, None))) {
        Some((k @ ("directional" | "zoom" | "spin"), a)) => (k, a.or(measured.and_then(|m| m.angle)), "vision"),
        Some(("gaussian" | "none", _)) => return,
        _ => match measured {
            Some(m) => (m.kind.as_str(), m.angle, "gradient statistics"),
            None => return,
        },
    };
    let (name, param, typical) = blurkind::param_for(kind, 1.0);
    // The amount comes from the signals: the shot's own conversion, measured
    // by blurring its reference frame the same way, when the kinds agree and
    // it is within a factor of 3 of what renders of the effect give. Otherwise
    // the blur stays Gaussian (its radius is measured) and the kind is a note.
    let factor = match shot.blur_kind.as_ref().filter(|k| k.kind == kind).and_then(|k| k.per_sigma) {
        Some(f) if f > typical / 3.0 && f < typical * 3.0 => f,
        _ => {
            out.notes.push(format!("shot {}: blur looks {kind} ({why}) but its amount was not measured as {kind}: kept gaussian, look again", shot.id));
            return;
        }
    };
    let Some(radius) = layer.effects[i].params.get("radius").cloned() else { return };
    let mut params = vec![(param, scale_prop(&radius, factor))];
    if kind == "directional" {
        params.push(("angle", num(angle.unwrap_or(0.0))));
    }
    layer.effects[i] = effect(name, params, vec![]);
    out.notes.push(format!("shot {}: blur drafted as {name} ({why})", shot.id));
}

/// Scale and place a card where the analysis found it (`Shot::card`), or by `default` scale.
fn card_geometry(layer: &mut Layer, shot: &Shot, default: f64) {
    match &shot.card {
        Some(b) => {
            // A cover-fitted picture is one frame height high at scale 1.
            layer.transform.scale = scale_prop(&layer.transform.scale, (b.size[1].clamp(0.2, 1.0) * 1000.0).round() / 1000.0);
            if matches!(layer.transform.position, Prop::Const(_)) {
                layer.transform.position = Prop::Const(Value::Vec(vec![b.center[0], b.center[1], 0.0]));
            }
        }
        None => layer.transform.scale = scale_prop(&layer.transform.scale, default),
    }
}

/// Additions to one shot's layer. `time(frame)` is the project time of a
/// frame of this pass; `next_slot` is the slot of the following shot (a cube's
/// second face).
pub fn dress(layer: &mut Layer, shot: &Shot, next_slot: Option<&str>, time: &dyn Fn(usize) -> f64, use_vision: bool, first: bool) -> Dressed {
    let mut out = Dressed::default();
    let (t0, t1) = (layer.start, layer.end.unwrap_or(layer.start + 1.0));
    let id = shot.id;
    let v = shot.vision.as_ref().filter(|_| use_vision);
    blur_kind(layer, shot, v.map(|v| v.blur.as_str()).filter(|b| *b != "unsure"), &mut out);

    // Colour hits the histogram detectors measured.
    let posts: Vec<&Event> = shot.events.iter().filter(|e| e.kind == "posterize").collect();
    // Posterise is a weak flag on its own (a flash or a codec thins the histogram too):
    // drafted when the vision labels saw it.
    let seen_poster = shot.vision.as_ref().filter(|_| use_vision).map(|v| has(&v.tone, "posterized")).unwrap_or(false);
    if let Some(e) = posts.first().filter(|_| seen_poster) {
        let levels = e.params.get("levels").map(|l| l.value.round().clamp(2.0, 16.0)).unwrap_or(4.0);
        let (a, b) = (time(e.f0).max(t0), if e.f1 >= shot.f1 { t1 } else { time(e.f1) });
        insert_colour(layer, effect("posterize", vec![("levels", step(t0, a, b, t1, levels, 256.0))], vec![]));
        out.notes.push(format!("shot {id}: posterize {levels} levels (measured)"));
    }
    // A duotone may be the picture itself (any few-coloured picture
    // reads so, a flash thins colour too): drafted with the measured colours
    // when the vision labels saw a duotone or a tint.
    let seen_duo = shot.vision.as_ref().filter(|_| use_vision).map(|v| has(&v.tone, "duotone") || has(&v.tone, "tinted")).unwrap_or(false);
    if let Some(e) = shot.events.iter().find(|e| e.kind == "duotone" && seen_duo) {
        let (s, h) = (e.colors.get("shadows").copied().unwrap_or([0.0; 3]), e.colors.get("highlights").copied().unwrap_or([1.0; 3]));
        let (a, b) = (time(e.f0).max(t0), if e.f1 >= shot.f1 { t1 } else { time(e.f1) });
        insert_colour(layer, effect("gradient_map", vec![("shadows", Prop::Const(linear(s))), ("highlights", Prop::Const(linear(h))), ("amount", step(t0, a, b, t1, 1.0, 0.0))], vec![("mode", "duotone")]));
        out.notes.push(format!("shot {id}: gradient_map duotone with the measured colours"));
    }
    let tracked = shot.status == Ident::Measured;
    if let Some(sh) = &shot.shake {
        // The residual jitter the keyed move leaves, as a shake modifier on the
        // position (smooth noise of about this RMS). A constant zoom keeps the
        // edges covered; compare reads scale relative to the shot, so it cancels.
        let (ax, ay) = ((1.6 * sh.rms_x.value).min(0.04), (1.6 * sh.rms_y.value).min(0.04));
        let base = match &layer.transform.position {
            Prop::Const(v) => Some(v.clone()),
            _ => None,
        };
        let m = edits_core::Modifier::Shake { amplitude: Prop::Const(Value::Vec(vec![(ax * 1e4).round() / 1e4, (ay * 1e4).round() / 1e4, 0.0])), frequency: 12.0, seed: shot.id as u32 };
        match &mut layer.transform.position {
            Prop::Anim(a) => a.modifiers.push(m),
            p => *p = Prop::Anim(Anim { keys: vec![], base, modifiers: vec![m] }),
        }
        layer.transform.scale = scale_prop(&layer.transform.scale, 1.0 + 2.5 * ax.max(ay));
        out.notes.push(format!("shot {id}: shake modifier from the measured residual"));
    }
    let Some(v) = v else { return out };
    // Labels add structure and tone only where the signals could not measure
    // the shot (not tracked): on a tracked shot the absence of an inversion,
    // a threshold or a 3D move was measured too, and a disagreement is a note.
    if tracked {
        for w in ["cube", "tilted_card", "inset_card", "picture_in_picture"] {
            if v.framing == w {
                out.notes.push(format!("shot {id}: labelled {}, but the track held as a flat picture: not drafted, look again", w.replace('_', " ")));
            }
        }
    }

    // ---- framing -----------------------------------------------------------------
    let axis = v.motion.iter().find_map(|m| m.strip_prefix("3d_turn:")).unwrap_or("y");
    let rot_axis = |deg: f64| match axis {
        "x" => vec![deg, 0.0, 0.0],
        "z" => vec![0.0, 0.0, deg],
        _ => vec![0.0, deg, 0.0],
    };
    let card = !tracked && matches!(v.framing.as_str(), "inset_card" | "tilted_card" | "picture_in_picture" | "cube");
    match if card { v.framing.as_str() } else { "" } {
        "cube" => {
            if let LayerKind::Image { slot } = &layer.kind {
                let other = next_slot.unwrap_or(slot).to_string();
                let s = slot.clone();
                layer.kind = LayerKind::Cube { faces: vec![s.clone(), other.clone(), s.clone(), other.clone(), s.clone(), other], size: [0.62, 0.62, 0.62] };
                let (c, sz) = shot.card.as_ref().map(|b| (b.center, b.size[1])).unwrap_or(([0.0, 0.0], 0.75));
                layer.transform.position = Prop::Const(Value::Vec(vec![c[0], c[1], 0.6]));
                // The cube's front face, 0.62 high and brought forward, looks about 0.75 high.
                layer.transform.scale = num(((sz / 0.75).clamp(0.4, 2.0) * 100.0).round() / 100.0);
                layer.transform.rotation = ramp(t0, t1, Value::Vec(rot_axis(-35.0)), Value::Vec(rot_axis(55.0)), "in_out");
                out.notes.push(format!("shot {id}: cube (faces {s} and the next picture) turning about {axis}"));
            }
        }
        "tilted_card" => {
            card_geometry(layer, shot, 0.78);
            let spin = has(&v.motion, "spin");
            let (a, b) = if spin { (vec![10.0, -26.0, -8.0], vec![6.0, -14.0, 8.0]) } else { (vec![10.0, -26.0, 0.0], vec![6.0, -14.0, 0.0]) };
            layer.transform.rotation = ramp(t0, t1, Value::Vec(a), Value::Vec(b), "out");
            out.notes.push(format!("shot {id}: tilted card (scale x0.78, turned about x and y{})", if spin { ", spinning" } else { "" }));
        }
        "inset_card" | "picture_in_picture" => {
            card_geometry(layer, shot, 0.72);
            if v.framing == "picture_in_picture" && !first && shot.card.is_none() {
                out.extend_previous_to = Some(t1);
            }
            out.notes.push(format!("shot {id}: {} (scale x0.72{})", v.framing.replace('_', " "), if v.framing == "picture_in_picture" { ", the previous picture stays underneath" } else { "" }));
        }
        _ => {}
    }
    // ---- border: around a card only (on a full-frame picture it is the frame's edge) ----
    let pos = "outside";
    match if card { v.border.as_str() } else { "" } {
        "thin_light" => layer.effects.push(effect("outline", vec![("width", num(0.35)), ("color", Prop::Const(linear([1.0, 1.0, 1.0])))], vec![("position", pos)])),
        "thin_dark" => layer.effects.push(effect("outline", vec![("width", num(0.35)), ("color", Prop::Const(linear([0.02, 0.02, 0.02])))], vec![("position", pos)])),
        "glow" => {
            let c = v.colors.get("glow").copied().unwrap_or([1.0, 1.0, 1.0]);
            layer.effects.push(effect("outline", vec![("width", num(2.0)), ("softness", num(6.0)), ("opacity", num(0.95)), ("color", Prop::Const(linear(c)))], vec![("position", pos)]));
        }
        _ => {}
    }
    if card && v.border != "none" && v.border != "unsure" {
        out.notes.push(format!("shot {id}: outline for the {} border ({pos})", v.border.replace('_', " ")));
    }
    // ---- tone --------------------------------------------------------------------
    let f1 = shot.f1;
    for word in v.tone.iter().filter(|_| !tracked) {
        // A tone hit the signals did not measure needs its tiles: a whole-shot
        // threshold or inversion is rarely what was seen.
        let needs_tiles = matches!(word.as_str(), "inverted" | "threshold" | "posterized" | "desaturated");
        if needs_tiles && v.frames.get(word.as_str()).map(|f| f.is_empty()).unwrap_or(true) {
            out.notes.push(format!("shot {id}: {word} seen but no tiles given: not drafted"));
            continue;
        }
        let (a, b) = span(v, word, time, t0, t1, f1);
        match word.as_str() {
            "inverted" if !has_effect(layer, "invert") => {
                insert_colour(layer, effect("invert", vec![("amount", step(t0, a, b, t1, 1.0, 0.0))], vec![]));
                out.notes.push(format!("shot {id}: invert (seen, not measured)"));
            }
            "threshold" if !has_effect(layer, "threshold") => {
                insert_colour(layer, effect("threshold", vec![("level", num(0.5)), ("amount", step(t0, a, b, t1, 1.0, 0.0))], vec![]));
                out.notes.push(format!("shot {id}: threshold (seen, not measured)"));
            }
            "posterized" if !has_effect(layer, "posterize") => {
                insert_colour(layer, effect("posterize", vec![("levels", step(t0, a, b, t1, 4.0, 256.0))], vec![]));
                out.notes.push(format!("shot {id}: posterize 4 levels (seen, not measured)"));
            }
            "duotone" if !has_effect(layer, "gradient_map") => {
                let (s, h) = (v.colors.get("shadows").copied().unwrap_or([0.0; 3]), v.colors.get("highlights").copied().unwrap_or([1.0; 3]));
                insert_colour(layer, effect("gradient_map", vec![("shadows", Prop::Const(linear(s))), ("highlights", Prop::Const(linear(h))), ("amount", step(t0, a, b, t1, 1.0, 0.0))], vec![("mode", "duotone")]));
                out.notes.push(format!("shot {id}: gradient_map duotone with the colours seen"));
            }
            "tinted" if !has_effect(layer, "tint") && !has_effect(layer, "gradient_map") => {
                let c = v.colors.get("tint").copied().unwrap_or([1.0, 1.0, 1.0]);
                insert_colour(layer, effect("tint", vec![("color", Prop::Const(linear(c))), ("amount", step(t0, a, b, t1, 0.65, 0.0))], vec![]));
                out.notes.push(format!("shot {id}: tint"));
            }
            "desaturated" if !has_effect(layer, "saturation") => {
                insert_colour(layer, effect("saturation", vec![("amount", step(t0, a, b, t1, 0.0, 1.0))], vec![]));
                out.notes.push(format!("shot {id}: black and white (seen, not measured)"));
            }
            "burnt_white" if !has_effect(layer, "exposure") => {
                out.presets.push(PresetOp { preset: "flash_burn".into(), layer: layer.id.clone(), at: a, vars: Map::new() });
                out.notes.push(format!("shot {id}: flash_burn preset (seen, no flash measured)"));
            }
            _ => {}
        }
    }
    // ---- light -------------------------------------------------------------------
    for word in &v.light {
        match word.as_str() {
            "white_flash" if !tracked && !has_effect(layer, "exposure") => {
                out.presets.push(PresetOp { preset: "flash".into(), layer: layer.id.clone(), at: span(v, word, time, t0, t1, f1).0, vars: Map::new() });
                out.notes.push(format!("shot {id}: flash preset (seen, not measured)"));
            }
            "black_flash" => {
                let mut vars = Map::new();
                vars.insert("color".into(), json!([0.0, 0.0, 0.0]));
                out.presets.push(PresetOp { preset: "flash".into(), layer: layer.id.clone(), at: span(v, word, time, t0, t1, f1).0, vars });
                out.notes.push(format!("shot {id}: black flash"));
            }
            "glow" if (!tracked || v.picture == "emblem") && !has_effect(layer, "glow") => {
                let c = v.colors.get("glow").copied().unwrap_or([1.0, 1.0, 1.0]);
                layer.effects.push(effect("glow", vec![("threshold", num(0.55)), ("radius", num(2.5)), ("intensity", num(1.0)), ("color", Prop::Const(linear(c)))], vec![]));
                out.notes.push(format!("shot {id}: glow"));
            }
            "light_rays" if !has_effect(layer, "light_rays") => {
                layer.effects.push(effect("light_rays", vec![], vec![]));
                out.notes.push(format!("shot {id}: light rays"));
            }
            "flare" if !has_effect(layer, "lens_flare") => {
                layer.effects.push(effect("lens_flare", vec![], vec![]));
                out.notes.push(format!("shot {id}: lens flare"));
            }
            _ => {}
        }
    }
    // ---- motion: only what the track did not measure ---------------------------------
    let has_scale = shot.curves.iter().any(|c| c.prop == "scale");
    let len = t1 - t0;
    if shot.shake.is_none() && !tracked && has(&v.motion, "shake") {
        let mut vars = Map::new();
        vars.insert("amplitude".into(), json!(0.012));
        out.presets.push(PresetOp { preset: "shake".into(), layer: layer.id.clone(), at: t0, vars });
        out.notes.push(format!("shot {id}: shake (seen)"));
    }
    if !(tracked && has_scale) && v.framing != "cube" {
        if has(&v.motion, "punch_in") {
            out.presets.push(PresetOp { preset: "punch".into(), layer: layer.id.clone(), at: t0, vars: Map::new() });
            out.notes.push(format!("shot {id}: punch (seen, not measured)"));
        } else if has(&v.motion, "push_in") {
            layer.transform.scale = Prop::Anim(Anim {
                keys: vec![Key { t: t0, v: Value::Num(1.0), ease: Ease::Name("linear".into()) }, Key { t: t1, v: Value::Num(1.12), ease: hold() }],
                base: None,
                modifiers: vec![],
            });
            if card && v.framing != "cube" {
                card_geometry(layer, shot, if v.framing == "tilted_card" { 0.78 } else { 0.72 });
            }
            out.notes.push(format!("shot {id}: push in (seen, not measured)"));
        } else if has(&v.motion, "zoom_out") {
            layer.transform.scale = ramp(t0, t1, Value::Num(1.18), Value::Num(1.0), "out");
            out.notes.push(format!("shot {id}: zoom out (seen, not measured)"));
        }
        if let Some(dir) = v.motion.iter().find_map(|m| m.strip_prefix("slide:")) {
            let mut vars = Map::new();
            let from = match dir {
                "left" => "from_right",
                "right" => "from_left",
                "up" => "from_bottom",
                _ => "from_top",
            };
            vars.insert("direction".into(), json!(from));
            vars.insert("length".into(), json!((len.min(0.5) * 100.0).round() / 100.0));
            out.presets.push(PresetOp { preset: "slide_in".into(), layer: layer.id.clone(), at: t0, vars });
            out.notes.push(format!("shot {id}: slide in ({from})"));
        }
        if has(&v.motion, "3d_turn") && v.framing != "tilted_card" {
            let mut vars = Map::new();
            vars.insert("axis".into(), json!(if axis == "x" { "x" } else { "y" }));
            vars.insert("length".into(), json!((len.min(0.5) * 100.0).round() / 100.0));
            out.presets.push(PresetOp { preset: "flip_3d".into(), layer: layer.id.clone(), at: t0, vars });
            out.notes.push(format!("shot {id}: flip in about {axis}"));
        }
    }
    // ---- overlays in the shot ------------------------------------------------------------
    for word in &v.overlays {
        match word.as_str() {
            "speed_lines" | "focus_lines" => {
                let mut vars = Map::new();
                vars.insert("mode".into(), json!(if word == "focus_lines" { "radial" } else { "horizontal" }));
                vars.insert("length".into(), json!((len * 100.0).round() / 100.0));
                out.presets.push(PresetOp { preset: "speed_lines".into(), layer: String::new(), at: t0, vars });
                out.notes.push(format!("shot {id}: {}", word.replace('_', " ")));
            }
            "screentone" if !has_effect(layer, "screentone") => {
                insert_colour(layer, effect("screentone", vec![("amount", num(0.5))], vec![]));
                out.notes.push(format!("shot {id}: screentone"));
            }
            "grain" if !has_effect(layer, "grain") => {
                layer.effects.push(effect("grain", vec![("amount", num(0.06))], vec![]));
                out.notes.push(format!("shot {id}: grain"));
            }
            "particles" => {
                let mut vars = Map::new();
                vars.insert("length".into(), json!((len * 100.0).round() / 100.0));
                out.presets.push(PresetOp { preset: "sparkles".into(), layer: String::new(), at: t0, vars });
                out.notes.push(format!("shot {id}: sparkles"));
            }
            _ => {}
        }
    }
    // ---- entry ---------------------------------------------------------------------------
    let transition = match v.entry.as_str() {
        "whip" => Some("whip_pan"),
        "zoom_through" => Some("cross_zoom"),
        "dissolve" => Some("fade"),
        "cube" => Some("cube"),
        "push" => Some("directional"),
        "glitch" => Some("glitch_displace"),
        _ => None,
    };
    if let (Some(kind), false, false) = (transition, first, tracked) {
        let mut vars = Map::new();
        vars.insert("duration".into(), json!(((len * 0.5).min(0.25) * 1000.0).round() / 1000.0));
        // Starting at the cut: the previous shot keeps its measured frames.
        vars.insert("align".into(), json!("after"));
        out.presets.push(PresetOp { preset: format!("transition:{kind}"), layer: layer.id.clone(), at: t0, vars });
        out.notes.push(format!("shot {id}: transition {kind} into it"));
    }
    if !tracked && v.entry == "flash_cut" && !shot.events.iter().any(|e| e.kind == "flash" && e.frame <= shot.f0 + 2) {
        out.presets.push(PresetOp { preset: "flash".into(), layer: layer.id.clone(), at: t0, vars: Map::new() });
        out.notes.push(format!("shot {id}: flash on the cut (seen, not measured)"));
    }
    let summary: Vec<String> = [v.picture.as_str(), v.framing.as_str()].iter().filter(|w| **w != "unsure" && **w != "full_frame").map(|w| w.to_string()).collect();
    if !summary.is_empty() {
        if let Some(n) = layer.name.as_mut() {
            n.push_str(&format!(" · vision: {}", summary.join(", ")));
        }
    }
    out
}

/// Apply the collected presets to the project document. Presets that fail
/// are skipped with a note (the draft stays valid without them).
pub fn apply_presets(doc: &mut Json, ops: &[PresetOp], notes: &mut Vec<String>) {
    for op in ops {
        let tc = edits_core::time::TimeCtx::from_doc(doc);
        let preset = match edits_core::preset::find(&op.preset, None) {
            Ok(p) => p,
            Err(e) => {
                notes.push(format!("preset {} skipped: {e}", op.preset));
                continue;
            }
        };
        let target = if op.layer.is_empty() { edits_core::preset::Target::None } else { edits_core::preset::Target::Layer(op.layer.clone()) };
        match edits_core::preset::apply(doc, &preset, &target, &[op.at], &op.vars, &tc) {
            Ok((next, _)) => *doc = next,
            Err(e) => notes.push(format!("preset {} on {} skipped: {e}", op.preset, if op.layer.is_empty() { "the project" } else { &op.layer })),
        }
    }
}

/// The vignette the label asks for when it disagrees with the measurement
/// (or nothing was measured): amount, radius, softness.
pub fn vignette_for(label: &str) -> Option<(f64, f64, f64)> {
    match label {
        "light" => Some((0.35, 0.45, 0.6)),
        "medium" => Some((0.6, 0.4, 0.6)),
        "strong" => Some((0.85, 0.3, 0.7)),
        "none" => Some((0.0, 0.75, 0.5)),
        _ => None,
    }
}
