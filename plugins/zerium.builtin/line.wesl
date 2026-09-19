import package::generated::util::{ZeriumContext, make_context, srgb_channel, srgb, scene_color, DEG2RAD};
import package::generated::item::{ZeriumItem, items, property_words, item_props, context, to_ndc, quad_corner, quad_uv, quad_position, rotate, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};
import package::generated::properties_line::{ZeriumPropsTuple0, ZeriumPropsTuple1, ZeriumProps, props};

struct LineVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) @interpolate(flat) round_ends: u32,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> LineVertexOutput {
    let properties = props(instance_index);
    let position = vec2(properties.position.v0, properties.position.v1);
    let size = vec2(properties.size.v0, properties.size.v1);
    let local = quad_corner(vertex_index);
    let half_size = size * 0.5;
    let angle = properties.angle * DEG2RAD;
    let rotated = rotate(local * half_size, angle);

    var output: LineVertexOutput;
    output.position = vec4(
        to_ndc(instance_index, position + rotated),
        0.0,
        1.0,
    );
    output.local = local;
    output.color = scene_color(properties.color);
    output.half_size = half_size;
    output.round_ends = select(0u, 1u, properties.round_ends);
    return output;
}

@fragment
fn fragment_main(input: LineVertexOutput) -> @location(0) vec4<f32> {
    if input.round_ends != 0u {
        let radius = min(input.half_size.x, input.half_size.y);
        let segment_half_length = max(input.half_size.x - radius, 0.0);
        let point = input.local * input.half_size;
        let distance = length(vec2(max(abs(point.x) - segment_half_length, 0.0), point.y))
            - radius;
        if distance > 0.0 {
            discard;
        }
    }
    return input.color;
}
