//! Song tools on screen: the timeline toolbar's "Analyse song" button and
//! its popover (the marks to show, Cut on beats…, Accent on…, writing the
//! tempo into the project), the lane of the song's marks under the ruler,
//! and the dialogs of "Cut on beats…" and "Accent on…". The logic is in
//! `song`.

use crate::app::{App, Cmd};
use crate::presets::{self, TargetChoice};
use crate::song::{self, CutOn, Mark};
use crate::theme::{self, color, radius, space, text};
use crate::widgets::{self, tip, Icon};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Painter, Rect, RichText, Shape, Stroke, Ui};
use edits_core::beatops;

/// The toolbar's song control: "Analyse song", "Analysing…", or "Song" with its popover.
pub(crate) fn toolbar(app: &mut App, ui: &mut Ui) {
    let has_audio = app.project().is_some_and(|p| p.audio.is_some());
    if app.song.job.is_some() {
        ui.add(egui::Spinner::new().size(14.0).color(color::TEXT_MUTED));
        ui.label(RichText::new("Analysing the song…").font(theme::small()).color(color::TEXT_MUTED));
        return;
    }
    if app.song.marks.is_none() {
        let r = ui.add_enabled_ui(has_audio, |ui| widgets::button(ui, Some(Icon::AudioLines), "Analyse song", false)).inner;
        let why = if has_audio { "Find the beats, bars, kick and snare onsets, sections and the drop of the project's audio, and show them on the timeline" } else { "This project has no audio to analyse" };
        if tip(r, why, &[]).on_disabled_hover_text(why).clicked() {
            app.run(Cmd::AnalyseSong);
        }
        return;
    }
    let r = tip(widgets::button(ui, Some(Icon::AudioLines), "Song", false), "The song's marks on the timeline, and cutting and accenting to them", &[]);
    egui::Popup::menu(&r).frame(widgets::popover_frame()).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| popover(app, ui));
}

fn popover(app: &mut App, ui: &mut Ui) {
    ui.set_width(270.0);
    ui.spacing_mut().item_spacing.y = space::XS;
    if let Some(m) = &app.song.marks {
        ui.label(widgets::eyebrow("Song"));
        ui.label(RichText::new(song::summary(m)).font(theme::small()).color(color::TEXT_MUTED));
        for n in &m.notes {
            ui.label(RichText::new(n).font(theme::small()).color(color::TEXT_FAINT));
        }
    }
    widgets::divider(ui);
    ui.label(widgets::eyebrow("Show on the timeline"));
    for (l, c) in [(song::Layer::Downbeats, color::SONG_BEAT), (song::Layer::Kicks, color::SONG_KICK), (song::Layer::Snares, color::SONG_SNARE), (song::Layer::Sections, color::MARKER)] {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 18.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 3.5, c);
            let name = l.name();
            ui.label(RichText::new(format!("{}{}", name[..1].to_uppercase(), &name[1..])).color(color::TEXT_MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut on = app.song.toggles.get(l);
                if widgets::switch(ui, &mut on).changed() {
                    app.song.toggles.flip(l);
                }
            });
        });
    }
    widgets::divider(ui);
    if widgets::menu_item(ui, Some(Icon::Scissors), "Cut on beats…", None).on_hover_text("One picture per beat, bar or onset, cycling through slots (edits beats cut)").clicked() {
        app.run(Cmd::CutDialog);
        ui.close();
    }
    if widgets::menu_item(ui, Some(Icon::Zap), "Accent on…", None).on_hover_text("A preset on every beat, downbeat, kick, snare or the drop (edits beats accent)").clicked() {
        app.run(Cmd::AccentDialog);
        ui.close();
    }
    if widgets::menu_item(ui, Some(Icon::Gauge), "Write tempo and markers", None)
        .on_hover_text("Set the project's tempo and beat offset to the song's and add its markers (drop, sections), as one edit (edits audio --project)")
        .clicked()
    {
        app.run(Cmd::SyncSong);
        ui.close();
    }
    if widgets::menu_item(ui, Some(Icon::RefreshCw), "Analyse again", None).clicked() {
        app.run(Cmd::AnalyseSong);
        ui.close();
    }
}

/// Whether the timeline has a song lane.
pub(crate) fn has_lane(app: &App) -> bool {
    app.song.marks.is_some() && app.song.toggles.any()
}

