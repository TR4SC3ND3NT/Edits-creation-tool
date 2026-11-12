//! The playback clock, and which frame to ask the renderer for.
//!
//! Playback follows real time: the playhead is where the clock says it is,
//! whether or not the picture for that frame exists yet. The preview shows the
//! newest finished frame at or before the playhead, and the renderer is asked
//! for a frame a little ahead of the playhead, by as much as a frame takes to
//! render, so that the picture is ready when the clock gets there. Frames in
//! between are skipped instead of slowing playback down.

/// A running playback: it started at `start_frame` and has been running for
/// the `elapsed` seconds given to [`PlayClock::at`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayClock {
    pub fps: f64,
    pub frames: u64,
    pub looping: bool,
    pub start_frame: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockPos {
    pub frame: u64,
    /// Seconds from the start of the project (within the current lap).
    pub seconds: f64,
    /// How many times playback has wrapped to the start.
    pub laps: u64,
    /// The end was reached and the clock does not loop.
    pub ended: bool,
}

impl PlayClock {
    pub fn at(&self, elapsed: f64) -> ClockPos {
        let frames = self.frames.max(1);
        let length = frames as f64 / self.fps;
        let start = self.start_frame.min(frames - 1) as f64 / self.fps;
        let s = start + elapsed.max(0.0);
        if s < length {
            return ClockPos { frame: frame_of(s, self.fps, frames), seconds: s, laps: 0, ended: false };
        }
        if !self.looping {
            return ClockPos { frame: frames - 1, seconds: (frames - 1) as f64 / self.fps, laps: 0, ended: true };
        }
        let laps = (s / length).floor();
        let within = s - laps * length;
        ClockPos { frame: frame_of(within, self.fps, frames), seconds: within, laps: laps as u64, ended: false }
    }
}

fn frame_of(seconds: f64, fps: f64, frames: u64) -> u64 {
    ((seconds * fps + 1e-6).floor().max(0.0) as u64).min(frames - 1)
}

/// How many frames ahead of the playhead to render, given how long a frame
/// takes: a 100 ms render at 25 fps is asked for 3 frames early. At most one
/// second.
pub fn lead_frames(render_ms: Option<f64>, fps: f64) -> u64 {
    let Some(ms) = render_ms else { return 1 };
    let lead = (ms.max(0.0) / 1000.0 * fps).ceil();
    (lead as u64).clamp(1, fps.ceil().max(1.0) as u64)
}

/// The frame to render next while playing: the first frame from
/// `playhead + lead` on, within `window` frames, that is not cached yet.
/// `None` when all of them are (the renderer can rest).
pub fn next_request(playhead: u64, lead: u64, frames: u64, looping: bool, window: u64, cached: impl Fn(u64) -> bool) -> Option<u64> {
    let frames = frames.max(1);
    // The frame under the playhead comes first if a faster setting can still catch it.
    if lead <= 1 && !cached(playhead.min(frames - 1)) {
        return Some(playhead.min(frames - 1));
    }
    for i in 0..window.min(frames) {
        let f = playhead + lead + i;
        let f = if f < frames {
            f
        } else if looping {
            f % frames
        } else {
            // Past the end: the last frame is where a non-looping clock stops.
            return (!cached(frames - 1)).then_some(frames - 1);
        };
        if !cached(f) {
            return Some(f);
        }
    }
    None
}

/// The frame to show while playing: the newest cached frame at or before the
/// playhead, looking back at most `max_back` frames. `None` keeps whatever is
/// on screen.
pub fn newest_ready(playhead: u64, max_back: u64, cached: impl Fn(u64) -> bool) -> Option<u64> {
    (0..=max_back.min(playhead)).map(|d| playhead - d).find(|f| cached(*f))
}

/// A running average of render times, so one slow frame does not swing the lead.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderAverage {
    value: Option<f64>,
}

