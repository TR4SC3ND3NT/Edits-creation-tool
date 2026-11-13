// kaleidoscope: the picture folded into wedges about a centre.
//
// Ported from EffectCraft (https://github.com/storytold/effectcraft),
// crates/effects/src/stylize2.rs `kaleida`. Copyright (c) 2026 ArtCraft Team
// and the EffectCraft contributors. MIT License (see THIRD_PARTY.md).
// Altered: any number of segments, mirroring as an option, zoom instead of
// size, composition units, mirrored sampling at the frame edges.
//
// p0 = segments  p1 = rotation (radians)  p2, p3 = centre  p4 = zoom
// OPT_MIRROR: MIRROR_ON or MIRROR_OFF

const KALEIDO_TAU: f32 = 6.2831853;

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = vec2<f32>(fx.p2, fx.p3);
    let seg = KALEIDO_TAU / fx.p0;
    let v = fx_to_comp(uv) - c;
    let r = length(v) / fx.p4;
    var a = atan2(v.y, v.x) - fx.p1;
    a = a - seg * floor(a / seg);
    if OPT_MIRROR == MIRROR_ON {
        a = select(a, seg - a, a > seg * 0.5);
    }
    let b = a + fx.p1;
    return src_mirror(fx_to_uv(c + r * vec2<f32>(cos(b), sin(b))));
}
