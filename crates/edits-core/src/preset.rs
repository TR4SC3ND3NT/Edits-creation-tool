//! Presets: named, parameterised building blocks (a flash, a punch-in, a
//! slide, a transition) that expand into ordinary layers, keyframes and
//! effects, so the result stays editable, diffable and undoable.
//!
//! A preset is a JSON file of format `edits.preset/1`: typed `variables`
//! with defaults, ranges and docs, and a list of actions (`apply`) written
//! relative to a time `at` and a target layer. The built-in presets live in
//! `presets/*.json` at the root of the repository and are compiled in; a
//! project can add its own in `<project dir>/presets/<name>.json`, and
//! `transition:<kind>` presets are generated from the transition registry.
//! The format is documented in `docs/FORMAT.md` (Presets).
//!
//! Inside actions, a string that starts with `=` is an expression (numbers,
//! `+ - * /`, parentheses, `min`, `max`, `abs`, `vec(a, b, ...)`; vectors
//! combine element-wise), a string that is exactly `$name` is the value of
//! a variable (any type), and anything else is literal. Expressions can use
//! the variables as `$name` and: `at` (the time the preset is applied at),
//! `beat` and `frame` (their lengths in project time), `in` and `out` of the
//! target layer, `prev_in` and `prev_out` of the layer it cuts from, and in
//! a pulse `base` (the value at `at` before the preset) and `orig` (the value
//! before the preset at that key's time), with components `base0`, `orig1`...

use crate::model::*;
use crate::time::TimeCtx;
use crate::{ease, effects, pointer, Evaluator};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "edits.preset/1";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub format: String,
    pub name: String,
    /// One line: what it looks like.
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// `layer`: changes a layer (`--layer`, or the picture on screen at each time);
    /// `project`: adds layers of its own and needs no target.
    #[serde(default)]
    pub target: TargetKind,
    #[serde(default)]
    pub variables: BTreeMap<String, Variable>,
    pub apply: Vec<Value>,
    /// Where it comes from, when it is not original work (licence and author).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    #[default]
    Layer,
    Project,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    #[serde(rename = "type")]
    pub ty: VarType,
    pub default: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// `enum`: each option, with the numbers it sets (available as `$name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<BTreeMap<String, Map<String, Value>>>,
    /// What a number is measured in, e.g. `beats`, `% of height`, `degrees`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub doc: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VarType {
    Number,
    Integer,
    Boolean,
    Color,
    Vector,
    Enum,
    String,
}

// ---- the library ------------------------------------------------------------

macro_rules! builtin {
    ($($name:literal),* $(,)?) => {
        /// The built-in presets: (file name, JSON text).
        pub const BUILTIN: &[(&str, &str)] = &[$(($name, include_str!(concat!("../../../presets/", $name, ".json")))),*];
    };
}

builtin!(
    "flash",
    "flash_burn",
    "punch",
    "zoom_out",
    "push_in",
    "shake",
    "rgb_split",
    "blur_in",
    "blur_out",
    "bw_hit",
    "invert_hit",
    "threshold_hit",
    "slide_in",
    "slide_out",
    "flip_3d",
    "cube_turn",
    "spin",
    "velocity",
    "glow_pulse",
    "vignette_pulse",
    "overlay_texture",
    "letterbox",
    "sakura",
    "sparkles",
    "dust",
    "beat_burst",
);

/// Parse and check one preset file's text.
pub fn parse(text: &str, origin: &str) -> Result<Preset, String> {
    let p: Preset = serde_json::from_str(text).map_err(|e| format!("{origin} is not a valid preset: {e}"))?;
    if p.format != FORMAT {
        return Err(format!("{origin}: \"format\" must be \"{FORMAT}\""));
    }
    for (name, v) in &p.variables {
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.is_empty() {
            return Err(format!("{origin}: variable name \"{name}\" must be letters, digits and _"));
        }
        coerce(name, v, &v.default).map_err(|e| format!("{origin}: the default of \"{name}\": {e}"))?;
        if v.ty == VarType::Enum && v.options.as_ref().is_none_or(|o| o.is_empty()) {
            return Err(format!("{origin}: enum variable \"{name}\" needs \"options\""));
        }
    }
    for (i, a) in p.apply.iter().enumerate() {
        let kind = action_kind(a).map_err(|e| format!("{origin}: apply[{i}]: {e}"))?;
        if p.target == TargetKind::Project && !matches!(kind, "layer" | "set_project") {
            return Err(format!("{origin}: apply[{i}]: a preset with target \"project\" can only add layers"));
        }
    }
    Ok(p)
}

fn action_kind(a: &Value) -> Result<&'static str, String> {
    let obj = a.as_object().ok_or("an action is an object such as { \"pulse\": { ... } }")?;
    let known = ["pulse", "effect", "layer", "set", "shake", "retime"];
    let found: Vec<&'static str> = known.iter().copied().filter(|k| obj.contains_key(*k)).collect();
    match found.as_slice() {
        [one] if obj.len() == 1 => Ok(one),
        _ => Err(format!("an action has exactly one of the keys {}", known.join(", "))),
    }
}

/// Every preset this build knows, built-in and generated, without the project's own.
pub fn builtin() -> Vec<Preset> {
    let mut out: Vec<Preset> = BUILTIN.iter().map(|(name, text)| parse(text, &format!("presets/{name}.json")).expect("built-in presets parse")).collect();
    out.extend(effects::TRANSITIONS.iter().map(transition_preset));
    out
}

/// Built-in presets plus the ones in `<project dir>/presets/` (which shadow
/// a built-in of the same name). Unreadable local files are reported, not skipped silently.
pub fn library(project_dir: Option<&Path>) -> (Vec<Preset>, Vec<String>) {
    let mut out = builtin();
    let mut problems = Vec::new();
    if let Some(dir) = project_dir.map(|d| d.join("presets")) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        files.sort();
        for f in files {
            match std::fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|t| parse(&t, &f.display().to_string())) {
                Ok(p) => {
                    out.retain(|q| q.name != p.name);
                    out.push(p);
                }
                Err(e) => problems.push(e),
            }
        }
    }
    (out, problems)
}

/// Find a preset by name (or a path to a preset file).
pub fn find(name: &str, project_dir: Option<&Path>) -> Result<Preset, String> {
    if name.ends_with(".json") {
        let path = match project_dir {
            Some(d) if Path::new(name).is_relative() && !Path::new(name).exists() => d.join(name),
            _ => PathBuf::from(name),
        };
        let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read preset {}: {e}", path.display()))?;
        return parse(&text, &path.display().to_string());
    }
    let (all, problems) = library(project_dir);
    if let Some(p) = all.iter().find(|p| p.name == name) {
        return Ok(p.clone());
    }
    let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).filter(|n| !n.starts_with("transition:")).collect();
    let mut msg = format!("no preset \"{name}\"; presets: {}, and transition:<kind> for every transition (`edits presets` lists them all)", names.join(", "));
    if !problems.is_empty() {
        msg.push_str(&format!(". Some project presets could not be read: {}", problems.join("; ")));
    }
    Err(msg)
}