impl RenderAverage {
    pub fn push(&mut self, ms: f64) {
        self.value = Some(match self.value {
            Some(v) => v * 0.7 + ms * 0.3,
            None => ms,
        });
    }
    pub fn get(&self) -> Option<f64> {
        self.value
    }
    pub fn reset(&mut self) {
        self.value = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn clock(looping: bool, start_frame: u64) -> PlayClock {
        PlayClock { fps: 25.0, frames: 100, looping, start_frame }
    }

    #[test]
    fn follows_real_time() {
        let c = clock(false, 0);
        assert_eq!(c.at(0.0).frame, 0);
        assert_eq!(c.at(0.039).frame, 0);
        assert_eq!(c.at(0.04).frame, 1);
        assert_eq!(c.at(1.0).frame, 25);
        assert_eq!(c.at(2.02).frame, 50);
        // Starting somewhere else only shifts it.
        assert_eq!(clock(false, 40).at(1.0).frame, 65);
        assert!((clock(false, 40).at(1.0).seconds - 2.6).abs() < 1e-9);
    }

    #[test]
    fn a_slow_ui_skips_frames_instead_of_slowing_down() {
        // The window only repaints every 150 ms: the playhead still covers
        // one second of project per second of wall clock.
        let c = clock(false, 0);
        let seen: Vec<u64> = (0..=6).map(|i| c.at(i as f64 * 0.15).frame).collect();
        assert_eq!(seen, vec![0, 3, 7, 11, 15, 18, 22]);
    }

    #[test]
    fn stops_at_the_end_without_loop() {
        let c = clock(false, 90);
        let p = c.at(0.36);
        assert_eq!((p.frame, p.ended), (99, false));
        let p = c.at(0.4);
        assert_eq!((p.frame, p.ended), (99, true));
        assert!(c.at(100.0).ended);
    }

    #[test]
    fn wraps_with_loop_and_counts_laps() {
        let c = clock(true, 90);
        let p = c.at(0.4);
        assert_eq!((p.frame, p.laps, p.ended), (0, 1, false));
        assert!(p.seconds.abs() < 1e-9);
        let p = c.at(0.4 + 4.0 + 0.08);
        assert_eq!((p.frame, p.laps), (2, 2));
        let p = c.at(0.2);
        assert_eq!((p.frame, p.laps), (95, 0));
    }

    #[test]
    fn a_start_frame_past_the_end_is_safe() {
        let c = PlayClock { fps: 25.0, frames: 10, looping: true, start_frame: 500 };
        assert!(c.at(0.0).frame < 10);
        let c = PlayClock { fps: 25.0, frames: 0, looping: false, start_frame: 0 };
        assert_eq!(c.at(3.0).frame, 0);
    }

    #[test]
    fn lead_matches_render_time() {
        assert_eq!(lead_frames(None, 25.0), 1);
        assert_eq!(lead_frames(Some(10.0), 25.0), 1);
        assert_eq!(lead_frames(Some(40.0), 25.0), 1);
        assert_eq!(lead_frames(Some(100.0), 25.0), 3);
        assert_eq!(lead_frames(Some(400.0), 25.0), 10);
        assert_eq!(lead_frames(Some(60_000.0), 25.0), 25, "never more than a second");
    }

    #[test]
    fn requests_skip_ahead_and_fill_gaps() {
        let none = |_: u64| false;
        // Nothing cached, fast renderer: the frame under the playhead.
        assert_eq!(next_request(10, 1, 100, false, 50, none), Some(10));
        // Slow renderer: skip to where the playhead will be.
        assert_eq!(next_request(10, 4, 100, false, 50, none), Some(14));
        // Cached frames are passed over: the renderer fills the gaps of the last pass.
        let cached: HashSet<u64> = [10, 11, 12, 14].into_iter().collect();
        assert_eq!(next_request(10, 1, 100, false, 50, |f| cached.contains(&f)), Some(13));
        // Everything in the window is there: nothing to do.
        assert_eq!(next_request(10, 1, 100, false, 2, |f| cached.contains(&f)), None);
        assert_eq!(next_request(0, 1, 5, true, 50, |_| true), None);
    }

    #[test]
    fn requests_wrap_only_when_looping() {
        let none = |_: u64| false;
        assert_eq!(next_request(98, 4, 100, true, 50, none), Some(2));
        assert_eq!(next_request(98, 4, 100, false, 50, none), Some(99));
        assert_eq!(next_request(98, 4, 100, false, 50, |f| f == 99), None);
        // A window longer than the project does not go round twice.
        let calls = std::cell::Cell::new(0);
        next_request(0, 1, 10, true, 1000, |_| {
            calls.set(calls.get() + 1);
            true
        });
        assert!(calls.get() <= 11);
    }

    #[test]
    fn shows_the_newest_finished_frame() {
        let cached: HashSet<u64> = [3, 7, 20].into_iter().collect();
        let has = |f: u64| cached.contains(&f);
        assert_eq!(newest_ready(7, 25, has), Some(7));
        assert_eq!(newest_ready(9, 25, has), Some(7));
        assert_eq!(newest_ready(19, 25, has), Some(7));
        assert_eq!(newest_ready(19, 5, has), None, "too old to be worth showing");
        assert_eq!(newest_ready(2, 25, has), None, "never looks before frame 0");
        // A frame rendered ahead is not shown early.
        assert_eq!(newest_ready(19, 25, has), Some(7));
        assert_eq!(newest_ready(20, 25, has), Some(20));
    }

    #[test]
    fn playback_with_a_slow_renderer_keeps_real_time() {
        // Simulate: 25 fps, each render takes 130 ms, the window repaints
        // every 20 ms. The playhead must reach frame 50 after 2 s whatever
        // happens, and what is shown must never be ahead of it.
        let clock = PlayClock { fps: 25.0, frames: 200, looping: false, start_frame: 0 };
        let mut cached: HashSet<u64> = HashSet::new();
        let mut busy: Option<(u64, f64)> = None; // (frame, finishes at)
        let mut avg = RenderAverage::default();
        let mut shown = Vec::new();
        let mut t = 0.0;
        while t <= 2.0 + 1e-9 {
            if let Some((f, done)) = busy {
                if t >= done {
                    cached.insert(f);
                    avg.push(130.0);
                    busy = None;
                }
            }
            let playhead = clock.at(t).frame;
            if busy.is_none() {
                let lead = lead_frames(avg.get(), clock.fps);
                if let Some(f) = next_request(playhead, lead, clock.frames, false, 50, |f| cached.contains(&f)) {
                    busy = Some((f, t + 0.13));
                }
            }
            if let Some(f) = newest_ready(playhead, 25, |f| cached.contains(&f)) {
                assert!(f <= playhead);
                if shown.last() != Some(&f) {
                    shown.push(f);
                }
            }
            t += 0.02;
        }
        assert_eq!(clock.at(2.0).frame, 50);
        // Roughly one picture per 130 ms, each close to the playhead when shown.
        assert!(shown.len() >= 12 && shown.len() <= 16, "{shown:?}");
        assert!(shown.windows(2).all(|w| w[1] > w[0]));
        assert!(*shown.last().unwrap() >= 44, "{shown:?}");
    }

    #[test]
    fn average_smooths_render_times() {
        let mut a = RenderAverage::default();
        assert_eq!(a.get(), None);
        a.push(100.0);
        assert_eq!(a.get(), Some(100.0));
        a.push(200.0);
        assert!((a.get().unwrap() - 130.0).abs() < 1e-9);
        a.reset();
        assert_eq!(a.get(), None);
    }
}
