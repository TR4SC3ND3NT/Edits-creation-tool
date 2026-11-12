//! Check a registration by eye: writes `<out>` with the frame, the reference
//! frame warped onto it by the tracker, and the reference frame itself.
//! `cargo run --example trackview -- video.mp4 <shot_first> <shot_end> <ref> <frame> out.png`
use edits_analyze::{img, measure, media, track};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&a[1]);
    let (f0, f1, r, f): (usize, usize, usize, usize) = (a[2].parse()?, a[3].parse()?, a[4].parse()?, a[5].parse()?);
    let info = media::probe(path)?;
    let frames = media::decode_frames(path, &info, 360)?;
    let usable = track::all_usable(&frames);
    let tr = track::track_shot(&frames, f0, f1, r, &usable);
    let t = &tr[f - f0];
    println!("frame {f}: scale {:.3} rot {:.2} x {:.3} y {:.3} zncc {:.2} {:?}", t.sim.scale(), t.sim.rotation_deg(), t.sim.tx, t.sim.ty, t.zncc, t.method);
    let pair = measure::make_pair(&frames, r, f, Some(&t.sim), &usable);
    let (w, h) = (frames.w, frames.h);
    let mut out = vec![0u8; w * 3 * h * 3];
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                out[(y * w * 3 + x) * 3 + c] = frames.rgb(f)[(y * w + x) * 3 + c];
                out[(y * w * 3 + w + x) * 3 + c] = pair.refw[(y * w + x) * 3 + c];
                out[(y * w * 3 + 2 * w + x) * 3 + c] = frames.rgb(r)[(y * w + x) * 3 + c];
            }
        }
    }
    img::save_rgb(std::path::Path::new(&a[6]), w * 3, h, &out)?;
    Ok(())
}
