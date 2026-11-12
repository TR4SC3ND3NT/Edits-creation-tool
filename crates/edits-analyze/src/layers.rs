//! Static layers that sit on top of every shot: an overlay plate (ornaments,
//! dust, handwriting) and a vignette.
//!
//! Overlay: the per-pixel temporal median over many frames keeps what does
//! not move. The pictures underneath average into a smooth blur, so a white
//! top-hat (median minus its morphological opening) isolates the thin bright
//! overlay strokes; only the part common to the three channels is kept (the
//! strokes are neutral, the residue of the pictures is coloured) and dividing
//! by `1 - background` undoes a screen blend.
//!
//! Vignette: the brightest frames (exposure flashes) are close to a white
//! field, so their radial brightness profile is the vignette. It is fitted
//! in linear light with `1 - amount * smoothstep(radius, radius + softness, r)`,
//! `r` being the distance from the centre as a fraction of the half-diagonal.

use crate::img::{median, srgb_lut, Frames};
use crate::schema::{Est, Ident, OverlayInfo, VignetteInfo};
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
    let med = temporal_percentile(frames, idx, 0.5);
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
    let lut = srgb_lut();
    let (cx, cy) = ((w as f64 - 1.0) / 2.0, (h as f64 - 1.0) / 2.0);
    let half_diag = (cx * cx + cy * cy).sqrt();
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
    if centre < 0.02 {
        return None;
    }
    let profile: Vec<f64> = raw.iter().map(|v| ((v / centre) * 1000.0).round() / 1000.0).collect();
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
            "radial profile of the brightest (flash) frames in linear light".into()
        } else {
            "no near-white frames: profile of the brightest frames, mixed with picture content".into()
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
}
