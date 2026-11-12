//! Video encoding through the system `ffmpeg`: raw RGBA frames go in on
//! stdin, an H.264 MP4 (with AAC audio when the project has any) comes out.

use anyhow::{anyhow, bail, Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

/// What the installed ffmpeg can do.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Ffmpeg {
    pub found: bool,
    /// First line of `ffmpeg -version`.
    pub version: Option<String>,
    /// H.264 encoders this tool can drive, best first.
    pub h264: Vec<String>,
    pub aac: Vec<String>,
}

const H264_ENCODERS: [&str; 2] = ["libx264", "libopenh264"];
const AAC_ENCODERS: [&str; 2] = ["aac", "libfdk_aac"];
const INSTALL_HINT: &str = "install ffmpeg with H.264 support, e.g. `apt install ffmpeg` (Debian/Ubuntu), `brew install ffmpeg` (macOS) or `winget install ffmpeg` (Windows); it must provide the libx264 or libopenh264 encoder";

fn program() -> String {
    std::env::var("EDITS_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string())
}

/// Ask ffmpeg what it has. Never fails: a missing ffmpeg is `found: false`.
pub fn probe() -> Ffmpeg {
    let run = |args: &[&str]| Command::new(program()).args(args).stdin(Stdio::null()).output().ok().filter(|o| o.status.success());
    let Some(version) = run(&["-hide_banner", "-version"]) else { return Ffmpeg::default() };
    let version = String::from_utf8_lossy(&version.stdout).lines().next().map(str::to_string);
    let listing = run(&["-hide_banner", "-encoders"]).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    // Lines look like " V....D libx264              libx264 H.264 / AVC ...".
    let names: Vec<&str> = listing.lines().filter_map(|l| l.split_whitespace().nth(1)).collect();
    let pick = |wanted: &[&str]| wanted.iter().filter(|w| names.contains(*w)).map(|w| w.to_string()).collect();
    Ffmpeg { found: true, version, h264: pick(&H264_ENCODERS), aac: pick(&AAC_ENCODERS) }
}

impl Ffmpeg {
    /// The H.264 encoder to use, or an error that says what to install.
    pub fn h264_encoder(&self) -> Result<&str> {
        if !self.found {
            bail!("ffmpeg was not found on PATH; {INSTALL_HINT} (or point EDITS_FFMPEG at the binary)");
        }
        self.h264.first().map(String::as_str).ok_or_else(|| anyhow!("this ffmpeg has no usable H.264 encoder (looked for libx264, then libopenh264); {INSTALL_HINT}"))
    }
}

pub struct AudioInput {
    pub path: PathBuf,
    /// Seconds into the file where the rendered range starts (may be negative).
    pub start: f64,
    pub gain: f64,
}

pub struct EncodeJob<'a> {
    pub output: &'a Path,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frames: u64,
    pub crf: u32,
    pub encoder: &'a str,
    pub audio: Option<AudioInput>,
    pub aac: Option<&'a str>,
}