/// The `transition:<kind>` preset of a registered transition: it moves the
/// cut so the two pictures overlap and sets the incoming layer's `transition`.
pub fn transition_preset(t: &effects::TransitionSpec) -> Preset {
    let mut variables = BTreeMap::new();
    variables.insert("duration".to_string(), Variable { ty: VarType::Number, default: json!(t.duration), min: Some(0.05), max: Some(16.0), options: None, unit: Some("beats".into()), doc: "Length of the transition.".into() });
    let mut align = BTreeMap::new();
    align.insert("center".to_string(), json!({ "pre": 0.5 }).as_object().cloned().unwrap_or_default());
    align.insert("after".to_string(), json!({ "pre": 0.0 }).as_object().cloned().unwrap_or_default());
    align.insert("before".to_string(), json!({ "pre": 1.0 }).as_object().cloned().unwrap_or_default());
    variables.insert("align".to_string(), Variable { ty: VarType::Enum, default: json!("center"), min: None, max: None, options: Some(align), unit: None, doc: "Where the transition sits: centred on the cut, starting at it (after) or ending at it (before).".into() });
    variables.insert("ease".to_string(), Variable { ty: VarType::String, default: json!("linear"), min: None, max: None, options: None, unit: None, doc: "Easing of the progress (any ease name, e.g. in_out).".into() });
    let mut params = Map::new();
    for p in t.params {
        let ty = if p.color { VarType::Color } else if p.default.len() == 1 { VarType::Number } else { VarType::Vector };
        let default = if p.default.len() == 1 { json!(p.default[0]) } else { json!(p.default) };
        variables.insert(p.name.to_string(), Variable { ty, default, min: p.min, max: p.max, options: None, unit: None, doc: p.doc.to_string() });
        params.insert(p.name.to_string(), json!(format!("${}", p.name)));
    }
    Preset {
        format: FORMAT.into(),
        name: format!("transition:{}", t.kind),
        description: format!("Transition into the layer at its cut: {}", t.doc),
        tags: vec!["transition".into(), "cut".into()],
        target: TargetKind::Layer,
        variables,
        apply: vec![
            json!({ "set": { "on": "previous", "pointer": "/out", "value": "=max(prev_out, at + (1 - $pre) * $duration * beat)" } }),
            json!({ "set": { "pointer": "/in", "value": "=at - $pre * $duration * beat" } }),
            json!({ "set": { "pointer": "/transition", "value": { "kind": t.kind, "duration": "=$duration * beat", "ease": "$ease", "params": params } } }),
        ],
        credit: Some(t.credit.to_string()),
    }
}

// ---- variables --------------------------------------------------------------

fn parse_numbers(s: &str) -> Option<Vec<f64>> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    s.split(',').map(|x| x.trim().parse::<f64>().ok()).collect()
}

/// Bring a given value to the variable's type, or say why it does not fit.
fn coerce(name: &str, v: &Variable, given: &Value) -> Result<Value, String> {
    let range = |n: f64| -> Result<f64, String> {
        if !n.is_finite() {
            return Err(format!("{name} must be a finite number"));
        }
        if let Some(min) = v.min.filter(|m| n < *m) {
            return Err(format!("{name} = {n} is below the minimum {min}"));
        }
        if let Some(max) = v.max.filter(|m| n > *m) {
            return Err(format!("{name} = {n} is above the maximum {max}"));
        }
        Ok(n)
    };
    let number = |g: &Value| -> Option<f64> {
        match g {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    };
    match v.ty {
        VarType::Number => Ok(json!(range(number(given).ok_or_else(|| format!("{name} must be a number, not {given}"))?)?)),
        VarType::Integer => {
            let n = number(given).ok_or_else(|| format!("{name} must be a whole number, not {given}"))?;
            if n.fract() != 0.0 {
                return Err(format!("{name} must be a whole number, not {n}"));
            }
            Ok(json!(range(n)? as i64))
        }
        VarType::Boolean => match given {
            Value::Bool(b) => Ok(json!(b)),
            Value::String(s) if matches!(s.as_str(), "true" | "yes" | "1" | "on") => Ok(json!(true)),
            Value::String(s) if matches!(s.as_str(), "false" | "no" | "0" | "off") => Ok(json!(false)),
            _ => Err(format!("{name} must be true or false, not {given}")),
        },
        VarType::Color | VarType::Vector => {
            let nums: Option<Vec<f64>> = match given {
                Value::Array(a) => a.iter().map(Value::as_f64).collect(),
                Value::Number(n) => n.as_f64().map(|x| vec![x]),
                Value::String(s) if v.ty == VarType::Color && s.starts_with('#') && s.len() == 7 => {
                    (0..3).map(|i| u8::from_str_radix(&s[1 + 2 * i..3 + 2 * i], 16).ok().map(|b| b as f64 / 255.0)).collect()
                }
                Value::String(s) => parse_numbers(s),
                _ => None,
            };
            let nums = nums.ok_or_else(|| format!("{name} must be {}, not {given}", if v.ty == VarType::Color { "a colour: [r, g, b] in 0..1, \"r,g,b\" or \"#rrggbb\"" } else { "numbers: [x, y] or \"x,y\"" }))?;
            let want = v.default.as_array().map(|a| a.len()).unwrap_or(1);
            if nums.len() != want && !(v.ty == VarType::Color && (nums.len() == 3 || nums.len() == 4)) {
                return Err(format!("{name} needs {want} numbers, got {}", nums.len()));
            }
            for n in &nums {
                range(*n)?;
            }
            Ok(json!(nums))
        }
        VarType::Enum => {
            let s = given.as_str().ok_or_else(|| format!("{name} must be one of the option names, not {given}"))?;
            let options = v.options.as_ref().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
            if !options.iter().any(|o| o == s) {
                return Err(format!("{name} = \"{s}\" is not an option; options: {}", options.join(", ")));
            }
            Ok(json!(s))
        }
        VarType::String => match given {
            Value::String(s) => Ok(json!(s)),
            other => Ok(json!(other.to_string())),
        },
    }
}

/// The variables' values: the given ones (checked) over the defaults, plus
/// what the chosen enum options set. Unknown names are an error.
pub fn variables(p: &Preset, given: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    for k in given.keys() {
        if !p.variables.contains_key(k) {
            let names: Vec<&str> = p.variables.keys().map(String::as_str).collect();
            return Err(format!("preset \"{}\" has no variable \"{k}\"; variables: {}", p.name, if names.is_empty() { "(none)".into() } else { names.join(", ") }));
        }
    }
    let mut out = Map::new();
    for (name, v) in &p.variables {
        let value = coerce(name, v, given.get(name).unwrap_or(&v.default))?;
        if v.ty == VarType::Enum {
            if let Some(extra) = value.as_str().and_then(|s| v.options.as_ref()?.get(s)) {
                for (k, x) in extra {
                    out.insert(k.clone(), x.clone());
                }
            }
        }
        out.insert(name.clone(), value);
    }
    Ok(out)
}

/// `name=value` pairs from a command line, as JSON values (numbers stay text
/// until they are checked against the preset's types).
pub fn parse_assignments(items: &[String]) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for item in items {
        let (k, v) = item.split_once('=').ok_or_else(|| format!("--var \"{item}\": write name=value, e.g. --var strength=0.8"))?;
        let value = serde_json::from_str::<Value>(v.trim()).ok().filter(|x| x.is_array() || x.is_boolean()).unwrap_or_else(|| json!(v.trim()));
        out.insert(k.trim().to_string(), value);
    }
    Ok(out)
}

