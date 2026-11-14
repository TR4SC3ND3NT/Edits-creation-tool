//! The vision pass: frames of every shot for a model to look at, a fixed
//! questionnaire, and the validated answers merged into the analysis.
//!
//! The signals measure what has a number (cut times, gain, blur radius,
//! scale); they cannot say *what kind* of thing a move is: a cube or a flat
//! card, a red glow around a card, a paper texture, a duotone. A model
//! looking at the frames can, as long as it is asked closed questions in
//! the engine's own vocabulary and is never asked for speeds or frame
//! counts. So:
//!
//! 1. [`export`] writes, per shot, a labelled strip (the last frame of the
//!    previous shot, the cut, cut+1, cut+2, the reference frame, the last
//!    frame, plus a second row with every frame around a flash or tone hit),
//!    a detail image of two crops at full resolution (centre and left edge, for
//!    textures, borders and glows), an overview of all shots, `questions.json`
//!    (the questions Q1-Q8 with their fixed words, the tiles and what the
//!    signals measured) and `answers.json`, a template filled with `unsure`.
//! 2. The agent looks at the images and fills `answers.json`.
//! 3. [`merge`] validates the answers against the vocabulary (an unknown
//!    word is an error that lists the valid ones), checks each label against
//!    the signals (an inversion needs a negative correlation, a white flash an
//!    exposure peak, a punch a scale change) and writes the labels into the
//!    analysis as `shots[].vision` and `vision`. `draft` then uses them to
//!    pick layer kinds, effects and presets; the measured numbers stay the
//!    authority for amounts and times.

use crate::contact::{self, Cell};
use crate::media;
use crate::schema::*;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A frame of a strip and what it shows.
type TileSpec = (usize, String);
/// A colour named by an answer: (key, sRGB).
type NamedColor = (String, [f64; 3]);
/// Labels per shot and for the whole edit.
pub type Labels = (BTreeMap<usize, ShotVision>, Option<GlobalVision>);

pub const QUESTIONS_FORMAT: &str = "edits.vision_questions/1";
pub const ANSWERS_FORMAT: &str = "edits.vision_answers/1";

/// What an answer word may carry after a colon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arg {
    None,
    /// left, right, up, down
    Dir,
    /// x, y, z
    Axis,
    /// Degrees.
    Angle,
    /// A colour: a name from [`COLORS`] or `#rrggbb`.
    Color,
    /// Two colours, dark then light: `navy/yellow`.
    TwoColors,
}

/// One question of the questionnaire.
pub struct Question {
    pub id: &'static str,
    /// Field of the answer (and of `ShotVision`).
    pub field: &'static str,
    pub ask: &'static str,
    pub words: &'static [&'static str],
    /// Several words allowed (a list) or exactly one.
    pub multi: bool,
}

/// The per-shot questions, after docs/EDIT_LOGIC.md section 4. Every
/// question also accepts `unsure`; a list question accepts `none`.
pub const QUESTIONS: &[Question] = &[
    Question { id: "Q1", field: "picture", ask: "What is the picture?", words: &edits_core::SLOT_ROLES, multi: false },
    Question {
        id: "Q2",
        field: "framing",
        ask: "How is it framed? cube = a 3D box with pictures on its faces; tilted_card = a flat picture turned in 3D with background around it; inset_card = a smaller flat picture on a background",
        words: &["full_frame", "inset_card", "tilted_card", "cube", "split", "picture_in_picture"],
        multi: false,
    },
    Question { id: "Q2b", field: "border", ask: "Border around the picture or card (glow:<colour> for a coloured glow)", words: &["none", "thin_light", "thin_dark", "glow"], multi: false },
    Question {
        id: "Q3",
        field: "tone",
        ask: "Tone treatments seen in any tile (duotone:<dark>/<light>, tinted:<colour>; burnt_white = flashed until only ink survives)",
        words: &["normal", "inverted", "threshold", "posterized", "duotone", "tinted", "desaturated", "burnt_white"],
        multi: true,
    },
    Question { id: "Q4", field: "light", ask: "Light effects", words: &["white_flash", "black_flash", "glow", "light_rays", "flare"], multi: true },
    Question {
        id: "Q5",
        field: "motion",
        ask: "Kind of motion only, never speed (slide:<left|right|up|down>, whip:<dir>, 3d_turn:<x|y|z>)",
        words: &["static", "punch_in", "zoom_out", "push_in", "shake", "slide", "spin", "3d_turn", "whip"],
        multi: true,
    },
    Question { id: "Q6", field: "blur", ask: "Kind of blur (directional:<angle in degrees, 0 = horizontal>)", words: &["none", "gaussian", "directional", "zoom", "spin"], multi: false },
    Question {
        id: "Q7",
        field: "overlays",
        ask: "Things over the picture (sfx_in_art = lettering drawn in the art itself)",
        words: &["paper", "handwriting", "grain", "scratches", "speed_lines", "focus_lines", "screentone", "particles", "caption", "sfx_in_art"],
        multi: true,
    },
    Question { id: "Q8", field: "entry", ask: "How does the shot come in?", words: &["hard_cut", "flash_cut", "dissolve", "whip", "zoom_through", "cube", "push", "glitch"], multi: false },
];

/// Questions about the whole edit, answered from the overview and the detail crops.
pub const GLOBAL_QUESTIONS: &[Question] = &[
    Question { id: "G1", field: "overlays", ask: "A static layer over every shot (it stays put while the pictures change)", words: &["paper", "handwriting", "grain", "scratches", "screentone", "dust", "light_leak"], multi: true },
    Question { id: "G2", field: "overlay_tone", ask: "Is that layer lighter than the pictures, darker, or both (light on dark frames, dark on light ones)?", words: &["lighter", "darker", "both"], multi: false },
    Question { id: "G3", field: "vignette", ask: "Dark corners on every frame", words: &["none", "light", "medium", "strong"], multi: false },
];

