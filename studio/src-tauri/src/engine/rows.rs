// Ported from `crates/edits-gui/src/rows.rs` (the egui window), unchanged but for
// imports, so Edits Studio and the old window edit projects the same way.

//! What the timeline draws for each layer: its span, its effects and the
//! times of every keyframe. Built once per accepted project, not per frame.

use edits_core::{Blend, Ease, Layer, LayerKind, Modifier, Project, Prop};

/// How a key is drawn: its shape says how the value leaves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyShape {
    /// `linear`: a diamond.
    Linear,
    /// `hold`: a square.
    Hold,
    /// Any other ease (curves, `decay`, `bezier`): a circle.
    Eased,
}

impl KeyShape {
    pub fn of(ease: &Ease) -> Self {
        match ease {
            Ease::Name(n) if n == "linear" => KeyShape::Linear,
            Ease::Name(n) if n == "hold" => KeyShape::Hold,
            _ => KeyShape::Eased,
        }
    }
}

/// One keyframe of one property.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyMark {
    /// Project time of the key.
    pub t: f64,
    /// Which property, e.g. `scale` or `glow.radius`.
    pub what: String,
    /// The key belongs to an effect parameter (otherwise to the transform or camera).
    pub effect: bool,
    /// The ease of the segment that starts at this key.
    pub shape: KeyShape,
}

/// One row of the timeline.
#[derive(Clone, Debug)]
pub struct Row {
    /// Index into `project.layers`; `None` for the camera row.
    pub layer: Option<usize>,
    pub id: String,
    /// `image a`, `cube`, `solid`, `adjustment`, `camera`.
    pub kind: String,
    pub start: f64,
    pub end: f64,
    pub enabled: bool,
    /// Effect kinds in order; a switched-off effect ends in ` (off)`.
    pub effects: Vec<String>,
    pub blend: Option<String>,
    /// Sorted by time.
    pub marks: Vec<KeyMark>,
    /// The transition the layer comes in with: (kind, duration).
    pub transition: Option<(String, f64)>,
}

/// The key times of an animatable value, in file order. The keys of a
/// keyframed shake amplitude (an envelope) count too.
pub fn prop_key_times(prop: &Prop) -> Vec<f64> {
    prop_keys(prop).into_iter().map(|(t, _)| t).collect()
}

/// The keys of an animatable value with their shapes, in file order (the
/// keys of a keyframed shake amplitude too).
pub fn prop_keys(prop: &Prop) -> Vec<(f64, KeyShape)> {
    let mut out = Vec::new();
    collect(prop, &mut out);
    out
}

fn collect(prop: &Prop, out: &mut Vec<(f64, KeyShape)>) {
    let Prop::Anim(anim) = prop else { return };
    out.extend(anim.keys.iter().map(|k| (k.t, KeyShape::of(&k.ease))));
    for m in &anim.modifiers {
        if let Modifier::Shake { amplitude, .. } = m {
            collect(amplitude, out);
        }
    }
}

fn push_marks(marks: &mut Vec<KeyMark>, prop: &Prop, what: &str, effect: bool) {
    for (t, shape) in prop_keys(prop) {
        if t.is_finite() {
            marks.push(KeyMark {
                t,
                what: what.to_string(),
                effect,
                shape,
            });
        }
    }
}

fn finish(mut marks: Vec<KeyMark>) -> Vec<KeyMark> {
    marks.sort_by(|a, b| a.t.total_cmp(&b.t));
    marks
}

/// Every keyframe of a layer: its transform, then each effect's parameters.
pub fn layer_marks(layer: &Layer) -> Vec<KeyMark> {
    let mut marks = Vec::new();
    let t = &layer.transform;
    for (name, prop) in [
        ("position", &t.position),
        ("scale", &t.scale),
        ("rotation", &t.rotation),
        ("anchor", &t.anchor),
        ("opacity", &t.opacity),
    ] {
        push_marks(&mut marks, prop, name, false);
    }
    for e in &layer.effects {
        for (name, prop) in &e.params {
            push_marks(&mut marks, prop, &format!("{}.{name}", e.kind), true);
        }
    }
    finish(marks)
}

