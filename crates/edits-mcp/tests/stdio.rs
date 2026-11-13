//! Drives the built `edits-mcp` binary the way a client does: JSON-RPC, one
//! message per line, over its stdin and stdout.
//!
//! The server speaks MCP 2026-07-28, which has no `initialize` handshake and
//! no session: the client may ask `server/discover` what the server supports,
//! and every request carries the protocol version and the client's
//! capabilities in `_meta`. The older `initialize` handshake still works and
//! is tested too, because clients move at their own pace.
//!
//! The tests need the `edits` executable (the renderer). It is looked for in
//! `EDITS_BIN`, next to the server binary and in the workspace's `target*`
//! folders; when it is not there the tests print why and pass.

use base64::Engine;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const SERVER: &str = env!("CARGO_BIN_EXE_edits-mcp");

const TOOLS: [&str; 25] = [
    "audio_analyze",
    "beats_accent",
    "beats_cut",
    "doctor",
    "effect_define",
    "effects_list",
    "job_cancel",
    "job_status",
    "preset_apply",
    "presets_list",
    "project_check",
    "project_describe",
    "project_edit",
    "project_fit",
    "project_read",
    "project_undo",
    "reference_analyze",
    "reference_compare",
    "reference_draft",
    "reference_probe",
    "reference_read",
    "render_frame",
    "render_sheet",
    "render_video",
    "ui_context",
];

fn find(name: &str, env: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    // An explicit path is final: when it names nothing, nothing else is tried.
    if let Some(p) = std::env::var_os(env).filter(|v| !v.is_empty()).map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let beside = Path::new(SERVER).with_file_name(&file);
    if beside.is_file() {
        return Some(beside);
    }
    // <workspace>/target*/{release,debug}/<name>
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?;
    let mut found: Vec<PathBuf> = std::fs::read_dir(workspace)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("target"))
        .flat_map(|e| ["release", "debug"].map(|profile| e.path().join(profile).join(&file)))
        .filter(|p| p.is_file())
        .collect();
    // The newest build wins.
    found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    found.pop()
}

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("edits-mcp-it-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The server compares against the canonical root; so do the assertions.
        TempDir(dir.canonicalize().unwrap())
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
    /// `_meta` sent with every request (the stateless lifecycle), or none (after `initialize`).
    meta: Option<Value>,
}

impl Client {
    fn start(root: &Path, edits: &Path, analyze: Option<&Path>) -> Self {
        let mut command = Command::new(SERVER);
        command.arg("--root").arg(root).arg("--edits").arg(edits).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        match analyze {
            Some(a) => command.arg("--edits-analyze").arg(a),
            None => command.env("EDITS_ANALYZE_BIN", root.join("no-such-binary")),
        };
        let mut child = command.spawn().expect("the server starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Client { child, stdin, lines, next_id: 0, meta: None }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").expect("the server reads its stdin");
        self.stdin.flush().unwrap();
    }

