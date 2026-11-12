//! The round trip: generate a recipe, render it, (degrade it,) analyse it,
//! score every estimator against the truth; then draft a project from the
//! analysis, render and analyse that, and compare the two analyses.

use crate::recipe::{self, Case, CaseSpec};
use crate::render::{degrade, Degrade, Stage};
use crate::report::{self, Report, WorstCase};
use crate::score::{self, Scores};
use anyhow::{Context, Result};
use edits_analyze::compare::{compare_analyses, Comparison};
use edits_analyze::draft::{draft, DraftOpts};
use edits_analyze::img::Frames;
use edits_analyze::pipeline::{analyze_frames, Analyzed, AnalyzeOpts};
use edits_analyze::schema::{Analysis, Source};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CASE_SCHEMA: &str = "edits.calibration_case/1";

#[derive(Clone, Debug)]
pub struct RunOpts {
    pub cases: usize,
    pub seed: u64,
    /// Height of the rendered frames.
    pub height: u32,
    /// Also score every case after this degradation.
    pub degrade: Option<Degrade>,
    /// Draft a project from each (clean) analysis and check that it reproduces the analysis.
    pub roundtrip: bool,
    pub audio: bool,
    /// Where the report, the worst cases and the scratch media go.
    pub out: PathBuf,
    /// How many of the worst cases to save.
    pub worst: usize,
}

impl Default for RunOpts {
    fn default() -> Self {
        RunOpts { cases: 40, seed: 1, height: 360, degrade: None, roundtrip: true, audio: true, out: PathBuf::from("calib"), worst: 6 }
    }
}

/// A case on disk: what is needed to run it again, and what it was.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CaseFile {
    pub schema: String,
    pub spec: CaseSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degrade: Option<Degrade>,
    pub roundtrip: bool,
    pub badness: f64,
    pub why: Vec<String>,
    pub project: edits_core::Project,
    pub truth: recipe::Truth,
}

pub struct CaseOutcome {
    pub case: Case,
    /// (condition, scores): `clean`, then the degraded condition if any.
    pub results: Vec<(String, Scores)>,
    /// Analysis of the clean render.
    pub analysis: Analysis,
    /// Comparison of the draft's render with the clean analysis.
    pub roundtrip: Option<Comparison>,
}

pub fn source_of(frames: &Frames, name: &str, audio: bool) -> Source {
    Source {
        path: name.into(),
        width: frames.w as u32,
        height: frames.h as u32,
        fps: frames.fps,
        frames: frames.len(),
        duration_seconds: frames.len() as f64 / frames.fps,
        has_audio: audio,
        analysis_width: frames.w,
        analysis_height: frames.h,
    }
}

/// Analyse rendered frames the way `analyze` would analyse the video.
pub fn analyse(frames: &Frames, audio: Option<&[f32]>, name: &str) -> Analyzed {
    let opts = AnalyzeOpts { height: frames.h as u32, samples: true, whole: true, ..Default::default() };
    analyze_frames(frames, audio.map(|a| (a, 22050)), source_of(frames, name, audio.is_some()), &opts)
}

/// Draft a project from `first`, put the case's pictures into its slots,
/// render it and compare its analysis with `first`.
pub fn roundtrip(stage: &mut Stage, case: &Case, first: &Analyzed, dir: &Path) -> Result<Comparison> {
    let mut a = first.analysis.clone();
    if let Some(o) = a.layers.overlay.as_mut() {
        edits_analyze::img::save_rgb(&dir.join("plate.png"), first.plate.w, first.plate.h, &first.plate.rgb)?;
        o.path = Some("plate.png".into());
    }
    let opts = DraftOpts { proxies: false, width: None, height: None, snap: true, repeats: true, analysis_dir: dir.to_path_buf(), project_dir: dir.to_path_buf() };
    let mut project = draft(&a, &opts);
    project.audio = None;
    // The picture each slot showed in the original.
    for slot in &a.slots {
        let pic = slot.shots.first().and_then(|id| a.shots.get(id - 1)).and_then(|sh| case.truth.frame.get(sh.ref_frame).copied().flatten()).map(|ft| case.truth.shots[ft.shot].slot.clone());
        if let (Some(pic), Some(s)) = (pic, project.slots.get_mut(&slot.id)) {
            s.path = Some(format!("img/{pic}.png"));
        }
    }
    let frames = stage.render(&project, dir, 1.0, None)?;
    let opts = AnalyzeOpts {
        height: frames.h as u32,
        bpm: Some(a.beats.bpm.value),
        beat_offset: Some(a.beats.offset_seconds.value),
        samples: true,
        whole: false,
        range: Some((a.ingest.analysed_f0, a.ingest.analysed_f1)),
        ..Default::default()
    };
    let second = analyze_frames(&frames, None, source_of(&frames, "draft", false), &opts);
    Ok(compare_analyses(&a, &second.analysis, None))
}

