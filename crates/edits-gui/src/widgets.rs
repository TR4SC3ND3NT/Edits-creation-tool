//! The window's own small widgets, drawn with the tokens of `theme`: icon
//! buttons (Lucide glyphs), tooltips that name an action and its shortcut,
//! collapsible sections, menu rows, key caps and empty states.

use crate::theme::{self, color, radius, size, space, text};
use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui};
pub use lucide_icons::Icon;

/// The character of a Lucide icon (drawn with the `icons` family of `theme`).
pub fn glyph(icon: Icon) -> char {
    char::from(icon)
}

/// An icon as text, at a size, for labels and buttons.
pub fn icon(icon: Icon) -> RichText {
    icon_sized(icon, text::ICON)
}

pub fn icon_sized(icon: Icon, size: f32) -> RichText {
    RichText::new(glyph(icon).to_string()).font(theme::icon_font(size))
}

/// Paint an icon centred on `at`.
pub fn paint_icon(painter: &egui::Painter, at: egui::Pos2, i: Icon, size: f32, color: Color32) -> Rect {
    painter.text(at, Align2::CENTER_CENTER, glyph(i), theme::icon_font(size), color)
}

/// Text for a section title ("eyebrow"): small, medium weight, spaced.
pub fn eyebrow(text: impl Into<String>) -> RichText {
    RichText::new(text).text_style(theme::label_style()).extra_letter_spacing(0.4).color(color::TEXT_FAINT)
}

/// A tooltip that names the action, then its keys in key caps.
pub fn tip(resp: Response, action: &str, keys: &[&str]) -> Response {
    if keys.is_empty() {
        return resp.on_hover_text(action);
    }
    resp.on_hover_ui(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.label(RichText::new(action).color(color::TEXT));
            ui.add_space(space::XS);
            for k in keys {
                kbd(ui, k);
            }
        });
    })
}

/// A key cap: `Ctrl`, `K`, `Space`.
pub fn kbd(ui: &mut Ui, key: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(key.to_string(), theme::mono_small(), color::TEXT_MUTED);
    let size = vec2((galley.size().x + 8.0).max(16.0), 16.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(rect, radius::SMALL, color::CONTROL, Stroke::new(1.0, color::LINE_STRONG), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color::TEXT_MUTED);
    resp
}

/// Key caps for a shortcut written `Ctrl+Shift+Z`.
pub fn shortcut(ui: &mut Ui, keys: &str) {
    ui.spacing_mut().item_spacing.x = 3.0;
    for k in keys.split('+') {
        kbd(ui, k);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Quiet,
    On,
    Danger,
}

fn icon_button_impl(ui: &mut Ui, i: Icon, side: f32, glyph_size: f32, look: Look) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hot = enabled && (resp.hovered() || resp.has_focus());
        let down = enabled && resp.is_pointer_button_down_on();
        let fill = match (look, hot, down) {
            (Look::On, _, _) => color::ACCENT_BG,
            (_, _, true) => color::CONTROL_ACTIVE,
            (_, true, _) => color::CONTROL_HOVER,
            _ => Color32::TRANSPARENT,
        };
        let fg = match look {
            _ if !enabled => color::TEXT_FAINT.gamma_multiply(0.7),
            Look::On => color::ACCENT_TEXT,
            Look::Danger if hot => color::ERROR_TEXT,
            _ if hot => color::TEXT,
            _ => color::TEXT_MUTED,
        };
        ui.painter().rect_filled(rect, radius::SMALL, fill);
        paint_icon(ui.painter(), rect.center(), i, glyph_size, fg);
    }
    if ui.is_enabled() {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A frameless icon button for toolbars: fill only on hover.
pub fn icon_button(ui: &mut Ui, i: Icon) -> Response {
    icon_button_impl(ui, i, size::ICON_BUTTON, text::ICON, Look::Quiet)
}

/// A smaller icon button for rows (inspector, lists).
pub fn small_icon_button(ui: &mut Ui, i: Icon) -> Response {
    icon_button_impl(ui, i, 22.0, text::ICON_SMALL, Look::Quiet)
}

/// A small icon button whose action removes something.
pub fn danger_icon_button(ui: &mut Ui, i: Icon) -> Response {
    icon_button_impl(ui, i, 22.0, text::ICON_SMALL, Look::Danger)
}

/// An icon button that stays lit while `on`.
pub fn icon_toggle(ui: &mut Ui, i: Icon, on: bool) -> Response {
    icon_button_impl(ui, i, size::ICON_BUTTON, text::ICON, if on { Look::On } else { Look::Quiet })
}

/// A button with a fill: `primary` uses the accent.
pub fn button(ui: &mut Ui, i: Option<Icon>, label: &str, primary: bool) -> Response {
    let font = FontId::new(text::BODY, theme::medium());
    let fg = if primary { Color32::WHITE } else { color::TEXT };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, fg);
    let icon_w = if i.is_some() { text::ICON_SMALL + space::S } else { 0.0 };
    let pad = if i.is_some() { 10.0 } else { 12.0 };
    let size = vec2(galley.size().x + icon_w + pad * 2.0, size::BUTTON);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hot = enabled && resp.hovered();
        let down = enabled && resp.is_pointer_button_down_on();
        let fill = match (primary, hot, down) {
            (true, _, true) => color::ACCENT,
            (true, true, _) => color::ACCENT_HOVER,
            (true, _, _) => color::ACCENT,
            (false, _, true) => color::CONTROL_ACTIVE,
            (false, true, _) => color::CONTROL_HOVER,
            _ => color::CONTROL,
        };
        let alpha = if enabled { 1.0 } else { 0.42 };
        ui.painter().rect_filled(rect, radius::SMALL, fill.gamma_multiply(alpha));
        let mut x = rect.left() + pad;
        if let Some(i) = i {
            paint_icon(ui.painter(), pos2(x + text::ICON_SMALL / 2.0, rect.center().y), i, text::ICON_SMALL, fg.gamma_multiply(alpha));
            x += icon_w;
        }
        ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg.gamma_multiply(alpha));
    }
    if ui.is_enabled() {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A row of a menu or popover: icon, text, and the shortcut on the right.
pub fn menu_item(ui: &mut Ui, i: Option<Icon>, label: &str, keys: Option<&str>) -> Response {
    menu_item_colored(ui, i, label, keys, color::TEXT)
}

pub fn menu_item_colored(ui: &mut Ui, i: Option<Icon>, label: &str, keys: Option<&str>, fg: Color32) -> Response {
    let width = ui.available_width().max(180.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, size::ROW), Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hot = enabled && (resp.hovered() || resp.has_focus());
        if hot {
            ui.painter().rect_filled(rect, radius::SMALL, color::CONTROL_HOVER);
        }
        let fg = if enabled { fg } else { color::TEXT_FAINT };
        let mut x = rect.left() + space::M;
        if let Some(i) = i {
            paint_icon(ui.painter(), pos2(x + 7.0, rect.center().y), i, text::ICON_SMALL, if hot { fg } else { color::TEXT_MUTED.lerp_to_gamma(fg, 0.2) });
            x += 14.0 + space::M;
        }
        ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, theme::body(), fg);
        if let Some(keys) = keys {
            ui.painter().text(pos2(rect.right() - space::M, rect.center().y), Align2::RIGHT_CENTER, keys, theme::mono_small(), color::TEXT_FAINT);
        }
    }
    resp
}

