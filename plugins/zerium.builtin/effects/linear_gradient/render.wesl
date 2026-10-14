import package::generated::host::effect::{
    ZeriumVertex,
    context,
    effect_sampler,
    effect_source,
    vertex,
};
import package::generated::host::util::{DEG2RAD, srgb};
import package::generated::linear_gradient::{get_stops, props};
import package::shared::gradient::{gradient_factor, gradient_include, gradient_start};

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
    let position = amount * 100.0;
    let first = get_stops(properties, 0u);
    var segment = gradient_start(first.v0, first.v1);
    for (var index = 1u; index < properties.stops_len; index += 1u) {
        let stop = get_stops(properties, index);
        segment = gradient_include(segment, stop.v0, stop.v1, position);
    }
    let color = mix(segment.start_color, segment.end_color, gradient_factor(segment, position));
    let alpha = source.a * color.a;
    return vec4(srgb(color.rgb) * alpha, alpha);
}
