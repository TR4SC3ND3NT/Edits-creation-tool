// turbulent_displace: the picture pushed around by fractal noise.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/distort2.rs (Turbulent Displace: fbm of value noise, the
// twist and bulge modes from the noise's gradient). Copyright (c) 2026
// ArtCraft Team and the EffectCraft contributors. MIT License (see
// THIRD_PARTY.md). Altered: composition units with sizes in % of height, no
// pinning, our noise (shaders/fx/lib.wgsl), WGSL.
//
// p0 = largest push  p1 = 1 / size  p2 = octaves  p3 = evolution (steps)  p4 = seed
// OPT_MODE: MODE_TURBULENT, MODE_HORIZONTAL, MODE_VERTICAL, MODE_TWIST, MODE_BULGE

fn td_n(q: vec2<f32>, salt: f32) -> f32 {
    return fxl_fbm(q, fx.p3, fx.p4 + salt, fx.p2);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let p = fx_to_comp(uv);
    let q = p * fx.p1;
    var push: vec2<f32>;
    if OPT_MODE == MODE_TWIST || OPT_MODE == MODE_BULGE {
        let e = 0.05;
        let g = vec2<f32>(td_n(q + vec2<f32>(e, 0.0), 0.0) - td_n(q - vec2<f32>(e, 0.0), 0.0), td_n(q + vec2<f32>(0.0, e), 0.0) - td_n(q - vec2<f32>(0.0, e), 0.0)) / (2.0 * e);
        push = select(g, vec2<f32>(-g.y, g.x), OPT_MODE == MODE_TWIST) * 0.5;
    } else {
        let a = (td_n(q, 0.0) - 0.5) * 2.0;
        let b = (td_n(q + vec2<f32>(31.4, 17.9), 17.0) - 0.5) * 2.0;
        push = vec2<f32>(a, b);
        if OPT_MODE == MODE_HORIZONTAL {
            push.y = 0.0;
        } else if OPT_MODE == MODE_VERTICAL {
            push = vec2<f32>(0.0, a);
        }
    }
    return src(fx_to_uv(p + push * fx.p0));
}
