//! Effect chains: every kind in `edits_core::effects::EFFECTS`. The kinds in
//! [`NATIVE`] are implemented here (colour operations, blurs, glow, ...);
//! every other kind is a built-in one-pass shader from `fxlib`.
//!
//! Sizes given in "% of height" become pixels of the frame being rendered, so
//! a chain looks the same at any `scale`. Effects whose strength is zero are
//! skipped. Neighbouring colour effects share one pass, and can also share it
//! with a glow's final add or with the layer's blend (see `compose.rs`).
//!
//! Multi-pass light effects (their passes are in `shaders/passes.wgsl`):
//! - `bloom`: a mip chain after Bevy's bloom (MIT OR Apache-2.0); the
//!   weight of each level depends on its size in % of height, so the bloom
//!   looks the same at any scale.
//! - `light_rays`: a bright pass streaked toward a point (after EffectCraft's
//!   `light_rays`, MIT OR Apache-2.0), with the one-sided zoom streak below.
//! - `lens_flare`: one analytic pass (EffectCraft's `lens_flare` and
//!   `flare_ghosts` tables).
//! - `outline`: a jump flood (Rong and Tan 2006) of the nearest pixel inside
//!   and outside the layer's alpha, then a stroke from the signed distance.
//! - `drop_shadow`: the gaussian below on the layer, offset and tinted.
//! - `tilt_shift`: glfx.js's two passes of a triangle filter whose radius
//!   grows with the distance from a line (MIT).

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

/// The effects implemented in this file rather than as `fxlib` shaders (a
/// test checks that every registered effect is one or the other).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const NATIVE: &[&str] = &[
    "exposure", "fade", "brightness_contrast", "saturation", "hue", "invert", "threshold", "posterize", "tint", "vignette", "grain",
    "gaussian_blur", "directional_blur", "zoom_blur", "spin_blur", "glow", "rgb_split", "transform2d", "custom",
    "bloom", "light_rays", "lens_flare", "outline", "drop_shadow", "tilt_shift",
    // Time effects, planned here and run by `time.rs`.
    "echo", "time_displacement",
];

/// Bloom: the weight of a mip level whose texels are `x` times the radius.
/// Levels near the radius weigh most and fine levels little, so that a
/// smaller frame, which lacks the finest level, blooms the same.
fn bloom_profile(x: f32) -> f32 {
    x * x * (-0.5 * x * x).exp()
}

/// The weights of the bloom's mip levels (level `k` is the frame divided by
/// `2^(k+1)`) for a radius of `radius` pixels, normalised to a sum of 1.
fn bloom_weights(radius: f32, w: u32, h: u32) -> Vec<f32> {
    let mut weights = Vec::new();
    let (mut lw, mut lh) = (w.div_ceil(2), h.div_ceil(2));
    let mut texel = 2.0;
    loop {
        let x = texel / radius.max(1e-3);
        if !weights.is_empty() && (x > 4.0 || lw.min(lh) < 2) {
            break;
        }
        weights.push(bloom_profile(x));
        (lw, lh, texel) = (lw.div_ceil(2), lh.div_ceil(2), texel * 2.0);
    }
    let sum: f32 = weights.iter().sum();
    weights.iter().map(|v| if sum > 0.0 { v / sum } else { 1.0 / weights.len() as f32 }).collect()
}

/// A flare ghost: position along the line from the light through the
/// frame's centre (0 at the light, 1 at the centre), radius (fraction of the
/// frame's diagonal), strength, ring (1) or disc (0), colour.
type Ghost = (f32, f32, f32, f32, [f32; 3]);

