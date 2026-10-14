import package::generated::host::effect::{ZeriumVertex, effect_input, effect_sampler, vertex};
import package::generated::exposure::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let source = textureSample(effect_input, effect_sampler, input.uv);
    return vec4(source.rgb * exp2(props().exposure), source.a);
}
