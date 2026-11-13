//! The right-hand panel: the Problems list, the inspector of the selected
//! layer (or the camera and project), the Slots page and the History list.
//!
//! Values are read from the project as shown (with a change being dragged
//! applied) and edits are prepared against the file's version, so dragging
//! a value is one edit however long the drag. Editing a constant changes
//! the constant; editing a keyframed property sets a key at the playhead.

use crate::app::{pal, App, Selected, Tab};
use crate::editing::{self, base_value, ease_choices, ease_from_choice, ease_name, key_at_or_before, key_index_at, keys_of, set_prop, tidy, with_ease, with_key_here, with_value, without_key, PropRef};
use crate::panels;
use crate::reload::Problem;
use crate::rows::{blend_text, prop_key_times};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Ui};
use edits_core::{effects, Blend, Ease, Evaluator, Layer, LayerKind, Modifier, Project, Prop};
use serde_json::json;
use std::ops::RangeInclusive;

/// `0.12345678` → `0.1235`, `2.0` → `2`.
pub(crate) fn num(v: f64) -> String {
    if !v.is_finite() {
        return format!("{v}");
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".to_string()
    } else {
        s.to_string()
    }
}

pub(crate) fn vector(v: &[f64]) -> String {
    match v {
        [one] => num(*one),
        many => format!("[{}]", many.iter().map(|x| num(*x)).collect::<Vec<_>>().join(", ")),
    }
}

/// How a property is animated, in a word or two; empty for a constant.
pub(crate) fn animation_note(prop: &Prop) -> String {
    let Prop::Anim(a) = prop else { return String::new() };
    let mut parts = Vec::new();
    if !a.keys.is_empty() {
        parts.push(format!("{} keys", a.keys.len()));
    }
    for m in &a.modifiers {
        parts.push(
            match m {
                Modifier::Shake { .. } => "shake",
                Modifier::Loop { .. } => "loop",
            }
            .to_string(),
        );
    }
    if parts.is_empty() && !prop_key_times(prop).is_empty() {
        parts.push("keys".to_string());
    }
    parts.join(" + ")
}

const BLENDS: [Blend; 8] = [Blend::Normal, Blend::Add, Blend::Screen, Blend::Multiply, Blend::Overlay, Blend::Lighten, Blend::Darken, Blend::Difference];

fn row(ui: &mut Ui, name: &str, value: String, note: &str) {
    ui.label(RichText::new(name).color(ui.visuals().weak_text_color()));
    ui.label(RichText::new(value).monospace());
    if note.is_empty() {
        ui.label("");
    } else {
        ui.label(RichText::new(note).small().color(pal::KEY_TRANSFORM));
    }
    ui.end_row();
}

fn problems(app: &App, ui: &mut Ui) {
    let (errors, warnings) = (app.state.errors(), app.state.warnings());
    if errors + warnings == 0 {
        ui.horizontal(|ui| {
            ui.strong("Problems");
            ui.colored_label(pal::GOOD, "none");
        });
        return;
    }
    let mut title = vec![];
    if errors > 0 {
        title.push(format!("{errors} error{}", if errors == 1 { "" } else { "s" }));
    }
    if warnings > 0 {
        title.push(format!("{warnings} warning{}", if warnings == 1 { "" } else { "s" }));
    }
    let color = if errors > 0 { pal::BAD } else { pal::WARN };
    // Warnings alone stay folded, so the inspector keeps its room.
    egui::CollapsingHeader::new(RichText::new(format!("Problems: {}", title.join(", "))).color(color)).id_salt("problems-header").default_open(errors > 0).open(if errors > 0 { Some(true) } else { None }).show(ui, |ui| {
        if app.state.rejected {
            let what = if app.state.current.is_some() {
                format!("The file on disk is not shown. The preview stays on revision {}, the last version without errors. Edits in the window wait until it is fixed (Undo may fix it).", app.state.revision)
            } else {
                "The file has never been valid, so there is nothing to show yet.".to_string()
            };
            ui.label(RichText::new(what).color(pal::BAD));
        }
        let max = (ui.available_height() * 0.4).max(100.0);
        egui::ScrollArea::vertical().id_salt("problems").max_height(max).auto_shrink([false, true]).show(ui, |ui| {
            for p in &app.state.problems {
                problem(ui, p);
            }
        });
    });
}

