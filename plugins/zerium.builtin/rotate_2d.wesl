import package::generated::util::{ZeriumContext, make_context, srgb_channel, srgb, scene_color, DEG2RAD};
import package::generated::effect::{ZeriumEffect, effect_input, effect_sampler, effect_instance, property_words, effect_source, effect_props, context, ZeriumVertex, vertex, uv_to_position, position_to_uv, uv_inside, sample_or_transparent, straight, rotate, over, premul, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};
import package::generated::properties_rotate_2d::{ZeriumPropsTuple1, ZeriumProps, props};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let center = vec2(properties.center.v0, properties.center.v1);
    let output_position = uv_to_position(input.uv);
    let relative_position = output_position - center;
    let angle = -properties.angle * DEG2RAD;
    let source_position = rotate(relative_position, angle) + center;
    let source_uv = position_to_uv(source_position);
    return sample_or_transparent(source_uv);
}
