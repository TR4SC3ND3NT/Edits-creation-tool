//! The inspector on the right: the Problems list, then the selected layer
//! (or the camera, or the project) in collapsible sections, every value
//! editable.
//!
//! Values are read from the project as shown (with a change being dragged
//! applied) and edits are prepared against the file's version, so dragging
//! a value is one edit however long the drag. Editing a constant changes
//! the constant; editing a keyframed property sets a key at the playhead.

use crate::app::{App, Cmd, Selected};
use crate::editing::{self, base_value, ease_choices, ease_from_choice, ease_name, key_at_or_before, key_index_at, keys_of, set_prop, tidy, with_ease, with_key_here, with_value, without_key, PropRef};
use crate::panels;
use crate::transitions;
use crate::reload::Problem;
use crate::rows::{blend_text, prop_key_times};
use crate::theme::{self, color, kind_colors, radius, size, space, text};
use crate::widgets::{self, tip, Icon};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, Ui};
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
                Modifier::Oscillate(_) => "oscillate",
            }
            .to_string(),
        );
    }
    if parts.is_empty() && !prop_key_times(prop).is_empty() {
        parts.push("keys".to_string());
    }
    parts.join(" + ")
}


/// Height of a row of the inspector.
const ROW: f32 = 26.0;
/// Width of the unit column after the values.
const UNIT: f32 = 36.0;
/// Width of the key button at the end of a row.
const KEY: f32 = 20.0;

/// A row: a label in the label column, then `add` in the rest. Returns
/// whether the pointer is over the row.
fn field_row(ui: &mut Ui, label: &str, hover: &str, add: impl FnOnce(&mut Ui)) -> bool {
    let row = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), ROW));
    let hovered = ui.rect_contains_pointer(row);
    ui.allocate_ui_with_layout(row.size(), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(row.size());
        let (r, resp) = ui.allocate_exact_size(vec2(size::LABEL_COLUMN, ROW), Sense::hover());
        let galley = widgets::one_line(ui.painter(), label, theme::body(), color::TEXT_MUTED, r.width() - space::XS);
        ui.painter().galley(pos2(r.left(), r.center().y - galley.size().y / 2.0), galley, color::TEXT_MUTED);
        if !hover.is_empty() {
            resp.on_hover_text(hover);
        }
        // The same gap as the value rows, so the columns line up.
        ui.spacing_mut().item_spacing.x = 3.0;
        add(ui);
    });
    hovered
}

fn label_text(t: &str) -> RichText {
    RichText::new(t).color(color::TEXT_MUTED)
}

fn faint(t: impl Into<String>) -> RichText {
    RichText::new(t).font(theme::small()).color(color::TEXT_FAINT)
}

/// A read-only value row.
fn info_row(ui: &mut Ui, name: &str, value: String) {
    field_row(ui, name, "", |ui| {
        ui.add(egui::Label::new(RichText::new(value).font(theme::mono()).color(color::TEXT)).truncate());
    });
}

fn problems(app: &App, ui: &mut Ui) {
    let (errors, warnings) = (app.state.errors(), app.state.warnings());
    if errors + warnings == 0 {
        return;
    }
    let mut title = vec![];
    if errors > 0 {
        title.push(format!("{errors} error{}", if errors == 1 { "" } else { "s" }));
    }
    if warnings > 0 {
        title.push(format!("{warnings} warning{}", if warnings == 1 { "" } else { "s" }));
    }
    // Warnings alone stay folded, so the inspector keeps its room.
    widgets::section(ui, ("problems", errors > 0), &format!("Problems · {}", title.join(", ")), errors > 0, |_| {}, |ui| {
        if app.state.rejected {
            let what = if app.state.current.is_some() {
                format!("The file on disk is not shown. The preview stays on revision {}, the last version without errors. Edits in the window wait until it is fixed (Undo may fix it).", app.state.revision)
            } else {
                "The file has never been valid, so there is nothing to show yet.".to_string()
            };
            ui.label(RichText::new(what).color(color::TEXT_MUTED));
            ui.add_space(space::XS);
        }
        for p in &app.state.problems {
            problem(ui, p);
        }
    });
    widgets::section_rule(ui);
}

fn problem(ui: &mut Ui, p: &Problem) {
    let (tint, fill, icon) = if p.error { (color::ERROR_TEXT, color::ERROR_BG, Icon::CircleAlert) } else { (color::WARN, color::RAISED, Icon::TriangleAlert) };
    egui::Frame::new().fill(fill).corner_radius(CornerRadius::same(radius::NORMAL)).inner_margin(egui::Margin::same(space::M as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = space::XS;
        ui.horizontal(|ui| {
            ui.label(widgets::icon_sized(icon, 13.0).color(tint));
            ui.add(egui::Label::new(RichText::new(&p.path).font(theme::mono_small()).color(color::TEXT_MUTED)).truncate());
        });
        ui.label(RichText::new(&p.message).color(color::TEXT));
        if let Some(hint) = &p.hint {
            ui.label(faint(hint));
        }
    });
    ui.add_space(space::XS);
}

