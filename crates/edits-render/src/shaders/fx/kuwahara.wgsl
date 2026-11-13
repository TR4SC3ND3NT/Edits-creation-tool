// kuwahara: each pixel takes the mean colour of whichever of the four
// squares meeting at it has the least variance, so flat areas are smoothed
// and edges are kept: an oil-paint look.
//
// After GPUImage (https://github.com/BradLarson/GPUImage),
// GPUImageKuwaharaFilter.m (four quadrants, mean and variance, the calmest
// wins). Copyright (c) 2012, Brad Larson, Ben Cochran, Hugues Lismonde,
// Keitaroh Kobayashi, Alaric Cole, Matthew Clark, Jacob Gundersen, Chris
// Williams. All rights reserved. BSD-3-Clause License (see THIRD_PARTY.md).
// Altered: one pass with a fixed 4x4 grid of samples per square, spaced over
// the radius (resolution independent, so large radii are sampled coarsely);
// premultiplied linear colour; WGSL.
//
// p0 = radius (composition units)  p1 = amount

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let step = fx_delta_uv(vec2<f32>(fx.p0 / 3.0));
    var best = c;
    var best_var = 1e30;
    for (var k = 0; k < 4; k++) {
        let dir = vec2<f32>(select(-1.0, 1.0, (k & 1) == 1), select(-1.0, 1.0, (k & 2) == 2));
        var sum = vec4<f32>(0.0);
        var sq = vec3<f32>(0.0);
        for (var j = 0; j < 4; j++) {
            for (var i = 0; i < 4; i++) {
                let s = src(uv + step * dir * vec2<f32>(f32(i), f32(j)));
                sum += s;
                sq += s.rgb * s.rgb;
            }
        }
        let mean = sum / 16.0;
        let v = sq / 16.0 - mean.rgb * mean.rgb;
        let total = v.r + v.g + v.b;
        if total < best_var {
            best_var = total;
            best = mean;
        }
    }
    return mix(c, best, fx.p1);
}
