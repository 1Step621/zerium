import package::generated::util::{ZeriumContext, make_context, srgb_channel, srgb, scene_color, DEG2RAD};
import package::generated::effect::{ZeriumEffect, effect_input, effect_sampler, effect_instance, property_words, effect_source, effect_props, context, ZeriumVertex, vertex, uv_to_position, position_to_uv, uv_inside, sample_or_transparent, straight, rotate, over, premul, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};
import package::generated::properties_chromatic_aberration::{ZeriumProps, props};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let angle = properties.angle * DEG2RAD;
    let offset = vec2(cos(angle), sin(angle)) * properties.amount
        / context().composition_size;
    let red_sample = textureSample(effect_input, effect_sampler, input.uv + offset);
    let center_sample = textureSample(effect_input, effect_sampler, input.uv);
    let blue_sample = textureSample(effect_input, effect_sampler, input.uv - offset);
    let alpha = max(red_sample.a, max(center_sample.a, blue_sample.a));
    let color = vec3(
        straight(red_sample).r,
        straight(center_sample).g,
        straight(blue_sample).b,
    );
    return vec4(color * alpha, alpha);
}
