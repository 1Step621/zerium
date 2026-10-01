import package::generated::host::effect::{effect_input, effect_sampler, viewport_uv, ZeriumVertex, vertex};
import package::generated::layer_mask::{behind, capability_sampler, props};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let layers = textureSample(behind, capability_sampler, viewport_uv(input.uv));
    let alpha = textureSample(effect_input, effect_sampler, input.uv).a;
    let mask = select(alpha, 1.0 - alpha, props().invert_mask);
    return layers * mask;
}
