//! The project's audio: decoded once to PCM by the system `ffmpeg`, played
//! through the default output device while the transport plays, and reduced
//! to peaks for the waveform under the ruler.
//!
//! Nothing here may stop the window from working: with no `ffmpeg`, a file
//! it cannot read or no sound device, playback is silent and the status bar
//! says why.

use rodio::{ChannelCount, MixerDeviceSink, SampleRate, Source};
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

/// Everything is decoded to interleaved stereo at this rate; the output
/// device's own rate is handled by the mixer.
pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// Waveform resolution: one (min, max) pair per 10 ms.
pub const PEAKS_PER_SECOND: usize = 100;

/// Decoded audio.
pub struct Clip {
    /// Interleaved stereo samples at [`RATE`].
    pub samples: Vec<f32>,
    /// (min, max) over each 10 ms, both channels together.
    pub peaks: Vec<[f32; 2]>,
}

impl Clip {
    pub fn from_samples(samples: Vec<f32>) -> Self {
        let peaks = peaks(&samples, CHANNELS, RATE as usize / PEAKS_PER_SECOND);
        Self { samples, peaks }
    }

    pub fn seconds(&self) -> f64 {
        (self.samples.len() / CHANNELS) as f64 / RATE as f64
    }
}

/// Reduce interleaved samples to one (min, max) pair per `frames_per_peak` frames.
pub fn peaks(samples: &[f32], channels: usize, frames_per_peak: usize) -> Vec<[f32; 2]> {
    let chunk = (channels * frames_per_peak).max(1);
    samples
        .chunks(chunk)
        .map(|c| {
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for &s in c {
                lo = lo.min(s);
                hi = hi.max(s);
            }
            [lo, hi]
        })
        .collect()
}

/// The (min, max) of the waveform between two times of the audio file, in
/// seconds. `None` where the file has no sound (before its start or after its end).
pub fn peak_range(peaks: &[[f32; 2]], from: f64, to: f64) -> Option<(f32, f32)> {
    if to < 0.0 || peaks.is_empty() {
        return None;
    }
    let first = (from.max(0.0) * PEAKS_PER_SECOND as f64 + 1e-9).floor() as usize;
    if first >= peaks.len() {
        return None;
    }
    let last = ((to.max(0.0) * PEAKS_PER_SECOND as f64 - 1e-9).ceil().max(0.0) as usize).clamp(first + 1, peaks.len());
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in &peaks[first..last] {
        lo = lo.min(p[0]);
        hi = hi.max(p[1]);
    }
    Some((lo, hi))
}

/// Index of the interleaved sample at `seconds` into the file. Negative
/// before the file starts: that much silence is played first.
pub fn start_sample(seconds: f64) -> i64 {
    (seconds * RATE as f64).round() as i64 * CHANNELS as i64
}

fn ffmpeg_binary() -> String {
    std::env::var("EDITS_FFMPEG").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "ffmpeg".to_string())
}

/// Decode any file `ffmpeg` understands (audio, or the audio of a video) to PCM.
pub fn decode(path: &Path) -> Result<Clip, String> {
    let bin = ffmpeg_binary();
    let out = Command::new(&bin)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-vn", "-map", "0:a:0", "-ac", "2", "-ar"])
        .arg(RATE.to_string())
        .args(["-f", "f32le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("cannot run {bin} ({e}); install ffmpeg or set EDITS_FFMPEG"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().last().unwrap_or("no message").trim().to_string();
        return Err(format!("ffmpeg could not decode it: {line}"));
    }
    let samples: Vec<f32> = out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    if samples.len() < CHANNELS {
        return Err(format!("{} has no audio", path.display()));
    }
    Ok(Clip::from_samples(samples))
}

/// A stretch of a clip as a rodio source. It ends when the clip does, or
/// when the engine's token moves on (stop, seek, loop).
struct ClipSource {
    clip: Arc<Clip>,
    pos: i64,
    gain: f32,
    token: Arc<AtomicU64>,
    mine: u64,
}

impl Iterator for ClipSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        // Checked once per stereo frame: cheap, and a stop takes effect at once.
        if self.pos % 2 == 0 && self.token.load(Ordering::Relaxed) != self.mine {
            return None;
        }
        let i = self.pos;
        self.pos += 1;
        if i < 0 {
            return Some(0.0);
        }
        self.clip.samples.get(i as usize).map(|s| s * self.gain)
    }
}

