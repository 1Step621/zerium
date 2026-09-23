import package::generated::capability_input::{behind, capability_sampler};
import package::generated::effect::{effect_input, effect_sampler, ZeriumVertex, vertex};
import package::generated::properties_layer_mask::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let layers = textureSample(behind, capability_sampler, input.uv);
    let alpha = textureSample(effect_input, effect_sampler, input.uv).a;
    let mask = select(alpha, 1.0 - alpha, props().invert_mask);
    return layers * mask;
}