/// Named colours accepted in answers, sRGB 0..1.
pub const COLORS: &[(&str, [f64; 3])] = &[
    ("white", [1.0, 1.0, 1.0]),
    ("black", [0.0, 0.0, 0.0]),
    ("grey", [0.5, 0.5, 0.5]),
    ("red", [0.9, 0.1, 0.1]),
    ("dark_red", [0.45, 0.04, 0.04]),
    ("orange", [1.0, 0.5, 0.1]),
    ("yellow", [1.0, 0.9, 0.15]),
    ("gold", [0.85, 0.65, 0.15]),
    ("green", [0.15, 0.75, 0.25]),
    ("dark_green", [0.05, 0.3, 0.1]),
    ("cyan", [0.1, 0.85, 0.9]),
    ("teal", [0.05, 0.5, 0.5]),
    ("blue", [0.15, 0.35, 0.95]),
    ("navy", [0.05, 0.07, 0.3]),
    ("purple", [0.5, 0.2, 0.7]),
    ("magenta", [0.95, 0.15, 0.75]),
    ("pink", [1.0, 0.55, 0.75]),
    ("brown", [0.4, 0.25, 0.12]),
    ("cream", [0.98, 0.94, 0.82]),
    ("sepia", [0.62, 0.48, 0.32]),
];

/// What a word may carry after a colon.
pub fn arg_of(field: &str, word: &str) -> Arg {
    match (field, word) {
        ("motion", "slide" | "whip") => Arg::Dir,
        ("motion", "3d_turn") => Arg::Axis,
        ("blur", "directional") => Arg::Angle,
        ("border", "glow") | ("tone", "tinted") => Arg::Color,
        ("tone", "duotone") => Arg::TwoColors,
        _ => Arg::None,
    }
}

/// `red`, `dark_red` or `#rrggbb` as sRGB 0..1.
pub fn parse_color(s: &str) -> Option<[f64; 3]> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            let v = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|b| b as f64 / 255.0);
            return Some([v(0)?, v(2)?, v(4)?]);
        }
        return None;
    }
    COLORS.iter().find(|(n, _)| *n == s.replace(['-', ' '], "_")).map(|(_, c)| *c)
}

fn color_names() -> String {
    COLORS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
}

/// One answer word checked against its question: the bare word and, for
/// words with an argument, the colours it names (key, colour).
fn check_word(q: &Question, raw: &str) -> std::result::Result<(String, Vec<NamedColor>), String> {
    let raw = raw.trim();
    let (word, arg) = match raw.split_once(':') {
        Some((w, a)) => (w.trim().to_ascii_lowercase(), Some(a.trim())),
        None => (raw.to_ascii_lowercase(), None),
    };
    if word == "unsure" || (word == "none" && (q.multi || q.words.contains(&"none"))) {
        return Ok((word, vec![]));
    }
    if !q.words.contains(&word.as_str()) {
        return Err(format!("\"{raw}\" is not a {} word; use one of: {}, unsure{}", q.id, q.words.join(", "), if q.multi { ", none" } else { "" }));
    }
    let kind = arg_of(q.field, &word);
    let mut colors = Vec::new();
    let normal = match (kind, arg) {
        (_, None) => word.clone(),
        (Arg::None, Some(a)) => return Err(format!("\"{word}\" takes no argument (got \"{a}\")")),
        (Arg::Dir, Some(a)) => {
            let a = a.to_ascii_lowercase();
            if !["left", "right", "up", "down"].contains(&a.as_str()) {
                return Err(format!("\"{word}:{a}\": the direction is left, right, up or down"));
            }
            format!("{word}:{a}")
        }
        (Arg::Axis, Some(a)) => {
            let a = a.to_ascii_lowercase();
            if !["x", "y", "z"].contains(&a.as_str()) {
                return Err(format!("\"{word}:{a}\": the axis is x, y or z"));
            }
            format!("{word}:{a}")
        }
        (Arg::Angle, Some(a)) => {
            let v: f64 = a.trim_end_matches("deg").trim().parse().map_err(|_| format!("\"{word}:{a}\": the angle is a number of degrees"))?;
            format!("{word}:{}", v.rem_euclid(180.0).round())
        }
        (Arg::Color, Some(a)) => {
            let c = parse_color(a).ok_or_else(|| format!("\"{word}:{a}\": unknown colour; use #rrggbb or one of {}", color_names()))?;
            colors.push((if word == "glow" { "glow" } else { "tint" }.to_string(), c));
            word.clone()
        }
        (Arg::TwoColors, Some(a)) => {
            let (d, l) = a.split_once('/').ok_or_else(|| format!("\"{word}:{a}\": give two colours, dark then light, as dark/light"))?;
            let cd = parse_color(d).ok_or_else(|| format!("\"{d}\": unknown colour; use #rrggbb or one of {}", color_names()))?;
            let cl = parse_color(l).ok_or_else(|| format!("\"{l}\": unknown colour; use #rrggbb or one of {}", color_names()))?;
            colors.push(("shadows".into(), cd));
            colors.push(("highlights".into(), cl));
            word.clone()
        }
    };
    Ok((normal, colors))
}

// ---- export -----------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ExportOpts {
    /// The reference video (default: the analysis's `source.path`).
    pub video: Option<PathBuf>,
    /// Output folder (default: `vision/` next to the analysis).
    pub out_dir: Option<PathBuf>,
    /// Height of a strip tile in pixels.
    pub tile_height: u32,
    /// Only these shots (default: all).
    pub shots: Vec<usize>,
}

impl Default for ExportOpts {
    fn default() -> Self {
        Self { video: None, out_dir: None, tile_height: 180, shots: vec![] }
    }
}

/// One tile of a shot's strip.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tile {
    /// 1-based, as labelled on the image.
    pub tile: usize,
    pub frame: usize,
    pub beat: f64,
    /// prev, cut, cut+1, cut+2, ref, last, or the event shown (flash, invert...).
    pub what: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShotSheet {
    pub shot: usize,
    pub frames: [usize; 2],
    pub beats: [f64; 2],
    pub status: String,
    /// Image paths relative to the output folder.
    pub strip: String,
    pub detail: String,
    pub tiles: Vec<Tile>,
    /// What the signals measured, in short sentences: confirm or contradict them.
    pub signals: Vec<String>,
}

/// What [`export`] wrote.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Exported {
    pub out_dir: String,
    pub questions: String,
    pub answers: String,
    pub overview: String,
    pub shots: Vec<ShotSheet>,
    /// Images written, and their total size in pixels (a cost hint).
    pub images: usize,
    pub megapixels: f64,
}

fn beat_of(a: &Analysis, f: usize) -> f64 {
    round_to(a.beats.beat_of_frame(f as f64, a.source.fps), 2)
}