fn problem(ui: &mut Ui, p: &Problem) {
    let color = if p.error { pal::BAD } else { pal::WARN };
    egui::Frame::new().fill(color.gamma_multiply(0.10)).stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.5))).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(if p.error { "error" } else { "warning" }).color(color).strong());
            ui.label(RichText::new(&p.path).monospace());
        });
        ui.label(&p.message);
        if let Some(hint) = &p.hint {
            ui.label(RichText::new(hint).size(11.5).color(ui.visuals().weak_text_color()));
        }
    });
    ui.add_space(3.0);
}

fn project_summary(ui: &mut Ui, p: &Project) {
    let s = &p.settings;
    egui::Grid::new("summary").num_columns(3).spacing([10.0, 3.0]).show(ui, |ui| {
        row(ui, "size", format!("{} x {}", s.width, s.height), "");
        row(ui, "fps", num(s.fps), "");
        row(ui, "duration", format!("{} {}", num(s.duration), if s.bpm.is_some() && s.time_base == edits_core::TimeBase::Beats { "beats" } else { "s" }), "");
        if let Some(bpm) = s.bpm {
            row(ui, "tempo", format!("{} BPM", num(bpm)), "");
            row(ui, "beat 0 at", format!("{} s", num(s.beat_offset)), "");
        }
        row(ui, "motion blur", format!("{} samples, shutter {}", s.motion_blur.samples, num(s.motion_blur.shutter)), "");
        row(ui, "layers", p.layers.len().to_string(), "");
        row(ui, "slots", p.slots.len().to_string(), "");
        if let Some(a) = &p.audio {
            row(ui, "audio", a.path.clone(), "");
            row(ui, "audio offset", format!("{} s", num(a.offset)), "");
        }
    });
}

// ---- editable values -------------------------------------------------------------

/// What every editable row needs to know.
struct Cx<'a> {
    /// The file's version: edits are prepared against it.
    base: &'a Project,
    /// What is on screen, with a change being made applied: values are read from it.
    shown: &'a Project,
    /// The layer's id; `None` for the camera.
    layer: Option<&'a str>,
    /// Project time of the playhead.
    t: f64,
    unit: &'static str,
}

impl Cx<'_> {
    fn base_layer(&self) -> Option<&Layer> {
        self.layer.and_then(|id| self.base.layers.iter().find(|l| l.id == id))
    }
    fn shown_layer(&self) -> Option<&Layer> {
        self.layer.and_then(|id| self.shown.layers.iter().find(|l| l.id == id))
    }
    fn id(&self) -> &str {
        self.layer.unwrap_or("camera")
    }
}

/// A drag-value whose identity does not depend on what is drawn before it,
/// so a drag (one edit) survives the layout changing while it lasts.
pub(crate) fn stable_drag(ui: &mut Ui, key: impl std::hash::Hash + std::fmt::Debug, dv: egui::DragValue) -> egui::Response {
    ui.scope_builder(egui::UiBuilder::new().scope_id(egui::Id::unique(("edits-value", key))), |ui| ui.add(dv)).inner
}

/// A small diamond button: filled when there is a key at the playhead.
fn key_button(ui: &mut Ui, filled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
    let c = rect.center();
    let r = if resp.hovered() { 5.5 } else { 4.8 };
    let pts = vec![egui::pos2(c.x, c.y - r), egui::pos2(c.x + r, c.y), egui::pos2(c.x, c.y + r), egui::pos2(c.x - r, c.y)];
    let color = if filled { pal::KEY_TRANSFORM } else { ui.visuals().weak_text_color() };
    let fill = if filled { color } else { Color32::TRANSPARENT };
    ui.painter().add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.2, color)));
    resp
}

fn short_name(r: &PropRef) -> String {
    match r {
        PropRef::Transform(n) | PropRef::Camera(n) => n.to_string(),
        PropRef::Effect { param, .. } => param.clone(),
    }
}

