//! The one way project files are written: readable, compact and stable.
//!
//! - Keys are written in a fixed order: the order a person reads a project in
//!   (`id`, `kind`, ... `transform`, `effects`), then everything else by name.
//!   The same document always produces the same text, whatever order its keys
//!   arrived in, so diffs between versions show only real changes.
//! - A value that fits on one line stays on one line (a key, a small vector);
//!   anything longer gets one child per line.

use serde_json::Value;

/// Widest line written before a value is broken over several lines.
const WIDTH: usize = 110;

/// Reading order for the keys of a project. Keys not listed come after, sorted.
const ORDER: &[&str] = &[
    "format", "settings", "audio", "audio_clips", "slots", "camera", "layers", "markers",
    // layers, effects, modifiers
    "id", "kind", "name", "slot", "faces", "size", "color", "in", "out", "enabled", "blend", "source", "transform", "params", "effects",
    // settings
    "width", "height", "fps", "duration", "time_base", "bpm", "beat_offset", "background", "motion_blur", "samples", "shutter",
    // slots and audio
    "path", "fit", "focus", "note", "offset", "gain",
    // audio clips
    "at", "trim_in", "trim_out", "fade_in", "fade_out",
    // camera and transform
    "fov", "position", "scale", "rotation", "anchor", "opacity",
    // animation
    "keys", "base", "modifiers", "t", "v", "ease", "label", "amplitude", "frequency", "seed", "from", "period",
];

fn rank(key: &str) -> usize {
    ORDER.iter().position(|k| *k == key).unwrap_or(ORDER.len())
}

fn sorted_keys(map: &serde_json::Map<String, Value>) -> Vec<&String> {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    keys
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).expect("a string serialises")
}

/// The value on one line.
pub fn inline(v: &Value) -> String {
    let mut out = String::new();
    write_inline(v, &mut out);
    out
}

fn write_inline(v: &Value, out: &mut String) {
    match v {
        Value::Array(a) => {
            out.push('[');
            for (i, e) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_inline(e, out);
            }
            out.push(']');
        }
        Value::Object(o) if o.is_empty() => out.push_str("{}"),
        Value::Object(o) => {
            out.push_str("{ ");
            for (i, k) in sorted_keys(o).into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&quote(k));
                out.push_str(": ");
                write_inline(&o[k], out);
            }
            out.push_str(" }");
        }
        Value::String(s) => out.push_str(&quote(s)),
        other => out.push_str(&other.to_string()),
    }
}

fn write(v: &Value, indent: usize, used: usize, out: &mut String) {
    let flat = inline(v);
    let scalar = !matches!(v, Value::Array(_) | Value::Object(_));
    if scalar || used + flat.chars().count() <= WIDTH || flat.len() <= 2 {
        out.push_str(&flat);
        return;
    }
    let pad = "  ".repeat(indent + 1);
    match v {
        // A long list of numbers (a curve, a per-frame signal) fills its lines
        // instead of taking one line per number.
        Value::Array(a) if a.iter().all(|e| !matches!(e, Value::Array(_) | Value::Object(_))) => {
            out.push_str("[\n");
            let mut line = pad.clone();
            for (i, e) in a.iter().enumerate() {
                let mut item = inline(e);
                if i + 1 < a.len() {
                    item.push(',');
                }
                if line.len() > pad.len() && line.chars().count() + 1 + item.chars().count() > WIDTH {
                    out.push_str(line.trim_end());
                    out.push('\n');
                    line = pad.clone();
                }
                if line.len() > pad.len() {
                    line.push(' ');
                }
                line.push_str(&item);
            }
            out.push_str(&line);
            out.push('\n');
            out.push_str(&"  ".repeat(indent));
            out.push(']');
        }
        Value::Array(a) => {
            out.push_str("[\n");
            for (i, e) in a.iter().enumerate() {
                out.push_str(&pad);
                write(e, indent + 1, pad.len() + 1, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            out.push_str(&"  ".repeat(indent));
            out.push(']');
        }
        Value::Object(o) => {
            out.push_str("{\n");
            let keys = sorted_keys(o);
            for (i, k) in keys.iter().enumerate() {
                let head = format!("{pad}{}: ", quote(k));
                out.push_str(&head);
                write(&o[*k], indent + 1, head.chars().count() + 1, out);
                out.push_str(if i + 1 < keys.len() { ",\n" } else { "\n" });
            }
            out.push_str(&"  ".repeat(indent));
            out.push('}');
        }
        _ => unreachable!("scalars are written inline"),
    }
}

/// Pretty text without a trailing newline (for tool results).
pub fn pretty(v: &Value) -> String {
    let mut out = String::new();
    write(v, 0, 0, &mut out);
    out
}

/// Pretty text for a file: ends with a newline.
pub fn to_file_text(v: &Value) -> String {
    let mut out = pretty(v);
    out.push('\n');
    out
}

/// A value on one line, cut to `max` characters, for diff summaries.
pub fn brief(v: &Value, max: usize) -> String {
    let s = inline(v);
    if s.chars().count() <= max {
        return s;
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_and_is_stable() {
        let v = json!({
            "layers": [ { "effects": [ { "params": { "stops": { "keys": [ { "v": 4, "t": 0, "ease": { "rate": 0.25, "name": "decay" } }, { "t": 1, "v": 0 } ] } }, "kind": "exposure" } ],
                          "kind": "adjustment", "id": "finish", "zeta": 1, "alpha": 2 } ],
            "settings": { "fps": 25, "width": 1080, "height": 1080, "duration": 8, "time_base": "beats", "bpm": 123.0 },
            "format": "edits.project/1"
        });
        let text = to_file_text(&v);
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back, v, "formatting must not change the document");
        assert_eq!(to_file_text(&back), text, "same document, same text");
        // reading order, not alphabetical order
        let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle} missing in\n{text}"));
        assert!(at("\"format\"") < at("\"settings\"") && at("\"settings\"") < at("\"layers\""));
        assert!(at("\"id\"") < at("\"kind\": \"adjustment\"") && at("\"kind\": \"adjustment\"") < at("\"effects\""));
        assert!(at("\"t\": 0") < at("\"v\": 4") && at("\"name\": \"decay\"") < at("\"rate\""));
        // unknown keys come last, sorted
        assert!(at("\"effects\"") < at("\"alpha\"") && at("\"alpha\"") < at("\"zeta\""));
        assert!(text.lines().all(|l| l.chars().count() <= WIDTH + 2), "{text}");
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn long_lists_of_numbers_fill_their_lines() {
        let v = json!({ "lum": (0..200).map(|i| i as f64 * 1.5).collect::<Vec<_>>() });
        let text = pretty(&v);
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), v);
        assert!(text.lines().count() < 20, "200 numbers take a few lines, not 200:\n{text}");
        assert!(text.lines().all(|l| l.chars().count() <= WIDTH), "{text}");
    }

    #[test]
    fn small_values_stay_on_one_line() {
        assert_eq!(pretty(&json!({ "position": [0, 0.5, 0] })), r#"{ "position": [0, 0.5, 0] }"#);
        assert_eq!(pretty(&json!([])), "[]");
        assert_eq!(pretty(&json!({})), "{}");
        assert_eq!(brief(&json!("abcdefghij"), 6), "\"abcd…");
    }
}
