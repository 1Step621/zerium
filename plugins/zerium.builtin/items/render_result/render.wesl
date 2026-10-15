import package::generated::render_result::render_result;
import package::helpers::surface::{SurfaceVertex, surface_pixel, surface_vertex};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> SurfaceVertex {
    return surface_vertex(vertex_index);
}

@fragment
fn fragment_main(input: SurfaceVertex) -> @location(0) vec4<f32> {
    return surface_pixel(render_result, input.position);
}
