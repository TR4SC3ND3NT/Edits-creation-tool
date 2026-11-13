// wave_warp: waves travel across the picture; each line across the direction
// of travel is pushed sideways by the wave's height there.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/distort.rs `wave_shape`, `wave_pin` and `wave_warp`.
// Copyright (c) 2026 ArtCraft Team and the EffectCraft contributors. MIT
// License (see THIRD_PARTY.md). Altered: composition units with amplitude and
// wavelength in % of height, the phase in cycles computed on the CPU (speed in
// waves per time unit of the project), pinning against the frame, WGSL.
//
// p0 = amplitude  p1 = wavelength  p2, p3 = direction of travel (unit, y down)
// p4 = phase (cycles)  p5 = seed
// OPT_SHAPE: SHAPE_SINE .. SHAPE_SMOOTH_NOISE   OPT_PIN: PIN_NONE .. PIN_VERTICAL

// Half a circle over 0..1: 0 at the ends, 1 in the middle.
fn ww_half(u: f32) -> f32 {
    let x = 2.0 * u - 1.0;
    return sqrt(max(0.0, 1.0 - x * x));
}

// A random level in -1..1 for half wave `i`.
fn ww_level(i: f32) -> f32 {
    return fxl_rand(i, 0.0, fx.p5) * 2.0 - 1.0;
}

// The wave at `u` cycles, in -1..1.
fn ww_shape(u: f32) -> f32 {
    let ph = fract(u);
    if OPT_SHAPE == SHAPE_SQUARE {
        return select(-1.0, 1.0, ph < 0.5);
    }
    if OPT_SHAPE == SHAPE_TRIANGLE {
        return 1.0 - 4.0 * abs(ph - 0.5);
    }
    if OPT_SHAPE == SHAPE_SAWTOOTH {
        return 2.0 * ph - 1.0;
    }
    if OPT_SHAPE == SHAPE_CIRCLE {
        return select(-ww_half(2.0 * ph - 1.0), ww_half(2.0 * ph), ph < 0.5);
    }
    if OPT_SHAPE == SHAPE_SEMICIRCLE {
        return 2.0 * ww_half(fract(2.0 * ph)) - 1.0;
    }
    if OPT_SHAPE == SHAPE_UNCIRCLE {
        return 1.0 - 2.0 * ww_half(fract(2.0 * ph));
    }
    if OPT_SHAPE == SHAPE_NOISE {
        return ww_level(floor(2.0 * u));
    }
    if OPT_SHAPE == SHAPE_SMOOTH_NOISE {
        let v = 2.0 * u;
        let i = floor(v);
        let f = v - i;
        let s = f * f * (3.0 - 2.0 * f);
        return mix(ww_level(i), ww_level(i + 1.0), s);
    }
    return sin(FXL_TAU * u);
}

// How much of the push a point keeps, 0 where it is pinned.
fn ww_pin(uv: vec2<f32>) -> f32 {
    let h = clamp(2.0 * min(uv.y, 1.0 - uv.y), 0.0, 1.0);
    let v = clamp(2.0 * min(uv.x, 1.0 - uv.x), 0.0, 1.0);
    switch OPT_PIN {
        case PIN_ALL: { return min(h, v); }
        case PIN_CENTER: { return min(1.0, 2.0 * length(uv - vec2<f32>(0.5))); }
        case PIN_LEFT: { return clamp(uv.x, 0.0, 1.0); }
        case PIN_TOP: { return clamp(uv.y, 0.0, 1.0); }
        case PIN_RIGHT: { return clamp(1.0 - uv.x, 0.0, 1.0); }
        case PIN_BOTTOM: { return clamp(1.0 - uv.y, 0.0, 1.0); }
        case PIN_HORIZONTAL: { return h; }
        case PIN_VERTICAL: { return v; }
        default: { return 1.0; }
    }
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let p = fx_to_comp(uv);
    let dir = vec2<f32>(fx.p2, fx.p3);
    let along = dot(p, dir);
    let push = fx.p0 * ww_shape(along / fx.p1 + fx.p4) * ww_pin(uv);
    // Sideways: a quarter turn anticlockwise on screen from the direction of travel.
    let side = vec2<f32>(dir.y, -dir.x);
    return src(fx_to_uv(p + side * push));
}
