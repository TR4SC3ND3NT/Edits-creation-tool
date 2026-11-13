//! Compact views of what `edits-analyze` produces. The files and tables it
//! writes are exact and large; an agent needs the few hundred tokens that say
//! where to look.

use serde_json::{json, Map, Value};

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn round(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (v * k).round() / k
}

/// The `value` of an estimate (`{ value, unit, confidence, identifiability }`), or the number itself.
fn est(v: &Value) -> Option<f64> {
    v.get("value").and_then(Value::as_f64).or_else(|| v.as_f64())
}

// ---- probe -----------------------------------------------------------------

const COLUMNS: [&str; 6] = ["lum", "lum_center", "contrast", "sat", "sharp", "diff"];

/// Summarise the per-frame signature table of `edits-analyze probe --json`.
///
/// Returns the text for the model and the same facts as JSON. A range of at
/// most `max_rows` frames is listed frame by frame; a longer one is averaged
/// into `max_rows` buckets, with the strongest frame-to-frame changes (cut
/// candidates) and jumps in brightness (flash candidates) named exactly.
pub fn probe(data: &Value, from: Option<u64>, to: Option<u64>, max_rows: usize) -> Result<(String, Value), String> {
    let frames = data.get("frames").and_then(Value::as_array).ok_or("the probe output has no \"frames\" table")?;
    let video = data.get("video").cloned().unwrap_or(Value::Null);
    let fps = video.get("fps").and_then(Value::as_f64).filter(|v| *v > 0.0).unwrap_or(25.0);
    let total = frames.len() as u64;
    let first = from.unwrap_or(0).min(total);
    let last = to.unwrap_or(total).min(total);
    if first >= last {
        return Err(format!("the frame range {first}..{last} is empty; the video has {total} frames (0..{total})"));
    }
    let rows = &frames[first as usize..last as usize];
    let col = |name: &str| -> Vec<f64> { rows.iter().map(|r| f(&r[name])).collect() };
    let number = |r: &Value| r["frame"].as_u64().unwrap_or(0);

    let mut stats = Map::new();
    for name in COLUMNS {
        let v = col(name);
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let (min, max) = v.iter().fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
        stats.insert(name.into(), json!({ "min": round(min, 2), "mean": round(mean, 2), "max": round(max, 2) }));
    }

    // Cut candidates: frame differences that stand out from their surroundings.
    let diff = col("diff");
    let mut sorted = diff.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2];
    let mut peaks: Vec<(usize, f64)> = (0..diff.len())
        .filter(|&i| {
            let d = diff[i];
            let before = if i > 0 { diff[i - 1] } else { 0.0 };
            let after = diff.get(i + 1).copied().unwrap_or(0.0);
            d > 3.0 * median.max(1.0) && d >= before && d >= after
        })
        .map(|i| (i, diff[i]))
        .collect();
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    peaks.truncate(24);
    peaks.sort_by_key(|p| p.0);

    // Flash candidates: the centre of the frame gets much brighter in one frame.
    let lum = col("lum_center");
    let mut jumps: Vec<(usize, f64)> = (1..lum.len()).map(|i| (i, lum[i] - lum[i - 1])).filter(|(_, d)| *d > 25.0).collect();
    jumps.sort_by(|a, b| b.1.total_cmp(&a.1));
    jumps.truncate(24);
    jumps.sort_by_key(|j| j.0);

    let at = |i: usize| number(&rows[i]);
    let seconds = |frame: u64| round(frame as f64 / fps, 2);
    let mut text = String::new();
    text.push_str(&format!(
        "{} {}x{} {} fps, {} frames ({} s){}; showing frames {first}..{last}\n",
        video.get("path").and_then(Value::as_str).unwrap_or("video"),
        video.get("width").and_then(Value::as_u64).unwrap_or(0),
        video.get("height").and_then(Value::as_u64).unwrap_or(0),
        fps,
        total,
        round(total as f64 / fps, 2),
        if video.get("has_audio").and_then(Value::as_bool) == Some(true) { ", audio" } else { "" },
    ));
    text.push_str("columns: lum, lum_c (centre) and sat are 0..255; sharp is relative; diff is the mean change from the previous frame\n");
    text.push_str("range    ");
    for name in COLUMNS {
        let s = &stats[name];
        text.push_str(&format!(" {}: {}..{} (mean {})", if name == "lum_center" { "lum_c" } else { name }, s["min"], s["max"], s["mean"]));
    }
    text.push('\n');
    let fmt_list = |items: &[(usize, f64)], unit: &str| -> String {
        if items.is_empty() {
            return "none".into();
        }
        items.iter().map(|(i, v)| format!("#{} ({} s, {unit}{:.0})", at(*i), seconds(at(*i)), v)).collect::<Vec<_>>().join(", ")
    };
    text.push_str(&format!("largest frame changes (cut or hit candidates): {}\n", fmt_list(&peaks, "diff ")));
    text.push_str(&format!("brightness jumps (flash candidates): {}\n", fmt_list(&jumps, "+")));

    // The table: exact when short, bucketed when long.
    let step = rows.len().div_ceil(max_rows.max(1)).max(1);
    let mut table = Vec::new();
    if step == 1 {
        text.push_str("frame    lum  lum_c  contrast    sat  sharp    diff\n");
    } else {
        text.push_str(&format!("each row averages {step} frames (diff is the largest in the row); pass from_frame/to_frame to see a range frame by frame\n"));
        text.push_str("frames         lum  lum_c  contrast    sat  sharp  max diff\n");
    }
    for chunk in rows.chunks(step) {
        let mean = |name: &str| chunk.iter().map(|r| f(&r[name])).sum::<f64>() / chunk.len() as f64;
        let max_diff = chunk.iter().map(|r| f(&r["diff"])).fold(0.0, f64::max);
        let (a, b) = (number(&chunk[0]), number(&chunk[chunk.len() - 1]));
        if step == 1 {
            text.push_str(&format!("{a:5} {:6.1} {:6.1} {:9.1} {:6.1} {:6.3} {:7.2}\n", mean("lum"), mean("lum_center"), mean("contrast"), mean("sat"), mean("sharp"), max_diff));
        } else {
            text.push_str(&format!("{:<11} {:6.1} {:6.1} {:9.1} {:6.1} {:6.3} {:9.2}\n", format!("{a}-{b}"), mean("lum"), mean("lum_center"), mean("contrast"), mean("sat"), mean("sharp"), max_diff));
        }
        table.push(json!({ "from": a, "to": b, "lum": round(mean("lum"), 1), "lum_center": round(mean("lum_center"), 1), "contrast": round(mean("contrast"), 1), "sat": round(mean("sat"), 1), "sharp": round(mean("sharp"), 3), "max_diff": round(max_diff, 2) }));
    }
    let structured = json!({
        "video": video,
        "frames": [first, last],
        "stats": stats,
        "frame_changes": peaks.iter().map(|(i, v)| json!({ "frame": at(*i), "seconds": seconds(at(*i)), "diff": round(*v, 2) })).collect::<Vec<_>>(),
        "brightness_jumps": jumps.iter().map(|(i, v)| json!({ "frame": at(*i), "seconds": seconds(at(*i)), "rise": round(*v, 1) })).collect::<Vec<_>>(),
        "frames_per_row": step,
        "table": table,
    });
    Ok((text.trim_end().to_string(), structured))
}

