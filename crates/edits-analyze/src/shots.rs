//! Cut detection that survives exposure flashes, blur and zoom punches.
//!
//! Consecutive frames are compared as z-normalised thumbnails (a flash is
//! affine in intensity, so it cancels) after a small search over shift and
//! zoom (so a punch-in or a slide is not a cut). The absolute value of the
//! correlation is used, so a colour inversion is not a cut either; its sign
//! is kept as an invert-toggle cue.

use crate::img::Gray;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct PairSim {
    /// Best |correlation| over the motion search, 0..1. NaN if a frame is blank.
    pub sim: f32,
    /// The signed correlation at that position (negative = inverted tones).
    pub signed: f32,
}

#[derive(Clone, Debug)]
pub struct Cut {
    /// First frame of the new shot.
    pub frame: usize,
    /// 1 - similarity across the cut.
    pub strength: f32,
}

/// Thumbnail height used for cut detection.
pub const THUMB: usize = 64;
const MARGIN: usize = 10;
const SHIFT: i32 = 9;
const ZOOMS: [f32; 4] = [1.0, 1.09, 1.19, 1.33];

/// Band-pass the thumbnails: structure is compared, not the brightness layout
/// (a vignette under a white flash would otherwise look like shared content).
pub fn prepare(thumbs: &[Gray]) -> Vec<Gray> {
    thumbs
        .par_iter()
        .map(|t| {
            let fine = crate::img::gaussian_blur(t, 0.6);
            let coarse = crate::img::gaussian_blur(t, 5.0);
            Gray { w: t.w, h: t.h, d: fine.d.iter().zip(&coarse.d).map(|(a, b)| a - b).collect() }
        })
        .collect()
}

fn zoom(src: &Gray, s: f32) -> Gray {
    if (s - 1.0).abs() < 1e-6 {
        return src.clone();
    }
    let (cx, cy) = ((src.w as f32 - 1.0) / 2.0, (src.h as f32 - 1.0) / 2.0);
    let mut out = Gray::new(src.w, src.h);
    for y in 0..src.h {
        for x in 0..src.w {
            out.d[y * src.w + x] = src.sample(cx + (x as f32 - cx) / s, cy + (y as f32 - cy) / s).unwrap_or(0.0);
        }
    }
    out
}

/// Correlation of the central window of `a` with the window of `b` shifted by (dx, dy).
fn ncc(a: &Gray, b: &Gray, dx: i32, dy: i32) -> Option<f32> {
    let (w, h) = (a.w - 2 * MARGIN, a.h - 2 * MARGIN);
    let n = (w * h) as f32;
    let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for y in 0..h {
        let ra = &a.d[(y + MARGIN) * a.w + MARGIN..(y + MARGIN) * a.w + MARGIN + w];
        let by = (y + MARGIN) as i32 + dy;
        let bx = MARGIN as i32 + dx;
        let rb = &b.d[by as usize * b.w + bx as usize..by as usize * b.w + bx as usize + w];
        for x in 0..w {
            let (p, q) = (ra[x], rb[x]);
            sa += p;
            sb += q;
            saa += p * p;
            sbb += q * q;
            sab += p * q;
        }
    }
    let va = saa / n - (sa / n) * (sa / n);
    let vb = sbb / n - (sb / n) * (sb / n);
    if va < 4e-6 || vb < 4e-6 {
        return None;
    }
    Some((sab / n - (sa / n) * (sb / n)) / (va * vb).sqrt())
}

/// Motion-compensated similarity of two thumbnails.
pub fn similarity(a: &Gray, b: &Gray) -> PairSim {
    let mut best = PairSim { sim: f32::NAN, signed: 0.0 };
    let consider = |v: Option<f32>, best: &mut PairSim| {
        if let Some(v) = v {
            if !(best.sim >= v.abs()) {
                *best = PairSim { sim: v.abs(), signed: v };
            }
        }
    };
    for (zi, z) in ZOOMS.iter().enumerate() {
        // Zoom whichever frame is the wider view; only interior samples are needed.
        let variants: &[bool] = if zi == 0 { &[false] } else { &[false, true] };
        for &zoom_a in variants {
            let (za, zb);
            let (pa, pb): (&Gray, &Gray) = if zi == 0 {
                (a, b)
            } else if zoom_a {
                za = zoom(a, *z);
                (&za, b)
            } else {
                zb = zoom(b, *z);
                (a, &zb)
            };
            // Coarse grid of shifts, then a one-pixel refinement around the best.
            let mut local = (f32::MIN, 0, 0, 0.0f32);
            let mut dy = -SHIFT + 1;
            while dy <= SHIFT {
                let mut dx = -SHIFT + 1;
                while dx <= SHIFT {
                    if let Some(v) = ncc(pa, pb, dx, dy) {
                        if v.abs() > local.0 {
                            local = (v.abs(), dx, dy, v);
                        }
                    }
                    dx += 2;
                }
                dy += 2;
            }
            if local.0 > f32::MIN {
                for dy in (local.2 - 1).max(-SHIFT)..=(local.2 + 1).min(SHIFT) {
                    for dx in (local.1 - 1).max(-SHIFT)..=(local.1 + 1).min(SHIFT) {
                        if let Some(v) = ncc(pa, pb, dx, dy) {
                            if v.abs() > local.0 {
                                local = (v.abs(), dx, dy, v);
                            }
                        }
                    }
                }
                consider(Some(local.3), &mut best);
            }
        }
    }
    best
}

/// `out[i]` compares frame `i` with frame `i - 1` (index 0 is NaN).
pub fn pair_sims(thumbs: &[Gray]) -> Vec<PairSim> {
    (0..thumbs.len())
        .into_par_iter()
        .map(|i| if i == 0 { PairSim { sim: f32::NAN, signed: 0.0 } } else { similarity(&thumbs[i - 1], &thumbs[i]) })
        .collect()
}