/// The flare of a lens: core size, streak count, halo radius, ghosts.
/// Values from EffectCraft's `lens_flare` and `flare_ghosts`
/// (https://github.com/storytold/effectcraft, MIT OR Apache-2.0).
fn flare_lens(lens: &str) -> (f32, f32, f32, &'static [Ghost]) {
    match lens {
        "prime35" => (0.035, 6.0, 0.18, &[
            (0.45, 0.03, 0.20, 0.0, [0.4, 0.7, 1.0]),
            (0.9, 0.06, 0.12, 0.0, [0.5, 1.0, 0.6]),
            (1.3, 0.12, 0.10, 1.0, [1.0, 0.6, 0.3]),
            (1.6, 0.04, 0.15, 0.0, [0.8, 0.5, 1.0]),
        ]),
        "prime105" => (0.02, 12.0, 0.3, &[
            (0.6, 0.02, 0.25, 0.0, [1.0, 0.8, 0.5]),
            (1.1, 0.05, 0.12, 0.0, [0.6, 0.6, 1.0]),
            (1.4, 0.09, 0.08, 1.0, [0.4, 0.9, 1.0]),
        ]),
        _ => (0.03, 8.0, 0.25, &[
            (0.25, 0.02, 0.25, 0.0, [1.0, 0.9, 0.6]),
            (0.5, 0.04, 0.15, 0.0, [0.3, 1.0, 0.5]),
            (0.75, 0.025, 0.2, 0.0, [0.5, 0.6, 1.0]),
            (1.15, 0.07, 0.10, 0.0, [1.0, 0.5, 0.3]),
            (1.45, 0.1, 0.08, 1.0, [0.6, 0.4, 1.0]),
            (1.8, 0.05, 0.15, 0.0, [0.4, 0.9, 1.0]),
            (2.1, 0.14, 0.06, 1.0, [1.0, 0.8, 0.4]),
        ]),
    }
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
    /// A built-in one-pass effect (`fxlib`), its numbers and extra inputs.
    Builtin(wgpu::RenderPipeline, crate::fxlib::Params, Box<crate::fxlib::Inputs>),
    Custom(wgpu::RenderPipeline, crate::fxlib::Params),
    /// Soft threshold (threshold, threshold - knee, 2 knee, 0.25 / knee),
    /// radius in pixels, colour x intensity.
    Bloom { threshold: [f32; 4], radius: f32, color: [f32; 3] },
    /// Luminance threshold, centre (uv), log of the largest zoom toward the
    /// centre, longest ray in pixels of the half-size bright pass, colour x
    /// intensity, mode (0 add, 1 screen, 2 lighten).
    Rays { threshold: f32, centre: [f32; 2], zoom: f32, length: f32, color: [f32; 3], mode: f32 },
    /// The uniforms of `fs_lens_flare`.
    Flare(Vec<[f32; 4]>),
    /// Colour and opacity; widths outside and inside and the edge ramp (1 +
    /// softness), in pixels.
    Outline { color: [f32; 4], widths: [f32; 3] },
    /// Colour and opacity, offset (uv), sigma in pixels.
    Shadow { color: [f32; 4], offset: [f32; 2], sigma: f32 },
    /// The uniforms of the two `fs_tilt` passes.
    TiltShift([[[f32; 4]; 3]; 2]),
    /// A time effect (`echo`, `time_displacement`): it renders its input
    /// again at other times, so it runs through `Renderer::run_chain`.
    Time(crate::time::TimeStep),
}

