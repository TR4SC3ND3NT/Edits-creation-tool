//! The command palette (Ctrl+K): every action of the window by name, with
//! fuzzy search. Typing narrows the list; Up/Down move, Enter runs, Esc
//! closes. The list is built from the window's state each time it opens
//! (layers, slots, markers and effects are commands too).
//!
//! The matching is pure and unit tested; `show` draws the palette.

use crate::theme::{self, color, radius, size, space, text};
use crate::widgets::{self, Icon};
use eframe::egui::{self, pos2, vec2, Align2, CornerRadius, FontId, Key, Modifiers, Rect, Sense, Stroke, Ui};

/// One entry of the palette.
#[derive(Clone, Debug)]
pub struct Command<A> {
    pub action: A,
    pub title: String,
    /// Where it belongs: `Playback`, `Layer`, `Go to`…
    pub group: &'static str,
    pub icon: Icon,
    pub keys: Option<&'static str>,
}

impl<A> Command<A> {
    pub fn new(group: &'static str, icon: Icon, title: impl Into<String>, action: A) -> Self {
        Self { action, title: title.into(), group, icon, keys: None }
    }

    pub fn keys(mut self, keys: &'static str) -> Self {
        self.keys = Some(keys);
        self
    }
}

/// How well `query` matches `text`: `None` unless every character of the
/// query appears in order. Higher is better: a prefix beats a word start,
/// which beats a run of letters, which beats letters scattered about.
pub fn score(query: &str, text: &str) -> Option<i64> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let lower: String = t.iter().collect();
    let compact: String = q.iter().collect();
    let mut s: i64 = 0;
    if lower.starts_with(&compact) {
        s += 60;
    } else if lower.contains(&compact) {
        s += 30;
    }
    let mut qi = 0;
    let mut last: Option<usize> = None;
    for (i, c) in t.iter().enumerate() {
        if qi == q.len() {
            break;
        }
        if *c != q[qi] {
            continue;
        }
        let word_start = i == 0 || !t[i - 1].is_alphanumeric();
        s += 1;
        if word_start {
            s += 8;
        }
        match last {
            Some(l) if l + 1 == i => s += 5,
            Some(l) => s -= ((i - l - 1) as i64).min(4),
            None => s -= (i as i64).min(6),
        }
        last = Some(i);
        qi += 1;
    }
    (qi == q.len()).then_some(s)
}

/// The commands matching `query`, best first (ties keep their order).
pub fn rank<A>(commands: &[Command<A>], query: &str) -> Vec<usize> {
    let mut hits: Vec<(i64, usize)> = commands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let title = score(query, &c.title);
            let grouped = score(query, &format!("{} {}", c.group, c.title)).map(|s| s - 10);
            title.max(grouped).map(|s| (s, i))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, i)| i).collect()
}

/// The palette's own state.
#[derive(Default)]
pub struct Palette {
    pub open: bool,
    pub query: String,
    pub cursor: usize,
    /// Focus the search field on the next frame.
    focus: bool,
}

impl Palette {
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.cursor = 0;
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }
}

const ROW: f32 = 32.0;
const VISIBLE_ROWS: f32 = 10.0;

