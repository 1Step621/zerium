import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, over, sample_input};
import package::generated::support::util::{DEG2RAD, scene_color};
import package::generated::long_shadow::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);

    if properties.length <= 0.0 {
        return source;
    }

    let direction = vec2(cos(properties.angle * DEG2RAD), sin(properties.angle * DEG2RAD));
    var alpha = 0.0;

    for(var tap = 0u; tap <= properties.samples; tap += 1u) {
        let fraction = f32(tap) / f32(properties.samples);
        let uv = position - direction * properties.length * fraction;
        let coverage = sample_input(uv).a * select(1.0, 1.0 - fraction, properties.fade);
        alpha = max(alpha, coverage);
    }

    return over(source, scene_color(properties.color) * alpha);
}
