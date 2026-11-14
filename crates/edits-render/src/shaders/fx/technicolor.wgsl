// technicolor: the two-strip process: red stays, green and blue both become
// their average (display values), so the picture is reds and cyans.
//
// Ported from three.js (https://github.com/mrdoob/three.js),
// examples/jsm/shaders/TechnicolorShader.js. Copyright (c) 2010-2026
// three.js authors. MIT License (see THIRD_PARTY.md). Altered: the alpha is
// kept, an amount, straight display values of a premultiplied linear
// picture, WGSL.
//
// p0 = amount

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    let gb = (d.g + d.b) * 0.5;
    return fxl_from_display(mix(d, vec3<f32>(d.r, gb, gb), fx.p0), c.a);
}
