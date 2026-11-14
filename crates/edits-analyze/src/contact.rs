//! Labelled image grids for an agent to look at: the per-shot strips of the
//! vision pass and the reference-versus-render pairs of `compare --sheet`.
//! Labels use the built-in 5x7 font (`edits_core::glyphs`), so no font
//! files are needed.

use crate::img::Frames;
use anyhow::{Context, Result};
use image::{imageops, Rgb, RgbImage};
use std::path::Path;

pub const BACKGROUND: [u8; 3] = [18, 18, 20];
const STRIP: [u8; 3] = [34, 34, 38];
const TEXT: [u8; 3] = [232, 232, 226];

/// One labelled picture. `accent` colours a bar above the label (used to
/// tell the reference from the render, or to mark a flagged frame).
pub struct Cell {
    pub image: RgbImage,
    pub label: String,
    pub accent: Option<[u8; 3]>,
}

/// Frame `i` of decoded frames as an image.
pub fn frame_image(frames: &Frames, i: usize) -> RgbImage {
    RgbImage::from_raw(frames.w as u32, frames.h as u32, frames.rgb(i).to_vec()).expect("frame buffer size")
}

/// Resize to `height` pixels high, keeping the aspect ratio.
pub fn fit_height(img: &RgbImage, height: u32) -> RgbImage {
    let w = ((img.width() as f64 * height as f64 / img.height().max(1) as f64).round() as u32).max(1);
    if (w, height) == img.dimensions() {
        return img.clone();
    }
    imageops::resize(img, w, height, imageops::FilterType::Triangle)
}

/// A `side` x `side` crop at pixel (`x`, `y`) (top left), clamped inside the image.
pub fn crop(img: &RgbImage, x: i64, y: i64, side: u32) -> RgbImage {
    let side = side.min(img.width()).min(img.height());
    let x = x.clamp(0, (img.width() - side) as i64) as u32;
    let y = y.clamp(0, (img.height() - side) as i64) as u32;
    imageops::crop_imm(img, x, y, side, side).to_image()
}

/// Draw `text` with its top-left corner at (`x`, `y`).
pub fn draw_text(img: &mut RgbImage, x: i32, y: i32, scale: u32, color: [u8; 3], text: &str) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    edits_core::glyphs::for_each_pixel(text, x, y, scale, |px, py| {
        if px >= 0 && py >= 0 && px < w && py < h {
            img.put_pixel(px as u32, py as u32, Rgb(color));
        }
    });
}

fn fill(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, c: [u8; 3]) {
    for py in y..(y + h).min(img.height()) {
        for px in x..(x + w).min(img.width()) {
            img.put_pixel(px, py, Rgb(c));
        }
    }
}

/// Font scale for labels under cells `width` pixels wide holding `longest` characters.
fn label_scale(width: u32, longest: usize) -> u32 {
    let fit = width.saturating_sub(4) / (longest.max(1) as u32 * edits_core::glyphs::ADVANCE);
    fit.clamp(1, 3).min((width / 110).max(1))
}

/// Lay `cells` out in rows of `cols`, each cell with its label underneath.
/// Cells may differ in width; every row is as tall as its tallest cell.
/// `title`, when given, is written across the top.
pub fn compose(cells: &[Cell], cols: usize, gap: u32, title: Option<&str>) -> RgbImage {
    let cols = cols.clamp(1, cells.len().max(1));
    let longest = cells.iter().map(|c| c.label.chars().count()).max().unwrap_or(1);
    let min_w = cells.iter().map(|c| c.image.width()).min().unwrap_or(64);
    let scale = label_scale(min_w, longest);
    let strip = edits_core::glyphs::GLYPH_H * scale + 2 * (scale + 1);
    let accent_h = 3u32;
    let rows: Vec<&[Cell]> = cells.chunks(cols).collect();
    let row_w: Vec<u32> = rows.iter().map(|r| r.iter().map(|c| c.image.width() + gap).sum::<u32>() + gap).collect();
    let row_h: Vec<u32> = rows.iter().map(|r| r.iter().map(|c| c.image.height()).max().unwrap_or(0) + accent_h + strip + gap).collect();
    let title_h = if title.is_some() { edits_core::glyphs::GLYPH_H * 2 + 8 } else { 0 };
    let width = row_w.iter().copied().max().unwrap_or(2 * gap).max(2 * gap);
    let height = gap + title_h + row_h.iter().sum::<u32>();
    let mut sheet = RgbImage::from_pixel(width, height, Rgb(BACKGROUND));
    if let Some(t) = title {
        draw_text(&mut sheet, gap as i32 + 2, gap as i32 + 2, 2, TEXT, t);
    }
    let mut y = gap + title_h;
    for (r, row) in rows.iter().enumerate() {
        let mut x = gap;
        let ch = row.iter().map(|c| c.image.height()).max().unwrap_or(0);
        for cell in row.iter() {
            let cw = cell.image.width();
            imageops::replace(&mut sheet, &cell.image, x as i64, y as i64);
            fill(&mut sheet, x, y + ch, cw, accent_h, cell.accent.unwrap_or(STRIP));
            fill(&mut sheet, x, y + ch + accent_h, cw, strip, STRIP);
            draw_text(&mut sheet, (x + scale + 1) as i32, (y + ch + accent_h + scale + 1) as i32, scale, TEXT, &cell.label);
            x += cw + gap;
        }
        y += row_h[r];
    }
    sheet
}

/// Write an image: JPEG (quality 88) for `.jpg`/`.jpeg`, PNG otherwise.
pub fn save(img: &RgbImage, path: &Path) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "jpg" || ext == "jpeg" {
        let file = std::fs::File::create(path).with_context(|| format!("writing {}", path.display()))?;
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 88);
        enc.encode_image(img).with_context(|| format!("encoding {}", path.display()))?;
    } else {
        img.save(path).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_of_different_widths_are_laid_out_with_labels() {
        let cell = |w: u32, c: u8, l: &str| Cell { image: RgbImage::from_pixel(w, 30, Rgb([c, 0, 0])), label: l.into(), accent: Some([0, 200, 0]) };
        let sheet = compose(&[cell(40, 50, "A1"), cell(60, 100, "B2"), cell(40, 150, "C3")], 2, 4, Some("T"));
        let title_h = edits_core::glyphs::GLYPH_H * 2 + 8;
        assert_eq!(sheet.width(), 4 + 44 + 64);
        assert_eq!(sheet.get_pixel(10, 4 + title_h + 10).0[0], 50);
        assert_eq!(sheet.get_pixel(4 + 44 + 10, 4 + title_h + 10).0[0], 100);
        // The accent bar under the first cell.
        assert_eq!(sheet.get_pixel(10, 4 + title_h + 30).0, [0, 200, 0]);
        let lit = sheet.pixels().filter(|p| p.0 == TEXT).count();
        assert!(lit > 20, "{lit}");
    }

    #[test]
    fn crops_stay_inside() {
        let img = RgbImage::from_pixel(100, 80, Rgb([1, 2, 3]));
        assert_eq!(crop(&img, 90, 70, 50).dimensions(), (50, 50));
        assert_eq!(crop(&img, -5, 0, 500).dimensions(), (80, 80));
        assert_eq!(fit_height(&img, 40).dimensions(), (50, 40));
    }
}
