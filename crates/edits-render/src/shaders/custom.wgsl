
@fragment
fn edits_fs(in: EditsVOut) -> @location(0) vec4<f32> {
    return effect(in.uv);
}
