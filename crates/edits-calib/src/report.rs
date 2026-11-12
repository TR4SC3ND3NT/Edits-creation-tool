//! Aggregate scores into an accuracy table per estimator: bias, spread,
//! share within tolerance, hit and miss rates, split by picture kind and by
//! condition (clean render, degraded).

use crate::score::Scores;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "edits.calibration_report/1";

/// The tolerance an estimator is held to. `relative` errors are fractions of the truth.
pub struct Tolerance {
    pub est: &'static str,
    pub unit: &'static str,
    pub tol: f64,
    pub relative: bool,
    pub what: &'static str,
}

const fn t(est: &'static str, unit: &'static str, tol: f64, relative: bool, what: &'static str) -> Tolerance {
    Tolerance { est, unit, tol, relative, what }
}

pub const TOLERANCES: &[Tolerance] = &[
    t("tempo.bpm", "bpm", 0.5, false, "tempo from the click track"),
    t("tempo.phase", "frames", 1.0, false, "beat 0 against the true beat offset"),
    t("cut.frame", "frames", 1.0, false, "frame of a matched cut"),
    t("flash.frame", "frames", 1.0, false, "peak frame of a matched flash"),
    t("flash.peak", "stops", 0.5, false, "flash peak, not clipped"),
    t("flash.peak_clipped", "stops", 0.75, false, "flash peak reported as a lower bound (clipped at white)"),
    t("flash.rate", "per frame", 0.1, false, "decay rate of a flash of 4 frames or more"),
    t("exposure.curve", "stops", 0.4, false, "per-frame exposure where the truth is not 0"),
    t("exposure.rest", "stops", 0.15, false, "per-frame exposure where the truth is 0"),
    t("saturation.amount", "x", 0.15, false, "level of a matched saturation hit"),
    t("saturation.curve", "x", 0.15, false, "per-frame saturation inside a hit"),
    t("saturation.rest", "x", 0.1, false, "per-frame saturation outside hits and flashes (truth 1)"),
    t("saturation.rest_flash", "x", 0.25, false, "per-frame saturation under a flash of more than 1 stop (truth 1)"),
    t("saturation.frames", "frames", 1.0, false, "length of a matched saturation hit"),
    t("invert.frames", "frames", 1.0, false, "length of a matched inversion hit"),
    t("threshold.frames", "frames", 1.0, false, "length of a matched threshold hit"),
    t("threshold.level", "0..1", 0.08, false, "threshold level"),
    t("blur.radius", "relative", 0.2, true, "per-frame Gaussian radius where the truth is 0.3% or more"),
    t("blur.rest", "% of height", 0.1, false, "per-frame blur where the truth is 0"),
    t("blur.rest_flash", "% of height", 0.2, false, "per-frame blur under a flash where the truth is 0"),
    t("scale", "x", 0.01, false, "per-frame scale relative to the reference frame"),
    t("position.x", "frame heights", 0.005, false, "per-frame horizontal position"),
    t("position.y", "frame heights", 0.005, false, "per-frame vertical position"),
    t("rotation", "degrees", 0.3, false, "per-frame rotation (clockwise positive)"),
    t("vignette.amount", "0..1", 0.1, false, "vignette amount"),
    t("vignette.radius", "half-diagonals", 0.12, false, "vignette radius"),
    t("vignette.softness", "half-diagonals", 0.2, false, "vignette softness"),
    t("vignette.gain", "linear gain", 0.08, false, "largest difference between the fitted and the true falloff curve"),
    t("slots.count", "slots", 0.5, false, "number of slots against the number of pictures"),
    t("draft.overall", "score", 0.1, false, "analysis of a render of the draft against the analysis it was drafted from"),
    t("draft.cuts", "score", 0.1, false, ""),
    t("draft.flashes", "score", 0.15, false, ""),
    t("draft.tone_hits", "score", 0.15, false, ""),
    t("draft.exposure", "score", 0.15, false, ""),
    t("draft.blur", "score", 0.2, false, ""),
    t("draft.saturation", "score", 0.15, false, ""),
    t("draft.scale", "score", 0.15, false, ""),
    t("draft.position", "score", 0.15, false, ""),
];

