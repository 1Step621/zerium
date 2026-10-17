import package::generated::_internal::item::_surface;
import package::generated::_internal::context::make_surface_context;
import package::generated::support::util::ZeriumContext;

// Composition coordinates use the frame center as (0, 0), +X to the right,
// and +Y downward. The viewport remains a pixel size rather than a position.
fn context() -> ZeriumContext {
    let output_size = _surface.output_size;
    return make_surface_context(
        output_size,
        _surface.composition_size,
        _surface.surface_size,
    );
}

fn to_ndc(position: vec2<f32>) -> vec2<f32> {
    let surface = _surface;
    return vec2(
        (position.x - surface.surface_min.x) / surface.surface_size.x * 2.0 - 1.0,
        1.0 - (position.y - surface.surface_min.y) / surface.surface_size.y * 2.0,
    );
}

// Fragment pixel centers map through the raster extent, including cropped images.
fn pixel_position(pixel: vec2<f32>) -> vec2<f32> {
    let surface = _surface;
    return surface.surface_min + pixel / surface.output_size * surface.surface_size;
}

// Two triangles covering a unit quad centered at the origin. Item shaders use
// this instead of duplicating vertex tables and coordinate conversion logic.
fn quad_corner(vertex_index: u32) -> vec2<f32> {
    let corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0),
        vec2(1.0, -1.0),
        vec2(-1.0, 1.0),
        vec2(-1.0, 1.0),
        vec2(1.0, -1.0),
        vec2(1.0, 1.0),
    );
    return corners[vertex_index];
}

fn quad_uv(corner: vec2<f32>) -> vec2<f32> {
    return corner * 0.5 + vec2(0.5);
}

// Each axis selects start (0), center (1), or end (2). Position is the
// selected origin's composition coordinate; size is the unpadded local size.
fn placement_center(position: vec2<f32>, size: vec2<f32>, origin: vec2<u32>) -> vec2<f32> {
    let anchor = vec2<f32>(origin) * 0.5;
    return position + (vec2(0.5) - anchor) * size;
}

fn quad_position(center: vec2<f32>, corner: vec2<f32>, size: vec2<f32>) -> vec4<f32> {
    let position = center + corner * size * 0.5;
    return vec4(to_ndc(position), 0.0, 1.0);
}
