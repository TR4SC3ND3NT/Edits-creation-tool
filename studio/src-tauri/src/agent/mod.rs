//! The agent panel's backend: an Agent Client Protocol (ACP) client.
//!
//! The studio starts an ACP adapter (Claude Code's `claude-agent-acp`,
//! `codex-acp`, or any ACP command; `adapters`) as a child process and
//! speaks JSON-RPC with it on stdio (`rpc`). It passes the `edits` MCP server
//! (`edits-mcp --root <project folder>`) to every session, forwards the
//! session's updates to the page as `agent` events (with where each edits
//! tool call points, from `edits_core::activity`), asks the person about
//! permission requests, and follows `.edits/activity.jsonl` for agents that
//! work outside the studio (`activity`). The Tauri commands are in
//! `commands`; this module has no Tauri dependency, so it is tested with a
//! fake agent process (`tests`).

pub mod activity;
pub mod adapters;
pub mod commands;
pub mod rpc;
#[cfg(test)]
mod tests;

use adapters::Adapter;
use rpc::{Connection, Inbound, RpcError};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

/// The ACP version the studio speaks.
pub const PROTOCOL_VERSION: u64 = 1;
/// ACP's error code for "log in first".
pub const AUTH_REQUIRED: i64 = -32000;
/// The MCP server's name in the agent's session: its tools are
/// `mcp__edits__project_edit` in Claude Code, `edits.project_edit` in Codex.
pub const MCP_NAME: &str = "edits";

pub type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;
/// The open project's file, if any.
pub type PathFn = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;
/// The open project's document, if any.
pub type DocFn = Arc<dyn Fn() -> Option<Value> + Send + Sync>;

/// What a failed agent command tells the page.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AgentError {
    pub message: String,
    pub code: Option<i64>,
    /// The adapter needs a login first (ACP error -32000).
    pub auth_required: bool,
    pub data: Option<Value>,
    /// The last lines the adapter wrote to stderr.
    pub stderr: Vec<String>,
}

impl AgentError {
    pub fn msg(message: impl Into<String>) -> Self {
        AgentError {
            message: message.into(),
            code: None,
            auth_required: false,
            data: None,
            stderr: Vec::new(),
        }
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Default)]
struct Inner {
    adapter: Option<Adapter>,
    init: Option<Value>,
    stderr: VecDeque<String>,
    /// Permission requests waiting for the person: key -> (request id, session).
    permissions: HashMap<String, (Value, String)>,
}