/// Text laid out on one line, cut with "…" to fit `max_width`.
pub fn one_line(painter: &egui::Painter, text: &str, font: FontId, color: Color32, max_width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping { max_width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    painter.layout_job(job)
}

/// A drop-down with the window's chevron instead of egui's triangle.
pub fn combo(id: impl std::hash::Hash + std::fmt::Debug, selected: impl Into<egui::WidgetText>, width: f32) -> egui::ComboBox<'static> {
    egui::ComboBox::from_id_salt(id).selected_text(selected).width(width).icon(|ui, rect, _visuals, open| {
        paint_icon(ui.painter(), rect.center(), if open { Icon::ChevronUp } else { Icon::ChevronDown }, 12.0, color::TEXT_FAINT);
    })
}

/// A thin divider inside menus and panels.
pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, color::LINE));
}

/// A collapsible section: a header row with a chevron and an eyebrow
/// title, `right` drawn at the right end of the header (buttons that
/// should not toggle it), and the body when open. Remembers being open.
pub fn section<R>(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, title: &str, default_open: bool, right: impl FnOnce(&mut Ui), body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let id = ui.make_persistent_id(("section", id));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let open = state.is_open();
    if resp.clicked() {
        state.toggle(ui);
    }
    let hot = resp.hovered();
    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
    paint_icon(ui.painter(), pos2(rect.left() + 7.0, rect.center().y), chevron, 13.0, if hot { color::TEXT_MUTED } else { color::TEXT_FAINT });
    let title_color = if hot { color::TEXT } else { color::TEXT_MUTED };
    ui.painter().text(
        pos2(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::new(text::SMALL, theme::medium()),
        title_color,
    );
    // The right-hand controls get their own little ui over the header.
    let right_rect = Rect::from_min_max(pos2(rect.left() + 120.0, rect.top()), rect.right_bottom());
    ui.scope_builder(egui::UiBuilder::new().max_rect(right_rect).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        right(ui);
    });
    resp.on_hover_cursor(CursorIcon::PointingHand);
    state.store(ui.ctx());
    let out = state.show_body_unindented(ui, |ui| {
        let r = body(ui);
        ui.add_space(space::S);
        r
    });
    out.map(|o| o.inner)
}

