import package::generated::host::item::quad_corner;

struct SurfaceVertex {
    @builtin(position) position: vec4<f32>,
};

fn surface_vertex(vertex_index: u32) -> SurfaceVertex {
    return SurfaceVertex(vec4(quad_corner(vertex_index), 0.0, 1.0));
}

fn surface_pixel(source: texture_2d<f32>, fragment_position: vec4<f32>) -> vec4<f32> {
    let size = textureDimensions(source);
    let position = min(vec2<u32>(fragment_position.xy), size - vec2(1u));
    return textureLoad(source, vec2<i32>(position), 0);
}
