// shatter: the picture breaks into flat shards that fly apart in 3D.
//
// Each shard is a sector around the impact point (optionally cut by rings)
// in the picture's plane. fxlib/kit.rs computes, for the instant being
// rendered, every shard's homography from the output frame back to the
// picture (its rigid 3D motion seen through a pinhole camera) and binds them
// as a table, nearest shard first: row j holds, as pairs of half floats
// (hi, lo), the three rows of the inverse homography, [a0, a1, shade] and
// [r0, r1, scale]. Here every pixel walks the shards front to back and
// composites the ones whose sector it falls in. Crack lines are drawn along
// the shard edges, up to `reach` from the impact point.
//
// Ported from the user's own Python model (shatter5.py of the Death Note
// kit: sectors, rigid bodies, one camera, black between). Written for edits.
// License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = shards  p1, p2 = impact point  p3 = gap  p4 = crack opacity
// p5 = crack width  p6 = reach  p7..p9 = crack colour  p10 = jag  p11 = seed
// p12 = glint
// (lengths in composition units)

fn sh_get(row: i32, k: i32) -> vec3<f32> {
    let a = textureLoad(edits_lut_texture, vec3<i32>(2 * k, row, 0), 0).rgb;
    let b = textureLoad(edits_lut_texture, vec3<i32>(2 * k + 1, row, 0), 0).rgb;
    return a + b;
}

fn sh_wrap_pi(a: f32) -> f32 {
    return a - FXL_TAU * floor((a + FXL_PI) / FXL_TAU);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let q = vec3<f32>(fx_to_comp(uv), 1.0);
    let n = i32(fx.p0);
    let c = vec2<f32>(fx.p1, fx.p2);
    let half_gap = fx.p3 * 0.5;
    let jag = fx.p10;
    let half_w = vec2<f32>(fx.aspect * 0.5, 0.5);
    var acc = vec4<f32>(0.0);
    for (var j = 0; j < n; j++) {
        if acc.a > 0.999 {
            break;
        }
        let ph = vec3<f32>(dot(sh_get(j, 0), q), dot(sh_get(j, 1), q), dot(sh_get(j, 2), q));
        if ph.z <= 0.0 {
            continue;
        }
        let p = ph.xy / ph.z;
        let ang = sh_get(j, 3);
        let rad = sh_get(j, 4);
        // Antialiasing width in picture units: one output pixel.
        let aa = fx_pixel() * rad.z;
        // The edges wander: the partition is taken at a displaced point, the
        // same for every shard, so neighbours still meet.
        var pj = p;
        if jag > 0.0 {
            let f = 1.0 / max(jag * 5.0, 0.015);
            pj = p + jag * 2.0 * (vec2<f32>(fx_noise(p * f, fx.p11), fx_noise(p * f + vec2<f32>(17.3, 5.1), fx.p11 + 1.0)) - vec2<f32>(0.5));
        }
        let d = pj - c;
        let r = length(d);
        let th = atan2(d.y, d.x);
        let span = ang.y - ang.x;
        let dm = sh_wrap_pi(th - (ang.x + span * 0.5));
        // Signed distance to the shard's edges, positive inside.
        var dist = r * sin(clamp(span * 0.5 - abs(dm), -1.5707963, 1.5707963));
        if rad.x > 0.0 {
            dist = min(dist, r - rad.x);
        }
        dist = min(dist, rad.y - r);
        // Inside the picture's frame.
        let m = min(half_w.x - abs(p.x), half_w.y - abs(p.y));
        let cov = smoothstep(half_gap - aa, half_gap, dist) * smoothstep(-aa, 0.0, m);
        if cov <= 0.0 {
            continue;
        }
        let s = src(fx_to_uv(clamp(p, -half_w, half_w)));
        var rgb = s.rgb * ang.z;
        if fx.p4 > 0.0 {
            // Each side of an edge draws half the line.
            let edge = dist - half_gap;
            let cw = fx.p5 * 0.5;
            let k = fx.p4 * (1.0 - smoothstep(cw - aa * 0.5, cw + aa * 0.5, edge)) * (1.0 - smoothstep(fx.p6 - aa, fx.p6, r));
            rgb = mix(rgb, vec3<f32>(fx.p7, fx.p8, fx.p9) * s.a, k);
            if fx.p12 > 0.0 {
                let g = fx.p5 * 0.6;
                let rim = smoothstep(cw - aa * 0.5, cw + aa * 0.5, edge) * (1.0 - smoothstep(cw + g - aa * 0.5, cw + g + aa * 0.5, edge));
                rgb = rgb + vec3<f32>(fx.p12 * rim * s.a) * (1.0 - smoothstep(fx.p6 - aa, fx.p6, r)) * fx.p4;
            }
        }
        acc = acc + (1.0 - acc.a) * vec4<f32>(rgb, s.a) * cov;
    }
    if OPT_FILL == FILL_BLACK {
        acc = acc + (1.0 - acc.a) * vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return acc;
}