/// The song lane under the ruler: section bands, downbeat lines, kicks
/// (dots under the middle) and snares (diamonds over it), the drop as a flag.
/// Returns a tooltip for the mark under `pointer`, if any.
pub(crate) fn lane(app: &App, painter: &Painter, lane: Rect, x_of: &dyn Fn(f64) -> f32, (t_from, t_to): (f64, f64), pointer: Option<egui::Pos2>, unit: &str) -> Option<String> {
    let m = app.song.marks.as_ref()?;
    let marks = song::visible(m, &app.song.toggles, t_from, t_to);
    let mid = lane.center().y;
    let mut near: Option<(f32, String)> = None;
    let mut consider = |x: f32, what: String| {
        if let Some(p) = pointer.filter(|p| lane.contains(*p)) {
            let d = (p.x - x).abs();
            if d <= 5.0 && near.as_ref().is_none_or(|(nd, _)| d < *nd) {
                near = Some((d, what));
            }
        }
    };
    let fmt = |t: f64| crate::editing::tidy((t * 100.0).round() / 100.0);
    let mut alt = false;
    for mk in &marks {
        if let Mark::Section(label, a, b) = mk {
            let r = Rect::from_min_max(pos2(x_of(*a), lane.top() + 1.0), pos2(x_of(*b), lane.bottom() - 1.0));
            painter.rect_filled(r, radius::SMALL, if alt { color::SONG_SECTION_ALT } else { color::SONG_SECTION });
            alt = !alt;
            let clip = painter.with_clip_rect(painter.clip_rect().intersect(r.shrink2(vec2(3.0, 0.0))));
            clip.text(pos2(r.left().max(painter.clip_rect().left()) + 5.0, lane.top() + 2.0), Align2::LEFT_TOP, label, theme::mono_small(), color::TEXT_FAINT);
        }
    }
    for mk in &marks {
        match mk {
            Mark::Downbeat(t) => {
                let x = x_of(*t);
                painter.vline(x, lane.top() + 2.0..=lane.bottom() - 2.0, Stroke::new(1.0, color::SONG_BEAT));
                consider(x, format!("downbeat (bar start) at {} {unit}", fmt(*t)));
            }
            Mark::Kick(t, s) => {
                let x = x_of(*t);
                painter.circle_filled(pos2(x, mid + 6.0), 1.5 + 2.0 * *s as f32, color::SONG_KICK.gamma_multiply(0.45 + 0.55 * *s as f32));
                consider(x, format!("kick at {} {unit} · strength {}", fmt(*t), fmt(*s)));
            }
            Mark::Snare(t, s) => {
                let x = x_of(*t);
                let r = 1.5 + 2.0 * *s as f32;
                let c = pos2(x, mid - 4.0);
                let fill = color::SONG_SNARE.gamma_multiply(0.45 + 0.55 * *s as f32);
                painter.add(Shape::convex_polygon(vec![pos2(c.x, c.y - r), pos2(c.x + r, c.y), pos2(c.x, c.y + r), pos2(c.x - r, c.y)], fill, Stroke::NONE));
                consider(x, format!("snare at {} {unit} · strength {}", fmt(*t), fmt(*s)));
            }
            Mark::Drop(t) => {
                let x = x_of(*t);
                painter.vline(x, lane.top()..=lane.bottom(), Stroke::new(1.5, color::MARKER));
                painter.rect_filled(Rect::from_min_size(pos2(x, lane.top() + 1.0), vec2(6.0, 6.0)), egui::CornerRadius { nw: 0, ne: 2, sw: 0, se: 2 }, color::MARKER);
                painter.text(pos2(x + 9.0, lane.top() + 1.0), Align2::LEFT_TOP, "drop", theme::small(), color::MARKER);
                consider(x, format!("the drop at {} {unit}", fmt(*t)));
            }
            Mark::Section(..) => {}
        }
    }
    near.map(|(_, w)| w)
}

// ---- dialogs ----------------------------------------------------------------------

fn modal_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(color::RAISED)
        .stroke(Stroke::new(1.0, color::CONTROL_HOVER))
        .corner_radius(egui::CornerRadius::same(radius::LARGE))
        .inner_margin(egui::Margin::same(20))
        .shadow(egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(160) })
}

fn title(ui: &mut Ui, icon: Icon, text_: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(widgets::icon(icon).color(color::ACCENT_TEXT));
        ui.label(RichText::new(text_).font(FontId::new(text::HEADLINE, theme::strong())).color(color::TEXT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if tip(widgets::small_icon_button(ui, Icon::X), "Close", &["Esc"]).clicked() {
                close = true;
            }
        });
    });
    close
}

/// A row of a form: a label column, then the controls.
fn row(ui: &mut Ui, label: &str, hover: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let r = ui.add_sized(vec2(96.0, 24.0), egui::Label::new(RichText::new(label).color(color::TEXT_MUTED)));
        if !hover.is_empty() {
            r.on_hover_text(hover);
        }
        add(ui);
    });
}

fn time_field(ui: &mut Ui, value: &mut String, hint: &str) {
    ui.add(egui::TextEdit::singleline(value).hint_text(hint).font(theme::mono()).desired_width(110.0).margin(egui::Margin::symmetric(6, 3)));
}

