//! Built-in one-pass effects: each kind is a WGSL file in `shaders/fx/`
//! defining `fn effect(uv) -> vec4`, compiled with the same contract as a
//! custom effect (`shaders/fx/prelude.wgsl`) plus the helpers of
//! `shaders/fx/lib.wgsl`, and a packer that turns the effect's resolved
//! values into the sixteen numbers `fx.p0` .. `fx.p15`.
//!
//! Options are compiled in, not branched on: for an effect with an option
//! `shape` whose values are `square` and `hex`, the source starts with
//! `const SHAPE_SQUARE: u32 = 0u; const SHAPE_HEX: u32 = 1u; const OPT_SHAPE: u32 = <chosen>u;`,
//! so each combination in use is its own pipeline and a CPU rasteriser
//! never evaluates the branches that are not taken.
//!
//! Some effects read more than the picture: `lut` a `.cube` table (loaded by
//! `Renderer::prepare` into [`Loaded`] and bound as the 3D lookup table),
//! `displacement_map` a slot's picture (bound as the auxiliary picture).
//!
//! To add an effect: its row in `edits_core::effects::<category>`, a WGSL
//! file here, a [`Builtin`] in the category's file below, a pixel test, and a
//! line in `docs/EFFECTS.md`.

use crate::fx::Step;
use crate::renderer::{Lut, Renderer, Tex};
use anyhow::{anyhow, Result};
use edits_core::effects::{EffectSpec, SourceKind};
use edits_core::{Layer, Project, ResolvedEffect};
use std::collections::HashMap;
use std::path::Path;

mod color;
mod distort;
mod generate;
mod stylize;

/// Helpers shared by the built-in effects (not part of the custom contract).
const LIB: &str = include_str!("../shaders/fx/lib.wgsl");

/// What a packer knows about the instant being rendered.
pub(crate) struct Frame {
    /// Height in pixels.
    pub h: f32,
    /// Width / height.
    pub aspect: f32,
    /// Project time, in the project's time base (beats or seconds).
    pub t: f64,
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

/// The extra inputs of one built-in effect step.
#[derive(Clone, Default)]
pub(crate) struct Inputs {
    /// Bound as `fx_aux` (a displacement map).
    pub aux: Option<Tex>,
    /// Bound as `fx_lut`.
    pub lut: Option<Lut>,
}

/// Lookup tables of a prepared project, by `source` as written.
#[derive(Clone, Default)]
pub(crate) struct Loaded {
    luts: HashMap<String, Lut>,
}

/// The slots `layer`'s effects read as maps (`"source": "slot:<name>"`), so
/// that `prepare` loads them.
pub(crate) fn slots_used(layer: &Layer) -> impl Iterator<Item = &str> {
    layer.effects.iter().filter_map(|e| {
        let spec = edits_core::effects::spec(&e.kind)?;
        (spec.source == SourceKind::Slot).then_some(())?;
        e.source.as_deref()?.strip_prefix("slot:")
    })
}

impl Renderer {
    /// Load what the project's built-in effects read from files: the `.cube`
    /// tables of `lut` effects (paths relative to `base_dir`).
    pub(crate) fn load_fx_inputs(&mut self, project: &Project, base_dir: &Path) -> Result<Loaded> {
        let mut out = Loaded::default();
        for l in project.all_layers() {
            for (i, e) in l.effects.iter().enumerate() {
                if edits_core::effects::spec(&e.kind).map(|s| s.source) != Some(SourceKind::Cube) {
                    continue;
                }
                let place = format!("layer \"{}\", effect {i}", l.id);
                let file = e.source.as_deref().ok_or_else(|| anyhow!("{place}: a {} effect needs \"source\" (a .cube file)", e.kind))?;
                if out.luts.contains_key(file) {
                    continue;
                }
                let cube = edits_core::cube::load(&base_dir.join(file)).map_err(|err| anyhow!("{place}: {err}"))?;
                let (size, rows) = cube.to_3d();
                let lut = self.upload_lut(size as u32, &rows).map_err(|err| anyhow!("{place}: {err}"))?;
                out.luts.insert(file.to_string(), lut);
            }
        }
        Ok(out)
    }

