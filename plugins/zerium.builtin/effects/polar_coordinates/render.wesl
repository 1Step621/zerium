import package::generated::host::effect::{
    ZeriumVertex,
    effect_input,
    effect_sampler,
    input_uv_to_position,
    position_to_uv,
    sample_or_transparent,
    uv_to_position,
    vertex,
};
import package::generated::host::util::DEG2RAD;
import package::generated::polar_coordinates::props;
import package::shared::geometry::TAU;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let size = input_uv_to_position(vec2(1.0)) - input_uv_to_position(vec2(0.0));
    let center = input_uv_to_position(vec2(0.5));
    let radius = min(size.x, size.y) * 0.5;
    let original = position_to_uv(uv_to_position(input.uv));
    var mapped = original;
    if properties.direction == 0u {
        let local = (uv_to_position(input.uv) - center) / radius;
        mapped = vec2(fract(atan2(local.y, local.x) / TAU - properties.angle / 360.0 + 1.0), length(local));
    } else {
        let angle = original.x * TAU + properties.angle * DEG2RAD;
        let point = center + vec2(cos(angle), sin(angle)) * original.y * radius;
        mapped = position_to_uv(point);
    }
    let amount = properties.amount / 100.0;
    let source = textureSample(effect_input, effect_sampler, original);
    return mix(source, sample_or_transparent(mapped), amount);
}