pub fn tolerance(est: &str) -> Option<&'static Tolerance> {
    TOLERANCES.iter().find(|t| t.est == est)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Row {
    pub est: String,
    pub condition: String,
    /// `all`, or one picture kind.
    pub kind: String,
    pub n: usize,
    /// Mean signed error (estimate minus truth; a fraction of the truth when `relative`).
    pub bias: f64,
    /// Standard deviation of the error.
    pub spread: f64,
    /// 90th percentile of the absolute error.
    pub p90: f64,
    /// Share of samples within the tolerance.
    pub within: f64,
    pub tol: f64,
    pub unit: String,
    pub relative: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EventRow {
    pub est: String,
    pub condition: String,
    pub kind: String,
    pub hits: usize,
    pub misses: usize,
    pub extras: usize,
    pub recall: f64,
    pub precision: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorstCase {
    pub seed: u64,
    pub badness: f64,
    pub file: String,
    pub why: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Report {
    pub schema: String,
    pub tool: String,
    pub adapter: String,
    pub cases: usize,
    pub seed: u64,
    pub height: u32,
    pub conditions: Vec<String>,
    pub rows: Vec<Row>,
    pub events: Vec<EventRow>,
    pub worst: Vec<WorstCase>,
    pub frames_rendered: u64,
    pub seconds: f64,
}

fn round(v: f64, d: i32) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    let m = 10f64.powi(d);
    (v * m).round() / m
}

pub fn error_of(est: &str, truth: f64, got: f64) -> f64 {
    match tolerance(est) {
        Some(t) if t.relative => (got - truth) / truth.abs().max(1e-9),
        _ => got - truth,
    }
}

fn row(est: &str, condition: &str, kind: &str, errs: &[f64]) -> Row {
    let n = errs.len();
    let tol = tolerance(est);
    let mean = errs.iter().sum::<f64>() / n.max(1) as f64;
    let var = errs.iter().map(|e| (e - mean).powi(2)).sum::<f64>() / n.max(1) as f64;
    let mut abs: Vec<f64> = errs.iter().map(|e| e.abs()).collect();
    abs.sort_by(|a, b| a.total_cmp(b));
    let p90 = if n == 0 { 0.0 } else { abs[((n - 1) as f64 * 0.9).round() as usize] };
    let lim = tol.map(|t| t.tol).unwrap_or(f64::INFINITY);
    Row {
        est: est.into(),
        condition: condition.into(),
        kind: kind.into(),
        n,
        bias: round(mean, 4),
        spread: round(var.sqrt(), 4),
        p90: round(p90, 4),
        within: round(abs.iter().filter(|e| **e <= lim).count() as f64 / n.max(1) as f64, 3),
        tol: tol.map(|t| t.tol).unwrap_or(0.0),
        unit: tol.map(|t| t.unit).unwrap_or("").into(),
        relative: tol.map(|t| t.relative).unwrap_or(false),
    }
}

/// Rows per (condition, estimator) and per kind, in the order of [`TOLERANCES`].
pub fn aggregate(all: &[(String, Scores)]) -> (Vec<Row>, Vec<EventRow>) {
    let mut errs: BTreeMap<(String, String, String), Vec<f64>> = BTreeMap::new();
    let mut counts: BTreeMap<(String, String, String), (usize, usize, usize)> = BTreeMap::new();
    for (cond, sc) in all {
        for s in &sc.samples {
            let e = error_of(&s.est, s.truth, s.got);
            errs.entry((cond.clone(), s.est.clone(), "all".into())).or_default().push(e);
            if s.kind != "-" {
                errs.entry((cond.clone(), s.est.clone(), s.kind.clone())).or_default().push(e);
            }
        }
        for c in &sc.counts {
            for k in ["all", c.kind.as_str()] {
                if k == "-" {
                    continue;
                }
                let e = counts.entry((cond.clone(), c.est.clone(), k.into())).or_default();
                e.0 += c.hits;
                e.1 += c.misses;
                e.2 += c.extras;
            }
        }
    }
    let order = |est: &str| TOLERANCES.iter().position(|t| t.est == est).unwrap_or(TOLERANCES.len());
    let mut rows: Vec<Row> = errs.iter().map(|((c, e, k), v)| row(e, c, k, v)).collect();
    rows.sort_by(|a, b| (order(&a.est), &a.est, &a.condition, a.kind != "all", &a.kind).cmp(&(order(&b.est), &b.est, &b.condition, b.kind != "all", &b.kind)));
    let events: Vec<EventRow> = counts
        .iter()
        .map(|((c, e, k), (h, m, x))| EventRow {
            est: e.clone(),
            condition: c.clone(),
            kind: k.clone(),
            hits: *h,
            misses: *m,
            extras: *x,
            recall: round(*h as f64 / (*h + *m).max(1) as f64, 3),
            precision: round(*h as f64 / (*h + *x).max(1) as f64, 3),
        })
        .collect();
    (rows, events)
}

/// How badly a case went: mean error in units of tolerance, plus missed and extra events.
pub fn badness(sc: &Scores) -> (f64, Vec<String>) {
    let mut per: BTreeMap<&str, (f64, usize)> = BTreeMap::new();
    for s in &sc.samples {
        if let Some(t) = tolerance(&s.est) {
            let e = per.entry(s.est.as_str()).or_default();
            e.0 += (error_of(&s.est, s.truth, s.got).abs() / t.tol).min(10.0);
            e.1 += 1;
        }
    }
    let mut parts: Vec<(f64, String)> = per.iter().map(|(k, (sum, n))| (sum / *n as f64, format!("{k}: mean error {:.1} tolerances over {n} samples", sum / *n as f64))).collect();
    let mut ev: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    for c in &sc.counts {
        let e = ev.entry(c.est.as_str()).or_default();
        e.0 += c.hits;
        e.1 += c.misses;
        e.2 += c.extras;
    }
    for (k, (h, m, x)) in ev {
        if m + x > 0 && k != "track.frames" {
            parts.push((2.0 * (m + x) as f64, format!("{k}: {h} found, {m} missed, {x} extra")));
        }
    }
    parts.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total = parts.iter().map(|p| p.0).sum::<f64>();
    (round(total, 2), parts.into_iter().filter(|p| p.0 >= 1.0).take(6).map(|p| p.1).collect())
}

/// The readable table.
pub fn table(r: &Report) -> String {
    let mut s = String::new();
    s += &format!("calibration: {} cases from seed {}, {} px high, conditions: {}; {} frames rendered in {:.0} s on {}\n", r.cases, r.seed, r.height, r.conditions.join(", "), r.frames_rendered, r.seconds, r.adapter);
    for cond in &r.conditions {
        s += &format!("\n== {cond}: magnitudes (error = estimate - truth) ==\n");
        s += &format!("{:<22} {:>6} {:>9} {:>9} {:>9} {:>7}  {:<16} by kind: bias (n)\n", "estimator", "n", "bias", "spread", "p90|err|", "in tol", "tolerance");
        let mut seen: Vec<&str> = Vec::new();
        for row in r.rows.iter().filter(|x| &x.condition == cond && x.kind == "all") {
            if seen.contains(&row.est.as_str()) {
                continue;
            }
            seen.push(&row.est);
            let kinds: Vec<String> = r
                .rows
                .iter()
                .filter(|x| &x.condition == cond && x.est == row.est && x.kind != "all")
                .map(|x| format!("{} {:+.3} ({})", x.kind, x.bias, x.n))
                .collect();
            s += &format!(
                "{:<22} {:>6} {:>+9.3} {:>9.3} {:>9.3} {:>6.0}%  {:<16} {}\n",
                row.est,
                row.n,
                row.bias,
                row.spread,
                row.p90,
                100.0 * row.within,
                if row.relative { format!("{:.0}% of truth", 100.0 * row.tol) } else { format!("{} {}", row.tol, row.unit) },
                kinds.join(", ")
            );
        }
        s += &format!("\n== {cond}: events ==\n");
        s += &format!("{:<22} {:>6} {:>6} {:>6} {:>7} {:>9}  by kind: found/missed/extra\n", "estimator", "found", "missed", "extra", "recall", "precision");
        for e in r.events.iter().filter(|x| &x.condition == cond && x.kind == "all") {
            let kinds: Vec<String> = r.events.iter().filter(|x| &x.condition == cond && x.est == e.est && x.kind != "all").map(|x| format!("{} {}/{}/{}", x.kind, x.hits, x.misses, x.extras)).collect();
            s += &format!("{:<22} {:>6} {:>6} {:>6} {:>6.0}% {:>8.0}%  {}\n", e.est, e.hits, e.misses, e.extras, 100.0 * e.recall, 100.0 * e.precision, kinds.join(", "));
        }
    }
    if !r.worst.is_empty() {
        s += "\n== worst cases (replay with `edits calib case <file>`) ==\n";
        for w in &r.worst {
            s += &format!("seed {:<6} badness {:>6.1}  {}\n", w.seed, w.badness, w.file);
            for line in &w.why {
                s += &format!("    {line}\n");
            }
        }
    }
    s
}
