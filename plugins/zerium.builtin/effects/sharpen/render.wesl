import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input, straight};
import package::generated::sharpen::props;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    if p.radius <= 0.0 || p.amount <= 0.0 || source.a <= 1e-5 {
        return source;
    }
    var blur = vec4(0.0);
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let weight = select(1.0, 2.0, x == 0) * select(1.0, 2.0, y == 0);
            blur += sample_input(position + vec2<f32>(f32(x), f32(y)) * p.radius) * weight;
        }
    }
    let rgb = max(straight(source) + (straight(source) - straight(blur)) * p.amount / 100.0, vec3(0.0));
    return vec4(rgb * source.a, source.a);
}
