//! Static layers that sit on top of every shot: an overlay plate (ornaments,
//! dust, handwriting) and a vignette.
//!
//! Overlay: a low per-pixel temporal percentile over many frames keeps what
//! is always there. The pictures underneath drop out wherever one of them is
//! dark, so a white top-hat (the percentile image minus its morphological
//! opening) isolates the thin bright
//! overlay strokes; only the part common to the three channels is kept (the
//! strokes are neutral, the residue of the pictures is coloured) and dividing
//! by `1 - background` undoes a screen blend.
//!
//! Vignette: the brightest frames (exposure flashes) are close to a white
//! field, so their radial brightness profile is the vignette. A vignette can
//! only darken, so the profile of the brightest value each pixel reaches
//! over all the content frames is taken too, and the brighter of the two
//! profiles is kept, ring by ring (inset cards on black and dark art make
//! either one alone read too dark). It is fitted in linear light with
//! `1 - amount * smoothstep(radius, radius + softness, r)`, `r` being the
//! distance from the centre as a fraction of the half-diagonal, which is the
//! renderer's formula (`edits-render` compose.rs, VIGNETTE).
//!
//! Texture: fine detail (log luminance minus its blur) that is the same in
//! every picture. Averaged over one frame per slot (different pictures),
//! picture detail cancels and a static texture (paper, handwriting, grain,
//! halftone; lighter or darker than the pictures) survives. Its share of the
//! detail energy, corrected for what survives by chance with that many
//! pictures, is the texture's presence. An overlay that is light on dark
//! pictures and dark on light ones (a `difference` blend) cancels in that
//! average, so the detail is also averaged with its sign turned where the
//! picture underneath is light.

use crate::img::{gaussian_blur, median, srgb_lut, Frames, Gray};
use crate::schema::{Est, Ident, OverlayInfo, TextureInfo, VignetteInfo};
use crate::signals::FrameSig;
use rayon::prelude::*;

pub struct Plate {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
}

/// Per-pixel, per-channel temporal percentile (`q` in 0..1) over the listed frames.
pub fn temporal_percentile(frames: &Frames, idx: &[usize], q: f64) -> Vec<u8> {
    let n = frames.w * frames.h * 3;
    let k = idx.len();
    let mut out = vec![0u8; n];
    if k == 0 {
        return out;
    }
    let pick = ((k - 1) as f64 * q).round() as usize;
    let row = frames.w * 3;
    out.par_chunks_mut(row).enumerate().for_each(|(y, dst)| {
        let mut vals = vec![0u8; k];
        for (x, d) in dst.iter_mut().enumerate() {
            for (j, f) in idx.iter().enumerate() {
                vals[j] = frames.rgb(*f)[y * row + x];
            }
            *d = *vals.select_nth_unstable(pick).1;
        }
    });
    out
}

fn minmax_filter(src: &[u8], w: usize, h: usize, r: usize, take_max: bool) -> Vec<u8> {
    let pick = |a: u8, b: u8| if take_max { a.max(b) } else { a.min(b) };
    let init = if take_max { 0u8 } else { 255u8 };
    let mut tmp = vec![0u8; src.len()];
    tmp.par_chunks_mut(w * 3).enumerate().for_each(|(y, dst)| {
        for x in 0..w {
            for c in 0..3 {
                let mut v = init;
                for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                    v = pick(v, src[(y * w + xx) * 3 + c]);
                }
                dst[x * 3 + c] = v;
            }
        }
    });
    let mut out = vec![0u8; src.len()];
    out.par_chunks_mut(w * 3).enumerate().for_each(|(y, dst)| {
        for x in 0..w {
            for c in 0..3 {
                let mut v = init;
                for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                    v = pick(v, tmp[(yy * w + x) * 3 + c]);
                }
                dst[x * 3 + c] = v;
            }
        }
    });
    out
}