fn roundtrip_scores(c: &Comparison) -> Scores {
    let mut s = Scores::default();
    s.samples.push(score::Sample { est: "draft.overall".into(), kind: "-".into(), truth: 1.0, got: c.overall, frame: 0 });
    for (k, v) in &c.scores {
        if let Some(v) = v {
            s.samples.push(score::Sample { est: format!("draft.{k}"), kind: "-".into(), truth: 1.0, got: *v, frame: 0 });
        }
    }
    s
}

/// Run one case in `dir` (scratch media is written there).
pub fn run_case(stage: &mut Stage, spec: &CaseSpec, dir: &Path, degraded: Option<&Degrade>, with_roundtrip: bool) -> Result<CaseOutcome> {
    let case = recipe::generate(spec);
    case.write_media(dir)?;
    let frames = stage.render(&case.project, dir, 1.0, None)?;
    let audio = if spec.audio { Some(case.click_track(22050)) } else { None };
    let first = analyse(&frames, audio.as_deref(), &format!("calib:{}", spec.seed));
    let mut clean = score::score(&case, &first.analysis);
    let mut rt = None;
    if with_roundtrip {
        let c = roundtrip(stage, &case, &first, dir)?;
        let extra = roundtrip_scores(&c);
        clean.samples.extend(extra.samples);
        rt = Some(c);
    }
    let mut results = vec![("clean".to_string(), clean)];
    if let Some(d) = degraded {
        let worse = degrade(&frames, d, &dir.join("degraded.mp4"))?;
        let a = analyse(&worse, audio.as_deref(), &format!("calib:{}:{}", spec.seed, d.label()));
        results.push((d.label(), score::score(&case, &a.analysis)));
    }
    Ok(CaseOutcome { case, results, analysis: first.analysis, roundtrip: rt })
}

fn case_file(o: &CaseOutcome, degraded: Option<&Degrade>, with_roundtrip: bool) -> CaseFile {
    let mut total = 0.0;
    let mut why = Vec::new();
    for (cond, sc) in &o.results {
        let (b, w) = report::badness(sc);
        total += b;
        why.extend(w.into_iter().map(|l| format!("[{cond}] {l}")));
    }
    CaseFile { schema: CASE_SCHEMA.into(), spec: o.case.spec.clone(), degrade: degraded.cloned(), roundtrip: with_roundtrip, badness: total, why, project: o.case.project.clone(), truth: o.case.truth.clone() }
}

