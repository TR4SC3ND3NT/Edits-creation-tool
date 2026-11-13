//! The desktop window of the edits engine.
//!
//! It shows a project while something else edits it, and edits it too: a
//! preview rendered by `edits-render` on a worker thread, a transport, a
//! timeline (drag layers, edges and keys), an inspector of values (drag or
//! type them), slots, a history, and live reload of the project file. The
//! JSON file stays the only source of truth: every change the window makes
//! goes through `edits_core::edit` (validated, written atomically, backed up
//! in the history it shares with the agent's MCP server) and is read back.
//!
//! ```no_run
//! # fn main() -> anyhow::Result<()> {
//! edits_gui::run("demo/project.json".into(), edits_gui::Options::default())
//! # }
//! ```
//!
//! The modules without a window in them (`timemap`, `clock`, `cache`,
//! `rows`, `reload`, `export`, `editing`, the mailbox of `worker`, the peaks
//! of `audio`, the matching of `palette`, the bookkeeping of `toasts`) hold
//! the logic and are unit tested; `app`, `timeline`, `inspector` and
//! `panels` draw and turn input into calls of that logic, with the tokens,
//! fonts and icons of `theme` and the small controls of `widgets`.

pub mod audio;
pub mod cache;
pub mod clock;
pub mod editing;
pub mod export;
pub mod presets;
pub mod reload;
pub mod rows;
pub mod song;
pub mod timemap;
pub mod transitions;
pub mod uictx;
pub mod worker;

pub mod palette;
pub mod theme;
pub mod toasts;

mod app;
mod inspector;
mod panels;
mod timeline;
mod widgets;

pub use cache::Scale;
pub use edits_render::AdapterPolicy;

use anyhow::{anyhow, bail, Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// How to open the window.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Save a picture of the whole window to this PNG once the preview is
    /// on screen, then close. For automated checks.
    pub screenshot: Option<PathBuf>,
    /// Frame to start on (and the frame of the screenshot).
    pub at: Option<u64>,
    /// Window size in points, `(width, height)`.
    pub size: Option<(u32, u32)>,
    /// Screenshot mode: keep the window running this many seconds before
    /// the picture is taken, so a change to the project made meanwhile is in it.
    pub delay: f64,
    /// Layer id (or `camera`) to select in the inspector.
    pub select: Option<String>,
    /// Start playing at once.
    pub play: bool,
    /// Preview size relative to the project (default: half).
    pub scale: Option<Scale>,
    /// Memory for cached preview frames, in megabytes (default 384).
    pub cache_mb: Option<usize>,
    /// Graphics adapter for the renderer; `None` reads `EDITS_ADAPTER`.
    pub adapter: Option<AdapterPolicy>,
}

pub const USAGE: &str = "\
edits-gui: watch an edits project while it is being edited

usage: edits-gui <project.json> [options]

  --scale 0.25|0.5|1        preview size relative to the project (default 0.5)
  --at <frame>              frame to start on
  --play                    start playing
  --select <layer id>       show this layer in the inspector (or: camera)
  --size <W>x<H>            window size (default 1360x860)
  --cache-mb <N>            memory for cached preview frames (default 384)
  --adapter auto|hardware|software
                            graphics adapter of the renderer (or EDITS_ADAPTER)
  --screenshot <out.png>    save a picture of the window once the preview of
                            the frame is on screen, then exit with code 0
  --delay <seconds>         with --screenshot: keep running this long first
  -h, --help                this text

keys: Ctrl+K command palette (every action by name) · ? all shortcuts ·
      Space play/pause · Left/Right one frame · Shift+Left/Right one beat ·
      Home start · End last frame · L loop · Ctrl+wheel zoom the timeline ·
      Ctrl+Z undo · Ctrl+Shift+Z or Ctrl+Y redo (the history is shared with the agent)

Every edit made in the window is checked, written to the project file and
kept in <project dir>/.edits/history/, like the agent's edits (docs/GUI.md).
";

/// What the command line asked for.
#[derive(Debug)]
pub enum Invocation {
    Run(PathBuf, Options),
    Help,
}

fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once(['x', 'X'])?;
    let (w, h) = (w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?);
    (w >= 320 && h >= 240 && w <= 16384 && h <= 16384).then_some((w, h))
}

