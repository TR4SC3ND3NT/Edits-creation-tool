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
            marks.push(KeyMark { t, what: what.to_string(), effect, shape });
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
    for (name, prop) in [("position", &t.position), ("scale", &t.scale), ("rotation", &t.rotation), ("anchor", &t.anchor), ("opacity", &t.opacity)] {
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
    for (name, prop) in [("fov", &p.camera.fov), ("position", &p.camera.position), ("rotation", &p.camera.rotation)] {
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
            let words: String = t.text.chars().take(24).map(|c| if c == '\n' { ' ' } else { c }).collect();
            format!("text \"{words}\"")
        }
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
            effects: l.effects.iter().map(|e| if e.enabled { e.kind.clone() } else { format!("{} (off)", e.kind) }).collect(),
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
            _ => out.push(MarkGroup { t: m.t, transform: !m.effect, effect: m.effect, what: vec![m.what.clone()], shape: m.shape }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project::from_json(
            r#"{"format":"edits.project/1",
                "settings":{"width":64,"height":64,"fps":25,"duration":8,"time_base":"beats","bpm":120},
                "slots":{"a":{},"b":{}},
                "camera":{"position":{"keys":[{"t":2,"v":[0,0,0]},{"t":6,"v":[0,0,0.5]}]}},
                "layers":[
                  {"id":"bg","kind":"solid","color":[0,0,0,1]},
                  {"id":"pic","kind":"image","slot":"a","in":1,"out":5,"blend":"screen",
                   "transform":{
                     "scale":{"keys":[{"t":1,"v":1.2},{"t":2,"v":1.0}]},
                     "opacity":{"keys":[{"t":4,"v":1},{"t":5,"v":0}]},
                     "position":{"base":[0,0,0],"modifiers":[{"kind":"shake",
                        "amplitude":{"keys":[{"t":1,"v":0.02},{"t":1.5,"v":0}]}}]}},
                   "effects":[
                     {"kind":"glow","params":{"radius":{"keys":[{"t":1,"v":6},{"t":3,"v":0}]},"intensity":2}},
                     {"kind":"rgb_split","enabled":false,"params":{"amount":{"keys":[{"t":2,"v":1}]}}}]},
                  {"id":"grade","kind":"adjustment","in":4,
                   "effects":[{"kind":"exposure","params":{"stops":{"keys":[{"t":4,"v":4},{"t":4.5,"v":0}]}}}]}
                ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn collects_the_key_times_of_a_property() {
        let constant: Prop = serde_json::from_str("[1, 2, 3]").unwrap();
        assert!(prop_key_times(&constant).is_empty());
        let keyed: Prop = serde_json::from_str(r#"{"keys":[{"t":0,"v":1},{"t":2.5,"v":2,"ease":"hold"},{"t":4,"v":0}]}"#).unwrap();
        assert_eq!(prop_key_times(&keyed), vec![0.0, 2.5, 4.0]);
        // A shake with a keyframed amplitude has the envelope's keys.
        let shaken: Prop = serde_json::from_str(
            r#"{"keys":[{"t":1,"v":0}],"modifiers":[{"kind":"shake","amplitude":{"keys":[{"t":3,"v":0.1},{"t":4,"v":0}]}},{"kind":"loop","period":2}]}"#,
        )
        .unwrap();
        assert_eq!(prop_key_times(&shaken), vec![1.0, 3.0, 4.0]);
    }

    #[test]
    fn layer_marks_cover_transform_and_effects_in_time_order() {
        let p = project();
        let marks = layer_marks(&p.layers[1]);
        let times: Vec<f64> = marks.iter().map(|m| m.t).collect();
        assert_eq!(times, vec![1.0, 1.0, 1.0, 1.5, 2.0, 2.0, 3.0, 4.0, 5.0]);
        let at = |t: f64| -> Vec<&str> { marks.iter().filter(|m| m.t == t).map(|m| m.what.as_str()).collect() };
        assert_eq!(at(1.0), vec!["position", "scale", "glow.radius"]);
        assert_eq!(at(1.5), vec!["position"]);
        assert_eq!(at(2.0), vec!["scale", "rgb_split.amount"]);
        assert_eq!(at(3.0), vec!["glow.radius"]);
        assert!(marks.iter().all(|m| m.effect == m.what.contains('.')));
        // A layer with nothing animated has no marks.
        assert!(layer_marks(&p.layers[0]).is_empty());
    }

    #[test]
    fn rows_run_from_the_top_layer_down() {
        let p = project();
        let rows = rows(&p);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["camera", "grade", "pic", "bg"]);
        assert_eq!(rows[0].layer, None);
        assert_eq!(rows[0].marks.len(), 2);
        assert_eq!(rows[1].layer, Some(2));
        assert_eq!((rows[1].start, rows[1].end), (4.0, 8.0), "no `out` means the end of the project");
        assert_eq!(rows[1].kind, "adjustment");
        assert_eq!(rows[2].kind, "image a");
        assert_eq!((rows[2].start, rows[2].end), (1.0, 5.0));
        assert_eq!(rows[2].effects, vec!["glow", "rgb_split (off)"]);
        assert_eq!(rows[2].blend.as_deref(), Some("screen"));
        assert_eq!(rows[3].blend, None);
        assert_eq!(rows[3].kind, "solid");
    }

    #[test]
    fn a_still_camera_has_no_row() {
        let mut p = project();
        p.camera = Default::default();
        assert_eq!(rows(&p).len(), 3);
        assert!(camera_marks(&p).is_empty());
    }

    #[test]
    fn marks_at_one_time_are_one_group() {
        let p = project();
        let groups = group_marks(&layer_marks(&p.layers[1]), 1e-6);
        let times: Vec<f64> = groups.iter().map(|g| g.t).collect();
        assert_eq!(times, vec![1.0, 1.5, 2.0, 3.0, 4.0, 5.0]);
        assert!(groups[0].transform && groups[0].effect);
        assert_eq!(groups[0].what, vec!["position", "scale", "glow.radius"]);
        assert!(groups[1].transform && !groups[1].effect);
        assert!(!groups[3].transform && groups[3].effect);
        // A wide tolerance (a zoomed-out timeline) merges neighbours.
        let coarse = group_marks(&layer_marks(&p.layers[1]), 0.6);
        assert_eq!(coarse.len(), 5);
        assert_eq!(coarse[0].what, vec!["position", "scale", "glow.radius"]);
    }

    #[test]
    fn keys_are_shaped_by_their_ease() {
        let prop: Prop = serde_json::from_str(r#"{"keys":[{"t":0,"v":1},{"t":1,"v":2,"ease":"hold"},{"t":2,"v":0,"ease":"quad_out"},{"t":3,"v":1,"ease":[0.2,0,0.4,1]},{"t":4,"v":1,"ease":{"name":"decay","rate":0.4}}]}"#).unwrap();
        let shapes: Vec<KeyShape> = prop_keys(&prop).into_iter().map(|(_, s)| s).collect();
        assert_eq!(shapes, vec![KeyShape::Linear, KeyShape::Hold, KeyShape::Eased, KeyShape::Eased, KeyShape::Eased]);
        // One group of keys with different shapes is drawn as a diamond.
        let marks = vec![
            KeyMark { t: 1.0, what: "scale".into(), effect: false, shape: KeyShape::Hold },
            KeyMark { t: 1.0, what: "opacity".into(), effect: false, shape: KeyShape::Eased },
            KeyMark { t: 2.0, what: "scale".into(), effect: false, shape: KeyShape::Hold },
        ];
        let groups = group_marks(&marks, 1e-6);
        assert_eq!(groups[0].shape, KeyShape::Linear);
        assert_eq!(groups[1].shape, KeyShape::Hold);
    }
}
