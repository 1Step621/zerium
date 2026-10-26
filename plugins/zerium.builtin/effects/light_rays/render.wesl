import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input};
import package::generated::light_rays::props;
import package::helpers::light::{highlight, add_light};

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    if p.length <= 0.0 || p.amount <= 0.0 {
        return source;
    }
    let delta = position - vec2(p.center.v0, p.center.v1);
    let distance = length(delta);
    let direction = delta / max(distance, 1e-5);
    var light = vec4(0.0);
    var weight = 0.0;
    for (var index = 0u; index < 48u; index += 1u) {
        let t = f32(index) / 47.0;
        let w = exp(-3.0 * t);
        light += highlight(sample_input(position - direction * min(p.length, distance) * t), p.threshold) * w;
        weight += w;
    }
    return add_light(source, light / weight, p.amount / 100.0);
}
