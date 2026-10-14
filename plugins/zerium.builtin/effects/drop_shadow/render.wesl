import package::generated::host::effect::{
    ZeriumVertex,
    context,
    effect_input,
    effect_sampler,
    effect_source,
    over,
    vertex,
};
import package::generated::host::util::scene_color;
import package::generated::drop_shadow::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_source, effect_sampler, input.uv);
    let uv = input.uv - vec2(properties.offset.v0, properties.offset.v1) / context().surface_size;
    let inside = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    let alpha = textureSample(effect_input, effect_sampler, uv).a * select(0.0, 1.0, inside);
    return over(source, scene_color(properties.color) * alpha);
}
