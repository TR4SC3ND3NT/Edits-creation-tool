//! The trace commands: `edits track` (planar tracking of known pictures in
//! a reference), `edits measure grade|blur` (tone curves and blur per frame
//! as effect parameters) and `edits metric` (per-frame SSIM against the
//! reference, worst-frame sheets, the author-pixel audit). See the "Trace"
//! section of docs/ANALYSIS.md.

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use edits_analyze::{contact, media, planar, trace};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

#[derive(Args, Clone)]
pub struct TrackArgs {
    /// The reference video
    pub video: PathBuf,
    /// A picture to find in it (PNG or JPEG); repeat for several. Its name is the file stem.
    #[arg(long, required = true)]
    pub art: Vec<PathBuf>,
    /// Frames to look in, per picture: NAME=FIRST-LAST[,FIRST-LAST...] (inclusive); repeat per picture. Default: every frame
    #[arg(long, value_name = "NAME=RANGES")]
    pub window: Vec<String>,
    /// Correlation (0..1) a frame needs to be trusted
    #[arg(long, default_value_t = 0.5)]
    pub min_ncc: f64,
    /// Carry the corners on at their last speed for up to N frames before the first and after the last trusted key of a window (blurred entries and exits); 0 = off. Check the result by eye: an abrupt move (a collapse, a cut) is not a constant speed
    #[arg(long, default_value_t = 0)]
    pub extend: usize,
    /// Longest run of untrusted frames to fill by interpolating the corners
    #[arg(long, default_value_t = 8)]
    pub max_gap: usize,
    /// Savitzky-Golay window for the corners (odd; 0 = none)
    #[arg(long, default_value_t = 5)]
    pub smooth: usize,
    /// Frame pixels to leave out of the refinement: dark, red (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub ignore: Vec<String>,
    /// Keypoints per frame and per picture
    #[arg(long, default_value_t = 1500)]
    pub features: usize,
    /// Write the track here (JSON); without it the track goes to stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Also write a sheet of frames with the tracked quads drawn on them (.png/.jpg)
    #[arg(long, value_name = "IMAGE")]
    pub sheet: Option<PathBuf>,
    /// Print a JSON summary (with --output)
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand, Clone)]
pub enum MeasureCommand {
    /// Tone curves (red, green, blue) that turn each tracked picture into its look in the reference, per key, as `curves` effect keys
    Grade(GradeArgs),
    /// Blur per frame (gaussian, zoom, directional) that brings a render closest to the reference, as effect keys
    Blur(BlurArgs),
}

#[derive(Args, Clone)]
pub struct GradeArgs {
    /// The reference video
    pub video: PathBuf,
    /// The track written by `edits track`
    #[arg(long)]
    pub track: PathBuf,
    /// Points per curve
    #[arg(long, default_value_t = 9)]
    pub points: usize,
    /// Measure every Nth key only (the rest are interpolated by the renderer)
    #[arg(long, default_value_t = 1)]
    pub every: usize,
    /// Write the result here (JSON); without it it goes to stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct BlurArgs {
    /// Our render (any size; compared at the reference's size)
    pub render: PathBuf,
    /// The reference video
    pub reference: PathBuf,
    /// Only these frames, e.g. 100-140,200 (default: every frame)
    #[arg(long)]
    pub frames: Option<String>,
    /// Report only frames where blur raises SSIM by at least this much
    #[arg(long, default_value_t = 0.01)]
    pub min_gain: f64,
    /// Write the result here (JSON); without it it goes to stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct MetricArgs {
    /// Our render
    pub render: PathBuf,
    /// The reference video
    pub reference: PathBuf,
    /// Gaussian blur (pixels at the reference size) applied to both before SSIM: compares placement and shape, not fine texture
    #[arg(long, default_value_t = 2.0)]
    pub pre_blur: f64,
    /// How many worst frames to list (and put on the sheet)
    #[arg(long, default_value_t = 12)]
    pub worst: usize,
    /// Write a sheet of the worst frames: reference | ours | difference (.png/.jpg)
    #[arg(long, value_name = "IMAGE")]
    pub sheet: Option<PathBuf>,
    /// Named frame ranges to report SSIM for, NAME=FIRST-LAST (repeatable)
    #[arg(long, value_name = "NAME=RANGE")]
    pub block: Vec<String>,
    /// Write the per-frame numbers here (JSON)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// The project the render came from: the worst frames then list the layers visible at them
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// Also write a side-by-side video here: reference | render, with the reference's sound
    #[arg(long, value_name = "MP4")]
    pub side_by_side: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

/// `a-b,c,d-e` -> inclusive ranges.
pub(crate) fn parse_ranges(s: &str) -> Result<Vec<(usize, usize)>> {
    let mut v = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>()?, b.trim().parse::<usize>()?),
            None => {
                let a = part.parse::<usize>()?;
                (a, a)
            }
        };
        if b < a {
            bail!("range {part} ends before it starts");
        }
        v.push((a, b));
    }
    Ok(v)
}