    /// The step of built-in effect `b` for `e` at the current instant, or
    /// `None` when it would change nothing.
    pub(crate) fn plan_builtin(&mut self, layer: &str, b: &'static Builtin, e: &ResolvedEffect, h: f32) -> Result<Option<Step>> {
        let spec = edits_core::effects::spec(b.kind).ok_or_else(|| anyhow!("effect \"{}\" is not in the registry", b.kind))?;
        let scene = self.scene.clone();
        let seconds = self.ctx.time[0] as f64;
        let t = scene.as_ref().map_or(seconds, |s| s.project.from_seconds(seconds));
        let Some(p) = (b.pack)(e, &Frame { h, aspect: self.ctx.time[3], t }) else { return Ok(None) };
        let mut inputs = Inputs::default();
        match spec.source {
            SourceKind::Cube => {
                let file = e.source.as_deref().unwrap_or("");
                let lut = scene.as_ref().and_then(|s| s.loaded.luts.get(file)).ok_or_else(|| anyhow!("layer \"{layer}\": lookup table \"{file}\" was not prepared"))?;
                inputs.lut = Some(lut.clone());
            }
            SourceKind::Slot => {
                let name = e.source.as_deref().and_then(|s| s.strip_prefix("slot:")).unwrap_or("");
                let tex = scene
                    .as_ref()
                    .and_then(|s| s.slot_texture(name))
                    .ok_or_else(|| anyhow!("layer \"{layer}\": the map \"slot:{name}\" was not prepared (write \"slot:<name>\" naming a slot)"))?;
                inputs.aux = Some(tex);
            }
            SourceKind::None | SourceKind::Shader => {}
        }
        let key = option_key(spec, e);
        let pipeline = self.builtin_pipeline(b, &key).map_err(|err| anyhow!("layer \"{layer}\": {err}"))?;
        Ok(Some(Step::Builtin(pipeline, p, Box::new(inputs))))
    }
}

/// Every built-in one-pass effect.
pub(crate) fn all() -> impl Iterator<Item = &'static Builtin> {
    distort::BUILTINS.iter().chain(stylize::BUILTINS).chain(color::BUILTINS).chain(generate::BUILTINS)
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
/// shared helpers, the effect, the shared contract and the entry point.
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
    out.push_str(LIB);
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

/// Two numbers of a parameter, each clamped to its range in the registry.
pub(crate) fn vec2(e: &ResolvedEffect, name: &str) -> [f32; 2] {
    let v = e.vec(name, 2);
    [clamped(e, name, v[0]), clamped(e, name, v[1])]
}

/// A colour parameter as three numbers.
pub(crate) fn rgb(e: &ResolvedEffect, name: &str) -> [f32; 3] {
    let v = e.vec(name, 3);
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

/// The value of a parameter clamped to its range in the registry.
pub(crate) fn num(e: &ResolvedEffect, name: &str) -> f32 {
    clamped(e, name, e.num(name))
}

fn clamped(e: &ResolvedEffect, name: &str, v: f64) -> f32 {
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

/// The fractional part of `x` (0..1), taken in f64 so that long times keep
/// their precision before they reach the shader.
pub(crate) fn fract(x: f64) -> f32 {
    if x.is_finite() {
        (x - x.floor()) as f32
    } else {
        0.0
    }
}

/// A seed parameter as a whole number the shader's hashes take.
pub(crate) fn seed(e: &ResolvedEffect) -> f32 {
    num(e, "seed").round().clamp(-1.0e6, 1.0e6)
}

/// Sixteen numbers from the first few.
pub(crate) fn params(head: &[f32]) -> Params {
    let mut p = [0.0; 16];
    p[..head.len()].copy_from_slice(head);
    p
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

    #[test]
    fn helpers_clamp_and_wrap() {
        assert_eq!(fract(-0.25), 0.75);
        assert_eq!(fract(1.0e9 + 0.5), 0.5);
        assert_eq!(fract(f64::NAN), 0.0);
        assert_eq!(params(&[1.0, 2.0])[..3], [1.0, 2.0, 0.0]);
    }
}
