import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input, straight};
import package::generated::alpha_morphology::props;

override direction_x: f32 = 1.0;

override direction_y: f32 = 0.0;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    if p.radius == 0.0 {
        return source;
    }
    let distance = abs(p.radius);
    var chosen = source;
    let extent = i32(ceil(distance));
    for (var index = -extent; index <= extent; index += 1) {
        let offset = clamp(f32(index), -distance, distance);
        let candidate = sample_input(position + vec2(direction_x, direction_y) * offset);
        if (p.radius > 0.0 && candidate.a > chosen.a) || (p.radius < 0.0 && candidate.a < chosen.a) {
            chosen = candidate;
        }
    }
    if p.radius < 0.0 {
        return vec4(straight(source) * chosen.a, chosen.a);
    }
    return chosen;
}
