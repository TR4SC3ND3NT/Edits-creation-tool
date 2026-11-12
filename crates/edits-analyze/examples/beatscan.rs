//! Print the beat-comb response around a tempo: `cargo run --example beatscan -- video.mp4 118 128 0.25`
use edits_analyze::{beats, media};
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let lo: f64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(60.0);
    let hi: f64 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(200.0);
    let step: f64 = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let info = media::probe(path)?;
    let audio = media::decode_audio(path, &info, 22050)?.ok_or_else(|| anyhow::anyhow!("no audio"))?;
    let onset = beats::onset_envelope(&audio, 22050);
    let t = beats::tempo(&onset, &Default::default());
    println!("{t:?}");
    for (bpm, v, ph) in beats::comb_scan(&onset, lo, hi, step) {
        println!("{bpm:7.2} bpm  comb {v:6.3}  phase {ph:.3} s");
    }
    Ok(())
}
