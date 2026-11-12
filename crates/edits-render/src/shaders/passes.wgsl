// Passes with a fixed shader. Blending, colour effects and the glow's final
// add are generated per combination instead: see `compose.rs`.

// ---- geometry: one textured quad (an image, a solid or a cube face) --------
// p[0] = uv offset.xy, uv scale.xy   p[1] = premultiplied linear colour
// p[2].x = 1 to cut the picture off outside 0..1 (fit: contain)

fn geo_color(in: VOut) -> vec4<f32> {
    let uv = u.p[0].xy + in.uv * u.p[0].zw;
    var c = textureSample(t0, samp, uv);
    if u.p[2].x > 0.5 {
        let w = max(fwidth(uv), vec2<f32>(1e-6, 1e-6));
        let m = clamp((vec2<f32>(0.5, 0.5) - abs(uv - vec2<f32>(0.5, 0.5))) / w + vec2<f32>(0.5, 0.5), vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
        c = c * (m.x * m.y);
    }
    return c * u.p[1];
}

@fragment
fn fs_geo(in: VOut) -> @location(0) vec4<f32> {
    return geo_color(in);
}

// ---- copy / scale ----------------------------------------------------------
// p[0].x = multiplier. Also used to downsample (bilinear = 2x2 box), to
// upsample, to build mip levels and to accumulate motion blur samples with
// additive blending.

@fragment
fn fs_copy(in: VOut) -> @location(0) vec4<f32> {
    return src0(in.uv) * u.p[0].x;
}

// Two weighted textures at once: p[0].x and p[0].y. Halves the passes needed
// to add up motion blur samples.
@fragment
fn fs_copy2(in: VOut) -> @location(0) vec4<f32> {
    return src0(in.uv) * u.p[0].x + textureSampleLevel(t1, samp, in.uv, 0.0) * u.p[0].y;
}

// ---- separable gaussian blur -----------------------------------------------
// p[0].xy = one texel step along the blur axis (uv), p[0].z = sigma in texels,
// p[0].w = taps on each side. Neighbouring taps are fetched in pairs through
// the bilinear filter.

@fragment
fn fs_blur(in: VOut) -> @location(0) vec4<f32> {
    let d = u.p[0].xy;
    let inv = 1.0 / (2.0 * u.p[0].z * u.p[0].z);
    let r = i32(u.p[0].w);
    var acc = src0(in.uv);
    var wsum = 1.0;
    for (var i = 1; i <= r; i += 2) {
        let a = f32(i);
        let b = a + 1.0;
        let wa = exp(-a * a * inv);
        let wb = exp(-b * b * inv);
        let w = wa + wb;
        let o = (a * wa + b * wb) / w;
        acc += (src0(in.uv + d * o) + src0(in.uv - d * o)) * w;
        wsum += 2.0 * w;
    }
    return acc / wsum;
}

// ---- streak blurs: directional, zoom, spin ---------------------------------
// p[0] = (mode, taps, span, aspect)   p[1].xy = centre (uv)
// p[2].xy = full streak vector (uv) for mode 0; p[2].x = strength (mode 1) or
// radians (mode 2). `span` shrinks the streak so that repeated passes combine
// into one evenly sampled streak.

@fragment
fn fs_streak(in: VOut) -> @location(0) vec4<f32> {
    let mode = i32(u.p[0].x);
    let n = i32(u.p[0].y);
    let span = u.p[0].z;
    let asp = vec2<f32>(u.p[0].w, 1.0);
    let centre = u.p[1].xy;
    var acc = vec4<f32>(0.0);
    for (var i = 0; i < n; i++) {
        let t = ((f32(i) + 0.5) / f32(n) - 0.5) * span;
        var uv: vec2<f32>;
        if mode == 0 {
            uv = in.uv + u.p[2].xy * t;
        } else if mode == 1 {
            uv = centre + (in.uv - centre) * exp(u.p[2].x * t);
        } else {
            let a = u.p[2].x * t;
            let cs = cos(a);
            let sn = sin(a);
            let v = (in.uv - centre) * asp;
            uv = centre + vec2<f32>(v.x * cs - v.y * sn, v.x * sn + v.y * cs) / asp;
        }
        acc += src0(uv);
    }
    return acc / f32(n);
}

// ---- glow ------------------------------------------------------------------
// Bright pass: p[0].x = threshold (linear luminance).

@fragment
fn fs_bright(in: VOut) -> @location(0) vec4<f32> {
    let c = src0(in.uv);
    let l = luma(c.rgb);
    let k = max(l - u.p[0].x, 0.0) / max(l, 1e-4);
    return vec4<f32>(c.rgb * k, c.a * k);
}

// ---- rgb split -------------------------------------------------------------
// p[0].xy = shift (uv)

@fragment
fn fs_rgb_split(in: VOut) -> @location(0) vec4<f32> {
    let r = src0(in.uv + u.p[0].xy);
    let g = src0(in.uv);
    let b = src0(in.uv - u.p[0].xy);
    return vec4<f32>(r.r, g.g, b.b, max(g.a, max(r.a, b.a)));
}

// ---- transform2d -----------------------------------------------------------
// p[0] = (1 / scale, radians, mirror, aspect)   p[1].xy = offset in composition units

@fragment
fn fs_transform2d(in: VOut) -> @location(0) vec4<f32> {
    let asp = vec2<f32>(u.p[0].w, 1.0);
    var v = (in.uv - vec2<f32>(0.5)) * asp - u.p[1].xy;
    let cs = cos(u.p[0].y);
    let sn = sin(u.p[0].y);
    v = vec2<f32>(v.x * cs + v.y * sn, -v.x * sn + v.y * cs) * u.p[0].x;
    let uv = v / asp + vec2<f32>(0.5);
    if u.p[0].z > 0.5 {
        return textureSampleLevel(t0, samp_mirror, uv, 0.0);
    }
    let w = max(fwidth(uv), vec2<f32>(1e-6));
    let m = clamp((vec2<f32>(0.5) - abs(uv - vec2<f32>(0.5))) / w + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    return src0(uv) * (m.x * m.y);
}
