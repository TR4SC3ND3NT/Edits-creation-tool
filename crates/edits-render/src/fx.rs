//! Effect chains: every kind in `edits_core::effects::EFFECTS`.
//!
//! Sizes given in "% of height" become pixels of the frame being rendered, so
//! a chain looks the same at any `scale`. Effects whose strength is zero are
//! skipped. Neighbouring colour effects share one pass, and can also share it
//! with a glow's final add or with the layer's blend (see `compose.rs`).

use crate::compose::{self, MAX_OPS};
use crate::renderer::{Pipe, Renderer, Sink, Source, Tex};
use anyhow::{anyhow, bail, Result};
use edits_core::ResolvedEffect;

/// Blur at reduced resolution until sigma is at most this many texels.
const MAX_SIGMA: f32 = 2.5;
/// Taps per streak pass; passes multiply, so three passes sample 64 points.
const STREAK_TAPS: f32 = 4.0;
/// Add streak passes until the samples are at most this many pixels apart.
const STREAK_SPACING: f32 = 2.0;

/// A colour operation's uniforms: (code, a, b, c) and a second vector.
pub(crate) type ColorOp = [[f32; 4]; 2];

/// One colour effect as uniforms, or `None` when it would change nothing.
fn color_op(e: &ResolvedEffect, height: f32, frame: f32) -> Option<ColorOp> {
    let n = |name: &str| e.num(name) as f32;
    let rgb = |name: &str| {
        let v = e.vec(name, 3);
        [v[0] as f32, v[1] as f32, v[2] as f32, 0.0]
    };
    let zero = [0.0; 4];
    let op = match e.kind.as_str() {
        "exposure" => (n("stops") != 0.0).then(|| [[compose::EXPOSURE as f32, 2f32.powf(n("stops")), 0.0, 0.0], zero]),
        "fade" => (n("amount") > 0.0).then(|| [[compose::FADE as f32, n("amount").min(1.0), 0.0, 0.0], rgb("color")]),
        "brightness_contrast" => (n("brightness") != 0.0 || n("contrast") != 0.0).then(|| {
            let gain = ((n("contrast").clamp(-1.0, 0.999) + 1.0) * std::f32::consts::FRAC_PI_4).tan();
            [[compose::BRIGHTNESS_CONTRAST as f32, n("brightness"), gain, 0.0], zero]
        }),
        "saturation" => (n("amount") != 1.0).then(|| [[compose::SATURATION as f32, n("amount").max(0.0), 0.0, 0.0], zero]),
        "hue" => (n("degrees") % 360.0 != 0.0).then(|| [[compose::HUE as f32, n("degrees").to_radians(), 0.0, 0.0], zero]),
        "invert" => (n("amount") > 0.0).then(|| [[compose::INVERT as f32, n("amount").min(1.0), 0.0, 0.0], zero]),
        "threshold" => (n("amount") > 0.0).then(|| [[compose::THRESHOLD as f32, n("level"), n("softness").max(0.0), n("amount").min(1.0)], zero]),
        "posterize" => Some([[compose::POSTERIZE as f32, n("levels").round().max(2.0), 0.0, 0.0], zero]),
        "tint" => (n("amount") > 0.0).then(|| [[compose::TINT as f32, n("amount").min(1.0), 0.0, 0.0], rgb("color")]),
        "vignette" => (n("amount") != 0.0).then(|| [[compose::VIGNETTE as f32, n("amount"), n("radius"), n("softness")], zero]),
        "grain" => (n("amount") > 0.0).then(|| [[compose::GRAIN as f32, n("amount"), (n("size") * height / 1080.0).max(0.05), frame], zero]),
        _ => return None,
    };
    op
}

fn is_color(kind: &str) -> bool {
    matches!(kind, "exposure" | "fade" | "brightness_contrast" | "saturation" | "hue" | "invert" | "threshold" | "posterize" | "tint" | "vignette" | "grain")
}

