//! The Presets drawer: the library grouped by tag with a search field; a
//! preset's card with its description, its variables (sliders, switches,
//! menus and colours with the preset's ranges and defaults), where and on
//! what it goes, a live preview on the stage, and Apply, which writes it as
//! one edit through the code of `edits apply` (`App::apply_preset`).

use crate::app::App;
use crate::editing::{ease_choices, tidy};
use crate::presets::{self, AtChoice, TargetChoice};
use crate::theme::{self, color, radius, space, text};
use crate::widgets::{self, tip, Icon};
use eframe::egui::{self, pos2, vec2, Align, Align2, CornerRadius, FontId, Layout, RichText, Sense, Ui};
use edits_core::preset::{Preset, TargetKind, VarType};
use serde_json::{json, Map, Value};

fn tag_icon(tag: &str) -> Icon {
    match tag {
        "hit" => Icon::Zap,
        "zoom" => Icon::ZoomIn,
        "move" => Icon::Move,
        "3d" => Icon::Box,
        "blur" => Icon::Droplet,
        "speed" => Icon::Gauge,
        "look" => Icon::Film,
        "overlay" => Icon::Layers,
        "transition" => Icon::ArrowRightLeft,
        _ => Icon::Sparkles,
    }
}

fn title_case(tag: &str) -> String {
    match tag {
        "3d" => "3D".into(),
        "hit" => "Hits".into(),
        "transition" => "Transitions".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// The variables of `p` as editable rows. Returns whether a value changed.
pub(crate) fn var_editor(ui: &mut Ui, p: &Preset, values: &mut Map<String, Value>, slots: &[String], salt: &str) -> bool {
    let mut changed = false;
    if p.variables.is_empty() {
        ui.label(RichText::new("No variables.").font(theme::small()).color(color::TEXT_FAINT));
        return false;
    }
    for (name, var) in &p.variables {
        let current = values.get(name).cloned().unwrap_or_else(|| var.default.clone());
        let is_default = presets::given(p, &Map::from_iter([(name.clone(), current.clone())])).is_empty();
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 26.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            let (r, resp) = ui.allocate_exact_size(vec2(84.0, 26.0), Sense::hover());
            let fg = if is_default { color::TEXT_MUTED } else { color::ACCENT_TEXT };
            let g = widgets::one_line(ui.painter(), name, theme::body(), fg, r.width() - space::XS);
            ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, fg);
            let unit = var.unit.as_deref().map(|u| format!(" ({u})")).unwrap_or_default();
            let range = match (var.min, var.max) {
                (Some(a), Some(b)) => format!("\n{} to {}", tidy(a), tidy(b)),
                _ => String::new(),
            };
            resp.on_hover_text(format!("{}{unit}{range}\nDefault {}", var.doc, edits_core::jsonfmt::inline(&var.default)));
            // The reset button on the right, the control in between.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                let reset = ui.add_enabled_ui(!is_default, |ui| widgets::small_icon_button(ui, Icon::RotateCcw)).inner;
                if tip(reset, "Back to the default", &[]).clicked() {
                    values.insert(name.clone(), var.default.clone());
                    changed = true;
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    if let Some(v) = var_control(ui, p, name, var, &current, slots, salt) {
                        values.insert(name.clone(), v);
                        changed = true;
                    }
                });
            });
        });
    }
    changed
}

