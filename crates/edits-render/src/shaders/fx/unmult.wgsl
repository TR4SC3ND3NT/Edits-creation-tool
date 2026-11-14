// unmult: removes a black (or white) background. On black the alpha is the
// brightest channel of the straight linear colour (ramped from `level` over
// `softness` of what remains above it) and the colour is divided by it, so
// the premultiplied colour does not change: over black the result is the
// original, over anything else the background shows through the dark parts.
// On white the darkest channel, mirrored (1 - darkest), sets the alpha and
// the white is taken back out of the colour.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/keying2.rs (Unmult). Copyright (c) 2026 ArtCraft Team
// and the EffectCraft contributors. MIT License (see THIRD_PARTY.md).
// Altered: on linear light (so that the result over black is the original
// in our linear compositing), colour matting always removed, WGSL.
//
// p0 = level  p1 = softness
// OPT_BACKGROUND: BACKGROUND_BLACK or BACKGROUND_WHITE

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let s = fx_straight(c);
    let white = OPT_BACKGROUND == BACKGROUND_WHITE;
    let lo = min(s.r, min(s.g, s.b));
    let hi = max(s.r, max(s.g, s.b));
    let d = clamp(select(hi, 1.0 - lo, white), 0.0, 1.0);
    // `level` is measured from the background in both cases.
    let start = fx.p0;
    let width = fx.p1 * (1.0 - start);
    var m = select(0.0, 1.0, d > start);
    if width > 1e-6 {
        m = clamp((d - start) / width, 0.0, 1.0);
    }
    if m <= 1e-6 {
        return vec4<f32>(0.0);
    }
    let bg = select(0.0, 1.0, white);
    // The straight colour is (s - bg (1 - m)) / m at alpha a m: premultiplied, the m cancels.
    return vec4<f32>(max(s - vec3<f32>(bg * (1.0 - m)), vec3<f32>(0.0)) * c.a, c.a * m);
}
