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
        Self { fps: s.fps, frames: p.frame_count(), bpm, beat_offset: s.beat_offset, duration: s.duration }
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

// ---- the horizontal view of the timeline -------------------------------------

/// Which part of the project the timeline shows: project time `start` is at
/// the left edge and one project unit is `px_per_unit` points wide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub start: f64,
    pub px_per_unit: f64,
}

impl View {
    /// Margin, in points, kept on both sides when the whole project is shown.
    pub const PAD: f64 = 8.0;

    /// Show `[t0, t1]` in `width` points.
    pub fn fit(t0: f64, t1: f64, width: f64) -> Self {
        let span = (t1 - t0).max(1e-6);
        let px = ((width - 2.0 * Self::PAD) / span).max(1e-3);
        Self { start: t0 - Self::PAD / px, px_per_unit: px }
    }

    /// Horizontal offset from the left edge of the time area.
    pub fn x_of(&self, t: f64) -> f64 {
        (t - self.start) * self.px_per_unit
    }

    pub fn t_at(&self, x: f64) -> f64 {
        self.start + x / self.px_per_unit
    }

    pub fn end(&self, width: f64) -> f64 {
        self.t_at(width)
    }

    /// Zoom by `factor`, keeping the time under offset `x` where it is.
    pub fn zoom_about(&mut self, factor: f64, x: f64) {
        let t = self.t_at(x);
        self.px_per_unit = (self.px_per_unit * factor).clamp(1e-3, 1e5);
        self.start = t - x / self.px_per_unit;
    }

    /// Keep the view inside `[t0, t1]` (plus the margin) and never zoomed
    /// out further than the whole project.
    pub fn clamp(&mut self, t0: f64, t1: f64, width: f64) {
        let fit = Self::fit(t0, t1, width);
        if self.px_per_unit <= fit.px_per_unit {
            *self = fit;
            return;
        }
        let pad = Self::PAD / self.px_per_unit;
        let lo = t0 - pad;
        let hi = t1 + pad - width / self.px_per_unit;
        self.start = self.start.clamp(lo, hi.max(lo));
    }

    /// Scroll so that `t` is visible, a page at a time (used while playing).
    pub fn follow(&mut self, t: f64, width: f64) {
        let x = self.x_of(t);
        if x < 0.0 || x > width {
            self.start = t - Self::PAD / self.px_per_unit;
        }
    }
}

// ---- ruler ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickLevel {
    /// A bar line (every four beats); a whole second in a seconds project.
    Bar,
    /// A whole beat; a labelled fraction of a second in a seconds project.
    Beat,
    /// A subdivision: a short line in the ruler only.
    Sub,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub t: f64,
    pub level: TickLevel,
    pub label: Option<String>,
}

/// Beats per bar. The format has no time signature; the grid assumes 4/4.
pub const BEATS_PER_BAR: f64 = 4.0;

const MIN_LINE_PX: f64 = 7.0;
const MIN_LABEL_PX: f64 = 46.0;

fn pick_step(candidates: &[f64], px_per_unit: f64, min_px: f64) -> f64 {
    for &c in candidates {
        if c * px_per_unit >= min_px {
            return c;
        }
    }
    // Zoomed out beyond the table: keep doubling.
    let mut c = *candidates.last().unwrap_or(&1.0);
    while c * px_per_unit < min_px && c < 1e9 {
        c *= 2.0;
    }
    c
}

fn is_multiple(t: f64, step: f64) -> bool {
    let q = t / step;
    (q - q.round()).abs() < 1e-6
}

