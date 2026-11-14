//! `edits gui` and `edits mcp`: start the desktop app or the MCP server.
//!
//! They are separate executables (`edits-studio` or `edits-gui`, and
//! `edits-mcp`) so that the main binary does not pull in windowing, audio or
//! async runtimes. These subcommands find them next to this executable or on
//! `PATH` and hand over the arguments, the standard streams and the exit code.
//!
//! `edits gui` starts Edits Studio (`studio/`, Tauri) when it is built, and
//! the older egui window (`crates/edits-gui`) otherwise, or when asked for
//! with `EDITS_GUI=egui` or an option only that window has (`--screenshot`...).

use anyhow::{bail, Result};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// Where `name` would be found: an environment override, then the folder of
/// this executable, then `PATH`.
pub fn locate(name: &str, env_override: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(env_override).map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
        let p = dir.join(&file);
        if p.is_file() {
            return Some(p);
        }
    }
    std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|d| d.join(&file)).find(|p| p.is_file()))
}

/// Options only the egui window has: with one of them, `edits gui` starts it.
const EGUI_ONLY: [&str; 7] = ["--screenshot", "--delay", "--size", "--scale", "--play", "--cache-mb", "--adapter"];

/// Which window `edits gui` starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// Edits Studio (`edits-studio`, the Tauri app in `studio/`).
    Studio,
    /// The egui window (`edits-gui`).
    Egui,
}

/// The studio when it is built, unless the egui window is asked for
/// (`EDITS_GUI=egui`, or an option only it has) or the studio is missing.
pub fn choose_window(args: &[OsString], studio_built: bool, egui_built: bool, env: Option<&str>) -> Window {
    let wants_egui = env.is_some_and(|e| e.eq_ignore_ascii_case("egui")) || args.iter().any(|a| a.to_str().is_some_and(|a| EGUI_ONLY.contains(&a)));
    let wants_studio = env.is_some_and(|e| e.eq_ignore_ascii_case("studio"));
    match (wants_studio, wants_egui) {
        (true, _) => Window::Studio,
        (false, true) => Window::Egui,
        _ if studio_built || !egui_built => Window::Studio,
        _ => Window::Egui,
    }
}

/// `edits gui`: start Edits Studio, or the egui window (see [`choose_window`]).
pub fn gui(args: &[OsString]) -> Result<ExitCode> {
    let studio = locate("edits-studio", "EDITS_STUDIO_BIN");
    let egui = locate("edits-gui", "EDITS_GUI_BIN");
    let env = std::env::var("EDITS_GUI").ok();
    match choose_window(args, studio.is_some(), egui.is_some(), env.as_deref()) {
        Window::Egui => exec("edits-gui", "EDITS_GUI_BIN", "edits-gui", args),
        Window::Studio if studio.is_none() => bail!(
            "neither Edits Studio nor the egui window is built: build the studio with `cd studio && bun install && bun run tauri build --no-bundle` \
             (it lands in target/release next to `edits`; docs/GUI.md lists the system packages), or the older window with \
             `cargo build --release -p edits-gui`; or set EDITS_STUDIO_BIN or EDITS_GUI_BIN to one"
        ),
        Window::Studio => exec("edits-studio", "EDITS_STUDIO_BIN", "edits-studio", args),
    }
}

/// Run `name` with `args`, inheriting stdin, stdout and stderr.
pub fn exec(name: &str, env_override: &str, crate_name: &str, args: &[OsString]) -> Result<ExitCode> {
    let Some(bin) = locate(name, env_override) else {
        bail!(
            "`{name}` was not found next to `edits` or on PATH; build it with `cargo build --release -p {crate_name}` \
             (it lands in target/release next to `edits`), or set {env_override} to its path"
        );
    };
    let mut cmd = Command::new(&bin);
    cmd.args(args);
    // The MCP server runs `edits` itself; point it at this very binary.
    if name == "edits-mcp" && std::env::var_os("EDITS_BIN").is_none() {
        if let Ok(me) = std::env::current_exe() {
            cmd.env("EDITS_BIN", me);
        }
    }
    let status = cmd.status().map_err(|e| anyhow::anyhow!("could not start {}: {e}", bin.display()))?;
    Ok(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        Some(c) => ExitCode::from(c.clamp(1, 255) as u8),
        None => ExitCode::FAILURE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_studio_is_started_when_it_is_built() {
        let p = args(&["demo/project.json"]);
        assert_eq!(choose_window(&p, true, true, None), Window::Studio);
        assert_eq!(choose_window(&p, false, true, None), Window::Egui, "falls back to the old window");
        assert_eq!(choose_window(&p, false, false, None), Window::Studio, "the error then names the studio");
        assert_eq!(choose_window(&p, true, true, Some("egui")), Window::Egui);
        assert_eq!(choose_window(&args(&["p.json", "--screenshot", "s.png"]), true, true, None), Window::Egui, "an option only the old window has");
        assert_eq!(choose_window(&args(&["p.json", "--page", "renders"]), true, true, None), Window::Studio);
        assert_eq!(choose_window(&args(&["p.json", "--screenshot", "s.png"]), true, true, Some("studio")), Window::Studio);
    }
}
