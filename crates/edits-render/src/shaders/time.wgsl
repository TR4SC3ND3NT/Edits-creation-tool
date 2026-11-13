
// ---- time effects (see `time.rs`) ------------------------------------------

// echo: one more picture into the running result. The operators are after
// EffectCraft's `time_fx.rs` echo (https://github.com/storytold/effectcraft,
// MIT OR Apache-2.0); premultiplied pictures are scaled by their weight in
// all four channels.
// t0 = the result so far, t1 = this echo's picture.
// p[0] = weight, mode (0 add, 1 screen, 2 max, 3 min, 4 over_back,
//        5 over_front), 1 when this is the first picture (t0 is ignored)
@fragment
fn fs_echo(in: VOut) -> @location(0) vec4<f32> {
    let q = textureSampleLevel(t1, samp, in.uv, 0.0) * u.p[0].x;
    if u.p[0].z > 0.5 {
        return vec4<f32>(max(q.rgb, vec3<f32>(0.0)), clamp(q.a, 0.0, 1.0));
    }
    let a = src0(in.uv);
    let mode = u.p[0].y;
    var r: vec4<f32>;
    if mode < 0.5 {
        r = a + q;
    } else if mode < 1.5 {
        // Screen, kept from darkening values above 1.
        r = a + q - clamp(a, vec4<f32>(0.0), vec4<f32>(1.0)) * clamp(q, vec4<f32>(0.0), vec4<f32>(1.0));
    } else if mode < 2.5 {
        r = max(a, q);
    } else if mode < 3.5 {
        r = min(a, q);
    } else if mode < 4.5 {
        // The result so far stays in front; this echo goes behind it.
        r = a + q * (1.0 - clamp(a.a, 0.0, 1.0));
    } else {
        // This echo goes in front of the result so far.
        r = q + a * (1.0 - clamp(q.a, 0.0, 1.0));
    }
    return vec4<f32>(max(r.rgb, vec3<f32>(0.0)), clamp(r.a, 0.0, 1.0));
}

// time displacement: write the pixels whose map value picks slice p[0].x out
// of p[0].y, discard the others (the pass loads what the target holds).
// t0 = this slice's picture, t1 = the map (stretched over the frame).
// p[0].z = the map value of a transparent pixel (the neutral value)
@fragment
fn fs_time_pick(in: VOut) -> @location(0) vec4<f32> {
    let m = textureSampleLevel(t1, samp, in.uv, 0.0);
    var l = u.p[0].z;
    if m.a > 1e-4 {
        l = clamp(enc1(max(luma(m.rgb / m.a), 0.0)), 0.0, 1.0);
    }
    let k = round(l * (u.p[0].y - 1.0));
    if abs(k - u.p[0].x) > 0.25 {
        discard;
    }
    return src0(in.uv);
}