/// Tiles of one shot: the fixed six, then every frame around its events.
fn tiles_for(a: &Analysis, s: &Shot) -> (Vec<TileSpec>, Vec<TileSpec>) {
    let last = s.f1 - 1;
    let mut main: Vec<(usize, String)> = Vec::new();
    let push = |v: &mut Vec<(usize, String)>, f: usize, what: &str| {
        if !v.iter().any(|(g, _)| *g == f) {
            v.push((f, what.to_string()));
        }
    };
    if s.f0 > 0 && s.f0 > a.ingest.content_f0 {
        push(&mut main, s.f0 - 1, "prev");
    }
    push(&mut main, s.f0, "cut");
    for (k, what) in [(1, "cut+1"), (2, "cut+2")] {
        if s.f0 + k < last {
            push(&mut main, s.f0 + k, what);
        }
    }
    push(&mut main, s.ref_frame, "ref");
    push(&mut main, last, "last");
    main.sort_by_key(|t| t.0);
    // Every frame around the strongest events: one-frame hits are invisible otherwise.
    let mut events: Vec<&Event> = s.events.iter().filter(|e| e.kind != "fade").collect();
    events.sort_by(|x, y| {
        let w = |e: &Event| if e.kind == "flash" { e.params.get("peak").map(|p| p.value).unwrap_or(0.0) } else { 10.0 };
        w(y).total_cmp(&w(x))
    });
    let mut extra: Vec<(usize, String)> = Vec::new();
    for e in events {
        let (from, to) = if e.kind == "flash" { (e.frame.saturating_sub(1), e.frame + 3) } else { (e.f0.saturating_sub(1), (e.f1 + 1).min(e.f0 + 5)) };
        for f in from.max(s.f0)..=to.min(last) {
            if extra.len() >= 8 {
                break;
            }
            if !extra.iter().any(|(g, _)| *g == f) && !main.iter().any(|(g, _)| *g == f) {
                let what = if f == e.frame { e.kind.clone() } else if f < e.frame { format!("{}-{}", e.kind, e.frame - f) } else { format!("{}+{}", e.kind, f - e.frame) };
                extra.push((f, what));
            }
        }
    }
    if s.status == Ident::NeedsAgent && extra.len() < 4 {
        // Not tracked: show the motion every other frame.
        let mut f = s.f0 + 1;
        while f < last && extra.len() < 8 {
            if !main.iter().any(|(g, _)| *g == f) && !extra.iter().any(|(g, _)| *g == f) {
                extra.push((f, "move".into()));
            }
            f += 2;
        }
    }
    extra.sort_by_key(|t| t.0);
    (main, extra)
}