// ---- expressions -------------------------------------------------------------

type Env = BTreeMap<String, Vec<f64>>;

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    env: &'a Env,
}

fn combine(a: Vec<f64>, b: Vec<f64>, f: impl Fn(f64, f64) -> f64) -> Result<Vec<f64>, String> {
    match (a.len(), b.len()) {
        (1, _) => Ok(b.iter().map(|y| f(a[0], *y)).collect()),
        (_, 1) => Ok(a.iter().map(|x| f(*x, b[0])).collect()),
        (n, m) if n == m => Ok(a.iter().zip(&b).map(|(x, y)| f(*x, *y)).collect()),
        (n, m) => Err(format!("cannot combine a vector of {n} numbers with one of {m}")),
    }
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn expr(&mut self) -> Result<Vec<f64>, String> {
        let mut v = self.term()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = combine(v, r, |x, y| if c == b'+' { x + y } else { x - y })?;
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<Vec<f64>, String> {
        let mut v = self.unary()?;
        while let Some(c @ (b'*' | b'/')) = self.peek() {
            self.i += 1;
            let r = self.unary()?;
            v = combine(v, r, |x, y| if c == b'*' { x * y } else { x / y })?;
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<Vec<f64>, String> {
        if self.peek() == Some(b'-') {
            self.i += 1;
            return Ok(self.unary()?.into_iter().map(|x| -x).collect());
        }
        self.primary()
    }
    fn ident(&mut self) -> String {
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_' || self.s[self.i] == b'$') {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
    }
    fn primary(&mut self) -> Result<Vec<f64>, String> {
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() != Some(b')') {
                    return Err("missing )".into());
                }
                self.i += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                let start = self.i;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
                    self.i += 1;
                }
                let t = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
                t.parse::<f64>().map(|n| vec![n]).map_err(|_| format!("\"{t}\" is not a number"))
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'$' || c == b'_' => {
                let name = self.ident();
                if self.peek() == Some(b'(') {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(b')') {
                        loop {
                            args.push(self.expr()?);
                            match self.peek() {
                                Some(b',') => self.i += 1,
                                Some(b')') => break,
                                _ => return Err(format!("missing ) after the arguments of {name}")),
                            }
                        }
                    }
                    self.i += 1;
                    return call(&name, args);
                }
                self.env.get(&name).cloned().ok_or_else(|| {
                    let known: Vec<&str> = self.env.keys().map(String::as_str).collect();
                    format!("unknown name \"{name}\" (known here: {})", known.join(", "))
                })
            }
            other => Err(format!("unexpected {}", other.map(|c| format!("\"{}\"", c as char)).unwrap_or_else(|| "end".into()))),
        }
    }
}

fn call(name: &str, args: Vec<Vec<f64>>) -> Result<Vec<f64>, String> {
    let one = |args: &[Vec<f64>]| -> Result<f64, String> { args.first().and_then(|a| a.first().copied()).ok_or_else(|| format!("{name} needs an argument")) };
    match name {
        "vec" => Ok(args.into_iter().flatten().collect()),
        "abs" => Ok(args.into_iter().next().unwrap_or_default().into_iter().map(f64::abs).collect()),
        "min" | "max" => {
            let mut it = args.into_iter();
            let mut v = it.next().ok_or_else(|| format!("{name} needs arguments"))?;
            for a in it {
                v = combine(v, a, |x, y| if name == "min" { x.min(y) } else { x.max(y) })?;
            }
            Ok(v)
        }
        "round" => Ok(vec![one(&args)?.round()]),
        other => Err(format!("unknown function \"{other}\" (functions: vec, min, max, abs, round)")),
    }
}

/// Evaluate an expression (the text after `=`).
pub fn eval(text: &str, env: &Env) -> Result<Vec<f64>, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0, env };
    let v = p.expr().map_err(|e| format!("in \"={text}\": {e}"))?;
    if p.peek().is_some() {
        return Err(format!("in \"={text}\": unexpected text after position {}", p.i));
    }
    if v.iter().any(|x| !x.is_finite()) {
        return Err(format!("\"={text}\" is not a finite number"));
    }
    Ok(v)
}

fn to_json(v: Vec<f64>) -> Value {
    let r = |x: f64| (x * 1e6).round() / 1e6;
    if v.len() == 1 {
        json!(r(v[0]))
    } else {
        json!(v.into_iter().map(r).collect::<Vec<_>>())
    }
}

/// Fill in a template: `=expr`, `$name`, recursively.
fn subst(v: &Value, env: &Env, vars: &Map<String, Value>) -> Result<Value, String> {
    Ok(match v {
        Value::String(s) if s.starts_with('=') => to_json(eval(&s[1..], env)?),
        Value::String(s) if s.starts_with('$') && s[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
            vars.get(&s[1..]).cloned().ok_or_else(|| format!("unknown variable {s}"))?
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| subst(x, env, vars)).collect::<Result<_, _>>()?),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| Ok((k.clone(), subst(x, env, vars)?))).collect::<Result<_, String>>()?),
        other => other.clone(),
    })
}

fn numeric_env(vars: &Map<String, Value>) -> Env {
    let mut env = Env::new();
    for (k, v) in vars {
        let nums: Option<Vec<f64>> = match v {
            Value::Number(n) => n.as_f64().map(|x| vec![x]),
            Value::Bool(b) => Some(vec![if *b { 1.0 } else { 0.0 }]),
            Value::Array(a) => a.iter().map(Value::as_f64).collect(),
            _ => None,
        };
        if let Some(n) = nums {
            env.insert(format!("${k}"), n);
        }
    }
    env
}

// ---- applying -----------------------------------------------------------------

/// Which layer a preset changes.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// This layer.
    Layer(String),
    /// At each time, the topmost visible picture with a normal blend.
    Auto,
    /// An adjustment layer `accents` on top (created when missing): the
    /// preset's effects then apply to everything beneath.
    Fx,
    /// No layer (for presets that add their own layers).
    None,
}

impl Target {
    pub fn parse(s: Option<&str>) -> Target {
        match s {
            None | Some("") => Target::Auto,
            Some("auto") => Target::Auto,
            Some("fx") => Target::Fx,
            Some(id) => Target::Layer(id.to_string()),
        }
    }
}

/// What one application changed.
#[derive(Serialize, Clone, Debug)]
pub struct Applied {
    pub at: f64,
    /// The layer the preset changed, if any.
    pub layer: Option<String>,
    /// One short line per change.
    pub changes: Vec<String>,
}

pub const FX_LAYER: &str = "accents";

