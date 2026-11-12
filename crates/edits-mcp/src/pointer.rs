//! JSON pointers (RFC 6901) with one addition for documents edited by agents:
//! inside an array, the token `@name` selects the element whose `id` is `name`
//! (layers), or, when the elements have no `id`, the one whose `kind` is `name`
//! (effects). Indices shift when a layer is inserted; ids do not.
//!
//! `/layers/@finish/effects/@exposure/params/stops` and
//! `/layers/4/effects/0/params/stops` name the same value in the demo project.

use serde_json::{Map, Value};

/// Split a pointer into its tokens. `""` is the whole document. A leading `#`
/// (URI fragment form) and a missing leading `/` are forgiven.
pub fn tokens(pointer: &str) -> Vec<String> {
    let p = pointer.trim();
    let p = p.strip_prefix('#').unwrap_or(p);
    if p.is_empty() {
        return Vec::new();
    }
    let p = p.strip_prefix('/').unwrap_or(p);
    p.split('/').map(|t| t.replace("~1", "/").replace("~0", "~")).collect()
}

/// Escape one token for use in a pointer.
pub fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Join tokens back into a pointer string.
pub fn join(tokens: &[String]) -> String {
    tokens.iter().map(|t| format!("/{}", escape(t))).collect()
}

fn here(done: &[String]) -> String {
    if done.is_empty() {
        "the document root".into()
    } else {
        join(done)
    }
}

fn list(mut names: Vec<String>) -> String {
    const MAX: usize = 24;
    let more = names.len().saturating_sub(MAX);
    names.truncate(MAX);
    let mut s = names.join(", ");
    if more > 0 {
        s.push_str(&format!(", and {more} more"));
    }
    if s.is_empty() {
        s.push_str("(none)");
    }
    s
}

fn field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// The index a token selects in an array. `end_ok` allows `-` and the index
/// one past the end (for appending).
pub fn index_in(array: &[Value], token: &str, done: &[String], end_ok: bool) -> Result<usize, String> {
    if let Some(name) = token.strip_prefix('@') {
        let by_id: Vec<usize> = array.iter().enumerate().filter(|(_, e)| field(e, "id") == Some(name)).map(|(i, _)| i).collect();
        let has_ids = array.iter().any(|e| e.get("id").is_some());
        let hits = if has_ids { by_id } else { array.iter().enumerate().filter(|(_, e)| field(e, "kind") == Some(name)).map(|(i, _)| i).collect() };
        let what = if has_ids { "id" } else { "kind" };
        return match hits.as_slice() {
            [i] => Ok(*i),
            [] => {
                let names: Vec<String> = array.iter().filter_map(|e| field(e, what)).map(str::to_string).collect();
                Err(format!("nothing in {} has {what} \"{name}\"; {what}s there: {}", here(done), list(names)))
            }
            many => Err(format!(
                "{} elements of {} have {what} \"{name}\"; use an index instead: {}",
                many.len(),
                here(done),
                many.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
            )),
        };
    }
    if token == "-" {
        return if end_ok { Ok(array.len()) } else { Err(format!("\"-\" (append) is only valid as the last token of a `set`; {} has {} elements", here(done), array.len())) };
    }
    let plain = !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()) && (token == "0" || !token.starts_with('0'));
    match token.parse::<usize>() {
        Ok(i) if plain && (i < array.len() || (end_ok && i == array.len())) => Ok(i),
        Ok(i) if plain => Err(format!("index {i} is past the end of {} ({} elements)", here(done), array.len())),
        _ => Err(format!("{} is an array: use an index, \"@id\" (the element's id, or kind for effects) or \"-\" to append, not \"{token}\"", here(done))),
    }
}

fn no_key(map: &Map<String, Value>, token: &str, done: &[String]) -> String {
    format!("{} has no key \"{token}\"; keys there: {}", here(done), list(map.keys().cloned().collect()))
}

fn scalar(v: &Value, token: &str, done: &[String]) -> String {
    let kind = match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        _ => "a value",
    };
    format!("{} is {kind}, so it has no \"{token}\" inside it", here(done))
}

