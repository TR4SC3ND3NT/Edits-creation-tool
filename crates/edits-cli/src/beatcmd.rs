//! `edits presets`, `apply`, `save-preset`, `beats cut`, `beats accent` and
//! `transitions`: the high-level editing commands. Each change goes through
//! `edits_core::edit::apply_with`, so it is validated, written atomically and
//! recorded as one step of the project's history (`project_undo` in the MCP
//! server, or the window's undo, takes it back).

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand};
use edits_core::beatops::{self, CutOpts, Place};
use edits_core::check::Severity;
use edits_core::edit::{self, EditOptions, EditOutcome};
use edits_core::preset::{self, Preset, Target};
use edits_core::song::SongInfo;
use edits_core::time::TimeCtx;
use edits_core::{effects, jsonfmt};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Args)]
pub struct PresetsArgs {
    /// Show only this preset, with every variable
    pub name: Option<String>,
    /// Also list the presets in this project's `presets/` folder
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// Only presets with this tag (hit, light, zoom, move, 3d, transition, ...)
    #[arg(long)]
    pub tag: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ApplyArgs {
    /// Project file (.json)
    pub project: PathBuf,
    /// Preset name (see `edits presets`), or a preset file
    pub preset: String,
    /// The layer it changes: an id, `auto` (the picture on screen at each time) or `fx` (an `accents` adjustment layer on top)
    #[arg(long, default_value = "auto")]
    pub layer: String,
    /// When: project time, b12.5, bar 4, 4:3, 2.5s, f60, a marker such as drop, drop+2b; several separated by commas [default: the layer's in]
    #[arg(long, value_name = "T")]
    pub at: Option<String>,
    /// A variable, name=value (repeat for more)
    #[arg(long = "var", value_name = "NAME=VALUE")]
    pub vars: Vec<String>,
    /// Song analysis from `edits audio`, for its markers and bars in --at
    #[arg(long)]
    pub audio: Option<PathBuf>,
    /// Validate and report what would change; write nothing
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct SavePresetArgs {
    /// Project file (.json)
    pub project: PathBuf,
    /// The layer whose keys and effects become the preset
    #[arg(long)]
    pub layer: String,
    /// Start of the selection (its keys become offsets from the time the preset is applied at)
    #[arg(long, value_name = "T")]
    pub from: String,
    /// End of the selection
    #[arg(long, value_name = "T")]
    pub to: String,
    /// Name of the new preset (written to <project dir>/presets/<name>.json)
    #[arg(long)]
    pub name: String,
    #[arg(long, default_value = "Saved from a project.")]
    pub description: String,
    /// Replace a preset file of the same name
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum BeatsCommand {
    /// One image layer per beat (or every N beats, or on each onset), cycling through slots
    Cut(CutArgs),
    /// Apply a preset on every beat, bar, kick, snare, hat or the drop
    Accent(AccentArgs),
}

#[derive(Args)]
pub struct CutArgs {
    /// Project file (.json)
    pub project: PathBuf,
    /// Slots to cycle through, e.g. a,b,c
    #[arg(long, value_delimiter = ',', required = true)]
    pub slots: Vec<String>,
    /// Beats per picture (4 = one per bar, 0.5 = two per beat)
    #[arg(long, default_value_t = 1.0, conflicts_with = "on")]
    pub every: f64,
    /// Cut on these instead: beats, offbeats, downbeats, kicks, snares, hats, or a list of times
    #[arg(long)]
    pub on: Option<String>,
    /// Start of the range [default: start]
    #[arg(long, value_name = "T")]
    pub from: Option<String>,
    /// End of the range; the project grows if this is past its end [default: end]
    #[arg(long, value_name = "T")]
    pub to: Option<String>,
    /// Song analysis from `edits audio` (needed for kicks, snares, hats; used for bars and markers)
    #[arg(long)]
    pub audio: Option<PathBuf>,
    /// Onsets weaker than this (0..1) are not cut on
    #[arg(long, default_value_t = 0.3)]
    pub min_strength: f64,
    /// Layer ids are <prefix>1, <prefix>2, ...
    #[arg(long, default_value = "cut")]
    pub prefix: String,
    /// Put the new layers directly above this layer [default: on top, below the adjustment layers at the top]
    #[arg(long, conflicts_with = "below")]
    pub above: Option<String>,
    /// Put the new layers directly below this layer
    #[arg(long)]
    pub below: Option<String>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct AccentArgs {
    /// Project file (.json)
    pub project: PathBuf,
    /// Preset to apply at each time (see `edits presets`)
    #[arg(long)]
    pub preset: String,
    /// beats, offbeats, downbeats, kicks, snares, hats, drop, or a list of times
    #[arg(long)]
    pub on: String,
    /// Song analysis from `edits audio`
    #[arg(long)]
    pub audio: Option<PathBuf>,
    /// The layer: an id, `auto` (the picture on screen at each time) or `fx` (an `accents` adjustment layer on top)
    #[arg(long, default_value = "auto")]
    pub layer: String,
    /// A variable of the preset, name=value (repeat for more)
    #[arg(long = "var", value_name = "NAME=VALUE")]
    pub vars: Vec<String>,
    #[arg(long, value_name = "T")]
    pub from: Option<String>,
    #[arg(long, value_name = "T")]
    pub to: Option<String>,
    /// Onsets weaker than this (0..1) are skipped
    #[arg(long, default_value_t = 0.3)]
    pub min_strength: f64,
    /// Only every Nth time (2 = every other beat)
    #[arg(long, default_value_t = 1)]
    pub every_nth: usize,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub json: bool,
}

fn dir_of(p: &Path) -> PathBuf {
    match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn read_doc(project: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(project).with_context(|| format!("cannot read project {}", project.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", project.display()))
}

fn load_song(path: Option<&Path>) -> Result<Option<SongInfo>> {
    path.map(|p| SongInfo::load(p).map_err(|e| anyhow!(e))).transpose()
}

/// The times of a project, with a song's markers and bars when given.
pub fn time_ctx(doc: &Value, song: Option<&SongInfo>) -> TimeCtx {
    let tc = TimeCtx::from_doc(doc);
    match song {
        Some(s) => tc.with_song(s, doc.pointer("/audio/offset").and_then(Value::as_f64).unwrap_or(0.0)),
        None => tc,
    }
}

fn resolve(tc: &TimeCtx, t: &str, flag: &str) -> Result<f64> {
    tc.resolve(t).map_err(|e| anyhow!("{flag}: {e}"))
}

/// Write `change` as one step and report it. Returns the outcome and the exit code.
fn write(project: &Path, label: &str, dry_run: bool, change: &dyn Fn(&Value) -> Result<Value, String>) -> Result<EditOutcome> {
    let opts = EditOptions { dry_run, actor: edit::AGENT, label: Some(label), ..Default::default() };
    edit::apply_with(project, &opts, change).map_err(|e| anyhow!(e))
}

fn outcome_json(o: &EditOutcome) -> Value {
    json!({
        "ok": o.ok,
        "written": o.written,
        "dry_run": o.dry_run,
        "changes": o.changes.len(),
        "changed": o.changes.iter().take(60).map(edit::change_line).collect::<Vec<_>>(),
        "findings": o.findings,
        "backup": o.backup,
    })
}

fn outcome_text(o: &EditOutcome, project: &Path) -> String {
    let mut s = if !o.ok {
        format!("NOT applied: the result would have errors, so {} is unchanged", project.display())
    } else if o.changes.is_empty() {
        "no changes".to_string()
    } else if o.dry_run {
        format!("dry run: {} value(s) would change; nothing written", o.changes.len())
    } else {
        format!("wrote {}: {} change(s) (undo restores the previous version)", project.display(), o.changes.len())
    };
    for f in &o.findings {
        s.push_str(&format!("\n{}", edit::finding_line(f)));
    }
    s
}

fn exit(o: &EditOutcome) -> ExitCode {
    if o.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---- presets ------------------------------------------------------------------

fn preset_json(p: &Preset) -> Value {
    json!({
        "name": p.name,
        "description": p.description,
        "tags": p.tags,
        "target": p.target,
        "variables": p.variables,
        "credit": p.credit,
    })
}

pub fn presets(a: PresetsArgs) -> Result<ExitCode> {
    let dir = a.project.as_deref().map(dir_of);
    if let Some(name) = &a.name {
        let p = preset::find(name, dir.as_deref()).map_err(|e| anyhow!(e))?;
        if a.json {
            crate::print_json(&json!({ "preset": preset_json(&p), "apply": p.apply }));
        } else {
            println!("{}: {}", p.name, p.description);
            println!("  tags: {}; target: {}", p.tags.join(", "), if p.target == preset::TargetKind::Project { "none (adds its own layers)" } else { "a layer (--layer id, auto or fx)" });
            for (n, v) in &p.variables {
                println!("  {}", preset::variable_line(n, v));
            }
            if let Some(c) = &p.credit {
                println!("  from: {c}");
            }
        }
        return Ok(ExitCode::SUCCESS);
    }
    let (all, problems) = preset::library(dir.as_deref());
    for e in &problems {
        eprintln!("warning: {e}");
    }
    let chosen: Vec<&Preset> = all.iter().filter(|p| a.tag.as_ref().is_none_or(|t| p.tags.iter().any(|x| x == t))).collect();
    if a.json {
        crate::print_json(&json!(chosen.iter().map(|p| preset_json(p)).collect::<Vec<_>>()));
        return Ok(ExitCode::SUCCESS);
    }
    let (transitions, others): (Vec<&&Preset>, Vec<&&Preset>) = chosen.iter().partition(|p| p.name.starts_with("transition:"));
    for p in &others {
        let vars: Vec<String> = p.variables.iter().map(|(n, v)| format!("{n}={}", jsonfmt::inline(&v.default))).collect();
        println!("{:<16} {}", p.name, p.description);
        println!("{:<16} [{}] {}", "", p.tags.join(","), vars.join(" "));
    }
    if !transitions.is_empty() {
        let names: Vec<&str> = transitions.iter().map(|p| p.name.as_str()).collect();
        println!("\ntransitions (apply on the incoming layer at its cut; `edits transitions` describes them): {}", names.join(", "));
    }
    println!("\n`edits presets <name>` shows a preset's variables; `edits apply <project> <name> --layer <id> --at <time>` applies it.");
    Ok(ExitCode::SUCCESS)
}

fn layer_in(doc: &Value, id: &str) -> Option<f64> {
    doc.get("layers")?.as_array()?.iter().find(|l| l.get("id").and_then(Value::as_str) == Some(id))?.get("in").and_then(Value::as_f64).or(Some(0.0))
}

pub fn apply(a: ApplyArgs) -> Result<ExitCode> {
    let doc = read_doc(&a.project)?;
    let song = load_song(a.audio.as_deref())?;
    let tc = time_ctx(&doc, song.as_ref());
    let p = preset::find(&a.preset, Some(&dir_of(&a.project))).map_err(|e| anyhow!(e))?;
    let vars = preset::parse_assignments(&a.vars).map_err(|e| anyhow!(e))?;
    let target = if p.target == preset::TargetKind::Project && a.layer == "auto" { Target::None } else { Target::parse(Some(&a.layer)) };
    let times: Vec<f64> = match &a.at {
        Some(list) => list.split(',').map(|t| resolve(&tc, t, "--at")).collect::<Result<_>>()?,
        None => match &target {
            Target::Layer(id) => vec![layer_in(&doc, id).ok_or_else(|| anyhow!("no layer has id \"{id}\""))?],
            _ => bail!("give --at (the time to apply the preset at)"),
        },
    };
    let label = format!("preset {} at {} on {}", p.name, times.iter().map(|t| format!("{t}")).collect::<Vec<_>>().join(","), a.layer);
    let report = std::cell::RefCell::new(Vec::new());
    let outcome = write(&a.project, &label, a.dry_run, &|d| {
        let (out, applied) = preset::apply(d, &p, &target, &times, &vars, &tc)?;
        *report.borrow_mut() = applied;
        Ok(out)
    })?;
    let applied = report.into_inner();
    if a.json {
        let mut v = outcome_json(&outcome);
        v["preset"] = json!(p.name);
        v["applied"] = json!(applied);
        crate::print_json(&v);
    } else {
        for x in &applied {
            println!("at {}: {}", x.at, x.changes.join("; "));
        }
        println!("{}", outcome_text(&outcome, &a.project));
    }
    Ok(exit(&outcome))
}

pub fn save_preset(a: SavePresetArgs) -> Result<ExitCode> {
    let doc = read_doc(&a.project)?;
    let tc = time_ctx(&doc, None);
    let (from, to) = (resolve(&tc, &a.from, "--from")?, resolve(&tc, &a.to, "--to")?);
    if !a.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!("--name must be letters, digits, - and _ (it becomes the file name)");
    }
    if preset::builtin().iter().any(|p| p.name == a.name) {
        eprintln!("warning: \"{}\" shadows the built-in preset of the same name for this project", a.name);
    }
    let p = preset::from_layer(&doc, &a.layer, from, to, &a.name, &a.description).map_err(|e| anyhow!(e))?;
    let path = dir_of(&a.project).join("presets").join(format!("{}.json", a.name));
    if path.exists() && !a.force {
        bail!("{} exists; pass --force to replace it", path.display());
    }
    std::fs::create_dir_all(path.parent().expect("has a parent"))?;
    std::fs::write(&path, jsonfmt::to_file_text(&serde_json::to_value(&p)?))?;
    if a.json {
        crate::print_json(&json!({ "preset": p.name, "file": path, "actions": p.apply.len() }));
    } else {
        println!("wrote {} ({} action(s)); apply it with `edits apply {} {} --layer <id> --at <time>`", path.display(), p.apply.len(), a.project.display(), p.name);
    }
    Ok(ExitCode::SUCCESS)
}

// ---- beats ----------------------------------------------------------------------

fn range(tc: &TimeCtx, from: &Option<String>, to: &Option<String>) -> Result<(f64, f64)> {
    let from = from.as_deref().map(|t| resolve(tc, t, "--from")).transpose()?.unwrap_or(0.0);
    let to = to.as_deref().map(|t| resolve(tc, t, "--to")).transpose()?.unwrap_or(tc.duration);
    if to <= from {
        bail!("--to ({to}) must be after --from ({from})");
    }
    Ok((from, to))
}

pub fn beats(cmd: BeatsCommand) -> Result<ExitCode> {
    match cmd {
        BeatsCommand::Cut(a) => cut(a),
        BeatsCommand::Accent(a) => accent(a),
    }
}

fn cut(a: CutArgs) -> Result<ExitCode> {
    let doc = read_doc(&a.project)?;
    let song = load_song(a.audio.as_deref())?;
    let tc = time_ctx(&doc, song.as_ref());
    let (from, to) = range(&tc, &a.from, &a.to)?;
    let audio_offset = doc.pointer("/audio/offset").and_then(Value::as_f64).unwrap_or(0.0);
    let times = a.on.as_deref().map(|on| beatops::times(on, song.as_ref(), &tc, from, to, a.min_strength, audio_offset).map_err(|e| anyhow!(e))).transpose()?;
    let place = match (&a.above, &a.below) {
        (Some(id), _) => Place::Above(id.clone()),
        (_, Some(id)) => Place::Below(id.clone()),
        _ => Place::Default,
    };
    let opts = CutOpts { slots: a.slots.clone(), times, every: a.every, from, to, prefix: a.prefix.clone(), place };
    let label = format!("beats cut {} from {from} to {to}", a.slots.join(","));
    let report = std::cell::RefCell::new(None);
    let outcome = write(&a.project, &label, a.dry_run, &|d| {
        let (out, r) = beatops::cut(d, &opts, &tc)?;
        *report.borrow_mut() = Some(r);
        Ok(out)
    })?;
    let r = report.into_inner();
    if a.json {
        let mut v = outcome_json(&outcome);
        v["layers"] = json!(r.as_ref().map(|r| r.layers.iter().map(|(id, slot, i, o)| json!({ "id": id, "slot": slot, "in": i, "out": o })).collect::<Vec<_>>()));
        v["extended_to"] = json!(r.as_ref().and_then(|r| r.extended_to));
        crate::print_json(&v);
    } else {
        if let Some(r) = &r {
            let list: Vec<String> = r.layers.iter().map(|(id, slot, i, o)| format!("{id}={slot}@{i}..{o}")).collect();
            println!("{} layer(s): {}", r.layers.len(), list.join(" "));
            if let Some(d) = r.extended_to {
                println!("the project now runs to {d}");
            }
        }
        println!("{}", outcome_text(&outcome, &a.project));
    }
    Ok(exit(&outcome))
}

fn accent(a: AccentArgs) -> Result<ExitCode> {
    let doc = read_doc(&a.project)?;
    let song = load_song(a.audio.as_deref())?;
    let tc = time_ctx(&doc, song.as_ref());
    let (from, to) = range(&tc, &a.from, &a.to)?;
    let audio_offset = doc.pointer("/audio/offset").and_then(Value::as_f64).unwrap_or(0.0);
    let mut times = beatops::times(&a.on, song.as_ref(), &tc, from, to, a.min_strength, audio_offset).map_err(|e| anyhow!(e))?;
    if a.every_nth > 1 {
        times = times.into_iter().step_by(a.every_nth).collect();
    }
    if times.is_empty() {
        bail!("no {} between {from} and {to}", a.on);
    }
    let p = preset::find(&a.preset, Some(&dir_of(&a.project))).map_err(|e| anyhow!(e))?;
    let vars: Map<String, Value> = preset::parse_assignments(&a.vars).map_err(|e| anyhow!(e))?;
    let target = Target::parse(Some(&a.layer));
    let label = format!("beats accent {} on {} ({} times)", p.name, a.on, times.len());
    let report = std::cell::RefCell::new(Vec::new());
    let outcome = write(&a.project, &label, a.dry_run, &|d| {
        let (out, applied) = preset::apply(d, &p, &target, &times, &vars, &tc)?;
        *report.borrow_mut() = applied;
        Ok(out)
    })?;
    let applied = report.into_inner();
    if a.json {
        let mut v = outcome_json(&outcome);
        v["preset"] = json!(p.name);
        v["times"] = json!(times);
        v["applied"] = json!(applied.iter().map(|x| json!({ "at": x.at, "layer": x.layer })).collect::<Vec<_>>());
        crate::print_json(&v);
    } else {
        let list: Vec<String> = applied.iter().map(|x| format!("{}@{}", x.layer.as_deref().unwrap_or("-"), x.at)).collect();
        println!("{} on {} time(s): {}", p.name, applied.len(), list.join(" "));
        println!("{}", outcome_text(&outcome, &a.project));
    }
    Ok(exit(&outcome))
}

/// `edits audio --project`: write the song's tempo and markers into a project.
pub fn sync_project(project: &Path, song: &Value, analysis_file: Option<&Path>) -> Result<Value> {
    let info = SongInfo::from_value(song).map_err(|e| anyhow!(e))?;
    let source = song.get("source").and_then(Value::as_str).map(PathBuf::from);
    let dir = dir_of(project);
    let rel = source.as_ref().map(|s| relative(s, &dir));
    let said = std::cell::RefCell::new(Vec::new());
    let outcome = write(project, "tempo and markers from a song analysis", false, &|d| {
        let (out, s) = beatops::sync(d, &info, rel.as_deref())?;
        *said.borrow_mut() = s;
        Ok(out)
    })?;
    let errors: Vec<String> = outcome.findings.iter().filter(|f| f.severity == Severity::Error).map(edit::finding_line).collect();
    if !outcome.ok {
        bail!("the project was not changed: {}", errors.join("; "));
    }
    let said = said.into_inner();
    let _ = analysis_file;
    Ok(json!({ "project": project, "written": outcome.written, "summary": format!("{}: {}", project.display(), said.join(", ")), "changes": said }))
}

/// `path` relative to `dir` when it is inside it, else as given.
fn relative(path: &Path, dir: &Path) -> String {
    let abs = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (p, d) = (abs(path), abs(dir));
    match p.strip_prefix(&d) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => {
            // One level up is common (song next to the project folder).
            let mut up = PathBuf::new();
            let mut base = d.clone();
            for _ in 0..3 {
                up.push("..");
                if let Some(parent) = base.parent() {
                    base = parent.to_path_buf();
                    if let Ok(rel) = p.strip_prefix(&base) {
                        return up.join(rel).to_string_lossy().into_owned();
                    }
                }
            }
            p.to_string_lossy().into_owned()
        }
    }
}

// ---- transitions ----------------------------------------------------------------

pub fn transitions(as_json: bool) {
    if as_json {
        let list: Vec<Value> = effects::TRANSITIONS
            .iter()
            .map(|t| {
                let params: Vec<Value> = t
                    .params
                    .iter()
                    .map(|p| json!({ "name": p.name, "default": if p.default.len() == 1 { json!(p.default[0]) } else { json!(p.default) }, "min": p.min, "max": p.max, "color": p.color, "doc": p.doc }))
                    .collect();
                json!({ "kind": t.kind, "doc": t.doc, "duration": t.duration, "params": params, "credit": t.credit })
            })
            .collect();
        crate::print_json(&json!(list));
        return;
    }
    println!("A transition sits on the incoming layer: \"transition\": {{ \"kind\", \"duration\", \"params\", \"ease\" }}; the preset transition:<kind> sets it and overlaps the cut.");
    for t in effects::TRANSITIONS {
        println!("{} ({} beats): {}", t.kind, t.duration, t.doc);
        for p in t.params {
            let default = if p.default.len() == 1 { format!("{}", p.default[0]) } else { format!("{:?}", p.default) };
            println!("    {:<14} default {:<20} {}", p.name, default, p.doc);
        }
    }
}