const TIME_HELP: &str = "A time: 12.5 (project time), b12.5 (beat), bar 4, 4:3 (bar 4, beat 3), 2.5s, f60, a marker such as drop, drop+2b. Empty: the start / the end.";

/// What `on` can be, with the song's onsets only when there is an analysis.
fn on_choices(analysed: bool) -> Vec<&'static str> {
    beatops::ON.iter().copied().filter(|o| analysed || !matches!(*o, "kicks" | "snares" | "hats")).filter(|o| *o != "bars").collect()
}

pub(crate) fn dialogs(app: &mut App, ctx: &egui::Context) {
    cut_dialog(app, ctx);
    accent_dialog(app, ctx);
}

fn cut_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut form) = app.cut_form.take() else { return };
    let analysed = app.song.info.is_some();
    let doc = app.state.doc.clone();
    let mut run = false;
    let mut close = false;
    let resp = egui::Modal::new(egui::Id::unique("edits-cut")).backdrop_color(color::BACKDROP).frame(modal_frame()).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.spacing_mut().item_spacing.y = space::S;
        close |= title(ui, Icon::Scissors, "Cut on beats");
        ui.label(RichText::new("New image layers, one per beat (or bar, or onset), cycling through the slots you pick, on top of the edit. One edit: Ctrl+Z takes them all back.").color(color::TEXT_MUTED));
        row(ui, "Slots", "The pictures to cycle through, in this order", |ui| {
            ui.horizontal_wrapped(|ui| {
                for (name, on) in form.slots.iter_mut() {
                    ui.checkbox(on, name.as_str());
                }
            });
        });
        let mut every = matches!(form.on, CutOn::Every(_));
        row(ui, "Cut", "Every N beats, or on the song's beats or onsets", |ui| {
            let mut choice = every;
            if widgets::segmented(ui, &mut choice, &[(true, "Every N beats"), (false, "On…")]) && choice != every {
                every = choice;
                form.on = if every { CutOn::Every(1.0) } else { CutOn::On(if analysed { "kicks".into() } else { "beats".into() }) };
            }
        });
        match &mut form.on {
            CutOn::Every(n) => row(ui, "Beats each", "4 = one picture per bar, 0.5 = two per beat", |ui| {
                ui.add(egui::DragValue::new(n).speed(0.05).range(0.125..=64.0).max_decimals(3));
                for q in [0.5, 1.0, 2.0, 4.0] {
                    if ui.selectable_label((*n - q).abs() < 1e-9, crate::inspector::num(q)).clicked() {
                        *n = q;
                    }
                }
            }),
            CutOn::On(on) => row(ui, "On", "Song onsets need the analysis (Analyse song)", |ui| {
                widgets::combo("cut-on", on.as_str(), 120.0).show_ui(ui, |ui| {
                    for o in on_choices(analysed) {
                        if o != "drop" && ui.selectable_label(on == o, o).clicked() {
                            *on = o.to_string();
                        }
                    }
                });
            }),
        }
        if matches!(&form.on, CutOn::On(o) if matches!(o.as_str(), "kicks" | "snares" | "hats")) {
            row(ui, "Min strength", "Onsets weaker than this (0 to 1) are not cut on", |ui| {
                ui.add(egui::Slider::new(&mut form.min_strength, 0.0..=1.0).step_by(0.05));
            });
        }
        row(ui, "From", TIME_HELP, |ui| {
            time_field(ui, &mut form.from, "start");
            ui.label(RichText::new("to").color(color::TEXT_MUTED));
            time_field(ui, &mut form.to, "end");
        });
        row(ui, "Layer ids", "New layers are called <prefix>1, <prefix>2, …", |ui| {
            ui.add(egui::TextEdit::singleline(&mut form.prefix).font(theme::mono()).desired_width(110.0).margin(egui::Margin::symmetric(6, 3)));
        });
        // What it would do, before doing it.
        let plan = doc.as_ref().map(|d| form.apply(d, app.song_info()));
        let ok = matches!(plan, Some(Ok(_)));
        match &plan {
            Some(Ok((_, n))) => ui.label(RichText::new(format!("Adds {n} layer{}.", if *n == 1 { "" } else { "s" })).color(color::OK_TEXT)),
            Some(Err(e)) => ui.label(RichText::new(e).color(color::ERROR_TEXT)),
            None => ui.label(RichText::new("No valid project.").color(color::ERROR_TEXT)),
        };
        ui.add_space(space::XS);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add_enabled_ui(ok, |ui| widgets::button(ui, Some(Icon::Scissors), "Cut", true)).inner.clicked() {
                run = true;
            }
            if widgets::button(ui, None, "Cancel", false).clicked() {
                close = true;
            }
        });
    });
    if close || resp.should_close() {
        return;
    }
    app.cut_form = Some(form);
    if run {
        app.run_cut();
    }
}

