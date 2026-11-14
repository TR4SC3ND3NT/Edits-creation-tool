//! Turning a rendered PNG into an image content block that is cheap enough
//! to send to a model.
//!
//! On Claude an image costs about `ceil(w / 28) * ceil(h / 28)` tokens, and a
//! client such as Claude Code caps one tool result at 25,000 tokens. A result
//! from this server stays far below that: an image is scaled down until it
//! fits a token budget, and re-encoded as JPEG when the PNG would be large
//! (noisy textures make PNGs of several megabytes).

use base64::Engine;
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;
use std::path::Path;

/// Most tokens one returned image may cost. Also what Claude itself keeps of
/// one image at its highest resolution tier, so nothing useful is lost.
pub const MAX_TOKENS: u32 = 4_800;
/// Largest encoded image sent (before base64).
pub const MAX_BYTES: usize = 900_000;

/// Approximate cost of an image on Claude, in tokens.
pub fn tokens(width: u32, height: u32) -> u32 {
    width.div_ceil(28) * height.div_ceil(28)
}

/// The largest scale (at most 1) at which a `width` x `height` image fits `budget` tokens.
pub fn fit_scale(width: u32, height: u32, budget: u32) -> f64 {
    if tokens(width, height) <= budget {
        return 1.0;
    }
    let mut scale = (f64::from(budget) / f64::from(tokens(width, height))).sqrt();
    // The estimate ignores rounding up to whole 28 px patches; step down until it holds.
    while scale > 0.01 {
        let (w, h) = scaled(width, height, scale);
        if tokens(w, h) <= budget {
            break;
        }
        scale *= 0.97;
    }
    scale
}

fn scaled(width: u32, height: u32, scale: f64) -> (u32, u32) {
    (((f64::from(width) * scale).round() as u32).max(1), ((f64::from(height) * scale).round() as u32).max(1))
}

#[derive(Debug)]
pub struct Prepared {
    pub base64: String,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    pub tokens: u32,
    pub bytes: usize,
    /// The size on disk, when the copy that is sent was scaled down.
    pub scaled_from: Option<(u32, u32)>,
}

impl Prepared {
    /// One sentence about what is being sent, for the text block beside the image.
    pub fn note(&self) -> String {
        let kind = if self.mime == "image/png" { "PNG" } else { "JPEG" };
        let from = self.scaled_from.map(|(w, h)| format!(", scaled down from {w}x{h}")).unwrap_or_default();
        format!("{}x{} {kind}, about {} image tokens{from}", self.width, self.height, self.tokens)
    }
}

fn encode(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    match format {
        ImageFormat::Jpeg => {
            let rgb = image.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82).encode_image(&rgb).map_err(|e| e.to_string())?;
        }
        _ => image.write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?,
    }
    Ok(out.into_inner())
}

/// Load a PNG (or a JPEG, such as the sheets `edits-analyze` writes) and make it fit the budgets.
pub fn prepare(path: &Path, max_tokens: u32, max_bytes: usize) -> Result<Prepared, String> {
    let file = std::fs::read(path).map_err(|e| format!("cannot read the rendered image {}: {e}", path.display()))?;
    let format = image::guess_format(&file).unwrap_or(ImageFormat::Png);
    let format = if format == ImageFormat::Jpeg { ImageFormat::Jpeg } else { ImageFormat::Png };
    let image = image::load_from_memory_with_format(&file, format).map_err(|e| format!("the rendered image {} cannot be decoded: {e}", path.display()))?;
    let original = (image.width(), image.height());
    let scale = fit_scale(original.0, original.1, max_tokens);
    let mut current = if scale < 1.0 {
        let (w, h) = scaled(original.0, original.1, scale);
        image.resize_exact(w, h, image::imageops::FilterType::CatmullRom)
    } else {
        image
    };
    // The file itself when nothing had to change; otherwise the smallest
    // encoding that fits, shrinking further only as a last resort.
    let (bytes, mime) = loop {
        let unchanged = (current.width(), current.height()) == original;
        if unchanged && file.len() <= max_bytes {
            break (file.clone(), if format == ImageFormat::Jpeg { "image/jpeg" } else { "image/png" });
        }
        if !unchanged {
            let png = encode(&current, ImageFormat::Png)?;
            if png.len() <= max_bytes {
                break (png, "image/png");
            }
        }
        let jpeg = encode(&current, ImageFormat::Jpeg)?;
        if jpeg.len() <= max_bytes || current.width() <= 64 || current.height() <= 64 {
            break (jpeg, "image/jpeg");
        }
        let (w, h) = scaled(current.width(), current.height(), 0.8);
        current = current.resize_exact(w, h, image::imageops::FilterType::CatmullRom);
    };
    let (width, height) = (current.width(), current.height());
    Ok(Prepared {
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        mime,
        width,
        height,
        tokens: tokens(width, height),
        bytes: bytes.len(),
        scaled_from: ((width, height) != original).then_some(original),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise_png(dir: &Path, name: &str, w: u32, h: u32) -> std::path::PathBuf {
        // Noise is the worst case for PNG: it does not compress.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let img = image::RgbaImage::from_fn(w, h, |_, _| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            image::Rgba([(state >> 8) as u8, (state >> 16) as u8, (state >> 24) as u8, 255])
        });
        let path = dir.join(name);
        img.save(&path).unwrap();
        path
    }

    #[test]
    fn token_arithmetic() {
        assert_eq!(tokens(540, 540), 400);
        assert_eq!(tokens(1920, 1080), 69 * 39);
        assert_eq!(fit_scale(540, 540, MAX_TOKENS), 1.0);
        for (w, h) in [(4000, 4000), (2580, 2740), (1300, 5000), (9000, 900)] {
            let s = fit_scale(w, h, MAX_TOKENS);
            let (sw, sh) = scaled(w, h, s);
            assert!(tokens(sw, sh) <= MAX_TOKENS, "{w}x{h} -> {sw}x{sh}");
            assert!(tokens(sw, sh) > MAX_TOKENS * 8 / 10, "{w}x{h} is not shrunk more than needed");
        }
    }

    #[test]
    fn small_images_pass_through_and_large_ones_are_capped() {
        let dir = std::env::temp_dir().join(format!("edits-mcp-images-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let small = image::RgbaImage::from_pixel(320, 180, image::Rgba([200, 30, 30, 255]));
        let path = dir.join("small.png");
        small.save(&path).unwrap();
        let p = prepare(&path, MAX_TOKENS, MAX_BYTES).unwrap();
        assert_eq!((p.width, p.height, p.mime, p.scaled_from), (320, 180, "image/png", None));
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(&p.base64).unwrap(), std::fs::read(&path).unwrap(), "an image within budget is sent as rendered");

        let big = noise_png(&dir, "big.png", 2600, 2000);
        let p = prepare(&big, MAX_TOKENS, MAX_BYTES).unwrap();
        assert!(p.tokens <= MAX_TOKENS && p.bytes <= MAX_BYTES, "{p:?}");
        assert_eq!(p.scaled_from, Some((2600, 2000)));
        assert_eq!(p.mime, "image/jpeg", "noise does not fit as PNG");
        let decoded = image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(&p.base64).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (p.width, p.height));
        assert!(p.note().contains("scaled down from 2600x2000"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
