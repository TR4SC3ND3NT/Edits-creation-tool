//! Generated fragment shaders.
//!
//! Blending, colour effects and the glow's final add are pointwise, so they
//! can share a pass: read a pixel (from a texture, from a quad's picture, or
//! from a picture plus its glow), run some colour operations on it, and write
//! it either as it is or blended over the image beneath. Each combination
//! that a project actually uses is written out as straight-line WGSL and
//! compiled once. Straight-line matters on CPU rasterisers, which evaluate
//! every branch of a `switch` for every pixel.

use edits_core::Blend;

/// Colour operations a single pass can carry. (The last two uniform vectors
/// stay free: `p[25]` is the quad margin read by the vertex shader.)
pub(crate) const MAX_OPS: usize = 10;
/// Index of the first operation's two vectors in the uniform array `p`.
pub(crate) const OPS_AT: usize = 4;
const _: () = assert!(OPS_AT + 2 * MAX_OPS <= 24, "the uniform array must hold a full recipe");

// Operation codes, also the first component of each operation's uniforms.
pub(crate) const EXPOSURE: u8 = 1;
pub(crate) const FADE: u8 = 2;
pub(crate) const BRIGHTNESS_CONTRAST: u8 = 3;
pub(crate) const SATURATION: u8 = 4;
pub(crate) const HUE: u8 = 5;
pub(crate) const INVERT: u8 = 6;
pub(crate) const THRESHOLD: u8 = 7;
pub(crate) const POSTERIZE: u8 = 8;
pub(crate) const TINT: u8 = 9;
pub(crate) const VIGNETTE: u8 = 10;
pub(crate) const GRAIN: u8 = 11;

/// Where a pass reads its pixel from.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Input {
    /// Texture 0, over the whole target.
    Tex,
    /// Texture 0 plus the blurred bright pass in texture 1, tinted by `p[1]`.
    Glow,
    /// A quad's picture (texture 0) through the fit in `p[0]`, tinted by
    /// `p[1]`. `cut` trims the picture where its coordinates leave 0..1.
    Quad { cut: bool },
}

/// How a pass writes its pixel.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Output {
    Plain,
    /// Blended, with opacity `p[2].z`, over texture 1, or over the flat
    /// colour `p[3]` when `flat` (the background, before anything is drawn).
    Blend { mode: u8, flat: bool },
}

/// One combination of input, colour operations and output.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Recipe {
    pub input: Input,
    pub ops: Vec<u8>,
    pub output: Output,
}

/// A blend mode's code in generated passes: its index in [`Blend::ALL`]
/// (0 is `normal`).
pub(crate) fn blend_code(mode: Blend) -> u8 {
    Blend::ALL.iter().position(|m| *m == mode).expect("every mode is in Blend::ALL") as u8
}

/// WGSL helpers for the blend formulas, added to a pass that blends with a
/// mode other than `normal` (functions it does not call cost nothing).
///
/// The formulas are the W3C "Compositing and Blending Level 1" ones, with
/// the mode list and the After Effects conventions of EffectCraft
/// (https://github.com/storytold/effectcraft, `crates/color/src/blend.rs`,
/// MIT OR Apache-2.0, used under MIT), changed to stay sensible on linear
/// light above 1 (see `docs/RENDERING.md`). Luminance is Rec. 709 (`luma`).
const BLEND_HELPERS: &str = r#"
fn c01(x: vec3<f32>) -> vec3<f32> {
    return clamp(x, vec3<f32>(0.0), vec3<f32>(1.0));
}
// Screen whose product term is clamped, so that light above 1 never darkens.
fn bl_screen(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    return a + b - c01(a) * c01(b);
}
fn bl_burn(b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    let r = vec3<f32>(1.0) - min((vec3<f32>(1.0) - b) / max(s, vec3<f32>(1e-6)), vec3<f32>(1.0));
    return select(select(r, vec3<f32>(0.0), s <= vec3<f32>(0.0)), vec3<f32>(1.0), b >= vec3<f32>(1.0));
}
fn bl_dodge(b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    let r = min(b / max(vec3<f32>(1.0) - s, vec3<f32>(1e-6)), vec3<f32>(1.0));
    return select(select(r, vec3<f32>(1.0), s >= vec3<f32>(1.0)), vec3<f32>(0.0), b <= vec3<f32>(0.0));
}
fn bl_soft(b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    let d = select(sqrt(b), ((16.0 * b - 12.0) * b + 4.0) * b, b <= vec3<f32>(0.25));
    return select(b + (2.0 * s - 1.0) * (d - b), b - (1.0 - 2.0 * s) * b * (1.0 - b), s <= vec3<f32>(0.5));
}
fn bl_clip(c: vec3<f32>) -> vec3<f32> {
    let l = luma(c);
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    var o = select(c, l + (c - l) * l / max(l - n, 1e-6), n < 0.0);
    o = select(o, l + (o - l) * (1.0 - l) / max(x - l, 1e-6), x > 1.0);
    return o;
}
fn bl_set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return bl_clip(c + vec3<f32>(l - luma(c)));
}
fn bl_sat(c: vec3<f32>) -> f32 {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}
fn bl_set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let mn = min(c.r, min(c.g, c.b));
    let range = bl_sat(c);
    return select((c - mn) * s / max(range, 1e-6), vec3<f32>(0.0), range <= 1e-6);
}
"#;

