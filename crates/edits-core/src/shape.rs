//! Shape layers: vector shapes drawn in layer space.
//!
//! A `shape` layer holds a list of `shapes`, drawn in order (the first at
//! the bottom). Each shape has a geometry (`rect`, `ellipse`, `polystar`,
//! `path` or `line`), a transform of its own (`position`, `rotation`,
//! `scale`), a `fill` and/or a `stroke` (the stroke above the fill), and
//! `operators` that change the geometry in order before it is painted:
//! `trim` (the line-draw-on), `repeater` (copies, each moved, turned,
//! scaled and faded a step further), `round_corners`, `offset_path`,
//! `zigzag` and `wiggle_path`.
//!
//! Units: geometry, radii, offsets and operator sizes in composition units
//! (height = 1, origin at the layer's origin, y down); stroke `width` and
//! `dash` in percent of the composition height; angles in degrees (0
//! pointing up for a polystar's first point, clockwise); trim in percent of
//! the path's length. Colours are straight-alpha sRGB, `[r, g, b]` or
//! `[r, g, b, a]`, like a text layer's.
//!
//! This module holds the model, validation, the text summary and the
//! evaluation of every animatable value at an instant ([`resolve`]). The
//! geometry itself (bezier paths, operators, stroking and rasterisation) is
//! in `edits-render/src/shape/`. A frame is a pure function of the project
//! and the frame number: the wiggle and the repeater's jitter draw from a
//! hash of their seed, never from a clock.

use crate::check::{Finding, Severity};
use crate::model::{Layer, Prop, Value};
use crate::Evaluator;
use serde::{Deserialize, Serialize};

/// A `shape` layer: what it draws.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Shapes {
    /// Drawn in order, the first at the bottom.
    #[serde(default)]
    pub shapes: Vec<Shape>,
}

/// One shape: geometry, its own transform, paint and operators.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Shape {
    #[serde(flatten)]
    pub geometry: Geometry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Where the shape's origin is in layer space (composition units).
    #[serde(default = "zero2", skip_serializing_if = "is_zero")]
    pub position: Prop,
    /// Turn about the shape's origin, in degrees (clockwise).
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub rotation: Prop,
    /// A number or `[x, y]`; scales the stroke too.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: Prop,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Fill>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    /// Applied in order to the geometry, before the shape's transform.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operators: Vec<Operator>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