// ---- reference analysis ----------------------------------------------------

fn shot_events(shot: &Value) -> Vec<String> {
    shot.get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|e| {
            let kind = e["kind"].as_str().unwrap_or("event");
            let beat = e.get("beat_snapped").and_then(Value::as_f64).unwrap_or_else(|| f(&e["beat"]));
            let detail = match kind {
                "flash" => {
                    let peak = est(&e["params"]["peak"]).map(|p| format!(" {:.1} stops", p)).unwrap_or_default();
                    let rate = est(&e["params"]["decay_rate"]).map(|r| format!(", decay {:.2}", r)).unwrap_or_default();
                    format!("{peak}{rate}")
                }
                _ => format!(" frames {}..{}", e["f0"], e["f1"]),
            };
            format!("{kind}@beat {}{detail}", round(beat, 2))
        })
        .collect()
}

fn shot_line(shot: &Value) -> Value {
    let curves: Vec<String> = shot
        .get("curves")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| format!("{} [{} keys, {}]", c["prop"].as_str().unwrap_or("?"), c["keys"].as_array().map_or(0, Vec::len), c["identifiability"].as_str().unwrap_or("?")))
        .collect();
    json!({
        "id": shot["id"],
        "frames": [shot["f0"], shot["f1"]],
        "beats": [shot.get("beat0_snapped").filter(|v| !v.is_null()).unwrap_or(&shot["beat0"]), shot.get("beat1_snapped").filter(|v| !v.is_null()).unwrap_or(&shot["beat1"])],
        "slot": shot["slot"],
        "status": shot["status"],
        "cut_grid": shot["cut"]["grid"],
        "events": shot_events(shot),
        "curves": curves,
        "notes": shot.get("notes").cloned().unwrap_or(json!([])),
    })
}

