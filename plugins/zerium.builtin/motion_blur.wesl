import package::generated::temporal::{temporal_sample, temporal_accumulation, temporal_sampler, info, ZeriumVertex, vertex};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let info = info();
    let sample_color = textureSample(temporal_sample, temporal_sampler, input.uv);
    let accumulation_color = textureSample(temporal_accumulation, temporal_sampler, input.uv);
    return accumulation_color + sample_color / f32(max(info.sample_count, 1u));
}