/// The overlay plate from the frames listed in `idx` (at the frames' resolution).
pub fn overlay_plate(frames: &Frames, idx: &[usize]) -> Plate {
    let (w, h) = (frames.w, frames.h);
    // A screen-blended overlay adds light: a pixel is never darker than the
    // plate there. The darkest tenth of the frames therefore shows the plate
    // almost bare, wherever any of the pictures is dark at some time; the
    // median (used before) keeps a stroke only where most frames are dark,
    // and keeps thin bright detail of a picture that is on screen for half
    // of the clip (`edits calib`: a plate was "found" in 16 of 19 clips
    // without one, and dust over bright pictures was missed).
    let med = temporal_percentile(frames, idx, 0.1);
    // Structuring element a little wider than a pen stroke.
    let r = ((h as f64 * 0.008).round() as usize).max(2);
    let opened = minmax_filter(&minmax_filter(&med, w, h, r, false), w, h, r, true);
    let mut rgb = vec![0u8; med.len()];
    for (i, px) in rgb.chunks_exact_mut(3).enumerate() {
        // The strokes are neutral (white ink); what is left of the averaged
        // pictures is coloured. Keep the part common to all three channels.
        let mut hat = f32::MAX;
        let mut bg = 0.0f32;
        for c in 0..3 {
            hat = hat.min(med[i * 3 + c].saturating_sub(opened[i * 3 + c]) as f32 / 255.0);
            bg += opened[i * 3 + c] as f32 / 255.0 / 3.0;
        }
        // Drop the residue, then undo the screen blend against the local background.
        let hat = (hat - 0.05).max(0.0);
        let v = ((hat / (1.0 - bg.min(0.8))).min(1.0) * 255.0).round() as u8;
        px.copy_from_slice(&[v, v, v]);
    }
    Plate { w, h, rgb }
}

fn luma(p: &[u8]) -> f32 {
    (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0
}

/// Summary of a plate; `None` when there is no overlay to speak of.
pub fn overlay_info(plate: &Plate) -> Option<OverlayInfo> {
    let n = plate.w * plate.h;
    let (mut covered, mut sum) = (0usize, 0.0f64);
    for p in plate.rgb.chunks_exact(3) {
        let l = luma(p);
        if l > 0.1 {
            covered += 1;
            sum += l as f64;
        }
    }
    let coverage = covered as f64 / n as f64;
    if coverage < 0.002 {
        return None;
    }
    Some(OverlayInfo {
        path: None,
        coverage: Est::measured(coverage, "fraction of frame", 0.6),
        strength: Est::new(sum / covered as f64, "luminance 0..1", 0.4, Ident::Qualitative),
        blend_guess: "screen".into(),
        method: "white top-hat of the temporal median, un-screened; blend mode and opacity are a guess".into(),
    })
}

/// Pixels that alignment may use: everything except the overlay strokes (dilated).
pub fn usable_mask(plate: &Plate) -> Vec<bool> {
    let (w, h) = (plate.w, plate.h);
    let hit: Vec<bool> = plate.rgb.chunks_exact(3).map(|p| luma(p) > 0.1).collect();
    let r = ((h as f64 * 0.006).round() as usize).max(1);
    let mut out = vec![true; w * h];
    for y in 0..h {
        for x in 0..w {
            if hit[y * w + x] {
                for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                    for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                        out[yy * w + xx] = false;
                    }
                }
            }
        }
    }
    out
}

const PROFILE_BINS: usize = 21;

