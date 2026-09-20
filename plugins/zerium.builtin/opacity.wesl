import package::generated::effect::{effect_input, effect_sampler, ZeriumVertex, vertex};
import package::generated::properties_opacity::props;

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