fn project_summary(ui: &mut Ui, p: &Project) {
    let s = &p.settings;
    info_row(ui, "Size", format!("{} × {}", s.width, s.height));
    info_row(ui, "Frame rate", format!("{} fps", num(s.fps)));
    info_row(ui, "Duration", format!("{} {}", num(s.duration), if s.bpm.is_some() && s.time_base == edits_core::TimeBase::Beats { "beats" } else { "s" }));
    if let Some(bpm) = s.bpm {
        info_row(ui, "Tempo", format!("{} BPM", num(bpm)));
        info_row(ui, "Beat 0 at", format!("{} s", num(s.beat_offset)));
    }
    info_row(ui, "Motion blur", format!("{} samples · shutter {}", s.motion_blur.samples, num(s.motion_blur.shutter)));
    info_row(ui, "Layers", p.layers.len().to_string());
    info_row(ui, "Slots", p.slots.len().to_string());
    if let Some(a) = &p.audio {
        info_row(ui, "Audio", a.path.clone());
        info_row(ui, "Audio offset", format!("{} s", num(a.offset)));
    }
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

/// A drag-value of a fixed width whose identity does not depend on what is
/// drawn before it, so a drag (one edit) survives the layout changing while it lasts.
pub(crate) fn stable_drag(ui: &mut Ui, key: impl std::hash::Hash + std::fmt::Debug, width: f32, dv: egui::DragValue) -> egui::Response {
    ui.scope_builder(egui::UiBuilder::new().scope_id(egui::Id::unique(("edits-value", key))), |ui| ui.add_sized(vec2(width, 22.0), dv)).inner
}

/// The key button: a diamond, filled when a key is at the playhead,
/// outlined when the property is animated, and only on hover otherwise.
fn key_button(ui: &mut Ui, here: bool, animated: bool, row_hovered: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(KEY, ROW), Sense::click());
    let hot = resp.hovered();
    if !(here || animated || row_hovered || hot) {
        return resp;
    }
    let c = rect.center();
    let r = 4.5;
    let pts = vec![pos2(c.x, c.y - r), pos2(c.x + r, c.y), pos2(c.x, c.y + r), pos2(c.x - r, c.y)];
    let ink = if hot { color::ACCENT_TEXT } else if here || animated { color::TEXT } else { color::TEXT_FAINT };
    let fill = if here { ink } else { Color32::TRANSPARENT };
    ui.painter().add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.2, ink)));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The key strip shows for a property picked by hand, or an animated one.
fn wants_strip(app: &App, cx: &Cx, r: &PropRef) -> bool {
    app.picked_by_hand || !keys_of(r.get(cx.base, cx.base_layer())).is_empty()
}

fn short_name(r: &PropRef) -> String {
    match r {
        PropRef::Transform(n) | PropRef::Camera(n) => n.to_string(),
        PropRef::Effect { param, .. } => param.clone(),
    }
}

/// One editable property: name (click to pick it), one value field per
/// component, its unit, and the key button.
#[allow(clippy::too_many_arguments)]
fn prop_row(app: &mut App, ui: &mut Ui, cx: &Cx, r: PropRef, speed: f64, range: Option<RangeInclusive<f64>>, doc: &str, unit: &str) {
    let base_prop = r.get(cx.base, cx.base_layer()).cloned();
    let shown_prop = r.get(cx.shown, cx.shown_layer()).cloned();
    let default = r.default_value();
    let width = |v: Vec<f64>| if matches!(r, PropRef::Effect { .. }) && v.len() < default.len() { edits_core::eval::pad(v, default.len()) } else { v };
    let value = width(base_value(cx.shown, shown_prop.as_ref(), &default, cx.t));
    let picked = app.selected_prop.as_ref() == Some(&r);
    let here = key_index_at(base_prop.as_ref(), cx.t).is_some();
    let animated = !keys_of(base_prop.as_ref()).is_empty();
    let note = match &base_prop {
        None => "default: the file does not set it".to_string(),
        Some(p) => animation_note(p),
    };
    let modifiers: Vec<Icon> = match &base_prop {
        Some(Prop::Anim(a)) => a.modifiers.iter().map(|m| if matches!(m, Modifier::Shake { .. } | Modifier::Oscillate(_)) { Icon::Activity } else { Icon::Repeat }).collect(),
        _ => Vec::new(),
    };

    let row = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), ROW));
    let row_hovered = ui.rect_contains_pointer(row);
    if picked {
        ui.painter().rect_filled(row.expand2(vec2(space::XS, 0.0)), radius::SMALL, color::ACCENT_BG);
    } else if row_hovered {
        ui.painter().rect_filled(row.expand2(vec2(space::XS, 0.0)), radius::SMALL, color::RAISED);
    }
    ui.allocate_ui_with_layout(row.size(), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(row.size());
        ui.spacing_mut().item_spacing.x = 3.0;
        // ---- the name: click to pick ----
        let (lr, lresp) = ui.allocate_exact_size(vec2(size::LABEL_COLUMN, ROW), Sense::click());
        let name_color = if picked {
            color::ACCENT_TEXT
        } else if lresp.hovered() {
            color::TEXT
        } else if base_prop.is_none() {
            color::TEXT_FAINT
        } else {
            color::TEXT_MUTED
        };
        let label = ui.painter().text(pos2(lr.left(), lr.center().y), Align2::LEFT_CENTER, short_name(&r), theme::body(), name_color);
        let mut x = label.right() + space::XS;
        for m in &modifiers {
            widgets::paint_icon(ui.painter(), pos2(x + 5.0, lr.center().y), *m, 11.0, color::TEXT_FAINT);
            x += 12.0;
        }
        let mut hover = String::new();
        if !doc.is_empty() {
            hover.push_str(doc);
            hover.push_str("\n\n");
        }
        if !note.is_empty() {
            hover.push_str(&format!("{note}\n"));
        }
        hover.push_str(&format!("Default {}{}\n", vector(&default), if unit.is_empty() { String::new() } else { format!(" {unit}") }));
        hover.push_str("Click to pick it: its keys show below, and double-clicking a timeline row keys it there.");
        if lresp.on_hover_text(hover).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.selected_prop = Some(r.clone());
            app.picked_by_hand = true;
        }

        // ---- the key button and the unit, from the right ----
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let key_tip = if here {
                "A key is at the playhead. Pick the property (click its name) to remove it or change its ease."
            } else if animated {
                "Key here: add a key at the playhead holding the value it has now"
            } else {
                "Key here: make this property keyframed, with a key at the playhead"
            };
            if key_button(ui, here, animated, row_hovered).on_hover_text(key_tip).clicked() {
                app.selected_prop = Some(r.clone());
                app.picked_by_hand = true;
                if let Some(prop) = with_key_here(cx.base, base_prop.as_ref(), &default, cx.t) {
                    app.commit(vec![set_prop(cx.id(), &r, &prop)], format!("key {} of {} at {} {}", r.label(), cx.id(), num(cx.t), cx.unit));
                }
            }
            let (ur, _) = ui.allocate_exact_size(vec2(UNIT, ROW), Sense::hover());
            if !unit.is_empty() {
                ui.painter().text(pos2(ur.left() + 1.0, ur.center().y), Align2::LEFT_CENTER, unit, theme::small(), color::TEXT_FAINT);
            }
            // ---- the values ----
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                // A grid of three slots: one or two values fill two, three fill three.
                let n = value.len().max(1);
                let slots = n.max(3) as f32;
                let slot = (ui.available_width() - 3.0 * (slots - 1.0)) / slots;
                let span = if n <= 2 { 2.0 * slot + 3.0 } else { n as f32 * slot + 3.0 * (n as f32 - 1.0) };
                let each = ((span - 3.0 * (n as f32 - 1.0)) / n as f32).max(32.0);
                for c in 0..value.len() {
                    let mut v = value[c];
                    let mut dv = egui::DragValue::new(&mut v).speed(speed).max_decimals(4).custom_formatter(|v, _| num(v));
                    if let Some(range) = &range {
                        dv = dv.range(range.clone()).clamp_existing_to_range(false);
                    }
                    let resp = stable_drag(ui, (cx.id(), r.label(), c), each, dv);
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
            });
        });
    });
}

