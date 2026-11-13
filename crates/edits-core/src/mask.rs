//! Masks and track mattes: what limits where a layer shows.
//!
//! A **mask** is a shape in the layer's own space (composition units, origin
//! at the layer's origin, y down, so a mask moves, scales and turns with the
//! layer). Each mask gives a coverage between 0 and 1 at every point: 1
//! inside the shape, 0 outside, with an edge that fades over `feather`.
//! The masks of a layer are combined top to bottom by their `mode` and the
//! result multiplies the layer's alpha. On an adjustment layer the masks
//! limit where its effects apply.
//!
//! A **track matte** takes another layer's picture at the same instant (with
//! its own transform, masks and effects) and multiplies this layer's alpha
//! by that picture's alpha or luminance. The matte source is usually hidden
//! with `"visible": false`, which keeps it working as a matte.
//!
//! The semantics follow After Effects, as EffectCraft implements them
//! (https://github.com/storytold/effectcraft, MIT OR Apache-2.0, used under
//! MIT; `crates/render/src/masks.rs` and the `MatteKind` of
//! `crates/render/src/layers.rs`): the mode formulas, the accumulator that
//! starts full when the first mask subtracts, a first `intersect` acting as
//! `add`, inversion before opacity, and luma mattes on the matte's colour
//! over black. The code here is our own; edges are signed distances
//! evaluated per pixel instead of rasterised paths blurred by box filters.

use crate::check::{Finding, Severity};
use crate::model::{Layer, LayerKind, Project, Prop, Value};
use crate::Evaluator;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most points a polygon mask can have (they travel in one uniform block).
pub const MAX_POLYGON_POINTS: usize = 32;

/// One mask of a layer.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Mask {
    pub shape: MaskShape,
    /// For `polygon`: the corners, `[x, y]` in layer space (composition
    /// units), at least 3, at most [`MAX_POLYGON_POINTS`]. They are scaled
    /// by `size`, turned by `rotation` and moved by `position`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f64; 2]>,
    /// For `rounded_rect`: the corner radius in composition units (layer space).
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub radius: Prop,
    /// Centre of the shape in layer space (composition units).
    #[serde(default = "zero2")]
    pub position: Prop,
    /// Width and height of a rect, ellipse or rounded rect in composition
    /// units (one number: a square or a circle); for a polygon, a scale of
    /// its points.
    #[serde(default = "one2")]
    pub size: Prop,
    /// Degrees, clockwise on screen, about `position`.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub rotation: Prop,
    /// Width of the soft edge in percent of the composition height (in
    /// layer space), centred on the shape's outline.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub feather: Prop,
    /// Grows (positive) or shrinks (negative) the shape by this much, in
    /// percent of the composition height (in layer space).
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub expansion: Prop,
    /// 0..1: how strongly the mask counts.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: Prop,
    #[serde(default, skip_serializing_if = "MaskMode::is_add")]
    pub mode: MaskMode,
    /// Use the outside of the shape instead of the inside.
    #[serde(default, alias = "invert", skip_serializing_if = "is_false")]
    pub inverted: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaskShape {
    Rect,
    Ellipse,
    Polygon,
    RoundedRect,
}

impl MaskShape {
    pub fn name(self) -> &'static str {
        match self {
            MaskShape::Rect => "rect",
            MaskShape::Ellipse => "ellipse",
            MaskShape::Polygon => "polygon",
            MaskShape::RoundedRect => "rounded_rect",
        }
    }
}

/// How a mask combines with the masks above it in the list (`a` is the
/// coverage so far, `m` this mask's).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    /// Union: `a + m - a m`.
    #[default]
    Add,
    /// Cut out: `a (1 - m)`. A first mask that subtracts starts from a full layer.
    Subtract,
    /// Keep the overlap: `a m`. A first mask that intersects acts as `add`.
    Intersect,
    /// `|a - m|`.
    Difference,
}

