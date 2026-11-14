// gradient_map: recolour by display luminance. Tritone: black becomes the
// shadow colour, mid grey the midtone colour, white the highlight colour;
// duotone: shadows to highlights; stops: through the colours of `stops`,
// evenly spaced (baked into a table by the packer). Colours are mixed as
// display values.
//
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0..p2 = shadows  p3..p5 = midtones  p6..p8 = highlights (display)  p9 = amount
// OPT_MODE: MODE_TRITONE, MODE_DUOTONE or MODE_STOPS (the table's row 0)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let l = clamp(fxl_display_luma(c), 0.0, 1.0);
    let s = vec3<f32>(fx.p0, fx.p1, fx.p2);
    let m = vec3<f32>(fx.p3, fx.p4, fx.p5);
    let h = vec3<f32>(fx.p6, fx.p7, fx.p8);
    var d: vec3<f32>;
    if OPT_MODE == MODE_STOPS {
        d = fxl_table(l, 0u);
    } else if OPT_MODE == MODE_DUOTONE {
        d = mix(s, h, l);
    } else {
        d = select(mix(m, h, 2.0 * l - 1.0), mix(s, m, 2.0 * l), l < 0.5);
    }
    return mix(c, fxl_from_display(d, c.a), fx.p9);
}
