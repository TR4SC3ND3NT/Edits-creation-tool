//! `edits analyze`, `draft`, `compare` and `probe`: the reference-analysis
//! commands, calling the `edits-analyze` library. The standalone
//! `edits-analyze` binary offers the same commands.

use anyhow::{ensure, Result};
use clap::{Args, Subcommand};
use edits_analyze::{compare, draft, media, pipeline, signals, AnalyzeOpts};
use edits_core::check::{self, Severity};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

#[derive(Args, Clone)]
pub struct AnalyzeArgs {
    /// The reference video
    pub video: PathBuf,
    /// Folder for reference_analysis.json, overlay.png and proxies/
    #[arg(short, long)]
    pub out: PathBuf,
    /// Height in pixels of the frames the analysis works on
    #[arg(long, default_value_t = 360)]
    pub height: u32,
    /// Use this tempo instead of estimating it
    #[arg(long)]
    pub bpm: Option<f64>,
    /// Seconds from the start of the video to beat 0
    #[arg(long, allow_hyphen_values = true)]
    pub beat_offset: Option<f64>,
    /// Analyse the whole clip even if it loops
    #[arg(long)]
    pub whole: bool,
    /// Keep raw per-frame track samples in the JSON
    #[arg(long)]
    pub samples: bool,
    /// Print a JSON summary instead of the text one
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Clone)]
pub struct DraftArgs {
    /// reference_analysis.json written by `edits analyze`
    pub analysis: PathBuf,
    /// Project file to write
    #[arg(short, long)]
    pub out: PathBuf,
    /// Point the slots at the reference's own stills so the draft renders immediately
    #[arg(long)]
    pub proxies: bool,
    #[arg(long)]
    pub width: Option<u32>,
    #[arg(long)]
    pub height: Option<u32>,
    /// Keep exact frame times instead of snapping to the quarter-beat grid
    #[arg(long)]
    pub no_snap: bool,
    /// Write only the first period of a looping edit
    #[arg(long)]
    pub single_loop: bool,
    /// Print a JSON summary
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Clone)]
pub struct CompareArgs {
    /// reference_analysis.json written by `edits analyze`
    pub analysis: PathBuf,
    /// The video to compare, typically a render of the drafted project (any pictures)
    pub video: PathBuf,
    #[arg(long)]
    pub json: bool,
    /// How many differences to list in the text report
    #[arg(long, default_value_t = 25)]
    pub top: usize,
}

#[derive(Args, Clone)]
pub struct ProbeArgs {
    pub video: PathBuf,
    #[arg(long)]
    pub json: bool,
    /// Height in pixels the frames are measured at
    #[arg(long, default_value_t = 360)]
    pub height: u32,
}

#[derive(Subcommand, Clone)]
pub enum AnalysisCommand {
    /// Extract the recipe of a finished edit: tempo, cuts, flashes, colour hits, blur, motion, overlays
    Analyze(AnalyzeArgs),
    /// Write a project that replays an analysed recipe, with one empty slot per picture
    Draft(DraftArgs),
    /// Compare a video with a reference analysis in signature space (different pictures are fine)
    Compare(CompareArgs),
    /// Cheap per-frame signatures of a video: luminance, saturation, sharpness, frame difference
    Probe(ProbeArgs),
}

pub fn run(cmd: AnalysisCommand) -> Result<ExitCode> {
    match cmd {
        AnalysisCommand::Analyze(a) => analyze(a),
        AnalysisCommand::Draft(a) => draft_project(a),
        AnalysisCommand::Compare(a) => compare_video(a),
        AnalysisCommand::Probe(a) => probe(a),
    }
}

fn analyze(a: AnalyzeArgs) -> Result<ExitCode> {
    let t = Instant::now();
    let opts = AnalyzeOpts { height: a.height, bpm: a.bpm, beat_offset: a.beat_offset, samples: a.samples, whole: a.whole, range: None };
    let an = pipeline::analyze_video(&a.video, Some(&a.out), &opts)?;
    let path = a.out.join("reference_analysis.json");
    let seconds = t.elapsed().as_secs_f64();
    if a.json {
        let needs: Vec<usize> = an.shots.iter().filter(|s| matches!(s.status, edits_analyze::schema::Ident::NeedsAgent)).map(|s| s.id).collect();
        crate::print_json(&json!({
            "analysis": path,
            "bpm": an.beats.bpm.value,
            "beat_offset_seconds": an.beats.offset_seconds.value,
            "shots": an.shots.len(),
            "slots": an.slots.len(),
            "needs_agent": needs,
            "seconds": (seconds * 10.0).round() / 10.0,
        }));
    } else {
        println!("{}", pipeline::summary(&an));
        println!("wrote {} in {:.1} s", path.display(), seconds);
    }
    Ok(ExitCode::SUCCESS)
}

