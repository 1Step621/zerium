import package::generated::host::item::{placement_center, quad_corner, quad_position, quad_uv};

// Placement uses the unpadded item size. Callers may expand or rotate the drawn
// quad while keeping its origin and composition-space center unchanged.
struct PlacedQuad {
    position: vec4<f32>,
    corner: vec2<f32>,
    center: vec2<f32>,
};

fn placed_quad(
    vertex_index: u32,
    instance_index: u32,
    position: vec2<f32>,
    size: vec2<f32>,
    origin: vec2<u32>,
) -> PlacedQuad {
    let center = placement_center(position, size, origin);
    let corner = quad_corner(vertex_index);
    return PlacedQuad(
        quad_position(instance_index, center, corner, size),
        corner,
        center,
    );
}

struct TextureVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

fn texture_vertex(
    vertex_index: u32,
    instance_index: u32,
    position: vec2<f32>,
    size: vec2<f32>,
    origin: vec2<u32>,
) -> TextureVertex {
    let quad = placed_quad(vertex_index, instance_index, position, size, origin);
    return TextureVertex(quad.position, quad_uv(quad.corner));
}
