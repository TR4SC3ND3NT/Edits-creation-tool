//! The chrome around the preview: the top bar, the rail on the left and its
//! drawers (Slots, History), and the menus that add layers and effects.
//! Each change they make goes through `App::commit` (or `App::edit_live`
//! for a value being dragged).

use crate::app::{shorten, App, Cmd, Drawer};
use crate::editing::{self, actor_label, ago, NewLayer};
use crate::theme::{self, color, radius, size, space, text};
use crate::widgets::{self, tip, Icon};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use edits_core::edit::{EditOp, SlotFit, AGENT, WINDOW};
use edits_core::Fit;
use std::time::{Duration, SystemTime};

// ---- top bar -----------------------------------------------------------------------

/// Project name and state on the left, the command field in the middle,
/// undo, redo and Export on the right.
pub(crate) fn top_bar(app: &mut App, ui: &mut Ui) {
    let row = ui.max_rect();
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::M;
        // The mark.
        let (mark, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
        ui.painter().rect_filled(mark, radius::NORMAL, color::ACCENT);
        widgets::paint_icon(ui.painter(), mark.center(), Icon::Clapperboard, 13.0, Color32::WHITE);
        // Folder / file.
        let folder = app.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let file = app.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            if !folder.is_empty() {
                ui.label(RichText::new(format!("{folder} /")).font(FontId::new(text::TITLE, theme::medium())).color(color::TEXT_FAINT));
            }
            ui.label(RichText::new(file).font(FontId::new(text::TITLE, theme::medium())).color(color::TEXT));
        });
        name.response.on_hover_text(app.path.display().to_string());
        // State: errors, or the revision, and the agent's last touch.
        if app.state.rejected {
            pill(ui, Some(Icon::CircleAlert), &format!("{} error{}", app.state.errors(), if app.state.errors() == 1 { "" } else { "s" }), color::ERROR_TEXT, color::ERROR_BG)
                .on_hover_text("The file on disk has errors: the window keeps showing the last valid version. See the inspector.");
        } else {
            pill(ui, None, &format!("rev {}", app.state.revision), color::TEXT_FAINT, color::RAISED).on_hover_text("Counts the versions of the project file that were loaded; it goes up when a change lands");
        }
        if let Some(e) = app.history.undo.last().filter(|e| e.actor == AGENT) {
            let fresh = e.time.and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|d| d < Duration::from_secs(60));
            if fresh {
                pill(ui, Some(Icon::Bot), "agent", color::AGENT, color::RAISED).on_hover_text(format!("The agent's last edit: {}\n{}", e.summary, ago(SystemTime::now(), e.time)));
            }
        }
    });

    // The command field, centred in the window.
    let field_w = 360.0f32.min(row.width() - 560.0).max(180.0);
    let field = Rect::from_center_size(pos2(row.center().x, row.center().y), vec2(field_w, 28.0));
    let resp = ui.interact(field, ui.scope_id().with("palette-field"), Sense::click());
    let fill = if resp.hovered() { color::CONTROL_HOVER } else { color::CONTROL };
    ui.painter().rect_filled(field, radius::NORMAL, fill);
    widgets::paint_icon(ui.painter(), pos2(field.left() + 16.0, field.center().y), Icon::Search, text::ICON_SMALL, color::TEXT_FAINT);
    ui.painter().text(pos2(field.left() + 32.0, field.center().y), Align2::LEFT_CENTER, "Search or run a command…", theme::body(), color::TEXT_FAINT);
    let mut x = field.right() - space::S;
    for k in ["K", "Ctrl"] {
        let g = ui.painter().layout_no_wrap(k.to_string(), theme::mono_small(), color::TEXT_MUTED);
        let w = (g.size().x + 8.0).max(16.0);
        let cap = Rect::from_min_size(pos2(x - w, field.center().y - 8.0), vec2(w, 16.0));
        ui.painter().rect(cap, radius::SMALL, color::RAISED, Stroke::new(1.0, color::LINE_STRONG), StrokeKind::Inside);
        ui.painter().galley(cap.center() - g.size() / 2.0, g, color::TEXT_MUTED);
        x -= w + 3.0;
    }
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.palette.open();
    }

    // Undo, redo, export.
    let right = Rect::from_min_max(pos2(field.right() + space::L, row.top()), row.right_bottom());
    ui.scope_builder(egui::UiBuilder::new().max_rect(right).layout(Layout::right_to_left(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if tip(widgets::button(ui, Some(Icon::Download), "Export", true), "Render the project to a video file", &["Ctrl", "E"]).clicked() {
            app.open_export();
        }
        ui.add_space(space::M);
        let redo = app.history.redo.last().map(|e| format!("Redo: {} ({})", shorten(&e.summary, 48), actor_label(&e.actor)));
        let r = ui.add_enabled_ui(redo.is_some(), |ui| widgets::icon_button(ui, Icon::Redo2)).inner;
        if tip(r, redo.as_deref().unwrap_or("Nothing to redo"), &["Ctrl", "Shift", "Z"]).clicked() {
            app.run(Cmd::Redo);
        }
        let undo = app.history.undo.last().map(|e| format!("Undo: {} ({})", shorten(&e.summary, 48), actor_label(&e.actor)));
        let r = ui.add_enabled_ui(undo.is_some(), |ui| widgets::icon_button(ui, Icon::Undo2)).inner;
        if tip(r, undo.as_deref().unwrap_or("Nothing to undo"), &["Ctrl", "Z"]).clicked() {
            app.run(Cmd::Undo);
        }
    });
}

