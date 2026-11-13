//! Compare a video (typically our render, with any pictures in it) with a
//! reference analysis, in signature space: the same measurements are taken
//! from both and the measurements are compared, never the pixels.
//!
//! - tempo and grid;
//! - cut, flash and tone-hit events matched one-to-one within one frame:
//!   precision, recall, mean signed offset;
//! - per reference shot, the exposure, blur, saturation and scale curves,
//!   resampled on the beat grid: normalised L2 distance plus start, end,
//!   peak and time-to-peak differences;
//! - a ranked plain-language list of the largest differences.

use crate::beats;
use crate::media;
use crate::pipeline::{self, AnalyzeOpts};
use crate::schema::*;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EventMatch {
    pub reference: usize,
    pub candidate: usize,
    pub matched: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    /// Mean of (candidate frame - reference frame) over matched events.
    pub mean_offset_frames: f64,
    /// Reference frames with no partner, and candidate frames with no partner.
    pub missing: Vec<usize>,
    pub extra: Vec<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CurveCmp {
    /// Normalised L2 distance (0 = identical; 1 = as large as the reference curve itself).
    pub distance: f64,
    pub ref_start: f64,
    pub cand_start: f64,
    pub ref_end: f64,
    pub cand_end: f64,
    pub ref_peak: f64,
    pub cand_peak: f64,
    /// Frames from the start of the shot to the peak.
    pub ref_peak_frame: f64,
    pub cand_peak_frame: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShotCmp {
    pub shot: usize,
    pub f0: usize,
    pub f1: usize,
    pub beat0: f64,
    pub beat1: f64,
    /// `exposure` (stops), `blur` (% of height), `saturation` (ratio), `scale` (ratio).
    pub curves: BTreeMap<String, CurveCmp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Difference {
    pub category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shot: Option<usize>,
    /// Size of the difference in units of what is tolerable (1 = just noticeable).
    pub severity: f64,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TempoCmp {
    pub reference_bpm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_bpm: Option<f64>,
    /// Candidate beat phase minus reference beat phase, in frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_offset_frames: Option<f64>,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Comparison {
    pub schema: String,
    pub reference: String,
    pub candidate: String,
    /// 0..1 per category (`null` = not comparable) and overall.
    pub scores: BTreeMap<String, Option<f64>>,
    pub overall: f64,
    pub tempo: TempoCmp,
    pub cuts: EventMatch,
    pub flashes: EventMatch,
    pub tone_hits: EventMatch,
    pub shots: Vec<ShotCmp>,
    /// Largest first.
    pub differences: Vec<Difference>,
}

/// One-to-one matching of two ascending frame lists within `tol` frames.
pub fn match_events(reference: &[usize], candidate: &[usize], tol: usize) -> (EventMatch, Vec<(usize, usize)>) {
    let mut pairs: Vec<(usize, usize, usize)> = Vec::new();
    for (i, r) in reference.iter().enumerate() {
        for (j, c) in candidate.iter().enumerate() {
            let d = r.abs_diff(*c);
            if d <= tol {
                pairs.push((d, i, j));
            }
        }
    }
    pairs.sort();
    let (mut used_r, mut used_c) = (vec![false; reference.len()], vec![false; candidate.len()]);
    let mut matched: Vec<(usize, usize)> = Vec::new();
    for (_, i, j) in pairs {
        if !used_r[i] && !used_c[j] {
            used_r[i] = true;
            used_c[j] = true;
            matched.push((i, j));
        }
    }
    matched.sort();
    let m = matched.len();
    let precision = if candidate.is_empty() { 1.0 } else { m as f64 / candidate.len() as f64 };
    let recall = if reference.is_empty() { 1.0 } else { m as f64 / reference.len() as f64 };
    let f1 = if precision + recall > 0.0 { 2.0 * precision * recall / (precision + recall) } else { 0.0 };
    let off = if m > 0 { matched.iter().map(|(i, j)| candidate[*j] as f64 - reference[*i] as f64).sum::<f64>() / m as f64 } else { 0.0 };
    (
        EventMatch {
            reference: reference.len(),
            candidate: candidate.len(),
            matched: m,
            precision: round_to(precision, 3),
            recall: round_to(recall, 3),
            f1: round_to(f1, 3),
            mean_offset_frames: round_to(off, 2),
            missing: (0..reference.len()).filter(|i| !used_r[*i]).map(|i| reference[i]).collect(),
            extra: (0..candidate.len()).filter(|j| !used_c[*j]).map(|j| candidate[j]).collect(),
        },
        matched,
    )
}

struct Signal {
    name: &'static str,
    label: &'static str,
    unit: &'static str,
    neutral: f64,
    /// Smallest curve size used for normalising, and the tolerance for a feature difference.
    floor: f64,
    tol: f64,
}

const SIGNALS: [Signal; 4] = [
    Signal { name: "exposure", label: "exposure", unit: " stops", neutral: 0.0, floor: 0.3, tol: 0.5 },
    Signal { name: "blur", label: "blur", unit: "% of height", neutral: 0.0, floor: 0.15, tol: 0.2 },
    Signal { name: "saturation", label: "saturation", unit: "x", neutral: 1.0, floor: 0.15, tol: 0.2 },
    Signal { name: "scale", label: "zoom", unit: "x", neutral: 1.0, floor: 0.03, tol: 0.04 },
];

fn series(sig: &Signature, name: &str) -> Vec<Option<f64>> {
    match name {
        "exposure" => sig.stops.iter().map(|v| Some(*v)).collect(),
        "blur" => sig.blur.iter().map(|v| Some(*v)).collect(),
        "saturation" => sig.sat.iter().map(|v| Some(*v)).collect(),
        _ => sig.scale.clone(),
    }
}

/// Linear interpolation of a per-frame series at a fractional frame.
fn at(v: &[f64], x: f64) -> f64 {
    let x = x.clamp(0.0, (v.len() - 1) as f64);
    let i = (x as usize).min(v.len().saturating_sub(2));
    if v.len() == 1 {
        return v[0];
    }
    v[i] + (v[i + 1] - v[i]) * (x - i as f64)
}

fn curve_cmp(r: &[f64], c: &[f64], s: &Signal, frames_per_beat: f64) -> CurveCmp {
    // Sixteen samples per beat, at least one per frame.
    let n = r.len();
    let step = (frames_per_beat / 16.0).clamp(0.25, 1.0);
    let mut xs = Vec::new();
    let mut x = 0.0;
    while x <= (n - 1) as f64 + 1e-9 {
        xs.push(x);
        x += step;
    }
    let (mut num, mut den) = (0.0, 0.0);
    for x in &xs {
        let (a, b) = (at(r, *x), at(c, *x));
        num += (a - b).powi(2);
        den += (a - s.neutral).powi(2);
    }
    let k = xs.len() as f64;
    let distance = (num / k).sqrt() / (den / k).sqrt().max(s.floor);
    let peak = |v: &[f64]| {
        let i = (0..v.len()).max_by(|a, b| (v[*a] - s.neutral).abs().partial_cmp(&(v[*b] - s.neutral).abs()).unwrap()).unwrap_or(0);
        (v[i], i as f64)
    };
    let (rp, rpf) = peak(r);
    let (cp, cpf) = peak(c);
    CurveCmp {
        distance: round_to(distance, 3),
        ref_start: round_to(r[0], 3),
        cand_start: round_to(c[0], 3),
        ref_end: round_to(r[n - 1], 3),
        cand_end: round_to(c[n - 1], 3),
        ref_peak: round_to(rp, 3),
        cand_peak: round_to(cp, 3),
        ref_peak_frame: rpf,
        cand_peak_frame: cpf,
    }
}

fn events_of<'a>(a: &'a Analysis, kinds: &[&str]) -> Vec<(usize, &'a Event)> {
    let mut v: Vec<(usize, &Event)> = a.shots.iter().flat_map(|s| s.events.iter().filter(|e| kinds.contains(&e.kind.as_str())).map(move |e| (s.id, e))).collect();
    v.sort_by_key(|(_, e)| e.frame);
    v
}

/// Compare two analyses. `cand` must have been made on the reference's beat
/// grid and frame range (see [`compare`]). `cand_tempo` is the candidate's own
/// tempo estimate, if it has audio.
pub fn compare_analyses(reference: &Analysis, cand: &Analysis, cand_tempo: Option<&beats::Tempo>) -> Comparison {
    let fps = reference.source.fps;
    let ratio = cand.source.fps / fps;
    // Candidate frame numbers expressed in reference frames.
    let to_ref = |f: usize| (f as f64 / ratio).round() as usize;
    let fpb = reference.beats.frames_per_beat;
    let beat = |f: usize| round_to(reference.beats.beat_of_frame(f as f64, fps), 2);
    let mut diffs: Vec<Difference> = Vec::new();
    let mut scores: BTreeMap<String, Option<f64>> = BTreeMap::new();

    // ---- tempo ------------------------------------------------------------
    let rb = reference.beats.bpm.value;
    let tempo = match cand_tempo {
        Some(t) => {
            let period = 60.0 / rb;
            // Audio phase against audio phase (the reference's grid may have been nudged onto its cuts).
            let ref_phase = reference.beats.audio_offset_seconds.unwrap_or(reference.beats.offset_seconds.value);
            let mut d = (t.phase - ref_phase).rem_euclid(period);
            if d > period / 2.0 {
                d -= period;
            }
            let rel = (t.bpm - rb).abs() / rb;
            scores.insert("tempo".into(), Some(round_to((1.0 - rel / 0.02).clamp(0.0, 1.0) * (1.0 - (d.abs() * fps / 3.0).min(1.0) * 0.5), 3)));
            if rel > 0.004 {
                diffs.push(Difference { category: "tempo".into(), shot: None, severity: rel / 0.004, text: format!("tempo is {:.2} bpm, reference {:.2} bpm", t.bpm, rb) });
            }
            if d.abs() * fps > 1.0 {
                diffs.push(Difference {
                    category: "tempo".into(),
                    shot: None,
                    severity: d.abs() * fps,
                    text: format!("the audio beat is {:.1} frames {} than in the reference", d.abs() * fps, if d > 0.0 { "later" } else { "earlier" }),
                });
            }
            TempoCmp { reference_bpm: round_to(rb, 2), candidate_bpm: Some(round_to(t.bpm, 2)), phase_offset_frames: Some(round_to(d * fps, 2)), note: "tempo and phase estimated from the candidate's own audio".into() }
        }
        None => {
            scores.insert("tempo".into(), None);
            TempoCmp { reference_bpm: round_to(rb, 2), candidate_bpm: None, phase_offset_frames: None, note: "the candidate has no audio: its events are placed on the reference's beat grid".into() }
        }
    };

    // ---- events -----------------------------------------------------------
    let rcuts: Vec<usize> = reference.shots.iter().skip(1).map(|s| s.f0).collect();
    let ccuts: Vec<usize> = cand.shots.iter().skip(1).map(|s| to_ref(s.f0)).collect();
    let (cuts, _) = match_events(&rcuts, &ccuts, 1);
    for f in &cuts.missing {
        diffs.push(Difference { category: "cuts".into(), shot: reference.shots.iter().find(|s| s.f0 == *f).map(|s| s.id), severity: 4.0, text: format!("cut at beat {} (frame {f}) has no match in the candidate", beat(*f)) });
    }
    for f in &cuts.extra {
        diffs.push(Difference { category: "cuts".into(), shot: None, severity: 3.0, text: format!("the candidate has an extra cut at beat {} (frame {f})", beat(*f)) });
    }
    if cuts.mean_offset_frames.abs() >= 0.5 {
        diffs.push(Difference {
            category: "cuts".into(),
            shot: None,
            severity: 2.0 * cuts.mean_offset_frames.abs(),
            text: format!("cuts are on average {:.1} frames {} than in the reference", cuts.mean_offset_frames.abs(), if cuts.mean_offset_frames > 0.0 { "later" } else { "earlier" }),
        });
    }
    scores.insert("cuts".into(), Some(cuts.f1));

    let (rfl, cfl) = (events_of(reference, &["flash"]), events_of(cand, &["flash"]));
    let (flashes, fl_pairs) = match_events(&rfl.iter().map(|(_, e)| e.frame).collect::<Vec<_>>(), &cfl.iter().map(|(_, e)| to_ref(e.frame)).collect::<Vec<_>>(), 1);
    let mut peak_err = Vec::new();
    for (i, j) in &fl_pairs {
        let (sid, re) = rfl[*i];
        let ce = cfl[*j].1;
        let (rp, cp) = (re.params["peak"].value, ce.params["peak"].value);
        peak_err.push(((rp - cp).abs() / rp.abs().max(1.0)).min(1.0));
        let bound = re.note.as_deref().map(|n| n.contains("lower bound")).unwrap_or(false) || ce.note.as_deref().map(|n| n.contains("lower bound")).unwrap_or(false);
        if (rp - cp).abs() > 0.5 {
            diffs.push(Difference {
                category: "flashes".into(),
                shot: Some(sid),
                severity: (rp - cp).abs() / 0.5 * if bound { 0.5 } else { 1.0 },
                text: format!(
                    "shot {sid}: flash at beat {} peaks at {cp:.1} stops, reference {rp:.1} stops{}",
                    beat(re.frame),
                    if bound { " (clipped at white: both are lower bounds)" } else { "" }
                ),
            });
        }
        let (rr, cr) = (re.params["decay_rate"].value, ce.params["decay_rate"].value);
        let conf = re.params["decay_rate"].confidence.min(ce.params["decay_rate"].confidence);
        if (rr - cr).abs() > 0.12 && conf >= 0.4 {
            diffs.push(Difference {
                category: "flashes".into(),
                shot: Some(sid),
                severity: (rr - cr).abs() / 0.12,
                text: format!("shot {sid}: flash at beat {} decays by {:.0}% per frame, reference {:.0}%", beat(re.frame), 100.0 * cr, 100.0 * rr),
            });
        }
    }
    for f in &flashes.missing {
        diffs.push(Difference { category: "flashes".into(), shot: None, severity: 3.0, text: format!("flash at beat {} (frame {f}) has no match in the candidate", beat(*f)) });
    }
    for f in &flashes.extra {
        diffs.push(Difference { category: "flashes".into(), shot: None, severity: 2.0, text: format!("the candidate has an extra flash at beat {} (frame {f})", beat(*f)) });
    }
    let peak_quality = if peak_err.is_empty() { 1.0 } else { 1.0 - peak_err.iter().sum::<f64>() / peak_err.len() as f64 };
    scores.insert("flashes".into(), Some(round_to(flashes.f1 * (0.5 + 0.5 * peak_quality), 3)));

    // Tone hits: matched per kind on their first frame.
    let kinds = ["saturation", "invert", "threshold"];
    let mut tone_total = EventMatch { reference: 0, candidate: 0, matched: 0, precision: 1.0, recall: 1.0, f1: 1.0, mean_offset_frames: 0.0, missing: vec![], extra: vec![] };
    let mut off_sum = 0.0;
    for kind in kinds {
        let (r, c) = (events_of(reference, &[kind]), events_of(cand, &[kind]));
        let (m, pairs) = match_events(&r.iter().map(|(_, e)| e.f0).collect::<Vec<_>>(), &c.iter().map(|(_, e)| to_ref(e.f0)).collect::<Vec<_>>(), 1);
        for f in &m.missing {
            diffs.push(Difference { category: "tone".into(), shot: None, severity: 3.0, text: format!("{kind} hit at beat {} (frame {f}) has no match in the candidate", beat(*f)) });
        }
        for f in &m.extra {
            diffs.push(Difference { category: "tone".into(), shot: None, severity: 2.0, text: format!("the candidate has an extra {kind} hit at beat {} (frame {f})", beat(*f)) });
        }
        for (i, j) in &pairs {
            let (re, ce) = (r[*i].1, c[*j].1);
            let (rl, cl) = ((re.f1 - re.f0) as f64, (ce.f1 - ce.f0) as f64 / ratio);
            if (rl - cl).abs() >= 2.0 {
                diffs.push(Difference {
                    category: "tone".into(),
                    shot: Some(r[*i].0),
                    severity: (rl - cl).abs() / 2.0,
                    text: format!("shot {}: {kind} hit at beat {} lasts {cl:.0} frames, reference {rl:.0}", r[*i].0, beat(re.f0)),
                });
            }
        }
        tone_total.reference += m.reference;
        tone_total.candidate += m.candidate;
        tone_total.matched += m.matched;
        off_sum += m.mean_offset_frames * m.matched as f64;
        tone_total.missing.extend(m.missing);
        tone_total.extra.extend(m.extra);
    }
    if tone_total.reference + tone_total.candidate > 0 {
        tone_total.precision = round_to(if tone_total.candidate > 0 { tone_total.matched as f64 / tone_total.candidate as f64 } else { 1.0 }, 3);
        tone_total.recall = round_to(if tone_total.reference > 0 { tone_total.matched as f64 / tone_total.reference as f64 } else { 1.0 }, 3);
        let (p, r) = (tone_total.precision, tone_total.recall);
        tone_total.f1 = round_to(if p + r > 0.0 { 2.0 * p * r / (p + r) } else { 0.0 }, 3);
        tone_total.mean_offset_frames = round_to(off_sum / tone_total.matched.max(1) as f64, 2);
    }
    scores.insert("tone_hits".into(), Some(tone_total.f1));

    // ---- curves per reference shot ---------------------------------------------
    let (rs, cs) = (&reference.signature, &cand.signature);
    let cand_shot = |f: usize| cand.shots.iter().position(|s| to_ref(s.f0) <= f && f < to_ref(s.f1).max(to_ref(s.f0) + 1));
    let mut shots_out: Vec<ShotCmp> = Vec::new();
    let mut acc: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
    for shot in &reference.shots {
        let mut sc = ShotCmp { shot: shot.id, f0: shot.f0, f1: shot.f1, beat0: shot.beat0, beat1: shot.beat1, curves: BTreeMap::new(), notes: vec![] };
        let n = shot.f1 - shot.f0;
        let home = cand_shot(shot.ref_frame);
        for s in &SIGNALS {
            let (rv, cv) = (series(rs, s.name), series(cs, s.name));
            let get_r = |f: usize| rv.get(f.wrapping_sub(rs.f0)).copied().flatten();
            let get_c = |f: usize| {
                let cf = (f as f64 * ratio).round() as usize;
                cv.get(cf.wrapping_sub(cs.f0)).copied().flatten()
            };
            // Re-base both curves at the reference's reference frame, so they are
            // relative to the same instant (each analysis measures a shot against
            // its own baseline, which need not be that frame).
            let relative = |v: f64, b: f64| match s.name {
                "exposure" => v - b,
                "blur" => (v * v - b * b).max(0.0).sqrt(),
                _ if b.abs() > 0.05 => v / b,
                _ => v,
            };
            let (base_r, base) = (get_r(shot.ref_frame), get_c(shot.ref_frame));
            let rebase = |f: usize, v: f64| match (base_r, base, home, cand_shot(f)) {
                (Some(_), Some(b), Some(h), Some(here)) if h == here => relative(v, b),
                _ => v,
            };
            let rebase_r = |v: f64| match (base_r, base, home) {
                (Some(b), Some(_), Some(_)) => relative(v, b),
                _ => v,
            };
            let frames: Vec<usize> = (shot.f0..shot.f1).filter(|f| get_r(*f).is_some() && get_c(*f).is_some()).collect();
            let ref_has = (shot.f0..shot.f1).filter(|f| get_r(*f).is_some()).count();
            if s.name == "scale" && ref_has * 2 < n {
                continue; // the reference itself has no track here
            }
            if frames.len() * 2 < ref_has.max(1) || frames.len() < 2 {
                if s.name == "scale" {
                    sc.notes.push("zoom could not be measured in the candidate for this shot".into());
                    diffs.push(Difference { category: "scale".into(), shot: Some(shot.id), severity: 2.0, text: format!("shot {}: the candidate's motion could not be registered (reference has a zoom track)", shot.id) });
                    let e = acc.entry(s.name).or_insert((0.0, 0.0));
                    e.1 += n as f64;
                }
                continue;
            }
            let r: Vec<f64> = frames.iter().map(|f| rebase_r(get_r(*f).unwrap())).collect();
            let c: Vec<f64> = frames.iter().map(|f| rebase(*f, get_c(*f).unwrap())).collect();
            let cmp = curve_cmp(&r, &c, s, fpb);
            let e = acc.entry(s.name).or_insert((0.0, 0.0));
            e.0 += (1.0 - cmp.distance).max(0.0) * n as f64;
            e.1 += n as f64;
            let id = shot.id;
            let span = format!("shot {id} (beats {:.2}-{:.2})", shot.beat0, shot.beat1);
            let mut feature = |what: &str, rv: f64, cv: f64| {
                if (rv - cv).abs() > s.tol {
                    diffs.push(Difference {
                        category: s.name.into(),
                        shot: Some(id),
                        severity: (rv - cv).abs() / s.tol,
                        text: format!("{span}: {} {what} {cv:.2}{}, reference {rv:.2}{}", s.label, s.unit, s.unit),
                    });
                }
            };
            feature("starts at", cmp.ref_start, cmp.cand_start);
            feature("ends at", cmp.ref_end, cmp.cand_end);
            feature("peaks at", cmp.ref_peak, cmp.cand_peak);
            let significant = (cmp.ref_peak - s.neutral).abs() > 2.0 * s.tol && (cmp.cand_peak - s.neutral).abs() > 2.0 * s.tol;
            let late = cmp.cand_peak_frame - cmp.ref_peak_frame;
            if significant && late.abs() >= 2.0 {
                diffs.push(Difference {
                    category: s.name.into(),
                    shot: Some(id),
                    severity: late.abs() / 1.5,
                    text: format!("{span}: {} peak is {:.0} frames {}", s.label, late.abs(), if late > 0.0 { "late" } else { "early" }),
                });
            }
            sc.curves.insert(s.name.into(), cmp);
        }
        shots_out.push(sc);
    }
    for s in &SIGNALS {
        scores.insert(s.name.into(), acc.get(s.name).filter(|e| e.1 > 0.0).map(|e| round_to(e.0 / e.1, 3)));
    }

    diffs.sort_by(|a, b| b.severity.partial_cmp(&a.severity).unwrap());
    diffs.iter_mut().for_each(|d| d.severity = round_to(d.severity, 2));
    let known: Vec<f64> = scores.values().filter_map(|v| *v).collect();
    let overall = if known.is_empty() { 0.0 } else { known.iter().sum::<f64>() / known.len() as f64 };
    Comparison {
        schema: "edits.signature_comparison/1".into(),
        reference: reference.source.path.clone(),
        candidate: cand.source.path.clone(),
        scores,
        overall: round_to(overall, 3),
        tempo,
        cuts,
        flashes,
        tone_hits: tone_total,
        shots: shots_out,
        differences: diffs,
    }
}

/// Analyse `video` with the reference's beat grid and frame range, and compare.
pub fn compare(reference: &Analysis, video: &Path, height: Option<u32>) -> Result<Comparison> {
    let info = media::probe(video)?;
    let ratio = info.fps / reference.source.fps;
    let map = |f: usize| (f as f64 * ratio).round() as usize;
    let opts = AnalyzeOpts {
        height: height.unwrap_or(reference.source.analysis_height as u32),
        bpm: Some(reference.beats.bpm.value),
        beat_offset: Some(reference.beats.offset_seconds.value),
        samples: false,
        whole: false,
        range: Some((map(reference.ingest.analysed_f0), map(reference.ingest.analysed_f1))),
    };
    let cand = pipeline::analyze_video(video, None, &opts)?;
    let tempo = media::decode_audio(video, &info, 22050)?.and_then(|a| beats::tempo(&beats::onset_envelope(&a, 22050), &Default::default()));
    Ok(compare_analyses(reference, &cand, tempo.as_ref()))
}

/// Plain-text report: scores, event tables and the largest differences.
pub fn report(c: &Comparison, top: usize) -> String {
    let mut s = String::new();
    s += &format!("reference: {}\ncandidate: {}\n", c.reference, c.candidate);
    s += &format!("overall {:.2}", c.overall);
    for (k, v) in &c.scores {
        s += &match v {
            Some(v) => format!("  {k} {v:.2}"),
            None => format!("  {k} n/a"),
        };
    }
    s += "\n";
    s += &format!(
        "tempo: reference {:.2} bpm, candidate {} ({})\n",
        c.tempo.reference_bpm,
        c.tempo.candidate_bpm.map(|b| format!("{b:.2} bpm")).unwrap_or("n/a".into()),
        c.tempo.note
    );
    for (name, m) in [("cuts", &c.cuts), ("flashes", &c.flashes), ("tone hits", &c.tone_hits)] {
        s += &format!(
            "{name}: {} in reference, {} in candidate, {} matched within 1 frame; precision {:.2}, recall {:.2}, mean offset {:+.2} frames\n",
            m.reference, m.candidate, m.matched, m.precision, m.recall, m.mean_offset_frames
        );
    }
    if c.differences.is_empty() {
        s += "no differences above tolerance\n";
    } else {
        s += &format!("largest differences ({} of {}):\n", top.min(c.differences.len()), c.differences.len());
        for (i, d) in c.differences.iter().take(top).enumerate() {
            s += &format!("{:3}. [{} {:.1}] {}\n", i + 1, d.category, d.severity, d.text);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_match_one_to_one_within_tolerance() {
        let (m, pairs) = match_events(&[10, 20, 30, 40], &[11, 19, 33, 41, 60], 1);
        assert_eq!(pairs, vec![(0, 0), (1, 1), (3, 3)]);
        assert_eq!((m.matched, m.missing.clone(), m.extra.clone()), (3, vec![30], vec![33, 60]));
        assert!((m.precision - 0.6).abs() < 1e-9 && (m.recall - 0.75).abs() < 1e-9);
        assert!((m.mean_offset_frames - 0.33).abs() < 0.01);
    }

    #[test]
    fn curve_distance_and_features() {
        let s = &SIGNALS[3];
        let r: Vec<f64> = (0..12).map(|t| 1.0 + 0.32 * 0.6f64.powi(t)).collect();
        let same = curve_cmp(&r, &r, s, 12.0);
        assert_eq!(same.distance, 0.0);
        // Smaller zoom, peaking two frames late.
        let c: Vec<f64> = (0..12).map(|t| if t < 2 { 1.0 } else { 1.0 + 0.18 * 0.6f64.powi(t - 2) }).collect();
        let d = curve_cmp(&r, &c, s, 12.0);
        assert!(d.distance > 0.5, "{d:?}");
        assert!((d.ref_peak - 1.32).abs() < 1e-9 && (d.cand_peak - 1.18).abs() < 1e-9);
        assert_eq!((d.ref_peak_frame, d.cand_peak_frame), (0.0, 2.0));
    }
}
