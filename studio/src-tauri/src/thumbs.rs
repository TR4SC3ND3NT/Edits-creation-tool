//! Preview pictures for the browser of the Edit page: every effect, preset
//! and transition rendered on a tiny sample project by `edits-render`, so a
//! card shows what the item does. They are made lazily (when a card first
//! shows), one at a time on a thread with a renderer of its own (the preview
//! renderer keeps the project's pictures), and kept on disk under the user's
//! cache folder (`$XDG_CACHE_HOME/edits-studio/thumbs/v1/`, else
//! `~/.cache/...`), so they are made once per machine.
//!
//! The sample is two generated pictures (warm circles, cool stripes) at
//! 192 x 108. An effect is put on the picture with its registry defaults;
//! when that changes almost nothing, its first parameter with a range is set
//! to 70 % of the range. A preset is applied at beat 1 and shown a little
//! after; a text preset gets a text layer to work on; a transition is shown
//! half way.

use edits_core::preset::{self, Target, TargetKind};
use edits_core::{effects, Project};
use edits_render::{RenderOptions, Renderer};
use image::RgbaImage;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

const W: u32 = 192;
const H: u32 = 108;
const FPS: f64 = 25.0;
const BPM: f64 = 120.0;

/// Where thumbnails are kept.
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("edits-studio").join("thumbs").join("v1")
}

/// The file of one thumbnail: `effect-glow.jpg`, `preset-flash.jpg`, `transition-fade.jpg`.
pub fn file_name(kind: &str, name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    format!("{kind}-{clean}.jpg")
}

/// Write the two sample pictures (once).
pub fn samples(dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let a = dir.join("sample-a.jpg");
    let b = dir.join("sample-b.jpg");
    let (w, h) = (384u32, 216u32);
    if !a.is_file() {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let mut c = [0.95 - 0.35 * fy, 0.45 + 0.25 * fx, 0.25 + 0.2 * fy];
            for (cx, cy, r, col) in [
                (0.32, 0.55, 0.22, [1.0, 0.85, 0.35]),
                (0.7, 0.38, 0.14, [0.2, 0.25, 0.45]),
                (0.82, 0.8, 0.09, [0.95, 0.95, 0.95]),
            ] {
                let d = ((fx - cx) * 16.0 / 9.0).hypot(fy - cy);
                if d < r {
                    c = col;
                }
            }
            image::Rgb(c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8))
        });
        img.save(&a).map_err(|e| e.to_string())?;
    }
    if !b.is_file() {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let stripe = ((fx * 8.0 + fy * 3.0).floor() as i32) % 2 == 0;
            let c = if stripe {
                [0.15 + 0.2 * fy, 0.45 + 0.3 * fx, 0.75]
            } else {
                [0.05, 0.2 + 0.2 * fx, 0.4 + 0.2 * fy]
            };
            image::Rgb(c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8))
        });
        img.save(&b).map_err(|e| e.to_string())?;
    }
    Ok((a, b))
}

fn base_doc(dir: &Path) -> Value {
    json!({
        "format": "edits.project/1",
        "settings": { "width": W, "height": H, "fps": FPS, "duration": 4, "time_base": "beats", "bpm": BPM },
        "slots": { "a": { "path": dir.join("sample-a.jpg").display().to_string() },
                   "b": { "path": dir.join("sample-b.jpg").display().to_string() } },
        "layers": [ { "id": "pic", "kind": "image", "slot": "a", "in": 0, "out": 4 } ]
    })
}

fn frame_at(beat: f64) -> u64 {
    (beat * 60.0 / BPM * FPS).round() as u64
}

