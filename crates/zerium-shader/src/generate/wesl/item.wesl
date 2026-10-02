import package::generated::host::_props::_items;
import package::generated::host::_context::make_surface_context;
import package::generated::host::util::ZeriumContext;

// Composition coordinates use the frame center as (0, 0), +X to the right,
// and +Y downward. The viewport remains a pixel size rather than a position.
fn context(instance_index: u32) -> ZeriumContext {
    let output_size = _items[instance_index].output_size;
    return make_surface_context(
        output_size,
        _items[instance_index].composition_size,
        _items[instance_index].surface_size,
    );
}

fn to_ndc(
    instance_index: u32,
    position: vec2<f32>,
) -> vec2<f32> {
    let item = _items[instance_index];
    return vec2(
        (position.x - item.surface_min.x) / item.surface_size.x * 2.0 - 1.0,
        1.0 - (position.y - item.surface_min.y) / item.surface_size.y * 2.0,
    );
}

// Two triangles covering a unit quad centered at the origin. Item shaders use
// this instead of duplicating vertex tables and coordinate conversion logic.
fn quad_corner(vertex_index: u32) -> vec2<f32> {
    let corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0),
        vec2( 1.0, -1.0),
        vec2(-1.0,  1.0),
        vec2(-1.0,  1.0),
        vec2( 1.0, -1.0),
        vec2( 1.0,  1.0),
    );
    return corners[vertex_index];
}

fn quad_uv(corner: vec2<f32>) -> vec2<f32> {
    return corner * 0.5 + vec2(0.5);
}

fn quad_position(
    instance_index: u32,
    center: vec2<f32>,
    corner: vec2<f32>,
    size: vec2<f32>,
) -> vec4<f32> {
    let position = center + corner * size * 0.5;
    return vec4(
        to_ndc(instance_index, position),
        0.0,
        1.0,
    );
}
