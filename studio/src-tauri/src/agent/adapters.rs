//! The agents the panel can start: ACP adapters of Claude Code and Codex
//! (the ones Zed uses), or any command that speaks ACP on stdio.
//!
//! Each adapter is found as: its environment variable (a whole command line),
//! its executable on `PATH`, or `npx -y <package>` when Node is installed.
//! The adapters use the person's own Claude Code or Codex login; the studio
//! never asks for or stores a key.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Adapter {
    /// `claude`, `codex` or `custom`.
    pub id: String,
    pub name: String,
    /// The agent's name in the interface ("Claude", "Codex").
    pub agent: String,
    /// The command line that starts it; empty when it was not found.
    pub command: Vec<String>,
    /// How it was found: `env`, `path`, `npx` or `missing`.
    pub source: String,
    /// The npm package of the adapter.
    pub package: Option<String>,
    /// How to install it, when it was not found.
    pub install: Option<String>,
    /// How to log in, one step per line.
    pub login: Vec<String>,
}

/// Split a command line into words: spaces separate, single and double
/// quotes group, a backslash escapes the next character.
pub fn split_command(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => cur.push(c),
            (_, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    have = true;
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                have = true;
            }
            (None, c) if c.is_whitespace() => {
                if have || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            (None, c) => {
                cur.push(c);
                have = true;
            }
        }
    }
    if have || !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// An executable on `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(&file))
            .find(|c| c.is_file())
    })
}

struct Known {
    id: &'static str,
    name: &'static str,
    agent: &'static str,
    env: &'static str,
    bin: &'static str,
    package: &'static str,
    login: &'static [&'static str],
}

const KNOWN: [Known; 2] = [
    Known {
        id: "claude",
        name: "Claude Code",
        agent: "Claude",
        env: "EDITS_AGENT_CLAUDE",
        bin: "claude-agent-acp",
        package: "@agentclientprotocol/claude-agent-acp",
        login: &[
            "Log in once with Claude Code in a terminal: run `claude`, then type `/login` (a Claude subscription or an Anthropic Console account).",
            "Or run the adapter's own login: `npx -y @agentclientprotocol/claude-agent-acp --cli auth login --claudeai`.",
            "Then press Retry. The adapter uses that login; the studio never sees a key.",
        ],
    },
    Known {
        id: "codex",
        name: "Codex",
        agent: "Codex",
        env: "EDITS_AGENT_CODEX",
        bin: "codex-acp",
        package: "@agentclientprotocol/codex-acp",
        login: &[
            "Log in once with Codex in a terminal: `npx -y @openai/codex login` (or `codex login` when Codex is installed), with ChatGPT.",
            "Or use “Log in with ChatGPT” below, which opens the browser.",
            "Then press Retry.",
        ],
    },
];

/// The adapters, each with the command that starts it here.
pub fn list(custom: Option<&str>) -> Vec<Adapter> {
    let npx = which("npx");
    let mut out: Vec<Adapter> = KNOWN
        .iter()
        .map(|k| {
            let (command, source) = if let Some(line) = std::env::var(k.env)
                .ok()
                .filter(|l| !l.trim().is_empty())
            {
                (split_command(&line), "env")
            } else if let Some(bin) = which(k.bin) {
                (vec![bin.display().to_string()], "path")
            } else if let Some(npx) = &npx {
                (
                    vec![
                        npx.display().to_string(),
                        "-y".into(),
                        format!("{}@latest", k.package),
                    ],
                    "npx",
                )
            } else {
                (Vec::new(), "missing")
            };
            Adapter {
                id: k.id.into(),
                name: k.name.into(),
                agent: k.agent.into(),
                install: (source == "missing").then(|| {
                    format!(
                        "Install Node.js 20 or newer (it brings `npx`), or `npm i -g {}`, or set {} to a command line that starts the adapter.",
                        k.package, k.env
                    )
                }),
                command,
                source: source.into(),
                package: Some(k.package.into()),
                login: k.login.iter().map(|s| s.to_string()).collect(),
            }
        })
        .collect();
    let line = custom
        .map(str::to_string)
        .or_else(|| std::env::var("EDITS_AGENT_CUSTOM").ok())
        .filter(|l| !l.trim().is_empty());
    out.push(Adapter {
        id: "custom".into(),
        name: "Custom ACP command".into(),
        agent: "Agent".into(),
        command: line.as_deref().map(split_command).unwrap_or_default(),
        source: if line.is_some() { "env" } else { "missing" }.into(),
        package: None,
        install: line
            .is_none()
            .then(|| "Type a command line that starts an ACP agent on stdio.".into()),
        login: vec!["Log in the way that agent's documentation says.".into()],
    });
    out
}

/// The `edits-mcp` executable for the agent's session: `EDITS_MCP_BIN`, next
/// to this executable, the workspace's other build folder, or `PATH`.
pub fn edits_mcp() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("EDITS_MCP_BIN").filter(|b| !b.is_empty()) {
        return Some(PathBuf::from(b));
    }
    let file = format!("edits-mcp{}", std::env::consts::EXE_SUFFIX);
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        for candidate in [
            dir.join(&file),
            dir.join("../release").join(&file),
            dir.join("../debug").join(&file),
        ] {
            if candidate.is_file() {
                return candidate.canonicalize().ok().or(Some(candidate));
            }
        }
    }
    which("edits-mcp")
}

/// The skill for agents (`.claude/skills/edits/SKILL.md`): `EDITS_SKILL`, or
/// the repository the executable was built in, or one above the project.
pub fn skill(project_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("EDITS_SKILL").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(s));
    }
    let rel = Path::new(".claude/skills/edits/SKILL.md");
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Some(d) = project_dir {
        starts.push(d.to_path_buf());
    }
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        starts.push(d);
    }
    for start in starts {
        let mut dir = Some(start.as_path());
        while let Some(d) = dir {
            if d.join(rel).is_file() {
                return Some(d.join(rel));
            }
            dir = d.parent();
        }
    }
    None
}

/// Start a terminal emulator running `command` (for an adapter's login).
pub fn open_terminal(command: &[String]) -> Result<String, String> {
    if command.is_empty() {
        return Err("no command to run".into());
    }
    let tries: [(&str, &[&str]); 5] = [
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-x"]),
        ("xterm", &["-e"]),
    ];
    for (term, pre) in tries {
        if which(term).is_some() {
            std::process::Command::new(term)
                .args(pre)
                .args(command)
                .spawn()
                .map_err(|e| format!("cannot start {term}: {e}"))?;
            return Ok(term.into());
        }
    }
    Err(format!(
        "no terminal emulator found: run this in a terminal yourself: {}",
        command.join(" ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_split_like_a_shell() {
        assert_eq!(split_command("npx -y pkg@1"), ["npx", "-y", "pkg@1"]);
        assert_eq!(
            split_command(r#"node "/a b/agent.mjs" --name 'x y' a\ b """#),
            ["node", "/a b/agent.mjs", "--name", "x y", "a b", ""]
        );
        assert!(split_command("   ").is_empty());
    }

    #[test]
    fn every_adapter_says_how_to_log_in() {
        let all = list(Some("node fake.mjs"));
        assert_eq!(
            all.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            ["claude", "codex", "custom"]
        );
        assert!(all.iter().all(|a| !a.login.is_empty()));
        let custom = all.last().unwrap();
        assert_eq!(custom.command, ["node", "fake.mjs"]);
        for a in &all[..2] {
            assert_eq!(a.command.is_empty(), a.source == "missing");
        }
    }
}
