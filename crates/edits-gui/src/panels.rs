//! The menu bar, the Slots page and the History list. Each change they make
//! goes through `App::commit` (or `App::edit_live` for a value being dragged).

use crate::app::{pal, shorten, App};
use crate::editing::{self, actor_label, ago, NewLayer};
use eframe::egui::{self, Button, Color32, RichText, Ui};
use edits_core::edit::{EditOp, SlotFit, AGENT, WINDOW};
use edits_core::Fit;
use std::time::SystemTime;

pub(crate) fn menu_bar(app: &mut App, ui: &mut Ui) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("Export video…").clicked() {
                app.open_export();
                ui.close();
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button("Edit", |ui| {
            let undo = app.history.undo.last().map(|e| format!("Undo {} ({})", shorten(&e.summary, 48), actor_label(&e.actor)));
            if ui.add_enabled(undo.is_some(), Button::new(undo.unwrap_or_else(|| "Undo".into())).shortcut_text("Ctrl+Z")).clicked() {
                app.undo();
                ui.close();
            }
            let redo = app.history.redo.last().map(|e| format!("Redo {} ({})", shorten(&e.summary, 48), actor_label(&e.actor)));
            if ui.add_enabled(redo.is_some(), Button::new(redo.unwrap_or_else(|| "Redo".into())).shortcut_text("Ctrl+Shift+Z")).clicked() {
                app.redo();
                ui.close();
            }
        });
        ui.menu_button("Layer", |ui| layer_menu(app, ui));
        ui.add_space(12.0);
        ui.weak("Edits are written to the project file at once, checked first, and can be undone (Ctrl+Z) by you or the agent.");
    });
}

/// Entries that add a layer: at the playhead, above the selected layer.
pub(crate) fn add_layer_entries(app: &mut App, ui: &mut Ui) {
    let Some(p) = app.project().cloned() else {
        ui.weak("No valid project yet.");
        return;
    };
    let slots: Vec<(String, String)> = p.slots.iter().map(|(n, s)| (n.clone(), s.path.clone().unwrap_or_else(|| "(empty)".into()))).collect();
    ui.menu_button("Image layer", |ui| {
        if slots.is_empty() {
            ui.weak("No slots yet: add one in the Slots tab.");
        }
        for (name, path) in &slots {
            if ui.button(name).on_hover_text(format!("A picture from slot \"{name}\": {path}")).clicked() {
                app.add_layer(NewLayer::Image(name.clone()));
                ui.close();
            }
        }
    });
    if ui.button("Solid layer").on_hover_text("A flat colour").clicked() {
        app.add_layer(NewLayer::Solid);
        ui.close();
    }
    if ui.button("Adjustment layer").on_hover_text("No picture: its effects apply to everything beneath it").clicked() {
        app.add_layer(NewLayer::Adjustment);
        ui.close();
    }
    ui.menu_button("Cube layer", |ui| {
        ui.weak("Picture on its faces:");
        for (name, path) in &slots {
            if ui.button(name).on_hover_text(format!("Slot \"{name}\": {path}")).clicked() {
                app.add_layer(NewLayer::Cube(name.clone()));
                ui.close();
            }
        }
    });
}

fn layer_menu(app: &mut App, ui: &mut Ui) {
    ui.weak("Add at the playhead, above the selected layer:");
    add_layer_entries(app, ui);
    ui.separator();
    let Some(id) = app.selected_layer() else {
        ui.weak("Select a layer in the timeline for more.");
        return;
    };
    ui.weak(format!("Layer \"{id}\":"));
    let enabled = app.project().and_then(|p| p.layers.iter().find(|l| l.id == id)).is_some_and(|l| l.enabled);
    if ui.button(if enabled { "Disable" } else { "Enable" }).clicked() {
        app.toggle_layer(&id);
        ui.close();
    }
    if ui.button("Duplicate").clicked() {
        app.duplicate_layer(&id);
        ui.close();
    }
    if ui.button("Move up (to the front)").clicked() {
        app.move_layer(&id, true);
        ui.close();
    }
    if ui.button("Move down (to the back)").clicked() {
        app.move_layer(&id, false);
        ui.close();
    }
    ui.menu_button("Add effect", |ui| effect_entries(app, ui, &id));
    ui.separator();
    if ui.button(RichText::new("Delete").color(pal::BAD)).clicked() {
        app.delete_layer(&id);
        ui.close();
    }
}