/// The sample project for one item and the frame to show.
pub fn sample(kind: &str, name: &str, dir: &Path, boost: bool) -> Result<(Value, u64), String> {
    let mut doc = base_doc(dir);
    match kind {
        "effect" => {
            let spec = effects::spec(name).ok_or_else(|| format!("unknown effect {name}"))?;
            if !matches!(spec.source, effects::SourceKind::None | effects::SourceKind::OptionalSlot) {
                return Err(format!("{name} needs a file and has no preview"));
            }
            let mut params = Map::new();
            if boost {
                if let Some(p) = spec.params.iter().find(|p| {
                    p.range.is_some_and(|(a, b)| a.is_finite() && b.is_finite()) && p.default.len() == 1
                }) {
                    let (a, b) = p.range.expect("filtered");
                    params.insert(p.name.into(), json!(a + 0.7 * (b - a)));
                }
            }
            doc["layers"][0]["effects"] = json!([ { "kind": name, "params": params } ]);
            // A little motion for the effects that read it (echoes, motion blur).
            doc["layers"][0]["transform"] = json!({ "position": { "keys": [ { "t": 0, "v": [-0.1, 0, 0] }, { "t": 4, "v": [0.1, 0, 0] } ] } });
            Ok((doc, frame_at(1.0)))
        }
        "transition" => {
            let spec = effects::TRANSITIONS
                .iter()
                .find(|t| t.kind == name)
                .ok_or_else(|| format!("unknown transition {name}"))?;
            let mut t = json!({ "kind": name, "duration": 1 });
            if matches!(spec.source, effects::SourceKind::OptionalSlot) {
                t["source"] = json!("slot:b");
            }
            doc["layers"]
                .as_array_mut()
                .expect("layers")
                .push(json!({ "id": "next", "kind": "image", "slot": "b", "in": 1.5, "out": 4, "transition": t }));
            Ok((doc, frame_at(2.0)))
        }
        "preset" => {
            let p = preset::find(name, None)?;
            let text = p.tags.iter().any(|t| t == "text");
            if text {
                doc["layers"]
                    .as_array_mut()
                    .expect("layers")
                    .push(json!({ "id": "title", "kind": "text", "text": "TEXT", "size": 34, "weight": 800, "in": 0, "out": 4 }));
            }
            let target = if p.target == TargetKind::Project {
                Target::None
            } else if text {
                Target::Layer("title".into())
            } else {
                Target::Layer("pic".into())
            };
            let tc = edits_core::time::TimeCtx::from_doc(&doc);
            let (out, _) = preset::apply(&doc, &p, &target, &[1.0], &Map::new(), &tc)?;
            let later = if text || p.tags.iter().any(|t| t == "intro" || t == "transition") { 1.5 } else { 1.12 };
            let frame = if p.target == TargetKind::Project { frame_at(1.6) } else { frame_at(later) };
            Ok((out, frame))
        }
        other => Err(format!("no thumbnails of {other}")),
    }
}

fn mean_diff(a: &RgbaImage, b: &RgbaImage) -> f64 {
    if a.dimensions() != b.dimensions() {
        return 255.0;
    }
    let sum: u64 = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as u64)
        .sum();
    sum as f64 / a.as_raw().len().max(1) as f64
}

struct Job {
    kind: String,
    name: String,
    reply: mpsc::Sender<Result<String, String>>,
}

/// The thumbnail thread.
pub struct Thumbs {
    tx: Mutex<Option<mpsc::Sender<Job>>>,
    dir: PathBuf,
}

impl Thumbs {
    pub fn new(dir: PathBuf) -> Arc<Self> {
        Arc::new(Thumbs { tx: Mutex::new(None), dir })
    }

    fn start(&self) -> mpsc::Sender<Job> {
        let mut g = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = g.as_ref() {
            return tx.clone();
        }
        let (tx, rx) = mpsc::channel::<Job>();
        let dir = self.dir.clone();
        let _ = std::thread::Builder::new()
            .name("edits-thumbs".into())
            .spawn(move || {
                let mut renderer: Option<Result<Renderer, String>> = None;
                let mut plain: Option<RgbaImage> = None;
                for job in rx {
                    let out = dir.join(file_name(&job.kind, &job.name));
                    if out.is_file() {
                        let _ = job.reply.send(Ok(out.display().to_string()));
                        continue;
                    }
                    let r = renderer.get_or_insert_with(|| {
                        Renderer::new(RenderOptions::default()).map_err(|e| format!("{e:#}"))
                    });
                    let result = match r {
                        Err(e) => Err(format!("no renderer: {e}")),
                        Ok(r) => make(r, &dir, &job.kind, &job.name, &mut plain, &out),
                    };
                    let _ = job.reply.send(result.map(|_| out.display().to_string()));
                }
            });
        *g = Some(tx.clone());
        tx
    }

