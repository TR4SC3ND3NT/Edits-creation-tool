//! The look of the window, in one place: colours, type, spacing, radii and
//! sizes as design tokens, the embedded fonts, and [`apply`], which turns
//! them into egui's `Style`. Other modules use these names and never write
//! a colour or a font size of their own.
//!
//! Neutral dark greys carry the interface; one accent (iris) marks
//! selection, focus and the primary action; green, amber and red appear
//! only for status. The preview is the brightest thing on screen.

use eframe::egui::{self, vec2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};
use std::sync::Arc;

/// Colours. Greys after Radix gray (dark), the accent after Radix iris.
pub mod color {
    use eframe::egui::Color32;

    /// Behind the preview picture.
    pub const STAGE: Color32 = Color32::from_rgb(0x08, 0x08, 0x08);
    /// Window and panels.
    pub const BG: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
    /// Headers, popovers, the palette, the timeline's track headers.
    pub const RAISED: Color32 = Color32::from_rgb(0x19, 0x19, 0x19);
    /// Fields and buttons.
    pub const CONTROL: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
    pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x2A);
    pub const CONTROL_ACTIVE: Color32 = Color32::from_rgb(0x31, 0x31, 0x31);
    /// Dividers.
    pub const LINE: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
    /// Outlines that must read against controls.
    pub const LINE_STRONG: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);

    /// Primary text and values.
    pub const TEXT: Color32 = Color32::from_rgb(0xEE, 0xEE, 0xEE);
    /// Labels.
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0xB4, 0xB4, 0xB4);
    /// Units, hints, disabled.
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x7B, 0x7B, 0x7B);

    /// Primary button, focus, active toggle.
    pub const ACCENT: Color32 = Color32::from_rgb(0x6E, 0x6A, 0xDE);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x7D, 0x78, 0xE6);
    /// Accent text and icons on dark.
    pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0xB1, 0xA9, 0xFF);
    /// Selected row and clip fill.
    pub const ACCENT_BG: Color32 = Color32::from_rgb(0x20, 0x22, 0x48);
    /// Text selection, selected toggles.
    pub const ACCENT_SELECT: Color32 = Color32::from_rgb(0x30, 0x33, 0x74);
    /// Selected clip outline.
    pub const ACCENT_LINE: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x95);

    pub const PLAYHEAD: Color32 = Color32::from_rgb(0xF5, 0xF5, 0xF5);

    pub const OK: Color32 = Color32::from_rgb(0x46, 0xA7, 0x58);
    pub const OK_TEXT: Color32 = Color32::from_rgb(0x71, 0xD0, 0x83);
    pub const WARN: Color32 = Color32::from_rgb(0xFF, 0xC5, 0x3D);
    pub const ERROR: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);
    pub const ERROR_TEXT: Color32 = Color32::from_rgb(0xFF, 0x95, 0x92);
    /// Error banner fill.
    pub const ERROR_BG: Color32 = Color32::from_rgb(0x3B, 0x12, 0x19);
    /// Things the agent did (a mark, never a fill). Sky, so it does not read as an adjustment layer.
    pub const AGENT: Color32 = Color32::from_rgb(0x7C, 0xE2, 0xFE);

    /// Markers on the timeline.
    pub const MARKER: Color32 = WARN;
    /// The waveform, and its part left of the playhead.
    pub const WAVE: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);
    pub const WAVE_PLAYED: Color32 = Color32::from_rgb(0x8E, 0x8E, 0x8E);
    /// The song's marks on the timeline: kick onsets (orange), snare onsets
    /// (pink), downbeats (a light grey line), sections (alternating bands).
    pub const SONG_KICK: Color32 = Color32::from_rgb(0xF7, 0x6B, 0x15);
    pub const SONG_SNARE: Color32 = Color32::from_rgb(0xE9, 0x3D, 0x82);
    pub const SONG_BEAT: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);
    pub const SONG_SECTION: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1C);
    pub const SONG_SECTION_ALT: Color32 = Color32::from_rgb(0x24, 0x24, 0x24);
    /// A transition's wedge at the start of a layer's bar.
    pub const TRANSITION: Color32 = ACCENT_TEXT;
    /// Modal backdrop.
    pub const BACKDROP: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 150);
}

