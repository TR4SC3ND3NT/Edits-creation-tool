// glitch_bands: a cut-up digital glitch in the style measured on a finished
// edit. Each pattern (a new one every `hold` output frames) has up to
// `bands` horizontal bands, each over its own stretch of the width, that
// read the picture shifted sideways by up to `shift`, stretched about their
// middle, and (a share `smear`) with their top row repeated all the way
// down; and up to `blocks` small blocks that are flat colour taken from the
// picture or, a share `noise`, bricks of saturated colour on a dark base.
// Every band reads the input picture, not the already shifted one, as in an
// editor's effect. Everything is a hash of the slot, the pattern and the seed.
//
// After glitch4.py of the user's Death Note kit (bands, duplicated rows,
// fills, noise blocks). Written for edits. License: MIT OR Apache-2.0, as the
// rest of the repository.
//
// p0 = amount  p1 = bands  p2 = tallest band  p3 = shortest band (share of
// width)  p4 = largest shift  p5 = stretch  p6 = smear share  p7 = blocks
// p8 = widest block  p9 = noise share  p10 = brick saturation  p11 = hold
// p12 = seed  (lengths in composition units)

fn gb_rand(i: u32, k: u32) -> f32 {
    let pattern = u32(floor(fx.frame / max(fx.p11, 1.0)));
    return fx_hash(pattern * 0x9E37u + bitcast<u32>(i32(fx.p12)) * 7919u, i, k);
}

fn gb_palette(k: f32, sat: f32) -> vec3<f32> {
    // green, magenta, cyan, yellow, blue, white (display values)
    var c = vec3<f32>(1.0);
    if k < 0.3 {
        c = vec3<f32>(0.1, 1.0, 0.2);
    } else if k < 0.48 {
        c = vec3<f32>(1.0, 0.15, 0.9);
    } else if k < 0.64 {
        c = vec3<f32>(0.1, 0.9, 1.0);
    } else if k < 0.78 {
        c = vec3<f32>(1.0, 0.95, 0.1);
    } else if k < 0.9 {
        c = vec3<f32>(0.2, 0.3, 1.0);
    }
    let g = dot(c, vec3<f32>(0.299, 0.587, 0.114));
    return mix(vec3<f32>(g), c, sat);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let p = fx_to_comp(uv);
    let half_w = fx.aspect * 0.5;
    var at = p;
    for (var i = 0u; i < u32(fx.p1); i++) {
        if gb_rand(i, 0u) >= fx.p0 {
            continue;
        }
        let y0 = gb_rand(i, 1u) - 0.5;
        let h = fx.p2 * (0.15 + 0.85 * gb_rand(i, 2u));
        let w = mix(fx.p3, 1.0, gb_rand(i, 3u)) * fx.aspect;
        let x0 = -half_w + gb_rand(i, 4u) * (fx.aspect - w);
        if p.y < y0 || p.y >= y0 + h || p.x < x0 || p.x >= x0 + w {
            continue;
        }
        let u = (gb_rand(i, 5u) * 2.0 - 1.0) * fx.p4;
        let sx = pow(1.0 + fx.p5, gb_rand(i, 6u) * 2.0 - 1.0);
        let cx = x0 + w * 0.5;
        at = vec2<f32>(cx + (p.x - u - cx) / sx, select(p.y, y0, gb_rand(i, 7u) < fx.p6));
    }
    var out = src(fx_to_uv(at));
    for (var i = 0u; i < u32(fx.p7); i++) {
        if gb_rand(i, 20u) >= fx.p0 {
            continue;
        }
        let bw = fx.p8 * (0.25 + 0.75 * gb_rand(i, 21u));
        let bh = max(bw * (0.06 + 0.16 * gb_rand(i, 22u)), 0.004);
        let b0 = vec2<f32>(-half_w + gb_rand(i, 23u) * (fx.aspect - bw), gb_rand(i, 24u) - 0.5 - bh * 0.5);
        if p.x < b0.x || p.x >= b0.x + bw || p.y < b0.y || p.y >= b0.y + bh {
            continue;
        }
        if gb_rand(i, 25u) < fx.p9 {
            // Bricks of about 3.5 x 1.7 pixels at 576 high on a dark base.
            let cell = floor((p - b0) / vec2<f32>(0.006, 0.003));
            let hc = fx_hash(bitcast<u32>(i32(cell.x)) + i * 131u, bitcast<u32>(i32(cell.y)), u32(floor(fx.frame / max(fx.p11, 1.0))) + 5u);
            var d = vec3<f32>(0.05, 0.04, 0.05);
            if hc < 0.45 {
                d = gb_palette(hc / 0.45, fx.p10);
            }
            out = fxl_from_display(d, 1.0);
        } else {
            let c = src(fx_to_uv(b0 + vec2<f32>(bw, bh) * 0.5));
            out = vec4<f32>(fx_straight(c), 1.0);
        }
    }
    return out;
}
