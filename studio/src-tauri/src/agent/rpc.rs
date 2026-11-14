//! JSON-RPC 2.0 over a child process's stdin and stdout, one message per
//! line: the transport of the Agent Client Protocol (ACP). The studio is the
//! client; the adapter (`claude-agent-acp`, `codex-acp`, ...) is the agent.
//!
//! Requests are answered by id on a reader thread. Requests and
//! notifications from the agent go to a callback; the client answers a
//! request later with [`Connection::respond`]. Lines that are not JSON (some
//! adapters print a banner) are passed on as [`Inbound::Junk`] and otherwise
//! ignored.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::Duration;

/// A JSON-RPC error (also what a failed request returns).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError {
            code,
            message: message.into(),
            data: None,
        }
    }
    pub fn method_not_found(method: &str) -> Self {
        RpcError::new(-32601, format!("the client does not handle {method}"))
    }
    /// The connection is gone (the agent exited or was stopped).
    pub fn closed(why: &str) -> Self {
        RpcError::new(-32099, why.to_string())
    }
    fn from_value(v: &Value) -> Self {
        RpcError {
            code: v.get("code").and_then(Value::as_i64).unwrap_or(-32603),
            message: v
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("error without a message")
                .to_string(),
            data: v.get("data").cloned().filter(|d| !d.is_null()),
        }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

/// What arrives from the agent, besides answers to our requests.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    /// A line of the agent's stderr.
    Stderr(String),
    /// A stdout line that is not a JSON-RPC message.
    Junk(String),
    /// The process ended (exit code, when it has one).
    Exit(Option<i32>),
}

pub type Handler = Arc<dyn Fn(Inbound) + Send + Sync>;

type Reply = mpsc::Sender<Result<Value, RpcError>>;

pub struct Connection {
    stdin: Mutex<Option<ChildStdin>>,
    pending: Mutex<HashMap<u64, Reply>>,
    next: AtomicU64,
    child: Mutex<Child>,
    alive: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What one line of stdout is.
pub enum Parsed {
    Response {
        id: u64,
        result: Result<Value, RpcError>,
    },
    Inbound(Inbound),
}

/// Classify one line of the agent's stdout.
pub fn parse_line(line: &str) -> Parsed {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Parsed::Inbound(Inbound::Junk(line.to_string()));
    };
    let method = v.get("method").and_then(Value::as_str).map(str::to_string);
    let id = v.get("id").cloned().filter(|i| !i.is_null());
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    match (method, id) {
        (Some(method), Some(id)) => Parsed::Inbound(Inbound::Request { id, method, params }),
        (Some(method), None) => Parsed::Inbound(Inbound::Notification { method, params }),
        (None, Some(id)) => {
            let Some(id) = id.as_u64() else {
                return Parsed::Inbound(Inbound::Junk(line.to_string()));
            };
            let result = match v.get("error") {
                Some(e) if !e.is_null() => Err(RpcError::from_value(e)),
                _ => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
            };
            Parsed::Response { id, result }
        }
        (None, None) => Parsed::Inbound(Inbound::Junk(line.to_string())),
    }
}

impl Connection {
    /// Start `program args` (stdin, stdout and stderr piped; in its own
    /// process group on Unix, so stopping it stops what it started, such as
    /// the `node` under `npx`) and read its output on two threads.
    pub fn spawn(
        program: &str,
        args: &[String],
        cwd: Option<&std::path::Path>,
        env: &[(String, String)],
        handler: Handler,
    ) -> std::io::Result<Arc<Connection>> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let conn = Arc::new(Connection {
            stdin: Mutex::new(stdin),
            pending: Mutex::default(),
            next: AtomicU64::new(0),
            child: Mutex::new(child),
            alive: AtomicBool::new(true),
        });
        let h = handler.clone();
        std::thread::Builder::new()
            .name("acp-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { break };
                    h(Inbound::Stderr(line));
                }
            })?;
        let weak = Arc::downgrade(&conn);
        std::thread::Builder::new()
            .name("acp-stdout".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let line = buf.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match parse_line(line) {
                        Parsed::Response { id, result } => {
                            let Some(c) = weak.upgrade() else { break };
                            let tx = lock(&c.pending).remove(&id);
                            if let Some(tx) = tx {
                                let _ = tx.send(result);
                            }
                        }
                        Parsed::Inbound(i) => handler(i),
                    }
                }
                let code = weak.upgrade().and_then(|c| {
                    c.alive.store(false, Ordering::SeqCst);
                    for (_, tx) in lock(&c.pending).drain() {
                        let _ = tx.send(Err(RpcError::closed("the agent exited")));
                    }
                    let status = lock(&c.child).wait().ok();
                    status.and_then(|s| s.code())
                });
                handler(Inbound::Exit(code));
            })?;
        Ok(conn)
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn write(&self, msg: &Value) -> Result<(), RpcError> {
        let mut line =
            serde_json::to_string(msg).map_err(|e| RpcError::new(-32603, e.to_string()))?;
        line.push('\n');
        let mut g = lock(&self.stdin);
        let stdin = g
            .as_mut()
            .ok_or_else(|| RpcError::closed("the agent is stopped"))?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| RpcError::closed(&format!("cannot write to the agent: {e}")))
    }

    /// Send a request and wait for its answer (at most `timeout`, if given).
    pub fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, RpcError> {
        if !self.alive() {
            return Err(RpcError::closed("the agent is not running"));
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).insert(id, tx);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(e) = self.write(&msg) {
            lock(&self.pending).remove(&id);
            return Err(e);
        }
        let answer = match timeout {
            Some(t) => rx.recv_timeout(t).map_err(|_| {
                lock(&self.pending).remove(&id);
                RpcError::new(
                    -32098,
                    format!(
                        "{method}: no answer from the agent within {} s",
                        t.as_secs()
                    ),
                )
            })?,
            None => rx
                .recv()
                .map_err(|_| RpcError::closed("the agent exited"))?,
        };
        answer
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Answer a request of the agent.
    pub fn respond(&self, id: Value, result: Result<Value, RpcError>) -> Result<(), RpcError> {
        let msg = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        };
        self.write(&msg)
    }

    /// Close stdin (adapters exit on end of input), then stop the process
    /// group if it is still there after a moment.
    pub fn stop(&self) {
        lock(&self.stdin).take();
        let pid = lock(&self.child).id();
        for _ in 0..20 {
            if !self.alive() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        #[cfg(not(unix))]
        {
            let _ = lock(&self.child).kill();
            let _ = pid;
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if self.alive() {
            lock(&self.stdin).take();
            #[cfg(unix)]
            unsafe {
                libc::kill(-(lock(&self.child).id() as i32), libc::SIGTERM);
            }
            #[cfg(not(unix))]
            {
                let _ = lock(&self.child).kill();
            }
        }
    }
}
