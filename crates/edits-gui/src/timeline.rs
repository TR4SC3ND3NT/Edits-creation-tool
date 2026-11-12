//! The timeline panel: a ruler, the project's markers, the waveform, one row
//! per layer with its span, effects and keyframes, and the playhead.
//! Everything is painted by hand; the arithmetic is in `timemap` and `rows`.

use crate::app::{pal, App, Selected};
use crate::audio::peak_range;
use crate::rows::{group_marks, Row};
use crate::timemap::{ruler_ticks, TickLevel, View};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder};

const GUTTER: f32 = 190.0;
const RULER_H: f32 = 22.0;
const MARKER_H: f32 = 16.0;
const WAVE_H: f32 = 38.0;
const ROW_H: f32 = 30.0;
const SCROLL_H: f32 = 10.0;

fn kind_color(kind: &str) -> Color32 {
    match kind.split(' ').next().unwrap_or("") {
        "image" => Color32::from_rgb(86, 150, 220),
        "cube" => Color32::from_rgb(178, 120, 225),
        "solid" => Color32::from_rgb(135, 138, 150),
        "adjustment" => Color32::from_rgb(225, 150, 70),
        _ => Color32::from_rgb(90, 185, 130),
    }
}

fn diamond(painter: &Painter, c: Pos2, r: f32, fill: Color32) {
    let points = vec![pos2(c.x, c.y - r), pos2(c.x + r, c.y), pos2(c.x, c.y + r), pos2(c.x - r, c.y)];
    painter.add(Shape::convex_polygon(points, fill, Stroke::new(1.0, Color32::from_black_alpha(200))));
}

fn dot(painter: &Painter, c: Pos2, r: f32, fill: Color32) {
    painter.circle(c, r, fill, Stroke::new(1.0, Color32::from_black_alpha(200)));
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
    let lanes = RULER_H + if project.markers.is_empty() { 0.0 } else { MARKER_H } + if project.audio.is_some() { WAVE_H } else { 0.0 };
    34.0 + lanes + rows as f32 * ROW_H + SCROLL_H + 12.0
}

