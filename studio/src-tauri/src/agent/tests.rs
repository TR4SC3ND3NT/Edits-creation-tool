//! The ACP client against a fake agent: this test binary started again with
//! `EDITS_FAKE_ACP=1` runs `fake_acp_agent_process`, which speaks ACP on
//! stdio the way the adapters do (a banner line first, session updates, a
//! permission request in the middle of a turn, `-32000` for "log in first").

use super::*;
use serde_json::json;
use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::time::Instant;

fn out(v: Value) {
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "{v}");
    let _ = o.flush();
}

fn update(session: &str, u: Value) {
    out(
        json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": session, "update": u } }),
    );
}

/// The fake agent's main loop.
fn fake_agent() {
    out(json!("this banner is not JSON-RPC")); // a JSON string, not a message: skipped
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    while let Some(Ok(line)) = lines.next() {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = msg.get("id").cloned();
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let reply = |result: Value| out(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
        match method.as_str() {
            "initialize" => reply(json!({
                "protocolVersion": 1,
                "agentInfo": { "name": "fake-agent", "version": "1.0.0" },
                "agentCapabilities": { "loadSession": true, "promptCapabilities": { "image": true, "embeddedContext": true }, "sessionCapabilities": { "list": {} } },
                "authMethods": [{ "id": "fake-login", "name": "Log in", "type": "terminal", "args": ["--login"] }],
                "echo": params,
            })),
            "session/new" => {
                if params["cwd"].as_str().unwrap_or("").contains("needs-login") {
                    out(
                        json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": "Authentication required" } }),
                    );
                    continue;
                }
                reply(json!({
                    "sessionId": "s1",
                    "modes": { "currentModeId": "default", "availableModes": [{ "id": "default", "name": "Default" }, { "id": "plan", "name": "Plan" }] },
                    "configOptions": [{ "id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "opus", "options": [{ "value": "opus", "name": "Opus" }] }],
                    "echo": params,
                }));
                update(
                    "s1",
                    json!({ "sessionUpdate": "available_commands_update", "availableCommands": [{ "name": "review", "description": "Review the edit" }] }),
                );
            }
            "session/list" => reply(
                json!({ "sessions": [{ "sessionId": "s0", "cwd": params["cwd"], "title": "An earlier thread" }] }),
            ),
            "session/set_mode" => {
                reply(json!({}));
                update(
                    "s1",
                    json!({ "sessionUpdate": "current_mode_update", "currentModeId": params["modeId"] }),
                );
            }
            "session/prompt" => {
                let session = params["sessionId"].as_str().unwrap_or("s1").to_string();
                update(
                    &session,
                    json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Adding a flash" } }),
                );
                update(
                    &session,
                    json!({
                        "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "mcp__edits__preset_apply", "kind": "edit", "status": "pending",
                        "rawInput": { "project": "project.json", "preset": "flash", "layer": "d", "at": "b2" },
                        "_meta": { "claudeCode": { "toolName": "mcp__edits__preset_apply" } },
                    }),
                );
                out(
                    json!({ "jsonrpc": "2.0", "id": "perm-1", "method": "session/request_permission", "params": {
                        "sessionId": session,
                        "toolCall": { "toolCallId": "t1", "title": "mcp__edits__preset_apply", "rawInput": { "preset": "flash", "layer": "d", "at": "b2" } },
                        "options": [{ "optionId": "allow", "name": "Allow", "kind": "allow_once" }, { "optionId": "reject", "name": "Reject", "kind": "reject_once" }],
                    }}),
                );
                // Wait for the answer (a cancel may come first).
                let mut chosen = String::from("cancelled");
                for line in lines.by_ref() {
                    let Ok(line) = line else { break };
                    let Ok(m) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if m.get("id") == Some(&json!("perm-1")) {
                        let o = &m["result"]["outcome"];
                        chosen = o["optionId"]
                            .as_str()
                            .unwrap_or(o["outcome"].as_str().unwrap_or("?"))
                            .to_string();
                        break;
                    }
                }
                let status = if chosen == "allow" {
                    "completed"
                } else {
                    "failed"
                };
                update(
                    &session,
                    json!({ "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": status }),
                );
                update(
                    &session,
                    json!({ "sessionUpdate": "plan", "entries": [{ "content": "Flash on the drop", "priority": "high", "status": "completed" }] }),
                );
                let stop = if chosen == "cancelled" {
                    "cancelled"
                } else {
                    "end_turn"
                };
                reply(json!({ "stopReason": stop, "chosen": chosen }));
            }
            "exit-now" => std::process::exit(3),
            _ if id.is_some() => out(
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("no {method}") } }),
            ),
            _ => {}
        }
    }
}

#[test]
fn fake_acp_agent_process() {
    if std::env::var_os("EDITS_FAKE_ACP").is_none() {
        return;
    }
    fake_agent();
    std::process::exit(0);
}

