//! Text layers: layout, glyph masks and glyph quads.
//!
//! A text layer is laid out once (shaped by harfrust, broken into lines and
//! aligned here) into glyphs placed in layer space, in composition-height
//! units. Every frame, each glyph becomes a quad of its own, drawn through
//! the layer's matrix: its outline (skrifa) is rasterised to a coverage mask
//! (zeno) at the size the glyph has on screen, the masks of the layer are
//! packed into an atlas, and the quads sample their cell of it. One quad and
//! one matrix per glyph is what per-character animation builds on: the
//! layer's animators (`edits_core::text_select`, evaluated in the core) give
//! each glyph an offset, scale, turn, opacity, colour, extra tracking and
//! blur, and the glyph's quad is drawn through its own matrix.
//!
//! Sharpness: the em is rasterised at its size in output pixels (to a
//! quarter pixel), so text is as sharp at `--scale 0.5` as at 4K. When the
//! layer is only moved and scaled (no rotation, no perspective), every glyph
//! is also rasterised at its sub-pixel offset and its quad is put on whole
//! pixels, so the atlas is sampled texel for pixel.
//!
//! Purity: masks depend only on (font, weight, glyph, size, stroke,
//! sub-pixel offset) and an atlas only on the list of masks it holds, packed
//! in the order the glyphs come, so the caches never change a picture.

use crate::assets;
use crate::renderer::{Renderer, Samp, Tex};
use anyhow::{anyhow, Context, Result};
use edits_core::text_select::{GlyphRef, GlyphState};
use edits_core::{Align, Evaluator, Text};
use glam::{DMat4, DVec3, DVec4};
use harfrust::{ShapeOptions, ShaperData, ShaperInstance, UnicodeBuffer};
use skrifa::instance::{Location, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Inter (variable), the font of text layers that name none. OFL: see
/// `assets/fonts/README.md`.
static INTER: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");
const INTER_KEY: &str = "builtin:Inter";

/// Largest em, in pixels, that glyphs are rasterised at; bigger text is
/// magnified from it.
const MAX_EM_PX: f64 = 640.0;
/// Glyph masks kept between frames, in bytes.
const MASK_BUDGET: usize = 64 << 20;
/// Atlases kept on the GPU (one per text layer and size, typically).
const ATLASES: usize = 8;
/// Draws per render pass (each takes a uniform block).
const DRAWS_PER_PASS: usize = 1024;
/// Largest blur sigma of an animated glyph, in mask pixels.
const MAX_BLUR_PX: f64 = 96.0;

/// A font file a text layer uses.
pub(crate) struct Font {
    /// The file (path, size, modification time), or `builtin:Inter`.
    pub key: Arc<str>,
    data: Cow<'static, [u8]>,
}

impl Font {
    pub(crate) fn builtin() -> Font {
        Font { key: INTER_KEY.into(), data: Cow::Borrowed(INTER) }
    }

    /// Read a font file and check that it is one.
    pub(crate) fn load(path: &Path, key: &str) -> Result<Font> {
        let data = std::fs::read(path).with_context(|| format!("cannot read font {}", path.display()))?;
        FontRef::from_index(&data, 0).map_err(|e| anyhow!("{} is not a TrueType or OpenType font ({e})", path.display()))?;
        Ok(Font { key: key.into(), data: Cow::Owned(data) })
    }

    fn font_ref(&self) -> FontRef<'_> {
        FontRef::from_index(&self.data, 0).expect("checked when the font was loaded")
    }
}

/// The position on the font's variation axes for `weight` (ignored by a font
/// without a `wght` axis; clamped to the axis' range).
fn location(font: &FontRef, weight: Option<f64>) -> Location {
    font.axes().location(weight.filter(|w| w.is_finite()).map(|w| ("wght", w as f32)))
}

// ---- layout --------------------------------------------------------------------

/// One glyph of a laid-out text.
#[derive(Clone, Debug)]
pub(crate) struct Glyph {
    pub id: u32,
    /// Its origin on the baseline, in layer units (composition height = 1,
    /// y down), from the centre of the text block.
    pub origin: [f64; 2],
    /// Advance width in layer units, tracking included.
    pub advance: f64,
    /// The character of the text (counted in chars, line breaks included)
    /// that the glyph's cluster starts at.
    pub char_index: usize,
    pub line: usize,
}

/// A text block laid out in layer space.
#[cfg_attr(not(test), allow(dead_code))] // `lines` and `size` are for tests and per-character animation
pub(crate) struct Layout {
    /// In reading order, line by line; spaces at the end of a line are left out.
    pub glyphs: Vec<Glyph>,
    pub lines: usize,
    /// Width (the box: `max_width`, or the longest line) and height (cap
    /// height of the first line to the baseline of the last) of the block.
    pub size: [f64; 2],
    /// Font size in layer units.
    pub em: f64,
    /// Cap height in layer units: glyphs turn and scale about the middle of
    /// their advance, half a cap height above the baseline.
    pub cap: f64,
}

/// A glyph out of the shaper, in logical order.
struct Shaped {
    id: u32,
    advance: f64,
    offset: [f64; 2],
    /// Byte offset of its cluster in the paragraph.
    cluster: usize,
    char_index: usize,
    space: bool,
    /// A line may start at this glyph.
    break_before: bool,
}

struct Line {
    glyphs: Vec<Shaped>,
    rtl: bool,
}