fn layers(doc: &Value) -> &[Value] {
    doc.get("layers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn index_of(doc: &Value, id: &str) -> Result<usize, String> {
    let ls = layers(doc);
    ls.iter().position(|l| l.get("id").and_then(Value::as_str) == Some(id)).ok_or_else(|| {
        let ids: Vec<&str> = ls.iter().filter_map(|l| l.get("id").and_then(Value::as_str)).collect();
        format!("no layer has id \"{id}\"; layers, bottom to top: {}", ids.join(", "))
    })
}

fn num_field(l: &Value, k: &str) -> Option<f64> {
    l.get(k).and_then(Value::as_f64)
}

fn active(l: &Value, t: f64, duration: f64) -> bool {
    let start = num_field(l, "in").unwrap_or(0.0);
    let end = num_field(l, "out").unwrap_or(duration);
    l.get("enabled").and_then(Value::as_bool).unwrap_or(true) && t >= start - 1e-9 && t < end - 1e-9
}

fn is_picture(l: &Value) -> bool {
    let kind = l.get("kind").and_then(Value::as_str).unwrap_or("");
    let blend = l.get("blend").and_then(Value::as_str).unwrap_or("normal");
    kind != "adjustment" && blend == "normal"
}

/// The topmost picture visible at `t`.
pub fn picture_at(doc: &Value, t: f64) -> Option<String> {
    let duration = doc.pointer("/settings/duration").and_then(Value::as_f64).unwrap_or(f64::MAX);
    layers(doc).iter().rev().find(|l| is_picture(l) && active(l, t, duration)).and_then(|l| l.get("id")?.as_str().map(str::to_string))
}

/// The picture the layer at `index` cuts from: the topmost picture below it
/// that is visible just before `at`.
fn previous(doc: &Value, index: usize, at: f64, frame: f64) -> Option<usize> {
    let duration = doc.pointer("/settings/duration").and_then(Value::as_f64).unwrap_or(f64::MAX);
    let ls = layers(doc);
    (0..index).rev().find(|&i| is_picture(&ls[i]) && active(&ls[i], at - 0.5 * frame, duration))
}

/// Value of the transform property `name` when it is not written.
fn transform_default(name: &str) -> Value {
    match name {
        "position" | "rotation" => json!([0.0, 0.0, 0.0]),
        "anchor" => json!([0.0, 0.0]),
        _ => json!(1.0),
    }
}

struct Run<'a> {
    doc: Value,
    project: Project,
    tc: &'a TimeCtx,
    vars: Map<String, Value>,
    env: Env,
    changes: Vec<String>,
    target: Option<usize>,
    at: f64,
}

/// Apply `preset` at each of `times` (project time) to `doc`, and return the
/// changed document with one report per time. `given` are the variables.
pub fn apply(doc: &Value, preset: &Preset, target: &Target, times: &[f64], given: &Map<String, Value>, tc: &TimeCtx) -> Result<(Value, Vec<Applied>), String> {
    if times.is_empty() {
        return Err("no times to apply the preset at".into());
    }
    let vars = variables(preset, given)?;
    let mut doc = doc.clone();
    let mut out = Vec::new();
    for &at in times {
        let (next, applied) = apply_once(&doc, preset, target, at, &vars, tc).map_err(|e| format!("preset \"{}\" at {}: {e}", preset.name, (at * 1000.0).round() / 1000.0))?;
        doc = next;
        out.push(applied);
    }
    Ok((doc, out))
}

fn apply_once(doc: &Value, preset: &Preset, target: &Target, at: f64, vars: &Map<String, Value>, tc: &TimeCtx) -> Result<(Value, Applied), String> {
    let mut doc = doc.clone();
    let mut target_id = match (preset.target, target) {
        (TargetKind::Project, Target::Layer(id)) => Some(id.clone()),
        (TargetKind::Project, _) => None,
        (_, Target::Layer(id)) => Some(id.clone()),
        // Half a frame later: a hit right on a cut belongs to the picture after it.
        (_, Target::Auto) => Some(picture_at(&doc, at + 0.5 * tc.frame_len()).ok_or_else(|| format!("no picture is visible at {at}; give --layer"))?),
        (_, Target::Fx) => {
            if index_of(&doc, FX_LAYER).is_err() {
                let ls = doc.get_mut("layers").and_then(Value::as_array_mut).ok_or("the project has no \"layers\"")?;
                ls.push(json!({ "id": FX_LAYER, "kind": "adjustment", "name": "accents added by presets" }));
            }
            Some(FX_LAYER.to_string())
        }
        (_, Target::None) => return Err(format!("preset \"{}\" changes a layer: give one with --layer (or auto, or fx)", preset.name)),
    };
    let target = match &target_id {
        Some(id) => Some(index_of(&doc, id)?),
        None => None,
    };
    if let Some(i) = target {
        target_id = layers(&doc)[i].get("id").and_then(Value::as_str).map(str::to_string);
    }
    let project: Project = serde_json::from_value(doc.clone()).map_err(|e| format!("the project does not parse ({e}); run `edits check` first"))?;
    let mut env = numeric_env(vars);
    env.insert("at".into(), vec![at]);
    env.insert("beat".into(), vec![tc.beat_len()]);
    env.insert("frame".into(), vec![tc.frame_len()]);
    env.insert("end".into(), vec![tc.duration]);
    let size = |k: &str| doc.pointer(&format!("/settings/{k}")).and_then(Value::as_f64).unwrap_or(1.0);
    env.insert("aspect".into(), vec![size("width") / size("height").max(1.0)]);
    if let Some(i) = target {
        let l = &layers(&doc)[i];
        env.insert("in".into(), vec![num_field(l, "in").unwrap_or(0.0)]);
        env.insert("out".into(), vec![num_field(l, "out").unwrap_or(tc.duration)]);
        if let Some(p) = previous(&doc, i, at, tc.frame_len()) {
            let pl = &layers(&doc)[p];
            env.insert("prev_in".into(), vec![num_field(pl, "in").unwrap_or(0.0)]);
            env.insert("prev_out".into(), vec![num_field(pl, "out").unwrap_or(tc.duration)]);
        }
    }
    let mut changes = Vec::new();
    if let (Some(i), false) = (target, preset.name.starts_with("transition:")) {
        let l = &layers(&doc)[i];
        if !active(l, at, tc.duration) && preset.target == TargetKind::Layer {
            let (a, b) = (num_field(l, "in").unwrap_or(0.0), num_field(l, "out").unwrap_or(tc.duration));
            changes.push(format!("note: {} is not visible at {at} (it runs {a}..{b}), so this may not show", target_id.as_deref().unwrap_or("?")));
        }
    }
    let mut run = Run { doc, project, tc, vars: vars.clone(), env, changes, target, at };
    for action in &preset.apply {
        run.action(action)?;
    }
    Ok((run.doc, Applied { at, layer: target_id, changes: run.changes }))
}

