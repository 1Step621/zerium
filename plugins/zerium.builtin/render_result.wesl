import package::generated::item::quad_corner;
import package::generated::capability_input::render_result;

struct RenderResultVertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
) -> RenderResultVertexOutput {
    let corner = quad_corner(vertex_index);
    return RenderResultVertexOutput(vec4(corner, 0.0, 1.0));
}

@fragment
fn fragment_main(input: RenderResultVertexOutput) -> @location(0) vec4<f32> {
    let size = textureDimensions(render_result);
    let position = min(
        vec2<u32>(input.position.xy),
        size - vec2(1u),
    );
    return textureLoad(render_result, vec2<i32>(position), 0);
}
