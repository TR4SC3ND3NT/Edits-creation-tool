//! Transitions between two pictures, from the gl-transitions collection.
//!
//! The shaders in `shaders/transitions/` are the WGSL the `gl-transitions`
//! crate generates (see the LICENSE.md there), used unchanged: here their
//! textures are moved to this renderer's bind group, their colour reads go
//! through sRGB (the space they were written for; the renderer works in
//! linear light), and their entry point is wrapped. Each declares its own
//! uniform struct (`progress`, `ratio`, then its parameters); the table
//! below says where each parameter goes in it.

use edits_core::ResolvedTransition;

#[derive(Clone, Copy)]
pub(crate) enum Ty {
    F,
    I,
    V2,
    V3,
    V4,
    IV2,
}

/// One transition: its WGSL and where its parameters go in its uniform struct.
pub(crate) struct Def {
    pub kind: &'static str,
    pub source: &'static str,
    /// (parameter name in the registry, type, byte offset).
    pub fields: &'static [(&'static str, Ty, usize)],
}

macro_rules! def {
    ($kind:literal, $file:literal, [$(($n:literal, $t:ident, $o:literal)),* $(,)?]) => {
        Def { kind: $kind, source: include_str!(concat!("shaders/transitions/", $file, ".wgsl")), fields: &[$(($n, Ty::$t, $o)),*] }
    };
}

pub(crate) const DEFS: &[Def] = &[
    def!("fade", "Fade", []),
    def!("fade_color", "FadeColor", [("color", V4, 16), ("color_phase", F, 32)]),
    def!("flash_fade", "FlashFade", [("color", V3, 16)]),
    def!("overexposure", "Overexposure", [("strength", F, 8)]),
    def!("lightning_flash", "LightningFlash", [("bolts", I, 8), ("strike", F, 12), ("seed", F, 16), ("color", V3, 32), ("glow", F, 44), ("flashes", I, 48)]),
    def!("cross_zoom", "CrossZoom", [("strength", F, 8)]),
    def!("simple_zoom", "SimpleZoom", [("zoom_quickness", F, 8)]),
    def!("zoom_in_out", "ZoomInOut", []),
    def!("dreamy_zoom", "DreamyZoom", [("rotation", F, 8), ("scale", F, 12)]),
    def!("directional", "Directional", [("direction", V2, 8)]),
    def!("directional_warp", "DirectionalWarp", [("smoothness", F, 8), ("direction", V2, 16)]),
    def!("whip_pan", "WhipPan", [("direction", V2, 8), ("blur", F, 16)]),
    def!("whip_spin", "WhipSpin", [("turns", F, 8), ("blur", F, 12), ("chroma", F, 16), ("zoom", F, 20)]),
    def!("whip_tilt", "WhipTilt", [("down", I, 8), ("blur", F, 12), ("chroma", F, 16), ("perspective", F, 20)]),
    def!("camera_shake", "CameraShake", [("strength", F, 8), ("jolts", F, 12), ("blur", F, 16), ("split", F, 20)]),
    def!("glitch_memories", "GlitchMemories", []),
    def!("glitch_displace", "GlitchDisplace", []),
    def!("cube", "Cube", [("persp", F, 8), ("unzoom", F, 12), ("reflection", F, 16), ("floating", F, 20), ("shading", F, 24)]),
    def!("swirl", "Swirl", [("strength", F, 8), ("radius", F, 12), ("center", V2, 16)]),
    def!("rotate_scale_fade", "RotateScaleFade", [("center", V2, 8), ("rotations", F, 16), ("scale", F, 20), ("back_color", V4, 32)]),
    def!("pixelize", "Pixelize", [("squares_min", IV2, 8), ("steps", I, 16)]),
    def!("film_burn", "FilmBurn", [("seed", F, 8)]),
];

pub(crate) fn def(kind: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.kind == kind)
}

/// Appended to every transition: the vertex stage, colour conversion and the fragment entry.
const SHIM: &str = r#"
// ---- appended by edits-render ------------------------------------------------
struct EditsTrOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn edits_tr_vs(@builtin(vertex_index) i: u32) -> EditsTrOut {
    var out: EditsTrOut;
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    // gl-transitions puts uv (0, 0) at the bottom-left.
    out.uv = vec2<f32>(x * 0.5 + 0.5, y * 0.5 + 0.5);
    return out;
}