/// The ffmpeg command line for a job (kept separate so it can be tested).
pub fn arguments(job: &EncodeJob) -> Vec<String> {
    let duration = job.frames as f64 / job.fps;
    let mut a: Vec<String> = ["-y", "-hide_banner", "-loglevel", "error", "-nostats", "-f", "rawvideo", "-pix_fmt", "rgba"].map(String::from).to_vec();
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));
    push(&["-s", &format!("{}x{}", job.width, job.height), "-framerate", &format!("{}", job.fps), "-i", "pipe:0"]);
    let audio = job.audio.as_ref().filter(|_| job.aac.is_some());
    if let Some(au) = audio {
        if au.start >= 0.0 {
            push(&["-ss", &format!("{:.6}", au.start), "-t", &format!("{duration:.6}")]);
        } else {
            // The range starts before the audio does: delay the audio instead.
            push(&["-itsoffset", &format!("{:.6}", -au.start), "-t", &format!("{:.6}", (duration + au.start).max(0.0))]);
        }
        push(&["-i", &au.path.to_string_lossy()]);
    }
    push(&["-map", "0:v:0"]);
    if audio.is_some() {
        push(&["-map", "1:a:0"]);
    }
    // Full-range RGB to limited-range BT.709, tagged so players decode it the same way.
    push(&["-vf", "scale=in_range=pc:out_range=tv:out_color_matrix=bt709,format=yuv420p"]);
    push(&["-c:v", job.encoder]);
    if job.encoder == "libx264" {
        push(&["-preset", "medium", "-crf", &job.crf.to_string()]);
    } else {
        // libopenh264 has no CRF: aim for a bitrate that matches the frame size and the requested quality.
        let quality = 2f64.powf((18.0 - job.crf as f64) / 6.0);
        let bitrate = (job.width as f64 * job.height as f64 * job.fps * 0.12 * quality).clamp(300_000.0, 80_000_000.0);
        push(&["-b:v", &format!("{}", bitrate as u64)]);
    }
    push(&["-pix_fmt", "yuv420p", "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]);
    if let (Some(au), Some(aac)) = (audio, job.aac) {
        push(&["-c:a", aac, "-b:a", "192k"]);
        if (au.gain - 1.0).abs() > 1e-9 {
            push(&["-filter:a", &format!("volume={:.4}", au.gain.max(0.0))]);
        }
    }
    push(&["-t", &format!("{duration:.6}"), "-movflags", "+faststart", &job.output.to_string_lossy()]);
    a
}

/// A running ffmpeg that takes frames on stdin.
pub struct Encoder {
    child: Child,
    stdin: Option<ChildStdin>,
    errors: Option<std::thread::JoinHandle<String>>,
    frame_bytes: usize,
}

impl Encoder {
    pub fn start(job: &EncodeJob) -> Result<Self> {
        let mut child = Command::new(program())
            .args(arguments(job))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("cannot start ffmpeg; {INSTALL_HINT}"))?;
        let stdin = child.stdin.take();
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        Ok(Self { child, stdin, errors: Some(errors), frame_bytes: (job.width * job.height * 4) as usize })
    }

    fn failure(&mut self, what: &str) -> anyhow::Error {
        drop(self.stdin.take());
        let status = self.child.wait().ok();
        let text = self.errors.take().and_then(|h| h.join().ok()).unwrap_or_default();
        let tail: Vec<&str> = text.lines().rev().take(12).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        anyhow!("ffmpeg {what} ({}):\n{}", status.map(|s| s.to_string()).unwrap_or_else(|| "no exit status".into()), tail.join("\n"))
    }

    /// Send one frame of straight RGBA8, top row first.
    pub fn write(&mut self, rgba: &[u8]) -> Result<()> {
        if rgba.len() != self.frame_bytes {
            bail!("frame has {} bytes, the encoder expects {}", rgba.len(), self.frame_bytes);
        }
        let result = self.stdin.as_mut().expect("encoder is open").write_all(rgba);
        result.map_err(|_| self.failure("stopped while frames were being written"))
    }

    /// Close the input and wait for the file to be finished.
    pub fn finish(mut self) -> Result<()> {
        drop(self.stdin.take());
        let status = self.child.wait().context("waiting for ffmpeg")?;
        if !status.success() {
            return Err(self.failure("failed"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job<'a>(encoder: &'a str, audio: Option<AudioInput>) -> EncodeJob<'a> {
        EncodeJob { output: Path::new("out.mp4"), width: 640, height: 360, fps: 25.0, frames: 50, crf: 18, encoder, audio, aac: Some("aac") }
    }

    #[test]
    fn x264_arguments() {
        let a = arguments(&job("libx264", None)).join(" ");
        assert!(a.contains("-f rawvideo -pix_fmt rgba -s 640x360 -framerate 25 -i pipe:0"), "{a}");
        assert!(a.contains("-c:v libx264 -preset medium -crf 18 -pix_fmt yuv420p"), "{a}");
        assert!(a.ends_with("-t 2.000000 -movflags +faststart out.mp4"), "{a}");
        assert!(!a.contains("-c:a"), "{a}");
    }

    #[test]
    fn audio_is_trimmed_to_the_rendered_range() {
        let au = AudioInput { path: "ref.wav".into(), start: 1.5, gain: 0.5 };
        let a = arguments(&job("libx264", Some(au))).join(" ");
        assert!(a.contains("-ss 1.500000 -t 2.000000 -i ref.wav -map 0:v:0 -map 1:a:0"), "{a}");
        assert!(a.contains("-c:a aac -b:a 192k -filter:a volume=0.5000"), "{a}");
        let early = AudioInput { path: "ref.wav".into(), start: -0.5, gain: 1.0 };
        let a = arguments(&job("libx264", Some(early))).join(" ");
        assert!(a.contains("-itsoffset 0.500000 -t 1.500000 -i ref.wav"), "{a}");
    }

    #[test]
    fn openh264_uses_a_bitrate() {
        let a = arguments(&job("libopenh264", None)).join(" ");
        assert!(a.contains("-c:v libopenh264 -b:v 691200"), "{a}");
        assert!(!a.contains("-crf"), "{a}");
    }

    #[test]
    fn missing_encoders_say_what_to_install() {
        let none = Ffmpeg::default();
        assert!(none.h264_encoder().unwrap_err().to_string().contains("not found"));
        let bare = Ffmpeg { found: true, ..Default::default() };
        assert!(bare.h264_encoder().unwrap_err().to_string().contains("libx264"));
        let both = Ffmpeg { found: true, h264: vec!["libx264".into(), "libopenh264".into()], ..Default::default() };
        assert_eq!(both.h264_encoder().unwrap(), "libx264");
    }
}