    /// Send a request and wait for the response with its id.
    fn request(&mut self, method: &str, mut params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        if let Some(meta) = &self.meta {
            params["_meta"] = meta.clone();
        }
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let left = deadline.checked_duration_since(Instant::now()).unwrap_or_else(|| panic!("no response to {method} within 120 s"));
            let line = self.lines.recv_timeout(left).unwrap_or_else(|_| panic!("the server closed stdout or did not answer {method}"));
            let message: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("stdout must carry only JSON-RPC, got {line:?}: {e}"));
            assert_eq!(message["jsonrpc"], "2.0", "{line}");
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
        let response = self.request(method, params);
        assert!(response.get("error").is_none(), "{method} failed: {response}");
        response["result"].clone()
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.result("tools/call", json!({ "name": tool, "arguments": arguments }))
    }

    /// Call a tool that must succeed; returns (all text joined, the whole result).
    fn ok(&mut self, tool: &str, arguments: Value) -> (String, Value) {
        let result = self.call(tool, arguments);
        assert_ne!(result["isError"], json!(true), "{tool} reported an error: {}", text_of(&result));
        (text_of(&result), result)
    }

    /// Call a tool that must report a tool-level error; returns its message.
    fn err(&mut self, tool: &str, arguments: Value) -> String {
        let result = self.call(tool, arguments);
        assert_eq!(result["isError"], json!(true), "{tool} should have failed: {result}");
        text_of(&result)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn text_of(result: &Value) -> String {
    result["content"].as_array().into_iter().flatten().filter(|c| c["type"] == "text").filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n")
}

/// A demo project in a fresh workspace, or `None` (with the reason printed) when there is no renderer.
fn setup(name: &str) -> Option<(TempDir, PathBuf, Option<PathBuf>)> {
    let Some(edits) = find("edits", "EDITS_BIN") else {
        eprintln!("skipping {name}: the `edits` executable was not found (build it with `cargo build --release -p edits-cli`, or set EDITS_BIN)");
        return None;
    };
    let root = TempDir::new(name);
    let made = Command::new(&edits).arg("demo").arg(root.0.join("demo")).output().expect("edits demo runs");
    assert!(made.status.success(), "edits demo failed: {}", String::from_utf8_lossy(&made.stderr));
    Some((root, edits, find("edits-analyze", "EDITS_ANALYZE_BIN")))
}

fn stateless_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { "name": "edits-mcp-test", "version": "0" },
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

#[test]
fn stateless_lifecycle_tools_edit_and_render() {
    let Some((root, edits, analyze)) = setup("stateless") else { return };
    let project = root.0.join("demo/project.json");
    let original = std::fs::read_to_string(&project).unwrap();
    let mut c = Client::start(&root.0, &edits, analyze.as_deref());
    c.meta = Some(stateless_meta());

    // 2026-07-28: no initialize. Discovery is optional and says what the server speaks.
    let discovered = c.result("server/discover", json!({}));
    let versions: Vec<&str> = discovered["supportedVersions"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(versions.contains(&"2026-07-28"), "{discovered}");
    assert!(discovered["capabilities"].get("tools").is_some(), "{discovered}");
    assert!(discovered.to_string().contains("edits-mcp"), "the server names itself: {discovered}");
    assert!(discovered["instructions"].as_str().is_some_and(|i| i.starts_with("edits renders")), "{discovered}");

    // A request without the per-request metadata is refused, not guessed at.
    let saved = c.meta.take();
    let refused = c.request("tools/list", json!({}));
    assert!(refused["error"]["message"].as_str().unwrap().contains("_meta"), "{refused}");
    c.meta = saved;

    // The tool list: fixed names, in a fixed order, with schemas and annotations.
    let listed = c.result("tools/list", json!({}));
    let tools = listed["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, TOOLS);
    assert!(listed.get("ttlMs").is_some() && listed.get("cacheScope").is_some(), "2026-07-28 lists carry cache hints: {listed}");
    let tool = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap();
    for t in tools {
        let schema = &t["inputSchema"];
        assert_eq!(schema["type"], "object", "{t}");
        assert_eq!(schema["additionalProperties"], json!(false), "unknown arguments are refused: {t}");
        assert!(t["description"].as_str().is_some_and(|d| d.len() > 40 && d.len() < 700), "{t}");
        assert_eq!(t["annotations"]["openWorldHint"], json!(false), "{t}");
        assert!(!schema.to_string().contains("\"null\""), "optional arguments are plain types: {t}");
    }
    assert_eq!(tool("project_describe")["annotations"]["readOnlyHint"], json!(true));
    assert_eq!(tool("project_edit")["annotations"]["readOnlyHint"], json!(false));
    assert_eq!(tool("project_edit")["annotations"]["destructiveHint"], json!(true));
    assert_eq!(tool("render_frame")["inputSchema"]["required"], json!(["project"]));
    // Times are numbers or text ("b12.5", "bar 4", "drop").
    assert_eq!(tool("render_frame")["inputSchema"]["properties"]["at"]["type"], json!(["number", "string"]));
    let op = &tool("project_edit")["inputSchema"];
    assert!(op.to_string().contains("insert_layer") && op.to_string().contains("set_slot"), "{op}");

    // describe: the outline
    let (outline, _) = c.ok("project_describe", json!({ "project": "demo/project.json" }));
    assert!(outline.contains("layers (bottom to top)") && outline.contains("123 BPM"), "{outline}");

    // read: one layer by id, one value by pointer
    let (_, read) = c.ok("project_read", json!({ "project": "demo/project.json", "pointer": "/layers/@finish/effects/@vignette/params/amount" }));
    assert_eq!(read["structuredContent"]["value"], json!(0.6));

    // edit: a batch, addressed by id
    let (said, edited) = c.ok(
        "project_edit",
        json!({ "project": "demo/project.json", "ops": [
            { "op": "set", "pointer": "/layers/@finish/effects/@vignette/params/amount", "value": 0.8 },
            { "op": "insert_layer", "below": "finish", "layer": { "id": "wash", "kind": "solid", "color": [1, 1, 1, 0.1], "in": 0, "out": 1 } }
        ] }),
    );
    assert!(said.contains("applied 2 op(s)") && said.contains("/layers/@wash") && said.contains("0.6 → 0.8"), "{said}");
    assert_eq!(edited["structuredContent"]["written"], json!(true));
    let now: Value = serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    assert_eq!(now["layers"][4]["id"], "wash");
    assert_eq!(now["layers"][5]["effects"][2]["params"]["amount"], json!(0.8));
    let after_edit = std::fs::read_to_string(&project).unwrap();

    // an edit that does not validate changes nothing and says why
    let why = c.err("project_edit", json!({ "project": "demo/project.json", "ops": [
        { "op": "set", "pointer": "/settings/bpm", "value": 90 },
        { "op": "set", "pointer": "/layers/@a/effects/-", "value": { "kind": "glow", "params": { "radus": 3 } } }
    ] }));
    assert!(why.contains("NOT applied") && why.contains("has no parameter \"radus\"") && why.contains("/layers/@a/effects/2/params/radus"), "{why}");
    assert_eq!(std::fs::read_to_string(&project).unwrap(), after_edit);

    // paths outside the workspace are refused before anything runs
    let outside = c.err("project_describe", json!({ "project": "../../etc/passwd" }));
    assert!(outside.contains("outside the workspace root"), "{outside}");
    let outside = c.err("render_video", json!({ "project": "demo/project.json", "output": "/tmp/escape.mp4" }));
    assert!(outside.contains("outside the workspace root"), "{outside}");

    // malformed arguments are refused with a message that names the problem and the choices
    let bad = c.err("render_frame", json!({ "project": "demo/project.json", "att": 3 }));
    assert!(bad.contains("unknown field `att`") && bad.contains("`at`"), "{bad}");
    let unknown = c.request("tools/call", json!({ "name": "render_everything", "arguments": {} }));
    assert_eq!(unknown["error"]["code"], json!(-32602), "{unknown}");

    // doctor says whether this machine can render; without an adapter the rest cannot run
    let (_, doctor) = c.ok("doctor", json!({}));
    assert_eq!(doctor["structuredContent"]["server"]["workspace_root"], json!(root.0.display().to_string()));
    if doctor["structuredContent"]["adapter"].is_null() {
        eprintln!("skipping the render checks: no graphics adapter ({})", doctor["structuredContent"]["adapter_error"]);
        return;
    }

    // render_frame: an image comes back, small enough to send to a model
    let (said, frame) = c.ok("render_frame", json!({ "project": "demo/project.json", "at": 3 }));
    let blocks = frame["content"].as_array().unwrap();
    let image = blocks.iter().find(|b| b["type"] == "image").expect("render_frame returns an image block");
    assert_eq!(image["mimeType"], "image/png");
    let bytes = base64::engine::general_purpose::STANDARD.decode(image["data"].as_str().unwrap()).expect("valid base64");
    let decoded = image::load_from_memory(&bytes).expect("a decodable image");
    assert_eq!((decoded.width(), decoded.height()), (540, 540), "1080x1080 renders at a 540 px long edge by default");
    assert!(decoded.to_rgb8().pixels().any(|p| p.0 != [0, 0, 0]), "the frame is not black");
    assert!(said.contains("frame 36 of demo/project.json at beat 2.952"), "{said}");
    assert!(said.contains("about 400 image tokens") && said.contains(".edits/renders/project-f36.png"), "{said}");
    assert!(root.0.join(".edits/renders/project-f36.png").is_file());
    assert!(frame.to_string().len() < 1_500_000, "one result stays small: {} bytes", frame.to_string().len());

    // render_sheet: one image for many frames
    let (said, sheet) = c.ok("render_sheet", json!({ "project": "demo/project.json", "from": 3.9, "to": 4.4, "every_frame": true, "cols": 3, "samples": 1 }));
    assert!(sheet["content"].as_array().unwrap().iter().any(|b| b["type"] == "image"), "{said}");
    assert!(said.contains("6 frames") && said.contains("3 columns x 2 rows"), "{said}");
    let too_many = c.err("render_sheet", json!({ "project": "demo/project.json", "every_frame": true }));
    assert!(too_many.contains("at most 64"), "{too_many}");

    // undo walks back: first the edit, then nothing is left
    let (said, _) = c.ok("project_undo", json!({ "project": "demo/project.json" }));
    assert!(said.contains("0.8 → 0.6") && said.contains("- /layers/@wash"), "{said}");
    let restored: Value = serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    assert_eq!(restored, serde_json::from_str::<Value>(&original).unwrap());
    assert_eq!(std::fs::read_to_string(&project).unwrap(), original, "the original bytes are back");
    assert!(c.err("project_undo", json!({ "project": "demo/project.json" })).contains("nothing to undo"));

    // render_video: a job id at once, then job_status until it is done
    let (said, started) = c.ok("render_video", json!({ "project": "demo/project.json", "output": "out/clip.mp4", "to": 1, "scale": 0.25, "samples": 1 }));
    let job = started["structuredContent"]["job"].as_str().unwrap().to_string();
    assert!(said.contains(&job) && job.starts_with("render-"), "{said}");
    let deadline = Instant::now() + Duration::from_secs(120);
    let done = loop {
        let (_, status) = c.ok("job_status", json!({ "job": job }));
        let state = status["structuredContent"]["state"].as_str().unwrap().to_string();
        if state != "running" {
            break status;
        }
        assert!(Instant::now() < deadline, "the render did not finish: {status}");
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(done["structuredContent"]["state"], "completed", "{}", text_of(&done));
    assert_eq!(done["structuredContent"]["result"]["frames"], json!(13));
    assert!(root.0.join("out/clip.mp4").metadata().unwrap().len() > 1000);
    let (listed, _) = c.ok("job_status", json!({}));
    assert!(listed.contains(&job) && listed.contains("completed"), "{listed}");
    assert!(c.err("job_status", json!({ "job": "render-nope" })).contains("no job"));

    // a job can be stopped, and a stopped render leaves no half-written file
    let (_, started) = c.ok("render_video", json!({ "project": "demo/project.json", "output": "out/full.mp4" }));
    let job = started["structuredContent"]["job"].as_str().unwrap().to_string();
    let (said, cancelled) = c.ok("job_cancel", json!({ "job": job }));
    assert_eq!(cancelled["structuredContent"]["state"], "cancelled", "{said}");
    assert!(!root.0.join("out/full.mp4").exists(), "{said}");

    // the analysis side, when its executable is built
    if analyze.is_some() {
        let (said, probe) = c.ok("reference_probe", json!({ "video": "out/clip.mp4" }));
        assert!(said.contains("out/clip.mp4 270x270 25 fps, 13 frames"), "{said}");
        assert_eq!(probe["structuredContent"]["table"].as_array().unwrap().len(), 13);
    } else {
        let missing = c.err("reference_probe", json!({ "video": "out/clip.mp4" }));
        assert!(missing.contains("EDITS_ANALYZE_BIN"), "{missing}");
    }
}

#[test]
fn the_initialize_handshake_of_older_clients_still_works() {
    let Some((root, edits, analyze)) = setup("initialize") else { return };
    let mut c = Client::start(&root.0, &edits, analyze.as_deref());
    let init = c.result("initialize", json!({ "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "edits-mcp-test", "version": "0" } }));
    assert_eq!(init["protocolVersion"], "2025-11-25");
    assert_eq!(init["serverInfo"]["name"], "edits-mcp");
    assert!(init["instructions"].as_str().unwrap().len() > 100);
    c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let listed = c.result("tools/list", json!({}));
    let names: Vec<&str> = listed["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, TOOLS);
    let (text, effects) = c.ok("effects_list", json!({ "kind": "exposure" }));
    assert!(text.contains("stops = 0"), "{text}");
    assert_eq!(effects["structuredContent"]["effects"][0]["kind"], "exposure");
    assert!(c.err("effects_list", json!({ "kind": "nope" })).contains("effects: exposure, fade"));
    let (text, check) = c.ok("project_check", json!({ "project": "demo/project.json" }));
    assert!(text.contains("0 error(s), 0 warning(s)"), "{text}");
    assert_eq!(check["structuredContent"]["ok"], json!(true));
}

/// Poll `job_status` until the job leaves the running state.
fn wait_for(c: &mut Client, job: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let status = c.call("job_status", json!({ "job": job }));
        if status["structuredContent"]["state"] != "running" {
            return status;
        }
        assert!(Instant::now() < deadline, "job {job} did not finish: {status}");
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// The lead use case, end to end through the tools: take a finished edit,
/// extract its recipe, draft a project from it, and measure a render against
/// the reference in signature space.
#[test]
fn recipe_of_a_reference_edit_analyze_draft_compare() {
    let Some((root, edits, analyze)) = setup("recipe") else { return };
    let Some(analyze) = analyze else {
        eprintln!("skipping: the `edits-analyze` executable was not found (cargo build --release -p edits-analyze, or set EDITS_ANALYZE_BIN)");
        return;
    };
    let mut c = Client::start(&root.0, &edits, Some(&analyze));
    c.meta = Some(stateless_meta());
    let (_, doctor) = c.ok("doctor", json!({}));
    if doctor["structuredContent"]["ok"] != json!(true) {
        eprintln!("skipping: this machine cannot render ({})", text_of(&doctor));
        return;
    }

    // The "reference": the first four beats of the demo, small and without motion blur to keep the test quick.
    let (_, started) = c.ok("render_video", json!({ "project": "demo/project.json", "output": "ref/reference.mp4", "to": 4, "scale": 0.25, "samples": 1 }));
    let done = wait_for(&mut c, started["structuredContent"]["job"].as_str().unwrap());
    assert_eq!(done["structuredContent"]["state"], "completed", "{}", text_of(&done));

    // analyze: a job; its result is the path and a summary
    let (said, started) = c.ok("reference_analyze", json!({ "video": "ref/reference.mp4", "out_dir": "ref/analysis", "bpm": 123 }));
    assert_eq!(started["structuredContent"]["analysis"], "ref/analysis/reference_analysis.json", "{said}");
    let done = wait_for(&mut c, started["structuredContent"]["job"].as_str().unwrap());
    let said = text_of(&done);
    assert_eq!(done["structuredContent"]["state"], "completed", "{said}");
    assert!(said.contains("tempo: 123.0 bpm") && said.contains("shots: "), "{said}");
    let summary = &done["structuredContent"]["summary"];
    assert!(summary["shot_count"].as_u64().unwrap() >= 2, "the cut at beat 2 is found: {summary}");
    assert!(!summary["flash_events"].as_array().unwrap().is_empty(), "the flashes are found: {summary}");
    assert!(root.0.join("ref/analysis/reference_analysis.json").is_file());

    // read: sections instead of the whole file
    let (said, _) = c.ok("reference_read", json!({ "analysis": "ref/analysis/reference_analysis.json" }));
    assert!(said.starts_with("source: 270x270"), "{said}");
    let (_, shot) = c.ok("reference_read", json!({ "analysis": "ref/analysis/reference_analysis.json", "shot": 1 }));
    assert_eq!(shot["structuredContent"]["id"], json!(1));
    assert!(shot["structuredContent"]["curves"].is_array());
    let (_, grid) = c.ok("reference_read", json!({ "analysis": "ref/analysis/reference_analysis.json", "section": "grid" }));
    assert_eq!(grid["structuredContent"]["bpm"]["value"], json!(123.0));
    assert!(c.err("reference_read", json!({ "analysis": "demo/project.json" })).contains("does not look like a reference analysis"));

    // draft: a project that passes check and renders at once from the proxy stills
    let (said, draft) = c.ok("reference_draft", json!({ "analysis": "ref/analysis/reference_analysis.json", "output": "edit/project.json", "proxies": true }));
    assert!(said.contains("wrote edit/project.json") && said.contains("layers (bottom to top)") && said.contains("check: 0 error(s)"), "{said}");
    assert_eq!(draft["structuredContent"]["ok"], json!(true));
    let (said, sheet) = c.ok("render_sheet", json!({ "project": "edit/project.json", "count": 6, "samples": 1 }));
    assert!(sheet["content"].as_array().unwrap().iter().any(|b| b["type"] == "image"), "{said}");

    // compare: the reference against itself, scored in signature space. Every event and
    // curve matches; the tempo score is left out of the assertion because `compare`
    // re-estimates the tempo from four beats of audio, which is ambiguous (82 or 123).
    let (said, cmp) = c.ok("reference_compare", json!({ "analysis": "ref/analysis/reference_analysis.json", "video": "ref/reference.mp4" }));
    assert!(said.contains("signature-space comparison") && said.contains("overall "), "{said}");
    assert_eq!(cmp["structuredContent"]["state"], "completed");
    let scores = &cmp["structuredContent"]["comparison"]["scores"];
    for name in ["cuts", "flashes", "exposure", "blur"] {
        assert_eq!(scores[name], json!(1.0), "{name}: {said}");
    }
    assert!(cmp["structuredContent"]["comparison"]["overall"].as_f64().unwrap() > 0.8, "{said}");

    // fit: a job that reports what it would change; a dry run writes nothing.
    let before = std::fs::read_to_string(root.0.join("edit/project.json")).unwrap();
    let (said, started) = c.ok("project_fit", json!({ "project": "edit/project.json", "analysis": "ref/analysis/reference_analysis.json", "budget": 3, "dry_run": true }));
    assert!(said.contains("started fit job") && said.contains("Nothing is written"), "{said}");
    let done = wait_for(&mut c, started["structuredContent"]["job"].as_str().unwrap());
    let said = text_of(&done);
    assert_eq!(done["structuredContent"]["state"], "completed", "{said}");
    let fit = &done["structuredContent"]["fit"];
    assert_eq!(fit["written"], json!(false), "{said}");
    assert!(fit["overall_before"].is_number() && fit["changes"].is_array(), "{said}");
    assert!(said.contains("overall ") && said.contains("NOT written"), "{said}");
    assert_eq!(std::fs::read_to_string(root.0.join("edit/project.json")).unwrap(), before);
}

/// The toolkit an agent uses to make an edit from a song and pictures:
/// analyse the song, cut to the beat, accent, add a transition, look.
#[test]
fn a_song_cut_to_the_beat_with_presets() {
    let Some((root, edits, analyze)) = setup("toolkit") else { return };
    let project = root.0.join("demo/project.json");
    let mut c = Client::start(&root.0, &edits, analyze.as_deref());
    c.meta = Some(stateless_meta());

    // The song: tempo, beats, onsets; written next to it and into the project.
    let (said, song) = c.ok("audio_analyze", json!({ "input": "demo/audio/beat.wav", "project": "demo/project.json" }));
    assert!(said.contains("123 BPM") && said.contains("demo/audio/beat.audio.json"), "{said}");
    assert_eq!(song["structuredContent"]["bpm"], json!(123.0), "{song}");
    assert!(root.0.join("demo/audio/beat.audio.json").is_file());

    // The presets an agent chooses from.
    let (said, _) = c.ok("presets_list", json!({}));
    assert!(said.contains("punch:") && said.contains("flash:") && said.contains("transition:whip_pan"), "{said}");
    let (said, _) = c.ok("presets_list", json!({ "name": "shake" }));
    assert!(said.contains("amplitude (number") && said.contains("frequency"), "{said}");

    // One picture per beat for 16 beats, then accents and a transition.
    let (said, _) = c.ok("beats_cut", json!({ "project": "demo/project.json", "slots": ["a", "b", "c", "d"], "from": 0, "to": "b16", "below": "tex" }));
    assert!(said.contains("16 image layer(s)") && said.contains("the project now runs to 16"), "{said}");
    let (said, _) = c.ok("beats_accent", json!({ "project": "demo/project.json", "preset": "punch", "on": "kicks", "audio": "demo/audio/beat.audio.json", "vars": { "amount": 1.1 } }));
    assert!(said.contains("punch on") && said.contains("cut1@0"), "{said}");
    let (said, _) = c.ok("preset_apply", json!({ "project": "demo/project.json", "preset": "transition:whip_pan", "layer": "cut5" }));
    assert!(said.contains("set cut5/transition") && said.contains("set cut4/out"), "{said}");
    let (said, _) = c.ok("preset_apply", json!({ "project": "demo/project.json", "preset": "flash", "layer": "cut9", "at": "bar 3", "vars": { "strength": 0.9 } }));
    assert!(said.contains("at 8:") && said.contains("added effect fade to cut9"), "{said}");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let cut5 = doc["layers"].as_array().unwrap().iter().find(|l| l["id"] == "cut5").unwrap();
    assert_eq!(cut5["transition"]["kind"], "whip_pan");
    // Mistakes say what to do.
    let why = c.err("preset_apply", json!({ "project": "demo/project.json", "preset": "flash", "at": 1, "vars": { "strenght": 1 } }));
    assert!(why.contains("variables: color, decay, strength"), "{why}");
    let why = c.err("beats_accent", json!({ "project": "demo/project.json", "preset": "flash", "on": "snares" }));
    assert!(why.contains("audio_analyze") || why.contains("edits audio"), "{why}");
    let (said, _) = c.ok("project_check", json!({ "project": "demo/project.json" }));
    assert!(said.contains("0 error(s)"), "{said}");

    // A custom effect: refused with a line number, then accepted and undoable.
    let bad = c.err("effect_define", json!({ "project": "demo/project.json", "name": "dim", "wgsl": "fn effect(uv: vec2<f32>) -> vec4<f32> {\n    return src(uv) * nope;\n}\n" }));
    assert!(bad.contains("line 2") && bad.contains("nope") && !root.0.join("demo/shaders/dim.wgsl").exists(), "{bad}");
    let good = "fn effect(uv: vec2<f32>) -> vec4<f32> {\n    let c = src(uv);\n    return vec4<f32>(c.rgb * (1.0 - 0.5 * fx.p0), c.a);\n}\n";
    let (said, _) = c.ok("effect_define", json!({ "project": "demo/project.json", "name": "dim", "wgsl": good, "params": { "p0": 0.5 } }));
    assert!(said.contains("wrote demo/shaders/dim.wgsl") && said.contains("reads p0"), "{said}");
    let (said, _) = c.ok("project_undo", json!({ "project": "demo/project.json" }));
    assert!(said.contains("- /layers/@dim"), "{said}");

    // What the window shows: nothing until it writes its file.
    let (said, _) = c.ok("ui_context", json!({ "project": "demo/project.json" }));
    assert!(said.contains("not open"), "{said}");
    let ui = json!({ "format": "edits.ui/1", "project": "project.json", "updated": edits_core::edit::utc_stamp(std::time::SystemTime::now()),
                     "playhead": { "t": 4.25 }, "selection": { "layer": "cut5" }, "note": "make this hit stronger" });
    std::fs::create_dir_all(root.0.join("demo/.edits")).unwrap();
    std::fs::write(root.0.join("demo/.edits/ui.json"), ui.to_string()).unwrap();
    let (said, ctx) = c.ok("ui_context", json!({ "project": "demo/project.json" }));
    assert!(said.contains("layer cut5") && said.contains("make this hit stronger") && said.contains("playhead at 4.25"), "{said}");
    assert_eq!(ctx["structuredContent"]["open"], json!(true));

    // Look at it.
    let (_, doctor) = c.ok("doctor", json!({}));
    if doctor["structuredContent"]["adapter"].is_null() {
        eprintln!("skipping the sheet: no graphics adapter");
        return;
    }
    let (said, sheet) = c.ok("render_sheet", json!({ "project": "demo/project.json", "from": "b3.5", "to": "b4.5", "count": 4, "samples": 1, "scale": 0.1 }));
    assert!(sheet["content"].as_array().unwrap().iter().any(|b| b["type"] == "image"), "{said}");
    assert!(said.contains("4 frames"), "{said}");
}
