//! A tiny built-in 5x7 bitmap font, so labels need no font files. Capitals,
//! digits and common punctuation; lower-case letters are drawn as capitals.
//! The glyphs are plain data in `edits_core::glyphs`.

use image::{Rgba, RgbaImage};

pub use edits_core::glyphs::{text_width, ADVANCE, GLYPH_H, GLYPH_W};

/// Draw `text` with its top-left corner at (`x`, `y`). Each font pixel is a
/// `scale` x `scale` square. Pixels outside the image are skipped.
pub fn draw_text(img: &mut RgbaImage, x: i32, y: i32, scale: u32, color: [u8; 4], text: &str) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    edits_core::glyphs::for_each_pixel(text, x, y, scale, |px, py| {
        if px >= 0 && py >= 0 && px < w && py < h {
            img.put_pixel(px as u32, py as u32, Rgba(color));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_drawn_and_clipped() {
        let mut img = RgbaImage::new(20, 9);
        draw_text(&mut img, 1, 1, 1, [255, 255, 255, 255], "f1 xyz long text");
        assert_eq!(img.get_pixel(1, 1).0, [255, 255, 255, 255]); // top-left of F
        assert_eq!(img.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(text_width("ab", 2), 22);
    }
}