/// The keys of the picked property: remove the one at the playhead, and the
/// ease of the segment the playhead is in.
fn key_strip(app: &mut App, ui: &mut Ui, cx: &Cx, r: &PropRef) {
    let base_prop = r.get(cx.base, cx.base_layer()).cloned();
    let keys = keys_of(base_prop.as_ref()).to_vec();
    ui.add_space(space::XS);
    egui::Frame::new().fill(color::RAISED).corner_radius(CornerRadius::same(radius::NORMAL)).inner_margin(egui::Margin::same(space::M as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = space::S;
        ui.horizontal(|ui| {
            ui.label(widgets::icon_sized(Icon::Diamond, 12.0).color(color::ACCENT_TEXT));
            ui.label(RichText::new(format!("Keys of {}", r.label())).font(FontId::new(text::BODY, theme::medium())).color(color::TEXT));
        });
        if keys.is_empty() {
            ui.label(faint("None: it is a constant. The diamond at the end of its row makes it keyframed, with a key at the playhead."));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            for k in &keys {
                let on = (k.t - cx.t).abs() < editing::SAME_TIME;
                let g = ui.painter().layout_no_wrap(num(k.t), theme::mono_small(), if on { color::ACCENT_TEXT } else { color::TEXT_MUTED });
                let (kr, _) = ui.allocate_exact_size(g.size() + vec2(10.0, 6.0), Sense::hover());
                ui.painter().rect_filled(kr, radius::SMALL, if on { color::ACCENT_BG } else { color::CONTROL });
                ui.painter().galley(kr.center() - g.size() / 2.0, g, color::TEXT_MUTED);
            }
            ui.label(faint(cx.unit));
        });
        let at = key_index_at(base_prop.as_ref(), cx.t);
        let seg = key_at_or_before(base_prop.as_ref(), cx.t).filter(|i| i + 1 < keys.len());
        ui.horizontal(|ui| {
            match seg {
                Some(i) => {
                    let k = &keys[i];
                    ui.label(label_text("Ease")).on_hover_text(format!("How the value moves from the key at {} to the one at {}", num(k.t), num(keys[i + 1].t)));
                    let current = ease_name(&k.ease);
                    widgets::combo(("ease", r.label()), RichText::new(&current).font(theme::mono()), 116.0).show_ui(ui, |ui| {
                        for name in ease_choices() {
                            if ui.selectable_label(current == name, name).clicked() && current != name {
                                if let Some(prop) = with_ease(base_prop.as_ref(), i, ease_from_choice(name, &k.ease)) {
                                    app.commit(vec![set_prop(cx.id(), r, &prop)], format!("ease of {} of {} at {}: {name}", r.label(), cx.id(), num(k.t)));
                                }
                            }
                        }
                    });
                }
                None => {
                    let why = if key_at_or_before(base_prop.as_ref(), cx.t).is_none() { "Before the first key: no ease here" } else { "After the last key: no ease here" };
                    ui.label(faint(why));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if at.is_some() && tip(widgets::button(ui, Some(Icon::Trash2), "Remove key", false), "Remove the key at the playhead. Without keys the property becomes a constant again.", &[]).clicked() {
                    if let Some(prop) = without_key(base_prop.as_ref(), cx.t) {
                        app.commit(vec![set_prop(cx.id(), r, &prop)], format!("remove key of {} of {} at {} {}", r.label(), cx.id(), num(cx.t), cx.unit));
                    }
                }
            });
        });
        let Some(i) = seg else { return };
        let k = &keys[i];
        // What is shown follows a value being dragged; the edit is made against the file's keys.
        let shown_prop = r.get(cx.shown, cx.shown_layer()).cloned();
        let shown_ease = keys_of(shown_prop.as_ref()).get(i).filter(|s| (s.t - k.t).abs() < editing::SAME_TIME).map_or(k.ease.clone(), |s| s.ease.clone());
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
                ui.horizontal(|ui| {
                    ui.label(label_text("Rate"));
                    let mut rate = args.get("rate").copied().unwrap_or(0.4);
                    let resp = stable_drag(ui, ("rate", cx.id(), r.label()), 64.0, egui::DragValue::new(&mut rate).speed(0.005).range(0.01..=0.99).max_decimals(3).custom_formatter(|v, _| num(v)))
                        .on_hover_text("Fraction of the remaining distance covered per frame (0.39 = a step ratio of 0.61)");
                    let mut args = args.clone();
                    args.insert("rate".into(), tidy(rate));
                    live(app, &resp, Ease::Params { name: name.clone(), args }, &format!("decay {}", num(rate)));
                });
            }
            Ease::Bezier(b) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(label_text("Curve"));
                    let each = ((ui.available_width() - 9.0) / 4.0).clamp(36.0, 64.0);
                    for c in 0..4 {
                        let mut x = b[c];
                        let range = if c % 2 == 0 { 0.0..=1.0 } else { -2.0..=3.0 };
                        let resp = stable_drag(ui, ("bezier", cx.id(), r.label(), c), each, egui::DragValue::new(&mut x).speed(0.005).range(range).max_decimals(3).custom_formatter(|v, _| num(v))).on_hover_text("CSS cubic bezier x1, y1, x2, y2");
                        let mut nb = *b;
                        nb[c] = tidy(x);
                        live(app, &resp, Ease::Bezier(nb), &format!("bezier {}", vector(&nb)));
                    }
                });
            }
            _ => {}
        }
    });
    ui.add_space(space::XS);
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