/// Fit `1 - amount * smoothstep(radius, radius + softness, r)` to a radial profile.
pub fn fit_vignette(profile: &[f64]) -> (f64, f64, f64, f64) {
    let n = profile.len();
    let rs: Vec<f64> = (0..n).map(|i| i as f64 / (n - 1) as f64).collect();
    let mut best = (f64::MAX, 0.0, 0.75, 0.5);
    let mut radius = 0.0;
    while radius <= 0.9 {
        let mut soft = 0.1;
        while soft <= 1.5 {
            let s: Vec<f64> = rs.iter().map(|r| crate::synth::smoothstep(radius, radius + soft, *r)).collect();
            // Least squares for amount: (1 - p) = amount * s, weighted by ring area.
            let (mut num, mut den) = (0.0, 0.0);
            for i in 0..n {
                let wgt = rs[i].max(0.02);
                num += wgt * s[i] * (1.0 - profile[i]);
                den += wgt * s[i] * s[i];
            }
            let amount = if den > 1e-9 { (num / den).clamp(0.0, 1.0) } else { 0.0 };
            let err: f64 = (0..n).map(|i| rs[i].max(0.02) * (1.0 - amount * s[i] - profile[i]).powi(2)).sum();
            if err < best.0 {
                best = (err, amount, radius, soft);
            }
            soft += 0.05;
        }
        radius += 0.05;
    }
    let wsum: f64 = rs.iter().map(|r| r.max(0.02)).sum();
    ((best.0 / wsum).sqrt(), best.1, best.2, best.3)
}

/// Radial brightness profile and fitted vignette over frames `f0..f1`.
pub fn vignette(frames: &Frames, sigs: &[FrameSig], f0: usize, f1: usize) -> Option<VignetteInfo> {
    let (w, h) = (frames.w, frames.h);
    let mut order: Vec<usize> = (f0..f1).collect();
    if order.len() < 4 {
        return None;
    }
    order.sort_by(|a, b| sigs[*b].lum_center.partial_cmp(&sigs[*a].lum_center).unwrap());
    let k = (order.len() / 16).clamp(4, 24).min(order.len());
    let bright = &order[..k];
    let centre_lum = bright.iter().map(|i| sigs[*i].lum_center as f64).sum::<f64>() / k as f64;
    let white_field = centre_lum > 170.0;
    // Upper-quartile per pixel across the bright frames: ink that survives a
    // flash sits at different places in different frames.
    let field = temporal_percentile(frames, bright, 0.75);
    // The brightest each pixel gets over all the content frames (90th percentile).
    let content: Vec<usize> = (f0..f1).filter(|i| !crate::ingest::is_blank(&sigs[*i])).collect();
    let stride = (content.len() / 60).max(1);
    let sample: Vec<usize> = content.iter().copied().step_by(stride).collect();
    let ceiling = if sample.len() >= 8 { Some(temporal_percentile(frames, &sample, 0.9)) } else { None };
    let lut = srgb_lut();
    let (cx, cy) = ((w as f64 - 1.0) / 2.0, (h as f64 - 1.0) / 2.0);
    let half_diag = (cx * cx + cy * cy).sqrt();
    let profile_of = |field: &[u8]| -> Option<Vec<f64>> {
        let mut bins: Vec<Vec<f64>> = vec![Vec::new(); PROFILE_BINS];
        for y in 0..h {
            for x in 0..w {
                let p = &field[(y * w + x) * 3..(y * w + x) * 3 + 3];
                let l = 0.2126 * lut[p[0] as usize] + 0.7152 * lut[p[1] as usize] + 0.0722 * lut[p[2] as usize];
                let r = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt() / half_diag;
                let b = ((r * (PROFILE_BINS - 1) as f64).round() as usize).min(PROFILE_BINS - 1);
                bins[b].push(l as f64);
            }
        }
        let raw: Vec<f64> = bins.iter().map(|b| median(b)).collect();
        let centre = (raw[0] + raw[1] + raw[2]) / 3.0;
        (centre >= 0.02).then(|| raw.iter().map(|v| (v / centre).min(1.05)).collect())
    };
    let from_bright = profile_of(&field);
    let from_all = ceiling.as_deref().and_then(profile_of);
    let profile: Vec<f64> = match (from_bright, from_all) {
        (Some(a), Some(b)) => a.iter().zip(&b).map(|(x, y)| x.max(*y)).collect(),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => return None,
    };
    let profile: Vec<f64> = profile.iter().map(|v| (v * 1000.0).round() / 1000.0).collect();
    let (rms, amount, radius, softness) = fit_vignette(&profile);
    let corner = profile[PROFILE_BINS - 1];
    if amount < 0.05 {
        return None;
    }
    let conf = if white_field { (0.85 - 3.0 * rms).clamp(0.2, 0.85) } else { 0.2 };
    let ident = if white_field { Ident::Measured } else { Ident::Qualitative };
    Some(VignetteInfo {
        profile,
        amount: Est::new(amount, "0..1", conf, ident),
        radius: Est::new(radius, "fraction of half-diagonal", conf * 0.8, ident),
        softness: Est::new(softness, "fraction of half-diagonal", conf * 0.8, ident),
        corner_ratio: Est::new(corner, "linear-light ratio", conf, ident),
        method: if white_field {
            "radial profile in linear light: the brighter of the flash frames' profile and the 90th-percentile ceiling of all frames".into()
        } else {
            "no near-white frames: the brighter of the brightest frames' profile and the 90th-percentile ceiling of all frames, mixed with picture content".into()
        },
    })
}

