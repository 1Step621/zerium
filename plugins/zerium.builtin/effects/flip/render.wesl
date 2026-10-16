import package::generated::host::effect::{
    ZeriumVertex,
    effect_input,
    effect_sampler,
    vertex,
};
import package::generated::flip::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let uv = select(input.uv, vec2(1.0) - input.uv, vec2(properties.horizontal, properties.vertical));
    return textureSample(effect_input, effect_sampler, uv);
}