/// The value a pointer names, with the pointer in canonical form
/// (`@id` kept for layers, so the answer reads the way the question did).
pub fn get<'a>(doc: &'a Value, pointer: &str) -> Result<&'a Value, String> {
    let toks = tokens(pointer);
    let mut cur = doc;
    for (i, tok) in toks.iter().enumerate() {
        let done = &toks[..i];
        cur = match cur {
            Value::Object(map) => map.get(tok).ok_or_else(|| no_key(map, tok, done))?,
            Value::Array(a) => &a[index_in(a, tok, done, false)?],
            other => return Err(scalar(other, tok, done)),
        };
    }
    Ok(cur)
}

/// Set the value at a pointer. Missing objects on the way are created, so
/// `/layers/@a/transform/scale` works on a layer that has no `transform` yet.
/// In an array the last token may be an existing index (replace), `-` or the
/// length (append).
pub fn set(doc: &mut Value, pointer: &str, value: Value) -> Result<(), String> {
    let toks = tokens(pointer);
    let Some((last, parents)) = toks.split_last() else {
        *doc = value;
        return Ok(());
    };
    let mut cur = doc;
    for (i, tok) in parents.iter().enumerate() {
        let done = &toks[..i];
        cur = match cur {
            Value::Object(map) => {
                if !map.contains_key(tok) {
                    let next = &toks[i + 1];
                    if next == "-" || next.starts_with('@') || next.parse::<usize>().is_ok() {
                        return Err(format!("{}; it would have to be an array: set the whole array at {} instead", no_key(map, tok, done), join(&toks[..=i])));
                    }
                    map.insert(tok.clone(), Value::Object(Map::new()));
                }
                map.get_mut(tok).expect("just checked")
            }
            Value::Array(a) => {
                let idx = index_in(a, tok, done, false)?;
                &mut a[idx]
            }
            other => return Err(scalar(other, tok, done)),
        };
    }
    match cur {
        Value::Object(map) => {
            map.insert(last.clone(), value);
        }
        Value::Array(a) => {
            let idx = index_in(a, last, parents, true)?;
            if idx == a.len() {
                a.push(value);
            } else {
                a[idx] = value;
            }
        }
        other => return Err(scalar(other, last, parents)),
    }
    Ok(())
}

/// Remove the key or array element at a pointer and return what was there.
pub fn remove(doc: &mut Value, pointer: &str) -> Result<Value, String> {
    let toks = tokens(pointer);
    let Some((last, parents)) = toks.split_last() else {
        return Err("cannot remove the whole document".into());
    };
    let mut cur = doc;
    for (i, tok) in parents.iter().enumerate() {
        let done = &toks[..i];
        cur = match cur {
            Value::Object(map) => {
                if !map.contains_key(tok) {
                    return Err(no_key(map, tok, done));
                }
                map.get_mut(tok).expect("just checked")
            }
            Value::Array(a) => {
                let idx = index_in(a, tok, done, false)?;
                &mut a[idx]
            }
            other => return Err(scalar(other, tok, done)),
        };
    }
    match cur {
        Value::Object(map) => {
            if !map.contains_key(last) {
                return Err(no_key(map, last, parents));
            }
            Ok(map.remove(last).expect("just checked"))
        }
        Value::Array(a) => {
            let idx = index_in(a, last, parents, false)?;
            Ok(a.remove(idx))
        }
        other => Err(scalar(other, last, parents)),
    }
}