impl MaskMode {
    fn is_add(&self) -> bool {
        *self == MaskMode::Add
    }
    pub fn name(self) -> &'static str {
        match self {
            MaskMode::Add => "add",
            MaskMode::Subtract => "subtract",
            MaskMode::Intersect => "intersect",
            MaskMode::Difference => "difference",
        }
    }
    /// The coverage before the first mask: full when it subtracts or
    /// intersects (so a first `intersect` acts as `add`), empty otherwise.
    pub fn start(first: MaskMode) -> f64 {
        match first {
            MaskMode::Subtract | MaskMode::Intersect => 1.0,
            MaskMode::Add | MaskMode::Difference => 0.0,
        }
    }
    /// The coverage after this mode combines `m` into `a`.
    pub fn combine(self, a: f64, m: f64) -> f64 {
        match self {
            MaskMode::Add => a + m - a * m,
            MaskMode::Subtract => a * (1.0 - m),
            MaskMode::Intersect => a * m,
            MaskMode::Difference => (a - m).abs(),
        }
    }
}

/// A track matte: another layer whose picture limits where this one shows.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Matte {
    /// The id of the matte source layer.
    pub layer: String,
    #[serde(default)]
    pub mode: MatteMode,
}

/// Which part of the matte source's picture is used.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatteMode {
    /// Show where the source is opaque.
    #[default]
    Alpha,
    /// Show where the source is transparent.
    AlphaInverted,
    /// Show where the source is bright: the Rec. 709 luminance of its
    /// display (sRGB) colour, times its alpha (its colour over black).
    Luma,
    /// Show where the source is dark (and where it is transparent).
    LumaInverted,
}

impl MatteMode {
    pub fn name(self) -> &'static str {
        match self {
            MatteMode::Alpha => "alpha",
            MatteMode::AlphaInverted => "alpha_inverted",
            MatteMode::Luma => "luma",
            MatteMode::LumaInverted => "luma_inverted",
        }
    }
    /// The factor a matte pixel gives, from its straight sRGB colour and its
    /// alpha (both 0..1) and the source layer's opacity.
    pub fn factor(self, srgb: [f64; 3], alpha: f64, opacity: f64) -> f64 {
        let luma = 0.2126 * srgb[0] + 0.7152 * srgb[1] + 0.0722 * srgb[2];
        let k = match self {
            MatteMode::Alpha => alpha * opacity,
            MatteMode::AlphaInverted => 1.0 - alpha * opacity,
            MatteMode::Luma => luma * alpha * opacity,
            MatteMode::LumaInverted => 1.0 - luma * alpha * opacity,
        };
        k.clamp(0.0, 1.0)
    }
}

/// A mask evaluated at an instant. Lengths are in composition units of
/// layer space (feather and expansion converted from percent).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedMask {
    pub shape: MaskShape,
    pub mode: MaskMode,
    pub inverted: bool,
    pub position: [f64; 2],
    pub size: [f64; 2],
    /// Degrees.
    pub rotation: f64,
    pub radius: f64,
    pub feather: f64,
    pub expansion: f64,
    pub opacity: f64,
    /// Polygon corners scaled by `size` (before rotation and translation).
    pub points: Vec<[f64; 2]>,
}

impl ResolvedMask {
    /// Every value as raw bits, for signatures of what a frame depends on.
    pub fn bits(&self) -> Vec<u64> {
        let mut out = vec![self.shape as u64, self.mode as u64, self.inverted as u64];
        out.extend(
            [self.position[0], self.position[1], self.size[0], self.size[1], self.rotation, self.radius, self.feather, self.expansion, self.opacity]
                .iter()
                .map(|v| v.to_bits()),
        );
        out.extend(self.points.iter().flat_map(|p| [p[0].to_bits(), p[1].to_bits()]));
        out
    }

    /// Signed distance (negative inside) from `p` (layer space) to the
    /// shape grown by `expansion`. Exact for rects, rounded rects and
    /// polygons; an approximation for ellipses that is exact on the outline
    /// and for circles. The renderer evaluates the same formulas in WGSL.
    pub fn distance(&self, p: [f64; 2]) -> f64 {
        let (s, c) = (-self.rotation.to_radians()).sin_cos();
        let d = [p[0] - self.position[0], p[1] - self.position[1]];
        let q = [d[0] * c - d[1] * s, d[0] * s + d[1] * c];
        let half = [self.size[0].abs() * 0.5, self.size[1].abs() * 0.5];
        let raw = match self.shape {
            MaskShape::Rect => sd_box(q, half, 0.0),
            MaskShape::RoundedRect => sd_box(q, half, self.radius),
            MaskShape::Ellipse => sd_ellipse(q, half),
            MaskShape::Polygon => sd_polygon(q, &self.points),
        };
        raw - self.expansion
    }

