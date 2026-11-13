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

// ---- bloom (multi-pass: see `fx.rs`) -----------------------------------------
// Ported from Bevy's `crates/bevy_post_process/src/bloom/bloom.wesl`
// (https://github.com/bevyengine/bevy, MIT OR Apache-2.0, used under MIT):
// the 13-tap downsample of "Next Generation Post Processing in Call of Duty:
// Advanced Warfare" (Jimenez 2014) with Karis' firefly reduction on the first
// level, the soft threshold, and the 3x3 tent upsample. Bevy mixes the levels
// with blend constants; here each upsample adds the finer level times its
// weight itself, and the result keeps no alpha (the final pass derives
// coverage from the light, as `glow` does).

// One tap, p[1].xy = a texel of the source (uv).
fn bloom_tap(uv: vec2<f32>, x: f32, y: f32) -> vec3<f32> {
    return max(src0(uv + vec2<f32>(x, y) * u.p[1].xy).rgb, vec3<f32>(0.0));
}

fn karis(c: vec3<f32>) -> f32 {
    return 1.0 / (1.0 + luma(c) * 0.25);
}

// Soft threshold (catlikecoding.com, via Bevy).
// p[0] = threshold, threshold - knee, 2 knee, 0.25 / knee.
fn bloom_threshold(c: vec3<f32>) -> vec3<f32> {
    let t = u.p[0];
    let brightness = max(c.r, max(c.g, c.b));
    var soft = clamp(brightness - t.y, 0.0, t.z);
    soft = soft * soft * t.w;
    let contribution = max(brightness - t.x, soft) / max(brightness, 1e-5);
    return c * contribution;
}

// The bright pass, pixel by pixel. Bevy thresholds the first level's
// downsampled average instead; thresholding each pixel first keeps the bloom
// of small bright shapes from depending on the frame's resolution.
@fragment
fn fs_bloom_bright(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(bloom_threshold(max(textureLoad(t0, vec2<i32>(in.pos.xy), 0).rgb, vec3<f32>(0.0))), 0.0);
}

// The first level, from the bright pass: the 13 taps in five groups, each
// weighted by Karis' average to tame single bright pixels.
@fragment
fn fs_bloom_first(in: VOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let a = bloom_tap(uv, -2.0, 2.0);
    let b = bloom_tap(uv, 0.0, 2.0);
    let c = bloom_tap(uv, 2.0, 2.0);
    let d = bloom_tap(uv, -2.0, 0.0);
    let e = bloom_tap(uv, 0.0, 0.0);
    let f = bloom_tap(uv, 2.0, 0.0);
    let g = bloom_tap(uv, -2.0, -2.0);
    let h = bloom_tap(uv, 0.0, -2.0);
    let i = bloom_tap(uv, 2.0, -2.0);
    let j = bloom_tap(uv, -1.0, 1.0);
    let k = bloom_tap(uv, 1.0, 1.0);
    let l = bloom_tap(uv, -1.0, -1.0);
    let m = bloom_tap(uv, 1.0, -1.0);
    let g0 = (a + b + d + e) * (0.125 / 4.0);
    let g1 = (b + c + e + f) * (0.125 / 4.0);
    let g2 = (d + e + g + h) * (0.125 / 4.0);
    let g3 = (e + f + h + i) * (0.125 / 4.0);
    let g4 = (j + k + l + m) * (0.5 / 4.0);
    let s = g0 * karis(g0) + g1 * karis(g1) + g2 * karis(g2) + g3 * karis(g3) + g4 * karis(g4);
    return vec4<f32>(s, 0.0);
}

@fragment
fn fs_bloom_down(in: VOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    var s = (bloom_tap(uv, -2.0, 2.0) + bloom_tap(uv, 2.0, 2.0) + bloom_tap(uv, -2.0, -2.0) + bloom_tap(uv, 2.0, -2.0)) * 0.03125;
    s += (bloom_tap(uv, 0.0, 2.0) + bloom_tap(uv, -2.0, 0.0) + bloom_tap(uv, 2.0, 0.0) + bloom_tap(uv, 0.0, -2.0)) * 0.0625;
    s += (bloom_tap(uv, 0.0, 0.0) + bloom_tap(uv, -1.0, 1.0) + bloom_tap(uv, 1.0, 1.0) + bloom_tap(uv, -1.0, -1.0) + bloom_tap(uv, 1.0, -1.0)) * 0.125;
    return vec4<f32>(s, 0.0);
}