/// One effect, resolved to numbers in pixels of the frame being rendered.
pub(crate) enum Step {
    Color(ColorOp),
    /// Sigma in pixels.
    Gaussian(f32),
    /// Mode (0 directional, 1 zoom, 2 spin), centre (uv), amount, longest streak in pixels.
    Streak(f32, [f32; 2], [f32; 2], f32),
    /// Threshold, sigma in pixels, colour x intensity.
    Glow(f32, f32, [f32; 3]),
    /// Shift (uv).
    RgbSplit([f32; 2]),
    /// 1 / scale, radians, mirror, offset in composition units.
    Transform(f32, f32, f32, [f32; 2]),
    Custom(wgpu::RenderPipeline, [f32; 8]),
}

impl Renderer {
    /// Turn resolved effects into the steps that will actually run for a
    /// `w` x `h` frame. Effects that would change nothing are left out, so an
    /// empty result means the chain can be skipped altogether.
    pub(crate) fn plan_effects(&self, layer: &str, effects: &[ResolvedEffect], w: f32, h: f32) -> Result<Vec<Step>> {
        let aspect = self.ctx.time[3];
        let frame = self.ctx.time[2];
        let mut steps = Vec::new();
        for e in effects {
            if is_color(&e.kind) {
                steps.extend(color_op(e, h, frame).map(Step::Color));
                continue;
            }
            let n = |name: &str| e.num(name) as f32;
            match e.kind.as_str() {
                "gaussian_blur" => {
                    let sigma = n("radius") / 100.0 * h;
                    if sigma >= 0.05 {
                        steps.push(Step::Gaussian(sigma));
                    }
                }
                "directional_blur" => {
                    let length = n("length").abs() / 100.0 * h;
                    let a = n("angle").to_radians();
                    if length >= 0.25 {
                        steps.push(Step::Streak(0.0, [0.5, 0.5], [a.cos() * length / w, a.sin() * length / h], length));
                    }
                }
                "zoom_blur" | "spin_blur" => {
                    let zoom = e.kind == "zoom_blur";
                    let amount = if zoom { n("strength") } else { n("angle").to_radians() };
                    let c = e.vec("center", 2);
                    let centre = [0.5 + c[0] as f32 / aspect, 0.5 + c[1] as f32];
                    // The longest streak is at the corner farthest from the centre.
                    let reach = ((centre[0].abs().max((1.0 - centre[0]).abs()) * w).powi(2) + (centre[1].abs().max((1.0 - centre[1]).abs()) * h).powi(2)).sqrt();
                    if amount != 0.0 {
                        steps.push(Step::Streak(if zoom { 1.0 } else { 2.0 }, centre, [amount, 0.0], amount.abs() * reach));
                    }
                }
                "glow" => {
                    let (k, color) = (n("intensity"), e.vec("color", 3));
                    if k > 0.0 {
                        steps.push(Step::Glow(n("threshold"), (n("radius") / 100.0 * h).max(1.0), [color[0] as f32 * k, color[1] as f32 * k, color[2] as f32 * k]));
                    }
                }
                "rgb_split" => {
                    let shift = n("amount") / 100.0 * h;
                    let a = n("angle").to_radians();
                    if shift != 0.0 {
                        steps.push(Step::RgbSplit([a.cos() * shift / w, a.sin() * shift / h]));
                    }
                }
                "transform2d" => {
                    let off = e.vec("offset", 2);
                    let (scale, rot) = (n("scale"), n("rotation"));
                    if scale != 1.0 || rot % 360.0 != 0.0 || off[0] != 0.0 || off[1] != 0.0 {
                        let inv = if scale < 0.0 { -1.0 } else { 1.0 } / scale.abs().max(1e-4);
                        steps.push(Step::Transform(inv, rot.to_radians(), if n("mirror") > 0.5 { 1.0 } else { 0.0 }, [off[0] as f32, off[1] as f32]));
                    }
                }
                "custom" => {
                    let scene = self.scene.as_ref().ok_or_else(|| anyhow!("no project prepared"))?;
                    let source = e.source.as_deref().ok_or_else(|| anyhow!("layer \"{layer}\": a custom effect needs \"source\""))?;
                    let pipeline = scene
                        .custom
                        .get(source)
                        .and_then(|key| self.custom.get(key))
                        .cloned()
                        .ok_or_else(|| anyhow!("layer \"{layer}\": custom effect \"{source}\" was not prepared"))?;
                    let mut p = [0.0; 8];
                    for (i, v) in p.iter_mut().enumerate() {
                        *v = n(&format!("p{i}"));
                    }
                    steps.push(Step::Custom(pipeline, p));
                }
                other => bail!("layer \"{layer}\": unknown effect \"{other}\" (run `edits effects` for the list)"),
            }
        }
        Ok(steps)
    }