/// The line between sections.
pub fn section_rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, color::LINE));
}

/// What a panel says when it has nothing to show: an icon, a line in
/// medium weight, and what to do (with key caps for `hints`).
pub fn empty_state(ui: &mut Ui, i: Icon, title: &str, body: &str, hints: &[(&str, &str)]) {
    ui.vertical_centered(|ui| {
        ui.add_space(space::XL);
        let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        ui.painter().rect_filled(rect, radius::LARGE, color::RAISED);
        paint_icon(ui.painter(), rect.center(), i, 18.0, color::TEXT_FAINT);
        ui.add_space(space::M);
        ui.label(RichText::new(title).font(FontId::new(text::BODY, theme::medium())).color(color::TEXT));
        ui.add_space(space::XXS);
        ui.label(RichText::new(body).color(color::TEXT_FAINT));
    });
    if !hints.is_empty() {
        ui.add_space(space::L);
        for (keys, what) in hints {
            ui.horizontal(|ui| {
                let left = ((ui.available_width() - 220.0) / 2.0).max(0.0);
                ui.add_space(left);
                ui.allocate_ui_with_layout(vec2(80.0, 18.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Right to left: the last key first.
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for k in keys.split('+').rev() {
                        kbd(ui, k);
                    }
                });
                ui.add_space(space::M);
                ui.label(RichText::new(*what).color(color::TEXT_MUTED));
            });
        }
    }
}

/// Text on a rounded fill, anchored at `pos`. Returns where it ended up.
pub fn chip(painter: &egui::Painter, pos: egui::Pos2, anchor: Align2, text: &str, fill: Color32, fg: Color32) -> Rect {
    let galley = painter.layout(text.to_string(), theme::small(), fg, (painter.clip_rect().width() - 40.0).max(80.0));
    let pad = vec2(8.0, 4.0);
    let rect = anchor.anchor_size(pos, galley.size() + pad * 2.0);
    painter.rect_filled(rect, radius::SMALL, fill);
    painter.galley(rect.min + pad, galley, fg);
    rect
}

/// A frame for popovers and menus.
pub fn popover_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(color::RAISED)
        .stroke(Stroke::new(1.0, color::CONTROL_HOVER))
        .corner_radius(CornerRadius::same(radius::NORMAL))
        .inner_margin(egui::Margin::same(space::S as i8))
        .shadow(egui::Shadow { offset: [0, 6], blur: 12, spread: 0, color: Color32::from_black_alpha(128) })
}

/// A segmented choice: one of `options`, the chosen one lit.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    let font = theme::mono();
    // Equal parts across the width.
    let total = ui.available_width();
    let widths: Vec<f32> = vec![(total - 4.0) / options.len().max(1) as f32; options.len()];
    let (rect, _) = ui.allocate_exact_size(vec2(total, 24.0), Sense::hover());
    ui.painter().rect_filled(rect, radius::SMALL, color::CONTROL);
    let mut x = rect.left() + 2.0;
    for ((v, label), w) in options.iter().zip(widths) {
        let r = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(w, rect.height() - 4.0));
        let resp = ui.interact(r, ui.scope_id().with(("seg", label)), Sense::click());
        let on = *value == *v;
        if on {
            ui.painter().rect_filled(r, 3, color::CONTROL_ACTIVE);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 3, color::CONTROL_HOVER);
        }
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, font.clone(), if on { color::TEXT } else { color::TEXT_MUTED });
        if resp.clicked() && !on {
            *value = *v;
            changed = true;
        }
        x += w;
    }
    changed
}

/// A switch: a pill with a knob, accent when on.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(26.0, 15.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let enabled = ui.is_enabled();
    let alpha = if enabled { 1.0 } else { 0.42 };
    let fill = color::CONTROL_ACTIVE.lerp_to_gamma(color::ACCENT, t).gamma_multiply(alpha);
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    let x = egui::lerp(rect.left() + 7.5..=rect.right() - 7.5, t);
    ui.painter().circle_filled(pos2(x, rect.center().y), 5.5, Color32::WHITE.gamma_multiply(if enabled { 0.95 } else { 0.5 }));
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// The icon of a layer kind (`image a`, `cube`, `solid`, `adjustment`, `camera`).
pub fn kind_icon(kind: &str) -> Icon {
    match kind.split(' ').next().unwrap_or("") {
        "image" => Icon::Image,
        "cube" => Icon::Box,
        "solid" => Icon::Square,
        "adjustment" => Icon::SlidersHorizontal,
        _ => Icon::Video,
    }
}