pub struct AgentHost {
    emit: Emit,
    project: PathFn,
    doc: DocFn,
    conn: Mutex<Option<Arc<Connection>>>,
    inner: Arc<Mutex<Inner>>,
    generation: AtomicU64,
    serial: Arc<AtomicU64>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The edits tool a tool call of the agent runs, from the forms adapters
/// report it in: `_meta.claudeCode.toolName` (`mcp__edits__project_edit`),
/// the call's `name` or `title` (`edits.project_edit`, `Tool: edits/...`),
/// or `rawInput.server` + `rawInput.tool` (Codex).
pub fn edits_tool(update: &Value) -> Option<(String, Value)> {
    let raw = update.get("rawInput").cloned().unwrap_or(Value::Null);
    let mut names: Vec<String> = Vec::new();
    for p in ["/_meta/claudeCode/toolName", "/name", "/title"] {
        if let Some(s) = update.pointer(p).and_then(Value::as_str) {
            names.push(s.to_string());
        }
    }
    let nested = raw.get("tool").and_then(Value::as_str).is_some()
        && raw
            .get("server")
            .and_then(Value::as_str)
            .is_none_or(|s| s == MCP_NAME);
    if nested {
        names.insert(0, format!("edits/{}", raw["tool"].as_str().unwrap_or("")));
    }
    let tool = names.iter().find_map(|n| find_tool(n))?;
    let args = if nested {
        raw.get("arguments").cloned().unwrap_or(Value::Null)
    } else {
        raw
    };
    Some((tool, args))
}

fn find_tool(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(t) = edits_core::activity::tool_name(text) {
        if edits_core::activity::TOOLS.contains(&t) {
            return Some(t.to_string());
        }
    }
    for pat in ["mcp__edits__", "edits.", "edits/", "edits__"] {
        if let Some(i) = text.find(pat) {
            let rest = &text[i + pat.len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if edits_core::activity::TOOLS.contains(&name.as_str()) {
                return Some(name);
            }
        }
    }
    None
}

/// Where a tool call of the agent points, for the presence highlights:
/// `{tool, label, layers, pointers, time, t, frame}`, or `None` for a call
/// that is not an edits tool or carries no arguments yet.
pub fn presence_of(update: &Value, doc: Option<&Value>) -> Option<Value> {
    let (tool, args) = edits_tool(update)?;
    if args.is_null() && update.get("sessionUpdate").and_then(Value::as_str) != Some("tool_call") {
        return None;
    }
    let mut t = edits_core::activity::target(&tool, &args);
    if let Some(doc) = doc {
        edits_core::activity::resolve(&mut t, doc);
    }
    Some(json!({
        "tool": tool,
        "label": t.label,
        "layers": t.layers,
        "pointers": t.pointers,
        "time": t.time,
        "t": t.t,
        "frame": t.frame,
        "args": if tool == "project_edit" { args.get("ops").cloned().unwrap_or(Value::Null) } else { Value::Null },
    }))
}

impl AgentHost {
    pub fn new(emit: Emit, project: PathFn, doc: DocFn) -> Arc<Self> {
        Arc::new(AgentHost {
            emit,
            project,
            doc,
            conn: Mutex::new(None),
            inner: Arc::default(),
            generation: AtomicU64::new(0),
            serial: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Follow `.edits/activity.jsonl` of the open project on a thread, as
    /// `agent` events `{type: "activity", entries, replay}`.
    pub fn watch_activity(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let _ = std::thread::Builder::new()
            .name("edits-activity".into())
            .spawn(move || {
                let mut tail = activity::LogTail::default();
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    let Some(host) = weak.upgrade() else { break };
                    let Some(project) = (host.project)() else {
                        continue;
                    };
                    let log = edits_core::activity::path(&project);
                    let (entries, replay) = tail.poll(&log, 40);
                    if !entries.is_empty() {
                        (host.emit)(
                            "agent",
                            json!({ "type": "activity", "entries": entries, "replay": replay }),
                        );
                    }
                }
            });
    }

    fn conn(&self) -> Result<Arc<Connection>, AgentError> {
        lock(&self.conn)
            .clone()
            .filter(|c| c.alive())
            .ok_or_else(|| AgentError::msg("no agent is running: choose one and press Connect"))
    }

    fn fail(&self, e: RpcError) -> AgentError {
        AgentError {
            auth_required: e.code == AUTH_REQUIRED,
            message: e.message,
            code: Some(e.code),
            data: e.data,
            stderr: lock(&self.inner).stderr.iter().cloned().collect(),
        }
    }

    fn project_dir(&self) -> Option<PathBuf> {
        (self.project)().map(|p| edits_core::edit::project_dir(&p))
    }

    /// Start the adapter `id` (`claude`, `codex`, or `custom` with
    /// `command`) and run the ACP handshake. Returns the adapter and the
    /// agent's `initialize` answer (its capabilities and login methods).
    pub fn connect(&self, id: &str, command: Option<&str>) -> Result<Value, AgentError> {
        let adapter = adapters::list(command)
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| AgentError::msg(format!("unknown agent {id}")))?;
        if adapter.command.is_empty() {
            return Err(AgentError::msg(
                adapter
                    .install
                    .clone()
                    .unwrap_or_else(|| format!("{} is not installed", adapter.name)),
            ));
        }
        let (program, args) = (adapter.command[0].clone(), adapter.command[1..].to_vec());
        self.connect_command(adapter, &program, &args, &[])
    }

    /// [`connect`] with an explicit command line (tests use it with a fake agent).
    pub fn connect_command(
        &self,
        adapter: Adapter,
        program: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<Value, AgentError> {
        self.disconnect();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut g = lock(&self.inner);
            *g = Inner::default();
            g.adapter = Some(adapter.clone());
        }
        let slot: Arc<Mutex<Weak<Connection>>> = Arc::new(Mutex::new(Weak::new()));
        let handler = self.handler(generation, slot.clone());
        let cwd = self
            .project_dir()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        let conn = Connection::spawn(program, args, cwd.as_deref(), env, handler).map_err(|e| {
            AgentError::msg(format!(
                "cannot start {} ({program}): {e}. {}",
                adapter.name,
                adapter.install.clone().unwrap_or_default()
            ))
        })?;
        *lock(&slot) = Arc::downgrade(&conn);
        *lock(&self.conn) = Some(conn.clone());
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": {
                "fs": { "readTextFile": false, "writeTextFile": false },
                "terminal": false,
                "auth": { "terminal": true },
                "session": { "configOptions": { "boolean": {} }, "notices": {} },
            },
            "clientInfo": { "name": "edits-studio", "title": "Edits Studio", "version": env!("CARGO_PKG_VERSION") },
        });
        // The first `npx` run downloads the adapter: allow it time.
        let init = conn
            .request("initialize", params, Some(Duration::from_secs(300)))
            .map_err(|e| {
                let mut err = self.fail(e);
                if !conn.alive() {
                    err.message = format!(
                        "{} stopped during the handshake: {}",
                        adapter.name,
                        err.stderr.last().cloned().unwrap_or(err.message.clone())
                    );
                }
                err
            })?;
        lock(&self.inner).init = Some(init.clone());
        Ok(json!({ "adapter": adapter, "init": init, "generation": generation }))
    }

