import package::generated::item::{quad_corner, quad_uv, quad_position};
import package::generated::properties_text::props;
import package::generated::capability_input::{text, capability_sampler};

struct TextVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> TextVertexOutput {
    let properties = props(instance_index);
    let position = vec2(properties.position.v0, properties.position.v1);
    let size = vec2(properties.size.v0, properties.size.v1);
    let corner = quad_corner(vertex_index);
    var output: TextVertexOutput;
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
fn fragment_main(input: TextVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(text, capability_sampler, input.uv);
    return color;
}