/// Draw the palette over the window; returns the command chosen, if any.
pub fn show<A: Clone>(ctx: &egui::Context, palette: &mut Palette, commands: &[Command<A>]) -> Option<A> {
    if !palette.open {
        return None;
    }
    let hits = rank(commands, &palette.query);
    // Keys for the list, taken before the text field sees them.
    let (up, down, enter, page_up, page_down) = ctx.input_mut(|i| {
        (
            i.count_and_consume_key(Modifiers::NONE, Key::ArrowUp),
            i.count_and_consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::PageUp),
            i.consume_key(Modifiers::NONE, Key::PageDown),
        )
    });
    let n = hits.len();
    if n > 0 {
        let step = down as i64 - up as i64 + if page_down { 8 } else { 0 } - if page_up { 8 } else { 0 };
        palette.cursor = (palette.cursor as i64 + step).clamp(0, n as i64 - 1) as usize;
    } else {
        palette.cursor = 0;
    }
    let moved = up + down > 0 || page_up || page_down;
    let mut chosen = if enter { hits.get(palette.cursor).map(|&i| commands[i].action.clone()) } else { None };

    let screen = ctx.content_rect();
    let width = size::PALETTE_WIDTH.min(screen.width() - 32.0);
    let modal = egui::Modal::new(egui::Id::unique("command-palette"))
        .backdrop_color(color::BACKDROP)
        .area(egui::Modal::default_area(egui::Id::unique("command-palette-area")).anchor(Align2::CENTER_TOP, vec2(0.0, (screen.height() * 0.12).clamp(40.0, 120.0))))
        .frame(
            egui::Frame::new()
                .fill(color::RAISED)
                .stroke(Stroke::new(1.0, color::CONTROL_HOVER))
                .corner_radius(CornerRadius::same(radius::LARGE))
                .shadow(egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: egui::Color32::from_black_alpha(160) }),
        );
    let resp = modal.show(ctx, |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        // ---- the search field ----
        ui.horizontal(|ui| {
            ui.set_height(48.0);
            ui.add_space(space::XL);
            ui.label(widgets::icon_sized(Icon::Search, text::ICON).color(color::TEXT_FAINT));
            ui.add_space(space::M);
            let field = egui::TextEdit::singleline(&mut palette.query)
                .frame(egui::Frame::NONE)
                .font(FontId::new(14.0, theme::body().family))
                .text_color(color::TEXT)
                .hint_text(egui::RichText::new("Search commands, layers, effects…").color(color::TEXT_FAINT))
                .desired_width(ui.available_width() - 64.0);
            let r = ui.add(field);
            if palette.focus {
                r.request_focus();
                palette.focus = false;
            }
            if r.changed() {
                palette.cursor = 0;
            }
            ui.add_space(space::S);
            widgets::kbd(ui, "Esc");
        });
        let (line, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
        ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, color::LINE));

        // ---- the list ----
        if hits.is_empty() {
            ui.allocate_ui(vec2(width, 72.0), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(format!("Nothing matches \"{}\"", palette.query.trim())).color(color::TEXT_FAINT));
                });
            });
            return;
        }
        egui::ScrollArea::vertical().max_height(ROW * VISIBLE_ROWS + 2.0 * space::S).auto_shrink([false, true]).show(ui, |ui| {
            ui.add_space(space::S);
            let mut group: Option<&str> = None;
            for (pos, &i) in hits.iter().enumerate() {
                let c = &commands[i];
                // Group titles only for the full list: when searching, the best match comes first.
                if palette.query.trim().is_empty() && group != Some(c.group) {
                    group = Some(c.group);
                    let (r, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
                    ui.painter().text(pos2(r.left() + space::XL, r.bottom() - 6.0), Align2::LEFT_BOTTOM, c.group, FontId::new(text::LABEL, theme::medium()), color::TEXT_FAINT);
                }
                let (r, resp) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                let row = Rect::from_min_max(pos2(r.left() + space::S, r.top()), pos2(r.right() - space::S, r.bottom()));
                if resp.hovered() && ui.ctx().input(|i| i.pointer.delta().length_sq() > 0.0) {
                    palette.cursor = pos;
                }
                let on = pos == palette.cursor;
                if on {
                    ui.painter().rect_filled(row, radius::NORMAL, color::CONTROL_HOVER);
                    if moved {
                        ui.scroll_to_rect(r, None);
                    }
                }
                widgets::paint_icon(ui.painter(), pos2(row.left() + 18.0, row.center().y), c.icon, text::ICON_SMALL, if on { color::TEXT } else { color::TEXT_MUTED });
                let title = ui.painter().text(pos2(row.left() + 38.0, row.center().y), Align2::LEFT_CENTER, &c.title, theme::body(), color::TEXT);
                if !palette.query.trim().is_empty() {
                    ui.painter().text(pos2(title.right() + space::M, row.center().y), Align2::LEFT_CENTER, c.group, theme::small(), color::TEXT_FAINT);
                }
                if let Some(keys) = c.keys {
                    let mut x = row.right() - space::M;
                    for k in keys.split('+').rev() {
                        let g = ui.painter().layout_no_wrap(k.to_string(), theme::mono_small(), color::TEXT_MUTED);
                        let w = (g.size().x + 8.0).max(16.0);
                        let cap = Rect::from_min_size(pos2(x - w, row.center().y - 8.0), vec2(w, 16.0));
                        ui.painter().rect(cap, radius::SMALL, color::CONTROL, Stroke::new(1.0, color::LINE_STRONG), egui::StrokeKind::Inside);
                        ui.painter().galley(cap.center() - g.size() / 2.0, g, color::TEXT_MUTED);
                        x -= w + 3.0;
                    }
                }
                if resp.clicked() {
                    chosen = Some(c.action.clone());
                }
            }
            ui.add_space(space::S);
        });
        // ---- footer ----
        let (line, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
        ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, color::LINE));
        ui.horizontal(|ui: &mut Ui| {
            ui.set_height(32.0);
            ui.add_space(space::XL);
            ui.spacing_mut().item_spacing.x = space::XS;
            for (keys, what) in [("↑+↓", "move"), ("Enter", "run"), ("Esc", "close")] {
                widgets::shortcut(ui, keys);
                ui.add_space(2.0);
                ui.label(egui::RichText::new(what).font(theme::small()).color(color::TEXT_FAINT));
                ui.add_space(space::L);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(space::XL);
                ui.label(egui::RichText::new(format!("{n} commands")).font(theme::small()).color(color::TEXT_FAINT));
            });
        });
    });
    if resp.should_close() || chosen.is_some() {
        palette.close();
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(titles: &[(&'static str, &str)]) -> Vec<Command<usize>> {
        titles.iter().enumerate().map(|(i, (g, t))| Command::new(g, Icon::Play, *t, i)).collect()
    }

    #[test]
    fn scores_need_every_letter_in_order() {
        assert!(score("ply", "Play or pause").is_some());
        assert!(score("yalp", "Play or pause").is_none());
        assert!(score("", "anything").is_some());
        assert!(score("PLAY", "play").is_some(), "case does not matter");
        assert!(score("add glow", "Add effect glow").is_some(), "spaces are ignored");
    }

    #[test]
    fn prefixes_and_word_starts_win() {
        let prefix = score("exp", "Export video…").unwrap();
        let inside = score("exp", "Add effect exposure").unwrap();
        assert!(prefix > inside, "{prefix} > {inside}");
        let words = score("ae", "Add effect").unwrap();
        let scattered = score("ae", "Play backwards").unwrap_or(i64::MIN);
        assert!(words > scattered);
    }

    #[test]
    fn ranks_best_first_and_keeps_order_on_ties() {
        let c = cmds(&[("Playback", "Play or pause"), ("Edit", "Undo"), ("Layer", "Duplicate layer"), ("Go to", "Select layer tex"), ("Layer", "Delete layer")]);
        assert_eq!(rank(&c, ""), vec![0, 1, 2, 3, 4], "no query: everything, in order");
        assert_eq!(rank(&c, "undo")[0], 1);
        assert_eq!(rank(&c, "tex"), vec![3]);
        let layer = rank(&c, "layer");
        assert_eq!(layer.len(), 3);
        assert!(rank(&c, "playback").contains(&0), "the group name matches too");
        assert!(rank(&c, "zzz").is_empty());
    }
}