/// Short sentences of what the signals measured in a shot.
pub fn signal_lines(a: &Analysis, s: &Shot) -> Vec<String> {
    let mut out = Vec::new();
    if s.status == Ident::NeedsAgent {
        out.push(format!("not tracked (needs_agent): {}", s.notes.first().cloned().unwrap_or_default()));
    }
    let flashes: Vec<String> = s.events.iter().filter(|e| e.kind == "flash").map(|e| format!("frame {} {:.1} stops", e.frame, e.params.get("peak").map(|p| p.value).unwrap_or(0.0))).collect();
    if !flashes.is_empty() {
        out.push(format!("exposure flashes: {}", flashes.join(", ")));
    }
    for e in s.events.iter().filter(|e| !matches!(e.kind.as_str(), "flash" | "fade")) {
        let mut t = format!("{} frames {}-{}", e.kind, e.f0, e.f1);
        if let Some(v) = e.params.get("amount").or(e.params.get("levels")).or(e.params.get("level")) {
            t += &format!(" ({} {})", round_to(v.value, 2), v.unit);
        }
        for (k, c) in &e.colors {
            t += &format!(" {k} #{:02x}{:02x}{:02x}", (c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8);
        }
        out.push(t);
    }
    let range = |prop: &str| -> Option<(f64, f64)> {
        let c = s.curves.iter().find(|c| c.prop == prop)?;
        let v: Vec<f64> = c.keys.iter().map(|k| k.v[0]).collect();
        Some((v.iter().copied().fold(f64::MAX, f64::min), v.iter().copied().fold(f64::MIN, f64::max)))
    };
    if let Some((lo, hi)) = range("scale") {
        out.push(format!("scale {:.2}x to {:.2}x of the reference frame", lo, hi));
    }
    if let Some((lo, hi)) = range("rotation") {
        if hi - lo > 1.0 {
            out.push(format!("rotation {:.1} to {:.1} degrees", lo, hi));
        }
    }
    if let Some(sh) = &s.shake {
        out.push(format!("shake residual {:.2}% / {:.2}% of height (x / y)", 100.0 * sh.rms_x.value, 100.0 * sh.rms_y.value));
    }
    if let Some((_, hi)) = range("gaussian_blur.radius") {
        let kind = s.blur_kind.as_ref().map(|k| format!(", looks {}{} (confidence {:.2})", k.kind, k.angle.map(|v| format!(" at {v:.0} deg")).unwrap_or_default(), k.confidence)).unwrap_or_default();
        out.push(format!("blur up to {:.2}% of height{kind}", hi));
    }
    let _ = a;
    out
}

/// Write strips, detail crops, the overview, `questions.json` and `answers.json`.
pub fn export(a: &Analysis, analysis_path: &Path, opts: &ExportOpts) -> Result<Exported> {
    let video = opts.video.clone().unwrap_or_else(|| PathBuf::from(&a.source.path));
    let info = media::probe(&video).with_context(|| format!("the reference video {} (pass --video if it moved)", video.display()))?;
    let analysis_dir = analysis_path.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let out = opts.out_dir.clone().unwrap_or_else(|| analysis_dir.join("vision"));
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    let wanted: Vec<&Shot> = a.shots.iter().filter(|s| opts.shots.is_empty() || opts.shots.contains(&s.id)).collect();
    if wanted.is_empty() {
        bail!("no shot to export: the analysis has shots 1..{}", a.shots.len());
    }
    // Frames to decode at tile size, and the reference frames at full size.
    let mut plan: Vec<(usize, Vec<TileSpec>, Vec<TileSpec>)> = Vec::new();
    let mut need: BTreeSet<usize> = BTreeSet::new();
    for s in &wanted {
        let (m, e) = tiles_for(a, s);
        need.extend(m.iter().map(|t| t.0));
        need.extend(e.iter().map(|t| t.0));
        plan.push((s.id, m, e));
    }
    for s in &a.shots {
        need.insert(s.ref_frame);
    }
    let need: Vec<usize> = need.into_iter().filter(|f| *f < a.source.frames.max(1)).collect();
    let th = opts.tile_height.clamp(64, 720).min(info.height);
    let small = media::decode_selected_at(&video, &info, &need, Some(th))?;
    anyhow::ensure!(small.len() == need.len(), "ffmpeg returned {} of the {} frames asked for from {}", small.len(), need.len(), video.display());
    let at = |f: usize| need.binary_search(&f).ok();
    let refs: Vec<usize> = {
        let mut r: Vec<usize> = wanted.iter().map(|s| s.ref_frame).collect();
        r.sort_unstable();
        r.dedup();
        r
    };
    let full = media::decode_selected(&video, &info, &refs)?;
    anyhow::ensure!(full.len() == refs.len(), "ffmpeg returned {} of the {} reference frames from {}", full.len(), refs.len(), video.display());

    let mut pixels = 0u64;
    let mut images = 0usize;
    let mut sheets = Vec::new();
    for (id, main, extra) in &plan {
        let s = &a.shots[id - 1];
        let mut tiles = Vec::new();
        let mut cells = Vec::new();
        for (f, what) in main.iter().chain(extra.iter()) {
            let Some(i) = at(*f) else { continue };
            let n = tiles.len() + 1;
            tiles.push(Tile { tile: n, frame: *f, beat: beat_of(a, *f), what: what.clone() });
            let accent = if what == "prev" { Some([90, 90, 96]) } else if what == "ref" { Some([60, 150, 230]) } else if main.iter().any(|(g, _)| g == f) { Some([70, 200, 110]) } else { Some([235, 170, 40]) };
            cells.push(Cell { image: contact::frame_image(&small, i), label: format!("{n} F{f} B{:.2} {}", beat_of(a, *f), what.to_uppercase()), accent });
        }
        let cols = main.len().max(extra.len()).max(1);
        let title = format!("SHOT {} FRAMES {}-{} BEATS {:.2}-{:.2}{}", s.id, s.f0, s.f1, s.beat0, s.beat1, if s.status == Ident::NeedsAgent { " NOT TRACKED" } else { "" });
        // Main tiles on the first row, event tiles on the second.
        let mut rows: Vec<Cell> = Vec::new();
        let nm = main.iter().filter(|(f, _)| at(*f).is_some()).count();
        for (k, c) in cells.into_iter().enumerate() {
            rows.push(c);
            if k + 1 == nm {
                while !rows.len().is_multiple_of(cols) {
                    rows.push(Cell { image: image::RgbImage::from_pixel(1, th, image::Rgb(contact::BACKGROUND)), label: String::new(), accent: Some(contact::BACKGROUND) });
                }
            }
        }
        let strip = contact::compose(&rows, cols, 4, Some(&title));
        let strip_name = format!("shot{:02}.jpg", s.id);
        contact::save(&strip, &out.join(&strip_name))?;
        pixels += strip.width() as u64 * strip.height() as u64;
        // Detail: centre and left edge at full resolution (enlarged when the video is small).
        let ri = refs.binary_search(&s.ref_frame).unwrap();
        let fimg = contact::frame_image(&full, ri);
        let side = (fimg.height() / 3).clamp(64, 400);
        let up = |c: image::RgbImage| if c.height() < 300 { contact::fit_height(&c, c.height() * 2) } else { c };
        let (w, h) = (fimg.width() as i64, fimg.height() as i64);
        let centre = up(contact::crop(&fimg, w / 2 - side as i64 / 2, h / 2 - side as i64 / 2, side));
        // The left edge, where static overlays (handwriting, ornaments) and borders sit.
        let corner = up(contact::crop(&fimg, w / 50, h / 20, side));
        let detail = contact::compose(
            &[Cell { image: centre, label: format!("F{} CENTRE 1:1", s.ref_frame), accent: None }, Cell { image: corner, label: format!("F{} EDGE 1:1", s.ref_frame), accent: None }],
            2,
            4,
            Some(&format!("SHOT {} DETAIL", s.id)),
        );
        let detail_name = format!("shot{:02}_detail.jpg", s.id);
        contact::save(&detail, &out.join(&detail_name))?;
        pixels += detail.width() as u64 * detail.height() as u64;
        images += 2;
        sheets.push(ShotSheet {
            shot: s.id,
            frames: [s.f0, s.f1],
            beats: [round_to(s.beat0, 2), round_to(s.beat1, 2)],
            status: if s.status == Ident::NeedsAgent { "needs_agent".into() } else { "measured".into() },
            strip: strip_name,
            detail: detail_name,
            tiles,
            signals: signal_lines(a, s),
        });
    }
    // Overview: the reference frame of every shot.
    let oh = (th * 3 / 4).max(96);
    let cells: Vec<Cell> = a
        .shots
        .iter()
        .filter_map(|s| at(s.ref_frame).map(|i| Cell { image: contact::fit_height(&contact::frame_image(&small, i), oh), label: format!("S{} B{:.1}", s.id, s.beat0), accent: None }))
        .collect();
    let cols = ((cells.len() as f64).sqrt().ceil() as usize).max(1);
    let overview = contact::compose(&cells, cols, 4, Some("ALL SHOTS: REFERENCE FRAMES"));
    contact::save(&overview, &out.join("overview.jpg"))?;
    pixels += overview.width() as u64 * overview.height() as u64;
    images += 1;

    let rel_analysis = crate::draft::relative_path(&out, analysis_path);
    let qs = |list: &[Question]| -> Vec<Value> {
        list.iter()
            .map(|q| {
                let mut words: Vec<String> = q.words.iter().map(|w| match arg_of(q.field, w) {
                    Arg::None => w.to_string(),
                    Arg::Dir => format!("{w}:<left|right|up|down>"),
                    Arg::Axis => format!("{w}:<x|y|z>"),
                    Arg::Angle => format!("{w}[:<degrees>]"),
                    Arg::Color => format!("{w}[:<colour>]"),
                    Arg::TwoColors => format!("{w}[:<dark>/<light>]"),
                }).collect();
                words.push("unsure".into());
                if q.multi {
                    words.push("none".into());
                }
                json!({ "id": q.id, "field": q.field, "ask": q.ask, "answer": if q.multi { "a list of words" } else { "one word" }, "words": words })
            })
            .collect()
    };
    let questions = json!({
        "format": QUESTIONS_FORMAT,
        "analysis": rel_analysis,
        "video": video.to_string_lossy(),
        "how": "Look at overview.jpg, then each shot's strip (tiles numbered on the image; the second row is every frame around a flash or tone hit) and detail (centre and left-edge crops at full resolution). Answer only with the listed words; a colour is a name from `colors` or #rrggbb. Say the KIND of thing, never speeds or frame counts: the signals have the numbers. When unsure, answer unsure. Put tile numbers that show a label in `tiles` ({\"inverted\": [5, 6]}). Fill answers.json, then run `edits vision merge`.",
        "questions": qs(QUESTIONS),
        "global_questions": qs(GLOBAL_QUESTIONS),
        "colors": COLORS.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        "overview": "overview.jpg",
        "global_signals": global_signal_lines(a),
        "shots": sheets,
    });
    std::fs::write(out.join("questions.json"), crate::jsonfmt::to_string(&questions)?)?;
    let answers_path = out.join("answers.json");
    if !answers_path.exists() {
        std::fs::write(&answers_path, crate::jsonfmt::to_string(&template(a, &wanted))?)?;
    }
    Ok(Exported {
        out_dir: out.to_string_lossy().into_owned(),
        questions: out.join("questions.json").to_string_lossy().into_owned(),
        answers: answers_path.to_string_lossy().into_owned(),
        overview: out.join("overview.jpg").to_string_lossy().into_owned(),
        shots: questions["shots"].as_array().map(|v| v.iter().filter_map(|s| serde_json::from_value(s.clone()).ok()).collect()).unwrap_or_default(),
        images,
        megapixels: round_to(pixels as f64 / 1e6, 2),
    })
}

/// Plain-text account of an export: what to open, in which order.
pub fn export_summary(e: &Exported) -> String {
    let mut s = format!("wrote {} shot strip(s) and detail crop(s), an overview and the questionnaire to {} ({} images, {:.1} megapixels)\n", e.shots.len(), e.out_dir, e.images, e.megapixels);
    s += &format!("1. look at {}\n", e.overview);
    s += "2. for each shot, look at its strip (and its detail crop for textures, borders and glows):\n";
    for sh in &e.shots {
        s += &format!("   shot {:2} frames {}-{} {:11} {}  {}\n", sh.shot, sh.frames[0], sh.frames[1], sh.status, sh.strip, sh.signals.join("; "));
    }
    s += &format!("3. answer the questions in {} by editing {} (fixed words only; unsure when in doubt)\n", e.questions, e.answers);
    s += "4. edits vision merge <analysis> <answers.json>, then draft\n";
    s
}

fn global_signal_lines(a: &Analysis) -> Vec<String> {
    let mut out = Vec::new();
    match &a.layers.overlay {
        Some(o) => out.push(format!("bright static strokes over {:.1}% of the frame (overlay.png)", 100.0 * o.coverage.value)),
        None => out.push("no bright static strokes found".into()),
    }
    if let Some(t) = &a.layers.texture {
        out.push(format!("static fine texture: presence {:.2}, strength {:.3} stops", t.presence.value, t.strength.value));
    }
    match &a.layers.vignette {
        Some(v) => out.push(format!("vignette: corners at {:.0}% of the centre's brightness ({})", 100.0 * v.corner_ratio.value, if v.amount.identifiability == Ident::Measured { "measured on flash frames" } else { "rough" })),
        None => out.push("no vignette measured".into()),
    }
    out
}

/// The answers file with every question answered `unsure`.
pub fn template(a: &Analysis, shots: &[&Shot]) -> Value {
    let mut g = Map::new();
    for q in GLOBAL_QUESTIONS {
        g.insert(q.field.into(), if q.multi { json!(["unsure"]) } else { json!("unsure") });
    }
    g.insert("notes".into(), json!(""));
    let shots: Vec<Value> = shots
        .iter()
        .map(|s| {
            let mut m = Map::new();
            m.insert("shot".into(), json!(s.id));
            for q in QUESTIONS {
                m.insert(q.field.into(), if q.multi { json!(["unsure"]) } else { json!("unsure") });
            }
            m.insert("character".into(), Value::Null);
            m.insert("focus".into(), Value::Null);
            m.insert("tiles".into(), json!({}));
            m.insert("notes".into(), json!(""));
            Value::Object(m)
        })
        .collect();
    let _ = a;
    json!({ "format": ANSWERS_FORMAT, "global": g, "shots": shots })
}

// ---- merge ------------------------------------------------------------------------

/// What [`merge`] did.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MergeReport {
    pub labelled: Vec<usize>,
    /// Shots whose answers are all `unsure`.
    pub unanswered: Vec<usize>,
    /// Labels the signals do not confirm: look again at every frame of that span.
    pub disagreements: Vec<String>,
    pub agreements: usize,
    /// Slot roles taken from the picture labels.
    pub roles: BTreeMap<String, String>,
}

