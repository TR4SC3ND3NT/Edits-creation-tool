//! `edits trace`: the one-command start of the trace method. Track the
//! pictures (and proxies) in a reference, measure their grade, find the beat
//! grid, write a draft project that replays the trace, and optionally
//! render it, compare it and write a side-by-side. See docs/ANALYSIS.md.

use crate::trace::parse_ranges;
use anyhow::{bail, Context, Result};
use clap::Args;
use edits_analyze::{contact, media, planar, trace};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

#[derive(Args, Clone)]
pub struct TraceArgs {
    /// The reference video
    pub video: PathBuf,
    /// Pictures that appear in the reference: files (PNG/JPEG) or folders of them
    #[arg(long, num_args = 1..)]
    pub arts: Vec<PathBuf>,
    /// The project to write; the track, grade and preview go into a `trace/` folder next to it
    #[arg(short, long)]
    pub output: PathBuf,
    /// Frames to look in per picture, NAME=FIRST-LAST[,...] (default: found by a quick pass over the video)
    #[arg(long, value_name = "NAME=RANGES")]
    pub window: Vec<String>,
    /// Track a plane of the reference that is not among the arts: NAME=FRAME:X,Y,W,H takes that rectangle of that frame (pixels) as a measurement template. The template is never rendered; `--fill` says what to show in its place
    #[arg(long, value_name = "NAME=FRAME:X,Y,W,H")]
    pub proxy: Vec<String>,
    /// The picture to show where a proxy was tracked, NAME=FILE: cropped (centred) to the proxy's shape and pinned to its corners
    #[arg(long, value_name = "NAME=FILE")]
    pub fill: Vec<String>,
    /// Correlation (0..1) a frame needs to be trusted
    #[arg(long, default_value_t = 0.5)]
    pub min_ncc: f64,
    /// Frame pixels to leave out of the refinement: dark, red
    #[arg(long, value_delimiter = ',')]
    pub ignore: Vec<String>,
    /// Width of the project (default: the reference's)
    #[arg(long)]
    pub width: Option<u32>,
    /// Height of the project (default: the reference's)
    #[arg(long)]
    pub height: Option<u32>,
    /// Leave the reference's sound out of the project
    #[arg(long)]
    pub no_audio: bool,
    /// Also render the draft, compare it (`edits metric`) and write a worst-frames sheet and a side-by-side video into trace/
    #[arg(long)]
    pub preview: bool,
    #[arg(long)]
    pub json: bool,
}

/// reference | render: the render scaled to the reference's size and frame
/// rate, with the reference's sound.
pub fn side_by_side(reference: &Path, render: &Path, out: &Path) -> Result<()> {
    let info = media::probe(reference)?;
    let ff = crate::encode::probe();
    let enc = ff.h264_encoder()?.to_string();
    if let Some(d) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    let filter = format!("[1:v]scale={}:{}:flags=area,fps={},setsar=1[o];[0:v]setsar=1[r];[r][o]hstack=inputs=2,format=yuv420p[v]", info.width, info.height, info.fps);
    let mut cmd = std::process::Command::new(crate::encode::program());
    cmd.args(["-v", "error", "-y", "-i"]).arg(reference).arg("-i").arg(render).args(["-filter_complex", &filter, "-map", "[v]", "-map", "0:a?", "-c:v", &enc]);
    if let Some(aac) = ff.aac.first() {
        cmd.args(["-c:a", aac]);
    } else {
        cmd.arg("-an");
    }
    let status = cmd.arg("-shortest").arg(out).status().context("running ffmpeg")?;
    if !status.success() {
        bail!("ffmpeg could not write {}", out.display());
    }
    Ok(())
}

fn image_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut inside: Vec<PathBuf> = std::fs::read_dir(p)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|q| q.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg")))
                .collect();
            inside.sort();
            v.extend(inside);
        } else {
            v.push(p.clone());
        }
    }
    Ok(v)
}

/// `path` relative to `base` when it is inside it, else absolute.
fn relative(path: &Path, base: &Path) -> String {
    let abs = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (p, b) = (abs(path), abs(base));
    match p.strip_prefix(&b) {
        Ok(r) => r.display().to_string(),
        Err(_) => p.display().to_string(),
    }
}

/// Crop `file` (centred) to `pw` x `ph`'s shape and save it. Returns the path and its size.
fn crop_fill(file: &Path, pw: usize, ph: usize, out: &Path) -> Result<(usize, usize)> {
    let img = image::open(file).with_context(|| format!("reading {}", file.display()))?.to_rgb8();
    let (fw, fh) = (img.width() as f64, img.height() as f64);
    let aspect = pw as f64 / ph as f64;
    let (cw, ch) = if fw / fh > aspect { (fh * aspect, fh) } else { (fw, fw / aspect) };
    let crop = image::imageops::crop_imm(&img, ((fw - cw) / 2.0) as u32, ((fh - ch) / 2.0) as u32, cw as u32, ch as u32).to_image();
    contact::save(&crop, out)?;
    Ok((crop.width() as usize, crop.height() as usize))
}