fn props(app: &mut App, ui: &mut Ui, cx: &Cx, props: Vec<(PropRef, &'static str)>) {
    ui.spacing_mut().item_spacing.y = 2.0;
    for (r, doc) in props {
        let unit = match &r {
            PropRef::Transform("rotation") | PropRef::Camera("rotation") | PropRef::Camera("fov") => "°",
            _ => editing::unit_of(doc),
        };
        let (speed, range) = speed_for(&r, unit, &r.default_value());
        prop_row(app, ui, cx, r, speed, range, doc, unit);
    }
}

fn camera(app: &mut App, ui: &mut Ui, base: &Project, shown: &Project, t: f64, unit: &'static str) {
    let cx = Cx { base, shown, layer: None, t, unit };
    let list = vec![
        (PropRef::Camera("fov"), "Vertical field of view in degrees; the plane z = 0 always fills the frame."),
        (PropRef::Camera("position"), "Offset of the camera from its rest position, in composition-height units."),
        (PropRef::Camera("rotation"), "Camera rotation in degrees (x, y, z)."),
    ];
    props(app, ui, &cx, list);
    if let Some(r @ PropRef::Camera(_)) = app.selected_prop.clone().filter(|r| wants_strip(app, &cx, r)) {
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

/// A value field for a component of a non-animated layer field (colour, size).
fn fixed_values(app: &mut App, ui: &mut Ui, id: &str, field: &'static str, shown: Vec<f64>, base: Vec<f64>, range: RangeInclusive<f64>) {
    ui.spacing_mut().item_spacing.x = 3.0;
    let n = shown.len() as f32;
    let each = ((ui.available_width() - UNIT - KEY - 6.0 - 3.0 * (n - 1.0)) / n).max(32.0);
    for c in 0..shown.len() {
        let mut v = shown[c];
        let resp = stable_drag(ui, (id, field, c), each, egui::DragValue::new(&mut v).speed(0.005).range(range.clone()).max_decimals(3).custom_formatter(|v, _| num(v)));
        if resp.changed() {
            let mut nv: Vec<f64> = base.iter().map(|x| tidy(*x)).collect();
            nv[c] = tidy(v);
            app.edit_live(resp.id, vec![editing::set_field(id, field, json!(nv))], format!("{field} of {id} = {}", vector(&nv)));
        }
        if resp.dragged() || resp.has_focus() {
            app.keep_live(resp.id);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn layer(app: &mut App, ui: &mut Ui, base: &Project, shown: &Project, l: &Layer, t: f64, frame_of: impl Fn(f64) -> u64, unit: &'static str) {
    let id = l.id.clone();
    let cx = Cx { base, shown, layer: Some(&id), t, unit };
    let base_l = cx.base_layer().cloned().unwrap_or_else(|| l.clone());
    let end = l.end.unwrap_or(shown.settings.duration);
    let active = Evaluator::new(shown).active(l, t);

    // ---- Layer ----
    widgets::section(ui, "layer", "Layer", true, |_| {}, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        match &l.kind {
            LayerKind::Image { slot } => {
                let file = shown.slots.get(slot).and_then(|s| s.path.clone()).unwrap_or_else(|| "empty: a placeholder".into());
                field_row(ui, "Slot", "The picture this layer shows. Change the slot's picture in the Slots drawer.", |ui| {
                    let names: Vec<String> = base.slots.keys().cloned().collect();
                    widgets::combo("layer-slot", slot.as_str(), 96.0).show_ui(ui, |ui| {
                        for n in &names {
                            if ui.selectable_label(n == slot, n).clicked() && n != slot {
                                app.commit(vec![editing::set_field(&id, "slot", json!(n))], format!("slot of {id}: {n}"));
                            }
                        }
                    });
                    ui.add(egui::Label::new(faint(file.clone())).truncate()).on_hover_text(file);
                });
            }
            LayerKind::Solid { color: c } => {
                let base_color = match &base_l.kind {
                    LayerKind::Solid { color } => *color,
                    _ => *c,
                };
                field_row(ui, "Colour", "Red, green, blue and alpha, from 0 to 1", |ui| {
                    let (sw, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                    ui.painter().rect(sw, radius::SMALL, Color32::from_rgba_unmultiplied(to8(c[0]), to8(c[1]), to8(c[2]), to8(c[3])), Stroke::new(1.0, color::LINE_STRONG), egui::StrokeKind::Inside);
                    fixed_values(app, ui, &id, "color", c.iter().map(|x| *x as f64).collect(), base_color.iter().map(|x| *x as f64).collect(), 0.0..=1.0);
                });
            }
            LayerKind::Cube { faces, size: sz } => {
                info_row(ui, "Faces", faces.join(", "));
                let base_size = match &base_l.kind {
                    LayerKind::Cube { size, .. } => *size,
                    _ => *sz,
                };
                field_row(ui, "Size", "Width, height and depth of the cube", |ui| {
                    fixed_values(app, ui, &id, "size", sz.to_vec(), base_size.to_vec(), 0.001..=100.0);
                });
            }
            LayerKind::Adjustment => {
                field_row(ui, "Kind", "", |ui| {
                    ui.label(faint("Applies its effects to the layers below"));
                });
            }
            LayerKind::Video(v) => {
                let file = shown.slots.get(&v.slot).and_then(|s| s.path.clone()).unwrap_or_else(|| "empty: a placeholder".into());
                info_row(ui, "Video", format!("{} ({file})", v.slot));
                let mut timing: Vec<&str> = Vec::new();
                if v.time_remap.is_some() {
                    timing.push("time remap");
                } else if v.speed.is_some() {
                    timing.push("speed");
                }
                if v.looped {
                    timing.push("loop");
                }
                if v.reverse {
                    timing.push("reverse");
                }
                if !timing.is_empty() {
                    info_row(ui, "Timing", timing.join(", "));
                }
            }
            LayerKind::Text(t) => {
                info_row(ui, "Text", t.text.replace('\n', " / "));
                let font = t.font.clone().unwrap_or_else(|| "Inter (built in)".into());
                let weight = t.weight.map(|w| format!(", weight {w}")).unwrap_or_default();
                info_row(ui, "Font", format!("{font}, size {}{weight}", num(t.size)));
            }
            LayerKind::Particles(p) => {
                info_row(ui, "Particles", p.describe());
            }
            LayerKind::Shape(s) => {
                info_row(ui, "Shapes", s.describe());
            }
            LayerKind::Comp(c) => {
                let size = shown.comps.get(&c.comp).map(|k| format!("{} {unit}, {} layers", num(k.duration), k.layers.len())).unwrap_or_else(|| "not defined".into());
                info_row(ui, "Comp", format!("{} ({size})", c.comp));
            }
        }
        for (field, value, label) in [("in", l.start, "In"), ("out", end, "Out")] {
            field_row(ui, label, &format!("In {unit}. Or drag the edge of the bar in the timeline (snaps to quarter beats; Alt: frames)."), |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let mut v = value;
                let resp = stable_drag(ui, (&id, field), 86.0, egui::DragValue::new(&mut v).speed(0.02).max_decimals(4).custom_formatter(|v, _| num(v)));
                if resp.changed() {
                    app.edit_live(resp.id, vec![editing::set_field(&id, field, json!(tidy(v)))], format!("{field} of {id} = {}", num(v)));
                }
                if resp.dragged() || resp.has_focus() {
                    app.keep_live(resp.id);
                }
                ui.label(faint(unit));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let note = if field == "out" && l.end.is_none() { format!("F {} · end", frame_of(value)) } else { format!("F {}", frame_of(value)) };
                    ui.label(RichText::new(note).font(theme::mono_small()).color(color::TEXT_FAINT));
                });
            });
        }
        field_row(ui, "Blend", "How the layer mixes with what is under it", |ui| {
            widgets::combo("layer-blend", blend_text(base_l.blend), 96.0).show_ui(ui, |ui| {
                for b in Blend::ALL {
                    if ui.selectable_label(b == base_l.blend, blend_text(b)).clicked() && b != base_l.blend {
                        app.commit(vec![editing::set_field(&id, "blend", json!(blend_text(b)))], format!("blend of {id}: {}", blend_text(b)));
                    }
                }
            });
        });
        field_row(ui, "Now", "Whether the layer is drawn at the playhead", |ui| {
            let (text, c) = if !l.enabled {
                ("Disabled", color::WARN)
            } else if active {
                ("Visible", color::OK_TEXT)
            } else {
                ("Outside in–out", color::TEXT_FAINT)
            };
            let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 3.0, c);
            ui.label(RichText::new(text).color(c));
        });
    });
    widgets::section_rule(ui);

    // ---- Transition in ----
    transition_section(app, ui, &cx, l, &base_l, unit);
    widgets::section_rule(ui);

    // ---- Transform ----
    widgets::section(ui, "transform", "Transform", true, |_| {}, |ui| {
        let list: Vec<(PropRef, &'static str)> = editing::TRANSFORM_PROPS.iter().map(|n| (PropRef::Transform(n), transform_doc(n))).collect();
        props(app, ui, &cx, list);
        if let Some(r @ PropRef::Transform(_)) = app.selected_prop.clone().filter(|r| wants_strip(app, &cx, r)) {
            key_strip(app, ui, &cx, &r);
        }
    });
    widgets::section_rule(ui);

    // ---- Effects ----
    let title = if l.effects.is_empty() { "Effects".to_string() } else { format!("Effects · {}", l.effects.len()) };
    let mut add: Option<&'static str> = None;
    widgets::section(
        ui,
        "effects",
        &title,
        true,
        |ui| {
            let r = tip(widgets::small_icon_button(ui, Icon::Plus), "Add an effect after the others (they apply top to bottom)", &[]);
            egui::Popup::menu(&r).frame(widgets::popover_frame()).show(|ui| {
                if let Some(kind) = panels::effect_menu(ui) {
                    add = Some(kind);
                    ui.close();
                }
            });
        },
        |ui| {
            for (i, e) in l.effects.iter().enumerate() {
                let spec = effects::spec(&e.kind);
                if i > 0 {
                    ui.add_space(space::XS);
                }
                // The effect's own header: switch, name, order and remove.
                ui.allocate_ui_with_layout(vec2(ui.available_width(), ROW + 2.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = space::S;
                    let base_enabled = base_l.effects.get(i).map_or(e.enabled, |b| b.enabled);
                    let mut on = base_enabled;
                    if widgets::switch(ui, &mut on).on_hover_text(if base_enabled { "On: click to switch it off" } else { "Off: click to switch it on" }).changed() {
                        if let Some(op) = editing::toggle_effect_op(&base_l, i) {
                            app.commit(vec![op], format!("{} {} on {id}", if base_enabled { "switch off" } else { "switch on" }, e.kind));
                        }
                    }
                    let name = ui.label(RichText::new(&e.kind).font(FontId::new(text::BODY, theme::medium())).color(if base_enabled { color::TEXT } else { color::TEXT_FAINT }));
                    if let Some(s) = spec {
                        name.on_hover_text(s.doc);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if tip(widgets::danger_icon_button(ui, Icon::Trash2), "Remove this effect", &[]).clicked() {
                            app.commit(vec![editing::remove_effect_op(&base_l, i)], format!("remove {} from {id}", e.kind));
                        }
                        let last = i + 1 == l.effects.len();
                        if tip(ui.add_enabled_ui(!last, |ui| widgets::small_icon_button(ui, Icon::ArrowDown)).inner, "Apply later (move down)", &[]).clicked() {
                            if let Some(op) = editing::move_effect_op(app.state.doc.as_deref(), &base_l, i, false) {
                                app.commit(vec![op], format!("move {} down on {id}", e.kind));
                            }
                        }
                        if tip(ui.add_enabled_ui(i > 0, |ui| widgets::small_icon_button(ui, Icon::ArrowUp)).inner, "Apply earlier (move up)", &[]).clicked() {
                            if let Some(op) = editing::move_effect_op(app.state.doc.as_deref(), &base_l, i, true) {
                                app.commit(vec![op], format!("move {} up on {id}", e.kind));
                            }
                        }
                    });
                });
                if let Some(src) = &e.source {
                    ui.label(faint(format!("source: {src}")));
                }
                let list: Vec<(PropRef, &'static str)> = match spec {
                    Some(s) => s.params.iter().map(|p| (PropRef::Effect { index: i, kind: e.kind.clone(), param: p.name.to_string() }, p.doc)).collect(),
                    None => e.params.keys().map(|k| (PropRef::Effect { index: i, kind: e.kind.clone(), param: k.clone() }, "")).collect(),
                };
                ui.scope(|ui| props(app, ui, &cx, list));
                if let Some(r @ PropRef::Effect { index, .. }) = app.selected_prop.clone().filter(|r| wants_strip(app, &cx, r)) {
                    if index == i && r.fits(l) {
                        key_strip(app, ui, &cx, &r);
                    }
                }
            }
            if l.effects.is_empty() {
                ui.label(faint("No effects. The + above lists them all; Ctrl K finds one by name."));
            }
        },
    );
    if let Some(kind) = add {
        app.run(Cmd::AddEffect(id.clone(), kind));
    }
}

/// How the layer comes in: its transition's kind, length, ease and
/// parameters, every one editable; `+` adds one (the `transition:<kind>`
/// preset, which keeps the picture it comes from), the bin removes it.
fn transition_section(app: &mut App, ui: &mut Ui, cx: &Cx, l: &Layer, base_l: &Layer, unit: &'static str) {
    let id = l.id.clone();
    let title = match &l.transition {
        Some(t) => format!("Transition in · {}", t.kind),
        None => "Transition in".to_string(),
    };
    let mut add: Option<&'static str> = None;
    let mut remove = false;
    widgets::section(
        ui,
        "transition",
        &title,
        true,
        |ui| {
            if base_l.transition.is_some() {
                if tip(widgets::danger_icon_button(ui, Icon::Trash2), "Remove the transition: the layer cuts in again", &[]).clicked() {
                    remove = true;
                }
            } else {
                let r = tip(widgets::small_icon_button(ui, Icon::Plus), "Add a transition into this layer at its in", &[]);
                egui::Popup::menu(&r).frame(widgets::popover_frame()).show(|ui| {
                    ui.set_width(240.0);
                    ui.label(widgets::eyebrow("Transition in"));
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for t in transitions::specs() {
                            if widgets::menu_item(ui, Some(Icon::ArrowRightLeft), t.kind, Some(&format!("{} b", num(t.duration)))).on_hover_text(t.doc).clicked() {
                                add = Some(t.kind);
                                ui.close();
                            }
                        }
                    });
                });
            }
        },
        |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let (Some(shown_t), Some(base_t)) = (&l.transition, &base_l.transition) else {
                ui.label(faint("None: the layer cuts in at its in. The + above adds one; the picture it comes from stays until it ends."));
                return;
            };
            let doc = app.state.doc.clone();
            let tc = doc.as_deref().map(edits_core::time::TimeCtx::from_doc);
            field_row(ui, "Kind", "The transition. Hover a name in the list for what it looks like.", |ui| {
                widgets::combo("transition-kind", RichText::new(&base_t.kind).font(theme::mono()), 150.0).show_ui(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for t in transitions::specs() {
                            if ui.selectable_label(t.kind == base_t.kind, t.kind).on_hover_text(t.doc).clicked() && t.kind != base_t.kind {
                                if let Some(d) = &doc {
                                    app.commit(transitions::kind_ops(d, &id, t.kind), format!("transition into {id}: {}", t.kind));
                                }
                            }
                        }
                    });
                });
            });
            if let Some(spec) = effects::transition(&shown_t.kind) {
                ui.label(faint(spec.doc));
            }
            field_row(ui, "Duration", &format!("Length in {unit}, from the layer's in. The picture it comes from is kept until it ends."), |ui| {
                let mut v = shown_t.duration;
                let resp = stable_drag(ui, (&id, "transition-duration"), 86.0, egui::DragValue::new(&mut v).speed(0.01).range(0.01..=64.0).max_decimals(4).custom_formatter(|v, _| num(v)));
                if resp.changed() {
                    if let (Some(d), Some(tc)) = (&doc, &tc) {
                        app.edit_live(resp.id, transitions::duration_ops(d, &id, v, tc), format!("transition into {id}: {}", transitions::describe(&base_t.kind, v, unit)));
                    }
                }
                if resp.dragged() || resp.has_focus() {
                    app.keep_live(resp.id);
                }
                ui.label(faint(unit));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("ends {}", num(l.start + shown_t.duration))).font(theme::mono_small()).color(color::TEXT_FAINT));
                });
            });
            field_row(ui, "Ease", "How the progress moves from 0 to 1", |ui| {
                let current = ease_name(&base_t.ease);
                widgets::combo("transition-ease", RichText::new(&current).font(theme::mono()), 116.0).show_ui(ui, |ui| {
                    for name in ease_choices() {
                        if ui.selectable_label(current == name, name).clicked() && current != name {
                            app.commit(vec![transitions::ease_op(&id, &ease_from_choice(name, &base_t.ease))], format!("ease of the transition into {id}: {name}"));
                        }
                    }
                });
            });
            let Some(spec) = effects::transition(&base_t.kind) else { return };
            let ev = Evaluator::new(cx.shown);
            for p in spec.params {
                let value = shown_t.params.get(p.name).map(|v| edits_core::eval::pad(ev.prop(v, l.start), p.default.len())).unwrap_or_else(|| p.default.to_vec());
                let animated = matches!(base_t.params.get(p.name), Some(Prop::Anim(_)));
                let hover = format!("{}\nDefault {}{}", p.doc, vector(p.default), if animated { "\nKeyframed in the file: edit its keys there (or ask the agent)." } else { "" });
                field_row(ui, p.name, &hover, |ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    if animated {
                        ui.label(RichText::new(format!("{} · keyed", vector(&value))).font(theme::mono()).color(color::TEXT_FAINT));
                        return;
                    }
                    if p.color {
                        let mut rgb = [value[0] as f32, value.get(1).copied().unwrap_or(0.0) as f32, value.get(2).copied().unwrap_or(0.0) as f32];
                        let r = egui::color_picker::color_edit_button_rgb(ui, &mut rgb);
                        if r.changed() {
                            let mut nv = value.clone();
                            for (c, x) in rgb.iter().enumerate() {
                                nv[c] = tidy(*x as f64);
                            }
                            app.commit(vec![transitions::param_op(&id, p.name, &nv)], format!("{} of the transition into {id} = {}", p.name, vector(&nv)));
                        }
                        ui.label(RichText::new(vector(&value)).font(theme::mono_small()).color(color::TEXT_FAINT));
                        return;
                    }
                    let n = value.len().max(1) as f32;
                    let each = ((ui.available_width() - UNIT - 3.0 * (n - 1.0)) / n).clamp(40.0, 96.0);
                    for c in 0..value.len() {
                        let mut v = value[c];
                        let mut dv = egui::DragValue::new(&mut v).speed(p.max.zip(p.min).map_or(0.01, |(hi, lo)| (hi - lo) / 200.0)).max_decimals(4).custom_formatter(|v, _| num(v));
                        if let (Some(lo), Some(hi)) = (p.min, p.max) {
                            dv = dv.range(lo..=hi);
                        }
                        let resp = stable_drag(ui, (&id, "transition", p.name, c), each, dv);
                        if resp.changed() {
                            let mut nv = value.clone();
                            nv[c] = tidy(v);
                            app.edit_live(resp.id, vec![transitions::param_op(&id, p.name, &nv)], format!("{} of the transition into {id} = {}", p.name, vector(&nv)));
                        }
                        if resp.dragged() || resp.has_focus() {
                            app.keep_live(resp.id);
                        }
                    }
                });
            }
            if spec.params.is_empty() {
                ui.label(faint("No parameters."));
            }
        },
    );
    if remove {
        app.run(Cmd::RemoveTransition(id.clone()));
    }
    if let Some(kind) = add {
        app.run(Cmd::AddTransition(id, kind));
    }
}