/// Straight colours `b` (beneath) and `s` (the layer) -> the blended colour,
/// as WGSL lines that end by defining `mixed`.
///
/// - Modes that are naturally fine above 1 use the values as they are; the
///   product terms of screen, overlay, hard light and exclusion use values
///   clamped to 0..1, so that bright light never turns dark.
/// - Modes built for 0..1 (colour burn and dodge, soft light, vivid light,
///   hard mix, divide) run on the values clamped to 0..1 and then add back
///   what the backdrop had above 1.
/// - Component modes (hue, saturation, colour, luminosity) run on both
///   colours divided by their brightest component when that is above 1, and
///   scale the result back.
fn blend_lines(mode: u8) -> String {
    let m = Blend::ALL.get(mode as usize).copied().unwrap_or(Blend::Normal);
    let simple = |f: &str| format!("let mixed = {f};");
    let clamped = |f: &str| format!("let cb = c01(b);\n    let cs = c01(s);\n    let mixed = {f} + max(b - vec3<f32>(1.0), vec3<f32>(0.0));");
    let component = |f: &str| {
        format!("let m = max(1.0, max(max(max(b.r, b.g), b.b), max(max(s.r, s.g), s.b)));\n    let bn = b / m;\n    let sn = s / m;\n    let mixed = m * {f};")
    };
    match m {
        Blend::Normal => simple("s"),
        Blend::Add | Blend::LinearDodge => simple("b + s"),
        Blend::Screen => simple("bl_screen(b, s)"),
        Blend::Multiply => simple("b * s"),
        Blend::Overlay => simple("select(bl_screen(s, 2.0 * b - 1.0), 2.0 * b * s, b <= vec3<f32>(0.5))"),
        Blend::Lighten => simple("max(b, s)"),
        Blend::Darken => simple("min(b, s)"),
        Blend::Difference => simple("abs(b - s)"),
        Blend::ColorBurn => clamped("bl_burn(cb, cs)"),
        Blend::LinearBurn => simple("max(b + s - 1.0, vec3<f32>(0.0))"),
        Blend::DarkerColor => simple("select(b, s, luma(s) < luma(b))"),
        Blend::ColorDodge => clamped("bl_dodge(cb, cs)"),
        Blend::LighterColor => simple("select(b, s, luma(s) > luma(b))"),
        Blend::SoftLight => clamped("bl_soft(cb, cs)"),
        Blend::HardLight => simple("select(bl_screen(b, 2.0 * s - 1.0), 2.0 * b * s, s <= vec3<f32>(0.5))"),
        Blend::VividLight => clamped("select(bl_dodge(cb, 2.0 * cs - 1.0), bl_burn(cb, 2.0 * cs), cs <= vec3<f32>(0.5))"),
        Blend::LinearLight => simple("max(b + 2.0 * s - 1.0, vec3<f32>(0.0))"),
        Blend::PinLight => simple("select(max(b, 2.0 * s - 1.0), min(b, 2.0 * s), s <= vec3<f32>(0.5))"),
        Blend::HardMix => clamped("select(vec3<f32>(0.0), vec3<f32>(1.0), cb + cs >= vec3<f32>(1.0))"),
        Blend::Exclusion => simple("b + s - 2.0 * c01(b) * c01(s)"),
        Blend::Subtract => simple("max(b - s, vec3<f32>(0.0))"),
        Blend::Divide => clamped("min(cb / max(cs, vec3<f32>(1e-6)), vec3<f32>(1.0))"),
        Blend::Hue => component("bl_set_lum(bl_set_sat(sn, bl_sat(bn)), luma(bn))"),
        Blend::Saturation => component("bl_set_lum(bl_set_sat(bn, bl_sat(sn)), luma(bn))"),
        Blend::Color => component("bl_set_lum(sn, luma(bn))"),
        Blend::Luminosity => component("bl_set_lum(bn, luma(sn))"),
    }
}

