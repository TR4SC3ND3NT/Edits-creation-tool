//! `edits gui` and `edits mcp`: start the preview window or the MCP server.
//!
//! They are separate executables (`edits-gui`, `edits-mcp`) so that the main
//! binary does not pull in windowing, audio or async runtimes. These
//! subcommands find them next to this executable or on `PATH` and hand over
//! the arguments, the standard streams and the exit code.

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
