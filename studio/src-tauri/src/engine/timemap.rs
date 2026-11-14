// Ported from `crates/edits-gui/src/timemap.rs` (the egui window), unchanged but for
// imports, so Edits Studio and the old window edit projects the same way.

//! Frames, seconds and project time (beats or seconds), and the ruler.
//!
//! Everything here is plain arithmetic with no window, so it is unit tested.
//! The conversions are the ones `edits_core::Project` uses: a frame starts at
//! `frame / fps` seconds, and in a beats project beat 0 is at `beat_offset`
//! seconds.

use edits_core::{Project, TimeBase};

/// The time layout of one project.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeMap {
    pub fps: f64,
    pub frames: u64,
    /// Beats per minute when the project counts in beats.
    pub bpm: Option<f64>,
    /// Seconds from the start of the project to beat 0.
    pub beat_offset: f64,
    /// Length of the project in project units.
    pub duration: f64,
}

impl TimeMap {
    pub fn of(p: &Project) -> Self {
        let s = &p.settings;
        let bpm = match (s.time_base, s.bpm) {
            (TimeBase::Beats, Some(b)) if b > 0.0 => Some(b),
            _ => None,
        };
        Self {
            fps: s.fps,
            frames: p.frame_count(),
            bpm,
            beat_offset: s.beat_offset,
            duration: s.duration,
        }
    }

    pub fn uses_beats(&self) -> bool {
        self.bpm.is_some()
    }

    /// Name of the project's time unit, for labels.
    pub fn unit(&self) -> &'static str {
        if self.uses_beats() {
            "beats"
        } else {
            "s"
        }
    }

    pub fn last_frame(&self) -> u64 {
        self.frames.saturating_sub(1)
    }

    /// Project time to seconds from the start of the project.
    pub fn to_seconds(&self, t: f64) -> f64 {
        match self.bpm {
            Some(bpm) => self.beat_offset + t * 60.0 / bpm,
            None => t,
        }
    }

    /// Seconds from the start of the project to project time.
    pub fn from_seconds(&self, s: f64) -> f64 {
        match self.bpm {
            Some(bpm) => (s - self.beat_offset) * bpm / 60.0,
            None => s,
        }
    }

    /// Seconds at which `frame` starts.
    pub fn frame_seconds(&self, frame: u64) -> f64 {
        frame as f64 / self.fps
    }

    /// Project time at which `frame` starts (what the evaluator is given).
    pub fn frame_time(&self, frame: u64) -> f64 {
        self.from_seconds(self.frame_seconds(frame))
    }

    /// Project time of the first frame: 0, or negative when beat 0 is late.
    pub fn start_time(&self) -> f64 {
        self.from_seconds(0.0)
    }

    /// Project time at the end of the last frame.
    pub fn end_time(&self) -> f64 {
        self.from_seconds(self.frames as f64 / self.fps)
    }

    /// The frame on screen at project time `t`, kept inside the project.
    pub fn frame_at(&self, t: f64) -> u64 {
        let f = (self.to_seconds(t) * self.fps + 1e-6).floor();
        (f.max(0.0) as u64).min(self.last_frame())
    }

    /// The first frame that starts at or after project time `t` (what a cut
    /// at `t` looks like), kept inside the project.
    pub fn first_frame_from(&self, t: f64) -> u64 {
        let f = (self.to_seconds(t) * self.fps - 1e-6).ceil();
        (f.max(0.0) as u64).min(self.last_frame())
    }

    /// Jump one beat (one second in a seconds project) forward or back from
    /// `frame`: to the first frame of the next whole unit, or of the nearest
    /// whole unit that starts before `frame`.
    pub fn jump_unit(&self, frame: u64, forward: bool) -> u64 {
        let frame = frame.min(self.last_frame());
        let t = self.frame_time(frame);
        if forward {
            let mut unit = (t + 1e-6).floor() + 1.0;
            // A beat shorter than a frame could land on the same frame.
            for _ in 0..4096 {
                let f = self.first_frame_from(unit);
                if f > frame || f == self.last_frame() {
                    return f;
                }
                unit += 1.0;
            }
            self.last_frame()
        } else {
            let mut unit = (t - 1e-6).ceil();
            for _ in 0..4096 {
                let f = self.first_frame_from(unit);
                if f < frame || self.to_seconds(unit) <= 0.0 {
                    return f.min(frame);
                }
                unit -= 1.0;
            }
            0
        }
    }
}