/// Lay `t` out with `font`: shaping, line breaking at `max_width`, alignment
/// and centring. Right-to-left paragraphs are set right to left; mixed
/// directions within a paragraph are not reordered.
pub(crate) fn layout(font: &Font, t: &Text) -> Layout {
    let fref = font.font_ref();
    let loc = location(&fref, t.weight);
    let metrics = fref.metrics(Size::unscaled(), &loc);
    let em = if t.size.is_finite() { t.size.max(0.0) / 100.0 } else { 0.0 };
    let unit = em / metrics.units_per_em.max(1) as f64;
    let cap = metrics.cap_height.filter(|c| *c > 0.0).unwrap_or(metrics.ascent * 0.7) as f64 * unit;
    let tracking = if t.tracking.is_finite() { t.tracking / 100.0 } else { 0.0 };
    let line_height = if t.line_height.is_finite() { t.line_height * em } else { em };
    let max_width = t.max_width.filter(|m| m.is_finite() && *m > 0.0);

    let data = ShaperData::new(&fref);
    let instance = ShaperInstance::from_coords(&fref, loc.coords().iter().copied());
    let shaper = data.shaper(&fref).instance(Some(&instance)).build();

    let mut lines: Vec<Line> = Vec::new();
    let mut char_base = 0;
    for paragraph in t.text.split('\n') {
        let chars = paragraph.chars().count();
        let para = paragraph.strip_suffix('\r').unwrap_or(paragraph);
        let starts: Vec<usize> = para.char_indices().map(|(b, _)| b).collect();
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(para);
        buffer.guess_segment_properties();
        let rtl = buffer.direction() == harfrust::Direction::RightToLeft;
        let shaped = shaper.shape(buffer, ShapeOptions::new());
        let mut glyphs: Vec<Shaped> = shaped
            .glyph_infos()
            .iter()
            .zip(shaped.glyph_positions())
            .map(|(info, pos)| {
                let cluster = info.cluster as usize;
                Shaped {
                    id: info.glyph_id,
                    advance: pos.x_advance as f64 * unit,
                    offset: [pos.x_offset as f64 * unit, pos.y_offset as f64 * unit],
                    cluster,
                    char_index: char_base + starts.partition_point(|&s| s < cluster),
                    space: para.get(cluster..).and_then(|s| s.chars().next()).is_some_and(char::is_whitespace),
                    break_before: false,
                }
            })
            .collect();
        char_base += chars + 1;
        if rtl {
            glyphs.reverse();
        }
        let n = glyphs.len();
        for i in 0..n {
            if i + 1 == n || glyphs[i + 1].cluster != glyphs[i].cluster {
                glyphs[i].advance += tracking;
            }
            if i > 0 && glyphs[i - 1].cluster != glyphs[i].cluster {
                glyphs[i].break_before = can_break(para, glyphs[i].cluster);
            }
        }
        lines.extend(break_lines(glyphs, max_width, tracking).into_iter().map(|glyphs| Line { glyphs, rtl }));
    }

    let widths: Vec<(usize, f64)> = lines.iter().map(|l| visible(&l.glyphs, tracking)).collect();
    let box_width = max_width.unwrap_or_else(|| widths.iter().map(|w| w.1).fold(0.0, f64::max));
    let count = lines.len().max(1);
    let shift = -((count - 1) as f64 * line_height - cap) / 2.0;
    let mut glyphs = Vec::new();
    for (k, (line, &(end, width))) in lines.iter().zip(&widths).enumerate() {
        let mut pen = match t.align {
            Align::Left => -box_width / 2.0,
            Align::Center => -width / 2.0,
            Align::Right => box_width / 2.0 - width,
        };
        let y = k as f64 * line_height + shift;
        let order: Box<dyn Iterator<Item = usize>> = if line.rtl { Box::new((0..end).rev()) } else { Box::new(0..end) };
        for i in order {
            let g = &line.glyphs[i];
            glyphs.push(Glyph { id: g.id, origin: [pen + g.offset[0], y - g.offset[1]], advance: g.advance, char_index: g.char_index, line: k });
            pen += g.advance;
        }
    }
    Layout { glyphs, lines: lines.len(), size: [box_width, cap + (count - 1) as f64 * line_height], em, cap }
}

/// The glyphs a line shows (without its trailing spaces) and their width
/// (without the tracking after the last one).
fn visible(glyphs: &[Shaped], tracking: f64) -> (usize, f64) {
    let end = glyphs.iter().rposition(|g| !g.space).map_or(0, |i| i + 1);
    let sum: f64 = glyphs[..end].iter().map(|g| g.advance).sum();
    (end, if end > 0 { sum - tracking } else { 0.0 })
}

/// Greedy line breaking: a line ends at the last break opportunity before
/// it grows wider than `max_width`. A word wider than the line stays whole.
fn break_lines(glyphs: Vec<Shaped>, max_width: Option<f64>, tracking: f64) -> Vec<Vec<Shaped>> {
    let Some(max) = max_width else { return vec![glyphs] };
    let mut ends = Vec::new();
    let (mut start, mut brk, mut i) = (0, None, 0);
    while i < glyphs.len() {
        if i > start && glyphs[i].break_before {
            brk = Some(i);
        }
        if !glyphs[i].space && visible(&glyphs[start..=i], tracking).1 > max + 1e-9 {
            if let Some(b) = brk.take().filter(|b| *b > start) {
                ends.push(b);
                start = b;
                continue;
            }
        }
        i += 1;
    }
    let mut out = Vec::new();
    let mut rest = glyphs;
    for end in ends.into_iter().rev() {
        let tail = rest.split_off(end);
        out.push(tail);
    }
    out.push(rest);
    out.reverse();
    out
}

