//! Colour looks read from a frame's own histogram, without a reference:
//! duotone (a gradient map or a tint) and posterisation.
//!
//! - **Duotone.** A gradient map gives every pixel a colour that depends on
//!   its luminance only: binned by display luminance, the chroma inside a bin
//!   hardly varies, while in a picture with its own colours it varies about
//!   as much as it does overall. The colours are a straight line through the
//!   bins (the renderer's `gradient_map` mixes display values by display
//!   luminance), evaluated at the darkest and brightest pixels: the
//!   `shadows` and `highlights` of the effect when the picture has ink and paper. A black-and-white frame has no
//!   chroma at all and is not a duotone.
//! - **Posterise.** Quantised channels leave a comb: few occupied histogram
//!   bins, each a narrow peak even after a codec has smeared it. The number of
//!   peaks per channel is the number of levels.

/// Duotone statistics of one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Duo {
    /// RMS of the chroma explained by luminance (display units, 0..1).
    pub chroma: f64,
    /// Chroma left inside luminance bins over the total (0 = a pure function of luminance).
    pub spread: f64,
    /// Display (sRGB) colours of the fitted line at the 1st and 99th percentile of luminance.
    pub shadows: [f64; 3],
    pub highlights: [f64; 3],
}

impl Duo {
    /// A coloured look that is a function of luminance.
    pub fn is_duotone(&self) -> bool {
        self.chroma > 0.035 && self.spread < 0.35
    }
}

const BINS: usize = 16;

/// Duotone statistics of a packed RGB8 frame (every other pixel), over `mask` where given.
pub fn duotone(rgb: &[u8], w: usize, h: usize, mask: Option<&[bool]>) -> Duo {
    let mut n = [0f64; BINS];
    let mut sc = [[0f64; 3]; BINS];
    let mut sc2 = [0f64; BINS];
    // Least squares of each channel on luminance.
    let (mut sy, mut syy, mut cnt) = (0f64, 0f64, 0f64);
    let mut lh = [0u32; 256];
    let mut sxy = [0f64; 3];
    let mut sx = [0f64; 3];
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let i = y * w + x;
            if mask.map(|m| !m[i]).unwrap_or(false) {
                continue;
            }
            let p = &rgb[i * 3..i * 3 + 3];
            let c = [p[0] as f64 / 255.0, p[1] as f64 / 255.0, p[2] as f64 / 255.0];
            let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            let ch = [c[0] - l, c[1] - l, c[2] - l];
            let b = ((l * BINS as f64) as usize).min(BINS - 1);
            lh[((l * 255.0).round() as usize).min(255)] += 1;
            n[b] += 1.0;
            for k in 0..3 {
                sc[b][k] += ch[k];
                sx[k] += c[k];
                sxy[k] += c[k] * l;
            }
            sc2[b] += ch[0] * ch[0] + ch[1] * ch[1] + ch[2] * ch[2];
            sy += l;
            syy += l * l;
            cnt += 1.0;
        }
    }
    if cnt < 50.0 {
        return Duo::default();
    }
    let (mut within, mut explained) = (0f64, 0f64);
    for b in 0..BINS {
        if n[b] < 1.0 {
            continue;
        }
        let m: Vec<f64> = sc[b].iter().map(|v| v / n[b]).collect();
        let m2 = m.iter().map(|v| v * v).sum::<f64>();
        within += (sc2[b] - n[b] * m2).max(0.0);
        explained += n[b] * m2;
    }
    let (within, explained) = ((within / cnt).sqrt(), (explained / cnt).sqrt());
    let var_y = (syy / cnt - (sy / cnt).powi(2)).max(1e-9);
    // The line's ends are not where luminance is 0 and 1 (the map's colours
    // have luminances of their own) but at the darkest and brightest pixels:
    // ink and paper, in a manga panel.
    let quantile = |q: f64| {
        let mut acc = 0u32;
        for (i, v) in lh.iter().enumerate() {
            acc += v;
            if acc as f64 >= q * cnt {
                return i as f64 / 255.0;
            }
        }
        1.0
    };
    let (l0, l1) = (quantile(0.01), quantile(0.99));
    let mut shadows = [0.0; 3];
    let mut highlights = [0.0; 3];
    for k in 0..3 {
        let slope = (sxy[k] / cnt - sx[k] / cnt * sy / cnt) / var_y;
        let icpt = sx[k] / cnt - slope * sy / cnt;
        shadows[k] = ((icpt + slope * l0) * 1000.0).round().clamp(0.0, 1000.0) / 1000.0;
        highlights[k] = ((icpt + slope * l1) * 1000.0).round().clamp(0.0, 1000.0) / 1000.0;
    }
    Duo { chroma: explained, spread: within / (within + explained + 1e-9), shadows, highlights }
}