/// A small rounded label with an optional icon.
fn pill(ui: &mut Ui, icon: Option<Icon>, label: &str, fg: Color32, fill: Color32) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::small_medium(), fg);
    let icon_w = if icon.is_some() { 16.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 14.0 + icon_w, 20.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    if let Some(i) = icon {
        widgets::paint_icon(ui.painter(), pos2(rect.left() + 13.0, rect.center().y), i, 12.0, fg);
    }
    ui.painter().galley(pos2(rect.left() + 7.0 + icon_w, rect.center().y - galley.size().y / 2.0), galley, fg);
    resp
}

// ---- rail and drawers ----------------------------------------------------------------

/// The column of icons on the left: drawers at the top, help at the bottom.
pub(crate) fn rail(app: &mut App, ui: &mut Ui) {
    let full = ui.max_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(full.shrink2(vec2(0.0, space::M))).layout(Layout::top_down(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        for d in Drawer::ALL {
            let on = app.drawer == Some(d);
            if tip(widgets::icon_toggle(ui, d.icon(), on), d.tip(), &d.keys().split('+').collect::<Vec<_>>()).clicked() {
                app.run(Cmd::Drawer(d));
            }
        }
        // Room for more drawers (presets) here.
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(full.shrink2(vec2(0.0, space::M))).layout(Layout::bottom_up(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        if tip(widgets::icon_button(ui, Icon::Keyboard), "Keyboard shortcuts", &["?"]).clicked() {
            app.run(Cmd::Shortcuts);
        }
        if tip(widgets::icon_button(ui, Icon::Command), "Command palette", &["Ctrl", "K"]).clicked() {
            app.run(Cmd::Palette);
        }
    });
}

/// The open drawer: a title row and its page.
pub(crate) fn drawer(app: &mut App, ui: &mut Ui, d: Drawer) {
    ui.add_space(space::S);
    ui.horizontal(|ui| {
        ui.set_height(28.0);
        ui.add_space(space::L);
        ui.label(widgets::icon(d.icon()).color(color::TEXT_FAINT));
        ui.label(RichText::new(d.title()).font(FontId::new(text::TITLE, theme::strong())).color(color::TEXT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(space::S);
            if tip(widgets::small_icon_button(ui, Icon::X), "Close", &["Esc"]).clicked() {
                app.drawer = None;
            }
            if d == Drawer::History {
                let r = ui.add_enabled_ui(!app.history.redo.is_empty(), |ui| widgets::small_icon_button(ui, Icon::Redo2)).inner;
                if tip(r, "Redo", &["Ctrl", "Shift", "Z"]).clicked() {
                    app.redo();
                }
                let r = ui.add_enabled_ui(!app.history.undo.is_empty(), |ui| widgets::small_icon_button(ui, Icon::Undo2)).inner;
                if tip(r, "Undo the newest edit, yours or the agent's", &["Ctrl", "Z"]).clicked() {
                    app.undo();
                }
            }
        });
    });
    ui.add_space(space::XS);
    widgets::section_rule(ui);
    egui::Frame::new().inner_margin(egui::Margin { left: space::L as i8, right: space::L as i8, top: space::M as i8, bottom: 0 }).show(ui, |ui| match d {
        Drawer::Slots => slots(app, ui),
        Drawer::History => history(app, ui),
    });
}

// ---- menus -----------------------------------------------------------------------------

/// Entries that add a layer: at the playhead, above the selected layer.
pub(crate) fn add_layer_entries(app: &mut App, ui: &mut Ui) {
    ui.set_width(240.0);
    let Some(p) = app.project().cloned() else {
        ui.label(RichText::new("No valid project yet.").color(color::TEXT_FAINT));
        return;
    };
    let mut chosen = None;
    ui.label(widgets::eyebrow("Add at the playhead"));
    if widgets::menu_item(ui, Some(Icon::Square), "Solid", None).on_hover_text("A flat colour").clicked() {
        chosen = Some(NewLayer::Solid);
    }
    if widgets::menu_item(ui, Some(Icon::SlidersHorizontal), "Adjustment", None).on_hover_text("No picture: its effects apply to everything beneath it").clicked() {
        chosen = Some(NewLayer::Adjustment);
    }
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        for (title, icon, cube) in [("Image from slot", Icon::Image, false), ("Cube with slot", Icon::Box, true)] {
            widgets::divider(ui);
            ui.label(widgets::eyebrow(title));
            if p.slots.is_empty() {
                ui.label(RichText::new("No slots yet: add one in the Slots drawer.").font(theme::small()).color(color::TEXT_FAINT));
            }
            for (name, slot) in &p.slots {
                let path = slot.path.clone().unwrap_or_else(|| "empty".into());
                let r = widgets::menu_item(ui, Some(icon), name, Some(&shorten(&path, 22)));
                if r.on_hover_text(format!("Slot \"{name}\": {path}")).clicked() {
                    chosen = Some(if cube { NewLayer::Cube(name.clone()) } else { NewLayer::Image(name.clone()) });
                }
            }
        }
    });
    if let Some(kind) = chosen {
        app.add_layer(kind);
        ui.close();
    }
}

