//! End-to-end tests of the `edits` binary. Commands that need a graphics
//! adapter or ffmpeg skip (pass) on machines without them, as reported by
//! `edits doctor`.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn edits(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_edits")).args(args).output().expect("the edits binary runs")
}

fn json(out: &Output) -> Value {
    assert!(out.status.success(), "command failed: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

fn doctor() -> Value {
    serde_json::from_slice(&edits(&["doctor", "--json"]).stdout).expect("doctor --json prints JSON")
}

/// A demo project in its own folder, removed when dropped.
struct Demo(PathBuf);
impl Demo {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("edits-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        json(&edits(&["demo", dir.to_str().unwrap()]));
        Self(dir)
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_string()
    }
}
impl Drop for Demo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn doctor_reports_adapter_and_encoders() {
    let out = edits(&["doctor", "--json"]);
    let d: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(d["ok"].as_bool().unwrap(), out.status.success());
    assert!(d["ffmpeg"]["found"].is_boolean() && d["encoders"]["h264"].is_array() && d["encoders"]["aac"].is_array());
    if !d["adapter"].is_null() {
        assert!(d["adapter"]["name"].is_string() && d["adapter"]["backend"].is_string() && d["adapter"]["software"].is_boolean());
    } else {
        assert!(d["adapter_error"].is_string());
    }
    let text = String::from_utf8(edits(&["doctor"]).stdout).unwrap();
    assert!(text.contains("adapter:") && text.contains("ffmpeg:") && text.contains("h264:"));
    // An impossible adapter policy is an error, not a crash.
    let bad = edits(&["--adapter", "quantum", "doctor"]);
    assert!(!bad.status.success());
}

#[test]
fn effects_lists_the_registry() {
    let list = json(&edits(&["effects", "--json"]));
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), edits_core::effects::EFFECTS.len());
    let glow = list.iter().find(|e| e["kind"] == "glow").unwrap();
    let radius = glow["params"].as_array().unwrap().iter().find(|p| p["name"] == "radius").unwrap();
    assert_eq!(radius["default"], 3.0);
    assert!(radius["doc"].as_str().unwrap().contains("% of height"));
    assert_eq!(glow["params"].as_array().unwrap().iter().find(|p| p["name"] == "color").unwrap()["default"], serde_json::json!([1.0, 1.0, 1.0]));
    assert_eq!((glow["category"].as_str(), radius["unit"].as_str()), (Some("light"), Some("%h")));
    assert_eq!(radius["range"], serde_json::json!([0.0, null]));
    let pixelate = list.iter().find(|e| e["kind"] == "pixelate").unwrap();
    assert_eq!(pixelate["options"][0]["values"], serde_json::json!(["square", "hex", "triangle", "polar"]));
    assert_eq!(pixelate["options"][0]["default"], "square");
    assert!(pixelate["credit"].as_str().unwrap().contains("BSD-3-Clause"));
    assert_eq!(list.iter().find(|e| e["kind"] == "custom").unwrap()["source"], "shader");
    let text = String::from_utf8(edits(&["effects"]).stdout).unwrap();
    for e in edits_core::effects::EFFECTS {
        assert!(text.contains(&format!("{}:", e.kind)), "{} is missing", e.kind);
    }
}