/// One editable property: name (click to pick it), one drag-value per
/// component, the key button, and how it is animated.
#[allow(clippy::too_many_arguments)]
fn prop_row(app: &mut App, ui: &mut Ui, cx: &Cx, r: PropRef, speed: f64, range: Option<RangeInclusive<f64>>, doc: &str, unit: &str) {
    let base_prop = r.get(cx.base, cx.base_layer()).cloned();
    let shown_prop = r.get(cx.shown, cx.shown_layer()).cloned();
    let default = r.default_value();
    let width = |v: Vec<f64>| if matches!(r, PropRef::Effect { .. }) && v.len() < default.len() { edits_core::eval::pad(v, default.len()) } else { v };
    let value = width(base_value(cx.shown, shown_prop.as_ref(), &default, cx.t));
    let selected = app.selected_prop.as_ref() == Some(&r);

    let mut tip = String::new();
    if !doc.is_empty() {
        tip.push_str(doc);
        tip.push('\n');
    }
    tip.push_str(&format!("default {}{}\n", vector(&default), if unit.is_empty() { String::new() } else { format!(" {unit}") }));
    tip.push_str("Click to pick it: the key strip edits its keys, and double-clicking a timeline row keys it there.");
    if ui.selectable_label(selected, short_name(&r)).on_hover_text(tip).clicked() {
        app.selected_prop = Some(r.clone());
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for c in 0..value.len() {
            let mut v = value[c];
            let mut dv = egui::DragValue::new(&mut v).speed(speed).max_decimals(4);
            if let Some(range) = &range {
                dv = dv.range(range.clone()).clamp_existing_to_range(false);
            }
            let resp = stable_drag(ui, (cx.id(), r.label(), c), dv);
            if resp.changed() {
                let mut new = width(base_value(cx.base, base_prop.as_ref(), &default, cx.t));
                new.resize(value.len(), 0.0);
                new[c] = tidy(v);
                let prop = with_value(base_prop.as_ref(), cx.t, new.clone());
                let how = if keys_of(base_prop.as_ref()).is_empty() { String::new() } else { format!(" (key at {} {})", num(cx.t), cx.unit) };
                app.edit_live(resp.id, vec![set_prop(cx.id(), &r, &prop)], format!("{} of {} = {}{how}", r.label(), cx.id(), vector(&new)));
            }
            if resp.dragged() || resp.has_focus() {
                app.keep_live(resp.id);
            }
        }
        if !unit.is_empty() {
            ui.label(RichText::new(unit).small().color(ui.visuals().weak_text_color()));
        }
    });

    let here = key_index_at(base_prop.as_ref(), cx.t).is_some();
    let animated = !keys_of(base_prop.as_ref()).is_empty();
    let tip = if here {
        "A key is at the playhead. Pick the property (click its name) to remove it or change its ease."
    } else if animated {
        "Key here: add a key at the playhead holding the value it has now"
    } else {
        "Key here: make this property keyframed, with a key at the playhead"
    };
    if key_button(ui, here).on_hover_text(tip).clicked() {
        app.selected_prop = Some(r.clone());
        if let Some(prop) = with_key_here(cx.base, base_prop.as_ref(), &default, cx.t) {
            app.commit(vec![set_prop(cx.id(), &r, &prop)], format!("key {} of {} at {} {}", r.label(), cx.id(), num(cx.t), cx.unit));
        }
    }

    let note = match &base_prop {
        None => "default".to_string(),
        Some(p) => animation_note(p),
    };
    let weak = note == "default";
    ui.label(RichText::new(note).small().color(if weak { ui.visuals().weak_text_color() } else { pal::KEY_TRANSFORM }));
    ui.end_row();
}