/// How a layer kind looks on the timeline: a muted fill and a thin stripe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindColors {
    pub fill: Color32,
    pub stripe: Color32,
}

/// Few hues, by kind: image blue, cube plum, solid grey, adjustment teal,
/// the camera iris.
pub fn kind_colors(kind: &str) -> KindColors {
    let (fill, stripe) = match kind.split(' ').next().unwrap_or("") {
        "image" => (0x0D2847, 0x0090FF),
        "cube" => (0x351A35, 0xAB4ABA),
        "solid" => (0x262626, 0x8B8B8B),
        "adjustment" => (0x0D2D2A, 0x12A594),
        _ => (0x202248, 0x6E6ADE),
    };
    let c = |v: u32| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
    KindColors { fill: c(fill), stripe: c(stripe) }
}

/// Spacing scale, in points.
pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const S: f32 = 6.0;
    pub const M: f32 = 8.0;
    pub const L: f32 = 12.0;
    pub const XL: f32 = 16.0;
    pub const XXL: f32 = 24.0;
    pub const XXXL: f32 = 32.0;
}

/// Corner radii: controls and clips, menus and cards, modals and toasts.
pub mod radius {
    pub const SMALL: u8 = 4;
    pub const NORMAL: u8 = 6;
    pub const LARGE: u8 = 10;
}

/// Type sizes, in points.
pub mod text {
    pub const BODY: f32 = 12.5;
    pub const SMALL: f32 = 11.0;
    /// Section titles ("eyebrows").
    pub const LABEL: f32 = 10.5;
    pub const TITLE: f32 = 13.5;
    pub const HEADLINE: f32 = 17.0;
    /// Numbers in fields and lists.
    pub const MONO: f32 = 11.5;
    pub const TIMECODE: f32 = 15.0;
    /// Icons in toolbars, and in rows.
    pub const ICON: f32 = 16.0;
    pub const ICON_SMALL: f32 = 14.0;
}

/// Heights and widths of the fixed parts of the layout.
pub mod size {
    pub const TOP_BAR: f32 = 40.0;
    pub const TRANSPORT: f32 = 44.0;
    pub const STATUS_BAR: f32 = 24.0;
    pub const RAIL: f32 = 44.0;
    pub const DRAWER: f32 = 296.0;
    pub const INSPECTOR: f32 = 320.0;
    pub const ROW: f32 = 24.0;
    pub const BUTTON: f32 = 26.0;
    pub const ICON_BUTTON: f32 = 28.0;
    pub const TIMELINE_TOOLBAR: f32 = 32.0;
    pub const OVERVIEW: f32 = 18.0;
    pub const RULER: f32 = 22.0;
    pub const MARKERS: f32 = 16.0;
    pub const WAVE: f32 = 40.0;
    /// The lane of the song's marks.
    pub const SONG: f32 = 26.0;
    pub const TRACK: f32 = 28.0;
    pub const TRACK_HEADER: f32 = 168.0;
    pub const PALETTE_WIDTH: f32 = 580.0;
    pub const TOAST_WIDTH: f32 = 340.0;
    /// Inspector: the label column.
    pub const LABEL_COLUMN: f32 = 84.0;
}

// ---- fonts -------------------------------------------------------------------------

const INTER: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");
const MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Variable.ttf");

/// Weights of the variable fonts, as `wght` coordinates. Light text on a
/// dark background looks thinner than it is, so the body is a little
/// heavier than Inter's regular 400.
const BODY_WEIGHT: f32 = 430.0;
const MEDIUM_WEIGHT: f32 = 540.0;
const STRONG_WEIGHT: f32 = 620.0;
const MONO_WEIGHT: f32 = 430.0;
const MONO_MEDIUM_WEIGHT: f32 = 540.0;

/// Inter at medium weight: buttons, titles of rows.
pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}

/// Inter semibold: headings.
pub fn strong() -> FontFamily {
    FontFamily::Name("strong".into())
}

/// JetBrains Mono at medium weight: the timecode.
pub fn mono_medium() -> FontFamily {
    FontFamily::Name("mono-medium".into())
}

