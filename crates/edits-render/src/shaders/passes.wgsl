// Passes with a fixed shader. Blending, colour effects and the glow's final
// add are generated per combination instead: see `compose.rs`.

// ---- geometry: one textured quad (an image, a solid or a cube face) --------
// p[0] = uv offset.xy, uv scale.xy   p[1] = premultiplied linear colour
// p[2].x = 1 to cut the picture off outside 0..1 (fit: contain)
// p[2].y = 1 to fade the quad's own edges by pixel coverage (flat layers;
//          cube faces rely on multisampling, which keeps shared edges closed)

@fragment
fn fs_geo(in: VOut) -> @location(0) vec4<f32> {
    let one = vec2<f32>(1.0, 1.0);
    let half = vec2<f32>(0.5, 0.5);
    let uv = u.p[0].xy + clamp(in.uv, vec2<f32>(0.0, 0.0), one) * u.p[0].zw;
    var c = textureSample(t0, samp, uv);
    if u.p[2].x > 0.5 {
        let w = max(fwidth(uv), vec2<f32>(1e-6, 1e-6));
        let m = clamp((half - abs(uv - half)) / w + half, vec2<f32>(0.0, 0.0), one);
        c = c * (m.x * m.y);
    }
    if u.p[2].y > 0.5 {
        let w = max(fwidth(in.uv), vec2<f32>(1e-6, 1e-6));
        let e = clamp(min(in.uv, one - in.uv) / w + half, vec2<f32>(0.0, 0.0), one);
        c = c * (e.x * e.y);
    }
    return c * u.p[1];
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
// p[0] = (taps, aspect)   p[1].xy = centre (uv)
// Tap i reads the picture at  centre + M_i (uv - centre) + o_i  with the 2x2
// matrix M_i = p[2 + i] (applied in square pixels) and the offset o_i =
// p[10 + i].xy: an offset along the line for a directional blur, a scale for a
// zoom, a rotation for a spin. Repeated passes with ever shorter streaks add
// up to one evenly sampled streak.

@fragment
fn fs_streak(in: VOut) -> @location(0) vec4<f32> {
    let n = i32(u.p[0].x);
    let asp = vec2<f32>(u.p[0].y, 1.0);
    let centre = u.p[1].xy;
    let v = (in.uv - centre) * asp;
    var acc = vec4<f32>(0.0);
    for (var i = 0; i < n; i++) {
        let m = u.p[2 + i];
        acc += src0(centre + vec2<f32>(m.x * v.x + m.y * v.y, m.z * v.x + m.w * v.y) / asp + u.p[10 + i].xy);
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