    /// Coverage 0..1 at `p`: a linear ramp across the outline, `feather` wide, or
    /// one `pixel` wide without feather (anti-aliasing), the two combined as
    /// `hypot(feather, pixel)` (before inversion and opacity).
    pub fn coverage(&self, p: [f64; 2], pixel: f64) -> f64 {
        let w = self.feather.hypot(pixel).max(1e-9);
        (0.5 - self.distance(p) / w).clamp(0.0, 1.0)
    }
}

fn sd_box(q: [f64; 2], half: [f64; 2], radius: f64) -> f64 {
    let r = radius.clamp(0.0, half[0].min(half[1]));
    let d = [q[0].abs() - half[0] + r, q[1].abs() - half[1] + r];
    let outside = (d[0].max(0.0)).hypot(d[1].max(0.0));
    outside + d[0].max(d[1]).min(0.0) - r
}

fn sd_ellipse(q: [f64; 2], r: [f64; 2]) -> f64 {
    let r = [r[0].max(1e-9), r[1].max(1e-9)];
    let k0 = (q[0] / r[0]).hypot(q[1] / r[1]);
    let k1 = (q[0] / (r[0] * r[0])).hypot(q[1] / (r[1] * r[1]));
    if k1 < 1e-12 {
        return -r[0].min(r[1]);
    }
    k0 * (k0 - 1.0) / k1
}

fn sd_polygon(p: [f64; 2], v: &[[f64; 2]]) -> f64 {
    if v.len() < 3 {
        return 1e9;
    }
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
    let mut d = dot(sub(p, v[0]), sub(p, v[0]));
    let mut s = 1.0;
    let mut j = v.len() - 1;
    for i in 0..v.len() {
        let e = sub(v[j], v[i]);
        let w = sub(p, v[i]);
        let t = (dot(w, e) / dot(e, e).max(1e-18)).clamp(0.0, 1.0);
        let b = [w[0] - e[0] * t, w[1] - e[1] * t];
        d = d.min(dot(b, b));
        let c = [p[1] >= v[i][1], p[1] < v[j][1], e[0] * w[1] > e[1] * w[0]];
        if c.iter().all(|x| *x) || c.iter().all(|x| !*x) {
            s = -s;
        }
        j = i;
    }
    s * d.sqrt()
}

impl Evaluator<'_> {
    /// A mask's values at `t` (the layer's time).
    pub fn mask(&self, m: &Mask, t: f64) -> ResolvedMask {
        let size = match self.prop(&m.size, t).as_slice() {
            [] => [1.0, 1.0],
            [s] => [*s, *s],
            [x, y, ..] => [*x, *y],
        };
        ResolvedMask {
            shape: m.shape,
            mode: m.mode,
            inverted: m.inverted,
            position: self.vec2(&m.position, t),
            size,
            rotation: self.num(&m.rotation, t),
            radius: self.num(&m.radius, t).max(0.0),
            feather: self.num(&m.feather, t).max(0.0) / 100.0,
            expansion: self.num(&m.expansion, t) / 100.0,
            opacity: self.num(&m.opacity, t).clamp(0.0, 1.0),
            points: if m.shape == MaskShape::Polygon { m.points.iter().map(|p| [p[0] * size[0], p[1] * size[1]]).collect() } else { Vec::new() },
        }
    }

    /// The masks of `layer` at `t` (its time), in order.
    pub fn masks(&self, layer: &Layer, t: f64) -> Vec<ResolvedMask> {
        layer.masks.iter().map(|m| self.mask(m, t)).collect()
    }
}

/// The combined coverage of `masks` at `p` (layer space): the CPU twin of
/// the renderer's mask passes, used by tests and tools. `pixel` is the
/// width of one pixel in layer units (the anti-aliased edge).
pub fn coverage(masks: &[ResolvedMask], p: [f64; 2], pixel: f64) -> f64 {
    let Some(first) = masks.first() else { return 1.0 };
    let mut a = MaskMode::start(first.mode);
    for m in masks {
        let mut c = m.coverage(p, pixel);
        if m.inverted {
            c = 1.0 - c;
        }
        a = m.mode.combine(a, c * m.opacity).clamp(0.0, 1.0);
    }
    a
}