    /// Run `steps` on `input` and return the result, or `None` when there is
    /// nothing to do. `input` is never released or written to.
    pub(crate) fn run_effects(&mut self, input: &Tex, steps: &[Step]) -> Option<Tex> {
        let aspect = self.ctx.time[3];
        let mut cur: Option<Tex> = None;
        let mut i = 0;
        while i < steps.len() {
            let s = cur.clone().unwrap_or_else(|| input.clone());
            let out = match &steps[i] {
                Step::Color(_) => {
                    let ops = color_run(&steps[i..]);
                    i += ops.len() - 1;
                    let out = self.acquire(s.w, s.h);
                    self.compose(&out, Source::Tex(&s), &ops, Sink::Plain);
                    out
                }
                Step::Gaussian(sigma) => self.gaussian(&s, *sigma),
                Step::Streak(mode, centre, amount, length) => self.streak(&s, *mode, *centre, *amount, *length),
                Step::Glow(threshold, sigma, color) => {
                    let half = self.acquire(s.w.div_ceil(2), s.h.div_ceil(2));
                    self.pass(Pipe::Bright, &half, false, &s, None, &[[*threshold, 0.0, 0.0, 0.0]]);
                    let blurred = self.gaussian(&half, sigma * 0.5);
                    self.release(half);
                    // Colour effects right after the glow share its final pass.
                    let ops = color_run(&steps[i + 1..]);
                    i += ops.len();
                    let out = self.acquire(s.w, s.h);
                    self.compose(&out, Source::Glow(&s, &blurred, *color), &ops, Sink::Plain);
                    self.release(blurred);
                    out
                }
                Step::RgbSplit(shift) => {
                    let out = self.acquire(s.w, s.h);
                    self.pass(Pipe::RgbSplit, &out, false, &s, None, &[[shift[0], shift[1], 0.0, 0.0]]);
                    out
                }
                Step::Transform(inv, radians, mirror, offset) => {
                    let out = self.acquire(s.w, s.h);
                    self.pass(Pipe::Transform2d, &out, false, &s, None, &[[*inv, *radians, *mirror, aspect], [offset[0], offset[1], 0.0, 0.0]]);
                    out
                }
                Step::Custom(pipeline, p) => {
                    let out = self.acquire(s.w, s.h);
                    self.pass_with(pipeline, &out, false, &s, None, &[[p[0], p[1], p[2], p[3]], [p[4], p[5], p[6], p[7]]]);
                    out
                }
            };
            if let Some(old) = cur.replace(out) {
                self.release(old);
            }
            i += 1;
        }
        cur
    }

