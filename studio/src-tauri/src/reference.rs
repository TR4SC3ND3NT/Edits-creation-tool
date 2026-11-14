//! The Reference page: a reference analysis (`reference_analysis.json`,
//! written by `edits-analyze analyze`), compare reports, and running a
//! comparison of a render against the analysis.
//!
//! A comparison is what `edits-analyze compare --json` prints (the same
//! library call), saved under `<project dir>/.edits/compare/<stamp>.json`,
//! with the render's own signature curves next to it
//! (`<stamp>.signature.json`) so the page can draw reference and render
//! over time. A report written by the command line works too; it has no
//! render curves, and the page says so.

use edits_analyze::compare::{self, Comparison};
use edits_analyze::schema::Signature;
use edits_analyze::{beats, media, pipeline, AnalyzeOpts};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Folders not worth looking into for analyses.
const SKIP: [&str; 6] = [
    "node_modules",
    "target",
    ".git",
    "proxies",
    "renders",
    "img",
];

/// Every `reference_analysis.json` under `dir`, at most `depth` folders down.
pub fn find_analyses(dir: &Path, depth: usize) -> Vec<String> {
    let mut out = Vec::new();
    walk(dir, depth, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if depth > 0
                && !SKIP.contains(&name.as_str())
                && (!name.starts_with('.') || name == ".edits")
            {
                walk(&p, depth - 1, out);
            }
        } else if name == "reference_analysis.json" {
            out.push(p.display().to_string());
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct ShotDto {
    pub id: usize,
    pub f0: usize,
    pub f1: usize,
    pub beat0: f64,
    pub beat1: f64,
    pub status: String,
    pub events: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct AnalysisDto {
    pub path: String,
    pub video: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frames: usize,
    pub bpm: f64,
    /// Seconds from the start of the video to beat 0.
    pub offset_seconds: f64,
    pub shots: Vec<ShotDto>,
    pub signature: Signature,
    pub notes: Vec<String>,
}

pub fn load_analysis(path: &Path) -> Result<AnalysisDto, String> {
    let a = pipeline::load(path).map_err(|e| format!("cannot read {}: {e:#}", path.display()))?;
    Ok(AnalysisDto {
        path: path.display().to_string(),
        video: a.source.path.clone(),
        width: a.source.width,
        height: a.source.height,
        fps: a.source.fps,
        frames: a.source.frames,
        bpm: a.beats.bpm.value,
        offset_seconds: a.beats.offset_seconds.value,
        shots: a
            .shots
            .iter()
            .map(|s| ShotDto {
                id: s.id,
                f0: s.f0,
                f1: s.f1,
                beat0: s.beat0,
                beat1: s.beat1,
                status: serde_json::to_value(&s.status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default(),
                events: s.events.iter().map(|e| e.kind.clone()).collect(),
            })
            .collect(),
        signature: a.signature,
        notes: a.notes,
    })
}

/// `<project dir>/.edits/compare/`.
pub fn compare_dir(project: &Path) -> PathBuf {
    edits_core::edit::project_dir(project)
        .join(".edits")
        .join("compare")
}

#[derive(Serialize, Clone, Debug)]
pub struct ReportBrief {
    pub path: String,
    /// Milliseconds since 1970.
    pub time: f64,
    pub overall: f64,
    pub scores: Value,
    pub candidate: String,
    pub reference: String,
}

/// The saved comparisons of a project, oldest first.
pub fn list_reports(project: &Path) -> Vec<ReportBrief> {
    let Ok(entries) = std::fs::read_dir(compare_dir(project)) else {
        return Vec::new();
    };
    let mut out: Vec<ReportBrief> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_string();
            if !name.ends_with(".json") || name.ends_with(".signature.json") {
                return None;
            }
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
            let time = e
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_secs_f64()
                * 1000.0;
            Some(ReportBrief {
                path: p.display().to_string(),
                time,
                overall: v.get("overall")?.as_f64()?,
                scores: v.get("scores").cloned().unwrap_or(Value::Null),
                candidate: v
                    .get("candidate")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                reference: v
                    .get("reference")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}

#[derive(Serialize, Clone, Debug)]
pub struct Report {
    pub path: String,
    pub comparison: Value,
    /// The render's signature curves, when the studio made the report.
    pub candidate_signature: Option<Value>,
}

fn sidecar(report: &Path) -> PathBuf {
    report.with_extension("signature.json")
}

pub fn load_report(path: &Path) -> Result<Report, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let comparison: Value =
        serde_json::from_str(&text).map_err(|e| format!("{} is not JSON: {e}", path.display()))?;
    if comparison.get("overall").is_none() || comparison.get("scores").is_none() {
        return Err(format!("{} is not a compare report (write one with `edits-analyze compare <analysis> <video> --json`)", path.display()));
    }
    let candidate_signature = std::fs::read_to_string(sidecar(path))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    Ok(Report {
        path: path.display().to_string(),
        comparison,
        candidate_signature,
    })
}

/// Compare `video` with the analysis, as `edits-analyze compare` does, and
/// keep the render's analysis' signature too.
pub fn run_compare(analysis: &Path, video: &Path) -> Result<(Comparison, Signature), String> {
    let reference = pipeline::load(analysis)
        .map_err(|e| format!("cannot read {}: {e:#}", analysis.display()))?;
    let info = media::probe(video).map_err(|e| format!("{e:#}"))?;
    let ratio = info.fps / reference.source.fps;
    let map = |f: usize| (f as f64 * ratio).round() as usize;
    let opts = AnalyzeOpts {
        height: reference.source.analysis_height as u32,
        bpm: Some(reference.beats.bpm.value),
        beat_offset: Some(reference.beats.offset_seconds.value),
        samples: false,
        whole: false,
        range: Some((
            map(reference.ingest.analysed_f0),
            map(reference.ingest.analysed_f1),
        )),
    };
    let cand = pipeline::analyze_video(video, None, &opts).map_err(|e| format!("{e:#}"))?;
    let tempo = media::decode_audio(video, &info, 22050)
        .map_err(|e| format!("{e:#}"))?
        .and_then(|a| beats::tempo(&beats::onset_envelope(&a, 22050), &Default::default()));
    Ok((
        compare::compare_analyses(&reference, &cand, tempo.as_ref()),
        cand.signature,
    ))
}

/// Save a comparison (and the render's signature) as a new report of the project.
pub fn save_report(project: &Path, c: &Comparison, sig: &Signature) -> Result<PathBuf, String> {
    let dir = compare_dir(project);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let stamp = edits_core::edit::utc_stamp(SystemTime::now());
    let path = dir.join(format!("{stamp}.json"));
    let text = edits_analyze::jsonfmt::to_string(c).map_err(|e| format!("{e:#}"))?;
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let sig_text = serde_json::to_string(sig).map_err(|e| e.to_string())?;
    std::fs::write(sidecar(&path), sig_text)
        .map_err(|e| format!("cannot write {}: {e}", sidecar(&path).display()))?;
    Ok(path)
}

/// A comparison running in the background.
#[derive(Serialize, Clone, Debug, Default)]
pub struct CompareRun {
    pub running: bool,
    pub video: Option<String>,
    pub error: Option<String>,
    pub report: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyses_are_found_below_the_project_and_reports_are_listed() {
        let dir = std::env::temp_dir().join(format!("edits-studio-ref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("analysis/proxies")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        std::fs::write(dir.join("analysis/reference_analysis.json"), "{}").unwrap();
        std::fs::write(dir.join("node_modules/x/reference_analysis.json"), "{}").unwrap();
        let found = find_analyses(&dir, 3);
        assert_eq!(
            found,
            [dir.join("analysis/reference_analysis.json")
                .display()
                .to_string()]
        );

        let project = dir.join("project.json");
        std::fs::write(&project, "{}").unwrap();
        assert!(list_reports(&project).is_empty());
        let cd = compare_dir(&project);
        std::fs::create_dir_all(&cd).unwrap();
        std::fs::write(cd.join("a.json"), r#"{"overall": 0.5, "scores": {"cuts": 1.0}, "candidate": "r.mp4", "reference": "ref.mp4"}"#).unwrap();
        std::fs::write(
            cd.join("a.signature.json"),
            r#"{"f0": 0, "lum": [], "stops": [], "blur": [], "sat": [], "scale": []}"#,
        )
        .unwrap();
        std::fs::write(cd.join("junk.json"), "[]").unwrap();
        let list = list_reports(&project);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].overall, 0.5);
        let r = load_report(Path::new(&list[0].path)).unwrap();
        assert!(r.candidate_signature.is_some());
        assert!(load_report(&cd.join("junk.json")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