/// Posterise statistics of one frame: share of the 256 codes per channel
/// that hold more than 0.1% of the pixels, and the median number of
/// separate clusters of codes per channel.
pub fn comb(rgb: &[u8], mask: Option<&[bool]>) -> (f64, u32) {
    let mut hist = [[0u32; 256]; 3];
    let mut n = 0u32;
    for (i, p) in rgb.chunks_exact(3).enumerate() {
        if mask.map(|m| !m[i]).unwrap_or(false) {
            continue;
        }
        for k in 0..3 {
            hist[k][p[k] as usize] += 1;
        }
        n += 1;
    }
    if n < 100 {
        return (1.0, 0);
    }
    let mut occ = 0.0;
    let mut peaks = Vec::new();
    for hk in &hist {
        occ += hk.iter().filter(|v| **v as f64 > 0.001 * n as f64).count() as f64 / 256.0;
        // Clusters of occupied codes separated by at least 4 empty ones,
        // each holding at least 0.3% of the pixels: one per level.
        let (mut p, mut mass, mut gap) = (0, 0u32, 99);
        for v in hk.iter() {
            if *v as f64 > 0.0005 * n as f64 {
                if gap >= 4 {
                    if mass as f64 > 0.003 * n as f64 {
                        p += 1;
                    }
                    mass = 0;
                }
                mass += v;
                gap = 0;
            } else {
                gap += 1;
            }
        }
        if mass as f64 > 0.003 * n as f64 {
            p += 1;
        }
        peaks.push(p);
    }
    peaks.sort_unstable();
    (occ / 3.0, peaks[1])
}

/// A posterise hit on a frame against its reference frame: the frame's
/// histogram is a comb and the reference's is not. Returns the levels.
pub fn posterized(cur: &[u8], reference: &[u8], mask: Option<&[bool]>) -> Option<u32> {
    let (oc, pc) = comb(cur, mask);
    let (or, _) = comb(reference, mask);
    (oc < 0.32 && or > 1.8 * oc && (2..=12).contains(&pc)).then_some(pc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    fn clip(f: impl Fn(&mut synth::FrameSpec)) -> crate::img::Frames {
        let mut c = synth::Clip::new(128, 128, 1);
        c.noise = 2.0;
        f(c.frame_mut(0));
        c.render()
    }

    #[test]
    fn duotone_is_found_with_its_colours_and_pictures_are_not() {
        let navy = [0.08, 0.1, 0.35];
        let yellow = [1.0, 0.92, 0.3];
        let d = clip(|s| s.duotone = Some((navy, yellow)));
        let duo = duotone(d.rgb(0), 128, 128, None);
        assert!(duo.is_duotone(), "{duo:?}");
        for k in 0..3 {
            assert!((duo.shadows[k] - navy[k] as f64).abs() < 0.08, "{duo:?}");
            assert!((duo.highlights[k] - yellow[k] as f64).abs() < 0.08, "{duo:?}");
        }
        // The procedural picture has its own colours; its black-and-white version has none.
        let plain = duotone(clip(|_| {}).rgb(0), 128, 128, None);
        assert!(!plain.is_duotone(), "{plain:?}");
        let grey = duotone(clip(|s| s.saturation = 0.0).rgb(0), 128, 128, None);
        assert!(!grey.is_duotone(), "{grey:?}");
    }

    #[test]
    fn posterize_levels_are_counted_through_noise() {
        let reference = clip(|_| {});
        for levels in [3, 4, 6] {
            let p = clip(|s| s.posterize = Some(levels));
            assert_eq!(posterized(p.rgb(0), reference.rgb(0), None), Some(levels), "{levels} levels: {:?}", comb(p.rgb(0), None));
        }
        assert_eq!(posterized(reference.rgb(0), reference.rgb(0), None), None);
        assert_eq!(posterized(clip(|s| s.stops = 1.0).rgb(0), reference.rgb(0), None), None);
    }
}