/// Whether a line may start at byte `at` of `text`: after spaces and
/// dashes, and between CJK characters (except before closing and after
/// opening punctuation), never before a space.
fn can_break(text: &str, at: usize) -> bool {
    let (Some(before), Some(c)) = (text.get(..at).and_then(|s| s.chars().next_back()), text.get(at..).and_then(|s| s.chars().next())) else {
        return false;
    };
    if c.is_whitespace() {
        return false;
    }
    if before.is_whitespace() || matches!(before, '-' | '\u{2010}' | '\u{2013}' | '\u{2014}' | '/') {
        return true;
    }
    (is_cjk(before) || is_cjk(c)) && !no_line_start(c) && !no_line_end(before)
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

/// Characters a line should not start with (kinsoku): closing brackets,
/// stops, small kana, the long vowel mark.
fn no_line_start(c: char) -> bool {
    "、。，．・：；？！ー」』）】〕〉》〙〗〟’”ゝゞヽヾぁぃぅぇぉっゃゅょゎゕゖァィゥェォッャュョヮヵヶ々〻゠〜～）］｝,.!?;:)]}%…‥".contains(c)
}

/// Characters a line should not end with: opening brackets.
fn no_line_end(c: char) -> bool {
    "「『（【〔〈《〘〖〝‘“［｛([{".contains(c)
}

// ---- glyph masks -----------------------------------------------------------------

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct MaskKey {
    font: Arc<str>,
    weight: Option<u64>,
    glyph: u32,
    /// Em in quarter pixels.
    em: u32,
    /// Stroke width (beyond the edge) in quarter pixels.
    stroke: u32,
    /// Sub-pixel offset of the origin in quarter pixels.
    sub: [u8; 2],
    /// Gaussian blur sigma in quarter pixels.
    blur: u32,
}

/// The coverage of one glyph: two bytes per pixel, the letter and the
/// letter with its outline. Pixel (0, 0) is at (`left`, `top`) from the
/// whole pixel the glyph's origin is in.
struct Mask {
    left: i32,
    top: i32,
    w: u32,
    h: u32,
    data: Vec<u8>,
}

/// Collects an outline (y up, pixels) as zeno commands (y down), moved by `shift`.
struct Pen {
    commands: Vec<zeno::Command>,
    shift: [f32; 2],
}

impl Pen {
    fn point(&self, x: f32, y: f32) -> zeno::Point {
        zeno::Point::new(x + self.shift[0], self.shift[1] - y)
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.commands.push(zeno::Command::MoveTo(p));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.commands.push(zeno::Command::LineTo(p));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (c, p) = (self.point(cx0, cy0), self.point(x, y));
        self.commands.push(zeno::Command::QuadTo(c, p));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (c0, c1, p) = (self.point(cx0, cy0), self.point(cx1, cy1), self.point(x, y));
        self.commands.push(zeno::Command::CurveTo(c0, c1, p));
    }
    fn close(&mut self) {
        self.commands.push(zeno::Command::Close);
    }
}

fn rasterise(font: &Font, key: &MaskKey) -> Mask {
    let empty = Mask { left: 0, top: 0, w: 0, h: 0, data: Vec::new() };
    let fref = font.font_ref();
    let Some(outline) = fref.outline_glyphs().get(GlyphId::new(key.glyph)) else { return empty };
    let loc = location(&fref, key.weight.map(f64::from_bits));
    let mut pen = Pen { commands: Vec::new(), shift: [key.sub[0] as f32 / 4.0, key.sub[1] as f32 / 4.0] };
    if outline.draw(DrawSettings::unhinted(Size::new(key.em as f32 / 4.0), &loc), &mut pen).is_err() || pen.commands.is_empty() {
        return empty;
    }
    let path = &pen.commands;
    let (fill, outer, place) = if key.stroke > 0 {
        let mut stroke = zeno::Stroke::new(key.stroke as f32 / 2.0);
        stroke.join(zeno::Join::Round);
        let (outer, place) = zeno::Mask::new(path).style(stroke).render();
        let (fill, _) = zeno::Mask::new(path).offset(zeno::Vector::new(-place.left as f32, -place.top as f32)).size(place.width, place.height).render();
        (fill, Some(outer), place)
    } else {
        let (fill, place) = zeno::Mask::new(path).render();
        (fill, None, place)
    };
    if place.width == 0 || place.height == 0 {
        return empty;
    }
    let mut data = Vec::with_capacity(fill.len() * 2);
    for (i, &f) in fill.iter().enumerate() {
        data.push(f);
        data.push(outer.as_ref().map_or(0, |o| o[i].max(f)));
    }
    let mask = Mask { left: place.left, top: place.top, w: place.width, h: place.height, data };
    if key.blur >= 2 {
        blurred(&mask, key.blur as f64 / 4.0)
    } else {
        mask
    }
}

/// The widths of three box filters whose sequence approximates a gaussian
/// of `sigma` pixels (odd widths, the standard "boxes for gauss").
fn boxes_for_gauss(sigma: f64) -> [usize; 3] {
    let n = 3.0;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let wlf = wl as f64;
    let m = ((12.0 * sigma * sigma - n * wlf * wlf - 4.0 * n * wlf - 3.0 * n) / (-4.0 * wlf - 4.0)).round() as i64;
    [0, 1, 2].map(|i| if i < m { wl as usize } else { wu as usize })
}

/// `mask` blurred by a gaussian of `sigma` pixels (three box passes each
/// way), grown by the reach of the blur on every side.
fn blurred(mask: &Mask, sigma: f64) -> Mask {
    let boxes = boxes_for_gauss(sigma);
    let pad: usize = boxes.iter().map(|b| b / 2).sum::<usize>() + 1;
    let (w, h) = (mask.w as usize + 2 * pad, mask.h as usize + 2 * pad);
    let mut out = vec![0u8; w * h * 2];
    let mut buf = vec![0f32; w * h];
    let mut tmp = vec![0f32; w.max(h)];
    for c in 0..2 {
        buf.iter_mut().for_each(|v| *v = 0.0);
        for y in 0..mask.h as usize {
            for x in 0..mask.w as usize {
                buf[(y + pad) * w + x + pad] = mask.data[(y * mask.w as usize + x) * 2 + c] as f32;
            }
        }
        for b in boxes {
            let r = b / 2;
            // Rows, then columns: a running sum over 2r + 1 pixels, zero outside.
            for y in 0..h {
                box_line(&mut buf, y * w, 1, w, r, &mut tmp);
            }
            for x in 0..w {
                box_line(&mut buf, x, w, h, r, &mut tmp);
            }
        }
        for (i, v) in buf.iter().enumerate() {
            out[i * 2 + c] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    Mask { left: mask.left - pad as i32, top: mask.top - pad as i32, w: w as u32, h: h as u32, data: out }
}

/// A box filter of radius `r` along `len` values of `buf` from `start`, `step` apart.
fn box_line(buf: &mut [f32], start: usize, step: usize, len: usize, r: usize, tmp: &mut [f32]) {
    if r == 0 {
        return;
    }
    let at = |i: usize| start + i * step;
    let norm = 1.0 / (2 * r + 1) as f32;
    let mut sum: f32 = (0..=r.min(len - 1)).map(|i| buf[at(i)]).sum();
    for i in 0..len {
        tmp[i] = sum * norm;
        if i + r + 1 < len {
            sum += buf[at(i + r + 1)];
        }
        if i >= r {
            sum -= buf[at(i - r)];
        }
    }
    for (i, v) in tmp.iter().take(len).enumerate() {
        buf[at(i)] = *v;
    }
}

// ---- atlas -----------------------------------------------------------------------

/// The masks of one text layer at one size, side by side in a texture with
/// a transparent pixel around each.
struct Atlas {
    tex: Tex,
    /// Top-left of each padded cell, in the order of the keys.
    cells: Vec<[u32; 2]>,
}

/// Shelf packing in order: (width, height, top-left of each), or `None` when
/// it does not fit in `max_side`.
fn pack(sizes: &[[u32; 2]], max_side: u32) -> Option<(u32, u32, Vec<[u32; 2]>)> {
    let area: f64 = sizes.iter().map(|s| s[0] as f64 * s[1] as f64).sum();
    let widest = sizes.iter().map(|s| s[0]).max().unwrap_or(1);
    let width = ((area.sqrt() * 1.15).ceil() as u32).max(widest).max(1).min(max_side);
    if widest > width {
        return None;
    }
    let (mut x, mut y, mut row) = (0, 0, 0);
    let mut at = Vec::with_capacity(sizes.len());
    for s in sizes {
        if x + s[0] > width {
            y += row;
            x = 0;
            row = 0;
        }
        at.push([x, y]);
        x += s[0];
        row = row.max(s[1]);
    }
    let height = (y + row).max(1);
    (height <= max_side).then_some((width, height, at))
}

/// Caches of text layers, kept by the renderer.
#[derive(Default)]
pub(crate) struct TextCache {
    /// Font files by file key.
    fonts: HashMap<String, Arc<Font>>,
    layouts: HashMap<String, Arc<Layout>>,
    masks: HashMap<MaskKey, Arc<Mask>>,
    mask_bytes: usize,
    /// Most recently used last.
    atlases: Vec<(Vec<MaskKey>, Arc<Atlas>)>,
    pipeline: Option<wgpu::RenderPipeline>,
}

/// How a text layer's glyphs land on the output pixels.
#[derive(Clone, Copy)]
enum Placement {
    /// Moved and uniformly scaled only: pixel = `origin` + `scale` * layer point.
    Snap { origin: [f64; 2], scale: f64 },
    /// Anything else: rasterised for `scale` pixels per layer unit around the origin.
    Free { scale: f64 },
}

/// A layer point through `mvp` into output pixels (y down).
fn to_pixels(mvp: &DMat4, p: [f64; 2], w: f64, h: f64) -> Option<[f64; 2]> {
    let c = *mvp * DVec4::new(p[0], p[1], 0.0, 1.0);
    (c.w > 1e-9).then(|| [(c.x / c.w * 0.5 + 0.5) * w, (0.5 - c.y / c.w * 0.5) * h])
}

fn placement(mvp: &DMat4, w: u32, h: u32, em: f64) -> Placement {
    let (w, h) = (w as f64, h as f64);
    let px = |p| to_pixels(mvp, p, w, h);
    if let (Some(o), Some(x), Some(y), Some(xy)) = (px([0.0, 0.0]), px([1.0, 0.0]), px([0.0, 1.0]), px([1.0, 1.0])) {
        let s = x[0] - o[0];
        let tol = 1e-7 * s.abs().max(1.0);
        let affine = (xy[0] - (x[0] + y[0] - o[0])).abs() < tol && (xy[1] - (x[1] + y[1] - o[1])).abs() < tol;
        if s > 0.0 && affine && (x[1] - o[1]).abs() < tol && (y[0] - o[0]).abs() < tol && (y[1] - o[1] - s).abs() < tol && em * s <= MAX_EM_PX {
            return Placement::Snap { origin: o, scale: s };
        }
    }
    // The local scale at the origin, for a layer turned or seen in perspective.
    let d = 1e-3;
    let scale = match (px([0.0, 0.0]), px([d, 0.0]), px([0.0, d])) {
        (Some(o), Some(x), Some(y)) => ((x[0] - o[0]).hypot(x[1] - o[1])).max((y[0] - o[0]).hypot(y[1] - o[1])) / d,
        _ => h,
    };
    Placement::Free { scale: if scale.is_finite() { scale } else { h } }
}

/// A glyph draw: atlas bind group, matrix, parameters (`shaders/text.wgsl`).
type GlyphDraw = (wgpu::BindGroup, [f32; 16], [[f32; 4]; 3]);

/// One glyph quad: where in layer space, which atlas cell, and what the
/// animators made of the glyph.
struct GlyphQuad {
    /// Left, top, width, height in layer units.
    rect: [f64; 4],
    mask: usize,
    /// The glyph's own matrix in layer space (its animated offset, turn and
    /// scale), when it has one.
    matrix: Option<DMat4>,
    /// Straight sRGB.
    fill: [f32; 4],
    opacity: f32,
}

/// Degrees X, then Y, then Z, as for layers (`geometry.rs`).
fn rotation(deg: [f64; 3]) -> DMat4 {
    DMat4::from_rotation_z(deg[2].to_radians()) * DMat4::from_rotation_y(deg[1].to_radians()) * DMat4::from_rotation_x(deg[0].to_radians())
}

/// How far each glyph moves sideways for the animators' tracking: the extra
/// advances of the glyphs before it on its line, with the line kept aligned
/// (a centred line grows both ways).
fn tracking_shifts(layout: &Layout, states: &[GlyphState], align: Align) -> Vec<f64> {
    let g = &layout.glyphs;
    let mut out = vec![0.0; g.len()];
    let mut i = 0;
    while i < g.len() {
        let (start, line) = (i, g[i].line);
        let mut acc = 0.0;
        while i < g.len() && g[i].line == line {
            out[i] = acc;
            acc += states[i].tracking / 100.0;
            i += 1;
        }
        // The extra after the last glyph does not widen the line.
        let total = acc - states[i - 1].tracking / 100.0;
        let shift = match align {
            Align::Left => 0.0,
            Align::Center => -total / 2.0,
            Align::Right => -total,
        };
        out[start..i].iter_mut().for_each(|v| *v += shift);
    }
    out
}

/// Straight sRGB to premultiplied linear.
fn premultiplied(c: [f32; 4]) -> [f32; 4] {
    let a = c[3].clamp(0.0, 1.0);
    let l = |v: f32| assets::srgb_to_linear(v.clamp(0.0, 1.0)) * a;
    [l(c[0]), l(c[1]), l(c[2]), a]
}

impl Renderer {
    /// The fonts the text layers of `layers` name, by the name as written
    /// (`""` for the built-in Inter), loaded or taken from the cache. Font
    /// files no layer uses any more are forgotten.
    pub(crate) fn load_fonts(&mut self, layers: &[(&str, Option<&str>)], base_dir: &Path, file_key: impl Fn(&Path) -> String) -> Result<HashMap<String, Arc<Font>>> {
        let mut out: HashMap<String, Arc<Font>> = HashMap::new();
        let mut kept = Vec::new();
        for &(id, font) in layers {
            let name = font.unwrap_or("");
            if out.contains_key(name) {
                continue;
            }
            let loaded = match font {
                None => Arc::new(Font::builtin()),
                Some(path) => {
                    let full = base_dir.join(path);
                    if !full.is_file() {
                        anyhow::bail!(
                            "layer \"{id}\": font not found: {} (paths are relative to the project file; remove \"font\" to use the built-in Inter)",
                            full.display()
                        );
                    }
                    let key = file_key(&full);
                    kept.push(key.clone());
                    match self.text.fonts.get(&key) {
                        Some(f) => f.clone(),
                        None => {
                            let f = Arc::new(Font::load(&full, &key).with_context(|| format!("layer \"{id}\""))?);
                            self.text.fonts.insert(key, f.clone());
                            f
                        }
                    }
                }
            };
            out.insert(name.to_string(), loaded);
        }
        self.text.fonts.retain(|k, _| kept.contains(k));
        let used: Vec<Arc<str>> = out.values().map(|f| f.key.clone()).collect();
        self.text.layouts.retain(|k, _| used.iter().any(|u| k.starts_with(&format!("{u}|"))));
        Ok(out)
    }

    /// The layout of `text` set in `font`, from the cache when it was made before.
    fn text_layout(&mut self, font: &Font, text: &Text) -> Arc<Layout> {
        // Everything the layout depends on (not the colours or the animators).
        let t = text;
        let key = format!("{}|{:?}", font.key, (&t.text, t.size, t.weight, t.align, t.line_height, t.tracking, t.max_width));
        if let Some(l) = self.text.layouts.get(&key) {
            return l.clone();
        }
        let l = Arc::new(layout(font, text));
        self.text.layouts.insert(key, l.clone());
        l
    }

    fn glyph_mask(&mut self, font: &Font, key: MaskKey) -> Arc<Mask> {
        if let Some(m) = self.text.masks.get(&key) {
            return m.clone();
        }
        let m = Arc::new(rasterise(font, &key));
        if self.text.mask_bytes + m.data.len() > MASK_BUDGET {
            self.text.masks.clear();
            self.text.mask_bytes = 0;
        }
        self.text.mask_bytes += m.data.len();
        self.text.masks.insert(key, m.clone());
        m
    }

    /// The atlas holding `masks` (in this order), uploaded the first time.
    fn atlas(&mut self, keys: Vec<MaskKey>, masks: &[Arc<Mask>]) -> Result<Option<Arc<Atlas>>> {
        if let Some(i) = self.text.atlases.iter().position(|(k, _)| *k == keys) {
            let entry = self.text.atlases.remove(i);
            let atlas = entry.1.clone();
            self.text.atlases.push(entry);
            return Ok(Some(atlas));
        }
        let max = self.max_texture_side();
        let sizes: Vec<[u32; 2]> = masks.iter().map(|m| [m.w + 2, m.h + 2]).collect();
        let Some((w, h, cells)) = pack(&sizes, max) else { return Ok(None) };
        let mut data = vec![0u8; (2 * w * h) as usize];
        for (m, at) in masks.iter().zip(&cells) {
            for row in 0..m.h {
                let src = (2 * row * m.w) as usize;
                let dst = (2 * ((at[1] + 1 + row) * w + at[0] + 1)) as usize;
                data[dst..dst + 2 * m.w as usize].copy_from_slice(&m.data[src..src + 2 * m.w as usize]);
            }
        }
        let tex = self.upload_mask(w, h, &data);
        let atlas = Arc::new(Atlas { tex, cells });
        self.text.atlases.push((keys, atlas.clone()));
        if self.text.atlases.len() > ATLASES {
            let (_, old) = self.text.atlases.remove(0);
            self.forget_texture(old.tex.id);
        }
        Ok(Some(atlas))
    }

    /// The masks and quads of `layout` drawn with `mvp` into a `w` x `h`
    /// frame, with the em rasterised at no more than `max_em` pixels, and
    /// each glyph as the animators left it (`states`, one per glyph).
    #[allow(clippy::too_many_arguments)]
    fn glyph_quads(&mut self, font: &Font, text: &Text, layout: &Layout, states: Option<&[GlyphState]>, mvp: &DMat4, (w, h): (u32, u32), max_em: f64) -> (Vec<MaskKey>, Vec<Arc<Mask>>, Vec<GlyphQuad>) {
        let em = layout.em;
        let place = match placement(mvp, w, h, em) {
            Placement::Snap { scale, .. } if em * scale > max_em => Placement::Free { scale },
            p => p,
        };
        let wanted = match place {
            Placement::Snap { scale, .. } | Placement::Free { scale } => em * scale,
        };
        let weight = text.weight.filter(|w| w.is_finite()).map(f64::to_bits);
        let shifts = states.map(|s| tracking_shifts(layout, s, text.align));
        let mut keys: Vec<MaskKey> = Vec::new();
        let mut index: HashMap<MaskKey, usize> = HashMap::new();
        let mut masks = Vec::new();
        let mut quads = Vec::new();
        for (gi, g) in layout.glyphs.iter().enumerate() {
            let st = states.map(|s| &s[gi]);
            let opacity = st.map_or(1.0, |s| s.opacity);
            if opacity.is_nan() || opacity <= 1e-4 || st.is_some_and(|s| s.scale.iter().any(|v| v.is_nan() || v.abs() <= 1e-6)) {
                continue;
            }
            let off = st.map_or([0.0; 3], |s| s.offset);
            let shift = [off[0] + shifts.as_ref().map_or(0.0, |v| v[gi]), off[1], off[2]];
            // Glyphs scaled up are rasterised bigger (in steps of half an octave).
            let grow = st.map_or(1.0, |s| s.scale[0].abs().max(s.scale[1].abs())).clamp(1.0, 64.0);
            let grow = if grow > 1.0 { 2f64.powf((grow.log2() * 2.0).ceil() / 2.0) } else { 1.0 };
            let em_q = ((wanted * grow).min(max_em) * 4.0).round().max(4.0);
            // Pixels per layer unit of the mask.
            let k = em_q / 4.0 / em.max(1e-12);
            let stroke_q = match &text.stroke {
                Some(s) if s.width.is_finite() && s.width > 0.0 && s.color[3] > 0.0 => (s.width / 100.0 * k * 4.0).round().max(1.0),
                _ => 0.0,
            };
            let blur_q = st.map_or(0.0, |s| (s.blur / 100.0 * k * 4.0).round().clamp(0.0, 4.0 * MAX_BLUR_PX));
            // Moved only (in the plane): the quad can still sit on whole pixels.
            let moved_only = st.is_none_or(|s| s.is_translation() && s.offset[2] == 0.0);
            let snap = match place {
                Placement::Snap { origin, scale } if moved_only => Some((origin, scale)),
                _ => None,
            };
            let (sub, base) = match snap {
                Some((origin, scale)) => {
                    let p = [origin[0] + scale * (g.origin[0] + shift[0]), origin[1] + scale * (g.origin[1] + shift[1])];
                    let mut base = [p[0].floor(), p[1].floor()];
                    let mut sub = [0u8; 2];
                    for i in 0..2 {
                        let q = ((p[i] - base[i]) * 4.0).round();
                        if q >= 4.0 {
                            base[i] += 1.0;
                        } else {
                            sub[i] = q as u8;
                        }
                    }
                    (sub, Some(base))
                }
                None => ([0, 0], None),
            };
            let key = MaskKey { font: font.key.clone(), weight, glyph: g.id, em: em_q as u32, stroke: stroke_q as u32, sub, blur: blur_q as u32 };
            let i = match index.get(&key) {
                Some(&i) => i,
                None => {
                    let mask = self.glyph_mask(font, key.clone());
                    if mask.w == 0 {
                        continue;
                    }
                    index.insert(key.clone(), masks.len());
                    keys.push(key);
                    masks.push(mask);
                    masks.len() - 1
                }
            };
            let m = &masks[i];
            let (cw, ch) = ((m.w + 2) as f64, (m.h + 2) as f64);
            let (rect, matrix) = match (snap, base) {
                (Some((origin, scale)), Some(base)) => {
                    let (x, y) = (base[0] + (m.left - 1) as f64, base[1] + (m.top - 1) as f64);
                    ([(x - origin[0]) / scale, (y - origin[1]) / scale, cw / scale, ch / scale], None)
                }
                _ => {
                    let rect = [g.origin[0] + (m.left - 1) as f64 / k, g.origin[1] + (m.top - 1) as f64 / k, cw / k, ch / k];
                    let matrix = st.map(|s| {
                        let pivot = DVec3::new(g.origin[0] + g.advance / 2.0, g.origin[1] - layout.cap / 2.0, 0.0);
                        DMat4::from_translation(pivot + DVec3::from(shift)) * rotation(s.rotation) * DMat4::from_scale(DVec3::from(s.scale)) * DMat4::from_translation(-pivot)
                    });
                    (rect, matrix)
                }
            };
            quads.push(GlyphQuad { rect, mask: i, matrix, fill: st.map_or(text.fill, |s| s.fill), opacity: opacity as f32 });
        }
        (keys, masks, quads)
    }

    /// The pipeline that draws glyphs, made the first time.
    fn glyph_pipeline(&mut self) -> wgpu::RenderPipeline {
        if let Some(p) = &self.text.pipeline {
            return p.clone();
        }
        let source = concat!(include_str!("shaders/common.wgsl"), include_str!("shaders/text.wgsl"));
        let p = self.make_blended_pipeline("edits text", source, "fs_glyph");
        self.text.pipeline = Some(p.clone());
        p
    }

    /// A text layer's picture: its glyphs drawn through `mvp` (the layer's
    /// matrix through the camera) into a fresh frame-sized texture, each as
    /// the layer's animators make it at `t` (the layer's own time).
    pub(crate) fn draw_text(&mut self, layer_id: &str, text: &Text, mvp: DMat4, t: f64) -> Result<Tex> {
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let font = scene
            .fonts
            .get(text.font.as_deref().unwrap_or(""))
            .cloned()
            .ok_or_else(|| anyhow!("layer \"{layer_id}\": its font was not loaded (prepare the project the layer belongs to)"))?;
        let (w, h) = (self.ctx.w, self.ctx.h);
        let layout = self.text_layout(&font, text);
        let states = text.animators.iter().any(|a| a.enabled).then(|| {
            let refs: Vec<GlyphRef> = layout.glyphs.iter().map(|g| GlyphRef { char_index: g.char_index, line: g.line }).collect();
            Evaluator::new(&scene.project).text_glyphs(text, &refs, t)
        });
        let mut max_em = MAX_EM_PX;
        let (atlas, masks, quads) = loop {
            let (keys, masks, quads) = self.glyph_quads(&font, text, &layout, states.as_deref(), &mvp, (w, h), max_em);
            if quads.is_empty() {
                break (None, masks, quads);
            }
            match self.atlas(keys, &masks)? {
                Some(atlas) => break (Some(atlas), masks, quads),
                // Too many large glyphs for one texture: rasterise smaller and magnify.
                None if max_em > 8.0 => max_em /= 2.0,
                None => anyhow::bail!("layer \"{layer_id}\": the text has too many glyphs to draw"),
            }
        };
        let target = self.acquire(w, h);
        let pipeline = self.glyph_pipeline();
        let Some(atlas) = atlas else {
            self.text_pass(&pipeline, &target, &[], true);
            return Ok(target);
        };
        let (aw, ah) = (atlas.tex.w as f32, atlas.tex.h as f32);
        let bind = self.bind(&atlas.tex, None, Samp::Linear);
        let stroke = text.stroke.as_ref().filter(|s| s.width.is_finite() && s.width > 0.0).map(|s| premultiplied(s.color));
        // Every outline beneath every letter, so that outlines never cover a neighbour.
        let mut draws: Vec<([f32; 16], [[f32; 4]; 3])> = Vec::with_capacity(quads.len() * 2);
        let passes: Vec<(Option<[f32; 4]>, [f32; 4])> = stroke.map(|c| (Some(c), [0.0, 1.0, 0.0, 0.0])).into_iter().chain([(None, [1.0, 0.0, 0.0, 0.0])]).collect();
        for (stroke, channel) in passes {
            for q in &quads {
                let colour = stroke.unwrap_or_else(|| premultiplied(q.fill)).map(|c| c * q.opacity);
                if colour[3].is_nan() || colour[3] <= 0.0 {
                    continue;
                }
                let m = &masks[q.mask];
                let cell = atlas.cells[q.mask];
                let mut model = DMat4::from_translation(DVec3::new(q.rect[0] + q.rect[2] / 2.0, q.rect[1] + q.rect[3] / 2.0, 0.0)) * DMat4::from_scale(DVec3::new(q.rect[2], q.rect[3], 1.0));
                if let Some(g) = q.matrix {
                    model = g * model;
                }
                let uv = [cell[0] as f32 / aw, cell[1] as f32 / ah, (m.w + 2) as f32 / aw, (m.h + 2) as f32 / ah];
                draws.push(((mvp * model).as_mat4().to_cols_array(), [uv, colour, channel]));
            }
        }
        let mut first = true;
        for chunk in draws.chunks(DRAWS_PER_PASS) {
            let list: Vec<_> = chunk.iter().map(|(m, p)| (bind.clone(), *m, *p)).collect();
            self.text_pass(&pipeline, &target, &list, first);
            first = false;
        }
        if first {
            self.text_pass(&pipeline, &target, &[], true);
        }
        Ok(target)
    }

    /// One pass of glyph draws (bind group, matrix, parameters) into `target`.
    fn text_pass(&mut self, pipeline: &wgpu::RenderPipeline, target: &Tex, draws: &[GlyphDraw], clear: bool) {
        self.reserve_uniforms(draws.len() as u64);
        let mut list = Vec::with_capacity(draws.len());
        for (bind, mvp, p) in draws {
            let mut uni = self.uniform(target, p);
            uni.mvp = *mvp;
            list.push((bind.clone(), self.push_uniform(&uni), 6));
        }
        self.name(|| "text".into());
        let clear = clear.then_some(wgpu::Color::TRANSPARENT);
        self.raw_pass(pipeline, &target.view, None, None, clear, &list);
    }
}

/// The pixel format of glyph atlases: coverage of the letter and of the
/// letter with its outline.
pub(crate) const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg8Unorm;


#[cfg(test)]
mod tests {
    use super::*;

    fn text(json: &str) -> Text {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_line_is_centred_on_the_origin() {
        let l = layout(&Font::builtin(), &text(r#"{"text":"HIH","size":20}"#));
        assert_eq!((l.lines, l.glyphs.len()), (1, 3));
        let first = l.glyphs[0].origin[0];
        let last = l.glyphs[2].origin[0] + l.glyphs[2].advance;
        assert!((first + last).abs() < 1e-9, "{first} {last}");
        // The capitals' middle is on y = 0: the baseline is half a cap height down.
        let cap = l.size[1];
        assert!((l.glyphs[0].origin[1] - cap / 2.0).abs() < 1e-9 && cap > 0.1 && cap < 0.2, "{cap}");
    }

    #[test]
    fn lines_wrap_at_max_width_and_align() {
        let one = layout(&Font::builtin(), &text(r#"{"text":"ONE TWO","size":10}"#));
        let width = one.glyphs.last().map(|g| g.origin[0] + g.advance).unwrap() - one.glyphs[0].origin[0];
        for (align, left_edge) in [("left", true), ("right", false)] {
            let t = text(&format!(r#"{{"text":"ONE TWO","size":10,"align":"{align}","max_width":{}}}"#, width * 0.8));
            let l = layout(&Font::builtin(), &t);
            assert_eq!(l.lines, 2);
            let line = |k: usize| l.glyphs.iter().filter(|g| g.line == k).collect::<Vec<_>>();
            let (a, b) = (line(0), line(1));
            assert_eq!((a.len(), b.len()), (3, 3), "the space ends the first line and is not drawn");
            if left_edge {
                assert!((a[0].origin[0] - b[0].origin[0]).abs() < 1e-9 && (a[0].origin[0] + width * 0.4).abs() < 1e-9);
            } else {
                let end = |g: &[&Glyph]| g.last().map(|g| g.origin[0] + g.advance).unwrap();
                assert!((end(&a) - end(&b)).abs() < 1e-9 && (end(&a) - width * 0.4).abs() < 1e-9);
            }
            assert!(b[0].origin[1] > a[0].origin[1] && b[0].char_index == 4);
        }
        // A word longer than the line stays whole.
        let l = layout(&Font::builtin(), &text(r#"{"text":"LONGWORD","size":10,"max_width":0.01}"#));
        assert_eq!(l.lines, 1);
    }

    #[test]
    fn tracking_and_weight_change_the_advance() {
        let w = |json: &str| {
            let l = layout(&Font::builtin(), &text(json));
            l.glyphs.last().map(|g| g.origin[0] + g.advance).unwrap() - l.glyphs[0].origin[0]
        };
        let base = w(r#"{"text":"ABC","size":10}"#);
        // One percent after each of the three letters (the last advance included).
        assert!((w(r#"{"text":"ABC","size":10,"tracking":1}"#) - base - 0.03).abs() < 1e-9);
        assert!(w(r#"{"text":"ABC","size":10,"weight":900}"#) > base * 1.03);
    }

    #[test]
    fn cjk_breaks_between_characters_but_not_before_closing_marks() {
        assert!(can_break("日本語", 3));
        assert!(!can_break("日。", 3));
        assert!(!can_break("「日", 3));
        assert!(can_break("a b", 2));
        assert!(!can_break("a b", 1));
        assert!(!can_break("ab", 1));
    }

    #[test]
    fn packing_keeps_cells_apart_and_in_bounds() {
        let sizes = [[10, 12], [30, 5], [7, 7], [12, 40]];
        let (w, h, at) = pack(&sizes, 4096).unwrap();
        for (i, (s, a)) in sizes.iter().zip(&at).enumerate() {
            assert!(a[0] + s[0] <= w && a[1] + s[1] <= h);
            for (t, b) in sizes.iter().zip(&at).skip(i + 1) {
                let apart = a[0] + s[0] <= b[0] || b[0] + t[0] <= a[0] || a[1] + s[1] <= b[1] || b[1] + t[1] <= a[1];
                assert!(apart, "{a:?} {b:?}");
            }
        }
        assert!(pack(&[[100, 100]], 50).is_none());
    }

    #[test]
    fn masks_have_a_wider_outline_channel() {
        let font = Font::builtin();
        let id = font.font_ref().charmap().map('O').unwrap().to_u32();
        let key = |stroke| MaskKey { font: font.key.clone(), weight: None, glyph: id, em: 400, stroke, sub: [0, 0], blur: 0 };
        let plain = rasterise(&font, &key(0));
        let outlined = rasterise(&font, &key(16));
        assert!(plain.w > 50 && plain.h > 50);
        // 4 pixels of outline on each side.
        assert!((plain.w + 7..=plain.w + 10).contains(&outlined.w) && (plain.h + 7..=plain.h + 10).contains(&outlined.h), "{}x{} {}x{}", plain.w, plain.h, outlined.w, outlined.h);
        let count = |m: &Mask, c: usize| m.data.chunks(2).filter(|p| p[c] > 127).count();
        assert_eq!(count(&plain, 1), 0);
        assert!(count(&outlined, 1) > count(&outlined, 0) * 3 / 2);
        assert!((count(&outlined, 0) as i64 - count(&plain, 0) as i64).abs() < 8);
    }
}
