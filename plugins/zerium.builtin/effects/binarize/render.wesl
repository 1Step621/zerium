import package::generated::host::effect::{
    ZeriumVertex,
    effect_input,
    effect_sampler,
    straight,
    vertex,
};
import package::generated::host::util::{scene_color, srgb};
import package::generated::binarize::props;
import package::shared::color::luminance;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_input, effect_sampler, input.uv);
    let luminance = luminance(straight(source));
    // The percentage describes display brightness; alpha is linear coverage.
    let threshold = srgb(vec3(properties.threshold / 100.0)).x;
    let color = select(properties.dark_color, properties.light_color, luminance >= threshold);
    var alpha = source.a;
    if properties.quantize_alpha {
        // Keep fully transparent pixels transparent, including at threshold zero.
        alpha = select(0.0, 1.0, source.a > 0.0 && source.a >= properties.alpha_threshold / 100.0);
    }
    return scene_color(color) * alpha;
}