    /// Gaussian blur with `sigma` in texels of `src`. Large blurs are done on
    /// a reduced copy: halve until sigma fits, blur, then double back up.
    fn gaussian(&mut self, src: &Tex, sigma: f32) -> Tex {
        let mut sizes = vec![(src.w, src.h)];
        let mut s = sigma;
        while s > MAX_SIGMA && sizes.len() < 9 {
            let (w, h) = sizes[sizes.len() - 1];
            if w.min(h) < 16 {
                break;
            }
            sizes.push((w.div_ceil(2), h.div_ceil(2)));
            s *= 0.5;
        }
        let one = [[1.0, 0.0, 0.0, 0.0]];
        let mut cur = src.clone();
        for &(w, h) in &sizes[1..] {
            let next = self.acquire(w, h);
            self.pass(Pipe::Copy, &next, false, &cur, None, &one);
            if cur.id != src.id {
                self.release(cur);
            }
            cur = next;
        }
        let taps = (((3.0 * s).ceil() as u32).max(2)).next_multiple_of(2) as f32;
        let (w, h) = (cur.w, cur.h);
        let horizontal = self.acquire(w, h);
        self.pass(Pipe::Blur, &horizontal, false, &cur, None, &[[1.0 / w as f32, 0.0, s, taps]]);
        if cur.id != src.id {
            self.release(cur);
        }
        let mut cur = self.acquire(w, h);
        self.pass(Pipe::Blur, &cur, false, &horizontal, None, &[[0.0, 1.0 / h as f32, s, taps]]);
        self.release(horizontal);
        for &(w, h) in sizes[..sizes.len() - 1].iter().rev() {
            let next = self.acquire(w, h);
            self.pass(Pipe::Copy, &next, false, &cur, None, &one);
            self.release(cur);
            cur = next;
        }
        cur
    }

    /// Directional (mode 0), zoom (1) or spin (2) blur. `length` is the
    /// longest streak in pixels and decides how many passes are needed: each
    /// pass takes `STREAK_TAPS` samples over a streak that is `STREAK_TAPS`
    /// times shorter than the one before, so `n` passes sample
    /// `STREAK_TAPS^n` evenly spaced points.
    fn streak(&mut self, src: &Tex, mode: f32, centre: [f32; 2], amount: [f32; 2], length: f32) -> Tex {
        let taps = STREAK_TAPS as usize;
        let mut passes = 1;
        while (taps as f32).powi(passes) < length / STREAK_SPACING && passes < 5 {
            passes += 1;
        }
        let aspect = src.w as f32 / src.h as f32;
        let mut cur = src.clone();
        let mut span = 1.0;
        for _ in 0..passes {
            let mut p = [[0.0f32; 4]; 18];
            p[0] = [STREAK_TAPS, aspect, 0.0, 0.0];
            p[1] = [centre[0], centre[1], 0.0, 0.0];
            for i in 0..taps {
                let t = ((i as f32 + 0.5) / STREAK_TAPS - 0.5) * span;
                (p[2 + i], p[10 + i]) = match mode as u32 {
                    0 => ([1.0, 0.0, 0.0, 1.0], [amount[0] * t, amount[1] * t, 0.0, 0.0]),
                    1 => {
                        let k = (amount[0] * t).exp();
                        ([k, 0.0, 0.0, k], [0.0; 4])
                    }
                    _ => {
                        let (sin, cos) = (amount[0] * t).sin_cos();
                        ([cos, -sin, sin, cos], [0.0; 4])
                    }
                };
            }
            let out = self.acquire(src.w, src.h);
            self.pass(Pipe::Streak, &out, false, &cur, None, &p);
            if cur.id != src.id {
                self.release(cur);
            }
            cur = out;
            span /= STREAK_TAPS;
        }
        cur
    }
}

/// The colour operations at the start of `steps`, at most one pass's worth.
fn color_run(steps: &[Step]) -> Vec<ColorOp> {
    steps.iter().map_while(|s| if let Step::Color(op) = s { Some(*op) } else { None }).take(MAX_OPS).collect()
}

/// Split a chain into the steps that need passes of their own and the colour
/// operations at its end, which can ride along with the layer's blend.
pub(crate) fn split_tail(steps: &[Step]) -> (&[Step], Vec<ColorOp>) {
    let tail = steps.iter().rev().take_while(|s| matches!(s, Step::Color(_))).take(MAX_OPS).count();
    let (head, rest) = steps.split_at(steps.len() - tail);
    (head, color_run(rest))
}
