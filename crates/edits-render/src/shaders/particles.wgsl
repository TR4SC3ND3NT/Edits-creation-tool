// Particle sprites: one instanced draw per particles layer (see particles.rs).
// Each instance is a quad in layer space: centre, unit axis (its length
// direction), half extents along the axis and across it, and a linear
// colour with its opacity. The quad goes through the layer's model, view and
// projection like any flat layer. Output is linear, premultiplied, blended
// "over" into the layer's own transparent texture.
//
// The sprite shapes are our own. (The particle state, curl-noise drift
// included, is computed on the CPU in edits-core/src/particles.rs.)

struct PU {
    mvp: mat4x4<f32>,
    // Target width, height, 1 / width, 1 / height.
    res: vec4<f32>,
    // x: sprite (0 dot, 1 spark, 2 star, 3 petal, 4 picture);
    // y: layer units per pixel (for the smallest size drawn).
    info: vec4<f32>,
}

@group(0) @binding(0) var<uniform> pu: PU;
@group(0) @binding(1) var sprite_tex: texture_2d<f32>;
@group(0) @binding(2) var sprite_samp: sampler;

struct PIn {
    @location(0) center: vec2<f32>,
    @location(1) axis: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) color: vec4<f32>,
}

struct POut {
    @builtin(position) pos: vec4<f32>,
    // -1..1 over the quad: x along the axis, y across it (down when the axis points right).
    @location(0) q: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_particle(@builtin(vertex_index) i: u32, p: PIn) -> POut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let c = corners[i];
    // Nothing is drawn smaller than a pixel and a half across: a smaller
    // particle is drawn at that size, fainter by the area it lacks, so it
    // neither flickers nor vanishes between pixels.
    let least = 0.75 * pu.info.y;
    var h = abs(p.half_size);
    var alpha = p.color.a;
    if h.x < least {
        alpha = alpha * h.x / least;
        h.x = least;
    }
    if h.y < least {
        alpha = alpha * h.y / least;
        h.y = least;
    }
    let across = vec2<f32>(-p.axis.y, p.axis.x);
    let at = p.center + p.axis * (c.x * h.x) + across * (c.y * h.y);
    var out: POut;
    out.pos = pu.mvp * vec4<f32>(at, 0.0, 1.0);
    out.q = c;
    out.color = vec4<f32>(p.color.rgb, alpha);
    return out;
}

// Coverage of the inside of a signed distance `d` (negative inside),
// faded over about a pixel.
fn inside(d: f32) -> f32 {
    return clamp(0.5 - d / max(fwidth(d), 1e-4), 0.0, 1.0);
}

@fragment
fn fs_particle(in: POut) -> @location(0) vec4<f32> {
    let q = in.q;
    let kind = u32(pu.info.x + 0.5);
    let r = length(q);
    var a = 0.0;
    var shade = 1.0;
    switch kind {
        case 0u: {
            // A round dot with a soft rim.
            a = 1.0 - smoothstep(0.45, 1.0, r);
        }
        case 1u: {
            // A streak: bright head at +x, fading tail, thin across.
            let across = 1.0 - smoothstep(0.15, 1.0, abs(q.y));
            let ends = 1.0 - smoothstep(0.7, 1.0, abs(q.x));
            a = across * ends * mix(0.12, 1.0, smoothstep(-1.0, 0.85, q.x));
        }
        case 2u: {
            // A four-point twinkle: a small core and two thin rays.
            let core = exp(-r * r * 14.0);
            let rays = exp(-abs(q.y) * 16.0) * max(0.0, 1.0 - abs(q.x)) + exp(-abs(q.x) * 16.0) * max(0.0, 1.0 - abs(q.y));
            a = clamp(core + rays, 0.0, 1.0);
        }
        case 3u: {
            // A sakura petal along x, base at -1, notched tip at +1.
            let x = q.x;
            let y = abs(q.y);
            let w = sqrt(max(0.0, 1.0 - x * x)) * (0.72 + 0.28 * x);
            a = inside(y - w) * (1.0 - inside(0.74 + 1.5 * y - x));
            // Deeper colour at the base.
            shade = mix(0.8, 1.05, smoothstep(-1.0, 0.6, x));
        }
        default: {
            let s = textureSample(sprite_tex, sprite_samp, q * 0.5 + vec2<f32>(0.5, 0.5));
            return s * vec4<f32>(in.color.rgb, 1.0) * in.color.a;
        }
    }
    let k = a * in.color.a;
    return vec4<f32>(in.color.rgb * shade * k, k);
}