    fn handler(&self, generation: u64, slot: Arc<Mutex<Weak<Connection>>>) -> rpc::Handler {
        let emit = self.emit.clone();
        let inner = self.inner.clone();
        let doc = self.doc.clone();
        let serial = self.serial.clone();
        Arc::new(move |msg: Inbound| match msg {
            Inbound::Notification { method, params } if method == "session/update" => {
                let update = params.get("update").cloned().unwrap_or(Value::Null);
                let kind = update
                    .get("sessionUpdate")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let presence = if matches!(kind, "tool_call" | "tool_call_update") {
                    presence_of(&update, doc().as_ref())
                } else {
                    None
                };
                emit(
                    "agent",
                    json!({
                        "type": "update",
                        "generation": generation,
                        "sessionId": params.get("sessionId"),
                        "update": update,
                        "presence": presence,
                    }),
                );
            }
            Inbound::Notification { method, params } => emit(
                "agent",
                json!({ "type": "notification", "generation": generation, "method": method, "params": params }),
            ),
            Inbound::Request { id, method, params } if method == "session/request_permission" => {
                let key = format!("p{}", serial.fetch_add(1, Ordering::SeqCst));
                let session = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                lock(&inner)
                    .permissions
                    .insert(key.clone(), (id, session.clone()));
                let tool = params.get("toolCall").cloned().unwrap_or(Value::Null);
                let presence = presence_of(&tool, doc().as_ref());
                emit(
                    "agent",
                    json!({
                        "type": "permission",
                        "generation": generation,
                        "key": key,
                        "sessionId": session,
                        "toolCall": tool,
                        "options": params.get("options").cloned().unwrap_or(json!([])),
                        "presence": presence,
                    }),
                );
            }
            Inbound::Request { id, method, .. } => {
                if let Some(c) = lock(&slot).upgrade() {
                    let _ = c.respond(id, Err(RpcError::method_not_found(&method)));
                }
            }
            Inbound::Stderr(line) => {
                log::debug!("agent: {line}");
                let mut g = lock(&inner);
                g.stderr.push_back(line);
                while g.stderr.len() > 60 {
                    g.stderr.pop_front();
                }
            }
            Inbound::Junk(line) => log::debug!("agent stdout (not JSON-RPC): {line}"),
            Inbound::Exit(code) => {
                let stderr: Vec<String> = {
                    let mut g = lock(&inner);
                    g.permissions.clear();
                    g.stderr.iter().rev().take(12).rev().cloned().collect()
                };
                emit(
                    "agent",
                    json!({ "type": "exit", "generation": generation, "code": code, "stderr": stderr }),
                );
            }
        })
    }