/// One entry per effect of the registry, each with its one-line documentation.
pub(crate) fn effect_entries(app: &mut App, ui: &mut Ui, id: &str) {
    let Some(layer) = app.project().and_then(|p| p.layers.iter().find(|l| l.id == id).cloned()) else { return };
    for spec in editing::addable_effects() {
        if ui.button(spec.kind).on_hover_text(spec.doc).clicked() {
            app.commit(vec![editing::add_effect_op(&layer, spec.kind)], format!("add {} to {id}", spec.kind));
            ui.close();
        }
    }
}

// ---- slots -----------------------------------------------------------------------

const DROP_HINT: &str = "Drop a picture file on a slot to use it there, or type its path (relative to the project file). \
Dropping files works on X11; on Wayland it depends on the compositor and may not reach the window: type the path instead.";

fn fit_text(f: Fit) -> &'static str {
    match f {
        Fit::Cover => "cover",
        Fit::Contain => "contain",
        Fit::Stretch => "stretch",
    }
}

pub(crate) fn slots(app: &mut App, ui: &mut Ui) {
    let (Some(base), Some(shown)) = (app.project().cloned(), app.shown_project()) else { return };
    ui.horizontal(|ui| {
        ui.heading("Slots");
        ui.label(RichText::new("(?)").color(ui.visuals().weak_text_color())).on_hover_text(DROP_HINT);
    });
    ui.weak("The pictures of the edit. Layers name a slot; change the slot to change the picture everywhere.");
    if app.files_hovering {
        ui.colored_label(pal::GOOD, "Drop the picture on the slot it is for.");
    }
    ui.add_space(4.0);
    let pointer = ui.ctx().input(|i| i.pointer.latest_pos());
    let dir = crate::reload::base_dir(&app.path);
    for (name, slot) in shown.slots.iter() {
        let users: Vec<&str> = base
            .layers
            .iter()
            .filter(|l| match &l.kind {
                edits_core::LayerKind::Image { slot } => slot == name,
                edits_core::LayerKind::Cube { faces, .. } => faces.contains(name),
                _ => false,
            })
            .map(|l| l.id.as_str())
            .collect();
        let current = slot.path.clone().unwrap_or_default();
        let frame = egui::Frame::new().fill(Color32::from_white_alpha(6)).stroke(egui::Stroke::new(1.0, Color32::from_white_alpha(20))).corner_radius(4.0).inner_margin(6.0);
        let resp = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.strong(name);
                ui.weak(if users.is_empty() { "not used by any layer".to_string() } else { format!("used by {}", users.join(", ")) });
            });
            ui.horizontal(|ui| {
                ui.label("path");
                let id = egui::Id::unique(("edits-slot-path", name));
                let focused = ui.memory(|m| m.has_focus(id));
                let text = app.slot_text.entry(name.clone()).or_default();
                if !focused {
                    *text = current.clone();
                }
                let r = ui.add(egui::TextEdit::singleline(text).id(id).hint_text("(empty: a placeholder)").desired_width(ui.available_width() - 4.0)).on_hover_text(DROP_HINT);
                if r.lost_focus() && text.trim() != current {
                    let typed = text.trim().to_string();
                    if typed.is_empty() {
                        app.commit(vec![EditOp::remove(format!("/slots/{}/path", edits_core::pointer::escape(name)))], format!("empty slot {name}"));
                    } else {
                        app.commit(vec![EditOp::set_slot(name.clone(), Some(typed.clone()), None, None)], format!("slot {name} = {typed}"));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("fit");
                egui::ComboBox::from_id_salt(("slot-fit", name)).selected_text(fit_text(slot.fit)).width(80.0).show_ui(ui, |ui| {
                    for (f, sf) in [(Fit::Cover, SlotFit::Cover), (Fit::Contain, SlotFit::Contain), (Fit::Stretch, SlotFit::Stretch)] {
                        if ui.selectable_label(slot.fit == f, fit_text(f)).clicked() && slot.fit != f {
                            app.commit(vec![EditOp::set_slot(name.clone(), None, Some(sf), None)], format!("fit of slot {name}: {}", fit_text(f)));
                        }
                    }
                });
                ui.label("focus").on_hover_text("The point of the picture kept in view by cover: x, y from 0 to 1");
                let focus = slot.focus;
                let base_focus = base.slots.get(name).map_or(focus, |s| s.focus);
                for c in 0..2 {
                    let mut v = focus[c];
                    let r = crate::inspector::stable_drag(ui, ("focus", name, c), egui::DragValue::new(&mut v).speed(0.005).range(0.0..=1.0).max_decimals(3));
                    if r.changed() {
                        let mut f = base_focus;
                        f[c] = editing::tidy(v);
                        app.edit_live(r.id, vec![EditOp::set_slot(name.clone(), None, None, Some(f))], format!("focus of slot {name}: [{}, {}]", f[0], f[1]));
                    }
                    if r.dragged() || r.has_focus() {
                        app.keep_live(r.id);
                    }
                }
            });
        });
        let rect = resp.response.rect;
        let over = pointer.is_some_and(|p| rect.contains(p));
        if app.files_hovering && over {
            ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(2.0, pal::GOOD), egui::StrokeKind::Outside);
        }
        if over && !app.dropped.is_empty() {
            let file = app.dropped.remove(0);
            app.dropped.clear();
            let path = editing::slot_path(&file, &dir);
            app.commit(vec![EditOp::set_slot(name.clone(), Some(path.clone()), None, None)], format!("slot {name} = {path}"));
        }
        ui.add_space(3.0);
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("New slot");
        let r = ui.add(egui::TextEdit::singleline(&mut app.new_slot).hint_text("name").desired_width(120.0));
        let name = app.new_slot.trim().to_string();
        let taken = base.slots.contains_key(&name);
        let ok = !name.is_empty() && !taken && !name.contains('/');
        let add = ui.add_enabled(ok, Button::new("Add slot")).on_disabled_hover_text(if taken { "A slot has that name" } else { "Type a name" });
        if add.clicked() || (ok && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            app.commit(vec![EditOp::set_slot(name.clone(), None, Some(SlotFit::Cover), None)], format!("add slot {name}"));
            app.new_slot.clear();
        }
    });
}

