// film_scratches: print damage as measured on a finished edit. On a frame
// that has them (chance p0), up to `count` thin vertical scratches over part
// of the height lighten the picture, and a few elongated dark specks of dust
// darken it; with `light_only` both show only on bright areas (scratches
// fade in between display luminance 20/255 and 80/255, dust in proportion
// to luminance up to 140/255), as on a print. Everything is hashed from the
// output frame and the seed.
//
// After post4 and dust() of the user's make_edit4.py (Death Note kit).
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = chance  p1 = count  p2 = longest scratch  p3 = width  p4 = brightness
// p5 = mean specks  p6 = speck radius  p7 = speck depth  p8 = light_only  p9 = seed
// (lengths in composition units)

fn fs_rand(a: u32, b: u32) -> f32 {
    return fx_hash(u32(fx.frame) * 7919u + a, b, bitcast<u32>(i32(fx.p9)) + 0x51ED27u);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    if c.a <= 0.0 {
        return c;
    }
    var d = fxl_display(c);
    let lum = fxl_display_luma(c);
    let p = fx_to_comp(uv);
    let px = fx_pixel();
    // Scratches.
    let gate_s = mix(1.0, clamp((lum - 0.078) / 0.235, 0.0, 1.0), fx.p8);
    var chance = fx.p0;
    for (var i = 0u; i < u32(fx.p1); i++) {
        if fs_rand(i, 1u) >= chance {
            break;
        }
        chance = chance * 0.5;
        let x = (fs_rand(i, 2u) - 0.5) * fx.aspect;
        let y0 = fs_rand(i, 3u) * 0.5 - 0.5;
        let len = fx.p2 * (0.334 + 0.666 * fs_rand(i, 4u));
        let w = max(fx.p3, px * 0.5);
        let across = 1.0 - smoothstep(w * 0.5, w * 0.5 + px, abs(p.x - x));
        let along = smoothstep(y0 - px, y0, p.y) * (1.0 - smoothstep(y0 + len, y0 + len + px, p.y));
        // A scratch narrower than a pixel is spread over one, as thinly.
        d = d + vec3<f32>(fx.p4 * across * along * gate_s * min(fx.p3 / px, 1.0));
    }
    // Dust.
    let gate_d = mix(1.0, clamp(lum / 0.55, 0.0, 1.0), fx.p8);
    let count = u32(round(fx.p5 * (0.5 + fs_rand(100u, 0u))));
    for (var i = 0u; i < min(count, 96u); i++) {
        let centre = vec2<f32>((fs_rand(i, 11u) - 0.5) * fx.aspect, fs_rand(i, 12u) - 0.5);
        let r = max(fx.p6 * (0.334 + 0.666 * fs_rand(i, 13u)), px * 0.5);
        let q = p - centre;
        if abs(q.x) > r * 2.5 + px || abs(q.y) > r * 2.5 + px {
            continue;
        }
        let a = fs_rand(i, 14u) * FXL_PI;
        let l = vec2<f32>(q.x * cos(a) + q.y * sin(a), -q.x * sin(a) + q.y * cos(a));
        let e = 0.8 + 1.4 * fs_rand(i, 15u);
        let dd = length(vec2<f32>(l.x / (r * e), l.y / r)) * r;
        let k = 1.0 - smoothstep(r - px * 0.5, r + px * 0.5, dd);
        d = d - vec3<f32>(fx.p7 * (0.6 + 0.4 * fs_rand(i, 16u)) * k * gate_d);
    }
    return fxl_from_display(max(d, vec3<f32>(0.0)), c.a);
}