/// The few facts an agent needs before it opens any section of an analysis.
pub fn analysis_summary(a: &Value) -> Value {
    let shots = a.get("shots").and_then(Value::as_array).cloned().unwrap_or_default();
    let needs: Vec<Value> = shots.iter().filter(|s| s["status"].as_str() != Some("measured")).map(|s| json!({ "shot": s["id"], "status": s["status"], "why": s.get("notes").cloned().unwrap_or(json!([])) })).collect();
    let flashes: Vec<Value> = shots
        .iter()
        .flat_map(|s| {
            let id = s["id"].clone();
            s.get("events").and_then(Value::as_array).into_iter().flatten().filter(|e| e["kind"].as_str() == Some("flash")).map(move |e| {
                json!({
                    "shot": id,
                    "frame": e["frame"],
                    "beat": e.get("beat_snapped").filter(|v| !v.is_null()).unwrap_or(&e["beat"]),
                    "peak_stops": est(&e["params"]["peak"]).map(|v| round(v, 2)),
                    "decay_rate": est(&e["params"]["decay_rate"]).map(|v| round(v, 3)),
                })
            }).collect::<Vec<_>>()
        })
        .collect();
    let tone_hits: Vec<Value> = shots
        .iter()
        .flat_map(|s| {
            let id = s["id"].clone();
            s.get("events").and_then(Value::as_array).into_iter().flatten().filter(|e| e["kind"].as_str() != Some("flash")).map(move |e| json!({ "shot": id, "kind": e["kind"], "frames": [e["f0"], e["f1"]], "beat": e.get("beat_snapped").filter(|v| !v.is_null()).unwrap_or(&e["beat"]) })).collect::<Vec<_>>()
        })
        .collect();
    let beats = &a["beats"];
    let loop_info = a["ingest"].get("loop_info").filter(|v| !v.is_null()).map(|l| {
        json!({ "first_frame": l["first_frame"], "period_frames": est(&l["period_frames"]), "period_beats": l.get("period_beats").and_then(est), "repeats": l["repeats"] })
    });
    let layers = &a["layers"];
    json!({
        "source": {
            "size": [a["source"]["width"], a["source"]["height"]],
            "fps": a["source"]["fps"],
            "frames": a["source"]["frames"],
            "seconds": a["source"]["duration_seconds"],
            "audio": a["source"]["has_audio"],
        },
        "tempo": {
            "bpm": est(&beats["bpm"]),
            "confidence": beats["bpm"]["confidence"],
            "from": beats["source"],
            "beat_offset_seconds": est(&beats["offset_seconds"]),
            "frames_per_beat": beats["frames_per_beat"],
        },
        "loop": loop_info,
        "analysed_frames": [a["ingest"]["analysed_f0"], a["ingest"]["analysed_f1"]],
        "shot_count": shots.len(),
        "shots": shots.iter().map(|s| json!({ "id": s["id"], "beats": [s.get("beat0_snapped").filter(|v| !v.is_null()).unwrap_or(&s["beat0"]), s.get("beat1_snapped").filter(|v| !v.is_null()).unwrap_or(&s["beat1"])], "slot": s["slot"], "status": s["status"] })).collect::<Vec<_>>(),
        "flash_events": flashes,
        "tone_hits": tone_hits,
        "slots": a["slots"],
        "overlay": layers.get("overlay").filter(|v| !v.is_null()).map(|o| json!({ "path": o["path"], "blend_guess": o["blend_guess"], "coverage": est(&o["coverage"]) })),
        "vignette": layers.get("vignette").filter(|v| !v.is_null()).map(|v| json!({ "amount": est(&v["amount"]), "radius": est(&v["radius"]), "softness": est(&v["softness"]) })),
        "needs_agent": needs,
        "notes": a.get("notes").cloned().unwrap_or(json!([])),
    })
}