/// A static texture found by [`texture`].
pub struct Texture {
    /// Share of the fine detail that is common to every picture, beyond
    /// chance (0..1), in the quarter of the frame where it is highest.
    pub presence: f64,
    /// RMS of the static detail where it is present, stops.
    pub strength: f64,
    /// Share of the frame where it is present.
    pub coverage: f64,
    /// `soft_light` (the texture darkens or lightens the same way on every
    /// picture) or `difference` (it is light on dark pictures and dark on
    /// light ones, as a difference-blended overlay is).
    pub blend: &'static str,
    /// The static detail per pixel, stops (signed for `soft_light`, the
    /// magnitude for `difference`).
    pub detail: Vec<f32>,
}

/// Static fine texture over one frame per picture (`groups`: frame indices
/// of different pictures; frames in one group are averaged first). `usable`
/// masks out the bright overlay strokes, which the overlay plate already
/// holds. Two readings are made and the stronger is kept: the detail as it
/// is, and the detail with its sign turned where the picture underneath is
/// light (an overlay that is light on dark frames and dark on light ones
/// cancels in the first and adds up in the second). `None` with fewer than 4 pictures.
pub fn texture(frames: &Frames, groups: &[Vec<usize>], usable: &[bool]) -> Option<Texture> {
    let (w, h) = (frames.w, frames.h);
    let groups: Vec<&Vec<usize>> = groups.iter().filter(|g| !g.is_empty()).collect();
    let n = groups.len();
    if n < 4 {
        return None;
    }
    let lut = srgb_lut();
    let sigma = (h as f32 * 0.006).max(1.0);
    // Detail of one frame, and the sign of its local background (+1 dark, -1 light).
    let detail = |f: usize| -> (Gray, Gray) {
        let rgb = frames.rgb(f);
        let mut g = Gray::new(w, h);
        let mut disp = Gray::new(w, h);
        for (i, p) in rgb.chunks_exact(3).enumerate() {
            let l = 0.2126 * lut[p[0] as usize] + 0.7152 * lut[p[1] as usize] + 0.0722 * lut[p[2] as usize];
            g.d[i] = (l + 0.04).log2();
            disp.d[i] = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0;
        }
        let b = gaussian_blur(&g, sigma);
        for (v, s) in g.d.iter_mut().zip(&b.d) {
            // Clipped: a picture's strong edges would otherwise outweigh a faint texture everywhere.
            *v = (*v - s).clamp(-0.4, 0.4);
        }
        let mut bg = gaussian_blur(&disp, 4.0 * sigma);
        bg.d.iter_mut().for_each(|v| *v = if *v < 0.5 { 1.0 } else { -1.0 });
        (g, bg)
    };
    let per: Vec<(Vec<f32>, Vec<f32>)> = groups
        .par_iter()
        .map(|g| {
            let (mut plain, mut aligned) = (vec![0f32; w * h], vec![0f32; w * h]);
            for f in g.iter() {
                let (d, bg) = detail(*f);
                for i in 0..w * h {
                    plain[i] += d.d[i] / g.len() as f32;
                    aligned[i] += d.d[i] * bg.d[i] / g.len() as f32;
                }
            }
            for i in 0..w * h {
                if !usable[i] {
                    plain[i] = 0.0;
                    aligned[i] = 0.0;
                }
            }
            (plain, aligned)
        })
        .collect();
    let count = usable.iter().filter(|u| **u).count().max(1) as f64;
    let chance = 1.0 / n as f64;
    let reading = |pick: &dyn Fn(&(Vec<f32>, Vec<f32>)) -> &Vec<f32>| -> Option<(f64, f64, f64, Vec<f32>)> {
        let mut mean = vec![0f32; w * h];
        for p in &per {
            for (m, v) in mean.iter_mut().zip(pick(p)) {
                *m += v / n as f32;
            }
        }
        let total: f64 = per.iter().map(|p| pick(p).iter().map(|v| (*v as f64).powi(2)).sum::<f64>()).sum::<f64>() / (n as f64 * count);
        let stat: f64 = mean.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / count;
        if total <= 1e-12 {
            return None;
        }
        // Overlays rarely cover the whole frame (a column of handwriting, an
        // ornament in a corner): read the share block by block on an 8 x 8
        // grid, and take the mean of the best quarter of the blocks.
        let mut blocks: Vec<(f64, f64, f64)> = Vec::with_capacity(64);
        let mut grid = [[0f64; 8]; 8];
        for by in 0..8 {
            for bx in 0..8 {
                let (mut st, mut to, mut k) = (0.0, 0.0, 0.0);
                for y in by * h / 8..(by + 1) * h / 8 {
                    for x in bx * w / 8..(bx + 1) * w / 8 {
                        if !usable[y * w + x] {
                            continue;
                        }
                        st += (mean[y * w + x] as f64).powi(2);
                        to += per.iter().map(|p| (pick(p)[y * w + x] as f64).powi(2)).sum::<f64>() / n as f64;
                        k += 1.0;
                    }
                }
                if k > 0.0 && to > 1e-12 {
                    let pr = ((st / to - chance) / (1.0 - chance)).clamp(0.0, 1.0);
                    grid[by][bx] = pr;
                    blocks.push((pr, ((st - to * chance).max(0.0) / (1.0 - chance) / k).sqrt(), k));
                }
            }
        }
        blocks.sort_by(|x, y| y.0.total_cmp(&x.0));
        let top = &blocks[..(blocks.len() / 4).max(1)];
        let presence = top.iter().map(|b| b.0).sum::<f64>() / top.len() as f64;
        let covered: Vec<&(f64, f64, f64)> = blocks.iter().filter(|b| b.0 > 0.06).collect();
        let coverage = covered.len() as f64 / blocks.len().max(1) as f64;
        let strength = if covered.is_empty() { 0.0 } else { (covered.iter().map(|b| b.1 * b.1 * b.2).sum::<f64>() / covered.iter().map(|b| b.2).sum::<f64>()).sqrt() };
        let _ = stat;
        // The plate keeps the detail where the blocks show a texture and fades the
        // pictures' residue elsewhere (block weights, bilinear between block centres).
        let wgt = |p: f64| ((p - 0.03) / 0.1).clamp(0.0, 1.0);
        let mut mean = mean;
        for y in 0..h {
            let gy = ((y as f64 + 0.5) * 8.0 / h as f64 - 0.5).clamp(0.0, 7.0);
            let (y0, fy) = (gy.floor() as usize, gy.fract());
            let y1 = (y0 + 1).min(7);
            for x in 0..w {
                let gx = ((x as f64 + 0.5) * 8.0 / w as f64 - 0.5).clamp(0.0, 7.0);
                let (x0, fx) = (gx.floor() as usize, gx.fract());
                let x1 = (x0 + 1).min(7);
                let p = (1.0 - fy) * ((1.0 - fx) * wgt(grid[y0][x0]) + fx * wgt(grid[y0][x1])) + fy * ((1.0 - fx) * wgt(grid[y1][x0]) + fx * wgt(grid[y1][x1]));
                mean[y * w + x] *= p as f32;
            }
        }
        Some((presence, strength, coverage, mean))
    };
    let plain = reading(&|p| &p.0)?;
    let aligned = reading(&|p| &p.1)?;
    // The aligned reading must clearly win: on a texture that is the same on
    // every picture it reads lower (the signs are scrambled where the backgrounds differ).
    Some(if aligned.0 > 1.3 * plain.0 + 0.01 {
        Texture { presence: aligned.0, strength: aligned.1, coverage: aligned.2, blend: "difference", detail: aligned.3 }
    } else {
        Texture { presence: plain.0, strength: plain.1, coverage: plain.2, blend: "soft_light", detail: plain.3 }
    })
}