/// The keys of the picked property: remove the one at the playhead, and the
/// ease of the segment the playhead is in.
fn key_strip(app: &mut App, ui: &mut Ui, cx: &Cx, r: &PropRef) {
    let base_prop = r.get(cx.base, cx.base_layer()).cloned();
    let keys = keys_of(base_prop.as_ref()).to_vec();
    egui::Frame::new().fill(pal::KEY_TRANSFORM.gamma_multiply(0.07)).stroke(Stroke::new(1.0, pal::KEY_TRANSFORM.gamma_multiply(0.35))).corner_radius(4.0).inner_margin(5.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Keys of {}", r.label())).strong());
            if keys.is_empty() {
                ui.weak("none: it is a constant. The diamond makes it keyframed, with a key at the playhead.");
                return;
            }
            ui.weak(format!("{} keys at {}", keys.len(), keys.iter().map(|k| num(k.t)).collect::<Vec<_>>().join(", ")));
        });
        if keys.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if key_index_at(base_prop.as_ref(), cx.t).is_some() {
                if ui.button("Remove key").on_hover_text("Remove the key at the playhead. Without keys the property becomes a constant again.").clicked() {
                    if let Some(prop) = without_key(base_prop.as_ref(), cx.t) {
                        app.commit(vec![set_prop(cx.id(), r, &prop)], format!("remove key of {} of {} at {} {}", r.label(), cx.id(), num(cx.t), cx.unit));
                    }
                }
            } else {
                ui.weak("no key at the playhead");
            }
        });
        let Some(i) = key_at_or_before(base_prop.as_ref(), cx.t) else {
            ui.weak("The playhead is before the first key: no ease to edit here.");
            return;
        };
        if i + 1 >= keys.len() {
            ui.weak("The playhead is at or after the last key: no ease to edit here.");
            return;
        }
        let k = &keys[i];
        // What is shown follows a value being dragged; the edit is made against the file's keys.
        let shown_prop = r.get(cx.shown, cx.shown_layer()).cloned();
        let shown_ease = keys_of(shown_prop.as_ref()).get(i).filter(|s| (s.t - k.t).abs() < editing::SAME_TIME).map_or(k.ease.clone(), |s| s.ease.clone());
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("ease {} to {}", num(k.t), num(keys[i + 1].t))).on_hover_text("How the value moves from this key to the next one");
            let current = ease_name(&k.ease);
            egui::ComboBox::from_id_salt(("ease", r.label())).selected_text(&current).width(110.0).show_ui(ui, |ui| {
                for name in ease_choices() {
                    if ui.selectable_label(current == name, name).clicked() && current != name {
                        if let Some(prop) = with_ease(base_prop.as_ref(), i, ease_from_choice(name, &k.ease)) {
                            app.commit(vec![set_prop(cx.id(), r, &prop)], format!("ease of {} of {} at {}: {name}", r.label(), cx.id(), num(k.t)));
                        }
                    }
                }
            });
            let live = |app: &mut App, resp: &egui::Response, ease: Ease, what: &str| {
                if resp.changed() {
                    if let Some(prop) = with_ease(base_prop.as_ref(), i, ease) {
                        app.edit_live(resp.id, vec![set_prop(cx.id(), r, &prop)], format!("ease of {} of {} at {}: {what}", r.label(), cx.id(), num(k.t)));
                    }
                }
                if resp.dragged() || resp.has_focus() {
                    app.keep_live(resp.id);
                }
            };
            match &shown_ease {
                Ease::Params { name, args } if name == "decay" => {
                    let mut rate = args.get("rate").copied().unwrap_or(0.4);
                    ui.label("rate");
                    let resp = stable_drag(ui, ("rate", cx.id(), r.label()), egui::DragValue::new(&mut rate).speed(0.005).range(0.01..=0.99).max_decimals(3)).on_hover_text("Fraction of the remaining distance covered per frame (0.39 = a step ratio of 0.61)");
                    let mut args = args.clone();
                    args.insert("rate".into(), tidy(rate));
                    live(app, &resp, Ease::Params { name: name.clone(), args }, &format!("decay {}", num(rate)));
                }
                Ease::Bezier(b) => {
                    for c in 0..4 {
                        let mut x = b[c];
                        let range = if c % 2 == 0 { 0.0..=1.0 } else { -2.0..=3.0 };
                        let resp = stable_drag(ui, ("bezier", cx.id(), r.label(), c), egui::DragValue::new(&mut x).speed(0.005).range(range).max_decimals(3)).on_hover_text("CSS cubic bezier x1, y1, x2, y2");
                        let mut nb = *b;
                        nb[c] = tidy(x);
                        live(app, &resp, Ease::Bezier(nb), &format!("bezier {}", vector(&nb)));
                    }
                }
                _ => {}
            }
        });
    });
}

