// vhs: a worn tape. The picture rolls vertically, its lines wobble sideways,
// colour bleeds to the right of edges, and static and a tracking band run
// through it. Static changes every output frame (`fx.frame`); the wobble and
// the tracking band move with time.
//
// Written for edits (the look of ISF Bad TV and VHS Glitch; no code taken).
// License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = noise  p1 = roll (frame heights, 0..1)  p2 = bleed  p3 = wobble
// p4 = seed  p5 = time (time units)  (lengths in composition units)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let seed = fx.p4;
    let t = fx.p5;
    let frame = u32(fx.frame);
    var p = vec2<f32>(uv.x, fract(uv.y + fx.p1));
    // Tracking band: a strip that drifts down the frame, torn sideways.
    let band_y = fract(t * 0.21 + fxl_rand(seed, 5.0, 0.0));
    let in_band = smoothstep(0.06, 0.0, abs(p.y - band_y)) * fx.p0;
    let wob = (fx_noise(vec2<f32>(p.y * 60.0, t * 6.0), seed) - 0.5) * 2.0 * fx.p3;
    let tear = (fx_noise(vec2<f32>(p.y * 300.0, f32(frame)), seed + 9.0) - 0.5) * in_band * 0.05;
    p.x = p.x + (wob + tear) / fx.aspect;
    let c = src(p);
    // Colour bleed: luminance from here, colour averaged over a smear to the left.
    var smear = vec4<f32>(0.0);
    for (var i = 0; i < 6; i++) {
        smear += src(p - vec2<f32>(fx.p2 * f32(i) / 5.0 / fx.aspect, 0.0));
    }
    smear = smear / 6.0;
    let cs = fx_straight(c);
    let ss = fx_straight(smear);
    var rgb = max(vec3<f32>(fx_luma(cs)) + ss - vec3<f32>(fx_luma(ss)), vec3<f32>(0.0));
    // Static: grey noise per pixel and frame, stronger in the band, and a few bright streaks.
    let pix = vec2<u32>(p * fx.resolution);
    let grain = fx_hash(pix.x, pix.y, frame * 747796405u + bitcast<u32>(i32(seed))) - 0.5;
    // Streaks are on a fixed grid of 400 lines, whatever the output size.
    let line = floor(p.y * 400.0);
    let streak = select(0.0, 0.6, fxl_rand(line, f32(frame), seed + 1.0) < fx.p0 * 0.02);
    let d = fxl_enc(rgb) + vec3<f32>(grain * fx.p0 * (0.3 + in_band) + streak * fxl_rand(floor(p.x * 40.0), line, seed));
    rgb = fxl_dec(max(d, vec3<f32>(0.0)));
    return vec4<f32>(rgb * c.a, c.a);
}