// ---- history ---------------------------------------------------------------------

fn actor_chip(ui: &mut Ui, actor: &str, dim: bool) {
    let color = match actor {
        AGENT => pal::KEY_EFFECT,
        WINDOW => pal::KEY_TRANSFORM,
        _ => Color32::GRAY,
    };
    let color = if dim { color.gamma_multiply(0.45) } else { color };
    egui::Frame::new().fill(color.gamma_multiply(0.18)).stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.7))).corner_radius(3.0).inner_margin(egui::Margin::symmetric(4, 0)).show(ui, |ui| {
        ui.label(RichText::new(actor_label(actor)).small().color(color));
    });
}

/// The shared history: what can be redone above the line, what can be undone below it, newest first.
pub(crate) fn history(app: &mut App, ui: &mut Ui) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("History").on_hover_text(
            "Every edit to the project file, by you in this window or by the agent (MCP project_edit), newest first.\n\
             Undo (Ctrl+Z) takes back the newest one, whoever made it; Redo (Ctrl+Shift+Z or Ctrl+Y) brings it back until the next edit.\n\
             The versions are kept in .edits/history/ next to the project.",
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!app.history.redo.is_empty(), Button::new("Redo").small()).on_hover_text("Ctrl+Shift+Z or Ctrl+Y").clicked() {
                app.redo();
            }
            if ui.add_enabled(!app.history.undo.is_empty(), Button::new("Undo").small()).on_hover_text("Ctrl+Z").clicked() {
                app.undo();
            }
        });
    });
    let now = SystemTime::now();
    let row = |ui: &mut Ui, e: &edits_core::edit::HistoryEntry, undone: bool| {
        ui.horizontal(|ui| {
            actor_chip(ui, &e.actor, undone);
            let summary = if e.summary.is_empty() { "(no description)".to_string() } else { e.summary.replace('→', "->") };
            let text = if undone { RichText::new(shorten(&summary, 46)).color(ui.visuals().weak_text_color()) } else { RichText::new(shorten(&summary, 46)) };
            ui.label(text).on_hover_text(format!("{summary}\n{}{}", if undone { "undone; Redo brings it back · " } else { "" }, ago(now, e.time)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(ago(now, e.time)).small().color(ui.visuals().weak_text_color()));
            });
        });
    };
    let (undo, redo) = (&app.history.undo, &app.history.redo);
    if undo.is_empty() && redo.is_empty() {
        ui.weak("No edits yet.");
        return;
    }
    egui::ScrollArea::vertical().id_salt("history-list").auto_shrink([false, false]).show(ui, |ui| {
        for e in redo.iter().rev().take(5).collect::<Vec<_>>().into_iter().rev() {
            row(ui, e, true);
        }
        if !redo.is_empty() {
            ui.label(RichText::new("— above: undone (Redo) · below: done (Undo) —").small().color(ui.visuals().weak_text_color()));
        }
        for e in undo.iter().rev().take(20) {
            row(ui, e, false);
        }
        if undo.len() > 20 {
            ui.weak(format!("… {} older", undo.len() - 20));
        }
    });
}
