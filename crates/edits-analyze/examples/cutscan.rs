//! Print the frame-to-frame similarity used for cut detection:
//! `cargo run --example cutscan -- video.mp4 [first] [last]`
use edits_analyze::{media, shots, signals};
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let info = media::probe(path)?;
    let frames = media::decode_frames(path, &info, 360)?;
    let a: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
    let b: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(frames.len());
    let sigs = signals::signatures(&frames);
    let thumbs = shots::prepare(&signals::thumbnails(&frames, shots::THUMB));
    let sims = shots::pair_sims(&thumbs);
    let lum: Vec<f32> = sigs.iter().map(|s| s.lum).collect();
    let cuts = shots::detect_cuts(&thumbs, &sims, &lum, a, b.min(frames.len()));
    for i in a..b.min(frames.len()) {
        let skip = if i >= 2 { shots::similarity(&thumbs[i - 2], &thumbs[i]).sim } else { f32::NAN };
        let cut = cuts.iter().find(|c| c.frame == i).map(|c| format!("CUT {:.2}", c.strength)).unwrap_or_default();
        println!("{i:4} lum {:5.1} sat {:5.1} sharp {:.3} diff {:5.1} sim {:5.2} signed {:5.2} skip1 {:5.2} {cut}", sigs[i].lum, sigs[i].sat, sigs[i].sharp, sigs[i].diff, sims[i].sim, sims[i].signed, skip);
    }
    Ok(())
}