struct Rig {
    host: Arc<AgentHost>,
    events: mpsc::Receiver<Value>,
    dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.host.disconnect();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rig(name: &str) -> Rig {
    let dir = std::env::temp_dir().join(format!("edits-studio-acp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let project = dir.join("project.json");
    let doc = json!({ "settings": { "time_base": "beats", "bpm": 120, "fps": 25, "duration": 8 }, "layers": [{ "id": "d" }] });
    std::fs::write(&project, doc.to_string()).unwrap();
    let (tx, events) = mpsc::channel();
    let tx = Mutex::new(tx);
    let host = AgentHost::new(
        Arc::new(move |name: &str, v: Value| {
            if name == "agent" {
                let _ = lock(&tx).send(v);
            }
        }),
        Arc::new(move || Some(project.clone())),
        Arc::new(move || Some(doc.clone())),
    );
    Rig { host, events, dir }
}

fn fake(rig: &Rig) -> Result<Value, AgentError> {
    let exe = std::env::current_exe().unwrap();
    let adapter = adapters::Adapter {
        id: "custom".into(),
        name: "Fake agent".into(),
        agent: "Fake".into(),
        command: vec![exe.display().to_string()],
        source: "env".into(),
        package: None,
        install: None,
        login: vec![],
    };
    rig.host.connect_command(
        adapter,
        &exe.display().to_string(),
        &[
            "agent::tests::fake_acp_agent_process".into(),
            "--exact".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ],
        &[("EDITS_FAKE_ACP".into(), "1".into())],
    )
}

/// The next event of a type, within a few seconds.
fn next_of(rig: &Rig, kind: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let e = rig
            .events
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("no {kind} event"));
        if e["type"] == kind {
            return e;
        }
    }
}

#[test]
fn a_turn_streams_updates_asks_permission_and_points_at_the_layer() {
    let rig = rig("turn");
    let init = fake(&rig).expect("handshake");
    assert_eq!(init["init"]["agentInfo"]["name"], "fake-agent");
    let caps = &init["init"]["echo"]["clientCapabilities"];
    assert_eq!(
        (
            caps["auth"]["terminal"].as_bool(),
            caps["terminal"].as_bool()
        ),
        (Some(true), Some(false))
    );
    assert_eq!(
        init["init"]["echo"]["protocolVersion"],
        json!(PROTOCOL_VERSION)
    );

    let s = rig.host.session_new().expect("session");
    assert_eq!(s["sessionId"], "s1");
    // The session runs in the project's folder with the edits MCP server when it is built.
    assert_eq!(
        s["echo"]["cwd"].as_str().unwrap(),
        rig.dir.display().to_string()
    );
    let servers = s["echo"]["mcpServers"].as_array().unwrap();
    if let Some(m) = servers.first() {
        assert_eq!(m["name"], MCP_NAME);
        assert_eq!(
            (&m["args"][0], &m["args"][1]),
            (&json!("--root"), &json!(rig.dir.display().to_string()))
        );
        assert_eq!(
            m["env"][0],
            json!({ "name": "EDITS_ACTIVITY_VIA", "value": "studio" })
        );
    }
    assert_eq!(s["studio"]["told"], json!(false));
    assert!(s["studio"]["note"]
        .as_str()
        .unwrap()
        .contains("project.json"));
    let cmds = next_of(&rig, "update");
    assert_eq!(cmds["update"]["availableCommands"][0]["name"], "review");

    let listed = rig.host.session_list().unwrap();
    assert_eq!(listed["sessions"][0]["title"], "An earlier thread");
    rig.host.set_mode("s1", "plan").unwrap();
    assert_eq!(next_of(&rig, "update")["update"]["currentModeId"], "plan");

    let host = rig.host.clone();
    let turn = std::thread::spawn(move || {
        host.prompt("s1", json!([{ "type": "text", "text": "add a flash" }]))
    });
    let chunk = next_of(&rig, "update");
    assert_eq!(chunk["update"]["content"]["text"], "Adding a flash");
    let call = next_of(&rig, "update");
    let p = &call["presence"];
    assert_eq!(
        (p["tool"].as_str(), p["label"].as_str()),
        (
            Some("preset_apply"),
            Some("applying flash at b2 on layer d")
        )
    );
    assert_eq!(
        (p["layers"][0].as_str(), p["t"].as_f64()),
        (Some("d"), Some(2.0))
    );
    let ask = next_of(&rig, "permission");
    assert_eq!(ask["options"][0]["optionId"], "allow");
    assert_eq!(ask["presence"]["layers"][0], "d");
    rig.host
        .permission(ask["key"].as_str().unwrap(), Some("allow"))
        .unwrap();
    assert!(
        rig.host
            .permission(ask["key"].as_str().unwrap(), Some("allow"))
            .is_err(),
        "answered once"
    );
    let done = turn.join().unwrap().unwrap();
    assert_eq!(
        (done["stopReason"].as_str(), done["chosen"].as_str()),
        (Some("end_turn"), Some("allow"))
    );
    let status = next_of(&rig, "update");
    assert_eq!(status["update"]["status"], "completed");
}

#[test]
fn a_cancel_answers_the_open_question_and_ends_the_turn() {
    let rig = rig("cancel");
    fake(&rig).unwrap();
    rig.host.session_new().unwrap();
    let host = rig.host.clone();
    let turn =
        std::thread::spawn(move || host.prompt("s1", json!([{ "type": "text", "text": "go" }])));
    next_of(&rig, "permission");
    rig.host.cancel("s1").unwrap();
    assert_eq!(
        next_of(&rig, "permission_done")["key"]
            .as_str()
            .map(|k| k.starts_with('p')),
        Some(true)
    );
    assert_eq!(turn.join().unwrap().unwrap()["stopReason"], "cancelled");
}

#[test]
fn an_agent_without_a_login_says_so_and_an_exit_is_reported() {
    let rig = rig("needs-login");
    fake(&rig).unwrap();
    let e = rig.host.session_new().unwrap_err();
    assert!(e.auth_required, "{e:?}");
    assert_eq!(e.code, Some(AUTH_REQUIRED));
    let cmd = rig.host.login_command("fake-login").unwrap();
    assert_eq!(cmd.last().map(String::as_str), Some("--login"));

    // A request the agent never answers fails when it exits, and the page hears of the exit.
    let conn = rig.host.conn().unwrap();
    let r = conn.request("exit-now", json!({}), Some(Duration::from_secs(10)));
    assert_eq!(r.unwrap_err().code, -32099);
    let exit = next_of(&rig, "exit");
    assert_eq!(exit["code"], json!(3));
    assert!(rig
        .host
        .session_new()
        .unwrap_err()
        .message
        .contains("no agent is running"));
}

#[test]
fn edits_tools_are_recognised_in_every_adapters_form() {
    let claude = json!({ "title": "Edit", "_meta": { "claudeCode": { "toolName": "mcp__edits__project_edit" } }, "rawInput": { "ops": [] } });
    assert_eq!(edits_tool(&claude).unwrap().0, "project_edit");
    let codex = json!({ "title": "Tool: edits/render_frame", "rawInput": { "server": "edits", "tool": "render_frame", "arguments": { "frame": 60 } } });
    assert_eq!(
        edits_tool(&codex),
        Some(("render_frame".into(), json!({ "frame": 60 })))
    );
    let dotted = json!({ "title": "edits.render_sheet", "rawInput": {} });
    assert_eq!(edits_tool(&dotted).unwrap().0, "render_sheet");
    assert_eq!(
        edits_tool(&json!({ "title": "Read project.json", "rawInput": { "file_path": "x" } })),
        None
    );
    assert_eq!(
        edits_tool(
            &json!({ "title": "x", "rawInput": { "server": "other", "tool": "render_frame" } })
        ),
        None
    );
    let f = presence_of(&json!({ "sessionUpdate": "tool_call", "title": "mcp__edits__render_frame", "rawInput": { "frame": 50 } }), Some(&json!({ "settings": { "time_base": "beats", "bpm": 120, "fps": 25 } }))).unwrap();
    assert_eq!(
        (f["frame"].as_u64(), f["t"].as_f64(), f["label"].as_str()),
        (Some(50), Some(4.0), Some("looking at frame 50"))
    );
    // A status-only update of a call carries no presence (the page keeps the call's).
    assert!(presence_of(
        &json!({ "sessionUpdate": "tool_call_update", "title": "mcp__edits__render_frame" }),
        None
    )
    .is_none());
}

#[test]
fn json_rpc_lines_are_told_apart() {
    use rpc::{parse_line, Inbound, Parsed};
    assert!(matches!(
        parse_line(r#"{"jsonrpc":"2.0","id":4,"result":{"ok":1}}"#),
        Parsed::Response {
            id: 4,
            result: Ok(_)
        }
    ));
    match parse_line(
        r#"{"jsonrpc":"2.0","id":5,"error":{"code":-32000,"message":"Authentication required"}}"#,
    ) {
        Parsed::Response {
            id: 5,
            result: Err(e),
        } => assert_eq!(
            (e.code, e.message.as_str()),
            (-32000, "Authentication required")
        ),
        _ => panic!("an error answer"),
    }
    assert!(matches!(
        parse_line(
            r#"{"jsonrpc":"2.0","id":"x","method":"session/request_permission","params":{}}"#
        ),
        Parsed::Inbound(Inbound::Request { .. })
    ));
    assert!(matches!(
        parse_line(r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#),
        Parsed::Inbound(Inbound::Notification { .. })
    ));
    assert!(matches!(
        parse_line("Starting agent..."),
        Parsed::Inbound(Inbound::Junk(_))
    ));
}