/// The body of one colour operation on the straight colour `c`. `K` and `Q`
/// are replaced by the operation's two uniform vectors.
fn op_body(kind: u8) -> &'static str {
    match kind {
        // K.y = gain
        EXPOSURE => "c = c * K.y;",
        // K.y = amount, Q.rgb = colour
        FADE => "c = mix(c, Q.rgb, K.y);",
        // Display-referred. K.y = brightness, K.z = contrast gain
        BRIGHTNESS_CONTRAST => "c = dec(max((enc(c) + vec3<f32>(K.y) - vec3<f32>(0.5)) * K.z + vec3<f32>(0.5), vec3<f32>(0.0)));",
        // K.y = amount
        SATURATION => "c = max(mix(vec3<f32>(luma(c)), c, K.y), vec3<f32>(0.0));",
        // K.y = radians: a rotation about the grey axis
        HUE => {
            "let axis = vec3<f32>(0.57735027);
        let cs = cos(K.y);
        let sn = sin(K.y);
        c = max(c * cs + cross(axis, c) * sn + axis * dot(axis, c) * (1.0 - cs), vec3<f32>(0.0));"
        }
        // Display-referred. K.y = amount
        INVERT => {
            "let e = enc(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
        c = dec(mix(e, vec3<f32>(1.0) - e, K.y));"
        }
        // On display-referred luminance. K.y = level, K.z = softness, K.w = amount
        THRESHOLD => {
            "let l = enc1(max(luma(c), 0.0));
        let h = max(K.z * 0.5, 1e-5);
        c = mix(c, vec3<f32>(smoothstep(K.y - h, K.y + h, l)), K.w);"
        }
        // Display-referred. K.y = levels
        POSTERIZE => {
            "let e = enc(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
        c = dec(min(floor(e * K.y), vec3<f32>(K.y - 1.0)) / (K.y - 1.0));"
        }
        // K.y = amount, Q.rgb = colour
        TINT => "c = mix(c, Q.rgb * luma(c), K.y);",
        // K.y = amount, K.z = radius, K.w = softness
        VIGNETTE => {
            "let a = vec2<f32>(u.time.w, 1.0);
        let d = length((frame_uv - vec2<f32>(0.5)) * a) / length(a * 0.5);
        c = c * (1.0 - K.y * smoothstep(K.z, K.z + max(K.w, 1e-4), d));"
        }
        // Display-referred. K.y = amount, K.z = cell size in pixels, K.w = frame
        GRAIN => {
            "let cell = floor(in.pos.xy / K.z);
        let fr = u32(K.w);
        let g = hash3(u32(cell.x), u32(cell.y), fr) + hash3(u32(cell.x) + 7919u, u32(cell.y) + 104729u, fr) - 1.0;
        c = dec(max(enc(c) + vec3<f32>(g * K.y), vec3<f32>(0.0)));"
        }
        other => unreachable!("no colour operation {other}"),
    }
}