/// Run `opts.cases` cases and write `report.json`, `report.txt` and the worst cases into `opts.out`.
pub fn run(opts: &RunOpts, mut progress: impl FnMut(&str)) -> Result<Report> {
    let started = std::time::Instant::now();
    std::fs::create_dir_all(&opts.out).with_context(|| format!("creating {}", opts.out.display()))?;
    let work = opts.out.join("work");
    let mut stage = Stage::new()?;
    let adapter = stage.renderer.info().name.clone();
    let mut all: Vec<(String, Scores)> = Vec::new();
    let mut files: Vec<CaseFile> = Vec::new();
    for i in 0..opts.cases {
        let spec = CaseSpec { seed: opts.seed + i as u64, height: opts.height, audio: opts.audio };
        let dir = work.join(format!("case_{}", spec.seed));
        let t = std::time::Instant::now();
        let outcome = run_case(&mut stage, &spec, &dir, opts.degrade.as_ref(), opts.roundtrip).with_context(|| format!("case with seed {}", spec.seed))?;
        let file = case_file(&outcome, opts.degrade.as_ref(), opts.roundtrip);
        progress(&format!("case {}/{} seed {} ({} frames, {} shots): badness {:.1} in {:.1} s", i + 1, opts.cases, spec.seed, outcome.case.truth.frames, outcome.case.truth.shots.len(), file.badness, t.elapsed().as_secs_f64()));
        files.push(file);
        all.extend(outcome.results);
        let _ = std::fs::remove_dir_all(&dir);
    }
    let _ = std::fs::remove_dir(&work);
    let (rows, events) = report::aggregate(&all);
    let mut conditions: Vec<String> = Vec::new();
    for (c, _) in &all {
        if !conditions.contains(c) {
            conditions.push(c.clone());
        }
    }
    // The worst cases, saved so they can be replayed.
    files.sort_by(|a, b| b.badness.total_cmp(&a.badness));
    let cases_dir = opts.out.join("cases");
    let _ = std::fs::remove_dir_all(&cases_dir);
    std::fs::create_dir_all(&cases_dir)?;
    let mut worst = Vec::new();
    for f in files.iter().take(opts.worst) {
        let path = cases_dir.join(format!("case_{}.json", f.spec.seed));
        std::fs::write(&path, serde_json::to_string_pretty(f)?)?;
        worst.push(WorstCase { seed: f.spec.seed, badness: f.badness, file: path.to_string_lossy().into_owned(), why: f.why.clone() });
    }
    let report = Report {
        schema: report::SCHEMA.into(),
        tool: format!("edits-calib {}", env!("CARGO_PKG_VERSION")),
        adapter,
        cases: opts.cases,
        seed: opts.seed,
        height: opts.height,
        conditions,
        rows,
        events,
        worst,
        frames_rendered: stage.rendered,
        seconds: (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
    };
    std::fs::write(opts.out.join("report.json"), serde_json::to_string_pretty(&report)?)?;
    std::fs::write(opts.out.join("report.txt"), report::table(&report))?;
    Ok(report)
}

pub struct Replay {
    pub file: CaseFile,
    pub outcome: CaseOutcome,
    pub report: Report,
}

/// Run a saved case again (it is regenerated from its seed) and describe it in detail.
pub fn replay(path: &Path, work: &Path) -> Result<Replay> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file: CaseFile = serde_json::from_str(&text).with_context(|| format!("{} is not a calibration case", path.display()))?;
    anyhow::ensure!(file.schema == CASE_SCHEMA, "{} is not an {CASE_SCHEMA} document", path.display());
    let started = std::time::Instant::now();
    let mut stage = Stage::new()?;
    let dir = work.join(format!("case_{}", file.spec.seed));
    let outcome = run_case(&mut stage, &file.spec, &dir, file.degrade.as_ref(), file.roundtrip)?;
    let _ = std::fs::remove_dir_all(&dir);
    let (rows, events) = report::aggregate(&outcome.results);
    let report = Report {
        schema: report::SCHEMA.into(),
        tool: format!("edits-calib {}", env!("CARGO_PKG_VERSION")),
        adapter: stage.renderer.info().name.clone(),
        cases: 1,
        seed: file.spec.seed,
        height: file.spec.height,
        conditions: outcome.results.iter().map(|r| r.0.clone()).collect(),
        rows,
        events,
        worst: vec![],
        frames_rendered: stage.rendered,
        seconds: (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
    };
    Ok(Replay { file, outcome, report })
}

/// The truth of a case next to what the analysis found, shot by shot.
pub fn describe(o: &CaseOutcome) -> String {
    let t = &o.case.truth;
    let mut s = format!(
        "truth: {} frames at {} fps, {} bpm, beat 0 at {:.3} s, {}x{}, vignette {:?}, overlay {}, shake {:?}\n",
        t.frames, t.fps, t.bpm, t.beat_offset, t.width, t.height, t.vignette, t.overlay, t.shake
    );
    for (i, sh) in t.shots.iter().enumerate() {
        let mut parts: Vec<String> = Vec::new();
        for f in t.flashes.iter().filter(|f| f.shot == i) {
            parts.push(format!("flash@{} {:.2}st (key {:.2}) rate {:.2} over {:.1}f", f.frame, f.visible, f.stops, f.rate, f.frames));
        }
        for h in t.hits.iter().filter(|h| h.shot == i) {
            parts.push(format!("{}@{}-{} {:.2}", h.kind, h.f0, h.f1, h.value));
        }
        for b in t.blurs.iter().filter(|b| b.shot == i) {
            parts.push(format!("blur@{} {:.2}% over {}f {}", b.frame, b.radius, b.frames, b.ease));
        }
        s += &format!("  true shot {:2} frames {:4}..{:4} {:8} {:12} {} | {}\n", i + 1, sh.f0, sh.f1, sh.kind.name(), sh.motion, sh.slot, parts.join(" "));
    }
    s += "analysis:\n";
    s += &edits_analyze::pipeline::summary(&o.analysis);
    if std::env::var_os("EDITS_CALIB_FRAMES").is_some() {
        s += "frame shot | truth: stops blur sat scale | measured (relative to the reference frame): stops blur sat scale\n";
        let sig = &o.analysis.signature;
        for sh in &o.analysis.shots {
            let rt = t.frame.get(sh.ref_frame).copied().flatten();
            for f in sh.f0..sh.f1 {
                let i = f - sig.f0;
                let rel = match (t.frame[f], rt) {
                    (Some(ft), Some(rt)) if ft.shot == rt.shot => score::relative(&ft, &rt),
                    _ => [f64::NAN; 7],
                };
                s += &format!(
                    "{f:5} {:3}{} | {:6.2} {:5.2} {:5.2} {:6.3} | {:6.2} {:5.2} {:5.2} {}\n",
                    sh.id,
                    if f == sh.ref_frame { "*" } else { " " },
                    rel[0],
                    rel[1],
                    rel[2],
                    rel[3],
                    sig.stops[i],
                    sig.blur[i],
                    sig.sat[i],
                    sig.scale[i].map(|v| format!("{v:6.3}")).unwrap_or("     -".into())
                );
            }
        }
    }
    if let Some(c) = &o.roundtrip {
        s += "draft round trip (render of the draft against the analysis):\n";
        s += &edits_analyze::compare::report(c, 12);
    }
    s
}