fn label_text(t: f64) -> String {
    let r = (t * 100.0).round() / 100.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{}", r.round() as i64)
    } else {
        let s = format!("{r:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// The grid lines of the ruler between project times `from` and `to`.
///
/// In a beats project lines fall on beats (and on halves and quarters when
/// zoomed in, or only on every 2nd, 4th, 8th beat when zoomed out); a bar
/// is four beats. In a seconds project lines fall on round numbers of seconds.
/// Labels are thinned so they never overlap.
pub fn ruler_ticks(from: f64, to: f64, px_per_unit: f64, beats: bool) -> Vec<Tick> {
    if !(to - from).is_finite() || to <= from || !px_per_unit.is_finite() || px_per_unit <= 0.0 {
        return Vec::new();
    }
    let (line_step, label_step) = if beats {
        const STEPS: [f64; 8] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0];
        let line = pick_step(&STEPS, px_per_unit, MIN_LINE_PX);
        (line, pick_step(&STEPS, px_per_unit, MIN_LABEL_PX).max(line))
    } else {
        const STEPS: [f64; 14] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 60.0, 120.0];
        let line = pick_step(&STEPS, px_per_unit, MIN_LINE_PX);
        (line, pick_step(&STEPS, px_per_unit, MIN_LABEL_PX).max(line))
    };
    let first = (from / line_step - 1e-9).ceil() as i64;
    let last = (to / line_step + 1e-9).floor() as i64;
    let mut out = Vec::with_capacity((last - first + 1).max(0) as usize);
    for i in first..=last {
        let t = i as f64 * line_step;
        let labelled = is_multiple(t, label_step);
        let level = if beats {
            if is_multiple(t, BEATS_PER_BAR) {
                TickLevel::Bar
            } else if is_multiple(t, 1.0) {
                TickLevel::Beat
            } else {
                TickLevel::Sub
            }
        } else if is_multiple(t, label_step.max(1.0)) {
            // Whole seconds (or the labelled step, when that is coarser).
            TickLevel::Bar
        } else if labelled {
            TickLevel::Beat
        } else {
            TickLevel::Sub
        };
        out.push(Tick { t, level, label: labelled.then(|| label_text(t)) });
    }
    out
}

/// `1.48` → `"1.480"`: seconds with millisecond digits.
pub fn fmt_seconds(s: f64) -> String {
    format!("{s:.3}")
}

