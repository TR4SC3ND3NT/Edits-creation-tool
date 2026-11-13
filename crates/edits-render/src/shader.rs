//! Checking a custom effect shader without a GPU.
//!
//! A custom effect is a WGSL file that defines
//! `fn effect(uv: vec2<f32>) -> vec4<f32>`; the renderer appends the rest
//! (`shaders/custom.wgsl`: the `fx` inputs, `src()`, `src_mirror()` and the
//! entry points) and compiles the whole. [`check_custom_effect`] does the same
//! with naga, the WGSL compiler wgpu uses, and reports problems with line
//! and column numbers in the file as written.

use serde::Serialize;

/// The code appended to a custom effect.
pub const CUSTOM_CONTRACT: &str = include_str!("shaders/custom.wgsl");

#[derive(Serialize, Clone, Debug)]
pub struct ShaderIssue {
    /// 1-based line in the file, when the problem is in it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// The line of the file the problem is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct ShaderReport {
    pub ok: bool,
    pub issues: Vec<ShaderIssue>,
    /// The parameters the shader reads (`fx.p0` .. `fx.p7`).
    pub params: Vec<String>,
    /// Whether it reads the time (`fx.time`, `fx.beats`, `fx.frame`), so it changes on every frame.
    pub animated: bool,
}

fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(inner) = src {
        let text = inner.to_string();
        if !s.contains(&text) {
            s.push_str(": ");
            s.push_str(&text);
        }
        src = inner.source();
    }
    s
}

/// Check a custom effect the way the renderer will compile it.
pub fn check_custom_effect(text: &str) -> ShaderReport {
    let combined = format!("{text}\n{CUSTOM_CONTRACT}");
    let own_lines = text.lines().count() as u32;
    let params: Vec<String> = (0..8).map(|i| format!("p{i}")).filter(|p| text.contains(&format!("fx.{p}"))).collect();
    let animated = ["fx.time", "fx.beats", "fx.frame"].iter().any(|k| text.contains(k));
    let issue = |loc: Option<naga::SourceLocation>, message: String| -> ShaderIssue {
        match loc {
            Some(l) if l.line_number <= own_lines => ShaderIssue {
                line: Some(l.line_number),
                column: Some(l.line_position),
                code: text.lines().nth(l.line_number as usize - 1).map(|c| c.trim().chars().take(160).collect()),
                message,
            },
            Some(_) => ShaderIssue {
                line: None,
                column: None,
                code: None,
                message: format!("{message} (in the code the renderer appends: the file must define `fn effect(uv: vec2<f32>) -> vec4<f32>` and must not declare `fx`, `src`, `src_texture`, `edits_vs` or `edits_fs` itself)"),
            },
            None => ShaderIssue { line: None, column: None, code: None, message },
        }
    };
    let mut issues = Vec::new();
    match naga::front::wgsl::parse_str(&combined) {
        Err(e) => issues.push(issue(e.location(&combined), e.message().to_string())),
        Ok(module) => {
            let effect = module.functions.iter().map(|(_, f)| f).find(|f| f.name.as_deref() == Some("effect"));
            match effect {
                None => issues.push(issue(None, "the file does not define `fn effect(uv: vec2<f32>) -> vec4<f32>`".into())),
                Some(f) => {
                    let is = |h: naga::Handle<naga::Type>, size: naga::VectorSize| matches!(module.types[h].inner, naga::TypeInner::Vector { size: s, scalar } if s == size && scalar == naga::Scalar::F32);
                    let args_ok = f.arguments.len() == 1 && is(f.arguments[0].ty, naga::VectorSize::Bi);
                    let result_ok = f.result.as_ref().is_some_and(|r| is(r.ty, naga::VectorSize::Quad));
                    if !args_ok || !result_ok {
                        issues.push(issue(None, "`effect` must take one `vec2<f32>` (the uv) and return a `vec4<f32>` (linear, premultiplied colour)".into()));
                    }
                }
            }
            if issues.is_empty() {
                let mut v = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default());
                if let Err(e) = v.validate(&module) {
                    // The last span is the most specific (the expression, not the function).
                    let spans: Vec<&(naga::Span, String)> = e.spans().filter(|(s, _)| s.is_defined()).collect();
                    let loc = spans.last().map(|(s, _)| s.location(&combined));
                    let labels: Vec<&str> = spans.iter().rev().map(|(_, d)| d.as_str()).filter(|d| !d.is_empty()).collect();
                    let mut message = chain(e.as_inner());
                    if !labels.is_empty() {
                        message.push_str(&format!(" ({})", labels.join("; ")));
                    }
                    issues.push(issue(loc, message));
                }
            }
        }
    }
    ShaderReport { ok: issues.is_empty(), issues, params, animated }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_shaders_pass() {
        for text in [include_str!("../../../examples/shaders/ripple.wgsl"), include_str!("../../../examples/shaders/scanlines.wgsl")] {
            let r = check_custom_effect(text);
            assert!(r.ok, "{:?}", r.issues);
            assert!(r.params.contains(&"p0".to_string()));
        }
    }

    #[test]
    fn errors_point_at_the_line_in_the_file() {
        let r = check_custom_effect("fn effect(uv: vec2<f32>) -> vec4<f32> {\n    let c = src(uv);\n    return c * nope;\n}\n");
        assert!(!r.ok);
        assert_eq!(r.issues[0].line, Some(3), "{:?}", r.issues);
        assert!(r.issues[0].code.as_deref().unwrap().contains("nope"));
        let r = check_custom_effect("fn effect(uv: vec2<f32>) -> vec4<f32> {\n    return vec3<f32>(1.0);\n}\n");
        assert!(!r.ok && r.issues[0].line == Some(2), "{:?}", r.issues);
        let r = check_custom_effect("fn wrong(uv: vec2<f32>) -> vec4<f32> { return src(uv); }\n");
        assert!(!r.ok && r.issues[0].message.contains("fn effect"), "{:?}", r.issues);
        let r = check_custom_effect("fn effect(uv: vec2<f32>) -> f32 { return 1.0; }\n");
        assert!(!r.ok && r.issues[0].message.contains("vec4"), "{:?}", r.issues);
    }
}
