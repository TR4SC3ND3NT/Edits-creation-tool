
// ---- appended by the renderer after a custom effect's source ----------------
// The user file provides:  fn effect(uv: vec2<f32>) -> vec4<f32>

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
}

@group(0) @binding(0) var<uniform> fx: EditsFx;
@group(1) @binding(0) var src_texture: texture_2d<f32>;
@group(1) @binding(1) var edits_aux_texture: texture_2d<f32>;
@group(1) @binding(2) var src_sampler: sampler;
@group(1) @binding(3) var src_mirror_sampler: sampler;

// The image the effect is applied to: linear light, premultiplied alpha.
fn src(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src_texture, src_sampler, uv, 0.0);
}
// Same, mirrored at the edges instead of clamped.
fn src_mirror(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src_texture, src_mirror_sampler, uv, 0.0);
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

@fragment
fn edits_fs(in: EditsVOut) -> @location(0) vec4<f32> {
    return effect(in.uv);
}