/// The summary as lines of text.
pub fn analysis_summary_text(s: &Value) -> String {
    let mut out = String::new();
    let src = &s["source"];
    out.push_str(&format!("source: {}x{} at {} fps, {} frames ({} s), audio: {}\n", src["size"][0], src["size"][1], src["fps"], src["frames"], src["seconds"], src["audio"]));
    let t = &s["tempo"];
    out.push_str(&format!("tempo: {} bpm (confidence {}, from {}), beat 0 at {} s, {} frames per beat\n", t["bpm"], t["confidence"], t["from"].as_str().unwrap_or("?"), t["beat_offset_seconds"], t["frames_per_beat"]));
    match &s["loop"] {
        Value::Null => out.push_str("loop: none detected\n"),
        l => out.push_str(&format!("loop: every {} frames ({} beats) from frame {}, {} repeats\n", l["period_frames"], l["period_beats"], l["first_frame"], l["repeats"])),
    }
    out.push_str(&format!("shots: {} (analysed frames {}..{})\n", s["shot_count"], s["analysed_frames"][0], s["analysed_frames"][1]));
    for shot in s["shots"].as_array().into_iter().flatten() {
        out.push_str(&format!("  shot {}: beats {}..{}, slot {}, {}\n", shot["id"], shot["beats"][0], shot["beats"][1], shot["slot"].as_str().unwrap_or("?"), shot["status"].as_str().unwrap_or("?")));
    }
    let flashes = s["flash_events"].as_array().cloned().unwrap_or_default();
    out.push_str(&format!("flashes: {}\n", flashes.len()));
    for e in &flashes {
        out.push_str(&format!("  shot {} beat {} (frame {}): {} stops, decay rate {}\n", e["shot"], e["beat"], e["frame"], e["peak_stops"], e["decay_rate"]));
    }
    let hits = s["tone_hits"].as_array().cloned().unwrap_or_default();
    if !hits.is_empty() {
        let list: Vec<String> = hits.iter().map(|h| format!("{} shot {} frames {}..{}", h["kind"].as_str().unwrap_or("?"), h["shot"], h["frames"][0], h["frames"][1])).collect();
        out.push_str(&format!("tone hits: {}\n", list.join("; ")));
    }
    let slots: Vec<String> = s["slots"].as_array().into_iter().flatten().map(|sl| format!("{} (shots {}, proxy {})", sl["id"].as_str().unwrap_or("?"), jsonish(&sl["shots"]), sl["proxy"].as_str().unwrap_or("none"))).collect();
    out.push_str(&format!("slots: {}\n", if slots.is_empty() { "none".into() } else { slots.join("; ") }));
    if !s["overlay"].is_null() {
        out.push_str(&format!("overlay: {} (blend guess {}, covers {} of the frame)\n", s["overlay"]["path"].as_str().unwrap_or("none"), s["overlay"]["blend_guess"].as_str().unwrap_or("?"), s["overlay"]["coverage"]));
    }
    if !s["vignette"].is_null() {
        out.push_str(&format!("vignette: amount {}, radius {}, softness {}\n", s["vignette"]["amount"], s["vignette"]["radius"], s["vignette"]["softness"]));
    }
    let needs = s["needs_agent"].as_array().cloned().unwrap_or_default();
    if needs.is_empty() {
        out.push_str("every shot is `measured`\n");
    } else {
        out.push_str("look at these yourself (not fully measured):\n");
        for n in &needs {
            let why: Vec<&str> = n["why"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            out.push_str(&format!("  shot {} is `{}`: {}\n", n["shot"], n["status"].as_str().unwrap_or("?"), if why.is_empty() { "no note".into() } else { why.join("; ") }));
        }
    }
    for note in s["notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        out.push_str(&format!("note: {note}\n"));
    }
    out.trim_end().to_string()
}

fn jsonish(v: &Value) -> String {
    crate::jsonfmt::inline(v)
}

pub const SECTIONS: &str = "summary, source, ingest, grid, shots, shot (with `shot`: an id), slots, layers, signature, notes";

/// One section of a `reference_analysis.json`.
pub fn analysis_section(a: &Value, section: &str, shot: Option<u64>) -> Result<Value, String> {
    let shots = || a.get("shots").and_then(Value::as_array).cloned().unwrap_or_default();
    match section {
        "summary" => Ok(analysis_summary(a)),
        "source" | "ingest" | "slots" | "layers" | "signature" | "notes" => a.get(section).cloned().ok_or_else(|| format!("the analysis has no \"{section}\"")),
        "grid" | "beats" => a.get("beats").cloned().ok_or_else(|| "the analysis has no \"beats\"".to_string()),
        "shots" => Ok(Value::Array(shots().iter().map(shot_line).collect())),
        "shot" => {
            let all = shots();
            let ids: Vec<String> = all.iter().map(|s| s["id"].to_string()).collect();
            let id = shot.ok_or_else(|| format!("section \"shot\" needs `shot`: one of {}", ids.join(", ")))?;
            let mut found = all.into_iter().find(|s| s["id"].as_u64() == Some(id)).ok_or_else(|| format!("no shot {id}; shot ids: {}", ids.join(", ")))?;
            // Raw per-frame samples are for plotting, not for reading; say they were left out.
            if let Some(obj) = found.as_object_mut() {
                if obj.remove("samples").is_some() {
                    obj.insert("samples".into(), json!("omitted: per-frame track samples; read them with pointer /shots/<index>/samples"));
                }
            }
            Ok(found)
        }
        other => Err(format!("unknown section \"{other}\"; sections: {SECTIONS}")),
    }
}

// ---- comparison ------------------------------------------------------------

/// Trim the output of `edits-analyze compare --json` to scores and the `top`
/// largest differences, and say the same in text.
pub fn comparison(c: &Value, top: usize) -> (String, Value) {
    let all = c.get("differences").and_then(Value::as_array).cloned().unwrap_or_default();
    let shown: Vec<Value> = all.iter().take(top).cloned().collect();
    let mut text = String::new();
    text.push_str(&format!("overall {} (1.00 = same recipe)\n", c["overall"]));
    let scores: Vec<String> = c["scores"].as_object().into_iter().flatten().map(|(k, v)| format!("{k} {}", if v.is_null() { "n/a".to_string() } else { v.to_string() })).collect();
    text.push_str(&format!("scores: {}\n", scores.join(", ")));
    let t = &c["tempo"];
    text.push_str(&format!("tempo: reference {} bpm, candidate {} bpm ({})\n", t["reference_bpm"], if t["candidate_bpm"].is_null() { "unknown".to_string() } else { t["candidate_bpm"].to_string() }, t["note"].as_str().unwrap_or("")));
    for (key, label) in [("cuts", "cuts"), ("flashes", "flashes"), ("tone_hits", "tone hits")] {
        let m = &c[key];
        if m.is_null() {
            continue;
        }
        text.push_str(&format!("{label}: {} in the reference, {} in the candidate, {} matched", m["reference"], m["candidate"], m["matched"]));
        for (key, what) in [("missing", "missing from the candidate at reference frames"), ("extra", "extra in the candidate at frames")] {
            if m[key].as_array().is_some_and(|a| !a.is_empty()) {
                text.push_str(&format!("; {what} {}", jsonish(&m[key])));
            }
        }
        text.push('\n');
    }
    if all.is_empty() {
        text.push_str("no differences above tolerance\n");
    } else {
        text.push_str(&format!("largest differences ({} of {}), most severe first:\n", shown.len(), all.len()));
        for (i, d) in shown.iter().enumerate() {
            text.push_str(&format!("{:3}. [{} {}] {}\n", i + 1, d["category"].as_str().unwrap_or("?"), d["severity"], d["text"].as_str().unwrap_or("")));
        }
    }
    let structured = json!({
        "overall": c["overall"],
        "scores": c["scores"],
        "tempo": c["tempo"],
        "cuts": c["cuts"],
        "flashes": c["flashes"],
        "tone_hits": c["tone_hits"],
        "difference_count": all.len(),
        "differences": shown,
    });
    (text.trim_end().to_string(), structured)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(n: u64) -> Value {
        let frames: Vec<Value> = (0..n)
            .map(|i| {
                // a cut at frame 50 and a flash at frame 120
                let diff = if i == 50 { 60.0 } else { 2.0 };
                let lum = if i == 120 { 230.0 } else if i == 121 { 180.0 } else { 90.0 };
                json!({ "frame": i, "lum": lum, "lum_center": lum, "contrast": 40.0, "sat": 50.0, "sharp": 1.0, "diff": diff })
            })
            .collect();
        json!({ "video": { "path": "ref.mp4", "width": 540, "height": 960, "fps": 25.0, "frames": n, "has_audio": true }, "frames": frames })
    }

    #[test]
    fn a_long_probe_table_is_bucketed_and_its_events_are_exact() {
        let (text, s) = probe(&table(600), None, None, 60).unwrap();
        assert_eq!(s["table"].as_array().unwrap().len(), 60);
        assert_eq!(s["frames_per_row"], json!(10));
        assert_eq!(s["frame_changes"][0]["frame"], json!(50));
        assert_eq!(s["brightness_jumps"][0]["frame"], json!(120));
        assert!(text.contains("#50 (2 s, diff 60)"), "{text}");
        assert!(text.len() < 8_000, "a 600-frame table stays compact: {} chars", text.len());
        // a short range is listed frame by frame
        let (text, s) = probe(&table(600), Some(45), Some(55), 60).unwrap();
        assert_eq!(s["frames_per_row"], json!(1));
        assert_eq!(s["table"].as_array().unwrap().len(), 10);
        assert!(text.contains("\n   50 "), "{text}");
        assert!(probe(&table(10), Some(20), None, 60).unwrap_err().contains("is empty"));
    }

    fn analysis() -> Value {
        json!({
            "source": { "width": 540, "height": 960, "fps": 25.0, "frames": 200, "duration_seconds": 8.0, "has_audio": true },
            "ingest": { "analysed_f0": 0, "analysed_f1": 98, "loop_info": { "first_frame": 0, "period_frames": { "value": 98.0 }, "period_beats": { "value": 8.0 }, "repeats": 2.04 } },
            "beats": { "source": "audio", "bpm": { "value": 123.0, "confidence": 0.8 }, "offset_seconds": { "value": 0.01 }, "frames_per_beat": 12.2 },
            "shots": [
                { "id": 1, "f0": 0, "f1": 25, "beat0": 0.01, "beat0_snapped": 0.0, "beat1": 2.06, "beat1_snapped": 2.0, "slot": "p01", "status": "measured", "cut": { "grid": "beat" },
                  "curves": [ { "prop": "scale", "identifiability": "relative_only", "keys": [1, 2] } ],
                  "events": [ { "kind": "flash", "frame": 0, "f0": 0, "f1": 4, "beat": 0.01, "beat_snapped": 0.0, "params": { "peak": { "value": 3.5 }, "decay_rate": { "value": 0.25 } } } ],
                  "samples": { "x": [1, 2, 3] } },
                { "id": 2, "f0": 25, "f1": 98, "beat0": 2.06, "beat1": 8.0, "slot": "p02", "status": "needs_agent", "cut": { "grid": "beat" }, "notes": ["3D cube: pose not measured"],
                  "curves": [], "events": [ { "kind": "saturation", "frame": 37, "f0": 37, "f1": 40, "beat": 3.05, "params": {} } ] }
            ],
            "slots": [ { "id": "p01", "shots": [1], "proxy": "proxies/p01.png" } ],
            "layers": { "overlay": { "path": "overlay.png", "blend_guess": "screen", "coverage": { "value": 0.02 } }, "vignette": { "amount": { "value": 0.4 }, "radius": { "value": 0.6 }, "softness": { "value": 0.5 } } },
            "notes": ["one note"]
        })
    }

    #[test]
    fn the_analysis_summary_says_what_to_look_at() {
        let s = analysis_summary(&analysis());
        assert_eq!(s["tempo"]["bpm"], json!(123.0));
        assert_eq!(s["loop"]["period_beats"], json!(8.0));
        assert_eq!(s["shot_count"], json!(2));
        assert_eq!(s["flash_events"][0]["peak_stops"], json!(3.5));
        assert_eq!(s["needs_agent"][0]["shot"], json!(2));
        let text = analysis_summary_text(&s);
        assert!(text.contains("tempo: 123.0 bpm"), "{text}");
        assert!(text.contains("shot 2 is `needs_agent`: 3D cube: pose not measured"), "{text}");
        assert!(text.contains("shot 1 beat 0.0 (frame 0): 3.5 stops, decay rate 0.25"), "{text}");
    }

    #[test]
    fn sections_are_small_and_errors_list_the_choices() {
        let a = analysis();
        assert_eq!(analysis_section(&a, "grid", None).unwrap()["source"], json!("audio"));
        let shots = analysis_section(&a, "shots", None).unwrap();
        assert_eq!(shots[0]["events"][0], json!("flash@beat 0 3.5 stops, decay 0.25"));
        assert!(shots[0].get("samples").is_none());
        let one = analysis_section(&a, "shot", Some(1)).unwrap();
        assert!(one["samples"].as_str().unwrap().starts_with("omitted"));
        assert!(analysis_section(&a, "shot", None).unwrap_err().contains("one of 1, 2"));
        assert!(analysis_section(&a, "shot", Some(9)).unwrap_err().contains("shot ids: 1, 2"));
        assert!(analysis_section(&a, "curves", None).unwrap_err().contains("sections: summary"));
    }

    #[test]
    fn a_comparison_is_ranked_and_cut_to_the_top() {
        let c = json!({
            "overall": 0.64,
            "scores": { "cuts": 0.67, "exposure": 0.21, "scale": null },
            "tempo": { "reference_bpm": 123.0, "candidate_bpm": 123.0, "note": "from audio" },
            "cuts": { "reference": 2, "candidate": 1, "matched": 1, "missing": [43], "extra": [] },
            "differences": [
                { "category": "exposure", "severity": 9.3, "shot": 3, "text": "shot 3: exposure peaks at 0.17 stops, reference -4.46 stops" },
                { "category": "cuts", "severity": 4.0, "shot": 3, "text": "cut at beat 3.54 has no match" },
                { "category": "blur", "severity": 3.5, "shot": 1, "text": "blur starts lower" }
            ],
            "shots": [ { "big": "table" } ]
        });
        let (text, s) = comparison(&c, 2);
        assert!(text.starts_with("overall 0.64"), "{text}");
        assert!(text.contains("largest differences (2 of 3)"), "{text}");
        assert!(text.contains("  1. [exposure 9.3] shot 3: exposure peaks"), "{text}");
        assert!(text.contains("scale n/a"), "{text}");
        assert!(text.contains("cuts: 2 in the reference, 1 in the candidate, 1 matched; missing from the candidate at reference frames [43]\n"), "{text}");
        assert_eq!(s["differences"].as_array().unwrap().len(), 2);
        assert_eq!(s["difference_count"], json!(3));
        assert!(s.get("shots").is_none(), "per-shot tables are left out");
    }
}

/// The result of `edits fit --json` as a short text and a compact structure:
/// whether the project was written, overall and per-category scores before
/// and after, and the values that changed.
pub fn fit_report(r: &Value) -> (String, Value) {
    let f = |v: &Value| v.as_f64().map(|x| format!("{x:.2}")).unwrap_or_else(|| "n/a".into());
    let written = r["written"].as_bool().unwrap_or(false);
    let changes = r["changes"].as_array().cloned().unwrap_or_default();
    let mut text = format!(
        "overall {} -> {}; {} value(s) changed; {} renders; {}",
        f(&r["overall_before"]),
        f(&r["overall_after"]),
        changes.len(),
        r["renders"],
        if written { "the project was written (one history entry; project_undo takes it back)" } else { "the project was NOT written" }
    );
    let (before, after) = (&r["before"], &r["after"]);
    if let Some(cats) = before.as_object() {
        let line: Vec<String> = cats.keys().map(|k| format!("{k} {} -> {}", f(&before[k]), f(&after[k]))).collect();
        text += &format!("\nscores: {}", line.join(", "));
    }
    const SHOWN: usize = 15;
    for c in changes.iter().take(SHOWN) {
        text += &format!("\n  {} {}: {} -> {}", c["layer"].as_str().unwrap_or("?"), c["what"].as_str().unwrap_or("?"), f(&c["old"]), f(&c["new"]));
    }
    if changes.len() > SHOWN {
        text += &format!("\n  ... and {} more", changes.len() - SHOWN);
    }
    for n in r["notes"].as_array().into_iter().flatten() {
        text += &format!("\nnote: {}", n.as_str().unwrap_or(""));
    }
    if !written && !r["notes"].as_array().is_some_and(|n| !n.is_empty()) && changes.is_empty() {
        text += "\nnothing to change: no parameter improved the match.";
    }
    let compact = json!({
        "written": written,
        "overall_before": r["overall_before"],
        "overall_after": r["overall_after"],
        "before": before,
        "after": after,
        "changes": changes.iter().map(|c| json!({ "layer": c["layer"], "pointer": c["pointer"], "what": c["what"], "old": c["old"], "new": c["new"] })).collect::<Vec<_>>(),
        "renders": r["renders"],
        "seconds": r["seconds"],
        "notes": r["notes"],
    });
    (text, compact)
}