/// The outline of a shape, centred on the shape's origin.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Geometry {
    /// A rectangle `size` = `[width, height]` (or one number for a square)
    /// with corners rounded by `roundness` (a radius in composition units).
    Rect {
        #[serde(default = "default_size")]
        size: Prop,
        #[serde(default = "zero", skip_serializing_if = "is_zero")]
        roundness: Prop,
    },
    /// An ellipse inside a box of `size` (one number for a circle's diameter).
    Ellipse {
        #[serde(default = "default_size")]
        size: Prop,
    },
    /// A star (or with `polygon`, a regular polygon) of `points` points, the
    /// first pointing up. Roundness is percent (0 sharp, 100 round).
    Polystar {
        #[serde(default = "five")]
        points: Prop,
        #[serde(default = "default_outer")]
        outer_radius: Prop,
        /// Ignored by a polygon.
        #[serde(default = "default_inner")]
        inner_radius: Prop,
        #[serde(default = "zero", skip_serializing_if = "is_zero")]
        outer_roundness: Prop,
        #[serde(default = "zero", skip_serializing_if = "is_zero")]
        inner_roundness: Prop,
        #[serde(default, skip_serializing_if = "is_false")]
        polygon: bool,
    },
    /// Vertices `[x, y]`, or `[x, y, in_x, in_y, out_x, out_y]` with bezier
    /// handles relative to the vertex (`in` towards the previous vertex,
    /// `out` towards the next). Each vertex can be keyed on its own.
    Path {
        points: Vec<Prop>,
        #[serde(default, skip_serializing_if = "is_false")]
        closed: bool,
    },
    /// A straight line from `from` to `to`.
    Line {
        #[serde(default = "line_from")]
        from: Prop,
        #[serde(default = "line_to")]
        to: Prop,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Fill {
    /// Straight-alpha sRGB, `[r, g, b]` or `[r, g, b, a]`.
    #[serde(default = "white")]
    pub color: Prop,
    /// 0..1.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: Prop,
    #[serde(default, skip_serializing_if = "FillRule::is_nonzero")]
    pub rule: FillRule,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillRule {
    #[default]
    #[serde(rename = "nonzero")]
    NonZero,
    #[serde(rename = "even_odd")]
    EvenOdd,
}
impl FillRule {
    fn is_nonzero(&self) -> bool {
        *self == FillRule::NonZero
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Stroke {
    /// Straight-alpha sRGB, `[r, g, b]` or `[r, g, b, a]`.
    #[serde(default = "white")]
    pub color: Prop,
    /// 0..1.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: Prop,
    /// Width of the line in percent of the composition height.
    #[serde(default = "one")]
    pub width: Prop,
    #[serde(default, skip_serializing_if = "Cap::is_butt")]
    pub cap: Cap,
    #[serde(default, skip_serializing_if = "Join::is_miter")]
    pub join: Join,
    /// Longest miter, in stroke widths, before a miter join is bevelled.
    #[serde(default = "four", skip_serializing_if = "is_four")]
    pub miter_limit: f64,
    /// Dash and gap lengths, alternating, in percent of the composition
    /// height. Empty: a solid line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dash: Vec<f64>,
    /// How far along the dash pattern the line starts (% of height).
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub dash_offset: Prop,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Cap {
    #[default]
    Butt,
    Round,
    Square,
}
impl Cap {
    fn is_butt(&self) -> bool {
        *self == Cap::Butt
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Join {
    #[default]
    Miter,
    Round,
    Bevel,
}
impl Join {
    fn is_miter(&self) -> bool {
        *self == Join::Miter
    }
}

/// A change of the geometry, applied in the order listed.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operator {
    /// Keep the part of each path from `start` to `end` (percent of its
    /// length), moved along by `offset` (percent; 100 is a whole turn). Key
    /// `end` from 0 to 100 to draw a line on.
    Trim {
        #[serde(default = "zero")]
        start: Prop,
        #[serde(default = "hundred")]
        end: Prop,
        #[serde(default = "zero", skip_serializing_if = "is_zero")]
        offset: Prop,
        /// Trim all paths of the shape as one long path, one after the
        /// other, instead of each by the same fractions at once.
        #[serde(default, skip_serializing_if = "is_false")]
        individually: bool,
    },
    /// Copies of everything so far.
    Repeater(Box<Repeater>),
    /// Round every sharp corner with an arc of `radius` (composition units).
    RoundCorners { radius: Prop },
    /// Grow (positive) or shrink (negative) the outline by `amount`
    /// (composition units).
    OffsetPath {
        amount: Prop,
        #[serde(default, skip_serializing_if = "Join::is_miter")]
        join: Join,
        #[serde(default = "four", skip_serializing_if = "is_four")]
        miter_limit: f64,
    },
    /// Turn every segment into `ridges` zigzags of `size` (composition units).
    Zigzag {
        size: Prop,
        #[serde(default = "five")]
        ridges: Prop,
        /// Waves instead of sharp points.
        #[serde(default, skip_serializing_if = "is_false")]
        smooth: bool,
    },
    /// Move the outline about by up to `size` (composition units), at
    /// `detail` points per segment, `speed` times a second, the same way
    /// every time for a `seed`.
    WigglePath {
        size: Prop,
        #[serde(default = "default_detail")]
        detail: Prop,
        /// Wiggles per second; 0 keeps a still, deformed outline.
        #[serde(default = "two_f")]
        speed: f64,
        #[serde(default)]
        seed: u32,
    },
}

/// The repeater: `copies` copies of the geometry so far. Copy `i` is moved
/// by `i` times the transform (`position`, `rotation` and `scale` about
/// `anchor`, counted from `offset`), and its opacity goes from
/// `start_opacity` (the first) to `end_opacity` (the last). The first copy
/// is drawn at the bottom.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Repeater {
    #[serde(default = "three")]
    pub copies: Prop,
    /// Shifts the copies along the transform (-1: the first copy is one
    /// step before the original).
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub offset: Prop,
    /// Pivot of rotation and scale, in the shape's units.
    #[serde(default = "zero2", skip_serializing_if = "is_zero")]
    pub anchor: Prop,
    /// Added per copy (composition units).
    #[serde(default = "zero2", skip_serializing_if = "is_zero")]
    pub position: Prop,
    /// Added per copy, degrees.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub rotation: Prop,
    /// Multiplied per copy: a number or `[x, y]` (0.9 shrinks each copy by 10 %).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: Prop,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub start_opacity: Prop,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub end_opacity: Prop,
    /// A random change of each copy, re-drawn as time goes by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<Jitter>,
}

/// Random per copy, from a hash of (`seed`, copy, time step): each copy is
/// moved by up to `position`, turned by up to `rotation` degrees, scaled by
/// up to `scale` (0.3: 70 % to 130 %) and faded by up to `opacity` (0..1).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Jitter {
    #[serde(default, skip_serializing_if = "is_zero2_f")]
    pub position: [f64; 2],
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub rotation: f64,
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub scale: f64,
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub opacity: f64,
    /// How often the random values change, in the project's time unit
    /// (counted from the layer's `in`): 0 at every frame, omitted never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<f64>,
    #[serde(default)]
    pub seed: u32,
}

// ---- evaluation --------------------------------------------------------------

/// A shape with every value read at an instant.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedShape {
    pub geometry: ResolvedGeometry,
    pub position: [f64; 2],
    /// Degrees.
    pub rotation: f64,
    pub scale: [f64; 2],
    pub fill: Option<ResolvedFill>,
    pub stroke: Option<ResolvedStroke>,
    pub operators: Vec<ResolvedOp>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedGeometry {
    Rect { size: [f64; 2], roundness: f64 },
    Ellipse { size: [f64; 2] },
    Polystar { points: f64, outer_radius: f64, inner_radius: f64, outer_roundness: f64, inner_roundness: f64, polygon: bool },
    /// `[x, y, in_x, in_y, out_x, out_y]` per vertex.
    Path { points: Vec<[f64; 6]>, closed: bool },
    Line { from: [f64; 2], to: [f64; 2] },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedFill {
    /// Straight sRGB; alpha is the colour's times the opacity, 0..1.
    pub color: [f64; 4],
    pub rule: FillRule,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedStroke {
    /// Straight sRGB; alpha is the colour's times the opacity, 0..1.
    pub color: [f64; 4],
    /// Composition units.
    pub width: f64,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
    /// Composition units.
    pub dash: Vec<f64>,
    pub dash_offset: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedOp {
    /// Fractions of the length (0..1).
    Trim { start: f64, end: f64, offset: f64, individually: bool },
    Repeater(Box<ResolvedRepeater>),
    RoundCorners { radius: f64 },
    OffsetPath { amount: f64, join: Join, miter_limit: f64 },
    Zigzag { size: f64, ridges: u32, smooth: bool },
    /// `phase` is seconds times speed: the noise's position in time.
    WigglePath { size: f64, detail: u32, phase: f64, seed: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedRepeater {
    pub copies: u32,
    pub offset: f64,
    pub anchor: [f64; 2],
    pub position: [f64; 2],
    pub rotation: f64,
    pub scale: [f64; 2],
    pub start_opacity: f64,
    pub end_opacity: f64,
    /// The jitter and the time step it is drawn for.
    pub jitter: Option<(Jitter, u64)>,
}

/// Most copies a repeater makes, ridges a zigzag puts on a segment and
/// points a wiggle puts on one.
pub const MAX_COPIES: u32 = 1000;
pub const MAX_RIDGES: u32 = 1000;
pub const MAX_DETAIL: u32 = 1000;

/// Every enabled shape of `layer` at layer time `t` (see [`Evaluator::layer_time`]).
pub fn resolve(ev: &Evaluator, layer: &Layer, s: &Shapes, t: f64) -> Vec<ResolvedShape> {
    s.shapes.iter().filter(|sh| sh.enabled).map(|sh| resolve_shape(ev, layer, sh, t)).collect()
}

fn resolve_shape(ev: &Evaluator, layer: &Layer, sh: &Shape, t: f64) -> ResolvedShape {
    let num = |p: &Prop| finite(ev.num(p, t));
    let v2 = |p: &Prop| ev.vec2(p, t).map(finite);
    let size = |p: &Prop| {
        let v = ev.prop(p, t);
        let v = match v.len() {
            0 => [0.0, 0.0],
            1 => [v[0], v[0]],
            _ => [v[0], v[1]],
        };
        v.map(|x| finite(x).max(0.0))
    };
    let geometry = match &sh.geometry {
        Geometry::Rect { size: sz, roundness } => ResolvedGeometry::Rect { size: size(sz), roundness: num(roundness).max(0.0) },
        Geometry::Ellipse { size: sz } => ResolvedGeometry::Ellipse { size: size(sz) },
        Geometry::Polystar { points, outer_radius, inner_radius, outer_roundness, inner_roundness, polygon } => ResolvedGeometry::Polystar {
            points: num(points).clamp(3.0, 1000.0),
            outer_radius: num(outer_radius).max(0.0),
            inner_radius: num(inner_radius).max(0.0),
            outer_roundness: num(outer_roundness),
            inner_roundness: num(inner_roundness),
            polygon: *polygon,
        },
        Geometry::Path { points, closed } => ResolvedGeometry::Path {
            points: points
                .iter()
                .map(|p| {
                    let v = ev.prop(p, t);
                    let g = |i: usize| finite(v.get(i).copied().unwrap_or(0.0));
                    [g(0), g(1), g(2), g(3), g(4), g(5)]
                })
                .collect(),
            closed: *closed,
        },
        Geometry::Line { from, to } => ResolvedGeometry::Line { from: v2(from), to: v2(to) },
    };
    let color = |c: &Prop, o: &Prop| {
        let v = ev.prop(c, t);
        let g = |i: usize, d: f64| finite(v.get(i).copied().unwrap_or(d)).clamp(0.0, 1.0);
        [g(0, 1.0), g(1, 1.0), g(2, 1.0), g(3, 1.0) * num(o).clamp(0.0, 1.0)]
    };
    let fill = sh.fill.as_ref().map(|f| ResolvedFill { color: color(&f.color, &f.opacity), rule: f.rule });
    let stroke = sh.stroke.as_ref().map(|s| ResolvedStroke {
        color: color(&s.color, &s.opacity),
        width: num(&s.width).max(0.0) / 100.0,
        cap: s.cap,
        join: s.join,
        miter_limit: if s.miter_limit.is_finite() { s.miter_limit.max(1.0) } else { 4.0 },
        dash: s.dash.iter().map(|d| finite(*d).max(0.0) / 100.0).collect(),
        dash_offset: num(&s.dash_offset) / 100.0,
    });
    let operators = sh
        .operators
        .iter()
        .map(|op| match op {
            Operator::Trim { start, end, offset, individually } => ResolvedOp::Trim {
                start: num(start).clamp(0.0, 100.0) / 100.0,
                end: num(end).clamp(0.0, 100.0) / 100.0,
                offset: num(offset) / 100.0,
                individually: *individually,
            },
            Operator::Repeater(r) => {
                let jitter = r.jitter.as_ref().map(|j| (j.clone(), jitter_step(ev, layer, j, t)));
                ResolvedOp::Repeater(Box::new(ResolvedRepeater {
                    copies: num(&r.copies).round().clamp(0.0, MAX_COPIES as f64) as u32,
                    offset: num(&r.offset),
                    anchor: v2(&r.anchor),
                    position: v2(&r.position),
                    rotation: num(&r.rotation),
                    scale: size(&r.scale),
                    start_opacity: num(&r.start_opacity).clamp(0.0, 1.0),
                    end_opacity: num(&r.end_opacity).clamp(0.0, 1.0),
                    jitter,
                }))
            }
            Operator::RoundCorners { radius } => ResolvedOp::RoundCorners { radius: num(radius).max(0.0) },
            Operator::OffsetPath { amount, join, miter_limit } => ResolvedOp::OffsetPath { amount: num(amount), join: *join, miter_limit: if miter_limit.is_finite() { miter_limit.max(1.0) } else { 4.0 } },
            Operator::Zigzag { size, ridges, smooth } => ResolvedOp::Zigzag { size: num(size), ridges: num(ridges).round().clamp(0.0, MAX_RIDGES as f64) as u32, smooth: *smooth },
            Operator::WigglePath { size, detail, speed, seed } => {
                let speed = finite(*speed);
                // A still wiggle reads no time, so a still shape stays cached.
                let phase = if speed == 0.0 { 0.0 } else { ev.project.to_seconds(t) * speed };
                ResolvedOp::WigglePath { size: num(size), detail: num(detail).round().clamp(1.0, MAX_DETAIL as f64) as u32, phase, seed: *seed }
            }
        })
        .collect();
    let scale = size(&sh.scale);
    // A negative scale flips: keep its sign.
    let scale = match ev.prop(&sh.scale, t).as_slice() {
        [s] => [finite(*s), finite(*s)],
        [x, y, ..] => [finite(*x), finite(*y)],
        [] => scale,
    };
    ResolvedShape { geometry, position: v2(&sh.position), rotation: num(&sh.rotation), scale, fill, stroke, operators }
}

/// The time step a jitter is drawn for at layer time `t`: 0 when it never
/// changes, the output frame when `every` is 0, else the step of `every`
/// counted from the layer's `in`.
fn jitter_step(ev: &Evaluator, layer: &Layer, j: &Jitter, t: f64) -> u64 {
    match j.every {
        None => 0,
        Some(e) if !e.is_finite() || e <= 0.0 => {
            let p = ev.project;
            (p.to_seconds(t) * p.settings.fps + 1e-6).floor().max(0.0) as u64
        }
        Some(e) => ((t - layer.start) / e + 1e-9).floor().max(0.0) as u64,
    }
}

impl ResolvedShape {
    /// Everything the shape's picture depends on, as raw bits: two shapes
    /// with equal bits draw the same.
    pub fn bits(shapes: &[ResolvedShape]) -> Vec<u64> {
        // The debug form prints every float exactly (shortest round trip).
        let text = format!("{shapes:?}");
        let bytes = text.as_bytes();
        let mut out = Vec::with_capacity(bytes.len() / 8 + 2);
        out.push(bytes.len() as u64);
        out.extend(bytes.chunks(8).map(|c| {
            let mut b = [0u8; 8];
            b[..c.len()].copy_from_slice(c);
            u64::from_le_bytes(b)
        }));
        out
    }
}

/// True when the shapes look the same at every instant: no keyed value,
/// no moving wiggle and no jitter that changes. Such a layer is rasterised
/// once and reused while the layer's transform stays the same.
pub fn is_static(s: &Shapes) -> bool {
    let mut props: Vec<&Prop> = Vec::new();
    let mut moving = false;
    for sh in s.shapes.iter().filter(|sh| sh.enabled) {
        props.extend([&sh.position, &sh.rotation, &sh.scale]);
        match &sh.geometry {
            Geometry::Rect { size, roundness } => props.extend([size, roundness]),
            Geometry::Ellipse { size } => props.push(size),
            Geometry::Polystar { points, outer_radius, inner_radius, outer_roundness, inner_roundness, .. } => props.extend([points, outer_radius, inner_radius, outer_roundness, inner_roundness]),
            Geometry::Path { points, .. } => props.extend(points.iter()),
            Geometry::Line { from, to } => props.extend([from, to]),
        }
        if let Some(f) = &sh.fill {
            props.extend([&f.color, &f.opacity]);
        }
        if let Some(st) = &sh.stroke {
            props.extend([&st.color, &st.opacity, &st.width, &st.dash_offset]);
        }
        for op in &sh.operators {
            match op {
                Operator::Trim { start, end, offset, .. } => props.extend([start, end, offset]),
                Operator::Repeater(r) => {
                    props.extend([&r.copies, &r.offset, &r.anchor, &r.position, &r.rotation, &r.scale, &r.start_opacity, &r.end_opacity]);
                    moving |= r.jitter.as_ref().is_some_and(|j| j.every.is_some());
                }
                Operator::RoundCorners { radius } => props.push(radius),
                Operator::OffsetPath { amount, .. } => props.push(amount),
                Operator::Zigzag { size, ridges, .. } => props.extend([size, ridges]),
                Operator::WigglePath { size, detail, speed, .. } => {
                    props.extend([size, detail]);
                    moving |= *speed != 0.0;
                }
            }
        }
    }
    !moving && props.iter().all(|p| matches!(p, Prop::Const(_)))
}

// ---- describe and check ----------------------------------------------------------

impl Shapes {
    /// One line for `describe`, e.g.
    /// `shape ellipse(stroke; trim) line(stroke; repeater x24, jitter)`.
    pub fn describe(&self) -> String {
        let mut parts = vec!["shape".to_string()];
        for sh in &self.shapes {
            let geo = match &sh.geometry {
                Geometry::Rect { .. } => "rect",
                Geometry::Ellipse { .. } => "ellipse",
                Geometry::Polystar { polygon: true, .. } => "polygon",
                Geometry::Polystar { .. } => "star",
                Geometry::Path { .. } => "path",
                Geometry::Line { .. } => "line",
            };
            let mut paint = Vec::new();
            if sh.fill.is_some() {
                paint.push("fill".to_string());
            }
            if let Some(s) = &sh.stroke {
                paint.push(format!("stroke {}%", short(&s.width)));
            }
            let ops: Vec<String> = sh
                .operators
                .iter()
                .map(|op| match op {
                    Operator::Trim { end, .. } => format!("trim{}", if matches!(end, Prop::Anim(_)) { " keyed" } else { "" }),
                    Operator::Repeater(r) => format!("repeater x{}{}", short(&r.copies), if r.jitter.is_some() { " jitter" } else { "" }),
                    Operator::RoundCorners { radius } => format!("round_corners {}", short(radius)),
                    Operator::OffsetPath { amount, .. } => format!("offset_path {}", short(amount)),
                    Operator::Zigzag { ridges, .. } => format!("zigzag {}", short(ridges)),
                    Operator::WigglePath { seed, .. } => format!("wiggle_path seed {seed}"),
                })
                .collect();
            let mut inner = if paint.is_empty() { "no paint".to_string() } else { paint.join(", ") };
            if !ops.is_empty() {
                inner = format!("{inner}; {}", ops.join(", "));
            }
            let off = if sh.enabled { "" } else { " (off)" };
            parts.push(format!("{geo}({inner}){off}"));
        }
        if self.shapes.is_empty() {
            parts.push("(empty)".into());
        }
        parts.join(" ")
    }
}

fn short(p: &Prop) -> String {
    match p {
        Prop::Const(Value::Num(n)) => format!("{n}"),
        Prop::Const(Value::Vec(v)) => format!("{v:?}"),
        Prop::Anim(a) => format!("keyed({})", a.keys.len()),
    }
}

/// Every value a property can take as written: the constant, the keys and the base.
fn written(prop: &Prop) -> Vec<Vec<f64>> {
    match prop {
        Prop::Const(v) => vec![v.to_vec()],
        Prop::Anim(a) => a.keys.iter().map(|k| k.v.to_vec()).chain(a.base.iter().map(Value::to_vec)).collect(),
    }
}

/// Validation of a shape layer (called by `check`).
pub fn check(s: &Shapes, lp: &str, err: &mut impl FnMut(String, String, Option<&str>), warn: &mut Vec<Finding>) {
    let mut warning = |path: String, message: String, hint: Option<&str>| warn.push(Finding { severity: Severity::Warning, path, message, hint: hint.map(str::to_string) });
    if s.shapes.is_empty() {
        warning(format!("{lp}.shapes"), "the layer has no shapes: it shows nothing".into(), Some("add e.g. { \"type\": \"ellipse\", \"size\": 0.5, \"fill\": {} }"));
    }
    for (i, sh) in s.shapes.iter().enumerate() {
        let sp = format!("{lp}.shapes[{i}]");
        let mut props: Vec<(String, &Prop)> = vec![(format!("{sp}.position"), &sh.position), (format!("{sp}.rotation"), &sh.rotation), (format!("{sp}.scale"), &sh.scale)];
        // (path, prop, smallest allowed value, how many numbers, what it is)
        let at_least = |path: String, prop: &Prop, min: f64, what: &str, hint: Option<&str>, err: &mut dyn FnMut(String, String, Option<&str>)| {
            if let Some(bad) = written(prop).into_iter().flatten().find(|v| !v.is_finite() || *v < min) {
                err(path, format!("{what} must be at least {min}, got {bad}"), hint);
            }
        };
        let mut errf = |p: String, m: String, h: Option<&str>| err(p, m, h);
        let arity = |path: String, prop: &Prop, ns: &[usize], what: &str, err: &mut dyn FnMut(String, String, Option<&str>)| {
            if let Some(bad) = written(prop).into_iter().find(|v| !ns.contains(&v.len())) {
                let want: Vec<String> = ns.iter().map(|n| n.to_string()).collect();
                err(path, format!("{what} has {} numbers; it takes {}", bad.len(), want.join(" or ")), None);
            }
        };
        match &sh.geometry {
            Geometry::Rect { size, roundness } => {
                arity(format!("{sp}.size"), size, &[1, 2], "size", &mut errf);
                at_least(format!("{sp}.size"), size, 0.0, "size", Some("[width, height] in composition units"), &mut errf);
                at_least(format!("{sp}.roundness"), roundness, 0.0, "roundness", Some("a corner radius in composition units"), &mut errf);
                props.push((format!("{sp}.size"), size));
                props.push((format!("{sp}.roundness"), roundness));
            }
            Geometry::Ellipse { size } => {
                arity(format!("{sp}.size"), size, &[1, 2], "size", &mut errf);
                at_least(format!("{sp}.size"), size, 0.0, "size", Some("[width, height] in composition units"), &mut errf);
                props.push((format!("{sp}.size"), size));
            }
            Geometry::Polystar { points, outer_radius, inner_radius, outer_roundness, inner_roundness, .. } => {
                at_least(format!("{sp}.points"), points, 3.0, "a polystar's points", Some("3 makes a triangle, 5 a five-pointed star"), &mut errf);
                at_least(format!("{sp}.outer_radius"), outer_radius, 0.0, "outer_radius", None, &mut errf);
                at_least(format!("{sp}.inner_radius"), inner_radius, 0.0, "inner_radius", None, &mut errf);
                for (n, p) in [("points", points), ("outer_radius", outer_radius), ("inner_radius", inner_radius), ("outer_roundness", outer_roundness), ("inner_roundness", inner_roundness)] {
                    props.push((format!("{sp}.{n}"), p));
                }
            }
            Geometry::Path { points, closed } => {
                if points.len() < 2 {
                    errf(format!("{sp}.points"), format!("a path needs at least 2 points, got {}", points.len()), Some("[[x, y], [x, y], ...] in composition units; or use a line"));
                }
                if *closed && points.len() == 2 {
                    warning(format!("{sp}.closed"), "a closed path of 2 points has no area".into(), None);
                }
                for (j, p) in points.iter().enumerate() {
                    arity(format!("{sp}.points[{j}]"), p, &[2, 6], "a path point", &mut errf);
                    props.push((format!("{sp}.points[{j}]"), p));
                }
            }
            Geometry::Line { from, to } => {
                arity(format!("{sp}.from"), from, &[2], "from", &mut errf);
                arity(format!("{sp}.to"), to, &[2], "to", &mut errf);
                props.push((format!("{sp}.from"), from));
                props.push((format!("{sp}.to"), to));
            }
        }
        let colour = |path: String, prop: &Prop, err: &mut dyn FnMut(String, String, Option<&str>)| {
            if let Some(bad) = written(prop).into_iter().find(|v| v.len() != 3 && v.len() != 4) {
                err(path, format!("a colour is [r, g, b] or [r, g, b, a] in 0..1, not {} numbers", bad.len()), Some("straight sRGB, like a text layer's fill"));
            }
        };
        if let Some(f) = &sh.fill {
            colour(format!("{sp}.fill.color"), &f.color, &mut errf);
            props.push((format!("{sp}.fill.color"), &f.color));
            props.push((format!("{sp}.fill.opacity"), &f.opacity));
        }
        if let Some(st) = &sh.stroke {
            colour(format!("{sp}.stroke.color"), &st.color, &mut errf);
            at_least(format!("{sp}.stroke.width"), &st.width, 0.0, "stroke width", Some("percent of the composition height, e.g. 1"), &mut errf);
            if st.dash.iter().any(|d| !d.is_finite() || *d < 0.0) {
                errf(format!("{sp}.stroke.dash"), "dash lengths must not be negative".into(), Some("[dash, gap, ...] in percent of the height"));
            } else if !st.dash.is_empty() && st.dash.iter().sum::<f64>() <= 0.0 {
                errf(format!("{sp}.stroke.dash"), "dash lengths add up to 0".into(), Some("remove \"dash\" for a solid line"));
            }
            for (n, p) in [("color", &st.color), ("opacity", &st.opacity), ("width", &st.width), ("dash_offset", &st.dash_offset)] {
                props.push((format!("{sp}.stroke.{n}"), p));
            }
        }
        if sh.fill.is_none() && sh.stroke.is_none() {
            warning(sp.clone(), "the shape has neither fill nor stroke: it draws nothing".into(), Some("add \"fill\": {} or \"stroke\": { \"width\": 1 }"));
        }
        for (j, op) in sh.operators.iter().enumerate() {
            let op_path = format!("{sp}.operators[{j}]");
            match op {
                Operator::Trim { start, end, offset, .. } => {
                    for (n, p) in [("start", start), ("end", end)] {
                        if let Some(bad) = written(p).into_iter().flatten().find(|v| !(0.0..=100.0).contains(v)) {
                            errf(format!("{op_path}.{n}"), format!("trim {n} is a percentage 0..100, got {bad}"), None);
                        }
                    }
                    props.extend([(format!("{op_path}.start"), start), (format!("{op_path}.end"), end), (format!("{op_path}.offset"), offset)]);
                }
                Operator::Repeater(r) => {
                    if let Some(bad) = written(&r.copies).into_iter().flatten().find(|v| !v.is_finite() || *v < 0.0 || *v > MAX_COPIES as f64) {
                        errf(format!("{op_path}.copies"), format!("copies must be between 0 and {MAX_COPIES}, got {bad}"), None);
                    }
                    if let Some(j) = &r.jitter {
                        if !(0.0..=1.0).contains(&j.opacity) || j.scale < 0.0 || !j.scale.is_finite() {
                            errf(format!("{op_path}.jitter"), "jitter opacity is 0..1 and scale 0 or more".into(), None);
                        }
                        if j.every.is_some_and(|e| !e.is_finite() || e < 0.0) {
                            errf(format!("{op_path}.jitter.every"), "every must be 0 (each frame) or a positive time".into(), Some("leave it out for a jitter that never changes"));
                        }
                    }
                    for (n, p) in [("copies", &r.copies), ("offset", &r.offset), ("anchor", &r.anchor), ("position", &r.position), ("rotation", &r.rotation), ("scale", &r.scale), ("start_opacity", &r.start_opacity), ("end_opacity", &r.end_opacity)] {
                        props.push((format!("{op_path}.{n}"), p));
                    }
                }
                Operator::RoundCorners { radius } => {
                    at_least(format!("{op_path}.radius"), radius, 0.0, "radius", None, &mut errf);
                    props.push((format!("{op_path}.radius"), radius));
                }
                Operator::OffsetPath { amount, .. } => props.push((format!("{op_path}.amount"), amount)),
                Operator::Zigzag { size, ridges, .. } => {
                    if let Some(bad) = written(ridges).into_iter().flatten().find(|v| !v.is_finite() || *v < 0.0 || *v > MAX_RIDGES as f64) {
                        errf(format!("{op_path}.ridges"), format!("ridges must be between 0 and {MAX_RIDGES}, got {bad}"), None);
                    }
                    props.extend([(format!("{op_path}.size"), size), (format!("{op_path}.ridges"), ridges)]);
                }
                Operator::WigglePath { size, detail, speed, .. } => {
                    if let Some(bad) = written(detail).into_iter().flatten().find(|v| !v.is_finite() || *v < 1.0 || *v > MAX_DETAIL as f64) {
                        errf(format!("{op_path}.detail"), format!("detail must be between 1 and {MAX_DETAIL}, got {bad}"), None);
                    }
                    if !speed.is_finite() {
                        errf(format!("{op_path}.speed"), "speed must be a number (wiggles per second)".into(), None);
                    }
                    props.extend([(format!("{op_path}.size"), size), (format!("{op_path}.detail"), detail)]);
                }
            }
        }
        for (path, prop) in props {
            crate::check::check_prop(prop, &path, &mut errf);
        }
    }
}

// ---- serde defaults --------------------------------------------------------

fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}
fn num(n: f64) -> Prop {
    Prop::Const(Value::Num(n))
}
fn zero() -> Prop {
    num(0.0)
}
fn one() -> Prop {
    num(1.0)
}
fn three() -> Prop {
    num(3.0)
}
fn five() -> Prop {
    num(5.0)
}
fn hundred() -> Prop {
    num(100.0)
}
fn default_detail() -> Prop {
    num(6.0)
}
fn default_outer() -> Prop {
    num(0.25)
}
fn default_inner() -> Prop {
    num(0.1)
}
fn zero2() -> Prop {
    Prop::Const(Value::Vec(vec![0.0, 0.0]))
}
fn default_size() -> Prop {
    Prop::Const(Value::Vec(vec![0.5, 0.5]))
}
fn line_from() -> Prop {
    Prop::Const(Value::Vec(vec![-0.25, 0.0]))
}
fn line_to() -> Prop {
    Prop::Const(Value::Vec(vec![0.25, 0.0]))
}
fn white() -> Prop {
    Prop::Const(Value::Vec(vec![1.0, 1.0, 1.0]))
}
fn four() -> f64 {
    4.0
}
fn two_f() -> f64 {
    2.0
}
fn yes() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn is_four(v: &f64) -> bool {
    *v == 4.0
}
fn is_zero_f(v: &f64) -> bool {
    *v == 0.0
}
fn is_zero2_f(v: &[f64; 2]) -> bool {
    *v == [0.0, 0.0]
}
fn is_zero(p: &Prop) -> bool {
    matches!(p, Prop::Const(v) if v.to_vec().iter().all(|x| *x == 0.0))
}
fn is_one(p: &Prop) -> bool {
    matches!(p, Prop::Const(Value::Num(n)) if *n == 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayerKind, Project};

    fn project(shapes: serde_json::Value) -> Project {
        let doc = serde_json::json!({
            "format": "edits.project/1",
            "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 120 },
            "layers": [ { "id": "s", "kind": "shape", "shapes": shapes } ]
        });
        serde_json::from_value(doc).unwrap()
    }
    fn shapes(p: &Project) -> &Shapes {
        match &p.layers[0].kind {
            LayerKind::Shape(s) => s,
            _ => unreachable!(),
        }
    }

    #[test]
    fn shapes_parse_round_trip_and_resolve() {
        let p = project(serde_json::json!([
            { "type": "rect", "size": [0.4, 0.2], "roundness": 0.02, "fill": { "color": [1, 0, 0] } },
            { "type": "ellipse", "size": 0.3, "stroke": { "width": 2, "cap": "round", "dash": [3, 1] },
              "operators": [ { "kind": "trim", "end": { "keys": [ { "t": 0, "v": 0 }, { "t": 2, "v": 100 } ] } } ] },
            { "type": "polystar", "points": 6, "polygon": true, "fill": {} },
            { "type": "path", "points": [[0, 0], [0.1, 0.1, -0.05, 0, 0.05, 0], [0.2, 0]], "closed": true, "fill": { "rule": "even_odd" } },
            { "type": "line", "from": [0, 0], "to": [0.5, 0], "stroke": {},
              "operators": [ { "kind": "repeater", "copies": 12, "rotation": 30, "jitter": { "rotation": 10, "every": 0, "seed": 3 } },
                             { "kind": "zigzag", "size": 0.01 }, { "kind": "wiggle_path", "size": 0.01 },
                             { "kind": "round_corners", "radius": 0.01 }, { "kind": "offset_path", "amount": 0.01 } ] }
        ]));
        let text = p.to_json();
        let back = Project::from_json(&text).unwrap();
        assert_eq!(back.to_json(), text);
        assert!(text.contains("\"type\": \"polystar\"") && text.contains("\"kind\": \"shape\""), "{text}");
        let ev = Evaluator::new(&p);
        let r = resolve(&ev, &p.layers[0], shapes(&p), 1.0);
        assert_eq!(r.len(), 5);
        assert_eq!(r[0].geometry, ResolvedGeometry::Rect { size: [0.4, 0.2], roundness: 0.02 });
        assert_eq!(r[1].geometry, ResolvedGeometry::Ellipse { size: [0.3, 0.3] });
        // Stroke width and dashes in composition units.
        let st = r[1].stroke.as_ref().unwrap();
        assert!((st.width - 0.02).abs() < 1e-12 && st.dash == vec![0.03, 0.01]);
        // Half way through the trim's keys.
        assert!(matches!(r[1].operators[0], ResolvedOp::Trim { end, .. } if (end - 0.5).abs() < 1e-12));
        assert_eq!(r[3].fill.as_ref().unwrap().rule, FillRule::EvenOdd);
        let ResolvedGeometry::Path { points, closed: true } = &r[3].geometry else { panic!() };
        assert_eq!(points[0], [0.0; 6]);
        assert_eq!(points[1], [0.1, 0.1, -0.05, 0.0, 0.05, 0.0]);
        assert!(!is_static(shapes(&p)));
    }

    #[test]
    fn static_shapes_are_detected_and_bits_follow_the_values() {
        let still = project(serde_json::json!([ { "type": "ellipse", "fill": {}, "operators": [ { "kind": "wiggle_path", "size": 0.02, "speed": 0 } ] } ]));
        assert!(is_static(shapes(&still)));
        let ev = Evaluator::new(&still);
        let a = ResolvedShape::bits(&resolve(&ev, &still.layers[0], shapes(&still), 0.5));
        let b = ResolvedShape::bits(&resolve(&ev, &still.layers[0], shapes(&still), 3.0));
        assert_eq!(a, b);
        let moving = project(serde_json::json!([ { "type": "ellipse", "fill": {}, "operators": [ { "kind": "wiggle_path", "size": 0.02 } ] } ]));
        assert!(!is_static(shapes(&moving)));
        let ev = Evaluator::new(&moving);
        let a = ResolvedShape::bits(&resolve(&ev, &moving.layers[0], shapes(&moving), 0.5));
        let b = ResolvedShape::bits(&resolve(&ev, &moving.layers[0], shapes(&moving), 3.0));
        assert_ne!(a, b);
        // A jitter re-drawn every beat changes on the beat, not in between.
        let j = project(serde_json::json!([ { "type": "line", "stroke": {}, "operators": [ { "kind": "repeater", "jitter": { "rotation": 5, "every": 1 } } ] } ]));
        let ev = Evaluator::new(&j);
        let at = |t: f64| ResolvedShape::bits(&resolve(&ev, &j.layers[0], shapes(&j), t));
        assert_eq!(at(1.1), at(1.9));
        assert_ne!(at(1.9), at(2.0));
    }

    #[test]
    fn check_reports_bad_shapes() {
        let p = project(serde_json::json!([
            { "type": "polystar", "points": 2, "fill": {} },
            { "type": "rect", "stroke": { "width": -1 } },
            { "type": "path", "points": [[0, 0]], "fill": {} },
            { "type": "ellipse" },
            { "type": "line", "stroke": {}, "operators": [ { "kind": "repeater", "copies": 5000 }, { "kind": "trim", "end": 150 } ] }
        ]));
        let f = crate::check::check(&p, None);
        let says = |path: &str, needle: &str| f.iter().any(|x| x.path == path && x.message.contains(needle) && x.severity == Severity::Error);
        let lp = "layers[0:\"s\"].shapes";
        assert!(says(&format!("{lp}[0].points"), "points must be at least 3"), "{f:#?}");
        assert!(says(&format!("{lp}[1].stroke.width"), "stroke width must be at least 0"), "{f:#?}");
        assert!(says(&format!("{lp}[2].points"), "at least 2 points"), "{f:#?}");
        assert!(says(&format!("{lp}[4].operators[0].copies"), "copies must be between"), "{f:#?}");
        assert!(says(&format!("{lp}[4].operators[1].end"), "percentage"), "{f:#?}");
        assert!(f.iter().any(|x| x.path == format!("{lp}[3]") && x.severity == Severity::Warning && x.message.contains("neither fill nor stroke")), "{f:#?}");
        // A good layer passes.
        let ok = project(serde_json::json!([ { "type": "polystar", "points": 5, "fill": { "color": [1, 1, 0, 0.5] }, "stroke": { "width": 0.5 } } ]));
        let f = crate::check::check(&ok, None);
        assert!(f.iter().all(|x| x.severity != Severity::Error), "{f:#?}");
    }

    #[test]
    fn describe_names_the_shapes() {
        let p = project(serde_json::json!([
            { "type": "ellipse", "stroke": { "width": 2 }, "operators": [ { "kind": "trim", "end": 50 } ] },
            { "type": "line", "stroke": {}, "operators": [ { "kind": "repeater", "copies": 24, "jitter": { "rotation": 5 } } ] }
        ]));
        let d = crate::describe::describe(&p);
        assert!(d.contains("shape ellipse(stroke 2%; trim) line(stroke 1%; repeater x24 jitter)"), "{d}");
    }
}
