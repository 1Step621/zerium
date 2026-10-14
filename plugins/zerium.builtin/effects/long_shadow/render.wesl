import package::generated::host::effect::{
    ZeriumVertex,
    over,
    position_to_uv,
    sample_or_transparent,
    uv_to_position,
    vertex,
};
import package::generated::host::util::{DEG2RAD, scene_color};
import package::generated::long_shadow::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let position = uv_to_position(input.uv);
    let source = sample_or_transparent(position_to_uv(position));
    if properties.length <= 0.0 {
        return source;
    }
    let direction = vec2(cos(properties.angle * DEG2RAD), sin(properties.angle * DEG2RAD));
    var alpha = 0.0;
    for (var tap = 0u; tap <= properties.samples; tap += 1u) {
        let fraction = f32(tap) / f32(properties.samples);
        let uv = position_to_uv(position - direction * properties.length * fraction);
        let coverage = sample_or_transparent(uv).a * select(1.0, 1.0 - fraction, properties.fade);
        alpha = max(alpha, coverage);
    }
    return over(source, scene_color(properties.color) * alpha);
}
