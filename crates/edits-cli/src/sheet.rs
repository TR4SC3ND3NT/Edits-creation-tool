//! Contact sheets: a grid of frames, each labelled with its frame number and
//! project time, so a whole edit can be read in one picture.

use edits_render::font;
use image::{imageops, Rgba, RgbaImage};

pub struct Cell {
    pub frame: u64,
    pub label: String,
    pub image: RgbaImage,
}

const GAP: u32 = 4;
const BACKGROUND: Rgba<u8> = Rgba([18, 18, 20, 255]);
const STRIP: Rgba<u8> = Rgba([34, 34, 38, 255]);
const TEXT: [u8; 4] = [232, 232, 226, 255];

/// Font scale and label strip height for cells `width` pixels wide.
fn label_metrics(width: u32, longest: usize) -> (u32, u32) {
    let fit = width.saturating_sub(4) / (longest.max(1) as u32 * font::ADVANCE);
    let scale = fit.clamp(1, 3).min((width / 150).max(1));
    (scale, font::GLYPH_H * scale + 2 * (scale + 1))
}

/// Lay `cells` out in `cols` columns. All images must have the same size.
pub fn compose(cells: &[Cell], cols: u32) -> RgbaImage {
    let cols = cols.clamp(1, cells.len().max(1) as u32);
    let rows = (cells.len() as u32).div_ceil(cols).max(1);
    let (cw, ch) = cells.first().map(|c| c.image.dimensions()).unwrap_or((2, 2));
    let longest = cells.iter().map(|c| c.label.chars().count()).max().unwrap_or(1);
    let (scale, strip) = label_metrics(cw, longest);
    let (step_x, step_y) = (cw + GAP, ch + strip + GAP);
    let mut sheet = RgbaImage::from_pixel(GAP + cols * step_x, GAP + rows * step_y, BACKGROUND);
    for (i, cell) in cells.iter().enumerate() {
        let (x, y) = (GAP + (i as u32 % cols) * step_x, GAP + (i as u32 / cols) * step_y);
        imageops::replace(&mut sheet, &cell.image, x as i64, y as i64);
        for py in y + ch..y + ch + strip {
            for px in x..x + cw {
                sheet.put_pixel(px, py, STRIP);
            }
        }
        font::draw_text(&mut sheet, (x + scale + 1) as i32, (y + ch + scale + 1) as i32, scale, TEXT, &cell.label);
    }
    sheet
}

/// `count` frame numbers spread evenly over `first..end`, first and last included.
pub fn spread(first: u64, end: u64, count: u64) -> Vec<u64> {
    let n = end.saturating_sub(first);
    if n == 0 {
        return Vec::new();
    }
    let count = count.clamp(1, n);
    if count == 1 {
        return vec![first];
    }
    (0..count).map(|i| first + ((i as f64) * (n - 1) as f64 / (count - 1) as f64).round() as u64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_includes_both_ends_without_repeats() {
        assert_eq!(spread(0, 98, 4), vec![0, 32, 65, 97]);
        assert_eq!(spread(10, 13, 8), vec![10, 11, 12]);
        assert_eq!(spread(5, 6, 3), vec![5]);
        assert!(spread(5, 5, 3).is_empty());
    }

    #[test]
    fn cells_are_laid_out_in_rows_with_labels() {
        let cell = |frame: u64, c: u8| Cell { frame, label: format!("#{frame}"), image: RgbaImage::from_pixel(40, 20, Rgba([c, 0, 0, 255])) };
        let sheet = compose(&[cell(0, 50), cell(1, 100), cell(2, 150)], 2);
        let (_, strip) = label_metrics(40, 2);
        assert_eq!(sheet.dimensions(), (4 + 2 * 44, 4 + 2 * (20 + strip + 4)));
        assert_eq!(sheet.get_pixel(10, 10).0[0], 50);
        assert_eq!(sheet.get_pixel(54, 10).0[0], 100);
        assert_eq!(sheet.get_pixel(10, 10 + 20 + strip + 4).0[0], 150);
        // The label strip under the first cell has text pixels in it.
        let lit = (4..44).flat_map(|x| (24..24 + strip).map(move |y| (x, y))).filter(|(x, y)| sheet.get_pixel(*x, *y).0 == TEXT).count();
        assert!(lit > 10, "{lit}");
    }
}
