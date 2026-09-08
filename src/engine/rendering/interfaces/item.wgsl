struct ZeriumItemInstance {
    params_offset: u32,
    params_size: u32,
    output_size: vec2<f32>,
    composition_size: vec2<f32>,
};

@group(0) @binding(0)
var<storage, read> zerium_items: array<ZeriumItemInstance>;

// Parameters are words internally so each item can use a different typed layout.
@group(0) @binding(1)
var<storage, read> zerium_parameter_words: array<u32>;

fn zerium_raw_params_for_instance(instance_index: u32) -> ZeriumRawParams {
    let item = zerium_items[instance_index];
    return ZeriumRawParams(item.params_offset, item.params_size);
}

// Composition coordinates use the frame center as (0, 0), +X to the right,
// and +Y downward. The viewport remains a pixel size rather than a position.
fn zerium_render_context(instance_index: u32) -> ZeriumRenderContext {
    let output_size = zerium_items[instance_index].output_size;
    return zerium_make_render_context(output_size, zerium_items[instance_index].composition_size);
}

fn zerium_composition_position_to_ndc(
    instance_index: u32,
    position: vec2<f32>,
) -> vec2<f32> {
    let composition_size = zerium_items[instance_index].composition_size;
    return vec2(
        position.x / composition_size.x * 2.0,
        -position.y / composition_size.y * 2.0,
    );
}

// Two triangles covering a unit quad centered at the origin. Item shaders use
// this instead of duplicating vertex tables and coordinate conversion logic.
fn zerium_item_quad_corner(vertex_index: u32) -> vec2<f32> {
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

fn zerium_item_quad_uv(corner: vec2<f32>) -> vec2<f32> {
    return corner * 0.5 + vec2(0.5);
}

fn zerium_item_quad_clip_position(
    instance_index: u32,
    center: vec2<f32>,
    corner: vec2<f32>,
    size: vec2<f32>,
) -> vec4<f32> {
    let position = center + corner * size * 0.5;
    return vec4(
        zerium_composition_position_to_ndc(instance_index, position),
        0.0,
        1.0,
    );
}

fn zerium_rotate_2d(point: vec2<f32>, radians: f32) -> vec2<f32> {
    let sine = sin(radians);
    let cosine = cos(radians);
    return vec2(
        point.x * cosine - point.y * sine,
        point.x * sine + point.y * cosine,
    );
}