pub(crate) fn show(app: &mut App, ui: &mut Ui) {
    let (Some(tm), Some(project)) = (app.tm, app.project().cloned()) else {
        ui.add_space(6.0);
        ui.weak("The timeline appears when the project file is valid.");
        return;
    };
    let unit = tm.unit();

    // ---- header: what the project is, the legend, zoom ----
    let mut zoom_by: Option<f64> = None;
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.strong("Timeline");
        let tempo = tm.bpm.map(|b| format!(" · {b} BPM")).unwrap_or_default();
        ui.weak(format!("{} layers · {} {unit} · {} frames at {} fps{tempo}", project.layers.len(), fmt_time(tm.duration), tm.frames, tm.fps));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Fit").on_hover_text("Show the whole project").clicked() {
                app.view_fit = true;
            }
            if ui.button("+").on_hover_text("Zoom in (Ctrl + wheel)").clicked() {
                zoom_by = Some(1.6);
            }
            if ui.button("−").on_hover_text("Zoom out (Ctrl + wheel)").clicked() {
                zoom_by = Some(1.0 / 1.6);
            }
            ui.weak("zoom");
            ui.separator();
            ui.weak("effect key");
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 12.0), Sense::hover());
            dot(ui.painter(), r.center(), 3.4, pal::KEY_EFFECT);
            ui.weak("transform key");
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 12.0), Sense::hover());
            diamond(ui.painter(), r.center(), 4.2, pal::KEY_TRANSFORM);
        });
    });
    ui.add_space(2.0);

    let area = ui.available_rect_before_wrap();
    // Claim the whole panel, so it keeps the height it was given or dragged to.
    ui.allocate_rect(area, Sense::hover());
    if area.height() < RULER_H + ROW_H || area.width() < GUTTER + 60.0 {
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
    let ruler = lane(RULER_H);
    let marker_lane = (!project.markers.is_empty()).then(|| lane(MARKER_H));
    let wave_lane = project.audio.as_ref().map(|_| lane(WAVE_H));
    let rows_rect = Rect::from_min_max(pos2(area.left(), y), pos2(area.right(), (area.bottom() - SCROLL_H - 3.0).max(y)));
    let scroll_rect = Rect::from_min_max(pos2(time_left, area.bottom() - SCROLL_H), area.right_bottom());
    let time_clip = Rect::from_min_max(pos2(time_left, area.top()), area.right_bottom());

    let weak = ui.visuals().weak_text_color();
    let text = ui.visuals().text_color();
    let gutter = ui.painter_at(Rect::from_min_max(area.left_top(), pos2(time_left - 1.0, area.bottom())));
    let tp = ui.painter_at(time_clip);
    let small = FontId::proportional(10.5);

    // ---- ruler and grid ----
    tp.rect_filled(ruler.intersect(time_clip), 0.0, Color32::from_gray(38));
    gutter.text(pos2(area.left() + 8.0, ruler.center().y), Align2::LEFT_CENTER, if tm.uses_beats() { "beats (bar = 4)" } else { "seconds" }, small.clone(), weak);
    // Outside the project: before the first frame and after the last.
    let shade = Color32::from_black_alpha(90);
    let (px0, px1) = (x_of(tm.start_time()), x_of(tm.end_time()));
    if px0 > time_left {
        tp.rect_filled(Rect::from_min_max(pos2(time_left, ruler.bottom()), pos2(px0, rows_rect.bottom())), 0.0, shade);
    }
    if px1 < area.right() {
        tp.rect_filled(Rect::from_min_max(pos2(px1, ruler.bottom()), pos2(area.right(), rows_rect.bottom())), 0.0, shade);
    }
    for tick in ruler_ticks(view.start, view.end(width), view.px_per_unit, tm.uses_beats()) {
        let x = x_of(tick.t);
        let (top, ruler_color, grid_alpha) = match tick.level {
            TickLevel::Bar => (ruler.top() + 1.0, Color32::from_gray(190), 46),
            TickLevel::Beat => (ruler.bottom() - 8.0, Color32::from_gray(130), 20),
            TickLevel::Sub => (ruler.bottom() - 4.0, Color32::from_gray(95), 0),
        };
        tp.line_segment([pos2(x, top), pos2(x, ruler.bottom())], Stroke::new(1.0, ruler_color));
        if grid_alpha > 0 {
            tp.line_segment([pos2(x, ruler.bottom()), pos2(x, rows_rect.bottom())], Stroke::new(1.0, Color32::from_white_alpha(grid_alpha)));
        }
        if let Some(label) = &tick.label {
            let color = if tick.level == TickLevel::Bar { text } else { weak };
            tp.text(pos2(x + 3.0, ruler.top() + 1.0), Align2::LEFT_TOP, label, small.clone(), color);
        }
    }

    // ---- markers ----
    if let Some(lane) = marker_lane {
        tp.rect_filled(lane.intersect(time_clip), 0.0, Color32::from_gray(30));
        gutter.text(pos2(area.left() + 8.0, lane.center().y), Align2::LEFT_CENTER, "markers", small.clone(), weak);
        for m in &project.markers {
            let x = x_of(m.t);
            tp.line_segment([pos2(x, lane.top()), pos2(x, lane.bottom())], Stroke::new(2.0, pal::MARKER));
            tp.line_segment([pos2(x, lane.bottom()), pos2(x, rows_rect.bottom())], Stroke::new(1.0, pal::MARKER.gamma_multiply(0.35)));
            tp.text(pos2(x + 4.0, lane.center().y), Align2::LEFT_CENTER, &m.label, small.clone(), pal::MARKER);
        }
    }

    // ---- waveform ----
    if let (Some(lane), Some(audio)) = (wave_lane, &project.audio) {
        tp.rect_filled(lane.intersect(time_clip), 0.0, Color32::from_gray(24));
        gutter.text(pos2(area.left() + 8.0, lane.center().y - 6.0), Align2::LEFT_CENTER, "audio", small.clone(), weak);
        gutter.text(pos2(area.left() + 8.0, lane.center().y + 7.0), Align2::LEFT_CENTER, &audio.path, small.clone(), weak);
        let mid = lane.center().y;
        let half = lane.height() / 2.0 - 2.0;
        match app.audio.clip() {
            Some(clip) => {
                tp.line_segment([pos2(time_left, mid), pos2(area.right(), mid)], Stroke::new(1.0, pal::WAVE.gamma_multiply(0.4)));
                let columns = width.ceil() as usize;
                for c in 0..columns {
                    // Project time → seconds of the project → seconds of the audio file.
                    let a = tm.to_seconds(view.t_at(c as f64)) + audio.offset;
                    let b = tm.to_seconds(view.t_at(c as f64 + 1.0)) + audio.offset;
                    if let Some((lo, hi)) = peak_range(&clip.peaks, a, b) {
                        let x = time_left + c as f32 + 0.5;
                        let (lo, hi) = (lo.clamp(-1.0, 1.0), hi.clamp(-1.0, 1.0));
                        tp.line_segment([pos2(x, mid - hi * half - 0.5), pos2(x, mid - lo * half + 0.5)], Stroke::new(1.0, pal::WAVE));
                    }
                }
            }
            None => {
                let note = match app.audio.status() {
                    crate::audio::AudioStatus::Decoding => "decoding the audio…".to_string(),
                    crate::audio::AudioStatus::Silent(why) => format!("no waveform: {}", crate::app::shorten(&why, 110)),
                    _ => String::new(),
                };
                tp.text(pos2(time_left + 8.0, mid), Align2::LEFT_CENTER, note, small.clone(), weak);
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
    let mut picked: Option<Selected> = None;
    let selected = app.selected.clone();
    let tolerance = 3.0 / view.px_per_unit;
    ui.scope_builder(UiBuilder::new().max_rect(rows_rect), |ui| {
        egui::ScrollArea::vertical().id_salt("timeline-rows").auto_shrink([false, false]).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), rows.len() as f32 * ROW_H), Sense::hover());
            let clip = ui.clip_rect();
            let gp = ui.painter().with_clip_rect(clip.intersect(Rect::from_min_max(pos2(area.left(), clip.top()), pos2(time_left - 1.0, clip.bottom()))));
            let tp = ui.painter().with_clip_rect(clip.intersect(time_clip));
            for (i, row) in rows.iter().enumerate() {
                let r = Rect::from_min_size(pos2(rect.left(), rect.top() + i as f32 * ROW_H), vec2(rect.width(), ROW_H));
                if !clip.intersects(r) {
                    continue;
                }
                let me = selection_of(row);
                let resp = ui.interact(r, ui.scope_id().with(("row", i)), Sense::click());
                if selected.as_ref() == Some(&me) {
                    ui.painter().rect_filled(r, 0.0, ui.visuals().selection.bg_fill.gamma_multiply(0.45));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, Color32::from_white_alpha(10));
                }
                ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, Color32::from_white_alpha(12)));

                // Gutter: id and kind.
                let name_color = if row.enabled { text } else { weak };
                let name = gp.text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, &row.id, FontId::proportional(13.0), name_color);
                let kind = match (row.layer, row.enabled) {
                    (None, _) => "position, rotation, fov".to_string(),
                    (Some(_), true) => row.kind.clone(),
                    (Some(_), false) => format!("{} (disabled)", row.kind),
                };
                gp.text(pos2(name.right() + 8.0, r.center().y + 0.5), Align2::LEFT_CENTER, kind, small.clone(), weak);

                // The layer's span.
                let (x0, x1) = (x_of(row.start), x_of(row.end));
                let bar = Rect::from_min_max(pos2(x0, r.top() + 3.0), pos2(x1.max(x0 + 2.0), r.bottom() - 3.0));
                let base = kind_color(&row.kind);
                let fill = base.gamma_multiply(if row.enabled { 0.5 } else { 0.18 });
                tp.rect(bar, 3.0, fill, Stroke::new(1.0, base.gamma_multiply(if row.enabled { 1.0 } else { 0.5 })), StrokeKind::Inside);

                // Effect names (and the blend mode), kept in sight when the bar starts off-screen.
                let mut parts: Vec<String> = Vec::new();
                if !row.effects.is_empty() {
                    parts.push(format!("fx  {}", row.effects.join(" · ")));
                }
                if let Some(b) = &row.blend {
                    parts.push(format!("blend {b}"));
                }
                if !parts.is_empty() {
                    let inside = tp.with_clip_rect(tp.clip_rect().intersect(bar.shrink(2.0)));
                    inside.text(pos2(x0.max(time_left) + 5.0, bar.top() + 1.5), Align2::LEFT_TOP, parts.join("    "), small.clone(), Color32::from_gray(240));
                }

                // Keyframes: a diamond for the transform, a dot for effect parameters.
                let groups = group_marks(&row.marks, tolerance);
                let key_y = bar.bottom() - 6.5;
                for g in &groups {
                    let x = x_of(g.t);
                    if x < time_left - 6.0 || x > area.right() + 6.0 {
                        continue;
                    }
                    if g.transform {
                        diamond(&tp, pos2(x, key_y), 4.2, pal::KEY_TRANSFORM);
                    }
                    if g.effect {
                        dot(&tp, pos2(x, key_y), if g.transform { 2.0 } else { 3.4 }, pal::KEY_EFFECT);
                    }
                }

                if resp.clicked() {
                    picked = Some(me);
                }
                // Hovering a keyframe says which properties it belongs to.
                if let Some(p) = resp.hover_pos() {
                    let near = groups.iter().filter(|g| (x_of(g.t) - p.x).abs() <= 6.0).min_by(|a, b| (x_of(a.t) - p.x).abs().total_cmp(&(x_of(b.t) - p.x).abs()));
                    if let (Some(g), true) = (near, p.x >= time_left) {
                        resp.on_hover_text_at_pointer(format!("{} {}  (frame {})\n{}", if tm.uses_beats() { "beat" } else { "second" }, fmt_time(g.t), tm.frame_at(g.t), g.what.join("\n")));
                    }
                }
            }
        });
    });
    if let Some(p) = picked {
        // A second click on the selected row lets go of it.
        app.selected = if app.selected.as_ref() == Some(&p) { None } else { Some(p) };
    }

    // ---- playhead ----
    let x = x_of(tm.frame_time(app.playhead));
    tp.line_segment([pos2(x, ruler.top()), pos2(x, rows_rect.bottom())], Stroke::new(1.5, pal::PLAYHEAD));
    tp.add(Shape::convex_polygon(vec![pos2(x - 5.5, ruler.top()), pos2(x + 5.5, ruler.top()), pos2(x, ruler.top() + 8.0)], pal::PLAYHEAD, Stroke::NONE));

    // ---- horizontal scroll bar ----
    let pad = View::PAD / view.px_per_unit;
    let (lo, hi) = (t0 - pad, t1 + pad);
    let total = (hi - lo).max(1e-9);
    let painter = ui.painter_at(area);
    painter.rect_filled(scroll_rect, 3.0, Color32::from_gray(30));
    if !app.view_fit {
        let frac0 = ((view.start - lo) / total).clamp(0.0, 1.0) as f32;
        let frac1 = ((view.end(width) - lo) / total).clamp(0.0, 1.0) as f32;
        let thumb = Rect::from_min_max(
            pos2(scroll_rect.left() + frac0 * scroll_rect.width(), scroll_rect.top()),
            pos2(scroll_rect.left() + (frac1 * scroll_rect.width()).max(frac0 * scroll_rect.width() + 12.0), scroll_rect.bottom()),
        );
        let resp = ui.interact(scroll_rect, ui.scope_id().with("hscroll"), Sense::click_and_drag());
        if resp.dragged() {
            view.start += resp.drag_delta().x as f64 / scroll_rect.width() as f64 * total;
            view.clamp(t0, t1, width);
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let centre = lo + ((p.x - scroll_rect.left()) / scroll_rect.width()) as f64 * total;
                view.start = centre - width / view.px_per_unit / 2.0;
                view.clamp(t0, t1, width);
            }
        }
        let color = if resp.hovered() || resp.dragged() { Color32::from_gray(150) } else { Color32::from_gray(105) };
        painter.rect_filled(thumb, 3.0, color);
    }
    app.view = Some(view);
}
