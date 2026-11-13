//! Built-in one-pass effects: each kind is a WGSL file in `shaders/fx/`
//! defining `fn effect(uv) -> vec4`, compiled with the same contract as a
//! custom effect (`shaders/fx/prelude.wgsl`), plus a packer that turns the
//! effect's resolved values into the sixteen numbers `fx.p0` .. `fx.p15`.
//!
//! Options are compiled in, not branched on: for an effect with an option
//! `shape` whose values are `square` and `hex`, the source starts with
//! `const SHAPE_SQUARE: u32 = 0u; const SHAPE_HEX: u32 = 1u; const OPT_SHAPE: u32 = <chosen>u;`,
//! so each combination in use is its own pipeline and a CPU rasteriser
//! never evaluates the branches that are not taken.
//!
//! To add an effect: its row in `edits_core::effects::<category>`, a WGSL
//! file here, a [`Builtin`] in the category's file below, a pixel test, and a
//! line in `docs/EFFECTS.md`.

use edits_core::effects::EffectSpec;
use edits_core::ResolvedEffect;

mod color;
mod distort;
mod stylize;

/// What a packer knows about the frame: its size in pixels and width / height.
pub(crate) struct Frame {
    pub w: f32,
    pub h: f32,
    pub aspect: f32,
}

/// The sixteen numbers an effect's shader reads as `fx.p0` .. `fx.p15`.
pub(crate) type Params = [f32; 16];

pub(crate) struct Builtin {
    pub kind: &'static str,
    /// The WGSL that defines `fn effect(uv: vec2<f32>) -> vec4<f32>`.
    pub source: &'static str,
    /// The effect's numbers for this frame, or `None` when it would change
    /// nothing (it is then skipped).
    pub pack: fn(&ResolvedEffect, &Frame) -> Option<Params>,
}

/// Every built-in one-pass effect.
pub(crate) fn all() -> impl Iterator<Item = &'static Builtin> {
    distort::BUILTINS.iter().chain(stylize::BUILTINS).chain(color::BUILTINS)
}

pub(crate) fn builtin(kind: &str) -> Option<&'static Builtin> {
    all().find(|b| b.kind == kind)
}

/// The chosen value of each of the effect's options, as indices into the
/// registry's lists: the key of the pipeline that runs it.
pub(crate) fn option_key(spec: &EffectSpec, e: &ResolvedEffect) -> Vec<u8> {
    spec.options.iter().map(|o| o.values.iter().position(|v| *v == e.option(o.name)).unwrap_or(0) as u8).collect()
}

fn ident(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect()
}

/// The WGSL compiled for `b` with the options `key`: option constants, the
/// effect, the shared contract and the entry point.
pub(crate) fn source(b: &Builtin, spec: &EffectSpec, key: &[u8]) -> String {
    let mut out = String::from("// ---- options, written by edits-render ----\n");
    for (i, o) in spec.options.iter().enumerate() {
        let name = ident(o.name);
        for (j, v) in o.values.iter().enumerate() {
            out.push_str(&format!("const {name}_{}: u32 = {j}u;\n", ident(v)));
        }
        out.push_str(&format!("const OPT_{name}: u32 = {}u;\n", key.get(i).copied().unwrap_or(0)));
    }
    out.push('\n');
    out.push_str(b.source);
    out.push('\n');
    out.push_str(crate::shader::CUSTOM_CONTRACT);
    out
}

/// Percent of height -> composition units.
pub(crate) fn pct(v: f64) -> f32 {
    (v / 100.0) as f32
}

/// A position parameter as two numbers.
pub(crate) fn pos(e: &ResolvedEffect, name: &str) -> [f32; 2] {
    let v = e.vec(name, 2);
    [v[0] as f32, v[1] as f32]
}

/// The value of a parameter clamped to its range in the registry.
pub(crate) fn num(e: &ResolvedEffect, name: &str) -> f32 {
    let v = e.num(name);
    let range = edits_core::effects::spec(&e.kind).and_then(|s| s.param(name)).and_then(|p| p.range);
    let v = match range {
        Some((lo, hi)) => v.clamp(lo, hi),
        None => v,
    };
    if v.is_finite() {
        v as f32
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_is_registered_and_compiles_with_every_option() {
        for b in all() {
            let spec = edits_core::effects::spec(b.kind).unwrap_or_else(|| panic!("{} is not in the registry", b.kind));
            // Every combination of options.
            let sizes: Vec<usize> = spec.options.iter().map(|o| o.values.len()).collect();
            let combos: usize = sizes.iter().product();
            for n in 0..combos {
                let mut rest = n;
                let key: Vec<u8> = sizes
                    .iter()
                    .map(|s| {
                        let k = rest % s;
                        rest /= s;
                        k as u8
                    })
                    .collect();
                let text = source(b, spec, &key);
                let module = naga::front::wgsl::parse_str(&text).unwrap_or_else(|e| panic!("{} {key:?}: {}", b.kind, e.emit_to_string(&text)));
                naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
                    .validate(&module)
                    .unwrap_or_else(|e| panic!("{} {key:?}: {e:?}", b.kind));
            }
            assert!(b.source.contains("License"), "{} names its licence", b.kind);
        }
    }

    #[test]
    fn every_registered_effect_is_implemented() {
        for spec in edits_core::effects::EFFECTS {
            assert!(builtin(spec.kind).is_some() || crate::fx::NATIVE.contains(&spec.kind), "{} has no implementation in the renderer", spec.kind);
            assert!(!(builtin(spec.kind).is_some() && crate::fx::NATIVE.contains(&spec.kind)), "{} is implemented twice", spec.kind);
        }
    }
}