/// Parse the arguments after the program name.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Invocation> {
    let mut options = Options::default();
    let mut project: Option<PathBuf> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| anyhow!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(Invocation::Help),
            "--screenshot" => options.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "--at" => {
                let v = value("--at")?;
                options.at = Some(v.parse().map_err(|_| anyhow!("--at takes a frame number, got \"{v}\""))?);
            }
            "--size" => {
                let v = value("--size")?;
                options.size = Some(parse_size(&v).ok_or_else(|| anyhow!("--size takes WIDTHxHEIGHT (at least 320x240), got \"{v}\""))?);
            }
            "--delay" => {
                let v = value("--delay")?;
                options.delay = v.parse::<f64>().ok().filter(|d| d.is_finite() && *d >= 0.0).ok_or_else(|| anyhow!("--delay takes seconds, got \"{v}\""))?;
            }
            "--select" => options.select = Some(value("--select")?),
            "--play" => options.play = true,
            "--scale" => {
                let v = value("--scale")?;
                options.scale = Some(Scale::parse(&v).ok_or_else(|| anyhow!("--scale takes 0.25, 0.5 or 1, got \"{v}\""))?);
            }
            "--cache-mb" => {
                let v = value("--cache-mb")?;
                options.cache_mb = Some(v.parse().map_err(|_| anyhow!("--cache-mb takes a number of megabytes, got \"{v}\""))?);
            }
            "--adapter" => options.adapter = Some(value("--adapter")?.parse()?),
            other if other.starts_with('-') && other.len() > 1 => bail!("unknown option {other}"),
            _ if project.is_none() => project = Some(PathBuf::from(arg)),
            _ => bail!("only one project file can be opened, got a second: {arg}"),
        }
    }
    let project = project.ok_or_else(|| anyhow!("which project? give the path of a project JSON file"))?;
    Ok(Invocation::Run(project, options))
}

/// Open the window on `project` and return when it is closed.
///
/// The project file must exist, but it need not be valid: a file with
/// errors opens a window that lists them and waits for the file to change.
/// In screenshot mode the result is `Ok` only if the picture was written.
pub fn run(project: PathBuf, options: Options) -> Result<()> {
    if !project.is_file() {
        bail!("project file not found: {}", project.display());
    }
    let first = reload::load(&project);
    let outcome: Arc<Mutex<Option<Result<(), String>>>> = Arc::default();
    let (w, h) = options.size.unwrap_or((1360, 860));
    let title = format!("edits · {}", project.display());
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(title)
            .with_app_id("edits-gui")
            .with_inner_size([w as f32, h as f32])
            .with_min_inner_size([640.0, 420.0]),
        ..Default::default()
    };
    let screenshot = options.screenshot.clone();
    let shared = outcome.clone();
    eframe::run_native("edits-gui", native, Box::new(move |cc| Ok(Box::new(app::App::new(cc, project, options, first, shared)))))
        .map_err(|e| anyhow!("{e}"))
        .context("cannot open the window (is there a display? set DISPLAY or WAYLAND_DISPLAY, or use xvfb-run)")?;
    let outcome = outcome.lock().ok().and_then(|mut o| o.take());
    match (outcome, screenshot) {
        (Some(Err(e)), _) => bail!("{e}"),
        (None, Some(path)) => bail!("the window was closed before the screenshot {} was taken", path.display()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Invocation> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_a_plain_run() {
        let Invocation::Run(path, o) = parse(&["demo/project.json"]).unwrap() else { panic!() };
        assert_eq!(path, PathBuf::from("demo/project.json"));
        assert!(o.screenshot.is_none() && o.at.is_none() && !o.play && o.delay == 0.0);
    }

    #[test]
    fn parses_screenshot_mode() {
        let Invocation::Run(path, o) =
            parse(&["--screenshot", "out.png", "p.json", "--at", "37", "--size", "1600x900", "--delay", "2.5", "--select", "cube", "--scale", "0.25", "--play"]).unwrap()
        else {
            panic!()
        };
        assert_eq!(path, PathBuf::from("p.json"));
        assert_eq!(o.screenshot, Some(PathBuf::from("out.png")));
        assert_eq!(o.at, Some(37));
        assert_eq!(o.size, Some((1600, 900)));
        assert_eq!(o.delay, 2.5);
        assert_eq!(o.select.as_deref(), Some("cube"));
        assert_eq!(o.scale, Some(Scale::Quarter));
        assert!(o.play);
    }

    #[test]
    fn rejects_what_it_does_not_understand() {
        assert!(matches!(parse(&["--help"]).unwrap(), Invocation::Help));
        for bad in [
            &[][..],
            &["--at", "3"],
            &["p.json", "--at"],
            &["p.json", "--at", "soon"],
            &["p.json", "--size", "big"],
            &["p.json", "--size", "10x10"],
            &["p.json", "--scale", "0.3"],
            &["p.json", "--delay", "-1"],
            &["p.json", "--frobnicate"],
            &["p.json", "q.json"],
            &["p.json", "--adapter", "quantum"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_project_is_an_error_not_a_window() {
        let e = run(PathBuf::from("/nonexistent/project.json"), Options::default()).unwrap_err();
        assert!(e.to_string().contains("not found"));
    }
}
