//! Render a project in-process into analysis frames, and pass frames through
//! a platform-like degradation (H.264 at a low bitrate, optional downscale).

use anyhow::{anyhow, bail, Context, Result};
use edits_analyze::img::Frames;
use edits_core::Project;
use edits_render::{RenderOptions, Renderer};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// One renderer, reused for every project of a run.
pub struct Stage {
    pub renderer: Renderer,
    /// Frames rendered so far.
    pub rendered: u64,
    pub render_seconds: f64,
}

impl Stage {
    pub fn new() -> Result<Self> {
        let mut renderer = Renderer::new(RenderOptions::default()).context("cannot start the renderer (run `edits doctor`)")?;
        renderer.set_motion_blur_samples(Some(1));
        Ok(Stage { renderer, rendered: 0, render_seconds: 0.0 })
    }

    /// Render frames `range` (default: all) of `project` at `scale`, one motion blur sample.
    pub fn render(&mut self, project: &Project, dir: &Path, scale: f32, range: Option<(usize, usize)>) -> Result<Frames> {
        let started = std::time::Instant::now();
        self.renderer.prepare(project, dir)?;
        let (w, h) = self.renderer.output_size(scale)?;
        let total = project.frame_count() as usize;
        let (f0, f1) = range.map(|(a, b)| (a.min(total), b.min(total))).unwrap_or((0, total));
        let mut frames = Frames::new(w as usize, h as usize, project.settings.fps);
        frames.data.reserve((f1 - f0) * (w * h * 3) as usize);
        for f in f0..f1 {
            let img = self.renderer.render_frame(f as u64, scale).with_context(|| format!("rendering frame {f}"))?;
            for p in img.as_raw().chunks_exact(4) {
                frames.data.extend_from_slice(&p[..3]);
            }
        }
        self.rendered += (f1 - f0) as u64;
        self.render_seconds += started.elapsed().as_secs_f64();
        Ok(frames)
    }
}

/// What a sharing platform does to an upload.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Degrade {
    /// Video bitrate in kbit/s.
    pub kbps: u32,
    /// Resize factor before encoding (1 = none); frames come back at their original size.
    pub scale: f64,
}

impl Default for Degrade {
    fn default() -> Self {
        Degrade { kbps: 1500, scale: 1.0 }
    }
}

impl Degrade {
    pub fn label(&self) -> String {
        if (self.scale - 1.0).abs() < 1e-9 {
            format!("h264_{}k", self.kbps)
        } else {
            format!("h264_{}k_x{:.2}", self.kbps, self.scale)
        }
    }
}

fn ffmpeg() -> String {
    std::env::var("EDITS_FFMPEG").unwrap_or_else(|_| "ffmpeg".into())
}

/// Encode with libx264 at the given bitrate and decode again. `tmp` is a file path to use.
pub fn degrade(frames: &Frames, d: &Degrade, tmp: &Path) -> Result<Frames> {
    let (w, h) = (frames.w, frames.h);
    let even = |v: f64| ((v / 2.0).round() as usize * 2).max(16);
    let (ew, eh) = (even(w as f64 * d.scale), even(h as f64 * d.scale));
    let mut child = Command::new(ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24"])
        .args(["-s", &format!("{w}x{h}"), "-r", &format!("{}", frames.fps), "-i", "-"])
        .args(["-vf", &format!("scale={ew}:{eh}:flags=bicubic"), "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p"])
        .args(["-b:v", &format!("{}k", d.kbps), "-maxrate", &format!("{}k", d.kbps * 3 / 2), "-bufsize", &format!("{}k", d.kbps * 2)])
        .arg(tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot run ffmpeg (needed for --degrade)")?;
    {
        let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("ffmpeg has no stdin"))?;
        stdin.write_all(&frames.data).context("writing frames to ffmpeg")?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("ffmpeg could not encode: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let out = Command::new(ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(tmp)
        .args(["-vf", &format!("scale={w}:{h}:flags=bicubic"), "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .context("cannot run ffmpeg")?;
    if !out.status.success() {
        bail!("ffmpeg could not decode: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let n = w * h * 3;
    let mut data = out.stdout;
    data.truncate(data.len() / n * n);
    if data.len() / n != frames.len() {
        bail!("the degraded clip has {} frames, the original {}", data.len() / n, frames.len());
    }
    let _ = std::fs::remove_file(tmp);
    Ok(Frames { w, h, fps: frames.fps, data })
}
