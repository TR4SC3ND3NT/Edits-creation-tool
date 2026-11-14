// bleach_bypass: the colour overlaid with its own luminance (display
// values): multiply below a luminance of 0.45, screen above 0.55, a blend
// between, which raises contrast and mutes colour.
//
// Ported from three.js (https://github.com/mrdoob/three.js),
// examples/jsm/shaders/BleachBypassShader.js. Copyright (c) 2010-2026
// three.js authors. MIT License (see THIRD_PARTY.md). Altered: straight
// display values of a premultiplied linear picture, WGSL.
//
// p0 = amount (three.js's opacity)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    let lum = dot(d, vec3<f32>(0.2126729, 0.7151522, 0.0721750));
    let blend = vec3<f32>(lum);
    let l = clamp(10.0 * (lum - 0.45), 0.0, 1.0);
    let r1 = 2.0 * d * blend;
    let r2 = vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - blend) * (vec3<f32>(1.0) - d);
    return fxl_from_display(mix(d, mix(r1, r2, l), fx.p0), c.a);
}