impl Source for ClipSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        NonZero::new(CHANNELS as u16).expect("two channels")
    }
    fn sample_rate(&self) -> SampleRate {
        NonZero::new(RATE).expect("a sample rate")
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// What the status bar says about sound.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioStatus {
    /// The project has no `audio`.
    None,
    Decoding,
    Ready { seconds: f64 },
    /// Playback is silent, and why.
    Silent(String),
}

pub struct AudioEngine {
    sink: Option<MixerDeviceSink>,
    sink_error: Option<String>,
    sink_tried: bool,
    /// Do not open a sound device at all (tests).
    muted: bool,
    clip: Option<Arc<Clip>>,
    /// The file the clip (or the running decode, or the error) belongs to.
    source: Option<(PathBuf, u64)>,
    decoding: Option<Receiver<Result<Clip, String>>>,
    decode_error: Option<String>,
    token: Arc<AtomicU64>,
}

impl AudioEngine {
    pub fn new(muted: bool) -> Self {
        Self { sink: None, sink_error: None, sink_tried: false, muted, clip: None, source: None, decoding: None, decode_error: None, token: Arc::default() }
    }

    /// Tell the engine which file the project uses now (`None`: no audio).
    /// A new or changed file is decoded on a thread; `wake` is called when it is done.
    pub fn set_file(&mut self, file: Option<PathBuf>, wake: impl Fn() + Send + 'static) {
        let Some(file) = file else {
            self.stop();
            self.clip = None;
            self.source = None;
            self.decoding = None;
            self.decode_error = None;
            return;
        };
        let stamp = crate::reload::stamp(std::slice::from_ref(&file));
        if self.source.as_ref() == Some(&(file.clone(), stamp)) {
            return;
        }
        self.stop();
        self.clip = None;
        self.decode_error = None;
        self.source = Some((file.clone(), stamp));
        let (tx, rx) = mpsc::channel();
        self.decoding = Some(rx);
        let spawned = std::thread::Builder::new().name("edits-audio-decode".into()).spawn(move || {
            let _ = tx.send(decode(&file));
            wake();
        });
        if let Err(e) = spawned {
            self.decoding = None;
            self.decode_error = Some(format!("cannot start the decoder thread: {e}"));
        }
        if !self.sink_tried && !self.muted {
            self.sink_tried = true;
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    sink.log_on_drop(false);
                    self.sink = Some(sink);
                }
                Err(e) => self.sink_error = Some(format!("no sound output device: {e}")),
            }
        }
    }

    /// Collect a finished decode. True when something changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.decoding else { return false };
        match rx.try_recv() {
            Ok(Ok(clip)) => self.clip = Some(Arc::new(clip)),
            Ok(Err(e)) => self.decode_error = Some(e),
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => self.decode_error = Some("the decoder thread stopped".into()),
        }
        self.decoding = None;
        true
    }

    /// No decode is running: the waveform is as complete as it will get.
    pub fn settled(&self) -> bool {
        self.decoding.is_none()
    }

    pub fn clip(&self) -> Option<&Arc<Clip>> {
        self.clip.as_ref()
    }

    /// Start playing from `seconds` into the audio file, replacing whatever plays.
    pub fn play(&mut self, seconds: f64, gain: f64) {
        self.stop();
        let (Some(sink), Some(clip)) = (&self.sink, &self.clip) else { return };
        let mine = self.token.load(Ordering::Relaxed);
        sink.mixer().add(ClipSource { clip: clip.clone(), pos: start_sample(seconds), gain: gain as f32, token: self.token.clone(), mine });
    }

    /// Stop whatever plays. Never blocks.
    pub fn stop(&mut self) {
        self.token.fetch_add(1, Ordering::Relaxed);
    }

    pub fn status(&self) -> AudioStatus {
        if self.source.is_none() {
            return AudioStatus::None;
        }
        if let Some(e) = &self.decode_error {
            return AudioStatus::Silent(e.clone());
        }
        if self.decoding.is_some() {
            return AudioStatus::Decoding;
        }
        if self.muted {
            return AudioStatus::Silent("sound is switched off".into());
        }
        if let Some(e) = &self.sink_error {
            return AudioStatus::Silent(e.clone());
        }
        match &self.clip {
            Some(c) => AudioStatus::Ready { seconds: c.seconds() },
            None => AudioStatus::Silent("not decoded".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_take_min_and_max_over_both_channels() {
        // Two frames per peak, stereo: four samples per bucket.
        let samples = [0.1, -0.2, 0.5, 0.0, -0.9, 0.3, 0.2, 0.1, 0.7, 0.7];
        let p = peaks(&samples, 2, 2);
        assert_eq!(p, vec![[-0.2, 0.5], [-0.9, 0.3], [0.7, 0.7]]);
        assert!(peaks(&[], 2, 480).is_empty());
    }

    #[test]
    fn peak_ranges_follow_the_file() {
        // One second of peaks: bucket i spans i*10 ms and holds ±i/100.
        let p: Vec<[f32; 2]> = (0..100).map(|i| [-(i as f32) / 100.0, i as f32 / 100.0]).collect();
        assert_eq!(peak_range(&p, 0.0, 0.01), Some((0.0, 0.0)));
        assert_eq!(peak_range(&p, 0.20, 0.30), Some((-0.29, 0.29)));
        // A column narrower than a bucket still gets that bucket.
        assert_eq!(peak_range(&p, 0.501, 0.502), Some((-0.5, 0.5)));
        // Before the file starts and after it ends there is nothing to draw.
        assert_eq!(peak_range(&p, -0.5, -0.1), None);
        assert_eq!(peak_range(&p, 1.0, 1.2), None);
        // A range that straddles the start is cut at 0.
        assert_eq!(peak_range(&p, -0.5, 0.02), Some((-0.01, 0.01)));
        assert_eq!(peak_range(&[], 0.0, 1.0), None);
    }

    #[test]
    fn playback_starts_on_a_frame_boundary() {
        assert_eq!(start_sample(0.0), 0);
        assert_eq!(start_sample(1.0), 96_000);
        assert_eq!(start_sample(0.48 + 1.44), 2 * 92_160);
        assert_eq!(start_sample(-0.5), -48_000);
        assert_eq!(start_sample(0.123_456) % 2, 0, "never starts on the right channel");
    }

    #[test]
    fn a_source_plays_silence_then_the_clip_and_stops_on_demand() {
        let clip = Arc::new(Clip::from_samples(vec![0.5, -0.5, 0.25, -0.25]));
        let token = Arc::new(AtomicU64::new(7));
        let src = ClipSource { clip: clip.clone(), pos: -2, gain: 2.0, token: token.clone(), mine: 7 };
        assert_eq!(src.collect::<Vec<f32>>(), vec![0.0, 0.0, 1.0, -1.0, 0.5, -0.5]);

        let mut src = ClipSource { clip, pos: 0, gain: 1.0, token: token.clone(), mine: 7 };
        assert_eq!(src.next(), Some(0.5));
        token.fetch_add(1, Ordering::Relaxed);
        // The frame in progress is finished, then it ends.
        assert_eq!(src.next(), Some(-0.5));
        assert_eq!(src.next(), None);
    }

    #[test]
    fn an_engine_without_a_file_or_a_device_stays_quiet() {
        let mut a = AudioEngine::new(true);
        assert_eq!(a.status(), AudioStatus::None);
        a.play(1.0, 1.0);
        a.stop();
        assert!(a.settled());
        assert!(!a.poll());

        // A file that cannot be decoded is reported, not fatal.
        a.set_file(Some(PathBuf::from("/nonexistent/edits-gui-test.wav")), || {});
        for _ in 0..200 {
            if a.poll() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(a.settled());
        assert!(matches!(a.status(), AudioStatus::Silent(_)), "{:?}", a.status());
        a.play(0.0, 1.0);
        a.set_file(None, || {});
        assert_eq!(a.status(), AudioStatus::None);
    }
}