    pub fn disconnect(&self) {
        let old = lock(&self.conn).take();
        if let Some(c) = old {
            c.stop();
        }
    }

    fn init(&self) -> Value {
        lock(&self.inner).init.clone().unwrap_or(Value::Null)
    }

    /// The `edits` MCP server for a session in `dir`, as ACP describes a stdio server.
    pub fn mcp_server(dir: &Path) -> Option<Value> {
        let bin = adapters::edits_mcp()?;
        let mut args = vec!["--root".to_string(), dir.display().to_string()];
        if let Some(edits) = crate::renders::locate("edits") {
            args.push("--edits".into());
            args.push(edits.display().to_string());
        }
        if let Some(analyze) = crate::renders::locate("edits-analyze") {
            args.push("--edits-analyze".into());
            args.push(analyze.display().to_string());
        }
        Some(json!({
            "name": MCP_NAME,
            "command": bin.display().to_string(),
            "args": args,
            "env": [{ "name": "EDITS_ACTIVITY_VIA", "value": "studio" }],
        }))
    }

    /// What the agent is told about where it is, once per thread.
    pub fn context_note(project: &Path, skill: Option<&Path>) -> String {
        let file = project
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project.json".into());
        let mut s = format!(
            "You are working inside Edits Studio, the desktop app of the edits engine, on the project `{file}` in `{}` (your working directory). \
The person watches the project live in the studio while you work: every change you make through the `edits` MCP tools shows up at once, with where you are working highlighted. \
Use the `edits` MCP tools with project \"{file}\": project_describe first, then project_edit / preset_apply / beats_cut, and look at render_frame or render_sheet images. \
`ui_context` tells you which layer, property and frame the person has selected.",
            edits_core::edit::project_dir(project).display()
        );
        if let Some(skill) = skill {
            s.push_str(&format!(
                " The skill for this engine is `{}`: read it before your first edit.",
                skill.display()
            ));
        }
        s
    }

    fn session_params(&self, extra: Value) -> Result<(Value, String, bool), AgentError> {
        let project = (self.project)().ok_or_else(|| {
            AgentError::msg("open a project first: the agent works in its folder")
        })?;
        let dir = edits_core::edit::project_dir(&project);
        let skill = adapters::skill(Some(&dir));
        let caps = self
            .init()
            .pointer("/agentCapabilities/sessionCapabilities")
            .cloned()
            .unwrap_or(Value::Null);
        let mut p = json!({
            "cwd": dir.display().to_string(),
            "mcpServers": Self::mcp_server(&dir).into_iter().collect::<Vec<_>>(),
        });
        let note = Self::context_note(&project, skill.as_deref());
        let mut told = false;
        if caps.get("systemPrompt").is_some_and(|v| !v.is_null()) {
            p["systemPrompt"] =
                json!({ "mode": "append", "content": [{ "type": "text", "text": note }] });
            told = true;
        }
        if caps
            .get("additionalDirectories")
            .is_some_and(|v| !v.is_null())
        {
            if let Some(root) = skill.as_ref().and_then(|s| s.ancestors().nth(4)) {
                if !dir.starts_with(root) {
                    p["additionalDirectories"] = json!([root.display().to_string()]);
                }
            }
        }
        if let (Value::Object(m), Value::Object(e)) = (&mut p, extra) {
            m.extend(e);
        }
        Ok((p, note, told))
    }

    /// `session/new` in the project's folder with the `edits` MCP server.
    /// The answer gets `studio: {note, told, mcp}`: the note for the agent,
    /// whether it went in as a system prompt (if not, the page sends it with
    /// the first message) and whether the MCP server was found.
    pub fn session_new(&self) -> Result<Value, AgentError> {
        let conn = self.conn()?;
        let (params, note, told) = self.session_params(json!({}))?;
        let mcp = params["mcpServers"]
            .as_array()
            .is_some_and(|a| !a.is_empty());
        let mut r = conn
            .request("session/new", params, Some(Duration::from_secs(180)))
            .map_err(|e| self.fail(e))?;
        r["studio"] = json!({ "note": note, "told": told, "mcp": mcp });
        Ok(r)
    }