/// One entry per effect of the registry, each with its one-line documentation:
/// the kind chosen, if any.
pub(crate) fn effect_menu(ui: &mut Ui) -> Option<&'static str> {
    ui.set_width(220.0);
    let mut chosen = None;
    ui.label(widgets::eyebrow("Add effect"));
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for spec in editing::addable_effects() {
            if widgets::menu_item(ui, Some(Icon::Sparkles), spec.kind, None).on_hover_text(spec.doc).clicked() {
                chosen = Some(spec.kind);
            }
        }
    });
    chosen
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
    let (Some(base), Some(shown)) = (app.project().cloned(), app.shown_project()) else {
        widgets::empty_state(ui, Icon::Images, "No project yet", "The slots appear when the project file is valid.", &[]);
        return;
    };
    ui.label(RichText::new("The pictures of the edit. Layers name a slot: change the slot to change the picture everywhere.").font(theme::small()).color(color::TEXT_FAINT))
        .on_hover_text(DROP_HINT);
    if app.files_hovering {
        ui.label(RichText::new("Drop the picture on the slot it is for.").color(color::ACCENT_TEXT));
    }
    ui.add_space(space::S);
    let pointer = ui.ctx().input(|i| i.pointer.latest_pos());
    let dir = crate::reload::base_dir(&app.path);
    egui::ScrollArea::vertical().id_salt("slots").auto_shrink([false, false]).show(ui, |ui| {
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
            let frame = egui::Frame::new().fill(color::RAISED).corner_radius(CornerRadius::same(radius::NORMAL)).inner_margin(egui::Margin::same(space::M as i8));
            let resp = frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = space::S;
                ui.horizontal(|ui| {
                    ui.label(widgets::icon_sized(Icon::Image, text::ICON_SMALL).color(color::TEXT_FAINT));
                    ui.label(RichText::new(name).font(FontId::new(text::BODY, theme::medium())).color(color::TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let used = if users.is_empty() { "unused".to_string() } else { format!("{} layer{}", users.len(), if users.len() == 1 { "" } else { "s" }) };
                        ui.label(RichText::new(used).font(theme::small()).color(color::TEXT_FAINT))
                            .on_hover_text(if users.is_empty() { "Not used by any layer".to_string() } else { format!("Used by {}", users.join(", ")) });
                    });
                });
                let id = egui::Id::unique(("edits-slot-path", name));
                let focused = ui.memory(|m| m.has_focus(id));
                let text = app.slot_text.entry(name.clone()).or_default();
                if !focused {
                    *text = current.clone();
                }
                let r = ui
                    .add(egui::TextEdit::singleline(text).id(id).font(theme::mono()).hint_text("empty: a placeholder").desired_width(ui.available_width()).margin(egui::Margin::symmetric(6, 4)))
                    .on_hover_text(DROP_HINT);
                if r.lost_focus() && text.trim() != current {
                    let typed = text.trim().to_string();
                    if typed.is_empty() {
                        app.commit(vec![EditOp::remove(format!("/slots/{}/path", edits_core::pointer::escape(name)))], format!("empty slot {name}"));
                    } else {
                        app.commit(vec![EditOp::set_slot(name.clone(), Some(typed.clone()), None, None)], format!("slot {name} = {typed}"));
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Fit").color(color::TEXT_MUTED));
                    widgets::combo(("slot-fit", name), fit_text(slot.fit), 84.0).show_ui(ui, |ui| {
                        for (f, sf) in [(Fit::Cover, SlotFit::Cover), (Fit::Contain, SlotFit::Contain), (Fit::Stretch, SlotFit::Stretch)] {
                            if ui.selectable_label(slot.fit == f, fit_text(f)).clicked() && slot.fit != f {
                                app.commit(vec![EditOp::set_slot(name.clone(), None, Some(sf), None)], format!("fit of slot {name}: {}", fit_text(f)));
                            }
                        }
                    });
                    ui.add_space(space::XS);
                    ui.label(RichText::new("Focus").color(color::TEXT_MUTED)).on_hover_text("The point of the picture kept in view by cover: x, y from 0 to 1");
                    let focus = slot.focus;
                    let base_focus = base.slots.get(name).map_or(focus, |s| s.focus);
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for c in 0..2 {
                        let mut v = focus[c];
                        let r = crate::inspector::stable_drag(ui, ("focus", name, c), 46.0, egui::DragValue::new(&mut v).speed(0.005).range(0.0..=1.0).max_decimals(3));
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
                ui.painter().rect_stroke(rect, radius::NORMAL, Stroke::new(1.5, color::ACCENT), StrokeKind::Outside);
            }
            if over && !app.dropped.is_empty() {
                let file = app.dropped.remove(0);
                app.dropped.clear();
                let path = editing::slot_path(&file, &dir);
                app.commit(vec![EditOp::set_slot(name.clone(), Some(path.clone()), None, None)], format!("slot {name} = {path}"));
            }
            ui.add_space(space::S);
        }
        if shown.slots.is_empty() {
            widgets::empty_state(ui, Icon::ImageOff, "No slots yet", "Name one below to add it, then type its picture's path.", &[]);
        }
        ui.add_space(space::S);
        ui.label(widgets::eyebrow("New slot"));
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut app.new_slot).hint_text("name").desired_width((ui.available_width() - 70.0).max(80.0)).margin(egui::Margin::symmetric(6, 4)));
            let name = app.new_slot.trim().to_string();
            let taken = base.slots.contains_key(&name);
            let ok = !name.is_empty() && !taken && !name.contains('/');
            let add = ui.add_enabled_ui(ok, |ui| widgets::button(ui, Some(Icon::Plus), "Add", false)).inner.on_disabled_hover_text(if taken { "A slot has that name" } else { "Type a name" });
            if add.clicked() || (ok && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                app.commit(vec![EditOp::set_slot(name.clone(), None, Some(SlotFit::Cover), None)], format!("add slot {name}"));
                app.new_slot.clear();
            }
        });
    });
}