fn words(v: &Value, q: &Question, path: &str, errors: &mut Vec<String>, colors: &mut BTreeMap<String, [f64; 3]>) -> Vec<String> {
    let items: Vec<&Value> = match v {
        Value::Null => vec![],
        Value::Array(a) if q.multi => a.iter().collect(),
        Value::String(_) if q.multi => vec![v],
        Value::String(_) => vec![v],
        _ => {
            errors.push(format!("{path}: give {}", if q.multi { "a list of words" } else { "one word" }));
            return vec![];
        }
    };
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let Some(s) = it.as_str() else {
            errors.push(format!("{path}[{i}]: a word is a string"));
            continue;
        };
        match check_word(q, s) {
            Ok((w, cs)) => {
                colors.extend(cs);
                if !out.contains(&w) {
                    out.push(w);
                }
            }
            Err(e) => errors.push(format!("{path}: {e}")),
        }
    }
    if q.multi {
        // `none` and `unsure` mean an empty list; keep `unsure` only alone.
        let real: Vec<String> = out.iter().filter(|w| *w != "none" && *w != "unsure").cloned().collect();
        if real.is_empty() && out.iter().any(|w| w == "unsure") {
            return vec!["unsure".into()];
        }
        real
    } else {
        if out.len() > 1 {
            errors.push(format!("{path}: one word only, got {}", out.join(", ")));
        }
        out.into_iter().take(1).collect()
    }
}

