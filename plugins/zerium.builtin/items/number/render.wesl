import package::generated::number::{capability_sampler, props, text};
import package::helpers::quad::{TextureVertex, texture_vertex};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> TextureVertex {
    let properties = props(instance_index);
    return texture_vertex(
        vertex_index,
        instance_index,
        vec2(properties.position.v0, properties.position.v1),
        vec2(properties.size.v0, properties.size.v1),
        vec2(properties.origin.v0, properties.origin.v1),
    );
}

@fragment
fn fragment_main(input: TextureVertex) -> @location(0) vec4<f32> {
    return textureSample(text, capability_sampler, input.uv);
}