// t0 = the coarser level so far (tent filtered; p[0].xy = its texel in uv),
// t1 = this level's downsample, added times p[0].z.
@fragment
fn fs_bloom_up(in: VOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let x = u.p[0].x;
    let y = u.p[0].y;
    var s = src0(uv).rgb * 0.25;
    s += (src0(uv + vec2<f32>(0.0, y)).rgb + src0(uv + vec2<f32>(-x, 0.0)).rgb + src0(uv + vec2<f32>(x, 0.0)).rgb + src0(uv + vec2<f32>(0.0, -y)).rgb) * 0.125;
    s += (src0(uv + vec2<f32>(-x, y)).rgb + src0(uv + vec2<f32>(x, y)).rgb + src0(uv + vec2<f32>(-x, -y)).rgb + src0(uv + vec2<f32>(x, -y)).rgb) * 0.0625;
    return vec4<f32>(s + textureSampleLevel(t1, samp, uv, 0.0).rgb * u.p[0].z, 0.0);
}

// ---- light rays --------------------------------------------------------------
// The rays (t1: a bright pass streaked toward the centre, see `fx.rs`) on the
// picture (t0), with EffectCraft's transfer modes (`light_rays` in
// https://github.com/storytold/effectcraft, MIT OR Apache-2.0).
// p[0].rgb = colour x intensity, p[0].w = mode: 0 add, 1 screen, 2 lighten.

@fragment
fn fs_rays_mix(in: VOut) -> @location(0) vec4<f32> {
    let px = src0(in.uv);
    let ray = textureSampleLevel(t1, samp, in.uv, 0.0).rgb * u.p[0].rgb;
    let la = clamp(max(ray.r, max(ray.g, ray.b)), 0.0, 1.0);
    let mode = u.p[0].w;
    if mode < 0.5 {
        return vec4<f32>(px.rgb + ray, px.a + (1.0 - px.a) * la);
    }
    if mode < 1.5 {
        // Screen, kept from darkening values above 1.
        let one = vec3<f32>(1.0);
        let rgb = px.rgb + ray - clamp(px.rgb, vec3<f32>(0.0), one) * clamp(ray, vec3<f32>(0.0), one);
        return vec4<f32>(rgb, px.a + la - px.a * la);
    }
    return vec4<f32>(max(px.rgb, ray), max(px.a, la));
}

// ---- lens flare --------------------------------------------------------------
// After EffectCraft's `lens_flare` (`crates/effects/src/generate2.rs`,
// MIT OR Apache-2.0): a core, streaks and a halo at the light, and ghosts
// (discs and rings) along the line from the light through the frame's centre.
// Distances are fractions of the frame's diagonal; positions in composition units.
// p[0] = light position, brightness, ghost count   p[1] = core size, streaks, halo radius, size
// p[2 + 2i] = ghost i: position along the line, radius, strength, ring   p[3 + 2i].rgb = its colour

@fragment
fn fs_lens_flare(in: VOut) -> @location(0) vec4<f32> {
    let px = src0(in.uv);
    let asp = u.time.w;
    let q = (in.uv - vec2<f32>(0.5)) * vec2<f32>(asp, 1.0);
    let light = u.p[0].xy;
    let diag = length(vec2<f32>(asp, 1.0));
    let size = max(u.p[1].w, 1e-4);
    let d = q - light;
    let r = length(d) / diag;
    let th = atan2(d.y, d.x + 1e-9);
    let ck = r / (u.p[1].x * size);
    let core = 1.5 * exp(-ck * ck) + 0.25 * exp(-r / (0.12 * size));
    let streak = pow(abs(cos(u.p[1].y * 0.5 * th)), 40.0) * exp(-r / (0.3 * size)) * 0.35;
    let hz = (r - u.p[1].z * size) / (0.012 * size);
    let halo = exp(-hz * hz) * 0.12;
    var l = vec3<f32>(core + streak) * vec3<f32>(1.0, 0.95, 0.85) + halo * vec3<f32>(0.6, 0.8, 1.0);
    let n = i32(u.p[0].w);
    for (var i = 0; i < n; i++) {
        let g = u.p[2 + 2 * i];
        let gd = length(q - light * (1.0 - g.x)) / diag;
        let gr = g.y * size;
        let z = (gd - gr) / (gr * 0.12);
        let v = select(1.0 - smoothstep(gr * 0.8, gr, gd), exp(-z * z), g.w > 0.5);
        l += u.p[3 + 2 * i].rgb * g.z * v;
    }
    l *= u.p[0].z;
    let la = min(max(l.r, max(l.g, l.b)), 1.0);
    return vec4<f32>(px.rgb + l, px.a + la * (1.0 - px.a));
}

// ---- outline: jump flooding ----------------------------------------------------
// Jump flooding (Rong and Tan, "Jump Flooding in GPU with Applications to
// Voronoi Diagram and Distance Transform", 2006). Each texel holds the offset
// in pixels to the nearest pixel inside the shape (xy) and outside it (zw);
// JFA_FAR means none found yet. Offsets stay small, so half floats hold them
// exactly.

const JFA_FAR: f32 = 16384.0;

// t0 = the layer: inside where alpha >= 0.5.
@fragment
fn fs_jfa_seed(in: VOut) -> @location(0) vec4<f32> {
    let a = textureLoad(t0, vec2<i32>(in.pos.xy), 0).a;
    return select(vec4<f32>(JFA_FAR, JFA_FAR, 0.0, 0.0), vec4<f32>(0.0, 0.0, JFA_FAR, JFA_FAR), a >= 0.5);
}

