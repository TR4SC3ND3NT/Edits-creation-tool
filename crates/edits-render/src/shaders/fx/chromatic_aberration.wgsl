// chromatic_aberration: radial lens fringing. Each pixel averages the picture
// at points spread along the line toward the lens centre, weighting each point
// by a spectrum running from red (outward) through green to blue (inward), so
// colours fan out by a distance that grows with the distance from the centre.
//
// Ported from Bevy (https://github.com/bevyengine/bevy),
// crates/bevy_post_process/src/effect_stack/chromatic_aberration.wesl, which
// implements the technique of Playdead's INSIDE. Copyright the Bevy
// contributors. MIT License (see THIRD_PARTY.md). Altered: the spread is given
// at the frame's corners in % of height and centred on the pixel, a fixed
// sample count (resolution independent), the default red-green-blue LUT
// written as a function, alpha averaged, WGSL fragment `effect`.
//
// p0 = spread per unit of distance from the centre  p1, p2 = centre  p3 = samples

fn ca_weight(t: f32) -> vec3<f32> {
    return vec3<f32>(max(1.0 - 2.0 * t, 0.0), 1.0 - abs(2.0 * t - 1.0), max(2.0 * t - 1.0, 0.0));
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p1, fx.p2);
    // From the centre out to this pixel, times the spread.
    let spread = fx_delta_uv((fx_to_comp(uv) - c) * fx.p0);
    let n = max(i32(fx.p3), 2);
    var sum = vec3<f32>(0.0);
    var weights = vec3<f32>(0.0);
    var alpha = 0.0;
    for (var i = 0; i < n; i++) {
        let t = (f32(i) + 0.5) / f32(n);
        let s = src(uv + spread * (t - 0.5));
        let w = ca_weight(t);
        sum += s.rgb * w;
        weights += w;
        alpha += s.a;
    }
    return vec4<f32>(sum / weights, alpha / f32(n));
}
