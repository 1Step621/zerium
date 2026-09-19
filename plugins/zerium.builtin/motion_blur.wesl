import package::generated::util::{ZeriumContext, make_context, srgb_channel, srgb, scene_color, DEG2RAD};
import package::generated::temporal::{ZeriumTemporal, temporal_sample, temporal_accumulation, temporal_sampler, temporal_state, property_words, effect_props, info, context, ZeriumVertex, vertex, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};
import package::generated::properties_motion_blur::{ZeriumProps, props};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let info = info();
    let sample_color = textureSample(temporal_sample, temporal_sampler, input.uv);
    let accumulation_color = textureSample(temporal_accumulation, temporal_sampler, input.uv);
    return accumulation_color + sample_color / f32(max(info.sample_count, 1u));
}
