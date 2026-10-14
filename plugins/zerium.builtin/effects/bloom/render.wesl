import package::generated::host::effect::{
    ZeriumVertex,
    effect_input,
    effect_sampler,
    effect_source,
    vertex,
};
import package::generated::bloom::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_source, effect_sampler, input.uv);
    if properties.radius <= 0.0 || properties.intensity <= 0.0 {
        return source;
    }
    let light = textureSample(effect_input, effect_sampler, input.uv) * properties.intensity / 100.0;
    let alpha = source.a + clamp(light.a, 0.0, 1.0) * (1.0 - source.a);
    return vec4(source.rgb + light.rgb, alpha);
}
