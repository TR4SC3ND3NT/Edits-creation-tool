//! Toasts: short notes in the corner of the preview for things that happen
//! (an agent's edit landing, a reload, an export finishing, an error), the
//! agent's edits with an Undo button. At most three at a time; a repeat of
//! the newest one counts up instead of stacking; hovering one keeps it.
//!
//! The bookkeeping and the reading of the shared history are pure and unit
//! tested; `show` draws them.

use crate::editing::{actor_label, readable_summary};
use crate::theme::{self, color, radius, size, space, text};
use crate::widgets::{self, Icon};
use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Ui};
use edits_core::edit::{HistoryEntry, HistoryList, AGENT};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Info,
    Ok,
    Warn,
    Error,
    /// Something the agent did.
    Agent,
}

/// What a toast's button does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Undo the agent's edit, if it is still the newest one in the history.
    UndoEdit(HistoryEntry),
    Redo,
    OpenExport,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub action: Option<(String, Action)>,
    pub born: Instant,
    pub ttl: Duration,
    /// How many times the same toast came in a row.
    pub count: u32,
}

impl Toast {
    fn left(&self, now: Instant) -> Duration {
        self.ttl.saturating_sub(now.saturating_duration_since(self.born))
    }
}

pub const MAX: usize = 3;
const SHORT: Duration = Duration::from_secs(4);
const LONG: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct Toasts {
    pub list: Vec<Toast>,
    next_id: u64,
}

impl Toasts {
    /// Add a toast; the same toast as the newest one counts up instead.
    pub fn push(&mut self, now: Instant, kind: Kind, title: impl Into<String>, body: impl Into<String>, action: Option<(String, Action)>) {
        let (title, body) = (title.into(), body.into());
        let ttl = if action.is_some() || kind == Kind::Error { LONG } else { SHORT };
        if let Some(last) = self.list.last_mut().filter(|t| t.kind == kind && t.title == title && t.body == body) {
            last.count += 1;
            last.born = now;
            last.action = action;
            return;
        }
        self.next_id += 1;
        self.list.push(Toast { id: self.next_id, kind, title, body, action, born: now, ttl, count: 1 });
        if self.list.len() > MAX {
            self.list.remove(0);
        }
    }

    pub fn dismiss(&mut self, id: u64) {
        self.list.retain(|t| t.id != id);
    }

    /// Keep a toast (it is hovered): its clock starts again.
    pub fn hold(&mut self, id: u64, now: Instant) {
        if let Some(t) = self.list.iter_mut().find(|t| t.id == id) {
            t.born = now;
        }
    }

    /// Drop the toasts whose time is up; how long until the next one is.
    pub fn prune(&mut self, now: Instant) -> Option<Duration> {
        self.list.retain(|t| !t.left(now).is_zero());
        self.list.iter().map(|t| t.left(now)).min()
    }
}

/// What a change of the project file on disk was, read from the shared history.
#[derive(Clone, Debug, PartialEq)]
pub enum DiskChange {
    /// A new edit in the history (by the agent, or another program writing through `edits_core::edit`).
    Edit(HistoryEntry),
    /// Someone else's undo took this edit back.
    Undo(HistoryEntry),
    /// The file changed without a new history entry: a text editor, or a script.
    Plain,
}

/// Compare the history before and after a change on disk.
pub fn disk_change(before: &HistoryList, after: &HistoryList) -> DiskChange {
    if let Some(newest) = after.undo.last() {
        if !before.undo.iter().any(|e| e.file == newest.file) {
            return DiskChange::Edit(newest.clone());
        }
    }
    if after.undo.len() < before.undo.len() {
        if let Some(taken) = before.undo.last() {
            return DiskChange::Undo(taken.clone());
        }
    }
    DiskChange::Plain
}

/// The toast for a change on disk: kind, title, body, button.
pub fn for_disk_change(change: &DiskChange, revision: u64) -> (Kind, String, String, Option<(String, Action)>) {
    match change {
        DiskChange::Edit(e) if e.actor == AGENT => (Kind::Agent, "Agent edit".into(), readable_summary(&e.summary), Some(("Undo".into(), Action::UndoEdit(e.clone())))),
        DiskChange::Edit(e) => (Kind::Info, format!("Edit by {}", actor_label(&e.actor)), readable_summary(&e.summary), None),
        DiskChange::Undo(e) if e.actor == AGENT => (Kind::Agent, "Agent undid its edit".into(), readable_summary(&e.summary), Some(("Redo".into(), Action::Redo))),
        DiskChange::Undo(e) => (Kind::Agent, "Agent undid an edit".into(), format!("{} ({})", readable_summary(&e.summary), actor_label(&e.actor)), Some(("Redo".into(), Action::Redo))),
        DiskChange::Plain => (Kind::Info, "Reloaded".into(), format!("The file changed on disk · revision {revision}"), None),
    }
}

fn accent(kind: Kind) -> (Icon, Color32) {
    match kind {
        Kind::Info => (Icon::RefreshCw, color::TEXT_MUTED),
        Kind::Ok => (Icon::CircleCheck, color::OK_TEXT),
        Kind::Warn => (Icon::TriangleAlert, color::WARN),
        Kind::Error => (Icon::CircleAlert, color::ERROR_TEXT),
        Kind::Agent => (Icon::Bot, color::AGENT),
    }
}