/// The Lucide icon font alone.
pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

fn weighted(bytes: &'static [u8], weight: f32) -> Arc<FontData> {
    let mut data = FontData::from_static(bytes);
    data.tweak.coords.push("wght", weight);
    Arc::new(data)
}

/// Inter for the interface, JetBrains Mono for every number, Lucide for
/// icons; egui's own fonts stay behind them for anything they lack.
pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let fallback_prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let fallback_mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    fonts.font_data.insert("inter".into(), weighted(INTER, BODY_WEIGHT));
    fonts.font_data.insert("inter-medium".into(), weighted(INTER, MEDIUM_WEIGHT));
    fonts.font_data.insert("inter-strong".into(), weighted(INTER, STRONG_WEIGHT));
    fonts.font_data.insert("mono".into(), weighted(MONO, MONO_WEIGHT));
    fonts.font_data.insert("mono-medium".into(), weighted(MONO, MONO_MEDIUM_WEIGHT));
    let mut lucide = FontData::from_static(lucide_icons::LUCIDE_FONT_BYTES);
    // Lucide's glyphs sit a little high next to Inter's lowercase.
    lucide.tweak.y_offset_factor = 0.07;
    fonts.font_data.insert("lucide".into(), Arc::new(lucide));

    let family = |first: &[&str], rest: &[String]| -> Vec<String> { first.iter().map(|s| s.to_string()).chain(["lucide".to_string()]).chain(rest.iter().cloned()).collect() };
    fonts.families.insert(FontFamily::Proportional, family(&["inter"], &fallback_prop));
    fonts.families.insert(FontFamily::Monospace, family(&["mono", "inter"], &fallback_mono));
    fonts.families.insert(medium(), family(&["inter-medium"], &fallback_prop));
    fonts.families.insert(strong(), family(&["inter-strong"], &fallback_prop));
    fonts.families.insert(mono_medium(), family(&["mono-medium", "inter"], &fallback_mono));
    fonts.families.insert(icons(), vec!["lucide".to_string()]);
    fonts
}

// ---- text styles -------------------------------------------------------------------

/// Section titles in the inspector and drawers.
pub fn label_style() -> TextStyle {
    TextStyle::Name("label".into())
}

/// The transport's timecode.
pub fn timecode_style() -> TextStyle {
    TextStyle::Name("timecode".into())
}

pub fn body() -> FontId {
    FontId::proportional(text::BODY)
}

pub fn small() -> FontId {
    FontId::proportional(text::SMALL)
}

pub fn small_medium() -> FontId {
    FontId::new(text::SMALL, medium())
}

pub fn mono() -> FontId {
    FontId::monospace(text::MONO)
}

pub fn mono_small() -> FontId {
    FontId::monospace(text::LABEL)
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, icons())
}

// ---- style -------------------------------------------------------------------------

fn widget(fill: Color32, fg: Color32) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals { bg_fill: fill, weak_bg_fill: fill, bg_stroke: Stroke::NONE, corner_radius: CornerRadius::same(radius::SMALL), fg_stroke: Stroke::new(1.0, fg), expansion: 0.0 }
}