impl Recipe {
    /// `vs_geo` for quads, `vs_full` for everything else.
    pub fn vertex(&self) -> &'static str {
        match self.input {
            Input::Quad { .. } => "vs_geo",
            _ => "vs_full",
        }
    }

    /// Vertices to draw: a quad is two triangles, a full pass one.
    pub fn vertices(&self) -> u32 {
        match self.input {
            Input::Quad { .. } => 6,
            _ => 3,
        }
    }

    /// A short name for profiles, e.g. `Quad+2ops+blend0`.
    pub fn label(&self) -> String {
        let input = match self.input {
            Input::Tex => "Tex",
            Input::Glow => "Glow",
            Input::Quad { .. } => "Quad",
        };
        let ops = if self.ops.is_empty() { String::new() } else { format!("+{}ops", self.ops.len()) };
        let output = match self.output {
            Output::Plain => String::new(),
            Output::Blend { mode, flat } => format!("+blend{mode}{}", if flat { "bg" } else { "" }),
        };
        format!("{input}{ops}{output}")
    }

    /// The fragment shader `fs_main` for this recipe (appended to `common.wgsl`).
    pub fn fragment(&self) -> String {
        let mut f = String::new();
        if matches!(self.output, Output::Blend { mode, .. } if mode != 0) {
            f.push_str(BLEND_HELPERS);
        }
        f.push_str("\n@fragment\nfn fs_main(in: VOut) -> @location(0) vec4<f32> {\n");
        let mut line = |text: &str| {
            f.push_str("    ");
            f.push_str(text);
            f.push('\n');
        };
        // Coordinates in the frame: a quad's own `uv` runs over the quad instead.
        match self.input {
            Input::Tex => {
                line("let frame_uv = in.uv;");
                line("let s0 = src0(in.uv);");
            }
            Input::Glow => {
                line("let frame_uv = in.uv;");
                line("let base = src0(in.uv);");
                line("let g = textureSampleLevel(t1, samp, in.uv, 0.0).rgb * u.p[1].rgb;");
                // Outside the layer the glow brings its own coverage, so that later
                // colour effects and blend modes see a sensible straight colour there.
                line("let cover = clamp(max(g.r, max(g.g, g.b)), 0.0, 1.0);");
                line("let s0 = vec4<f32>(base.rgb + g, base.a + (1.0 - base.a) * cover);");
            }
            Input::Quad { cut } => {
                line("let frame_uv = in.pos.xy * u.res.zw;");
                line("let quv = u.p[0].xy + in.uv * u.p[0].zw;");
                if cut {
                    line("let qw = max(fwidth(quv), vec2<f32>(1e-6));");
                    line("let qm = clamp((vec2<f32>(0.5) - abs(quv - vec2<f32>(0.5))) / qw + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(1.0));");
                    line("let s0 = textureSample(t0, samp, quv) * (qm.x * qm.y) * u.p[1];");
                } else {
                    line("let s0 = textureSample(t0, samp, quv) * u.p[1];");
                }
            }
        }
        if self.ops.is_empty() {
            line("let s1 = s0;");
        } else {
            line("let alpha = s0.a;");
            line("var c = s0.rgb / max(alpha, 1e-5);");
            for (i, kind) in self.ops.iter().enumerate() {
                let (k, q) = (format!("u.p[{}]", OPS_AT + 2 * i), format!("u.p[{}]", OPS_AT + 2 * i + 1));
                line("{");
                line(&format!("    {}", op_body(*kind).replace('K', &k).replace('Q', &q)));
                line("}");
            }
            line("let s1 = vec4<f32>(c * alpha, alpha);");
        }
        match self.output {
            Output::Plain => line("return s1;"),
            Output::Blend { mode, flat } => {
                line("let sp = s1 * u.p[2].z;");
                if flat {
                    line("let d = u.p[3];");
                } else if matches!(self.input, Input::Quad { .. }) {
                    line("let d = textureLoad(t1, vec2<i32>(in.pos.xy), 0);");
                } else {
                    line("let d = textureSampleLevel(t1, samp, in.uv, 0.0);");
                }
                if mode == 0 {
                    line("return sp + d * (1.0 - sp.a);");
                } else {
                    line("let s = sp.rgb / max(sp.a, 1e-5);");
                    line("let b = d.rgb / max(d.a, 1e-5);");
                    line(&blend_lines(mode));
                    line("return vec4<f32>(sp.a * (1.0 - d.a) * s + sp.a * d.a * mixed + (1.0 - sp.a) * d.rgb, sp.a + d.a - sp.a * d.a);");
                }
            }
        }
        f.push_str("}\n");
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_only_contain_what_they_need() {
        let plain = Recipe { input: Input::Tex, ops: vec![EXPOSURE, VIGNETTE], output: Output::Plain }.fragment();
        assert!(plain.contains("c = c * u.p[4].y;") && plain.contains("u.p[6].z") && !plain.contains("hash3") && !plain.contains("t1"));
        let blend = Recipe { input: Input::Quad { cut: false }, ops: vec![], output: Output::Blend { mode: 2, flat: false } }.fragment();
        assert!(blend.contains("textureLoad(t1") && blend.contains("bl_screen(b, s)") && !blend.contains("fwidth") && !blend.contains("var c"));
        // A normal blend needs none of the blend helpers.
        let normal = Recipe { input: Input::Tex, ops: vec![], output: Output::Blend { mode: 0, flat: false } }.fragment();
        assert!(!normal.contains("bl_screen") && !normal.contains("let mixed"));
        // Every mode has a code of its own and a formula.
        for (i, m) in Blend::ALL.iter().enumerate() {
            assert_eq!(blend_code(*m) as usize, i);
            assert!(blend_lines(i as u8).contains("let mixed ="), "{m:?}");
        }
        let flat = Recipe { input: Input::Quad { cut: true }, ops: vec![], output: Output::Blend { mode: 0, flat: true } }.fragment();
        assert!(flat.contains("let d = u.p[3];") && flat.contains("fwidth") && !flat.contains("t1"));
        // No operation's text mentions the placeholders after substitution.
        let all: Vec<u8> = (EXPOSURE..=GRAIN).take(MAX_OPS).collect();
        let every = Recipe { input: Input::Glow, ops: all, output: Output::Plain }.fragment();
        assert!(!every.contains("K.") && !every.contains("Q."), "{every}");
        assert!(every.contains(&format!("u.p[{}]", OPS_AT + 2 * (MAX_OPS - 1))));
    }
}