pub fn trace_cmd(a: TraceArgs) -> Result<ExitCode> {
    let t0 = Instant::now();
    let proj_dir = a.output.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let dir = proj_dir.join("trace");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let info = media::probe(&a.video)?;
    eprintln!("decoding {} ({}x{}, {} fps)", a.video.display(), info.width, info.height, info.fps);
    let frames = media::decode_frames(&a.video, &info, info.height)?;
    let mut opts = planar::TrackOpts { min_ncc: a.min_ncc, ..Default::default() };
    for i in &a.ignore {
        match i.as_str() {
            "dark" => opts.ignore_dark = true,
            "red" => opts.ignore_red = true,
            other => bail!("--ignore {other}: use dark and/or red"),
        }
    }
    let mut windows: BTreeMap<String, Vec<(usize, usize)>> = Default::default();
    for w in &a.window {
        let (name, ranges) = w.split_once('=').with_context(|| format!("--window {w}: write NAME=FIRST-LAST"))?;
        windows.insert(name.trim().to_string(), parse_ranges(ranges)?);
    }
    let mut fills: BTreeMap<String, PathBuf> = Default::default();
    for f in &a.fill {
        let (name, file) = f.split_once('=').with_context(|| format!("--fill {f}: write NAME=FILE"))?;
        fills.insert(name.trim().to_string(), PathBuf::from(file.trim()));
    }
    let mut arts = Vec::new();
    for p in image_files(&a.arts)? {
        let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "art".into());
        arts.push(planar::load_art(&p, &name, windows.get(&name).cloned().unwrap_or_default())?);
    }
    // Proxies: rectangles of reference frames, used as templates only.
    let mut proxies: BTreeMap<String, (usize, usize)> = Default::default();
    for p in &a.proxy {
        let bad = || format!("--proxy {p}: write NAME=FRAME:X,Y,W,H (pixels of the reference)");
        let (name, spec) = p.split_once('=').with_context(bad)?;
        let (fr, rect) = spec.split_once(':').with_context(bad)?;
        let fr: usize = fr.trim().parse().with_context(bad)?;
        let r: Vec<usize> = rect.split(',').map(|v| v.trim().parse::<usize>()).collect::<std::result::Result<_, _>>().with_context(bad)?;
        if r.len() != 4 || fr >= frames.len() || r[0] + r[2] > frames.w || r[1] + r[3] > frames.h || r[2] < 16 || r[3] < 16 {
            bail!("--proxy {p}: the rectangle must lie inside frame {fr} of the {}x{} reference ({} frames) and be at least 16 px", frames.w, frames.h, frames.len());
        }
        let img = contact::frame_image(&frames, fr);
        let crop = image::imageops::crop_imm(&img, r[0] as u32, r[1] as u32, r[2] as u32, r[3] as u32).to_image();
        let path = dir.join("proxies").join(format!("{}.png", name.trim()));
        contact::save(&crop, &path)?;
        let name = name.trim().to_string();
        proxies.insert(name.clone(), (r[2], r[3]));
        arts.push(planar::load_art(&path, &name, windows.get(&name).cloned().unwrap_or_default())?);
    }
    if arts.is_empty() {
        bail!("no pictures: give --arts (files or folders) and/or --proxy");
    }
    for name in fills.keys() {
        if !proxies.contains_key(name) {
            bail!("--fill {name}: there is no --proxy named {name}");
        }
    }
    // Pictures without a window: a quick pass over every 3rd frame finds them.
    let need: Vec<usize> = (0..arts.len()).filter(|i| arts[*i].windows.is_empty()).collect();
    if !need.is_empty() {
        eprintln!("looking for {} picture(s) in every 3rd frame", need.len());
        let sub: Vec<planar::Art> = need.iter().map(|i| planar::Art { windows: vec![], gray: arts[*i].gray.clone(), name: arts[*i].name.clone(), path: arts[*i].path.clone(), w: arts[*i].w, h: arts[*i].h }).collect();
        let found = planar::find_windows(&frames, &sub, &opts, 3, 0.3);
        for (k, i) in need.iter().enumerate() {
            arts[*i].windows = found[k].clone();
        }
    }
    let (present, absent): (Vec<planar::Art>, Vec<planar::Art>) = arts.into_iter().partition(|x| !x.windows.is_empty());
    let progress = |name: &str, f: usize| {
        if f.is_multiple_of(48) {
            eprintln!("tracking {name} at frame {f}");
        }
    };
    let mut tr = planar::track(&frames, &a.video.display().to_string(), &present, &opts, &progress);
    for x in &absent {
        tr.arts.push(planar::ArtTrack { name: x.name.clone(), path: x.path.clone(), size: [x.w, x.h], windows: vec![], keys: vec![], missing: vec![] });
    }
    // A proxy's layer shows its fill: the same corners, the fill's pixels.
    for at in &mut tr.arts {
        let Some(&(pw, ph)) = proxies.get(&at.name) else { continue };
        match fills.get(&at.name) {
            Some(file) => {
                let path = dir.join("fills").join(format!("{}.png", at.name));
                let (cw, ch) = crop_fill(file, pw, ph, &path)?;
                let (sx, sy) = (pw as f64 / cw as f64, ph as f64 / ch as f64);
                for k in &mut at.keys {
                    for r in 0..3 {
                        k.h[r * 3] *= sx;
                        k.h[r * 3 + 1] *= sy;
                    }
                }
                at.path = path.display().to_string();
                at.size = [cw, ch];
            }
            None => {
                eprintln!("note: proxy {} has no --fill; its layer shows a placeholder until you set the slot's path", at.name);
                at.path = String::new();
            }
        }
    }
    let track_path = dir.join("track.json");
    std::fs::write(&track_path, edits_analyze::jsonfmt::to_string(&tr)?)?;
    eprintln!("measuring the grade");
    let mut graded = tr.clone();
    graded.arts.retain(|x| !x.path.is_empty() && !x.keys.is_empty());
    let grade = trace::measure_grade(&a.video, &graded, 9, 1)?;
    let grade_path = dir.join("grade.json");
    std::fs::write(&grade_path, edits_analyze::jsonfmt::to_string(&grade)?)?;
    // The beat grid of the reference's sound: cuts near a beat go on it.
    let samples = media::decode_audio(&a.video, &info, edits_analyze::song::RATE)?;
    let beats = samples
        .as_ref()
        .and_then(|s| edits_analyze::song::analyze_samples(s, edits_analyze::song::RATE, "reference", &Default::default()).ok())
        .map(|s| (s.tempo.bpm, s.grid.beat_offset, s.beats.iter().map(|b| b.t).collect::<Vec<_>>()));
    let audio = if a.no_audio || samples.is_none() { None } else { Some(relative(&a.video, &proj_dir)) };
    let base = proj_dir.clone();
    let dopts = trace::DraftOpts {
        width: a.width.unwrap_or(info.width),
        height: a.height.unwrap_or(info.height),
        fps: info.fps,
        duration: frames.len() as f64 / info.fps,
        audio,
        beats,
        rel: Box::new(move |p: &str| if p.is_empty() { String::new() } else { relative(Path::new(p), &base) }),
        samples: 8,
    };
    let (mut project, mut report) = trace::draft(&tr, Some(&grade), &dopts);
    // Slots of proxies without a fill stay empty (a placeholder).
    if let Some(slots) = project["slots"].as_object_mut() {
        for s in slots.values_mut() {
            if s["path"].as_str() == Some("") {
                s.as_object_mut().map(|o| o.remove("path"));
            }
        }
    }
    let text = serde_json::to_string_pretty(&project)?;
    edits_core::Project::from_json(&text).context("the drafted project does not parse (a bug in edits trace)")?;
    std::fs::write(&a.output, &text).with_context(|| format!("writing {}", a.output.display()))?;
    report["project"] = json!(a.output);
    report["track"] = json!(track_path);
    report["grade"] = json!(grade_path);
    if a.preview {
        let exe = std::env::current_exe()?;
        let draft = dir.join("draft.mp4");
        eprintln!("rendering the draft");
        let st = std::process::Command::new(&exe).arg("render").arg(&a.output).arg("-o").arg(&draft).arg("--quiet").stdout(std::process::Stdio::null()).status()?;
        if !st.success() {
            bail!("rendering the draft failed; run `edits render {} -o draft.mp4` to see why", a.output.display());
        }
        let sheet = dir.join("worst.jpg");
        let m = trace::metric(&draft, &a.video, &trace::MetricOpts { pre_blur: 2.0, worst: 8, blocks: vec![], sheet: Some(sheet.clone()) })?;
        let sbs = dir.join("side_by_side.mp4");
        side_by_side(&a.video, &draft, &sbs)?;
        report["preview"] = json!({ "render": draft, "ssim_mean": m.ssim_mean, "author_share_mean": m.author_share_mean, "worst_frames": m.worst.iter().map(|f| f.frame).collect::<Vec<_>>(), "sheet": sheet, "side_by_side": sbs });
    }
    report["seconds"] = json!(t0.elapsed().as_secs());
    report["next"] = json!([
        format!("look at {} (and the reference frames) yourself", dir.join("worst.jpg").display()),
        format!("edits render {} -o ours.mp4 && edits metric ours.mp4 {} --project {} --sheet worst.jpg", a.output.display(), a.video.display(), a.output.display()),
    ]);
    if a.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        for l in report["layers"].as_array().into_iter().flatten() {
            println!("{}: {} frames {} ncc {}", l["id"].as_str().unwrap_or(""), l["art"].as_str().unwrap_or(""), l["frames"], l["mean_ncc"]);
        }
        if let Some(nf) = report["arts_not_found"].as_array().filter(|v| !v.is_empty()) {
            println!("not found in the reference: {}", nf.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
        }
        if let Some(p) = report.get("preview") {
            println!("draft SSIM {} (author pixels {}), worst frames {}", p["ssim_mean"], p["author_share_mean"], p["worst_frames"]);
            println!("sheet {}, side by side {}", p["sheet"], p["side_by_side"]);
        }
        println!("wrote {}", a.output.display());
    }
    Ok(ExitCode::SUCCESS)
}
