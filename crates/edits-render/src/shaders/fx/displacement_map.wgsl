// displacement_map: each pixel reads the picture pushed by the colour of a
// map (a slot's picture stretched over the frame): a display value of 0.5
// pushes nothing, 1 pushes the full amount, 0 the full amount the other way.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/distort2.rs (Displacement Map). Copyright (c) 2026
// ArtCraft Team and the EffectCraft contributors. MIT License (see
// THIRD_PARTY.md). Altered: the map is always stretched to the frame, amounts
// in % of height, WGSL.
//
// p0, p1 = largest push across and down (composition units)
// OPT_CHANNEL_X, OPT_CHANNEL_Y: which map channel pushes along each axis

// Map channel by index: 0 red, 1 green, 2 blue, 3 alpha, 4 luminance, 5 off.
fn dm_channel(m: vec4<f32>, which: u32) -> f32 {
    let d = fxl_display(m);
    switch which {
        case 0u: { return d.r; }
        case 1u: { return d.g; }
        case 2u: { return d.b; }
        case 3u: { return m.a; }
        case 4u: { return fxl_display_luma(m); }
        default: { return 0.5; }
    }
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let m = fx_aux(uv);
    // channel_y lists green first: map its index onto the same order as channel_x.
    var y = OPT_CHANNEL_Y;
    if y == CHANNEL_Y_GREEN {
        y = 1u;
    } else if y == CHANNEL_Y_RED {
        y = 0u;
    }
    let push = (vec2<f32>(dm_channel(m, OPT_CHANNEL_X), dm_channel(m, y)) - vec2<f32>(0.5)) * 2.0 * vec2<f32>(fx.p0, fx.p1);
    return src(uv + fx_delta_uv(push));
}