impl Run<'_> {
    fn layer(&self, i: usize) -> &Value {
        &layers(&self.doc)[i]
    }
    fn layer_id(&self, i: usize) -> String {
        self.layer(i).get("id").and_then(Value::as_str).unwrap_or("?").to_string()
    }
    fn target(&self, what: &str) -> Result<usize, String> {
        self.target.ok_or_else(|| format!("{what} needs a target layer: give --layer"))
    }
    fn layer_mut(&mut self, i: usize) -> &mut Value {
        &mut self.doc["layers"][i]
    }

    fn action(&mut self, a: &Value) -> Result<(), String> {
        let kind = action_kind(a)?;
        let spec = a[kind].as_object().ok_or_else(|| format!("\"{kind}\" must be an object"))?.clone();
        match kind {
            "pulse" => self.pulse(&spec),
            "shake" => self.shake(&spec),
            "effect" => {
                let i = self.target("an effect")?;
                let effect = subst(&Value::Object(spec), &self.env, &self.vars)?;
                let kind = effect.get("kind").and_then(Value::as_str).unwrap_or("?").to_string();
                let l = self.layer_mut(i);
                if l.get("effects").is_none() {
                    l["effects"] = json!([]);
                }
                l["effects"].as_array_mut().ok_or("\"effects\" is not an array")?.push(effect);
                let id = self.layer_id(i);
                self.changes.push(format!("added effect {kind} to {id}"));
                Ok(())
            }
            "layer" => self.add_layer(&spec),
            "set" => self.set(&spec),
            "retime" => self.retime(&spec),
            _ => unreachable!(),
        }
    }

    /// Where a pulse or shake writes: a pointer to the property, its value when
    /// missing, and a label. Creates the effect when the layer does not have it.
    fn locate(&mut self, spec: &Map<String, Value>) -> Result<(String, Value, String), String> {
        let i = self.target("a pulse")?;
        let id = self.layer_id(i);
        let lp = format!("/layers/{i}");
        let adjustment = self.layer(i).get("kind").and_then(Value::as_str) == Some("adjustment");
        if let Some(name) = spec.get("transform").and_then(Value::as_str) {
            if adjustment {
                return match spec.get("adjustment").and_then(Value::as_object) {
                    Some(alt) => self.locate(&alt.clone()),
                    None => Err(format!("this preset moves its layer, and \"{id}\" is an adjustment layer; give a picture layer")),
                };
            }
            if !["position", "scale", "rotation", "anchor", "opacity"].contains(&name) {
                return Err(format!("unknown transform property \"{name}\""));
            }
            return Ok((format!("{lp}/transform/{name}"), transform_default(name), format!("{id}.{name}")));
        }
        let kind = spec.get("effect").and_then(Value::as_str).ok_or("a pulse names \"transform\" or \"effect\"")?;
        let param = spec.get("param").and_then(Value::as_str).ok_or("an effect pulse names its \"param\"")?;
        let es = effects::spec(kind).ok_or_else(|| format!("unknown effect \"{kind}\""))?;
        let ps = es.params.iter().find(|p| p.name == param).ok_or_else(|| format!("effect \"{kind}\" has no parameter \"{param}\""))?;
        let default = if ps.default.len() == 1 { json!(ps.default[0]) } else { json!(ps.default) };
        let base = match spec.get("base") {
            Some(b) => subst(b, &self.env, &self.vars)?,
            None => default,
        };
        let existing = self.layer(i).get("effects").and_then(Value::as_array).and_then(|es| es.iter().position(|e| e.get("kind").and_then(Value::as_str) == Some(kind)));
        let ei = match existing {
            Some(ei) => ei,
            None => {
                let mut params = match spec.get("params") {
                    Some(p) => subst(p, &self.env, &self.vars)?,
                    None => json!({}),
                };
                params[param] = base.clone();
                let l = self.layer_mut(i);
                if l.get("effects").is_none() {
                    l["effects"] = json!([]);
                }
                let list = l["effects"].as_array_mut().ok_or("\"effects\" is not an array")?;
                list.push(json!({ "kind": kind, "params": params }));
                let n = list.len() - 1;
                self.changes.push(format!("added effect {kind} to {id}"));
                n
            }
        };
        Ok((format!("{lp}/effects/{ei}/params/{param}"), base, format!("{id}.{kind}.{param}")))
    }

    fn prop_at(&self, ptr: &str, missing: &Value) -> Result<(Value, Prop), String> {
        let v = self.doc.pointer(ptr).cloned().unwrap_or_else(|| missing.clone());
        let prop: Prop = serde_json::from_value(v.clone()).map_err(|e| format!("{ptr} is not an animatable value: {e}"))?;
        Ok((v, prop))
    }

    /// The keyframed part of a property (no modifiers) at `t`.
    fn orig(&self, prop: &Prop, t: f64) -> Vec<f64> {
        let keys_only = match prop {
            Prop::Anim(a) => Prop::Anim(Anim { keys: a.keys.clone(), base: a.base.clone(), modifiers: Vec::new() }),
            c => c.clone(),
        };
        Evaluator::new(&self.project).prop(&keys_only, t)
    }

    fn pulse(&mut self, spec: &Map<String, Value>) -> Result<(), String> {
        let (ptr, missing, label) = self.locate(spec)?;
        let (_, prop) = self.prop_at(&ptr, &missing)?;
        let keys = spec.get("keys").and_then(Value::as_array).ok_or("a pulse needs \"keys\": [{ \"dt\", \"v\", \"ease\" }, ...]")?.clone();
        let new = self.keys_from(&prop, &keys)?;
        let merged = self.merge(&prop, new)?;
        self.doc.pointer_mut(&ptr).map(|v| *v = merged.clone()).unwrap_or_else(|| {
            let _ = pointer::set(&mut self.doc, &ptr, merged.clone());
        });
        self.changes.push(format!("keyed {label} at {}", (self.at * 1000.0).round() / 1000.0));
        Ok(())
    }

    /// Keys of a pulse at absolute times, with `base` and `orig` in reach.
    fn keys_from(&self, prop: &Prop, keys: &[Value]) -> Result<Vec<Key>, String> {
        let base = self.orig(prop, self.at);
        let mut out: Vec<Key> = Vec::new();
        for k in keys {
            let mut env = self.env.clone();
            env.insert("base".into(), base.clone());
            for (c, x) in base.iter().enumerate() {
                env.insert(format!("base{c}"), vec![*x]);
            }
            let dt = match k.get("dt") {
                Some(d) => subst(d, &env, &self.vars)?.as_f64().ok_or("\"dt\" must be a number")?,
                None => 0.0,
            };
            // Rounded to a millionth of a beat or second: `at + dt` in floating
            // point leaves noise like 5.665999999 in the written project.
            let t = ((self.at + dt) * 1e6).round() / 1e6;
            let orig = self.orig(prop, t);
            env.insert("orig".into(), orig.clone());
            for (c, x) in orig.iter().enumerate() {
                env.insert(format!("orig{c}"), vec![*x]);
            }
            let v = subst(k.get("v").ok_or("every pulse key needs \"v\"")?, &env, &self.vars)?;
            let v: Value_ = serde_json::from_value(v.clone()).map_err(|_| format!("a key value must be a number or numbers, not {v}"))?;
            let ease = match k.get("ease") {
                Some(e) => serde_json::from_value(subst(e, &env, &self.vars)?).map_err(|e| format!("bad ease: {e}"))?,
                None => Ease::default(),
            };
            out.push(Key { t, v, ease });
        }
        if out.windows(2).any(|w| w[1].t < w[0].t) {
            return Err("the keys of a pulse must be in time order (\"dt\" increasing)".into());
        }
        Ok(out)
    }

    /// Put `new` keys into a property: what was before them is kept up to
    /// their start (held, so it does not drift toward them), what was after
    /// them is kept from their end.
    fn merge(&self, prop: &Prop, mut new: Vec<Key>) -> Result<Value, String> {
        let first = new.first().ok_or("a pulse needs at least one key")?.t;
        let last = new.last().map(|k| k.t).unwrap_or(first);
        let eps = 1e-6;
        let (old, base, modifiers) = match prop {
            Prop::Const(v) => (Vec::new(), Some(v.clone()), Vec::new()),
            Prop::Anim(a) => (a.keys.clone(), a.base.clone(), a.modifiers.clone()),
        };
        let start = self.target.map(|i| num_field(self.layer(i), "in").unwrap_or(0.0)).unwrap_or(0.0).min(first);
        let mut before: Vec<Key> = old.iter().filter(|k| k.t < first - eps).cloned().collect();
        let after: Vec<Key> = old.iter().filter(|k| k.t > last + eps).cloned().collect();
        let value = |v: Vec<f64>| if v.len() == 1 { Value_::Num(v[0]) } else { Value_::Vec(v) };
        let hold = Ease::Name("hold".into());
        match before.last_mut() {
            None => {
                if first > start + eps {
                    before.push(Key { t: start, v: value(self.orig(prop, first)), ease: hold });
                }
            }
            Some(k) => {
                let lead = 0.25 * self.tc.frame_len();
                let just_before = self.orig(prop, first - lead);
                if k.ease != hold && (first - lead) > k.t + eps && crate::eval::pad(k.v.to_vec(), just_before.len()) != just_before {
                    // The curve was moving: keep it until just before the hit.
                    before.push(Key { t: first - lead, v: value(just_before), ease: hold });
                } else {
                    k.ease = hold;
                }
            }
        }
        if !after.is_empty() {
            // Continue toward the next key the way the old curve did.
            let ease = old.iter().rev().find(|k| k.t <= last + eps).map(|k| k.ease.clone()).unwrap_or_default();
            if let Some(k) = new.last_mut() {
                if k.ease == Ease::default() {
                    k.ease = ease;
                }
            }
        }
        let mut keys = before;
        keys.extend(new);
        keys.extend(after);
        let anim = Anim { keys, base: if old.is_empty() { None } else { base }, modifiers };
        serde_json::to_value(Prop::Anim(anim)).map_err(|e| e.to_string())
    }

    fn shake(&mut self, spec: &Map<String, Value>) -> Result<(), String> {
        let (ptr, missing, label) = self.locate(spec)?;
        let (raw, prop) = self.prop_at(&ptr, &missing)?;
        let frequency = subst(spec.get("frequency").unwrap_or(&json!(12.0)), &self.env, &self.vars)?.as_f64().ok_or("\"frequency\" must be a number")?;
        let keys = spec.get("keys").and_then(Value::as_array).ok_or("a shake needs \"keys\" for its amplitude")?.clone();
        let mut anim = match prop {
            Prop::Const(v) => Anim { keys: Vec::new(), base: Some(v), modifiers: Vec::new() },
            Prop::Anim(a) => a,
        };
        let found = anim.modifiers.iter().position(|m| matches!(m, Modifier::Shake { frequency: f, .. } if (f - frequency).abs() < 1e-9));
        let mi = match found {
            Some(i) => i,
            None => {
                let seed = anim.modifiers.len() as u32 + 1;
                anim.modifiers.push(Modifier::Shake { amplitude: Prop::Const(Value_::Num(0.0)), frequency, seed });
                anim.modifiers.len() - 1
            }
        };
        let Modifier::Shake { amplitude, .. } = &anim.modifiers[mi] else { unreachable!() };
        let amp = amplitude.clone();
        let new = self.keys_from(&amp, &keys)?;
        let merged: Prop = serde_json::from_value(self.merge(&amp, new)?).map_err(|e| e.to_string())?;
        if let Modifier::Shake { amplitude, .. } = &mut anim.modifiers[mi] {
            *amplitude = merged;
        }
        let _ = raw;
        let v = serde_json::to_value(Prop::Anim(anim)).map_err(|e| e.to_string())?;
        pointer::set(&mut self.doc, &ptr, v)?;
        self.changes.push(format!("shook {label} at {}", (self.at * 1000.0).round() / 1000.0));
        Ok(())
    }

    fn add_layer(&mut self, spec: &Map<String, Value>) -> Result<(), String> {
        let mut layer = subst(spec.get("layer").ok_or("\"layer\" needs a \"layer\" object")?, &self.env, &self.vars)?;
        let target_id = self.target.map(|i| self.layer_id(i));
        let wanted = layer.get("id").and_then(Value::as_str).unwrap_or("preset").replace("{target}", target_id.as_deref().unwrap_or("fx"));
        let ids: Vec<String> = layers(&self.doc).iter().filter_map(|l| l.get("id").and_then(Value::as_str).map(str::to_string)).collect();
        let mut id = wanted.clone();
        let mut n = 2;
        while ids.contains(&id) {
            id = format!("{wanted}-{n}");
            n += 1;
        }
        layer["id"] = json!(id);
        if let Some(slot) = layer.get("slot").and_then(Value::as_str) {
            if self.doc.pointer(&format!("/slots/{}", pointer::escape(slot))).is_none() {
                let slots: Vec<String> = self.doc.get("slots").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
                return Err(format!("slot \"{slot}\" does not exist; slots: {}", slots.join(", ")));
            }
        }
        let place = spec.get("place").and_then(Value::as_str).unwrap_or("top");
        let ls = self.doc.get_mut("layers").and_then(Value::as_array_mut).ok_or("the project has no \"layers\"")?;
        let at = match (place, self.target) {
            ("above", Some(i)) => i + 1,
            ("below", Some(i)) => i,
            _ => ls.len(),
        };
        ls.insert(at, layer);
        if let Some(t) = self.target.as_mut() {
            if at <= *t {
                *t += 1;
            }
        }
        self.changes.push(format!("added layer {id}"));
        Ok(())
    }

    fn set(&mut self, spec: &Map<String, Value>) -> Result<(), String> {
        let ptr = spec.get("pointer").and_then(Value::as_str).ok_or("\"set\" needs a \"pointer\" inside the layer, e.g. /in")?;
        let i = self.target("\"set\"")?;
        let on = spec.get("on").and_then(Value::as_str).unwrap_or("target");
        let li = match on {
            "target" => i,
            "previous" => previous(&self.doc, i, self.at, self.tc.frame_len())
                .ok_or_else(|| format!("no picture below \"{}\" is visible just before {}: a transition needs the outgoing picture under the incoming one", self.layer_id(i), self.at))?,
            other => return Err(format!("\"on\" is \"target\" or \"previous\", not \"{other}\"")),
        };
        let value = subst(spec.get("value").ok_or("\"set\" needs a \"value\"")?, &self.env, &self.vars)?;
        let full = format!("/layers/{li}{ptr}");
        pointer::set(&mut self.doc, &full, value.clone())?;
        let id = self.layer_id(li);
        self.changes.push(format!("set {id}{ptr} = {}", crate::jsonfmt::brief(&value, 60)));
        Ok(())
    }

    /// Replay the target's keyed motion inside [at, at + length] through a
    /// speed curve: slow, then fast, then slow again, like a speed ramp.
    fn retime(&mut self, spec: &Map<String, Value>) -> Result<(), String> {
        let i = self.target("\"retime\"")?;
        let length = subst(spec.get("length").ok_or("\"retime\" needs \"length\"")?, &self.env, &self.vars)?.as_f64().ok_or("\"length\" must be a number")?;
        if length <= 0.0 {
            return Err("\"length\" must be positive".into());
        }
        let curve: Ease = match spec.get("curve") {
            Some(c) => serde_json::from_value(subst(c, &self.env, &self.vars)?).map_err(|e| format!("bad curve: {e}"))?,
            None => Ease::Bezier([0.8, 0.0, 0.2, 1.0]),
        };
        let (w0, w1) = (self.at, self.at + length);
        let mut retimed = 0;
        for name in ["position", "scale", "rotation", "opacity", "anchor"] {
            let ptr = format!("/layers/{i}/transform/{name}");
            let Some(raw) = self.doc.pointer(&ptr).cloned() else { continue };
            let Ok(Prop::Anim(a)) = serde_json::from_value::<Prop>(raw) else { continue };
            let prop = Prop::Anim(a.clone());
            let (v0, v1) = (self.orig(&prop, w0), self.orig(&prop, w1));
            let moving = a.keys.iter().any(|k| k.t > w0 && k.t < w1) || v0 != v1;
            if !moving {
                continue;
            }
            const N: usize = 12;
            let frames = length / self.tc.frame_len().max(1e-9);
            let mut keys: Vec<Key> = a.keys.iter().filter(|k| k.t < w0 - 1e-9).cloned().collect();
            for s in 0..=N {
                let u = s as f64 / N as f64;
                let src = w0 + length * ease::apply(&curve, u, frames);
                let v = self.orig(&prop, src);
                keys.push(Key { t: w0 + length * u, v: if v.len() == 1 { Value_::Num(v[0]) } else { Value_::Vec(v) }, ease: Ease::default() });
            }
            let ease_after = a.keys.iter().rev().find(|k| k.t <= w1 + 1e-9).map(|k| k.ease.clone()).unwrap_or_default();
            if let Some(k) = keys.last_mut() {
                k.ease = ease_after;
            }
            keys.extend(a.keys.iter().filter(|k| k.t > w1 + 1e-9).cloned());
            let v = serde_json::to_value(Prop::Anim(Anim { keys, base: a.base.clone(), modifiers: a.modifiers.clone() })).map_err(|e| e.to_string())?;
            pointer::set(&mut self.doc, &ptr, v)?;
            retimed += 1;
            let id = self.layer_id(i);
            self.changes.push(format!("retimed {id}.{name} over {w0}..{w1}"));
        }
        if retimed == 0 {
            match spec.get("otherwise").and_then(Value::as_array) {
                Some(actions) => {
                    for a in actions.clone() {
                        self.action(&a)?;
                    }
                }
                None => self.changes.push(format!("nothing moves on {} between {w0} and {w1}: nothing to retime", self.layer_id(i))),
            }
        }
        Ok(())
    }
}

