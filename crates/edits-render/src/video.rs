//! Video sources: what a clip is (`ffprobe`), its frames (`ffmpeg` decoding
//! to raw RGBA on a pipe) and a cache of decoded frames.
//!
//! One decoder process runs per clip. It is started at the first frame asked
//! for (seeking half a frame early, so the first frame it gives is exactly
//! that one) and then read in order: a forward render reads each frame once
//! from one process. A frame behind the decoder, or more than
//! [`MAX_SKIP_SECONDS`] ahead of it, restarts it at that frame. Every frame
//! read goes into a least-recently-used cache with a byte budget, keyed by
//! the clip (file, size) and the frame index, so going back a little, motion
//! blur samples and frame blending find their frames there.
//!
//! None of this changes what a frame looks like: the picture of source frame
//! `n` is the same however it was reached.

use crate::assets;
use anyhow::{anyhow, bail, Context, Result};
use image::RgbaImage;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, OnceLock};

/// A jump forward by more than this restarts the decoder instead of reading through.
const MAX_SKIP_SECONDS: f64 = 2.0;
/// Default byte budget of the frame cache; `EDITS_VIDEO_CACHE_MB` changes it.
const DEFAULT_CACHE_MB: usize = 512;

const INSTALL_HINT: &str = "install ffmpeg (it brings ffprobe), e.g. `apt install ffmpeg` or `brew install ffmpeg`, or point EDITS_FFMPEG and EDITS_FFPROBE at the binaries";

/// The `ffmpeg` to run: `EDITS_FFMPEG`, or `ffmpeg` on `PATH`.
pub fn ffmpeg_program() -> String {
    std::env::var("EDITS_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string())
}

/// The `ffprobe` to run: `EDITS_FFPROBE`, or the `ffprobe` next to
/// `EDITS_FFMPEG` when that names a path, or `ffprobe` on `PATH`.
pub fn ffprobe_program() -> String {
    if let Ok(p) = std::env::var("EDITS_FFPROBE") {
        return p;
    }
    if let Ok(ff) = std::env::var("EDITS_FFMPEG") {
        let ff = PathBuf::from(ff);
        if let (Some(dir), Some(name)) = (ff.parent().filter(|d| !d.as_os_str().is_empty()), ff.file_name()) {
            let probe = dir.join(name.to_string_lossy().replacen("ffmpeg", "ffprobe", 1));
            if probe.is_file() {
                return probe.to_string_lossy().into_owned();
            }
        }
    }
    "ffprobe".to_string()
}

/// What `ffprobe` says about a video file.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo {
    /// Stored size in pixels.
    pub width: u32,
    pub height: u32,
    /// Width over height as displayed (sample aspect ratio included).
    pub aspect: f64,
    /// Frames per second (`avg_frame_rate`, else `r_frame_rate`).
    pub fps: f64,
    /// Frames in the video stream.
    pub frames: u64,
    pub has_audio: bool,
}

impl VideoInfo {
    pub fn duration(&self) -> f64 {
        self.frames as f64 / self.fps
    }
}

fn rational(s: &str) -> Option<f64> {
    let (n, d) = s.split_once(['/', ':']).unwrap_or((s, "1"));
    let (n, d): (f64, f64) = (n.trim().parse().ok()?, d.trim().parse().ok()?);
    let v = n / d;
    (v.is_finite() && v > 0.0).then_some(v)
}

/// Parse `ffprobe -of compact=p=0` output: one line per stream, `key=value|...`.
fn parse_probe(text: &str) -> Option<VideoInfo> {
    let streams: Vec<HashMap<&str, &str>> = text.lines().map(|l| l.split('|').filter_map(|kv| kv.split_once('=')).collect()).collect();
    let video = streams.iter().find(|s| s.get("codec_type") == Some(&"video"))?;
    let has_audio = streams.iter().any(|s| s.get("codec_type") == Some(&"audio"));
    let num = |k: &str| video.get(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite() && *v > 0.0);
    let fps = video.get("avg_frame_rate").and_then(|v| rational(v)).or_else(|| video.get("r_frame_rate").and_then(|v| rational(v)))?;
    let (width, height) = (num("width")? as u32, num("height")? as u32);
    let sar = video.get("sample_aspect_ratio").and_then(|v| rational(v)).unwrap_or(1.0);
    let frames = num("nb_read_packets").or(num("nb_frames")).or_else(|| num("duration").map(|d| (d * fps).round()))? as u64;
    Some(VideoInfo { width, height, aspect: width as f64 * sar / height as f64, fps, frames, has_audio })
}

