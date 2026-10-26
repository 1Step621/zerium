import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_info, input_position, sample_input};
import package::generated::bend::props;
import package::generated::support::util::DEG2RAD;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    if abs(p.angle) < 1e-5 {
        return source;
    }
    let center = input_position(vec2(0.5));
    var local = position - center;
    var size = input_info().bounds_size;
    if p.axis == 1u {
        local = local.yx;
        size = size.yx;
    }
    let curvature = p.angle * DEG2RAD / max(size.x, 1e-5);
    let sine = local.x * curvature;
    if abs(sine) >= 1.0 {
        return vec4(0.0);
    }
    let x = asin(sine) / curvature;
    let y = local.y - (1.0 - sqrt(max(1.0 - sine * sine, 0.0))) / curvature;
    var mapped = vec2(x, y);
    if p.axis == 1u {
        mapped = mapped.yx;
    }
    return sample_input(center + mapped);
}