fn speed_for(r: &PropRef, unit: &str, default: &[f64]) -> (f64, Option<RangeInclusive<f64>>) {
    match r {
        PropRef::Transform("rotation") | PropRef::Camera("rotation") => (0.5, None),
        PropRef::Transform("opacity") => (0.005, Some(0.0..=1.0)),
        PropRef::Camera("fov") => (0.2, Some(1.0..=170.0)),
        PropRef::Transform(_) | PropRef::Camera(_) => (0.005, None),
        PropRef::Effect { .. } if unit == "°" => (0.5, None),
        PropRef::Effect { .. } => (default.iter().fold(1.0f64, |m, v| m.max(v.abs())) * 0.01, None),
    }
}

fn props_grid(app: &mut App, ui: &mut Ui, cx: &Cx, salt: impl std::hash::Hash + std::fmt::Debug, props: Vec<(PropRef, &'static str)>) {
    egui::Grid::new(salt).num_columns(4).spacing([8.0, 3.0]).show(ui, |ui| {
        for (r, doc) in props {
            let unit = editing::unit_of(doc);
            let (speed, range) = speed_for(&r, unit, &r.default_value());
            prop_row(app, ui, cx, r, speed, range, doc, unit);
        }
    });
}

fn camera(app: &mut App, ui: &mut Ui, base: &Project, shown: &Project, t: f64, unit: &'static str) {
    let cx = Cx { base, shown, layer: None, t, unit };
    let props = vec![
        (PropRef::Camera("fov"), "Vertical field of view in degrees; the plane z = 0 always fills the frame."),
        (PropRef::Camera("position"), "Offset of the camera from its rest position, in composition-height units."),
        (PropRef::Camera("rotation"), "Camera rotation in degrees (x, y, z)."),
    ];
    props_grid(app, ui, &cx, "camera", props);
    if let Some(r @ PropRef::Camera(_)) = app.selected_prop.clone() {
        key_strip(app, ui, &cx, &r);
    }
}

fn transform_doc(name: &str) -> &'static str {
    match name {
        "position" => "x, y, z in composition-height units; origin at the centre, y down, z away.",
        "scale" => "A number (uniform), [x, y] or [x, y, z]. 1 fills the frame at z = 0.",
        "rotation" => "Degrees about x, then y, then z.",
        "anchor" => "Pivot inside the layer box, in layer-box units from its centre.",
        _ => "0 = invisible, 1 = opaque.",
    }
}

fn small_button(ui: &mut Ui, text: &str, tip: &str) -> bool {
    ui.add(egui::Button::new(text).small()).on_hover_text(tip).clicked()
}