/// The control of one variable; the new value when it changed.
fn var_control(ui: &mut Ui, p: &Preset, name: &str, var: &edits_core::preset::Variable, current: &Value, slots: &[String], salt: &str) -> Option<Value> {
    let id = (salt, &p.name, name);
    let width = ui.available_width();
    match var.ty {
        VarType::Number | VarType::Integer => {
            let (lo, hi, step) = presets::number_range(var);
            let mut v = current.as_f64().or_else(|| current.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
            let r = ui
                .scope_builder(egui::UiBuilder::new().id_salt(id), |ui| {
                    ui.spacing_mut().slider_width = (width - 64.0).max(60.0);
                    // No step for numbers: a step would move the default onto its grid.
                    let mut s = egui::Slider::new(&mut v, lo..=hi).clamping(egui::SliderClamping::Always).custom_formatter(|v, _| crate::inspector::num(v));
                    if var.ty == VarType::Integer {
                        s = s.integer().step_by(step);
                    }
                    ui.add(s)
                })
                .inner;
            r.changed().then(|| if var.ty == VarType::Integer { json!(v.round() as i64) } else { json!(tidy(v)) })
        }
        VarType::Boolean => {
            let mut on = current.as_bool().unwrap_or(false);
            ui.scope_builder(egui::UiBuilder::new().id_salt(id), |ui| widgets::switch(ui, &mut on)).inner.changed().then(|| json!(on))
        }
        VarType::Enum => {
            let now = current.as_str().unwrap_or("").to_string();
            let mut out = None;
            widgets::combo(id, now.as_str(), (width - 8.0).max(80.0)).show_ui(ui, |ui| {
                for opt in var.options.as_ref().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
                    if ui.selectable_label(opt == now, &opt).clicked() && opt != now {
                        out = Some(json!(opt));
                    }
                }
            });
            out
        }
        VarType::Color => {
            let mut rgb = presets::rgb_of(current);
            let r = ui.scope_builder(egui::UiBuilder::new().id_salt(id), |ui| egui::color_picker::color_edit_button_rgb(ui, &mut rgb)).inner;
            ui.label(RichText::new(crate::inspector::vector(&rgb.iter().map(|c| *c as f64).collect::<Vec<_>>())).font(theme::mono_small()).color(color::TEXT_FAINT));
            r.changed().then(|| presets::with_rgb(current, rgb))
        }
        VarType::Vector => {
            let mut v: Vec<f64> = current.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            if v.is_empty() {
                v = var.default.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_else(|| vec![0.0]);
            }
            let each = ((width - 3.0 * (v.len() as f32 - 1.0)) / v.len() as f32).max(36.0);
            let mut changed = false;
            for (c, x) in v.iter_mut().enumerate() {
                let mut dv = egui::DragValue::new(x).speed(0.01).max_decimals(3).custom_formatter(|v, _| crate::inspector::num(v));
                if let (Some(a), Some(b)) = (var.min, var.max) {
                    dv = dv.range(a..=b);
                }
                changed |= crate::inspector::stable_drag(ui, (salt, &p.name, name, c), each, dv).changed();
            }
            changed.then(|| json!(v.iter().map(|x| tidy(*x)).collect::<Vec<_>>()))
        }
        VarType::String if name == "ease" || name == "slot" => {
            let now = current.as_str().unwrap_or("").to_string();
            let choices: Vec<String> = if name == "ease" { ease_choices().into_iter().filter(|n| *n != "bezier").map(str::to_string).collect() } else { slots.to_vec() };
            let mut out = None;
            widgets::combo(id, RichText::new(&now).font(theme::mono()), (width - 8.0).max(80.0)).show_ui(ui, |ui| {
                for c in choices {
                    if ui.selectable_label(c == now, &c).clicked() && c != now {
                        out = Some(json!(c));
                    }
                }
            });
            out
        }
        VarType::String => {
            let mut s = current.as_str().map(str::to_string).unwrap_or_else(|| current.to_string());
            let r = ui.add(egui::TextEdit::singleline(&mut s).id_salt(id).desired_width(width).margin(egui::Margin::symmetric(6, 3)));
            r.changed().then(|| json!(s))
        }
    }
}

/// A label in the label column of the card.
fn cell(ui: &mut Ui, label: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(84.0, 24.0), Sense::hover());
    ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, label, theme::body(), color::TEXT_MUTED);
}

