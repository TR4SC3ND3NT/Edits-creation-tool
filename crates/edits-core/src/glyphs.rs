//! A tiny 5x7 bitmap font as plain data, so any crate can draw labels on
//! images without font files or a renderer: capitals, digits and common
//! punctuation; lower-case letters are drawn as capitals. `edits-render`
//! draws it into images (`font::draw_text`), `edits-analyze` labels the
//! frames it exports for an agent to look at.

pub const GLYPH_W: u32 = 5;
pub const GLYPH_H: u32 = 7;
/// Horizontal distance from one character to the next, at scale 1.
pub const ADVANCE: u32 = 6;

pub const GLYPHS: &[(char, &str)] = &[
    ('A', ".###. #...# #...# ##### #...# #...# #...#"),
    ('B', "####. #...# #...# ####. #...# #...# ####."),
    ('C', ".###. #...# #.... #.... #.... #...# .###."),
    ('D', "####. #...# #...# #...# #...# #...# ####."),
    ('E', "##### #.... #.... ####. #.... #.... #####"),
    ('F', "##### #.... #.... ####. #.... #.... #...."),
    ('G', ".###. #...# #.... #.### #...# #...# .###."),
    ('H', "#...# #...# #...# ##### #...# #...# #...#"),
    ('I', ".###. ..#.. ..#.. ..#.. ..#.. ..#.. .###."),
    ('J', "..### ...#. ...#. ...#. ...#. #..#. .##.."),
    ('K', "#...# #..#. #.#.. ##... #.#.. #..#. #...#"),
    ('L', "#.... #.... #.... #.... #.... #.... #####"),
    ('M', "#...# ##.## #.#.# #.#.# #...# #...# #...#"),
    ('N', "#...# ##..# #.#.# #..## #...# #...# #...#"),
    ('O', ".###. #...# #...# #...# #...# #...# .###."),
    ('P', "####. #...# #...# ####. #.... #.... #...."),
    ('Q', ".###. #...# #...# #...# #.#.# #..#. .##.#"),
    ('R', "####. #...# #...# ####. #.#.. #..#. #...#"),
    ('S', ".#### #.... #.... .###. ....# ....# ####."),
    ('T', "##### ..#.. ..#.. ..#.. ..#.. ..#.. ..#.."),
    ('U', "#...# #...# #...# #...# #...# #...# .###."),
    ('V', "#...# #...# #...# #...# #...# .#.#. ..#.."),
    ('W', "#...# #...# #...# #.#.# #.#.# ##.## #...#"),
    ('X', "#...# #...# .#.#. ..#.. .#.#. #...# #...#"),
    ('Y', "#...# #...# .#.#. ..#.. ..#.. ..#.. ..#.."),
    ('Z', "##### ....# ...#. ..#.. .#... #.... #####"),
    ('0', ".###. #...# #..## #.#.# ##..# #...# .###."),
    ('1', "..#.. .##.. ..#.. ..#.. ..#.. ..#.. .###."),
    ('2', ".###. #...# ....# ...#. ..#.. .#... #####"),
    ('3', "##### ...#. ..#.. ...#. ....# #...# .###."),
    ('4', "...#. ..##. .#.#. #..#. ##### ...#. ...#."),
    ('5', "##### #.... ####. ....# ....# #...# .###."),
    ('6', "..##. .#... #.... ####. #...# #...# .###."),
    ('7', "##### ....# ...#. ..#.. .#... .#... .#..."),
    ('8', ".###. #...# #...# .###. #...# #...# .###."),
    ('9', ".###. #...# #...# .#### ....# ...#. .##.."),
    ('.', "..... ..... ..... ..... ..... .##.. .##.."),
    (',', "..... ..... ..... ..... .##.. ..#.. .#..."),
    (':', "..... .##.. .##.. ..... .##.. .##.. ....."),
    ('=', "..... ..... ##### ..... ##### ..... ....."),
    ('-', "..... ..... ..... ##### ..... ..... ....."),
    ('+', "..... ..#.. ..#.. ##### ..#.. ..#.. ....."),
    ('_', "..... ..... ..... ..... ..... ..... #####"),
    ('/', "....# ....# ...#. ..#.. .#... #.... #...."),
    ('%', "##..# ##..# ...#. ..#.. .#... #..## #..##"),
    ('(', "...#. ..#.. .#... .#... .#... ..#.. ...#."),
    (')', ".#... ..#.. ...#. ...#. ...#. ..#.. .#..."),
    ('[', ".###. .#... .#... .#... .#... .#... .###."),
    (']', ".###. ...#. ...#. ...#. ...#. ...#. .###."),
    ('#', ".#.#. .#.#. ##### .#.#. ##### .#.#. .#.#."),
    ('!', "..#.. ..#.. ..#.. ..#.. ..#.. ..... ..#.."),
    ('?', ".###. #...# ....# ...#. ..#.. ..... ..#.."),
    ('\'', "..#.. ..#.. .#... ..... ..... ..... ....."),
    ('"', ".#.#. .#.#. .#.#. ..... ..... ..... ....."),
    ('<', "...#. ..#.. .#... #.... .#... ..#.. ...#."),
    ('>', ".#... ..#.. ...#. ....# ...#. ..#.. .#..."),
    ('*', "..... #.#.# .###. ##### .###. #.#.# ....."),
];
pub const UNKNOWN: &str = "##### #...# #...# #...# #...# #...# #####";

/// The seven rows of `c` (`#` = ink, `.` = paper), separated by spaces;
/// `None` for a space. Unknown characters are a hollow box.
pub fn glyph(c: char) -> Option<&'static str> {
    if c == ' ' {
        return None;
    }
    let c = c.to_ascii_uppercase();
    Some(GLYPHS.iter().find(|(g, _)| *g == c).map(|(_, rows)| *rows).unwrap_or(UNKNOWN))
}

/// Width in pixels of `text` drawn at `scale`.
pub fn text_width(text: &str, scale: u32) -> u32 {
    (text.chars().count() as u32 * ADVANCE).saturating_sub(1) * scale
}

/// Calls `ink(x, y)` for every ink pixel of `text` with its top-left corner
/// at (`x`, `y`), each font pixel a `scale` x `scale` square. The caller clips.
pub fn for_each_pixel(text: &str, x: i32, y: i32, scale: u32, mut ink: impl FnMut(i32, i32)) {
    let scale = scale.max(1) as i32;
    for (i, c) in text.chars().enumerate() {
        let Some(rows) = glyph(c) else { continue };
        let gx = x + i as i32 * ADVANCE as i32 * scale;
        for (row, bits) in rows.split(' ').enumerate() {
            for (col, bit) in bits.bytes().enumerate() {
                if bit != b'#' {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        ink(gx + col as i32 * scale + dx, y + row as i32 * scale + dy);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_five_by_seven() {
        for (c, rows) in GLYPHS.iter().copied().chain([('?', UNKNOWN)]) {
            let rows: Vec<&str> = rows.split(' ').collect();
            assert_eq!(rows.len(), GLYPH_H as usize, "{c}");
            assert!(rows.iter().all(|r| r.len() == GLYPH_W as usize && r.bytes().all(|b| b == b'#' || b == b'.')), "{c}");
        }
    }

    #[test]
    fn pixels_follow_the_glyph() {
        let mut n = 0;
        for_each_pixel("-", 0, 0, 1, |x, y| {
            assert_eq!(y, 3);
            assert!((0..5).contains(&x));
            n += 1;
        });
        assert_eq!(n, 5);
        assert_eq!(text_width("ab", 2), 22);
    }
}