#[allow(clippy::too_many_arguments)]
fn layer(app: &mut App, ui: &mut Ui, base: &Project, shown: &Project, l: &Layer, t: f64, frame_of: impl Fn(f64) -> u64, unit: &'static str) {
    let id = l.id.clone();
    let cx = Cx { base, shown, layer: Some(&id), t, unit };
    let base_l = cx.base_layer().cloned().unwrap_or_else(|| l.clone());
    let end = l.end.unwrap_or(shown.settings.duration);
    let active = Evaluator::new(shown).active(l, t);

    ui.horizontal_wrapped(|ui| {
        let mut enabled = base_l.enabled;
        if ui.checkbox(&mut enabled, "enabled").on_hover_text("A disabled layer is not drawn").changed() {
            app.toggle_layer(&id);
        }
        if small_button(ui, "Up", "Move one place up: towards the front") {
            app.move_layer(&id, true);
        }
        if small_button(ui, "Down", "Move one place down: towards the back") {
            app.move_layer(&id, false);
        }
        if small_button(ui, "Duplicate", "A copy directly above, with a new id") {
            app.duplicate_layer(&id);
        }
        if ui.add(egui::Button::new(RichText::new("Delete").color(pal::BAD)).small()).on_hover_text("Remove the layer (Undo brings it back)").clicked() {
            app.delete_layer(&id);
        }
    });
    ui.add_space(3.0);

    egui::Grid::new("layer").num_columns(3).spacing([8.0, 3.0]).show(ui, |ui| {
        let weak = ui.visuals().weak_text_color();
        match &l.kind {
            LayerKind::Image { slot } => {
                ui.label(RichText::new("slot").color(weak));
                let names: Vec<String> = base.slots.keys().cloned().collect();
                egui::ComboBox::from_id_salt("layer-slot").selected_text(slot.as_str()).width(110.0).show_ui(ui, |ui| {
                    for n in &names {
                        if ui.selectable_label(n == slot, n).clicked() && n != slot {
                            app.commit(vec![editing::set_field(&id, "slot", json!(n))], format!("slot of {id}: {n}"));
                        }
                    }
                });
                let file = shown.slots.get(slot).and_then(|s| s.path.clone()).unwrap_or_else(|| "(empty: placeholder)".into());
                ui.label(RichText::new(crate::app::shorten(&file, 28)).small().color(weak)).on_hover_text(file);
                ui.end_row();
            }
            LayerKind::Solid { color } => {
                ui.label(RichText::new("color").color(weak));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let base_color = match &base_l.kind {
                        LayerKind::Solid { color } => *color,
                        _ => *color,
                    };
                    for c in 0..4 {
                        let mut v = color[c] as f64;
                        let resp = stable_drag(ui, (&id, "color", c), egui::DragValue::new(&mut v).speed(0.005).range(0.0..=1.0).max_decimals(3));
                        if resp.changed() {
                            let mut nc = base_color.map(|x| tidy(x as f64));
                            nc[c] = tidy(v);
                            app.edit_live(resp.id, vec![editing::set_field(&id, "color", json!(nc))], format!("color of {id} = {}", vector(&nc)));
                        }
                        if resp.dragged() || resp.has_focus() {
                            app.keep_live(resp.id);
                        }
                    }
                });
                ui.label(RichText::new("r g b a").small().color(weak));
                ui.end_row();
            }
            LayerKind::Cube { faces, size } => {
                row(ui, "faces", faces.join(", "), "");
                ui.label(RichText::new("size").color(weak));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let base_size = match &base_l.kind {
                        LayerKind::Cube { size, .. } => *size,
                        _ => *size,
                    };
                    for c in 0..3 {
                        let mut v = size[c];
                        let resp = stable_drag(ui, (&id, "size", c), egui::DragValue::new(&mut v).speed(0.005).range(0.001..=100.0).max_decimals(3));
                        if resp.changed() {
                            let mut ns = base_size;
                            ns[c] = tidy(v);
                            app.edit_live(resp.id, vec![editing::set_field(&id, "size", json!(ns))], format!("size of {id} = {}", vector(&ns)));
                        }
                        if resp.dragged() || resp.has_focus() {
                            app.keep_live(resp.id);
                        }
                    }
                });
                ui.label(RichText::new("w h d").small().color(weak));
                ui.end_row();
            }
            LayerKind::Adjustment => row(ui, "kind", "adjustment".into(), "applies its effects to the layers below"),
        }
        for (field, value) in [("in", l.start), ("out", end)] {
            ui.label(RichText::new(field).color(weak)).on_hover_text(format!("In {unit}. Or drag the edge of the bar in the timeline (snaps to quarter beats; Alt: frames)."));
            let mut v = value;
            let resp = stable_drag(ui, (&id, field), egui::DragValue::new(&mut v).speed(0.02).max_decimals(4).suffix(format!(" {unit}")));
            if resp.changed() {
                app.edit_live(resp.id, vec![editing::set_field(&id, field, json!(tidy(v)))], format!("{field} of {id} = {}", num(v)));
            }
            if resp.dragged() || resp.has_focus() {
                app.keep_live(resp.id);
            }
            let note = if field == "out" && l.end.is_none() { format!("frame {} (end of project)", frame_of(value)) } else { format!("frame {}", frame_of(value)) };
            ui.label(RichText::new(note).small().color(weak));
            ui.end_row();
        }
        ui.label(RichText::new("blend").color(weak));
        egui::ComboBox::from_id_salt("layer-blend").selected_text(blend_text(base_l.blend)).width(110.0).show_ui(ui, |ui| {
            for b in BLENDS {
                if ui.selectable_label(b == base_l.blend, blend_text(b)).clicked() && b != base_l.blend {
                    app.commit(vec![editing::set_field(&id, "blend", json!(blend_text(b)))], format!("blend of {id}: {}", blend_text(b)));
                }
            }
        });
        ui.label("");
        ui.end_row();
        ui.label(RichText::new("now").color(weak));
        if !l.enabled {
            ui.colored_label(pal::WARN, "disabled");
        } else if active {
            ui.colored_label(pal::GOOD, "visible");
        } else {
            ui.colored_label(Color32::GRAY, "outside in..out");
        }
        ui.label("");
        ui.end_row();
    });

    ui.add_space(6.0);
    ui.strong("Transform");
    let props: Vec<(PropRef, &'static str)> = editing::TRANSFORM_PROPS.iter().map(|n| (PropRef::Transform(n), transform_doc(n))).collect();
    props_grid(app, ui, &cx, "transform", props);
    if let Some(r @ PropRef::Transform(_)) = app.selected_prop.clone() {
        key_strip(app, ui, &cx, &r);
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.strong("Effects");
        ui.menu_button("+ Add effect", |ui| panels::effect_entries(app, ui, &id)).response.on_hover_text("Add an effect after the others (they apply top to bottom)");
    });
    for (i, e) in l.effects.iter().enumerate() {
        ui.add_space(4.0);
        let spec = effects::spec(&e.kind);
        ui.horizontal(|ui| {
            let title = ui.label(RichText::new(format!("{} · {}", i + 1, e.kind)).strong());
            if let Some(s) = spec {
                title.on_hover_text(s.doc);
            }
            let base_enabled = base_l.effects.get(i).map_or(e.enabled, |b| b.enabled);
            let mut on = base_enabled;
            if ui.checkbox(&mut on, "on").changed() {
                if let Some(op) = editing::toggle_effect_op(&base_l, i) {
                    app.commit(vec![op], format!("{} {} on {id}", if base_enabled { "switch off" } else { "switch on" }, e.kind));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("×").color(pal::BAD)).small()).on_hover_text("Remove this effect").clicked() {
                    app.commit(vec![editing::remove_effect_op(&base_l, i)], format!("remove {} from {id}", e.kind));
                }
                if small_button(ui, "down", "Apply later (move down)") {
                    if let Some(op) = editing::move_effect_op(app.state.doc.as_deref(), &base_l, i, false) {
                        app.commit(vec![op], format!("move {} down on {id}", e.kind));
                    }
                }
                if small_button(ui, "up", "Apply earlier (move up)") {
                    if let Some(op) = editing::move_effect_op(app.state.doc.as_deref(), &base_l, i, true) {
                        app.commit(vec![op], format!("move {} up on {id}", e.kind));
                    }
                }
            });
        });
        if let Some(src) = &e.source {
            ui.weak(format!("source: {src}"));
        }
        let props: Vec<(PropRef, &'static str)> = match spec {
            Some(s) => s.params.iter().map(|p| (PropRef::Effect { index: i, kind: e.kind.clone(), param: p.name.to_string() }, p.doc)).collect(),
            None => e.params.keys().map(|k| (PropRef::Effect { index: i, kind: e.kind.clone(), param: k.clone() }, "")).collect(),
        };
        props_grid(app, ui, &cx, ("effect", i), props);
        if let Some(r @ PropRef::Effect { index, .. }) = app.selected_prop.clone() {
            if index == i && r.fits(l) {
                key_strip(app, ui, &cx, &r);
            }
        }
    }
    if l.effects.is_empty() {
        ui.weak("No effects. \"+ Add effect\" lists them all.");
    }
}

