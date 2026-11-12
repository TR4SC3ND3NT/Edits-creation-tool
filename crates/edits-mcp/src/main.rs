//! `edits-mcp`: the MCP server for the edits engine, on stdin and stdout.

use edits_mcp::Config;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "edits-mcp: MCP server (stdio) for the edits engine

usage: edits-mcp [--root DIR] [--edits PATH] [--edits-analyze PATH]
       edits-mcp --tools        print the tools it serves, as JSON, and exit

  --root DIR             workspace root; every path a tool receives must be inside it
                         [env: EDITS_ROOT] [default: the working directory]
  --edits PATH           the `edits` executable [env: EDITS_BIN]
                         [default: next to this executable, then PATH]
  --edits-analyze PATH   the `edits-analyze` executable [env: EDITS_ANALYZE_BIN]
  -h, --help             this text
  -V, --version          the version

It speaks MCP on stdin and stdout; start it from an MCP client (see docs/AGENTS.md).";

fn run() -> anyhow::Result<()> {
    let mut config = Config::from_env();
    let mut list_tools = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().map(PathBuf::from).ok_or_else(|| anyhow::anyhow!("{name} needs a value"));
        match arg.as_str() {
            "--root" => config.root = Some(value("--root")?),
            "--edits" => config.edits_bin = Some(value("--edits")?),
            "--edits-analyze" => config.analyze_bin = Some(value("--edits-analyze")?),
            "--tools" => list_tools = true,
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("edits-mcp {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => anyhow::bail!("unknown argument {other:?} (try --help)"),
        }
    }
    if list_tools {
        let server = edits_mcp::EditsServer::new(config)?;
        println!("{}", serde_json::to_string_pretty(&server.tools())?);
        return Ok(());
    }
    edits_mcp::serve_stdio(config)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
