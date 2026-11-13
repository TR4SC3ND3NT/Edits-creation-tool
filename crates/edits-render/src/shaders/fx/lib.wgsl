// ---- helpers shared by the built-in effects (shaders/fx/*.wgsl) ----------
// Prepended to every built-in effect, after its option constants; not part
// of the custom-effect contract. Written for edits. License: MIT OR
// Apache-2.0, as the rest of the repository.

const FXL_TAU: f32 = 6.2831853;
const FXL_PI: f32 = 3.14159265;

// Linear -> sRGB transfer, sign preserving and extended above 1.
fn fxl_enc1(v: f32) -> f32 {
    let a = abs(v);
    return select(1.055 * pow(a, 1.0 / 2.4) - 0.055, a * 12.92, a <= 0.0031308) * sign(v);
}
fn fxl_dec1(v: f32) -> f32 {
    let a = abs(v);
    return select(pow((a + 0.055) / 1.055, 2.4), a / 12.92, a <= 0.04045) * sign(v);
}
// Linear light -> display (sRGB-encoded) values, and back.
fn fxl_enc(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(fxl_enc1(c.r), fxl_enc1(c.g), fxl_enc1(c.b));
}
fn fxl_dec(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(fxl_dec1(c.r), fxl_dec1(c.g), fxl_dec1(c.b));
}

// The display value (straight colour, sRGB-encoded) of a premultiplied pixel.
fn fxl_display(c: vec4<f32>) -> vec3<f32> {
    return fxl_enc(fx_straight(c));
}
// A display value with alpha `a` -> premultiplied linear light.
fn fxl_from_display(d: vec3<f32>, a: f32) -> vec4<f32> {
    return vec4<f32>(fxl_dec(d) * a, a);
}

// Display (sRGB-encoded) luminance of a premultiplied pixel, 0..1 for ordinary pictures.
fn fxl_display_luma(c: vec4<f32>) -> f32 {
    return fxl_enc1(max(fx_luma(fx_straight(c)), 0.0));
}

// Smooth value noise in 0..1 that also varies along a third axis `z`
// (features one unit apart along each axis).
fn fxl_noise3(p: vec2<f32>, z: f32, seed: f32) -> f32 {
    let zi = floor(z);
    let f = z - zi;
    let s = f * f * (3.0 - 2.0 * f);
    let a = fx_noise(p + vec2<f32>(zi * 37.17, zi * 17.31), seed);
    let b = fx_noise(p + vec2<f32>((zi + 1.0) * 37.17, (zi + 1.0) * 17.31), seed);
    return mix(a, b, s);
}

// Fractal noise: `octaves` (1..8, fractional allowed) of `fxl_noise3`, each
// at twice the frequency and half the weight of the last, normalised to 0..1.
fn fxl_fbm(p: vec2<f32>, z: f32, seed: f32, octaves: f32) -> f32 {
    let n = i32(ceil(clamp(octaves, 1.0, 8.0)));
    let last = clamp(octaves, 1.0, 8.0) - f32(n - 1);
    var sum = 0.0;
    var norm = 0.0;
    var amp = 1.0;
    var freq = 1.0;
    for (var o = 0; o < n; o++) {
        let w = amp * select(1.0, last, o == n - 1);
        // Each octave is offset and turned a little so the grids do not line up.
        let q = vec2<f32>(p.x * 0.8 - p.y * 0.6, p.x * 0.6 + p.y * 0.8) * freq + vec2<f32>(f32(o) * 19.7, f32(o) * 7.3);
        sum += fxl_noise3(select(p * freq, q, o > 0), z + f32(o) * 5.17, seed + f32(o) * 101.0) * w;
        norm += w;
        amp *= 0.5;
        freq *= 2.0;
    }
    return sum / max(norm, 1e-6);
}

// A hash of a whole-number value and a seed to 0..1 (for random levels per step).
fn fxl_rand(i: f32, j: f32, seed: f32) -> f32 {
    return fx_hash(bitcast<u32>(i32(i)), bitcast<u32>(i32(j)), bitcast<u32>(i32(seed)));
}

// Generator blend: straight colour `s` over premultiplied `b`, keeping b's
// transparency. mode: 0 replace, 1 multiply, 2 screen, 3 add, 4 overlay.
fn fxl_generate(b: vec4<f32>, s: vec3<f32>, mode: u32, amount: f32) -> vec4<f32> {
    let base = fx_straight(b);
    var o = s;
    if mode == 1u {
        o = base * s;
    } else if mode == 2u {
        o = base + s - clamp(base, vec3<f32>(0.0), vec3<f32>(1.0)) * clamp(s, vec3<f32>(0.0), vec3<f32>(1.0));
    } else if mode == 3u {
        o = base + s;
    } else if mode == 4u {
        o = select(vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - base) * (vec3<f32>(1.0) - s), 2.0 * base * s, base < vec3<f32>(0.5));
    }
    return vec4<f32>(mix(base, o, amount) * b.a, b.a);
}
