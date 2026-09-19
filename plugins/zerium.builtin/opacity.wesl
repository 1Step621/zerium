import package::generated::util::{ZeriumContext, make_context, srgb_channel, srgb, scene_color, DEG2RAD};
import package::generated::effect::{ZeriumEffect, effect_input, effect_sampler, effect_instance, property_words, effect_source, effect_props, context, ZeriumVertex, vertex, uv_to_position, position_to_uv, uv_inside, sample_or_transparent, straight, rotate, over, premul, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};
import package::generated::properties_opacity::{ZeriumProps, props};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let amount = clamp(properties.opacity / 100.0, 0.0, 1.0);
    return textureSample(effect_input, effect_sampler, input.uv) * amount;
}
