// lift_gamma_gain: colour wheels on display values, per channel: value +
// lift x (1 - value), times gain, raised to 1 / gamma; then saturation about
// the Rec. 709 luminance of the result.
//
// After ISF-Files LGG.fs, "by VIDVOX" (https://github.com/Vidvox/ISF-Files,
// ISF/LGG.fs). Copyright (c) 2018 VIDVOX. MIT License (see THIRD_PARTY.md).
// Altered: lift, gamma and gain as plain numbers (0, 1, 1 neutral) instead
// of colours offset by 0.5 and scaled by 4, the gamma exponent inverted (above
// 1 brightens, as `levels`), no clamping to 0..1 (values above 1 stay), WGSL.
//
// p0..p2 = lift  p3..p5 = 1 / gamma  p6..p8 = gain  p9 = saturation

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    var d = fxl_display(c);
    d = d + vec3<f32>(fx.p0, fx.p1, fx.p2) * (vec3<f32>(1.0) - d);
    d = d * vec3<f32>(fx.p6, fx.p7, fx.p8);
    d = pow(max(d, vec3<f32>(0.0)), vec3<f32>(fx.p3, fx.p4, fx.p5));
    let l = fxl_matte_luma(d);
    d = max(vec3<f32>(l) + fx.p9 * (d - vec3<f32>(l)), vec3<f32>(0.0));
    return fxl_from_display(d, c.a);
}
