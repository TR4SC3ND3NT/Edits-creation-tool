use anyhow::Result;
use clap::{Parser, Subcommand};
use edits_analyze::{media, pipeline, signals, AnalyzeOpts};
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
        AnalyzeOpts { height: self.height, bpm: self.bpm, beat_offset: self.beat_offset, samples, whole: self.whole }
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
