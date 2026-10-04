import package::generated::host::item::{placement_center, quad_corner, quad_uv, quad_position};
import package::generated::host::entity::{props, source, capability_sampler};

struct MediaVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> MediaVertexOutput {
    let properties = props(instance_index);
    let size = vec2(properties.size.v0, properties.size.v1);
    let position = placement_center(
        vec2(properties.position.v0, properties.position.v1),
        size,
        vec2(properties.origin.v0, properties.origin.v1),
    );
    let corner = quad_corner(vertex_index);
    var output: MediaVertexOutput;
    output.position = quad_position(
        instance_index,
        position,
        corner,
        size,
    );
    output.uv = quad_uv(corner);
    return output;
}

@fragment
fn fragment_main(input: MediaVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(source, capability_sampler, input.uv);
    return color;
}