/// Validate an answers document and turn it into labels per shot and global labels.
pub fn parse_answers(a: &Analysis, doc: &Value) -> std::result::Result<Labels, Vec<String>> {
    let mut errors = Vec::new();
    if doc.get("format").and_then(Value::as_str) != Some(ANSWERS_FORMAT) {
        errors.push(format!("format: expected \"{ANSWERS_FORMAT}\" (the answers.json written by `vision export`)"));
    }
    let mut out = BTreeMap::new();
    let known_fields: BTreeSet<&str> = QUESTIONS.iter().map(|q| q.field).chain(["shot", "character", "focus", "tiles", "notes"]).collect();
    for (k, sv) in doc.get("shots").and_then(Value::as_array).cloned().unwrap_or_default().iter().enumerate() {
        let path = format!("shots[{k}]");
        let Some(id) = sv.get("shot").and_then(Value::as_u64).map(|v| v as usize) else {
            errors.push(format!("{path}.shot: the shot number is missing"));
            continue;
        };
        if id == 0 || id > a.shots.len() {
            errors.push(format!("{path}.shot: no shot {id}; the analysis has shots 1..{}", a.shots.len()));
            continue;
        }
        if let Some(obj) = sv.as_object() {
            for key in obj.keys() {
                if !known_fields.contains(key.as_str()) {
                    errors.push(format!("{path}.{key}: unknown field; fields: {}", known_fields.iter().copied().collect::<Vec<_>>().join(", ")));
                }
            }
        }
        let mut colors = BTreeMap::new();
        let mut get = |q: &Question| words(sv.get(q.field).unwrap_or(&Value::Null), q, &format!("{path}.{}", q.field), &mut errors, &mut colors);
        let one = |v: Vec<String>| v.into_iter().next().unwrap_or_else(|| "unsure".into());
        let q = |f: &str| QUESTIONS.iter().find(|q| q.field == f).unwrap();
        let mut v = ShotVision {
            picture: one(get(q("picture"))),
            framing: one(get(q("framing"))),
            border: one(get(q("border"))),
            tone: get(q("tone")),
            light: get(q("light")),
            motion: get(q("motion")),
            blur: one(get(q("blur"))),
            overlays: get(q("overlays")),
            entry: one(get(q("entry"))),
            ..Default::default()
        };
        v.colors = colors;
        v.character = sv.get("character").and_then(Value::as_str).map(str::to_string).filter(|s| !s.is_empty());
        v.notes = sv.get("notes").and_then(Value::as_str).unwrap_or_default().to_string();
        match sv.get("focus") {
            None | Some(Value::Null) => {}
            Some(f) => match serde_json::from_value::<[f64; 2]>(f.clone()) {
                Ok(p) if p.iter().all(|c| (0.0..=1.0).contains(c)) => v.focus = Some(p),
                _ => errors.push(format!("{path}.focus: give [x, y] with both in 0..1 (from the top left of the picture)")),
            },
        }
        if let Some(t) = sv.get("tiles").and_then(Value::as_object) {
            for (label, list) in t {
                match serde_json::from_value::<Vec<usize>>(list.clone()) {
                    Ok(l) => {
                        v.tiles.insert(label.clone(), l);
                    }
                    Err(_) => errors.push(format!("{path}.tiles.{label}: a list of tile numbers")),
                }
            }
        }
        if out.insert(id, v).is_some() {
            errors.push(format!("{path}: shot {id} is answered twice"));
        }
    }
    let global = match doc.get("global") {
        None | Some(Value::Null) => None,
        Some(g) => {
            let mut colors = BTreeMap::new();
            let mut get = |f: &str| {
                let q = GLOBAL_QUESTIONS.iter().find(|q| q.field == f).unwrap();
                words(g.get(f).unwrap_or(&Value::Null), q, &format!("global.{f}"), &mut errors, &mut colors)
            };
            let one = |v: Vec<String>| v.into_iter().next().filter(|w| w != "unsure");
            Some(GlobalVision {
                overlays: get("overlays").into_iter().filter(|w| w != "unsure").collect(),
                overlay_tone: one(get("overlay_tone")),
                vignette: one(get("vignette")),
                checks: vec![],
                notes: g.get("notes").and_then(Value::as_str).unwrap_or_default().to_string(),
            })
        }
    };
    if errors.is_empty() {
        Ok((out, global))
    } else {
        Err(errors)
    }
}

fn unsure(v: &ShotVision) -> bool {
    let all = |l: &[String]| l.is_empty() || l == ["unsure"];
    [&v.picture, &v.framing, &v.border, &v.blur, &v.entry].iter().all(|w| *w == "unsure") && all(&v.tone) && all(&v.light) && all(&v.motion) && all(&v.overlays)
}

pub fn has(list: &[String], word: &str) -> bool {
    list.iter().any(|w| w == word || w.starts_with(&format!("{word}:")))
}