/// The drawer's page.
pub(crate) fn drawer(app: &mut App, ui: &mut Ui) {
    let Some(project) = app.project().cloned() else {
        widgets::empty_state(ui, Icon::WandSparkles, "No project yet", "Presets appear when the project file is valid.", &[]);
        return;
    };
    let lib = app.library().clone();
    // ---- search ----
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        let r = ui.add(egui::TextEdit::singleline(&mut app.pre.query).hint_text("Search presets…").desired_width(ui.available_width() - 28.0).margin(egui::Margin::symmetric(8, 5)));
        let painter = ui.painter();
        if app.pre.query.is_empty() && !r.has_focus() {
            widgets::paint_icon(painter, pos2(r.rect.right() - 12.0, r.rect.center().y), Icon::Search, 12.0, color::TEXT_FAINT);
        }
        if tip(widgets::small_icon_button(ui, Icon::RefreshCw), "Read the presets again (the project's presets/ folder too)", &[]).clicked() {
            app.reload_presets();
        }
    });
    let target = match app.selected_layer() {
        Some(id) => format!("Apply to {id} at the playhead, or at its cut for transitions."),
        None => "No layer selected: presets go on the picture on screen at the playhead.".to_string(),
    };
    ui.label(RichText::new(target).font(theme::small()).color(color::TEXT_FAINT));
    for e in &lib.problems {
        ui.label(RichText::new(e).font(theme::small()).color(color::ERROR_TEXT));
    }
    ui.add_space(space::XS);
    let searching = !app.pre.query.trim().is_empty();
    let groups = presets::groups(&lib.presets, &app.pre.query);
    let slots: Vec<String> = project.slots.keys().cloned().collect();
    egui::ScrollArea::vertical().id_salt("presets-list").auto_shrink([false, false]).show(ui, |ui| {
        if groups.is_empty() {
            widgets::empty_state(ui, Icon::SearchX, "No preset matches", "Try a tag: hit, zoom, move, 3d, blur, transition.", &[]);
        }
        for (tag, list) in &groups {
            let title = format!("{} · {}", title_case(tag), list.len());
            widgets::section(ui, ("preset-group", tag.as_str(), searching), &title, searching || tag != "transition", |_| {}, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for &i in list {
                    preset_row(app, ui, &lib.presets[i], tag, &slots);
                }
            });
            widgets::section_rule(ui);
        }
        ui.add_space(space::L);
        ui.label(RichText::new("A preset writes ordinary keys, effects and layers: edit them afterwards like any other, Ctrl+Z takes the whole preset back. The agent applies the same presets with preset_apply.").font(theme::small()).color(color::TEXT_FAINT));
    });
}

fn preset_row(app: &mut App, ui: &mut Ui, p: &Preset, tag: &str, slots: &[String]) {
    let open = app.pre.open.as_deref() == Some(p.name.as_str());
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click());
    let fill = if open { color::ACCENT_BG } else if resp.hovered() { color::RAISED } else { egui::Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, radius::SMALL, fill);
    let icon_c = if open { color::ACCENT_TEXT } else { color::TEXT_FAINT };
    widgets::paint_icon(ui.painter(), pos2(rect.left() + 13.0, rect.top() + 13.0), tag_icon(tag), 13.0, icon_c);
    let name = p.name.strip_prefix("transition:").unwrap_or(&p.name);
    let left = rect.left() + 28.0;
    ui.painter().text(pos2(left, rect.top() + 6.0), Align2::LEFT_TOP, name, FontId::new(text::BODY, theme::medium()), color::TEXT);
    let desc = p.description.strip_prefix("Transition into the layer at its cut: ").unwrap_or(&p.description);
    let g = widgets::one_line(ui.painter(), desc, theme::small(), color::TEXT_FAINT, rect.right() - left - space::S);
    ui.painter().galley(pos2(left, rect.top() + 22.0), g, color::TEXT_FAINT);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{}\n\ntags: {}", p.description, p.tags.join(", ")));
    if resp.clicked() {
        if open {
            app.pre.open = None;
        } else {
            app.open_preset(&p.name);
        }
    }
    if open {
        card(app, ui, p, slots);
        if std::mem::take(&mut app.pre.scroll) {
            let shown = egui::Rect::from_min_max(rect.min, pos2(rect.right(), ui.cursor().top()));
            ui.scroll_to_rect(shown, None);
        }
    }
}

