import package::generated::util::{srgb, DEG2RAD};
import package::generated::effect::{effect_input, effect_source, effect_sampler, context, ZeriumVertex, vertex};
import package::generated::properties_linear_gradient::props;

fn decode_pair(value: vec4<f32>) -> vec2<u32> {
    let bytes = vec4<u32>(round(clamp(value, vec4(0.0), vec4(1.0)) * 255.0));
    return vec2<u32>(
        bytes.x | (bytes.y << 8u),
        bytes.z | (bytes.w << 8u),
    );
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_source, effect_sampler, input.uv);
    let output_size = context().output_size;
    let x_bounds = decode_pair(textureLoad(effect_input, vec2<i32>(0, 0), 0));
    if x_bounds.x == 0u {
        return vec4(0.0);
    }
    var y_bounds = vec2<u32>(1u);
    if output_size.x > 1.0 || output_size.y > 1.0 {
        let y_position = select(vec2<i32>(0, 1), vec2<i32>(1, 0), output_size.x > 1.0);
        y_bounds = decode_pair(textureLoad(effect_input, y_position, 0));
    }
    let minimum = vec2<f32>(f32(x_bounds.x - 1u), f32(y_bounds.x - 1u));
    let maximum = vec2<f32>(f32(x_bounds.y), f32(y_bounds.y));
    let dimensions = maximum - minimum;
    let center = (minimum + maximum) * 0.5;
    let point = input.uv * output_size;
    let angle = properties.angle * DEG2RAD;
    let direction = vec2(cos(angle), sin(angle));
    let half_span = max(dot(abs(direction), dimensions) * 0.5, 0.00001);
    let amount = clamp(dot(point - center, direction) / (half_span * 2.0) + 0.5, 0.0, 1.0);
    let color = mix(properties.start_color, properties.end_color, amount);
    let alpha = source.a * color.a;
    return vec4(srgb(color.rgb) * alpha, alpha);
}