// One step: look at the 3x3 neighbours p[0].x pixels apart.
@fragment
fn fs_jfa(in: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    let last = vec2<i32>(textureDimensions(t0)) - vec2<i32>(1);
    let k = i32(u.p[0].x);
    var best_in = vec2<f32>(JFA_FAR);
    var best_out = vec2<f32>(JFA_FAR);
    var d_in = 1e30;
    var d_out = 1e30;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let q = clamp(p + vec2<i32>(i, j) * k, vec2<i32>(0), last);
            let n = textureLoad(t0, q, 0);
            let off = vec2<f32>(q - p);
            let a = n.xy + off;
            let b = n.zw + off;
            let la = dot(a, a);
            let lb = dot(b, b);
            if la < d_in {
                d_in = la;
                best_in = a;
            }
            if lb < d_out {
                d_out = lb;
                best_out = b;
            }
        }
    }
    return vec4<f32>(best_in, best_out);
}

// The stroke: t0 = the layer, t1 = its jump flood.
// p[0] = colour, opacity   p[1] = width outside, width inside, edge ramp (pixels)
// The signed distance to the edge is taken half a pixel from the nearest pixel
// centre across it (negative inside).
@fragment
fn fs_outline(in: VOut) -> @location(0) vec4<f32> {
    let pix = vec2<i32>(in.pos.xy);
    let s = textureLoad(t0, pix, 0);
    let f = textureLoad(t1, pix, 0);
    let sd = select(length(f.xy) - 0.5, 0.5 - length(f.zw), s.a >= 0.5);
    let ramp = u.p[1].z;
    // Outside: a band beneath the layer, reaching under its antialiased edge.
    let a_out = select(0.0, clamp((u.p[1].x - sd) / ramp + 0.5, 0.0, 1.0) * clamp(sd + 1.5, 0.0, 1.0), u.p[1].x > 0.0) * u.p[0].w;
    // Inside: a band over the layer, within its alpha.
    let a_in = select(0.0, clamp((u.p[1].y + sd) / ramp + 0.5, 0.0, 1.0), u.p[1].y > 0.0) * u.p[0].w * s.a;
    let col = u.p[0].rgb;
    let top = vec4<f32>(col * a_in, a_in) + s * (1.0 - a_in);
    return top + vec4<f32>(col * a_out, a_out) * (1.0 - top.a);
}

// ---- drop shadow ---------------------------------------------------------------
// t0 = the layer, t1 = the layer blurred (or the layer itself).
// p[0] = colour, opacity   p[1].xy = offset (uv)

@fragment
fn fs_shadow(in: VOut) -> @location(0) vec4<f32> {
    let s = src0(in.uv);
    let uv = in.uv - u.p[1].xy;
    // Nothing comes from outside the frame (the sampler would repeat its edge).
    let w = max(fwidth(uv), vec2<f32>(1e-6));
    let m = clamp((vec2<f32>(0.5) - abs(uv - vec2<f32>(0.5))) / w + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    let a = textureSampleLevel(t1, samp, uv, 0.0).a * m.x * m.y * u.p[0].w;
    return s + vec4<f32>(u.p[0].rgb * a, a) * (1.0 - s.a);
}

// ---- tilt shift ----------------------------------------------------------------
// Ported from glfx.js `tiltShift` (https://github.com/evanw/glfx.js, MIT,
// Copyright (C) 2011 by Evan Wallace): one pass of a triangle filter along
// p[2].xy whose radius grows with the distance from a line; run twice, along
// the line and across it. Our pictures are already premultiplied.
// p[0] = a point on the line (pixels), the line's unit normal
// p[1] = half width of the sharp band, length of the ramp, largest radius (pixels), taps per side
// p[2] = direction (unit), jitter (1 when the taps are more than a pixel apart)

@fragment
fn fs_tilt(in: VOut) -> @location(0) vec4<f32> {
    let pos = in.pos.xy;
    let dist = abs(dot(pos - u.p[0].xy, u.p[0].zw));
    let radius = u.p[1].z * smoothstep(0.0, 1.0, (dist - u.p[1].x) / max(u.p[1].y, 1e-3));
    let n = i32(u.p[1].w);
    let jitter = select(0.0, hash3(u32(pos.x), u32(pos.y), 7u) - 0.5, u.p[2].z > 0.5);
    let along = u.p[2].xy * u.res.zw * radius;
    var acc = vec4<f32>(0.0);
    var total = 0.0;
    for (var i = -n; i <= n; i++) {
        let percent = (f32(i) + jitter) / f32(n);
        let weight = max(1.0 - abs(percent), 0.0);
        acc += src0(in.uv + along * percent) * weight;
        total += weight;
    }
    return acc / total;
}
