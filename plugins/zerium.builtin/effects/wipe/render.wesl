import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_info, input_position, sample_input};
import package::generated::wipe::props;
import package::generated::support::util::DEG2RAD;
import package::helpers::geometry::TAU;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    if p.progress <= 0.0 {
        return vec4(0.0);
    }
    if p.progress >= 100.0 {
        return source;
    }
    let size = input_info().bounds_size;
    let center = input_position(vec2(0.5));
    let local = position - center;
    let direction = vec2(cos(p.angle * DEG2RAD), sin(p.angle * DEG2RAD));
    var value = dot(local, direction) / max(dot(abs(direction), size), 1e-5) + 0.5;
    if p.mode == 1u {
        value = length(local) / max(length(size * 0.5), 1e-5);
    } else if p.mode == 2u {
        value = fract((atan2(local.y, local.x) - p.angle * DEG2RAD) / TAU + 0.25);
    }
    if p.invert {
        value = 1.0 - value;
    }
    let soft = max(p.softness / 100.0, 1e-5);
    let edge = p.progress / 100.0 * (1.0 + 2.0 * soft) - soft;
    return source * (1.0 - smoothstep(edge - soft, edge + soft, value));
}
