import package::generated::host::effect::{
    ZeriumVertex,
    effect_input,
    effect_sampler,
    straight,
    vertex,
};
import package::generated::host::util::scene_color;
import package::generated::monochrome::props;
import package::helpers::color::luminance;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_input, effect_sampler, input.uv);
    let luminance = luminance(straight(source));
    let tint = scene_color(properties.color) * source.a;
    return vec4(tint.rgb * select(1.0, luminance, properties.use_luminance), tint.a);
}
