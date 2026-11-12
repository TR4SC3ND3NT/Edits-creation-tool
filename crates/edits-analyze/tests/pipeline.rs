//! End to end on a synthetic edit with a known recipe: analyse, draft a
//! project, check it, and compare renders in signature space. Everything is
//! generated in code; no footage is involved.

use edits_analyze::compare::compare_analyses;
use edits_analyze::draft::{draft, DraftOpts};
use edits_analyze::schema::{Analysis, Ident, Source};
use edits_analyze::synth::Clip;
use edits_analyze::{analyze_frames, AnalyzeOpts};
use edits_core::{LayerKind, Prop, TimeBase};

const FPS: f64 = 25.0;
const BPM: f64 = 125.0; // exactly 12 frames per beat
const LEAD: usize = 6; // black frames before beat 0
const SHOT: usize = 12;
const SHOTS: usize = 6;

/// Clicks on the beat, the first one at the first picture frame.
fn audio() -> Vec<f32> {
    let sr = 22050.0;
    let n = ((LEAD + SHOT * SHOTS) as f64 / FPS * sr) as usize;
    let mut s = vec![0.0f32; n];
    let mut k = 0;
    loop {
        let i0 = ((LEAD as f64 / FPS + k as f64 * 60.0 / BPM) * sr) as usize;
        if i0 + 1500 >= n {
            break;
        }
        for j in 0..1500 {
            s[i0 + j] += 0.8 * (-(j as f32) / 250.0).exp() * (std::f32::consts::TAU * 110.0 * j as f32 / sr as f32).sin();
        }
        k += 1;
    }
    s
}

struct Recipe {
    zoom: f64,
    flash_on_shot_3: bool,
    late_cut: usize,
}

/// Six one-beat shots; the sixth shows the first picture again.
fn clip(r: &Recipe) -> Clip {
    let mut c = Clip::new(128, 128, LEAD + SHOT * SHOTS);
    c.fps = FPS;
    c.noise = 1.0;
    c.overlay = true;
    c.vignette = 0.5;
    for f in 0..LEAD {
        c.frame_mut(f).picture = 0;
    }
    for f in LEAD..LEAD + SHOT * SHOTS {
        let t = f - LEAD;
        // Shot 4 (index 3) can start late.
        let mut shot = t / SHOT;
        if shot == 3 && t % SHOT < r.late_cut {
            shot = 2;
        }
        let k = if shot == 2 && t / SHOT == 3 { (t % SHOT + SHOT) as f64 } else if shot == 3 { (t % SHOT - r.late_cut) as f64 } else { (t % SHOT) as f64 };
        let fr = c.frame_mut(f);
        fr.picture = [1, 2, 3, 4, 5, 1][shot];
        // A flash on every cut, decaying by half per frame.
        if shot != 2 || r.flash_on_shot_3 {
            fr.stops = if k < 6.0 { 3.0 * 0.5f64.powf(k) } else { 0.0 };
        }
        match shot {
            0 => fr.scale = 1.0 + (r.zoom - 1.0) * 0.6f64.powf(k), // punch-in: decay rate 0.4
            1 => fr.tx = -0.1 * 0.6f64.powf(k),                    // slide in from the left
            2 => fr.blur = 1.8 * 0.55f64.powf(k),                  // defocus settling
            3 => fr.saturation = if k < 3.0 { 0.0 } else { 1.0 },  // black-and-white hit
            4 => fr.invert = (5.0..8.0).contains(&k),              // inversion hit
            _ => fr.scale = 1.0 + 0.004 * k,                       // slow push
        }
    }
    c
}

fn analyse(r: &Recipe, opts: &AnalyzeOpts) -> Analysis {
    let frames = clip(r).render();
    let source = Source {
        path: "synthetic.mp4".into(),
        width: 128,
        height: 128,
        fps: FPS,
        frames: frames.len(),
        duration_seconds: frames.len() as f64 / FPS,
        has_audio: true,
        analysis_width: 128,
        analysis_height: 128,
    };
    let a = audio();
    analyze_frames(&frames, Some((&a, 22050)), source, opts).analysis
}

const TRUE: Recipe = Recipe { zoom: 1.3, flash_on_shot_3: true, late_cut: 0 };