/// A clock reading for the transport: `00:01.48` (minutes, seconds, hundredths).
pub fn fmt_clock(s: f64) -> String {
    let cs = (s.max(0.0) * 100.0 + 1e-6).floor() as u64;
    format!("{:02}:{:02}.{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beats() -> TimeMap {
        // 120 BPM: a beat is 0.5 s, 12.5 frames at 25 fps.
        TimeMap { fps: 25.0, frames: 200, bpm: Some(120.0), beat_offset: 0.0, duration: 16.0 }
    }

    fn seconds() -> TimeMap {
        TimeMap { fps: 30.0, frames: 90, bpm: None, beat_offset: 0.0, duration: 3.0 }
    }

    #[test]
    fn matches_the_project_conversions() {
        let p = Project::from_json(
            r#"{"format":"edits.project/1",
                "settings":{"width":64,"height":64,"fps":25,"duration":16,"time_base":"beats","bpm":123,"beat_offset":0.48},
                "layers":[]}"#,
        )
        .unwrap();
        let tm = TimeMap::of(&p);
        assert_eq!(tm.frames, p.frame_count());
        for f in [0u64, 1, 36, 37, 100] {
            assert!((tm.frame_time(f) - p.frame_time(f)).abs() < 1e-12);
        }
        assert!((tm.to_seconds(3.0) - p.to_seconds(3.0)).abs() < 1e-12);
        // Beat 0 is 0.48 s in, so the first frame is before it.
        assert!(tm.start_time() < 0.0);
        assert!((tm.end_time() - tm.duration).abs() < tm.frame_time(1) - tm.frame_time(0));
    }

    #[test]
    fn frame_time_and_back() {
        let tm = beats();
        assert!((tm.frame_seconds(25) - 1.0).abs() < 1e-12);
        assert!((tm.frame_time(25) - 2.0).abs() < 1e-12);
        for f in 0..tm.frames {
            assert_eq!(tm.frame_at(tm.frame_time(f)), f);
            assert_eq!(tm.first_frame_from(tm.frame_time(f)), f);
        }
        // Beat 3 is 1.5 s = frame 37.5: inside frame 37; a cut there starts on 38.
        assert_eq!(tm.frame_at(3.0), 37);
        assert_eq!(tm.first_frame_from(3.0), 38);
        assert_eq!(tm.frame_at(-5.0), 0);
        assert_eq!(tm.frame_at(1e9), 199);
    }

    #[test]
    fn seconds_projects_count_in_seconds() {
        let tm = seconds();
        assert!(!tm.uses_beats());
        assert_eq!(tm.unit(), "s");
        assert_eq!(tm.frame_at(1.0), 30);
        assert!((tm.frame_time(45) - 1.5).abs() < 1e-12);
        assert_eq!(tm.jump_unit(45, true), 60);
        assert_eq!(tm.jump_unit(45, false), 30);
        assert_eq!(tm.jump_unit(30, false), 0);
    }

    #[test]
    fn beat_jumps_land_on_beat_frames() {
        let tm = beats();
        // From the start: beat 1 = 0.5 s = frame 12.5 → 13.
        assert_eq!(tm.jump_unit(0, true), 13);
        // Beat 2 = 1.0 s = frame 25 exactly.
        assert_eq!(tm.jump_unit(13, true), 25);
        assert_eq!(tm.jump_unit(25, true), 38);
        // Back from the middle of a beat goes to its start, then one more.
        assert_eq!(tm.jump_unit(30, false), 25);
        assert_eq!(tm.jump_unit(25, false), 13);
        assert_eq!(tm.jump_unit(13, false), 0);
        assert_eq!(tm.jump_unit(0, false), 0);
        // And it stops at the ends.
        assert_eq!(tm.jump_unit(199, true), 199);
        assert_eq!(tm.jump_unit(195, true), 199);
    }

    #[test]
    fn beat_jumps_with_a_late_first_beat() {
        let tm = TimeMap { beat_offset: 0.48, ..beats() };
        // Frame 0 is before beat 0 (at 0.48 s = frame 12).
        assert_eq!(tm.jump_unit(0, true), 12);
        assert_eq!(tm.jump_unit(12, false), 0);
        assert_eq!(tm.jump_unit(5, false), 0);
    }

    #[test]
    fn view_maps_and_zooms_about_a_point() {
        let mut v = View::fit(0.0, 16.0, 816.0);
        assert!((v.px_per_unit - 50.0).abs() < 1e-9);
        assert!((v.x_of(0.0) - View::PAD).abs() < 1e-9);
        assert!((v.x_of(16.0) - (816.0 - View::PAD)).abs() < 1e-9);
        assert!((v.t_at(v.x_of(7.25)) - 7.25).abs() < 1e-9);

        let x = v.x_of(4.0);
        v.zoom_about(2.0, x);
        assert!((v.px_per_unit - 100.0).abs() < 1e-9);
        assert!((v.x_of(4.0) - x).abs() < 1e-9, "the time under the pointer stays put");
    }

    #[test]
    fn view_is_kept_inside_the_project() {
        let mut v = View { start: -100.0, px_per_unit: 100.0 };
        v.clamp(0.0, 16.0, 800.0);
        assert!((v.x_of(0.0) - View::PAD).abs() < 1e-9);
        v.start = 1000.0;
        v.clamp(0.0, 16.0, 800.0);
        assert!((v.x_of(16.0) - (800.0 - View::PAD)).abs() < 1e-9);
        // Zooming out past the whole project snaps to the fitted view.
        let mut v = View { start: 3.0, px_per_unit: 1.0 };
        v.clamp(0.0, 16.0, 800.0);
        assert_eq!(v, View::fit(0.0, 16.0, 800.0));
    }

    #[test]
    fn view_follows_the_playhead_by_pages() {
        let mut v = View { start: 0.0, px_per_unit: 100.0 };
        v.follow(3.0, 800.0);
        assert_eq!(v.start, 0.0, "still visible: no scrolling");
        v.follow(8.5, 800.0);
        assert!((v.x_of(8.5) - View::PAD).abs() < 1e-9);
        v.follow(1.0, 800.0);
        assert!((v.x_of(1.0) - View::PAD).abs() < 1e-9);
    }

    #[test]
    fn beat_ruler_marks_bars_beats_and_subdivisions() {
        let at = |ticks: &[Tick], t: f64| ticks.iter().find(|k| k.t == t).cloned().unwrap();
        let labels = |ticks: &[Tick]| -> Vec<String> { ticks.iter().filter_map(|t| t.label.clone()).collect() };

        // 50 px per beat: a short line every quarter beat, a number on every beat.
        let ticks = ruler_ticks(0.0, 8.0, 50.0, true);
        assert_eq!(ticks.len(), 33);
        assert_eq!(at(&ticks, 0.0), Tick { t: 0.0, level: TickLevel::Bar, label: Some("0".into()) });
        assert_eq!(at(&ticks, 0.25), Tick { t: 0.25, level: TickLevel::Sub, label: None });
        assert_eq!(at(&ticks, 1.0).level, TickLevel::Beat);
        assert_eq!(at(&ticks, 4.0).level, TickLevel::Bar);
        assert_eq!(at(&ticks, 8.0).level, TickLevel::Bar);
        assert_eq!(labels(&ticks), vec!["0", "1", "2", "3", "4", "5", "6", "7", "8"]);

        // 100 px per beat: numbers on half beats too.
        let ticks = ruler_ticks(0.0, 1.0, 100.0, true);
        let ts: Vec<f64> = ticks.iter().map(|t| t.t).collect();
        assert_eq!(ts, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
        assert_eq!(labels(&ticks), vec!["0", "0.5", "1"]);
        assert_eq!(at(&ticks, 0.5).level, TickLevel::Sub);

        // 12 px per beat: every beat still has a line, numbers only on bars.
        let ticks = ruler_ticks(0.0, 16.0, 12.0, true);
        assert_eq!(ticks.len(), 17);
        assert_eq!(labels(&ticks), vec!["0", "4", "8", "12", "16"]);

        // 2 px per beat: only bars are drawn at all.
        let ticks = ruler_ticks(0.0, 64.0, 2.0, true);
        assert!(ticks.iter().all(|t| t.level == TickLevel::Bar));
        assert!(ticks.windows(2).all(|w| (w[1].t - w[0].t) * 2.0 >= MIN_LINE_PX));
    }

    #[test]
    fn rulers_cover_negative_time_and_only_the_visible_range() {
        // A project whose first beat is late starts before beat 0.
        let ticks = ruler_ticks(-1.3, 2.2, 10.0, true);
        let ts: Vec<f64> = ticks.iter().map(|t| t.t).collect();
        assert_eq!(ts, vec![-1.0, 0.0, 1.0, 2.0]);
        let ticks = ruler_ticks(-1.3, 0.2, 50.0, true);
        assert_eq!(ticks.iter().find(|t| t.t == -1.0).unwrap().label.as_deref(), Some("-1"));
        assert!(ticks.iter().all(|t| t.t >= -1.3 && t.t <= 0.2));
        assert!(ruler_ticks(5.0, 5.0, 50.0, true).is_empty());
        assert!(ruler_ticks(0.0, 5.0, 0.0, true).is_empty());
    }

    #[test]
    fn second_ruler_uses_round_steps() {
        // 100 px per second: lines every 0.1 s, labels every 0.5 s.
        let ticks = ruler_ticks(0.0, 1.0, 100.0, false);
        assert_eq!(ticks.len(), 11);
        let labels: Vec<&str> = ticks.iter().filter_map(|t| t.label.as_deref()).collect();
        assert_eq!(labels, vec!["0", "0.5", "1"]);
        assert_eq!(ticks[0].level, TickLevel::Bar);
        assert_eq!(ticks[5].level, TickLevel::Beat, "a labelled half second");
        assert_eq!(ticks[1].level, TickLevel::Sub);
        assert_eq!(ticks[10].level, TickLevel::Bar);
        // Labels never crowd, at any zoom.
        for px in [0.3, 1.0, 7.0, 33.0, 250.0, 4000.0] {
            let ticks = ruler_ticks(0.0, 2000.0 / px, px, false);
            let labelled: Vec<f64> = ticks.iter().filter(|t| t.label.is_some()).map(|t| t.t).collect();
            assert!(labelled.windows(2).all(|w| (w[1] - w[0]) * px >= MIN_LABEL_PX - 1e-6), "px {px}");
        }
    }

    #[test]
    fn clock_readings() {
        assert_eq!(fmt_clock(0.0), "00:00.00");
        assert_eq!(fmt_clock(1.48), "00:01.48");
        assert_eq!(fmt_clock(61.999), "01:01.99");
        assert_eq!(fmt_clock(-3.0), "00:00.00");
    }
}