fn write_or_print(out: Option<&Path>, text: &str) -> Result<()> {
    match out {
        Some(p) => {
            if let Some(d) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(p, text).with_context(|| format!("writing {}", p.display()))
        }
        None => {
            println!("{text}");
            Ok(())
        }
    }
}

pub fn track(a: TrackArgs) -> Result<ExitCode> {
    let t0 = Instant::now();
    let mut windows: std::collections::BTreeMap<String, Vec<(usize, usize)>> = Default::default();
    for w in &a.window {
        let (name, ranges) = w.split_once('=').with_context(|| format!("--window {w}: write NAME=FIRST-LAST, e.g. photo=92-121"))?;
        windows.insert(name.trim().to_string(), parse_ranges(ranges).with_context(|| format!("--window {w}"))?);
    }
    let mut arts = Vec::new();
    for p in &a.art {
        let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "art".into());
        let win = windows.remove(&name).unwrap_or_default();
        arts.push(planar::load_art(p, &name, win)?);
    }
    if let Some(name) = windows.keys().next() {
        bail!("--window names \"{name}\", which is not the file stem of any --art");
    }
    let mut opts = planar::TrackOpts { min_ncc: a.min_ncc, max_gap: a.max_gap, smooth: a.smooth, features: a.features, extend: a.extend, ..Default::default() };
    for i in &a.ignore {
        match i.as_str() {
            "dark" => opts.ignore_dark = true,
            "red" => opts.ignore_red = true,
            other => bail!("--ignore {other}: use dark and/or red"),
        }
    }
    let info = media::probe(&a.video)?;
    eprintln!("decoding {} ({}x{}, {} fps)", a.video.display(), info.width, info.height, info.fps);
    let frames = media::decode_frames(&a.video, &info, info.height)?;
    let last = std::sync::Mutex::new(String::new());
    let progress = |name: &str, f: usize| {
        if f.is_multiple_of(24) {
            let mut l = last.lock().unwrap();
            let s = format!("{name} {f}");
            if *l != s {
                eprintln!("tracking {s}/{}", frames.len());
                *l = s;
            }
        }
    };
    let tr = planar::track(&frames, &a.video.display().to_string(), &arts, &opts, &progress);
    let text = edits_analyze::jsonfmt::to_string(&tr)?;
    if let Some(sheet) = &a.sheet {
        let mut keyed: Vec<usize> = tr.arts.iter().flat_map(|x| x.keys.iter().map(|k| k.frame)).collect();
        keyed.sort();
        keyed.dedup();
        let step = keyed.len().div_ceil(24).max(1);
        let cells: Vec<contact::Cell> = keyed
            .iter()
            .step_by(step)
            .map(|&f| {
                let rgb = planar::overlay(&frames, f, &tr);
                let img = image::RgbImage::from_raw(frames.w as u32, frames.h as u32, rgb).unwrap();
                let names: Vec<String> = tr.arts.iter().filter_map(|x| x.keys.iter().find(|k| k.frame == f).map(|k| format!("{} {:.2}{}", x.name, k.ncc, if k.method == "filled" { "*" } else { "" }))).collect();
                contact::Cell { image: contact::fit_height(&img, 240), label: format!("f{f} {}", names.join(" ")), accent: None }
            })
            .collect();
        contact::save(&contact::compose(&cells, 6, 6, Some("edits track: quads per frame (red = filled)")), sheet)?;
    }
    let summary: Vec<_> = tr
        .arts
        .iter()
        .map(|x| {
            let found = x.keys.iter().filter(|k| !matches!(k.method.as_str(), "filled" | "extended")).count();
            let filled = x.keys.len() - found;
            let mean = if found > 0 { x.keys.iter().filter(|k| !matches!(k.method.as_str(), "filled" | "extended")).map(|k| k.ncc).sum::<f64>() / found as f64 } else { 0.0 };
            json!({ "art": x.name, "keys": x.keys.len(), "found": found, "filled": filled, "missing": x.missing.len(), "mean_ncc": (mean * 1000.0).round() / 1000.0 })
        })
        .collect();
    match &a.output {
        Some(p) => {
            write_or_print(Some(p), &text)?;
            if a.json {
                println!("{}", serde_json::to_string_pretty(&json!({ "output": p, "sheet": a.sheet, "arts": summary, "seconds": t0.elapsed().as_secs_f64().round() }))?);
            } else {
                for s in &summary {
                    println!("{}: {} keys ({} found, {} filled), {} missing, mean ncc {}", s["art"].as_str().unwrap_or(""), s["keys"], s["found"], s["filled"], s["missing"], s["mean_ncc"]);
                }
                println!("wrote {}", p.display());
            }
        }
        None => println!("{text}"),
    }
    Ok(ExitCode::SUCCESS)
}

