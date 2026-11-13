// ---- the effect contract: appended by the renderer after an effect's source --
// The effect provides:  fn effect(uv: vec2<f32>) -> vec4<f32>
// Built-in effects (`shaders/fx/*.wgsl`) and custom effects share this file.

struct EditsFx {
    edits_reserved: mat4x4<f32>,
    // Size of the image in pixels, and its reciprocal.
    resolution: vec2<f32>,
    inv_resolution: vec2<f32>,
    // Project time in seconds and in beats, the output frame index, width / height.
    time: f32,
    beats: f32,
    frame: f32,
    aspect: f32,
    p0: f32,
    p1: f32,
    p2: f32,
    p3: f32,
    p4: f32,
    p5: f32,
    p6: f32,
    p7: f32,
    p8: f32,
    p9: f32,
    p10: f32,
    p11: f32,
    p12: f32,
    p13: f32,
    p14: f32,
    p15: f32,
}

@group(0) @binding(0) var<uniform> fx: EditsFx;
@group(1) @binding(0) var src_texture: texture_2d<f32>;
@group(1) @binding(1) var edits_aux_texture: texture_2d<f32>;
@group(1) @binding(2) var src_sampler: sampler;
@group(1) @binding(3) var src_mirror_sampler: sampler;
@group(1) @binding(4) var edits_lut_texture: texture_3d<f32>;

// The image the effect is applied to: linear light, premultiplied alpha.
fn src(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src_texture, src_sampler, uv, 0.0);
}
// Same, mirrored at the edges instead of clamped.
fn src_mirror(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src_texture, src_mirror_sampler, uv, 0.0);
}
// A second picture, when the effect has one (a map); otherwise opaque white.
fn fx_aux(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(edits_aux_texture, src_sampler, uv, 0.0);
}
// A colour through the effect's 3D lookup table (identity when it has none).
// `c` is clamped to 0..1.
fn fx_lut(c: vec3<f32>) -> vec3<f32> {
    let n = f32(textureDimensions(edits_lut_texture).x);
    let p = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * ((n - 1.0) / n) + vec3<f32>(0.5 / n);
    return textureSampleLevel(edits_lut_texture, src_sampler, p, 0.0).rgb;
}

// uv (0..1, y down) -> composition units (height 1, origin at the centre, y down).
fn fx_to_comp(uv: vec2<f32>) -> vec2<f32> {
    return (uv - vec2<f32>(0.5)) * vec2<f32>(fx.aspect, 1.0);
}
// Composition units -> uv.
fn fx_to_uv(p: vec2<f32>) -> vec2<f32> {
    return p / vec2<f32>(fx.aspect, 1.0) + vec2<f32>(0.5);
}
// A displacement in composition units -> the same displacement in uv.
fn fx_delta_uv(d: vec2<f32>) -> vec2<f32> {
    return d / vec2<f32>(fx.aspect, 1.0);
}
// One pixel of the image being rendered, in composition units.
fn fx_pixel() -> f32 {
    return fx.inv_resolution.y;
}

fn fx_luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}
// Premultiplied -> straight colour.
fn fx_straight(c: vec4<f32>) -> vec3<f32> {
    return c.rgb / max(c.a, 1e-5);
}

// A hash of three integers to 0..1: exact, so the same on every adapter.
fn fx_hash(x: u32, y: u32, z: u32) -> f32 {
    var h = x * 0x9E3779B1u + y * 0x85EBCA77u + z * 0xC2B2AE3Du;
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    h *= 0x297A2D39u;
    h ^= h >> 15u;
    return f32(h >> 8u) / 16777216.0;
}
// A hash of the integer cell that `p` falls in, and a seed, to 0..1.
fn fx_hash2(p: vec2<f32>, seed: f32) -> f32 {
    let i = vec2<i32>(floor(p));
    return fx_hash(bitcast<u32>(i.x), bitcast<u32>(i.y), u32(i32(seed)));
}
// Smooth value noise in 0..1 with features about one unit apart.
fn fx_noise(p: vec2<f32>, seed: f32) -> f32 {
    let i = floor(p);
    let f = p - i;
    let s = f * f * (vec2<f32>(3.0) - 2.0 * f);
    let a = fx_hash2(i, seed);
    let b = fx_hash2(i + vec2<f32>(1.0, 0.0), seed);
    let c = fx_hash2(i + vec2<f32>(0.0, 1.0), seed);
    let d = fx_hash2(i + vec2<f32>(1.0, 1.0), seed);
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

struct EditsVOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn edits_vs(@builtin(vertex_index) i: u32) -> EditsVOut {
    var out: EditsVOut;
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}