/// The layer `id` names among `layers` (first) or the project's layers.
pub fn find_layer<'a>(layers: &'a [Layer], project: &'a Project, id: &str) -> Option<&'a Layer> {
    layers.iter().find(|l| l.id == id).or_else(|| project.layers.iter().find(|l| l.id == id))
}

/// Checks of one layer's masks and matte (the matte's source and cycles
/// are checked for the whole project by [`check_mattes`]).
pub fn check_layer(l: &Layer, lp: &str, err: &mut impl FnMut(String, String, Option<&str>), warn: &mut Vec<Finding>) {
    let values = |prop: &Prop| -> Vec<f64> {
        match prop {
            Prop::Const(v) => v.to_vec(),
            Prop::Anim(a) => a.keys.iter().flat_map(|k| k.v.to_vec()).chain(a.base.iter().flat_map(Value::to_vec)).collect(),
        }
    };
    for (i, m) in l.masks.iter().enumerate() {
        let mp = format!("{lp}.masks[{i}]");
        let shape = m.shape.name();
        match m.shape {
            MaskShape::Polygon if m.points.len() < 3 => {
                err(format!("{mp}.points"), format!("a polygon mask needs at least 3 points, got {}", m.points.len()), Some("[[x, y], ...] in layer space (composition units, origin at the layer's centre, y down)"));
            }
            MaskShape::Polygon if m.points.len() > MAX_POLYGON_POINTS => {
                err(format!("{mp}.points"), format!("a polygon mask takes at most {MAX_POLYGON_POINTS} points, got {}", m.points.len()), Some("simplify the outline, or split it into several masks with mode add"));
            }
            MaskShape::Polygon if m.points.iter().flatten().any(|v| !v.is_finite()) => {
                err(format!("{mp}.points"), "polygon points must be finite numbers".into(), None);
            }
            MaskShape::Polygon => {}
            _ if !m.points.is_empty() => {
                warn.push(Finding { severity: Severity::Warning, path: format!("{mp}.points"), message: format!("a {shape} mask ignores \"points\""), hint: Some("use \"shape\": \"polygon\"".into()) });
            }
            _ => {}
        }
        if m.shape != MaskShape::RoundedRect && !is_zero(&m.radius) {
            warn.push(Finding { severity: Severity::Warning, path: format!("{mp}.radius"), message: format!("a {shape} mask ignores \"radius\""), hint: Some("use \"shape\": \"rounded_rect\"".into()) });
        }
        for (name, prop) in [("position", &m.position), ("size", &m.size), ("rotation", &m.rotation), ("radius", &m.radius), ("feather", &m.feather), ("expansion", &m.expansion), ("opacity", &m.opacity)] {
            crate::check::check_prop(prop, &format!("{mp}.{name}"), err);
            if let Some(bad) = values(prop).into_iter().find(|v| !v.is_finite()) {
                err(format!("{mp}.{name}"), format!("{name} must be a finite number, got {bad}"), None);
            }
        }
        if let Some(bad) = values(&m.feather).into_iter().find(|v| *v < 0.0) {
            err(format!("{mp}.feather"), format!("feather must be 0 or more, got {bad}"), Some("percent of the composition height; use expansion to shrink the shape"));
        }
        if let Some(bad) = values(&m.radius).into_iter().find(|v| *v < 0.0) {
            err(format!("{mp}.radius"), format!("radius must be 0 or more, got {bad}"), Some("composition units (the frame is 1 high)"));
        }
        if let Some(bad) = values(&m.opacity).into_iter().find(|v| !(0.0..=1.0).contains(v)) {
            warn.push(Finding { severity: Severity::Warning, path: format!("{mp}.opacity"), message: format!("{bad} is outside 0..1; the renderer clamps it"), hint: None });
        }
    }
    if matches!(l.kind, LayerKind::Adjustment) && !l.visible {
        warn.push(Finding { severity: Severity::Warning, path: format!("{lp}.visible"), message: "a hidden adjustment layer does nothing (it has no picture to be a matte)".into(), hint: Some("remove \"visible\": false, or set \"enabled\": false".into()) });
    }
}

