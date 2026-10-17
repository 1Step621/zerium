import package::generated::host::item::{pixel_position, quad_corner};

struct SurfaceVertex {
    @builtin(position) position: vec4<f32>,
    @interpolate(flat)
    @location(0) instance_index: u32,
}

fn surface_vertex(vertex_index: u32, instance_index: u32) -> SurfaceVertex {
    return SurfaceVertex(vec4(quad_corner(vertex_index), 0.0, 1.0), instance_index);
}

fn surface_position(input: SurfaceVertex) -> vec2<f32> {
    return pixel_position(input.position.xy);
}