#[allow(clippy::too_many_arguments)]
fn inspect(app: &mut App, ui: &mut Ui, base: &Project, shown: &Project, t: f64, playhead: u64, frame_of: impl Fn(f64) -> u64, unit: &'static str) {
    let at = format!("at frame {playhead} · {} {unit}", num(t));
    match app.selected.clone() {
        Some(Selected::Layer(id)) => match shown.layers.iter().find(|l| l.id == id).cloned() {
            Some(l) => {
                ui.horizontal(|ui| {
                    ui.heading(&l.id);
                    ui.label(RichText::new(crate::rows::kind_text(&l.kind)).color(ui.visuals().weak_text_color()));
                    if ui.small_button("×").on_hover_text("Deselect").clicked() {
                        app.selected = None;
                    }
                });
                ui.weak(format!("Values {at}. Drag or type a value: a constant changes; a keyframed one gets a key at the playhead."));
                ui.add_space(4.0);
                layer(app, ui, base, shown, &l, t, frame_of, unit);
            }
            None => {
                ui.weak(format!("Layer \"{id}\" is not in the project any more."));
            }
        },
        Some(Selected::Camera) => {
            ui.horizontal(|ui| {
                ui.heading("camera");
                if ui.small_button("×").on_hover_text("Deselect").clicked() {
                    app.selected = None;
                }
            });
            ui.weak(format!("Values {at}"));
            ui.add_space(4.0);
            camera(app, ui, base, shown, t, unit);
        }
        None => {
            ui.heading("Inspector");
            ui.weak("Click a layer in the timeline to see and edit it. The Layer menu adds one.");
            ui.add_space(6.0);
            ui.strong("Project");
            project_summary(ui, shown);
            ui.add_space(6.0);
            ui.strong("Camera");
            ui.weak(at);
            camera(app, ui, base, shown, t, unit);
        }
    }
}

