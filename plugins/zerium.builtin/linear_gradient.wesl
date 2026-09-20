import package::generated::util::{srgb, DEG2RAD};
import package::generated::effect::{effect_input, effect_sampler, context, ZeriumVertex, vertex};
import package::generated::properties_linear_gradient::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_input, effect_sampler, input.uv);
    let dimensions = context().output_size;
    let point = input.uv * dimensions;
    let center = dimensions * 0.5;
    let angle = properties.angle * DEG2RAD;
    let direction = vec2(cos(angle), sin(angle));
    let half_span = max(dot(abs(direction), dimensions) * 0.5, 0.00001);
    let amount = clamp(dot(point - center, direction) / (half_span * 2.0) + 0.5, 0.0, 1.0);
    let color = mix(properties.start_color, properties.end_color, amount);
    let alpha = source.a * color.a;
    return vec4(srgb(color.rgb) * alpha, alpha);
}