/// Mattes across the project: the source exists, is not the layer itself,
/// is not an adjustment layer, and no chain of mattes comes back to where it
/// started.
pub fn check_mattes(p: &Project, err: &mut impl FnMut(String, String, Option<&str>), warn: &mut Vec<Finding>) {
    let index: BTreeMap<&str, usize> = p.layers.iter().enumerate().map(|(i, l)| (l.id.as_str(), i)).collect();
    for (i, l) in p.layers.iter().enumerate() {
        let Some(m) = &l.matte else { continue };
        let mp = format!("layers[{i}:\"{}\"].matte", l.id);
        let Some(&si) = index.get(m.layer.as_str()) else {
            let ids: Vec<&str> = p.layers.iter().map(|l| l.id.as_str()).filter(|id| *id != l.id).collect();
            err(format!("{mp}.layer"), format!("no layer \"{}\" to use as a matte", m.layer), Some(&format!("layers: {}", if ids.is_empty() { "(none)".into() } else { ids.join(", ") })));
            continue;
        };
        if si == i {
            err(format!("{mp}.layer"), format!("layer \"{}\" cannot be its own matte", l.id), Some("name another layer, usually one with \"visible\": false"));
            continue;
        }
        let src = &p.layers[si];
        if matches!(src.kind, LayerKind::Adjustment) {
            err(format!("{mp}.layer"), format!("\"{}\" is an adjustment layer: it has no picture to be a matte", src.id), Some("use a solid, image, text or other picture layer"));
        }
        // Follow the chain of mattes from the source; coming back here is a cycle.
        let mut seen = vec![i];
        let mut at = si;
        loop {
            if seen.contains(&at) {
                if at == i {
                    let chain: Vec<&str> = seen.iter().map(|&k| p.layers[k].id.as_str()).chain([l.id.as_str()]).collect();
                    err(format!("{mp}.layer"), format!("mattes form a cycle: {}", chain.join(" -> ")), Some("a matte source cannot (through its own matte) depend on the layer it mattes"));
                }
                break;
            }
            seen.push(at);
            match p.layers[at].matte.as_ref().and_then(|m| index.get(m.layer.as_str())) {
                Some(&next) => at = next,
                None => break,
            }
        }
        if src.visible && src.enabled {
            warn.push(Finding {
                severity: Severity::Warning,
                path: format!("{mp}.layer"),
                message: format!("the matte source \"{}\" is also drawn", src.id),
                hint: Some("give it \"visible\": false to use it only as a matte".into()),
            });
        }
        if !src.enabled {
            warn.push(Finding { severity: Severity::Warning, path: format!("{mp}.layer"), message: format!("the matte source \"{}\" is disabled, so the matte is empty", src.id), hint: Some("use \"visible\": false instead of \"enabled\": false to hide a matte source".into()) });
        }
    }
}

/// `mask rect add pos [0, 0] size [0.5, 0.5] feather 2 inverted` and `matte title luma`.
pub fn describe(l: &Layer, out: &mut Vec<String>) {
    let text = |p: &Prop| crate::describe::prop_summary(p).unwrap_or_else(|| crate::describe::const_text(p));
    for m in &l.masks {
        let mut parts = vec![format!("mask {} {}", m.shape.name(), m.mode.name())];
        match m.shape {
            MaskShape::Polygon => parts.push(format!("{} points", m.points.len())),
            _ => parts.push(format!("size {}", text(&m.size))),
        }
        if !is_zero2(&m.position) {
            parts.push(format!("at {}", text(&m.position)));
        }
        for (name, prop) in [("radius", &m.radius), ("rotation", &m.rotation), ("feather", &m.feather), ("expansion", &m.expansion)] {
            if !is_zero(prop) {
                parts.push(format!("{name} {}", text(prop)));
            }
        }
        if !is_one(&m.opacity) {
            parts.push(format!("opacity {}", text(&m.opacity)));
        }
        if m.inverted {
            parts.push("inverted".into());
        }
        out.push(parts.join(" "));
    }
    if let Some(m) = &l.matte {
        out.push(format!("matte {} {}", m.layer, m.mode.name()));
    }
}

