//! MCP server for the edits engine.
//!
//! A thin adapter: every tool runs the `edits` or `edits-analyze` executable
//! with `--json` (or edits the project file directly, for `project_edit`) and
//! returns the result. The server keeps no project state; the project path,
//! the analysis path and the job id are arguments of each call, which is what
//! MCP revision 2026-07-28 (stateless, no sessions) expects. Background jobs
//! keep their state in files under `<root>/.edits/jobs/`.
//!
//! ```no_run
//! // What the `edits-mcp` binary does, and what an `edits mcp` subcommand can do:
//! edits_mcp::serve_stdio(edits_mcp::Config::from_env()).unwrap();
//! ```
//!
//! Environment: `EDITS_ROOT` (workspace root; default the working directory),
//! `EDITS_BIN` and `EDITS_ANALYZE_BIN` (the executables; default: next to the
//! running executable, then `PATH`), `EDITS_MCP_TIMEOUT` (seconds one
//! foreground command may run; default 50).

pub mod activity;
pub mod edit;
pub mod exec;
pub mod images;
pub mod jobs;
pub mod paths;
pub mod server;
pub mod summarize;

// Moved to `edits_core` (shared with the window); kept at their old paths.
pub use edits_core::{jsonfmt, pointer};

use std::path::PathBuf;
use std::time::Duration;

pub use server::EditsServer;

/// Seconds a foreground command may run. Below the 60 s default tool timeout
/// of Codex, so the server answers (with advice) before the client gives up.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(50);

/// How the server finds its files and executables. `Default` leaves
/// everything to the environment variables and the built-in search.
#[derive(Clone, Debug)]
pub struct Config {
    /// Workspace root: every path argument must resolve inside it.
    /// `None`: the working directory.
    pub root: Option<PathBuf>,
    /// The `edits` executable. `None`: `EDITS_BIN`, then next to the running
    /// executable, then `PATH`.
    pub edits_bin: Option<PathBuf>,
    /// The `edits-analyze` executable. `None`: `EDITS_ANALYZE_BIN`, then next
    /// to the running executable, then `PATH`.
    pub analyze_bin: Option<PathBuf>,
    /// Longest a foreground command (a frame, a sheet, a probe) may run.
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self { root: None, edits_bin: None, analyze_bin: None, timeout: DEFAULT_TIMEOUT }
    }
}

impl Config {
    /// Configuration from `EDITS_ROOT` and `EDITS_MCP_TIMEOUT`. The
    /// executables' variables are read when a tool needs them.
    pub fn from_env() -> Self {
        let root = std::env::var_os("EDITS_ROOT").filter(|v| !v.is_empty()).map(PathBuf::from);
        let timeout = std::env::var("EDITS_MCP_TIMEOUT").ok().and_then(|v| v.trim().parse::<u64>().ok()).filter(|s| *s > 0).map(Duration::from_secs).unwrap_or(DEFAULT_TIMEOUT);
        Self { root, timeout, ..Self::default() }
    }
}

/// Serve MCP on stdin and stdout until the client closes the connection.
/// Blocks; builds its own runtime, so it can be called from a plain `main`.
/// Nothing but protocol messages is written to stdout.
pub fn serve_stdio(config: Config) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    runtime.block_on(serve_stdio_async(config))
}

/// The async form of [`serve_stdio`], for callers that already run tokio.
pub async fn serve_stdio_async(config: Config) -> anyhow::Result<()> {
    use rmcp::ServiceExt;
    let server = EditsServer::new(config)?;
    let running = match server.serve(rmcp::transport::stdio()).await {
        Ok(running) => running,
        // A client that closes the connection before its first request is not an error.
        Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    running.waiting().await?;
    Ok(())
}