fn accent_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut form) = app.accent_form.take() else { return };
    let analysed = app.song.info.is_some();
    let lib = app.library().clone();
    let doc = app.state.doc.clone();
    let slots: Vec<String> = app.project().map(|p| p.slots.keys().cloned().collect()).unwrap_or_default();
    let selected = app.selected_layer();
    let mut run = false;
    let mut close = false;
    let resp = egui::Modal::new(egui::Id::unique("edits-accent")).backdrop_color(color::BACKDROP).frame(modal_frame()).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.spacing_mut().item_spacing.y = space::S;
        close |= title(ui, Icon::Zap, "Accent on beats");
        ui.label(RichText::new("A preset at every beat, downbeat, kick, snare or the drop, as one edit.").color(color::TEXT_MUTED));
        row(ui, "Preset", "", |ui| {
            widgets::combo("accent-preset", form.preset.as_str(), 160.0).show_ui(ui, |ui| {
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for p in lib.presets.iter().filter(|p| !presets::is_transition(p)) {
                        if ui.selectable_label(p.name == form.preset, &p.name).on_hover_text(&p.description).clicked() && p.name != form.preset {
                            form = song::AccentForm { preset: p.name.clone(), values: presets::defaults(p), ..form.clone() };
                        }
                    }
                });
            });
        });
        row(ui, "On", "Song onsets need the analysis (Analyse song)", |ui| {
            widgets::combo("accent-on", form.on.as_str(), 120.0).show_ui(ui, |ui| {
                for o in on_choices(analysed) {
                    if ui.selectable_label(form.on == o, o).clicked() {
                        form.on = o.to_string();
                    }
                }
            });
            ui.label(RichText::new("every").color(color::TEXT_MUTED));
            let suffix = match form.every_nth {
                1 => "st",
                2 => "nd",
                3 => "rd",
                _ => "th",
            };
            ui.add(egui::DragValue::new(&mut form.every_nth).range(1..=16).speed(0.05).suffix(suffix));
        });
        if matches!(form.on.as_str(), "kicks" | "snares" | "hats") {
            row(ui, "Min strength", "Onsets weaker than this (0 to 1) are skipped", |ui| {
                ui.add(egui::Slider::new(&mut form.min_strength, 0.0..=1.0).step_by(0.05));
            });
        }
        row(ui, "From", TIME_HELP, |ui| {
            time_field(ui, &mut form.from, "start");
            ui.label(RichText::new("to").color(color::TEXT_MUTED));
            time_field(ui, &mut form.to, "end");
        });
        let p = lib.get(&form.preset).cloned();
        if let Some(p) = &p {
            if p.target == edits_core::preset::TargetKind::Layer {
                row(ui, "On layer", "", |ui| {
                    let mut choice = form.target;
                    let options: Vec<(TargetChoice, &str)> = TargetChoice::ALL.iter().map(|c| (*c, c.label())).collect();
                    if widgets::segmented(ui, &mut choice, &options) {
                        form.target = choice;
                    }
                });
            }
            ui.label(widgets::eyebrow("Variables"));
            var_rows(ui, p, &mut form.values, &slots);
        }
        let plan = match (&doc, &p) {
            (Some(d), Some(p)) => Some(form.times(d, app.song_info()).and_then(|t| form.apply(d, p, selected.as_deref(), app.song_info()).map(|_| t))),
            _ => None,
        };
        let ok = matches!(plan, Some(Ok(_)));
        match &plan {
            Some(Ok(times)) => {
                let list: Vec<String> = times.iter().take(8).map(|t| crate::inspector::num(*t)).collect();
                let more = if times.len() > 8 { " …" } else { "" };
                ui.label(RichText::new(format!("{} time{}: {}{more}", times.len(), if times.len() == 1 { "" } else { "s" }, list.join(", "))).color(color::OK_TEXT));
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(color::ERROR_TEXT));
            }
            None => {
                ui.label(RichText::new("No valid project.").color(color::ERROR_TEXT));
            }
        }
        ui.add_space(space::XS);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add_enabled_ui(ok, |ui| widgets::button(ui, Some(Icon::Zap), "Accent", true)).inner.clicked() {
                run = true;
            }
            if widgets::button(ui, None, "Cancel", false).clicked() {
                close = true;
            }
        });
    });
    if close || resp.should_close() {
        return;
    }
    app.accent_form = Some(form);
    if run {
        app.run_accent();
    }
}

fn var_rows(ui: &mut Ui, p: &edits_core::preset::Preset, values: &mut serde_json::Map<String, serde_json::Value>, slots: &[String]) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        crate::presetpanel::var_editor(ui, p, values, slots, "accent");
    });
}
