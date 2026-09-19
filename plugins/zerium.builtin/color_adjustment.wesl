import package::generated::util::{ZeriumContext, make_context, srgb_channel, srgb, scene_color, DEG2RAD};
import package::generated::effect::{ZeriumEffect, effect_input, effect_sampler, effect_instance, property_words, effect_source, effect_props, context, ZeriumVertex, vertex, uv_to_position, position_to_uv, uv_inside, sample_or_transparent, straight, rotate, over, premul, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};
import package::generated::properties_color_adjustment::{ZeriumProps, props};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_input, effect_sampler, input.uv);
    let alpha = source.a;
    var color = straight(source);

    color += vec3(properties.brightness);
    color = (color - vec3(0.5)) * properties.contrast + vec3(0.5);
    let luminance = dot(color, vec3(0.2126, 0.7152, 0.0722));
    color = mix(vec3(luminance), color, properties.saturation);
    color += vec3(properties.temperature * 0.12, 0.0, -properties.temperature * 0.12);
    color += vec3(properties.tint * 0.06, -properties.tint * 0.06, properties.tint * 0.06);
    color = pow(max(color, vec3(0.0)), vec3(1.0 / max(properties.gamma, 0.01)));

    return vec4(max(color, vec3(0.0)) * alpha, alpha);
}