pub fn camera_marks(p: &Project) -> Vec<KeyMark> {
    let mut marks = Vec::new();
    for (name, prop) in [
        ("fov", &p.camera.fov),
        ("position", &p.camera.position),
        ("rotation", &p.camera.rotation),
    ] {
        push_marks(&mut marks, prop, name, false);
    }
    finish(marks)
}

pub fn kind_text(kind: &LayerKind) -> String {
    match kind {
        LayerKind::Image { slot } => format!("image {slot}"),
        LayerKind::Solid { .. } => "solid".to_string(),
        LayerKind::Cube { .. } => "cube".to_string(),
        LayerKind::Adjustment => "adjustment".to_string(),
        LayerKind::Video(v) => format!("video {}", v.slot),
        LayerKind::Text(t) => {
            let words: String = t
                .text
                .chars()
                .take(24)
                .map(|c| if c == '\n' { ' ' } else { c })
                .collect();
            format!("text \"{words}\"")
        }
        LayerKind::Particles(p) => format!("particles {}", p.sprite),
        LayerKind::Comp(c) => format!("comp {}", c.comp),
        LayerKind::Shape(s) => format!("shape ({})", s.shapes.len()),
    }
}

pub fn blend_text(blend: Blend) -> String {
    blend.name().to_string()
}

/// The rows of the timeline, top of the picture first: the camera (only when
/// it is animated), then the layers from the last in the file (drawn on top)
/// to the first.
pub fn rows(p: &Project) -> Vec<Row> {
    let mut out = Vec::with_capacity(p.layers.len() + 1);
    let cam = camera_marks(p);
    if !cam.is_empty() {
        out.push(Row {
            layer: None,
            id: "camera".to_string(),
            kind: "camera".to_string(),
            start: 0.0,
            end: p.settings.duration,
            enabled: true,
            effects: Vec::new(),
            blend: None,
            marks: cam,
            transition: None,
        });
    }
    for (i, l) in p.layers.iter().enumerate().rev() {
        out.push(Row {
            layer: Some(i),
            id: l.id.clone(),
            kind: kind_text(&l.kind),
            start: l.start,
            end: l.end.unwrap_or(p.settings.duration),
            enabled: l.enabled,
            effects: l
                .effects
                .iter()
                .map(|e| {
                    if e.enabled {
                        e.kind.clone()
                    } else {
                        format!("{} (off)", e.kind)
                    }
                })
                .collect(),
            blend: (l.blend != Blend::Normal).then(|| blend_text(l.blend)),
            marks: layer_marks(l),
            transition: l.transition.as_ref().map(|t| (t.kind.clone(), t.duration)),
        });
    }
    out
}

/// A group of marks drawn as one symbol.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkGroup {
    pub t: f64,
    pub transform: bool,
    pub effect: bool,
    /// The properties keyed at this time, without repeats.
    pub what: Vec<String>,
    /// The shape of its keys; a diamond when they differ.
    pub shape: KeyShape,
}

/// Marks at the same time (within `tolerance`) become one group, so a time
/// keyed on five properties is one symbol with five names.
pub fn group_marks(marks: &[KeyMark], tolerance: f64) -> Vec<MarkGroup> {
    let mut out: Vec<MarkGroup> = Vec::new();
    for m in marks {
        match out.last_mut() {
            Some(g) if (m.t - g.t).abs() <= tolerance => {
                g.transform |= !m.effect;
                g.effect |= m.effect;
                if !g.what.contains(&m.what) {
                    g.what.push(m.what.clone());
                }
                if g.shape != m.shape {
                    g.shape = KeyShape::Linear;
                }
            }
            _ => out.push(MarkGroup {
                t: m.t,
                transform: !m.effect,
                effect: m.effect,
                what: vec![m.what.clone()],
                shape: m.shape,
            }),
        }
    }
    out
}