/// The open preset: variables, target, time, preview, Apply.
fn card(app: &mut App, ui: &mut Ui, p: &Preset, slots: &[String]) {
    ui.add_space(2.0);
    egui::Frame::new().fill(color::RAISED).corner_radius(CornerRadius::same(radius::NORMAL)).inner_margin(egui::Margin::same(space::M as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = space::S;
        ui.label(RichText::new(&p.description).color(color::TEXT_MUTED));
        ui.label(widgets::eyebrow("Variables"));
        let mut values = std::mem::take(&mut app.pre.values);
        var_editor(ui, p, &mut values, slots, "drawer");
        app.pre.values = values;
        // ---- what and when ----
        if p.target == TargetKind::Layer {
            ui.horizontal(|ui| {
                cell(ui, "On");
                let mut choice = app.pre.target;
                let options: Vec<(TargetChoice, &str)> = TargetChoice::ALL.iter().map(|c| (*c, c.label())).collect();
                if widgets::segmented(ui, &mut choice, &options) {
                    app.pre.target = choice;
                }
            })
            .response
            .on_hover_text(TargetChoice::ALL.iter().map(|c| format!("{}: {}", c.label(), c.tip())).collect::<Vec<_>>().join("\n"));
            ui.horizontal(|ui| {
                cell(ui, "At");
                let mut at = app.pre.at;
                if widgets::segmented(ui, &mut at, &[(AtChoice::Playhead, "Playhead"), (AtChoice::LayerIn, "Cut")]) {
                    app.pre.at = at;
                }
            })
            .response
            .on_hover_text("Playhead: where the playhead is. Cut: the target layer's in (where a transition belongs)."); 
        }
        ui.horizontal(|ui| {
            cell(ui, "Preview");
            widgets::switch(ui, &mut app.pre.preview).on_hover_text("Show the result at the playhead before applying it (nothing is written)");
        });
        let plan = app.state.doc.clone().zip(app.tm).map(|(doc, tm)| {
            let target = presets::target(p, app.pre.target, app.selected_layer().as_deref());
            let at = presets::at_time(&doc, app.pre.at, &target, tm.frame_time(app.playhead));
            (target, at, tm)
        });
        if app.pre.preview {
            app.preview_preset();
        }
        if let Some(e) = &app.pre.error {
            ui.label(RichText::new(e).font(theme::small()).color(color::ERROR_TEXT));
        }
        // ---- apply ----
        ui.horizontal(|ui| {
            let line = presets::var_line(p, &app.pre.values);
            if let Some((target, at, tm)) = &plan {
                let unit = tm.unit();
                let what = format!("at {} {unit} on {}", tidy(*at), presets::target_text(target));
                ui.add(egui::Label::new(RichText::new(what).font(theme::mono_small()).color(color::TEXT_FAINT)).truncate()).on_hover_text(format!(
                    "The same as: edits apply <project> {} --layer {} --at {}{}",
                    p.name,
                    presets::target_text(target),
                    tidy(*at),
                    if line.is_empty() { String::new() } else { format!(" {}", line.split(' ').map(|a| format!("--var {a}")).collect::<Vec<_>>().join(" ")) }
                ));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ok = app.pre.error.is_none() || !app.pre.preview;
                if tip(ui.add_enabled_ui(ok, |ui| widgets::button(ui, Some(Icon::Check), "Apply", true)).inner, "Write it to the project: one edit, Ctrl+Z takes it back", &[]).clicked() {
                    app.apply_preset();
                }
            });
        });
        if let Some(c) = &p.credit {
            ui.label(RichText::new(c).font(theme::small()).color(color::TEXT_FAINT));
        }
        if app.selected.is_none() && p.target == TargetKind::Layer && app.pre.target == TargetChoice::Selected {
            ui.label(RichText::new("Tip: click a layer in the timeline to apply it there.").font(theme::small()).color(color::TEXT_FAINT));
        }
    });
    ui.add_space(space::XS);
}