fn zero() -> Prop {
    Prop::Const(Value::Num(0.0))
}
fn one() -> Prop {
    Prop::Const(Value::Num(1.0))
}
fn zero2() -> Prop {
    Prop::Const(Value::Vec(vec![0.0, 0.0]))
}
fn one2() -> Prop {
    Prop::Const(Value::Vec(vec![1.0, 1.0]))
}
fn is_zero(p: &Prop) -> bool {
    matches!(p, Prop::Const(v) if v.to_vec().iter().all(|x| *x == 0.0))
}
fn is_zero2(p: &Prop) -> bool {
    is_zero(p)
}
fn is_one(p: &Prop) -> bool {
    matches!(p, Prop::Const(Value::Num(n)) if *n == 1.0)
}
fn is_false(v: &bool) -> bool {
    !*v
}
pub(crate) fn is_true(v: &bool) -> bool {
    *v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{check, Severity};

    fn project(layers: &str) -> Project {
        Project::from_json(&format!(r#"{{"format":"edits.project/1","settings":{{"width":64,"height":64,"fps":25,"duration":4}},"layers":[{layers}]}}"#)).unwrap()
    }

    fn mask(json: &str) -> ResolvedMask {
        let m: Mask = serde_json::from_str(json).unwrap();
        Evaluator::new(&project("")).mask(&m, 0.0)
    }

    #[test]
    fn distances_of_the_shapes() {
        let rect = mask(r#"{"shape":"rect","size":[1,0.5]}"#);
        assert!((rect.distance([0.0, 0.0]) + 0.25).abs() < 1e-12);
        assert!((rect.distance([0.75, 0.0]) - 0.25).abs() < 1e-12);
        assert!((rect.distance([0.8, 0.55]) - 0.3f64.hypot(0.3)).abs() < 1e-12);
        let circle = mask(r#"{"shape":"ellipse","size":0.5,"position":[0.1,0]}"#);
        assert!((circle.distance([0.1, 0.0]) + 0.25).abs() < 1e-12);
        assert!((circle.distance([0.6, 0.0]) - 0.25).abs() < 1e-12);
        let ellipse = mask(r#"{"shape":"ellipse","size":[1,0.5]}"#);
        assert!(ellipse.distance([0.5, 0.0]).abs() < 1e-12 && ellipse.distance([0.0, 0.25]).abs() < 1e-12);
        let round = mask(r#"{"shape":"rounded_rect","size":[1,1],"radius":0.2}"#);
        // The corner is cut: the distance from the corner of the box to the arc.
        assert!((round.distance([0.5, 0.5]) - (0.2 * 2f64.sqrt() - 0.2)).abs() < 1e-12);
        let tri = mask(r#"{"shape":"polygon","points":[[0,-0.5],[0.5,0.5],[-0.5,0.5]],"expansion":10}"#);
        assert!(tri.distance([0.0, 0.0]) < 0.0 && tri.distance([0.0, 0.7]) > 0.0);
        assert!((tri.distance([0.0, 0.55]) - (0.05 - 0.1)).abs() < 1e-12, "expansion grows the shape");
        let turned = mask(r#"{"shape":"rect","size":[1,0.2],"rotation":90}"#);
        assert!(turned.distance([0.0, 0.4]) < 0.0 && turned.distance([0.4, 0.0]) > 0.0);
    }

    #[test]
    fn modes_combine_in_order() {
        let big = mask(r#"{"shape":"rect","size":[1,1]}"#);
        let small = mask(r#"{"shape":"rect","size":[0.4,0.4]}"#);
        let with = |m: &ResolvedMask, mode: MaskMode, inverted: bool| ResolvedMask { mode, inverted, ..m.clone() };
        let (centre, ring, out) = ([0.0, 0.0], [0.4, 0.0], [0.9, 0.0]);
        let sub = [big.clone(), with(&small, MaskMode::Subtract, false)];
        assert_eq!([coverage(&sub, centre, 0.0), coverage(&sub, ring, 0.0), coverage(&sub, out, 0.0)], [0.0, 1.0, 0.0]);
        let first_subtract = [with(&small, MaskMode::Subtract, false)];
        assert_eq!([coverage(&first_subtract, centre, 0.0), coverage(&first_subtract, out, 0.0)], [0.0, 1.0]);
        let first_intersect = [with(&small, MaskMode::Intersect, false)];
        assert_eq!([coverage(&first_intersect, centre, 0.0), coverage(&first_intersect, ring, 0.0)], [1.0, 0.0]);
        let inter = [big.clone(), with(&small, MaskMode::Intersect, false)];
        assert_eq!([coverage(&inter, centre, 0.0), coverage(&inter, ring, 0.0)], [1.0, 0.0]);
        let diff = [big.clone(), with(&small, MaskMode::Difference, false)];
        assert_eq!([coverage(&diff, centre, 0.0), coverage(&diff, ring, 0.0)], [0.0, 1.0]);
        let inv = [with(&small, MaskMode::Add, true)];
        assert_eq!([coverage(&inv, centre, 0.0), coverage(&inv, out, 0.0)], [0.0, 1.0]);
        let half = [ResolvedMask { opacity: 0.5, ..big.clone() }];
        assert_eq!(coverage(&half, centre, 0.0), 0.5);
        assert_eq!(coverage(&[], centre, 0.0), 1.0);
    }

    #[test]
    fn feather_is_a_ramp_centred_on_the_edge() {
        let m = mask(r#"{"shape":"rect","size":[1,1],"feather":20}"#);
        assert!((m.coverage([0.5, 0.0], 0.0) - 0.5).abs() < 1e-12);
        assert_eq!(m.coverage([0.39, 0.0], 0.0), 1.0);
        assert_eq!(m.coverage([0.61, 0.0], 0.0), 0.0);
        assert!((m.coverage([0.45, 0.0], 0.0) - 0.75).abs() < 1e-12);
    }

    #[test]
    fn mattes_and_masks_are_checked() {
        let p = project(
            r#"{"id":"a","kind":"solid","color":[1,1,1,1],"matte":{"layer":"b"},
                "masks":[{"shape":"polygon","points":[[0,0],[1,1]]},{"shape":"rect","feather":-1,"points":[[0,0]]},{"shape":"ellipse","radius":0.1}]},
               {"id":"b","kind":"solid","color":[1,1,1,1],"matte":{"layer":"a"},"visible":false},
               {"id":"c","kind":"solid","color":[1,1,1,1],"matte":{"layer":"nope","mode":"luma"}},
               {"id":"d","kind":"solid","color":[1,1,1,1],"matte":{"layer":"d"}},
               {"id":"e","kind":"adjustment"},
               {"id":"f","kind":"solid","color":[1,1,1,1],"matte":{"layer":"e"}},
               {"id":"g","kind":"solid","color":[1,1,1,1],"matte":{"layer":"c","mode":"alpha_inverted"}}"#,
        );
        let f = check(&p, None);
        let says = |path: &str, needle: &str, sev: Severity| f.iter().any(|x| x.path == path && x.message.contains(needle) && x.severity == sev);
        assert!(says("layers[0:\"a\"].masks[0].points", "at least 3 points, got 2", Severity::Error), "{f:#?}");
        assert!(says("layers[0:\"a\"].masks[1].feather", "feather must be 0 or more, got -1", Severity::Error));
        assert!(says("layers[0:\"a\"].masks[1].points", "ignores \"points\"", Severity::Warning));
        assert!(says("layers[0:\"a\"].masks[2].radius", "ignores \"radius\"", Severity::Warning));
        assert!(says("layers[0:\"a\"].matte.layer", "mattes form a cycle: a -> b -> a", Severity::Error));
        assert!(says("layers[1:\"b\"].matte.layer", "mattes form a cycle: b -> a -> b", Severity::Error));
        assert!(says("layers[2:\"c\"].matte.layer", "no layer \"nope\"", Severity::Error));
        assert!(says("layers[3:\"d\"].matte.layer", "cannot be its own matte", Severity::Error));
        assert!(says("layers[5:\"f\"].matte.layer", "is an adjustment layer", Severity::Error));
        // A chain that does not come back is fine; a visible source is drawn too.
        assert!(!f.iter().any(|x| x.path.starts_with("layers[6:") && x.severity == Severity::Error), "{f:#?}");
        assert!(says("layers[6:\"g\"].matte.layer", "is also drawn", Severity::Warning));
    }

    #[test]
    fn masks_and_mattes_round_trip_and_describe() {
        let p = project(
            r#"{"id":"logo","kind":"text","text":"HI","visible":false},
               {"id":"pic","kind":"solid","color":[1,0,0,1],"matte":{"layer":"logo","mode":"luma_inverted"},
                "masks":[{"shape":"ellipse","size":[0.5,0.3],"feather":{"keys":[{"t":0,"v":0},{"t":1,"v":5}]},"mode":"subtract","invert":true},
                         {"shape":"polygon","points":[[0,0],[1,0],[0,1]],"opacity":0.5}]}"#,
        );
        assert!(!p.layers[0].visible && p.layers[1].visible);
        assert!(p.layers[1].masks[0].inverted, "\"invert\" is accepted for \"inverted\"");
        let again = Project::from_json(&p.to_json()).unwrap();
        assert_eq!(again.to_json(), p.to_json());
        assert!(p.to_json().contains("\"visible\": false") && !p.to_json().contains("\"visible\": true"));
        let d = crate::describe::describe(&p);
        assert!(d.contains("hidden (matte only)"), "{d}");
        assert!(d.contains("mask ellipse subtract size [0.5, 0.3] feather 0→0, 1→5 inverted"), "{d}");
        assert!(d.contains("mask polygon add 3 points opacity 0.5"), "{d}");
        assert!(d.contains("matte logo luma_inverted"), "{d}");
    }

    #[test]
    fn iris_and_split_presets_write_masks() {
        use crate::preset::{apply, find, Target};
        use serde_json::{json, Map};
        let d = json!({ "format": "edits.project/1",
            "settings": { "width": 128, "height": 72, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 120 },
            "slots": { "a": {}, "b": {} },
            "layers": [ { "id": "a", "kind": "image", "slot": "a" }, { "id": "b", "kind": "image", "slot": "b" } ] });
        let tc = crate::time::TimeCtx::from_doc(&d);
        let (out, _) = apply(&d, &find("iris_in", None).unwrap(), &Target::Layer("b".into()), &[2.0], &Map::new(), &tc).unwrap();
        let p: Project = serde_json::from_value(out).unwrap();
        assert!(!crate::check::has_errors(&check(&p, None)));
        let ev = Evaluator::new(&p);
        let size = |t: f64| ev.mask(&p.layers[1].masks[0], t).size[0];
        assert_eq!(size(2.0), 0.0);
        assert!(size(2.5) > 1.0 && size(3.0) > 3.0, "{} {}", size(2.5), size(3.0));
        // At the end the circle covers every corner of the frame.
        let open = ev.masks(&p.layers[1], 3.0);
        assert_eq!(coverage(&open, [-0.89, -0.5], 0.0), 1.0);

        let mut vars = Map::new();
        vars.insert("side".into(), json!("right"));
        vars.insert("gap".into(), json!(0.1));
        let (out, _) = apply(&d, &find("split_panels", None).unwrap(), &Target::Layer("b".into()), &[0.0], &vars, &tc).unwrap();
        let (out, _) = apply(&out, &find("split_panels", None).unwrap(), &Target::Layer("a".into()), &[0.0], &Map::new(), &tc).unwrap();
        let p: Project = serde_json::from_value(out).unwrap();
        assert!(!crate::check::has_errors(&check(&p, None)));
        let ev = Evaluator::new(&p);
        let (left, right) = (ev.masks(&p.layers[0], 0.0), ev.masks(&p.layers[1], 0.0));
        // Slanted: at the top the cut is right of the middle, at the bottom left of it.
        assert_eq!([coverage(&left, [-0.5, 0.0], 0.0), coverage(&right, [-0.5, 0.0], 0.0)], [1.0, 0.0]);
        assert_eq!([coverage(&left, [0.5, 0.0], 0.0), coverage(&right, [0.5, 0.0], 0.0)], [0.0, 1.0]);
        assert_eq!(coverage(&left, [0.05, -0.45], 0.0), 1.0);
        assert_eq!(coverage(&right, [0.0, 0.45], 0.0), 1.0);
        // The gap shows neither.
        assert_eq!([coverage(&left, [0.04, 0.0], 0.0), coverage(&right, [-0.04, 0.0], 0.0)], [0.0, 0.0]);
    }

    #[test]
    fn matte_factors() {
        assert_eq!(MatteMode::Alpha.factor([0.0; 3], 0.25, 1.0), 0.25);
        assert_eq!(MatteMode::AlphaInverted.factor([0.0; 3], 0.25, 1.0), 0.75);
        assert!((MatteMode::Luma.factor([0.5; 3], 1.0, 0.5) - 0.25).abs() < 1e-12);
        assert!((MatteMode::LumaInverted.factor([1.0, 0.0, 0.0], 1.0, 1.0) - 0.7874).abs() < 1e-12);
        assert_eq!(MatteMode::LumaInverted.factor([1.0; 3], 0.0, 1.0), 1.0);
    }
}
