// ink: manga lineart. A pixel becomes ink where it is darker than its
// surroundings (the mean of a small neighbourhood against a large one), or
// where it is nearly black; everything else becomes flat paper.
//
// After glfx.js (https://github.com/evanw/glfx.js), src/filters/fun/ink.js:
// the difference of a large and a small neighbourhood average. Copyright (C)
// 2011 by Evan Wallace. MIT License (see THIRD_PARTY.md). Altered: display
// luminance, sizes in % of height, a threshold, ink and paper colours, solid
// blacks kept, WGSL.
//
// p0 = strength  p1 = threshold  p2 = neighbourhood radius (composition units)
// p3 = amount  p4..p6 = ink (display)  p7..p9 = paper (display)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fx_delta_uv(vec2<f32>(max(fx.p2, 2.0 * fx_pixel()) * 0.5));
    var big = 0.0;
    var small = 0.0;
    for (var j = -2; j <= 2; j++) {
        for (var i = -2; i <= 2; i++) {
            let l = fxl_display_luma(src(uv + d * vec2<f32>(f32(i), f32(j))));
            big += l;
            if abs(i) + abs(j) <= 1 {
                small += l;
            }
        }
    }
    big = big / 25.0;
    small = small / 5.0;
    let line = clamp((big - small - fx.p1) * fx.p0 * 6.0, 0.0, 1.0);
    let solid = 1.0 - smoothstep(0.05, 0.15, small);
    let k = max(line, solid);
    let drawn = fxl_from_display(mix(vec3<f32>(fx.p7, fx.p8, fx.p9), vec3<f32>(fx.p4, fx.p5, fx.p6), k), c.a);
    return mix(c, drawn, fx.p3);
}