    /// Open an earlier thread: `session/load` (the agent replays it as
    /// updates) or `session/resume`, whichever the agent offers.
    pub fn session_load(&self, session: &str) -> Result<Value, AgentError> {
        let conn = self.conn()?;
        let init = self.init();
        let method = if init.pointer("/agentCapabilities/loadSession") == Some(&Value::Bool(true)) {
            "session/load"
        } else if init
            .pointer("/agentCapabilities/sessionCapabilities/resume")
            .is_some_and(|v| !v.is_null())
        {
            "session/resume"
        } else {
            return Err(AgentError::msg("this agent cannot reopen earlier threads"));
        };
        let (params, note, told) = self.session_params(json!({ "sessionId": session }))?;
        let mut r = conn
            .request(method, params, Some(Duration::from_secs(180)))
            .map_err(|e| self.fail(e))?;
        if !r.is_object() {
            r = json!({});
        }
        r["sessionId"] = json!(session);
        r["studio"] = json!({ "note": note, "told": told, "mcp": true });
        Ok(r)
    }

    /// The agent's earlier threads in the project's folder (`session/list`).
    pub fn session_list(&self) -> Result<Value, AgentError> {
        let conn = self.conn()?;
        if self
            .init()
            .pointer("/agentCapabilities/sessionCapabilities/list")
            .is_none_or(Value::is_null)
        {
            return Ok(json!({ "sessions": [], "supported": false }));
        }
        let cwd = self.project_dir().map(|d| d.display().to_string());
        let mut r = conn
            .request(
                "session/list",
                json!({ "cwd": cwd }),
                Some(Duration::from_secs(30)),
            )
            .map_err(|e| self.fail(e))?;
        r["supported"] = json!(true);
        Ok(r)
    }

    /// One turn: returns when the agent has finished (`stopReason`).
    pub fn prompt(&self, session: &str, prompt: Value) -> Result<Value, AgentError> {
        let conn = self.conn()?;
        conn.request(
            "session/prompt",
            json!({ "sessionId": session, "prompt": prompt }),
            None,
        )
        .map_err(|e| self.fail(e))
    }

    /// Stop the turn: `session/cancel`, and every permission question of the
    /// session answered "cancelled", as ACP asks.
    pub fn cancel(&self, session: &str) -> Result<(), AgentError> {
        let conn = self.conn()?;
        let waiting: Vec<(String, Value)> = {
            let mut g = lock(&self.inner);
            let keys: Vec<String> = g
                .permissions
                .iter()
                .filter(|(_, (_, s))| s == session)
                .map(|(k, _)| k.clone())
                .collect();
            keys.into_iter()
                .filter_map(|k| g.permissions.remove(&k).map(|(id, _)| (k, id)))
                .collect()
        };
        for (key, id) in waiting {
            let _ = conn.respond(id, Ok(json!({ "outcome": { "outcome": "cancelled" } })));
            (self.emit)("agent", json!({ "type": "permission_done", "key": key }));
        }
        conn.notify("session/cancel", json!({ "sessionId": session }))
            .map_err(|e| self.fail(e))
    }

    /// Answer a permission question: an option id, or `None` for cancelled.
    pub fn permission(&self, key: &str, option: Option<&str>) -> Result<(), AgentError> {
        let conn = self.conn()?;
        let (id, _) = lock(&self.inner)
            .permissions
            .remove(key)
            .ok_or_else(|| AgentError::msg("that question was already answered"))?;
        let outcome = match option {
            Some(o) => json!({ "outcome": { "outcome": "selected", "optionId": o } }),
            None => json!({ "outcome": { "outcome": "cancelled" } }),
        };
        conn.respond(id, Ok(outcome)).map_err(|e| self.fail(e))?;
        (self.emit)("agent", json!({ "type": "permission_done", "key": key }));
        Ok(())
    }