/// The selected layer's (or the camera's) title: kind badge, name, actions.
fn header(app: &mut App, ui: &mut Ui, l: Option<&Layer>, at: &str) {
    ui.add_space(space::M);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 36.0), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::M;
        let kind = l.map_or("camera".to_string(), |l| crate::rows::kind_text(&l.kind));
        let tint = kind_colors(&kind);
        let (badge, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
        ui.painter().rect_filled(badge, radius::NORMAL, tint.fill);
        widgets::paint_icon(ui.painter(), badge.center(), widgets::kind_icon(&kind), text::ICON_SMALL, tint.stripe.lerp_to_gamma(Color32::WHITE, 0.35));
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let name = l.map_or("Camera".to_string(), |l| l.id.clone());
            ui.add(egui::Label::new(RichText::new(name).font(FontId::new(text::TITLE, theme::strong())).color(color::TEXT)).truncate());
            ui.label(faint(kind));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if tip(widgets::small_icon_button(ui, Icon::X), "Deselect", &["Esc"]).clicked() {
                app.selected = None;
            }
            let Some(l) = l else { return };
            let id = l.id.clone();
            let more = tip(widgets::small_icon_button(ui, Icon::Ellipsis), "More", &[]);
            egui::Popup::menu(&more).frame(widgets::popover_frame()).show(|ui| {
                ui.set_width(200.0);
                if widgets::menu_item(ui, Some(Icon::ArrowUp), "Move up", None).on_hover_text("One place up: towards the front").clicked() {
                    app.run(Cmd::MoveLayer(id.clone(), true));
                    ui.close();
                }
                if widgets::menu_item(ui, Some(Icon::ArrowDown), "Move down", None).on_hover_text("One place down: towards the back").clicked() {
                    app.run(Cmd::MoveLayer(id.clone(), false));
                    ui.close();
                }
                widgets::divider(ui);
                if widgets::menu_item_colored(ui, Some(Icon::Trash2), "Delete layer", Some("Del"), color::ERROR_TEXT).on_hover_text("Undo brings it back").clicked() {
                    app.run(Cmd::Delete(id.clone()));
                    ui.close();
                }
            });
            if tip(widgets::small_icon_button(ui, Icon::Copy), "Duplicate: a copy directly above, with a new id", &["Ctrl", "D"]).clicked() {
                app.run(Cmd::Duplicate(id.clone()));
            }
            let eye = if l.enabled { Icon::Eye } else { Icon::EyeOff };
            if tip(widgets::small_icon_button(ui, eye), if l.enabled { "Enabled: click to disable (not drawn)" } else { "Disabled: click to enable" }, &[]).clicked() {
                app.run(Cmd::ToggleLayer(id.clone()));
            }
        });
    });
    ui.label(RichText::new(at).font(theme::mono_small()).color(color::TEXT_FAINT)).on_hover_text("Values are what the project evaluates to at the playhead. Drag or type one: a constant changes; a keyframed one gets a key at the playhead.");
    ui.add_space(space::S);
    widgets::section_rule(ui);
}

