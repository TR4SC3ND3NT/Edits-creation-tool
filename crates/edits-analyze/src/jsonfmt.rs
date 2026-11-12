//! Compact, readable JSON: indented objects, but arrays of numbers and small
//! all-scalar objects (an estimate, a key) stay on one line.

use serde::Serialize;
use serde_json::Value;

pub fn to_string<T: Serialize>(v: &T) -> anyhow::Result<String> {
    let value = serde_json::to_value(v)?;
    let mut out = String::new();
    write(&value, 0, &mut out);
    out.push('\n');
    Ok(out)
}

fn scalar(v: &Value) -> bool {
    !matches!(v, Value::Array(_) | Value::Object(_))
}

fn inline(v: &Value) -> bool {
    match v {
        Value::Array(a) => a.iter().all(|e| scalar(e) || matches!(e, Value::Array(b) if b.iter().all(scalar))),
        Value::Object(o) => o.len() <= 6 && o.values().all(|e| scalar(e) || matches!(e, Value::Array(b) if b.len() <= 4 && b.iter().all(scalar))),
        _ => true,
    }
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
        Value::Object(o) => {
            out.push_str("{ ");
            for (i, (k, e)) in o.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push_str(": ");
                write_inline(e, out);
            }
            out.push_str(" }");
        }
        other => out.push_str(&other.to_string()),
    }
}

fn write(v: &Value, depth: usize, out: &mut String) {
    if inline(v) {
        write_inline(v, out);
        return;
    }
    let pad = "  ".repeat(depth + 1);
    match v {
        Value::Array(a) => {
            out.push_str("[\n");
            for (i, e) in a.iter().enumerate() {
                out.push_str(&pad);
                write(e, depth + 1, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            out.push_str(&"  ".repeat(depth));
            out.push(']');
        }
        Value::Object(o) => {
            out.push_str("{\n");
            let n = o.len();
            for (i, (k, e)) in o.iter().enumerate() {
                out.push_str(&pad);
                out.push_str(&Value::String(k.clone()).to_string());
                out.push_str(": ");
                write(e, depth + 1, out);
                out.push_str(if i + 1 < n { ",\n" } else { "\n" });
            }
            out.push_str(&"  ".repeat(depth));
            out.push('}');
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn output_parses_back_to_the_same_value() {
        let v = serde_json::json!({"a": [1, 2, 3], "b": {"value": 1.5, "unit": "s"}, "c": [{"k": [1, 2], "deep": {"x": {"y": [1, {"z": 2}]}}}], "d": "q\"uote"});
        let text = super::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap(), v);
        assert!(text.contains("\"a\": [1, 2, 3]"));
    }
}
