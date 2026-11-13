//! The timeline panel: a toolbar, an overview strip of the whole edit, a
//! ruler, the project's markers, the waveform, one track per layer with its
//! span, effects and keyframes, and the playhead. Everything is painted by
//! hand with the tokens of `theme`; the arithmetic is in `timemap` and `rows`.
//!
//! Editing: drag a bar to move the layer (with its keys), drag its left or
//! right edge to change `in` or `out`, drag a keyframe marker to move the
//! keys at that time, double-click a row to key the picked property there.
//! Times snap to quarter beats (Alt: frames). A drag is drawn as a ghost and
//! written as one edit when the button is released (`editing::TimeDrag`).

use crate::app::{App, Cmd, Selected};
use crate::audio::peak_range;
use crate::editing::{self, DragKind, DragResult, Snap, TimeDrag};
use crate::rows::{group_marks, KeyShape, Row};
use crate::theme::{self, color, kind_colors, radius, size, space, text};
use crate::timemap::{ruler_ticks, TickLevel, View};
use crate::widgets::{self, tip, Icon};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, Layout, Painter, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder};

const GUTTER: f32 = size::TRACK_HEADER;
const RULER_H: f32 = size::RULER;
const MARKER_H: f32 = size::MARKERS;
const WAVE_H: f32 = size::WAVE;
const ROW_H: f32 = 30.0;
const OVERVIEW_H: f32 = size::OVERVIEW;
const TOOLBAR_H: f32 = size::TIMELINE_TOOLBAR;
/// Key glyphs: half their size.
const KEY_R: f32 = 4.0;

/// A key glyph: its shape says the ease of the segment it starts.
fn key_glyph(painter: &Painter, c: Pos2, r: f32, shape: KeyShape, fill: Color32) {
    let outline = Stroke::new(1.0, Color32::from_black_alpha(160));
    match shape {
        KeyShape::Linear => {
            let r = r + 0.6;
            painter.add(Shape::convex_polygon(vec![pos2(c.x, c.y - r), pos2(c.x + r, c.y), pos2(c.x, c.y + r), pos2(c.x - r, c.y)], fill, outline));
        }
        KeyShape::Hold => {
            painter.rect(Rect::from_center_size(c, vec2(2.0 * r - 1.0, 2.0 * r - 1.0)), 1, fill, outline, StrokeKind::Outside);
        }
        KeyShape::Eased => {
            painter.circle(c, r - 0.3, fill, outline);
        }
    }
}

fn selection_of(row: &Row) -> Selected {
    match row.layer {
        Some(_) => Selected::Layer(row.id.clone()),
        None => Selected::Camera,
    }
}