pub fn measure(c: MeasureCommand) -> Result<ExitCode> {
    match c {
        MeasureCommand::Grade(a) => {
            let tr: planar::Track = serde_json::from_str(&std::fs::read_to_string(&a.track).with_context(|| format!("reading {}", a.track.display()))?).context("not an edits.track/1 file: write one with `edits track`")?;
            let g = trace::measure_grade(&a.video, &tr, a.points, a.every.max(1), &[])?;
            write_or_print(a.output.as_deref(), &edits_analyze::jsonfmt::to_string(&g)?)?;
            if let Some(p) = &a.output {
                eprintln!("wrote {}", p.display());
            }
        }
        MeasureCommand::Blur(a) => {
            let frames = match &a.frames {
                Some(s) => Some(parse_ranges(s)?.into_iter().flat_map(|(x, y)| x..=y).collect::<Vec<_>>()),
                None => None,
            };
            let b = trace::measure_blur(&a.render, &a.reference, frames.as_deref(), a.min_gain)?;
            write_or_print(a.output.as_deref(), &edits_analyze::jsonfmt::to_string(&b)?)?;
            if let Some(p) = &a.output {
                eprintln!("wrote {}", p.display());
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub fn metric(a: MetricArgs) -> Result<ExitCode> {
    let mut blocks = Vec::new();
    for b in &a.block {
        let (name, r) = b.split_once('=').with_context(|| format!("--block {b}: write NAME=FIRST-LAST"))?;
        for (s, e) in parse_ranges(r)? {
            blocks.push((name.to_string(), s, e));
        }
    }
    let m = trace::metric(&a.render, &a.reference, &trace::MetricOpts { pre_blur: a.pre_blur, worst: a.worst, blocks, sheet: a.sheet.clone() })?;
    if let Some(p) = &a.output {
        write_or_print(Some(p), &edits_analyze::jsonfmt::to_string(&m)?)?;
    }
    let mut small = serde_json::to_value(&m)?;
    if let Some(o) = small.as_object_mut() {
        o.remove("frames");
    }
    if let Some(pp) = &a.project {
        let project = edits_core::Project::from_json(&std::fs::read_to_string(pp).with_context(|| format!("reading {}", pp.display()))?)?;
        let ev = edits_core::Evaluator::new(&project);
        if let Some(w) = small["worst"].as_array_mut() {
            for f in w.iter_mut() {
                let frame = f["frame"].as_u64().unwrap_or(0);
                // Reference frame n shows the instant n / fps.
                let t = project.from_seconds(frame as f64 / m.fps);
                let ids: Vec<&str> = project.layers.iter().filter(|l| l.enabled && ev.active(l, t)).map(|l| l.id.as_str()).collect();
                f["layers"] = json!(ids);
            }
        }
    }
    if let Some(out) = &a.side_by_side {
        crate::tracecmd::side_by_side(&a.reference, &a.render, out)?;
        small["side_by_side"] = json!(out);
    }
    if a.json {
        println!("{}", serde_json::to_string_pretty(&small)?);
    } else {
        print!("{}", trace::metric_report(&m));
        if let Some(w) = small["worst"].as_array().filter(|_| a.project.is_some()) {
            for f in w.iter().take(6) {
                println!("  f{} layers: {}", f["frame"], f["layers"]);
            }
        }
        if let Some(out) = &a.side_by_side {
            println!("side by side: {}", out.display());
        }
    }
    Ok(ExitCode::SUCCESS)
}