/// Draw the toasts in the bottom-right corner of `area`; returns the
/// button that was clicked, if any.
pub fn show(ctx: &egui::Context, area: Rect, toasts: &mut Toasts) -> Option<Action> {
    let now = Instant::now();
    if let Some(next) = toasts.prune(now) {
        ctx.request_repaint_after(next.min(Duration::from_millis(250)));
    }
    let mut clicked = None;
    let mut dismiss = None;
    let mut hold = None;
    let mut bottom = area.bottom() - space::L;
    for t in toasts.list.iter().rev() {
        let id = egui::Id::unique(("toast", t.id));
        let appear = ctx.animate_bool_with_time(id, true, 0.16);
        let width = size::TOAST_WIDTH.min(area.width() - 2.0 * space::L).max(160.0);
        let inner = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(area.right() - space::L - width, bottom))
            .pivot(Align2::LEFT_BOTTOM)
            .interactable(true)
            .show(ctx, |ui: &mut Ui| {
                ui.set_opacity(appear);
                egui::Frame::new()
                    .fill(color::RAISED)
                    .stroke(Stroke::new(1.0, color::CONTROL_HOVER))
                    .corner_radius(CornerRadius::same(radius::LARGE))
                    .inner_margin(egui::Margin { left: 12, right: 8, top: 10, bottom: 10 })
                    .shadow(egui::Shadow { offset: [0, 6], blur: 16, spread: 0, color: Color32::from_black_alpha(140) })
                    .show(ui, |ui| {
                        ui.set_width(width - 20.0);
                        let (icon, tint) = accent(t.kind);
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = space::M;
                            let (r, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
                            widgets::paint_icon(ui.painter(), r.center(), icon, text::ICON_SMALL, tint);
                            let text_w = ui.available_width() - 22.0 - if t.action.is_some() { 58.0 } else { 0.0 };
                            ui.vertical(|ui| {
                                ui.set_width(text_w.max(60.0));
                                ui.spacing_mut().item_spacing.y = 1.0;
                                let title = if t.count > 1 { format!("{}  ×{}", t.title, t.count) } else { t.title.clone() };
                                ui.label(RichText::new(title).font(FontId::new(text::BODY, theme::medium())).color(color::TEXT));
                                if !t.body.is_empty() {
                                    ui.add(egui::Label::new(RichText::new(&t.body).font(theme::small()).color(color::TEXT_MUTED)).truncate()).on_hover_text(&t.body);
                                }
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                if widgets::small_icon_button(ui, Icon::X).on_hover_text("Dismiss").clicked() {
                                    dismiss = Some(t.id);
                                }
                                if let Some((label, action)) = &t.action {
                                    if widgets::button(ui, None, label, false).clicked() {
                                        clicked = Some(action.clone());
                                        dismiss = Some(t.id);
                                    }
                                }
                            });
                        });
                    });
            });
        if inner.response.contains_pointer() {
            hold = Some(t.id);
        }
        bottom = inner.response.rect.top() - space::M;
    }
    if let Some(id) = hold {
        toasts.hold(id, now);
    }
    if let Some(id) = dismiss {
        toasts.dismiss(id);
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(file: &str, actor: &str) -> HistoryEntry {
        HistoryEntry { file: PathBuf::from(file), time: None, actor: actor.into(), summary: format!("edit {file}") }
    }

    #[test]
    fn repeats_count_up_and_at_most_three_stay() {
        let now = Instant::now();
        let mut t = Toasts::default();
        t.push(now, Kind::Info, "Reloaded", "rev 2", None);
        t.push(now, Kind::Info, "Reloaded", "rev 2", None);
        assert_eq!(t.list.len(), 1);
        assert_eq!(t.list[0].count, 2);
        for i in 0..4 {
            t.push(now, Kind::Ok, format!("done {i}"), "", None);
        }
        assert_eq!(t.list.len(), MAX);
        assert_eq!(t.list[0].title, "done 1", "the oldest went first");
    }

    #[test]
    fn toasts_expire_unless_held() {
        let now = Instant::now();
        let mut t = Toasts::default();
        t.push(now, Kind::Info, "a", "", None);
        t.push(now, Kind::Agent, "b", "", Some(("Undo".into(), Action::Redo)));
        assert_eq!(t.prune(now + Duration::from_secs(1)), Some(Duration::from_secs(3)));
        let id = t.list[1].id;
        t.hold(id, now + Duration::from_secs(5));
        t.prune(now + Duration::from_secs(5));
        assert_eq!(t.list.len(), 1, "the short one is gone");
        assert!(t.prune(now + Duration::from_secs(12)).is_some(), "the held one is still there");
        assert_eq!(t.prune(now + Duration::from_secs(14)), None);
        assert!(t.list.is_empty());
    }

    #[test]
    fn reads_what_happened_from_the_history() {
        let before = HistoryList { undo: vec![entry("a", "window")], redo: vec![] };
        let edited = HistoryList { undo: vec![entry("a", "window"), entry("b", AGENT)], redo: vec![] };
        assert_eq!(disk_change(&before, &edited), DiskChange::Edit(entry("b", AGENT)));
        let (kind, _, _, action) = for_disk_change(&disk_change(&before, &edited), 3);
        assert_eq!(kind, Kind::Agent);
        assert_eq!(action, Some(("Undo".into(), Action::UndoEdit(entry("b", AGENT)))));

        let undone = HistoryList { undo: vec![], redo: vec![entry("a2", "window")] };
        assert_eq!(disk_change(&before, &undone), DiskChange::Undo(entry("a", "window")));
        assert_eq!(disk_change(&before, &before), DiskChange::Plain);
        let (kind, title, _, action) = for_disk_change(&DiskChange::Plain, 7);
        assert_eq!((kind, title.as_str(), action), (Kind::Info, "Reloaded", None));
    }
}
