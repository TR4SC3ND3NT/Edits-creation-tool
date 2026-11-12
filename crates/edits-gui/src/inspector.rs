//! The right-hand panel: the Problems list, and the read-only inspector that
//! shows what the selected layer evaluates to at the playhead.

use crate::app::{pal, App, Selected};
use crate::reload::Problem;
use crate::rows::{blend_text, prop_key_times};
use eframe::egui::{self, Color32, RichText, Ui};
use edits_core::{Evaluator, Layer, LayerKind, Modifier, Project, Prop};

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
    ui.horizontal(|ui| {
        ui.strong("Problems");
        if errors > 0 {
            ui.colored_label(pal::BAD, format!("{errors} error{}", if errors == 1 { "" } else { "s" }));
        }
        if warnings > 0 {
            ui.colored_label(pal::WARN, format!("{warnings} warning{}", if warnings == 1 { "" } else { "s" }));
        }
    });
    if app.state.rejected {
        let what = if app.state.current.is_some() {
            format!("The file on disk is not shown. The preview stays on revision {}, the last version without errors.", app.state.revision)
        } else {
            "The file has never been valid, so there is nothing to show yet.".to_string()
        };
        ui.label(RichText::new(what).color(pal::BAD));
    }
    let max = (ui.available_height() * 0.6).max(120.0);
    egui::ScrollArea::vertical().id_salt("problems").max_height(max).auto_shrink([false, true]).show(ui, |ui| {
        for p in &app.state.problems {
            problem(ui, p);
        }
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

fn camera(ui: &mut Ui, p: &Project, t: f64) {
    let c = Evaluator::new(p).camera(t);
    egui::Grid::new("camera").num_columns(3).spacing([10.0, 3.0]).show(ui, |ui| {
        row(ui, "fov", num(c.fov), &animation_note(&p.camera.fov));
        row(ui, "position", vector(&c.position), &animation_note(&p.camera.position));
        row(ui, "rotation", vector(&c.rotation), &animation_note(&p.camera.rotation));
    });
}

fn layer(ui: &mut Ui, p: &Project, l: &Layer, t: f64, frame_of: impl Fn(f64) -> u64) {
    let ev = Evaluator::new(p);
    let end = l.end.unwrap_or(p.settings.duration);
    let active = ev.active(l, t);
    egui::Grid::new("layer").num_columns(3).spacing([10.0, 3.0]).show(ui, |ui| {
        match &l.kind {
            LayerKind::Image { slot } => {
                row(ui, "kind", "image".into(), "");
                let file = p.slots.get(slot).and_then(|s| s.path.clone()).unwrap_or_else(|| "(empty: placeholder)".into());
                row(ui, "slot", format!("{slot}: {file}"), "");
            }
            LayerKind::Solid { color } => {
                row(ui, "kind", "solid".into(), "");
                row(ui, "color", vector(&color.map(|c| c as f64)), "");
            }
            LayerKind::Cube { faces, size } => {
                row(ui, "kind", "cube".into(), "");
                row(ui, "faces", faces.join(", "), "");
                row(ui, "size", vector(size), "");
            }
            LayerKind::Adjustment => row(ui, "kind", "adjustment".into(), ""),
        }
        if let Some(name) = &l.name {
            row(ui, "name", name.clone(), "");
        }
        row(ui, "in .. out", format!("{} .. {}", num(l.start), num(end)), &format!("frames {}..{}", frame_of(l.start), frame_of(end)));
        row(ui, "blend", blend_text(l.blend), "");
        ui.label(RichText::new("now").color(ui.visuals().weak_text_color()));
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
    let r = ev.layer(l, t);
    let tr = &l.transform;
    egui::Grid::new("transform").num_columns(3).spacing([10.0, 3.0]).show(ui, |ui| {
        row(ui, "position", vector(&r.position), &animation_note(&tr.position));
        row(ui, "scale", vector(&r.scale), &animation_note(&tr.scale));
        row(ui, "rotation", vector(&r.rotation), &animation_note(&tr.rotation));
        row(ui, "anchor", vector(&r.anchor), &animation_note(&tr.anchor));
        row(ui, "opacity", num(r.opacity), &animation_note(&tr.opacity));
    });

    for (i, e) in l.effects.iter().enumerate() {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.strong(format!("Effect {}: {}", i + 1, e.kind));
            if !e.enabled {
                ui.colored_label(pal::WARN, "off");
            }
        });
        let resolved = ev.effect(e, t);
        egui::Grid::new(("effect", i)).num_columns(3).spacing([10.0, 3.0]).show(ui, |ui| {
            if let Some(src) = &e.source {
                row(ui, "source", src.clone(), "");
            }
            for (name, value) in &resolved.params {
                match e.params.get(name) {
                    Some(prop) => row(ui, name, vector(value), &animation_note(prop)),
                    None => {
                        // Not in the file: the registry's default.
                        let weak = ui.visuals().weak_text_color();
                        ui.label(RichText::new(name).color(weak));
                        ui.label(RichText::new(vector(value)).monospace().color(weak));
                        ui.label(RichText::new("default").small().color(weak));
                        ui.end_row();
                    }
                }
            }
        });
    }
    if l.effects.is_empty() {
        ui.add_space(6.0);
        ui.weak("No effects.");
    }
}

pub(crate) fn show(app: &mut App, ui: &mut Ui) {
    ui.add_space(4.0);
    problems(app, ui);
    ui.separator();

    let (Some(tm), Some(project)) = (app.tm, app.project().cloned()) else {
        ui.weak("Nothing to inspect yet.");
        return;
    };
    let t = tm.frame_time(app.playhead);
    egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).show(ui, |ui| {
        let at = format!("at frame {} · {} {}", app.playhead, num(t), if tm.uses_beats() { "beats" } else { "s" });
        match app.selected.clone() {
            Some(Selected::Layer(id)) => match project.layers.iter().find(|l| l.id == id) {
                Some(l) => {
                    ui.horizontal(|ui| {
                        ui.heading(&l.id);
                        if ui.small_button("×").on_hover_text("Deselect").clicked() {
                            app.selected = None;
                        }
                    });
                    ui.weak(format!("Values {at} (read-only)"));
                    ui.add_space(4.0);
                    layer(ui, &project, l, t, |time| tm.frame_at(time));
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
                ui.weak(format!("Values {at} (read-only)"));
                ui.add_space(4.0);
                camera(ui, &project, t);
            }
            None => {
                ui.heading("Inspector");
                ui.weak("Click a layer in the timeline to see what it evaluates to at the playhead.");
                ui.add_space(6.0);
                ui.strong("Project");
                project_summary(ui, &project);
                ui.add_space(6.0);
                ui.strong("Camera");
                ui.weak(at);
                camera(ui, &project, t);
            }
        }
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
}
