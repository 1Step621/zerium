@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumTemporalVertexOutput {
    return zerium_temporal_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumTemporalVertexOutput) -> @location(0) vec4<f32> {
    let info = zerium_temporal_info();
    let sample = textureSample(zerium_temporal_sample, zerium_temporal_sampler, input.uv);
    let accumulation = textureSample(zerium_temporal_accumulation, zerium_temporal_sampler, input.uv);
    return accumulation + sample / f32(max(info.sample_count, 1u));
}