#[allow(clippy::too_many_arguments)]
fn inspect(app: &mut App, ui: &mut Ui, base: &Project, shown: &Project, t: f64, playhead: u64, frame_of: impl Fn(f64) -> u64, unit: &'static str) {
    let at = format!("Values at frame {playhead} · {} {}", num(t), if unit == "s" { "s" } else { "beats" });
    match app.selected.clone() {
        Some(Selected::Layer(id)) => match shown.layers.iter().find(|l| l.id == id).cloned() {
            Some(l) => {
                header(app, ui, Some(&l), &at);
                layer(app, ui, base, shown, &l, t, frame_of, unit);
            }
            None => {
                widgets::empty_state(ui, Icon::Layers, "Layer not found", &format!("\"{id}\" is not in the project any more."), &[]);
            }
        },
        Some(Selected::Camera) => {
            header(app, ui, None, &at);
            widgets::section(ui, "camera-props", "Camera", true, |_| {}, |ui| camera(app, ui, base, shown, t, unit));
        }
        None => {
            widgets::empty_state(ui, Icon::MousePointerClick, "No layer selected", "Click a layer in the timeline to see and edit it.", &[("Ctrl+K", "Find a layer or command"), ("Space", "Play or pause")]);
            ui.add_space(space::L);
            widgets::section_rule(ui);
            widgets::section(ui, "project", "Project", true, |_| {}, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                project_summary(ui, shown);
            });
            widgets::section_rule(ui);
            widgets::section(ui, "camera", "Camera", false, |_| {}, |ui| {
                ui.label(RichText::new(&at).font(theme::mono_small()).color(color::TEXT_FAINT));
                camera(app, ui, base, shown, t, unit);
            });
        }
    }
}

pub(crate) fn show(app: &mut App, ui: &mut Ui) {
    egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin { left: space::L as i8, right: space::L as i8, top: space::XS as i8, bottom: space::L as i8 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            problems(app, ui);
            let (Some(tm), Some(base), Some(shown)) = (app.tm, app.project().cloned(), app.shown_project()) else {
                widgets::empty_state(ui, Icon::FileWarning, "Nothing to inspect yet", "The inspector fills in when the project file is valid.", &[]);
                return;
            };
            let t = tm.frame_time(app.playhead);
            let unit = if tm.uses_beats() { "beats" } else { "s" };
            let playhead = app.playhead;
            inspect(app, ui, &base, &shown, t, playhead, |time| tm.frame_at(time), unit);
        });
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