#[test]
fn recipe_is_recovered_drafted_and_compared() {
    let a = analyse(&TRUE, &AnalyzeOpts::default());
    let text = edits_analyze::jsonfmt::to_string(&a).unwrap();
    let again: Analysis = serde_json::from_str(&text).unwrap();
    assert_eq!(again.shots.len(), a.shots.len(), "the document round-trips through JSON");

    // ---- ingest and beats --------------------------------------------------
    assert_eq!((a.ingest.content_f0, a.ingest.content_f1), (LEAD, LEAD + SHOT * SHOTS));
    assert_eq!(a.ingest.blank_runs[0].kind, "black");
    assert!(a.ingest.loop_info.is_none());
    assert!((a.beats.bpm.value - BPM).abs() < 0.6, "bpm {}", a.beats.bpm.value);
    assert!((a.beats.offset_seconds.value - LEAD as f64 / FPS).abs() < 0.03, "beat 0 at {}", a.beats.offset_seconds.value);

    // ---- shots on the beat, slots -------------------------------------------
    let cuts: Vec<usize> = a.shots.iter().map(|s| s.f0).collect();
    assert_eq!(cuts, (0..SHOTS).map(|i| LEAD + i * SHOT).collect::<Vec<_>>());
    for s in &a.shots {
        assert_eq!(s.cut.grid, "beat", "shot {}", s.id);
        assert!(s.cut.offset_frames.value.abs() <= 1.0);
        assert_eq!(s.beat0_snapped, Some((s.id - 1) as f64), "shot {}", s.id);
        assert_eq!(s.status, Ident::Measured, "shot {}: {:?}", s.id, s.notes);
        assert!(s.ref_frame >= s.f0 + 5, "shot {}: reference frame {} is in the flash", s.id, s.ref_frame);
    }
    assert_eq!(a.slots.len(), 5, "slots: {:?}", a.slots);
    assert_eq!(a.shots[0].slot, a.shots[5].slot);

    // ---- flashes -----------------------------------------------------------------
    for s in &a.shots {
        let fl: Vec<_> = s.events.iter().filter(|e| e.kind == "flash").collect();
        assert_eq!(fl.len(), 1, "shot {}: {:?}", s.id, s.events);
        assert_eq!(fl[0].frame, s.f0, "shot {}", s.id);
        assert!((fl[0].params["peak"].value - 3.0).abs() < 0.7, "shot {}: peak {}", s.id, fl[0].params["peak"].value);
        assert!((fl[0].params["decay_rate"].value - 0.5).abs() < 0.15, "shot {}: decay {}", s.id, fl[0].params["decay_rate"].value);
        assert!(s.curves.iter().any(|c| c.prop == "exposure.stops"));
    }

    // ---- motion ------------------------------------------------------------------
    let curve = |shot: usize, prop: &str| a.shots[shot].curves.iter().find(|c| c.prop == prop).unwrap_or_else(|| panic!("shot {} has no {prop} curve: {:?}", shot + 1, a.shots[shot].curves.iter().map(|c| &c.prop).collect::<Vec<_>>()));
    let zoom = curve(0, "scale");
    assert_eq!(zoom.identifiability, Ident::RelativeOnly);
    let first = zoom.keys.iter().map(|k| k.v[0]).fold(0.0, f64::max);
    assert!((first - 1.3).abs() < 0.06, "punch-in starts at {first}: {:?}", zoom.keys);
    assert!((zoom.keys.last().unwrap().v[0] - 1.0).abs() < 0.01);
    let slide = curve(1, "position");
    let far = slide.keys.iter().map(|k| k.v[0]).fold(0.0, f64::min);
    assert!((far + 0.1).abs() < 0.03, "slide starts at {far}: {:?}", slide.keys);
    assert!(a.shots[1].curves.iter().all(|c| c.prop != "scale"), "a slide is not a zoom");
    let push = curve(5, "scale");
    assert!(push.keys.len() == 2 && (push.keys[0].v[0] / push.keys[1].v[0] - 1.0 / 1.044).abs() < 0.012, "slow push: {:?}", push.keys);

    // ---- blur and tone hits ----------------------------------------------------------
    let blur = curve(2, "gaussian_blur.radius");
    let peak = blur.keys.iter().map(|k| k.v[0]).fold(0.0, f64::max);
    assert!((peak - 1.8).abs() < 0.6, "blur peak {peak}%: {:?}", blur.keys);
    assert!(blur.keys.last().unwrap().v[0] < 0.2);
    for (i, s) in a.shots.iter().enumerate() {
        if i != 2 {
            assert!(s.curves.iter().all(|c| c.prop != "gaussian_blur.radius" || c.keys.iter().all(|k| k.v[0] < 0.5)), "shot {} should not be blurred", s.id);
        }
    }
    let bw: Vec<_> = a.shots.iter().flat_map(|s| s.events.iter().filter(|e| e.kind == "saturation")).collect();
    assert_eq!(bw.len(), 1, "{bw:?}");
    // The hit starts on the flash peak, where almost nothing coloured is left
    // unclipped to measure: it may be picked up one frame late.
    assert!(bw[0].f0 == LEAD + 3 * SHOT || bw[0].f0 == LEAD + 3 * SHOT + 1, "{:?}", bw[0]);
    assert_eq!(bw[0].f1, LEAD + 3 * SHOT + 3);
    assert!(bw[0].params["amount"].value < 0.1);
    let inv: Vec<_> = a.shots.iter().flat_map(|s| s.events.iter().filter(|e| e.kind == "invert")).collect();
    assert_eq!(inv.len(), 1, "{inv:?}");
    assert_eq!((inv[0].f0, inv[0].f1), (LEAD + 4 * SHOT + 5, LEAD + 4 * SHOT + 8));
    assert!(a.shots.iter().all(|s| s.events.iter().all(|e| e.kind != "threshold" && e.kind != "posterize")));

    // ---- static layers ---------------------------------------------------------------
    let v = a.layers.vignette.as_ref().expect("vignette");
    assert!((v.amount.value - 0.5).abs() < 0.12, "vignette amount {}", v.amount.value);
    assert!(a.layers.overlay.is_some());

    // ---- every estimate says how far to trust it ------------------------------------------
    assert!(text.contains("\"identifiability\": \"relative_only\"") && text.contains("\"identifiability\": \"measured\""));

    // ---- draft -----------------------------------------------------------------------------
    let mut a_with_files = a.clone();
    a_with_files.layers.overlay.as_mut().unwrap().path = Some("overlay.png".into());
    for snap in [true, false] {
        let opts = DraftOpts { proxies: false, width: None, height: None, snap, repeats: true, analysis_dir: "analysis".into(), project_dir: "project".into() };
        let p = draft(&a_with_files, &opts);
        let findings = edits_core::check::check(&p, None);
        assert!(!edits_core::check::has_errors(&findings), "{findings:?}");
        assert_eq!(p.settings.time_base, TimeBase::Beats);
        assert!((p.settings.bpm.unwrap() - BPM).abs() < 0.6);
        assert_eq!(p.audio.as_ref().unwrap().offset, 0.0);
        let shots: Vec<_> = p.layers.iter().filter(|l| matches!(l.kind, LayerKind::Image { .. }) && l.id.starts_with("shot")).collect();
        assert_eq!(shots.len(), SHOTS);
        assert_eq!(p.slots.len(), 6, "five pictures and the overlay");
        assert!(p.slots.iter().filter(|(k, _)| k.as_str() != "overlay").all(|(_, s)| s.path.is_none()));
        assert_eq!(p.slots["overlay"].path.as_deref(), Some("../analysis/overlay.png"));
        assert!(p.layers.iter().any(|l| l.id == "overlay" && l.blend == edits_core::Blend::Screen));
        assert!(p.layers.iter().any(|l| l.id == "finish" && l.effects[0].kind == "vignette"));
        if snap {
            // Cuts sit on whole beats.
            for (i, l) in shots.iter().enumerate() {
                assert_eq!(l.start, i as f64, "{}", l.id);
                assert_eq!(l.end, Some((i + 1) as f64), "{}", l.id);
            }
        }
        // Every shot starts on the frame where the reference shows it.
        for (i, l) in shots.iter().enumerate() {
            let first = (0..p.frame_count()).find(|f| p.frame_time(*f) >= l.start - 1e-9).unwrap();
            assert_eq!(first as usize, LEAD + i * SHOT, "{} (snap {snap})", l.id);
        }
        let s1 = shots[0];
        assert!(matches!(&s1.transform.scale, Prop::Anim(a) if a.keys.len() >= 2), "shot 1 scale is keyed");
        assert!(s1.effects.iter().any(|e| e.kind == "exposure"));
        assert!(shots[2].effects.iter().any(|e| e.kind == "gaussian_blur"));
        assert!(shots[3].effects.iter().any(|e| e.kind == "saturation"));
        assert!(shots[4].effects.iter().any(|e| e.kind == "invert"));
        // The project survives its own JSON.
        let back = edits_core::Project::from_json(&p.to_json()).unwrap();
        assert_eq!(back.layers.len(), p.layers.len());
    }

    // ---- compare: the same recipe scores as the same -----------------------------------------
    let grid = AnalyzeOpts { bpm: Some(a.beats.bpm.value), beat_offset: Some(a.beats.offset_seconds.value), range: Some((a.ingest.analysed_f0, a.ingest.analysed_f1)), ..Default::default() };
    let same = compare_analyses(&a, &analyse(&TRUE, &grid), None);
    assert!(same.overall > 0.97, "{:?}", same.scores);
    assert!(same.differences.is_empty(), "{:?}", same.differences);
    assert_eq!(same.scores["tempo"], None);

    // ---- compare: a different recipe is told apart, in words ------------------------------------
    let other = compare_analyses(&a, &analyse(&Recipe { zoom: 1.12, flash_on_shot_3: false, late_cut: 3 }, &grid), None);
    let texts: Vec<&str> = other.differences.iter().map(|d| d.text.as_str()).collect();
    assert!(other.overall < same.overall - 0.05, "{:?}", other.scores);
    assert_eq!(other.cuts.missing, vec![LEAD + 3 * SHOT], "{:?}", other.cuts);
    assert_eq!(other.cuts.extra, vec![LEAD + 3 * SHOT + 3]);
    assert!(other.flashes.recall < 1.0 && other.flashes.missing.contains(&(LEAD + 2 * SHOT)), "{:?}", other.flashes);
    let zoom_line = texts.iter().find(|t| t.starts_with("shot 1 ") && t.contains("zoom")).unwrap_or_else(|| panic!("no zoom difference for shot 1 in {texts:#?}"));
    assert!(zoom_line.contains("reference 1.3") || zoom_line.contains("reference 1.2"), "{zoom_line}");
    assert!(other.scores["scale"].unwrap() < same.scores["scale"].unwrap() - 0.05);
    assert!(texts.iter().any(|t| t.contains("cut at beat 3") && t.contains("no match")), "{texts:#?}");
    let report = edits_analyze::compare::report(&other, 10);
    assert!(report.contains("largest differences"));
}

