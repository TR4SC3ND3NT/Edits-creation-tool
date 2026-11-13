// film_burn: a burn spreading from a centre. Fractal noise plus the closeness
// to the centre is compared with a threshold that falls as `amount` rises:
// beyond it the picture chars, then glows the burn's colour, then burns
// through to white-hot light. The noise churns with time.
//
// After EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/stylize3.rs (CC Burn Film: f = 0.65 noise + 0.35 (1 - d),
// threshold 1 - 1.25 amount, a band of 0.12, char then hot edge). Copyright
// (c) 2026 ArtCraft Team and the EffectCraft contributors. MIT License (see
// THIRD_PARTY.md). Altered: the hole burns to light instead of transparency,
// a colour, animated noise, composition units, WGSL.
//
// p0 = amount  p1..p3 = colour  p4, p5 = centre  p6 = 1 / feature size
// p7 = evolution (steps)  p8 = seed  p9 = distance that counts as 1 (half the frame's diagonal)

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let p = fx_to_comp(uv);
    let b = fx.p0;
    let d = min(length(p - vec2<f32>(fx.p4, fx.p5)) / fx.p9, 1.0);
    // The noise stretched to fill 0..1 (fbm gathers around 0.5).
    let n = clamp((fxl_fbm(p * fx.p6, fx.p7, fx.p8 + 17.0, 4.0) - 0.5) * 2.0 + 0.5, 0.0, 1.0);
    let f = 0.65 * n + 0.35 * (1.0 - d);
    let th = 1.0 - 1.25 * b;
    let band = 0.12 * min(10.0 * b, 1.0);
    let char_k = smoothstep(th - 2.0 * band, th, f);
    let hot = smoothstep(th, th + band, f);
    let hole = smoothstep(th + band, th + 2.0 * band, f);
    let colour = vec3<f32>(fx.p1, fx.p2, fx.p3);
    // Charred picture, then the hot colour, then white-hot light beyond 1.
    var rgb = c.rgb * (1.0 - 0.85 * char_k * (1.0 - hot));
    rgb = mix(rgb, colour * 1.5, hot);
    rgb = mix(rgb, mix(colour, vec3<f32>(1.0), 0.6) * 3.0, hole);
    return vec4<f32>(rgb, mix(c.a, 1.0, hot));
}
