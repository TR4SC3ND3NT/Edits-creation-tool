//! Automatic fitting: adjust a project's effect and motion magnitudes so that
//! its render measures like a reference, in signature space.
//!
//! The draft of an analysis replays the reference's timing exactly, but its
//! magnitudes are only as good as the estimators, and the same flash looks
//! different on another picture (a dark picture clips later than a white
//! one). `fit` closes that loop: for each reference shot it renders only that
//! shot's frames, in-process and at the reference's analysis resolution,
//! measures them with the same stages `edits-analyze compare` uses
//! ([`edits_analyze::pipeline::measure_shot`]), scores them against the
//! reference shot with [`edits_analyze::compare::compare_analyses`], and
//! moves the shot's numeric parameters with a bounded, deterministic
//! coordinate search until the score stops improving or the render budget is
//! spent. Key times, easing names, slot paths, layer structure and anything
//! outside the shot's own layer are never changed.
//!
//! Because both sides are measured relative to their own shot, the pictures
//! in the slots do not have to be the reference's: that is the point.

use crate::recipe::is_shot_layer;
use crate::render::Stage;
use crate::rng::Rng;
use anyhow::{bail, Context, Result};
use edits_analyze::compare::{compare_analyses, Comparison};
use edits_analyze::draft::sample_curve;
use edits_analyze::img::Frames;
use edits_analyze::pipeline::{analyze_frames, measure_shot, AnalyzeOpts, ShotContext};
use edits_analyze::schema::{Analysis, Curve, Shot, Signature, Source};
use edits_core::{Evaluator, Project};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const SCHEMA: &str = "edits.fit_report/1";

