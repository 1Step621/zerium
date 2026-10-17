import package::generated::camera::inputs::render_result::sample;
import package::helpers::surface::{SurfaceVertex, surface_position, surface_vertex};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> SurfaceVertex {
    return surface_vertex(vertex_index, instance_index);
}

@fragment
fn fragment_main(input: SurfaceVertex) -> @location(0) vec4<f32> {
    return sample(surface_position(input));
}
