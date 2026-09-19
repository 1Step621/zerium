import package::generated::util::{ZeriumContext, make_context, srgb_channel, srgb, scene_color, DEG2RAD};
import package::generated::item::{ZeriumItem, items, property_words, item_props, context, to_ndc, quad_corner, quad_uv, quad_position, rotate, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};
import package::generated::properties_triangle::{ZeriumPropsTuple0, ZeriumPropsTuple1, ZeriumProps, props};

struct TriangleVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) apex_position: f32,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> TriangleVertexOutput {
    let properties = props(instance_index);
    let position = vec2(properties.position.v0, properties.position.v1);
    let size = vec2(properties.size.v0, properties.size.v1);
    let local = quad_corner(vertex_index);

    var output: TriangleVertexOutput;
    output.position = quad_position(
        instance_index,
        position,
        local,
        size,
    );
    output.local = local;
    output.color = scene_color(properties.color);
    output.apex_position = clamp(properties.apex_position / 100.0, 0.0, 1.0) * 2.0 - 1.0;
    return output;
}

fn triangle_edge(a: vec2<f32>, b: vec2<f32>, point: vec2<f32>) -> f32 {
    return (point.x - a.x) * (b.y - a.y) -
        (point.y - a.y) * (b.x - a.x);
}

@fragment
fn fragment_main(input: TriangleVertexOutput) -> @location(0) vec4<f32> {
    let apex = vec2(input.apex_position, -1.0);
    let left = vec2(-1.0, 1.0);
    let right = vec2(1.0, 1.0);
    let edge_a = triangle_edge(apex, left, input.local);
    let edge_b = triangle_edge(left, right, input.local);
    let edge_c = triangle_edge(right, apex, input.local);
    let all_positive = edge_a >= 0.0 && edge_b >= 0.0 && edge_c >= 0.0;
    let all_negative = edge_a <= 0.0 && edge_b <= 0.0 && edge_c <= 0.0;
    if !(all_positive || all_negative) {
        discard;
    }
    return input.color;
}