/// Parameter groups `fit` knows. `auto` is all of them.
pub const GROUPS: &[(&str, &str)] = &[
    ("exposure", "exposure `stops` key values that are not 0 (flash peaks)"),
    ("exposure_decay", "`decay` rates of exposure keys"),
    ("saturation", "saturation `amount` values that are not 1"),
    ("threshold", "threshold `level`"),
    ("blur", "gaussian_blur `radius` values that are not 0"),
    ("scale", "scale key values other than the shot's resting (smallest) one"),
    ("scale_decay", "`decay` rates of scale keys"),
    ("position", "position key values (x, y) other than the resting (last) one"),
    ("position_decay", "`decay` rates of position keys"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FitOpts {
    /// Reference shot ids (1-based, as in the analysis) to fit; empty = every shot.
    pub shots: Vec<usize>,
    /// Parameter groups (see [`GROUPS`]); `["auto"]` = all.
    pub params: Vec<String>,
    /// Renders of a shot allowed per shot, the starting point included.
    pub budget: usize,
    /// Render scale; `None` renders at the reference's analysis height.
    pub scale: Option<f32>,
    /// Report what would change without writing the project.
    pub dry_run: bool,
    /// Seeds the order in which parameters are visited.
    pub seed: u64,
}

impl Default for FitOpts {
    fn default() -> Self {
        FitOpts { shots: vec![], params: vec!["auto".into()], budget: 24, scale: None, dry_run: false, seed: 1 }
    }
}

/// One parameter that was changed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub layer: String,
    /// JSON pointer into the project file.
    pub pointer: String,
    /// Parameter group (see [`GROUPS`]).
    pub group: String,
    /// A readable name, e.g. `exposure: exposure.stops key 1`.
    pub what: String,
    pub old: f64,
    pub new: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShotFit {
    /// Reference shot id.
    pub shot: usize,
    pub layer: String,
    pub f0: usize,
    pub f1: usize,
    /// Parameters searched.
    pub params: usize,
    /// Renders of this shot (objective evaluations).
    pub renders: usize,
    /// The minimised distance (0 = measures like the reference; see [`objective`]).
    pub objective_before: f64,
    pub objective_after: f64,
    /// `compare` scores of this shot alone, before and after.
    pub before: BTreeMap<String, Option<f64>>,
    pub after: BTreeMap<String, Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FitReport {
    pub schema: String,
    pub project: String,
    pub reference: String,
    /// The project file was rewritten (false for a dry run or when nothing improved).
    pub written: bool,
    pub scale: f32,
    /// Whole-clip `compare` scores of the in-process render (no audio, so no tempo) before and after.
    pub before: BTreeMap<String, Option<f64>>,
    pub after: BTreeMap<String, Option<f64>>,
    pub overall_before: f64,
    pub overall_after: f64,
    pub shots: Vec<ShotFit>,
    pub changes: Vec<Change>,
    /// Shot renders plus the two whole-clip renders.
    pub renders: usize,
    pub frames_rendered: u64,
    pub seconds: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A searchable number in the project JSON.
#[derive(Clone, Debug)]
struct Param {
    pointer: String,
    group: &'static str,
    what: String,
    value: f64,
    lo: f64,
    hi: f64,
    decimals: i32,
}

fn round(v: f64, d: i32) -> f64 {
    let m = 10f64.powi(d);
    (v * m).round() / m
}

fn wanted(opts: &FitOpts, group: &str) -> bool {
    opts.params.iter().any(|p| p == "auto" || p == "all" || p == group)
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

/// Key values (scalars or vector components) and decay rates of an animated
/// property at `base` (a JSON pointer to the property). `neutral(i, v)` says
/// which values are left alone.
fn keyed_params(prop: &Value, base: &str, label: &str, group: &'static str, decay_group: &'static str, opts: &FitOpts, out: &mut Vec<Param>, bounds: &dyn Fn(f64, usize) -> Option<(f64, f64, i32)>) {
    match prop {
        Value::Number(n) => {
            if wanted(opts, group) {
                if let Some((lo, hi, d)) = bounds(n.as_f64().unwrap_or(0.0), 0) {
                    out.push(Param { pointer: base.to_string(), group, what: format!("{group}: {label}"), value: n.as_f64().unwrap_or(0.0), lo, hi, decimals: d });
                }
            }
        }
        Value::Object(o) => {
            let Some(Value::Array(keys)) = o.get("keys") else { return };
            for (k, key) in keys.iter().enumerate() {
                if wanted(opts, group) {
                    match key.get("v") {
                        Some(Value::Number(n)) => {
                            let v = n.as_f64().unwrap_or(0.0);
                            if let Some((lo, hi, d)) = bounds(v, 0) {
                                out.push(Param { pointer: format!("{base}/keys/{k}/v"), group, what: format!("{group}: {label} key {k}"), value: v, lo, hi, decimals: d });
                            }
                        }
                        Some(Value::Array(a)) => {
                            for (c, x) in a.iter().enumerate().take(2) {
                                let v = num(x).unwrap_or(0.0);
                                if let Some((lo, hi, d)) = bounds(v, c) {
                                    out.push(Param { pointer: format!("{base}/keys/{k}/v/{c}"), group, what: format!("{group}: {label} key {k} [{c}]"), value: v, lo, hi, decimals: d });
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if wanted(opts, decay_group) && k + 1 < keys.len() {
                    if let Some(rate) = key.get("ease").filter(|e| e.get("name").and_then(Value::as_str) == Some("decay")).and_then(|e| e.get("rate")).and_then(num) {
                        out.push(Param { pointer: format!("{base}/keys/{k}/ease/rate"), group: decay_group, what: format!("{decay_group}: {label} key {k} decay rate"), value: rate, lo: 0.03, hi: 0.95, decimals: 3 });
                    }
                }
            }
        }
        _ => {}
    }
}

/// The parameters of layer `li` that `opts` asks for.
fn layer_params(project: &Value, li: usize, opts: &FitOpts) -> Vec<Param> {
    let mut out = Vec::new();
    let layer = &project["layers"][li];
    let base = format!("/layers/{li}");
    let keys_of = |p: &Value| -> Vec<Value> { p.get("keys").and_then(Value::as_array).map(|k| k.iter().filter_map(|k| k.get("v").cloned()).collect()).unwrap_or_default() };
    // Scale: relative only, so the resting (smallest) value stays fixed and the others may not go below it (the frame stays covered).
    let scale = &layer["transform"]["scale"];
    let svals: Vec<f64> = keys_of(scale).iter().filter_map(num).collect();
    if svals.len() >= 2 {
        let rest = svals.iter().copied().fold(f64::MAX, f64::min);
        keyed_params(scale, &format!("{base}/transform/scale"), "scale", "scale", "scale_decay", opts, &mut out, &|v, _| {
            ((v - rest).abs() > 1e-6).then(|| (rest, (v + (v - rest).abs().max(0.05)).min(rest * 2.5), 4))
        });
    }
    // Position: the last key is where the picture rests.
    let pos = &layer["transform"]["position"];
    let pvals = keys_of(pos);
    if pvals.len() >= 2 {
        let rest: Vec<f64> = pvals.last().and_then(Value::as_array).map(|a| a.iter().filter_map(num).collect()).unwrap_or_default();
        let all: Vec<Vec<f64>> = pvals.iter().map(|v| v.as_array().map(|a| a.iter().filter_map(num).collect()).unwrap_or_default()).collect();
        keyed_params(pos, &format!("{base}/transform/position"), "position", "position", "position_decay", opts, &mut out, &|v, c| {
            let r = rest.get(c).copied().unwrap_or(0.0);
            // Skip the resting key itself (all of its components equal the rest).
            let is_rest = all.iter().any(|k| k.get(c).map(|x| (x - v).abs() < 1e-9).unwrap_or(false) && k.iter().zip(&rest).all(|(a, b)| (a - b).abs() < 1e-9));
            (!is_rest).then(|| {
                let span = (v - r).abs().max(0.03);
                (v - span, v + span, 4)
            })
        });
    }
    let Some(effects) = layer.get("effects").and_then(Value::as_array) else { return out };
    for (ei, e) in effects.iter().enumerate() {
        if e.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let kind = e.get("kind").and_then(Value::as_str).unwrap_or("");
        let pbase = format!("{base}/effects/{ei}/params");
        let params = &e["params"];
        let label = |p: &str| format!("{kind}.{p}");
        match kind {
            "exposure" => keyed_params(&params["stops"], &format!("{pbase}/stops"), &label("stops"), "exposure", "exposure_decay", opts, &mut out, &|v, _| {
                (v.abs() > 1e-6).then(|| ((v - 2.0).max(-3.0), (v + 2.0).min(8.0), 2))
            }),
            "saturation" => keyed_params(&params["amount"], &format!("{pbase}/amount"), &label("amount"), "saturation", "saturation", opts, &mut out, &|v, _| ((v - 1.0).abs() > 1e-6).then_some((0.0, 2.0, 3))),
            // Inversion and threshold hits are on/off toggles: their timing comes from the analysis, their amount stays 1.
            "threshold" => keyed_params(&params["level"], &format!("{pbase}/level"), &label("level"), "threshold", "threshold", opts, &mut out, &|_, _| Some((0.05, 0.95, 3))),
            "gaussian_blur" => keyed_params(&params["radius"], &format!("{pbase}/radius"), &label("radius"), "blur", "blur", opts, &mut out, &|v, _| (v.abs() > 1e-6).then(|| (0.0, (3.0 * v).max(v + 2.0), 3))),
            _ => {}
        }
    }
    out
}

/// The shot layer on screen at reference frame `f`: the topmost enabled image
/// layer limited in time (see [`is_shot_layer`]).
fn layer_at(project: &Project, f: usize) -> Option<usize> {
    let ev = Evaluator::new(project);
    let t = project.frame_time(f as u64) + 0.5 * project.frame_len();
    project.layers.iter().enumerate().rev().find(|(_, l)| is_shot_layer(project, l) && ev.active(l, t)).map(|(i, _)| i)
}

/// The analysis the reference shot `shot` would have on its own.
fn sub_analysis(a: &Analysis, shot: &Shot, sig: &Signature) -> Analysis {
    let mut s = a.clone();
    s.shots = vec![shot.clone()];
    let (i0, i1) = (shot.f0 - sig.f0, shot.f1 - sig.f0);
    s.signature = Signature {
        f0: shot.f0,
        lum: sig.lum[i0..i1].to_vec(),
        stops: sig.stops[i0..i1].to_vec(),
        blur: sig.blur[i0..i1].to_vec(),
        sat: sig.sat[i0..i1].to_vec(),
        scale: sig.scale[i0..i1].to_vec(),
    };
    s.slots = vec![];
    s
}

fn position_curve(shot: &Shot) -> Option<Vec<Vec<f64>>> {
    shot.curves.iter().find(|c: &&Curve| c.prop == "position").map(|c| sample_curve(c, shot.f0, shot.f1))
}

/// Distance of the candidate shot's position curve from the reference's
/// (both relative to the reference's reference frame), normalised like
/// `compare`'s curves with a floor of 0.01 frame heights. `None` when
/// neither moves.
fn position_distance(r: &Shot, c: &Shot) -> Option<f64> {
    let (pr, pc) = (position_curve(r), position_curve(c));
    if pr.is_none() && pc.is_none() {
        return None;
    }
    let n = r.f1 - r.f0;
    let zero = vec![vec![0.0, 0.0]; n];
    let (pr, pc) = (pr.unwrap_or_else(|| zero.clone()), pc.unwrap_or(zero));
    let k = r.ref_frame.saturating_sub(r.f0).min(n - 1);
    let (br, bc) = (pr[k].clone(), pc[k].clone());
    let (mut num, mut den) = (0.0, 0.0);
    for i in 0..n {
        for d in 0..2 {
            let (a, b) = (pr[i][d] - br[d], pc[i][d] - bc[d]);
            num += (a - b).powi(2);
            den += a * a;
        }
    }
    Some((num / n as f64).sqrt() / (den / n as f64).sqrt().max(0.01))
}

/// The number `fit` minimises for one shot, from `compare` of the shot alone:
/// the sum of the curve distances (exposure, blur, saturation, scale and,
/// when the shot moves, position) plus twice the shortfall of the flash and
/// tone-hit scores (a missing or extra event costs about as much as a curve
/// that is entirely wrong).
pub fn objective(c: &Comparison, position: Option<f64>) -> f64 {
    let curves: f64 = c.shots.iter().flat_map(|s| s.curves.values()).map(|v| v.distance.min(3.0)).sum();
    let ev = |k: &str| 1.0 - c.scores.get(k).copied().flatten().unwrap_or(1.0);
    curves + position.map(|p| p.min(3.0)).unwrap_or(0.0) + 2.0 * (ev("flashes") + ev("tone_hits"))
}

fn shot_scores(c: &Comparison) -> BTreeMap<String, Option<f64>> {
    c.scores.iter().filter(|(k, _)| !matches!(k.as_str(), "tempo" | "cuts")).map(|(k, v)| (k.clone(), *v)).collect()
}

struct Ctx<'a> {
    stage: Stage,
    dir: &'a Path,
    scale: f32,
    reference: &'a Analysis,
    shot_ctx: ShotContext,
    opts: AnalyzeOpts,
    renders: usize,
}

impl Ctx<'_> {
    /// Render `project` over the reference shot, measure it and compare it with the reference shot.
    fn eval(&mut self, project: &Value, shot: &Shot) -> Result<(f64, Comparison)> {
        let p = Project::from_json(&project.to_string()).context("the edited project does not parse")?;
        let first = shot.f0.saturating_sub(1);
        let frames = self.stage.render(&p, self.dir, self.scale, Some((first, shot.f1)))?;
        self.renders += 1;
        anyhow::ensure!(frames.len() == shot.f1 - first, "the project is shorter than the reference (frame {} not rendered)", shot.f1 - 1);
        let frames = fit_height(frames, self.reference.source.analysis_height);
        let (cand_shot, cand_sig) = measure_shot(&frames, first, shot.f0, shot.f1, shot.id, &self.shot_ctx, &self.opts);
        let r = sub_analysis(self.reference, shot, &self.reference.signature);
        let mut c = r.clone();
        c.source.path = "fit candidate".into();
        c.shots = vec![cand_shot.clone()];
        c.signature = cand_sig;
        let cmp = compare_analyses(&r, &c, None);
        let pos = position_distance(shot, &cand_shot);
        Ok((objective(&cmp, pos), cmp))
    }
}

/// Resize frames to `h` rows (area average) when the render came out at another height.
fn fit_height(frames: Frames, h: usize) -> Frames {
    if frames.h == h || h == 0 {
        return frames;
    }
    let w = ((frames.w as f64 * h as f64 / frames.h as f64 / 2.0).round() as usize * 2).max(2);
    let mut out = Frames::new(w, h, frames.fps);
    for i in 0..frames.len() {
        let src = frames.rgb(i);
        let mut planes = Vec::new();
        for c in 0..3 {
            let g = edits_analyze::img::Gray { w: frames.w, h: frames.h, d: src.chunks_exact(3).map(|p| p[c] as f32).collect() };
            planes.push(edits_analyze::img::resize_area(&g, w, h));
        }
        let mut rgb = Vec::with_capacity(w * h * 3);
        for k in 0..w * h {
            for p in &planes {
                rgb.push(p.d[k].round().clamp(0.0, 255.0) as u8);
            }
        }
        out.push(&rgb);
    }
    out
}

fn set(v: &mut Value, pointer: &str, x: f64) {
    if let Some(slot) = v.pointer_mut(pointer) {
        *slot = serde_json::json!(x);
    }
}

/// Bounded coordinate search with shrinking steps on parameters scaled to
/// 0..1 by their bounds. Deterministic: the visiting order of each sweep is a
/// shuffle drawn from `seed`. Returns the best values found.
fn coordinate_search(params: &[Param], budget: usize, seed: u64, f0: f64, mut eval: impl FnMut(&[f64]) -> Result<f64>) -> Result<(Vec<f64>, f64, usize)> {
    let n = params.len();
    let to_u = |i: usize, v: f64| ((v - params[i].lo) / (params[i].hi - params[i].lo).max(1e-9)).clamp(0.0, 1.0);
    let to_v = |i: usize, u: f64| round(params[i].lo + u * (params[i].hi - params[i].lo), params[i].decimals);
    let mut x: Vec<f64> = params.iter().map(|p| p.value).collect();
    let mut best = f0;
    let mut used = 1;
    let mut step = vec![0.2f64; n];
    let mut rng = Rng::new(seed);
    while used < budget && step.iter().any(|s| *s >= 0.01) {
        let mut order: Vec<usize> = (0..n).filter(|i| step[*i] >= 0.01).collect();
        for i in (1..order.len()).rev() {
            let j = rng.int(0, i as i64) as usize;
            order.swap(i, j);
        }
        for i in order {
            if used >= budget {
                break;
            }
            let u = to_u(i, x[i]);
            let mut moved = false;
            for dir in [1.0, -1.0] {
                if used >= budget {
                    break;
                }
                let cand = to_v(i, (u + dir * step[i]).clamp(0.0, 1.0));
                if (cand - x[i]).abs() < 10f64.powi(-params[i].decimals) * 0.5 {
                    continue;
                }
                let mut y = x.clone();
                y[i] = cand;
                let f = eval(&y)?;
                used += 1;
                if f < best - 1e-4 {
                    best = f;
                    x = y;
                    moved = true;
                    step[i] = (step[i] * 1.5).min(0.5);
                    break;
                }
            }
            if !moved {
                step[i] *= 0.4;
            }
        }
    }
    Ok((x, best, used))
}

/// Render the analysed range of `project` and analyse it as `compare` would.
fn whole(stage: &mut Stage, project: &Value, dir: &Path, scale: f32, reference: &Analysis) -> Result<(edits_analyze::pipeline::Analyzed, Comparison)> {
    let p = Project::from_json(&project.to_string())?;
    let (a0, a1) = (reference.ingest.analysed_f0, reference.ingest.analysed_f1);
    let frames = stage.render(&p, dir, scale, Some((0, a1)))?;
    anyhow::ensure!(frames.len() >= a1, "the project has {} frames, the reference's analysed range ends at frame {a1}", frames.len());
    let frames = fit_height(frames, reference.source.analysis_height);
    let opts = AnalyzeOpts {
        height: frames.h as u32,
        bpm: Some(reference.beats.bpm.value),
        beat_offset: Some(reference.beats.offset_seconds.value),
        samples: false,
        whole: false,
        range: Some((a0, a1)),
    };
    let src = Source {
        path: "fit render".into(),
        width: frames.w as u32,
        height: frames.h as u32,
        fps: frames.fps,
        frames: frames.len(),
        duration_seconds: frames.len() as f64 / frames.fps,
        has_audio: false,
        analysis_width: frames.w,
        analysis_height: frames.h,
    };
    let done = analyze_frames(&frames, None, src, &opts);
    let cmp = compare_analyses(reference, &done.analysis, None);
    Ok((done, cmp))
}

/// Fit the project at `project_path` to `reference`. Pictures are loaded
/// relative to the project file. Unless `opts.dry_run`, the improved values
/// are written back to the file (pretty JSON as `edits` writes it; key times
/// and everything else unchanged).
pub fn fit(project_path: &Path, reference: &Analysis, opts: &FitOpts) -> Result<FitReport> {
    let started = std::time::Instant::now();
    for p in &opts.params {
        if p != "auto" && p != "all" && !GROUPS.iter().any(|(g, _)| g == p) {
            bail!("unknown parameter group \"{p}\"; use auto or one of: {}", GROUPS.iter().map(|g| g.0).collect::<Vec<_>>().join(", "));
        }
    }
    let text = std::fs::read_to_string(project_path).with_context(|| format!("reading {}", project_path.display()))?;
    let mut doc: Value = serde_json::from_str(&text).with_context(|| format!("{} is not JSON", project_path.display()))?;
    let project = Project::from_json(&text).with_context(|| format!("{} is not an edits project", project_path.display()))?;
    let fps = project.settings.fps;
    if (fps - reference.source.fps).abs() > 1e-6 {
        bail!("the project runs at {fps} fps and the reference at {} fps: fit compares frame by frame, so draft the project from this analysis (or set settings.fps to {})", reference.source.fps, reference.source.fps);
    }
    let dir = project_path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let scale = opts.scale.unwrap_or(reference.source.analysis_height as f32 / project.settings.height as f32).clamp(0.05, 1.0);
    let mut stage = Stage::new()?;
    let (done, before) = whole(&mut stage, &doc, dir, scale, reference)?;
    let shot_ctx = ShotContext::of(&done);
    let mut notes = Vec::new();
    let mut ctx = Ctx {
        stage,
        dir,
        scale,
        reference,
        shot_ctx,
        opts: AnalyzeOpts { height: reference.source.analysis_height as u32, ..Default::default() },
        renders: 0,
    };
    let mut shots_out = Vec::new();
    let mut changes = Vec::new();
    for shot in &reference.shots {
        if !opts.shots.is_empty() && !opts.shots.contains(&shot.id) {
            continue;
        }
        if shot.f0 < reference.signature.f0 || shot.f1 > reference.signature.f0 + reference.signature.stops.len() {
            continue;
        }
        let Some(li) = layer_at(&project, shot.ref_frame) else {
            notes.push(format!("shot {}: no shot layer is on screen at its reference frame {}", shot.id, shot.ref_frame));
            continue;
        };
        let layer_id = project.layers[li].id.clone();
        let params = layer_params(&doc, li, opts);
        let (f_before, cmp_before) = ctx.eval(&doc, shot)?;
        let mut sf = ShotFit {
            shot: shot.id,
            layer: layer_id.clone(),
            f0: shot.f0,
            f1: shot.f1,
            params: params.len(),
            renders: 1,
            objective_before: round(f_before, 4),
            objective_after: round(f_before, 4),
            before: shot_scores(&cmp_before),
            after: shot_scores(&cmp_before),
            note: None,
        };
        if params.is_empty() {
            sf.note = Some("no parameters to fit on this layer".into());
            shots_out.push(sf);
            continue;
        }
        let renders0 = ctx.renders;
        let base = doc.clone();
        let mut best_cmp = cmp_before.clone();
        let mut best_f = f_before;
        let (x, f_after, _) = coordinate_search(&params, opts.budget.max(1), opts.seed ^ shot.id as u64, f_before, |vals| {
            let mut d = base.clone();
            for (p, v) in params.iter().zip(vals) {
                set(&mut d, &p.pointer, *v);
            }
            let (f, cmp) = ctx.eval(&d, shot)?;
            if f < best_f {
                best_f = f;
                best_cmp = cmp;
            }
            Ok(f)
        })?;
        sf.renders = 1 + ctx.renders - renders0;
        sf.objective_after = round(f_after, 4);
        sf.after = shot_scores(&best_cmp);
        // Write the shot's values, and the same values into the layers that repeat it in a loop (`<id>_r2`, ...).
        let repeats: Vec<usize> = project.layers.iter().enumerate().filter(|(_, l)| l.id.starts_with(&format!("{layer_id}_r"))).map(|(i, _)| i).collect();
        for (p, v) in params.iter().zip(&x) {
            if (p.value - v).abs() < 1e-12 {
                continue;
            }
            set(&mut doc, &p.pointer, *v);
            changes.push(Change { layer: layer_id.clone(), pointer: p.pointer.clone(), group: p.group.into(), what: p.what.clone(), old: p.value, new: *v });
            let tail = p.pointer.splitn(4, '/').nth(3).unwrap_or("");
            for r in &repeats {
                let ptr = format!("/layers/{r}/{tail}");
                if doc.pointer(&ptr).and_then(Value::as_f64).map(|old| (old - p.value).abs() < 1e-9).unwrap_or(false) {
                    set(&mut doc, &ptr, *v);
                    changes.push(Change { layer: project.layers[*r].id.clone(), pointer: ptr, group: p.group.into(), what: format!("{} (loop repeat)", p.what), old: p.value, new: *v });
                }
            }
        }
        shots_out.push(sf);
    }
    let (_, after) = whole(&mut ctx.stage, &doc, dir, scale, reference)?;
    let improved = after.overall >= before.overall - 1e-9 && !changes.is_empty();
    if !improved && !changes.is_empty() {
        notes.push(format!("the whole-clip score did not improve ({:.3} -> {:.3}); the project was left unchanged", before.overall, after.overall));
    }
    let mut written = false;
    if improved && !opts.dry_run {
        let p = Project::from_json(&doc.to_string()).context("the fitted project does not parse")?;
        std::fs::write(project_path, p.to_json() + "\n").with_context(|| format!("writing {}", project_path.display()))?;
        written = true;
    }
    Ok(FitReport {
        schema: SCHEMA.into(),
        project: project_path.to_string_lossy().into_owned(),
        reference: reference.source.path.clone(),
        written,
        scale,
        before: before.scores.clone(),
        after: after.scores.clone(),
        overall_before: before.overall,
        overall_after: after.overall,
        shots: shots_out,
        changes,
        renders: ctx.renders + 2,
        frames_rendered: ctx.stage.rendered,
        seconds: round(started.elapsed().as_secs_f64(), 1),
        notes,
    })
}

/// [`fit`] with the reference read from a `reference_analysis.json`.
pub fn fit_file(project_path: &Path, reference_path: &Path, opts: &FitOpts) -> Result<FitReport> {
    let reference = edits_analyze::pipeline::load(reference_path)?;
    fit(project_path, &reference, opts)
}

/// A short plain-text account of a report.
pub fn summary(r: &FitReport) -> String {
    let mut s = format!(
        "fit {}: overall {:.3} -> {:.3}, {} changes, {} renders ({} frames) in {:.0} s{}\n",
        r.project,
        r.overall_before,
        r.overall_after,
        r.changes.len(),
        r.renders,
        r.frames_rendered,
        r.seconds,
        if r.written { ", written" } else { ", not written" }
    );
    for (k, v) in &r.after {
        let b = r.before.get(k).copied().flatten();
        s += &format!("  {k:<11} {} -> {}\n", b.map(|v| format!("{v:.3}")).unwrap_or("n/a".into()), v.map(|v| format!("{v:.3}")).unwrap_or("n/a".into()));
    }
    for sh in &r.shots {
        s += &format!("  shot {:2} {:10} frames {:4}..{:4}: {} params, {:2} renders, objective {:.3} -> {:.3}{}\n", sh.shot, sh.layer, sh.f0, sh.f1, sh.params, sh.renders, sh.objective_before, sh.objective_after, sh.note.as_deref().map(|n| format!(" ({n})")).unwrap_or_default());
    }
    for c in &r.changes {
        s += &format!("  {} {}: {} -> {}\n", c.layer, c.what, c.old, c.new);
    }
    s
}
