//! Score an analysis against the truth of the recipe that was rendered.
//!
//! Every estimator contributes *samples* (truth, estimate) and/or *counts*
//! (hits, misses, extras), tagged with the kind of picture they were measured
//! on. Aggregation into bias, spread and rates is in `report`.

use crate::recipe::{Case, FrameTruth, Truth};
use edits_analyze::schema::{Analysis, Event, Ident};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Sample {
    pub est: String,
    /// Picture kind, or `-` for clip-level estimators.
    pub kind: String,
    pub truth: f64,
    pub got: f64,
    /// Frame (or shot's first frame) the sample belongs to.
    pub frame: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Count {
    pub est: String,
    pub kind: String,
    pub hits: usize,
    pub misses: usize,
    pub extras: usize,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Scores {
    pub samples: Vec<Sample>,
    pub counts: Vec<Count>,
}

impl Scores {
    fn sample(&mut self, est: &str, kind: &str, truth: f64, got: f64, frame: usize) {
        if truth.is_finite() && got.is_finite() {
            self.samples.push(Sample { est: est.into(), kind: kind.into(), truth, got, frame });
        }
    }
    fn count(&mut self, est: &str, kind: &str, hits: usize, misses: usize, extras: usize) {
        self.counts.push(Count { est: est.into(), kind: kind.into(), hits, misses, extras });
    }
}

/// One-to-one matching of two ascending frame lists within `tol`: pairs (truth index, estimate index).
pub fn match_frames(truth: &[usize], est: &[usize], tol: usize) -> Vec<(usize, usize)> {
    let mut cand: Vec<(usize, usize, usize)> = Vec::new();
    for (i, t) in truth.iter().enumerate() {
        for (j, e) in est.iter().enumerate() {
            if t.abs_diff(*e) <= tol {
                cand.push((t.abs_diff(*e), i, j));
            }
        }
    }
    cand.sort();
    let (mut ut, mut ue) = (vec![false; truth.len()], vec![false; est.len()]);
    let mut out = Vec::new();
    for (_, i, j) in cand {
        if !ut[i] && !ue[j] {
            ut[i] = true;
            ue[j] = true;
            out.push((i, j));
        }
    }
    out.sort();
    out
}

/// The vignette's brightness multiplier at radius `r` (fraction of the half-diagonal): the renderer's formula.
pub fn vignette_gain(v: &[f64; 3], r: f64) -> f64 {
    let t = ((r - v[1]) / v[2].max(1e-4)).clamp(0.0, 1.0);
    1.0 - v[0] * t * t * (3.0 - 2.0 * t)
}

/// Truth of frame `f` relative to frame `r` of the same shot: what the
/// analysis can measure (stops, blur, saturation, scale, x, y, rotation).
pub fn relative(t: &FrameTruth, r: &FrameTruth) -> [f64; 7] {
    let s = t.scale / r.scale;
    let d = (t.rot - r.rot).to_radians();
    let (rx, ry) = (r.x * d.cos() - r.y * d.sin(), r.x * d.sin() + r.y * d.cos());
    [t.stops - r.stops, (t.blur * t.blur - r.blur * r.blur).max(0.0).sqrt(), if r.sat > 1e-6 { t.sat / r.sat } else { 1.0 }, s, t.x - s * rx, t.y - s * ry, t.rot - r.rot]
}

fn events<'a>(a: &'a Analysis, kind: &str) -> Vec<&'a Event> {
    let mut v: Vec<&Event> = a.shots.iter().flat_map(|s| s.events.iter().filter(|e| e.kind == kind)).collect();
    v.sort_by_key(|e| e.frame);
    v
}

