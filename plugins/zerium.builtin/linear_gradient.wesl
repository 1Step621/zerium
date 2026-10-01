import package::generated::host::util::{srgb, DEG2RAD};
import package::generated::host::effect::{effect_source, effect_sampler, context, ZeriumVertex, vertex};
import package::generated::linear_gradient::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_source, effect_sampler, input.uv);
    let output_size = context().output_size;
    let center = output_size * 0.5;
    let point = input.uv * output_size;
    let angle = properties.angle * DEG2RAD;
    let direction = vec2(cos(angle), sin(angle));
    let half_span = max(dot(abs(direction), output_size) * 0.5, 0.00001);
    let amount = clamp(dot(point - center, direction) / (half_span * 2.0) + 0.5, 0.0, 1.0);
    let color = mix(properties.start_color, properties.end_color, amount);
    let alpha = source.a * color.a;
    return vec4(srgb(color.rgb) * alpha, alpha);
}
