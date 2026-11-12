use anyhow::Result;
use clap::{Parser, Subcommand};
use edits_analyze::{compare, draft, media, pipeline, signals, AnalyzeOpts};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "edits-analyze", version, about = "Extract the recipe of a finished edit, draft a project from it, compare renders in signature space")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args, Clone)]
struct Common {
    /// Height in pixels of the frames the analysis works on
    #[arg(long, default_value_t = 360)]
    height: u32,
    /// Use this tempo instead of estimating it
    #[arg(long)]
    bpm: Option<f64>,
    /// Seconds from the start of the video to beat 0
    #[arg(long)]
    beat_offset: Option<f64>,
    /// Analyse the whole clip even if it loops
    #[arg(long)]
    whole: bool,
}

impl Common {
    fn opts(&self, samples: bool) -> AnalyzeOpts {
        AnalyzeOpts { height: self.height, bpm: self.bpm, beat_offset: self.beat_offset, samples, whole: self.whole, range: None }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Analyse a reference video; writes reference_analysis.json, overlay.png and proxies/ into the output directory
    Analyze {
        video: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Keep raw per-frame track samples in the JSON
        #[arg(long)]
        samples: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Write an edits.project/1 project that replays the analysed recipe
    Draft {
        /// reference_analysis.json written by `analyze`
        analysis: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Point the slots at the proxy stills so the draft renders immediately
        #[arg(long)]
        proxies: bool,
        #[arg(long)]
        width: Option<u32>,
        #[arg(long)]
        height: Option<u32>,
        /// Keep exact frame times instead of snapping to the quarter-beat grid
        #[arg(long)]
        no_snap: bool,
        /// Write only the first period of a looping edit
        #[arg(long)]
        single_loop: bool,
    },
    /// Analyse a second video the same way and compare it with a reference analysis in signature space
    Compare {
        /// reference_analysis.json written by `analyze`
        analysis: PathBuf,
        /// The video to compare (typically a render of the drafted project)
        video: PathBuf,
        #[arg(long)]
        json: bool,
        /// How many differences to list in the text report
        #[arg(long, default_value_t = 25)]
        top: usize,
    },
    /// Cheap per-frame signature table: luminance, saturation, sharpness, frame difference
    Probe {
        video: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 360)]
        height: u32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Analyze { video, out, samples, common } => {
            let t = std::time::Instant::now();
            let a = pipeline::analyze_video(&video, Some(&out), &common.opts(samples))?;
            println!("{}", pipeline::summary(&a));
            println!("wrote {} in {:.1} s", out.join("reference_analysis.json").display(), t.elapsed().as_secs_f64());
        }
        Cmd::Draft { analysis, out, proxies, width, height, no_snap, single_loop } => {
            let a = pipeline::load(&analysis)?;
            let dir = |p: &PathBuf| p.parent().filter(|d| !d.as_os_str().is_empty()).map(|d| d.to_path_buf()).unwrap_or(PathBuf::from("."));
            let opts = draft::DraftOpts { proxies, width, height, snap: !no_snap, repeats: !single_loop, analysis_dir: dir(&analysis), project_dir: dir(&out) };
            let project = draft::draft(&a, &opts);
            std::fs::create_dir_all(&opts.project_dir)?;
            std::fs::write(&out, project.to_json())?;
            let findings = edits_core::check::check(&project, Some(&opts.project_dir));
            let errors = findings.iter().filter(|f| f.severity == edits_core::check::Severity::Error).count();
            for f in findings.iter().filter(|f| f.severity == edits_core::check::Severity::Error) {
                eprintln!("error: {}: {}", f.path, f.message);
            }
            println!(
                "wrote {}: {} layers, {} slots, {} markers, {:.2} beats at {} bpm; check: {} errors, {} warnings",
                out.display(),
                project.layers.len(),
                project.slots.len(),
                project.markers.len(),
                project.settings.duration,
                project.settings.bpm.unwrap_or(0.0),
                errors,
                findings.len() - errors
            );
            anyhow::ensure!(errors == 0, "the drafted project does not pass `check`");
        }
        Cmd::Compare { analysis, video, json, top } => {
            let a = pipeline::load(&analysis)?;
            let c = compare::compare(&a, &video, None)?;
            if json {
                println!("{}", edits_analyze::jsonfmt::to_string(&c)?);
            } else {
                print!("{}", compare::report(&c, top));
            }
        }
        Cmd::Probe { video, json, height } => {
            let info = media::probe(&video)?;
            let frames = media::decode_frames(&video, &info, height)?;
            let sigs = signals::signatures(&frames);
            if json {
                println!("{}", edits_analyze::jsonfmt::to_string(&serde_json::json!({ "video": info, "frames": sigs }))?);
            } else {
                println!("# {} {}x{} {} fps {} frames", info.path, info.width, info.height, info.fps, frames.len());
                println!("frame    lum  lum_c  contrast    sat  sharp    diff");
                for s in &sigs {
                    println!("{:5} {:6.1} {:6.1} {:9.1} {:6.1} {:6.3} {:7.2}", s.frame, s.lum, s.lum_center, s.contrast, s.sat, s.sharp, s.diff);
                }
            }
        }
    }
    Ok(())
}
