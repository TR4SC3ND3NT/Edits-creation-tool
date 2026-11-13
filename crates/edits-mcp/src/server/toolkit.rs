//! The tools for making an edit from a song and pictures: song analysis,
//! presets, beat operations, custom effects and the window's context.
//!
//! As everywhere in this server: the analysis and the shader check run the
//! `edits` command; changes to a project go through `edits_core::edit` (one
//! validated, undoable step, with files outside the workspace refused), using
//! the same `edits_core` functions as `edits apply` and `edits beats`.

use super::{dir_of, os, Out, Reply, TimeArg};
use crate::edit;
use crate::exec::Tool;
use edits_core::beatops::{self, CutOpts, Place};
use edits_core::check::Severity;
use edits_core::edit::EditOutcome;
use edits_core::preset::{self, Target};
use edits_core::song::SongInfo;
use edits_core::time::TimeCtx;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioAnalyzeArgs {
    /// A song or a video with a soundtrack.
    pub input: String,
    /// Where to write the analysis (.json). Default: <input name>.audio.json next to the input.
    pub output: Option<String>,
    /// The tempo, if you know it; otherwise it is estimated.
    pub bpm: Option<f64>,
    /// Beats in a bar (default 4).
    pub beats_per_bar: Option<u32>,
    /// Also write the tempo and the markers (drop, sections) into this project, as one undoable edit; sets its audio when it has none.
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetsArgs {
    /// One preset, to see all its variables. Omit for the list.
    pub name: Option<String>,
    /// Only presets with this tag: hit, light, zoom, camera, move, 3d, blur, colour, glitch, transition, look...
    pub tag: Option<String>,
    /// Also list the presets saved in this project's presets/ folder.
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetApplyArgs {
    /// Project file (.json).
    pub project: String,
    /// Preset name from presets_list, e.g. "flash", "punch", "transition:whip_pan".
    pub preset: String,
    /// The layer it changes: an id, "auto" (default: the picture on screen at that time) or "fx" (an `accents` adjustment layer on top, for effects on everything). For transition:<kind>, the incoming layer.
    pub layer: Option<String>,
    /// When; several separated by commas ("b4, b8, drop"). Default: the layer's in (its cut).
    pub at: Option<TimeArg>,
    /// The preset's variables, e.g. {"strength": 0.7, "color": [1, 0.2, 0.2]}.
    pub vars: Option<Map<String, Value>>,
    /// Song analysis from audio_analyze, for its markers and bars in `at`.
    pub audio: Option<String>,
    /// Report what would change without writing.
    pub dry_run: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BeatsCutArgs {
    /// Project file (.json).
    pub project: String,
    /// Slots to cycle through, in order, e.g. ["a", "b", "c"].
    pub slots: Vec<String>,
    /// Beats per picture: 1 (default), 2, 4 (a bar), 0.5.
    pub every: Option<f64>,
    /// Cut on these instead of a fixed step: "beats", "offbeats", "downbeats", "kicks", "snares", "hats" (onsets need `audio`), or a list of times.
    pub on: Option<String>,
    /// Start of the range. Default: the start.
    pub from: Option<TimeArg>,
    /// End of the range; the project grows when this is past its end. Default: the end.
    pub to: Option<TimeArg>,
    /// Song analysis from audio_analyze.
    pub audio: Option<String>,
    /// Onsets weaker than this (0..1) are skipped (default 0.3).
    pub min_strength: Option<f64>,
    /// Layer ids are <prefix>1, <prefix>2... (default "cut").
    pub prefix: Option<String>,
    /// Put the new layers directly above this layer. Default: on top, below the adjustment layers at the top.
    pub above: Option<String>,
    /// Put the new layers directly below this layer.
    pub below: Option<String>,
    /// Report what would change without writing.
    pub dry_run: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BeatsAccentArgs {
    /// Project file (.json).
    pub project: String,
    /// Preset to apply at each time, e.g. "flash", "punch", "shake", "rgb_split".
    pub preset: String,
    /// "beats", "offbeats", "downbeats", "kicks", "snares", "hats", "drop" (onsets and the drop need `audio`), or a list of times.
    pub on: String,
    /// Song analysis from audio_analyze.
    pub audio: Option<String>,
    /// "auto" (default: the picture on screen at each time), "fx" (one adjustment layer on top) or a layer id.
    pub layer: Option<String>,
    /// The preset's variables.
    pub vars: Option<Map<String, Value>>,
    pub from: Option<TimeArg>,
    pub to: Option<TimeArg>,
    /// Onsets weaker than this (0..1) are skipped (default 0.3).
    pub min_strength: Option<f64>,
    /// Only every Nth time (2 = every other one).
    pub every_nth: Option<u32>,
    /// Report what would change without writing.
    pub dry_run: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectDefineArgs {
    /// Project file (.json).
    pub project: String,
    /// Name of the effect: letters, digits, - and _. The shader is written to shaders/<name>.wgsl next to the project.
    pub name: String,
    /// The WGSL source: must define fn effect(uv: vec2<f32>) -> vec4<f32>. src(uv) reads the picture (linear, premultiplied), fx.p0..fx.p15 are the parameters, fx.time / fx.beats / fx.frame / fx.aspect / fx.resolution the inputs, fx_to_comp / fx_to_uv / fx_hash / fx_noise helpers.
    pub wgsl: String,
    /// Values for p0..p15 (numbers, or keyframed values).
    pub params: Option<Map<String, Value>>,
    /// Add it to this layer's effects. Default: a new adjustment layer named after the effect, on top, so it applies to everything.
    pub layer: Option<String>,
}

fn ctx(doc: &Value, song: Option<&SongInfo>) -> TimeCtx {
    let tc = TimeCtx::from_doc(doc);
    match song {
        Some(s) => tc.with_song(s, doc.pointer("/audio/offset").and_then(Value::as_f64).unwrap_or(0.0)),
        None => tc,
    }
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

impl super::EditsServer {
    fn song(&self, audio: &Option<String>) -> Result<Option<SongInfo>, String> {
        audio.as_ref().map(|a| self.ws.file(a, "audio").and_then(|p| SongInfo::load(&p))).transpose()
    }

    fn doc(&self, project: &Path) -> Result<Value, String> {
        Ok(self.load(project)?.0)
    }

    /// One change to a project as one step, reported like project_edit.
    fn step(&self, project: &Path, label: &str, dry_run: bool, head: &RefCell<Vec<String>>, change: &dyn Fn(&Value) -> Result<Value, String>) -> Out {
        let outcome = {
            let _one_at_a_time = self.writing.lock().unwrap_or_else(|e| e.into_inner());
            edit::edit_with(project, label, dry_run, &self.ws, change)?
        };
        Ok(self.outcome_reply(project, &outcome, head.borrow().clone()))
    }

    fn outcome_reply(&self, project: &Path, o: &EditOutcome, head: Vec<String>) -> Reply {
        let shown = self.ws.show(project);
        let mut text = if !o.ok {
            let errors = o.findings.iter().filter(|f| f.severity == Severity::Error).count();
            format!("NOT applied: the result would have {errors} error(s), so {shown} is unchanged.")
        } else if o.changes.is_empty() {
            format!("no changes: {shown} already has these values")
        } else if o.dry_run {
            format!("dry run, nothing written: {} value(s) of {shown} would change", o.changes.len())
        } else {
            format!("applied to {shown}: {} change(s) as one step (project_undo takes it back)", o.changes.len())
        };
        for line in &head {
            text.push_str(&format!("\n{line}"));
        }
        for f in &o.findings {
            text.push_str(&format!("\n{}", edit::finding_line(f)));
        }
        let structured = json!({
            "ok": o.ok,
            "written": o.written,
            "dry_run": o.dry_run,
            "changes": o.changes.len(),
            "findings": o.findings,
            "backup": o.backup.as_ref().map(|b| self.ws.show(b)),
        });
        Reply { text, image: None, structured: Some(structured), failed: !o.ok }
    }

    // ---- song ----

    pub(super) async fn audio_analyze(&self, a: AudioAnalyzeArgs) -> Out {
        let input = self.ws.file(&a.input, "input")?;
        let output = match &a.output {
            Some(o) => self.ws.resolve(o, "output")?,
            None => {
                let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "song".into());
                self.ws.resolve(&dir_of(&input).join(format!("{stem}.audio.json")).to_string_lossy(), "output")?
            }
        };
        let mut args: Vec<OsString> = vec!["audio".into(), input.clone().into(), "-o".into(), output.clone().into(), "--json".into()];
        if let Some(b) = super::finite(a.bpm, "bpm")? {
            args.push(format!("--bpm={b}").into());
        }
        if let Some(n) = a.beats_per_bar {
            args.extend(os(&["--beats-per-bar", &n.to_string()]));
        }
        if let Some(p) = &a.project {
            let p = self.ws.file(p, "project")?;
            args.push("--project".into());
            args.push(p.into());
        }
        let info = self.run_json(Tool::Edits, args).await?;
        let mut text = format!(
            "{}: {} s, {} BPM (confidence {}), beat 0 at {} s, {} beats in {} bars (bar 1 starts on beat {}); onsets: {} kick, {} snare, {} hat",
            self.ws.show(&input),
            info["duration"],
            info["bpm"],
            info["confidence"],
            info["beat_offset"],
            info["beats"],
            info["bars"],
            info["downbeat"],
            info["onsets"]["kick"],
            info["onsets"]["snare"],
            info["onsets"]["hat"]
        );
        match info["drop"].as_object() {
            Some(d) => text.push_str(&format!("\ndrop at {} s (beat {}, bar {}), energy rise {}", d["t"], d["b"], d["bar"], d["rise"])),
            None => text.push_str("\nno drop: the energy does not rise sharply anywhere"),
        }
        let sections: Vec<String> = info["sections"].as_array().into_iter().flatten().map(|s| format!("{} {}-{} s (bar {}, energy {})", s["label"].as_str().unwrap_or("?"), s["from"], s["to"], s["bar"], s["energy"])).collect();
        if !sections.is_empty() {
            text.push_str(&format!("\nsections: {}", sections.join("; ")));
        }
        for n in info["notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            text.push_str(&format!("\nnote: {n}"));
        }
        if let Some(s) = info["project"]["summary"].as_str() {
            text.push_str(&format!("\nproject: {}", s.replace(&self.root().display().to_string(), ".")));
        }
        text.push_str(&format!(
            "\nwritten to {} (beats, downbeats, onsets with strengths, energy per beat). Pass it as `audio` to beats_cut, beats_accent and preset_apply; times like \"drop\", \"bar 9\", \"b16\" then work.",
            self.ws.show(&output)
        ));
        let mut data = info;
        data["analysis"] = json!(self.ws.show(&output));
        Ok(Reply::data(text, data))
    }

    // ---- presets ----

    pub(super) async fn presets_list(&self, a: PresetsArgs) -> Out {
        let mut args: Vec<OsString> = vec!["presets".into(), "--json".into()];
        if let Some(n) = &a.name {
            args.insert(1, n.into());
        }
        if let Some(t) = &a.tag {
            args.extend(os(&["--tag", t]));
        }
        if let Some(p) = &a.project {
            args.push("--project".into());
            args.push(self.ws.file(p, "project")?.into());
        }
        let data = self.run_json(Tool::Edits, args).await?;
        let line = |p: &Value| -> String {
            let vars: Vec<String> = p["variables"].as_object().into_iter().flatten().map(|(k, v)| format!("{k}={}", crate::jsonfmt::inline(&v["default"]))).collect();
            format!("{}: {} [{}] {}", p["name"].as_str().unwrap_or("?"), p["description"].as_str().unwrap_or(""), p["tags"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(","), vars.join(" "))
        };
        if a.name.is_some() {
            let p = &data["preset"];
            let mut text = line(p);
            for (k, v) in p["variables"].as_object().into_iter().flatten() {
                let range = match (v["min"].as_f64(), v["max"].as_f64()) {
                    (Some(lo), Some(hi)) => format!(", {lo}..{hi}"),
                    _ => String::new(),
                };
                let options = v["options"].as_object().map(|o| format!(": {}", o.keys().cloned().collect::<Vec<_>>().join("|"))).unwrap_or_default();
                let unit = v["unit"].as_str().map(|u| format!(", {u}")).unwrap_or_default();
                text.push_str(&format!("\n  {k} ({}{options}, default {}{range}{unit}) {}", v["type"].as_str().unwrap_or("?"), crate::jsonfmt::inline(&v["default"]), v["doc"].as_str().unwrap_or("")));
            }
            if let Some(c) = p["credit"].as_str() {
                text.push_str(&format!("\n  from: {c}"));
            }
            return Ok(Reply::data(text, data));
        }
        let list = data.as_array().cloned().unwrap_or_default();
        let (transitions, others): (Vec<&Value>, Vec<&Value>) = list.iter().partition(|p| p["name"].as_str().is_some_and(|n| n.starts_with("transition:")));
        let mut text = String::from("Presets expand into ordinary keys, effects and layers (undoable). Apply one with preset_apply {project, preset, layer, at, vars}, or on every beat with beats_accent.\n");
        for p in &others {
            text.push_str(&format!("{}\n", line(p)));
        }
        if !transitions.is_empty() {
            let names: Vec<&str> = transitions.iter().filter_map(|p| p["name"].as_str()).collect();
            text.push_str(&format!(
                "transitions (preset_apply on the incoming layer; it overlaps the cut; vars: duration in beats, align center|after|before, ease, and the transition's own parameters; presets_list with a name shows them): {}",
                names.join(", ")
            ));
        }
        Ok(Reply::data(text.trim_end(), json!({ "presets": list })))
    }

    pub(super) async fn preset_apply(&self, a: PresetApplyArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let doc = self.doc(&project)?;
        let song = self.song(&a.audio)?;
        let tc = ctx(&doc, song.as_ref());
        let p = preset::find(&a.preset, Some(&dir_of(&project)))?;
        let vars = a.vars.clone().unwrap_or_default();
        let layer = a.layer.clone().unwrap_or_else(|| "auto".into());
        let target = if p.target == preset::TargetKind::Project && layer == "auto" { Target::None } else { Target::parse(Some(&layer)) };
        let times: Vec<f64> = match &a.at {
            Some(TimeArg::Text(list)) => list.split(',').map(|t| tc.resolve(t).map_err(|e| format!("`at`: {e}"))).collect::<Result<_, _>>()?,
            Some(t) => vec![t.resolve(&tc, "at")?],
            None => match &target {
                Target::Layer(id) => vec![doc["layers"].as_array().into_iter().flatten().find(|l| l["id"] == id.as_str()).map(|l| l["in"].as_f64().unwrap_or(0.0)).ok_or_else(|| format!("no layer has id \"{id}\""))?],
                _ => return Err("give `at` (when to apply it), or a `layer` whose start it applies at".into()),
            },
        };
        let label = format!("preset {} at {} on {layer}", p.name, times.iter().map(|t| r3(*t).to_string()).collect::<Vec<_>>().join(","));
        let head = RefCell::new(Vec::new());
        let reply = self.step(&project, &label, a.dry_run.unwrap_or(false), &head, &|d| {
            let (out, applied) = preset::apply(d, &p, &target, &times, &vars, &tc)?;
            *head.borrow_mut() = applied.iter().map(|x| format!("at {}: {}", r3(x.at), x.changes.join("; "))).collect();
            Ok(out)
        })?;
        Ok(reply)
    }

    // ---- beats ----

    pub(super) async fn beats_cut(&self, a: BeatsCutArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let doc = self.doc(&project)?;
        let song = self.song(&a.audio)?;
        let tc = ctx(&doc, song.as_ref());
        let from = a.from.as_ref().map(|t| t.resolve(&tc, "from")).transpose()?.unwrap_or(0.0);
        let to = a.to.as_ref().map(|t| t.resolve(&tc, "to")).transpose()?.unwrap_or(tc.duration);
        let offset = doc.pointer("/audio/offset").and_then(Value::as_f64).unwrap_or(0.0);
        let times = a.on.as_deref().map(|on| beatops::times(on, song.as_ref(), &tc, from, to, a.min_strength.unwrap_or(0.3), offset)).transpose()?;
        if a.above.is_some() && a.below.is_some() {
            return Err("give `above` or `below`, not both".into());
        }
        let place = match (&a.above, &a.below) {
            (Some(id), _) => Place::Above(id.clone()),
            (_, Some(id)) => Place::Below(id.clone()),
            _ => Place::Default,
        };
        let opts = CutOpts { slots: a.slots.clone(), times, every: a.every.unwrap_or(1.0), from, to, prefix: a.prefix.clone().unwrap_or_else(|| "cut".into()), place };
        let label = format!("beats cut {} from {} to {}", a.slots.join(","), r3(from), r3(to));
        let head = RefCell::new(Vec::new());
        self.step(&project, &label, a.dry_run.unwrap_or(false), &head, &|d| {
            let (out, rep) = beatops::cut(d, &opts, &tc)?;
            let list: Vec<String> = rep.layers.iter().map(|(id, slot, i, o)| format!("{id}={slot}@{}..{}", r3(*i), r3(*o))).collect();
            let mut lines = vec![format!("{} image layer(s): {}", rep.layers.len(), list.join(" "))];
            if let Some(d) = rep.extended_to {
                lines.push(format!("the project now runs to {d}"));
            }
            *head.borrow_mut() = lines;
            Ok(out)
        })
    }

    pub(super) async fn beats_accent(&self, a: BeatsAccentArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        let doc = self.doc(&project)?;
        let song = self.song(&a.audio)?;
        let tc = ctx(&doc, song.as_ref());
        let from = a.from.as_ref().map(|t| t.resolve(&tc, "from")).transpose()?.unwrap_or(0.0);
        let to = a.to.as_ref().map(|t| t.resolve(&tc, "to")).transpose()?.unwrap_or(tc.duration);
        let offset = doc.pointer("/audio/offset").and_then(Value::as_f64).unwrap_or(0.0);
        let mut times = beatops::times(&a.on, song.as_ref(), &tc, from, to, a.min_strength.unwrap_or(0.3), offset)?;
        if let Some(n) = a.every_nth.filter(|n| *n > 1) {
            times = times.into_iter().step_by(n as usize).collect();
        }
        if times.is_empty() {
            return Err(format!("no {} between {} and {}", a.on, r3(from), r3(to)));
        }
        let p = preset::find(&a.preset, Some(&dir_of(&project)))?;
        let vars = a.vars.clone().unwrap_or_default();
        let target = Target::parse(a.layer.as_deref());
        let label = format!("beats accent {} on {} ({} times)", p.name, a.on, times.len());
        let head = RefCell::new(Vec::new());
        self.step(&project, &label, a.dry_run.unwrap_or(false), &head, &|d| {
            let (out, applied) = preset::apply(d, &p, &target, &times, &vars, &tc)?;
            let list: Vec<String> = applied.iter().map(|x| format!("{}@{}", x.layer.as_deref().unwrap_or("-"), r3(x.at))).collect();
            *head.borrow_mut() = vec![format!("{} on {} time(s): {}", p.name, applied.len(), list.join(" "))];
            Ok(out)
        })
    }

    // ---- custom effects ----

    pub(super) async fn effect_define(&self, a: EffectDefineArgs) -> Out {
        let project = self.ws.file(&a.project, "project")?;
        if a.name.is_empty() || !a.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err("`name` must be letters, digits, - and _ (it becomes shaders/<name>.wgsl)".into());
        }
        if a.wgsl.len() > 64 * 1024 {
            return Err("the shader is over 64 KB; a custom effect is one function".into());
        }
        // Check it before anything is written next to the project.
        let scratch = edit::state_dir(self.root(), "shaders").map_err(|e| format!("cannot create a scratch folder: {e}"))?.join(format!("{}-{}.wgsl", a.name, std::process::id()));
        std::fs::write(&scratch, &a.wgsl).map_err(|e| format!("cannot write {}: {e}", scratch.display()))?;
        let checked = self.run(Tool::Edits, vec!["shader".into(), "check".into(), scratch.clone().into(), "--json".into()]).await;
        let _ = std::fs::remove_file(&scratch);
        let out = checked?;
        let report: Value = serde_json::from_str(&out.stdout).map_err(|_| out.error_text())?;
        if report["ok"] != json!(true) {
            let mut text = format!("the shader \"{}\" does not compile, so nothing was written:", a.name);
            for i in report["issues"].as_array().into_iter().flatten() {
                let at = match (i["line"].as_u64(), i["column"].as_u64()) {
                    (Some(l), Some(c)) => format!("line {l}, column {c}: "),
                    _ => String::new(),
                };
                text.push_str(&format!("\n{at}{}", i["message"].as_str().unwrap_or("")));
                if let Some(code) = i["code"].as_str() {
                    text.push_str(&format!("\n    {code}"));
                }
            }
            return Ok(Reply { text, image: None, structured: Some(report), failed: true });
        }
        let rel = format!("shaders/{}.wgsl", a.name);
        let file: PathBuf = dir_of(&project).join(&rel);
        let file = self.ws.resolve(&file.to_string_lossy(), "shader")?;
        let replaced = file.is_file();
        std::fs::create_dir_all(dir_of(&file)).map_err(|e| format!("cannot create {}: {e}", dir_of(&file).display()))?;
        std::fs::write(&file, &a.wgsl).map_err(|e| format!("cannot write {}: {e}", self.ws.show(&file)))?;
        let mut effect = json!({ "kind": "custom", "source": rel });
        if let Some(p) = &a.params {
            effect["params"] = Value::Object(p.clone());
        }
        let name = a.name.clone();
        let layer = a.layer.clone();
        let head = RefCell::new(vec![format!(
            "{} {} (reads {}{})",
            if replaced { "replaced" } else { "wrote" },
            self.ws.show(&file),
            report["params"].as_array().map(|p| p.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).filter(|s| !s.is_empty()).unwrap_or_else(|| "no parameters".into()),
            if report["animated"] == json!(true) { "; changes with time" } else { "" }
        )]);
        let label = format!("custom effect {name}");
        let reply = self.step(&project, &label, false, &head, &|d| {
            let mut d = d.clone();
            let layers = d.get_mut("layers").and_then(Value::as_array_mut).ok_or("the project has no \"layers\"")?;
            match &layer {
                Some(id) => {
                    let l = layers.iter_mut().find(|l| l["id"] == id.as_str()).ok_or_else(|| format!("no layer has id \"{id}\""))?;
                    if l.get("effects").is_none() {
                        l["effects"] = json!([]);
                    }
                    l["effects"].as_array_mut().ok_or("\"effects\" is not an array")?.push(effect.clone());
                }
                None => {
                    let mut id = name.clone();
                    let mut n = 2;
                    while layers.iter().any(|l| l["id"] == id.as_str()) {
                        id = format!("{name}-{n}");
                        n += 1;
                    }
                    layers.push(json!({ "id": id, "kind": "adjustment", "effects": [effect.clone()] }));
                }
            }
            Ok(d)
        })?;
        Ok(reply)
    }

    // ---- the window ----

    pub(super) async fn ui_context(&self, project: &str) -> Out {
        let project = self.ws.file(project, "project")?;
        match edits_core::ui::read(&project)? {
            edits_core::ui::Found::Closed => Ok(Reply::data(
                format!("the window is not open on {} (no .edits/ui.json); the person can open it with `edits gui {}`", self.ws.show(&project), self.ws.show(&project)),
                json!({ "open": false }),
            )),
            edits_core::ui::Found::Open { ctx, age, stale } => {
                let mut text = if stale {
                    format!("the window seems closed: it last wrote its state {:.0} s ago. Its last state:", age.as_secs_f64())
                } else {
                    format!("the window is open on {} (state from {:.1} s ago):", self.ws.show(&project), age.as_secs_f64())
                };
                if let Some(p) = &ctx.playhead {
                    text.push_str(&format!("\nplayhead at {}{}{}", r3(p.t), p.seconds.map(|s| format!(" ({} s)", r3(s))).unwrap_or_default(), p.frame.map(|f| format!(", frame {f}")).unwrap_or_default()));
                }
                if ctx.playing == Some(true) {
                    text.push_str(", playing");
                }
                if let Some(s) = &ctx.selection {
                    let parts: Vec<String> = [s.layer.as_ref().map(|l| format!("layer {l}")), s.effect.map(|e| format!("effect {e}")), s.property.clone(), s.key.map(|k| format!("key {k}"))].into_iter().flatten().collect();
                    if !parts.is_empty() {
                        text.push_str(&format!("\nselected: {}", parts.join(", ")));
                    }
                }
                if let Some(v) = &ctx.visible {
                    text.push_str(&format!("\nvisible on the timeline: {}..{}", r3(v.from), r3(v.to)));
                }
                if let Some(l) = &ctx.loop_range {
                    text.push_str(&format!("\nlooping {}..{}", r3(l.from), r3(l.to)));
                }
                if let Some(n) = ctx.note.as_deref().filter(|n| !n.trim().is_empty()) {
                    text.push_str(&format!("\nnote from the person: \"{n}\""));
                }
                let mut data = serde_json::to_value(&ctx).map_err(|e| e.to_string())?;
                data["open"] = json!(!stale);
                data["age_seconds"] = json!(r3(age.as_secs_f64()));
                Ok(Reply::data(text, data))
            }
        }
    }
}