/// Turn a path from `edits check` (`layers[4:"finish"].effects[2].params.radius`,
/// `slots.b`) into a pointer that `project_edit` accepts
/// (`/layers/@finish/effects/2/params/radius`, `/slots/b`).
pub fn from_check_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut rest = path;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('.') {
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix('[') {
            let (inside, after) = if let Some(colon) = r.find(":\"") {
                // [index:"id"]: the id may contain any character but a quote.
                let from = colon + 2;
                let close = r[from..].find("\"]")? + from;
                (format!("@{}", &r[from..close]), &r[close + 2..])
            } else {
                let close = r.find(']')?;
                (r[..close].to_string(), &r[close + 1..])
            };
            out.push('/');
            out.push_str(&escape(&inside));
            rest = after;
            continue;
        }
        let end = rest.find(['.', '[']).unwrap_or(rest.len());
        out.push('/');
        out.push_str(&escape(&rest[..end]));
        rest = &rest[end..];
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Value {
        json!({
            "settings": { "bpm": 123 },
            "slots": { "a/b": { "path": "x.png" } },
            "layers": [
                { "id": "a", "kind": "image", "slot": "a" },
                { "id": "finish", "kind": "adjustment", "effects": [
                    { "kind": "exposure", "params": { "stops": 0 } },
                    { "kind": "glow", "params": {} },
                    { "kind": "glow", "params": { "radius": 2 } }
                ] }
            ]
        })
    }

    #[test]
    fn plain_pointers_and_escapes() {
        let d = doc();
        assert_eq!(get(&d, "/settings/bpm").unwrap(), &json!(123));
        assert_eq!(get(&d, "").unwrap(), &d);
        assert_eq!(get(&d, "/slots/a~1b/path").unwrap(), &json!("x.png"));
        assert_eq!(get(&d, "/layers/1/id").unwrap(), &json!("finish"));
        // forgiven: no leading slash, URI fragment form
        assert_eq!(get(&d, "settings/bpm").unwrap(), &json!(123));
        assert_eq!(get(&d, "#/settings/bpm").unwrap(), &json!(123));
    }

    #[test]
    fn layers_by_id_and_effects_by_kind() {
        let d = doc();
        assert_eq!(get(&d, "/layers/@finish/kind").unwrap(), &json!("adjustment"));
        assert_eq!(get(&d, "/layers/@finish/effects/@exposure/params/stops").unwrap(), &json!(0));
        assert_eq!(get(&d, "/layers/@finish/effects/2/params/radius").unwrap(), &json!(2));
    }

    #[test]
    fn errors_say_what_exists() {
        let d = doc();
        let e = get(&d, "/layers/@nope").unwrap_err();
        assert!(e.contains("ids there: a, finish"), "{e}");
        let e = get(&d, "/layers/@finish/effects/@glow").unwrap_err();
        assert!(e.contains("2 elements") && e.contains("1, 2"), "{e}");
        let e = get(&d, "/settings/bmp").unwrap_err();
        assert!(e.contains("keys there: bpm"), "{e}");
        let e = get(&d, "/layers/7").unwrap_err();
        assert!(e.contains("past the end"), "{e}");
        let e = get(&d, "/layers/first").unwrap_err();
        assert!(e.contains("is an array"), "{e}");
        let e = get(&d, "/settings/bpm/x").unwrap_err();
        assert!(e.contains("is a number"), "{e}");
    }

    #[test]
    fn set_creates_objects_replaces_and_appends() {
        let mut d = doc();
        set(&mut d, "/layers/@a/transform/scale", json!(1.2)).unwrap();
        assert_eq!(d["layers"][0]["transform"]["scale"], json!(1.2));
        set(&mut d, "/layers/@finish/effects/0/params/stops", json!(4)).unwrap();
        assert_eq!(d["layers"][1]["effects"][0]["params"]["stops"], json!(4));
        set(&mut d, "/layers/@finish/effects/-", json!({ "kind": "vignette" })).unwrap();
        assert_eq!(d["layers"][1]["effects"].as_array().unwrap().len(), 4);
        set(&mut d, "/layers/@finish/effects/1", json!({ "kind": "grain" })).unwrap();
        assert_eq!(d["layers"][1]["effects"][1]["kind"], json!("grain"));
        // a missing array is not invented
        let e = set(&mut d, "/layers/@a/effects/-", json!({ "kind": "glow" })).unwrap_err();
        assert!(e.contains("set the whole array at /layers/@a/effects"), "{e}");
    }

    #[test]
    fn remove_keys_and_elements() {
        let mut d = doc();
        assert_eq!(remove(&mut d, "/layers/@finish/effects/@exposure").unwrap()["kind"], json!("exposure"));
        assert_eq!(d["layers"][1]["effects"].as_array().unwrap().len(), 2);
        assert_eq!(remove(&mut d, "/settings/bpm").unwrap(), json!(123));
        assert!(remove(&mut d, "/settings/bpm").is_err());
        assert!(remove(&mut d, "").is_err());
    }

    #[test]
    fn check_paths_become_pointers() {
        assert_eq!(from_check_path("layers[4:\"finish\"].effects[2].params.radius").as_deref(), Some("/layers/@finish/effects/2/params/radius"));
        assert_eq!(from_check_path("slots.b").as_deref(), Some("/slots/b"));
        assert_eq!(from_check_path("settings.motion_blur.samples").as_deref(), Some("/settings/motion_blur/samples"));
        assert_eq!(from_check_path("layers[0:\"a\"].transform.scale.keys[1].ease").as_deref(), Some("/layers/@a/transform/scale/keys/1/ease"));
        assert_eq!(from_check_path(""), None);
    }
}