/// Probe a video file: size, frame rate, frame count (by counting the
/// packets of the video stream, which does not decode) and whether it has sound.
pub fn probe(path: &Path) -> Result<VideoInfo> {
    let out = Command::new(ffprobe_program())
        .args(["-v", "error", "-count_packets", "-show_entries", "stream=codec_type,width,height,sample_aspect_ratio,avg_frame_rate,r_frame_rate,nb_frames,nb_read_packets,duration", "-of", "compact=p=0"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("cannot run ffprobe; {INSTALL_HINT}"))?;
    if !out.status.success() {
        bail!("ffprobe cannot read {}: {}", path.display(), String::from_utf8_lossy(&out.stderr).trim());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_probe(&text).ok_or_else(|| anyhow!("{} has no video stream with a size, a frame rate and frames (ffprobe said: {})", path.display(), text.trim()))
}

/// A clip as a layer uses it: a file, the frames its slot's `trim` keeps,
/// and the size frames are decoded at.
#[derive(Clone, Debug)]
pub(crate) struct Clip {
    pub path: PathBuf,
    /// The file (path, size, modification time) and the decode size.
    pub key: String,
    pub info: VideoInfo,
    /// First source frame kept by the trim, and how many are kept.
    pub first: u64,
    pub count: u64,
    /// Decoded frame size.
    pub w: u32,
    pub h: u32,
}

impl Clip {
    pub fn new(path: PathBuf, file_key: &str, info: VideoInfo, trim: (u64, u64), max_side: u32) -> Self {
        // Square pixels at the displayed aspect, no larger than `max_side`, never upscaled.
        let (dw, dh) = (info.height as f64 * info.aspect, info.height as f64);
        let k = (max_side as f64 / dw.max(dh)).min(1.0);
        let (w, h) = (((dw * k).round() as u32).max(1), ((dh * k).round() as u32).max(1));
        Self { key: format!("{file_key}|{w}x{h}"), path, info, first: trim.0, count: trim.1, w, h }
    }

    pub fn info(&self) -> edits_core::ClipInfo {
        edits_core::ClipInfo { fps: self.info.fps, frames: self.count }
    }
}

/// A running decoder: the next frame it will give is `next` (a source frame index).
struct Decoder {
    child: Child,
    out: ChildStdout,
    next: u64,
    errors: Option<std::thread::JoinHandle<String>>,
}

impl Decoder {
    fn start(clip: &Clip, frame: u64) -> Result<Self> {
        // Half a frame early: the first frame at or after that time is `frame`,
        // whatever the container's timestamp rounding.
        let at = ((frame as f64 - 0.5) / clip.info.fps).max(0.0);
        let mut cmd = Command::new(ffmpeg_program());
        cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
        if at > 0.0 {
            cmd.args(["-ss", &format!("{at:.6}")]);
        }
        cmd.arg("-i").arg(&clip.path).args(["-map", "0:v:0", "-an", "-sn", "-dn", "-fps_mode", "passthrough"]);
        if (clip.w, clip.h) != (clip.info.width, clip.info.height) {
            cmd.args(["-vf", &format!("scale={}:{}:flags=bicubic", clip.w, clip.h)]);
        }
        cmd.args(["-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"]);
        let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().with_context(|| format!("cannot start ffmpeg to decode {}; {INSTALL_HINT}", clip.path.display()))?;
        let out = child.stdout.take().expect("stdout is piped");
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        Ok(Self { child, out, next: frame, errors: Some(errors) })
    }

    /// The next frame, or `None` at the end of the stream.
    fn read(&mut self, w: u32, h: u32) -> Option<RgbaImage> {
        let mut buf = vec![0u8; (w * h * 4) as usize];
        self.out.read_exact(&mut buf).ok()?;
        self.next += 1;
        RgbaImage::from_raw(w, h, buf)
    }

    /// What ffmpeg said on stderr, once it has stopped.
    fn finish(mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.errors.take().and_then(|h| h.join().ok()).unwrap_or_default().trim().to_string()
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Counters for tests and profiling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoStats {
    /// Decoder processes started.
    pub decoders_started: u64,
    /// Frames read from decoders.
    pub frames_decoded: u64,
    /// Frames served from the cache.
    pub cache_hits: u64,
}

/// Decoders and the frame cache, shared by every clip of every project.
pub(crate) struct Videos {
    decoders: HashMap<String, Decoder>,
    /// (clip key, source frame) -> (frame, last use).
    cache: HashMap<(String, u64), (Arc<RgbaImage>, u64)>,
    /// Frames past the end of a clip whose frame count was too high: clip key -> frames really there.
    short: HashMap<String, u64>,
    bytes: usize,
    budget: usize,
    clock: u64,
    pub stats: VideoStats,
}

impl Default for Videos {
    fn default() -> Self {
        let mb = std::env::var("EDITS_VIDEO_CACHE_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(DEFAULT_CACHE_MB);
        Self { decoders: HashMap::new(), cache: HashMap::new(), short: HashMap::new(), bytes: 0, budget: mb.max(1) << 20, clock: 0, stats: VideoStats::default() }
    }
}

impl Videos {
    /// Stop the decoders and drop the cached frames of clips not in `keep`.
    pub fn retain(&mut self, keep: &[&str]) {
        self.decoders.retain(|k, _| keep.contains(&k.as_str()));
        let before = self.cache.len();
        self.cache.retain(|(k, _), _| keep.contains(&k.as_str()));
        if self.cache.len() != before {
            self.bytes = self.cache.values().map(|(f, _)| f.as_raw().len()).sum();
        }
    }

    fn insert(&mut self, key: (String, u64), frame: Arc<RgbaImage>) {
        self.clock += 1;
        self.bytes += frame.as_raw().len();
        if let Some((old, _)) = self.cache.insert(key, (frame, self.clock)) {
            self.bytes -= old.as_raw().len();
        }
        while self.bytes > self.budget && self.cache.len() > 2 {
            let oldest = self.cache.iter().min_by_key(|(_, (_, used))| *used).map(|(k, _)| k.clone()).expect("not empty");
            let (gone, _) = self.cache.remove(&oldest).expect("present");
            self.bytes -= gone.as_raw().len();
        }
    }

    /// Frame `index` of `clip` (counted from the trim's first frame; past the
    /// end it is the last frame), premultiplied sRGB RGBA8.
    pub fn frame(&mut self, clip: &Clip, index: u64) -> Result<Arc<RgbaImage>> {
        let mut last = clip.first + clip.count.max(1) - 1;
        if let Some(&n) = self.short.get(&clip.key) {
            last = last.min(n.saturating_sub(1));
        }
        let want = (clip.first + index).min(last);
        let key = (clip.key.clone(), want);
        if let Some((frame, used)) = self.cache.get_mut(&key) {
            self.clock += 1;
            *used = self.clock;
            self.stats.cache_hits += 1;
            return Ok(frame.clone());
        }
        let ahead = (MAX_SKIP_SECONDS * clip.info.fps).ceil().max(8.0) as u64;
        let reuse = self.decoders.get(&clip.key).is_some_and(|d| d.next <= want && want <= d.next + ahead);
        if !reuse {
            self.decoders.remove(&clip.key);
            self.decoders.insert(clip.key.clone(), Decoder::start(clip, want)?);
            self.stats.decoders_started += 1;
        }
        loop {
            let decoder = self.decoders.get_mut(&clip.key).expect("started above");
            let at = decoder.next;
            match decoder.read(clip.w, clip.h) {
                Some(mut img) => {
                    self.stats.frames_decoded += 1;
                    assets::premultiply(&mut img);
                    let img = Arc::new(img);
                    self.insert((clip.key.clone(), at), img.clone());
                    if at == want {
                        return Ok(img);
                    }
                }
                None => {
                    let decoder = self.decoders.remove(&clip.key).expect("present");
                    let said = decoder.finish();
                    if at == 0 || at <= clip.first {
                        bail!("ffmpeg gave no frame {at} of {}{}", clip.path.display(), if said.is_empty() { String::new() } else { format!(": {said}") });
                    }
                    // The file has fewer frames than its header says: hold the last one there is.
                    self.short.insert(clip.key.clone(), at);
                    return self.frame(clip, at - 1 - clip.first);
                }
            }
        }
    }
}

/// Cross-fade two premultiplied sRGB pictures of the same size: `b` by
/// `weight`, in linear light.
pub(crate) fn mix(a: &RgbaImage, b: &RgbaImage, weight: f64) -> RgbaImage {
    static DECODE: OnceLock<Vec<f32>> = OnceLock::new();
    static ENCODE: OnceLock<Vec<u8>> = OnceLock::new();
    const STEPS: usize = 1 << 16;
    let decode = DECODE.get_or_init(|| (0..256).map(|i| assets::srgb_to_linear(i as f32 / 255.0)).collect());
    let encode = ENCODE.get_or_init(|| (0..STEPS).map(|i| (assets::linear_to_srgb(i as f32 / (STEPS - 1) as f32) * 255.0).round() as u8).collect());
    let w = weight.clamp(0.0, 1.0) as f32;
    let mut out = a.clone();
    for (o, q) in out.as_mut().chunks_exact_mut(4).zip(b.as_raw().chunks_exact(4)) {
        for c in 0..3 {
            let v = decode[o[c] as usize] * (1.0 - w) + decode[q[c] as usize] * w;
            o[c] = encode[(v.clamp(0.0, 1.0) * (STEPS - 1) as f32).round() as usize];
        }
        o[3] = (o[3] as f32 * (1.0 - w) + q[3] as f32 * w).round() as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_output_is_parsed() {
        let text = "codec_type=video|width=1920|height=1080|sample_aspect_ratio=1:1|r_frame_rate=30000/1001|avg_frame_rate=30000/1001|duration=N/A|nb_frames=N/A|nb_read_packets=48\n\
                    codec_type=audio|r_frame_rate=0/0|avg_frame_rate=0/0|duration=N/A|nb_frames=N/A|nb_read_packets=94\n";
        let info = parse_probe(text).unwrap();
        assert_eq!((info.width, info.height, info.frames, info.has_audio), (1920, 1080, 48, true));
        assert!((info.fps - 29.97).abs() < 0.01 && (info.aspect - 16.0 / 9.0).abs() < 1e-9);
        // Anamorphic pixels, no packet count, no sound.
        let info = parse_probe("codec_type=video|width=720|height=576|sample_aspect_ratio=64:45|avg_frame_rate=0/0|r_frame_rate=25/1|duration=2.000000|nb_frames=N/A\n").unwrap();
        assert_eq!((info.frames, info.has_audio), (50, false));
        assert!((info.aspect - 720.0 * 64.0 / 45.0 / 576.0).abs() < 1e-9);
        assert!(parse_probe("codec_type=audio|nb_read_packets=3\n").is_none());
    }

    #[test]
    fn clips_decode_at_square_pixels_without_upscaling() {
        let info = VideoInfo { width: 3840, height: 2160, aspect: 16.0 / 9.0, fps: 30.0, frames: 10, has_audio: false };
        let c = Clip::new("a.mp4".into(), "a", info.clone(), (0, 10), 1920);
        assert_eq!((c.w, c.h), (1920, 1080));
        let c = Clip::new("a.mp4".into(), "a", VideoInfo { width: 64, height: 36, ..info }, (0, 10), 1920);
        assert_eq!((c.w, c.h), (64, 36));
    }

    #[test]
    fn mixing_halfway_averages_in_linear_light() {
        let a = RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 100, 255]));
        let b = RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 200, 255]));
        let m = mix(&a, &b, 0.5).get_pixel(0, 0).0;
        let half = |x: u8, y: u8| (assets::linear_to_srgb((assets::srgb_to_linear(x as f32 / 255.0) + assets::srgb_to_linear(y as f32 / 255.0)) / 2.0) * 255.0).round() as u8;
        assert_eq!(m, [half(0, 255), 255, half(100, 200), 255]);
        assert_eq!(mix(&a, &b, 0.0), a);
    }
}
