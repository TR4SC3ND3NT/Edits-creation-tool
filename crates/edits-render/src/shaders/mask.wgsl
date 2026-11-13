
// ---- masks (see `mask.rs` and `edits_core::mask`) --------------------------
// One pass per mask. t0 is what the combined coverage multiplies (the
// layer's picture on the last pass, white before), t1 the coverage so far.
// p[0..3].xyz = rows of the map from the frame (NDC x, y, 1) to the layer
//               plane (homogeneous; z <= 0 is behind the camera)
// p[3] = (shape: 0 rect or rounded rect, 1 ellipse, 2 polygon;
//         mode: 0 add, 1 subtract, 2 intersect, 3 difference; inverted; opacity)
// p[4] = (coverage before this mask, or -1 to read it from t1; polygon points)
// p[5] = (position.xy, cos, sin of the rotation)
// p[6] = (half size.xy, corner radius)
// p[7] = (feather, expansion) in layer units
// p[8..24] = polygon points, two per vector

fn mask_box(q: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let r = clamp(radius, 0.0, min(half.x, half.y));
    let d = abs(q) - half + vec2<f32>(r);
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - r;
}

// Exact on the outline and for circles; close to it near the outline.
fn mask_ellipse(q: vec2<f32>, radii: vec2<f32>) -> f32 {
    let r = max(radii, vec2<f32>(1e-7));
    let k0 = length(q / r);
    let k1 = length(q / (r * r));
    if k1 < 1e-12 {
        return -min(r.x, r.y);
    }
    return k0 * (k0 - 1.0) / k1;
}

fn mask_point(i: i32) -> vec2<f32> {
    let v = u.p[8 + i / 2];
    return select(v.xy, v.zw, (i & 1) == 1);
}

// Distance to the outline, negative inside (even-odd rule).
fn mask_polygon(q: vec2<f32>, n: i32) -> f32 {
    var d = dot(q - mask_point(0), q - mask_point(0));
    var s = 1.0;
    var j = n - 1;
    for (var i = 0; i < n; i++) {
        let vi = mask_point(i);
        let vj = mask_point(j);
        let e = vj - vi;
        let w = q - vi;
        let b = w - e * clamp(dot(w, e) / max(dot(e, e), 1e-18), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c = vec3<bool>((q.y >= vi.y), (q.y < vj.y), (e.x * w.y > e.y * w.x));
        if all(c) || !any(c) {
            s = -s;
        }
        j = i;
    }
    return s * sqrt(d);
}

@fragment
fn fs_mask(in: VOut) -> @location(0) vec4<f32> {
    let ndc = vec3<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0, 1.0);
    let h = vec3<f32>(dot(u.p[0].xyz, ndc), dot(u.p[1].xyz, ndc), dot(u.p[2].xyz, ndc));
    let front = h.z > 1e-9;
    let pos = h.xy / max(h.z, 1e-9);
    let dp = pos - u.p[5].xy;
    let cs = u.p[5].z;
    let sn = u.p[5].w;
    let q = vec2<f32>(dp.x * cs + dp.y * sn, -dp.x * sn + dp.y * cs);
    let shape = i32(u.p[3].x);
    var d: f32;
    if shape == 2 {
        d = mask_polygon(q, i32(u.p[4].y));
    } else if shape == 1 {
        d = mask_ellipse(q, u.p[6].xy);
    } else {
        d = mask_box(q, u.p[6].xy, u.p[6].z);
    }
    d = d - u.p[7].y;
    // The edge fades over the feather, or one pixel without (anti-aliasing).
    let aa = fwidth(d);
    var c = clamp(0.5 - d / max(length(vec2<f32>(u.p[7].x, aa)), 1e-9), 0.0, 1.0);
    c = select(0.0, c, front);
    if u.p[3].z > 0.5 {
        c = 1.0 - c;
    }
    c = c * u.p[3].w;
    var a = u.p[4].x;
    if a < 0.0 {
        a = textureSampleLevel(t1, samp, in.uv, 0.0).a;
    }
    let mode = i32(u.p[3].y);
    var r: f32;
    if mode == 1 {
        r = a * (1.0 - c);
    } else if mode == 2 {
        r = a * c;
    } else if mode == 3 {
        r = abs(a - c);
    } else {
        r = a + c - a * c;
    }
    return src0(in.uv) * clamp(r, 0.0, 1.0);
}

// ---- track matte -------------------------------------------------------------
// t0 = the layer's picture, t1 = the matte source's picture (both frame-sized,
// premultiplied linear). p[0] = (mode: 0 alpha, 1 alpha inverted, 2 luma,
// 3 luma inverted; the source's opacity). Luma is Rec. 709 on the source's
// display (sRGB) colour, times its alpha: its colour over black.

@fragment
fn fs_matte(in: VOut) -> @location(0) vec4<f32> {
    let m = textureSampleLevel(t1, samp, in.uv, 0.0);
    let op = u.p[0].y;
    let mode = i32(u.p[0].x);
    let alpha = clamp(m.a, 0.0, 1.0);
    let straight = clamp(m.rgb / max(m.a, 1e-5), vec3<f32>(0.0), vec3<f32>(1.0));
    let l = luma(enc(straight)) * alpha;
    var k: f32;
    if mode == 1 {
        k = 1.0 - alpha * op;
    } else if mode == 2 {
        k = l * op;
    } else if mode == 3 {
        k = 1.0 - l * op;
    } else {
        k = alpha * op;
    }
    return src0(in.uv) * clamp(k, 0.0, 1.0);
}
