import package::generated::util::{ZeriumContext, make_context};

struct ZeriumItem {
    property_offset: u32,
    property_size: u32,
    output_size: vec2<f32>,
    composition_size: vec2<f32>,
};

@group(0) @binding(0)
var<storage, read> items: array<ZeriumItem>;

// Properties are words internally so each item can use a different typed layout.
@group(0) @binding(1)
var<storage, read> property_words: array<u32>;

fn item_props(instance_index: u32) -> ZeriumRawProps {
    let item = items[instance_index];
    return ZeriumRawProps(item.property_offset, item.property_size);
}

// Composition coordinates use the frame center as (0, 0), +X to the right,
// and +Y downward. The viewport remains a pixel size rather than a position.
fn context(instance_index: u32) -> ZeriumContext {
    let output_size = items[instance_index].output_size;
    return make_context(output_size, items[instance_index].composition_size);
}

fn to_ndc(
    instance_index: u32,
    position: vec2<f32>,
) -> vec2<f32> {
    let composition_size = items[instance_index].composition_size;
    return vec2(
        position.x / composition_size.x * 2.0,
        -position.y / composition_size.y * 2.0,
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

fn rotate(point: vec2<f32>, radians: f32) -> vec2<f32> {
    let sine = sin(radians);
    let cosine = cos(radians);
    return vec2(
        point.x * cosine - point.y * sine,
        point.x * sine + point.y * cosine,
    );
}