/// The texture as a plate. For `soft_light`: mid grey plus the static
/// detail (about 0.7 of a display step per stop; neutral at grey). For
/// `difference`: the size of the detail on black (neutral at black), light
/// on dark pictures and dark on light ones once blended.
pub fn texture_plate(w: usize, h: usize, t: &Texture) -> Plate {
    let mut rgb = vec![0u8; w * h * 3];
    for (i, d) in t.detail.iter().enumerate() {
        let v = if t.blend == "difference" { 1.4 * d.max(0.0) } else { 0.5 + 0.7 * d };
        let v = (v * 255.0).round().clamp(0.0, 255.0) as u8;
        rgb[i * 3..i * 3 + 3].copy_from_slice(&[v, v, v]);
    }
    Plate { w, h, rgb }
}

/// The analysis entry for a measured texture (`None` below the presence a
/// handful of unrelated pictures reach by chance).
pub fn texture_info(t: &Texture) -> Option<TextureInfo> {
    (t.presence >= 0.08 && t.strength >= 0.025).then(|| TextureInfo {
        presence: Est::new(t.presence, "share of fine detail common to every picture, in the quarter of the frame where it is highest", (0.3 + 2.0 * t.presence).min(0.85), Ident::Measured),
        coverage: Est::new(t.coverage, "share of the frame", 0.5, Ident::Qualitative),
        strength: Est::new(t.strength, "stops (RMS)", 0.5, Ident::Qualitative),
        path: None,
        blend_guess: t.blend.into(),
        method: if t.blend == "difference" {
            "high-pass of log luminance over one frame per picture, sign turned where the picture is light (the texture is light on dark pictures and dark on light ones); overlay strokes masked".into()
        } else {
            "high-pass of log luminance averaged over one frame per picture; overlay strokes masked".into()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::signatures;
    use crate::synth;

    fn clip() -> Frames {
        // Six pictures, each drifting, with a flash at every cut.
        let mut c = synth::Clip::new(120, 120, 48);
        c.overlay = true;
        c.vignette = 0.6;
        for f in 0..48 {
            let fr = c.frame_mut(f);
            fr.picture = (f / 8) as u32 + 1;
            fr.scale = 1.0 + 0.02 * (f % 8) as f64;
            fr.tx = 0.01 * (f % 8) as f64;
            fr.stops = if f % 8 == 0 { 6.0 } else { 0.0 };
        }
        c.render()
    }

    #[test]
    fn overlay_plate_recovers_the_static_strokes() {
        let frames = clip();
        let idx: Vec<usize> = (0..48).collect();
        let plate = overlay_plate(&frames, &idx);
        let truth = {
            let mut c = synth::Clip::new(120, 120, 1);
            c.overlay = true;
            c
        };
        let (mut tp, mut fp, mut fnn) = (0, 0, 0);
        for y in 0..120 {
            for x in 0..120 {
                let want = truth.overlay_at(x, y) > 0.5;
                let got = luma(&plate.rgb[(y * 120 + x) * 3..(y * 120 + x) * 3 + 3]) > 0.3;
                match (want, got) {
                    (true, true) => tp += 1,
                    (false, true) => fp += 1,
                    (true, false) => fnn += 1,
                    _ => {}
                }
            }
        }
        let (precision, recall) = (tp as f64 / (tp + fp).max(1) as f64, tp as f64 / (tp + fnn).max(1) as f64);
        assert!(precision > 0.85 && recall > 0.85, "precision {precision:.2} recall {recall:.2}");
        let info = overlay_info(&plate).unwrap();
        assert!(info.coverage.value > 0.01 && info.coverage.value < 0.15);
        let usable = usable_mask(&plate);
        assert!(!usable[3 * 120 + 5], "an overlay stroke must be masked");
        let centre_usable = (40..80).flat_map(|y| (40..80).map(move |x| (x, y))).filter(|(x, y)| usable[y * 120 + x]).count();
        assert!(centre_usable > 1500, "centre usable {centre_usable} of 1600");
    }

    #[test]
    fn vignette_fit_recovers_amount_radius_and_softness() {
        let frames = clip();
        let sigs = signatures(&frames);
        let v = vignette(&frames, &sigs, 0, 48).unwrap();
        assert!((v.amount.value - 0.6).abs() < 0.08, "amount {}", v.amount.value);
        assert!((v.radius.value - 0.5).abs() < 0.11, "radius {}", v.radius.value);
        assert!((v.softness.value - 0.5).abs() < 0.16, "softness {}", v.softness.value);
        assert_eq!(v.amount.identifiability, Ident::Measured);
    }

    #[test]
    fn no_vignette_is_reported_as_none() {
        let mut c = synth::Clip::new(96, 96, 16);
        for f in 0..16 {
            c.frame_mut(f).stops = if f % 4 == 0 { 6.0 } else { 0.0 };
        }
        let frames = c.render();
        let sigs = signatures(&frames);
        assert!(vignette(&frames, &sigs, 0, 16).is_none());
    }

    #[test]
    fn vignette_without_flashes_is_read_from_the_ceiling_of_all_frames() {
        // Eight pictures, no flash: no white field.
        let mut c = synth::Clip::new(120, 120, 64);
        c.vignette = 0.6;
        for f in 0..64 {
            c.frame_mut(f).picture = (f / 8) as u32 + 1;
        }
        let frames = c.render();
        let sigs = signatures(&frames);
        let v = vignette(&frames, &sigs, 0, 64).unwrap();
        assert!((v.amount.value - 0.6).abs() < 0.15, "amount {} profile {:?}", v.amount.value, v.profile);
    }

    #[test]
    fn a_static_texture_is_found_and_unrelated_pictures_are_not() {
        let run = |strength: f64| {
            let mut c = synth::Clip::new(120, 120, 8);
            c.noise = 1.0;
            c.texture = strength;
            for f in 0..8 {
                c.frame_mut(f).picture = f as u32 + 1;
            }
            let frames = c.render();
            let groups: Vec<Vec<usize>> = (0..8).map(|f| vec![f]).collect();
            texture(&frames, &groups, &vec![true; 120 * 120]).unwrap()
        };
        let t = run(0.25);
        let (p, s, detail) = (t.presence, t.strength, t.detail.clone());
        assert_eq!(t.blend, "soft_light");
        assert!(p > 0.08 && s > 0.035, "presence {p:.2} strength {s:.3}");
        assert!(texture_info(&t).is_some());
        // The plate follows the texture: darker on the strokes.
        let c = synth::Clip::new(120, 120, 1);
        let (mut on, mut off) = (vec![], vec![]);
        for y in 10..110 {
            for x in 10..110 {
                if c.texture_at(x, y) < -0.3 {
                    on.push(detail[y * 120 + x] as f64)
                } else {
                    off.push(detail[y * 120 + x] as f64)
                }
            }
        }
        let (a, b) = (crate::img::mean(&on), crate::img::mean(&off));
        assert!(a < b - 0.03, "on the strokes {a:.3}, elsewhere {b:.3}");
        let t0 = run(0.0);
        assert!(texture_info(&t0).is_none(), "presence {:.2} strength {:.3}", t0.presence, t0.strength);
    }
}
