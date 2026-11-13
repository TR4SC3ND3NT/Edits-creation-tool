//! Decoding through the system `ffmpeg` / `ffprobe` (subprocess, no linking).

use crate::img::Frames;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VideoInfo {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Frame count reported by the container (0 when unknown).
    pub frames: u64,
    pub duration: f64,
    pub has_audio: bool,
}

fn rational(s: &str) -> f64 {
    let mut it = s.split('/');
    let n: f64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let d: f64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    if d > 0.0 {
        n / d
    } else {
        0.0
    }
}

/// Container facts from `ffprobe`.
pub fn probe(path: &Path) -> Result<VideoInfo> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_streams", "-show_format", "-of", "json"])
        .arg(path)
        .output()
        .context("running ffprobe (is ffmpeg installed and on PATH?)")?;
    if !out.status.success() {
        bail!("ffprobe failed on {}: {}", path.display(), String::from_utf8_lossy(&out.stderr).trim());
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or_else(|| anyhow!("{} has no video stream", path.display()))?;
    let has_audio = streams.iter().any(|s| s["codec_type"] == "audio");
    let num = |val: &serde_json::Value| val.as_str().and_then(|s| s.parse::<f64>().ok()).or(val.as_f64());
    let mut fps = rational(video["avg_frame_rate"].as_str().unwrap_or("0/1"));
    if fps.is_nan() || fps <= 0.0 {
        fps = rational(video["r_frame_rate"].as_str().unwrap_or("0/1"));
    }
    let duration = num(&video["duration"]).or(num(&v["format"]["duration"])).unwrap_or(0.0);
    let frames = num(&video["nb_frames"]).unwrap_or((duration * fps).round()) as u64;
    Ok(VideoInfo {
        path: path.to_string_lossy().into_owned(),
        width: video["width"].as_u64().unwrap_or(0) as u32,
        height: video["height"].as_u64().unwrap_or(0) as u32,
        fps,
        frames,
        duration,
        has_audio,
    })
}

fn run_ffmpeg(args: &[String]) -> Result<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("running ffmpeg (is it installed and on PATH?)")?;
    if !out.status.success() {
        bail!("ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// Analysis size for a source: `height` px high (never upscaled), even width.
pub fn analysis_size(info: &VideoInfo, height: u32) -> (usize, usize) {
    let h = height.min(info.height).max(16) & !1;
    let w = ((info.width as f64 * h as f64 / info.height as f64 / 2.0).round() as u32 * 2).max(16);
    (w as usize, h as usize)
}

/// Decode every frame, scaled to `height` px high, as RGB8.
pub fn decode_frames(path: &Path, info: &VideoInfo, height: u32) -> Result<Frames> {
    let (w, h) = analysis_size(info, height);
    let args: Vec<String> = [
        "-i",
        &path.to_string_lossy(),
        "-an",
        "-sn",
        "-vf",
        &format!("scale={w}:{h}:flags=area"),
        "-fps_mode",
        "passthrough",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let data = run_ffmpeg(&args)?;
    let n = w * h * 3;
    if data.len() < n {
        bail!("no frames decoded from {}", path.display());
    }
    let mut data = data;
    data.truncate(data.len() / n * n);
    Ok(Frames { w, h, fps: info.fps, data })
}

/// Decode the listed frames (ascending frame numbers) at full resolution.
pub fn decode_selected(path: &Path, info: &VideoInfo, frames: &[usize]) -> Result<Frames> {
    let (w, h) = (info.width as usize, info.height as usize);
    if frames.is_empty() {
        return Ok(Frames::new(w, h, info.fps));
    }
    let expr: Vec<String> = frames.iter().map(|f| format!("eq(n\\,{f})")).collect();
    let args: Vec<String> = [
        "-i",
        &path.to_string_lossy(),
        "-an",
        "-sn",
        "-vf",
        &format!("select='{}'", expr.join("+")),
        "-fps_mode",
        "passthrough",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut data = run_ffmpeg(&args)?;
    let n = w * h * 3;
    data.truncate(data.len() / n * n);
    Ok(Frames { w, h, fps: info.fps, data })
}

/// Decode the audio track to mono float samples at `rate` Hz. `None` when the
/// file has no audio.
pub fn decode_audio(path: &Path, info: &VideoInfo, rate: u32) -> Result<Option<Vec<f32>>> {
    if !info.has_audio {
        return Ok(None);
    }
    let args: Vec<String> = ["-i", &path.to_string_lossy(), "-vn", "-sn", "-f", "s16le", "-ar", &rate.to_string(), "-ac", "1", "-"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let data = run_ffmpeg(&args)?;
    let samples: Vec<f32> = data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    Ok(if samples.is_empty() { None } else { Some(samples) })
}

/// Decode the audio of any media file (a song, or a video's soundtrack) to
/// mono float samples at `rate` Hz.
pub fn decode_audio_file(path: &Path, rate: u32) -> Result<Vec<f32>> {
    if !path.is_file() {
        bail!("{} does not exist or is not a file", path.display());
    }
    let args: Vec<String> = ["-i", &path.to_string_lossy(), "-vn", "-sn", "-dn", "-f", "s16le", "-ar", &rate.to_string(), "-ac", "1", "-"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let data = run_ffmpeg(&args).with_context(|| format!("decoding the audio of {}", path.display()))?;
    let samples: Vec<f32> = data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    if samples.is_empty() {
        bail!("{} has no audio track", path.display());
    }
    Ok(samples)
}