/// Compare one shot's labels with what the signals measured.
fn cross_check(s: &Shot, v: &ShotVision) -> (Vec<String>, Vec<String>) {
    let (mut ok, mut bad) = (Vec::new(), Vec::new());
    let ev = |k: &str| s.events.iter().filter(|e| e.kind == k).count();
    let max_flash = s.events.iter().filter(|e| e.kind == "flash").filter_map(|e| e.params.get("peak").map(|p| p.value)).fold(0.0, f64::max);
    let scale = s.curves.iter().find(|c| c.prop == "scale").map(|c| {
        let v: Vec<f64> = c.keys.iter().map(|k| k.v[0]).collect();
        v.iter().copied().fold(f64::MIN, f64::max) / v.iter().copied().fold(f64::MAX, f64::min).max(1e-6)
    });
    let blur_peak = s.curves.iter().find(|c| c.prop == "gaussian_blur.radius").map(|c| c.keys.iter().map(|k| k.v[0]).fold(0.0, f64::max)).unwrap_or(0.0);
    let mut check = |label: &str, seen: bool, measured: bool, what: String| {
        if !seen {
            return;
        }
        if measured {
            ok.push(format!("{label}: {what}"));
        } else {
            bad.push(format!("shot {}: \"{label}\" seen, but {what}", s.id));
        }
    };
    check("inverted", has(&v.tone, "inverted"), ev("invert") > 0, if ev("invert") > 0 { "inversion measured".into() } else { "no negative luminance correlation was measured (a whole-shot inversion cannot be: draft adds it)".into() });
    check("threshold", has(&v.tone, "threshold"), ev("threshold") > 0, if ev("threshold") > 0 { "threshold measured".into() } else { "no two-tone hit was measured".into() });
    check("posterized", has(&v.tone, "posterized"), ev("posterize") > 0, if ev("posterize") > 0 { "posterize measured".into() } else { "no posterise hit was measured".into() });
    check(if has(&v.tone, "duotone") { "duotone" } else { "tinted" }, has(&v.tone, "duotone") || has(&v.tone, "tinted"), ev("duotone") > 0, if ev("duotone") > 0 { "duotone measured".into() } else { "no duotone was measured".into() });
    check("white_flash", has(&v.light, "white_flash"), max_flash >= 0.8, if max_flash >= 0.8 { format!("flash peak {max_flash:.1} stops") } else { "no exposure peak of 0.8 stops or more".into() });
    check("shake", has(&v.motion, "shake"), s.shake.is_some(), if s.shake.is_some() { "shake residual measured".into() } else { "no shake residual (or the shot is not tracked)".into() });
    for m in ["punch_in", "push_in", "zoom_out"] {
        let tracked = s.status == Ident::Measured;
        check(m, has(&v.motion, m), !tracked || scale.map(|r| r > 1.04).unwrap_or(false), match scale {
            Some(r) => format!("scale changes by {:.0}%", 100.0 * (r - 1.0)),
            None => "no scale change was measured".into(),
        });
    }
    let blur_seen = !matches!(v.blur.as_str(), "none" | "unsure");
    check(&v.blur.clone(), blur_seen, blur_peak >= 0.15, if blur_peak >= 0.15 { format!("blur up to {blur_peak:.2}% of height") } else { "no blur was measured".into() });
    if let (true, Some(k)) = (blur_seen, &s.blur_kind) {
        let label = v.blur.split(':').next().unwrap_or("");
        if label != k.kind && k.confidence >= 0.5 {
            bad.push(format!("shot {}: blur labelled {label}, the gradient statistics say {} (confidence {:.2})", s.id, k.kind, k.confidence));
        }
    }
    let flat = matches!(v.framing.as_str(), "full_frame");
    if flat && s.status == Ident::NeedsAgent {
        bad.push(format!("shot {}: framing full_frame, but the picture could not be tracked as a flat full-frame picture ({})", s.id, s.notes.first().cloned().unwrap_or_default()));
    }
    if matches!(v.framing.as_str(), "cube" | "tilted_card") && s.status == Ident::Measured {
        bad.push(format!("shot {}: framing {}, but the track held as a flat 2D move", s.id, v.framing));
    }
    // Signals the labels leave out.
    if ev("invert") > 0 && !has(&v.tone, "inverted") && !v.tone.is_empty() && v.tone != ["unsure"] {
        bad.push(format!("shot {}: an inversion was measured but the tone labels do not list inverted", s.id));
    }
    if max_flash >= 2.0 && !has(&v.light, "white_flash") && !has(&v.tone, "burnt_white") && !v.light.is_empty() && v.light != ["unsure"] {
        bad.push(format!("shot {}: a {max_flash:.1}-stop flash was measured but the light labels do not list white_flash", s.id));
    }
    (ok, bad)
}

/// Validate `answers` and merge them into `a`. Fails, changing nothing, on any invalid answer.
pub fn merge(a: &mut Analysis, answers: &Value) -> std::result::Result<MergeReport, Vec<String>> {
    let (labels, global) = parse_answers(a, answers)?;
    let mut rep = MergeReport::default();
    for (id, mut v) in labels {
        // Tile numbers as frames, numbered as `export` numbers them.
        let (main, extra) = tiles_for(a, &a.shots[id - 1]);
        let order: Vec<usize> = main.iter().chain(extra.iter()).map(|t| t.0).collect();
        v.frames = v.tiles.iter().map(|(k, ns)| (k.clone(), ns.iter().filter_map(|n| order.get(n.wrapping_sub(1)).copied()).collect())).collect();
        let s = &mut a.shots[id - 1];
        if unsure(&v) {
            rep.unanswered.push(id);
        } else {
            rep.labelled.push(id);
        }
        let (ok, bad) = cross_check(s, &v);
        rep.agreements += ok.len();
        v.checks = ok.iter().map(|o| format!("agrees: {o}")).chain(bad.iter().map(|b| format!("look again: {b}"))).collect();
        rep.disagreements.extend(bad);
        s.vision = Some(v);
    }
    if let Some(mut g) = global {
        if !g.overlays.is_empty() && a.layers.overlay.is_none() && a.layers.texture.is_none() {
            let m = format!("global: overlay {} seen, but no static strokes or texture were measured: draft adds a texture layer the person must supply", g.overlays.join(", "));
            g.checks.push(format!("look again: {m}"));
            rep.disagreements.push(m);
        }
        if let (Some(vw), Some(vm)) = (&g.vignette, &a.layers.vignette) {
            let strong = vm.corner_ratio.value < 0.45;
            if (vw == "none" && vm.amount.value > 0.3) || (vw == "strong" && !strong) || (vw == "light" && strong) {
                let m = format!("global: vignette labelled {vw}, measured corners at {:.0}% of the centre", 100.0 * vm.corner_ratio.value);
                g.checks.push(format!("look again: {m}"));
                rep.disagreements.push(m);
            }
        }
        a.vision = Some(g);
    }
    // Slot roles: the most common picture label of the slot's shots.
    for slot in a.slots.iter_mut() {
        let mut count: BTreeMap<String, usize> = BTreeMap::new();
        for id in &slot.shots {
            if let Some(v) = a.shots.get(id - 1).and_then(|s| s.vision.as_ref()) {
                if v.picture != "unsure" {
                    *count.entry(v.picture.clone()).or_default() += 1;
                }
            }
        }
        if let Some((role, _)) = count.into_iter().max_by_key(|(_, n)| *n) {
            rep.roles.insert(slot.id.clone(), role.clone());
            slot.role = Some(role);
        }
    }
    Ok(rep)
}

