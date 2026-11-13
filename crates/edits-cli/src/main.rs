//! `edits`: render, inspect and check edit projects from a shell.
//!
//! Data goes to stdout (JSON where it is data), progress and errors go to
//! stderr, and every failure ends with a non-zero exit code.

mod analysis;
mod beatcmd;
mod demo;
mod encode;
mod launch;
mod sheet;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use edits_core::{check, describe, effects, Project, TimeBase};
use edits_render::{AdapterPolicy, RenderOptions, Renderer};
use serde_json::json;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "edits", version, about = "Render beat-synced edits from a JSON project", long_about = None)]
struct Cli {
    /// Which adapter to render on: auto, hardware or software [env: EDITS_ADAPTER]
    #[arg(long, global = true, value_name = "POLICY")]
    adapter: Option<AdapterPolicy>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a project to an H.264 MP4 through the system ffmpeg
    Render(RenderArgs),
    /// Render one frame to a PNG
    Frame(FrameArgs),
    /// Render a contact sheet: a grid of frames labelled with frame number and time
    Sheet(SheetArgs),
    /// Time the renderer on a project without writing anything
    Bench(BenchArgs),
    /// Print a compact text summary of a project
    Describe {
        /// Project file (.json)
        project: PathBuf,
    },
    /// Validate a project; exits 1 when it has errors
    Check {
        /// Project file (.json)
    project: PathBuf,
        /// Print the findings as JSON
        #[arg(long)]
        json: bool,
    },
    /// List every effect with its parameters, defaults and meaning
    Effects {
        #[arg(long)]
        json: bool,
    },
    /// Report the graphics adapter and the ffmpeg encoders this machine has
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Write a self-contained demo project with generated pictures and audio
    Demo {
        /// Folder to write into (created if missing)
        dir: PathBuf,
    },
    #[command(flatten)]
    Analysis(analysis::AnalysisCommand),
    /// List the presets (named, parameterised flashes, punches, slides, transitions...) with their variables
    Presets(beatcmd::PresetsArgs),
    /// Apply a preset to a project at one or more times, as one undoable edit
    Apply(beatcmd::ApplyArgs),
    /// Save what a layer does over a time range as a preset in the project's presets/ folder
    SavePreset(beatcmd::SavePresetArgs),
    /// Beat operations: cut pictures to the beat, accent beats and onsets with a preset
    #[command(subcommand)]
    Beats(beatcmd::BeatsCommand),
    /// Custom effect shaders: `edits shader check <file.wgsl>` validates one without a GPU
    #[command(subcommand)]
    Shader(ShaderCommand),
    /// List every transition with its parameters
    Transitions {
        #[arg(long)]
        json: bool,
    },
    /// Tune a project's effect values until its render matches a reference recipe (flashes, saturation, blur, zoom, moves)
    Fit(analysis::FitArgs),
    /// Open the preview window on a project (runs `edits-gui`; see docs/GUI.md)
    #[command(disable_help_flag = true)]
    Gui {
        /// Arguments for edits-gui, e.g. `project.json` or `project.json --screenshot out.png --at 30`
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Serve the MCP tools over stdio for an agent (runs `edits-mcp`; see docs/AGENTS.md)
    #[command(disable_help_flag = true)]
    Mcp {
        /// Arguments for edits-mcp, e.g. `--root <dir>` or `--tools`
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
}

#[derive(Subcommand)]
enum ShaderCommand {
    /// Check a custom effect file against the custom-effect contract (fn effect(uv: vec2<f32>) -> vec4<f32>); exits 1 when it would not compile
    Check {
        /// The WGSL file
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct RenderArgs {
    /// Project file (.json)
    project: PathBuf,
    /// Output file (.mp4)
    #[arg(short, long)]
    output: PathBuf,
    /// Start of the range: project time (beats or seconds), or b8, bar 2, 2.5s, f60, a marker
    #[arg(long, value_name = "T")]
    from: Option<String>,
    /// End of the range, written the same way
    #[arg(long, value_name = "T")]
    to: Option<String>,
    /// Render at this fraction of the project resolution
    #[arg(long, default_value_t = 1.0)]
    scale: f32,
    /// Motion blur samples per frame (overrides the project)
    #[arg(long, value_name = "N")]
    samples: Option<u32>,
    /// H.264 quality: lower is better, 18 is visually lossless
    #[arg(long, default_value_t = 18)]
    crf: u32,
    /// Do not mux the project's audio
    #[arg(long)]
    no_audio: bool,
    /// Do not print progress
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Args)]
struct BenchArgs {
    /// Project file (.json)
    project: PathBuf,
    /// Start of the range: project time (beats or seconds), or b8, bar 2, 2.5s, f60, a marker
    #[arg(long, value_name = "T")]
    from: Option<String>,
    /// End of the range, written the same way
    #[arg(long, value_name = "T")]
    to: Option<String>,
    /// Time only this many frames, spread evenly over the range [default: every frame]
    #[arg(long, value_name = "N")]
    count: Option<u64>,
    #[arg(long, default_value_t = 1.0)]
    scale: f32,
    /// Motion blur samples (overrides the project)
    #[arg(long, value_name = "N")]
    samples: Option<u32>,
    /// Also time every kind of render pass (slower: waits for the GPU after each pass)
    #[arg(long)]
    passes: bool,
}

#[derive(Args)]
struct FrameArgs {
    /// Project file (.json)
    project: PathBuf,
    /// Project time (beats or seconds), or b8, bar 2, 4:3, 2.5s, a marker such as drop; the frame containing it is rendered
    #[arg(long, value_name = "T", conflicts_with = "frame", allow_hyphen_values = true)]
    at: Option<String>,
    /// Frame number, starting at 0
    #[arg(long, value_name = "N")]
    frame: Option<u64>,
    /// Output file (.png)
    #[arg(short, long)]
    output: PathBuf,
    #[arg(long, default_value_t = 1.0)]
    scale: f32,
    /// Motion blur samples (overrides the project)
    #[arg(long, value_name = "N")]
    samples: Option<u32>,
}

#[derive(Args)]
struct SheetArgs {
    /// Project file (.json)
    project: PathBuf,
    /// Start of the range: project time (beats or seconds), or b8, bar 2, 2.5s, f60, a marker
    #[arg(long, value_name = "T")]
    from: Option<String>,
    /// End of the range, written the same way
    #[arg(long, value_name = "T")]
    to: Option<String>,
    /// How many frames, spread evenly over the range [default: 16]
    #[arg(long, value_name = "N", conflicts_with = "every_frame")]
    count: Option<u64>,
    /// Every frame of the range
    #[arg(long)]
    every_frame: bool,
    /// Columns [default: about the square root of the number of frames]
    #[arg(long, value_name = "C")]
    cols: Option<u32>,
    /// Size of each cell as a fraction of the project resolution [default: cells about 320 px wide]
    #[arg(long)]
    scale: Option<f32>,
    /// Motion blur samples (overrides the project)
    #[arg(long, value_name = "N")]
    samples: Option<u32>,
    /// Output file (.png)
    #[arg(short, long)]
    output: PathBuf,
    /// Do not print progress
    #[arg(short, long)]
    quiet: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let adapter = cli.adapter;
    match cli.command {
        Command::Render(a) => render(a, adapter),
        Command::Frame(a) => frame(a, adapter),
        Command::Sheet(a) => contact_sheet(a, adapter),
        Command::Bench(a) => bench(a, adapter),
        Command::Describe { project } => {
            print!("{}", describe::describe(&read_project(&project)?));
            Ok(ExitCode::SUCCESS)
        }
        Command::Check { project, json } => check_project(&project, json),
        Command::Effects { json } => {
            list_effects(json);
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor { json } => doctor(json, adapter),
        Command::Demo { dir } => {
            let files = demo::write(&dir)?;
            let project = dir.join("project.json");
            eprintln!("demo written to {}; try: edits render {} -o demo.mp4", dir.display(), project.display());
            print_json(&json!({ "project": project, "files": files }));
            Ok(ExitCode::SUCCESS)
        }
        Command::Analysis(cmd) => analysis::run(cmd),
        Command::Presets(a) => beatcmd::presets(a),
        Command::Apply(a) => beatcmd::apply(a),
        Command::SavePreset(a) => beatcmd::save_preset(a),
        Command::Beats(cmd) => beatcmd::beats(cmd),
        Command::Shader(ShaderCommand::Check { file, json }) => shader_check(&file, json),
        Command::Transitions { json } => {
            beatcmd::transitions(json);
            Ok(ExitCode::SUCCESS)
        }
        Command::Fit(a) => analysis::fit(a),
        Command::Gui { args } => launch::exec("edits-gui", "EDITS_GUI_BIN", "edits-gui", &args),
        Command::Mcp { args } => launch::exec("edits-mcp", "EDITS_MCP_BIN", "edits-mcp", &args),
    }
}

pub(crate) fn print_json(value: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(value).expect("JSON serialises"));
}

// ---- projects ----------------------------------------------------------------

fn read_project(path: &Path) -> Result<Project> {
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read project {}", path.display()))?;
    Project::from_json(&text).with_context(|| format!("{} is not a valid project (see docs/FORMAT.md)", path.display()))
}

fn base_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn finding_line(f: &check::Finding) -> String {
    let level = if f.severity == check::Severity::Error { "error" } else { "warning" };
    let hint = f.hint.as_deref().map(|h| format!(" ({h})")).unwrap_or_default();
    format!("{level}: {}: {}{hint}", f.path, f.message)
}

/// Read and validate a project for rendering; errors stop here with the list of what to fix.
fn load(path: &Path) -> Result<(Project, PathBuf)> {
    let project = read_project(path)?;
    let dir = base_dir(path);
    let findings = check::check(&project, Some(&dir));
    if check::has_errors(&findings) {
        let lines: Vec<String> = findings.iter().filter(|f| f.severity == check::Severity::Error).map(finding_line).collect();
        bail!("{} has errors:\n{}", path.display(), lines.join("\n"));
    }
    Ok((project, dir))
}

fn check_project(path: &Path, as_json: bool) -> Result<ExitCode> {
    let findings = match read_project(path) {
        Ok(project) => check::check(&project, Some(&base_dir(path))),
        // A file that does not parse is a finding too, so `--json` always answers in JSON.
        Err(e) => vec![check::Finding { severity: check::Severity::Error, path: String::new(), message: format!("{e:#}"), hint: Some("see docs/FORMAT.md".into()) }],
    };
    let errors = findings.iter().filter(|f| f.severity == check::Severity::Error).count();
    let warnings = findings.len() - errors;
    if as_json {
        print_json(&json!({ "ok": errors == 0, "errors": errors, "warnings": warnings, "findings": findings }));
    } else {
        for f in &findings {
            println!("{}", finding_line(f));
        }
        println!("{}: {errors} error(s), {warnings} warning(s)", path.display());
    }
    Ok(if errors == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn list_effects(as_json: bool) {
    let range = |r: Option<(f64, f64)>| match r {
        // JSON has no infinity: an open end is null.
        Some((lo, hi)) => json!([if lo.is_finite() { json!(lo) } else { json!(null) }, if hi.is_finite() { json!(hi) } else { json!(null) }]),
        None => json!(null),
    };
    if as_json {
        let list: Vec<_> = effects::EFFECTS
            .iter()
            .map(|e| {
                let params: Vec<_> = e
                    .params
                    .iter()
                    .map(|p| {
                        json!({ "name": p.name, "default": if p.default.len() == 1 { json!(p.default[0]) } else { json!(p.default) },
                                "unit": p.unit.as_str(), "range": range(p.range), "doc": p.doc })
                    })
                    .collect();
                let options: Vec<_> = e.options.iter().map(|o| json!({ "name": o.name, "values": o.values, "default": o.default(), "doc": o.doc })).collect();
                json!({ "kind": e.kind, "category": e.category, "doc": e.doc, "params": params, "options": options,
                        "source": e.source, "reads_time": e.reads_time, "credit": e.credit })
            })
            .collect();
        print_json(&json!(list));
        return;
    }
    let mut category = "";
    for e in effects::EFFECTS {
        if e.category != category {
            category = e.category;
            println!("-- {category} --");
        }
        println!("{}: {}", e.kind, e.doc);
        for p in e.params {
            let default = if p.default.len() == 1 { format!("{}", p.default[0]) } else { format!("{:?}", p.default) };
            let unit = p.unit.as_str();
            let bounds = p.range.map(|(lo, hi)| {
                let b = |v: f64| if v.is_finite() { format!("{v}") } else { String::new() };
                format!(" {}..{}", b(lo), b(hi))
            });
            let unit = format!("{unit}{}", bounds.unwrap_or_default());
            println!("    {:<12} default {:<16} {:<10} {}", p.name, default, unit.trim(), p.doc);
        }
        for o in e.options {
            println!("    option {:<5} {} (default {}): {}", o.name, o.values.join(" | "), o.default(), o.doc);
        }
        if !e.credit.is_empty() {
            println!("    credit: {}", e.credit);
        }
    }
    let kinds: Vec<&str> = effects::TRANSITIONS.iter().map(|t| t.kind).collect();
    println!("\ntransitions (a layer's \"transition\", not an effect; `edits transitions` describes them): {}", kinds.join(", "));
}

fn shader_check(file: &Path, as_json: bool) -> Result<ExitCode> {
    let text = std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    let r = edits_render::shader::check_custom_effect(&text);
    if as_json {
        print_json(&json!({ "file": file, "ok": r.ok, "issues": r.issues, "params": r.params, "animated": r.animated }));
    } else {
        for i in &r.issues {
            match (i.line, i.column) {
                (Some(l), Some(c)) => println!("error: {}:{l}:{c}: {}", file.display(), i.message),
                _ => println!("error: {}: {}", file.display(), i.message),
            }
            if let Some(code) = &i.code {
                println!("    {code}");
            }
        }
        if r.ok {
            let params = if r.params.is_empty() { "no parameters".to_string() } else { format!("reads {}", r.params.join(", ")) };
            println!("{}: ok ({params}{})", file.display(), if r.animated { ", changes with time" } else { "" });
        }
    }
    Ok(if r.ok { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

// ---- rendering ---------------------------------------------------------------

fn adapter_json(r: &Renderer) -> serde_json::Value {
    let i = r.info();
    json!({ "name": i.name, "backend": i.backend, "type": i.device_type, "driver": i.driver, "software": i.software })
}

/// Open a renderer and give it the project. Returns it with the time that took, in ms.
fn open(project: &Project, dir: &Path, adapter: Option<AdapterPolicy>, samples: Option<u32>) -> Result<(Renderer, f64)> {
    let started = Instant::now();
    let mut renderer = Renderer::new(RenderOptions { adapter, ..Default::default() }).context("cannot start the renderer (run `edits doctor`)")?;
    renderer.prepare(project, dir)?;
    if let Some(n) = samples {
        if n == 0 || n > 64 {
            bail!("--samples must be between 1 and 64");
        }
        renderer.set_motion_blur_samples(Some(n));
    }
    Ok((renderer, started.elapsed().as_secs_f64() * 1000.0))
}

/// A time written on the command line (`12`, `b12`, `bar 4`, `2.5s`, `drop`...) in project time.
fn time_arg(p: &Project, t: &Option<String>, flag: &str) -> Result<Option<f64>> {
    t.as_deref().map(|t| edits_core::time::TimeCtx::from_project(p).resolve(t).map_err(|e| anyhow!("{flag}: {e}"))).transpose()
}

/// Frames `first..end` covering project times `from..to` (defaults: the whole project).
fn frame_range(p: &Project, from: &Option<String>, to: &Option<String>) -> Result<(u64, u64)> {
    let (from, to) = (time_arg(p, from, "--from")?, time_arg(p, to, "--to")?);
    let total = p.frame_count();
    let index = |t: f64| (p.to_seconds(t) * p.settings.fps - 1e-6).ceil().max(0.0) as u64;
    let first = from.map(index).unwrap_or(0);
    let end = to.map(index).unwrap_or(total).min(total);
    if first >= end {
        bail!(
            "the range --from {} --to {} contains no frames (the project runs from 0 to {} {}, {total} frames)",
            from.map(|v| v.to_string()).unwrap_or_else(|| "start".into()),
            to.map(|v| v.to_string()).unwrap_or_else(|| "end".into()),
            p.settings.duration,
            unit(p)
        );
    }
    Ok((first, end))
}

fn unit(p: &Project) -> &'static str {
    if p.settings.time_base == TimeBase::Beats {
        "beats"
    } else {
        "seconds"
    }
}

fn check_scale(scale: f32) -> Result<()> {
    if !(scale.is_finite() && scale > 0.0 && scale <= 8.0) {
        bail!("--scale must be greater than 0 and at most 8");
    }
    Ok(())
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    Ok(())
}

/// Progress lines on stderr: rewritten in place on a terminal, about one a second otherwise.
struct Progress {
    quiet: bool,
    tty: bool,
    total: u64,
    started: Instant,
    last: Option<Instant>,
}

impl Progress {
    fn new(total: u64, quiet: bool) -> Self {
        Self { quiet, tty: std::io::stderr().is_terminal(), total, started: Instant::now(), last: None }
    }
    fn tick(&mut self, done: u64) {
        if self.quiet {
            return;
        }
        let now = Instant::now();
        let finished = done == self.total;
        let every = if self.tty { 0.1 } else { 1.0 };
        if !finished && self.last.is_some_and(|l| now.duration_since(l).as_secs_f64() < every) {
            return;
        }
        self.last = Some(now);
        let elapsed = self.started.elapsed().as_secs_f64();
        let per = elapsed / done.max(1) as f64;
        let line = format!("frame {done}/{} ({:.0}%)  {:.0} ms/frame  {:.0} s left", self.total, done as f64 * 100.0 / self.total as f64, per * 1000.0, per * (self.total - done) as f64);
        let mut err = std::io::stderr().lock();
        let _ = if self.tty { write!(err, "\r{line}\x1b[K{}", if finished { "\n" } else { "" }) } else { writeln!(err, "{line}") };
        let _ = err.flush();
    }
}

/// The sound of the project's video layers, for a render that starts `start`
/// seconds into the project. A layer is heard when it is enabled, its
/// `volume` is above 0, its file has sound, and its speed is constant with
/// no `time_remap` (a ramp or a remap would need the sound retimed sample by
/// sample; those layers are left silent, with a warning).
fn clip_audio(project: &Project, dir: &Path, start: f64) -> Vec<encode::ClipAudio> {
    let mut out = Vec::new();
    for l in project.layers.iter().filter(|l| l.enabled) {
        let edits_core::LayerKind::Video(v) = &l.kind else { continue };
        let Some(slot) = project.slots.get(&v.slot) else { continue };
        let Some(path) = slot.path.as_deref().map(|p| dir.join(p)) else { continue };
        if v.volume <= 0.0 {
            continue;
        }
        let Ok(info) = edits_render::video::probe(&path) else { continue };
        if !info.has_audio {
            continue;
        }
        let speed = match (&v.time_remap, &v.speed) {
            (None, None) => 1.0,
            (None, Some(edits_core::Prop::Const(edits_core::Value::Num(s)))) => *s,
            _ => {
                eprintln!("warning: layer \"{}\": the sound of a video with an animated speed or a time_remap is not mixed", l.id);
                continue;
            }
        };
        let (first, count) = slot.trim_frames(info.fps, info.frames);
        let begin = project.to_seconds(l.start);
        let end = project.to_seconds(l.end.unwrap_or(project.settings.duration).min(project.settings.duration));
        out.push(encode::ClipAudio {
            path,
            source: [first as f64 / info.fps, (first + count) as f64 / info.fps],
            at: begin - start,
            length: end - begin,
            speed,
            looped: v.looped,
            reverse: v.reverse,
            volume: v.volume,
        });
    }
    out
}

/// Seconds of sound in a file, from ffprobe; `None` when it cannot tell.
fn sound_seconds(path: &Path) -> Option<f64> {
    let out = std::process::Command::new(edits_render::video::ffprobe_program())
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=duration:format=duration", "-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    // The stream's duration first, then the container's.
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.trim().parse::<f64>().ok()).find(|d| d.is_finite() && *d > 0.0)
}

/// The project's sound effects (`audio_clips`), for a render that starts
/// `start` seconds into the project. A clip is heard when it is enabled and
/// its volume is above 0. Its end is `trim_out`, or the end of the file.
fn sound_clips(project: &Project, dir: &Path, start: f64) -> Vec<encode::SoundClip> {
    let mut out = Vec::new();
    for c in project.audio_clips.iter().filter(|c| c.enabled && c.volume > 0.0) {
        let path = dir.join(&c.path);
        let file = sound_seconds(&path);
        if file.is_none() {
            eprintln!("warning: audio clip \"{}\": ffprobe cannot tell how long {} is; it plays to its end without a fade out", c.id, path.display());
        }
        let end = match (c.trim_out, file) {
            (Some(b), Some(f)) => b.min(f),
            (Some(b), None) => b,
            (None, Some(f)) => f,
            (None, None) => f64::INFINITY,
        };
        if end <= c.trim_in {
            eprintln!("warning: audio clip \"{}\": nothing of {} is left after trim_in {} s; it is not heard", c.id, path.display(), c.trim_in);
            continue;
        }
        out.push(encode::SoundClip {
            path,
            source: [c.trim_in, end],
            at: project.to_seconds(c.at) - start,
            volume: c.volume,
            fade_in: c.fade_in,
            fade_out: c.fade_out,
        });
    }
    out
}

fn render(a: RenderArgs, adapter: Option<AdapterPolicy>) -> Result<ExitCode> {
    check_scale(a.scale)?;
    let (project, dir) = load(&a.project)?;
    let (first, end) = frame_range(&project, &a.from, &a.to)?;
    let ffmpeg = encode::probe();
    let encoder = ffmpeg.h264_encoder()?.to_string();
    let audio = match (&project.audio, a.no_audio) {
        (Some(au), false) => {
            if ffmpeg.aac.is_empty() {
                eprintln!("warning: this ffmpeg has no AAC encoder; rendering without audio");
                None
            } else {
                Some(encode::AudioInput { path: dir.join(&au.path), start: au.offset + first as f64 / project.settings.fps, gain: au.gain })
            }
        }
        _ => None,
    };
    let clips = if a.no_audio || ffmpeg.aac.is_empty() { Vec::new() } else { clip_audio(&project, &dir, first as f64 / project.settings.fps) };
    let fps = project.settings.fps;
    let mut sounds = if a.no_audio || ffmpeg.aac.is_empty() { Vec::new() } else { sound_clips(&project, &dir, first as f64 / fps) };
    sounds.retain(|s| s.heard((end - first) as f64 / fps));
    let has_audio = audio.is_some() || !clips.is_empty() || !sounds.is_empty();
    let clip_count = clips.len();
    let sound_count = sounds.len();
    let (mut renderer, startup_ms) = open(&project, &dir, adapter, a.samples)?;
    let (width, height) = renderer.output_size(a.scale)?;
    ensure_parent(&a.output)?;
    let frames = end - first;
    let job = encode::EncodeJob { output: &a.output, width, height, fps: project.settings.fps, frames, crf: a.crf, encoder: &encoder, audio, clips, sounds, aac: ffmpeg.aac.first().map(String::as_str) };
    let mut enc = encode::Encoder::start(&job)?;

    let started = Instant::now();
    let mut progress = Progress::new(frames, a.quiet);
    let mut first_frame_ms = 0.0;
    let mut render_seconds = 0.0;
    for (done, index) in (first..end).enumerate() {
        let t = Instant::now();
        let image = renderer.render_frame(index, a.scale).with_context(|| format!("rendering frame {index}"))?;
        let took = t.elapsed().as_secs_f64();
        render_seconds += took;
        if done == 0 {
            first_frame_ms = took * 1000.0;
        }
        enc.write(image.as_raw())?;
        progress.tick(done as u64 + 1);
    }
    enc.finish()?;
    let seconds = started.elapsed().as_secs_f64();
    let stats = renderer.stats();
    print_json(&json!({
        "output": a.output,
        "frames": frames,
        "first_frame": first,
        "width": width,
        "height": height,
        "fps": project.settings.fps,
        "duration_seconds": frames as f64 / project.settings.fps,
        "motion_blur_samples": stats.samples,
        "seconds": round(seconds, 3),
        "ms_per_frame": round(render_seconds * 1000.0 / frames as f64, 1),
        "startup_ms": round(startup_ms, 1),
        "first_frame_ms": round(first_frame_ms, 1),
        "adapter": adapter_json(&renderer),
        "encoder": encoder,
        "audio": has_audio,
        "clip_audio": clip_count,
        "audio_clips": sound_count,
    }));
    Ok(ExitCode::SUCCESS)
}

fn round(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (v * k).round() / k
}

fn frame(a: FrameArgs, adapter: Option<AdapterPolicy>) -> Result<ExitCode> {
    check_scale(a.scale)?;
    let (project, dir) = load(&a.project)?;
    let total = project.frame_count();
    let at = time_arg(&project, &a.at, "--at")?;
    let index = match (a.frame, at) {
        (Some(n), _) => n,
        (None, Some(t)) => {
            if t < 0.0 {
                bail!("--at must not be negative");
            }
            (project.to_seconds(t) * project.settings.fps + 1e-6).floor().max(0.0) as u64
        }
        (None, None) => 0,
    };
    if index >= total {
        bail!("frame {index} is past the end of the project ({total} frames, {} {})", project.settings.duration, unit(&project));
    }
    let (mut renderer, startup_ms) = open(&project, &dir, adapter, a.samples)?;
    let started = Instant::now();
    let image = renderer.render_frame(index, a.scale)?;
    let render_ms = started.elapsed().as_secs_f64() * 1000.0;
    ensure_parent(&a.output)?;
    image.save(&a.output).with_context(|| format!("cannot write {}", a.output.display()))?;
    let time = project.frame_time(index);
    print_json(&json!({
        "output": a.output,
        "frame": index,
        "time": round(time, 4),
        "time_unit": unit(&project),
        "seconds": round(project.to_seconds(time), 4),
        "width": image.width(),
        "height": image.height(),
        "render_ms": round(render_ms, 1),
        "startup_ms": round(startup_ms, 1),
        "adapter": adapter_json(&renderer),
    }));
    Ok(ExitCode::SUCCESS)
}

/// The label under a contact sheet cell: frame number, project time, seconds.
fn cell_label(p: &Project, frame: u64) -> String {
    let t = p.frame_time(frame);
    match p.settings.time_base {
        TimeBase::Beats => format!("#{frame}  beat {t:.2}  {:.2}s", p.to_seconds(t)),
        TimeBase::Seconds => format!("#{frame}  {t:.2}s"),
    }
}

fn contact_sheet(a: SheetArgs, adapter: Option<AdapterPolicy>) -> Result<ExitCode> {
    let (project, dir) = load(&a.project)?;
    let (first, end) = frame_range(&project, &a.from, &a.to)?;
    let frames: Vec<u64> = if a.every_frame { (first..end).collect() } else { sheet::spread(first, end, a.count.unwrap_or(16).max(1)) };
    let scale = a.scale.unwrap_or_else(|| (320.0 / project.settings.width as f32).min(1.0));
    check_scale(scale)?;
    let cols = a.cols.unwrap_or_else(|| (frames.len() as f64).sqrt().ceil() as u32).max(1);
    let (mut renderer, startup_ms) = open(&project, &dir, adapter, a.samples)?;
    let started = Instant::now();
    let mut progress = Progress::new(frames.len() as u64, a.quiet);
    let mut cells = Vec::with_capacity(frames.len());
    for (done, &index) in frames.iter().enumerate() {
        let image = renderer.render_frame(index, scale).with_context(|| format!("rendering frame {index}"))?;
        cells.push(sheet::Cell { frame: index, label: cell_label(&project, index), image });
        progress.tick(done as u64 + 1);
    }
    let image = sheet::compose(&cells, cols);
    ensure_parent(&a.output)?;
    image.save(&a.output).with_context(|| format!("cannot write {}", a.output.display()))?;
    let cell = cells.first().map(|c| c.image.dimensions()).ok_or_else(|| anyhow!("no frames to draw"))?;
    print_json(&json!({
        "output": a.output,
        "frames": cells.iter().map(|c| c.frame).collect::<Vec<_>>(),
        "cols": cols.min(cells.len() as u32),
        "rows": (cells.len() as u32).div_ceil(cols.min(cells.len() as u32)),
        "cell_width": cell.0,
        "cell_height": cell.1,
        "width": image.width(),
        "height": image.height(),
        "seconds": round(started.elapsed().as_secs_f64(), 3),
        "startup_ms": round(startup_ms, 1),
        "adapter": adapter_json(&renderer),
    }));
    Ok(ExitCode::SUCCESS)
}

fn bench(a: BenchArgs, adapter: Option<AdapterPolicy>) -> Result<ExitCode> {
    check_scale(a.scale)?;
    let (project, dir) = load(&a.project)?;
    let (first, end) = frame_range(&project, &a.from, &a.to)?;
    let frames: Vec<u64> = match a.count {
        Some(n) => sheet::spread(first, end, n.max(1)),
        None => (first..end).collect(),
    };
    let (mut renderer, startup_ms) = open(&project, &dir, adapter, a.samples)?;
    renderer.set_profiling(a.passes);
    let started = Instant::now();
    let mut times = Vec::with_capacity(frames.len());
    let (mut passes, mut rendered_samples) = (0u64, 0u64);
    for &index in &frames {
        let t = Instant::now();
        renderer.render_frame(index, a.scale).with_context(|| format!("rendering frame {index}"))?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        passes += renderer.stats().passes as u64;
        rendered_samples += renderer.stats().rendered_samples as u64;
    }
    let seconds = started.elapsed().as_secs_f64();
    let stats = renderer.stats();
    let n = times.len() as f64;
    let first_frame_ms = times[0];
    // The first frame also compiles shaders and allocates textures: report it apart.
    let steady = if times.len() > 1 { &times[1..] } else { &times[..] };
    let mut sorted = steady.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    // Per kind of pass: how many, and where the time went.
    let mut kinds: Vec<(String, u64, f64)> = Vec::new();
    for t in renderer.take_profile() {
        match kinds.iter_mut().find(|k| k.0 == t.label) {
            Some(k) => (k.1, k.2) = (k.1 + 1, k.2 + t.ms),
            None => kinds.push((t.label, 1, t.ms)),
        }
    }
    kinds.sort_by(|a, b| b.2.total_cmp(&a.2));
    let kinds: Vec<_> = kinds.iter().map(|(label, count, ms)| json!({ "pass": label, "count": count, "ms_total": round(*ms, 1), "ms_each": round(ms / *count as f64, 2) })).collect();
    print_json(&json!({
        "passes": if a.passes { json!(kinds) } else { json!(null) },
        "frames": frames.len(),
        "width": stats.width,
        "height": stats.height,
        "motion_blur_samples": stats.samples,
        "rendered_samples_per_frame": round(rendered_samples as f64 / n, 2),
        "passes_per_frame": round(passes as f64 / n, 1),
        "seconds": round(seconds, 3),
        "ms_per_frame": round(steady.iter().sum::<f64>() / steady.len() as f64, 1),
        "ms_median": round(sorted[sorted.len() / 2], 1),
        "ms_slowest": round(sorted[sorted.len() - 1], 1),
        "first_frame_ms": round(first_frame_ms, 1),
        "startup_ms": round(startup_ms, 1),
        "adapter": adapter_json(&renderer),
    }));
    Ok(ExitCode::SUCCESS)
}

// ---- doctor ------------------------------------------------------------------

fn doctor(as_json: bool, adapter: Option<AdapterPolicy>) -> Result<ExitCode> {
    let renderer = Renderer::new(RenderOptions { adapter, ..Default::default() });
    let ffmpeg = encode::probe();
    let encoder = ffmpeg.h264_encoder().map(str::to_string);
    let ok = renderer.is_ok() && encoder.is_ok();
    if as_json {
        print_json(&json!({
            "ok": ok,
            "adapter": renderer.as_ref().ok().map(adapter_json),
            "adapter_error": renderer.as_ref().err().map(|e| format!("{e:#}")),
            "ffmpeg": { "found": ffmpeg.found, "version": ffmpeg.version },
            "encoders": { "h264": ffmpeg.h264, "aac": ffmpeg.aac, "selected_h264": encoder.as_ref().ok() },
            "encoder_error": encoder.as_ref().err().map(|e| format!("{e:#}")),
        }));
    } else {
        match &renderer {
            Ok(r) => {
                let i = r.info();
                println!("adapter:  {} ({}, {}{})", i.name, i.backend, i.device_type, if i.software { ", software rasteriser" } else { "" });
                println!("driver:   {}", i.driver);
            }
            Err(e) => println!("adapter:  NOT AVAILABLE: {e:#}"),
        }
        match &ffmpeg.version {
            Some(v) => println!("ffmpeg:   {v}"),
            None => println!("ffmpeg:   NOT FOUND"),
        }
        let list = |v: &[String]| if v.is_empty() { "none".to_string() } else { v.join(", ") };
        println!("h264:     {}", list(&ffmpeg.h264));
        println!("aac:      {}", list(&ffmpeg.aac));
        match &encoder {
            Ok(name) => println!("video:    will encode with {name}"),
            Err(e) => println!("video:    CANNOT ENCODE: {e:#}"),
        }
        println!("{}", if ok { "ready to render" } else { "not ready: see above" });
    }
    Ok(if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}