#[test]
fn a_looping_edit_is_analysed_once() {
    // Two passes of a four-shot loop, then a fade from white to black.
    let period = 48;
    let mut c = Clip::new(96, 96, 4 + 2 * period + 8);
    for f in 0..4 {
        c.frame_mut(f).picture = 0;
    }
    for t in 0..2 * period {
        let fr = c.frame_mut(4 + t);
        let k = (t % 12) as f64;
        fr.picture = [1, 2, 3, 4][(t % period) / 12];
        fr.scale = 1.0 + 0.2 * 0.6f64.powf(k);
        fr.stops = if k < 5.0 { 2.5 * 0.5f64.powf(k) } else { 0.0 };
    }
    let mut frames = c.render();
    // Tail: solid frames ramping from white to black.
    let n = frames.w * frames.h * 3;
    for i in 0..8 {
        let v = (255.0 * (1.0 - i as f64 / 7.0)) as u8;
        let at = (4 + 2 * period + i) * n;
        frames.data[at..at + n].fill(v);
    }
    let source = Source { path: "loop.mp4".into(), width: 96, height: 96, fps: 25.0, frames: frames.len(), duration_seconds: frames.len() as f64 / 25.0, has_audio: false, analysis_width: 96, analysis_height: 96 };
    let a = analyze_frames(&frames, None, source, &AnalyzeOpts::default()).analysis;
    let l = a.ingest.loop_info.as_ref().expect("loop");
    assert_eq!((l.first_frame, l.period_frames.value as usize), (4, period));
    assert!((l.repeats - 2.0).abs() < 0.05);
    assert_eq!((a.ingest.analysed_f0, a.ingest.analysed_f1), (4, 4 + period));
    assert_eq!(a.shots.len(), 4);
    let tail = a.ingest.blank_runs.last().unwrap();
    assert_eq!((tail.f0, tail.kind.as_str()), (4 + 2 * period, "fade"));
    // No audio: the tempo comes from the picture (12 frames per beat = 125 bpm).
    assert_eq!(a.beats.source, "visual");
    assert!((a.beats.bpm.value - 125.0).abs() < 4.0, "visual tempo {}", a.beats.bpm.value);
    // The draft plays both passes and the tail.
    let opts = DraftOpts { proxies: false, width: Some(540), height: None, snap: true, repeats: true, analysis_dir: ".".into(), project_dir: ".".into() };
    let p = draft(&a, &opts);
    assert!(!edits_core::check::has_errors(&edits_core::check::check(&p, None)));
    assert_eq!(p.layers.iter().filter(|l| l.id.starts_with("shot")).count(), 8);
    assert!(p.layers.iter().any(|l| l.id == "shot01_r2"));
    assert!(p.layers.iter().any(|l| matches!(l.kind, LayerKind::Solid { .. })));
    assert_eq!((p.settings.width, p.settings.height), (540, 540));
    assert!(p.audio.is_none());
}
