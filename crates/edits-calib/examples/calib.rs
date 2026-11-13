//! Run the calibration harness from the command line.
//!
//! ```text
//! cargo run --release -p edits-calib --example calib -- run  [--cases 36] [--seed 1] [--height 320] [--degrade 1500] [--no-roundtrip] [--no-audio] [--out DIR]
//! cargo run --release -p edits-calib --example calib -- case <case.json> [--out DIR]
//! cargo run --release -p edits-calib --example calib -- fit <project.json> <reference_analysis.json> [--dry-run] [--budget N] [--shots 1,2] [--params exposure,blur] [--json OUT]
//! cargo run --release -p edits-calib --example calib -- sig <reference_analysis.json> <video>
//! ```
//!
//! `run` writes `report.txt`, `report.json` and the worst cases into `--out`
//! (default `calib/`, which is ignored by git); `case` replays one saved case
//! and prints the truth next to what the analysis found
//! (`EDITS_CALIB_FRAMES=1` adds a per-frame table).

use anyhow::{bail, Context, Result};
use edits_calib::{harness, Degrade, RunOpts};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let num = |name: &str, d: u64| -> Result<u64> { value(name).map(|v| v.parse::<u64>().with_context(|| format!("{name} takes a number"))).transpose().map(|v| v.unwrap_or(d)) };
    match args.first().map(String::as_str) {
        Some("run") => {
            let opts = RunOpts {
                cases: num("--cases", 36)? as usize,
                seed: num("--seed", 1)?,
                height: num("--height", 320)? as u32,
                degrade: value("--degrade").map(|k| k.parse::<u32>().map(|kbps| Degrade { kbps, scale: 1.0 })).transpose().context("--degrade takes a bitrate in kbit/s")?,
                roundtrip: !flag("--no-roundtrip"),
                audio: !flag("--no-audio"),
                out: PathBuf::from(value("--out").unwrap_or_else(|| "calib".into())),
                worst: num("--worst", 6)? as usize,
            };
            let report = harness::run(&opts, |line| eprintln!("{line}"))?;
            print!("{}", edits_calib::report::table(&report));
            eprintln!("wrote {}", opts.out.join("report.txt").display());
        }
        Some("case") => {
            let Some(path) = args.get(1) else { bail!("usage: calib case <case.json> [--out DIR]") };
            let out = PathBuf::from(value("--out").unwrap_or_else(|| "calib".into()));
            let r = harness::replay(std::path::Path::new(path), &out.join("work"))?;
            print!("{}", harness::describe(&r.outcome));
            print!("{}", edits_calib::report::table(&r.report));
        }
        Some("fit") => {
            let (Some(p), Some(r)) = (args.get(1), args.get(2)) else { bail!("usage: calib fit <project.json> <reference_analysis.json> [--dry-run] [--budget N] [--shots 1,2] [--params a,b] [--scale S] [--json OUT]") };
            let list = |name: &str| value(name).map(|v| v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>());
            let opts = edits_calib::fit::FitOpts {
                shots: list("--shots").unwrap_or_default().iter().map(|x| x.parse::<usize>()).collect::<Result<_, _>>().context("--shots takes shot numbers")?,
                params: list("--params").unwrap_or_else(|| vec!["auto".into()]),
                budget: num("--budget", 24)? as usize,
                scale: value("--scale").map(|v| v.parse::<f32>()).transpose().context("--scale takes a number")?,
                dry_run: flag("--dry-run"),
                seed: num("--seed", 1)?,
                ..Default::default()
            };
            let report = edits_calib::fit::fit_file(std::path::Path::new(p), std::path::Path::new(r), &opts)?;
            if let Some(out) = value("--json") {
                std::fs::write(&out, serde_json::to_string_pretty(&report)?)?;
            }
            print!("{}", edits_calib::fit::summary(&report));
        }
        Some("blur") => {
            // Measured Gaussian blur against the renderer's radius, per picture kind, optionally through H.264.
            let height = num("--height", 320)? as u32;
            let kbps = value("--degrade").map(|v| v.parse::<u32>()).transpose()?;
            let radii = [0.0, 0.3, 0.6, 1.0, 1.5, 2.0, 3.0, 4.0];
            let dir = PathBuf::from(value("--out").unwrap_or_else(|| "calib".into())).join("blurprobe");
            std::fs::create_dir_all(dir.join("img"))?;
            let mut stage = edits_calib::Stage::new()?;
            for kind in edits_calib::pictures::KINDS {
                for seed in 1..=num("--seeds", 3)? {
                    edits_calib::pictures::generate(kind, seed * 7919, height * 5 / 4).save(dir.join("img/p.png"))?;
                    let keys: Vec<serde_json::Value> = radii.iter().enumerate().map(|(i, r)| serde_json::json!({ "t": i as f64 / 25.0, "v": r, "ease": "hold" })).collect();
                    let project = serde_json::json!({
                        "format": "edits.project/1",
                        "settings": { "width": height, "height": height, "fps": 25, "duration": radii.len() as f64 / 25.0, "time_base": "seconds" },
                        "slots": { "a": { "path": "img/p.png" } },
                        "layers": [ { "id": "a", "kind": "image", "slot": "a", "effects": [ { "kind": "gaussian_blur", "params": { "radius": { "keys": keys } } } ] } ]
                    });
                    let p = edits_core::Project::from_json(&project.to_string())?;
                    let mut frames = stage.render(&p, &dir, 1.0, None)?;
                    if let Some(k) = kbps {
                        frames = edits_calib::render::degrade(&frames, &edits_calib::Degrade { kbps: k, scale: 1.0 }, &dir.join("d.mp4"))?;
                    }
                    let lin = |i: usize| edits_analyze::measure::linear_luma_image(frames.rgb(i), frames.w, frames.h);
                    let r0 = lin(0);
                    let got: Vec<String> = (1..radii.len()).map(|i| format!("{:.2}->{:.2}", radii[i], 100.0 * edits_analyze::blur::measure(&lin(i), &r0).sigma_px / frames.h as f64)).collect();
                    println!("{:8} seed {seed}: {}", kind.name(), got.join("  "));
                }
            }
        }
        Some("sig") => {
            // Reference and candidate signatures side by side, shot by shot (the candidate analysed as `compare` does).
            let (Some(r), Some(v)) = (args.get(1), args.get(2)) else { bail!("usage: calib sig <reference_analysis.json> <video>") };
            let reference = edits_analyze::pipeline::load(std::path::Path::new(r))?;
            let opts = edits_analyze::AnalyzeOpts {
                height: reference.source.analysis_height as u32,
                bpm: Some(reference.beats.bpm.value),
                beat_offset: Some(reference.beats.offset_seconds.value),
                samples: false,
                whole: false,
                range: Some((reference.ingest.analysed_f0, reference.ingest.analysed_f1)),
            };
            let cand = edits_analyze::pipeline::analyze_video(std::path::Path::new(v), None, &opts)?;
            if let Some(out) = value("--json") {
                std::fs::write(&out, edits_analyze::jsonfmt::to_string(&cand)?)?;
            }
            let (rs, cs) = (&reference.signature, &cand.signature);
            let ev = |a: &edits_analyze::Analysis, f: usize| -> String {
                a.shots.iter().flat_map(|s| s.events.iter()).filter(|e| e.frame == f).map(|e| match e.kind.as_str() {
                    "flash" => format!("flash {:.1}", e.params["peak"].value),
                    k => format!("{k} {}-{}", e.f0, e.f1),
                }).collect::<Vec<_>>().join(",")
            };
            for sh in &reference.shots {
                let cref = cand.shots.iter().find(|c| c.f0 <= sh.ref_frame && sh.ref_frame < c.f1).map(|c| c.ref_frame);
                println!("shot {} frames {}..{} ref {} | candidate ref {:?}", sh.id, sh.f0, sh.f1, sh.ref_frame, cref);
                for f in sh.f0..sh.f1 {
                    let (i, j) = (f - rs.f0, f - cs.f0);
                    println!("  {f:4} st {:6.2} {:6.2} | bl {:5.2} {:5.2} | sat {:5.2} {:5.2} | sc {:>7} {:>7} | {} | {}", rs.stops[i], cs.stops[j], rs.blur[i], cs.blur[j], rs.sat[i], cs.sat[j],
                        rs.scale[i].map(|v| format!("{v:.3}")).unwrap_or("-".into()), cs.scale[j].map(|v| format!("{v:.3}")).unwrap_or("-".into()), ev(&reference, f), ev(&cand, f));
                }
            }
        }
        _ => bail!("usage: calib run [--cases N] [--seed S] [--height H] [--degrade KBPS] [--no-roundtrip] [--no-audio] [--out DIR] | calib case <case.json>"),
    }
    Ok(())
}
