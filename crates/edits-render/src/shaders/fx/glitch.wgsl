// glitch: digital glitch. At each change (rate per time unit) a random share
// of horizontal bands jumps sideways with its colours split, and a random
// share of square blocks is read from elsewhere with red and blue swapped.
// Everything is a hash of the band or block, the change number and the seed.
//
// Written for edits (the look of three.js DigitalGlitch, pmndrs GlitchEffect
// and EffectCraft's VR Digital Glitch; no code taken). License: MIT OR
// Apache-2.0, as the rest of the repository.
//
// p0 = share of bands  p1 = change number  p2 = bands  p3 = largest jump
// p4 = colour split  p5 = share of blocks  p6 = seed  (lengths in composition units)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let n = fx.p1;
    let seed = fx.p6;
    var at = uv;
    var swap = false;
    // Blocks: squares two bands tall.
    if fx.p5 > 0.0 {
        let size = 2.0 / fx.p2;
        let cell = floor(fx_to_comp(uv) / size);
        if fx_hash(bitcast<u32>(i32(cell.x)), bitcast<u32>(i32(cell.y)), bitcast<u32>(i32(n) * 7919 + i32(seed))) < fx.p5 {
            let jump = vec2<f32>(fxl_rand(cell.x * 31.0 + cell.y, n, seed + 3.0), fxl_rand(cell.y * 17.0 - cell.x, n, seed + 4.0)) - vec2<f32>(0.5);
            at = at + fx_delta_uv(floor(jump * 8.0) * size * 0.5);
            swap = true;
        }
    }
    let band = floor(uv.y * fx.p2);
    var shift = 0.0;
    var split = 0.0;
    if fxl_rand(band, n, seed) < fx.p0 {
        shift = (fxl_rand(band, n, seed + 1.0) * 2.0 - 1.0) * fx.p3;
        split = fx.p4 * select(-1.0, 1.0, fxl_rand(band, n, seed + 2.0) < 0.5);
    }
    let s = fx_delta_uv(vec2<f32>(shift, 0.0));
    let k = fx_delta_uv(vec2<f32>(split, 0.0));
    let mid = src(at + s);
    let red = src(at + s + k);
    let blue = src(at + s - k);
    var out = vec4<f32>(red.r, mid.g, blue.b, max(mid.a, max(red.a, blue.a)));
    if swap {
        out = out.bgra;
    }
    return out;
}
