// sharpen: robust contrast-adaptive sharpening (RCAS). A negative lobe on the
// four neighbours, as strong as it can be without pushing the pixel outside
// the range of its neighbours, so hard edges get no halo.
//
// After AMD FidelityFX FSR 1 RCAS, as used in Bevy
// (https://github.com/bevyengine/bevy,
// crates/bevy_anti_alias/src/contrast_adaptive_sharpening/robust_contrast_adaptive_sharpening.wesl).
// FidelityFX FSR: Copyright (c) 2021 Advanced Micro Devices, Inc., MIT
// License. Bevy: Copyright the Bevy contributors, MIT License (see
// THIRD_PARTY.md). Altered: written from the published RCAS formula, the
// limits taken on colours clamped to 0..1, no denoise, `amount` scales the
// lobe directly, WGSL fragment `effect`.
//
// p0 = amount (0..1)

const RCAS_LIMIT: f32 = 0.1875;

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let px = fx.inv_resolution;
    let e = src(uv);
    let b = src(uv + vec2<f32>(0.0, -px.y)).rgb;
    let d = src(uv + vec2<f32>(-px.x, 0.0)).rgb;
    let f = src(uv + vec2<f32>(px.x, 0.0)).rgb;
    let h = src(uv + vec2<f32>(0.0, px.y)).rgb;
    let lo = vec3<f32>(0.0);
    let hi = vec3<f32>(1.0);
    let mn = min(min(clamp(b, lo, hi), clamp(d, lo, hi)), min(clamp(f, lo, hi), clamp(h, lo, hi)));
    let mx = max(max(clamp(b, lo, hi), clamp(d, lo, hi)), max(clamp(f, lo, hi), clamp(h, lo, hi)));
    let hit_min = mn / (4.0 * mx + vec3<f32>(1e-5));
    let hit_max = (hi - mx) / (4.0 * mn - vec3<f32>(4.0 + 1e-5));
    let lobe3 = max(-hit_min, hit_max);
    let lobe = max(-RCAS_LIMIT, min(max(lobe3.r, max(lobe3.g, lobe3.b)), 0.0)) * fx.p0;
    let rgb = (lobe * (b + d + f + h) + e.rgb) / (4.0 * lobe + 1.0);
    return vec4<f32>(max(rgb, vec3<f32>(0.0)), e.a);
}
