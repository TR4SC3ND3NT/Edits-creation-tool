//! Run the calibration harness from the command line.
//!
//! ```text
//! cargo run --release -p edits-calib --example calib -- run  [--cases 36] [--seed 1] [--height 320] [--degrade 1500] [--no-roundtrip] [--no-audio] [--out DIR]
//! cargo run --release -p edits-calib --example calib -- case <case.json> [--out DIR]
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
        _ => bail!("usage: calib run [--cases N] [--seed S] [--height H] [--degrade KBPS] [--no-roundtrip] [--no-audio] [--out DIR] | calib case <case.json>"),
    }
    Ok(())
}
