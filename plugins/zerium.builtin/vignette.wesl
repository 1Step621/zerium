import package::generated::util::{ZeriumContext, make_context, srgb_channel, srgb, scene_color, DEG2RAD};
import package::generated::effect::{ZeriumEffect, effect_input, effect_sampler, effect_instance, property_words, effect_source, effect_props, context, ZeriumVertex, vertex, uv_to_position, position_to_uv, uv_inside, sample_or_transparent, straight, rotate, over, premul, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};
import package::generated::properties_vignette::{ZeriumProps, props};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let dimensions = context().output_size;
    let aspect = dimensions.x / max(dimensions.y, 1.0);
    var centered = input.uv * 2.0 - vec2(1.0);
    centered.x *= aspect * max(properties.roundness, 0.01);
    let corner_distance = length(vec2(aspect * max(properties.roundness, 0.01), 1.0));
    let distance = length(centered) / max(corner_distance, 0.00001);
    let vignette = smoothstep(
        max(1.0 - properties.softness, 0.0),
        1.0,
        distance,
    ) * clamp(properties.amount, 0.0, 1.0);
    let source = textureSample(effect_input, effect_sampler, input.uv);
    return vec4(source.rgb * (1.0 - vignette), source.a);
}