/// Cuts inside `f0..f1` (the first shot starts at `f0`; it is not listed).
/// `lum[i]` is the mean luminance of frame `i` (any scale); it only breaks
/// ties: when two neighbouring frames both look like the cut (a transition
/// frame, or a flash frame too white to compare), the one where the
/// brightness jumps up is taken, because a flash peaks on its cut.
pub fn detect_cuts(thumbs: &[Gray], sims: &[PairSim], lum: &[f32], f0: usize, f1: usize) -> Vec<Cut> {
    const STRONG: f32 = 0.35;
    const WEAK: f32 = 0.6;
    // Frames with picture content; solid frames (a full-white flash) are skipped over.
    let nb: Vec<usize> = (f0..f1).filter(|i| ncc(&thumbs[*i], &thumbs[*i], 0, 0).is_some()).collect();
    let mut cands: Vec<Cut> = Vec::new();
    for k in 1..nb.len() {
        let (p, q) = (nb[k - 1], nb[k]);
        let s = if q == p + 1 { sims[q].sim } else { similarity(&thumbs[p], &thumbs[q]).sim };
        if s.is_nan() || s >= WEAK {
            continue;
        }
        // Does the old picture come back right after? Then it was a flash frame, not a cut.
        let mut across = 0.0f32;
        for (b, a) in [(k - 1, k + 1), (k.saturating_sub(2), k + 1), (k - 1, k + 2)] {
            if a < nb.len() {
                let v = similarity(&thumbs[nb[b]], &thumbs[nb[a]]).sim;
                if !v.is_nan() {
                    across = across.max(v);
                }
            }
        }
        let limit = if s < STRONG { 0.6 } else { 0.45 };
        if across < limit {
            cands.push(Cut { frame: p + 1, strength: 1.0 - s });
        }
    }
    // One cut per group of candidates closer than 3 frames.
    let strong = |c: &Cut| c.strength > 1.0 - STRONG;
    let range = lum[f0..f1].iter().fold((f32::MAX, f32::MIN), |a, v| (a.0.min(*v), a.1.max(*v)));
    let jump_min = 0.2 * (range.1 - range.0).max(1e-6);
    let jump = |f: usize| lum[f] - lum[f - 1];
    let low = |f: usize| f > f0 && f < f1 && sims[f].sim < WEAK;
    let mut groups: Vec<Vec<Cut>> = Vec::new();
    for c in cands {
        match groups.last_mut() {
            Some(g) if c.frame - g.last().unwrap().frame < 3 => g.push(c),
            _ => groups.push(vec![c]),
        }
    }
    let picks: Vec<(Cut, bool)> = groups
        .iter()
        .map(|g| {
            // Possible positions: the candidates and the dissimilar frames touching them.
            let mut places: Vec<usize> = g.iter().map(|c| c.frame).collect();
            for c in g {
                for f in [c.frame.saturating_sub(1), c.frame + 1] {
                    if low(f) && !places.contains(&f) {
                        places.push(f);
                    }
                }
            }
            let by_jump = places.iter().copied().filter(|f| jump(*f) > jump_min).max_by(|a, b| jump(*a).partial_cmp(&jump(*b)).unwrap());
            let frame = by_jump.unwrap_or_else(|| g.iter().max_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap()).unwrap().frame);
            let strength = g.iter().map(|c| c.strength).fold(0.0, f32::max);
            (Cut { frame, strength }, g.iter().any(strong))
        })
        .collect();
    // A weak cut right next to a strong one is part of the same transition.
    let mut cuts: Vec<Cut> = picks
        .iter()
        .filter(|(c, is_strong)| *is_strong || !picks.iter().any(|(o, s)| *s && o.frame.abs_diff(c.frame) < 4))
        .map(|(c, _)| c.clone())
        .collect();
    cuts.retain(|c| c.frame >= f0 + 2 && c.frame + 2 <= f1);
    cuts
}

/// Tone-inversion toggles: frames whose best match with the previous frame
/// is a strong negative correlation.
pub fn invert_toggles(sims: &[PairSim]) -> Vec<bool> {
    sims.iter().map(|s| !s.sim.is_nan() && s.signed < -0.3 && s.sim > 0.3).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    #[test]
    fn finds_cuts_but_not_flashes_zooms_or_inversions() {
        // Three pictures, cuts at frames 12 and 24. Zoom punch and flash at
        // every cut, an inversion hit inside the second shot.
        let mut clip = synth::Clip::new(96, 96, 36);
        for f in 0..36 {
            let shot = f / 12;
            let k = (f % 12) as f64;
            let fr = clip.frame_mut(f);
            fr.picture = shot as u32 + 1;
            fr.scale = 1.0 + 0.35 * 0.6f64.powf(k);
            fr.stops = 3.0 * 0.5f64.powf(k);
            fr.invert = (16..19).contains(&f);
        }
        let frames = clip.render();
        let raw = crate::signals::thumbnails(&frames, THUMB);
        let lum: Vec<f32> = raw.iter().map(|t| t.mean()).collect();
        let thumbs = prepare(&raw);
        let sims = pair_sims(&thumbs);
        let cuts: Vec<usize> = detect_cuts(&thumbs, &sims, &lum, 0, 36).iter().map(|c| c.frame).collect();
        assert_eq!(cuts, vec![12, 24], "sims: {:?}", sims.iter().map(|s| s.sim).collect::<Vec<_>>());
        let tog = invert_toggles(&sims);
        let at: Vec<usize> = (0..36).filter(|i| tog[*i]).collect();
        assert_eq!(at, vec![16, 19]);
    }
}