pub fn score(case: &Case, a: &Analysis) -> Scores {
    let t: &Truth = &case.truth;
    let mut s = Scores::default();
    let kind_of_frame = |f: usize| t.frame.get(f).copied().flatten().map(|ft| t.shots[ft.shot].kind.name()).unwrap_or("-");

    // ---- tempo ---------------------------------------------------------------
    if case.spec.audio {
        s.sample("tempo.bpm", "-", t.bpm, a.beats.bpm.value, 0);
        let period = 60.0 / t.bpm;
        let mut d = (a.beats.offset_seconds.value - t.beat_offset).rem_euclid(period);
        if d > period / 2.0 {
            d -= period;
        }
        s.sample("tempo.phase", "-", 0.0, d * t.fps, 0);
    }

    // ---- cuts -----------------------------------------------------------------
    let tcuts: Vec<usize> = t.shots.iter().skip(1).map(|sh| sh.f0).collect();
    let ecuts: Vec<usize> = a.shots.iter().skip(1).map(|sh| sh.f0).collect();
    let m = match_frames(&tcuts, &ecuts, 1);
    for (i, j) in &m {
        s.sample("cut.frame", kind_of_frame(tcuts[*i]), tcuts[*i] as f64, ecuts[*j] as f64, tcuts[*i]);
    }
    s.count("cut", "-", m.len(), tcuts.len() - m.len(), ecuts.len() - m.len());

    // ---- flashes ----------------------------------------------------------------
    // A flash under a threshold hit cannot be seen (white stays white, black
    // stays black): it starts, for an observer, on the first frame after the hit.
    struct Seen {
        shot: usize,
        frame: usize,
        visible: f64,
        rate: Option<f64>,
    }
    let mut seen: Vec<Seen> = Vec::new();
    for f in &t.flashes {
        let hidden = |fr: usize| t.frame.get(fr).copied().flatten().map(|ft| ft.shot == f.shot && ft.threshold.is_some()).unwrap_or(false);
        if !hidden(f.frame) {
            seen.push(Seen { shot: f.shot, frame: f.frame, visible: f.visible, rate: if f.frames >= 4.0 { Some(f.rate) } else { None } });
            continue;
        }
        let mut fr = f.frame;
        while hidden(fr) {
            fr += 1;
        }
        if let Some(ft) = t.frame.get(fr).copied().flatten().filter(|ft| ft.shot == f.shot && ft.stops >= 0.6) {
            seen.push(Seen { shot: f.shot, frame: fr, visible: ft.stops, rate: None });
        }
    }
    let efl = events(a, "flash");
    let tfl: Vec<usize> = seen.iter().map(|f| f.frame).collect();
    let m = match_frames(&tfl, &efl.iter().map(|e| e.frame).collect::<Vec<_>>(), 1);
    for (i, j) in &m {
        let (tf, e) = (&seen[*i], efl[*j]);
        let kind = t.shots[tf.shot].kind.name();
        // The estimate is relative to the shot's least exposed frame.
        let shot = a.shots.iter().find(|sh| sh.f0 <= e.frame && e.frame < sh.f1);
        let base = shot
            .map(|sh| (sh.f0..sh.f1).filter_map(|f| t.frame.get(f).copied().flatten()).filter(|ft| ft.shot == tf.shot && ft.invert < 0.5 && ft.threshold.is_none()).map(|ft| ft.stops).fold(f64::MAX, f64::min))
            .filter(|b| *b < f64::MAX)
            .unwrap_or(0.0);
        let clipped = e.params.contains_key("lower_bound") || e.note.as_deref().map(|n| n.contains("lower bound")).unwrap_or(false);
        s.sample("flash.frame", kind, tf.frame as f64, e.frame as f64, tf.frame);
        s.sample(if clipped { "flash.peak_clipped" } else { "flash.peak" }, kind, tf.visible - base, e.params["peak"].value, tf.frame);
        if let Some(rate) = tf.rate.filter(|_| e.params["decay_rate"].confidence >= 0.3) {
            s.sample("flash.rate", kind, rate, e.params["decay_rate"].value, tf.frame);
        }
    }
    for kind in crate::pictures::KINDS {
        let k = kind.name();
        let of_kind: Vec<usize> = (0..tfl.len()).filter(|i| t.shots[seen[*i].shot].kind == kind).collect();
        let hit = m.iter().filter(|(i, _)| of_kind.contains(i)).count();
        let extra = (0..efl.len()).filter(|j| !m.iter().any(|(_, mj)| mj == j) && kind_of_frame(efl[*j].frame) == k).count();
        if !of_kind.is_empty() || extra > 0 {
            s.count("flash", k, hit, of_kind.len() - hit, extra);
        }
    }

    // ---- tone hits --------------------------------------------------------------
    for hit_kind in ["saturation", "invert", "threshold"] {
        let ev = events(a, hit_kind);
        // A whole shot reported as black and white "maybe the picture itself" is not a hit.
        let ev: Vec<&Event> = ev.into_iter().filter(|e| e.params.values().all(|p| p.identifiability != Ident::NeedsSource)).collect();
        let th: Vec<&crate::recipe::HitTruth> = t.hits.iter().filter(|h| h.kind == hit_kind).collect();
        let m = match_frames(&th.iter().map(|h| h.f0).collect::<Vec<_>>(), &ev.iter().map(|e| e.f0).collect::<Vec<_>>(), 1);
        for (i, j) in &m {
            let (h, e) = (th[*i], ev[*j]);
            let kind = t.shots[h.shot].kind.name();
            s.sample(&format!("{hit_kind}.frames"), kind, (h.f1 - h.f0) as f64, (e.f1 - e.f0) as f64, h.f0);
            match hit_kind {
                "saturation" => s.sample("saturation.amount", kind, h.value, e.params["amount"].value, h.f0),
                "threshold" => s.sample("threshold.level", kind, h.value, e.params["level"].value, h.f0),
                _ => {}
            }
        }
        for kind in crate::pictures::KINDS {
            let k = kind.name();
            let of_kind: Vec<usize> = (0..th.len()).filter(|i| t.shots[th[*i].shot].kind == kind).collect();
            let hit = m.iter().filter(|(i, _)| of_kind.contains(i)).count();
            let extra = (0..ev.len()).filter(|j| !m.iter().any(|(_, mj)| mj == j) && kind_of_frame(ev[*j].f0) == k).count();
            if !of_kind.is_empty() || extra > 0 {
                s.count(hit_kind, k, hit, of_kind.len() - hit, extra);
            }
        }
    }

    // ---- per-frame curves ---------------------------------------------------------
    let sig = &a.signature;
    for sh in &a.shots {
        let Some(rt) = t.frame.get(sh.ref_frame).copied().flatten() else { continue };
        let kind = t.shots[rt.shot].kind.name();
        s.count("shot.measured", kind, (sh.status == Ident::Measured) as usize, (sh.status != Ident::Measured) as usize, 0);
        let (mut valid, mut invalid) = (0, 0);
        // Exposure is reported relative to the shot's least exposed frame.
        let floor = (sh.f0..sh.f1).filter_map(|f| t.frame.get(f).copied().flatten()).filter(|ft| ft.shot == rt.shot && ft.invert < 0.5 && ft.threshold.is_none()).map(|ft| ft.stops).fold(rt.stops, f64::min);
        for f in sh.f0..sh.f1 {
            let Some(ft) = t.frame.get(f).copied().flatten().filter(|ft| ft.shot == rt.shot) else { continue };
            let i = f - sig.f0;
            let mut rel = relative(&ft, &rt);
            rel[0] = ft.stops - floor;
            let hit = ft.invert > 0.5 || ft.threshold.is_some();
            // Exposure.
            if !hit {
                if rel[0].abs() > 0.25 {
                    s.sample("exposure.curve", kind, rel[0], sig.stops[i], f);
                } else if rel[0].abs() < 1e-6 {
                    s.sample("exposure.rest", kind, 0.0, sig.stops[i], f);
                }
            }
            // Blur.
            if rel[1] >= 0.3 {
                s.sample("blur.radius", kind, rel[1], sig.blur[i], f);
            } else if rel[1] < 1e-6 && f != sh.ref_frame {
                s.sample(if ft.stops > 0.5 { "blur.rest_flash" } else { "blur.rest" }, kind, 0.0, sig.blur[i], f);
            }
            // Saturation.
            if !hit && rt.sat > 0.99 {
                if ft.sat < 0.6 {
                    s.sample("saturation.curve", kind, rel[2], sig.sat[i], f);
                } else if ft.stops > 1.0 {
                    s.sample("saturation.rest_flash", kind, 1.0, sig.sat[i], f);
                } else {
                    s.sample("saturation.rest", kind, 1.0, sig.sat[i], f);
                }
            }
            // Motion.
            match sig.scale[i] {
                Some(v) => {
                    valid += 1;
                    s.sample("scale", kind, rel[3], v, f);
                }
                None => invalid += 1,
            }
            if let Some(smp) = &sh.samples {
                let k = f - sh.f0;
                if smp.method.as_bytes().get(k).map(|m| *m != b'b').unwrap_or(false) && sh.status == Ident::Measured {
                    s.sample("position.x", kind, rel[4], smp.x[k], f);
                    s.sample("position.y", kind, rel[5], smp.y[k], f);
                    s.sample("rotation", kind, rel[6], smp.rotation[k], f);
                }
            }
        }
        s.count("track.frames", kind, valid, invalid, 0);
    }

    // ---- static layers --------------------------------------------------------------
    match (&t.vignette, &a.layers.vignette) {
        (Some(tv), Some(ev)) => {
            s.count("vignette", "-", 1, 0, 0);
            s.sample("vignette.amount", "-", tv[0], ev.amount.value, 0);
            s.sample("vignette.radius", "-", tv[1], ev.radius.value, 0);
            s.sample("vignette.softness", "-", tv[2], ev.softness.value, 0);
            let got = [ev.amount.value, ev.radius.value, ev.softness.value];
            let worst = (0..=20).map(|i| (vignette_gain(tv, i as f64 / 20.0) - vignette_gain(&got, i as f64 / 20.0)).abs()).fold(0.0, f64::max);
            s.sample("vignette.gain", "-", 0.0, worst, 0);
        }
        (Some(_), None) => s.count("vignette", "-", 0, 1, 0),
        (None, Some(_)) => s.count("vignette", "-", 0, 0, 1),
        (None, None) => {}
    }
    match (t.overlay, a.layers.overlay.is_some()) {
        (true, true) => s.count("overlay", "-", 1, 0, 0),
        (true, false) => s.count("overlay", "-", 0, 1, 0),
        (false, true) => s.count("overlay", "-", 0, 0, 1),
        _ => {}
    }

    // ---- slots -----------------------------------------------------------------------
    let true_pic: Vec<Option<&str>> = a.shots.iter().map(|sh| t.frame.get(sh.ref_frame).copied().flatten().map(|ft| t.shots[ft.shot].slot.as_str())).collect();
    let mut distinct: Vec<&str> = true_pic.iter().flatten().copied().collect();
    distinct.sort_unstable();
    distinct.dedup();
    s.sample("slots.count", "-", distinct.len() as f64, a.slots.len() as f64, 0);
    let (mut agree, mut split, mut merged) = (0, 0, 0);
    for i in 0..a.shots.len() {
        for j in i + 1..a.shots.len() {
            if let (Some(p), Some(q)) = (true_pic[i], true_pic[j]) {
                match (p == q, a.shots[i].slot == a.shots[j].slot) {
                    (true, false) => split += 1,
                    (false, true) => merged += 1,
                    (true, true) => agree += 1,
                    _ => {}
                }
            }
        }
    }
    s.count("slots.same_picture", "-", agree, split, merged);
    s
}