fn fmt_time(t: f64) -> String {
    let s = format!("{t:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The panel height at which every row of `project` is visible without scrolling.
pub(crate) fn natural_height(project: &edits_core::Project, rows: usize) -> f32 {
    let lanes = TOOLBAR_H + OVERVIEW_H + 6.0 + RULER_H + if project.markers.is_empty() { 0.0 } else { MARKER_H } + if project.audio.is_some() { WAVE_H } else { 0.0 };
    lanes + rows as f32 * ROW_H + 10.0
}

/// The toolbar: add a layer, what drags snap to, the size of the edit, zoom.
fn toolbar(app: &mut App, ui: &mut Ui, rect: Rect, tm: &crate::timemap::TimeMap, layers: usize) -> Option<f64> {
    let mut zoom_by = None;
    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(space::S, 0.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let add = tip(widgets::button(ui, Some(Icon::Plus), "Layer", false), "Add a layer at the playhead, above the selected one", &[]);
        egui::Popup::menu(&add).frame(widgets::popover_frame()).show(|ui| crate::panels::add_layer_entries(app, ui));
        ui.add_space(space::M);
        let alt = ui.input(|i| i.modifiers.alt);
        let snap = Snap::of(tm, alt);
        let (r, resp) = ui.allocate_exact_size(vec2(16.0, size::ICON_BUTTON), Sense::hover());
        widgets::paint_icon(ui.painter(), r.center(), Icon::Magnet, text::ICON_SMALL, color::TEXT_FAINT);
        let label = match snap {
            Snap::QuarterBeat => "¼ beat",
            Snap::Frame => "frames",
        };
        let l = ui.label(RichText::new(label).font(theme::small()).color(color::TEXT_MUTED));
        let help = format!(
            "Drags snap to {}.\n\nDrag a bar to move a layer (its keys move with it), its left or right edge to change in or out, a key to move the keys at that time. \
             Double-click a row to add a key of the property picked in the inspector. One edit per drag, written when you let go; Ctrl+Z undoes it.\n\n\
             Keys: ◆ linear, ■ hold, ● eased.",
            snap.label()
        );
        resp.union(l).on_hover_text(help);
        ui.add_space(space::L);
        let tempo = tm.bpm.map(|b| format!(" · {b} BPM")).unwrap_or_default();
        ui.label(RichText::new(format!("{layers} layer{} · {} {}{tempo} · {} fps", if layers == 1 { "" } else { "s" }, fmt_time(tm.duration), tm.unit(), tm.fps)).font(theme::small()).color(color::TEXT_FAINT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if tip(widgets::icon_toggle(ui, Icon::Maximize2, app.view_fit), "Fit the whole edit", &[]).clicked() {
                app.run(Cmd::ZoomFit);
            }
            if tip(widgets::icon_button(ui, Icon::ZoomIn), "Zoom in", &["Ctrl", "Wheel"]).clicked() {
                zoom_by = Some(1.6);
            }
            if tip(widgets::icon_button(ui, Icon::ZoomOut), "Zoom out", &["Ctrl", "Wheel"]).clicked() {
                zoom_by = Some(1.0 / 1.6);
            }
        });
    });
    zoom_by
}

pub(crate) fn show(app: &mut App, ui: &mut Ui) {
    let full = ui.max_rect();
    // Claim the whole panel, so it keeps the height it was given or dragged to.
    ui.allocate_rect(full, Sense::hover());
    let (Some(tm), Some(project)) = (app.tm, app.project().cloned()) else {
        ui.scope_builder(UiBuilder::new().max_rect(full), |ui| {
            widgets::empty_state(ui, Icon::Film, "No timeline yet", "It appears when the project file is valid.", &[]);
        });
        return;
    };

    // ---- toolbar ----
    let bar = Rect::from_min_size(full.min, vec2(full.width(), TOOLBAR_H));
    let mut zoom_by = toolbar(app, ui, bar, &tm, project.layers.len());
    if let Some(z) = app.zoom_request.take() {
        zoom_by = Some(z);
    }
    let painter = ui.painter_at(full);
    painter.hline(full.x_range(), bar.bottom(), Stroke::new(1.0, color::LINE));

    let area = Rect::from_min_max(pos2(full.left(), bar.bottom() + 1.0), full.right_bottom());
    if area.height() < OVERVIEW_H + RULER_H + ROW_H || area.width() < GUTTER + 60.0 {
        return;
    }
    let time_left = area.left() + GUTTER;
    let width = (area.right() - time_left) as f64;
    let (t0, t1) = (tm.start_time().min(0.0), tm.end_time().max(tm.duration));
    let fitted = View::fit(t0, t1, width);

    // ---- the view: zoom, scroll, follow ----
    let mut view = if app.view_fit { fitted } else { app.view.unwrap_or(fitted) };
    let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p));
    if let Some(p) = pointer {
        let (zoom, scroll_x) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta.x));
        if (zoom - 1.0).abs() > 1e-4 {
            view.zoom_about(zoom as f64, (p.x - time_left).max(0.0) as f64);
            app.view_fit = false;
        }
        if scroll_x != 0.0 && !app.view_fit {
            view.start -= scroll_x as f64 / view.px_per_unit;
        }
    }
    if let Some(z) = zoom_by {
        let x = view.x_of(tm.frame_time(app.playhead)).clamp(0.0, width);
        view.zoom_about(z, x);
        app.view_fit = false;
    }
    if app.playing.is_some() && !app.view_fit {
        view.follow(tm.frame_time(app.playhead), width);
    }
    view.clamp(t0, t1, width);
    if view == fitted {
        app.view_fit = true;
    }
    let x_of = |t: f64| time_left + view.x_of(t) as f32;

    // ---- lanes ----
    let mut y = area.top();
    let mut lane = |h: f32| {
        let r = Rect::from_min_max(pos2(area.left(), y), pos2(area.right(), y + h));
        y += h;
        r
    };
    let overview_lane = lane(OVERVIEW_H + 6.0);
    let ruler = lane(RULER_H);
    let marker_lane = (!project.markers.is_empty()).then(|| lane(MARKER_H));
    let wave_lane = project.audio.as_ref().map(|_| lane(WAVE_H));
    let rows_rect = Rect::from_min_max(pos2(area.left(), y), pos2(area.right(), area.bottom().max(y)));
    let time_clip = Rect::from_min_max(pos2(time_left, area.top()), area.right_bottom());

    let gutter_rect = Rect::from_min_max(area.left_top(), pos2(time_left, area.bottom()));
    let gutter = ui.painter_at(gutter_rect);
    let tp = ui.painter_at(time_clip);
    let small = theme::small();
    let tiny = theme::mono_small();

    // The track headers sit on a raised column.
    gutter.rect_filled(gutter_rect, 0, color::RAISED);
    painter.vline(time_left, area.y_range(), Stroke::new(1.0, color::LINE));

    // ---- overview: the whole edit, the visible part, the playhead ----
    let mut recentre: Option<f64> = None;
    let ov = Rect::from_min_max(pos2(time_left + 6.0, overview_lane.top() + 4.0), pos2(area.right() - 6.0, overview_lane.bottom() - 2.0));
    {
        let ov_x = |t: f64| ov.left() + ((t - t0) / (t1 - t0).max(1e-9)) as f32 * ov.width();
        let p = ui.painter_at(overview_lane);
        p.rect_filled(ov, radius::SMALL, color::CONTROL);
        let layers: Vec<&Row> = app.rows.iter().collect();
        let n = layers.len().max(1) as f32;
        let lh = ((ov.height() - 4.0) / n).clamp(1.0, 3.0);
        let top = ov.center().y - lh * n / 2.0;
        for (i, row) in layers.iter().enumerate() {
            let k = kind_colors(&row.kind);
            let r = Rect::from_min_max(pos2(ov_x(row.start), top + i as f32 * lh), pos2(ov_x(row.end).max(ov_x(row.start) + 1.0), top + (i as f32 + 1.0) * lh - 0.5));
            p.rect_filled(r, 0, k.stripe.gamma_multiply(if row.enabled { 0.7 } else { 0.25 }));
        }
        for m in &project.markers {
            p.vline(ov_x(m.t), ov.y_range(), Stroke::new(1.0, color::MARKER.gamma_multiply(0.7)));
        }
        if !app.view_fit {
            let vr = Rect::from_min_max(pos2(ov_x(view.start).max(ov.left()), ov.top()), pos2(ov_x(view.end(width)).min(ov.right()), ov.bottom()));
            p.rect(vr, radius::SMALL, color::ACCENT.gamma_multiply(0.12), Stroke::new(1.0, color::ACCENT_LINE), StrokeKind::Inside);
        }
        let px = ov_x(tm.frame_time(app.playhead));
        p.vline(px, ov.top() - 2.0..=ov.bottom() + 1.0, Stroke::new(1.5, color::PLAYHEAD));
        gutter.text(pos2(area.left() + space::L, ov.center().y), Align2::LEFT_CENTER, "Overview", small.clone(), color::TEXT_FAINT);
        // Click or drag to move the playhead; drag the box (when zoomed) to scroll.
        let resp = ui.interact(ov.expand2(vec2(4.0, 2.0)), ui.scope_id().with("overview"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand);
        let resp = resp.on_hover_text("The whole edit. Click or drag to move the playhead.");
        if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.clicked() || resp.dragged()) {
            let t = t0 + ((p.x - ov.left()) / ov.width()).clamp(0.0, 1.0) as f64 * (t1 - t0);
            app.seek(tm.frame_at(t));
            if !app.view_fit && (t < view.start || t > view.end(width)) {
                recentre = Some(t - width / view.px_per_unit / 2.0);
            }
        }
    }

    // ---- ruler and grid ----
    tp.rect_filled(ruler.intersect(time_clip), 0, color::BG);
    painter.hline(area.x_range(), ruler.bottom(), Stroke::new(1.0, color::LINE));
    gutter.text(pos2(area.left() + space::L, ruler.center().y), Align2::LEFT_CENTER, if tm.uses_beats() { "Beats" } else { "Seconds" }, small.clone(), color::TEXT_FAINT);
    // Outside the project: before the first frame and after the last.
    let shade = Color32::from_black_alpha(80);
    let (px0, px1) = (x_of(tm.start_time()), x_of(tm.end_time()));
    if px0 > time_left {
        tp.rect_filled(Rect::from_min_max(pos2(time_left, ruler.bottom()), pos2(px0, rows_rect.bottom())), 0, shade);
    }
    if px1 < area.right() {
        tp.rect_filled(Rect::from_min_max(pos2(px1, ruler.bottom()), pos2(area.right(), rows_rect.bottom())), 0, shade);
    }
    for tick in ruler_ticks(view.start, view.end(width), view.px_per_unit, tm.uses_beats()) {
        let x = x_of(tick.t);
        let (top, ruler_color, grid_alpha) = match tick.level {
            TickLevel::Bar => (ruler.top() + 3.0, color::TEXT_FAINT, 22),
            TickLevel::Beat => (ruler.bottom() - 6.0, color::LINE_STRONG, 10),
            TickLevel::Sub => (ruler.bottom() - 3.0, color::CONTROL_HOVER, 0),
        };
        tp.vline(x, top..=ruler.bottom(), Stroke::new(1.0, ruler_color));
        if grid_alpha > 0 {
            tp.vline(x, ruler.bottom()..=rows_rect.bottom(), Stroke::new(1.0, Color32::from_white_alpha(grid_alpha)));
        }
        if let Some(label) = &tick.label {
            let c = if tick.level == TickLevel::Bar { color::TEXT_MUTED } else { color::TEXT_FAINT };
            tp.text(pos2(x + 4.0, ruler.top() + 2.0), Align2::LEFT_TOP, label, tiny.clone(), c);
        }
    }

    // ---- markers ----
    if let Some(lane) = marker_lane {
        gutter.text(pos2(area.left() + space::L, lane.center().y), Align2::LEFT_CENTER, "Markers", small.clone(), color::TEXT_FAINT);
        for m in &project.markers {
            let x = x_of(m.t);
            tp.vline(x, lane.top() + 2.0..=rows_rect.bottom(), Stroke::new(1.0, color::MARKER.gamma_multiply(0.25)));
            let flag = Rect::from_min_size(pos2(x, lane.top() + 2.0), vec2(6.0, 6.0));
            tp.rect_filled(flag, CornerRadius { nw: 0, ne: 2, sw: 0, se: 2 }, color::MARKER);
            tp.text(pos2(x + 9.0, lane.center().y), Align2::LEFT_CENTER, &m.label, small.clone(), color::MARKER);
        }
    }

    // ---- waveform ----
    if let (Some(lane), Some(audio)) = (wave_lane, &project.audio) {
        painter.hline(area.x_range(), lane.bottom(), Stroke::new(1.0, color::LINE));
        let name = audio.path.rsplit('/').next().unwrap_or(&audio.path).to_string();
        let (ir, _) = (Rect::from_center_size(pos2(area.left() + space::L + 7.0, lane.center().y), vec2(14.0, 14.0)), ());
        widgets::paint_icon(&gutter, ir.center(), Icon::AudioWaveform, text::ICON_SMALL, color::TEXT_FAINT);
        gutter.text(pos2(area.left() + space::L + 22.0, lane.center().y - 7.0), Align2::LEFT_CENTER, "Audio", theme::body(), color::TEXT_MUTED);
        gutter.text(pos2(area.left() + space::L + 22.0, lane.center().y + 8.0), Align2::LEFT_CENTER, crate::app::shorten(&name, 20), small.clone(), color::TEXT_FAINT);
        let mid = lane.center().y;
        let half = lane.height() / 2.0 - 4.0;
        let played_x = x_of(tm.frame_time(app.playhead));
        match app.audio.clip() {
            Some(clip) => {
                let columns = width.ceil() as usize;
                for c in 0..columns {
                    // Project time -> seconds of the project -> seconds of the audio file.
                    let a = tm.to_seconds(view.t_at(c as f64)) + audio.offset;
                    let b = tm.to_seconds(view.t_at(c as f64 + 1.0)) + audio.offset;
                    if let Some((lo, hi)) = peak_range(&clip.peaks, a, b) {
                        let x = time_left + c as f32 + 0.5;
                        let (lo, hi) = (lo.clamp(-1.0, 1.0), hi.clamp(-1.0, 1.0));
                        let ink = if x <= played_x { color::WAVE_PLAYED } else { color::WAVE };
                        tp.vline(x, (mid - hi * half - 0.5)..=(mid - lo * half + 0.5), Stroke::new(1.0, ink));
                    }
                }
            }
            None => {
                let note = match app.audio.status() {
                    crate::audio::AudioStatus::Decoding => "Decoding the audio…".to_string(),
                    crate::audio::AudioStatus::Silent(why) => format!("No waveform: {}", crate::app::shorten(&why, 110)),
                    _ => String::new(),
                };
                tp.text(pos2(time_left + space::M, mid), Align2::LEFT_CENTER, note, small.clone(), color::TEXT_FAINT);
            }
        }
    }

    // ---- click or drag on the ruler (and the lanes under it) to seek ----
    let seek_rect = Rect::from_min_max(pos2(time_left, ruler.top()), pos2(area.right(), rows_rect.top()));
    let seek = ui.interact(seek_rect, ui.scope_id().with("seek"), Sense::click_and_drag());
    if seek.clicked() || seek.dragged() || seek.drag_started() {
        if let Some(p) = seek.interact_pointer_pos() {
            app.seek(tm.frame_at(view.t_at((p.x - time_left) as f64)));
        }
    }

    // ---- layer rows ----
    let rows = app.rows.clone();
    // A click on a row's background toggles its selection; a click or drag on its bar selects it.
    let mut picked: Option<Selected> = None;
    let mut chosen: Option<Selected> = None;
    let mut toggled: Option<String> = None;
    let selected = app.selected.clone();
    let tolerance = 3.0 / view.px_per_unit;
    let alt = ui.input(|i| i.modifiers.alt);
    let pointer_t = |p: Pos2| view.t_at((p.x - time_left) as f64);
    let current = app.drag.as_ref().map(|(d, _)| d.clone());
    let mut started: Option<TimeDrag> = None;
    let mut moved_to: Option<f64> = None;
    let mut released = false;
    let mut keyed: Option<(String, f64)> = None;
    if rows.is_empty() {
        let r = Rect::from_min_max(rows_rect.min, pos2(rows_rect.right(), rows_rect.top() + 48.0));
        painter.text(pos2(time_left + space::L, r.center().y), Align2::LEFT_CENTER, "No layers yet. + Layer adds one at the playhead.", theme::body(), color::TEXT_FAINT);
    }
    ui.scope_builder(UiBuilder::new().max_rect(rows_rect), |ui| {
        egui::ScrollArea::vertical().id_salt("timeline-rows").auto_shrink([false, false]).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), rows.len() as f32 * ROW_H), Sense::hover());
            let clip = ui.clip_rect();
            let gp = ui.painter().with_clip_rect(clip.intersect(Rect::from_min_max(pos2(area.left(), clip.top()), pos2(time_left - 1.0, clip.bottom()))));
            let tp = ui.painter().with_clip_rect(clip.intersect(time_clip));
            for (i, row) in rows.iter().enumerate() {
                let r = Rect::from_min_size(pos2(rect.left(), rect.top() + i as f32 * ROW_H), vec2(rect.width(), ROW_H));
                let dragging_me = current.as_ref().is_some_and(|d| d.layer == row.id && row.layer.is_some());
                if !clip.intersects(r) && !dragging_me {
                    continue;
                }
                let me = selection_of(row);
                let is_selected = selected.as_ref() == Some(&me);
                let resp = ui.interact(r, ui.scope_id().with(("row", i)), Sense::click());
                let header = Rect::from_min_max(r.min, pos2(time_left, r.bottom()));
                if is_selected {
                    gp.rect_filled(header, 0, color::ACCENT_BG);
                    gp.rect_filled(Rect::from_min_size(header.min, vec2(2.0, header.height())), 0, color::ACCENT);
                    tp.rect_filled(r, 0, Color32::from_rgba_unmultiplied(0x6E, 0x6A, 0xDE, 8));
                } else if resp.hovered() {
                    gp.rect_filled(header, 0, color::CONTROL);
                    tp.rect_filled(r, 0, Color32::from_white_alpha(4));
                }
                ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, Color32::from_white_alpha(7)));

                // Header: kind icon, id, kind; the eye on hover (always when disabled).
                let k = kind_colors(&row.kind);
                let dim = if row.enabled { 1.0 } else { 0.45 };
                widgets::paint_icon(&gp, pos2(r.left() + space::L + 7.0, r.center().y), widgets::kind_icon(&row.kind), 13.0, k.stripe.lerp_to_gamma(Color32::WHITE, 0.3).gamma_multiply(dim));
                let name_color = if is_selected { color::TEXT } else if row.enabled { color::TEXT_MUTED } else { color::TEXT_FAINT };
                let name = gp.text(pos2(r.left() + space::L + 22.0, r.center().y), Align2::LEFT_CENTER, crate::app::shorten(&row.id, 16), theme::body(), name_color);
                let kind = match row.layer {
                    None => "camera".to_string(),
                    Some(_) => row.kind.split(' ').next().unwrap_or("").to_string(),
                };
                if name.right() + 70.0 < time_left {
                    gp.text(pos2(name.right() + space::S, r.center().y + 0.5), Align2::LEFT_CENTER, kind, small.clone(), color::TEXT_FAINT);
                }
                if row.layer.is_some() && (resp.hovered() || !row.enabled || ui.rect_contains_pointer(header)) {
                    let eye = Rect::from_center_size(pos2(time_left - 18.0, r.center().y), vec2(22.0, 22.0));
                    let er = ui.interact(eye, ui.scope_id().with(("eye", i)), Sense::click());
                    if er.hovered() {
                        gp.rect_filled(eye, radius::SMALL, color::CONTROL_HOVER);
                    }
                    widgets::paint_icon(&gp, eye.center(), if row.enabled { Icon::Eye } else { Icon::EyeOff }, 13.0, if er.hovered() { color::TEXT } else { color::TEXT_FAINT });
                    if tip(er.on_hover_cursor(CursorIcon::PointingHand), if row.enabled { "Disable this layer (not drawn)" } else { "Enable this layer" }, &[]).clicked() {
                        toggled = Some(row.id.clone());
                    }
                }

                // The layer's span: a muted fill with a stripe of its kind on top.
                let (x0, x1) = (x_of(row.start), x_of(row.end));
                let bar = Rect::from_min_max(pos2(x0, r.top() + 3.0), pos2(x1.max(x0 + 2.0), r.bottom() - 3.0));
                let ghosted = dragging_me && current.as_ref().is_some_and(|d| !matches!(d.kind, DragKind::Keys { .. }));
                let fade = if ghosted { 0.45 } else { 1.0 } * if row.enabled { 1.0 } else { 0.45 };
                let fill = if is_selected { color::ACCENT_BG.lerp_to_gamma(k.fill, 0.35) } else { k.fill };
                tp.rect_filled(bar, radius::SMALL, fill.gamma_multiply(fade));
                let stripe = Rect::from_min_max(bar.min, pos2(bar.right(), bar.top() + 2.0));
                tp.rect_filled(stripe, CornerRadius { nw: radius::SMALL, ne: radius::SMALL, sw: 0, se: 0 }, k.stripe.gamma_multiply(fade * if is_selected { 1.0 } else { 0.85 }));
                if is_selected {
                    tp.rect_stroke(bar, radius::SMALL, Stroke::new(1.5, color::ACCENT), StrokeKind::Inside);
                }

                // Effect names (and the blend mode), kept in sight when the bar starts off-screen.
                let mut parts: Vec<String> = Vec::new();
                if !row.effects.is_empty() {
                    parts.push(row.effects.join(" · "));
                }
                if let Some(b) = &row.blend {
                    parts.push(format!("blend {b}"));
                }
                if !parts.is_empty() && bar.width() > 40.0 {
                    let inside = tp.with_clip_rect(tp.clip_rect().intersect(bar.shrink2(vec2(4.0, 0.0))));
                    let fg = if row.enabled { color::TEXT_MUTED } else { color::TEXT_FAINT };
                    inside.text(pos2(x0.max(time_left) + 6.0, bar.top() + 4.0), Align2::LEFT_TOP, parts.join("   "), theme::mono_small(), fg);
                }

                // Keyframes: white glyphs; the shape tells the ease.
                let groups = group_marks(&row.marks, tolerance);
                let key_y = bar.bottom() - 5.5;
                for g in &groups {
                    let x = x_of(g.t);
                    if x < time_left - 6.0 || x > area.right() + 6.0 {
                        continue;
                    }
                    let here = (g.t - tm.frame_time(app.playhead)).abs() < 1e-6 * g.t.abs().max(1.0) || tm.frame_at(g.t) == app.playhead;
                    let ink = if is_selected && here { color::ACCENT_TEXT } else { color::TEXT.gamma_multiply(if row.enabled { 1.0 } else { 0.5 }) };
                    key_glyph(&tp, pos2(x, key_y), KEY_R, g.shape, ink);
                }

                if resp.clicked() {
                    picked = Some(me.clone());
                }
                if resp.double_clicked() && row.layer.is_some() {
                    if let Some(p) = resp.interact_pointer_pos().filter(|p| p.x >= time_left) {
                        keyed = Some((row.id.clone(), pointer_t(p)));
                    }
                }
                // Hovering a keyframe says which properties it belongs to.
                if let Some(p) = resp.hover_pos() {
                    let near = groups.iter().filter(|g| (x_of(g.t) - p.x).abs() <= 6.0).min_by(|a, b| (x_of(a.t) - p.x).abs().total_cmp(&(x_of(b.t) - p.x).abs()));
                    if let (Some(g), true) = (near, p.x >= time_left) {
                        resp.on_hover_text_at_pointer(format!("{} {}  (frame {})\n{}\ndrag to move these keys", if tm.uses_beats() { "beat" } else { "second" }, fmt_time(g.t), tm.frame_at(g.t), g.what.join("\n")));
                    }
                }

                // ---- editing: the bar, its edges and its key markers (the camera row only shows) ----
                if row.layer.is_none() {
                    continue;
                }
                let visible = bar.intersect(time_clip);
                let mut handles: Vec<(egui::Response, DragKind)> = Vec::new();
                if visible.width() > 0.0 {
                    let body = ui.interact(visible, ui.scope_id().with(("bar", i)), Sense::click_and_drag()).on_hover_cursor(CursorIcon::Grab);
                    if body.clicked() {
                        chosen = Some(me.clone());
                    }
                    if body.double_clicked() {
                        if let Some(p) = body.interact_pointer_pos() {
                            keyed = Some((row.id.clone(), pointer_t(p)));
                        }
                    }
                    handles.push((body, DragKind::Move));
                }
                for g in &groups {
                    let x = x_of(g.t);
                    let hit = Rect::from_center_size(pos2(x, key_y), vec2(11.0, 11.0));
                    if !time_clip.contains(hit.center()) {
                        continue;
                    }
                    let k = ui.interact(hit, ui.scope_id().with(("key", i, g.t.to_bits())), Sense::drag()).on_hover_cursor(CursorIcon::ResizeHorizontal);
                    let tip = format!("{} {}  (frame {})\n{}\ndrag to move these keys", if tm.uses_beats() { "beat" } else { "second" }, fmt_time(g.t), tm.frame_at(g.t), g.what.join("\n"));
                    let k = k.on_hover_text_at_pointer(tip);
                    handles.push((k, DragKind::Keys { from: g.t }));
                }
                // The edges sit on the upper part of the bar, so a key at the very start stays draggable.
                for (x, kind) in [(x0, DragKind::TrimIn), (x1, DragKind::TrimOut)] {
                    let edge = Rect::from_min_max(pos2(x - 4.0, bar.top()), pos2(x + 4.0, bar.center().y + 1.0));
                    if !time_clip.intersects(edge) {
                        continue;
                    }
                    let tip = if kind == DragKind::TrimIn { "drag: change in" } else { "drag: change out" };
                    let e = ui.interact(edge, ui.scope_id().with(("edge", i, kind == DragKind::TrimIn)), Sense::drag()).on_hover_cursor(CursorIcon::ResizeHorizontal).on_hover_text_at_pointer(tip);
                    if e.hovered() {
                        tp.vline(x + if kind == DragKind::TrimIn { 1.0 } else { -1.0 }, bar.top() + 2.0..=bar.bottom() - 2.0, Stroke::new(2.0, color::TEXT));
                    }
                    handles.push((e, kind));
                }
                for (h, kind) in handles {
                    if h.drag_started() {
                        let press = ui.input(|inp| inp.pointer.press_origin()).or(h.interact_pointer_pos());
                        if let Some(p) = press {
                            started = Some(TimeDrag::new(row.id.clone(), kind.clone(), pointer_t(p), alt));
                            chosen = Some(me.clone());
                        }
                    }
                    if h.dragged() {
                        if let Some(p) = h.interact_pointer_pos() {
                            moved_to = Some(pointer_t(p));
                        }
                    }
                    if h.drag_stopped() {
                        released = true;
                    }
                }

                // The ghost: where the drag puts things if released now.
                if let (true, Some(d)) = (dragging_me, &current) {
                    match d.result(&project, &tm) {
                        Some(DragResult::Span { start, end }) => {
                            let g = Rect::from_min_max(pos2(x_of(start), bar.top() - 1.0), pos2(x_of(end).max(x_of(start) + 2.0), bar.bottom() + 1.0));
                            tp.rect(g, radius::SMALL, Color32::from_white_alpha(22), Stroke::new(1.5, color::TEXT), StrokeKind::Inside);
                            // The times next to the edge that moves, outside the bar's own text.
                            let label = format!("{} – {}", fmt_time(start), fmt_time(end));
                            let (at, align) = if d.kind == DragKind::TrimIn { (pos2(g.left() + 4.0, g.bottom() - 2.0), Align2::LEFT_BOTTOM) } else { (pos2(g.right() - 4.0, g.bottom() - 2.0), Align2::RIGHT_BOTTOM) };
                            let r = tp.text(at, align, &label, tiny.clone(), color::TEXT);
                            tp.rect_filled(r.expand(2.0), 2, Color32::from_black_alpha(200));
                            tp.text(at, align, label, tiny.clone(), color::TEXT);
                        }
                        Some(DragResult::Key { to }) => {
                            if let DragKind::Keys { from } = d.kind {
                                let (a, b) = (x_of(from), x_of(to));
                                tp.hline(a.min(b)..=a.max(b), key_y, Stroke::new(1.0, color::TEXT));
                                key_glyph(&tp, pos2(b, key_y), KEY_R + 1.5, KeyShape::Linear, color::ACCENT_TEXT);
                                let r = tp.text(pos2(b + 8.0, bar.top() + 2.0), Align2::LEFT_TOP, fmt_time(to), tiny.clone(), color::TEXT);
                                tp.rect_filled(r.expand(2.0), 2, Color32::from_black_alpha(200));
                                tp.text(pos2(b + 8.0, bar.top() + 2.0), Align2::LEFT_TOP, fmt_time(to), tiny.clone(), color::TEXT);
                            }
                        }
                        None => {}
                    }
                }
            }
        });
    });
    if let Some(id) = toggled {
        app.toggle_layer(&id);
    } else if let Some(p) = chosen {
        app.selected = Some(p);
    } else if let Some(p) = picked {
        // A second click on the selected row lets go of it.
        app.selected = if app.selected.as_ref() == Some(&p) { None } else { Some(p) };
    }
    if let Some(d) = started {
        app.drag = Some((d, app.state.text_hash));
    }
    if let (Some(t), Some((d, _))) = (moved_to, app.drag.as_mut()) {
        d.update(t, alt);
    }
    // Released (or the button is up and the widget was not drawn to say so): one edit.
    let button_up = !ui.input(|i| i.pointer.any_down());
    if released || (button_up && app.drag.is_some()) {
        if let Some((mut d, base)) = app.drag.take() {
            d.alt = alt;
            let (ops, label) = d.ops(&project, &tm);
            app.commit_against(ops, label, base);
        }
    }
    if let Some((id, t)) = keyed {
        app.selected = Some(Selected::Layer(id.clone()));
        app.key_at(&id, editing::snap(&tm, t, alt));
    }

    // ---- playhead: a light line with an accent cap on the ruler ----
    let x = x_of(tm.frame_time(app.playhead));
    if x >= time_left - 1.0 {
        tp.vline(x, ruler.top()..=rows_rect.bottom(), Stroke::new(1.0, color::PLAYHEAD));
        let cap = vec![pos2(x - 5.0, ruler.top()), pos2(x + 5.0, ruler.top()), pos2(x + 5.0, ruler.top() + 7.0), pos2(x, ruler.top() + 12.0), pos2(x - 5.0, ruler.top() + 7.0)];
        tp.add(Shape::convex_polygon(cap, color::ACCENT, Stroke::new(1.0, color::ACCENT_TEXT.gamma_multiply(0.6))));
    }
    if let Some(start) = recentre {
        view.start = start;
        view.clamp(t0, t1, width);
    }
    app.view = Some(view);
}