/// The style for the dark theme.
pub fn style(mut style: egui::Style) -> egui::Style {
    use color::*;
    style.text_styles = [
        (TextStyle::Small, small()),
        (TextStyle::Body, body()),
        (TextStyle::Button, body()),
        (TextStyle::Heading, FontId::new(text::TITLE, strong())),
        (TextStyle::Monospace, mono()),
        (label_style(), FontId::new(text::LABEL, medium())),
        (timecode_style(), FontId::new(text::TIMECODE, mono_medium())),
    ]
    .into();
    // Every number a value field shows is in the mono font: digits that do not dance.
    style.drag_value_text_style = TextStyle::Monospace;
    style.animation_time = 0.12;

    let s = &mut style.spacing;
    s.item_spacing = vec2(space::M, space::S);
    s.button_padding = vec2(space::M, space::XS);
    s.interact_size = vec2(36.0, 22.0);
    s.window_margin = Margin::same(space::L as i8);
    s.menu_margin = Margin::same(space::S as i8);
    s.indent = space::L;
    s.icon_width = 14.0;
    s.icon_width_inner = 8.0;
    s.icon_spacing = space::S;
    s.combo_height = 280.0;
    s.tooltip_width = 320.0;
    s.menu_width = 220.0;
    s.scroll = egui::style::ScrollStyle::thin();
    s.scroll.bar_width = 6.0;
    s.scroll.floating_allocated_width = 0.0;

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = None;
    v.weak_text_color = Some(TEXT_FAINT);
    v.hyperlink_color = ACCENT_TEXT;
    v.faint_bg_color = RAISED;
    v.extreme_bg_color = STAGE;
    v.text_edit_bg_color = Some(CONTROL);
    v.code_bg_color = CONTROL;
    v.warn_fg_color = WARN;
    v.error_fg_color = ERROR_TEXT;
    v.panel_fill = BG;
    v.window_fill = RAISED;
    v.window_stroke = Stroke::new(1.0, CONTROL_HOVER);
    v.window_corner_radius = CornerRadius::same(radius::LARGE);
    v.menu_corner_radius = CornerRadius::same(radius::NORMAL);
    v.window_shadow = Shadow { offset: [0, 12], blur: 24, spread: 0, color: Color32::from_black_alpha(150) };
    v.popup_shadow = Shadow { offset: [0, 6], blur: 12, spread: 0, color: Color32::from_black_alpha(128) };
    v.window_highlight_topmost = false;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.striped = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.interact_cursor = None;
    v.disabled_alpha = 0.42;
    v.button_frame = true;
    v.selection.bg_fill = ACCENT_SELECT;
    v.selection.stroke = Stroke::new(1.0, ACCENT_TEXT);
    v.text_cursor.stroke = Stroke::new(1.5, ACCENT_TEXT);
    v.widgets.noninteractive = egui::style::WidgetVisuals { bg_stroke: Stroke::new(1.0, LINE), ..widget(BG, TEXT_MUTED) };
    v.widgets.inactive = widget(CONTROL, TEXT);
    v.widgets.hovered = widget(CONTROL_HOVER, TEXT);
    v.widgets.active = widget(CONTROL_ACTIVE, TEXT);
    v.widgets.open = widget(CONTROL_ACTIVE, TEXT);
    // Focused text fields get an accent outline (egui draws it with the active/hovered stroke).
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.text_options.font_hinting = true;
    v.text_options.subpixel_binning = false;
    style
}

/// Fonts and style for the whole window. Call once at start.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|s| *s = style(s.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_the_window_uses_has_fonts() {
        let f = fonts();
        for family in [FontFamily::Proportional, FontFamily::Monospace, medium(), strong(), mono_medium(), icons()] {
            let names = f.families.get(&family).unwrap_or_else(|| panic!("{family:?} is missing"));
            assert!(!names.is_empty());
            for n in names {
                assert!(f.font_data.contains_key(n), "{family:?} names {n}, which is not loaded");
            }
        }
        assert_eq!(f.families[&FontFamily::Proportional][0], "inter");
        assert_eq!(f.families[&FontFamily::Monospace][0], "mono");
    }

    #[test]
    fn the_style_uses_the_tokens() {
        let s = style(egui::Style::default());
        assert_eq!(s.visuals.panel_fill, color::BG);
        assert_eq!(s.visuals.widgets.inactive.weak_bg_fill, color::CONTROL);
        assert_eq!(s.visuals.widgets.inactive.bg_stroke, Stroke::NONE, "no outlines on resting buttons");
        assert_eq!(s.text_styles[&TextStyle::Body].size, text::BODY);
        assert_eq!(s.drag_value_text_style, TextStyle::Monospace);
    }

    #[test]
    fn kinds_have_distinct_stripes() {
        let kinds = ["image a", "cube", "solid", "adjustment", "camera"];
        for (i, a) in kinds.iter().enumerate() {
            for b in &kinds[i + 1..] {
                assert_ne!(kind_colors(a).stripe, kind_colors(b).stripe, "{a} and {b}");
            }
        }
    }
}
