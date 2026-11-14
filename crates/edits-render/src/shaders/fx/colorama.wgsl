// colorama: a phase taken from each pixel (display values), repeated
// `cycles` times and shifted by `phase`, picks a colour of a palette that
// the packer bakes into a table (display values, one cycle over 0..1); the
// result is mixed with the original by `amount`. A phase that lands exactly
// on the end of a cycle (white with one cycle) takes the palette's end, not
// its start.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/color_fx.rs (Colorama: phase sources, output cycle and
// its palettes). Copyright (c) 2026 ArtCraft Team and the EffectCraft
// contributors. MIT License (see THIRD_PARTY.md). Altered: the palette is
// sampled from a table, every channel takes the palette colour, an amount
// in place of Blend With Original, WGSL.
//
// p0 = phase shift (turns)  p1 = cycles  p2 = amount
// OPT_INPUT: INPUT_LUMINANCE, INPUT_HUE, INPUT_SATURATION, INPUT_RED, INPUT_GREEN, INPUT_BLUE

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let d = fxl_display(c);
    var p = 0.0;
    switch OPT_INPUT {
        case INPUT_HUE: { p = fxl_rgb2hsv(max(d, vec3<f32>(0.0))).x; }
        case INPUT_SATURATION: { p = fxl_rgb2hsv(max(d, vec3<f32>(0.0))).y; }
        case INPUT_RED: { p = d.r; }
        case INPUT_GREEN: { p = d.g; }
        case INPUT_BLUE: { p = d.b; }
        default: { p = fxl_display_luma(c); }
    }
    let ph = clamp(p, 0.0, 1.0) * fx.p1 + fx.p0;
    var t = fract(ph);
    if t == 0.0 && ph > 0.0 {
        t = 1.0;
    }
    return fxl_from_display(mix(d, fxl_table(t, 0u), fx.p2), c.a);
}
