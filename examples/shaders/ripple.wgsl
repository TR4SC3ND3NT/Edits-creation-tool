// A custom effect for `edits`: a ring that spreads from the centre once per
// beat and bends the picture as it passes.
//
//   { "kind": "custom", "source": "shaders/ripple.wgsl",
//     "params": { "p0": 0.5, "p1": 0.5 } }
//
//   p0  how far the ring pushes the picture, 0..1
//   p1  width of the ring, 0..1

fn effect(uv: vec2<f32>) -> vec4<f32> {
    // Work in units of the height with the origin at the centre, like layers do.
    let p = (uv - vec2<f32>(0.5, 0.5)) * vec2<f32>(fx.aspect, 1.0);
    let distance = length(p);
    let phase = fract(fx.beats);            // 0 on the beat, 1 just before the next
    let radius = phase * 0.9;
    let width = 0.02 + 0.1 * fx.p1;
    let ring = exp(-pow((distance - radius) / width, 2.0)) * (1.0 - phase);
    let push = normalize(p + vec2<f32>(1e-6, 0.0)) * ring * fx.p0 * 0.06;
    return src_mirror(uv - push / vec2<f32>(fx.aspect, 1.0)) * (1.0 + ring * fx.p0);
}