    /// The thumbnail's file, made now if it is not there yet (blocks until it is).
    pub fn get(&self, kind: &str, name: &str) -> Result<String, String> {
        let out = self.dir.join(file_name(kind, name));
        if out.is_file() {
            return Ok(out.display().to_string());
        }
        let (reply, rx) = mpsc::channel();
        self.start()
            .send(Job { kind: kind.into(), name: name.into(), reply })
            .map_err(|_| "the thumbnail thread stopped".to_string())?;
        rx.recv().map_err(|_| "the thumbnail thread stopped".to_string())?
    }
}

fn render(r: &mut Renderer, doc: &Value, frame: u64) -> Result<RgbaImage, String> {
    let p = Project::from_json(&doc.to_string()).map_err(|e| format!("{e:#}"))?;
    r.set_motion_blur_samples(Some(1));
    r.prepare(&p, Path::new("/")).map_err(|e| format!("{e:#}"))?;
    r.render_frame(frame, 1.0).map_err(|e| format!("{e:#}"))
}

fn make(
    r: &mut Renderer,
    dir: &Path,
    kind: &str,
    name: &str,
    plain: &mut Option<RgbaImage>,
    out: &Path,
) -> Result<(), String> {
    samples(dir)?;
    let (doc, frame) = sample(kind, name, dir, false)?;
    let mut img = render(r, &doc, frame)?;
    if kind == "effect" {
        if plain.is_none() {
            *plain = Some(render(r, &base_doc(dir), frame)?);
        }
        if plain.as_ref().is_some_and(|p| mean_diff(p, &img) < 2.0) {
            if let Ok((doc, frame)) = sample(kind, name, dir, true) {
                if let Ok(boosted) = render(r, &doc, frame) {
                    img = boosted;
                }
            }
        }
    }
    let bytes = crate::render::encode_jpeg(&img)?;
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("edits-thumbs-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn every_item_has_a_sample_project_that_checks() {
        let d = dir("samples");
        samples(&d).unwrap();
        let mut items: Vec<(&str, String)> = effects::EFFECTS
            .iter()
            .filter(|e| matches!(e.source, effects::SourceKind::None | effects::SourceKind::OptionalSlot))
            .map(|e| ("effect", e.kind.to_string()))
            .collect();
        items.extend(effects::TRANSITIONS.iter().map(|t| ("transition", t.kind.to_string())));
        items.extend(
            preset::builtin()
                .into_iter()
                .filter(|p| !p.name.starts_with("transition:"))
                .map(|p| ("preset", p.name)),
        );
        for (kind, name) in items {
            let (doc, frame) = match sample(kind, &name, &d, false) {
                Ok(x) => x,
                // A preset that needs a variable (a texture slot) has no preview.
                Err(e) if kind == "preset" => {
                    eprintln!("{name}: {e}");
                    continue;
                }
                Err(e) => panic!("{kind} {name}: {e}"),
            };
            let p = Project::from_json(&doc.to_string()).unwrap_or_else(|e| panic!("{kind} {name}: {e}"));
            assert!(frame < p.frame_count(), "{kind} {name}");
            let errors: Vec<_> = edits_core::check::check(&p, Some(&d))
                .into_iter()
                .filter(|f| f.severity == edits_core::check::Severity::Error)
                .collect();
            assert!(errors.is_empty(), "{kind} {name}: {errors:?}");
        }
        assert_eq!(file_name("preset", "transition:fade"), "preset-transition_fade.jpg");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_thumbnail_is_rendered_once_and_kept() {
        let d = dir("render");
        let t = Thumbs::new(d.clone());
        match t.get("effect", "invert") {
            Ok(path) => {
                let img = image::open(&path).unwrap();
                assert_eq!((img.width(), img.height()), (W, H));
                assert_eq!(t.get("effect", "invert").unwrap(), path);
                assert!(t.get("effect", "no_such_effect").is_err());
            }
            Err(e) if e.contains("no renderer") => eprintln!("skipped: {e}"),
            Err(e) => panic!("{e}"),
        }
        std::fs::remove_dir_all(&d).ok();
    }
}