pub(crate) fn show(app: &mut App, ui: &mut Ui) {
    ui.add_space(4.0);
    problems(app, ui);
    ui.separator();
    ui.horizontal(|ui| {
        ui.selectable_value(&mut app.tab, Tab::Inspector, "Inspector");
        ui.selectable_value(&mut app.tab, Tab::Slots, "Slots").on_hover_text("The pictures: paths, fit and focus. Drop picture files here.");
    });
    egui::Panel::bottom("history").resizable(true).default_size(150.0).size_range(60.0..=420.0).show(ui, |ui| panels::history(app, ui));

    let (Some(tm), Some(base), Some(shown)) = (app.tm, app.project().cloned(), app.shown_project()) else {
        ui.weak("Nothing to inspect yet.");
        return;
    };
    let t = tm.frame_time(app.playhead);
    let unit = if tm.uses_beats() { "beats" } else { "s" };
    let playhead = app.playhead;
    egui::ScrollArea::vertical().id_salt(("inspector", app.tab == Tab::Slots)).auto_shrink([false, false]).show(ui, |ui| match app.tab {
        Tab::Slots => panels::slots(app, ui),
        Tab::Inspector => inspect(app, ui, &base, &shown, t, playhead, |time| tm.frame_at(time), unit),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short_and_exact_enough() {
        assert_eq!(num(0.0), "0");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(2.0), "2");
        assert_eq!(num(1.25), "1.25");
        assert_eq!(num(0.123456), "0.1235");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(-90.5), "-90.5");
        assert_eq!(num(1234.5), "1234.5");
        assert_eq!(vector(&[1.0]), "1");
        assert_eq!(vector(&[0.0, -0.25, 3.0]), "[0, -0.25, 3]");
    }

    #[test]
    fn animation_notes_name_what_moves_a_value() {
        let constant: Prop = serde_json::from_str("1").unwrap();
        assert_eq!(animation_note(&constant), "");
        let keyed: Prop = serde_json::from_str(r#"{"keys":[{"t":0,"v":1},{"t":1,"v":2}]}"#).unwrap();
        assert_eq!(animation_note(&keyed), "2 keys");
        let shaken: Prop = serde_json::from_str(r#"{"base":[0,0,0],"modifiers":[{"kind":"shake","amplitude":0.01}]}"#).unwrap();
        assert_eq!(animation_note(&shaken), "shake");
        let both: Prop = serde_json::from_str(r#"{"keys":[{"t":0,"v":1}],"modifiers":[{"kind":"loop","period":2},{"kind":"shake","amplitude":0.01}]}"#).unwrap();
        assert_eq!(animation_note(&both), "1 keys + loop + shake");
    }

    #[test]
    fn drags_move_at_a_useful_speed() {
        let (s, range) = speed_for(&PropRef::Transform("opacity"), "", &[1.0]);
        assert_eq!((s, range), (0.005, Some(0.0..=1.0)));
        let radius = PropRef::Effect { index: 0, kind: "glow".into(), param: "radius".into() };
        assert_eq!(speed_for(&radius, "% h", &[3.0]).0, 0.03);
        let hue = PropRef::Effect { index: 0, kind: "hue".into(), param: "degrees".into() };
        assert_eq!(speed_for(&hue, "°", &[0.0]).0, 0.5);
    }
}