fn parent_dir(p: &Path) -> PathBuf {
    p.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

fn draft_project(a: DraftArgs) -> Result<ExitCode> {
    let an = pipeline::load(&a.analysis)?;
    let opts = draft::DraftOpts {
        proxies: a.proxies,
        width: a.width,
        height: a.height,
        snap: !a.no_snap,
        repeats: !a.single_loop,
        analysis_dir: parent_dir(&a.analysis),
        project_dir: parent_dir(&a.out),
    };
    let project = draft::draft(&an, &opts);
    std::fs::create_dir_all(&opts.project_dir)?;
    std::fs::write(&a.out, project.to_json())?;
    let findings = check::check(&project, Some(&opts.project_dir));
    let errors = findings.iter().filter(|f| f.severity == Severity::Error).count();
    for f in findings.iter().filter(|f| f.severity == Severity::Error) {
        eprintln!("error: {}: {}", f.path, f.message);
    }
    if a.json {
        crate::print_json(&json!({
            "project": a.out,
            "layers": project.layers.len(),
            "slots": project.slots.len(),
            "markers": project.markers.len(),
            "beats": project.settings.duration,
            "bpm": project.settings.bpm,
            "errors": errors,
            "warnings": findings.len() - errors,
        }));
    } else {
        println!(
            "wrote {}: {} layers, {} slots, {} markers, {:.2} beats at {} bpm; check: {} errors, {} warnings",
            a.out.display(),
            project.layers.len(),
            project.slots.len(),
            project.markers.len(),
            project.settings.duration,
            project.settings.bpm.unwrap_or(0.0),
            errors,
            findings.len() - errors
        );
    }
    ensure!(errors == 0, "the drafted project does not pass `check`");
    Ok(ExitCode::SUCCESS)
}

fn compare_video(a: CompareArgs) -> Result<ExitCode> {
    let an = pipeline::load(&a.analysis)?;
    let c = compare::compare(&an, &a.video, None)?;
    if a.json {
        println!("{}", edits_analyze::jsonfmt::to_string(&c)?);
    } else {
        print!("{}", compare::report(&c, a.top));
    }
    Ok(ExitCode::SUCCESS)
}

fn probe(a: ProbeArgs) -> Result<ExitCode> {
    let info = media::probe(&a.video)?;
    let frames = media::decode_frames(&a.video, &info, a.height)?;
    let sigs = signals::signatures(&frames);
    if a.json {
        println!("{}", edits_analyze::jsonfmt::to_string(&json!({ "video": info, "frames": sigs }))?);
    } else {
        println!("# {} {}x{} {} fps {} frames", info.path, info.width, info.height, info.fps, frames.len());
        println!("frame    lum  lum_c  contrast    sat  sharp    diff");
        for s in &sigs {
            println!("{:5} {:6.1} {:6.1} {:9.1} {:6.1} {:6.3} {:7.2}", s.frame, s.lum, s.lum_center, s.contrast, s.sat, s.sharp, s.diff);
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Args, Clone)]
pub struct FitArgs {
    /// Project to improve, usually drafted from the same analysis (any pictures in its slots)
    pub project: PathBuf,
    /// reference_analysis.json written by `edits analyze`
    #[arg(long, short)]
    pub reference: PathBuf,
    /// Only these reference shots, e.g. 3,7 (default: every shot)
    #[arg(long, value_delimiter = ',')]
    pub shots: Vec<usize>,
    /// Parameter groups to fit: auto, or any of exposure, exposure_decay, saturation, threshold, blur, scale, scale_decay, position, position_decay
    #[arg(long, value_delimiter = ',', default_value = "auto")]
    pub params: Vec<String>,
    /// Renders allowed per shot
    #[arg(long, default_value_t = 24)]
    pub budget: usize,
    /// Render scale while fitting (default: the reference's analysis height)
    #[arg(long)]
    pub scale: Option<f32>,
    /// Report what would change; write nothing
    #[arg(long)]
    pub dry_run: bool,
    /// Seed for the order in which parameters are tried
    #[arg(long, default_value_t = 1)]
    pub seed: u64,
    /// Print the full report as JSON
    #[arg(long)]
    pub json: bool,
}

pub fn fit(a: FitArgs) -> Result<ExitCode> {
    let opts = edits_calib::fit::FitOpts {
        shots: a.shots,
        params: a.params,
        budget: a.budget,
        scale: a.scale,
        dry_run: a.dry_run,
        seed: a.seed,
        actor: edits_core::edit::AGENT.to_string(),
    };
    eprintln!("fitting {} to {} (this renders each shot up to {} times)...", a.project.display(), a.reference.display(), opts.budget);
    let report = edits_calib::fit::fit_file(&a.project, &a.reference, &opts)?;
    if a.json {
        println!("{}", edits_analyze::jsonfmt::to_string(&report)?);
    } else {
        print!("{}", edits_calib::fit::summary(&report));
    }
    Ok(ExitCode::SUCCESS)
}