/// The model's value type under a name that does not clash with JSON's.
type Value_ = crate::model::Value;

// ---- saving -------------------------------------------------------------------

/// Turn what a layer does between `from` and `to` (its keyed transform and
/// effect parameters, and its constant effects) into a preset that replays it
/// at any time on any layer. Values are copied as they are; times become
/// offsets from `at`.
pub fn from_layer(doc: &Value, id: &str, from: f64, to: f64, name: &str, description: &str) -> Result<Preset, String> {
    let i = index_of(doc, id)?;
    let layer = &layers(doc)[i];
    let mut apply = Vec::new();
    let keys_in = |prop: &Value| -> Vec<Value> {
        prop.get("keys")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|k| k.get("t").and_then(Value::as_f64).is_some_and(|t| t >= from - 1e-9 && t <= to + 1e-9))
            .map(|k| {
                let dt = k["t"].as_f64().unwrap_or(0.0) - from;
                let mut out = json!({ "dt": format!("={}", (dt * 1e6).round() / 1e6), "v": k["v"].clone() });
                if let Some(e) = k.get("ease") {
                    out["ease"] = e.clone();
                }
                out
            })
            .collect()
    };
    if let Some(tr) = layer.get("transform").and_then(Value::as_object) {
        for (name, prop) in tr {
            let keys = keys_in(prop);
            if !keys.is_empty() {
                apply.push(json!({ "pulse": { "transform": name, "keys": keys } }));
            }
        }
    }
    for e in layer.get("effects").and_then(Value::as_array).into_iter().flatten() {
        let kind = e.get("kind").and_then(Value::as_str).unwrap_or("");
        if kind == "custom" {
            continue;
        }
        let mut constant = Map::new();
        for (param, prop) in e.get("params").and_then(Value::as_object).into_iter().flatten() {
            let keys = keys_in(prop);
            if !keys.is_empty() {
                apply.push(json!({ "pulse": { "effect": kind, "param": param, "keys": keys } }));
            } else if !prop.is_object() {
                constant.insert(param.clone(), prop.clone());
            }
        }
        if !constant.is_empty() && !apply.iter().any(|a| a.pointer("/pulse/effect").and_then(Value::as_str) == Some(kind)) {
            apply.push(json!({ "effect": { "kind": kind, "params": constant } }));
        }
    }
    if apply.is_empty() {
        return Err(format!("layer \"{id}\" has no keys between {from} and {to} and no effects: nothing to save"));
    }
    let p = Preset {
        format: FORMAT.into(),
        name: name.into(),
        description: description.into(),
        tags: vec!["saved".into()],
        target: TargetKind::Layer,
        variables: BTreeMap::new(),
        apply,
        credit: None,
    };
    // What is written must load again.
    parse(&serde_json::to_string(&p).map_err(|e| e.to_string())?, name)?;
    Ok(p)
}

