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

/// Colour operations a single pass can carry.
pub(crate) const MAX_OPS: usize = 11;
/// Index of the first operation's two vectors in the uniform array `p`.
pub(crate) const OPS_AT: usize = 4;

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

pub(crate) fn blend_code(mode: Blend) -> u8 {
    match mode {
        Blend::Normal => 0,
        Blend::Add => 1,
        Blend::Screen => 2,
        Blend::Multiply => 3,
        Blend::Overlay => 4,
        Blend::Lighten => 5,
        Blend::Darken => 6,
        Blend::Difference => 7,
    }
}

/// Straight colours `b` (beneath) and `s` (the layer) -> the blended colour.
fn blend_expr(mode: u8) -> &'static str {
    match mode {
        1 => "b + s",
        // Screen, kept from darkening values above 1.
        2 => "b + s - clamp(b, vec3<f32>(0.0), vec3<f32>(1.0)) * clamp(s, vec3<f32>(0.0), vec3<f32>(1.0))",
        3 => "b * s",
        4 => "select(vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - b) * (vec3<f32>(1.0) - s), 2.0 * b * s, b < vec3<f32>(0.5))",
        5 => "max(b, s)",
        6 => "min(b, s)",
        7 => "abs(b - s)",
        _ => "s",
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

    /// The fragment shader `fs_main` for this recipe (appended to `common.wgsl`).
    pub fn fragment(&self) -> String {
        let mut f = String::from("\n@fragment\nfn fs_main(in: VOut) -> @location(0) vec4<f32> {\n");
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
                    line(&format!("let mixed = {};", blend_expr(mode)));
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
        assert!(blend.contains("textureLoad(t1") && blend.contains("clamp(b") && !blend.contains("fwidth") && !blend.contains("var c"));
        let flat = Recipe { input: Input::Quad { cut: true }, ops: vec![], output: Output::Blend { mode: 0, flat: true } }.fragment();
        assert!(flat.contains("let d = u.p[3];") && flat.contains("fwidth") && !flat.contains("t1"));
        // No operation's text mentions the placeholders after substitution.
        let all: Vec<u8> = (EXPOSURE..=GRAIN).collect();
        let every = Recipe { input: Input::Glow, ops: all, output: Output::Plain }.fragment();
        assert!(!every.contains("K.") && !every.contains("Q."), "{every}");
        assert!(every.contains(&format!("u.p[{}]", OPS_AT + 2 * (MAX_OPS - 1))));
    }

    #[test]
    fn the_uniform_array_holds_a_full_recipe() {
        assert!(OPS_AT + 2 * MAX_OPS <= 26);
    }
}
