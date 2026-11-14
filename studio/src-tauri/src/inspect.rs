//! What the inspector shows for a layer (or the camera) at the playhead:
//! every property with its value there, whether the file sets it, its keys,
//! and the unit, range and default the effect registry gives it.

use crate::engine::editing::{self, PropRef, CAMERA_PROPS, TRANSFORM_PROPS};
use crate::project::kind_tag;
use edits_core::effects::{self, Unit};
use edits_core::{Evaluator, Layer, LayerKind, Modifier, Project, Prop};
use serde::{Deserialize, Serialize};

/// A property as the frontend names it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PropKey {
    Transform {
        name: String,
    },
    Effect {
        index: usize,
        kind: String,
        param: String,
    },
    Camera {
        name: String,
    },
}

impl PropKey {
    pub fn to_ref(&self) -> Result<PropRef, String> {
        let find = |list: &[&'static str], n: &str| {
            list.iter()
                .copied()
                .find(|x| *x == n)
                .ok_or_else(|| format!("no property {n}"))
        };
        Ok(match self {
            PropKey::Transform { name } => PropRef::Transform(find(&TRANSFORM_PROPS, name)?),
            PropKey::Effect { index, kind, param } => PropRef::Effect {
                index: *index,
                kind: kind.clone(),
                param: param.clone(),
            },
            PropKey::Camera { name } => PropRef::Camera(find(&CAMERA_PROPS, name)?),
        })
    }

