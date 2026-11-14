//! Video encoding through the system `ffmpeg`: raw RGBA frames go in on
//! stdin, an H.264 MP4 (with AAC audio when the project has any) comes out.
//! The sound of video layers and the sound effects (`audio_clips`) are mixed
//! with the song through `amix`.

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

pub(crate) fn program() -> String {
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

/// The sound of a video layer, mixed into the render.
#[derive(Clone, Debug)]
pub struct ClipAudio {
    pub path: PathBuf,
    /// Seconds of the file that play, `[in, out)` (the slot's trim).
    pub source: [f64; 2],
    /// Seconds from the start of the rendered range to the layer's `in`
    /// (negative when the layer started before the range).
    pub at: f64,
    /// Seconds the layer lasts.
    pub length: f64,
    /// Constant playback rate (pitch is kept).
    pub speed: f64,
    pub looped: bool,
    pub reverse: bool,
    pub volume: f64,
}

/// A sound effect (an entry of `audio_clips`), mixed into the render.
#[derive(Clone, Debug)]
pub struct SoundClip {
    pub path: PathBuf,
    /// Seconds of the file that play, `[in, out)`. `out` is infinite when the
    /// file's length is not known; the clip then plays to its end, unfaded.
    pub source: [f64; 2],
    /// Seconds from the start of the rendered range to the clip's start
    /// (negative when it started before the range).
    pub at: f64,
    /// Linear gain.
    pub volume: f64,
    /// Seconds of fade in and out, at the ends of `source`.
    pub fade_in: f64,
    pub fade_out: f64,
}

impl SoundClip {
    /// Seconds the clip plays.
    fn length(&self) -> f64 {
        self.source[1] - self.source[0]
    }
    /// Whether any of it is heard in a render of `duration` seconds.
    pub fn heard(&self, duration: f64) -> bool {
        self.volume > 0.0 && self.length() > 0.0 && self.at < duration && self.at + self.length() > 0.0
    }
}

/// Rate every input is resampled to before mixing.
const MIX_RATE: u32 = 48_000;

/// The filter chain that turns input `input` into the layer's sound on the
/// render's timeline, labelled `[label]`.
fn clip_filter(input: usize, c: &ClipAudio, label: &str) -> String {
    let [a, b] = c.source;
    let mut f = format!("[{input}:a]aresample={MIX_RATE},atrim=start={a:.6}:end={b:.6},asetpts=PTS-STARTPTS");
    if c.reverse {
        f.push_str(",areverse");
    }
    if c.looped {
        f.push_str(&format!(",aloop=loop=-1:size={}", ((b - a) * MIX_RATE as f64).round().max(1.0) as u64));
    }
    // atempo takes 0.5 to 100: slower than half speed is a chain of halves.
    let mut speed = c.speed.clamp(1e-3, 100.0);
    while speed < 0.5 {
        f.push_str(",atempo=0.5");
        speed /= 0.5;
    }
    if (speed - 1.0).abs() > 1e-9 {
        f.push_str(&format!(",atempo={speed:.6}"));
    }
    f.push_str(&format!(",atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS", (-c.at).max(0.0), c.length));
    if (c.volume - 1.0).abs() > 1e-9 {
        f.push_str(&format!(",volume={:.4}", c.volume));
    }
    let delay = (c.at.max(0.0) * 1000.0).round() as u64;
    if delay > 0 {
        f.push_str(&format!(",adelay={delay}:all=1"));
    }
    f.push_str(&format!("[{label}]"));
    f
}

/// The filter chain for a sound effect: cut, faded at its own ends, cut
/// again where the rendered range starts, then delayed to its place.
fn sound_filter(input: usize, c: &SoundClip, label: &str) -> String {
    let [a, b] = c.source;
    let len = c.length();
    let mut f = format!("[{input}:a]aresample={MIX_RATE},atrim=start={a:.6}");
    if b.is_finite() {
        f.push_str(&format!(":end={b:.6}"));
    }
    f.push_str(",asetpts=PTS-STARTPTS");
    // Fades that do not fit are shortened in proportion.
    let (mut fi, mut fo) = (c.fade_in.max(0.0), if len.is_finite() { c.fade_out.max(0.0) } else { 0.0 });
    if len.is_finite() && fi + fo > len {
        let k = len / (fi + fo);
        (fi, fo) = (fi * k, fo * k);
    }
    if fi > 0.0 {
        f.push_str(&format!(",afade=t=in:st=0:d={fi:.6}"));
    }
    if fo > 0.0 {
        f.push_str(&format!(",afade=t=out:st={:.6}:d={fo:.6}", len - fo));
    }
    if c.at < 0.0 {
        f.push_str(&format!(",atrim=start={:.6},asetpts=PTS-STARTPTS", -c.at));
    }
    if (c.volume - 1.0).abs() > 1e-9 {
        f.push_str(&format!(",volume={:.4}", c.volume));
    }
    let delay = (c.at.max(0.0) * 1000.0).round() as u64;
    if delay > 0 {
        f.push_str(&format!(",adelay={delay}:all=1"));
    }
    f.push_str(&format!("[{label}]"));
    f
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
    /// Video layers whose sound is mixed in.
    pub clips: Vec<ClipAudio>,
    /// Sound effects mixed in.
    pub sounds: Vec<SoundClip>,
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
    // Clips that are heard somewhere in the rendered range.
    let clips: Vec<&ClipAudio> = if job.aac.is_some() {
        job.clips.iter().filter(|c| c.volume > 0.0 && c.speed > 0.0 && c.source[1] > c.source[0] && c.at < duration && c.at + c.length > 0.0).collect()
    } else {
        Vec::new()
    };
    // Sound effects that are heard somewhere in the rendered range.
    let sounds: Vec<&SoundClip> = if job.aac.is_some() {
        job.sounds.iter().filter(|c| c.heard(duration)).collect()
    } else {
        Vec::new()
    };
    let first_clip = if audio.is_some() { 2 } else { 1 };
    for c in &clips {
        push(&["-i", &c.path.to_string_lossy()]);
    }
    for c in &sounds {
        push(&["-i", &c.path.to_string_lossy()]);
    }
    let mixed = !clips.is_empty() || !sounds.is_empty();
    push(&["-map", "0:v:0"]);
    if !mixed {
        if audio.is_some() {
            push(&["-map", "1:a:0"]);
        }
    } else {
        let mut graph = Vec::new();
        let mut labels = String::new();
        if let Some(au) = audio {
            graph.push(format!("[1:a]aresample={MIX_RATE},volume={:.4}[song]", au.gain.max(0.0)));
            labels.push_str("[song]");
        }
        for (i, c) in clips.iter().enumerate() {
            let label = format!("clip{i}");
            graph.push(clip_filter(first_clip + i, c, &label));
            labels.push_str(&format!("[{label}]"));
        }
        for (i, c) in sounds.iter().enumerate() {
            let label = format!("sfx{i}");
            graph.push(sound_filter(first_clip + clips.len() + i, c, &label));
            labels.push_str(&format!("[{label}]"));
        }
        let inputs = clips.len() + sounds.len() + usize::from(audio.is_some());
        // When every input starts late (adelay), amix's first frame has no
        // timestamp and the AAC track comes out a few milliseconds long:
        // aresample puts the mix back on a timeline that starts at 0.
        graph.push(format!("{labels}amix=inputs={inputs}:duration=longest:normalize=0,aresample=async=1:first_pts=0[mix]"));
        push(&["-filter_complex", &graph.join(";"), "-map", "[mix]"]);
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
    if let (Some(aac), true) = (job.aac, audio.is_some() || mixed) {
        push(&["-c:a", aac, "-b:a", "192k"]);
    }
    if let (Some(au), false) = (audio, mixed) {
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
        EncodeJob { output: Path::new("out.mp4"), width: 640, height: 360, fps: 25.0, frames: 50, crf: 18, encoder, audio, clips: Vec::new(), sounds: Vec::new(), aac: Some("aac") }
    }

    fn clip(at: f64) -> ClipAudio {
        ClipAudio { path: "clip.mp4".into(), source: [1.0, 3.0], at, length: 1.5, speed: 1.0, looped: false, reverse: false, volume: 1.0 }
    }

    #[test]
    fn clip_sound_is_mixed_with_the_song() {
        let mut j = job("libx264", Some(AudioInput { path: "song.wav".into(), start: 0.0, gain: 0.5 }));
        j.clips = vec![clip(0.5), ClipAudio { speed: 0.25, looped: true, reverse: true, volume: 0.8, ..clip(-0.25) }, ClipAudio { volume: 0.0, ..clip(0.0) }, clip(5.0)];
        let a = arguments(&j);
        let text = a.join(" ");
        // The muted clip and the one after the range are left out.
        assert!(text.contains("-i song.wav -i clip.mp4 -i clip.mp4 -map 0:v:0 -filter_complex"), "{text}");
        let graph = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        let parts: Vec<&str> = graph.split(';').collect();
        assert_eq!(parts[0], "[1:a]aresample=48000,volume=0.5000[song]");
        assert_eq!(parts[1], "[2:a]aresample=48000,atrim=start=1.000000:end=3.000000,asetpts=PTS-STARTPTS,atrim=start=0.000000:end=1.500000,asetpts=PTS-STARTPTS,adelay=500:all=1[clip0]");
        assert_eq!(
            parts[2],
            "[3:a]aresample=48000,atrim=start=1.000000:end=3.000000,asetpts=PTS-STARTPTS,areverse,aloop=loop=-1:size=96000,atempo=0.5,atempo=0.500000,atrim=start=0.250000:end=1.500000,asetpts=PTS-STARTPTS,volume=0.8000[clip1]"
        );
        assert_eq!(parts[3], "[song][clip0][clip1]amix=inputs=3:duration=longest:normalize=0,aresample=async=1:first_pts=0[mix]");
        assert!(text.contains("-map [mix]") && text.contains("-c:a aac") && !text.contains("-filter:a"), "{text}");
        // A clip alone still makes a sound track.
        let mut j = job("libx264", None);
        j.clips = vec![clip(0.0)];
        let text = arguments(&j).join(" ");
        assert!(text.contains("-i pipe:0 -i clip.mp4 -map 0:v:0 -filter_complex [1:a]aresample") && text.contains("[clip0]amix=inputs=1"), "{text}");
        assert!(text.contains("-c:a aac"), "{text}");
    }

    fn sound(at: f64) -> SoundClip {
        SoundClip { path: "hit.wav".into(), source: [0.0, 0.5], at, volume: 1.0, fade_in: 0.0, fade_out: 0.0 }
    }

    fn graph(a: &[String]) -> Vec<String> {
        a[a.iter().position(|x| x == "-filter_complex").expect("a filter graph") + 1].split(';').map(str::to_string).collect()
    }

    #[test]
    fn no_sound_effects_leave_the_song_alone() {
        let mut j = job("libx264", Some(AudioInput { path: "song.wav".into(), start: 0.0, gain: 1.0 }));
        // Muted, empty, after the range or over before it: none is heard.
        j.sounds = vec![SoundClip { volume: 0.0, ..sound(0.5) }, SoundClip { source: [1.0, 1.0], ..sound(0.5) }, sound(2.0), sound(-0.5)];
        let text = arguments(&j).join(" ");
        assert!(text.contains("-i song.wav -map 0:v:0 -map 1:a:0") && !text.contains("hit.wav") && !text.contains("-filter_complex"), "{text}");
    }

    #[test]
    fn one_sound_effect_is_mixed_over_the_song() {
        let mut j = job("libx264", Some(AudioInput { path: "song.wav".into(), start: 0.0, gain: 1.0 }));
        j.sounds = vec![sound(1.25)];
        let a = arguments(&j);
        let text = a.join(" ");
        assert!(text.contains("-i song.wav -i hit.wav -map 0:v:0 -filter_complex"), "{text}");
        assert_eq!(
            graph(&a),
            [
                "[1:a]aresample=48000,volume=1.0000[song]",
                "[2:a]aresample=48000,atrim=start=0.000000:end=0.500000,asetpts=PTS-STARTPTS,adelay=1250:all=1[sfx0]",
                "[song][sfx0]amix=inputs=2:duration=longest:normalize=0,aresample=async=1:first_pts=0[mix]",
            ]
        );
        assert!(text.contains("-map [mix]") && text.contains("-c:a aac") && !text.contains("-filter:a"), "{text}");
    }

    #[test]
    fn sound_effects_are_trimmed_faded_and_placed() {
        let mut j = job("libx264", Some(AudioInput { path: "song.wav".into(), start: 0.0, gain: 0.5 }));
        j.clips = vec![clip(0.0)];
        j.sounds = vec![
            // A riser cut from the middle of its file, faded both ways, quieter.
            SoundClip { path: "riser.wav".into(), source: [2.0, 3.5], at: 0.5, volume: 0.5, fade_in: 0.25, fade_out: 1.0 },
            // Started before the range; fades longer than the clip are shortened in proportion.
            SoundClip { source: [0.1, 0.6], fade_in: 0.5, fade_out: 0.5, ..sound(-0.2) },
            // Length unknown: no end, no fade out.
            SoundClip { source: [0.0, f64::INFINITY], fade_out: 0.3, ..sound(1.0) },
        ];
        let a = arguments(&j);
        let text = a.join(" ");
        assert!(text.contains("-i song.wav -i clip.mp4 -i riser.wav -i hit.wav -i hit.wav -map 0:v:0"), "{text}");
        let g = graph(&a);
        assert_eq!(g[0], "[1:a]aresample=48000,volume=0.5000[song]");
        assert!(g[1].starts_with("[2:a]") && g[1].ends_with("[clip0]"), "{}", g[1]);
        assert_eq!(g[2], "[3:a]aresample=48000,atrim=start=2.000000:end=3.500000,asetpts=PTS-STARTPTS,afade=t=in:st=0:d=0.250000,afade=t=out:st=0.500000:d=1.000000,volume=0.5000,adelay=500:all=1[sfx0]");
        assert_eq!(g[3], "[4:a]aresample=48000,atrim=start=0.100000:end=0.600000,asetpts=PTS-STARTPTS,afade=t=in:st=0:d=0.250000,afade=t=out:st=0.250000:d=0.250000,atrim=start=0.200000,asetpts=PTS-STARTPTS[sfx1]");
        assert_eq!(g[4], "[5:a]aresample=48000,atrim=start=0.000000,asetpts=PTS-STARTPTS,adelay=1000:all=1[sfx2]");
        assert_eq!(g[5], "[song][clip0][sfx0][sfx1][sfx2]amix=inputs=5:duration=longest:normalize=0,aresample=async=1:first_pts=0[mix]");
        // Sound effects alone make a sound track.
        let mut j = job("libx264", None);
        j.sounds = vec![sound(0.0), sound(0.5)];
        let a = arguments(&j);
        let text = a.join(" ");
        assert!(text.contains("-i pipe:0 -i hit.wav -i hit.wav -map 0:v:0 -filter_complex [1:a]aresample"), "{text}");
        assert_eq!(graph(&a)[2], "[sfx0][sfx1]amix=inputs=2:duration=longest:normalize=0,aresample=async=1:first_pts=0[mix]");
        assert!(text.contains("-c:a aac"), "{text}");
        // No AAC encoder: nothing is mixed.
        j.aac = None;
        assert!(!arguments(&j).join(" ").contains("hit.wav"));
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
