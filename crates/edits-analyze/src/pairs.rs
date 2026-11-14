//! Side-by-side sheets: the reference and a render at the same instants, in
//! pairs, so an agent can judge by eye what signatures cannot say (the kind
//! of effect, the direction of a move, the framing, an overlay), whatever
//! the pictures are.
//!
//! Per shot: the cut, the strongest flash, the reference frame and the last
//! frame; each as a pair (reference left with a blue bar, render right with
//! an orange one), labelled with shot, frame, beat and what the instant is.
//! Shots are shown worst-scoring first when a comparison is given.

use crate::compare::Comparison;
use crate::contact::{self, Cell};
use crate::media;
use crate::schema::*;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct SheetOpts {
    /// Only these reference shots (default: all).
    pub shots: Vec<usize>,
    /// Pairs per page.
    pub per_page: usize,
    /// Height of a picture in pixels.
    pub cell_height: u32,
    /// The reference video (default: the analysis's `source.path`).
    pub reference_video: Option<PathBuf>,
}

impl Default for SheetOpts {
    fn default() -> Self {
        Self { shots: vec![], per_page: 16, cell_height: 150, reference_video: None }
    }
}

/// One reference / render pair on a sheet.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Pair {
    pub page: usize,
    pub shot: usize,
    pub frame: usize,
    pub beat: f64,
    pub what: String,
}

/// The instants of one shot worth seeing side by side.
fn instants(s: &Shot) -> Vec<(usize, &'static str)> {
    let mut out: Vec<(usize, &'static str)> = vec![(s.f0, "cut")];
    if let Some(e) = s.events.iter().filter(|e| e.kind == "flash").max_by(|a, b| a.params["peak"].value.total_cmp(&b.params["peak"].value)) {
        out.push((e.frame, "flash"));
    }
    if let Some(e) = s.events.iter().find(|e| !matches!(e.kind.as_str(), "flash" | "fade")) {
        out.push((e.f0, "tone"));
    }
    out.push((s.ref_frame, "ref"));
    out.push((s.f1 - 1, "last"));
    let mut seen = BTreeSet::new();
    out.retain(|(f, _)| seen.insert(*f));
    out.sort_by_key(|t| t.0);
    out
}

/// Write the sheets (`out` itself for one page, `<stem>_2.<ext>`... for more)
/// and return their paths and the pairs on them.
pub fn write(reference: &Analysis, candidate: &Path, cmp: Option<&Comparison>, out: &Path, opts: &SheetOpts) -> Result<(Vec<PathBuf>, Vec<Pair>)> {
    let ref_video = opts.reference_video.clone().unwrap_or_else(|| PathBuf::from(&reference.source.path));
    let ri = media::probe(&ref_video).with_context(|| format!("the reference video {} (pass the reference video if it moved)", ref_video.display()))?;
    let ci = media::probe(candidate)?;
    let ratio = ci.fps / reference.source.fps;
    let mut shots: Vec<&Shot> = reference.shots.iter().filter(|s| opts.shots.is_empty() || opts.shots.contains(&s.id)).collect();
    // Worst first: the mean curve distance of each shot.
    let badness = |id: usize| -> f64 {
        cmp.and_then(|c| c.shots.iter().find(|s| s.shot == id)).map(|s| {
            let d: Vec<f64> = s.curves.values().map(|c| c.distance.min(3.0)).collect();
            if d.is_empty() { 0.0 } else { d.iter().sum::<f64>() / d.len() as f64 }
        }).unwrap_or(0.0)
    };
    if cmp.is_some() {
        shots.sort_by(|a, b| badness(b.id).total_cmp(&badness(a.id)));
    }
    let mut pairs: Vec<Pair> = Vec::new();
    for s in &shots {
        for (f, what) in instants(s) {
            pairs.push(Pair { page: 0, shot: s.id, frame: f, beat: round_to(reference.beats.beat_of_frame(f as f64, reference.source.fps), 2), what: what.into() });
        }
    }
    let per_page = opts.per_page.max(1);
    for (i, p) in pairs.iter_mut().enumerate() {
        p.page = i / per_page + 1;
    }
    let ref_frames: Vec<usize> = pairs.iter().map(|p| p.frame).collect::<BTreeSet<_>>().into_iter().collect();
    let cand_frames: Vec<usize> = pairs.iter().map(|p| (p.frame as f64 * ratio).round() as usize).filter(|f| (*f as u64) < ci.frames.max(1)).collect::<BTreeSet<_>>().into_iter().collect();
    let h = opts.cell_height.clamp(48, 540);
    let rf = media::decode_selected_at(&ref_video, &ri, &ref_frames, Some(h.min(ri.height)))?;
    let cf = media::decode_selected_at(candidate, &ci, &cand_frames, Some(h.min(ci.height)))?;
    let pages = pairs.last().map(|p| p.page).unwrap_or(0);
    let mut paths = Vec::new();
    for page in 1..=pages {
        let mut cells = Vec::new();
        for p in pairs.iter().filter(|p| p.page == page) {
            let score = cmp.map(|_| format!(" D{:.2}", badness(p.shot))).unwrap_or_default();
            let left = ref_frames.binary_search(&p.frame).ok().filter(|i| *i < rf.len()).map(|i| contact::fit_height(&contact::frame_image(&rf, i), h));
            let cfr = (p.frame as f64 * ratio).round() as usize;
            let right = cand_frames.binary_search(&cfr).ok().filter(|i| *i < cf.len()).map(|i| contact::fit_height(&contact::frame_image(&cf, i), h));
            let blank = || image::RgbImage::from_pixel(h, h, image::Rgb([0, 0, 0]));
            cells.push(Cell { image: left.unwrap_or_else(blank), label: format!("S{} F{} {}{score}", p.shot, p.frame, p.what.to_uppercase()), accent: Some([60, 150, 230]) });
            cells.push(Cell { image: right.unwrap_or_else(blank), label: format!("B{:.2} OURS", p.beat), accent: Some([235, 150, 40]) });
        }
        let title = format!(
            "LEFT REFERENCE (BLUE) / RIGHT RENDER (ORANGE){}  PAGE {page}/{pages}",
            cmp.map(|c| format!("  OVERALL {:.2}", c.overall)).unwrap_or_default()
        );
        let sheet = contact::compose(&cells, 8, 4, Some(&title));
        let path = if pages == 1 || page == 1 {
            out.to_path_buf()
        } else {
            let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("sheet");
            let ext = out.extension().and_then(|s| s.to_str()).unwrap_or("jpg");
            out.with_file_name(format!("{stem}_{page}.{ext}"))
        };
        contact::save(&sheet, &path)?;
        paths.push(path);
    }
    Ok((paths, pairs))
}

/// What to ask of each pair, in the fixed vocabulary of the vision pass.
pub const QUESTIONS: &str = "For each pair (reference left, render right), ignoring that the pictures differ: \
1. Same kind of picture and framing (full frame, inset card, tilted card, cube, picture in picture)? \
2. Same light: is a flash, burn to white or glow on the same instant in both, about as strong? \
3. Same tone: inverted, threshold, posterized, duotone or black and white in one and not the other? \
4. Same motion: punch, push, slide, shake, 3D turn in the same direction and at the same moment? \
5. Same blur kind: gaussian, directional (same direction), zoom, spin? \
6. Same overlays and finish: texture, handwriting, vignette as dark, speed lines? \
Answer per shot with the vocabulary words and the change to make (layer, effect, value).";
