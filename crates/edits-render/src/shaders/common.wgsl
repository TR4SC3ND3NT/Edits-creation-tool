// Shared by every built-in pass. One 512-byte uniform block per pass and two
// source textures. All colour is linear light with premultiplied alpha.

struct U {
    mvp: mat4x4<f32>,
    // Target width, height, 1 / width, 1 / height (pixels).
    res: vec4<f32>,
    // Seconds, beats, frame index, aspect ratio (width / height) of the frame.
    time: vec4<f32>,
    p: array<vec4<f32>, 26>,
}

@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var t0: texture_2d<f32>;
@group(1) @binding(1) var t1: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
@group(1) @binding(3) var samp_mirror: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One triangle covering the target. uv (0, 0) is the top-left texel.
@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> VOut {
    var out: VOut;
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// Linear -> sRGB transfer, sign preserving and extended above 1.
fn enc1(v: f32) -> f32 {
    let a = abs(v);
    var r: f32;
    if a <= 0.0031308 {
        r = a * 12.92;
    } else {
        r = 1.055 * pow(a, 1.0 / 2.4) - 0.055;
    }
    return r * sign(v);
}
fn dec1(v: f32) -> f32 {
    let a = abs(v);
    var r: f32;
    if a <= 0.04045 {
        r = a / 12.92;
    } else {
        r = pow((a + 0.055) / 1.055, 2.4);
    }
    return r * sign(v);
}
fn enc(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(enc1(c.r), enc1(c.g), enc1(c.b));
}
fn dec(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dec1(c.r), dec1(c.g), dec1(c.b));
}

fn src0(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(t0, samp, uv, 0.0);
}

// Layer quads: top-left, top-right, bottom-right, top-left, bottom-right,
// bottom-left of the unit square, placed by `u.mvp`.
@vertex
fn vs_geo(@builtin(vertex_index) i: u32) -> VOut {
    // Top-left, top-right, bottom-right, top-left, bottom-right, bottom-left.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, 0.5), vec2<f32>(-0.5, 0.5));
    let c = corners[i];
    var out: VOut;
    out.pos = u.mvp * vec4<f32>(c, 0.0, 1.0);
    out.uv = c + vec2<f32>(0.5, 0.5);
    return out;
}

// A hash of three integers to 0..1; exact, so grain is the same on every adapter.
fn hash3(x: u32, y: u32, z: u32) -> f32 {
    var h = x * 0x9E3779B1u + y * 0x85EBCA77u + z * 0xC2B2AE3Du;
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    h *= 0x297A2D39u;
    h ^= h >> 15u;
    return f32(h >> 8u) / 16777216.0;
}