#[test]
fn demo_describe_and_check() {
    let demo = Demo::new("check");
    let project = demo.path("project.json");
    for file in ["project.json", "img/a.png", "img/b.png", "img/c.png", "img/d.png", "img/tex.png", "audio/beat.wav"] {
        assert!(Path::new(&demo.path(file)).is_file(), "{file} was not written");
    }
    let text = String::from_utf8(edits(&["describe", &project]).stdout).unwrap();
    assert!(text.contains("1080x1080") && text.contains("cube") && text.contains("img/a.png"), "{text}");

    let ok = edits(&["check", &project, "--json"]);
    let v = json(&ok);
    assert_eq!(v["ok"], true);
    assert_eq!(v["errors"], 0);

    // Break it: an unknown effect and a missing picture.
    let broken = std::fs::read_to_string(&project).unwrap().replace("\"kind\": \"vignette\"", "\"kind\": \"vinyette\"").replace("img/b.png", "img/nope.png");
    let bad = demo.path("broken.json");
    std::fs::write(&bad, broken).unwrap();
    let out = edits(&["check", &bad, "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    let messages: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["message"].as_str().unwrap()).collect();
    assert!(messages.iter().any(|m| m.contains("unknown effect \"vinyette\"")) && messages.iter().any(|m| m.contains("img/nope.png")), "{messages:?}");
    let plain = edits(&["check", &bad]);
    assert_eq!(plain.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&plain.stdout).contains("error: "));
    // Rendering refuses a project with errors and says why.
    let out = edits(&["frame", &bad, "-o", &demo.path("x.png")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("vinyette"));

    // Not JSON at all, and a file that is not there.
    std::fs::write(demo.path("garbage.json"), "{ not json").unwrap();
    let out = edits(&["check", &demo.path("garbage.json"), "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(serde_json::from_slice::<Value>(&out.stdout).unwrap()["ok"], false);
    let out = edits(&["describe", &demo.path("missing.json")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read project"));
}

#[test]
fn frame_and_sheet_write_pngs() {
    if doctor()["adapter"].is_null() {
        eprintln!("skipping: no graphics adapter");
        return;
    }
    let demo = Demo::new("frame");
    let project = demo.path("project.json");

    let png = demo.path("out/frame.png");
    let v = json(&edits(&["frame", &project, "--at", "2.5", "--scale", "0.1", "-o", &png]));
    // Beat 2.5 at 123 BPM and 25 fps is 1.2195 s: frame 30, which starts at beat 2.46.
    assert_eq!(v["frame"], 30);
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(108), Some(108)));
    assert_eq!(v["time_unit"], "beats");
    let img = image::open(&png).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (108, 108));
    assert!(img.pixels().any(|p| p.0[0] > 60), "the frame is black");

    // --frame gives the same picture as --at for the same frame.
    let again = demo.path("again.png");
    json(&edits(&["frame", &project, "--frame", "30", "--scale", "0.1", "-o", &again]));
    assert!(image::open(&again).unwrap().to_rgba8() == img);
    let out = edits(&["frame", &project, "--frame", "5000", "-o", &again]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("past the end"));

    let sheet = demo.path("sheet.png");
    let v = json(&edits(&["sheet", &project, "--count", "6", "--cols", "3", "--scale", "0.1", "--samples", "1", "-q", "-o", &sheet]));
    assert_eq!(v["frames"].as_array().unwrap().len(), 6);
    assert_eq!(v["frames"][0], 0);
    assert_eq!(v["frames"][5], 97);
    assert_eq!((v["cols"].as_u64(), v["rows"].as_u64()), (Some(3), Some(2)));
    let img = image::open(&sheet).unwrap();
    assert_eq!((img.width() as u64, img.height() as u64), (v["width"].as_u64().unwrap(), v["height"].as_u64().unwrap()));
    assert!(img.width() >= 3 * 108 && img.height() > 2 * 108);

    let v = json(&edits(&["sheet", &project, "--from", "2", "--to", "2.5", "--every-frame", "--scale", "0.05", "--samples", "1", "-q", "-o", &sheet]));
    assert_eq!(v["frames"], serde_json::json!([25, 26, 27, 28, 29, 30]));
    let out = edits(&["sheet", &project, "--from", "9", "-o", &sheet]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no frames"));

    let v = json(&edits(&["bench", &project, "--count", "3", "--scale", "0.1"]));
    assert_eq!(v["frames"], 3);
    assert!(v["ms_per_frame"].as_f64().unwrap() > 0.0);
}

#[test]
fn render_writes_an_h264_mp4_with_audio() {
    let d = doctor();
    if d["ok"] != true {
        eprintln!("skipping: {}", if d["adapter"].is_null() { "no graphics adapter" } else { "no ffmpeg with an H.264 encoder" });
        return;
    }
    let demo = Demo::new("render");
    let project = demo.path("project.json");
    let mp4 = demo.path("out.mp4");
    let v = json(&edits(&["render", &project, "-o", &mp4, "--from", "3.5", "--to", "4.5", "--scale", "0.2", "--samples", "2", "--crf", "30", "-q"]));
    // Beats 3.5..4.5 are seconds 1.707..2.195: frames 43..=54.
    assert_eq!(v["first_frame"], 43);
    assert_eq!(v["frames"], 12);
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(216), Some(216)));
    assert_eq!(v["motion_blur_samples"], 2);
    assert_eq!(v["audio"], d["encoders"]["aac"].as_array().is_some_and(|a| !a.is_empty()));
    assert_eq!(v["encoder"], d["encoders"]["selected_h264"]);
    let bytes = std::fs::read(&mp4).unwrap();
    assert!(bytes.len() > 2000 && &bytes[4..8] == b"ftyp", "not an MP4");
    // +faststart puts the index before the media data.
    let find = |tag: &[u8]| bytes.windows(4).position(|w| w == tag).unwrap();
    assert!(find(b"moov") < find(b"mdat"));

    if let Ok(probe) = Command::new("ffprobe").args(["-v", "error", "-show_entries", "stream=codec_name,pix_fmt,width,height,nb_frames", "-of", "json", &mp4]).output() {
        if probe.status.success() {
            let p: Value = serde_json::from_slice(&probe.stdout).unwrap();
            let streams = p["streams"].as_array().unwrap();
            let video = streams.iter().find(|s| s["codec_name"] == "h264").expect("an H.264 stream");
            assert_eq!(video["pix_fmt"], "yuv420p");
            assert_eq!((video["width"].as_u64(), video["height"].as_u64()), (Some(216), Some(216)));
            assert_eq!(video["nb_frames"], "12");
            assert_eq!(streams.iter().any(|s| s["codec_name"] == "aac"), v["audio"].as_bool().unwrap());
        }
    }

    let silent = demo.path("silent.mp4");
    let v = json(&edits(&["render", &project, "-o", &silent, "--from", "0", "--to", "0.2", "--scale", "0.1", "--no-audio", "-q"]));
    assert_eq!(v["audio"], false);
    assert_eq!(v["frames"], 3);
    // A range with nothing in it fails cleanly.
    let out = edits(&["render", &project, "-o", &silent, "--from", "5", "--to", "5"]);
    assert_eq!(out.status.code(), Some(1));
}

/// The loudest sample of `file`'s sound between `from` and `to` seconds, in dB.
fn max_volume(file: &str, from: f64, to: f64) -> f64 {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-ss", &from.to_string(), "-t", &(to - from).to_string(), "-i", file, "-map", "0:a:0", "-af", "volumedetect", "-f", "null", "-"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    let line = text.lines().find(|l| l.contains("max_volume:")).unwrap_or_else(|| panic!("no volumedetect output: {text}"));
    line.split("max_volume:").nth(1).unwrap().trim().trim_end_matches(" dB").parse().unwrap()
}

#[test]
fn render_mixes_the_sound_of_video_layers() {
    let d = doctor();
    if d["ok"] != true || d["encoders"]["aac"].as_array().is_none_or(|a| a.is_empty()) {
        eprintln!("skipping: no graphics adapter, or no ffmpeg with H.264 and AAC encoders");
        return;
    }
    let dir = std::env::temp_dir().join(format!("edits-cli-video-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A one-second clip with a tone.
    let clip = dir.join("clip.mkv");
    let made = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=32x32:rate=24:duration=1", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-c:v", "ffv1", "-c:a", "pcm_s16le"])
        .arg(&clip)
        .status()
        .unwrap();
    assert!(made.success());
    let project = dir.join("project.json");
    std::fs::write(
        &project,
        r#"{"format":"edits.project/1","settings":{"width":32,"height":32,"fps":24,"duration":2},
            "slots":{"v":{"path":"clip.mkv"}},
            "layers":[{"id":"clip","kind":"video","slot":"v","in":0.5,"volume":0.5},
                      {"id":"slow","kind":"video","slot":"v","in":1,"speed":{"keys":[{"t":1,"v":1},{"t":2,"v":0.5}]},"transform":{"scale":0.5}}]}"#,
    )
    .unwrap();
    let mp4 = dir.join("out.mp4");
    let out = edits(&["render", project.to_str().unwrap(), "-o", mp4.to_str().unwrap(), "--scale", "1", "-q"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("layer \"slow\": the sound of a video with an animated speed"), "{}", String::from_utf8_lossy(&out.stderr));
    let v = json(&out);
    assert_eq!((v["audio"].as_bool(), v["clip_audio"].as_u64(), v["frames"].as_u64()), (Some(true), Some(1), Some(48)));
    let mp4 = mp4.to_str().unwrap();
    // Silent until the layer comes in at 0.5 s, then the tone: lavfi's sine peaks at 1/8
    // (-18 dB), and half volume takes 6 dB off.
    assert!(max_volume(mp4, 0.0, 0.4) < -60.0);
    let loud = max_volume(mp4, 0.6, 1.4);
    assert!((-27.0..-21.0).contains(&loud), "{loud} dB");
    // --no-audio leaves it out.
    let v = json(&edits(&["render", project.to_str().unwrap(), "-o", mp4, "--no-audio", "-q"]));
    assert_eq!((v["audio"].as_bool(), v["clip_audio"].as_u64()), (Some(false), Some(0)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// analyze → draft → compare through the `edits` command, on a short render of
/// the demo: the recipe of a video compared with that same video scores 1.
#[test]
fn analyze_draft_and_compare_round_trip() {
    let d = doctor();
    if !d["ok"].as_bool().unwrap_or(false) {
        eprintln!("skipped: no adapter or no H.264 encoder ({d})");
        return;
    }
    let demo = Demo::new("analyze");
    let video = demo.path("ref.mp4");
    let out = edits(&["render", &demo.path("project.json"), "-o", &video, "--to", "4", "--scale", "0.25", "--samples", "1"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let an = json(&edits(&["analyze", &video, "-o", &demo.path("an"), "--bpm", "123", "--json"]));
    assert_eq!(an["bpm"].as_f64(), Some(123.0));
    assert!(an["shots"].as_u64().unwrap() >= 2, "{an}");
    let analysis = demo.path("an/reference_analysis.json");

    let dr = json(&edits(&["draft", &analysis, "-o", &demo.path("edit/project.json"), "--proxies", "--json"]));
    assert_eq!(dr["errors"].as_u64(), Some(0), "{dr}");
    json(&edits(&["check", &demo.path("edit/project.json"), "--json"]));

    let cmp = json(&edits(&["compare", &analysis, &video, "--json"]));
    let scores = &cmp["scores"];
    for name in ["cuts", "flashes", "exposure", "blur", "saturation"] {
        assert_eq!(scores[name].as_f64(), Some(1.0), "{name}: {cmp}");
    }

    let probe = json(&edits(&["probe", &video, "--json"]));
    assert_eq!(probe["frames"].as_array().unwrap().len(), 49);
}

#[test]
fn gui_and_mcp_say_how_to_build_a_missing_binary() {
    for (sub, var, krate) in [("gui", "EDITS_GUI_BIN", "edits-gui"), ("mcp", "EDITS_MCP_BIN", "edits-mcp")] {
        let out = Command::new(env!("CARGO_BIN_EXE_edits")).args([sub, "x"]).env(var, "/nonexistent/binary").output().unwrap();
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.starts_with("error:") && err.contains(&format!("cargo build --release -p {krate}")), "{err}");
    }
}
