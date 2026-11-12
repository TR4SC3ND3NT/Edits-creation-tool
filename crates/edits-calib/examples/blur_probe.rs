//! Temporary: a static picture with a blur ramp, measured frame by frame.
use edits_analyze::blur;
use edits_analyze::measure::linear_luma_image;
use edits_calib::pictures::{self, KINDS};
use edits_calib::Stage;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(&a[1]);
    let h: u32 = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(360);
    let extra = a.get(3).cloned().unwrap_or_default();
    std::fs::create_dir_all(dir.join("img"))?;
    let mut stage = Stage::new()?;
    for kind in KINDS {
        pictures::generate(kind, 7, h * 5 / 4).save(dir.join("img/p.png"))?;
        pictures::overlay(3, h, h).save(dir.join("img/o.png"))?;
        let overlay = if extra.contains("overlay") { r#",{"id":"o","kind":"image","slot":"o","blend":"screen"}"# } else { "" };
        let vig = if extra.contains("vignette") { r#",{"id":"v","kind":"adjustment","effects":[{"kind":"vignette","params":{"amount":0.6,"radius":0.5,"softness":0.4}}]}"# } else { "" };
        let flash = if extra.contains("flash") { r#",{"kind":"exposure","params":{"stops":{"keys":[{"t":0,"v":3,"ease":"linear"},{"t":12,"v":0}]}}}"# } else { "" };
        let text = format!(
            r#"{{"format":"edits.project/1","settings":{{"width":{h},"height":{h},"fps":25,"duration":14,"time_base":"seconds"}},
            "slots":{{"p":{{"path":"img/p.png"}},"o":{{"path":"img/o.png","fit":"stretch"}}}},
            "layers":[{{"id":"a","kind":"image","slot":"p","transform":{{"scale":1.1}},"effects":[{{"kind":"gaussian_blur","params":{{"radius":{{"keys":[{{"t":0,"v":3,"ease":"linear"}},{{"t":12,"v":0}}]}}}}}}{flash}]}}{overlay}{vig}]}}"#
        );
        let mut project = edits_core::Project::from_json(&text)?;
        project.settings.fps = 1.0;
        let frames = stage.render(&project, &dir, 1.0, None)?;
        let mut usable = vec![true; frames.w * frames.h];
        if extra.contains("mask") {
            let plate = image::open(dir.join("img/o.png"))?.to_rgb8();
            let p = edits_analyze::layers::Plate { w: frames.w, h: frames.h, rgb: plate.into_raw() };
            usable = edits_analyze::layers::usable_mask(&p);
        }
        let flat = edits_analyze::measure::FlatField::flat(frames.w, frames.h);
        let _ = (blur::window(8, 8), linear_luma_image(frames.rgb(0), frames.w, frames.h));
        print!("{:9}", kind.name());
        for f in 0..13 {
            let truth = 3.0 * (1.0 - f as f64 / 12.0);
            let m = edits_analyze::pipeline::measure_frame(&frames, 13, f, Some(&edits_analyze::img::Sim::IDENTITY), true, false, &usable, &flat);
            print!(" {:.2}/{:.2} {:+.2}", 100.0 * m.blur.sigma_px / frames.h as f64, truth, m.exp.stops);
        }
        println!();
    }
    Ok(())
}