impl Renderer {
    /// Turn resolved effects into the steps that will actually run for a
    /// `w` x `h` frame. Effects that would change nothing are left out, so an
    /// empty result means the chain can be skipped altogether.
    pub(crate) fn plan_effects(&mut self, layer: &str, effects: &[ResolvedEffect], w: f32, h: f32) -> Result<Vec<Step>> {
        let aspect = self.ctx.time[3];
        let frame = self.ctx.time[2];
        let mut steps = Vec::new();
        for (index, e) in effects.iter().enumerate() {
            if crate::time::is_time(&e.kind) {
                steps.extend(self.plan_time(layer, index, e)?);
                continue;
            }
            if is_color(&e.kind) {
                steps.extend(color_op(e, h, frame).map(Step::Color));
                continue;
            }
            if let Some(b) = crate::fxlib::builtin(&e.kind) {
                steps.extend(self.plan_builtin(layer, b, e, h)?);
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
                "bloom" => {
                    let (k, color, radius) = (n("intensity"), e.vec("color", 3), n("radius") / 100.0 * h);
                    if k > 0.0 && radius > 0.0 {
                        let threshold = n("threshold").max(0.0);
                        let knee = threshold * n("knee").clamp(0.0, 1.0);
                        steps.push(Step::Bloom {
                            threshold: [threshold, threshold - knee, 2.0 * knee, 0.25 / (knee + 1e-5)],
                            radius,
                            color: [color[0] as f32 * k, color[1] as f32 * k, color[2] as f32 * k],
                        });
                    }
                }
                "light_rays" => {
                    let (k, color, length) = (n("intensity"), e.vec("color", 3), n("length").clamp(0.0, 0.95));
                    if k > 0.0 && length > 0.0 {
                        let c = e.vec("center", 2);
                        let centre = [0.5 + c[0] as f32 / aspect, 0.5 + c[1] as f32];
                        let reach = ((centre[0].abs().max((1.0 - centre[0]).abs()) * w).powi(2) + (centre[1].abs().max((1.0 - centre[1]).abs()) * h).powi(2)).sqrt();
                        let mode = match e.option("mode") {
                            "screen" => 1.0,
                            "lighten" => 2.0,
                            _ => 0.0,
                        };
                        steps.push(Step::Rays {
                            threshold: n("threshold"),
                            centre,
                            zoom: -(1.0 - length).ln(),
                            length: length * reach * 0.5,
                            color: [color[0] as f32 * k, color[1] as f32 * k, color[2] as f32 * k],
                            mode,
                        });
                    }
                }
                "lens_flare" => {
                    let (brightness, size, pos) = (n("brightness"), n("size"), e.vec("position", 2));
                    if brightness > 0.0 && size > 0.0 {
                        let (core, streaks, halo, ghosts) = flare_lens(e.option("lens"));
                        let mut p = vec![[pos[0] as f32, pos[1] as f32, brightness, ghosts.len() as f32], [core, streaks, halo, size]];
                        for &(t, r, strength, ring, c) in ghosts {
                            p.push([t, r, strength, ring]);
                            p.push([c[0], c[1], c[2], 0.0]);
                        }
                        steps.push(Step::Flare(p));
                    }
                }
                "outline" => {
                    let (width, opacity, c) = (n("width").max(0.0) / 100.0 * h, n("opacity").clamp(0.0, 1.0), e.vec("color", 3));
                    if width > 0.0 && opacity > 0.0 {
                        let (outside, inside) = match e.option("position") {
                            "inside" => (0.0, width),
                            "center" => (width * 0.5, width * 0.5),
                            _ => (width, 0.0),
                        };
                        let ramp = 1.0 + n("softness").max(0.0) / 100.0 * h;
                        steps.push(Step::Outline { color: [c[0] as f32, c[1] as f32, c[2] as f32, opacity], widths: [outside, inside, ramp] });
                    }
                }
                "drop_shadow" => {
                    let (opacity, c, off) = (n("opacity").clamp(0.0, 1.0), e.vec("color", 3), e.vec("offset", 2));
                    if opacity > 0.0 {
                        steps.push(Step::Shadow {
                            color: [c[0] as f32, c[1] as f32, c[2] as f32, opacity],
                            offset: [off[0] as f32 / aspect, off[1] as f32],
                            sigma: n("softness").max(0.0) / 100.0 * h,
                        });
                    }
                }
                "tilt_shift" => {
                    let radius = n("blur") / 100.0 * h;
                    if radius >= 0.25 {
                        let c = e.vec("center", 2);
                        let centre = [(0.5 + c[0] as f32 / aspect) * w, (0.5 + c[1] as f32) * h];
                        let (sin, cos) = n("angle").to_radians().sin_cos();
                        let half = n("width").max(0.0) / 100.0 * h * 0.5;
                        // Taps on each side; past one per pixel the gaps are hidden by jitter.
                        let taps = radius.ceil().clamp(2.0, 24.0);
                        let jitter = if radius > taps { 1.0 } else { 0.0 };
                        let pass = |dir: [f32; 2]| [[centre[0], centre[1], -sin, cos], [half, half, radius, taps], [dir[0], dir[1], jitter, 0.0]];
                        steps.push(Step::TiltShift([pass([cos, sin]), pass([-sin, cos])]));
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
                    let mut p = [0.0; 16];
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
    /// nothing to do. `input` is never released or written to. Time steps
    /// are skipped: a chain that may have them runs through
    /// `Renderer::run_chain`.
    pub(crate) fn run_effects(&mut self, input: &Tex, steps: &[Step]) -> Option<Tex> {
        let aspect = self.ctx.time[3];
        let mut cur: Option<Tex> = None;
        let mut i = 0;
        while i < steps.len() {
            if let Step::Time(_) = &steps[i] {
                i += 1;
                continue;
            }
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
                Step::Streak(mode, centre, amount, length) => self.streak(&s, *mode, *centre, *amount, *length, false),
                Step::Bloom { threshold, radius, color } => {
                    let bloom = self.bloom(&s, *threshold, *radius);
                    // Colour effects right after the bloom share its final pass.
                    let ops = color_run(&steps[i + 1..]);
                    i += ops.len();
                    let out = self.acquire(s.w, s.h);
                    self.compose(&out, Source::Glow(&s, &bloom, *color), &ops, Sink::Plain);
                    self.release(bloom);
                    out
                }
                Step::Rays { threshold, centre, zoom, length, color, mode } => {
                    let half = self.acquire(s.w.div_ceil(2), s.h.div_ceil(2));
                    self.pass(Pipe::Bright, &half, false, &s, None, &[[*threshold, 0.0, 0.0, 0.0]]);
                    let rays = self.streak(&half, 1.0, *centre, [*zoom, 0.0], *length, true);
                    self.release(half);
                    let out = self.acquire(s.w, s.h);
                    self.pass(Pipe::Pass("fs_rays_mix"), &out, false, &s, Some(&rays), &[[color[0], color[1], color[2], *mode]]);
                    self.release(rays);
                    out
                }
                Step::Flare(p) => {
                    let out = self.acquire(s.w, s.h);
                    self.pass(Pipe::Pass("fs_lens_flare"), &out, false, &s, None, p);
                    out
                }
                Step::Outline { color, widths } => {
                    let field = self.jump_flood(&s, widths[0].max(widths[1]) + widths[2] + 2.0);
                    let out = self.acquire(s.w, s.h);
                    self.pass(Pipe::Pass("fs_outline"), &out, false, &s, Some(&field), &[*color, [widths[0], widths[1], widths[2], 0.0]]);
                    self.release(field);
                    out
                }
                Step::Shadow { color, offset, sigma } => {
                    let blurred = (*sigma >= 0.05).then(|| self.gaussian(&s, *sigma));
                    let out = self.acquire(s.w, s.h);
                    self.pass(Pipe::Pass("fs_shadow"), &out, false, &s, Some(blurred.as_ref().unwrap_or(&s)), &[*color, [offset[0], offset[1], 0.0, 0.0]]);
                    if let Some(b) = blurred {
                        self.release(b);
                    }
                    out
                }
                Step::TiltShift([first, second]) => {
                    let mid = self.acquire(s.w, s.h);
                    self.pass(Pipe::Pass("fs_tilt"), &mid, false, &s, None, first);
                    let out = self.acquire(s.w, s.h);
                    self.pass(Pipe::Pass("fs_tilt"), &out, false, &mid, None, second);
                    self.release(mid);
                    out
                }
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
                Step::Builtin(pipeline, p, inputs) => {
                    let out = self.acquire(s.w, s.h);
                    self.fx_pass(pipeline, &out, &s, inputs.aux.as_ref(), inputs.lut.as_ref(), p);
                    out
                }
                Step::Custom(pipeline, p) => {
                    let out = self.acquire(s.w, s.h);
                    self.fx_pass(pipeline, &out, &s, None, None, p);
                    out
                }
                Step::Time(_) => unreachable!("skipped above"),
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
    /// `STREAK_TAPS^n` evenly spaced points. The streak is centred on each
    /// pixel, or with `one_sided` reaches from it toward the centre only (a
    /// zoom then samples scales from `exp(-amount)` to 1: light rays).
    fn streak(&mut self, src: &Tex, mode: f32, centre: [f32; 2], amount: [f32; 2], length: f32, one_sided: bool) -> Tex {
        let taps = STREAK_TAPS as usize;
        let mut passes = 1;
        while (taps as f32).powi(passes) < length / STREAK_SPACING && passes < 5 {
            passes += 1;
        }
        let aspect = src.w as f32 / src.h as f32;
        let mut cur = src.clone();
        let mut span = 1.0;
        // Shifting the first pass's taps by half a streak shifts the whole
        // sum of passes: from -0.5..0.5 to -1..0.
        let mut shift = if one_sided { -0.5 } else { 0.0 };
        for _ in 0..passes {
            let mut p = [[0.0f32; 4]; 18];
            p[0] = [STREAK_TAPS, aspect, 0.0, 0.0];
            p[1] = [centre[0], centre[1], 0.0, 0.0];
            for i in 0..taps {
                let t = ((i as f32 + 0.5) / STREAK_TAPS - 0.5) * span + shift;
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
            shift = 0.0;
        }
        cur
    }

    /// The bloom of `src` at half size, before its colour: a soft threshold
    /// pixel by pixel, a 13-tap downsample with Karis' average, a chain of
    /// further 13-tap downsamples, then tent
    /// upsamples from the smallest level that add each finer level times its
    /// weight (see [`bloom_weights`]).
    fn bloom(&mut self, src: &Tex, threshold: [f32; 4], radius: f32) -> Tex {
        let weights = bloom_weights(radius, src.w, src.h);
        let bright = self.acquire(src.w, src.h);
        self.pass(Pipe::Pass("fs_bloom_bright"), &bright, false, src, None, &[threshold]);
        let first = self.acquire(src.w.div_ceil(2), src.h.div_ceil(2));
        self.pass(Pipe::Pass("fs_bloom_first"), &first, false, &bright, None, &[[0.0; 4], [1.0 / src.w as f32, 1.0 / src.h as f32, 0.0, 0.0]]);
        self.release(bright);
        let mut levels = vec![first];
        while levels.len() < weights.len() {
            let prev = levels[levels.len() - 1].clone();
            let next = self.acquire(prev.w.div_ceil(2), prev.h.div_ceil(2));
            self.pass(Pipe::Pass("fs_bloom_down"), &next, false, &prev, None, &[[0.0; 4], [1.0 / prev.w as f32, 1.0 / prev.h as f32, 0.0, 0.0]]);
            levels.push(next);
        }
        let last = levels.pop().expect("at least one level");
        let mut acc = self.acquire(last.w, last.h);
        self.pass(Pipe::Copy, &acc, false, &last, None, &[[weights[levels.len()], 0.0, 0.0, 0.0]]);
        self.release(last);
        while let Some(level) = levels.pop() {
            let out = self.acquire(level.w, level.h);
            self.pass(Pipe::Pass("fs_bloom_up"), &out, false, &acc, Some(&level), &[[1.0 / acc.w as f32, 1.0 / acc.h as f32, weights[levels.len()], 0.0]]);
            self.release(acc);
            self.release(level);
            acc = out;
        }
        acc
    }

    /// A jump flood of `src`'s alpha (inside where it is at least one half):
    /// each pixel of the result holds the offset in pixels to the nearest
    /// pixel inside (`xy`) and to the nearest pixel outside (`zw`), for
    /// distances up to `reach` pixels.
    fn jump_flood(&mut self, src: &Tex, reach: f32) -> Tex {
        let mut field = self.acquire(src.w, src.h);
        self.pass(Pipe::Pass("fs_jfa_seed"), &field, false, src, None, &[]);
        let mut step = ((reach.ceil().max(1.0) as u32).next_power_of_two() / 2).max(1);
        let mut steps = Vec::new();
        while step >= 1 {
            steps.push(step);
            step /= 2;
        }
        // One more pass of 1 mends the rare misses of plain jump flooding.
        steps.push(1);
        for step in steps {
            let next = self.acquire(src.w, src.h);
            self.pass(Pipe::Pass("fs_jfa"), &next, false, &field, None, &[[step as f32, 0.0, 0.0, 0.0]]);
            self.release(field);
            field = next;
        }
        field
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