    pub fn set_mode(&self, session: &str, mode: &str) -> Result<Value, AgentError> {
        self.conn()?
            .request(
                "session/set_mode",
                json!({ "sessionId": session, "modeId": mode }),
                Some(Duration::from_secs(30)),
            )
            .map_err(|e| self.fail(e))
    }

    pub fn set_model(&self, session: &str, model: &str) -> Result<Value, AgentError> {
        self.conn()?
            .request(
                "session/set_model",
                json!({ "sessionId": session, "modelId": model }),
                Some(Duration::from_secs(30)),
            )
            .map_err(|e| self.fail(e))
    }

    /// `session/set_config_option` (model, effort, fast mode, ...). A boolean
    /// value is sent with `type: "boolean"`.
    pub fn set_config(
        &self,
        session: &str,
        config: &str,
        value: Value,
    ) -> Result<Value, AgentError> {
        let mut p = json!({ "sessionId": session, "configId": config, "value": value });
        if value.is_boolean() {
            p["type"] = json!("boolean");
        }
        self.conn()?
            .request(
                "session/set_config_option",
                p,
                Some(Duration::from_secs(30)),
            )
            .map_err(|e| self.fail(e))
    }

    /// Log in with one of the agent's own methods (it may open a browser).
    pub fn authenticate(&self, method: &str) -> Result<Value, AgentError> {
        self.conn()?
            .request(
                "authenticate",
                json!({ "methodId": method }),
                Some(Duration::from_secs(600)),
            )
            .map_err(|e| self.fail(e))
    }

    /// The command line of a terminal login method of the running adapter.
    pub fn login_command(&self, method: &str) -> Result<Vec<String>, AgentError> {
        let g = lock(&self.inner);
        let adapter = g
            .adapter
            .clone()
            .ok_or_else(|| AgentError::msg("no agent is running"))?;
        let m = g
            .init
            .as_ref()
            .and_then(|i| i.get("authMethods"))
            .and_then(Value::as_array)
            .and_then(|ms| {
                ms.iter()
                    .find(|m| m.get("id").and_then(Value::as_str) == Some(method))
                    .cloned()
            })
            .ok_or_else(|| AgentError::msg(format!("the agent has no login method {method}")))?;
        let mut cmd = adapter.command.clone();
        if let Some(meta) = m.pointer("/_meta/terminal-auth") {
            let mut c: Vec<String> = meta
                .get("command")
                .and_then(Value::as_str)
                .map(|s| vec![s.to_string()])
                .unwrap_or_default();
            c.extend(
                meta.get("args")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string),
            );
            if !c.is_empty() {
                return Ok(c);
            }
        }
        cmd.extend(
            m.get("args")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string),
        );
        Ok(cmd)
    }

    /// What the panel shows about the setup: the project folder, the MCP
    /// server and the skill.
    pub fn setup(&self) -> Value {
        let dir = self.project_dir();
        json!({
            "projectDir": dir.as_ref().map(|d| d.display().to_string()),
            "mcp": adapters::edits_mcp().map(|p| p.display().to_string()),
            "skill": adapters::skill(dir.as_deref()).map(|p| p.display().to_string()),
            "running": lock(&self.conn).as_ref().is_some_and(|c| c.alive()),
        })
    }

    /// Files in the project's folder for `@` mentions (relative paths; no
    /// hidden folders; at most 400).
    pub fn files(&self) -> Vec<String> {
        let Some(dir) = self.project_dir() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut stack = vec![(dir.clone(), 0usize)];
        while let Some((d, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            let mut entries: Vec<_> = rd.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                let p = e.path();
                if p.is_dir() {
                    if depth < 3 {
                        stack.push((p, depth + 1));
                    }
                } else if let Ok(rel) = p.strip_prefix(&dir) {
                    out.push(rel.display().to_string());
                    if out.len() >= 400 {
                        return out;
                    }
                }
            }
        }
        out.sort();
        out
    }
}

impl Drop for AgentHost {
    fn drop(&mut self) {
        self.disconnect();
    }
}
