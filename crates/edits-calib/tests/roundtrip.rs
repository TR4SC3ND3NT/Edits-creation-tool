//! The round-trip invariant as a fast regression test: analysing a render of
//! a recipe returns that recipe within stated tolerances, and a project
//! drafted from that analysis, rendered and analysed, reproduces the
//! analysis. A handful of generated cases at a small size; the tolerances
//! are those of `report::TOLERANCES`, the required shares are a little below
//! what `edits calib` measures over 36 cases at 320 px (see
//! `docs/CALIBRATION.md`), so a regression in any estimator or in the draft
//! mapping fails here.
//!
//! Needs a wgpu adapter (Mesa lavapipe is enough); skips with a note without one.

use edits_calib::harness::run_case;
use edits_calib::recipe::CaseSpec;
use edits_calib::render::Stage;
use edits_calib::report::{aggregate, EventRow, Row};

fn rows() -> Option<(Vec<Row>, Vec<EventRow>)> {
    let mut stage = match Stage::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: no renderer ({e:#})");
            return None;
        }
    };
    let mut all = Vec::new();
    // Seeds chosen to cover flashes (single and double, clipped), saturation,
    // invert and threshold hits, blur under a flash, punches, slides and an overlay.
    for seed in [3u64, 20, 25] {
        let dir = std::env::temp_dir().join(format!("edits-calib-roundtrip-{seed}-{}", std::process::id()));
        let o = run_case(&mut stage, &CaseSpec { seed, height: 240, audio: true }, &dir, None, true).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        all.extend(o.results);
    }
    Some(aggregate(&all))
}

fn row<'a>(rows: &'a [Row], est: &str) -> &'a Row {
    rows.iter().find(|r| r.est == est && r.kind == "all").unwrap_or_else(|| panic!("no samples for {est}"))
}

#[test]
fn analysis_and_draft_reproduce_the_recipe() {
    let Some((rows, events)) = rows() else { return };
    for r in rows.iter().filter(|r| r.kind == "all") {
        eprintln!("{:<22} n {:4} bias {:+.3} spread {:.3} within {:.0}%", r.est, r.n, r.bias, r.spread, 100.0 * r.within);
    }
    for e in events.iter().filter(|e| e.kind == "all") {
        eprintln!("{:<22} found {} missed {} extra {}", e.est, e.hits, e.misses, e.extras);
    }
    // (estimator, share of samples within its tolerance, largest |bias|)
    let required: &[(&str, f64, f64)] = &[
        ("cut.frame", 1.0, 0.1),
        ("flash.frame", 1.0, 0.1),
        ("flash.peak", 0.9, 0.15),
        ("flash.rate", 0.85, 0.04),
        ("exposure.curve", 0.9, 0.1),
        ("exposure.rest", 0.95, 0.05),
        ("saturation.amount", 1.0, 0.05),
        ("saturation.rest", 0.95, 0.03),
        // Seed 20 blurs a smooth (photo) picture under a 2-stop flash that
        // clips most of it: the remaining unclipped tones and the clip
        // boundaries hide the blur, and it reads at a tenth of the truth on
        // that frame (a known limit, see docs/CALIBRATION.md). Seeds 3 and 25
        // are within 20%.
        ("blur.radius", 0.55, 0.35),
        ("blur.rest", 0.95, 0.03),
        ("scale", 0.95, 0.003),
        ("draft.cuts", 0.9, 0.1),
        ("draft.flashes", 0.9, 0.1),
        ("draft.exposure", 0.9, 0.1),
        ("draft.saturation", 0.6, 0.15),
        ("draft.scale", 0.9, 0.1),
        ("draft.overall", 0.6, 0.1),
    ];
    let mut failures = Vec::new();
    for (est, share, bias) in required {
        let r = row(&rows, est);
        if r.within < *share || r.bias.abs() > *bias {
            failures.push(format!("{est}: {:.0}% within {} (need {:.0}%), bias {:+.3} (limit {bias})", 100.0 * r.within, r.tol, 100.0 * share, r.bias));
        }
    }
    for e in events.iter().filter(|e| e.kind == "all" && matches!(e.est.as_str(), "cut" | "flash")) {
        if e.recall < 0.95 || e.precision < 0.95 {
            failures.push(format!("{}: recall {:.2}, precision {:.2}", e.est, e.recall, e.precision));
        }
    }
    assert!(failures.is_empty(), "round trip out of tolerance:\n{}", failures.join("\n"));
}
