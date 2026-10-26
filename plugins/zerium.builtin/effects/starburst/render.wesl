import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input};
import package::generated::starburst::props;
import package::generated::support::util::DEG2RAD;
import package::helpers::geometry::TAU;
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
    var light = vec4(0.0);
    var weight = 0.0;
    for (var ray = 0u; ray < p.rays; ray += 1u) {
        let angle = p.angle * DEG2RAD + f32(ray) * TAU / f32(p.rays);
        let direction = vec2(cos(angle), sin(angle));
        for (var index = 1u; index <= 24u; index += 1u) {
            let t = f32(index) / 24.0;
            let w = (1.0 - t) * (1.0 - t);
            light += highlight(sample_input(position - direction * p.length * t), p.threshold) * w;
            weight += w;
        }
    }
    return add_light(source, light / max(weight, 1e-5), p.amount / 100.0);
}