    pub fn of(r: &PropRef) -> Self {
        match r {
            PropRef::Transform(n) => PropKey::Transform {
                name: n.to_string(),
            },
            PropRef::Effect { index, kind, param } => PropKey::Effect {
                index: *index,
                kind: kind.clone(),
                param: param.clone(),
            },
            PropRef::Camera(n) => PropKey::Camera {
                name: n.to_string(),
            },
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct KeyDto {
    pub t: f64,
    pub v: Vec<f64>,
    /// The ease of the segment that starts at this key (`linear`, `decay`, `bezier`...).
    pub ease: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct PropDto {
    pub key: PropKey,
    /// `scale`, `radius`, `fov`.
    pub name: String,
    pub pointer: String,
    /// What it evaluates to at the playhead, shake included.
    pub value: Vec<f64>,
    /// Without the shake: what editing changes.
    pub base: Vec<f64>,
    /// The file sets it (otherwise the default applies).
    pub set: bool,
    pub animated: bool,
    pub shake: bool,
    pub looped: bool,
    pub keys: Vec<KeyDto>,
    /// Index of the key at the playhead.
    pub key_here: Option<usize>,
    /// Index of the key whose segment the playhead is in.
    pub segment: Option<usize>,
    /// The registry's unit: `""`, `%h`, `deg`, `pos`, `color`, `x`, `time`, `count`, `stops`, `px1080`.
    pub unit: String,
    pub range: Option<(f64, f64)>,
    pub default: Vec<f64>,
    pub doc: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct OptionDto {
    pub name: String,
    pub values: Vec<String>,
    pub value: String,
    pub doc: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct EffectDto {
    pub index: usize,
    pub kind: String,
    pub category: String,
    pub doc: String,
    pub enabled: bool,
    pub known: bool,
    pub params: Vec<PropDto>,
    pub options: Vec<OptionDto>,
}

#[derive(Serialize, Clone, Debug)]
pub struct TransitionDto {
    pub kind: String,
    pub duration: f64,
    pub ease: String,
    pub doc: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct LayerInfo {
    pub id: String,
    pub index: usize,
    pub kind: String,
    pub label: String,
    pub name: Option<String>,
    pub start: f64,
    pub end: Option<f64>,
    pub enabled: bool,
    pub blend: String,
    pub visible_now: bool,
    pub slot: Option<String>,
    pub color: Option<[f32; 4]>,
    pub transition: Option<TransitionDto>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Inspection {
    pub t: f64,
    pub layer: Option<LayerInfo>,
    pub transform: Vec<PropDto>,
    pub effects: Vec<EffectDto>,
    pub camera: Vec<PropDto>,
}

fn ease_text(e: &edits_core::Ease) -> String {
    editing::ease_name(e)
}

fn prop_info(r: &PropRef) -> (Unit, Option<(f64, f64)>, String) {
    match r {
        PropRef::Transform(n) => match *n {
            "position" => (Unit::Position, None, "Position [x, y, z] in composition units (height = 1, origin at the centre, y down).".into()),
            "scale" => (Unit::Factor, Some((0.0, f64::INFINITY)), "Scale: one number, or [x, y, z].".into()),
            "rotation" => (Unit::Degrees, None, "Rotation [x, y, z] in degrees; z turns in the picture's plane, clockwise.".into()),
            "anchor" => (Unit::Position, None, "The point the layer scales and turns about, [x, y] in composition units.".into()),
            _ => (Unit::Number, Some((0.0, 1.0)), "Opacity, 0 to 1.".into()),
        },
        PropRef::Effect { kind, param, .. } => match effects::spec(kind).and_then(|s| s.param(param)) {
            Some(p) => (p.unit, p.range, p.doc.to_string()),
            None => (Unit::Number, None, String::new()),
        },
        PropRef::Camera(n) => match *n {
            "fov" => (Unit::Degrees, Some((1.0, 179.0)), "Vertical field of view in degrees.".into()),
            "position" => (Unit::Position, None, "Camera offset [x, y, z] in composition units.".into()),
            _ => (Unit::Degrees, None, "Camera rotation [x, y, z] in degrees.".into()),
        },
    }
}

fn prop_dto(p: &Project, layer: Option<&Layer>, r: &PropRef, t: f64) -> PropDto {
    let prop = r.get(p, layer);
    let default = r.default_value();
    let ev = Evaluator::new(p);
    let value = match prop {
        Some(prop) => {
            let v = ev.prop(prop, t);
            if v.is_empty() {
                default.clone()
            } else {
                v
            }
        }
        None => default.clone(),
    };
    let base = editing::base_value(p, prop, &default, t);
    let (unit, range, doc) = prop_info(r);
    let (shake, looped) = match prop {
        Some(Prop::Anim(a)) => (
            a.modifiers
                .iter()
                .any(|m| matches!(m, Modifier::Shake { .. })),
            a.modifiers
                .iter()
                .any(|m| matches!(m, Modifier::Loop { .. })),
        ),
        _ => (false, false),
    };
    let keys = editing::keys_of(prop)
        .iter()
        .map(|k| KeyDto {
            t: k.t,
            v: k.v.to_vec(),
            ease: ease_text(&k.ease),
        })
        .collect::<Vec<_>>();
    PropDto {
        key: PropKey::of(r),
        name: match r {
            PropRef::Transform(n) | PropRef::Camera(n) => n.to_string(),
            PropRef::Effect { param, .. } => param.clone(),
        },
        pointer: r.pointer(layer.map(|l| l.id.as_str()).unwrap_or("")),
        value,
        base,
        set: prop.is_some(),
        animated: !keys.is_empty(),
        shake,
        looped,
        key_here: editing::key_index_at(prop, t),
        segment: editing::key_at_or_before(prop, t),
        keys,
        unit: unit.as_str().to_string(),
        range,
        default,
        doc,
    }
}

/// The inspector's content for `layer` (or, with `None`, the camera) at project time `t`.
pub fn inspect(p: &Project, layer: Option<&str>, t: f64) -> Result<Inspection, String> {
    let camera = || {
        CAMERA_PROPS
            .iter()
            .map(|n| prop_dto(p, None, &PropRef::Camera(n), t))
            .collect::<Vec<_>>()
    };
    let Some(id) = layer else {
        return Ok(Inspection {
            t,
            layer: None,
            transform: Vec::new(),
            effects: Vec::new(),
            camera: camera(),
        });
    };
    let (index, l) = p
        .layers
        .iter()
        .enumerate()
        .find(|(_, l)| l.id == id)
        .ok_or_else(|| format!("there is no layer \"{id}\" any more"))?;
    let transform = TRANSFORM_PROPS
        .iter()
        .map(|n| prop_dto(p, Some(l), &PropRef::Transform(n), t))
        .collect();
    let effects = l
        .effects
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let spec = effects::spec(&e.kind);
            let names: Vec<String> = match spec {
                Some(s) => s.params.iter().map(|p| p.name.to_string()).collect(),
                None => e.params.keys().cloned().collect(),
            };
            EffectDto {
                index: i,
                kind: e.kind.clone(),
                category: spec.map(|s| s.category.to_string()).unwrap_or_default(),
                doc: spec.map(|s| s.doc.to_string()).unwrap_or_default(),
                enabled: e.enabled,
                known: spec.is_some(),
                params: names
                    .into_iter()
                    .map(|param| {
                        prop_dto(
                            p,
                            Some(l),
                            &PropRef::Effect {
                                index: i,
                                kind: e.kind.clone(),
                                param,
                            },
                            t,
                        )
                    })
                    .collect(),
                options: spec
                    .map(|s| {
                        s.options
                            .iter()
                            .map(|o| OptionDto {
                                name: o.name.into(),
                                values: o.values.iter().map(|v| v.to_string()).collect(),
                                value: e
                                    .options
                                    .get(o.name)
                                    .cloned()
                                    .unwrap_or_else(|| o.default().to_string()),
                                doc: o.doc.into(),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect();
    let ev = Evaluator::new(p);
    let info = LayerInfo {
        id: l.id.clone(),
        index,
        kind: kind_tag(&l.kind).into(),
        label: crate::engine::rows::kind_text(&l.kind),
        name: l.name.clone(),
        start: l.start,
        end: l.end,
        enabled: l.enabled,
        blend: l.blend.name().into(),
        visible_now: ev.active(l, t),
        slot: match &l.kind {
            LayerKind::Image { slot } => Some(slot.clone()),
            LayerKind::Video(v) => Some(v.slot.clone()),
            _ => None,
        },
        color: match &l.kind {
            LayerKind::Solid { color } => Some(*color),
            _ => None,
        },
        transition: l.transition.as_ref().map(|tr| TransitionDto {
            kind: tr.kind.clone(),
            duration: tr.duration,
            ease: ease_text(&tr.ease),
            doc: effects::transition(&tr.kind)
                .map(|s| s.doc.to_string())
                .unwrap_or_default(),
        }),
    };
    Ok(Inspection {
        t,
        layer: Some(info),
        transform,
        effects,
        camera: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project::from_json(crate::project::tests::PROJECT).unwrap()
    }

    #[test]
    fn a_layer_shows_its_values_at_the_playhead_with_units_and_keys() {
        let p = project();
        let i = inspect(&p, Some("a"), 0.5).unwrap();
        let l = i.layer.unwrap();
        assert_eq!(
            (l.id.as_str(), l.kind.as_str(), l.visible_now),
            ("a", "image", true)
        );
        let scale = i.transform.iter().find(|p| p.name == "scale").unwrap();
        assert!(scale.animated && scale.set);
        assert_eq!(scale.keys.len(), 2);
        assert_eq!(scale.keys[0].ease, "hold");
        assert_eq!(scale.value, vec![1.3], "held until the next key");
        assert_eq!(scale.segment, Some(0));
        assert_eq!(scale.key_here, None);
        assert_eq!(scale.unit, "x");
        let opacity = i.transform.iter().find(|p| p.name == "opacity").unwrap();
        assert_eq!(
            (opacity.value.clone(), opacity.range),
            (vec![1.0], Some((0.0, 1.0)))
        );
    }

    #[test]
    fn effect_parameters_come_from_the_registry_with_defaults() {
        let p = project();
        let i = inspect(&p, Some("finish"), 2.0).unwrap();
        let v = &i.effects[0];
        assert_eq!(v.kind, "vignette");
        assert!(v.known && v.enabled);
        let amount = v.params.iter().find(|p| p.name == "amount").unwrap();
        assert!(!amount.set, "the file does not set it");
        assert_eq!(amount.value, amount.default);
        assert_eq!(amount.pointer, "/layers/@finish/effects/0/params/amount");
        assert_eq!(PropKey::of(&amount.key.to_ref().unwrap()), amount.key);
    }

    #[test]
    fn without_a_layer_the_camera_is_shown() {
        let p = project();
        let i = inspect(&p, None, 0.0).unwrap();
        assert!(i.layer.is_none());
        assert_eq!(
            i.camera.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["fov", "position", "rotation"]
        );
        assert!(inspect(&p, Some("nope"), 0.0).is_err());
    }
}
