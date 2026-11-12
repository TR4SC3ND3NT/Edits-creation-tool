// A custom effect for `edits`: CRT scanlines with a slow roll and a little
// colour bleed. Use it from a layer or an adjustment layer:
//
//   { "kind": "custom", "source": "shaders/scanlines.wgsl",
//     "params": { "p0": 0.5, "p1": 0.5 } }
//
//   p0  strength of the dark lines, 0..1
//   p1  colour bleed, 0..1 (shifts red and blue sideways)
//
// The file must define `effect`. `uv` runs from (0, 0) at the top-left to
// (1, 1) at the bottom-right. `src(uv)` reads the picture the effect is
// applied to (linear light, premultiplied alpha). `fx` carries the inputs:
// resolution, inv_resolution, time (seconds), beats, frame, aspect, p0..p7.

fn effect(uv: vec2<f32>) -> vec4<f32> {
    // Sizes are fractions of the height, so the look is the same at any scale.
    let lines = 180.0;
    let roll = fx.time * 0.05;
    let wave = 0.5 + 0.5 * cos((uv.y + roll) * lines * 6.2831853);
    let shade = 1.0 - fx.p0 * wave * wave;

    let bleed = vec2<f32>(fx.p1 * 0.004 / fx.aspect, 0.0);
    let centre = src(uv);
    let rgb = vec3<f32>(src(uv + bleed).r, centre.g, src(uv - bleed).b);
    return vec4<f32>(rgb * shade, centre.a);
}