fn edits_enc1(v: f32) -> f32 {
    let a = max(v, 0.0);
    if a <= 0.0031308 {
        return a * 12.92;
    }
    return 1.055 * pow(a, 1.0 / 2.4) - 0.055;
}
fn edits_dec1(v: f32) -> f32 {
    let a = max(v, 0.0);
    if a <= 0.04045 {
        return a / 12.92;
    }
    return pow((a + 0.055) / 1.055, 2.4);
}
// Working colour (linear, premultiplied) to what the transitions expect (sRGB, straight alpha).
fn edits_in(c: vec4<f32>) -> vec4<f32> {
    if c.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let rgb = c.rgb / c.a;
    return vec4<f32>(edits_enc1(rgb.r), edits_enc1(rgb.g), edits_enc1(rgb.b), c.a);
}
fn edits_from(uv: vec2<f32>, lod: f32) -> vec4<f32> {
    return edits_in(textureSampleLevel(_from, _sampler, uv, lod));
}
fn edits_to(uv: vec2<f32>, lod: f32) -> vec4<f32> {
    return edits_in(textureSampleLevel(_to, _sampler, uv, lod));
}

@fragment
fn edits_tr_fs(in: EditsTrOut) -> @location(0) vec4<f32> {
    let c = edits_glt_main(in.uv);
    let a = clamp(c.a, 0.0, 1.0);
    return vec4<f32>(edits_dec1(c.r) * a, edits_dec1(c.g) * a, edits_dec1(c.b) * a, a);
}
"#;

/// The WGSL the renderer compiles for a transition.
pub(crate) fn source(d: &Def) -> String {
    let lines: Vec<&str> = d.source.lines().collect();
    let mut out = String::with_capacity(d.source.len() + SHIM.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let next = lines.get(i + 1).copied().unwrap_or("");
        if line.trim_start().starts_with("@group(0) @binding(") {
            // Textures and sampler move to group 1 (the renderer's texture group).
            let rebound = if next.starts_with("var _from") {
                Some("@group(1) @binding(0)")
            } else if next.starts_with("var _to") {
                Some("@group(1) @binding(1)")
            } else if next.starts_with("var _sampler") {
                Some("@group(1) @binding(2)")
            } else {
                None
            };
            out.push_str(rebound.unwrap_or(line));
            out.push('\n');
            i += 1;
            continue;
        }
        if line.trim() == "@fragment" && next.starts_with("fn main(") {
            out.push_str("fn edits_glt_main(_uv: vec2<f32>) -> vec4<f32> {\n");
            i += 2;
            continue;
        }
        let l = line.replace("textureSampleLevel(_from, _sampler,", "edits_from(").replace("textureSampleLevel(_to, _sampler,", "edits_to(");
        out.push_str(&l);
        out.push('\n');
        i += 1;
    }
    out.push_str(SHIM);
    out
}

/// The uniform bytes for a transition at `progress`, for a frame of `ratio` (width / height).
pub(crate) fn uniforms(d: &Def, t: &ResolvedTransition, ratio: f32, out: &mut [u8]) {
    let mut put = |offset: usize, bytes: [u8; 4]| {
        if offset + 4 <= out.len() {
            out[offset..offset + 4].copy_from_slice(&bytes);
        }
    };
    put(0, (t.progress as f32).to_le_bytes());
    put(4, ratio.to_le_bytes());
    for (name, ty, offset) in d.fields {
        let v = t.params.get(*name).cloned().unwrap_or_default();
        let at = |i: usize| v.get(i).or(v.last()).copied().unwrap_or(0.0);
        let (n, int) = match ty {
            Ty::F => (1, false),
            Ty::I => (1, true),
            Ty::V2 => (2, false),
            Ty::V3 => (3, false),
            Ty::V4 => (4, false),
            Ty::IV2 => (2, true),
        };
        for c in 0..n {
            let bytes = if int { (at(c).round() as i32).to_le_bytes() } else { (at(c) as f32).to_le_bytes() };
            put(offset + 4 * c, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_transition_has_a_shader_and_matching_parameters() {
        assert_eq!(DEFS.len(), edits_core::effects::TRANSITIONS.len());
        for spec in edits_core::effects::TRANSITIONS {
            let d = def(spec.kind).unwrap_or_else(|| panic!("no shader for {}", spec.kind));
            let names: Vec<&str> = d.fields.iter().map(|f| f.0).collect();
            let registered: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
            assert_eq!(names, registered, "{}", spec.kind);
            let src = source(d);
            assert!(src.contains("fn edits_glt_main(") && !src.contains("fn main("), "{}", spec.kind);
            assert!(!src.contains("@group(0) @binding(1)") && !src.contains("textureSampleLevel(_from, _sampler, vec2"), "{}", spec.kind);
            assert!(d.source.contains("// License: MIT"), "{} keeps its licence header", spec.kind);
        }
    }
}
