import package::generated::host::item::quad_corner;
import package::generated::camera::render_result;

struct CameraVertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
) -> CameraVertexOutput {
    let corner = quad_corner(vertex_index);
    return CameraVertexOutput(vec4(corner, 0.0, 1.0));
}

@fragment
fn fragment_main(input: CameraVertexOutput) -> @location(0) vec4<f32> {
    let size = textureDimensions(render_result);
    let position = min(
        vec2<u32>(input.position.xy),
        size - vec2(1u),
    );
    return textureLoad(render_result, vec2<i32>(position), 0);
}
