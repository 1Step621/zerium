import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input, sample_source};
import package::generated::host::temporal::info;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let info = info();
    let sample_color = sample_input(uv_to_position(input.uv));
    let accumulation_color = sample_source(uv_to_position(input.uv));
    return accumulation_color + sample_color / f32(max(info.sample_count, 1u));
}