/// One line per variable, for listings.
pub fn variable_line(name: &str, v: &Variable) -> String {
    let mut s = format!("{name} ({}", serde_json::to_value(v.ty).ok().and_then(|t| t.as_str().map(str::to_string)).unwrap_or_default());
    if let Some(o) = &v.options {
        s.push_str(&format!(": {}", o.keys().cloned().collect::<Vec<_>>().join("|")));
    }
    s.push_str(&format!(", default {}", crate::jsonfmt::inline(&v.default)));
    match (v.min, v.max) {
        (Some(a), Some(b)) => s.push_str(&format!(", {a}..{b}")),
        (Some(a), None) => s.push_str(&format!(", >= {a}")),
        (None, Some(b)) => s.push_str(&format!(", <= {b}")),
        _ => {}
    }
    if let Some(u) = &v.unit {
        s.push_str(&format!(", {u}"));
    }
    s.push(')');
    if !v.doc.is_empty() {
        s.push_str(&format!(" {}", v.doc));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Value {
        json!({ "format": "edits.project/1",
            "settings": { "width": 64, "height": 64, "fps": 25, "duration": 8, "time_base": "beats", "bpm": 120 },
            "slots": { "a": {}, "b": {} },
            "layers": [
                { "id": "a", "kind": "image", "slot": "a", "in": 0, "out": 4 },
                { "id": "b", "kind": "image", "slot": "b", "in": 4, "out": 8, "transform": { "scale": { "keys": [ { "t": 4, "v": 1 }, { "t": 8, "v": 1.2 } ] } } },
                { "id": "grade", "kind": "adjustment", "effects": [ { "kind": "vignette" } ] }
            ] })
    }

    fn ctx(d: &Value) -> TimeCtx {
        TimeCtx::from_doc(d)
    }

    #[test]
    fn every_builtin_parses_and_names_are_unique() {
        let all = builtin();
        assert!(all.len() >= 22 + effects::TRANSITIONS.len());
        let mut names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len());
        for (file, _) in BUILTIN {
            assert!(all.iter().any(|p| p.name == *file), "presets/{file}.json is named after its file");
        }
    }

    #[test]
    fn expressions() {
        let mut env = Env::new();
        env.insert("$a".into(), vec![2.0]);
        env.insert("base".into(), vec![1.0, 2.0, 3.0]);
        assert_eq!(eval("1 + $a * 3", &env).unwrap(), vec![7.0]);
        assert_eq!(eval("-(1 + 1) / 4", &env).unwrap(), vec![-0.5]);
        assert_eq!(eval("base * $a + vec(1, 0, 0)", &env).unwrap(), vec![3.0, 4.0, 6.0]);
        assert_eq!(eval("max(1, $a, 0.5)", &env).unwrap(), vec![2.0]);
        assert!(eval("nope + 1", &env).unwrap_err().contains("unknown name"));
        assert!(eval("1 +", &env).is_err());
    }

    #[test]
    fn a_flash_keys_an_effect_and_holds_before_it() {
        let d = doc();
        let p = find("flash", None).unwrap();
        let (out, applied) = apply(&d, &p, &Target::Layer("a".into()), &[2.0], &Map::new(), &ctx(&d)).unwrap();
        let fx = &out["layers"][0]["effects"][0];
        assert_eq!(fx["kind"], "fade");
        let keys = fx["params"]["amount"]["keys"].as_array().unwrap();
        assert_eq!(keys[0]["t"], json!(0.0));
        assert_eq!(keys[0]["ease"], json!("hold"));
        assert_eq!(keys[1]["t"], json!(2.0));
        assert!(keys[1]["v"].as_f64().unwrap() > 0.5);
        assert_eq!(keys.last().unwrap()["v"], json!(0.0));
        assert!(applied[0].changes.iter().any(|c| c.contains("added effect fade")));
        // A second flash goes into the same effect.
        let (out2, _) = apply(&out, &p, &Target::Layer("a".into()), &[3.0], &Map::new(), &ctx(&out)).unwrap();
        assert_eq!(out2["layers"][0]["effects"].as_array().unwrap().len(), 1);
        let keys = out2["layers"][0]["effects"][0]["params"]["amount"]["keys"].as_array().unwrap();
        assert!(keys.iter().any(|k| k["t"] == json!(3.0)));
        let project: Project = serde_json::from_value(out2.clone()).unwrap();
        let ev = Evaluator::new(&project);
        let amount = |t: f64| ev.num(&project.layers[0].effects[0].params["amount"], t);
        assert_eq!(amount(1.9), 0.0);
        assert!(amount(2.0) > 0.5 && amount(3.0) > 0.5);
        assert!(amount(2.9) < 0.05, "the first flash has decayed: {}", amount(2.9));
    }

    #[test]
    fn a_punch_on_a_ramp_keeps_the_ramp() {
        let d = doc();
        let p = find("punch", None).unwrap();
        let (out, _) = apply(&d, &p, &Target::Layer("b".into()), &[6.0], &Map::new(), &ctx(&d)).unwrap();
        let project: Project = serde_json::from_value(out).unwrap();
        let ev = Evaluator::new(&project);
        let s = |t: f64| ev.num(&project.layers[1].transform.scale, t);
        assert!((s(5.0) - 1.05).abs() < 0.01, "the ramp before the hit: {}", s(5.0));
        assert!(s(6.0) > 1.15, "the punch: {}", s(6.0));
        assert!((s(8.0) - 1.2).abs() < 1e-6, "back on the ramp: {}", s(8.0));
    }

    #[test]
    fn variables_are_checked() {
        let d = doc();
        let p = find("flash", None).unwrap();
        let mut bad = Map::new();
        bad.insert("strenght".into(), json!(1));
        assert!(apply(&d, &p, &Target::Auto, &[1.0], &bad, &ctx(&d)).unwrap_err().contains("variables:"));
        let mut big = Map::new();
        big.insert("strength".into(), json!("9"));
        assert!(apply(&d, &p, &Target::Auto, &[1.0], &big, &ctx(&d)).unwrap_err().contains("above the maximum"));
        let vars = parse_assignments(&["color=#ff0000".into(), "strength=0.5".into()]).unwrap();
        let (out, _) = apply(&d, &p, &Target::Auto, &[1.0], &vars, &ctx(&d)).unwrap();
        assert_eq!(out["layers"][0]["effects"][0]["params"]["color"], json!([1.0, 0.0, 0.0]));
    }

    #[test]
    fn auto_and_fx_targets() {
        let d = doc();
        assert_eq!(picture_at(&d, 5.0).as_deref(), Some("b"));
        let p = find("rgb_split", None).unwrap();
        let (out, applied) = apply(&d, &p, &Target::Auto, &[1.0, 5.0], &Map::new(), &ctx(&d)).unwrap();
        assert_eq!(applied[0].layer.as_deref(), Some("a"));
        assert_eq!(applied[1].layer.as_deref(), Some("b"));
        let (out, _) = apply(&out, &p, &Target::Fx, &[2.0], &Map::new(), &ctx(&d)).unwrap();
        assert_eq!(out["layers"].as_array().unwrap().last().unwrap()["id"], FX_LAYER);
        // A punch on an adjustment layer zooms the image beneath instead.
        let (out, _) = apply(&out, &find("punch", None).unwrap(), &Target::Fx, &[3.0], &Map::new(), &ctx(&d)).unwrap();
        let fx = out["layers"].as_array().unwrap().last().unwrap();
        assert!(fx["effects"].as_array().unwrap().iter().any(|e| e["kind"] == "transform2d"), "{fx}");
    }

    #[test]
    fn a_transition_overlaps_the_cut() {
        let d = doc();
        let p = find("transition:fade", None).unwrap();
        let mut vars = Map::new();
        vars.insert("duration".into(), json!(1));
        let (out, _) = apply(&d, &p, &Target::Layer("b".into()), &[4.0], &vars, &ctx(&d)).unwrap();
        assert_eq!(out["layers"][0]["out"], json!(4.5));
        assert_eq!(out["layers"][1]["in"], json!(3.5));
        assert_eq!(out["layers"][1]["transition"]["kind"], "fade");
        assert_eq!(out["layers"][1]["transition"]["duration"], json!(1.0));
        // Nothing below to come from.
        assert!(apply(&d, &p, &Target::Layer("a".into()), &[0.0], &vars, &ctx(&d)).unwrap_err().contains("outgoing picture"));
    }

    #[test]
    fn saving_a_layer_makes_a_preset_that_replays_it() {
        let d = doc();
        let (out, _) = apply(&d, &find("punch", None).unwrap(), &Target::Layer("a".into()), &[1.0], &Map::new(), &ctx(&d)).unwrap();
        let saved = from_layer(&out, "a", 1.0, 2.0, "my_punch", "a punch").unwrap();
        let (again, _) = apply(&d, &saved, &Target::Layer("a".into()), &[3.0], &Map::new(), &ctx(&d)).unwrap();
        let keys = again["layers"][0]["transform"]["scale"]["keys"].as_array().unwrap();
        assert!(keys.iter().any(|k| k["t"] == json!(3.0)), "{keys:?}");
    }
}
