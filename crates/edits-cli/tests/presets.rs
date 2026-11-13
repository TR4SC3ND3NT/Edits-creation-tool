//! Every preset (built-in and `transition:<kind>`) applied to the demo
//! project: the result passes `check`, renders, and looks different from the
//! demo without it. Then the commands an agent uses: `audio`, `beats cut`,
//! `beats accent`, `apply` and `save-preset`, end to end. Rendering is
//! skipped (with a note) on a machine without a graphics adapter.

use edits_core::preset::{self, Target, TargetKind};
use edits_core::time::TimeCtx;
use edits_core::{check, Project};
use edits_render::{RenderOptions, Renderer};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn edits(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_edits")).args(args).output().expect("the edits binary runs")
}

fn json_of(out: &Output) -> Value {
    assert!(out.status.success(), "command failed: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

struct Demo(PathBuf);
impl Demo {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("edits-presets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        json_of(&edits(&["demo", dir.to_str().unwrap()]));
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

fn mean_diff(a: &image::RgbaImage, b: &image::RgbaImage) -> f64 {
    let n = (a.width() * a.height() * 3) as f64;
    a.pixels().zip(b.pixels()).map(|(p, q)| (0..3).map(|i| (p[i] as f64 - q[i] as f64).abs()).sum::<f64>()).sum::<f64>() / n
}

/// Where each preset is tried on the demo (layers a 0..2, cube 2..4, d 4..8).
fn place(p: &preset::Preset) -> (Target, f64) {
    if p.target == TargetKind::Project {
        return (Target::None, 5.0);
    }
    if p.name.starts_with("transition:") {
        // The cut from the cube into d at beat 4.
        return (Target::Layer("d".into()), 4.0);
    }
    if p.name == "cube_turn" {
        return (Target::Layer("cube".into()), 2.5);
    }
    if p.layer_kind.as_deref() == Some("text") {
        // The caption added by `with_caption`, found by `auto`.
        return (Target::Auto, 4.5);
    }
    (Target::Layer("d".into()), 5.0)
}

/// The demo with a caption over its pictures from beat 4 to 8, for the
/// presets that animate letters.
fn with_caption(doc: &Value) -> Value {
    let mut doc = doc.clone();
    let caption = json!({ "id": "caption", "kind": "text", "text": "SAY MY\nNAME", "in": 4, "out": 8, "size": 18, "weight": 800,
                          "stroke": { "color": [0, 0, 0], "width": 0.6 } });
    doc["layers"].as_array_mut().unwrap().push(caption);
    doc
}

#[test]
fn every_preset_applies_to_the_demo_checks_and_renders() {
    let demo = Demo::new("all");
    let dir = demo.0.clone();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    let tc = TimeCtx::from_doc(&doc);
    let mut renderer = match Renderer::new(RenderOptions::default()) {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("skipping the renders: no graphics adapter ({e:#})");
            None
        }
    };
    let plain = Project::from_json(&text).unwrap();
    let captioned: Project = serde_json::from_value(with_caption(&doc)).unwrap();
    let all = preset::builtin();
    assert!(all.len() >= 40, "{} presets", all.len());
    for p in &all {
        let (target, at) = place(p);
        let text = p.layer_kind.as_deref() == Some("text");
        let (doc, original) = if text { (with_caption(&doc), &captioned) } else { (doc.clone(), &plain) };
        let (out, applied) = preset::apply(&doc, p, &target, &[at], &Map::new(), &tc).unwrap_or_else(|e| panic!("{}: {e}", p.name));
        assert!(!applied[0].changes.is_empty(), "{} changed nothing", p.name);
        let project: Project = serde_json::from_value(out.clone()).unwrap_or_else(|e| panic!("{}: the result does not parse: {e}", p.name));
        let findings = check::check(&project, Some(&dir));
        let errors: Vec<_> = findings.iter().filter(|f| f.severity == check::Severity::Error).collect();
        assert!(errors.is_empty(), "{}: {errors:#?}", p.name);
        let Some(r) = renderer.as_mut() else { continue };
        // Moments inside what the preset does: just after a hit, or further
        // into something that builds up (a blur out, a push).
        let offsets: &[f64] = if p.name.starts_with("transition:") { &[0.0] } else { &[0.2, 0.6] };
        let mut best = 0.0f64;
        for dt in offsets {
            let frame = (project.to_seconds(at + dt) * project.settings.fps).floor() as u64;
            r.set_motion_blur_samples(Some(1));
            r.prepare(original, &dir).unwrap();
            let before = r.render_frame(frame, 0.1).unwrap();
            r.prepare(&project, &dir).unwrap_or_else(|e| panic!("{}: {e:#}", p.name));
            let after = r.render_frame(frame, 0.1).unwrap_or_else(|e| panic!("{}: {e:#}", p.name));
            best = best.max(mean_diff(&before, &after));
        }
        assert!(best > 0.15, "{} at {at} looks the same as without it (difference {best:.3})", p.name);
    }
}

#[test]
fn audio_beats_and_presets_from_the_command_line() {
    let demo = Demo::new("cli");
    let project = demo.path("project.json");
    let song = demo.path("song.json");

    // The click track: 123 BPM, written into the project.
    let a = json_of(&edits(&["audio", &demo.path("audio/beat.wav"), "-o", &song, "--project", &project, "--json"]));
    assert_eq!(a["bpm"], json!(123.0), "{a}");
    assert!(a["onsets"]["kick"].as_u64().unwrap() >= 7, "{a}");
    let analysis: Value = serde_json::from_str(&std::fs::read_to_string(&song).unwrap()).unwrap();
    assert_eq!(analysis["format"], "edits.audio/1");
    assert!(analysis["beats"].as_array().unwrap().len() >= 8);

    // Sixteen beats of cuts, below the overlay.
    let cut = json_of(&edits(&["beats", "cut", &project, "--slots", "a,b,c,d", "--every", "1", "--from", "0", "--to", "16", "--below", "tex", "--json"]));
    assert_eq!(cut["layers"].as_array().unwrap().len(), 16, "{cut}");
    assert_eq!(cut["extended_to"], json!(16.0));
    // Accents on the kicks of the song, and on bars.
    let acc = json_of(&edits(&["beats", "accent", &project, "--preset", "punch", "--on", "kicks", "--audio", &song, "--json"]));
    assert!(acc["times"].as_array().unwrap().len() >= 7, "{acc}");
    let acc = json_of(&edits(&["beats", "accent", &project, "--preset", "flash", "--on", "downbeats", "--var", "strength=0.6", "--json"]));
    assert_eq!(acc["times"], json!([0.0, 4.0, 8.0, 12.0]), "{acc}");
    // A transition at the cut into cut9, a preset at a bar.
    json_of(&edits(&["apply", &project, "transition:cross_zoom", "--layer", "cut9", "--json"]));
    let applied = json_of(&edits(&["apply", &project, "shake", "--layer", "cut13", "--at", "bar 4", "--json"]));
    assert_eq!(applied["applied"][0]["at"], json!(12.0), "{applied}");
    let check = json_of(&edits(&["check", &project, "--json"]));
    assert_eq!(check["errors"], json!(0), "{check}");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let cut9 = doc["layers"].as_array().unwrap().iter().find(|l| l["id"] == "cut9").unwrap();
    assert_eq!(cut9["transition"]["kind"], "cross_zoom");
    assert_eq!(cut9["in"], json!(7.75));

    // Mistakes are refused with what to do instead.
    let bad = edits(&["apply", &project, "flsh", "--layer", "cut1", "--at", "1"]);
    assert!(!bad.status.success());
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("no preset \"flsh\"") && err.contains("flash"), "{err}");
    let bad = edits(&["apply", &project, "flash", "--layer", "cut1", "--at", "dorp"]);
    assert!(String::from_utf8_lossy(&bad.stderr).contains("not a time or a marker"));

    // Save what a layer does as a preset, then use it elsewhere.
    let saved = json_of(&edits(&["save-preset", &project, "--layer", "cut13", "--from", "12", "--to", "13", "--name", "my_shake", "--json"]));
    assert!(Path::new(saved["file"].as_str().unwrap()).is_file());
    let listed = json_of(&edits(&["presets", "--project", &project, "--json"]));
    assert!(listed.as_array().unwrap().iter().any(|p| p["name"] == "my_shake"));
    json_of(&edits(&["apply", &project, "my_shake", "--layer", "cut2", "--at", "1", "--json"]));

    // Times resolve in the render commands too.
    let doctor: Value = serde_json::from_slice(&edits(&["doctor", "--json"]).stdout).unwrap();
    if doctor["adapter"].is_null() {
        eprintln!("skipping the render check: no graphics adapter");
        return;
    }
    let f = json_of(&edits(&["frame", &project, "--at", "bar 3+0.5b", "-o", &demo.path("f.png"), "--scale", "0.1"]));
    // The frame that contains beat 8.5 (frames are 0.082 beats long at 123 BPM and 25 fps).
    let time = f["time"].as_f64().unwrap();
    assert!(time <= 8.5 && time > 8.5 - 0.083, "{f}");
    let s = json_of(&edits(&["sheet", &project, "--from", "b7.5", "--to", "b8.5", "--count", "4", "--scale", "0.1", "--samples", "1", "-o", &demo.path("s.png")]));
    assert_eq!(s["frames"].as_array().unwrap().len(), 4);
}