// ---- history ---------------------------------------------------------------------

/// Who made an edit: an icon and a colour.
fn actor_look(actor: &str) -> (Icon, Color32) {
    match actor {
        AGENT => (Icon::Bot, color::AGENT),
        WINDOW => (Icon::User, color::TEXT_MUTED),
        _ => (Icon::FileJson, color::TEXT_FAINT),
    }
}

/// The shared history: what can be redone above the line, what can be undone below it, newest first.
pub(crate) fn history(app: &mut App, ui: &mut Ui) {
    let (undo, redo) = (&app.history.undo, &app.history.redo);
    if undo.is_empty() && redo.is_empty() {
        widgets::empty_state(ui, Icon::History, "No edits yet", "Your edits and the agent's appear here, newest first. Ctrl+Z undoes the newest, whoever made it.", &[]);
        return;
    }
    let now = SystemTime::now();
    let row = |ui: &mut Ui, e: &edits_core::edit::HistoryEntry, undone: bool, newest: bool| {
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
        if resp.hovered() {
            ui.painter().rect_filled(rect, radius::SMALL, color::RAISED);
        }
        let (icon, tint) = actor_look(&e.actor);
        let alpha = if undone { 0.5 } else { 1.0 };
        let badge = Rect::from_center_size(pos2(rect.left() + 14.0, rect.top() + 14.0), vec2(22.0, 22.0));
        ui.painter().rect_filled(badge, radius::NORMAL, color::CONTROL.gamma_multiply(alpha));
        widgets::paint_icon(ui.painter(), badge.center(), icon, 12.0, tint.gamma_multiply(alpha));
        let summary = if e.summary.is_empty() { "(no description)".to_string() } else { editing::readable_summary(&e.summary) };
        let text_left = rect.left() + 34.0;
        let max_w = rect.right() - text_left - space::XS;
        let fg = if undone { color::TEXT_FAINT } else { color::TEXT };
        let galley = widgets::one_line(ui.painter(), &summary, theme::body(), fg, max_w);
        ui.painter().galley(pos2(text_left, rect.top() + 6.0), galley, fg);
        let meta = format!("{} · {}{}", actor_label(&e.actor), ago(now, e.time), if undone { " · undone" } else if newest { " · Ctrl+Z undoes it" } else { "" });
        ui.painter().text(pos2(text_left, rect.top() + 23.0), Align2::LEFT_TOP, meta, theme::small(), color::TEXT_FAINT);
        resp.on_hover_text(format!("{}\n{}{}", e.summary, if undone { "Undone: Redo brings it back · " } else { "" }, ago(now, e.time)));
    };
    egui::ScrollArea::vertical().id_salt("history-list").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for e in redo.iter().rev().take(5).collect::<Vec<_>>().into_iter().rev() {
            row(ui, e, true, false);
        }
        if !redo.is_empty() {
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
                let label = ui.painter().text(r.center(), Align2::CENTER_CENTER, "undone above · done below", theme::small(), color::TEXT_FAINT);
                ui.painter().hline(r.left()..=label.left() - space::S, r.center().y, Stroke::new(1.0, color::LINE_STRONG));
                ui.painter().hline(label.right() + space::S..=r.right(), r.center().y, Stroke::new(1.0, color::LINE_STRONG));
            });
            ui.add_space(space::XS);
        }
        for (i, e) in undo.iter().rev().take(30).enumerate() {
            row(ui, e, false, i == 0);
        }
        if undo.len() > 30 {
            ui.label(RichText::new(format!("… {} older", undo.len() - 30)).font(theme::small()).color(color::TEXT_FAINT));
        }
        let _ = size::ROW;
    });
}
