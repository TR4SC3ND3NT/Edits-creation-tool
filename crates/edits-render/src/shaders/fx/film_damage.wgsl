// film_damage: old film. Every output frame (`fx.frame`) gets its own gate
// weave (the picture jumps a little), brightness flicker, up to five vertical
// scratches and a scatter of dust specks, all hashed from the frame number
// and the seed, so the same frame always looks the same.
//
// Written for edits. License: MIT OR Apache-2.0, as the rest of the repository.
//
// p0 = scratches  p1 = dust  p2 = flicker  p3 = jitter (composition units)  p4 = seed

const FD_DUST_CELL: f32 = 0.04;

fn fd_rand(a: f32, b: f32) -> f32 {
    return fxl_rand(fx.frame, a * 131.0 + b, fx.p4);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let jump = vec2<f32>(fd_rand(1.0, 0.0), fd_rand(2.0, 0.0)) * 2.0 - vec2<f32>(1.0);
    let c = src_mirror(uv - fx_delta_uv(jump * fx.p3));
    var rgb = fx_straight(c);
    // Flicker: one gain for the whole frame.
    rgb = rgb * (1.0 + (fd_rand(3.0, 0.0) * 2.0 - 1.0) * fx.p2 * 0.5);
    let p = fx_to_comp(uv);
    let px = fx_pixel();
    // Scratches: thin vertical lines, bright or dark, that wander a little.
    for (var i = 0; i < 5; i++) {
        let fi = f32(i);
        if fd_rand(10.0, fi) < fx.p0 * (1.0 - fi * 0.18) {
            let x = (fd_rand(11.0, fi) - 0.5) * fx.aspect + 0.004 * sin(p.y * (8.0 + 20.0 * fd_rand(12.0, fi)) + fi);
            let w = max(0.0008 + 0.0015 * fd_rand(13.0, fi), px);
            let k = (1.0 - smoothstep(w * 0.5, w * 0.5 + px, abs(p.x - x))) * (0.4 + 0.6 * fd_rand(14.0, fi));
            let ink = select(vec3<f32>(1.0), vec3<f32>(0.05), fd_rand(15.0, fi) < 0.3);
            rgb = mix(rgb, ink, k);
        }
    }
    // Dust: at most one dark speck per cell, of up to a third of the cell.
    let cell = floor(p / FD_DUST_CELL);
    let h = fx_hash(bitcast<u32>(i32(cell.x)), bitcast<u32>(i32(cell.y)), u32(fx.frame) * 2654435761u + bitcast<u32>(i32(fx.p4)));
    if h < fx.p1 * 0.08 {
        let r1 = fx_hash(bitcast<u32>(i32(cell.x)) + 7u, bitcast<u32>(i32(cell.y)), u32(fx.frame) + 1013u);
        let r2 = fx_hash(bitcast<u32>(i32(cell.x)), bitcast<u32>(i32(cell.y)) + 7u, u32(fx.frame) + 2027u);
        let centre = (cell + vec2<f32>(0.3 + 0.4 * r1, 0.3 + 0.4 * r2)) * FD_DUST_CELL;
        let radius = max(FD_DUST_CELL * (0.04 + 0.12 * h / max(fx.p1 * 0.08, 1e-5)), px);
        let k = 1.0 - smoothstep(radius, radius + px, length(p - centre));
        rgb = mix(rgb, vec3<f32>(0.02), k * 0.9);
    }
    return vec4<f32>(max(rgb, vec3<f32>(0.0)) * c.a, c.a);
}