/// Read, merge and write back: `merge` on files. With `dry_run` nothing is written.
pub fn merge_file(analysis: &Path, answers: &Path, out: Option<&Path>, dry_run: bool) -> Result<MergeReport> {
    let mut a = crate::pipeline::load(analysis)?;
    let text = std::fs::read_to_string(answers).with_context(|| format!("reading {}", answers.display()))?;
    let doc: Value = serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", answers.display()))?;
    let rep = merge(&mut a, &doc).map_err(|errs| anyhow!("{} has {} invalid answer(s); nothing was merged:\n  {}", answers.display(), errs.len(), errs.join("\n  ")))?;
    if !dry_run {
        let target = out.unwrap_or(analysis);
        std::fs::write(target, crate::jsonfmt::to_string(&a)?).with_context(|| format!("writing {}", target.display()))?;
    }
    Ok(rep)
}

/// Plain-text account of a merge.
pub fn merge_summary(r: &MergeReport) -> String {
    let mut s = format!("labelled {} shot(s); {} still all unsure{}\n", r.labelled.len(), r.unanswered.len(), if r.unanswered.is_empty() { String::new() } else { format!(" ({})", r.unanswered.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")) });
    s += &format!("{} label(s) confirmed by the signals, {} to look at again\n", r.agreements, r.disagreements.len());
    for d in &r.disagreements {
        s += &format!("  look again: {d}\n");
    }
    if !r.roles.is_empty() {
        s += &format!("slot roles: {}\n", r.roles.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", "));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis() -> Analysis {
        let frames = {
            let mut c = crate::synth::Clip::new(96, 96, 36);
            for f in 0..36 {
                let fr = c.frame_mut(f);
                fr.picture = (f / 12) as u32 + 1;
                fr.scale = 1.0 + 0.02 * (f % 12) as f64;
                fr.stops = if f == 12 { 3.0 } else { 0.0 };
                fr.invert = (26..29).contains(&f);
            }
            c.render()
        };
        let source = Source { path: "x.mp4".into(), width: 96, height: 96, fps: 25.0, frames: 36, duration_seconds: 1.44, has_audio: false, analysis_width: 96, analysis_height: 96 };
        crate::pipeline::analyze_frames(&frames, None, source, &crate::AnalyzeOpts { bpm: Some(120.0), beat_offset: Some(0.0), ..Default::default() }).analysis
    }

    #[test]
    fn words_with_arguments_are_checked_and_colours_parsed() {
        let tone = QUESTIONS.iter().find(|q| q.field == "tone").unwrap();
        let (w, c) = check_word(tone, "duotone:navy/#ffee00").unwrap();
        assert_eq!(w, "duotone");
        assert_eq!(c[1].1, [1.0, 238.0 / 255.0, 0.0]);
        assert!(check_word(tone, "sepia_tone").unwrap_err().contains("not a Q3 word"));
        let motion = QUESTIONS.iter().find(|q| q.field == "motion").unwrap();
        assert_eq!(check_word(motion, "slide:Left").unwrap().0, "slide:left");
        assert!(check_word(motion, "slide:sideways").is_err());
        let blur = QUESTIONS.iter().find(|q| q.field == "blur").unwrap();
        assert_eq!(check_word(blur, "directional:200").unwrap().0, "directional:20");
        let border = QUESTIONS.iter().find(|q| q.field == "border").unwrap();
        assert_eq!(check_word(border, "glow:red").unwrap().1[0].0, "glow");
        assert!(check_word(border, "glow:ultraviolet").is_err());
    }

    #[test]
    fn merge_validates_cross_checks_and_sets_roles() {
        let mut a = analysis();
        assert_eq!(a.shots.len(), 3, "{:?}", a.shots.iter().map(|s| (s.f0, s.f1)).collect::<Vec<_>>());
        let mut doc = template(&a, &a.shots.iter().collect::<Vec<_>>());
        // An invalid word: nothing is merged and the error lists the words.
        doc["shots"][0]["picture"] = json!("portrait");
        let errs = merge(&mut a.clone(), &doc).unwrap_err();
        assert!(errs[0].contains("shots[0].picture") && errs[0].contains("close_up"), "{errs:?}");
        doc["shots"][0]["picture"] = json!("close_up");
        doc["shots"][0]["light"] = json!(["white_flash"]);
        doc["shots"][0]["framing"] = json!("full_frame");
        doc["shots"][1]["picture"] = json!("emblem");
        doc["shots"][1]["light"] = json!(["white_flash"]);
        doc["shots"][2]["tone"] = json!(["inverted", "duotone:black/gold"]);
        doc["shots"][2]["tiles"] = json!({ "inverted": [5] });
        let rep = merge(&mut a, &doc).unwrap();
        assert_eq!(rep.labelled, vec![1, 2, 3]);
        let v3 = a.shots[2].vision.as_ref().unwrap();
        assert_eq!(v3.tone, vec!["inverted", "duotone"]);
        assert_eq!(v3.colors["highlights"], [0.85, 0.65, 0.15]);
        // Shot 2 has the 3-stop flash, shot 1 none; shot 3 has the inversion.
        assert!(rep.disagreements.iter().any(|d| d.starts_with("shot 1: \"white_flash\"")), "{:?}", rep.disagreements);
        assert!(!rep.disagreements.iter().any(|d| d.starts_with("shot 2: \"white_flash\"")), "{:?}", rep.disagreements);
        assert!(v3.checks.iter().any(|c| c.starts_with("agrees: inverted")), "{:?}", v3.checks);
        assert_eq!(a.slots.iter().find(|s| s.shots.contains(&1)).unwrap().role.as_deref(), Some("close_up"));
        // The labels survive a round trip through the JSON file.
        let back: Analysis = serde_json::from_str(&crate::jsonfmt::to_string(&a).unwrap()).unwrap();
        assert_eq!(back.shots[2].vision, a.shots[2].vision);
    }

    #[test]
    fn tiles_follow_the_guide() {
        let a = analysis();
        let (main, extra) = tiles_for(&a, &a.shots[1]);
        let what: Vec<&str> = main.iter().map(|t| t.1.as_str()).collect();
        assert_eq!(what.first(), Some(&"prev"));
        // Every frame around the flash at the cut: the frames after cut+2 go to the second row, none twice.
        assert!(extra.iter().any(|t| t.1 == "flash+3"), "{extra:?}");
        assert!(extra.iter().all(|t| !main.iter().any(|m| m.0 == t.0)), "{main:?} {extra:?}");
        assert!(what.contains(&"cut") && what.contains(&"ref") && what.contains(&"last"), "{what:?}");
    }
}
